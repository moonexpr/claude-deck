// PORTED: plan_service.py + api/v1/plans.py

use axum::{
    Router,
    extract::{Json, Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::fileio::read_json_file;
use crate::paths;

pub fn router() -> Router<ApiState> {
    // Static `/stats` and `/search` MUST precede `/{filename}` (Python ordering).
    Router::new()
        .route("/", get(list_plans))
        .route("/stats", get(get_plan_stats))
        .route("/search", get(search_plans))
        .route("/{filename}", get(get_plan_detail))
}

#[derive(Deserialize)]
struct ProjectPathQuery {
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    project_path: Option<String>,
}

// ---- merged config (subset needed for plansDirectory) -----------------------

fn obj(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}

fn shallow_update(target: &mut Map<String, Value>, src: &Map<String, Value>) {
    for (k, v) in src {
        target.insert(k.clone(), v.clone());
    }
}

/// Port of the `settings` portion of `ConfigService.get_merged_config` — only
/// the merged settings dict is needed to read `plansDirectory`.
fn merged_settings(project_path: Option<&str>) -> Map<String, Value> {
    let mut settings = Map::new();

    let user_settings = paths::get_claude_user_settings_file();
    if user_settings.exists()
        && let Some(data) = read_json_file(&user_settings)
            && let Some(m) = obj(&data) {
                shallow_update(&mut settings, m);
            }

    let user_local = paths::get_claude_user_settings_local_file();
    if user_local.exists()
        && let Some(data) = read_json_file(&user_local)
            && let Some(m) = obj(&data) {
                shallow_update(&mut settings, m);
            }

    if let Some(pp) = project_path {
        let proj = PathBuf::from(pp);

        let proj_settings = proj.join(".claude/settings.json");
        if proj_settings.exists()
            && let Some(data) = read_json_file(&proj_settings)
                && let Some(m) = obj(&data) {
                    shallow_update(&mut settings, m);
                }

        let proj_local = proj.join(".claude/settings.local.json");
        if proj_local.exists()
            && let Some(data) = read_json_file(&proj_local)
                && let Some(m) = obj(&data) {
                    shallow_update(&mut settings, m);
                }
    }

    settings
}

/// Port of `PlanService.resolve_plans_dir`.
fn resolve_plans_dir(project_path: Option<&str>, cwd_fallback: &Path) -> PathBuf {
    let settings = merged_settings(project_path);
    let plans_dir_setting = settings
        .get("plansDirectory")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    if let Some(raw) = plans_dir_setting {
        // Path(setting).expanduser()
        let expanded = if let Some(rest) = raw.strip_prefix('~') {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
            if rest.is_empty() {
                home
            } else {
                home.join(rest.trim_start_matches('/'))
            }
        } else {
            PathBuf::from(raw)
        };

        if expanded.is_absolute() {
            return expanded;
        }
        // Relative to project root (or cwd_fallback when no project_path).
        let base = match project_path {
            Some(p) => PathBuf::from(p),
            None => cwd_fallback.to_path_buf(),
        };
        let joined = base.join(&expanded);
        return std::fs::canonicalize(&joined).unwrap_or(joined);
    }

    paths::get_claude_plans_dir()
}

// ---- markdown extraction helpers --------------------------------------------

fn extract_title(content: &str) -> String {
    for raw in content.split('\n') {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("# ") {
            let mut title = rest.trim().to_string();
            let lower = title.to_lowercase();
            if lower.starts_with("plan:") {
                title = title[5..].trim().to_string();
            } else if lower.starts_with("plan —") {
                // Python slices [6:] on the original string. "plan —" is 7
                // bytes in UTF-8 (— is 3 bytes) but Python indexes by chars:
                // p-l-a-n-space-— = 6 chars.
                title = title.chars().skip(6).collect::<String>().trim().to_string();
            }
            return title;
        }
    }
    "(untitled)".to_string()
}

fn extract_excerpt(content: &str, max_len: usize) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut body_lines: Vec<String> = Vec::new();
    let mut past_title = false;
    for line in &lines {
        let stripped = line.trim();
        if !past_title {
            if stripped.starts_with("# ") {
                past_title = true;
                continue;
            }
            if stripped.is_empty() {
                continue;
            }
            past_title = true;
        }
        if !stripped.is_empty() && !stripped.starts_with("---") {
            body_lines.push(stripped.to_string());
            if body_lines.join(" ").chars().count() >= max_len {
                break;
            }
        }
    }

    let excerpt = body_lines.join(" ");
    if excerpt.chars().count() > max_len {
        let truncated: String = excerpt.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    } else {
        excerpt
    }
}

fn extract_headings(content: &str) -> Vec<String> {
    let mut headings = Vec::new();
    for raw in content.split('\n') {
        let stripped = raw.trim();
        if stripped.starts_with("## ") || stripped.starts_with("### ") {
            headings.push(stripped.trim_start_matches('#').trim().to_string());
        }
    }
    headings
}

fn count_code_blocks(content: &str) -> usize {
    content
        .split('\n')
        .filter(|line| line.starts_with("```"))
        .count()
        / 2
}

fn is_table_separator(line: &str) -> bool {
    // Python regex: ^\|[\s:|-]+\|$
    let s = line.trim();
    if s.len() < 3 || !s.starts_with('|') || !s.ends_with('|') {
        return false;
    }
    let inner = &s[1..s.len() - 1];
    !inner.is_empty()
        && inner
            .chars()
            .all(|c| c.is_whitespace() || c == ':' || c == '|' || c == '-')
}

fn count_tables(content: &str) -> usize {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut count = 0;
    for i in 0..lines.len() {
        if lines[i].contains('|') && i + 1 < lines.len() && is_table_separator(lines[i + 1]) {
            count += 1;
        }
    }
    count
}

/// Unix mtime -> Python `datetime.fromtimestamp(mtime).isoformat()` (local TZ).
fn mtime_iso(meta: &std::fs::Metadata) -> String {
    let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let dt: chrono::DateTime<chrono::Local> = modified.into();
    dt.naive_local().format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

/// Python `dir.glob("*.md")` — one level, names ending `.md`.
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
    out
}

// ---- plan operations --------------------------------------------------------

fn list_plans_impl(plans_dir: &Path) -> Vec<Value> {
    let mut plans: Vec<Value> = Vec::new();
    if !plans_dir.exists() {
        return plans;
    }
    for plan_file in glob_md(plans_dir) {
        let Ok(meta) = std::fs::metadata(&plan_file) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&plan_file) else {
            continue;
        };
        let title = extract_title(&content);
        let excerpt = extract_excerpt(&content, 200);
        plans.push(json!({
            "filename": plan_file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            "slug": plan_file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            "title": title,
            "excerpt": excerpt,
            "modified_at": mtime_iso(&meta),
            "size_bytes": meta.len(),
        }));
    }
    plans.sort_by(|a, b| {
        let ma = a.get("modified_at").and_then(|v| v.as_str()).unwrap_or("");
        let mb = b.get("modified_at").and_then(|v| v.as_str()).unwrap_or("");
        mb.cmp(ma)
    });
    plans
}

