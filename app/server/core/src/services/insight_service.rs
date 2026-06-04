//! Insight Platform M1 — per-session analysis (Plan 0004a).
//!
//! `analyze_session` loads a session transcript, derives a structured insight
//! via forced tool-use, and persists each grounded artifact behind the
//! **two-check provenance gate**: an item is kept only if its `source_ref`
//! resolves to a real transcript entry (citation) AND its `quote` is found in
//! that entry's text (groundedness). Failures are dropped and counted, never
//! stored. Schema-validity (from forced tool-use) is necessary but not
//! sufficient — groundedness is enforced here, post-decode.
//!
//! Decision A (derived store): every row is a rebuildable projection of the
//! session file; nothing is written back to disk.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use anyhow::{Context, Result};
use serde::Serialize;
use sqlx::{Row, SqlitePool};

use crate::services::headless_claude;
use crate::services::session_service::SessionService;

const ANALYZER_VERSION: &str = "m2.0-headless";

// ---------------------------------------------------------------------------
// Response shape (also the cache-reload shape)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct InsightArtifact {
    pub id: i64,
    /// Discriminator carried in `title`: "Summary" | "Decision" | "Error" | "Follow-up".
    pub kind: String,
    pub body: String,
    pub severity: String,
    pub source_locator: Option<String>,
    pub quote: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct JudgmentCallOut {
    pub id: i64,
    pub summary: String,
    pub options: Vec<String>,
    pub chosen: Option<String>,
    pub rationale: Option<String>,
    pub source_locator: Option<String>,
    pub quote: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct DropCounts {
    pub citation_error: i64,
    pub groundedness_error: i64,
}

#[derive(Debug, Serialize)]
pub struct SessionInsight {
    pub run_id: i64,
    pub status: String,
    pub insights: Vec<InsightArtifact>,
    pub judgment_calls: Vec<JudgmentCallOut>,
    pub dropped: DropCounts,
}

// ---------------------------------------------------------------------------
// Transcript → grounding units
// ---------------------------------------------------------------------------

/// One grounding unit (a conversation): a stable `locator` and the flattened
/// text the model may quote from.
struct Entry {
    locator: String,
    text: String,
}

/// Load all pages of a session and flatten conversations into grounding units
/// plus a prompt rendering. Locator = the conversation timestamp (the same key
/// the transcript viewer scrolls by).
async fn load_entries(
    session_service: &SessionService,
    project: &str,
    session: &str,
) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let first = session_service
        .get_session_detail(project, session, 1)
        .await
        .context("load session page 1")?;
    let total_pages = first.total_pages.max(1);

    for page in 1..=total_pages {
        let detail = if page == 1 {
            first.clone()
        } else {
            session_service
                .get_session_detail(project, session, page)
                .await
                .with_context(|| format!("load session page {page}"))?
        };
        for convo in detail.session.conversations {
            let mut text = String::new();
            text.push_str(&convo.user_text);
            for msg in &convo.messages {
                for block in &msg.content {
                    if let Some(t) = &block.text {
                        text.push('\n');
                        text.push_str(t);
                    }
                    if let Some(th) = &block.thinking {
                        text.push('\n');
                        text.push_str(th);
                    }
                }
            }
            entries.push(Entry { locator: convo.timestamp.clone(), text });
        }
    }
    Ok(entries)
}

/// Render the transcript for the prompt, tagging each entry with its locator so
/// the model cites the same string back.
fn render_prompt(entries: &[Entry]) -> String {
    let mut out = String::new();
    for e in entries {
        out.push_str(&format!("[entry {}]\n{}\n\n", e.locator, e.text.trim()));
    }
    out
}

