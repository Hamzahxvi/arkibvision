#![forbid(unsafe_code)]

use crate::ancient_scripts::decode_ancient_script;
use crate::archives::{
    ensure_default_document_groups, ensure_default_location_groups, ensure_default_locations,
    ensure_default_people_groups, extract_document_structure, extract_text_from_file,
    list_documents, list_locations, list_people_groups, load_location, search_documents,
    search_locations, summarize_file_info, summarize_with_ollama,
};
use crate::biometrics::FaceEngine;
use crate::config::{
    AUTO_GALLERY_MAX_IMAGES, AUTO_GALLERY_MIN_SCORE, AppConfig, MAX_AUTO_IMAGE_AGE_SECS,
    MIN_CLIP_IDENTIFY_THRESHOLD, PERCEPTUAL_HASH_DISTANCE, is_image_ext,
};
use crate::conservation::{analyze_image_quality_and_conservation, enhance_archival_image};
use crate::forensics::{
    analyze_authenticity, extract_file_metadata, fuse_visual_audit, register_checksum_entry,
    sha256_bytes, verify_archive_integrity,
};
use crate::jawi::decode_jawi_text;
use crate::metadata::{
    format_identity_name, format_time_12h, merge_profile_metadata, normalize_alnum_lower,
    normalize_other_details, parse_proposed_metadata, profile_slug, public_other_details,
};
use crate::monitor::{ArchiveCounts, SystemMonitor};
use crate::vector_store::{FacePoint, VectorStore};
use crate::vision_ops::{
    build_ocr_document_chunks, encode_image_b64, hash_distance, perceptual_hash,
};
use chrono::Utc;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Clone, Debug)]
struct CacheEntry {
    hash: String,
    threshold: f32,
    matches_count: usize,
    matches: Vec<Value>,
    cached_from: String,
}

