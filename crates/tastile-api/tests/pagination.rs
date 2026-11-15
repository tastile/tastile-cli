//! Source-tile lists and detail collections must walk the Core cursor
//! contract so large owners are never silently truncated.

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

fn response_page(items: Vec<serde_json::Value>, next_cursor: Option<&str>) -> serde_json::Value {
    let count = items.len();
    serde_json::json!({
        "items": items,
        "count": count,
        "next_cursor": next_cursor
    })
}

fn detail_page(
    occurrence_start: usize,
    occurrence_count: usize,
    occurrence_next: Option<&str>,
    placement_start: usize,
    placement_count: usize,
    placement_next: Option<&str>,
) -> String {
    let occurrences: Vec<serde_json::Value> = (occurrence_start
        ..occurrence_start + occurrence_count)
        .map(|index| serde_json::json!({"occurrence_id": format!("occ-{index}")}))
        .collect();
    let placements: Vec<serde_json::Value> = (placement_start..placement_start + placement_count)
        .map(placement_json)
        .collect();
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
        "occurrences": response_page(occurrences, occurrence_next),
        "placements": response_page(placements, placement_next)
    })
    .to_string()
}

fn page(start: usize, count: usize, next_cursor: Option<&str>) -> String {
    let rows: Vec<serde_json::Value> = (start..start + count)
        .map(|index| serde_json::from_str(&source_tile_json(index)).unwrap())
        .collect();
    response_page(rows, next_cursor).to_string()
}

