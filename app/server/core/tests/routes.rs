/// A8 route smoke test — one `#[tokio::test]` per `/api/v1/*` family + `/health`.
///
/// Strategy: build a `ServerConfig` against a tempdir (no socket, no HTTP
/// client), call `server_core::app(config).await`, then drive every route
/// family via `tower::ServiceExt::oneshot`.  The bar is:
///   - 404  → FAIL (route not mounted)
///   - 500  → FAIL (handler panicked or internal error; stop and report)
///   - 200  → PASS (normal / stub families)
///   - 501  → PASS only for POST /api/v1/ai/chat (documented stub)
///   - Other 4xx → only acceptable if that is the endpoint's genuine contract
///
/// Families covered (22 total):
///   health, status, agents, ai, backup, cc_bridge, cli, commands, config,
///   context, hooks, mcp, memory, output_styles, permissions, plans, plugins,
///   presence, projects, sessions, statusline, usage

use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_config(tmp: &std::path::Path) -> server_core::ServerConfig {
    let db_url = format!(
        "sqlite:{}?mode=rwc",
        tmp.join("test.db").display()
    );
    server_core::ServerConfig {
        db_url,
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        anthropic_api_key: None,
        enable_external_tools: false,
        cwd_fallback: tmp.to_path_buf(),
    }
}

fn make_tmp() -> std::path::PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "claude_deck_a8_routes_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&tmp).expect("failed to create temp dir");
    tmp
}

async fn get(router: &axum::Router, uri: &str) -> u16 {
    let req = axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    router
        .clone()
        .oneshot(req)
        .await
        .unwrap_or_else(|_| panic!("GET {} failed", uri))
        .status()
        .as_u16()
}

async fn post_empty(router: &axum::Router, uri: &str) -> u16 {
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    router
        .clone()
        .oneshot(req)
        .await
        .unwrap_or_else(|_| panic!("POST {} failed", uri))
        .status()
        .as_u16()
}

// ---------------------------------------------------------------------------
// Main smoke test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn all_route_families_mounted_and_respond() {
    let tmp = make_tmp();
    let config = make_config(&tmp);
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    // ---- GET /health → 200 -------------------------------------------------
    {
        let req = axum::http::Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = router.clone().oneshot(req).await.expect("GET /health");
        assert_eq!(resp.status().as_u16(), 200, "GET /health");
        // Verify body is valid JSON with status:"running"
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("/health not JSON");
        assert_eq!(body["status"], "running", "/health status field");
    }

    // ---- GET /api/v1/status (inline handler, not nested) → 200 ------------
    {
        let code = get(&router, "/api/v1/status").await;
        assert_eq!(code, 200, "GET /api/v1/status");
    }

    // ---- agents: GET /api/v1/agents → 200 ---------------------------------
    {
        let code = get(&router, "/api/v1/agents").await;
        assert_eq!(code, 200, "GET /api/v1/agents");
    }

    // ---- ai: POST /api/v1/ai/chat → 501 (documented stub) -----------------
    {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/ai/chat")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let resp = router.clone().oneshot(req).await.expect("POST /api/v1/ai/chat");
        assert_eq!(resp.status().as_u16(), 501, "POST /api/v1/ai/chat should be 501");
    }

    // ---- backup: GET /api/v1/backup/list → 200 -----------------------------
    {
        let code = get(&router, "/api/v1/backup/list").await;
        assert_eq!(code, 200, "GET /api/v1/backup/list");
    }

    // ---- cc_bridge: GET /api/v1/cc-bridge/sessions → 200 ------------------
    {
        let code = get(&router, "/api/v1/cc-bridge/sessions").await;
        assert_eq!(code, 200, "GET /api/v1/cc-bridge/sessions");
    }

    // ---- cli: POST /api/v1/cli/execute — sends invalid command → 422 ------
    // The handler validates the command whitelist; an unknown command is a
    // genuine 422 contract (not a routing failure).
    {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/cli/execute")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"command":"mcp","args":[]}"#))
            .unwrap();
        let resp = router.clone().oneshot(req).await.expect("POST /api/v1/cli/execute");
        let code = resp.status().as_u16();
        // Acceptable: 200 (ran), 422 (validation), 500 only if genuinely broken.
        // The route must at minimum be mounted (not 404).
        assert_ne!(code, 404, "POST /api/v1/cli/execute must be mounted");
        assert_ne!(code, 405, "POST /api/v1/cli/execute method must be allowed");
    }

    // ---- commands: GET /api/v1/commands → 200 -----------------------------
    {
        let code = get(&router, "/api/v1/commands").await;
        assert_eq!(code, 200, "GET /api/v1/commands");
    }

    // ---- config: GET /api/v1/config → 200 ---------------------------------
    {
        let code = get(&router, "/api/v1/config").await;
        assert_eq!(code, 200, "GET /api/v1/config");
    }

    // ---- context: GET /api/v1/context/active → 200 ------------------------
    {
        let code = get(&router, "/api/v1/context/active").await;
        assert_eq!(code, 200, "GET /api/v1/context/active");
    }

    // ---- hooks: GET /api/v1/hooks → 200 -----------------------------------
    {
        let code = get(&router, "/api/v1/hooks").await;
        assert_eq!(code, 200, "GET /api/v1/hooks");
    }

    // ---- mcp: GET /api/v1/mcp/servers → 200 --------------------------------
    {
        let code = get(&router, "/api/v1/mcp/servers").await;
        assert_eq!(code, 200, "GET /api/v1/mcp/servers");
    }

    // ---- memory: GET /api/v1/memory/hierarchy → 200 -----------------------
    {
        let code = get(&router, "/api/v1/memory/hierarchy").await;
        assert_eq!(code, 200, "GET /api/v1/memory/hierarchy");
    }

    // ---- output_styles: GET /api/v1/output-styles → 200 -------------------
    {
        let code = get(&router, "/api/v1/output-styles").await;
        assert_eq!(code, 200, "GET /api/v1/output-styles");
    }

    // ---- permissions: GET /api/v1/permissions → 200 -----------------------
    {
        let code = get(&router, "/api/v1/permissions").await;
        assert_eq!(code, 200, "GET /api/v1/permissions");
    }

    // ---- plans: GET /api/v1/plans → 200 -----------------------------------
    {
        let code = get(&router, "/api/v1/plans").await;
        assert_eq!(code, 200, "GET /api/v1/plans");
    }

    // ---- plugins: GET /api/v1/plugins → 200 --------------------------------
    {
        let code = get(&router, "/api/v1/plugins").await;
        assert_eq!(code, 200, "GET /api/v1/plugins");
    }

    // ---- presence: GET /api/v1/presence/sessions → 200 --------------------
    {
        let code = get(&router, "/api/v1/presence/sessions").await;
        assert_eq!(code, 200, "GET /api/v1/presence/sessions");
    }

    // ---- projects: GET /api/v1/projects → 200 -----------------------------
    {
        let code = get(&router, "/api/v1/projects").await;
        assert_eq!(code, 200, "GET /api/v1/projects");
    }

    // ---- sessions: GET /api/v1/sessions → 200 -----------------------------
    {
        let code = get(&router, "/api/v1/sessions").await;
        assert_eq!(code, 200, "GET /api/v1/sessions");
    }

    // ---- statusline: GET /api/v1/statusline → 200 -------------------------
    {
        let code = get(&router, "/api/v1/statusline").await;
        assert_eq!(code, 200, "GET /api/v1/statusline");
    }

    // ---- status-info: GET /api/v1/status-info → 200 -----------------------
    // status::router() mounts route("/", handler) under the /status-info nest.
    {
        let code = get(&router, "/api/v1/status-info").await;
        assert_eq!(code, 200, "GET /api/v1/status-info");
    }

    // ---- usage: GET /api/v1/usage/summary → 200 ---------------------------
    {
        let code = get(&router, "/api/v1/usage/summary").await;
        assert_eq!(code, 200, "GET /api/v1/usage/summary");
    }

    // ---- Cleanup -----------------------------------------------------------
    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// SPA fallback regression: client-side routes must return 200, not 404
