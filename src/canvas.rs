#![forbid(unsafe_code)]

//! Split-Screen "Arkib Canvas" Backend API (`/canvas/api/*`).
//!
//! Serves deterministic Jawi OCR Workbench (`ocr`) and Identity Enrollment (`enroll`)
//! sessions for the LibreChat-embedded right-side drawer with zero extra LLM token cost
//! and strict Two-Tier Role Governance (Guest -> staged `REQ-*`, Admin -> direct commit).

use crate::admin::{AdminState, cors_middleware, is_admin_request};
use crate::archives;
use crate::config::AppConfig;
use crate::export::{ExportDocumentInput, build_docx_bytes, build_pdf_bytes};
use crate::jawi::decode_jawi_text;
use crate::mcp_sse::is_allowed_host;
use crate::metadata::{default_detail_visibility, profile_slug, value_to_trimmed_string};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use uuid::Uuid;

const MAX_CANVAS_IMAGE_BYTES: usize = 10 * 1024 * 1024; // 10 MB

pub fn canvas_sessions_dir(cfg: &AppConfig) -> PathBuf {
    cfg.project_root
        .join("data")
        .join("interim")
        .join("canvas_sessions")
}

fn is_safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Atomically persists a Canvas session snapshot (`ocr_<id>.json` or `enroll_<id>.json`)
/// and returns its short session ID.
pub fn save_canvas_session(
    cfg: &AppConfig,
    kind: &str,
    mut payload: Map<String, Value>,
) -> Result<String, String> {
    let dir = canvas_sessions_dir(cfg);
    fs::create_dir_all(&dir).map_err(|e| format!("Create canvas_sessions dir failed: {e}"))?;
    let id = Uuid::new_v4().simple().to_string()[..12].to_string();
    payload.insert("session_id".to_string(), Value::String(id.clone()));
    payload.insert("kind".to_string(), Value::String(kind.to_string()));
    payload.insert(
        "created_at".to_string(),
        Value::String(Utc::now().to_rfc3339()),
    );
    let target = dir.join(format!("{kind}_{id}.json"));
    let tmp = dir.join(format!("{kind}_{id}.json.tmp"));
    let bytes = serde_json::to_vec_pretty(&Value::Object(payload))
        .map_err(|e| format!("Serialize canvas session failed: {e}"))?;
    fs::write(&tmp, bytes).map_err(|e| format!("Write canvas session tmp failed: {e}"))?;
    fs::rename(&tmp, &target).map_err(|e| format!("Rename canvas session failed: {e}"))?;
    Ok(id)
}

/// Removes stale Canvas session files older than `max_age_secs` (default 24 hours).
pub fn cleanup_stale_canvas_sessions(cfg: &AppConfig, max_age_secs: u64) -> (usize, u64) {
    let dir = canvas_sessions_dir(cfg);
    let mut removed = 0usize;
    let mut freed = 0u64;
    let now = SystemTime::now();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            if let Ok(meta) = fs::metadata(&p) {
                let age = meta
                    .modified()
                    .ok()
                    .and_then(|m| now.duration_since(m).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if age >= max_age_secs && fs::remove_file(&p).is_ok() {
                    removed += 1;
                    freed += meta.len();
                }
            }
        }
    }
    (removed, freed)
}

async fn host_guard_middleware(req: Request, next: Next) -> Response {
    if let Some(host) = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        && !is_allowed_host(host)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "status": "error", "reason": "Forbidden Host header." })),
        )
            .into_response();
    }
    next.run(req).await
}

pub fn canvas_router(state: AdminState) -> Router {
    Router::new()
        .route("/canvas/api/session/{kind}/{id}", get(canvas_get_session))
        .route("/canvas/api/image/{id}", get(canvas_serve_image))
        .route("/canvas/api/jawi-preview", post(canvas_jawi_preview))
        .route(
            "/canvas/api/export-transcription/{format}",
            post(canvas_export_transcription),
        )
        .route("/canvas/api/submit-ocr-edit", post(canvas_submit_ocr_edit))
        .route(
            "/canvas/api/submit-enrollment",
            post(canvas_submit_enrollment),
        )
        .route("/canvas/api/track-chat-query", post(canvas_track_chat_query))
        .layer(DefaultBodyLimit::max(14 * 1024 * 1024))
        .layer(middleware::from_fn(host_guard_middleware))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            cors_middleware,
        ))
        .with_state(state)
}

