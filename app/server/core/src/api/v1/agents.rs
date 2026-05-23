// PORTED: agent_service.py + skills_registry_service.py + skill_dependency_service.py + api/v1/agents.py
//
// Faithful port. Paths via `crate::paths`, errors via `crate::error::AppError`
// (body `{"detail": ...}`), route paths IDENTICAL to the Python router (the
// unchanged frontend was built against them).
//
// NEEDS-DEP: there is no YAML crate in Cargo.toml. Python uses `yaml.safe_load`
// for frontmatter. We ship a minimal YAML-subset parser (`yaml_min`) covering
// the constructs real agent/skill frontmatter uses: `key: scalar`, block
// sequences (`- item` / `- key: v`), nested mappings by indentation, flow
// lists `[a, b]`, quoted strings, bools/ints, and the empty document. Exotic
// YAML (anchors, multi-line block scalars `|`/`>`, complex keys) is NOT
// supported and degrades to a string scalar / empty map like Python's
// `yaml.YAMLError` fallback. If full fidelity is required, add `serde_yaml`.

use axum::{
    Router,
    extract::{Json, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::paths;

pub fn router() -> Router<ApiState> {
    // Paths mirror Python's `APIRouter(prefix="/agents")` exactly. Order
    // matters: the specific `/skills...` and `/skills/registry...` routes must
    // be registered before the catch-all `/{scope}/{name}`.
    Router::new()
        .route("/", get(list_agents).post(create_agent))
        .route("/skills", get(list_skills))
        .route("/skills/registry", get(browse_registry_skills))
        .route("/skills/registry/install", post(install_registry_skill))
        .route("/skills/{location}/{name}", get(get_skill))
        .route(
            "/skills/{location}/{name}/dependencies",
            get(check_skill_dependencies),
        )
        .route(
            "/skills/{location}/{name}/install",
            post(install_skill_dependencies),
        )
        .route("/skills/{location}/{name}/files", get(list_skill_files))
        .route(
            "/{scope}/{name}",
            get(get_agent).put(update_agent).delete(delete_agent),
        )
}

// ============================================================================
// Minimal YAML-subset parser (yaml.safe_load substitute)
// ============================================================================

mod yaml_min {
    use serde_json::{Map, Value};

    /// Parse a YAML-subset document into a JSON Value. Returns `{}` for an
    /// empty document, mirroring Python's `yaml.safe_load(...) or {}`.
    pub fn parse(input: &str) -> Value {
        let lines: Vec<&str> = input
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with('#')
            })
            .collect();
        if lines.is_empty() {
            return Value::Object(Map::new());
        }
        let mut idx = 0;
        let v = parse_block(&lines, &mut idx, indent_of(lines[0]));
        if v.is_null() {
            Value::Object(Map::new())
        } else {
            v
        }
    }

    fn indent_of(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    fn parse_block(lines: &[&str], idx: &mut usize, base_indent: usize) -> Value {
        if *idx >= lines.len() {
            return Value::Null;
        }
        let first = lines[*idx];
        let ind = indent_of(first);
        if ind < base_indent {
            return Value::Null;
        }
        if first.trim_start().starts_with("- ") || first.trim_start() == "-" {
            parse_sequence(lines, idx, ind)
        } else {
            parse_mapping(lines, idx, ind)
        }
    }

    fn parse_sequence(lines: &[&str], idx: &mut usize, seq_indent: usize) -> Value {
        let mut arr = Vec::new();
        while *idx < lines.len() {
            let line = lines[*idx];
            let ind = indent_of(line);
            if ind < seq_indent {
                break;
            }
            if ind > seq_indent {
                break;
            }
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("- ") || trimmed == "-") {
                break;
            }
            let rest = if trimmed == "-" { "" } else { &trimmed[2..] };
            *idx += 1;
            if rest.trim().is_empty() {
                // Nested block belonging to this list item.
                let child = parse_block(lines, idx, seq_indent + 1);
                arr.push(if child.is_null() { Value::Null } else { child });
            } else if let Some((k, v)) = split_kv(rest) {
                // Inline mapping start: `- key: value` plus possibly more
                // mapping keys indented under the dash column.
                let mut map = Map::new();
                insert_kv(&mut map, k, v, lines, idx, seq_indent + 2);
                let item_indent = seq_indent + 2;
                while *idx < lines.len() {
                    let l = lines[*idx];
                    let li = indent_of(l);
                    if li != item_indent {
                        break;
                    }
                    let lt = l.trim_start();
                    if lt.starts_with("- ") || lt == "-" {
                        break;
                    }
                    if let Some((k2, v2)) = split_kv(lt) {
                        *idx += 1;
                        insert_kv(&mut map, k2, v2, lines, idx, item_indent + 1);
                    } else {
                        break;
                    }
                }
                arr.push(Value::Object(map));
            } else {
                arr.push(scalar(rest.trim()));
            }
        }
        Value::Array(arr)
    }

    fn parse_mapping(lines: &[&str], idx: &mut usize, map_indent: usize) -> Value {
        let mut map = Map::new();
        while *idx < lines.len() {
            let line = lines[*idx];
            let ind = indent_of(line);
            if ind != map_indent {
                break;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("- ") || trimmed == "-" {
                break;
            }
            let Some((k, v)) = split_kv(trimmed) else {
                break;
            };
            *idx += 1;
            insert_kv(&mut map, k, v, lines, idx, map_indent + 1);
        }
        Value::Object(map)
    }

    fn insert_kv(
        map: &mut Map<String, Value>,
        key: String,
        raw_val: String,
        lines: &[&str],
        idx: &mut usize,
        child_indent: usize,
    ) {
        let v = raw_val.trim();
        if v.is_empty() {
            // Value is a nested block (mapping or sequence) on following lines.
            if *idx < lines.len() && indent_of(lines[*idx]) >= child_indent {
                let child = parse_block(lines, idx, indent_of(lines[*idx]));
                map.insert(key, if child.is_null() { Value::Null } else { child });
            } else {
                map.insert(key, Value::Null);
            }
        } else {
            map.insert(key, scalar(v));
        }
    }

    /// Split `key: value` honoring quoted keys. Returns None when there is no
    /// top-level colon (so the line is not a mapping entry).
    fn split_kv(s: &str) -> Option<(String, String)> {
        let bytes = s.as_bytes();
        let mut in_s = false;
        let mut in_d = false;
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i] as char;
            match c {
                '\'' if !in_d => in_s = !in_s,
                '"' if !in_s => in_d = !in_d,
                ':' if !in_s && !in_d => {
                    let next_is_space =
                        i + 1 >= bytes.len() || bytes[i + 1] == b' ' || bytes[i + 1] == b'\t';
                    if next_is_space {
                        let k = s[..i].trim();
                        let v = s[i + 1..].trim();
                        return Some((unquote(k), v.to_string()));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    fn unquote(s: &str) -> String {
        let s = s.trim();
        if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
            || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
        {
            s[1..s.len() - 1].to_string()
        } else {
            s.to_string()
        }
    }

    fn scalar(s: &str) -> Value {
        let s = s.trim();
        // Strip trailing inline comments only on unquoted scalars.
        if s.is_empty() {
            return Value::Null;
        }
        // Flow sequence: [a, b, c]
        if s.starts_with('[') && s.ends_with(']') {
            let inner = &s[1..s.len() - 1];
            let mut arr = Vec::new();
            if !inner.trim().is_empty() {
                for part in split_flow(inner) {
                    arr.push(scalar(part.trim()));
                }
            }
            return Value::Array(arr);
        }
        // Flow mapping: {a: b}
        if s.starts_with('{') && s.ends_with('}') {
            let inner = &s[1..s.len() - 1];
            let mut map = Map::new();
            if !inner.trim().is_empty() {
                for part in split_flow(inner) {
                    if let Some((k, v)) = part.split_once(':') {
                        map.insert(unquote(k.trim()), scalar(v.trim()));
                    }
                }
            }
            return Value::Object(map);
        }
        if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
            || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
        {
            return Value::String(s[1..s.len() - 1].to_string());
        }
        match s {
            "true" | "True" => return Value::Bool(true),
            "false" | "False" => return Value::Bool(false),
            "null" | "~" | "Null" | "None" => return Value::Null,
            _ => {}
        }
        if let Ok(i) = s.parse::<i64>() {
            return Value::Number(i.into());
        }
        if let Ok(f) = s.parse::<f64>() {
            if let Some(n) = serde_json::Number::from_f64(f) {
                return Value::Number(n);
            }
        }
        Value::String(s.to_string())
    }

    /// Split a flow collection body on top-level commas.
    fn split_flow(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut depth = 0i32;
        let mut in_s = false;
        let mut in_d = false;
        let mut cur = String::new();
        for c in s.chars() {
            match c {
                '\'' if !in_d => {
                    in_s = !in_s;
                    cur.push(c);
                }
                '"' if !in_s => {
                    in_d = !in_d;
                    cur.push(c);
                }
                '[' | '{' if !in_s && !in_d => {
                    depth += 1;
                    cur.push(c);
                }
                ']' | '}' if !in_s && !in_d => {
                    depth -= 1;
                    cur.push(c);
                }
                ',' if depth == 0 && !in_s && !in_d => {
                    out.push(std::mem::take(&mut cur));
                }
                _ => cur.push(c),
            }
        }
        if !cur.trim().is_empty() || !out.is_empty() {
            out.push(cur);
        }
        out
    }

    /// Serialize a metadata map back to a YAML-subset block, approximating
    /// `yaml.dump(metadata, default_flow_style=False, allow_unicode=True)`.
    pub fn dump(value: &Value) -> String {
        let mut out = String::new();
        dump_value(value, 0, &mut out, false);
        out
    }

    fn dump_value(value: &Value, indent: usize, out: &mut String, _inline: bool) {
        match value {
            Value::Object(m) => {
                for (k, v) in m {
                    dump_kv(k, v, indent, out);
                }
            }
            _ => {}
        }
    }

    fn dump_kv(key: &str, v: &Value, indent: usize, out: &mut String) {
        let pad = " ".repeat(indent);
        match v {
            Value::Object(m) => {
                if m.is_empty() {
                    out.push_str(&format!("{}{}: {{}}\n", pad, key));
                } else {
                    out.push_str(&format!("{}{}:\n", pad, key));
                    for (k2, v2) in m {
                        dump_kv(k2, v2, indent + 2, out);
                    }
                }
            }
            Value::Array(a) => {
                if a.is_empty() {
                    out.push_str(&format!("{}{}: []\n", pad, key));
                } else {
                    out.push_str(&format!("{}{}:\n", pad, key));
                    for item in a {
                        dump_seq_item(item, indent, out);
                    }
                }
            }
            other => {
                out.push_str(&format!("{}{}: {}\n", pad, key, scalar_repr(other)));
            }
        }
    }

    fn dump_seq_item(item: &Value, indent: usize, out: &mut String) {
        let pad = " ".repeat(indent);
        match item {
            Value::Object(m) => {
                if m.is_empty() {
                    out.push_str(&format!("{}- {{}}\n", pad));
                    return;
                }
                let mut first = true;
                for (k, v) in m {
                    if first {
                        first = false;
                        match v {
                            Value::Object(_) | Value::Array(_) => {
                                out.push_str(&format!("{}- {}:\n", pad, k));
                                dump_kv(k, v, indent + 2, out);
                            }
                            _ => {
                                out.push_str(&format!("{}- {}: {}\n", pad, k, scalar_repr(v)));
                            }
                        }
                    } else {
                        dump_kv(k, v, indent + 2, out);
                    }
                }
            }
            other => {
                out.push_str(&format!("{}- {}\n", pad, scalar_repr(other)));
            }
        }
    }

    fn scalar_repr(v: &Value) -> String {
        match v {
            Value::String(s) => {
                if s.is_empty()
                    || s.contains(':')
                    || s.contains('#')
                    || s.contains('\n')
                    || s.starts_with(['[', '{', '\'', '"', '-', '&', '*', '!', '|', '>', '@', '`'])
                    || matches!(s.as_str(), "true" | "false" | "null" | "yes" | "no")
                {
                    format!("'{}'", s.replace('\'', "''"))
                } else {
                    s.clone()
                }
            }
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::Null => "null".to_string(),
            _ => "''".to_string(),
        }
    }
}

// ============================================================================
// Frontmatter parsing (AgentService._parse_frontmatter)
// ============================================================================

/// Parse `^---\s*\n(.*?)\n---\s*\n(.*)$` (DOTALL). Returns (metadata, body).
fn parse_frontmatter(content: &str) -> (Value, String) {
    if let Some((yaml, body)) = split_frontmatter(content) {
        (yaml_min::parse(&yaml), body.trim().to_string())
    } else {
        (Value::Object(Map::new()), content.trim().to_string())
    }
}

/// Returns Some((yaml_text, body)) only when the document opens with a `---`
/// fence and has a closing `---` fence, matching the Python regex semantics.
fn split_frontmatter(content: &str) -> Option<(String, String)> {
    // Opening fence: `---` optionally trailed by whitespace, then newline.
    let mut rest = content;
    // Match `^---\s*\n`
    let after_open = {
        let stripped = rest.strip_prefix("---")?;
        let nl = stripped.find('\n')?;
        let between = &stripped[..nl];
        if !between.trim().is_empty() {
            return None;
        }
        &stripped[nl + 1..]
    };
    rest = after_open;

    // Find closing `\n---\s*\n` (the `.*?` is non-greedy → first closing fence).
    let bytes = rest;
    let mut search_from = 0;
    loop {
        let idx = bytes[search_from..].find("\n---")?;
        let abs = search_from + idx;
        let yaml_text = &bytes[..abs];
        let after_dashes = &bytes[abs + 4..];
        // after `\n---` must be `\s*\n` then the rest is the body.
        if let Some(nl) = after_dashes.find('\n') {
            let between = &after_dashes[..nl];
            if between.trim().is_empty() {
                let body = &after_dashes[nl + 1..];
                return Some((yaml_text.to_string(), body.to_string()));
            }
        } else {
            // closing fence at EOF: `\n---` then optional spaces, no newline.
            if after_dashes.trim().is_empty() {
                return Some((yaml_text.to_string(), String::new()));
            }
        }
        search_from = abs + 1;
    }
}

fn build_frontmatter(metadata: &Map<String, Value>) -> String {
    if metadata.is_empty() {
        return String::new();
    }
    let yaml_content = yaml_min::dump(&Value::Object(metadata.clone()));
    format!("---\n{}---\n\n", yaml_content)
}

// ============================================================================
// Field helpers
// ============================================================================

fn mget<'a>(m: &'a Value, key: &str) -> Option<&'a Value> {
    m.as_object()
        .and_then(|o| o.get(key))
        .filter(|v| !v.is_null())
}

/// Python `metadata.get(a) or metadata.get(b)` — falsy (null/"") falls through.
fn truthy<'a>(v: Option<&'a Value>) -> Option<&'a Value> {
    match v {
        Some(Value::Null) | None => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(Value::Bool(false)) => None,
        Some(other) => Some(other),
    }
}

