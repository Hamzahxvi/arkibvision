#![forbid(unsafe_code)]

//! Managed Archival Folders (Locations & Documents) + Multi-Format Document Extraction
//! & Automatic Archival Summarization (100% Pure Safe Rust).

use crate::ancient_scripts::detect_script;
use crate::biometrics::FaceEngine;
use crate::config::{AppConfig, OLLAMA_UNIFORM_NUM_CTX, is_document_ext, is_image_ext};
use crate::conservation::analyze_image_quality_and_conservation;
use crate::forensics::{
    analyze_authenticity, extract_file_metadata, find_duplicate_in_archives,
    register_checksum_entry, sha256_bytes,
};
use crate::jawi::extract_hijri_colophon_dates;
use crate::metadata::{profile_slug, value_to_trimmed_string};
use crate::vector_store::{FacePoint, VectorStore};
use chrono::Utc;
use flate2::read::ZlibDecoder;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use uuid::Uuid;

// ─── Managed Location Folders (e.g. National Archives of Malaysia wings/rooms) ───

pub fn ensure_default_locations(cfg: &AppConfig) {
    let has_any = fs::read_dir(&cfg.locations_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| e.path().join("metadata.json").is_file());
    if has_any {
        return;
    }
    let samples = [
        json!({
            "name": "Arkib Negara Malaysia — Dewan Penyelidikan",
            "group_slug": "arkib_negara_malaysia",
            "group": "Arkib Negara Malaysia",
            "parent_institution": "Arkib Negara Malaysia (National Archives of Malaysia)",
            "building_or_wing": "Bangunan Utama (Wisma Warisan)",
            "floor_or_room": "Tingkat 1, Dewan Penyelidikan",
            "category": "Research Hall",
            "address": "Jalan Tuanku Abdul Halim, 50568 Kuala Lumpur, Malaysia",
            "coordinates": "3.1736, 101.6776",
            "historical_significance": "Primary archival reading room housing microfilm, classical Malay manuscripts (Jawi, treaties, colonial records), and national historical files for researchers.",
            "description": "Main public research hall at the National Archives of Malaysia (Arkib Negara Malaysia) in Kuala Lumpur.",
            "notes": "Editable sample archival location. Add reference photos so uploaded photos automatically recognize this hall."
        }),
        json!({
            "name": "Arkib Negara Malaysia — Galeri Pameran Tetap",
            "group_slug": "arkib_negara_malaysia",
            "group": "Arkib Negara Malaysia",
            "parent_institution": "Arkib Negara Malaysia (National Archives of Malaysia)",
            "building_or_wing": "Blok Pameran & Memorial",
            "floor_or_room": "Aras Bawah, Galeri Utama",
            "category": "Exhibition Gallery",
            "address": "Jalan Tuanku Abdul Halim, 50568 Kuala Lumpur, Malaysia",
            "coordinates": "3.1738, 101.6779",
            "historical_significance": "Permanent exhibition gallery showcasing independence declarations, royal manuscripts, historical photographs, and Malaysia's documentary heritage.",
            "description": "Permanent historical exhibition space inside the National Archives of Malaysia.",
            "notes": "Editable sample archival location."
        }),
    ];
    for item in samples {
        if let Some(obj) = item.as_object() {
            let _ = save_location(cfg, None, obj);
        }
    }
}

pub fn list_locations(cfg: &AppConfig) -> Vec<Value> {
    let mut items = Vec::new();
    if let Ok(entries) = fs::read_dir(&cfg.locations_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && let Some(slug) = p.file_name().and_then(|s| s.to_str())
                && let Some(rec) = load_location(cfg, slug)
            {
                items.push(rec);
            }
        }
    }
    items.sort_by(|a, b| {
        let na = a["name"].as_str().unwrap_or("").to_ascii_lowercase();
        let nb = b["name"].as_str().unwrap_or("").to_ascii_lowercase();
        na.cmp(&nb)
    });
    items
}

pub fn load_location(cfg: &AppConfig, slug: &str) -> Option<Value> {
    let meta_path = cfg.locations_dir.join(slug).join("metadata.json");
    let text = fs::read_to_string(&meta_path).ok()?;
    let mut meta = serde_json::from_str::<Value>(&text)
        .ok()?
        .as_object()
        .cloned()?;
    let images = collect_location_images(cfg, slug);
    meta.insert("slug".to_string(), Value::String(slug.to_string()));
    meta.insert("image_count".to_string(), json!(images.len()));
    meta.insert("images".to_string(), Value::Array(images));
    Some(Value::Object(meta))
}

fn collect_location_images(cfg: &AppConfig, slug: &str) -> Vec<Value> {
    let dir = cfg.locations_dir.join(slug).join("images");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_image_ext(p))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let fname = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            json!({
                "filename": fname,
                "admin_url": format!("/admin/location-images/{slug}/{fname}"),
                "url": format!("{}/images/location-gallery/{slug}/{fname}", cfg.librechat_url),
            })
        })
        .collect()
}

