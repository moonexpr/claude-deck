// PORTED: command_service.py + api/v1/commands.py
//
// Faithful port of `backend_python/app/{services/command_service.py,
// api/v1/commands.py}`. Route paths/methods, query/body field names and
// response JSON shapes match the Python module exactly (the unchanged
// frontend was built against them). Python has no path-traversal guard for
// commands (it builds `base_dir / path` directly) — behavior matched 1:1.

use axum::{
    Router,
    extract::{Json, Path as AxumPath, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::fileio::read_json_file;
use crate::paths;

pub fn router() -> Router<ApiState> {
    // Paths mirror Python's `APIRouter(prefix="/commands")` exactly.
    // Python uses `{scope}/{path:path}` (path captures slashes) — axum's
    // `{*path}` wildcard is the equivalent.
    Router::new()
        .route("/", get(list_commands).post(create_command))
        .route(
            "/{scope}/{*path}",
            get(get_command).put(update_command).delete(delete_command),
        )
}

#[derive(Deserialize)]
struct ProjectPathQuery {
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct SlashCommandCreate {
    name: String,
    scope: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
    content: String,
}

#[derive(Deserialize)]
struct SlashCommandUpdate {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    content: Option<String>,
}

// ---- frontmatter -----------------------------------------------------------

/// Port of `CommandService._parse_frontmatter`.
///
/// Python regex: `^---\s*\n(.*?)\n---\s*\n(.*)$` with DOTALL. Returns
/// (metadata, content-without-frontmatter). On no match: ({}, content.strip()).
/// YAML parse here is restricted to the flat shape this service ever writes:
/// scalar `key: value` and block sequences (`- item`). `yaml.safe_load`
/// failure → `{}`.
fn parse_frontmatter(content: &str) -> (Map<String, Value>, String) {
    if let Some((fm, body)) = split_frontmatter(content) {
        let metadata = parse_simple_yaml(&fm).unwrap_or_default();
        (metadata, body.trim().to_string())
    } else {
        (Map::new(), content.trim().to_string())
    }
}

/// Equivalent of the `^---\s*\n(.*?)\n---\s*\n(.*)$` DOTALL match.
fn split_frontmatter(content: &str) -> Option<(String, String)> {
    let rest = content.strip_prefix("---")?;
    // `---\s*\n` : optional spaces/tabs then a newline.
    let nl = rest.find('\n')?;
    if !rest[..nl].trim().is_empty() {
        return None;
    }
    let after_open = &rest[nl + 1..];

    // Find a line that is `---\s*` (closing delimiter), non-greedy (first one).
    let mut idx = 0usize;
    loop {
        let line_end = after_open[idx..].find('\n').map(|p| idx + p);
        let (line, next_idx) = match line_end {
            Some(e) => (&after_open[idx..e], e + 1),
            None => return None, // no closing `---\n` → no match
        };
        if let Some(rem) = line.strip_prefix("---")
            && rem.trim().is_empty() {
                let yaml_content = after_open[..idx.saturating_sub(1)].to_string();
                let body = after_open[next_idx..].to_string();
                return Some((yaml_content, body));
            }
        idx = next_idx;
    }
}

/// Minimal `yaml.safe_load` for the flat metadata this service emits:
/// `key: scalar` and `key:` followed by `- item` block sequences. Anything
/// outside that shape returns `None` (→ Python's `except YAMLError: {}`).
fn parse_simple_yaml(text: &str) -> Option<Map<String, Value>> {
    let mut map = Map::new();
    let mut lines = text.lines().peekable();
    while let Some(raw) = lines.next() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        // Top-level key must be unindented.
        if line.starts_with([' ', '\t']) {
            return None;
        }
        let colon = line.find(':')?;
        let key = line[..colon].trim().to_string();
        if key.is_empty() {
            return None;
        }
        let value_part = line[colon + 1..].trim();
        if value_part.is_empty() {
            // Possible block sequence on following indented `-` lines.
            let mut seq: Vec<Value> = Vec::new();
            let mut saw_item = false;
            while let Some(peek) = lines.peek() {
                let t = peek.trim();
                if t.is_empty() {
                    lines.next();
                    continue;
                }
                if !peek.starts_with([' ', '\t']) {
                    break;
                }
                let item = t.strip_prefix('-')?.trim();
                seq.push(Value::String(unquote_scalar(item)));
                saw_item = true;
                lines.next();
            }
            if saw_item {
                map.insert(key, Value::Array(seq));
            } else {
                map.insert(key, Value::Null);
            }
        } else {
            map.insert(key, parse_scalar(value_part));
        }
    }
    Some(map)
}

fn parse_scalar(s: &str) -> Value {
    match s {
        "null" | "~" | "" => Value::Null,
        "true" | "True" => Value::Bool(true),
        "false" | "False" => Value::Bool(false),
        _ => {
            if let Ok(i) = s.parse::<i64>() {
                return Value::Number(i.into());
            }
            if let Ok(f) = s.parse::<f64>()
                && let Some(n) = serde_json::Number::from_f64(f) {
                    return Value::Number(n);
                }
            Value::String(unquote_scalar(s))
        }
    }
}

fn unquote_scalar(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        let b = s.as_bytes();
        if (b[0] == b'"' && b[s.len() - 1] == b'"') || (b[0] == b'\'' && b[s.len() - 1] == b'\'') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

/// Port of `CommandService._build_frontmatter`. Empty metadata → "".
/// Mirrors `yaml.dump(default_flow_style=False, allow_unicode=True)` for the
/// flat metadata shape (string scalars and string lists), keys sorted (PyYAML
/// default `sort_keys=True`).
fn build_frontmatter(metadata: &Map<String, Value>) -> String {
    if metadata.is_empty() {
        return String::new();
    }
    let mut keys: Vec<&String> = metadata.keys().collect();
    keys.sort();
    let mut yaml = String::new();
    for k in keys {
        match &metadata[k] {
            Value::Array(items) => {
                yaml.push_str(k);
                yaml.push_str(":\n");
                for it in items {
                    yaml.push_str("- ");
                    yaml.push_str(&yaml_scalar(it));
                    yaml.push('\n');
                }
            }
            v => {
                yaml.push_str(k);
                yaml.push_str(": ");
                yaml.push_str(&yaml_scalar(v));
                yaml.push('\n');
            }
        }
    }
    format!("---\n{}---\n\n", yaml)
}

fn yaml_scalar(v: &Value) -> String {
    match v {
        Value::String(s) => yaml_quote_if_needed(s),
        Value::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

/// Conservative quoting: PyYAML emits bare scalars when safe and single-quotes
/// otherwise. Quote when empty, has leading/trailing space, or contains a
/// character that would change YAML structure for this flat shape.
fn yaml_quote_if_needed(s: &str) -> String {
    let needs = s.is_empty()
        || s != s.trim()
        || s.starts_with([
            '!', '&', '*', '-', '?', '|', '>', '%', '@', '`', '"', '\'', '#', ',', '[', ']', '{',
            '}',
        ])
        || s.contains(": ")
        || s.ends_with(':')
        || s.contains('\n')
        || matches!(
            s,
            "null" | "Null" | "NULL" | "~" | "true" | "True" | "false" | "False" | "yes" | "no"
        );
    if needs {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        s.to_string()
    }
}

// ---- name <-> path ---------------------------------------------------------

/// Port of `CommandService._path_to_name`:
/// `commands/tools/analyze.md` (relative to base) -> `tools:analyze`.
fn path_to_name(file_path: &Path, base_dir: &Path) -> String {
    let rel = file_path.strip_prefix(base_dir).unwrap_or(file_path);
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if let Some(last) = parts.last_mut() {
        *last = last.replace(".md", "");
    }
    parts.join(":")
}

/// Port of `CommandService._name_to_path`:
/// `tools:analyze` -> `<base>/tools/analyze.md`.
fn name_to_path(name: &str, base_dir: &Path) -> PathBuf {
    let mut parts: Vec<String> = name.split(':').map(|s| s.to_string()).collect();
    if let Some(last) = parts.last_mut() {
        *last = format!("{}.md", last);
    }
    let mut p = base_dir.to_path_buf();
    for part in parts {
        p.push(part);
    }
    p
}

// ---- allowed-tools normalization ------------------------------------------

/// Python: `if isinstance(allowed_tools, str): [t.strip() for t in split(",")]`.
/// list stays as-is. None -> None.
fn normalize_allowed_tools(meta: &Map<String, Value>) -> Value {
    match meta.get("allowed-tools") {
        Some(Value::String(s)) => Value::Array(
            s.split(',')
                .map(|t| Value::String(t.trim().to_string()))
                .collect(),
        ),
        Some(Value::Array(a)) => Value::Array(a.clone()),
        Some(other) => other.clone(),
        None => Value::Null,
    }
}

/// Raw (no string-splitting) allowed-tools, as used by `_scan_commands_dir`.
fn raw_allowed_tools(meta: &Map<String, Value>) -> Value {
    meta.get("allowed-tools").cloned().unwrap_or(Value::Null)
}

fn description_value(meta: &Map<String, Value>) -> Value {
    meta.get("description").cloned().unwrap_or(Value::Null)
}

fn command_json(
    name: String,
    path: String,
    scope: String,
    description: Value,
    allowed_tools: Value,
    content: String,
) -> Value {
    json!({
        "name": name,
        "path": path,
        "scope": scope,
        "description": description,
        "allowed_tools": allowed_tools,
        "content": content,
    })
}

// ---- scanning --------------------------------------------------------------

fn rglob_md(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn glob_md_one_level(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Port of `CommandService._scan_commands_dir`.
fn scan_commands_dir(base_dir: &Path, scope: &str) -> Vec<Value> {
    let mut commands = Vec::new();
    for md_file in rglob_md(base_dir) {
        let Ok(content) = std::fs::read_to_string(&md_file) else {
            continue;
        };
        let (metadata, markdown_content) = parse_frontmatter(&content);
        let command_name = path_to_name(&md_file, base_dir);
        let relative_path = md_file
            .strip_prefix(base_dir)
            .unwrap_or(&md_file)
            .to_string_lossy()
            .into_owned();
        commands.push(command_json(
            command_name,
            relative_path,
            scope.to_string(),
            description_value(&metadata),
            raw_allowed_tools(&metadata),
            markdown_content,
        ));
    }
    commands
}

/// Port of `CommandService._scan_plugin_commands`.
fn scan_plugin_commands() -> Vec<Value> {
    let mut commands = Vec::new();

    let installed_file = paths::get_claude_user_plugins_dir().join("installed_plugins.json");
    if !installed_file.exists() {
        return commands;
    }
    let Some(installed_data) = read_json_file(&installed_file) else {
        return commands;
    };
    let Some(plugins) = installed_data.get("plugins").and_then(|v| v.as_object()) else {
        return commands;
    };

    let settings_file = paths::get_claude_user_settings_file();
    let settings_data = read_json_file(&settings_file).unwrap_or_else(|| json!({}));
    let enabled_plugins = settings_data
        .get("enabledPlugins")
        .cloned()
        .unwrap_or_else(|| json!({}));

    for (plugin_key, install_list) in plugins {
        let enabled = enabled_plugins
            .get(plugin_key)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }
        let arr = match install_list.as_array() {
            Some(a) if !a.is_empty() => a,
            _ => continue,
        };
        let install_path = match arr[0].get("installPath").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };

        let plugin_dir = PathBuf::from(install_path);
        let commands_dir = plugin_dir.join("commands");
        if !commands_dir.exists() {
            continue;
        }

        let plugin_name = plugin_key.split('@').next().unwrap_or(plugin_key);
        let scope = format!("plugin:{}", plugin_name);

        for md_file in glob_md_one_level(&commands_dir) {
            let Ok(content) = std::fs::read_to_string(&md_file) else {
                continue;
            };
            let (metadata, markdown_content) = parse_frontmatter(&content);
            let command_name = md_file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rel = md_file
                .strip_prefix(&plugin_dir)
                .unwrap_or(&md_file)
                .to_string_lossy()
                .into_owned();
            commands.push(command_json(
                command_name,
                rel,
                scope.clone(),
                description_value(&metadata),
                normalize_allowed_tools(&metadata),
                markdown_content,
            ));
        }
    }
    commands
}

/// Port of `CommandService.list_commands`.
fn list_commands_impl(project_path: Option<&str>, cwd_fallback: &Path) -> Vec<Value> {
    let mut commands = Vec::new();

    let user_commands_dir = paths::get_claude_user_commands_dir();
    if user_commands_dir.exists() {
        commands.extend(scan_commands_dir(&user_commands_dir, "user"));
    }

    if let Some(pp) = project_path {
        let project_commands_dir = paths::get_project_commands_dir(Some(pp), cwd_fallback);
        if project_commands_dir.exists() {
            commands.extend(scan_commands_dir(&project_commands_dir, "project"));
        }
    }

    commands.extend(scan_plugin_commands());
    commands
}

/// Port of `CommandService._get_plugin_command`.
fn get_plugin_command(plugin_name: &str, path: &str) -> Option<Value> {
    let installed_file = paths::get_claude_user_plugins_dir().join("installed_plugins.json");
    if !installed_file.exists() {
        return None;
    }
    let installed_data = read_json_file(&installed_file)?;
    let plugins = installed_data.get("plugins")?.as_object()?;

    for (plugin_key, install_list) in plugins {
        let key_plugin_name = plugin_key.split('@').next().unwrap_or(plugin_key);
        if key_plugin_name != plugin_name {
            continue;
        }
        let arr = match install_list.as_array() {
            Some(a) if !a.is_empty() => a,
            _ => continue,
        };
        let install_path = match arr[0].get("installPath").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let plugin_dir = PathBuf::from(install_path);
        let file_path = plugin_dir.join(path);
        if !file_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&file_path).ok()?;
        let (metadata, markdown_content) = parse_frontmatter(&content);
        let command_name = file_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Some(command_json(
            command_name,
            path.to_string(),
            format!("plugin:{}", plugin_name),
            description_value(&metadata),
            normalize_allowed_tools(&metadata),
            markdown_content,
        ));
    }
    None
}

/// Port of `CommandService.get_command`.
fn get_command_impl(
    scope: &str,
    path: &str,
    project_path: Option<&str>,
    cwd_fallback: &Path,
) -> Option<Value> {
    let base_dir: PathBuf;
    if scope == "user" {
        base_dir = paths::get_claude_user_commands_dir();
    } else if scope == "project" {
        base_dir = paths::get_project_commands_dir(project_path, cwd_fallback);
    } else if let Some(plugin_name) = scope.strip_prefix("plugin:") {
        return get_plugin_command(plugin_name, path);
    } else {
        return None;
    }

    let file_path = base_dir.join(path);
    if !file_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&file_path).ok()?;
    let (metadata, markdown_content) = parse_frontmatter(&content);
    let command_name = path_to_name(&file_path, &base_dir);
    Some(command_json(
        command_name,
        path.to_string(),
        scope.to_string(),
        description_value(&metadata),
        normalize_allowed_tools(&metadata),
        markdown_content,
    ))
}

// ---- handlers --------------------------------------------------------------

/// GET /api/v1/commands
async fn list_commands(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let commands = list_commands_impl(q.project_path.as_deref(), &state.cwd_fallback);
    Ok(Json(json!({ "commands": commands })))
}

/// GET /api/v1/commands/{scope}/{path}
async fn get_command(
    State(state): State<ApiState>,
    AxumPath((scope, path)): AxumPath<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    if scope != "user" && scope != "project" && !scope.starts_with("plugin:") {
        return Err(AppError::bad_request(format!(
            "Invalid scope: {}. Must be 'user', 'project', or 'plugin:name'",
            scope
        )));
    }
    match get_command_impl(
        &scope,
        &path,
        q.project_path.as_deref(),
        &state.cwd_fallback,
    ) {
        Some(cmd) => Ok(Json(cmd)),
        None => Err(AppError::not_found(format!(
            "Command not found: {}/{}",
            scope, path
        ))),
    }
}

/// POST /api/v1/commands  (201)
async fn create_command(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
    Json(command): Json<SlashCommandCreate>,
) -> AppResult<impl IntoResponse> {
    if command.scope != "user" && command.scope != "project" {
        return Err(AppError::bad_request(format!(
            "Invalid scope: {}. Must be 'user' or 'project'",
            command.scope
        )));
    }

    let base_dir = if command.scope == "user" {
        paths::get_claude_user_commands_dir()
    } else {
        paths::get_project_commands_dir(q.project_path.as_deref(), &state.cwd_fallback)
    };

    let file_path = name_to_path(&command.name, &base_dir);

    if file_path.exists() {
        return Err(AppError::bad_request(format!(
            "Command already exists: {}",
            command.name
        )));
    }

    if let Some(parent) = file_path.parent() {
        paths::ensure_directory_exists(parent);
    }

    let mut metadata = Map::new();
    if let Some(desc) = command.description.as_ref()
        && !desc.is_empty() {
            metadata.insert("description".into(), Value::String(desc.clone()));
        }
    if let Some(tools) = command.allowed_tools.as_ref()
        && !tools.is_empty() {
            metadata.insert(
                "allowed-tools".into(),
                Value::Array(tools.iter().cloned().map(Value::String).collect()),
            );
        }

    let frontmatter = build_frontmatter(&metadata);
    let full_content = format!("{}{}", frontmatter, command.content);

    if let Err(e) = std::fs::write(&file_path, &full_content) {
        return Err(AppError::internal(format!(
            "Failed to create command: {}",
            e
        )));
    }

    let relative_path = file_path
        .strip_prefix(&base_dir)
        .unwrap_or(&file_path)
        .to_string_lossy()
        .into_owned();

    let body = command_json(
        command.name.clone(),
        relative_path,
        command.scope.clone(),
        command
            .description
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
        command
            .allowed_tools
            .clone()
            .map(|t| Value::Array(t.into_iter().map(Value::String).collect()))
            .unwrap_or(Value::Null),
        command.content.clone(),
    );

    Ok((StatusCode::CREATED, Json(body)))
}

/// PUT /api/v1/commands/{scope}/{path}
async fn update_command(
    State(state): State<ApiState>,
    AxumPath((scope, path)): AxumPath<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
    Json(command): Json<SlashCommandUpdate>,
) -> AppResult<Json<Value>> {
    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request(format!(
            "Invalid scope: {}. Must be 'user' or 'project'",
            scope
        )));
    }

    let base_dir = if scope == "user" {
        paths::get_claude_user_commands_dir()
    } else {
        paths::get_project_commands_dir(q.project_path.as_deref(), &state.cwd_fallback)
    };

    let file_path = base_dir.join(&path);
    if !file_path.exists() {
        return Err(AppError::not_found(format!(
            "Command not found: {}/{}",
            scope, path
        )));
    }

    // Python wraps the read/parse/write in try/except → None → 404.
    let result: Option<Value> = (|| {
        let existing_content = std::fs::read_to_string(&file_path).ok()?;
        let (mut metadata, mut markdown_content) = parse_frontmatter(&existing_content);

        if let Some(desc) = command.description.as_ref() {
            metadata.insert("description".into(), Value::String(desc.clone()));
        }
        if let Some(tools) = command.allowed_tools.as_ref() {
            metadata.insert(
                "allowed-tools".into(),
                Value::Array(tools.iter().cloned().map(Value::String).collect()),
            );
        }
        if let Some(c) = command.content.as_ref() {
            markdown_content = c.clone();
        }

        let frontmatter = build_frontmatter(&metadata);
        let full_content = format!("{}{}", frontmatter, markdown_content);
        std::fs::write(&file_path, &full_content).ok()?;

        let command_name = path_to_name(&file_path, &base_dir);
        Some(command_json(
            command_name,
            path.clone(),
            scope.clone(),
            description_value(&metadata),
            raw_allowed_tools(&metadata),
            markdown_content,
        ))
    })();

    match result {
        Some(cmd) => Ok(Json(cmd)),
        None => Err(AppError::not_found(format!(
            "Command not found: {}/{}",
            scope, path
        ))),
    }
}

/// DELETE /api/v1/commands/{scope}/{path}  (204)
async fn delete_command(
    State(state): State<ApiState>,
    AxumPath((scope, path)): AxumPath<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<impl IntoResponse> {
    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request(format!(
            "Invalid scope: {}. Must be 'user' or 'project'",
            scope
        )));
    }

    let base_dir = if scope == "user" {
        paths::get_claude_user_commands_dir()
    } else {
        paths::get_project_commands_dir(q.project_path.as_deref(), &state.cwd_fallback)
    };

    let file_path = base_dir.join(&path);
    if !file_path.exists() {
        return Err(AppError::not_found(format!(
            "Command not found: {}/{}",
            scope, path
        )));
    }

    if std::fs::remove_file(&file_path).is_err() {
        // Python: unlink failure → False → 404.
        return Err(AppError::not_found(format!(
            "Command not found: {}/{}",
            scope, path
        )));
    }

    Ok(StatusCode::NO_CONTENT)
}
