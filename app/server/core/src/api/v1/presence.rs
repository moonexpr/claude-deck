// PORTED: presence_service.py + api/v1/presence.py

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path as AxumPath, State,
    },
    http::HeaderMap,
    response::Response,
    routing::{get, patch, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::{broadcast, Mutex};

use axum::response::IntoResponse;
use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};

const IDLE_TIMEOUT_MINUTES: i64 = 15;
const BUCKET_COUNT: usize = 30;
const EVENT_RETENTION_DAYS: i64 = 7;
const IDLE_CHECK_INTERVAL_SECONDS: i64 = 30;

const STATUS_ACTIVE: &str = "active";
const STATUS_IDLE: &str = "idle";
const STATUS_ERROR: &str = "error";
const STATUS_STOPPED: &str = "stopped";

const FILE_EDIT_TOOLS: [&str; 3] = ["Write", "Edit", "MultiEdit"];

/// Shared broadcast handle for live presence updates. Created in `router()`
/// and injected via an axum `Extension` layer — `ApiState` is left untouched.
#[derive(Clone)]
pub struct PresenceBroadcast {
    pub tx: broadcast::Sender<String>,
}

// Maintenance throttling — guarded by an async mutex to mirror Python's
// `_maintenance_lock`. Process-global like the Python module-level globals.
struct MaintenanceState {
    last_idle_check: Option<DateTime<Utc>>,
    last_prune: Option<DateTime<Utc>>,
}

fn maintenance() -> &'static Mutex<MaintenanceState> {
    static M: OnceLock<Mutex<MaintenanceState>> = OnceLock::new();
    M.get_or_init(|| {
        Mutex::new(MaintenanceState {
            last_idle_check: None,
            last_prune: None,
        })
    })
}

fn schema_init() -> &'static tokio::sync::OnceCell<()> {
    static C: OnceLock<tokio::sync::OnceCell<()>> = OnceLock::new();
    C.get_or_init(tokio::sync::OnceCell::new)
}

pub fn router() -> Router<ApiState> {
    let (tx, _rx) = broadcast::channel::<String>(256);
    let bc = PresenceBroadcast { tx };
    Router::new()
        .route("/events", post(receive_event))
        .route("/sessions", get(list_sessions).delete(clear_all_sessions))
        .route(
            "/sessions/{session_id}",
            patch(update_session_label).delete(remove_session),
        )
        .route("/config-snippet", get(get_config_snippet))
        .route("/ws", get(presence_websocket))
        .layer(Extension(Arc::new(bc)))
}

// ---- schema -----------------------------------------------------------------

/// Idempotently create the presence tables. The Rust app does not run
/// SQLAlchemy `create_all`, so each presence path ensures the schema on first
/// use. Columns mirror the SQLAlchemy models in `app/models/database.py`
/// (datetimes stored as TEXT ISO-8601, JSON columns as TEXT).
async fn create_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS presence_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            tool_name TEXT,
            tool_input TEXT,
            tool_result TEXT,
            message TEXT,
            cwd TEXT,
            timestamp TEXT NOT NULL,
            received_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS ix_presence_events_session_id ON presence_events (session_id)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS presence_sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL UNIQUE,
            label TEXT,
            project_path TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            status_text TEXT,
            last_narrative TEXT,
            last_narrative_at TEXT,
            modified_files TEXT,
            last_command TEXT,
            last_command_exit INTEGER,
            activity_buckets TEXT,
            bucket_start TEXT,
            total_events INTEGER NOT NULL DEFAULT 0,
            error_count INTEGER NOT NULL DEFAULT 0,
            started_at TEXT NOT NULL,
            last_event_at TEXT NOT NULL,
            ended_at TEXT,
            last_user_prompt TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS ix_presence_sessions_session_id ON presence_sessions (session_id)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS ix_presence_sessions_status ON presence_sessions (status)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS ix_presence_sessions_last_event_at ON presence_sessions (last_event_at)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn ensure_schema(pool: &SqlitePool) -> AppResult<()> {
    schema_init()
        .get_or_try_init(|| async {
            create_schema(pool)
                .await
                .map_err(|e| AppError::internal(e.to_string()))
        })
        .await?;
    Ok(())
}

