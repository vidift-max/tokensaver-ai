# AI-PHONE-01 — Initial Architecture & Repository Audit

**Mission:** AI-PHONE-01 — AI-owned persistent mobile identity, continuity and audio  
**Repository:** `vidift-max/tokensaver-ai`  
**Base branch:** `v2-saas-alpha`  
**Base SHA observed:** `d1f2bd764361b14af0604104db8e670ee6f4f361`  
**Audit branch:** `mission/ai-phone-01`  
**Audit date:** 2026-11-12  
**Phase:** Round 1 — audit and architecture only  
**Implementation status:** NOT STARTED

## 1. Executive finding

The repository already contains a substantial PostgreSQL-backed AI Team Room foundation: stable AI principals, participants, short-lived revocable sessions, workload execution bindings, permission grants, room presence, persistent messages, a transactional outbox, task leases, monotonic fencing, checkpoints, authenticated handoffs and a recovery worker.

These are reusable foundations, not an AI-Phone implementation. No AI-Phone entity, durable phone endpoint ownership, phone-specific inbox/ACK protocol, phone handoff state machine, or real-time audio call stack was found in the audited tree.

The critical identity distinction is already present in the Team Room model:

- `principal_id` is the stable registered identity mapped from the exact external issuer + subject.
- `session_id` is a revocable, room-scoped access session.
- `execution_id` binds one workload execution to a principal, participant and session.

The AI-Phone must be keyed to the stable AI principal (or a separately governed stable `ai_id` mapped one-to-one to it), never to `session_id`, `participant_id`, display name, provider label, or endpoint.

**Important blocker:** the existing OIDC trust model requires a genuine upstream issuer, audience and JWKS. The Team Room architecture explicitly does not infer a model identity from a display name. No verified GPT-6 workload issuer/subject binding or provider integration was evidenced by this audit. Therefore AI-01 cannot yet be safely enrolled as an authenticated workload merely by declaring its name to be GPT-6.

## 2. Repository and service topology

- Repository: `vidift-max/tokensaver-ai`
- Current base branch: `v2-saas-alpha`
- Current base SHA: `d1f2bd764361b14af0604104db8e670ee6f4f361`
- Team Room service: standalone Rust/Axum service in `services/ai-team-room`
- Persistence: PostgreSQL through SQLx migrations
- WebSocket communication: `services/ai-team-room/src/communication.rs`
- Existing service routes: `services/ai-team-room/src/main.rs`
- Existing identity/authentication: `services/ai-team-room/src/auth.rs`
- Existing task leases/checkpoints: `services/ai-team-room/src/tasks.rs`
- Existing authenticated handoff: `services/ai-team-room/src/handoffs.rs`
- Existing Runner: `scripts/mission-runner.mjs`
- Existing recovery worker: `scripts/team-room-recovery-worker.mjs`

No source files or existing migrations were changed in this audit. The audit is isolated on `mission/ai-phone-01`.

## 3. Reuse matrix

