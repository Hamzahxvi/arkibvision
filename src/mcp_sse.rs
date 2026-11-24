#![forbid(unsafe_code)]

use crate::service::RojakFaceService;
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
    pub service: Arc<RojakFaceService>,
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

pub fn mcp_router(service: Arc<RojakFaceService>) -> Router {
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

async fn handle_jsonrpc(service: &Arc<RojakFaceService>, req: Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

    // Notifications (no id) require no response.
    if id.is_none() || method.starts_with("notifications/") {
        return None;
    }
    let id = id.unwrap();

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
                    "name": "RojakFace-Service",
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
    service: &Arc<RojakFaceService>,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    match name {
        "rojakface_search_identity" => {
            let identity = arg_str(&args, "identity");
            let query = arg_str(&args, "query");
            let limit = arg_i64(&args, "limit", 0);
            let photos_only = arg_bool(&args, "photos_only", false);
            Ok(service
                .tool_search_identity(&identity, &query, limit, photos_only)
                .await)
        }
        other => {
            let svc = service.clone();
            let tool = other.to_string();
            tokio::task::spawn_blocking(move || match tool.as_str() {
                "rojakface_detect_faces" => {
                    Ok(svc.tool_detect_faces(&arg_str(&args, "image_path")))
                }
                "rojakface_ocr_document" => Ok(svc.tool_ocr_document(
                    &arg_str(&args, "image_path"),
                    arg_i64(&args, "max_chunks", 3),
                )),
                "rojakface_decode_jawi" => Ok(svc.tool_decode_jawi(&arg_str(&args, "text"))),
                "rojakface_identify_faces" => Ok(svc.tool_identify_faces(
                    &arg_str(&args, "image_path"),
                    arg_f32(&args, "threshold", 0.88),
                    arg_bool(&args, "save_new_image", true),
                )),
                "rojakface_request_enrollment" => Ok(svc.tool_request_enrollment(
                    &arg_str(&args, "identity"),
                    &arg_str(&args, "image_path"),
                    &arg_str(&args, "proposed_metadata"),
                    &arg_str(&args, "notes"),
                )),
                "rojakface_request_update_metadata" => Ok(svc.tool_request_update_metadata(
                    &arg_str(&args, "identity"),
                    &arg_str(&args, "proposed_metadata"),
                    &arg_str(&args, "notes"),
                    &arg_str(&args, "admin_key"),
                )),
                "rojakface_enroll_named_identity" => Ok(svc.tool_enroll_named_identity(
                    &arg_str(&args, "identity"),
                    &arg_str(&args, "image_path"),
                    &arg_str(&args, "admin_key"),
                    &arg_str(&args, "proposed_metadata"),
                )),
                "rojakface_enroll_or_learn" => Ok(svc.tool_enroll_or_learn(
                    &arg_str(&args, "image_path"),
                    arg_f32(&args, "threshold", 0.88),
                    &arg_str(&args, "admin_key"),
                )),
                "rojakface_list_pending_requests" => {
                    Ok(svc.tool_list_pending_requests(&arg_str(&args, "status")))
                }
                "rojakface_review_pending_request" => Ok(svc.tool_review_pending_request(
                    &arg_str(&args, "request_id"),
                    &arg_str(&args, "action"),
                    &arg_str(&args, "admin_key"),
                    &arg_str(&args, "comment"),
                )),
                "rojakface_rename_identity" => Ok(svc.tool_rename_identity(
                    &arg_str(&args, "old_identity"),
                    &arg_str(&args, "new_identity"),
                    &arg_str(&args, "admin_key"),
                )),
                "rojakface_list_identities" => Ok(svc.tool_list_identities()),
                "rojakface_delete_identity" => Ok(svc.tool_delete_identity(
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
            "name": "rojakface_detect_faces",
            "description": "Detect faces in an image using YuNet. Leave image_path empty for chat-attached images.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "default": "" }
                }
            }
        }),
        json!({
            "name": "rojakface_ocr_document",
            "description": "Slice a document or handwriting image along natural horizontal line valleys into contrast-enhanced chunks for accurate multilingual OCR.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "default": "" },
                    "max_chunks": { "type": "integer", "default": 3 }
                }
            }
        }),
        json!({
            "name": "rojakface_decode_jawi",
            "description": "Decode Jawi script or rough VLM transliterations into corrected 3-dot Jawi and natural Standard Bahasa Melayu (Rumi).",
            "inputSchema": {
                "type": "object",
                "required": ["text"],
                "properties": {
                    "text": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "rojakface_identify_faces",
            "description": "Identify faces against enrolled identities with strict anti-false-positive threshold (All Roles).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "default": "" },
                    "threshold": { "type": "number", "default": 0.88 },
                    "save_new_image": { "type": "boolean", "default": true }
                }
            }
        }),
        json!({
            "name": "rojakface_request_enrollment",
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
            "name": "rojakface_request_update_metadata",
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
            "name": "rojakface_enroll_named_identity",
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
            "name": "rojakface_enroll_or_learn",
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
            "name": "rojakface_list_pending_requests",
            "description": "List enrollment and metadata requests with simple #1, #2 numbering for one-word approval (All Roles).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": { "type": "string", "default": "pending" }
                }
            }
        }),
        json!({
            "name": "rojakface_review_pending_request",
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
            "name": "rojakface_rename_identity",
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
            "name": "rojakface_list_identities",
            "description": "List all enrolled identities currently stored in the vector database and their reference vector counts (All Roles).",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "rojakface_search_identity",
            "description": "Search the local profile database for an enrolled person (case-insensitive), with optional visual attribute query, photo count limit, and photos_only mode.",
            "inputSchema": {
                "type": "object",
                "required": ["identity"],
                "properties": {
                    "identity": { "type": "string" },
                    "query": { "type": "string", "default": "" },
                    "limit": { "type": "integer", "default": 0 },
                    "photos_only": { "type": "boolean", "default": false }
                }
            }
        }),
        json!({
            "name": "rojakface_delete_identity",
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
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_catalog_and_host_guard() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 14);
        for t in &tools {
            assert!(t["name"].as_str().unwrap_or("").starts_with("rojakface_"));
            assert_eq!(t["inputSchema"]["type"].as_str(), Some("object"));
        }
        assert!(is_allowed_host("localhost:8005"));
        assert!(is_allowed_host("host.docker.internal:8005"));
        assert!(is_allowed_host("127.0.0.1:8005"));
        assert!(!is_allowed_host("evil.example.com:8005"));
    }
}