fn or2<'a>(m: &'a Value, a: &str, b: &str) -> Option<&'a Value> {
    truthy(m.as_object().and_then(|o| o.get(a)))
        .or_else(|| truthy(m.as_object().and_then(|o| o.get(b))))
}

/// AgentService._parse_list_field: list stays, comma-string splits, else None.
fn parse_list_field(v: Option<&Value>) -> Option<Vec<String>> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
        ),
        Some(Value::Array(a)) => Some(
            a.iter()
                .map(|x| match x {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect(),
        ),
        Some(other) => Some(vec![other.to_string()]),
    }
}

/// AgentService._parse_hooks → dict[str, list[AgentHook]] | None.
fn parse_hooks(raw: Option<&Value>) -> Option<Value> {
    let raw = raw?.as_object()?;
    if raw.is_empty() {
        return None;
    }
    let mut result = Map::new();
    let mk = |h: &Value| -> Value {
        let o = h.as_object();
        json!({
            "type": o.and_then(|m| m.get("type")).and_then(|v| v.as_str()).unwrap_or("command"),
            "command": o.and_then(|m| m.get("command")).cloned().unwrap_or(Value::Null),
            "prompt": o.and_then(|m| m.get("prompt")).cloned().unwrap_or(Value::Null),
        })
    };
    for (event, hook_list) in raw {
        match hook_list {
            Value::Array(a) => {
                let hooks: Vec<Value> = a.iter().filter(|h| h.is_object()).map(mk).collect();
                result.insert(event.clone(), Value::Array(hooks));
            }
            Value::Object(_) => {
                result.insert(event.clone(), Value::Array(vec![mk(hook_list)]));
            }
            _ => {}
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(Value::Object(result))
    }
}

/// AgentService._hooks_to_dict — strip null/empty fields per hook.
fn hooks_to_dict(hooks: &Value) -> Option<Value> {
    let m = hooks.as_object()?;
    if m.is_empty() {
        return None;
    }
    let mut result = Map::new();
    for (event, list) in m {
        let arr = list.as_array().cloned().unwrap_or_default();
        let mut out = Vec::new();
        for h in &arr {
            let mut o = Map::new();
            for k in ["type", "command", "prompt"] {
                if let Some(v) = h.get(k) {
                    let keep = match v {
                        Value::Null => false,
                        Value::String(s) => !s.is_empty(),
                        _ => true,
                    };
                    if keep {
                        o.insert(k.to_string(), v.clone());
                    }
                }
            }
            out.push(Value::Object(o));
        }
        result.insert(event.clone(), Value::Array(out));
    }
    Some(Value::Object(result))
}

/// Python: `Path.home() / ".claude" / "agent-memory" / agent_name`.
fn agent_memory_dir(agent_name: &str) -> PathBuf {
    paths::get_user_home()
        .join(".claude")
        .join("agent-memory")
        .join(agent_name)
}

// ============================================================================
// Installed plugins
// ============================================================================

fn get_installed_plugins() -> Vec<Value> {
    let plugins_file = paths::get_installed_plugins_file();
    if !plugins_file.exists() {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(&plugins_file) else {
        return Vec::new();
    };
    let Ok(data) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut plugins = Vec::new();
    if let Some(map) = data.get("plugins").and_then(|p| p.as_object()) {
        for (plugin_name, installs) in map {
            if let Some(arr) = installs.as_array() {
                for install in arr {
                    if let Some(install_path) = install.get("installPath").and_then(|v| v.as_str())
                    {
                        if Path::new(install_path).exists() {
                            plugins.push(json!({
                                "name": plugin_name,
                                "path": install_path,
                                "scope": install.get("scope").and_then(|v| v.as_str()).unwrap_or("user"),
                            }));
                        }
                    }
                }
            }
        }
    }
    plugins
}

// ============================================================================
// Agent model builder
// ============================================================================

fn build_agent(name: &str, scope: &str, metadata: &Value, prompt: &str) -> Value {
    let tools = parse_list_field(mget(metadata, "tools"));
    let disallowed_tools = parse_list_field(or2(metadata, "disallowed-tools", "disallowed_tools"));
    let skills = parse_list_field(mget(metadata, "skills"));
    let hooks = parse_hooks(mget(metadata, "hooks"));
    let permission_mode = or2(metadata, "permission-mode", "permission_mode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    json!({
        "name": name,
        "scope": scope,
        "description": mget(metadata, "description").cloned().unwrap_or(Value::Null),
        "tools": tools,
        "model": mget(metadata, "model").cloned().unwrap_or(Value::Null),
        "prompt": prompt,
        "disallowed_tools": disallowed_tools,
        "permission_mode": permission_mode,
        "skills": skills,
        "hooks": hooks,
        "memory": mget(metadata, "memory").cloned().unwrap_or(Value::Null),
    })
}

fn list_md_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("md") {
                out.push(p);
            }
        }
    }
    out
}

