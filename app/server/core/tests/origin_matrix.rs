/// Origin validation matrix — Phase C follow-up.
///
/// Tests the same-origin enforcement across all state-changing routes after
/// the "reject absent Origin" tightening (parity with cc-bridge WS close 4403).
///
/// Each test is a single `#[tokio::test]` with a descriptive name. All share
/// one `make_router()` helper and two request helpers (`post_with_origin`,
/// `get_with_origin`).
///
/// Routes covered (4 — one per endpoint family):
///   POST /api/v1/ai/chat        — streaming AI proxy
///   POST /api/v1/ai/suggest     — non-streaming AI proxy
///   POST /api/v1/chat           — CRUD create
///   GET  /api/v1/chat           — CRUD list (read — also requires Origin now)
///
/// For routes that would 503/200/201/204 with a valid origin + valid state,
/// the test accepts any non-403 status (2xx OR 503 for missing API key) because
/// the goal is verifying the origin gate, not the business logic.
///
/// Localhost loophole (documented, not fixed here):
///   is_same_origin() allows any localhost variant (localhost, 127.0.0.1, ::1)
///   regardless of port. This lets the Tauri shell and dev server on different
///   ports work without configuration. The loophole is accepted behavior; a
///   future decision to close it would be its own cycle.
use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn make_tmp() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "claude_deck_origin_matrix_{}_{}_{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos(),
    ))
}

async fn make_router() -> axum::Router {
    let tmp = make_tmp();
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let db_url = format!("sqlite:{}?mode=rwc", tmp.join("test.db").display());
    let config = server_core::ServerConfig {
        db_url,
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        anthropic_api_key: None, // causes 503 on AI routes — that's acceptable
        anthropic_base_url: "https://api.anthropic.com".to_string(),
        enable_external_tools: false,
        cwd_fallback: tmp,
    };
    server_core::app(config).await.expect("server_core::app")
}

/// POST to `uri` with the given `origin` (None = absent) and `host`.
/// Returns the HTTP status code.
async fn post_with_origin(
    router: &axum::Router,
    uri: &str,
    origin: Option<&str>,
    host: &str,
    body: &str,
) -> u16 {
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("host", host);
    if let Some(o) = origin {
        builder = builder.header("origin", o);
    }
    let req = builder.body(Body::from(body.to_string())).unwrap();
    let resp = router.clone().oneshot(req).await.expect("oneshot");
    resp.status().as_u16()
}

/// GET `uri` with the given `origin` (None = absent) and `host`.
/// Returns the HTTP status code.
async fn get_with_origin(
    router: &axum::Router,
    uri: &str,
    origin: Option<&str>,
    host: &str,
) -> u16 {
    let mut builder = axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", host);
    if let Some(o) = origin {
        builder = builder.header("origin", o);
    }
    let req = builder.body(Body::empty()).unwrap();
    let resp = router.clone().oneshot(req).await.expect("oneshot");
    resp.status().as_u16()
}

const HOST: &str = "127.0.0.1:8000";
const MATCHING_ORIGIN: &str = "http://127.0.0.1:8000";
const EVIL_ORIGIN: &str = "http://evil.example";
const AI_CHAT: &str = "/api/v1/ai/chat";
const AI_SUGGEST: &str = "/api/v1/ai/suggest";
const CHAT_CRUD: &str = "/api/v1/chat";
const AI_BODY: &str = r#"{"messages":[{"role":"user","content":"hi"}]}"#;
const CHAT_BODY: &str = r#"{"title":"T","messages":[]}"#;

// ---------------------------------------------------------------------------
// POST /api/v1/ai/chat
// ---------------------------------------------------------------------------

/// Absent Origin → 403 (tightening — was 503 before this change).
#[tokio::test]
async fn ai_chat_rejects_absent_origin() {
    let router = make_router().await;
    let status = post_with_origin(&router, AI_CHAT, None, HOST, AI_BODY).await;
    assert_eq!(status, 403, "absent Origin must be 403");
}

/// Empty string Origin → 403.
#[tokio::test]
async fn ai_chat_rejects_empty_origin() {
    let router = make_router().await;
    let status = post_with_origin(&router, AI_CHAT, Some(""), HOST, AI_BODY).await;
    assert_eq!(status, 403, "empty Origin must be 403");
}

/// Matching Origin → allowed (503 because no API key — not 403).
#[tokio::test]
async fn ai_chat_allows_matching_origin() {
    let router = make_router().await;
    let status = post_with_origin(&router, AI_CHAT, Some(MATCHING_ORIGIN), HOST, AI_BODY).await;
    assert_ne!(status, 403, "matching Origin must not be 403; got {}", status);
}

