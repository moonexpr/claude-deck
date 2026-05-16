pub mod sessions;

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
        .route("/status", get(status))
        .nest("/sessions", sessions::router())
        .with_state(state)
}

async fn status() -> &'static str {
    "v1 API is active"
}
