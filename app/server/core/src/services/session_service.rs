use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

/// Conversations are paginated this many user-prompts per page (parity with
/// the Python reference backend).
const PROMPTS_PER_PAGE: usize = 5;

/// A typed content block within a message. Mirrors the frontend
/// `ContentBlock` (app/web/src/types/sessions.ts). Unknown JSONL fields
/// (e.g. thinking `signature`, tool_use `caller`) are ignored on parse.
/// `tool_result` blocks use `tool_use_id` in the raw JSONL — the serde alias
/// folds that into `id` so the frontend's `block.id` is populated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, alias = "tool_use_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Value>,
}

/// A single user or assistant message. Mirrors the frontend `SessionMessage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    #[serde(rename = "type")]
    pub type_: String,
    pub timestamp: String,
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Value>,
}

/// A conversation: one user prompt plus the assistant/tool messages that
/// follow it. Mirrors the frontend `SessionConversation`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConversation {
    pub user_text: String,
    pub timestamp: String,
    pub messages: Vec<SessionMessage>,
    pub is_continuation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_count: Option<u64>,
}

/// Full session transcript. Mirrors the frontend `SessionDetail`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub id: String,
    pub project_folder: String,
    pub project_name: String,
    pub conversations: Vec<SessionConversation>,
    pub total_messages: usize,
    pub total_tool_calls: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    pub models_used: Vec<String>,
}

/// Paginated session-detail envelope. Mirrors the frontend
/// `SessionDetailResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetailResponse {
    pub session: SessionDetail,
    pub current_page: usize,
    pub total_pages: usize,
    pub prompts_per_page: usize,
}

