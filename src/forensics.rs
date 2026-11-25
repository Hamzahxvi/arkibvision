#![forbid(unsafe_code)]

//! Forensic image/file metadata extraction & authenticity verification:
//! - Full file metadata (name, size, MIME, dimensions, SHA-256, perceptual aHash)
//! - EXIF metadata (DateTimeOriginal, Camera Make/Model, Software, GPS coordinates, Artist/Description)
//! - PNG text chunk inspection (tEXt / iTXt / zTXt) & C2PA/XMP provenance marker scan
//! - Archive duplicate detection (SHA-256 exact copy & perceptual aHash near-duplicate copy)
//! - Multi-signal AI-generated / Original / Copy / Edited fraud assessment

use crate::config::{PERCEPTUAL_HASH_DISTANCE, is_image_ext};
use crate::vision_ops::{encode_jpeg, hash_distance, load_rgb_image, perceptual_hash};
use chrono::Utc;
use flate2::read::ZlibDecoder;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use walkdir::WalkDir;

const AI_SOFTWARE_MARKERS: &[&str] = &[
    "stable diffusion",
    "automatic1111",
    "comfyui",
    "midjourney",
    "dall-e",
    "dalle",
    "openai",
    "chatgpt",
    "gpt-image",
    "adobe firefly",
    "firefly",
    "imagen",
    "made with google ai",
    "google ai",
    "google synthid",
    "synthid",
    "gemini",
    "meta ai",
    "imagined with ai",
    "sora",
    "veo",
    "kling",
    "runway",
    "hailuo",
    "luma",
    "pika",
    "grok",
    "flux",
    "novelai",
    "invokeai",
    "leonardo.ai",
    "ideogram",
    "recraft",
    "krea",
    "heygen",
    "deepfake",
    "faceswap",
    "reface",
    "bing image creator",
    "generative fill",
    "ai generated",
    "ai-generated",
    "c2pa.ai_generative_training",
    "c2pa.created",
    "trainedalgorithmicmedia",
    "compositewithtrainedalgorithmicmedia",
    "compositesynthetic",
];

const EXTERNAL_SOURCE_MARKERS: &[&str] = &[
    "fbmd",
    "fbmn",
    "whatsapp",
    "telegram",
    "tiktok",
    "shutterstock",
    "gettyimages",
    "istockphoto",
    "alamy",
    "adobe stock",
    "dreamstime",
    "depositphotos",
    "123rf",
    "snipping tool",
    "screenshot",
];

const EDITOR_SOFTWARE_MARKERS: &[&str] = &[
    "adobe photoshop",
    "lightroom",
    "gimp",
    "canva",
    "snapseed",
    "picsart",
    "affinity photo",
    "pixelmator",
    "corel",
    "paint.net",
    "capcut",
    "meitu",
];

const COMMON_AI_DIMS: &[(u32, u32)] = &[
    (512, 512),
    (768, 768),
    (1024, 1024),
    (1024, 1536),
    (1536, 1024),
    (1024, 1792),
    (1792, 1024),
    (832, 1216),
    (1216, 832),
    (896, 1152),
    (1152, 896),
    (1344, 768),
    (768, 1344),
    (1408, 768),
    (768, 1408),
];

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{b:02x}");
    }
    hex
}

pub fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB", (bytes as f64) / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", (bytes as f64) / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn extract_exif_map(bytes: &[u8]) -> Map<String, Value> {
    let mut out = Map::new();
    let mut cursor = Cursor::new(bytes);
    let Ok(exif_data) = exif::Reader::new().read_from_container(&mut cursor) else {
        return out;
    };

    let get_tag = |tag: exif::Tag| -> Option<String> {
        exif_data
            .get_field(tag, exif::In::PRIMARY)
            .map(|f| f.display_value().with_unit(&exif_data).to_string())
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
    };

    for (key, tag) in [
        ("date_time_original", exif::Tag::DateTimeOriginal),
        ("date_time", exif::Tag::DateTime),
        ("camera_make", exif::Tag::Make),
        ("camera_model", exif::Tag::Model),
        ("software", exif::Tag::Software),
        ("artist", exif::Tag::Artist),
        ("copyright", exif::Tag::Copyright),
        ("image_description", exif::Tag::ImageDescription),
        ("user_comment", exif::Tag::UserComment),
        ("lens_model", exif::Tag::LensModel),
        ("exposure_time", exif::Tag::ExposureTime),
        ("f_number", exif::Tag::FNumber),
        ("iso_speed", exif::Tag::PhotographicSensitivity),
        ("orientation", exif::Tag::Orientation),
    ] {
        if let Some(val) = get_tag(tag) {
            out.insert(key.to_string(), Value::String(val));
        }
    }

    if let (Some(lat), Some(lon)) = (
        extract_gps_coord(&exif_data, true),
        extract_gps_coord(&exif_data, false),
    ) {
        out.insert(
            "gps_latitude".to_string(),
            json!((lat * 1_000_000.0).round() / 1_000_000.0),
        );
        out.insert(
            "gps_longitude".to_string(),
            json!((lon * 1_000_000.0).round() / 1_000_000.0),
        );
        out.insert(
            "gps_coordinates".to_string(),
            Value::String(format!("{lat:.6}, {lon:.6}")),
        );
    }

    out
}

fn extract_gps_coord(exif_data: &exif::Exif, is_lat: bool) -> Option<f64> {
    let (val_tag, ref_tag) = if is_lat {
        (exif::Tag::GPSLatitude, exif::Tag::GPSLatitudeRef)
    } else {
        (exif::Tag::GPSLongitude, exif::Tag::GPSLongitudeRef)
    };
    let field = exif_data.get_field(val_tag, exif::In::PRIMARY)?;
    let exif::Value::Rational(ref rats) = field.value else {
        return None;
    };
    if rats.len() < 3 {
        return None;
    }
    let deg = rats[0].to_f64();
    let min = rats[1].to_f64();
    let sec = rats[2].to_f64();
    let mut decimal = deg + min / 60.0 + sec / 3600.0;
    if let Some(rf) = exif_data.get_field(ref_tag, exif::In::PRIMARY) {
        let s = rf.display_value().to_string();
        if s.contains('S') || s.contains('W') {
            decimal = -decimal;
        }
    }
    Some(decimal)
}

