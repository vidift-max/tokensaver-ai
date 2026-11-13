# AI-PHONE-02C — Ownership Contract and Deletion Policy

Status: **DRAFT — NOT APPROVED FOR EXECUTION**  
Immutable TARGET: `ebb8eeecfea087443c5f3eba7021e96cb5302fe7`  
Audit harness: `9cacad2604a7b3208d283accce6edcd2f3d07065`  
PostgreSQL catalog evidence: workflow `43473339618`, job `132050673893`, artifact `14062092447`  
Catalog artifact SHA-256: `67b4a4192a80337184c946ae31325da5dd364e85ca90dd469b3c0c955af19d06`  
Catalog database: PostgreSQL `16.16`, `server_version_num=160016`

## 1. Terminology and hard boundaries

The applied target schema has **no `fixture_id` column**. A fixture UUID in the harness is provenance metadata only. It is not a persisted ownership key and does not establish that a caller-supplied row ID belongs to that fixture.

Keep these concepts separate:

- **Fixture provenance:** the harness manifest records which exact IDs a test created and when. It is external to the production schema.
- **Persisted ownership:** database-enforced tenant, AI, Phone, endpoint and parent-child relationships, verified by exact composite keys and catalog constraints.
- **Global identity:** `ai_id` in `ai_identities`; it may be bound to multiple tenants and is never a Phone fixture resource.
- **Tenant:** shared security and data boundary. A Phone cleanup is not tenant deletion.
- **Fixture ownership:** may be claimed only when the exact row was created by that fixture, is recorded in its manifest, all ownership edges match, and no unlisted dependent or external consumer exists.

### Ownership classes

1. **Fixture-exclusive** — exact ID created by this fixture and recorded in its manifest; all dependency edges independently verified.
2. **Shared** — usable/referenced by another Phone, principal, session, task, endpoint, channel, or tenant binding.
3. **Global** — global identity/catalog/replay state or cross-tenant resource.
4. **Retained by policy** — audit, security, durable history, outbox, or records whose retention must outlive the fixture.
5. **Ownership not proven** — missing provenance, incomplete FK, soft reference, cross-scope relation, or unknown consumer. Preserve; abort if it blocks a safe cleanup.

An exact ID alone is never proof of ownership. A foreign key proves referential integrity, not fixture provenance.

## 2. PostgreSQL catalog verification

The catalog was queried after applying the target migrations to a disposable PostgreSQL 16.16 database. The audit artifact contains 524 JSONL lines covering server identity, constraints, all public table columns, and non-internal triggers.

Observed catalog facts:

- 34 public tables were present, including SQLx's `_sqlx_migrations` bookkeeping table.
- Every cataloged PK, unique/check constraint and FK definition was emitted.
- FK delete actions were read from `pg_constraint`, not inferred from migration text.
- Seven non-internal triggers were found.
- Four `SET NULL` FK constraints were found; one is a composite FK that nulls only two named columns.
- `security_audit_events` and `auth_token_replays` have no inbound or outbound FK.
- `security_audit_events.tenant_id` is a soft reference only; it is not protected by a tenant FK.
- `audit_events.room_id` and `actor_principal_id`, `outbox_events.aggregate_id`, `task_lease_events.tenant_id/room_id/principal_id/session_id`, `team_room_attachment_refs.task_id`, `team_room_ws_tickets.principal_id`, `team_room_decisions.mission_id/participants[]`, and other scalar/array references are not all enforced by FKs. Treat them as soft references.
- No DELETE trigger was found in the seven-trigger catalog. The update/insert triggers listed below still have lifecycle/security effects and must not be bypassed by assumptions.

The catalog is the authority for the applied disposable schema at that commit. It does not prove application-level ownership for soft references or external storage.

## 3. Table ownership and FK matrix

The FK summary below lists the persisted primary key, the relevant foreign-key targets and actual delete action from the PostgreSQL catalog. Any relation not listed as an FK is a soft reference unless otherwise stated.