fn scan_agents_dir(base_dir: &Path, scope: &str) -> Vec<Value> {
    let mut agents = Vec::new();
    for md_file in list_md_files(base_dir) {
        let Ok(content) = std::fs::read_to_string(&md_file) else {
            continue;
        };
        let (metadata, markdown_content) = parse_frontmatter(&content);
        let agent_name = md_file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        agents.push(build_agent(
            &agent_name,
            scope,
            &metadata,
            &markdown_content,
        ));
    }
    agents
}

fn service_list_agents(project_path: Option<&str>, cwd_fallback: &Path) -> Vec<Value> {
    let mut agents = Vec::new();

    let user_agents_dir = paths::get_claude_user_agents_dir();
    if user_agents_dir.exists() {
        agents.extend(scan_agents_dir(&user_agents_dir, "user"));
    }

    if let Some(pp) = project_path {
        let project_agents_dir = paths::get_project_agents_dir(Some(pp), cwd_fallback);
        if project_agents_dir.exists() {
            agents.extend(scan_agents_dir(&project_agents_dir, "project"));
        }
    }

    for plugin in get_installed_plugins() {
        let plugin_path = PathBuf::from(plugin["path"].as_str().unwrap_or(""));
        let agents_dir = plugin_path.join("agents");
        if agents_dir.exists() {
            let scope = format!("plugin:{}", plugin["name"].as_str().unwrap_or(""));
            agents.extend(scan_agents_dir(&agents_dir, &scope));
        }
    }

    agents
}

fn agent_base_dir(scope: &str, project_path: Option<&str>, cwd_fallback: &Path) -> PathBuf {
    if scope == "user" {
        paths::get_claude_user_agents_dir()
    } else {
        paths::get_project_agents_dir(project_path, cwd_fallback)
    }
}

fn service_get_agent(
    scope: &str,
    name: &str,
    project_path: Option<&str>,
    cwd_fallback: &Path,
) -> Option<Value> {
    let base_dir = agent_base_dir(scope, project_path, cwd_fallback);
    let file_path = base_dir.join(format!("{}.md", name));
    if !file_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&file_path).ok()?;
    let (metadata, markdown_content) = parse_frontmatter(&content);
    Some(build_agent(name, scope, &metadata, &markdown_content))
}

fn ensure_agent_memory_dir(agent_name: &str, memory_scope: Option<&str>) {
    if let Some(scope) = memory_scope {
        if !scope.is_empty() && scope != "none" {
            paths::ensure_directory_exists(&agent_memory_dir(agent_name));
        }
    }
}

// ============================================================================
// Skill model builder
// ============================================================================

fn metadata_to_frontmatter(metadata: &Value) -> Value {
    let meta_sub = mget(metadata, "metadata")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));

    // allowed-tools: top-level (hyphen/underscore) or metadata sub-dict.
    let allowed_raw = truthy(metadata.as_object().and_then(|o| o.get("allowed-tools")))
        .or_else(|| truthy(metadata.as_object().and_then(|o| o.get("allowed_tools"))))
        .or_else(|| truthy(meta_sub.as_object().and_then(|o| o.get("allowed-tools"))));
    let allowed_tools: Option<Value> = match allowed_raw {
        Some(Value::String(s)) => Some(Value::Array(
            s.split(',')
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(|t| Value::String(t.to_string()))
                .collect(),
        )),
        Some(Value::Array(a)) => Some(Value::Array(a.clone())),
        _ => None,
    };

    let user_invocable = metadata
        .get("user-invocable")
        .or_else(|| metadata.get("user_invocable"))
        .cloned();
    let disable_model = metadata
        .get("disable-model-invocation")
        .or_else(|| metadata.get("disable_model_invocation"))
        .cloned();
    let argument_hint = truthy(metadata.as_object().and_then(|o| o.get("argument-hint")))
        .or_else(|| truthy(metadata.as_object().and_then(|o| o.get("argument_hint"))))
        .or_else(|| truthy(meta_sub.as_object().and_then(|o| o.get("argument-hint"))))
        .or_else(|| truthy(meta_sub.as_object().and_then(|o| o.get("argument_hint"))))
        .cloned();
    let version = truthy(metadata.as_object().and_then(|o| o.get("version")))
        .or_else(|| truthy(meta_sub.as_object().and_then(|o| o.get("version"))));
    let version_str = version.map(|v| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    });

    json!({
        "name": mget(metadata, "name").cloned().unwrap_or(Value::Null),
        "description": mget(metadata, "description").cloned().unwrap_or(Value::Null),
        "version": version_str,
        "license": mget(metadata, "license").cloned().unwrap_or(Value::Null),
        "context": mget(metadata, "context").cloned().unwrap_or(Value::Null),
        "agent": mget(metadata, "agent").cloned().unwrap_or(Value::Null),
        "model": mget(metadata, "model").cloned().unwrap_or(Value::Null),
        "allowed_tools": allowed_tools,
        "user_invocable": user_invocable.unwrap_or(Value::Null),
        "disable_model_invocation": disable_model.unwrap_or(Value::Null),
        "argument_hint": argument_hint.unwrap_or(Value::Null),
        "hooks": mget(metadata, "hooks").cloned().unwrap_or(Value::Null),
        "metadata": if meta_sub.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
            meta_sub
        } else {
            Value::Null
        },
    })
}

fn make_skill(name: &str, metadata: &Value, location: &str, content: Option<&str>) -> Value {
    json!({
        "name": name,
        "description": mget(metadata, "description").cloned().unwrap_or(Value::Null),
        "location": location,
        "content": content.map(|c| Value::String(c.to_string())).unwrap_or(Value::Null),
        "frontmatter": metadata_to_frontmatter(metadata),
        "dependency_status": Value::Null,
        "supporting_files": Value::Null,
    })
}

fn list_subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.push(p);
            }
        }
    }
    out
}

