# AI-PHONE-01 — Implementation Report

**Repository:** `vidift-max/tokensaver-ai`  
**Base branch:** `v2-saas-alpha`  
**Implementation branch:** `mission/ai-phone-01-implementation`  
**Initial SHA:** `d1f2bd764361b14af0604104db8e670ee6f4f361`  
**Code SHA validated:** `515f1afbe6173b53977d73ba1f2153bc5ed961b5`  
**Report scope:** first functional persistent AI identity + AI-Phone + endpoint portability slice  
**Decision:** **GO CONDITIONED**

## 1. Executive result

The first functional AI-Phone-01 slice is implemented on a dedicated branch. It creates a global AI identity, a permanent phone ID, a phone endpoint registry, and session/execution-bound phone attachments with independent monotonic fencing and leases.

The Team Room service authenticates every operation using its existing bearer-session authentication. The AI-Phone service then verifies that the authenticated identity is an enabled AI principal with an active workload identity, session, execution, participant and room membership. The client does not supply an AI_ID or AI_PHONE_ID when attaching; the server resolves the phone through the authenticated principal binding.

The PostgreSQL 16 integration tests executed successfully and demonstrated:

- AI identity and phone registration are idempotent for the same authenticated principal and display key;
- endpoint A attaches to the permanent phone;
- a second live endpoint cannot silently take over;
- explicit transfer to endpoint B advances the phone fence;
- the old attachment is rejected after transfer;
- a new Team Room session reconnects to endpoint B while retaining the same AI_PHONE_ID;
- lease expiry allows a new fenced attachment;
- detach succeeds and stale detach is rejected;
- two concurrent attach requests produce exactly one active owner.

**This is not a complete production phone product.** Owner-to-AI authorization/provisioning, cryptographic endpoint attestation, endpoint revocation API, background lease sweeper, device client, messaging, and audio remain pending. The verdict is therefore GO CONDITIONED, not unconditional production GO.

## 2. Scope and non-goals

Implemented:

- permanent global `AI_ID`;
- explicit tenant-scoped Team Room principal binding;
- permanent `AI_PHONE_ID`;
- persistent endpoint registry;
- authenticated attach, detach, reconnect, transfer and heartbeat;
- phone-specific lease and fencing generation;
- audit trail;
- PostgreSQL integration tests and runtime evidence.

Not implemented by design:

- AI Gateway changes;
- task lease/fencing changes;
- performance benchmark changes or benchmark execution;
- messaging inbox;
- WebRTC, audio calls or media streaming;
- mobile/desktop endpoint application;
- production owner enrollment/consent workflow.

The existing `team_tasks.fencing_token`, task lease operations, checkpoints and handoff code were not modified.

## 3. Files changed

The final implementation diff from the requested initial SHA contains four files:

| File | Change |
|---|---|
| `.github/workflows/ci.yml` | Service test command uses `-- --show-output` so successful integration tests expose their generated evidence IDs in CI logs |
| `services/ai-team-room/migrations/0011_ai_phone_01.sql` | New AI identity, phone, endpoint, attachment and audit schema |
| `services/ai-team-room/src/main.rs` | Registers the new phone router without replacing Team Room or communication routers |
| `services/ai-team-room/src/phone.rs` | New phone domain service, HTTP handlers, transactional ownership protocol and PostgreSQL tests |

No Gateway file or benchmark implementation file was changed.

## 4. Database model and migration

Migration: `services/ai-team-room/migrations/0011_ai_phone_01.sql`

### 4.1 Tables created

- `ai_identities`: global UUID identity and globally unique display key; status and timestamps.
- `ai_principal_bindings`: explicit mapping from an AI identity to a tenant-scoped Team Room principal. A tenant principal can bind to only one AI_ID.
- `ai_phones`: one permanent phone per AI identity; immutable AI owner; lifecycle status; independent binding generation and version.
- `ai_phone_endpoints`: multiple endpoints per phone; endpoint type, status, client-generated stable endpoint key, registration principal, last session/execution, last activity and lifecycle timestamps.
- `ai_phone_attachments`: append-only attachment history with phone, AI, endpoint, tenant, principal, participant, session, execution, idempotency key, lease, status and fencing generation.
- `ai_phone_audit_events`: append-only registration, attach, detach, transfer, expiry and heartbeat audit events.