| Table | Primary key | FK dependencies and actual delete action | Ownership class | Safe cleanup policy |
|---|---|---|---|---|
| `tenants` | `tenant_id` | Parent of rooms, principals, audit_events, outbox_events, task_handoffs (CASCADE); ai_identity_tenants (RESTRICT) | Shared / retained by policy | Never delete from Phone cleanup |
| `ai_identities` | `ai_id` | Referenced by AI mappings, Phones, messages and operator scope (RESTRICT) | Global | Never delete |
| `ai_identity_principals` | `ai_id, tenant_id, principal_id` | AI identity and tenant principal (RESTRICT) | Shared | Preserve unless exact mapping provenance and zero external dependents are proven |
| `ai_identity_tenants` | `ai_id, tenant_id` | AI identity, tenant, and AI/principal mapping (all RESTRICT) | Shared | Preserve by default; never infer ownership from AI/tenant IDs |
| `principals` | `principal_id` | Tenant (CASCADE); referenced by participants, sessions, permissions, workload identities (CASCADE), Phone records (RESTRICT), and Team Room records | Shared | Exact fixture principal only after complete dependent closure; never tenant-sweep |
| `participants` | `participant_id` | Principal (CASCADE); referenced by memberships/workloads (CASCADE), Phone attachments (RESTRICT), tasks/handoffs (NO ACTION) | Shared | Exact fixture row only after membership, execution, task, handoff and Phone dependency checks |
| `rooms` | `room_id` | Tenant (CASCADE); parent of sessions, memberships, workloads, tasks, handoffs, channels, messages, decisions, attachments and WS tickets (mostly CASCADE) | Shared | Never delete from Phone cleanup |
| `sessions` | `session_id` | Room and principal (CASCADE); memberships reference (NO ACTION); Phone attachments (RESTRICT); handoff session references (SET NULL); WS tickets (CASCADE) | Shared | Preserve unless exact session and every dependent record are fixture-owned and approved |
| `room_memberships` | `membership_id` | Room, participant and principal (CASCADE); session (NO ACTION) | Shared | Exact fixture membership only; preserve otherwise |
| `workload_identities` | `workload_id` | Principal (CASCADE) | Shared | Exact fixture row only after external workload identity and auth dependencies are reviewed |
| `workload_executions` | `execution_id` | Room, participant and session (CASCADE); Phone attachments (RESTRICT); task owner binding (NO ACTION); handoff execution refs (SET NULL) | Shared | Exact fixture execution only after task, handoff, Phone attachment and session closure |
| `principal_permissions` | `permission_id` | Principal and room (CASCADE); permission catalog (NO ACTION) | Shared | Exact fixture grant only; no wildcard principal/tenant deletion |
| `permission_catalog` | `permission` | Referenced by principal_permissions (NO ACTION) | Global | Never delete |
| `auth_token_replays` | `issuer, token_jti` | No FKs | Global / retained by policy | Never delete in Phone cleanup |
| `security_audit_events` | `security_event_id` | No FKs; tenant/room/principal IDs are soft references | Retained by policy / ownership not proven | Preserve; tenant deletion does not cascade to it |
| `audit_events` | `audit_event_id` | Tenant (CASCADE); room and actor principal are soft references | Retained by policy | Preserve; tenant cascade would erase audit history |
| `outbox_events` | `outbox_event_id` | Tenant (CASCADE); aggregate type/ID are soft references | Retained by policy | Preserve; tenant cascade would erase pending/published event history |
| `team_tasks` | `task_id` | Room (CASCADE); owner principal/participant/session/execution refs (NO ACTION); referenced by lease events, handoffs and Phone mission links | Shared | Never delete implicitly from Phone cleanup |
| `task_lease_events` | `lease_event_id` | Task (CASCADE); tenant/room/principal/session are soft references | Retained by policy / shared | Preserve task/lease history; no Phone cleanup deletion |
| `task_handoffs` | `handoff_id` | Task, tenant and room (CASCADE); participant/principal refs (NO ACTION); session/execution refs (SET NULL) | Shared / retained by policy | Preserve handoff history; do not delete task or tenant to remove it |
| `team_room_channels` | `channel_id` | Room (CASCADE); creator principal (NO ACTION) | Shared | Never delete from Phone cleanup |
| `team_room_channel_members` | `channel_id, principal_id` | Channel and principal (CASCADE); added-by principal (NO ACTION) | Shared | Exact fixture membership only after channel ACL validation |
| `team_room_messages` | `message_id` | Channel and room (CASCADE); sender and parent message (NO ACTION) | Retained by policy / shared | Preserve durable room conversation history |
| `team_room_message_reads` | `channel_id, principal_id` | Channel and principal (CASCADE) | Shared | Exact ephemeral row only if independently proven; not part of Phone cleanup |
| `team_room_decisions` | `decision_id` | Room (CASCADE); author and reference message (NO ACTION); mission ID and participants array are soft references | Retained by policy / shared | Preserve decisions and governance history |
| `team_room_attachment_refs` | `attachment_id` | Channel and room (CASCADE); message and uploader (NO ACTION); task ID is a soft reference | Shared / ownership not proven | Preserve; never assume external storage object is fixture-exclusive |
| `team_room_ws_tickets` | `ticket_hash` | Room and session (CASCADE); principal ID is a soft reference | Session-linked ephemeral / shared | Exact ticket only after session/room provenance; not a tenant or Phone cleanup shortcut |
| `ai_phones` | `ai_phone_id` | Global AI identity and tenant binding (RESTRICT); current task/mission composite pointer (SET NULL on only current_task_id and current_mission_id) | Phone-exclusive only with manifest proof; otherwise shared | Exact Phone row only after all Phone descendants and external refs are resolved; tenant and AI remain |
| `phone_endpoints` | `endpoint_id` | Phone and composite Phone/tenant/AI (RESTRICT); credential lifecycle trigger on status update | Phone-exclusive only with manifest proof | Exact endpoint only after credentials and attachments are resolved |
| `phone_endpoint_operator_keys` | `operator_key_id` | AI/tenant binding (RESTRICT) | Shared within AI/tenant scope | Exact key only if fixture-created and no other endpoint/operator uses it; preserve otherwise |
| `phone_endpoint_credentials` | `credential_id` | Endpoint and Phone/tenant/AI (RESTRICT) | Endpoint-exclusive candidate; security-sensitive | Prefer revoke/expire and retain audit provenance; delete only under explicit retention approval |
| `phone_attachments` | `attachment_id` | Phone, endpoint, AI mapping, principal, participant, session, workload execution (all RESTRICT) | Cross-resource / ownership not proven until graph validated | Preserve by default; exact delete only after every parent and manifest edge matches |
| `phone_messages` | `message_id` | Phone, recipient AI/global identity and sender principal (all RESTRICT) | Durable data / retained by policy | Preserve by default; apply message retention policy, not fixture cleanup |
| `phone_mission_links` | `ai_phone_id` | Phone, tenant-scoped Phone and task/tenant/room (all RESTRICT) | Shared dependency | Exact link only; never delete Team Task or lease history |

