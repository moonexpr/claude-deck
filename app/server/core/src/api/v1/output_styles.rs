// PORTED: output_style_service.py + api/v1/output_styles.py

use axum::{
    extract::{Json, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::paths;

pub fn router() -> Router<ApiState> {
    // Python: APIRouter(prefix="/output-styles"); nested under /output-styles.
    Router::new()
        .route("/", get(list_output_styles).post(create_output_style))
        .route(
            "/{scope}/{name}",
            get(get_output_style)
                .put(update_output_style)
                .delete(delete_output_style),
        )
}

#[derive(Deserialize)]
struct ProjectPathQuery {
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct OutputStyleCreate {
    name: String,
    scope: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    keep_coding_instructions: bool,
    content: String,
}

#[derive(Deserialize)]
struct OutputStyleUpdate {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    keep_coding_instructions: Option<bool>,
    #[serde(default)]
    content: Option<String>,
}

// ---- frontmatter helpers ----------------------------------------------------
//
// Python uses PyYAML for the `---` frontmatter of output-style `.md` files.
// The metadata is always a flat scalar map (`name`, `description`,
// `keep-coding-instructions`). We replicate `yaml.safe_load` for that flat
// shape and `yaml.dump(..., default_flow_style=False, allow_unicode=True)`
// for the values that actually occur. NEEDS-DEP `serde_yaml` for byte-exact
// fidelity on pathological inputs (embedded newlines, tabs, control chars in
// `description`); those degrade gracefully rather than matching PyYAML's
// folded/double-quoted forms.

#[derive(Clone)]
enum Meta {
    Str(String),
    Bool(bool),
}

/// Port of `_parse_frontmatter`. Returns (metadata, content-without-fm).
fn parse_frontmatter(content: &str) -> (Vec<(String, Meta)>, String) {
    // Python regex: ^---\s*\n(.*?)\n---\s*\n(.*)$  with DOTALL.
    let bytes = content;
    if let Some(rest) = strip_opening_fence(bytes) {
        // rest begins right after "---<ws>\n"
        if let Some((yaml_block, body)) = split_closing_fence(rest) {
            let meta = parse_flat_yaml(yaml_block);
            return (meta, body.trim().to_string());
        }
    }
    (Vec::new(), content.trim().to_string())
}

/// Match `^---[ \t]*\n` and return the remainder.
fn strip_opening_fence(s: &str) -> Option<&str> {
    let after = s.strip_prefix("---")?;
    // \s* up to and including the first newline (PyYAML \s also matches \r etc.)
    let mut idx = 0;
    let b = after.as_bytes();
    while idx < b.len() {
        match b[idx] {
            b'\n' => return Some(&after[idx + 1..]),
            c if (c as char).is_whitespace() => idx += 1,
            _ => return None,
        }
    }
    None
}

/// Find a line that is `---\s*` and split into (yaml_before, body_after).
/// Mirrors the non-greedy `(.*?)\n---\s*\n(.*)` capture.
fn split_closing_fence(s: &str) -> Option<(&str, &str)> {
    let mut search_from = 0;
    loop {
        let rel = s[search_from..].find("\n---")?;
        let fence_start = search_from + rel; // index of the '\n'
        let yaml_block = &s[..fence_start];
        let after_dashes = &s[fence_start + 4..]; // skip "\n---"
        // remaining whitespace then a newline
        let bb = after_dashes.as_bytes();
        let mut i = 0;
        let mut ok = true;
        while i < bb.len() {
            match bb[i] {
                b'\n' => break,
                c if (c as char).is_whitespace() => i += 1,
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && i < bb.len() && bb[i] == b'\n' {
            let body = &after_dashes[i + 1..];
            return Some((yaml_block, body));
        }
        search_from = fence_start + 1;
    }
}

/// Parse a flat `key: value` YAML block (the only shape these files use).
fn parse_flat_yaml(block: &str) -> Vec<(String, Meta)> {
    let mut out = Vec::new();
    for line in block.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(colon) = trimmed.find(':') else {
            continue;
        };
        let key = trimmed[..colon].trim().to_string();
        let raw = trimmed[colon + 1..].trim();
        if key.is_empty() {
            continue;
        }
        out.push((key, parse_scalar(raw)));
    }
    out
}

fn parse_scalar(raw: &str) -> Meta {
    if raw.is_empty() {
        return Meta::Str(String::new());
    }
    // single-quoted
    if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        let inner = &raw[1..raw.len() - 1];
        return Meta::Str(inner.replace("''", "'"));
    }
    // double-quoted
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        let inner = &raw[1..raw.len() - 1];
        return Meta::Str(unescape_double(inner));
    }
    match raw {
        "true" | "True" | "TRUE" => Meta::Bool(true),
        "false" | "False" | "FALSE" => Meta::Bool(false),
        _ => Meta::Str(raw.to_string()),
    }
}

fn unescape_double(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn meta_get_str(meta: &[(String, Meta)], key: &str) -> Option<String> {
    meta.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
        Meta::Str(s) => Some(s.clone()),
        Meta::Bool(b) => Some(b.to_string()),
    })
}

fn meta_get_bool(meta: &[(String, Meta)], key: &str) -> bool {
    meta.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| match v {
            Meta::Bool(b) => *b,
            Meta::Str(s) => !s.is_empty(),
        })
        .unwrap_or(false)
}