/// Inspect PNG `tEXt`, `iTXt`, and `zTXt` chunks where AI generators embed prompts/workflows.
fn extract_png_text_chunks(bytes: &[u8]) -> Map<String, Value> {
    let mut out = Map::new();
    const PNG_SIG: &[u8] = &[137, 80, 78, 71, 13, 10, 26, 10];
    if bytes.len() < 16 || &bytes[..8] != PNG_SIG {
        return out;
    }
    let mut pos = 8usize;
    while pos + 12 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let chunk_type = &bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end = data_start.saturating_add(len);
        if data_end + 4 > bytes.len() {
            break;
        }
        let data = &bytes[data_start..data_end];
        if chunk_type == b"tEXt" {
            if let Some(nul) = data.iter().position(|&b| b == 0) {
                let key = String::from_utf8_lossy(&data[..nul]).to_string();
                let val = String::from_utf8_lossy(&data[nul + 1..])
                    .chars()
                    .take(500)
                    .collect::<String>();
                out.insert(key, Value::String(val));
            }
        } else if chunk_type == b"zTXt" {
            if let Some(nul) = data.iter().position(|&b| b == 0)
                && nul + 2 < data.len()
            {
                let key = String::from_utf8_lossy(&data[..nul]).to_string();
                let mut decoder = ZlibDecoder::new(&data[nul + 2..]);
                let mut decoded = String::new();
                if decoder.read_to_string(&mut decoded).is_ok() {
                    let preview: String = decoded.chars().take(500).collect();
                    out.insert(key, Value::String(preview));
                }
            }
        } else if chunk_type == b"iTXt"
            && let Some(nul) = data.iter().position(|&b| b == 0)
        {
            let key = String::from_utf8_lossy(&data[..nul]).to_string();
            let text_preview = String::from_utf8_lossy(&data[nul + 1..])
                .replace('\0', " ")
                .trim()
                .chars()
                .take(500)
                .collect::<String>();
            if !text_preview.is_empty() {
                out.insert(key, Value::String(text_preview));
            }
        }
        pos = data_end + 4;
    }
    out
}

fn scan_raw_markers(bytes: &[u8]) -> (Vec<String>, Vec<String>, Vec<String>, bool, bool) {
    // Scan header + trailer up to 384 KB for C2PA/JUMBF/XMP/IPTC/AI/external-source markers.
    let mut sample = String::new();
    let head_len = bytes.len().min(262_144);
    sample.push_str(&String::from_utf8_lossy(&bytes[..head_len]));
    if bytes.len() > head_len {
        let tail_start = bytes.len().saturating_sub(131_072).max(head_len);
        sample.push_str(&String::from_utf8_lossy(&bytes[tail_start..]));
    }
    let low = sample.to_ascii_lowercase();

    let mut ai_hits = Vec::new();
    for &marker in AI_SOFTWARE_MARKERS {
        if low.contains(marker) {
            ai_hits.push(marker.to_string());
        }
    }
    let mut editor_hits = Vec::new();
    for &marker in EDITOR_SOFTWARE_MARKERS {
        if low.contains(marker) {
            editor_hits.push(marker.to_string());
        }
    }
    let mut external_hits = Vec::new();
    for &marker in EXTERNAL_SOURCE_MARKERS {
        if low.contains(marker) {
            external_hits.push(marker.to_string());
        }
    }
    let has_c2pa =
        low.contains("c2pa") || low.contains("jumb") || low.contains("contentcredentials");
    let has_iptc_ai_source = low.contains("digitalsourcetype")
        && (low.contains("trainedalgorithmicmedia") || low.contains("compositesynthetic"));
    (
        ai_hits,
        editor_hits,
        external_hits,
        has_c2pa,
        has_iptc_ai_source,
    )
}

/// Deep JPEG structural & header inspection: detects APP0..APP15 segments, C2PA JUMBF,
/// XMP/IPTC provenance, ICC profiles, quantization quality, and bare stripped web/social copies.
fn inspect_jpeg_structure(bytes: &[u8]) -> Option<Value> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    let mut has_jfif = false;
    let mut has_exif = false;
    let mut has_xmp = false;
    let mut has_icc_profile = false;
    let mut has_c2pa_jumbf = false;
    let mut has_iptc_photoshop = false;
    let mut has_adobe_app14 = false;
    let mut has_comment = false;
    let mut is_progressive = false;
    let mut est_jpeg_quality: Option<u8> = None;
    let mut segment_names: Vec<String> = Vec::new();
    let mut xmp_creator_tool = String::new();
    let mut xmp_digital_source = String::new();

    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        while i + 1 < bytes.len() && bytes[i + 1] == 0xFF {
            i += 1;
        }
        if i + 1 >= bytes.len() {
            break;
        }
        let marker = bytes[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if marker == 0x00 || (0xD0..=0xD8).contains(&marker) {
            i += 2;
            continue;
        }
        if i + 4 > bytes.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if seg_len < 2 || i + 2 + seg_len > bytes.len() {
            break;
        }
        let payload = &bytes[i + 4..i + 2 + seg_len];
        match marker {
            0xE0 => {
                has_jfif = payload.starts_with(b"JFIF");
                segment_names.push("APP0(JFIF)".into());
            }
            0xE1 => {
                if payload.starts_with(b"Exif\0\0") {
                    has_exif = true;
                    segment_names.push("APP1(EXIF)".into());
                } else if payload.starts_with(b"http://ns.adobe.com/xap/") {
                    has_xmp = true;
                    segment_names.push("APP1(XMP)".into());
                    let xmp_txt = String::from_utf8_lossy(payload);
                    if let Some(v) = extract_xml_attr_or_tag(&xmp_txt, "CreatorTool") {
                        xmp_creator_tool = v;
                    }
                    if let Some(v) = extract_xml_attr_or_tag(&xmp_txt, "DigitalSourceType") {
                        xmp_digital_source = v;
                    }
                } else {
                    segment_names.push("APP1".into());
                }
            }
            0xE2 => {
                if payload.starts_with(b"ICC_PROFILE") {
                    has_icc_profile = true;
                    if !segment_names.iter().any(|s| s == "APP2(ICC)") {
                        segment_names.push("APP2(ICC)".into());
                    }
                }
            }
            0xEB => {
                // APP11 carries JUMBF boxes (ISO 19566-5), used by C2PA Content Credentials.
                let txt =
                    String::from_utf8_lossy(&payload[..payload.len().min(64)]).to_ascii_lowercase();
                if txt.contains("jumb") || txt.contains("c2pa") {
                    has_c2pa_jumbf = true;
                }
                segment_names.push("APP11(JUMBF)".into());
            }
            0xED => {
                if payload.starts_with(b"Photoshop 3.0") {
                    has_iptc_photoshop = true;
                    segment_names.push("APP13(IPTC/Photoshop)".into());
                }
            }
            0xEE => {
                has_adobe_app14 = true;
                segment_names.push("APP14(Adobe)".into());
            }
            0xFE => {
                has_comment = true;
                segment_names.push("COM".into());
            }
            0xC2 => {
                is_progressive = true;
            }
            0xDB => {
                // Luminance quantization table (precision=0, table_id=0) -> approximate IJG quality.
                if est_jpeg_quality.is_none() && payload.len() >= 65 && payload[0] == 0 {
                    let sum: u32 = payload[1..65].iter().map(|&v| u32::from(v)).sum();
                    let avg = f64::from(sum) / 64.0;
                    // Standard IJG Q=50 luminance table avg ~= 47.6; Q=90 ~= 9.5; Q=75 ~= 23.8.
                    let q = if avg <= 1.2 {
                        100
                    } else if avg <= 47.6 {
                        (100.0 - (avg / 47.6) * 50.0).round().clamp(50.0, 99.0) as u8
                    } else {
                        ((50.0 * 47.6) / avg).round().clamp(5.0, 50.0) as u8
                    };
                    est_jpeg_quality = Some(q);
                }
            }
            _ => {}
        }
        i += 2 + seg_len;
    }

    let is_bare_stripped_jpeg =
        !has_jfif && !has_exif && !has_xmp && !has_icc_profile && !has_iptc_photoshop;

    Some(json!({
        "is_bare_stripped_jpeg": is_bare_stripped_jpeg,
        "has_jfif": has_jfif,
        "has_exif_segment": has_exif,
        "has_xmp_segment": has_xmp,
        "has_icc_profile": has_icc_profile,
        "has_c2pa_jumbf_box": has_c2pa_jumbf,
        "has_iptc_photoshop": has_iptc_photoshop,
        "has_adobe_app14": has_adobe_app14,
        "has_comment": has_comment,
        "is_progressive": is_progressive,
        "estimated_jpeg_quality": est_jpeg_quality,
        "xmp_creator_tool": xmp_creator_tool,
        "xmp_digital_source_type": xmp_digital_source,
        "segments_found": segment_names,
    }))
}

