//! Write-target safety gate shared by every mutating CLI operation.
//!
//! A loopback API base is **ambiguous**: it can be an isolated test stack
//! or the operational Muse stack that owns the real schedule. The gate
//! therefore refuses to write to loopback targets unless the operator has
//! independently attested that the stack is isolated, and it refuses the
//! reserved operational port outright.
//!
//! The attestation is a *safeguard*, never a proof of isolation: it only
//! records that somebody checked the database, owner, worker and
//! notification sink of the target stack. The real verification is a
//! read-only probe against the target (see `app::guard_write`).

use anyhow::{Result, bail};
use tastile_config::Config;
use url::{Host, Url};

/// Port reserved for the Muse operational Tastile API. Writes to it are
/// refused from every host, loopback or not.
pub const OPERATIONAL_API_PORT: u16 = 31400;

/// Environment variable an operator sets after verifying that the loopback
/// target is a dedicated stack (separate database, owner, worker and
/// notification sink).
pub const ISOLATION_ENV: &str = "TASTILE_ISOLATED_TEST_STACK";

/// The only accepted value of [`ISOLATION_ENV`].
pub const ISOLATION_CONFIRMED: &str = "confirmed";

/// Classification of the API base a write would be sent to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteTarget {
    /// A remote API reached over TLS. The operator configured it explicitly.
    RemoteHttps { host: String },
    /// A loopback API that carries an explicit isolation attestation.
    IsolatedLoopback { port: u16 },
}

impl WriteTarget {
    /// Human-readable description used in CLI output. Never contains a
    /// credential.
    pub fn describe(&self) -> String {
        match self {
            Self::RemoteHttps { host } => format!("remote API `{host}`"),
            Self::IsolatedLoopback { port } => format!("isolated loopback API on port {port}"),
        }
    }
}

/// True when the URL points at the loopback interface.
fn is_loopback(host: &Host<&str>) -> bool {
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

/// True when the given API base URL points at the loopback interface.
///
/// Used to keep the test-only file credential store away from remote
/// (production) API bases.
pub fn is_loopback_url(raw: &str) -> bool {
    match Url::parse(raw.trim_end_matches('/')) {
        Ok(url) => url.host().is_some_and(|host| is_loopback(&host)),
        Err(_) => false,
    }
}

/// True when the operator attested that the loopback target is an isolated
/// stack. Reading the environment is kept out of [`classify_write_target`]
/// so the decision stays a pure function of its inputs.
pub fn isolation_attested() -> bool {
    std::env::var(ISOLATION_ENV).ok().as_deref() == Some(ISOLATION_CONFIRMED)
}

/// Decide whether a write may be sent to the configured API base.
///
/// * Loopback targets need an isolation attestation and must not use the
///   reserved operational port.
/// * Remote targets must use TLS; plaintext remote writes are ambiguous
///   (they can be a proxy or a mistyped address) and are refused.
pub fn classify_write_target(cfg: &Config, attested: bool) -> Result<WriteTarget> {
    let url = Url::parse(cfg.api_url.trim_end_matches('/'))
        .map_err(|e| anyhow::anyhow!("invalid configured API URL: {e}"))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        bail!("unsupported API URL scheme `{scheme}`; expected http or https");
    }
    let host = url
        .host()
        .ok_or_else(|| anyhow::anyhow!("configured API URL has no host"))?;
    let port = url.port_or_known_default().unwrap_or(80);

    if port == OPERATIONAL_API_PORT {
        bail!(
            "refusing to write to API host `{host}` on port {OPERATIONAL_API_PORT}: that port is reserved for the \
             Muse operational Tastile stack, whose schedule is the source of truth",
        );
    }

    if is_loopback(&host) {
        if scheme != "http" {
            bail!(
                "refusing to write to loopback API on port {port} over TLS: the local Core speaks plain \
                 HTTP on loopback; check the configured API URL",
            );
        }
        if !attested {
            bail!(
                "no isolated test-stack confirmation for loopback API on port {port}: set {ISOLATION_ENV}={ISOLATION_CONFIRMED} \
                 only after independently verifying a separate database, owner, worker and \
                 notification sink. The confirmation is an attestation, not proof of isolation.",
            );
        }
        return Ok(WriteTarget::IsolatedLoopback { port });
    }

    if scheme != "https" {
        bail!(
            "refusing to write to remote API host `{host}` over plaintext HTTP: remote writes must use \
             HTTPS",
        );
    }
    Ok(WriteTarget::RemoteHttps {
        host: host.to_string(),
    })
}

