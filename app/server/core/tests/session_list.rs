//! Session list endpoints — integration tests.
//!
//! Verifies the Rust list/projects endpoints (previously stubs) enumerate
//! `projects_dir` and return summaries matching the frontend contract.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

const SESS_A: &str = concat!(
    r#"{"type":"user","timestamp":"2026-06-04T10:00:00Z","message":{"role":"user","content":"do a thing"}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-06-04T10:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"},{"type":"tool_use","name":"Bash","input":{}}]}}"#,
    "\n",
);
const SESS_B: &str = concat!(
    r#"{"type":"user","timestamp":"2026-06-04T09:00:00Z","message":{"role":"user","content":"earlier session"}}"#,
    "\n",
);

fn make_tmp() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "claude_deck_sesslist_{}_{}_{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos(),
    ));
    let proj = tmp.join("projects").join("proj");
    std::fs::create_dir_all(&proj).expect("mk projects/proj");
    std::fs::write(proj.join("a.jsonl"), SESS_A).unwrap();
    std::fs::write(proj.join("b.jsonl"), SESS_B).unwrap();
    tmp
}

fn config(tmp: &std::path::Path) -> server_core::ServerConfig {
    server_core::ServerConfig {
        db_url: format!("sqlite:{}?mode=rwc", tmp.join("test.db").display()),
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        key_source: server_core::KeySource::None,
        anthropic_base_url: "http://unused".into(),
        enable_external_tools: false,
        cwd_fallback: tmp.to_path_buf(),
    }
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
async fn lists_sessions_with_summaries_and_counts() {
    let tmp = make_tmp();
    let app = server_core::app(config(&tmp)).await.expect("app builds");

    let (status, body) = get(&app, "/api/v1/sessions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);

    // Order is by file mtime (not content timestamp), so locate by summary.
    let a = sessions
        .iter()
        .find(|s| s["summary"] == "do a thing")
        .expect("session a present");
    assert_eq!(a["project_name"], "proj");
    assert_eq!(a["total_messages"], 2);
    assert_eq!(a["total_tool_calls"], 1);
    assert!(a["modified_at"].as_str().unwrap().starts_with("2026"));
    assert!(a["size_bytes"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn lists_projects_with_counts() {
    let tmp = make_tmp();
    let app = server_core::app(config(&tmp)).await.expect("app builds");

    let (status, body) = get(&app, "/api/v1/sessions/projects").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_sessions"], 2);
    let projects = body["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["folder"], "proj");
    assert_eq!(projects[0]["session_count"], 2);
}

#[tokio::test]
async fn limit_and_project_filter_apply() {
    let tmp = make_tmp();
    let app = server_core::app(config(&tmp)).await.expect("app builds");

    let (_s, body) = get(&app, "/api/v1/sessions?project_folder=proj&limit=1").await;
    // total reflects all matched files; the page is capped by limit.
    assert_eq!(body["total"], 2);
    assert_eq!(body["sessions"].as_array().unwrap().len(), 1);

    let (_s2, empty) = get(&app, "/api/v1/sessions?project_folder=nope").await;
    assert_eq!(empty["total"], 0);
}

#[tokio::test]
async fn dashboard_stats_counts_sessions() {
    let tmp = make_tmp();
    let app = server_core::app(config(&tmp)).await.expect("app builds");

    let (status, body) = get(&app, "/api/v1/sessions/dashboard/stats").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_sessions"], 2);
    assert_eq!(body["most_active_project"], "proj");
    // 3 non-empty JSONL entries across the two fixtures.
    assert_eq!(body["total_messages"], 3);
}
