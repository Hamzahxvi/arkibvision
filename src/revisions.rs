#![forbid(unsafe_code)]

//! Change Detection & Before/After Revision Comparison Engine (100% Pure Safe Rust).
//!
//! Captures versioned before/after metadata, text, image, and file snapshots whenever
//! documents, profiles, or locations are mutated, and detects external on-disk file
//! modifications against `checksum_manifest.json`.

use crate::config::{AppConfig, is_image_ext};
use crate::forensics::sha256_bytes;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldDiff {
    pub field: String,
    pub label: String,
    pub before: String,
    pub after: String,
    pub kind: String, // "added" | "removed" | "changed"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileDiff {
    pub filename: String,
    pub change_kind: String, // "added" | "replaced" | "edited" | "deleted" | "external_modified"
    pub is_image: bool,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
    pub before_size: Option<u64>,
    pub after_size: Option<u64>,
    pub before_dims: Option<String>,
    pub after_dims: Option<String>,
    pub perceptual_distance: Option<u32>,
    pub before_snapshot_url: Option<String>,
    pub after_snapshot_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevisionEntry {
    pub id: String,
    pub entity_type: String, // "document" | "profile" | "location"
    pub slug: String,
    pub title: String,
    pub timestamp: String,
    pub actor: String,
    pub action: String, // "metadata_edit" | "file_added" | "file_replaced" | "file_deleted" | "image_edited" | "external_modified"
    pub summary: String,
    #[serde(default)]
    pub field_diffs: Vec<FieldDiff>,
    #[serde(default)]
    pub file_diff: Option<FileDiff>,
}

fn sanitize_segment(raw: &str) -> Option<String> {
    let clean = raw.trim();
    if clean.is_empty()
        || clean.contains("..")
        || clean.contains('/')
        || clean.contains('\\')
        || clean.contains('\0')
    {
        return None;
    }
    Some(clean.to_string())
}

fn entity_rev_dir(cfg: &AppConfig, entity_type: &str, slug: &str) -> Option<PathBuf> {
    let et = sanitize_segment(entity_type)?;
    let sl = sanitize_segment(slug)?;
    Some(cfg.revisions_dir.join(et).join(sl))
}

fn history_path(cfg: &AppConfig, entity_type: &str, slug: &str) -> Option<PathBuf> {
    Some(entity_rev_dir(cfg, entity_type, slug)?.join("history.json"))
}

fn load_history(cfg: &AppConfig, entity_type: &str, slug: &str) -> Vec<RevisionEntry> {
    let Some(path) = history_path(cfg, entity_type, slug) else {
        return Vec::new();
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<RevisionEntry>>(&raw).unwrap_or_default()
}

fn save_history(
    cfg: &AppConfig,
    entity_type: &str,
    slug: &str,
    entries: &[RevisionEntry],
) -> Result<(), String> {
    let dir = entity_rev_dir(cfg, entity_type, slug)
        .ok_or_else(|| "Invalid revision entity path.".to_string())?;
    fs::create_dir_all(&dir).map_err(|e| format!("Failed creating revision dir: {e}"))?;
    let path = dir.join("history.json");
    let tmp = dir.join("history.json.tmp");
    let body = serde_json::to_string_pretty(entries)
        .map_err(|e| format!("Failed serializing revision history: {e}"))?;
    fs::write(&tmp, body).map_err(|e| format!("Failed writing revision temp file: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("Failed replacing revision history: {e}"))?;
    Ok(())
}

fn pretty_field_label(key: &str) -> String {
    match key {
        "title" => "Title / Subject".to_string(),
        "subject" => "Subject".to_string(),
        "category" => "Category".to_string(),
        "year" => "Year".to_string(),
        "date_created" => "Date Created".to_string(),
        "author_or_source" => "Source / Provenance".to_string(),
        "origin_location" => "Origin Location".to_string(),
        "reference_code" => "Reference Code".to_string(),
        "script_type" => "Script Type".to_string(),
        "hijri_date" => "Hijri Date".to_string(),
        "gregorian_date_masihi" => "Masihi Date".to_string(),
        "volume_id" => "Volume ID".to_string(),
        "folio_number" => "Folio Number".to_string(),
        "group" | "group_slug" => "Group Folder".to_string(),
        "extracted_text" => "Extracted / OCR Text".to_string(),
        "summary" => "Summary".to_string(),
        "notes" => "Notes".to_string(),
        "name" => "Name".to_string(),
        "public_fields" => "Public Profile Fields".to_string(),
        "private_fields" => "Private Profile Fields".to_string(),
        other => other
            .split('_')
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn format_value_compact(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(arr) => arr
            .iter()
            .map(format_value_compact)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(map) => {
            if map.is_empty() {
                String::new()
            } else {
                serde_json::to_string_pretty(v).unwrap_or_default()
            }
        }
    }
}

fn is_ignored_diff_key(key: &str) -> bool {
    matches!(
        key,
        "slug"
            | "files"
            | "images"
            | "file_count"
            | "image_count"
            | "updated_at"
            | "created_at"
            | "revision_count"
            | "last_modified_at"
            | "group_slug"
            | "authenticity_report"
            | "conservation_report"
    )
}

/// Compute clean, field-by-field before/after differences between two JSON objects.
pub fn compute_json_diffs(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Vec<FieldDiff> {
    let mut keys: BTreeSet<&str> = BTreeSet::new();
    for k in before.keys() {
        if !is_ignored_diff_key(k) {
            keys.insert(k.as_str());
        }
    }
    for k in after.keys() {
        if !is_ignored_diff_key(k) {
            keys.insert(k.as_str());
        }
    }

    let mut diffs = Vec::new();
    for k in keys {
        // Expand nested public_fields / private_fields so admins see individual field changes!
        if matches!(k, "public_fields" | "private_fields") {
            let b_map = before.get(k).and_then(Value::as_object);
            let a_map = after.get(k).and_then(Value::as_object);
            let mut sub_keys: BTreeSet<&str> = BTreeSet::new();
            if let Some(bm) = b_map {
                for sk in bm.keys() {
                    sub_keys.insert(sk.as_str());
                }
            }
            if let Some(am) = a_map {
                for sk in am.keys() {
                    sub_keys.insert(sk.as_str());
                }
            }
            let prefix = if k == "private_fields" {
                "Private: "
            } else {
                "Public: "
            };
            for sk in sub_keys {
                let bv = b_map
                    .and_then(|m| m.get(sk))
                    .map(format_value_compact)
                    .unwrap_or_default();
                let av = a_map
                    .and_then(|m| m.get(sk))
                    .map(format_value_compact)
                    .unwrap_or_default();
                if bv != av {
                    let kind = if bv.is_empty() {
                        "added"
                    } else if av.is_empty() {
                        "removed"
                    } else {
                        "changed"
                    };
                    diffs.push(FieldDiff {
                        field: format!("{k}.{sk}"),
                        label: format!("{prefix}{}", pretty_field_label(sk)),
                        before: bv,
                        after: av,
                        kind: kind.to_string(),
                    });
                }
            }
            continue;
        }

        let bv = before.get(k).map(format_value_compact).unwrap_or_default();
        let av = after.get(k).map(format_value_compact).unwrap_or_default();
        if bv != av {
            let kind = if bv.is_empty() {
                "added"
            } else if av.is_empty() {
                "removed"
            } else {
                "changed"
            };
            diffs.push(FieldDiff {
                field: k.to_string(),
                label: pretty_field_label(k),
                before: bv,
                after: av,
                kind: kind.to_string(),
            });
        }
    }
    diffs
}

/// Record a full lifecycle event (`created` or `deleted`) for any entity (Document, Person,
/// Location, Category, or Group Folder) so admins can always inspect added and deleted items.
#[allow(clippy::too_many_arguments)]
pub fn record_lifecycle_event(
    cfg: &AppConfig,
    entity_type: &str,
    slug: &str,
    title: &str,
    actor: &str,
    action: &str, // "created" | "deleted"
    before: Option<&Map<String, Value>>,
    after: Option<&Map<String, Value>>,
) -> Option<RevisionEntry> {
    let empty = Map::new();
    let diffs = compute_json_diffs(before.unwrap_or(&empty), after.unwrap_or(&empty));
    let label_type = match entity_type {
        "document" => "document",
        "profile" => "person profile",
        "location" => "location",
        "category" => "category",
        "group" => "group folder",
        other => other,
    };
    let summary = match action {
        "created" => format!("Created {label_type} \"{title}\""),
        "deleted" => format!("Deleted {label_type} \"{title}\""),
        _ => format!("Changed {label_type} \"{title}\""),
    };
    let entry = RevisionEntry {
        id: format!("rev_{}", &Uuid::new_v4().simple().to_string()[..10]),
        entity_type: entity_type.to_string(),
        slug: slug.to_string(),
        title: title.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        actor: actor.to_string(),
        action: action.to_string(),
        summary,
        field_diffs: diffs,
        file_diff: None,
    };
    let mut history = load_history(cfg, entity_type, slug);
    history.push(entry.clone());
    if history.len() > 50 {
        let drop_n = history.len() - 50;
        history.drain(0..drop_n);
    }
    let _ = save_history(cfg, entity_type, slug, &history);
    Some(entry)
}

/// Record a metadata/text revision if any fields actually changed.
#[allow(clippy::too_many_arguments)]
pub fn record_metadata_change(
    cfg: &AppConfig,
    entity_type: &str,
    slug: &str,
    title: &str,
    actor: &str,
    action: &str,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Option<RevisionEntry> {
    let diffs = compute_json_diffs(before, after);
    if diffs.is_empty() {
        return None;
    }
    let changed_labels: Vec<String> = diffs.iter().take(4).map(|d| d.label.clone()).collect();
    let suffix = if diffs.len() > 4 {
        format!(" (+{} more)", diffs.len() - 4)
    } else {
        String::new()
    };
    let summary = format!("Updated {}{}", changed_labels.join(", "), suffix);
    let entry = RevisionEntry {
        id: format!("rev_{}", &Uuid::new_v4().simple().to_string()[..10]),
        entity_type: entity_type.to_string(),
        slug: slug.to_string(),
        title: title.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        actor: actor.to_string(),
        action: action.to_string(),
        summary,
        field_diffs: diffs,
        file_diff: None,
    };
    let mut history = load_history(cfg, entity_type, slug);
    history.push(entry.clone());
    // Keep last 50 revisions per entity to avoid unbounded disk growth.
    if history.len() > 50 {
        let drop_n = history.len() - 50;
        history.drain(0..drop_n);
    }
    let _ = save_history(cfg, entity_type, slug, &history);
    Some(entry)
}

fn image_metrics(bytes: &[u8]) -> (Option<String>, Option<u64>) {
    let Ok(img) = image::load_from_memory(bytes) else {
        return (None, None);
    };
    let dims = format!("{}×{}", img.width(), img.height());
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

/// Record a file or image change with preserved before/after snapshots for visual/byte comparison.
#[allow(clippy::too_many_arguments)]
pub fn record_file_change(
    cfg: &AppConfig,
    entity_type: &str,
    slug: &str,
    title: &str,
    actor: &str,
    filename: &str,
    change_kind: &str,
    before_bytes: Option<&[u8]>,
    after_bytes: Option<&[u8]>,
) -> Option<RevisionEntry> {
    let dir = entity_rev_dir(cfg, entity_type, slug)?;
    let snap_dir = dir.join("snapshots");
    let _ = fs::create_dir_all(&snap_dir);

    let rev_id = format!("rev_{}", &Uuid::new_v4().simple().to_string()[..10]);
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_else(|| "bin".to_string());
    let is_img = is_image_ext(Path::new(filename));

    let before_sha = before_bytes.map(sha256_bytes);
    let after_sha = after_bytes.map(sha256_bytes);
    if before_sha.is_some() && before_sha == after_sha && change_kind != "deleted" {
        return None;
    }

    let (before_dims, before_hash) = before_bytes
        .filter(|_| is_img)
        .map(image_metrics)
        .unwrap_or((None, None));
    let (after_dims, after_hash) = after_bytes
        .filter(|_| is_img)
        .map(image_metrics)
        .unwrap_or((None, None));

    let perceptual_distance = match (before_hash, after_hash) {
        (Some(bh), Some(ah)) => Some((bh ^ ah).count_ones()),
        _ => None,
    };

    let before_snapshot_url = before_bytes.and_then(|bytes| {
        let name = format!("{rev_id}_before.{ext}");
        fs::write(snap_dir.join(&name), bytes).ok()?;
        Some(format!(
            "/admin/revision-snapshots/{entity_type}/{slug}/{name}"
        ))
    });
    let after_snapshot_url = after_bytes.and_then(|bytes| {
        let name = format!("{rev_id}_after.{ext}");
        fs::write(snap_dir.join(&name), bytes).ok()?;
        Some(format!(
            "/admin/revision-snapshots/{entity_type}/{slug}/{name}"
        ))
    });

    let action = match change_kind {
        "added" => "file_added",
        "deleted" => "file_deleted",
        "edited" => "image_edited",
        "external_modified" => "external_modified",
        _ => "file_replaced",
    };

    let summary = match change_kind {
        "added" => format!("Added file '{filename}'"),
        "deleted" => format!("Removed file '{filename}'"),
        "edited" => format!("Edited image/file '{filename}'"),
        "external_modified" => format!("Detected external on-disk modification to '{filename}'"),
        _ => format!("Replaced file '{filename}'"),
    };

    let entry = RevisionEntry {
        id: rev_id,
        entity_type: entity_type.to_string(),
        slug: slug.to_string(),
        title: title.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        actor: actor.to_string(),
        action: action.to_string(),
        summary,
        field_diffs: Vec::new(),
        file_diff: Some(FileDiff {
            filename: filename.to_string(),
            change_kind: change_kind.to_string(),
            is_image: is_img,
            before_sha256: before_sha,
            after_sha256: after_sha,
            before_size: before_bytes.map(|b| b.len() as u64),
            after_size: after_bytes.map(|b| b.len() as u64),
            before_dims,
            after_dims,
            perceptual_distance,
            before_snapshot_url,
            after_snapshot_url,
        }),
    };

    let mut history = load_history(cfg, entity_type, slug);
    history.push(entry.clone());
    if history.len() > 50 {
        let drop_n = history.len() - 50;
        history.drain(0..drop_n);
    }
    let _ = save_history(cfg, entity_type, slug, &history);
    Some(entry)
}

pub fn get_entity_revisions(cfg: &AppConfig, entity_type: &str, slug: &str) -> Vec<RevisionEntry> {
    let mut list = load_history(cfg, entity_type, slug);
    list.reverse(); // Most recent first
    list
}

/// Fast summary count `(revision_count, last_modified_at)` for cards in Admin Portal.
pub fn revision_summary_map(
    cfg: &AppConfig,
    entity_type: &str,
) -> HashMap<String, (usize, String)> {
    let mut map = HashMap::new();
    let Some(et) = sanitize_segment(entity_type) else {
        return map;
    };
    let root = cfg.revisions_dir.join(et);
    let Ok(entries) = fs::read_dir(&root) else {
        return map;
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let Some(slug) = entry.file_name().to_str().map(String::from) else {
            continue;
        };
        let hist = load_history(cfg, entity_type, &slug);
        if let Some(last) = hist.last() {
            map.insert(slug, (hist.len(), last.timestamp.clone()));
        }
    }
    map
}

/// List recent revisions across all entities (optionally filtered by entity_type).
pub fn list_recent_revisions(
    cfg: &AppConfig,
    entity_type_filter: Option<&str>,
    limit: usize,
) -> Vec<RevisionEntry> {
    let mut all = Vec::new();
    let types: Vec<&str> = match entity_type_filter.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => vec![t],
        None => vec!["document", "profile", "location", "category", "group"],
    };
    for et in types {
        let root = cfg.revisions_dir.join(et);
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            if let Some(slug) = entry.file_name().to_str() {
                all.extend(load_history(cfg, et, slug));
            }
        }
    }
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    all.truncate(limit.clamp(1, 200));
    all
}

pub fn resolve_snapshot_path(
    cfg: &AppConfig,
    entity_type: &str,
    slug: &str,
    filename: &str,
) -> Option<PathBuf> {
    let dir = entity_rev_dir(cfg, entity_type, slug)?;
    let fname = sanitize_segment(filename)?;
    let path = dir.join("snapshots").join(fname);
    if path.is_file() { Some(path) } else { None }
}

/// Compare files on disk against `checksum_manifest.json` to detect external tampering/edits.
pub fn scan_external_disk_changes(cfg: &AppConfig) -> Value {
    let Ok(raw) = fs::read_to_string(&cfg.checksum_manifest_file) else {
        return json!({
            "status": "ok",
            "checked_files": 0,
            "modified_count": 0,
            "missing_count": 0,
            "findings": [],
        });
    };
    let Ok(manifest) = serde_json::from_str::<Value>(&raw) else {
        return json!({
            "status": "ok",
            "checked_files": 0,
            "modified_count": 0,
            "missing_count": 0,
            "findings": [],
        });
    };
    let entries = manifest
        .get("entries")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let mut checked = 0usize;
    let mut modified_count = 0usize;
    let mut missing_count = 0usize;
    let mut findings = Vec::new();

    for (rel_or_abs, info) in entries {
        checked += 1;
        let expected_sha = info["sha256"].as_str().unwrap_or("");
        let label = info["label"].as_str().unwrap_or(&rel_or_abs);
        let candidate = PathBuf::from(&rel_or_abs);
        let full_path = if candidate.is_absolute() {
            candidate
        } else {
            cfg.project_root.join(&rel_or_abs)
        };
        if !full_path.is_file() {
            missing_count += 1;
            findings.push(json!({
                "path": rel_or_abs,
                "label": label,
                "kind": "missing_on_disk",
                "expected_sha256": expected_sha,
                "actual_sha256": Value::Null,
                "detected_at": Utc::now().to_rfc3339(),
            }));
            continue;
        }
        if let Ok(bytes) = fs::read(&full_path) {
            let actual_sha = sha256_bytes(&bytes);
            if !expected_sha.is_empty() && actual_sha != expected_sha {
                modified_count += 1;
                findings.push(json!({
                    "path": rel_or_abs,
                    "label": label,
                    "kind": "modified_externally",
                    "expected_sha256": expected_sha,
                    "actual_sha256": actual_sha,
                    "size_bytes": bytes.len(),
                    "detected_at": Utc::now().to_rfc3339(),
                }));
            }
        }
    }

    json!({
        "status": if modified_count > 0 || missing_count > 0 { "changes_detected" } else { "clean" },
        "checked_files": checked,
        "modified_count": modified_count,
        "missing_count": missing_count,
        "findings": findings,
        "scanned_at": Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};
    use std::io::Cursor;

    fn temp_cfg() -> AppConfig {
        let mut cfg = AppConfig::load();
        let root = std::env::temp_dir().join(format!("arkib_rev_test_{}", Uuid::new_v4().simple()));
        cfg.revisions_dir = root.join("revisions");
        cfg.checksum_manifest_file = root.join("checksum_manifest.json");
        let _ = fs::create_dir_all(&cfg.revisions_dir);
        cfg
    }

    fn png_bytes(r: u8, g: u8, b: u8, stripe: bool) -> Vec<u8> {
        let mut img = RgbaImage::from_pixel(32, 32, Rgba([r, g, b, 255]));
        if stripe {
            for y in 0..16 {
                for x in 0..32 {
                    img.put_pixel(x, y, Rgba([255 - r, 255 - g, 255 - b, 255]));
                }
            }
        }
        let mut buf = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn metadata_and_image_revisions_capture_before_and_after() {
        let cfg = temp_cfg();
        let before = json!({
            "title": "Surat Lama",
            "category": "Manuskrip",
            "year": "1957",
            "author_or_source": "Arkib Negara"
        })
        .as_object()
        .unwrap()
        .clone();
        let after = json!({
            "title": "Surat Merdeka 1957",
            "category": "Surat Rasmi",
            "year": "1957",
            "author_or_source": "Jabatan Perdana Menteri"
        })
        .as_object()
        .unwrap()
        .clone();

        let rev = record_metadata_change(
            &cfg,
            "document",
            "surat_merdeka",
            "Surat Merdeka 1957",
            "admin",
            "metadata_edit",
            &before,
            &after,
        )
        .expect("should record diff");
        assert_eq!(rev.field_diffs.len(), 3);

        let img_a = png_bytes(20, 20, 20, false);
        let img_b = png_bytes(20, 20, 20, true);
        let file_rev = record_file_change(
            &cfg,
            "document",
            "surat_merdeka",
            "Surat Merdeka 1957",
            "admin",
            "scan.png",
            "edited",
            Some(&img_a),
            Some(&img_b),
        )
        .expect("should record image change");
        let fd = file_rev.file_diff.expect("file diff");
        assert!(fd.is_image);
        assert_ne!(fd.before_sha256, fd.after_sha256);
        assert!(fd.perceptual_distance.unwrap_or(0) > 0);
        assert!(fd.before_snapshot_url.is_some());
        assert!(fd.after_snapshot_url.is_some());

        let hist = get_entity_revisions(&cfg, "document", "surat_merdeka");
        assert_eq!(hist.len(), 2);

        let _ = fs::remove_dir_all(cfg.revisions_dir.parent().unwrap());
    }
}
