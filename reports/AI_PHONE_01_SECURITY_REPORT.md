# AI-PHONE-01 — Security Report

**Assessment date:** 2026-11-12  
**Scope:** tenant isolation, phone identity, endpoint/session continuity, and durable text mailbox.  
**Status:** implementation in progress; final verdict must follow exact-SHA PostgreSQL and CI evidence.

## 1. Confirmed baseline vulnerability

The adversarial proof branch `mission/ai-phone-01-tenant-isolation-proof` demonstrated the P0 defect against PostgreSQL 16 and HTTP:

```text
same global AI_ID in tenant A and tenant B
        -> same AI_PHONE_ID
        -> tenant boundary collapses at the phone/mailbox resource
```

Evidence reference:
- Proof branch commit: `c9a1f9053e9cbab8d431953b9d6cbef3532395dc`.
- PostgreSQL/HTTP proof workflow run: `43235366735`.
- The proof job intentionally failed its security assertion after reproducing the cross-tenant behavior. This is evidence of the vulnerable baseline, not a passing remediation test.

## 2. Required security invariants

1. `(tenant_id, ai_id)` resolves to at most one tenant-specific phone.
2. A global `ai_id` may exist in several tenants, but each tenant binding has a different `ai_phone_id`.
3. Phone UUIDs in paths/bodies never override the authenticated tenant.
4. Endpoints, attachments, messages, ACKs, and mission links are constrained by composite tenant-aware keys.
5. A principal cannot provision another tenant's AI phone.
6. A revoked endpoint ID cannot be re-registered as a new endpoint, and stale endpoint fences cannot mutate current phone state.
7. Message idempotency is scoped to the tenant phone and rejects a conflicting payload replay.
8. Team Room remains authoritative for task/lease/checkpoint/handoff/recovery state.

## 3. Structural controls implemented on the remediation branch

- Unique `(ai_id, tenant_id)` binding and tenant FK for identity bindings.
- Unique `(tenant_id, ai_id)` phone key plus composite candidate keys for dependent FKs.
- Tenant-aware endpoint, attachment, mailbox, mission, and task references.
- Explicit, permission-gated, idempotent `POST /ai-phone/provision`; `GET /ai-phone/me` is read-only.
- Message destination and idempotency lookups include the authenticated tenant.
- Inbox reads require the caller's own phone, a live session/execution, and an active attachment with the current fence.
- ACK operations require a live session/execution and the current attachment fence.
- Revoked endpoint UUID reuse is rejected.
- Durable sent-message history is scoped by tenant and sender principal.
- Migration 0012 fails closed when an already-provisioned legacy phone is shared across tenants; it never silently rekeys a tenant.

## 4. Adversarial regression matrix

The PostgreSQL integration test is being expanded to exercise two authenticated tenants with the same global AI identity, distinct phones, separate endpoints, and separate sessions. The expected negative results include:

| Attack | Expected behavior |
|---|---|
| Tenant A reads B phone presence/endpoints/mailbox | 404 / concealed |
| Tenant B reads A phone presence/endpoints/mailbox | 404 / concealed |
| A sends to B phone or B sends to A phone | 404 / concealed |
| A ACKs B message or B ACKs A message | rejected |
| A provisions B principal or B provisions A principal | rejected |
| Cross-tenant endpoint attachment row | composite FK rejects |
| Cross-tenant message row | composite FK rejects |
| Cross-tenant mission link row | composite FK rejects |
| Duplicate principal binding for one AI/tenant | unique constraint rejects |
| Provision without `phone.provision` | 403 |
| Concurrent provisioning | exactly one phone ID is created |
| Same idempotency key + same payload | existing message returned |
| Same idempotency key + different payload | conflict |
| Revoked endpoint reattach/replay | conflict |
| Old endpoint/session/fence after reconnect | rejected |

## 5. Explicit limitations

- A pre-0012 production-like legacy shared-phone dataset was not available. The migration contains fail-closed gates; exercising those gates against a real legacy snapshot is `NOT_RUN — ENVIRONMENT LIMITATION`.
- Browser-level UI E2E and accessibility testing are separate from the backend PostgreSQL suite.
- PSTN, external telephone numbers, audio, WebRTC, and voice calls are out of scope and are not claimed to work.
- No production database was accessed or modified.

