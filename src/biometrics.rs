#![forbid(unsafe_code)]

use crate::config::AppConfig;
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
const CLIP_MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
const CLIP_STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];

type RawFaceBox = (f32, f32, f32, f32, f32);

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
            clip: Mutex::new(clip),
        })
    }

    pub fn extract_features_from_path(&self, path: &Path) -> Result<Vec<DetectedFace>, String> {
        let img = load_rgb_image(path)?;
        self.extract_features(&img)
    }

    pub fn extract_features_from_bytes(&self, bytes: &[u8]) -> Result<Vec<DetectedFace>, String> {
        let img = load_rgb_image_from_bytes(bytes)?;
        self.extract_features(&img)
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

        let boxes = self.detect_yunet(img)?;
        let mut results = Vec::with_capacity(boxes.len());
        for (x1_f, y1_f, x2_f, y2_f, conf) in boxes {
            let x = x1_f.floor() as i32;
            let y = y1_f.floor() as i32;
            let fw = (x2_f - x1_f).round() as i32;
            let fh = (y2_f - y1_f).round() as i32;

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
                self.embed_clip(&crop)?
            } else {
                Vec::new()
            };

            results.push(DetectedFace {
                box_xywh: [x1 as i32, y1 as i32, fw, fh],
                confidence: conf,
                embedding,
                face_crop_jpeg,
            });
        }
        Ok(results)
    }

    fn detect_yunet(&self, img: &DynamicImage) -> Result<Vec<RawFaceBox>, String> {
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
        let mut raw_boxes: Vec<(f32, f32, f32, f32, f32)> = Vec::new();

        for (stride, cls_name, obj_name, bbox_name) in [
            (8usize, "cls_8", "obj_8", "bbox_8"),
            (16usize, "cls_16", "obj_16", "bbox_16"),
            (32usize, "cls_32", "obj_32", "bbox_32"),
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
                    raw_boxes.push((x1, y1, x2, y2, score));
                }
            }
        }

        Ok(nms(raw_boxes, YUNET_NMS_THRESHOLD))
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

fn iou(a: (f32, f32, f32, f32, f32), b: (f32, f32, f32, f32, f32)) -> f32 {
    let ix1 = a.0.max(b.0);
    let iy1 = a.1.max(b.1);
    let ix2 = a.2.min(b.2);
    let iy2 = a.3.min(b.3);
    let inter = (ix2 - ix1).max(0.0) * (iy2 - iy1).max(0.0);
    let area_a = (a.2 - a.0).max(0.0) * (a.3 - a.1).max(0.0);
    let area_b = (b.2 - b.0).max(0.0) * (b.3 - b.1).max(0.0);
    let denom = area_a + area_b - inter;
    if denom <= 0.0 { 0.0 } else { inter / denom }
}

fn nms(mut boxes: Vec<RawFaceBox>, thresh: f32) -> Vec<RawFaceBox> {
    boxes.sort_by(|a, b| b.4.total_cmp(&a.4));
    let mut kept: Vec<RawFaceBox> = Vec::new();
    for cand in boxes {
        if kept.iter().all(|&k| iou(k, cand) <= thresh) {
            kept.push(cand);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_square_pad_and_nms() {
        let rect = RgbImage::from_pixel(40, 20, Rgb([128, 64, 32]));
        let sq = square_pad(&rect);
        assert_eq!(sq.dimensions(), (40, 40));
        assert_eq!(*sq.get_pixel(0, 0), Rgb([0, 0, 0]));
        assert_eq!(*sq.get_pixel(20, 20), Rgb([128, 64, 32]));

        let boxes = vec![
            (10.0, 10.0, 50.0, 50.0, 0.95),
            (12.0, 12.0, 52.0, 52.0, 0.85), // overlapping -> suppressed
            (100.0, 100.0, 140.0, 140.0, 0.90),
        ];
        let kept = nms(boxes, 0.30);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].4 - 0.95).abs() < 1e-6);
        assert!((kept[1].4 - 0.90).abs() < 1e-6);
    }
}
