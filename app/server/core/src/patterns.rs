//! Permission pattern validation and migration.
//!
//! Faithful port of `backend_python/app/utils/pattern_utils.py`. Shared by
//! the `config` and `permissions` modules.

use serde_json::{Map, Value};
use std::sync::LazyLock;

/// Maximum length for a permission pattern.
pub const MAX_PATTERN_LENGTH: usize = 500;

static TOOL_NAME_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());
// `(?s)` = DOTALL, matching Python's re.DOTALL on TOOL_ARG_RE.
static TOOL_ARG_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)^([A-Za-z_][A-Za-z0-9_]*)\((.+)\)$").unwrap());
static TOOL_SUBCOMMAND_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*):\*$").unwrap());
static DEPRECATED_COLON_STAR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r":\*$").unwrap());

/// Validate a permission pattern. Returns `(is_valid, error_message)`.
pub fn validate_permission_pattern(pattern: &str) -> (bool, Option<String>) {
    if pattern.trim().is_empty() {
        return (false, Some("Pattern must not be empty".to_string()));
    }
    if pattern.contains('\n') || pattern.contains('\r') {
        return (
            false,
            Some("Pattern must not contain newline characters".to_string()),
        );
    }
    if pattern.len() > MAX_PATTERN_LENGTH {
        return (
            false,
            Some(format!(
                "Pattern exceeds maximum length of {} characters",
                MAX_PATTERN_LENGTH
            )),
        );
    }

    if let Some(caps) = TOOL_ARG_RE.captures(pattern) {
        let tool = caps.get(1).map_or("", |m| m.as_str());
        let arg = caps.get(2).map_or("", |m| m.as_str());
        if tool != "MCP" && DEPRECATED_COLON_STAR_RE.is_match(arg) {
            return (
                false,
                Some(
                    "The :* pattern inside Tool(...) is deprecated. \
Use space-wildcard instead: e.g., Bash(command *) not Bash(command:*)"
                        .to_string(),
                ),
            );
        }
        return (true, None);
    }

    if TOOL_SUBCOMMAND_RE.is_match(pattern) {
        return (true, None);
    }

    if TOOL_NAME_RE.is_match(pattern) {
        return (true, None);
    }

    (false, Some(format!("Invalid pattern format: {}", pattern)))
}

/// Attempt to migrate a deprecated pattern. Returns `None` if not possible.
pub fn migrate_deprecated_pattern(pattern: &str) -> Option<String> {
    if pattern.contains('\n') || pattern.contains('\r') {
        return None;
    }
    if pattern.len() > MAX_PATTERN_LENGTH {
        return None;
    }
    if let Some(caps) = TOOL_ARG_RE.captures(pattern) {
        let tool = caps.get(1).map_or("", |m| m.as_str());
        let arg = caps.get(2).map_or("", |m| m.as_str());
        if tool != "MCP" && DEPRECATED_COLON_STAR_RE.is_match(arg) {
            let migrated_arg = DEPRECATED_COLON_STAR_RE.replace(arg, " *");
            return Some(format!("{}({})", tool, migrated_arg));
        }
    }
    None
}

/// Result of [`sanitize_permission_rules`].
pub struct SanitizeResult {
    /// `{original, migrated, category}` for each auto-migrated pattern.
    pub migrated: Vec<Value>,
    /// `{pattern, category, reason}` for each removed pattern.
    pub removed: Vec<Value>,
    /// The cleaned settings object.
    pub sanitized_settings: Value,
}

/// Validate/sanitize `permissions.{allow,ask,deny}` arrays in a settings dict.
/// Auto-migrates deprecated patterns and drops invalid ones.
pub fn sanitize_permission_rules(settings: &Value) -> SanitizeResult {
    let mut migrated: Vec<Value> = Vec::new();
    let mut removed: Vec<Value> = Vec::new();

    let permissions = match settings.get("permissions") {
        Some(Value::Object(p)) => p.clone(),
        _ => {
            return SanitizeResult {
                migrated,
                removed,
                sanitized_settings: settings.clone(),
            };
        }
    };

    let mut sanitized = match settings {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    let mut new_permissions = permissions.clone();

    for category in ["allow", "ask", "deny"] {
        let rules = match permissions.get(category) {
            Some(Value::Array(a)) => a,
            _ => continue,
        };

        let mut clean_rules: Vec<Value> = Vec::new();
        for pattern in rules {
            let pat = match pattern {
                Value::String(s) => s.clone(),
                other => {
                    removed.push(serde_json::json!({
                        "pattern": other.to_string(),
                        "category": category,
                        "reason": "Pattern is not a string",
                    }));
                    continue;
                }
            };

            let (is_valid, error) = validate_permission_pattern(&pat);
            if is_valid {
                clean_rules.push(Value::String(pat));
                continue;
            }

            if let Some(migrated_pattern) = migrate_deprecated_pattern(&pat) {
                let (ok, _) = validate_permission_pattern(&migrated_pattern);
                if ok {
                    clean_rules.push(Value::String(migrated_pattern.clone()));
                    migrated.push(serde_json::json!({
                        "original": pat,
                        "migrated": migrated_pattern,
                        "category": category,
                    }));
                    continue;
                }
            }

            removed.push(serde_json::json!({
                "pattern": pat,
                "category": category,
                "reason": error.unwrap_or_else(|| "Invalid pattern".to_string()),
            }));
        }

        new_permissions.insert(category.to_string(), Value::Array(clean_rules));
    }

    sanitized.insert("permissions".to_string(), Value::Object(new_permissions));

    SanitizeResult {
        migrated,
        removed,
        sanitized_settings: Value::Object(sanitized),
    }
}
