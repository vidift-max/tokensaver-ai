# Team Room Performance Benchmark Lab — provisioning contract

This directory contains reproducible load probes and a disposable fixture seeder. **Mission 9.2 prepares the lab only; it does not run a benchmark.** A successful CI run, build, syntax check or integration test is not a performance result.

## 1. Services and network contract

Required for a real staging run:

| Service | Requirement | Connection |
|---|---|---|
| Team Room Axum HTTP service | Same code SHA being measured, healthy and reachable | `TEAM_ROOM_API_URL`, normally HTTPS; API port exposed to benchmark runner |
| PostgreSQL | Dedicated disposable/staging database, same dataset as recorded in manifest | `DATABASE_URL`, normally TCP 5432 or provider-assigned TLS port |
| Benchmark runner | Node.js 22+; PostgreSQL client (`psql`) for SQL profiling and fixture seeding | Outbound access to API and DB |
| WebSocket endpoint | Same service origin; upgrade permitted through proxy/load balancer | Derived from `TEAM_ROOM_API_URL`, HTTP(S) -> WS(S) |
| Optional staging proxy | Must support WebSocket upgrade and preserve auth headers | Explicitly document proxy path/port |

The API service and PostgreSQL must be the same staging deployment/data plane. Do not benchmark a local API against an unrelated database. Never point the fixture seeder at production.

## 2. Required credentials and variables

Required:
- `TEAM_ROOM_API_URL`: base origin of the Team Room service, without a trailing slash.
- `TEAM_ROOM_ROOM_ID`: UUID of the pre-seeded target room.
- `TEAM_ROOM_SESSION_TOKEN`: one short-lived bearer session for local single-participant runs, **or** `TEAM_ROOM_SESSION_TOKENS_JSON`: JSON array of distinct room-bound tokens for multi-participant runs.
- `DATABASE_URL`: direct connection string for the dedicated benchmark PostgreSQL database; use TLS where staging requires it.

Required grants:
- HTTP read routes: `room.read`, `task.read`, `attachment.read`.
- Message writes: `room.write`.
- WebSocket ticket/connection: active room membership and `room.read`; fanout writes additionally need `room.write`.
- Heartbeat: active authenticated session and valid workload/session binding.

The SQL profiler also requires PostgreSQL credentials with read access to the listed Team Room tables and visibility into permitted `pg_stat_*` views. Fixture seeding requires INSERT on the fixture tables, ANALYZE, and permission to connect. Use a dedicated disposable database and a dedicated benchmark principal. The seeder stores token digests only; raw short-lived tokens are written to Actions' masked job environment or a local env file with mode 0600.

Never commit `.env`, session tokens, database URLs, client secrets or token JSON. Never place credentials in `BENCH_CONFIG_FILE`.

## 3. PostgreSQL and code provenance

- PostgreSQL major version must be recorded; the current isolated workflow pins PostgreSQL 16.
- Record `server_version`, relevant `pg_settings`, dataset cardinalities and exact `GITHUB_SHA` / `BENCH_CODE_SHA`.
- The benchmark runner must use the exact source SHA that built the Team Room binary. For a staging deployment, expose the deployed SHA through deployment metadata and compare it before starting.
- Do not compare runs across PostgreSQL major versions, schema revisions, dataset cardinalities or materially different settings without labeling the comparison.

## 4. Dataset contract

The included isolated fixture defaults to:
- 20 tenants and 20 rooms;
- 8 distinct participants, active memberships, sessions and workload executions in the target room;
- 4 noise participants in 4 separate rooms (when tenant count permits);
- 10,000 target-room messages;
- 250 tasks, 250 decisions and 250 attachment references;
- one general channel in the target room;
- permission rows for the target and noise participants.

This is a deterministic cardinality profile, but generated UUIDs and timestamps differ per seed. The fixture seeder runs ANALYZE. Record the exact counts from the metadata file and SQL cardinality query; do not assume configured counts equal actual rows if seeding failed. For staging, use a dedicated disposable database; never overwrite production or a shared user dataset.

