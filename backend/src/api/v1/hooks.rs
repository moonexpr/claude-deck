use axum::{
    routing::{get, post, put, delete},
    Router, Json,
};
use crate::api::v1::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/", get(list_hooks).post(create_hook))
        .route("/{hook_id}", put(update_hook).delete(delete_hook))
        // NOTE: distinct path to avoid axum 0.8 same-segment param conflict
        // with `/{hook_id}`. Real port (task #3) reconciles with the Python
        // router + frontend; this only keeps the baseline booting.
        .route("/by-event/{event}", get(get_hooks_by_event))
}

async fn list_hooks() -> Json<serde_json::Value> {
    Json(serde_json::json!({"hooks": []}))
}

async fn create_hook() -> axum::http::StatusCode {
    axum::http::StatusCode::CREATED
}

async fn update_hook() -> axum::http::StatusCode {
    axum::http::StatusCode::OK
}

async fn delete_hook() -> axum::http::StatusCode {
    axum::http::StatusCode::NO_CONTENT
}

async fn get_hooks_by_event() -> Json<serde_json::Value> {
    Json(serde_json::json!({"hooks": []}))
}
