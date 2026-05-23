use crate::api::v1::ApiState;
use axum::{Json, Router, routing::get};

pub fn router() -> Router<ApiState> {
    Router::new().route("/", get(get_status))
}

async fn get_status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "claude_code_version": "0.29.0",
        "active_sessions": 0
    }))
}
