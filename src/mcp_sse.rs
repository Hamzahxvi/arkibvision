#![forbid(unsafe_code)]

use crate::service::ArkibService;
use axum::extract::{Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

#[derive(Clone)]
pub struct McpState {
    pub service: Arc<ArkibService>,
    pub sessions: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
}

#[derive(Deserialize)]
struct SessionQuery {
    session_id: String,
}

/// Automatically removes an SSE session from the map when the client disconnects.
struct ManagedSseStream<S> {
    inner: S,
    session_id: String,
    sessions: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
}

impl<S: Stream<Item = Result<Event, Infallible>> + Unpin> Stream for ManagedSseStream<S> {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

impl<S> Drop for ManagedSseStream<S> {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.sessions.lock() {
            guard.remove(&self.session_id);
        }
    }
}

fn is_allowed_host(host: &str) -> bool {
    let bare = host
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        bare.as_str(),
        "localhost" | "127.0.0.1" | "host.docker.internal" | "0.0.0.0" | "[::1]"
    )
}

async fn dns_rebinding_guard(req: Request, next: Next) -> Response {
    if let Some(host_val) = req.headers().get(header::HOST)
        && let Ok(host_str) = host_val.to_str()
        && !is_allowed_host(host_str)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Disallowed Host header" })),
        )
            .into_response();
    }
    next.run(req).await
}

pub fn mcp_router(service: Arc<ArkibService>) -> Router {
    let state = McpState {
        service,
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };
    Router::new()
        .route("/sse", get(sse_handler))
        .route("/messages", post(messages_handler))
        .route("/messages/", post(messages_handler))
        .layer(middleware::from_fn(dns_rebinding_guard))
        .with_state(state)
}