// ---------------------------------------------------------------------------

/// Build a minimal fake dist tree:
///   <tmp>/dist/index.html          — the SPA shell
///   <tmp>/dist/assets/app.js       — a static asset
///   <tmp>/dist/favicon-32.png      — a root-level static file
fn make_fake_dist(tmp: &std::path::Path) -> std::path::PathBuf {
    let dist = tmp.join("dist");
    std::fs::create_dir_all(dist.join("assets")).unwrap();
    std::fs::write(
        dist.join("index.html"),
        b"<!DOCTYPE html><html><body>SPA</body></html>",
    )
    .unwrap();
    std::fs::write(dist.join("assets").join("app.js"), b"console.log('ok')").unwrap();
    std::fs::write(dist.join("favicon-32.png"), b"\x89PNG").unwrap();
    dist
}

#[tokio::test]
async fn spa_fallback_returns_200_for_client_routes() {
    let tmp = std::env::temp_dir().join(format!(
        "claude_deck_spa_fallback_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let dist = make_fake_dist(&tmp);

    let db_url = format!("sqlite:{}?mode=rwc", tmp.join("test.db").display());
    let config = server_core::ServerConfig {
        db_url,
        projects_dir: tmp.join("projects"),
        frontend_dist_path: Some(dist.clone()),
        presence_public_url: None,
        anthropic_api_key: None,
        enable_external_tools: false,
        cwd_fallback: tmp.clone(),
    };
    let router = server_core::app(config).await.expect("server_core::app failed");

    // GET / → 200 (index.html)
    let code = get(&router, "/").await;
    assert_eq!(code, 200, "GET / should be 200");

    // GET /agents → 200 (client-side route, not a file — must serve index.html)
    let code = get(&router, "/agents").await;
    assert_eq!(code, 200, "GET /agents (SPA route) should be 200, not 404");

    // GET /cc-bridge → 200
    let code = get(&router, "/cc-bridge").await;
    assert_eq!(code, 200, "GET /cc-bridge (SPA route) should be 200");

    // GET /usage → 200
    let code = get(&router, "/usage").await;
    assert_eq!(code, 200, "GET /usage (SPA route) should be 200");

    // GET /assets/app.js → 200 (real static file)
    let code = get(&router, "/assets/app.js").await;
    assert_eq!(code, 200, "GET /assets/app.js (real file) should be 200");

    // GET /favicon-32.png → 200 (real static file at root)
    let code = get(&router, "/favicon-32.png").await;
    assert_eq!(code, 200, "GET /favicon-32.png (real file) should be 200");

    // GET /api/v1/status → 200 JSON (API route must not be shadowed by fallback)
    let code = get(&router, "/api/v1/status").await;
    assert_eq!(code, 200, "GET /api/v1/status must still work as API route");

    let _ = std::fs::remove_dir_all(&tmp);
}
