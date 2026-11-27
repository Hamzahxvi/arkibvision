#![forbid(unsafe_code)]

//! Interactive Before vs. After Comparison Engine (100% Pure Safe Rust).
//!
//! Powers Arkib Canvas Mode 3 (`#arkib-canvas:compare:<id>`):
//! - Image Before/After Conservation & Integrity Metrics (Quality, Readability, Lighting, Damage, SHA-256, aHash)
//! - Document Files Original vs. Latest Page-Filtered Word-Level Diff (ONLY changed pages shown)
//! - Intake Metadata Comparison Table (`field`, `before`, `after`, `status`)
//! - Format Migration Fidelity Report (Format, Size, Dimensions, Visual aHash Fidelity, EXIF Retention, Checksums)

use crate::archives;
use crate::canvas::save_canvas_session;
use crate::config::{AppConfig, is_image_ext};
use crate::conservation::analyze_image_quality_and_conservation;
use crate::forensics::{extract_file_metadata, human_size, sha256_bytes};
use crate::revisions;
use chrono::Utc;
use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn compare_media_dir(cfg: &AppConfig) -> PathBuf {
    cfg.project_root
        .join("data")
        .join("interim")
        .join("canvas_sessions")
        .join("compare_media")
}

fn save_compare_media(cfg: &AppConfig, prefix: &str, filename: &str, bytes: &[u8]) -> Option<String> {
    let dir = compare_media_dir(cfg);
    fs::create_dir_all(&dir).ok()?;
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_else(|| "bin".to_string());
    let out_name = format!("{prefix}_{}.{ext}", &Uuid::new_v4().simple().to_string()[..10]);
    fs::write(dir.join(&out_name), bytes).ok()?;
    Some(format!("/canvas/api/compare-media/{out_name}"))
}

fn image_ahash_and_dims(bytes: &[u8]) -> (Option<(u32, u32)>, Option<u64>) {
    let Ok(img) = image::load_from_memory(bytes) else {
        return (None, None);
    };
    let dims = (img.width(), img.height());
    let small = img
        .grayscale()
        .resize_exact(8, 8, image::imageops::FilterType::Triangle);
    let luma = small.to_luma8();
    let sum: u64 = luma.pixels().map(|p| u64::from(p.0[0])).sum();
    let mean = sum / 64;
    let mut hash = 0u64;
    for (idx, p) in luma.pixels().enumerate() {
        if u64::from(p.0[0]) >= mean {
            hash |= 1u64 << idx;
        }
    }
    (Some(dims), Some(hash))
}

fn ext_format_label(filename: &str) -> String {
    Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_uppercase())
        .unwrap_or_else(|| "FILE".to_string())
}

/// Extract text split into pages (real PDF pages via `lopdf`, or form-feed / page markers / chunks).
pub fn extract_pages_from_bytes(filename: &str, bytes: &[u8]) -> Vec<(usize, String)> {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    if ext == "pdf"
        && let Ok(doc) = lopdf::Document::load_mem(bytes)
    {
        let mut page_nums: Vec<u32> = doc.get_pages().keys().copied().collect();
        page_nums.sort();
        let mut pages = Vec::new();
        for pnum in page_nums {
            let txt = doc.extract_text(&[pnum]).unwrap_or_default();
            pages.push((pnum as usize, txt.trim().to_string()));
        }
        if !pages.is_empty() {
            return pages;
        }
    }

    if is_image_ext(Path::new(filename)) {
        return Vec::new();
    }

    let tmp = std::env::temp_dir().join(format!(
        "arkib_cmp_pg_{}.{}",
        Uuid::new_v4().simple(),
        if ext.is_empty() { "txt" } else { &ext }
    ));
    let full_text = if fs::write(&tmp, bytes).is_ok() {
        let t = archives::extract_text_from_file(&tmp).unwrap_or_default();
        let _ = fs::remove_file(&tmp);
        t
    } else {
        String::from_utf8_lossy(bytes).to_string()
    };

    split_text_into_pages(&full_text)
}

