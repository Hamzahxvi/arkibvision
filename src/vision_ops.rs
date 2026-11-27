#![forbid(unsafe_code)]

use crate::config::AppConfig;
use crate::metadata::profile_slug;
use base64::Engine as _;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, GrayImage, ImageBuffer, Rgb, RgbImage};
use serde_json::{Value, json};
use std::fs;
use std::io::Cursor;
use std::path::Path;

pub fn load_rgb_image(path: &Path) -> Result<DynamicImage, String> {
    let bytes =
        fs::read(path).map_err(|e| format!("Failed reading image {}: {e}", path.display()))?;
    load_rgb_image_from_bytes(&bytes)
}

pub fn load_rgb_image_from_bytes(bytes: &[u8]) -> Result<DynamicImage, String> {
    let img =
        image::load_from_memory(bytes).map_err(|e| format!("Failed decoding image bytes: {e}"))?;
    let orientation = read_exif_orientation(bytes).unwrap_or(1);
    let oriented = match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    };
    Ok(DynamicImage::ImageRgb8(oriented.to_rgb8()))
}

fn read_exif_orientation(bytes: &[u8]) -> Option<u32> {
    let mut cursor = Cursor::new(bytes);
    let exif = exif::Reader::new().read_from_container(&mut cursor).ok()?;
    let field = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
    field.value.get_uint(0)
}

pub fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let rgb = img.to_rgb8();
    let mut buf = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, quality);
    encoder
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("JPEG encode failed: {e}"))?;
    Ok(buf)
}

pub fn encode_image_b64(path: &Path, max_dim: u32) -> Result<String, String> {
    let img = load_rgb_image(path)?;
    let resized = img.thumbnail(max_dim, max_dim);
    let jpeg = encode_jpeg(&resized, 80)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(jpeg))
}

/// Fast in-memory thumbnail + JPEG base64 encoder for local Vision LLM calls.
/// Produces 5–10x smaller payloads than PNG, cutting Ollama vision latency and token overhead.
pub fn encode_image_bytes_b64(bytes: &[u8], max_dim: u32, quality: u8) -> Result<String, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("Image decode failed: {e}"))?;
    let resized = if img.width() > max_dim || img.height() > max_dim {
        img.thumbnail(max_dim, max_dim)
    } else {
        img
    };
    let jpeg = encode_jpeg(&resized, quality)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(jpeg))
}

pub fn compute_ahash_bits(pixels: &[i64]) -> String {
    if pixels.is_empty() {
        return String::new();
    }
    let sum: i128 = pixels.iter().map(|&p| i128::from(p)).sum();
    let average = (sum as f64) / (pixels.len() as f64);
    let min_pixel = average.ceil() as i128;
    let mut out = String::with_capacity(pixels.len());
    for &pixel in pixels {
        out.push(if i128::from(pixel) >= min_pixel {
            '1'
        } else {
            '0'
        });
    }
    out
}

pub fn hash_distance(left: &str, right: &str) -> i64 {
    if left.is_ascii() && right.is_ascii() {
        left.bytes()
            .zip(right.bytes())
            .filter(|(a, b)| a != b)
            .count() as i64
    } else {
        left.chars()
            .zip(right.chars())
            .filter(|(a, b)| a != b)
            .count() as i64
    }
}

pub fn perceptual_hash_from_image(img: &DynamicImage) -> String {
    let gray: GrayImage = img
        .grayscale()
        .resize_exact(16, 16, FilterType::Triangle)
        .to_luma8();
    let pixels: Vec<i64> = gray.as_raw().iter().map(|&b| i64::from(b)).collect();
    compute_ahash_bits(&pixels)
}

pub fn perceptual_hash_from_bytes(bytes: &[u8]) -> Result<String, String> {
    let img = load_rgb_image_from_bytes(bytes)?;
    Ok(perceptual_hash_from_image(&img))
}

pub fn perceptual_hash(path: &Path) -> Result<String, String> {
    let img = load_rgb_image(path)?;
    Ok(perceptual_hash_from_image(&img))
}