fn load_latest_session_of_kind(
    cfg: &AppConfig,
    kind: &str,
) -> Option<(String, Map<String, Value>)> {
    let dir = canvas_sessions_dir(cfg);
    let prefix = format!("{kind}_");
    let mut best: Option<(SystemTime, String, PathBuf)> = None;
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !fname.starts_with(&prefix) || !fname.ends_with(".json") {
                continue;
            }
            let id = fname
                .trim_start_matches(&prefix)
                .trim_end_matches(".json")
                .to_string();
            let modified = fs::metadata(&p)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(t, _, _)| modified > *t) {
                best = Some((modified, id, p));
            }
        }
    }
    let (_, id, path) = best?;
    let raw = fs::read_to_string(&path).ok()?;
    let map = serde_json::from_str::<Value>(&raw)
        .ok()?
        .as_object()
        .cloned()?;
    Some((id, map))
}

async fn canvas_get_session(
    State(state): State<AdminState>,
    headers: HeaderMap,
    AxumPath((kind, id)): AxumPath<(String, String)>,
) -> Response {
    if kind != "ocr" && kind != "enroll" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Canvas mode must be 'ocr' or 'enroll'." })),
        )
            .into_response();
    }
    if !is_safe_id(&id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Invalid session ID." })),
        )
            .into_response();
    }

    let is_admin = is_admin_request(&headers, &state.sessions);
    let svc = state.service.clone();

    let payload = tokio::task::spawn_blocking(move || {
        let dir = canvas_sessions_dir(&svc.cfg);
        let (resolved_id, session_map) = if id == "latest" {
            load_latest_session_of_kind(&svc.cfg, &kind)
                .unwrap_or_else(|| ("new".to_string(), Map::new()))
        } else if id == "new" {
            ("new".to_string(), Map::new())
        } else {
            let path = dir.join(format!("{kind}_{id}.json"));
            let map = fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            (id.clone(), map)
        };

        if kind == "ocr" {
            let raw_jawi = session_map
                .get("raw_jawi_text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let decoded = if !raw_jawi.trim().is_empty() {
                decode_jawi_text(raw_jawi.clone()).unwrap_or_else(|_| json!({}))
            } else {
                json!({
                    "corrected_jawi_lines": [],
                    "standard_malay_rumi_lines": [],
                    "standard_malay_rumi_text": "",
                    "coverage_pct": 100.0,
                    "script_style": "modern_jawi",
                    "hijri_colophon_dates": [],
                })
            };
            let has_image = session_map
                .get("source_image_path")
                .and_then(Value::as_str)
                .is_some_and(|p| Path::new(p).is_file());
            let image_url = if has_image && resolved_id != "new" {
                Value::String(format!("/canvas/api/image/ocr_{resolved_id}"))
            } else {
                Value::Null
            };
            // Public document list (slug + title only) so admin/guest can link an edit to a document.
            let docs: Vec<Value> = archives::list_documents(&svc.cfg)
                .into_iter()
                .map(|d| {
                    json!({
                        "slug": d.get("slug").and_then(Value::as_str).unwrap_or(""),
                        "title": d.get("title").and_then(Value::as_str).unwrap_or(""),
                        "reference_code": d.get("reference_code").and_then(Value::as_str).unwrap_or(""),
                    })
                })
                .collect();

            json!({
                "status": "ok",
                "kind": "ocr",
                "session_id": resolved_id,
                "is_admin": is_admin,
                "file": session_map.get("file").cloned().unwrap_or_else(|| json!("")),
                "document_slug": session_map.get("document_slug").cloned().unwrap_or(Value::Null),
                "image_url": image_url,
                "image_dimensions": session_map.get("image_dimensions").cloned().unwrap_or(Value::Null),
                "has_rubrication": session_map.get("has_rubrication").and_then(Value::as_bool).unwrap_or(false),
                "chunks": session_map.get("chunks").cloned().unwrap_or_else(|| json!([])),
                "raw_jawi_text": raw_jawi,
                "corrected_jawi_lines": decoded["corrected_jawi_lines"],
                "standard_malay_rumi_lines": decoded["standard_malay_rumi_lines"],
                "standard_malay_rumi_text": decoded["standard_malay_rumi_text"],
                "coverage_pct": decoded["coverage_pct"],
                "script_style": decoded["script_style"],
                "hijri_colophon_dates": decoded["hijri_colophon_dates"],
                "documents": docs,
            })
        } else {
            // Enroll mode: return ONLY public display names, group folders, and field schema hints (NEVER private values!).
            let has_image = session_map
                .get("source_image_path")
                .and_then(Value::as_str)
                .is_some_and(|p| Path::new(p).is_file());
            let image_url = if has_image && resolved_id != "new" {
                Value::String(format!("/canvas/api/image/enroll_{resolved_id}"))
            } else {
                Value::Null
            };
            let existing_identities: Vec<String> = svc.store.list_counts().into_keys().collect();
            let people_groups: Vec<Value> = archives::list_people_groups(&svc.cfg)
                .into_iter()
                .map(|g| {
                    json!({
                        "slug": g.get("slug").and_then(Value::as_str).unwrap_or(""),
                        "name": g.get("name").and_then(Value::as_str).unwrap_or(""),
                    })
                })
                .collect();

            json!({
                "status": "ok",
                "kind": "enroll",
                "session_id": resolved_id,
                "is_admin": is_admin,
                "file": session_map.get("file").cloned().unwrap_or_else(|| json!("")),
                "image_url": image_url,
                "image_dimensions": session_map.get("image_dimensions").cloned().unwrap_or(Value::Null),
                "face_box_xywh": session_map.get("face_box_xywh").cloned().unwrap_or(Value::Null),
                "matched_identities": session_map.get("matched_identities").cloned().unwrap_or_else(|| json!([])),
                "suggested_identity": session_map.get("suggested_identity").cloned().unwrap_or_else(|| json!("")),
                "existing_identities": existing_identities,
                "people_groups": people_groups,
                "public_field_hints": ["description", "location", "photograph_by", "date", "notes", "role", "department"],
                "private_field_hints": ["no_kp", "phone", "email", "dob", "address", "passport"],
            })
        }
    })
    .await
    .unwrap_or_else(|e| json!({ "status": "error", "reason": e.to_string() }));

    (StatusCode::OK, Json(payload)).into_response()
}

