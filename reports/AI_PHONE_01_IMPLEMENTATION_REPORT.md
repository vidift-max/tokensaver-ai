# AI-PHONE-01 — Implementation Report

**Repository:** `vidift-max/tokensaver-ai`  
**Branch:** `mission/ai-phone-01`  
**Base branch:** `v2-saas-alpha`  
**Initial SHA:** `d1f2bd764361b14af0604104db8e670ee6f4f361`  
**Implementation SHA validated before report-only commit:** `c73b08c6031ed4814a26bf7f80d2a94bd7b2dc41`  
**PR:** #7 — https://github.com/vidift-max/tokensaver-ai/pull/7  
**Status:** GO CONDITIONED

## 1. Objective

Implement the structural foundation for a persistent AI-owned phone whose identity survives session and endpoint changes. No GPT-6 provider identity or audio integration is fabricated.

## 2. Architecture implemented

The persistence model is:

`ai_identities` (global stable identity)  
→ `ai_identity_principals` (tenant-scoped principal bindings)  
→ `ai_phones` (one permanent phone per global AI identity)  
→ `phone_endpoints` (durable endpoint registry)  
→ `phone_attachments` (temporary authenticated session/execution binding with independent phone fence)

The phone identity is not derived from `session_id`, `execution_id`, `participant_id`, or endpoint ID.

The global AI identity can be mapped to separate enabled AI principals in multiple tenants using the same stable `identity_key`. A tenant/principal can be bound to only one global AI identity. A database trigger rejects HUMAN or disabled principals.

The test identity is explicitly marked `identity_kind='TEST'` and `AI-PHONE-TEST-IDENTITY`. No real GPT-6 identity is created.

## 3. Files changed

- `services/ai-team-room/migrations/0011_ai_phone_foundation.sql`
- `services/ai-team-room/src/phone.rs`
- `services/ai-team-room/src/lib.rs`
- `services/ai-team-room/src/main.rs`
- `services/ai-team-room/Cargo.toml`
- `services/ai-team-room/tests/phone_postgres.rs`

No AI Gateway or benchmark files were intentionally changed. The original audit remains at `reports/AI_PHONE_01_ARCHITECTURE_AUDIT.md`.

## 4. Database and migrations

New migration: `0011_ai_phone_foundation.sql`.

Tables:
- `ai_identities`
- `ai_identity_principals`
- `ai_phones`
- `phone_endpoints`
- `phone_attachments`
- `phone_messages`
- `phone_mission_links`

Database invariants:
- Unique global identity key.
- One phone per `ai_id`.
- One AI principal mapping per tenant/principal.
- Only enabled AI principals can be mapped to global AI identity.
- Endpoint belongs to the same phone as its attachment (composite FK).
- Attachment principal must be mapped to the same global AI identity as the phone (composite FK).
- Attachment session/execution must match existing Team Room bindings (composite FKs).
- Partial unique index permits only one active attachment per phone and one active attachment per endpoint.
- Phone endpoint fence is monotonic and advances by at most one per update.
- Message idempotency and per-phone sequence are unique.
- Message identity/content are immutable; state transitions are guarded.
- New phone permissions are registered in `permission_catalog`, including privileged `phone.transfer` and `ai.identity.provision`.

No provider credentials, bearer tokens, OIDC assertions, or secrets are stored in phone or endpoint records.

## 5. Rust models and APIs

Implemented in `src/phone.rs`, mounted through the shared library router:

| Method | Route | Purpose |
|---|---|---|
| POST | `/ai-phone/identities` | Privileged stable AI identity provisioning/binding |
| GET | `/ai-phone/me` | Get/create caller's mapped permanent phone and mission pointer |
| POST | `/ai-phone/attach` | Attach a new endpoint to caller's phone |
| POST | `/ai-phone/detach` | Detach current endpoint using its fence |
| POST | `/ai-phone/heartbeat` | Refresh last-seen for current endpoint/fence |
| POST | `/ai-phone/reconnect` | Reattach after prior session has expired/revoked |
| POST | `/ai-phone/transfer` | Explicitly transfer endpoint under `phone.transfer` permission |
| GET | `/ai-phone/{phone_id}/messages` | Retrieve inbox and mark pending messages delivered |
| POST | `/ai-phone/{phone_id}/messages` | Enqueue durable message (offline delivery supported) |
| POST | `/ai-phone/{phone_id}/messages/{message_id}/ack` | ACK a delivered message on the active attachment |
| POST | `/ai-phone/mission` | Link the caller's authorized task to its phone |