## 4. Actual delete actions and cascade consequences

### Tenant deletion is destructive beyond the tenant row

Catalog confirms direct `ON DELETE CASCADE` from `tenants` to:

- `rooms`
- `principals`
- `audit_events`
- `outbox_events`
- `task_handoffs`

Catalog also confirms `ai_identity_tenants.tenant_id` is `ON DELETE RESTRICT`. A tenant delete can therefore either fail due to the binding, or—if code first removes that binding—continue into destructive cascades.

Transitive cascade paths include:

- Tenant → room → sessions, memberships, workload executions, tasks, handoffs, channels, messages, decisions, attachment refs and WS tickets.
- Tenant → principal → participants, sessions, memberships, workload identities, workload executions, permissions, channel memberships and message-read rows.
- Tenant → task/room → task lease events and task handoffs.
- Tenant → audit_events and outbox_events directly.

The observed test evidence is consistent with this: scenarios 1, 4 and 5 returned success while the tenant, audit row and outbox row disappeared. The global AI identity in scenario 4 survived because the helper removed the tenant binding and global AI itself is protected by RESTRICT references, not a tenant cascade.

### SET NULL actions

The applied schema has these actual SET NULL constraints:

- `ai_phones.current_task_mission_tenant_fk`: nulls only `current_task_id` and `current_mission_id` when the referenced Team Task/tenant/mission tuple is deleted; it does not null `tenant_id`.
- `task_handoffs.from_session_id`, `to_session_id`, `from_execution_id`, `to_execution_id`: become NULL when their referenced session/execution is deleted. The handoff row itself remains unless another cascade path deletes it.

### RESTRICT and NO ACTION

Phone endpoints, credentials, operator keys, attachments, messages, mission links and AI/tenant mappings are protected primarily by RESTRICT. Team Room relations include NO ACTION FKs that can block deletion. These constraints are not a substitute for explicit ownership validation: a failed DELETE may simply be rolled back after earlier statements, and a successful delete may still erase unrelated cascaded records.

## 5. Trigger inventory and non-FK side effects

The PostgreSQL catalog reports these seven non-internal triggers:

| Trigger | Table / event | Effect | Cleanup implication |
|---|---|---|---|
| `ai_identity_principal_kind_guard` | ai_identity_principals BEFORE INSERT/UPDATE | Requires enabled AI principal | Mapping validity rule; not ownership proof |
| `phone_attachment_fence_guard` | ai_phones BEFORE UPDATE | Fence must be monotonic and advance by at most one | Lifecycle updates must respect fencing |
| `team_room_outbox_notify` | outbox_events AFTER INSERT | Emits NOTIFY with outbox event UUID | Outbox records are durable; no delete trigger observed |
| `phone_endpoint_credentials_lifecycle_revoke` | phone_endpoints AFTER UPDATE OF endpoint_status | Revokes active credentials on DETACHED/REVOKED | Endpoint lifecycle changes mutate security state |
| `phone_message_transition_guard` | phone_messages BEFORE UPDATE | Message identity/content immutable; constrains state transitions | Message lifecycle is not a cleanup mechanism |
| `task_handoff_state_transition_guard` | task_handoffs BEFORE UPDATE OF status | Enforces legal handoff state transitions | Handoff history must not be silently removed |
| `team_tasks_fencing_guard` | team_tasks BEFORE UPDATE | Enforces owner/fencing/checkpoint monotonicity | Task ownership cannot be altered as a cleanup shortcut |

