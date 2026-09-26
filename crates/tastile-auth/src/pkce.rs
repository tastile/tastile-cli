//! PKCE state machine for the browser-based authorization flow.
//!
//! Per RFC 7636 we generate a random `verifier`, derive a `challenge`
//! (S256), and ship `challenge` + `state` in the authorization URL. The
//! `state` value is used to defend against CSRF on the callback.

use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Generated PKCE pair (verifier + challenge) plus a random opaque
/// `state` value used for CSRF protection on the callback.
///
/// Construction goes through [`PkceState::generate`] which produces
/// cryptographically random values. The struct exposes the challenge and
/// state for the authorization URL but keeps the verifier private — the
/// caller must pass it explicitly to [`PkceState::verifier`] when the
/// code is exchanged for a token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkceState {
    /// 43–128 char URL-safe base64 random string. Never sent to the server
    /// in the auth URL — only used at the code-exchange step.
    verifier: String,
    /// `BASE64URL(SHA256(verifier))`.
    challenge: String,
    /// Opaque random string echoed back by the server on the callback.
    state: String,
}

/// Public view of the PKCE pair for the authorization URL. The verifier
/// stays in `PkceState` so it cannot leak into logs accidentally.
#[derive(Debug, Clone)]
pub struct PkcePair {
    pub challenge: String,
    pub state: String,
}

impl PkceState {
    /// Generate a fresh state. Verifier is 32 bytes → 43 url-safe base64
    /// chars (no padding). `state` is 16 bytes → 22 chars.
    pub fn generate() -> Self {
        let mut verifier_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut verifier_bytes);
        let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);

        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let digest = hasher.finalize();
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

        let mut state_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut state_bytes);
        let state = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(state_bytes);

        Self {
            verifier,
            challenge,
            state,
        }
    }

    /// Borrow the public pair (challenge + state) for the auth URL.
    pub fn pair(&self) -> PkcePair {
        PkcePair {
            challenge: self.challenge.clone(),
            state: self.state.clone(),
        }
    }

    /// Borrow the verifier for the code-exchange step. Do not log.
    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    /// Constant-time comparison of the `state` echoed back by the server
    /// against the value we generated. Returns `true` only if they match
    /// exactly. Uses `subtle::ConstantTimeEq` semantics by hand to avoid a
    /// new dependency.
    pub fn state_matches(&self, candidate: &str) -> bool {
        let a = self.state.as_bytes();
        let b = candidate.as_bytes();
        if a.len() != b.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for (x, y) in a.iter().zip(b.iter()) {
            diff |= x ^ y;
        }
        diff == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_state_matches_itself() {
        let s = PkceState::generate();
        assert!(s.state_matches(&s.state.clone()));
    }

    #[test]
    fn generated_state_rejects_other() {
        let s1 = PkceState::generate();
        let s2 = PkceState::generate();
        assert!(!s1.state_matches(&s2.state));
    }

    #[test]
    fn pair_has_distinct_challenge_and_state() {
        let s = PkceState::generate();
        let p = s.pair();
        assert_ne!(p.challenge, p.state);
        assert!(!p.challenge.is_empty());
        assert!(!p.state.is_empty());
    }

    #[test]
    fn verifier_differs_from_challenge() {
        let s = PkceState::generate();
        assert_ne!(s.verifier, s.challenge);
    }
}
