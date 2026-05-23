// PORTED: plugin_service.py + plugin_descriptions.py + api/v1/plugins.py
//
// Faithful port of the Python plugin module. Routes, request/response field
// names and JSON shapes match the Python API exactly (the unchanged frontend
// was built against them). Paths via `crate::paths`, IO via `crate::fileio`,
// errors via `crate::error::AppError`. The Python DB-backed marketplace
// methods (add/list/sync/browse/remove on the SQLAlchemy `Marketplace`
// model) are intentionally NOT ported: none of the 17 endpoints expose them
// — every marketplace endpoint uses the file-based or `claude plugin ...`
// CLI methods instead. CLI passthrough replicated with `tokio::process`
// using the same `claude` subcommands/args/timeouts as the Python
// `CLIExecutor`.

use axum::{
    Router,
    extract::{Json, Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

use crate::api::v1::ApiState;
use crate::error::{AppError, AppResult};
use crate::fileio::read_json_file;
use crate::paths;

pub fn router() -> Router<ApiState> {
    // Mounted by `mod.rs` via `.nest("/plugins", ...)`, so paths here are
    // relative to that prefix. Final paths therefore equal the Python
    // router's `/plugins/...` exactly. Registration order mirrors Python's
    // APIRouter: static + marketplace routes before the `/{name}`
    // parameterised routes.
    Router::new()
        .route("/", get(list_plugins))
        .route("/marketplaces", get(list_marketplaces))
        .route("/marketplaces", post(add_marketplace))
        .route("/marketplaces/{name}", delete(remove_marketplace))
        .route("/marketplace/{name}/browse", get(browse_marketplace))
        .route(
            "/marketplace/{marketplace_name}/plugin/{plugin_name}",
            get(get_marketplace_plugin_details),
        )
        .route("/marketplace/{name}/update", post(update_marketplace))
        .route(
            "/marketplace/{name}/auto-update",
            put(set_marketplace_auto_update),
        )
        .route("/updates", get(check_plugin_updates))
        .route("/available", get(get_all_available_plugins))
        .route("/validate", post(validate_plugin))
        .route("/update-all", post(update_all_plugins))
        .route("/{name}/update", post(update_plugin))
        .route("/install", post(install_plugin))
        .route("/{name}/toggle", post(toggle_plugin))
        .route("/{name}", get(get_plugin))
        .route("/{name}", delete(uninstall_plugin))
}

// ---- request/query structs --------------------------------------------------

#[derive(Deserialize)]
struct ProjectPathQuery {
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct MarketplaceCreate {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    input: Option<String>,
}

#[derive(Deserialize)]
struct AutoUpdateRequest {
    #[serde(default)]
    enabled: bool,
}

#[derive(Deserialize)]
struct PluginInstallRequest {
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    marketplace_name: Option<String>,
    #[serde(default = "default_scope")]
    #[allow(dead_code)]
    scope: String,
}

fn default_scope() -> String {
    "user".to_string()
}

#[derive(Deserialize)]
struct PluginToggleRequest {
    enabled: bool,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Deserialize)]
struct PluginValidateRequest {
    path: String,
}

// ---- plugin_descriptions.py port -------------------------------------------

/// Returns `(description, usage, examples)` for a known official plugin, or
/// `None`. Mirrors `OFFICIAL_PLUGIN_DESCRIPTIONS` / `get_plugin_info`.
fn get_plugin_info(name: &str) -> Option<(&'static str, &'static str, &'static [&'static str])> {
    let info: &[(&str, &str, &str, &[&str])] = &[
        (
            "document-skills",
            "Comprehensive document creation and manipulation toolkit. Supports creating, editing, and analyzing PDF, DOCX, XLSX, and PPTX files with full formatting support.",
            "Use slash commands like /pdf, /docx, /xlsx, /pptx to work with documents. The plugin provides tools for creating new documents, modifying existing ones, extracting content, and handling forms.",
            &[
                "/pdf - Create or manipulate PDF documents",
                "/docx - Work with Word documents including tracked changes",
                "/xlsx - Create spreadsheets with formulas and formatting",
                "/pptx - Build presentations with layouts and speaker notes",
            ],
        ),
        (
            "context7",
            "Library documentation lookup tool that retrieves up-to-date documentation and code examples for any programming library or framework directly from Context7.",
            "When you need documentation for a library, Claude will automatically use Context7 to fetch current documentation. You can ask about any library's API, usage patterns, or code examples.",
            &[
                "Ask about React hooks usage",
                "Get FastAPI endpoint documentation",
                "Look up Pandas DataFrame methods",
                "Find Next.js routing examples",
            ],
        ),
        (
            "frontend-design",
            "Create distinctive, production-grade frontend interfaces with high design quality. Generates creative, polished React components and UI designs.",
            "Use when building web components, pages, dashboards, or applications. The plugin helps create websites, landing pages, React components, and HTML/CSS layouts with professional styling.",
            &[
                "Build a responsive landing page",
                "Create a dashboard with charts",
                "Design a form with validation",
                "Style a navigation component",
            ],
        ),
        (
            "example-skills",
            "Collection of example skills demonstrating various Claude Code capabilities including document creation, design, and workflow automation.",
            "Reference these as templates when creating custom skills or use them directly for common tasks like algorithmic art, brand guidelines, or MCP server building.",
            &[
                "/algorithmic-art - Create generative art with p5.js",
                "/brand-guidelines - Apply Anthropic brand styling",
                "/mcp-builder - Guide for creating MCP servers",
                "/skill-creator - Create new custom skills",
            ],
        ),
        (
            "commit-commands",
            "Git workflow automation for committing, pushing, and creating pull requests with proper formatting and conventions.",
            "Use slash commands for streamlined git operations. The plugin handles commit message formatting, branch management, and PR creation.",
            &[
                "/commit - Create a well-formatted git commit",
                "/commit-push-pr - Commit, push, and open a PR in one step",
                "/clean_gone - Clean up deleted remote branches",
            ],
        ),
        (
            "code-review",
            "Automated code review for pull requests. Analyzes code changes for bugs, security issues, and adherence to best practices.",
            "Use /code-review with a PR number or URL to get a comprehensive review of the changes including potential issues and improvement suggestions.",
            &[
                "/code-review 123 - Review PR #123",
                "/code-review https://github.com/org/repo/pull/123",
            ],
        ),
        (
            "feature-dev",
            "Guided feature development with codebase understanding and architecture focus. Helps plan and implement new features systematically.",
            "Use /feature-dev to start a guided workflow for implementing new features. The plugin helps understand existing patterns and plan implementation steps.",
            &[
                "/feature-dev - Start guided feature development",
                "Analyze codebase architecture before implementing",
                "Plan implementation with consideration for existing patterns",
            ],
        ),
        (
            "agent-sdk-dev",
            "Tools for building applications with the Claude Agent SDK. Helps create, configure, and verify Agent SDK applications.",
            "Use when building custom Claude agents. The plugin provides templates, verification, and best practices for both Python and TypeScript implementations.",
            &[
                "/new-sdk-app - Create a new Agent SDK application",
                "Verify Agent SDK app configuration",
                "Follow SDK best practices and patterns",
            ],
        ),
        (
            "ralph-wiggum",
            "Loop and iteration technique for complex multi-step tasks. Named after the Simpsons character, this plugin helps manage iterative workflows.",
            "Use for tasks that require multiple iterations or continuous operation. Start a loop session and the plugin manages the iteration state.",
            &[
                "/ralph-loop - Start an iterative loop session",
                "/cancel-ralph - Cancel an active loop",
                "/help - Get help on Ralph Wiggum techniques",
            ],
        ),
        (
            "canvas-design",
            "Create beautiful visual art and designs in PNG and PDF formats using design principles. Ideal for posters, artwork, and static visual pieces.",
            "Use when asked to create posters, artwork, designs, or other static visual pieces. The plugin applies design philosophy to create original visuals.",
            &[
                "Create a poster design",
                "Generate visual artwork",
                "Design infographics",
            ],
        ),
        (
            "algorithmic-art",
            "Create algorithmic and generative art using p5.js with seeded randomness and interactive parameter exploration.",
            "Use for creating generative art, flow fields, particle systems, or any code-based artistic creation with controllable randomness.",
            &[
                "Create a flow field visualization",
                "Generate particle system art",
                "Build interactive generative sketches",
            ],
        ),
        (
            "mcp-builder",
            "Guide for creating high-quality MCP (Model Context Protocol) servers. Helps build servers that enable LLMs to interact with external services.",
            "Use when building MCP servers to integrate external APIs or services. Supports both Python (FastMCP) and Node/TypeScript implementations.",
            &[
                "Create a new MCP server for an API",
                "Design MCP tools for a service",
                "Follow MCP best practices",
            ],
        ),
        (
            "skill-creator",
            "Guide for creating effective custom skills that extend Claude's capabilities with specialized knowledge, workflows, or tool integrations.",
            "Use when you want to create or update a skill that adds new capabilities to Claude Code.",
            &[
                "Create a new custom skill",
                "Update an existing skill",
                "Design skill workflows",
            ],
        ),
        (
            "internal-comms",
            "Resources for writing internal communications including status reports, leadership updates, newsletters, FAQs, and incident reports.",
            "Use when writing any internal communication. The plugin provides templates and formats for various communication types.",
            &[
                "Write a status report",
                "Create a leadership update",
                "Draft an incident report",
                "Compose a project update",
            ],
        ),
        (
            "doc-coauthoring",
            "Structured workflow for co-authoring documentation, proposals, technical specs, and decision documents with iterative refinement.",
            "Use when writing documentation, proposals, or specs. The workflow helps transfer context, refine content, and verify readability.",
            &[
                "Co-author technical documentation",
                "Write a design proposal",
                "Draft a decision document",
            ],
        ),
        (
            "webapp-testing",
            "Toolkit for interacting with and testing local web applications using Playwright. Supports UI verification, debugging, and screenshot capture.",
            "Use to test frontend functionality, debug UI behavior, capture browser screenshots, and view browser logs for local web applications.",
            &[
                "Test a web application UI",
                "Debug frontend behavior",
                "Capture browser screenshots",
                "View browser console logs",
            ],
        ),
    ];

    info.iter()
        .find(|(n, _, _, _)| *n == name)
        .map(|(_, d, u, e)| (*d, *u, *e))
}

// ---- Plugin model builder ---------------------------------------------------

/// Builds a `Plugin` JSON object with the full default schema, matching the
/// Pydantic `Plugin` model field defaults.
struct PluginBuilder {
    name: String,
    version: Value,
    description: Value,
    author: Value,
    category: Value,
    source: Value,
    enabled: bool,
    scope: Value,
    components: Vec<Value>,
    skill_count: i64,
    agent_count: i64,
    hook_count: i64,
    mcp_count: i64,
    lsp_count: i64,
    usage: Value,
    examples: Value,
    readme: Value,
    hooks: Value,
    lsp_configs: Value,
}

impl PluginBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: Value::Null,
            description: Value::Null,
            author: Value::Null,
            category: Value::Null,
            source: Value::Null,
            enabled: true,
            scope: Value::Null,
            components: Vec::new(),
            skill_count: 0,
            agent_count: 0,
            hook_count: 0,
            mcp_count: 0,
            lsp_count: 0,
            usage: Value::Null,
            examples: Value::Null,
            readme: Value::Null,
            hooks: Value::Null,
            lsp_configs: Value::Null,
        }
    }

    fn build(self) -> Value {
        json!({
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "author": self.author,
            "category": self.category,
            "source": self.source,
            "enabled": self.enabled,
            "scope": self.scope,
            "components": self.components,
            "skill_count": self.skill_count,
            "agent_count": self.agent_count,
            "hook_count": self.hook_count,
            "mcp_count": self.mcp_count,
            "lsp_count": self.lsp_count,
            "usage": self.usage,
            "examples": self.examples,
            "readme": self.readme,
            "hooks": self.hooks,
            "lsp_configs": self.lsp_configs,
        })
    }
}

