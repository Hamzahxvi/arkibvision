#![forbid(unsafe_code)]

use crate::config::{
    AUTO_GALLERY_MIN_SCORE, AUTO_GALLERY_MIN_SCORE_MOBILEFACENET, AppConfig,
    MIN_CLIP_IDENTIFY_THRESHOLD, MIN_MOBILEFACENET_IDENTIFY_THRESHOLD,
};
use crate::vector_store::normalize_l2;
use crate::vision_ops::{encode_jpeg, load_rgb_image, load_rgb_image_from_bytes};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;

const YUNET_INPUT_SIZE: u32 = 640;
const YUNET_SCORE_THRESHOLD: f32 = 0.70;
const YUNET_NMS_THRESHOLD: f32 = 0.30;
const MIN_EMBED_SIZE: i32 = 40;
const ARCFACE_SIZE: u32 = 112;
const CLIP_MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
const CLIP_STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];

/// Standard InsightFace / ArcFace 112×112 5-point facial landmark template:
/// `[left_eye, right_eye, nose_tip, left_mouth, right_mouth]` (viewer's perspective).
pub const ARCFACE_REF_112: [(f32, f32); 5] = [
    (38.2946, 51.6963),
    (73.5318, 51.5014),
    (56.0252, 71.7366),
    (41.5493, 92.3655),
    (70.7299, 92.2041),
];

#[derive(Clone, Copy, Debug)]
struct RawFaceDetection {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    score: f32,
    landmarks: Option<[(f32, f32); 5]>,
}

/// 2D 4-DOF similarity transform (`scale + rotation + translation`):
/// `x' = a*x - b*y + tx`, `y' = b*x + a*y + ty`.
#[derive(Clone, Copy, Debug)]
pub struct Similarity2D {
    pub a: f32,
    pub b: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Similarity2D {
    /// Estimates the least-squares 4-DOF Umeyama similarity transform mapping `src` points
    /// onto `dst` points (`ARCFACE_REF_112`). Returns `None` if source points are degenerate.
    pub fn estimate(src: &[(f32, f32); 5], dst: &[(f32, f32); 5]) -> Option<Self> {
        let n = 5.0_f64;
        let (mut msx, mut msy, mut mdx, mut mdy) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
        for i in 0..5 {
            msx += f64::from(src[i].0);
            msy += f64::from(src[i].1);
            mdx += f64::from(dst[i].0);
            mdy += f64::from(dst[i].1);
        }
        msx /= n;
        msy /= n;
        mdx /= n;
        mdy /= n;

        let mut var_s = 0.0_f64;
        let mut num_a = 0.0_f64;
        let mut num_b = 0.0_f64;
        for i in 0..5 {
            let sx = f64::from(src[i].0) - msx;
            let sy = f64::from(src[i].1) - msy;
            let dx = f64::from(dst[i].0) - mdx;
            let dy = f64::from(dst[i].1) - mdy;
            var_s += sx * sx + sy * sy;
            num_a += sx * dx + sy * dy;
            num_b += sx * dy - sy * dx;
        }
        if var_s <= 1e-6 {
            return None;
        }
        let a = num_a / var_s;
        let b = num_b / var_s;
        if (a * a + b * b) <= 1e-8 {
            return None;
        }
        let tx = mdx - (a * msx - b * msy);
        let ty = mdy - (b * msx + a * msy);
        Some(Self {
            a: a as f32,
            b: b as f32,
            tx: tx as f32,
            ty: ty as f32,
        })
    }

    #[cfg(test)]
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x - self.b * y + self.tx,
            self.b * x + self.a * y + self.ty,
        )
    }

    /// Maps destination coordinate `(dx, dy)` back to source image coordinate `(sx, sy)`.
    pub fn invert_apply(&self, dx: f32, dy: f32) -> (f32, f32) {
        let det = (self.a * self.a + self.b * self.b).max(1e-8);
        let ux = dx - self.tx;
        let uy = dy - self.ty;
        let sx = (self.a * ux + self.b * uy) / det;
        let sy = (-self.b * ux + self.a * uy) / det;
        (sx, sy)
    }
}

