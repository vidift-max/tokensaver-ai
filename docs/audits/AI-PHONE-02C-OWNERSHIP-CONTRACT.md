# AI-PHONE-02C — Ownership Contract and Deletion Policy

Status: **DRAFT — NOT APPROVED FOR EXECUTION**  
Immutable TARGET: `ebb8eeecfea087443c5f3eba7021e96cb5302fe7`  
Audit baseline: `f95cc647d05d70a1622ef2d245c7dc6b7b368b69`

## 1. Cleanup contract

Cleanup is a capability over a finite set of fixture-created row IDs. It is not a tenant deletion request.

Required scope:
- `fixture_id`: harness UUID recorded in a fixture manifest. TARGET schema has no fixture_id column; this is provenance, not a database ownership key.
- `tenant_id`: exact tenant created by fixture; never permission to delete the tenant.
- `ai_id`: global identity reference; never Phone-fixture-owned and never deleted by Phone cleanup.
- `ai_phone_id`: exact Phone row created by fixture and bound to tenant + AI.
- `principal_id`: exact fixture-created principal bound to tenant and AI mapping.
- Exact arrays of endpoint, operator-key, credential, message, attachment, mission-link, session, workload-execution, room-membership, task, handoff, channel, message, ticket and other fixture-created row IDs.
- For every ID: expected parent IDs, tenant scope, creation provenance and ownership class.

No wildcard discovery followed by deletion. No deletion by tenant, AI, label, prefix, or guessed association. A row without exact ID and verifiable ownership is preserved.

## 2. Ownership classes

1. **Fixture-exclusive** — created by this fixture and recorded by exact ID in its manifest.
2. **Phone-exclusive** — exact row belongs to one Phone/endpoint and is manifest-recorded, with no external dependency.
3. **Tenant-shared** — may be used by other Phones, principals, sessions or workflows in the tenant.
4. **Cross-tenant/global-shared** — global or referenced by multiple tenant bindings.
5. **Ownership-unproven** — provenance or dependency closure is incomplete. Preserve and abort if it blocks cleanup.

## 3. Ownership matrix (migrations 0001–0014 review)