pub fn split_text_into_pages(text: &str) -> Vec<(usize, String)> {
    let clean = text.replace("\r\n", "\n");
    if clean.trim().is_empty() {
        return Vec::new();
    }
    if clean.contains('\x0c') {
        return clean
            .split('\x0c')
            .enumerate()
            .map(|(i, p)| (i + 1, p.trim().to_string()))
            .collect();
    }
    // Check explicit "--- Page N ---" or "[Page N]" markers
    let mut explicit_pages: Vec<(usize, String)> = Vec::new();
    let mut cur_buf = Vec::new();
    let mut cur_page = 1usize;
    let mut found_marker = false;
    for line in clean.lines() {
        let l = line.trim();
        let lc = l.to_ascii_lowercase();
        if (lc.starts_with("--- page ") || lc.starts_with("[page ") || lc.starts_with("=== page "))
            && l.len() <= 40
        {
            if !cur_buf.is_empty() || found_marker {
                explicit_pages.push((cur_page, cur_buf.join("\n").trim().to_string()));
                cur_buf.clear();
                cur_page += 1;
            }
            found_marker = true;
            continue;
        }
        cur_buf.push(line);
    }
    if found_marker {
        if !cur_buf.is_empty() {
            explicit_pages.push((cur_page, cur_buf.join("\n").trim().to_string()));
        }
        return explicit_pages;
    }

    // Fallback: group into logical ~18-line archival pages
    let lines: Vec<&str> = clean.lines().collect();
    if lines.len() <= 20 {
        return vec![(1, clean.trim().to_string())];
    }
    lines
        .chunks(18)
        .enumerate()
        .map(|(i, chunk)| (i + 1, chunk.join("\n").trim().to_string()))
        .collect()
}

/// Pure-Rust word-level LCS diff returning tokens tagged `eq` | `del` (before) and `eq` | `add` (after)
/// so the UI can highlight changed words with crisp red/green inline pills.
pub fn word_level_diff(before: &str, after: &str) -> (Vec<Value>, Vec<Value>, usize) {
    let a: Vec<&str> = before.split_whitespace().collect();
    let b: Vec<&str> = after.split_whitespace().collect();

    // Guard DP memory for very large single pages
    if a.len() > 450 || b.len() > 450 {
        let same = before.trim() == after.trim();
        let bt = vec![json!({ "text": before.trim(), "kind": if same { "eq" } else { "del" } })];
        let at = vec![json!({ "text": after.trim(), "kind": if same { "eq" } else { "add" } })];
        return (bt, at, usize::from(!same));
    }

    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut before_tokens: Vec<Value> = Vec::new();
    let mut after_tokens: Vec<Value> = Vec::new();
    let mut changes = 0usize;
    let (mut i, mut j) = (0usize, 0usize);

    while i < n && j < m {
        if a[i] == b[j] {
            before_tokens.push(json!({ "text": a[i], "kind": "eq" }));
            after_tokens.push(json!({ "text": b[j], "kind": "eq" }));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            before_tokens.push(json!({ "text": a[i], "kind": "del" }));
            changes += 1;
            i += 1;
        } else {
            after_tokens.push(json!({ "text": b[j], "kind": "add" }));
            changes += 1;
            j += 1;
        }
    }
    while i < n {
        before_tokens.push(json!({ "text": a[i], "kind": "del" }));
        changes += 1;
        i += 1;
    }
    while j < m {
        after_tokens.push(json!({ "text": b[j], "kind": "add" }));
        changes += 1;
        j += 1;
    }

    (before_tokens, after_tokens, changes)
}

