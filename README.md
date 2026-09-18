# MobHub

Ultra-high-performance native Rust and Mojo rewrite of the MobHub faction operations engine for Torn City.

Replaces the legacy Node.js/Next.js implementation with compiled native binaries, embedded SQLite persistence, and sub-millisecond execution.

## Architectural Highlights

- **Pure Native Execution**: Built with native Rust (Axum, Tokio, SQLite) and Modular Mojo. Zero Node.js or Python daemons required.
- **MobPay Payout Engine**: Deterministic calculation engine supporting flat-rate respect-based and pool-share hit-based models. Bit-for-bit validated against GanjaWarriors ranked war #40711 ground truth data. Zero rounding drift reconciliation.
- **Disbursement & Money-Log Reconciliation**: Parses Torn depositFunds money-log news entries (with automatic OC-cut exclusion) and matches them against payout register rows to record and confirm paid status.
- **MobWorth Valuation**: Instant faction net worth calculations parsing spendable vault funds and armory holdings.
- **MobGym Mojo Kernel**: High-speed numerical calculations implementing Vladar Training Formula V2.0 for single-train and multi-train sessions with happy decay.
- **Hardware TPM Key Vault**: Dynamic in-memory credential resolution (atlas-vault get TORN_API_KEY). Zero plaintext credentials stored on disk.
- **Embedded Web Dashboard**: Self-contained web UI served directly from Axum with zero external CDN or npm package dependencies.

## Installation and Build

### Prerequisites
- Rust toolchain (1.80+)
- Modular Mojo (for gym trajectory compilation)
- SQLite3

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
| `GET` | `/status` | TPM vault status and runtime telemetry |
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
| `GET` | `/api/balances` | List member vault balances |
| `GET` | `/` | Embedded web management dashboard |

## Test Verification

Run all unit and integration tests:
```bash
cargo test --verbose
```

All calculations are tested against real faction ground truth data and reference gym formula values.

## License

MIT OR Apache-2.0