/// Port of `_build_frontmatter`. Empty metadata -> "".
/// Keys are emitted PyYAML-style: sorted, block flow, single-quoting scalars
/// when required (replicating `yaml.dump(default_flow_style=False)`).
fn build_frontmatter(mut metadata: Vec<(String, Meta)>) -> String {
    if metadata.is_empty() {
        return String::new();
    }
    metadata.sort_by(|a, b| a.0.cmp(&b.0));
    let mut yaml = String::new();
    for (k, v) in &metadata {
        match v {
            Meta::Bool(b) => {
                yaml.push_str(&format!("{}: {}\n", k, if *b { "true" } else { "false" }));
            }
            Meta::Str(s) => {
                yaml.push_str(&format!("{}: {}\n", k, emit_yaml_scalar(s)));
            }
        }
    }
    format!("---\n{}---\n\n", yaml)
}

/// Replicate PyYAML's plain-vs-single-quoted decision for a mapping value.
fn emit_yaml_scalar(s: &str) -> String {
    if needs_quoting(s) {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        s.to_string()
    }
}

fn needs_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    // PyYAML quotes if it would otherwise parse as a non-string scalar,
    // contains indicator chars, or has leading/trailing space.
    let lower = s.to_ascii_lowercase();
    const RESERVED: [&str; 11] = [
        "true", "false", "null", "yes", "no", "on", "off", "~", ".inf", "-.inf", ".nan",
    ];
    if RESERVED.contains(&lower.as_str()) {
        return true;
    }
    // numeric-looking (int / float / date-ish) -> quoted
    if s.parse::<f64>().is_ok() || s.parse::<i64>().is_ok() {
        return true;
    }
    if s.chars().next().map(|c| c == ' ').unwrap_or(false)
        || s.chars().last().map(|c| c == ' ').unwrap_or(false)
    {
        return true;
    }
    let first = s.chars().next().unwrap();
    if "!&*?|>%@`\"'#-[]{},".contains(first) {
        return true;
    }
    // ": " or trailing ":" / " #" force quoting
    if s.contains(": ") || s.ends_with(':') || s.contains(" #") {
        return true;
    }
    // control chars / tabs / newlines: cannot be a plain scalar
    if s.chars().any(|c| c.is_control()) {
        return true;
    }
    // looks like a date (YYYY-MM-DD)
    if is_date_like(s) {
        return true;
    }
    false
}

fn is_date_like(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().all(|c| c.is_ascii_digit())
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit())
}

fn base_dir_for(scope: &str, project_path: Option<&str>, cwd_fallback: &std::path::Path) -> PathBuf {
    if scope == "user" {
        paths::get_claude_user_output_styles_dir()
    } else {
        paths::get_project_output_styles_dir(project_path, cwd_fallback)
    }
}

fn style_json(name: &str, scope: &str, description: Option<&str>, kci: bool, content: &str) -> Value {
    json!({
        "name": name,
        "scope": scope,
        "description": description,
        "keep_coding_instructions": kci,
        "content": content,
    })
}

fn read_style(base: &std::path::Path, scope: &str, name: &str) -> Option<Value> {
    let file_path = base.join(format!("{}.md", name));
    if !file_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&file_path).ok()?;
    let (meta, md) = parse_frontmatter(&content);
    Some(style_json(
        &meta_get_str(&meta, "name").unwrap_or_else(|| name.to_string()),
        scope,
        meta_get_str(&meta, "description").as_deref(),
        meta_get_bool(&meta, "keep-coding-instructions"),
        &md,
    ))
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/output-styles
async fn list_output_styles(Query(q): Query<ProjectPathQuery>) -> AppResult<Json<Value>> {
    let mut styles: Vec<Value> = Vec::new();

    let user_dir = paths::get_claude_user_output_styles_dir();
    if user_dir.exists() {
        scan_styles_dir(&user_dir, "user", &mut styles);
    }

    if let Some(pp) = q.project_path.as_deref() {
        let proj_dir = paths::get_project_output_styles_dir(Some(pp), std::path::Path::new(""));
        if proj_dir.exists() {
            scan_styles_dir(&proj_dir, "project", &mut styles);
        }
    }

    Ok(Json(json!({ "output_styles": styles })))
}

