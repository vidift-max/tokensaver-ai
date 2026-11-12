# AI-PHONE-01 — Architecture Preparation Report

**Repository:** `vidift-max/tokensaver-ai`  
**Audited ref:** `v2-saas-alpha`  
**Audited SHA:** `d1f2bd764361b14af0604104db8e670ee6f4f361`  
**Audit date:** 2026-11-12  
**Scope:** read-only architectural audit and implementation specification  
**Gateway:** not modified  
**AI-Phone implementation:** not created  
**Decision:** **READY FOR IMPLEMENTATION — ARCHITECTURE ONLY**

> This decision means the repository contains sufficient identity, session, authorization, task-continuity and communication foundations to begin a carefully staged AI-Phone implementation. It does not mean an AI-Phone exists, that production identity federation is configured, or that messaging delivery guarantees or audio calls are implemented.

## 1. Executive summary

The repository already contains a standalone Rust/Axum Team Room service backed by PostgreSQL. Its real persisted concepts include tenants, rooms, AI/HUMAN principals, participants, workload identities, sessions, workload executions, room memberships, presence timestamps/state, permissions, persistent room messages, a transactional outbox, tasks, leases, monotonic fencing, checkpoints and authenticated handoffs.

The most important architectural boundary is:

- `principal_id` is the stable registered principal within a tenant;
- `participant_id` is the principal's Team Room participant representation;
- `session_id` is a revocable, short-lived authenticated session;
- `execution_id` is one workload execution bound to that session;
- task ownership is bound to the complete principal/participant/session/execution tuple and a fencing generation.

The code does **not** contain a global AI identity independent of tenant, an AI-Phone record, a persistent endpoint registry, a portable phone binding, an owner-to-phone authorization relation, an AI-Phone inbox, per-recipient delivery/ACK states, or an audio-call subsystem.

The AI-Phone should therefore be added as a new domain layer above existing identity and continuity primitives. It must not be implemented as a new Gateway, a renamed session, a Team Room participant, or a replacement for task leases.

## 2. State actually found at the audited SHA

### 2.1 Identity

**Present in schema/code:**

- `principals`: `principal_id`, `tenant_id`, `principal_kind` (`AI`/`HUMAN`), `provider`, `external_subject`, `enabled`, `created_at`; uniqueness is tenant + provider + external subject.
- `workload_identities`: registered AI workload mapping to a principal, with issuer, subject, status, timestamps and metadata; one registered workload identity per tenant/principal.
- `participants`: tenant-scoped participant record attached to a principal; one participant per principal per tenant.
- `room_memberships`: room-specific participant/principal/session membership.

**Meaning and limitation:** `principal_id` is stable across sessions in the same tenant. It is not a globally portable AI identity across tenants. The OIDC subject is an upstream workload identity, not a model name. `GPT-6` is not itself proof of identity and must not be accepted as an authentication credential or external subject merely because it is the product/model label.

No dedicated `AI_ID` or `AI_PHONE_ID` table exists.

### 2.2 Sessions and execution endpoints

- `sessions` persist room/principal binding, issuance and expiry, status, revocation/closure, a SHA-256 token digest and optional rotated-from session reference.
- Session lifecycle in the architecture record: `CREATED → ACTIVE → EXPIRED | REVOKED | CLOSED`.
- `workload_executions` persist `execution_id`, tenant, room, participant, principal, session, worker subject, heartbeat, expiry, status and revocation.
- Authenticated local bearer tokens are generated server-side, returned once and stored as SHA-256 digests. Session lifetime is bounded by upstream expiry and a local maximum of 15 minutes.
- A new successful OIDC exchange for the same principal/room revokes the previous active session and execution and creates new IDs.

**Not present:** a durable endpoint identity/registry, endpoint health/route ownership, device registration, phone attachment, or session-independent endpoint binding. `worker_subject` is not an endpoint ID.

### 2.3 Participants and presence

- Membership is persisted in `room_memberships`, bound to room, principal, participant and session.
- Presence state is persisted as one of `ONLINE`, `AWAY`, `BUSY`, `EXECUTING`, `WAITING`, `BLOCKED`.
- Heartbeat updates `workload_executions.heartbeat_at`, `room_memberships.last_seen_at` and `workload_identities.last_seen_at`, then emits an outbox event.
- Room presence response derives connection status from session/execution heartbeat age and derives `EXECUTING` from a currently owned task.
- The code explicitly keeps presence heartbeat separate from task lease renewal.