/// Deterministic cache key. `DefaultHasher::new()` uses fixed keys, so this is
/// stable across runs (unlike `RandomState`).
fn input_hash(model: &str, entries: &[Entry]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ANALYZER_VERSION.hash(&mut h);
    model.hash(&mut h);
    for e in entries {
        e.locator.hash(&mut h);
        e.text.hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(PartialEq)]
enum Gate {
    Ok,
    Citation,
    Groundedness,
}

/// The two-check provenance gate.
fn gate(locator: &str, quote: &str, entries: &HashMap<String, String>) -> Gate {
    match entries.get(locator) {
        None => Gate::Citation,
        Some(entry_text) => {
            if normalize_ws(entry_text).contains(&normalize_ws(quote)) {
                Gate::Ok
            } else {
                Gate::Groundedness
            }
        }
    }
}

const SYSTEM_PROMPT: &str = "You analyze a Claude Code session transcript and emit one structured insight as JSON. \
Respond with ONLY a JSON object (no prose, no markdown fence) of EXACTLY this shape: \
{\"summary\": string, \"decisions\": [{\"text\": string, \"source_ref\": string, \"quote\": string}], \
\"judgment_calls\": [{\"summary\": string, \"options\": [string], \"chosen\": string, \"rationale\": string, \"source_ref\": string, \"quote\": string}], \
\"errors_hit\": [{\"text\": string, \"source_ref\": string, \"quote\": string}], \"follow_ups\": [string]}. \
Rules: (1) For every decision, judgment_call, and error, set source_ref to the [entry <locator>] tag you drew it from, and set quote to an EXACT verbatim span copied from that entry's text. \
(2) Emit nothing you cannot quote. (3) summary and follow_ups are session-level and do not cite. \
A judgment_call is a point where a choice was made between alternatives.";

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Analyze a session: return the cached insight if the input is unchanged, else
/// run inference, gate, persist, and return.
#[allow(clippy::too_many_arguments)]
pub async fn analyze_session(
    pool: &SqlitePool,
    session_service: &SessionService,
    claude_bin: &str,
    model: Option<String>,
    project: &str,
    session: &str,
) -> Result<SessionInsight> {
    let model_str = model.unwrap_or_else(|| "claude-sonnet-4-5".to_string());
    let target_ref = format!("{project}/{session}");

    let entries = load_entries(session_service, project, session).await?;
    let hash = input_hash(&model_str, &entries);

    // Cache hit?
    if let Some(row) =
        sqlx::query("SELECT id, status FROM analysis_runs WHERE kind = 'session_insight' AND input_hash = ?")
            .bind(&hash)
            .fetch_optional(pool)
            .await?
    {
        let run_id: i64 = row.get(0);
        let status: String = row.get(1);
        if status == "done" {
            return load_session_insight(pool, run_id).await;
        }
    }

    // New run.
    let run_id = sqlx::query(
        "INSERT INTO analysis_runs (kind, target_ref, model, input_hash, status) \
         VALUES ('session_insight', ?, ?, ?, 'running')",
    )
    .bind(&target_ref)
    .bind(&model_str)
    .bind(&hash)
    .execute(pool)
    .await?
    .last_insert_rowid();

    match run_inference_and_persist(pool, claude_bin, &model_str, &target_ref, run_id, &entries)
        .await
    {
        Ok(()) => load_session_insight(pool, run_id).await,
        Err(e) => {
            let _ = sqlx::query("UPDATE analysis_runs SET status = 'error', error = ?, finished_at = datetime('now') WHERE id = ?")
                .bind(e.to_string())
                .bind(run_id)
                .execute(pool)
                .await;
            Err(e)
        }
    }
}

async fn run_inference_and_persist(
    pool: &SqlitePool,
    claude_bin: &str,
    model_str: &str,
    target_ref: &str,
    run_id: i64,
    entries: &[Entry],
) -> Result<()> {
    let lookup: HashMap<String, String> =
        entries.iter().map(|e| (e.locator.clone(), e.text.clone())).collect();

    let (input, usage) = headless_claude::run_structured(
        claude_bin,
        Some(model_str),
        SYSTEM_PROMPT,
        &render_prompt(entries),
    )
    .await?;

    let mut citation_error = 0i64;
    let mut groundedness_error = 0i64;

    let mut tx = pool.begin().await?;

    // Session-level (ungrounded by design): summary + follow_ups.
    if let Some(summary) = input.get("summary").and_then(|v| v.as_str()) {
        insert_insight(&mut tx, run_id, None, None, "Summary", summary, "info").await?;
    }
    if let Some(arr) = input.get("follow_ups").and_then(|v| v.as_array()) {
        for f in arr.iter().filter_map(|v| v.as_str()) {
            insert_insight(&mut tx, run_id, None, None, "Follow-up", f, "info").await?;
        }
    }

    // Grounded claims: decisions + errors → insights; judgment_calls → judgment_calls.
    for (key, kind, severity) in [("decisions", "Decision", "info"), ("errors_hit", "Error", "risk")] {
        if let Some(arr) = input.get(key).and_then(|v| v.as_array()) {
            for item in arr {
                let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let loc = item.get("source_ref").and_then(|v| v.as_str()).unwrap_or("");
                let quote = item.get("quote").and_then(|v| v.as_str()).unwrap_or("");
                match gate(loc, quote, &lookup) {
                    Gate::Citation => citation_error += 1,
                    Gate::Groundedness => groundedness_error += 1,
                    Gate::Ok => {
                        let source_id = insert_source(&mut tx, target_ref, loc).await?;
                        insert_insight(&mut tx, run_id, Some(source_id), Some(quote), kind, text, severity).await?;
                    }
                }
            }
        }
    }

    if let Some(arr) = input.get("judgment_calls").and_then(|v| v.as_array()) {
        for item in arr {
            let summary = item.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            let loc = item.get("source_ref").and_then(|v| v.as_str()).unwrap_or("");
            let quote = item.get("quote").and_then(|v| v.as_str()).unwrap_or("");
            match gate(loc, quote, &lookup) {
                Gate::Citation => citation_error += 1,
                Gate::Groundedness => groundedness_error += 1,
                Gate::Ok => {
                    let source_id = insert_source(&mut tx, target_ref, loc).await?;
                    let options_json = item
                        .get("options")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "[]".to_string());
                    let chosen = item.get("chosen").and_then(|v| v.as_str());
                    let rationale = item.get("rationale").and_then(|v| v.as_str());
                    sqlx::query(
                        "INSERT INTO judgment_calls (run_id, source_id, quote, summary, options_json, chosen, rationale) \
                         VALUES (?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(run_id)
                    .bind(source_id)
                    .bind(quote)
                    .bind(summary)
                    .bind(options_json)
                    .bind(chosen)
                    .bind(rationale)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
    }

    sqlx::query(
        "UPDATE analysis_runs SET status = 'done', input_tokens = ?, output_tokens = ?, \
         citation_error = ?, groundedness_error = ?, finished_at = datetime('now') WHERE id = ?",
    )
    .bind(usage.input_tokens as i64)
    .bind(usage.output_tokens as i64)
    .bind(citation_error)
    .bind(groundedness_error)
    .bind(run_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

async fn insert_source(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    target_ref: &str,
    locator: &str,
) -> Result<i64> {
    let id = sqlx::query("INSERT INTO sources (type, ref, locator) VALUES ('session', ?, ?)")
        .bind(target_ref)
        .bind(locator)
        .execute(&mut **tx)
        .await?
        .last_insert_rowid();
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
async fn insert_insight(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: i64,
    source_id: Option<i64>,
    quote: Option<&str>,
    title: &str,
    body: &str,
    severity: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO insights (run_id, source_id, quote, title, body, severity) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(run_id)
    .bind(source_id)
    .bind(quote)
    .bind(title)
    .bind(body)
    .bind(severity)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Rebuild the `SessionInsight` response from persisted rows.
pub async fn load_session_insight(pool: &SqlitePool, run_id: i64) -> Result<SessionInsight> {
    let (status, citation_error, groundedness_error): (String, i64, i64) = sqlx::query_as(
        "SELECT status, citation_error, groundedness_error FROM analysis_runs WHERE id = ?",
    )
    .bind(run_id)
    .fetch_one(pool)
    .await?;

    let insight_rows = sqlx::query(
        "SELECT i.id, i.title, i.body, i.severity, i.quote, i.status, s.locator \
         FROM insights i LEFT JOIN sources s ON s.id = i.source_id \
         WHERE i.run_id = ? ORDER BY i.id",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;

    let insights = insight_rows
        .into_iter()
        .map(|r| InsightArtifact {
            id: r.get(0),
            kind: r.get(1),
            body: r.get(2),
            severity: r.get(3),
            quote: r.get(4),
            status: r.get(5),
            source_locator: r.get(6),
        })
        .collect();

    let jc_rows = sqlx::query(
        "SELECT jc.id, jc.summary, jc.options_json, jc.chosen, jc.rationale, jc.quote, jc.status, s.locator \
         FROM judgment_calls jc LEFT JOIN sources s ON s.id = jc.source_id \
         WHERE jc.run_id = ? ORDER BY jc.id",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;

    let judgment_calls = jc_rows
        .into_iter()
        .map(|r| {
            let options_json: Option<String> = r.get(2);
            let options: Vec<String> = options_json
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            JudgmentCallOut {
                id: r.get(0),
                summary: r.get(1),
                options,
                chosen: r.get(3),
                rationale: r.get(4),
                quote: r.get(5),
                status: r.get(6),
                source_locator: r.get(7),
            }
        })
        .collect();

    Ok(SessionInsight {
        run_id,
        status,
        insights,
        judgment_calls,
        dropped: DropCounts { citation_error, groundedness_error },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("ts-1".to_string(), "We merged PR nine after validation.".to_string());
        m
    }

    #[test]
    fn gate_ok_when_locator_resolves_and_quote_present() {
        assert!(gate("ts-1", "merged PR nine", &entries()) == Gate::Ok);
    }

    #[test]
    fn gate_citation_error_when_locator_missing() {
        assert!(gate("ts-404", "anything", &entries()) == Gate::Citation);
    }

    #[test]
    fn gate_groundedness_error_when_quote_absent() {
        assert!(gate("ts-1", "deleted the database", &entries()) == Gate::Groundedness);
    }

    #[test]
    fn gate_tolerates_whitespace_differences() {
        assert!(gate("ts-1", "merged   PR\n nine", &entries()) == Gate::Ok);
    }
}
