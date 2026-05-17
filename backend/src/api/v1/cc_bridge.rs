// PORTED: api/v1/cc_bridge/* (router.py + services/cc_bridge/{discovery,spawn,pty_relay}.py)

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path as AxumPath, Query,
    },
    http::HeaderMap,
    response::Response,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{AppError, AppResult};
use crate::api::v1::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/sessions", get(list_sessions).post(spawn_session_endpoint))
        .route("/sessions/{target}", axum::routing::delete(kill_session_endpoint))
        .route("/sessions/{target}/preview", get(get_session_preview))
        .route("/sessions/{target}/terminal", get(session_terminal))
        .route("/token", get(get_terminal_token))
}

// ---- one-time token store ---------------------------------------------------

const TOKEN_TTL: u64 = 30;

fn token_store() -> &'static Mutex<HashMap<String, u64>> {
    static S: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn spawned_sessions() -> &'static Mutex<HashMap<String, SpawnMeta>> {
    static S: OnceLock<Mutex<HashMap<String, SpawnMeta>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone)]
struct SpawnMeta {
    mode: String,
    directory: String,
    worktree_name: Option<String>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn rand_token() -> String {
    // 32 bytes of OS randomness, URL-safe base64 (no padding) — matches the
    // shape of Python's secrets.token_urlsafe(32).
    let mut buf = [0u8; 32];
    if getrandom(&mut buf).is_err() {
        let seed = now_secs();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = ((seed >> (i % 8)) ^ (i as u64 * 1103515245)) as u8;
        }
    }
    base64_urlsafe(&buf)
}

fn getrandom(buf: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom")?;
    f.read_exact(buf)
}

fn base64_urlsafe(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        }
    }
    out
}

// ---- discovery --------------------------------------------------------------

const PANE_FORMAT: &str = "#{session_name}:#{window_index}.#{pane_index}|#{session_name}|#{window_name}|#{pane_id}|#{pane_current_path}|#{pane_pid}|#{pane_current_command}";

const MAX_TREE_DEPTH: usize = 4;