fn component(type_: &str, name: &str, description: Option<String>) -> Value {
    json!({
        "type": type_,
        "name": name,
        "description": description.map(Value::String).unwrap_or(Value::Null),
    })
}

fn str_opt(v: &Value, key: &str) -> Value {
    match v.get(key) {
        Some(Value::String(s)) => Value::String(s.clone()),
        _ => Value::Null,
    }
}

fn get_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Python truthiness for `bool(x)` on dict/list/str/None/number.
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

// ---- filesystem scanning helpers (port of private service methods) ---------

/// `_count_directory_items`: count entries that are dirs or `*.md`.
fn count_directory_items(directory: &Path) -> i64 {
    let Ok(rd) = std::fs::read_dir(directory) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| {
            let p = e.path();
            p.is_dir() || p.extension().and_then(|x| x.to_str()) == Some("md")
        })
        .count() as i64
}

/// `_parse_plugin_hooks`: read `<dir>/hooks/hooks.json` into PluginHook list.
fn parse_plugin_hooks(plugin_dir: &Path) -> Option<Vec<Value>> {
    let hooks_json = plugin_dir.join("hooks").join("hooks.json");
    if !hooks_json.exists() {
        return None;
    }
    let data = read_json_file(&hooks_json)?;
    let mut hooks: Vec<Value> = Vec::new();

    let mk = |event: &str, hook: &Value| {
        json!({
            "event": event,
            "type": hook.get("type").and_then(|v| v.as_str()).unwrap_or("command"),
            "matcher": hook.get("matcher").cloned().unwrap_or(Value::Null),
            "command": hook.get("command").cloned().unwrap_or(Value::Null),
            "prompt": hook.get("prompt").cloned().unwrap_or(Value::Null),
        })
    };

    match &data {
        Value::Object(m) => {
            for (event, hook_list) in m {
                if let Some(arr) = hook_list.as_array() {
                    for hook in arr {
                        hooks.push(mk(event, hook));
                    }
                }
            }
        }
        Value::Array(arr) => {
            for hook in arr {
                let event = hook.get("event").and_then(|v| v.as_str()).unwrap_or("");
                hooks.push(mk(event, hook));
            }
        }
        _ => {}
    }

    if hooks.is_empty() { None } else { Some(hooks) }
}