/// Compare Original vs. Latest pages and return ONLY pages with actual changes.
pub fn compare_pages_only_changed(
    before_pages: &[(usize, String)],
    after_pages: &[(usize, String)],
) -> Value {
    let total_pages = before_pages.len().max(after_pages.len());
    let mut changed_pages = Vec::new();
    let mut unchanged_page_numbers = Vec::new();

    for idx in 0..total_pages {
        let page_num = idx + 1;
        let b_txt = before_pages
            .get(idx)
            .map(|(_, s)| s.as_str())
            .unwrap_or("");
        let a_txt = after_pages
            .get(idx)
            .map(|(_, s)| s.as_str())
            .unwrap_or("");

        if b_txt.trim() == a_txt.trim() {
            unchanged_page_numbers.push(page_num);
            continue;
        }

        let (before_tokens, after_tokens, word_changes) = word_level_diff(b_txt, a_txt);
        let status = if b_txt.trim().is_empty() {
            "Page Added"
        } else if a_txt.trim().is_empty() {
            "Page Removed"
        } else {
            "Modified"
        };
        let is_rtl = b_txt.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c))
            || a_txt.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c));

        changed_pages.push(json!({
            "page": page_num,
            "status": status,
            "word_changes": word_changes,
            "is_rtl": is_rtl,
            "before_text": b_txt,
            "after_text": a_txt,
            "before_tokens": before_tokens,
            "after_tokens": after_tokens,
        }));
    }

    json!({
        "total_pages": total_pages,
        "changed_page_count": changed_pages.len(),
        "unchanged_page_count": unchanged_page_numbers.len(),
        "unchanged_page_numbers": unchanged_page_numbers,
        "changed_pages": changed_pages,
    })
}

