// PORTED: mcp_service.py + mcp_registry_service.py + api/v1/mcp.py
//
// Faithful port of the MCP subsystem: server config CRUD across user
// (~/.claude.json), project (.mcp.json), plugin, and managed scopes;
// real connection probing (stdio subprocess / streamable-HTTP / SSE) with
// DB-cached results in `mcp_server_cache`; approval settings; OAuth 2.1 +
// PKCE flow; and a proxy for the official MCP registry. Route paths, query
// names and JSON shapes match the Python router byte-for-byte (the unchanged
// frontend was built against them). Errors render as `{"detail": ...}`.

use axum::{
    Router,
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::time::{Duration, timeout};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::fileio::{read_json_file, write_json_file};
use crate::paths;

const REGISTRY_BASE_URL: &str = "https://registry.modelcontextprotocol.io/v0.1";
const REGISTRY_REQUEST_TIMEOUT: u64 = 15;
const MAX_CACHED_ITEMS: usize = 200;
const SENSITIVE_PATTERNS: [&str; 5] = ["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL"];

pub fn router() -> Router<ApiState> {
    // Paths mirror Python's `router` (mounted at `/mcp`) exactly.
    Router::new()
        .route("/servers", get(list_mcp_servers).post(create_mcp_server))
        .route("/servers/test-all", post(test_all_servers))
        .route(
            "/servers/{name}",
            get(get_mcp_server)
                .put(update_mcp_server)
                .delete(delete_mcp_server),
        )
        .route("/servers/{name}/toggle", post(toggle_mcp_server))
        .route("/servers/{name}/test", post(test_mcp_server_connection))
        .route("/servers/{name}/auth-status", get(get_auth_status))
        .route("/servers/{name}/auth/start", post(start_auth))
        .route("/auth/callback", get(auth_callback))
        .route(
            "/approval-settings",
            get(get_approval_settings).put(update_approval_settings),
        )
        .route("/registry/search", get(search_registry))
        .route("/registry/install", post(install_registry_server))
        .route("/registry/{*rest}", get(registry_server_dispatch))
}

// ---- helpers ----------------------------------------------------------------

fn now_iso() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.6f")
        .to_string()
}

/// SQLAlchemy DateTime columns are stored as `YYYY-MM-DD HH:MM:SS[.ffffff]`;
/// Python serializes them with `datetime.isoformat()` (a `T` separator) before
/// sending to the frontend. Reproduce that exactly.
fn normalize_isoformat(s: &str) -> String {
    s.replacen(' ', "T", 1)
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Percent-encode like Python `urllib.parse.quote(s, safe="")`.
fn quote_strict(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", b));
        }
    }
    out
}

/// Percent-encode for application/x-www-form-urlencoded values
/// (matches `urllib.parse.urlencode` / form bodies: space -> '+').
fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b' ' => out.push('+'),
            x if x.is_ascii_alphanumeric() || matches!(x, b'_' | b'.' | b'-' | b'*') => {
                out.push(x as char)
            }
            x => out.push_str(&format!("%{:02X}", x)),
        }
    }
    out
}