fn get_plan_impl(plans_dir: &Path, filename: &str) -> Option<Value> {
    let plan_file = plans_dir.join(filename);
    if !plan_file.exists() || plan_file.extension().and_then(|e| e.to_str()) != Some("md") {
        return None;
    }
    let meta = std::fs::metadata(&plan_file).ok()?;
    let content = std::fs::read_to_string(&plan_file).ok()?;
    Some(json!({
        "filename": plan_file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "slug": plan_file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "title": extract_title(&content),
        "content": content,
        "modified_at": mtime_iso(&meta),
        "size_bytes": meta.len(),
        "headings": extract_headings(&content),
        "code_block_count": count_code_blocks(&content),
        "table_count": count_tables(&content),
    }))
}

fn search_plans_impl(plans_dir: &Path, query: &str) -> Vec<Value> {
    let mut results: Vec<Value> = Vec::new();
    let query_lower = query.to_lowercase();
    if !plans_dir.exists() {
        return results;
    }
    for plan_file in glob_md(plans_dir) {
        let Ok(content) = std::fs::read_to_string(&plan_file) else {
            continue;
        };
        let content_lower = content.to_lowercase();
        if !content_lower.contains(&query_lower) {
            continue;
        }
        let title = extract_title(&content);
        let Ok(meta) = std::fs::metadata(&plan_file) else {
            continue;
        };

        let mut matches: Vec<String> = Vec::new();
        for line in content.split('\n') {
            let line_lower = line.to_lowercase();
            if line_lower.contains(&query_lower) {
                let stripped = line.trim();
                let snippet = if stripped.chars().count() > 120 {
                    // Python indexes the raw `line` (not stripped) by char.
                    let lchars: Vec<char> = line.chars().collect();
                    let llower_chars: Vec<char> = line_lower.chars().collect();
                    let idx = find_char_index(&llower_chars, &query_lower).unwrap_or(0);
                    let start = idx.saturating_sub(40);
                    let qlen = query.chars().count();
                    let end = std::cmp::min(stripped.chars().count(), idx + qlen + 40);
                    let mid: String = lchars
                        .iter()
                        .skip(start)
                        .take(end.saturating_sub(start))
                        .collect();
                    let prefix = if start > 0 { "..." } else { "" };
                    let suffix = if end < stripped.chars().count() {
                        "..."
                    } else {
                        ""
                    };
                    format!("{}{}{}", prefix, mid, suffix)
                } else {
                    stripped.to_string()
                };
                matches.push(snippet);
                if matches.len() >= 3 {
                    break;
                }
            }
        }

        results.push(json!({
            "filename": plan_file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            "slug": plan_file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            "title": title,
            "matches": matches,
            "modified_at": mtime_iso(&meta),
        }));
    }
    results.sort_by(|a, b| {
        let ma = a.get("modified_at").and_then(|v| v.as_str()).unwrap_or("");
        let mb = b.get("modified_at").and_then(|v| v.as_str()).unwrap_or("");
        mb.cmp(ma)
    });
    results
}

