// PORTED: backup_service.py + api/v1/backup.py
//
// Faithful port of `backend_python/app/{services/backup_service.py,
// api/v1/backup.py}`. Route paths, request/response field names and JSON
// shapes are IDENTICAL to the Python router (the unchanged frontend was built
// against them). Archive format matches Python's `zipfile.ZipFile(...,
// ZIP_DEFLATED)` so backups created by either backend remain mutually
// restorable.
//
// NEEDS-DEP: zip = "2"
// (Python uses the stdlib `zipfile` module with `ZIP_DEFLATED`. The Rust tree
//  has no zip dependency. This module is written against the `zip` crate v2,
//  which reads/writes standard PKZIP/DEFLATE archives — byte-compatible with
//  Python `zipfile`, so existing `.zip` backups stay restorable. The
//  architect must add `zip = "2"` to backend/Cargo.toml.)

use axum::{
    Router,
    body::Body,
    extract::{Json, Path, Query, State},
    http::{StatusCode, header},
    response::Response,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::{Path as FsPath, PathBuf};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::paths;

pub fn router() -> Router<ApiState> {
    // Paths mirror Python's `APIRouter(prefix="/backup")` exactly.
    Router::new()
        .route("/list", get(list_backups))
        .route("/create", post(create_backup))
        .route("/export", post(export_config))
        .route("/{backup_id}", get(get_backup).delete(delete_backup))
        .route("/{backup_id}/contents", get(get_backup_contents))
        .route("/{backup_id}/manifest", get(get_backup_manifest))
        .route("/{backup_id}/plan", get(get_restore_plan))
        .route("/{backup_id}/validate", post(validate_backup))
        .route("/{backup_id}/download", get(download_backup))
        .route("/{backup_id}/restore", post(restore_backup))
        .route(
            "/{backup_id}/install-dependencies",
            post(install_dependencies),
        )
}

// ---- DB row -----------------------------------------------------------------

#[derive(Clone)]
struct BackupRow {
    id: i64,
    name: String,
    description: Option<String>,
    file_path: String,
    scope: String,
    project_id: Option<i64>,
    created_at: String,
    size_bytes: i64,
}

type BackupTuple = (
    i64,
    String,
    Option<String>,
    String,
    String,
    Option<i64>,
    String,
    i64,
);

fn row_from_tuple(r: BackupTuple) -> BackupRow {
    BackupRow {
        id: r.0,
        name: r.1,
        description: r.2,
        file_path: r.3,
        scope: r.4,
        project_id: r.5,
        created_at: r.6,
        size_bytes: r.7,
    }
}

/// Ensure the `backups` table exists. The Python backend relies on SQLAlchemy
/// `create_all`; the Rust backend has no migration system, so create-if-missing
/// here keeps behavior identical (schema mirrors `models/database.py::Backup`).
async fn ensure_table(pool: &sqlx::SqlitePool) -> Result<(), AppError> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS backups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            description TEXT,
            file_path TEXT NOT NULL,
            scope TEXT NOT NULL,
            project_id INTEGER,
            created_at TEXT NOT NULL,
            size_bytes INTEGER NOT NULL
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

async fn fetch_backup(pool: &sqlx::SqlitePool, id: i64) -> Result<Option<BackupRow>, AppError> {
    ensure_table(pool).await?;
    let row = sqlx::query_as::<_, BackupTuple>(
        "SELECT id, name, description, file_path, scope, project_id, created_at, size_bytes FROM backups WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(row.map(row_from_tuple))
}

// ---- request bodies ---------------------------------------------------------

#[derive(Deserialize)]
struct BackupCreate {
    name: String,
    #[serde(default)]
    description: Option<String>,
    scope: String,
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    project_id: Option<i64>,
}

#[derive(Deserialize)]
struct ExportRequest {
    paths: Vec<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize, Default)]
struct RestoreOptions {
    #[serde(default)]
    selective_restore: Option<Vec<String>>,
    #[serde(default)]
    install_dependencies: bool,
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    skip_plugins: bool,
    #[serde(default)]
    skip_skills: bool,
    #[serde(default)]
    #[allow(dead_code)]
    skip_mcp_servers: bool,
}

#[derive(Deserialize, Default)]
struct DependencyInstallRequest {
    #[serde(default = "default_true")]
    install_npm: bool,
    #[serde(default = "default_true")]
    install_pip: bool,
    #[serde(default = "default_true")]
    install_plugins: bool,
    #[serde(default)]
    skill_names: Option<Vec<String>>,
    #[serde(default)]
    plugin_names: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct ProjectPathQuery {
    #[serde(default)]
    project_path: Option<String>,
}

// ---- platform / version helpers --------------------------------------------

fn get_current_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        _ => "linux",
    }
}

fn get_claude_code_version() -> Option<String> {
    let out = std::process::Command::new("claude")
        .arg("--version")
        .output()
        .ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    } else {
        None
    }
}

// ---- path collection --------------------------------------------------------

/// Recursively collect all files under `dir` (Python `Path.rglob("*")`,
/// `is_file()` filter).
fn rglob_files(dir: &FsPath) -> Vec<PathBuf> {
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
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
    out
}

