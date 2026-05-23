/// Phase C, task C1 — AI proxy integration tests.
///
/// Three tests drive `POST /api/v1/ai/chat` through a real axum router (via
/// `tower::ServiceExt::oneshot`) and validate the three main response paths:
///
///   1. no_key_returns_503           — anthropic_api_key is None → 503
///   2. bad_upstream_returns_502     — wiremock returns 401     → 502
///   3. happy_path_streams_data_frames — valid SSE stream        → Vercel Data Stream frames
use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_tmp() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "claude_deck_ai_test_{}_{}_{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos(),
    ));
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    tmp
}

/// Build a `ServerConfig` pointing at `wiremock_uri` as the Anthropic base URL.
fn make_config_with_key(
    tmp: &std::path::Path,
    anthropic_base_url: String,
    api_key: Option<String>,
) -> server_core::ServerConfig {
    let db_url = format!("sqlite:{}?mode=rwc", tmp.join("test.db").display());
    server_core::ServerConfig {
        db_url,
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        anthropic_api_key: api_key,
        anthropic_base_url,
        enable_external_tools: false,
        cwd_fallback: tmp.to_path_buf(),
    }
}

/// POST body for a minimal chat request.
const CHAT_BODY: &str = r#"{"messages":[{"role":"user","content":"hi"}]}"#;

// Origin + Host used by all test requests so the same-origin check passes.
// Absent-Origin requests now return 403 (parity with cc-bridge WS).
const TEST_HOST: &str = "127.0.0.1:8000";
const TEST_ORIGIN: &str = "http://127.0.0.1:8000";

// ---------------------------------------------------------------------------
// Test 1: no key → 503
// ---------------------------------------------------------------------------

/// When `anthropic_api_key` is `None`, the handler must return 503 with a JSON
/// body containing `"anthropic_key_configured": false`.
#[tokio::test]
async fn no_key_returns_503() {
    let tmp = make_tmp();
    let config = make_config_with_key(
        &tmp,
        "https://api.anthropic.com".to_string(),
        None, // no key
    );
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/ai/chat")
        .header("content-type", "application/json")
        .header("origin", TEST_ORIGIN)
        .header("host", TEST_HOST)
        .body(Body::from(CHAT_BODY))
        .unwrap();

    let resp = router.oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status().as_u16(), 503, "expected 503 when no API key");

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value =
        serde_json::from_slice(&bytes).expect("response body must be valid JSON");

    assert_eq!(
        body["anthropic_key_configured"],
        serde_json::Value::Bool(false),
        "anthropic_key_configured must be false; body: {}",
        body
    );
    assert_eq!(
        body["status"], "unavailable",
        "status field; body: {}",
        body
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Test 2: upstream 401 → 502
// ---------------------------------------------------------------------------

/// When Anthropic returns a non-2xx status (simulated via wiremock returning
/// 401), the handler must return 502 with `"status_code": 401` in the body.
#[tokio::test]
async fn bad_upstream_returns_502() {
    let mock_server = MockServer::start().await;

    // Respond to any POST on /v1/messages with 401 Unauthorized.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_string(
            r#"{"type":"error","error":{"type":"authentication_error","message":"invalid key"}}"#,
        ))
        .mount(&mock_server)
        .await;

    let tmp = make_tmp();
    let config = make_config_with_key(
        &tmp,
        mock_server.uri(), // point at wiremock
        Some("bad-key".to_string()),
    );
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/ai/chat")
        .header("content-type", "application/json")
        .header("origin", TEST_ORIGIN)
        .header("host", TEST_HOST)
        .body(Body::from(CHAT_BODY))
        .unwrap();

    let resp = router.oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status().as_u16(), 502, "expected 502 for upstream 401");

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value =
        serde_json::from_slice(&bytes).expect("response body must be valid JSON");

    assert_eq!(
        body["status_code"],
        serde_json::Value::Number(401.into()),
        "status_code must be 401; body: {}",
        body
    );
    assert_eq!(
        body["status"], "upstream_error",
        "status field; body: {}",
        body
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Test 3: happy path — SSE stream → Vercel Data Stream frames
// ---------------------------------------------------------------------------

/// Wiremock returns a canned Anthropic SSE stream with one `content_block_delta`
/// event ("Hello") followed by `message_stop`.  The test reads the full response
/// body and asserts:
///   - it contains a `0:"Hello"\n` frame
///   - it ends with a `d:{...}\n` frame containing `"finishReason":"stop"`
#[tokio::test]
async fn happy_path_streams_data_frames() {
    let mock_server = MockServer::start().await;

    // Canned Anthropic SSE stream.  Follows the real Anthropic wire format:
    // each SSE event is "event: <type>\ndata: <json>\n\n".
    let sse_body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-5\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n",
        "\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n",
        "\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n",
        "\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n",
        "\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":5}}\n",
        "\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n",
        "\n",
    );

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body),
        )
        .mount(&mock_server)
        .await;

    let tmp = make_tmp();
    let config = make_config_with_key(&tmp, mock_server.uri(), Some("test-key".to_string()));
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/ai/chat")
        .header("content-type", "application/json")
        .header("origin", TEST_ORIGIN)
        .header("host", TEST_HOST)
        .body(Body::from(CHAT_BODY))
        .unwrap();

    let resp = router.oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status().as_u16(), 200, "expected 200 for happy path");

    // Verify Vercel AI SDK response headers.
    let headers = resp.headers();
    assert_eq!(
        headers
            .get("x-vercel-ai-data-stream")
            .and_then(|v| v.to_str().ok()),
        Some("v1"),
        "x-vercel-ai-data-stream header must be v1"
    );
    assert!(
        headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|ct| ct.starts_with("text/plain"))
            .unwrap_or(false),
        "content-type must be text/plain"
    );

    // Collect the streaming body.
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_str = std::str::from_utf8(&bytes).expect("body must be valid UTF-8");

    // Must contain a 0: text frame for "Hello"
    assert!(
        body_str.contains("0:\"Hello\"\n"),
        "body must contain 0:\"Hello\"\\n frame; got: {:?}",
        body_str
    );

    // Must contain a d: finalization frame with finishReason:stop and
    // accurate token counts accumulated from message_start (input_tokens:10)
    // and message_delta (output_tokens:5) in the canned SSE stream above.
    assert!(
        body_str.contains("d:{") && body_str.contains("\"finishReason\":\"stop\""),
        "body must contain d: frame with finishReason:stop; got: {:?}",
        body_str
    );
    assert!(
        body_str.contains("\"promptTokens\":10"),
        "d: frame must carry promptTokens:10 (from message_start); got: {:?}",
        body_str
    );
    assert!(
        body_str.contains("\"completionTokens\":5"),
        "d: frame must carry completionTokens:5 (from message_delta); got: {:?}",
        body_str
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