/// Mismatched Origin → 403.
#[tokio::test]
async fn ai_chat_rejects_mismatched_origin() {
    let router = make_router().await;
    let status = post_with_origin(&router, AI_CHAT, Some(EVIL_ORIGIN), HOST, AI_BODY).await;
    assert_eq!(status, 403, "mismatched Origin must be 403");
}

/// localhost:9999 vs 127.0.0.1:8000 → allowed (loophole, documented above).
#[tokio::test]
async fn ai_chat_allows_localhost_any_port() {
    let router = make_router().await;
    let status =
        post_with_origin(&router, AI_CHAT, Some("http://localhost:9999"), HOST, AI_BODY).await;
    assert_ne!(status, 403, "localhost loophole: must not be 403; got {}", status);
}

/// localhost (no port) vs 127.0.0.1:8000 → allowed (loophole).
#[tokio::test]
async fn ai_chat_allows_localhost_no_port() {
    let router = make_router().await;
    let status =
        post_with_origin(&router, AI_CHAT, Some("http://localhost"), HOST, AI_BODY).await;
    assert_ne!(status, 403, "localhost no-port loophole: must not be 403; got {}", status);
}

/// 127.0.0.1:9999 vs 127.0.0.1:8000 → allowed (loophole).
#[tokio::test]
async fn ai_chat_allows_127_any_port() {
    let router = make_router().await;
    let status =
        post_with_origin(&router, AI_CHAT, Some("http://127.0.0.1:9999"), HOST, AI_BODY).await;
    assert_ne!(status, 403, "127.0.0.1 any-port loophole: must not be 403; got {}", status);
}

/// [::1]:9999 vs 127.0.0.1:8000 — IPv6 loopback behavior.
///
/// KNOWN BUG: `is_same_origin` splits the netloc on `:` to extract the
/// host-only portion, but `[::1]:9999` contains `:` inside the brackets.
/// `split(':').next()` on `[::1]:9999` yields `[` (not `[::1]`), so the
/// `matches!(host_only, ... "[::1]")` arm never matches. IPv6 loopback
/// origins therefore return 403. This is a defect in cc_bridge::is_same_origin;
/// it is documented here and left for a dedicated fix in a future cycle.
#[tokio::test]
async fn ai_chat_ipv6_loopback_returns_403_known_bug() {
    let router = make_router().await;
    let status =
        post_with_origin(&router, AI_CHAT, Some("http://[::1]:9999"), HOST, AI_BODY).await;
    // Currently 403 due to IPv6 bracket parsing bug in is_same_origin.
    // When the bug is fixed, change this assertion to assert_ne!(status, 403).
    assert_eq!(status, 403, "IPv6 loopback: known bug — currently 403; got {}", status);
}

/// tauri://localhost → allowed (Tauri app origin).
#[tokio::test]
async fn ai_chat_allows_tauri_localhost() {
    let router = make_router().await;
    let status =
        post_with_origin(&router, AI_CHAT, Some("tauri://localhost"), HOST, AI_BODY).await;
    assert_ne!(status, 403, "tauri://localhost must not be 403; got {}", status);
}

/// https://127.0.0.1:8000 vs 127.0.0.1:8000 — scheme mismatch.
/// Current behavior: is_same_origin compares only the netloc (host:port), so
/// scheme is ignored and this is allowed. Documented as-is; scheme tightening
/// is a separate future decision.
#[tokio::test]
async fn ai_chat_scheme_mismatch_allowed_current_behavior() {
    let router = make_router().await;
    let status =
        post_with_origin(&router, AI_CHAT, Some("https://127.0.0.1:8000"), HOST, AI_BODY).await;
    // NOT 403 — scheme is not part of the current same-origin check.
    // If this starts returning 403 after a future scheme-tightening change,
    // update this test accordingly.
    assert_ne!(
        status, 403,
        "scheme mismatch: current policy ignores scheme; must not be 403; got {}",
        status
    );
}

// ---------------------------------------------------------------------------
// POST /api/v1/ai/suggest
// ---------------------------------------------------------------------------

/// Absent Origin → 403.
#[tokio::test]
async fn ai_suggest_rejects_absent_origin() {
    let router = make_router().await;
    let status = post_with_origin(&router, AI_SUGGEST, None, HOST, AI_BODY).await;
    assert_eq!(status, 403, "absent Origin must be 403");
}

/// Empty string Origin → 403.
#[tokio::test]
async fn ai_suggest_rejects_empty_origin() {
    let router = make_router().await;
    let status = post_with_origin(&router, AI_SUGGEST, Some(""), HOST, AI_BODY).await;
    assert_eq!(status, 403, "empty Origin must be 403");
}

