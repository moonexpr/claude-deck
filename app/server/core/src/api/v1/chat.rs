/// Chat conversations CRUD — Phase C, task C3.
///
/// GET    /api/v1/chat/        — list conversations (message_count, no blob)
/// POST   /api/v1/chat/        — create conversation → 201
/// GET    /api/v1/chat/{id}    — fetch single conversation
/// PUT    /api/v1/chat/{id}    — partial update (title and/or messages)
/// DELETE /api/v1/chat/{id}    — remove → 204
///
/// Security: same-origin parity with /api/v1/ai (absent Origin allowed).
/// Messages column: JSON array of { "role": "...", "content": "..." }.
/// Streaming chat is NOT here — it lives at /api/v1/ai/chat (C1).
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::api::v1::ApiState;
use crate::cc_bridge::is_same_origin;
use crate::services::ai::Message;

// ---------------------------------------------------------------------------
// Same-origin helper (same policy as ai.rs)
// ---------------------------------------------------------------------------

fn same_origin_ok(headers: &HeaderMap) -> bool {
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    origin.is_empty() || is_same_origin(origin, headers)
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Body for POST /api/v1/chat/
#[derive(Deserialize)]
struct CreateRequest {
    title: String,
    #[serde(default)]
    messages: Vec<Message>,
}

/// Body for PUT /api/v1/chat/{id} — all fields optional for partial update.
#[derive(Deserialize)]
struct UpdateRequest {
    title: Option<String>,
    messages: Option<Vec<Message>>,
}

/// Full conversation record (GET single / POST / PUT response).
#[derive(Serialize)]
struct ConversationResponse {
    id: String,
    title: String,
    messages: Vec<Message>,
    created_at: i64,
    updated_at: i64,
}

/// Lightweight list item — `message_count` instead of the full messages blob.
#[derive(Serialize)]
struct ConversationSummary {
    id: String,
    title: String,
    message_count: i64,
    created_at: i64,
    updated_at: i64,
}

#[derive(Serialize)]
struct ListResponse {
    conversations: Vec<ConversationSummary>,
}

/// Generic error body matching the C1 / cc-bridge style.
#[derive(Serialize)]
struct ErrorBody {
    status: &'static str,
    detail: String,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn validate_title(title: &str) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    if title.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                status: "bad_request",
                detail: "title must not be empty".to_string(),
            }),
        ));
    }
    Ok(())
}

/// Validate that `messages` slice is well-formed and serialize it to JSON.
fn validate_and_serialize_messages(
    messages: &[Message],
) -> Result<String, (StatusCode, Json<ErrorBody>)> {
    for (i, msg) in messages.iter().enumerate() {
        if msg.role.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    status: "bad_request",
                    detail: format!("messages[{}].role must not be empty", i),
                }),
            ));
        }
        if msg.content.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    status: "bad_request",
                    detail: format!("messages[{}].content must not be empty", i),
                }),
            ));
        }
    }
    serde_json::to_string(messages).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                status: "internal_error",
                detail: "failed to serialize messages".to_string(),
            }),
        )
    })
}

/// Deserialize the messages JSON blob from the DB.  Corruption → 500.
fn deserialize_messages(raw: &str) -> Result<Vec<Message>, (StatusCode, Json<ErrorBody>)> {
    serde_json::from_str::<Vec<Message>>(raw).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                status: "internal_error",
                detail: "corrupt messages column in database".to_string(),
            }),
        )
    })
}

fn forbidden() -> impl IntoResponse {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorBody {
            status: "forbidden",
            detail: "same-origin check failed".to_string(),
        }),
    )
}

fn not_found(id: &str) -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            status: "not_found",
            detail: format!("conversation {} does not exist", id),
        }),
    )
}

fn db_error(e: sqlx::Error) -> impl IntoResponse {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody {
            status: "internal_error",
            detail: e.to_string(),
        }),
    )
}

// ---------------------------------------------------------------------------
// GET /api/v1/chat/
// ---------------------------------------------------------------------------