fn get_user_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for p in [
        paths::get_claude_user_config_file(),
        paths::get_claude_user_settings_file(),
        paths::get_claude_user_settings_local_file(),
    ] {
        if p.exists() {
            paths.push(p);
        }
    }

    for d in [
        paths::get_claude_user_commands_dir(),
        paths::get_claude_user_agents_dir(),
        paths::get_claude_user_skills_dir(),
        paths::get_claude_user_plugins_dir(),
    ] {
        if d.exists() {
            paths.extend(rglob_files(&d));
        }
    }

    paths
}

fn get_project_config_paths(project_path: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    let cwd = std::path::Path::new("");
    let claude_dir = paths::get_project_claude_dir(Some(project_path), cwd);
    if claude_dir.exists() {
        paths.extend(rglob_files(&claude_dir));
    }

    let mcp_file = paths::get_project_mcp_config_file(Some(project_path), cwd);
    if mcp_file.exists() {
        paths.push(mcp_file);
    }

    let claude_md = paths::get_project_claude_md_file(Some(project_path), cwd);
    if claude_md.exists() {
        paths.push(claude_md);
    }

    paths
}

// ---- manifest generation ----------------------------------------------------

fn detect_skill_dependencies(skill_path: &FsPath) -> Value {
    let skill_name = skill_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut has_package_json = false;
    let mut has_requirements_txt = false;
    let mut has_install_script = false;
    let mut dependencies: Vec<Value> = Vec::new();

    let package_json = skill_path.join("package.json");
    if package_json.exists() {
        has_package_json = true;
        if let Ok(txt) = std::fs::read_to_string(&package_json) {
            if let Ok(pkg) = serde_json::from_str::<Value>(&txt) {
                for key in ["dependencies", "devDependencies"] {
                    if let Some(map) = pkg.get(key).and_then(|v| v.as_object()) {
                        for (name, version) in map {
                            dependencies.push(json!({
                                "kind": "npm",
                                "name": name,
                                "version": version.as_str(),
                            }));
                        }
                    }
                }
            }
        }
    }

    let requirements_txt = skill_path.join("requirements.txt");
    if requirements_txt.exists() {
        has_requirements_txt = true;
        if let Ok(txt) = std::fs::read_to_string(&requirements_txt) {
            for raw in txt.lines() {
                let line = raw.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut matched = false;
                for sep in ["==", ">=", "<=", "~=", "!="] {
                    if let Some(idx) = line.find(sep) {
                        let name = line[..idx].trim();
                        let version = line[idx + sep.len()..].trim();
                        dependencies.push(json!({
                            "kind": "pip",
                            "name": name,
                            "version": version,
                        }));
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    dependencies.push(json!({
                        "kind": "pip",
                        "name": line,
                        "version": Value::Null,
                    }));
                }
            }
        }
    }

    if skill_path.join("install.sh").exists() {
        has_install_script = true;
    }

    json!({
        "name": skill_name,
        "path": skill_path.to_string_lossy(),
        "has_package_json": has_package_json,
        "has_requirements_txt": has_requirements_txt,
        "has_install_script": has_install_script,
        "dependencies": dependencies,
    })
}

fn get_plugin_install_info(plugin_name: &str, plugin_path: &FsPath) -> Value {
    let mut version: Value = Value::Null;
    let mut source: Value = Value::Null;
    let mut marketplace: Value = Value::Null;
    let mut install_command: Value = Value::Null;

    let manifest_path = plugin_path.join("manifest.json");
    if manifest_path.exists() {
        if let Ok(txt) = std::fs::read_to_string(&manifest_path) {
            if let Ok(m) = serde_json::from_str::<Value>(&txt) {
                version = m.get("version").cloned().unwrap_or(Value::Null);
                source = m.get("source").cloned().unwrap_or(Value::Null);
            }
        }
    }

    let source_file = plugin_path.join(".source");
    if source_file.exists() {
        if let Ok(txt) = std::fs::read_to_string(&source_file) {
            if let Ok(sd) = serde_json::from_str::<Value>(&txt) {
                marketplace = sd.get("marketplace").cloned().unwrap_or(Value::Null);
                install_command = sd.get("install_command").cloned().unwrap_or(Value::Null);
            }
        }
    }

    json!({
        "name": plugin_name,
        "version": version,
        "source": source,
        "install_command": install_command,
        "marketplace": marketplace,
    })
}

fn detect_mcp_server_info(name: &str, config: &Value, scope: &str) -> Value {
    let url = config.get("url").and_then(|v| v.as_str());
    let server_type = if let Some(u) = url {
        if u.to_lowercase().contains("sse") {
            "sse"
        } else {
            "http"
        }
    } else {
        "stdio"
    };

    let command = config.get("command").and_then(|v| v.as_str());
    let args = config.get("args").cloned();
    let requires_npm_install = command.map(|c| c.starts_with("npx")).unwrap_or(false);

    json!({
        "name": name,
        "type": server_type,
        "scope": scope,
        "command": command,
        "args": args.unwrap_or(Value::Null),
        "url": url,
        "requires_npm_install": requires_npm_install,
    })
}

fn generate_manifest(paths_list: &[PathBuf], scope: &str) -> Value {
    let home = paths::get_user_home();

    let mut files: Vec<Value> = Vec::new();
    for path in paths_list {
        let rel = match path.strip_prefix(&home) {
            Ok(r) => r.to_string_lossy().into_owned(),
            Err(_) => path.to_string_lossy().into_owned(),
        };
        files.push(Value::String(rel));
    }

    let mut skills: Vec<Value> = Vec::new();
    let skills_dir = paths::get_claude_user_skills_dir();
    if skills_dir.exists() {
        if let Ok(rd) = std::fs::read_dir(&skills_dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    skills.push(detect_skill_dependencies(&p));
                }
            }
        }
    }

    let mut plugins: Vec<Value> = Vec::new();
    let plugins_dir = paths::get_claude_user_plugins_dir();
    if plugins_dir.exists() {
        if let Ok(rd) = std::fs::read_dir(&plugins_dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let nm = p
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    plugins.push(get_plugin_install_info(&nm, &p));
                }
            }
        }
    }

    let mut mcp_servers: Vec<Value> = Vec::new();
    let config_file = paths::get_claude_user_config_file();
    if config_file.exists() {
        if let Ok(txt) = std::fs::read_to_string(&config_file) {
            if let Ok(config) = serde_json::from_str::<Value>(&txt) {
                if let Some(map) = config.get("mcpServers").and_then(|v| v.as_object()) {
                    for (name, srv) in map {
                        mcp_servers.push(detect_mcp_server_info(name, srv, "user"));
                    }
                }
            }
        }
    }

    let mut agents: Vec<Value> = Vec::new();
    let agents_dir = paths::get_claude_user_agents_dir();
    if agents_dir.exists() {
        if let Ok(rd) = std::fs::read_dir(&agents_dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("md") {
                    if let Some(stem) = p.file_stem() {
                        agents.push(Value::String(stem.to_string_lossy().into_owned()));
                    }
                }
            }
        }
    }

    let mut commands: Vec<Value> = Vec::new();
    let commands_dir = paths::get_claude_user_commands_dir();
    if commands_dir.exists() {
        let mut md_files = Vec::new();
        let mut stack = vec![commands_dir.clone()];
        while let Some(d) = stack.pop() {
            if let Ok(rd) = std::fs::read_dir(&d) {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if p.extension().and_then(|e| e.to_str()) == Some("md") {
                        md_files.push(p);
                    }
                }
            }
        }
        for cmd_file in md_files {
            match cmd_file.strip_prefix(&commands_dir) {
                Ok(rel) => commands.push(Value::String(rel.to_string_lossy().replace(".md", ""))),
                Err(_) => {
                    if let Some(stem) = cmd_file.file_stem() {
                        commands.push(Value::String(stem.to_string_lossy().into_owned()));
                    }
                }
            }
        }
    }

    json!({
        "version": "1.0",
        "created_at": chrono::Utc::now().naive_utc().format("%Y-%m-%dT%H:%M:%S%.6f").to_string(),
        "claude_code_version": get_claude_code_version(),
        "platform": get_current_platform(),
        "scope": scope,
        "contents": {
            "files": files,
            "skills": skills,
            "plugins": plugins,
            "mcp_servers": mcp_servers,
            "agents": agents,
            "commands": commands,
        }
    })
}

