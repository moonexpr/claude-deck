//! Insight Platform M1 integration tests (Plan 0004a validator #1), headless path.
//!
//! Drives `insight_service::analyze_session` against a **stub `claude` binary**
//! (a script printing a canned `--output-format json` envelope, passed as
//! `claude_bin` so there's no global-env race). Asserts the claim-level
//! two-check provenance gate: a grounded item persists; a citation error and a
//! groundedness error are dropped and counted, not stored.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use server_core::services::insight_service::analyze_session;
use server_core::services::session_service::SessionService;

/// Fixture: one user turn + one assistant reply whose text contains the
/// grounded quote. Conversation locator = the user timestamp.
const FIXTURE: &str = concat!(
    r#"{"type":"user","timestamp":"2026-06-04T10:00:00Z","message":{"role":"user","content":"Should we merge PR nine?"}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-06-04T10:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"I merged PR nine after independent validation passed; build and tests were green."}]}}"#,
    "\n",
);

fn make_tmp() -> PathBuf {
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
    std::fs::create_dir_all(tmp.join("projects").join("proj")).unwrap();
    std::fs::write(tmp.join("projects").join("proj").join("sess.jsonl"), FIXTURE).unwrap();
    tmp
}

/// Build a stub `claude` that ignores its args, records each invocation, and
/// prints `envelope_json`. Returns the script path (use as `claude_bin`).
fn write_stub(tmp: &Path, envelope_json: &str) -> PathBuf {
    let env_path = tmp.join("envelope.json");
    std::fs::write(&env_path, envelope_json).unwrap();
    let calls = tmp.join("calls.txt");
    let script = tmp.join("claude-stub.sh");
    let body = format!(
        "#!/bin/sh\necho run >> \"{}\"\ncat \"{}\"\n",
        calls.display(),
        env_path.display()
    );
    std::fs::write(&script, body).unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn call_count(tmp: &Path) -> usize {
    std::fs::read_to_string(tmp.join("calls.txt"))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// A canned `claude --output-format json` envelope whose `result` carries an
/// insight mixing a grounded judgment call, a citation error, and a
/// groundedness error, plus an ungated summary + follow-up.
fn success_envelope() -> String {
    let insight = serde_json::json!({
        "summary": "Worked the open PR queue to resolution.",
        "decisions": [
            { "text": "cites a turn that does not exist",
              "source_ref": "NO-SUCH-TIMESTAMP", "quote": "irrelevant" }
        ],
        "judgment_calls": [
            { "summary": "merge PR nine", "options": ["merge", "hold"],
              "chosen": "merge", "rationale": "fully validated, closes a tracked bug",
              "source_ref": "2026-06-04T10:00:00Z",
              "quote": "merged PR nine after independent validation" }
        ],
        "errors_hit": [
            { "text": "this quote is not in the transcript",
              "source_ref": "2026-06-04T10:00:00Z",
              "quote": "a phrase that appears nowhere in the session" }
        ],
        "follow_ups": ["write the docs"]
    });
    serde_json::json!({
        "type": "result", "subtype": "success", "is_error": false,
        "result": insight.to_string(),
        "usage": { "input_tokens": 512, "output_tokens": 88 }
    })
    .to_string()
}

async fn migrate_and_pool(tmp: &Path) -> sqlx::SqlitePool {
    let db_url = format!("sqlite:{}?mode=rwc", tmp.join("test.db").display());
    let config = server_core::ServerConfig {
        db_url: db_url.clone(),
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        key_source: server_core::KeySource::None,
        anthropic_base_url: "http://unused".into(),
        enable_external_tools: false,
        cwd_fallback: tmp.to_path_buf(),
    };
    server_core::app(config).await.expect("app builds + migrates");
    sqlx::SqlitePool::connect(&db_url).await.expect("pool")
}

#[tokio::test]
async fn gate_keeps_grounded_and_drops_citation_and_groundedness_errors() {
    let tmp = make_tmp();
    let pool = migrate_and_pool(&tmp).await;
    let ss = SessionService::new(tmp.join("projects"));
    let stub = write_stub(&tmp, &success_envelope());

    let insight = analyze_session(&pool, &ss, stub.to_str().unwrap(), None, "proj", "sess")
        .await
        .expect("analyze succeeds");

    assert_eq!(insight.status, "done");

    // The grounded judgment call survives with provenance intact.
    assert_eq!(insight.judgment_calls.len(), 1);
    let jc = &insight.judgment_calls[0];
    assert_eq!(jc.summary, "merge PR nine");
    assert_eq!(jc.source_locator.as_deref(), Some("2026-06-04T10:00:00Z"));
    assert_eq!(jc.quote.as_deref(), Some("merged PR nine after independent validation"));
    assert_eq!(jc.options, vec!["merge", "hold"]);

    // Both bad items dropped and counted separately.
    assert_eq!(insight.dropped.citation_error, 1);
    assert_eq!(insight.dropped.groundedness_error, 1);

    // insights = ungated summary + follow-up only; no grounded Decision/Error.
    let kinds: Vec<&str> = insight.insights.iter().map(|i| i.kind.as_str()).collect();
    assert!(kinds.contains(&"Summary"));
    assert!(kinds.contains(&"Follow-up"));
    assert!(!kinds.contains(&"Decision"), "citation-error decision must be dropped");
    assert!(!kinds.contains(&"Error"), "groundedness-error must be dropped");
}

#[tokio::test]
async fn second_analyze_is_cache_hit_no_second_spawn() {
    let tmp = make_tmp();
    let pool = migrate_and_pool(&tmp).await;
    let ss = SessionService::new(tmp.join("projects"));
    let stub = write_stub(&tmp, &success_envelope());
    let bin = stub.to_str().unwrap();

    let first = analyze_session(&pool, &ss, bin, None, "proj", "sess").await.unwrap();
    let second = analyze_session(&pool, &ss, bin, None, "proj", "sess").await.unwrap();

    assert_eq!(first.run_id, second.run_id, "same input → cached run");
    assert_eq!(call_count(&tmp), 1, "second analyze must NOT spawn claude again");
}

#[tokio::test]
async fn claude_is_error_surfaces_as_err() {
    let tmp = make_tmp();
    let pool = migrate_and_pool(&tmp).await;
    let ss = SessionService::new(tmp.join("projects"));
    // Envelope reporting the real-world failure when ANTHROPIC_API_KEY routed
    // claude to depleted API credits.
    let envelope = serde_json::json!({
        "type": "result", "is_error": true, "result": "Credit balance is too low"
    })
    .to_string();
    let stub = write_stub(&tmp, &envelope);

    let res = analyze_session(&pool, &ss, stub.to_str().unwrap(), None, "proj", "sess").await;
    let err = res.expect_err("is_error envelope must fail").to_string();
    assert!(err.contains("Credit balance"), "got: {err}");
}
