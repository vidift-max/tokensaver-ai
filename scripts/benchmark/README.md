# Team Room Performance Benchmark Lab

This lab sends real authenticated HTTP and WebSocket traffic to an already-running Team Room service and profiles a pre-seeded PostgreSQL database. It does not create tenants, identities, permissions, rooms, participants, or seed data.

## Requirements

- Node.js 22+ (built-in `fetch` and `WebSocket`).
- A dedicated benchmark/staging Team Room service and PostgreSQL database.
- A real room-bound session token with the relevant grants:
  - HTTP reads: `room.read`, `task.read`, `attachment.read`.
  - Message writes: `room.write`.
  - Heartbeat: valid authenticated session.
  - WebSocket: `room.read` plus active membership; message fanout additionally needs `room.write`.
- A pre-seeded room. Record tenant, room, channel, principal, participant, session and workload execution counts independently; the probes do not provision them.

Never run load tests against production without an approved traffic window and explicit rate limits. Do not put bearer tokens in command-line arguments or output.

## HTTP

```bash
export TEAM_ROOM_API_URL='https://team-room-benchmark.example'
export TEAM_ROOM_ROOM_ID='<room-uuid>'
export TEAM_ROOM_SESSION_TOKEN='<short-lived-local-session-token>'
export BENCH_CLIENTS=8
export BENCH_EVENTS=100
export BENCH_MESSAGES=100
node scripts/benchmark/team-room-http.mjs > http-results.json
```

The script concurrently exercises message history/unread, presence, heartbeat, participants, tasks, decisions, attachment references, and optionally message writes. It emits total requests, success/error counts, error rate, elapsed time, throughput and per-endpoint p50/p95/p99/max latency. `BENCH_EVENTS` is the number of calls to each read/heartbeat endpoint; `BENCH_MESSAGES` is the number of unique message writes. These are workload parameters, not measured database cardinalities.

## WebSocket fanout

```bash
export BENCH_CLIENTS=8
export BENCH_MESSAGES=25
export BENCH_HEARTBEATS=10
export BENCH_SETTLE_MS=1500
node scripts/benchmark/team-room-websocket.mjs > websocket-results.json
```

The script obtains a distinct single-use ticket for each connection, opens N authenticated sockets to the same room, posts unique general-room messages, and counts expected/observed same-room deliveries, missing deliveries, duplicates, delivery latency and throughput. It can also post heartbeats and count `PRESENCE_CHANGED` frames. All sockets use the same session/principal, so this is a connection/fanout test, **not** a distinct-participant simulation. Multi-room isolation requires separate room-bound identities and is not covered by this one-room probe.

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
