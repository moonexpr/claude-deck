use crate::api::v1::ApiState;
use crate::services::session_service::{
    SessionDetailResponse, SessionListResponse, SessionProjectListResponse, SessionStatsResponse,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;

/// Default page size for the session list when the client doesn't pass `limit`.
const DEFAULT_LIST_LIMIT: usize = 100;

#[derive(Debug, Deserialize)]
struct DetailQuery {
    #[serde(default)]
    page: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    project_folder: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    // sort_by / sort_order accepted for forward-compat; list is modified-desc.
    #[serde(default)]
    #[allow(dead_code)]
    sort_by: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    sort_order: Option<String>,
}

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/", get(list_sessions))
        .route("/projects", get(get_session_projects))
        .route("/dashboard/stats", get(get_session_stats))
        .route("/{project}/{session}", get(get_session_detail))
}

async fn list_sessions(
    State(state): State<ApiState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<SessionListResponse>, (StatusCode, String)> {
    let limit = query.limit.unwrap_or(DEFAULT_LIST_LIMIT).max(1);
    state
        .session_service
        .list_sessions(query.project_folder.as_deref(), limit)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn get_session_projects(
    State(state): State<ApiState>,
) -> Result<Json<SessionProjectListResponse>, (StatusCode, String)> {
    state
        .session_service
        .list_projects()
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn get_session_stats(
    State(state): State<ApiState>,
) -> Result<Json<SessionStatsResponse>, (StatusCode, String)> {
    state
        .session_service
        .dashboard_stats()
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
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