fn extract_xml_attr_or_tag(xml: &str, name: &str) -> Option<String> {
    let attr_pat = format!("{name}=\"");
    if let Some(pos) = xml.find(&attr_pat) {
        let rest = &xml[pos + attr_pat.len()..];
        if let Some(end) = rest.find('"') {
            let val = rest[..end].trim();
            if !val.is_empty() {
                return Some(val.chars().take(160).collect());
            }
        }
    }
    let tag_open = format!("{name}>");
    if let Some(pos) = xml.find(&tag_open) {
        let rest = &xml[pos + tag_open.len()..];
        if let Some(end) = rest.find('<') {
            let val = rest[..end].trim();
            if !val.is_empty() {
                return Some(val.chars().take(160).collect());
            }
        }
    }
    None
}

/// Deep pixel-level forensic inspection:
/// - Error Level Analysis (ELA) across an 8x8 spatial grid for localized splice/composite edits
/// - Local high-pass sensor noise uniformity / consistency analysis
/// - 8x8 JPEG block-grid boundary ratio (re-encoding / external copy detection)
/// - LSB bit-plane statistical probe for steganographic / invisible watermarks
fn analyze_pixel_forensics(path: &Path) -> Option<Value> {
    let img = load_rgb_image(path).ok()?;
    let work = if img.width().max(img.height()) > 640 {
        img.thumbnail(640, 640)
    } else {
        img
    };
    let rgb = work.to_rgb8();
    let (w, h) = rgb.dimensions();
    if w < 32 || h < 32 {
        return None;
    }

    // 1. Error Level Analysis (re-encode at JPEG Q=90 and compare spatial block residuals)
    let re_bytes = encode_jpeg(&work, 90).ok()?;
    let re_img = image::load_from_memory(&re_bytes).ok()?.to_rgb8();

    let cols = 8usize;
    let rows = 8usize;
    let bw = (w as usize / cols).max(1);
    let bh = (h as usize / rows).max(1);
    let mut ela_blocks = Vec::with_capacity(cols * rows);
    let mut noise_blocks = Vec::with_capacity(cols * rows);

    let mut total_ela = 0.0_f64;
    let mut lsb_ones = 0usize;
    let mut lsb_same_neighbor = 0usize;
    let mut lsb_pairs = 0usize;
    let mut total_px = 0usize;

    // Precompute luma buffer for noise + 8x8 blockiness analysis
    let wu = w as usize;
    let hu = h as usize;
    let mut luma = vec![0.0_f64; wu * hu];
    for y in 0..h {
        for x in 0..w {
            let p = rgb.get_pixel(x, y);
            let idx = (y as usize) * wu + (x as usize);
            luma[idx] = 0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2]);
            let bit = p[0] & 1;
            if bit == 1 {
                lsb_ones += 1;
            }
            if x + 1 < w {
                let np = rgb.get_pixel(x + 1, y);
                if (np[0] & 1) == bit {
                    lsb_same_neighbor += 1;
                }
                lsb_pairs += 1;
            }
            total_px += 1;
        }
    }

    for r in 0..rows {
        let y0 = r * bh;
        let y1 = if r + 1 == rows {
            hu
        } else {
            ((r + 1) * bh).min(hu)
        };
        for c in 0..cols {
            let x0 = c * bw;
            let x1 = if c + 1 == cols {
                wu
            } else {
                ((c + 1) * bw).min(wu)
            };
            let mut b_ela = 0.0_f64;
            let mut b_noise = 0.0_f64;
            let mut b_cnt = 0usize;
            let mut n_cnt = 0usize;
            for y in y0..y1 {
                for x in x0..x1 {
                    let p1 = rgb.get_pixel(x as u32, y as u32);
                    let p2 = re_img.get_pixel(x as u32, y as u32);
                    let diff = ((i32::from(p1[0]) - i32::from(p2[0])).abs()
                        + (i32::from(p1[1]) - i32::from(p2[1])).abs()
                        + (i32::from(p1[2]) - i32::from(p2[2])).abs())
                        as f64
                        / 3.0;
                    b_ela += diff;
                    total_ela += diff;
                    b_cnt += 1;

                    if x > 0 && y > 0 && x + 1 < wu && y + 1 < hu {
                        let idx = y * wu + x;
                        let hp = (4.0 * luma[idx]
                            - luma[idx - 1]
                            - luma[idx + 1]
                            - luma[idx - wu]
                            - luma[idx + wu])
                            .abs();
                        b_noise += hp;
                        n_cnt += 1;
                    }
                }
            }
            if b_cnt > 0 {
                ela_blocks.push(b_ela / (b_cnt as f64));
            }
            if n_cnt > 0 {
                noise_blocks.push(b_noise / (n_cnt as f64));
            }
        }
    }

    let ela_mean = total_ela / (total_px.max(1) as f64);
    ela_blocks.sort_by(|a, b| a.total_cmp(b));
    let ela_median = ela_blocks
        .get(ela_blocks.len() / 2)
        .copied()
        .unwrap_or(0.0)
        .max(0.25);
    let ela_p95 = ela_blocks
        .get((ela_blocks.len() * 95) / 100)
        .copied()
        .unwrap_or(ela_median);
    let ela_peak_ratio = ela_p95 / ela_median;

    let noise_mean = if noise_blocks.is_empty() {
        0.0
    } else {
        noise_blocks.iter().sum::<f64>() / (noise_blocks.len() as f64)
    };
    let noise_std = if noise_blocks.is_empty() {
        0.0
    } else {
        (noise_blocks
            .iter()
            .map(|&v| {
                let d = v - noise_mean;
                d * d
            })
            .sum::<f64>()
            / (noise_blocks.len() as f64))
            .sqrt()
    };
    let noise_cv = if noise_mean > 0.5 {
        noise_std / noise_mean
    } else {
        0.0
    };

    // 8x8 JPEG block-grid boundary discontinuity ratio
    let mut boundary_diff = 0.0_f64;
    let mut boundary_cnt = 0usize;
    let mut interior_diff = 0.0_f64;
    let mut interior_cnt = 0usize;
    for y in 0..hu {
        for x in 1..wu {
            let d = (luma[y * wu + x] - luma[y * wu + x - 1]).abs();
            if x % 8 == 0 {
                boundary_diff += d;
                boundary_cnt += 1;
            } else {
                interior_diff += d;
                interior_cnt += 1;
            }
        }
    }
    let blockiness_ratio = if boundary_cnt > 0 && interior_cnt > 0 {
        (boundary_diff / (boundary_cnt as f64)) / ((interior_diff / (interior_cnt as f64)).max(0.1))
    } else {
        1.0
    };

    let lsb_ones_ratio = (lsb_ones as f64) / (total_px.max(1) as f64);
    let lsb_neighbor_corr = (lsb_same_neighbor as f64) / (lsb_pairs.max(1) as f64);
    // LSB steganographic watermarks only survive in lossless containers (PNG/BMP); JPEG DCT quantization naturally randomizes RGB LSBs.
    let is_lossless_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "bmp"));
    let lsb_watermark_anomaly = is_lossless_ext
        && (lsb_ones_ratio - 0.5).abs() < 0.004
        && (lsb_neighbor_corr - 0.5).abs() < 0.008;

    let ela_splice_suspicious = ela_peak_ratio >= 2.6 && ela_p95 >= 4.0;
    let noise_inconsistent = noise_cv >= 0.72;
    let recompressed_block_artifacts = blockiness_ratio >= 1.12;

    Some(json!({
        "ela_mean": (ela_mean * 100.0).round() / 100.0,
        "ela_p95_to_median_ratio": (ela_peak_ratio * 100.0).round() / 100.0,
        "ela_splice_suspicious": ela_splice_suspicious,
        "noise_mean": (noise_mean * 100.0).round() / 100.0,
        "noise_variation_cv": (noise_cv * 100.0).round() / 100.0,
        "noise_inconsistent": noise_inconsistent,
        "jpeg_blockiness_ratio": (blockiness_ratio * 1000.0).round() / 1000.0,
        "recompressed_block_artifacts": recompressed_block_artifacts,
        "lsb_ones_ratio": (lsb_ones_ratio * 10000.0).round() / 10000.0,
        "lsb_neighbor_correlation": (lsb_neighbor_corr * 10000.0).round() / 10000.0,
        "lsb_watermark_signal": lsb_watermark_anomaly,
    }))
}

