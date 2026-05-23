// PORTED: usage_service.py + pricing_service.py + api/v1/usage.py
//
// Faithful port of `backend_python/app/{services/usage_service.py,
// services/pricing_service.py, api/v1/usage.py}`. Route paths, query field
// names and response JSON shapes are byte-identical to the Python router (the
// unchanged frontend was built against them). JSONL parsing, date bucketing,
// model-pricing lookup and 5-hour block detection mirror the Python exactly.

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Timelike, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::paths::{convert_path_to_folder_name, get_claude_projects_dir};

pub fn router() -> Router<ApiState> {
    // Paths mirror Python's `APIRouter(prefix="/usage")` exactly.
    Router::new()
        .route("/summary", get(get_usage_summary))
        .route("/daily", get(get_daily_usage))
        .route("/sessions", get(get_session_usage))
        .route("/monthly", get(get_monthly_usage))
        .route("/blocks", get(get_block_usage))
        .route("/export", get(export_usage))
        .route("/cache/invalidate", post(invalidate_cache))
}

const CACHE_TTL_MINUTES: i64 = 5;
const SESSION_DURATION_HOURS: i64 = 5;
const DEFAULT_RECENT_DAYS: i64 = 3;
const TIERED_THRESHOLD: i64 = 200_000;

// ===========================================================================
// Pricing (port of pricing_service.py)
// ===========================================================================

struct ModelPrice {
    input: Option<f64>,
    output: Option<f64>,
    cache_creation: Option<f64>,
    cache_read: Option<f64>,
    input_above_200k: Option<f64>,
    output_above_200k: Option<f64>,
    cache_creation_above_200k: Option<f64>,
    cache_read_above_200k: Option<f64>,
}

const M: f64 = 1_000_000.0;

/// Port of `PricingService.MODEL_PRICING` (LiteLLM-derived static table).
fn model_pricing_table() -> &'static [(&'static str, ModelPrice)] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<(&'static str, ModelPrice)>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            vec![
                (
                    "claude-sonnet-4-20250514",
                    ModelPrice {
                        input: Some(3.00 / M),
                        output: Some(15.00 / M),
                        cache_creation: Some(3.75 / M),
                        cache_read: Some(0.30 / M),
                        input_above_200k: Some(6.00 / M),
                        output_above_200k: Some(22.50 / M),
                        cache_creation_above_200k: Some(7.50 / M),
                        cache_read_above_200k: Some(0.60 / M),
                    },
                ),
                (
                    "claude-opus-4-20250514",
                    ModelPrice {
                        input: Some(15.00 / M),
                        output: Some(75.00 / M),
                        cache_creation: Some(18.75 / M),
                        cache_read: Some(1.50 / M),
                        input_above_200k: Some(30.00 / M),
                        output_above_200k: Some(112.50 / M),
                        cache_creation_above_200k: Some(37.50 / M),
                        cache_read_above_200k: Some(3.00 / M),
                    },
                ),
                (
                    "claude-opus-4-5-20251101",
                    ModelPrice {
                        input: Some(15.00 / M),
                        output: Some(75.00 / M),
                        cache_creation: Some(18.75 / M),
                        cache_read: Some(1.50 / M),
                        input_above_200k: Some(30.00 / M),
                        output_above_200k: Some(112.50 / M),
                        cache_creation_above_200k: Some(37.50 / M),
                        cache_read_above_200k: Some(3.00 / M),
                    },
                ),
                (
                    "claude-3-5-sonnet-20241022",
                    ModelPrice {
                        input: Some(3.00 / M),
                        output: Some(15.00 / M),
                        cache_creation: Some(3.75 / M),
                        cache_read: Some(0.30 / M),
                        input_above_200k: None,
                        output_above_200k: None,
                        cache_creation_above_200k: None,
                        cache_read_above_200k: None,
                    },
                ),
                (
                    "claude-3-5-sonnet-20240620",
                    ModelPrice {
                        input: Some(3.00 / M),
                        output: Some(15.00 / M),
                        cache_creation: Some(3.75 / M),
                        cache_read: Some(0.30 / M),
                        input_above_200k: None,
                        output_above_200k: None,
                        cache_creation_above_200k: None,
                        cache_read_above_200k: None,
                    },
                ),
                (
                    "claude-3-5-haiku-20241022",
                    ModelPrice {
                        input: Some(0.80 / M),
                        output: Some(4.00 / M),
                        cache_creation: Some(1.00 / M),
                        cache_read: Some(0.08 / M),
                        input_above_200k: None,
                        output_above_200k: None,
                        cache_creation_above_200k: None,
                        cache_read_above_200k: None,
                    },
                ),
                (
                    "claude-3-opus-20240229",
                    ModelPrice {
                        input: Some(15.00 / M),
                        output: Some(75.00 / M),
                        cache_creation: Some(18.75 / M),
                        cache_read: Some(1.50 / M),
                        input_above_200k: None,
                        output_above_200k: None,
                        cache_creation_above_200k: None,
                        cache_read_above_200k: None,
                    },
                ),
                (
                    "claude-3-sonnet-20240229",
                    ModelPrice {
                        input: Some(3.00 / M),
                        output: Some(15.00 / M),
                        cache_creation: Some(3.75 / M),
                        cache_read: Some(0.30 / M),
                        input_above_200k: None,
                        output_above_200k: None,
                        cache_creation_above_200k: None,
                        cache_read_above_200k: None,
                    },
                ),
                (
                    "claude-3-haiku-20240307",
                    ModelPrice {
                        input: Some(0.25 / M),
                        output: Some(1.25 / M),
                        cache_creation: Some(0.30 / M),
                        cache_read: Some(0.03 / M),
                        input_above_200k: None,
                        output_above_200k: None,
                        cache_creation_above_200k: None,
                        cache_read_above_200k: None,
                    },
                ),
            ]
        })
        .as_slice()
}

