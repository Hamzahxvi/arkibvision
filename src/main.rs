#![forbid(unsafe_code)]

mod admin;
mod ancient_scripts;
mod archives;
mod biometrics;
mod config;
mod forensics;
mod jawi;
mod mcp_sse;
mod metadata;
mod service;
mod vector_store;
mod vision_ops;

use crate::config::AppConfig;
use crate::service::RojakFaceService;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

async fn health_handler(State(service): State<Arc<RojakFaceService>>) -> Json<Value> {
    let identities = service.store.list_counts().len();
    let vectors = service.store.total_vectors();
    let locations = archives::list_locations(&service.cfg).len();
    let documents = archives::list_documents(&service.cfg).len();
    Json(json!({
        "status": "ok",
        "service": "rojakface-server",
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "rust",
        "identities": identities,
        "vectors": vectors,
        "locations": locations,
        "documents": documents,
    }))
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    println!("Shutdown signal received; stopping RojakFace server gracefully...");
}

#[tokio::main]
async fn main() {
    let cfg = AppConfig::load();
    let args: Vec<String> = std::env::args().collect();

    let service = match RojakFaceService::new(cfg.clone()) {
        Ok(s) => Arc::new(s),
        Err(err) => {
            eprintln!("Fatal startup error: {err}");
            std::process::exit(1);
        }
    };

    // CLI mode: `--verify-face <image_path>` for instant biometric smoke-testing.
    if args.len() >= 3 && args[1] == "--verify-face" {
        let out = service.tool_identify_faces(&args[2], 0.88, false);
        println!(
            "{}",
            serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".into())
        );
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

    let admin_routes = match admin::admin_router(service.clone()) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Fatal Admin Portal error: {err}");
            std::process::exit(1);
        }
    };

    let health_route = Router::new()
        .route("/health", get(health_handler))
        .with_state(service.clone());

    let app: Router = mcp_sse::mcp_router(service.clone())
        .merge(admin_routes)
        .merge(health_route);

    let addr_str = format!("{}:{}", cfg.host, cfg.port);
    let addr: SocketAddr = addr_str.parse().unwrap_or_else(|_| {
        eprintln!("Invalid ROJAKFACE_HOST:ROJAKFACE_PORT '{addr_str}'");
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
        "RojakFace Pure-Rust MCP Server v{} listening on http://{}/sse (0% Python, {} identities / {} vectors)",
        env!("CARGO_PKG_VERSION"),
        addr,
        ident_count,
        vec_count
    );
    println!(
        "Admin Portal: http://127.0.0.1:{}/admin | Health: http://127.0.0.1:{}/health",
        cfg.port, cfg.port
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
