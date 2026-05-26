//! Credential abstraction for the AI proxy.
//!
//! `ai.rs` handlers no longer reach into a bare `Option<String>` on
//! `ApiState`. Instead they ask a `KeyProvider` for the current
//! `AuthCredential` per request. Two impls land here:
//!
//!   - `ApiKeyProvider`           — returns a static `sk-ant-…` API key
//!     (back-compat with the pre-D7 path; still keychain/env-resolved by
//!     the embedder).
//!   - `ClaudeCodeOAuthProvider`  — wraps an
//!     `Arc<claudecode_ext_core::Handle>` and asks the running
//!     observation framework for the latest observed OAuth bearer.
//!
//! Anthropic's two auth flows differ in header convention:
//!   - API key → `x-api-key: sk-ant-…` + `anthropic-version: 2023-06-01`
//!   - OAuth   → `Authorization: Bearer <jwt>`
//! `AuthCredential` carries the discriminant; `services/ai/anthropic.rs`
//! picks the header based on the variant.

use std::sync::Arc;

use async_trait::async_trait;

/// What gets sent to Anthropic. The discriminant determines the header.
#[derive(Clone)]
pub enum AuthCredential {
    /// `x-api-key: <key>` — the existing pay-per-token flow.
    ApiKey(String),
    /// `Authorization: Bearer <token>` — the Pro/Max subscription flow,
    /// resolved by observing Claude Code's outbound traffic via
    /// `claudecode_ext`.
    Bearer(String),
}

impl std::fmt::Debug for AuthCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthCredential::ApiKey(_) => write!(f, "AuthCredential::ApiKey(<elided>)"),
            AuthCredential::Bearer(_) => write!(f, "AuthCredential::Bearer(<elided>)"),
        }
    }
}

/// Resolves a credential on demand. Async so OAuth providers can refresh
/// without changing the API surface; ApiKey providers just clone.
#[async_trait]
pub trait KeyProvider: Send + Sync {
    /// Returns the currently-usable credential, or `None` if unavailable
    /// (e.g. OAuth has never observed a bearer, or the cached bearer
    /// expired and no new one has been seen).
    async fn current_credential(&self) -> Option<AuthCredential>;

    /// Stable identifier for the diagnostic surface — `"api_key"` or
    /// `"oauth"`. Reported on 503 responses so the UI knows whether to
    /// suggest "set your API key" or "launch Claude Code".
    fn label(&self) -> &'static str;
}

/// Static `sk-ant-…` API key wrapped in the `KeyProvider` shape.
pub struct ApiKeyProvider {
    key: String,
}

impl ApiKeyProvider {
    pub fn new(key: String) -> Self {
        Self { key }
    }
}

#[async_trait]
impl KeyProvider for ApiKeyProvider {
    async fn current_credential(&self) -> Option<AuthCredential> {
        if self.key.is_empty() {
            None
        } else {
            Some(AuthCredential::ApiKey(self.key.clone()))
        }
    }

    fn label(&self) -> &'static str {
        "api_key"
    }
}

/// Wraps a live `claudecode_ext_core::Handle` so AI handlers can ask
/// "what bearer has Claude Code been using?" per request.
pub struct ClaudeCodeOAuthProvider {
    handle: Arc<claudecode_ext_core::Handle>,
}

impl ClaudeCodeOAuthProvider {
    pub fn new(handle: Arc<claudecode_ext_core::Handle>) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl KeyProvider for ClaudeCodeOAuthProvider {
    async fn current_credential(&self) -> Option<AuthCredential> {
        let bearer = self.handle.current_bearer().await?;
        Some(AuthCredential::Bearer(bearer.token))
    }

    fn label(&self) -> &'static str {
        "oauth"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn api_key_provider_returns_api_key_credential() {
        let p = ApiKeyProvider::new("sk-ant-test".to_string());
        match p.current_credential().await {
            Some(AuthCredential::ApiKey(k)) => assert_eq!(k, "sk-ant-test"),
            other => panic!("expected ApiKey, got {other:?}"),
        }
        assert_eq!(p.label(), "api_key");
    }

    #[tokio::test]
    async fn api_key_provider_returns_none_when_empty() {
        let p = ApiKeyProvider::new(String::new());
        assert!(p.current_credential().await.is_none());
    }

    #[test]
    fn debug_elides_credential() {
        let s = format!("{:?}", AuthCredential::ApiKey("secret".to_string()));
        assert!(!s.contains("secret"), "Debug must not leak: {s}");
        let s = format!("{:?}", AuthCredential::Bearer("secret".to_string()));
        assert!(!s.contains("secret"), "Debug must not leak: {s}");
    }
}
