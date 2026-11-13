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


## 8. AI-PHONE-01 closure validation — 2026-11-13

**Validated implementation SHA:** `3f5f715fd8790537e79871560f1de3c053897ba3`  
**Previous functional code SHA:** `349ee06f1bd6223158c11d406ffa9404930105ba`  
**Checkpoint SHA:** `ecc846dab33d0fe7f3cac35a1dd2e9e16ac7b8db`  
**Workflow:** `43336099326` — PostgreSQL 16 security job, conclusion `success`.  
**PostgreSQL:** `16.16`.

### 8.1 Real pre-0012 legacy migration execution

The new CI script `services/ai-team-room/tests/ai_phone_legacy_migration.sh` creates disposable databases, applies the actual migrations 0001–0011, seeds legacy records, and executes the unmodified migration file `0012_ai_phone_tenant_isolation.sql` inside a single transaction.

**Ambiguous/shared legacy case — fail closed:**

- Fixture: 2 tenants, one global AI identity, one shared legacy AI_PHONE_ID, 2 endpoints, 2 attachments, 2 messages, and 1 Team Room mission/task link.
- Before migration: `1:2:2:2:1` (phones:endpoints:attachments:messages:mission-links).
- PostgreSQL error at migration line 64:

```text
ERROR: AI-PHONE-01 migration blocked: an existing AI_PHONE_ID is shared across tenants; an operator must approve an explicit tenant-to-phone identity mapping before migration
```

- The migration exited non-zero as expected. The script verified the same row counts after rollback (`1:2:2:2:1`) and verified that the `ai_phones.tenant_id` DDL did not survive the rollback.
- Result: **FAIL-CLOSED / ROLLBACK / DATA PRESERVED**.

**Compatible legacy case — successful migration:**

- Fixture: one tenant binding for the AI identity, one phone, one endpoint, one attachment, one message, and one mission/task link.
- Migration 0012 completed successfully.
- Before and after counts remained `1:1:1:1:1`.
- The script verified the phone, endpoint, attachment, message, and mission link were all backfilled to the same authorized tenant.
- Result: **MIGRATION SUCCESS / DATA PRESERVED / TENANT BACKFILL VERIFIED**.

No production database was accessed or modified.

### 8.2 P0 adversarial and concurrency evidence

The PostgreSQL integration test ran with a configured disposable `DATABASE_URL`; the former success-shaped early return was replaced with a hard failure when the database URL is absent. The synthetic SQL block that imitated the migration guard was removed.

The passing PostgreSQL suite covers:

- The same global `AI_ID` bound in two tenants resolves to two distinct `AI_PHONE_ID` values.
- Cross-tenant phone send, presence, endpoint-list, and inbox reads are concealed/rejected.
- Cross-tenant ACKs are rejected.
- Cross-tenant provisioning and mission/task linking are rejected.
- Composite foreign keys reject cross-tenant attachment, endpoint, message, and mission/task relationships.
- A caller cannot claim another tenant's endpoint UUID.
- Concurrent provisioning returns exactly one `CREATED` and one `OK`, with the same resulting phone ID.
- Concurrent attach yields one winner and one conflict.
- Same idempotency key and same payload return the original message; same key with a different payload returns conflict.
- Duplicate ACK is idempotent.
- Revoked endpoint re-registration/reattach is rejected.
- Expired sessions and stale endpoint fences cannot mutate the current attachment.
- Session/endpoint rotation preserves the stable phone identity.
- Offline messages remain durable and can be delivered/ACKed after reconnect.

**Exact workflow test totals:** 6 + 9 + 3 + 4 + 1 = **23 passed, 0 failed**. The phone integration test `phone_identity_survives_session_rotation_and_endpoint_change` passed on this SHA.

### 8.3 Build and quality

