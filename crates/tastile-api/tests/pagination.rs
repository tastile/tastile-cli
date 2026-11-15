//! The source-tile listings must not stop at the server's default page
//! size. The CLI walks the documented `limit` / `offset` pagination, so a
//! large owner is never silently truncated.

use mockito::{Matcher, Server};
use tastile_api::{ApiClient, ApiConfig, BearerToken, source_tiles};

fn source_tile_json(index: usize) -> String {
    format!(
        r#"{{
            "source_tile_id": "00000000-0000-0000-0000-{index:012}",
            "plan_id": "11111111-1111-1111-1111-111111111111",
            "owner_id": "22222222-2222-2222-2222-222222222222",
            "revision": 1,
            "source_state": 0,
            "title": "tile-{index}",
            "plan_role": 0,
            "schedule": {{
                "required_duration_ms": 1800000,
                "generation": {{"kind": 0, "at": "2026-11-15T00:00:00Z"}},
                "window": {{"start_offset_ms": 0, "end_offset_ms": 14400000}},
                "split_policy": {{"kind": 0}},
                "priority": 0
            }},
            "created_at": "2026-09-19T00:00:00Z",
            "updated_at": "2026-09-19T00:00:00Z"
        }}"#
    )
}

fn placement_json(index: usize) -> serde_json::Value {
    serde_json::json!({
        "placement_id": format!("00000000-0000-0000-0001-{index:012}"),
        "source_tile_id": "00000000-0000-0000-0000-000000000001",
        "occurrence_id": "11111111-1111-1111-1111-111111111111",
        "split_index": 0,
        "split_count": 1,
        "split_group_id": "22222222-2222-2222-2222-222222222222",
        "start": "2026-11-20T09:00:00Z",
        "end": "2026-11-20T09:30:00Z",
        "closed": false,
        "revision": 1
    })
}

fn detail_page(start: usize, count: usize) -> String {
    let placements: Vec<serde_json::Value> = (start..start + count).map(placement_json).collect();
    serde_json::json!({
        "source": {
            "source_tile_id": "00000000-0000-0000-0000-000000000001",
            "plan_id": "11111111-1111-1111-1111-111111111111",
            "owner_id": "22222222-2222-2222-2222-222222222222",
            "revision": 1,
            "source_state": 0,
            "title": "large detail",
            "plan_role": 0,
            "schedule": {
                "required_duration_ms": 1800000,
                "generation": {"kind": 0, "at": "2026-11-15T00:00:00Z"},
                "window": {"start_offset_ms": 0, "end_offset_ms": 14400000},
                "split_policy": {"kind": 0},
                "priority": 0
            },
            "created_at": "2026-09-19T00:00:00Z",
            "updated_at": "2026-09-19T00:00:00Z"
        },
        "relations": [],
        "occurrences": [],
        "placements": placements
    })
    .to_string()
}

fn page(start: usize, count: usize) -> String {
    let rows: Vec<String> = (start..start + count).map(source_tile_json).collect();
    format!("[{}]", rows.join(","))
}

#[tokio::test]
async fn list_all_source_tiles_walks_every_page() {
    let mut server = Server::new_async().await;
    let first = server
        .mock("GET", "/v1/source-tiles")
        .match_query(Matcher::Regex("limit=500.*offset=0".into()))
        .with_body(page(0, 500))
        .create_async()
        .await;
    let second = server
        .mock("GET", "/v1/source-tiles")
        .match_query(Matcher::Regex("limit=500.*offset=500".into()))
        .with_body(page(500, 3))
        .create_async()
        .await;

    let client = ApiClient::new(ApiConfig::new(&server.url()).expect("valid url")).expect("client");
    let token = BearerToken::new("test-token");
    let query = source_tiles::ListSourceTilesQuery::default();

    let all = source_tiles::list_all_source_tiles(&client, &token, &query)
        .await
        .expect("lists");

    assert_eq!(
        all.len(),
        503,
        "every row across both pages must be returned"
    );
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn list_all_source_tiles_stops_on_a_short_page() {
    let mut server = Server::new_async().await;
    let only = server
        .mock("GET", "/v1/source-tiles")
        .match_query(Matcher::Regex("limit=500.*offset=0".into()))
        .with_body(page(0, 2))
        .create_async()
        .await;

    let client = ApiClient::new(ApiConfig::new(&server.url()).expect("valid url")).expect("client");
    let token = BearerToken::new("test-token");

    let all = source_tiles::list_all_source_tiles(&client, &token, &Default::default())
        .await
        .expect("lists");

    assert_eq!(all.len(), 2);
    only.assert_async().await;
}

#[tokio::test]
async fn get_source_tile_walks_all_occurrence_and_placement_pages() {
    let mut server = Server::new_async().await;
    let first = server
        .mock(
            "GET",
            "/v1/source-tiles/00000000-0000-0000-0000-000000000001",
        )
        .match_query(Matcher::Regex("limit=500.*offset=0".into()))
        .with_body(detail_page(0, 500))
        .create_async()
        .await;
    let second = server
        .mock(
            "GET",
            "/v1/source-tiles/00000000-0000-0000-0000-000000000001",
        )
        .match_query(Matcher::Regex("limit=500.*offset=500".into()))
        .with_body(detail_page(500, 2))
        .create_async()
        .await;

    let client = ApiClient::new(ApiConfig::new(&server.url()).expect("valid url")).expect("client");
    let token = BearerToken::new("test-token");
    let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();

    let detail = source_tiles::get_source_tile(&client, &token, id)
        .await
        .expect("gets all detail pages");

    assert_eq!(detail.placements.len(), 502);
    assert_eq!(detail.source.title, "large detail");
    first.assert_async().await;
    second.assert_async().await;
}
