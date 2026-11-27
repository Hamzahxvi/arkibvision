#![forbid(unsafe_code)]

use crate::analytics;
use crate::archives;
use crate::audit;
use crate::categories;
use crate::config::{ADMIN_SESSION_COOKIE, ADMIN_SESSION_TTL_SECS, is_document_ext, is_image_ext};
use crate::metadata::value_to_trimmed_string;
use crate::preferences;
use crate::revisions;
use crate::service::ArkibService;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use base64::Engine as _;
use rand::RngCore;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct AdminState {
    pub service: Arc<ArkibService>,
    pub sessions: Arc<Mutex<HashMap<String, u64>>>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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

fn valid_session_token(
    headers: &HeaderMap,
    sessions: &Mutex<HashMap<String, u64>>,
) -> Option<String> {
    let now = now_secs();
    let mut guard = sessions.lock().expect("sessions lock");
    for val in headers.get_all(header::COOKIE) {
        let Ok(raw) = val.to_str() else { continue };
        for item in raw.split(';') {
            let Some((name, token)) = item.trim().split_once('=') else {
                continue;
            };
            if name == ADMIN_SESSION_COOKIE
                && let Some(&exp) = guard.get(token)
            {
                if exp > now {
                    return Some(token.to_string());
                }
                guard.remove(token);
            }
        }
    }
    None
}

pub fn is_admin_request(headers: &HeaderMap, sessions: &Mutex<HashMap<String, u64>>) -> bool {
    valid_session_token(headers, sessions).is_some()
}

fn is_allowed_origin(origin: &str, librechat_url: &str) -> bool {
    let o = origin.trim().trim_end_matches('/');
    o.eq_ignore_ascii_case(librechat_url.trim_end_matches('/'))
        || o.eq_ignore_ascii_case("http://localhost:3080")
        || o.eq_ignore_ascii_case("http://127.0.0.1:3080")
}

pub async fn cors_middleware(
    State(state): State<AdminState>,
    req: Request,
    next: Next,
) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let allowed = origin
        .as_deref()
        .filter(|o| is_allowed_origin(o, &state.service.cfg.librechat_url));

    if req.method() == Method::OPTIONS {
        let mut resp = StatusCode::NO_CONTENT.into_response();
        if let Some(orig) = allowed
            && let Ok(hv) = HeaderValue::from_str(orig)
        {
            let h = resp.headers_mut();
            h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, hv);
            h.insert(
                header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
            h.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, POST, PATCH, DELETE, OPTIONS"),
            );
            h.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("Content-Type"),
            );
            h.insert(header::VARY, HeaderValue::from_static("Origin"));
        }
        return resp;
    }

    let mut resp = next.run(req).await;
    if let Some(orig) = allowed
        && let Ok(hv) = HeaderValue::from_str(orig)
    {
        let h = resp.headers_mut();
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, hv);
        h.insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
        h.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    resp
}

async fn audit_middleware(State(state): State<AdminState>, req: Request, next: Next) -> Response {
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    let is_authed = valid_session_token(req.headers(), &state.sessions).is_some();
    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            s.split(',')
                .next()
                .unwrap_or("127.0.0.1")
                .trim()
                .to_string()
        })
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let started = std::time::Instant::now();

    let resp = next.run(req).await;
    let status = resp.status().as_u16();
    let duration_ms = started.elapsed().as_millis() as u64;

    let skip_quiet = method == "GET"
        && status < 400
        && query.is_empty()
        && (path.starts_with("/admin/profile-images/")
            || path.starts_with("/admin/location-images/")
            || path.starts_with("/admin/document-files/")
            || path.starts_with("/guest/profile-images/")
            || path.starts_with("/guest/location-images/")
            || path.starts_with("/guest/document-files/")
            || path.starts_with("/admin/images/")
            || path.starts_with("/admin/enhanced-images/")
            || path.starts_with("/admin/revision-snapshots/")
            || matches!(
                path.as_str(),
                "/admin/api/session"
                    | "/admin/api/audit/dashboard"
                    | "/admin/api/system/health"
                    | "/admin/api/preferences"
                    | "/admin/api/analytics/behavior"
            ));
    if skip_quiet || method == "OPTIONS" {
        return resp;
    }

    let action = if path == "/admin/api/login" {
        if status == 200 {
            "LOGIN_OK"
        } else {
            "LOGIN_FAIL"
        }
    } else if path.contains("/export/") || path.contains("/report/") {
        "EXPORT"
    } else {
        match method.as_str() {
            "POST" => "CREATE",
            "PATCH" | "PUT" => "UPDATE",
            "DELETE" => "DELETE",
            _ => "READ",
        }
    };

    let (resource_type, resource_id) = classify_resource(&path);
    let actor = if is_authed || (path == "/admin/api/login" && status == 200) {
        state.service.cfg.admin_user.clone()
    } else {
        "guest".to_string()
    };
    let role = if is_authed { "admin" } else { "guest" };

    audit::log_event(
        &state.service.cfg,
        &actor,
        role,
        &ip,
        &method,
        &path,
        &query,
        action,
        &resource_type,
        &resource_id,
        status,
        "",
        duration_ms,
    );
    resp
}

fn classify_resource(path: &str) -> (String, String) {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 3 && (parts[0] == "admin" || parts[0] == "guest") && parts[1] == "api" {
        let rtype = parts[2].to_string();
        let rid = parts.get(3).copied().unwrap_or("").to_string();
        (rtype, rid)
    } else if parts.first().copied() == Some("guest") {
        ("guest".to_string(), path.to_string())
    } else {
        ("admin".to_string(), path.to_string())
    }
}

/// Strict read-only allowlist for the Guest Portal (`/guest/*`).
/// Only GET reads and read-only face recognition / search analytics are permitted;
/// every write, edit, delete, enrollment, or admin endpoint is denied.
pub fn is_guest_allowed_request(method: &Method, path: &str) -> bool {
    if !path.starts_with("/guest") {
        return false;
    }
    if *method == Method::GET {
        return matches!(
            path,
            "/guest"
                | "/guest/"
                | "/guest/api/profiles"
                | "/guest/api/people-groups"
                | "/guest/api/locations"
                | "/guest/api/location-groups"
                | "/guest/api/documents"
                | "/guest/api/document-groups"
                | "/guest/api/categories"
        ) || path.starts_with("/guest/profile-images/")
            || path.starts_with("/guest/location-images/")
            || path.starts_with("/guest/document-files/")
            || (path.starts_with("/guest/api/documents/") && path.contains("/export/"));
    }
    if *method == Method::POST {
        return matches!(
            path,
            "/guest/api/test-recognition" | "/guest/api/analytics/track"
        );
    }
    false
}

/// Rewrite `/admin/...` static media URLs inside JSON payloads into `/guest/...`
/// so unauthenticated Guest Portal browsers can view photos & files read-only.
fn rewrite_admin_urls_for_guest(val: &mut Value) {
    match val {
        Value::String(s) => {
            if let Some(rest) = s.strip_prefix("/admin/profile-images/") {
                *s = format!("/guest/profile-images/{rest}");
            } else if let Some(rest) = s.strip_prefix("/admin/location-images/") {
                *s = format!("/guest/location-images/{rest}");
            } else if let Some(rest) = s.strip_prefix("/admin/document-files/") {
                *s = format!("/guest/document-files/{rest}");
            }
        }
        Value::Array(arr) => {
            for item in arr {
                rewrite_admin_urls_for_guest(item);
            }
        }
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                rewrite_admin_urls_for_guest(v);
            }
        }
        _ => {}
    }
}

async fn auth_middleware(State(state): State<AdminState>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let method = req.method();
    if path == "/guest" || path.starts_with("/guest/") {
        if is_guest_allowed_request(method, path) {
            return next.run(req).await;
        }
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "status": "forbidden",
                "reason": "Guest role is strictly read-only.",
            })),
        )
            .into_response();
    }
    if matches!(
        path,
        "/admin" | "/admin/" | "/admin/api/session" | "/admin/api/login" | "/admin/api/logout"
    ) {
        return next.run(req).await;
    }
    if valid_session_token(req.headers(), &state.sessions).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "status": "unauthorized",
                "reason": "Please sign in to the Admin Portal.",
            })),
        )
            .into_response();
    }
    next.run(req).await
}