pub fn save_location(
    cfg: &AppConfig,
    existing_slug: Option<&str>,
    payload: &Map<String, Value>,
) -> Result<Value, String> {
    let name = payload
        .get("name")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    if name.is_empty() && existing_slug.is_none() {
        return Err("Location name is required.".to_string());
    }

    let target_slug = if let Some(old_slug) = existing_slug {
        if !name.is_empty() {
            let candidate = profile_slug(&name);
            if candidate.is_empty() {
                old_slug.to_string()
            } else {
                candidate
            }
        } else {
            old_slug.to_string()
        }
    } else {
        let s = profile_slug(&name);
        if s.is_empty() {
            format!("loc_{}", &Uuid::new_v4().simple().to_string()[..8])
        } else {
            s
        }
    };

    if let Some(old_slug) = existing_slug
        && old_slug != target_slug
    {
        let old_dir = cfg.locations_dir.join(old_slug);
        let new_dir = cfg.locations_dir.join(&target_slug);
        if old_dir.is_dir() {
            if new_dir.exists() {
                return Err(format!("Location folder '{target_slug}' already exists."));
            }
            fs::rename(&old_dir, &new_dir).map_err(|e| format!("Rename failed: {e}"))?;
        }
        let old_web = cfg.location_web_dir.join(old_slug);
        let new_web = cfg.location_web_dir.join(&target_slug);
        if old_web.is_dir() && !new_web.exists() {
            let _ = fs::rename(&old_web, &new_web);
        }
    }

    let loc_dir = cfg.locations_dir.join(&target_slug);
    let images_dir = loc_dir.join("images");
    let web_dir = cfg.location_web_dir.join(&target_slug);
    fs::create_dir_all(&images_dir).map_err(|e| format!("Create dir failed: {e}"))?;
    let _ = fs::create_dir_all(&web_dir);

    let meta_path = loc_dir.join("metadata.json");
    let mut meta: Map<String, Value> = fs::read_to_string(&meta_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    for key in [
        "name",
        "parent_institution",
        "building_or_wing",
        "floor_or_room",
        "category",
        "address",
        "coordinates",
        "historical_significance",
        "description",
        "notes",
    ] {
        if let Some(val) = payload.get(key)
            && !val.is_null()
        {
            meta.insert(key.to_string(), Value::String(value_to_trimmed_string(val)));
        } else {
            meta.entry(key.to_string())
                .or_insert_with(|| Value::String(String::new()));
        }
    }
    if payload.contains_key("group_slug") || payload.contains_key("group") {
        let raw = payload
            .get("group_slug")
            .or_else(|| payload.get("group"))
            .map(value_to_trimmed_string)
            .unwrap_or_default();
        if raw.is_empty() {
            meta.insert("group_slug".to_string(), Value::String(String::new()));
            meta.insert("group".to_string(), Value::String(String::new()));
        } else if let Some((gs, gn)) = resolve_location_group(cfg, &raw) {
            meta.insert("group_slug".to_string(), Value::String(gs));
            meta.insert("group".to_string(), Value::String(gn));
        } else {
            meta.insert("group_slug".to_string(), Value::String(group_slug(&raw)));
            meta.insert("group".to_string(), Value::String(raw));
        }
    } else {
        meta.entry("group_slug".to_string())
            .or_insert_with(|| Value::String(String::new()));
        meta.entry("group".to_string())
            .or_insert_with(|| Value::String(String::new()));
    }
    meta.insert("slug".to_string(), Value::String(target_slug.clone()));
    meta.insert(
        "updated_at".to_string(),
        Value::String(Utc::now().to_rfc3339()),
    );
    meta.entry("created_at".to_string())
        .or_insert_with(|| Value::String(Utc::now().to_rfc3339()));

    let pretty =
        serde_json::to_string_pretty(&Value::Object(meta)).map_err(|e| format!("JSON err: {e}"))?;
    fs::write(&meta_path, pretty).map_err(|e| format!("Write metadata failed: {e}"))?;

    load_location(cfg, &target_slug).ok_or_else(|| "Failed reloading location.".to_string())
}

pub fn add_location_image(
    cfg: &AppConfig,
    engine: &FaceEngine,
    loc_store: &VectorStore,
    slug: &str,
    orig_filename: &str,
    bytes: &[u8],
    force: bool,
) -> Result<Value, String> {
    let loc = load_location(cfg, slug).ok_or_else(|| "Location not found.".to_string())?;
    let loc_name = loc["name"].as_str().unwrap_or(slug).to_string();

    if !force {
        let roots = [
            cfg.profile_dir.as_path(),
            cfg.locations_dir.as_path(),
            cfg.documents_dir.as_path(),
        ];
        if let Some(dup) = find_duplicate_in_archives(bytes, orig_filename, &roots) {
            let kind = dup["duplicate_type"].as_str().unwrap_or("duplicate");
            let existing = dup["existing_file"].as_str().unwrap_or("");
            return Err(format!(
                "Duplicate item detected ({kind}): this image already exists in the archive at '{existing}'. Pass force=1 to override."
            ));
        }
    }

    let safe_stem = profile_slug(
        Path::new(orig_filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("photo"),
    );
    let ext = Path::new(orig_filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg")
        .to_ascii_lowercase();
    let short = &Uuid::new_v4().simple().to_string()[..6];
    let final_name = format!(
        "{}_{short}.{ext}",
        if safe_stem.is_empty() {
            "photo"
        } else {
            &safe_stem
        }
    );

    let img_dir = cfg.locations_dir.join(slug).join("images");
    let web_dir = cfg.location_web_dir.join(slug);
    fs::create_dir_all(&img_dir).map_err(|e| format!("Create dir failed: {e}"))?;
    let _ = fs::create_dir_all(&web_dir);

    let disk_path = img_dir.join(&final_name);
    fs::write(&disk_path, bytes).map_err(|e| format!("Write image failed: {e}"))?;
    let _ = fs::write(web_dir.join(&final_name), bytes);
    register_checksum_entry(
        &cfg.checksum_manifest_file,
        &disk_path,
        &sha256_bytes(bytes),
        bytes.len() as u64,
    );

    // Index full photo with CLIP ViT-B/32 (768-D) for automatic landmark/location recognition!
    if let Ok(embedding) = engine.embed_full_image_from_path(&disk_path) {
        let pt = FacePoint::new(
            format!("{slug}::{final_name}"),
            loc_name,
            final_name.clone(),
            slug.to_string(),
            embedding,
        );
        let _ = loc_store.upsert(pt);
    }

    load_location(cfg, slug).ok_or_else(|| "Failed reloading location.".to_string())
}

pub fn delete_location_image(cfg: &AppConfig, slug: &str, filename: &str) -> Result<Value, String> {
    let disk_path = cfg.locations_dir.join(slug).join("images").join(filename);
    if !disk_path.is_file() {
        return Err("Location image not found.".to_string());
    }
    let _ = fs::remove_file(&disk_path);
    let _ = fs::remove_file(cfg.location_web_dir.join(slug).join(filename));
    load_location(cfg, slug).ok_or_else(|| "Location not found.".to_string())
}

pub fn delete_location(
    cfg: &AppConfig,
    loc_store: &VectorStore,
    slug: &str,
) -> Result<Value, String> {
    let loc = load_location(cfg, slug).ok_or_else(|| "Location not found.".to_string())?;
    let name = loc["name"].as_str().unwrap_or(slug).to_string();
    let _ = fs::remove_dir_all(cfg.locations_dir.join(slug));
    let _ = fs::remove_dir_all(cfg.location_web_dir.join(slug));
    let _ = loc_store.delete_identity(&name);
    Ok(json!({ "status": "deleted", "slug": slug, "name": name }))
}

pub fn search_locations(cfg: &AppConfig, query: &str) -> Vec<Value> {
    let all = list_locations(cfg);
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return all;
    }
    let tokens: Vec<&str> = q.split_whitespace().collect();
    all.into_iter()
        .filter(|loc| {
            let hay = [
                "name",
                "parent_institution",
                "building_or_wing",
                "floor_or_room",
                "category",
                "group",
                "address",
                "historical_significance",
                "description",
                "notes",
            ]
            .iter()
            .filter_map(|k| loc.get(*k).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
            tokens.iter().all(|t| hay.contains(t))
        })
        .collect()
}

// ─── Managed Document Folders & Multi-Format Extraction ──────────────────────

pub fn list_documents(cfg: &AppConfig) -> Vec<Value> {
    let mut items = Vec::new();
    if let Ok(entries) = fs::read_dir(&cfg.documents_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && let Some(slug) = p.file_name().and_then(|s| s.to_str())
                && let Some(rec) = load_document(cfg, slug)
            {
                items.push(rec);
            }
        }
    }
    items.sort_by(|a, b| {
        let ta = a["updated_at"].as_str().unwrap_or("");
        let tb = b["updated_at"].as_str().unwrap_or("");
        tb.cmp(ta)
    });
    items
}

pub fn load_document(cfg: &AppConfig, slug: &str) -> Option<Value> {
    let meta_path = cfg.documents_dir.join(slug).join("metadata.json");
    let text = fs::read_to_string(&meta_path).ok()?;
    let mut meta = serde_json::from_str::<Value>(&text)
        .ok()?
        .as_object()
        .cloned()?;
    let files = collect_document_files(cfg, slug);
    meta.insert("slug".to_string(), Value::String(slug.to_string()));
    meta.insert("file_count".to_string(), json!(files.len()));
    meta.insert("files".to_string(), Value::Array(files));
    Some(Value::Object(meta))
}

fn collect_document_files(cfg: &AppConfig, slug: &str) -> Vec<Value> {
    let dir = cfg.documents_dir.join(slug).join("files");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_document_ext(p))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let fname = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let size = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            let is_img = is_image_ext(&p);
            json!({
                "filename": fname,
                "size_bytes": size,
                "is_image": is_img,
                "admin_url": format!("/admin/document-files/{slug}/{fname}"),
                "url": format!("{}/images/document-gallery/{slug}/{fname}", cfg.librechat_url),
            })
        })
        .collect()
}

