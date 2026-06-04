//! Insight Platform routes (Plan 0004a M1).
//!
//! GET  /session/{project}/{session}          → latest persisted insight, or 404
//! POST /session/{project}/{session}/analyze  → run (or return cached) analysis
//! PATCH /insight/{id}                         → set an insight's status
//! PATCH /judgment-call/{id}                   → set a judgment call's status
//!
//! Credential source: `ApiState.key_provider` only (mirrors ai.rs). A missing
//! credential yields a 503 with `key_source`, matching the chat banner contract.
//!
//! NOTE (M1 scope): unlike the AI proxy, these routes don't yet enforce the
//! same-origin check. They are localhost dev-tool endpoints; same-origin
//! hardening on `analyze` (which spends the credential) is a follow-up.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use serde::Deserialize;

use crate::api::v1::ApiState;
use crate::services::insight_service;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/session/{project}/{session}", get(get_cached))
        .route("/session/{project}/{session}/analyze", post(analyze))
        .route("/insight/{id}", patch(patch_insight))
        .route("/judgment-call/{id}", patch(patch_judgment_call))
}

async fn get_cached(
    State(state): State<ApiState>,
    Path((project, session)): Path<(String, String)>,
) -> Response {
    let target_ref = format!("{project}/{session}");
    let latest = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM analysis_runs \
         WHERE kind = 'session_insight' AND target_ref = ? AND status = 'done' \
         ORDER BY id DESC LIMIT 1",
    )
    .bind(&target_ref)
    .fetch_optional(&state.pool)
    .await;

    match latest {
        Ok(Some(run_id)) => match insight_service::load_session_insight(&state.pool, run_id).await {
            Ok(insight) => Json(insight).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        },
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn analyze(
    State(state): State<ApiState>,
    Path((project, session)): Path<(String, String)>,
) -> Response {
    // Inference runs through headless Claude Code (subscription auth, no API
    // key). Override the binary via CLAUDE_BIN; default `claude` on PATH.
    let claude_bin = std::env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());

    match insight_service::analyze_session(
        &state.pool,
        &state.session_service,
        &claude_bin,
        None,
        &project,
        &session,
    )
    .await
    {
        Ok(insight) => Json(insight).into_response(),
        // Surface the headless-claude failure detail so the card can show it
        // (e.g. "claude error: Credit balance is too low", or not-on-PATH).
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "status": "error", "detail": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct StatusPatch {
    status: String,
}

async fn patch_insight(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(body): Json<StatusPatch>,
) -> Response {
    match sqlx::query("UPDATE insights SET status = ? WHERE id = ?")
        .bind(&body.status)
        .bind(id)
        .execute(&state.pool)
        .await
    {
        Ok(r) if r.rows_affected() > 0 => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn patch_judgment_call(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(body): Json<StatusPatch>,
) -> Response {
    // `open` clears the resolution timestamp; any terminal state stamps it.
    let resolved = if body.status == "open" {
        "resolved_at = NULL"
    } else {
        "resolved_at = datetime('now')"
    };
    let sql = format!("UPDATE judgment_calls SET status = ?, {resolved} WHERE id = ?");
    match sqlx::query(&sql)
        .bind(&body.status)
        .bind(id)
        .execute(&state.pool)
        .await
    {
        Ok(r) if r.rows_affected() > 0 => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