/// Bilinear-warps `src` into a landmark-aligned `112×112` RGB face image using `sim`.
pub fn warp_face_112(src: &RgbImage, sim: &Similarity2D) -> RgbImage {
    let (sw, sh) = src.dimensions();
    let max_x = sw.saturating_sub(1) as i32;
    let max_y = sh.saturating_sub(1) as i32;
    RgbImage::from_fn(ARCFACE_SIZE, ARCFACE_SIZE, |dx, dy| {
        let (sx, sy) = sim.invert_apply(dx as f32, dy as f32);
        if sx < -0.5 || sy < -0.5 || sx > (sw as f32) - 0.5 || sy > (sh as f32) - 0.5 {
            return Rgb([0, 0, 0]);
        }
        let x0 = (sx.floor() as i32).clamp(0, max_x) as u32;
        let y0 = (sy.floor() as i32).clamp(0, max_y) as u32;
        let x1 = (x0 as i32 + 1).clamp(0, max_x) as u32;
        let y1 = (y0 as i32 + 1).clamp(0, max_y) as u32;
        let fx = (sx - sx.floor()).clamp(0.0, 1.0);
        let fy = (sy - sy.floor()).clamp(0.0, 1.0);

        let p00 = src.get_pixel(x0, y0).0;
        let p10 = src.get_pixel(x1, y0).0;
        let p01 = src.get_pixel(x0, y1).0;
        let p11 = src.get_pixel(x1, y1).0;
        let mut out = [0u8; 3];
        for ch in 0..3 {
            let top = f32::from(p00[ch]) * (1.0 - fx) + f32::from(p10[ch]) * fx;
            let bot = f32::from(p01[ch]) * (1.0 - fx) + f32::from(p11[ch]) * fx;
            let val = top * (1.0 - fy) + bot * fy;
            out[ch] = val.round().clamp(0.0, 255.0) as u8;
        }
        Rgb(out)
    })
}

#[derive(Clone, Debug)]
pub struct DetectedFace {
    pub box_xywh: [i32; 4],
    pub confidence: f32,
    pub embedding: Vec<f32>,
    pub face_crop_jpeg: Vec<u8>,
}

pub struct FaceEngine {
    _env: ort::environment::Environment,
    yunet: Mutex<Session>,
    mobilefacenet: Option<(Mutex<Session>, String)>,
    clip: Mutex<Session>,
}

