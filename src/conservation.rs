#![forbid(unsafe_code)]

//! Archival Image Quality Analysis, Damage Classification & Restoration Engine (Pure Safe Rust):
//! - Image Quality: Resolution, Lighting/Exposure, Readability/Sharpness
//! - Archival Damage Detection: Tear, Faded, Mold, Aging (classified as none / light / medium / heavy)
//! - Physical Conservation / Healing Recommendations (fungal removal, controlled drying, coating, Kozo mending, deacidification)
//! - Digital Image Quality Enhancement (de-yellowing, exposure correction, autocontrast, unsharp mask)

use crate::config::AppConfig;
use crate::metadata::profile_slug;
use crate::vision_ops::{encode_jpeg, load_rgb_image};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb, RgbImage};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageLevel {
    None,
    Light,
    Medium,
    Heavy,
}

impl DamageLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Light => "light",
            Self::Medium => "medium",
            Self::Heavy => "heavy",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Light => 1,
            Self::Medium => 2,
            Self::Heavy => 3,
        }
    }

    fn from_score(score: f64, light: f64, medium: f64, heavy: f64) -> Self {
        if score >= heavy {
            Self::Heavy
        } else if score >= medium {
            Self::Medium
        } else if score >= light {
            Self::Light
        } else {
            Self::None
        }
    }
}

#[derive(Clone, Debug)]
struct PixelStats {
    width: u32,
    height: u32,
    mean_luma: f64,
    std_luma: f64,
    p5_luma: f64,
    p95_luma: f64,
    dark_clip_ratio: f64,
    bright_clip_ratio: f64,
    laplacian_sharpness: f64,
    faded_score: f64,
    aging_score: f64,
    mold_score: f64,
    tear_score: f64,
}

