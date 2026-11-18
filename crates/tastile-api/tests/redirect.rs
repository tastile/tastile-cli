//! The API client must never follow redirects: an authenticated request
//! classified for one host must not be bounced by a 3xx to another.

use mockito::Server;
use tastile_api::{ApiClient, ApiConfig, BearerToken};

#[tokio::test]
async fn post_does_not_follow_redirects() {
    let mut server = Server::new_async().await;
    let redirected = server
        .mock("POST", "/v1/commands")
        .with_status(200)
        .create_async()
        .await;
    let _redirect = server
        .mock("POST", "/v1/old-path")
        .with_status(307)
        .with_header("location", "/v1/commands")
        .create_async()
        .await;

    let cfg = ApiConfig {
        base_url: format!("{}/", server.url()).parse().unwrap(),
        request_timeout: std::time::Duration::from_secs(5),
        user_agent: "tastile-cli-test".to_string(),
    };
    let client = ApiClient::new(cfg).unwrap();
    let token = BearerToken::new("test-token".to_string());

    let err = client
        .post_json::<serde_json::Value, serde_json::Value>(
            &token,
            "/v1/old-path",
            &serde_json::json!({"k": "v"}),
        )
        .await
        .expect_err("redirect must surface as an HTTP error, not be followed");
    assert!(
        format!("{err:?}").contains("307"),
        "unexpected error: {err:?}"
    );
    assert!(!redirected.matched());
}
