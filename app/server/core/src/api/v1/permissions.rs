// PORTED: permission_service.py + api/v1/permissions.py
//!
//! Faithful port of `backend_python/app/{services/permission_service.py,
//! api/v1/permissions.py}`. Route paths IDENTICAL to the Python router
//! (`APIRouter(prefix="/permissions")`); the unchanged React frontend was
//! built against them. Rule IDs are RFC 4122 UUIDv5 over `NAMESPACE_DNS`
//! with name `"{scope}-{type}-{pattern}"`, byte-for-byte matching Python's
//! `uuid.uuid5(uuid.NAMESPACE_DNS, ...)`.

use axum::{
    extract::{Json, Path as AxumPath, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::fileio::{read_json_file, write_json_file};
use crate::paths;
use crate::patterns::validate_permission_pattern;

const VALID_PERMISSION_MODES: [&str; 4] = ["default", "acceptEdits", "dontAsk", "plan"];

pub fn router() -> Router<ApiState> {
    // Mirrors Python's `APIRouter(prefix="/permissions")` exactly:
    //   GET    ""              -> list_permissions
    //   POST   ""              -> add_permission (201)
    //   GET    /scope/{scope}  -> list_permissions_by_scope
    //   PUT    /settings       -> update_settings
    //   PUT    /{rule_id}      -> update_permission
    //   DELETE /{rule_id}      -> remove_permission (204)
    Router::new()
        .route("/", get(list_permissions).post(add_permission))
        .route("/scope/{scope}", get(list_permissions_by_scope))
        .route("/settings", put(update_settings))
        .route(
            "/{rule_id}",
            put(update_permission).delete(remove_permission),
        )
}

// ---- request / query types --------------------------------------------------

#[derive(Deserialize)]
struct ProjectPathQuery {
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct ScopeQuery {
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct PermissionRuleCreate {
    #[serde(rename = "type")]
    type_: String,
    pattern: String,
    scope: String,
}

#[derive(Deserialize)]
struct PermissionRuleUpdate {
    #[serde(default, rename = "type")]
    type_: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
}

#[derive(Deserialize)]
struct PermissionSettingsUpdate {
    #[serde(default, rename = "defaultMode")]
    default_mode: Option<String>,
    #[serde(default, rename = "additionalDirectories")]
    additional_directories: Option<Vec<String>>,
    #[serde(default, rename = "disableBypassPermissionsMode")]
    disable_bypass_permissions_mode: Option<bool>,
}

// ---- internal rule model ----------------------------------------------------

#[derive(Clone)]
struct Rule {
    id: String,
    type_: String,
    pattern: String,
    scope: String,
}

impl Rule {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "type": self.type_,
            "pattern": self.pattern,
            "scope": self.scope,
        })
    }
}

// ---- UUIDv5 (RFC 4122, SHA-1 over NAMESPACE_DNS) ----------------------------
//
// Python's `uuid.NAMESPACE_DNS` == 6ba7b810-9dad-11d1-80b4-00c04fd430c8.
// `uuid5` = SHA-1(namespace_bytes || name_utf8); take first 16 bytes, set
// version (5) and variant (RFC 4122) bits, format canonical. Self-contained
// SHA-1 so no extra crate is needed and the digest is byte-exact vs Python.

const NAMESPACE_DNS: [u8; 16] = [
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
];

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let ml: u64 = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&ml.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

fn uuid5_dns(name: &str) -> String {
    let mut input = Vec::with_capacity(16 + name.len());
    input.extend_from_slice(&NAMESPACE_DNS);
    input.extend_from_slice(name.as_bytes());
    let hash = sha1(&input);

    let mut b = [0u8; 16];
    b.copy_from_slice(&hash[0..16]);
    b[6] = (b[6] & 0x0F) | 0x50; // version 5
    b[8] = (b[8] & 0x3F) | 0x80; // RFC 4122 variant

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14],
        b[15]
    )
}

fn rule_id(scope: &str, type_: &str, pattern: &str) -> String {
    uuid5_dns(&format!("{}-{}-{}", scope, type_, pattern))
}