pub fn save_document(
    cfg: &AppConfig,
    existing_slug: Option<&str>,
    payload: &Map<String, Value>,
) -> Result<Value, String> {
    let title = payload
        .get("title")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    if title.is_empty() && existing_slug.is_none() {
        return Err("Document title is required.".to_string());
    }

    let target_slug = if let Some(old_slug) = existing_slug {
        if !title.is_empty() {
            let cand = profile_slug(&title);
            if cand.is_empty() {
                old_slug.to_string()
            } else {
                cand
            }
        } else {
            old_slug.to_string()
        }
    } else {
        let s = profile_slug(&title);
        if s.is_empty() {
            format!("doc_{}", &Uuid::new_v4().simple().to_string()[..8])
        } else {
            s
        }
    };

    if let Some(old_slug) = existing_slug
        && old_slug != target_slug
    {
        let old_dir = cfg.documents_dir.join(old_slug);
        let new_dir = cfg.documents_dir.join(&target_slug);
        if old_dir.is_dir() {
            if new_dir.exists() {
                return Err(format!("Document folder '{target_slug}' already exists."));
            }
            fs::rename(&old_dir, &new_dir).map_err(|e| format!("Rename failed: {e}"))?;
        }
        let old_web = cfg.document_web_dir.join(old_slug);
        let new_web = cfg.document_web_dir.join(&target_slug);
        if old_web.is_dir() && !new_web.exists() {
            let _ = fs::rename(&old_web, &new_web);
        }
    }

    let doc_dir = cfg.documents_dir.join(&target_slug);
    let files_dir = doc_dir.join("files");
    let web_dir = cfg.document_web_dir.join(&target_slug);
    fs::create_dir_all(&files_dir).map_err(|e| format!("Create dir failed: {e}"))?;
    let _ = fs::create_dir_all(&web_dir);

    let meta_path = doc_dir.join("metadata.json");
    let mut meta: Map<String, Value> = fs::read_to_string(&meta_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    for key in [
        "title",
        "reference_code",
        "category",
        "script_type",
        "date_created",
        "origin_location",
        "author_or_source",
        "volume_id",
        "folio_number",
        "hijri_date",
        "gregorian_date_masihi",
        "summary",
        "extracted_text",
        "notes",
    ] {
        if let Some(val) = payload.get(key)
            && !val.is_null()
        {
            meta.insert(key.to_string(), Value::String(value_to_trimmed_string(val)));
        } else {
            meta.entry(key.to_string())
                .or_insert_with(|| Value::String(String::new()));
        }
    }
    if payload.contains_key("group_slug") || payload.contains_key("group") {
        let raw = payload
            .get("group_slug")
            .or_else(|| payload.get("group"))
            .map(value_to_trimmed_string)
            .unwrap_or_default();
        if raw.is_empty() {
            meta.insert("group_slug".to_string(), Value::String(String::new()));
            meta.insert("group".to_string(), Value::String(String::new()));
        } else if let Some((gs, gn)) = resolve_document_group(cfg, &raw) {
            meta.insert("group_slug".to_string(), Value::String(gs));
            meta.insert("group".to_string(), Value::String(gn));
        } else {
            meta.insert("group_slug".to_string(), Value::String(group_slug(&raw)));
            meta.insert("group".to_string(), Value::String(raw));
        }
    } else {
        meta.entry("group_slug".to_string())
            .or_insert_with(|| Value::String(String::new()));
        meta.entry("group".to_string())
            .or_insert_with(|| Value::String(String::new()));
    }
    for arr_key in ["key_topics", "key_names", "key_dates"] {
        if let Some(val) = payload.get(arr_key) {
            if val.is_array() {
                meta.insert(arr_key.to_string(), val.clone());
            } else if let Some(s) = val.as_str() {
                let items: Vec<Value> = s
                    .split(',')
                    .map(|x| x.trim())
                    .filter(|x| !x.is_empty())
                    .map(|x| Value::String(x.to_string()))
                    .collect();
                meta.insert(arr_key.to_string(), Value::Array(items));
            }
        } else {
            meta.entry(arr_key.to_string())
                .or_insert_with(|| Value::Array(Vec::new()));
        }
    }
    if let Some(auth) = payload.get("authenticity_report") {
        meta.insert("authenticity_report".to_string(), auth.clone());
    }
    if let Some(cons) = payload.get("conservation_report") {
        meta.insert("conservation_report".to_string(), cons.clone());
    }

    // Auto-extract key metadata whenever extracted_text is provided and summary/topics are empty.
    let extracted_text = meta
        .get("extracted_text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !extracted_text.trim().is_empty() {
        let structured = extract_document_structure(&extracted_text);
        if meta
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            meta.insert(
                "summary".to_string(),
                structured["extractive_summary"].clone(),
            );
        }
        if meta
            .get("key_topics")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        {
            meta.insert("key_topics".to_string(), structured["key_topics"].clone());
        }
        if meta
            .get("key_dates")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        {
            meta.insert("key_dates".to_string(), structured["key_dates"].clone());
        }
        if meta
            .get("key_names")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        {
            meta.insert("key_names".to_string(), structured["key_names"].clone());
        }
        if meta
            .get("hijri_date")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
            && let Some(h) = structured.get("hijri_date").and_then(Value::as_str)
            && !h.is_empty()
        {
            meta.insert("hijri_date".to_string(), Value::String(h.to_string()));
        }
        if meta
            .get("gregorian_date_masihi")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
            && let Some(g) = structured
                .get("gregorian_date_masihi")
                .and_then(Value::as_str)
            && !g.is_empty()
        {
            meta.insert(
                "gregorian_date_masihi".to_string(),
                Value::String(g.to_string()),
            );
        }
    }

    meta.insert("slug".to_string(), Value::String(target_slug.clone()));
    meta.insert(
        "updated_at".to_string(),
        Value::String(Utc::now().to_rfc3339()),
    );
    meta.entry("created_at".to_string())
        .or_insert_with(|| Value::String(Utc::now().to_rfc3339()));

    let pretty =
        serde_json::to_string_pretty(&Value::Object(meta)).map_err(|e| format!("JSON err: {e}"))?;
    fs::write(&meta_path, pretty).map_err(|e| format!("Write metadata failed: {e}"))?;

    load_document(cfg, &target_slug).ok_or_else(|| "Failed reloading document.".to_string())
}

