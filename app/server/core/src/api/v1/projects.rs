// PORTED: project_service.py + api/v1/projects.py

use axum::{
    Router,
    extract::{Json, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::fileio::read_json_file;
use crate::paths;

pub fn router() -> Router<ApiState> {
    // Paths mirror Python's `APIRouter` mounted at `/projects`.
    // `/active` routes MUST be registered before `/{project_id}` (Python ordering).
    Router::new()
        .route("/", get(list_projects).post(add_project))
        .route("/discover", post(discover_projects))
        .route(
            "/active",
            get(get_active_project)
                .put(set_active_project)
                .delete(clear_active_project),
        )
        .route("/{project_id}", axum::routing::delete(remove_project))
        .route("/{project_id}/config", get(get_project_config))
}

#[derive(Deserialize)]
struct ProjectCreate {
    name: String,
    path: String,
    #[serde(default)]
    #[allow(dead_code)]
    source: Option<String>,
}

#[derive(Deserialize)]
struct ProjectDiscoveryRequest {
    base_path: String,
}

#[derive(Deserialize)]
struct SetActiveProjectRequest {
    project_id: i64,
}

// ---- helpers ----------------------------------------------------------------

/// Python `datetime.utcnow().isoformat()` -> `2026-05-16T12:34:56.123456`.
fn utcnow_iso() -> String {
    chrono::Utc::now()
        .naive_utc()
        .format("%Y-%m-%dT%H:%M:%S%.6f")
        .to_string()
}

/// SQLAlchemy stores naive datetimes as `YYYY-MM-DD HH:MM:SS[.ffffff]`.
/// `ProjectResponse` exposes them via `.isoformat()` -> `YYYY-MM-DDTHH:MM:SS...`.
fn stored_to_isoformat(raw: &str) -> String {
    raw.replacen(' ', "T", 1)
}

/// Ensure the `projects` table exists (mirrors SQLAlchemy `create_all`; the
/// Python backend had no migration system either).
async fn ensure_table(pool: &sqlx::SqlitePool) -> Result<(), AppError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS projects (\
            id INTEGER PRIMARY KEY AUTOINCREMENT,\
            name TEXT NOT NULL,\
            path TEXT NOT NULL UNIQUE,\
            is_active BOOLEAN NOT NULL DEFAULT 0,\
            last_accessed DATETIME NOT NULL,\
            created_at DATETIME NOT NULL)",
    )
    .execute(pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

struct ProjectRow {
    id: i64,
    name: String,
    path: String,
    is_active: bool,
    last_accessed: String,
    created_at: String,
}

fn row_to_response(p: &ProjectRow) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "path": p.path,
        "source": Value::Null,
        "is_active": p.is_active,
        "last_accessed": stored_to_isoformat(&p.last_accessed),
        "created_at": stored_to_isoformat(&p.created_at),
    })
}

async fn fetch_project(pool: &sqlx::SqlitePool, id: i64) -> Result<Option<ProjectRow>, AppError> {
    use sqlx::Row;
    let r = sqlx::query(
        "SELECT id, name, path, is_active, last_accessed, created_at FROM projects WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(r.map(|row| ProjectRow {
        id: row.get("id"),
        name: row.get("name"),
        path: row.get("path"),
        is_active: row.get::<bool, _>("is_active"),
        last_accessed: row.get("last_accessed"),
        created_at: row.get("created_at"),
    }))
}

fn obj(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}

fn shallow_update(target: &mut Map<String, Value>, src: &Map<String, Value>) {
    for (k, v) in src {
        target.insert(k.clone(), v.clone());
    }
}

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
    out
}