fn lsp_entry(server: &Value) -> Value {
    json!({
        "name": server.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        "language": server.get("language").and_then(|v| v.as_str()).unwrap_or(""),
        "command": server.get("command").and_then(|v| v.as_str()).unwrap_or(""),
        "args": server.get("args").cloned().unwrap_or(Value::Null),
        "env": server.get("env").cloned().unwrap_or(Value::Null),
    })
}

/// `_parse_lsp_config`: read `<dir>/.lsp.json` (or `.claude-plugin/.lsp.json`).
fn parse_lsp_config(plugin_dir: &Path) -> Option<Vec<Value>> {
    let mut lsp_json = plugin_dir.join(".lsp.json");
    if !lsp_json.exists() {
        lsp_json = plugin_dir.join(".claude-plugin").join(".lsp.json");
        if !lsp_json.exists() {
            return None;
        }
    }
    let data = read_json_file(&lsp_json)?;
    let mut configs: Vec<Value> = Vec::new();

    match &data {
        Value::Object(m) => {
            if let Some(servers) = m.get("servers").and_then(|s| s.as_array()) {
                for server in servers {
                    configs.push(lsp_entry(server));
                }
            } else {
                configs.push(lsp_entry(&data));
            }
        }
        Value::Array(arr) => {
            for server in arr {
                configs.push(lsp_entry(server));
            }
        }
        _ => {}
    }

    if configs.is_empty() {
        None
    } else {
        Some(configs)
    }
}

/// `_read_plugin_readme`: try README in dir or `.claude-plugin/`.
fn read_plugin_readme(plugin_dir: &Path) -> Option<String> {
    let candidates = [
        plugin_dir.join("README.md"),
        plugin_dir.join("readme.md"),
        plugin_dir.join(".claude-plugin").join("README.md"),
        plugin_dir.join(".claude-plugin").join("readme.md"),
    ];
    for p in &candidates {
        if p.exists() {
            if let Ok(s) = std::fs::read_to_string(p) {
                return Some(s);
            }
        }
    }
    None
}

/// `_get_installed_plugins_map`: read `installed_plugins.json` → plugins map.
fn get_installed_plugins_map() -> Map<String, Value> {
    let installed_file = paths::get_installed_plugins_file();
    if !installed_file.exists() {
        return Map::new();
    }
    let Some(data) = read_json_file(&installed_file) else {
        return Map::new();
    };
    match data.get("plugins") {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    }
}

/// `_get_enabled_plugins_from_settings`.
fn get_enabled_plugins_from_settings() -> Vec<Value> {
    let mut plugins: Vec<Value> = Vec::new();

    let settings_file = paths::get_claude_user_settings_file();
    if !settings_file.exists() {
        return plugins;
    }
    let Some(settings_data) = read_json_file(&settings_file) else {
        return plugins;
    };
    let enabled_plugins = match settings_data.get("enabledPlugins") {
        Some(Value::Object(m)) => m.clone(),
        _ => return plugins,
    };

    let installed_map = get_installed_plugins_map();

    for (plugin_key, is_enabled) in &enabled_plugins {
        let (name, source) = match plugin_key.rsplit_once('@') {
            Some((n, s)) => (n.to_string(), s.to_string()),
            None => (plugin_key.clone(), "unknown".to_string()),
        };

        let info = get_plugin_info(&name);

        let install_info = installed_map.get(plugin_key);
        let mut install_path: Option<String> = None;
        let mut version: Value = Value::Null;
        if let Some(Value::Array(arr)) = install_info {
            if let Some(first) = arr.first() {
                install_path = first
                    .get("installPath")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                version = first.get("version").cloned().unwrap_or(Value::Null);
            }
        }

        let mut components: Vec<Value> = Vec::new();
        let mut skill_count: i64 = 0;
        let mut agent_count: i64 = 0;
        let mut hook_count: i64 = 0;
        let mut mcp_count: i64 = 0;
        let mut lsp_count: i64 = 0;
        let mut hooks_v: Value = Value::Null;
        let mut lsp_v: Value = Value::Null;
        let mut readme_v: Value = Value::Null;

        if let Some(ip) = &install_path {
            let plugin_dir = PathBuf::from(ip);
            if plugin_dir.exists() {
                let commands_dir = plugin_dir.join("commands");
                if commands_dir.exists() {
                    if let Ok(rd) = std::fs::read_dir(&commands_dir) {
                        for entry in rd.flatten() {
                            let p = entry.path();
                            if p.extension().and_then(|x| x.to_str()) == Some("md") {
                                skill_count += 1;
                                let stem = p
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                components.push(component(
                                    "command",
                                    &stem,
                                    Some(format!("Command: {}", stem)),
                                ));
                            }
                        }
                    }
                }

                let skills_dir = plugin_dir.join("skills");
                if skills_dir.exists() {
                    if let Ok(rd) = std::fs::read_dir(&skills_dir) {
                        for entry in rd.flatten() {
                            let p = entry.path();
                            let is_dir = p.is_dir();
                            if is_dir || p.extension().and_then(|x| x.to_str()) == Some("md") {
                                skill_count += 1;
                                let n = if is_dir {
                                    p.file_name()
                                        .map(|s| s.to_string_lossy().into_owned())
                                        .unwrap_or_default()
                                } else {
                                    p.file_stem()
                                        .map(|s| s.to_string_lossy().into_owned())
                                        .unwrap_or_default()
                                };
                                components.push(component(
                                    "skill",
                                    &n,
                                    Some(format!("Skill: {}", n)),
                                ));
                            }
                        }
                    }
                }

                let agents_dir = plugin_dir.join("agents");
                if agents_dir.exists() {
                    if let Ok(rd) = std::fs::read_dir(&agents_dir) {
                        for entry in rd.flatten() {
                            let p = entry.path();
                            if p.extension().and_then(|x| x.to_str()) == Some("md") {
                                agent_count += 1;
                                let stem = p
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                components.push(component(
                                    "agent",
                                    &stem,
                                    Some(format!("Agent: {}", stem)),
                                ));
                            }
                        }
                    }
                }

                let mcp_dir = plugin_dir.join("mcp-servers");
                if mcp_dir.exists() {
                    mcp_count = count_directory_items(&mcp_dir);
                }

                if let Some(h) = parse_plugin_hooks(&plugin_dir) {
                    hook_count = h.len() as i64;
                    hooks_v = Value::Array(h);
                }

                if let Some(l) = parse_lsp_config(&plugin_dir) {
                    lsp_count = l.len() as i64;
                    lsp_v = Value::Array(l);
                }

                if let Some(r) = read_plugin_readme(&plugin_dir) {
                    readme_v = Value::String(r);
                }
            }
        }

        let (desc, usage, examples) = match info {
            Some((d, u, e)) => (
                Value::String(d.to_string()),
                Value::String(u.to_string()),
                Value::Array(e.iter().map(|s| Value::String(s.to_string())).collect()),
            ),
            None => (
                Value::String(format!("Plugin from {}", source)),
                Value::Null,
                Value::Null,
            ),
        };

        let mut b = PluginBuilder::new(name);
        b.version = version;
        b.source = Value::String(source);
        b.enabled = is_enabled.as_bool().unwrap_or(false);
        b.description = desc;
        b.usage = usage;
        b.examples = examples;
        b.components = components;
        b.skill_count = skill_count;
        b.agent_count = agent_count;
        b.hook_count = hook_count;
        b.mcp_count = mcp_count;
        b.lsp_count = lsp_count;
        b.hooks = hooks_v;
        b.lsp_configs = lsp_v;
        b.readme = readme_v;
        plugins.push(b.build());
    }

    plugins
}