// ---- archive create ---------------------------------------------------------

fn create_archive(
    name: &str,
    file_paths: &[PathBuf],
    scope: &str,
    base_path: Option<&FsPath>,
) -> Result<(PathBuf, i64, Value), AppError> {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let archive_name = format!("{}_{}.zip", name, timestamp);
    let backup_dir = paths::get_user_home()
        .join(".claude-registry")
        .join("backups");
    std::fs::create_dir_all(&backup_dir).map_err(|e| AppError::internal(e.to_string()))?;
    let archive_path = backup_dir.join(&archive_name);

    let manifest = generate_manifest(file_paths, scope);

    let file =
        std::fs::File::create(&archive_path).map_err(|e| AppError::internal(e.to_string()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("manifest.json", options)
        .map_err(|e| AppError::internal(e.to_string()))?;
    let manifest_str =
        serde_json::to_string_pretty(&manifest).map_err(|e| AppError::internal(e.to_string()))?;
    zip.write_all(manifest_str.as_bytes())
        .map_err(|e| AppError::internal(e.to_string()))?;

    let home = paths::get_user_home();
    for fp in file_paths {
        let arcname = if let Some(bp) = base_path {
            match fp.strip_prefix(bp) {
                Ok(r) => r.to_string_lossy().into_owned(),
                Err(_) => fp.to_string_lossy().into_owned(),
            }
        } else {
            match fp.strip_prefix(&home) {
                Ok(r) => r.to_string_lossy().into_owned(),
                Err(_) => fp.to_string_lossy().into_owned(),
            }
        };
        let data = std::fs::read(fp).map_err(|e| AppError::internal(e.to_string()))?;
        zip.start_file(arcname, options)
            .map_err(|e| AppError::internal(e.to_string()))?;
        zip.write_all(&data)
            .map_err(|e| AppError::internal(e.to_string()))?;
    }

    zip.finish()
        .map_err(|e| AppError::internal(e.to_string()))?;

    let size_bytes = std::fs::metadata(&archive_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    Ok((archive_path, size_bytes, manifest))
}

// ---- archive read helpers ---------------------------------------------------

fn get_manifest_from_backup(file_path: &str) -> Option<Value> {
    let archive_path = PathBuf::from(file_path);
    if !archive_path.exists() {
        return None;
    }
    let file = std::fs::File::open(&archive_path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut mf = zip.by_name("manifest.json").ok()?;
    let mut buf = String::new();
    mf.read_to_string(&mut buf).ok()?;
    serde_json::from_str(&buf).ok()
}

fn list_archive_members(file_path: &str) -> Vec<String> {
    let archive_path = PathBuf::from(file_path);
    if !archive_path.exists() {
        return Vec::new();
    }
    let Ok(file) = std::fs::File::open(&archive_path) else {
        return Vec::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for i in 0..zip.len() {
        if let Ok(f) = zip.by_index(i) {
            let n = f.name().to_string();
            if n != "manifest.json" {
                names.push(n);
            }
        }
    }
    names
}

// ---- response builders ------------------------------------------------------

fn backup_basic_response(b: &BackupRow) -> Value {
    json!({
        "id": b.id,
        "name": b.name,
        "description": b.description,
        "scope": b.scope,
        "file_path": b.file_path,
        "project_id": b.project_id,
        "created_at": b.created_at,
        "size_bytes": b.size_bytes,
    })
}

fn backup_full_response(b: &BackupRow, manifest: Option<&Value>) -> Value {
    let mut resp = json!({
        "id": b.id,
        "name": b.name,
        "description": b.description,
        "scope": b.scope,
        "file_path": b.file_path,
        "project_id": b.project_id,
        "created_at": b.created_at,
        "size_bytes": b.size_bytes,
        "has_dependencies": false,
        "skill_count": 0,
        "plugin_count": 0,
        "mcp_server_count": 0,
    });

    if let Some(m) = manifest {
        let contents = m.get("contents").cloned().unwrap_or(json!({}));
        let skills = contents
            .get("skills")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let plugins = contents
            .get("plugins")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mcp = contents
            .get("mcp_servers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let has_dependencies = skills.iter().any(|s| {
            let dep_len = s
                .get("dependencies")
                .and_then(|d| d.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let has_script = s
                .get("has_install_script")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            dep_len > 0 || has_script
        }) || plugins.iter().any(|p| {
            !p.get("install_command")
                .map(|v| v.is_null())
                .unwrap_or(true)
        });

        resp["skill_count"] = json!(skills.len());
        resp["plugin_count"] = json!(plugins.len());
        resp["mcp_server_count"] = json!(mcp.len());
        resp["has_dependencies"] = json!(has_dependencies);
    }

    resp
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/backup/list
async fn list_backups(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    ensure_table(&state.pool).await?;
    let rows = sqlx::query_as::<_, BackupTuple>(
        "SELECT id, name, description, file_path, scope, project_id, created_at, size_bytes FROM backups ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;

    let backups: Vec<Value> = rows
        .into_iter()
        .map(|r| backup_basic_response(&row_from_tuple(r)))
        .collect();

    Ok(Json(json!({ "backups": backups })))
}

/// POST /api/v1/backup/create
async fn create_backup(
    State(state): State<ApiState>,
    Json(req): Json<BackupCreate>,
) -> AppResult<(StatusCode, Json<Value>)> {
    if !["full", "user", "project"].contains(&req.scope.as_str()) {
        return Err(AppError::bad_request(
            "Scope must be 'full', 'user', or 'project'",
        ));
    }
    if (req.scope == "full" || req.scope == "project") && req.project_path.is_none() {
        return Err(AppError::bad_request(
            "project_path is required for full or project scope",
        ));
    }

    let mut file_paths: Vec<PathBuf> = Vec::new();
    if req.scope == "full" || req.scope == "user" {
        file_paths.extend(get_user_config_paths());
    }
    if (req.scope == "full" || req.scope == "project") && req.project_path.is_some() {
        file_paths.extend(get_project_config_paths(req.project_path.as_ref().unwrap()));
    }

    if file_paths.is_empty() {
        return Err(AppError::bad_request(
            "No configuration files found to backup",
        ));
    }

    let base_path: Option<PathBuf> = if req.scope == "project" {
        req.project_path.as_ref().map(PathBuf::from)
    } else {
        None
    };

    let (archive_path, size_bytes, manifest) =
        create_archive(&req.name, &file_paths, &req.scope, base_path.as_deref())
            .map_err(|e| AppError::internal(format!("Failed to create backup: {}", e.detail)))?;

    ensure_table(&state.pool).await?;
    let created_at = chrono::Utc::now()
        .naive_utc()
        .format("%Y-%m-%dT%H:%M:%S%.6f")
        .to_string();
    let result = sqlx::query(
        "INSERT INTO backups (name, description, file_path, scope, project_id, created_at, size_bytes) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(archive_path.to_string_lossy().as_ref())
    .bind(&req.scope)
    .bind(req.project_id)
    .bind(&created_at)
    .bind(size_bytes)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError::internal(format!("Failed to create backup: {}", e)))?;

    let id = result.last_insert_rowid();
    let row = BackupRow {
        id,
        name: req.name.clone(),
        description: req.description.clone(),
        file_path: archive_path.to_string_lossy().into_owned(),
        scope: req.scope.clone(),
        project_id: req.project_id,
        created_at,
        size_bytes,
    };

    Ok((
        StatusCode::CREATED,
        Json(backup_full_response(&row, Some(&manifest))),
    ))
}

/// GET /api/v1/backup/{backup_id}
async fn get_backup(
    State(state): State<ApiState>,
    Path(backup_id): Path<i64>,
) -> AppResult<Json<Value>> {
    let backup = fetch_backup(&state.pool, backup_id)
        .await?
        .ok_or_else(|| AppError::not_found("Backup not found"))?;
    let manifest = get_manifest_from_backup(&backup.file_path);
    Ok(Json(backup_full_response(&backup, manifest.as_ref())))
}

/// GET /api/v1/backup/{backup_id}/contents
async fn get_backup_contents(
    State(state): State<ApiState>,
    Path(backup_id): Path<i64>,
) -> AppResult<Json<Value>> {
    let backup = fetch_backup(&state.pool, backup_id)
        .await?
        .ok_or_else(|| AppError::not_found("Backup not found"))?;
    let files = list_archive_members(&backup.file_path);
    Ok(Json(json!({ "files": files })))
}

/// GET /api/v1/backup/{backup_id}/manifest
async fn get_backup_manifest(
    State(state): State<ApiState>,
    Path(backup_id): Path<i64>,
) -> AppResult<Json<Value>> {
    let backup = fetch_backup(&state.pool, backup_id)
        .await?
        .ok_or_else(|| AppError::not_found("Backup not found"))?;
    let manifest = get_manifest_from_backup(&backup.file_path)
        .ok_or_else(|| AppError::not_found("Manifest not found in backup (older backup format)"))?;
    Ok(Json(manifest))
}

/// GET /api/v1/backup/{backup_id}/plan
async fn get_restore_plan(
    State(state): State<ApiState>,
    Path(backup_id): Path<i64>,
    Query(_q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let backup = fetch_backup(&state.pool, backup_id)
        .await?
        .ok_or_else(|| AppError::not_found("Backup not found"))?;

    let archive_path = PathBuf::from(&backup.file_path);
    if !archive_path.exists() {
        return Err(AppError::not_found("Backup not found"));
    }

    let current_platform = get_current_platform();
    let manifest = get_manifest_from_backup(&backup.file_path);

    let platform_backup = manifest
        .as_ref()
        .and_then(|m| m.get("platform").and_then(|v| v.as_str()))
        .unwrap_or("unknown")
        .to_string();

    let mut warnings: Vec<Value> = Vec::new();
    let mut platform_compatible = true;
    if let Some(m) = &manifest {
        if let Some(pb) = m.get("platform").and_then(|v| v.as_str()) {
            if pb != current_platform {
                platform_compatible = false;
                warnings.push(json!({
                    "type": "platform",
                    "message": format!("Backup was created on {}, current platform is {}. Some paths or scripts may not work correctly.", pb, current_platform),
                    "severity": "warning",
                }));
            }
        }
    }

    let files_to_restore: Vec<Value> = list_archive_members(&backup.file_path)
        .into_iter()
        .map(Value::String)
        .collect();

    let mut skills_to_restore: Vec<Value> = Vec::new();
    let mut plugins_to_restore: Vec<Value> = Vec::new();
    let mut mcp_servers_to_restore: Vec<Value> = Vec::new();
    let mut dependencies: Vec<Value> = Vec::new();
    let mut manual_steps: Vec<Value> = Vec::new();

    if let Some(m) = &manifest {
        let contents = m.get("contents").cloned().unwrap_or(json!({}));
        skills_to_restore = contents
            .get("skills")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        plugins_to_restore = contents
            .get("plugins")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        mcp_servers_to_restore = contents
            .get("mcp_servers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        for skill in &skills_to_restore {
            let sname = skill.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(deps) = skill.get("dependencies").and_then(|v| v.as_array()) {
                for dep in deps {
                    dependencies.push(json!({
                        "kind": dep.get("kind"),
                        "name": dep.get("name"),
                        "version": dep.get("version"),
                        "source": format!("skill:{}", sname),
                        "install_command": Value::Null,
                    }));
                }
            }
            if skill
                .get("has_install_script")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                manual_steps.push(Value::String(format!(
                    "Run install.sh for skill '{}'",
                    sname
                )));
            }
        }

        for plugin in &plugins_to_restore {
            let pname = plugin.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let install_command = plugin.get("install_command");
            let source = plugin.get("source");
            if install_command.map(|v| !v.is_null()).unwrap_or(false) {
                dependencies.push(json!({
                    "kind": "plugin",
                    "name": pname,
                    "version": Value::Null,
                    "source": plugin.get("marketplace"),
                    "install_command": install_command,
                }));
            } else if source.map(|v| !v.is_null()).unwrap_or(false) {
                let src = source.and_then(|v| v.as_str()).unwrap_or("marketplace");
                let src = if src.is_empty() { "marketplace" } else { src };
                manual_steps.push(Value::String(format!(
                    "Reinstall plugin '{}' from {}",
                    pname, src
                )));
            }
        }

        for mcp in &mcp_servers_to_restore {
            let requires = mcp
                .get("requires_npm_install")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if requires {
                let mname = mcp.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let pkg_name = mcp
                    .get("args")
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| mname.to_string());
                dependencies.push(json!({
                    "kind": "mcp_npm",
                    "name": pkg_name,
                    "version": Value::Null,
                    "source": format!("mcp:{}", mname),
                    "install_command": Value::Null,
                }));
            }
        }
    }

    let has_dependencies = !dependencies.is_empty();

    Ok(Json(json!({
        "backup_id": backup.id,
        "backup_name": backup.name,
        "created_at": backup.created_at,
        "scope": backup.scope,
        "platform_current": current_platform,
        "platform_backup": platform_backup,
        "platform_compatible": platform_compatible,
        "files_to_restore": files_to_restore,
        "skills_to_restore": skills_to_restore,
        "plugins_to_restore": plugins_to_restore,
        "mcp_servers_to_restore": mcp_servers_to_restore,
        "dependencies": dependencies,
        "has_dependencies": has_dependencies,
        "warnings": warnings,
        "manual_steps": manual_steps,
    })))
}

/// POST /api/v1/backup/{backup_id}/validate
async fn validate_backup(
    State(state): State<ApiState>,
    Path(backup_id): Path<i64>,
) -> AppResult<Json<Value>> {
    let backup = match fetch_backup(&state.pool, backup_id).await? {
        Some(b) => b,
        None => {
            return Ok(Json(
                json!({ "valid": false, "issues": ["Backup not found"] }),
            ));
        }
    };

    let archive_path = PathBuf::from(&backup.file_path);
    if !archive_path.exists() {
        return Ok(Json(
            json!({ "valid": false, "issues": ["Backup file not found on disk"] }),
        ));
    }

    let mut issues: Vec<String> = Vec::new();

    let file = match std::fs::File::open(&archive_path) {
        Ok(f) => f,
        Err(_) => {
            return Ok(Json(
                json!({ "valid": false, "issues": ["Backup file is corrupted"] }),
            ));
        }
    };
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(z) => z,
        Err(_) => {
            return Ok(Json(
                json!({ "valid": false, "issues": ["Backup file is corrupted"] }),
            ));
        }
    };

    let mut has_manifest = false;
    let mut bad_file: Option<String> = None;
    for i in 0..zip.len() {
        match zip.by_index(i) {
            Ok(mut f) => {
                let name = f.name().to_string();
                if name == "manifest.json" {
                    has_manifest = true;
                }
                let mut sink = Vec::new();
                if f.read_to_end(&mut sink).is_err() {
                    bad_file = Some(name);
                    break;
                }
            }
            Err(_) => {
                return Ok(Json(
                    json!({ "valid": false, "issues": ["Backup file is corrupted"] }),
                ));
            }
        }
    }

    if let Some(bf) = bad_file {
        issues.push(format!("Corrupted file in archive: {}", bf));
    }
    if !has_manifest {
        issues.push("Backup is missing manifest.json (older format)".to_string());
    }

    Ok(Json(json!({
        "valid": issues.is_empty(),
        "issues": issues,
    })))
}

/// GET /api/v1/backup/{backup_id}/download
async fn download_backup(
    State(state): State<ApiState>,
    Path(backup_id): Path<i64>,
) -> AppResult<Response> {
    let backup = fetch_backup(&state.pool, backup_id)
        .await?
        .ok_or_else(|| AppError::not_found("Backup not found"))?;

    let file_path = PathBuf::from(&backup.file_path);
    if !file_path.exists() {
        return Err(AppError::not_found("Backup file not found"));
    }

    let data = std::fs::read(&file_path).map_err(|e| AppError::internal(e.to_string()))?;
    let filename = file_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup.zip".to_string());

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(Body::from(data))
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(resp)
}

/// POST /api/v1/backup/{backup_id}/restore
async fn restore_backup(
    State(state): State<ApiState>,
    Path(backup_id): Path<i64>,
    Query(q): Query<ProjectPathQuery>,
    body: Option<Json<RestoreOptions>>,
) -> AppResult<Json<Value>> {
    let options = body.map(|Json(o)| o).unwrap_or_default();

    let backup = match fetch_backup(&state.pool, backup_id).await? {
        Some(b) => b,
        None => return Err(AppError::not_found("Backup not found")),
    };

    let archive_path = PathBuf::from(&backup.file_path);
    if !archive_path.exists() {
        return Ok(Json(json!({
            "success": false,
            "message": format!("Backup file not found: {}", archive_path.to_string_lossy()),
            "files_restored": 0,
            "files_skipped": 0,
            "dry_run": options.dry_run,
            "dependency_results": [],
            "manual_steps": [],
        })));
    }

    let target_path: PathBuf = if backup.scope == "project" && q.project_path.is_some() {
        PathBuf::from(q.project_path.as_ref().unwrap())
    } else {
        paths::get_user_home()
    };

    let manifest = get_manifest_from_backup(&backup.file_path);

    let mut files_restored = 0i64;
    let mut files_skipped = 0i64;

    let file = std::fs::File::open(&archive_path)
        .map_err(|e| AppError::internal(format!("Failed to restore backup: {}", e)))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::internal(format!("Failed to restore backup: {}", e)))?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::internal(format!("Failed to restore backup: {}", e)))?;
        let member = entry.name().to_string();

        if member == "manifest.json" {
            continue;
        }

        if let Some(sel) = &options.selective_restore {
            if !sel.contains(&member) {
                files_skipped += 1;
                continue;
            }
        }

        if options.skip_skills && member.contains(".claude/skills/") {
            files_skipped += 1;
            continue;
        }

        if options.skip_plugins && member.contains(".claude/plugins/") {
            files_skipped += 1;
            continue;
        }

        let member_target = target_path.join(&member);

        if options.dry_run {
            files_restored += 1;
            continue;
        }

        if let Some(parent) = member_target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::internal(format!("Failed to restore backup: {}", e)))?;
        }

        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| AppError::internal(format!("Failed to restore backup: {}", e)))?;
        std::fs::write(&member_target, &buf)
            .map_err(|e| AppError::internal(format!("Failed to restore backup: {}", e)))?;

        files_restored += 1;
    }

    let mut dependency_results: Vec<Value> = Vec::new();
    if options.install_dependencies && !options.dry_run && manifest.is_some() {
        let dep = run_install_dependencies(
            manifest.as_ref().unwrap(),
            &DependencyInstallRequest {
                install_npm: true,
                install_pip: true,
                install_plugins: true,
                skill_names: None,
                plugin_names: None,
            },
        );
        if let Some(installed) = dep.get("installed").and_then(|v| v.as_array()) {
            dependency_results.extend(installed.iter().cloned());
        }
        if let Some(failed) = dep.get("failed").and_then(|v| v.as_array()) {
            dependency_results.extend(failed.iter().cloned());
        }
    }

    let mut manual_steps: Vec<Value> = Vec::new();
    if let Some(m) = &manifest {
        if let Some(skills) = m
            .get("contents")
            .and_then(|c| c.get("skills"))
            .and_then(|v| v.as_array())
        {
            for skill in skills {
                if skill
                    .get("has_install_script")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    let sname = skill.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    manual_steps.push(Value::String(format!(
                        "Run: cd ~/.claude/skills/{} && ./install.sh",
                        sname
                    )));
                }
            }
        }
    }

    let message = format!(
        "{} {} files{}",
        if options.dry_run {
            "Would restore"
        } else {
            "Restored"
        },
        files_restored,
        if files_skipped != 0 {
            format!(", skipped {}", files_skipped)
        } else {
            String::new()
        }
    );

    Ok(Json(json!({
        "success": true,
        "message": message,
        "files_restored": files_restored,
        "files_skipped": files_skipped,
        "dry_run": options.dry_run,
        "dependency_results": dependency_results,
        "manual_steps": manual_steps,
    })))
}

