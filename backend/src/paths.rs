//! Claude Code configuration file locations.
//!
//! Faithful port of `backend_python/app/utils/path_utils.py`. Function names
//! mirror the Python originals so per-module ports map 1:1.

use std::path::PathBuf;

/// User's home directory. Falls back to `/` only if it cannot be determined
/// (matches Python `Path.home()` which would raise; we degrade instead).
pub fn get_user_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Managed (admin-enforced, read-only) settings file, OS-specific.
pub fn get_managed_settings_file() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/ClaudeCode/managed-settings.json")
    } else if cfg!(target_os = "windows") {
        PathBuf::from("C:/ProgramData/ClaudeCode/managed-settings.json")
    } else {
        PathBuf::from("/etc/claude-code/managed-settings.json")
    }
}

/// Managed (admin-enforced, read-only) MCP config file, OS-specific.
pub fn get_managed_mcp_config_file() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/ClaudeCode/managed-mcp.json")
    } else if cfg!(target_os = "windows") {
        PathBuf::from("C:/ProgramData/ClaudeCode/managed-mcp.json")
    } else {
        PathBuf::from("/etc/claude-code/managed-mcp.json")
    }
}

// ---- User-level (~/.claude) -------------------------------------------------

/// `~/.claude/`
pub fn get_claude_user_config_dir() -> PathBuf {
    get_user_home().join(".claude")
}

/// `~/.claude.json`
pub fn get_claude_user_config_file() -> PathBuf {
    get_user_home().join(".claude.json")
}

/// `~/.claude/settings.json`
pub fn get_claude_user_settings_file() -> PathBuf {
    get_claude_user_config_dir().join("settings.json")
}

/// `~/.claude/settings.local.json`
pub fn get_claude_user_settings_local_file() -> PathBuf {
    get_claude_user_config_dir().join("settings.local.json")
}

/// `~/.claude/commands/`
pub fn get_claude_user_commands_dir() -> PathBuf {
    get_claude_user_config_dir().join("commands")
}

/// `~/.claude/agents/`
pub fn get_claude_user_agents_dir() -> PathBuf {
    get_claude_user_config_dir().join("agents")
}

/// `~/.claude/skills/`
pub fn get_claude_user_skills_dir() -> PathBuf {
    get_claude_user_config_dir().join("skills")
}

/// `~/.claude/plugins/`
pub fn get_claude_user_plugins_dir() -> PathBuf {
    get_claude_user_config_dir().join("plugins")
}

/// `~/.claude/plugins/installed_plugins.json`
pub fn get_installed_plugins_file() -> PathBuf {
    get_claude_user_plugins_dir().join("installed_plugins.json")
}

/// `~/.claude/plugins/known_marketplaces.json`
pub fn get_known_marketplaces_file() -> PathBuf {
    get_claude_user_plugins_dir().join("known_marketplaces.json")
}

/// `~/.claude/plugins/marketplaces/`
pub fn get_marketplaces_dir() -> PathBuf {
    get_claude_user_plugins_dir().join("marketplaces")
}

/// `~/.claude/output-styles/`
pub fn get_claude_user_output_styles_dir() -> PathBuf {
    get_claude_user_config_dir().join("output-styles")
}

/// `~/.claude/projects/`
pub fn get_claude_projects_dir() -> PathBuf {
    get_claude_user_config_dir().join("projects")
}

/// `~/.claude/plans/`
pub fn get_claude_plans_dir() -> PathBuf {
    get_claude_user_config_dir().join("plans")
}

// ---- Project-level (.claude in a project dir) -------------------------------

/// `<project>/.claude/` (or `$CWD/.claude` when `project_path` is None).
pub fn get_project_claude_dir(project_path: Option<&str>) -> PathBuf {
    match project_path {
        Some(p) => PathBuf::from(p).join(".claude"),
        None => std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".claude"),
    }
}

pub fn get_project_settings_file(project_path: Option<&str>) -> PathBuf {
    get_project_claude_dir(project_path).join("settings.json")
}

pub fn get_project_settings_local_file(project_path: Option<&str>) -> PathBuf {
    get_project_claude_dir(project_path).join("settings.local.json")
}

pub fn get_project_commands_dir(project_path: Option<&str>) -> PathBuf {
    get_project_claude_dir(project_path).join("commands")
}

pub fn get_project_agents_dir(project_path: Option<&str>) -> PathBuf {
    get_project_claude_dir(project_path).join("agents")
}

pub fn get_project_hooks_dir(project_path: Option<&str>) -> PathBuf {
    get_project_claude_dir(project_path).join("hooks")
}

pub fn get_project_skills_dir(project_path: Option<&str>) -> PathBuf {
    get_project_claude_dir(project_path).join("skills")
}

pub fn get_project_plugins_dir(project_path: Option<&str>) -> PathBuf {
    get_project_claude_dir(project_path).join("plugins")
}

pub fn get_project_output_styles_dir(project_path: Option<&str>) -> PathBuf {
    get_project_claude_dir(project_path).join("output-styles")
}

/// `<project>/.mcp.json` (or `$CWD/.mcp.json`).
pub fn get_project_mcp_config_file(project_path: Option<&str>) -> PathBuf {
    match project_path {
        Some(p) => PathBuf::from(p).join(".mcp.json"),
        None => std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".mcp.json"),
    }
}

/// `<project>/CLAUDE.md` (or `$CWD/CLAUDE.md`).
pub fn get_project_claude_md_file(project_path: Option<&str>) -> PathBuf {
    match project_path {
        Some(p) => PathBuf::from(p).join("CLAUDE.md"),
        None => std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("CLAUDE.md"),
    }
}

// ---- Misc helpers -----------------------------------------------------------

/// Ensure a directory exists, creating parents as needed (best-effort).
pub fn ensure_directory_exists(path: &std::path::Path) {
    let _ = std::fs::create_dir_all(path);
}

/// Convert a Claude project folder name to a display name.
/// `-home-user-projects-foo` -> `foo`
pub fn get_project_display_name(folder_name: &str) -> String {
    let parts: Vec<&str> = folder_name.split('-').collect();
    if parts.len() > 3 {
        parts[parts.len() - 1].to_string()
    } else {
        folder_name.to_string()
    }
}

/// Convert an absolute path to Claude's hyphenated folder format.
/// `/home/juan/projects/foo` -> `-home-juan-projects-foo`
/// `/home/juan/.ssh` -> `-home-juan--ssh`
pub fn convert_path_to_folder_name(absolute_path: &str) -> String {
    absolute_path
        .trim_end_matches('/')
        .replace('/', "-")
        .replace('.', "-")
}