#[tokio::test]
async fn list_all_source_tiles_follows_next_cursor() {
    let mut server = Server::new_async().await;
    let first = server
        .mock("GET", "/v1/source-tiles")
        .match_query(Matcher::Regex("limit=500$".into()))
        .with_body(page(0, 500, Some("source-next")))
        .create_async()
        .await;
    let second = server
        .mock("GET", "/v1/source-tiles")
        .match_query(Matcher::Regex("limit=500&cursor=source-next".into()))
        .with_body(page(500, 3, None))
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
        "every row across both cursor pages must be returned"
    );
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn list_all_source_tiles_stops_on_a_short_page() {
    let mut server = Server::new_async().await;
    let only = server
        .mock("GET", "/v1/source-tiles")
        .match_query(Matcher::Regex("limit=500$".into()))
        .with_body(page(0, 2, None))
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
async fn get_source_tile_walks_independent_occurrence_and_placement_cursors() {
    let mut server = Server::new_async().await;
    let first = server
        .mock(
            "GET",
            "/v1/source-tiles/00000000-0000-0000-0000-000000000001",
        )
        .match_query(Matcher::Regex(
            "occurrence_limit=500&placement_limit=500$".into(),
        ))
        .with_body(detail_page(0, 1, None, 0, 2, Some("placements-next")))
        .create_async()
        .await;
    let second = server
        .mock(
            "GET",
            "/v1/source-tiles/00000000-0000-0000-0000-000000000001",
        )
        .match_query(Matcher::Regex(
            "occurrence_limit=500&placement_limit=500&placement_cursor=placements-next".into(),
        ))
        .with_body(detail_page(0, 1, None, 2, 1, None))
        .create_async()
        .await;

    let client = ApiClient::new(ApiConfig::new(&server.url()).expect("valid url")).expect("client");
    let token = BearerToken::new("test-token");
    let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();

    let detail = source_tiles::get_source_tile(&client, &token, id)
        .await
        .expect("gets all detail pages");

    assert_eq!(
        detail.occurrences.len(),
        1,
        "finished collection is not duplicated"
    );
    assert_eq!(detail.placements.len(), 3);
    assert_eq!(detail.source.title, "large detail");
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn get_source_tile_sends_occurrence_cursor_when_placements_are_done() {
    let mut server = Server::new_async().await;
    let first = server
        .mock(
            "GET",
            "/v1/source-tiles/00000000-0000-0000-0000-000000000001",
        )
        .match_query(Matcher::Regex(
            "occurrence_limit=500&placement_limit=500$".into(),
        ))
        .with_body(detail_page(0, 1, Some("occurrences-next"), 0, 1, None))
        .create_async()
        .await;
    let second = server
        .mock(
            "GET",
            "/v1/source-tiles/00000000-0000-0000-0000-000000000001",
        )
        .match_query(Matcher::Regex(
            "occurrence_limit=500&placement_limit=500&occurrence_cursor=occurrences-next".into(),
        ))
        .with_body(detail_page(1, 2, None, 0, 1, None))
        .create_async()
        .await;

    let client = ApiClient::new(ApiConfig::new(&server.url()).expect("valid url")).expect("client");
    let token = BearerToken::new("test-token");
    let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let detail = source_tiles::get_source_tile(&client, &token, id)
        .await
        .expect("walk occurrence pages");

    assert_eq!(detail.occurrences.len(), 3);
    assert_eq!(
        detail.placements.len(),
        1,
        "completed collection is not duplicated"
    );
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn list_all_source_tile_placements_follows_next_cursor() {
    let mut server = Server::new_async().await;
    let first = server
        .mock(
            "GET",
            "/v1/source-tiles/00000000-0000-0000-0000-000000000001/placements",
        )
        .match_query(Matcher::Regex("limit=500$".into()))
        .with_body(
            response_page(
                (0..500).map(placement_json).collect(),
                Some("placements-next"),
            )
            .to_string(),
        )
        .create_async()
        .await;
    let second = server
        .mock(
            "GET",
            "/v1/source-tiles/00000000-0000-0000-0000-000000000001/placements",
        )
        .match_query(Matcher::Regex("limit=500&cursor=placements-next".into()))
        .with_body(response_page(vec![placement_json(500)], None).to_string())
        .create_async()
        .await;

    let client = ApiClient::new(ApiConfig::new(&server.url()).expect("valid url")).expect("client");
    let token = BearerToken::new("test-token");
    let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let all = source_tiles::list_all_source_tile_placements(&client, &token, id)
        .await
        .expect("list all placements");

    assert_eq!(all.len(), 501);
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn source_tile_list_still_decodes_the_pinned_array_response() {
    let mut server = Server::new_async().await;
    let only = server
        .mock("GET", "/v1/source-tiles")
        .match_query(Matcher::Regex("limit=500$".into()))
        .with_body(format!("[{}]", source_tile_json(0)))
        .create_async()
        .await;

    let client = ApiClient::new(ApiConfig::new(&server.url()).expect("valid url")).expect("client");
    let token = BearerToken::new("test-token");
    let all = source_tiles::list_all_source_tiles(&client, &token, &Default::default())
        .await
        .expect("legacy pinned response");

    assert_eq!(all.len(), 1);
    only.assert_async().await;
}

#[tokio::test]
async fn full_legacy_source_tile_page_fails_instead_of_silently_truncating() {
    let mut server = Server::new_async().await;
    let only = server
        .mock("GET", "/v1/source-tiles")
        .match_query(Matcher::Regex("limit=500$".into()))
        .with_body(format!(
            "[{}]",
            (0..500).map(source_tile_json).collect::<Vec<_>>().join(",")
        ))
        .create_async()
        .await;

    let client = ApiClient::new(ApiConfig::new(&server.url()).expect("valid url")).expect("client");
    let token = BearerToken::new("test-token");
    let error = source_tiles::list_all_source_tiles(
        &client,
        &token,
        &source_tiles::ListSourceTilesQuery::default(),
    )
    .await
    .expect_err("a full legacy page cannot prove that no rows were truncated");

    assert!(error.to_string().contains("cursor pagination is required"));
    only.assert_async().await;
}

#[tokio::test]
async fn full_legacy_placement_page_fails_instead_of_silently_truncating() {
    let mut server = Server::new_async().await;
    let only = server
        .mock(
            "GET",
            "/v1/source-tiles/00000000-0000-0000-0000-000000000001/placements",
        )
        .match_query(Matcher::Regex("limit=500$".into()))
        .with_body(serde_json::Value::Array((0..500).map(placement_json).collect()).to_string())
        .create_async()
        .await;

    let client = ApiClient::new(ApiConfig::new(&server.url()).expect("valid url")).expect("client");
    let token = BearerToken::new("test-token");
    let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let error = source_tiles::list_all_source_tile_placements(&client, &token, id)
        .await
        .expect_err("a full legacy page cannot prove completeness");

    assert!(error.to_string().contains("cursor pagination is required"));
    only.assert_async().await;
}

#[tokio::test]
async fn full_legacy_detail_collection_page_fails_instead_of_silently_truncating() {
    let mut server = Server::new_async().await;
    let mut detail: serde_json::Value =
        serde_json::from_str(&detail_page(0, 0, None, 0, 0, None)).unwrap();
    detail["occurrences"] = serde_json::Value::Array(
        (0..500)
            .map(|index| serde_json::json!({"occurrence_id": format!("occ-{index}")}))
            .collect(),
    );
    detail["placements"] = serde_json::json!([]);
    let only = server
        .mock(
            "GET",
            "/v1/source-tiles/00000000-0000-0000-0000-000000000001",
        )
        .match_query(Matcher::Regex(
            "occurrence_limit=500&placement_limit=500$".into(),
        ))
        .with_body(detail.to_string())
        .create_async()
        .await;

    let client = ApiClient::new(ApiConfig::new(&server.url()).expect("valid url")).expect("client");
    let token = BearerToken::new("test-token");
    let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let error = source_tiles::get_source_tile(&client, &token, id)
        .await
        .expect_err("a full legacy detail page cannot prove completeness");

    assert!(error.to_string().contains("cursor pagination is required"));
    only.assert_async().await;
}
