# AI-PHONE-01 — Migration Compatibility and Operator Decision Report

**Migration:** `services/ai-team-room/migrations/0012_ai_phone_tenant_isolation.sql`  
**Target:** PostgreSQL 16  
**Policy:** fail closed; preserve existing phone identities; no automatic tenant rekey.

## Compatibility rules

| Legacy condition | Migration behavior | Required action |
|---|---|---|
| One tenant binding for an existing phone | Keep the exact existing `AI_PHONE_ID`; add tenant ownership | No identity decision required |
| Same existing `AI_PHONE_ID` resolves to multiple tenant bindings | Abort before data changes | Operator must approve tenant-to-phone mapping and a separate rekey/backfill |
| Multiple principal bindings for the same `(ai_id, tenant_id)` | Abort before data changes | Resolve which principal owns that tenant's AI identity |
| Phone without a principal/tenant binding | Abort | Repair the identity binding; do not infer tenant |
| Message tenant does not match the phone's AI binding | Abort | Reconcile message ownership from audit/business evidence |
| Attachment tenant does not match the phone's AI binding | Abort | Reconcile endpoint/attachment ownership |
| Current task belongs to a different tenant than the phone binding | Abort | Reconcile the task pointer; Team Room remains task authority |
| Mission link references a missing task or incompatible tenant | Abort | Repair from Team Room's canonical task record |
| Endpoint or attachment ownership cannot be proven | Abort | Produce an explicit reviewed mapping before retrying |

The migration does not delete phone, message, endpoint, attachment, or task history. It does not select a tenant by lexicographic order and does not create replacement IDs for other tenants automatically.

## Preflight query

Run against a read-only snapshot before applying migration 0012:

```sql
SELECT
    p.ai_phone_id,
    p.ai_id,
    array_agg(DISTINCT ip.tenant_id ORDER BY ip.tenant_id) AS tenant_ids,
    count(DISTINCT ip.tenant_id) AS tenant_count
FROM ai_phones p
JOIN ai_identity_principals ip ON ip.ai_id = p.ai_id
GROUP BY p.ai_phone_id, p.ai_id
HAVING count(DISTINCT ip.tenant_id) > 1;
```

**Expected result for automatic migration:** zero rows. Any returned row is a hard stop.

Also inspect duplicate tenant bindings:

```sql
SELECT ai_id, tenant_id, array_agg(principal_id ORDER BY principal_id) AS principals
FROM ai_identity_principals
GROUP BY ai_id, tenant_id
HAVING count(*) > 1;
```

**Expected result:** zero rows.

## Operator decision required for a shared legacy phone

For every shared legacy `AI_PHONE_ID`, the operator must approve:

1. The tenant that retains the existing `AI_PHONE_ID`.
2. A new, distinct `AI_PHONE_ID` for each other tenant.
3. The authoritative tenant ownership of every message, endpoint, attachment, mission link, and current task pointer.
4. The mapping evidence and approval record.
5. The order of the data copy/relink and post-migration verification.

This decision is not inferred by this migration. A separate reviewed migration must consume the approved mapping and prove row counts, per-tenant histories, and referential integrity before it can be considered safe.

## Rollback

Migration 0012 is forward-only. A lossless generic down migration would have to merge tenant-specific identities and could reintroduce the vulnerability. Operational rollback is restoration from a verified pre-migration backup. A consolidation migration requires separate review and explicit approval.

## Validation status

- Fresh-schema migration on disposable PostgreSQL 16: recorded by the branch's AI-PHONE CI workflow; final exact-SHA result is reported separately.
- Legacy shared-phone fixture applied *before* migration 0012: `NOT_RUN — ENVIRONMENT LIMITATION`. The CI workflow starts from a fresh schema and does not contain a production-like pre-0012 data snapshot.
- Production database compatibility: not asserted; no persistent production database was accessed.