async fn sse_handler(
    State(state): State<McpState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let session_id = Uuid::new_v4().simple().to_string();
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    state
        .sessions
        .lock()
        .expect("sessions lock")
        .insert(session_id.clone(), tx);

    let endpoint_event = Event::default()
        .event("endpoint")
        .data(format!("/messages/?session_id={session_id}"));

    let initial =
        futures_util::stream::once(async move { Ok::<Event, Infallible>(endpoint_event) });
    let messages = UnboundedReceiverStream::new(rx)
        .map(|payload| Ok::<Event, Infallible>(Event::default().event("message").data(payload)));

    let stream = ManagedSseStream {
        inner: Box::pin(initial.chain(messages)),
        session_id,
        sessions: state.sessions.clone(),
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

async fn messages_handler(
    State(state): State<McpState>,
    Query(query): Query<SessionQuery>,
    Json(body): Json<Value>,
) -> StatusCode {
    let tx = {
        let guard = state.sessions.lock().expect("sessions lock");
        guard.get(&query.session_id).cloned()
    };
    let Some(tx) = tx else {
        return StatusCode::NOT_FOUND;
    };

    let service = state.service.clone();
    tokio::spawn(async move {
        if let Some(reply) = handle_jsonrpc(&service, body).await
            && let Ok(text) = serde_json::to_string(&reply)
        {
            let _ = tx.send(text);
        }
    });

    StatusCode::ACCEPTED
}

async fn handle_jsonrpc(service: &Arc<ArkibService>, req: Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

    // Notifications (no id) require no response.
    let id = id?;
    if method.starts_with("notifications/") {
        return None;
    }

    let result = match method {
        "initialize" => {
            let proto = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2024-11-05");
            Ok(json!({
                "protocolVersion": proto,
                "capabilities": {
                    "tools": { "listChanged": false },
                },
                "serverInfo": {
                    "name": "ArkibVision-Service",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let tool_name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match dispatch_tool(service, &tool_name, args).await {
                Ok(val) => {
                    let text = serde_json::to_string_pretty(&val).unwrap_or_else(|_| "{}".into());
                    Ok(json!({
                        "content": [{ "type": "text", "text": text }],
                        "isError": false,
                    }))
                }
                Err(msg) => Ok(json!({
                    "content": [{ "type": "text", "text": msg }],
                    "isError": true,
                })),
            }
        }
        _ => Err((-32601, format!("Method not found: {method}"))),
    };

    Some(match result {
        Ok(res) => json!({ "jsonrpc": "2.0", "id": id, "result": res }),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }),
    })
}

fn arg_str(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn arg_f32(args: &Value, key: &str, default: f32) -> f32 {
    args.get(key)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(default)
}

fn arg_i64(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key).and_then(Value::as_i64).unwrap_or(default)
}

fn arg_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

async fn dispatch_tool(
    service: &Arc<ArkibService>,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    match name {
        "arkib_search_identity" => {
            let identity = arg_str(&args, "identity");
            let query = arg_str(&args, "query");
            let limit = arg_i64(&args, "limit", 0);
            let photos_only = arg_bool(&args, "photos_only", false);
            Ok(service
                .tool_search_identity(&identity, &query, limit, photos_only)
                .await)
        }
        "arkib_summarize_document" => {
            let file_path = arg_str(&args, "file_path");
            let text = arg_str(&args, "text");
            let language = arg_str(&args, "language");
            Ok(service
                .tool_summarize_document(&file_path, &text, &language)
                .await)
        }
        "arkib_system_health" => Ok(service.tool_system_health().await),
        "arkib_analyze_image" => {
            let image_path = arg_str(&args, "image_path");
            Ok(service.tool_analyze_image(&image_path).await)
        }
        "arkib_detect_authenticity" => {
            let file_path = arg_str(&args, "file_path");
            Ok(service.tool_detect_authenticity(&file_path).await)
        }
        "arkib_assess_conservation" => {
            let image_path = arg_str(&args, "image_path");
            Ok(service.tool_assess_conservation(&image_path).await)
        }
        other => {
            let svc = service.clone();
            let tool = other.to_string();
            tokio::task::spawn_blocking(move || match tool.as_str() {
                "arkib_detect_faces" => Ok(svc.tool_detect_faces(&arg_str(&args, "image_path"))),
                "arkib_ocr_document" => Ok(svc.tool_ocr_document(
                    &arg_str(&args, "image_path"),
                    arg_i64(&args, "max_chunks", 5),
                )),
                "arkib_decode_jawi" => Ok(svc.tool_decode_jawi(&arg_str(&args, "text"))),
                "arkib_decode_ancient_script" => Ok(svc.tool_decode_ancient_script(
                    &arg_str(&args, "text"),
                    &arg_str(&args, "script"),
                )),
                "arkib_enhance_image" => {
                    Ok(svc
                        .tool_enhance_image(&arg_str(&args, "image_path"), &arg_str(&args, "mode")))
                }
                "arkib_verify_integrity" => {
                    Ok(svc.tool_verify_integrity(arg_bool(&args, "update_manifest", true)))
                }
                "arkib_search_locations" => Ok(svc.tool_search_locations(&arg_str(&args, "query"))),
                "arkib_search_documents" => {
                    Ok(svc
                        .tool_search_documents(&arg_str(&args, "query"), &arg_str(&args, "script")))
                }
                "arkib_identify_faces" => Ok(svc.tool_identify_faces(
                    &arg_str(&args, "image_path"),
                    arg_f32(&args, "threshold", 0.0),
                    arg_bool(&args, "save_new_image", true),
                )),
                "arkib_request_enrollment" => Ok(svc.tool_request_enrollment(
                    &arg_str(&args, "identity"),
                    &arg_str(&args, "image_path"),
                    &arg_str(&args, "proposed_metadata"),
                    &arg_str(&args, "notes"),
                )),
                "arkib_request_update_metadata" => Ok(svc.tool_request_update_metadata(
                    &arg_str(&args, "identity"),
                    &arg_str(&args, "proposed_metadata"),
                    &arg_str(&args, "notes"),
                    &arg_str(&args, "admin_key"),
                )),
                "arkib_enroll_named_identity" => Ok(svc.tool_enroll_named_identity(
                    &arg_str(&args, "identity"),
                    &arg_str(&args, "image_path"),
                    &arg_str(&args, "admin_key"),
                    &arg_str(&args, "proposed_metadata"),
                )),
                "arkib_enroll_or_learn" => Ok(svc.tool_enroll_or_learn(
                    &arg_str(&args, "image_path"),
                    arg_f32(&args, "threshold", 0.0),
                    &arg_str(&args, "admin_key"),
                )),
                "arkib_list_pending_requests" => {
                    Ok(svc.tool_list_pending_requests(&arg_str(&args, "status")))
                }
                "arkib_review_pending_request" => Ok(svc.tool_review_pending_request(
                    &arg_str(&args, "request_id"),
                    &arg_str(&args, "action"),
                    &arg_str(&args, "admin_key"),
                    &arg_str(&args, "comment"),
                )),
                "arkib_rename_identity" => Ok(svc.tool_rename_identity(
                    &arg_str(&args, "old_identity"),
                    &arg_str(&args, "new_identity"),
                    &arg_str(&args, "admin_key"),
                )),
                "arkib_list_identities" => Ok(svc.tool_list_identities()),
                "arkib_delete_identity" => Ok(svc.tool_delete_identity(
                    &arg_str(&args, "identity"),
                    arg_bool(&args, "purge_files", true),
                    &arg_str(&args, "admin_key"),
                )),
                _ => Err(format!("Unknown tool: {tool}")),
            })
            .await
            .map_err(|e| format!("Tool worker error: {e}"))?
        }
    }
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "arkib_detect_faces",
            "description": "Detect faces in an image using YuNet. Leave image_path empty for chat-attached images.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "arkib_ocr_document",
            "description": "Read or transcribe text, handwriting, letters, certificates, or old manuscripts (Modern/Old Jawi, Pallava, Kawi, Rencong, Malay, English, Tamil, Mandarin, Sanskrit) from an uploaded image by slicing dense pages into 3-4 line strips to prevent long-context hallucination. Always leave image_path empty (\"\") for chat uploads, and ALWAYS pass extracted Jawi text into arkib_decode_jawi.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "default": "", "description": "Leave empty (\"\") for chat-uploaded images. Never invent 'image.jpg'." },
                    "max_chunks": { "type": "integer", "default": 5, "description": "Maximum horizontal line-valley strips (1-8; default 5 keeps dense Jawi pages to ~3-4 lines per strip)." }
                }
            }
        }),
        json!({
            "name": "arkib_decode_jawi",
            "description": "MANDATORY deterministic decoder for ALL Modern Jawi and Old Classical Jawi (Jawi Lama / Hikayat / Surat Diraja / multi-font Unicode & calligraphic kashida variants). Always call this tool on Jawi text or Jawi OCR output instead of translating from memory, and strictly follow its standard_malay_rumi_text without inventing lines.",
            "inputSchema": {
                "type": "object",
                "required": ["text"],
                "properties": {
                    "text": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "arkib_identify_faces",
            "description": "Recognize or check who is in an uploaded photo against saved people in the database ('who is this?', 'do we know him/her?', 'siapa dalam gambar ini?'). Always leave image_path empty (\"\") for chat uploads.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "default": "", "description": "Leave empty (\"\") for chat-uploaded images. Never invent 'image.jpg'." },
                    "threshold": { "type": "number", "default": 0.88 },
                    "save_new_image": { "type": "boolean", "default": true }
                }
            }
        }),
        json!({
            "name": "arkib_request_enrollment",
            "description": "Guest / User tool: Submit an enrollment request for an unknown face. Staged for Admin verification.",
            "inputSchema": {
                "type": "object",
                "required": ["identity"],
                "properties": {
                    "identity": { "type": "string" },
                    "image_path": { "type": "string", "default": "" },
                    "proposed_metadata": { "type": "string", "default": "" },
                    "notes": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "arkib_request_update_metadata",
            "description": "Request a metadata update. Guest requests are queued for Admin Portal approval; do not ask the user for an admin key.",
            "inputSchema": {
                "type": "object",
                "required": ["identity", "proposed_metadata"],
                "properties": {
                    "identity": { "type": "string" },
                    "proposed_metadata": { "type": "string" },
                    "notes": { "type": "string", "default": "" },
                    "admin_key": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "arkib_enroll_named_identity",
            "description": "Enroll a face for an administrator. Guest requests are queued for Admin Portal approval; never ask the user for an admin key.",
            "inputSchema": {
                "type": "object",
                "required": ["identity"],
                "properties": {
                    "identity": { "type": "string" },
                    "image_path": { "type": "string", "default": "" },
                    "admin_key": { "type": "string", "default": "" },
                    "proposed_metadata": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "arkib_enroll_or_learn",
            "description": "Direct unsupervised learning. Restricted to admins to avoid false enrollments.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "default": "" },
                    "threshold": { "type": "number", "default": 0.88 },
                    "admin_key": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "arkib_list_pending_requests",
            "description": "List enrollment and metadata requests with simple #1, #2 numbering for one-word approval (All Roles).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": { "type": "string", "default": "pending" }
                }
            }
        }),
        json!({
            "name": "arkib_review_pending_request",
            "description": "Approve or reject a pending request by number ('1'), person name ('Farid Azizan'), 'latest', or full ID (Admin only).",
            "inputSchema": {
                "type": "object",
                "required": ["request_id", "action"],
                "properties": {
                    "request_id": { "type": "string" },
                    "action": { "type": "string" },
                    "admin_key": { "type": "string", "default": "" },
                    "comment": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "arkib_rename_identity",
            "description": "Rename an enrolled identity in vector store and filesystem (Admin only).",
            "inputSchema": {
                "type": "object",
                "required": ["old_identity", "new_identity"],
                "properties": {
                    "old_identity": { "type": "string" },
                    "new_identity": { "type": "string" },
                    "admin_key": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "arkib_list_identities",
            "description": "List all enrolled identities currently stored in the vector database and their reference vector counts (All Roles).",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "arkib_search_identity",
            "description": "Search the local profile database for an enrolled person (case-insensitive), with optional visual attribute query, photo count limit, and photos_only mode. Put the person's name in `identity` and ONLY clothing/visual filters (e.g. 'pakai songkok', 'cermin mata') in `query`.",
            "inputSchema": {
                "type": "object",
                "required": ["identity"],
                "properties": {
                    "identity": { "type": "string", "description": "Person's name ONLY (e.g. 'Lando Zawawi'). Never omit this." },
                    "query": { "type": "string", "default": "", "description": "Optional visual/clothing filter ONLY (e.g. 'pakai songkok', 'cermin mata', 'wearing glasses'). Do NOT put the person's name here." },
                    "limit": { "type": "integer", "default": 0 },
                    "photos_only": { "type": "boolean", "default": false }
                }
            }
        }),
        json!({
            "name": "arkib_delete_identity",
            "description": "Remove an enrolled identity and all of its vectors from the database (Admin only).",
            "inputSchema": {
                "type": "object",
                "required": ["identity"],
                "properties": {
                    "identity": { "type": "string" },
                    "purge_files": { "type": "boolean", "default": true },
                    "admin_key": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "arkib_decode_ancient_script",
            "description": "Recognize, decode, and transliterate historical Malay & Southeast Asian scripts (Jawi, Pallava, Kawi, Rencong / Surat Ulu) into searchable digital text with archival historical notes.",
            "inputSchema": {
                "type": "object",
                "required": ["text"],
                "properties": {
                    "text": { "type": "string" },
                    "script": { "type": "string", "default": "auto", "description": "Optional script hint: auto, jawi, pallava, kawi, or rencong" }
                }
            }
        }),
        json!({
            "name": "arkib_analyze_image",
            "description": "All-in-one smart photo & archival check: use for general requests like 'check this', 'what is this?', 'what building/place is this?', 'where was this taken?', 'tell me about this photo', 'semak gambar ini'. Checks file details, date/time/location, authenticity/duplicates, quality/damage, and matches saved buildings or archive halls. Always leave image_path empty (\"\") for chat uploads.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "default": "", "description": "Leave empty (\"\") for chat-uploaded images. Never invent 'image.jpg'." }
                }
            }
        }),
        json!({
            "name": "arkib_detect_authenticity",
            "description": "Deep multi-layer authenticity, AI watermark, and fraud check: inspects C2PA/JUMBF Content Credentials, IPTC DigitalSourceType, XMP/EXIF/PNG metadata, bare stripped JPEG headers, pixel-level ELA splicing/noise/blockiness/LSB watermark probes, archive duplicates, AND runs a dedicated local visual AI audit for visible AI/stock watermarks, impossible composites, and garbled text ('is this real or fake?', 'is this AI?', 'check watermark/metadata', 'do we already have this?', 'gambar ini tulen atau palsu?'). Always leave file_path empty (\"\") for chat uploads.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "default": "", "description": "Leave empty (\"\") for chat-uploaded files/images. Never invent 'image.jpg'." }
                }
            }
        }),
        json!({
            "name": "arkib_summarize_document",
            "description": "Summarize or explain a long document or pasted text in simple points ('summarize this', 'what is this document about?', 'give me the main points', 'ringkaskan dokumen ini'). Extracts key topics, names, dates, events, and authenticity status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "default": "", "description": "Leave empty (\"\") for chat-uploaded files or when using raw text." },
                    "text": { "type": "string", "default": "" },
                    "language": { "type": "string", "default": "auto" }
                }
            }
        }),
        json!({
            "name": "arkib_search_locations",
            "description": "Find or list saved places, buildings, landmarks, rooms, galleries, or wings (e.g. Arkib Negara Malaysia / National Archives of Malaysia). Use for 'show locations', 'list places', 'senarai lokasi', 'cari dewan'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "arkib_search_documents",
            "description": "Find or list saved archival documents, manuscripts, letters, or records ('show documents', 'find files about...', 'senarai dokumen', 'cari manuskrip').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "default": "" },
                    "script": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "arkib_assess_conservation",
            "description": "Check photo/scan quality (clarity, brightness, readability), spot damage (tears, fading, mould/fungus, yellowing/old paper) rated light/medium/heavy, check if real/AI/duplicate, and suggest how to repair, clean, dry, coat, or preserve the item ('is this damaged/spoilt?', 'how to fix/preserve this old document?', 'gambar ini rosak tak?', 'semak kualiti & kerosakan'). Always leave image_path empty (\"\") for chat uploads.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "default": "", "description": "Leave empty (\"\") for chat-uploaded images. Never invent 'image.jpg'." }
                }
            }
        }),
        json!({
            "name": "arkib_enhance_image",
            "description": "Improve, clean up, brighten, sharpen, or restore a blurry, dark, faded, or yellowed photo/scan ('make this clearer', 'fix this dark/old photo', 'brighten/sharpen this', 'jelaskan/baiki gambar ini'). Always leave image_path empty (\"\") for chat uploads.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "default": "", "description": "Leave empty (\"\") for chat-uploaded images. Never invent 'image.jpg'." },
                    "mode": { "type": "string", "default": "auto", "description": "auto, manuscript_ocr, deyellow_restore, contrast_lighting, or denoise_sharpen" }
                }
            }
        }),
        json!({
            "name": "arkib_verify_integrity",
            "description": "Check if all saved archive files are safe, intact, not corrupted or changed, and find any duplicate files ('are our files safe?', 'check for broken/corrupted/duplicate files', 'semak fail rosak atau pendua', 'audit integriti fail').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "update_manifest": { "type": "boolean", "default": true }
                }
            }
        }),
        json!({
            "name": "arkib_system_health",
            "description": "Check how the system/server is running: speed, memory, CPU, storage space, problems/overload, and what actions to take ('how is the system doing?', 'is the server okay or slow?', 'any system issues?', 'sistem ok ke?', 'kenapa lambat?').",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_catalog_and_host_guard() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 24);
        for t in &tools {
            assert!(t["name"].as_str().unwrap_or("").starts_with("arkib_"));
            assert_eq!(t["inputSchema"]["type"].as_str(), Some("object"));
        }
        assert!(is_allowed_host("localhost:8005"));
        assert!(is_allowed_host("host.docker.internal:8005"));
        assert!(is_allowed_host("127.0.0.1:8005"));
        assert!(!is_allowed_host("evil.example.com:8005"));
    }
}