fn compute_pixel_stats(img: &DynamicImage) -> PixelStats {
    let (orig_w, orig_h) = img.dimensions();
    // Analyze on a normalized working canvas (up to 640px) for fast, consistent signal scaling.
    let work = if orig_w.max(orig_h) > 640 {
        img.thumbnail(640, 640).to_rgb8()
    } else {
        img.to_rgb8()
    };
    let (w, h) = work.dimensions();
    let total = ((w as usize) * (h as usize)).max(1);

    let mut luma = vec![0.0_f64; total];
    let mut hist = [0usize; 256];
    let mut sum_luma = 0.0_f64;
    let mut dark_clip = 0usize;
    let mut bright_clip = 0usize;
    let mut warm_pixels = 0usize;
    let mut mold_pixels = 0usize;
    let mut sum_sat = 0.0_f64;
    let mut sum_rb_diff = 0.0_f64;

    for y in 0..h {
        for x in 0..w {
            let p = work.get_pixel(x, y);
            let r = f64::from(p[0]);
            let g = f64::from(p[1]);
            let b = f64::from(p[2]);
            let y_val = 0.299 * r + 0.587 * g + 0.114 * b;
            let idx = (y as usize) * (w as usize) + (x as usize);
            luma[idx] = y_val;
            hist[(y_val.round().clamp(0.0, 255.0)) as usize] += 1;
            sum_luma += y_val;
            if y_val < 25.0 {
                dark_clip += 1;
            } else if y_val > 235.0 {
                bright_clip += 1;
            }

            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);
            let sat = if max_c > 0.0 {
                (max_c - min_c) / max_c
            } else {
                0.0
            };
            sum_sat += sat;
            sum_rb_diff += (r - b).max(0.0);

            // Aging / yellowing / foxing cast: warm yellowish-brown paper tones.
            if r > g + 8.0 && g > b + 8.0 && y_val > 85.0 && y_val < 235.0 {
                warm_pixels += 1;
            }
            // Mold / fungal spotting: dark greenish-gray or irregular olive/brown blotches.
            if (22.0..=115.0).contains(&y_val)
                && ((g >= r && g > b + 6.0) || (r > b + 12.0 && g > b + 8.0 && y_val < 95.0))
            {
                mold_pixels += 1;
            }
        }
    }

    let mean_luma = sum_luma / (total as f64);
    let var_luma: f64 = luma
        .iter()
        .map(|&v| {
            let d = v - mean_luma;
            d * d
        })
        .sum::<f64>()
        / (total as f64);
    let std_luma = var_luma.sqrt();

    let percentile = |pct: f64| -> f64 {
        let target = ((total as f64) * pct) as usize;
        let mut acc = 0usize;
        for (i, &cnt) in hist.iter().enumerate() {
            acc += cnt;
            if acc >= target {
                return i as f64;
            }
        }
        255.0
    };
    let p5_luma = percentile(0.05);
    let p95_luma = percentile(0.95);
    let dyn_range = (p95_luma - p5_luma).max(0.0);

    // Laplacian energy for sharpness / text readability + border/interior tear discontinuity.
    let mut lap_sum = 0.0_f64;
    let mut lap_sq_sum = 0.0_f64;
    let mut lap_cnt = 0usize;
    let mut border_void_edges = 0usize;
    let mut border_pixels = 0usize;
    let mut interior_void_edges = 0usize;

    let wu = w as usize;
    let hu = h as usize;
    if wu >= 4 && hu >= 4 {
        for y in 1..(hu - 1) {
            for x in 1..(wu - 1) {
                let idx = y * wu + x;
                let c = luma[idx];
                let lap =
                    (4.0 * c - luma[idx - 1] - luma[idx + 1] - luma[idx - wu] - luma[idx + wu])
                        .abs();
                lap_sum += lap;
                lap_sq_sum += lap * lap;
                lap_cnt += 1;

                let is_border = x < (wu / 10).max(2)
                    || x >= wu - (wu / 10).max(2)
                    || y < (hu / 10).max(2)
                    || y >= hu - (hu / 10).max(2);
                let is_abrupt_void = (c < 22.0 || c > 246.0)
                    && ((c - luma[idx - 1]).abs() > 85.0
                        || (c - luma[idx + 1]).abs() > 85.0
                        || (c - luma[idx - wu]).abs() > 85.0
                        || (c - luma[idx + wu]).abs() > 85.0);
                if is_border {
                    border_pixels += 1;
                    if is_abrupt_void {
                        border_void_edges += 1;
                    }
                } else if is_abrupt_void {
                    interior_void_edges += 1;
                }
            }
        }
    }
    let lap_mean = if lap_cnt > 0 {
        lap_sum / (lap_cnt as f64)
    } else {
        0.0
    };
    let lap_var = if lap_cnt > 0 {
        (lap_sq_sum / (lap_cnt as f64) - lap_mean * lap_mean).max(0.0)
    } else {
        0.0
    };
    let laplacian_sharpness = lap_var.sqrt();

    let mean_sat = sum_sat / (total as f64);
    let mean_rb = sum_rb_diff / (total as f64);
    let warm_ratio = (warm_pixels as f64) / (total as f64);
    let mold_ratio = (mold_pixels as f64) / (total as f64);
    let border_tear_ratio = if border_pixels > 0 {
        (border_void_edges as f64) / (border_pixels as f64)
    } else {
        0.0
    };
    let interior_tear_ratio = (interior_void_edges as f64) / (total as f64);

    // Normalized 0..1 damage scores calibrated for historical documents & photographs.
    let faded_score =
        (1.0 - (dyn_range / 170.0).min(1.0)) * 0.65 + (1.0 - (std_luma / 52.0).min(1.0)) * 0.35;
    let aging_score = (warm_ratio * 1.45).min(1.0) * 0.65
        + ((mean_rb / 45.0).min(1.0)) * 0.35
        + if mean_sat < 0.08 { -0.15 } else { 0.0 };
    let aging_score = aging_score.clamp(0.0, 1.0);
    let mold_score = (mold_ratio * 3.8).clamp(0.0, 1.0);
    let tear_score = ((border_tear_ratio * 14.0) + (interior_tear_ratio * 22.0)).clamp(0.0, 1.0);

    PixelStats {
        width: orig_w,
        height: orig_h,
        mean_luma,
        std_luma,
        p5_luma,
        p95_luma,
        dark_clip_ratio: (dark_clip as f64) / (total as f64),
        bright_clip_ratio: (bright_clip as f64) / (total as f64),
        laplacian_sharpness,
        faded_score: faded_score.clamp(0.0, 1.0),
        aging_score,
        mold_score,
        tear_score,
    }
}

