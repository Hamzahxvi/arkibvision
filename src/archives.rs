#![forbid(unsafe_code)]

//! Managed Archival Folders (Locations & Documents) + Multi-Format Document Extraction
//! & Automatic Archival Summarization (100% Pure Safe Rust).

use crate::ancient_scripts::detect_script;
use crate::biometrics::FaceEngine;
use crate::config::{AppConfig, is_document_ext, is_image_ext};
use crate::conservation::analyze_image_quality_and_conservation;
use crate::forensics::{
    analyze_authenticity, extract_file_metadata, find_duplicate_in_archives,
    register_checksum_entry, sha256_bytes,
};
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

/// Pure-Rust PDF text extractor supporting uncompressed and FlateDecode text streams.
fn extract_pdf_text(path: &Path) -> Result<String, String> {
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
            r"\b(?:[A-Z][a-z]{2,}(?:\s+(?:bin|binti|al-|ibn|of|the|&|[A-Z][a-z]{2,})){1,4})\b",
        )
        .expect("name regex")
    });
    let word_re = WORD_RE.get_or_init(|| Regex::new(r"[A-Za-z]{4,}").expect("word regex"));

    const STOP: &[&str] = &[
        "that", "this", "with", "from", "have", "were", "been", "their", "which", "about", "there",
        "would", "could", "should", "into", "more", "other", "also", "such", "than", "when",
        "what", "where", "they", "them", "these", "those", "will", "your", "only", "very", "after",
        "before", "yang", "untuk", "dengan", "dalam", "pada", "adalah", "ialah", "atau", "juga",
        "oleh", "dari", "daripada", "kepada", "telah", "akan", "tidak", "bukan", "bagi", "sebagai",
        "tersebut", "bahawa", "dapat", "boleh", "antara", "serta", "secara", "lebih", "semua",
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
    json!({
        "detected_script": detected.as_str(),
        "script_name": detected.display_name(),
        "word_count": text.split_whitespace().count(),
        "line_count": text.lines().filter(|l| !l.trim().is_empty()).count(),
        "character_count": text.chars().count(),
        "key_topics": key_topics,
        "key_names": key_names,
        "key_dates": dates,
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
    {
        "Write in natural Standard Bahasa Melayu (Malaysia)."
    } else {
        "Reply in the same language as the document (English or Standard Bahasa Melayu Malaysia)."
    };
    let body = json!({
        "model": cfg.vision_model,
        "stream": false,
        "think": false,
        "keep_alive": "30m",
        "options": { "temperature": 0.1, "num_predict": 420 },
        "messages": [{
            "role": "user",
            "content": format!(
                "Generate a concise archival description and structured summary of this document preserving its exact original context and meaning. Include main purpose, key topics, people/organizations, dates, and events. Do NOT invent facts. {lang_rule}\n\nDOCUMENT:\n{clipped}"
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
}