pub fn add_document_file(
    cfg: &AppConfig,
    slug: &str,
    orig_filename: &str,
    bytes: &[u8],
    force: bool,
) -> Result<Value, String> {
    let _doc = load_document(cfg, slug).ok_or_else(|| "Document folder not found.".to_string())?;
    let roots = [
        cfg.profile_dir.as_path(),
        cfg.locations_dir.as_path(),
        cfg.documents_dir.as_path(),
    ];
    if !force && let Some(dup) = find_duplicate_in_archives(bytes, orig_filename, &roots) {
        let kind = dup["duplicate_type"].as_str().unwrap_or("duplicate");
        let existing = dup["existing_file"].as_str().unwrap_or("");
        return Err(format!(
            "Duplicate item detected ({kind}): this file already exists in the archive at '{existing}'. Pass force=1 to override."
        ));
    }

    let safe_stem = profile_slug(
        Path::new(orig_filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file"),
    );
    let ext = Path::new(orig_filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("txt")
        .to_ascii_lowercase();
    let short = &Uuid::new_v4().simple().to_string()[..6];
    let final_name = format!(
        "{}_{short}.{ext}",
        if safe_stem.is_empty() {
            "file"
        } else {
            &safe_stem
        }
    );

    let files_dir = cfg.documents_dir.join(slug).join("files");
    let web_dir = cfg.document_web_dir.join(slug);
    fs::create_dir_all(&files_dir).map_err(|e| format!("Create dir failed: {e}"))?;
    let _ = fs::create_dir_all(&web_dir);

    let disk_path = files_dir.join(&final_name);
    fs::write(&disk_path, bytes).map_err(|e| format!("Write file failed: {e}"))?;
    let _ = fs::write(web_dir.join(&final_name), bytes);
    register_checksum_entry(
        &cfg.checksum_manifest_file,
        &disk_path,
        &sha256_bytes(bytes),
        bytes.len() as u64,
    );

    // Run authenticity & duplicate check against entire archive + conservation analysis (if image) + extract text!
    let auth = analyze_authenticity(&disk_path, &roots).ok();
    let conservation = if is_image_ext(&disk_path) {
        analyze_image_quality_and_conservation(&disk_path).ok()
    } else {
        None
    };
    let extracted = extract_text_from_file(&disk_path).ok().unwrap_or_default();

    let mut patch = Map::new();
    if let Some(a) = auth {
        patch.insert("authenticity_report".to_string(), a);
    }
    if let Some(c) = conservation {
        patch.insert("conservation_report".to_string(), c);
    }
    if !extracted.trim().is_empty() {
        let existing_text = load_document(cfg, slug)
            .and_then(|d| d["extracted_text"].as_str().map(String::from))
            .unwrap_or_default();
        let combined = if existing_text.trim().is_empty() {
            extracted
        } else {
            format!("{existing_text}\n\n--- {final_name} ---\n{extracted}")
        };
        let detected = detect_script(&combined);
        patch.insert("extracted_text".to_string(), Value::String(combined));
        if detected != crate::ancient_scripts::AncientScript::LatinOrUnknown {
            patch.insert(
                "script_type".to_string(),
                Value::String(detected.display_name().to_string()),
            );
        }
    }
    save_document(cfg, Some(slug), &patch)
}

pub fn delete_document_file(cfg: &AppConfig, slug: &str, filename: &str) -> Result<Value, String> {
    let disk_path = cfg.documents_dir.join(slug).join("files").join(filename);
    if !disk_path.is_file() {
        return Err("Document file not found.".to_string());
    }
    let _ = fs::remove_file(&disk_path);
    let _ = fs::remove_file(cfg.document_web_dir.join(slug).join(filename));
    load_document(cfg, slug).ok_or_else(|| "Document folder not found.".to_string())
}

pub fn delete_document(cfg: &AppConfig, slug: &str) -> Result<Value, String> {
    let doc = load_document(cfg, slug).ok_or_else(|| "Document folder not found.".to_string())?;
    let title = doc["title"].as_str().unwrap_or(slug).to_string();
    let _ = fs::remove_dir_all(cfg.documents_dir.join(slug));
    let _ = fs::remove_dir_all(cfg.document_web_dir.join(slug));
    Ok(json!({ "status": "deleted", "slug": slug, "title": title }))
}

pub fn search_documents(cfg: &AppConfig, query: &str, script_filter: &str) -> Vec<Value> {
    let all = list_documents(cfg);
    let q = query.trim().to_ascii_lowercase();
    let sf = script_filter.trim().to_ascii_lowercase();
    let tokens: Vec<&str> = q.split_whitespace().collect();

    all.into_iter()
        .filter(|doc| {
            if !sf.is_empty() {
                let st = doc["script_type"]
                    .as_str()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !st.contains(&sf) {
                    return false;
                }
            }
            if tokens.is_empty() {
                return true;
            }
            let hay = [
                "title",
                "reference_code",
                "category",
                "script_type",
                "date_created",
                "origin_location",
                "author_or_source",
                "summary",
                "extracted_text",
                "notes",
            ]
            .iter()
            .filter_map(|k| doc.get(*k).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
            tokens.iter().all(|t| hay.contains(t))
        })
        .collect()
}

// ─── Multi-Format Document Text Extraction (TXT/MD/CSV/JSON/HTML/DOCX/PDF) ───

pub fn extract_text_from_file(path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "md" | "csv" | "json" => {
            fs::read_to_string(path).map_err(|e| format!("Failed reading text file: {e}"))
        }
        "html" | "htm" | "xml" => {
            let raw =
                fs::read_to_string(path).map_err(|e| format!("Failed reading XML/HTML: {e}"))?;
            Ok(strip_xml_tags(&raw))
        }
        "docx" => extract_docx_text(path),
        "pdf" => extract_pdf_text(path),
        _ => Ok(String::new()),
    }
}

fn strip_xml_tags(xml: &str) -> String {
    static TAG_RE: OnceLock<Regex> = OnceLock::new();
    let re = TAG_RE.get_or_init(|| Regex::new(r"<[^>]+>").expect("regex"));
    let with_breaks = xml
        .replace("</w:p>", "\n")
        .replace("</p>", "\n")
        .replace("<br/>", "\n")
        .replace("<br>", "\n");
    let stripped = re.replace_all(&with_breaks, " ");
    stripped
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_docx_text(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("Read DOCX failed: {e}"))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("Invalid DOCX zip archive: {e}"))?;
    let mut xml = String::new();
    let mut doc_file = archive
        .by_name("word/document.xml")
        .map_err(|e| format!("Missing word/document.xml: {e}"))?;
    doc_file
        .read_to_string(&mut xml)
        .map_err(|e| format!("Read document.xml failed: {e}"))?;
    Ok(strip_xml_tags(&xml))
}

/// Pure-Rust PDF text extractor powered by `lopdf` (`/ToUnicode` CMap & UTF-16BE decoding)
/// with graceful fallback to FlateDecode stream scraping and empty-text pass-through for scanned PDFs.
fn extract_pdf_text(path: &Path) -> Result<String, String> {
    if let Ok(doc) = lopdf::Document::load(path) {
        let mut page_nums: Vec<u32> = doc.get_pages().keys().copied().collect();
        page_nums.sort_unstable();
        let mut pages_text = Vec::new();
        for pno in page_nums {
            if let Ok(txt) = doc.extract_text(&[pno]) {
                let trimmed = txt.trim();
                if !trimmed.is_empty() {
                    pages_text.push(trimmed.to_string());
                }
            }
        }
        if !pages_text.is_empty() {
            return Ok(pages_text.join("\n\n"));
        }
    }

    // Fallback for malformed legacy PDFs where lopdf cannot build the page tree.
    let bytes = fs::read(path).map_err(|e| format!("Read PDF failed: {e}"))?;
    let mut extracted = Vec::new();
    let mut pos = 0usize;
    while let Some(s_off) = find_bytes(&bytes[pos..], b"stream") {
        let mut start = pos + s_off + 6;
        if bytes.get(start) == Some(&b'\r') {
            start += 1;
        }
        if bytes.get(start) == Some(&b'\n') {
            start += 1;
        }
        let Some(e_off) = find_bytes(&bytes[start..], b"endstream") else {
            break;
        };
        let stream_data = &bytes[start..start + e_off];
        let mut decompressed = Vec::new();
        let content_bytes: &[u8] = {
            let mut dec = ZlibDecoder::new(stream_data);
            if dec.read_to_end(&mut decompressed).is_ok() && !decompressed.is_empty() {
                &decompressed
            } else {
                stream_data
            }
        };
        let chunk = extract_pdf_operators(&String::from_utf8_lossy(content_bytes));
        if !chunk.is_empty() {
            extracted.push(chunk);
        }
        pos = start + e_off + 9;
    }
    Ok(extracted.join("\n"))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn extract_pdf_operators(stream_text: &str) -> String {
    static LIT_RE: OnceLock<Regex> = OnceLock::new();
    let re = LIT_RE.get_or_init(|| Regex::new(r"\(([^()\\]*(?:\\.[^()\\]*)*)\)").expect("regex"));
    let mut parts = Vec::new();
    if !stream_text.contains("Tj") && !stream_text.contains("TJ") {
        return String::new();
    }
    for caps in re.captures_iter(stream_text) {
        let raw = caps[1]
            .replace(r"\(", "(")
            .replace(r"\)", ")")
            .replace(r"\\", "\\")
            .replace(r"\n", "\n");
        let clean: String = raw
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .collect();
        if !clean.trim().is_empty() {
            parts.push(clean);
        }
    }
    parts.join(" ")
}

// ─── Structured Archival Extraction & Automatic Summarization ────────────────

pub fn extract_document_structure(text: &str) -> Value {
    static DATE_RE: OnceLock<Regex> = OnceLock::new();
    static NAME_RE: OnceLock<Regex> = OnceLock::new();
    static WORD_RE: OnceLock<Regex> = OnceLock::new();

    let date_re = DATE_RE.get_or_init(|| {
        Regex::new(
            r"\b(?:\d{1,2}[/-]\d{1,2}[/-]\d{2,4}|\d{1,2}\s+(?:Jan(?:uari|uary)?|Feb(?:ruari|ruary)?|Mac|Mar(?:ch)?|Apr(?:il)?|Mei|May|Jun(?:e)?|Jul(?:ai|y)?|Ogos|Aug(?:ust)?|Sep(?:tember)?|Okt(?:ober)?|Oct(?:ober)?|Nov(?:ember)?|Dis(?:ember)?|Dec(?:ember)?)\s+\d{4}|(?:1[5-9]\d{2}|20\d{2}))\b",
        )
        .expect("date regex")
    });
    let name_re = NAME_RE.get_or_init(|| {
        Regex::new(
            r"\b(?:(?:Tun|Tan\s+Sri|Datuk|Dato'?|Datin|Tunku|Tuanku|Sultan|Raja|Tuan|Puan|Encik|Cik|Haji|Hajah|Dr\.|Prof\.|Sir)\s+)?(?:[A-Z][a-zA-Z'-]{2,}(?:\s+(?:bin|binti|al-|Al-|ibn|of|the|&|[A-Z][a-zA-Z'-]{2,})){1,5})\b",
        )
        .expect("name regex")
    });
    let word_re = WORD_RE.get_or_init(|| Regex::new(r"[A-Za-z]{4,}").expect("word regex"));

    const STOP: &[&str] = &[
        "that",
        "this",
        "with",
        "from",
        "have",
        "were",
        "been",
        "their",
        "which",
        "about",
        "there",
        "would",
        "could",
        "should",
        "into",
        "more",
        "other",
        "also",
        "such",
        "than",
        "when",
        "what",
        "where",
        "they",
        "them",
        "these",
        "those",
        "will",
        "your",
        "only",
        "very",
        "after",
        "before",
        "yang",
        "untuk",
        "dengan",
        "dalam",
        "pada",
        "adalah",
        "ialah",
        "atau",
        "juga",
        "oleh",
        "dari",
        "daripada",
        "kepada",
        "telah",
        "akan",
        "tidak",
        "bukan",
        "bagi",
        "sebagai",
        "tersebut",
        "bahawa",
        "dapat",
        "boleh",
        "antara",
        "serta",
        "secara",
        "lebih",
        "semua",
        "merupakan",
        "seperti",
        "kerana",
        "tetapi",
        "namun",
        "supaya",
        "hingga",
        "sehingga",
        "melalui",
        "terhadap",
        "mengikut",
        "berdasarkan",
        "selain",
        "kini",
        "pihak",
    ];

    let mut dates = Vec::new();
    let mut seen_dates = HashSet::new();
    for m in date_re.find_iter(text) {
        let s = m.as_str().to_string();
        if seen_dates.insert(s.clone()) {
            dates.push(s);
            if dates.len() >= 15 {
                break;
            }
        }
    }

    let mut name_counts: HashMap<String, usize> = HashMap::new();
    for m in name_re.find_iter(text) {
        let s = m.as_str().trim().to_string();
        if s.len() >= 5 {
            *name_counts.entry(s).or_insert(0) += 1;
        }
    }
    let mut names_sorted: Vec<(String, usize)> = name_counts.into_iter().collect();
    names_sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let key_names: Vec<String> = names_sorted.into_iter().take(12).map(|(k, _)| k).collect();

    let mut word_counts: BTreeMap<String, usize> = BTreeMap::new();
    for m in word_re.find_iter(text) {
        let w = m.as_str().to_ascii_lowercase();
        if !STOP.contains(&w.as_str()) {
            *word_counts.entry(w).or_insert(0) += 1;
        }
    }
    let mut topics_sorted: Vec<(String, usize)> = word_counts.into_iter().collect();
    topics_sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
    let key_topics: Vec<String> = topics_sorted.into_iter().take(10).map(|(k, _)| k).collect();

    // Extractive archival summary: top 3 informative sentences ranked by keyword density.
    let sentences: Vec<&str> = text
        .split(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|s| s.split_whitespace().count() >= 5)
        .collect();
    let topic_set: HashSet<&str> = key_topics.iter().map(String::as_str).collect();
    let mut scored: Vec<(usize, usize, &str)> = sentences
        .iter()
        .enumerate()
        .map(|(idx, &sent)| {
            let low = sent.to_ascii_lowercase();
            let score =
                topic_set.iter().filter(|&&t| low.contains(t)).count() + usize::from(idx == 0) * 2;
            (score, idx, sent)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut top: Vec<(usize, &str)> = scored.into_iter().take(3).map(|(_, i, s)| (i, s)).collect();
    top.sort_by_key(|(i, _)| *i);
    let extractive_summary = top
        .into_iter()
        .map(|(_, s)| format!("{s}."))
        .collect::<Vec<_>>()
        .join(" ");

    let detected = detect_script(text);
    let hijri_colophon_dates = extract_hijri_colophon_dates(text);
    let primary_hijri = hijri_colophon_dates
        .first()
        .and_then(|v| v.get("hijri_date"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let primary_masihi = hijri_colophon_dates
        .first()
        .and_then(|v| v.get("gregorian_date_masihi"))
        .and_then(Value::as_str)
        .unwrap_or("");
    json!({
        "detected_script": detected.as_str(),
        "script_name": detected.display_name(),
        "word_count": text.split_whitespace().count(),
        "line_count": text.lines().filter(|l| !l.trim().is_empty()).count(),
        "character_count": text.chars().count(),
        "key_topics": key_topics,
        "key_names": key_names,
        "key_dates": dates,
        "hijri_date": primary_hijri,
        "gregorian_date_masihi": primary_masihi,
        "hijri_colophon_dates": hijri_colophon_dates,
        "extractive_summary": extractive_summary,
    })
}

pub async fn summarize_with_ollama(
    http: &reqwest::Client,
    cfg: &AppConfig,
    text: &str,
    language_hint: &str,
) -> Option<String> {
    let clipped: String = text.chars().take(12_000).collect();
    if clipped.trim().is_empty() {
        return None;
    }
    let lang_rule = if language_hint.eq_ignore_ascii_case("ms")
        || language_hint.eq_ignore_ascii_case("malay")
        || language_hint.eq_ignore_ascii_case("bm")
    {
        "Write strictly in natural Standard Bahasa Malaysia (DBP). Use Malaysian terms (maklumat, boleh, pejabat, kulat, merosakkan, asas perundangan, cabaran, adakah, gambar, pemasyhuran, pemuliharaan) — NEVER Indonesian terms (informasi, bisa, kantor, jamur, merusak, dasar hukum, tantangan, apakah, foto, kesulitan)."
    } else {
        "Reply in the same language as the document (English or Standard Bahasa Malaysia DBP). If replying in Bahasa Malaysia, use Malaysian DBP terms (maklumat, boleh, pejabat, kulat, merosakkan, asas perundangan, adakah, pemasyhuran) and avoid Indonesian vocabulary (informasi, bisa, kantor, jamur, merusak, dasar hukum, apakah, tantangan)."
    };
    let body = json!({
        "model": cfg.vision_model,
        "stream": false,
        "think": false,
        "keep_alive": "30m",
        "options": { "temperature": 0.1, "num_predict": 420, "num_ctx": OLLAMA_UNIFORM_NUM_CTX },
        "messages": [{
            "role": "user",
            "content": format!(
                "Generate a concise archival description and structured summary of this document preserving its exact original context, roles, and meaning (pay strict attention to who acted on behalf of whom, e.g. 'bagi pihak X' means representing X). Include main purpose, key topics, people/organizations, dates, and events. Do NOT invent facts. {lang_rule}\n\nDOCUMENT:\n{clipped}"
            )
        }]
    });
    let resp = http.post(&cfg.ollama_url).json(&body).send().await.ok()?;
    let data = resp.json::<Value>().await.ok()?;
    let out = data
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    (!out.is_empty()).then_some(out)
}

pub fn summarize_file_info(path: &Path) -> Result<Value, String> {
    extract_file_metadata(path)
}

// ─── Maintainable Enrolled People Group Folders (e.g. Staff, External Entity) ─

fn group_slug(name: &str) -> String {
    profile_slug(name).to_ascii_lowercase()
}

fn load_raw_people_groups(path: &Path) -> Vec<Map<String, Value>> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_object().cloned())
        .collect()
}

fn save_raw_people_groups(path: &Path, groups: &[Map<String, Value>]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let val = Value::Array(groups.iter().cloned().map(Value::Object).collect());
    let pretty = serde_json::to_string_pretty(&val).map_err(|e| format!("JSON error: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, pretty).map_err(|e| format!("Write groups failed: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("Rename groups failed: {e}"))
}

pub fn ensure_default_people_groups(cfg: &AppConfig) {
    if cfg.people_groups_file.is_file() {
        return;
    }
    let now = Utc::now().to_rfc3339();
    let defaults = [
        (
            "Staff",
            "Internal staff, archivists, officers, and organization personnel.",
        ),
        (
            "External Entity",
            "External stakeholders, public figures, partners, visitors, and third-party entities.",
        ),
    ];
    let mut list = Vec::new();
    for (name, desc) in defaults {
        let mut m = Map::new();
        m.insert("slug".to_string(), Value::String(group_slug(name)));
        m.insert("name".to_string(), Value::String(name.to_string()));
        m.insert("description".to_string(), Value::String(desc.to_string()));
        m.insert("created_at".to_string(), Value::String(now.clone()));
        m.insert("updated_at".to_string(), Value::String(now.clone()));
        list.push(m);
    }
    let _ = save_raw_people_groups(&cfg.people_groups_file, &list);
}

fn count_members_by_group(profile_dir: &Path) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    if let Ok(entries) = fs::read_dir(profile_dir) {
        for entry in entries.flatten() {
            let meta_path = entry.path().join("metadata.json");
            if let Ok(text) = fs::read_to_string(&meta_path)
                && let Ok(val) = serde_json::from_str::<Value>(&text)
            {
                let gs = val
                    .get("group_slug")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        val.get("group")
                            .and_then(Value::as_str)
                            .map(group_slug)
                            .filter(|s| !s.is_empty())
                    })
                    .unwrap_or_default();
                if !gs.is_empty() {
                    *counts.entry(gs).or_insert(0) += 1;
                }
            }
        }
    }
    counts
}

pub fn list_people_groups(cfg: &AppConfig) -> Vec<Value> {
    ensure_default_people_groups(cfg);
    let counts = count_members_by_group(&cfg.profile_dir);
    let mut groups = load_raw_people_groups(&cfg.people_groups_file);
    for g in &mut groups {
        let slug = g
            .get("slug")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let cnt = counts.get(&slug).copied().unwrap_or(0);
        g.insert("member_count".to_string(), json!(cnt));
    }
    groups.into_iter().map(Value::Object).collect()
}

pub fn resolve_people_group(cfg: &AppConfig, group_or_slug: &str) -> Option<(String, String)> {
    let clean = group_or_slug.trim();
    if clean.is_empty() {
        return None;
    }
    let target_slug = group_slug(clean);
    let groups = load_raw_people_groups(&cfg.people_groups_file);
    for g in groups {
        let s = g.get("slug").and_then(Value::as_str).unwrap_or("");
        let n = g.get("name").and_then(Value::as_str).unwrap_or("");
        if s.eq_ignore_ascii_case(clean)
            || s.eq_ignore_ascii_case(&target_slug)
            || n.eq_ignore_ascii_case(clean)
        {
            return Some((s.to_string(), n.to_string()));
        }
    }
    None
}

pub fn create_people_group(
    cfg: &AppConfig,
    name: &str,
    description: &str,
) -> Result<Value, String> {
    ensure_default_people_groups(cfg);
    let clean_name = name.trim();
    if clean_name.is_empty() {
        return Err("Group folder name is required.".to_string());
    }
    let slug = group_slug(clean_name);
    if slug.is_empty() || slug == "all" || slug == "unassigned" {
        return Err(format!(
            "'{clean_name}' is a reserved or invalid group name."
        ));
    }
    let mut groups = load_raw_people_groups(&cfg.people_groups_file);
    if groups.iter().any(|g| {
        g.get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case(&slug))
    }) {
        return Err(format!("Group folder '{clean_name}' already exists."));
    }
    let now = Utc::now().to_rfc3339();
    let mut m = Map::new();
    m.insert("slug".to_string(), Value::String(slug));
    m.insert("name".to_string(), Value::String(clean_name.to_string()));
    m.insert(
        "description".to_string(),
        Value::String(description.trim().to_string()),
    );
    m.insert("created_at".to_string(), Value::String(now.clone()));
    m.insert("updated_at".to_string(), Value::String(now));
    groups.push(m.clone());
    save_raw_people_groups(&cfg.people_groups_file, &groups)?;
    m.insert("member_count".to_string(), json!(0));
    Ok(Value::Object(m))
}

fn cascade_profile_group_update(
    profile_dir: &Path,
    old_slug: &str,
    old_name: &str,
    new_slug: &str,
    new_name: &str,
) -> usize {
    let mut updated = 0usize;
    let Ok(entries) = fs::read_dir(profile_dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let meta_path = entry.path().join("metadata.json");
        let Ok(text) = fs::read_to_string(&meta_path) else {
            continue;
        };
        let Ok(mut val) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(obj) = val.as_object_mut() else {
            continue;
        };
        let cur_slug = obj
            .get("group_slug")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let cur_name = obj
            .get("group")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if cur_slug == old_slug.to_ascii_lowercase() || cur_name.eq_ignore_ascii_case(old_name) {
            obj.insert(
                "group_slug".to_string(),
                Value::String(new_slug.to_string()),
            );
            obj.insert("group".to_string(), Value::String(new_name.to_string()));
            if let Ok(pretty) = serde_json::to_string_pretty(&Value::Object(obj.clone())) {
                let _ = fs::write(&meta_path, pretty);
                updated += 1;
            }
        }
    }
    updated
}

pub fn rename_people_group(
    cfg: &AppConfig,
    old_slug: &str,
    new_name: &str,
    description: Option<&str>,
) -> Result<Value, String> {
    ensure_default_people_groups(cfg);
    let clean_name = new_name.trim();
    if clean_name.is_empty() {
        return Err("New group folder name is required.".to_string());
    }
    let new_slug = group_slug(clean_name);
    if new_slug.is_empty() || new_slug == "all" || new_slug == "unassigned" {
        return Err(format!(
            "'{clean_name}' is a reserved or invalid group name."
        ));
    }

    let mut groups = load_raw_people_groups(&cfg.people_groups_file);
    let Some(pos) = groups.iter().position(|g| {
        g.get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case(old_slug))
    }) else {
        return Err("Group folder not found.".to_string());
    };
    if groups.iter().enumerate().any(|(i, g)| {
        i != pos
            && g.get("slug")
                .and_then(Value::as_str)
                .is_some_and(|s| s.eq_ignore_ascii_case(&new_slug))
    }) {
        return Err(format!(
            "Another group folder named '{clean_name}' already exists."
        ));
    }

    let old_name = groups[pos]
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let old_slug_actual = groups[pos]
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or(old_slug)
        .to_string();

    groups[pos].insert("slug".to_string(), Value::String(new_slug.clone()));
    groups[pos].insert("name".to_string(), Value::String(clean_name.to_string()));
    if let Some(desc) = description {
        groups[pos].insert(
            "description".to_string(),
            Value::String(desc.trim().to_string()),
        );
    }
    groups[pos].insert(
        "updated_at".to_string(),
        Value::String(Utc::now().to_rfc3339()),
    );
    save_raw_people_groups(&cfg.people_groups_file, &groups)?;

    let migrated = cascade_profile_group_update(
        &cfg.profile_dir,
        &old_slug_actual,
        &old_name,
        &new_slug,
        clean_name,
    );
    let mut out = groups[pos].clone();
    out.insert("migrated_profiles".to_string(), json!(migrated));
    Ok(Value::Object(out))
}