/// Port of `_scan_styles_dir`: non-recursive `*.md`.
fn scan_styles_dir(base: &std::path::Path, scope: &str, out: &mut Vec<Value>) {
    let Ok(rd) = std::fs::read_dir(base) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let style_name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (meta, md) = parse_frontmatter(&content);
        out.push(style_json(
            &meta_get_str(&meta, "name").unwrap_or_else(|| style_name.clone()),
            scope,
            meta_get_str(&meta, "description").as_deref(),
            meta_get_bool(&meta, "keep-coding-instructions"),
            &md,
        ));
    }
}

/// GET /api/v1/output-styles/{scope}/{name}
async fn get_output_style(
    State(state): State<ApiState>,
    Path((scope, name)): Path<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    let base = base_dir_for(&scope, q.project_path.as_deref(), &state.cwd_fallback);
    match read_style(&base, &scope, &name) {
        Some(v) => Ok(Json(v)),
        None => Err(AppError::not_found(format!(
            "Output style '{}' not found in {} scope",
            name, scope
        ))),
    }
}

/// POST /api/v1/output-styles
async fn create_output_style(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
    Json(style): Json<OutputStyleCreate>,
) -> AppResult<impl IntoResponse> {
    if style.scope != "user" && style.scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    if style.scope == "project" && q.project_path.is_none() {
        return Err(AppError::bad_request(
            "project_path is required for project-scoped output styles",
        ));
    }

    let base = base_dir_for(&style.scope, q.project_path.as_deref(), &state.cwd_fallback);
    let file_path = base.join(format!("{}.md", style.name));

    if file_path.exists() {
        return Err(AppError::bad_request(format!(
            "Output style already exists: {}",
            style.name
        )));
    }

    paths::ensure_directory_exists(&base);

    let mut metadata: Vec<(String, Meta)> = Vec::new();
    if let Some(d) = style.description.as_deref() {
        if !d.is_empty() {
            metadata.push(("description".into(), Meta::Str(d.to_string())));
        }
    }
    if style.keep_coding_instructions {
        metadata.push((
            "keep-coding-instructions".into(),
            Meta::Bool(style.keep_coding_instructions),
        ));
    }

    let frontmatter = build_frontmatter(metadata);
    let full = format!("{}{}", frontmatter, style.content);

    std::fs::write(&file_path, full)
        .map_err(|e| AppError::internal(format!("Failed to create output style: {}", e)))?;

    let body = style_json(
        &style.name,
        &style.scope,
        style.description.as_deref(),
        style.keep_coding_instructions,
        &style.content,
    );
    Ok((StatusCode::CREATED, Json(body)))
}

/// PUT /api/v1/output-styles/{scope}/{name}
async fn update_output_style(
    State(state): State<ApiState>,
    Path((scope, name)): Path<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
    Json(update): Json<OutputStyleUpdate>,
) -> AppResult<Json<Value>> {
    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }

    let base = base_dir_for(&scope, q.project_path.as_deref(), &state.cwd_fallback);
    let file_path = base.join(format!("{}.md", name));
    if !file_path.exists() {
        return Err(AppError::not_found(format!(
            "Output style '{}' not found in {} scope",
            name, scope
        )));
    }

    let existing = std::fs::read_to_string(&file_path)
        .map_err(|e| AppError::internal(format!("Failed to update output style: {}", e)))?;
    let (mut metadata, mut md) = parse_frontmatter(&existing);

    if let Some(d) = update.description {
        upsert_meta(&mut metadata, "description", Meta::Str(d));
    }
    if let Some(k) = update.keep_coding_instructions {
        upsert_meta(&mut metadata, "keep-coding-instructions", Meta::Bool(k));
    }
    if let Some(c) = update.content {
        md = c;
    }

    let frontmatter = build_frontmatter(metadata.clone());
    let full = format!("{}{}", frontmatter, md);
    std::fs::write(&file_path, full)
        .map_err(|e| AppError::internal(format!("Failed to update output style: {}", e)))?;

    Ok(Json(style_json(
        &meta_get_str(&metadata, "name").unwrap_or_else(|| name.clone()),
        &scope,
        meta_get_str(&metadata, "description").as_deref(),
        meta_get_bool(&metadata, "keep-coding-instructions"),
        &md,
    )))
}

fn upsert_meta(metadata: &mut Vec<(String, Meta)>, key: &str, value: Meta) {
    if let Some(slot) = metadata.iter_mut().find(|(k, _)| k == key) {
        slot.1 = value;
    } else {
        metadata.push((key.to_string(), value));
    }
}

/// DELETE /api/v1/output-styles/{scope}/{name}
async fn delete_output_style(
    State(state): State<ApiState>,
    Path((scope, name)): Path<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<impl IntoResponse> {
    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }

    let base = base_dir_for(&scope, q.project_path.as_deref(), &state.cwd_fallback);
    let file_path = base.join(format!("{}.md", name));
    if !file_path.exists() {
        return Err(AppError::not_found(format!(
            "Output style '{}' not found in {} scope",
            name, scope
        )));
    }

    std::fs::remove_file(&file_path)
        .map_err(|e| AppError::internal(format!("Failed to delete output style: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}