const PROVIDER_PREFIXES: [&str; 4] = ["anthropic/", "claude-", "claude-3-", "claude-3-5-"];

fn normalize_model_name(model_name: &str) -> String {
    let mut normalized = model_name.to_lowercase();
    for prefix in PROVIDER_PREFIXES {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            normalized = stripped.to_string();
        }
    }
    normalized
}

fn get_model_pricing(model_name: &str) -> Option<&'static ModelPrice> {
    let table = model_pricing_table();

    // Direct match
    if let Some((_, p)) = table.iter().find(|(k, _)| *k == model_name) {
        return Some(p);
    }

    // Provider-prefixed match
    for prefix in PROVIDER_PREFIXES {
        let full_name = format!("{}{}", prefix, model_name);
        if let Some((_, p)) = table.iter().find(|(k, _)| *k == full_name) {
            return Some(p);
        }
    }

    // Normalized match
    let normalized = normalize_model_name(model_name);
    if let Some((_, p)) = table
        .iter()
        .find(|(k, _)| normalize_model_name(k) == normalized)
    {
        return Some(p);
    }

    // Fuzzy match
    if let Some((_, p)) = table
        .iter()
        .find(|(k, _)| model_name.contains(*k) || k.contains(model_name))
    {
        return Some(p);
    }

    None
}

fn calculate_tiered_cost(
    total_tokens: i64,
    base_price: Option<f64>,
    tiered_price: Option<f64>,
) -> f64 {
    if total_tokens <= 0 {
        return 0.0;
    }

    if total_tokens > TIERED_THRESHOLD {
        if let Some(tiered) = tiered_price {
            let tokens_below = total_tokens.min(TIERED_THRESHOLD);
            let tokens_above = (total_tokens - TIERED_THRESHOLD).max(0);
            let mut tiered_cost = tokens_above as f64 * tiered;
            if let Some(base) = base_price {
                tiered_cost += tokens_below as f64 * base;
            }
            return tiered_cost;
        }
    }

    if let Some(base) = base_price {
        return total_tokens as f64 * base;
    }

    0.0
}

fn calculate_cost(
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    model: &str,
) -> f64 {
    let pricing = match get_model_pricing(model) {
        Some(p) => p,
        None => return 0.0,
    };

    let input_cost = calculate_tiered_cost(input_tokens, pricing.input, pricing.input_above_200k);
    let output_cost =
        calculate_tiered_cost(output_tokens, pricing.output, pricing.output_above_200k);
    let cache_creation_cost = calculate_tiered_cost(
        cache_creation_tokens,
        pricing.cache_creation,
        pricing.cache_creation_above_200k,
    );
    let cache_read_cost = calculate_tiered_cost(
        cache_read_tokens,
        pricing.cache_read,
        pricing.cache_read_above_200k,
    );

    input_cost + output_cost + cache_creation_cost + cache_read_cost
}

// ===========================================================================
// JSONL parsing (port of usage_service.py)
// ===========================================================================

#[derive(Clone)]
struct LoadedUsageEntry {
    timestamp: DateTime<FixedOffset>,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    cost_usd: Option<f64>,
    model: String,
    session_id: String,
    version: Option<String>,
    project_path: String,
}

fn calculate_entry_cost(e: &LoadedUsageEntry) -> f64 {
    if let Some(c) = e.cost_usd {
        return c;
    }
    calculate_cost(
        e.input_tokens,
        e.output_tokens,
        e.cache_creation_tokens,
        e.cache_read_tokens,
        &e.model,
    )
}

fn json_i64(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(|x| x.as_i64()).unwrap_or(0)
}

/// Port of `UsageService.discover_jsonl_files`.
fn discover_jsonl_files(project_path: Option<&str>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let projects_dir = get_claude_projects_dir();

    if !projects_dir.exists() {
        return files;
    }

    if let Some(pp) = project_path {
        let folder_name = convert_path_to_folder_name(pp);
        let project_folder = projects_dir.join(folder_name);
        if project_folder.exists() {
            if let Ok(rd) = std::fs::read_dir(&project_folder) {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        files.push(p);
                    }
                }
            }
        }
    } else {
        if let Ok(rd) = std::fs::read_dir(&projects_dir) {
            for entry in rd.flatten() {
                let folder = entry.path();
                if folder.is_dir() {
                    if let Ok(inner) = std::fs::read_dir(&folder) {
                        for f in inner.flatten() {
                            let p = f.path();
                            if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                                files.push(p);
                            }
                        }
                    }
                }
            }
        }
    }

    files
}

/// Parse an ISO timestamp, mirroring Python's
/// `datetime.fromisoformat(s.replace("Z", "+00:00"))`. The parsed offset is
/// preserved (Python does not convert to UTC for `strftime`).
fn parse_iso(ts: &str) -> Option<DateTime<FixedOffset>> {
    let normalized = ts.replace('Z', "+00:00");
    DateTime::parse_from_rfc3339(&normalized).ok()
}

/// Port of `UsageService.parse_usage_from_jsonl`.
fn parse_usage_from_jsonl(filepath: &Path) -> Vec<LoadedUsageEntry> {
    let mut entries = Vec::new();
    let project_folder = filepath
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_stem = filepath
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let content = match std::fs::read_to_string(filepath) {
        Ok(c) => c,
        Err(_) => return entries,
    };

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        let obj: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if obj.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }

        let message = obj.get("message").cloned().unwrap_or(Value::Null);
        let usage = message.get("usage").cloned().unwrap_or(Value::Null);

        let usage_is_empty = match &usage {
            Value::Object(m) => m.is_empty(),
            Value::Null => true,
            _ => false,
        };
        if usage_is_empty {
            continue;
        }

        let input_tokens = json_i64(&usage, "input_tokens");
        let output_tokens = json_i64(&usage, "output_tokens");
        let cache_creation = json_i64(&usage, "cache_creation_input_tokens");
        let cache_read = json_i64(&usage, "cache_read_input_tokens");

        if input_tokens == 0 && output_tokens == 0 && cache_creation == 0 && cache_read == 0 {
            continue;
        }

        let timestamp_str = obj.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
        let timestamp = match parse_iso(timestamp_str) {
            Some(t) => t,
            None => continue,
        };

        let model = message
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string();

        let cost_usd = obj.get("costUSD").and_then(|c| c.as_f64());

        let session_id = obj
            .get("sessionId")
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| file_stem.clone());

        let version = obj
            .get("version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        entries.push(LoadedUsageEntry {
            timestamp,
            input_tokens,
            output_tokens,
            cache_creation_tokens: cache_creation,
            cache_read_tokens: cache_read,
            cost_usd,
            model,
            session_id,
            version,
            project_path: project_folder.clone(),
        });
    }

    entries
}

