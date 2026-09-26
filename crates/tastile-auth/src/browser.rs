//! Cross-platform browser launch.

use thiserror::Error;

/// Launch `url` in the user's default browser. Returns the target URL
/// (echoed back) on success so the caller can log it without holding any
/// secret-bearing state.
pub fn open_browser(url: &str) -> Result<&str, BrowserError> {
    open::that(url).map_err(|e| BrowserError::Launch(e.to_string()))?;
    Ok(url)
}

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("could not open browser: {0}")]
    Launch(String),
}

#[cfg(test)]
mod tests {
    // We cannot test the actual browser launch in CI; just verify the error
    // type is `Send + Sync`.
    fn assert_send_sync<T: Send + Sync>() {}
    #[test]
    fn errors_are_send_sync() {
        assert_send_sync::<crate::browser::BrowserError>();
    }
}