fn scan_skills_dir(base_dir: &Path, location: &str) -> Vec<Value> {
    let mut skills = Vec::new();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Layout 1: flat .md files
    for skill_file in list_md_files(base_dir) {
        let Ok(content) = std::fs::read_to_string(&skill_file) else {
            continue;
        };
        let (metadata, _) = parse_frontmatter(&content);
        let skill_name = skill_file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        seen_names.insert(skill_name.clone());
        skills.push(make_skill(&skill_name, &metadata, location, None));
    }

    // Layout 2: subdirectories with SKILL.md
    for item in list_subdirs(base_dir) {
        let skill_file = item.join("SKILL.md");
        if !skill_file.exists() {
            // Nested skills (e.g. nextjs/vercel-ai-sdk/SKILL.md)
            for sub in list_subdirs(&item) {
                let sub_skill = sub.join("SKILL.md");
                let sub_name = sub
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if sub_skill.exists() && !seen_names.contains(&sub_name) {
                    if let Ok(content) = std::fs::read_to_string(&sub_skill) {
                        let (metadata, _) = parse_frontmatter(&content);
                        seen_names.insert(sub_name.clone());
                        skills.push(make_skill(&sub_name, &metadata, location, None));
                    }
                }
            }
            continue;
        }
        let item_name = item
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if seen_names.contains(&item_name) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&skill_file) {
            let (metadata, _) = parse_frontmatter(&content);
            seen_names.insert(item_name.clone());
            skills.push(make_skill(&item_name, &metadata, location, None));
        }
    }

    skills
}

fn scan_plugin_skills_dir(base_dir: &Path, location: &str) -> Vec<Value> {
    let mut skills = Vec::new();
    for skill_subdir in list_subdirs(base_dir) {
        let skill_file = skill_subdir.join("SKILL.md");
        if !skill_file.exists() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&skill_file) else {
            continue;
        };
        let (metadata, _) = parse_frontmatter(&content);
        let skill_name = skill_subdir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        skills.push(make_skill(&skill_name, &metadata, location, None));
    }
    skills
}

fn service_list_skills(project_path: Option<&str>) -> Vec<Value> {
    let mut skills = Vec::new();

    let user_skills_dir = paths::get_claude_user_skills_dir();
    if user_skills_dir.exists() {
        skills.extend(scan_skills_dir(&user_skills_dir, "user"));
    }

    if let Some(pp) = project_path {
        let project_skills_dir = PathBuf::from(pp).join(".claude").join("skills");
        if project_skills_dir.exists() {
            skills.extend(scan_skills_dir(&project_skills_dir, "project"));
        }
    }

    for plugin in get_installed_plugins() {
        let plugin_path = PathBuf::from(plugin["path"].as_str().unwrap_or(""));
        let skills_dir = plugin_path.join("skills");
        if skills_dir.exists() {
            let commands_dir = plugin_path.join("commands");
            let mut command_names: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            if commands_dir.exists() {
                for cmd_file in list_md_files(&commands_dir) {
                    if let Some(stem) = cmd_file.file_stem() {
                        command_names.insert(stem.to_string_lossy().into_owned());
                    }
                }
            }
            let location = format!("plugin:{}", plugin["name"].as_str().unwrap_or(""));
            let mut plugin_skills = scan_plugin_skills_dir(&skills_dir, &location);
            if !command_names.is_empty() {
                plugin_skills.retain(|s| !command_names.contains(s["name"].as_str().unwrap_or("")));
            }
            skills.extend(plugin_skills);
        }
    }

    skills
}

/// AgentService._find_skill_file
fn find_skill_file(base_dir: &Path, name: &str) -> Option<PathBuf> {
    let flat = base_dir.join(format!("{}.md", name));
    if flat.exists() {
        return Some(flat);
    }
    let subdir = base_dir.join(name).join("SKILL.md");
    if subdir.exists() {
        return Some(subdir);
    }
    for parent in list_subdirs(base_dir) {
        let nested = parent.join(name).join("SKILL.md");
        if nested.exists() {
            return Some(nested);
        }
    }
    None
}

fn service_get_skill(name: &str, location: &str, project_path: Option<&str>) -> Option<Value> {
    let skill_file: Option<PathBuf> = if location == "user" {
        find_skill_file(&paths::get_claude_user_skills_dir(), name)
    } else if location == "project" && project_path.is_some() {
        let base = PathBuf::from(project_path.unwrap())
            .join(".claude")
            .join("skills");
        find_skill_file(&base, name)
    } else if let Some(plugin_name) = location.strip_prefix("plugin:") {
        let mut found = None;
        for plugin in get_installed_plugins() {
            if plugin["name"].as_str() == Some(plugin_name) {
                let plugin_path = PathBuf::from(plugin["path"].as_str().unwrap_or(""));
                found = Some(plugin_path.join("skills").join(name).join("SKILL.md"));
                break;
            }
        }
        found
    } else {
        None
    };

    let skill_file = skill_file?;
    if !skill_file.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&skill_file).ok()?;
    let (metadata, markdown_content) = parse_frontmatter(&content);
    Some(make_skill(
        name,
        &metadata,
        location,
        Some(&markdown_content),
    ))
}

// ============================================================================
// Skill dependency service
// ============================================================================

/// SkillDependencyService._parse_frontmatter (returns just metadata dict).
fn dep_parse_frontmatter(content: &str) -> Value {
    // Pattern `^---\s*\n(.*?)\n---\s*\n` (note: no body capture).
    if let Some((yaml, _)) = split_frontmatter(content) {
        yaml_min::parse(&yaml)
    } else {
        Value::Object(Map::new())
    }
}

fn resolve_skill_dir(base: &Path, name: &str) -> Option<PathBuf> {
    let skill_dir = base.join(name);
    if skill_dir.is_dir() && skill_dir.join("SKILL.md").exists() {
        return Some(skill_dir);
    }
    if base.exists() {
        for parent in list_subdirs(base) {
            let nested = parent.join(name);
            if nested.is_dir() && nested.join("SKILL.md").exists() {
                return Some(nested);
            }
        }
    }
    None
}

