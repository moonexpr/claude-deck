/// AI proxy handlers — Phase C, task C1.
///
/// POST /api/v1/ai/chat    — streaming chat via Vercel Data Stream Protocol v1
/// POST /api/v1/ai/suggest — single-shot non-streaming suggestion
///
/// Security: same-origin check reused from cc_bridge (see note below).
/// Key source: `ApiState.anthropic_api_key` only — never std::env.
///
/// Same-origin parity with cc-bridge; auth hardening tracked in
/// docs/requests/INBOX.md
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    routing::post,
};
use bytes::Bytes;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::api::v1::ApiState;
use crate::cc_bridge::is_same_origin;
use crate::services::ai::{Message, anthropic, proxy};

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AiRequest {
    messages: Vec<Message>,
    #[serde(default)]
    model: Option<String>,
}

/// Generic error body. Fields are present only when relevant to the error type.
#[derive(Serialize)]
struct ErrorBody {
    status: &'static str,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    anthropic_key_configured: Option<bool>,
    /// Always null when present (indicates no key source is configured).
    #[serde(skip_serializing_if = "Option::is_none")]
    key_source: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_code: Option<u16>,
}

#[derive(Serialize)]
struct SuggestResponse {
    text: String,
    usage: SuggestUsage,
}

#[derive(Serialize)]
struct SuggestUsage {
    input_tokens: u64,
    output_tokens: u64,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Parse an upstream error string produced by the anthropic service layer.
/// Format: "upstream_error|<status_code>|<detail>"
fn parse_upstream_error(err: &str) -> (u16, String) {
    if let Some(rest) = err.strip_prefix("upstream_error|")
        && let Some((code_str, detail)) = rest.split_once('|')
            && let Ok(code) = code_str.parse::<u16>() {
                return (code, detail.to_string());
            }
    (502, err.to_string())
}

fn upstream_error_response(err: &str) -> Response<Body> {
    let (code, detail) = parse_upstream_error(err);
    let body = ErrorBody {
        status: "upstream_error",
        detail,
        anthropic_key_configured: None,
        key_source: None,
        status_code: Some(code),
    };
    (StatusCode::BAD_GATEWAY, Json(body)).into_response()
}

fn forbidden_response() -> Response<Body> {
    let body = ErrorBody {
        status: "forbidden",
        detail: "same-origin check failed".to_string(),
        anthropic_key_configured: None,
        key_source: None,
        status_code: None,
    };
    (StatusCode::FORBIDDEN, Json(body)).into_response()
}

fn no_key_response() -> Response<Body> {
    let body = ErrorBody {
        status: "unavailable",
        detail: "anthropic_api_key not configured".to_string(),
        anthropic_key_configured: Some(false),
        key_source: Some(serde_json::Value::Null),
        status_code: None,
    };
    (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response()
}

/// Origin required for all state-changing requests; aligns with cc-bridge WS policy.
///
/// Absent or empty Origin → 403. This mirrors the WebSocket endpoint which has
/// always rejected absent-Origin connections (close code 4403). The previous
/// "absent allowed for non-browser callers" rationale no longer applies: every
/// legitimate Deck client (browser, Tauri) sends Origin. Server-to-server callers
/// must pass an explicit same-origin header to be accepted.
fn same_origin_ok(headers: &HeaderMap) -> bool {
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    !origin.is_empty() && is_same_origin(origin, headers)
}

// ---------------------------------------------------------------------------
// POST /api/v1/ai/chat — streaming
// ---------------------------------------------------------------------------

async fn post_chat(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<AiRequest>,
) -> Response<Body> {
    if !same_origin_ok(&headers) {
        return forbidden_response();
    }

    let api_key = match &state.anthropic_api_key {
        Some(k) => k.clone(),
        None => return no_key_response(),
    };

    let client = Client::new();
    let base_url = state.anthropic_base_url.clone();
    let model = req.model.clone();

    let event_stream =
        match anthropic::stream_messages(client, base_url, api_key, model, req.messages).await {
            Ok(s) => s,
            Err(e) => return upstream_error_response(&e.to_string()),
        };

    // Transform Anthropic SSE events → Vercel Data Stream frames.
    let data_stream = proxy::anthropic_sse_to_data_stream(event_stream)
        .map(Ok::<Bytes, std::convert::Infallible>);
    let body = Body::from_stream(data_stream);

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; charset=utf-8")
        .header("x-vercel-ai-data-stream", "v1")
        .header("cache-control", "no-cache")
        .header("transfer-encoding", "chunked")
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// ---------------------------------------------------------------------------
// POST /api/v1/ai/suggest — non-streaming
// ---------------------------------------------------------------------------

async fn post_suggest(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<AiRequest>,
) -> Response<Body> {
    if !same_origin_ok(&headers) {
        return forbidden_response();
    }

    let api_key = match &state.anthropic_api_key {
        Some(k) => k.clone(),
        None => return no_key_response(),
    };

    let client = Client::new();
    let base_url = state.anthropic_base_url.clone();
    let model = req.model.clone();

    match anthropic::complete_messages(client, base_url, api_key, model, req.messages).await {
        Ok((text, usage)) => {
            let resp = SuggestResponse {
                text,
                usage: SuggestUsage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                },
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => upstream_error_response(&e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/chat", post(post_chat))
        .route("/suggest", post(post_suggest))
}