| Capability | Existing component / evidence | Reuse decision | Gap for AI-Phone |
|---|---|---|---|
| Stable identity | `principals`, `principal_kind='AI'`, unique tenant/provider/external_subject in migration 0001 | Reuse as the authoritative AI identity; optionally add explicit `ai_id` only if the domain requires a public stable identifier | Need a verified issuer+subject enrollment for AI-01 |
| Participant | `participants` bound to tenant + principal | Reuse as room membership/profile, not as phone identity | Participant can be disabled or room-scoped; phone must outlive room presence |
| Authentication | OIDC RS256/JWKS exchange in `auth.rs`; replay protection via `auth_token_replays` | Reuse; phone APIs must authenticate the bearer session | External OIDC workload identity not configured in the latest continuity evidence |
| Session | `sessions` with status, hashed local bearer, expiry, revocation and rotation | Reuse for request authentication only | Session must be replaceable without changing phone ID |
| Execution binding | `workload_executions` ties principal/participant/session/execution | Reuse to prove the current execution context | Rebind phone attachment transactionally after authenticated new session |
| Presence | `room_memberships.presence_state`, `last_seen_at`, heartbeat and presence endpoint | Reuse heartbeat/outbox patterns; do not overload membership presence as phone state | Add phone-scoped presence independent of room membership |
| Messages | `team_room_messages`, channels, unique client message ID, history | Reuse schema patterns and authorization conventions | Phone-specific inbox requires recipient phone ID, durable delivery state and ACK |
| Event delivery | `outbox_events`, PostgreSQL NOTIFY and WebSocket listener | Reuse transactional outbox; notification is a wake-up, not source of truth | Add phone event types and idempotent consumer semantics |
| Tasks/missions | `team_tasks`, mission/task canonical identity and task routes | Reuse as the canonical mission execution record | Phone needs a stable pointer and a safe read/recovery contract |
| Lease/fencing | `team_tasks.fencing_token`, owner principal/participant/session/execution, DB trigger | Reuse; no second fencing authority | Endpoint attachment must not bypass or reset task fencing |
| Checkpoints | `team_tasks.checkpoint`, `checkpoint_version`, checkpoint route | Reuse as mission checkpoint authority | Persist phone's current mission pointer separately from task payload |
| Handoff | `task_handoffs`, recipient-bound session/execution, checkpoint_reference and transition trigger | Reuse for execution handoff where applicable | Add phone endpoint handoff state and fencing epoch |
| Recovery | bounded `team-room-recovery-worker.mjs` and authenticated recovery endpoints | Reuse recovery contract and evidence model | Phone reconnect orchestration is not present |
| Audit | `security_audit_events`, task lease events and outbox | Reuse for phone ownership, endpoint and call audit | Add phone-specific action names and metadata minimization |

### Important existing constraints

1. OIDC exchange revokes a previous active session for the same principal and room. The phone must not be cascaded/deleted when this happens.
2. Room membership reconnect is distinct from phone attachment and mission lease ownership.
3. Heartbeat is deliberately separate from task lease renewal. Phone presence must preserve that separation.
4. Existing Team Room messages are room/channel messages, not a dedicated personal phone inbox.
5. Existing WebSocket delivery is driven by persisted outbox events and clients refresh authoritative snapshots. Keep this model; do not rely on transient notifications for inbox durability.

## 4. Proposed AI-Phone domain contract

The initial implementation should use PostgreSQL as the source of truth and establish a permanent mapping:

`tenant_id + ai_id -> ai_phone_id`

If the existing `principal_id` is adopted as `ai_id`, document that explicitly and avoid redundant IDs. If a public AI ID is required, store a unique mapping to the AI principal and enforce it with a foreign key and unique constraints.

### Core record

| Field | Type | Meaning |
|---|---|---|
| `ai_phone_id` | UUID PK | Permanent phone identity; never regenerated during session or endpoint changes |
| `tenant_id` | UUID FK | Tenant boundary |
| `ai_id` | UUID FK / stable AI principal mapping | Permanent AI owner; must resolve to enabled `principal_kind='AI'` |
| `phone_status` | enum/check text | `ATTACHED`, `TRANSFER_REQUESTED`, `TRANSFERRING`, `ATTACHED_NEW_ENDPOINT`, `DISCONNECTED` |
| `current_session_id` | UUID nullable FK | Current authenticated session only; nullable while disconnected |
| `current_execution_id` | UUID nullable FK | Current execution binding, if present |
| `current_endpoint` | JSONB nullable | Validated endpoint metadata; no provider credentials or bearer tokens |
| `endpoint_epoch` | BIGINT | Monotonic fencing epoch for endpoint attachment |
| `last_seen` | TIMESTAMPTZ nullable | Last authenticated phone heartbeat |
| `current_mission_id` | UUID nullable | Pointer to canonical mission/task state |
| `created_at`, `updated_at` | TIMESTAMPTZ | Database timestamps |