pub fn admin_router(state: AdminState) -> Result<Router, String> {
    let router = Router::new()
        // Redirect legacy /admin URL straight into LibreChat's integrated Admin Portal
        .route("/admin", get(admin_redirect_to_librechat))
        .route("/admin/", get(admin_redirect_to_librechat))
        // JSON Session Auth for LibreChat-native Admin Portal
        .route("/admin/api/session", get(admin_api_session))
        .route("/admin/api/login", post(admin_api_login))
        .route("/admin/api/logout", post(admin_api_logout))
        .route(
            "/admin/images/{img_type}/{request_id}",
            get(admin_serve_image),
        )
        .route("/admin/api/requests", get(admin_api_requests))
        .route(
            "/admin/api/profiles",
            get(admin_api_profiles).post(admin_create_profile),
        )
        // Maintainable Enrolled People Group Folders (e.g. Staff, External Entity)
        .route(
            "/admin/api/people-groups",
            get(admin_api_people_groups).post(admin_create_people_group),
        )
        .route(
            "/admin/api/people-groups/{slug}",
            patch(admin_rename_people_group).delete(admin_delete_people_group),
        )
        .route("/admin/api/test-recognition", post(admin_test_recognition))
        .route(
            "/admin/api/profiles/{profile_slug}",
            patch(admin_update_profile).delete(admin_delete_profile),
        )
        .route(
            "/admin/api/profiles/{profile_slug}/images",
            post(admin_add_profile_image),
        )
        .route(
            "/admin/api/profiles/{profile_slug}/images/{*filename}",
            patch(admin_update_profile_image).delete(admin_delete_profile_image),
        )
        .route(
            "/admin/profile-images/{profile_slug}/{*filename}",
            get(admin_profile_image),
        )
        .route("/admin/api/approve/{request_id}", post(admin_approve))
        .route("/admin/api/reject/{request_id}", post(admin_reject))
        // Managed Archival Location Folders & Location Group Folders
        .route(
            "/admin/api/location-groups",
            get(admin_api_location_groups).post(admin_create_location_group),
        )
        .route(
            "/admin/api/location-groups/{slug}",
            patch(admin_rename_location_group).delete(admin_delete_location_group),
        )
        .route(
            "/admin/api/locations",
            get(admin_api_locations).post(admin_create_location),
        )
        .route(
            "/admin/api/locations/{slug}",
            patch(admin_update_location).delete(admin_delete_location),
        )
        .route(
            "/admin/api/locations/{slug}/images",
            post(admin_add_location_image),
        )
        .route(
            "/admin/api/locations/{slug}/images/{*filename}",
            axum::routing::delete(admin_delete_location_image),
        )
        .route(
            "/admin/location-images/{slug}/{*filename}",
            get(admin_location_image),
        )
        // Managed Archival Document Folders & Document Group Folders
        .route(
            "/admin/api/document-groups",
            get(admin_api_document_groups).post(admin_create_document_group),
        )
        .route(
            "/admin/api/document-groups/{slug}",
            patch(admin_rename_document_group).delete(admin_delete_document_group),
        )
        .route(
            "/admin/api/documents",
            get(admin_api_documents).post(admin_create_document),
        )
        .route(
            "/admin/api/documents/{slug}",
            patch(admin_update_document).delete(admin_delete_document),
        )
        .route(
            "/admin/api/documents/{slug}/files",
            post(admin_add_document_file),
        )
        .route(
            "/admin/api/documents/{slug}/files/{*filename}",
            axum::routing::delete(admin_delete_document_file),
        )
        .route(
            "/admin/api/documents/{slug}/summarize",
            post(admin_summarize_document),
        )
        .route(
            "/admin/api/documents/{slug}/export/{format}",
            get(admin_export_document),
        )
        .route(
            "/admin/document-files/{slug}/{*filename}",
            get(admin_document_file),
        )
        // System Performance Monitor, Integrity Audit & Archival Conservation Lab
        .route("/admin/api/system/health", get(admin_system_health))
        .route("/admin/api/system/integrity", post(admin_system_integrity))
        .route(
            "/admin/api/system/clear-cache",
            post(admin_system_clear_cache),
        )
        .route(
            "/admin/api/conservation/analyze",
            post(admin_conservation_analyze),
        )
        .route(
            "/admin/api/conservation/enhance",
            post(admin_conservation_enhance),
        )
        .route(
            "/admin/enhanced-images/{*filename}",
            get(admin_enhanced_image),
        )
        // Maintainable Categories CRUD
        .route(
            "/admin/api/categories",
            get(admin_api_categories).post(admin_create_category),
        )
        .route(
            "/admin/api/categories/{slug}",
            patch(admin_update_category).delete(admin_delete_category),
        )
        // AI Content Analysis -> Auto-Suggest Form Fields on Upload
        .route("/admin/api/analyze-upload", post(admin_analyze_upload))
        // Change Detection & Before/After Revision Comparison
        .route("/admin/api/revisions", get(admin_api_revisions))
        .route(
            "/admin/api/revisions/{entity_type}/{slug}",
            get(admin_api_entity_revisions),
        )
        .route(
            "/admin/api/revisions/scan-disk",
            post(admin_revisions_scan_disk),
        )
        .route(
            "/admin/revision-snapshots/{entity_type}/{slug}/{filename}",
            get(admin_revision_snapshot),
        )
        // Audit Management, Rules Engine, Outlier Detection & AI Summary Report
        .route("/admin/api/audit/dashboard", get(admin_audit_dashboard))
        .route(
            "/admin/api/audit/inspect-sql",
            post(admin_audit_inspect_sql),
        )
        .route("/admin/api/audit/explain", post(admin_audit_explain))
        .route(
            "/admin/api/audit/report/{format}",
            get(admin_audit_export_report),
        )
        // User Click Preference Learning & Personalization
        .route(
            "/admin/api/preferences",
            get(admin_api_preferences).patch(admin_update_preferences),
        )
        .route("/admin/api/preferences/track", post(admin_track_preference))
        .route(
            "/admin/api/preferences/reset",
            post(admin_reset_preferences),
        )
        // User Behavior, Search History & Chatbot Hot-Topics Analytics
        .route(
            "/admin/api/analytics/behavior",
            get(admin_api_behavior_analytics),
        )
        .route(
            "/admin/api/analytics/track",
            post(admin_track_behavior_analytics),
        )
        // Read-Only Guest Portal (People, Locations, Documents, Test Face Recognition ONLY)
        .route("/guest", get(guest_redirect_to_librechat))
        .route("/guest/", get(guest_redirect_to_librechat))
        .route("/guest/api/profiles", get(guest_api_profiles))
        .route("/guest/api/people-groups", get(admin_api_people_groups))
        .route(
            "/guest/profile-images/{profile_slug}/{*filename}",
            get(admin_profile_image),
        )
        .route("/guest/api/locations", get(guest_api_locations))
        .route("/guest/api/location-groups", get(admin_api_location_groups))
        .route(
            "/guest/location-images/{slug}/{*filename}",
            get(admin_location_image),
        )
        .route("/guest/api/documents", get(guest_api_documents))
        .route("/guest/api/document-groups", get(admin_api_document_groups))
        .route("/guest/api/categories", get(admin_api_categories))
        .route(
            "/guest/document-files/{slug}/{*filename}",
            get(admin_document_file),
        )
        .route(
            "/guest/api/documents/{slug}/export/{format}",
            get(admin_export_document),
        )
        .route("/guest/api/test-recognition", post(admin_test_recognition))
        .route(
            "/guest/api/analytics/track",
            post(guest_track_behavior_analytics),
        )
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            audit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            cors_middleware,
        ))
        .with_state(state);

    Ok(router)
}

async fn admin_redirect_to_librechat(State(state): State<AdminState>) -> Redirect {
    Redirect::temporary(&format!("{}/#arkib-admin", state.service.cfg.librechat_url))
}

async fn guest_redirect_to_librechat(State(state): State<AdminState>) -> Redirect {
    Redirect::temporary(&format!("{}/#arkib-guest", state.service.cfg.librechat_url))
}

async fn guest_api_profiles(State(state): State<AdminState>) -> Json<Value> {
    let Json(mut val) = admin_api_profiles(State(state)).await;
    rewrite_admin_urls_for_guest(&mut val);
    Json(val)
}

async fn guest_api_locations(State(state): State<AdminState>) -> Json<Value> {
    let Json(mut val) = admin_api_locations(State(state)).await;
    rewrite_admin_urls_for_guest(&mut val);
    Json(val)
}

async fn guest_api_documents(State(state): State<AdminState>) -> Json<Value> {
    let Json(mut val) = admin_api_documents(State(state)).await;
    rewrite_admin_urls_for_guest(&mut val);
    Json(val)
}

async fn guest_track_behavior_analytics(
    State(state): State<AdminState>,
    Json(payload): Json<TrackBehaviorPayload>,
) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let source = match payload.source.trim() {
        "guest_view" => "guest_view".to_string(),
        _ => "guest_search".to_string(),
    };
    let _ = tokio::task::spawn_blocking(move || {
        analytics::record_admin_activity(
            &cfg,
            &source,
            "guest",
            &payload.query_text,
            &payload.entity_type,
            &payload.item_slug,
            &payload.item_title,
            &payload.category,
            payload.tags,
        );
    })
    .await;
    Json(json!({ "status": "ok" }))
}

#[derive(Deserialize)]
struct LoginPayload {
    username: Option<String>,
    password: Option<String>,
}

async fn admin_api_session(State(state): State<AdminState>, headers: HeaderMap) -> Json<Value> {
    let authed = valid_session_token(&headers, &state.sessions).is_some();
    Json(json!({
        "authenticated": authed,
        "user": if authed { Value::String(state.service.cfg.admin_user.clone()) } else { Value::Null },
        "face_embedding_model": state.service.engine.face_model_name(),
        "face_embedding_dim": state.service.engine.face_embedding_dim(),
        "default_face_threshold": state.service.engine.default_identify_threshold(),
    }))
}

async fn admin_api_login(
    State(state): State<AdminState>,
    Json(payload): Json<LoginPayload>,
) -> Response {
    let user = payload.username.unwrap_or_default();
    let pass = payload.password.unwrap_or_default();
    let ok = constant_time_eq(user.trim(), &state.service.cfg.admin_user)
        && constant_time_eq(&pass, &state.service.cfg.admin_pass);
    if !ok {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "status": "error",
                "reason": "Invalid username or password.",
            })),
        )
            .into_response();
    }
    let now = now_secs();
    let mut tok_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut tok_bytes);
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(tok_bytes);
    {
        let mut guard = state.sessions.lock().expect("sessions lock");
        guard.retain(|_, &mut exp| exp > now);
        guard.insert(token.clone(), now + ADMIN_SESSION_TTL_SECS);
    }
    let cookie = format!(
        "{ADMIN_SESSION_COOKIE}={token}; Max-Age={ADMIN_SESSION_TTL_SECS}; HttpOnly; SameSite=Lax; Path=/"
    );
    let mut resp = (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "authenticated": true,
            "user": state.service.cfg.admin_user,
        })),
    )
        .into_response();
    if let Ok(val) = HeaderValue::from_str(&cookie) {
        resp.headers_mut().append(header::SET_COOKIE, val);
    }
    resp
}

async fn admin_api_logout(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Some(tok) = valid_session_token(&headers, &state.sessions) {
        state.sessions.lock().expect("sessions lock").remove(&tok);
    }
    let mut resp = (
        StatusCode::OK,
        Json(json!({ "status": "logged_out", "authenticated": false })),
    )
        .into_response();
    if let Ok(val) = HeaderValue::from_str(&format!(
        "{ADMIN_SESSION_COOKIE}=; Max-Age=0; HttpOnly; SameSite=Lax; Path=/"
    )) {
        resp.headers_mut().append(header::SET_COOKIE, val);
    }
    resp
}

