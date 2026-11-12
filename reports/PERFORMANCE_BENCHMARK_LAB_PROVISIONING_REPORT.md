# MISSION 9.2 — PERFORMANCE BENCHMARK LAB PROVISIONING

**Repository:** `vidift-max/tokensaver-ai`  
**Working branch:** `mission-9/performance-benchmark-lab`  
**PR:** [#6 — Team Room Performance Benchmark Lab](https://github.com/vidift-max/tokensaver-ai/pull/6)  
**Initial SHA:** `23ac3c138562875736f831a5401b457d1dd5c87e`  
**Final SHA:** recorded in the Mission 9.2 commit and GitHub handoff; no self-referential hash embedded  
**Execution status:** no live benchmark traffic sent in this mission.

## 1. Audit and exact environment contract

The lab measures the Team Room Axum HTTP API, WebSocket ticket/upgrade and message fanout, heartbeat/presence behavior, and read-only PostgreSQL query shapes/statistics. It requires Node.js 22+, PostgreSQL client `psql`, a migrated Team Room schema, a running service, network access to API and PostgreSQL, a dedicated database, target room/channel, and room-bound short-lived sessions.

Required variables: `TEAM_ROOM_API_URL`, `TEAM_ROOM_ROOM_ID`, `TEAM_ROOM_SESSION_TOKEN` or `TEAM_ROOM_SESSION_TOKENS_JSON`, and `DATABASE_URL`. Multi-participant load requires distinct active session tokens and membership for each participant. Read/write grants and PostgreSQL permissions are specified in `scripts/benchmark/README.md`.

Measured routes are listed in that runbook. The SQL profiles message history, unread, presence, tasks, decisions and attachment references; stats snapshots include outbox, database, WAL and waits. The current three probes do not measure lease/fencing performance under load.

## 2. Dataset

The included seeder creates an isolated fixture on an already migrated disposable database: 20 tenants/rooms, 8 target-room participants with active sessions/workload executions, up to 4 noise participants in separate rooms, 10,000 messages, 250 tasks, 250 decisions and 250 attachment references. It runs ANALYZE and writes non-secret cardinality metadata. It does not create cloud infrastructure, migrate schema, start the API or configure an external identity provider.

Local seeding now supports a protected `BENCH_ENV_FILE` (0600, no overwrite) as an alternative to GitHub's `GITHUB_ENV`; raw tokens are never printed or stored in artifacts.

## 3. Reproducibility and measurement method

Added a JSON configuration file contract with environment-variable overrides. Added explicit duration cap, aggregate request/write rate cap and message payload byte size to HTTP and WebSocket probes. Fixed-count remains the default when duration/rate are zero. HTTP output records planned vs executed requests, successful throughput and attempted RPS separately; WebSocket records connection/participant counts, accepted writes, expected/observed/missing/duplicate deliveries, message-write throughput and heartbeat frame counts. The run manifest rejects a deployed-SHA mismatch.

The runbook defines percentile nearest-rank, throughput, error rate, fanout, heartbeat amplification, locks, buffers, WAL, EXPLAIN ANALYZE, and what cannot be inferred for lease/fencing. No arbitrary SLO was introduced.

## 4. CI isolation

The benchmark job was removed from normal `ci.yml` and moved into a dedicated `workflow_dispatch` workflow. It uses a disposable PostgreSQL 16 service and the Team Room binary built from the checked-out SHA. This manual workflow is an isolated harness benchmark, not production staging. It does not require or expose production secrets.

## 5. Offline validation

No HTTP, WebSocket or PostgreSQL benchmark was run. No live endpoint, session token or staging DATABASE_URL was supplied in this mission. Offline validation is limited to source-level configuration checks and CI syntax/unit validation; results will be recorded after exact-SHA Actions checks complete.

## 6. Remaining staging requirements

1. Provision a migrated, dedicated PostgreSQL database (PostgreSQL 16 for comparable baseline) and Team Room service built from the same SHA.
2. Make API and DB reachable from one benchmark runner; permit HTTP and WebSocket upgrade plus direct PostgreSQL connection.
3. Seed the disposable dataset using the documented seeder.
4. Issue short-lived room-bound session tokens for distinct participants with least-privilege grants.
5. Record exact deployed SHA, PostgreSQL version/settings and dataset metadata.
6. Run HTTP, WebSocket and SQL profiling only after an explicit staging approval/rate window.

## 7. Exact commands for Mission 9.1

After staging is provisioned and environment variables are securely loaded, set the libpq connection variables (PGHOST, PGPORT, PGUSER, PGPASSWORD, PGDATABASE) through the secret manager; keep DATABASE_URL only in the process environment for manifest capture, never as a command-line argument:

```bash
export BENCH_CONFIG_FILE="$PWD/scripts/benchmark/benchmark-config.example.json"
node scripts/benchmark/team-room-http.mjs > benchmark-artifacts/http.json
node scripts/benchmark/team-room-websocket.mjs > benchmark-artifacts/websocket.json
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 \
  -v tenant_id="$TEAM_ROOM_TENANT_ID" \
  -v room_id="$TEAM_ROOM_ROOM_ID" \
  -v channel_id="$TEAM_ROOM_CHANNEL_ID" \
  -v principal_id="$TEAM_ROOM_PRINCIPAL_ID" \
  -f scripts/benchmark/team-room-postgres.sql > benchmark-artifacts/postgres-explain.txt
```

The example JSON is a baseline configuration, not a performance threshold. Copy it to a private file and tune load only after staging capacity and safety limits are agreed. Record the deployed SHA in the run manifest before execution.

## 8. Evidence and verdict

- Initial SHA: `23ac3c138562875736f831a5401b457d1dd5c87e`
- PR #6 remains open and unmerged.
- No Gateway, AI-Phone, Team Room core, identity, leases/fencing, Runner or other mission files were changed.
- No live benchmark numbers are reported.

**Verdict: BLOCKED** — the reproducible lab contract and harness provisioning are prepared, but the external staging API, credentials and dedicated database are not provisioned/available in this execution context. This mission does not claim READY FOR REAL BENCHMARK.