// ---- in-memory session representation --------------------------------------

#[derive(Clone)]
struct SessionRow {
    session_id: String,
    label: Option<String>,
    project_path: Option<String>,
    status: String,
    status_text: Option<String>,
    last_narrative: Option<String>,
    last_narrative_at: Option<String>,
    modified_files: Value,
    last_command: Option<String>,
    last_command_exit: Option<i64>,
    activity_buckets: Value,
    bucket_start: Option<String>,
    total_events: i64,
    error_count: i64,
    started_at: String,
    last_event_at: String,
    ended_at: Option<String>,
    last_user_prompt: Option<String>,
}

fn iso(dt: DateTime<Utc>) -> String {
    // Mirrors Python datetime.isoformat() on a tz-aware UTC datetime.
    dt.format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()
}

fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .ok()
}

fn row_to_response(s: &SessionRow) -> Value {
    json!({
        "session_id": s.session_id,
        "label": s.label,
        "project_path": s.project_path,
        "status": s.status,
        "status_text": s.status_text,
        "last_narrative": s.last_narrative,
        "last_narrative_at": s.last_narrative_at,
        "modified_files": s.modified_files,
        "last_user_prompt": s.last_user_prompt,
        "last_command": s.last_command,
        "last_command_exit": s.last_command_exit,
        "activity_buckets": s.activity_buckets,
        "total_events": s.total_events,
        "error_count": s.error_count,
        "started_at": s.started_at,
        "last_event_at": s.last_event_at,
        "ended_at": s.ended_at,
    })
}

fn json_text(v: &Value) -> Option<String> {
    if v.is_null() {
        None
    } else {
        Some(v.to_string())
    }
}

fn parse_json_col(s: Option<String>) -> Value {
    match s {
        Some(t) => serde_json::from_str(&t).unwrap_or(Value::Null),
        None => Value::Null,
    }
}

const SELECT_COLS: &str = r#"session_id, label, project_path, status, status_text,
    last_narrative, last_narrative_at, modified_files, last_command, last_command_exit,
    activity_buckets, bucket_start, total_events, error_count,
    started_at, last_event_at, ended_at, last_user_prompt"#;

fn map_row(r: &SqliteRow) -> SessionRow {
    SessionRow {
        session_id: r.try_get("session_id").unwrap_or_default(),
        label: r.try_get("label").ok().flatten(),
        project_path: r.try_get("project_path").ok().flatten(),
        status: r.try_get("status").unwrap_or_else(|_| STATUS_ACTIVE.to_string()),
        status_text: r.try_get("status_text").ok().flatten(),
        last_narrative: r.try_get("last_narrative").ok().flatten(),
        last_narrative_at: r.try_get("last_narrative_at").ok().flatten(),
        modified_files: parse_json_col(r.try_get("modified_files").ok().flatten()),
        last_command: r.try_get("last_command").ok().flatten(),
        last_command_exit: r.try_get("last_command_exit").ok().flatten(),
        activity_buckets: parse_json_col(r.try_get("activity_buckets").ok().flatten()),
        bucket_start: r.try_get("bucket_start").ok().flatten(),
        total_events: r.try_get("total_events").unwrap_or(0),
        error_count: r.try_get("error_count").unwrap_or(0),
        started_at: r.try_get("started_at").unwrap_or_default(),
        last_event_at: r.try_get("last_event_at").unwrap_or_default(),
        ended_at: r.try_get("ended_at").ok().flatten(),
        last_user_prompt: r.try_get("last_user_prompt").ok().flatten(),
    }
}