/// `_scan_plugins_directory`.
fn scan_plugins_directory(plugins_dir: &Path, scope: &str) -> Vec<Value> {
    let mut plugins: Vec<Value> = Vec::new();
    if !plugins_dir.exists() {
        return plugins;
    }
    let Ok(rd) = std::fs::read_dir(plugins_dir) else {
        return plugins;
    };
    for entry in rd.flatten() {
        let plugin_dir = entry.path();
        if !plugin_dir.is_dir() {
            continue;
        }
        let plugin_json_path = plugin_dir.join(".claude-plugin").join("plugin.json");
        if !plugin_json_path.exists() {
            continue;
        }
        let Some(plugin_data) = read_json_file(&plugin_json_path) else {
            // Skip plugins with invalid plugin.json
            continue;
        };

        let mut components: Vec<Value> = Vec::new();
        let mut skill_count: i64 = 0;
        let mut agent_count: i64 = 0;
        let mut hook_count: i64 = 0;
        let mut mcp_count: i64 = 0;
        let mut lsp_count: i64 = 0;

        if let Some(comps) = plugin_data.get("components").and_then(|c| c.as_array()) {
            for comp in comps {
                let comp_type = comp.get("type").and_then(|v| v.as_str()).unwrap_or("");
                components.push(json!({
                    "type": comp_type,
                    "name": comp.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "description": comp.get("description").cloned().unwrap_or(Value::Null),
                }));
                match comp_type {
                    "skill" | "command" => skill_count += 1,
                    "agent" => agent_count += 1,
                    "hook" => hook_count += 1,
                    "mcp" => mcp_count += 1,
                    "lsp" => lsp_count += 1,
                    _ => {}
                }
            }
        }

        skill_count += count_directory_items(&plugin_dir.join("skills"));
        agent_count += count_directory_items(&plugin_dir.join("agents"));
        mcp_count += count_directory_items(&plugin_dir.join("mcp-servers"));

        let hooks = parse_plugin_hooks(&plugin_dir);
        if let Some(h) = &hooks {
            hook_count = h.len() as i64;
        }

        let lsp_configs = parse_lsp_config(&plugin_dir);
        if let Some(l) = &lsp_configs {
            lsp_count = l.len() as i64;
        }

        let readme = read_plugin_readme(&plugin_dir);

        let dir_name = plugin_dir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let mut b = PluginBuilder::new(get_str(&plugin_data, "name").unwrap_or(dir_name));
        b.version = str_opt(&plugin_data, "version");
        b.description = str_opt(&plugin_data, "description");
        b.author = str_opt(&plugin_data, "author");
        b.category = str_opt(&plugin_data, "category");
        b.scope = Value::String(scope.to_string());
        b.components = components;
        b.skill_count = skill_count;
        b.agent_count = agent_count;
        b.hook_count = hook_count;
        b.mcp_count = mcp_count;
        b.lsp_count = lsp_count;
        b.usage = str_opt(&plugin_data, "usage");
        b.examples = plugin_data.get("examples").cloned().unwrap_or(Value::Null);
        b.readme = readme.map(Value::String).unwrap_or(Value::Null);
        b.hooks = hooks.map(Value::Array).unwrap_or(Value::Null);
        b.lsp_configs = lsp_configs.map(Value::Array).unwrap_or(Value::Null);
        plugins.push(b.build());
    }

    plugins
}

/// `list_installed_plugins`.
fn list_installed_plugins(
    project_path: Option<&str>,
    cwd_fallback: &std::path::Path,
) -> Vec<Value> {
    let mut plugins: Vec<Value> = get_enabled_plugins_from_settings();

    let name_of = |p: &Value| {
        p.get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    let user_plugins_dir = paths::get_claude_user_plugins_dir();
    if user_plugins_dir.exists() {
        let local_plugins = scan_plugins_directory(&user_plugins_dir, "user");
        for mut plugin in local_plugins {
            plugin["source"] = Value::String("local".to_string());
            let pname = name_of(&plugin);
            if !plugins.iter().any(|p| name_of(p) == pname) {
                plugins.push(plugin);
            }
        }
    }

    if let Some(pp) = project_path {
        let project_plugins_dir = paths::get_project_plugins_dir(Some(pp), cwd_fallback);
        if project_plugins_dir.exists() {
            let local_plugins = scan_plugins_directory(&project_plugins_dir, "project");
            for mut plugin in local_plugins {
                plugin["source"] = Value::String("local-project".to_string());
                let pname = name_of(&plugin);
                if !plugins.iter().any(|p| name_of(p) == pname) {
                    plugins.push(plugin);
                }
            }
        }
    }

    plugins
}

// ---- marketplace file helpers -----------------------------------------------

fn load_marketplace_auto_update_settings() -> Map<String, Value> {
    let settings_file = paths::get_claude_user_plugins_dir().join("marketplace_settings.json");
    if !settings_file.exists() {
        return Map::new();
    }
    let Some(data) = read_json_file(&settings_file) else {
        return Map::new();
    };
    match data.get("auto_update") {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    }
}

fn list_marketplaces_from_files() -> Vec<Value> {
    let known_file = paths::get_known_marketplaces_file();
    let known_data = read_json_file(&known_file).unwrap_or_else(|| json!({}));
    let Some(known_map) = known_data.as_object() else {
        return Vec::new();
    };

    let auto_update = load_marketplace_auto_update_settings();
    let mut marketplaces: Vec<Value> = Vec::new();

    for (name, info) in known_map {
        let marketplace_json = paths::get_marketplaces_dir()
            .join(name)
            .join(".claude-plugin")
            .join("marketplace.json");
        let marketplace_data = read_json_file(&marketplace_json).unwrap_or_else(|| json!({}));
        let plugin_count = marketplace_data
            .get("plugins")
            .and_then(|p| p.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let repo = info
            .get("source")
            .and_then(|s| s.get("repo"))
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();
        let install_location = info
            .get("installLocation")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let last_updated = info.get("lastUpdated").cloned().unwrap_or(Value::Null);
        let au = auto_update
            .get(name)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        marketplaces.push(json!({
            "name": name,
            "repo": repo,
            "install_location": install_location,
            "last_updated": last_updated,
            "plugin_count": plugin_count,
            "auto_update": au,
        }));
    }

    marketplaces
}

fn browse_marketplace_from_files(name: &str) -> Vec<Value> {
    let marketplace_json = paths::get_marketplaces_dir()
        .join(name)
        .join(".claude-plugin")
        .join("marketplace.json");
    let marketplace_data = read_json_file(&marketplace_json).unwrap_or_else(|| json!({}));
    marketplace_data
        .get("plugins")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default()
}

fn get_all_available_plugins_list() -> Vec<Value> {
    let mut all_plugins: Vec<Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for marketplace in list_marketplaces_from_files() {
        let mname = marketplace
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        for plugin_data in browse_marketplace_from_files(mname) {
            let name = plugin_data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !name.is_empty() && !seen.contains(&name) {
                seen.insert(name.clone());
                let install_command = plugin_data
                    .get("install_command")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("claude plugin install {}", name));
                all_plugins.push(json!({
                    "name": name,
                    "description": plugin_data.get("description").cloned().unwrap_or(Value::Null),
                    "version": plugin_data.get("version").cloned().unwrap_or(Value::Null),
                    "install_command": install_command,
                }));
            }
        }
    }

    all_plugins
}

// ---- CLI executor port ------------------------------------------------------

struct CliResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

/// Port of `CLIExecutor._find_claude_binary` (= `shutil.which("claude")`).
fn find_claude_binary(enable_external_tools: bool) -> Option<PathBuf> {
    if !enable_external_tools {
        return None;
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("claude");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Port of `CLIExecutor.execute` for the `plugin` subcommand. `extra_env` is
/// applied on top of the inherited environment (used by install for the
/// HTTPS-instead-of-SSH git config).
async fn cli_execute(
    args: &[&str],
    timeout_secs: u64,
    extra_env: &[(&str, &str)],
    enable_external_tools: bool,
) -> CliResult {
    let Some(binary) = find_claude_binary(enable_external_tools) else {
        return CliResult {
            stdout: String::new(),
            stderr: "Failed to execute command: Claude CLI binary not found in PATH. Please ensure Claude Code is installed and accessible.".to_string(),
            exit_code: -1,
        };
    };

    let mut cmd = tokio::process::Command::new(&binary);
    cmd.arg("plugin");
    for a in args {
        cmd.arg(a);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return CliResult {
                stdout: String::new(),
                stderr: format!("Failed to execute command: {}", e),
                exit_code: -1,
            };
        }
    };

    let fut = child.wait_with_output();
    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), fut).await {
        Ok(Ok(output)) => CliResult {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
        },
        Ok(Err(e)) => CliResult {
            stdout: String::new(),
            stderr: format!("Failed to execute command: {}", e),
            exit_code: -1,
        },
        Err(_) => CliResult {
            stdout: String::new(),
            stderr: format!("Command timed out after {} seconds", timeout_secs),
            exit_code: -1,
        },
    }
}