pub fn compute_text_line_chunks(row_energies: &[f64], height: i64, max_chunks: i64) -> Value {
    if height <= 0 || row_energies.is_empty() {
        return json!({ "detected_text_lines": 0, "chunk_count": 0, "chunks": [] });
    }

    let clamped_max = max_chunks.clamp(1, 8) as usize;
    let n = row_energies.len();
    let win = (height / 150).max(3);
    let half_left = win / 2;
    let half_right = (win - 1) / 2;
    let inv_win = 1.0 / (win as f64);

    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0_f64);
    let mut acc = 0.0_f64;
    for &v in row_energies {
        acc += if v.is_finite() { v } else { 0.0 };
        prefix.push(acc);
    }

    let n_i64 = n as i64;
    let mut smooth = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i as i64;
        let lo = idx.saturating_sub(half_left).max(0) as usize;
        let hi = idx.saturating_add(half_right).saturating_add(1).min(n_i64) as usize;
        smooth.push((prefix[hi] - prefix[lo]) * inv_win);
    }

    let mut sorted_vals = smooth.clone();
    sorted_vals.sort_by(f64::total_cmp);
    let pos = 0.30 * ((sorted_vals.len() - 1) as f64);
    let lo_i = pos as usize;
    let hi_i = (lo_i + 1).min(sorted_vals.len() - 1);
    let frac = pos - (lo_i as f64);
    let p30 = sorted_vals[lo_i] * (1.0 - frac) + sorted_vals[hi_i] * frac;
    let thresh = 3.5_f64.max(p30 * 1.35);

    let min_line_h = (height / 140).max(4);
    let mut lines: Vec<(i64, i64)> = Vec::new();
    let mut in_line = false;
    let mut start_y: i64 = 0;
    for (y, &val) in smooth.iter().enumerate() {
        let y_i64 = y as i64;
        let active = val >= thresh;
        if active && !in_line {
            in_line = true;
            start_y = y_i64;
        } else if !active && in_line {
            in_line = false;
            if y_i64 - start_y >= min_line_h {
                lines.push((start_y, y_i64));
            }
        }
    }
    if in_line && n_i64 - start_y >= min_line_h {
        lines.push((start_y, n_i64));
    }

    // Keep each visual strip to ~3-4 lines so 4B VLMs do not drift or hallucinate on dense Jawi/manuscript pages.
    let num_chunks = if lines.len() <= 4 || height < 420 || clamped_max == 1 {
        1
    } else if lines.len() <= 8 || height < 750 {
        2.min(clamped_max)
    } else if lines.len() <= 12 {
        3.min(clamped_max)
    } else {
        lines.len().div_ceil(4).min(clamped_max)
    };

    let mut chunk_groups: Vec<&[(i64, i64)]> = Vec::new();
    if num_chunks == 1 || lines.len() < num_chunks * 2 {
        chunk_groups.push(&lines);
    } else {
        let mut split_indices: Vec<usize> = Vec::with_capacity(num_chunks - 1);
        let radius = (lines.len() / 6).max(1);
        for k in 1..num_chunks {
            let target =
                ((k * lines.len()) as f64 / (num_chunks as f64)).round_ties_even() as usize;
            let lo = target.saturating_sub(radius).max(1);
            let hi = (target + radius).min(lines.len() - 1);
            let mut best_idx = target;
            let mut best_gap: i64 = -1;
            for idx in lo..=hi {
                if split_indices.last().is_some_and(|&prev| idx <= prev) {
                    continue;
                }
                let gap = lines[idx].0 - lines[idx - 1].1;
                if gap > best_gap {
                    best_gap = gap;
                    best_idx = idx;
                }
            }
            split_indices.push(best_idx);
        }
        let mut prev = 0;
        for s_idx in split_indices {
            chunk_groups.push(&lines[prev..s_idx]);
            prev = s_idx;
        }
        chunk_groups.push(&lines[prev..]);
    }

    let pad = 12_i64.max(((height as f64) * 0.03) as i64);
    let total_groups = chunk_groups.len();
    let mut chunks = Vec::with_capacity(total_groups);
    for (i, group) in chunk_groups.iter().enumerate() {
        let idx = i + 1;
        let mut y0 = if let Some(&(first_y, _)) = group.first() {
            (first_y - pad).max(0)
        } else {
            (((idx - 1) as f64 * (height as f64)) / (total_groups as f64)) as i64
        };
        let mut y1 = if let Some(&(_, last_y)) = group.last() {
            last_y.saturating_add(pad).min(height)
        } else {
            ((idx as f64 * (height as f64)) / (total_groups as f64)) as i64
        };
        if idx == 1 {
            y0 = 0;
        }
        if idx == total_groups {
            y1 = height;
        }
        let label = match (total_groups, i) {
            (1, 0) => "Full Document".to_string(),
            (2, 0) => "Top Section".to_string(),
            (2, 1) => "Bottom Section".to_string(),
            (3, 0) => "Top Section".to_string(),
            (3, 1) => "Middle Section".to_string(),
            (3, 2) => "Bottom Section".to_string(),
            _ => format!("Section {idx}"),
        };
        let denom = height.max(1) as f64;
        let pct_start = ((100_i128 * i128::from(y0)) as f64 / denom).round_ties_even() as i64;
        let pct_end = ((100_i128 * i128::from(y1)) as f64 / denom).round_ties_even() as i64;
        chunks.push(json!({
            "chunk_index": idx,
            "region_label": label,
            "y0": y0,
            "y1": y1.max(y0.saturating_add(1)),
            "vertical_range_pct": format!("{pct_start}%-{pct_end}%"),
            "detected_text_lines": if group.is_empty() { 1 } else { group.len() },
        }));
    }

    json!({
        "detected_text_lines": lines.len(),
        "chunk_count": chunks.len(),
        "chunks": chunks,
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

fn sharpen_rgb(img: &RgbImage, amount: f32) -> RgbImage {
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

const OCR_STRIP_MAX_WIDTH: u32 = 1024;

/// Detects classical Malay red-ink rubrication (*dakwat merah*, e.g. Fasal, Bab, Bermula, Alkisah)
/// before grayscaling. Returns `(has_rubrication, red_ink_pct_of_foreground)`.
pub fn detect_red_ink_rubrication(img: &RgbImage) -> (bool, f64) {
    let mut fg_pixels = 0usize;
    let mut red_pixels = 0usize;
    for p in img.pixels() {
        let [r, g, b] = p.0;
        let is_red =
            r > 140 && f32::from(r) > f32::from(g) * 1.45 && f32::from(r) > f32::from(b) * 1.45;
        let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
        if is_red || luma < 195.0 {
            fg_pixels += 1;
        }
        if is_red {
            red_pixels += 1;
        }
    }
    if fg_pixels == 0 {
        return (false, 0.0);
    }
    let ratio = (red_pixels as f64) / (fg_pixels as f64);
    let pct = (ratio * 10_000.0).round() / 100.0;
    (red_pixels >= 12 && ratio >= 0.003, pct)
}

/// Decides adaptively whether an OCR strip is an aged/yellowed/stained manuscript that benefits
/// from Sauvola adaptive thresholding instead of standard global autocontrast.
fn needs_manuscript_sauvola(img: &RgbImage) -> bool {
    let (w, h) = img.dimensions();
    if w < 16 || h < 16 {
        return false;
    }
    let step_x = (w / 48).max(1);
    let step_y = (h / 32).max(1);
    let mut sum_r = 0.0_f64;
    let mut sum_b = 0.0_f64;
    let mut sum_luma = 0.0_f64;
    let mut count = 0.0_f64;
    let mut tile_sums = [0.0_f64; 16];
    let mut tile_cnts = [0.0_f64; 16];

    for y in (0..h).step_by(step_y as usize) {
        let ty = ((y * 4) / h).min(3) as usize;
        for x in (0..w).step_by(step_x as usize) {
            let tx = ((x * 4) / w).min(3) as usize;
            let [r, g, b] = img.get_pixel(x, y).0;
            let luma = 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b);
            sum_r += f64::from(r);
            sum_b += f64::from(b);
            sum_luma += luma;
            count += 1.0;
            let tidx = ty * 4 + tx;
            tile_sums[tidx] += luma;
            tile_cnts[tidx] += 1.0;
        }
    }
    if count <= 0.0 {
        return false;
    }
    let mean_luma = sum_luma / count;
    let yellowing = (sum_r - sum_b) / count;
    let mut min_tile = 255.0_f64;
    let mut max_tile = 0.0_f64;
    for i in 0..16 {
        if tile_cnts[i] > 0.0 {
            let tm = tile_sums[i] / tile_cnts[i];
            min_tile = min_tile.min(tm);
            max_tile = max_tile.max(tm);
        }
    }
    let uneven_bg = (max_tile - min_tile) > 36.0;
    yellowing > 14.0 || mean_luma < 175.0 || uneven_bg
}

/// Pure-Rust O(1) per-pixel integral-image Sauvola adaptive thresholding blended 70% Sauvola +
/// 30% locally normalized grayscale so paper stains & bleed-through are suppressed while tiny
/// Jawi diacritical dots (*titik* on ب، ت، ث، ن، ي، ڤ، ڠ، ݢ، چ، ڽ) remain intact.
pub fn sauvola_blend_70_30(img: &RgbImage, window: usize, k: f64, r_dyn: f64) -> RgbImage {
    let (w_u32, h_u32) = img.dimensions();
    let w = w_u32 as usize;
    let h = h_u32 as usize;
    if w == 0 || h == 0 {
        return img.clone();
    }

    let stride = w + 1;
    let mut gray = vec![0.0_f64; w * h];
    let mut sat = vec![0.0_f64; stride * (h + 1)];
    let mut sat_sq = vec![0.0_f64; stride * (h + 1)];

    for y in 0..h {
        let mut row_sum = 0.0_f64;
        let mut row_sq = 0.0_f64;
        for x in 0..w {
            let [r, g, b] = img.get_pixel(x as u32, y as u32).0;
            let lum = 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b);
            gray[y * w + x] = lum;
            row_sum += lum;
            row_sq += lum * lum;
            let idx = (y + 1) * stride + (x + 1);
            let above = y * stride + (x + 1);
            sat[idx] = sat[above] + row_sum;
            sat_sq[idx] = sat_sq[above] + row_sq;
        }
    }

    let half = (window.max(3)) / 2;
    let r_safe = r_dyn.max(1.0);
    ImageBuffer::from_fn(w_u32, h_u32, |x_u32, y_u32| {
        let x = x_u32 as usize;
        let y = y_u32 as usize;
        let x0 = x.saturating_sub(half);
        let y0 = y.saturating_sub(half);
        let x1 = (x + half + 1).min(w);
        let y1 = (y + half + 1).min(h);
        let area = ((x1 - x0) * (y1 - y0)).max(1) as f64;

        let s = sat[y1 * stride + x1] - sat[y0 * stride + x1] - sat[y1 * stride + x0]
            + sat[y0 * stride + x0];
        let sq = sat_sq[y1 * stride + x1] - sat_sq[y0 * stride + x1] - sat_sq[y1 * stride + x0]
            + sat_sq[y0 * stride + x0];
        let mean = s / area;
        let var = (sq / area - mean * mean).max(0.0);
        let std_dev = var.sqrt();

        let lum = gray[y * w + x];
        let thresh = mean * (1.0 + k * ((std_dev / r_safe) - 1.0));
        let sauvola = if lum < thresh { 0.0 } else { 255.0 };
        let local_bg = (mean + std_dev * 0.5).max(32.0);
        let local_norm = ((lum / local_bg) * 245.0).clamp(0.0, 255.0);
        let blended = (0.70 * sauvola + 0.30 * local_norm)
            .round()
            .clamp(0.0, 255.0) as u8;
        Rgb([blended, blended, blended])
    })
}

pub fn build_ocr_document_chunks(
    cfg: &AppConfig,
    image_path: &Path,
    max_chunks: i64,
) -> Result<Value, String> {
    let base_img = load_rgb_image(image_path)?;
    let (width, height) = base_img.dimensions();
    let gray = base_img.to_luma8();

    let mut row_energy = vec![0.0_f64; height as usize];
    if width > 2 {
        let denom = (width - 1) as f64;
        for y in 0..height {
            let mut sum = 0.0_f64;
            for x in 1..width {
                let a = i32::from(gray.get_pixel(x, y)[0]);
                let b = i32::from(gray.get_pixel(x - 1, y)[0]);
                sum += f64::from((a - b).abs());
            }
            row_energy[y as usize] = sum / denom;
        }
    }

    let plan = compute_text_line_chunks(&row_energy, i64::from(height), max_chunks);
    let planned_chunks = plan["chunks"].as_array().cloned().unwrap_or_default();
    let total_detected_lines = plan["detected_text_lines"].as_u64().unwrap_or(0);

    let out_dir = cfg.librechat_dir.join("images").join("ocr-chunks");
    let _ = fs::create_dir_all(&out_dir);
    let raw_stem = image_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("doc");
    let stem: String = profile_slug(raw_stem).chars().take(48).collect();
    let stem = if stem.is_empty() {
        "doc".to_string()
    } else {
        stem
    };

    let mut chunks = Vec::new();
    let mut any_rubrication = false;
    for item in planned_chunks {
        let idx = item["chunk_index"].as_u64().unwrap_or(1);
        let y0 = item["y0"].as_i64().unwrap_or(0).clamp(0, i64::from(height)) as u32;
        let y1 = item["y1"]
            .as_i64()
            .unwrap_or(i64::from(height))
            .clamp(i64::from(y0 + 1).min(i64::from(height)), i64::from(height))
            as u32;
        let crop_h = y1.saturating_sub(y0).max(1);
        let raw_crop = base_img.crop_imm(0, y0, width, crop_h).to_rgb8();

        // Cap strip width to 1024px (aspect ratio preserved) to keep VLM vision tokens lean.
        let crop = if raw_crop.width() > OCR_STRIP_MAX_WIDTH {
            let new_h = ((u64::from(raw_crop.height()) * u64::from(OCR_STRIP_MAX_WIDTH))
                / u64::from(raw_crop.width().max(1)))
            .max(1) as u32;
            DynamicImage::ImageRgb8(raw_crop)
                .resize_exact(OCR_STRIP_MAX_WIDTH, new_h, FilterType::Triangle)
                .to_rgb8()
        } else {
            raw_crop
        };

        // Detect red-ink rubrication (dakwat merah) before any grayscaling/binarization.
        let (has_rubrication, red_ink_pct) = detect_red_ink_rubrication(&crop);
        if has_rubrication {
            any_rubrication = true;
        }

        // Adaptively apply 70/30 integral-image Sauvola for aged/stained manuscripts; preserve clean autocontrast+sharpen for crisp modern scans.
        let (enhanced, preproc_mode) = if needs_manuscript_sauvola(&crop) {
            (
                sauvola_blend_70_30(&crop, 25, 0.25, 128.0),
                "sauvola_70_30_adaptive",
            )
        } else {
            (
                sharpen_rgb(&autocontrast_rgb(&crop, 1.0), 1.4),
                "autocontrast_sharpen",
            )
        };

        let chunk_name = format!("{stem}_chunk_{idx}.jpg");
        let jpeg = encode_jpeg(&DynamicImage::ImageRgb8(enhanced), 92)?;
        fs::write(out_dir.join(&chunk_name), jpeg)
            .map_err(|e| format!("Failed saving OCR chunk: {e}"))?;
        chunks.push(json!({
            "chunk_index": idx,
            "region_label": item["region_label"],
            "vertical_range_pct": item["vertical_range_pct"],
            "box_xywh": [0, y0, width, crop_h],
            "detected_text_lines": item["detected_text_lines"],
            "has_rubrication": has_rubrication,
            "red_ink_pct": red_ink_pct,
            "preprocessing": preproc_mode,
            "enhanced_chunk_url": format!("{}/images/ocr-chunks/{chunk_name}", cfg.librechat_url),
        }));
    }

    Ok(json!({
        "file": image_path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
        "image_dimensions": { "width": width, "height": height },
        "total_chunks": chunks.len(),
        "total_detected_text_lines": total_detected_lines,
        "has_rubrication": any_rubrication,
        "chunks": chunks,
        "anti_hallucination_guide": "Read the document/handwriting strictly chunk-by-chunk from Top to Bottom (3-4 lines per strip), matching the detected_text_lines count in each chunk. Chunks flagged with has_rubrication=true contain red-ink rubrication (dakwat merah) typically marking chapter headings (Fasal, Bab, Bermula, Alkisah). Transcribe ONLY characters visibly present in the image. Never invent extra lines, never guess missing words from memory, and mark any illegible calligraphic/handwritten glyphs with [?]. If the text contains Jawi (Modern or Old Classical Jawi), ALWAYS pass the exact extracted Jawi lines into arkib_decode_jawi instead of translating by memory.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vision_ops_pure_rust() {
        assert_eq!(compute_ahash_bits(&[0, 10, 20, 200, 210, 220]), "000111");
        assert_eq!(hash_distance("000111", "010101"), 2);
        let mut energies = vec![1.0; 900];
        for l in 0..12 {
            let s = 40 + l * 70;
            for v in &mut energies[s..s + 24] {
                *v = 28.0;
            }
        }
        let plan = compute_text_line_chunks(&energies, 900, 3);
        assert_eq!(plan["detected_text_lines"], 12);
        assert_eq!(plan["chunk_count"], 3);

        // Dense 20-line Jawi page should auto-slice into 5 short strips (4 lines each) to prevent 4B hallucination
        let mut dense = vec![1.0; 1400];
        for l in 0..20 {
            let s = 30 + l * 66;
            for v in &mut dense[s..s + 20] {
                *v = 28.0;
            }
        }
        let dense_plan = compute_text_line_chunks(&dense, 1400, 5);
        assert_eq!(dense_plan["detected_text_lines"], 20);
        assert_eq!(dense_plan["chunk_count"], 5);
    }

    #[test]
    fn test_sauvola_blend_and_red_ink_rubrication() {
        // Synthetic aged/yellowed manuscript strip with dark ink strokes + red-ink heading
        let mut img = RgbImage::from_pixel(120, 60, Rgb([215, 195, 150]));
        // Black ink strokes
        for x in 10..110 {
            for y in 28..33 {
                img.put_pixel(x, y, Rgb([35, 30, 25]));
            }
        }
        // Red ink rubrication block (dakwat merah)
        for x in 10..35 {
            for y in 10..16 {
                img.put_pixel(x, y, Rgb([190, 45, 40]));
            }
        }
        let (has_red, red_pct) = detect_red_ink_rubrication(&img);
        assert!(has_red, "Expected red-ink rubrication to be detected");
        assert!(red_pct > 1.0);
        assert!(needs_manuscript_sauvola(&img));

        let out = sauvola_blend_70_30(&img, 25, 0.25, 128.0);
        assert_eq!(out.dimensions(), img.dimensions());
        // Background paper pixel should be lifted near white; ink stroke pixel should remain dark
        let bg_val = out.get_pixel(60, 50)[0];
        let ink_val = out.get_pixel(60, 30)[0];
        assert!(bg_val > 210, "Background should be bright, got {bg_val}");
        assert!(ink_val < 90, "Ink stroke should be dark, got {ink_val}");
    }
}