No non-internal DELETE trigger was found in the applied public schema. This does not cover external storage deletion, application hooks, event consumers, or relations represented only in JSON/UUID arrays.

## 6. Soft references and unresolved ownership

The following references are not fully enforced by composite FKs in the applied catalog and must be treated as ownership-unproven until application-level code and fixtures are audited:

- `security_audit_events.tenant_id`, `room_id`, `principal_id`.
- `audit_events.room_id`, `actor_principal_id`.
- `outbox_events.aggregate_type`, `aggregate_id`.
- `task_lease_events.tenant_id`, `room_id`, `principal_id`, `session_id`.
- `team_room_attachment_refs.task_id` and external `storage_ref`.
- `team_room_ws_tickets.principal_id`.
- `team_room_decisions.mission_id` and `participants[]`.
- Team Room single-column principal FKs that do not prove same-tenant membership.
- Any external object referenced by attachment metadata, JSON payload, storage key, or application-level identifier.

`auth_token_replays` has no tenant key or FK and is global issuer/JTI replay state. It must never be swept by Phone cleanup.

## 7. Required fixture manifest

The harness must record, before cleanup:

- fixture UUID (provenance only), harness commit, TARGET SHA, PostgreSQL version, scenario and creation timestamp;
- exact tenant ID, AI reference ID, Phone ID and principal ID;
- exact endpoint IDs, operator key IDs, credential IDs, attachment IDs, message IDs and mission-link IDs;
- exact room, participant, session, workload identity, workload execution, membership, task, lease event, handoff, channel, Team Room message, decision, attachment reference and WS ticket IDs if created;
- for every ID: expected parent IDs, expected composite tuple, creation result, owner class and whether it is retained;
- pre-cleanup counts and immutable snapshots of every relevant table, including soft-reference and security/audit/outbox tables.

The manifest must be generated by fixture creation code, not reconstructed from caller-supplied cleanup IDs. It must not claim ownership of global/shared rows.

## 8. Pre-delete validation and transactional requirements

A future cleanup implementation must perform all validation inside one transaction and before the first DELETE:

1. Lock the exact tenant, AI binding, Phone, endpoint and all parent rows needed to prove the graph; define and test a stable lock order.
2. Verify tenant existence but never treat it as deletable Phone scope.
3. Verify exact AI/tenant mapping and Phone tuple `(ai_phone_id, tenant_id, ai_id)`.
4. Verify principal kind, enabled status, tenant mapping and exact `ai_identity_principals` tuple.
5. Verify each endpoint tuple `(endpoint_id, ai_phone_id, tenant_id, ai_id)`.
6. Verify each operator key against exact `operator_key_id, tenant_id, ai_id`; reject IDs from other fixtures or unknown IDs.
7. Verify each credential, attachment, message and mission link against all actual FK parent tuples and manifest entries.
8. Enumerate inbound FKs and soft-reference consumers for every candidate row; no unlisted CASCADE/RESTRICT/NO ACTION dependency may be ignored.
9. Preserve tenant, global AI identity, AI/tenant bindings, audit, security audit, outbox, replay state, Team Room tasks and retained history.
10. On any mismatch, unknown row, missing manifest entry, unexpected dependent, or row-count discrepancy: perform zero DELETEs, roll back, return an explicit error and emit structured evidence.
11. Only after validation, perform exact-ID deletes in FK-safe order for resources explicitly approved as fixture-exclusive. Verify row counts and postconditions before commit.

Locks and constraints must be tested against concurrent ownership changes. A successful FK lookup is not fixture ownership. No deletion by tenant, AI, label, prefix, wildcard, or guessed association is permitted.

## 9. Approval status and blockers

**This contract remains NOT APPROVED for cleanup execution.**

Reasons:

- Three of five independently executed adversarial scenarios still deleted a valid tenant and cascaded audit/outbox rows.
- Two scenarios were blocked by RESTRICT constraints, not by an ownership validator.
- The catalog is now verified, but soft references and external dependencies remain outside FK enforcement.
- The existing 10-cycle cleanup test still does not create all Phone descendants (notably real operator keys, credentials, attachments, messages and mission links in every cycle) and its snapshot is incomplete.
- No rollback-injection, idempotency, ownership-change race, unlisted dependent, or complete dependency-closure test has been approved and executed.
- The current helper still contains broad tenant deletion and has not been modified in this audit branch.

TARGET remains immutable. No production code has been changed. Rounds 3–8 remain blocked until the contract is independently reviewed and approved. Do not run 160 races, 20 handoffs, integration or operational cleanup under this contract.