### 4.2 Database invariants

- One `ai_phone` per `ai_id`.
- One active attachment per phone, enforced by a partial unique index.
- One phone binding generation per attachment and strictly increasing phone generation.
- Phone identity (`ai_id`) cannot be changed after creation.
- Attachment lifecycle permits only `ACTIVE → DETACHED | EXPIRED | TRANSFERRED`.
- Phone fencing is separate from task fencing; no `team_tasks` row or task token is reused.
- Foreign keys bind phone, endpoint, principal, participant, session and execution records.

## 5. Identity and persistence

### 5.1 Identity separation

The implementation preserves four distinct identifiers:

| Identifier | Persistence | Meaning |
|---|---|---|
| `AI_ID` | Permanent | Global logical AI identity |
| `AI_PHONE_ID` | Permanent | Stable phone resource/address |
| `ENDPOINT_ID` | Endpoint lifecycle | Registered runtime/device |
| `SESSION_ID` | Temporary | Authenticated Team Room session |

`EXECUTION_ID` remains a separate short-lived workload execution.

The AI_ID is generated by the server. The phone is created in the same registration transaction. The authenticated principal mapping is explicit and tenant-scoped; the global AI identity itself is not tenant-scoped.

### 5.2 Registration behavior

`POST /ai-phone/register` accepts a display key (for example, `AI-01`). It derives the principal from the authenticated bearer session, verifies that the principal is an active AI workload, and atomically creates:

1. global `ai_identities` row;
2. `ai_principal_bindings` row;
3. permanent `ai_phones` row.

A transaction-scoped advisory lock serializes first registration for the same tenant principal. Repeating the same registration returns the existing identity and phone; using a different display key for the same binding returns conflict. A display key already owned by another identity also conflicts.

The model name/provider is not used as an authentication credential.

## 6. Endpoint registry

`POST /ai-phone/endpoints` registers an endpoint under the phone resolved from the authenticated AI principal.

Supported categories:

- `MOBILE`
- `DESKTOP`
- `WORKER`
- `BROWSER`

The server generates `endpoint_id`; the caller supplies a stable UUID `endpoint_key` for idempotent endpoint registration. The endpoint records the registering tenant/principal and, after attach, the responsible session/execution and last activity.

An endpoint identifier is not a bearer credential. Requests still require a valid authenticated Team Room session and server-side phone/principal binding.

## 7. HTTP API

All routes are registered by the Team Room service and use the existing bearer-session authentication.

| Method | Route | Purpose |
|---|---|---|
| GET | `/ai-phone/me` | Return the authenticated AI's phone, endpoint registry and derived connectivity/current attachment |
| POST | `/ai-phone/register` | Idempotently provision AI_ID + AI_PHONE_ID |
| POST | `/ai-phone/endpoints` | Register or idempotently retrieve an endpoint |
| POST | `/ai-phone/attach` | Acquire phone attachment with lease and new fence |
| POST | `/ai-phone/detach` | Detach current session's attachment using attachment ID and fence |
| POST | `/ai-phone/transfer` | Atomically transfer to another registered endpoint using expected source generation |
| POST | `/ai-phone/heartbeat` | Renew the active phone lease and endpoint activity |

Lease duration is bounded to 15–300 seconds; default is 60 seconds.

Relevant response mapping:

- 400 invalid request;
- 401 invalid/expired/revoked authentication;
- 403 authorization/ownership mismatch;
- 404 phone or endpoint not found;
- 409 active-owner conflict or idempotency mismatch;
- 412 stale fencing generation;
- 500 database/internal failure.

## 8. Attach, detach, reconnect and transfer

