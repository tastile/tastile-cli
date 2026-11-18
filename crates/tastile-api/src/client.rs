//! HTTP transport layer.
//!
//! Owns:
//! - the `reqwest::Client` connection pool,
//! - the base URL (API root, no trailing slash),
//! - request timeout,
//! - a single place to inject `Authorization: Bearer <raw>`.
//!
//! Operations live in per-endpoint modules (`tiles`, `prompts`, …). They all
//! take `&ApiClient` and produce typed responses.

use std::fmt;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::debug;
use url::{Host, Url};

use crate::error::{ApiError, ApiResult};

/// Raw bearer token. Construction does not validate token contents; never log
/// this value.
#[derive(Clone)]
pub struct BearerToken(String);

impl fmt::Debug for BearerToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BearerToken").field(&"[REDACTED]").finish()
    }
}

impl BearerToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }
    /// Borrow the underlying raw token. Only used to inject the
    /// `Authorization` header; never print.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Configuration for the API client. Cheap to clone.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// Base URL with no trailing slash, e.g. `https://api.tastile.app`.
    pub base_url: Url,
    /// Per-request timeout. The CLI defaults to 15s which is enough for any
    /// current endpoint; ops can override via `TASTILE_API_TIMEOUT_MS`.
    pub request_timeout: Duration,
    /// User-agent string. The CLI overrides this in `tastile-cli` so the
    /// default here is a fallback.
    pub user_agent: String,
}

impl ApiConfig {
    /// Build a config from a base URL string. Returns an error if the URL
    /// cannot be parsed. Callers should pass the env var or config-file
    /// value as-is; this is the only place the URL is validated.
    pub fn new(base_url: &str) -> ApiResult<Self> {
        let url = Url::parse(base_url.trim_end_matches('/'))
            .map_err(|e| ApiError::InvalidBaseUrl(format!("invalid API base URL: {e}")))?;
        if url.port() == Some(31_400) {
            return Err(ApiError::InvalidBaseUrl(
                "API base URL may not use reserved operational port 31400".into(),
            ));
        }
        if url.path() != "/" {
            return Err(ApiError::InvalidBaseUrl(
                "API base URL must not contain a path component".into(),
            ));
        }
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ApiError::InvalidBaseUrl(
                "API base URL must not contain credentials, a query, or a fragment".into(),
            ));
        }
        let host = url
            .host()
            .ok_or_else(|| ApiError::InvalidBaseUrl("API base URL must contain a host".into()))?;
        if url.scheme() == "http" && !is_loopback_host(&host) {
            return Err(ApiError::InvalidBaseUrl(
                "remote API bases must use HTTPS; plaintext HTTP is allowed only for loopback test targets".into(),
            ));
        }
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(ApiError::InvalidBaseUrl(
                "API base URL scheme must be HTTP or HTTPS".into(),
            ));
        }
        Ok(Self {
            base_url: url,
            request_timeout: Duration::from_secs(15),
            user_agent: format!("tastile-cli/{}", env!("CARGO_PKG_VERSION")),
        })
    }

    /// Override the per-request timeout.
    #[must_use]
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.request_timeout = d;
        self
    }
}

fn is_loopback_host(host: &Host<&str>) -> bool {
    match host {
        Host::Ipv4(ip) => ip.is_loopback(),
        Host::Ipv6(ip) => {
            ip.is_loopback()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| mapped.is_loopback())
        }
        Host::Domain(name) => name.trim_end_matches('.').eq_ignore_ascii_case("localhost"),
    }
}

/// Thin wrapper over `reqwest::Client` that adds:
///
/// - a stable base URL,
/// - a default `User-Agent`,
/// - bearer-token injection on every call,
/// - JSON encode/decode helpers,
/// - timeout handling that maps to `ApiError::Timeout` rather than the
///   `reqwest::Error::timeout` variant (which is annoying to match on).
#[derive(Debug, Clone)]
pub struct ApiClient {
    inner: reqwest::Client,
    cfg: ApiConfig,
}

impl ApiClient {
    /// Construct an `ApiClient` from configuration. The `reqwest::Client` is
    /// created once and reused across calls.
    pub fn new(cfg: ApiConfig) -> ApiResult<Self> {
        let inner = reqwest::Client::builder()
            .timeout(cfg.request_timeout)
            .user_agent(cfg.user_agent.clone())
            // Never follow redirects: an authenticated POST (e.g. schedule
            // publish) must not be bounced by a 3xx to a different host than
            // the one the caller's write-guard classified.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(ApiError::Transport)?;
        Ok(Self { inner, cfg })
    }

    /// Borrow the configuration so callers can inspect the base URL.
    pub fn config(&self) -> &ApiConfig {
        &self.cfg
    }

    /// Resolve a path under the configured base URL.
    fn url(&self, path: &str) -> ApiResult<Url> {
        let trimmed = path.trim_start_matches('/');
        self.cfg
            .base_url
            .join(trimmed)
            .map_err(|e| ApiError::InvalidBaseUrl(format!("{trimmed}: {e}")))
    }