/// Extract comprehensive technical + EXIF + PNG metadata for any file or image.
pub fn extract_file_metadata(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed reading {}: {e}", path.display()))?;
    let meta = fs::metadata(path).map_err(|e| format!("Failed stat {}: {e}", path.display()))?;
    let file_size = meta.len();
    let sha256 = sha256_bytes(&bytes);
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();

    let modified_iso = meta
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
        .unwrap_or_default();

    let mut dimensions: Option<(u32, u32)> = None;
    let mut ahash: Option<String> = None;
    let mut exif_map = Map::new();
    let mut png_chunks = Map::new();

    if is_image_ext(path) {
        if let Ok(dims) = image::image_dimensions(path) {
            dimensions = Some(dims);
        }
        ahash = perceptual_hash(path).ok();
        exif_map = extract_exif_map(&bytes);
        png_chunks = extract_png_text_chunks(&bytes);
    }

    let date_time = exif_map
        .get("date_time_original")
        .or_else(|| exif_map.get("date_time"))
        .and_then(Value::as_str)
        .unwrap_or(&modified_iso)
        .to_string();
    let location = exif_map
        .get("gps_coordinates")
        .and_then(Value::as_str)
        .unwrap_or("Unknown")
        .to_string();
    let subject = exif_map
        .get("image_description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    Ok(json!({
        "file_name": file_name,
        "file_size_bytes": file_size,
        "file_size_human": human_size(file_size),
        "mime_type": mime,
        "date_time": date_time,
        "file_modified_utc": modified_iso,
        "location_gps": location,
        "subject": subject,
        "dimensions": dimensions.map(|(w, h)| json!({ "width": w, "height": h })),
        "sha256": sha256,
        "perceptual_hash": ahash,
        "exif": Value::Object(exif_map),
        "png_metadata": Value::Object(png_chunks),
    }))
}

