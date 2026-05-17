use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde_json::Value;
use crate::api::v1::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/", get(list_sessions))
        .route("/projects", get(get_session_projects))
        .route("/dashboard/stats", get(get_session_stats))
        .route("/{project}/{session}", get(get_session_detail))
}

async fn list_sessions() -> Json<Value> {
    Json(serde_json::json!({"sessions": [], "total": 0}))
}

async fn get_session_projects() -> Json<Value> {
    Json(serde_json::json!({"projects": [], "total_sessions": 0}))
}

async fn get_session_stats() -> Json<Value> {
    Json(serde_json::json!({
        "total_sessions": 0,
        "sessions_today": 0,
        "sessions_this_week": 0,
        "most_active_project": null,
        "total_messages": 0
    }))
}

async fn get_session_detail(
    State(state): State<ApiState>,
    Path((project, session)): Path<(String, String)>,
) -> Result<Json<Vec<Value>>, (axum::http::StatusCode, String)> {
    let path = state.session_service.resolve_session_path(&project, &session).await
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;

    let entries = state.session_service.parse_jsonl_file(&path).await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(entries))
}