/// Find the char index of `needle` within `haystack` (already lowercased char
/// vec), mirroring Python `str.index`.
fn find_char_index(haystack: &[char], needle: &str) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() {
        return Some(0);
    }
    if haystack.len() < n.len() {
        return None;
    }
    for i in 0..=haystack.len() - n.len() {
        if haystack[i..i + n.len()] == n[..] {
            return Some(i);
        }
    }
    None
}

fn empty_stats() -> Value {
    json!({
        "total_plans": 0,
        "oldest_date": Value::Null,
        "newest_date": Value::Null,
        "total_size_bytes": 0,
    })
}

fn get_plan_stats_impl(plans_dir: &Path) -> Value {
    if !plans_dir.exists() {
        return empty_stats();
    }
    let plan_files = glob_md(plans_dir);
    if plan_files.is_empty() {
        return empty_stats();
    }
    let mut mtimes: Vec<std::time::SystemTime> = Vec::new();
    let mut total_size: u64 = 0;
    for f in &plan_files {
        if let Ok(meta) = std::fs::metadata(f) {
            mtimes.push(meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH));
            total_size += meta.len();
        }
    }
    let to_iso = |t: std::time::SystemTime| {
        let dt: chrono::DateTime<chrono::Local> = t.into();
        dt.naive_local().format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
    };
    let oldest = mtimes.iter().min().copied();
    let newest = mtimes.iter().max().copied();
    json!({
        "total_plans": plan_files.len(),
        "oldest_date": oldest.map(to_iso),
        "newest_date": newest.map(to_iso),
        "total_size_bytes": total_size,
    })
}

