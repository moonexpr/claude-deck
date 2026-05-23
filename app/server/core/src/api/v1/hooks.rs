// PORTED: hook_service.py + api/v1/hooks.py

use axum::{
    Router,
    extract::{Json, Path, Query},
    routing::{get, put},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::paths;

static UUID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate an RFC-4122 v4-format UUID string. No `uuid` crate is available in
/// Cargo.toml (and the contract forbids editing it), so entropy is derived by
/// hashing high-resolution time + pid + an atomic counter + a stack address.
/// The frontend treats the id as an opaque string; only the
/// `str(uuid.uuid4())` *shape* matters, which this reproduces exactly.
fn gen_uuid_v4() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let counter = UUID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let stack_marker = 0u8;
    let stack_addr = &stack_marker as *const u8 as usize as u64;

    let mut hasher = Sha256::new();
    hasher.update(now.as_secs().to_le_bytes());
    hasher.update(now.subsec_nanos().to_le_bytes());
    hasher.update((std::process::id() as u64).to_le_bytes());
    hasher.update(counter.to_le_bytes());
    hasher.update(stack_addr.to_le_bytes());
    let h = hasher.finalize();

    let mut b = [0u8; 16];
    b.copy_from_slice(&h[..16]);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC-4122 variant

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

const VALID_HOOK_EVENTS: [&str; 12] = [
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PermissionRequest",
    "Notification",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
];

pub fn router() -> Router<ApiState> {
    // Python `APIRouter(prefix="/hooks")` exposes:
    //   GET    ""            list_hooks
    //   GET    "/{event}"    get_hooks_by_event
    //   POST   ""            create_hook
    //   PUT    "/{hook_id}"  update_hook
    //   DELETE "/{hook_id}"  delete_hook
    //
    // axum 0.8 panics if two routes put differently-named path params at the
    // same position (`/{event}` vs `/{hook_id}`). The unchanged frontend
    // (frontend/src/features/hooks/HooksPage.tsx) only calls:
    //   GET/POST hooks            PUT/DELETE hooks/{hookId}
    // It never calls GET hooks/{event}. So `/{hook_id}` owns the dynamic
    // position the frontend uses, and the by-event lookup keeps its Python
    // behavior under a static-disambiguated path that cannot collide.
    Router::new()
        .route("/", get(list_hooks).post(create_hook))
        .route("/{hook_id}", put(update_hook).delete(delete_hook))
        .route("/by-event/{event}", get(get_hooks_by_event))
}

#[derive(Deserialize)]
struct ProjectPathQuery {
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct ScopeQuery {
    scope: String,
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct HookCreate {
    event: String,
    #[serde(default)]
    matcher: Option<String>,
    #[serde(default = "default_type")]
    r#type: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "async")]
    async_: Option<bool>,
    #[serde(default)]
    statusMessage: Option<String>,
    #[serde(default)]
    once: Option<bool>,
    #[serde(default)]
    timeout: Option<i64>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    headers: Option<Map<String, Value>>,
    #[serde(default)]
    allowedEnvVars: Option<Vec<String>>,
    scope: String,
}

fn default_type() -> String {
    "command".to_string()
}

#[derive(Deserialize)]
struct HookUpdate {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    matcher: Option<String>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "async")]
    async_: Option<bool>,
    #[serde(default)]
    statusMessage: Option<String>,
    #[serde(default)]
    once: Option<bool>,
    #[serde(default)]
    timeout: Option<i64>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    headers: Option<Map<String, Value>>,
    #[serde(default)]
    allowedEnvVars: Option<Vec<String>>,
}

// ---- helpers ----------------------------------------------------------------

fn settings_file_for_scope(scope: &str, project_path: Option<&str>) -> PathBuf {
    if scope == "user" {
        paths::get_claude_user_settings_file()
    } else {
        paths::get_project_settings_file(project_path, std::path::Path::new(""))
    }
}

fn read_settings(file: &std::path::Path) -> Option<Value> {
    let content = std::fs::read_to_string(file).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_settings(file: &std::path::Path, settings: &Value) -> std::io::Result<()> {
    // Python: json.dump(settings, f, indent=2)
    let serialized = serde_json::to_string_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(file, serialized)
}

/// Port of `HookService._parse_hook_from_data`.
fn parse_hook_from_data(hook_data: &Value, event: &str, scope: &str) -> Value {
    let get = |k: &str| hook_data.get(k).cloned();
    let id = match hook_data.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => gen_uuid_v4(),
    };
    let type_ = hook_data
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("command")
        .to_string();
    json!({
        "id": id,
        "event": event,
        "matcher": get("matcher").unwrap_or(Value::Null),
        "type": type_,
        "command": get("command").unwrap_or(Value::Null),
        "prompt": get("prompt").unwrap_or(Value::Null),
        "model": get("model").unwrap_or(Value::Null),
        "async": get("async").unwrap_or(Value::Null),
        "statusMessage": get("statusMessage").unwrap_or(Value::Null),
        "once": get("once").unwrap_or(Value::Null),
        "timeout": get("timeout").unwrap_or(Value::Null),
        "url": get("url").unwrap_or(Value::Null),
        "headers": get("headers").unwrap_or(Value::Null),
        "allowedEnvVars": get("allowedEnvVars").unwrap_or(Value::Null),
        "scope": scope,
    })
}

/// Port of `HookService._iter_hooks_from_settings`.
fn iter_hooks_from_settings(hooks_section: &Value, scope: &str, out: &mut Vec<Value>) {
    let Some(section) = hooks_section.as_object() else {
        return;
    };
    for (event, event_hooks) in section {
        let Some(arr) = event_hooks.as_array() else {
            continue;
        };
        for entry in arr {
            let Some(entry_obj) = entry.as_object() else {
                continue;
            };
            match entry_obj.get("hooks").and_then(|h| h.as_array()) {
                Some(inner) => {
                    let group_matcher = entry_obj
                        .get("matcher")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    for hook_data in inner {
                        if let Some(hd) = hook_data.as_object() {
                            if !group_matcher.is_empty() && !hd.contains_key("matcher") {
                                let mut merged = hd.clone();
                                merged.insert(
                                    "matcher".to_string(),
                                    Value::String(group_matcher.to_string()),
                                );
                                out.push(parse_hook_from_data(
                                    &Value::Object(merged),
                                    event,
                                    scope,
                                ));
                            } else {
                                out.push(parse_hook_from_data(hook_data, event, scope));
                            }
                        }
                    }
                }
                None => {
                    out.push(parse_hook_from_data(entry, event, scope));
                }
            }
        }
    }
}

fn collect_hooks(project_path: Option<&str>) -> Vec<Value> {
    let mut hooks = Vec::new();

    let user_settings_file = paths::get_claude_user_settings_file();
    if user_settings_file.exists() {
        if let Some(data) = read_settings(&user_settings_file) {
            let section = data.get("hooks").cloned().unwrap_or_else(|| json!({}));
            iter_hooks_from_settings(&section, "user", &mut hooks);
        }
    }

    if let Some(pp) = project_path {
        let project_settings_file =
            paths::get_project_settings_file(Some(pp), std::path::Path::new(""));
        if project_settings_file.exists() {
            if let Some(data) = read_settings(&project_settings_file) {
                let section = data.get("hooks").cloned().unwrap_or_else(|| json!({}));
                iter_hooks_from_settings(&section, "project", &mut hooks);
            }
        }
    }

    hooks
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/hooks
async fn list_hooks(Query(q): Query<ProjectPathQuery>) -> AppResult<Json<Value>> {
    let hooks = collect_hooks(q.project_path.as_deref());
    Ok(Json(json!({ "hooks": hooks })))
}

/// GET /api/v1/hooks/by-event/{event}  (Python: GET /hooks/{event})
async fn get_hooks_by_event(
    Path(event): Path<String>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let all_hooks = collect_hooks(q.project_path.as_deref());
    let filtered: Vec<Value> = all_hooks
        .into_iter()
        .filter(|h| h.get("event").and_then(|v| v.as_str()) == Some(event.as_str()))
        .collect();
    Ok(Json(json!({ "hooks": filtered })))
}

/// POST /api/v1/hooks
async fn create_hook(
    Query(q): Query<ProjectPathQuery>,
    Json(hook): Json<HookCreate>,
) -> AppResult<(axum::http::StatusCode, Json<Value>)> {
    if !["command", "prompt", "agent", "http"].contains(&hook.r#type.as_str()) {
        return Err(AppError::bad_request(
            "Hook type must be 'command', 'prompt', 'agent', or 'http'",
        ));
    }
    if !["user", "project"].contains(&hook.scope.as_str()) {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    if hook.r#type == "command" && hook.command.as_deref().unwrap_or("").is_empty() {
        return Err(AppError::bad_request(
            "Command is required for command-type hooks",
        ));
    }
    if hook.r#type == "prompt" && hook.prompt.as_deref().unwrap_or("").is_empty() {
        return Err(AppError::bad_request(
            "Prompt is required for prompt-type hooks",
        ));
    }
    if hook.r#type == "http" {
        if hook.url.as_deref().unwrap_or("").is_empty() {
            return Err(AppError::bad_request("URL is required for http-type hooks"));
        }
        if !hook.command.as_deref().unwrap_or("").is_empty()
            || !hook.prompt.as_deref().unwrap_or("").is_empty()
        {
            return Err(AppError::bad_request(
                "HTTP hooks should not have command or prompt fields",
            ));
        }
    }

    // Validate event type (HookService.add_hook -> ValueError -> 500).
    if !VALID_HOOK_EVENTS.contains(&hook.event.as_str()) {
        return Err(AppError::internal(format!(
            "Failed to create hook: Invalid event type: {}. Valid types: {}",
            hook.event,
            VALID_HOOK_EVENTS.join(", ")
        )));
    }

    let hook_id = gen_uuid_v4();
    let settings_file = settings_file_for_scope(&hook.scope, q.project_path.as_deref());

    if let Some(parent) = settings_file.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Err(AppError::internal(format!("Failed to create hook: {}", e)));
        }
    }

    let mut settings: Value = if settings_file.exists() {
        match read_settings(&settings_file) {
            Some(v) => v,
            None => {
                return Err(AppError::internal(
                    "Failed to create hook: invalid settings JSON".to_string(),
                ));
            }
        }
    } else {
        json!({})
    };

    if !settings.is_object() {
        settings = json!({});
    }
    let root = settings.as_object_mut().unwrap();
    if !root.contains_key("hooks") {
        root.insert("hooks".to_string(), json!({}));
    }
    let hooks_section = root.get_mut("hooks").unwrap().as_object_mut().unwrap();
    if !hooks_section.contains_key(&hook.event) {
        hooks_section.insert(hook.event.clone(), json!([]));
    }

    let mut hook_data = Map::new();
    hook_data.insert("id".to_string(), Value::String(hook_id.clone()));
    hook_data.insert("type".to_string(), Value::String(hook.r#type.clone()));
    if let Some(v) = hook.matcher.as_ref().filter(|s| !s.is_empty()) {
        hook_data.insert("matcher".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = hook.command.as_ref().filter(|s| !s.is_empty()) {
        hook_data.insert("command".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = hook.prompt.as_ref().filter(|s| !s.is_empty()) {
        hook_data.insert("prompt".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = hook.model.as_ref().filter(|s| !s.is_empty()) {
        hook_data.insert("model".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = hook.async_ {
        hook_data.insert("async".to_string(), Value::Bool(v));
    }
    if let Some(v) = hook.statusMessage.as_ref().filter(|s| !s.is_empty()) {
        hook_data.insert("statusMessage".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = hook.once {
        hook_data.insert("once".to_string(), Value::Bool(v));
    }
    if let Some(v) = hook.timeout.filter(|n| *n != 0) {
        hook_data.insert("timeout".to_string(), json!(v));
    }
    if let Some(v) = hook.url.as_ref().filter(|s| !s.is_empty()) {
        hook_data.insert("url".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = hook.headers.as_ref().filter(|m| !m.is_empty()) {
        hook_data.insert("headers".to_string(), Value::Object(v.clone()));
    }
    if let Some(v) = hook.allowedEnvVars.as_ref().filter(|a| !a.is_empty()) {
        hook_data.insert(
            "allowedEnvVars".to_string(),
            Value::Array(v.iter().cloned().map(Value::String).collect()),
        );
    }

    let matcher = hook.matcher.clone().unwrap_or_default();
    let event_groups = hooks_section
        .get_mut(&hook.event)
        .unwrap()
        .as_array_mut()
        .unwrap();

    let mut target_idx: Option<usize> = None;
    for (i, group) in event_groups.iter().enumerate() {
        if let Some(g) = group.as_object() {
            if g.contains_key("hooks")
                && g.get("matcher").and_then(|v| v.as_str()).unwrap_or("") == matcher
            {
                target_idx = Some(i);
                break;
            }
        }
    }

    let idx = match target_idx {
        Some(i) => i,
        None => {
            event_groups.push(json!({ "matcher": matcher, "hooks": [] }));
            event_groups.len() - 1
        }
    };
    event_groups[idx]
        .get_mut("hooks")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(Value::Object(hook_data));

    if let Err(e) = write_settings(&settings_file, &settings) {
        return Err(AppError::internal(format!("Failed to create hook: {}", e)));
    }

    let created = json!({
        "id": hook_id,
        "event": hook.event,
        "matcher": hook.matcher,
        "type": hook.r#type,
        "command": hook.command,
        "prompt": hook.prompt,
        "model": hook.model,
        "async": hook.async_,
        "statusMessage": hook.statusMessage,
        "once": hook.once,
        "timeout": hook.timeout,
        "url": hook.url,
        "headers": hook.headers,
        "allowedEnvVars": hook.allowedEnvVars,
        "scope": hook.scope,
    });
    Ok((axum::http::StatusCode::CREATED, Json(created)))
}

/// PUT /api/v1/hooks/{hook_id}
async fn update_hook(
    Path(hook_id): Path<String>,
    Query(q): Query<ScopeQuery>,
    Json(hook_update): Json<HookUpdate>,
) -> AppResult<Json<Value>> {
    if !["user", "project"].contains(&q.scope.as_str()) {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    if let Some(t) = hook_update.r#type.as_deref() {
        if !["command", "prompt", "agent", "http"].contains(&t) {
            return Err(AppError::bad_request(
                "Hook type must be 'command', 'prompt', 'agent', or 'http'",
            ));
        }
    }

    // HookService.update_hook: invalid event -> ValueError -> 500.
    if let Some(ev) = hook_update.event.as_deref() {
        if !ev.is_empty() && !VALID_HOOK_EVENTS.contains(&ev) {
            return Err(AppError::internal(format!(
                "Failed to update hook: Invalid event type: {}. Valid types: {}",
                ev,
                VALID_HOOK_EVENTS.join(", ")
            )));
        }
    }

    let settings_file = settings_file_for_scope(&q.scope, q.project_path.as_deref());
    if !settings_file.exists() {
        return Err(AppError::not_found(format!(
            "Hook with ID {} not found in {} scope",
            hook_id, q.scope
        )));
    }

    let mut settings = match read_settings(&settings_file) {
        Some(v) => v,
        None => {
            return Err(AppError::internal(
                "Failed to update hook: invalid settings JSON".to_string(),
            ));
        }
    };

    let new_event = hook_update
        .event
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let mut updated_hook: Option<Value> = None;
    let mut moved_hook: Option<Value> = None;
    let mut move_target_event: Option<String> = None;

    if let Some(hooks_section) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        let event_names: Vec<String> = hooks_section.keys().cloned().collect();
        'outer: for event in event_names {
            let Some(event_hooks) = hooks_section.get_mut(&event).and_then(|v| v.as_array_mut())
            else {
                continue;
            };

            let mut gi = 0;
            while gi < event_hooks.len() {
                let is_obj = event_hooks[gi].is_object();
                if !is_obj {
                    gi += 1;
                    continue;
                }
                let is_nested = event_hooks[gi]
                    .get("hooks")
                    .map(|h| h.is_array())
                    .unwrap_or(false);

                let inner_len = if is_nested {
                    event_hooks[gi]
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0)
                } else {
                    1
                };

                let mut found_i: Option<usize> = None;
                for i in 0..inner_len {
                    let hd = if is_nested {
                        &event_hooks[gi]["hooks"][i]
                    } else {
                        &event_hooks[gi]
                    };
                    if hd.get("id").and_then(|v| v.as_str()) == Some(hook_id.as_str()) {
                        found_i = Some(i);
                        break;
                    }
                }

                let Some(i) = found_i else {
                    gi += 1;
                    continue;
                };

                // Apply updates onto the located hook_data in place.
                {
                    let hd: &mut Value = if is_nested {
                        &mut event_hooks[gi]["hooks"][i]
                    } else {
                        &mut event_hooks[gi]
                    };
                    let m = hd.as_object_mut().unwrap();
                    if let Some(v) = &hook_update.matcher {
                        m.insert("matcher".into(), Value::String(v.clone()));
                    }
                    if let Some(v) = &hook_update.r#type {
                        m.insert("type".into(), Value::String(v.clone()));
                    }
                    if let Some(v) = &hook_update.command {
                        m.insert("command".into(), Value::String(v.clone()));
                    }
                    if let Some(v) = &hook_update.prompt {
                        m.insert("prompt".into(), Value::String(v.clone()));
                    }
                    if let Some(v) = &hook_update.model {
                        m.insert("model".into(), Value::String(v.clone()));
                    }
                    if let Some(v) = hook_update.async_ {
                        m.insert("async".into(), Value::Bool(v));
                    }
                    if let Some(v) = &hook_update.statusMessage {
                        m.insert("statusMessage".into(), Value::String(v.clone()));
                    }
                    if let Some(v) = hook_update.once {
                        m.insert("once".into(), Value::Bool(v));
                    }
                    if let Some(v) = hook_update.timeout {
                        m.insert("timeout".into(), json!(v));
                    }
                    if let Some(v) = &hook_update.url {
                        m.insert("url".into(), Value::String(v.clone()));
                    }
                    if let Some(v) = &hook_update.headers {
                        m.insert("headers".into(), Value::Object(v.clone()));
                    }
                    if let Some(v) = &hook_update.allowedEnvVars {
                        m.insert(
                            "allowedEnvVars".into(),
                            Value::Array(v.iter().cloned().map(Value::String).collect()),
                        );
                    }
                }

                // Handle event change.
                let event_changes = new_event.as_ref().map(|ne| ne != &event).unwrap_or(false);

                if event_changes {
                    let ne = new_event.clone().unwrap();
                    let hook_data = if is_nested {
                        let inner = event_hooks[gi]["hooks"].as_array_mut().unwrap();
                        let hd = inner.remove(i);
                        if inner.is_empty() {
                            event_hooks.remove(gi);
                        }
                        hd
                    } else {
                        event_hooks.remove(gi)
                    };
                    updated_hook = Some(parse_hook_from_data(&hook_data, &ne, &q.scope));
                    moved_hook = Some(hook_data);
                    move_target_event = Some(ne);
                    break 'outer;
                } else {
                    let hd = if is_nested {
                        &event_hooks[gi]["hooks"][i]
                    } else {
                        &event_hooks[gi]
                    };
                    updated_hook = Some(parse_hook_from_data(hd, &event, &q.scope));
                    break 'outer;
                }
            }
        }

        // Re-insert a moved hook into its new event group.
        if let (Some(hd), Some(target)) = (moved_hook, move_target_event) {
            if !hooks_section.contains_key(&target) {
                hooks_section.insert(target.clone(), json!([]));
            }
            hooks_section
                .get_mut(&target)
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(json!({ "matcher": "", "hooks": [hd] }));
        }
    }

    let Some(result) = updated_hook else {
        return Err(AppError::not_found(format!(
            "Hook with ID {} not found in {} scope",
            hook_id, q.scope
        )));
    };

    if let Err(e) = write_settings(&settings_file, &settings) {
        return Err(AppError::internal(format!("Failed to update hook: {}", e)));
    }

    Ok(Json(result))
}

/// DELETE /api/v1/hooks/{hook_id}
async fn delete_hook(
    Path(hook_id): Path<String>,
    Query(q): Query<ScopeQuery>,
) -> AppResult<axum::http::StatusCode> {
    if !["user", "project"].contains(&q.scope.as_str()) {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }

    let settings_file = settings_file_for_scope(&q.scope, q.project_path.as_deref());
    if !settings_file.exists() {
        return Err(AppError::not_found(format!(
            "Hook with ID {} not found in {} scope",
            hook_id, q.scope
        )));
    }

    let mut settings = match read_settings(&settings_file) {
        Some(v) => v,
        None => {
            return Err(AppError::internal(
                "Failed to delete hook: invalid settings JSON".to_string(),
            ));
        }
    };

    let mut removed = false;

    if let Some(hooks_section) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        let event_names: Vec<String> = hooks_section.keys().cloned().collect();
        'outer: for event in event_names {
            let Some(event_hooks) = hooks_section.get_mut(&event).and_then(|v| v.as_array_mut())
            else {
                continue;
            };

            let mut gi = 0;
            while gi < event_hooks.len() {
                if !event_hooks[gi].is_object() {
                    gi += 1;
                    continue;
                }
                let is_nested = event_hooks[gi]
                    .get("hooks")
                    .map(|h| h.is_array())
                    .unwrap_or(false);

                if is_nested {
                    let inner = event_hooks[gi]["hooks"].as_array_mut().unwrap();
                    let mut found: Option<usize> = None;
                    for (i, hd) in inner.iter().enumerate() {
                        if hd.get("id").and_then(|v| v.as_str()) == Some(hook_id.as_str()) {
                            found = Some(i);
                            break;
                        }
                    }
                    if let Some(i) = found {
                        inner.remove(i);
                        if inner.is_empty() {
                            event_hooks.remove(gi);
                        }
                        removed = true;
                        break 'outer;
                    }
                } else if event_hooks[gi].get("id").and_then(|v| v.as_str())
                    == Some(hook_id.as_str())
                {
                    event_hooks.remove(gi);
                    removed = true;
                    break 'outer;
                }
                gi += 1;
            }
        }
    }

    if !removed {
        return Err(AppError::not_found(format!(
            "Hook with ID {} not found in {} scope",
            hook_id, q.scope
        )));
    }

    if let Err(e) = write_settings(&settings_file, &settings) {
        return Err(AppError::internal(format!("Failed to delete hook: {}", e)));
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}