impl FaceEngine {
    pub fn load(cfg: &AppConfig) -> Result<Self, String> {
        if !cfg.ort_dylib_path.is_file() {
            return Err(format!(
                "ONNX Runtime library not found at {}. Set ORT_DYLIB_PATH or place onnxruntime.dll in models/runtime/.",
                cfg.ort_dylib_path.display()
            ));
        }
        if !cfg.yunet_model_path.is_file() {
            return Err(format!(
                "YuNet ONNX model not found at {}.",
                cfg.yunet_model_path.display()
            ));
        }
        if !cfg.clip_model_path.is_file() {
            return Err(format!(
                "CLIP ViT-B/32 ONNX model not found at {}.",
                cfg.clip_model_path.display()
            ));
        }

        let env = ort::init_from(&cfg.ort_dylib_path)
            .map_err(|e| {
                format!(
                    "Failed initializing ONNX Runtime from {}: {e}",
                    cfg.ort_dylib_path.display()
                )
            })?
            .build()
            .map_err(|e| format!("Failed building ONNX Runtime environment: {e}"))?;

        let threads = std::thread::available_parallelism()
            .map(|n| n.get().clamp(2, 4))
            .unwrap_or(4);

        let yunet = Session::builder(&env)
            .map_err(|e| format!("YuNet session builder error: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| format!("YuNet optimization error: {e}"))?
            .with_intra_threads(threads)
            .map_err(|e| format!("YuNet threads error: {e}"))?
            .commit_from_file(&cfg.yunet_model_path)
            .map_err(|e| format!("Failed loading YuNet model: {e}"))?;

        let mobilefacenet = if cfg.mobilefacenet_model_path.is_file() {
            let sess = Session::builder(&env)
                .map_err(|e| format!("MobileFaceNet session builder error: {e}"))?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|e| format!("MobileFaceNet optimization error: {e}"))?
                .with_intra_threads(threads)
                .map_err(|e| format!("MobileFaceNet threads error: {e}"))?
                .commit_from_file(&cfg.mobilefacenet_model_path)
                .map_err(|e| format!("Failed loading MobileFaceNet model: {e}"))?;
            let input_name = sess
                .inputs()
                .first()
                .map(|i| i.name().to_string())
                .unwrap_or_else(|| "input.1".to_string());
            Some((Mutex::new(sess), input_name))
        } else {
            None
        };

        let clip = Session::builder(&env)
            .map_err(|e| format!("CLIP session builder error: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| format!("CLIP optimization error: {e}"))?
            .with_intra_threads(threads)
            .map_err(|e| format!("CLIP threads error: {e}"))?
            .commit_from_file(&cfg.clip_model_path)
            .map_err(|e| format!("Failed loading CLIP model: {e}"))?;

        Ok(Self {
            _env: env,
            yunet: Mutex::new(yunet),
            mobilefacenet,
            clip: Mutex::new(clip),
        })
    }

    pub fn face_model_name(&self) -> &'static str {
        if self.mobilefacenet.is_some() {
            "mobilefacenet_512"
        } else {
            "clip_vit_b32_768"
        }
    }

    pub fn face_embedding_dim(&self) -> usize {
        if self.mobilefacenet.is_some() {
            512
        } else {
            768
        }
    }

    pub fn default_identify_threshold(&self) -> f32 {
        if self.mobilefacenet.is_some() {
            MIN_MOBILEFACENET_IDENTIFY_THRESHOLD
        } else {
            MIN_CLIP_IDENTIFY_THRESHOLD
        }
    }

    pub fn auto_gallery_threshold(&self) -> f32 {
        if self.mobilefacenet.is_some() {
            AUTO_GALLERY_MIN_SCORE_MOBILEFACENET
        } else {
            AUTO_GALLERY_MIN_SCORE
        }
    }

    pub fn extract_features_from_path(&self, path: &Path) -> Result<Vec<DetectedFace>, String> {
        let img = load_rgb_image(path)?;
        self.extract_features(&img)
    }

    pub fn extract_features_from_bytes(&self, bytes: &[u8]) -> Result<Vec<DetectedFace>, String> {
        let img = load_rgb_image_from_bytes(bytes)?;
        self.extract_features(&img)
    }

    /// Compute a 768-D CLIP ViT-B/32 embedding for a full photo (buildings, landmarks, archival locations).
    pub fn embed_full_image_from_path(&self, path: &Path) -> Result<Vec<f32>, String> {
        let img = load_rgb_image(path)?;
        self.embed_clip(&img)
    }

    pub fn primary_face_bytes(&self, path: &Path) -> Result<(Vec<u8>, String), String> {
        let img = load_rgb_image(path)?;
        let detections = self.extract_features(&img)?;
        let primary = detections
            .into_iter()
            .max_by_key(|d| i64::from(d.box_xywh[2].max(0)) * i64::from(d.box_xywh[3].max(0)))
            .ok_or_else(|| "No face was detected for enrollment.".to_string())?;
        let [x, y, fw, fh] = primary.box_xywh;
        let pad = ((fw.max(fh) as f32) * 0.35) as i32;
        let (iw, ih) = img.dimensions();
        let x1 = (x - pad).max(0) as u32;
        let y1 = (y - pad).max(0) as u32;
        let x2 = (x + fw + pad).min(iw as i32).max(x1 as i32 + 1) as u32;
        let y2 = (y + fh + pad).min(ih as i32).max(y1 as i32 + 1) as u32;
        let crop = img.crop_imm(x1, y1, x2 - x1, y2 - y1);
        let bytes = encode_jpeg(&crop, 95)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("face");
        Ok((bytes, format!("{stem}_primary.jpg")))
    }

    pub fn extract_features(&self, img: &DynamicImage) -> Result<Vec<DetectedFace>, String> {
        let (orig_w, orig_h) = img.dimensions();
        if orig_w == 0 || orig_h == 0 {
            return Ok(Vec::new());
        }

        let detections = self.detect_yunet(img)?;
        let rgb_full = if self.mobilefacenet.is_some() {
            Some(img.to_rgb8())
        } else {
            None
        };
        let mut results = Vec::with_capacity(detections.len());

        for det in detections {
            let x = det.x1.floor() as i32;
            let y = det.y1.floor() as i32;
            let fw = (det.x2 - det.x1).round() as i32;
            let fh = (det.y2 - det.y1).round() as i32;

            let x1 = x.max(0) as u32;
            let y1 = y.max(0) as u32;
            let x2 = (x + fw).min(orig_w as i32).max(0) as u32;
            let y2 = (y + fh).min(orig_h as i32).max(0) as u32;
            if x2 <= x1 || y2 <= y1 {
                continue;
            }

            let crop = img.crop_imm(x1, y1, x2 - x1, y2 - y1);
            let face_crop_jpeg = encode_jpeg(&crop, 92)?;
            let embedding = if fw.max(fh) >= MIN_EMBED_SIZE {
                if self.mobilefacenet.is_some() {
                    let aligned_112 = if let (Some(full_rgb), Some(kps)) =
                        (&rgb_full, det.landmarks)
                        && let Some(sim) = Similarity2D::estimate(&kps, &ARCFACE_REF_112)
                    {
                        warp_face_112(full_rgb, &sim)
                    } else {
                        crop.resize_exact(ARCFACE_SIZE, ARCFACE_SIZE, FilterType::CatmullRom)
                            .to_rgb8()
                    };
                    self.embed_mobilefacenet(&aligned_112)?
                } else {
                    self.embed_clip(&crop)?
                }
            } else {
                Vec::new()
            };

            results.push(DetectedFace {
                box_xywh: [x1 as i32, y1 as i32, fw, fh],
                confidence: det.score,
                embedding,
                face_crop_jpeg,
            });
        }
        Ok(results)
    }

    fn detect_yunet(&self, img: &DynamicImage) -> Result<Vec<RawFaceDetection>, String> {
        let (orig_w, orig_h) = img.dimensions();
        let resized = img
            .resize_exact(YUNET_INPUT_SIZE, YUNET_INPUT_SIZE, FilterType::Triangle)
            .to_rgb8();

        let sz = YUNET_INPUT_SIZE as usize;
        let plane = sz * sz;
        let mut input = vec![0.0_f32; 3 * plane];
        // OpenCV FaceDetectorYN expects BGR channels in [0, 255] NCHW.
        for y in 0..sz {
            for x in 0..sz {
                let p = resized.get_pixel(x as u32, y as u32);
                let idx = y * sz + x;
                input[idx] = f32::from(p[2]); // B
                input[plane + idx] = f32::from(p[1]); // G
                input[2 * plane + idx] = f32::from(p[0]); // R
            }
        }

        let tensor = Tensor::from_array(([1usize, 3, sz, sz], input))
            .map_err(|e| format!("YuNet input tensor error: {e}"))?;

        let mut yunet = self.yunet.lock().expect("yunet lock");
        let outputs = yunet
            .run(ort::inputs!["input" => tensor])
            .map_err(|e| format!("YuNet inference error: {e}"))?;

        let sx = (orig_w as f32) / (YUNET_INPUT_SIZE as f32);
        let sy = (orig_h as f32) / (YUNET_INPUT_SIZE as f32);
        let mut raw_boxes: Vec<RawFaceDetection> = Vec::new();

        for (stride, cls_name, obj_name, bbox_name, kps_name) in [
            (8usize, "cls_8", "obj_8", "bbox_8", "kps_8"),
            (16usize, "cls_16", "obj_16", "bbox_16", "kps_16"),
            (32usize, "cls_32", "obj_32", "bbox_32", "kps_32"),
        ] {
            let (_, cls_data) = outputs[cls_name]
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("YuNet {cls_name} extract error: {e}"))?;
            let (_, obj_data) = outputs[obj_name]
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("YuNet {obj_name} extract error: {e}"))?;
            let (_, bbox_data) = outputs[bbox_name]
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("YuNet {bbox_name} extract error: {e}"))?;
            let kps_data: Option<&[f32]> = outputs
                .get(kps_name)
                .and_then(|v| v.try_extract_tensor::<f32>().ok())
                .map(|(_, slice)| slice);

            let grid = sz / stride;
            let stride_f = stride as f32;
            for r in 0..grid {
                for c in 0..grid {
                    let idx = r * grid + c;
                    let cls = cls_data[idx].clamp(0.0, 1.0);
                    let obj = obj_data[idx].clamp(0.0, 1.0);
                    let score = (cls * obj).sqrt();
                    if score < YUNET_SCORE_THRESHOLD {
                        continue;
                    }
                    let b = idx * 4;
                    let cx = (c as f32 + bbox_data[b]) * stride_f;
                    let cy = (r as f32 + bbox_data[b + 1]) * stride_f;
                    let bw = bbox_data[b + 2].exp() * stride_f;
                    let bh = bbox_data[b + 3].exp() * stride_f;
                    let x1 = (cx - bw * 0.5) * sx;
                    let y1 = (cy - bh * 0.5) * sy;
                    let x2 = (cx + bw * 0.5) * sx;
                    let y2 = (cy + bh * 0.5) * sy;

                    let landmarks = kps_data.and_then(|kps| {
                        let k = idx * 10;
                        if k + 9 >= kps.len() {
                            return None;
                        }
                        let mut pts = [(0.0_f32, 0.0_f32); 5];
                        for i in 0..5 {
                            pts[i] = (
                                (c as f32 + kps[k + i * 2]) * stride_f * sx,
                                (r as f32 + kps[k + i * 2 + 1]) * stride_f * sy,
                            );
                        }
                        Some(pts)
                    });

                    raw_boxes.push(RawFaceDetection {
                        x1,
                        y1,
                        x2,
                        y2,
                        score,
                        landmarks,
                    });
                }
            }
        }