async fn canvas_serve_image(
    State(state): State<AdminState>,
    AxumPath(session_key): AxumPath<String>,
) -> Response {
    if !is_safe_id(&session_key) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let session_file = canvas_sessions_dir(&state.service.cfg).join(format!("{session_key}.json"));
    let Ok(raw) = fs::read_to_string(&session_file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(img_path_str) = val.get("source_image_path").and_then(Value::as_str) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let img_path = PathBuf::from(img_path_str);
    if !img_path.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    // Enforce canonical path boundary (must reside inside project_root or librechat_dir).
    let Ok(canon) = img_path.canonicalize() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let allowed_roots = [
        state.service.cfg.project_root.canonicalize().ok(),
        state.service.cfg.librechat_dir.canonicalize().ok(),
    ];
    let within_allowed = allowed_roots
        .iter()
        .flatten()
        .any(|root| canon.starts_with(root));
    if !within_allowed {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(bytes) = fs::read(&canon) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(&canon).first_or_octet_stream();
    let mut resp = Response::new(Body::from(bytes));
    if let Ok(hv) = HeaderValue::from_str(mime.as_ref()) {
        resp.headers_mut().insert(header::CONTENT_TYPE, hv);
    }
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=60"),
    );
    resp
}

#[derive(Deserialize)]
struct JawiPreviewReq {
    jawi_text: String,
}

async fn canvas_jawi_preview(Json(payload): Json<JawiPreviewReq>) -> Response {
    match decode_jawi_text(payload.jawi_text) {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(reason) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
    }
}

#[derive(Clone, Debug, Deserialize, serde::Serialize)]
pub struct LexiconPair {
    pub jawi: String,
    pub rumi: String,
}

#[derive(Deserialize)]
struct SubmitOcrEditReq {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    document_slug: Option<String>,
    #[serde(default)]
    document_title: Option<String>,
    #[serde(default)]
    edited_jawi_text: String,
    #[serde(default)]
    edited_rumi_text: String,
    #[serde(default)]
    lexicon_suggestions: Vec<LexiconPair>,
    #[serde(default)]
    notes: Option<String>,
}

/// Deduplicated, atomic append of verified Jawi–Rumi word pairs to `data/lexicon/dbp_jawi_rumi.tsv`.
pub fn append_verified_lexicon_pairs(
    cfg: &AppConfig,
    pairs: &[LexiconPair],
) -> Result<usize, String> {
    let clean_pairs: Vec<(String, String)> = pairs
        .iter()
        .filter_map(|p| {
            let j = p.jawi.trim().replace(['\t', '\r', '\n'], "");
            let r = p.rumi.trim().replace(['\t', '\r', '\n'], "");
            if j.is_empty() || r.is_empty() {
                None
            } else {
                Some((j, r))
            }
        })
        .collect();
    if clean_pairs.is_empty() {
        return Ok(0);
    }

    let lexicon_path = cfg
        .project_root
        .join("data")
        .join("lexicon")
        .join("dbp_jawi_rumi.tsv");
    if let Some(parent) = lexicon_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Create lexicon dir failed: {e}"))?;
    }

    let existing_raw = fs::read_to_string(&lexicon_path).unwrap_or_default();
    let mut seen_keys: HashSet<String> = HashSet::new();
    for line in existing_raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((first, _)) = trimmed.split_once('\t') {
            seen_keys.insert(crate::jawi::normalize_jawi_key(first.trim()));
        }
    }

    let mut additions = Vec::new();
    for (j, r) in clean_pairs {
        let key = crate::jawi::normalize_jawi_key(&j);
        if !key.is_empty() && seen_keys.insert(key) {
            additions.push(format!("{j}\t{j}\t{r}"));
        }
    }
    if additions.is_empty() {
        return Ok(0);
    }

    let mut updated = existing_raw;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    for row in &additions {
        updated.push_str(row);
        updated.push('\n');
    }

    let tmp = lexicon_path.with_extension("tsv.tmp");
    fs::write(&tmp, updated.as_bytes()).map_err(|e| format!("Write lexicon tmp failed: {e}"))?;
    fs::rename(&tmp, &lexicon_path).map_err(|e| format!("Rename lexicon failed: {e}"))?;
    Ok(additions.len())
}