async fn list_conversations(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !same_origin_ok(&headers) {
        return forbidden().into_response();
    }

    let rows = sqlx::query(
        r#"
        SELECT
            id,
            title,
            json_array_length(messages) AS message_count,
            created_at,
            updated_at
        FROM chat_conversations
        ORDER BY updated_at DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await;

    match rows {
        Err(e) => db_error(e).into_response(),
        Ok(rows) => {
            use sqlx::Row as _;
            let conversations: Vec<ConversationSummary> = rows
                .iter()
                .map(|r| ConversationSummary {
                    id: r.get("id"),
                    title: r.get("title"),
                    message_count: r.get("message_count"),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                })
                .collect();
            Json(ListResponse { conversations }).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/v1/chat/
// ---------------------------------------------------------------------------

async fn create_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<CreateRequest>,
) -> impl IntoResponse {
    if !same_origin_ok(&headers) {
        return forbidden().into_response();
    }

    if let Err(e) = validate_title(&req.title) {
        return e.into_response();
    }

    let messages_json = match validate_and_serialize_messages(&req.messages) {
        Ok(j) => j,
        Err(e) => return e.into_response(),
    };

    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    let result = sqlx::query(
        "INSERT INTO chat_conversations (id, title, messages, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&req.title)
    .bind(&messages_json)
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await;

    match result {
        Err(e) => db_error(e).into_response(),
        Ok(_) => (
            StatusCode::CREATED,
            Json(ConversationResponse {
                id,
                title: req.title,
                messages: req.messages,
                created_at: now,
                updated_at: now,
            }),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// GET /api/v1/chat/{id}
// ---------------------------------------------------------------------------

async fn get_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !same_origin_ok(&headers) {
        return forbidden().into_response();
    }

    let row = sqlx::query(
        "SELECT id, title, messages, created_at, updated_at FROM chat_conversations WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await;

    match row {
        Err(e) => db_error(e).into_response(),
        Ok(None) => not_found(&id).into_response(),
        Ok(Some(r)) => {
            use sqlx::Row as _;
            let messages_raw: String = r.get("messages");
            match deserialize_messages(&messages_raw) {
                Err(e) => e.into_response(),
                Ok(msgs) => Json(ConversationResponse {
                    id: r.get("id"),
                    title: r.get("title"),
                    messages: msgs,
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                })
                .into_response(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PUT /api/v1/chat/{id}
// ---------------------------------------------------------------------------

async fn update_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateRequest>,
) -> impl IntoResponse {
    if !same_origin_ok(&headers) {
        return forbidden().into_response();
    }

    // At least one field must be set.
    if req.title.is_none() && req.messages.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                status: "bad_request",
                detail: "at least one of title or messages must be provided".to_string(),
            }),
        )
            .into_response();
    }

    // Validate title if provided.
    if let Some(ref t) = req.title {
        if let Err(e) = validate_title(t) {
            return e.into_response();
        }
    }

    // Validate messages if provided.
    let messages_json_opt = if let Some(ref msgs) = req.messages {
        match validate_and_serialize_messages(msgs) {
            Ok(j) => Some(j),
            Err(e) => return e.into_response(),
        }
    } else {
        None
    };

    // Fetch the current row.
    let existing = sqlx::query(
        "SELECT id, title, messages, created_at, updated_at FROM chat_conversations WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await;

    let existing = match existing {
        Err(e) => return db_error(e).into_response(),
        Ok(None) => return not_found(&id).into_response(),
        Ok(Some(r)) => r,
    };

    use sqlx::Row as _;
    let existing_title: String = existing.get("title");
    let existing_messages: String = existing.get("messages");
    let existing_created_at: i64 = existing.get("created_at");

    let new_title = req.title.as_deref().unwrap_or(&existing_title);
    let new_messages_json = messages_json_opt.as_deref().unwrap_or(&existing_messages);
    let now = Utc::now().timestamp();

    let result = sqlx::query(
        "UPDATE chat_conversations SET title = ?, messages = ?, updated_at = ? WHERE id = ?",
    )
    .bind(new_title)
    .bind(new_messages_json)
    .bind(now)
    .bind(&id)
    .execute(&state.pool)
    .await;

    match result {
        Err(e) => db_error(e).into_response(),
        Ok(_) => match deserialize_messages(new_messages_json) {
            Err(e) => e.into_response(),
            Ok(messages) => Json(ConversationResponse {
                id,
                title: new_title.to_string(),
                messages,
                created_at: existing_created_at,
                updated_at: now,
            })
            .into_response(),
        },
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/chat/{id}
// ---------------------------------------------------------------------------

async fn delete_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !same_origin_ok(&headers) {
        return forbidden().into_response();
    }

    let result = sqlx::query("DELETE FROM chat_conversations WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await;

    match result {
        Err(e) => db_error(e).into_response(),
        Ok(r) if r.rows_affected() == 0 => not_found(&id).into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/", get(list_conversations).post(create_conversation))
        .route(
            "/{id}",
            get(get_conversation)
                .put(update_conversation)
                .delete(delete_conversation),
        )
}
