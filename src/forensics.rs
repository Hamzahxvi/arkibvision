#![forbid(unsafe_code)]

//! Forensic image/file metadata extraction & authenticity verification:
//! - Full file metadata (name, size, MIME, dimensions, SHA-256, perceptual aHash)
//! - EXIF metadata (DateTimeOriginal, Camera Make/Model, Software, GPS coordinates, Artist/Description)
//! - PNG text chunk inspection (tEXt / iTXt / zTXt) & C2PA/XMP provenance marker scan
//! - Archive duplicate detection (SHA-256 exact copy & perceptual aHash near-duplicate copy)
//! - Multi-signal AI-generated / Original / Copy / Edited fraud assessment

use crate::config::{PERCEPTUAL_HASH_DISTANCE, is_image_ext};
use crate::vision_ops::{hash_distance, perceptual_hash};
use flate2::read::ZlibDecoder;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
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
    "adobe firefly",
    "imagen",
    "flux",
    "novelai",
    "invokeai",
    "leonardo.ai",
    "ideogram",
    "bing image creator",
    "generative fill",
    "ai generated",
    "ai-generated",
    "c2pa.ai_generative_training",
    "trainedalgorithmicmedia",
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

fn scan_raw_markers(bytes: &[u8]) -> (Vec<String>, Vec<String>, bool) {
    // Scan printable ASCII substrings (header + trailer up to 256 KB) for C2PA/XMP/AI strings.
    let mut sample = String::new();
    let head_len = bytes.len().min(131_072);
    sample.push_str(&String::from_utf8_lossy(&bytes[..head_len]));
    if bytes.len() > head_len {
        let tail_start = bytes.len().saturating_sub(65_536).max(head_len);
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
    let has_c2pa =
        low.contains("c2pa") || low.contains("jumb") || low.contains("contentcredentials");
    (ai_hits, editor_hits, has_c2pa)
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
                        "matched_file": p.display().to_string(),
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
                        "matched_file": p.display().to_string(),
                        "match_type": "near_duplicate_perceptual_copy",
                        "hamming_distance": dist,
                    }));
                }
            }
        }
    }

    let (mut ai_hits, editor_hits, has_c2pa) = scan_raw_markers(&bytes);
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
    ai_hits.sort();
    ai_hits.dedup();

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
    if has_c2pa {
        signals.push("C2PA / Content Credentials metadata header present.".to_string());
    }
    if !editor_hits.is_empty() {
        signals.push(format!(
            "Image editing software signature found: {}",
            editor_hits.join(", ")
        ));
    }
    if has_camera_exif {
        signals
            .push("Authentic hardware camera EXIF tags present (Make/Model/Exposure).".to_string());
    } else if is_img {
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

    let (verdict, risk_level, confidence) = if !ai_hits.is_empty() {
        ("LIKELY_AI_GENERATED", "HIGH", 0.94)
    } else if !exact_copies.is_empty() {
        ("EXACT_COPY_OF_ARCHIVED_FILE", "MEDIUM", 1.0)
    } else if !near_copies.is_empty() {
        ("NEAR_DUPLICATE_OR_REENCODED_COPY", "MEDIUM", 0.88)
    } else if !editor_hits.is_empty() {
        ("EDITED_OR_PROCESSED", "MEDIUM", 0.82)
    } else if has_ai_dims && !has_camera_exif {
        ("SUSPECTED_AI_GENERATED_OR_SYNTHETIC", "MEDIUM", 0.68)
    } else if has_camera_exif {
        ("LIKELY_ORIGINAL_CAMERA_CAPTURE", "LOW", 0.85)
    } else {
        ("INCONCLUSIVE_UNVERIFIED_SOURCE", "LOW", 0.50)
    };

    Ok(json!({
        "verdict": verdict,
        "fraud_risk": risk_level,
        "confidence": confidence,
        "is_exact_copy": !exact_copies.is_empty(),
        "is_near_duplicate_copy": !near_copies.is_empty(),
        "ai_markers_detected": ai_hits,
        "editor_markers_detected": editor_hits,
        "c2pa_present": has_c2pa,
        "camera_exif_present": has_camera_exif,
        "matched_exact_copies": exact_copies,
        "matched_near_duplicates": near_copies,
        "forensic_signals": signals,
        "metadata": meta,
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
        let (ai, ed, c2pa) = scan_raw_markers(b"PNG ComfyUI workflow Stable Diffusion c2pa");
        assert!(!ai.is_empty());
        assert!(ed.is_empty());
        assert!(c2pa);
    }
}