/// Applies an approved/direct OCR edit atomically to `data/processed/documents/<slug>/metadata.json`
/// (creating a verified archival document if `document_slug` is omitted) and optionally appends
/// verified lexicon pairs to `data/lexicon/dbp_jawi_rumi.tsv`.
#[allow(clippy::too_many_arguments)]
pub fn apply_ocr_edit_to_archive(
    cfg: &AppConfig,
    document_slug: Option<&str>,
    document_title: Option<&str>,
    edited_jawi: &str,
    edited_rumi: &str,
    notes: &str,
    lexicon_suggestions: &[LexiconPair],
    add_to_lexicon: bool,
) -> Result<Value, String> {
    let target_slug = document_slug
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            let base_title = document_title
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("Verified Jawi Transcription");
            let s = profile_slug(base_title);
            if s.is_empty() {
                format!("jawi_{}", &Uuid::new_v4().simple().to_string()[..8])
            } else {
                s
            }
        });

    let doc_dir = cfg.documents_dir.join(&target_slug);
    fs::create_dir_all(doc_dir.join("files"))
        .map_err(|e| format!("Create document dir failed: {e}"))?;
    let meta_path = doc_dir.join("metadata.json");
    let before_meta: Map<String, Value> = fs::read_to_string(&meta_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    let had_existing = !before_meta.is_empty();
    let mut meta = before_meta.clone();

    meta.entry("slug".to_string())
        .or_insert_with(|| Value::String(target_slug.clone()));
    if meta
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        let title = document_title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Verified Jawi Transcription");
        meta.insert("title".to_string(), Value::String(title.to_string()));
    }
    meta.insert("script_type".to_string(), Value::String("jawi".to_string()));
    meta.insert(
        "jawi_text".to_string(),
        Value::String(edited_jawi.to_string()),
    );
    let combined_text = if edited_rumi.trim().is_empty() {
        edited_jawi.to_string()
    } else {
        edited_rumi.to_string()
    };
    meta.insert(
        "extracted_text".to_string(),
        Value::String(combined_text.clone()),
    );
    meta.insert("verified_by_human".to_string(), Value::Bool(true));
    if !notes.trim().is_empty() {
        meta.insert("notes".to_string(), Value::String(notes.trim().to_string()));
    }
    let structured = archives::extract_document_structure(&combined_text);
    if let Some(h) = structured.get("hijri_date").and_then(Value::as_str)
        && !h.is_empty()
    {
        meta.insert("hijri_date".to_string(), Value::String(h.to_string()));
    }
    if let Some(g) = structured
        .get("gregorian_date_masihi")
        .and_then(Value::as_str)
        && !g.is_empty()
    {
        meta.insert(
            "gregorian_date_masihi".to_string(),
            Value::String(g.to_string()),
        );
    }
    meta.insert(
        "updated_at".to_string(),
        Value::String(Utc::now().to_rfc3339()),
    );
    meta.entry("created_at".to_string())
        .or_insert_with(|| Value::String(Utc::now().to_rfc3339()));

    let pretty = serde_json::to_string_pretty(&Value::Object(meta.clone()))
        .map_err(|e| format!("Serialize document.json failed: {e}"))?;
    let tmp = meta_path.with_extension("json.tmp");
    fs::write(&tmp, pretty).map_err(|e| format!("Write document tmp failed: {e}"))?;
    fs::rename(&tmp, &meta_path).map_err(|e| format!("Rename document failed: {e}"))?;

    let title_str = meta
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(&target_slug);
    if had_existing {
        let _ = crate::revisions::record_metadata_change(
            cfg,
            "document",
            &target_slug,
            title_str,
            &cfg.admin_user,
            "canvas_ocr_edit",
            &before_meta,
            &meta,
        );
    } else {
        let _ = crate::revisions::record_lifecycle_event(
            cfg,
            "document",
            &target_slug,
            title_str,
            &cfg.admin_user,
            "created",
            None,
            Some(&meta),
        );
    }

    // Automatically write a downloadable .docx file into the document folder so it is immediately usable (never 0 files).
    let export_in = ExportDocumentInput::from_doc_value(&Value::Object(meta));
    if let Ok(docx_bytes) = build_docx_bytes(&export_in) {
        let docx_path = doc_dir.join("files").join(format!("{target_slug}.docx"));
        let docx_tmp = docx_path.with_extension("docx.tmp");
        if fs::write(&docx_tmp, docx_bytes).is_ok() {
            let _ = fs::rename(&docx_tmp, &docx_path);
        }
    }

    let lexicon_added = if add_to_lexicon {
        append_verified_lexicon_pairs(cfg, lexicon_suggestions)?
    } else {
        0
    };

    Ok(json!({
        "status": "saved_by_admin",
        "document_slug": target_slug,
        "verified_by_human": true,
        "lexicon_added": lexicon_added,
    }))
}