pub fn delete_people_group(cfg: &AppConfig, slug: &str) -> Result<Value, String> {
    ensure_default_people_groups(cfg);
    let mut groups = load_raw_people_groups(&cfg.people_groups_file);
    let Some(pos) = groups.iter().position(|g| {
        g.get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case(slug))
    }) else {
        return Err("Group folder not found.".to_string());
    };
    let removed = groups.remove(pos);
    let old_name = removed
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let old_slug = removed
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or(slug)
        .to_string();
    save_raw_people_groups(&cfg.people_groups_file, &groups)?;
    let unassigned = cascade_profile_group_update(&cfg.profile_dir, &old_slug, &old_name, "", "");
    Ok(json!({
        "status": "deleted",
        "slug": old_slug,
        "name": old_name,
        "unassigned_profiles": unassigned,
    }))
}

// ─── Maintainable Document Group Folders (e.g. Historical Manuscripts, Official Records) ─

pub fn ensure_default_document_groups(cfg: &AppConfig) {
    if cfg.document_groups_file.is_file() {
        return;
    }
    let now = Utc::now().to_rfc3339();
    let defaults = [
        (
            "Historical Manuscripts",
            "Classical Malay, Jawi, Pallava, Kawi, and Rencong heritage manuscripts.",
        ),
        (
            "Official Records",
            "Treaties, government letters, legal instruments, and archival files.",
        ),
    ];
    let mut list = Vec::new();
    for (name, desc) in defaults {
        let mut m = Map::new();
        m.insert("slug".to_string(), Value::String(group_slug(name)));
        m.insert("name".to_string(), Value::String(name.to_string()));
        m.insert("description".to_string(), Value::String(desc.to_string()));
        m.insert("created_at".to_string(), Value::String(now.clone()));
        m.insert("updated_at".to_string(), Value::String(now.clone()));
        list.push(m);
    }
    let _ = save_raw_people_groups(&cfg.document_groups_file, &list);
}