| Resource/table | Ownership relation | Default class | Cleanup policy |
|---|---|---|---|
| Fixture manifest / fixture_id | Harness provenance; no schema column | Fixture-exclusive metadata | Retain manifest; never infer from labels |
| tenants | Root for many rooms, principals and services | Tenant-shared | Never delete during Phone cleanup |
| ai_identities | Global PK ai_id; may bind to many tenants | Cross-tenant/global-shared | Never delete |
| ai_identity_principals | Exact ai_id + tenant_id + principal_id | Tenant-shared unless exact fixture-created mapping | Delete exact mapping only after dependency check |
| ai_identity_tenants | Exact ai_id + tenant_id, backed by principal mappings | Cross-tenant/global-shared binding | Preserve unless exact mapping ownership and no remaining principals are proven |
| principals | Tenant-scoped; referenced by sessions, tasks, permissions, workloads and Phone mapping | Tenant-shared | Delete exact fixture principal only after all dependents are proven fixture-owned |
| participants | Tenant + principal; referenced by memberships/workloads | Tenant-shared | Exact fixture row and dependency closure required |
| ai_phones | Phone row bound to tenant + global AI | Phone-exclusive if manifest-proven | Delete exact Phone only after all children and external refs resolved |
| phone_endpoints | Endpoint belongs to Phone, tenant and AI | Phone-exclusive | Delete exact endpoint IDs only |
| phone_endpoint_operator_keys | Tenant + AI key; can authorize multiple endpoint operations | Tenant-shared within AI scope | Delete only exact key IDs created by fixture |
| phone_endpoint_credentials | Exact endpoint + Phone + tenant + AI | Phone-exclusive | Delete exact credential IDs after endpoint validation |
| phone_messages | Exact Phone + tenant; sender may be another principal | Phone-exclusive data, tenant-linked | Exact fixture IDs only; apply retention policy |
| phone_attachments | Phone + endpoint + principal + participant + session + workload execution | Phone link with external dependencies | Exact IDs only after every reference is validated |
| phone_mission_links | Phone-to-Team-Task relation | Shared dependency | Delete exact link; never delete task implicitly |
| rooms | Tenant-scoped root for sessions, tasks, channels, memberships | Tenant-shared | Never delete in Phone cleanup |
| room_memberships | Tenant + room + participant + principal + session | Tenant-shared unless exact fixture row | Exact fixture rows only |
| sessions | Tenant + room + principal; referenced by workloads, Phone attachments, WS tickets and handoffs | Tenant-shared | Exact fixture sessions only after dependent closure |
| workload_identities | Tenant + principal + issuer/subject | Tenant-shared | Exact fixture rows only; no tenant sweep |
| workload_executions | Tenant + room + participant + principal + session | Tenant-shared | Exact fixture rows only after dependent links |
| team_tasks | Tenant + room + owner principal/participant/session/execution; Phone can point to task | Tenant-shared | Never delete implicitly from Phone cleanup |
| task_lease_events | Task/lease history | Ownership/retention requires exact FK review | Preserve by default |
| task_handoffs | Tenant + from/to principals, participants, sessions and executions | Tenant-shared | Exact fixture handoff only after both sides validated |
| outbox_events | Tenant event stream; base schema FK cascade | Tenant-shared | Never invoke tenant cascade to clear Phone rows |
| audit_events | Tenant/principal/room audit trail | Tenant-shared audit data | Preserve |
| security_audit_events | tenant_id exists but reviewed migration has no tenant FK | Ownership-unproven / audit-retained | Preserve |
| auth_token_replays | Replay/idempotency state without Phone-fixture ownership | Ownership-unproven | Never delete in Phone cleanup |
| principal_permissions | Tenant + principal + optional room + permission | Tenant-shared | Exact fixture grants only |
| team_room_channels | Tenant + room | Tenant-shared | Preserve |
| team_room_channel_members | Channel/principal membership | Tenant-shared | Exact fixture row only |
| team_room_messages | Tenant + room + sender/channel | Tenant-shared / audit-like | Preserve by default |
| team_room_message_reads | Message/principal read state | Tenant-shared | Exact fixture row only |
| team_room_decisions | Tenant + room decision records | Tenant-shared / audit-like | Preserve by default |
| team_room_attachment_refs | Tenant + room attachment reference | Tenant-shared | Exact fixture row only; prove underlying attachment separately |
| team_room_ws_tickets | Tenant + room + session | Session-linked ephemeral | Exact fixture ticket only |
| permission_catalog | Global permission definitions | Cross-tenant/global-shared | Never delete |
| Unreviewed/remaining tables | No complete ownership proof yet | Ownership-unproven | Preserve and block |

## 4. Pre-delete validation and concurrency

One transaction must perform all validation before the first DELETE:

1. Lock tenant and exact Phone rows with row-level locks; lock relevant parent rows used to prove ownership.
2. Verify tenant exists and the exact (AI, tenant) binding exists.
3. Verify exact Phone row matches ai_phone_id + ai_id + tenant_id.
4. Verify principal and exact ai_identity_principals mapping.
5. Verify every endpoint belongs to supplied Phone/tenant/AI.
6. Verify each operator key belongs to supplied AI/tenant and its ID is explicitly listed.
7. Verify credentials, messages, attachments and mission links against complete expected composite ownership.
8. Verify all indirect IDs are manifest entries and all parent relationships match.
9. Detect unlisted RESTRICT/CASCADE dependencies and rows outside the manifest. Abort before deletion on ambiguity.
10. Execute exact-ID deletes in FK-safe order, validate row counts, then commit. Any mismatch rolls back.

Locks and constraints must be tested against concurrent ownership changes. FK existence alone does not prove fixture ownership. Broad tenant deletion is prohibited.

## 5. Current blockers

- Current helper ends with `DELETE FROM tenants WHERE tenant_id=$1`.
- It does not validate tenant/AI/Phone/principal associations before deletion.
- Current 10-cycle test creates tenant/principal/identity mappings/Phone rows, but no real endpoints, operator keys, credentials, messages, attachments or mission links.
- Current snapshot omits Team Room, sessions, tasks, workloads, handoffs, outbox and audit records.
- No injected rollback or adversarial ownership-concurrency test has been executed.
- TARGET has no fixture_id column; fixture provenance needs an explicit harness manifest or a separately reviewed test-only registry.

## 6. Approval

This is a conservative draft based on reviewed migrations 0001–0014, not authorization to execute cleanup. Unenumerated relations remain ownership-unproven. Rounds 3–8 remain blocked until this matrix is completed, implemented and tested.