fn install_skill_dependencies(skill_path: &FsPath) -> (bool, String) {
    let mut logs: Vec<String> = Vec::new();
    let mut success = true;
    let skill_name = skill_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    if skill_path.join("package.json").exists() {
        match std::process::Command::new("npm")
            .arg("install")
            .current_dir(skill_path)
            .output()
        {
            Ok(out) => {
                logs.push(format!("npm install in {}:", skill_name));
                logs.push(String::from_utf8_lossy(&out.stdout).into_owned());
                if !out.status.success() {
                    logs.push(format!("Error: {}", String::from_utf8_lossy(&out.stderr)));
                    success = false;
                }
            }
            Err(e) => {
                logs.push(format!("npm install failed: {}", e));
                success = false;
            }
        }
    }

    if skill_path.join("requirements.txt").exists() {
        match std::process::Command::new("pip")
            .args(["install", "-r", "requirements.txt"])
            .current_dir(skill_path)
            .output()
        {
            Ok(out) => {
                logs.push(format!("pip install in {}:", skill_name));
                logs.push(String::from_utf8_lossy(&out.stdout).into_owned());
                if !out.status.success() {
                    logs.push(format!("Error: {}", String::from_utf8_lossy(&out.stderr)));
                    success = false;
                }
            }
            Err(e) => {
                logs.push(format!("pip install failed: {}", e));
                success = false;
            }
        }
    }

    (success, logs.join("\n"))
}