pub fn list_document_groups(cfg: &AppConfig) -> Vec<Value> {
    ensure_default_document_groups(cfg);
    let counts = count_members_by_group(&cfg.documents_dir);
    let mut groups = load_raw_people_groups(&cfg.document_groups_file);
    for g in &mut groups {
        let slug = g
            .get("slug")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let cnt = counts.get(&slug).copied().unwrap_or(0);
        g.insert("member_count".to_string(), json!(cnt));
    }
    groups.into_iter().map(Value::Object).collect()
}

pub fn resolve_document_group(cfg: &AppConfig, group_or_slug: &str) -> Option<(String, String)> {
    let clean = group_or_slug.trim();
    if clean.is_empty() {
        return None;
    }
    let target_slug = group_slug(clean);
    let groups = load_raw_people_groups(&cfg.document_groups_file);
    for g in groups {
        let s = g.get("slug").and_then(Value::as_str).unwrap_or("");
        let n = g.get("name").and_then(Value::as_str).unwrap_or("");
        if s.eq_ignore_ascii_case(clean)
            || s.eq_ignore_ascii_case(&target_slug)
            || n.eq_ignore_ascii_case(clean)
        {
            return Some((s.to_string(), n.to_string()));
        }
    }
    None
}

pub fn create_document_group(
    cfg: &AppConfig,
    name: &str,
    description: &str,
) -> Result<Value, String> {
    ensure_default_document_groups(cfg);
    let clean_name = name.trim();
    if clean_name.is_empty() {
        return Err("Document group folder name is required.".to_string());
    }
    let slug = group_slug(clean_name);
    if slug.is_empty() || slug == "all" || slug == "unassigned" {
        return Err(format!(
            "'{clean_name}' is a reserved or invalid folder name."
        ));
    }
    let mut groups = load_raw_people_groups(&cfg.document_groups_file);
    if groups.iter().any(|g| {
        g.get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case(&slug))
    }) {
        return Err(format!(
            "Document group folder '{clean_name}' already exists."
        ));
    }
    let now = Utc::now().to_rfc3339();
    let mut m = Map::new();
    m.insert("slug".to_string(), Value::String(slug));
    m.insert("name".to_string(), Value::String(clean_name.to_string()));
    m.insert(
        "description".to_string(),
        Value::String(description.trim().to_string()),
    );
    m.insert("created_at".to_string(), Value::String(now.clone()));
    m.insert("updated_at".to_string(), Value::String(now));
    groups.push(m.clone());
    save_raw_people_groups(&cfg.document_groups_file, &groups)?;
    m.insert("member_count".to_string(), json!(0));
    Ok(Value::Object(m))
}