- `cargo check --manifest-path services/ai-team-room/Cargo.toml`: **PASS**.
- `cargo test --manifest-path services/ai-team-room/Cargo.toml -- --show-output`: **PASS — 23 tests**.
- `npm install` and `npm run build` (TypeScript + Vite production build): **PASS**.
- `cargo fmt --manifest-path services/ai-team-room/Cargo.toml -- --check`: **FAIL — formatting differences** across the service, including `auth.rs`, `lib.rs`, `phone.rs`, `communication.rs`, `handoffs.rs`, `main.rs`, `tasks.rs`, and existing PostgreSQL test files. This diagnostic is explicitly non-blocking in the workflow; no broad formatter rewrite was applied because it would change files outside the P0 scope.
- Browser automation was not available in this execution environment: **BROWSER_E2E_NOT_AVAILABLE**. The production build is not represented as browser E2E evidence.
- Accessibility and device/browser responsive E2E remain unverified.

### 8.4 SHA-256 manifest and artifacts

The exact-SHA workflow generated `ai-phone-01-sha256-manifest.txt` and uploaded it with the test log.

Artifact:
- Name: `ai-phone-tenant-isolation-fix-43336099326`
- Artifact ID: `13997322222`
- Workflow run: `https://github.com/vidift-max/tokensaver-ai/actions/runs/43336099326`

SHA-256 values recorded by that run:

```text
9281af43659653b11738d5138fc4c7f0aebadd9bfde255048cd6ab22ec810526  services/ai-team-room/migrations/0012_ai_phone_tenant_isolation.sql
a419ff1e801b46727566fc344741e413fd95296ee6972446b4f098f68a919051  services/ai-team-room/src/phone.rs
87a5520ce6cd6ec007a2c458d1fba70422c67fb3a017ee9e137abfc71a422fd6  services/ai-team-room/src/handoffs.rs
0be97fe74ba38770ee4c2d3a3c6442f2989c37936a0c51bf7e2bbeeeec4e4230  services/ai-team-room/tests/phone_postgres.rs
577c2be749b1ca575b06da6576f2d6ce9c6001ca313fbdee489ea43ec8032e04  services/ai-team-room/tests/ai_phone_legacy_migration.sh
03e58be2b3d6e44b4615178bf7264aa2b96a751743eb9e80ff0c4c4d56eddf49  src/AIPhone.tsx
1b17f1d16c351cb1b5c7493daa8b3237dff7647e319ef18828419021d5e97714  src/TeamRoom.tsx
ae3ba6f74a74f40c8807a7cc83340b73819020fd5bcdbf23bfc1756a667d63b7  src/styles.css
332ff407d0bd60a80e2364d12d720cf7c4e4005f9d15b0f629f7b50b10455c8a  reports/AI_PHONE_01_SECURITY_REPORT.md
```

### 8.5 Upgrade policy

An installation with one legacy global phone mapped to more than one tenant is **not auto-repaired**. Migration 0012 blocks and rolls back. The operator must approve an explicit tenant-to-phone identity mapping and a reviewed endpoint/attachment history reconciliation before retrying. Messages, attachments, and Team Room task references must be preserved and assigned only through that authorized mapping. No tenant receives access to the ambiguous phone during reconciliation.

Compatible single-tenant legacy records can be migrated by 0012, with tenant ownership backfilled and verified.

### 8.6 Integrity and scope

- Changes after checkpoint SHA `ecc846d` are limited to the AI-PHONE validation workflow, the new legacy migration test script, and the phone PostgreSQL integration test.
- No reset, rebase, force push, or destructive history rewrite was performed.
- Branch `v2-saas-alpha` remained at `d1f2bd764361b14af0604104db8e670ee6f4f361`.
- PR #7 and PR #8 remained open and unmerged.
- No merge was performed and no production deployment was attempted.

### 8.7 Final verdict

**`PHONE_MVP_PARTIAL`**

The tenant-isolation remediation, real migration compatibility cases, provisioning, durable messaging, endpoint fencing, backend PostgreSQL suite, and frontend production build have passing evidence on the validated implementation SHA. The result is not elevated to `PHONE_MVP_READY_FOR_REVIEW` because `cargo fmt --check` remains failing, browser E2E/accessibility/device validation is unavailable, and independent security review is not complete. This is not a production-ready declaration.


## 9. Final closure review — 2026-11-13

### 9.1 Exact final state

