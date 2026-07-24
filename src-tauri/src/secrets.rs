//! Secret storage in the OS credential store (Windows Credential Manager via the
//! `keyring` crate). Per the privacy constraints (§6, NFR), tokens and API keys
//! are **never** written to SQLite or plaintext config — only here. SQLite holds
//! a boolean "has a secret" at most.
//!
//! Services namespace the secret kinds: `nexus-mcp` for connector auth tokens
//! (Phase 6), `nexus-cloud` for provider API keys (Phase 7). The `account` is the
//! connector/provider id.

use keyring::Entry;

pub const SERVICE_MCP: &str = "nexus-mcp";
// Used by the BYOK cloud providers in Phase 7.
#[allow(dead_code)]
pub const SERVICE_CLOUD: &str = "nexus-cloud";

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("credential store error: {0}")]
    Keyring(#[from] keyring::Error),
}

/// Store (or replace) a secret for `service`/`account`.
pub fn set_secret(service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
    Entry::new(service, account)?.set_password(secret)?;
    Ok(())
}

/// Fetch a secret, returning `None` if none is stored.
pub fn get_secret(service: &str, account: &str) -> Result<Option<String>, SecretError> {
    match Entry::new(service, account)?.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Remove a secret; succeeds even if there was nothing stored.
pub fn delete_secret(service: &str, account: &str) -> Result<(), SecretError> {
    match Entry::new(service, account)?.delete_password() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Whether a secret exists for `service`/`account`.
pub fn has_secret(service: &str, account: &str) -> bool {
    matches!(get_secret(service, account), Ok(Some(_)))
}