Presence describes a current room/session/execution observation. It is not proof that a persistent AI identity owns a phone, nor a durable network endpoint.

### 2.4 Tasks, leases, fencing and checkpoints

Migration `0005_distributed_task_ownership.sql` creates `team_tasks` with:

- `task_id`, `tenant_id`, `room_id`, `mission_id`, title and task status;
- owner principal, participant, session and execution IDs;
- monotonic `fencing_token`;
- lease issue, renewal and expiry timestamps;
- JSONB checkpoint and `checkpoint_version`;
- creator and lifecycle timestamps.

Migration `0007_idempotent_mission_task_identity.sql` enforces one canonical task per tenant/room/mission. The task row is the serialization point. PostgreSQL guards prevent fence rollback, jumps larger than one, ownership changes without a newer fence, checkpoint-version rollback/jumps, and mutation of terminal tasks.

Lease acquire/renew/release, checkpoint and complete operations validate authenticated identity, current owner tuple, lease expiry and fencing token. Checkpoints are persisted in PostgreSQL.

### 2.5 Handoff and recovery

- `task_handoffs` persist source and recipient principal/participant/session/execution IDs, status, source/target fencing values, checkpoint reference, reason, expiry and lifecycle timestamps.
- Migration `0010_authenticated_task_handoffs.sql` normalizes the status vocabulary to `PENDING`, `ACCEPTED`, `REJECTED`, `EXPIRED`, `CANCELLED`, `COMPLETED`, adds relational constraints and transition guards, and enforces one pending handoff per task.
- Acceptance checks the addressed recipient tuple and its live authenticated authorization; task ownership moves to the recipient with a new fencing generation.
- Recovery of expired leases and a separate recovery worker are present.

**Important boundary:** the handoff contract is intentionally session/execution-specific. It is not an AI-Phone transfer protocol. The tested distributed proof covers distinct Runner identities and authenticated handoff; it does not establish a general phone attach/reconnect contract or every same-AI/new-session continuity case.

### 2.6 Communication

Migration `0009_team_room_communication.sql` creates:

- general/private channels and channel membership;
- persistent `team_room_messages` with sender principal, client message ID, parent, body and timestamps;
- per-channel read watermark;
- decisions and attachment references;
- hashed, expiring, single-use WebSocket tickets;
- transactional outbox notification through PostgreSQL `NOTIFY`.

The communication module implements general/private history and send routes, channel ACL checks, message persistence and client idempotency using `(channel_id, sender_principal_id, client_message_id)`. Reusing the same idempotency key with different body/parent data returns conflict. The WebSocket is authenticated by a single-use ticket and checks live session/membership/permission; private-channel fanout is ACL-filtered.

**Not present:** an AI-Phone-addressed inbox, per-recipient delivery records, explicit `PENDING`/`DELIVERED`/`ACK`/`FAILED` states, a durable consumer cursor per phone, phone-level ordering sequence, retry/dead-letter policy, or a guarantee that a WebSocket notification was consumed by the destination AI. The current outbox is an event-notification mechanism, not a phone message queue.

### 2.7 Authentication and authorization

The Team Room architecture and `auth.rs` implement a configured OIDC workload assertion exchange using RS256 and pinned issuer/audience/JWKS configuration; issuer and JWKS are not taken from untrusted token fields. The flow checks required claims including subject, audience, expiry, unique `jti` and connection scope; replayed issuer + `jti` is rejected. It maps the verified identity to registered workload/principal/participant rows and creates a local opaque session.

Authorization is default-deny and permission-based. The catalog includes room, task, messaging, private-channel, attachment, presence, decision, administration and audio permissions.

The architecture record says external trust and production deployment are pending. CI previously used ephemeral test identities and did not establish live production OIDC federation. No owner-to-AI-Phone authorization policy exists.

### 2.8 Audio

The permission catalog contains `audio`, `audio.publish` and `audio.subscribe`. These are permission vocabulary only. No audio call routes, WebRTC offer/answer signaling, ICE candidate exchange, STUN/TURN integration, SFU/media server, audio track lifecycle, codec policy, recording/transcription subsystem or mobile audio client was found in the audited service.