        Ok(nms(raw_boxes, YUNET_NMS_THRESHOLD))
    }

    fn embed_mobilefacenet(&self, aligned_112: &RgbImage) -> Result<Vec<f32>, String> {
        let Some((sess_mutex, input_name)) = &self.mobilefacenet else {
            return Err("MobileFaceNet session is not loaded.".to_string());
        };
        let sz = ARCFACE_SIZE as usize;
        let plane = sz * sz;
        let mut input = vec![0.0_f32; 3 * plane];
        // InsightFace MobileFaceNet (w600k_mbf.onnx): RGB normalized (pixel - 127.5) / 128.0 in NCHW.
        for y in 0..sz {
            for x in 0..sz {
                let p = aligned_112.get_pixel(x as u32, y as u32);
                let idx = y * sz + x;
                for ch in 0..3 {
                    input[ch * plane + idx] = (f32::from(p[ch]) - 127.5) / 128.0;
                }
            }
        }
        let tensor = Tensor::from_array(([1usize, 3, sz, sz], input))
            .map_err(|e| format!("MobileFaceNet input tensor error: {e}"))?;
        let mut sess = sess_mutex.lock().expect("mobilefacenet lock");
        let outputs = sess
            .run(ort::inputs![input_name.as_str() => tensor])
            .map_err(|e| format!("MobileFaceNet inference error: {e}"))?;
        let (_, emb) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("MobileFaceNet output extract error: {e}"))?;
        Ok(normalize_l2(emb))
    }

    fn embed_clip(&self, face: &DynamicImage) -> Result<Vec<f32>, String> {
        let padded = square_pad(&face.to_rgb8());
        let resized = DynamicImage::ImageRgb8(padded)
            .resize_exact(224, 224, FilterType::CatmullRom)
            .to_rgb8();

        let plane = 224 * 224;
        let mut input = vec![0.0_f32; 3 * plane];
        for y in 0..224usize {
            for x in 0..224usize {
                let p = resized.get_pixel(x as u32, y as u32);
                let idx = y * 224 + x;
                for ch in 0..3 {
                    let v = f32::from(p[ch]) / 255.0;
                    input[ch * plane + idx] = (v - CLIP_MEAN[ch]) / CLIP_STD[ch];
                }
            }
        }

        let tensor = Tensor::from_array(([1usize, 3, 224, 224], input))
            .map_err(|e| format!("CLIP input tensor error: {e}"))?;

        let mut clip = self.clip.lock().expect("clip lock");
        let outputs = clip
            .run(ort::inputs!["pixel_values" => tensor])
            .map_err(|e| format!("CLIP inference error: {e}"))?;
        let (_, emb) = outputs["embeddings"]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("CLIP output extract error: {e}"))?;
        Ok(emb.to_vec())
    }
}