#[derive(Deserialize)]
struct ExportTranscriptionReq {
    #[serde(default)]
    title: String,
    #[serde(default)]
    jawi_text: String,
    #[serde(default)]
    rumi_text: String,
    #[serde(default)]
    notes: String,
}

async fn canvas_export_transcription(
    AxumPath(format): AxumPath<String>,
    Json(payload): Json<ExportTranscriptionReq>,
) -> Response {
    let fmt = format.trim().to_ascii_lowercase();
    if fmt != "docx" && fmt != "pdf" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Format must be 'docx' or 'pdf'." })),
        )
            .into_response();
    }
    let title = if payload.title.trim().is_empty() {
        "Jawi Archival Transcription".to_string()
    } else {
        payload.title.trim().to_string()
    };
    let structured = archives::extract_document_structure(if payload.rumi_text.trim().is_empty() {
        &payload.jawi_text
    } else {
        &payload.rumi_text
    });
    let doc_input = ExportDocumentInput {
        title: title.clone(),
        reference_code: String::new(),
        group: String::new(),
        script_type: "jawi".to_string(),
        verified_by_human: false,
        hijri_date: structured["hijri_date"].as_str().unwrap_or("").to_string(),
        gregorian_date_masihi: structured["gregorian_date_masihi"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        jawi_text: payload.jawi_text,
        rumi_text: payload.rumi_text,
        summary: String::new(),
        notes: payload.notes,
    };
    let slug = {
        let s = profile_slug(&title);
        if s.is_empty() {
            "jawi_transcription".to_string()
        } else {
            s
        }
    };
    let built = if fmt == "docx" {
        build_docx_bytes(&doc_input).map(|b| {
            (
                b,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                format!("{slug}.docx"),
            )
        })
    } else {
        build_pdf_bytes(&doc_input).map(|b| (b, "application/pdf", format!("{slug}.pdf")))
    };
    match built {
        Ok((bytes, mime, fname)) => {
            let mut resp = Response::new(Body::from(bytes));
            if let Ok(hv) = HeaderValue::from_str(mime) {
                resp.headers_mut().insert(header::CONTENT_TYPE, hv);
            }
            if let Ok(cd) = HeaderValue::from_str(&format!("attachment; filename=\"{fname}\"")) {
                resp.headers_mut().insert(header::CONTENT_DISPOSITION, cd);
            }
            resp
        }
        Err(reason) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
    }
}

async fn canvas_submit_ocr_edit(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(payload): Json<SubmitOcrEditReq>,
) -> Response {
    if payload.edited_jawi_text.trim().is_empty() && payload.edited_rumi_text.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "error",
                "reason": "Provide edited Jawi or Rumi text before submitting.",
            })),
        )
            .into_response();
    }

    let is_admin = is_admin_request(&headers, &state.sessions);
    let svc = state.service.clone();

    let (code, body) = tokio::task::spawn_blocking(move || {
        let notes = payload.notes.unwrap_or_default();
        if is_admin {
            match apply_ocr_edit_to_archive(
                &svc.cfg,
                payload.document_slug.as_deref(),
                payload.document_title.as_deref(),
                &payload.edited_jawi_text,
                &payload.edited_rumi_text,
                &notes,
                &payload.lexicon_suggestions,
                true,
            ) {
                Ok(val) => (StatusCode::OK, val),
                Err(reason) => (
                    StatusCode::BAD_REQUEST,
                    json!({ "status": "error", "reason": reason }),
                ),
            }
        } else {
            let res = svc.stage_ocr_correction_request(
                &payload.session_id,
                payload.document_slug.as_deref(),
                payload.document_title.as_deref(),
                &payload.edited_jawi_text,
                &payload.edited_rumi_text,
                &payload.lexicon_suggestions,
                &notes,
            );
            (StatusCode::OK, res)
        }
    })
    .await
    .unwrap_or_else(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "status": "error", "reason": e.to_string() }),
        )
    });

    (code, Json(body)).into_response()
}