Required constraints:
- UNIQUE (`tenant_id, ai_id`) — one permanent phone per AI.
- Composite foreign keys bind session/execution to the same tenant, principal, room and one another.
- `current_session_id` is nullable only for detached/disconnected states.
- Endpoint changes increment `endpoint_epoch` monotonically.
- A conditional unique/row-lock protocol prevents two active endpoints.
- No secret, upstream provider key, local bearer token, or OIDC assertion is stored in endpoint JSON.

### Suggested supporting records

- `ai_phone_endpoint_events`: immutable endpoint attach/transfer/detach audit with old/new endpoint metadata, epoch, actor principal/session/execution and result.
- `ai_phone_messages`: durable recipient phone inbox with sender identity, client idempotency key, per-phone sequence, body/payload, `PENDING | DELIVERED | ACKED | FAILED`, timestamps and attempt metadata.
- `ai_phone_mission_links`: optional only if one current mission pointer is insufficient; canonical checkpoint data remains in `team_tasks`.
- `ai_phone_calls`: call signaling/audit state only after a real media architecture is selected; do not mark media connected from signaling alone.

Avoid duplicating the entire task checkpoint into the phone row. The phone stores a pointer; `team_tasks.checkpoint` and its fencing/version remain authoritative.

## 5. Endpoint handoff protocol

All operations must occur in PostgreSQL transactions and lock the phone row:

1. Authenticate the new request through the existing OIDC-issued session.
2. Resolve the caller's principal/participant/execution from the session; never accept owner identity IDs from request body.
3. Lock the `ai_phones` row for the authenticated `ai_id`.
4. Verify phone ownership, current endpoint epoch and explicit phone permission.
5. Create a transfer request with an idempotency key and expected epoch.
6. Mark transfer in progress and revoke the previous endpoint's attachment authority by incrementing the epoch.
7. Attach the new endpoint only after the new authenticated session/execution binding is verified.
8. Commit the new session/execution and endpoint atomically; write audit and outbox event in the same transaction.
9. Any request carrying an older epoch is rejected, even if its session has not yet expired.

Do not treat knowing `ai_phone_id` as authorization. Do not automatically grant `task.transition`, `message.create`, `audio.publish`, `audio.subscribe`, or administrative permissions from phone ownership.

## 6. Message inbox protocol

Use an append-only or transition-guarded persistent message record:

- Unique idempotency key: (`ai_phone_id, sender_principal_id, client_message_id`).
- Monotonic sequence allocated transactionally per phone.
- Initial state `PENDING`.
- Delivery attempt changes state to `DELIVERED` only when an authenticated active endpoint receives the message.
- Explicit authenticated ACK changes state to `ACKED`.
- Retries use the same message ID and never create duplicate messages.
- Offline messages remain `PENDING`.
- Reconnection queries PostgreSQL by sequence and state; outbox/NOTIFY only wakes the receiver.
- History access requires phone-owner identity and separate read permission.

## 7. Mission continuity contract

Phone record stores `current_mission_id` and optionally `current_task_id`; canonical execution remains in `team_tasks`.

Recovery response should expose, under `task.read` plus phone ownership:
- mission_id, task_id, task status;
- checkpoint JSON and checkpoint_version;
- current owner principal/session/execution;
- lease expiry and fencing token;
- last persisted result/progress when available;
- attachment/file references already persisted by the service;
- latest handoff and blockers if modeled.

A returning session must acquire or accept the task using the existing lease/handoff protocol. It must not simply overwrite the owner session or reuse an old fencing token.

## 8. Presence and event contract

Phone-scoped presence should be separate from `room_memberships.presence_state`.

Minimum states:
`ONLINE`, `BUSY`, `EXECUTING`, `WAITING`, `DISCONNECTED`, `RECOVERING`.

Persist state changes and emit outbox events:
`PHONE_CONNECTED`, `PHONE_DISCONNECTED`, `CALL_INCOMING`, `MESSAGE_RECEIVED`, `MISSION_UPDATED`, `MISSION_BLOCKED`, `MISSION_COMPLETED`.

Deduplicate event consumers by outbox event ID; coalesce UI refreshes but never coalesce or drop inbox records.

## 9. Audio feasibility and explicit block

