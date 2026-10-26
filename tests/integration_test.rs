//! Integration tests for MobHub native suite.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mobhub::gym::{SessionParams, StatName};
use mobhub::payout::{MemberInput, PayoutModel, PayoutOptions, PayoutPolicy};
use mobhub::reconcile::{GiveForMatch, RegisterRowForMatch};
use mobhub::server::{create_router, AccessTokens, AppState};
use mobhub::storage::Database;
use mobhub::torn::TornClient;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

fn access(origin: Option<&str>) -> Arc<AccessTokens> {
    Arc::new(AccessTokens::new("r".repeat(32), "w".repeat(32), origin).unwrap())
}

#[test]
fn gym_rejects_non_progress_and_nonfinite_inputs() {
    let params = SessionParams {
        stat: StatName::Strength,
        start_stat: 100.0,
        start_happy: 100.0,
        energy_available: 10.0,
        energy_per_train: 0.0,
        gym_dots: 7.0,
        perk_bonuses: vec![],
        happy_loss_per_train: None,
        min_happy: None,
    };
    assert!(mobhub::gym::validate_session(&params).is_err());
    let mut negative = params.clone();
    negative.energy_per_train = -1.0;
    assert!(mobhub::gym::validate_session(&negative).is_err());
    let mut huge = params.clone();
    huge.energy_per_train = f64::MIN_POSITIVE;
    assert!(mobhub::gym::validate_session(&huge).is_err());
    let mut nonfinite = params.clone();
    nonfinite.start_stat = f64::INFINITY;
    assert!(mobhub::gym::validate_session(&nonfinite).is_err());
}