/// Faithful port of `ConfigService.get_merged_config` (NOT masked — Python's
/// `get_project_config` returns the raw merged config).
fn get_merged_config(project_path: Option<&str>) -> Value {
    let mut settings = Map::new();
    let mut mcp_servers = Map::new();
    let mut hooks: Map<String, Value> = Map::new();
    let mut permissions: Map<String, Value> = Map::new();
    permissions.insert("allow".into(), json!([]));
    permissions.insert("deny".into(), json!([]));
    let mut commands: Vec<Value> = Vec::new();
    let mut agents: Vec<Value> = Vec::new();

    let user_claude = paths::get_claude_user_config_file();
    if user_claude.exists()
        && let Some(data) = read_json_file(&user_claude) {
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

    let user_settings = paths::get_claude_user_settings_file();
    if user_settings.exists()
        && let Some(data) = read_json_file(&user_settings) {
            if let Some(m) = obj(&data) {
                shallow_update(&mut settings, m);
            }
            if let Some(h) = data.get("hooks")
                && let Some(hm) = h.as_object() {
                    hooks = hm.clone();
                }
            if let Some(p) = data.get("permissions").and_then(obj) {
                shallow_update(&mut permissions, p);
            }
        }

    let user_local = paths::get_claude_user_settings_local_file();
    if user_local.exists()
        && let Some(data) = read_json_file(&user_local)
            && let Some(m) = obj(&data) {
                shallow_update(&mut settings, m);
            }

    if let Some(pp) = project_path {
        let proj = PathBuf::from(pp);

        let mcp_json = proj.join(".mcp.json");
        if mcp_json.exists()
            && let Some(data) = read_json_file(&mcp_json)
                && let Some(srv) = data.get("mcpServers").and_then(obj) {
                    shallow_update(&mut mcp_servers, srv);
                }

        let proj_settings = proj.join(".claude/settings.json");
        if proj_settings.exists()
            && let Some(data) = read_json_file(&proj_settings) {
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

        let proj_local = proj.join(".claude/settings.local.json");
        if proj_local.exists()
            && let Some(data) = read_json_file(&proj_local)
                && let Some(m) = obj(&data) {
                    shallow_update(&mut settings, m);
                }
    }

    let cmds = paths::get_claude_user_commands_dir();
    if cmds.exists() {
        for f in rglob_md(&cmds) {
            if let Ok(rel) = f.strip_prefix(&cmds) {
                commands.push(Value::String(rel.to_string_lossy().into_owned()));
            }
        }
    }
    if let Some(pp) = project_path {
        let proj_cmds = PathBuf::from(pp).join(".claude/commands");
        if proj_cmds.exists() {
            for f in rglob_md(&proj_cmds) {
                if let Ok(rel) = f.strip_prefix(&proj_cmds) {
                    commands.push(Value::String(format!("project:{}", rel.to_string_lossy())));
                }
            }
        }
    }

    let agents_dir = paths::get_claude_user_agents_dir();
    if agents_dir.exists()
        && let Ok(rd) = std::fs::read_dir(&agents_dir) {
            for p in rd.flatten().map(|e| e.path()) {
                if p.extension().and_then(|x| x.to_str()) == Some("md")
                    && let Some(stem) = p.file_stem() {
                        agents.push(Value::String(stem.to_string_lossy().into_owned()));
                    }
            }
        }
    if let Some(pp) = project_path {
        let proj_agents = PathBuf::from(pp).join(".claude/agents");
        if proj_agents.exists()
            && let Ok(rd) = std::fs::read_dir(&proj_agents) {
                for p in rd.flatten().map(|e| e.path()) {
                    if p.extension().and_then(|x| x.to_str()) == Some("md")
                        && let Some(stem) = p.file_stem() {
                            agents
                                .push(Value::String(format!("project:{}", stem.to_string_lossy())));
                        }
                }
            }
    }

    json!({
        "settings": Value::Object(settings),
        "mcp_servers": Value::Object(mcp_servers),
        "hooks": Value::Object(hooks),
        "permissions": Value::Object(permissions),
        "commands": commands,
        "agents": agents,
    })
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/projects
async fn list_projects(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    use sqlx::Row;
    ensure_table(&state.pool).await?;
    let rows = sqlx::query(
        "SELECT id, name, path, is_active, last_accessed, created_at \
         FROM projects ORDER BY last_accessed DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;

    let projects: Vec<Value> = rows
        .iter()
        .map(|row| {
            row_to_response(&ProjectRow {
                id: row.get("id"),
                name: row.get("name"),
                path: row.get("path"),
                is_active: row.get::<bool, _>("is_active"),
                last_accessed: row.get("last_accessed"),
                created_at: row.get("created_at"),
            })
        })
        .collect();

    Ok(Json(json!({ "projects": projects })))
}

/// POST /api/v1/projects
async fn add_project(
    State(state): State<ApiState>,
    Json(req): Json<ProjectCreate>,
) -> AppResult<Json<Value>> {
    use sqlx::Row;
    ensure_table(&state.pool).await?;

    let existing = sqlx::query("SELECT id FROM projects WHERE path = ?")
        .bind(&req.path)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;

    let now = utcnow_iso();

    if let Some(row) = existing {
        let id: i64 = row.get("id");
        sqlx::query("UPDATE projects SET name = ?, last_accessed = ? WHERE id = ?")
            .bind(&req.name)
            .bind(&now)
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(|e| AppError::internal(e.to_string()))?;
        let p = fetch_project(&state.pool, id)
            .await?
            .ok_or_else(|| AppError::internal("project vanished after update"))?;
        return Ok(Json(row_to_response(&p)));
    }

    let res = sqlx::query(
        "INSERT INTO projects (name, path, is_active, last_accessed, created_at) \
         VALUES (?, ?, 0, ?, ?)",
    )
    .bind(&req.name)
    .bind(&req.path)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;

    let id = res.last_insert_rowid();
    let p = fetch_project(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::internal("project vanished after insert"))?;
    Ok(Json(row_to_response(&p)))
}

/// POST /api/v1/projects/discover
async fn discover_projects(Json(req): Json<ProjectDiscoveryRequest>) -> AppResult<Json<Value>> {
    let mut discovered: Vec<Value> = Vec::new();
    let mut discovered_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Path(base_path).expanduser().resolve()
    let expanded = if let Some(rest) = req.base_path.strip_prefix('~') {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        if rest.is_empty() {
            home
        } else {
            home.join(rest.trim_start_matches('/'))
        }
    } else {
        PathBuf::from(&req.base_path)
    };
    let base_dir = std::fs::canonicalize(&expanded).unwrap_or(expanded);

    if !base_dir.exists() || !base_dir.is_dir() {
        return Ok(Json(json!({ "discovered": [] })));
    }

    let mut dirs_to_check: Vec<PathBuf> = vec![base_dir.clone()];
    if let Ok(rd) = std::fs::read_dir(&base_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir()
                && let Some(name) = p.file_name().and_then(|n| n.to_str())
                    && !name.starts_with('.') {
                        dirs_to_check.push(p);
                    }
        }
    }

    // Phase 1: .claude/ dir or .mcp.json file => "configured"
    for directory in &dirs_to_check {
        let dir_str = directory.to_string_lossy().into_owned();
        let cwd = std::path::Path::new("");
        let claude_dir = paths::get_project_claude_dir(Some(&dir_str), cwd);
        let mcp_file = paths::get_project_mcp_config_file(Some(&dir_str), cwd);
        if claude_dir.exists() || mcp_file.exists() {
            let name = directory
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            discovered.push(json!({
                "name": name,
                "path": dir_str,
                "source": "configured",
            }));
            discovered_paths.insert(dir_str);
        }
    }

    // Phase 2: ~/.claude/projects/ session history
    let global_projects_dir = paths::get_claude_projects_dir();
    if global_projects_dir.exists() {
        let mut global_entries: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        if let Ok(rd) = std::fs::read_dir(&global_projects_dir) {
            for entry in rd.flatten() {
                if entry.path().is_dir() {
                    global_entries.insert(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
        for directory in &dirs_to_check {
            let dir_str = directory.to_string_lossy().into_owned();
            if discovered_paths.contains(&dir_str) {
                continue;
            }
            let encoded = paths::convert_path_to_folder_name(&dir_str);
            if global_entries.contains(&encoded) {
                let name = directory
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                discovered.push(json!({
                    "name": name,
                    "path": dir_str,
                    "source": "session_history",
                }));
            }
        }
    }

    Ok(Json(json!({ "discovered": discovered })))
}

/// PUT /api/v1/projects/active
async fn set_active_project(
    State(state): State<ApiState>,
    Json(req): Json<SetActiveProjectRequest>,
) -> AppResult<Json<Value>> {
    ensure_table(&state.pool).await?;

    sqlx::query("UPDATE projects SET is_active = 0")
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;

    let target = fetch_project(&state.pool, req.project_id).await?;
    let Some(_) = target else {
        return Err(AppError::not_found("Project not found"));
    };

    let now = utcnow_iso();
    sqlx::query("UPDATE projects SET is_active = 1, last_accessed = ? WHERE id = ?")
        .bind(&now)
        .bind(req.project_id)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;

    let p = fetch_project(&state.pool, req.project_id)
        .await?
        .ok_or_else(|| AppError::not_found("Project not found"))?;
    Ok(Json(row_to_response(&p)))
}

/// GET /api/v1/projects/active
async fn get_active_project(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    use sqlx::Row;
    ensure_table(&state.pool).await?;
    let row = sqlx::query(
        "SELECT id, name, path, is_active, last_accessed, created_at \
         FROM projects WHERE is_active = 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;

    match row {
        Some(row) => Ok(Json(row_to_response(&ProjectRow {
            id: row.get("id"),
            name: row.get("name"),
            path: row.get("path"),
            is_active: row.get::<bool, _>("is_active"),
            last_accessed: row.get("last_accessed"),
            created_at: row.get("created_at"),
        }))),
        None => Err(AppError::not_found("No active project set")),
    }
}

/// DELETE /api/v1/projects/active
async fn clear_active_project(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    ensure_table(&state.pool).await?;
    sqlx::query("UPDATE projects SET is_active = 0")
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(Json(json!({ "message": "Active project cleared" })))
}

/// DELETE /api/v1/projects/{project_id}
async fn remove_project(
    State(state): State<ApiState>,
    axum::extract::Path(project_id): axum::extract::Path<i64>,
) -> AppResult<Json<Value>> {
    ensure_table(&state.pool).await?;

    let existing = fetch_project(&state.pool, project_id).await?;
    if existing.is_none() {
        return Err(AppError::not_found("Project not found"));
    }

    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(project_id)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;

    Ok(Json(json!({ "message": "Project removed successfully" })))
}

/// GET /api/v1/projects/{project_id}/config
async fn get_project_config(
    State(state): State<ApiState>,
    axum::extract::Path(project_id): axum::extract::Path<i64>,
) -> AppResult<Json<Value>> {
    ensure_table(&state.pool).await?;

    let project = fetch_project(&state.pool, project_id).await?;
    let Some(project) = project else {
        return Err(AppError::not_found("Project not found"));
    };

    let merged = get_merged_config(Some(&project.path));

    Ok(Json(json!({
        "project": row_to_response(&project),
        "config": merged,
    })))
}