The audited Team Room service has Axum WebSockets for authenticated signaling/notifications but no evidenced WebRTC media stack, SDP/ICE signaling protocol, STUN/TURN configuration, SFU/media relay, browser microphone permission flow, audio codec negotiation, or real media integration test.

A WebSocket message is not bidirectional audio media. A call row or UI state is not a real call.

**BLOCKED — real audio is not currently evidenced or deployable from the audited stack.** Before claiming audio:
- choose browser-native WebRTC peer-to-peer or a managed/self-hosted SFU architecture;
- provision authenticated signaling and ICE/STUN/TURN or SFU;
- define participant authorization for `audio.publish` and `audio.subscribe`;
- implement offer/answer, ICE candidates, accept/reject/hangup, reconnect and timeout;
- test actual microphone capture and bidirectional RTP/media in two independent clients;
- collect media stats and call audit evidence.

Do not create fake call success or mark media connected when only signaling succeeded.

## 10. Security risks to close before implementation

- **Identity bootstrap:** no verified GPT-6 workload issuer/subject binding is evidenced. A model label is not an authenticator.
- **Cross-tenant IDOR:** every phone, message, endpoint, mission and call query must constrain tenant and owner principal.
- **Session rotation:** OIDC exchange revokes prior sessions; phone identity must survive and attachment must be rebound only by the same principal.
- **Endpoint takeover:** require row lock, expected epoch, authenticated owner and stale-epoch rejection.
- **Concurrent endpoint attachment:** database uniqueness/transaction protocol must prevent split brain.
- **Message replay:** unique sender/client message key and ACK transition guard.
- **Permission escalation:** phone possession grants no permissions by itself.
- **Mission takeover:** task lease and fencing token remain authoritative.
- **Secret leakage:** no provider credentials, bearer tokens or upstream JWT in phone metadata, messages, logs or audit payloads.
- **Call abuse:** explicit call authorization, rate limits, short-lived call tickets, origin validation and audit.
- **Event storms:** outbox idempotency and bounded/coalesced UI refresh, with no loss of persisted inbox messages.

## 11. Validation status and next steps

| Round | Status at this audit |
|---|---|
| 1 — Audit and architecture | COMPLETE as an initial repository audit |
| 2 — Permanent phone identity | NOT IMPLEMENTED |
| 3 — Endpoint portability/fencing | NOT IMPLEMENTED |
| 4 — Persistent inbox/ACK | NOT IMPLEMENTED |
| 5 — Mission pointer/recovery | NOT IMPLEMENTED |
| 6 — Phone presence/events | NOT IMPLEMENTED |
| 7 — Real audio | BLOCKED — media stack and runtime evidence absent |
| 8 — Phone-specific security tests | NOT IMPLEMENTED |
| 9 — Integrated portability/recovery E2E | NOT IMPLEMENTED |
| 10 — Final report/CI/SHA verification | NOT IMPLEMENTED |

The GitHub repository API was available for source inspection and an isolated audit branch. This execution did not have a checked-out local runtime, provisioned PostgreSQL service, configured OIDC issuer, independent endpoint clients, microphone access, or TURN/SFU deployment. Therefore no local Rust tests, live PostgreSQL integration, live OIDC exchange, endpoint handoff, or real audio test is claimed in this audit.

**Next implementation gate:** confirm/provision the genuine workload identity for AI-01 (issuer + subject + audience + JWKS trust). Without it, the schema and authorization design can be implemented and negatively tested, but no authenticated GPT-6 phone attachment can honestly be demonstrated.

## 12. Audit conclusion

The repository provides strong reusable primitives for the AI-Phone domain. The correct design is a permanent phone record owned by the stable AI principal, with session and endpoint attachment as replaceable, fenced runtime bindings. Messages must be phone-scoped durable records; mission state remains in the existing canonical task/checkpoint system; presence is phone-scoped; audio remains explicitly blocked until real media transport and independent client testing exist.

**Round 1 result:** architecture and audit documented. No AI-Phone functionality is claimed as implemented.