async fn admin_api_requests(State(state): State<AdminState>) -> Json<Value> {
    let reqs = state.service.load_pending_requests();
    let mut out: Vec<Value> = reqs
        .into_iter()
        .map(|(req_id, req)| {
            json!({
                "id": req_id,
                "name": req.get("identity").and_then(Value::as_str).unwrap_or(""),
                "status": req.get("status").and_then(Value::as_str).unwrap_or("pending"),
                "request_type": req.get("type").and_then(Value::as_str).unwrap_or(""),
                "submitted_at": req.get("submitted_at").and_then(Value::as_str).unwrap_or(""),
                "metadata": req.get("proposed_metadata").cloned().unwrap_or_else(|| json!({})),
                "note": req.get("notes").and_then(Value::as_str).unwrap_or(""),
                "document_slug": req.get("document_slug").cloned().unwrap_or(Value::Null),
                "document_title": req.get("document_title").cloned().unwrap_or(Value::Null),
                "edited_jawi_text": req.get("edited_jawi_text").cloned().unwrap_or(Value::Null),
                "edited_rumi_text": req.get("edited_rumi_text").cloned().unwrap_or(Value::Null),
                "lexicon_suggestions": req.get("lexicon_suggestions").cloned().unwrap_or_else(|| json!([])),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        let sa = a.get("submitted_at").and_then(Value::as_str).unwrap_or("");
        let sb = b.get("submitted_at").and_then(Value::as_str).unwrap_or("");
        sb.cmp(sa)
    });
    Json(Value::Array(out))
}

async fn admin_api_profiles(State(state): State<AdminState>) -> Json<Value> {
    let svc = state.service.clone();
    let profiles = tokio::task::spawn_blocking(move || {
        let rev_map = revisions::revision_summary_map(&svc.cfg, "profile");
        let counts = svc.store.list_counts();
        let mut entries: Vec<(String, usize)> = counts.into_iter().collect();
        entries.sort_by_key(|(name, _)| name.to_ascii_lowercase());
        entries
            .into_iter()
            .map(|(n, count)| {
                let mut rec = svc.admin_profile_record_with_count(&n, count);
                if let Some(obj) = rec.as_object_mut() {
                    let slug = obj["slug"].as_str().unwrap_or("").to_string();
                    if let Some((rc, lm)) = rev_map.get(&slug) {
                        obj.insert("revision_count".to_string(), json!(rc));
                        obj.insert("last_modified_at".to_string(), json!(lm));
                    } else {
                        obj.insert("revision_count".to_string(), json!(0));
                    }
                }
                rec
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    Json(Value::Array(profiles))
}

fn serve_file_bytes(path: &Path) -> Response {
    let Ok(bytes) = fs::read(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn safe_filename(filename: &str) -> Option<&str> {
    let clean = filename.trim_start_matches('/');
    if clean.is_empty() || clean.contains('/') || clean.contains('\\') || clean.contains("..") {
        return None;
    }
    Some(clean)
}

async fn admin_profile_image(
    State(state): State<AdminState>,
    AxumPath((slug, filename)): AxumPath<(String, String)>,
) -> Response {
    let Some(clean_name) = safe_filename(&filename) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if state.service.admin_profile_identity(&slug).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = state
        .service
        .cfg
        .profile_dir
        .join(&slug)
        .join("images")
        .join(clean_name);
    if !path.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve_file_bytes(&path)
}

async fn admin_serve_image(
    State(state): State<AdminState>,
    AxumPath((img_type, req_id)): AxumPath<(String, String)>,
) -> Response {
    static REQ_RE: OnceLock<Regex> = OnceLock::new();
    let re = REQ_RE.get_or_init(|| Regex::new(r"\A[A-Za-z0-9_-]+\z").expect("regex"));
    if !re.is_match(&req_id) || (img_type != "source" && img_type != "crop") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let prefix = if img_type == "crop" {
        format!("{req_id}_primary.")
    } else {
        format!("{req_id}_source.")
    };
    if let Ok(entries) = fs::read_dir(&state.service.cfg.pending_images_dir) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(&prefix))
                && entry.path().is_file()
            {
                return serve_file_bytes(&entry.path());
            }
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

fn sniff_image_ext(filename: &str, bytes: &[u8]) -> Option<String> {
    if is_image_ext(Path::new(filename))
        && let Some(ext) = Path::new(filename).extension().and_then(|e| e.to_str())
    {
        return Some(ext.to_ascii_lowercase());
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png".to_string())
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg".to_string())
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp".to_string())
    } else if bytes.starts_with(b"BM") {
        Some("bmp".to_string())
    } else {
        None
    }
}

async fn admin_test_recognition(
    State(state): State<AdminState>,
    mut multipart: Multipart,
) -> Response {
    let mut upload_bytes: Option<Vec<u8>> = None;
    let mut upload_name = String::new();
    let mut threshold = state.service.engine.default_identify_threshold();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "image" {
            upload_name = field.file_name().unwrap_or("").to_string();
            if let Ok(bytes) = field.bytes().await {
                upload_bytes = Some(bytes.to_vec());
            }
        } else if name == "threshold"
            && let Ok(text) = field.text().await
            && let Ok(val) = text.trim().parse::<f32>()
        {
            threshold = val.max(0.50);
        }
    }

    let Some(bytes) = upload_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Choose an image to test." })),
        )
            .into_response();
    };
    let Some(ext) = sniff_image_ext(&upload_name, &bytes) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Use JPG, PNG, WEBP, or BMP images." })),
        )
            .into_response();
    };

    let temp_path = state.service.cfg.pending_images_dir.join(format!(
        "ADMIN-TEST-{}.{ext}",
        uuid::Uuid::new_v4().simple()
    ));
    if let Err(e) = fs::write(&temp_path, &bytes) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response();
    }

    let svc = state.service.clone();
    let tp = temp_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let res = svc.tool_identify_faces(&tp.to_string_lossy(), threshold, false);
        let _ = fs::remove_file(&tp);
        res
    })
    .await
    .unwrap_or_else(|e| json!({ "status": "error", "reason": e.to_string() }));

    let status = if result.get("status").and_then(Value::as_str) == Some("unresolved") {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    (status, Json(result)).into_response()
}

async fn admin_create_profile(
    State(state): State<AdminState>,
    mut multipart: Multipart,
) -> Response {
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut upload_bytes: Option<Vec<u8>> = None;
    let mut upload_name = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "image" {
            upload_name = field.file_name().unwrap_or("upload.jpg").to_string();
            if let Ok(bytes) = field.bytes().await {
                upload_bytes = Some(bytes.to_vec());
            }
        } else if let Ok(text) = field.text().await {
            fields.insert(name, text.trim().to_string());
        }
    }

    let identity = fields.get("identity").cloned().unwrap_or_default();
    let Some(bytes) = upload_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "Name and a face image are required." })),
        )
            .into_response();
    };
    if identity.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "Name and a face image are required." })),
        )
            .into_response();
    }
    let Some(ext) = sniff_image_ext(&upload_name, &bytes) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "Use JPG, PNG, WEBP, or BMP images." })),
        )
            .into_response();
    };
    let temp_path = state
        .service
        .cfg
        .pending_images_dir
        .join(format!("ADMIN-{}.{ext}", uuid::Uuid::new_v4().simple()));
    if let Err(e) = fs::write(&temp_path, &bytes) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response();
    }

    let mut proposed = Map::new();
    for k in [
        "address",
        "notes",
        "description",
        "photograph_by",
        "location",
        "date",
        "time",
    ] {
        if let Some(v) = fields.get(k)
            && !v.is_empty()
        {
            proposed.insert(k.to_string(), Value::String(v.clone()));
        }
    }
    if let Some(raw_group) = fields
        .get("group_slug")
        .or_else(|| fields.get("group"))
        .map(String::as_str)
        && !raw_group.trim().is_empty()
    {
        if let Some((gs, gn)) = archives::resolve_people_group(&state.service.cfg, raw_group) {
            proposed.insert("group_slug".to_string(), Value::String(gs));
            proposed.insert("group".to_string(), Value::String(gn));
        } else {
            proposed.insert(
                "group".to_string(),
                Value::String(raw_group.trim().to_string()),
            );
        }
    }
    if let Some(raw_other) = fields.get("other_details")
        && !raw_other.is_empty()
        && let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(raw_other)
    {
        proposed.insert("other_details".to_string(), Value::Object(obj));
    }

    let svc = state.service.clone();
    let tp = temp_path.clone();
    let (status, body) = tokio::task::spawn_blocking(move || {
        let proposed_str =
            serde_json::to_string(&Value::Object(proposed.clone())).unwrap_or_default();
        let res = svc.tool_enroll_named_identity(
            &identity,
            &tp.to_string_lossy(),
            &svc.cfg.admin_key,
            &proposed_str,
        );
        let _ = fs::remove_file(&tp);
        let st = res.get("status").and_then(Value::as_str).unwrap_or("");
        if st != "enrolled_by_admin" && st != "unchanged" {
            return (StatusCode::BAD_REQUEST, res);
        }
        if !proposed.is_empty() {
            let meta_res =
                svc.tool_request_update_metadata(&identity, &proposed_str, "", &svc.cfg.admin_key);
            if meta_res.get("status").and_then(Value::as_str) != Some("updated_by_admin") {
                return (StatusCode::BAD_REQUEST, meta_res);
            }
        }
        let profile_rec = svc.admin_profile_record(&identity);
        if let Some(am) = profile_rec.as_object() {
            let slug = am["slug"].as_str().unwrap_or(&identity);
            let _ = revisions::record_lifecycle_event(
                &svc.cfg,
                "profile",
                slug,
                &identity,
                &svc.cfg.admin_user,
                "created",
                None,
                Some(am),
            );
        }
        let mut out = res.as_object().cloned().unwrap_or_default();
        out.insert("profile".to_string(), profile_rec);
        (StatusCode::OK, Value::Object(out))
    })
    .await
    .unwrap_or_else(|e| {
        (
            StatusCode::BAD_REQUEST,
            json!({ "status": "error", "reason": e.to_string() }),
        )
    });

    (status, Json(body)).into_response()
}