The seed script runs against an existing migrated Team Room schema. It does not start PostgreSQL, run migrations, start the API, provision an external OIDC provider, or create cloud infrastructure. Those remain staging/provisioning responsibilities.

## 5. Explicit workload configuration

Copy `benchmark-config.example.json` to a private local file and pass `BENCH_CONFIG_FILE=/secure/path/bench.json`. Environment variables override JSON values. Secrets are excluded from the JSON contract and must be supplied through environment/secret injection.

| Parameter | Variable | Meaning |
|---|---|---|
| Concurrent HTTP workers / WS sockets | `BENCH_CLIENTS` | Parallel HTTP workers; number of WebSocket connections |
| Distinct participants | `BENCH_PARTICIPANTS` | Unique room-bound participant sessions; use one token per client for independent participants |
| Per-read endpoint request count | `BENCH_EVENTS` | Fixed requests to each HTTP read/heartbeat route |
| Message writes | `BENCH_MESSAGES` | Unique message writes |
| Maximum duration | `BENCH_DURATION_SECONDS` | Optional time cap; 0 disables the cap and uses request-count mode |
| Aggregate arrival-rate cap | `BENCH_RATE_PER_SECOND` | 0 = uncapped; otherwise aggregate scheduled operations per second |
| Message body size | `BENCH_MESSAGE_BYTES` | UTF-8 payload target size in bytes (ASCII padding is used) |
| Request timeout | `BENCH_TIMEOUT_MS` | HTTP request timeout |
| Heartbeat writes | `BENCH_HEARTBEATS` | WebSocket probe heartbeat count |
| Fanout settle window | `BENCH_SETTLE_MS` | Wait after writes before final delivery accounting |
| Other-room noise | `BENCH_IRRELEVANT_WRITES_PER_ROOM` | Writes per other room during target-room sockets |
| Fixture cardinality | `BENCH_TENANTS`, `BENCH_SEED_MESSAGES`, etc. | Seeder dataset dimensions |

The HTTP probe uses a fixed request-count plan, optionally capped by duration and paced by the aggregate rate gate. The WebSocket probe uses fixed write counts, optionally capped and paced. Record requested and actually completed operations; do not describe a capped run as having completed the full plan.

### Endpoints measured

HTTP:
- `GET /team-room/{room_id}/messages?limit=50` (history response includes unread data);
- `GET /team-room/{room_id}/presence`;
- `POST /team-room/heartbeat`;
- `GET /team-room/{room_id}/participants`;
- `GET /team-room/{room_id}/tasks`;
- `GET /team-room/{room_id}/decisions`;
- `GET /team-room/{room_id}/attachments`;
- optional `POST /team-room/{room_id}/messages`.

WebSocket:
- `POST /team-room/{room_id}/ws-ticket`;
- `GET /team-room/{room_id}/ws?ticket=...` (upgrade);
- message creation through the HTTP message endpoint, measured as WebSocket delivery to all target-room sockets;
- optional heartbeat and other-room noise writes.

PostgreSQL profiling:
- message history, unread count, presence snapshot, task list, decision list, attachment references;
- `EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS)`;
- database waits/locks snapshot and before/after statistics from `pg_stat_user_tables`, `pg_stat_database`, `pg_stat_wal`, `pg_stat_activity`, `pg_locks`.

## 6. Dataset seeding

Local disposable staging:
```bash
export DATABASE_URL='postgres://<benchmark-user>:<password>@<host>:5432/<database>'
export BENCH_ENV_FILE="$PWD/.private/benchmark-session.env"
mkdir -p .private && chmod 700 .private
node scripts/benchmark/seed-team-room.mjs
chmod 600 "$BENCH_ENV_FILE"
```
Load the resulting env file into the current shell without printing it (e.g. `set -a; . "$BENCH_ENV_FILE"; set +a`). The file contains raw short-lived session tokens; delete it after the run. The local seeder refuses to overwrite an existing env file.