// ---- core: list_permissions -------------------------------------------------

/// Mirrors `PermissionService.list_permissions`. Returns `(rules, settings)`
/// where `settings` is the JSON object shaped like `PermissionSettings`
/// (always present so the response matches `PermissionListResponse`).
fn list_permissions_core(project_path: Option<&str>) -> (Vec<Rule>, Value) {
    let mut rules: Vec<Rule> = Vec::new();

    // PermissionSettings defaults: defaultMode="default",
    // additionalDirectories=None, disableBypassPermissionsMode=False.
    let mut default_mode: Value = json!("default");
    let mut additional_directories: Value = Value::Null;
    let mut disable_bypass: Value = json!(false);

    fn parse_scope(perms: &Map<String, Value>, scope: &str, rules: &mut Vec<Rule>) {
        for kind in ["allow", "ask", "deny"] {
            if let Some(Value::Array(arr)) = perms.get(kind) {
                for p in arr {
                    if let Value::String(pattern) = p {
                        rules.push(Rule {
                            id: rule_id(scope, kind, pattern),
                            type_: kind.to_string(),
                            pattern: pattern.clone(),
                            scope: scope.to_string(),
                        });
                    }
                }
            }
        }
    }

    // User-level.
    let user_path = paths::get_claude_user_settings_file();
    if let Some(user_settings) = read_json_file(&user_path) {
        if let Some(perms) = user_settings.get("permissions").and_then(|v| v.as_object()) {
            if let Some(dm) = perms.get("defaultMode") {
                default_mode = dm.clone();
            }
            if let Some(ad) = perms.get("additionalDirectories") {
                additional_directories = ad.clone();
            }
            if let Some(db) = perms.get("disableBypassPermissionsMode") {
                disable_bypass = db.clone();
            }
            parse_scope(perms, "user", &mut rules);
        }
    }

    // Project-level (overrides / merges user).
    if let Some(pp) = project_path {
        let proj_path = paths::get_project_settings_file(Some(pp));
        if let Some(proj_settings) = read_json_file(&proj_path) {
            if let Some(perms) = proj_settings.get("permissions").and_then(|v| v.as_object()) {
                if let Some(dm) = perms.get("defaultMode") {
                    default_mode = dm.clone();
                }
                if let Some(Value::Array(proj_dirs)) = perms.get("additionalDirectories") {
                    // Python: list(set(user + project)) when user dirs exist,
                    // else project dirs. Set ordering is non-deterministic in
                    // Python; we union preserving first-seen order (the
                    // frontend only displays the list).
                    let proj_strs: Vec<String> = proj_dirs
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    match &additional_directories {
                        Value::Array(user_arr) if !user_arr.is_empty() => {
                            let mut merged: Vec<String> = Vec::new();
                            for v in user_arr {
                                if let Some(s) = v.as_str() {
                                    if !merged.iter().any(|x| x == s) {
                                        merged.push(s.to_string());
                                    }
                                }
                            }
                            for s in &proj_strs {
                                if !merged.iter().any(|x| x == s) {
                                    merged.push(s.clone());
                                }
                            }
                            additional_directories =
                                Value::Array(merged.into_iter().map(Value::String).collect());
                        }
                        _ => {
                            additional_directories = Value::Array(
                                proj_strs.into_iter().map(Value::String).collect(),
                            );
                        }
                    }
                }
                if let Some(db) = perms.get("disableBypassPermissionsMode") {
                    disable_bypass = db.clone();
                }
                parse_scope(perms, "project", &mut rules);
            }
        }
    }

    let settings = json!({
        "defaultMode": default_mode,
        "additionalDirectories": additional_directories,
        "disableBypassPermissionsMode": disable_bypass,
    });

    (rules, settings)
}