### 8.1 Attach

Attach validates the live authenticated Team Room identity, resolves the phone from `ai_principal_bindings`, locks the phone row with `SELECT ... FOR UPDATE`, verifies the endpoint belongs to that phone, and checks the current attachment.

If a different live attachment exists, attach returns conflict. If the existing attachment lease has expired or its session/execution/workload identity is no longer live, the old attachment is marked `EXPIRED`. The phone generation is incremented, and a new attachment is created.

Repeated requests with the same phone idempotency key, endpoint and session return the original attachment/generation. Reusing the key with different binding data conflicts.

### 8.2 Detach

Detach requires the current authenticated session/execution, attachment ID and exact generation. It changes `ACTIVE → DETACHED` transactionally. Repeating detach for the same already-detached attachment is idempotent. A stale generation or another session cannot detach a newer attachment.

### 8.3 Reconnect

Reconnect does not revive a revoked Team Room session. The client must authenticate a new session/execution. The service resolves the same AI identity and permanent phone, expires the invalid prior attachment, and attaches the new session to a registered endpoint with a strictly newer phone generation.

### 8.4 Transfer

Transfer requires a current authenticated session mapped to the same AI identity, a registered target endpoint and the caller's expected source generation. In one transaction it marks the source attachment `TRANSFERRED`, releases the old endpoint state, increments the phone generation and attaches the target endpoint.

A concurrent stale request cannot use the prior attachment generation. This first version uses an atomic server-side cutover; it does not yet persist a multi-party transfer-intent/acceptance workflow.

### 8.5 Lease expiration

Expired or revoked attachments are detected during attach/reconnect and transitioned to `EXPIRED`. Connectivity returned by `GET /ai-phone/me` is derived from the attachment lease plus live session, execution and workload identity.

There is no background expiry sweeper in this first slice. Expired rows may remain marked `ACTIVE` until the next attach/reconnect performs lazy expiry; they are not treated as connected and cannot renew their lease.

## 9. Real integrated test scenario and IDs

Evidence was emitted by the PostgreSQL 16 integration test at code SHA `515f1afbe6173b53977d73ba1f2153bc5ed961b5`, workflow run `43215757602`, job `131174587737`.

<badge>Runtime-generated CI IDs</badge>

These are actual UUIDs emitted by the successful test execution, not fabricated or hard-coded IDs.

| Resource | Actual ID |
|---|---|
| AI_ID | `7740f574-df51-4913-90b0-89df30b55c52` |
| AI_PHONE_ID | `6f44dbee-2e5e-42f6-bac1-a295439e015f` |
| Tenant | `21e4b1e7-e3ad-4650-b23c-07919243c06a` |
| Team Room principal | `caceba98-2306-4f65-a990-9f7c1c7a3885` |
| Endpoint A | `ea4b4681-cfb3-4b15-8f83-6a1a8403e7ac` |
| Endpoint B | `51781930-3c51-4501-844e-00f55568571c` |
| Session A | `16dc907e-793d-45c4-932f-8003c9c54840` |
| Session B | `b8dfba37-d72e-4173-852b-26a32979223e` |
| Execution A | `0d7abd0c-2635-49de-84eb-3d5bf6383efe` |
| Execution B | `ee64ea09-ff0f-4fb1-a373-f24eb643e337` |
| Initial attachment A | `4fe97b83-eef9-4625-b701-e178465549ca` |
| Transfer attachment B | `cf438c4d-f6fc-4479-8394-2b402122a84b` |
| Reconnect attachment (Session B / Endpoint B) | `f870c786-4f15-403d-946d-b0844b6865d6` |
| Recovered attachment after lease expiry | `e0e788bb-c59c-4053-9a40-26f871a72c4a` |

### 9.1 Executed lifecycle