fn build_query(pairs: &[(&str, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Replicate `shutil.which`: find an executable on PATH. Absolute/relative
/// paths (containing a separator) are checked directly, as Python does.
fn which(cmd: &str, enable_external_tools: bool) -> Option<PathBuf> {
    if !enable_external_tools {
        return None;
    }
    let is_exec = |p: &std::path::Path| -> bool {
        if !p.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            p.metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            true
        }
    };
    if cmd.contains('/') {
        let p = PathBuf::from(cmd);
        return if is_exec(&p) { Some(p) } else { None };
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(cmd);
        if is_exec(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn str_field<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

// ---- MD5 (config hash, must match Python hashlib.md5 for cache parity) -------

fn md5_hex(input: &[u8]) -> String {
    // RFC 1321.
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = String::with_capacity(32);
    for v in [a0, b0, c0, d0] {
        for byte in v.to_le_bytes() {
            out.push_str(&format!("{:02x}", byte));
        }
    }
    out
}

/// Stable JSON like Python `json.dumps(obj, sort_keys=True)`.
fn canonical_json(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let body = keys
                .iter()
                .map(|k| format!("{}: {}", json_str(k), canonical_json(&m[*k])))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{}}}", body)
        }
        Value::Array(a) => {
            let body = a.iter().map(canonical_json).collect::<Vec<_>>().join(", ");
            format!("[{}]", body)
        }
        Value::String(s) => json_str(s),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
    }
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

fn compute_config_hash(config: &Value) -> String {
    md5_hex(canonical_json(config).as_bytes())
}

// ---- masking ----------------------------------------------------------------

fn mask_sensitive_env(env: Option<&Value>) -> Option<Value> {
    let env = env?;
    let m = env.as_object()?;
    let mut out = Map::new();
    for (k, v) in m {
        let uk = k.to_uppercase();
        if SENSITIVE_PATTERNS.iter().any(|p| uk.contains(p)) {
            out.insert(k.clone(), json!("***MASKED***"));
        } else {
            out.insert(k.clone(), v.clone());
        }
    }
    Some(Value::Object(out))
}

/// Build the MCPServer JSON object from a raw config dict (env masked).
fn mcp_server_dict(name: &str, config: &Value, scope: &str) -> Value {
    json!({
        "name": name,
        "type": str_field(config, "type").unwrap_or("stdio"),
        "scope": scope,
        "source": Value::Null,
        "disabled": Value::Null,
        "command": config.get("command").cloned().unwrap_or(Value::Null),
        "args": config.get("args").cloned().unwrap_or(Value::Null),
        "url": config.get("url").cloned().unwrap_or(Value::Null),
        "headers": config.get("headers").cloned().unwrap_or(Value::Null),
        "env": mask_sensitive_env(config.get("env")).unwrap_or(Value::Null),
        "is_connected": Value::Null,
        "last_tested_at": Value::Null,
        "last_error": Value::Null,
        "mcp_server_name": Value::Null,
        "mcp_server_version": Value::Null,
        "tools": Value::Null,
        "tool_count": Value::Null,
        "resources": Value::Null,
        "prompts": Value::Null,
        "resource_count": Value::Null,
        "prompt_count": Value::Null,
        "capabilities": Value::Null,
    })
}

// ---- config readers / writers ----------------------------------------------

fn read_user_mcp_config(project_path: Option<&str>) -> Map<String, Value> {
    let config = read_json_file(&paths::get_claude_user_config_file());
    let mut servers = Map::new();
    let Some(config) = config else {
        return servers;
    };
    if let Some(s) = config.get("mcpServers").and_then(|v| v.as_object()) {
        for (k, v) in s {
            servers.insert(k.clone(), v.clone());
        }
    }
    if let Some(pp) = project_path {
        if let Some(projects) = config.get("projects").and_then(|v| v.as_object()) {
            if let Some(pc) = projects.get(pp) {
                if let Some(s) = pc.get("mcpServers").and_then(|v| v.as_object()) {
                    for (k, v) in s {
                        servers.insert(k.clone(), v.clone());
                    }
                }
            }
        }
    }
    servers
}

fn read_project_mcp_config(
    project_path: Option<&str>,
    cwd_fallback: &std::path::Path,
) -> Map<String, Value> {
    let config = read_json_file(&paths::get_project_mcp_config_file(
        project_path,
        cwd_fallback,
    ));
    match config
        .as_ref()
        .and_then(|c| c.get("mcpServers"))
        .and_then(|v| v.as_object())
    {
        Some(s) => s.clone(),
        None => Map::new(),
    }
}

fn read_managed_mcp_config() -> Map<String, Value> {
    let config = read_json_file(&paths::get_managed_mcp_config_file());
    match config
        .as_ref()
        .and_then(|c| c.get("mcpServers"))
        .and_then(|v| v.as_object())
    {
        Some(s) => s.clone(),
        None => Map::new(),
    }
}

struct PluginServer {
    name: String,
    config: Value,
    plugin_name: String,
}

fn read_plugin_mcp_servers() -> Vec<PluginServer> {
    let installed = read_json_file(&paths::get_installed_plugins_file());
    let mut out = Vec::new();
    let Some(installed) = installed else {
        return out;
    };
    let Some(plugins) = installed.get("plugins").and_then(|v| v.as_object()) else {
        return out;
    };

    for (plugin_key, installations) in plugins {
        if !plugin_key.contains('@') {
            continue;
        }
        let (plugin_name, _marketplace) = match plugin_key.rsplit_once('@') {
            Some((n, m)) => (n.to_string(), m.to_string()),
            None => continue,
        };
        let installs = match installations.as_array() {
            Some(a) if !a.is_empty() => a,
            _ => continue,
        };
        let install_path = match str_field(&installs[0], "installPath") {
            Some(p) => PathBuf::from(p),
            None => continue,
        };

        let mut mcp_servers: Map<String, Value> = Map::new();

        // .mcp.json (legacy/flat)
        if let Some(pmc) = read_json_file(&install_path.join(".mcp.json")) {
            if let Some(o) = pmc.as_object() {
                if let Some(inner) = o.get("mcpServers").and_then(|v| v.as_object()) {
                    for (k, v) in inner {
                        mcp_servers.insert(k.clone(), v.clone());
                    }
                } else {
                    for (k, v) in o {
                        mcp_servers.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        // .claude-plugin/plugin.json -> mcpServers
        let plugin_json_path = install_path.join(".claude-plugin").join("plugin.json");
        if let Some(pj) = read_json_file(&plugin_json_path) {
            if let Some(msv) = pj.get("mcpServers") {
                if let Some(obj) = msv.as_object() {
                    for (k, v) in obj {
                        mcp_servers.insert(k.clone(), v.clone());
                    }
                } else if let Some(rel) = msv.as_str() {
                    let parent = plugin_json_path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| install_path.clone());
                    if let Ok(ref_path) = parent.join(rel).canonicalize() {
                        let install_canon = install_path
                            .canonicalize()
                            .unwrap_or_else(|_| install_path.clone());
                        let prefix = format!("{}/", install_canon.to_string_lossy());
                        if ref_path.to_string_lossy().starts_with(&prefix) {
                            if let Some(rd) = read_json_file(&ref_path) {
                                if let Some(inner) =
                                    rd.get("mcpServers").and_then(|v| v.as_object())
                                {
                                    for (k, v) in inner {
                                        mcp_servers.insert(k.clone(), v.clone());
                                    }
                                }
                            }
                        } else {
                            continue;
                        }
                    }
                }
            }
        }

        if mcp_servers.is_empty() {
            continue;
        }

        for (server_name, server_config) in &mcp_servers {
            out.push(PluginServer {
                name: format!("plugin:{}:{}", plugin_name, server_name),
                config: server_config.clone(),
                plugin_name: plugin_name.clone(),
            });
        }
    }
    out
}

async fn write_user_mcp_config(servers: &Map<String, Value>) -> bool {
    let path = paths::get_claude_user_config_file();
    let mut config = read_json_file(&path).unwrap_or_else(|| json!({}));
    if !config.is_object() {
        config = json!({});
    }
    config
        .as_object_mut()
        .unwrap()
        .insert("mcpServers".into(), Value::Object(servers.clone()));
    write_json_file(&path, &config).await
}

async fn write_project_mcp_config(
    servers: &Map<String, Value>,
    project_path: Option<&str>,
    cwd_fallback: &std::path::Path,
) -> bool {
    let path = paths::get_project_mcp_config_file(project_path, cwd_fallback);
    let mut config = read_json_file(&path).unwrap_or_else(|| json!({}));
    if !config.is_object() {
        config = json!({});
    }
    config
        .as_object_mut()
        .unwrap()
        .insert("mcpServers".into(), Value::Object(servers.clone()));
    write_json_file(&path, &config).await
}

fn get_disabled_servers() -> std::collections::HashSet<String> {
    let config = read_json_file(&paths::get_claude_user_settings_file());
    let mut set = std::collections::HashSet::new();
    if let Some(config) = config {
        if let Some(arr) = config.get("disabledMcpServers").and_then(|v| v.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    set.insert(s.to_string());
                }
            }
        }
    }
    set
}

// ---- cache (mcp_server_cache table) ----------------------------------------

struct CacheRow {
    is_connected: bool,
    last_tested_at: Option<String>,
    last_error: Option<String>,
    mcp_server_name: Option<String>,
    mcp_server_version: Option<String>,
    tools: Option<Value>,
    tool_count: i64,
    resources: Option<Value>,
    prompts: Option<Value>,
    resource_count: i64,
    prompt_count: i64,
    capabilities: Option<Value>,
}

#[allow(clippy::type_complexity)]
async fn get_cached_server_info(
    pool: &sqlx::SqlitePool,
    name: &str,
    scope: &str,
) -> Option<CacheRow> {
    let row = sqlx::query_as::<
        _,
        (
            bool,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
            Option<String>,
            i64,
            i64,
            Option<String>,
        ),
    >(
        "SELECT is_connected, last_tested_at, last_error, mcp_server_name, \
         mcp_server_version, tools, tool_count, resources, prompts, \
         resource_count, prompt_count, capabilities \
         FROM mcp_server_cache WHERE server_name = ? AND server_scope = ?",
    )
    .bind(name)
    .bind(scope)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;

    let parse =
        |s: Option<String>| -> Option<Value> { s.and_then(|t| serde_json::from_str(&t).ok()) };
    Some(CacheRow {
        is_connected: row.0,
        last_tested_at: row.1,
        last_error: row.2,
        mcp_server_name: row.3,
        mcp_server_version: row.4,
        tools: parse(row.5),
        tool_count: row.6,
        resources: parse(row.7),
        prompts: parse(row.8),
        resource_count: row.9,
        prompt_count: row.10,
        capabilities: parse(row.11),
    })
}

async fn update_server_cache(
    pool: &sqlx::SqlitePool,
    name: &str,
    scope: &str,
    test_result: &Value,
    config_hash: &str,
) {
    let tools_list = test_result
        .get("tools")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let resources_list = test_result
        .get("resources")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let prompts_list = test_result
        .get("prompts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let is_success = test_result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let now = now_iso();

    let last_error = if is_success {
        None
    } else {
        test_result
            .get("message")
            .and_then(|v| v.as_str())
            .map(String::from)
    };
    let cap = |v: &[Value]| -> String {
        serde_json::to_string(&v.iter().take(MAX_CACHED_ITEMS).cloned().collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".into())
    };
    let tools_json = cap(&tools_list);
    let resources_json = cap(&resources_list);
    let prompts_json = cap(&prompts_list);
    let tool_count = test_result
        .get("tool_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(tools_list.len() as i64);
    let resource_count = test_result
        .get("resource_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(resources_list.len() as i64);
    let prompt_count = test_result
        .get("prompt_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(prompts_list.len() as i64);
    let mcp_server_name = test_result.get("server_name").and_then(|v| v.as_str());
    let mcp_server_version = test_result.get("server_version").and_then(|v| v.as_str());
    let capabilities = test_result
        .get("capabilities")
        .filter(|v| !v.is_null())
        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "null".into()));

    let existing: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM mcp_server_cache WHERE server_name = ? AND server_scope = ?",
    )
    .bind(name)
    .bind(scope)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if let Some((id,)) = existing {
        let _ = sqlx::query(
            "UPDATE mcp_server_cache SET is_connected=?, last_tested_at=?, last_error=?, \
             mcp_server_name=?, mcp_server_version=?, tools=?, tool_count=?, resources=?, \
             prompts=?, resource_count=?, prompt_count=?, capabilities=?, cached_at=?, \
             config_hash=? WHERE id=?",
        )
        .bind(is_success)
        .bind(&now)
        .bind(&last_error)
        .bind(mcp_server_name)
        .bind(mcp_server_version)
        .bind(&tools_json)
        .bind(tool_count)
        .bind(&resources_json)
        .bind(&prompts_json)
        .bind(resource_count)
        .bind(prompt_count)
        .bind(&capabilities)
        .bind(&now)
        .bind(config_hash)
        .bind(id)
        .execute(pool)
        .await;
    } else {
        let _ = sqlx::query(
            "INSERT INTO mcp_server_cache (server_name, server_scope, is_connected, \
             last_tested_at, last_error, mcp_server_name, mcp_server_version, tools, \
             tool_count, resources, prompts, resource_count, prompt_count, capabilities, \
             cached_at, config_hash) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(name)
        .bind(scope)
        .bind(is_success)
        .bind(&now)
        .bind(&last_error)
        .bind(mcp_server_name)
        .bind(mcp_server_version)
        .bind(&tools_json)
        .bind(tool_count)
        .bind(&resources_json)
        .bind(&prompts_json)
        .bind(resource_count)
        .bind(prompt_count)
        .bind(&capabilities)
        .bind(&now)
        .bind(config_hash)
        .execute(pool)
        .await;
    }
}

// ---- server listing / retrieval --------------------------------------------

async fn build_server_list(
    pool: &sqlx::SqlitePool,
    project_path: Option<&str>,
    cwd_fallback: &std::path::Path,
) -> Vec<Value> {
    let mut servers: Vec<Value> = Vec::new();
    let disabled = get_disabled_servers();

    for (name, config) in read_managed_mcp_config() {
        let mut s = mcp_server_dict(&name, &config, "managed");
        s["source"] = json!("enterprise");
        servers.push(s);
    }
    for (name, config) in read_user_mcp_config(project_path) {
        servers.push(mcp_server_dict(&name, &config, "user"));
    }
    for (name, config) in read_project_mcp_config(project_path, cwd_fallback) {
        servers.push(mcp_server_dict(&name, &config, "project"));
    }
    for ps in read_plugin_mcp_servers() {
        let mut s = mcp_server_dict(&ps.name, &ps.config, "plugin");
        s["source"] = json!(ps.plugin_name);
        servers.push(s);
    }

    for s in servers.iter_mut() {
        let name = s["name"].as_str().unwrap_or("").to_string();
        s["disabled"] = json!(disabled.contains(&name));
    }

    for s in servers.iter_mut() {
        let name = s["name"].as_str().unwrap_or("").to_string();
        let scope = s["scope"].as_str().unwrap_or("").to_string();
        if let Some(c) = get_cached_server_info(pool, &name, &scope).await {
            s["is_connected"] = json!(c.is_connected);
            s["last_tested_at"] = match c.last_tested_at {
                Some(t) => json!(normalize_isoformat(&t)),
                None => Value::Null,
            };
            s["last_error"] = json!(c.last_error);
            s["mcp_server_name"] = json!(c.mcp_server_name);
            s["mcp_server_version"] = json!(c.mcp_server_version);
            s["tool_count"] = json!(c.tool_count);
            s["resource_count"] = json!(c.resource_count);
            s["prompt_count"] = json!(c.prompt_count);
            s["capabilities"] = c.capabilities.unwrap_or(Value::Null);
            if let Some(t) = c.tools {
                if !matches!(&t, Value::Array(a) if a.is_empty()) {
                    s["tools"] = t;
                }
            }
            if let Some(r) = c.resources {
                if !matches!(&r, Value::Array(a) if a.is_empty()) {
                    s["resources"] = r;
                }
            }
            if let Some(p) = c.prompts {
                if !matches!(&p, Value::Array(a) if a.is_empty()) {
                    s["prompts"] = p;
                }
            }
        }
    }

    servers
}

fn get_server(name: &str, scope: &str, cwd_fallback: &std::path::Path) -> Option<Value> {
    match scope {
        "managed" => {
            let servers = read_managed_mcp_config();
            let c = servers.get(name)?;
            let mut s = mcp_server_dict(name, c, scope);
            s["source"] = json!("enterprise");
            Some(s)
        }
        "user" => {
            let servers = read_user_mcp_config(None);
            let c = servers.get(name)?;
            Some(mcp_server_dict(name, c, scope))
        }
        "project" => {
            let servers = read_project_mcp_config(None, cwd_fallback);
            let c = servers.get(name)?;
            Some(mcp_server_dict(name, c, scope))
        }
        "plugin" => {
            for ps in read_plugin_mcp_servers() {
                if ps.name == name {
                    let mut s = mcp_server_dict(name, &ps.config, scope);
                    s["source"] = json!(ps.plugin_name);
                    return Some(s);
                }
            }
            None
        }
        _ => None,
    }
}

// ---- query / body structs ---------------------------------------------------

#[derive(Deserialize)]
struct ProjectPathQuery {
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct ScopeQuery {
    scope: String,
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct AuthScopeQuery {
    #[serde(default = "default_user_scope")]
    #[allow(dead_code)]
    scope: String,
}

fn default_user_scope() -> String {
    "user".to_string()
}

#[derive(Deserialize)]
struct ServerCreateBody {
    name: String,
    #[serde(rename = "type")]
    type_: String,
    scope: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
struct ServerUpdateBody {
    #[serde(rename = "type", default)]
    type_: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
struct ToggleBody {
    disabled: bool,
}

// ---- handlers: CRUD ---------------------------------------------------------

/// GET /api/v1/mcp/servers
async fn list_mcp_servers(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let servers =
        build_server_list(&state.pool, q.project_path.as_deref(), &state.cwd_fallback).await;
    Ok(Json(json!({ "servers": servers })))
}

/// GET /api/v1/mcp/servers/{name}
async fn get_mcp_server(
    State(state): State<ApiState>,
    Path(name): Path<String>,
    Query(q): Query<ScopeQuery>,
) -> AppResult<Json<Value>> {
    match get_server(&name, &q.scope, &state.cwd_fallback) {
        Some(s) => Ok(Json(s)),
        None => Err(AppError::not_found(format!(
            "Server '{}' not found in '{}' scope",
            name, q.scope
        ))),
    }
}

/// POST /api/v1/mcp/servers
async fn create_mcp_server(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
    Json(server): Json<ServerCreateBody>,
) -> AppResult<Response> {
    if !["stdio", "http", "sse"].contains(&server.type_.as_str()) {
        return Err(AppError::bad_request(
            "Server type must be 'stdio', 'http', or 'sse'",
        ));
    }
    if !["user", "project"].contains(&server.scope.as_str()) {
        return Err(AppError::bad_request(
            "Server scope must be 'user' or 'project'",
        ));
    }
    if server.type_ == "stdio" && server.command.as_deref().unwrap_or("").is_empty() {
        return Err(AppError::bad_request(
            "Command is required for stdio servers",
        ));
    }
    if ["http", "sse"].contains(&server.type_.as_str())
        && server.url.as_deref().unwrap_or("").is_empty()
    {
        return Err(AppError::bad_request(
            "URL is required for http/sse servers",
        ));
    }

    let mut config = Map::new();
    config.insert("type".into(), json!(server.type_));
    if let Some(c) = &server.command {
        if !c.is_empty() {
            config.insert("command".into(), json!(c));
        }
    }
    if let Some(a) = &server.args {
        if !a.is_empty() {
            config.insert("args".into(), json!(a));
        }
    }
    if let Some(u) = &server.url {
        if !u.is_empty() {
            config.insert("url".into(), json!(u));
        }
    }
    if let Some(h) = &server.headers {
        if !h.is_empty() {
            config.insert("headers".into(), json!(h));
        }
    }
    if let Some(e) = &server.env {
        if !e.is_empty() {
            config.insert("env".into(), json!(e));
        }
    }
    let config = Value::Object(config);

    let written = if server.scope == "user" {
        let mut servers = read_user_mcp_config(None);
        servers.insert(server.name.clone(), config.clone());
        write_user_mcp_config(&servers).await
    } else {
        let mut servers = read_project_mcp_config(q.project_path.as_deref(), &state.cwd_fallback);
        servers.insert(server.name.clone(), config.clone());
        write_project_mcp_config(&servers, q.project_path.as_deref(), &state.cwd_fallback).await
    };
    if !written {
        return Err(AppError::internal("Failed to create server: write failed"));
    }

    let body = mcp_server_dict(&server.name, &config, &server.scope);
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

/// PUT /api/v1/mcp/servers/{name}
async fn update_mcp_server(
    State(state): State<ApiState>,
    Path(name): Path<String>,
    Query(q): Query<ScopeQuery>,
    Json(server): Json<ServerUpdateBody>,
) -> AppResult<Json<Value>> {
    if !["user", "project"].contains(&q.scope.as_str()) {
        return Err(AppError::bad_request(
            "Server scope must be 'user' or 'project'",
        ));
    }

    let mut servers = if q.scope == "user" {
        read_user_mcp_config(None)
    } else {
        read_project_mcp_config(q.project_path.as_deref(), &state.cwd_fallback)
    };

    if !servers.contains_key(&name) {
        return Err(AppError::not_found(format!(
            "Server '{}' not found in '{}' scope",
            name, q.scope
        )));
    }

    let mut config = servers.get(&name).cloned().unwrap_or_else(|| json!({}));
    let cm = config.as_object_mut().unwrap();
    if let Some(v) = &server.type_ {
        cm.insert("type".into(), json!(v));
    }
    if let Some(v) = &server.command {
        cm.insert("command".into(), json!(v));
    }
    if let Some(v) = &server.args {
        cm.insert("args".into(), json!(v));
    }
    if let Some(v) = &server.url {
        cm.insert("url".into(), json!(v));
    }
    if let Some(v) = &server.headers {
        cm.insert("headers".into(), json!(v));
    }
    if let Some(v) = &server.env {
        cm.insert("env".into(), json!(v));
    }

    servers.insert(name.clone(), config.clone());

    let ok = if q.scope == "user" {
        write_user_mcp_config(&servers).await
    } else {
        write_project_mcp_config(&servers, q.project_path.as_deref(), &state.cwd_fallback).await
    };
    if !ok {
        return Err(AppError::internal("Failed to update server: write failed"));
    }

    Ok(Json(mcp_server_dict(&name, &config, &q.scope)))
}

/// DELETE /api/v1/mcp/servers/{name}
async fn delete_mcp_server(
    State(state): State<ApiState>,
    Path(name): Path<String>,
    Query(q): Query<ScopeQuery>,
) -> AppResult<Response> {
    if !["user", "project"].contains(&q.scope.as_str()) {
        return Err(AppError::bad_request(
            "Server scope must be 'user' or 'project'",
        ));
    }

    let mut servers = if q.scope == "user" {
        read_user_mcp_config(None)
    } else {
        read_project_mcp_config(q.project_path.as_deref(), &state.cwd_fallback)
    };

    if servers.remove(&name).is_none() {
        return Err(AppError::not_found(format!(
            "Server '{}' not found in '{}' scope",
            name, q.scope
        )));
    }

    let ok = if q.scope == "user" {
        write_user_mcp_config(&servers).await
    } else {
        write_project_mcp_config(&servers, q.project_path.as_deref(), &state.cwd_fallback).await
    };
    if !ok {
        return Err(AppError::internal("Failed to write config"));
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// POST /api/v1/mcp/servers/{name}/toggle
async fn toggle_mcp_server(
    Path(name): Path<String>,
    Json(req): Json<ToggleBody>,
) -> AppResult<Json<Value>> {
    let path = paths::get_claude_user_settings_file();
    let mut config = read_json_file(&path).unwrap_or_else(|| json!({}));
    if !config.is_object() {
        config = json!({});
    }
    let mut disabled_list: std::collections::BTreeSet<String> = config
        .get("disabledMcpServers")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if req.disabled {
        disabled_list.insert(name.clone());
    } else {
        disabled_list.remove(&name);
    }
    config.as_object_mut().unwrap().insert(
        "disabledMcpServers".into(),
        json!(disabled_list.into_iter().collect::<Vec<_>>()),
    );

    if !write_json_file(&path, &config).await {
        return Err(AppError::internal("Failed to write settings file"));
    }

    Ok(Json(json!({
        "success": true,
        "message": format!("Server '{}' {} successfully", name,
            if req.disabled { "disabled" } else { "enabled" }),
        "server_name": name,
        "disabled": req.disabled,
    })))
}

// ---- connection testing -----------------------------------------------------

fn test_response_body(result: &Value) -> Value {
    json!({
        "success": result.get("success").and_then(|v| v.as_bool()).unwrap_or(false),
        "message": result.get("message").and_then(|v| v.as_str()).unwrap_or(""),
        "server_name": result.get("server_name").cloned().unwrap_or(Value::Null),
        "server_version": result.get("server_version").cloned().unwrap_or(Value::Null),
        "tools": result.get("tools").cloned().unwrap_or(Value::Null),
        "resources": result.get("resources").cloned().unwrap_or(Value::Null),
        "prompts": result.get("prompts").cloned().unwrap_or(Value::Null),
        "resource_count": result.get("resource_count").cloned().unwrap_or(Value::Null),
        "prompt_count": result.get("prompt_count").cloned().unwrap_or(Value::Null),
        "capabilities": result.get("capabilities").cloned().unwrap_or(Value::Null),
    })
}

fn fail(msg: impl Into<String>) -> Value {
    json!({ "success": false, "message": msg.into() })
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Parse one MCP framed/raw JSON message from a buffered stdout reader.
async fn read_mcp_message<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    overall_timeout: Duration,
) -> std::io::Result<Option<Value>> {
    let mut line = String::new();
    let n = timeout(overall_timeout, reader.read_line(&mut line))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"))??;
    if n == 0 {
        return Ok(None);
    }
    let trimmed = line.trim();
    if trimmed.starts_with("Content-Length:") {
        let cl: usize = trimmed
            .split(':')
            .nth(1)
            .and_then(|x| x.trim().parse().ok())
            .unwrap_or(0);
        let mut blank = String::new();
        let _ = timeout(Duration::from_secs(5), reader.read_line(&mut blank)).await;
        let mut buf = vec![0u8; cl];
        timeout(Duration::from_secs(5), reader.read_exact(&mut buf))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"))??;
        Ok(serde_json::from_slice(&buf).ok())
    } else {
        Ok(serde_json::from_str(trimmed).ok())
    }
}

async fn test_connection(
    pool: &sqlx::SqlitePool,
    name: &str,
    scope: &str,
    cache: bool,
    enable_external_tools: bool,
    cwd_fallback: &std::path::Path,
) -> Value {
    let server = match get_server(name, scope, cwd_fallback) {
        Some(s) => s,
        None => return fail(format!("Server '{}' not found", name)),
    };
    let stype = str_field(&server, "type").unwrap_or("stdio").to_string();

    if stype == "stdio" {
        let command = match str_field(&server, "command") {
            Some(c) if !c.is_empty() => c.to_string(),
            _ => return fail("No command specified for stdio server"),
        };
        if which(&command, enable_external_tools).is_none() {
            return fail(format!("Command '{}' not found in PATH", command));
        }
        let args: Vec<String> = server
            .get("args")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let result = stdio_probe(&command, &args).await;
        if cache
            && result
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        {
            let cfg = json!({
                "type": server.get("type"),
                "command": server.get("command"),
                "args": server.get("args"),
                "url": server.get("url"),
            });
            update_server_cache(pool, name, scope, &result, &compute_config_hash(&cfg)).await;
        }
        result
    } else if stype == "http" {
        let url = match str_field(&server, "url") {
            Some(u) if !u.is_empty() => u.to_string(),
            _ => return fail("No URL specified for http server"),
        };
        let result = http_probe(&server, &url).await;
        if cache
            && result
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        {
            let cfg = json!({ "type": server.get("type"), "url": url });
            update_server_cache(pool, name, scope, &result, &compute_config_hash(&cfg)).await;
        }
        result
    } else if stype == "sse" {
        let url = match str_field(&server, "url") {
            Some(u) if !u.is_empty() => u.to_string(),
            _ => return fail("No URL specified for SSE server"),
        };
        sse_probe(&server, &url).await
    } else {
        fail(format!("Unknown server type: {}", stype))
    }
}

async fn stdio_probe(command: &str, args: &[String]) -> Value {
    let mut child = match tokio::process::Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return fail(format!("Command '{}' not found", command));
        }
        Err(e) => return fail(format!("Failed to start server: {}", e)),
    };

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "claude-deck-test", "version": "1.0.0"},
        },
    });
    let is_npx = command == "npx";

    async fn cleanup(mut c: tokio::process::Child) {
        if let Ok(None) = c.try_wait() {
            let _ = c.start_kill();
            let _ = timeout(Duration::from_secs(2), c.wait()).await;
        }
    }

    if is_npx {
        // Wait for the server to be ready by watching stderr (up to ~30s).
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Ok(Some(status)) = child.try_wait() {
                let mut buf = vec![0u8; 4096];
                let n = stderr.read(&mut buf).await.unwrap_or(0);
                let err = if n > 0 {
                    String::from_utf8_lossy(&buf[..n]).trim().to_string()
                } else {
                    format!("Process exited ({})", status)
                };
                return fail(format!("Server failed: {}", truncate(&err, 300)));
            }
            let mut buf = vec![0u8; 4096];
            if let Ok(Ok(n)) = timeout(Duration::from_millis(300), stderr.read(&mut buf)).await {
                if n > 0 {
                    let low = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                    if low.contains("running on stdio") || low.contains("server") {
                        break;
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let raw = format!("{}\n", init_request);
    if stdin.write_all(raw.as_bytes()).await.is_err() {
        cleanup(child).await;
        return fail("Failed to start server: stdin write failed");
    }
    let _ = stdin.flush().await;

    tokio::time::sleep(Duration::from_millis(500)).await;

    if let Ok(Some(_status)) = child.try_wait() {
        let mut ebuf = vec![0u8; 4096];
        let en = stderr.read(&mut ebuf).await.unwrap_or(0);
        let mut error_output = if en > 0 {
            String::from_utf8_lossy(&ebuf[..en]).trim().to_string()
        } else {
            String::new()
        };
        if error_output.is_empty() {
            let mut obuf = vec![0u8; 4096];
            let on = reader.read(&mut obuf).await.unwrap_or(0);
            error_output = if on > 0 {
                String::from_utf8_lossy(&obuf[..on]).trim().to_string()
            } else {
                "Process exited".to_string()
            };
        }
        cleanup(child).await;
        return fail(format!("Server failed: {}", truncate(&error_output, 300)));
    }

    let init_resp = match read_mcp_message(&mut reader, Duration::from_secs(30)).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            let mut buf = vec![0u8; 4096];
            let n = stderr.read(&mut buf).await.unwrap_or(0);
            let s = if n > 0 {
                String::from_utf8_lossy(&buf[..n]).trim().to_string()
            } else {
                "No output".to_string()
            };
            cleanup(child).await;
            return fail(format!(
                "Server closed without response: {}",
                truncate(&s, 300)
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            if let Ok(Some(_)) = child.try_wait() {
                let mut buf = vec![0u8; 1024];
                let n = stderr.read(&mut buf).await.unwrap_or(0);
                let s = if n > 0 {
                    String::from_utf8_lossy(&buf[..n]).trim().to_string()
                } else {
                    "Unknown error".to_string()
                };
                cleanup(child).await;
                return fail(format!("Server exited: {}", truncate(&s, 200)));
            }
            cleanup(child).await;
            return fail("Server did not respond within timeout");
        }
        Err(e) => {
            cleanup(child).await;
            return fail(format!("Invalid JSON response: {}", e));
        }
    };

    if let Some(result) = init_resp.get("result") {
        let server_info = result
            .get("serverInfo")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let server_name = str_field(&server_info, "name")
            .unwrap_or("unknown")
            .to_string();
        let server_version = server_info.get("version").cloned().unwrap_or(Value::Null);
        let capabilities = result
            .get("capabilities")
            .cloned()
            .unwrap_or_else(|| json!({}));

        async fn send_jsonrpc(
            stdin: &mut tokio::process::ChildStdin,
            reader: &mut BufReader<tokio::process::ChildStdout>,
            method: &str,
            req_id: i64,
            timeout_s: u64,
        ) -> Option<Value> {
            let req = json!({"jsonrpc":"2.0","id":req_id,"method":method,"params":{}});
            let msg = format!("{}\n", req);
            stdin.write_all(msg.as_bytes()).await.ok()?;
            stdin.flush().await.ok()?;
            read_mcp_message(reader, Duration::from_secs(timeout_s))
                .await
                .ok()
                .flatten()
        }

        let mut tools = Vec::new();
        let mut tool_count = 0i64;
        if let Some(tr) = send_jsonrpc(&mut stdin, &mut reader, "tools/list", 2, 10).await {
            if let Some(list) = tr
                .get("result")
                .and_then(|r| r.get("tools"))
                .and_then(|v| v.as_array())
            {
                tool_count = list.len() as i64;
                for t in list.iter().take(MAX_CACHED_ITEMS) {
                    tools.push(json!({
                        "name": str_field(t, "name").unwrap_or("unknown"),
                        "description": t.get("description").cloned().unwrap_or(Value::Null),
                        "inputSchema": t.get("inputSchema").cloned().unwrap_or(Value::Null),
                    }));
                }
            }
        }

        let mut resources = Vec::new();
        let mut resource_count = 0i64;
        if capabilities.get("resources").is_some() {
            if let Some(rr) = send_jsonrpc(&mut stdin, &mut reader, "resources/list", 3, 5).await {
                if let Some(list) = rr
                    .get("result")
                    .and_then(|r| r.get("resources"))
                    .and_then(|v| v.as_array())
                {
                    resource_count = list.len() as i64;
                    for r in list.iter().take(MAX_CACHED_ITEMS) {
                        resources.push(json!({
                            "uri": str_field(r, "uri").unwrap_or(""),
                            "name": str_field(r, "name").unwrap_or(""),
                            "description": r.get("description").cloned().unwrap_or(Value::Null),
                            "mimeType": r.get("mimeType").cloned().unwrap_or(Value::Null),
                        }));
                    }
                }
            }
        }

        let mut prompts = Vec::new();
        let mut prompt_count = 0i64;
        if capabilities.get("prompts").is_some() {
            if let Some(pr) = send_jsonrpc(&mut stdin, &mut reader, "prompts/list", 4, 5).await {
                if let Some(list) = pr
                    .get("result")
                    .and_then(|r| r.get("prompts"))
                    .and_then(|v| v.as_array())
                {
                    prompt_count = list.len() as i64;
                    for p in list.iter().take(MAX_CACHED_ITEMS) {
                        let arguments = p.get("arguments").and_then(|v| v.as_array()).map(|aa| {
                            aa.iter()
                                .map(|a| {
                                    json!({
                                        "name": str_field(a, "name").unwrap_or(""),
                                        "description": a.get("description").cloned().unwrap_or(Value::Null),
                                        "required": a.get("required").cloned().unwrap_or(Value::Null),
                                    })
                                })
                                .collect::<Vec<_>>()
                        });
                        prompts.push(json!({
                            "name": str_field(p, "name").unwrap_or(""),
                            "description": p.get("description").cloned().unwrap_or(Value::Null),
                            "arguments": arguments,
                        }));
                    }
                }
            }
        }

        cleanup(child).await;
        return json!({
            "success": true,
            "message": format!("MCP server '{}' initialized successfully", server_name),
            "server_name": server_name,
            "server_version": server_version,
            "tools": if tools.is_empty() { Value::Null } else { json!(tools) },
            "tool_count": tool_count,
            "resources": if resources.is_empty() { Value::Null } else { json!(resources) },
            "resource_count": resource_count,
            "prompts": if prompts.is_empty() { Value::Null } else { json!(prompts) },
            "prompt_count": prompt_count,
            "capabilities": if capabilities.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                Value::Null
            } else {
                capabilities
            },
        });
    } else if let Some(err) = init_resp.get("error") {
        let msg = str_field(err, "message")
            .unwrap_or("Unknown error")
            .to_string();
        cleanup(child).await;
        return fail(format!("MCP error: {}", msg));
    }

    cleanup(child).await;
    fail(format!("Server responded (command: {})", command))
}

async fn http_probe(server: &Value, url: &str) -> Value {
    let mut headers: HashMap<String, String> = server
        .get("headers")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    headers.insert(
        "Accept".into(),
        "application/json, text/event-stream".into(),
    );

    let server_name = str_field(server, "name").unwrap_or("");
    if let Some(tok) = creds_get_mcp_token(server_name, url) {
        headers.insert("Authorization".into(), format!("Bearer {}", tok));
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => return fail(format!("Unexpected error: {}", e)),
    };

    let to_header_map = |h: &HashMap<String, String>| -> reqwest::header::HeaderMap {
        let mut hm = reqwest::header::HeaderMap::new();
        for (k, v) in h {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                hm.insert(name, val);
            }
        }
        hm
    };

    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "claude-deck-test", "version": "1.0.0"},
        },
    });

    let resp = match timeout(
        Duration::from_secs(10),
        client
            .post(url)
            .headers(to_header_map(&headers))
            .json(&init_request)
            .send(),
    )
    .await
    {
        Ok(Ok(r)) => r,
        Ok(Err(e)) if e.is_timeout() => return fail("Connection timeout"),
        Ok(Err(e)) => return fail(format!("Request error: {}", e)),
        Err(_) => return fail("Connection timeout"),
    };

    if let Some(sid) = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
    {
        headers.insert("mcp-session-id".into(), sid.to_string());
    }

    let status = resp.status();
    if status.as_u16() >= 400 {
        return fail(format!(
            "HTTP server returned error status {}",
            status.as_u16()
        ));
    }

    let resp_data: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return fail(format!("Unexpected error: {}", e)),
    };

    if let Some(err) = resp_data.get("error") {
        let msg = str_field(err, "message").unwrap_or("Unknown error");
        return fail(format!("MCP error: {}", msg));
    }
    let result = match resp_data.get("result") {
        Some(r) => r.clone(),
        None => {
            return json!({
                "success": true,
                "message": format!("Server responded (status {})", status.as_u16()),
            });
        }
    };

    let server_info = result
        .get("serverInfo")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let server_name_val = str_field(&server_info, "name")
        .unwrap_or("unknown")
        .to_string();
    let server_version = server_info.get("version").cloned().unwrap_or(Value::Null);
    let capabilities = result
        .get("capabilities")
        .cloned()
        .unwrap_or_else(|| json!({}));

    async fn http_jsonrpc(
        client: &reqwest::Client,
        url: &str,
        hm: reqwest::header::HeaderMap,
        method: &str,
        req_id: i64,
        timeout_s: u64,
    ) -> Option<Value> {
        let req = json!({"jsonrpc":"2.0","id":req_id,"method":method,"params":{}});
        let r = timeout(
            Duration::from_secs(timeout_s),
            client.post(url).headers(hm).json(&req).send(),
        )
        .await
        .ok()?
        .ok()?;
        if r.status().as_u16() < 400 {
            r.json().await.ok()
        } else {
            None
        }
    }

    let mut tools = Vec::new();
    let mut tool_count = 0i64;
    if let Some(tr) = http_jsonrpc(&client, url, to_header_map(&headers), "tools/list", 2, 10).await
    {
        if let Some(list) = tr
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|v| v.as_array())
        {
            tool_count = list.len() as i64;
            for t in list.iter().take(MAX_CACHED_ITEMS) {
                tools.push(json!({
                    "name": str_field(t, "name").unwrap_or("unknown"),
                    "description": t.get("description").cloned().unwrap_or(Value::Null),
                    "inputSchema": t.get("inputSchema").cloned().unwrap_or(Value::Null),
                }));
            }
        }
    }

    let mut resources = Vec::new();
    let mut resource_count = 0i64;
    if capabilities.get("resources").is_some() {
        if let Some(rr) = http_jsonrpc(
            &client,
            url,
            to_header_map(&headers),
            "resources/list",
            3,
            5,
        )
        .await
        {
            if let Some(list) = rr
                .get("result")
                .and_then(|r| r.get("resources"))
                .and_then(|v| v.as_array())
            {
                resource_count = list.len() as i64;
                for r in list.iter().take(MAX_CACHED_ITEMS) {
                    resources.push(json!({
                        "uri": str_field(r, "uri").unwrap_or(""),
                        "name": str_field(r, "name").unwrap_or(""),
                        "description": r.get("description").cloned().unwrap_or(Value::Null),
                        "mimeType": r.get("mimeType").cloned().unwrap_or(Value::Null),
                    }));
                }
            }
        }
    }

    let mut prompts = Vec::new();
    let mut prompt_count = 0i64;
    if capabilities.get("prompts").is_some() {
        if let Some(pr) =
            http_jsonrpc(&client, url, to_header_map(&headers), "prompts/list", 4, 5).await
        {
            if let Some(list) = pr
                .get("result")
                .and_then(|r| r.get("prompts"))
                .and_then(|v| v.as_array())
            {
                prompt_count = list.len() as i64;
                for p in list.iter().take(MAX_CACHED_ITEMS) {
                    let arguments = p.get("arguments").and_then(|v| v.as_array()).map(|aa| {
                        aa.iter()
                            .map(|a| {
                                json!({
                                    "name": str_field(a, "name").unwrap_or(""),
                                    "description": a.get("description").cloned().unwrap_or(Value::Null),
                                    "required": a.get("required").cloned().unwrap_or(Value::Null),
                                })
                            })
                            .collect::<Vec<_>>()
                    });
                    prompts.push(json!({
                        "name": str_field(p, "name").unwrap_or(""),
                        "description": p.get("description").cloned().unwrap_or(Value::Null),
                        "arguments": arguments,
                    }));
                }
            }
        }
    }

    json!({
        "success": true,
        "message": format!("MCP server '{}' initialized successfully", server_name_val),
        "server_name": server_name_val,
        "server_version": server_version,
        "tools": if tools.is_empty() { Value::Null } else { json!(tools) },
        "tool_count": tool_count,
        "resources": if resources.is_empty() { Value::Null } else { json!(resources) },
        "resource_count": resource_count,
        "prompts": if prompts.is_empty() { Value::Null } else { json!(prompts) },
        "prompt_count": prompt_count,
        "capabilities": if capabilities.as_object().map(|m| m.is_empty()).unwrap_or(true) {
            Value::Null
        } else {
            capabilities
        },
    })
}

async fn sse_probe(server: &Value, url: &str) -> Value {
    let mut headers: HashMap<String, String> = server
        .get("headers")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    headers.insert("Accept".into(), "text/event-stream".into());

    let server_name = str_field(server, "name").unwrap_or("");
    if let Some(tok) = creds_get_mcp_token(server_name, url) {
        headers.insert("Authorization".into(), format!("Bearer {}", tok));
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return fail(format!("Unexpected error: {}", e)),
    };
    let mut hm = reqwest::header::HeaderMap::new();
    for (k, v) in &headers {
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            reqwest::header::HeaderValue::from_str(v),
        ) {
            hm.insert(name, val);
        }
    }

    let resp = match timeout(Duration::from_secs(5), client.get(url).headers(hm).send()).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) if e.is_timeout() => return fail("Connection timeout"),
        Ok(Err(e)) => return fail(format!("Request error: {}", e)),
        Err(_) => return fail("Connection timeout"),
    };

    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if status < 400 {
        if content_type.contains("text/event-stream") {
            json!({
                "success": true,
                "message": format!("SSE server connected (status {})", status),
            })
        } else {
            json!({
                "success": true,
                "message": format!("Server responded (status {}, type: {})", status, content_type),
            })
        }
    } else {
        fail(format!("SSE server returned error status {}", status))
    }
}