/// `_enhance_git_error_message`.
fn enhance_git_error_message(stderr: &str, stdout: &str) -> String {
    let combined = format!("{}\n{}", stderr, stdout).to_lowercase();

    if combined.contains("permission denied") && combined.contains("publickey") {
        return format!(
            "Failed to clone repository: SSH authentication failed.\n\n\
This usually means the plugin repository is private or requires authentication.\n\n\
For private repositories:\n\
\u{2022} Set up SSH keys: Add your SSH public key to GitHub\n\
\u{2022} Or use GitHub CLI: Run 'gh auth login' to authenticate\n\n\
For public repositories:\n\
\u{2022} This should not happen - please report this issue\n\n\
Original error:\n{}",
            stderr
        );
    }

    if combined.contains("could not read from remote repository") {
        return format!(
            "Failed to access remote repository. Please verify:\n\
\u{2022} The repository exists and is accessible\n\
\u{2022} You have the correct access permissions\n\
\u{2022} Your network connection is working\n\n\
Original error:\n{}",
            stderr
        );
    }

    stderr.to_string()
}

// ---- version compare --------------------------------------------------------

#[derive(Clone)]
enum VPart {
    Int(i64),
    Str(String),
}

fn normalize_version(v: &str) -> Vec<VPart> {
    let v = v.trim_start_matches('v');
    v.split('.')
        .map(|part| match part.parse::<i64>() {
            Ok(n) => VPart::Int(n),
            Err(_) => VPart::Str(part.to_string()),
        })
        .collect()
}

/// `_version_compare`: -1 if v1<v2, 0 if equal, 1 if v1>v2.
fn version_compare(v1: &str, v2: &str) -> i32 {
    let p1 = normalize_version(v1);
    let p2 = normalize_version(v2);
    let n = p1.len().max(p2.len());
    for i in 0..n {
        let a = p1.get(i).cloned().unwrap_or(VPart::Int(0));
        let b = p2.get(i).cloned().unwrap_or(VPart::Int(0));
        match (&a, &b) {
            (VPart::Int(x), VPart::Int(y)) => {
                if x < y {
                    return -1;
                } else if x > y {
                    return 1;
                }
            }
            _ => {
                let sa = match &a {
                    VPart::Int(x) => x.to_string(),
                    VPart::Str(s) => s.clone(),
                };
                let sb = match &b {
                    VPart::Int(x) => x.to_string(),
                    VPart::Str(s) => s.clone(),
                };
                if sa < sb {
                    return -1;
                } else if sa > sb {
                    return 1;
                }
            }
        }
    }
    0
}

/// Minimal insertion-ordered map (mirrors Python dict iteration order, with
/// last-write-wins on duplicate keys, key position preserved on overwrite).
struct OrderedMap {
    keys: Vec<String>,
    vals: std::collections::HashMap<String, Value>,
}

impl OrderedMap {
    fn new() -> Self {
        Self {
            keys: Vec::new(),
            vals: std::collections::HashMap::new(),
        }
    }
    fn insert(&mut self, k: String, v: Value) {
        if !self.vals.contains_key(&k) {
            self.keys.push(k.clone());
        }
        self.vals.insert(k, v);
    }
    fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.keys
            .iter()
            .map(move |k| (k, self.vals.get(k).unwrap()))
    }
}

fn check_for_updates(cwd_fallback: &std::path::Path) -> Value {
    let installed = list_installed_plugins(None, cwd_fallback);
    let available = get_all_available_plugins_list();

    let mut available_by_name: std::collections::HashMap<String, Value> =
        std::collections::HashMap::new();
    for p in &available {
        if let Some(n) = p.get("name").and_then(|v| v.as_str()) {
            available_by_name.insert(n.to_string(), p.clone());
        }
    }

    let mut installed_by_name = OrderedMap::new();
    for p in &installed {
        if let Some(n) = p.get("name").and_then(|v| v.as_str()) {
            installed_by_name.insert(n.to_string(), p.clone());
        }
    }

    let mut update_info_list: Vec<Value> = Vec::new();
    for (name, inst) in installed_by_name.iter() {
        let Some(avail) = available_by_name.get(name) else {
            continue;
        };
        let avail_ver = avail.get("version").and_then(|v| v.as_str());
        let inst_ver = inst.get("version").and_then(|v| v.as_str());
        if let (Some(av), Some(iv)) = (avail_ver, inst_ver) {
            if !av.is_empty() && !iv.is_empty() && version_compare(iv, av) < 0 {
                update_info_list.push(json!({
                    "name": name,
                    "installed_version": iv,
                    "latest_version": av,
                    "has_update": true,
                    "source": inst.get("source").cloned().unwrap_or(Value::Null),
                }));
            }
        }
    }

    let count = update_info_list.len();
    json!({
        "plugins": update_info_list,
        "outdated_count": count,
    })
}

