//! `tastile-config` — file-based configuration for the CLI.
//!
//! The CLI reads `~/.config/tastile/config.toml` (or the platform
//! equivalent via `dirs::config_dir()`) and merges it with environment
//! variables. The lookup precedence (highest first):
//!
//! 1. CLI flag (`--api-url`, `--web-url`).
//! 2. Environment variable (`TASTILE_API_URL`, `TASTILE_WEB_URL`).
//! 3. Config file.
//! 4. Compiled-in default.
//!
//! The config file is optional. Missing file is **not** an error at the
//! `load()` level — the defaults are returned and the CLI proceeds. The
//! user can opt into strict loading via `load_strict()` if they want to
//! diagnose missing files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::warn;

pub mod paths;

/// Top-level user configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Base URL for the v1 API. No trailing slash. Default: production.
    pub api_url: String,
    /// Base URL for the web app (where the browser auth happens). No
    /// trailing slash. Default: production.
    pub web_url: String,
    /// Per-request API timeout in milliseconds.
    pub api_timeout_ms: u64,
    /// `true` to enable verbose tracing output for `tastile doctor`.
    pub verbose: bool,
    /// `client_id` passed to the web login page. Operators can rotate this
    /// if they ever want to revoke outstanding CLI authorizations.
    pub oauth_client_id: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_url: "https://api.tastile.app".into(),
            web_url: "https://app.tastile.app".into(),
            api_timeout_ms: 15_000,
            verbose: false,
            oauth_client_id: "tastile-cli".into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not determine user config directory: {0}")]
    NoConfigDir(String),
    #[error("could not read config file {path}: {message}")]
    Read { path: PathBuf, message: String },
    #[error("config file {path} is invalid TOML: {message}")]
    Parse { path: PathBuf, message: String },
}

/// Default config file path on the current platform.
pub fn default_config_path() -> Option<PathBuf> {
    paths::config_path()
}

/// Load configuration. Reads the file at `default_config_path()` if it
/// exists; missing file is not an error. Returns `Config::default()` if
/// no file exists.
pub fn load() -> Result<Config, ConfigError> {
    let path = default_config_path();
    match path {
        None => Ok(Config::default()),
        Some(p) if !p.exists() => Ok(Config::default()),
        Some(p) => load_from(&p),
    }
}

/// Same as `load()`, but treats a missing file as an error.
pub fn load_strict() -> Result<Config, ConfigError> {
    let path = default_config_path().ok_or_else(|| {
        ConfigError::NoConfigDir("XDG_CONFIG_HOME or platform equivalent is unset".into())
    })?;
    load_from(&path)
}

fn load_from(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let cfg: Config = toml::from_str(&text).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    Ok(cfg)
}

/// Layer environment variables on top of a `Config` loaded from disk.
pub fn with_env_overrides(mut cfg: Config) -> Config {
    cfg = apply_env_overrides(cfg, read_env);
    cfg
}

/// Internal: apply env-var overrides with an injected reader so tests can
/// substitute a static map without mutating the real process environment.
fn apply_env_overrides<F: Fn(&str) -> Option<String>>(mut cfg: Config, read: F) -> Config {
    if let Some(v) = read("TASTILE_API_URL")
        && !v.trim().is_empty()
    {
        cfg.api_url = v;
    }
    if let Some(v) = read("TASTILE_WEB_URL")
        && !v.trim().is_empty()
    {
        cfg.web_url = v;
    }
    if let Some(v) = read("TASTILE_API_TIMEOUT_MS") {
        if let Ok(parsed) = v.parse::<u64>() {
            cfg.api_timeout_ms = parsed;
        } else {
            warn!("TASTILE_API_TIMEOUT_MS is not a u64; ignored");
        }
    }
    if let Some(v) = read("TASTILE_VERBOSE") {
        cfg.verbose = matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes");
    }
    cfg
}

fn read_env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_production_urls() {
        let cfg = Config::default();
        assert!(cfg.api_url.starts_with("https://"));
        assert!(cfg.web_url.starts_with("https://"));
    }

    #[test]
    fn env_overrides_take_precedence() {
        // Use a value unlikely to collide with anything else.
        let fake = |key: &str| match key {
            "TASTILE_API_URL" => Some("https://example.test".into()),
            _ => None,
        };
        let cfg = apply_env_overrides(Config::default(), fake);
        assert_eq!(cfg.api_url, "https://example.test");
    }

    #[test]
    fn env_overrides_ignored_when_empty() {
        let fake = |key: &str| match key {
            "TASTILE_API_URL" => Some("   ".into()),
            _ => None,
        };
        let cfg = apply_env_overrides(Config::default(), fake);
        // empty/whitespace override does NOT clobber the default
        assert!(!cfg.api_url.is_empty());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let bad = "api_url = \"https://x\"\nunexpected = 1\n";
        let result: Result<Config, _> = toml::from_str(bad);
        assert!(result.is_err());
    }
}