/// POST /api/v1/mcp/servers/{name}/test
async fn test_mcp_server_connection(
    State(state): State<ApiState>,
    Path(name): Path<String>,
    Query(q): Query<ScopeQuery>,
) -> AppResult<Json<Value>> {
    if !["user", "project", "plugin", "managed"].contains(&q.scope.as_str()) {
        return Err(AppError::bad_request(
            "Server scope must be 'user', 'project', 'plugin', or 'managed'",
        ));
    }
    let result = test_connection(
        &state.pool,
        &name,
        &q.scope,
        true,
        state.enable_external_tools,
        &state.cwd_fallback,
    )
    .await;
    Ok(Json(test_response_body(&result)))
}

/// POST /api/v1/mcp/servers/test-all
async fn test_all_servers(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let servers =
        build_server_list(&state.pool, q.project_path.as_deref(), &state.cwd_fallback).await;
    let mut results = Vec::new();
    for s in &servers {
        let name = s["name"].as_str().unwrap_or("").to_string();
        let scope = s["scope"].as_str().unwrap_or("").to_string();
        let tr = test_connection(
            &state.pool,
            &name,
            &scope,
            true,
            state.enable_external_tools,
            &state.cwd_fallback,
        )
        .await;
        results.push(json!({
            "server_name": name,
            "scope": scope,
            "success": tr.get("success").and_then(|v| v.as_bool()).unwrap_or(false),
            "message": tr.get("message").and_then(|v| v.as_str()).unwrap_or(""),
            "tool_count": tr.get("tool_count").cloned().unwrap_or(Value::Null),
            "resource_count": tr.get("resource_count").cloned().unwrap_or(Value::Null),
            "prompt_count": tr.get("prompt_count").cloned().unwrap_or(Value::Null),
        }));
    }
    Ok(Json(json!({ "results": results })))
}