/// Perform multi-signal authenticity & fraud detection (Original vs Exact Copy vs Near-Duplicate vs AI-Generated vs Edited).
pub fn analyze_authenticity(path: &Path, archive_roots: &[&Path]) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed reading {}: {e}", path.display()))?;
    let meta = extract_file_metadata(path)?;
    let target_sha = meta["sha256"].as_str().unwrap_or("").to_string();
    let target_ahash = meta["perceptual_hash"].as_str().map(String::from);
    let is_img = is_image_ext(path);
    let canonical_target = fs::canonicalize(path).ok();

    let mut exact_copies: Vec<Value> = Vec::new();
    let mut near_copies: Vec<Value> = Vec::new();

    for root in archive_roots {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root).into_iter().flatten() {
            let p = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            if let (Some(ct), Ok(cp)) = (&canonical_target, fs::canonicalize(p))
                && ct == &cp
            {
                continue;
            }
            let fname = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if fname.ends_with(".json") || fname.ends_with(".jsonl") {
                continue;
            }
            if let Ok(other_bytes) = fs::read(p) {
                let other_sha = sha256_bytes(&other_bytes);
                if other_sha == target_sha {
                    exact_copies.push(json!({
                        "matched_file": normalized_manifest_key(p),
                        "match_type": "exact_sha256_copy",
                        "sha256": other_sha,
                    }));
                    continue;
                }
            }
            if is_img
                && is_image_ext(p)
                && let (Some(h1), Ok(h2)) = (&target_ahash, perceptual_hash(p))
            {
                let dist = hash_distance(h1, &h2);
                if dist <= PERCEPTUAL_HASH_DISTANCE {
                    near_copies.push(json!({
                        "matched_file": normalized_manifest_key(p),
                        "match_type": "near_duplicate_perceptual_copy",
                        "hamming_distance": dist,
                    }));
                }
            }
        }
    }

    let (mut ai_hits, editor_hits, external_hits, has_c2pa_raw, mut has_iptc_ai_source) =
        scan_raw_markers(&bytes);
    if let Some(png_obj) = meta["png_metadata"].as_object() {
        for (k, v) in png_obj {
            let kl = k.to_ascii_lowercase();
            let vl = v.as_str().unwrap_or("").to_ascii_lowercase();
            if matches!(
                kl.as_str(),
                "parameters" | "prompt" | "workflow" | "sd-metadata"
            ) || AI_SOFTWARE_MARKERS.iter().any(|m| vl.contains(m))
            {
                ai_hits.push(format!("png_chunk:{k}"));
            }
        }
    }

    let jpeg_structure = inspect_jpeg_structure(&bytes);
    let pixel_forensics = if is_img {
        analyze_pixel_forensics(path)
    } else {
        None
    };

    let is_bare_stripped_jpeg = jpeg_structure
        .as_ref()
        .and_then(|j| j["is_bare_stripped_jpeg"].as_bool())
        .unwrap_or(false);
    let has_c2pa_jumbf = jpeg_structure
        .as_ref()
        .and_then(|j| j["has_c2pa_jumbf_box"].as_bool())
        .unwrap_or(false);
    let has_c2pa = has_c2pa_raw || has_c2pa_jumbf;
    let xmp_creator_tool = jpeg_structure
        .as_ref()
        .and_then(|j| j["xmp_creator_tool"].as_str())
        .unwrap_or("")
        .to_string();
    let xmp_digital_source = jpeg_structure
        .as_ref()
        .and_then(|j| j["xmp_digital_source_type"].as_str())
        .unwrap_or("")
        .to_string();
    if !xmp_digital_source.is_empty() {
        let low_src = xmp_digital_source.to_ascii_lowercase();
        if low_src.contains("trainedalgorithmicmedia")
            || low_src.contains("compositesynthetic")
            || low_src.contains("algorithmicmedia")
        {
            has_iptc_ai_source = true;
            ai_hits.push(format!("iptc_digital_source_type:{xmp_digital_source}"));
        }
    }
    if !xmp_creator_tool.is_empty() {
        let low_tool = xmp_creator_tool.to_ascii_lowercase();
        if AI_SOFTWARE_MARKERS.iter().any(|m| low_tool.contains(m)) {
            ai_hits.push(format!("xmp_creator_tool:{xmp_creator_tool}"));
        }
    }
    ai_hits.sort();
    ai_hits.dedup();

    let ela_splice_suspicious = pixel_forensics
        .as_ref()
        .and_then(|p| p["ela_splice_suspicious"].as_bool())
        .unwrap_or(false);
    let noise_inconsistent = pixel_forensics
        .as_ref()
        .and_then(|p| p["noise_inconsistent"].as_bool())
        .unwrap_or(false);
    let recompressed_block_artifacts = pixel_forensics
        .as_ref()
        .and_then(|p| p["recompressed_block_artifacts"].as_bool())
        .unwrap_or(false);
    let lsb_watermark_signal = pixel_forensics
        .as_ref()
        .and_then(|p| p["lsb_watermark_signal"].as_bool())
        .unwrap_or(false);

    let exif_obj = meta["exif"].as_object();
    let has_camera_exif = exif_obj.is_some_and(|e| {
        e.contains_key("camera_make")
            || e.contains_key("camera_model")
            || e.contains_key("lens_model")
            || e.contains_key("exposure_time")
    });
    let has_ai_dims = meta["dimensions"]
        .as_object()
        .and_then(|d| {
            let w = d.get("width")?.as_u64()? as u32;
            let h = d.get("height")?.as_u64()? as u32;
            Some(COMMON_AI_DIMS.contains(&(w, h)))
        })
        .unwrap_or(false);

    let mut signals: Vec<String> = Vec::new();
    if !exact_copies.is_empty() {
        signals.push(format!(
            "Exact SHA-256 cryptographic match found against {} archived file(s).",
            exact_copies.len()
        ));
    }
    if !near_copies.is_empty() {
        signals.push(format!(
            "Perceptual near-duplicate match (re-encoded/resized copy) found against {} archived image(s).",
            near_copies.len()
        ));
    }
    if !ai_hits.is_empty() {
        signals.push(format!(
            "Explicit AI-generation provenance markers detected: {}",
            ai_hits.join(", ")
        ));
    }
    if has_iptc_ai_source {
        signals.push(
            "IPTC/XMP DigitalSourceType declares AI-generated or composite synthetic media."
                .to_string(),
        );
    }
    if has_c2pa {
        signals.push(
            "C2PA / Content Credentials (JUMBF) provenance container present in file header."
                .to_string(),
        );
    }
    if !editor_hits.is_empty() {
        signals.push(format!(
            "Image editing software signature found: {}",
            editor_hits.join(", ")
        ));
    }
    if !external_hits.is_empty() {
        signals.push(format!(
            "External platform / stock / messaging source markers found: {}",
            external_hits.join(", ")
        ));
    }
    if is_bare_stripped_jpeg {
        let q_txt = jpeg_structure
            .as_ref()
            .and_then(|j| j["estimated_jpeg_quality"].as_u64())
            .map(|q| format!(" (est. JPEG quality ~{q})"))
            .unwrap_or_default();
        signals.push(format!(
            "Bare stripped JPEG container{q_txt}: ZERO APP/EXIF/JFIF/ICC/XMP metadata segments found. Authentic cameras/phones do not produce bare JPEGs — this strongly indicates a web/social-media re-encoded copy or a stripped AI/synthetic download."
        ));
    }
    if has_camera_exif {
        signals
            .push("Authentic hardware camera EXIF tags present (Make/Model/Exposure).".to_string());
    } else if is_img && !is_bare_stripped_jpeg {
        signals.push(
            "No camera hardware EXIF tags present (stripped, screenshot, or synthetic)."
                .to_string(),
        );
    }
    if has_ai_dims && !has_camera_exif {
        signals.push(
            "Exact standard diffusion/AI generator canvas dimensions without camera EXIF."
                .to_string(),
        );
    }
    if ela_splice_suspicious {
        let ratio = pixel_forensics
            .as_ref()
            .and_then(|p| p["ela_p95_to_median_ratio"].as_f64())
            .unwrap_or(0.0);
        signals.push(format!(
            "Error Level Analysis (ELA) flagged localized compression disparity (p95/median = {ratio:.2}), consistent with splicing, face/head grafting, or localized editing."
        ));
    }
    if noise_inconsistent {
        let cv = pixel_forensics
            .as_ref()
            .and_then(|p| p["noise_variation_cv"].as_f64())
            .unwrap_or(0.0);
        signals.push(format!(
            "Local high-pass sensor noise is spatially inconsistent across the image (CV = {cv:.2}), often seen in composites or AI-rendered regions."
        ));
    }
    if recompressed_block_artifacts && !has_camera_exif {
        let br = pixel_forensics
            .as_ref()
            .and_then(|p| p["jpeg_blockiness_ratio"].as_f64())
            .unwrap_or(1.0);
        signals.push(format!(
            "8x8 JPEG block-grid recompression artifacts detected (ratio = {br:.3}), indicating the image was re-saved or copied from another source."
        ));
    }
    if lsb_watermark_signal {
        signals.push(
            "LSB bit-plane exhibits unusually flat/uncorrelated distribution (possible steganographic or invisible digital watermark signal; not conclusive on its own)."
                .to_string(),
        );
    }

    let (verdict, plain_verdict, risk_level, confidence) = if !ai_hits.is_empty()
        || has_iptc_ai_source
    {
        (
            "LIKELY_AI_GENERATED",
            "Likely AI-generated image (explicit AI generator or IPTC/C2PA synthetic provenance detected)",
            "HIGH",
            0.95,
        )
    } else if !exact_copies.is_empty() {
        (
            "EXACT_COPY_OF_ARCHIVED_FILE",
            "Exact duplicate — this exact file already exists in the archive",
            "MEDIUM",
            1.0,
        )
    } else if !near_copies.is_empty() {
        (
            "NEAR_DUPLICATE_OR_REENCODED_COPY",
            "Possible copy — a visually similar image already exists in the archive",
            "MEDIUM",
            0.88,
        )
    } else if ela_splice_suspicious && (!has_camera_exif || noise_inconsistent) {
        (
            "SUSPECTED_MANIPULATED_OR_COMPOSITE",
            "Suspected digitally manipulated or spliced composite image (localized ELA compression disparity with missing camera provenance)",
            "HIGH",
            0.84,
        )
    } else if !editor_hits.is_empty() {
        (
            "EDITED_OR_PROCESSED",
            "Edited or modified using photo-editing software",
            "MEDIUM",
            0.82,
        )
    } else if has_ai_dims && !has_camera_exif {
        (
            "SUSPECTED_AI_GENERATED_OR_SYNTHETIC",
            "Possibly AI-generated or synthetic (standard AI canvas dimensions with no camera details)",
            "MEDIUM",
            0.72,
        )
    } else if !external_hits.is_empty()
        || is_bare_stripped_jpeg
        || (recompressed_block_artifacts && !has_camera_exif)
    {
        (
            "COPIED_OR_REENCODED_FROM_EXTERNAL_SOURCE",
            "Copied or re-encoded from an external/web source (all original camera metadata has been stripped — may be a forwarded copy or a stripped AI/manipulated image)",
            "MEDIUM",
            0.76,
        )
    } else if has_camera_exif {
        (
            "LIKELY_ORIGINAL_CAMERA_CAPTURE",
            "Likely an original camera photograph (original camera hardware EXIF details intact)",
            "LOW",
            0.86,
        )
    } else {
        (
            "UNVERIFIED_NO_CAMERA_METADATA",
            "Unverified source — no camera provenance found (common for screenshots, web downloads, or stripped AI images; treat with caution)",
            "MEDIUM",
            0.55,
        )
    };

    Ok(json!({
        "verdict": verdict,
        "plain_language_verdict": plain_verdict,
        "fraud_risk": risk_level,
        "confidence": confidence,
        "is_exact_copy": !exact_copies.is_empty(),
        "is_near_duplicate_copy": !near_copies.is_empty(),
        "ai_markers_detected": ai_hits,
        "editor_markers_detected": editor_hits,
        "external_source_markers_detected": external_hits,
        "c2pa_present": has_c2pa,
        "iptc_ai_source_present": has_iptc_ai_source,
        "camera_exif_present": has_camera_exif,
        "matched_exact_copies": exact_copies,
        "matched_near_duplicates": near_copies,
        "watermark_and_provenance": {
            "c2pa_content_credentials_present": has_c2pa,
            "iptc_ai_digital_source_type": if xmp_digital_source.is_empty() { Value::Null } else { Value::String(xmp_digital_source) },
            "xmp_creator_tool": if xmp_creator_tool.is_empty() { Value::Null } else { Value::String(xmp_creator_tool) },
            "ai_metadata_markers": ai_hits,
            "external_source_markers": external_hits,
            "editor_markers": editor_hits,
            "lsb_watermark_signal": lsb_watermark_signal,
            "invisible_watermark_note": "Checks open C2PA/JUMBF, IPTC DigitalSourceType, PNG/XMP AI tags, visible watermarks, and LSB statistical anomalies. Proprietary cryptographic pixel watermarks (such as Google SynthID) cannot be decoded locally without Google's private verification API.",
        },
        "jpeg_structure": jpeg_structure,
        "pixel_forensics": pixel_forensics,
        "forensic_signals": signals,
        "visual_inspection_instruction": "Present the verdict and fraud_risk faithfully in friendly, plain language. Explain metadata/watermark findings, JPEG/pixel checks, and visual AI findings clearly. NEVER downgrade a HIGH or MEDIUM risk verdict to claim the image is a real or original photo.",
        "metadata": meta,
    }))
}

