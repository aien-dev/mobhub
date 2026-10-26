# Security policy

## Reporting a vulnerability

Report privately. Do not open a public issue, pull request or discussion about it.

1. GitHub: open this repository's Security tab and choose "Report a vulnerability". This
   creates a private advisory that only the maintainers can see. If that option is not shown,
   private vulnerability reporting has not been enabled for this repository yet; use email.
2. Email: drake@aienos.com with the subject `[mobhub] security report`.

Include the commit or release you tested, the component (path), steps to reproduce, and what an
attacker gains. If the report involves a secret, say where it was exposed and do not send the
secret itself. Encrypted mail is not required; do not post proof-of-concept payloads publicly.

## Response targets

This project has one maintainer; the targets below are best effort and stated honestly.

- Acknowledgement: within 3 working days.
- Triage and severity: within 7 days of acknowledgement.
- Fix or mitigation for confirmed issues: aim for 30 days for high and critical, 90 days
  otherwise, with a status update at least every 14 days until closed.
- Disclosure: coordinated. An advisory is published with credit (unless you ask otherwise)
  once the fix is on the default branch. Default embargo 90 days, negotiable.

## Scope

MobHub is a Rust program (Axum, Tokio, SQLite) with an embedded web dashboard: war payouts,
faction valuation, gym training simulation and money-log reconciliation for the online game Torn
City. It is a side project kept in the AIEN organization, not part of the AIEN stack (README).

In scope:

- Any protected route reachable without a valid bearer token, or a read token that can perform a
  `POST` (README: the read token permits `GET /status` and `GET /api/*`; the write token permits
  reads and all `POST /api/*`).
- Token handling: startup that accepts missing, weak (under 32 printable ASCII bytes) or identical
  `MOBHUB_READ_TOKEN` and `MOBHUB_WRITE_TOKEN`; tokens written to logs, URLs or browser storage.
- CORS that admits an origin other than the one exact `MOBHUB_MANAGEMENT_ORIGIN`.
- Script injection in the embedded dashboard at `/`.
- SQL injection or unbounded resource use in `src/storage.rs` and the gym, payout and reconcile
  handlers.
- Leakage of the Torn API key (`TORN_API_KEY`) through output, errors or `/status`.

Out of scope:

- The Torn City API and game, any reverse proxy in front of a non-loopback deployment, the
  `atlas-vault` program, and the host operating system.

## Supported versions

Only the default branch `main`; there is no release. Reports against older commits are welcome but fixes land on the default branch only.

## Status

README: status maintenance. The Torn API client is written but no command or route calls it yet.
No certification, no bug bounty. See AUDIT_SCOPE.md for what to review and how to build and test it.
