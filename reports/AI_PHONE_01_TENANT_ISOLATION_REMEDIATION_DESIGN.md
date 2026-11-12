# AI-PHONE-01 — Tenant Isolation Remediation Design

## Rebaseline — 2026-11-12

- PR #7 baseline: `fcdbd9f913e016a5a50610e0d6c675f72e5c8974`.
- PR #8 reference head: `e164cdd2ebf45900d4d9d5b8aebb28e74724081e`; it remains untouched.
- The proof branch `mission/ai-phone-01-tenant-isolation-proof` contains the PostgreSQL/HTTP regression that demonstrated the shared-phone cross-tenant defect.
- The remediation continues the existing `fix/ai-phone-01-tenant-isolation` branch and does not merge PR #7 or PR #8.
- The implementation is based on the existing Team Room service; it does not replace its task, lease, checkpoint, handoff, or recovery ownership.

## Resource boundary matrix

| Resource | Required ownership key | Structural control |
|---|---|---|
| Global AI identity | `ai_identities.ai_id` | Remains global and is not an authorization boundary |
| AI principal binding | `(ai_id, tenant_id)` | Unique pair; one principal binding for a given global AI in a tenant |
| Tenant AI binding | `(ai_id, tenant_id)` | FK to the principal binding and tenant |
| AI phone | `(tenant_id, ai_id)` | Unique pair; `ai_phone_id` remains globally unique |
| Endpoint | `(endpoint_id, ai_phone_id, tenant_id, ai_id)` | Composite phone FK; endpoint IDs cannot be claimed by another tenant or reused after revocation |
| Attachment | Phone + endpoint + tenant + AI + principal/session/execution | Composite FKs and monotonic endpoint fence |
| Mailbox | `(ai_phone_id, tenant_id, recipient_ai_id)` | Composite FK; tenant-filtered reads/writes; per-phone sequence and idempotency |
| Mission link | `(ai_phone_id, tenant_id, room_id, task_id)` | Composite phone FK and composite Team Room task FK |
| Team Room task | `(task_id, tenant_id, room_id)` | Team Room remains the canonical owner of lease, fencing, checkpoint, handoff, and recovery |

## Migration safety policy

Migration `0012_ai_phone_tenant_isolation.sql` is deliberately fail-closed.

- A legacy phone with exactly one tenant binding retains its existing `AI_PHONE_ID`.
- If an existing `AI_PHONE_ID` is associated with more than one tenant, the migration aborts before changing data. It does not choose a tenant by sort order and does not silently assign replacement phone IDs.
- Duplicate `(ai_id, tenant_id)` principal bindings abort the migration.
- Orphan phone rows, message/attachment tenant mismatches, task-tenant mismatches, missing mission tasks, and endpoint/attachment ownership conflicts abort the migration.
- No existing message, task, endpoint, attachment, or phone row is deleted by the migration.
- New phones for additional tenant bindings are created only through explicit tenant-authorized provisioning after the migration.

If a legacy shared phone is detected, an operator must first approve a mapping containing the legacy `AI_PHONE_ID`, every affected `tenant_id`, the tenant that retains the old ID, the replacement IDs for other tenants, and the per-resource ownership evidence used to partition history. That mapping must be reviewed and applied by a separate migration. This mission does not guess the owner.

The migration is forward-only. A lossless automatic down migration is not possible after tenant-specific phone identities and references are established. Rollback requires restoring a pre-migration backup or a separately reviewed consolidation plan.

## Provisioning and API rules

- `GET /ai-phone/me` is read-only and returns 404 if the caller's tenant/AI phone has not been explicitly provisioned.
- `POST /ai-phone/provision` requires an authenticated live session and the explicit `phone.provision` permission; it is idempotent on `(tenant_id, ai_id)`.
- Identity binding and phone provisioning are separate operations.
- The same global `AI_ID` can be bound once in each tenant; each tenant receives a distinct `AI_PHONE_ID`.
- A phone UUID in a URL or request body is never sufficient authority. Every read and mutation is constrained by the authenticated tenant and principal binding.
- Endpoint attach/reconnect increments the fencing value; stale sessions/endpoints and revoked endpoint IDs are rejected.
- Mailbox history is durable in PostgreSQL. Offline sends are persisted, and the receiver reads them after reconnection. Idempotency keys reject conflicting replays.
- AI-Phone stores only a pointer to Team Room tasks. It does not implement a parallel task/lease/checkpoint/handoff/recovery system.

## Validation gates

A green compile alone is not sufficient. Acceptance requires PostgreSQL 16 migration execution, explicit provisioning tests, same-global-AI/two-tenant tests, bidirectional HTTP isolation, composite-FK rejection, replay and concurrency tests, endpoint revocation and fencing tests, and a frontend build. Any validation not executed on the exact final SHA must be reported as `NOT_RUN — ENVIRONMENT LIMITATION`.
