//! Insight Platform M1 integration tests (Plan 0004a validator #1).
//!
//! Drives POST /api/v1/insights/session/{project}/{session}/analyze through a
//! real axum router with a wiremock'd Anthropic, and asserts the claim-level
//! two-check provenance gate: a grounded item persists; a citation error and a
//! groundedness error are dropped and counted, not stored.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LOCATOR: &str = "2026-06-04T10:00:00Z";

/// Fixture: one user turn + one assistant reply. The assistant text contains
/// the grounded quote; the conversation's locator is the user timestamp.
const FIXTURE: &str = concat!(
    r#"{"type":"user","timestamp":"2026-06-04T10:00:00Z","message":{"role":"user","content":"Should we merge PR nine?"}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-06-04T10:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"I merged PR nine after independent validation passed; build and tests were green."}]}}"#,
    "\n",
);

fn make_tmp() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "claude_deck_insights_test_{}_{}_{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos(),
    ));
    std::fs::create_dir_all(tmp.join("projects").join("proj")).expect("mk projects/proj");
    std::fs::write(tmp.join("projects").join("proj").join("sess.jsonl"), FIXTURE).expect("write fixture");
    tmp
}

fn make_config(
    tmp: &std::path::Path,
    anthropic_base_url: String,
    key_source: server_core::KeySource,
) -> server_core::ServerConfig {
    server_core::ServerConfig {
        db_url: format!("sqlite:{}?mode=rwc", tmp.join("test.db").display()),
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        key_source,
        anthropic_base_url,
        enable_external_tools: false,
        cwd_fallback: tmp.to_path_buf(),
    }
}

/// Canned Anthropic non-streaming tool_use response: a grounded judgment call,
/// a citation error (unknown locator), a groundedness error (quote absent),
/// plus an ungated summary + follow-up.
fn canned_tool_use() -> serde_json::Value {
    serde_json::json!({
        "content": [{
            "type": "tool_use",
            "name": "record_session_insight",
            "input": {
                "summary": "Worked the open PR queue to resolution.",
                "decisions": [
                    { "text": "this cites a turn that does not exist",
                      "source_ref": "NO-SUCH-TIMESTAMP",
                      "quote": "irrelevant" }
                ],
                "judgment_calls": [
                    { "summary": "merge PR nine",
                      "options": ["merge", "hold"],
                      "chosen": "merge",
                      "rationale": "fully validated, closes a tracked bug",
                      "source_ref": "2026-06-04T10:00:00Z",
                      "quote": "merged PR nine after independent validation" }
                ],
                "errors_hit": [
                    { "text": "this quote is not in the transcript",
                      "source_ref": "2026-06-04T10:00:00Z",
                      "quote": "a phrase that appears nowhere in the session" }
                ],
                "follow_ups": ["write the docs"]
            }
        }],
        "usage": { "input_tokens": 512, "output_tokens": 88 }
    })
}

async fn json_body(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn analyze_req() -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/insights/session/proj/sess/analyze")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn gate_keeps_grounded_and_drops_citation_and_groundedness_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(canned_tool_use()))
        .mount(&server)
        .await;

    let tmp = make_tmp();
    let config = make_config(&tmp, server.uri(), server_core::KeySource::ApiKey("sk-ant-test".into()));
    let app = server_core::app(config).await.expect("app builds");

    let resp = app.oneshot(analyze_req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;

    assert_eq!(body["status"], "done");

    // The grounded judgment call survives, with its provenance intact.
    let jcs = body["judgment_calls"].as_array().unwrap();
    assert_eq!(jcs.len(), 1, "only the grounded judgment call persists");
    assert_eq!(jcs[0]["summary"], "merge PR nine");
    assert_eq!(jcs[0]["source_locator"], LOCATOR);
    assert_eq!(jcs[0]["quote"], "merged PR nine after independent validation");
    assert_eq!(jcs[0]["options"], serde_json::json!(["merge", "hold"]));

    // Both bad items were dropped and counted separately.
    assert_eq!(body["dropped"]["citation_error"], 1);
    assert_eq!(body["dropped"]["groundedness_error"], 1);

    // insights = ungated summary + follow-up only; no grounded Decision/Error
    // survived (the decision was a citation error, the error a groundedness one).
    let kinds: Vec<String> = body["insights"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap().to_string())
        .collect();
    assert!(kinds.contains(&"Summary".to_string()));
    assert!(kinds.contains(&"Follow-up".to_string()));
    assert!(!kinds.contains(&"Decision".to_string()), "citation-error decision must be dropped");
    assert!(!kinds.contains(&"Error".to_string()), "groundedness-error must be dropped");
}

#[tokio::test]
async fn no_credential_returns_503_with_key_source() {
    let tmp = make_tmp();
    let config = make_config(&tmp, "http://unused".into(), server_core::KeySource::None);
    let app = server_core::app(config).await.expect("app builds");

    let resp = app.oneshot(analyze_req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = json_body(resp).await;
    assert_eq!(body["status"], "unavailable");
    assert!(body["key_source"].is_null());
}

#[tokio::test]
async fn second_analyze_is_cache_hit_no_upstream_call() {
    let server = MockServer::start().await;
    // expect EXACTLY one upstream call across the two POSTs; verified on drop.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(canned_tool_use()))
        .expect(1)
        .mount(&server)
        .await;

    let tmp = make_tmp();
    let config = make_config(&tmp, server.uri(), server_core::KeySource::ApiKey("sk-ant-test".into()));
    let app = server_core::app(config).await.expect("app builds");

    let first = app.clone().oneshot(analyze_req()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let second = app.oneshot(analyze_req()).await.unwrap();
    assert_eq!(second.status(), StatusCode::OK);

    // Same input → second POST returns the cached run without re-inferring.
    let b1 = json_body(first).await;
    let b2 = json_body(second).await;
    assert_eq!(b1["run_id"], b2["run_id"]);
}
