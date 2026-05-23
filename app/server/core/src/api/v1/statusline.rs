// PORTED: statusline_service.py + api/v1/statusline.py

use axum::{
    Router,
    extract::{Json, Path},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::paths;

pub fn router() -> Router<ApiState> {
    // Python: APIRouter(prefix="/statusline"); nested under /statusline.
    // `@router.get("")` / `@router.put("")` -> the nested base path.
    Router::new()
        .route(
            "/",
            get(get_statusline_config).put(update_statusline_config),
        )
        .route("/presets", get(get_statusline_presets))
        .route("/apply-preset/{preset_id}", post(apply_statusline_preset))
        .route("/script", post(save_custom_script))
        .route("/preview", post(preview_statusline_script))
        .route("/check-nodejs", get(check_nodejs))
        .route("/powerline-presets", get(get_powerline_presets))
        .route("/apply-powerline/{preset_id}", post(apply_powerline_preset))
}

// ---- preset data (verbatim from statusline_service.py) ----------------------

struct Preset {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    script: &'static str,
}

struct Powerline {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    theme: &'static str,
    style: &'static str,
    command: &'static str,
}

const MOCK_PREVIEW_DATA: &str = r#"{"model": {"display_name": "claude-sonnet-4-20250514"}, "workspace": {"current_dir": "/home/user/my-project"}, "context_window": {"used": 45000, "max": 200000}}"#;

const SCRIPT_SIMPLE: &str = "#!/bin/bash\ninput=$(cat)\nMODEL_DISPLAY=$(echo \"$input\" | jq -r '.model.display_name')\nCURRENT_DIR=$(echo \"$input\" | jq -r '.workspace.current_dir')\necho \"[$MODEL_DISPLAY] \u{1F4C1} ${CURRENT_DIR##*/}\"\n";

const SCRIPT_GIT_AWARE: &str = "#!/bin/bash\ninput=$(cat)\nMODEL_DISPLAY=$(echo \"$input\" | jq -r '.model.display_name')\nCURRENT_DIR=$(echo \"$input\" | jq -r '.workspace.current_dir')\nGIT_BRANCH=\"\"\nif git rev-parse --git-dir > /dev/null 2>&1; then\n    BRANCH=$(git branch --show-current 2>/dev/null)\n    if [ -n \"$BRANCH\" ]; then\n        GIT_BRANCH=\" | \u{1F33F} $BRANCH\"\n    fi\nfi\necho \"[$MODEL_DISPLAY] \u{1F4C1} ${CURRENT_DIR##*/}$GIT_BRANCH\"\n";

const SCRIPT_MINIMAL: &str = "#!/bin/bash\ninput=$(cat)\nMODEL_DISPLAY=$(echo \"$input\" | jq -r '.model.display_name')\necho \"$MODEL_DISPLAY\"\n";

const SCRIPT_FULL_CONTEXT: &str = "#!/bin/bash\ninput=$(cat)\nMODEL_DISPLAY=$(echo \"$input\" | jq -r '.model.display_name')\nCURRENT_DIR=$(echo \"$input\" | jq -r '.workspace.current_dir')\nCONTEXT_USED=$(echo \"$input\" | jq -r '.context_window.used // 0')\nCONTEXT_MAX=$(echo \"$input\" | jq -r '.context_window.max // 200000')\n\n# Calculate percentage\nif [ \"$CONTEXT_MAX\" -gt 0 ]; then\n    PERCENT=$((CONTEXT_USED * 100 / CONTEXT_MAX))\nelse\n    PERCENT=0\nfi\n\n# Git branch\nGIT_BRANCH=\"\"\nif git rev-parse --git-dir > /dev/null 2>&1; then\n    BRANCH=$(git branch --show-current 2>/dev/null)\n    if [ -n \"$BRANCH\" ]; then\n        GIT_BRANCH=\" \u{1F33F} $BRANCH\"\n    fi\nfi\n\necho \"[$MODEL_DISPLAY] \u{1F4C1} ${CURRENT_DIR##*/}$GIT_BRANCH | \u{1F4CA} ${PERCENT}%\"\n";

fn statusline_presets() -> [Preset; 4] {
    [
        Preset {
            id: "simple",
            name: "Simple",
            description: "Shows model name and current directory",
            script: SCRIPT_SIMPLE,
        },
        Preset {
            id: "git-aware",
            name: "Git Aware",
            description: "Shows model, directory, and git branch",
            script: SCRIPT_GIT_AWARE,
        },
        Preset {
            id: "minimal",
            name: "Minimal",
            description: "Just the model name",
            script: SCRIPT_MINIMAL,
        },
        Preset {
            id: "full-context",
            name: "Full Context",
            description: "Model, directory, git branch, and context usage",
            script: SCRIPT_FULL_CONTEXT,
        },
    ]
}

fn powerline_presets() -> [Powerline; 6] {
    [
        Powerline {
            id: "powerline-dark",
            name: "Dark Powerline",
            description: "Classic dark theme with powerline separators",
            theme: "dark",
            style: "powerline",
            command: "npx -y @owloops/claude-powerline@latest --theme=dark --style=powerline",
        },
        Powerline {
            id: "powerline-light",
            name: "Light Powerline",
            description: "Clean light theme with powerline separators",
            theme: "light",
            style: "powerline",
            command: "npx -y @owloops/claude-powerline@latest --theme=light --style=powerline",
        },
        Powerline {
            id: "powerline-nord",
            name: "Nord Minimal",
            description: "Popular Nord color scheme with minimal style",
            theme: "nord",
            style: "minimal",
            command: "npx -y @owloops/claude-powerline@latest --theme=nord --style=minimal",
        },
        Powerline {
            id: "powerline-tokyo",
            name: "Tokyo Night",
            description: "Tokyo Night theme with powerline separators",
            theme: "tokyo-night",
            style: "powerline",
            command: "npx -y @owloops/claude-powerline@latest --theme=tokyo-night --style=powerline",
        },
        Powerline {
            id: "powerline-rose",
            name: "Rose Pine Capsule",
            description: "Rose Pine theme with capsule-style segments",
            theme: "rose-pine",
            style: "capsule",
            command: "npx -y @owloops/claude-powerline@latest --theme=rose-pine --style=capsule",
        },
        Powerline {
            id: "powerline-gruvbox",
            name: "Gruvbox Minimal",
            description: "Retro Gruvbox theme with minimal style",
            theme: "gruvbox",
            style: "minimal",
            command: "npx -y @owloops/claude-powerline@latest --theme=gruvbox --style=minimal",
        },
    ]
}

// ---- helpers ----------------------------------------------------------------

fn default_script_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".claude")
        .join("statusline.sh")
}