```text
AI_ID 7740f574-df51-4913-90b0-89df30b55c52
  -> AI_PHONE_ID 6f44dbee-2e5e-42f6-bac1-a295439e015f
  -> Endpoint A / Session A / Execution A
  -> attach accepted, phone generation 1
  -> Endpoint B live takeover attempt rejected
  -> explicit transfer to Endpoint B accepted, generation 2
  -> old Endpoint A attachment rejected by stale fence
  -> Endpoint B heartbeat accepted
  -> Session A revoked; Session B + Execution B created
  -> reconnect to Endpoint B accepted, same AI_PHONE_ID, generation 3
  -> lease deliberately expired in PostgreSQL
  -> reattach/recovery accepted, generation 4
  -> detach accepted
  -> stale detach rejected
```

### 9.2 Boolean assertions emitted by the test

| Assertion | Actual result |
|---|---|
| Identity registration idempotent | `true` |
| AI_PHONE_ID stable on reconnect | `true` |
| Live competing takeover rejected | `true` |
| Old endpoint fenced | `true` |
| New endpoint accepted | `true` |
| Expired lease reclaimed | `true` |
| Detach accepted | `true` |
| Stale detach rejected | `true` |

The concurrency test ran two attach calls concurrently against PostgreSQL. Exactly one returned success, the other conflicted, and the database contained exactly one unexpired active attachment for the phone.

## 10. Tests and CI evidence

### 10.1 Final code validation

