//! M0 — Insight Platform artifact schema (Plan 0004a).
//!
//! Verifies migration `0002_insight_platform.sql` applies during `app()` boot
//! and that the core tables round-trip an insert with provenance joins intact.

use sqlx::Row;

/// Unique tempdir per test run (mirrors tests/chat_crud.rs).
fn make_tmp() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "claude_deck_insight_schema_{}_{}_{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos(),
    ));
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    tmp
}

fn make_config(tmp: &std::path::Path, db_url: &str) -> server_core::ServerConfig {
    server_core::ServerConfig {
        db_url: db_url.to_string(),
        projects_dir: tmp.join("projects"),
        frontend_dist_path: None,
        presence_public_url: None,
        key_source: server_core::KeySource::None,
        anthropic_base_url: "https://api.anthropic.com".to_string(),
        enable_external_tools: false,
        cwd_fallback: tmp.to_path_buf(),
    }
}

#[tokio::test]
async fn migration_applies_and_artifacts_round_trip() {
    let tmp = make_tmp();
    let db_url = format!("sqlite:{}?mode=rwc", tmp.join("test.db").display());

    // app() runs embedded migrations, including 0002.
    let _app = server_core::app(make_config(&tmp, &db_url))
        .await
        .expect("app builds and migrates");

    let pool = sqlx::SqlitePool::connect(&db_url)
        .await
        .expect("open pool to migrated db");

    // A run + a source + a grounded judgment call referencing it.
    let run_id = sqlx::query(
        "INSERT INTO analysis_runs (kind, target_ref, model, input_hash, status, output_tokens) \
         VALUES ('session_insight', 'proj/sess', 'claude', 'hash-1', 'done', 42)",
    )
    .execute(&pool)
    .await
    .expect("insert run")
    .last_insert_rowid();

    let source_id = sqlx::query(
        "INSERT INTO sources (type, ref, locator) VALUES ('session', 'proj/sess', 'entry-uuid-1')",
    )
    .execute(&pool)
    .await
    .expect("insert source")
    .last_insert_rowid();

    sqlx::query(
        "INSERT INTO judgment_calls (run_id, source_id, quote, summary, chosen, rationale) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(run_id)
    .bind(source_id)
    .bind("merged #9 autonomously")
    .bind("merge the validated bug-fix PR")
    .bind("merge")
    .bind("bounded, fully validated, closes a tracked bug")
    .execute(&pool)
    .await
    .expect("insert judgment call");

    // Round-trip via the provenance join.
    let row = sqlx::query(
        "SELECT jc.summary, jc.quote, jc.status, s.locator, s.type, ar.output_tokens \
         FROM judgment_calls jc \
         JOIN sources s ON s.id = jc.source_id \
         JOIN analysis_runs ar ON ar.id = jc.run_id \
         WHERE jc.run_id = ?",
    )
    .bind(run_id)
    .fetch_one(&pool)
    .await
    .expect("select joined row");

    let summary: String = row.get(0);
    let quote: String = row.get(1);
    let status: String = row.get(2);
    let locator: String = row.get(3);
    let stype: String = row.get(4);
    let out_tokens: i64 = row.get(5);

    assert_eq!(summary, "merge the validated bug-fix PR");
    assert_eq!(quote, "merged #9 autonomously");
    assert_eq!(status, "open"); // default lifecycle state
    assert_eq!(locator, "entry-uuid-1");
    assert_eq!(stype, "session");
    assert_eq!(out_tokens, 42);

    // Default drop-counters exist and start at zero.
    let (cite, ground): (i64, i64) =
        sqlx::query_as("SELECT citation_error, groundedness_error FROM analysis_runs WHERE id = ?")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .expect("select counters");
    assert_eq!(cite, 0);
    assert_eq!(ground, 0);
}
