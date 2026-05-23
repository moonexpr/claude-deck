//! Configuration API — REFERENCE MODULE.
//!
//! Faithful port of `backend_python/app/{services/config_service.py,
//! api/v1/config.py}`. This is the pattern every other ported module should
//! follow: paths via `crate::paths`, IO via `crate::fileio`, errors via
//! `crate::error::AppError` (body `{"detail": ...}`, mirroring FastAPI),
//! route paths IDENTICAL to the Python router (the unchanged frontend was
//! built against them — do not "improve" paths).

use axum::{
    Router,
    extract::{Json, Query},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::fileio::read_json_file;
use crate::paths;
use crate::patterns::{
    migrate_deprecated_pattern, sanitize_permission_rules, validate_permission_pattern,
};

pub fn router() -> Router<ApiState> {
    // Paths mirror Python's `APIRouter(prefix="/config")` exactly.
    Router::new()
        .route("/", get(get_merged_config)) // GET /api/v1/config
        .route("/files", get(list_config_files))
        .route("/raw", get(get_raw_file_content))
        .route("/settings", put(update_settings))
        .route("/validate-settings", post(validate_settings))
        .route("/settings/{scope}", get(get_settings_by_scope))
        .route("/resolved", get(get_resolved_config))
        .route("/scopes", get(get_all_scoped_settings))
}

#[derive(Deserialize)]
struct ProjectPathQuery {
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct RawQuery {
    path: String,
}

#[derive(Deserialize)]
struct SettingsUpdateRequest {
    scope: String,
    settings: Value,
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct SettingsValidationRequest {
    settings: Value,
}

// ---- helpers ----------------------------------------------------------------

/// Recursively collect `*.md` files under `dir` (Python `Path.rglob("*.md")`).
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

fn obj(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}

/// Python `dict.update` — shallow overwrite of top-level keys.
fn shallow_update(target: &mut Map<String, Value>, src: &Map<String, Value>) {
    for (k, v) in src {
        target.insert(k.clone(), v.clone());
    }
}

/// Port of `ConfigService._deep_merge`. `null` override removes the key;
/// empty merged sub-dicts are pruned.
fn deep_merge(base: &Map<String, Value>, override_: &Map<String, Value>) -> Map<String, Value> {
    let mut result = base.clone();
    for (key, value) in override_ {
        if value.is_null() {
            result.remove(key);
        } else if let (Some(Value::Object(b)), Value::Object(o)) = (result.get(key), value) {
            let merged = deep_merge(b, o);
            if merged.is_empty() {
                result.remove(key);
            } else {
                result.insert(key.clone(), Value::Object(merged));
            }
        } else {
            result.insert(key.clone(), value.clone());
        }
    }
    result
}

/// Port of `ConfigService.mask_sensitive_values`.
fn mask_sensitive(value: &Value) -> Value {
    const SENSITIVE: [&str; 6] = ["key", "token", "secret", "password", "api_key", "apikey"];
    match value {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, v) in m {
                let lk = k.to_lowercase();
                if SENSITIVE.iter().any(|s| lk.contains(s)) {
                    let masked = match v {
                        Value::String(s) if s.chars().count() > 4 => {
                            let prefix: String = s.chars().take(4).collect();
                            format!("{}{}", prefix, "*".repeat(s.chars().count() - 4))
                        }
                        _ => "****".to_string(),
                    };
                    out.insert(k.clone(), Value::String(masked));
                } else {
                    match v {
                        Value::Object(_) => {
                            out.insert(k.clone(), mask_sensitive(v));
                        }
                        Value::Array(a) => {
                            let arr = a
                                .iter()
                                .map(|i| {
                                    if i.is_object() {
                                        mask_sensitive(i)
                                    } else {
                                        i.clone()
                                    }
                                })
                                .collect();
                            out.insert(k.clone(), Value::Array(arr));
                        }
                        _ => {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn settings_file_for_scope(scope: &str, project_path: Option<&str>) -> Result<PathBuf, AppError> {
    match scope {
        "user" => Ok(paths::get_claude_user_settings_file()),
        "user_local" => Ok(paths::get_claude_user_settings_local_file()),
        "project" => project_path
            .map(|p| paths::get_project_settings_file(Some(p), std::path::Path::new("")))
            .ok_or_else(|| AppError::bad_request("project_path is required for project scope")),
        "local" => project_path
            .map(|p| paths::get_project_settings_local_file(Some(p), std::path::Path::new("")))
            .ok_or_else(|| AppError::bad_request("project_path is required for local scope")),
        other => Err(AppError::bad_request(format!(
            "Invalid scope: {}. Must be user, user_local, project, or local",
            other
        ))),
    }
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/config/files
async fn list_config_files(Query(q): Query<ProjectPathQuery>) -> AppResult<Json<Value>> {
    let mut files: Vec<Value> = Vec::new();

    for p in [
        paths::get_claude_user_config_file(),
        paths::get_claude_user_settings_file(),
        paths::get_claude_user_settings_local_file(),
    ] {
        files.push(json!({
            "path": p.to_string_lossy(),
            "scope": "user",
            "exists": p.exists(),
            "content": Value::Null,
        }));
    }

    let cmds = paths::get_claude_user_commands_dir();
    if cmds.exists() {
        for f in rglob_md(&cmds) {
            files.push(json!({
                "path": f.to_string_lossy(), "scope": "user",
                "exists": true, "content": Value::Null,
            }));
        }
    }

    if let Some(pp) = q.project_path.as_deref() {
        let proj = PathBuf::from(pp);
        for rel in [
            ".claude/settings.json",
            ".claude/settings.local.json",
            ".mcp.json",
            "CLAUDE.md",
        ] {
            let fp = proj.join(rel);
            files.push(json!({
                "path": fp.to_string_lossy(), "scope": "project",
                "exists": fp.exists(), "content": Value::Null,
            }));
        }
        let proj_cmds = proj.join(".claude/commands");
        if proj_cmds.exists() {
            for f in rglob_md(&proj_cmds) {
                files.push(json!({
                    "path": f.to_string_lossy(), "scope": "project",
                    "exists": true, "content": Value::Null,
                }));
            }
        }
    }

    Ok(Json(json!({ "files": files })))
}

/// GET /api/v1/config
async fn get_merged_config(Query(q): Query<ProjectPathQuery>) -> AppResult<Json<Value>> {
    let pp = q.project_path.as_deref();

    let mut settings = Map::new();
    let mut mcp_servers = Map::new();
    let mut hooks: Map<String, Value> = Map::new();
    let mut permissions: Map<String, Value> = Map::new();
    permissions.insert("allow".into(), json!([]));
    permissions.insert("deny".into(), json!([]));
    let mut commands: Vec<Value> = Vec::new();
    let mut agents: Vec<Value> = Vec::new();

    // user ~/.claude.json — top-level + per-project mcpServers
    let user_claude = paths::get_claude_user_config_file();
    if user_claude.exists() {
        if let Some(data) = read_json_file(&user_claude) {
            if let Some(srv) = data.get("mcpServers").and_then(obj) {
                shallow_update(&mut mcp_servers, srv);
            }
            if let Some(projs) = data.get("projects").and_then(obj) {
                for (_, pc) in projs {
                    if let Some(srv) = pc.get("mcpServers").and_then(obj) {
                        shallow_update(&mut mcp_servers, srv);
                    }
                }
            }
        }
    }

    // user settings.json
    let user_settings = paths::get_claude_user_settings_file();
    if user_settings.exists() {
        if let Some(data) = read_json_file(&user_settings) {
            if let Some(m) = obj(&data) {
                shallow_update(&mut settings, m);
            }
            if let Some(h) = data.get("hooks") {
                if let Some(hm) = h.as_object() {
                    hooks = hm.clone();
                }
            }
            if let Some(p) = data.get("permissions").and_then(obj) {
                shallow_update(&mut permissions, p);
            }
        }
    }

    // user settings.local.json (overrides)
    let user_local = paths::get_claude_user_settings_local_file();
    if user_local.exists() {
        if let Some(data) = read_json_file(&user_local) {
            if let Some(m) = obj(&data) {
                shallow_update(&mut settings, m);
            }
        }
    }

    if let Some(pp) = pp {
        let proj = PathBuf::from(pp);

        let mcp_json = proj.join(".mcp.json");
        if mcp_json.exists() {
            if let Some(data) = read_json_file(&mcp_json) {
                if let Some(srv) = data.get("mcpServers").and_then(obj) {
                    shallow_update(&mut mcp_servers, srv);
                }
            }
        }

        let proj_settings = proj.join(".claude/settings.json");
        if proj_settings.exists() {
            if let Some(data) = read_json_file(&proj_settings) {
                if let Some(m) = obj(&data) {
                    shallow_update(&mut settings, m);
                }
                if let Some(ph) = data.get("hooks").and_then(obj) {
                    for (key, hk) in ph {
                        let entry = hooks.entry(key.clone()).or_insert_with(|| json!([]));
                        if let (Some(dst), Some(src)) = (entry.as_array_mut(), hk.as_array()) {
                            dst.extend(src.iter().cloned());
                        }
                    }
                }
            }
        }

        let proj_local = proj.join(".claude/settings.local.json");
        if proj_local.exists() {
            if let Some(data) = read_json_file(&proj_local) {
                if let Some(m) = obj(&data) {
                    shallow_update(&mut settings, m);
                }
            }
        }
    }

    // commands
    let cmds = paths::get_claude_user_commands_dir();
    if cmds.exists() {
        for f in rglob_md(&cmds) {
            if let Ok(rel) = f.strip_prefix(&cmds) {
                commands.push(Value::String(rel.to_string_lossy().into_owned()));
            }
        }
    }
    if let Some(pp) = pp {
        let proj_cmds = PathBuf::from(pp).join(".claude/commands");
        if proj_cmds.exists() {
            for f in rglob_md(&proj_cmds) {
                if let Ok(rel) = f.strip_prefix(&proj_cmds) {
                    commands.push(Value::String(format!("project:{}", rel.to_string_lossy())));
                }
            }
        }
    }

    // agents (non-recursive *.md, stem)
    let agents_dir = paths::get_claude_user_agents_dir();
    if agents_dir.exists() {
        if let Ok(rd) = std::fs::read_dir(&agents_dir) {
            let mut stems: Vec<String> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
                .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .collect();
            stems.sort();
            agents.extend(stems.into_iter().map(Value::String));
        }
    }
    if let Some(pp) = pp {
        let proj_agents = PathBuf::from(pp).join(".claude/agents");
        if proj_agents.exists() {
            if let Ok(rd) = std::fs::read_dir(&proj_agents) {
                let mut stems: Vec<String> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
                    .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .collect();
                stems.sort();
                agents.extend(
                    stems
                        .into_iter()
                        .map(|s| Value::String(format!("project:{}", s))),
                );
            }
        }
    }

    let merged = json!({
        "settings": Value::Object(settings),
        "mcp_servers": Value::Object(mcp_servers),
        "hooks": Value::Object(hooks),
        "permissions": Value::Object(permissions),
        "commands": commands,
        "agents": agents,
    });

    Ok(Json(mask_sensitive(&merged)))
}

/// GET /api/v1/config/raw?path=
async fn get_raw_file_content(Query(q): Query<RawQuery>) -> AppResult<Json<Value>> {
    let path = PathBuf::from(&q.path);
    if !path.exists() {
        return Ok(Json(
            json!({ "path": q.path, "content": "", "exists": false }),
        ));
    }
    let content_str = if path.extension().and_then(|e| e.to_str()) == Some("json") {
        match read_json_file(&path) {
            Some(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            None => String::new(),
        }
    } else {
        match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                return Ok(Json(json!({
                    "path": q.path,
                    "content": format!("Error reading file: {}", e),
                    "exists": true,
                })));
            }
        }
    };
    Ok(Json(
        json!({ "path": q.path, "content": content_str, "exists": true }),
    ))
}

/// PUT /api/v1/config/settings
async fn update_settings(Json(req): Json<SettingsUpdateRequest>) -> AppResult<Json<Value>> {
    let file_path = settings_file_for_scope(&req.scope, req.project_path.as_deref())?;

    if let Some(parent) = file_path.parent() {
        paths::ensure_directory_exists(parent);
    }

    let existing = if file_path.exists() {
        read_json_file(&file_path).unwrap_or_else(|| json!({}))
    } else {
        json!({})
    };
    let base = existing.as_object().cloned().unwrap_or_default();
    let over = req.settings.as_object().cloned().unwrap_or_default();
    let merged = Value::Object(deep_merge(&base, &over));

    let san = sanitize_permission_rules(&merged);

    let serialized = serde_json::to_string_pretty(&san.sanitized_settings)?;
    match std::fs::write(&file_path, serialized) {
        Ok(_) => {
            let mut message = "Settings updated successfully".to_string();
            if !san.migrated.is_empty() {
                message += &format!(" ({} pattern(s) auto-migrated)", san.migrated.len());
            }
            if !san.removed.is_empty() {
                message += &format!(" ({} invalid pattern(s) removed)", san.removed.len());
            }
            Ok(Json(json!({
                "success": true,
                "message": message,
                "path": file_path.to_string_lossy(),
                "migrated_patterns": san.migrated,
                "removed_patterns": san.removed,
            })))
        }
        Err(e) => Ok(Json(json!({
            "success": false,
            "message": format!("Failed to write settings: {}", e),
            "path": file_path.to_string_lossy(),
        }))),
    }
}

/// POST /api/v1/config/validate-settings
async fn validate_settings(Json(req): Json<SettingsValidationRequest>) -> AppResult<Json<Value>> {
    let mut issues: Vec<Value> = Vec::new();
    let permissions = match req.settings.get("permissions") {
        Some(Value::Object(p)) => p.clone(),
        _ => return Ok(Json(json!({ "valid": true, "issues": [] }))),
    };

    for category in ["allow", "ask", "deny"] {
        let rules = match permissions.get(category) {
            Some(Value::Array(a)) => a,
            _ => continue,
        };
        for pattern in rules {
            match pattern {
                Value::String(s) => {
                    let (ok, err) = validate_permission_pattern(s);
                    if !ok {
                        let suggestion = migrate_deprecated_pattern(s);
                        issues.push(json!({
                            "pattern": s,
                            "category": category,
                            "error": err.unwrap_or_else(|| "Invalid pattern".to_string()),
                            "suggestion": suggestion,
                        }));
                    }
                }
                other => issues.push(json!({
                    "pattern": other.to_string(),
                    "category": category,
                    "error": "Pattern is not a string",
                })),
            }
        }
    }

    Ok(Json(
        json!({ "valid": issues.is_empty(), "issues": issues }),
    ))
}

/// GET /api/v1/config/settings/{scope}
async fn get_settings_by_scope(
    axum::extract::Path(scope): axum::extract::Path<String>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let pp = q.project_path.as_deref();
    let file_path = match scope.as_str() {
        "managed" => Some(paths::get_managed_settings_file()),
        "user" => Some(paths::get_claude_user_settings_file()),
        "user_local" => Some(paths::get_claude_user_settings_local_file()),
        "project" => {
            pp.map(|p| paths::get_project_settings_file(Some(p), std::path::Path::new("")))
        }
        "local" => {
            pp.map(|p| paths::get_project_settings_local_file(Some(p), std::path::Path::new("")))
        }
        _ => None,
    };
    let settings = match file_path {
        Some(fp) if fp.exists() => read_json_file(&fp).unwrap_or_else(|| json!({})),
        _ => json!({}),
    };
    Ok(Json(json!({ "settings": settings, "scope": scope })))
}

fn read_scope(fp: &Path) -> Value {
    if fp.exists() {
        read_json_file(fp).unwrap_or_else(|| json!({}))
    } else {
        json!({})
    }
}

fn all_scoped(pp: Option<&str>) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert(
        "managed".into(),
        read_scope(&paths::get_managed_settings_file()),
    );
    m.insert(
        "user".into(),
        read_scope(&paths::get_claude_user_settings_file()),
    );
    m.insert(
        "project".into(),
        pp.map(|p| {
            read_scope(&paths::get_project_settings_file(
                Some(p),
                std::path::Path::new(""),
            ))
        })
        .unwrap_or_else(|| json!({})),
    );
    m.insert(
        "local".into(),
        pp.map(|p| {
            read_scope(&paths::get_project_settings_local_file(
                Some(p),
                std::path::Path::new(""),
            ))
        })
        .unwrap_or_else(|| json!({})),
    );
    m
}

/// GET /api/v1/config/scopes
async fn get_all_scoped_settings(Query(q): Query<ProjectPathQuery>) -> AppResult<Json<Value>> {
    Ok(Json(
        json!({ "scopes": all_scoped(q.project_path.as_deref()) }),
    ))
}

fn flatten_keys(v: &Value, parent: &str, out: &mut std::collections::BTreeSet<String>) {
    if let Value::Object(m) = v {
        for (k, val) in m {
            let nk = if parent.is_empty() {
                k.clone()
            } else {
                format!("{}.{}", parent, k)
            };
            if val.is_object() {
                flatten_keys(val, &nk, out);
            } else {
                out.insert(nk);
            }
        }
    }
}

fn get_nested<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    let mut cur = v;
    for part in key.split('.') {
        cur = cur.as_object()?.get(part)?;
    }
    Some(cur)
}

/// GET /api/v1/config/resolved
async fn get_resolved_config(Query(q): Query<ProjectPathQuery>) -> AppResult<Json<Value>> {
    let pp = q.project_path.as_deref();
    let scoped = all_scoped(pp);
    const PRIORITY: [&str; 4] = ["managed", "local", "project", "user"];

    let mut all_keys = std::collections::BTreeSet::new();
    for s in scoped.values() {
        flatten_keys(s, "", &mut all_keys);
    }

    let mut resolved = Map::new();
    for key in &all_keys {
        let mut values_by_scope = Map::new();
        let mut effective: Option<Value> = None;
        let mut source: Option<String> = None;
        for scope in PRIORITY {
            if let Some(val) = scoped.get(scope).and_then(|s| get_nested(s, key)) {
                values_by_scope.insert(scope.to_string(), val.clone());
                if effective.is_none() {
                    effective = Some(val.clone());
                    source = Some(scope.to_string());
                }
            }
        }
        resolved.insert(
            key.clone(),
            json!({
                "effective_value": effective.unwrap_or(Value::Null),
                "source_scope": source,
                "values_by_scope": values_by_scope,
            }),
        );
    }

    let managed = paths::get_managed_settings_file();
    let user = paths::get_claude_user_settings_file();
    let proj_p = pp.map(|p| paths::get_project_settings_file(Some(p), std::path::Path::new("")));
    let local_p =
        pp.map(|p| paths::get_project_settings_local_file(Some(p), std::path::Path::new("")));

    Ok(Json(json!({
        "resolved": resolved,
        "scopes": {
            "managed": { "settings": scoped["managed"], "path": managed.to_string_lossy(), "exists": managed.exists(), "readonly": true },
            "user": { "settings": scoped["user"], "path": user.to_string_lossy(), "exists": user.exists(), "readonly": false },
            "project": {
                "settings": scoped["project"],
                "path": proj_p.as_ref().map(|p| p.to_string_lossy().into_owned()),
                "exists": proj_p.as_ref().map(|p| p.exists()).unwrap_or(false),
                "readonly": false
            },
            "local": {
                "settings": scoped["local"],
                "path": local_p.as_ref().map(|p| p.to_string_lossy().into_owned()),
                "exists": local_p.as_ref().map(|p| p.exists()).unwrap_or(false),
                "readonly": false
            }
        }
    })))
}