- Repository: `vidift-max/tokensaver-ai`
- Branch: `fix/ai-phone-01-tenant-isolation`
- Implementation SHA validated by final CI: `673ef169709dbfea906128205f0fd544df0555a3`
- Previous validated implementation SHA: `3f5f715fd8790537e79871560f1de3c053897ba3`
- Previous functional baseline SHA: `349ee06f1bd6223158c11d406ffa9404930105ba`
- Final exact-SHA workflow run: `43339668102`
- Workflow conclusion: **success**
- PostgreSQL: **16.16**

Final CI: https://github.com/vidift-max/tokensaver-ai/actions/runs/43339668102

Artifact:
- Name: `ai-phone-tenant-isolation-fix-43339668102`
- Artifact ID: `13998693718`
- Includes `ai-phone-tenant-isolation.log` and the generated `ai-phone-01-sha256-manifest.txt`.

This is the authoritative final CI artifact. Earlier run artifacts are historical and should not replace this exact-SHA evidence.

### 9.2 Final exact-SHA results

- `cargo check --manifest-path services/ai-team-room/Cargo.toml`: **PASS**.
- `cargo test --manifest-path services/ai-team-room/Cargo.toml -- --show-output`: **PASS**.
- Test groups: 6 + 9 + 3 + 4 + 1 = **23 passed, 0 failed**.
- The phone PostgreSQL test `phone_identity_survives_session_rotation_and_endpoint_change`: **PASS**.
- Team Room recovery/handoff test `postgres_same_ai_phone_session_rotation_recovers_real_task_lease_and_fences_old_session`: **PASS**.
- Concurrent same-key message send: **PASS**; two concurrent requests converged to one durable message, one `200 OK` idempotent replay and one `201 Created`, with one persisted row.
- TypeScript check and Vite production build (`tsc && vite build`): **PASS**.
- Real legacy migration compatibility script: **PASS** in both ambiguous and compatible fixtures.
- Test execution fails closed when `DATABASE_URL` is absent; no early-return success path remains.
- The workflow's cargo test pipeline uses `set -o pipefail`, so a failing Cargo process cannot be hidden by `tee`.

### 9.3 Final migration evidence

The final run applied the real `0012_ai_phone_tenant_isolation.sql` against disposable PostgreSQL 16.16 databases created by applying migrations 0001–0011 first.

**Ambiguous legacy fixture:**
- Two tenants; same global AI identity and shared legacy phone.
- Two endpoints, two attachments, two messages, and one Team Room mission/task link.
- Before and after row counts: `1:2:2:2:1`.
- PostgreSQL rejected the migration with:

```text
ERROR: AI-PHONE-01 migration blocked: an existing AI_PHONE_ID is shared across tenants; an operator must approve an explicit tenant-to-phone identity mapping before migration
```

- Transaction rollback was verified; the `tenant_id` DDL did not survive.
- Result: **FAIL-CLOSED; legacy resource counts preserved**.

**Compatible legacy fixture:**
- One tenant binding and one phone, endpoint, attachment, message, and mission/task link.
- Migration succeeded.
- Before and after row counts: `1:1:1:1:1`.
- Phone and related resource ownership backfill to the authorized tenant was verified.
- Result: **SUCCESS; tested resources preserved and tenant-mapped**.

No production database was accessed.

### 9.4 Final adversarial security review

The final review checked authenticated tenant context, caller-supplied identifiers, phone ownership, endpoint claims, attachments, mailbox reads, message send/ACK, replay/idempotency, session expiry/revocation, endpoint fencing, mission/task links, handoff and recovery references, and composite foreign keys.

One authorization defect was found and corrected before the final green run:

- The phone mission-link handler mutated the canonical phone mission/task pointer while requiring `task.read`.
- It now requires `task.transition`.
- The PostgreSQL test proves `task.read` alone is rejected, `task.transition` plus task ownership permits a same-tenant link, and `task.transition` still cannot link a foreign-tenant task.