#[derive(Deserialize)]
struct SubmitEnrollmentReq {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    image_base64: String,
    #[serde(default)]
    identity_name: String,
    #[serde(default)]
    existing_identity: Option<String>,
    #[serde(default)]
    group_folder: String,
    #[serde(default)]
    public_fields: Map<String, Value>,
    #[serde(default)]
    private_fields: Map<String, Value>,
    #[serde(default)]
    notes: String,
}

/// Classifies submitted Canvas enrollment fields using `src/metadata.rs::default_detail_visibility`
/// so sensitive keys (`ic`, `nric`, `no_kp`, `phone`, `email`, `dob`, `passport`, etc.) are
/// ALWAYS routed into private storage even if submitted inside `public_fields`.
pub fn build_classified_proposed_metadata(
    cfg: &AppConfig,
    group_folder: &str,
    public_fields: &Map<String, Value>,
    private_fields: &Map<String, Value>,
    notes: &str,
) -> Map<String, Value> {
    let mut proposed = Map::new();
    let mut other_details = Map::new();

    if !group_folder.trim().is_empty() {
        if let Some((gs, gn)) = archives::resolve_people_group(cfg, group_folder) {
            proposed.insert("group_slug".to_string(), Value::String(gs));
            proposed.insert("group".to_string(), Value::String(gn));
        } else {
            proposed.insert(
                "group".to_string(),
                Value::String(group_folder.trim().to_string()),
            );
        }
    }

    let standard_public = ["description", "location", "photograph_by", "date", "time"];

    for (raw_k, raw_v) in public_fields {
        let k = raw_k.trim();
        let v = value_to_trimmed_string(raw_v);
        if k.is_empty() || v.is_empty() {
            continue;
        }
        // Enforce metadata.rs privacy boundary: sensitive keys can NEVER remain public!
        if default_detail_visibility(k) == "private" {
            other_details.insert(
                k.to_string(),
                json!({ "value": v, "visibility": "private" }),
            );
        } else if standard_public.contains(&k.to_ascii_lowercase().as_str()) {
            proposed.insert(k.to_ascii_lowercase(), Value::String(v));
        } else {
            other_details.insert(k.to_string(), json!({ "value": v, "visibility": "public" }));
        }
    }

    for (raw_k, raw_v) in private_fields {
        let k = raw_k.trim();
        let v = value_to_trimmed_string(raw_v);
        if k.is_empty() || v.is_empty() {
            continue;
        }
        other_details.insert(
            k.to_string(),
            json!({ "value": v, "visibility": "private" }),
        );
    }

    if !notes.trim().is_empty() {
        proposed.insert("notes".to_string(), Value::String(notes.trim().to_string()));
    }
    if !other_details.is_empty() {
        proposed.insert("other_details".to_string(), Value::Object(other_details));
    }
    proposed
}

fn decode_canvas_image_base64(raw: &str) -> Result<Vec<u8>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Missing image data (`image_base64`).".to_string());
    }
    let b64_part = match trimmed.split_once(",") {
        Some((prefix, rest)) if prefix.starts_with("data:image/") => rest.trim(),
        _ => trimmed,
    };
    // Pre-check base64 length (~4/3 expansion) before allocating full decoded buffer.
    if b64_part.len() > (MAX_CANVAS_IMAGE_BYTES * 4) / 3 + 64 {
        return Err("Image exceeds maximum allowed size of 10 MB.".to_string());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64_part)
        .map_err(|e| format!("Invalid base64 image payload: {e}"))?;
    if bytes.is_empty() || bytes.len() > MAX_CANVAS_IMAGE_BYTES {
        return Err("Decoded image is empty or exceeds 10 MB.".to_string());
    }
    // Validate image bytes decode cleanly via safe Rust `image` crate.
    image::load_from_memory(&bytes)
        .map_err(|e| format!("Uploaded image could not be decoded as a valid image: {e}"))?;
    Ok(bytes)
}