fn quality_scores_from_stats(s: &PixelStats) -> Value {
    // Resolution score (0..100)
    let min_dim = s.width.min(s.height) as f64;
    let megapixels = (f64::from(s.width) * f64::from(s.height)) / 1_000_000.0;
    let res_score = ((min_dim / 1200.0).min(1.0) * 70.0 + (megapixels / 2.0).min(1.0) * 30.0)
        .round()
        .clamp(0.0, 100.0);
    let res_grade = if res_score >= 80.0 {
        "ARCHIVAL_HIGH"
    } else if res_score >= 55.0 {
        "GOOD"
    } else if res_score >= 30.0 {
        "FAIR"
    } else {
        "LOW_RESOLUTION"
    };

    // Lighting / Exposure score (0..100)
    let exposure_penalty = ((s.mean_luma - 135.0).abs() / 115.0).min(1.0) * 45.0;
    let clip_penalty = ((s.dark_clip_ratio + s.bright_clip_ratio) * 100.0).min(35.0);
    let contrast_bonus = (s.std_luma / 55.0).min(1.0) * 20.0;
    let light_score = (80.0 - exposure_penalty - clip_penalty + contrast_bonus)
        .round()
        .clamp(0.0, 100.0);
    let light_condition = if s.mean_luma < 65.0 {
        "UNDEREXPOSED_DARK"
    } else if s.mean_luma > 215.0 {
        "OVEREXPOSED_WASHED_OUT"
    } else if s.std_luma < 24.0 {
        "LOW_CONTRAST_FLAT"
    } else {
        "BALANCED"
    };

    // Readability / Sharpness score (0..100)
    let sharp_component = (s.laplacian_sharpness / 36.0).min(1.0) * 65.0;
    let range_component = ((s.p95_luma - s.p5_luma) / 180.0).min(1.0) * 35.0;
    let read_score = (sharp_component + range_component)
        .round()
        .clamp(0.0, 100.0);
    let read_grade = if read_score >= 75.0 {
        "HIGH_READABILITY"
    } else if read_score >= 50.0 {
        "MODERATE_READABILITY"
    } else {
        "LOW_READABILITY"
    };

    let overall = ((res_score * 0.30) + (light_score * 0.35) + (read_score * 0.35)).round();
    let overall_grade = if overall >= 80.0 {
        "EXCELLENT"
    } else if overall >= 62.0 {
        "GOOD"
    } else if overall >= 42.0 {
        "FAIR"
    } else {
        "POOR"
    };

    json!({
        "overall_score": overall,
        "overall_grade": overall_grade,
        "resolution": {
            "width": s.width,
            "height": s.height,
            "megapixels": (megapixels * 100.0).round() / 100.0,
            "score": res_score,
            "grade": res_grade,
        },
        "lighting": {
            "mean_luminance": (s.mean_luma * 10.0).round() / 10.0,
            "contrast_stddev": (s.std_luma * 10.0).round() / 10.0,
            "dark_clipped_pct": (s.dark_clip_ratio * 1000.0).round() / 10.0,
            "bright_clipped_pct": (s.bright_clip_ratio * 1000.0).round() / 10.0,
            "score": light_score,
            "condition": light_condition,
        },
        "readability": {
            "sharpness_index": (s.laplacian_sharpness * 10.0).round() / 10.0,
            "dynamic_range": (s.p95_luma - s.p5_luma).round(),
            "score": read_score,
            "grade": read_grade,
        },
    })
}

