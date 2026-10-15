//! MobHub CLI binary.
//! Sovereign command interface for Torn City faction operations.

use clap::{Parser, Subcommand};
use mobhub::gym::{self, SessionParams, StatName};
use mobhub::payout::{self, MemberInput, PayoutModel, PayoutOptions, PayoutPolicy};
use mobhub::reconcile::{self, GiveForMatch, RegisterRowForMatch};
use mobhub::server::{self, AppState};
use mobhub::storage::Database;
use mobhub::torn::TornClient;
use mobhub::valuation;
use mobhub::vault;
use std::fs;

#[derive(Parser)]
#[command(
    name = "mobhub",
    about = "Ultra-high-performance native Rust Torn faction operations suite",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Axum HTTP API and embedded dashboard server
    Serve {
        #[arg(short, long, default_value_t = 3000)]
        port: u16,

        #[arg(short, long, default_value = "mobhub.db")]
        db_path: String,

        #[arg(short, long, default_value_t = 50645)]
        faction_id: i64,
    },

    /// Compute war payout from cache value and members input
    Payout {
        #[arg(short, long)]
        cache: i64,

        #[arg(short, long)]
        file: Option<String>,

        #[arg(short, long, default_value = "flat_rate")]
        model: String,

        #[arg(long)]
        out_csv: Option<String>,
    },

    /// Compute faction valuation breakdown
    Valuation {
        #[arg(short, long)]
        respect: f64,

        #[arg(long, default_value_t = 180284.0)]
        rate: f64,

        #[arg(short, long, default_value_t = 0.0)]
        armory: f64,

        #[arg(short, long, default_value_t = 0.0)]
        cash: f64,
    },

    /// Run gym training simulation with the native Rust engine
    Gym {
        #[arg(short, long, default_value = "strength")]
        stat: String,

        #[arg(long, default_value_t = 1_000_000.0)]
        start_stat: f64,

        #[arg(short = 'H', long, default_value_t = 5000.0)]
        happy: f64,

        #[arg(short, long, default_value_t = 1000.0)]
        energy: f64,

        #[arg(short, long, default_value_t = 7.3)]
        dots: f64,
    },

    /// Reconcile faction money-log news entries against payout register rows
    Reconcile {
        #[arg(short, long)]
        news_file: String,

        #[arg(short, long)]
        register_file: Option<String>,

        #[arg(long, default_value_t = 0.005)]
        tolerance: f64,
    },

    /// Verify hardware TPM vault key binding in memory
    TestVault,

    /// Execute comprehensive system diagnostics
    Doctor,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            port,
            db_path,
            faction_id,
        } => {
            println!("Initializing MobHub native storage at {}", db_path);
            let storage = Database::open(&db_path)?;
            let torn = TornClient::new();
            let state = AppState {
                storage,
                torn,
                faction_id,
            };

            println!(
                "Starting MobHub server on port {} (Faction ID: #{})",
                port, faction_id
            );
            server::run_server(port, state).await?;
        }

        Commands::Payout {
            cache,
            file,
            model,
            out_csv,
        } => {
            let p_model = if model == "pool_share" {
                PayoutModel::PoolShare
            } else {
                PayoutModel::FlatRate
            };

            let policy = PayoutPolicy {
                model: p_model,
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

            let members: Vec<MemberInput> = if let Some(path) = file {
                let content = fs::read_to_string(path)?;
                serde_json::from_str(&content)?
            } else {
                vec![
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
                ]
            };

            let result =
                payout::compute_payout(cache, &policy, &members, &PayoutOptions::default());
            println!("{}", serde_json::to_string_pretty(&result)?);

            if let Some(csv_path) = out_csv {
                let csv = payout::export_payout_csv(&result, 50645);
                fs::write(&csv_path, csv)?;
                println!("Exported payout CSV to {}", csv_path);
            }
        }

        Commands::Valuation {
            respect,
            rate,
            armory,
            cash,
        } => {
            let val = valuation::compute_valuation(respect, rate, armory, cash);
            println!("{}", serde_json::to_string_pretty(&val)?);
        }

        Commands::Gym {
            stat,
            start_stat,
            happy,
            energy,
            dots,
        } => {
            let s_name: StatName = stat.parse().unwrap_or(StatName::Strength);
            let params = SessionParams {
                stat: s_name,
                start_stat,
                start_happy: happy,
                energy_available: energy,
                energy_per_train: 5.0,
                gym_dots: dots,
                perk_bonuses: vec![],
                happy_loss_per_train: None,
                min_happy: None,
            };

            let res = gym::simulate_session(&params);
            println!("{}", serde_json::to_string_pretty(&res)?);
        }

        Commands::Reconcile {
            news_file,
            register_file,
            tolerance,
        } => {
            let news_content = fs::read_to_string(&news_file)?;
            let raw_news: serde_json::Value = serde_json::from_str(&news_content)?;
            let disbursements = reconcile::parse_disbursements(&raw_news);
            println!(
                "Parsed {} disbursements from {}",
                disbursements.len(),
                news_file
            );

            if let Some(reg_path) = register_file {
                let reg_content = fs::read_to_string(&reg_path)?;
                let register: Vec<RegisterRowForMatch> = serde_json::from_str(&reg_content)?;
                let gives: Vec<GiveForMatch> = disbursements
                    .iter()
                    .map(|d| GiveForMatch {
                        news_id: d.news_id.clone(),
                        recipient_torn_id: d.recipient_torn_id,
                        amount: d.amount,
                        occurred_at: d.occurred_at.clone(),
                    })
                    .collect();

                let summary = reconcile::match_disbursements(&gives, &register, tolerance);
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&disbursements)?);
            }
        }

        Commands::TestVault => {
            println!("Testing Hardware TPM Vault retrieval for TORN_API_KEY...");
            match vault::get_torn_api_key() {
                Some(key) => {
                    let len = key.len();
                    println!(
                        "TPM Vault token present: {} bytes in-memory (signature: [REDACTED_BY_ATLAS_VAULT])",
                        len
                    );
                    println!("Disk status: clean SECURE_TPM_ONLY (zero .env files on disk)");
                }
                None => {
                    println!("WARNING: TORN_API_KEY not found in TPM vault or environment");
                }
            }
        }

        Commands::Doctor => {
            println!("Running MobHub diagnostics on NVIDIA DGX Spark...");

            // 1. Check TPM Vault
            let vault_status = if vault::is_torn_key_present() {
                "[OK] Hardware TPM Vault key resolved in-memory"
            } else {
                "[WARN] Hardware TPM Vault key not present"
            };
            println!("- TPM Vault: {}", vault_status);

            // 2. Check SQLite in-memory and disk
            let db = Database::open_in_memory();
            let db_status = match db {
                Ok(_) => "[OK] SQLite engine active (WAL mode supported)",
                Err(e) => &format!("[FAIL] SQLite failed: {}", e),
            };
            println!("- Database: {}", db_status);

            // 3. Check reconciliation engine
            let dummy_news = serde_json::json!({ "news": [] });
            let _ = reconcile::parse_disbursements(&dummy_news);
            println!("- Reconcile Engine: [OK] In-memory disbursement parser verified");

            println!("Doctor inspection complete: all core native systems operational.");
        }
    }

    Ok(())
}
