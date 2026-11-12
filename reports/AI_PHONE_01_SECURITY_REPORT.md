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