/// Full Image Quality Analysis + Archival Damage Classification (light/medium/heavy) + Conservation/Healing Suggestions.
pub fn analyze_image_quality_and_conservation(path: &Path) -> Result<Value, String> {
    let img = load_rgb_image(path)?;
    let s = compute_pixel_stats(&img);
    let quality = quality_scores_from_stats(&s);

    let faded_lvl = DamageLevel::from_score(s.faded_score, 0.28, 0.48, 0.68);
    let aging_lvl = DamageLevel::from_score(s.aging_score, 0.22, 0.44, 0.66);
    let mold_lvl = DamageLevel::from_score(s.mold_score, 0.14, 0.34, 0.58);
    let tear_lvl = DamageLevel::from_score(s.tear_score, 0.16, 0.36, 0.60);

    let max_rank = faded_lvl
        .rank()
        .max(aging_lvl.rank())
        .max(mold_lvl.rank())
        .max(tear_lvl.rank());
    let overall_damage_level = match max_rank {
        3 => "heavy",
        2 => "medium",
        1 => "light",
        _ => "none",
    };

    let mut healing_suggestions: Vec<Value> = Vec::new();
    if mold_lvl != DamageLevel::None {
        healing_suggestions.push(json!({
            "issue": "mold",
            "severity": mold_lvl.as_str(),
            "treatment": "Fungal Removal & Controlled Drying",
            "actions": [
                "Isolate/quarantine the item immediately from healthy archival holdings.",
                "Perform controlled drying in a low-humidity chamber (45–50% RH, 18–20°C) until spores become dormant.",
                "Use a soft brush with HEPA-filtered archival micro-vacuum for dry fungal removal.",
                if mold_lvl == DamageLevel::Heavy {
                    "Apply conservator-supervised ethanol/antifungal vapor sterilization and consolidate weakened paper fibers."
                } else {
                    "Re-house in acid-free buffered enclosure with silica moisture monitoring."
                },
            ],
        }));
    }
    if tear_lvl != DamageLevel::None {
        healing_suggestions.push(json!({
            "issue": "tear",
            "severity": tear_lvl.as_str(),
            "treatment": "Physical Mending & Structural Support",
            "actions": [
                "Flatten gently under controlled humidification and archival blotters.",
                "Reinforce tears using reversible Japanese Kozo tissue and wheat-starch paste.",
                if tear_lvl == DamageLevel::Heavy {
                    "Perform full archival lining or polyester (Melinex/Mylar) encapsulation on rigid acid-free board."
                } else {
                    "Store flat in an archival polyester sleeve to prevent edge propagation."
                },
            ],
        }));
    }
    if aging_lvl != DamageLevel::None {
        healing_suggestions.push(json!({
            "issue": "aging",
            "severity": aging_lvl.as_str(),
            "treatment": "Deacidification, Protective Coating & Climate Stabilization",
            "actions": [
                "Test paper pH and apply non-aqueous or aqueous deacidification to halt acid hydrolysis/yellowing.",
                "Apply reversible archival protective sizing/coating (e.g. methylcellulose) to stabilize brittle fibers.",
                "Store in lignin-free, buffered archival folders inside climate-controlled dark storage (18–20°C, 45% RH).",
            ],
        }));
    }
    if faded_lvl != DamageLevel::None {
        healing_suggestions.push(json!({
            "issue": "faded",
            "severity": faded_lvl.as_str(),
            "treatment": "UV Protection, High-DPI Master Scan & Digital Contrast Recovery",
            "actions": [
                "Eliminate UV/daylight exposure; store in opaque archival boxes and display only under <= 50 lux UV-filtered lighting.",
                "Create a lossless high-DPI archival master scan (minimum 400–600 DPI) before further ink degradation.",
                "Use `arkib_enhance_image` (mode='auto' or 'manuscript_ocr') for non-destructive digital legibility recovery.",
            ],
        }));
    }
    if quality["overall_score"].as_f64().unwrap_or(100.0) < 62.0 {
        healing_suggestions.push(json!({
            "issue": "digital_capture_quality",
            "severity": if quality["overall_score"].as_f64().unwrap_or(0.0) < 42.0 { "heavy" } else { "medium" },
            "treatment": "Digital Re-Capture & Automated Quality Enhancement",
            "actions": [
                "Re-scan with even diffuse overhead lighting at >= 400 DPI if physical item is accessible.",
                "Run `arkib_enhance_image` to correct lighting, remove yellowing cast, and sharpen text edges.",
            ],
        }));
    }
    if healing_suggestions.is_empty() {
        healing_suggestions.push(json!({
            "issue": "preventive_care",
            "severity": "none",
            "treatment": "Standard Preventive Archival Preservation",
            "actions": [
                "Item shows good digital quality and minimal visible physical deterioration.",
                "Maintain standard archival storage (18–20°C, 45–50% RH, acid-free folders) and periodic SHA-256 checksum audits.",
            ],
        }));
    }

    Ok(json!({
        "file": path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
        "image_quality": quality,
        "damage_assessment": {
            "overall_severity": overall_damage_level,
            "tear": { "level": tear_lvl.as_str(), "score": (s.tear_score * 100.0).round() / 100.0 },
            "faded": { "level": faded_lvl.as_str(), "score": (s.faded_score * 100.0).round() / 100.0 },
            "mold": { "level": mold_lvl.as_str(), "score": (s.mold_score * 100.0).round() / 100.0 },
            "aging": { "level": aging_lvl.as_str(), "score": (s.aging_score * 100.0).round() / 100.0 },
        },
        "conservation_healing_suggestions": healing_suggestions,
        "enhancement_recommended": max_rank >= 1 || quality["overall_score"].as_f64().unwrap_or(100.0) < 68.0,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UpscaleSpec {
    scale: u32,
    anime_model: bool,
}

fn parse_upscale_mode(mode_clean: &str) -> Option<UpscaleSpec> {
    match mode_clean {
        "ai_upscale_2x" | "upscale_2x" | "upscale" | "ai_upscale" => Some(UpscaleSpec {
            scale: 2,
            anime_model: false,
        }),
        "ai_upscale_4x" | "upscale_4x" | "4x" => Some(UpscaleSpec {
            scale: 4,
            anime_model: false,
        }),
        "ai_upscale_anime" | "upscale_anime" | "ai_upscale_graphic" => Some(UpscaleSpec {
            scale: 4,
            anime_model: true,
        }),
        _ => None,
    }
}

/// Discover local Vulkan Real-ESRGAN / Upscayl binary + models directory.
fn find_vulkan_upscaler(cfg: &AppConfig) -> Option<(PathBuf, PathBuf)> {
    let mut candidates: Vec<(PathBuf, PathBuf)> = vec![
        (
            cfg.project_root
                .join("tools")
                .join("upscaler")
                .join("realesrgan-ncnn-vulkan.exe"),
            cfg.project_root
                .join("tools")
                .join("upscaler")
                .join("models"),
        ),
        (
            cfg.project_root
                .join("tools")
                .join("upscaler")
                .join("upscayl-bin.exe"),
            cfg.project_root
                .join("tools")
                .join("upscaler")
                .join("models"),
        ),
    ];
    for env_key in ["ProgramFiles", "LOCALAPPDATA"] {
        if let Ok(base) = std::env::var(env_key) {
            let root = PathBuf::from(base).join("Upscayl").join("resources");
            candidates.push((
                root.join("bin").join("upscayl-bin.exe"),
                root.join("models"),
            ));
            let prog = PathBuf::from(std::env::var(env_key).unwrap_or_default())
                .join("Programs")
                .join("Upscayl")
                .join("resources");
            candidates.push((
                prog.join("bin").join("upscayl-bin.exe"),
                prog.join("models"),
            ));
        }
    }
    for (bin, models_dir) in candidates {
        if bin.is_file() && models_dir.is_dir() {
            return Some((bin, models_dir));
        }
    }
    None
}

/// Discover an optional in-process ONNX Super-Resolution model in `models/`.
fn find_onnx_sr_model(cfg: &AppConfig) -> Option<PathBuf> {
    let models_dir = cfg.project_root.join("models");
    for name in [
        "realesr_general_x4v3.onnx",
        "realesr-general-x4v3.onnx",
        "realesrgan_x4plus.onnx",
        "span_x4.onnx",
        "upscaler.onnx",
    ] {
        let p = models_dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn run_vulkan_upscale(
    bin: &Path,
    models_dir: &Path,
    orig: &DynamicImage,
    spec: UpscaleSpec,
) -> Result<(DynamicImage, String), String> {
    let (ow, oh) = orig.dimensions();
    // Guard against excessive output memory on huge scans: cap input to 1024px for 4x (max 4096px out)
    let max_in = if spec.scale >= 4 { 1024 } else { 1536 };
    let prep = if ow > max_in || oh > max_in {
        orig.resize(max_in, max_in, FilterType::Lanczos3)
    } else {
        orig.clone()
    };

    let tmp_dir = std::env::temp_dir().join(format!("arkib_sr_{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&tmp_dir).map_err(|e| format!("Create temp SR dir failed: {e}"))?;
    let in_file = tmp_dir.join("in.png");
    let out_file = tmp_dir.join("out.png");
    prep.save(&in_file)
        .map_err(|e| format!("Write temp SR input failed: {e}"))?;

    let model_name = if spec.anime_model && models_dir.join("realesrgan-x4plus-anime.bin").is_file()
    {
        "realesrgan-x4plus-anime"
    } else if models_dir.join("realesrgan-x4plus.bin").is_file() {
        "realesrgan-x4plus"
    } else if models_dir.join("ultrasharp-4x.bin").is_file() {
        "ultrasharp-4x"
    } else {
        "realesrgan-x4plus"
    };

    let status = Command::new(bin)
        .arg("-i")
        .arg(&in_file)
        .arg("-o")
        .arg(&out_file)
        .arg("-n")
        .arg(model_name)
        .arg("-m")
        .arg(models_dir)
        .arg("-s")
        .arg("4")
        .arg("-t")
        .arg("200")
        .arg("-g")
        .arg("0")
        .output()
        .map_err(|e| format!("Failed launching Vulkan upscaler: {e}"))?;

    if !status.status.success() || !out_file.is_file() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(format!("Vulkan upscaler failed: {}", stderr.trim()));
    }

    let mut upscaled = image::open(&out_file)
        .map_err(|e| format!("Failed reading Vulkan upscaled output: {e}"))?
        .to_rgb8();
    let _ = fs::remove_dir_all(&tmp_dir);

    let mut dyn_img = DynamicImage::ImageRgb8(upscaled);
    if spec.scale == 2 {
        let target_w = (prep.width() * 2).max(1);
        let target_h = (prep.height() * 2).max(1);
        dyn_img = dyn_img.resize_exact(target_w, target_h, FilterType::Lanczos3);
    } else {
        // Rebind to satisfy borrow checker cleanly
        upscaled = dyn_img.to_rgb8();
        dyn_img = DynamicImage::ImageRgb8(upscaled);
    }

    Ok((dyn_img, format!("Real-ESRGAN Vulkan GPU ({model_name})")))
}

fn run_onnx_sr_upscale(
    cfg: &AppConfig,
    model_path: &Path,
    orig: &DynamicImage,
    spec: UpscaleSpec,
) -> Result<(DynamicImage, String), String> {
    let env = ort::init_from(&cfg.ort_dylib_path)
        .map_err(|e| format!("ORT init error: {e}"))?
        .build()
        .map_err(|e| format!("ORT env build error: {e}"))?;
    let mut sess = Session::builder(&env)
        .map_err(|e| format!("ORT SR builder error: {e}"))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| format!("ORT SR opt error: {e}"))?
        .with_intra_threads(4)
        .map_err(|e| format!("ORT SR threads error: {e}"))?
        .commit_from_file(model_path)
        .map_err(|e| format!("ORT SR load error: {e}"))?;

    let input_name = sess
        .inputs()
        .first()
        .map(|i| i.name().to_string())
        .unwrap_or_else(|| "input".to_string());

    let max_in = 768u32;
    let prep = if orig.width() > max_in || orig.height() > max_in {
        orig.resize(max_in, max_in, FilterType::Lanczos3)
    } else {
        orig.clone()
    };
    let rgb = prep.to_rgb8();
    let (w, h) = rgb.dimensions();

    // Tiled 128x128 inference with 8px overlap to keep RAM low and avoid seams
    let tile = 128u32;
    let pad = 8u32;
    let mut out_img: Option<RgbImage> = None;
    let mut detected_scale = 4u32;

    let mut y0 = 0u32;
    while y0 < h {
        let mut x0 = 0u32;
        while x0 < w {
            let x1 = (x0 + tile).min(w);
            let y1 = (y0 + tile).min(h);
            let px0 = x0.saturating_sub(pad);
            let py0 = y0.saturating_sub(pad);
            let px1 = (x1 + pad).min(w);
            let py1 = (y1 + pad).min(h);
            let tw = (px1 - px0) as usize;
            let th = (py1 - py0) as usize;

            let mut data = vec![0.0f32; 3 * th * tw];
            for ty in 0..th {
                for tx in 0..tw {
                    let p = rgb.get_pixel(px0 + tx as u32, py0 + ty as u32).0;
                    let idx = ty * tw + tx;
                    data[idx] = f32::from(p[0]) / 255.0;
                    data[th * tw + idx] = f32::from(p[1]) / 255.0;
                    data[2 * th * tw + idx] = f32::from(p[2]) / 255.0;
                }
            }
            let tensor = Tensor::from_array(([1usize, 3, th, tw], data))
                .map_err(|e| format!("ORT SR tensor error: {e}"))?;
            let outputs = sess
                .run(ort::inputs![input_name.as_str() => tensor])
                .map_err(|e| format!("ORT SR run error: {e}"))?;
            let (shape, out_slice) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("ORT SR extract error: {e}"))?;
            if shape.len() != 4 {
                return Err("Unexpected ONNX SR output shape".to_string());
            }
            let oh = shape[2] as usize;
            let ow = shape[3] as usize;
            let sc = ((ow / tw.max(1)) as u32).max(1);
            detected_scale = sc;
            let canvas = out_img.get_or_insert_with(|| RgbImage::new(w * sc, h * sc));

            let crop_x = ((x0 - px0) * sc) as usize;
            let crop_y = ((y0 - py0) * sc) as usize;
            let copy_w = ((x1 - x0) * sc) as usize;
            let copy_h = ((y1 - y0) * sc) as usize;
            let plane = oh * ow;
            for cy in 0..copy_h {
                for cx in 0..copy_w {
                    let sy = (crop_y + cy).min(oh - 1);
                    let sx = (crop_x + cx).min(ow - 1);
                    let idx = sy * ow + sx;
                    let r = (out_slice[idx].clamp(0.0, 1.0) * 255.0).round() as u8;
                    let g = (out_slice[plane + idx].clamp(0.0, 1.0) * 255.0).round() as u8;
                    let b = (out_slice[2 * plane + idx].clamp(0.0, 1.0) * 255.0).round() as u8;
                    canvas.put_pixel(x0 * sc + cx as u32, y0 * sc + cy as u32, Rgb([r, g, b]));
                }
            }
            x0 += tile;
        }
        y0 += tile;
    }

    let mut dyn_img = DynamicImage::ImageRgb8(out_img.unwrap_or(rgb));
    if spec.scale != detected_scale {
        let tw = (w * spec.scale).max(1);
        let th = (h * spec.scale).max(1);
        dyn_img = dyn_img.resize_exact(tw, th, FilterType::Lanczos3);
    }
    let mname = model_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("onnx-sr");
    Ok((dyn_img, format!("Pure-Rust ONNX Runtime ({mname})")))
}

/// Hybrid 3-Tier Neural Super-Resolution:
/// 1) Real-ESRGAN / Upscayl Vulkan GPU (`tools/upscaler/realesrgan-ncnn-vulkan.exe` or installed Upscayl)
/// 2) In-process Pure-Rust ONNX Super-Resolution (`models/realesr*.onnx` via `ort`)
/// 3) Clean Pure-Rust Lanczos3 + Unsharp detail fallback
fn run_hybrid_upscale(
    cfg: &AppConfig,
    orig: &DynamicImage,
    spec: UpscaleSpec,
) -> (DynamicImage, String, Vec<&'static str>) {
    if let Some((bin, models_dir)) = find_vulkan_upscaler(cfg)
        && let Ok((img, engine_label)) = run_vulkan_upscale(&bin, &models_dir, orig, spec)
    {
        return (
            img,
            engine_label,
            vec!["realesrgan_vulkan_gpu_super_resolution"],
        );
    }
    if let Some(onnx_path) = find_onnx_sr_model(cfg)
        && let Ok((img, engine_label)) = run_onnx_sr_upscale(cfg, &onnx_path, orig, spec)
    {
        return (img, engine_label, vec!["ort_onnx_neural_super_resolution"]);
    }
    let tw = (orig.width() * spec.scale).clamp(1, 4096);
    let th = (orig.height() * spec.scale).clamp(1, 4096);
    let resized = orig.resize_exact(tw, th, FilterType::Lanczos3).to_rgb8();
    let sharpened = unsharp_mask_rgb(&resized, 1.55);
    (
        DynamicImage::ImageRgb8(sharpened),
        "Pure-Rust Lanczos3 Fallback".to_string(),
        vec!["lanczos3_upscale", "unsharp_mask_detail_boost"],
    )
}

/// Pure-Rust archival image enhancement & hybrid neural AI super-resolution pipeline.
pub fn enhance_archival_image(
    cfg: &AppConfig,
    image_path: &Path,
    mode: &str,
) -> Result<Value, String> {
    let orig = load_rgb_image(image_path)?;
    let (before_w, before_h) = orig.dimensions();
    let before_stats = compute_pixel_stats(&orig);
    let before_quality = quality_scores_from_stats(&before_stats);

    let mode_clean = mode.trim().to_ascii_lowercase();
    let (enhanced_dyn, upscale_engine, steps_applied, scale_factor) =
        if let Some(spec) = parse_upscale_mode(&mode_clean) {
            let (up_img, engine_label, steps) = run_hybrid_upscale(cfg, &orig, spec);
            (up_img, engine_label, steps, spec.scale)
        } else {
            let mut rgb = orig.to_rgb8();
            let mut steps: Vec<&str> = Vec::new();

            // 1. De-yellowing / Gray-World White Balance (when aging/yellowing detected or requested)
            if mode_clean == "deyellow_restore"
                || (matches!(mode_clean.as_str(), "" | "auto" | "manuscript_ocr")
                    && before_stats.aging_score >= 0.22)
            {
                rgb = white_balance_deyellow(&rgb, 0.72);
                steps.push("gray_world_deyellow_white_balance");
            }

            // 2. Adaptive Gamma / Lighting Exposure Correction
            let gamma = if before_stats.mean_luma < 85.0 {
                0.72
            } else if before_stats.mean_luma > 205.0 {
                1.32
            } else if mode_clean == "contrast_lighting" {
                0.88
            } else {
                1.0
            };
            if (gamma - 1.0_f64).abs() > 0.01 {
                rgb = apply_gamma(&rgb, gamma);
                steps.push("adaptive_gamma_lighting_correction");
            }

            // 3. Percentile Autocontrast Stretch (1.5% cutoff)
            let cutoff = if mode_clean == "manuscript_ocr" {
                2.0
            } else {
                1.2
            };
            rgb = autocontrast_rgb(&rgb, cutoff);
            steps.push("percentile_autocontrast_stretch");

            // 4. Unsharp Masking (sharpens faded text strokes & fine architectural detail)
            let sharpen_amt = if mode_clean == "manuscript_ocr" {
                1.75
            } else if mode_clean == "denoise_sharpen" {
                1.60
            } else {
                1.45
            };
            rgb = unsharp_mask_rgb(&rgb, sharpen_amt);
            steps.push("unsharp_mask_readability_boost");

            (
                DynamicImage::ImageRgb8(rgb),
                "Pure-Rust Archival Pipeline".to_string(),
                steps,
                1u32,
            )
        };

    let (after_w, after_h) = enhanced_dyn.dimensions();
    let after_stats = compute_pixel_stats(&enhanced_dyn);
    let after_quality = quality_scores_from_stats(&after_stats);

    let out_dir = cfg.librechat_dir.join("images").join("enhanced");
    fs::create_dir_all(&out_dir).map_err(|e| format!("Create enhanced dir failed: {e}"))?;
    let stem = profile_slug(
        image_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("image"),
    );
    let short = &Uuid::new_v4().simple().to_string()[..6];
    let out_name = format!(
        "{}_enhanced_{short}.jpg",
        if stem.is_empty() { "image" } else { &stem }
    );
    let out_path = out_dir.join(&out_name);
    let jpeg = encode_jpeg(&enhanced_dyn, 94)?;
    fs::write(&out_path, jpeg).map_err(|e| format!("Save enhanced image failed: {e}"))?;

    Ok(json!({
        "status": "enhanced",
        "mode": if mode_clean.is_empty() { "auto" } else { &mode_clean },
        "upscale_engine": upscale_engine,
        "scale_factor": scale_factor,
        "before_dimensions": format!("{before_w}x{before_h}"),
        "after_dimensions": format!("{after_w}x{after_h}"),
        "steps_applied": steps_applied,
        "enhanced_filename": out_name,
        "enhanced_image_url": format!("{}/images/enhanced/{out_name}", cfg.librechat_url),
        "before_quality": before_quality,
        "after_quality": after_quality,
        "display_instruction": "Display the enhanced image using ![Enhanced archival image](enhanced_image_url) and summarize the Before vs After resolution, engine, lighting, contrast, and readability improvements.",
    }))
}

fn white_balance_deyellow(img: &RgbImage, strength: f64) -> RgbImage {
    let (w, h) = img.dimensions();
    let total = ((w as usize) * (h as usize)).max(1) as f64;
    let (mut sr, mut sg, mut sb) = (0.0, 0.0, 0.0);
    for p in img.pixels() {
        sr += f64::from(p[0]);
        sg += f64::from(p[1]);
        sb += f64::from(p[2]);
    }
    let mr = (sr / total).max(1.0);
    let mg = (sg / total).max(1.0);
    let mb = (sb / total).max(1.0);
    let gray = (mr + mg + mb) / 3.0;
    let gr = 1.0 + ((gray / mr) - 1.0) * strength;
    let gg = 1.0 + ((gray / mg) - 1.0) * strength;
    let gb = 1.0 + ((gray / mb) - 1.0) * strength;

    ImageBuffer::from_fn(w, h, |x, y| {
        let p = img.get_pixel(x, y);
        Rgb([
            (f64::from(p[0]) * gr).round().clamp(0.0, 255.0) as u8,
            (f64::from(p[1]) * gg).round().clamp(0.0, 255.0) as u8,
            (f64::from(p[2]) * gb).round().clamp(0.0, 255.0) as u8,
        ])
    })
}

fn apply_gamma(img: &RgbImage, gamma: f64) -> RgbImage {
    let mut lut = [0u8; 256];
    for (i, slot) in lut.iter_mut().enumerate() {
        *slot = (((i as f64) / 255.0).powf(gamma) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    let (w, h) = img.dimensions();
    ImageBuffer::from_fn(w, h, |x, y| {
        let p = img.get_pixel(x, y);
        Rgb([lut[p[0] as usize], lut[p[1] as usize], lut[p[2] as usize]])
    })
}

fn autocontrast_rgb(img: &RgbImage, cutoff_pct: f32) -> RgbImage {
    let (w, h) = img.dimensions();
    let total = (w as usize) * (h as usize);
    if total == 0 {
        return img.clone();
    }
    let cut = ((total as f32) * (cutoff_pct / 100.0)) as usize;
    let mut lut = [[0u8; 256]; 3];
    for ch in 0..3 {
        let mut hist = [0usize; 256];
        for p in img.pixels() {
            hist[p[ch] as usize] += 1;
        }
        let mut lo = 0usize;
        let mut acc = 0usize;
        while lo < 255 && acc + hist[lo] <= cut {
            acc += hist[lo];
            lo += 1;
        }
        let mut hi = 255usize;
        acc = 0;
        while hi > lo && acc + hist[hi] <= cut {
            acc += hist[hi];
            hi -= 1;
        }
        let span = hi.saturating_sub(lo);
        if span == 0 {
            for (i, slot) in lut[ch].iter_mut().enumerate() {
                *slot = i as u8;
            }
        } else {
            for (i, slot) in lut[ch].iter_mut().enumerate() {
                let scaled = if i <= lo {
                    0.0
                } else if i >= hi {
                    255.0
                } else {
                    ((i - lo) as f32) * 255.0 / (span as f32)
                };
                *slot = scaled.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    ImageBuffer::from_fn(w, h, |x, y| {
        let p = img.get_pixel(x, y);
        Rgb([
            lut[0][p[0] as usize],
            lut[1][p[1] as usize],
            lut[2][p[2] as usize],
        ])
    })
}

fn unsharp_mask_rgb(img: &RgbImage, amount: f32) -> RgbImage {
    let (w, h) = img.dimensions();
    if w < 3 || h < 3 {
        return img.clone();
    }
    ImageBuffer::from_fn(w, h, |x, y| {
        if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
            return *img.get_pixel(x, y);
        }
        let c = img.get_pixel(x, y);
        let l = img.get_pixel(x - 1, y);
        let r = img.get_pixel(x + 1, y);
        let u = img.get_pixel(x, y - 1);
        let d = img.get_pixel(x, y + 1);
        let mut out = [0u8; 3];
        for ch in 0..3 {
            let blur =
                (f32::from(l[ch]) + f32::from(r[ch]) + f32::from(u[ch]) + f32::from(d[ch])) * 0.25;
            let val = f32::from(c[ch]) + (f32::from(c[ch]) - blur) * (amount - 1.0);
            out[ch] = val.round().clamp(0.0, 255.0) as u8;
        }
        Rgb(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_damage_classification_synthetic() {
        // Yellowed, faded archival parchment simulation
        let aged = DynamicImage::ImageRgb8(RgbImage::from_pixel(400, 400, Rgb([210, 190, 140])));
        let s = compute_pixel_stats(&aged);
        assert!(s.aging_score > 0.40);
        assert!(s.faded_score > 0.60);
        assert_eq!(
            DamageLevel::from_score(s.faded_score, 0.28, 0.48, 0.68),
            DamageLevel::Heavy
        );
    }

    #[test]
    fn test_upscale_mode_parsing() {
        assert_eq!(
            parse_upscale_mode("ai_upscale_2x"),
            Some(UpscaleSpec {
                scale: 2,
                anime_model: false
            })
        );
        assert_eq!(
            parse_upscale_mode("ai_upscale_4x"),
            Some(UpscaleSpec {
                scale: 4,
                anime_model: false
            })
        );
        assert_eq!(
            parse_upscale_mode("ai_upscale_anime"),
            Some(UpscaleSpec {
                scale: 4,
                anime_model: true
            })
        );
        assert_eq!(parse_upscale_mode("auto"), None);
    }
}
