use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Manager, State};

mod keychain;

/// Managed state: the base URL of the in-process embedded server.
pub struct ServerBaseUrl(pub Arc<String>);

/// Tauri command: return the embedded server base URL to the webview.
#[tauri::command]
fn server_base_url(state: State<'_, ServerBaseUrl>) -> String {
    (*state.0).clone()
}

/// Build the Tauri app. Called from main.rs.
///
/// The embedded server is started in Tauri's setup hook:
///  1. Bind a std TcpListener on 127.0.0.1:0 synchronously so the OS assigns
///     an ephemeral port before the first window loads.
///  2. Read back the port via local_addr().
///  3. Spawn a Tauri async task that hands the listener to Tokio, builds
///     server-core's Router, and runs axum::serve.
///  4. Store the base URL in managed state for the `server_base_url` command.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // ---- Step 1: bind synchronously on an OS-assigned port ----------
            let std_listener =
                TcpListener::bind("127.0.0.1:0").expect("failed to bind embedded server listener");
            std_listener
                .set_nonblocking(true)
                .expect("failed to set non-blocking on listener");
            let port = std_listener
                .local_addr()
                .expect("failed to read local_addr")
                .port();

            let base_url = format!("http://127.0.0.1:{}", port);

            // ---- Step 2: store base URL in managed state --------------------
            app.manage(ServerBaseUrl(Arc::new(base_url.clone())));

            // ---- Step 3: resolve paths for ServerConfig ---------------------
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app_data_dir");
            if !app_data_dir.exists() {
                std::fs::create_dir_all(&app_data_dir).expect("failed to create app_data_dir");
            }

            let db_path = app_data_dir.join("claude_registry.db");
            let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

            let projects_dir: PathBuf = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/"))
                .join(".claude")
                .join("projects");

            let cwd_fallback = app_data_dir.clone();

            // D8: Credential auto-detect.
            //   1. If the user has a (claude-deck, anthropic_api_key) entry
            //      in the keychain → use it.
            //   2. Else if Claude Code itself is logged in (probed via
            //      `security find-generic-password -s 'Claude Code-credentials'`
            //      which does NOT trigger Touch ID since the value isn't read)
            //      → spawn the claudecode_ext framework and observe Claude
            //      Code's bearer.
            //   3. Else → AI proxy returns 503 until the user configures one.
            //
            // Explicit override via a Tauri settings UI lands later; for v1
            // the auto-fallback matches the issue #4 acceptance ("Tauri build
            // picks up the same OAuth creds when no anthropic_api_key
            // keychain entry exists").
            let key_source = if let Some(k) = keychain::read_anthropic_key() {
                server_core::KeySource::ApiKey(k)
            } else if keychain::claude_code_oauth_present() {
                tracing::info!(
                    "no claude-deck API key; Claude Code OAuth detected — using claudecode_ext",
                );
                server_core::KeySource::ClaudeCodeOAuth {
                    config: server_core::claudecode_ext_core::Config::default(),
                }
            } else {
                server_core::KeySource::None
            };

            let config = server_core::ServerConfig {
                db_url,
                projects_dir,
                frontend_dist_path: None, // Tauri serves the UI; server is API-only
                presence_public_url: None,
                key_source,
                anthropic_base_url: "https://api.anthropic.com".to_string(),
                enable_external_tools: true,
                cwd_fallback,
            };

            // ---- Step 4: spawn the server on the Tauri async runtime --------
            tauri::async_runtime::spawn(async move {
                let router = server_core::app(config)
                    .await
                    .expect("failed to build server-core router");

                // Convert std listener → tokio listener
                let tokio_listener = tokio::net::TcpListener::from_std(std_listener)
                    .expect("failed to convert std listener to tokio listener");

                tracing::info!(base_url = %base_url, "embedded server listening");

                axum::serve(tokio_listener, router)
                    .await
                    .expect("embedded server error");
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![server_base_url])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
