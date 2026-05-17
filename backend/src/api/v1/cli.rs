// PORTED: cli_executor.py + api/v1/cli.py

use axum::{extract::Json, routing::post, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};

/// Whitelist of allowed Claude CLI subcommands (CLIExecutor.ALLOWED_COMMANDS).
const ALLOWED_COMMANDS: [&str; 3] = ["mcp", "config", "plugin"];

/// subprocess.run default timeout in cli_executor.execute().
const TIMEOUT_SECS: u64 = 30;

pub fn router() -> Router<ApiState> {
    Router::new().route("/execute", post(execute_cli_command))
}

#[derive(Deserialize)]
struct CLIExecuteRequest {
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

fn validate_command(command: &str) -> bool {
    ALLOWED_COMMANDS.contains(&command)
}

/// Port of `shutil.which("claude")` — scan PATH for an executable `claude`.
fn find_claude_binary() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join("claude");
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable(p: &std::path::Path) -> bool {
    p.is_file()
}

/// POST /api/v1/cli/execute
async fn execute_cli_command(Json(req): Json<CLIExecuteRequest>) -> AppResult<Json<Value>> {
    // Validate command against whitelist (HTTPException 400).
    if !validate_command(&req.command) {
        return Err(AppError::bad_request(format!(
            "Command '{}' is not allowed. Allowed commands: {}",
            req.command,
            ALLOWED_COMMANDS.join(", ")
        )));
    }

    // Check if claude binary is available (HTTPException 500).
    let claude_binary = match find_claude_binary() {
        Some(b) => b,
        None => {
            return Err(AppError::internal(
                "Claude CLI binary not found in PATH. Please ensure Claude Code is installed.",
            ))
        }
    };

    // Build full command: [claude_binary, command, *args]
    let mut cmd = tokio::process::Command::new(&claude_binary);
    cmd.arg(&req.command);
    cmd.args(&req.args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Ok(Json(json!({
                "stdout": "",
                "stderr": format!("Failed to execute command: {}", e),
                "exit_code": -1,
            })))
        }
    };

    let wait = child.wait_with_output();
    match tokio::time::timeout(Duration::from_secs(TIMEOUT_SECS), wait).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            let exit_code = output.status.code().unwrap_or(-1);
            Ok(Json(json!({
                "stdout": stdout,
                "stderr": stderr,
                "exit_code": exit_code,
            })))
        }
        Ok(Err(e)) => Ok(Json(json!({
            "stdout": "",
            "stderr": format!("Failed to execute command: {}", e),
            "exit_code": -1,
        }))),
        Err(_) => Ok(Json(json!({
            "stdout": "",
            "stderr": format!("Command timed out after {} seconds", TIMEOUT_SECS),
            "exit_code": -1,
        }))),
    }
}