fn dep_get_skill_dir(name: &str, location: &str, project_path: Option<&str>) -> Option<PathBuf> {
    if location == "user" {
        let base = paths::get_claude_user_skills_dir();
        if let Some(r) = resolve_skill_dir(&base, name) {
            return Some(r);
        }
        None
    } else if location == "project" && project_path.is_some() {
        let base = PathBuf::from(project_path.unwrap())
            .join(".claude")
            .join("skills");
        resolve_skill_dir(&base, name)
    } else if let Some(plugin_name) = location.strip_prefix("plugin:") {
        let plugins_file = paths::get_installed_plugins_file();
        if !plugins_file.exists() {
            return None;
        }
        let text = std::fs::read_to_string(&plugins_file).ok()?;
        let data: Value = serde_json::from_str(&text).ok()?;
        if let Some(plugins) = data.get("plugins").and_then(|p| p.as_object()) {
            for (key, installs) in plugins {
                if key.starts_with(&format!("{}@", plugin_name)) || key == plugin_name {
                    if let Some(arr) = installs.as_array() {
                        for install in arr {
                            if let Some(install_path) =
                                install.get("installPath").and_then(|v| v.as_str())
                            {
                                let skill_dir =
                                    PathBuf::from(install_path).join("skills").join(name);
                                if skill_dir.is_dir() {
                                    return Some(skill_dir);
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    } else {
        None
    }
}

fn dep_get_skill_file(name: &str, location: &str, project_path: Option<&str>) -> Option<PathBuf> {
    if let Some(skill_dir) = dep_get_skill_dir(name, location, project_path) {
        let skill_file = skill_dir.join("SKILL.md");
        if skill_file.exists() {
            return Some(skill_file);
        }
    }
    if location == "user" {
        let flat_file = paths::get_claude_user_skills_dir().join(format!("{}.md", name));
        if flat_file.exists() {
            return Some(flat_file);
        }
    }
    None
}

fn which(name: &str, enable_external_tools: bool) -> Option<PathBuf> {
    if !enable_external_tools {
        return None;
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if let Ok(meta) = std::fs::metadata(&candidate) {
            if meta.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if meta.permissions().mode() & 0o111 != 0 {
                        return Some(candidate);
                    }
                }
                #[cfg(not(unix))]
                {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn run_cmd(
    cmd: &str,
    args: &[&str],
    timeout_secs: u64,
    cwd: Option<&Path>,
) -> Option<std::process::Output> {
    use std::process::{Command, Stdio};
    let mut c = Command::new(cmd);
    c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    let mut child = c.spawn().ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed().as_secs() >= timeout_secs {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}

fn check_binary(name: &str, enable_external_tools: bool) -> (bool, Option<String>) {
    if which(name, enable_external_tools).is_none() {
        return (false, None);
    }
    let mut version = None;
    for flag in ["--version", "-v", "version"] {
        if let Some(out) = run_cmd(name, &[flag], 5, None) {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    version = Some(trimmed.lines().next().unwrap_or("").to_string());
                    break;
                }
            }
        }
    }
    (true, version)
}

fn check_npm_package(name: &str) -> (bool, Option<String>) {
    if let Some(out) = run_cmd("npm", &["list", "-g", name, "--json"], 15, None) {
        if out.status.success() {
            if let Ok(data) = serde_json::from_slice::<Value>(&out.stdout) {
                if let Some(deps) = data.get("dependencies").and_then(|d| d.as_object()) {
                    if let Some(entry) = deps.get(name) {
                        return (
                            true,
                            entry
                                .get("version")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                        );
                    }
                }
            }
        }
    }
    (false, None)
}

fn pip_show(cmd: &str, name: &str) -> Option<(bool, Option<String>)> {
    let out = run_cmd(cmd, &["show", name], 10, None)?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("Version:") {
                return Some((true, Some(rest.trim().to_string())));
            }
        }
        return Some((true, None));
    }
    Some((false, None))
}

fn check_pip_package(name: &str, enable_external_tools: bool) -> (bool, Option<String>) {
    if which("pip", enable_external_tools).is_some() {
        if let Some(r) = pip_show("pip", name) {
            return r;
        }
    }
    if which("pip3", enable_external_tools).is_some() {
        if let Some(r) = pip_show("pip3", name) {
            return r;
        }
    }
    (false, None)
}

fn dep_obj(kind: &str, name: &str, installed: bool, installed_version: Option<String>) -> Value {
    json!({
        "kind": kind,
        "name": name,
        "installed": installed,
        "version": Value::Null,
        "installed_version": installed_version,
    })
}

fn check_dependencies(
    name: &str,
    location: &str,
    project_path: Option<&str>,
    enable_external_tools: bool,
) -> Value {
    let mut dependencies: Vec<Value> = Vec::new();
    let mut has_install_script = false;
    let mut install_script_path: Value = Value::Null;

    if let Some(skill_file) = dep_get_skill_file(name, location, project_path) {
        if let Ok(content) = std::fs::read_to_string(&skill_file) {
            let metadata = dep_parse_frontmatter(&content);

            let mut openclaw_meta = json!({});
            if let Some(meta) = metadata.get("metadata") {
                if meta.is_object() {
                    openclaw_meta = meta.get("openclaw").cloned().unwrap_or_else(|| json!({}));
                }
            }

            let mut requires = openclaw_meta
                .get("requires")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let install_defs = openclaw_meta
                .get("install")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            if requires.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                if let Some(r) = metadata.get("requires") {
                    requires = r.clone();
                }
            }

            let bins = requires
                .get("bins")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for b in &bins {
                if let Some(bin_name) = b.as_str() {
                    let (installed, version) = check_binary(bin_name, enable_external_tools);
                    dependencies.push(dep_obj("bin", bin_name, installed, version));
                }
            }

            let npm_deps = requires
                .get("npm")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for p in &npm_deps {
                if let Some(pkg) = p.as_str() {
                    let (installed, version) = check_npm_package(pkg);
                    dependencies.push(dep_obj("npm", pkg, installed, version));
                }
            }

            let pip_deps = requires
                .get("pip")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for p in &pip_deps {
                if let Some(pkg) = p.as_str() {
                    let (installed, version) = check_pip_package(pkg, enable_external_tools);
                    dependencies.push(dep_obj("pip", pkg, installed, version));
                }
            }

            for install_def in &install_defs {
                if !install_def.is_object() {
                    continue;
                }
                let kind = install_def
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let pkg = install_def
                    .get("package")
                    .and_then(|v| v.as_str())
                    .or_else(|| install_def.get("formula").and_then(|v| v.as_str()))
                    .unwrap_or("");
                if pkg.is_empty() {
                    continue;
                }
                if dependencies.iter().any(|d| d["name"].as_str() == Some(pkg)) {
                    continue;
                }
                if kind == "npm" {
                    let (installed, version) = check_npm_package(pkg);
                    dependencies.push(dep_obj("npm", pkg, installed, version));
                } else if kind == "pip" {
                    let (installed, version) = check_pip_package(pkg, enable_external_tools);
                    dependencies.push(dep_obj("pip", pkg, installed, version));
                } else if kind == "brew" || kind == "apt" {
                    let check_bins = install_def
                        .get("bins")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    for b in &check_bins {
                        if let Some(bin_name) = b.as_str() {
                            if !dependencies
                                .iter()
                                .any(|d| d["name"].as_str() == Some(bin_name))
                            {
                                let (installed, version) =
                                    check_binary(bin_name, enable_external_tools);
                                dependencies.push(dep_obj("bin", bin_name, installed, version));
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(skill_dir) = dep_get_skill_dir(name, location, project_path) {
        for script_name in ["scripts/install.sh", "install.sh", "setup.sh"] {
            let script_path = skill_dir.join(script_name);
            if script_path.exists() {
                has_install_script = true;
                install_script_path = Value::String(script_path.to_string_lossy().into_owned());
                dependencies.push(json!({
                    "kind": "script",
                    "name": script_name,
                    "installed": true,
                    "version": Value::Null,
                    "installed_version": Value::Null,
                }));
                break;
            }
        }
    }

    let all_satisfied = dependencies
        .iter()
        .all(|d| d["installed"].as_bool().unwrap_or(false));

    json!({
        "skill_name": name,
        "all_satisfied": all_satisfied,
        "dependencies": dependencies,
        "has_install_script": has_install_script,
        "install_script_path": install_script_path,
    })
}

fn rglob_all(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p.clone());
            }
            out.push(p);
        }
    }
    out.sort();
    out
}

fn list_supporting_files(name: &str, location: &str, project_path: Option<&str>) -> Vec<Value> {
    let Some(skill_dir) = dep_get_skill_dir(name, location, project_path) else {
        return Vec::new();
    };
    if !skill_dir.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    for path in rglob_all(&skill_dir) {
        if !path.is_file() {
            continue;
        }
        let fname = path.file_name().map(|s| s.to_string_lossy().into_owned());
        if fname.as_deref() == Some("SKILL.md") && path.parent() == Some(skill_dir.as_path()) {
            continue;
        }
        let relative = path
            .strip_prefix(&skill_dir)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string_lossy().into_owned());
        let ext = path.extension().and_then(|e| e.to_str());
        let mut is_script = matches!(ext, Some("sh") | Some("py") | Some("js") | Some("ts"));
        #[cfg(unix)]
        if !is_script {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.permissions().mode() & 0o111 != 0 {
                    is_script = true;
                }
            }
        }
        let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        files.push(json!({
            "name": relative,
            "path": path.to_string_lossy(),
            "size_bytes": size_bytes,
            "is_script": is_script,
        }));
    }
    files
}

fn install_dependencies(
    name: &str,
    location: &str,
    project_path: Option<&str>,
    enable_external_tools: bool,
) -> Value {
    let status = check_dependencies(name, location, project_path, enable_external_tools);

    if status["all_satisfied"].as_bool().unwrap_or(false) {
        return json!({
            "success": true,
            "message": "All dependencies are already satisfied.",
            "installed": [],
            "failed": [],
            "logs": "",
        });
    }

    let mut installed: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut all_logs: Vec<String> = Vec::new();
    let skill_dir = dep_get_skill_dir(name, location, project_path);
    let deps = status["dependencies"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    if status["has_install_script"].as_bool().unwrap_or(false) {
        if let Some(sp) = status["install_script_path"].as_str() {
            let script_path = PathBuf::from(sp);
            let script_name = script_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
            }
            match run_cmd(
                "bash",
                &[&script_path.to_string_lossy()],
                120,
                skill_dir.as_deref(),
            ) {
                Some(out) => {
                    all_logs.push(format!("=== Install script: {} ===", script_name));
                    let so = String::from_utf8_lossy(&out.stdout);
                    if !so.is_empty() {
                        all_logs.push(so.to_string());
                    }
                    let se = String::from_utf8_lossy(&out.stderr);
                    if !se.is_empty() {
                        all_logs.push(se.to_string());
                    }
                    if out.status.success() {
                        installed.push(format!("script:{}", script_name));
                    } else {
                        failed.push(format!(
                            "script:{} (exit code {})",
                            script_name,
                            out.status.code().unwrap_or(-1)
                        ));
                    }
                }
                None => {
                    failed.push(format!("script:{} (timeout)", script_name));
                    all_logs.push("Install script timed out after 120s".to_string());
                }
            }
        }
    }

    for dep in &deps {
        if dep["installed"].as_bool().unwrap_or(false) || dep["kind"].as_str() != Some("npm") {
            continue;
        }
        let dname = dep["name"].as_str().unwrap_or("");
        all_logs.push(format!("\n=== npm install -g {} ===", dname));
        match run_cmd("npm", &["install", "-g", dname], 60, None) {
            Some(out) => {
                let so = String::from_utf8_lossy(&out.stdout);
                if !so.is_empty() {
                    all_logs.push(so.to_string());
                }
                let se = String::from_utf8_lossy(&out.stderr);
                if !se.is_empty() {
                    all_logs.push(se.to_string());
                }
                if out.status.success() {
                    installed.push(format!("npm:{}", dname));
                } else {
                    failed.push(format!("npm:{}", dname));
                }
            }
            None => failed.push(format!("npm:{} (timeout)", dname)),
        }
    }

    for dep in &deps {
        if dep["installed"].as_bool().unwrap_or(false) || dep["kind"].as_str() != Some("pip") {
            continue;
        }
        let dname = dep["name"].as_str().unwrap_or("");
        let pip_cmd = if which("pip3", enable_external_tools).is_some() {
            "pip3"
        } else {
            "pip"
        };
        all_logs.push(format!("\n=== {} install {} ===", pip_cmd, dname));
        match run_cmd(pip_cmd, &["install", dname], 60, None) {
            Some(out) => {
                let so = String::from_utf8_lossy(&out.stdout);
                if !so.is_empty() {
                    all_logs.push(so.to_string());
                }
                let se = String::from_utf8_lossy(&out.stderr);
                if !se.is_empty() {
                    all_logs.push(se.to_string());
                }
                if out.status.success() {
                    installed.push(format!("pip:{}", dname));
                } else {
                    failed.push(format!("pip:{}", dname));
                }
            }
            None => failed.push(format!("pip:{} (timeout)", dname)),
        }
    }

    for dep in &deps {
        if dep["installed"].as_bool().unwrap_or(false) || dep["kind"].as_str() != Some("bin") {
            continue;
        }
        let dname = dep["name"].as_str().unwrap_or("");
        failed.push(format!("bin:{} (manual install required)", dname));
        all_logs.push(format!(
            "\n⚠ Binary '{}' not found. Install it manually.",
            dname
        ));
    }

    let success = failed.is_empty();
    let message = if success {
        format!("Successfully installed {} dependencies.", installed.len())
    } else if !installed.is_empty() {
        format!(
            "Installed {} dependencies, {} failed.",
            installed.len(),
            failed.len()
        )
    } else {
        format!("Failed to install {} dependencies.", failed.len())
    };

    json!({
        "success": success,
        "message": message,
        "installed": installed,
        "failed": failed,
        "logs": all_logs.join("\n"),
    })
}

// ============================================================================
// Skills registry service (skills.sh)
// ============================================================================

const CACHE_TTL_SECONDS: f64 = 24.0 * 60.0 * 60.0;
const SEARCH_CACHE_TTL_SECONDS: f64 = 60.0 * 60.0;
const SKILLS_SH_BASE: &str = "https://skills.sh";
const SKILLS_SH_SEARCH_API: &str = "https://skills.sh/api/search";

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

type HomeCache = Mutex<Option<(Vec<Value>, f64)>>;
type SearchCache = Mutex<std::collections::HashMap<String, (Vec<Value>, f64)>>;

fn home_cache() -> &'static HomeCache {
    static C: OnceLock<HomeCache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

fn search_cache() -> &'static SearchCache {
    static C: OnceLock<SearchCache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn registry_skill_dict(
    skill_id: &str,
    name: &str,
    source: &str,
    installs: i64,
    registry_id: &str,
) -> Value {
    let registry_id = if registry_id.is_empty() {
        format!("{}/{}", source, skill_id)
    } else {
        registry_id.to_string()
    };
    json!({
        "skill_id": skill_id,
        "name": name,
        "source": source,
        "installs": installs,
        "registry_id": registry_id,
        "url": format!("{}/s/{}/{}", SKILLS_SH_BASE, source, skill_id),
        "github_url": format!("https://github.com/{}", source),
    })
}

fn parse_homepage_skills(html: &str) -> Vec<Value> {
    use regex::Regex;
    let mut skills = Vec::new();
    let chunk_re = match Regex::new(r#"self\.__next_f\.push\(\[1,"(.*?)"\]\)"#) {
        Ok(r) => r,
        Err(_) => return skills,
    };
    let skill_re = match Regex::new(
        r#"\{"source":"([^"]+)","skillId":"([^"]+)","name":"([^"]+)","installs":(\d+)\}"#,
    ) {
        Ok(r) => r,
        Err(_) => return skills,
    };
    // Python uses re.DOTALL for the chunk regex; emulate by allowing `.` to
    // span newlines via the `(?s)` inline flag.
    let chunk_re = Regex::new(&format!("(?s){}", chunk_re.as_str())).unwrap_or(chunk_re);

    for cap in chunk_re.captures_iter(html) {
        let chunk = &cap[1];
        let clean = chunk.replace("\\\"", "\"").replace("\\n", "\n");
        if !clean.contains("skillId") || !clean.contains("installs") {
            continue;
        }
        for sc in skill_re.captures_iter(&clean) {
            let source = sc[1].to_string();
            let skill_id = sc[2].to_string();
            let name = sc[3].to_string();
            let installs: i64 = sc[4].parse().unwrap_or(0);
            skills.push(registry_skill_dict(&skill_id, &name, &source, installs, ""));
        }
    }
    skills
}

async fn get_homepage_skills(force_refresh: bool) -> Vec<Value> {
    if !force_refresh {
        if let Ok(guard) = home_cache().lock() {
            if let Some((data, t)) = guard.as_ref() {
                if (now_secs() - t) < CACHE_TTL_SECONDS {
                    return data.clone();
                }
            }
        }
    }

    let fetched: Option<String> = async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .ok()?;
        let resp = client.get(SKILLS_SH_BASE).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.text().await.ok()
    }
    .await;

    match fetched {
        Some(html) => {
            let skills = parse_homepage_skills(&html);
            if !skills.is_empty() {
                if let Ok(mut guard) = home_cache().lock() {
                    *guard = Some((skills.clone(), now_secs()));
                }
            }
            skills
        }
        None => {
            if let Ok(guard) = home_cache().lock() {
                if let Some((data, _)) = guard.as_ref() {
                    return data.clone();
                }
            }
            Vec::new()
        }
    }
}

async fn search_skills(query: &str, limit: i64) -> Vec<Value> {
    if query.chars().count() < 2 {
        return Vec::new();
    }
    let cache_key = format!("{}:{}", query, limit);
    if let Ok(guard) = search_cache().lock() {
        if let Some((data, t)) = guard.get(&cache_key) {
            if (now_secs() - t) < SEARCH_CACHE_TTL_SECONDS {
                return data.clone();
            }
        }
    }

    let fetched: Option<Value> = async {
        let client: reqwest::Client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .ok()?;
        let enc = |s: &str| -> String {
            s.bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{:02X}", b),
                })
                .collect()
        };
        let url = format!("{}?q={}&limit={}", SKILLS_SH_SEARCH_API, enc(query), limit);
        let resp = client.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json::<Value>().await.ok()
    }
    .await;

    match fetched {
        Some(data) => {
            let mut skills = Vec::new();
            if let Some(arr) = data.get("skills").and_then(|v| v.as_array()) {
                for s in arr {
                    let skill_id = s.get("skillId").and_then(|v| v.as_str()).unwrap_or("");
                    let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let source = s.get("source").and_then(|v| v.as_str()).unwrap_or("");
                    let installs = s.get("installs").and_then(|v| v.as_i64()).unwrap_or(0);
                    let registry_id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    skills.push(registry_skill_dict(
                        skill_id,
                        name,
                        source,
                        installs,
                        registry_id,
                    ));
                }
            }
            if let Ok(mut guard) = search_cache().lock() {
                guard.insert(cache_key, (skills.clone(), now_secs()));
            }
            skills
        }
        None => {
            if let Ok(guard) = search_cache().lock() {
                if let Some((data, _)) = guard.get(&cache_key) {
                    return data.clone();
                }
            }
            Vec::new()
        }
    }
}

fn get_installed_skill_names(project_path: Option<&str>) -> std::collections::HashSet<String> {
    let skills = service_list_skills(project_path);
    skills
        .iter()
        .filter_map(|s| s["name"].as_str().map(String::from))
        .collect()
}

/// _clean_terminal_output port.
fn clean_terminal_output(text: &str) -> String {
    use regex::Regex;
    let mut t = text.to_string();
    let subs: &[(&str, &str)] = &[(r"\x1b\[[0-9;?]*[A-Za-z]", ""), (r"\x1b\][^\x07]*\x07", "")];
    for (pat, rep) in subs {
        if let Ok(re) = Regex::new(pat) {
            t = re.replace_all(&t, *rep).into_owned();
        }
    }
    t = t.replace('\u{1b}', "");
    for pat in [
        r"\[\?25[hl]",
        r"\[\d+D",
        r"\[J",
        r"\[\d+(?:;\d+)*m",
        r"\[0m",
    ] {
        if let Ok(re) = Regex::new(pat) {
            t = re.replace_all(&t, "").into_owned();
        }
    }
    if let Ok(re) = Regex::new(r"(?m)[█╗╔═║╚╝╟╠╣╬╩╦╨╤╥╙╘╒╓╫╪┘┐┌└├┤┬┴┼]+.*\n?")
    {
        t = re.replace_all(&t, "").into_owned();
    }

    let spinner_re = Regex::new(r"^[│├└┌]?\s*[◒◐◓◑]\s+").ok();
    let box_re = Regex::new(r"^[│├└┌]\s*").ok();
    let mut cleaned: Vec<String> = Vec::new();
    for line in t.split('\n') {
        let stripped = line.trim();
        if stripped.is_empty() {
            continue;
        }
        if let Some(re) = &spinner_re {
            if re.is_match(stripped) {
                continue;
            }
        }
        let mut s = stripped.to_string();
        if let Some(re) = &box_re {
            s = re.replace(&s, "").into_owned();
        }
        if s.trim().is_empty() {
            continue;
        }
        cleaned.push(s);
    }
    let mut result = cleaned.join("\n");
    if let Ok(re) = Regex::new(r"Tip: use the --yes.*\n?") {
        result = re.replace_all(&result, "").into_owned();
    }
    if let Ok(re) = Regex::new(r"\n{3,}") {
        result = re.replace_all(&result, "\n\n").into_owned();
    }
    result.trim().to_string()
}

fn install_skill_registry(
    source: &str,
    skill_names: Option<&Vec<String>>,
    global_install: bool,
    project_path: Option<&str>,
) -> Value {
    use std::process::{Command, Stdio};

    let mut args: Vec<String> = vec![
        "-y".into(),
        "skills".into(),
        "add".into(),
        source.into(),
        "--yes".into(),
    ];
    if global_install {
        args.push("--global".into());
    }
    if let Some(names) = skill_names {
        if !names.is_empty() {
            args.push("--skill".into());
            args.push(names.join(","));
        }
    }
    let cwd = if project_path.is_some() && !global_install {
        project_path
    } else {
        None
    };

    let mut cmd = Command::new("npx");
    cmd.args(&args)
        .env("CI", "1")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }

    let skill_names_val = match skill_names {
        Some(n) => Value::Array(n.iter().cloned().map(Value::String).collect()),
        None => Value::Null,
    };

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "success": false,
                "message": format!("Installation error: {}", e),
                "logs": "",
                "source": source,
                "skill_names": skill_names_val,
            });
        }
    };

    let start = std::time::Instant::now();
    let mut child = child;
    let output = loop {
        match child.try_wait() {
            Ok(Some(_)) => break child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed().as_secs() >= 120 {
                    let _ = child.kill();
                    let _ = child.wait();
                    return json!({
                        "success": false,
                        "message": "Installation timed out (120s)",
                        "logs": "",
                        "source": source,
                        "skill_names": skill_names_val,
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                return json!({
                    "success": false,
                    "message": format!("Installation error: {}", e),
                    "logs": "",
                    "source": source,
                    "skill_names": skill_names_val,
                });
            }
        }
    };

    match output {
        Some(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let raw_logs = if stderr.is_empty() {
                stdout.to_string()
            } else {
                format!("{}\n{}", stdout, stderr)
            };
            let logs = clean_terminal_output(&raw_logs);
            let success = out.status.success();
            json!({
                "success": success,
                "message": if success {
                    format!("Successfully installed skill(s) from {}", source)
                } else {
                    format!(
                        "Installation failed (exit code {})",
                        out.status.code().unwrap_or(-1)
                    )
                },
                "logs": logs.trim(),
                "source": source,
                "skill_names": skill_names_val,
            })
        }
        None => json!({
            "success": false,
            "message": "Installation error: process error",
            "logs": "",
            "source": source,
            "skill_names": skill_names_val,
        }),
    }
}

// ============================================================================
// HTTP layer
// ============================================================================

#[derive(Deserialize)]
struct ProjectPathQuery {
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct AgentCreate {
    name: String,
    scope: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default)]
    model: Option<String>,
    prompt: String,
    #[serde(default)]
    disallowed_tools: Option<Vec<String>>,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    skills: Option<Vec<String>>,
    #[serde(default)]
    hooks: Option<Value>,
    #[serde(default)]
    memory: Option<String>,
}