fn reinstall_plugin(name: &str, install_command: &str) -> (bool, String) {
    match std::process::Command::new("sh")
        .arg("-c")
        .arg(install_command)
        .output()
    {
        Ok(out) => {
            let mut logs = format!(
                "Installing {}:\n{}",
                name,
                String::from_utf8_lossy(&out.stdout)
            );
            if !out.status.success() {
                logs += &format!("\nError: {}", String::from_utf8_lossy(&out.stderr));
                return (false, logs);
            }
            (true, logs)
        }
        Err(e) => (false, format!("Failed to install {}: {}", name, e)),
    }
}

fn run_install_dependencies(manifest: &Value, request: &DependencyInstallRequest) -> Value {
    let mut installed: Vec<Value> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    let mut logs: Vec<String> = Vec::new();

    let contents = manifest.get("contents").cloned().unwrap_or(json!({}));

    if request.install_npm || request.install_pip {
        let skills_dir = paths::get_claude_user_skills_dir();
        if let Some(skills) = contents.get("skills").and_then(|v| v.as_array()) {
            for skill_info in skills {
                let sname = skill_info
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if let Some(names) = &request.skill_names {
                    if !names.contains(&sname) {
                        continue;
                    }
                }

                let skill_path = skills_dir.join(&sname);
                if !skill_path.exists() {
                    failed.push(json!({
                        "name": sname,
                        "kind": "skill",
                        "success": false,
                        "message": format!("Skill directory not found: {}", skill_path.to_string_lossy()),
                    }));
                    continue;
                }

                let (ok, log) = install_skill_dependencies(&skill_path);
                logs.push(log);
                let status = json!({
                    "name": sname,
                    "kind": "skill",
                    "success": ok,
                    "message": if ok { "Dependencies installed" } else { "Installation failed" },
                });
                if ok {
                    installed.push(status);
                } else {
                    failed.push(status);
                }
            }
        }
    }

    if request.install_plugins {
        if let Some(plugins) = contents.get("plugins").and_then(|v| v.as_array()) {
            for plugin_info in plugins {
                let pname = plugin_info
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if let Some(names) = &request.plugin_names {
                    if !names.contains(&pname) {
                        continue;
                    }
                }

                let install_command = plugin_info.get("install_command").and_then(|v| v.as_str());
                if let Some(cmd) = install_command {
                    let (ok, log) = reinstall_plugin(&pname, cmd);
                    logs.push(log);
                    let status = json!({
                        "name": pname,
                        "kind": "plugin",
                        "success": ok,
                        "message": if ok { "Plugin reinstalled" } else { "Reinstall failed" },
                    });
                    if ok {
                        installed.push(status);
                    } else {
                        failed.push(status);
                    }
                }
            }
        }
    }

    let success = failed.is_empty();
    let message = format!(
        "Installed {} dependencies, {} failed",
        installed.len(),
        failed.len()
    );

    json!({
        "success": success,
        "message": message,
        "installed": installed,
        "failed": failed,
        "logs": logs.join("\n"),
    })
}

