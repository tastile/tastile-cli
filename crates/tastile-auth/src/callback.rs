//! Loopback callback listener.
//!
//! The web login page redirects the user's browser to
//! `http://127.0.0.1:<port>/callback?code=<authorization_code>&state=<state>`
//! after they grant the CLI access. This module:
//!
//! 1. Binds a single TCP listener on a loopback address.
//! 2. Serves one HTTP request — the redirect.
//! 3. Returns the parsed `(code, state)` to the caller.
//!
//! The listener binds **only** to loopback (127.0.0.1 or ::1) and refuses
//! to bind to anything else. The port is allocated by the OS so we never
//! collide with another running `tastile auth login`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing::{debug, warn};
use url::Url;

/// What the loopback handler extracted from the redirect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackOutcome {
    /// User authorized; we have a one-time code.
    Authorized { code: String, state: String },
    /// User denied the request (or the server sent `error=access_denied`).
    Denied { reason: String },
    /// Something else went wrong (malformed URL, missing params, …).
    Error(String),
}

/// Errors that can arise from binding or running the listener.
#[derive(Debug, Error)]
pub enum CallbackError {
    #[error("could not bind loopback listener: {0}")]
    Bind(String),
    #[error("listener stopped unexpectedly: {0}")]
    Listener(String),
    #[error("timed out waiting for the browser callback")]
    Timeout,
}

/// Loopback callback listener. Construct via [`CallbackListener::bind`].
#[derive(Debug)]
pub struct CallbackListener {
    addr: SocketAddr,
    timeout: Duration,
}

impl CallbackListener {
    /// Bind to a random loopback port. The choice between IPv4 and IPv6
    /// is OS-dependent; both are loopback.
    pub async fn bind() -> Result<Self, CallbackError> {
        let v4 = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .map_err(|e| CallbackError::Bind(format!("127.0.0.1:0: {e}")))?;
        let addr = v4
            .local_addr()
            .map_err(|e| CallbackError::Bind(format!("local_addr: {e}")))?;
        // Drop the listener; we will rebind on `serve` so we can hand the
        // port out to the caller before serving.
        drop(v4);
        Ok(Self {
            addr,
            timeout: Duration::from_secs(120),
        })
    }

    /// The loopback URL the browser will be redirected to.
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.addr.port())
    }

    /// Override the wait-for-callback timeout. Default is 2 minutes.
    #[must_use]
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// Serve one HTTP request and return the parsed outcome.
    pub async fn serve(self) -> Result<CallbackOutcome, CallbackError> {
        let listener = TcpListener::bind(self.addr)
            .await
            .map_err(|e| CallbackError::Bind(format!("{}: {e}", self.addr)))?;

        let (tx, rx) = oneshot::channel::<CallbackOutcome>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let timeout = self.timeout;

        let serve = tokio::spawn(async move {
            if let Err(e) = run_server(listener, tx).await {
                warn!("callback server error: {e}");
            }
        });

        let outcome = tokio::select! {
            res = rx => res.map_err(|e| CallbackError::Listener(e.to_string()))?,
            _ = tokio::time::sleep(timeout) => {
                serve.abort();
                return Err(CallbackError::Timeout);
            }
        };
        Ok(outcome)
    }
}

#[derive(Debug, Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn run_server(
    listener: TcpListener,
    tx: Arc<std::sync::Mutex<Option<oneshot::Sender<CallbackOutcome>>>>,
) -> Result<(), CallbackError> {
    // We only serve a single request. After that, the server shuts down.
    let (stream, peer) = listener
        .accept()
        .await
        .map_err(|e| CallbackError::Listener(format!("accept: {e}")))?;
    debug!(%peer, "callback request received");

    // Reject non-loopback. The peer address is whatever the OS sees — for
    // 127.0.0.1 bindings it is always loopback. Defensive check anyway.
    if !peer.ip().is_loopback() {
        let outcome = CallbackOutcome::Error(format!(
            "callback came from non-loopback peer {}; refusing",
            peer.ip()
        ));
        if let Some(tx) = tx.lock().unwrap().take() {
            let _ = tx.send(outcome);
        }
        return Ok(());
    }

    let outcome = match parse_first_request(stream).await {
        Ok(parsed) => parsed,
        Err(e) => CallbackOutcome::Error(format!("malformed callback: {e}")),
    };

    if let Some(tx) = tx.lock().unwrap().take() {
        let _ = tx.send(outcome);
    }
    Ok(())
}

async fn parse_first_request(mut stream: tokio::net::TcpStream) -> Result<CallbackOutcome, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = [0u8; 4096];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| format!("read: {e}"))?;
    let raw = std::str::from_utf8(&buf[..n]).map_err(|e| format!("utf8: {e}"))?;

    let request_line = raw
        .lines()
        .next()
        .ok_or_else(|| "empty request".to_string())?;
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "no path in request line".to_string())?;

    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                    Connection: close\r\n\r\n\
                    <!doctype html><meta charset=utf-8>\
                    <title>tastile auth</title>\
                    <body style=\"font-family:system-ui;padding:2rem\">\
                    <h1>You are signed in.</h1>\
                    <p>You can close this tab and return to the terminal.</p>\
                    </body>";
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;

    let url =
        Url::parse(&format!("http://localhost{path}")).map_err(|e| format!("url parse: {e}"))?;

    let params: CallbackParams = url.query_pairs().fold(
        CallbackParams {
            code: None,
            state: None,
            error: None,
            error_description: None,
        },
        |mut acc, (k, v)| {
            match k.as_ref() {
                "code" => acc.code = Some(v.into_owned()),
                "state" => acc.state = Some(v.into_owned()),
                "error" => acc.error = Some(v.into_owned()),
                "error_description" => acc.error_description = Some(v.into_owned()),
                _ => {}
            }
            acc
        },
    );

    match params {
        CallbackParams {
            error: Some(reason),
            ..
        } => Ok(CallbackOutcome::Denied {
            reason: params.error_description.unwrap_or_else(|| reason.clone()),
        }),
        CallbackParams {
            code: Some(code),
            state: Some(state),
            ..
        } => Ok(CallbackOutcome::Authorized { code, state }),
        CallbackParams { .. } => Err("missing `code` or `state` in callback".into()),
    }
}

// Silence the dead-code warning on the unused Ipv6Addr import — keep it for
// future IPv6-only loopback support.
#[allow(dead_code)]
const _IPV6_LOOPBACK: Ipv6Addr = Ipv6Addr::LOCALHOST;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn listener_reports_authorized() {
        let listener = CallbackListener::bind()
            .await
            .expect("bind")
            .with_timeout(Duration::from_secs(5));
        let url = listener.redirect_uri();
        // Simulate the browser redirect.
        tokio::spawn(async move {
            // tiny sleep so the server is ready
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = reqwest::get(format!("{url}/callback?code=ABC&state=XYZ")).await;
        });

        let outcome = listener.serve().await.expect("serve");
        assert!(matches!(
            outcome,
            CallbackOutcome::Authorized { code, state }
                if code == "ABC" && state == "XYZ"
        ));
    }

    #[tokio::test]
    async fn listener_reports_denied() {
        let listener = CallbackListener::bind()
            .await
            .expect("bind")
            .with_timeout(Duration::from_secs(5));
        let url = listener.redirect_uri();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = reqwest::get(format!("{url}/callback?error=access_denied")).await;
        });

        let outcome = listener.serve().await.expect("serve");
        assert!(matches!(outcome, CallbackOutcome::Denied { .. }));
    }
}