/// Port of `PlanService.get_plan_sessions` + `_scan_jsonl_for_slug`.
fn get_plan_sessions(slug: &str) -> Vec<Value> {
    let mut sessions: Vec<Value> = Vec::new();
    let projects_dir = paths::get_claude_projects_dir();
    if !projects_dir.exists() {
        return sessions;
    }

    let Ok(rd) = std::fs::read_dir(&projects_dir) else {
        return sessions;
    };
    for entry in rd.flatten() {
        let project_folder = entry.path();
        if !project_folder.is_dir() {
            continue;
        }
        let folder_name = project_folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let Ok(jrd) = std::fs::read_dir(&project_folder) else {
            continue;
        };
        for jentry in jrd.flatten() {
            let jp = jentry.path();
            if jp.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(info) = scan_jsonl_for_slug(&jp, slug, &folder_name) {
                sessions.push(info);
            }
        }
    }

    sessions.sort_by(|a, b| {
        let la = a.get("last_seen").and_then(|v| v.as_str()).unwrap_or("");
        let lb = b.get("last_seen").and_then(|v| v.as_str()).unwrap_or("");
        lb.cmp(la)
    });
    sessions
}

fn scan_jsonl_for_slug(filepath: &Path, slug: &str, project_folder: &str) -> Option<Value> {
    let session_id = filepath
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let content = std::fs::read_to_string(filepath).ok()?;
    let mut first_seen: Option<String> = None;
    let mut last_seen: Option<String> = None;
    let mut git_branch: Option<String> = None;

    for line in content.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let obj: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if obj.get("slug").and_then(|v| v.as_str()) != Some(slug) {
            continue;
        }

        if let Some(ts) = obj.get("timestamp").and_then(|v| v.as_str()) {
            if first_seen.is_none() {
                first_seen = Some(ts.to_string());
            }
            last_seen = Some(ts.to_string());
        }
        if git_branch.is_none()
            && let Some(gb) = obj.get("gitBranch").and_then(|v| v.as_str())
                && !gb.is_empty() {
                    git_branch = Some(gb.to_string());
                }
    }

    first_seen.as_ref()?;

    Some(json!({
        "session_id": session_id,
        "project_folder": project_folder,
        "project_name": paths::get_project_display_name(project_folder),
        "git_branch": git_branch,
        "first_seen": first_seen,
        "last_seen": last_seen,
    }))
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/plans
async fn list_plans(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let plans_dir = resolve_plans_dir(q.project_path.as_deref(), &state.cwd_fallback);
    let plans = list_plans_impl(&plans_dir);
    let total = plans.len();
    Ok(Json(json!({ "plans": plans, "total": total })))
}

/// GET /api/v1/plans/stats
async fn get_plan_stats(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let plans_dir = resolve_plans_dir(q.project_path.as_deref(), &state.cwd_fallback);
    Ok(Json(get_plan_stats_impl(&plans_dir)))
}

/// GET /api/v1/plans/search
async fn search_plans(
    State(state): State<ApiState>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<Value>> {
    if q.q.is_empty() {
        return Err(AppError::bad_request(
            "ensure this value has at least 1 characters",
        ));
    }
    let plans_dir = resolve_plans_dir(q.project_path.as_deref(), &state.cwd_fallback);
    let results = search_plans_impl(&plans_dir, &q.q);
    let total = results.len();
    Ok(Json(
        json!({ "results": results, "query": q.q, "total": total }),
    ))
}

/// GET /api/v1/plans/{filename}
async fn get_plan_detail(
    State(state): State<ApiState>,
    axum::extract::Path(filename): axum::extract::Path<String>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let plans_dir = resolve_plans_dir(q.project_path.as_deref(), &state.cwd_fallback);
    let plan_data = get_plan_impl(&plans_dir, &filename);

    let Some(mut plan_data) = plan_data else {
        return Err(AppError::not_found("Plan not found"));
    };

    let slug = plan_data
        .get("slug")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let linked = get_plan_sessions(&slug);
    if let Some(m) = plan_data.as_object_mut() {
        m.insert("linked_sessions".to_string(), json!(linked));
    }

    Ok(Json(json!({ "plan": plan_data })))
}