fn square_pad(img: &RgbImage) -> RgbImage {
    let (w, h) = img.dimensions();
    if w == h {
        return img.clone();
    }
    let side = w.max(h);
    let mut canvas = RgbImage::from_pixel(side, side, Rgb([0, 0, 0]));
    let ox = (side - w) / 2;
    let oy = (side - h) / 2;
    for y in 0..h {
        for x in 0..w {
            canvas.put_pixel(x + ox, y + oy, *img.get_pixel(x, y));
        }
    }
    canvas
}

fn iou(a: &RawFaceDetection, b: &RawFaceDetection) -> f32 {
    let ix1 = a.x1.max(b.x1);
    let iy1 = a.y1.max(b.y1);
    let ix2 = a.x2.min(b.x2);
    let iy2 = a.y2.min(b.y2);
    let inter = (ix2 - ix1).max(0.0) * (iy2 - iy1).max(0.0);
    let area_a = (a.x2 - a.x1).max(0.0) * (a.y2 - a.y1).max(0.0);
    let area_b = (b.x2 - b.x1).max(0.0) * (b.y2 - b.y1).max(0.0);
    let denom = area_a + area_b - inter;
    if denom <= 0.0 { 0.0 } else { inter / denom }
}

fn nms(mut boxes: Vec<RawFaceDetection>, thresh: f32) -> Vec<RawFaceDetection> {
    boxes.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut keep = Vec::new();
    while let Some(best) = boxes.first().copied() {
        keep.push(best);
        boxes.remove(0);
        boxes.retain(|b| iou(&best, b) < thresh);
    }
    keep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_square_pad_and_nms() {
        let img = RgbImage::from_pixel(10, 4, Rgb([255, 0, 0]));
        let padded = square_pad(&img);
        assert_eq!(padded.dimensions(), (10, 10));

        let mk = |x1, y1, x2, y2, score| RawFaceDetection {
            x1,
            y1,
            x2,
            y2,
            score,
            landmarks: None,
        };
        let boxes = vec![
            mk(0.0, 0.0, 10.0, 10.0, 0.95),
            mk(1.0, 1.0, 11.0, 11.0, 0.80),
            mk(50.0, 50.0, 60.0, 60.0, 0.90),
        ];
        let kept = nms(boxes, 0.30);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn test_5point_similarity_affine_alignment_and_warp() {
        // Apply a known scale (2.0), rotation (+15 deg), and translation (+100, +40) to ARCFACE_REF_112
        let angle = 15.0_f32.to_radians();
        let scale = 2.0_f32;
        let ca = scale * angle.cos();
        let sa = scale * angle.sin();
        let mut src_pts = [(0.0_f32, 0.0_f32); 5];
        for i in 0..5 {
            let (rx, ry) = ARCFACE_REF_112[i];
            src_pts[i] = (ca * rx - sa * ry + 100.0, sa * rx + ca * ry + 40.0);
        }

        let sim = Similarity2D::estimate(&src_pts, &ARCFACE_REF_112).expect("similarity fit");
        for i in 0..5 {
            let (mx, my) = sim.apply(src_pts[i].0, src_pts[i].1);
            assert!((mx - ARCFACE_REF_112[i].0).abs() < 1e-3);
            assert!((my - ARCFACE_REF_112[i].1).abs() < 1e-3);
            // Round-trip inverse check
            let (ix, iy) = sim.invert_apply(mx, my);
            assert!((ix - src_pts[i].0).abs() < 1e-2);
            assert!((iy - src_pts[i].1).abs() < 1e-2);
        }

        let canvas = RgbImage::from_pixel(320, 320, Rgb([120, 130, 140]));
        let warped = warp_face_112(&canvas, &sim);
        assert_eq!(warped.dimensions(), (112, 112));
        // Center nose point should sample cleanly from the uniform canvas
        assert_eq!(warped.get_pixel(56, 72).0, [120, 130, 140]);
    }
}