async fn admin_update_profile(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    Json(mut payload): Json<Map<String, Value>>,
) -> Response {
    let Some(mut identity) = state.service.admin_profile_identity(&slug) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Enrolled person not found." })),
        )
            .into_response();
    };
    let new_identity = payload
        .remove("identity")
        .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| identity.clone());

    let allowed = [
        "address",
        "notes",
        "description",
        "photograph_by",
        "location",
        "date",
        "time",
    ];
    let mut metadata = Map::new();
    for (k, v) in &payload {
        if allowed.contains(&k.as_str()) && !v.is_null() {
            metadata.insert(k.clone(), Value::String(value_to_trimmed_string(v)));
        }
    }
    if payload.contains_key("group_slug") || payload.contains_key("group") {
        let raw = payload
            .get("group_slug")
            .or_else(|| payload.get("group"))
            .map(value_to_trimmed_string)
            .unwrap_or_default();
        if raw.is_empty() {
            metadata.insert("group_slug".to_string(), Value::String(String::new()));
            metadata.insert("group".to_string(), Value::String(String::new()));
        } else if let Some((gs, gn)) = archives::resolve_people_group(&state.service.cfg, &raw) {
            metadata.insert("group_slug".to_string(), Value::String(gs));
            metadata.insert("group".to_string(), Value::String(gn));
        } else {
            metadata.insert("group".to_string(), Value::String(raw));
        }
    }
    if let Some(Value::Object(other)) = payload.get("other_details") {
        metadata.insert("other_details".to_string(), Value::Object(other.clone()));
        metadata.insert("_replace_other".to_string(), Value::Bool(true));
    }

    let svc = state.service.clone();
    let (status, body) = tokio::task::spawn_blocking(move || {
        let before_rec = svc.admin_profile_record(&identity);
        if new_identity != identity {
            let res = svc.tool_rename_identity(&identity, &new_identity, &svc.cfg.admin_key);
            if res.get("status").and_then(Value::as_str) != Some("updated") {
                return (StatusCode::BAD_REQUEST, res);
            }
            identity = new_identity;
        }
        if !metadata.is_empty() {
            let meta_str = serde_json::to_string(&Value::Object(metadata)).unwrap_or_default();
            let res =
                svc.tool_request_update_metadata(&identity, &meta_str, "", &svc.cfg.admin_key);
            if res.get("status").and_then(Value::as_str) != Some("updated_by_admin") {
                return (StatusCode::BAD_REQUEST, res);
            }
        }
        let after_rec = svc.admin_profile_record(&identity);
        if let (Some(bm), Some(am)) = (before_rec.as_object(), after_rec.as_object()) {
            let slug = am["slug"].as_str().unwrap_or(&identity);
            let _ = revisions::record_metadata_change(
                &svc.cfg,
                "profile",
                slug,
                &identity,
                &svc.cfg.admin_user,
                "metadata_edit",
                bm,
                am,
            );
        }
        (
            StatusCode::OK,
            json!({ "status": "updated", "profile": after_rec }),
        )
    })
    .await
    .unwrap_or_else(|e| {
        (
            StatusCode::BAD_REQUEST,
            json!({ "status": "error", "reason": e.to_string() }),
        )
    });

    (status, Json(body)).into_response()
}

async fn admin_delete_profile(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
) -> Response {
    let Some(identity) = state.service.admin_profile_identity(&slug) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Enrolled person not found." })),
        )
            .into_response();
    };
    let svc = state.service.clone();
    let slug_for_rev = slug.clone();
    let res = tokio::task::spawn_blocking(move || {
        let before = svc.admin_profile_record(&identity);
        let del = svc.tool_delete_identity(&identity, true, &svc.cfg.admin_key);
        if del.get("status").and_then(Value::as_str) == Some("deleted")
            && let Some(bm) = before.as_object()
        {
            let _ = revisions::record_lifecycle_event(
                &svc.cfg,
                "profile",
                &slug_for_rev,
                &identity,
                &svc.cfg.admin_user,
                "deleted",
                Some(bm),
                None,
            );
        }
        del
    })
    .await
    .unwrap_or_else(|e| json!({ "status": "error", "reason": e.to_string() }));
    let status = if res.get("status").and_then(Value::as_str) == Some("deleted") {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, Json(res)).into_response()
}

async fn admin_add_profile_image(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    mut multipart: Multipart,
) -> Response {
    let Some(identity) = state.service.admin_profile_identity(&slug) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "An enrolled person and image are required." })),
        )
            .into_response();
    };
    let mut upload_bytes: Option<Vec<u8>> = None;
    let mut upload_name = String::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("image") {
            upload_name = field.file_name().unwrap_or("upload.jpg").to_string();
            if let Ok(bytes) = field.bytes().await {
                upload_bytes = Some(bytes.to_vec());
            }
        }
    }
    let Some(bytes) = upload_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "An enrolled person and image are required." })),
        )
            .into_response();
    };
    let Some(ext) = sniff_image_ext(&upload_name, &bytes) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "Use JPG, PNG, WEBP, or BMP images." })),
        )
            .into_response();
    };
    let temp_path = state
        .service
        .cfg
        .pending_images_dir
        .join(format!("ADMIN-{}.{ext}", uuid::Uuid::new_v4().simple()));
    if let Err(e) = fs::write(&temp_path, &bytes) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response();
    }

    let svc = state.service.clone();
    let tp = temp_path.clone();
    let bytes_clone = bytes.clone();
    let upload_name_clone = upload_name.clone();
    let slug_clone = slug.clone();
    let (status, body) = tokio::task::spawn_blocking(move || {
        let res = svc.admin_add_profile_image(&identity, &tp);
        let _ = fs::remove_file(&tp);
        if res.get("status").and_then(Value::as_str) != Some("image_added") {
            return (StatusCode::BAD_REQUEST, res);
        }
        let _ = revisions::record_file_change(
            &svc.cfg,
            "profile",
            &slug_clone,
            &identity,
            &svc.cfg.admin_user,
            &upload_name_clone,
            "added",
            None,
            Some(&bytes_clone),
        );
        let mut out = res.as_object().cloned().unwrap_or_default();
        out.insert("profile".to_string(), svc.admin_profile_record(&identity));
        (StatusCode::OK, Value::Object(out))
    })
    .await
    .unwrap_or_else(|e| {
        (
            StatusCode::BAD_REQUEST,
            json!({ "status": "error", "reason": e.to_string() }),
        )
    });
    (status, Json(body)).into_response()
}

async fn admin_update_profile_image(
    State(state): State<AdminState>,
    AxumPath((slug, filename)): AxumPath<(String, String)>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let Some(clean_name) = safe_filename(&filename) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Profile image not found." })),
        )
            .into_response();
    };
    let Some(identity) = state.service.admin_profile_identity(&slug) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Profile image not found." })),
        )
            .into_response();
    };
    let meta_file = state
        .service
        .cfg
        .profile_dir
        .join(&slug)
        .join("metadata.json");
    let Ok(text) = fs::read_to_string(&meta_file) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Profile metadata not found." })),
        )
            .into_response();
    };
    let mut metadata = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    let allowed = ["description", "photograph_by", "location", "date", "time"];
    let mut images_list = metadata
        .get("images")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut updated = false;
    for item in &mut images_list {
        if let Some(obj) = item.as_object_mut() {
            let pname = obj
                .get("profile_path")
                .or_else(|| obj.get("filename"))
                .and_then(Value::as_str)
                .and_then(|s| Path::new(s).file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if pname == clean_name {
                for k in allowed {
                    if let Some(v) = payload.get(k)
                        && !v.is_null()
                    {
                        let s = v.as_str().unwrap_or("").trim();
                        obj.insert(
                            k.to_string(),
                            Value::String(if s.is_empty() {
                                "Unknown".into()
                            } else {
                                s.into()
                            }),
                        );
                    }
                }
                updated = true;
                break;
            }
        }
    }
    if !updated {
        let mut entry = Map::new();
        entry.insert(
            "filename".to_string(),
            Value::String(clean_name.to_string()),
        );
        entry.insert(
            "profile_path".to_string(),
            Value::String(format!("profiles/{slug}/images/{clean_name}")),
        );
        entry.insert(
            "url".to_string(),
            Value::String(format!(
                "{}/images/profile-gallery/{slug}/{clean_name}",
                state.service.cfg.librechat_url
            )),
        );
        for k in allowed {
            if let Some(v) = payload.get(k)
                && !v.is_null()
            {
                let s = v.as_str().unwrap_or("").trim();
                entry.insert(
                    k.to_string(),
                    Value::String(if s.is_empty() {
                        "Unknown".into()
                    } else {
                        s.into()
                    }),
                );
            }
        }
        images_list.push(Value::Object(entry));
    }
    metadata.insert("images".to_string(), Value::Array(images_list));
    if let Ok(pretty) = serde_json::to_string_pretty(&Value::Object(metadata)) {
        let _ = fs::write(&meta_file, pretty);
    }

    Json(json!({
        "status": "image_updated",
        "identity": identity,
        "filename": clean_name,
        "profile": state.service.admin_profile_record(&identity),
    }))
    .into_response()
}

async fn admin_delete_profile_image(
    State(state): State<AdminState>,
    AxumPath((slug, filename)): AxumPath<(String, String)>,
) -> Response {
    let Some(clean_name) = safe_filename(&filename) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Profile image not found." })),
        )
            .into_response();
    };
    let Some(identity) = state.service.admin_profile_identity(&slug) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Profile image not found." })),
        )
            .into_response();
    };
    let pdir = state.service.cfg.profile_dir.join(&slug);
    let image_path = pdir.join("images").join(clean_name);
    if !image_path.is_file() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Profile image not found." })),
        )
            .into_response();
    }
    let _ = fs::remove_file(&image_path);
    let web_path: PathBuf = state
        .service
        .cfg
        .profile_web_dir
        .join(&slug)
        .join(clean_name);
    let _ = fs::remove_file(&web_path);

    let meta_file = pdir.join("metadata.json");
    if let Ok(text) = fs::read_to_string(&meta_file)
        && let Ok(Value::Object(mut metadata)) = serde_json::from_str::<Value>(&text)
    {
        let filtered: Vec<Value> = metadata
            .get("images")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|item| {
                let pname = item
                    .get("profile_path")
                    .or_else(|| item.get("filename"))
                    .and_then(Value::as_str)
                    .and_then(|s| Path::new(s).file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                pname != clean_name
            })
            .collect();
        metadata.insert(
            "stored_original_image_count".to_string(),
            json!(filtered.len()),
        );
        metadata.insert("images".to_string(), Value::Array(filtered));
        if let Ok(pretty) = serde_json::to_string_pretty(&Value::Object(metadata)) {
            let _ = fs::write(&meta_file, pretty);
        }
    }

    Json(json!({
        "status": "image_deleted",
        "identity": identity,
        "filename": clean_name,
        "profile": state.service.admin_profile_record(&identity),
    }))
    .into_response()
}

#[derive(Default, Deserialize)]
struct ApproveOpts {
    #[serde(default)]
    add_to_lexicon: Option<bool>,
}