/// A session list-row summary. Mirrors the frontend `SessionSummary`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub project_folder: String,
    pub project_name: String,
    pub summary: String,
    pub modified_at: String,
    pub size_bytes: u64,
    pub total_messages: usize,
    pub total_tool_calls: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionSummary>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionProject {
    pub folder: String,
    pub name: String,
    pub session_count: usize,
    pub most_recent: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionProjectListResponse {
    pub projects: Vec<SessionProject>,
    pub total_sessions: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionStatsResponse {
    pub total_sessions: usize,
    pub sessions_today: usize,
    pub sessions_this_week: usize,
    pub most_active_project: Option<String>,
    pub total_messages: usize,
}

pub struct SessionService {
    projects_dir: PathBuf,
}

impl SessionService {
    pub fn new(projects_dir: PathBuf) -> Self {
        Self { projects_dir }
    }

    /// Validate that path component doesn't contain traversal sequences.
    fn validate_path_component(&self, component: &str) -> Result<String> {
        if component.contains("..") || component.contains('/') || component.contains('\\') {
            return Err(anyhow!("Invalid path component: traversal detected"));
        }
        if component.starts_with('.') {
            return Err(anyhow!("Hidden paths not allowed"));
        }
        if component.trim().is_empty() || component == "." || component == ".." {
            return Err(anyhow!("Invalid path component: empty or restricted name"));
        }
        Ok(component.to_string())
    }

    /// Safely resolve a filepath within the projects directory.
    pub async fn resolve_session_path(
        &self,
        project_folder: &str,
        session_id: &str,
    ) -> Result<PathBuf> {
        let project_folder = self.validate_path_component(project_folder)?;
        let session_id = self.validate_path_component(session_id)?;

        let mut path = self.projects_dir.clone();
        path.push(project_folder);
        path.push(format!("{}.jsonl", session_id));

        // Canonicalize to check if it's still within projects_dir
        let canonical_projects = fs::canonicalize(&self.projects_dir).await?;
        let canonical_file = fs::canonicalize(&path)
            .await
            .map_err(|e| anyhow!("File not found or inaccessible: {}", e))?;

        if !canonical_file.starts_with(&canonical_projects) {
            return Err(anyhow!(
                "Path traversal detected outside projects directory"
            ));
        }

        Ok(canonical_file)
    }

    /// Parse JSONL file with safety limits to prevent DoS.
    pub async fn parse_jsonl_file(&self, filepath: &Path) -> Result<Vec<Value>> {
        const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
        const MAX_ENTRIES: usize = 10000;

        let metadata = fs::metadata(filepath).await?;
        if metadata.len() > MAX_FILE_SIZE {
            return Err(anyhow!(
                "File too large: {} bytes (max {} bytes)",
                metadata.len(),
                MAX_FILE_SIZE
            ));
        }

        let file = File::open(filepath).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut entries = Vec::new();

        while let Some(line) = lines.next_line().await? {
            if entries.len() >= MAX_ENTRIES {
                return Err(anyhow!(
                    "Too many entries in session file (max {})",
                    MAX_ENTRIES
                ));
            }
            if !line.trim().is_empty()
                && let Ok(json) = serde_json::from_str(&line) {
                    entries.push(json);
                }
        }

        Ok(entries)
    }

    /// Calculate SHA-256 hash of file content (optimized for large files).
    pub async fn get_file_hash(&self, filepath: &Path) -> Result<String> {
        let metadata = fs::metadata(filepath).await?;
        let size = metadata.len();
        let mtime = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let mut hasher = Sha256::new();

        if size < 1024 * 1024 {
            // < 1MB, hash everything
            let content = fs::read(filepath).await?;
            hasher.update(&content);
        } else {
            // For large files, hash first 64KB and last 64KB + metadata
            let mut file = File::open(filepath).await?;
            let mut buffer = vec![0u8; 65536];

            // Read first 64KB
            let n = file.read(&mut buffer).await?;
            hasher.update(&buffer[..n]);

            // Seek to last 64KB
            if size > 65536 {
                let pos = size - 65536;
                tokio::io::AsyncSeekExt::seek(&mut file, io::SeekFrom::Start(pos)).await?;
                let n = file.read(&mut buffer).await?;
                hasher.update(&buffer[..n]);
            }
        }

        // Include metadata in hash
        hasher.update(format!("{}:{}", size, mtime).as_bytes());

        Ok(hex::encode(hasher.finalize()))
    }

    /// Convert a Claude project folder name to a display name.
    /// `-Users-jc-Garden-external-claude-deck` -> `deck`. Mirrors the Python
    /// `get_project_display_name`: split on `-`, return the last part when
    /// there are more than 3 parts, else the folder name verbatim.
    fn project_display_name(folder_name: &str) -> String {
        let parts: Vec<&str> = folder_name.split('-').collect();
        if parts.len() > 3 {
            parts.last().unwrap_or(&folder_name).to_string()
        } else {
            folder_name.to_string()
        }
    }

    /// Extract plain text from a message `content` value (string or array of
    /// blocks). Only `text` blocks contribute; joined with spaces and trimmed.
    fn extract_text_from_content(content: &Value) -> String {
        match content {
            Value::String(s) => s.trim().to_string(),
            Value::Array(blocks) => {
                let texts: Vec<String> = blocks
                    .iter()
                    .filter_map(|b| {
                        if b.get("type").and_then(Value::as_str) == Some("text") {
                            b.get("text")
                                .and_then(Value::as_str)
                                .filter(|t| !t.is_empty())
                                .map(str::to_string)
                        } else {
                            None
                        }
                    })
                    .collect();
                texts.join(" ").trim().to_string()
            }
            _ => String::new(),
        }
    }

    /// Build a `SessionMessage` from a raw JSONL entry (a `user`/`assistant`
    /// line). `message.content` may be a string (wrapped into a single text
    /// block) or an array of blocks; unparseable blocks are skipped.
    fn build_session_message(entry: &Value) -> SessionMessage {
        let message = entry.get("message");
        let content_val = message.and_then(|m| m.get("content"));

        let content: Vec<ContentBlock> = match content_val {
            Some(Value::String(s)) => vec![ContentBlock {
                type_: "text".to_string(),
                text: Some(s.clone()),
                thinking: None,
                name: None,
                id: None,
                input: None,
                content: None,
                is_error: None,
                source: None,
            }],
            Some(Value::Array(blocks)) => blocks
                .iter()
                .filter(|b| b.is_object())
                .filter_map(|b| serde_json::from_value::<ContentBlock>(b.clone()).ok())
                .collect(),
            _ => Vec::new(),
        };

        SessionMessage {
            type_: entry
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("user")
                .to_string(),
            timestamp: entry
                .get("timestamp")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            content,
            model: message
                .and_then(|m| m.get("model"))
                .and_then(Value::as_str)
                .map(str::to_string),
            usage: message.and_then(|m| m.get("usage")).cloned(),
        }
    }

    /// Group raw JSONL entries into conversations. A non-meta `user` entry
    /// starts a new conversation; subsequent `assistant` entries append to it.
    /// Mirrors the Python `parse_session_to_conversations`.
    fn parse_session_to_conversations(entries: &[Value]) -> Vec<SessionConversation> {
        let mut conversations: Vec<SessionConversation> = Vec::new();
        let mut current: Option<SessionConversation> = None;

        for entry in entries {
            let entry_type = entry.get("type").and_then(Value::as_str);
            let is_meta = entry.get("isMeta").and_then(Value::as_bool).unwrap_or(false);

            match entry_type {
                Some("user") if !is_meta => {
                    if let Some(convo) = current.take() {
                        conversations.push(convo);
                    }
                    let raw_text = entry
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .map(Self::extract_text_from_content)
                        .unwrap_or_default();
                    let user_text = if raw_text.chars().count() > 100 {
                        let truncated: String = raw_text.chars().take(100).collect();
                        format!("{}...", truncated)
                    } else {
                        raw_text
                    };
                    current = Some(SessionConversation {
                        user_text,
                        timestamp: entry
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        messages: vec![Self::build_session_message(entry)],
                        is_continuation: false,
                        token_count: None,
                    });
                }
                Some("assistant") => {
                    if let Some(convo) = current.as_mut() {
                        convo.messages.push(Self::build_session_message(entry));
                    }
                }
                _ => {}
            }
        }

        if let Some(convo) = current.take() {
            conversations.push(convo);
        }

        conversations
    }

    /// Build the grouped, paginated session-detail response from a session's
    /// JSONL entries. This is the contract the frontend `getSessionDetail`
    /// consumes (`SessionDetailResponse`).
    pub fn build_session_detail(
        entries: &[Value],
        session_id: &str,
        project_folder: &str,
        page: usize,
    ) -> SessionDetailResponse {
        let conversations = Self::parse_session_to_conversations(entries);

        let total_pages = conversations.len().div_ceil(PROMPTS_PER_PAGE);
        let page = page.max(1);
        let start = (page - 1) * PROMPTS_PER_PAGE;
        let end = (start + PROMPTS_PER_PAGE).min(conversations.len());
        let paginated = if start < conversations.len() {
            conversations[start..end].to_vec()
        } else {
            Vec::new()
        };

        let total_messages = entries
            .iter()
            .filter(|e| matches!(e.get("type").and_then(Value::as_str), Some("user") | Some("assistant")))
            .count();

        let total_tool_calls = entries
            .iter()
            .filter(|e| e.get("type").and_then(Value::as_str) == Some("assistant"))
            .flat_map(|e| {
                e.get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(Value::as_array)
                    .map(|a| a.as_slice())
                    .unwrap_or(&[])
            })
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
            .count();

        let mut models_used: Vec<String> = Vec::new();
        for entry in entries {
            if entry.get("type").and_then(Value::as_str) == Some("assistant")
                && let Some(model) = entry
                    .get("message")
                    .and_then(|m| m.get("model"))
                    .and_then(Value::as_str)
                && !models_used.iter().any(|m| m == model)
            {
                models_used.push(model.to_string());
            }
        }

        let detail = SessionDetail {
            id: session_id.to_string(),
            project_folder: project_folder.to_string(),
            project_name: Self::project_display_name(project_folder),
            conversations: paginated,
            total_messages,
            total_tool_calls,
            total_tokens: None,
            models_used,
        };

        SessionDetailResponse {
            session: detail,
            current_page: page,
            total_pages,
            prompts_per_page: PROMPTS_PER_PAGE,
        }
    }

    /// Parse a session file and return the grouped, paginated detail response.
    pub async fn get_session_detail(
        &self,
        project_folder: &str,
        session_id: &str,
        page: usize,
    ) -> Result<SessionDetailResponse> {
        let path = self.resolve_session_path(project_folder, session_id).await?;
        let entries = self.parse_jsonl_file(&path).await?;
        Ok(Self::build_session_detail(
            &entries,
            session_id,
            project_folder,
            page,
        ))
    }

    /// Scan `projects_dir` for `.jsonl` session files (optionally one project),
    /// returning `(folder, session_id, path, modified, size)` from fs metadata
    /// only — no file contents are read here.
    async fn scan_session_files(
        &self,
        project_filter: Option<&str>,
    ) -> Result<Vec<(String, String, PathBuf, std::time::SystemTime, u64)>> {
        let mut out = Vec::new();
        let mut dir = match fs::read_dir(&self.projects_dir).await {
            Ok(d) => d,
            Err(_) => return Ok(out), // projects dir missing → no sessions
        };
        while let Some(entry) = dir.next_entry().await? {
            let folder = entry.file_name().to_string_lossy().to_string();
            if let Some(filter) = project_filter
                && folder != filter
            {
                continue;
            }
            match entry.metadata().await {
                Ok(m) if m.is_dir() => {}
                _ => continue,
            }
            let mut files = match fs::read_dir(entry.path()).await {
                Ok(d) => d,
                Err(_) => continue,
            };
            while let Some(f) = files.next_entry().await? {
                let fname = f.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".jsonl") {
                    continue;
                }
                let meta = match f.metadata().await {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                let id = fname.trim_end_matches(".jsonl").to_string();
                out.push((folder.clone(), id, f.path(), modified, meta.len()));
            }
        }
        Ok(out)
    }

    /// List sessions (most-recent first), fully parsing at most `limit` files
    /// for their summaries so a large `projects_dir` stays responsive.
    pub async fn list_sessions(
        &self,
        project_filter: Option<&str>,
        limit: usize,
    ) -> Result<SessionListResponse> {
        let mut files = self.scan_session_files(project_filter).await?;
        let total = files.len();
        files.sort_by(|a, b| b.3.cmp(&a.3)); // modified desc
        files.truncate(limit);

        let mut sessions = Vec::with_capacity(files.len());
        for (folder, id, path, modified, size) in files {
            if let Ok(s) = self.summarize_session(&path, &folder, &id, modified, size).await {
                sessions.push(s);
            }
        }
        Ok(SessionListResponse { sessions, total })
    }

    /// Parse one session file into a list summary (first user prompt + counts).
    async fn summarize_session(
        &self,
        path: &Path,
        folder: &str,
        id: &str,
        modified: std::time::SystemTime,
        size: u64,
    ) -> Result<SessionSummary> {
        let content = fs::read_to_string(path).await?;
        let mut total_messages = 0usize;
        let mut total_tool_calls = 0usize;
        let mut summary = String::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            match v.get("type").and_then(Value::as_str) {
                Some("user") => {
                    total_messages += 1;
                    let is_meta = v.get("isMeta").and_then(Value::as_bool).unwrap_or(false);
                    if summary.is_empty() && !is_meta {
                        let raw = v
                            .get("message")
                            .and_then(|m| m.get("content"))
                            .map(Self::extract_text_from_content)
                            .unwrap_or_default();
                        if !raw.is_empty() {
                            summary = raw.chars().take(200).collect();
                        }
                    }
                }
                Some("assistant") => {
                    total_messages += 1;
                    if let Some(blocks) = v
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(Value::as_array)
                    {
                        total_tool_calls += blocks
                            .iter()
                            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                            .count();
                    }
                }
                _ => {}
            }
        }

        Ok(SessionSummary {
            id: id.to_string(),
            project_folder: folder.to_string(),
            project_name: Self::project_display_name(folder),
            summary,
            modified_at: chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339(),
            size_bytes: size,
            total_messages,
            total_tool_calls,
        })
    }

    /// List projects with session counts and the most-recent activity, from fs
    /// metadata only.
    pub async fn list_projects(&self) -> Result<SessionProjectListResponse> {
        let files = self.scan_session_files(None).await?;
        let total_sessions = files.len();
        let mut by_folder: std::collections::HashMap<String, (usize, std::time::SystemTime)> =
            std::collections::HashMap::new();
        for (folder, _id, _path, modified, _size) in &files {
            let e = by_folder
                .entry(folder.clone())
                .or_insert((0, std::time::UNIX_EPOCH));
            e.0 += 1;
            if *modified > e.1 {
                e.1 = *modified;
            }
        }
        let mut projects: Vec<SessionProject> = by_folder
            .into_iter()
            .map(|(folder, (count, recent))| SessionProject {
                name: Self::project_display_name(&folder),
                folder,
                session_count: count,
                most_recent: chrono::DateTime::<chrono::Utc>::from(recent).to_rfc3339(),
            })
            .collect();
        projects.sort_by(|a, b| b.most_recent.cmp(&a.most_recent));
        Ok(SessionProjectListResponse {
            projects,
            total_sessions,
        })
    }

    /// Dashboard stats. Session counts come from fs metadata; `total_messages`
    /// is an entry-count sum across all files (read but not fully parsed).
    pub async fn dashboard_stats(&self) -> Result<SessionStatsResponse> {
        let files = self.scan_session_files(None).await?;
        let total_sessions = files.len();
        let now = std::time::SystemTime::now();
        let day = std::time::Duration::from_secs(86_400);
        let week = std::time::Duration::from_secs(7 * 86_400);

        let mut sessions_today = 0;
        let mut sessions_this_week = 0;
        let mut by_folder: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for (folder, _id, _path, modified, _size) in &files {
            if let Ok(age) = now.duration_since(*modified) {
                if age <= day {
                    sessions_today += 1;
                }
                if age <= week {
                    sessions_this_week += 1;
                }
            }
            *by_folder.entry(folder.clone()).or_insert(0) += 1;
        }
        let most_active_project = by_folder
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(f, _)| Self::project_display_name(&f));

        // total_messages: cheap newline-count sum (≈ entries). TODO: cache.
        let mut total_messages = 0usize;
        for (_f, _id, path, _m, _s) in &files {
            if let Ok(content) = fs::read_to_string(path).await {
                total_messages += content.lines().filter(|l| !l.trim().is_empty()).count();
            }
        }

        Ok(SessionStatsResponse {
            total_sessions,
            sessions_today,
            sessions_this_week,
            most_active_project,
            total_messages,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user(text: &str) -> Value {
        json!({"type": "user", "timestamp": "t0", "message": {"role": "user", "content": text}})
    }

    fn assistant(model: &str, content: Value) -> Value {
        json!({"type": "assistant", "timestamp": "t1",
            "message": {"role": "assistant", "model": model, "usage": {"input_tokens": 10, "output_tokens": 5}, "content": content}})
    }

    #[test]
    fn groups_user_then_assistant_into_one_conversation() {
        let entries = vec![
            user("hello"),
            assistant("claude-opus-4-8", json!([
                {"type": "text", "text": "hi"},
                {"type": "tool_use", "id": "tu1", "name": "Bash", "input": {"command": "ls"}, "caller": "x"},
            ])),
        ];
        let resp = SessionService::build_session_detail(&entries, "sid", "-a-b-c-d-proj", 1);
        assert_eq!(resp.session.conversations.len(), 1);
        let c = &resp.session.conversations[0];
        assert_eq!(c.user_text, "hello");
        assert_eq!(c.messages.len(), 2);
        assert_eq!(resp.session.total_messages, 2);
        assert_eq!(resp.session.total_tool_calls, 1);
        assert_eq!(resp.session.models_used, vec!["claude-opus-4-8"]);
        assert_eq!(resp.session.project_name, "proj"); // >3 parts -> last
    }

    #[test]
    fn new_user_starts_new_conversation() {
        let entries = vec![user("first"), assistant("m", json!([])), user("second")];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        assert_eq!(resp.session.conversations.len(), 2);
        assert_eq!(resp.session.conversations[1].user_text, "second");
    }

    #[test]
    fn tool_result_id_folds_from_tool_use_id() {
        // tool_result blocks carry `tool_use_id` in raw JSONL; the frontend
        // reads block.id, so it must serialize back as "id".
        let entries = vec![
            user("go"),
            json!({"type": "user", "timestamp": "t", "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "tu-42", "content": "ok", "is_error": false}
            ]}}),
        ];
        // The tool_result is on a *new* user entry -> its own conversation.
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        let block = &resp.session.conversations[1].messages[0].content[0];
        assert_eq!(block.type_, "tool_result");
        assert_eq!(block.id.as_deref(), Some("tu-42"));
        let v = serde_json::to_value(block).unwrap();
        assert_eq!(v.get("id").and_then(Value::as_str), Some("tu-42"));
        assert!(v.get("tool_use_id").is_none());
    }

    #[test]
    fn string_content_becomes_text_block() {
        let entries = vec![user("plain string")];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        let block = &resp.session.conversations[0].messages[0].content[0];
        assert_eq!(block.type_, "text");
        assert_eq!(block.text.as_deref(), Some("plain string"));
    }

    #[test]
    fn pagination_slices_five_per_page() {
        let mut entries = Vec::new();
        for i in 0..12 {
            entries.push(user(&format!("q{i}")));
            entries.push(assistant("m", json!([])));
        }
        let resp = SessionService::build_session_detail(&entries, "s", "p", 2);
        assert_eq!(resp.total_pages, 3);
        assert_eq!(resp.current_page, 2);
        assert_eq!(resp.prompts_per_page, 5);
        assert_eq!(resp.session.conversations.len(), 5);
        assert_eq!(resp.session.conversations[0].user_text, "q5");
        // total counts span the whole session, not just the page.
        assert_eq!(resp.session.total_messages, 24);
    }

    #[test]
    fn out_of_range_page_returns_empty_slice() {
        let entries = vec![user("only")];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 9);
        assert!(resp.session.conversations.is_empty());
        assert_eq!(resp.total_pages, 1);
    }

    #[test]
    fn long_user_text_is_truncated_to_100_chars_plus_ellipsis() {
        let long = "x".repeat(250);
        let entries = vec![user(&long)];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        let ut = &resp.session.conversations[0].user_text;
        assert_eq!(ut.chars().count(), 103); // 100 + "..."
        assert!(ut.ends_with("..."));
    }

    #[test]
    fn meta_user_does_not_start_conversation() {
        let entries = vec![
            json!({"type": "user", "isMeta": true, "timestamp": "t", "message": {"content": "meta"}}),
            user("real"),
        ];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        assert_eq!(resp.session.conversations.len(), 1);
        assert_eq!(resp.session.conversations[0].user_text, "real");
    }

    #[test]
    fn non_message_entry_types_are_ignored() {
        let entries = vec![
            json!({"type": "summary", "summary": "s"}),
            json!({"type": "system", "content": "x"}),
            user("q"),
            json!({"type": "file-history-snapshot"}),
            assistant("m", json!([{"type": "thinking", "thinking": "hmm", "signature": "sig"}])),
        ];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        assert_eq!(resp.session.conversations.len(), 1);
        assert_eq!(resp.session.total_messages, 2);
        // thinking block parsed, signature dropped on re-serialize.
        let blk = &resp.session.conversations[0].messages[1].content[0];
        assert_eq!(blk.type_, "thinking");
        assert_eq!(blk.thinking.as_deref(), Some("hmm"));
        let v = serde_json::to_value(blk).unwrap();
        assert!(v.get("signature").is_none());
    }

    #[test]
    fn serializes_with_frontend_field_names() {
        let entries = vec![user("q"), assistant("m", json!([{"type": "text", "text": "a"}]))];
        let resp = SessionService::build_session_detail(&entries, "sid", "p", 1);
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v.get("current_page").is_some());
        assert!(v.get("total_pages").is_some());
        assert!(v.get("prompts_per_page").is_some());
        let sess = v.get("session").unwrap();
        assert!(sess.get("total_messages").is_some());
        assert!(sess.get("models_used").is_some());
        let convo = &sess.get("conversations").unwrap()[0];
        assert!(convo.get("user_text").is_some());
        assert!(convo.get("is_continuation").is_some());
        let msg = &convo.get("messages").unwrap()[0];
        assert_eq!(msg.get("type").and_then(Value::as_str), Some("user"));
        assert!(msg.get("type_").is_none());
    }

    // --- Cross-model adversarial round (inputs generated by DeepSeek-V3 via the
    // HF Inference Providers router; expectations judged against the contract,
    // not the model's verdict; executed here — execution is the only critic). ---

    #[test]
    fn adv_multibyte_truncation_no_panic_and_char_counted() {
        // 150 two-byte chars. Truncation is by char (code point), matching the
        // Python reference — NOT graphemes. Must not panic / split a char.
        let long = "é".repeat(150);
        let entries = vec![user(&long)];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        let ut = &resp.session.conversations[0].user_text;
        assert_eq!(ut.chars().count(), 103); // 100 chars + "..."
        assert!(ut.ends_with("..."));
        assert!(std::str::from_utf8(ut.as_bytes()).is_ok());
    }

    #[test]
    fn adv_emoji_truncation_no_panic() {
        // ZWJ family emoji are multi-scalar; truncating by char may cut a
        // sequence but must remain valid UTF-8 and must not panic.
        let long = "👨‍👩‍👧".repeat(60);
        let entries = vec![user(&long)];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        let ut = &resp.session.conversations[0].user_text;
        assert!(ut.ends_with("..."));
        assert!(std::str::from_utf8(ut.as_bytes()).is_ok());
    }

    #[test]
    fn adv_non_string_content_does_not_crash() {
        for c in [json!(42), json!(null), json!({"k": "v"}), json!(true)] {
            let entries = vec![json!({"type": "user", "timestamp": "t", "message": {"content": c}})];
            let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
            assert_eq!(resp.session.conversations.len(), 1);
            assert_eq!(resp.session.conversations[0].user_text, "");
            assert_eq!(resp.session.conversations[0].messages[0].content.len(), 0);
        }
    }

    #[test]
    fn adv_missing_message_field() {
        let entries = vec![json!({"type": "user", "timestamp": "t"})];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        assert_eq!(resp.session.conversations.len(), 1);
        assert_eq!(resp.session.conversations[0].user_text, "");
        assert_eq!(resp.session.conversations[0].messages[0].content.len(), 0);
    }

    #[test]
    fn adv_ismeta_is_strict_true_only() {
        // Contract is `isMeta == true` (strict), per the goal spec — so a
        // non-boolean truthy value (1) is NOT meta and DOES start a convo.
        // (Deliberate divergence from Python's truthy check; real data uses bool.)
        let entries = vec![
            json!({"type": "user", "isMeta": 1, "timestamp": "t", "message": {"content": "truthy"}}),
            json!({"type": "user", "isMeta": true, "timestamp": "t", "message": {"content": "metatrue"}}),
            user("real"),
        ];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        let texts: Vec<&str> = resp.session.conversations.iter().map(|c| c.user_text.as_str()).collect();
        assert_eq!(texts, vec!["truthy", "real"]); // isMeta:true skipped; isMeta:1 kept
    }

    #[test]
    fn adv_tool_use_without_id_is_fine() {
        let entries = vec![
            user("go"),
            assistant("m", json!([{"type": "tool_use", "name": "Bash", "input": {"command": "ls"}}])),
        ];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        let blk = &resp.session.conversations[0].messages[1].content[0];
        assert_eq!(blk.type_, "tool_use");
        assert!(blk.id.is_none());
        assert_eq!(resp.session.total_tool_calls, 1);
        let v = serde_json::to_value(blk).unwrap();
        assert!(v.get("id").is_none()); // omitted, not null
    }

    #[test]
    fn adv_page_zero_acts_as_page_one() {
        let entries = vec![user("a"), user("b")];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 0);
        assert_eq!(resp.current_page, 1);
        assert_eq!(resp.session.conversations.len(), 2);
    }

    #[test]
    fn adv_mixed_blocks_user_text_is_text_only() {
        let entries = vec![json!({"type": "user", "timestamp": "t", "message": {"content": [
            {"type": "text", "text": "hello"},
            {"type": "tool_result", "tool_use_id": "x", "content": "ignored"},
            {"type": "image", "source": {"data": "..."}},
        ]}})];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        assert_eq!(resp.session.conversations[0].user_text, "hello");
        // all three blocks are still carried on the message
        assert_eq!(resp.session.conversations[0].messages[0].content.len(), 3);
    }

    #[test]
    fn adv_assistant_before_user_dropped_but_still_counted() {
        let entries = vec![assistant("m", json!([])), user("q"), assistant("m", json!([]))];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        assert_eq!(resp.session.conversations.len(), 1);
        assert_eq!(resp.session.conversations[0].messages.len(), 2); // orphan dropped from grouping
        assert_eq!(resp.session.total_messages, 3); // but counted in whole-session stats
    }

    #[test]
    fn adv_null_and_duplicate_models_distinct() {
        let entries = vec![
            user("q"),
            assistant("claude-opus-4-8", json!([])),
            json!({"type": "assistant", "timestamp": "t", "message": {"model": null, "content": []}}),
            assistant("claude-opus-4-8", json!([])),
        ];
        let resp = SessionService::build_session_detail(&entries, "s", "p", 1);
        assert_eq!(resp.session.models_used, vec!["claude-opus-4-8"]);
    }
}
