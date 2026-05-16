use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde_json::Value;
use crate::api::v1::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/:project/:session", get(get_session_detail))
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