**Audio calls are not implemented or validated.**

## 3. Proposed domain model

### 3.1 Formal identities

The following identifiers must never be conflated:

| Identifier | Meaning | Lifetime |
|---|---|---|
| `AI_ID` | Permanent logical AI identity, independent of a particular login/session/runtime | Long-lived |
| `SESSION_ID` | Authenticated, revocable connection/session | Short-lived |
| `ENDPOINT_ID` | Registered runtime/device/client endpoint capable of connecting the AI | Endpoint lifecycle |
| `AI_PHONE_ID` | Stable communication address/phone resource owned by one AI identity | Long-lived |

`AI_ID ≠ SESSION_ID ≠ ENDPOINT_ID ≠ AI_PHONE_ID`.

A display label such as `AI-01 = GPT-6` is metadata attached to the identity/profile. The model/provider may change without changing the AI identity or phone address, provided the owner-authorized identity mapping remains intact.

### 3.2 Recommended global identity strategy

Do not assume current `principals.principal_id` is globally portable. It is tenant-scoped and derived from tenant + issuer + external subject.

Recommended future model:

- `ai_identities(ai_id UUID PRIMARY KEY, display_key UNIQUE, status, created_at, updated_at)`: global permanent identity.
- `ai_principal_bindings(ai_id, tenant_id, principal_id, status, created_at, revoked_at)`: explicit mapping from global AI identity to tenant-scoped authenticated principals.
- `ai_endpoints(endpoint_id UUID PRIMARY KEY, ai_id, endpoint_kind, status, registered_at, last_seen_at, metadata)`: registered execution clients. Avoid persisting raw IPs or bearer credentials as endpoint identity.
- `ai_phones(ai_phone_id UUID PRIMARY KEY, ai_id UNIQUE, phone_status, binding_generation BIGINT, created_at, updated_at)`: stable phone resource; unique owner relationship by default.
- `ai_phone_bindings(binding_id, ai_phone_id, ai_id, tenant_id, principal_id, participant_id, session_id, execution_id, endpoint_id, fencing_generation, lease_expires_at, attached_at, detached_at, reason)`: appendable binding history with exactly one current binding per phone, enforced by a partial unique index and transaction-level row locking.

An alternative for a strictly single-tenant first release is to use an AI `principal_id` as the initial `AI_ID` alias. That shortcut must be explicitly documented as tenant-scoped and must not be advertised as cross-tenant portability. The recommended global mapping avoids making a future migration mandatory.

### 3.3 Minimum requested phone fields: persist vs derive

| Requested field | Recommendation | Reason |
|---|---|---|
| `AI_ID` | Persist as FK to permanent identity | Stable owner of the phone |
| `AI_PHONE_ID` | Persist as primary key | Stable address/resource |
| `PHONE_STATUS` | Persist lifecycle/admin state | Distinguish ACTIVE, SUSPENDED, REVOKED, RETIRED; connectivity itself should be derived |
| `CURRENT_SESSION_ID` | Derive from current active binding; optionally denormalize only with strict transactional consistency | It changes on reconnect and is not phone identity |
| `CURRENT_ENDPOINT` | Derive endpoint from active binding; store `endpoint_id`, not a mutable raw URL | Network routes can rotate; avoid stale or attacker-controlled destinations |
| `LAST_SEEN` | Persist on endpoint/binding heartbeat or derive from latest authenticated heartbeat | Needed for operational presence; not proof of ownership |
| `CREATED_AT` | Persist | Immutable audit metadata |
| `UPDATED_AT` | Persist | Change tracking |

Additional mandatory binding fields: `fencing_generation`, `lease_expires_at`, `attached_at`, `detached_at`, and `binding_status`. Without a generation and lease, simultaneous endpoint ownership cannot be safely serialized.

Phone status must not be overloaded with presence. Example: `PHONE_STATUS=ACTIVE` while current connection is absent; expose derived connectivity as `CONNECTED`/ `DISCONNECTED` separately.

## 4. Identity and session continuity

Example:

```text
Permanent AI_ID = AI-01
Permanent AI_PHONE_ID = PHONE-01

Session A + Endpoint A
        |
        | authenticated attach, generation 17
        v
PHONE-01 -> AI-01 -> Session A / Endpoint A
        |
        | Session A expires or disconnects
        | binding lease expires or is explicitly detached
        v
PHONE-01 remains ACTIVE but temporarily disconnected
        |
        | fresh OIDC authentication
        | Session B + Endpoint B
        | attach with generation 18
        v
PHONE-01 -> AI-01 -> Session B / Endpoint B
```

The phone ID and AI ID remain unchanged. Session and endpoint IDs change. Old session credentials are revoked/expired; the old binding generation becomes stale and cannot send, receive, or mutate the phone's current binding.

The current auth exchange revokes a previous active session in the same principal/room. AI-Phone must not weaken this behavior to keep an old session alive. Reconnect means authenticate a new session and acquire a new phone binding generation, not revive a revoked session.

## 5. Portability protocol

### 5.1 Attach

1. Endpoint authenticates through the existing trusted workload identity path and obtains a fresh Team Room session.
2. Service resolves the authenticated principal to exactly one authorized global `AI_ID`; the client cannot choose another AI ID in request data.
3. Owner/administrator policy and endpoint registration are checked.
4. Transaction locks the `ai_phones` row (`SELECT ... FOR UPDATE`), checks phone status, current binding and lease.
5. If there is a valid live owner, reject attach unless an explicit authorized transfer protocol is in progress.
6. If unbound or safely expired, increment `binding_generation`, create active binding, set a short lease and audit the operation.
7. Return a signed/opaque binding grant containing phone ID, AI ID, endpoint ID, session binding, generation and expiry. Do not use the phone ID as a bearer secret.

### 5.2 Detach

- Require authenticated current binding owner and exact current generation.
- Mark binding detached and clear active binding transactionally.
- Repeated detach should be idempotent for the same binding request.
- A stale endpoint must not detach a newer binding.
- Logout/revocation should invalidate the corresponding binding or make it ineligible immediately; cleanup workers may finalize expired rows.

### 5.3 Transfer

- Require explicit authorization for source and destination endpoints, plus owner policy.
- Create a durable transfer intent with source binding ID/generation, destination endpoint/session, expiry and one-time nonce.
- Destination proves control of its authenticated session and accepts the transfer.
- In one serialized transaction: verify source generation is current; verify destination is authorized/live; fence source; increment generation; activate destination binding; mark transfer accepted.
- Do not leave a period in which both endpoints can act. If atomic cutover cannot be guaranteed, fail closed and require reconnect.
- Emit audit/outbox events only after state mutation commits.

### 5.4 Reconnect

- Obtain fresh authentication, resolve the same `AI_ID`, validate endpoint registration and owner policy.
- Reacquire binding using expected generation/version and a new generation.
- Re-fetch pending messages after attach; WebSocket reconnect alone is not proof that all prior messages were delivered.
- If the previous binding is still live, require an explicit transfer or wait for lease expiry; do not silently steal it.

### 5.5 Fencing and single-owner invariant

Use PostgreSQL as the authority, not in-process mutexes or client timestamps.

Required invariants:

- one phone has at most one active binding;
- binding generation is strictly monotonic;
- every mutating phone request includes the current generation;
- server verifies authenticated session, principal-to-AI mapping, endpoint registration, binding ID, generation and lease expiry;
- every transfer/reconnect increments generation;
- expired/revoked session or execution invalidates the binding;
- stale generation always returns a fenced/precondition failure;
- unique partial index + `FOR UPDATE` transaction prevents two concurrent attaches from succeeding.

Phone fencing is a separate domain generation. It must not reuse or mutate `team_tasks.fencing_token`; the two protect different resources.

## 6. Mission continuity integration

Target conceptual chain:

```text
AI_PHONE_ID
    -> AI_ID
        -> authenticated current SESSION_ID + ENDPOINT_ID
            -> Team Room principal/participant/execution
                -> mission_id
                    -> canonical team_tasks.task_id
                        -> task lease + task fencing_token
                            -> persisted checkpoint + checkpoint_version
                                -> authenticated task handoff/recovery
```

Actual current mapping:

- `team_tasks.mission_id` is a UUID, unique within tenant + room; there is no durable global mission registry table in these migrations.
- `scripts/mission-runner.mjs` keeps local mission metadata/state and uses the remote Team Room task as the authority for ownership/checkpoint operations.
- Task owner is the principal/participant/session/execution tuple, not AI_PHONE_ID.
- Checkpoint JSON and version live in PostgreSQL.
- Handoff targets a specific recipient tuple and advances task fencing on acceptance.

