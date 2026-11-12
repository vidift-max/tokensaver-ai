# AI-PHONE-01 — Tenant Isolation Remediation Execution Report

**Classification: FIX_BLOCKED**

Date: 2026-11-12  
Branch: `fix/ai-phone-01-tenant-isolation`  
Baseline: `fcdbd9f913e016a5a50610e0d6c675f72e5c8974`

## 1. Identity and scope

The remediation branch was created directly from the requested PR #7 baseline. It is isolated: 10 commits ahead of the baseline and 0 behind at the time of this report.

Verified unchanged refs:
- `v2-saas-alpha`: `d1f2bd764361b14af0604104db8e670ee6f4f361` (compare against recorded SHA: identical).
- PR #8 implementation branch `mission/ai-phone-01-implementation`: `e164cdd2ebf45900d4d9d5b8aebb28e74724081e` (compare against recorded SHA: identical).
- PR #7 branch `mission/ai-phone-01`: identical to baseline `fcdbd9f913e016a5a50610e0d6c675f72e5c8974`.
- Adversarial proof branch `mission/ai-phone-01-tenant-isolation-proof`: unchanged at `c9a1f9053e9cbab8d431953b9d6cbef3532395dc`.

No merge, PR creation, persistent database access, production credentials, or real phone provisioning occurred.

## 2. Files changed

At the pre-report implementation head, the baseline comparison showed four changed files:
- `.github/workflows/ai-phone-tenant-isolation-fix.yml` — isolated PostgreSQL 16 CI gate; no `continue-on-error`.
- `reports/AI_PHONE_01_TENANT_ISOLATION_REMEDIATION_DESIGN.md` — rebaseline, resource matrix, migration safety strategy.
- `services/ai-team-room/migrations/0012_ai_phone_tenant_isolation.sql` — proposed tenant binding, phone partition/backfill, composite constraints and indexes.
- `services/ai-team-room/src/phone.rs` — partial tenant scoping of phone lookup, provisioning, endpoint, attachment, mailbox, ACK and mission-link paths.

This report is an additional documentation file.

## 3. Data model proposal implemented in migration

The migration introduces `ai_identity_tenants(ai_id, tenant_id)` as the canonical tenant binding of the still-global AI identity. It changes `ai_phones` to include `tenant_id`, removes global uniqueness on `ai_id`, and adds uniqueness on `(tenant_id, ai_id)`.

Composite foreign keys are added for:
- phone to canonical tenant/AI binding;
- endpoints to `(ai_phone_id, tenant_id, ai_id)`;
- attachments to both the composite phone and composite endpoint;
- messages to `(ai_phone_id, tenant_id, recipient_ai_id)`;
- mission links to the tenant phone and the Team Room task's `(task_id, tenant_id, room_id)`;
- current task pointer to `(current_task_id, tenant_id)`.

Mailbox sequence and idempotency remain scoped by the newly tenant-specific phone ID. Team Room remains the sole task authority.

## 4. Migration safety gate

The migration partitions legacy phone rows across distinct tenant bindings, preserving global `ai_id` and message/task IDs. Messages are reassigned using their existing `tenant_id`; mission links use the tenant and room of the canonical Team Room task.

A shared legacy phone with existing endpoint/attachment history is rejected because the legacy endpoint table does not carry tenant ownership. Guessing endpoint ownership would be unsafe. Such a database requires an explicitly reviewed endpoint-to-tenant backfill before the migration can run.

The migration is forward-only; consolidation back to a single global phone is not losslessly reversible. Recovery requires a pre-migration backup or a separately reviewed consolidation procedure.

**Important:** the migration has not yet been executed against PostgreSQL 16. Its syntax and data backfill are not yet certified.

## 5. HTTP and concurrency validation status

No post-fix authenticated HTTP adversarial run is available. The original proof test was not yet ported and inverted on this branch. Therefore the following remain unproven:
- distinct phone IDs for the same global AI across two tenants;
- cross-tenant mailbox read, DELIVERED, ACK, idempotency and sequence isolation;
- endpoint enumeration and attachment isolation;
- transfer isolation;
- mission-link independence and task-reference integrity;
- concurrent provisioning, attach, transfer, heartbeat, send, ACK and mission-link races;
- preservation of Team Room lease, fencing, checkpoint, handoff and Recovery Worker behavior under the corrected schema.

The existing proof test branch remains unchanged, but its adversarial test has not yet been incorporated into this remediation branch as a passing regression. No security assertion was removed or masked.

## 6. CI evidence

A dedicated PostgreSQL 16 workflow was added with ordinary failing semantics and artifact upload. However, no workflow Run ID, Job ID, artifact ID, PostgreSQL log, cargo-check result, or cargo-test result was verified for the final remediation SHA.

The workflow definition is not evidence that it ran. No CI success is claimed.

## 7. Required remaining work

1. Correct and validate the migration on a disposable PostgreSQL 16 database, including a legacy multi-tenant dataset with ambiguous endpoints that must fail closed.
2. Complete review of every SQL statement and query binding in `phone.rs`; current changes are partial and not yet compiler-verified.
3. Add explicit idempotent tenant phone provisioning and verify that `GET /ai-phone/me` is read-only and returns not found for an unprovisioned binding.
4. Port the original adversarial HTTP proof and invert each exploit assertion into a passing regression without deleting the scenario.
5. Add cross-tenant idempotency-key and sequence tests.
6. Add concurrency tests for all operations in the mission.
7. Run full `cargo check` and `cargo test` against disposable PostgreSQL 16 on the exact final SHA; archive and hash the evidence.
8. Verify Team Room task authority regressions.

## 8. Final decision

`FIX_BLOCKED`

The branch contains a structural remediation design, a fail-closed migration draft, and partial tenant-scoped query changes. It does **not** satisfy the required evidence gates. In particular, no final HTTP proof, PostgreSQL 16 migration execution, concurrency run, or exact-SHA CI evidence exists. Do not merge, do not approve PR #7 based on this branch, and do not describe AI-PHONE-01 as fixed.