Final review result:
- Known unresolved P0 findings: **0**.
- Known unresolved P1 findings: **0** in the reviewed AI-PHONE paths.
- The authorization defect above is fixed and covered by the final passing test.
- This was an adversarial source review by the implementation/audit agent, not an external third-party penetration test or independent human security certification.

### 9.5 Team Room and browser validation

- Backend PostgreSQL Team Room continuity/handoff/recovery/fencing test: **PASS**.
- Full end-to-end flow across the rendered Team Room UI and a live AI client was not available in this environment: **TEAM_ROOM_E2E_NOT_AVAILABLE**.
- Browser/device/accessibility automation was not available: **BROWSER_E2E_NOT_AVAILABLE**.
- No visual/browser validation is claimed.
- No dedicated UI test suite was found in the package scripts; `test:missions` is not a substitute for AI-PHONE UI E2E.

### 9.6 Rust formatting assessment

The exact-SHA `cargo fmt --manifest-path services/ai-team-room/Cargo.toml -- --check` command still reports formatting differences. The workflow records this as a non-blocking diagnostic; the command itself is **not PASS**.

Files reported by rustfmt:

**Pre-existing files unchanged since the initial AI-PHONE remediation baseline `e052a9053618ac5931ef5d903e0f367808ffde5a`:**
- `services/ai-team-room/src/auth.rs`
- `services/ai-team-room/src/lib.rs`
- `services/ai-team-room/src/communication.rs`
- `services/ai-team-room/src/main.rs`
- `services/ai-team-room/src/tasks.rs`
- `services/ai-team-room/tests/admission_postgres.rs`
- `services/ai-team-room/tests/auth_postgres.rs`

**AI-PHONE mission files also reported by rustfmt:**
- `services/ai-team-room/src/phone.rs`
- `services/ai-team-room/src/handoffs.rs`
- `services/ai-team-room/tests/phone_postgres.rs`

No broad crate-wide formatting rewrite was applied. The seven pre-existing untouched files were not changed. The three mission-touched files remain a formatting task; for `handoffs.rs` and `phone_postgres.rs`, this run did not establish which individual formatting differences predate the AI-PHONE edits versus were introduced by them. A formatting-only scoped commit is still required before the formatting gate can be called clean.

### 9.7 Integrity and expansion scope

Changes after the previous documentary HEAD `9bdad2abeea5051d99dfcb20dfbbea3f46a5dca8` were limited to:
- `.github/workflows/ai-phone-tenant-isolation-fix.yml`
- `services/ai-team-room/src/phone.rs`
- `services/ai-team-room/tests/phone_postgres.rs`

The legacy migration fixture script was added earlier in this closure and is included in the final CI artifact and manifest.

Integrity checks:
- No reset, rebase, force-push, merge, or destructive history rewrite.
- `v2-saas-alpha` remains at `d1f2bd764361b14af0604104db8e670ee6f4f361`.
- PR #7 and PR #8 remain open and unmerged.
- No other AI-PHONE-02 through AI-PHONE-07 implementation files or provisioning work were added.
- The generic phone schema and the second tenant/phone in the PostgreSQL fixture are test/architecture coverage, not creation of the six expansion phones.
- No production provisioning or deployment was performed.
- No real credential was added; the CI database uses a disposable test-only credential.

### 9.8 Final verdict and recommendation

**`PHONE_MVP_PARTIAL`**

The P0 tenant isolation fix, real migration behavior, PostgreSQL security tests, provisioning, messaging, idempotency, fencing, backend Team Room handoff/recovery, and frontend production build have exact-SHA passing evidence.

The first AI-PHONE is a valid **architectural reference** for the design review, but it is **not yet approved as the operational baseline to start 1 → 7 replication**. Before replication:
1. Format the three mission-touched Rust files selectively and record the remaining pre-existing crate-wide formatting baseline separately.
2. Run a real Team Room-to-AI E2E flow when the environment is available (`TEAM_ROOM_E2E_NOT_AVAILABLE` remains unresolved).
3. Run browser smoke/E2E and accessibility checks when browser automation is available (`BROWSER_E2E_NOT_AVAILABLE`).

This is not a production-ready declaration and does not authorize creating the other six phones.