Future resume flow:

1. AI-Phone resolves `AI_ID` and current authenticated session.
2. Mission service lists missions/tasks visible to that AI identity under tenant/room ACL.
3. If the task is still owned by the same live session/execution, continue only with its valid task lease/fence.
4. If the old session is revoked or lease expired, do not reuse its ownership. Run the existing recovery path and create an authenticated handoff addressed to the new session/execution.
5. New execution accepts handoff; server advances task fence and returns the persisted checkpoint.
6. Runner restores mission context from PostgreSQL checkpoint and continues under the new task lease.

**Open compatibility gate:** current handoff code and CI proof establish distinct recipient identities. The exact case “same global AI_ID, new session/execution, same mission” must receive dedicated PostgreSQL integration coverage. Do not infer that phone attach automatically transfers task ownership. If current handoff authorization or candidate selection rejects this same-AI case, extend the handoff domain narrowly with a same-AI rebind path that still requires fresh auth, owner policy, new task fence and audit evidence.

## 7. Persistent AI-Phone messaging

### 7.1 Proposed model

Use a phone inbox/delivery model separate from Team Room channels:

- `ai_phone_messages`: immutable message envelope, `message_id`, `ai_phone_id`, sender identity/owner, client idempotency key, payload type/body reference, created timestamp, sequence number, expiry and metadata.
- `ai_phone_deliveries`: one row per target phone/recipient delivery, status, attempt count, next attempt, delivered timestamp, ack timestamp, last error and consumer binding generation.
- `ai_phone_message_events` or audit/outbox events: append-only state transitions.
- Payload storage must have size/type limits, encryption and explicit retention policy. Large attachments should use scoped object references, not inline blobs.

### 7.2 State machine

```text
PENDING -> DELIVERED -> ACK
    |          |
    +--------> FAILED (terminal or retry-exhausted)
```

- `PENDING`: committed to PostgreSQL and eligible for delivery. Remains pending while phone is disconnected.
- `DELIVERED`: destination endpoint received the message envelope; this is transport/application receipt, not proof the AI processed it.
- `ACK`: destination AI/client explicitly acknowledged processing or acceptance according to the chosen contract.
- `FAILED`: terminal failure after expiry, permanent authorization/payload error, or retry exhaustion; preserve reason and timestamps.

Transient delivery failures should normally remain retryable (PENDING with attempt metadata) until retry policy/TTL is exhausted. Avoid treating a transient network error as immediate permanent FAILED.

### 7.3 Idempotency and ordering

- Require caller-generated `client_message_id` scoped to `ai_phone_id + authenticated sender + operation`; enforce unique constraint.
- Repeated same key + same canonical payload returns the original message; same key + different payload returns conflict.
- Assign per-phone monotonic `sequence_no` transactionally under a phone row lock or a dedicated sequence allocator.
- Consumers request messages after the last acknowledged sequence; do not rely on timestamps for total ordering.
- ACK is idempotent and bound to message ID, destination phone, authenticated current AI identity, session, endpoint and binding generation.
- Delivery retries may cause repeated transport attempts; receiver deduplicates by message ID/sequence. Exactly-once network delivery is not a realistic guarantee; implement at-least-once delivery plus idempotent processing.
- Messages received while disconnected remain durably PENDING and are delivered after a successful attach/reconnect and ACL revalidation.
- Rebinding to a new endpoint must not reset sequence or expose messages belonging to another AI phone.

### 7.4 Relationship to existing messaging

Reuse existing PostgreSQL/outbox patterns where appropriate, but do not overload `team_room_messages` as the phone inbox. Room messages have channel/room ACL and channel-scoped idempotency. AI-Phone requires stable addressing independent of a room, per-phone ordering and per-recipient delivery/ACK semantics.

## 8. Preliminary bidirectional audio architecture

Target only:

```text
OWNER (authenticated human principal)
   -> explicit call authorization / consent
      -> AI_PHONE_ID -> AI_ID
         -> current authenticated endpoint/session
            <-> WebRTC encrypted media session
```

No call or audio media feature is implemented in the audited repository.

### 8.1 Recommended topology

