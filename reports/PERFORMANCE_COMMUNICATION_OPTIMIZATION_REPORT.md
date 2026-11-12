# Mission 8 — Performance & Communication Optimization Report

**Repository:** `vidift-max/tokensaver-ai`  
**Base branch:** `v2-saas-alpha`  
**Base SHA:** `e18557c65753714bf926845639526b22c0309826`  
**Mission branch:** `mission-8/performance-communication-optimization`  
**PR:** [#4 — Performance & Communication Optimization](https://github.com/vidift-max/tokensaver-ai/pull/4)  
**Implementation candidate SHA:** `4c4da5786cdfa6536167a3d9fb366d4b002cf814`  
**Audit date:** 2026-11-12

## 1. Executive summary

The highest-confidence application bottleneck found was Team Room's WebSocket-to-REST refresh amplification. Every recognized WebSocket event caused a full refresh of five REST resources; concurrent refreshes were possible. The UI has been changed to coalesce event bursts over a 75 ms scheduling window, serialize refreshes, and run one trailing refresh when notifications arrive during an in-flight refresh.

The implementation adds deterministic tests for burst coalescing, non-overlap, trailing refresh and cancellation. CI now caches Rust build dependencies and npm downloads; no test job or assertion was removed.

**No production latency, request-rate, PostgreSQL query-time or CI-duration baseline was available.** The only quantifiable comparison is the deterministic scheduler test's intended synthetic workload (100 notifications in one burst -> one refresh invocation); exact candidate CI has not completed. Therefore, production speedup is not claimed.

**Performance gate: NO-GO pending exact-SHA CI and runtime/DB benchmarks.**

## 2. Baseline

Evidence source: repository code at base SHA `e18557c65753714bf926845639526b22c0309826`, migrations, CI workflow, PR history and source inspection.

| Component | Bottleneck / risk | Impact | Evidence | Priority |
|---|---|---|---|---|
| Team Room frontend | Every WebSocket event triggered all five refresh requests; concurrent refreshes could overlap | High request amplification and stale-response races | `src/TeamRoom.tsx` base implementation | P0 |
| WebSocket fanout | All sockets listen on the same PostgreSQL notification channel; each listener receives global notifications and loads the event row before room filtering | Cross-room event volume can create unnecessary DB reads per connected socket | `communication.rs::websocket_loop`; migration 0009 | P1 |
| Presence heartbeat | Every heartbeat writes a `PRESENCE_CHANGED` outbox event | Heartbeat traffic amplifies notifications and downstream UI refreshes | `main.rs::heartbeat` | P1 |
| Team Room history | Backend keyset pagination exists (default 50, max 100); current UI does not expose older-page loading | History beyond the first page is not browsable; repeated refresh reloads the first page | `communication.rs::history`; `TeamRoom.tsx` | P1 |
| PostgreSQL presence | Presence query contains a correlated latest-task subquery per membership | Potentially expensive as room membership and task count grow | `main.rs::room_presence`; existing owner/lease index | P1 — EXPLAIN required |
| PostgreSQL unread count | History endpoint counts unread messages on each request | Repeated aggregation can grow with channel history | `communication.rs::history` and `private_history` | P1 — EXPLAIN required |
| Outbox / reports | Outbox rows persist and PostgreSQL NOTIFY drives WebSocket delivery; durable replay/publisher and report-delivery pipeline are not present | No independently recoverable report delivery or consumer replay mechanism | migration 0001/0009; implementation report and source | P1 |
| Runner | Remote calls are sequential at lifecycle boundaries; local state/checkpoint metadata and remote authority are reconciled by explicit commands | Mission-to-next-mission handoff can include manual wait and extra round trips | `mission-runner.mjs`, `team-room-client.mjs` | P2 |
| Recovery Worker | Watch mode defaults to 2-second polling and performs recovery POST then candidate GET sequentially per iteration | Repeated DB/API work while waiting for candidates | `team-room-recovery-worker.mjs` | P1 |
| CI | Rust and npm dependencies were not cached; npm uses `npm install`; no package-lock or Cargo.lock is tracked | Repeated dependency download/resolve work and non-reproducible resolution | `.github/workflows/ci.yml`, `package.json`; lockfile fetch returned 404 | P1 |
| Desktop/Tauri | Local blob integrity checks intentionally hash existing files; import uses an immediate SQLite transaction and fsync for blob durability | Potential import latency, but integrity/durability costs are intentional | `src-tauri/src/lib.rs` | P2 — benchmark before changing |

No source-level inspection was treated as a substitute for live load testing, PostgreSQL `EXPLAIN (ANALYZE, BUFFERS)`, or application telemetry.

## 3. Ten-round execution ledger

| Round | Scope | Classification | Outcome |
|---|---|---|---|
| 1 | Baseline performance audit | IMPLEMENTADO / PENDING | Mapped frontend, Axum service, PostgreSQL, WebSocket/outbox, Runner, Recovery Worker, Tauri and CI. No production telemetry or local benchmark environment was available. |
| 2 | Communication audit | IMPLEMENTADO | Identified full refresh on every event, overlapping refreshes, global notification fanout and heartbeat outbox amplification. One safe frontend optimization implemented. |
| 3 | Team Room | IMPLEMENTADO / TESTADO PENDING | Added 75 ms notification coalescing, serialized refreshes and one trailing refresh. Event types and authoritative REST snapshot reads remain unchanged. Automated CI test result pending. |
| 4 | Mission/report delivery | PENDING | Documented the missing durable report-delivery/replay pipeline and manual handoff gap. No speculative queue, delivery semantics or new service was introduced. |
| 5 | Runner/Recovery Worker | IMPLEMENTADO POR AUDITORIA / PENDING | Inspected lease, checkpoint, handoff, heartbeat and recovery paths. No lease/fencing/recovery behavior changed. Poll/backoff optimization deferred pending load tests and server-side API support. |
| 6 | PostgreSQL | PENDING | Reviewed migrations and query shapes. Existing indexes cover message history, task room/status, owner lease, handoff target and unpublished outbox. No new index/query rewrite without live EXPLAIN evidence. |
| 7 | Frontend/Tauri | IMPLEMENTADO PARCIAL | Team Room refresh amplification reduced at source. No offline, storage, Tauri IPC or blob durability behavior changed. |
| 8 | CI/development | IMPLEMENTADO / TESTADO PENDING | Added Rust build cache and npm download cache; enabled prefer-offline/no-audit/no-fund npm install; test command now discovers all `tests/*.test.mjs`. No tests skipped. |
| 9 | Implementation/benchmark/regression | IMPLEMENTADO / TESTADO PENDING | Added deterministic scheduler tests for 100-event bursts, trailing refresh, no overlap and unmount cancellation. Candidate CI is still running; no measured production latency claim. |
| 10 | Performance release gate | PENDING | PR #4 opened. CI and distributed PostgreSQL lab have not completed on the implementation candidate SHA. This report records the pending evidence and blockers. |

## 4. Communication and Team Room optimization

### BEFORE

`src/TeamRoom.tsx` called `refresh()` for each recognized event:
- `MESSAGE_CREATED`
- `TASK_CREATED`
- `TASK_UPDATED`
- `PRESENCE_CHANGED`
- `DECISION_CREATED`
- `ATTACHMENT_CREATED`
- `PARTICIPANT_JOINED`
- `PARTICIPANT_LEFT`

Each refresh executed five concurrent REST calls:
1. messages/history;
2. presence;
3. tasks;
4. decisions;
5. attachments.

For a burst of N events, the client could initiate 5N REST requests, while prior refreshes were still in flight.

### ALTERAÇÃO

New `src/teamRoomRefreshScheduler.mjs`:
- coalesces notifications during a 75 ms window;
- prevents overlapping refresh executions;
- marks state dirty when notifications arrive during an in-flight refresh;
- performs one trailing authoritative refresh after the active refresh finishes;
- supports cancellation on component unmount.

WebSocket events are not discarded as a source of truth: they are invalidation signals. The client reloads current server snapshots after coalescing. The initial connection refresh and user-triggered refresh remain.

### DEPOIS

The deterministic test submits 100 scheduler notifications in one burst and asserts one refresh invocation. The in-flight test submits 20 more notifications while the first refresh is running and asserts exactly one trailing refresh and maximum concurrency of one.

### EVIDÊNCIA / GANHO

- Synthetic test target: 100 event notifications -> 1 refresh.
- At five endpoints per refresh, the corresponding burst changes from up to 500 endpoint requests to 5.
- Synthetic request reduction for that burst: 99%.
- This is a deterministic code-level test target, **not a production traffic measurement**. Final test execution is pending CI.

No heartbeat cadence, outbox insert, PostgreSQL NOTIFY, authorization, message idempotency, audit, lease, fencing, ownership or handoff semantics changed.

## 5. Mission, checkpoint, handoff and report delivery

The current system persists task/checkpoint state in PostgreSQL and keeps local Runner state as metadata. Handoff is authenticated and recipient-bound. Report delivery remains a GitHub commit/PR operation, not a dedicated durable delivery queue with acknowledgement and replay.

A future delivery pipeline should use:
1. immutable mission/task ID and idempotency key;
2. append-only execution/checkpoint events;
3. durable outbox with consumer cursor and retry/dead-letter state;
4. report artifact commit/upload with content hash and evidence manifest;
5. explicit delivery acknowledgement tied to report SHA;
6. release of the next mission only after acknowledgement and review gate.

This is a design recommendation, not implemented in this mission. It requires a separate architecture mission because it affects delivery semantics and persistence contracts.

## 6. Runner and Recovery Worker

Observed:
- TeamRoom client has a bounded 10-second HTTP timeout.
- Lease duration is configurable from 15 to 300 seconds, default 60.
- Presence heartbeat does not renew task leases; lease renewal remains separately fenced.
- Recovery Worker defaults to 2,000 ms polling and a bounded 300,000 ms watch deadline.
- Each watch iteration performs recovery POST followed by candidate GET.

No optimization was made to lease renewal, fencing, expiration, ownership, recovery or handoff. Backoff/jitter, server-side wait/notification and a combined recovery-candidate endpoint need concurrency and load tests before implementation.

## 7. PostgreSQL

Reviewed existing index coverage:
- `team_room_messages_history_idx(channel_id, created_at DESC, message_id DESC)`;
- `team_tasks_room_status_idx(room_id, status, updated_at DESC)`;
- partial owner/lease index for owned/reassigned tasks;
- `outbox_unpublished_idx(created_at) WHERE published_at IS NULL`;
- handoff pending/target indexes;
- active room membership index.

No index was added. There was no production database connection or representative dataset to run `EXPLAIN (ANALYZE, BUFFERS)`, inspect lock waits, measure rows scanned, or validate index write overhead. Correlated presence lookup and unread-count aggregation remain candidates for measured investigation, not confirmed slow queries.

## 8. Frontend / Tauri

The Team Room UI now bounds refresh scheduling and prevents refresh overlap. Existing message history remains limited to the server's default first page; UI pagination and list virtualization are not implemented. No offline storage, Tauri IPC, SQLite transaction, blob hashing, fsync or project-storage behavior changed.

## 9. CI / development optimization

Changes:
- `Swatinem/rust-cache@v2` added to Rust jobs.
- npm cache added to Linux and Windows foundation jobs using `actions/cache@v4`.
- npm install uses `--prefer-offline --no-audit --no-fund`.
- `test:missions` runs `node --test tests/*.test.mjs`, including the new scheduler suite.
- Existing independent CI jobs remain parallel.
- No test, assert, integration job or coverage gate was removed.

**CI duration gain: GANHO NÃO MEDIDO.** No comparable before/after duration was available. Cache hits and reduced dependency download time require completed repeated CI runs.

Reproducibility risk: neither `package-lock.json` nor `Cargo.lock` exists on the base branch. npm install remains dependency-range resolution, not lockfile-pinned `npm ci`. Lockfile adoption requires a separate dependency governance change.

## 10. Benchmark comparison

| Área | Antes | Depois | Evidência | Ganho |
|---|---|---|---|---|
| Comunicação | One refresh per recognized event | Coalesced refresh with trailing update | 100-event scheduler test added; CI pending | 99% fewer refresh invocations in the synthetic burst; production GANHO NÃO MEDIDO |
| Team Room | Five REST calls per refresh; overlap possible | One active refresh; burst coalescing | Scheduler tests added; CI pending | Up to 500 -> 5 endpoint requests in the synthetic 100-event burst; production GANHO NÃO MEDIDO |
| Missões | Manual lifecycle and explicit checkpoint/handoff calls | Unchanged | Runner and TeamRoomClient inspected | GANHO NÃO MEDIDO |
| Relatórios | GitHub commit/PR delivery, no durable report queue | Unchanged; delivery pipeline proposed | Existing reports and outbox design inspected | GANHO NÃO MEDIDO |
| Runner | Sequential lifecycle calls; 10 s request timeout | Unchanged | `team-room-client.mjs`, `mission-runner.mjs` | GANHO NÃO MEDIDO |
| Recovery | 2 s default poll; recovery POST then candidate GET | Unchanged | `team-room-recovery-worker.mjs` | GANHO NÃO MEDIDO |
| PostgreSQL | Existing indexes and query patterns | No schema/query changes | Migration inspection only; no EXPLAIN | GANHO NÃO MEDIDO |
| Frontend | Event-driven full refresh; overlap possible | 75 ms coalescing and serialized refresh | Deterministic tests added; CI pending | Synthetic gain above; real UI latency GANHO NÃO MEDIDO |
| CI | No Rust/npm dependency cache | Rust/npm caches added; same jobs/tests | Workflow diff; run #609 pending | GANHO NÃO MEDIDO |

## 11. Files modified

1. `.github/workflows/ci.yml`
2. `package.json`
3. `src/TeamRoom.tsx`
4. `src/teamRoomRefreshScheduler.mjs`
5. `src/teamRoomRefreshScheduler.d.mts`
6. `tests/team-room-refresh-scheduler.test.mjs`
7. `reports/PERFORMANCE_COMMUNICATION_OPTIMIZATION_REPORT.md` (this report)

## 12. Commits

- `f188db5ca5376420af6ae263ec1fa0844113c0b0` — scheduler module
- `e11f517ee6420e9a9131441418c6b806a90c0543` — TypeScript declaration
- `28fc182fd493a1e1ebb1b8974e47fe9263c6ebf8` — scheduler tests
- `4fabeba56eda52629326d4111d3139a3f7fb73b9` — Team Room integration
- `f2c432a572922716c8a1fe1cd61d9e15b4edd79e` — mission test glob
- `4c4da5786cdfa6536167a3d9fb366d4b002cf814` — CI dependency caches

## 13. CI and artifacts

Implementation candidate SHA: `4c4da5786cdfa6536167a3d9fb366d4b002cf814`.

- TokenSaver AI CI run #609, ID `43199905102`: **IN_PROGRESS** at report creation.
- Distributed Runner PostgreSQL Lab run #132, ID `43199905325`: **IN_PROGRESS** at report creation.
- No workflow failures had been reported in the retrieved job snapshots at report creation.
- No artifacts were listed at report creation.
- No test suite result is marked PASS until the exact candidate workflows finish.

## 14. Risks, regressions and dependencies

### Risks / pending evidence
- Global PostgreSQL notification channel creates cross-room notification fanout. A room/tenant-scoped event routing design needs a separate migration and integration coverage.
- Every authenticated heartbeat currently writes a presence outbox event; notification coalescing reduces client read amplification but does not reduce outbox writes or socket notification fanout.
- Outbox delivery/replay and report delivery acknowledgement are not implemented.
- History pagination is supported in the backend but not exposed in the UI.
- Presence and unread-count query performance requires production-like EXPLAIN/ANALYZE.
- Recovery Worker polling and API round trips need load-tested backoff/notification design.
- No production APM/metrics, representative DB, traffic replay or local build environment was available.
- CI cache benefit is not measured; lockfiles are absent.

### Regressions
No regression is established. The new scheduler tests and full CI have not completed on this candidate. Security, fencing, idempotency, authorization and audit semantics were intentionally left unchanged.

### External dependencies
- **PENDENTE — DEPENDÊNCIA EXTERNA:** representative PostgreSQL environment and dataset for query plans/lock analysis.
- **PENDENTE — DEPENDÊNCIA EXTERNA:** production telemetry/APM and realistic event/heartbeat traffic.
- **PENDENTE — DEPENDÊNCIA EXTERNA:** durable event/report queue or equivalent delivery infrastructure if report replay/acknowledgement is required.

## 15. Next optimization missions

1. Room-scoped PostgreSQL notification fanout and event batching with multi-room isolation tests.
2. Heartbeat/outbox event policy that distinguishes liveness timestamp refresh from actual presence-state changes without weakening liveness semantics.
3. Durable report delivery pipeline: outbox consumer, retries, idempotent report artifacts, SHA-bound acknowledgement and next-mission gate.
4. PostgreSQL EXPLAIN/ANALYZE and load test for presence, unread count, recovery candidate selection and outbox retention.
5. Keyset pagination UI with stable compound cursor and message list virtualization.
6. Recovery Worker backoff/jitter or notification-driven wait, preserving bounded recovery deadlines and lease expiry semantics.
7. Lockfile adoption and dependency reproducibility.
8. End-to-end benchmark harness for mission start -> checkpoint -> handoff -> report acknowledgement.

## 16. Final performance gate

# NO-GO

The Team Room refresh amplification fix and CI cache changes are committed on an isolated PR. The synthetic scheduler benchmark is encoded as a regression test, but candidate CI has not completed and no production latency, database or CI-duration gain has been measured. Do not merge or claim system-wide performance improvement until the exact candidate workflows pass and representative runtime/DB benchmarks are recorded.
