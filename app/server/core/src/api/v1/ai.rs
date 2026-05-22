// A7 scaffold: AI chat stub route.
//
// POST /api/v1/ai/chat → HTTP 501 Not Implemented.
//
// The handler reads `anthropic_api_key` from `ApiState` (set once by the
// embedder; never via `std::env`). The 501 body carries a diagnostic boolean
// so clients can distinguish "no key configured" from "key present but proxy
// not built yet". The real Anthropic streaming proxy lands in Phase C (C1).

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::Serialize;

use crate::api::v1::ApiState;

#[derive(Serialize)]
struct AiChatStubResponse {
    status: &'static str,
    detail: &'static str,
    anthropic_key_configured: bool,
}

/// POST /ai/chat
///
/// Stub handler. Returns 501 with a JSON diagnostic body. The
/// `anthropic_key_configured` field lets the client surface a meaningful
/// disabled-state message without guessing why chat is unavailable.
async fn post_chat(State(state): State<ApiState>) -> impl IntoResponse {
    let key_present = state.anthropic_api_key.is_some();
    let body = AiChatStubResponse {
        status: "not_implemented",
        detail: "AI chat proxy lands in Phase C (C1)",
        anthropic_key_configured: key_present,
    };
    (StatusCode::NOT_IMPLEMENTED, Json(body))
}

pub fn router() -> Router<ApiState> {
    Router::new().route("/chat", post(post_chat))
}
