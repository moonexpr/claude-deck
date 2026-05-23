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
