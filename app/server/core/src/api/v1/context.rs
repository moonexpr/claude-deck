// PORTED: context_service.py + api/v1/context.py

use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    routing::get,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/active", get(get_active_sessions))
        .route("/{project_folder}/{session_id}", get(get_session_context))
}

const DEFAULT_CONTEXT_LIMIT: i64 = 200_000;
const ACTIVE_SESSION_THRESHOLD_SECONDS: f64 = 600.0;
const CHARS_PER_TOKEN_ESTIMATE: i64 = 4;
const SYSTEM_TOOLS_ESTIMATE: i64 = 8_200;

fn model_context_limits() -> &'static [(&'static str, i64)] {
    &[
        ("claude-fable-5", 1_000_000),
        ("claude-mythos-5", 1_000_000),
        ("claude-opus-4-8", 1_000_000),
        ("claude-opus-4-7", 1_000_000),
        ("claude-opus-4-6", 1_000_000),
        ("claude-sonnet-4-6", 1_000_000),
        ("claude-haiku-4-5", 200_000),
        ("claude-sonnet-4-5", 200_000),
        ("claude-sonnet-4", 200_000),
        ("claude-opus-4", 200_000),
        ("claude-3-5-sonnet", 200_000),
        ("claude-3-5-haiku", 200_000),
        ("claude-3-opus", 200_000),
    ]
}

fn normalize_model(model: &str) -> String {
    if model.is_empty() {
        return model.to_string();
    }
    if let Some(idx) = model.rfind('-') {
        let (head, tail) = model.split_at(idx);
        let suffix = &tail[1..];
        if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
            return head.to_string();
        }
    }
    model.to_string()
}

fn get_context_limit(model: &str) -> i64 {
    let normalized = normalize_model(model);
    for (key, limit) in model_context_limits() {
        if normalized == *key {
            return *limit;
        }
    }
    let mut best: (i64, i64) = (-1, DEFAULT_CONTEXT_LIMIT);
    for (key, limit) in model_context_limits() {
        if normalized.starts_with(key) && (key.len() as i64) > best.0 {
            best = (key.len() as i64, *limit);
        }
    }
    best.1
}

fn get_context_zone(percentage: f64) -> &'static str {
    if percentage >= 95.0 {
        "red"
    } else if percentage >= 80.0 {
        "orange"
    } else if percentage >= 50.0 {
        "yellow"
    } else {
        "green"
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn parse_jsonl(content: &str) -> Vec<Value> {
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            entries.push(v);
        }
    }
    entries
}

fn usage_int(usage: &Value, key: &str) -> i64 {
    usage.get(key).and_then(|v| v.as_i64()).unwrap_or(0)
}

fn get_total_input_context(usage: &Value) -> i64 {
    usage_int(usage, "cache_read_input_tokens")
        + usage_int(usage, "cache_creation_input_tokens")
        + usage_int(usage, "input_tokens")
}

