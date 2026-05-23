/// Phase C, task C3 — chat CRUD integration tests.
///
/// Tests the `/api/v1/chat` endpoints via `tower::ServiceExt::oneshot`.
/// All tests use a fresh tempdir and in-process SQLite DB so they are fully
/// isolated from each other and from any on-disk databases.
///
/// Axum 0.8 trailing-slash note: GET /api/v1/chat (no trailing slash) matches
/// the `"/"` route in the nested chat router. GET /api/v1/chat/ (with trailing
/// slash) does NOT match in axum 0.8 without a NormalizePathLayer. All tests
/// use the no-trailing-slash form for the list endpoint.
///
/// Covered cases (8 total):
///   1. migration_creates_table
///   2. migration_idempotent_on_existing_db
///   3. crud_round_trip
///   4. list_returns_message_count_not_messages
///   5. bad_origin_rejected
///   6. validation_rejects_empty_title
///   7. put_404_on_missing_id
///   8. delete_204_then_get_404
use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers — mirroring the pattern in tests/routes.rs
// ---------------------------------------------------------------------------

/// Create a uniquely-named tempdir for a single test run.
fn make_tmp() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "claude_deck_chat_test_{}_{}_{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos(),
    ));
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    tmp
}

fn make_config(tmp: &std::path::Path) -> server_core::ServerConfig {
    let db_url = format!("sqlite:{}?mode=rwc", tmp.join("test.db").display());
    server_core::ServerConfig {
        db_url,
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        key_source: server_core::KeySource::None,
        anthropic_base_url: "https://api.anthropic.com".to_string(),
        enable_external_tools: false,
        cwd_fallback: tmp.to_path_buf(),
    }
}

// Default host/origin used by all same-origin helpers below.
// Absent-Origin requests now return 403 (parity with cc-bridge WS).
const TEST_HOST: &str = "127.0.0.1:8000";
const TEST_ORIGIN: &str = "http://127.0.0.1:8000";

/// Issue a GET request and return (status, body-bytes).
///
/// Includes a matching Origin + Host so same-origin check passes.
async fn do_get(router: &axum::Router, uri: &str) -> (u16, bytes::Bytes) {
    let req = axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("origin", TEST_ORIGIN)
        .header("host", TEST_HOST)
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status().as_u16();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, body)
}

/// Issue a POST with a JSON body, return (status, body-bytes).
///
/// Includes a matching Origin + Host so same-origin check passes.
async fn do_post(router: &axum::Router, uri: &str, body: &str) -> (u16, bytes::Bytes) {
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("origin", TEST_ORIGIN)
        .header("host", TEST_HOST)
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, bytes)
}

/// Issue a POST with a custom Origin header.
async fn do_post_with_origin(
    router: &axum::Router,
    uri: &str,
    body: &str,
    origin: &str,
    host: &str,
) -> u16 {
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("origin", origin)
        .header("host", host)
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    resp.status().as_u16()
}

/// Issue a PUT with a JSON body, return (status, body-bytes).
///
/// Includes a matching Origin + Host so same-origin check passes.
async fn do_put(router: &axum::Router, uri: &str, body: &str) -> (u16, bytes::Bytes) {
    let req = axum::http::Request::builder()
        .method("PUT")
        .uri(uri)
        .header("content-type", "application/json")
        .header("origin", TEST_ORIGIN)
        .header("host", TEST_HOST)
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, bytes)
}

/// Issue a DELETE, return status code.
///
/// Includes a matching Origin + Host so same-origin check passes.
async fn do_delete(router: &axum::Router, uri: &str) -> u16 {
    let req = axum::http::Request::builder()
        .method("DELETE")
        .uri(uri)
        .header("origin", TEST_ORIGIN)
        .header("host", TEST_HOST)
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    resp.status().as_u16()
}

