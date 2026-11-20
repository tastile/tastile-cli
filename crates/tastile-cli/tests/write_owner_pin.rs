//! Regression coverage for tastile-cli#13: every mutation must fail closed
//! when the stored credential's pinned owner does not match the owner the
//! write target reports, and the pin must persist across CLI invocations.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use uuid::Uuid;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("tastile-cli-owner-pin-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn source_tile_json(owner_id: Uuid) -> serde_json::Value {
    serde_json::json!({
        "source_tile_id": Uuid::new_v4(),
        "plan_id": Uuid::new_v4(),
        "owner_id": owner_id,
        "revision": 1,
        "source_state": 0,
        "title": "owner-pin-e2e",
        "plan_role": 0,
        "schedule": {
            "required_duration_ms": 1800000,
            "generation": {"kind": 0, "at": "2026-12-01T09:00:00Z"},
            "window": {"start_offset_ms": 0, "end_offset_ms": 14400000},
            "split_policy": {"kind": 0},
            "priority": 1
        },
        "created_at": "2026-11-16T00:00:00Z",
        "updated_at": "2026-11-16T00:00:00Z"
    })
}

fn detail_json(owner_id: Uuid) -> String {
    serde_json::json!({
        "source": source_tile_json(owner_id),
        "relations": [],
        "occurrences": {"items": [], "count": 0},
        "placements": {"items": [], "count": 0}
    })
    .to_string()
}

fn command_response_json(source_tile_id: Uuid) -> String {
    serde_json::json!({
        "command_id": Uuid::new_v4(),
        "accepted_at": "2026-11-16T00:00:00Z",
        "aggregate": null,
        "revision": 1,
        "result": 0,
        "pending": [],
        "aggregate_meta": {"source_tile_id": source_tile_id}
    })
    .to_string()
}

/// Read one HTTP request (request line, headers, optional body) and return
/// the connection plus the request line.
fn read_request(listener: &TcpListener) -> (std::net::TcpStream, String) {
    let (stream, _) = listener.accept().expect("accept API request");
    let mut reader = BufReader::new(stream.try_clone().expect("clone request stream"));
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .expect("read request line");
    let mut content_length = 0_usize;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).expect("read request header");
        if header == "\r\n" || header.is_empty() {
            break;
        }
        if let Some(value) = header
            .strip_prefix("content-length:")
            .or_else(|| header.strip_prefix("Content-Length:"))
        {
            content_length = value.trim().parse().expect("valid content length");
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).expect("read request body");
    drop(reader);
    (stream, request_line)
}

fn write_json(stream: &mut std::net::TcpStream, body: &str) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write API response");
}

fn request_path(request_line: &str) -> &str {
    request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
}

fn write_credential(path: &std::path::Path, api_url: &str) {
    std::fs::write(
        path,
        serde_json::json!({
            "api_base_url": api_url,
            "bearer": "test-only-token",
            "expires_at": null,
            "subject": "test-owner"
        })
        .to_string(),
    )
    .expect("write test credential");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("restrict test credential permissions");
    }
}

fn run_cli(
    args: &[&str],
    api_url: &str,
    credential_path: &std::path::Path,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tastile"))
        .args(args)
        .env("TASTILE_API_URL", api_url)
        .env("TASTILE_CREDENTIAL_FILE", credential_path)
        .env("TASTILE_ISOLATED_TEST_STACK", "confirmed")
        .output()
        .expect("run CLI")
}