    /// Build a `GET` request, attach the bearer token, execute it, and
    /// decode the JSON body into `T`. Returns the typed response or an error.
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        token: &BearerToken,
        path: &str,
        query: &[(&str, String)],
    ) -> ApiResult<T> {
        let url = self.url(path)?;
        let mut req = self.inner.get(url).bearer_auth(token.as_str());
        for (k, v) in query {
            req = req.query(&[(k, v.as_str())]);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: redact(&message),
            });
        }
        debug!(path, %status, "GET ok");
        resp.json::<T>().await.map_err(ApiError::Transport)
    }

    /// `DELETE` with a JSON body. Used by `DELETE /v1/owners/{kind}/{id}`.
    pub async fn delete_json<B: Serialize, T: DeserializeOwned>(
        &self,
        token: &BearerToken,
        path: &str,
        body: &B,
    ) -> ApiResult<T> {
        let url = self.url(path)?;
        let resp = self
            .inner
            .delete(url)
            .bearer_auth(token.as_str())
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: redact(&message),
            });
        }
        // Some 204 endpoints return no body.
        if status == reqwest::StatusCode::NO_CONTENT {
            return resp
                .json::<T>()
                .await
                .or_else(|_| Ok(serde_json::from_str("null")?));
        }
        resp.json::<T>().await.map_err(ApiError::Transport)
    }

    /// `POST` with a JSON body.
    pub async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        token: &BearerToken,
        path: &str,
        body: &B,
    ) -> ApiResult<T> {
        let url = self.url(path)?;
        let resp = self
            .inner
            .post(url)
            .bearer_auth(token.as_str())
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: redact(&message),
            });
        }
        resp.json::<T>().await.map_err(ApiError::Transport)
    }

    /// `PUT` with a JSON body.
    pub async fn put_json<B: Serialize, T: DeserializeOwned>(
        &self,
        token: &BearerToken,
        path: &str,
        body: &B,
    ) -> ApiResult<T> {
        let url = self.url(path)?;
        let resp = self
            .inner
            .put(url)
            .bearer_auth(token.as_str())
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: redact(&message),
            });
        }
        resp.json::<T>().await.map_err(ApiError::Transport)
    }

    /// `POST` with no body. Used by `POST /v1/auth/signout`.
    pub async fn post_empty(
        &self,
        token: &BearerToken,
        path: &str,
    ) -> ApiResult<reqwest::StatusCode> {
        let url = self.url(path)?;
        let resp = self
            .inner
            .post(url)
            .bearer_auth(token.as_str())
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: redact(&message),
            });
        }
        Ok(status)
    }
}

/// Strip anything that looks like a bearer token out of an error message
/// before it can be logged. Crude but cheap.
fn redact(s: &str) -> String {
    const MARKER: &str = "bearer ";
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = find_ci(rest, MARKER) {
        out.push_str(&rest[..pos]);
        out.push_str("[redacted-bearer]");
        // Advance past the marker and any token characters.
        let after_marker = &rest[pos + MARKER.len()..];
        let token_len = after_marker
            .char_indices()
            .take_while(|(_, c)| !c.is_whitespace())
            .last()
            .map(|(idx, c)| idx + c.len_utf8())
            .unwrap_or(0);
        rest = &after_marker[token_len..];
    }
    out.push_str(rest);
    out
}

/// Case-insensitive substring search. Returns the byte offset of the first
/// match, or `None`. ASCII-only — fine for the small set of HTTP header names
/// we are redacting.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let needle_bytes = needle.as_bytes();
    let hay_bytes = haystack.as_bytes();
    if needle_bytes.len() > hay_bytes.len() {
        return None;
    }
    for start in 0..=hay_bytes.len() - needle_bytes.len() {
        let mut matched = true;
        for (a, b) in hay_bytes[start..start + needle_bytes.len()]
            .iter()
            .zip(needle_bytes.iter())
        {
            if a.to_ascii_lowercase() != *b {
                matched = false;
                break;
            }
        }
        if matched {
            return Some(start);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_config_accepts_trailing_slash() {
        // Trailing slash is tolerated; `Url::parse` keeps it. Operations join
        // paths via `Url::join`, which canonicalises the slash for us.
        let cfg = ApiConfig::new("https://example.test/").expect("valid url");
        assert!(cfg.base_url.as_str().starts_with("https://example.test"));
    }

    #[test]
    fn api_config_refuses_reserved_operational_port_for_every_request() {
        for base in [
            "http://127.0.0.1:31400",
            "https://localhost:31400",
            "https://api.example.test:31400",
        ] {
            let error = ApiConfig::new(base).expect_err("reserved port must be rejected");
            assert!(error.to_string().contains("31400"), "{base}: {error}");
        }
    }

    #[test]
    fn api_config_rejects_path_components() {
        assert!(ApiConfig::new("https://api.example.test/v1").is_err());
        assert!(ApiConfig::new("https://api.example.test/").is_ok());
    }

    #[test]
    fn api_config_rejects_garbage() {
        assert!(ApiConfig::new("not a url").is_err());
    }

    #[test]
    fn api_config_allows_plaintext_only_for_loopback() {
        assert!(ApiConfig::new("http://127.0.0.1:31500").is_ok());
        assert!(ApiConfig::new("http://localhost:31500").is_ok());
        assert!(ApiConfig::new("http://api.example.test").is_err());
    }

    #[test]
    fn redact_strips_bearer_value() {
        let input = "Authorization: Bearer abcdef123456 hello world";
        let out = redact(input);
        assert!(out.contains("[redacted-bearer]"));
        assert!(!out.contains("abcdef123456"));
    }

    #[test]
    fn bearer_token_debug_redacts_the_secret() {
        let token = BearerToken::new("do-not-print-this-token");
        let debug = format!("{token:?}");
        assert!(!debug.contains("do-not-print-this-token"));
        assert!(debug.contains("[REDACTED]"));
    }
}