async fn admin_approve(
    State(state): State<AdminState>,
    AxumPath(req_id): AxumPath<String>,
    body: Option<Json<ApproveOpts>>,
) -> Response {
    let add_to_lexicon = body.and_then(|b| b.0.add_to_lexicon).unwrap_or(true);
    let svc = state.service.clone();
    let res = tokio::task::spawn_blocking(move || {
        svc.review_pending_request_with_opts(
            &req_id,
            "approve",
            &svc.cfg.admin_key,
            "",
            add_to_lexicon,
        )
    })
    .await
    .unwrap_or_else(|e| json!({ "status": "error", "reason": e.to_string() }));
    let status = if res.get("status").and_then(Value::as_str) == Some("approved") {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, Json(res)).into_response()
}

async fn admin_reject(
    State(state): State<AdminState>,
    AxumPath(req_id): AxumPath<String>,
) -> Response {
    let svc = state.service.clone();
    let res = tokio::task::spawn_blocking(move || {
        svc.tool_review_pending_request(&req_id, "reject", &svc.cfg.admin_key, "")
    })
    .await
    .unwrap_or_else(|e| json!({ "status": "error", "reason": e.to_string() }));
    let status = if res.get("status").and_then(Value::as_str) == Some("rejected") {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, Json(res)).into_response()
}

// ─── Managed Location Folder Endpoints ───────────────────────────────────────

async fn admin_api_locations(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let list = tokio::task::spawn_blocking(move || {
        let rev_map = revisions::revision_summary_map(&cfg, "location");
        archives::list_locations(&cfg)
            .into_iter()
            .map(|mut rec| {
                if let Some(obj) = rec.as_object_mut() {
                    let slug = obj["slug"].as_str().unwrap_or("").to_string();
                    if let Some((rc, lm)) = rev_map.get(&slug) {
                        obj.insert("revision_count".to_string(), json!(rc));
                        obj.insert("last_modified_at".to_string(), json!(lm));
                    } else {
                        obj.insert("revision_count".to_string(), json!(0));
                    }
                }
                rec
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    Json(Value::Array(list))
}

async fn admin_create_location(
    State(state): State<AdminState>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        let loc = archives::save_location(&cfg, None, &payload)?;
        if let Some(a) = loc.as_object() {
            let slug = a["slug"].as_str().unwrap_or("location");
            let title = a["name"].as_str().unwrap_or(slug);
            let _ = revisions::record_lifecycle_event(
                &cfg,
                "location",
                slug,
                title,
                &cfg.admin_user,
                "created",
                None,
                Some(a),
            );
        }
        Ok::<Value, String>(loc)
    })
    .await
    {
        Ok(Ok(loc)) => (
            StatusCode::OK,
            Json(json!({ "status": "created", "location": loc })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_update_location(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid location slug." })),
        )
            .into_response();
    };
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        let before = archives::load_location(&cfg, &clean_slug);
        let saved = archives::save_location(&cfg, Some(&clean_slug), &payload)?;
        if let (Some(b), Some(a)) = (
            before.as_ref().and_then(Value::as_object),
            saved.as_object(),
        ) {
            let slug = a["slug"].as_str().unwrap_or(&clean_slug);
            let title = a["name"].as_str().unwrap_or(slug);
            let _ = revisions::record_metadata_change(
                &cfg,
                "location",
                slug,
                title,
                &cfg.admin_user,
                "metadata_edit",
                b,
                a,
            );
        }
        Ok::<Value, String>(saved)
    })
    .await
    {
        Ok(Ok(loc)) => (
            StatusCode::OK,
            Json(json!({ "status": "updated", "location": loc })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_delete_location(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid location slug." })),
        )
            .into_response();
    };
    let svc = state.service.clone();
    match tokio::task::spawn_blocking(move || {
        let before = archives::load_location(&svc.cfg, &clean_slug);
        let res = archives::delete_location(&svc.cfg, &svc.location_store, &clean_slug)?;
        if let Some(bm) = before.as_ref().and_then(Value::as_object) {
            let title = bm["name"].as_str().unwrap_or(&clean_slug);
            let _ = revisions::record_lifecycle_event(
                &svc.cfg,
                "location",
                &clean_slug,
                title,
                &svc.cfg.admin_user,
                "deleted",
                Some(bm),
                None,
            );
        }
        Ok::<Value, String>(res)
    })
    .await
    {
        Ok(Ok(res)) => (StatusCode::OK, Json(res)).into_response(),
        Ok(Err(reason)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize, Default)]
struct ForceQuery {
    force: Option<String>,
}

fn is_force(q: &ForceQuery) -> bool {
    q.force
        .as_deref()
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

async fn admin_add_location_image(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    Query(q): Query<ForceQuery>,
    mut multipart: Multipart,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Invalid location slug." })),
        )
            .into_response();
    };
    let mut upload_bytes: Option<Vec<u8>> = None;
    let mut upload_name = String::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("image") {
            upload_name = field.file_name().unwrap_or("photo.jpg").to_string();
            if let Ok(bytes) = field.bytes().await {
                upload_bytes = Some(bytes.to_vec());
            }
        }
    }
    let Some(bytes) = upload_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "Choose an image to upload." })),
        )
            .into_response();
    };
    if !is_image_ext(Path::new(&upload_name)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "Use JPG, PNG, WEBP, or BMP images." })),
        )
            .into_response();
    }
    let force = is_force(&q);
    let svc = state.service.clone();
    match tokio::task::spawn_blocking(move || {
        archives::add_location_image(
            &svc.cfg,
            &svc.engine,
            &svc.location_store,
            &clean_slug,
            &upload_name,
            &bytes,
            force,
        )
    })
    .await
    {
        Ok(Ok(loc)) => (
            StatusCode::OK,
            Json(json!({ "status": "image_added", "location": loc })),
        )
            .into_response(),
        Ok(Err(reason)) => {
            let code = if reason.starts_with("Duplicate item detected") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            (code, Json(json!({ "status": "error", "reason": reason }))).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_delete_location_image(
    State(state): State<AdminState>,
    AxumPath((slug, filename)): AxumPath<(String, String)>,
) -> Response {
    let (Some(clean_slug), Some(clean_name)) = (safe_filename(&slug), safe_filename(&filename))
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Image not found." })),
        )
            .into_response();
    };
    match archives::delete_location_image(&state.service.cfg, clean_slug, clean_name) {
        Ok(loc) => (
            StatusCode::OK,
            Json(json!({ "status": "image_deleted", "location": loc })),
        )
            .into_response(),
        Err(reason) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": reason })),
        )
            .into_response(),
    }
}

async fn admin_location_image(
    State(state): State<AdminState>,
    AxumPath((slug, filename)): AxumPath<(String, String)>,
) -> Response {
    let (Some(clean_slug), Some(clean_name)) = (safe_filename(&slug), safe_filename(&filename))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = state
        .service
        .cfg
        .locations_dir
        .join(clean_slug)
        .join("images")
        .join(clean_name);
    if !path.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve_file_bytes(&path)
}

// ─── Managed Document Folder Endpoints ───────────────────────────────────────

async fn admin_api_documents(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let list = tokio::task::spawn_blocking(move || {
        let rev_map = revisions::revision_summary_map(&cfg, "document");
        archives::list_documents(&cfg)
            .into_iter()
            .map(|mut rec| {
                if let Some(obj) = rec.as_object_mut() {
                    let slug = obj["slug"].as_str().unwrap_or("").to_string();
                    if let Some((rc, lm)) = rev_map.get(&slug) {
                        obj.insert("revision_count".to_string(), json!(rc));
                        obj.insert("last_modified_at".to_string(), json!(lm));
                    } else {
                        obj.insert("revision_count".to_string(), json!(0));
                    }
                }
                rec
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    Json(Value::Array(list))
}

async fn admin_create_document(
    State(state): State<AdminState>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        let doc = archives::save_document(&cfg, None, &payload)?;
        if let Some(a) = doc.as_object() {
            let slug = a["slug"].as_str().unwrap_or("doc");
            let title = a["title"].as_str().unwrap_or(slug);
            let _ = revisions::record_lifecycle_event(
                &cfg,
                "document",
                slug,
                title,
                &cfg.admin_user,
                "created",
                None,
                Some(a),
            );
        }
        Ok::<Value, String>(doc)
    })
    .await
    {
        Ok(Ok(doc)) => (
            StatusCode::OK,
            Json(json!({ "status": "created", "document": doc })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_update_document(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid document slug." })),
        )
            .into_response();
    };
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        let before = archives::load_document(&cfg, &clean_slug);
        let saved = archives::save_document(&cfg, Some(&clean_slug), &payload)?;
        if let (Some(b), Some(a)) = (
            before.as_ref().and_then(Value::as_object),
            saved.as_object(),
        ) {
            let slug = a["slug"].as_str().unwrap_or(&clean_slug);
            let title = a["title"].as_str().unwrap_or(slug);
            let _ = revisions::record_metadata_change(
                &cfg,
                "document",
                slug,
                title,
                &cfg.admin_user,
                "metadata_edit",
                b,
                a,
            );
        }
        Ok::<Value, String>(saved)
    })
    .await
    {
        Ok(Ok(doc)) => (
            StatusCode::OK,
            Json(json!({ "status": "updated", "document": doc })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_delete_document(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid document slug." })),
        )
            .into_response();
    };
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        let before = archives::load_document(&cfg, &clean_slug);
        let res = archives::delete_document(&cfg, &clean_slug)?;
        if let Some(b) = before.as_ref().and_then(Value::as_object) {
            let title = b["title"].as_str().unwrap_or(&clean_slug);
            let _ = revisions::record_lifecycle_event(
                &cfg,
                "document",
                &clean_slug,
                title,
                &cfg.admin_user,
                "deleted",
                Some(b),
                None,
            );
        }
        Ok::<Value, String>(res)
    })
    .await
    {
        Ok(Ok(res)) => (StatusCode::OK, Json(res)).into_response(),
        Ok(Err(reason)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_add_document_file(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    Query(q): Query<ForceQuery>,
    mut multipart: Multipart,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Invalid document slug." })),
        )
            .into_response();
    };
    let mut upload_bytes: Option<Vec<u8>> = None;
    let mut upload_name = String::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") || field.name() == Some("image") {
            upload_name = field.file_name().unwrap_or("document.txt").to_string();
            if let Ok(bytes) = field.bytes().await {
                upload_bytes = Some(bytes.to_vec());
            }
        }
    }
    let Some(bytes) = upload_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "Choose a file to upload." })),
        )
            .into_response();
    };
    if !is_document_ext(Path::new(&upload_name)) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "status": "rejected", "reason": "Supported formats: TXT, MD, CSV, JSON, HTML, XML, PDF, DOCX, JPG, PNG, WEBP, BMP." }))).into_response();
    }
    let force = is_force(&q);
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        let existing_path = cfg
            .documents_dir
            .join(&clean_slug)
            .join("files")
            .join(&upload_name);
        let before_bytes = fs::read(&existing_path).ok();
        let doc = archives::add_document_file(&cfg, &clean_slug, &upload_name, &bytes, force)?;
        let title = doc["title"].as_str().unwrap_or(&clean_slug).to_string();
        let kind = if before_bytes.is_some() {
            "replaced"
        } else {
            "added"
        };
        let _ = revisions::record_file_change(
            &cfg,
            "document",
            &clean_slug,
            &title,
            &cfg.admin_user,
            &upload_name,
            kind,
            before_bytes.as_deref(),
            Some(&bytes),
        );
        Ok::<Value, String>(doc)
    })
    .await
    {
        Ok(Ok(doc)) => (
            StatusCode::OK,
            Json(json!({ "status": "file_added", "document": doc })),
        )
            .into_response(),
        Ok(Err(reason)) => {
            let code = if reason.starts_with("Duplicate item detected") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            (code, Json(json!({ "status": "error", "reason": reason }))).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_delete_document_file(
    State(state): State<AdminState>,
    AxumPath((slug, filename)): AxumPath<(String, String)>,
) -> Response {
    let (Some(clean_slug), Some(clean_name)) = (safe_filename(&slug), safe_filename(&filename))
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "File not found." })),
        )
            .into_response();
    };
    let old_path = state
        .service
        .cfg
        .documents_dir
        .join(clean_slug)
        .join("files")
        .join(clean_name);
    let before_bytes = fs::read(&old_path).ok();
    match archives::delete_document_file(&state.service.cfg, clean_slug, clean_name) {
        Ok(doc) => {
            let title = doc["title"].as_str().unwrap_or(clean_slug);
            let _ = revisions::record_file_change(
                &state.service.cfg,
                "document",
                clean_slug,
                title,
                &state.service.cfg.admin_user,
                clean_name,
                "deleted",
                before_bytes.as_deref(),
                None,
            );
            (
                StatusCode::OK,
                Json(json!({ "status": "file_deleted", "document": doc })),
            )
                .into_response()
        }
        Err(reason) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": reason })),
        )
            .into_response(),
    }
}

