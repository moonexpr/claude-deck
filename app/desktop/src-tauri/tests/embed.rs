/// A3 headless proof: server-core embeds and answers expected routes.
///
/// Uses `tower::ServiceExt::oneshot` — no socket, no HTTP client, no network.
/// The router is built exactly as the Tauri setup hook builds it, minus the
/// Tauri plumbing (no window, no app_data_dir — we use a tempdir instead).
use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn embedded_server_answers_health_and_status() {
    // ---- Build a ServerConfig with a temp dir so the test is self-contained.
    let tmp = std::env::temp_dir().join(format!(
        "claude_deck_a3_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&tmp).expect("failed to create temp dir");

    let db_url = format!("sqlite:{}?mode=rwc", tmp.join("test.db").display());

    let config = server_core::ServerConfig {
        db_url,
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        anthropic_api_key: None,
        enable_external_tools: false, // skip PATH-dependent tool discovery
        cwd_fallback: tmp.clone(),
    };

    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    // ---- Test GET /health → 200 ---------------------------------------------
    let health_req = axum::http::Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let health_resp = router
        .clone()
        .oneshot(health_req)
        .await
        .expect("GET /health failed");

    assert_eq!(
        health_resp.status(),
        axum::http::StatusCode::OK,
        "GET /health should return 200"
    );

    let body_bytes = health_resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect /health body")
        .to_bytes();
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("GET /health body is not valid JSON");

    assert_eq!(body["status"], "running", "/health status field should be 'running'");
    assert!(
        body.get("name").is_some(),
        "/health response should have 'name' field"
    );
    assert!(
        body.get("version").is_some(),
        "/health response should have 'version' field"
    );

    // ---- Test GET /api/v1/status → 200 --------------------------------------
    let status_req = axum::http::Request::builder()
        .method("GET")
        .uri("/api/v1/status")
        .body(Body::empty())
        .unwrap();

    let status_resp = router
        .oneshot(status_req)
        .await
        .expect("GET /api/v1/status failed");

    assert_eq!(
        status_resp.status(),
        axum::http::StatusCode::OK,
        "GET /api/v1/status should return 200"
    );

    // ---- Cleanup ------------------------------------------------------------
    let _ = std::fs::remove_dir_all(&tmp);
}
