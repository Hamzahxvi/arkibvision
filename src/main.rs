#![forbid(unsafe_code)]

mod admin;
mod biometrics;
mod config;
mod jawi;
mod mcp_sse;
mod metadata;
mod service;
mod vector_store;
mod vision_ops;

use crate::config::AppConfig;
use crate::service::RojakFaceService;
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;

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

    let admin_routes = match admin::admin_router(service.clone()) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Fatal Admin Portal error: {err}");
            std::process::exit(1);
        }
    };

    let app: Router = mcp_sse::mcp_router(service.clone()).merge(admin_routes);

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
    let vec_count = service.store.all_points().len();
    println!(
        "RojakFace Pure-Rust MCP Server v{} listening on http://{}/sse (0% Python, {} identities / {} vectors)",
        env!("CARGO_PKG_VERSION"),
        addr,
        ident_count,
        vec_count
    );
    println!(
        "Admin Portal: http://127.0.0.1:{}/admin (form login required)",
        cfg.port
    );
    println!("Roles Active: Guest (default, staging review) | Admin (valid admin_key required)");

    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("Server error: {err}");
        std::process::exit(1);
    }
}