In GitHub Actions, the script writes tokens only to `GITHUB_ENV`, masks them, and does not put them in artifacts. The workflow uses a disposable PostgreSQL 16 service and isolated service process. The seeder requires the Team Room schema to have been migrated before it runs.

## 7. Reproducible execution

Before execution, create an evidence directory with:
- `run-manifest.json`: run ID, UTC timestamp, exact source/deployed SHA, branch, API origin (no credentials), PostgreSQL server version/settings, configuration, dataset counts, runner OS/Node/psql versions;
- raw `http-*.json`, `websocket-*.json`, `postgres-explain.txt`, before/after stats JSON;
- sanitized service logs, resource samples and failure samples;
- final report with hashes of every raw artifact.

Use separate repetitions and do not combine HTTP latency with WebSocket delivery latency. Keep all raw output; do not only preserve percentile summaries.

## 8. Measurement definitions and pass/fail semantics

- p50/p95/p99: nearest-rank percentile of sorted per-request or per-delivery latency samples; index `ceil(p*n)-1`. Report sample count and unit.
- Throughput: completed successful operations divided by monotonic wall-clock duration. For HTTP also report attempted RPS and errors; for fanout report observed deliveries/sec and message writes/sec separately.
- Error rate: failed attempts / total attempts; timeouts, non-2xx, malformed responses and runtime failures count as errors. Keep status/error samples.
- Fanout: expected deliveries = accepted target-room messages × connected target-room sockets; observed = unique (message_id, socket) pairs. Missing = expected − observed; duplicates are repeated deliveries for the same pair. A run with failed writes or sockets is incomplete and must not hide the shortfall.
- Heartbeat amplification: report accepted heartbeat writes, emitted PRESENCE_CHANGED frames, expected frames under the current topology, missing frames, and frames per accepted heartbeat. This is an observed frame ratio, not a universal cost multiplier.
- Locks: report active waiters, ungranted locks and wait-event snapshots; a snapshot is not a complete history of transient waits.
- Buffers: report shared hit/read counts from EXPLAIN BUFFERS and database block hit/read deltas; distinguish per-query plan counters from database-wide counters.
- WAL: report WAL bytes/records/FPI deltas from before/after snapshots; note other concurrent writers invalidate attribution.
- EXPLAIN ANALYZE: retain the complete plan including actual rows, loops, planning/execution time, buffers, WAL and settings. Run only on isolated benchmark data because ANALYZE executes the statement.
- Leases/fencing under load: not measured by these three probes. Do not infer lease throughput/latency from Runner integration tests. A separate authorized workload probe and explicit concurrency scenario must be designed before claiming lease/fencing performance.
- Pass/fail: technical validity requires no missing/duplicate fanout deliveries, no unexpected endpoint failures, complete provenance and stable environment. Performance SLO thresholds must be set by product owners from user-facing objectives and capacity targets; this lab intentionally defines no arbitrary latency or throughput target.

## 9. CI separation and limitations

The isolated benchmark job is a separate manual workflow (`.github/workflows/team-room-performance-benchmark.yml`), not part of the normal build/test CI. It starts a disposable PostgreSQL 16 service and local Team Room binary, seeds the fixture and preserves raw artifacts. This validates the benchmark harness under an ephemeral CI topology; it is **not a production/staging benchmark** and does not prove external network, proxy, OIDC-provider or AI-provider performance.

For a real staging run, provision the API and DB outside GitHub Actions, then run the probes from a runner with access to both. Do not store credentials in repository files or artifacts.

## 10. Safety and execution gates

- Never run against production without written approval, a bounded traffic window and explicit rate cap.
- Use a dedicated disposable DB and short-lived tokens with least-privilege room grants.
- Verify deployed SHA and PostgreSQL version before running.
- Abort on unexpected 401/403, non-2xx spikes, schema mismatch or fixture contamination.
- A build, syntax check, CI pass or integration test is not a real benchmark result.