/// Build the structured Intake Metadata Comparison Table (`field`, `label`, `before`, `after`, `status`).
pub fn build_intake_metadata_comparison(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Vec<Value> {
    let tracked_fields = [
        ("title", "Subject / Title"),
        ("category", "Category"),
        ("year", "Year"),
        ("author_or_source", "Source / Agency"),
        ("group", "Group Folder"),
        ("reference_code", "Reference Code"),
        ("script_type", "Script Type"),
        ("date_created", "Date / Era"),
        ("hijri_date", "Hijri Date"),
        ("gregorian_date_masihi", "Masihi Date"),
        ("origin_location", "Origin / Location"),
        ("volume_id", "Volume ID"),
        ("folio_number", "Folio Number"),
        ("summary", "Archival Summary"),
        ("notes", "Notes"),
    ];

    let fmt_val = |m: &Map<String, Value>, k: &str| -> String {
        match m.get(k) {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            _ => String::new(),
        }
    };

    let mut rows = Vec::new();
    for (field, label) in tracked_fields {
        let bv = fmt_val(before, field);
        let av = fmt_val(after, field);
        if bv.is_empty() && av.is_empty() {
            continue;
        }
        let status = if bv == av {
            "Unchanged"
        } else if bv.is_empty() {
            "Added"
        } else if av.is_empty() {
            "Removed"
        } else {
            "Updated"
        };
        rows.push(json!({
            "field": field,
            "label": label,
            "before": if bv.is_empty() { "—" } else { &bv },
            "after": if av.is_empty() { "—" } else { &av },
            "status": status,
        }));
    }
    rows
}

fn analyze_image_conservation_bytes(filename: &str, bytes: &[u8]) -> Option<Value> {
    if !is_image_ext(Path::new(filename)) {
        return None;
    }
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let tmp = std::env::temp_dir().join(format!("arkib_cmp_cons_{}.{ext}", Uuid::new_v4().simple()));
    fs::write(&tmp, bytes).ok()?;
    let res = analyze_image_quality_and_conservation(&tmp).ok();
    let _ = fs::remove_file(&tmp);
    res
}

fn count_exif_tags_bytes(filename: &str, bytes: &[u8]) -> usize {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let tmp = std::env::temp_dir().join(format!("arkib_cmp_meta_{}.{ext}", Uuid::new_v4().simple()));
    if fs::write(&tmp, bytes).is_err() {
        return 0;
    }
    let cnt = extract_file_metadata(&tmp)
        .ok()
        .and_then(|v| v["exif_tags"].as_object().map(Map::len))
        .unwrap_or(0);
    let _ = fs::remove_file(&tmp);
    cnt
}

fn build_conservation_comparison(
    before_cons: Option<&Value>,
    after_cons: Option<&Value>,
    before_sha: &str,
    after_sha: &str,
    perceptual_dist: Option<u32>,
) -> Vec<Value> {
    let mut rows = Vec::new();
    let add_score_row = |rows: &mut Vec<Value>, label: &str, b: Option<f64>, a: Option<f64>, unit: &str| {
        if let (Some(bv), Some(av)) = (b, a) {
            let delta = ((av - bv) * 10.0).round() / 10.0;
            let status = if delta > 1.0 {
                "Improved"
            } else if delta < -1.0 {
                "Degraded"
            } else {
                "Stable"
            };
            let delta_str = if delta > 0.0 {
                format!("+{delta}{unit}")
            } else {
                format!("{delta}{unit}")
            };
            rows.push(json!({
                "metric": label,
                "before": format!("{bv}{unit}"),
                "after": format!("{av}{unit}"),
                "delta": delta_str,
                "status": status,
            }));
        }
    };

    if let (Some(bc), Some(ac)) = (before_cons, after_cons) {
        add_score_row(
            &mut rows,
            "Overall Archival Quality",
            bc["image_quality"]["overall_score"].as_f64(),
            ac["image_quality"]["overall_score"].as_f64(),
            "/100",
        );
        add_score_row(
            &mut rows,
            "Readability & Sharpness",
            bc["image_quality"]["readability"]["score"].as_f64(),
            ac["image_quality"]["readability"]["score"].as_f64(),
            "/100",
        );
        add_score_row(
            &mut rows,
            "Lighting & Contrast",
            bc["image_quality"]["lighting"]["score"].as_f64(),
            ac["image_quality"]["lighting"]["score"].as_f64(),
            "/100",
        );
        let bd = bc["damage_assessment"]["overall_severity"]
            .as_str()
            .unwrap_or("none");
        let ad = ac["damage_assessment"]["overall_severity"]
            .as_str()
            .unwrap_or("none");
        rows.push(json!({
            "metric": "Physical Damage Grade",
            "before": bd,
            "after": ad,
            "delta": if bd == ad { "Unchanged" } else { "Updated" },
            "status": if ad == "none" || (bd == "heavy" && ad != "heavy") { "Improved" } else if bd == ad { "Stable" } else { "Review" },
        }));
        for (dk, dlbl) in [("faded", "Fading"), ("aging", "Yellowing / Aging"), ("mold", "Mold / Stain"), ("tear", "Tear / Crease")] {
            let bl = bc["damage_assessment"][dk]["level"].as_str().unwrap_or("none");
            let al = ac["damage_assessment"][dk]["level"].as_str().unwrap_or("none");
            if bl != "none" || al != "none" {
                rows.push(json!({
                    "metric": dlbl,
                    "before": bl,
                    "after": al,
                    "delta": if bl == al { "Unchanged" } else { "Changed" },
                    "status": if al == "none" { "Improved" } else if bl == al { "Stable" } else { "Review" },
                }));
            }
        }
    }

    if let Some(dist) = perceptual_dist {
        let sim = ((64.0 - f64::from(dist)) / 64.0 * 100.0).round();
        rows.push(json!({
            "metric": "Visual Structure Match (aHash)",
            "before": "Baseline (64-bit)",
            "after": format!("{sim}% ({dist}/64 bits diff)"),
            "delta": format!("{dist} bits"),
            "status": if dist <= 8 { "Preserved" } else if dist <= 18 { "Enhanced" } else { "Major Change" },
        }));
    }

    if !before_sha.is_empty() || !after_sha.is_empty() {
        let same = !before_sha.is_empty() && before_sha == after_sha;
        rows.push(json!({
            "metric": "SHA-256 File Integrity",
            "before": if before_sha.is_empty() { "—".to_string() } else { format!("{}…", &before_sha[..14.min(before_sha.len())]) },
            "after": if after_sha.is_empty() { "—".to_string() } else { format!("{}…", &after_sha[..14.min(after_sha.len())]) },
            "delta": if same { "Identical" } else { "New Hash" },
            "status": if same { "Preserved" } else { "Verified New" },
        }));
    }

    rows
}

#[allow(clippy::too_many_arguments)]
fn build_migration_fidelity(
    before_name: &str,
    before_bytes: &[u8],
    after_name: &str,
    after_bytes: &[u8],
    before_dims: Option<(u32, u32)>,
    after_dims: Option<(u32, u32)>,
    perceptual_dist: Option<u32>,
    before_sha: &str,
    after_sha: &str,
) -> Value {
    let src_fmt = ext_format_label(before_name);
    let dst_fmt = ext_format_label(after_name);
    let b_sz = before_bytes.len() as u64;
    let a_sz = after_bytes.len() as u64;
    let size_delta_pct = if b_sz > 0 {
        (((a_sz as f64 - b_sz as f64) / b_sz as f64) * 100.0).round()
    } else {
        0.0
    };
    let dims_before = before_dims
        .map(|(w, h)| format!("{w}×{h}"))
        .unwrap_or_else(|| "N/A".to_string());
    let dims_after = after_dims
        .map(|(w, h)| format!("{w}×{h}"))
        .unwrap_or_else(|| "N/A".to_string());
    let dims_status = match (before_dims, after_dims) {
        (Some(b), Some(a)) if b == a => "Preserved",
        (Some(_), Some(_)) => "Resized",
        _ => "N/A",
    };

    let visual_sim_pct = perceptual_dist.map(|d| ((64.0 - f64::from(d)) / 64.0 * 100.0).round());
    let visual_status = match perceptual_dist {
        Some(d) if d <= 8 => "High Fidelity",
        Some(d) if d <= 18 => "Enhanced / Minor Shift",
        Some(_) => "Review Needed",
        None => "Text / Non-Image",
    };

    let b_exif = count_exif_tags_bytes(before_name, before_bytes);
    let a_exif = count_exif_tags_bytes(after_name, after_bytes);
    let exif_status = if b_exif == 0 && a_exif == 0 {
        "No EXIF Present"
    } else if a_exif >= b_exif {
        "Preserved"
    } else {
        "Stripped — Manual Review"
    };

    let verdict = if dims_status == "Resized" || exif_status.starts_with("Stripped") || perceptual_dist.is_some_and(|d| d > 20) {
        "REVIEW"
    } else {
        "PASS"
    };

    json!({
        "source_format": src_fmt,
        "target_format": dst_fmt,
        "format_changed": src_fmt != dst_fmt,
        "before_size_bytes": b_sz,
        "after_size_bytes": a_sz,
        "before_size_human": human_size(b_sz),
        "after_size_human": human_size(a_sz),
        "size_delta_pct": size_delta_pct,
        "before_dimensions": dims_before,
        "after_dimensions": dims_after,
        "dimensions_status": dims_status,
        "perceptual_distance": perceptual_dist,
        "visual_similarity_pct": visual_sim_pct,
        "visual_status": visual_status,
        "before_exif_tags": b_exif,
        "after_exif_tags": a_exif,
        "metadata_retention": exif_status,
        "before_sha256": before_sha,
        "after_sha256": after_sha,
        "verdict": verdict,
    })
}

/// Compare any two images or document files (bytes in memory) and persist a Compare Canvas session.
pub fn build_file_comparison_session(
    cfg: &AppConfig,
    title: &str,
    before_name: &str,
    before_bytes: &[u8],
    after_name: &str,
    after_bytes: &[u8],
) -> Result<(String, Value), String> {
    let both_images =
        is_image_ext(Path::new(before_name)) && is_image_ext(Path::new(after_name));
    let before_sha = sha256_bytes(before_bytes);
    let after_sha = sha256_bytes(after_bytes);

    let (b_dims, b_hash) = if is_image_ext(Path::new(before_name)) {
        image_ahash_and_dims(before_bytes)
    } else {
        (None, None)
    };
    let (a_dims, a_hash) = if is_image_ext(Path::new(after_name)) {
        image_ahash_and_dims(after_bytes)
    } else {
        (None, None)
    };
    let perceptual_dist = match (b_hash, a_hash) {
        (Some(bh), Some(ah)) => Some((bh ^ ah).count_ones()),
        _ => None,
    };

    let before_img_url = if is_image_ext(Path::new(before_name)) {
        save_compare_media(cfg, "before", before_name, before_bytes)
    } else {
        None
    };
    let after_img_url = if is_image_ext(Path::new(after_name)) {
        save_compare_media(cfg, "after", after_name, after_bytes)
    } else {
        None
    };

    let before_cons = analyze_image_conservation_bytes(before_name, before_bytes);
    let after_cons = analyze_image_conservation_bytes(after_name, after_bytes);
    let conservation_metrics = build_conservation_comparison(
        before_cons.as_ref(),
        after_cons.as_ref(),
        &before_sha,
        &after_sha,
        perceptual_dist,
    );

    let migration_fidelity = build_migration_fidelity(
        before_name,
        before_bytes,
        after_name,
        after_bytes,
        b_dims,
        a_dims,
        perceptual_dist,
        &before_sha,
        &after_sha,
    );

    let document_pages = if !both_images {
        let bp = extract_pages_from_bytes(before_name, before_bytes);
        let ap = extract_pages_from_bytes(after_name, after_bytes);
        compare_pages_only_changed(&bp, &ap)
    } else {
        json!({
            "total_pages": 0,
            "changed_page_count": 0,
            "unchanged_page_count": 0,
            "unchanged_page_numbers": [],
            "changed_pages": [],
        })
    };

    let mut before_meta = Map::new();
    before_meta.insert("title".into(), Value::String(before_name.to_string()));
    let mut after_meta = Map::new();
    after_meta.insert("title".into(), Value::String(after_name.to_string()));
    let intake_metadata = build_intake_metadata_comparison(&before_meta, &after_meta);

    let payload_val = json!({
        "title": title,
        "comparison_kind": if both_images { "image" } else { "file" },
        "before_label": format!("Original · {before_name}"),
        "after_label": format!("Latest · {after_name}"),
        "before_image_url": before_img_url,
        "after_image_url": after_img_url,
        "conservation_metrics": conservation_metrics,
        "document_pages": document_pages,
        "intake_metadata": intake_metadata,
        "migration_fidelity": migration_fidelity,
        "created_at": Utc::now().to_rfc3339(),
    });

    let map = payload_val.as_object().cloned().unwrap_or_default();
    let session_id = save_canvas_session(cfg, "compare", map)?;
    let mut out = payload_val;
    if let Some(o) = out.as_object_mut() {
        o.insert("session_id".into(), Value::String(session_id.clone()));
    }
    Ok((session_id, out))
}

/// Compare an archived document's **Original** baseline against its **Latest** saved state
/// (changed pages only + Intake Metadata Before→After + Conservation & Format Migration Fidelity).
pub fn build_archived_document_comparison(cfg: &AppConfig, slug: &str) -> Result<Value, String> {
    let latest_doc = archives::load_document(cfg, slug)
        .ok_or_else(|| format!("Document '{slug}' not found."))?;
    let latest_map = latest_doc.as_object().cloned().unwrap_or_default();
    let title = latest_map
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(slug)
        .to_string();

    // Reconstruct earliest Original metadata by reversing revision history diffs
    let revs = revisions::get_entity_revisions(cfg, "document", slug); // newest-first
    let mut original_map = latest_map.clone();
    let mut latest_file_diff: Option<revisions::FileDiff> = None;

    for rv in &revs {
        if latest_file_diff.is_none()
            && let Some(fd) = &rv.file_diff
        {
            latest_file_diff = Some(fd.clone());
        }
        for fd in &rv.field_diffs {
            original_map.insert(fd.field.clone(), Value::String(fd.before.clone()));
        }
    }

    let orig_text = original_map
        .get("extracted_text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let latest_text = latest_map
        .get("extracted_text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let document_pages = compare_pages_only_changed(
        &split_text_into_pages(orig_text),
        &split_text_into_pages(latest_text),
    );

    let orig_jawi = original_map
        .get("jawi_text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let latest_jawi = latest_map
        .get("jawi_text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let jawi_pages = if !orig_jawi.trim().is_empty() || !latest_jawi.trim().is_empty() {
        Some(compare_pages_only_changed(
            &split_text_into_pages(orig_jawi),
            &split_text_into_pages(latest_jawi),
        ))
    } else {
        None
    };

    let intake_metadata = build_intake_metadata_comparison(&original_map, &latest_map);

    let mut before_img_url: Option<String> = None;
    let mut after_img_url: Option<String> = None;
    let mut conservation_metrics = Vec::new();
    let mut migration_fidelity = Value::Null;

    if let Some(fd) = latest_file_diff {
        before_img_url = fd.before_snapshot_url.clone();
        after_img_url = fd.after_snapshot_url.clone();
        let b_sha = fd.before_sha256.clone().unwrap_or_default();
        let a_sha = fd.after_sha256.clone().unwrap_or_default();
        conservation_metrics = build_conservation_comparison(
            None,
            None,
            &b_sha,
            &a_sha,
            fd.perceptual_distance,
        );
        migration_fidelity = json!({
            "source_format": ext_format_label(&fd.filename),
            "target_format": ext_format_label(&fd.filename),
            "format_changed": false,
            "before_size_bytes": fd.before_size.unwrap_or(0),
            "after_size_bytes": fd.after_size.unwrap_or(0),
            "before_size_human": human_size(fd.before_size.unwrap_or(0)),
            "after_size_human": human_size(fd.after_size.unwrap_or(0)),
            "size_delta_pct": 0.0,
            "before_dimensions": fd.before_dims.clone().unwrap_or_else(|| "N/A".into()),
            "after_dimensions": fd.after_dims.clone().unwrap_or_else(|| "N/A".into()),
            "dimensions_status": if fd.before_dims == fd.after_dims { "Preserved" } else { "Changed" },
            "perceptual_distance": fd.perceptual_distance,
            "visual_similarity_pct": fd.perceptual_distance.map(|d| ((64.0 - f64::from(d)) / 64.0 * 100.0).round()),
            "visual_status": match fd.perceptual_distance {
                Some(d) if d <= 8 => "High Fidelity",
                Some(d) if d <= 18 => "Enhanced / Minor Shift",
                Some(_) => "Review Needed",
                None => "Document File",
            },
            "metadata_retention": "Tracked in Revision History",
            "before_sha256": b_sha,
            "after_sha256": a_sha,
            "verdict": if fd.perceptual_distance.is_some_and(|d| d > 20) { "REVIEW" } else { "PASS" },
        });
    }

    Ok(json!({
        "title": title,
        "document_slug": slug,
        "comparison_kind": "document",
        "revision_count": revs.len(),
        "before_label": "Original (First Recorded Version)",
        "after_label": "Latest Saved Version",
        "before_image_url": before_img_url,
        "after_image_url": after_img_url,
        "conservation_metrics": conservation_metrics,
        "document_pages": document_pages,
        "jawi_pages": jawi_pages,
        "intake_metadata": intake_metadata,
        "migration_fidelity": migration_fidelity,
        "created_at": Utc::now().to_rfc3339(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_comparison_returns_only_changed_pages_with_word_diffs() {
        let orig = vec![
            (1, "Fasal pertama surat lama".to_string()),
            (2, "Fasal kedua farjanjin negeri 1324H".to_string()),
            (3, "Penutup rasmi arkib".to_string()),
        ];
        let latest = vec![
            (1, "Fasal pertama surat lama".to_string()),
            (2, "Fasal kedua perjanjian negeri 1324H (1906)".to_string()),
            (3, "Penutup rasmi arkib".to_string()),
        ];

        let cmp = compare_pages_only_changed(&orig, &latest);
        assert_eq!(cmp["total_pages"], 3);
        assert_eq!(cmp["changed_page_count"], 1);
        assert_eq!(cmp["unchanged_page_count"], 2);
        let pages = cmp["changed_pages"].as_array().unwrap();
        assert_eq!(pages[0]["page"], 2);
        assert!(pages[0]["before_tokens"].as_array().unwrap().iter().any(|t| t["text"] == "farjanjin" && t["kind"] == "del"));
        assert!(pages[0]["after_tokens"].as_array().unwrap().iter().any(|t| t["text"] == "perjanjian" && t["kind"] == "add"));
    }

    #[test]
    fn intake_metadata_comparison_tracks_field_before_after_status() {
        let before = json!({
            "title": "Surat Lama",
            "category": "Manuskrip",
            "year": "1906"
        })
        .as_object()
        .unwrap()
        .clone();
        let after = json!({
            "title": "Surat Perjanjian 1324H",
            "category": "Manuskrip",
            "year": "1906",
            "author_or_source": "Arkib Negara Malaysia"
        })
        .as_object()
        .unwrap()
        .clone();
        let rows = build_intake_metadata_comparison(&before, &after);
        assert!(rows.iter().any(|r| r["field"] == "title" && r["status"] == "Updated"));
        assert!(rows.iter().any(|r| r["field"] == "category" && r["status"] == "Unchanged"));
        assert!(rows.iter().any(|r| r["field"] == "author_or_source" && r["status"] == "Added"));
    }
}