fn settings_path_for(
    scope: &str,
    project_path: Option<&str>,
) -> Result<std::path::PathBuf, AppError> {
    if scope == "user" {
        Ok(paths::get_claude_user_settings_file())
    } else {
        match project_path {
            Some(pp) => Ok(paths::get_project_settings_file(Some(pp))),
            None => Err(AppError::bad_request(
                "project_path is required for project scope",
            )),
        }
    }
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/permissions
async fn list_permissions(Query(q): Query<ProjectPathQuery>) -> AppResult<Json<Value>> {
    let (rules, settings) = list_permissions_core(q.project_path.as_deref());
    Ok(Json(json!({
        "rules": rules.iter().map(Rule::to_json).collect::<Vec<_>>(),
        "settings": settings,
    })))
}

/// GET /api/v1/permissions/scope/{scope}
async fn list_permissions_by_scope(
    AxumPath(scope): AxumPath<String>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    let (rules, settings) = list_permissions_core(q.project_path.as_deref());
    let filtered: Vec<Value> = rules
        .iter()
        .filter(|r| r.scope == scope)
        .map(Rule::to_json)
        .collect();
    Ok(Json(json!({ "rules": filtered, "settings": settings })))
}

/// PUT /api/v1/permissions/settings
async fn update_settings(
    Query(q): Query<ScopeQuery>,
    Json(body): Json<PermissionSettingsUpdate>,
) -> AppResult<Json<Value>> {
    let scope = q.scope.clone().unwrap_or_else(|| "user".to_string());

    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    if scope == "project" && q.project_path.is_none() {
        return Err(AppError::bad_request(
            "project_path query parameter is required for project scope",
        ));
    }
    // Endpoint-level mode check (HTTPException 400).
    if let Some(dm) = &body.default_mode {
        if !dm.is_empty() && !VALID_PERMISSION_MODES.contains(&dm.as_str()) {
            return Err(AppError::bad_request(format!(
                "defaultMode must be one of: {}",
                VALID_PERMISSION_MODES.join(", ")
            )));
        }
    }
    // Service-level mode check (ValueError -> 400).
    if let Some(dm) = &body.default_mode {
        if !dm.is_empty() && !VALID_PERMISSION_MODES.contains(&dm.as_str()) {
            return Err(AppError::bad_request(format!(
                "Invalid permission mode: {}. Must be one of: {}",
                dm,
                VALID_PERMISSION_MODES.join(", ")
            )));
        }
    }

    let settings_path = settings_path_for(&scope, q.project_path.as_deref())?;

    let mut settings = read_json_file(&settings_path).unwrap_or_else(|| json!({}));
    if !settings.is_object() {
        settings = json!({});
    }
    {
        let obj = settings.as_object_mut().unwrap();
        if !obj.get("permissions").map(|v| v.is_object()).unwrap_or(false) {
            obj.insert("permissions".to_string(), json!({}));
        }
        let perms = obj
            .get_mut("permissions")
            .and_then(|v| v.as_object_mut())
            .unwrap();
        if let Some(dm) = &body.default_mode {
            perms.insert("defaultMode".to_string(), json!(dm));
        }
        if let Some(ad) = &body.additional_directories {
            perms.insert("additionalDirectories".to_string(), json!(ad));
        }
        if let Some(db) = &body.disable_bypass_permissions_mode {
            perms.insert("disableBypassPermissionsMode".to_string(), json!(db));
        }
    }

    if !write_json_file(&settings_path, &settings).await {
        return Err(AppError::internal(format!(
            "Failed to write settings file: {}",
            settings_path.to_string_lossy()
        )));
    }

    let (_, result_settings) = list_permissions_core(q.project_path.as_deref());
    Ok(Json(result_settings))
}

/// POST /api/v1/permissions  (201 Created)
async fn add_permission(
    Query(q): Query<ProjectPathQuery>,
    Json(rule): Json<PermissionRuleCreate>,
) -> AppResult<impl IntoResponse> {
    if !["allow", "ask", "deny"].contains(&rule.type_.as_str()) {
        return Err(AppError::bad_request(
            "Type must be 'allow', 'ask', or 'deny'",
        ));
    }
    if rule.scope != "user" && rule.scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    if rule.scope == "project" && q.project_path.is_none() {
        return Err(AppError::bad_request(
            "project_path query parameter is required for project scope",
        ));
    }

    let created = add_permission_core(&rule, q.project_path.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(created.to_json())))
}