async fn admin_document_file(
    State(state): State<AdminState>,
    AxumPath((slug, filename)): AxumPath<(String, String)>,
) -> Response {
    let (Some(clean_slug), Some(clean_name)) = (safe_filename(&slug), safe_filename(&filename))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = state
        .service
        .cfg
        .documents_dir
        .join(clean_slug)
        .join("files")
        .join(clean_name);
    if !path.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve_file_bytes(&path)
}

async fn admin_summarize_document(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid document slug." })),
        )
            .into_response();
    };
    let Some(doc) = archives::load_document(&state.service.cfg, &clean_slug) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Document folder not found." })),
        )
            .into_response();
    };
    let text = doc["extracted_text"].as_str().unwrap_or("").to_string();
    if text.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "status": "error", "reason": "Add extracted text or upload a readable file (TXT/DOCX/PDF) before summarizing." }))).into_response();
    }
    let summary_res = state
        .service
        .tool_summarize_document("", &text, "auto")
        .await;
    let mut patch = Map::new();
    if let Some(s) = summary_res["archival_summary"].as_str() {
        patch.insert("summary".to_string(), Value::String(s.to_string()));
    }
    if let Some(st) = summary_res.get("structured_extraction") {
        patch.insert("key_topics".to_string(), st["key_topics"].clone());
        patch.insert("key_names".to_string(), st["key_names"].clone());
        patch.insert("key_dates".to_string(), st["key_dates"].clone());
    }
    match archives::save_document(&state.service.cfg, Some(&clean_slug), &patch) {
        Ok(updated) => (
            StatusCode::OK,
            Json(json!({ "status": "summarized", "document": updated })),
        )
            .into_response(),
        Err(reason) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
    }
}

// ─── System Performance Monitor, Checksum Integrity & Conservation Endpoints ─

async fn admin_export_document(
    State(state): State<AdminState>,
    AxumPath((slug, format)): AxumPath<(String, String)>,
) -> Response {
    let fmt = format.trim().to_ascii_lowercase();
    if fmt != "docx" && fmt != "pdf" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Export format must be 'docx' or 'pdf'." })),
        )
            .into_response();
    }
    let cfg = state.service.cfg.clone();
    let slug_c = slug.clone();
    let doc_opt = tokio::task::spawn_blocking(move || archives::load_document(&cfg, &slug_c))
        .await
        .ok()
        .flatten();
    let Some(doc) = doc_opt else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": "Document not found." })),
        )
            .into_response();
    };
    let export_in = crate::export::ExportDocumentInput::from_doc_value(&doc);
    let built = if fmt == "docx" {
        crate::export::build_docx_bytes(&export_in).map(|b| {
            (
                b,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                format!("{slug}.docx"),
            )
        })
    } else {
        crate::export::build_pdf_bytes(&export_in)
            .map(|b| (b, "application/pdf", format!("{slug}.pdf")))
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

async fn admin_system_health(State(state): State<AdminState>) -> Json<Value> {
    Json(state.service.tool_system_health().await)
}

async fn admin_system_integrity(State(state): State<AdminState>) -> Json<Value> {
    let svc = state.service.clone();
    let res = tokio::task::spawn_blocking(move || svc.tool_verify_integrity(true))
        .await
        .unwrap_or_else(|e| json!({ "status": "error", "reason": e.to_string() }));
    Json(res)
}

async fn admin_system_clear_cache(State(state): State<AdminState>) -> Json<Value> {
    let svc = state.service.clone();
    let res = tokio::task::spawn_blocking(move || svc.clear_temp_image_caches())
        .await
        .unwrap_or_else(|e| json!({ "status": "error", "reason": e.to_string() }));
    Json(res)
}

async fn admin_conservation_analyze(
    State(state): State<AdminState>,
    mut multipart: Multipart,
) -> Response {
    let mut upload_bytes: Option<Vec<u8>> = None;
    let mut upload_name = String::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("image") || field.name() == Some("file") {
            upload_name = field.file_name().unwrap_or("scan.jpg").to_string();
            if let Ok(bytes) = field.bytes().await {
                upload_bytes = Some(bytes.to_vec());
            }
        }
    }
    let Some(bytes) = upload_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Choose an image to analyze." })),
        )
            .into_response();
    };
    if !is_image_ext(Path::new(&upload_name)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Use JPG, PNG, WEBP, or BMP images." })),
        )
            .into_response();
    }
    let ext = Path::new(&upload_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    let temp_path = state.service.cfg.pending_images_dir.join(format!(
        "ADMIN-CONS-{}.{ext}",
        uuid::Uuid::new_v4().simple()
    ));
    if let Err(e) = fs::write(&temp_path, &bytes) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response();
    }
    let result = state
        .service
        .tool_assess_conservation(&temp_path.to_string_lossy())
        .await;
    let _ = fs::remove_file(&temp_path);
    (StatusCode::OK, Json(result)).into_response()
}

async fn admin_conservation_enhance(
    State(state): State<AdminState>,
    mut multipart: Multipart,
) -> Response {
    let mut upload_bytes: Option<Vec<u8>> = None;
    let mut upload_name = String::new();
    let mut mode = "auto".to_string();
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "image" || name == "file" {
            upload_name = field.file_name().unwrap_or("scan.jpg").to_string();
            if let Ok(bytes) = field.bytes().await {
                upload_bytes = Some(bytes.to_vec());
            }
        } else if name == "mode"
            && let Ok(text) = field.text().await
            && !text.trim().is_empty()
        {
            mode = text.trim().to_string();
        }
    }
    let Some(bytes) = upload_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Choose an image to enhance." })),
        )
            .into_response();
    };
    if !is_image_ext(Path::new(&upload_name)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Use JPG, PNG, WEBP, or BMP images." })),
        )
            .into_response();
    }
    let ext = Path::new(&upload_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    let temp_path = state
        .service
        .cfg
        .pending_images_dir
        .join(format!("ADMIN-ENH-{}.{ext}", uuid::Uuid::new_v4().simple()));
    if let Err(e) = fs::write(&temp_path, &bytes) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response();
    }
    let svc = state.service.clone();
    let tp = temp_path.clone();
    let mut result = tokio::task::spawn_blocking(move || {
        let res = svc.tool_enhance_image(&tp.to_string_lossy(), &mode);
        let _ = fs::remove_file(&tp);
        res
    })
    .await
    .unwrap_or_else(|e| json!({ "status": "error", "reason": e.to_string() }));
    if let Some(fname) = result["enhanced_filename"].as_str().map(String::from)
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert(
            "admin_preview_url".to_string(),
            Value::String(format!("/admin/enhanced-images/{fname}")),
        );
    }
    (StatusCode::OK, Json(result)).into_response()
}

async fn admin_enhanced_image(
    State(state): State<AdminState>,
    AxumPath(filename): AxumPath<String>,
) -> Response {
    let Some(clean_name) = safe_filename(&filename) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = state
        .service
        .cfg
        .librechat_dir
        .join("images")
        .join("enhanced")
        .join(clean_name);
    if !path.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve_file_bytes(&path)
}

// ─── Maintainable People Group Folder Endpoints ──────────────────────────────

async fn admin_api_people_groups(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let list = tokio::task::spawn_blocking(move || archives::list_people_groups(&cfg))
        .await
        .unwrap_or_default();
    Json(Value::Array(list))
}