/// Port of `UsageService.get_all_usage_entries`.
fn get_all_usage_entries(project_path: Option<&str>) -> Vec<LoadedUsageEntry> {
    let files = discover_jsonl_files(project_path);
    let mut all_entries = Vec::new();
    for filepath in files {
        all_entries.extend(parse_usage_from_jsonl(&filepath));
    }
    all_entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    all_entries
}

// ===========================================================================
// Aggregation helpers
// ===========================================================================

#[derive(Default, Clone)]
struct Agg {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    cost: f64,
}

/// Port of `_aggregate_model_breakdowns`. Insertion order preserved (Python's
/// `defaultdict` iterates in first-seen order).
fn aggregate_model_breakdowns(entries: &[LoadedUsageEntry]) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, Agg> = std::collections::HashMap::new();

    for e in entries {
        if !map.contains_key(&e.model) {
            order.push(e.model.clone());
            map.insert(e.model.clone(), Agg::default());
        }
        let d = map.get_mut(&e.model).unwrap();
        d.input_tokens += e.input_tokens;
        d.output_tokens += e.output_tokens;
        d.cache_creation_tokens += e.cache_creation_tokens;
        d.cache_read_tokens += e.cache_read_tokens;
        d.cost += calculate_entry_cost(e);
    }

    order
        .into_iter()
        .map(|model| {
            let d = &map[&model];
            json!({
                "model": model,
                "input_tokens": d.input_tokens,
                "output_tokens": d.output_tokens,
                "cache_creation_tokens": d.cache_creation_tokens,
                "cache_read_tokens": d.cache_read_tokens,
                "cost": d.cost,
            })
        })
        .collect()
}

/// `set(e.model ...)` -> insertion-ordered unique list (Python set iteration
/// order is unspecified; frontend treats this as an unordered label list).
fn unique_models(entries: &[LoadedUsageEntry]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for e in entries {
        if seen.insert(e.model.clone()) {
            out.push(e.model.clone());
        }
    }
    out
}

fn unique_versions(entries: &[LoadedUsageEntry]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for e in entries {
        if let Some(v) = &e.version {
            if seen.insert(v.clone()) {
                out.push(v.clone());
            }
        }
    }
    out
}

fn sum_tokens(entries: &[LoadedUsageEntry]) -> (i64, i64, i64, i64) {
    let mut i = 0;
    let mut o = 0;
    let mut cc = 0;
    let mut cr = 0;
    for e in entries {
        i += e.input_tokens;
        o += e.output_tokens;
        cc += e.cache_creation_tokens;
        cr += e.cache_read_tokens;
    }
    (i, o, cc, cr)
}

fn sum_cost(entries: &[LoadedUsageEntry]) -> f64 {
    entries.iter().map(calculate_entry_cost).sum()
}

fn fmt_ymd(dt: &DateTime<FixedOffset>) -> String {
    dt.format("%Y-%m-%d").to_string()
}

fn fmt_ym(dt: &DateTime<FixedOffset>) -> String {
    dt.format("%Y-%m").to_string()
}

// ===========================================================================
// Cache (best-effort port of UsageService cache methods)
//
// The Python cache layer is a pure optimisation that no-ops whenever the DB
// is unavailable (`if not self.db: return None`). The Rust backend has no
// migration that creates the `usage_cache` table, so every query is treated
// as best-effort: any sqlx error (missing table, etc.) is swallowed and the
// data is recomputed — identical observable behaviour to the Python no-DB
// path.
// ===========================================================================

fn get_cache_key(
    cache_type: &str,
    project_path: Option<&str>,
    params: &[(&str, Option<String>)],
) -> String {
    let mut key_parts: Vec<String> = vec![cache_type.to_string()];
    if let Some(pp) = project_path {
        key_parts.push(format!("project:{}", pp));
    }
    let mut sorted: Vec<&(&str, Option<String>)> = params.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (k, v) in sorted {
        if let Some(val) = v {
            key_parts.push(format!("{}:{}", k, val));
        }
    }
    key_parts.join(":")
}

async fn get_from_cache(pool: &sqlx::SqlitePool, cache_key: &str) -> Option<Value> {
    let row: Option<(String, DateTime<Utc>)> =
        sqlx::query_as("SELECT data, cached_at FROM usage_cache WHERE cache_key = ?")
            .bind(cache_key)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    let (data, cached_at) = row?;
    if Utc::now() - cached_at > Duration::minutes(CACHE_TTL_MINUTES) {
        return None;
    }
    serde_json::from_str(&data).ok()
}

async fn save_to_cache(
    pool: &sqlx::SqlitePool,
    cache_key: &str,
    cache_type: &str,
    data: &Value,
    project_path: Option<&str>,
) {
    let serialized = match serde_json::to_string(data) {
        Ok(s) => s,
        Err(_) => return,
    };
    let now = Utc::now();
    let _ = sqlx::query(
        "INSERT INTO usage_cache (cache_key, cache_type, project_path, data, cached_at) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(cache_key) DO UPDATE SET data = excluded.data, cached_at = excluded.cached_at",
    )
    .bind(cache_key)
    .bind(cache_type)
    .bind(project_path)
    .bind(serialized)
    .bind(now)
    .execute(pool)
    .await;
}

