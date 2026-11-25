#![forbid(unsafe_code)]

use crate::archives;
use crate::config::{
    ADMIN_SESSION_COOKIE, ADMIN_SESSION_TTL_SECS, MIN_CLIP_IDENTIFY_THRESHOLD, is_document_ext,
    is_image_ext,
};
use crate::metadata::value_to_trimmed_string;
use crate::service::RojakFaceService;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Form, Multipart, Path as AxumPath, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Redirect, Response};
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
    pub service: Arc<RojakFaceService>,
    pub sessions: Arc<Mutex<HashMap<String, u64>>>,
    pub login_html: Arc<String>,
    pub dashboard_html: Arc<String>,
    pub test_recognition_html: Arc<String>,
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

fn render_login(login_html: &str, error: &str) -> Html<String> {
    let err_html = if error.is_empty() {
        String::new()
    } else {
        format!(r#"<div class="error">{error}</div>"#)
    };
    Html(login_html.replace("{{ERROR}}", &err_html))
}

async fn auth_middleware(State(state): State<AdminState>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if path == "/admin/login" || path == "/admin/logout" {
        return next.run(req).await;
    }
    if valid_session_token(req.headers(), &state.sessions).is_none() {
        if path.starts_with("/admin/api/")
            || path.starts_with("/admin/profile-images/")
            || path.starts_with("/admin/location-images/")
            || path.starts_with("/admin/document-files/")
            || path.starts_with("/admin/enhanced-images/")
        {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "status": "unauthorized",
                    "reason": "Please sign in to the Admin Portal.",
                })),
            )
                .into_response();
        }
        return render_login(&state.login_html, "").into_response();
    }
    next.run(req).await
}

pub fn admin_router(service: Arc<RojakFaceService>) -> Result<Router, String> {
    let tdir = &service.cfg.templates_dir;
    let login_html = fs::read_to_string(tdir.join("login.html"))
        .map_err(|e| format!("Missing login.html in {}: {e}", tdir.display()))?;
    let dashboard_html = fs::read_to_string(tdir.join("dashboard.html"))
        .map_err(|e| format!("Missing dashboard.html in {}: {e}", tdir.display()))?;
    let test_recognition_html = fs::read_to_string(tdir.join("test_recognition.html"))
        .map_err(|e| format!("Missing test_recognition.html in {}: {e}", tdir.display()))?;

    let state = AdminState {
        service,
        sessions: Arc::new(Mutex::new(HashMap::new())),
        login_html: Arc::new(login_html),
        dashboard_html: Arc::new(dashboard_html),
        test_recognition_html: Arc::new(test_recognition_html),
    };

    let router = Router::new()
        .route("/admin/login", get(admin_login_get).post(admin_login_post))
        .route("/admin/logout", get(admin_logout))
        .route("/admin", get(admin_dashboard))
        .route("/admin/", get(admin_dashboard))
        .route("/admin/test-recognition", get(admin_test_recognition_page))
        .route(
            "/admin/images/{img_type}/{request_id}",
            get(admin_serve_image),
        )
        .route("/admin/api/requests", get(admin_api_requests))
        .route(
            "/admin/api/profiles",
            get(admin_api_profiles).post(admin_create_profile),
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
        // Managed Archival Location Folders
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
        // Managed Archival Document Folders
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
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state);

    Ok(router)
}

#[derive(Deserialize)]
struct LoginForm {
    username: Option<String>,
    password: Option<String>,
}

async fn admin_login_get(State(state): State<AdminState>) -> Html<String> {
    render_login(&state.login_html, "")
}

async fn admin_login_post(
    State(state): State<AdminState>,
    Form(form): Form<LoginForm>,
) -> Response {
    let user = form.username.unwrap_or_default();
    let pass = form.password.unwrap_or_default();
    let ok = constant_time_eq(user.trim(), &state.service.cfg.admin_user)
        && constant_time_eq(&pass, &state.service.cfg.admin_pass);
    if !ok {
        return render_login(&state.login_html, "Invalid username or password.").into_response();
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
        "{ADMIN_SESSION_COOKIE}={token}; Max-Age={ADMIN_SESSION_TTL_SECS}; HttpOnly; SameSite=Lax; Path=/admin"
    );
    let mut resp = Redirect::to("/admin").into_response();
    if let Ok(val) = HeaderValue::from_str(&cookie) {
        resp.headers_mut().append(header::SET_COOKIE, val);
    }
    resp
}