/// GET /api/v1/context/active
async fn get_active_sessions() -> AppResult<Json<Value>> {
    let projects_dir = crate::paths::get_claude_projects_dir();
    let mut sessions: Vec<Value> = Vec::new();

    if !projects_dir.exists() {
        return Ok(Json(json!({ "sessions": [] })));
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let Ok(rd) = std::fs::read_dir(&projects_dir) else {
        return Ok(Json(json!({ "sessions": [] })));
    };

    for entry in rd.flatten() {
        let project_folder = entry.path();
        if !project_folder.is_dir() {
            continue;
        }
        let folder_name = project_folder
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let Ok(files) = std::fs::read_dir(&project_folder) else {
            continue;
        };
        for f in files.flatten() {
            let jsonl_file = f.path();
            if jsonl_file.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&jsonl_file) else {
                continue;
            };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);

            let age_seconds = now - mtime;
            let is_active = age_seconds <= ACTIVE_SESSION_THRESHOLD_SECONDS;

            if age_seconds > 3600.0 {
                continue;
            }

            let Some((usage, model, timestamp)) = get_last_assistant_usage(&jsonl_file) else {
                continue;
            };

            let total_context = get_total_input_context(&usage);
            let max_context = get_context_limit(&model);
            let percentage = if max_context > 0 {
                (100.0_f64).min(total_context as f64 / max_context as f64 * 100.0)
            } else {
                0.0
            };

            let stem = jsonl_file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();

            sessions.push(json!({
                "session_id": stem,
                "project_folder": folder_name,
                "project_name": crate::paths::get_project_display_name(&folder_name),
                "model": model,
                "context_percentage": round1(percentage),
                "current_context_tokens": total_context,
                "max_context_tokens": max_context,
                "is_active": is_active,
                "last_activity": timestamp,
            }));
        }
    }

    sessions.sort_by(|a, b| {
        let a_active = a
            .get("is_active")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let b_active = b
            .get("is_active")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let a_pct = a
            .get("context_percentage")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let b_pct = b
            .get("context_percentage")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        // Python key: (not is_active, -context_percentage)
        (!a_active).cmp(&(!b_active)).then(
            (-a_pct)
                .partial_cmp(&(-b_pct))
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    Ok(Json(json!({ "sessions": sessions })))
}

fn get_last_assistant_usage(filepath: &Path) -> Option<(Value, String, String)> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(filepath).ok()?;
    let file_size = file.metadata().ok()?.len();
    let read_size = file_size.min(32 * 1024);

    let buf: String;
    if file_size > read_size {
        file.seek(SeekFrom::Start(file_size - read_size)).ok()?;
        let mut raw = Vec::new();
        file.read_to_end(&mut raw).ok()?;
        let s = String::from_utf8_lossy(&raw);
        // Skip partial first line
        if let Some(idx) = s.find('\n') {
            buf = s[idx + 1..].to_string();
        } else {
            buf = s.to_string();
        }
    } else {
        let mut s = String::new();
        file.read_to_string(&mut s).ok()?;
        buf = s;
    }

    let lines: Vec<&str> = buf.trim().split('\n').collect();
    for line in lines.iter().rev() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if obj.get("type").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let message = obj.get("message").cloned().unwrap_or(json!({}));
        let usage = message.get("usage").cloned();
        let Some(usage) = usage else { continue };
        if usage.is_null() {
            continue;
        }
        let model = message
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let timestamp = obj
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some((usage, model, timestamp));
    }
    None
}

fn content_blocks(message: &Value) -> Vec<Value> {
    let content = message.get("content").cloned().unwrap_or(json!([]));
    match content {
        Value::String(s) => vec![json!({"type": "text", "text": s})],
        Value::Array(a) => a,
        _ => vec![],
    }
}

fn result_chars(result_content: &Value) -> i64 {
    match result_content {
        Value::String(s) => s.chars().count() as i64,
        Value::Array(a) => {
            let mut total = 0;
            for rc in a {
                if rc.get("type").and_then(|v| v.as_str()) == Some("text") {
                    total += rc
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| s.chars().count())
                        .unwrap_or(0) as i64;
                }
            }
            total
        }
        _ => 0,
    }
}