async fn invalidate_cache_db(
    pool: &sqlx::SqlitePool,
    cache_type: Option<&str>,
    project_path: Option<&str>,
) {
    let mut query = String::from("DELETE FROM usage_cache WHERE 1=1");
    if cache_type.is_some() {
        query.push_str(" AND cache_type = ?");
    }
    if project_path.is_some() {
        query.push_str(" AND project_path = ?");
    }
    let mut q = sqlx::query(&query);
    if let Some(ct) = cache_type {
        q = q.bind(ct);
    }
    if let Some(pp) = project_path {
        q = q.bind(pp);
    }
    let _ = q.execute(pool).await;
}

// ===========================================================================
// Public computation methods
// ===========================================================================

fn token_counts(i: i64, o: i64, cc: i64, cr: i64) -> Value {
    json!({
        "input_tokens": i,
        "output_tokens": o,
        "cache_creation_tokens": cc,
        "cache_read_tokens": cr,
    })
}

fn compute_summary(project_path: Option<&str>) -> Value {
    let entries = get_all_usage_entries(project_path);

    if entries.is_empty() {
        return json!({
            "total_cost": 0.0,
            "total_input_tokens": 0,
            "total_output_tokens": 0,
            "total_cache_creation_tokens": 0,
            "total_cache_read_tokens": 0,
            "total_tokens": 0,
            "project_count": 0,
            "session_count": 0,
            "models_used": [],
            "date_range_start": Value::Null,
            "date_range_end": Value::Null,
        });
    }

    let total_cost = sum_cost(&entries);
    let (total_input, total_output, total_cache_creation, total_cache_read) = sum_tokens(&entries);
    let total_tokens = total_input + total_output + total_cache_creation + total_cache_read;

    let mut projects = std::collections::HashSet::new();
    let mut sessions = std::collections::HashSet::new();
    for e in &entries {
        projects.insert(e.project_path.clone());
        sessions.insert(format!("{}:{}", e.project_path, e.session_id));
    }
    let models = unique_models(&entries);

    let min_ts = entries.iter().map(|e| e.timestamp).min().unwrap();
    let max_ts = entries.iter().map(|e| e.timestamp).max().unwrap();

    json!({
        "total_cost": total_cost,
        "total_input_tokens": total_input,
        "total_output_tokens": total_output,
        "total_cache_creation_tokens": total_cache_creation,
        "total_cache_read_tokens": total_cache_read,
        "total_tokens": total_tokens,
        "project_count": projects.len(),
        "session_count": sessions.len(),
        "models_used": models,
        "date_range_start": fmt_ymd(&min_ts),
        "date_range_end": fmt_ymd(&max_ts),
    })
}

fn aggregate_by_daily(entries: &[LoadedUsageEntry]) -> Vec<Value> {
    let mut groups: BTreeMap<String, Vec<LoadedUsageEntry>> = BTreeMap::new();
    for e in entries {
        groups
            .entry(fmt_ymd(&e.timestamp))
            .or_default()
            .push(e.clone());
    }

    let mut out = Vec::new();
    for (date, day_entries) in groups.iter().rev() {
        let (i, o, cc, cr) = sum_tokens(day_entries);
        out.push(json!({
            "date": date,
            "input_tokens": i,
            "output_tokens": o,
            "cache_creation_tokens": cc,
            "cache_read_tokens": cr,
            "total_cost": sum_cost(day_entries),
            "models_used": unique_models(day_entries),
            "model_breakdowns": aggregate_model_breakdowns(day_entries),
            "project": Value::Null,
        }));
    }
    out
}

fn aggregate_by_monthly(entries: &[LoadedUsageEntry]) -> Vec<Value> {
    let mut groups: BTreeMap<String, Vec<LoadedUsageEntry>> = BTreeMap::new();
    for e in entries {
        groups
            .entry(fmt_ym(&e.timestamp))
            .or_default()
            .push(e.clone());
    }

    let mut out = Vec::new();
    for (month, month_entries) in groups.iter().rev() {
        let (i, o, cc, cr) = sum_tokens(month_entries);
        out.push(json!({
            "month": month,
            "input_tokens": i,
            "output_tokens": o,
            "cache_creation_tokens": cc,
            "cache_read_tokens": cr,
            "total_cost": sum_cost(month_entries),
            "models_used": unique_models(month_entries),
            "model_breakdowns": aggregate_model_breakdowns(month_entries),
            "project": Value::Null,
        }));
    }
    out
}

fn aggregate_by_session(entries: &[LoadedUsageEntry]) -> Vec<Value> {
    // Preserve first-seen group order (Python defaultdict), then stable-sort by
    // last_activity descending.
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<LoadedUsageEntry>> =
        std::collections::HashMap::new();

    for e in entries {
        let key = format!("{}:{}", e.project_path, e.session_id);
        if !groups.contains_key(&key) {
            order.push(key.clone());
            groups.insert(key.clone(), Vec::new());
        }
        groups.get_mut(&key).unwrap().push(e.clone());
    }

    let mut session_usage: Vec<(String, Value)> = Vec::new();
    for key in &order {
        let session_entries = &groups[key];
        let (project_path, session_id) = match key.split_once(':') {
            Some((p, s)) => (p.to_string(), s.to_string()),
            None => (key.clone(), String::new()),
        };

        let (i, o, cc, cr) = sum_tokens(session_entries);
        let last_entry = session_entries
            .iter()
            .max_by(|a, b| a.timestamp.cmp(&b.timestamp))
            .unwrap();
        let last_activity = fmt_ymd(&last_entry.timestamp);

        let v = json!({
            "session_id": session_id,
            "project_path": project_path,
            "input_tokens": i,
            "output_tokens": o,
            "cache_creation_tokens": cc,
            "cache_read_tokens": cr,
            "total_cost": sum_cost(session_entries),
            "last_activity": last_activity,
            "versions": unique_versions(session_entries),
            "models_used": unique_models(session_entries),
            "model_breakdowns": aggregate_model_breakdowns(session_entries),
        });
        session_usage.push((last_activity, v));
    }

    session_usage.sort_by(|a, b| b.0.cmp(&a.0));
    session_usage.into_iter().map(|(_, v)| v).collect()
}