Authentication is performed through the existing `authenticate_session`. Authorization is checked with separate permission grants. Phone possession or knowledge of `ai_phone_id` does not grant access.

## 6. AI_ID and AI_PHONE_ID

- `ai_identities.ai_id` is a global stable UUID.
- `identity_key` provides stable uniqueness and allows a global identity to be bound across tenant principals.
- `ai_phones.ai_phone_id` is generated once and has a unique FK to `ai_id`.
- Phone creation is idempotent for an already mapped AI identity.
- The phone record survives session expiry and endpoint changes.

A trusted caller needs the explicit `ai.identity.provision` permission to create/bind a global identity. The route only binds enabled AI principals in the caller's tenant. Repeated binding of the same stable identity key in another tenant reuses the same `ai_id`; a conflicting identity or principal mapping is rejected.

## 7. Endpoint registry, attach/detach, reconnect/transfer

Endpoint types: `WEB`, `DESKTOP`, `MOBILE`, `WORKER`, `TEST`.

Endpoint states: `REGISTERED`, `ATTACHED`, `DETACHED`, `REVOKED`.

The registry records `created_at`, `last_seen_at`, `detached_at`, endpoint metadata and a durable endpoint ID. Metadata is JSON-object-only, bounded, and rejects credential-like field names.

Attach validates a live session and execution binding, identity mapping, explicit permission and phone ownership. The phone row is locked; a partial unique index prevents split-brain attachments. Attach to the same endpoint/session is idempotent.

Reconnect with a new session succeeds only when the previous attachment's session is no longer live. Explicit transfer requires `phone.transfer` and fences the old attachment even when the old session remains live.

Detach requires the current live session, active attachment and matching phone fence. Heartbeat requires the same live binding and fence. Old endpoint requests with a stale fence are rejected.

## 8. Fencing

Phone fencing is stored in `ai_phones.endpoint_fence` and copied to each `phone_attachments.endpoint_fence`.

It is independent from `team_tasks.fencing_token`. Endpoint fencing protects phone attachment ownership; task fencing protects mission/task ownership. Neither token is reused for the other domain.

## 9. Mailbox

`phone_messages` stores:
- message ID, phone ID, sender principal, recipient AI identity;
- monotonic per-phone sequence;
- sender idempotency key;
- JSON content;
- `PENDING`, `DELIVERED`, `ACK`, `FAILED` states;
- attempt count and created/delivered/acknowledged timestamps.

Messages can be enqueued while no endpoint is attached. They remain `PENDING` and are delivered from persistent storage after reconnect. Sender/idempotency duplicates return the existing message; conflicting content under the same key is rejected. Delivery and ACK transitions are guarded. Inbox, delivery and ACK events are written to the existing transactional outbox and audit stream.

Content is bounded and the API rejects credential-like keys (token, secret, password, API key, authorization, etc.). This is defensive filtering, not a guarantee of perfect secret detection; callers must not send credentials in message content.

## 10. Mission continuity

`ai_phones.current_mission_id` and `current_task_id` plus `phone_mission_links` store a pointer to existing `team_tasks`.

The phone does not copy or replace:
- `team_tasks`;
- task leases;
- task fencing;
- checkpoint JSON/version;
- task handoffs;
- Recovery Worker.

The mission-link API requires `task.read` and only links a task owned by the authenticated principal in the authenticated room/tenant. Phone snapshot reads the canonical task checkpoint and current task fencing data. A new session must still use existing task lease/handoff authorization to continue execution; the phone does not acquire or steal a task lease.

## 11. Tests and evidence

GitHub Actions run for implementation SHA `c73b08c6031ed4814a26bf7f80d2a94bd7b2dc41`:

- Workflow: TokenSaver AI CI
- Run ID: `43215870580`
- Job: `ai-team-room-service`
- Result: **SUCCESS**
- `cargo check --manifest-path services/ai-team-room/Cargo.toml`: PASS
- `cargo test --manifest-path services/ai-team-room/Cargo.toml`: PASS
- PostgreSQL service: PostgreSQL 16 CI service container (real database, not a mock)

Test suite summaries from the job log:
- 6 passed
- 8 passed
- 3 passed
- 4 passed
- 1 passed (`phone_identity_survives_session_rotation_and_endpoint_change`)
- 0 doc tests; no failures