// ---- handlers ---------------------------------------------------------------

/// GET /api/v1/plugins
async fn list_plugins(
    State(state): State<ApiState>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let plugins = list_installed_plugins(q.project_path.as_deref(), &state.cwd_fallback);
    Ok(Json(json!({ "plugins": plugins })))
}

/// GET /api/v1/plugins/marketplaces
async fn list_marketplaces() -> AppResult<Json<Value>> {
    Ok(Json(
        json!({ "marketplaces": list_marketplaces_from_files() }),
    ))
}

/// POST /api/v1/plugins/marketplaces  (201)
async fn add_marketplace(
    State(state): State<ApiState>,
    Json(req): Json<MarketplaceCreate>,
) -> AppResult<Response> {
    let _ = (&req.name, &req.url);
    let input = match req.input.as_deref() {
        Some(s) if !s.is_empty() => s,
        _ => return Err(AppError::bad_request("Marketplace input is required")),
    };

    let result = cli_execute(
        &["marketplace", "add", input],
        120,
        &[],
        state.enable_external_tools,
    )
    .await;
    let success = result.exit_code == 0;
    let message = if success {
        result.stdout.clone()
    } else {
        result.stderr.clone()
    };

    if !success {
        return Err(AppError::bad_request(message));
    }

    let body = json!({
        "success": success,
        "message": message,
        "stdout": result.stdout,
        "stderr": result.stderr,
    });
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

/// DELETE /api/v1/plugins/marketplaces/{name}
async fn remove_marketplace(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> AppResult<Json<Value>> {
    let result = cli_execute(
        &["marketplace", "remove", &name],
        60,
        &[],
        state.enable_external_tools,
    )
    .await;
    let success = result.exit_code == 0;
    let message = if success {
        result.stdout.clone()
    } else {
        result.stderr.clone()
    };
    if !success {
        return Err(AppError::bad_request(message));
    }
    Ok(Json(json!({
        "success": success,
        "message": message,
        "stdout": result.stdout,
        "stderr": result.stderr,
    })))
}

/// GET /api/v1/plugins/marketplace/{name}/browse
async fn browse_marketplace(AxumPath(name): AxumPath<String>) -> AppResult<Json<Value>> {
    Ok(Json(
        json!({ "plugins": browse_marketplace_from_files(&name) }),
    ))
}

/// GET /api/v1/plugins/marketplace/{marketplace_name}/plugin/{plugin_name}
async fn get_marketplace_plugin_details(
    AxumPath((marketplace_name, plugin_name)): AxumPath<(String, String)>,
) -> AppResult<Json<Value>> {
    let marketplace_dir = paths::get_marketplaces_dir().join(&marketplace_name);
    let marketplace_json = marketplace_dir
        .join(".claude-plugin")
        .join("marketplace.json");
    let marketplace_data = read_json_file(&marketplace_json).unwrap_or_else(|| json!({}));
    let plugins = marketplace_data
        .get("plugins")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    let plugin_info = plugins
        .iter()
        .find(|p| p.get("name").and_then(|v| v.as_str()) == Some(plugin_name.as_str()))
        .cloned();

    let Some(plugin_info) = plugin_info else {
        return Err(AppError::not_found("Plugin not found in marketplace"));
    };

    let mut source_path = plugin_info
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if let Some(stripped) = source_path.strip_prefix("./") {
        source_path = stripped.to_string();
    }

    let plugin_dir = marketplace_dir.join(&source_path);

    let mut readme_content: Value = Value::Null;
    for readme_name in ["README.md", "readme.md", "README.MD"] {
        let readme_path = plugin_dir.join(readme_name);
        if readme_path.exists() {
            if let Ok(s) = std::fs::read_to_string(&readme_path) {
                readme_content = Value::String(s);
                break;
            }
        }
    }

    let plugin_json_path = plugin_dir.join(".claude-plugin").join("plugin.json");
    let plugin_json_data = read_json_file(&plugin_json_path).unwrap_or_else(|| json!({}));

    let known_file = paths::get_known_marketplaces_file();
    let known_data = read_json_file(&known_file).unwrap_or_else(|| json!({}));
    let repo = known_data
        .get(&marketplace_name)
        .and_then(|m| m.get("source"))
        .and_then(|s| s.get("repo"))
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();

    let mut github_url: Value = Value::Null;
    if !repo.is_empty() {
        let url = if !source_path.is_empty() {
            format!("https://github.com/{}/tree/main/{}", repo, source_path)
        } else {
            format!("https://github.com/{}", repo)
        };
        github_url = Value::String(url);
    }

    let homepage = match plugin_info.get("homepage") {
        Some(Value::String(s)) if !s.is_empty() => Value::String(s.clone()),
        _ => github_url.clone(),
    };

    let mut components: Vec<Value> = Vec::new();
    let commands_dir = plugin_dir.join("commands");
    if commands_dir.exists() {
        if let Ok(rd) = std::fs::read_dir(&commands_dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension().and_then(|x| x.to_str()) == Some("md") {
                    let stem = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    components.push(json!({ "type": "command", "name": stem }));
                }
            }
        }
    }
    let agents_dir = plugin_dir.join("agents");
    if agents_dir.exists() {
        if let Ok(rd) = std::fs::read_dir(&agents_dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension().and_then(|x| x.to_str()) == Some("md") {
                    let stem = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    components.push(json!({ "type": "agent", "name": stem }));
                }
            }
        }
    }
    let skills_dir = plugin_dir.join("skills");
    if skills_dir.exists() {
        if let Ok(rd) = std::fs::read_dir(&skills_dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                let is_dir = p.is_dir();
                if is_dir || p.extension().and_then(|x| x.to_str()) == Some("md") {
                    let n = if is_dir {
                        p.file_name().map(|s| s.to_string_lossy().into_owned())
                    } else {
                        p.file_stem().map(|s| s.to_string_lossy().into_owned())
                    }
                    .unwrap_or_default();
                    components.push(json!({ "type": "skill", "name": n }));
                }
            }
        }
    }

    let has_mcp = plugin_info
        .get("mcpServers")
        .map(is_truthy)
        .unwrap_or(false)
        || plugin_json_data
            .get("mcpServers")
            .map(is_truthy)
            .unwrap_or(false);
    let has_lsp = plugin_info
        .get("lspServers")
        .map(is_truthy)
        .unwrap_or(false)
        || plugin_json_data
            .get("lspServers")
            .map(is_truthy)
            .unwrap_or(false);

    let version = match plugin_info.get("version") {
        Some(v) if is_truthy(v) => v.clone(),
        _ => plugin_json_data
            .get("version")
            .cloned()
            .unwrap_or(Value::Null),
    };
    let author = match plugin_info.get("author") {
        Some(v) if is_truthy(v) => v.clone(),
        _ => plugin_json_data
            .get("author")
            .cloned()
            .unwrap_or(Value::Null),
    };

    Ok(Json(json!({
        "name": plugin_info.get("name").cloned().unwrap_or(Value::Null),
        "description": plugin_info.get("description").cloned().unwrap_or(Value::Null),
        "version": version,
        "author": author,
        "category": plugin_info.get("category").cloned().unwrap_or(Value::Null),
        "homepage": homepage,
        "github_url": github_url,
        "readme": readme_content,
        "components": components,
        "has_mcp": has_mcp,
        "has_lsp": has_lsp,
    })))
}