// ---- Session blocks (5-hour billing periods) ------------------------------

fn floor_to_hour(dt: &DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    dt.with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap()
}

fn create_block(
    start_time: &DateTime<FixedOffset>,
    entries: &[LoadedUsageEntry],
    now: &DateTime<Utc>,
) -> Value {
    let session_duration = Duration::hours(SESSION_DURATION_HOURS);
    let end_time = *start_time + session_duration;

    let last_entry = entries.last();
    let actual_end_time = last_entry.map(|e| e.timestamp).unwrap_or(*start_time);

    let now_fixed = now.with_timezone(actual_end_time.offset());
    let is_active = (now_fixed - actual_end_time) < session_duration
        && now.with_timezone(end_time.offset()) < end_time;

    let (input_tokens, output_tokens, cache_creation, cache_read) = sum_tokens(entries);
    let cost_usd = sum_cost(entries);
    let models = unique_models(entries);

    let mut burn_rate_tokens: Option<f64> = None;
    let mut burn_rate_cost: Option<f64> = None;
    let mut projected_tokens: Option<i64> = None;
    let mut projected_cost: Option<f64> = None;
    let mut remaining_minutes: Option<i64> = None;

    if is_active && entries.len() > 1 {
        let first_entry = &entries[0];
        let last = last_entry.unwrap();
        let duration_minutes =
            (last.timestamp - first_entry.timestamp).num_milliseconds() as f64 / 1000.0 / 60.0;

        if duration_minutes > 0.0 {
            let total_tokens = input_tokens + output_tokens + cache_creation + cache_read;
            let br_tokens = total_tokens as f64 / duration_minutes;
            let br_cost = (cost_usd / duration_minutes) * 60.0;

            let remaining_ms =
                (end_time - now.with_timezone(end_time.offset())).num_milliseconds() as f64;
            let rem_min = ((remaining_ms / (1000.0 * 60.0)) as i64).max(0);

            let projected_additional_tokens = br_tokens * rem_min as f64;
            let proj_tokens = (total_tokens as f64 + projected_additional_tokens) as i64;

            let projected_additional_cost = (br_cost / 60.0) * rem_min as f64;
            let proj_cost = round2(cost_usd + projected_additional_cost);

            burn_rate_tokens = Some(br_tokens);
            burn_rate_cost = Some(br_cost);
            remaining_minutes = Some(rem_min);
            projected_tokens = Some(proj_tokens);
            projected_cost = Some(proj_cost);
        }
    }

    json!({
        "id": py_isoformat(start_time),
        "start_time": py_isoformat(start_time),
        "end_time": py_isoformat(&end_time),
        "actual_end_time": py_isoformat(&actual_end_time),
        "is_active": is_active,
        "is_gap": false,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cache_creation_tokens": cache_creation,
        "cache_read_tokens": cache_read,
        "cost_usd": cost_usd,
        "models": models,
        "burn_rate_tokens_per_minute": burn_rate_tokens,
        "burn_rate_cost_per_hour": burn_rate_cost,
        "projected_total_tokens": projected_tokens,
        "projected_total_cost": projected_cost,
        "remaining_minutes": remaining_minutes,
    })
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Mirror Python `datetime.isoformat()`: no fractional part when microseconds
/// are zero, otherwise exactly 6 fractional digits; offset as `+HH:MM`.
fn py_isoformat(dt: &DateTime<FixedOffset>) -> String {
    let micros = dt.timestamp_subsec_micros();
    if micros == 0 {
        dt.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
    } else {
        format!(
            "{}.{:06}{}",
            dt.format("%Y-%m-%dT%H:%M:%S"),
            micros,
            dt.format("%:z")
        )
    }
}

fn create_gap_block(
    last_activity: &DateTime<FixedOffset>,
    next_activity: &DateTime<FixedOffset>,
) -> Option<Value> {
    let session_duration = Duration::hours(SESSION_DURATION_HOURS);
    let gap_duration = *next_activity - *last_activity;

    if gap_duration <= session_duration {
        return None;
    }

    let gap_start = *last_activity + session_duration;
    let gap_end = *next_activity;

    Some(json!({
        "id": format!("gap-{}", py_isoformat(&gap_start)),
        "start_time": py_isoformat(&gap_start),
        "end_time": py_isoformat(&gap_end),
        "actual_end_time": Value::Null,
        "is_active": false,
        "is_gap": true,
        "input_tokens": 0,
        "output_tokens": 0,
        "cache_creation_tokens": 0,
        "cache_read_tokens": 0,
        "cost_usd": 0.0,
        "models": [],
        "burn_rate_tokens_per_minute": Value::Null,
        "burn_rate_cost_per_hour": Value::Null,
        "projected_total_tokens": Value::Null,
        "projected_total_cost": Value::Null,
        "remaining_minutes": Value::Null,
    }))
}

fn identify_session_blocks(entries: &[LoadedUsageEntry]) -> Vec<Value> {
    if entries.is_empty() {
        return Vec::new();
    }

    let session_duration_ms = (SESSION_DURATION_HOURS * 60 * 60 * 1000) as f64;
    let mut blocks: Vec<Value> = Vec::new();

    let mut sorted_entries = entries.to_vec();
    sorted_entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    let mut current_block_start: Option<DateTime<FixedOffset>> = None;
    let mut current_block_entries: Vec<LoadedUsageEntry> = Vec::new();
    let now = Utc::now();

    for entry in &sorted_entries {
        let entry_time = entry.timestamp;

        if current_block_start.is_none() {
            current_block_start = Some(floor_to_hour(&entry_time));
            current_block_entries = vec![entry.clone()];
        } else {
            let start = current_block_start.unwrap();
            let time_since_start = (entry_time - start).num_milliseconds() as f64;
            let last_entry = current_block_entries.last().cloned();
            let time_since_last = match &last_entry {
                Some(le) => (entry_time - le.timestamp).num_milliseconds() as f64,
                None => 0.0,
            };

            if time_since_start > session_duration_ms || time_since_last > session_duration_ms {
                let block = create_block(&start, &current_block_entries, &now);
                blocks.push(block);

                if let Some(le) = &last_entry {
                    if time_since_last > session_duration_ms {
                        if let Some(gap) = create_gap_block(&le.timestamp, &entry_time) {
                            blocks.push(gap);
                        }
                    }
                }

                current_block_start = Some(floor_to_hour(&entry_time));
                current_block_entries = vec![entry.clone()];
            } else {
                current_block_entries.push(entry.clone());
            }
        }
    }

    if let Some(start) = current_block_start {
        if !current_block_entries.is_empty() {
            blocks.push(create_block(&start, &current_block_entries, &now));
        }
    }

    blocks
}

fn block_start_dt(block: &Value) -> Option<DateTime<FixedOffset>> {
    block
        .get("start_time")
        .and_then(|v| v.as_str())
        .and_then(parse_iso)
}

fn filter_recent_blocks(blocks: Vec<Value>) -> Vec<Value> {
    let now = Utc::now();
    let cutoff = now - Duration::days(DEFAULT_RECENT_DAYS);
    blocks
        .into_iter()
        .filter(|b| {
            let is_active = b
                .get("is_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match block_start_dt(b) {
                Some(dt) => dt.with_timezone(&Utc) >= cutoff || is_active,
                None => is_active,
            }
        })
        .collect()
}

fn compute_blocks(project_path: Option<&str>, recent: bool, active: bool) -> Value {
    let entries = get_all_usage_entries(project_path);
    let mut blocks = identify_session_blocks(&entries);

    if recent {
        blocks = filter_recent_blocks(blocks);
    }
    if active {
        blocks.retain(|b| {
            b.get("is_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        });
    }

    let active_block = blocks
        .iter()
        .find(|b| {
            b.get("is_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .cloned();

    let non_gap: Vec<&Value> = blocks
        .iter()
        .filter(|b| !b.get("is_gap").and_then(|v| v.as_bool()).unwrap_or(false))
        .collect();

    let mut ti = 0i64;
    let mut to = 0i64;
    let mut tcc = 0i64;
    let mut tcr = 0i64;
    let mut total_cost = 0.0;
    for b in &non_gap {
        ti += b.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        to += b.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        tcc += b
            .get("cache_creation_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        tcr += b
            .get("cache_read_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        total_cost += b.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }

    blocks.sort_by(|a, b| {
        let sa = a.get("start_time").and_then(|v| v.as_str()).unwrap_or("");
        let sb = b.get("start_time").and_then(|v| v.as_str()).unwrap_or("");
        sb.cmp(sa)
    });

    json!({
        "data": blocks,
        "active_block": active_block.unwrap_or(Value::Null),
        "totals": token_counts(ti, to, tcc, tcr),
        "total_cost": total_cost,
    })
}

// ---- Date-range filtering (port of get_daily / get_monthly filters) -------

/// Python compares a tz-aware entry timestamp against a naive
/// `datetime.strptime` value. We reproduce the intended boundary semantics by
/// comparing on the entry's naive local datetime (its parsed offset clock
/// reading), which matches the YYYY-MM-DD bucketing.
fn entry_naive(e: &LoadedUsageEntry) -> chrono::NaiveDateTime {
    e.timestamp.naive_local()
}

fn filter_daily_range(
    mut entries: Vec<LoadedUsageEntry>,
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> Vec<LoadedUsageEntry> {
    if let Some(sd) = start_date {
        if let Ok(start) = NaiveDate::parse_from_str(sd, "%Y-%m-%d") {
            let start_dt = start.and_hms_opt(0, 0, 0).unwrap();
            entries.retain(|e| entry_naive(e) >= start_dt);
        }
    }
    if let Some(ed) = end_date {
        if let Ok(end) = NaiveDate::parse_from_str(ed, "%Y-%m-%d") {
            let end_dt = end.and_hms_opt(0, 0, 0).unwrap() + Duration::days(1);
            entries.retain(|e| entry_naive(e) < end_dt);
        }
    }
    entries
}

fn filter_monthly_range(
    mut entries: Vec<LoadedUsageEntry>,
    start_month: Option<&str>,
    end_month: Option<&str>,
) -> Vec<LoadedUsageEntry> {
    if let Some(sm) = start_month {
        if let Ok(start) = NaiveDate::parse_from_str(&format!("{}-01", sm), "%Y-%m-%d") {
            let start_dt = start.and_hms_opt(0, 0, 0).unwrap();
            entries.retain(|e| entry_naive(e) >= start_dt);
        }
    }
    if let Some(em) = end_month {
        if let Ok(end) = NaiveDate::parse_from_str(&format!("{}-01", em), "%Y-%m-%d") {
            let next = if end.month() == 12 {
                NaiveDate::from_ymd_opt(end.year() + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(end.year(), end.month() + 1, 1)
            };
            if let Some(n) = next {
                let end_dt = n.and_hms_opt(0, 0, 0).unwrap();
                entries.retain(|e| entry_naive(e) < end_dt);
            }
        }
    }
    entries
}

// ===========================================================================
// Top-level computation -> response shapes
// ===========================================================================

fn build_daily(
    project_path: Option<&str>,
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> Value {
    let entries = get_all_usage_entries(project_path);
    let entries = filter_daily_range(entries, start_date, end_date);
    let daily_data = aggregate_by_daily(&entries);

    let (mut i, mut o, mut cc, mut cr) = (0i64, 0i64, 0i64, 0i64);
    let mut total_cost = 0.0;
    for d in &daily_data {
        i += d.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        o += d.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        cc += d
            .get("cache_creation_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        cr += d
            .get("cache_read_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        total_cost += d.get("total_cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }

    json!({
        "data": daily_data,
        "totals": token_counts(i, o, cc, cr),
        "total_cost": total_cost,
    })
}

fn build_monthly(
    project_path: Option<&str>,
    start_month: Option<&str>,
    end_month: Option<&str>,
) -> Value {
    let entries = get_all_usage_entries(project_path);
    let entries = filter_monthly_range(entries, start_month, end_month);
    let monthly_data = aggregate_by_monthly(&entries);

    let (mut i, mut o, mut cc, mut cr) = (0i64, 0i64, 0i64, 0i64);
    let mut total_cost = 0.0;
    for m in &monthly_data {
        i += m.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        o += m.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        cc += m
            .get("cache_creation_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        cr += m
            .get("cache_read_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        total_cost += m.get("total_cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }

    json!({
        "data": monthly_data,
        "totals": token_counts(i, o, cc, cr),
        "total_cost": total_cost,
    })
}

fn build_sessions(project_path: Option<&str>, limit: usize) -> Value {
    let entries = get_all_usage_entries(project_path);
    let session_data = aggregate_by_session(&entries);
    let total = session_data.len();
    let session_data: Vec<Value> = session_data.into_iter().take(limit).collect();

    let (mut i, mut o, mut cc, mut cr) = (0i64, 0i64, 0i64, 0i64);
    let mut total_cost = 0.0;
    for s in &session_data {
        i += s.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        o += s.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        cc += s
            .get("cache_creation_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        cr += s
            .get("cache_read_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        total_cost += s.get("total_cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }

    json!({
        "data": session_data,
        "totals": token_counts(i, o, cc, cr),
        "total_cost": total_cost,
        "total": total,
    })
}

// ===========================================================================
// Handlers
// ===========================================================================

#[derive(Deserialize)]
struct SummaryQuery {
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct DailyQuery {
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
}

#[derive(Deserialize)]
struct SessionQuery {
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Deserialize)]
struct MonthlyQuery {
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    start_month: Option<String>,
    #[serde(default)]
    end_month: Option<String>,
}

#[derive(Deserialize)]
struct BlockQuery {
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default = "default_true")]
    recent: bool,
    #[serde(default)]
    active: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct ExportQuery {
    #[serde(default = "default_dataset")]
    dataset: String,
    #[serde(default = "default_format")]
    format: String,
    #[serde(default)]
    project_path: Option<String>,
}

fn default_dataset() -> String {
    "daily".to_string()
}

fn default_format() -> String {
    "json".to_string()
}

#[derive(Deserialize)]
struct InvalidateQuery {
    #[serde(default)]
    cache_type: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
}

fn re_ymd(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[8..10].iter().all(u8::is_ascii_digit)
}

fn re_ym(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 7
        && b[4] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
}

/// GET /api/v1/usage/summary
async fn get_usage_summary(
    State(state): State<ApiState>,
    Query(q): Query<SummaryQuery>,
) -> AppResult<Json<Value>> {
    let pp = q.project_path.as_deref();
    let cache_key = get_cache_key("summary", pp, &[]);

    if let Some(cached) = get_from_cache(&state.pool, &cache_key).await {
        return Ok(Json(json!({ "summary": cached })));
    }

    let summary = compute_summary(pp);

    if summary
        .get("project_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        != 0
        || summary
            .get("total_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            != 0
    {
        save_to_cache(&state.pool, &cache_key, "summary", &summary, pp).await;
    }

    Ok(Json(json!({ "summary": summary })))
}

/// GET /api/v1/usage/daily
async fn get_daily_usage(
    State(state): State<ApiState>,
    Query(q): Query<DailyQuery>,
) -> AppResult<Json<Value>> {
    if let Some(s) = &q.start_date {
        if !re_ymd(s) {
            return Err(AppError::bad_request("Invalid start_date"));
        }
    }
    if let Some(s) = &q.end_date {
        if !re_ymd(s) {
            return Err(AppError::bad_request("Invalid end_date"));
        }
    }

    let pp = q.project_path.as_deref();
    let cache_key = get_cache_key(
        "daily",
        pp,
        &[("start", q.start_date.clone()), ("end", q.end_date.clone())],
    );

    if let Some(cached) = get_from_cache(&state.pool, &cache_key).await {
        return Ok(Json(cached));
    }

    let response = build_daily(pp, q.start_date.as_deref(), q.end_date.as_deref());
    save_to_cache(&state.pool, &cache_key, "daily", &response, pp).await;
    Ok(Json(response))
}

/// GET /api/v1/usage/sessions
async fn get_session_usage(
    State(state): State<ApiState>,
    Query(q): Query<SessionQuery>,
) -> AppResult<Json<Value>> {
    if q.limit < 1 || q.limit > 500 {
        return Err(AppError::bad_request("limit must be between 1 and 500"));
    }

    let pp = q.project_path.as_deref();
    let cache_key = get_cache_key("session", pp, &[("limit", Some(q.limit.to_string()))]);

    if let Some(cached) = get_from_cache(&state.pool, &cache_key).await {
        return Ok(Json(cached));
    }

    let response = build_sessions(pp, q.limit as usize);
    save_to_cache(&state.pool, &cache_key, "session", &response, pp).await;
    Ok(Json(response))
}

/// GET /api/v1/usage/monthly
async fn get_monthly_usage(
    State(state): State<ApiState>,
    Query(q): Query<MonthlyQuery>,
) -> AppResult<Json<Value>> {
    if let Some(s) = &q.start_month {
        if !re_ym(s) {
            return Err(AppError::bad_request("Invalid start_month"));
        }
    }
    if let Some(s) = &q.end_month {
        if !re_ym(s) {
            return Err(AppError::bad_request("Invalid end_month"));
        }
    }

    let pp = q.project_path.as_deref();
    let cache_key = get_cache_key(
        "monthly",
        pp,
        &[
            ("start", q.start_month.clone()),
            ("end", q.end_month.clone()),
        ],
    );

    if let Some(cached) = get_from_cache(&state.pool, &cache_key).await {
        return Ok(Json(cached));
    }

    let response = build_monthly(pp, q.start_month.as_deref(), q.end_month.as_deref());
    save_to_cache(&state.pool, &cache_key, "monthly", &response, pp).await;
    Ok(Json(response))
}

/// GET /api/v1/usage/blocks
async fn get_block_usage(
    State(state): State<ApiState>,
    Query(q): Query<BlockQuery>,
) -> AppResult<Json<Value>> {
    let pp = q.project_path.as_deref();
    let cache_key = get_cache_key(
        "block",
        pp,
        &[
            ("recent", Some(py_bool(q.recent))),
            ("active", Some(py_bool(q.active))),
        ],
    );

    if let Some(cached) = get_from_cache(&state.pool, &cache_key).await {
        return Ok(Json(cached));
    }

    let response = compute_blocks(pp, q.recent, q.active);
    save_to_cache(&state.pool, &cache_key, "block", &response, pp).await;
    Ok(Json(response))
}

/// Mirror Python `str(True)` / `str(False)` used in cache-key construction.
fn py_bool(b: bool) -> String {
    if b {
        "True".to_string()
    } else {
        "False".to_string()
    }
}

const EXPORT_DATASETS: [&str; 5] = ["summary", "daily", "sessions", "monthly", "blocks"];

fn collect_export_rows(dataset: &str, project_path: Option<&str>) -> Vec<Value> {
    match dataset {
        "summary" => vec![compute_summary(project_path)],
        "daily" => build_daily(project_path, None, None)
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "sessions" => build_sessions(project_path, 500)
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "monthly" => build_monthly(project_path, None, None)
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "blocks" => compute_blocks(project_path, false, false)
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Port of `_flatten_for_csv`: list-of-dicts -> JSON string, scalar list ->
/// `|`-joined, None -> "". Header order is first-seen across all rows.
fn flatten_for_csv(rows: &[Value]) -> (Vec<String>, Vec<serde_json::Map<String, Value>>) {
    let mut keys: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut flat: Vec<serde_json::Map<String, Value>> = Vec::new();

    for row in rows {
        let mut out = serde_json::Map::new();
        if let Value::Object(m) = row {
            for (k, v) in m {
                let cell = match v {
                    Value::Array(a) => {
                        if a.first().map(|x| x.is_object()).unwrap_or(false) {
                            Value::String(serde_json::to_string(a).unwrap_or_default())
                        } else {
                            let joined =
                                a.iter().map(scalar_to_py_str).collect::<Vec<_>>().join("|");
                            Value::String(joined)
                        }
                    }
                    Value::Null => Value::String(String::new()),
                    other => other.clone(),
                };
                out.insert(k.clone(), cell);
                if seen.insert(k.clone()) {
                    keys.push(k.clone());
                }
            }
        }
        flat.push(out);
    }

    (keys, flat)
}

fn scalar_to_py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Null => "None".to_string(),
        other => other.to_string(),
    }
}

fn csv_cell(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Null => String::new(),
        other => other.to_string(),
    };
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// GET /api/v1/usage/export
async fn export_usage(Query(q): Query<ExportQuery>) -> Result<Response, AppError> {
    if !EXPORT_DATASETS.contains(&q.dataset.as_str()) {
        let mut sorted = EXPORT_DATASETS.to_vec();
        sorted.sort();
        return Err(AppError::bad_request(format!(
            "dataset must be one of {:?}",
            sorted
        )));
    }
    if q.format != "json" && q.format != "csv" {
        return Err(AppError::bad_request("format must be 'json' or 'csv'"));
    }

    let rows = collect_export_rows(&q.dataset, q.project_path.as_deref());

    let stamp = Utc::now().format("%Y-%m-%d").to_string();
    let filename = format!("claude-usage-{}-{}.{}", q.dataset, stamp, q.format);

    if q.format == "json" {
        let body = serde_json::to_string_pretty(&rows)?;
        return Ok((
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/json".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", filename),
                ),
            ],
            body,
        )
            .into_response());
    }

    // CSV
    let mut body = String::new();
    if !rows.is_empty() {
        let (keys, flat_rows) = flatten_for_csv(&rows);
        body.push_str(
            &keys
                .iter()
                .map(|k| csv_cell(&Value::String(k.clone())))
                .collect::<Vec<_>>()
                .join(","),
        );
        body.push_str("\r\n");
        for row in &flat_rows {
            let line = keys
                .iter()
                .map(|k| csv_cell(row.get(k).unwrap_or(&Value::Null)))
                .collect::<Vec<_>>()
                .join(",");
            body.push_str(&line);
            body.push_str("\r\n");
        }
    }

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        body,
    )
        .into_response())
}

/// POST /api/v1/usage/cache/invalidate
async fn invalidate_cache(
    State(state): State<ApiState>,
    Query(q): Query<InvalidateQuery>,
) -> AppResult<Json<Value>> {
    invalidate_cache_db(
        &state.pool,
        q.cache_type.as_deref(),
        q.project_path.as_deref(),
    )
    .await;
    Ok(Json(
        json!({ "status": "ok", "message": "Cache invalidated" }),
    ))
}
