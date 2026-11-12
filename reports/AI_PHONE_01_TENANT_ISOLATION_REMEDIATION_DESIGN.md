# AI-PHONE-01 — Tenant Isolation Remediation Design

## Rebaseline (2026-11-12)

- PR #7 audit baseline: `fcdbd9f913e016a5a50610e0d6c675f72e5c8974`.
- PR #8 reported implementation head: `e164cdd2ebf45900d4d9d5b8aebb28e74724081e` (must remain untouched).
- `v2-saas-alpha` previously recorded at `d1f2bd764361b14af0604104db8e670ee6f4f361`; live branch head revalidation is pending the repository ref API.
- Adversarial proof branch: `mission/ai-phone-01-tenant-isolation-proof`, compared to baseline: 8 commits ahead, 0 behind, exactly two changed files (the proof workflow and `phone_postgres.rs`).
- New remediation branch: `fix/ai-phone-01-tenant-isolation`, created directly from the PR #7 baseline SHA above.
- PR #7 and #8 are open and are not modified by this branch.

## Resource boundary matrix

| Resource | Existing key / scope | Defect | Proposed key / scope |
|---|---|---|---|
| Global AI identity | `ai_identities.ai_id` | Must remain global; not itself an ownership key | Keep global `ai_id` |
| Principal binding | `(ai_id, tenant_id, principal_id)` | Binding has no unique tenant/AI pair key for dependent FK | Unique `(ai_id, tenant_id)`; retain principal binding |
| AI phone | Unique `ai_id` | One phone shared by all tenant bindings | Unique `(tenant_id, ai_id)`; composite FK to binding |
| Endpoint | `(ai_phone_id, endpoint_id)` | No tenant/AI ownership columns | `(endpoint_id, ai_phone_id, tenant_id, ai_id)`; composite phone FK |
| Attachment | Carries tenant/AI, but phone FK only global | Does not prove phone belongs to attachment tenant | Composite phone + endpoint + principal/session/execution FKs |
| Mailbox | `ai_phone_id`, tenant field; sequence/idempotency per phone | Global phone merges tenants; several queries scope only by phone | Phone ID is tenant-specific; composite FK `(ai_phone_id, tenant_id, recipient_ai_id)`; sequence and idempotency remain per phone |
| Mission link | PK `ai_phone_id`, task FK by task ID | One global link can be overwritten by another tenant | One link per tenant-specific phone; composite task FK `(task_id, tenant_id, room_id)` |
| Team Room task | Global task PK; tenant/room/owner columns | Must remain canonical task authority | Composite candidate key for reference integrity; Team Room remains owner of lease/fencing/checkpoint/handoff/recovery |

## Migration safety strategy

Migration `0012_ai_phone_tenant_isolation.sql` will preserve global AI identities and all message/task history. It will partition the legacy global phone row into one row per distinct `(tenant_id, ai_id)` binding, keeping the old phone UUID for the lexicographically first tenant and generating deterministic-per-migration UUIDs for additional tenant bindings. Messages and mission links are reassigned using their existing tenant/task ownership columns. Endpoints and attachments are tenant-tagged and constrained through composite foreign keys.

The legacy endpoint schema has no tenant column. A phone shared by multiple tenant bindings with existing endpoints/attachments cannot be safely attributed without an explicit owner mapping. The migration must fail closed for this ambiguous case rather than guess. It also fails if a phone has no tenant binding or a mission task's tenant cannot be reconciled. This is a deliberate deployment gate: an operator must produce a reviewed, tenant-to-endpoint mapping/backfill before retrying. No persistent database is accessed by this mission.

Rollback is not losslessly reversible after tenant rows are split and resources partitioned. The migration is forward-only; rollback requires restoring a pre-migration backup or a separately reviewed data-consolidation plan. No data deletion is part of the migration.

## Provisioning and API rules

- `GET /ai-phone/me` is read-only and returns 404 when the tenant/AI phone has not been explicitly provisioned.
- Provisioning is a separate authenticated, permission-gated, idempotent operation, keyed by `(tenant_id, ai_id)`.
- Every resource lookup/mutation resolves the caller's tenant binding and matches phone UUID, tenant UUID, and global AI UUID together.
- A phone UUID supplied in a path is never sufficient authority.
- Mailbox sequence and idempotency uniqueness remain scoped to tenant-specific phone UUIDs.
- Mission linking remains a pointer to Team Room tasks. AI-Phone does not implement task lease, fencing, checkpoint, handoff, or Recovery Worker logic.

## Validation gates

The final classification remains blocked until the original adversarial HTTP test is inverted into a passing regression (and retained), a second tenant with the same global AI gets a distinct phone, every cross-tenant resource attempt is rejected, migration runs on disposable PostgreSQL 16, and the full Rust check/test suite plus concurrency probes pass on the exact final SHA. A green compile or schema-only test is not sufficient.