Use WebRTC for real-time bidirectional media, preferably through a managed or self-hosted SFU for multi-device, NAT traversal and operational control. A direct peer-to-peer connection may be an optional optimization for simple two-party sessions, not the only production topology.

Components required:

1. **Call control API:** durable call ID, caller/owner, target AI_PHONE_ID, authorization, consent, state machine, timestamps, termination and audit.
2. **Signaling service:** authenticated WebSocket/HTTPS exchange for SDP offer/answer, ICE candidates, renegotiation and hangup. Signaling is not the audio transport.
3. **ICE/STUN/TURN:** STUN for candidate discovery; TURN relay for restrictive NAT/firewall environments. TURN credentials must be short-lived and scoped.
4. **WebRTC media transport:** DTLS-SRTP encrypted audio tracks, ICE connectivity checks, jitter buffer, packet-loss/congestion handling and reconnect logic.
5. **SFU/media infrastructure:** LiveKit, mediasoup or Janus are candidates to evaluate. Select after load, deployment, compliance, observability and cost testing; none is present/configured in this repo.
6. **Client runtime:** mobile/desktop audio capture/playback, device selection, microphone permission, audio focus, mute, output routing, background-call policy and interruption handling.
7. **AI audio adapter:** connect the live audio stream to an actually available voice/realtime model interface. The current GPT-6 product label does not prove a realtime audio API or credentials are available. Provider capability must be confirmed separately.
8. **Operations:** TURN bandwidth, SFU capacity, regional routing, metrics, abuse controls, retention, incident response and call-quality telemetry.

### 8.2 Call lifecycle and authorization

Suggested lifecycle: `REQUESTED -> RINGING -> ACCEPTED -> CONNECTING -> ACTIVE -> ENDED`, with `REJECTED`, `MISSED`, `FAILED` and `CANCELLED` terminal outcomes.

Require explicit owner authorization to call a specific AI_PHONE_ID and explicit permission for the AI endpoint to publish/subscribe audio. The existing `audio.publish` and `audio.subscribe` catalog entries can be reused as permission vocabulary only after call-scoped grants and consent are defined. Do not infer authorization from room membership.

Use one-time, short-lived signaling tickets bound to call ID, participant, session, endpoint and role. Revalidate authorization during call setup and on reconnect. Revoke media credentials on hangup, session revocation, endpoint transfer or owner cancellation.

No claim of implemented call, simulated call, live audio, WebRTC signaling or model voice integration is made.

## 9. Security threat model and controls

| Threat | Required controls |
|---|---|
| Phone takeover | Global AI mapping; owner-controlled enrollment; endpoint registration; attach challenge; row lock; lease; monotonic phone fencing; audit |
| Replay | One-time nonces/tickets; expiry; unique jti/idempotency constraints; generation-bound requests; replay audit |
| Endpoint hijacking | Endpoint registration and revocation; no caller-supplied arbitrary endpoint URL; challenge proof; TLS; generation-bound route |
| Stolen session | Short TTL; secure client storage; session revocation; endpoint binding; re-authentication; never treat phone ID as credential |
| Impersonation | Pinned OIDC issuer/audience/JWKS; stable subject mapping; explicit global AI-to-principal mapping; no name/model-string authentication |
| Message misrouting | Phone ID resolved server-side; tenant/AI ACL; immutable recipient; per-phone queue; ACK checks recipient and generation |
| Unauthorized calls | Owner consent; call-scoped grants; short-lived signaling credentials; audio.publish/subscribe authorization; revocation |
| History leakage | Per-phone ACL; tenant isolation; encryption at rest/in transit; no broad room-history fallback; retention and audit |
| Privilege escalation | Default-deny permissions; separate owner/admin grants; no permission inheritance from display role; least-privilege DB/service identities |
| Dual ownership / split brain | PostgreSQL transaction + row lock + partial unique active binding + monotonic phone generation + lease expiry |
| Cross-tenant confusion | Global AI ID mapped explicitly to tenant-scoped principal; every query includes tenant and resource scope |
| Message payload abuse | Size/type validation; malware scanning for attachments; rate limits; safe rendering; retention and redaction policy |

Additional requirements:

- Never store plaintext bearer tokens, OIDC assertions, TURN passwords or media secrets in the phone tables or audit metadata.
- Never log message body, call audio, authorization header or raw ticket.
- Treat owner authorization as a separate principal/relationship, not as the fact that a user knows an AI_PHONE_ID.
- Define data deletion/export and call recording consent before enabling recording/transcription.
- Add abuse/rate limits for attach attempts, inbox sends, call requests and signaling.

## 10. Implementation plan for the next AI

### Phase 0 — contracts and threat review

- Freeze the identity vocabulary and decide whether AI_ID is globally unique from day one (recommended) or a documented single-tenant principal alias.
- Define owner principal model and explicit owner-to-AI permissions.
- Define endpoint registration/proof and phone status/binding state machines.
- Write schema/API contracts and migration rollback/forward policy.
- No Gateway changes.

### Phase 1 — persistent identity and phone registry

- Add global AI identity and tenant-principal mapping tables.
- Add endpoint registry and AI phone table.
- Seed/register AI-01 through an explicit trusted provisioning workflow; treat “GPT-6” as profile metadata, not authentication.
- Add phone binding history, lease, generation, constraints and audit events.
- Add transactional attach/detach/reconnect and explicit transfer operations.
- Add concurrency tests proving exactly one active binding and stale-generation rejection.

### Phase 2 — continuity integration

- Add phone-aware mission lookup without replacing `team_tasks`.
- Bind current authenticated session/execution to AI_ID via validated principal mapping.
- Add dedicated PostgreSQL test for same AI identity reconnecting under a new session/execution and resuming the same mission through an authorized handoff.
- Preserve existing task fencing and checkpoint version rules.
- Test revoked session, expired lease, stale Runner, transfer race, recipient loss and repeated recovery.

### Phase 3 — durable phone inbox

- Add immutable message and per-recipient delivery schema.
- Add send/list/claim/ack/retry/dead-letter APIs.
- Implement per-phone sequence allocation and at-least-once delivery.
- Reuse outbox notification for wakeups only; PostgreSQL remains the durable source of truth.
- Test disconnected queueing, reconnect catch-up, duplicate sends, duplicate ACK, payload mismatch, out-of-order transport and endpoint rebinding.

### Phase 4 — audio proof of concept

- Confirm actual voice/realtime API availability for the selected model/provider and credentials.
- Select SFU and provision STUN/TURN.
- Build call-control and signaling service, then one-to-one WebRTC audio with explicit owner consent.
- Add session/endpoint transfer termination, call revocation, network interruption, NAT/TURN and mobile background tests.
- Only claim audio implemented after real devices and end-to-end media paths are validated.

### Phase 5 — production hardening

- Configure real OIDC issuer/workload federation and secure secret management.
- Add production PostgreSQL backups, monitoring, alerting, retention, privacy controls and rate limits.
- Load-test attach races, inbox throughput, delivery retries and media capacity.
- Perform independent security review and incident-response rehearsal.

## 11. Classification

### JÁ EXISTE

- Tenant-scoped AI/HUMAN principals.
- Workload identity registration and principal mapping.
- Participant and room membership records.
- OIDC assertion exchange, short-lived opaque sessions, token hashing, replay prevention and session revocation in code.
- Workload execution records and heartbeat.
- Presence state and last-seen tracking.
- Permission catalog, default-deny permission checks and security audit events.
- PostgreSQL tasks keyed by mission ID, leases, task fencing, checkpoint/version and recovery.
- Authenticated recipient-bound handoff with new task fencing.
- Persistent Team Room general/private messages, channel ACL, message idempotency, read watermark, outbox and authenticated WebSocket notifications.

### PRECISA SER IMPLEMENTADO

- Global permanent AI_ID (or a deliberately limited single-tenant alias policy).
- Owner-to-AI identity/phone authorization and provisioning.
- AI_PHONE_ID registry and phone lifecycle.
- Endpoint registry and endpoint revocation.
- Phone attach/detach/transfer/reconnect.
- Phone-specific binding lease, generation/fencing and exactly-one-current-endpoint invariant.
- Phone-to-mission lookup and same-AI/new-session continuity integration tests.
- AI-Phone inbox, delivery rows, PENDING/DELIVERED/ACK/FAILED, retry, ordering, idempotency and reconnect catch-up.
- Call control, consent, signaling, WebRTC client/media integration and call audit.

