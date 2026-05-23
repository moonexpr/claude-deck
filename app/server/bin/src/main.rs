use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use server_core::{KeySource, ServerConfig, app, claudecode_ext_core};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env if present (best-effort, not required).
    let _ = dotenvy::dotenv();

    // Init tracing from RUST_LOG env var (defaults to "info").
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // ---- Collect all configuration from environment -------------------------

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        // Default: sqlite file next to the binary's working directory.
        "sqlite:claude_registry.db?mode=rwc".to_owned()
    });

    let projects_dir: PathBuf = std::env::var("PROJECTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Default: ~/.claude/projects
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/"))
                .join(".claude")
                .join("projects")
        });

    let frontend_dist_path: Option<PathBuf> =
        std::env::var("FRONTEND_DIST").ok().map(PathBuf::from);

    let presence_public_url: Option<String> = std::env::var("PRESENCE_PUBLIC_URL").ok();

    // Credential resolution:
    //   CLAUDECODE_EXT_KEY_SOURCE=oauth → KeySource::ClaudeCodeOAuth (spawn
    //       the claudecode_ext framework; no API key needed). Bearer comes
    //       from observing Claude Code's outbound traffic.
    //   otherwise                       → ANTHROPIC_API_KEY env (existing).
    //                                     Absent → KeySource::None (AI proxy
    //                                     handlers return 503).
    let key_source: KeySource = match std::env::var("CLAUDECODE_EXT_KEY_SOURCE")
        .ok()
        .as_deref()
    {
        Some("oauth") => {
            tracing::info!(
                "key_source=oauth — claudecode_ext framework will be spawned at app init",
            );
            KeySource::ClaudeCodeOAuth {
                config: claudecode_ext_core_config_from_env(),
            }
        }
        _ => match std::env::var("ANTHROPIC_API_KEY").ok() {
            Some(k) if !k.is_empty() => KeySource::ApiKey(k),
            _ => KeySource::None,
        },
    };

    let anthropic_base_url: String = std::env::var("ANTHROPIC_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".to_string());

    let enable_external_tools: bool = std::env::var("ENABLE_EXTERNAL_TOOLS")
        .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "false" | "no"))
        .unwrap_or(true);

    // Capture CWD once here so the library never reads it.
    let cwd_fallback: PathBuf =
        std::env::current_dir().context("could not determine current working directory")?;

    // ---- Bind address -------------------------------------------------------

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_owned());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8000);
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .context("invalid HOST/PORT")?;

    // ---- Build and serve ----------------------------------------------------

    tracing::info!(
        db_url = %db_url,
        projects_dir = %projects_dir.display(),
        addr = %addr,
        "starting server"
    );

    let config = ServerConfig {
        db_url,
        projects_dir,
        frontend_dist_path,
        presence_public_url,
        key_source,
        anthropic_base_url,
        enable_external_tools,
        cwd_fallback,
    };

    let router = app(config).await.context("failed to build app")?;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind to {}", addr))?;

    tracing::info!(addr = %addr, "listening");

    axum::serve(listener, router)
        .await
        .context("server error")?;

    Ok(())
}

/// Optional overrides for the claudecode_ext framework, via env vars:
///   CLAUDECODE_EXT_BIND          (default 127.0.0.1:0 — ephemeral)
///   CLAUDECODE_EXT_CA_DIR        (default ~/.claudecode_ext/ca)
///   CLAUDECODE_EXT_DISCOVERY     (default ~/.claudecode_ext/proxy.sock)
fn claudecode_ext_core_config_from_env() -> claudecode_ext_core::Config {
    let mut cfg = claudecode_ext_core::Config::default();
    if let Ok(b) = std::env::var("CLAUDECODE_EXT_BIND")
        && let Ok(parsed) = b.parse()
    {
        cfg.bind = parsed;
    }
    if let Ok(d) = std::env::var("CLAUDECODE_EXT_CA_DIR") {
        cfg.ca_dir = PathBuf::from(d);
    }
    if let Ok(d) = std::env::var("CLAUDECODE_EXT_DISCOVERY") {
        cfg.discovery_path = PathBuf::from(d);
    }
    cfg
}
