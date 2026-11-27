#![forbid(unsafe_code)]

mod admin;
mod analytics;
mod ancient_scripts;
mod archives;
mod audit;
mod biometrics;
mod canvas;
mod categories;
mod config;
mod conservation;
mod export;
mod forensics;
mod jawi;
mod mcp_sse;
mod metadata;
mod monitor;
mod preferences;
mod revisions;
mod service;
mod vector_store;
mod vision_ops;

use crate::config::AppConfig;
use crate::service::ArkibService;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// RAII guard so cancelled/dropped HTTP requests never leak `in_flight` counts.
struct InFlightGuard {
    service: Arc<ArkibService>,
    started: Instant,
    completed: bool,
}

impl InFlightGuard {
    fn new(service: Arc<ArkibService>) -> Self {
        service.monitor.begin_request();
        Self {
            service,
            started: Instant::now(),
            completed: false,
        }
    }

    fn finish(mut self, is_error: bool) {
        self.completed = true;
        let elapsed_ms = self.started.elapsed().as_millis() as u64;
        self.service.monitor.finish_request(elapsed_ms, is_error);
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if !self.completed {
            let elapsed_ms = self.started.elapsed().as_millis() as u64;
            self.service.monitor.finish_request(elapsed_ms, false);
        }
    }
}

async fn monitor_middleware(
    State(service): State<Arc<ArkibService>>,
    req: Request,
    next: Next,
) -> Response {
    // Skip long-lived SSE connections so they don't skew HTTP request latency/concurrency counters.
    if req.uri().path() == "/sse" {
        return next.run(req).await;
    }
    let guard = InFlightGuard::new(service);
    let resp = next.run(req).await;
    guard.finish(resp.status().is_server_error());
    resp
}

async fn health_handler(State(service): State<Arc<ArkibService>>) -> Json<Value> {
    let identities = service.store.list_counts().len();
    let vectors = service.store.total_vectors();
    let locations = archives::list_locations(&service.cfg).len();
    let documents = archives::list_documents(&service.cfg).len();
    Json(json!({
        "status": "ok",
        "service": "arkib-server",
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "rust",
        "face_embedding_model": service.engine.face_model_name(),
        "face_embedding_dim": service.engine.face_embedding_dim(),
        "default_face_threshold": service.engine.default_identify_threshold(),
        "identities": identities,
        "vectors": vectors,
        "locations": locations,
        "documents": documents,
    }))
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    println!("Shutdown signal received; stopping ArkibVision server gracefully...");
}

#[tokio::main]
async fn main() {
    let cfg = AppConfig::load();
    let args: Vec<String> = std::env::args().collect();

    let service = match ArkibService::new(cfg.clone()) {
        Ok(s) => Arc::new(s),
        Err(err) => {
            eprintln!("Fatal startup error: {err}");
            std::process::exit(1);
        }
    };

    // CLI mode: `--verify-face <image_path>` for instant biometric smoke-testing.
    if args.len() >= 3 && args[1] == "--verify-face" {
        let out = service.tool_identify_faces(
            &args[2],
            service.engine.default_identify_threshold(),
            false,
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".into())
        );
        return;
    }

    // CLI mode: `--reindex-faces` rebuilds all enrolled face vectors with the active encoder.
    if args.len() >= 2 && args[1] == "--reindex-faces" {
        match service.reindex_faces_from_originals() {
            Ok(out) => println!(
                "{}",
                serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".into())
            ),
            Err(err) => {
                eprintln!("Re-index error: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    // CLI mode: `--enroll-dir <dir>` for pure-Rust batch enrollment from `<dir>/<Name>/*.jpg`.
    if args.len() >= 3 && args[1] == "--enroll-dir" {
        match service.enroll_directory(&PathBuf::from(&args[2])) {
            Ok(out) => println!(
                "{}",
                serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".into())
            ),
            Err(err) => {
                eprintln!("Batch enrollment error: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    let shared_admin_state = admin::AdminState {
        service: service.clone(),
        sessions: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };
    let admin_routes = match admin::admin_router(shared_admin_state.clone()) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Fatal Admin Portal error: {err}");
            std::process::exit(1);
        }
    };
    let canvas_routes = canvas::canvas_router(shared_admin_state);

    let health_route = Router::new()
        .route("/health", get(health_handler))
        .with_state(service.clone());

    let app: Router = mcp_sse::mcp_router(service.clone())
        .merge(admin_routes)
        .merge(canvas_routes)
        .merge(health_route)
        .layer(middleware::from_fn_with_state(
            service.clone(),
            monitor_middleware,
        ));

    let addr_str = format!("{}:{}", cfg.host, cfg.port);
    let addr: SocketAddr = addr_str.parse().unwrap_or_else(|_| {
        eprintln!("Invalid ARKIB_HOST:ARKIB_PORT '{addr_str}'");
        std::process::exit(1);
    });

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(err) => {
            eprintln!("Failed binding {addr}: {err}");
            std::process::exit(1);
        }
    };

    let ident_count = service.store.list_counts().len();
    let vec_count = service.store.total_vectors();
    println!(
        "ArkibVision Pure-Rust MCP Server v{} listening on http://{}/sse (0% Python, {} identities / {} vectors)",
        env!("CARGO_PKG_VERSION"),
        addr,
        ident_count,
        vec_count
    );
    println!(
        "Admin Portal: Integrated inside LibreChat ({}/#arkib-admin) | Health: http://127.0.0.1:{}/health",
        cfg.librechat_url, cfg.port
    );
    println!("Roles Active: Guest (default, staging review) | Admin (valid admin_key required)");

    if let Err(err) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        eprintln!("Server error: {err}");
        std::process::exit(1);
    }
}
