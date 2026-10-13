# MobHub

Native Rust and Mojo rewrite of the MobHub faction operations engine for Torn City.

Replaces the legacy Node.js/Next.js implementation with compiled native binaries and embedded SQLite persistence.

## Architectural Highlights

- **Pure Native Execution**: Built with native Rust (Axum, Tokio, SQLite) and Modular Mojo. Zero Node.js or Python daemons required.
- **MobPay Payout Engine**: Deterministic calculation engine supporting flat-rate respect-based and pool-share hit-based models. `test_ganja_warriors_ground_truth` in `src/payout.rs` checks it against per-member payouts from the GanjaWarriors war (cache 349,499,999), within 2,000 per member and 50 on the total. Rounding drift is reconciled to the pool target by default for pool-share and on request (`reconcile_drift`) for flat-rate.
- **Disbursement & Money-Log Reconciliation**: Parses Torn depositFunds money-log news entries (with automatic OC-cut exclusion) and matches them to unpaid payout register rows (exact amount first, then within a tolerance), reporting a match decision for each entry.
- **MobWorth Valuation**: Faction net worth as respect x rate + armory value + faction cash. Armory value and cash are supplied by the caller; `src/valuation.rs` also has a parser for spendable cash (vault total minus member deposits) that nothing calls yet.
- **MobGym Mojo Kernel**: Vladar Training Formula V2.0 for single-train and multi-train sessions with happy decay, in `mojo/gym_optimizer.mojo` and in native Rust (`src/gym.rs`). `mobhub gym` and `/api/gym/simulate` run a compiled Mojo binary if they find one (`MOJO_GYM_OPTIMIZER_BIN`, or `gym_optimizer` / `mojo/gym_optimizer` next to the executable or in the working directory) and otherwise use the Rust engine. `cargo build` does not compile the Mojo kernel.
- **Hardware TPM Key Vault**: The Torn API key is resolved in memory by running `atlas-vault get TORN_API_KEY` (the vault tool, TPM-bound on the DGX Spark), with the `TORN_API_KEY` environment variable as fallback. MobHub writes no credential files, and log output redacts the key. The Torn API client in `src/torn.rs` is not called by any command or route yet; today the key only drives the vault status shown by `test-vault`, `doctor`, `/status` and the dashboard.
- **Embedded Web Dashboard**: Self-contained web UI served directly from Axum with zero external CDN or npm package dependencies.

## Installation and Build

### Prerequisites
- Rust toolchain (1.80+)
- Modular Mojo (optional, only to build the gym kernel; without it the Rust engine is used)
- A C compiler (SQLite is built in through the `bundled` feature of rusqlite, so no system SQLite is needed)

### Compilation
```bash
cargo build --release
```

Compiled binary will be available at `target/release/mobhub`.

## CLI Usage

### System Diagnostics
```bash
./target/release/mobhub doctor
```

### TPM Vault Verification
```bash
./target/release/mobhub test-vault
```

### Calculate War Payout
```bash
./target/release/mobhub payout --cache 349499999 --model flat_rate
```

`--model` is `flat_rate` or `pool_share`. `--file` takes a JSON list of members and `--out-csv` writes the CSV. Without `--file` it runs on two built-in sample members.

### Calculate Faction Valuation
```bash
./target/release/mobhub valuation --respect 100000 --rate 180284 --armory 50000000 --cash 25000000
```

### Simulate Gym Training Session
```bash
./target/release/mobhub gym --stat strength --start-stat 1000000 --happy 5000 --energy 1000 --dots 7.3
```

### Reconcile Money Log Disbursements
```bash
./target/release/mobhub reconcile --news-file news.json --register-file register.json
```

### Run HTTP Service & Embedded Dashboard
```bash
./target/release/mobhub serve --port 3000 --db-path mobhub.db --faction-id 50645
```

## API Endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/health` | System health check |
| `GET` | `/status` | Vault status, service name and version |
| `POST` | `/api/payout/compute` | Run deterministic war payout calculation |
| `POST` | `/api/payout/export-csv` | Export payout register as CSV |
| `POST` | `/api/valuation/compute` | Calculate faction valuation breakdown |
| `POST` | `/api/gym/simulate` | Simulate gym session via Mojo/Rust engine |
| `POST` | `/api/reconcile/parse` | Parse raw Torn news to disbursements |
| `POST` | `/api/reconcile/match` | Match disbursements to payout register rows |
| `GET` | `/api/disbursements` | List stored disbursements |
| `POST` | `/api/disbursements` | Store disbursements into SQLite |
| `GET` | `/api/wars` | List archived wars from SQLite |
| `POST` | `/api/wars` | Record or import war |
| `GET` | `/api/wars/:id` | Fetch one archived war (404 if not found) |
| `GET` | `/api/balances` | List stored member balances from SQLite (no route or command writes them yet) |
| `GET` | `/` | Embedded web management dashboard |

## Test Verification

Run all unit and integration tests:
```bash
cargo test --verbose
```

The payout engine is tested against GanjaWarriors war data and the gym formula against reference values; valuation, reconciliation, storage, vault redaction and the HTTP router have their own tests.

## License and Governance

Licensed under the **Sovereign Resource Commons License 1.0 (SRCL-1.0)**: Apache-2.0 with the LLVM exception, plus the Sovereign Reciprocal Commons Rider in [LICENSE](LICENSE). The Rider requires public release of direct source modifications deployed as a public or commercial networked service (Section 11) and requires an enterprise above 25 million USD that trains an AI model on this work to publish the resulting model weights (Section 12).
Copyright (c) 2026 Drake Stapleton (Operator & Lead AI Architect) <drake.aien@proton.me>.
All rights reserved.

Authored and architected in cognitive partnership with AIEN (Autonomous Cognitive Architecture operating on the Atlas Framework). All legal rights, title, copyright, and licensing authority reside exclusively in Drake Stapleton.

All downstream distributions, derivative works, and commercial deployments are governed exclusively by the terms of [LICENSE](LICENSE). [CONSTITUTION.md](CONSTITUTION.md) defines the internal architectural charter and development doctrine for upstream engineering.