/// Decide whether `schedule publish --apply` may write to this target.
/// The repository's CLI contract restricts this operation to an attested
/// loopback Core even though other explicitly configured HTTPS API targets
/// can be used by ordinary write commands.
pub fn classify_schedule_publish_target(cfg: &Config, attested: bool) -> Result<WriteTarget> {
    let target = classify_write_target(cfg, attested)?;
    if !matches!(target, WriteTarget::IsolatedLoopback { .. }) {
        bail!(
            "schedule publish --apply is restricted to an isolated loopback Core; remote targets are refused"
        );
    }
    Ok(target)
}

/// Compare API bases after URL canonicalization and harmless trailing-slash
/// normalization. Credentials and query/fragment components are rejected so
/// they cannot silently change the target or leak through diagnostics.
pub fn api_bases_match(configured: &str, stored: &str) -> bool {
    fn parts(raw: &str) -> Option<(String, String, Option<u16>, String)> {
        let url = Url::parse(raw.trim_end_matches('/')).ok()?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let host = url.host_str()?.to_ascii_lowercase();
        let port = url.port_or_known_default();
        let path = url.path().trim_end_matches('/').to_string();
        Some((url.scheme().to_string(), host, port, path))
    }
    parts(configured)
        .zip(parts(stored))
        .is_some_and(|(a, b)| a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(url: &str) -> Config {
        let mut cfg = Config::default();
        cfg.api_url = url.to_string();
        cfg
    }

    #[test]
    fn operational_port_is_refused_on_every_host() {
        assert!(classify_write_target(&cfg_with("http://127.0.0.1:31400"), true).is_err());
        assert!(classify_write_target(&cfg_with("http://localhost:31400"), true).is_err());
        assert!(classify_write_target(&cfg_with("http://192.168.1.2:31400"), true).is_err());
    }

    #[test]
    fn loopback_without_attestation_is_refused() {
        assert!(classify_write_target(&cfg_with("http://127.0.0.1:31500"), false).is_err());
        assert!(classify_write_target(&cfg_with("http://localhost:31500"), false).is_err());
        assert!(classify_write_target(&cfg_with("http://localhost.:31500"), false).is_err());
        assert!(classify_write_target(&cfg_with("http://[::1]:31500"), false).is_err());
        assert!(
            classify_write_target(&cfg_with("http://[::ffff:127.0.0.1]:31500"), false).is_err()
        );
    }

    #[test]
    fn remote_plaintext_is_refused() {
        assert!(classify_write_target(&cfg_with("http://api.example.test"), false).is_err());
    }

    #[test]
    fn remote_https_is_allowed() {
        let target = classify_write_target(&cfg_with("https://api.example.test"), false).unwrap();
        assert_eq!(
            target,
            WriteTarget::RemoteHttps {
                host: "api.example.test".to_string()
            }
        );
    }

    #[test]
    fn loopback_with_attestation_is_allowed() {
        let target = classify_write_target(&cfg_with("http://127.0.0.1:31500"), true).unwrap();
        assert_eq!(target, WriteTarget::IsolatedLoopback { port: 31500 });
    }

    #[test]
    fn schedule_publish_refuses_remote_https() {
        assert!(
            classify_schedule_publish_target(&cfg_with("https://api.example.test"), false).is_err()
        );
    }

    #[test]
    fn api_base_match_normalizes_trailing_slash_and_default_port() {
        assert!(api_bases_match(
            "https://api.example.test/",
            "https://api.example.test:443"
        ));
        assert!(!api_bases_match(
            "https://api.example.test",
            "https://other.example.test"
        ));
    }

    #[test]
    fn api_base_match_rejects_embedded_credentials_and_query_tokens() {
        assert!(!api_bases_match(
            "https://api.example.test?token=secret",
            "https://api.example.test"
        ));
        assert!(!api_bases_match(
            "https://user:secret@api.example.test",
            "https://api.example.test"
        ));
    }
}