fn run_cmd(args: &[&str], timeout_secs: u64) -> Result<(i32, String, String), std::io::Error> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(args[0])
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let start = SystemTime::now();
    loop {
        match child.try_wait()? {
            Some(_) => break,
            None => {
                if start.elapsed().map(|e| e.as_secs()).unwrap_or(0) > timeout_secs {
                    let _ = child.kill();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "command timed out",
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    let out = child.wait_with_output()?;
    Ok((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

fn has_claude_descendant(pid: &str, depth: usize, visited: &mut Vec<String>) -> bool {
    if depth > MAX_TREE_DEPTH {
        return false;
    }
    if visited.iter().any(|v| v == pid) {
        return false;
    }
    visited.push(pid.to_string());

    let Ok((_, stdout, _)) = run_cmd(&["pgrep", "-a", "-P", pid], 5) else {
        return false;
    };
    for line in stdout.trim().lines() {
        let mut parts = line.splitn(2, char::is_whitespace);
        let child_pid = parts.next().unwrap_or("");
        let cmdline = parts.next().unwrap_or("");
        if child_pid.is_empty() || cmdline.is_empty() {
            continue;
        }
        let argv0 = cmdline.split_whitespace().next().unwrap_or("");
        if Path::new(argv0).file_name().and_then(|s| s.to_str()) == Some("claude") {
            return true;
        }
        if has_claude_descendant(child_pid, depth + 1, visited) {
            return true;
        }
    }
    false
}

fn is_claude_code(command: &str, pid: &str) -> bool {
    let c = command.trim().to_lowercase();
    if c == "claude" {
        return true;
    }
    if c == "node" {
        if !pid.chars().all(|ch| ch.is_ascii_digit()) || pid.is_empty() {
            return false;
        }
        let mut visited = Vec::new();
        return has_claude_descendant(pid, 0, &mut visited);
    }
    false
}

fn discover_cc_sessions() -> Vec<Value> {
    let res = run_cmd(
        &["tmux", "list-panes", "-a", "-F", PANE_FORMAT],
        10,
    );
    let stdout = match res {
        Ok((code, stdout, _)) if code == 0 => stdout,
        _ => return Vec::new(),
    };

    let mut results = Vec::new();
    for line in stdout.trim().lines() {
        let parts: Vec<&str> = line.splitn(7, '|').collect();
        if parts.len() != 7 {
            continue;
        }
        let (target, session_name, window_name, pane_id, cwd, pid, command) = (
            parts[0], parts[1], parts[2], parts[3], parts[4], parts[5], parts[6],
        );
        if is_claude_code(command, pid) {
            results.push(json!({
                "tmux_target": target,
                "session_name": session_name,
                "window_name": window_name,
                "pane_id": pane_id,
                "cwd": cwd,
                "pid": pid,
                "status": "active",
            }));
        }
    }
    results
}

fn capture_pane_preview(target: &str) -> String {
    match run_cmd(&["tmux", "capture-pane", "-t", target, "-p", "-e"], 5) {
        Ok((code, stdout, _)) if code == 0 => stdout,
        _ => String::new(),
    }
}

// ---- spawn / kill -----------------------------------------------------------

fn resolve_project_directory(project_folder: &str) -> Result<String, AppError> {
    let decoded = format!(
        "/{}",
        project_folder.trim_start_matches('-').replace('-', "/")
    );
    let decoded_path = PathBuf::from(&decoded);
    let resolved = std::fs::canonicalize(&decoded_path).unwrap_or_else(|_| decoded_path.clone());

    let has_traversal = decoded_path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir));
    if !resolved.is_absolute() || has_traversal {
        return Err(AppError::bad_request(format!(
            "Invalid project folder: '{}'",
            project_folder
        )));
    }
    if resolved.is_dir() {
        return Ok(resolved.to_string_lossy().into_owned());
    }
    Err(AppError::bad_request(format!(
        "Could not resolve project directory for '{}'. Please provide the directory path explicitly.",
        project_folder
    )))
}

fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "@%_-+=:,./".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[derive(Deserialize)]
struct SpawnRequest {
    directory: String,
    mode: String,
    #[serde(default)]
    worktree_name: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    project_folder: Option<String>,
    #[serde(default)]
    skip_permissions: bool,
}

fn spawn_session(req: &SpawnRequest) -> Result<Value, AppError> {
    let mut directory = req.directory.clone();

    if req.mode == "resume" && directory.trim().is_empty() {
        if let Some(pf) = &req.project_folder {
            directory = resolve_project_directory(pf)?;
        }
    }

    let dir_input = PathBuf::from(&directory);
    let dir_path = std::fs::canonicalize(&dir_input).unwrap_or_else(|_| dir_input.clone());
    if !dir_path.is_absolute() {
        return Err(AppError::bad_request(format!(
            "Directory must be an absolute path: {}",
            directory
        )));
    }
    if dir_input
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(AppError::bad_request(format!(
            "Directory must not contain path traversal: {}",
            directory
        )));
    }
    if !dir_path.is_dir() {
        return Err(AppError::bad_request(format!(
            "Directory does not exist: {}",
            directory
        )));
    }
    let directory = dir_path.to_string_lossy().into_owned();

    let dir_basename = dir_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    let dir_basename = if dir_basename.is_empty() {
        "project"
    } else {
        dir_basename
    };
    let safe_basename: String = dir_basename
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' })
        .take(20)
        .collect();
    let suffix = base64_urlsafe(&{
        let mut b = [0u8; 3];
        let _ = getrandom(&mut b);
        b
    })
    .chars()
    .filter(|c| c.is_ascii_alphanumeric())
    .take(4)
    .collect::<String>();
    let suffix = if suffix.len() == 4 {
        suffix
    } else {
        format!("{:04x}", now_secs() & 0xffff)
    };
    let name = format!("{}-{}", safe_basename, suffix);

    let mut command: Vec<String> = vec!["claude".to_string()];
    match req.mode.as_str() {
        "plain" => {}
        "worktree" => {
            let wt_name = req.worktree_name.clone().unwrap_or_else(|| name.clone());
            command.push("--worktree".to_string());
            command.push(wt_name);
        }
        "resume" => {
            let sid = req
                .session_id
                .clone()
                .ok_or_else(|| AppError::bad_request("session_id is required for resume mode"))?;
            command.push("--resume".to_string());
            command.push(sid);
        }
        other => {
            return Err(AppError::bad_request(format!("Unknown mode: {}", other)));
        }
    }
    if req.skip_permissions {
        command.push("--dangerously-skip-permissions".to_string());
    }

    let shell_command = command
        .iter()
        .map(|p| shell_quote(p))
        .collect::<Vec<_>>()
        .join(" ");

    match run_cmd(
        &[
            "tmux",
            "new-session",
            "-d",
            "-s",
            &name,
            "-c",
            &directory,
            &shell_command,
        ],
        10,
    ) {
        Ok((code, _, _)) if code == 0 => {}
        Ok((_, _, stderr)) => {
            return Err(AppError::bad_request(format!(
                "tmux new-session failed: {}",
                stderr.trim()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::bad_request("tmux is not installed or not in PATH"));
        }
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            return Err(AppError::bad_request("tmux new-session timed out"));
        }
        Err(e) => return Err(AppError::bad_request(e.to_string())),
    }

    let wt = req
        .worktree_name
        .clone()
        .or_else(|| if req.mode == "worktree" { Some(name.clone()) } else { None });
    spawned_sessions().lock().unwrap().insert(
        name.clone(),
        SpawnMeta {
            mode: req.mode.clone(),
            directory: directory.clone(),
            worktree_name: wt,
        },
    );

    Ok(json!({
        "tmux_target": format!("{}:0.0", name),
        "session_name": name,
    }))
}

fn kill_session(session_name: &str, cleanup_worktree: bool) -> Value {
    let metadata = spawned_sessions().lock().unwrap().get(session_name).cloned();

    match run_cmd(&["tmux", "kill-session", "-t", session_name], 10) {
        Ok((code, _, _)) if code == 0 => {}
        Ok((_, _, stderr)) => {
            return json!({ "killed": false, "error": stderr.trim() });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return json!({ "killed": false, "error": "tmux is not installed or not in PATH" });
        }
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            return json!({ "killed": false, "error": "tmux kill-session timed out" });
        }
        Err(e) => return json!({ "killed": false, "error": e.to_string() }),
    }

    if cleanup_worktree {
        if let Some(meta) = &metadata {
            if meta.mode == "worktree" {
                if let Some(wt_name) = &meta.worktree_name {
                    let _ = run_cmd(
                        &[
                            "git",
                            "-C",
                            &meta.directory,
                            "worktree",
                            "remove",
                            wt_name,
                            "--force",
                        ],
                        10,
                    );
                }
            }
        }
    }

    spawned_sessions().lock().unwrap().remove(session_name);
    json!({ "killed": true })
}

// ---- origin / token validation ---------------------------------------------

fn origin_netloc(origin: &str) -> Option<String> {
    let rest = origin
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(origin);
    let netloc = rest.split(['/', '?', '#']).next().unwrap_or("");
    if netloc.is_empty() {
        None
    } else {
        Some(netloc.to_lowercase())
    }
}

fn is_same_origin(origin: &str, headers: &HeaderMap) -> bool {
    let Some(origin_host) = origin_netloc(origin) else {
        return false;
    };
    let request_host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if !request_host.is_empty() && origin_host == request_host {
        return true;
    }
    let host_only = origin_host.split(':').next().unwrap_or("");
    matches!(host_only, "localhost" | "127.0.0.1" | "[::1]")
}

fn validate_token(token: &str) -> bool {
    let mut store = token_store().lock().unwrap();
    match store.remove(token) {
        Some(issued_at) => now_secs().saturating_sub(issued_at) <= TOKEN_TTL,
        None => false,
    }
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/cc-bridge/sessions
async fn list_sessions() -> AppResult<Json<Value>> {
    let sessions = tokio::task::spawn_blocking(discover_cc_sessions)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    let count = sessions.len();
    Ok(Json(json!({ "sessions": sessions, "count": count })))
}

/// GET /api/v1/cc-bridge/token
async fn get_terminal_token() -> AppResult<Json<Value>> {
    let now = now_secs();
    {
        let mut store = token_store().lock().unwrap();
        store.retain(|_, ts| now.saturating_sub(*ts) <= TOKEN_TTL);
        let token = rand_token();
        store.insert(token.clone(), now);
        Ok(Json(json!({ "token": token })))
    }
}

#[derive(Deserialize)]
struct TerminalQuery {
    #[serde(default)]
    token: String,
    #[serde(default = "default_mode")]
    mode: String,
}

fn default_mode() -> String {
    "readonly".to_string()
}

/// GET /api/v1/cc-bridge/sessions/{target}/preview
async fn get_session_preview(
    AxumPath(target): AxumPath<String>,
) -> AppResult<Json<Value>> {
    let cap_target = target.clone();
    let content = tokio::task::spawn_blocking(move || capture_pane_preview(&cap_target))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    if content.is_empty() {
        return Err(AppError::not_found("Could not capture pane"));
    }
    Ok(Json(json!({ "target": target, "content": content })))
}

/// GET (WebSocket) /api/v1/cc-bridge/sessions/{target}/terminal
async fn session_terminal(
    AxumPath(target): AxumPath<String>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    Query(q): Query<TerminalQuery>,
) -> Response {
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !origin.is_empty() && !is_same_origin(&origin, &headers) {
        return ws.on_upgrade(|mut s| async move {
            let _ = s
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 4403,
                    reason: "Invalid origin".into(),
                })))
                .await;
        });
    }
    if !validate_token(&q.token) {
        return ws.on_upgrade(|mut s| async move {
            let _ = s
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 4401,
                    reason: "Invalid or expired token".into(),
                })))
                .await;
        });
    }

    let read_only = q.mode != "interactive";
    ws.on_upgrade(move |socket| terminal_relay(socket, target, read_only))
}