- **Code SHA:** `515f1afbe6173b53977d73ba1f2153bc5ed961b5`
- **Workflow:** [TokenSaver AI CI — run 43215757602](https://github.com/vidift-max/tokensaver-ai/actions/runs/43215757602)
- **Team Room service job:** `ai-team-room-service`, job ID `131174587737`, conclusion **SUCCESS**
- `cargo check --manifest-path services/ai-team-room/Cargo.toml`: **SUCCESS**
- `cargo test --manifest-path services/ai-team-room/Cargo.toml -- --show-output`: **SUCCESS**

Test counts from the completed service job:

| Test target | Passed | Failed |
|---|---:|---:|
| Library/auth unit tests | 6 | 0 |
| Binary/service tests (includes both AI-Phone tests) | 10 | 0 |
| PostgreSQL admission integration | 3 | 0 |
| PostgreSQL auth integration | 4 | 0 |
| Doc tests | 0 | 0 |
| **Total** | **23** | **0** |

New AI-Phone tests:

- `phone::tests::ai_phone_lifecycle_covers_identity_endpoint_reconnect_fencing_takeover_and_expiry`
- `phone::tests::concurrent_attach_allows_exactly_one_phone_owner`

### 10.2 Additional CI evidence

- Keycloak/OIDC smoke job in run `43215757602`: **SUCCESS**. This is the existing ephemeral CI federation test, not production OIDC provisioning.
- Distributed Runner PostgreSQL lab for the same implementation commit, run `43215757931`: **SUCCESS**.
- At report authoring time, the broader frontend/Linux and Windows foundation jobs in TokenSaver AI CI were still running. They are not needed to establish the Team Room service's PostgreSQL test result; their final outcomes are not claimed here.

The service integration test executed against the CI PostgreSQL 16 service. No mock database or in-memory ownership map was used for the AI-Phone lifecycle/concurrency proof.

## 11. Security properties implemented

- AI identity is resolved from the authenticated server-side principal binding; request bodies cannot select another AI_ID/AI_PHONE_ID.
- The session, execution, workload identity, principal, participant and active room membership are revalidated before phone operations.
- Phone row locking serializes concurrent ownership mutations.
- A partial unique index enforces one active attachment per phone.
- A monotonically increasing phone generation fences old attachments.
- Stale heartbeat/detach operations fail with precondition/fencing errors.
- Attach idempotency key is unique per phone; key reuse with different binding data conflicts.
- Phone generation and attachment state transitions are guarded in PostgreSQL.
- Audit rows record successful identity/endpoint/attachment lifecycle operations.
- Phone fencing does not alter or reuse task fencing.

## 12. Limitations and BLOCKED items

The following are explicitly not complete:

1. **Owner-to-AI authorization:** no persistent human-owner relation, owner consent or owner-controlled enrollment workflow exists yet. The current initial registration is self-provisioning by an already authenticated AI workload principal.
2. **Global cross-tenant provisioning API:** the schema can represent multiple tenant-principal bindings for one global AI_ID, but this first HTTP API creates the initial binding only. Additional tenant bindings need a privileged provisioning workflow.
3. **Endpoint cryptographic proof:** `endpoint_key` is a client-supplied stable UUID, not a hardware-backed key, signed challenge or device attestation. A stolen valid AI session remains a security risk.
4. **Endpoint revocation/closure API:** schema statuses include `REVOKED` and `CLOSED`, but no dedicated endpoint revoke/close HTTP route is exposed yet.
5. **Background expiry worker:** expired attachment status is lazily finalized on attach/reconnect; no scheduled sweeper exists.
6. **Transfer intent/acceptance:** transfer is atomic and authenticated, but there is no durable two-party transfer invitation/acceptance protocol.
7. **Real mobile/desktop client:** no mobile application, push registration, secure device key storage or device-level lifecycle has been validated.
8. **Production deployment:** no production OIDC trust, production database rollout, backup/restore or external security review is claimed.
9. **Messaging and audio:** no phone inbox, SMS-like messaging, call control, WebRTC, STUN/TURN, SFU or audio path is implemented.
10. **15-phone fleet:** this mission provisions only the first test identity/phone. No fleet provisioning or 15-phone orchestration was attempted.

## 13. Next implementation steps

1. Add owner principal and owner-approved AI identity provisioning; remove open self-assignment of privileged display keys for production.
2. Add controlled cross-tenant AI principal binding with explicit privileged authorization and audit.
3. Add endpoint public-key enrollment/challenge proof, rotation and revocation.
4. Add endpoint close/revoke routes and background lease-expiry worker.
5. Add transfer intent/acceptance with one-time nonce and explicit destination proof if multi-session transfer semantics require it.
6. Add service-level integration tests for stolen/revoked endpoint credentials, owner denial, cross-tenant mapping, endpoint revocation, heartbeat expiry and transfer replay.
7. Deploy the service with production OIDC, TLS, database backups, monitoring, retention and operational runbooks.
8. Only after the ownership/endpoint security gate is closed, proceed to persistent messaging. Audio remains a separate later mission.

## 14. Change control and SHA record

- **Initial SHA:** `d1f2bd764361b14af0604104db8e670ee6f4f361`
- **Code SHA with successful PostgreSQL integration tests:** `515f1afbe6173b53977d73ba1f2153bc5ed961b5`
- **Files in implementation diff:** `.github/workflows/ci.yml`, `services/ai-team-room/migrations/0011_ai_phone_01.sql`, `services/ai-team-room/src/main.rs`, `services/ai-team-room/src/phone.rs`.
- **AI Gateway changes:** none.
- **Benchmark changes or benchmark execution:** none.
- **Task lease/fencing changes:** none.
- **Audio/WebRTC implementation:** none.
- **Merge:** not performed.
- **Pull request:** [PR #8 — AI-Phone-01 persistent identity and fenced endpoint portability](https://github.com/vidift-max/tokensaver-ai/pull/8), open and draft.

The report commit will be the final branch SHA; it changes documentation only. The implementation code SHA above is the exact SHA whose PostgreSQL tests and runtime UUID evidence are recorded.

## 15. Final decision

**GO CONDITIONED**

The first functional proof is complete: one global AI identity owns one persistent AI_PHONE_ID, and that phone survives session rotation, endpoint transfer, lease expiry and reconnect with monotonically increasing phone fencing. Concurrent attach is serialized and tested against PostgreSQL 16.

This is a GO for the next controlled implementation stage, not permission to expose production enrollment broadly. Owner authorization, endpoint cryptographic proof, endpoint revocation and operational lease expiry remain release gates before production use.
