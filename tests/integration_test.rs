//! Integration tests for MobHub native suite.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mobhub::gym::{SessionParams, StatName};
use mobhub::payout::{MemberInput, PayoutModel, PayoutOptions, PayoutPolicy};
use mobhub::server::{create_router, AppState};
use mobhub::storage::Database;
use mobhub::torn::TornClient;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn test_full_router_integration() {
    let storage = Database::open_in_memory().unwrap();
    let torn = TornClient::new_mock();
    let state = AppState {
        storage,
        torn,
        faction_id: 50645,
    };
    let app = create_router(state);

    // 1. Health check
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 2. Status check
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 3. Payout compute endpoint
    let policy = PayoutPolicy {
        model: PayoutModel::FlatRate,
        rate_per_respect: 180284.0,
        rate_per_outside_hit: 500000.0,
        rate_per_assist: 250000.0,
        weight_war_hit: 1.0,
        weight_assist: 0.5,
        weight_outside_hit: 0.5,
        split_members_pct: 0.8,
        split_faction_pct: 0.2,
        split_leadership_pct: 0.0,
        leadership_recipient_torn_id: None,
        use_fixed_respect_rate: None,
        leadership_also_gets_member_pay: None,
    };
    let members = vec![
        MemberInput {
            torn_id: 2532285,
            name: Some("MrRazorlove".to_string()),
            flat_respect: 413.12,
            war_hits: 55,
            assists: 0,
            outside_hits: 1,
            is_leadership: None,
        },
        MemberInput {
            torn_id: 4187899,
            name: Some("BabyBubbles".to_string()),
            flat_respect: 180.5,
            war_hits: 26,
            assists: 0,
            outside_hits: 0,
            is_leadership: None,
        },
    ];
    let payload = json!({
        "cache_value": 349499999i64,
        "policy": policy,
        "members": members,
        "options": PayoutOptions::default()
    });

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/payout/compute")
                .header("Content-Type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let resp_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp_json["faction_fund"], 69900000);
    assert_eq!(resp_json["members"].as_array().unwrap().len(), 2);

    // 4. Valuation compute endpoint
    let val_payload = json!({
        "respect": 100000.0,
        "respect_rate": 180284.0,
        "armory_value": 50000000.0,
        "faction_cash": 25000000.0
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/valuation/compute")
                .header("Content-Type", "application/json")
                .body(Body::from(val_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let val_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val_json["total_value"], 18028400000i64 + 50000000 + 25000000);

    // 5. Gym simulate endpoint
    let gym_payload = json!(SessionParams {
        stat: StatName::Strength,
        start_stat: 1_000_000.0,
        start_happy: 5000.0,
        energy_available: 50.0,
        energy_per_train: 5.0,
        gym_dots: 7.3,
        perk_bonuses: vec![],
        happy_loss_per_train: Some(2.5),
        min_happy: Some(0.0),
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/gym/simulate")
                .header("Content-Type", "application/json")
                .body(Body::from(gym_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let gym_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(gym_json["trains"], 10);
}
