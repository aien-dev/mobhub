//! Ultra-low-latency Axum HTTP service and embedded dashboard UI.

use crate::gym::{self, SessionParams};
use crate::payout::{self, PayoutOptions, PayoutPolicy, PayoutResult, MemberInput};
use crate::reconcile::{self, GiveForMatch, ParsedDisbursement, ReconcileSummary, RegisterRowForMatch};
use crate::storage::{Database, StoredDisbursement, StoredWar};
use crate::torn::TornClient;
use crate::valuation::{self, ValuationBreakdown};
use crate::vault;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub storage: Database,
    pub torn: TornClient,
    pub faction_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct PayoutComputeRequest {
    pub cache_value: i64,
    pub policy: PayoutPolicy,
    pub members: Vec<MemberInput>,
    #[serde(default)]
    pub options: PayoutOptions,
}

#[derive(Debug, Deserialize)]
pub struct ValuationRequest {
    pub respect: f64,
    pub respect_rate: f64,
    pub armory_value: f64,
    pub faction_cash: f64,
}

#[derive(Debug, Deserialize)]
pub struct CsvExportRequest {
    pub result: PayoutResult,
    pub faction_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ReconcileMatchRequest {
    pub gives: Vec<GiveForMatch>,
    pub register: Vec<RegisterRowForMatch>,
    #[serde(default)]
    pub tolerance_pct: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct DisbursementsQuery {
    pub war_id: Option<i64>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/health", get(health_handler))
        .route("/status", get(status_handler))
        .route("/api/payout/compute", post(payout_compute_handler))
        .route("/api/payout/export-csv", post(payout_csv_handler))
        .route("/api/valuation/compute", post(valuation_compute_handler))
        .route("/api/gym/simulate", post(gym_simulate_handler))
        .route("/api/reconcile/parse", post(reconcile_parse_handler))
        .route("/api/reconcile/match", post(reconcile_match_handler))
        .route("/api/disbursements", get(list_disbursements_handler).post(save_disbursements_handler))
        .route("/api/wars", get(list_wars_handler).post(create_war_handler))
        .route("/api/wars/:id", get(get_war_handler))
        .route("/api/balances", get(list_balances_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(state))
}

pub async fn run_server(port: u16, state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let app = create_router(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("MobHub native server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health_handler() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "mobhub",
        "engine": "native-rust-axum",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn status_handler() -> impl IntoResponse {
    let tpm_active = vault::is_torn_key_present();
    Json(json!({
        "status": "ok",
        "service": "mobhub",
        "tpm_vault_active": tpm_active,
        "runtime": "native-arm64-rust",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn payout_compute_handler(
    Json(req): Json<PayoutComputeRequest>,
) -> Result<Json<PayoutResult>, (StatusCode, Json<serde_json::Value>)> {
    let res = payout::compute_payout(req.cache_value, &req.policy, &req.members, &req.options);
    Ok(Json(res))
}

async fn payout_csv_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CsvExportRequest>,
) -> impl IntoResponse {
    let fid = req.faction_id.unwrap_or(state.faction_id);
    let csv_content = payout::export_payout_csv(&req.result, fid);

    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"payout.csv\"",
            ),
        ],
        csv_content,
    )
}

async fn valuation_compute_handler(
    Json(req): Json<ValuationRequest>,
) -> Json<ValuationBreakdown> {
    let res = valuation::compute_valuation(
        req.respect,
        req.respect_rate,
        req.armory_value,
        req.faction_cash,
    );
    Json(res)
}

async fn gym_simulate_handler(
    Json(req): Json<SessionParams>,
) -> Json<gym::SessionResult> {
    let res = gym::simulate_session(&req);
    Json(res)
}

async fn reconcile_parse_handler(
    Json(news_payload): Json<serde_json::Value>,
) -> Json<Vec<ParsedDisbursement>> {
    let disbursements = reconcile::parse_disbursements(&news_payload);
    Json(disbursements)
}

async fn reconcile_match_handler(
    Json(req): Json<ReconcileMatchRequest>,
) -> Json<ReconcileSummary> {
    let tol = req.tolerance_pct.unwrap_or(0.005);
    let summary = reconcile::match_disbursements(&req.gives, &req.register, tol);
    Json(summary)
}

async fn list_disbursements_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DisbursementsQuery>,
) -> Result<Json<Vec<StoredDisbursement>>, (StatusCode, String)> {
    state
        .storage
        .list_disbursements(query.war_id)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct SaveDisbursementPayload {
    pub news_id: String,
    pub war_id: Option<i64>,
    pub recipient_torn_id: i64,
    pub recipient_name: Option<String>,
    pub amount: i64,
    pub occurred_at: Option<String>,
    pub matched_register_id: Option<i64>,
    pub raw_text: String,
}

async fn save_disbursements_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SaveDisbursementPayload>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    state
        .storage
        .save_disbursement(
            &payload.news_id,
            payload.war_id,
            payload.recipient_torn_id,
            payload.recipient_name.as_deref(),
            payload.amount,
            payload.occurred_at.as_deref(),
            payload.matched_register_id,
            &payload.raw_text,
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((StatusCode::CREATED, Json(json!({ "saved": true, "news_id": payload.news_id }))))
}

async fn list_wars_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<StoredWar>>, (StatusCode, String)> {
    state
        .storage
        .list_wars()
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

#[derive(Debug, Deserialize)]
struct CreateWarPayload {
    id: i64,
    name: String,
    cache_value: i64,
    status: String,
    data: serde_json::Value,
}

async fn create_war_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateWarPayload>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    let data_str = payload.data.to_string();
    state
        .storage
        .save_war(
            payload.id,
            &payload.name,
            payload.cache_value,
            &payload.status,
            &data_str,
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "recorded": true, "war_id": payload.id })),
    ))
}

async fn get_war_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<StoredWar>, (StatusCode, String)> {
    match state.storage.get_war(id) {
        Ok(Some(w)) => Ok(Json(w)),
        Ok(None) => Err((StatusCode::NOT_FOUND, "War not found".to_string())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn list_balances_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::storage::StoredBalance>>, (StatusCode, String)> {
    state
        .storage
        .list_balances()
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn dashboard_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let tpm_active = vault::is_torn_key_present();
    let status_badge = if tpm_active {
        r#"<span class="badge active">TPM Vault Active</span>"#
    } else {
        r#"<span class="badge warn">Vault Unbound</span>"#
    };

    let html = format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>MobHub: Sovereign Faction Ops</title>
    <style>
        :root {{
            --bg: #0d1117;
            --surface: #161b22;
            --border: #30363d;
            --text: #c9d1d9;
            --text-dim: #8b949e;
            --accent: #238636;
            --accent-glow: rgba(35, 134, 54, 0.4);
            --gold: #d29922;
        }}
        * {{ box-sizing: border-box; margin: 0; padding: 0; }}
        body {{
            background: var(--bg);
            color: var(--text);
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, monospace;
            line-height: 1.5;
            padding: 2rem;
        }}
        .header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            border-bottom: 1px solid var(--border);
            padding-bottom: 1rem;
            margin-bottom: 2rem;
        }}
        h1 {{
            font-size: 1.5rem;
            color: #fff;
            letter-spacing: -0.5px;
        }}
        .badge {{
            padding: 0.25rem 0.6rem;
            border-radius: 9999px;
            font-size: 0.75rem;
            font-weight: 600;
            text-transform: uppercase;
        }}
        .badge.active {{
            background: var(--accent-glow);
            color: #3fb950;
            border: 1px solid #2ea043;
        }}
        .badge.warn {{
            background: rgba(210, 153, 34, 0.2);
            color: #d29922;
            border: 1px solid #bb8009;
        }}
        .container {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
            gap: 1.5rem;
        }}
        .card {{
            background: var(--surface);
            border: 1px solid var(--border);
            border-radius: 6px;
            padding: 1.25rem;
        }}
        h2 {{
            font-size: 1.1rem;
            margin-bottom: 1rem;
            color: #fff;
            border-bottom: 1px solid var(--border);
            padding-bottom: 0.5rem;
        }}
        label {{
            display: block;
            font-size: 0.8rem;
            color: var(--text-dim);
            margin-top: 0.8rem;
            margin-bottom: 0.2rem;
        }}
        input, select {{
            width: 100%;
            background: var(--bg);
            border: 1px solid var(--border);
            color: #fff;
            padding: 0.5rem;
            border-radius: 4px;
            font-family: inherit;
        }}
        button {{
            margin-top: 1rem;
            padding: 0.6rem 1.2rem;
            background: var(--accent);
            color: white;
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-weight: 600;
            width: 100%;
        }}
        button:hover {{
            filter: brightness(1.1);
        }}
        .result {{
            margin-top: 1rem;
            background: var(--bg);
            border: 1px solid var(--border);
            padding: 0.75rem;
            border-radius: 4px;
            font-family: monospace;
            font-size: 0.85rem;
            white-space: pre-wrap;
            max-height: 250px;
            overflow-y: auto;
        }}
    </style>
</head>
<body>
    <div class="header">
        <h1>MobHub Sovereign Faction Suite</h1>
        <div>
            {status_badge}
            <span style="margin-left: 0.8rem; font-size: 0.85rem; color: var(--text-dim);">Faction: #{faction_id}</span>
        </div>
    </div>
    <div class="container">
        <!-- MobPay Calculator -->
        <div class="card">
            <h2>MobPay Payout Engine</h2>
            <label>War Cache Value ($)</label>
            <input type="number" id="payoutCache" value="349499999">
            <label>Model</label>
            <select id="payoutModel">
                <option value="flat_rate">Flat Rate (Respect Based)</option>
                <option value="pool_share">Pool Share (Hit Based)</option>
            </select>
            <label>Members Split (0.0 - 1.0)</label>
            <input type="number" step="0.01" id="splitMembers" value="0.8">
            <label>Faction Split (0.0 - 1.0)</label>
            <input type="number" step="0.01" id="splitFaction" value="0.2">
            <button onclick="runPayout()">Compute Payout</button>
            <div class="result" id="payoutResult">Ready.</div>
        </div>

        <!-- MobWorth Valuation -->
        <div class="card">
            <h2>MobWorth Faction Valuation</h2>
            <label>Faction Respect</label>
            <input type="number" id="valRespect" value="100000">
            <label>Respect Rate ($/respect)</label>
            <input type="number" id="valRate" value="180284">
            <label>Armory Value ($)</label>
            <input type="number" id="valArmory" value="50000000">
            <label>Spendable Faction Cash ($)</label>
            <input type="number" id="valCash" value="25000000">
            <button onclick="runValuation()">Calculate Worth</button>
            <div class="result" id="valResult">Ready.</div>
        </div>

        <!-- MobGym Projections -->
        <div class="card">
            <h2>MobGym Projections</h2>
            <label>Stat Type</label>
            <select id="gymStat">
                <option value="strength">Strength</option>
                <option value="speed">Speed</option>
                <option value="dexterity">Dexterity</option>
                <option value="defense">Defense</option>
            </select>
            <label>Current Stat</label>
            <input type="number" id="gymStartStat" value="1000000">
            <label>Current Happy</label>
            <input type="number" id="gymHappy" value="5000">
            <label>Energy to Spend</label>
            <input type="number" id="gymEnergy" value="1000">
            <label>Gym Dots (/10)</label>
            <input type="number" step="0.1" id="gymDots" value="7.3">
            <button onclick="runGym()">Simulate Session</button>
            <div class="result" id="gymResult">Ready.</div>
        </div>
    </div>

    <script>
        async function runPayout() {{
            const cache = parseInt(document.getElementById('payoutCache').value);
            const model = document.getElementById('payoutModel').value;
            const splitM = parseFloat(document.getElementById('splitMembers').value);
            const splitF = parseFloat(document.getElementById('splitFaction').value);

            const payload = {{
                cache_value: cache,
                policy: {{
                    model: model,
                    rate_per_respect: 180284,
                    rate_per_outside_hit: 500000,
                    rate_per_assist: 250000,
                    weight_war_hit: 1.0,
                    weight_assist: 0.5,
                    weight_outside_hit: 0.5,
                    split_members_pct: splitM,
                    split_faction_pct: splitF,
                    split_leadership_pct: 0.0,
                    leadership_recipient_torn_id: null
                }},
                members: [
                    {{ torn_id: 2532285, name: "MrRazorlove", flat_respect: 413.12, war_hits: 55, assists: 0, outside_hits: 1 }},
                    {{ torn_id: 4187899, name: "BabyBubbles", flat_respect: 180.5, war_hits: 26, assists: 0, outside_hits: 0 }},
                    {{ torn_id: 3455459, name: "MrLoki", flat_respect: 168.01, war_hits: 33, assists: 0, outside_hits: 0 }}
                ]
            }};

            try {{
                const res = await fetch('/api/payout/compute', {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'application/json' }},
                    body: JSON.stringify(payload)
                }});
                const data = await res.json();
                document.getElementById('payoutResult').innerText = JSON.stringify(data, null, 2);
            }} catch (e) {{
                document.getElementById('payoutResult').innerText = 'Error: ' + e;
            }}
        }}

        async function runValuation() {{
            const r = parseFloat(document.getElementById('valRespect').value);
            const rate = parseFloat(document.getElementById('valRate').value);
            const a = parseFloat(document.getElementById('valArmory').value);
            const c = parseFloat(document.getElementById('valCash').value);

            try {{
                const res = await fetch('/api/valuation/compute', {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'application/json' }},
                    body: JSON.stringify({{ respect: r, respect_rate: rate, armory_value: a, faction_cash: c }})
                }});
                const data = await res.json();
                document.getElementById('valResult').innerText = JSON.stringify(data, null, 2);
            }} catch (e) {{
                document.getElementById('valResult').innerText = 'Error: ' + e;
            }}
        }}

        async function runGym() {{
            const stat = document.getElementById('gymStat').value;
            const s = parseFloat(document.getElementById('gymStartStat').value);
            const h = parseFloat(document.getElementById('gymHappy').value);
            const e = parseFloat(document.getElementById('gymEnergy').value);
            const d = parseFloat(document.getElementById('gymDots').value);

            try {{
                const res = await fetch('/api/gym/simulate', {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'application/json' }},
                    body: JSON.stringify({{
                        stat: stat,
                        start_stat: s,
                        start_happy: h,
                        energy_available: e,
                        energy_per_train: 5.0,
                        gym_dots: d,
                        perk_bonuses: []
                    }})
                }});
                const data = await res.json();
                document.getElementById('gymResult').innerText = JSON.stringify(data, null, 2);
            }} catch (err) {{
                document.getElementById('gymResult').innerText = 'Error: ' + err;
            }}
        }}
    </script>
</body>
</html>"#, status_badge = status_badge, faction_id = state.faction_id);

    Html(html)
}
