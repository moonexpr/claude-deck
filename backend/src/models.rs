use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub is_active: bool,
    pub last_accessed: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Backup {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub file_path: String,
    pub scope: String,
    pub project_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub size_bytes: i64,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Marketplace {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub last_synced: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct SessionCache {
    pub id: i64,
    pub session_id: String,
    pub project_folder: String,
    pub project_name: String,
    pub summary: String,
    pub modified_at: DateTime<Utc>,
    pub size_bytes: i64,
    pub total_messages: i64,
    pub total_tool_calls: i64,
    pub cached_at: DateTime<Utc>,
    pub file_hash: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct UsageCache {
    pub id: i64,
    pub cache_key: String,
    pub cache_type: String,
    pub project_path: Option<String>,
    pub data: serde_json::Value,
    pub cached_at: DateTime<Utc>,
    pub file_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct MCPServerCache {
    pub id: i64,
    pub server_name: String,
    pub server_scope: String,
    pub is_connected: bool,
    pub last_tested_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub mcp_server_name: Option<String>,
    pub mcp_server_version: Option<String>,
    pub tools: Option<serde_json::Value>,
    pub tool_count: i32,
    pub resources: Option<serde_json::Value>,
    pub prompts: Option<serde_json::Value>,
    pub resource_count: i32,
    pub prompt_count: i32,
    pub capabilities: Option<serde_json::Value>,
    pub cached_at: DateTime<Utc>,
    pub config_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct PresenceEvent {
    pub id: i64,
    pub session_id: String,
    pub event_type: String,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_result: Option<serde_json::Value>,
    pub message: Option<String>,
    pub cwd: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct PresenceSession {
    pub id: i64,
    pub session_id: String,
    pub label: Option<String>,
    pub project_path: Option<String>,
    pub status: String,
    pub status_text: Option<String>,
    pub last_narrative: Option<String>,
    pub last_narrative_at: Option<DateTime<Utc>>,
    pub modified_files: Option<serde_json::Value>,
    pub last_command: Option<String>,
    pub last_command_exit: Option<i32>,
    pub activity_buckets: Option<serde_json::Value>,
    pub bucket_start: Option<DateTime<Utc>>,
    pub total_events: i64,
    pub error_count: i64,
    pub started_at: DateTime<Utc>,
    pub last_event_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub last_user_prompt: Option<String>,
}