/// Mirrors `PermissionService.add_permission`. ValueError -> 400, IOError -> 500.
async fn add_permission_core(
    rule: &PermissionRuleCreate,
    project_path: Option<&str>,
) -> Result<Rule, AppError> {
    if !["allow", "ask", "deny"].contains(&rule.type_.as_str()) {
        return Err(AppError::bad_request(format!(
            "Invalid rule type: {}. Must be 'allow', 'ask', or 'deny'",
            rule.type_
        )));
    }
    let (valid, _) = validate_permission_pattern(&rule.pattern);
    if !valid {
        return Err(AppError::bad_request(format!(
            "Invalid pattern format: {}",
            rule.pattern
        )));
    }

    let settings_path = if rule.scope == "user" {
        paths::get_claude_user_settings_file()
    } else if let Some(pp) = project_path {
        paths::get_project_settings_file(Some(pp))
    } else {
        return Err(AppError::bad_request(
            "project_path is required for project scope",
        ));
    };

    let mut settings = read_json_file(&settings_path).unwrap_or_else(|| json!({}));
    if !settings.is_object() {
        settings = json!({});
    }
    {
        let obj = settings.as_object_mut().unwrap();
        if !obj.get("permissions").map(|v| v.is_object()).unwrap_or(false) {
            obj.insert(
                "permissions".to_string(),
                json!({ "allow": [], "ask": [], "deny": [] }),
            );
        }
        let perms = obj
            .get_mut("permissions")
            .and_then(|v| v.as_object_mut())
            .unwrap();
        for kind in ["allow", "ask", "deny"] {
            if !perms.get(kind).map(|v| v.is_array()).unwrap_or(false) {
                perms.insert(kind.to_string(), json!([]));
            }
        }
        let list = perms
            .get_mut(&rule.type_)
            .and_then(|v| v.as_array_mut())
            .unwrap();
        if list
            .iter()
            .any(|v| v.as_str() == Some(rule.pattern.as_str()))
        {
            return Err(AppError::bad_request(format!(
                "Pattern already exists in {} list: {}",
                rule.type_, rule.pattern
            )));
        }
        list.push(json!(rule.pattern));
    }

    if !write_json_file(&settings_path, &settings).await {
        return Err(AppError::internal(format!(
            "Failed to write settings file: {}",
            settings_path.to_string_lossy()
        )));
    }

    Ok(Rule {
        id: rule_id(&rule.scope, &rule.type_, &rule.pattern),
        type_: rule.type_.clone(),
        pattern: rule.pattern.clone(),
        scope: rule.scope.clone(),
    })
}

/// PUT /api/v1/permissions/{rule_id}
async fn update_permission(
    AxumPath(rule_id_param): AxumPath<String>,
    Query(q): Query<ScopeQuery>,
    Json(update): Json<PermissionRuleUpdate>,
) -> AppResult<Json<Value>> {
    let scope = q
        .scope
        .clone()
        .ok_or_else(|| AppError::bad_request("scope query parameter is required"))?;

    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    if scope == "project" && q.project_path.is_none() {
        return Err(AppError::bad_request(
            "project_path query parameter is required for project scope",
        ));
    }
    // Endpoint-level type check (HTTPException 400).
    if let Some(t) = &update.type_ {
        if !t.is_empty() && !["allow", "ask", "deny"].contains(&t.as_str()) {
            return Err(AppError::bad_request(
                "Type must be 'allow', 'ask', or 'deny'",
            ));
        }
    }
    // Service-level type check (ValueError -> 404 per endpoint mapping).
    if let Some(t) = &update.type_ {
        if !t.is_empty() && !["allow", "ask", "deny"].contains(&t.as_str()) {
            return Err(AppError::not_found(format!(
                "Invalid rule type: {}. Must be 'allow', 'ask', or 'deny'",
                t
            )));
        }
    }

    let project_path = q.project_path.as_deref();
    let (rules, _) = list_permissions_core(project_path);
    let existing = rules
        .iter()
        .find(|r| r.id == rule_id_param && r.scope == scope)
        .cloned()
        .ok_or_else(|| {
            AppError::not_found(format!("Permission rule not found: {}", rule_id_param))
        })?;

    // Remove old rule (service ValueError -> 404).
    remove_permission_core(&rule_id_param, &scope, project_path)
        .await
        .map_err(value_error_to_404)?;

    let new_rule = PermissionRuleCreate {
        type_: update
            .type_
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| existing.type_.clone()),
        pattern: update
            .pattern
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| existing.pattern.clone()),
        scope: scope.clone(),
    };

    let created = add_permission_core(&new_rule, project_path)
        .await
        .map_err(value_error_to_404)?;
    Ok(Json(created.to_json()))
}