/// Matching Origin → allowed (503 — no key).
#[tokio::test]
async fn ai_suggest_allows_matching_origin() {
    let router = make_router().await;
    let status =
        post_with_origin(&router, AI_SUGGEST, Some(MATCHING_ORIGIN), HOST, AI_BODY).await;
    assert_ne!(status, 403, "matching Origin must not be 403; got {}", status);
}

/// Mismatched Origin → 403.
#[tokio::test]
async fn ai_suggest_rejects_mismatched_origin() {
    let router = make_router().await;
    let status = post_with_origin(&router, AI_SUGGEST, Some(EVIL_ORIGIN), HOST, AI_BODY).await;
    assert_eq!(status, 403, "mismatched Origin must be 403");
}

// ---------------------------------------------------------------------------
// POST /api/v1/chat (CRUD create)
// ---------------------------------------------------------------------------

/// Absent Origin → 403.
#[tokio::test]
async fn chat_create_rejects_absent_origin() {
    let router = make_router().await;
    let status = post_with_origin(&router, CHAT_CRUD, None, HOST, CHAT_BODY).await;
    assert_eq!(status, 403, "absent Origin must be 403");
}

/// Empty string Origin → 403.
#[tokio::test]
async fn chat_create_rejects_empty_origin() {
    let router = make_router().await;
    let status = post_with_origin(&router, CHAT_CRUD, Some(""), HOST, CHAT_BODY).await;
    assert_eq!(status, 403, "empty Origin must be 403");
}

/// Matching Origin → 201 created.
#[tokio::test]
async fn chat_create_allows_matching_origin() {
    let router = make_router().await;
    let status =
        post_with_origin(&router, CHAT_CRUD, Some(MATCHING_ORIGIN), HOST, CHAT_BODY).await;
    assert_eq!(status, 201, "matching Origin must create → 201; got {}", status);
}

/// Mismatched Origin → 403.
#[tokio::test]
async fn chat_create_rejects_mismatched_origin() {
    let router = make_router().await;
    let status = post_with_origin(&router, CHAT_CRUD, Some(EVIL_ORIGIN), HOST, CHAT_BODY).await;
    assert_eq!(status, 403, "mismatched Origin must be 403");
}

/// localhost:9999 → allowed (loophole).
#[tokio::test]
async fn chat_create_allows_localhost_any_port() {
    let router = make_router().await;
    let status =
        post_with_origin(&router, CHAT_CRUD, Some("http://localhost:9999"), HOST, CHAT_BODY).await;
    assert_ne!(status, 403, "localhost loophole: must not be 403; got {}", status);
}

/// tauri://localhost → allowed (Tauri app origin).
#[tokio::test]
async fn chat_create_allows_tauri_localhost() {
    let router = make_router().await;
    let status =
        post_with_origin(&router, CHAT_CRUD, Some("tauri://localhost"), HOST, CHAT_BODY).await;
    assert_ne!(status, 403, "tauri://localhost must not be 403; got {}", status);
}

// ---------------------------------------------------------------------------
// GET /api/v1/chat (CRUD list — reads also require Origin after tightening)
// ---------------------------------------------------------------------------

/// Absent Origin → 403 (reads also gated).
#[tokio::test]
async fn chat_list_rejects_absent_origin() {
    let router = make_router().await;
    let status = get_with_origin(&router, CHAT_CRUD, None, HOST).await;
    assert_eq!(status, 403, "absent Origin on GET must be 403");
}

/// Empty string Origin → 403.
#[tokio::test]
async fn chat_list_rejects_empty_origin() {
    let router = make_router().await;
    let status = get_with_origin(&router, CHAT_CRUD, Some(""), HOST).await;
    assert_eq!(status, 403, "empty Origin on GET must be 403");
}

/// Matching Origin → 200.
#[tokio::test]
async fn chat_list_allows_matching_origin() {
    let router = make_router().await;
    let status = get_with_origin(&router, CHAT_CRUD, Some(MATCHING_ORIGIN), HOST).await;
    assert_eq!(status, 200, "matching Origin on GET must be 200; got {}", status);
}

/// Mismatched Origin → 403.
#[tokio::test]
async fn chat_list_rejects_mismatched_origin() {
    let router = make_router().await;
    let status = get_with_origin(&router, CHAT_CRUD, Some(EVIL_ORIGIN), HOST).await;
    assert_eq!(status, 403, "mismatched Origin on GET must be 403");
}

/// localhost:9999 → allowed (loophole).
#[tokio::test]
async fn chat_list_allows_localhost_any_port() {
    let router = make_router().await;
    let status = get_with_origin(&router, CHAT_CRUD, Some("http://localhost:9999"), HOST).await;
    assert_ne!(status, 403, "localhost loophole on GET: must not be 403; got {}", status);
}