### DEPENDE DE INFRAESTRUTURA EXTERNA

- Production OIDC issuer/federation trust and workload attestation policy.
- Mobile/desktop endpoint distribution and secure credential storage.
- Push notification delivery (APNs/FCM) if background wake-up is required.
- TURN service and SFU/media infrastructure for reliable WebRTC.
- Actual supported realtime voice model/API and provider credentials.
- Production secrets management, TLS ingress, monitoring, backups and retention operations.

### NÃO PODE SER VALIDADO AINDA

- AI-Phone attach/transfer/reconnect: not implemented.
- Permanent global AI identity across tenants/providers: not implemented.
- Single-owner phone binding and phone fencing: not implemented.
- Same AI identity/new session resuming the same mission: not proven by existing CI evidence; requires dedicated integration test.
- Per-phone message delivery/ACK guarantees: not implemented.
- Live bidirectional audio/WebRTC: not implemented.
- Production OIDC federation and real model voice capability: external configuration/capability not evidenced here.

## 12. Risks and architectural decisions

1. **Do not overload `principal_id`:** it is tenant-scoped. A global AI_ID should be separate and explicitly mapped.
2. **Do not use `session_id` as phone identity:** sessions rotate and are revoked.
3. **Do not use `execution_id` as endpoint identity:** it describes a workload execution, not a durable endpoint installation.
4. **Do not use room membership as phone ownership:** a participant can leave a room while the phone identity must persist.
5. **Do not merge phone fencing with task fencing:** each resource requires an independent generation.
6. **Do not confuse outbox notification with delivery ACK:** durable delivery needs a per-phone consumer/delivery state.
7. **Do not infer audio from permission names:** media/signaling infrastructure is absent.
8. **Do not assume the model name authenticates the agent:** identity must come from a trusted issuer and owner-approved provisioning.
9. **Do not change Gateway:** the phone layer should call existing authenticated services and remain provider-neutral.

## 13. Validation evidence and limitations

This was a source/schema audit at the exact audited SHA. Files directly inspected include:

- `services/ai-team-room/ARCHITECTURE.md`
- `services/ai-team-room/migrations/0001_team_room_foundation.sql` through `0010_authenticated_task_handoffs.sql` (including task ownership, presence, communication and idempotent mission identity migrations)
- `services/ai-team-room/src/auth.rs`
- `services/ai-team-room/src/communication.rs`
- `services/ai-team-room/src/handoffs.rs`
- `services/ai-team-room/src/tasks.rs`
- `services/ai-team-room/src/main.rs`
- `services/ai-team-room/src/lib.rs`
- `scripts/mission-runner.mjs`
- `scripts/team-room-client.mjs`
- `reports/AI_HANDOFF_CONTINUITY_REPORT.md`

The continuity report records successful prior GitHub Actions evidence for PostgreSQL 16 lease/fencing, authenticated handoff, checkpoint recovery and independent Runner processes at implementation SHA `e18557c65753714bf926845639526b22c0309826`. The audited SHA `d1f2bd764361b14af0604104db8e670ee6f4f361` adds that report only; it does not change the implementation tree from its parent.

No new tests were run in this architecture-only mission. This report does not claim runtime validation of any AI-Phone functionality. It also does not claim production OIDC federation, real provider voice support, mobile app support or WebRTC deployment.

## 14. SHA and change-control record

- **Audited SHA:** `d1f2bd764361b14af0604104db8e670ee6f4f361`
- **Audited commit parent:** `e18557c65753714bf926845639526b22c0309826`
- **Audited commit changed files:** `reports/AI_HANDOFF_CONTINUITY_REPORT.md` only.
- **AI-Phone source implementation in this mission:** none.
- **AI Gateway changes in this mission:** none.
- **Merge performed:** no.

## 15. Final decision

**READY FOR IMPLEMENTATION — ARCHITECTURE ONLY.**

The existing Team Room identity, session, authorization, task continuity and communication primitives are a sound base for an additive AI-Phone domain. Implementation must begin with global identity mapping, owner authorization, endpoint registry, phone binding and phone fencing. Messaging and audio must be separate later phases.

This verdict does not authorize bypassing the current authentication model, weakening task fencing, treating existing room chat as a phone inbox, or claiming audio functionality before real infrastructure and end-to-end tests exist.