async fn canvas_submit_enrollment(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(payload): Json<SubmitEnrollmentReq>,
) -> Response {
    let target_name = payload
        .existing_identity
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| payload.identity_name.trim())
        .to_string();
    if target_name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Person name is required." })),
        )
            .into_response();
    }

    let is_admin = is_admin_request(&headers, &state.sessions);
    let svc = state.service.clone();

    let (code, body) = tokio::task::spawn_blocking(move || {
        // Resolve image bytes either from base64 canvas crop or fallback to the session's source image.
        let image_bytes = if !payload.image_base64.trim().is_empty() {
            match decode_canvas_image_base64(&payload.image_base64) {
                Ok(b) => b,
                Err(reason) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        json!({ "status": "error", "reason": reason }),
                    );
                }
            }
        } else if is_safe_id(&payload.session_id) {
            let sfile =
                canvas_sessions_dir(&svc.cfg).join(format!("enroll_{}.json", payload.session_id));
            let resolved = fs::read_to_string(&sfile)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| {
                    v.get("source_image_path")
                        .and_then(Value::as_str)
                        .map(PathBuf::from)
                });
            match resolved.and_then(|p| fs::read(&p).ok()) {
                Some(b) if !b.is_empty() && b.len() <= MAX_CANVAS_IMAGE_BYTES => b,
                _ => {
                    return (
                        StatusCode::BAD_REQUEST,
                        json!({
                            "status": "error",
                            "reason": "Provide a cropped/uploaded image before submitting enrollment.",
                        }),
                    );
                }
            }
        } else {
            return (
                StatusCode::BAD_REQUEST,
                json!({
                    "status": "error",
                    "reason": "Provide a cropped/uploaded image before submitting enrollment.",
                }),
            );
        };

        let proposed = build_classified_proposed_metadata(
            &svc.cfg,
            &payload.group_folder,
            &payload.public_fields,
            &payload.private_fields,
            &payload.notes,
        );

        let staged_dir = svc.cfg.pending_dir.join("staged");
        let _ = fs::create_dir_all(&staged_dir);
        let temp_file = staged_dir.join(format!("CANVAS-{}.jpg", Uuid::new_v4().simple()));
        if let Err(e) = fs::write(&temp_file, &image_bytes) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "status": "error", "reason": format!("Failed saving image: {e}") }),
            );
        }

        if is_admin {
            let existing_target = payload
                .existing_identity
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let enroll_res = if let Some(existing_name) = existing_target {
                svc.admin_add_profile_image(existing_name, &temp_file)
            } else {
                let proposed_str =
                    serde_json::to_string(&Value::Object(proposed.clone())).unwrap_or_default();
                svc.tool_enroll_named_identity(
                    &target_name,
                    &temp_file.to_string_lossy(),
                    &svc.cfg.admin_key,
                    &proposed_str,
                )
            };
            let _ = fs::remove_file(&temp_file);
            let st = enroll_res
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("");
            if st != "enrolled_by_admin" && st != "image_added" && st != "unchanged" {
                return (StatusCode::BAD_REQUEST, enroll_res);
            }
            if !proposed.is_empty() {
                let proposed_str =
                    serde_json::to_string(&Value::Object(proposed)).unwrap_or_default();
                let _ = svc.tool_request_update_metadata(
                    &target_name,
                    &proposed_str,
                    &payload.notes,
                    &svc.cfg.admin_key,
                );
            }
            let mut out = enroll_res.as_object().cloned().unwrap_or_default();
            out.insert("is_admin".to_string(), Value::Bool(true));
            out.insert("identity".to_string(), Value::String(target_name));
            (StatusCode::OK, Value::Object(out))
        } else {
            // Guest path: stage strictly into pending_requests (NEVER touches arkib_vectors.json or profiles/).
            let staged_res = svc.stage_canvas_enrollment_request(
                &target_name,
                &temp_file,
                proposed,
                &payload.notes,
            );
            let _ = fs::remove_file(&temp_file);
            (StatusCode::OK, staged_res)
        }
    })
    .await
    .unwrap_or_else(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "status": "error", "reason": e.to_string() }),
        )
    });

    (code, Json(body)).into_response()
}

#[derive(Deserialize)]
struct TrackChatQueryReq {
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    actor: String,
}