async fn fetch_session(pool: &SqlitePool, session_id: &str) -> AppResult<Option<SessionRow>> {
    let row = sqlx::query(&format!(
        "SELECT {} FROM presence_sessions WHERE session_id = ?",
        SELECT_COLS
    ))
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;

    Ok(row.map(|r| map_row(&r)))
}

async fn upsert_session(pool: &SqlitePool, s: &SessionRow) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO presence_sessions (
            session_id, label, project_path, status, status_text,
            last_narrative, last_narrative_at, modified_files, last_command, last_command_exit,
            activity_buckets, bucket_start, total_events, error_count,
            started_at, last_event_at, ended_at, last_user_prompt
        ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
        ON CONFLICT(session_id) DO UPDATE SET
            label=excluded.label,
            project_path=excluded.project_path,
            status=excluded.status,
            status_text=excluded.status_text,
            last_narrative=excluded.last_narrative,
            last_narrative_at=excluded.last_narrative_at,
            modified_files=excluded.modified_files,
            last_command=excluded.last_command,
            last_command_exit=excluded.last_command_exit,
            activity_buckets=excluded.activity_buckets,
            bucket_start=excluded.bucket_start,
            total_events=excluded.total_events,
            error_count=excluded.error_count,
            started_at=excluded.started_at,
            last_event_at=excluded.last_event_at,
            ended_at=excluded.ended_at,
            last_user_prompt=excluded.last_user_prompt
        "#,
    )
    .bind(&s.session_id)
    .bind(&s.label)
    .bind(&s.project_path)
    .bind(&s.status)
    .bind(&s.status_text)
    .bind(&s.last_narrative)
    .bind(&s.last_narrative_at)
    .bind(json_text(&s.modified_files))
    .bind(&s.last_command)
    .bind(s.last_command_exit)
    .bind(json_text(&s.activity_buckets))
    .bind(&s.bucket_start)
    .bind(s.total_events)
    .bind(s.error_count)
    .bind(&s.started_at)
    .bind(&s.last_event_at)
    .bind(&s.ended_at)
    .bind(&s.last_user_prompt)
    .execute(pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

// ---- service logic ----------------------------------------------------------

fn derive_label(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
}

fn get_file_path(entry: &Value) -> String {
    match entry {
        Value::String(s) => s.clone(),
        Value::Object(_) => entry
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

fn extract_exit_code(tool_result: &Value) -> Option<i64> {
    if tool_result.is_null() || !tool_result.is_object() {
        return None;
    }
    if let Some(ec) = tool_result.get("exit_code") {
        return ec.as_i64();
    }
    if let Some(content) = tool_result.get("content").and_then(|v| v.as_str()) {
        if content.to_lowercase().contains("exit code") {
            if let Ok(re) = regex::Regex::new(r"(?i)exit code[:\s]+(\d+)") {
                if let Some(c) = re.captures(content) {
                    if let Some(m) = c.get(1) {
                        return m.as_str().parse::<i64>().ok();
                    }
                }
            }
        }
    }
    if tool_result
        .get("is_error")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Some(1);
    }
    Some(0)
}

fn update_activity_buckets(s: &mut SessionRow, now: DateTime<Utc>) {
    let mut buckets: Vec<i64> = s
        .activity_buckets
        .as_array()
        .map(|a| a.iter().map(|v| v.as_i64().unwrap_or(0)).collect())
        .unwrap_or_else(|| vec![0; BUCKET_COUNT]);
    if buckets.is_empty() {
        buckets = vec![0; BUCKET_COUNT];
    }

    let bucket_start = s.bucket_start.as_deref().and_then(parse_dt);
    let Some(bucket_start) = bucket_start else {
        s.bucket_start = Some(iso(now));
        let mut b = vec![0i64; BUCKET_COUNT - 1];
        b.push(1);
        s.activity_buckets = json!(b);
        return;
    };

    let mut offset = ((now - bucket_start).num_seconds() as f64 / 60.0) as i64;

    if offset >= BUCKET_COUNT as i64 {
        let shift = offset - BUCKET_COUNT as i64 + 1;
        let shift_u = shift.max(0) as usize;
        if shift_u >= buckets.len() {
            buckets = vec![0; BUCKET_COUNT];
        } else {
            buckets = buckets[shift_u..].to_vec();
            buckets.extend(std::iter::repeat(0).take(shift_u));
        }
        s.bucket_start = Some(iso(bucket_start + Duration::minutes(shift)));
        offset = BUCKET_COUNT as i64 - 1;
    }

    if offset >= 0 && (offset as usize) < buckets.len() {
        buckets[offset as usize] += 1;
    }

    s.activity_buckets = json!(buckets);
}

async fn assign_unique_label(
    pool: &SqlitePool,
    base_label: &str,
    session_id: &str,
) -> AppResult<String> {
    let existing = sqlx::query_as::<_, (String,)>(
        "SELECT session_id FROM presence_sessions WHERE label = ? AND session_id != ?",
    )
    .bind(base_label)
    .bind(session_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;

    if !existing.is_empty() {
        for (sid,) in &existing {
            let new_label = format!("{} ({})", base_label, &sid.chars().take(6).collect::<String>());
            sqlx::query("UPDATE presence_sessions SET label = ? WHERE session_id = ?")
                .bind(&new_label)
                .bind(sid)
                .execute(pool)
                .await
                .map_err(|e| AppError::internal(e.to_string()))?;
        }
        return Ok(format!(
            "{} ({})",
            base_label,
            &session_id.chars().take(6).collect::<String>()
        ));
    }
    Ok(base_label.to_string())
}

async fn mark_idle_sessions(pool: &SqlitePool, now: DateTime<Utc>) -> AppResult<()> {
    let cutoff = iso(now - Duration::minutes(IDLE_TIMEOUT_MINUTES));
    sqlx::query(
        "UPDATE presence_sessions SET status = ?, status_text = NULL
         WHERE status = ? AND last_event_at < ?",
    )
    .bind(STATUS_IDLE)
    .bind(STATUS_ACTIVE)
    .bind(&cutoff)
    .execute(pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

async fn prune_old_events(pool: &SqlitePool, now: DateTime<Utc>) -> AppResult<()> {
    let cutoff = iso(now - Duration::days(EVENT_RETENTION_DAYS));
    sqlx::query("DELETE FROM presence_events WHERE timestamp < ?")
        .bind(&cutoff)
        .execute(pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

async fn maybe_run_maintenance(pool: &SqlitePool, now: DateTime<Utc>) -> AppResult<()> {
    let mut run_idle = false;
    let mut run_prune = false;
    {
        let mut m = maintenance().lock().await;
        let go_idle = m
            .last_idle_check
            .map(|t| (now - t).num_seconds() >= IDLE_CHECK_INTERVAL_SECONDS)
            .unwrap_or(true);
        if go_idle {
            m.last_idle_check = Some(now);
            run_idle = true;
        }
        let go_prune = m
            .last_prune
            .map(|t| (now - t).num_seconds() >= 3600)
            .unwrap_or(true);
        if go_prune {
            m.last_prune = Some(now);
            run_prune = true;
        }
    }
    if run_idle {
        mark_idle_sessions(pool, now).await?;
    }
    if run_prune {
        prune_old_events(pool, now).await?;
    }
    Ok(())
}

async fn process_event(pool: &SqlitePool, payload: &Value) -> AppResult<Value> {
    let now = Utc::now();
    let now_s = iso(now);

    let session_id = payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::bad_request("session_id is required"))?
        .to_string();
    let event_type = payload
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    // Store raw event
    sqlx::query(
        r#"INSERT INTO presence_events
           (session_id, event_type, tool_name, tool_input, tool_result, message, cwd, timestamp, received_at)
           VALUES (?,?,?,?,?,?,?,?,?)"#,
    )
    .bind(&session_id)
    .bind(&event_type)
    .bind(payload.get("tool_name").and_then(|v| v.as_str()))
    .bind(payload.get("tool_input").filter(|v| !v.is_null()).map(|v| v.to_string()))
    .bind(payload.get("tool_result").filter(|v| !v.is_null()).map(|v| v.to_string()))
    .bind(payload.get("message").and_then(|v| v.as_str()))
    .bind(payload.get("cwd").and_then(|v| v.as_str()))
    .bind(&now_s)
    .bind(&now_s)
    .execute(pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;

    let mut session = match fetch_session(pool, &session_id).await? {
        Some(s) => s,
        None => SessionRow {
            session_id: session_id.clone(),
            label: None,
            project_path: None,
            status: STATUS_ACTIVE.to_string(),
            status_text: None,
            last_narrative: None,
            last_narrative_at: None,
            modified_files: json!([]),
            last_command: None,
            last_command_exit: None,
            activity_buckets: json!(vec![0i64; BUCKET_COUNT]),
            bucket_start: Some(now_s.clone()),
            total_events: 0,
            error_count: 0,
            started_at: now_s.clone(),
            last_event_at: now_s.clone(),
            ended_at: None,
            last_user_prompt: None,
        },
    };

    let cwd = payload.get("cwd").and_then(|v| v.as_str());
    if let Some(cwd) = cwd {
        if session.project_path.is_none() {
            session.project_path = Some(cwd.to_string());
        }
        if session.label.is_none() {
            let base = derive_label(cwd);
            session.label = Some(assign_unique_label(pool, &base, &session_id).await?);
        }
    }

    match event_type.as_str() {
        "Notification" => {
            if let Some(msg) = payload.get("message").and_then(|v| v.as_str()) {
                let lower = msg.to_lowercase();
                let waiting = ["waiting for your input", "waiting for input"];
                if !waiting.iter().any(|p| lower.contains(p)) {
                    session.last_narrative = Some(msg.to_string());
                    session.last_narrative_at = Some(now_s.clone());
                    let st: String = msg.replace('\n', " ").trim().chars().take(120).collect();
                    session.status_text = Some(st);
                }
            }
        }
        "PostToolUse" => {
            let tool_name = payload
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tool_input = payload
                .get("tool_input")
                .cloned()
                .filter(|v| v.is_object())
                .unwrap_or(json!({}));
            let tool_result = payload
                .get("tool_result")
                .cloned()
                .filter(|v| v.is_object())
                .unwrap_or(json!({}));

            if FILE_EDIT_TOOLS.contains(&tool_name) {
                let file_path = tool_input
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .or_else(|| tool_input.get("path").and_then(|v| v.as_str()));
                if let Some(file_path) = file_path {
                    let op = if tool_name == "Write" { "created" } else { "modified" };
                    let mut files: Vec<Value> = session
                        .modified_files
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    files.retain(|f| get_file_path(f) != file_path);
                    files.push(json!({ "path": file_path, "op": op }));
                    if files.len() > 10 {
                        files = files.split_off(files.len() - 10);
                    }
                    session.modified_files = json!(files);
                    let basename = file_path.rsplit('/').next().unwrap_or(file_path);
                    session.status_text = Some(format!("Edited {}", basename));
                } else {
                    session.status_text = Some(format!("Used tool: {}", tool_name));
                }
            } else if tool_name == "Bash" {
                let cmd = tool_input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                session.last_command = if cmd.is_empty() {
                    None
                } else {
                    Some(cmd.chars().take(500).collect())
                };
                let exit_code = extract_exit_code(&tool_result);
                session.last_command_exit = exit_code;
                if let Some(ec) = exit_code {
                    if ec != 0 {
                        session.error_count += 1;
                        session.status = STATUS_ERROR.to_string();
                    }
                }
                session.status_text = Some("Ran command".to_string());
            } else {
                session.status_text = Some(format!("Used tool: {}", tool_name));
            }
        }
        "PreToolUse" => {
            let tool_name = payload
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            session.status_text = Some(format!("Running tool: {}...", tool_name));
        }
        "UserPromptSubmit" => {
            let msg = payload
                .get("user_prompt")
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("message").and_then(|v| v.as_str()));
            if let Some(msg) = msg {
                session.last_user_prompt = Some(msg.trim().chars().take(500).collect());
            }
            session.status_text = Some("Processing user message...".to_string());
        }
        "SubagentStart" => {
            session.status_text = Some("Started subagent".to_string());
        }
        "SubagentStop" => {
            session.status_text = Some("Subagent completed".to_string());
        }
        "Stop" => {
            session.status = STATUS_STOPPED.to_string();
            session.status_text = Some("Waiting for input".to_string());
        }
        "SessionEnd" => {
            session.status = STATUS_STOPPED.to_string();
            session.ended_at = Some(now_s.clone());
            session.status_text = Some("Session ended".to_string());
        }
        "SessionStart" => {
            session.status = STATUS_ACTIVE.to_string();
            session.ended_at = None;
            session.started_at = now_s.clone();
            session.total_events = 0;
            session.error_count = 0;
            session.modified_files = json!([]);
            session.last_narrative = None;
            session.last_user_prompt = None;
            session.last_command = None;
            session.last_command_exit = None;
            session.activity_buckets = json!(vec![0i64; BUCKET_COUNT]);
            session.bucket_start = Some(now_s.clone());
            session.status_text = Some("Session started".to_string());
        }
        _ => {}
    }

    session.last_event_at = now_s.clone();
    session.total_events += 1;

    let passive = ["Stop", "SessionEnd", "Notification", "SubagentStop"];
    if !passive.contains(&event_type.as_str())
        && (session.status == STATUS_IDLE || session.status == STATUS_STOPPED)
    {
        session.status = STATUS_ACTIVE.to_string();
        session.ended_at = None;
    }

    update_activity_buckets(&mut session, now);

    upsert_session(pool, &session).await?;

    maybe_run_maintenance(pool, now).await?;

    Ok(row_to_response(&session))
}

async fn get_all_sessions(pool: &SqlitePool) -> AppResult<Vec<SessionRow>> {
    let now = Utc::now();
    mark_idle_sessions(pool, now).await?;

    let rows = sqlx::query(&format!(
        "SELECT {} FROM presence_sessions ORDER BY last_event_at DESC",
        SELECT_COLS
    ))
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;

    Ok(rows.iter().map(map_row).collect())
}

// ---- handlers ---------------------------------------------------------------

/// POST /api/v1/presence/events
async fn receive_event(
    State(state): State<ApiState>,
    Extension(bc): Extension<Arc<PresenceBroadcast>>,
    Json(payload): Json<Value>,
) -> AppResult<Json<Value>> {
    ensure_schema(&state.pool).await?;
    let updated = process_event(&state.pool, &payload).await?;
    let msg = json!({ "type": "session_update", "session": updated });
    let _ = bc.tx.send(msg.to_string());
    Ok(Json(json!({})))
}

/// GET /api/v1/presence/sessions
async fn list_sessions(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    ensure_schema(&state.pool).await?;
    let sessions = get_all_sessions(&state.pool).await?;
    let active = sessions.iter().filter(|s| s.status == STATUS_ACTIVE).count();
    let error = sessions.iter().filter(|s| s.status == STATUS_ERROR).count();
    let total = sessions.len();
    let list: Vec<Value> = sessions.iter().map(row_to_response).collect();
    Ok(Json(json!({
        "sessions": list,
        "total": total,
        "active": active,
        "error": error,
    })))
}

#[derive(Deserialize)]
struct LabelUpdate {
    label: String,
}

/// PATCH /api/v1/presence/sessions/{session_id}
async fn update_session_label(
    State(state): State<ApiState>,
    AxumPath(session_id): AxumPath<String>,
    Json(update): Json<LabelUpdate>,
) -> AppResult<Json<Value>> {
    ensure_schema(&state.pool).await?;
    let mut session = fetch_session(&state.pool, &session_id)
        .await?
        .ok_or_else(|| AppError::not_found("Session not found"))?;
    session.label = Some(update.label.clone());
    sqlx::query("UPDATE presence_sessions SET label = ? WHERE session_id = ?")
        .bind(&update.label)
        .bind(&session_id)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(Json(row_to_response(&session)))
}

/// DELETE /api/v1/presence/sessions/{session_id}
async fn remove_session(
    State(state): State<ApiState>,
    Extension(bc): Extension<Arc<PresenceBroadcast>>,
    AxumPath(session_id): AxumPath<String>,
) -> AppResult<Response> {
    ensure_schema(&state.pool).await?;
    let res = sqlx::query("DELETE FROM presence_sessions WHERE session_id = ?")
        .bind(&session_id)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("Session not found"));
    }
    let msg = json!({ "type": "session_remove", "session_id": session_id });
    let _ = bc.tx.send(msg.to_string());
    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}

/// DELETE /api/v1/presence/sessions
async fn clear_all_sessions(
    State(state): State<ApiState>,
    Extension(bc): Extension<Arc<PresenceBroadcast>>,
) -> AppResult<Response> {
    ensure_schema(&state.pool).await?;
    sqlx::query("DELETE FROM presence_sessions")
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    let msg = json!({ "type": "sessions_cleared" });
    let _ = bc.tx.send(msg.to_string());
    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}

/// GET /api/v1/presence/config-snippet
async fn get_config_snippet(headers: HeaderMap) -> AppResult<Json<Value>> {
    let override_url = std::env::var("PRESENCE_PUBLIC_URL")
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/')
        .to_string();

    let base = if !override_url.is_empty() {
        override_url
    } else {
        let host = headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("localhost:8000")
            .to_string();
        let proto = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http")
            .to_string();
        format!("{}://{}", proto, host)
    };
    let url = format!("{}/api/v1/presence/events", base);

    let events = [
        "Notification",
        "PreToolUse",
        "PostToolUse",
        "Stop",
        "SessionStart",
        "SessionEnd",
        "UserPromptSubmit",
        "SubagentStart",
        "SubagentStop",
    ];
    let mut hooks = serde_json::Map::new();
    for event in events {
        hooks.insert(
            event.to_string(),
            json!([{ "hooks": [{ "type": "http", "url": url }] }]),
        );
    }
    let snippet = json!({ "hooks": hooks });
    let instructions = "Add this to your ~/.claude/settings.json (or merge into existing hooks). \
Then restart any running Claude Code sessions for the hooks to take effect.";

    Ok(Json(json!({
        "snippet": snippet,
        "instructions": instructions,
    })))
}

/// GET /api/v1/presence/ws
async fn presence_websocket(
    ws: WebSocketUpgrade,
    State(state): State<ApiState>,
    Extension(bc): Extension<Arc<PresenceBroadcast>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, bc))
}

async fn handle_socket(mut socket: WebSocket, state: ApiState, bc: Arc<PresenceBroadcast>) {
    let mut rx = bc.tx.subscribe();

    if ensure_schema(&state.pool).await.is_err() {
        return;
    }

    // Send initial state — all current sessions as session_update messages.
    if let Ok(sessions) = get_all_sessions(&state.pool).await {
        for s in &sessions {
            let msg = json!({ "type": "session_update", "session": row_to_response(s) });
            if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                return;
            }
        }
    }

    loop {
        tokio::select! {
            broadcast_msg = rx.recv() => {
                match broadcast_msg {
                    Ok(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(_)) => continue,
                    _ => break,
                }
            }
        }
    }
}