/// DELETE /api/v1/permissions/{rule_id}  (204 No Content)
async fn remove_permission(
    AxumPath(rule_id_param): AxumPath<String>,
    Query(q): Query<ScopeQuery>,
) -> AppResult<impl IntoResponse> {
    let scope = q
        .scope
        .clone()
        .ok_or_else(|| AppError::bad_request("scope query parameter is required"))?;

    if scope != "user" && scope != "project" {
        return Err(AppError::bad_request("Scope must be 'user' or 'project'"));
    }
    if scope == "project" && q.project_path.is_none() {
        return Err(AppError::bad_request(
            "project_path query parameter is required for project scope",
        ));
    }

    remove_permission_core(&rule_id_param, &scope, q.project_path.as_deref())
        .await
        .map_err(value_error_to_404)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Mirrors `PermissionService.remove_permission`. ValueError signalled as
/// `AppError::bad_request` internally then remapped to 404 by callers; IOError
/// surfaces as 500.
async fn remove_permission_core(
    rule_id_param: &str,
    scope: &str,
    project_path: Option<&str>,
) -> Result<(), AppError> {
    let (rules, _) = list_permissions_core(project_path);
    let existing = rules
        .iter()
        .find(|r| r.id == rule_id_param && r.scope == scope)
        .cloned()
        .ok_or_else(|| {
            AppError::bad_request(format!("Permission rule not found: {}", rule_id_param))
        })?;

    let settings_path = if scope == "user" {
        paths::get_claude_user_settings_file()
    } else if let Some(pp) = project_path {
        paths::get_project_settings_file(Some(pp))
    } else {
        return Err(AppError::bad_request(
            "project_path is required for project scope",
        ));
    };

    let mut settings = read_json_file(&settings_path).unwrap_or_else(|| json!({}));
    if !settings.is_object() {
        settings = json!({});
    }

    let has_type = settings
        .get("permissions")
        .and_then(|p| p.get(&existing.type_))
        .is_some();
    if !settings
        .get("permissions")
        .map(|v| v.is_object())
        .unwrap_or(false)
        || !has_type
    {
        return Err(AppError::bad_request("Permissions not found in settings"));
    }

    {
        let perms = settings
            .get_mut("permissions")
            .and_then(|v| v.as_object_mut())
            .unwrap();
        if let Some(list) = perms.get_mut(&existing.type_).and_then(|v| v.as_array_mut()) {
            if let Some(pos) = list
                .iter()
                .position(|v| v.as_str() == Some(existing.pattern.as_str()))
            {
                list.remove(pos);
            }
        }
    }

    if !write_json_file(&settings_path, &settings).await {
        return Err(AppError::internal(format!(
            "Failed to write settings file: {}",
            settings_path.to_string_lossy()
        )));
    }

    Ok(())
}

// "Permission rule not found" / "Permissions not found" / invalid type /
// invalid pattern / pattern-exists are Python ValueErrors; the update_permission
// and remove_permission endpoints translate ValueError -> HTTP 404. IOErrors
// (write failures) stay 500.
fn value_error_to_404(e: AppError) -> AppError {
    let msg = &e.detail;
    if e.status == StatusCode::BAD_REQUEST
        && (msg.contains("not found")
            || msg.starts_with("Invalid rule type")
            || msg.starts_with("Invalid pattern format")
            || msg.starts_with("Pattern already exists")
            || msg.contains("is required for project scope"))
    {
        AppError::not_found(msg.clone())
    } else {
        e
    }
}