/// DELETE /api/v1/cc-bridge/sessions/{target}
async fn kill_session_endpoint(
    AxumPath(target): AxumPath<String>,
    Query(params): Query<HashMap<String, String>>,
) -> AppResult<Json<Value>> {
    let target = target.trim_end_matches('/').to_string();
    let cleanup = params
        .get("cleanup_worktree")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    let result = tokio::task::spawn_blocking(move || kill_session(&target, cleanup))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(Json(result))
}

/// POST /api/v1/cc-bridge/sessions
async fn spawn_session_endpoint(Json(req): Json<SpawnRequest>) -> AppResult<Json<Value>> {
    let result = tokio::task::spawn_blocking(move || spawn_session(&req))
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;
    Ok(Json(result))
}

// ---- terminal relay ---------------------------------------------------------

/// Real tmux terminal relay without a pseudo-terminal dependency.
///
/// The Python implementation attaches a dedicated `tmux attach-session` over a
/// Unix pty (`pty.openpty`). No pty crate is in the dependency set (see
/// NEEDS-DEP note in the porting report), so a byte-identical pty attach is not
/// reproducible here. This implementation provides a real, working relay over
/// the same target: input bytes/keystrokes are delivered with
/// `tmux send-keys -t <target>` and screen output is streamed by polling
/// `tmux capture-pane -p -e -t <target>` and emitting it on change. Control
/// frames (`resize`, `mode`) are honoured. Read-only mode drops input exactly
/// like the Python relay.
async fn terminal_relay(mut socket: WebSocket, target: String, mut read_only: bool) {
    use tokio::time::{interval, Duration};

    let mut last = String::new();
    let mut tick = interval(Duration::from_millis(150));

    // Send initial snapshot.
    {
        let t = target.clone();
        if let Ok(content) = tokio::task::spawn_blocking(move || capture_pane_preview(&t)).await {
            if !content.is_empty() {
                last = content.clone();
                if socket
                    .send(Message::Binary(content.into_bytes().into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let t = target.clone();
                let content = match tokio::task::spawn_blocking(move || capture_pane_preview(&t)).await {
                    Ok(c) => c,
                    Err(_) => break,
                };
                if content != last && !content.is_empty() {
                    last = content.clone();
                    if socket.send(Message::Binary(content.into_bytes().into())).await.is_err() {
                        break;
                    }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Binary(b))) => {
                        if !read_only && !b.is_empty() {
                            let s = String::from_utf8_lossy(&b).into_owned();
                            let t = target.clone();
                            let _ = tokio::task::spawn_blocking(move || {
                                run_cmd(&["tmux", "send-keys", "-t", &t, "-l", &s], 5)
                            }).await;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(ctrl) = serde_json::from_str::<Value>(&text) {
                            if ctrl.is_object() && ctrl.get("type").is_some() {
                                match ctrl.get("type").and_then(|v| v.as_str()) {
                                    Some("resize") => {
                                        let rows = ctrl.get("rows").and_then(|v| v.as_i64()).unwrap_or(24);
                                        let cols = ctrl.get("cols").and_then(|v| v.as_i64()).unwrap_or(80);
                                        let t = target.clone();
                                        let _ = tokio::task::spawn_blocking(move || {
                                            run_cmd(
                                                &[
                                                    "tmux", "resize-window", "-t", &t,
                                                    "-x", &cols.to_string(), "-y", &rows.to_string(),
                                                ],
                                                5,
                                            )
                                        }).await;
                                    }
                                    Some("mode") => {
                                        read_only = ctrl.get("readOnly").and_then(|v| v.as_bool()).unwrap_or(true);
                                    }
                                    _ => {}
                                }
                                continue;
                            }
                        }
                        if !read_only && !text.is_empty() {
                            let t = target.clone();
                            let s = text.to_string();
                            let _ = tokio::task::spawn_blocking(move || {
                                run_cmd(&["tmux", "send-keys", "-t", &t, "-l", &s], 5)
                            }).await;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}