/// POST /api/v1/plugins/marketplace/{name}/update  (200)
async fn update_marketplace(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> AppResult<Json<Value>> {
    let result = cli_execute(
        &["marketplace", "update", &name],
        120,
        &[],
        state.enable_external_tools,
    )
    .await;
    let success = result.exit_code == 0;
    let message = if success {
        result.stdout.clone()
    } else {
        result.stderr.clone()
    };
    if !success {
        return Err(AppError::bad_request(message));
    }
    Ok(Json(json!({
        "success": success,
        "message": message,
        "stdout": result.stdout,
        "stderr": result.stderr,
    })))
}

/// PUT /api/v1/plugins/marketplace/{name}/auto-update  (200)
async fn set_marketplace_auto_update(
    AxumPath(name): AxumPath<String>,
    Json(req): Json<AutoUpdateRequest>,
) -> AppResult<Json<Value>> {
    let enabled = req.enabled;
    let settings_file = paths::get_claude_user_plugins_dir().join("marketplace_settings.json");
    if let Some(parent) = settings_file.parent() {
        paths::ensure_directory_exists(parent);
    }

    let mut data = read_json_file(&settings_file).unwrap_or_else(|| json!({}));
    if !data.is_object() {
        data = json!({});
    }
    {
        let obj = data.as_object_mut().unwrap();
        let au = obj
            .entry("auto_update".to_string())
            .or_insert_with(|| json!({}));
        if !au.is_object() {
            *au = json!({});
        }
        au.as_object_mut()
            .unwrap()
            .insert(name.clone(), Value::Bool(enabled));
    }

    let success = crate::fileio::write_json_file(&settings_file, &data).await;
    if !success {
        return Err(AppError::internal("Failed to save auto-update setting"));
    }

    Ok(Json(json!({
        "success": true,
        "name": name,
        "auto_update": enabled,
    })))
}

/// GET /api/v1/plugins/updates
async fn check_plugin_updates(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(check_for_updates(&state.cwd_fallback)))
}

/// GET /api/v1/plugins/available
async fn get_all_available_plugins() -> AppResult<Json<Value>> {
    Ok(Json(json!({ "plugins": get_all_available_plugins_list() })))
}

/// POST /api/v1/plugins/validate
async fn validate_plugin(Json(req): Json<PluginValidateRequest>) -> AppResult<Json<Value>> {
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let plugin_path = PathBuf::from(&req.path);
    if !plugin_path.exists() {
        return Ok(Json(json!({
            "valid": false,
            "errors": [format!("Path does not exist: {}", req.path)],
            "warnings": [],
        })));
    }

    let plugin_json_path = plugin_path.join(".claude-plugin").join("plugin.json");
    if !plugin_json_path.exists() {
        errors.push("Missing .claude-plugin/plugin.json".to_string());
    } else {
        match std::fs::read_to_string(&plugin_json_path) {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(plugin_data) => {
                    if !plugin_data.get("name").map(is_truthy).unwrap_or(false) {
                        errors.push("Missing 'name' field in plugin.json".to_string());
                    }
                    if !plugin_data
                        .get("description")
                        .map(is_truthy)
                        .unwrap_or(false)
                    {
                        warnings.push("Missing 'description' field in plugin.json".to_string());
                    }
                    if !plugin_data.get("version").map(is_truthy).unwrap_or(false) {
                        warnings.push("Missing 'version' field in plugin.json".to_string());
                    }
                }
                Err(e) => {
                    errors.push(format!("Invalid JSON in plugin.json: {}", e));
                }
            },
            Err(e) => {
                errors.push(format!("Invalid JSON in plugin.json: {}", e));
            }
        }
    }

    let readme_exists =
        plugin_path.join("README.md").exists() || plugin_path.join("readme.md").exists();
    if !readme_exists {
        warnings.push("Missing README.md".to_string());
    }

    Ok(Json(json!({
        "valid": errors.is_empty(),
        "errors": errors,
        "warnings": warnings,
    })))
}

fn update_plugin_inner(name: &str, result: CliResult) -> Value {
    let success = result.exit_code == 0;
    json!({
        "success": success,
        "message": format!(
            "Plugin '{}' {}",
            name,
            if success { "updated successfully" } else { "update failed" }
        ),
        "stdout": result.stdout,
        "stderr": result.stderr,
    })
}

/// POST /api/v1/plugins/update-all
async fn update_all_plugins(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    let updates = check_for_updates(&state.cwd_fallback);
    let mut results: Vec<Value> = Vec::new();
    let mut updated_count = 0i64;
    let mut failed_count = 0i64;

    if let Some(plugins) = updates.get("plugins").and_then(|p| p.as_array()) {
        for pinfo in plugins {
            let name = pinfo.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let cli = cli_execute(&["update", name], 120, &[], state.enable_external_tools).await;
            let result = update_plugin_inner(name, cli);
            if result
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                updated_count += 1;
            } else {
                failed_count += 1;
            }
            results.push(result);
        }
    }

    Ok(Json(json!({
        "success": failed_count == 0,
        "message": format!("Updated {} plugins, {} failed", updated_count, failed_count),
        "updated_count": updated_count,
        "failed_count": failed_count,
        "results": results,
    })))
}

/// POST /api/v1/plugins/{name}/update
async fn update_plugin(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> AppResult<Json<Value>> {
    let cli = cli_execute(&["update", &name], 120, &[], state.enable_external_tools).await;
    Ok(Json(update_plugin_inner(&name, cli)))
}

/// POST /api/v1/plugins/install
async fn install_plugin(
    State(state): State<ApiState>,
    Json(req): Json<PluginInstallRequest>,
) -> AppResult<Json<Value>> {
    let extra_env = [
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "url.https://github.com/.insteadOf"),
        ("GIT_CONFIG_VALUE_0", "git@github.com:"),
    ];
    let result = cli_execute(
        &["install", &req.name],
        120,
        &extra_env,
        state.enable_external_tools,
    )
    .await;
    let success = result.exit_code == 0;

    let (message, enhanced_stderr) = if success {
        (
            format!("Successfully installed plugin '{}'", req.name),
            result.stderr.clone(),
        )
    } else {
        (
            format!("Failed to install plugin '{}'", req.name),
            enhance_git_error_message(&result.stderr, &result.stdout),
        )
    };

    Ok(Json(json!({
        "success": success,
        "message": message,
        "stdout": result.stdout,
        "stderr": enhanced_stderr,
    })))
}