#[tokio::test]
async fn scoped_tokens_allow_reads_and_only_write_token_mutates() {
    let storage = Database::open_in_memory().unwrap();
    let app = create_router(AppState {
        storage: storage.clone(),
        torn: TornClient::new_mock(),
        faction_id: 50645,
        access: access(None),
    });
    let war =
        json!({"id": 10, "name": "original", "cache_value": 42, "status": "active", "data": {}});
    for (token, expected) in [
        ("r".repeat(32), StatusCode::UNAUTHORIZED),
        ("invalid".to_string(), StatusCode::UNAUTHORIZED),
        ("w".repeat(32), StatusCode::CREATED),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri("/api/wars")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(war.to_string()))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            expected
        );
    }
    assert_eq!(storage.get_war(10).unwrap().unwrap().name, "original");
    let overwrite =
        json!({"id": 10, "name": "changed", "cache_value": 42, "status": "active", "data": {}});
    let request = Request::builder()
        .method("POST")
        .uri("/api/wars")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", "r".repeat(32)))
        .body(Body::from(overwrite.to_string()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(storage.get_war(10).unwrap().unwrap().name, "original");
    for token in ["r".repeat(32), "w".repeat(32)] {
        let request = Request::builder()
            .uri("/api/wars")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn gym_api_rejects_bad_inputs_before_worker_admission() {
    let app = create_router(AppState {
        storage: Database::open_in_memory().unwrap(),
        torn: TornClient::new_mock(),
        faction_id: 50645,
        access: access(None),
    });
    let mut payload = json!({"stat":"strength", "start_stat":100, "start_happy":100,
        "energy_available":10, "energy_per_train":0, "gym_dots":7});
    for step in [0.0, -1.0, 0.000001] {
        payload["energy_per_train"] = json!(step);
        let request = Request::builder()
            .method("POST")
            .uri("/api/gym/simulate")
            .header("authorization", format!("Bearer {}", "w".repeat(32)))
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
    }
    payload["energy_per_train"] = json!(1);
    payload["energy_available"] = json!(10001);
    let request = Request::builder()
        .method("POST")
        .uri("/api/gym/simulate")
        .header("authorization", format!("Bearer {}", "w".repeat(32)))
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn access_configuration_fails_closed() {
    assert!(AccessTokens::new("short".into(), "w".repeat(32), None).is_err());
    assert!(AccessTokens::new("r".repeat(32), "r".repeat(32), None).is_err());
    assert!(AccessTokens::new(
        "r".repeat(32),
        "w".repeat(32),
        Some("https://trusted.invalid/path")
    )
    .is_err());
    assert!(AccessTokens::new(
        "r".repeat(32),
        "w".repeat(32),
        Some("https://trusted.invalid")
    )
    .is_ok());
}

#[tokio::test]
async fn private_records_require_authorization_before_read_or_write() {
    let storage = Database::open_in_memory().unwrap();
    let app = create_router(AppState {
        storage: storage.clone(),
        torn: TornClient::new_mock(),
        faction_id: 50645,
        access: access(None),
    });
    let war =
        json!({"id": 10, "name": "private", "cache_value": 42, "status": "active", "data": {}});
    let write = Request::builder()
        .method("POST")
        .uri("/api/wars")
        .header("content-type", "application/json")
        .body(Body::from(war.to_string()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(write).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(
        storage.get_war(10).unwrap().is_none(),
        "unauthorized write changed storage"
    );
    let disbursement = json!({"news_id":"unauthorized-1", "war_id":null,
        "recipient_torn_id":123, "recipient_name":"recipient", "amount":999,
        "occurred_at":null, "matched_register_id":null, "raw_text":"bank"});
    let write = Request::builder()
        .method("POST")
        .uri("/api/disbursements")
        .header("content-type", "application/json")
        .body(Body::from(disbursement.to_string()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(write).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(storage.list_disbursements(None).unwrap().is_empty());
    for uri in [
        "/api/wars",
        "/api/disbursements",
        "/api/balances",
        "/api/wars/10",
    ] {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED,
            "{uri}"
        );
    }
}

#[tokio::test]
async fn browser_origin_is_not_permissive() {
    let app = create_router(AppState {
        storage: Database::open_in_memory().unwrap(),
        torn: TornClient::new_mock(),
        faction_id: 50645,
        access: access(Some("https://trusted.invalid")),
    });
    let request = Request::builder()
        .method("OPTIONS")
        .uri("/api/wars")
        .header("origin", "https://attacker.invalid")
        .header("access-control-request-method", "POST")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert!(response
        .headers()
        .get("access-control-allow-origin")
        .is_none());
    let request = Request::builder()
        .method("OPTIONS")
        .uri("/api/wars")
        .header("origin", "https://trusted.invalid")
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type",
        )
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        "https://trusted.invalid"
    );
}

#[tokio::test]
async fn test_full_router_integration() {
    let storage = Database::open_in_memory().unwrap();
    let torn = TornClient::new_mock();
    let state = AppState {
        storage,
        torn,
        faction_id: 50645,
        access: access(None),
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
                .header("authorization", format!("Bearer {}", "r".repeat(32)))
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
                .header("authorization", format!("Bearer {}", "w".repeat(32)))
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
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
                .header("authorization", format!("Bearer {}", "w".repeat(32)))
                .body(Body::from(val_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        val_json["total_value"],
        18028400000i64 + 50000000 + 25000000
    );

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
                .header("authorization", format!("Bearer {}", "w".repeat(32)))
                .body(Body::from(gym_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let gym_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(gym_json["trains"], 10);

    // 6. Reconcile parse endpoint
    let news_payload = json!({
        "news": [{
            "id": "1001",
            "timestamp": 1720000000,
            "text": "<a href=\"profiles.php?XID=100\">Banker</a> increased <a href=\"profiles.php?XID=2532285\">MrRazorlove</a>'s money balance by $74,978,810 from $0 to $74,978,810."
        }]
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/reconcile/parse")
                .header("Content-Type", "application/json")
                .header("authorization", format!("Bearer {}", "w".repeat(32)))
                .body(Body::from(news_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed_json.as_array().unwrap().len(), 1);
    assert_eq!(parsed_json[0]["amount"], 74978810);

    // 7. Reconcile match endpoint
    let match_payload = json!({
        "gives": [
            GiveForMatch { news_id: "1001".to_string(), recipient_torn_id: 2532285, amount: 74978810, occurred_at: None }
        ],
        "register": [
            RegisterRowForMatch { id: 1, torn_id: 2532285, amount_owed: 74978810, paid: false }
        ]
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/reconcile/match")
                .header("Content-Type", "application/json")
                .header("authorization", format!("Bearer {}", "w".repeat(32)))
                .body(Body::from(match_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let match_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(match_json["matched"], 1);
    assert_eq!(match_json["decisions"][0]["matched_register_id"], 1);
}
