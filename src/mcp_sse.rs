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
use std::sync::{Arc, Mutex, OnceLock};
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

pub(crate) fn is_allowed_host(host: &str) -> bool {
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
            crate::analytics::record_mcp_tool_usage(&service.cfg, &tool_name, &args);
            match dispatch_tool(service, &tool_name, args).await {
                Ok(val) => {
                    let compact = compact_mcp_payload(val);
                    let text = serde_json::to_string(&compact).unwrap_or_else(|_| "{}".into());
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
                "arkib_compare_before_after" => Ok(svc.tool_compare_before_after(
                    &arg_str(&args, "document"),
                    &arg_str(&args, "before_path"),
                    &arg_str(&args, "after_path"),
                    &arg_str(&args, "title"),
                )),
                _ => Err(format!("Unknown tool: {tool}")),
            })
            .await
            .map_err(|e| format!("Tool worker error: {e}"))?
        }
    }
}

/// Token-efficiency sanitizer for MCP tool responses:
/// - Strips `null`, empty string `""`, and empty array `[]` noise keys (keeping essential fields)
/// - Guards against runaway raw text dumps (>4,000 chars) overflowing the 16k local LLM window
/// - Combined with compact `serde_json::to_string`, cuts MCP tool response tokens by ~35–50%.
pub(crate) fn compact_mcp_payload(val: Value) -> Value {
    const MAX_MCP_TEXT_CHARS: usize = 4_000;
    const KEEP_IF_EMPTY: &[&str] = &[
        "status",
        "matches",
        "detections",
        "identity",
        "standard_malay_rumi_text",
        "canvas_markdown_link",
    ];
    const TRUNCATE_KEYS: &[&str] = &["extracted_text", "raw_text", "ocr_text", "document_text"];

    match val {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                let keep_empty = KEEP_IF_EMPTY.contains(&k.as_str());
                let cleaned = match v {
                    Value::String(s) if TRUNCATE_KEYS.contains(&k.as_str()) => {
                        let char_cnt = s.chars().count();
                        if char_cnt > MAX_MCP_TEXT_CHARS {
                            let head: String = s.chars().take(MAX_MCP_TEXT_CHARS).collect();
                            Value::String(format!(
                                "{head}... [truncated {char_cnt} chars; full text in archive]"
                            ))
                        } else {
                            Value::String(s)
                        }
                    }
                    other => compact_mcp_payload(other),
                };
                if !keep_empty {
                    match &cleaned {
                        Value::Null => continue,
                        Value::String(s) if s.is_empty() => continue,
                        Value::Array(a) if a.is_empty() => continue,
                        Value::Object(o) if o.is_empty() => continue,
                        _ => {}
                    }
                }
                out.insert(k, cleaned);
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(compact_mcp_payload).collect()),
        other => other,
    }
}

