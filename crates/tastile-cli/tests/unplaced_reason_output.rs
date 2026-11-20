use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;

use uuid::Uuid;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("tastile-cli-output-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn create_prints_core_unplaced_reasons_and_conflicting_source() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock API");
    let api_url = format!("http://{}", listener.local_addr().unwrap());
    let insufficient_capacity_occurrence_id = Uuid::new_v4();
    let limit_conflict_occurrence_id = Uuid::new_v4();
    let priority_tie_occurrence_id = Uuid::new_v4();
    let conflicting_source_id = Uuid::new_v4();
    let response = serde_json::json!({
        "command_id": Uuid::new_v4(),
        "accepted_at": "2026-11-16T00:00:00Z",
        "aggregate": null,
        "revision": 1,
        "result": 0,
        "pending": [],
        "aggregate_meta": {
            "unplaced_occurrences": [
                {
                    "occurrence_id": insufficient_capacity_occurrence_id,
                    "reason": 0,
                    "required_duration_ms": 1800000,
                    "available_duration_ms": 0,
                    "blocked_at": "2026-11-16T00:00:00Z"
                },
                {
                    "occurrence_id": limit_conflict_occurrence_id,
                    "reason": 1,
                    "required_duration_ms": 1800000,
                    "available_duration_ms": 0,
                    "blocked_at": "2026-11-16T00:00:00Z"
                },
                {
                    "occurrence_id": priority_tie_occurrence_id,
                    "reason": 2,
                    "required_duration_ms": 1800000,
                    "available_duration_ms": 0,
                    "blocked_at": "2026-11-16T00:00:00Z",
                    "conflicting_source_tile_id": conflicting_source_id
                }
            ]
        }
    })
    .to_string();

    let api = tokio::task::spawn_blocking(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept API request");
            let mut request = BufReader::new(stream.try_clone().expect("clone request stream"));
            let mut request_line = String::new();
            request
                .read_line(&mut request_line)
                .expect("read request line");
            let mut content_length = 0_usize;
            loop {
                let mut header = String::new();
                request.read_line(&mut header).expect("read request header");
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
            let mut request_body = vec![0; content_length];
            request
                .read_exact(&mut request_body)
                .expect("read request body");

            let body = if request_line.starts_with("GET ") {
                "[]"
            } else {
                response.as_str()
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write API response");
        }
    });

    let temp = TestDirectory::new();
    let credential_path = temp.0.join("credential.json");
    std::fs::write(
        &credential_path,
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
        std::fs::set_permissions(&credential_path, std::fs::Permissions::from_mode(0o600))
            .expect("restrict test credential permissions");
    }

    let api_url_for_command = api_url.clone();
    let credential_path_for_command = credential_path.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_tastile"))
            .args([
                "source-tiles",
                "create",
                "--title",
                "reason-output-e2e",
                "--once",
                "2026-12-01T09:00:00Z",
            ])
            .env("TASTILE_API_URL", api_url_for_command)
            .env("TASTILE_CREDENTIAL_FILE", credential_path_for_command)
            .env("TASTILE_ISOLATED_TEST_STACK", "confirmed")
            .output()
            .expect("run CLI create")
    })
    .await
    .expect("join CLI process");
    api.await.expect("join mock API");

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("INSUFFICIENT_CAPACITY"), "{stdout}");
    assert!(stdout.contains("LIMIT_CONFLICT"), "{stdout}");
    assert!(stdout.contains("PRIORITY_TIE_CONFLICT"), "{stdout}");
    assert!(
        stdout.contains(&insufficient_capacity_occurrence_id.to_string()),
        "{stdout}"
    );
    assert!(
        stdout.contains(&limit_conflict_occurrence_id.to_string()),
        "{stdout}"
    );
    assert!(
        stdout.contains(&priority_tie_occurrence_id.to_string()),
        "{stdout}"
    );
    assert!(
        stdout.contains(&conflicting_source_id.to_string()),
        "{stdout}"
    );
}

#[tokio::test]
async fn get_prints_blocked_reason_from_source_occurrence_detail() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock API");
    let api_url = format!("http://{}", listener.local_addr().unwrap());
    let source_id = Uuid::new_v4();
    let occurrence_id = Uuid::new_v4();
    let conflicting_source_id = Uuid::new_v4();
    let response = serde_json::json!({
        "source": {
            "source_tile_id": source_id,
            "plan_id": Uuid::new_v4(),
            "owner_id": Uuid::new_v4(),
            "revision": 1,
            "source_state": 0,
            "title": "blocked-detail-e2e",
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
        },
        "relations": [],
        "occurrences": {
            "items": [{
                "occurrence_id": occurrence_id,
                "blocked": {
                    "reason": 2,
                    "required_duration_ms": 1800000,
                    "available_duration_ms": 0,
                    "blocked_at": "2026-11-16T00:00:00Z",
                    "conflicting_source_tile_id": conflicting_source_id
                }
            }],
            "count": 1
        },
        "placements": {"items": [], "count": 0}
    })
    .to_string();

    let api = tokio::task::spawn_blocking(move || {
        let (mut stream, _) = listener.accept().expect("accept API request");
        let mut request = BufReader::new(stream.try_clone().expect("clone request stream"));
        let mut request_line = String::new();
        request
            .read_line(&mut request_line)
            .expect("read request line");
        loop {
            let mut header = String::new();
            request.read_line(&mut header).expect("read request header");
            if header == "\r\n" || header.is_empty() {
                break;
            }
        }
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response.len(),
            response
        )
        .expect("write API response");
    });

    let temp = TestDirectory::new();
    let credential_path = temp.0.join("credential.json");
    std::fs::write(
        &credential_path,
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
        std::fs::set_permissions(&credential_path, std::fs::Permissions::from_mode(0o600))
            .expect("restrict test credential permissions");
    }

    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_tastile"))
            .args(["source-tiles", "get", &source_id.to_string()])
            .env("TASTILE_API_URL", api_url)
            .env("TASTILE_CREDENTIAL_FILE", credential_path)
            .output()
            .expect("run CLI get")
    })
    .await
    .expect("join CLI process");
    api.await.expect("join mock API");

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PRIORITY_TIE_CONFLICT"), "{stdout}");
    assert!(stdout.contains(&occurrence_id.to_string()), "{stdout}");
    assert!(
        stdout.contains(&conflicting_source_id.to_string()),
        "{stdout}"
    );
}