#[tokio::test]
async fn mutation_pins_owner_and_refuses_a_different_owner() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock API");
    let api_url = format!("http://{}", listener.local_addr().unwrap());
    let owner_a = Uuid::new_v4();
    let owner_b = Uuid::new_v4();
    let created_source_id = Uuid::new_v4();

    // The guard probe lists source tiles. Phase 1 reports owner A; the
    // second write attempt reports owner B and must be refused before any
    // mutation request is sent.
    let list_calls = Arc::new(AtomicUsize::new(0));
    let list_calls_server = Arc::clone(&list_calls);
    let api = tokio::task::spawn_blocking(move || {
        // Phase 1, request 1: write guard probe -> owner A tile.
        let (mut stream, request_line) = read_request(&listener);
        assert!(request_line.starts_with("GET "), "{request_line}");
        assert_eq!(request_path(&request_line), "/v1/source-tiles");
        list_calls_server.fetch_add(1, Ordering::SeqCst);
        write_json(
            &mut stream,
            &serde_json::json!([source_tile_json(owner_a)]).to_string(),
        );

        // Phase 1, request 2: create mutation.
        let (mut stream, request_line) = read_request(&listener);
        assert!(request_line.starts_with("POST "), "{request_line}");
        assert_eq!(request_path(&request_line), "/v1/source-tiles");
        write_json(&mut stream, &command_response_json(created_source_id));

        // Phase 1, request 3: owner-pin detail read of the created source.
        let (mut stream, request_line) = read_request(&listener);
        assert!(request_line.starts_with("GET "), "{request_line}");
        assert_eq!(
            request_path(&request_line),
            format!("/v1/source-tiles/{created_source_id}")
        );
        write_json(&mut stream, &detail_json(owner_a));

        // Phase 2: second write attempt. Only the guard probe may arrive;
        // it reports owner B, which must fail closed against the pin.
        let (mut stream, request_line) = read_request(&listener);
        assert!(request_line.starts_with("GET "), "{request_line}");
        assert_eq!(request_path(&request_line), "/v1/source-tiles");
        list_calls_server.fetch_add(1, Ordering::SeqCst);
        write_json(
            &mut stream,
            &serde_json::json!([source_tile_json(owner_b)]).to_string(),
        );
    });

    let temp = TestDirectory::new();
    let credential_path = temp.0.join("credential.json");
    write_credential(&credential_path, &api_url);

    // Phase 1: create succeeds and pins owner A into the credential file.
    let api_url_create = api_url.clone();
    let credential_path_create = credential_path.clone();
    let phase1 = tokio::task::spawn_blocking(move || {
        run_cli(
            &[
                "source-tiles",
                "create",
                "--title",
                "owner-pin-e2e",
                "--once",
                "2026-12-01T09:00:00Z",
            ],
            &api_url_create,
            &credential_path_create,
        )
    })
    .await
    .expect("join phase-1 CLI");
    assert!(
        phase1.status.success(),
        "phase-1 create failed: {}",
        String::from_utf8_lossy(&phase1.stderr)
    );

    let stored: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&credential_path).expect("read credential file"),
    )
    .expect("credential file is JSON");
    assert_eq!(
        stored.get("owner_id").and_then(|v| v.as_str()),
        Some(owner_a.to_string().as_str()),
        "the observed owner must be pinned on the credential"
    );

    // Phase 2: the same credential against a target reporting owner B is
    // refused before any mutation request leaves the process. The timeout
    // keeps a regression from hanging the test instead of failing it.
    let api_url_retry = api_url.clone();
    let credential_path_retry = credential_path.clone();
    let phase2 = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::task::spawn_blocking(move || {
            run_cli(
                &[
                    "source-tiles",
                    "create",
                    "--title",
                    "owner-pin-e2e-2",
                    "--once",
                    "2026-12-02T09:00:00Z",
                ],
                &api_url_retry,
                &credential_path_retry,
            )
        }),
    )
    .await
    .expect("phase-2 CLI timed out")
    .expect("join phase-2 CLI");
    tokio::time::timeout(std::time::Duration::from_secs(30), api)
        .await
        .expect("mock API timed out")
        .expect("join mock API");

    assert!(
        !phase2.status.success(),
        "phase-2 create must fail closed: {}",
        String::from_utf8_lossy(&phase2.stdout)
    );
    let stderr = String::from_utf8_lossy(&phase2.stderr);
    assert!(
        stderr.contains("pinned to owner") || stderr.contains(&owner_b.to_string()),
        "unexpected phase-2 failure: {stderr}"
    );
    assert_eq!(list_calls.load(Ordering::SeqCst), 2);
}