pub fn rename_document_group(
    cfg: &AppConfig,
    old_slug: &str,
    new_name: &str,
    description: Option<&str>,
) -> Result<Value, String> {
    ensure_default_document_groups(cfg);
    let clean_name = new_name.trim();
    if clean_name.is_empty() {
        return Err("New document group folder name is required.".to_string());
    }
    let new_slug = group_slug(clean_name);
    if new_slug.is_empty() || new_slug == "all" || new_slug == "unassigned" {
        return Err(format!(
            "'{clean_name}' is a reserved or invalid folder name."
        ));
    }

    let mut groups = load_raw_people_groups(&cfg.document_groups_file);
    let Some(pos) = groups.iter().position(|g| {
        g.get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case(old_slug))
    }) else {
        return Err("Document group folder not found.".to_string());
    };
    if groups.iter().enumerate().any(|(i, g)| {
        i != pos
            && g.get("slug")
                .and_then(Value::as_str)
                .is_some_and(|s| s.eq_ignore_ascii_case(&new_slug))
    }) {
        return Err(format!(
            "Another document group folder named '{clean_name}' already exists."
        ));
    }

    let old_name = groups[pos]
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let old_slug_actual = groups[pos]
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or(old_slug)
        .to_string();

    groups[pos].insert("slug".to_string(), Value::String(new_slug.clone()));
    groups[pos].insert("name".to_string(), Value::String(clean_name.to_string()));
    if let Some(desc) = description {
        groups[pos].insert(
            "description".to_string(),
            Value::String(desc.trim().to_string()),
        );
    }
    groups[pos].insert(
        "updated_at".to_string(),
        Value::String(Utc::now().to_rfc3339()),
    );
    save_raw_people_groups(&cfg.document_groups_file, &groups)?;

    let migrated = cascade_profile_group_update(
        &cfg.documents_dir,
        &old_slug_actual,
        &old_name,
        &new_slug,
        clean_name,
    );
    let mut out = groups[pos].clone();
    out.insert("migrated_documents".to_string(), json!(migrated));
    Ok(Value::Object(out))
}

pub fn delete_document_group(cfg: &AppConfig, slug: &str) -> Result<Value, String> {
    ensure_default_document_groups(cfg);
    let mut groups = load_raw_people_groups(&cfg.document_groups_file);
    let Some(pos) = groups.iter().position(|g| {
        g.get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case(slug))
    }) else {
        return Err("Document group folder not found.".to_string());
    };
    let removed = groups.remove(pos);
    let old_name = removed
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let old_slug = removed
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or(slug)
        .to_string();
    save_raw_people_groups(&cfg.document_groups_file, &groups)?;
    let unassigned = cascade_profile_group_update(&cfg.documents_dir, &old_slug, &old_name, "", "");
    Ok(json!({
        "status": "deleted",
        "slug": old_slug,
        "name": old_name,
        "unassigned_documents": unassigned,
    }))
}

// ─── Maintainable Location Group Folders (e.g. Arkib Negara Malaysia, Historical Landmarks) ─

pub fn ensure_default_location_groups(cfg: &AppConfig) {
    if cfg.location_groups_file.is_file() {
        return;
    }
    let now = Utc::now().to_rfc3339();
    let defaults = [
        (
            "Arkib Negara Malaysia",
            "Halls, wings, exhibition galleries, and rooms at the National Archives of Malaysia.",
        ),
        (
            "Historical Landmarks",
            "Heritage buildings, monuments, memorials, and historical sites.",
        ),
    ];
    let mut list = Vec::new();
    for (name, desc) in defaults {
        let mut m = Map::new();
        m.insert("slug".to_string(), Value::String(group_slug(name)));
        m.insert("name".to_string(), Value::String(name.to_string()));
        m.insert("description".to_string(), Value::String(desc.to_string()));
        m.insert("created_at".to_string(), Value::String(now.clone()));
        m.insert("updated_at".to_string(), Value::String(now.clone()));
        list.push(m);
    }
    let _ = save_raw_people_groups(&cfg.location_groups_file, &list);
}

pub fn list_location_groups(cfg: &AppConfig) -> Vec<Value> {
    ensure_default_location_groups(cfg);
    let counts = count_members_by_group(&cfg.locations_dir);
    let mut groups = load_raw_people_groups(&cfg.location_groups_file);
    for g in &mut groups {
        let slug = g
            .get("slug")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let cnt = counts.get(&slug).copied().unwrap_or(0);
        g.insert("member_count".to_string(), json!(cnt));
    }
    groups.into_iter().map(Value::Object).collect()
}

pub fn resolve_location_group(cfg: &AppConfig, group_or_slug: &str) -> Option<(String, String)> {
    let clean = group_or_slug.trim();
    if clean.is_empty() {
        return None;
    }
    let target_slug = group_slug(clean);
    let groups = load_raw_people_groups(&cfg.location_groups_file);
    for g in groups {
        let s = g.get("slug").and_then(Value::as_str).unwrap_or("");
        let n = g.get("name").and_then(Value::as_str).unwrap_or("");
        if s.eq_ignore_ascii_case(clean)
            || s.eq_ignore_ascii_case(&target_slug)
            || n.eq_ignore_ascii_case(clean)
        {
            return Some((s.to_string(), n.to_string()));
        }
    }
    None
}

pub fn create_location_group(
    cfg: &AppConfig,
    name: &str,
    description: &str,
) -> Result<Value, String> {
    ensure_default_location_groups(cfg);
    let clean_name = name.trim();
    if clean_name.is_empty() {
        return Err("Location group folder name is required.".to_string());
    }
    let slug = group_slug(clean_name);
    if slug.is_empty() || slug == "all" || slug == "unassigned" {
        return Err(format!(
            "'{clean_name}' is a reserved or invalid folder name."
        ));
    }
    let mut groups = load_raw_people_groups(&cfg.location_groups_file);
    if groups.iter().any(|g| {
        g.get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case(&slug))
    }) {
        return Err(format!(
            "Location group folder '{clean_name}' already exists."
        ));
    }
    let now = Utc::now().to_rfc3339();
    let mut m = Map::new();
    m.insert("slug".to_string(), Value::String(slug));
    m.insert("name".to_string(), Value::String(clean_name.to_string()));
    m.insert(
        "description".to_string(),
        Value::String(description.trim().to_string()),
    );
    m.insert("created_at".to_string(), Value::String(now.clone()));
    m.insert("updated_at".to_string(), Value::String(now));
    groups.push(m.clone());
    save_raw_people_groups(&cfg.location_groups_file, &groups)?;
    m.insert("member_count".to_string(), json!(0));
    Ok(Value::Object(m))
}