/// Fuses a dedicated local Vision-AI forensic audit (scene observation, physical plausibility,
/// garbled AI text, visible watermarks, screenshot cues) into the file/metadata/pixel forensic report.
pub fn fuse_visual_audit(report: &mut Value, mut visual: Value) {
    let Some(obj) = report.as_object_mut() else {
        return;
    };
    let visible_wm = visual["visible_watermark"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    let wm_low = visible_wm.to_ascii_lowercase();
    let has_visible_wm = !wm_low.is_empty()
        && !matches!(
            wm_low.as_str(),
            "none" | "no" | "n/a" | "null" | "not detected" | "none detected"
        )
        && !wm_low.contains("exact logo name");
    let is_ai_wm = has_visible_wm
        && [
            "gemini",
            "veo",
            "sora",
            "kling",
            "meta ai",
            "midjourney",
            "dall-e",
            "dalle",
            "runway",
            "firefly",
            "synthid",
            "grok",
            "leonardo",
            "flux",
            "sparkle",
            "bing",
            "copilot",
            "chatgpt",
            "openai",
            "luma",
            "pika",
            "hailuo",
            "minimax",
            "seedance",
            "ai",
        ]
        .iter()
        .any(|k| wm_low.contains(k));
    let is_stock_or_social_wm = has_visible_wm && !is_ai_wm;

    let physically_impossible = visual["physically_possible_in_real_life"]
        .as_bool()
        .is_some_and(|v| !v);
    let has_garbled_text = visual["has_garbled_ai_text"].as_bool().unwrap_or(false);
    let is_screenshot = visual["is_screenshot_or_repost"].as_bool().unwrap_or(false);

    // Deterministically compute visual_assessment from factual booleans if not explicitly supplied.
    let explicit_assessment = visual["visual_assessment"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    let assessment = if physically_impossible || has_garbled_text || is_ai_wm {
        "LIKELY_AI_OR_MANIPULATED".to_string()
    } else if is_screenshot || is_stock_or_social_wm {
        "COPIED_OR_SCREENSHOT".to_string()
    } else if !explicit_assessment.is_empty() {
        explicit_assessment
    } else {
        "REAL_PHOTO".to_string()
    };
    if let Some(vobj) = visual.as_object_mut() {
        vobj.insert(
            "visual_assessment".to_string(),
            Value::String(assessment.clone()),
        );
    }

    let reason = visual["reason"].as_str().unwrap_or("").trim().to_string();
    let anomalies: Vec<String> = visual["anomalies"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    // Update watermark summary with visible watermark result
    if let Some(wp) = obj
        .get_mut("watermark_and_provenance")
        .and_then(Value::as_object_mut)
    {
        wp.insert(
            "visible_watermark_detected".to_string(),
            if has_visible_wm {
                Value::String(visible_wm.clone())
            } else {
                Value::String("none".to_string())
            },
        );
    }

    // Append visual signals to forensic_signals
    if let Some(sigs) = obj
        .get_mut("forensic_signals")
        .and_then(Value::as_array_mut)
    {
        if has_visible_wm {
            sigs.push(Value::String(format!(
                "Visible watermark/logo detected by Visual AI audit: {visible_wm}"
            )));
        }
        if physically_impossible {
            sigs.push(Value::String(
                "Visual Forensic AI Audit: scene depicts a physically impossible or surreal composite/hybrid.".to_string(),
            ));
        }
        if has_garbled_text {
            sigs.push(Value::String(
                "Visual Forensic AI Audit: garbled or distorted AI-style text/characters detected in the image.".to_string(),
            ));
        }
        if !reason.is_empty() {
            sigs.push(Value::String(format!(
                "Visual Forensic AI Audit ({assessment}): {reason}"
            )));
        }
        for a in anomalies.iter().take(4) {
            sigs.push(Value::String(format!("Visual anomaly: {a}")));
        }
    }

    let cur_verdict = obj
        .get("verdict")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let cur_conf = obj.get("confidence").and_then(Value::as_f64).unwrap_or(0.5);

    if (assessment == "LIKELY_AI_OR_MANIPULATED" || is_ai_wm)
        && (cur_verdict != "EXACT_COPY_OF_ARCHIVED_FILE" || is_ai_wm)
    {
        let new_verdict = if is_ai_wm || cur_verdict == "LIKELY_AI_GENERATED" {
            "LIKELY_AI_GENERATED"
        } else {
            "SUSPECTED_AI_OR_MANIPULATED"
        };
        let detail = if !reason.is_empty() {
            reason.clone()
        } else if !anomalies.is_empty() {
            anomalies.join("; ")
        } else {
            "synthetic or digitally manipulated visual features detected".to_string()
        };
        obj.insert(
            "verdict".to_string(),
            Value::String(new_verdict.to_string()),
        );
        obj.insert("fraud_risk".to_string(), Value::String("HIGH".to_string()));
        obj.insert("confidence".to_string(), json!(cur_conf.max(0.90)));
        obj.insert(
            "plain_language_verdict".to_string(),
            Value::String(format!(
                "Likely AI-generated or digitally manipulated ({detail})"
            )),
        );
    } else if (assessment == "COPIED_OR_SCREENSHOT" || is_stock_or_social_wm)
        && matches!(
            cur_verdict.as_str(),
            "LIKELY_ORIGINAL_CAMERA_CAPTURE" | "UNVERIFIED_NO_CAMERA_METADATA"
        )
    {
        let detail = if has_visible_wm {
            format!("visible external/stock watermark '{visible_wm}' detected")
        } else if !reason.is_empty() {
            reason.clone()
        } else {
            "screenshot or external-source framing cues detected".to_string()
        };
        obj.insert(
            "verdict".to_string(),
            Value::String("COPIED_OR_REENCODED_FROM_EXTERNAL_SOURCE".to_string()),
        );
        obj.insert(
            "fraud_risk".to_string(),
            Value::String("MEDIUM".to_string()),
        );
        obj.insert("confidence".to_string(), json!(cur_conf.max(0.80)));
        obj.insert(
            "plain_language_verdict".to_string(),
            Value::String(format!(
                "Copied or taken from an external source ({detail})"
            )),
        );
    }

    obj.insert("visual_forensic_audit".to_string(), visual);
}

fn normalized_manifest_key(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if let Some(idx) = s.find("/data/processed/") {
        return s[idx + "/data/processed/".len()..].to_string();
    }
    s
}

/// Checks if raw uploaded bytes already exist in the archive (exact SHA-256 or perceptual aHash for images).
pub fn find_duplicate_in_archives(
    bytes: &[u8],
    orig_filename: &str,
    archive_roots: &[&Path],
) -> Option<Value> {
    let target_sha = sha256_bytes(bytes);
    let is_img = is_image_ext(Path::new(orig_filename));
    let target_ahash = if is_img {
        image::load_from_memory(bytes).ok().map(|img| {
            let small = img
                .grayscale()
                .resize_exact(8, 8, image::imageops::FilterType::Triangle);
            let pixels = small.to_luma8();
            let sum: u64 = pixels.pixels().map(|p| u64::from(p[0])).sum();
            let mean = (sum as f64) / 64.0;
            let mut bits = 0u64;
            for (idx, p) in pixels.pixels().enumerate() {
                if f64::from(p[0]) >= mean {
                    bits |= 1u64 << idx;
                }
            }
            format!("{bits:016x}")
        })
    } else {
        None
    };

    for root in archive_roots {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root).into_iter().flatten() {
            let p = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            let fname = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if fname.ends_with(".json") || fname.ends_with(".jsonl") {
                continue;
            }
            if let Ok(other_bytes) = fs::read(p) {
                let other_sha = sha256_bytes(&other_bytes);
                if other_sha == target_sha {
                    return Some(json!({
                        "duplicate_type": "exact_sha256",
                        "sha256": target_sha,
                        "existing_file": normalized_manifest_key(p),
                        "existing_filename": fname,
                    }));
                }
            }
            if is_img
                && is_image_ext(p)
                && let (Some(h1), Ok(h2)) = (&target_ahash, perceptual_hash(p))
            {
                let dist = hash_distance(h1, &h2);
                if dist <= 6 {
                    return Some(json!({
                        "duplicate_type": "near_duplicate_perceptual",
                        "sha256": target_sha,
                        "hamming_distance": dist,
                        "existing_file": normalized_manifest_key(p),
                        "existing_filename": fname,
                    }));
                }
            }
        }
    }
    None
}

/// Registers or updates a single file's SHA-256 checksum inside `checksum_manifest.json`.
pub fn register_checksum_entry(
    manifest_path: &Path,
    file_path: &Path,
    sha256: &str,
    size_bytes: u64,
) {
    let mut entries: Map<String, Value> = fs::read_to_string(manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("files").and_then(Value::as_object).cloned())
        .unwrap_or_default();
    let key = normalized_manifest_key(file_path);
    entries.insert(
        key,
        json!({
            "sha256": sha256,
            "size_bytes": size_bytes,
            "recorded_at": Utc::now().to_rfc3339(),
        }),
    );
    let doc = json!({
        "updated_at": Utc::now().to_rfc3339(),
        "total_tracked": entries.len(),
        "files": Value::Object(entries),
    });
    if let Ok(pretty) = serde_json::to_string_pretty(&doc) {
        let tmp = manifest_path.with_extension("json.tmp");
        if fs::write(&tmp, &pretty).is_ok() {
            let _ = fs::rename(&tmp, manifest_path);
        }
    }
}

/// Audits SHA-256 file integrity across all archival roots (`profiles/`, `locations/`, `documents/`)
/// against `checksum_manifest.json`, detecting tampered/corrupted files, missing files, and exact duplicates.
pub fn verify_archive_integrity(
    manifest_path: &Path,
    archive_roots: &[&Path],
    update_manifest: bool,
) -> Result<Value, String> {
    let prev_entries: Map<String, Value> = fs::read_to_string(manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("files").and_then(Value::as_object).cloned())
        .unwrap_or_default();

    let mut current_files: BTreeMap<String, (String, u64)> = BTreeMap::new();
    let mut sha_groups: HashMap<String, Vec<String>> = HashMap::new();

    for root in archive_roots {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root).into_iter().flatten() {
            let p = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            let fname = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if fname.ends_with(".json") || fname.ends_with(".jsonl") {
                continue;
            }
            let bytes = fs::read(p).map_err(|e| format!("Failed reading {}: {e}", p.display()))?;
            let sha = sha256_bytes(&bytes);
            let key = normalized_manifest_key(p);
            current_files.insert(key.clone(), (sha.clone(), bytes.len() as u64));
            sha_groups.entry(sha).or_default().push(key);
        }
    }

    let mut verified_count = 0usize;
    let mut new_indexed: Vec<String> = Vec::new();
    let mut corrupted: Vec<Value> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let mut next_entries = prev_entries.clone();

    for (key, (cur_sha, size)) in &current_files {
        if let Some(prev) = prev_entries.get(key) {
            let expected_sha = prev["sha256"].as_str().unwrap_or("");
            if expected_sha == cur_sha {
                verified_count += 1;
            } else {
                corrupted.push(json!({
                    "file": key,
                    "expected_sha256": expected_sha,
                    "actual_sha256": cur_sha,
                    "size_bytes": size,
                }));
            }
        } else {
            new_indexed.push(key.clone());
            next_entries.insert(
                key.clone(),
                json!({
                    "sha256": cur_sha,
                    "size_bytes": size,
                    "recorded_at": Utc::now().to_rfc3339(),
                }),
            );
        }
    }

    for key in prev_entries.keys() {
        if !current_files.contains_key(key) {
            missing.push(key.clone());
            if update_manifest {
                next_entries.remove(key);
            }
        }
    }

    let mut duplicate_groups: Vec<Value> = sha_groups
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .map(|(sha, mut paths)| {
            paths.sort();
            json!({
                "sha256": sha,
                "count": paths.len(),
                "files": paths,
            })
        })
        .collect();
    duplicate_groups.sort_by(|a, b| b["count"].as_u64().cmp(&a["count"].as_u64()));

    if update_manifest || !manifest_path.is_file() {
        let doc = json!({
            "updated_at": Utc::now().to_rfc3339(),
            "total_tracked": next_entries.len(),
            "files": Value::Object(next_entries),
        });
        let pretty = serde_json::to_string_pretty(&doc).map_err(|e| format!("JSON err: {e}"))?;
        let tmp = manifest_path.with_extension("json.tmp");
        fs::write(&tmp, &pretty).map_err(|e| format!("Write manifest failed: {e}"))?;
        fs::rename(&tmp, manifest_path).map_err(|e| format!("Rename manifest failed: {e}"))?;
    }

    let integrity_status = if !corrupted.is_empty() {
        "CORRUPTION_OR_TAMPERING_DETECTED"
    } else if !missing.is_empty() {
        "MISSING_ARCHIVED_FILES"
    } else {
        "VERIFIED_INTACT"
    };

    Ok(json!({
        "integrity_status": integrity_status,
        "manifest_name": "checksum_manifest.json",
        "manifest_updated": update_manifest || !manifest_path.is_file(),
        "total_files_scanned": current_files.len(),
        "verified_intact_count": verified_count,
        "newly_indexed_count": new_indexed.len(),
        "corrupted_count": corrupted.len(),
        "missing_count": missing.len(),
        "duplicate_groups_count": duplicate_groups.len(),
        "corrupted_files": corrupted,
        "missing_files": missing,
        "duplicate_groups": duplicate_groups,
        "checked_at_utc": Utc::now().to_rfc3339(),
        "presentation_note": "Summarize results in plain, non-technical language for the user (e.g. 'All files are safe and intact', plus any duplicate items found). Do not expose internal server directory paths.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_and_marker_scan() {
        assert_eq!(
            sha256_bytes(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        let (ai, ed, ext, c2pa, iptc_ai) = scan_raw_markers(
            b"PNG ComfyUI workflow Stable Diffusion c2pa DigitalSourceType trainedAlgorithmicMedia FBMD",
        );
        assert!(!ai.is_empty());
        assert!(ed.is_empty());
        assert!(!ext.is_empty());
        assert!(c2pa);
        assert!(iptc_ai);
    }

    #[test]
    fn test_bare_stripped_jpeg_and_visual_fusion() {
        // Minimal bare JPEG: SOI + DQT(empty) + SOS -> zero APP/EXIF/JFIF/ICC segments
        let bare = [0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9];
        let st = inspect_jpeg_structure(&bare).expect("jpeg structure");
        assert_eq!(st["is_bare_stripped_jpeg"], true);
        assert_eq!(st["has_exif_segment"], false);

        let mut report = json!({
            "verdict": "COPIED_OR_REENCODED_FROM_EXTERNAL_SOURCE",
            "plain_language_verdict": "Copied or re-encoded",
            "fraud_risk": "MEDIUM",
            "confidence": 0.76,
            "watermark_and_provenance": {},
            "forensic_signals": [],
        });
        fuse_visual_audit(
            &mut report,
            json!({
                "scene_description": "A person with a robotic body outdoors.",
                "physically_possible_in_real_life": false,
                "has_garbled_ai_text": true,
                "visible_watermark": "none",
                "is_screenshot_or_repost": false,
                "anomalies": ["Human head grafted onto a humanoid robot body"],
                "reason": "Impossible human-head-on-robot composite with garbled AI text."
            }),
        );
        assert_eq!(report["verdict"], "SUSPECTED_AI_OR_MANIPULATED");
        assert_eq!(report["fraud_risk"], "HIGH");
        assert!(report["confidence"].as_f64().unwrap() >= 0.90);

        // Genuine portrait with normal real-life scene should stay LOW risk original capture
        let mut real_report = json!({
            "verdict": "LIKELY_ORIGINAL_CAMERA_CAPTURE",
            "plain_language_verdict": "Likely an original camera photograph",
            "fraud_risk": "LOW",
            "confidence": 0.86,
            "watermark_and_provenance": {},
            "forensic_signals": [],
        });
        fuse_visual_audit(
            &mut real_report,
            json!({
                "scene_description": "A smiling man outdoors.",
                "physically_possible_in_real_life": true,
                "has_garbled_ai_text": false,
                "visible_watermark": "none",
                "is_screenshot_or_repost": false,
                "anomalies": [],
                "reason": "Normal real-life scene."
            }),
        );
        assert_eq!(real_report["verdict"], "LIKELY_ORIGINAL_CAMERA_CAPTURE");
        assert_eq!(real_report["fraud_risk"], "LOW");
        assert_eq!(
            real_report["visual_forensic_audit"]["visual_assessment"],
            "REAL_PHOTO"
        );
    }

    #[test]
    fn test_checksum_integrity_audit_and_tamper_detection() {
        let base = std::env::temp_dir().join(format!(
            "rojakface_integrity_test_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let root = base.join("docs");
        fs::create_dir_all(&root).unwrap();
        let f1 = root.join("charter.txt");
        let f2 = root.join("charter_copy.txt");
        fs::write(&f1, b"Original archival text").unwrap();
        fs::write(&f2, b"Original archival text").unwrap();
        let manifest = base.join("checksum_manifest.json");

        // Initial baseline manifest creation
        let r1 = verify_archive_integrity(&manifest, &[root.as_path()], true).unwrap();
        assert_eq!(r1["integrity_status"], "VERIFIED_INTACT");
        assert_eq!(r1["newly_indexed_count"], 2);
        assert_eq!(r1["duplicate_groups_count"], 1);

        // Tamper with f1
        fs::write(&f1, b"Tampered archival text!").unwrap();
        let r2 = verify_archive_integrity(&manifest, &[root.as_path()], false).unwrap();
        assert_eq!(r2["integrity_status"], "CORRUPTION_OR_TAMPERING_DETECTED");
        assert_eq!(r2["corrupted_count"], 1);

        let _ = fs::remove_dir_all(&base);
    }
}