The AI-Phone PostgreSQL integration test proves:
1. Test global AI identity and phone creation.
2. Idempotent attach to endpoint A.
3. Concurrent second attach is rejected while endpoint A is active.
4. Session A expires without deleting the phone.
5. Session B reconnects to the same `ai_phone_id`.
6. Explicit endpoint transfer increments the independent phone fence.
7. Old endpoint/session/fence cannot detach the new attachment.
8. Concurrent endpoint race produces one winner and one conflict.
9. Detach releases ownership.
10. Offline message remains `PENDING` after reconnect.
11. Duplicate sender/idempotency key does not create a second message.
12. Delivery and ACK transitions persist.
13. Existing task checkpoint is returned through the same permanent phone.
14. A HUMAN principal cannot be mapped to a global AI identity.
15. Endpoint heartbeat updates last-seen under the current fence.

The earlier failing test run exposed the existing workload execution timestamp invariant (`expires_at > heartbeat_at`). The fixture was corrected to expire heartbeat and execution together. The subsequent PostgreSQL run passed.

No HTTP-level end-to-end test of every new route has yet been added. The Rust service compile and PostgreSQL service integration test passed; route-level auth behavior is supported by the existing auth implementation and the new permission checks, but is not claimed as independently E2E-tested for every route.

## 12. CI

At report preparation, the `ai-team-room-service` job for run `43215870580` is successful. The other workflow jobs were still running at the last observation; their final conclusions must be checked against the final report commit before treating the entire repository workflow as green.

The Keycloak OIDC smoke job is separate from AI-Phone identity provisioning. Its success does not prove a real GPT-6 workload identity.

## 13. Audio and provider identity

**BLOCKED — real GPT-6 workload identity:** no verified GPT-6 issuer/subject/audience/JWKS mapping or provider integration was supplied. The implementation uses only an explicit TEST identity in PostgreSQL integration tests. The privileged identity provisioning path can bind a future genuine AI principal after upstream trust is configured.

**BLOCKED — real audio:** no WebRTC/SFU, SDP/ICE signaling, STUN/TURN, RTP media path, or two-client microphone/audio validation was implemented. No call is simulated or marked successful.

## 14. Security and limitations

Implemented:
- tenant-scoped principal mapping to global AI identity;
- enabled-AI-only identity binding trigger;
- phone ownership derived from authenticated session principal mapping;
- explicit permissions;
- session and execution validation on attach/detach/heartbeat;
- phone-specific monotonic fencing;
- DB uniqueness against concurrent active attachments;
- stale attachment rejection;
- idempotent message send;
- message state transition guard;
- endpoint metadata credential-key filtering;
- transactional audit/outbox events for attachment, transfer, detach and mailbox events.

Not yet independently proven:
- exhaustive cross-tenant HTTP IDOR matrix;
- every permission-denial route via HTTP integration;
- production concurrency/load behavior beyond the PostgreSQL attach race test;
- full delivery retry/dead-letter policy;
- presence states beyond attachment/last-seen and the existing Team Room presence;
- production deployment and live external AI provider enrollment.

No provider credentials are accepted or stored by AI-Phone. Possessing `ai_phone_id` alone is insufficient to authenticate, attach, read messages, ACK, or change a mission pointer.

## 15. Remaining implementation gaps

- HTTP-level E2E suite covering all AI-Phone routes and negative authorization cases.
- Explicit delivery retry/dead-letter worker and bounded inbox pagination.
- Full phone presence-state API (`ONLINE`, `BUSY`, `EXECUTING`, `WAITING`, `DISCONNECTED`, `RECOVERING`) if required by the product UI.
- Real GPT-6 workload trust bootstrap.
- Real audio/WebRTC stack and media validation.

## 16. Final classification

**GO CONDITIONED**

The persistent identity/phone foundation, endpoint attach/reconnect/transfer, independent fencing, durable mailbox, heartbeat and task checkpoint pointer have been implemented and passed the PostgreSQL 16 integration test on the recorded implementation SHA.

Conditioning remains because GPT-6 identity enrollment is not real, audio is not implemented, exhaustive HTTP security E2E and delivery retry/presence completeness are still outstanding, and the full CI workflow must be checked on the report's final commit.


---

## 17. AI-PHONE-01 — FINAL VALIDATION ADDENDUM (2026-11-12)

This addendum supersedes the earlier status statements in sections 11–16 wherever they conflict. The earlier implementation-SHA results are historical; the final validation evidence is below.

### 17.1 Final SHA and CI