fn expanduser(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~") {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        if rest.is_empty() {
            return home;
        }
        if let Some(r) = rest.strip_prefix('/') {
            return home.join(r);
        }
        // `~user` form is not expanded by Python's expanduser for unknown user;
        // fall through to literal.
    }
    PathBuf::from(p)
}

#[derive(Deserialize)]
struct StatusLineUpdate {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    padding: Option<i64>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Deserialize)]
struct PreviewRequest {
    script: String,
}

/// Port of `StatusLineService.get_config` -> StatusLineConfig JSON.
fn get_config() -> Value {
    let settings_file = paths::get_claude_user_settings_file();

    // Defaults from StatusLineConfig(enabled=False): type stays "command".
    let mut enabled = false;
    let mut typ = "command".to_string();
    let mut command: Option<String> = None;
    let mut padding: Option<i64> = None;

    if settings_file.exists() {
        if let Ok(text) = std::fs::read_to_string(&settings_file) {
            if let Ok(settings) = serde_json::from_str::<Value>(&text) {
                if let Some(sl) = settings.get("statusLine") {
                    // Python `if status_line:` is falsy for {}/null/false.
                    let truthy = match sl {
                        Value::Object(m) => !m.is_empty(),
                        Value::Null => false,
                        Value::Bool(b) => *b,
                        Value::String(s) => !s.is_empty(),
                        Value::Array(a) => !a.is_empty(),
                        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
                    };
                    if truthy {
                        enabled = true;
                        typ = sl
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("command")
                            .to_string();
                        command = sl
                            .get("command")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        padding = sl.get("padding").and_then(|v| v.as_i64());
                    }
                }
            }
        }
    }

    let mut script_content: Option<String> = None;
    if let Some(cmd) = command.as_deref() {
        let sp = expanduser(cmd);
        if sp.exists() {
            if let Ok(c) = std::fs::read_to_string(&sp) {
                script_content = Some(c);
            }
        }
    }

    json!({
        "type": typ,
        "command": command,
        "padding": padding,
        "enabled": enabled,
        "script_content": script_content,
    })
}