async fn admin_logout(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Some(tok) = valid_session_token(&headers, &state.sessions) {
        state.sessions.lock().expect("sessions lock").remove(&tok);
    }
    let mut resp = render_login(&state.login_html, "You have been signed out.").into_response();
    if let Ok(val) = HeaderValue::from_str(&format!(
        "{ADMIN_SESSION_COOKIE}=; Max-Age=0; HttpOnly; SameSite=Lax; Path=/admin"
    )) {
        resp.headers_mut().append(header::SET_COOKIE, val);
    }
    resp
}

async fn admin_dashboard(State(state): State<AdminState>) -> Html<String> {
    Html((*state.dashboard_html).clone())
}

async fn admin_test_recognition_page(State(state): State<AdminState>) -> Html<String> {
    Html((*state.test_recognition_html).clone())
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
        let counts = svc.store.list_counts();
        let mut entries: Vec<(String, usize)> = counts.into_iter().collect();
        entries.sort_by_key(|(name, _)| name.to_ascii_lowercase());
        entries
            .into_iter()
            .map(|(n, count)| svc.admin_profile_record_with_count(&n, count))
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

async fn admin_test_recognition(
    State(state): State<AdminState>,
    mut multipart: Multipart,
) -> Response {
    let mut upload_bytes: Option<Vec<u8>> = None;
    let mut upload_name = String::new();
    let mut threshold = MIN_CLIP_IDENTIFY_THRESHOLD;

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
    let ext = Path::new(&upload_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !is_image_ext(Path::new(&upload_name)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "reason": "Use JPG, PNG, WEBP, or BMP images." })),
        )
            .into_response();
    }

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
    if !is_image_ext(Path::new(&upload_name)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "Use JPG, PNG, WEBP, or BMP images." })),
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
    if let Some(Value::Object(other)) = payload.get("other_details") {
        metadata.insert("other_details".to_string(), Value::Object(other.clone()));
        metadata.insert("_replace_other".to_string(), Value::Bool(true));
    }

    let svc = state.service.clone();
    let (status, body) = tokio::task::spawn_blocking(move || {
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
        (
            StatusCode::OK,
            json!({ "status": "updated", "profile": svc.admin_profile_record(&identity) }),
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
    let res = tokio::task::spawn_blocking(move || {
        svc.tool_delete_identity(&identity, true, &svc.cfg.admin_key)
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
    if !is_image_ext(Path::new(&upload_name)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "rejected", "reason": "Use JPG, PNG, WEBP, or BMP images." })),
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
    let (status, body) = tokio::task::spawn_blocking(move || {
        let res = svc.admin_add_profile_image(&identity, &tp);
        let _ = fs::remove_file(&tp);
        if res.get("status").and_then(Value::as_str) != Some("image_added") {
            return (StatusCode::BAD_REQUEST, res);
        }
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

async fn admin_approve(
    State(state): State<AdminState>,
    AxumPath(req_id): AxumPath<String>,
) -> Response {
    let svc = state.service.clone();
    let res = tokio::task::spawn_blocking(move || {
        svc.tool_review_pending_request(&req_id, "approve", &svc.cfg.admin_key, "")
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
    let list = tokio::task::spawn_blocking(move || archives::list_locations(&cfg))
        .await
        .unwrap_or_default();
    Json(Value::Array(list))
}

async fn admin_create_location(
    State(state): State<AdminState>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || archives::save_location(&cfg, None, &payload)).await {
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
        archives::save_location(&cfg, Some(&clean_slug), &payload)
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
        archives::delete_location(&svc.cfg, &svc.location_store, &clean_slug)
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
    let list = tokio::task::spawn_blocking(move || archives::list_documents(&cfg))
        .await
        .unwrap_or_default();
    Json(Value::Array(list))
}

async fn admin_create_document(
    State(state): State<AdminState>,
    Json(payload): Json<Map<String, Value>>,
) -> Response {
    let cfg = state.service.cfg.clone();
    match tokio::task::spawn_blocking(move || archives::save_document(&cfg, None, &payload)).await {
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
        archives::save_document(&cfg, Some(&clean_slug), &payload)
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
    match tokio::task::spawn_blocking(move || archives::delete_document(&cfg, &clean_slug)).await {
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
        archives::add_document_file(&cfg, &clean_slug, &upload_name, &bytes, force)
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
    match archives::delete_document_file(&state.service.cfg, clean_slug, clean_name) {
        Ok(doc) => (
            StatusCode::OK,
            Json(json!({ "status": "file_deleted", "document": doc })),
        )
            .into_response(),
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
    let svc = state.service.clone();
    let tp = temp_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let res = svc.tool_assess_conservation(&tp.to_string_lossy());
        let _ = fs::remove_file(&tp);
        res
    })
    .await
    .unwrap_or_else(|e| json!({ "status": "error", "reason": e.to_string() }));
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