pub fn rename_location_group(
    cfg: &AppConfig,
    old_slug: &str,
    new_name: &str,
    description: Option<&str>,
) -> Result<Value, String> {
    ensure_default_location_groups(cfg);
    let clean_name = new_name.trim();
    if clean_name.is_empty() {
        return Err("New location group folder name is required.".to_string());
    }
    let new_slug = group_slug(clean_name);
    if new_slug.is_empty() || new_slug == "all" || new_slug == "unassigned" {
        return Err(format!(
            "'{clean_name}' is a reserved or invalid folder name."
        ));
    }

    let mut groups = load_raw_people_groups(&cfg.location_groups_file);
    let Some(pos) = groups.iter().position(|g| {
        g.get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case(old_slug))
    }) else {
        return Err("Location group folder not found.".to_string());
    };
    if groups.iter().enumerate().any(|(i, g)| {
        i != pos
            && g.get("slug")
                .and_then(Value::as_str)
                .is_some_and(|s| s.eq_ignore_ascii_case(&new_slug))
    }) {
        return Err(format!(
            "Another location group folder named '{clean_name}' already exists."
        ));
    }

    let old_name = groups[pos]
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let old_slug_actual = groups[pos]
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or(old_slug)
        .to_string();

    groups[pos].insert("slug".to_string(), Value::String(new_slug.clone()));
    groups[pos].insert("name".to_string(), Value::String(clean_name.to_string()));
    if let Some(desc) = description {
        groups[pos].insert(
            "description".to_string(),
            Value::String(desc.trim().to_string()),
        );
    }
    groups[pos].insert(
        "updated_at".to_string(),
        Value::String(Utc::now().to_rfc3339()),
    );
    save_raw_people_groups(&cfg.location_groups_file, &groups)?;

    let migrated = cascade_profile_group_update(
        &cfg.locations_dir,
        &old_slug_actual,
        &old_name,
        &new_slug,
        clean_name,
    );
    let mut out = groups[pos].clone();
    out.insert("migrated_locations".to_string(), json!(migrated));
    Ok(Value::Object(out))
}

pub fn delete_location_group(cfg: &AppConfig, slug: &str) -> Result<Value, String> {
    ensure_default_location_groups(cfg);
    let mut groups = load_raw_people_groups(&cfg.location_groups_file);
    let Some(pos) = groups.iter().position(|g| {
        g.get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case(slug))
    }) else {
        return Err("Location group folder not found.".to_string());
    };
    let removed = groups.remove(pos);
    let old_name = removed
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let old_slug = removed
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or(slug)
        .to_string();
    save_raw_people_groups(&cfg.location_groups_file, &groups)?;
    let unassigned = cascade_profile_group_update(&cfg.locations_dir, &old_slug, &old_name, "", "");
    Ok(json!({
        "status": "deleted",
        "slug": old_slug,
        "name": old_name,
        "unassigned_locations": unassigned,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_structure_and_xml_strip() {
        let sample = "On 31 August 1957, Tunku Abdul Rahman proclaimed the independence of the Federation of Malaya at Stadium Merdeka in Kuala Lumpur. The proclamation is preserved at the National Archives of Malaysia (Arkib Negara Malaysia) as a foundational historical record.";
        let st = extract_document_structure(sample);
        assert!(
            st["key_dates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "31 August 1957")
        );
        assert!(
            st["key_names"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str().unwrap_or("").contains("Tunku Abdul Rahman"))
        );
        assert!(!st["extractive_summary"].as_str().unwrap_or("").is_empty());

        let xml = "<w:p><w:r><w:t>Arkib Negara</w:t></w:r></w:p><w:p><w:r><w:t>Malaysia &amp; Warisan</w:t></w:r></w:p>";
        assert_eq!(strip_xml_tags(xml), "Arkib Negara\nMalaysia & Warisan");
    }

    #[test]
    fn test_people_group_create_rename_and_delete() {
        let base =
            std::env::temp_dir().join(format!("arkib_groups_test_{}", Uuid::new_v4().simple()));
        let pdir = base.join("profiles");
        let p1 = pdir.join("Ali_Bin_Abu");
        fs::create_dir_all(&p1).unwrap();
        let gfile = base.join("people_groups.json");

        let mut cfg = AppConfig::load();
        cfg.profile_dir = pdir.clone();
        cfg.people_groups_file = gfile;

        ensure_default_people_groups(&cfg);
        let initial = list_people_groups(&cfg);
        assert_eq!(initial.len(), 2);

        let created = create_people_group(&cfg, "VIP Guests", "Visiting dignitaries").unwrap();
        assert_eq!(created["slug"], "vip_guests");

        fs::write(
            p1.join("metadata.json"),
            r#"{"identity":"Ali Bin Abu","group":"VIP Guests","group_slug":"vip_guests"}"#,
        )
        .unwrap();

        let renamed = rename_people_group(&cfg, "vip_guests", "Royal Guests", None).unwrap();
        assert_eq!(renamed["slug"], "royal_guests");
        assert_eq!(renamed["migrated_profiles"], 1);

        let after_rename: Value =
            serde_json::from_str(&fs::read_to_string(p1.join("metadata.json")).unwrap()).unwrap();
        assert_eq!(after_rename["group"], "Royal Guests");
        assert_eq!(after_rename["group_slug"], "royal_guests");

        let deleted = delete_people_group(&cfg, "royal_guests").unwrap();
        assert_eq!(deleted["unassigned_profiles"], 1);

        let after_del: Value =
            serde_json::from_str(&fs::read_to_string(p1.join("metadata.json")).unwrap()).unwrap();
        assert_eq!(after_del["group"], "");

        // Also test Document Group Folders CRUD & cascade
        let ddir = base.join("documents");
        let d1 = ddir.join("hikayat_hang_tuah");
        fs::create_dir_all(&d1).unwrap();
        cfg.documents_dir = ddir;
        cfg.document_groups_file = base.join("document_groups.json");

        ensure_default_document_groups(&cfg);
        assert_eq!(list_document_groups(&cfg).len(), 2);
        let dg = create_document_group(&cfg, "Royal Treaties", "Treaty records").unwrap();
        assert_eq!(dg["slug"], "royal_treaties");
        fs::write(
            d1.join("metadata.json"),
            r#"{"title":"Hikayat","group":"Royal Treaties","group_slug":"royal_treaties"}"#,
        )
        .unwrap();
        let d_ren =
            rename_document_group(&cfg, "royal_treaties", "National Treaties", None).unwrap();
        assert_eq!(d_ren["migrated_documents"], 1);
        let d_del = delete_document_group(&cfg, "national_treaties").unwrap();
        assert_eq!(d_del["unassigned_documents"], 1);

        // Also test Location Group Folders CRUD & cascade
        let ldir = base.join("locations");
        let l1 = ldir.join("dewan_penyelidikan");
        fs::create_dir_all(&l1).unwrap();
        cfg.locations_dir = ldir;
        cfg.location_groups_file = base.join("location_groups.json");

        ensure_default_location_groups(&cfg);
        assert_eq!(list_location_groups(&cfg).len(), 2);
        let lg =
            create_location_group(&cfg, "State Branches", "Regional archive branches").unwrap();
        assert_eq!(lg["slug"], "state_branches");
        fs::write(
            l1.join("metadata.json"),
            r#"{"name":"Dewan Penyelidikan","group":"State Branches","group_slug":"state_branches"}"#,
        )
        .unwrap();
        let l_ren =
            rename_location_group(&cfg, "state_branches", "Regional Branches", None).unwrap();
        assert_eq!(l_ren["migrated_locations"], 1);
        let l_del = delete_location_group(&cfg, "regional_branches").unwrap();
        assert_eq!(l_del["unassigned_locations"], 1);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_lopdf_extraction_and_hijri_volume_metadata() {
        use lopdf::content::{Content, Operation};
        use lopdf::{Document, Object, Stream, dictionary};

        let tmp = std::env::temp_dir().join(format!("arkib_pdf_test_{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&tmp).unwrap();
        let pdf_path = tmp.join("sample.pdf");

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![50.into(), 700.into()]),
                Operation::new(
                    "Tj",
                    vec![Object::string_literal(
                        "Arkib Negara Malaysia 13 Muharam 1324H",
                    )],
                ),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.save(&pdf_path).unwrap();

        let text = extract_pdf_text(&pdf_path).unwrap();
        assert!(text.contains("Arkib Negara Malaysia"));
        let structured = extract_document_structure(&text);
        assert_eq!(structured["hijri_date"], "13 Muharam 1324H");
        assert_eq!(structured["gregorian_date_masihi"], "1906-03-09");

        // Graceful fallback on empty / scanned PDF returns empty string (no crash)
        let scanned_path = tmp.join("scanned.pdf");
        fs::write(&scanned_path, b"%PDF-1.4\n%%EOF\n").unwrap();
        assert_eq!(extract_pdf_text(&scanned_path).unwrap(), "");

        let _ = fs::remove_dir_all(&tmp);
    }
}
