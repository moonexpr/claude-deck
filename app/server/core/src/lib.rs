pub mod api;
pub mod cc_bridge;
pub mod error;
pub mod fileio;
pub mod models;
pub mod paths;
pub mod patterns;
pub mod services;

use axum::{
    routing::get,
    Router,
    Json,
};
use sqlx::sqlite::SqlitePoolOptions;
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

/// Configuration supplied by the embedder (or `server-bin`). All env/process
/// reads happen in the caller; `server-core` is purely config-in, Router-out.
pub struct ServerConfig {
    /// SQLite connection string, e.g. `sqlite:claude_registry.db?mode=rwc`.
    pub db_url: String,
    /// Absolute path to the Claude projects directory (e.g. `~/.claude/projects`).
    pub projects_dir: PathBuf,
    /// When `Some`, mount `ServeDir` at this path for static frontend files.
    /// When `None`, skip frontend serving (API routes still mount).
    pub frontend_dist_path: Option<PathBuf>,
    /// Override base URL for presence event hooks (e.g. `https://deck.example.com`).
    pub presence_public_url: Option<String>,
    /// Anthropic API key — carried through for future phases; unused for now.
    pub anthropic_api_key: Option<String>,
    /// When `false`, PATH-dependent external-tool discovery is skipped and
    /// affected routes return the same empty/default result a missing tool yields.
    pub enable_external_tools: bool,
    /// Working-directory fallback used when a handler needs a CWD-relative base
    /// path and no `project_path` query param was supplied. Captured once by
    /// `server-bin` from `std::env::current_dir()` so the library never reads it.
    pub cwd_fallback: PathBuf,
}

/// Build the axum `Router` from the supplied config.
///
/// This function creates the SQLite pool, runs schema bootstrap, configures
/// CORS, and assembles all route modules. It does NOT init tracing, call
/// `dotenvy`, or bind a socket — those responsibilities belong to the caller.
pub async fn app(config: ServerConfig) -> anyhow::Result<axum::Router> {
    // Setup database pool
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&config.db_url)
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

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build our application
    let mut router = Router::new()
        .route("/health", get(health))
        .nest("/api/v1", api::v1::router(pool, config.projects_dir, config.presence_public_url, config.enable_external_tools, config.cwd_fallback, config.anthropic_api_key))
        .layer(cors);

    // Mount static frontend serving only when a dist path was supplied.
    if let Some(frontend_dist) = config.frontend_dist_path {
        let index_path = frontend_dist.join("index.html");
        router = router.fallback_service(
            ServeDir::new(frontend_dist)
                .append_index_html_on_directories(true)
                .not_found_service(tower_http::services::ServeFile::new(index_path))
        );
    }

    Ok(router)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        name: "Claude Deck".to_string(),
        version: "1.2.0".to_string(),
        status: "running".to_string(),
    })
}
