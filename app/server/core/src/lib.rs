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
    response::IntoResponse,
    http::StatusCode,
};
use sqlx::sqlite::SqlitePoolOptions;
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

/// Map a file extension to a MIME type string.
///
/// Covers the set of extensions produced by a typical Vite/React build:
/// JavaScript modules, CSS, images, fonts, and common data formats.
/// Falls back to `application/octet-stream` for anything else.
fn ext_to_mime(ext: &str) -> &'static str {
    match ext {
        "html" | "htm"  => "text/html; charset=utf-8",
        "js" | "mjs"    => "application/javascript",
        "css"           => "text/css",
        "json"          => "application/json",
        "svg"           => "image/svg+xml",
        "png"           => "image/png",
        "jpg" | "jpeg"  => "image/jpeg",
        "gif"           => "image/gif",
        "ico"           => "image/x-icon",
        "webp"          => "image/webp",
        "woff"          => "font/woff",
        "woff2"         => "font/woff2",
        "ttf"           => "font/ttf",
        "otf"           => "font/otf",
        "txt"           => "text/plain; charset=utf-8",
        "xml"           => "application/xml",
        "wasm"          => "application/wasm",
        _               => "application/octet-stream",
    }
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
    //
    // Why not ServeDir::not_found_service(ServeFile)?
    // ServeDir's not_found_service propagates the 404 status from the parent
    // ServeDir even though the inner ServeFile successfully serves the bytes.
    // The result is index.html body + HTTP 404, which breaks SPA deep-links,
    // browser caching, and "add to home screen".
    //
    // Correct idiom: use a single axum fallback handler.  The handler first
    // tries to serve a real file from the dist directory (covers
    // /assets/*, /favicon-32.png, etc.).  When the file does not exist the
    // handler falls back to index.html with an explicit 200.  API routes are
    // mounted before the fallback and matched first by axum, so /api/v1/* is
    // never touched by this handler.
    if let Some(frontend_dist) = config.frontend_dist_path {
        let index_path = frontend_dist.join("index.html");
        let dist_root = frontend_dist.clone();
        let spa_fallback = move |req: axum::http::Request<axum::body::Body>| {
            let dist = dist_root.clone();
            let index = index_path.clone();
            async move {
                // Strip leading '/' and resolve against the dist root.
                let path_str = req.uri().path().trim_start_matches('/');
                let candidate = dist.join(path_str);

                // Only serve a real file if the resolved path stays inside the
                // dist root (no path-traversal) and is a plain file.
                let is_safe_file = candidate
                    .canonicalize()
                    .ok()
                    .and_then(|abs| {
                        dist.canonicalize()
                            .ok()
                            .map(|base| abs.starts_with(base) && abs.is_file())
                    })
                    .unwrap_or(false);

                if is_safe_file {
                    match tokio::fs::read(&candidate).await {
                        Ok(bytes) => {
                            let mime = ext_to_mime(
                                candidate
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or(""),
                            );
                            (
                                StatusCode::OK,
                                [(axum::http::header::CONTENT_TYPE, mime)],
                                bytes,
                            )
                                .into_response()
                        }
                        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                    }
                } else {
                    // SPA client-side route — serve index.html with 200.
                    match tokio::fs::read(&index).await {
                        Ok(bytes) => (
                            StatusCode::OK,
                            [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                            bytes,
                        )
                            .into_response(),
                        Err(_) => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            }
        };
        router = router.fallback(spa_fallback);
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
