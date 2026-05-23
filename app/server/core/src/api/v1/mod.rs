pub mod agents;
pub mod ai;
pub mod backup;
pub mod chat;
pub mod cli;
pub mod commands;
pub mod config;
pub mod context;
pub mod hooks;
pub mod mcp;
pub mod memory;
pub mod output_styles;
pub mod permissions;
pub mod plans;
pub mod plugins;
pub mod presence;
pub mod projects;
pub mod sessions;
pub mod status;
pub mod statusline;
pub mod usage;

use crate::services::session_service::SessionService;
use axum::{Router, routing::get};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct ApiState {
    pub pool: SqlitePool,
    pub session_service: Arc<SessionService>,
    /// Override base URL for presence event hook snippet generation.
    /// `None` means derive from the incoming request's `Host` header.
    pub presence_public_url: Option<String>,
    /// When `false`, PATH-dependent external-tool discovery is skipped in
    /// agents, cli, mcp, and plugins routes.
    pub enable_external_tools: bool,
    /// CWD fallback used when handlers need a base path and no `project_path`
    /// query param was provided. Captured once by the embedder; never via
    /// `std::env::current_dir()` inside the library.
    pub cwd_fallback: PathBuf,
    /// Anthropic API key resolved by the embedder. `None` means no key was
    /// configured. Carried here so `ai::post_chat` can report the diagnostic
    /// boolean without reading `std::env`.
    pub anthropic_api_key: Option<String>,
    /// Base URL for the Anthropic API (default: `https://api.anthropic.com`).
    /// Overridable in tests to point at a wiremock server.
    pub anthropic_base_url: String,
}

pub fn router(
    pool: SqlitePool,
    projects_dir: PathBuf,
    presence_public_url: Option<String>,
    enable_external_tools: bool,
    cwd_fallback: PathBuf,
    anthropic_api_key: Option<String>,
    anthropic_base_url: String,
) -> Router {
    let state = ApiState {
        pool,
        session_service: Arc::new(SessionService::new(projects_dir)),
        presence_public_url,
        enable_external_tools,
        cwd_fallback,
        anthropic_api_key,
        anthropic_base_url,
    };

    Router::new()
        .route("/status", get(status_handler))
        .nest("/sessions", sessions::router())
        .nest("/presence", presence::router())
        .nest("/projects", projects::router())
        .nest("/config", config::router())
        .nest("/usage", usage::router())
        .nest("/hooks", hooks::router())
        .nest("/status-info", status::router())
        .nest("/agents", agents::router())
        .nest("/backup", backup::router())
        .nest("/cli", cli::router())
        .nest("/commands", commands::router())
        .nest("/mcp", mcp::router())
        .nest("/memory", memory::router())
        .nest("/output-styles", output_styles::router())
        .nest("/permissions", permissions::router())
        .nest("/plans", plans::router())
        .nest("/plugins", plugins::router())
        .nest("/statusline", statusline::router())
        .nest("/ai", ai::router())
        .nest("/cc-bridge", crate::cc_bridge::router())
        .nest("/chat", chat::router())
        .nest("/context", context::router())
        .with_state(state)
}

async fn status_handler() -> &'static str {
    "v1 API is active"
}