/// Port of `StatusLineService.update_config`.
fn update_config(update: &StatusLineUpdate) -> AppResult<Value> {
    let settings_file = paths::get_claude_user_settings_file();

    if let Some(parent) = settings_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut settings: Value = if settings_file.exists() {
        let text = std::fs::read_to_string(&settings_file)?;
        serde_json::from_str(&text)?
    } else {
        json!({})
    };

    let obj = settings
        .as_object_mut()
        .ok_or_else(|| AppError::internal("settings.json is not an object"))?;

    // Python: `if update.enabled is False:` — only an explicit false removes.
    if update.enabled == Some(false) {
        obj.remove("statusLine");
    } else {
        if !obj.contains_key("statusLine") {
            obj.insert(
                "statusLine".to_string(),
                json!({
                    "type": "command",
                    "command": default_script_path().to_string_lossy(),
                }),
            );
        }
        let sl = obj
            .get_mut("statusLine")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| AppError::internal("statusLine is not an object"))?;
        if let Some(t) = &update.r#type {
            sl.insert("type".to_string(), json!(t));
        }
        if let Some(c) = &update.command {
            sl.insert("command".to_string(), json!(c));
        }
        if let Some(p) = update.padding {
            sl.insert("padding".to_string(), json!(p));
        }
    }

    let serialized = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&settings_file, serialized)?;

    Ok(get_config())
}

/// Port of `StatusLineService.write_script`: write + chmod 0755.
fn write_script(content: &str) -> AppResult<PathBuf> {
    let path = default_script_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // S_IRWXU | S_IRGRP | S_IXGRP | S_IROTH | S_IXOTH == 0o755
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(path)
}

/// Port of `StatusLineService.preview_script`. Returns (success, output, error).
async fn preview_script(script_content: &str, timeout: u64) -> (bool, String, Option<String>) {
    // Python uses tempfile.NamedTemporaryFile(suffix=".sh"); replicate a
    // unique temp path so concurrent previews don't collide.
    let unique = format!(
        "statusline-{}-{}.sh",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let tmp = std::env::temp_dir().join(unique);

    {
        let mut f = match std::fs::File::create(&tmp) {
            Ok(f) => f,
            Err(e) => return (false, String::new(), Some(e.to_string())),
        };
        if let Err(e) = f.write_all(script_content.as_bytes()) {
            let _ = std::fs::remove_file(&tmp);
            return (false, String::new(), Some(e.to_string()));
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700)) {
            let _ = std::fs::remove_file(&tmp);
            return (false, String::new(), Some(e.to_string()));
        }
    }

    let result = run_preview(&tmp, timeout).await;
    let _ = std::fs::remove_file(&tmp);
    result
}

async fn run_preview(
    script_path: &std::path::Path,
    timeout: u64,
) -> (bool, String, Option<String>) {
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let mut cmd = Command::new(script_path);
    cmd.current_dir("/tmp")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (false, String::new(), Some(e.to_string())),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(MOCK_PREVIEW_DATA.as_bytes()).await;
        drop(stdin);
    }

    let waited = tokio::time::timeout(Duration::from_secs(timeout), child.wait_with_output()).await;

    match waited {
        Err(_) => (
            false,
            String::new(),
            Some(format!(
                "Script execution timed out after {} seconds",
                timeout
            )),
        ),
        Ok(Err(e)) => (false, String::new(), Some(e.to_string())),
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() {
                (true, stdout, None)
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let err = if stderr.is_empty() {
                    "Script failed".to_string()
                } else {
                    stderr
                };
                (false, stdout, Some(err))
            }
        }
    }
}

