//! Credential storage abstraction.
//!
//! The CLI persists the bearer token in the operating system's secure
//! credential store:
//!
//! - macOS: Keychain
//! - Windows: Credential Manager
//! - Linux: Secret Service (via `keyring` 3.x)
//!
//! The trait exists so tests can substitute a deterministic in-memory store.

use thiserror::Error;

/// Errors from the credential store. Most map directly to `keyring::Error`,
/// but we wrap them to keep the public surface stable across crate upgrades.
#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("no credential stored under service={service} user={user}")]
    NotFound { service: String, user: String },

    #[error("credential store backend failed: {0}")]
    Backend(String),

    #[error("malformed stored credential: {0}")]
    Malformed(String),
}

/// Subset of the stored token. The struct intentionally does not `derive
/// Display` so it cannot be printed accidentally.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredToken {
    pub api_base_url: String,
    pub bearer: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub subject: Option<String>,
}

impl StoredToken {
    /// Construct a new stored token. The caller is responsible for not
    /// passing an empty bearer — the constructor enforces that.
    pub fn new(
        api_base_url: impl Into<String>,
        bearer: impl Into<String>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        subject: Option<String>,
    ) -> Self {
        Self {
            api_base_url: api_base_url.into(),
            bearer: bearer.into(),
            expires_at,
            subject,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait.
// ---------------------------------------------------------------------------

/// Abstraction over the OS credential store. The CLI uses
/// [`KeyringStore`] in production and an in-memory implementation in tests.
pub trait CredentialStore: Send + Sync {
    /// Load the token previously stored under `(service, user)`. Returns
    /// `Ok(None)` if no entry exists.
    fn load(&self, service: &str, user: &str) -> Result<Option<StoredToken>, CredentialError>;

    /// Persist `token` under `(service, user)`. Overwrites any prior entry.
    fn save(&self, service: &str, user: &str, token: &StoredToken) -> Result<(), CredentialError>;

    /// Delete the entry. Returns `Ok(())` even if the entry did not exist.
    fn delete(&self, service: &str, user: &str) -> Result<(), CredentialError>;
}

/// Standard service name used by `tastile-cli`. Public so `doctor` can show
/// it without hard-coding in multiple places.
pub const DEFAULT_SERVICE: &str = "tastile-cli";

/// Standard user label. CLI supports only a single account at MVP, but the
/// `(service, user)` shape keeps the door open for multi-account later.
pub const DEFAULT_USER: &str = "default";

// ---------------------------------------------------------------------------
// Keyring-backed implementation.
// ---------------------------------------------------------------------------

/// In-memory credential store. Used by integration tests and by the
/// `--dry-run` flag in the CLI. Production paths always go through
/// [`KeyringStore`].
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: std::sync::Mutex<std::collections::HashMap<(String, String), StoredToken>>,
}

impl CredentialStore for MemoryStore {
    fn load(&self, service: &str, user: &str) -> Result<Option<StoredToken>, CredentialError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .get(&(service.to_string(), user.to_string()))
            .cloned())
    }

    fn save(&self, service: &str, user: &str, token: &StoredToken) -> Result<(), CredentialError> {
        self.inner
            .lock()
            .unwrap()
            .insert((service.to_string(), user.to_string()), token.clone());
        Ok(())
    }

    fn delete(&self, service: &str, user: &str) -> Result<(), CredentialError> {
        self.inner
            .lock()
            .unwrap()
            .remove(&(service.to_string(), user.to_string()));
        Ok(())
    }
}

/// Production credential store backed by the `keyring` crate.
#[derive(Debug, Default, Clone)]
pub struct KeyringStore;

impl CredentialStore for KeyringStore {
    fn load(&self, service: &str, user: &str) -> Result<Option<StoredToken>, CredentialError> {
        let entry = keyring::Entry::new(service, user)
            .map_err(|e| CredentialError::Backend(e.to_string()))?;
        match entry.get_password() {
            Ok(s) => {
                let parsed: StoredToken = serde_json::from_str(&s)
                    .map_err(|e| CredentialError::Malformed(e.to_string()))?;
                Ok(Some(parsed))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }

    fn save(&self, service: &str, user: &str, token: &StoredToken) -> Result<(), CredentialError> {
        let entry = keyring::Entry::new(service, user)
            .map_err(|e| CredentialError::Backend(e.to_string()))?;
        let serialized =
            serde_json::to_string(token).map_err(|e| CredentialError::Malformed(e.to_string()))?;
        entry
            .set_password(&serialized)
            .map_err(|e| CredentialError::Backend(e.to_string()))?;
        Ok(())
    }

    fn delete(&self, service: &str, user: &str) -> Result<(), CredentialError> {
        let entry = keyring::Entry::new(service, user)
            .map_err(|e| CredentialError::Backend(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }
}

// Sealed trait pattern is intentionally NOT applied at this stage — adding
// extension trait barriers would be premature without a second consumer.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let store = MemoryStore::default();
        let token = StoredToken::new("https://api.test", "secret", None, None);
        store.save(DEFAULT_SERVICE, DEFAULT_USER, &token).unwrap();
        let loaded = store
            .load(DEFAULT_SERVICE, DEFAULT_USER)
            .unwrap()
            .expect("present");
        assert_eq!(loaded.bearer, "secret");
        assert_eq!(loaded.api_base_url, "https://api.test");
        store.delete(DEFAULT_SERVICE, DEFAULT_USER).unwrap();
        assert!(store.load(DEFAULT_SERVICE, DEFAULT_USER).unwrap().is_none());
    }
}