// ---- approval settings ------------------------------------------------------

/// GET /api/v1/mcp/approval-settings
async fn get_approval_settings() -> AppResult<Json<Value>> {
    Ok(Json(read_approval_settings()))
}

fn read_approval_settings() -> Value {
    let config = read_json_file(&paths::get_claude_user_settings_file());
    let Some(config) = config else {
        return json!({ "default_mode": "ask-every-time", "server_overrides": [] });
    };
    let mcp = config
        .get("mcpServerApproval")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let default_mode = str_field(&mcp, "defaultMode")
        .unwrap_or("ask-every-time")
        .to_string();
    let mut overrides = Vec::new();
    if let Some(so) = mcp.get("serverOverrides").and_then(|v| v.as_object()) {
        for (server_name, mode) in so {
            overrides.push(json!({
                "server_name": server_name,
                "mode": mode.as_str().unwrap_or(""),
            }));
        }
    }
    json!({ "default_mode": default_mode, "server_overrides": overrides })
}

#[derive(Deserialize)]
struct ApprovalUpdateBody {
    #[serde(default)]
    default_mode: Option<String>,
    #[serde(default)]
    server_overrides: Option<Vec<ApprovalOverride>>,
}

#[derive(Deserialize, Clone)]
struct ApprovalOverride {
    server_name: String,
    mode: String,
}

