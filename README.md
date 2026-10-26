# MobHub

A faction operations tool for the online game Torn City: war payouts, faction valuation, gym training simulation and money-log reconciliation, as one Rust program with a built-in web dashboard and an SQLite database. It replaces an earlier Node.js/Next.js version.

MobHub is a side project kept in the AIEN organization on GitHub. It is not part of the AIEN stack. For the project itself, start at [Aienix](https://github.com/aien-dev/aienix), the core tree of AIEN.

**Status: maintenance.** The gym and payout engines run in Rust. The Torn API client is written but no command or route calls it yet. The service binds loopback by default and requires scoped tokens for API access; the lockfile pins rustls-webpki 0.103.16 for RUSTSEC-2026-0382.

## What it does

- **One Rust program**: built with Axum, Tokio and SQLite. No Node.js or Python is needed to run it.
- **MobPay Payout Engine**: Deterministic calculation engine supporting flat-rate respect-based and pool-share hit-based models. `test_ganja_warriors_ground_truth` in `src/payout.rs` checks it against per-member payouts from the GanjaWarriors war (cache 349,499,999), within 2,000 per member and 50 on the total. Rounding drift is reconciled to the pool target by default for pool-share and on request (`reconcile_drift`) for flat-rate.
- **Disbursement & Money-Log Reconciliation**: Parses Torn depositFunds money-log news entries (with automatic OC-cut exclusion) and matches them to unpaid payout register rows (exact amount first, then within a tolerance), reporting a match decision for each entry.
- **MobWorth Valuation**: Faction net worth as respect x rate + armory value + faction cash. Armory value and cash are supplied by the caller; `src/valuation.rs` also has a parser for spendable cash (vault total minus member deposits) that nothing calls yet.
- **MobGym Engine**: Vladar Training Formula V2.0 for single-train and multi-train sessions with happy decay, in native Rust (`src/gym.rs`). `mobhub gym` and `/api/gym/simulate` both run this engine.
- **Torn API credential source**: `atlas-vault get TORN_API_KEY` is preferred; the `TORN_API_KEY` process environment is a fallback. `/status`, `test-vault` and `doctor` identify which source supplied the key. They do not certify TPM binding for an environment key or inspect disk contents.
- **Embedded Web Dashboard**: a self-contained web page served by the program itself, with no outside CDN or npm packages.

## Installation and Build

### Prerequisites
- Rust toolchain (1.80+)
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

### Torn API Key Source
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

Before serving, provision distinct `MOBHUB_READ_TOKEN` and `MOBHUB_WRITE_TOKEN` values through `atlas-vault get` (or process environment for isolated tests). Each token must contain at least 32 printable ASCII bytes, with no whitespace. Startup fails closed if either is absent, weak or identical. The read token permits `GET /status` and `GET /api/*`; the write token permits both reads and all `POST /api/*`. Send `Authorization: Bearer <token>` on every protected request. `/health` and the static dashboard at `/` require no token; the dashboard holds its write token only in its password field and sends it explicitly on API calls. Do not put tokens in URLs, committed config or browser storage.

The service binds `127.0.0.1` by default:
```bash
./target/release/mobhub serve --port 3000 --db-path mobhub.db --faction-id 50645
```

`--bind` explicitly overrides the address. Non-loopback use requires a trusted TLS reverse proxy and network access controls, since bearer tokens otherwise cross plaintext HTTP. Cross-origin API access is disabled by default; set `MOBHUB_MANAGEMENT_ORIGIN` to one exact trusted `https://host[:port]` origin if a separate management UI needs CORS. The origin is not an authorization grant. The dashboard served from the same origin does not require CORS. Gym requests reject non-finite/out-of-range parameters and sessions exceeding 10,000 trains; at most two gym simulations occupy blocking workers at once.
## API Endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/health` | System health check |
| `GET` | `/status` | Credential source, service name and version (read or write token) |
| `POST` | `/api/payout/compute` | Run deterministic war payout calculation |
| `POST` | `/api/payout/export-csv` | Export payout register as CSV |
| `POST` | `/api/valuation/compute` | Calculate faction valuation breakdown |
| `POST` | `/api/gym/simulate` | Simulate gym session with the Rust engine |
| `POST` | `/api/reconcile/parse` | Parse raw Torn news to disbursements |
| `POST` | `/api/reconcile/match` | Match disbursements to payout register rows |
| `GET` | `/api/disbursements` | List stored disbursements (read or write token) |
| `POST` | `/api/disbursements` | Store disbursements into SQLite (write token) |
| `GET` | `/api/wars` | List archived wars from SQLite (read or write token) |
| `POST` | `/api/wars` | Record or import war (write token) |
| `GET` | `/api/wars/:id` | Fetch one archived war (404 if not found; read or write token) |
| `GET` | `/api/balances` | List stored member balances from SQLite (read or write token; no route or command writes them yet) |
| `GET` | `/` | Public static embedded web dashboard; API calls require write token |

## Test Verification

Run all unit and integration tests:
```bash
cargo test --verbose
```

The payout engine is tested against GanjaWarriors war data and the gym formula against reference values; valuation, reconciliation, storage, vault redaction and the HTTP router have their own tests.

## License and Governance

Licensed under the Apache License, Version 2.0 with LLVM Exceptions (`Apache-2.0 WITH LLVM-exception`); see [LICENSE](LICENSE) and [NOTICE](NOTICE).

Authored by Drake Stapleton with AIEN agents. Contact: drake@aienos.com.

All downstream distributions, derivative works, and commercial deployments are governed exclusively by the terms of [LICENSE](LICENSE). [CONSTITUTION.md](CONSTITUTION.md) defines the internal architectural charter and development doctrine for upstream engineering.
