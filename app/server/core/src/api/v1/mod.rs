pub mod sessions;
pub mod presence;
pub mod projects;
pub mod config;
pub mod usage;
pub mod hooks;
pub mod status;
pub mod agents;
pub mod backup;
pub mod cli;
pub mod commands;
pub mod mcp;
pub mod memory;
pub mod output_styles;
pub mod permissions;
pub mod plans;
pub mod plugins;
pub mod statusline;
pub mod cc_bridge;
pub mod context;

use axum::{
    routing::get,
    Router,
};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use crate::services::session_service::SessionService;

#[derive(Clone)]
pub struct ApiState {
    pub pool: SqlitePool,
    pub session_service: Arc<SessionService>,
}

pub fn router(pool: SqlitePool) -> Router {
    let projects_dir = std::env::var("PROJECTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut p = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
            p.push(".claude/projects");
            p
        });

    let state = ApiState {
        pool,
        session_service: Arc::new(SessionService::new(projects_dir)),
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
        .nest("/cc-bridge", cc_bridge::router())
        .nest("/context", context::router())
        .with_state(state)
}

async fn status_handler() -> &'static str {
    "v1 API is active"
}