## 6. Final evidence fields

The final report must fill in the exact branch HEAD, PostgreSQL 16 workflow run, Cargo check/test conclusions, frontend build status, and SHA-256 manifest. Until those fields are complete and green, do not classify the phone MVP as ready.


## 7. CHECKPOINT_PARTIAL — 2026-11-12

**Checkpoint recorded from:** `349ee06f1bd6223158c11d406ffa9404930105ba`  
**State at checkpoint:** implementation remains partial; no merge performed.  
**Latest exact-SHA CI evidence:** workflow `43298891075`, commit `349ee06f1bd6223158c11d406ffa9404930105ba`, conclusion `success`.

### Completed / evidenced

- Tenant-scoped phone schema and composite constraints are present in migration `0012_ai_phone_tenant_isolation.sql`; explicit provisioning and read-only `GET /ai-phone/me` are implemented in the service.
- Phone, endpoint, attachment, message, ACK, presence, and Team Room mission/task paths have tenant-scoping changes.
- Endpoint UUID claim/replay handling was hardened, including race-safe inserts and stale/revoked endpoint rejection.
- A responsive React phone surface exists at `src/AIPhone.tsx` and is wired into `src/TeamRoom.tsx`.
- Architecture, migration-compatibility, remediation-design, and security reports exist.
- GitHub Actions ran PostgreSQL **16.15**. On the exact code SHA above, `cargo check --manifest-path services/ai-team-room/Cargo.toml` passed and `cargo test --manifest-path services/ai-team-room/Cargo.toml -- --show-output` passed. The log reports test binaries: 6 + 9 + 3 + 4 + 1 tests, all passing (23 total).
- The passing PostgreSQL test `phone_identity_survives_session_rotation_and_endpoint_change` includes the phone continuity, tenant-separation, structural-FK, endpoint, and mailbox assertions added to that integration scenario.
- Evidence artifact: `ai-phone-tenant-isolation-fix-43298891075`, artifact ID `13980447172`.

### Partially completed / not yet accepted as complete

- Migration 0012 has fail-closed compatibility guards, but no real legacy production-like snapshot was available; migration against a legacy dataset remains `NOT_RUN — ENVIRONMENT LIMITATION`.
- The PostgreSQL CI suite proves the current integration fixture, not a production rollout or exhaustive proof of every listed cross-tenant resource/action.
- UI source is present; browser E2E, accessibility, responsive-device validation, and frontend production build are not evidenced in this checkpoint.
- The backend suite exercises a same-tenant AI-to-AI offline-message flow in the expanded integration test, but a separately deployed multi-process live communication demonstration is not evidenced.
- No production database was migrated or modified.

### Failed / superseded CI runs

- `43297948264` — failed at the PostgreSQL integration/adversarial test step; Cargo check passed.
- `43298134870` — failed at the PostgreSQL integration/adversarial test step; Cargo check passed.
- The later exact-SHA run `43298891075` passed. Do not treat earlier failures as current failures, but retain them as iteration history. The retrieved logs did not expose a reliable root-cause line for those two failures, so no cause is asserted here.

### Not run / outstanding

- Frontend build, browser E2E, accessibility checks, `cargo fmt --check`, and a separate full `cargo test` command outside the passing GitHub workflow: `NOT_RUN — ENVIRONMENT LIMITATION` / not evidenced.
- Migration execution against a representative pre-0012 legacy snapshot.
- Independent adversarial coverage for all endpoint/session/key/attachment/mailbox/message/ACK/task/mission/checkpoint/handoff permutations and concurrency/replay combinations.
- SHA-256 manifest of all delivered files.
- Final independent security review and final verdict.

### Next continuation actions

1. Continue from the branch HEAD recorded in the checkpoint report (the checkpoint-only commit may advance HEAD; re-read it before any code changes).
2. Review the complete migration 0012 against the actual predecessor schema and run it against a representative PostgreSQL 16 legacy snapshot.
3. Expand adversarial and concurrency tests for all acceptance-matrix rows, preserving Team Room as task/lease/checkpoint/handoff authority.
4. Run frontend build, browser E2E, accessibility, formatting, and exact-SHA CI; generate SHA-256 manifest.
5. Do not merge or claim `PHONE_MVP_READY_FOR_REVIEW` until all acceptance criteria have evidence.