async fn admin_create_people_group(
    State(state): State<AdminState>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let name = payload
        .get("name")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    let description = payload
        .get("description")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        archives::create_people_group(&cfg, &name, &description)
    })
    .await
    {
        Ok(Ok(group)) => (
            StatusCode::OK,
            Json(json!({ "status": "created", "group": group })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_rename_people_group(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid group slug." })),
        )
            .into_response();
    };
    let name = payload
        .get("name")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    let desc = payload
        .get("description")
        .filter(|v| !v.is_null())
        .map(value_to_trimmed_string);
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        archives::rename_people_group(&cfg, &clean_slug, &name, desc.as_deref())
    })
    .await
    {
        Ok(Ok(group)) => (
            StatusCode::OK,
            Json(json!({ "status": "updated", "group": group })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_delete_people_group(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid group slug." })),
        )
            .into_response();
    };
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || archives::delete_people_group(&cfg, &clean_slug))
        .await
    {
        Ok(Ok(res)) => (StatusCode::OK, Json(res)).into_response(),
        Ok(Err(reason)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

// ─── Maintainable Document Group Folder Endpoints ────────────────────────────

async fn admin_api_document_groups(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let list = tokio::task::spawn_blocking(move || archives::list_document_groups(&cfg))
        .await
        .unwrap_or_default();
    Json(Value::Array(list))
}

async fn admin_create_document_group(
    State(state): State<AdminState>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let name = payload
        .get("name")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    let description = payload
        .get("description")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        archives::create_document_group(&cfg, &name, &description)
    })
    .await
    {
        Ok(Ok(group)) => (
            StatusCode::OK,
            Json(json!({ "status": "created", "group": group })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_rename_document_group(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid group slug." })),
        )
            .into_response();
    };
    let name = payload
        .get("name")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    let desc = payload
        .get("description")
        .filter(|v| !v.is_null())
        .map(value_to_trimmed_string);
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        archives::rename_document_group(&cfg, &clean_slug, &name, desc.as_deref())
    })
    .await
    {
        Ok(Ok(group)) => (
            StatusCode::OK,
            Json(json!({ "status": "updated", "group": group })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_delete_document_group(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid group slug." })),
        )
            .into_response();
    };
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || archives::delete_document_group(&cfg, &clean_slug))
        .await
    {
        Ok(Ok(res)) => (StatusCode::OK, Json(res)).into_response(),
        Ok(Err(reason)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

// ─── Maintainable Location Group Folder Endpoints ────────────────────────────

async fn admin_api_location_groups(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let list = tokio::task::spawn_blocking(move || archives::list_location_groups(&cfg))
        .await
        .unwrap_or_default();
    Json(Value::Array(list))
}

async fn admin_create_location_group(
    State(state): State<AdminState>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let name = payload
        .get("name")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    let description = payload
        .get("description")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        archives::create_location_group(&cfg, &name, &description)
    })
    .await
    {
        Ok(Ok(group)) => (
            StatusCode::OK,
            Json(json!({ "status": "created", "group": group })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_rename_location_group(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid group slug." })),
        )
            .into_response();
    };
    let name = payload
        .get("name")
        .map(value_to_trimmed_string)
        .unwrap_or_default();
    let desc = payload
        .get("description")
        .filter(|v| !v.is_null())
        .map(value_to_trimmed_string);
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        archives::rename_location_group(&cfg, &clean_slug, &name, desc.as_deref())
    })
    .await
    {
        Ok(Ok(group)) => (
            StatusCode::OK,
            Json(json!({ "status": "updated", "group": group })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_delete_location_group(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid group slug." })),
        )
            .into_response();
    };
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || archives::delete_location_group(&cfg, &clean_slug))
        .await
    {
        Ok(Ok(res)) => (StatusCode::OK, Json(res)).into_response(),
        Ok(Err(reason)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

// ─── Maintainable Categories CRUD Endpoints ──────────────────────────────────

#[derive(Deserialize)]
struct CategoryPayload {
    name: Option<String>,
    color: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct DeleteCategoryQuery {
    reassign_to: Option<String>,
}

async fn admin_api_categories(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let list = tokio::task::spawn_blocking(move || {
        let docs = archives::list_documents(&cfg);
        categories::list_categories_with_counts(&cfg, &docs)
    })
    .await
    .unwrap_or_default();
    Json(Value::Array(list))
}

async fn admin_create_category(
    State(state): State<AdminState>,
    Json(payload): Json<CategoryPayload>,
) -> Response {
    let cfg = state.service.cfg.clone();
    let name = payload.name.unwrap_or_default();
    let color = payload.color;
    let desc = payload.description;
    match tokio::task::spawn_blocking(move || {
        let cat = categories::create_category(&cfg, &name, color.as_deref(), desc.as_deref())?;
        if let Ok(Value::Object(am)) = serde_json::to_value(&cat) {
            let _ = revisions::record_lifecycle_event(
                &cfg,
                "category",
                &cat.slug,
                &cat.name,
                &cfg.admin_user,
                "created",
                None,
                Some(&am),
            );
        }
        Ok::<categories::CategoryItem, String>(cat)
    })
    .await
    {
        Ok(Ok(cat)) => (
            StatusCode::OK,
            Json(json!({ "status": "created", "category": cat })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_update_category(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    Json(payload): Json<CategoryPayload>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid category slug." })),
        )
            .into_response();
    };
    let cfg = state.service.cfg.clone();
    let name = payload.name;
    let color = payload.color;
    let desc = payload.description;
    match tokio::task::spawn_blocking(move || {
        categories::update_category(
            &cfg,
            &clean_slug,
            name.as_deref(),
            color.as_deref(),
            desc.as_deref(),
        )
    })
    .await
    {
        Ok(Ok((cat, updated_docs))) => (
            StatusCode::OK,
            Json(json!({
                "status": "updated",
                "category": cat,
                "updated_documents": updated_docs,
            })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_delete_category(
    State(state): State<AdminState>,
    AxumPath(slug): AxumPath<String>,
    Query(q): Query<DeleteCategoryQuery>,
) -> Response {
    let Some(clean_slug) = safe_filename(&slug).map(str::to_string) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "not_found", "reason": "Invalid category slug." })),
        )
            .into_response();
    };
    let cfg = state.service.cfg.clone();
    let reassign = q.reassign_to;
    match tokio::task::spawn_blocking(move || {
        let res = categories::delete_category(&cfg, &clean_slug, reassign.as_deref())?;
        let dname = res["deleted_name"].as_str().unwrap_or(&clean_slug);
        if let Some(bm) = res.as_object() {
            let _ = revisions::record_lifecycle_event(
                &cfg,
                "category",
                &clean_slug,
                dname,
                &cfg.admin_user,
                "deleted",
                Some(bm),
                None,
            );
        }
        Ok::<Value, String>(res)
    })
    .await
    {
        Ok(Ok(res)) => (StatusCode::OK, Json(res)).into_response(),
        Ok(Err(reason)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

// ─── AI Auto-Suggest on File Upload ──────────────────────────────────────────

async fn admin_analyze_upload(
    State(state): State<AdminState>,
    mut multipart: Multipart,
) -> Response {
    let mut upload_bytes: Option<Vec<u8>> = None;
    let mut upload_name = String::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        if matches!(
            field.name(),
            Some("file") | Some("image") | Some("document")
        ) {
            upload_name = field.file_name().unwrap_or("upload.txt").to_string();
            if let Ok(bytes) = field.bytes().await {
                upload_bytes = Some(bytes.to_vec());
            }
        }
    }
    let Some(bytes) = upload_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Please choose an image or document file to analyze." })),
        )
            .into_response();
    };
    if !is_document_ext(Path::new(&upload_name)) && !is_image_ext(Path::new(&upload_name)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Supported formats: JPG, PNG, WEBP, BMP, PDF, DOCX, TXT, MD, CSV, HTML, XML, JSON." })),
        )
            .into_response();
    }
    match archives::analyze_file_for_suggestions(&state.service.cfg, &upload_name, &bytes).await {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(reason) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
    }
}

// ─── Change Detection & Before/After Revision Endpoints ──────────────────────

#[derive(Deserialize)]
struct RevisionListQuery {
    entity_type: Option<String>,
    limit: Option<usize>,
}

async fn admin_api_revisions(
    State(state): State<AdminState>,
    Query(q): Query<RevisionListQuery>,
) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let et = q.entity_type;
    let limit = q.limit.unwrap_or(50);
    let list = tokio::task::spawn_blocking(move || {
        revisions::list_recent_revisions(&cfg, et.as_deref(), limit)
    })
    .await
    .unwrap_or_default();
    Json(json!({
        "status": "ok",
        "count": list.len(),
        "revisions": list,
    }))
}

async fn admin_api_entity_revisions(
    State(state): State<AdminState>,
    AxumPath((entity_type, slug)): AxumPath<(String, String)>,
) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let list = tokio::task::spawn_blocking(move || {
        revisions::get_entity_revisions(&cfg, &entity_type, &slug)
    })
    .await
    .unwrap_or_default();
    Json(json!({
        "status": "ok",
        "count": list.len(),
        "revisions": list,
    }))
}

async fn admin_revisions_scan_disk(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let result = tokio::task::spawn_blocking(move || revisions::scan_external_disk_changes(&cfg))
        .await
        .unwrap_or_else(|_| json!({ "status": "error" }));
    Json(result)
}

async fn admin_revision_snapshot(
    State(state): State<AdminState>,
    AxumPath((entity_type, slug, filename)): AxumPath<(String, String, String)>,
) -> Response {
    let Some(path) =
        revisions::resolve_snapshot_path(&state.service.cfg, &entity_type, &slug, &filename)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    serve_file_bytes(&path)
}

// ─── Audit Management, Rules Engine, Outlier Detection & Report Export ───────

#[derive(Deserialize)]
struct InspectSqlPayload {
    sql_log: Option<String>,
    actor: Option<String>,
}

async fn admin_audit_dashboard(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let res = tokio::task::spawn_blocking(move || {
        let snapshot = audit::build_audit_snapshot(&cfg);
        let recent_revisions = revisions::list_recent_revisions(&cfg, None, 25);
        let deterministic_summary = audit::deterministic_anomaly_explanation(&snapshot);
        json!({
            "status": "ok",
            "snapshot": snapshot,
            "deterministic_summary": deterministic_summary,
            "recent_revisions": recent_revisions,
        })
    })
    .await
    .unwrap_or_else(|_| json!({ "status": "error" }));
    Json(res)
}

async fn admin_audit_inspect_sql(
    State(state): State<AdminState>,
    Json(payload): Json<InspectSqlPayload>,
) -> Response {
    let raw = payload.sql_log.unwrap_or_default();
    if raw.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Paste SQL queries or audit log lines to inspect." })),
        )
            .into_response();
    }
    let actor = payload
        .actor
        .unwrap_or_else(|| "external_db_user".to_string());
    let result = audit::inspect_external_sql_logs(&raw, &actor);

    audit::log_event(
        &state.service.cfg,
        &state.service.cfg.admin_user,
        "admin",
        "127.0.0.1",
        "POST",
        "/admin/api/audit/inspect-sql",
        "",
        "READ",
        "sql_audit",
        &format!("anomalies={}", result["anomaly_count"]),
        200,
        raw.lines().next().unwrap_or(""),
        1,
    );

    (StatusCode::OK, Json(result)).into_response()
}

async fn admin_audit_explain(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let snapshot = tokio::task::spawn_blocking(move || audit::build_audit_snapshot(&cfg))
        .await
        .unwrap_or_else(|_| audit::build_audit_snapshot(&state.service.cfg));
    let out = audit::explain_anomalies_with_ai(&state.service.cfg, &snapshot).await;
    Json(out)
}

async fn admin_audit_export_report(
    State(state): State<AdminState>,
    AxumPath(format): AxumPath<String>,
) -> Response {
    let fmt = format.trim().to_ascii_lowercase();
    if fmt != "docx" && fmt != "pdf" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Format must be 'docx' or 'pdf'." })),
        )
            .into_response();
    }

    let cfg = state.service.cfg.clone();
    let snapshot = tokio::task::spawn_blocking(move || audit::build_audit_snapshot(&cfg))
        .await
        .unwrap_or_else(|_| audit::build_audit_snapshot(&state.service.cfg));
    let ai_summary = audit::deterministic_anomaly_explanation(&snapshot);

    let mut body_lines = vec![
        format!("Generated At: {}", snapshot.generated_at),
        format!(
            "Overall Risk Level: {} (Score: {}/100)",
            snapshot.risk_level, snapshot.risk_score
        ),
        format!("Total Events Analyzed: {}", snapshot.total_events_analyzed),
        format!(
            "Severity Breakdown: CRITICAL={}, HIGH={}, MEDIUM={}, LOW={}",
            snapshot
                .severity_counts
                .get("CRITICAL")
                .copied()
                .unwrap_or(0),
            snapshot.severity_counts.get("HIGH").copied().unwrap_or(0),
            snapshot.severity_counts.get("MEDIUM").copied().unwrap_or(0),
            snapshot.severity_counts.get("LOW").copied().unwrap_or(0),
        ),
        String::new(),
        "=== RULES ENGINE ALERTS ===".to_string(),
    ];
    if snapshot.rule_alerts.is_empty() {
        body_lines.push("No rules-engine anomalies detected.".to_string());
    } else {
        for a in &snapshot.rule_alerts {
            body_lines.push(format!(
                "[{}] {} ({}) | Actor: {} | Events: {} | {}",
                a.severity, a.title, a.rule_id, a.actor, a.event_count, a.description
            ));
            body_lines.push(format!("  Recommended Action: {}", a.recommended_action));
        }
    }
    body_lines.push(String::new());
    body_lines.push("=== STATISTICAL OUTLIER FINDINGS ===".to_string());
    if snapshot.statistical_outliers.is_empty() {
        body_lines.push("No statistical behavioral outliers detected.".to_string());
    } else {
        for o in &snapshot.statistical_outliers {
            body_lines.push(format!(
                "[{}] {} | Actor: {} | Observed: {} vs Baseline: {} (Z={}) — {}",
                o.severity,
                o.metric,
                o.actor,
                o.observed_value,
                o.baseline_mean,
                o.z_score,
                o.explanation
            ));
        }
    }

    let export_input = crate::export::ExportDocumentInput {
        title: "ArkibVision Audit & Anomaly Inspection Report".to_string(),
        reference_code: format!("ANM-AUDIT-{}", &snapshot.generated_at[..10]),
        group: "Audit & Security Governance".to_string(),
        script_type: "english".to_string(),
        verified_by_human: false,
        hijri_date: String::new(),
        gregorian_date_masihi: snapshot.generated_at[..10].to_string(),
        jawi_text: String::new(),
        rumi_text: body_lines.join("\n"),
        summary: ai_summary,
        notes: "Generated by ArkibVision Pure-Rust Rules Engine & Statistical Outlier Detector."
            .to_string(),
    };

    let (bytes_res, content_type, ext) = if fmt == "docx" {
        (
            crate::export::build_docx_bytes(&export_input),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
        )
    } else {
        (
            crate::export::build_pdf_bytes(&export_input),
            "application/pdf",
            "pdf",
        )
    };

    match bytes_res {
        Ok(bytes) => {
            let fname = format!("arkib_audit_report_{}.{ext}", &snapshot.generated_at[..10]);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{fname}\""),
                )
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(reason) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
    }
}

// ─── User Click Preference Learning & Personalization Endpoints ──────────────

#[derive(Deserialize)]
struct UpdatePrefsPayload {
    pinned_categories: Option<Vec<String>>,
    pinned_sources: Option<Vec<String>>,
    pinned_folders: Option<Vec<String>>,
    custom_interests: Option<Vec<String>>,
}

async fn admin_api_preferences(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let user = cfg.admin_user.clone();
    let res = tokio::task::spawn_blocking(move || {
        let docs = archives::list_documents(&cfg);
        preferences::get_preferences_and_recommendations(&cfg, &user, &docs)
    })
    .await
    .unwrap_or_else(|_| json!({ "status": "error" }));
    Json(res)
}

async fn admin_track_preference(
    State(state): State<AdminState>,
    Json(event): Json<preferences::InteractionEvent>,
) -> Response {
    let cfg = state.service.cfg.clone();
    let user = cfg.admin_user.clone();
    match tokio::task::spawn_blocking(move || {
        if !event.item_slug.trim().is_empty() || !event.item_title.trim().is_empty() {
            let etype = if event.event_type.contains("person") {
                "profile"
            } else if event.event_type.contains("location") {
                "location"
            } else {
                "document"
            };
            analytics::record_admin_activity(
                &cfg,
                "item_check",
                &user,
                "",
                etype,
                &event.item_slug,
                &event.item_title,
                &event.category,
                event.tags.clone(),
            );
        }
        preferences::track_interaction(&cfg, &user, event)
    })
    .await
    {
        Ok(Ok(summary)) => (
            StatusCode::OK,
            Json(json!({ "status": "tracked", "preferences": summary })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_update_preferences(
    State(state): State<AdminState>,
    Json(payload): Json<UpdatePrefsPayload>,
) -> Response {
    let cfg = state.service.cfg.clone();
    let user = cfg.admin_user.clone();
    match tokio::task::spawn_blocking(move || {
        preferences::update_pinned_preferences(
            &cfg,
            &user,
            payload.pinned_categories,
            payload.pinned_sources,
            payload.pinned_folders,
            payload.custom_interests,
        )
    })
    .await
    {
        Ok(Ok(summary)) => (
            StatusCode::OK,
            Json(json!({ "status": "updated", "preferences": summary })),
        )
            .into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

async fn admin_reset_preferences(State(state): State<AdminState>) -> Response {
    let cfg = state.service.cfg.clone();
    let user = cfg.admin_user.clone();
    match tokio::task::spawn_blocking(move || preferences::reset_preferences(&cfg, &user)).await {
        Ok(Ok(res)) => (StatusCode::OK, Json(res)).into_response(),
        Ok(Err(reason)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        )
            .into_response(),
    }
}

// ─── User Behavior, Search History & Chatbot Hot-Topics Analytics ────────────

#[derive(Deserialize)]
struct TrackBehaviorPayload {
    #[serde(default)]
    source: String,
    #[serde(default)]
    query_text: String,
    #[serde(default)]
    entity_type: String,
    #[serde(default)]
    item_slug: String,
    #[serde(default)]
    item_title: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn admin_api_behavior_analytics(State(state): State<AdminState>) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let res = tokio::task::spawn_blocking(move || analytics::build_behavior_analytics(&cfg))
        .await
        .unwrap_or_else(|_| json!({ "status": "error" }));
    Json(res)
}

async fn admin_track_behavior_analytics(
    State(state): State<AdminState>,
    Json(payload): Json<TrackBehaviorPayload>,
) -> Json<Value> {
    let cfg = state.service.cfg.clone();
    let actor = cfg.admin_user.clone();
    let source = if payload.source.trim().is_empty() {
        "admin_search".to_string()
    } else {
        payload.source.trim().to_string()
    };
    let _ = tokio::task::spawn_blocking(move || {
        analytics::record_admin_activity(
            &cfg,
            &source,
            &actor,
            &payload.query_text,
            &payload.entity_type,
            &payload.item_slug,
            &payload.item_title,
            &payload.category,
            payload.tags,
        );
    })
    .await;
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_allowlist_enforces_strict_read_only_access() {
        assert!(is_guest_allowed_request(
            &Method::GET,
            "/guest/api/profiles"
        ));
        assert!(is_guest_allowed_request(
            &Method::GET,
            "/guest/api/locations"
        ));
        assert!(is_guest_allowed_request(
            &Method::GET,
            "/guest/api/documents"
        ));
        assert!(is_guest_allowed_request(
            &Method::GET,
            "/guest/api/documents/surat_01/export/pdf"
        ));
        assert!(is_guest_allowed_request(
            &Method::GET,
            "/guest/profile-images/anwar/01.jpg"
        ));
        assert!(is_guest_allowed_request(
            &Method::POST,
            "/guest/api/test-recognition"
        ));

        // Every write/delete/admin action must be denied for guests
        assert!(!is_guest_allowed_request(
            &Method::POST,
            "/guest/api/profiles"
        ));
        assert!(!is_guest_allowed_request(
            &Method::DELETE,
            "/guest/api/profiles"
        ));
        assert!(!is_guest_allowed_request(
            &Method::PATCH,
            "/guest/api/documents"
        ));
        assert!(!is_guest_allowed_request(
            &Method::GET,
            "/guest/api/requests"
        ));
        assert!(!is_guest_allowed_request(
            &Method::GET,
            "/guest/api/audit/dashboard"
        ));
        assert!(!is_guest_allowed_request(
            &Method::GET,
            "/admin/api/profiles"
        ));
    }

    #[test]
    fn guest_url_rewrite_converts_admin_media_paths() {
        let mut payload = json!([
            {
                "images": [{ "url": "/admin/profile-images/ali/01.jpg" }],
                "files": [{ "admin_url": "/admin/document-files/doc1/a.pdf" }],
                "loc": { "admin_url": "/admin/location-images/hall/01.png" }
            }
        ]);
        rewrite_admin_urls_for_guest(&mut payload);
        assert_eq!(
            payload[0]["images"][0]["url"],
            "/guest/profile-images/ali/01.jpg"
        );
        assert_eq!(
            payload[0]["files"][0]["admin_url"],
            "/guest/document-files/doc1/a.pdf"
        );
        assert_eq!(
            payload[0]["loc"]["admin_url"],
            "/guest/location-images/hall/01.png"
        );
    }
}