#[derive(Deserialize)]
struct AgentUpdate {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    disallowed_tools: Option<Vec<String>>,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    skills: Option<Vec<String>>,
    #[serde(default)]
    hooks: Option<Value>,
    #[serde(default)]
    memory: Option<String>,
}

#[derive(Deserialize)]
struct RegistryBrowseQuery {
    #[serde(default)]
    query: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    force_refresh: bool,
}

fn default_limit() -> i64 {
    50
}

#[derive(Deserialize)]
struct RegistryInstallRequest {
    source: String,
    #[serde(default)]
    skill_names: Option<Vec<String>>,
    #[serde(default = "default_true")]
    global_install: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct SkillDetailQuery {
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default = "default_true")]
    include_deps: bool,
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/agents
async fn list_agents(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let agents = service_list_agents(q.project_path.as_deref(), &state.cwd_fallback);
    Ok(Json(json!({ "agents": agents })))
}

/// GET /api/v1/agents/skills
async fn list_skills(Query(q): Query<ProjectPathQuery>) -> AppResult<Json<Value>> {
    let skills = service_list_skills(q.project_path.as_deref());
    Ok(Json(json!({ "skills": skills })))
}

/// GET /api/v1/agents/skills/{location}/{name}
async fn get_skill(
    State(state): State<ApiState>,
    axum::extract::Path((location, name)): axum::extract::Path<(String, String)>,
    Query(q): Query<SkillDetailQuery>,
) -> AppResult<Json<Value>> {
    let mut skill =
        service_get_skill(&name, &location, q.project_path.as_deref()).ok_or_else(|| {
            AppError::not_found(format!(
                "Skill '{}' not found in location '{}'",
                name, location
            ))
        })?;

    if q.include_deps {
        skill["dependency_status"] = check_dependencies(
            &name,
            &location,
            q.project_path.as_deref(),
            state.enable_external_tools,
        );
        skill["supporting_files"] = Value::Array(list_supporting_files(
            &name,
            &location,
            q.project_path.as_deref(),
        ));
    }

    Ok(Json(skill))
}

