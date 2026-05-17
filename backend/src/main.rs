mod api;
mod error;
mod fileio;
mod models;
mod paths;
mod patterns;
mod services;

use axum::{
    routing::get,
    Router,
    Json,
};
use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tower_http::services::ServeDir;
use tower_http::cors::{CorsLayer, Any};
use std::path::PathBuf;
use serde::Serialize;

#[derive(Serialize)]
struct HealthResponse {
    name: String,
    version: String,
    status: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:claude_registry.db?mode=rwc".to_string());

    // Setup database pool
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    // Schema bootstrap. Per-module ensure functions create projects/backups/
    // presence_* tables on first use; these two are the only tables no module
    // creates (mcp + usage degrade gracefully if absent, but creating them up
    // front avoids first-request errors). Columns mirror models.rs (which in
    // turn mirrors the Python SQLAlchemy schema).
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS usage_cache (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cache_key TEXT NOT NULL UNIQUE,
            cache_type TEXT NOT NULL,
            project_path TEXT,
            data TEXT NOT NULL,
            cached_at TEXT NOT NULL,
            file_hash TEXT
        )"#,
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS mcp_server_cache (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_name TEXT NOT NULL,
            server_scope TEXT NOT NULL,
            is_connected BOOLEAN NOT NULL DEFAULT 0,
            last_tested_at TEXT,
            last_error TEXT,
            mcp_server_name TEXT,
            mcp_server_version TEXT,
            tools TEXT,
            tool_count INTEGER NOT NULL DEFAULT 0,
            resources TEXT,
            prompts TEXT,
            resource_count INTEGER NOT NULL DEFAULT 0,
            prompt_count INTEGER NOT NULL DEFAULT 0,
            capabilities TEXT,
            cached_at TEXT NOT NULL,
            config_hash TEXT,
            UNIQUE(server_name, server_scope)
        )"#,
    )
    .execute(&pool)
    .await?;

    // Determine frontend path
    let mut frontend_dist = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    frontend_dist.pop(); // Up from backend
    frontend_dist.push("frontend/dist");

    let index_path = frontend_dist.join("index.html");

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build our application
    let app = Router::new()
        .route("/health", get(health))
        .nest("/api/v1", api::v1::router(pool))
        .fallback_service(
            ServeDir::new(frontend_dist)
                .append_index_html_on_directories(true)
                .not_found_service(tower_http::services::ServeFile::new(index_path))
        )
        .layer(cors);

    // Run it with hyper
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;

    tracing::info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        name: "Claude Deck".to_string(),
        version: "1.2.0".to_string(),
        status: "running".to_string(),
    })
}
