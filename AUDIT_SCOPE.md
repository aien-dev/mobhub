# Audit scope

Written for an outside, independent security audit. Revision described:
`5e1130654715d9b46a8903d40b52d2035233a47c` on `main`, 2026-10-26. Everything below is taken from the repository's own files at
that revision; items marked NOT_RUN were not executed while writing this document.

## What this repository is

A faction operations tool for the online game Torn City, as one Rust binary with a built-in web
dashboard and an SQLite database: war payouts (`src/payout.rs`, 922 lines), money-log
reconciliation (`src/reconcile.rs`), valuation (`src/valuation.rs`), gym simulation
(`src/gym.rs`), a Torn API client not yet called by anything (`src/torn.rs`), storage
(`src/storage.rs`), secret lookup (`src/vault.rs`), the HTTP server and dashboard
(`src/server.rs`, 691 lines) and the CLI (`src/main.rs`). It replaces an earlier Node.js version
and is not part of the AIEN stack. Licence Apache-2.0 WITH LLVM-exception.

## What to review, in priority order

1. `src/server.rs` access control: `AccessTokens::new` and `from_vault` (token presence, length,
   distinctness, origin parsing), `token_matches` (length check then XOR fold), `authorize`
   middleware (write token for any method, read token for `GET` only), and `create_router` (`/`
   and `/health` public; every other route behind `authorize`).
2. CORS in `create_router`: off unless `MOBHUB_MANAGEMENT_ORIGIN` is set; then an exact-match
   predicate, methods `GET` and `POST`, headers `Authorization` and `Content-Type`.
3. Bind address in `run_server` and the CLI: `127.0.0.1` by default; `--bind` overrides. README:
   non-loopback use requires a trusted TLS reverse proxy, because the server speaks plain HTTP.
4. `dashboard_handler`: an HTML page built with `format!` that interpolates `faction_id`; the page
   sends the write token from a password field on API calls. Check how API responses are written
   into the page.
5. `src/storage.rs` (rusqlite, bundled SQLite): statement construction and the war and
   disbursement records.
6. Input validation: gym requests reject non-finite or out-of-range values and more than 10,000
   trains, with at most two simulations on blocking workers (`GYM_SLOTS` semaphore); payout,
   valuation and reconcile handlers take JSON with `f64` and `i64` fields. Check request body size
   limits and overflow in payout arithmetic.
7. `src/vault.rs`: runs `atlas-vault get <KEY>` and falls back to the process environment;
   `redact_secrets`. `src/torn.rs`: reqwest client with a 15 s timeout against
   `https://api.torn.com/v2`.

Tracked under `.github/`: `pull_request_template.md` and, added by the audit-readiness change,
`workflows/ci.yml` (`cargo build --locked --all-targets`, `cargo test --locked`, SPDX SBOM
artifact; actions pinned by commit sha, read-only token). AGENTS.md describes an "AEGIS Defense
Bot" PR review and requires `cargo audit` from contributors; `cargo audit` is not enforced by a
workflow in the tree.

## Trust boundaries

- Untrusted: HTTP clients and every request body; uploaded news and register JSON; Torn API
  responses (once the client is wired in).
- Token holders: read token (GET), write token (GET and POST).
- Trusted: the `atlas-vault` binary on `PATH` and the process environment that supply tokens and
  the Torn key; the local SQLite file.

## Threat model summary

- Assets: the read and write tokens, the Torn API key, the SQLite records of wars, disbursements
  and balances.
- Attackers considered: unauthenticated local or network clients; a browser page on another
  origin; a read-token holder attempting writes; malformed numeric input to the gym engine.
- Defended: fail-closed token startup, scoped tokens, XOR-fold comparison, CORS off by
  default, loopback default bind, gym input bounds and worker cap.
- Not defended / explicitly out of scope: plaintext HTTP (TLS is delegated to a proxy); `POST`
  compute routes need the write token, so a read-only client cannot run computations; no rate
  limiting is described.

## Build from source at a pin

Prerequisites (README): Rust 1.80 or later and a C compiler (SQLite is built through rusqlite's
`bundled` feature). No `rust-toolchain.toml`; the exact version is not pinned.

```sh
cargo build --release
```

## Run every test

NOT_RUN while writing this document.

```sh
cargo test --verbose
```

Unit tests in `src/gym.rs`, `src/payout.rs` (including `test_ganja_warriors_ground_truth`),
`src/reconcile.rs`, `src/storage.rs`, `src/torn.rs`, `src/valuation.rs`, `src/vault.rs`, and
`tests/integration_test.rs` (scoped tokens, gym input rejection, fail-closed access configuration,
authorization before reads and writes, non-permissive browser origin, full router). Expected:
`test result: ok` for each target. `cargo audit` (named in AGENTS.md): NOT_RUN.

CLI and server smoke (`mobhub doctor`, `mobhub serve ...`): NOT_RUN; `serve` needs both tokens
provisioned.

## Known limitations and NOT_RUN

- No tracked CI workflow at this revision.
- The Torn API client has no caller.
- `GET /api/balances` lists balances that no route or command writes yet (README).
- The valuation cash parser is not called (README).
- README: `/status`, `test-vault` and `doctor` report which source supplied the key; they do not
  certify hardware binding for an environment key.

## Dependencies and provenance

- `Cargo.lock` committed (219 packages). README: the lockfile pins rustls-webpki 0.103.16 for
  RUSTSEC-2026-0382.
- Direct dependencies include tokio, axum 0.7, tower, tower-http (cors, trace), serde, serde_json,
  rusqlite 0.31 (bundled), clap, chrono, reqwest 0.12, tracing, uuid, csv, url.
- Run-time: `atlas-vault` on `PATH` (optional; environment fallback).

## Evidence

No receipts are stored in this repository. AGENTS.md asks that pull requests lead with test
output.