/// GET /api/v1/agents/skills/{location}/{name}/dependencies
async fn check_skill_dependencies(
    State(state): State<ApiState>,
    axum::extract::Path((location, name)): axum::extract::Path<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    if service_get_skill(&name, &location, q.project_path.as_deref()).is_none() {
        return Err(AppError::not_found(format!(
            "Skill '{}' not found in location '{}'",
            name, location
        )));
    }
    Ok(Json(check_dependencies(
        &name,
        &location,
        q.project_path.as_deref(),
        state.enable_external_tools,
    )))
}

/// POST /api/v1/agents/skills/{location}/{name}/install
async fn install_skill_dependencies(
    State(state): State<ApiState>,
    axum::extract::Path((location, name)): axum::extract::Path<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    if service_get_skill(&name, &location, q.project_path.as_deref()).is_none() {
        return Err(AppError::not_found(format!(
            "Skill '{}' not found in location '{}'",
            name, location
        )));
    }
    let pp = q.project_path.clone();
    let loc = location.clone();
    let nm = name.clone();
    let ext = state.enable_external_tools;
    let result =
        tokio::task::spawn_blocking(move || install_dependencies(&nm, &loc, pp.as_deref(), ext))
            .await
            .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(Json(result))
}

/// GET /api/v1/agents/skills/{location}/{name}/files
async fn list_skill_files(
    axum::extract::Path((location, name)): axum::extract::Path<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    if service_get_skill(&name, &location, q.project_path.as_deref()).is_none() {
        return Err(AppError::not_found(format!(
            "Skill '{}' not found in location '{}'",
            name, location
        )));
    }
    Ok(Json(Value::Array(list_supporting_files(
        &name,
        &location,
        q.project_path.as_deref(),
    ))))
}

