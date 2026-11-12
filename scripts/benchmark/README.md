# Team Room Performance Benchmark Lab

The HTTP and WebSocket probes send real authenticated traffic to an already-running Team Room service and the SQL script profiles a pre-seeded PostgreSQL database. The standalone probes do not provision identities or seed data. The dedicated GitHub Actions benchmark job uses `seed-team-room.mjs` to create a disposable, isolated test fixture and short-lived opaque sessions.

## Requirements

- Node.js 22+ (built-in `fetch` and `WebSocket`).
- A dedicated benchmark/staging Team Room service and PostgreSQL database.
- A real room-bound session token with the relevant grants:
  - HTTP reads: `room.read`, `task.read`, `attachment.read`.
  - Message writes: `room.write`.
  - Heartbeat: valid authenticated session.
  - WebSocket: `room.read` plus active membership; message fanout additionally needs `room.write`.
- A pre-seeded room. For multi-participant load, set `TEAM_ROOM_SESSION_TOKENS_JSON` to a JSON array of unique room-bound session tokens; the probes rotate requests across those identities. A single `TEAM_ROOM_SESSION_TOKEN` remains supported for local single-identity checks.
- For the isolated GitHub Actions execution, `scripts/benchmark/seed-team-room.mjs` creates a disposable fixture with 20 tenants/rooms, 8 distinct authenticated participants in the target room, 4 additional room-bound noise participants in 4 separate rooms, 10,000 messages, 250 tasks, 250 decisions and 250 attachment references. It stores only SHA-256 session-token digests in PostgreSQL and exports short-lived raw tokens to the current Actions job environment, never to artifacts.

Never run load tests against production without an approved traffic window and explicit rate limits. Do not put bearer tokens in command-line arguments or output.

## HTTP

```bash
export TEAM_ROOM_API_URL='https://team-room-benchmark.example'
export TEAM_ROOM_ROOM_ID='<room-uuid>'
export TEAM_ROOM_SESSION_TOKENS_JSON='["<participant-1-token>","<participant-2-token>"]'
export BENCH_PARTICIPANTS=2
export BENCH_CLIENTS=2
export BENCH_REQUIRE_DISTINCT_PARTICIPANTS=true
export BENCH_EVENTS=100
export BENCH_MESSAGES=100
node scripts/benchmark/team-room-http.mjs > http-results.json
```

The script concurrently exercises message history/unread, presence, heartbeat, participants, tasks, decisions, attachment references, and optionally message writes. It emits total requests, success/error counts, error rate, elapsed time, throughput and per-endpoint p50/p95/p99/max latency. `BENCH_EVENTS` is the number of calls to each read/heartbeat endpoint; `BENCH_MESSAGES` is the number of unique message writes. These are workload parameters, not measured database cardinalities.

## WebSocket fanout

```bash
export BENCH_CLIENTS=2
export BENCH_PARTICIPANTS=2
export BENCH_REQUIRE_DISTINCT_PARTICIPANTS=true
export BENCH_MESSAGES=25
export BENCH_HEARTBEATS=10
export BENCH_SETTLE_MS=1500
export BENCH_IRRELEVANT_WRITES_PER_ROOM=5
node scripts/benchmark/team-room-websocket.mjs > websocket-results.json
```

The script obtains a distinct single-use ticket for each connection, opens N authenticated sockets to the same room, and posts unique general-room messages using the matching participant session. With `TEAM_ROOM_SESSION_TOKENS_JSON` and `BENCH_REQUIRE_DISTINCT_PARTICIPANTS=true`, each socket must have a unique token/session. It counts expected/observed same-room deliveries, missing deliveries, duplicates, delivery latency and throughput. It can also post heartbeats and count `PRESENCE_CHANGED` frames. If only one token is supplied, connections share one principal and the result must not be described as a distinct-participant test. The automated CI run also generates 5 messages in each of 4 other rooms while the target-room sockets are connected, and asserts that none of those unrelated room events is delivered to the target-room sockets. This measures cross-room event isolation under global-channel noise; it does not open a second set of WebSockets in those other rooms.

## PostgreSQL

Use a dedicated benchmark database seeded to a documented cardinality. Supply real UUIDs:

```bash
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 \
  -v tenant_id='<tenant-uuid>' \
  -v room_id='<room-uuid>' \
  -v channel_id='<general-channel-uuid>' \
  -v principal_id='<principal-uuid>' \
  -f scripts/benchmark/team-room-postgres.sql > postgres-explain.txt
```

The SQL runs read-only `EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS)` plans for the message history, unread, presence, task, decision and attachment-reference query shapes used by the service, then captures a database-wait/lock snapshot. Save the PostgreSQL version, settings, dataset cardinalities, output, and exact code SHA with each run.

The script does not create or modify indexes. Compare plans only on the same PostgreSQL version, configuration, dataset and cache protocol. A single warm-cache run is not a cold-cache comparison.

## Reporting rules

- Preserve raw JSON/text outputs; do not transcribe only favorable percentiles.
- Record API origin (not credentials), code SHA, PostgreSQL version, dataset cardinalities, client/connection count, event/message count, participant count, task count, duration, concurrency and error samples.
- Distinguish HTTP request latency from WebSocket delivery latency.
- Distinguish connections from unique participants.
- A PostgreSQL integration test is not a database benchmark.
- A scheduler unit test is not an HTTP or production benchmark.
- A successful API request is not proof of end-to-end report delivery or external AI dispatch.
- Do not claim a benchmark has run unless raw output and exact-SHA provenance are attached.

## Automated isolated execution

The `team-room-performance-benchmark` job in `.github/workflows/ci.yml` starts a disposable PostgreSQL 16 service and the real Axum Team Room binary, seeds the fixture, then executes three HTTP read/heartbeat repetitions, a message-write workload, eight authenticated WebSocket connections for eight distinct participants, eight heartbeat writes, twenty irrelevant message events across four other rooms, and the PostgreSQL EXPLAIN/statistics snapshots. The fixture runs ANALYZE after seeding so query plans use fresh cardinality statistics. Raw JSON, SQL plans, PostgreSQL counter snapshots and process-resource samples are uploaded as a 14-day Actions artifact. This is a reproducible CI benchmark environment, not a production deployment or external AI-provider test.

The PostgreSQL stats snapshots include `outbox_events.idx_scan`, tuple insert counters, per-event outbox row counts, database block/transaction counters, WAL counters and active waits. Compare the before/after snapshots around the WebSocket window to quantify the global `team_room_outbox` listener fanout in this isolated run.