/// POST /api/v1/plugins/{name}/toggle
async fn toggle_plugin(
    AxumPath(name): AxumPath<String>,
    Json(req): Json<PluginToggleRequest>,
) -> AppResult<Json<Value>> {
    let settings_file = paths::get_claude_user_settings_file();

    let mut settings_data = read_json_file(&settings_file).unwrap_or_else(|| json!({}));
    if !settings_data.is_object() {
        settings_data = json!({});
    }

    {
        let obj = settings_data.as_object_mut().unwrap();
        if !obj
            .get("enabledPlugins")
            .map(|v| v.is_object())
            .unwrap_or(false)
        {
            obj.insert("enabledPlugins".to_string(), json!({}));
        }
    }

    let plugin_key = if let Some(src) = req.source.as_deref() {
        format!("{}@{}", name, src)
    } else {
        let enabled_obj = settings_data
            .get("enabledPlugins")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let mut existing_key: Option<String> = None;
        for key in enabled_obj.keys() {
            if key == &name || key.starts_with(&format!("{}@", name)) {
                existing_key = Some(key.clone());
                break;
            }
        }
        existing_key.unwrap_or_else(|| name.clone())
    };

    settings_data["enabledPlugins"][&plugin_key] = Value::Bool(req.enabled);

    let success = crate::fileio::write_json_file(&settings_file, &settings_data).await;
    if !success {
        return Ok(Json(json!({
            "success": false,
            "message": "Failed to write settings file",
            "plugin": Value::Null,
        })));
    }

    let info = get_plugin_info(&name);
    let (desc, usage, examples) = match info {
        Some((d, u, e)) => (
            Value::String(d.to_string()),
            Value::String(u.to_string()),
            Value::Array(e.iter().map(|s| Value::String(s.to_string())).collect()),
        ),
        None => (Value::Null, Value::Null, Value::Null),
    };

    let mut b = PluginBuilder::new(name.clone());
    b.source = Value::String(req.source.clone().unwrap_or_else(|| "unknown".to_string()));
    b.enabled = req.enabled;
    b.description = desc;
    b.usage = usage;
    b.examples = examples;
    let plugin = b.build();

    Ok(Json(json!({
        "success": true,
        "message": format!(
            "Plugin '{}' {} successfully",
            name,
            if req.enabled { "enabled" } else { "disabled" }
        ),
        "plugin": plugin,
    })))
}

/// GET /api/v1/plugins/{name}
async fn get_plugin(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
    Query(q): Query<ProjectPathQuery>,
) -> AppResult<Json<Value>> {
    let user_plugins_dir = paths::get_claude_user_plugins_dir();
    let mut plugin_path = user_plugins_dir
        .join(&name)
        .join(".claude-plugin")
        .join("plugin.json");

    if !plugin_path.exists() {
        if let Some(pp) = q.project_path.as_deref() {
            let project_plugins_dir = paths::get_project_plugins_dir(Some(pp), &state.cwd_fallback);
            plugin_path = project_plugins_dir
                .join(&name)
                .join(".claude-plugin")
                .join("plugin.json");
        }
    }

    if !plugin_path.exists() {
        return Err(AppError::not_found(format!("Plugin '{}' not found", name)));
    }

    let Some(plugin_data) = read_json_file(&plugin_path) else {
        return Err(AppError::not_found(format!("Plugin '{}' not found", name)));
    };

    let mut components: Vec<Value> = Vec::new();
    if let Some(comps) = plugin_data.get("components").and_then(|c| c.as_array()) {
        for comp in comps {
            components.push(json!({
                "type": comp.get("type").and_then(|v| v.as_str()).unwrap_or(""),
                "name": comp.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "description": Value::Null,
            }));
        }
    }

    let mut b = PluginBuilder::new(get_str(&plugin_data, "name").unwrap_or_else(|| name.clone()));
    b.version = str_opt(&plugin_data, "version");
    b.description = str_opt(&plugin_data, "description");
    b.author = str_opt(&plugin_data, "author");
    b.category = str_opt(&plugin_data, "category");
    b.components = components;
    Ok(Json(b.build()))
}

/// DELETE /api/v1/plugins/{name}  (204)
async fn uninstall_plugin(
    AxumPath(name): AxumPath<String>,
    Query(_q): Query<ProjectPathQuery>,
) -> AppResult<Response> {
    let mut removed_any = false;
    let mut matching_key: Option<String> = None;

    let installed_plugins_file = paths::get_installed_plugins_file();
    if installed_plugins_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&installed_plugins_file) {
            if let Ok(mut data) = serde_json::from_str::<Value>(&content) {
                let plugins_obj = data
                    .get("plugins")
                    .and_then(|v| v.as_object())
                    .cloned()
                    .unwrap_or_default();

                for key in plugins_obj.keys() {
                    if key == &name || key.starts_with(&format!("{}@", name)) {
                        matching_key = Some(key.clone());
                        break;
                    }
                }

                if let Some(mk) = &matching_key {
                    if let Some(entries) = plugins_obj.get(mk).and_then(|v| v.as_array()) {
                        for entry in entries {
                            if let Some(install_path) =
                                entry.get("installPath").and_then(|v| v.as_str())
                            {
                                let plugin_dir = PathBuf::from(install_path);
                                if plugin_dir.exists()
                                    && std::fs::remove_dir_all(&plugin_dir).is_ok()
                                {
                                    removed_any = true;
                                }
                            }
                        }
                    }

                    if let Some(pmap) = data.get_mut("plugins").and_then(|v| v.as_object_mut()) {
                        pmap.remove(mk);
                    }
                    if let Ok(serialized) = serde_json::to_string_pretty(&data) {
                        if std::fs::write(&installed_plugins_file, serialized).is_ok() {
                            removed_any = true;
                        }
                    }
                }
            }
        }
    }

    // ALWAYS try to remove from settings.json (enabledPlugins)
    let settings_file = paths::get_claude_user_settings_file();
    if settings_file.exists() {
        if let Some(mut settings_data) = read_json_file(&settings_file) {
            if !settings_data.is_object() {
                settings_data = json!({});
            }
            let enabled = settings_data
                .get("enabledPlugins")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();

            let keys_to_remove: Vec<String> = enabled
                .keys()
                .filter(|k| {
                    *k == &name
                        || Some(*k) == matching_key.as_ref()
                        || k.starts_with(&format!("{}@", name))
                })
                .cloned()
                .collect();

            if !keys_to_remove.is_empty() {
                if let Some(ep) = settings_data
                    .get_mut("enabledPlugins")
                    .and_then(|v| v.as_object_mut())
                {
                    for k in &keys_to_remove {
                        ep.remove(k);
                    }
                }
                if crate::fileio::write_json_file(&settings_file, &settings_data).await {
                    removed_any = true;
                }
            }
        }
    }

    if !removed_any {
        return Err(AppError::not_found(format!("Plugin '{}' not found", name)));
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}