pub struct RojakFaceService {
    pub cfg: AppConfig,
    pub engine: Arc<FaceEngine>,
    pub store: Arc<VectorStore>,
    pub location_store: Arc<VectorStore>,
    pub monitor: Arc<SystemMonitor>,
    pub http: reqwest::Client,
    id_cache: Mutex<Vec<CacheEntry>>,
    pub pending_lock: Mutex<()>,
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn file_latest_unix_secs(path: &Path) -> u64 {
    let Ok(meta) = fs::metadata(path) else {
        return 0;
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let atime = meta
        .accessed()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    mtime.max(atime)
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn find_docker_binary() -> Option<PathBuf> {
    if let Ok(path_env) = std::env::var("PATH") {
        let name = if cfg!(windows) {
            "docker.exe"
        } else {
            "docker"
        };
        for dir in std::env::split_paths(&path_env) {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    let candidates = [
        PathBuf::from(r"C:\Program Files\Docker\Docker\resources\bin\docker.exe"),
        PathBuf::from(r"C:\Program Files\Docker\Docker\resources\docker.exe"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

impl RojakFaceService {
    pub fn new(cfg: AppConfig) -> Result<Self, String> {
        ensure_default_location_groups(&cfg);
        ensure_default_locations(&cfg);
        ensure_default_people_groups(&cfg);
        ensure_default_document_groups(&cfg);
        let engine = Arc::new(FaceEngine::load(&cfg)?);
        let store = Arc::new(VectorStore::load_or_create(&cfg.vectors_file)?);
        let location_store = Arc::new(VectorStore::load_or_create(&cfg.location_vectors_file)?);
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .map_err(|e| format!("HTTP client build error: {e}"))?;
        let monitor = Arc::new(SystemMonitor::new());
        Ok(Self {
            cfg,
            engine,
            store,
            location_store,
            monitor,
            http,
            id_cache: Mutex::new(Vec::new()),
            pending_lock: Mutex::new(()),
        })
    }

    pub fn verify_admin(&self, provided_key: &str) -> bool {
        let k = provided_key.trim();
        !k.is_empty() && constant_time_eq(k, &self.cfg.admin_key)
    }

    pub fn clear_identification_cache(&self) {
        self.id_cache.lock().expect("cache lock").clear();
    }

    fn cached_identification(&self, image_path: &Path, threshold: f32) -> Option<Value> {
        let img_hash = perceptual_hash(image_path).ok()?;
        let guard = self.id_cache.lock().expect("cache lock");
        for entry in guard.iter() {
            if (entry.threshold - threshold).abs() < 1e-6
                && hash_distance(&img_hash, &entry.hash) <= PERCEPTUAL_HASH_DISTANCE
            {
                return Some(json!({
                    "matches_count": entry.matches_count,
                    "matches": entry.matches,
                    "cached_from": entry.cached_from,
                    "file": image_path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
                    "deduplicated": true,
                }));
            }
        }
        None
    }

    fn cache_identification(&self, image_path: &Path, threshold: f32, matches: &[Value]) {
        let Ok(img_hash) = perceptual_hash(image_path) else {
            return;
        };
        let mut guard = self.id_cache.lock().expect("cache lock");
        guard.push(CacheEntry {
            hash: img_hash,
            threshold,
            matches_count: matches.len(),
            matches: matches.to_vec(),
            cached_from: image_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string(),
        });
        if guard.len() > 128 {
            let excess = guard.len() - 128;
            guard.drain(0..excess);
        }
    }

    fn latest_uploaded_image(&self) -> (Option<PathBuf>, bool) {
        if let Some(docker_bin) = find_docker_binary() {
            let mongo_container = std::env::var("LIBRECHAT_MONGO_CONTAINER")
                .unwrap_or_else(|_| "chat-mongodb".to_string());
            let mongo_db =
                std::env::var("LIBRECHAT_MONGO_DB").unwrap_or_else(|_| "LibreChat".to_string());
            let query = r#"db.messages.find({"files.0.type":/image/},{files:1,createdAt:1}).sort({createdAt:-1}).limit(1).forEach(x=>print(JSON.stringify(x)))"#;
            if let Ok(output) = Command::new(docker_bin)
                .args([
                    "exec",
                    &mongo_container,
                    "mongosh",
                    &mongo_db,
                    "--quiet",
                    "--eval",
                    query,
                ])
                .output()
                && output.status.success()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines().rev() {
                    if let Ok(record) = serde_json::from_str::<Value>(line)
                        && let Some(stored) = record
                            .get("files")
                            .and_then(Value::as_array)
                            .and_then(|a| a.first())
                            .and_then(|f| f.get("filepath"))
                            .and_then(Value::as_str)
                    {
                        let cand = if let Some(rest) = stored.strip_prefix("/images/") {
                            self.cfg.librechat_dir.join("images").join(rest)
                        } else if let Some(rest) = stored.strip_prefix("/uploads/") {
                            self.cfg.librechat_dir.join("uploads").join(rest)
                        } else {
                            PathBuf::from(stored)
                        };
                        if is_image_ext(&cand) && cand.is_file() {
                            return (Some(cand), true);
                        }
                    }
                }
            }
        }

        let mut best: Option<(u64, PathBuf)> = None;
        for folder in &self.cfg.image_dirs {
            if !folder.is_dir() {
                continue;
            }
            for entry in WalkDir::new(folder)
                .into_iter()
                .filter_entry(|e| {
                    if e.file_type().is_dir() {
                        let name = e.file_name().to_str().unwrap_or("");
                        !matches!(
                            name,
                            "profile-gallery"
                                | "ocr-chunks"
                                | "identity-gallery"
                                | "location-gallery"
                                | "document-gallery"
                                | "enhanced"
                        )
                    } else {
                        true
                    }
                })
                .flatten()
            {
                let p = entry.path();
                if entry.file_type().is_file() && is_image_ext(p) {
                    let ts = file_latest_unix_secs(p);
                    if best.as_ref().is_none_or(|(b_ts, _)| ts > *b_ts) {
                        best = Some((ts, p.to_path_buf()));
                    }
                }
            }
        }
        (best.map(|(_, p)| p), false)
    }

    pub fn resolve_image_path(&self, image_path: &str) -> Result<PathBuf, String> {
        let cleaned = image_path.trim().trim_matches('"').trim_matches('\'');
        let lower = cleaned.to_ascii_lowercase();
        let is_placeholder = cleaned.is_empty()
            || matches!(
                lower.as_str(),
                "latest"
                    | "uploaded"
                    | "image"
                    | "chat_image"
                    | "attached"
                    | "attachment"
                    | "current"
                    | "file"
                    | "photo"
                    | "picture"
                    | "image.jpg"
                    | "image.jpeg"
                    | "image.png"
                    | "photo.jpg"
                    | "photo.png"
                    | "upload.jpg"
                    | "upload.png"
                    | "uploaded_image.jpg"
                    | "uploaded_image.png"
                    | "attached_image.jpg"
                    | "attached_image.png"
                    | "input.jpg"
                    | "input.png"
                    | "gambar"
                    | "gambar.jpg"
                    | "gambar.jpeg"
                    | "gambar.png"
                    | "foto"
                    | "foto.jpg"
                    | "foto.png"
                    | "fail"
                    | "dokumen"
                    | "lampiran"
            );

        if !is_placeholder {
            let cand = PathBuf::from(cleaned);
            if cand.is_file() {
                return Ok(cand);
            }
            let local = self.cfg.project_root.join(cleaned);
            if local.is_file() {
                return Ok(local);
            }
            if let Some(fname) = cand.file_name().and_then(|s| s.to_str()) {
                for folder in &self.cfg.image_dirs {
                    if !folder.is_dir() {
                        continue;
                    }
                    for entry in WalkDir::new(folder).into_iter().flatten() {
                        if entry.file_type().is_file() && entry.file_name().to_str() == Some(fname)
                        {
                            return Ok(entry.into_path());
                        }
                    }
                }
            }
            // If the model invented a filename (e.g. "document.jpg" or "/tmp/photo.png")
            // while the user actually uploaded an image in chat, gracefully fall back to the
            // latest chat attachment instead of failing with a confusing "file not found" error!
        }

        let (latest, from_chat) = self.latest_uploaded_image();
        if let Some(path) = latest {
            let age = now_unix_secs().saturating_sub(file_latest_unix_secs(&path));
            if from_chat || age <= MAX_AUTO_IMAGE_AGE_SECS {
                return Ok(path);
            }
            if !is_placeholder {
                return Err(format!(
                    "Could not find '{image_path}' and no newly uploaded image was detected. Please attach the image directly in your message."
                ));
            }
            return Err(
                "No newly uploaded image was detected in this message. Please attach or re-upload your image and try again.".to_string(),
            );
        }

        if !is_placeholder {
            return Err(format!(
                "Could not find '{image_path}'. Please attach the image or file directly in the chat."
            ));
        }

        Err(
            "No image attachment was found in this message. Please attach an image and try again."
                .to_string(),
        )
    }

    pub fn canonical_identity(&self, identity: &str) -> String {
        let clean = format_identity_name(identity);
        if clean.is_empty() || !self.cfg.profile_dir.is_dir() {
            return clean;
        }
        let norm_target = normalize_alnum_lower(&clean);
        let mut matches = Vec::new();
        let mut fuzzy = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.cfg.profile_dir) {
            for entry in entries.flatten() {
                let meta_path = entry.path().join("metadata.json");
                if let Ok(text) = fs::read_to_string(&meta_path)
                    && let Ok(val) = serde_json::from_str::<Value>(&text)
                    && let Some(stored) = val.get("identity").and_then(Value::as_str)
                {
                    let stored = stored.trim();
                    if stored.is_empty() {
                        continue;
                    }
                    if stored.eq_ignore_ascii_case(&clean) {
                        matches.push(stored.to_string());
                    } else if !norm_target.is_empty()
                        && normalize_alnum_lower(stored) == norm_target
                    {
                        fuzzy.push(stored.to_string());
                    }
                }
            }
        }
        let pool = if !matches.is_empty() { matches } else { fuzzy };
        if pool.is_empty() {
            return clean;
        }
        if let Some(exact) = pool.iter().find(|n| *n == &clean) {
            return exact.clone();
        }
        pool.iter()
            .find(|n| n.chars().next().is_some_and(char::is_uppercase))
            .cloned()
            .unwrap_or_else(|| pool[0].clone())
    }

    pub fn sync_profile_folder(&self, identity: &str, source_image: Option<&Path>) {
        let slug = profile_slug(identity);
        if slug.is_empty() {
            return;
        }
        let pdir = self.cfg.profile_dir.join(&slug);
        let images_dir = pdir.join("images");
        let web_dir = self.cfg.profile_web_dir.join(&slug);
        let _ = fs::create_dir_all(&images_dir);
        let _ = fs::create_dir_all(&web_dir);

        let metadata_path = pdir.join("metadata.json");
        let mut existing_meta: Map<String, Value> = fs::read_to_string(&metadata_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();

        let mut existing_images: Vec<Map<String, Value>> = Vec::new();
        if let Some(arr) = existing_meta.get("images").and_then(Value::as_array) {
            for item in arr {
                let Some(mut obj) = item.as_object().cloned() else {
                    continue;
                };
                let target_name = obj
                    .get("profile_path")
                    .or_else(|| obj.get("filename"))
                    .and_then(Value::as_str)
                    .and_then(|s| Path::new(s).file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if !target_name.is_empty() {
                    let profile_file = images_dir.join(&target_name);
                    let web_file = web_dir.join(&target_name);
                    if profile_file.is_file() && !web_file.is_file() {
                        let _ = fs::copy(&profile_file, &web_file);
                    }
                    obj.insert(
                        "profile_path".to_string(),
                        Value::String(format!("profiles/{slug}/images/{target_name}")),
                    );
                    obj.insert(
                        "url".to_string(),
                        Value::String(format!(
                            "{}/images/profile-gallery/{slug}/{target_name}",
                            self.cfg.librechat_url
                        )),
                    );
                }
                existing_images.push(obj);
            }
        }

        if let Some(src) = source_image
            && src.is_file()
            && let Some(src_name) = src.file_name().and_then(|s| s.to_str())
        {
            let target_name = format!("{:02}_{src_name}", existing_images.len() + 1);
            let profile_target = images_dir.join(&target_name);
            let web_target = web_dir.join(&target_name);
            if !profile_target.is_file() {
                let _ = fs::copy(src, &profile_target);
                if let Ok(bytes) = fs::read(&profile_target) {
                    register_checksum_entry(
                        &self.cfg.checksum_manifest_file,
                        &profile_target,
                        &sha256_bytes(&bytes),
                        bytes.len() as u64,
                    );
                }
            }
            if !web_target.is_file() {
                let _ = fs::copy(src, &web_target);
            }
            let rel = format!("profiles/{slug}/images/{target_name}");
            let already = existing_images
                .iter()
                .any(|m| m.get("profile_path").and_then(Value::as_str) == Some(rel.as_str()));
            if !already {
                let mut entry = Map::new();
                entry.insert("filename".to_string(), Value::String(src_name.to_string()));
                entry.insert("profile_path".to_string(), Value::String(rel));
                entry.insert(
                    "url".to_string(),
                    Value::String(format!(
                        "{}/images/profile-gallery/{slug}/{target_name}",
                        self.cfg.librechat_url
                    )),
                );
                existing_images.push(entry);
            }
        }

        let vector_count = self.store.count_by_identity(identity);
        existing_meta.insert("identity".to_string(), Value::String(identity.to_string()));
        existing_meta
            .entry("database".to_string())
            .or_insert_with(|| Value::String("rojakface_vectors".to_string()));
        existing_meta.insert("vector_count".to_string(), json!(vector_count));
        existing_meta.insert(
            "stored_original_image_count".to_string(),
            json!(existing_images.len()),
        );
        existing_meta.insert(
            "images".to_string(),
            Value::Array(existing_images.into_iter().map(Value::Object).collect()),
        );
        if let Ok(pretty) = serde_json::to_string_pretty(&Value::Object(existing_meta)) {
            let _ = fs::write(&metadata_path, pretty);
        }
    }

    fn maybe_save_recognized_image(&self, identity: &str, image_path: &Path, score: f32) -> bool {
        if identity.is_empty() || identity == "Unknown" || score < AUTO_GALLERY_MIN_SCORE {
            return false;
        }
        let images_dir = self
            .cfg
            .profile_dir
            .join(profile_slug(identity))
            .join("images");
        let existing_files: Vec<PathBuf> = fs::read_dir(&images_dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && is_image_ext(p))
            .collect();
        if existing_files.len() >= AUTO_GALLERY_MAX_IMAGES {
            return false;
        }
        let Ok(new_hash) = perceptual_hash(image_path) else {
            return false;
        };
        for existing in existing_files {
            if let Ok(ex_hash) = perceptual_hash(&existing)
                && hash_distance(&new_hash, &ex_hash) <= PERCEPTUAL_HASH_DISTANCE
            {
                return false;
            }
        }
        self.sync_profile_folder(identity, Some(image_path));
        true
    }

    pub fn public_profile_display(&self, canonical: &str) -> Option<Value> {
        let slug = profile_slug(canonical);
        let meta_path = self.cfg.profile_dir.join(&slug).join("metadata.json");
        let text = fs::read_to_string(&meta_path).ok()?;
        let profile = serde_json::from_str::<Value>(&text).ok()?;

        let public_details = public_other_details(profile.get("other_details"));
        let identity_name = profile
            .get("identity")
            .and_then(Value::as_str)
            .unwrap_or(canonical)
            .to_string();

        let mut public_fields = vec![json!({ "field": "Name", "value": identity_name })];
        if let Some(grp) = profile
            .get("group")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            public_fields.push(json!({ "field": "Group", "value": grp }));
        }
        for (k, v) in &public_details {
            public_fields.push(json!({ "field": k, "value": v }));
        }

        let def_desc = profile
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
        let def_by = profile
            .get("photograph_by")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
        let def_loc = profile
            .get("location")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
        let def_date = profile
            .get("date")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
        let def_time = profile
            .get("time")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");

        let mut images = Vec::new();
        if let Some(arr) = profile.get("images").and_then(Value::as_array) {
            for item in arr {
                let Some(url) = item.get("url").and_then(Value::as_str) else {
                    continue;
                };
                if url.is_empty() {
                    continue;
                }
                let pick = |key: &str, fallback: &str| -> String {
                    item.get(key)
                        .and_then(Value::as_str)
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or(fallback)
                        .to_string()
                };
                images.push(json!({
                    "filename": item.get("filename").cloned().unwrap_or(Value::Null),
                    "url": url,
                    "description": pick("description", def_desc),
                    "photograph_by": pick("photograph_by", def_by),
                    "location": pick("location", def_loc),
                    "date": pick("date", def_date),
                    "time": format_time_12h(&pick("time", def_time)),
                }));
            }
        }

        Some(json!({
            "name": identity_name,
            "public_fields": public_fields,
            "public_other_details": Value::Object(public_details),
            "images": images,
        }))
    }

    // ─── Core Biometric Operations ──────────────────────────────────────────

    fn recognize_path(&self, path: &Path, threshold: f32) -> Result<Vec<Value>, String> {
        let faces = self.engine.extract_features_from_path(path)?;
        let mut out = Vec::with_capacity(faces.len());
        for face in faces {
            if face.embedding.is_empty() {
                out.push(json!({
                    "box": face.box_xywh,
                    "identity": "Unknown",
                    "confidence": 0.0,
                    "crop_path": Value::Null,
                    "reason": "face too small for reliable embedding",
                }));
                continue;
            }
            let hits = self.store.search(&face.embedding, 1);
            if let Some(hit) = hits.first()
                && hit.score >= threshold
            {
                out.push(json!({
                    "box": face.box_xywh,
                    "identity": hit.point.identity,
                    "confidence": (f64::from(hit.score) * 10000.0).round() / 10000.0,
                    "crop_path": if hit.point.crop_path.is_empty() { Value::Null } else { Value::String(hit.point.crop_path.clone()) },
                }));
            } else {
                out.push(json!({
                    "box": face.box_xywh,
                    "identity": "Unknown",
                    "confidence": 0.0,
                    "crop_path": Value::Null,
                }));
            }
        }
        Ok(out)
    }

    fn learn_bytes(
        &self,
        image_bytes: &[u8],
        threshold: f32,
        filename: Option<&str>,
    ) -> Result<Value, String> {
        let saved_orig = if let Some(fname) = filename {
            let unique = format!("{}_{fname}", Uuid::new_v4().simple());
            let _ = fs::write(
                self.cfg.store_faces_dir.join("originals").join(&unique),
                image_bytes,
            );
            unique
        } else {
            String::new()
        };

        let faces = self.engine.extract_features_from_bytes(image_bytes)?;
        if faces.is_empty() {
            return Ok(json!({ "new_faces": 0, "known_faces": 0, "details": [] }));
        }

        // Phase 1: Search all faces before inserting (prevents batch cross-contamination).
        enum Decision {
            Skipped,
            Known(String, f32, String),
            New,
        }
        let mut decisions = Vec::with_capacity(faces.len());
        for face in &faces {
            if face.embedding.is_empty() {
                decisions.push(Decision::Skipped);
                continue;
            }
            let hits = self.store.search(&face.embedding, 1);
            if let Some(hit) = hits.first()
                && hit.score >= threshold
            {
                decisions.push(Decision::Known(
                    hit.point.identity.clone(),
                    hit.score,
                    hit.point.crop_path.clone(),
                ));
            } else {
                decisions.push(Decision::New);
            }
        }

        // Phase 2: Insert new faces.
        let mut new_faces = 0usize;
        let mut known_faces = 0usize;
        let mut details = Vec::new();
        for (face, decision) in faces.into_iter().zip(decisions) {
            match decision {
                Decision::Skipped => {
                    details.push(json!({
                        "status": "skipped",
                        "box": face.box_xywh,
                        "reason": "face too small for reliable embedding",
                    }));
                }
                Decision::Known(identity, score, matched_crop) => {
                    let crop_name = format!(
                        "{identity}_{}.jpg",
                        &Uuid::new_v4().simple().to_string()[..4]
                    );
                    let _ = fs::write(
                        self.cfg.store_faces_dir.join("crops").join(&crop_name),
                        &face.face_crop_jpeg,
                    );
                    known_faces += 1;
                    details.push(json!({
                        "status": "known",
                        "identity": identity,
                        "confidence": score,
                        "matched_crop_path": if matched_crop.is_empty() { Value::Null } else { Value::String(matched_crop) },
                    }));
                }
                Decision::New => {
                    let short = Uuid::new_v4().simple().to_string();
                    let new_identity = format!("Person_{}", &short[..8]);
                    let crop_name = format!("{new_identity}_{}.jpg", &short[8..12]);
                    let crop_rel = format!("crops/{crop_name}");
                    let _ = fs::write(
                        self.cfg.store_faces_dir.join("crops").join(&crop_name),
                        &face.face_crop_jpeg,
                    );
                    let pt = FacePoint::new(
                        Uuid::new_v4().to_string(),
                        new_identity.clone(),
                        saved_orig.clone(),
                        crop_rel,
                        face.embedding,
                    );
                    self.store.upsert(pt)?;
                    new_faces += 1;
                    details.push(json!({
                        "status": "new",
                        "identity": new_identity,
                        "confidence": 0.0,
                    }));
                }
            }
        }

        Ok(json!({
            "new_faces": new_faces,
            "known_faces": known_faces,
            "details": details,
        }))
    }

    // ─── Pending Requests Queue ─────────────────────────────────────────────

    pub fn load_pending_requests(&self) -> Map<String, Value> {
        let _guard = self.pending_lock.lock().expect("pending lock");
        fs::read_to_string(&self.cfg.pending_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    }

    fn save_pending_requests_unlocked(&self, reqs: &Map<String, Value>) {
        if let Ok(text) = serde_json::to_string_pretty(&Value::Object(reqs.clone())) {
            let _ = fs::write(&self.cfg.pending_file, text);
        }
    }

    fn append_audit_history(&self, entry: &Value) {
        let history_path = self.cfg.pending_dir.join("history.jsonl");
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&history_path)
            && let Ok(line) = serde_json::to_string(entry)
        {
            let _ = writeln!(file, "{line}");
        }
    }

    fn cleanup_staged_request_images(&self, req_id: &str) {
        if let Ok(entries) = fs::read_dir(&self.cfg.pending_images_dir) {
            let prefix = format!("{req_id}_");
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with(&prefix))
                {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    fn create_metadata_update_request(
        &self,
        identity: &str,
        proposed_metadata: Map<String, Value>,
        notes: &str,
    ) -> Value {
        let clean = self.canonical_identity(identity);
        let req_id = format!(
            "REQ-{}",
            Uuid::new_v4().simple().to_string()[..8].to_uppercase()
        );
        let now = Utc::now().to_rfc3339();
        let entry = json!({
            "request_id": req_id,
            "type": "metadata_update",
            "identity": clean,
            "proposed_metadata": Value::Object(proposed_metadata.clone()),
            "notes": notes,
            "submitted_by": "guest",
            "submitted_at": now,
            "status": "pending",
        });
        {
            let _guard = self.pending_lock.lock().expect("pending lock");
            let mut reqs = fs::read_to_string(&self.cfg.pending_file)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            reqs.insert(req_id.clone(), entry);
            self.save_pending_requests_unlocked(&reqs);
        }
        json!({
            "status": "staged_for_admin_review",
            "request_id": req_id,
            "type": "metadata_update",
            "identity": clean,
            "proposed_metadata": Value::Object(proposed_metadata),
            "message": format!("Metadata update request {req_id} for '{clean}' has been staged for Admin Portal review."),
        })
    }

    fn create_enrollment_request(
        &self,
        identity: &str,
        image_path: &Path,
        proposed_metadata: Map<String, Value>,
        notes: &str,
    ) -> Value {
        let clean = self.canonical_identity(identity);
        // Check if face already matches an enrolled person -> auto-route to metadata update!
        if let Ok(existing) = self.recognize_path(image_path, MIN_CLIP_IDENTIFY_THRESHOLD) {
            let matched: Vec<&str> = existing
                .iter()
                .filter_map(|m| m.get("identity").and_then(Value::as_str))
                .filter(|&id| id != "Unknown")
                .collect();
            if !matched.is_empty() && !proposed_metadata.is_empty() {
                return self.create_metadata_update_request(matched[0], proposed_metadata, notes);
            }
        }

        let req_id = format!(
            "REQ-{}",
            Uuid::new_v4().simple().to_string()[..8].to_uppercase()
        );
        let ext = image_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");
        let staged_source = self
            .cfg
            .pending_images_dir
            .join(format!("{req_id}_source.{ext}"));
        let _ = fs::copy(image_path, &staged_source);

        let staged_crop = if let Ok((crop_bytes, _)) = self.engine.primary_face_bytes(image_path) {
            let cp = self
                .cfg
                .pending_images_dir
                .join(format!("{req_id}_primary.jpg"));
            let _ = fs::write(&cp, crop_bytes);
            Some(cp.to_string_lossy().to_string())
        } else {
            None
        };

        let now = Utc::now().to_rfc3339();
        let entry = json!({
            "request_id": req_id,
            "type": "enrollment",
            "identity": clean,
            "staged_image_path": staged_source.to_string_lossy(),
            "staged_crop_path": staged_crop,
            "proposed_metadata": Value::Object(proposed_metadata.clone()),
            "notes": notes,
            "submitted_by": "guest",
            "submitted_at": now,
            "status": "pending",
        });
        {
            let _guard = self.pending_lock.lock().expect("pending lock");
            let mut reqs = fs::read_to_string(&self.cfg.pending_file)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            reqs.insert(req_id.clone(), entry);
            self.save_pending_requests_unlocked(&reqs);
        }
        json!({
            "status": "staged_for_admin_review",
            "request_id": req_id,
            "type": "enrollment",
            "identity": clean,
            "proposed_metadata": Value::Object(proposed_metadata),
            "message": format!("Enrollment request {req_id} for '{clean}' has been staged for Admin Portal review."),
        })
    }

    // ─── Visual Attribute Search (visual_index.json + Ollama fallback) ──────

    async fn match_images_via_ollama(
        &self,
        entries: &[(Value, PathBuf)],
        query: &str,
    ) -> Vec<Value> {
        if entries.is_empty() {
            return Vec::new();
        }
        let mut images_b64 = Vec::with_capacity(entries.len());
        for (_, p) in entries {
            let Ok(b64) = encode_image_b64(p, 320) else {
                return Vec::new();
            };
            images_b64.push(b64);
        }
        let n = images_b64.len();
        let body = json!({
            "model": self.cfg.vision_model,
            "stream": false,
            "think": false,
            "keep_alive": "30m",
            "options": { "temperature": 0, "num_predict": 32 },
            "messages": [{
                "role": "user",
                "content": format!(
                    "You are given {n} numbered photo(s) in order from 1 to {n}. Which photo number(s) match the request: '{query}'? Reply ONLY with the matching number(s) separated by commas (e.g. 1 or 1, 3), or NONE."
                ),
                "images": images_b64,
            }],
        });
        let Ok(resp) = self
            .http
            .post(&self.cfg.ollama_url)
            .json(&body)
            .send()
            .await
        else {
            return Vec::new();
        };
        let Ok(data) = resp.json::<Value>().await else {
            return Vec::new();
        };
        let answer = data
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_uppercase();
        if answer.contains("NONE") {
            return Vec::new();
        }
        static NUM_RE: OnceLock<Regex> = OnceLock::new();
        let re = NUM_RE.get_or_init(|| Regex::new(r"\b(\d+)\b").expect("regex"));
        let mut selected = Vec::new();
        let mut seen = HashSet::new();
        for caps in re.captures_iter(&answer) {
            if let Ok(num) = caps[1].parse::<usize>()
                && num >= 1
                && num <= n
                && seen.insert(num)
            {
                selected.push(entries[num - 1].0.clone());
            }
        }
        selected
    }

    async fn filter_profile_images(
        &self,
        canonical: &str,
        images: Vec<Value>,
        query: &str,
    ) -> Vec<Value> {
        let clean_query = query.trim();
        if clean_query.is_empty() || images.is_empty() {
            return images;
        }
        let token_groups = expand_bilingual_query_groups(clean_query);

        let slug = profile_slug(canonical);
        let visual_index_path = self.cfg.profile_dir.join(&slug).join("visual_index.json");
        let visual_index: Map<String, Value> = fs::read_to_string(&visual_index_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();

        if !token_groups.is_empty() {
            let mut text_matches = Vec::new();
            for item in &images {
                let url_name = item
                    .get("url")
                    .or_else(|| item.get("filename"))
                    .and_then(Value::as_str)
                    .and_then(|s| Path::new(s).file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let mut hay = String::new();
                for k in [
                    "description",
                    "photograph_by",
                    "location",
                    "date",
                    "time",
                    "filename",
                ] {
                    if let Some(s) = item.get(k).and_then(Value::as_str) {
                        hay.push_str(s);
                        hay.push(' ');
                    }
                }
                if let Some(v) = visual_index.get(url_name).and_then(Value::as_str) {
                    hay.push_str(v);
                }
                let hay_lower = hay.to_ascii_lowercase();
                if token_groups
                    .iter()
                    .all(|group| group.iter().any(|syn| hay_lower.contains(syn.as_str())))
                {
                    text_matches.push(item.clone());
                }
            }
            if !text_matches.is_empty() {
                return text_matches;
            }
        }

        let mut candidates = Vec::new();
        for item in images {
            let url_name = item
                .get("url")
                .or_else(|| item.get("filename"))
                .and_then(Value::as_str)
                .and_then(|s| Path::new(s).file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            for cand in [
                self.cfg
                    .profile_dir
                    .join(&slug)
                    .join("images")
                    .join(&url_name),
                self.cfg.profile_web_dir.join(&slug).join(&url_name),
            ] {
                if cand.is_file() {
                    candidates.push((item, cand));
                    break;
                }
            }
        }
        self.match_images_via_ollama(&candidates, clean_query).await
    }

    // ─── ALL 24 MCP TOOLS ───────────────────────────────────────────────────

    pub fn tool_detect_faces(&self, image_path: &str) -> Value {
        let resolved = match self.resolve_image_path(image_path) {
            Ok(p) => p,
            Err(reason) => return json!({ "status": "unresolved", "reason": reason }),
        };
        let detections = match self.engine.extract_features_from_path(&resolved) {
            Ok(d) => d,
            Err(reason) => return json!({ "status": "error", "reason": reason }),
        };
        let faces: Vec<Value> = detections
            .iter()
            .enumerate()
            .map(|(idx, d)| {
                json!({
                    "face_index": idx,
                    "box_xywh": d.box_xywh,
                    "confidence": (f64::from(d.confidence) * 10000.0).round() / 10000.0,
                    "embeddable": !d.embedding.is_empty(),
                })
            })
            .collect();
        json!({
            "file": resolved.file_name().and_then(|s| s.to_str()).unwrap_or(""),
            "faces_detected": faces.len(),
            "faces": faces,
        })
    }

    pub fn tool_ocr_document(&self, image_path: &str, max_chunks: i64) -> Value {
        let resolved = match self.resolve_image_path(image_path) {
            Ok(p) => p,
            Err(reason) => return json!({ "status": "unresolved", "reason": reason }),
        };
        match build_ocr_document_chunks(&self.cfg, &resolved, max_chunks) {
            Ok(v) => v,
            Err(reason) => json!({ "status": "error", "reason": reason }),
        }
    }

    pub fn tool_decode_jawi(&self, text: &str) -> Value {
        decode_jawi_text(text.to_string())
            .unwrap_or_else(|e| json!({ "status": "error", "reason": e }))
    }

    pub fn tool_decode_ancient_script(&self, text: &str, script_hint: &str) -> Value {
        decode_ancient_script(text, script_hint)
    }

    /// Dedicated zero-temperature local Vision-AI forensic auditor.
    /// Uses factual observation-first booleans (scene description, physical plausibility,
    /// garbled AI text, visible watermarks, screenshot cues) so Rust computes the verdict deterministically.
    async fn visual_forensic_audit(&self, image_path: &Path) -> Option<Value> {
        let p = image_path.to_path_buf();
        let b64 = tokio::task::spawn_blocking(move || encode_image_b64(&p, 384))
            .await
            .ok()?
            .ok()?;

        let prompt = concat!(
            "Inspect this image carefully and reply ONLY with JSON (no markdown):\n",
            "{\n",
            "  \"scene_description\": \"one short sentence describing what is in the image\",\n",
            "  \"physically_possible_in_real_life\": true or false,\n",
            "  \"has_garbled_ai_text\": true or false,\n",
            "  \"visible_watermark\": \"none\",\n",
            "  \"is_screenshot_or_repost\": true or false,\n",
            "  \"anomalies\": [],\n",
            "  \"reason\": \"one short sentence\"\n",
            "}\n",
            "Rules:\n",
            "- physically_possible_in_real_life is FALSE if the image shows something impossible in reality (such as a living human head fused onto a metal robot/cyborg/animal body, sci-fi hybrid, floating impossible objects, or surreal CGI). It is TRUE for ordinary real people, events, buildings, or documents (even if background is blurry or lighting is bright).\n",
            "- has_garbled_ai_text is TRUE only if visible letters/numbers on chests, signs, or labels are distorted/nonsense AI gibberish.\n",
            "- visible_watermark: put \"none\" if no watermark/logo is visible, otherwise put the exact name (e.g. \"Gemini\", \"Veo\", \"Sora\", \"TikTok\", \"Shutterstock\", \"Getty\")."
        );

        let body = json!({
            "model": self.cfg.vision_model,
            "stream": false,
            "think": false,
            "keep_alive": "30m",
            "options": { "temperature": 0, "num_predict": 160 },
            "messages": [{
                "role": "user",
                "content": prompt,
                "images": [b64],
            }],
        });

        let resp = self
            .http
            .post(&self.cfg.ollama_url)
            .timeout(std::time::Duration::from_secs(90))
            .json(&body)
            .send()
            .await
            .ok()?;
        let data = resp.json::<Value>().await.ok()?;
        let raw = data
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if raw.is_empty() {
            return None;
        }
        let parsed: Value = serde_json::from_str(raw).ok().or_else(|| {
            let start = raw.find('{')?;
            let end = raw.rfind('}')?;
            (end > start).then(|| serde_json::from_str(&raw[start..=end]).ok())?
        })?;
        if !parsed.is_object() {
            return None;
        }
        Some(parsed)
    }

    /// Multi-layer deep authenticity check:
    /// Layer 1: Archive SHA-256 & perceptual hash duplicate check
    /// Layer 2: Deep metadata, C2PA/JUMBF, IPTC DigitalSourceType, XMP, PNG chunks, JPEG structure
    /// Layer 3: Pixel-level ELA splice check, local noise CV, 8x8 blockiness, lossless LSB probe
    /// Layer 4: Dedicated zero-temperature local Vision-AI forensic audit fused into the verdict
    /// Layers 1–3 (CPU/disk) run concurrently with Layer 4 (Ollama HTTP) via `tokio::join!`.
    pub async fn analyze_authenticity_deep(&self, path: &Path) -> Result<Value, String> {
        let p = path.to_path_buf();
        let profile_dir = self.cfg.profile_dir.clone();
        let locations_dir = self.cfg.locations_dir.clone();
        let documents_dir = self.cfg.documents_dir.clone();
        let file_forensics_fut = tokio::task::spawn_blocking(move || {
            let roots = [
                profile_dir.as_path(),
                locations_dir.as_path(),
                documents_dir.as_path(),
            ];
            analyze_authenticity(&p, &roots)
        });
        let visual_fut = async {
            if is_image_ext(path) {
                self.visual_forensic_audit(path).await
            } else {
                None
            }
        };
        let (file_res, visual_opt) = tokio::join!(file_forensics_fut, visual_fut);
        let mut report = file_res.map_err(|e| format!("Forensics task failed: {e}"))??;
        if let Some(visual) = visual_opt {
            fuse_visual_audit(&mut report, visual);
        }
        Ok(report)
    }

    pub async fn tool_analyze_image(&self, image_path: &str) -> Value {
        let resolved = match self.resolve_image_path(image_path) {
            Ok(p) => p,
            Err(reason) => return json!({ "status": "unresolved", "reason": reason }),
        };

        // Overlap heavy CPU/ONNX work (CLIP location match, YuNet faces, conservation) with deep authenticity + Ollama.
        let rp = resolved.clone();
        let engine = self.engine.clone();
        let loc_store = self.location_store.clone();
        let cfg = self.cfg.clone();
        let vision_task = tokio::task::spawn_blocking(move || {
            let mut matched_locations = Vec::new();
            if let Ok(emb) = engine.embed_full_image_from_path(&rp) {
                let hits = loc_store.search(&emb, 3);
                for hit in hits {
                    if hit.score >= 0.78 {
                        let loc_detail = load_location(&cfg, &hit.point.crop_path);
                        matched_locations.push(json!({
                            "location_name": hit.point.identity,
                            "location_slug": hit.point.crop_path,
                            "similarity": (f64::from(hit.score) * 10000.0).round() / 10000.0,
                            "matched_reference_image": hit.point.filename,
                            "location_details": loc_detail,
                        }));
                    }
                }
            }
            let faces_detected = engine
                .extract_features_from_path(&rp)
                .map(|d| d.len())
                .unwrap_or(0);
            let conservation = analyze_image_quality_and_conservation(&rp)
                .unwrap_or_else(|e| json!({ "error": e }));
            (matched_locations, faces_detected, conservation)
        });

        let (auth_res, vision_res) =
            tokio::join!(self.analyze_authenticity_deep(&resolved), vision_task);
        let authenticity = auth_res.unwrap_or_else(|e| json!({ "error": e }));
        let metadata = authenticity
            .get("metadata")
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or_else(|| {
                extract_file_metadata(&resolved).unwrap_or_else(|e| json!({ "error": e }))
            });
        let (matched_locations, faces_detected, conservation) =
            vision_res.unwrap_or_else(|e| (Vec::new(), 0, json!({ "error": e.to_string() })));

        json!({
            "file": resolved.file_name().and_then(|s| s.to_str()).unwrap_or(""),
            "file_metadata": metadata,
            "authenticity_analysis": authenticity,
            "conservation_assessment": conservation,
            "matched_archival_locations": matched_locations,
            "faces_detected": faces_detected,
            "analysis_instruction": "Present the extracted metadata, the fused authenticity verdict (Original/Copy/AI-Generated/Manipulated — NEVER override a HIGH or MEDIUM risk verdict), image quality & archival damage assessment (tear/faded/mold/aging + healing suggestions), and any matched archival location details in clear, friendly language.",
        })
    }

    pub async fn tool_assess_conservation(&self, image_path: &str) -> Value {
        let resolved = match self.resolve_image_path(image_path) {
            Ok(p) => p,
            Err(reason) => return json!({ "status": "unresolved", "reason": reason }),
        };
        let rp = resolved.clone();
        let cons_fut =
            tokio::task::spawn_blocking(move || analyze_image_quality_and_conservation(&rp));
        let (cons_res, auth_res) =
            tokio::join!(cons_fut, self.analyze_authenticity_deep(&resolved));
        let mut report = match cons_res {
            Ok(Ok(r)) => r,
            Ok(Err(reason)) => return json!({ "status": "error", "reason": reason }),
            Err(e) => return json!({ "status": "error", "reason": e.to_string() }),
        };
        if let Ok(auth) = auth_res
            && let Some(obj) = report.as_object_mut()
        {
            obj.insert(
                "authenticity_summary".to_string(),
                json!({
                    "verdict": auth["verdict"],
                    "plain_language_verdict": auth["plain_language_verdict"],
                    "fraud_risk": auth["fraud_risk"],
                    "confidence": auth["confidence"],
                    "is_exact_copy": auth["is_exact_copy"],
                    "is_near_duplicate_copy": auth["is_near_duplicate_copy"],
                    "c2pa_present": auth["c2pa_present"],
                    "camera_exif_present": auth["camera_exif_present"],
                    "watermark_and_provenance": auth["watermark_and_provenance"],
                    "jpeg_structure": auth["jpeg_structure"],
                    "pixel_forensics": auth["pixel_forensics"],
                    "visual_forensic_audit": auth["visual_forensic_audit"],
                    "forensic_signals": auth["forensic_signals"],
                    "sha256": auth["metadata"]["sha256"],
                }),
            );
        }
        report
    }

    pub fn tool_enhance_image(&self, image_path: &str, mode: &str) -> Value {
        let resolved = match self.resolve_image_path(image_path) {
            Ok(p) => p,
            Err(reason) => return json!({ "status": "unresolved", "reason": reason }),
        };
        match enhance_archival_image(&self.cfg, &resolved, mode) {
            Ok(res) => res,
            Err(reason) => json!({ "status": "error", "reason": reason }),
        }
    }

    pub fn tool_verify_integrity(&self, update_manifest: bool) -> Value {
        let archive_roots = [
            self.cfg.profile_dir.as_path(),
            self.cfg.locations_dir.as_path(),
            self.cfg.documents_dir.as_path(),
        ];
        match verify_archive_integrity(
            &self.cfg.checksum_manifest_file,
            &archive_roots,
            update_manifest,
        ) {
            Ok(report) => report,
            Err(reason) => json!({ "status": "error", "reason": reason }),
        }
    }

    pub async fn tool_system_health(&self) -> Value {
        let identities = self.store.list_counts().len();
        let vectors = self.store.total_vectors();
        let locations = list_locations(&self.cfg).len();
        let documents = list_documents(&self.cfg).len();
        let pending = self
            .load_pending_requests()
            .values()
            .filter(|r| r.get("status").and_then(Value::as_str) == Some("pending"))
            .count();
        self.monitor
            .snapshot(
                &self.cfg,
                &self.http,
                ArchiveCounts {
                    identities,
                    vectors,
                    locations,
                    documents,
                    pending_requests: pending,
                },
            )
            .await
    }

    pub fn clear_temp_image_caches(&self) -> Value {
        let mut removed_files = 0usize;
        let mut freed_bytes = 0u64;
        for subdir in ["ocr-chunks", "enhanced"] {
            let dir = self.cfg.librechat_dir.join("images").join(subdir);
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        if let Ok(m) = fs::metadata(&p) {
                            freed_bytes += m.len();
                        }
                        if fs::remove_file(&p).is_ok() {
                            removed_files += 1;
                        }
                    }
                }
            }
        }
        self.clear_identification_cache();
        json!({
            "status": "cleared",
            "removed_files": removed_files,
            "freed_bytes": freed_bytes,
            "freed_mb": ((freed_bytes as f64) / (1024.0 * 1024.0) * 100.0).round() / 100.0,
        })
    }

    pub async fn tool_detect_authenticity(&self, file_path: &str) -> Value {
        let resolved = match self.resolve_image_path(file_path) {
            Ok(p) => p,
            Err(reason) => return json!({ "status": "unresolved", "reason": reason }),
        };
        match self.analyze_authenticity_deep(&resolved).await {
            Ok(report) => report,
            Err(reason) => json!({ "status": "error", "reason": reason }),
        }
    }

    pub async fn tool_summarize_document(
        &self,
        file_path: &str,
        raw_text: &str,
        language: &str,
    ) -> Value {
        let mut text = raw_text.trim().to_string();
        let mut file_meta: Value = Value::Null;
        let mut authenticity: Value = Value::Null;
        let mut ocr_chunks: Value = Value::Null;

        if text.is_empty() && !file_path.trim().is_empty() {
            let resolved = match self.resolve_image_path(file_path) {
                Ok(p) => p,
                Err(_) => {
                    let direct = PathBuf::from(file_path.trim());
                    let rel = self.cfg.project_root.join(file_path.trim());
                    if direct.is_file() {
                        direct
                    } else if rel.is_file() {
                        rel
                    } else {
                        return json!({
                            "status": "unresolved",
                            "reason": format!("Document '{file_path}' not found on disk."),
                        });
                    }
                }
            };
            file_meta = summarize_file_info(&resolved).unwrap_or(Value::Null);
            authenticity = self
                .analyze_authenticity_deep(&resolved)
                .await
                .unwrap_or(Value::Null);
            if is_image_ext(&resolved) {
                ocr_chunks =
                    build_ocr_document_chunks(&self.cfg, &resolved, 3).unwrap_or(Value::Null);
            } else {
                text = extract_text_from_file(&resolved).unwrap_or_default();
            }
        }

        if text.is_empty() && !ocr_chunks.is_null() {
            return json!({
                "status": "image_document_chunked",
                "file_metadata": file_meta,
                "authenticity_analysis": authenticity,
                "ocr_document_chunks": ocr_chunks,
                "instruction": "Read the attached document image chunk-by-chunk, extract all text accurately, then produce a structured archival description & summary covering: Summary, Key Topics, Key Names, Key Dates, Events, and the Authenticity Verdict.",
            });
        }

        if text.is_empty() {
            return json!({
                "status": "rejected",
                "reason": "Provide either `text` or a valid `file_path` (TXT, MD, CSV, JSON, HTML, DOCX, PDF, or image scan).",
            });
        }

        let structure = extract_document_structure(&text);
        let ai_summary = summarize_with_ollama(&self.http, &self.cfg, &text, language).await;
        let final_summary = ai_summary.clone().unwrap_or_else(|| {
            structure["extractive_summary"]
                .as_str()
                .unwrap_or("")
                .to_string()
        });

        json!({
            "status": "summarized",
            "file_metadata": file_meta,
            "authenticity_analysis": authenticity,
            "archival_summary": final_summary,
            "generated_by_llm": ai_summary.is_some(),
            "structured_extraction": structure,
        })
    }

    pub fn tool_search_locations(&self, query: &str) -> Value {
        let items = search_locations(&self.cfg, query);
        json!({
            "status": if items.is_empty() { "not_found" } else { "found" },
            "query": query,
            "total_matches": items.len(),
            "locations": items,
        })
    }

    pub fn tool_search_documents(&self, query: &str, script_filter: &str) -> Value {
        let items = search_documents(&self.cfg, query, script_filter);
        json!({
            "status": if items.is_empty() { "not_found" } else { "found" },
            "query": query,
            "script_filter": script_filter,
            "total_matches": items.len(),
            "documents": items,
        })
    }

    pub fn tool_identify_faces(
        &self,
        image_path: &str,
        threshold: f32,
        save_new_image: bool,
    ) -> Value {
        let resolved = match self.resolve_image_path(image_path) {
            Ok(p) => p,
            Err(reason) => return json!({ "status": "unresolved", "reason": reason }),
        };
        let safe_threshold = threshold.max(MIN_CLIP_IDENTIFY_THRESHOLD);

        let build_display_instruction = |status: &str| -> Value {
            if status == "MATCHED" {
                Value::String(
                    "If the user only asked who the person is, reply concisely with the enrolled name(s) and confidence. If the user also asked for details/profile/photos, render Table 1 (| Field | Value | from matched_profiles[0].public_fields) and Table 2 (| Image | Description | Photograph by | Location | Date | Time |) with ONE ROW FOR EVERY IMAGE in matched_profiles[0].images.".to_string(),
                )
            } else {
                Value::Null
            }
        };

        if let Some(cached) = self.cached_identification(&resolved, safe_threshold) {
            let cached_matches = cached["matches"].as_array().cloned().unwrap_or_default();
            let mut unique_names = Vec::new();
            for m in &cached_matches {
                if let Some(id) = m.get("identity").and_then(Value::as_str)
                    && id != "Unknown"
                    && !unique_names.contains(&id.to_string())
                {
                    unique_names.push(id.to_string());
                }
            }
            let matched_profiles: Vec<Value> = unique_names
                .iter()
                .filter_map(|n| self.public_profile_display(n))
                .collect();
            let status = if !unique_names.is_empty() {
                "MATCHED"
            } else if cached_matches.is_empty() {
                "NO_FACE_DETECTED"
            } else {
                "NOT_IN_DATABASE"
            };
            let ocr_chunks = if status == "NO_FACE_DETECTED" {
                build_ocr_document_chunks(&self.cfg, &resolved, 3).ok()
            } else {
                None
            };
            return json!({
                "file": cached["file"],
                "database_status": status,
                "faces_detected": cached_matches.len(),
                "matched_identities": unique_names,
                "matched_profiles": matched_profiles,
                "ocr_document_chunks": ocr_chunks,
                "display_instruction": build_display_instruction(status),
                "applied_threshold": (f64::from(safe_threshold) * 10000.0).round() / 10000.0,
                "matches_count": cached_matches.len(),
                "matches": cached_matches,
                "deduplicated": true,
                "cached_from": cached["cached_from"],
            });
        }

        let clean_matches = match self.recognize_path(&resolved, safe_threshold) {
            Ok(m) => m,
            Err(reason) => return json!({ "status": "error", "reason": reason }),
        };
        let mut matched_names = Vec::new();
        for m in &clean_matches {
            if let Some(id) = m.get("identity").and_then(Value::as_str)
                && id != "Unknown"
            {
                matched_names.push(id.to_string());
            }
        }

        let mut gallery_saved = false;
        if save_new_image && clean_matches.len() == 1 && matched_names.len() == 1 {
            let score = clean_matches[0]
                .get("confidence")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as f32;
            gallery_saved = self.maybe_save_recognized_image(&matched_names[0], &resolved, score);
        }

        let mut unique_names = Vec::new();
        for name in &matched_names {
            if !unique_names.contains(name) {
                unique_names.push(name.clone());
            }
        }
        let matched_profiles: Vec<Value> = unique_names
            .iter()
            .filter_map(|n| self.public_profile_display(n))
            .collect();
        let db_status = if !matched_names.is_empty() {
            "MATCHED"
        } else if clean_matches.is_empty() {
            "NO_FACE_DETECTED"
        } else {
            "NOT_IN_DATABASE"
        };
        let ocr_chunks = if db_status == "NO_FACE_DETECTED" {
            build_ocr_document_chunks(&self.cfg, &resolved, 3).ok()
        } else {
            None
        };

        self.cache_identification(&resolved, safe_threshold, &clean_matches);
        json!({
            "file": resolved.file_name().and_then(|s| s.to_str()).unwrap_or(""),
            "database_status": db_status,
            "faces_detected": clean_matches.len(),
            "matched_identities": matched_names,
            "matched_profiles": matched_profiles,
            "ocr_document_chunks": ocr_chunks,
            "display_instruction": build_display_instruction(db_status),
            "gallery_image_saved": gallery_saved,
            "applied_threshold": (f64::from(safe_threshold) * 10000.0).round() / 10000.0,
            "matches_count": clean_matches.len(),
            "matches": clean_matches,
            "deduplicated": false,
        })
    }

    pub fn tool_request_enrollment(
        &self,
        identity: &str,
        image_path: &str,
        proposed_metadata: &str,
        notes: &str,
    ) -> Value {
        let resolved = match self.resolve_image_path(image_path) {
            Ok(p) => p,
            Err(reason) => return json!({ "status": "unresolved", "reason": reason }),
        };
        let meta = parse_proposed_metadata(proposed_metadata);
        self.create_enrollment_request(identity, &resolved, meta, notes)
    }

    pub fn tool_request_update_metadata(
        &self,
        identity: &str,
        proposed_metadata: &str,
        notes: &str,
        admin_key: &str,
    ) -> Value {
        let parsed = parse_proposed_metadata(proposed_metadata);
        if self.verify_admin(admin_key) {
            let clean = identity.trim();
            let meta_file = self
                .cfg
                .profile_dir
                .join(profile_slug(clean))
                .join("metadata.json");
            let Ok(text) = fs::read_to_string(&meta_file) else {
                return json!({
                    "status": "not_found",
                    "reason": format!("Identity '{clean}' not found."),
                });
            };
            let mut meta = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            merge_profile_metadata(&mut meta, &parsed);
            if let Ok(pretty) = serde_json::to_string_pretty(&Value::Object(meta.clone())) {
                let _ = fs::write(&meta_file, pretty);
            }
            return json!({
                "status": "updated_by_admin",
                "identity": clean,
                "metadata": Value::Object(meta),
            });
        }
        self.create_metadata_update_request(identity, parsed, notes)
    }

    pub fn tool_enroll_named_identity(
        &self,
        identity: &str,
        image_path: &str,
        admin_key: &str,
        proposed_metadata: &str,
    ) -> Value {
        static NAME_RE: OnceLock<Regex> = OnceLock::new();
        let name_re = NAME_RE
            .get_or_init(|| Regex::new(r"\A[A-Za-z][A-Za-z0-9 .'-]{0,79}\z").expect("regex"));
        let clean = self.canonical_identity(identity);
        if !name_re.is_match(&clean) {
            return json!({
                "status": "rejected",
                "reason": "Identity must be a plain name.",
                "identity": clean,
            });
        }
        let resolved = match self.resolve_image_path(image_path) {
            Ok(p) => p,
            Err(reason) => return json!({ "status": "unresolved", "reason": reason }),
        };
        if !self.verify_admin(admin_key) {
            let meta = parse_proposed_metadata(proposed_metadata);
            return self.create_enrollment_request(&clean, &resolved, meta, "");
        }

        if let Ok(existing) = self.recognize_path(&resolved, MIN_CLIP_IDENTIFY_THRESHOLD) {
            let known: Vec<Value> = existing
                .into_iter()
                .filter(|m| m.get("identity").and_then(Value::as_str) != Some("Unknown"))
                .collect();
            if !known.is_empty() {
                return json!({
                    "status": "rejected",
                    "reason": "The image already matches an enrolled identity; no rename was performed.",
                    "existing_matches": known,
                });
            }
        }

        let (primary_bytes, primary_fname) = match self.engine.primary_face_bytes(&resolved) {
            Ok(x) => x,
            Err(reason) => return json!({ "status": "error", "reason": reason }),
        };
        let stats = match self.learn_bytes(
            &primary_bytes,
            MIN_CLIP_IDENTIFY_THRESHOLD,
            Some(&primary_fname),
        ) {
            Ok(s) => s,
            Err(reason) => return json!({ "status": "error", "reason": reason }),
        };

        let mut created = Vec::new();
        if let Some(arr) = stats.get("details").and_then(Value::as_array) {
            for d in arr {
                if d.get("status").and_then(Value::as_str) == Some("new")
                    && let Some(gen_id) = d.get("identity").and_then(Value::as_str)
                {
                    let _ = self.store.rename_identity(gen_id, &clean);
                    created.push(gen_id.to_string());
                }
            }
        }
        if !created.is_empty() {
            self.sync_profile_folder(&clean, Some(&resolved));
            self.clear_identification_cache();
        }
        json!({
            "status": if !created.is_empty() { "enrolled_by_admin" } else { "unchanged" },
            "identity": clean,
            "role": "admin",
            "generated_identities": created,
            "details": stats["details"],
        })
    }

    pub fn tool_enroll_or_learn(&self, image_path: &str, threshold: f32, admin_key: &str) -> Value {
        if !self.verify_admin(admin_key) {
            return json!({
                "status": "rejected",
                "reason": "Direct unsupervised learning is restricted to Admins. Guest users must use rojakface_request_enrollment.",
            });
        }
        let resolved = match self.resolve_image_path(image_path) {
            Ok(p) => p,
            Err(reason) => return json!({ "status": "unresolved", "reason": reason }),
        };
        let safe_threshold = threshold.max(MIN_CLIP_IDENTIFY_THRESHOLD);
        let (primary_bytes, primary_fname) = match self.engine.primary_face_bytes(&resolved) {
            Ok(x) => x,
            Err(reason) => return json!({ "status": "error", "reason": reason }),
        };
        let stats = match self.learn_bytes(&primary_bytes, safe_threshold, Some(&primary_fname)) {
            Ok(s) => s,
            Err(reason) => return json!({ "status": "error", "reason": reason }),
        };
        if let Some(arr) = stats.get("details").and_then(Value::as_array) {
            for d in arr {
                if let Some(gen_id) = d.get("identity").and_then(Value::as_str) {
                    self.sync_profile_folder(gen_id, Some(&resolved));
                }
            }
        }
        self.clear_identification_cache();
        stats
    }

    pub fn tool_list_pending_requests(&self, status: &str) -> Value {
        let clean_status = if status.trim().is_empty() {
            "pending".to_string()
        } else {
            status.trim().to_ascii_lowercase()
        };
        let all_reqs = self.load_pending_requests();
        let mut filtered: Vec<Value> = all_reqs
            .values()
            .filter(|r| {
                clean_status == "all"
                    || r.get("status").and_then(Value::as_str) == Some(clean_status.as_str())
            })
            .cloned()
            .collect();
        filtered.sort_by(|a, b| {
            let sa = a.get("submitted_at").and_then(Value::as_str).unwrap_or("");
            let sb = b.get("submitted_at").and_then(Value::as_str).unwrap_or("");
            sa.cmp(sb)
        });

        let formatted: Vec<Value> = filtered
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let idx = i + 1;
                let ident = r.get("identity").and_then(Value::as_str).unwrap_or("");
                json!({
                    "number": idx,
                    "request_id": r["request_id"],
                    "type": r.get("type").and_then(Value::as_str).unwrap_or("enrollment"),
                    "identity": ident,
                    "status": r["status"],
                    "submitted_at": r["submitted_at"],
                    "quick_approve": format!("approve {idx} (or 'approve {ident}')"),
                    "quick_reject": format!("reject {idx} (or 'reject {ident}')"),
                    "proposed_metadata": r.get("proposed_metadata").cloned().unwrap_or_else(|| json!({})),
                    "notes": r.get("notes").and_then(Value::as_str).unwrap_or(""),
                })
            })
            .collect();
        json!({
            "total_requests": formatted.len(),
            "filter_status": clean_status,
            "instructions": "To approve or reject, simply say 'approve 1', 'approve [Name]', 'approve latest', or 'reject 1'.",
            "requests": formatted,
        })
    }

    pub fn tool_review_pending_request(
        &self,
        request_id: &str,
        action: &str,
        admin_key: &str,
        comment: &str,
    ) -> Value {
        if !self.verify_admin(admin_key) {
            return json!({
                "status": "unauthorized",
                "reason": "Invalid or missing admin_key.",
            });
        }
        let act = action.trim().to_ascii_lowercase();
        if act != "approve" && act != "reject" {
            return json!({ "status": "rejected", "reason": "Action must be 'approve' or 'reject'." });
        }

        let target_key_opt = {
            let _guard = self.pending_lock.lock().expect("pending lock");
            let reqs = fs::read_to_string(&self.cfg.pending_file)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            let raw = request_id.trim();
            if reqs.contains_key(raw) {
                Some(raw.to_string())
            } else {
                let mut pending: Vec<(String, Value)> = reqs
                    .into_iter()
                    .filter(|(_, v)| v.get("status").and_then(Value::as_str) == Some("pending"))
                    .collect();
                pending.sort_by(|a, b| {
                    let sa =
                        a.1.get("submitted_at")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                    let sb =
                        b.1.get("submitted_at")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                    sa.cmp(sb)
                });
                if raw.eq_ignore_ascii_case("latest") {
                    pending.last().map(|(k, _)| k.clone())
                } else if let Ok(num) = raw.parse::<usize>()
                    && num >= 1
                    && num <= pending.len()
                {
                    Some(pending[num - 1].0.clone())
                } else {
                    pending
                        .into_iter()
                        .find(|(_, v)| {
                            v.get("identity")
                                .and_then(Value::as_str)
                                .is_some_and(|n| n.eq_ignore_ascii_case(raw))
                        })
                        .map(|(k, _)| k)
                }
            }
        };

        let Some(req_key) = target_key_opt else {
            return json!({ "status": "not_found", "reason": format!("Pending request '{request_id}' not found.") });
        };

        let req_obj = {
            let _guard = self.pending_lock.lock().expect("pending lock");
            let reqs = fs::read_to_string(&self.cfg.pending_file)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            reqs.get(&req_key).cloned()
        };
        let Some(mut req) = req_obj else {
            return json!({ "status": "not_found", "reason": "Request vanished." });
        };

        let req_type = req
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("enrollment")
            .to_string();
        let identity = req
            .get("identity")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let proposed = req
            .get("proposed_metadata")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        if act == "approve" {
            if req_type == "enrollment" {
                let staged_path = req
                    .get("staged_image_path")
                    .and_then(Value::as_str)
                    .map(PathBuf::from);
                if let Some(ref p) = staged_path
                    && p.is_file()
                {
                    let res = self.tool_enroll_named_identity(
                        &identity,
                        &p.to_string_lossy(),
                        &self.cfg.admin_key,
                        "",
                    );
                    let st = res.get("status").and_then(Value::as_str).unwrap_or("");
                    if st != "enrolled_by_admin" && st != "unchanged" {
                        return res;
                    }
                }
                if !proposed.is_empty() {
                    let meta_str =
                        serde_json::to_string(&Value::Object(proposed)).unwrap_or_default();
                    let _ = self.tool_request_update_metadata(
                        &identity,
                        &meta_str,
                        "",
                        &self.cfg.admin_key,
                    );
                }
            } else if !proposed.is_empty() {
                let meta_str = serde_json::to_string(&Value::Object(proposed)).unwrap_or_default();
                let res = self.tool_request_update_metadata(
                    &identity,
                    &meta_str,
                    "",
                    &self.cfg.admin_key,
                );
                if res.get("status").and_then(Value::as_str) != Some("updated_by_admin") {
                    return res;
                }
            }
        }

        let final_status = if act == "approve" {
            "approved"
        } else {
            "rejected"
        };
        if let Some(obj) = req.as_object_mut() {
            obj.insert(
                "status".to_string(),
                Value::String(final_status.to_string()),
            );
            obj.insert(
                "reviewed_at".to_string(),
                Value::String(Utc::now().to_rfc3339()),
            );
            obj.insert(
                "review_comment".to_string(),
                Value::String(comment.to_string()),
            );
        }
        self.append_audit_history(&req);
        self.cleanup_staged_request_images(&req_key);

        {
            let _guard = self.pending_lock.lock().expect("pending lock");
            let mut reqs = fs::read_to_string(&self.cfg.pending_file)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            reqs.remove(&req_key);
            self.save_pending_requests_unlocked(&reqs);
        }

        json!({
            "status": final_status,
            "request_id": req_key,
            "identity": identity,
            "type": req_type,
        })
    }

    pub fn tool_rename_identity(
        &self,
        old_identity: &str,
        new_identity: &str,
        admin_key: &str,
    ) -> Value {
        if !self.verify_admin(admin_key) {
            return json!({
                "status": "unauthorized",
                "reason": "Only Administrators can rename identities. Please provide valid admin_key.",
            });
        }
        let old_clean = old_identity.trim();
        let new_clean = new_identity.trim();
        if let Err(e) = self.store.rename_identity(old_clean, new_clean) {
            return json!({ "status": "error", "reason": e });
        }

        let old_slug = profile_slug(old_clean);
        let new_slug = profile_slug(new_clean);
        let old_pdir = self.cfg.profile_dir.join(&old_slug);
        let new_pdir = self.cfg.profile_dir.join(&new_slug);
        if old_pdir.exists() && !old_slug.eq_ignore_ascii_case(&new_slug) {
            if new_pdir.exists() {
                return json!({
                    "status": "conflict",
                    "reason": format!("A profile folder already exists for '{new_clean}'."),
                });
            }
            let _ = fs::rename(&old_pdir, &new_pdir);
        }

        let old_wdir = self.cfg.profile_web_dir.join(&old_slug);
        let new_wdir = self.cfg.profile_web_dir.join(&new_slug);
        if old_wdir.exists() && !old_slug.eq_ignore_ascii_case(&new_slug) {
            if new_wdir.exists() {
                return json!({
                    "status": "conflict",
                    "reason": format!("A web profile folder already exists for '{new_clean}'."),
                });
            }
            let _ = fs::rename(&old_wdir, &new_wdir);
        }

        self.sync_profile_folder(new_clean, None);
        self.clear_identification_cache();
        json!({
            "status": "updated",
            "old_identity": old_clean,
            "new_identity": new_clean,
        })
    }

    pub fn tool_list_identities(&self) -> Value {
        let counts: BTreeMap<String, usize> = self.store.list_counts();
        let total: usize = counts.values().sum();
        let mut by_group: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for name in counts.keys() {
            let slug = profile_slug(name);
            let meta_path = self.cfg.profile_dir.join(&slug).join("metadata.json");
            let grp = fs::read_to_string(&meta_path)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.get("group").and_then(Value::as_str).map(String::from))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Unassigned".to_string());
            by_group.entry(grp).or_default().push(name.clone());
        }
        let groups = list_people_groups(&self.cfg);
        json!({
            "total_vectors": total,
            "identities": counts,
            "people_groups": groups,
            "identities_by_group": by_group,
        })
    }

    pub async fn tool_search_identity(
        &self,
        identity: &str,
        query: &str,
        limit: i64,
        photos_only: bool,
    ) -> Value {
        let known_counts = self.store.list_counts();
        let known_names: Vec<String> = known_counts.keys().cloned().collect();

        let mut requested = identity.trim().to_string();
        let mut effective_query = query.trim().to_string();

        // Recover gracefully if the LLM placed the person's name inside `query` and left `identity` empty
        // (common with colloquial Bahasa Malaysia prompts like "tunjuk 2 gambar Lando Zawawi pakai songkok").
        if requested.is_empty() && !effective_query.is_empty() {
            let q_low = effective_query.to_ascii_lowercase();
            let q_norm = normalize_alnum_lower(&effective_query);
            let mut matched_name: Option<String> = known_names
                .iter()
                .filter(|n| {
                    let nl = n.to_ascii_lowercase();
                    let nn = normalize_alnum_lower(n);
                    q_low.contains(&nl) || (!nn.is_empty() && q_norm.contains(&nn))
                })
                .max_by_key(|n| n.len())
                .cloned();
            if matched_name.is_none() {
                matched_name = Some(effective_query.clone());
            }
            if let Some(name) = matched_name {
                // Strip the matched name from effective_query so leftover words remain as the visual filter.
                let nl = name.to_ascii_lowercase();
                if let Some(pos) = q_low.find(&nl) {
                    let mut rest = String::new();
                    rest.push_str(&effective_query[..pos]);
                    rest.push(' ');
                    rest.push_str(&effective_query[pos + nl.len()..]);
                    effective_query = rest.trim().to_string();
                } else if q_low == nl {
                    effective_query.clear();
                }
                requested = name;
            }
        }

        if requested.is_empty() {
            return json!({ "status": "rejected", "reason": "Provide an identity name." });
        }
        let norm_req = normalize_alnum_lower(&requested);

        let mut canonical = known_names
            .iter()
            .find(|n| n.eq_ignore_ascii_case(&requested))
            .cloned();
        if canonical.is_none() && !norm_req.is_empty() {
            canonical = known_names
                .iter()
                .find(|n| normalize_alnum_lower(n) == norm_req)
                .cloned();
        }
        if canonical.is_none() {
            let req_low = requested.to_ascii_lowercase();
            let partial: Vec<String> = known_names
                .iter()
                .filter(|n| {
                    n.to_ascii_lowercase().contains(&req_low)
                        || (!norm_req.is_empty() && normalize_alnum_lower(n).contains(&norm_req))
                })
                .cloned()
                .collect();
            if partial.len() == 1 {
                canonical = Some(partial[0].clone());
            } else if partial.len() > 1 {
                return json!({
                    "status": "ambiguous",
                    "identity": requested,
                    "matching_identities": partial,
                });
            } else {
                // Handle the reverse case: LLM put "Lando Zawawi pakai songkok" inside `identity`.
                if let Some(found) = known_names
                    .iter()
                    .filter(|n| {
                        let nl = n.to_ascii_lowercase();
                        let nn = normalize_alnum_lower(n);
                        req_low.contains(&nl) || (!nn.is_empty() && norm_req.contains(&nn))
                    })
                    .max_by_key(|n| n.len())
                    .cloned()
                {
                    let nl = found.to_ascii_lowercase();
                    if effective_query.is_empty()
                        && let Some(pos) = req_low.find(&nl)
                    {
                        let mut rest = String::new();
                        rest.push_str(&requested[..pos]);
                        rest.push(' ');
                        rest.push_str(&requested[pos + nl.len()..]);
                        effective_query = rest.trim().to_string();
                    }
                    canonical = Some(found);
                }
            }
        }
        let Some(canonical) = canonical else {
            return json!({
                "status": "not_found",
                "identity": requested,
                "available_identities": known_names,
            });
        };

        if let Some(mut display_profile) = self.public_profile_display(&canonical) {
            let clean_query = effective_query.trim();
            let mut imgs = display_profile["images"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if !clean_query.is_empty() {
                imgs = self
                    .filter_profile_images(&canonical, imgs, clean_query)
                    .await;
            }
            let max_photos = limit.max(0) as usize;
            if max_photos > 0 && imgs.len() > max_photos {
                imgs.truncate(max_photos);
            }
            let matched_count = imgs.len();
            display_profile["images"] = Value::Array(imgs);

            let instruction = if !clean_query.is_empty() && matched_count == 0 {
                format!(
                    "No stored photo for {canonical} matched '{clean_query}'. Tell the user clearly that no matching photo was found in the database."
                )
            } else if photos_only {
                "Render ONLY each image in profile_metadata.images using ![Original image](url), one per line. Do NOT include metadata tables, extra fields, or private data.".to_string()
            } else {
                "If the user specifically asked for photos/images only, render ONLY each image in profile_metadata.images as ![Original image](url) without extra metadata tables. Otherwise, format the response using clean Markdown tables with centered columns: 1) First show a table with columns | Field | Value | (separator | :---: | :---: |) with ONE row for EACH entry in profile_metadata.public_fields (putting entry.field in the Field column and entry.value in the Value column — e.g. | Name | Eula Lawrence |, | test | hamzah |. NEVER print the literal key 'public_other_details' or 'public_fields' as a row!). 2) Then show a table with columns | Image | Description | Photograph by | Location | Date | Time | (separator | :---: | :---: | :---: | :---: | :---: | :---: |) with one row per image in profile_metadata.images, putting ![Original image](url) in the Image column and that image's own Description, Photograph by, Location, Date, and 12-hour AM/PM Time in the other columns. Never show private data, profile folder, or face crops.".to_string()
            };

            return json!({
                "status": "found",
                "identity": canonical,
                "database_only": true,
                "query_applied": if clean_query.is_empty() { Value::Null } else { Value::String(clean_query.to_string()) },
                "limit_applied": if max_photos > 0 { json!(max_photos) } else { Value::Null },
                "photos_only": photos_only,
                "matched_image_count": matched_count,
                "profile_metadata": display_profile,
                "display_instruction": instruction,
            });
        }

        let pts = self.store.points_by_identity(&canonical);
        json!({
            "status": "found",
            "identity": canonical,
            "database_only": true,
            "vector_count": pts.len(),
            "records": pts.iter().map(|p| json!({
                "point_id": p.id,
                "database_payload": { "identity": p.identity, "filename": p.filename, "crop_path": p.crop_path },
            })).collect::<Vec<_>>(),
        })
    }

    pub fn tool_delete_identity(
        &self,
        identity: &str,
        purge_files: bool,
        admin_key: &str,
    ) -> Value {
        if !self.verify_admin(admin_key) {
            return json!({
                "status": "unauthorized",
                "reason": "Only Administrators can delete identities. Please provide valid admin_key.",
            });
        }
        let clean = self.canonical_identity(identity);
        let removed_vectors = match self.store.delete_identity(&clean) {
            Ok(n) => n,
            Err(e) => return json!({ "status": "error", "reason": e }),
        };
        if purge_files {
            let slug = profile_slug(&clean);
            let _ = fs::remove_dir_all(self.cfg.profile_dir.join(&slug));
            let _ = fs::remove_dir_all(self.cfg.profile_web_dir.join(&slug));
        }
        self.clear_identification_cache();
        json!({
            "status": "deleted",
            "identity": clean,
            "removed_vectors": removed_vectors,
        })
    }

    // ─── Admin Portal Helpers ───────────────────────────────────────────────

    pub fn admin_profile_identity(&self, slug: &str) -> Option<String> {
        let clean = slug.trim();
        self.store
            .list_counts()
            .into_keys()
            .find(|identity| profile_slug(identity) == clean)
    }

    pub fn admin_profile_record(&self, identity: &str) -> Value {
        let count = self.store.count_by_identity(identity);
        self.admin_profile_record_with_count(identity, count)
    }

    pub fn admin_profile_record_with_count(&self, identity: &str, vector_count: usize) -> Value {
        let slug = profile_slug(identity);
        let pdir = self.cfg.profile_dir.join(&slug);
        let meta_file = pdir.join("metadata.json");
        let metadata: Map<String, Value> = fs::read_to_string(&meta_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();

        let mut meta_images_by_name: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
        if let Some(arr) = metadata.get("images").and_then(Value::as_array) {
            for item in arr {
                if let Some(obj) = item.as_object() {
                    let pname = obj
                        .get("profile_path")
                        .or_else(|| obj.get("filename"))
                        .and_then(Value::as_str)
                        .and_then(|s| Path::new(s).file_name())
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    if !pname.is_empty() {
                        meta_images_by_name.insert(pname, obj.clone());
                    }
                }
            }
        }

        let mut images = Vec::new();
        let images_dir = pdir.join("images");
        if let Ok(entries) = fs::read_dir(&images_dir) {
            let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for img_path in paths {
                if !img_path.is_file() || !is_image_ext(&img_path) {
                    continue;
                }
                let stem = img_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if stem.contains("_primary") || stem.contains("crop") || stem.contains("face_crop")
                {
                    continue;
                }
                if let Ok(dims) = image::image_dimensions(&img_path)
                    && dims.0.max(dims.1) <= 160
                {
                    continue;
                }
                let fname = img_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let img_meta = meta_images_by_name.get(&fname);
                let field = |k: &str| -> String {
                    img_meta
                        .and_then(|m| m.get(k))
                        .or_else(|| metadata.get(k))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                };
                images.push(json!({
                    "filename": fname,
                    "url": format!("/admin/profile-images/{slug}/{fname}"),
                    "description": field("description"),
                    "photograph_by": field("photograph_by"),
                    "location": field("location"),
                    "date": field("date"),
                    "time": field("time"),
                }));
            }
        }

        let mut raw_notes = metadata
            .get("notes")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if raw_notes.starts_with("Only database-backed fields are listed.") {
            raw_notes.clear();
        }
        let get_str = |k: &str| -> String {
            metadata
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };

        json!({
            "slug": slug,
            "identity": identity,
            "group": get_str("group"),
            "group_slug": get_str("group_slug"),
            "description": get_str("description"),
            "photograph_by": get_str("photograph_by"),
            "location": get_str("location"),
            "date": get_str("date"),
            "time": get_str("time"),
            "address": get_str("address"),
            "notes": raw_notes,
            "other_details": Value::Object(normalize_other_details(metadata.get("other_details"))),
            "vector_count": vector_count,
            "images": images,
        })
    }

    pub fn admin_add_profile_image(&self, identity: &str, image_path: &Path) -> Value {
        let clean = self.canonical_identity(identity);
        if clean.is_empty() || !image_path.is_file() {
            return json!({ "status": "rejected", "reason": "Identity and image are required." });
        }
        if let Ok(existing) = self.recognize_path(image_path, MIN_CLIP_IDENTIFY_THRESHOLD) {
            let known: HashSet<String> = existing
                .iter()
                .filter_map(|m| m.get("identity").and_then(Value::as_str))
                .filter(|&id| id != "Unknown")
                .map(String::from)
                .collect();
            if !known.is_empty() && !known.contains(&clean) {
                let mut sorted: Vec<String> = known.into_iter().collect();
                sorted.sort();
                return json!({
                    "status": "rejected",
                    "reason": "The image matches a different enrolled identity.",
                    "matches": sorted,
                });
            }
        }
        let (primary_bytes, primary_fname) = match self.engine.primary_face_bytes(image_path) {
            Ok(x) => x,
            Err(reason) => return json!({ "status": "error", "reason": reason }),
        };
        // Use a high deduplication threshold (0.995) because cross-identity conflict was already checked above;
        // this guarantees every distinct face angle/photo added by an Admin stores a new 768-D vector.
        let stats = match self.learn_bytes(&primary_bytes, 0.995, Some(&primary_fname)) {
            Ok(s) => s,
            Err(reason) => return json!({ "status": "error", "reason": reason }),
        };
        let mut vectors_added = 0usize;
        if let Some(arr) = stats.get("details").and_then(Value::as_array) {
            for d in arr {
                if d.get("status").and_then(Value::as_str) == Some("new")
                    && let Some(gen_id) = d.get("identity").and_then(Value::as_str)
                {
                    let _ = self.store.rename_identity(gen_id, &clean);
                    vectors_added += 1;
                }
            }
        }
        self.sync_profile_folder(&clean, Some(image_path));
        self.clear_identification_cache();
        let total_vectors = self.store.count_by_identity(&clean);
        json!({
            "status": "image_added",
            "identity": clean,
            "vectors_added": vectors_added,
            "vector_count": total_vectors,
            "details": stats["details"],
        })
    }

    /// Pure-Rust batch enrollment from `<root>/<Identity Name>/*.jpg` directories.
    pub fn enroll_directory(&self, dir: &Path) -> Result<Value, String> {
        if !dir.is_dir() {
            return Err(format!("Directory not found: {}", dir.display()));
        }
        let mut enrolled = 0usize;
        let mut skipped = 0usize;
        let mut errors = Vec::new();
        let mut subdirs: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|e| format!("Failed reading {}: {e}", dir.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();

        for person_dir in subdirs {
            let Some(identity) = person_dir.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let mut imgs: Vec<PathBuf> = fs::read_dir(&person_dir)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && is_image_ext(p))
                .collect();
            imgs.sort();
            for img_path in imgs {
                let res = self.tool_enroll_named_identity(
                    identity,
                    &img_path.to_string_lossy(),
                    &self.cfg.admin_key,
                    "",
                );
                match res.get("status").and_then(Value::as_str).unwrap_or("") {
                    "enrolled_by_admin" => enrolled += 1,
                    "unchanged" | "rejected" => skipped += 1,
                    _ => errors.push(json!({
                        "identity": identity,
                        "file": img_path.display().to_string(),
                        "result": res,
                    })),
                }
            }
        }
        Ok(json!({
            "status": "completed",
            "enrolled": enrolled,
            "skipped": skipped,
            "errors": errors,
        }))
    }
}

/// Tokenizes an English or Bahasa Melayu visual search query, strips bilingual stopwords,
/// and expands Malay phrases/words into English+Malay synonym groups for instant visual_index matching.
fn expand_bilingual_query_groups(query: &str) -> Vec<Vec<String>> {
    const STOPWORDS: &[&str] = &[
        // English
        "the",
        "a",
        "an",
        "that",
        "this",
        "with",
        "in",
        "on",
        "of",
        "is",
        "are",
        "photo",
        "photos",
        "image",
        "images",
        "picture",
        "pictures",
        "show",
        "only",
        "his",
        "her",
        "their",
        "wearing",
        "wear",
        "man",
        "woman",
        "person",
        "holding",
        "hold",
        // Bahasa Melayu (Formal + Colloquial / Bahasa Pasar)
        "yang",
        "pakai",
        "memakai",
        "berpakaian",
        "gambar",
        "foto",
        "tunjuk",
        "tunjukkan",
        "papar",
        "paparkan",
        "cari",
        "carikan",
        "tolong",
        "bagi",
        "cek",
        "cekkan",
        "semak",
        "sahaja",
        "saja",
        "je",
        "jer",
        "ja",
        "ni",
        "tu",
        "la",
        "lah",
        "jap",
        "kejap",
        "nak",
        "mahu",
        "hendak",
        "dengan",
        "di",
        "dalam",
        "pada",
        "untuk",
        "tentang",
        "mengenai",
        "lelaki",
        "perempuan",
        "wanita",
        "orang",
        "dia",
        "beliau",
        "ini",
        "itu",
        "ada",
        "sedang",
        "sila",
        "boleh",
        "pegang",
        "memegang",
        "berwarna",
        "warna",
    ];
    const PHRASES: &[(&str, &[&str])] = &[
        (
            "cermin mata",
            &["cermin mata", "glasses", "spectacles", "eyewear"],
        ),
        (
            "bercermin mata",
            &["cermin mata", "glasses", "spectacles", "eyewear"],
        ),
        (
            "kaca mata",
            &["kaca mata", "glasses", "spectacles", "eyewear"],
        ),
        (
            "berkaca mata",
            &["kaca mata", "glasses", "spectacles", "eyewear"],
        ),
        ("baju petak", &["baju petak", "plaid", "checkered"]),
        ("baju berkotak", &["baju berkotak", "plaid", "checkered"]),
        ("topi lanun", &["topi lanun", "pirate"]),
        ("tali leher", &["tali leher", "tie", "necktie"]),
        ("baju melayu", &["baju melayu", "traditional", "malay"]),
        ("baju kurung", &["baju kurung", "traditional", "kurung"]),
        ("baju batik", &["batik", "shirt"]),
        ("rambut biru", &["rambut biru", "blue hair"]),
        ("luar bangunan", &["outdoor", "outside"]),
        ("dalam bangunan", &["indoor", "inside"]),
    ];
    const SYNONYMS: &[(&str, &[&str])] = &[
        ("spek", &["spek", "glasses", "spectacles", "eyewear"]),
        ("bercermin", &["bercermin", "glasses", "spectacles"]),
        ("songkok", &["songkok", "peci", "cap"]),
        ("bersongkok", &["songkok", "peci", "cap"]),
        ("kopiah", &["kopiah", "skullcap", "taqiyah", "cap"]),
        ("berkopiah", &["kopiah", "skullcap", "taqiyah", "cap"]),
        (
            "tanjak",
            &["tanjak", "tengkolok", "headgear", "traditional"],
        ),
        ("tengkolok", &["tengkolok", "tanjak", "headgear"]),
        ("serban", &["serban", "turban"]),
        ("tudung", &["tudung", "hijab", "headscarf"]),
        ("bertudung", &["tudung", "hijab", "headscarf"]),
        ("selendang", &["selendang", "shawl", "scarf"]),
        ("sampin", &["sampin", "samping", "traditional"]),
        ("kebaya", &["kebaya", "traditional"]),
        ("topi", &["topi", "hat", "cap"]),
        ("lanun", &["lanun", "pirate"]),
        ("petak", &["petak", "plaid", "checkered"]),
        ("berkotak", &["berkotak", "plaid", "checkered"]),
        ("kotak", &["kotak", "plaid", "checkered"]),
        ("baju", &["baju", "shirt", "attire", "top"]),
        ("kemeja", &["kemeja", "shirt", "collared"]),
        ("kot", &["kot", "suit", "blazer", "jacket", "coat"]),
        ("berkot", &["kot", "suit", "blazer", "jacket"]),
        ("sut", &["sut", "suit", "blazer"]),
        ("bersut", &["sut", "suit", "blazer"]),
        ("jaket", &["jaket", "jacket"]),
        ("batik", &["batik"]),
        ("seragam", &["seragam", "uniform"]),
        ("berseragam", &["seragam", "uniform"]),
        ("tentera", &["tentera", "military", "uniform"]),
        ("mikrofon", &["mikrofon", "microphone", "mic"]),
        ("pentas", &["pentas", "stage", "podium"]),
        ("podium", &["podium", "stage", "lectern"]),
        ("bendera", &["bendera", "flag"]),
        ("berucap", &["speaking", "speech", "microphone", "stage"]),
        ("bercakap", &["speaking", "talking", "microphone"]),
        ("berdiri", &["standing"]),
        ("duduk", &["sitting", "seated"]),
        ("janggut", &["janggut", "beard"]),
        ("berjanggut", &["janggut", "beard"]),
        ("misai", &["misai", "mustache", "moustache"]),
        ("bermisai", &["misai", "mustache", "moustache"]),
        ("senyum", &["senyum", "smiling", "smile"]),
        ("tersenyum", &["senyum", "smiling", "smile"]),
        ("ketawa", &["laughing", "smiling", "smile"]),
        ("hitam", &["hitam", "black"]),
        ("putih", &["putih", "white"]),
        ("biru", &["biru", "blue"]),
        ("hijau", &["hijau", "green"]),
        ("merah", &["merah", "red"]),
        ("kuning", &["kuning", "yellow"]),
        ("kelabu", &["kelabu", "gray", "grey"]),
        ("coklat", &["coklat", "brown"]),
        ("emas", &["emas", "gold", "golden"]),
        ("ungu", &["ungu", "purple"]),
    ];

    static TOK_RE: OnceLock<Regex> = OnceLock::new();
    let tok_re = TOK_RE.get_or_init(|| Regex::new(r"[a-z0-9]+").expect("regex"));

    let mut remaining = query.to_ascii_lowercase();
    let mut groups: Vec<Vec<String>> = Vec::new();
    for &(phrase, syns) in PHRASES {
        if remaining.contains(phrase) {
            groups.push(syns.iter().map(|s| (*s).to_string()).collect());
            remaining = remaining.replace(phrase, " ");
        }
    }

    for m in tok_re.find_iter(&remaining) {
        let tok = m.as_str();
        if STOPWORDS.contains(&tok) {
            continue;
        }
        if let Some(&(_, syns)) = SYNONYMS.iter().find(|&&(k, _)| k == tok) {
            groups.push(syns.iter().map(|s| (*s).to_string()).collect());
        } else {
            groups.push(vec![tok.to_string()]);
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches_caption(query: &str, caption: &str) -> bool {
        let groups = expand_bilingual_query_groups(query);
        let hay = caption.to_ascii_lowercase();
        !groups.is_empty()
            && groups
                .iter()
                .all(|g| g.iter().any(|syn| hay.contains(syn.as_str())))
    }

    #[test]
    fn test_bilingual_malay_english_visual_search() {
        assert!(matches_caption(
            "tunjuk gambar yang pakai cermin mata",
            "Close-up portrait of a man wearing glasses indoors"
        ));
        assert!(matches_caption(
            "pakai baju petak",
            "Smiling man in a red plaid shirt"
        ));
        assert!(matches_caption(
            "topi lanun",
            "Outdoor photo wearing a black pirate hat"
        ));
        assert!(matches_caption(
            "pakai songkok hitam",
            "Formal portrait wearing a black songkok and suit"
        ));
        assert!(matches_caption(
            "memegang mikrofon",
            "Speaker holding a microphone on stage"
        ));
        assert!(matches_caption(
            "tolong tunjuk gambar dia bersongkok hitam je",
            "Formal portrait wearing a black songkok"
        ));
        assert!(matches_caption(
            "carikan gambar yang bertudung biru",
            "Portrait of a woman wearing a blue hijab"
        ));
        assert!(matches_caption(
            "gambar berucap di pentas",
            "Speaker giving a speech on stage with a microphone"
        ));
        assert!(!matches_caption(
            "pakai cermin mata",
            "Portrait wearing a white kopiah"
        ));
    }
}