/// Port of `StatusLineService.check_nodejs`.
async fn check_nodejs_impl() -> (bool, Option<String>) {
    use tokio::process::Command;
    let fut = Command::new("node").arg("--version").output();
    match tokio::time::timeout(Duration::from_secs(5), fut).await {
        Ok(Ok(out)) if out.status.success() => {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (true, Some(v))
        }
        _ => (false, None),
    }
}

fn preset_to_json(p: &Preset) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "description": p.description,
        "script": p.script,
    })
}

fn powerline_to_json(p: &Powerline) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "description": p.description,
        "theme": p.theme,
        "style": p.style,
        "command": p.command,
    })
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/statusline
async fn get_statusline_config() -> AppResult<Json<Value>> {
    Ok(Json(get_config()))
}

/// PUT /api/v1/statusline
async fn update_statusline_config(Json(update): Json<StatusLineUpdate>) -> AppResult<Json<Value>> {
    update_config(&update).map(Json).map_err(|e| {
        AppError::internal(format!("Failed to update status line config: {}", e.detail))
    })
}

/// GET /api/v1/statusline/presets
async fn get_statusline_presets() -> AppResult<Json<Value>> {
    let presets: Vec<Value> = statusline_presets().iter().map(preset_to_json).collect();
    Ok(Json(json!({ "presets": presets })))
}

/// POST /api/v1/statusline/apply-preset/{preset_id}
async fn apply_statusline_preset(Path(preset_id): Path<String>) -> AppResult<Json<Value>> {
    let presets = statusline_presets();
    let Some(preset) = presets.iter().find(|p| p.id == preset_id) else {
        return Err(AppError::not_found(format!(
            "Preset not found: {}",
            preset_id
        )));
    };

    write_script(preset.script)
        .map_err(|e| AppError::internal(format!("Failed to apply preset: {}", e.detail)))?;

    let update = StatusLineUpdate {
        r#type: Some("command".to_string()),
        command: Some(default_script_path().to_string_lossy().into_owned()),
        padding: None,
        enabled: Some(true),
    };
    update_config(&update)
        .map(Json)
        .map_err(|e| AppError::internal(format!("Failed to apply preset: {}", e.detail)))
}

/// POST /api/v1/statusline/script  (body is a bare JSON string)
async fn save_custom_script(Json(script_content): Json<String>) -> AppResult<Json<Value>> {
    write_script(&script_content)
        .map_err(|e| AppError::internal(format!("Failed to save script: {}", e.detail)))?;
    let update = StatusLineUpdate {
        r#type: Some("command".to_string()),
        command: Some(default_script_path().to_string_lossy().into_owned()),
        padding: None,
        enabled: Some(true),
    };
    update_config(&update)
        .map(Json)
        .map_err(|e| AppError::internal(format!("Failed to save script: {}", e.detail)))
}

/// POST /api/v1/statusline/preview
async fn preview_statusline_script(Json(req): Json<PreviewRequest>) -> AppResult<Json<Value>> {
    let (success, output, error) = preview_script(&req.script, 5).await;
    Ok(Json(json!({
        "success": success,
        "output": output,
        "error": error,
    })))
}

/// GET /api/v1/statusline/check-nodejs
async fn check_nodejs() -> AppResult<Json<Value>> {
    let (available, version) = check_nodejs_impl().await;
    Ok(Json(json!({ "available": available, "version": version })))
}

/// GET /api/v1/statusline/powerline-presets
async fn get_powerline_presets() -> AppResult<Json<Value>> {
    let presets: Vec<Value> = powerline_presets().iter().map(powerline_to_json).collect();
    Ok(Json(json!({ "presets": presets })))
}

/// POST /api/v1/statusline/apply-powerline/{preset_id}
async fn apply_powerline_preset(Path(preset_id): Path<String>) -> AppResult<Json<Value>> {
    let presets = powerline_presets();
    let Some(preset) = presets.iter().find(|p| p.id == preset_id) else {
        return Err(AppError::not_found(format!(
            "Powerline preset not found: {}",
            preset_id
        )));
    };

    let update = StatusLineUpdate {
        r#type: Some("command".to_string()),
        command: Some(preset.command.to_string()),
        padding: None,
        enabled: Some(true),
    };
    update_config(&update)
        .map(Json)
        .map_err(|e| AppError::internal(format!("Failed to apply powerline preset: {}", e.detail)))
}