async fn canvas_track_chat_query(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(payload): Json<TrackChatQueryReq>,
) -> Json<Value> {
    let prompt = payload.prompt.trim().to_string();
    if prompt.len() >= 2 {
        let actor = if !payload.actor.trim().is_empty() {
            payload.actor.trim().to_string()
        } else if is_admin_request(&headers, &state.sessions) {
            state.service.cfg.admin_user.clone()
        } else {
            "chatbot_user".to_string()
        };
        let cfg = state.service.cfg.clone();
        let _ = tokio::task::spawn_blocking(move || {
            crate::analytics::record_chat_prompt(&cfg, &prompt, &actor);
        })
        .await;
    }
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn test_canvas_jawi_preview_deterministic() {
        let res = decode_jawi_text(
            "مک تتکال ايت سݢل راج٢ دان اورڠ بسر٢ ڤون برهيمڤون دباليروڠ سري".to_string(),
        )
        .unwrap();
        assert_eq!(
            res["standard_malay_rumi_text"],
            "maka tatkala itu segala raja-raja dan orang besar-besar pun berhimpun di balairung seri"
        );
        assert_eq!(res["coverage_pct"], 100.0);
    }

    #[test]
    fn test_canvas_ocr_edit_admin_commit_and_lexicon_dedup() {
        let base =
            std::env::temp_dir().join(format!("arkib_canvas_ocr_test_{}", Uuid::new_v4().simple()));
        let mut cfg = AppConfig::load();
        cfg.project_root = base.clone();
        cfg.documents_dir = base.join("data").join("processed").join("documents");
        cfg.revisions_dir = base.join("data").join("interim").join("revisions");
        fs::create_dir_all(&cfg.documents_dir).unwrap();

        let pairs = vec![
            LexiconPair {
                jawi: "تستجاوي".into(),
                rumi: "testjawi".into(),
            },
            LexiconPair {
                jawi: "تستجاوي".into(),
                rumi: "testjawi".into(),
            },
        ];
        let out = apply_ocr_edit_to_archive(
            &cfg,
            Some("surat_sultan_01"),
            Some("Surat Sultan 01"),
            "سورت سلطان ١٣ محرم ١٣٢٤",
            "Surat Sultan 13 Muharam 1324H",
            "Verified by archivist",
            &pairs,
            true,
        )
        .unwrap();
        assert_eq!(out["status"], "saved_by_admin");
        assert_eq!(out["verified_by_human"], true);
        assert_eq!(out["lexicon_added"], 1);

        let meta_raw = fs::read_to_string(
            cfg.documents_dir
                .join("surat_sultan_01")
                .join("metadata.json"),
        )
        .unwrap();
        let meta: Value = serde_json::from_str(&meta_raw).unwrap();
        assert_eq!(meta["verified_by_human"], true);
        assert_eq!(meta["hijri_date"], "13 Muharam 1324H");
        assert_eq!(meta["gregorian_date_masihi"], "1906-03-09");

        // Second append of the same word must deduplicate to 0
        let added_again = append_verified_lexicon_pairs(&cfg, &pairs).unwrap();
        assert_eq!(added_again, 0);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_canvas_enrollment_private_key_routing_and_image_decode() {
        // Test base64 image decode + validation
        let img = RgbImage::from_pixel(32, 32, Rgb([100, 120, 140]));
        let mut jpeg_buf = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg_buf),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        let b64 = format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&jpeg_buf)
        );
        let decoded = decode_canvas_image_base64(&b64).unwrap();
        assert_eq!(decoded, jpeg_buf);

        // Enforce private-key classification even when sensitive fields are sent under public_fields!
        let base = std::env::temp_dir().join(format!(
            "arkib_canvas_sess_test_{}",
            Uuid::new_v4().simple()
        ));
        let mut cfg = AppConfig::load();
        cfg.project_root = base.clone();
        cfg.people_groups_file = base.join("people_groups.json");

        let mut pub_map = Map::new();
        pub_map.insert("no_kp".into(), Value::String("900101-14-1234".into()));
        pub_map.insert("phone".into(), Value::String("012-3456789".into()));
        pub_map.insert("department".into(), Value::String("Pemuliharaan".into()));
        pub_map.insert("description".into(), Value::String("Archivist".into()));
        let mut priv_map = Map::new();
        priv_map.insert("email".into(), Value::String("user@arkib.gov.my".into()));

        let proposed =
            build_classified_proposed_metadata(&cfg, "Staff", &pub_map, &priv_map, "Test note");
        assert_eq!(proposed["description"], "Archivist");
        let od = proposed["other_details"].as_object().unwrap();
        assert_eq!(od["no_kp"]["visibility"], "private");
        assert_eq!(od["phone"]["visibility"], "private");
        assert_eq!(od["email"]["visibility"], "private");
        assert_eq!(od["department"]["visibility"], "public");

        // Test save + stale cleanup of canvas sessions
        let mut m = Map::new();
        m.insert("raw_jawi_text".into(), Value::String("سجاره".into()));
        let sid = save_canvas_session(&cfg, "ocr", m).unwrap();
        assert!(
            canvas_sessions_dir(&cfg)
                .join(format!("ocr_{sid}.json"))
                .is_file()
        );
        let (removed, _) = cleanup_stale_canvas_sessions(&cfg, 0);
        assert_eq!(removed, 1);
        let _ = fs::remove_dir_all(&base);
    }
}
