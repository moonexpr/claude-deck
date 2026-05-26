/// Read the Anthropic API key from the OS keychain.
///
/// Service: `"claude-deck"`, key: `"anthropic_api_key"`.
///
/// Provision with:
///   security add-generic-password -s claude-deck -a anthropic_api_key -w 'sk-ant-…'
///
/// Any error (entry absent, no backend, permission denied) returns `None`.
/// The key value is never logged.
pub fn read_anthropic_key() -> Option<String> {
    match keyring::Entry::new("claude-deck", "anthropic_api_key") {
        Ok(entry) => match entry.get_password() {
            Ok(key) if !key.is_empty() => {
                tracing::debug!("keychain: anthropic_api_key resolved");
                Some(key)
            }
            Ok(_) => {
                tracing::debug!("keychain: anthropic_api_key entry is empty — key not configured");
                None
            }
            Err(keyring::Error::NoEntry) => {
                tracing::debug!("keychain: anthropic_api_key not configured");
                None
            }
            Err(e) => {
                tracing::debug!("keychain: anthropic_api_key read error: {e} — key not configured");
                None
            }
        },
        Err(e) => {
            tracing::debug!("keychain: failed to open entry: {e} — key not configured");
            None
        }
    }
}

/// Probe whether a Claude Code OAuth credential entry exists in the keychain
/// **without reading its value** (so no Touch ID / password prompt fires).
///
/// macOS only — uses the `security` command's metadata lookup, which exits 0
/// when the entry exists and 44 (errSecItemNotFound) when it doesn't. The
/// value is never extracted, so the system never asks the user to authorize
/// access to Claude Code's keychain item.
///
/// Used by the Tauri build to auto-fall-back to `KeySource::ClaudeCodeOAuth`
/// when no claude-deck API key is configured but Claude Code itself is
/// logged in. Returns `false` on non-macOS platforms (where `security` is
/// absent) — those platforms still need explicit configuration.
pub fn claude_code_oauth_present() -> bool {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", "Claude Code-credentials"])
            // Suppress stdout (metadata dump) and stderr (errSec…) — we only
            // care about the exit code. Both go to /dev/null.
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => {
                tracing::debug!("keychain: Claude Code-credentials present");
                true
            }
            Ok(_) => {
                tracing::debug!("keychain: Claude Code-credentials absent");
                false
            }
            Err(e) => {
                tracing::debug!("keychain: failed to invoke security: {e}");
                false
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