/// POST /api/v1/backup/{backup_id}/install-dependencies
async fn install_dependencies(
    State(state): State<ApiState>,
    Path(backup_id): Path<i64>,
    Json(request): Json<DependencyInstallRequest>,
) -> AppResult<Json<Value>> {
    let backup = match fetch_backup(&state.pool, backup_id).await? {
        Some(b) => b,
        None => {
            return Ok(Json(json!({
                "success": false,
                "message": "Backup not found",
                "installed": [],
                "failed": [],
                "logs": "",
            })));
        }
    };

    let manifest = match get_manifest_from_backup(&backup.file_path) {
        Some(m) => m,
        None => {
            return Ok(Json(json!({
                "success": false,
                "message": "No manifest in backup",
                "installed": [],
                "failed": [],
                "logs": "",
            })));
        }
    };

    Ok(Json(run_install_dependencies(&manifest, &request)))
}

/// DELETE /api/v1/backup/{backup_id}
async fn delete_backup(
    State(state): State<ApiState>,
    Path(backup_id): Path<i64>,
) -> AppResult<StatusCode> {
    let backup = fetch_backup(&state.pool, backup_id)
        .await?
        .ok_or_else(|| AppError::not_found("Backup not found"))?;

    let archive_path = PathBuf::from(&backup.file_path);
    if archive_path.exists() {
        let _ = std::fs::remove_file(&archive_path);
    }

    sqlx::query("DELETE FROM backups WHERE id = ?")
        .bind(backup_id)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/backup/export
async fn export_config(
    State(_state): State<ApiState>,
    Json(req): Json<ExportRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let valid_paths: Vec<PathBuf> = req
        .paths
        .iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect();

    if valid_paths.is_empty() {
        return Err(AppError::bad_request("No valid paths to export"));
    }

    let name = req.name.unwrap_or_else(|| "export".to_string());
    let (archive_path, size_bytes, _) = create_archive(&name, &valid_paths, "export", None)
        .map_err(|e| AppError::internal(format!("Failed to export config: {}", e.detail)))?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "file_path": archive_path.to_string_lossy(),
            "size_bytes": size_bytes,
        })),
    ))
}
