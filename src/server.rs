//! Ultra-low-latency Axum HTTP service and embedded dashboard UI.

use crate::gym::{self, SessionParams};
use crate::payout::{self, PayoutOptions, PayoutPolicy, PayoutResult, MemberInput};
use crate::storage::{Database, StoredWar};
use crate::torn::TornClient;
use crate::valuation::{self, ValuationBreakdown};
use crate::vault;

use axum::extract::{Path, State};
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

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/health", get(health_handler))
        .route("/status", get(status_handler))
        .route("/api/payout/compute", post(payout_compute_handler))
        .route("/api/payout/export-csv", post(payout_csv_handler))
        .route("/api/valuation/compute", post(valuation_compute_handler))
        .route("/api/gym/simulate", post(gym_simulate_handler))
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
    let res = gym::run_mojo_optimizer(&req);
    Json(res)
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
    <title>MobHub — Sovereign Faction Ops</title>
    <style>
        :root {{
            --bg: #0d1117;
            --surface: #161b22;
            --border: #30363d;
            --accent: #238636;
            --text: #f0f6fc;
            --text-dim: #8b949e;
            --gold: #f1e05a;
        }}
        body {{
            margin: 0;
            padding: 0;
            background: var(--bg);
            color: var(--text);
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
        }}
        .header {{
            background: var(--surface);
            border-bottom: 1px solid var(--border);
            padding: 1rem 2rem;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .header h1 {{
            margin: 0;
            font-size: 1.4rem;
            letter-spacing: 0.5px;
        }}
        .badge {{
            padding: 0.25rem 0.6rem;
            border-radius: 9999px;
            font-size: 0.8rem;
            font-weight: 600;
        }}
        .badge.active {{
            background: rgba(35, 134, 54, 0.2);
            color: #3fb950;
            border: 1px solid #238636;
        }}
        .badge.warn {{
            background: rgba(210, 153, 34, 0.2);
            color: #d29922;
            border: 1px solid #d29922;
        }}
        .container {{
            max-width: 1200px;
            margin: 2rem auto;
            padding: 0 1rem;
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
            gap: 1.5rem;
        }}
        .card {{
            background: var(--surface);
            border: 1px solid var(--border);
            border-radius: 8px;
            padding: 1.5rem;
        }}
        .card h2 {{
            margin-top: 0;
            font-size: 1.15rem;
            border-bottom: 1px solid var(--border);
            padding-bottom: 0.5rem;
        }}
        label {{
            display: block;
            margin: 0.75rem 0 0.25rem;
            color: var(--text-dim);
            font-size: 0.85rem;
        }}
        input, select {{
            width: 100%;
            padding: 0.5rem;
            background: var(--bg);
            border: 1px solid var(--border);
            color: var(--text);
            border-radius: 4px;
            box-sizing: border-box;
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
            <h2>MobGym Mojo Accelerator</h2>
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
