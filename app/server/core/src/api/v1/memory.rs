// PORTED: memory_service.py + api/v1/memory.py
//
// Faithful port of `backend_python/app/{services/memory_service.py,
// api/v1/memory.py}`. Route paths, query/body field names and response JSON
// shapes match the Python module exactly (the unchanged frontend was built
// against them). Paths via `crate::paths`, errors via `crate::error::AppError`.

use axum::{
    Router,
    extract::{Json, Query, State},
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::paths;

pub fn router() -> Router<ApiState> {
    // Paths mirror Python's `APIRouter(prefix="/memory")` exactly.
    Router::new()
        .route("/hierarchy", get(get_memory_hierarchy))
        .route("/file", get(get_memory_file))
        .route("/file", put(save_memory_file))
        .route("/file", delete(delete_memory_file))
        .route("/rules", get(list_rules))
        .route("/rules", post(create_rule))
        .route("/auto-memory", get(list_auto_memory))
        .route("/imports", get(resolve_imports))
}

// ---- query / body structs ---------------------------------------------------

#[derive(Deserialize)]
struct ProjectPathQuery {
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct FileQuery {
    file_path: String,
    #[serde(default = "default_true")]
    include_imports: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct FilePathOnlyQuery {
    file_path: String,
}

#[derive(Deserialize)]
struct SaveMemoryRequest {
    content: String,
}

#[derive(Deserialize)]
struct CreateRuleRequest {
    name: String,
    content: String,
    #[serde(default)]
    paths: Option<Vec<String>>,
    #[serde(default)]
    description: Option<String>,
}

// ---- path helpers (port of MemoryService static methods) --------------------

fn get_user_claude_md() -> PathBuf {
    paths::get_claude_user_config_dir().join("CLAUDE.md")
}

fn get_managed_claude_md() -> PathBuf {
    PathBuf::from("/etc/claude-code/CLAUDE.md")
}

fn get_project_claude_md(project_path: Option<&str>, cwd_fallback: &Path) -> PathBuf {
    match project_path {
        Some(p) => PathBuf::from(p).join("CLAUDE.md"),
        None => cwd_fallback.join("CLAUDE.md"),
    }
}

fn get_project_alt_claude_md(project_path: Option<&str>, cwd_fallback: &Path) -> PathBuf {
    paths::get_project_claude_dir(project_path, cwd_fallback).join("CLAUDE.md")
}

fn get_local_claude_md(project_path: Option<&str>, cwd_fallback: &Path) -> PathBuf {
    match project_path {
        Some(p) => PathBuf::from(p).join("CLAUDE.local.md"),
        None => cwd_fallback.join("CLAUDE.local.md"),
    }
}

fn get_rules_dir(project_path: Option<&str>, cwd_fallback: &Path) -> PathBuf {
    paths::get_project_claude_dir(project_path, cwd_fallback).join("rules")
}

/// Python `Path(p).expanduser()` — expand a leading `~` to the home dir.
fn expanduser(p: &str) -> PathBuf {
    if p == "~" {
        return home_dir();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(p)
}

fn home_dir() -> PathBuf {
    crate::paths::get_user_home()
}

/// Recursively collect `*.md` files under `dir`, sorted (Python
/// `sorted(Path.rglob("*.md"))`).
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

/// Non-recursive `*.md` glob, sorted (Python `sorted(dir.glob("*.md"))`).
fn glob_md(dir: &Path) -> Vec<PathBuf> {
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

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ---- frontmatter / imports --------------------------------------------------

/// Port of `_parse_rule_frontmatter`. Returns (frontmatter, remaining body).
/// Mirrors the Python regex `\n---\s*\n` applied to `content[3:]`.
fn parse_rule_frontmatter(content: &str) -> (Value, String) {
    if !content.starts_with("---") {
        return (json!({}), content.to_string());
    }
    let after = &content[3..];
    // Find the first `\n---` followed by optional spaces/tabs then `\n`.
    let bytes = after.as_bytes();
    let mut idx = 0usize;
    let mut found: Option<(usize, usize)> = None; // (match_start_in_after, match_end_in_after)
    while idx < bytes.len() {
        if bytes[idx] == b'\n' {
            // candidate: \n --- \s* \n
            let mut j = idx + 1;
            if after[j..].starts_with("---") {
                j += 3;
                let ws_start = j;
                while j < bytes.len()
                    && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\r')
                {
                    j += 1;
                }
                // require at least the trailing newline
                if j < bytes.len() && bytes[j] == b'\n' {
                    let _ = ws_start;
                    found = Some((idx, j + 1));
                    break;
                }
            }
        }
        idx += 1;
    }
    let Some((m_start, m_end)) = found else {
        return (json!({}), content.to_string());
    };
    let frontmatter_str = &content[3..m_start + 3];
    let remaining = &content[m_end + 3..];
    // NEEDS-DEP: serde_yaml (not in Cargo.toml). Python uses `yaml.safe_load`
    // (full YAML). This is a minimal block-style YAML subset parser covering
    // the shapes Claude rule frontmatter actually uses (scalar `key: value`
    // and `key:` + `  - item` sequences), which is exactly what
    // `MemoryService.create_rule` emits. Documents not matching this subset
    // fall back to `{}` (same observable result as `yaml.safe_load` raising).
    match parse_simple_yaml(frontmatter_str) {
        Some(v) if v.is_object() => (v, remaining.to_string()),
        Some(_) => (json!({}), remaining.to_string()),
        None => (json!({}), content.to_string()),
    }
}

/// Minimal block YAML mapping parser. Returns `None` on a construct outside
/// the supported subset (signals a parse failure, mirroring `yaml.YAMLError`).
fn parse_simple_yaml(src: &str) -> Option<Value> {
    let mut map = Map::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let raw = lines[i];
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            i += 1;
            continue;
        }
        // Top-level keys must be unindented.
        if line.starts_with([' ', '\t']) {
            return None;
        }
        let colon = line.find(':')?;
        let key = line[..colon].trim().to_string();
        if key.is_empty() {
            return None;
        }
        let rest = line[colon + 1..].trim();
        if rest.is_empty() {
            // Possible block sequence on following indented `- ` lines.
            let mut seq: Vec<Value> = Vec::new();
            i += 1;
            while i < lines.len() {
                let item_raw = lines[i];
                if item_raw.trim().is_empty() {
                    i += 1;
                    continue;
                }
                let trimmed = item_raw.trim_start();
                if !item_raw.starts_with([' ', '\t']) {
                    break;
                }
                let item = trimmed
                    .strip_prefix("- ")
                    .or_else(|| if trimmed == "-" { Some("") } else { None })?;
                seq.push(scalar_to_value(item.trim()));
                i += 1;
            }
            map.insert(key, Value::Array(seq));
        } else {
            map.insert(key, scalar_to_value(rest));
            i += 1;
        }
    }
    Some(Value::Object(map))
}

fn scalar_to_value(s: &str) -> Value {
    let t = s.trim();
    if (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
        || (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2)
    {
        return Value::String(t[1..t.len() - 1].to_string());
    }
    match t {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(n) = t.parse::<i64>() {
        return json!(n);
    }
    if let Ok(f) = t.parse::<f64>() {
        if t.chars().any(|c| c == '.' || c == 'e' || c == 'E') {
            return json!(f);
        }
    }
    Value::String(t.to_string())
}

/// Port of `_extract_imports`: regex `@([~./]?[\w./-]+)`, deduped (set order
/// is non-deterministic in Python; we sort for stable output).
fn extract_imports(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut set: BTreeSet<String> = BTreeSet::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let mut j = i + 1;
            // optional single leading char from [~./]
            if j < bytes.len() && matches!(bytes[j], b'~' | b'.' | b'/') {
                j += 1;
            }
            while j < bytes.len() && is_word_path_char(bytes[j]) {
                j += 1;
            }
            // Python `@([~./]?[\w./-]+)` requires at least one [\w./-] char.
            if j > i + 1 {
                set.insert(content[i + 1..j].to_string());
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    set.into_iter().collect()
}

fn is_word_path_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'/' || b == b'-'
}

// ---- core service ports -----------------------------------------------------

fn memory_hierarchy(project_path: Option<&str>, cwd_fallback: &Path) -> Vec<Value> {
    let mut files: Vec<Value> = Vec::new();

    let managed = get_managed_claude_md();
    files.push(json!({
        "path": managed.to_string_lossy(),
        "scope": "managed",
        "type": "claude_md",
        "exists": managed.exists(),
        "readonly": true,
        "description": "Organization-wide instructions (managed policy)",
    }));

    let user = get_user_claude_md();
    files.push(json!({
        "path": user.to_string_lossy(),
        "scope": "user",
        "type": "claude_md",
        "exists": user.exists(),
        "readonly": false,
        "description": "Personal preferences (all projects)",
    }));

    let proj = get_project_claude_md(project_path, cwd_fallback);
    let proj_alt = get_project_alt_claude_md(project_path, cwd_fallback);
    if proj.exists() {
        files.push(json!({
            "path": proj.to_string_lossy(),
            "scope": "project",
            "type": "claude_md",
            "exists": true,
            "readonly": false,
            "description": "Team-shared project instructions",
        }));
    } else if proj_alt.exists() {
        files.push(json!({
            "path": proj_alt.to_string_lossy(),
            "scope": "project",
            "type": "claude_md",
            "exists": true,
            "readonly": false,
            "description": "Team-shared project instructions",
        }));
    } else {
        files.push(json!({
            "path": proj.to_string_lossy(),
            "scope": "project",
            "type": "claude_md",
            "exists": false,
            "readonly": false,
            "description": "Team-shared project instructions",
        }));
    }

    let local = get_local_claude_md(project_path, cwd_fallback);
    files.push(json!({
        "path": local.to_string_lossy(),
        "scope": "local",
        "type": "claude_md",
        "exists": local.exists(),
        "readonly": false,
        "description": "Personal project-specific preferences (gitignored)",
    }));

    let rules_dir = get_rules_dir(project_path, cwd_fallback);
    if rules_dir.exists() {
        for rule_file in rglob_md(&rules_dir) {
            let rel = rule_file
                .strip_prefix(&rules_dir)
                .unwrap_or(&rule_file)
                .to_path_buf();
            files.push(json!({
                "path": rule_file.to_string_lossy(),
                "scope": "rules",
                "type": "rule",
                "name": stem(&rel),
                "relative_path": rel.to_string_lossy(),
                "exists": true,
                "readonly": false,
                "description": format!("Rule: {}", stem(&rel)),
            }));
        }
    }

    files
}

fn read_memory_file(file_path: &str, include_imports: bool) -> Value {
    let path = expanduser(file_path);
    let path_str = path.to_string_lossy().into_owned();
    let exists = path.exists();

    let mut result = Map::new();
    result.insert("path".into(), json!(path_str));
    result.insert("exists".into(), json!(exists));
    result.insert("content".into(), Value::Null);
    result.insert("imports".into(), json!([]));
    result.insert("frontmatter".into(), json!({}));

    if exists {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                result.insert("content".into(), json!(content));
                if include_imports {
                    result.insert("imports".into(), json!(extract_imports(&content)));
                }
                if path_str.contains(".claude/rules/") || path_str.contains("/rules/") {
                    let (fm, _) = parse_rule_frontmatter(&content);
                    result.insert("frontmatter".into(), fm);
                }
            }
            Err(e) => {
                result.insert("error".into(), json!(e.to_string()));
            }
        }
    }

    Value::Object(result)
}

fn save_memory(file_path: &str, content: &str, create_parents: bool) -> Value {
    let path = expanduser(file_path);
    let path_str = path.to_string_lossy().into_owned();

    if path_str == get_managed_claude_md().to_string_lossy() {
        return json!({
            "success": false,
            "error": "Cannot modify managed policy file",
            "path": path_str,
        });
    }

    if create_parents {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return json!({
                    "success": false,
                    "error": e.to_string(),
                    "path": path_str,
                });
            }
        }
    }

    match std::fs::write(&path, content) {
        Ok(_) => json!({ "success": true, "path": path_str }),
        Err(e) => json!({
            "success": false,
            "error": e.to_string(),
            "path": path_str,
        }),
    }
}

fn delete_memory(file_path: &str) -> Value {
    let path = expanduser(file_path);
    let path_str = path.to_string_lossy().into_owned();

    if path_str == get_managed_claude_md().to_string_lossy() {
        return json!({
            "success": false,
            "error": "Cannot delete managed policy file",
            "path": path_str,
        });
    }

    if path.exists() {
        match std::fs::remove_file(&path) {
            Ok(_) => json!({ "success": true, "path": path_str }),
            Err(e) => json!({
                "success": false,
                "error": e.to_string(),
                "path": path_str,
            }),
        }
    } else {
        json!({
            "success": false,
            "error": "File does not exist",
            "path": path_str,
        })
    }
}

fn rules_list(project_path: Option<&str>, cwd_fallback: &Path) -> Vec<Value> {
    let rules_dir = get_rules_dir(project_path, cwd_fallback);
    let mut rules: Vec<Value> = Vec::new();
    if !rules_dir.exists() {
        return rules;
    }
    for rule_file in rglob_md(&rules_dir) {
        let rel = rule_file
            .strip_prefix(&rules_dir)
            .unwrap_or(&rule_file)
            .to_path_buf();
        let content = std::fs::read_to_string(&rule_file).unwrap_or_default();
        let (frontmatter, body) = parse_rule_frontmatter(&content);

        let scoped_paths: Vec<String> = match frontmatter.get("paths") {
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect(),
            Some(Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        };

        let description = match frontmatter.get("description") {
            Some(Value::String(s)) => s.clone(),
            Some(other) if !other.is_null() => other.to_string(),
            _ => String::new(),
        };

        let preview: String = body.chars().take(200).collect();

        rules.push(json!({
            "name": stem(&rel),
            "path": rule_file.to_string_lossy(),
            "relative_path": rel.to_string_lossy(),
            "frontmatter": frontmatter,
            "scoped_paths": scoped_paths,
            "description": description,
            "content_preview": preview,
        }));
    }
    rules
}

fn create_rule_file(
    project_path: Option<&str>,
    name: &str,
    content: &str,
    rule_paths: Option<&[String]>,
    description: Option<&str>,
    cwd_fallback: &Path,
) -> Result<Value, AppError> {
    let rules_dir = get_rules_dir(project_path, cwd_fallback);
    if let Err(e) = std::fs::create_dir_all(&rules_dir) {
        return Err(AppError::internal(e.to_string()));
    }

    let rule_path = rules_dir.join(format!("{}.md", name));

    let mut frontmatter_parts: Vec<String> = Vec::new();
    if let Some(d) = description {
        if !d.is_empty() {
            frontmatter_parts.push(format!("description: {}", d));
        }
    }
    if let Some(ps) = rule_paths {
        if !ps.is_empty() {
            frontmatter_parts.push("paths:".to_string());
            for p in ps {
                frontmatter_parts.push(format!("  - {}", p));
            }
        }
    }

    let full_content = if !frontmatter_parts.is_empty() {
        format!("---\n{}\n---\n\n{}", frontmatter_parts.join("\n"), content)
    } else {
        content.to_string()
    };

    Ok(save_memory(
        &rule_path.to_string_lossy(),
        &full_content,
        true,
    ))
}

fn auto_memory(project_path: &str) -> Value {
    let folder_name = paths::convert_path_to_folder_name(project_path);
    let memory_dir = paths::get_claude_projects_dir()
        .join(&folder_name)
        .join("memory");

    let mut files: Vec<Value> = Vec::new();
    if memory_dir.exists() {
        for md_file in glob_md(&memory_dir) {
            if let Ok(meta) = std::fs::metadata(&md_file) {
                let size = meta.len();
                let modified_at = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                files.push(json!({
                    "name": md_file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    "path": md_file.to_string_lossy(),
                    "size": size,
                    "modified_at": modified_at,
                }));
            }
        }
    }

    json!({
        "memory_dir": memory_dir.to_string_lossy(),
        "files": files,
    })
}

fn resolve_import_tree(
    file_path: &str,
    visited: &mut BTreeSet<String>,
    cwd_fallback: &Path,
) -> Value {
    let path = expanduser(file_path);
    let path_str = path.to_string_lossy().into_owned();
    let resolved_str = std::fs::canonicalize(&path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| {
            // Python Path.resolve() returns an absolute path even when the
            // target does not exist. Approximate with absolute(path).
            if path.is_absolute() {
                path_str.clone()
            } else {
                cwd_fallback.join(&path).to_string_lossy().into_owned()
            }
        });

    if visited.contains(&resolved_str) {
        return json!({
            "path": path_str,
            "exists": path.exists(),
            "cycle": true,
            "imports": [],
        });
    }

    visited.insert(resolved_str);

    let exists = path.exists();
    let mut result = Map::new();
    result.insert("path".into(), json!(path_str));
    result.insert("exists".into(), json!(exists));
    result.insert("cycle".into(), json!(false));
    result.insert("imports".into(), json!([]));

    if !exists {
        return Value::Object(result);
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let imports = extract_imports(&content);
            let mut import_nodes: Vec<Value> = Vec::new();
            for imp in imports {
                let imp_path: PathBuf = if imp.starts_with("~/") || imp == "~" {
                    expanduser(&imp)
                } else if imp.starts_with("./") || imp.starts_with("../") {
                    let joined = path
                        .parent()
                        .map(|p| p.join(&imp))
                        .unwrap_or_else(|| PathBuf::from(&imp));
                    std::fs::canonicalize(&joined).unwrap_or_else(|_| {
                        if joined.is_absolute() {
                            joined.clone()
                        } else {
                            cwd_fallback.join(&joined)
                        }
                    })
                } else {
                    PathBuf::from(&imp)
                };
                let mut branch = visited.clone();
                import_nodes.push(resolve_import_tree(
                    &imp_path.to_string_lossy(),
                    &mut branch,
                    cwd_fallback,
                ));
            }
            result.insert("imports".into(), Value::Array(import_nodes));
        }
        Err(e) => {
            result.insert("error".into(), json!(e.to_string()));
        }
    }

    Value::Object(result)
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/memory/hierarchy
async fn get_memory_hierarchy(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let files = memory_hierarchy(q.project_path.as_deref(), &state.cwd_fallback);
    Ok(Json(json!({ "files": files })))
}

/// GET /api/v1/memory/file
async fn get_memory_file(Query(q): Query<FileQuery>) -> AppResult<Json<Value>> {
    Ok(Json(read_memory_file(&q.file_path, q.include_imports)))
}

/// PUT /api/v1/memory/file
async fn save_memory_file(
    Query(q): Query<FilePathOnlyQuery>,
    Json(req): Json<SaveMemoryRequest>,
) -> AppResult<Json<Value>> {
    let result = save_memory(&q.file_path, &req.content, true);
    if result.get("success").and_then(|v| v.as_bool()) != Some(true) {
        let msg = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Save failed")
            .to_string();
        return Err(AppError::bad_request(msg));
    }
    Ok(Json(result))
}

/// DELETE /api/v1/memory/file
async fn delete_memory_file(Query(q): Query<FilePathOnlyQuery>) -> AppResult<Json<Value>> {
    let result = delete_memory(&q.file_path);
    if result.get("success").and_then(|v| v.as_bool()) != Some(true) {
        let msg = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Delete failed")
            .to_string();
        return Err(AppError::bad_request(msg));
    }
    Ok(Json(result))
}

/// GET /api/v1/memory/rules
async fn list_rules(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let pp = q.project_path.as_deref();
    let rules = rules_list(pp, &state.cwd_fallback);
    let rules_dir = get_rules_dir(pp, &state.cwd_fallback);
    Ok(Json(json!({
        "rules": rules,
        "rules_dir": rules_dir.to_string_lossy(),
    })))
}

/// POST /api/v1/memory/rules
async fn create_rule(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
    Json(req): Json<CreateRuleRequest>,
) -> AppResult<Json<Value>> {
    let result = create_rule_file(
        q.project_path.as_deref(),
        &req.name,
        &req.content,
        req.paths.as_deref(),
        req.description.as_deref(),
        &state.cwd_fallback,
    )?;
    if result.get("success").and_then(|v| v.as_bool()) != Some(true) {
        let msg = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Create failed")
            .to_string();
        return Err(AppError::bad_request(msg));
    }
    Ok(Json(result))
}

/// GET /api/v1/memory/auto-memory
async fn list_auto_memory(Query(q): Query<FilePathProjectQuery>) -> AppResult<Json<Value>> {
    Ok(Json(auto_memory(&q.project_path)))
}

#[derive(Deserialize)]
struct FilePathProjectQuery {
    project_path: String,
}

/// GET /api/v1/memory/imports
async fn resolve_imports(
    State(state): State<ApiState>,
    Query(q): Query<FilePathOnlyQuery>,
) -> AppResult<Json<Value>> {
    let mut visited = BTreeSet::new();
    let tree = resolve_import_tree(&q.file_path, &mut visited, &state.cwd_fallback);
    Ok(Json(json!({ "tree": tree })))
}