- Requested initial SHA: `d83785f5461f82c335f698311f6d28c8a8cdf61a`
- The requested SHA was checked independently. TokenSaver AI CI run `43216386393` completed with all four jobs successful.
- Final implementation/test SHA: `7d072604d777b6ecdf8d0af66cbb4ed85b74915a`
- Final TokenSaver AI CI run: `43221238250`
- Result: all four jobs **SUCCESS**:
  - `ai-team-room-service`
  - `foundation`
  - `foundation-windows`
  - `keycloak-oidc-smoke`
- The separate Distributed Runner PostgreSQL Lab workflow was not modified as part of this work.

The final implementation SHA was tested after the HTTP, mailbox and cross-tenant test additions. The report-only commit that follows this addendum does not change application code.

### 17.2 PostgreSQL and Rust results

The CI service job used a real PostgreSQL 16.15 service container and ran migrations through the service test setup. No PostgreSQL mock was used.

On SHA `7d072604d777b6ecdf8d0af66cbb4ed85b74915a`:
- `cargo check --manifest-path services/ai-team-room/Cargo.toml`: PASS
- `cargo test --manifest-path services/ai-team-room/Cargo.toml`: PASS
- Library tests: 6 passed
- Binary tests: 9 passed
- `admission_postgres`: 3 passed
- `auth_postgres`: 4 passed
- `phone_postgres`: 1 passed
- Doc tests: 0 failures

The PostgreSQL log also records expected rejections from negative fencing, handoff-state and AI/HUMAN mapping checks. These are expected test outcomes, not suite failures.

### 17.3 HTTP E2E coverage

The integration test invokes the actual Axum router and handlers using HTTP requests with a persisted bearer-token hash and a real PostgreSQL database. It is router-level HTTP E2E using Tower `oneshot`; it is not a socket-level test against a separately deployed service process.

Covered successful routes:
- `GET /ai-phone/me` — creates or returns the permanent phone
- `POST /ai-phone/attach`
- `POST /ai-phone/endpoints/register`
- `GET /ai-phone/{phone_id}/endpoints`
- `GET /ai-phone/{phone_id}/presence`
- `POST /ai-phone/heartbeat`
- `POST /ai-phone/reconnect`
- `POST /ai-phone/transfer`
- `POST /ai-phone/{phone_id}/messages`
- `GET /ai-phone/{phone_id}/messages`
- `POST /ai-phone/{phone_id}/messages/{message_id}/ack`
- `POST /ai-phone/detach`

Negative HTTP/database cases:
- invalid bearer token: `401`;
- expired session A after B reconnects: `401`;
- unknown or cross-tenant phone ID for endpoint lookup/message send: `404`;
- stale/detached endpoint heartbeat: `412`;
- stale endpoint/fence after transfer: rejected;
- duplicate ACK: idempotent `204`;
- HUMAN principal mapped to AI identity: rejected by the PostgreSQL trigger.

The privileged AI identity provisioning handler and mission-link handler are not each independently covered by a dedicated HTTP E2E case in this round. The underlying AI identity mapping and canonical mission pointer are covered by PostgreSQL tests.

### 17.4 Concurrency and endpoint fencing

The PostgreSQL attach race starts two concurrent attach operations for different endpoint IDs against the same phone. The database lock and uniqueness invariant produce exactly one winner and one conflict. The test is concurrent requests from one test process; it does not claim two independent OS processes or two physical devices.

Phone fencing is independent of task fencing:
- endpoint A attaches with phone fence 1;
- endpoint B reconnects/transfers with phone fence 2;
- old endpoint/session/fence operations are rejected.

The existing task lease fencing mechanism remains separate and unchanged except for the narrow recovery-handoff eligibility correction described below.

### 17.5 Mission continuity — real task lease proof

The PostgreSQL test `postgres_same_ai_phone_session_rotation_recovers_real_task_lease_and_fences_old_session` passed on the final implementation SHA.

Test sequence:
1. One stable AI identity and AI_PHONE_ID attach endpoint A under session A.
2. Session A owns a real Team Room task lease and writes checkpoint version 1.
3. Session A and its execution expire; the phone row and mission pointer remain persistent.
4. Session B is created for the same AI principal with a different session and execution.
5. Session B reconnects to the same AI_PHONE_ID on endpoint B; phone fencing advances from 1 to 2.
6. The existing recovery path marks the task recoverable; an authorized Recovery Worker identity creates the normal recovery handoff to B.
7. B accepts the handoff through the existing task transition/lease machinery. Task fencing advances from 1 to 2.
8. B writes checkpoint version 2 and completes the task.
9. A's lease renewal, checkpoint, completion, release and phone heartbeat are rejected.