/// PUT /api/v1/mcp/approval-settings
async fn update_approval_settings(Json(body): Json<ApprovalUpdateBody>) -> AppResult<Json<Value>> {
    let current = read_approval_settings();
    let default_mode = body.default_mode.clone().unwrap_or_else(|| {
        current["default_mode"]
            .as_str()
            .unwrap_or("ask-every-time")
            .to_string()
    });

    let overrides: Vec<ApprovalOverride> = match body.server_overrides {
        Some(v) => v,
        None => current["server_overrides"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| {
                        Some(ApprovalOverride {
                            server_name: o.get("server_name")?.as_str()?.to_string(),
                            mode: o.get("mode")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };

    let path = paths::get_claude_user_settings_file();
    let mut config = read_json_file(&path).unwrap_or_else(|| json!({}));
    if !config.is_object() {
        config = json!({});
    }
    let mut so_map = Map::new();
    for o in &overrides {
        so_map.insert(o.server_name.clone(), json!(o.mode));
    }
    config.as_object_mut().unwrap().insert(
        "mcpServerApproval".into(),
        json!({ "defaultMode": default_mode, "serverOverrides": so_map }),
    );
    write_json_file(&path, &config).await;

    Ok(Json(json!({
        "default_mode": default_mode,
        "server_overrides": overrides
            .iter()
            .map(|o| json!({ "server_name": o.server_name, "mode": o.mode }))
            .collect::<Vec<_>>(),
    })))
}

// ---- credentials (port of credentials_service.py, OAuth subset) -------------

fn credentials_path() -> PathBuf {
    paths::get_claude_user_config_dir().join(".credentials.json")
}

fn read_credentials() -> Value {
    let path = credentials_path();
    if !path.exists() {
        return json!({});
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_credentials(data: &Value) {
    let path = credentials_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(data) {
        if std::fs::write(&path, s).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

fn make_cred_key(server_name: &str, server_url: &str) -> String {
    let mut h = Sha256::new();
    h.update(server_url.as_bytes());
    let digest = hex::encode(h.finalize());
    format!("{}|{}", server_name, &digest[..16])
}

fn find_cred_entry(server_name: &str, server_url: Option<&str>) -> Option<Value> {
    let creds = read_credentials();
    let mcp_oauth = creds.get("mcpOAuth").and_then(|v| v.as_object()).cloned()?;

    if let Some(url) = server_url {
        let key = make_cred_key(server_name, url);
        if let Some(entry) = mcp_oauth.get(&key) {
            if entry
                .get("accessToken")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            {
                return Some(entry.clone());
            }
        }
    }

    let prefix = format!("{}|", server_name);
    let matches: Vec<&Value> = mcp_oauth
        .iter()
        .filter(|(k, _)| k.starts_with(&prefix))
        .map(|(_, v)| v)
        .collect();
    if matches.is_empty() {
        return None;
    }
    if let Some(e) = matches.iter().find(|e| {
        e.get("accessToken")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }) {
        return Some((*e).clone());
    }
    Some(matches[0].clone())
}

fn creds_get_mcp_token(server_name: &str, server_url: &str) -> Option<String> {
    let entry = find_cred_entry(server_name, Some(server_url))?;
    let token = entry.get("accessToken").and_then(|v| v.as_str())?;
    if token.is_empty() {
        return None;
    }
    let expires_at = entry
        .get("expiresAt")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if expires_at != 0.0 && unix_now() > expires_at {
        return None;
    }
    Some(token.to_string())
}

fn creds_get_auth_status(server_name: &str) -> Value {
    let entry = match find_cred_entry(server_name, None) {
        Some(e) => e,
        None => {
            return json!({
                "has_token": false,
                "expired": false,
                "server_url": Value::Null,
                "has_client_registration": Value::Null,
            });
        }
    };
    let access_token = entry
        .get("accessToken")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if access_token.is_empty() {
        let has_client = entry
            .get("clientId")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        return json!({
            "has_token": false,
            "expired": false,
            "server_url": entry.get("serverUrl").cloned().unwrap_or(Value::Null),
            "has_client_registration": has_client,
        });
    }
    let expires_at = entry
        .get("expiresAt")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let expired = expires_at != 0.0 && unix_now() > expires_at;
    json!({
        "has_token": true,
        "expired": expired,
        "server_url": entry.get("serverUrl").cloned().unwrap_or(Value::Null),
        "has_client_registration": Value::Null,
    })
}

#[allow(clippy::too_many_arguments)]
fn creds_store_token(
    server_name: &str,
    server_url: &str,
    client_id: &str,
    client_secret: Option<&str>,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_at: i64,
) {
    let mut creds = read_credentials();
    if !creds.is_object() {
        creds = json!({});
    }
    let obj = creds.as_object_mut().unwrap();
    if !obj.contains_key("mcpOAuth") {
        obj.insert("mcpOAuth".into(), json!({}));
    }
    let key = make_cred_key(server_name, server_url);
    obj["mcpOAuth"].as_object_mut().unwrap().insert(
        key,
        json!({
            "accessToken": access_token,
            "refreshToken": refresh_token,
            "expiresAt": expires_at,
            "clientId": client_id,
            "clientSecret": client_secret,
            "serverUrl": server_url,
        }),
    );
    write_credentials(&creds);
}

// ---- OAuth flow (port of oauth_service.py) ----------------------------------

#[derive(Clone)]
struct PendingAuth {
    server_name: String,
    server_url: String,
    code_verifier: String,
    token_endpoint: String,
    client_id: String,
    client_secret: Option<String>,
    redirect_uri: String,
    created_at: f64,
}

fn pending_store() -> &'static Mutex<HashMap<String, PendingAuth>> {
    static STORE: OnceLock<Mutex<HashMap<String, PendingAuth>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

const PENDING_TTL: f64 = 300.0;
const CALLBACK_PATH: &str = "/api/v1/mcp/auth/callback";

fn cleanup_expired_pending() {
    let now = unix_now();
    if let Ok(mut store) = pending_store().lock() {
        store.retain(|_, v| now - v.created_at <= PENDING_TTL);
    }
}

fn rand_token(byte_len: usize) -> String {
    // token_urlsafe-style: base64url of random bytes, no padding.
    let mut bytes = vec![0u8; byte_len];
    if getrandom(&mut bytes).is_err() {
        let seed = unix_now().to_bits();
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = ((seed >> (i % 8 * 8)) as u8) ^ (i as u8).wrapping_mul(31);
        }
    }
    base64url_nopad(&bytes)
}

fn getrandom(buf: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom")?;
    f.read_exact(buf)
}

fn base64url_nopad(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in input.chunks(3) {
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

fn generate_pkce() -> (String, String) {
    let verifier: String = rand_token(64).chars().take(128).collect();
    let mut h = Sha256::new();
    h.update(verifier.as_bytes());
    let digest = h.finalize();
    let challenge = base64url_nopad(&digest);
    (verifier, challenge)
}

async fn discover_oauth_metadata(server_url: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let probe = client.get(server_url).send().await;
    let www_auth = match &probe {
        Ok(r) if r.status().as_u16() == 401 => r
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    };

    let mut resource_metadata_url: Option<String> = None;
    if www_auth.contains("resource_metadata") {
        for part in www_auth.split(',') {
            let part = part.trim();
            if let Some(rest) = part.strip_prefix("resource_metadata=") {
                resource_metadata_url = Some(rest.trim_matches('"').to_string());
                break;
            }
        }
    }

    let mut auth_server_url: Option<String> = None;
    if let Some(rm_url) = &resource_metadata_url {
        if let Ok(rm) = client.get(rm_url).send().await {
            if rm.status().as_u16() == 200 {
                if let Ok(rm_data) = rm.json::<Value>().await {
                    if let Some(arr) = rm_data
                        .get("authorization_servers")
                        .and_then(|v| v.as_array())
                    {
                        if let Some(first) = arr.first().and_then(|v| v.as_str()) {
                            auth_server_url = Some(first.to_string());
                        }
                    }
                }
            }
        }
    }

    if auth_server_url.is_none() {
        // Derive scheme://netloc from the server URL (Python urlparse).
        let (scheme, rest) = server_url
            .split_once("://")
            .ok_or_else(|| format!("Invalid server URL: {}", server_url))?;
        let netloc = rest.split(['/', '?', '#']).next().unwrap_or(rest);
        auth_server_url = Some(format!("{}://{}", scheme, netloc));
    }
    let base = auth_server_url.unwrap();
    let base = base.trim_end_matches('/');

    let well_known = format!("{}/.well-known/oauth-authorization-server", base);
    if let Ok(r) = client.get(&well_known).send().await {
        if r.status().as_u16() == 200 {
            if let Ok(v) = r.json::<Value>().await {
                return Ok(v);
            }
        }
    }
    let openid = format!("{}/.well-known/openid-configuration", base);
    if let Ok(r) = client.get(&openid).send().await {
        if r.status().as_u16() == 200 {
            if let Ok(v) = r.json::<Value>().await {
                return Ok(v);
            }
        }
    }

    Err(format!(
        "Could not discover OAuth metadata for {}. Server may not support MCP OAuth authentication.",
        server_url
    ))
}

async fn register_client(
    registration_endpoint: &str,
    redirect_uri: &str,
    server_name: &str,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let reg_data = json!({
        "client_name": format!("Claude Deck - {}", server_name),
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    let resp = client
        .post(registration_endpoint)
        .json(&reg_data)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status == 200 || status == 201 {
        resp.json::<Value>().await.map_err(|e| e.to_string())
    } else {
        let text = resp.text().await.unwrap_or_default();
        Err(format!("Client registration failed: {} {}", status, text))
    }
}

async fn oauth_start_auth(
    server_url: &str,
    server_name: &str,
    callback_base_url: &str,
) -> Result<Value, String> {
    cleanup_expired_pending();

    let metadata = discover_oauth_metadata(server_url).await?;
    let authorization_endpoint = metadata
        .get("authorization_endpoint")
        .and_then(|v| v.as_str())
        .map(String::from);
    let token_endpoint = metadata
        .get("token_endpoint")
        .and_then(|v| v.as_str())
        .map(String::from);
    let registration_endpoint = metadata
        .get("registration_endpoint")
        .and_then(|v| v.as_str())
        .map(String::from);

    let (authorization_endpoint, token_endpoint) = match (authorization_endpoint, token_endpoint) {
        (Some(a), Some(t)) => (a, t),
        _ => {
            return Err(
                "OAuth metadata missing authorization_endpoint or token_endpoint".to_string(),
            );
        }
    };

    let redirect_uri = format!("{}{}", callback_base_url, CALLBACK_PATH);

    let mut client_id: Option<String> = None;
    let mut client_secret: Option<String> = None;
    if let Some(reg_ep) = &registration_endpoint {
        match register_client(reg_ep, &redirect_uri, server_name).await {
            Ok(reg) => {
                client_id = reg
                    .get("client_id")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                client_secret = reg
                    .get("client_secret")
                    .and_then(|v| v.as_str())
                    .map(String::from);
            }
            Err(e) => {
                return Err(format!(
                    "OAuth client registration failed: {}. The server may not support dynamic client registration.",
                    e
                ));
            }
        }
    }

    let client_id = match client_id {
        Some(c) => c,
        None => {
            return Err(
                "OAuth metadata does not include a registration_endpoint. This server may require manual client registration.".to_string(),
            )
        }
    };

    let (code_verifier, code_challenge) = generate_pkce();
    let state = rand_token(32);

    if let Ok(mut store) = pending_store().lock() {
        store.insert(
            state.clone(),
            PendingAuth {
                server_name: server_name.to_string(),
                server_url: server_url.to_string(),
                code_verifier,
                token_endpoint,
                client_id: client_id.clone(),
                client_secret,
                redirect_uri: redirect_uri.clone(),
                created_at: unix_now(),
            },
        );
    }

    let mut params = vec![
        ("response_type", "code".to_string()),
        ("client_id", client_id.clone()),
        ("redirect_uri", redirect_uri.clone()),
        ("state", state.clone()),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256".to_string()),
    ];
    if let Some(scopes) = metadata.get("scopes_supported").and_then(|v| v.as_array()) {
        let joined = scopes
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if !joined.is_empty() {
            params.push(("scope", joined));
        }
    }
    let auth_url = format!("{}?{}", authorization_endpoint, build_query(&params));

    Ok(json!({ "auth_url": auth_url, "state": state }))
}

async fn oauth_handle_callback(code: &str, state: &str) -> Result<Value, String> {
    cleanup_expired_pending();

    let pending = {
        let store = pending_store().lock().map_err(|_| "lock".to_string())?;
        store.get(state).cloned()
    };
    let pending = match pending {
        Some(p) => p,
        None => {
            return Err(
                "Invalid or expired OAuth state. Please try authenticating again.".to_string(),
            );
        }
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", pending.redirect_uri.clone()),
        ("client_id", pending.client_id.clone()),
        ("code_verifier", pending.code_verifier.clone()),
    ];
    if let Some(cs) = &pending.client_secret {
        form.push(("client_secret", cs.clone()));
    }
    let body = build_query(&form);

    let resp = client
        .post(&pending.token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status != 200 && status != 201 {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Token exchange failed: {} {}", status, text));
    }
    let token_resp: Value = resp.json().await.map_err(|e| e.to_string())?;

    let access_token = match token_resp.get("access_token").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return Err("Token response missing access_token".to_string()),
    };
    let refresh_token = token_resp
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(String::from);
    let expires_in = token_resp
        .get("expires_in")
        .and_then(|v| v.as_f64())
        .unwrap_or(3600.0);
    let expires_at = (unix_now() + expires_in) as i64;

    creds_store_token(
        &pending.server_name,
        &pending.server_url,
        &pending.client_id,
        pending.client_secret.as_deref(),
        &access_token,
        refresh_token.as_deref(),
        expires_at,
    );

    if let Ok(mut store) = pending_store().lock() {
        store.remove(state);
    }

    Ok(json!({ "success": true, "server_name": pending.server_name }))
}

// ---- handlers: auth ---------------------------------------------------------

/// GET /api/v1/mcp/servers/{name}/auth-status
async fn get_auth_status(
    Path(name): Path<String>,
    Query(_q): Query<AuthScopeQuery>,
) -> AppResult<Json<Value>> {
    Ok(Json(creds_get_auth_status(&name)))
}

#[derive(Deserialize)]
struct AuthStartQuery {
    #[serde(default = "default_user_scope")]
    scope: String,
}

/// POST /api/v1/mcp/servers/{name}/auth/start
async fn start_auth(
    State(state): State<ApiState>,
    Path(name): Path<String>,
    Query(q): Query<AuthStartQuery>,
    headers: HeaderMap,
) -> AppResult<Json<Value>> {
    let server = get_server(&name, &q.scope, &state.cwd_fallback).ok_or_else(|| {
        AppError::not_found(format!(
            "Server '{}' not found in '{}' scope",
            name, q.scope
        ))
    })?;
    let url = match str_field(&server, "url") {
        Some(u) if !u.is_empty() => u.to_string(),
        _ => {
            return Err(AppError::bad_request(
                "OAuth authentication is only supported for HTTP/SSE servers with a URL",
            ));
        }
    };

    // Derive callback base from incoming request (scheme + host[:port]).
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let callback_base_url = format!("{}://{}", scheme, host);

    match oauth_start_auth(&url, &name, &callback_base_url).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => Err(AppError::bad_request(e)),
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

fn success_html(server_name: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head><title>Authentication Successful</title>
<style>
  body {{ font-family: system-ui, sans-serif; display: flex; align-items: center;
         justify-content: center; min-height: 100vh; margin: 0;
         background: #f8fafc; color: #1e293b; }}
  .card {{ text-align: center; padding: 2rem; border-radius: 12px;
           background: white; box-shadow: 0 1px 3px rgba(0,0,0,0.1); max-width: 400px; }}
  .check {{ font-size: 3rem; margin-bottom: 1rem; }}
  h1 {{ font-size: 1.25rem; margin: 0 0 0.5rem; }}
  p {{ color: #64748b; margin: 0; font-size: 0.875rem; }}
</style>
</head>
<body>
  <div class="card">
    <div class="check">&#10003;</div>
    <h1>Authenticated!</h1>
    <p>Server <strong>{name}</strong> has been authenticated.<br>You can close this tab.</p>
  </div>
  <script>
    // Notify opener and auto-close after a short delay
    if (window.opener) {{
      window.opener.postMessage({{ type: 'mcp-oauth-complete', serverName: '{name}' }}, '*');
    }}
    setTimeout(() => window.close(), 3000);
  </script>
</body>
</html>"#,
        name = server_name
    )
}

fn failure_html(err: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head><title>Authentication Failed</title>
<style>
  body {{ font-family: system-ui, sans-serif; display: flex; align-items: center;
         justify-content: center; min-height: 100vh; margin: 0;
         background: #fef2f2; color: #991b1b; }}
  .card {{ text-align: center; padding: 2rem; border-radius: 12px;
           background: white; box-shadow: 0 1px 3px rgba(0,0,0,0.1); max-width: 400px; }}
  .x {{ font-size: 3rem; margin-bottom: 1rem; }}
  h1 {{ font-size: 1.25rem; margin: 0 0 0.5rem; }}
  p {{ color: #64748b; margin: 0; font-size: 0.875rem; }}
</style>
</head>
<body>
  <div class="card">
    <div class="x">&#10007;</div>
    <h1>Authentication Failed</h1>
    <p>{err}</p>
  </div>
</body>
</html>"#,
        err = err
    )
}

/// GET /api/v1/mcp/auth/callback
async fn auth_callback(Query(q): Query<CallbackQuery>) -> Response {
    match oauth_handle_callback(&q.code, &q.state).await {
        Ok(result) => {
            let server_name = result
                .get("server_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Html(success_html(server_name)).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Html(failure_html(&e))).into_response(),
    }
}

// ---- registry (port of mcp_registry_service.py) -----------------------------

#[derive(Deserialize)]
struct RegistrySearchQuery {
    q: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

/// GET /api/v1/mcp/registry/search
async fn search_registry(Query(q): Query<RegistrySearchQuery>) -> AppResult<Json<Value>> {
    let mut params: Vec<(&str, String)> = vec![
        ("limit", q.limit.to_string()),
        ("version", "latest".to_string()),
    ];
    if let Some(s) = &q.q {
        if !s.is_empty() {
            params.push(("search", s.clone()));
        }
    }
    if let Some(c) = &q.cursor {
        if !c.is_empty() {
            params.push(("cursor", c.clone()));
        }
    }
    let url = format!("{}/servers?{}", REGISTRY_BASE_URL, build_query(&params));
    registry_get(&url).await
}

/// GET /api/v1/mcp/registry/* — dispatches the two `:path` Python routes:
///   /registry/servers/{server_name:path}/versions/{version}
///   /registry/servers/{server_name:path}/versions
async fn registry_server_dispatch(Path(rest): Path<String>) -> AppResult<Json<Value>> {
    let stripped = rest
        .strip_prefix("servers/")
        .ok_or_else(|| AppError::not_found("Not Found"))?;

    if let Some(idx) = stripped.rfind("/versions/") {
        let server_name = &stripped[..idx];
        let version = &stripped[idx + "/versions/".len()..];
        let url = format!(
            "{}/servers/{}/versions/{}",
            REGISTRY_BASE_URL,
            quote_strict(server_name),
            version
        );
        return registry_get(&url).await;
    }
    if let Some(server_name) = stripped.strip_suffix("/versions") {
        let url = format!(
            "{}/servers/{}/versions",
            REGISTRY_BASE_URL,
            quote_strict(server_name)
        );
        return registry_get(&url).await;
    }
    Err(AppError::not_found("Not Found"))
}

async fn registry_get(url: &str) -> AppResult<Json<Value>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(REGISTRY_REQUEST_TIMEOUT))
        .build()
        .map_err(|e| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                format!("Registry API error: {}", e),
            )
        })?;
    let resp = client.get(url).send().await.map_err(|e| {
        AppError::new(
            StatusCode::BAD_GATEWAY,
            format!("Registry API error: {}", e),
        )
    })?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            format!("Registry API error: HTTP status {}", status.as_u16()),
        ));
    }
    let body: Value = resp.json().await.map_err(|e| {
        AppError::new(
            StatusCode::BAD_GATEWAY,
            format!("Registry API error: {}", e),
        )
    })?;
    Ok(Json(body))
}

#[derive(Deserialize)]
struct RegistryInstallBody {
    server_name: String,
    scope: String,
    #[serde(default)]
    package_registry_type: Option<String>,
    #[serde(default)]
    package_identifier: Option<String>,
    #[serde(default)]
    package_version: Option<String>,
    #[serde(default)]
    package_runtime_hint: Option<String>,
    #[serde(default)]
    package_arguments: Option<HashMap<String, String>>,
    #[serde(default)]
    remote_type: Option<String>,
    #[serde(default)]
    remote_url: Option<String>,
    #[serde(default)]
    remote_headers: Option<HashMap<String, String>>,
    #[serde(default)]
    env_values: Option<HashMap<String, String>>,
}

fn generate_package_config(
    registry_type: &str,
    identifier: &str,
    version: Option<&str>,
    runtime_hint: Option<&str>,
    arguments: Option<&HashMap<String, String>>,
) -> Value {
    let mut config = Map::new();
    config.insert("type".into(), json!("stdio"));
    let mut args: Vec<String> = Vec::new();

    match registry_type {
        "npm" => {
            let command = runtime_hint.unwrap_or("npx");
            config.insert("command".into(), json!(command));
            if command == "npx" {
                args.push("-y".to_string());
            }
            let pkg = match version {
                Some(v) => format!("{}@{}", identifier, v),
                None => identifier.to_string(),
            };
            args.push(pkg);
        }
        "pypi" => {
            config.insert("command".into(), json!(runtime_hint.unwrap_or("uvx")));
            args.push(identifier.to_string());
        }
        "oci" => {
            config.insert("command".into(), json!("docker"));
            args.extend(
                ["run", "-i", "--rm", identifier]
                    .iter()
                    .map(|s| s.to_string()),
            );
        }
        _ => {
            config.insert("command".into(), json!(runtime_hint.unwrap_or(identifier)));
            args.push(identifier.to_string());
        }
    }

    if let Some(arguments) = arguments {
        // Python iterates dict insertion order; HashMap is unordered. The
        // registry passes name/value flags whose order is not semantically
        // significant for the resulting command.
        for (name, value) in arguments {
            if !value.is_empty() {
                args.push(format!("--{}", name));
                args.push(value.clone());
            }
        }
    }

    config.insert("args".into(), json!(args));
    Value::Object(config)
}

fn generate_remote_config(
    remote_type: &str,
    url: &str,
    headers: Option<&HashMap<String, String>>,
) -> Value {
    let config_type = if remote_type == "streamable-http" {
        "http"
    } else {
        remote_type
    };
    let mut config = Map::new();
    config.insert("type".into(), json!(config_type));
    config.insert("url".into(), json!(url));
    if let Some(h) = headers {
        if !h.is_empty() {
            config.insert("headers".into(), json!(h));
        }
    }
    Value::Object(config)
}

/// POST /api/v1/mcp/registry/install
async fn install_registry_server(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
    Json(req): Json<RegistryInstallBody>,
) -> AppResult<Json<Value>> {
    if !["user", "project"].contains(&req.scope.as_str()) {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }

    let mut config = if req.package_registry_type.is_some() && req.package_identifier.is_some() {
        generate_package_config(
            req.package_registry_type.as_deref().unwrap(),
            req.package_identifier.as_deref().unwrap(),
            req.package_version.as_deref(),
            req.package_runtime_hint.as_deref(),
            req.package_arguments.as_ref(),
        )
    } else if req.remote_type.is_some() && req.remote_url.is_some() {
        generate_remote_config(
            req.remote_type.as_deref().unwrap(),
            req.remote_url.as_deref().unwrap(),
            req.remote_headers.as_ref(),
        )
    } else {
        Value::Object(Map::new())
    };

    if let Some(env) = &req.env_values {
        config
            .as_object_mut()
            .unwrap()
            .insert("env".into(), json!(env));
    }

    if config.as_object().map(|m| m.is_empty()).unwrap_or(true) {
        return Err(AppError::bad_request(
            "Must provide either package or remote transport info",
        ));
    }

    // install_server -> MCPService.add_server: writes the generated config
    // directly (no field-emptiness filtering).
    let written = if req.scope == "user" {
        let mut servers = read_user_mcp_config(None);
        servers.insert(req.server_name.clone(), config.clone());
        write_user_mcp_config(&servers).await
    } else {
        let mut servers = read_project_mcp_config(q.project_path.as_deref(), &state.cwd_fallback);
        servers.insert(req.server_name.clone(), config.clone());
        write_project_mcp_config(&servers, q.project_path.as_deref(), &state.cwd_fallback).await
    };
    if !written {
        return Err(AppError::internal("Failed to install server: write failed"));
    }

    Ok(Json(json!({
        "success": true,
        "server_name": req.server_name,
        "config": config,
        "scope": req.scope,
    })))
}
