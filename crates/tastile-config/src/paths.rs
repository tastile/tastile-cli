//! Filesystem paths used by the CLI.
//!
//! Centralised so the rest of the code does not have to know about
//! `dirs::config_dir()` quirks across macOS / Linux / Windows.

use std::path::PathBuf;

/// `~/.config/tastile/config.toml` (or platform equivalent).
pub fn config_path() -> Option<PathBuf> {
    let base = dirs::config_dir()?;
    Some(base.join("tastile").join("config.toml"))
}

/// `~/.local/state/tastile/` (or platform equivalent). Used for caches
/// such as the OpenAPI drift report.
pub fn state_dir() -> Option<PathBuf> {
    let base = dirs::state_dir()?;
    Some(base.join("tastile"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_inside_tastile_namespace() {
        if let Some(p) = config_path() {
            assert!(
                p.components().any(|c| c.as_os_str() == "tastile"),
                "config path should contain `tastile` namespace: {p:?}"
            );
        }
    }
}