The test uses the existing PostgreSQL-backed recovery/handoff functions and their authorization checks. It does not directly overwrite the task owner, lease or fencing token to manufacture success. It exercises the Recovery Worker identity and recovery functions in-process, not a separately deployed worker daemon.

### 17.6 Mailbox and ordering

The final HTTP/PostgreSQL test now proves the offline path:
- an offline message is persisted as `PENDING`;
- endpoint B reconnects to the same phone;
- authenticated inbox GET changes it to `DELIVERED`;
- attempt count increments;
- HTTP ACK changes it to `ACK`;
- duplicate ACK remains idempotent;
- inbox sequence order is `1 → 2`.

A second poll re-presents unacknowledged `DELIVERED` messages and increments `attempt_count`, providing a simple at-least-once retry mechanism.

Limit: there is still no scheduled retry worker, exponential backoff, dead-letter policy, explicit delivery-failure HTTP operation, or bounded pagination. The `FAILED` state is not evidence that a complete failure/retry subsystem exists.

### 17.7 Presence

The authenticated phone presence endpoint distinguishes the six requested phone states:
- `REGISTERED`
- `CONNECTED`
- `ATTACHED`
- `OFFLINE`
- `RECONNECTING`
- `DISCONNECTED`

The state is derived from phone existence, endpoint registry state, active attachment, session/execution validity and endpoint heartbeat age. Phone identity remains persistent when session presence expires. The integration test exercises all six transitions, including stale endpoint heartbeat and explicit detach.

Presence is a phone-level derived view; it is not a claim that the user/session presence subsystem has been replaced.

### 17.8 Security review

Implemented and exercised:
- tenant/principal mapping checks on message send and endpoint lookup;
- cross-tenant phone ID concealment with HTTP `404`;
- authenticated session and execution checks;
- explicit phone permissions;
- AI-only identity binding enforced by database trigger;
- phone fencing independent from task fencing;
- stale endpoint/session rejection;
- idempotent message send and ACK;
- credential-like metadata/content key rejection;
- monotonic task fencing and stale lease rejection in the existing task suite.

Still not fully proven:
- exhaustive per-route permission-denial matrix;
- two independent external client processes racing the same phone;
- exhaustive replay/IDOR fuzzing and production security review;
- real external AI provider enrollment and production deployment.

### 17.9 Final 10-round disposition

| Round | Result | Evidence / limitation |
|---|---|---|
| 1 — SHA final | PASS | Requested SHA checked; final implementation SHA and CI run recorded above |
| 2 — PostgreSQL | PASS | PostgreSQL 16.15; migrations, cargo check/test and integration suites passed |
| 3 — HTTP E2E | GO CONDITIONED | Actual Axum route tests cover core phone, endpoint, presence, transfer, reconnect and mailbox routes; not every privileged route or socket-level deployment |
| 4 — Concurrency | GO CONDITIONED | PostgreSQL race gives one winner/one conflict; same-process concurrent requests, not independent OS clients |
| 5 — Mission continuity | PASS | Session B legitimately accepts recovery handoff, acquires new task fence, checkpoints and completes |
| 6 — Old session attack | PASS | Old A renew/checkpoint/complete/release/heartbeat rejected |
| 7 — Mailbox | GO CONDITIONED | Offline delivery, retry-on-poll, ordering and duplicate ACK pass; no failure worker/dead-letter/backoff |
| 8 — Presence | PASS | Six phone-level presence states exercised |
| 9 — Security | GO CONDITIONED | Tenant IDOR, AI/HUMAN, auth, stale fence and idempotency tests pass; exhaustive matrix not complete |
| 10 — Report | PASS | This addendum records the final evidence and remaining limitations |

### 17.10 Final classification

**GO CONDITIONED**

The requested structural objective is demonstrated: the same permanent AI_PHONE_ID survives session rotation, reconnects on a new endpoint, receives offline mailbox messages and resumes a real existing mission through authorized task recovery/handoff with a new task fencing token. The old session is fenced out.

Conditions remain for independent-process concurrency proof, exhaustive HTTP authorization/security coverage, full mailbox failure handling, production identity enrollment and production rollout.

No WebRTC/audio stack was implemented. No real GPT-6 identity was invented. AI Gateway and benchmark code were not modified. PR #7 remains open and Draft; no merge was performed.
