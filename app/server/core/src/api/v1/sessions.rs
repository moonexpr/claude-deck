use crate::api::v1::ApiState;
use crate::services::session_service::SessionDetailResponse;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct DetailQuery {
    #[serde(default)]
    page: Option<usize>,
}

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
    Query(query): Query<DetailQuery>,
) -> Result<Json<SessionDetailResponse>, (axum::http::StatusCode, String)> {
    let page = query.page.unwrap_or(1).max(1);

    let detail = state
        .session_service
        .get_session_detail(&project, &session, page)
        .await
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(detail))
}