/// GET /api/v1/agents/skills/registry
async fn browse_registry_skills(Query(q): Query<RegistryBrowseQuery>) -> AppResult<Json<Value>> {
    let limit = q.limit.clamp(1, 100);
    let installed_names = get_installed_skill_names(q.project_path.as_deref());

    let raw_skills = match &q.query {
        Some(query) if query.chars().count() >= 2 => search_skills(query, limit).await,
        _ => get_homepage_skills(q.force_refresh).await,
    };

    let mut skills = Vec::new();
    for s in raw_skills.into_iter().take(limit as usize) {
        let mut s = s;
        let nm = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
        s["installed"] = Value::Bool(installed_names.contains(nm));
        skills.push(s);
    }

    let total = skills.len();
    Ok(Json(json!({
        "skills": skills,
        "total": total,
        "cached": !q.force_refresh,
    })))
}

/// POST /api/v1/agents/skills/registry/install
async fn install_registry_skill(
    Query(q): Query<ProjectPathQuery>,
    Json(req): Json<RegistryInstallRequest>,
) -> AppResult<Json<Value>> {
    let pp = q.project_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        install_skill_registry(
            &req.source,
            req.skill_names.as_ref(),
            req.global_install,
            pp.as_deref(),
        )
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(Json(result))
}

/// GET /api/v1/agents/{scope}/{name}
async fn get_agent(
    State(state): State<ApiState>,
    axum::extract::Path((scope, name)): axum::extract::Path<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    let agent = service_get_agent(
        &scope,
        &name,
        q.project_path.as_deref(),
        &state.cwd_fallback,
    )
    .ok_or_else(|| AppError::not_found(format!("Agent '{}' not found in {} scope", name, scope)))?;
    Ok(Json(agent))
}

/// POST /api/v1/agents
async fn create_agent(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
    Json(agent): Json<AgentCreate>,
) -> AppResult<impl IntoResponse> {
    if agent.scope != "user" && agent.scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    if agent.scope == "project" && q.project_path.is_none() {
        return Err(AppError::bad_request(
            "project_path is required for project-scoped agents",
        ));
    }

    let base_dir = if agent.scope == "user" {
        paths::get_claude_user_agents_dir()
    } else {
        paths::get_project_agents_dir(q.project_path.as_deref(), &state.cwd_fallback)
    };
    let file_path = base_dir.join(format!("{}.md", agent.name));

    if file_path.exists() {
        // Python: ValueError → HTTPException(400)
        return Err(AppError::bad_request(format!(
            "Agent already exists: {}",
            agent.name
        )));
    }

    paths::ensure_directory_exists(&base_dir);

    let mut metadata = Map::new();
    if let Some(d) = &agent.description {
        if !d.is_empty() {
            metadata.insert("description".into(), json!(d));
        }
    }
    if let Some(t) = &agent.tools {
        if !t.is_empty() {
            metadata.insert("tools".into(), json!(t));
        }
    }
    if let Some(m) = &agent.model {
        if !m.is_empty() {
            metadata.insert("model".into(), json!(m));
        }
    }
    if let Some(dt) = &agent.disallowed_tools {
        if !dt.is_empty() {
            metadata.insert("disallowed-tools".into(), json!(dt));
        }
    }
    if let Some(pm) = &agent.permission_mode {
        if !pm.is_empty() {
            metadata.insert("permission-mode".into(), json!(pm));
        }
    }
    if let Some(sk) = &agent.skills {
        if !sk.is_empty() {
            metadata.insert("skills".into(), json!(sk));
        }
    }
    if let Some(h) = &agent.hooks {
        if !h.is_null() && !(h.is_object() && h.as_object().unwrap().is_empty()) {
            if let Some(hd) = hooks_to_dict(h) {
                metadata.insert("hooks".into(), hd);
            }
        }
    }
    if let Some(mem) = &agent.memory {
        if !mem.is_empty() {
            metadata.insert("memory".into(), json!(mem));
        }
    }

    let frontmatter = build_frontmatter(&metadata);
    let full_content = format!("{}{}", frontmatter, agent.prompt);

    std::fs::write(&file_path, full_content)
        .map_err(|e| AppError::internal(format!("Failed to create agent: {}", e)))?;

    ensure_agent_memory_dir(&agent.name, agent.memory.as_deref());

    let created = json!({
        "name": agent.name,
        "scope": agent.scope,
        "description": agent.description,
        "tools": agent.tools,
        "model": agent.model,
        "prompt": agent.prompt,
        "disallowed_tools": agent.disallowed_tools,
        "permission_mode": agent.permission_mode,
        "skills": agent.skills,
        "hooks": agent.hooks.clone().unwrap_or(Value::Null),
        "memory": agent.memory,
    });
    Ok((StatusCode::CREATED, Json(created)))
}

/// PUT /api/v1/agents/{scope}/{name}
async fn update_agent(
    State(state): State<ApiState>,
    axum::extract::Path((scope, name)): axum::extract::Path<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
    Json(upd): Json<AgentUpdate>,
) -> AppResult<Json<Value>> {
    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }

    let base_dir = agent_base_dir(&scope, q.project_path.as_deref(), &state.cwd_fallback);
    let file_path = base_dir.join(format!("{}.md", name));
    if !file_path.exists() {
        return Err(AppError::not_found(format!(
            "Agent '{}' not found in {} scope",
            name, scope
        )));
    }

    let existing_content = std::fs::read_to_string(&file_path)
        .map_err(|e| AppError::internal(format!("Failed to update agent: {}", e)))?;
    let (metadata_v, mut markdown_content) = parse_frontmatter(&existing_content);
    let mut metadata = metadata_v.as_object().cloned().unwrap_or_default();

    if let Some(v) = &upd.description {
        metadata.insert("description".into(), json!(v));
    }
    if let Some(v) = &upd.tools {
        metadata.insert("tools".into(), json!(v));
    }
    if let Some(v) = &upd.model {
        metadata.insert("model".into(), json!(v));
    }
    if let Some(v) = &upd.disallowed_tools {
        if !v.is_empty() {
            metadata.insert("disallowed-tools".into(), json!(v));
        } else {
            metadata.remove("disallowed-tools");
            metadata.remove("disallowed_tools");
        }
    }
    if let Some(v) = &upd.permission_mode {
        if !v.is_empty() {
            metadata.insert("permission-mode".into(), json!(v));
        } else {
            metadata.remove("permission-mode");
            metadata.remove("permission_mode");
        }
    }
    if let Some(v) = &upd.skills {
        if !v.is_empty() {
            metadata.insert("skills".into(), json!(v));
        } else {
            metadata.remove("skills");
        }
    }
    if let Some(v) = &upd.hooks {
        let empty = v.is_null() || (v.is_object() && v.as_object().unwrap().is_empty());
        if !empty {
            if let Some(hd) = hooks_to_dict(v) {
                metadata.insert("hooks".into(), hd);
            } else {
                metadata.remove("hooks");
            }
        } else {
            metadata.remove("hooks");
        }
    }
    if let Some(v) = &upd.memory {
        if !v.is_empty() && v != "none" {
            metadata.insert("memory".into(), json!(v));
            ensure_agent_memory_dir(&name, Some(v));
        } else {
            metadata.remove("memory");
        }
    }

    if let Some(p) = &upd.prompt {
        markdown_content = p.clone();
    }

    let frontmatter = build_frontmatter(&metadata);
    let full_content = format!("{}{}", frontmatter, markdown_content);

    std::fs::write(&file_path, full_content)
        .map_err(|e| AppError::internal(format!("Failed to update agent: {}", e)))?;

    let result = build_agent(&name, &scope, &Value::Object(metadata), &markdown_content);
    Ok(Json(result))
}

/// DELETE /api/v1/agents/{scope}/{name}
async fn delete_agent(
    State(state): State<ApiState>,
    axum::extract::Path((scope, name)): axum::extract::Path<(String, String)>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<impl IntoResponse> {
    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }

    let base_dir = agent_base_dir(&scope, q.project_path.as_deref(), &state.cwd_fallback);
    let file_path = base_dir.join(format!("{}.md", name));
    if !file_path.exists() {
        return Err(AppError::not_found(format!(
            "Agent '{}' not found in {} scope",
            name, scope
        )));
    }

    std::fs::remove_file(&file_path)
        .map_err(|e| AppError::internal(format!("Failed to delete agent: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}