pub fn tool_definitions() -> Vec<Value> {
    static TOOLS: OnceLock<Vec<Value>> = OnceLock::new();
    TOOLS
        .get_or_init(|| {
            vec![
                json!({
                    "name": "arkib_detect_faces",
                    "description": "Detect faces (YuNet). Pass image_path=\"\" for uploads.",
                    "inputSchema": { "type": "object", "properties": { "image_path": { "type": "string" } } }
                }),
                json!({
                    "name": "arkib_ocr_document",
                    "description": "Slice manuscript/doc image into OCR strips (rubrication + Sauvola). Pass image_path=\"\" for uploads; send Jawi to arkib_decode_jawi.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "image_path": { "type": "string" },
                            "max_chunks": { "type": "integer", "default": 5 }
                        }
                    }
                }),
                json!({
                    "name": "arkib_decode_jawi",
                    "description": "Deterministic Modern/Old Jawi-to-Rumi + Hijri colophon decoder. Always call for Jawi.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["text"],
                        "properties": { "text": { "type": "string" } }
                    }
                }),
                json!({
                    "name": "arkib_identify_faces",
                    "description": "Identify faces against enrolled people. Pass image_path=\"\" for uploads.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "image_path": { "type": "string" },
                            "threshold": { "type": "number" },
                            "save_new_image": { "type": "boolean", "default": true }
                        }
                    }
                }),
                json!({
                    "name": "arkib_request_enrollment",
                    "description": "Stage Guest face enrollment (REQ-*) for Admin approval.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["identity"],
                        "properties": {
                            "identity": { "type": "string" },
                            "image_path": { "type": "string" },
                            "proposed_metadata": { "type": "string" },
                            "notes": { "type": "string" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_request_update_metadata",
                    "description": "Stage profile/ID metadata update for Admin approval.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["identity", "proposed_metadata"],
                        "properties": {
                            "identity": { "type": "string" },
                            "proposed_metadata": { "type": "string" },
                            "notes": { "type": "string" },
                            "admin_key": { "type": "string" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_enroll_named_identity",
                    "description": "Admin direct face enrollment (auto-stages for Guests).",
                    "inputSchema": {
                        "type": "object",
                        "required": ["identity"],
                        "properties": {
                            "identity": { "type": "string" },
                            "image_path": { "type": "string" },
                            "admin_key": { "type": "string" },
                            "proposed_metadata": { "type": "string" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_enroll_or_learn",
                    "description": "Admin batch face learning.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "image_path": { "type": "string" },
                            "threshold": { "type": "number" },
                            "admin_key": { "type": "string" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_list_pending_requests",
                    "description": "List pending enrollment/metadata/OCR requests.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "status": { "type": "string", "default": "pending" } }
                    }
                }),
                json!({
                    "name": "arkib_review_pending_request",
                    "description": "Admin approve/reject pending request by ID, index, name, or 'latest'.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["request_id", "action"],
                        "properties": {
                            "request_id": { "type": "string" },
                            "action": { "type": "string" },
                            "admin_key": { "type": "string" },
                            "comment": { "type": "string" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_rename_identity",
                    "description": "Admin rename an enrolled person.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["old_identity", "new_identity"],
                        "properties": {
                            "old_identity": { "type": "string" },
                            "new_identity": { "type": "string" },
                            "admin_key": { "type": "string" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_list_identities",
                    "description": "List enrolled people and vector counts.",
                    "inputSchema": { "type": "object", "properties": {} }
                }),
                json!({
                    "name": "arkib_search_identity",
                    "description": "Search person profile & photos (`identity`=name, `query`=clothing/visual filter only). Leave `photos_only=false` for 'who is' / profile questions.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["identity"],
                        "properties": {
                            "identity": { "type": "string" },
                            "query": { "type": "string" },
                            "limit": { "type": "integer" },
                            "photos_only": { "type": "boolean", "default": false, "description": "Set true ONLY if user asks for photos only without profile details." }
                        }
                    }
                }),
                json!({
                    "name": "arkib_delete_identity",
                    "description": "Admin delete enrolled person and vectors.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["identity"],
                        "properties": {
                            "identity": { "type": "string" },
                            "purge_files": { "type": "boolean", "default": true },
                            "admin_key": { "type": "string" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_decode_ancient_script",
                    "description": "Transliterate Jawi/Pallava/Kawi/Rencong to Rumi.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["text"],
                        "properties": {
                            "text": { "type": "string" },
                            "script": { "type": "string", "default": "auto" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_analyze_image",
                    "description": "Photo check: EXIF/GPS, location match, authenticity, quality, damage. Pass image_path=\"\" for uploads.",
                    "inputSchema": { "type": "object", "properties": { "image_path": { "type": "string" } } }
                }),
                json!({
                    "name": "arkib_detect_authenticity",
                    "description": "4-layer authenticity, AI watermark (C2PA/EXIF/ELA/LSB), and duplicate check. Pass file_path=\"\" for uploads.",
                    "inputSchema": { "type": "object", "properties": { "file_path": { "type": "string" } } }
                }),
                json!({
                    "name": "arkib_summarize_document",
                    "description": "Extract & summarize TXT/DOCX/PDF/image (topics, names, dates, Hijri, authenticity).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string" },
                            "text": { "type": "string" },
                            "language": { "type": "string", "default": "auto" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_search_locations",
                    "description": "Search saved archival buildings, halls, wings, landmarks.",
                    "inputSchema": { "type": "object", "properties": { "query": { "type": "string" } } }
                }),
                json!({
                    "name": "arkib_search_documents",
                    "description": "Search saved archival documents and manuscripts.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "script": { "type": "string" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_assess_conservation",
                    "description": "Assess scan quality, damage (tear/fade/mold), authenticity, and healing steps. Pass image_path=\"\" for uploads.",
                    "inputSchema": { "type": "object", "properties": { "image_path": { "type": "string" } } }
                }),
                json!({
                    "name": "arkib_enhance_image",
                    "description": "Enhance dark/blurry/faded/yellowed image (auto|manuscript_ocr|deyellow_restore|contrast_lighting|denoise_sharpen). Pass image_path=\"\" for uploads.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "image_path": { "type": "string" },
                            "mode": { "type": "string", "default": "auto" }
                        }
                    }
                }),
                json!({
                    "name": "arkib_verify_integrity",
                    "description": "Audit SHA-256 integrity and duplicates across archive folders.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "update_manifest": { "type": "boolean", "default": true } }
                    }
                }),
                json!({
                    "name": "arkib_system_health",
                    "description": "Live CPU, RAM, Disk, Ollama latency, and workload status.",
                    "inputSchema": { "type": "object", "properties": {} }
                }),
                json!({
                    "name": "arkib_compare_before_after",
                    "description": "Open Canvas Before vs After studio for images/files or archived docs (changed pages, Conservation/Integrity, Intake Metadata, Migration Fidelity).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "document": { "type": "string" },
                            "before_path": { "type": "string" },
                            "after_path": { "type": "string" },
                            "title": { "type": "string" }
                        }
                    }
                }),
            ]
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_catalog_and_host_guard() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 25);
        for t in &tools {
            assert!(t["name"].as_str().unwrap_or("").starts_with("arkib_"));
            assert_eq!(t["inputSchema"]["type"].as_str(), Some("object"));
        }
        assert!(is_allowed_host("localhost:8005"));
        assert!(is_allowed_host("host.docker.internal:8005"));
        assert!(is_allowed_host("127.0.0.1:8005"));
        assert!(!is_allowed_host("evil.example.com:8005"));
    }

    #[test]
    fn test_compact_mcp_payload_reduces_tokens_and_preserves_essentials() {
        let raw = json!({
            "status": "ok",
            "identity": "Anwar Ibrahim",
            "matches": [],
            "empty_note": "",
            "null_field": null,
            "empty_tags": [],
            "empty_meta": {},
            "extracted_text": "A".repeat(6000),
            "canvas_markdown_link": "[Open Canvas](#arkib-canvas:ocr:abc)"
        });
        let pretty_len = serde_json::to_string_pretty(&raw).unwrap().len();
        let compact = compact_mcp_payload(raw);
        let compact_str = serde_json::to_string(&compact).unwrap();
        assert!(compact_str.len() < pretty_len);
        assert_eq!(compact["status"], "ok");
        assert_eq!(compact["identity"], "Anwar Ibrahim");
        assert!(compact.get("matches").is_some());
        assert!(compact.get("empty_note").is_none());
        assert!(compact.get("null_field").is_none());
        assert!(compact.get("empty_tags").is_none());
        assert!(compact.get("empty_meta").is_none());
        assert!(
            compact["extracted_text"]
                .as_str()
                .unwrap()
                .contains("[truncated 6000 chars")
        );
    }
}