/// Composition breakdown. The MCP/agent/memory/skill sub-services are not part
/// of this DB/websocket port; Python wraps each in `try/except: pass`, so when
/// they are unavailable the result is the residual-only composition computed
/// here with those categories at zero — byte-for-byte identical to Python's
/// behavior in that situation.
fn get_context_composition(model: &str, current_context_tokens: i64, message_chars: i64) -> Value {
    let context_limit = get_context_limit(model);
    let autocompact_buffer = (context_limit as f64 * 0.165) as i64;

    let mcp_total: i64 = 0;
    let agent_total: i64 = 0;
    let memory_total: i64 = 0;
    let skill_total: i64 = 0;

    let messages_tokens = message_chars / CHARS_PER_TOKEN_ESTIMATE;

    let system_total = (current_context_tokens
        - messages_tokens
        - mcp_total
        - agent_total
        - memory_total
        - skill_total)
        .max(0);
    let system_tools_tokens = system_total.min(SYSTEM_TOOLS_ESTIMATE);
    let system_prompt_tokens = system_total - system_tools_tokens;

    let free_space = (context_limit - current_context_tokens - autocompact_buffer).max(0);

    let mut categories: Vec<Value> = Vec::new();
    let mut add = |name: &str, tokens: i64, color: &str| {
        let pct = if context_limit > 0 {
            tokens as f64 / context_limit as f64 * 100.0
        } else {
            0.0
        };
        if tokens > 0 || name == "Free Space" {
            categories.push(json!({
                "category": name,
                "estimated_tokens": tokens,
                "percentage": round1(pct),
                "color": color,
                "items": Value::Null,
            }));
        }
    };

    add("System prompt", system_prompt_tokens, "#6b7280");
    add("System tools", system_tools_tokens, "#9ca3af");
    add("MCP tools", mcp_total, "#0891b2");
    add("Custom agents", agent_total, "#b1b9f9");
    add("Memory files", memory_total, "#d77757");
    add("Skills", skill_total, "#ffc107");
    add("Messages", messages_tokens, "#9333ea");
    add("Autocompact buffer", autocompact_buffer, "#555555");
    add("Free space", free_space, "#333333");

    let total_tokens: i64 = categories
        .iter()
        .map(|c| {
            c.get("estimated_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
        })
        .sum();

    json!({
        "categories": categories,
        "total_tokens": total_tokens,
        "context_limit": context_limit,
        "model": model,
    })
}

/// GET /api/v1/context/{project_folder}/{session_id}
async fn get_session_context(
    State(_state): State<ApiState>,
    AxumPath((project_folder, session_id)): AxumPath<(String, String)>,
) -> AppResult<Json<Value>> {
    let projects_dir = crate::paths::get_claude_projects_dir();
    let filepath = projects_dir
        .join(&project_folder)
        .join(format!("{}.jsonl", session_id));

    if !filepath.exists() {
        return Err(AppError::not_found(format!(
            "Session not found: {}",
            session_id
        )));
    }

    let content = std::fs::read_to_string(&filepath).unwrap_or_default();
    let entries = parse_jsonl(&content);

    let mut snapshots: Vec<Value> = Vec::new();
    let mut turn_number: i64 = 0;
    let mut model = "unknown".to_string();

    let mut user_chars: i64 = 0;
    let mut assistant_chars: i64 = 0;
    let mut tool_result_chars: i64 = 0;
    let mut tool_call_chars: i64 = 0;
    let mut thinking_chars: i64 = 0;

    // path -> (count, chars), preserving insertion order
    let mut file_reads: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut file_reads_order: Vec<String> = Vec::new();
    // tool_name -> (count, result_chars), preserving insertion order
    let mut tool_stats: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut tool_stats_order: Vec<String> = Vec::new();
    let mut tool_use_id_to_name: BTreeMap<String, String> = BTreeMap::new();

    let mut total_cache_read: i64 = 0;
    let mut total_cache_creation: i64 = 0;
    let mut total_uncached: i64 = 0;

    for entry in &entries {
        let entry_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let message = entry.get("message").cloned().unwrap_or(json!({}));
        let content = content_blocks(&message);

        if entry_type == "user" {
            for block in &content {
                if !block.is_object() {
                    continue;
                }
                let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if block_type == "text" {
                    user_chars += block
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| s.chars().count())
                        .unwrap_or(0) as i64;
                } else if block_type == "tool_result" {
                    let rc_val = block.get("content").cloned().unwrap_or(json!(""));
                    let rchars = result_chars(&rc_val);
                    tool_result_chars += rchars;

                    if let Some(tool_use_id) = block.get("tool_use_id").and_then(|v| v.as_str())
                        && let Some(name) = tool_use_id_to_name.get(tool_use_id).cloned() {
                            let st = tool_stats.entry(name.clone()).or_insert_with(|| {
                                tool_stats_order.push(name.clone());
                                (0, 0)
                            });
                            st.1 += rchars;
                        }
                }
            }
        } else if entry_type == "assistant" {
            let usage = message.get("usage").cloned();
            let entry_model = message
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| model.clone());
            let timestamp = entry
                .get("timestamp")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            for block in &content {
                if !block.is_object() {
                    continue;
                }
                let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if block_type == "text" {
                    assistant_chars += block
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| s.chars().count())
                        .unwrap_or(0) as i64;
                } else if block_type == "thinking" {
                    thinking_chars += block
                        .get("thinking")
                        .and_then(|v| v.as_str())
                        .map(|s| s.chars().count())
                        .unwrap_or(0) as i64;
                } else if block_type == "tool_use" {
                    let tool_input = block.get("input").cloned().unwrap_or(json!({}));
                    tool_call_chars += serde_json::to_string(&tool_input)
                        .unwrap_or_default()
                        .chars()
                        .count() as i64;

                    let tool_name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tool_use_id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    if !tool_name.is_empty() {
                        let st = tool_stats.entry(tool_name.clone()).or_insert_with(|| {
                            tool_stats_order.push(tool_name.clone());
                            (0, 0)
                        });
                        st.0 += 1;
                        if let Some(tid) = &tool_use_id {
                            tool_use_id_to_name.insert(tid.clone(), tool_name.clone());
                        }
                    }

                    if tool_name == "Read"
                        && let Some(fp) = tool_input.get("file_path").and_then(|v| v.as_str())
                            && !fp.is_empty() {
                                file_reads
                                    .entry(fp.to_string())
                                    .or_insert_with(|| {
                                        file_reads_order.push(fp.to_string());
                                        (0, 0)
                                    })
                                    .0 += 1;
                            }
                }
            }

            if let Some(usage) = usage.filter(|u| !u.is_null()) {
                turn_number += 1;
                model = entry_model.clone();

                let cache_read = usage_int(&usage, "cache_read_input_tokens");
                let cache_creation = usage_int(&usage, "cache_creation_input_tokens");
                let input_tokens = usage_int(&usage, "input_tokens");
                let output_tokens = usage_int(&usage, "output_tokens");
                let total_context = cache_read + cache_creation + input_tokens;

                total_cache_read += cache_read;
                total_cache_creation += cache_creation;
                total_uncached += input_tokens;

                let max_context = get_context_limit(&entry_model);
                let percentage = if max_context > 0 {
                    (100.0_f64).min(total_context as f64 / max_context as f64 * 100.0)
                } else {
                    0.0
                };

                snapshots.push(json!({
                    "turn_number": turn_number,
                    "timestamp": timestamp,
                    "total_context_tokens": total_context,
                    "input_tokens": input_tokens,
                    "cache_creation_tokens": cache_creation,
                    "cache_read_tokens": cache_read,
                    "output_tokens": output_tokens,
                    "model": entry_model,
                    "context_percentage": round1(percentage),
                }));
            }
        }
    }

    // Second pass: match Read tool_use with the subsequent tool_result for chars
    let mut pending_read_path: Option<String> = None;
    for entry in &entries {
        let entry_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let message = entry.get("message").cloned().unwrap_or(json!({}));
        let content = content_blocks(&message);

        if entry_type == "assistant" {
            for block in &content {
                if !block.is_object() {
                    continue;
                }
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_use")
                    && block.get("name").and_then(|v| v.as_str()) == Some("Read")
                {
                    pending_read_path = block
                        .get("input")
                        .and_then(|i| i.get("file_path"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
        } else if entry_type == "user" && pending_read_path.is_some() {
            for block in &content {
                if !block.is_object() {
                    continue;
                }
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                    let rc_val = block.get("content").cloned().unwrap_or(json!(""));
                    let char_count = result_chars(&rc_val);
                    if let Some(p) = &pending_read_path
                        && let Some(fr) = file_reads.get_mut(p) {
                            fr.1 += char_count;
                        }
                    pending_read_path = None;
                    break;
                }
            }
        }
    }

    // Content categories
    let total_chars =
        user_chars + assistant_chars + tool_result_chars + tool_call_chars + thinking_chars;
    let mut categories: Vec<Value> = Vec::new();
    for (name, chars) in [
        ("User Messages", user_chars),
        ("Assistant Messages", assistant_chars),
        ("Tool Results", tool_result_chars),
        ("Tool Calls", tool_call_chars),
        ("Thinking", thinking_chars),
    ] {
        if chars > 0 {
            let est_tokens = chars / CHARS_PER_TOKEN_ESTIMATE;
            let pct = if total_chars > 0 {
                chars as f64 / total_chars as f64 * 100.0
            } else {
                0.0
            };
            categories.push(json!({
                "category": name,
                "estimated_chars": chars,
                "estimated_tokens": est_tokens,
                "percentage": round1(pct),
            }));
        }
    }
    categories.sort_by(|a, b| {
        let bt = b
            .get("estimated_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let at = a
            .get("estimated_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        bt.cmp(&at)
    });

    // File consumption
    let mut file_consumptions: Vec<Value> = Vec::new();
    for fpath in &file_reads_order {
        let (count, chars) = file_reads.get(fpath).copied().unwrap_or((0, 0));
        file_consumptions.push(json!({
            "file_path": fpath,
            "read_count": count,
            "total_chars": chars,
            "estimated_tokens": chars / CHARS_PER_TOKEN_ESTIMATE,
        }));
    }
    file_consumptions.sort_by(|a, b| {
        let bt = b
            .get("estimated_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let at = a
            .get("estimated_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        bt.cmp(&at)
    });
    file_consumptions.truncate(50);

    // Per-tool consumption
    let mut tool_consumptions: Vec<Value> = Vec::new();
    for name in &tool_stats_order {
        let (count, rchars) = tool_stats.get(name).copied().unwrap_or((0, 0));
        let result_tokens = rchars / CHARS_PER_TOKEN_ESTIMATE;
        let avg_tokens = if count > 0 { result_tokens / count } else { 0 };
        tool_consumptions.push(json!({
            "tool_name": name,
            "call_count": count,
            "total_result_chars": rchars,
            "total_result_tokens": result_tokens,
            "avg_result_tokens": avg_tokens,
        }));
    }
    tool_consumptions.sort_by(|a, b| {
        let bt = b
            .get("total_result_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let at = a
            .get("total_result_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let bc = b.get("call_count").and_then(|v| v.as_i64()).unwrap_or(0);
        let ac = a.get("call_count").and_then(|v| v.as_i64()).unwrap_or(0);
        bt.cmp(&at).then(bc.cmp(&ac))
    });

    // Cache efficiency
    let total_input_all = total_cache_read + total_cache_creation + total_uncached;
    let hit_ratio = if total_input_all > 0 {
        total_cache_read as f64 / total_input_all as f64
    } else {
        0.0
    };
    let cache_efficiency = json!({
        "total_cache_read": total_cache_read,
        "total_cache_creation": total_cache_creation,
        "total_uncached": total_uncached,
        "hit_ratio": round3(hit_ratio),
    });

    let max_context = get_context_limit(&model);
    let mut current_context: i64 = 0;
    let mut context_percentage: f64 = 0.0;
    if let Some(last) = snapshots.last() {
        current_context = last
            .get("total_context_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        context_percentage = last
            .get("context_percentage")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
    }

    let mut avg_tokens_per_turn: i64 = 0;
    let mut estimated_turns_remaining: i64 = 0;
    if snapshots.len() >= 2 {
        let mut growths: Vec<i64> = Vec::new();
        for i in 1..snapshots.len() {
            let cur = snapshots[i]
                .get("total_context_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let prev = snapshots[i - 1]
                .get("total_context_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let growth = cur - prev;
            if growth > 0 {
                growths.push(growth);
            }
        }
        if !growths.is_empty() {
            avg_tokens_per_turn = growths.iter().sum::<i64>() / growths.len() as i64;
            let remaining_tokens = max_context - current_context;
            if avg_tokens_per_turn > 0 {
                estimated_turns_remaining = (remaining_tokens / avg_tokens_per_turn).max(0);
            }
        }
    } else if snapshots.len() == 1 {
        avg_tokens_per_turn = current_context;
        let remaining_tokens = max_context - current_context;
        if avg_tokens_per_turn > 0 {
            estimated_turns_remaining = (remaining_tokens / avg_tokens_per_turn).max(0);
        }
    }

    let conversation_chars = user_chars + assistant_chars + tool_result_chars + tool_call_chars;
    let composition = get_context_composition(&model, current_context, conversation_chars);

    let analysis = json!({
        "session_id": session_id,
        "project_folder": project_folder,
        "project_name": crate::paths::get_project_display_name(&project_folder),
        "model": model,
        "current_context_tokens": current_context,
        "max_context_tokens": max_context,
        "context_percentage": round1(context_percentage),
        "snapshots": snapshots,
        "content_categories": categories,
        "file_consumptions": file_consumptions,
        "tool_consumptions": tool_consumptions,
        "cache_efficiency": cache_efficiency,
        "avg_tokens_per_turn": avg_tokens_per_turn,
        "estimated_turns_remaining": estimated_turns_remaining,
        "context_zone": get_context_zone(context_percentage),
        "total_turns": turn_number,
        "composition": composition,
    });

    Ok(Json(json!({ "analysis": analysis })))
}