/// Query sqlite_master to check if a table exists.
async fn table_exists(pool: &sqlx::SqlitePool, name: &str) -> bool {
    use sqlx::Row as _;
    sqlx::query("SELECT COUNT(*) AS cnt FROM sqlite_master WHERE type='table' AND name=?")
        .bind(name)
        .fetch_one(pool)
        .await
        .map(|r| r.get::<i64, _>("cnt") > 0)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Test 1: migration_creates_table
// ---------------------------------------------------------------------------

/// After `app(config)` completes, the `chat_conversations` table must exist.
/// Verified by hitting the list endpoint which queries the table.
#[tokio::test]
async fn migration_creates_table() {
    let tmp = make_tmp();
    let config = make_config(&tmp);
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    // GET /api/v1/chat (no trailing slash — axum 0.8 is slash-strict)
    let (status, _) = do_get(&router, "/api/v1/chat").await;
    assert_eq!(
        status, 200,
        "list endpoint must return 200 — table must exist"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Test 2: migration_idempotent_on_existing_db
// ---------------------------------------------------------------------------

/// Pre-populate the DB with a non-chat table, then call `app` twice.
/// The phase-B table must survive, and `chat_conversations` must appear.
#[tokio::test]
async fn migration_idempotent_on_existing_db() {
    let tmp = make_tmp();
    let db_path = tmp.join("test.db");
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

    // Seed the DB with a "phase-B" table using raw sqlx before the app boots.
    {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .expect("pre-seed pool");
        sqlx::query("CREATE TABLE IF NOT EXISTS phase_b_table (id INTEGER PRIMARY KEY)")
            .execute(&pool)
            .await
            .expect("create phase_b_table");
        pool.close().await;
    }

    // Boot the app (runs migration).
    let config = server_core::ServerConfig {
        db_url: db_url.clone(),
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        key_source: server_core::KeySource::None,
        anthropic_base_url: "https://api.anthropic.com".to_string(),
        enable_external_tools: false,
        cwd_fallback: tmp.to_path_buf(),
    };
    let _ = server_core::app(config).await.expect("first app() call");

    // Boot again — migration must be idempotent (no error on second invocation).
    let config2 = server_core::ServerConfig {
        db_url: db_url.clone(),
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        key_source: server_core::KeySource::None,
        anthropic_base_url: "https://api.anthropic.com".to_string(),
        enable_external_tools: false,
        cwd_fallback: tmp.to_path_buf(),
    };
    let router = server_core::app(config2)
        .await
        .expect("second app() call must not fail");

    // Verify both tables exist via sqlite_master using a fresh pool.
    let verify_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&db_url)
        .await
        .expect("verify pool");

    assert!(
        table_exists(&verify_pool, "phase_b_table").await,
        "phase_b_table must survive migration"
    );
    assert!(
        table_exists(&verify_pool, "chat_conversations").await,
        "chat_conversations must exist after migration"
    );

    // list endpoint must also work (full integration check).
    let (status, _) = do_get(&router, "/api/v1/chat").await;
    assert_eq!(status, 200, "list endpoint OK after idempotent migration");

    verify_pool.close().await;
    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Test 3: crud_round_trip
// ---------------------------------------------------------------------------

/// POST → GET → PUT → GET → DELETE → GET(404)
#[tokio::test]
async fn crud_round_trip() {
    let tmp = make_tmp();
    let config = make_config(&tmp);
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    // POST — create
    let create_body = r#"{"title":"Round Trip","messages":[{"role":"user","content":"Hello"}]}"#;
    let (status, bytes) = do_post(&router, "/api/v1/chat", create_body).await;
    assert_eq!(
        status,
        201,
        "POST /chat must return 201; body: {}",
        String::from_utf8_lossy(&bytes)
    );

    let created: serde_json::Value =
        serde_json::from_slice(&bytes).expect("POST response must be JSON");
    let id = created["id"].as_str().expect("id field must be present");
    assert_eq!(created["title"], "Round Trip");
    assert_eq!(created["messages"][0]["role"], "user");
    assert_eq!(created["messages"][0]["content"], "Hello");
    assert!(
        created["created_at"].is_number(),
        "created_at must be a number"
    );
    assert!(
        created["updated_at"].is_number(),
        "updated_at must be a number"
    );

    // GET single — verify round-trip
    let uri = format!("/api/v1/chat/{}", id);
    let (status, bytes) = do_get(&router, &uri).await;
    assert_eq!(status, 200, "GET /chat/{{id}} must return 200");
    let fetched: serde_json::Value = serde_json::from_slice(&bytes).expect("GET response JSON");
    assert_eq!(fetched["title"], "Round Trip");
    assert_eq!(fetched["messages"][0]["content"], "Hello");

    // PUT — replace messages
    let update_body =
        r#"{"messages":[{"role":"user","content":"Updated"},{"role":"assistant","content":"OK"}]}"#;
    let (status, bytes) = do_put(&router, &uri, update_body).await;
    assert_eq!(status, 200, "PUT /chat/{{id}} must return 200");
    let updated: serde_json::Value = serde_json::from_slice(&bytes).expect("PUT response JSON");
    assert_eq!(updated["messages"].as_array().unwrap().len(), 2);
    assert_eq!(updated["messages"][0]["content"], "Updated");
    // title unchanged
    assert_eq!(updated["title"], "Round Trip");

    // GET again — verify PUT persisted
    let (status, bytes) = do_get(&router, &uri).await;
    assert_eq!(status, 200, "GET after PUT must return 200");
    let refetched: serde_json::Value = serde_json::from_slice(&bytes).expect("GET after PUT JSON");
    assert_eq!(refetched["messages"].as_array().unwrap().len(), 2);

    // DELETE
    let status = do_delete(&router, &uri).await;
    assert_eq!(status, 204, "DELETE must return 204");

    // GET after DELETE → 404
    let (status, bytes) = do_get(&router, &uri).await;
    assert_eq!(status, 404, "GET after DELETE must return 404");
    let err: serde_json::Value = serde_json::from_slice(&bytes).expect("404 body must be JSON");
    assert_eq!(err["status"], "not_found");

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Test 4: list_returns_message_count_not_messages
// ---------------------------------------------------------------------------

/// POST two conversations with different message counts.
/// GET /api/v1/chat must return `message_count` and NOT a `messages` field.
/// Ordering must be by `updated_at DESC`.
#[tokio::test]
async fn list_returns_message_count_not_messages() {
    let tmp = make_tmp();
    let config = make_config(&tmp);
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    // POST first conversation (1 message).
    let body1 = r#"{"title":"Alpha","messages":[{"role":"user","content":"one"}]}"#;
    let (status, bytes) = do_post(&router, "/api/v1/chat", body1).await;
    assert_eq!(
        status,
        201,
        "POST alpha: {}",
        String::from_utf8_lossy(&bytes)
    );
    let alpha: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let alpha_id = alpha["id"].as_str().unwrap().to_string();

    // POST second conversation (2 messages).
    let body2 = r#"{"title":"Beta","messages":[{"role":"user","content":"one"},{"role":"assistant","content":"two"}]}"#;
    let (status, bytes) = do_post(&router, "/api/v1/chat", body2).await;
    assert_eq!(
        status,
        201,
        "POST beta: {}",
        String::from_utf8_lossy(&bytes)
    );
    let beta: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let beta_id = beta["id"].as_str().unwrap().to_string();

    // Touch Beta via a PUT so its updated_at is refreshed.
    let _ = do_put(
        &router,
        &format!("/api/v1/chat/{}", beta_id),
        r#"{"title":"Beta Updated"}"#,
    )
    .await;

    // GET list (no trailing slash).
    let (status, bytes) = do_get(&router, "/api/v1/chat").await;
    assert_eq!(status, 200, "GET /chat list");
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let convs = list["conversations"]
        .as_array()
        .expect("conversations array");

    // Must have at least the two we created.
    assert!(convs.len() >= 2, "expected at least 2 conversations");

    // Every item must have `message_count`, not `messages`.
    for item in convs.iter() {
        assert!(
            item.get("message_count").is_some(),
            "list item must have message_count; got: {}",
            item
        );
        assert!(
            item.get("messages").is_none(),
            "list item must NOT have messages blob; got: {}",
            item
        );
    }

    // Find our two conversations in the list.
    let alpha_item = convs
        .iter()
        .find(|c| c["id"].as_str() == Some(&alpha_id))
        .expect("alpha in list")
        .clone();
    let beta_item = convs
        .iter()
        .find(|c| c["id"].as_str() == Some(&beta_id))
        .expect("beta in list")
        .clone();

    assert_eq!(
        alpha_item["message_count"].as_i64().unwrap(),
        1,
        "alpha message_count"
    );
    assert_eq!(
        beta_item["message_count"].as_i64().unwrap(),
        2,
        "beta message_count"
    );

    // Beta was updated more recently so its updated_at must be >= alpha's.
    let alpha_ts = alpha_item["updated_at"].as_i64().unwrap();
    let beta_ts = beta_item["updated_at"].as_i64().unwrap();
    assert!(
        beta_ts >= alpha_ts,
        "beta updated_at ({}) should be >= alpha updated_at ({})",
        beta_ts,
        alpha_ts
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Test 5: bad_origin_rejected
// ---------------------------------------------------------------------------

/// A request with a cross-origin `Origin` header must be rejected with 403.
#[tokio::test]
async fn bad_origin_rejected() {
    let tmp = make_tmp();
    let config = make_config(&tmp);
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    let status = do_post_with_origin(
        &router,
        "/api/v1/chat",
        r#"{"title":"Evil","messages":[]}"#,
        "http://evil.example",
        "127.0.0.1:8000",
    )
    .await;

    assert_eq!(status, 403, "cross-origin POST must be rejected with 403");

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Test 6: validation_rejects_empty_title
// ---------------------------------------------------------------------------

/// POST with an empty `title` must return 400.
#[tokio::test]
async fn validation_rejects_empty_title() {
    let tmp = make_tmp();
    let config = make_config(&tmp);
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    let (status, bytes) = do_post(&router, "/api/v1/chat", r#"{"title":"","messages":[]}"#).await;

    assert_eq!(
        status,
        400,
        "empty title must return 400; body: {}",
        String::from_utf8_lossy(&bytes)
    );
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("400 body must be JSON");
    assert_eq!(body["status"], "bad_request");

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Test 7: put_404_on_missing_id
// ---------------------------------------------------------------------------

/// PUT with an unknown ID must return 404.
#[tokio::test]
async fn put_404_on_missing_id() {
    let tmp = make_tmp();
    let config = make_config(&tmp);
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    let (status, bytes) = do_put(
        &router,
        "/api/v1/chat/does-not-exist",
        r#"{"title":"Ghost"}"#,
    )
    .await;

    assert_eq!(
        status,
        404,
        "PUT on missing id must return 404; body: {}",
        String::from_utf8_lossy(&bytes)
    );
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("404 body must be JSON");
    assert_eq!(body["status"], "not_found");

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Test 8: delete_204_then_get_404
// ---------------------------------------------------------------------------

/// DELETE returns 204; subsequent GET on the same ID returns 404.
#[tokio::test]
async fn delete_204_then_get_404() {
    let tmp = make_tmp();
    let config = make_config(&tmp);
    let router = server_core::app(config)
        .await
        .expect("server_core::app failed");

    // Create a conversation.
    let (status, bytes) = do_post(
        &router,
        "/api/v1/chat",
        r#"{"title":"To Be Deleted","messages":[{"role":"user","content":"bye"}]}"#,
    )
    .await;
    assert_eq!(status, 201, "POST: {}", String::from_utf8_lossy(&bytes));
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    let uri = format!("/api/v1/chat/{}", id);

    // DELETE → 204
    let status = do_delete(&router, &uri).await;
    assert_eq!(status, 204, "DELETE must return 204");

    // GET → 404
    let (status, bytes) = do_get(&router, &uri).await;
    assert_eq!(status, 404, "GET after DELETE must return 404");
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("404 body JSON");
    assert_eq!(body["status"], "not_found");

    let _ = std::fs::remove_dir_all(&tmp);
}
