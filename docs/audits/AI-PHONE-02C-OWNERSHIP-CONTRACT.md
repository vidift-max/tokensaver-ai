# AI-PHONE-02C — Adversarial Ownership Review and Safe Cleanup Specification

Status: **BLOCKED — NOT APPROVED FOR IMPLEMENTATION OR EXECUTION**  
Immutable implementation TARGET: `ebb8eeecfea087443c5f3eba7021e96cb5302fe7`  
Audit branch: `audit/ai-phone-02c-execution`  
Harness scenario commit: `007f1de0c4670b3442cd76980e2a64df143b883d`  
Catalog workflow commit: `9cacad2604a7b3208d283accce6edcd2f3d07065`  
Current document baseline: `212ceed36bbcc17b168b33d94628caf5acde1a5a`  
No production code is changed by this specification.

## 1. Executive decision

The current cleanup implementation is unsafe. Three of five independent adversarial scenarios successfully deleted a valid tenant and cascaded deletion into audit/outbox data. The other two were stopped by RESTRICT constraints; they did not prove that cleanup validated ownership.

The existing 10-cycle isolation test is not a valid safety proof: its snapshots show zero endpoints, credentials, attachments, messages and mission links before cleanup, and it treats deletion of the tenant and identity mappings as successful cleanup. It does not cover the required resource graph.

**A safe physical deletion of an AI Phone is not currently specifiable for every state in the applied schema while simultaneously preserving retained children.** In particular, `phone_messages` has RESTRICT FKs to `ai_phones`; endpoint credentials have RESTRICT FKs to both endpoint and Phone; attachments have multiple RESTRICT parent FKs. If these rows must be retained, their Phone/endpoint parents must remain as anchors. The safe default for the current schema is therefore *deprovision/retire*, not unconditional physical deletion.

No implementation, migration, production-code change, operational cleanup, tenant deletion, 160-race run, 20-handoff run, integration, or merge is authorized by this document.

## 2. Evidence verification (Round 1)

### 2.1 Workflow and commit provenance

| Item | Verified value |
|---|---|
| TARGET | `ebb8eeecfea087443c5f3eba7021e96cb5302fe7` |
| Workflow 1 | `43472486661`, job `132047686824`, branch `audit/ai-phone-02c-execution`, head `007f1de0c4670b3442cd76980e2a64df143b883d` |
| Workflow 2 | `43473339618`, job `132050673893`, branch `audit/ai-phone-02c-execution`, head `9cacad2604a7b3208d283accce6edcd2f3d07065` |
| Current audit branch HEAD before this document update | `212ceed36bbcc17b168b33d94628caf5acde1a5a` |
| PostgreSQL | `16.16`, `server_version_num=160016` |
| Event | `push` |

The commit comparison `007f1de...` → `9cacad...` reports exactly one commit and one changed file: `.github/workflows/ai-phone-02c-dynamic-audit.yml` (+59 lines, no deletions). The five-scenario harness source was not changed by that commit. The later `9cacad...` → `212ceed...` comparison reports only the ownership-contract document changed. Thus the second workflow's `HARNESS_SHA=9cacad...` is the workflow/harness checkout commit, while the five scenario test source is inherited unchanged from `007f1de...`. This distinction must remain explicit in future reports.

The second run manifest records:
- `TARGET_SHA=ebb8eeecfea087443c5f3eba7021e96cb5302fe7`
- `TARGET_HEAD=ebb8eeecfea087443c5f3eba7021e96cb5302fe7`
- `HARNESS_SHA=9cacad2604a7b3208d283accce6edcd2f3d07065`
- `WORKFLOW_SHA=9cacad2604a7b3208d283accce6edcd2f3d07065`
- `RUN_ID=43473339618`
- `EVENT=push`
- PostgreSQL 16.16 build string as above.

### 2.2 Test results read from both workflow job logs

Both workflows produced the same five scenario outcomes and the same test-target summary:

| Test target | Result |
|---|---:|
| Rust unit tests | 6 passed, 0 failed |
| Main integration test target | 10 passed, 0 failed |
| Admission PostgreSQL target | 3 passed, 0 failed |
| `ai_phone_02c_network` | 12 passed, 3 failed |
| Total completed before suite abort | 31 passed, 3 failed |

The three failing assertions are the expected security failures:
- `ai_phone_02c_round1_case1_valid_tenant_random_ai_only`
- `ai_phone_02c_round1_case4_valid_tenant_nonexistent_operator_key`
- `ai_phone_02c_round1_case5_tenant_a_ids_from_different_fixtures`

The two tests reported as passing are negative tests whose expected behavior was a database FK error:
- `ai_phone_02c_round1_case2_valid_tenant_random_phone_only`
- `ai_phone_02c_round1_case3_valid_tenant_incompatible_principal`

The full suite stopped at the failing network test target. The frontend production build was skipped. `cargo check` and legacy migration compatibility completed successfully; neither is security approval.

### 2.3 Five scenario records verified in the original JSONL

The artifact `ai-phone-02c-dynamic-audit-43473339618.zip` contains `audit-evidence/cleanup-isolation-results.jsonl` with 26 JSONL records. The first six are the combined reproduction plus the five individual scenarios; the remaining 20 are ten A/B cleanup cycles.

| Scenario | Direct artifact observation | Assessment |
|---|---|---|
| Combined reproduction | `cleanup_returned_ok=true`; valid tenant no longer exists | FAIL |
| 1. Valid tenant + random nonexistent AI only | tenant true→false; audit 1→0; outbox 1→0; helper returned OK | FAIL |
| 2. Valid tenant + valid AI + random nonexistent Phone | FK `ai_phones_tenant_binding_fk` rejected removal of AI/tenant mapping; all snapshots unchanged | BLOCKED BY FK, not ownership proof |
| 3. Valid tenant + incompatible principal | FK `ai_identity_tenants_tenant_id_fkey` rejected tenant deletion; all snapshots unchanged | BLOCKED BY FK, not ownership proof |
| 4. Valid tenant + nonexistent operator key | helper returned OK; tenant, principal, Phone and audit/outbox rows disappeared; global AI survived | FAIL |
| 5. Tenant A + AI/Phone/principal IDs from fixtures B/C | helper returned OK; tenant A and its audit/outbox disappeared; B/C fixture records remained | FAIL |

The JSONL fields directly support these conclusions: `before`, `after`, `cleanup_error`, `cleanup_returned_ok`, `scenario`, `target_sha`, and `harness_sha`.

### 2.4 Artifact identity and hashes

GitHub's artifact API reported the following digests; local SHA-256 verification of the downloaded ZIPs matched them:

| Workflow | Artifact ID / name | SHA-256 |
|---|---|---|
| `43472486661` | `14059889216` / `ai-phone-02c-dynamic-audit-43472486661` | `0cdb65486aeacbc0acb84cbd3745182224356135521f667d00204e86c65b466f` |
| `43473339618` | `14062092447` / `ai-phone-02c-dynamic-audit-43473339618` | `67b4a4192a80337184c946ae31325da5dd364e85ca90dd469b3c0c955af19d06` |

The second ZIP contains:
- `ai-phone-02c-run-context.txt`
- `ai-phone-02c-evidence-manifest.txt`
- `audit-evidence/cleanup-isolation-results.jsonl`
- WebSocket evidence JSONL files
- `ai-phone-02c-full-suite.log`
- `ai-phone-02c-postgres-catalog.jsonl`

The catalog JSONL has 524 lines. The manifest includes SHA-256 entries for the audit harness files, target source files and migrations. The PostgreSQL catalog was captured after the test stage even though the security test target failed.

### 2.5 Ten-cycle test is not sufficient

The 20 records labelled `cleanup_A_preserves_B` and `cleanup_B_complete` show ten cycles. They are not complete Phone graphs:
- `phone_endpoints`, `phone_endpoint_credentials`, `phone_attachments`, `phone_messages`, and `phone_mission_links` are zero in the pre-cleanup snapshots.
- No complete populated session/workload/task/handoff/Team Room dependency closure is snapshotted.
- The `cleanup_B_complete` records show tenant, principal, Phone and AI mapping counts reaching zero. That test therefore accepts tenant/mapping deletion as completion, contrary to this mission's hard boundary.
- The snapshot does not include complete row contents or all 35 public tables.

These cycles demonstrate only a narrow synthetic A/B behavior under an incomplete fixture graph. They do not prove safe cleanup.

## 3. PostgreSQL catalog re-count and corrections

The actual catalog JSONL contains **35 public tables**, not 34:
- 34 application tables.
- 1 infrastructure table: `_sqlx_migrations`.

The constraint record counts are:
- 35 PRIMARY KEY constraints.
- 28 UNIQUE constraints.
- 52 CHECK constraints.
- 85 FOREIGN KEY constraints.
- 33 FK actions `ON DELETE CASCADE`.
- 29 FK actions `ON DELETE RESTRICT`.
- 18 FK actions `NO ACTION`.
- **5 FK actions `ON DELETE SET NULL`**, not four.

The five SET NULL constraints are:
1. `ai_phones_current_task_mission_tenant_fk` — nulls only `current_task_id` and `current_mission_id`.
2. `task_handoffs_from_execution_fk`.
3. `task_handoffs_from_session_fk`.
4. `task_handoffs_to_execution_fk`.
5. `task_handoffs_to_session_fk`.

The previous contract's “34 public tables” and “four SET NULL constraints” statements are incorrect. The earlier table matrix also omitted `_sqlx_migrations`. The matrix below accounts for all 35 catalog tables.

Seven non-internal triggers were captured. No non-internal DELETE trigger was found. Trigger definitions are in the catalog artifact.

## 4. Ownership classes and non-negotiable semantics

The required classifications are:

- `FIXTURE_EXCLUSIVE`: a *row-level* classification only. The exact row was created by this fixture, appears in a signed/immutable manifest produced at creation time, and every parent/ownership edge matches. No table is automatically fixture-exclusive.
- `PHONE_OWNED`: the row is structurally scoped to one exact Phone by enforced key relationships. This does not override retention, child references, or fixture-provenance requirements.
- `TENANT_SHARED`: tenant-scoped, AI/tenant-scoped, room-scoped, principal-scoped or otherwise available to more than one Phone/workflow.
- `GLOBAL`: global identity, permission catalog, replay protection or infrastructure state.
- `RETAINED`: security, audit, outbox, durable message, lease, handoff or governance history that must not be erased by Phone cleanup.
- `OWNERSHIP_UNPROVEN`: soft reference, missing FK, external object, incomplete provenance, unknown consumer or cross-resource relationship without a complete proof.

A row may be tagged `FIXTURE_EXCLUSIVE` in a manifest only if the underlying table's default classification and the row's actual relationships permit it. This does not permit deleting a `GLOBAL`, `TENANT_SHARED`, or `RETAINED` row. An ID supplied by a caller is not evidence of fixture provenance. An FK proves referential integrity, not fixture ownership.

**Tenant is never a Phone cleanup resource.** The tenant may be locked and validated as a scope anchor; it is never added to the delete set. AI identities, AI/tenant bindings, principals and audit/history are also preserved by default.

## 5. Complete 35-table ownership matrix (Rounds 2 and 4)

“Creator” below identifies the observed application writer/provisioning path where the reviewed TARGET source exposes one. The catalog itself cannot establish who created an existing row. Where a control-plane or external writer is not present in the reviewed runtime source, that uncertainty is stated explicitly rather than guessed.

| Table / primary key | Observed creator or writer | Logical owner and tenant boundary | Class / shared? | Survives Phone deletion? / tenant deletion? | FK guarantee and soft references | Dependencies, cascade effects and safe policy |
|---|---|---|---|---|---|---|
| `_sqlx_migrations` / `version` | SQLx migration runner | Database deployment metadata; no tenant | `GLOBAL`; infrastructure | Yes / yes | No tenant FK; not application data | Never include in fixture manifest or cleanup. Preserve deployment history. |
| `tenants` / `tenant_id` | Tenant provisioning/control plane and test fixtures; no Phone runtime creator | Root security boundary | `TENANT_SHARED`; shared by all tenant resources | Yes / No; tenant is the deletion root | Parent FK to rooms, principals, audit, outbox and handoffs; AI tenant binding is RESTRICT | Direct cascades can erase large tenant graph and retained history. Lock only for validation; never delete. |
| `ai_identities` / `ai_id` | `phone.rs` identity registration path | Global AI identity; may bind to multiple tenants | `GLOBAL`; cross-tenant shared | Yes / yes | Referenced by identity mappings, Phones, messages and other scope rows with RESTRICT | Never delete or infer ownership from tenant/Phone ID. |
| `ai_identity_principals` / `(ai_id, tenant_id, principal_id)` | `phone.rs` identity registration/binding | AI-to-principal mapping within tenant; unique `(ai_id, tenant_id)` | `TENANT_SHARED`; shared binding | Yes / tenant deletion is blocked until mapping is removed | FK to global AI and composite tenant/principal; trigger requires enabled AI principal | Not Phone-owned. Preserve by default; never remove to make tenant deletion succeed. |
| `ai_identity_tenants` / `(ai_id, tenant_id)` | `phone.rs` identity registration/binding | Global AI-to-tenant binding | `TENANT_SHARED`; cross-Phone binding | Yes / tenant deletion is blocked while binding exists | FKs to AI, AI/principal mapping and tenant, all RESTRICT | Never delete in Phone cleanup. It is an explicit boundary relation. |
| `principals` / `principal_id` | Tenant identity provisioning; fixture setup in reviewed tests | Tenant-scoped identity | `TENANT_SHARED` | Yes / No; tenant CASCADE | Tenant FK CASCADE; inbound relations include participant/session/permission/workload and Team Room rows; some principal FKs are single-column | Deletion can cascade many resources and may be blocked by Phone RESTRICT. Preserve; no principal-wide cleanup. |
| `participants` / `participant_id` | Tenant/room provisioning and fixture setup | Tenant participant identity | `TENANT_SHARED` | Yes / No; cascades from principal | FK to tenant+principal CASCADE; inbound memberships/workloads CASCADE; Phone attachment RESTRICT; task/handoff NO ACTION | Never delete as Phone child. Preserve unless a separate tenant lifecycle explicitly authorizes it. |
| `rooms` / `room_id` | Room provisioning/control plane and fixture setup | Tenant-scoped collaboration room | `TENANT_SHARED` | Yes / No; tenant CASCADE | Tenant FK CASCADE; inbound sessions, memberships, workloads, tasks, handoffs, channels, decisions, messages, attachments, WS tickets and permissions | A room delete is a broad graph deletion. Never delete from Phone cleanup. |
| `sessions` / `session_id` | `auth.rs` verified session exchange | Tenant + room + principal + workload context | `TENANT_SHARED` | Yes / No; room/principal cascades | Composite tenant/principal and tenant/room FKs CASCADE; Phone attachments RESTRICT; handoff refs SET NULL; WS tickets/workloads cascade | Revoke/close via lifecycle path if needed; preserve session history and dependent audit. Do not delete from Phone cleanup. |
| `room_memberships` / `membership_id` | Room join/presence path in communication service and fixtures | Tenant + room + participant + principal + session | `TENANT_SHARED` | Yes / No; room/participant/principal cascades | Composite room/participant/principal FKs CASCADE; session NO ACTION | Shared membership state. Do not delete based on Phone ID. |
| `workload_identities` / `workload_id` | Trusted workload/OIDC provisioning; `auth.rs` reads and updates last-seen/revocation | Tenant + principal + issuer/subject | `TENANT_SHARED` | Yes / No; principal CASCADE | FK tenant/principal CASCADE; external issuer/subject semantics are application-level | Revoke through identity lifecycle; preserve identity history; never infer Phone ownership. |
| `workload_executions` / `execution_id` | `auth.rs` session exchange | Tenant + room + participant + principal + session | `TENANT_SHARED` | Yes / No; session/participant/room cascades | Composite FKs to session, participant and room CASCADE; Phone attachments RESTRICT; task owner NO ACTION; handoff refs SET NULL | Shared execution context. Preserve unless independently approved lifecycle policy; Phone cleanup must not delete it. |
| `principal_permissions` / `permission_id` | Permission provisioning/control plane and fixtures | Principal/room permission grant | `TENANT_SHARED` | Yes / No; principal/room cascades | Principal and room FKs CASCADE; permission catalog NO ACTION | Revoke exact grant through authorization lifecycle; no wildcard deletion. |
| `permission_catalog` / `permission` | Migration seed / schema deployment | Global authorization vocabulary | `GLOBAL` | Yes / yes | Referenced by principal_permissions with NO ACTION | Never delete from Phone cleanup. |
| `auth_token_replays` / `(issuer, token_jti)` | `auth.rs` verified token exchange; expiry sweeper is global | Global issuer/JTI replay protection | `GLOBAL` | Yes / yes | No inbound or outbound FKs; no tenant column | Never delete in Phone cleanup. Existing expiry maintenance is a separate global TTL policy. |
| `security_audit_events` / `security_event_id` | `auth.rs`, `handoffs.rs` | Security history; tenant/room/principal are metadata references | `RETAINED` | Yes / yes; rows can outlive tenant | No inbound/outbound FKs; tenant_id, room_id and principal_id are soft refs | Preserve. Tenant deletion will not cascade to these rows and can leave orphaned scope identifiers. |
| `audit_events` / `audit_event_id` | `phone.rs` Phone event recorder | Tenant audit history; resource IDs are logical references | `RETAINED` | Yes / No; direct tenant CASCADE | Tenant FK CASCADE; room_id, actor_principal_id and resource_id are soft refs | Preserve; tenant deletion erases history. Phone deletion alone does not delete these rows. |
| `outbox_events` / `outbox_event_id` | `phone.rs`, `communication.rs`, `tasks.rs`, `handoffs.rs`, `main.rs` | Tenant event stream and downstream delivery history | `RETAINED` | Yes / No; direct tenant CASCADE | Tenant FK CASCADE; aggregate_type/aggregate_id are soft refs; INSERT trigger emits NOTIFY | Preserve pending and published events. Never delete tenant to clear Phone. |
| `team_tasks` / `task_id` | `tasks.rs` task creation | Tenant + room task; owner is principal/participant/session/execution tuple | `TENANT_SHARED` | Yes / No; room CASCADE | Composite owner FKs are NO ACTION; room FK CASCADE; Phone mission link references task RESTRICT | Never delete task as a Phone side effect. Lease events and handoffs depend on it. |
| `task_lease_events` / `lease_event_id` | `tasks.rs`, `handoffs.rs` | Durable lease/fencing history | `RETAINED` | Yes / No when task is removed through room cascade | FK to task CASCADE; tenant/room/principal/participant/session references are soft refs | Preserve. Task deletion cascades lease history; never use task deletion as cleanup. |
| `task_handoffs` / `handoff_id` | `handoffs.rs` | Durable task transfer history involving two principals/participants/sessions | `RETAINED` | Yes / No; direct tenant and room/task cascades | Task, tenant and room FKs CASCADE; participant/principal NO ACTION; session/execution refs SET NULL | Preserve. Deleting tenant, room or task erases handoff history; deleting session/execution can null history fields. |
| `team_room_channels` / `channel_id` | `communication.rs` | Tenant + room collaboration channel | `TENANT_SHARED` | Yes / No; room CASCADE | Room FK CASCADE; creator principal composite FK NO ACTION | Shared channel; channel deletion cascades members, reads, messages and attachment refs. Never delete from Phone cleanup. |
| `team_room_channel_members` / `(channel_id, principal_id)` | `communication.rs` | Channel membership for principal | `TENANT_SHARED` | Yes / No; channel/principal cascades | Channel and principal FKs CASCADE; added_by_principal_id single-column NO ACTION, not a same-tenant composite proof | Preserve unless exact membership lifecycle is separately approved. |
| `team_room_messages` / `message_id` | `communication.rs` | Durable tenant/room/channel conversation history | `RETAINED` | Yes / No; room/channel cascades | Room/channel FKs CASCADE; sender and parent-message FKs NO ACTION; sender FK is not tenant-composite | Preserve. Tenant/room/channel deletion can erase conversation history. |
| `team_room_message_reads` / `(channel_id, principal_id)` | `communication.rs` | Per-principal read state | `TENANT_SHARED` | Yes / No; channel/principal cascades | Channel and principal single-column FKs CASCADE; no composite same-tenant ownership proof | Ephemeral/shared state; not a Phone resource. |
| `team_room_decisions` / `decision_id` | `communication.rs` | Room governance/decision history | `RETAINED` | Yes / No; room CASCADE | Room FK CASCADE; author and reference-message NO ACTION; mission_id and participants[] are soft refs | Preserve. Soft mission/participant references require explicit policy; room/tenant deletion erases the record. |
| `team_room_attachment_refs` / `attachment_id` | `communication.rs` | Room/channel attachment metadata and external storage reference | `OWNERSHIP_UNPROVEN` | Yes / No; room/channel cascades | Channel and room FKs CASCADE; message/uploader NO ACTION; task_id and storage_ref are soft/external refs | Preserve. Never delete external storage object or reference without an independent storage ownership and retention proof. |
| `team_room_ws_tickets` / `ticket_hash` | `communication.rs` | Room/session ephemeral ticket; principal association | `OWNERSHIP_UNPROVEN` | Yes / No; room/session cascades | Room and session FKs CASCADE; principal_id is a soft reference | Exact ticket expiry/consumption is separate lifecycle. Do not infer Phone ownership. |
| `ai_phones` / `ai_phone_id` | `phone.rs` Phone creation/ensure path | One Phone row scoped by tenant + global AI | `PHONE_OWNED` | Parent anchor for Phone children; cannot be removed while retained RESTRICT children remain / tenant removal is blocked by AI binding until this graph is resolved | AI and AI/tenant binding FKs RESTRICT; current task/mission composite FK SET NULL on two columns | Candidate for exact Phone lifecycle only. Preserve if messages, credentials, attachments or other retained/unproven children depend on it. Never delete tenant or AI. |
| `phone_endpoints` / `endpoint_id` | `phone.rs` endpoint enrollment | Exact Phone + tenant + AI endpoint | `PHONE_OWNED` | No while credentials/attachments remain; endpoint is a credential parent / not a tenant cascade row | Phone and composite Phone/tenant/AI FKs RESTRICT; endpoint lifecycle trigger revokes active credentials on DETACHED/REVOKED | Deprovision by status transition. Physical deletion only after every child and retention decision is proven. |
| `phone_endpoint_operator_keys` / `operator_key_id` | Trusted control plane out-of-band (migration comment); no Phone service insert path identified | AI + tenant authorization scope, not endpoint/Phone scope | `TENANT_SHARED` | Yes / tenant deletion blocked by mapping until key removed | Only FK is AI/tenant binding RESTRICT; no FK to Phone or endpoint | Shared authorization credential. Revoke exact key via control plane; never delete because a Phone is removed. |
| `phone_endpoint_credentials` / `credential_id` | `phone.rs` endpoint enrollment/rotation | Exact endpoint + Phone + tenant + AI; security-sensitive credential history | `RETAINED` | Must remain with endpoint and Phone if retention is required / no direct tenant cascade, but blocks parent deletion | Composite endpoint/Phone/tenant/AI and Phone FKs RESTRICT; lifecycle trigger revokes active credential on endpoint detach/revoke | Revoke via endpoint status trigger; retain hashes/history by default. If retained, endpoint and Phone rows remain as anchors. Delete only under explicit retention approval. |
| `phone_attachments` / `attachment_id` | `phone.rs` attach/transfer lifecycle | Phone/endpoint binding to principal, participant, room, session and workload execution | `OWNERSHIP_UNPROVEN` until manifest and every parent tuple match | It is a Phone child but cross-resource parents remain / can block parent deletion | Eight composite/scope FKs use RESTRICT; references exact Phone, endpoint, AI mapping, principal, participant, session and workload execution | Prefer FENCED/DETACHED lifecycle and retain evidence. Exact physical delete only if fixture-exclusive and no policy/external consumer; never delete its parent session/workload. |
| `phone_messages` / `message_id` | `phone.rs` send path | Durable Phone message; sender principal and recipient global AI | `RETAINED` | Message row cannot outlive Phone under current FK; Phone cannot be deleted while message is retained / no direct tenant cascade, but sender principal may block tenant cascade | RESTRICT FKs to Phone, Phone/tenant/recipient AI tuple, global AI identity and tenant/principal sender | Preserve message and Phone anchor. Physical Phone deletion requires an explicit message retention/archive decision; current schema cannot both delete Phone and retain its messages. |
| `phone_mission_links` / `ai_phone_id` | `phone.rs` and `handoffs.rs` mission link/upsert | Phone-to-Team-Task relationship row; task itself is not Phone-owned | `PHONE_OWNED` relationship only when manifest-proven | Link cannot outlive Phone; task must survive / task and room may not survive tenant deletion | RESTRICT FKs to Phone and Team Task including tenant/room tuple | Exact link may be removed only with provenance; never delete task, lease events or handoffs. |

### 5.1 Important ownership conclusions

1. `phone_endpoint_operator_keys` are scoped to `(ai_id, tenant_id)`, not to a Phone or endpoint. The table has no Phone/endpoint FK. They are shared authorization material.
2. `ai_identity_principals` and `ai_identity_tenants` are identity/tenant boundary mappings, not Phone children.
3. `phone_messages` and `phone_endpoint_credentials` have RESTRICT dependencies that conflict with physical parent deletion when their history is retained.
4. `phone_attachments` bind several independently owned resources. Their Phone FK does not establish fixture-exclusive ownership of the principal, participant, session or execution.
5. `phone_mission_links` is a Phone-to-task relationship. The link may be Phone-owned; the task, lease history and handoff history are not.
6. `security_audit_events` and `auth_token_replays` have no FKs. They survive tenant/Phone deletion and must not be swept by inferred scope.
7. A Phone cleanup may not delete a tenant, room, principal, AI identity, AI/tenant mapping, Team Task, global replay state, audit/security history, outbox history, or Team Room governance history.

## 6. Cascade and FK action matrix (Round 2)

### 6.1 All 33 CASCADE edges

The catalog reports 33 FK constraints with `ON DELETE CASCADE`. They are grouped by referenced parent:

| Referenced parent | Cascading child tables |
|---|---|
| `tenants` | `rooms`, `principals`, `audit_events`, `outbox_events`, `task_handoffs` |
| `principals` | `participants`, `principal_permissions`, `room_memberships`, `sessions`, `team_room_channel_members`, `team_room_message_reads`, `workload_identities` |
| `participants` | `room_memberships`, `workload_executions` |
| `rooms` | `principal_permissions`, `room_memberships`, `sessions`, `task_handoffs`, `team_room_attachment_refs`, `team_room_channels`, `team_room_decisions`, `team_room_messages`, `team_room_ws_tickets`, `team_tasks`, `workload_executions` |
| `team_tasks` | `task_handoffs`, `task_lease_events` |
| `team_room_channels` | `team_room_attachment_refs`, `team_room_channel_members`, `team_room_message_reads`, `team_room_messages` |
| `sessions` | `team_room_ws_tickets`, `workload_executions` |

Some rows have multiple cascading parent FKs; this is why the child may be deleted by more than one path. The list above groups all 33 actual FK constraints, not merely unique child table names.

### 6.2 Tenant deletion is never a cleanup primitive

Direct tenant cascades erase:
- rooms and their sessions/memberships/workloads/tasks/channels/messages/decisions/attachments/WS tickets;
- principals and their participants, permissions, sessions, workload identities, memberships and Team Room memberships/reads;
- audit_events and outbox_events directly;
- task_handoffs directly and through task/room paths.

Other RESTRICT mappings may first block tenant deletion. Removing those mappings to “unblock” the delete does not make the operation safe; it opens the destructive cascade paths above.

### 6.3 SET NULL, RESTRICT and NO ACTION

- Five SET NULL constraints exist, enumerated in Section 3. The Phone composite pointer nulls only `current_task_id` and `current_mission_id`; it does not null Phone `tenant_id`.
- 29 RESTRICT FKs protect cross-resource Phone rows and AI/tenant bindings.
- 18 NO ACTION FKs protect relationships such as task ownership, parent messages, channel creator and some Team Room actors.
- FK enforcement is not fixture provenance. A successful FK check does not prove the referenced row belongs to the same fixture.
- A failed statement may follow earlier statements in the transaction; the whole transaction must roll back. A successful DELETE can still trigger unrelated cascades.

## 7. Independent adversarial review findings (Round 3)

| ID | Document location | Affected relation | Risk and evidence | Required correction |
|---|---|---|---|---|
| A-01 | Previous §2 catalog count | Entire public schema | Previous contract said 34 public tables. Artifact has 35, including 34 application tables plus `_sqlx_migrations`. | Correct count; inventory all 35, including infrastructure table. |
| A-02 | Previous §2 SET NULL count | Phone and task handoff pointers | Previous contract said four SET NULL FKs. Catalog contains five: one Phone composite pointer and four handoff session/execution pointers. | Correct count and enumerate all five. |
| A-03 | Previous §1 ownership classes and §3 matrix | Every table | Previous labels did not use the six required enum values and did not distinguish row-level fixture provenance from table-level default ownership. | Use exactly the six classifications in §4; assign FIXTURE_EXCLUSIVE only to exact manifest rows. |
| A-04 | Previous §3 Phone rows and §8 delete order | `phone_messages` → `ai_phones`; credentials → endpoints → Phone | Previous text says preserve message/credential history but suggests deleting Phone/endpoint after children are “resolved”. Actual RESTRICT FKs make physical parent deletion impossible while retained children remain. | Define deprovision/retire with retained parent anchors as default. Hard delete only after explicit retention approval and zero retained children. |
| A-05 | Previous §3 table matrix | All 35 tables | Matrix omitted creator, logical owner, tenant boundary, shared status, survival after Phone/tenant removal, soft refs and fail-safe condition as separate answers. It also omitted `_sqlx_migrations`. | Replace with the 35-row matrix in §5. |
| A-06 | Previous §2/§6 soft-reference list | Audit, outbox, task lease, Team Room and external storage | Prior list omitted `task_lease_events.participant_id` and did not fully spell out single-column principal FKs that fail to prove same-tenant ownership; external storage_ref was not connected to a concrete preserve rule. | Treat every listed scalar/array/external reference as OWNERSHIP_UNPROVEN; require application-level consumer audit. |
| A-07 | Previous §7 fixture manifest | All manifest resources | A list of IDs and expected tuples is insufficient if the manifest can be caller-authored or altered after creation. No persisted fixture_id exists in TARGET. | Generate manifest at fixture creation, canonicalize, hash/sign, bind to TARGET/harness/run, reject tampering or partial manifests. |
| A-08 | Previous §8 lock proposal | All Phone writers and soft-reference writers | Locking only currently known parent rows cannot prevent a new soft reference from being inserted by a writer that does not acquire the same lock. Catalog has no FK to serialize those writes. | Require a shared advisory-lock protocol across every writer or preserve/abort where that protocol cannot be proven. |
| A-09 | Previous §9 test evidence | Ten-cycle A/B cleanup | JSONL shows zero Phone descendants in the 10-cycle snapshots; cleanup_B_complete reports tenant and identity mappings deleted. This tests tenant deletion, not safe Phone-only cleanup. | Replace with two fully populated fixtures and all-table snapshots; tenant must remain after either cleanup. |
| A-10 | Previous §3 operator-key row | `phone_endpoint_operator_keys` | Table has only AI/tenant binding FK and no Phone/endpoint FK; migration says trusted control plane provisions keys out-of-band. Treating the key as Phone-owned risks revoking another endpoint/operator. | Classify TENANT_SHARED; exact revoke only through its control-plane lifecycle, not Phone cleanup. |
| A-11 | Previous §3 security/replay rows | `security_audit_events`, `auth_token_replays` | Both tables have no inbound/outbound FK. Tenant/Phone deletion will not cascade to them; a tenant_id field is only a soft reference. | Preserve; never sweep by tenant or Phone ID. |
| A-12 | Previous §4 cascade summary | All tenant descendants | Cascade consequences were summarized but not exhaustively enumerated by the catalog's 33 CASCADE constraints. | Use the complete grouped FK matrix in §6. |
| A-13 | Previous §9 approval | Cleanup readiness | Previous wording described future deletion of exact rows without a policy for retained children, idempotency receipt, soft-reference writers, or the no-delete anchor case. | Adopt the algorithm and fail-closed states in §§8–10; remain blocked until tests pass. |

### 7.1 Confirmed versus unproven

Confirmed by catalog/artifact:
- table, PK, FK, CHECK, UNIQUE, FK delete actions, trigger definitions;
- no fixture_id column;
- no FK on security_audit_events and auth_token_replays;
- operator key is AI/tenant-scoped, not Phone/endpoint-scoped;
- Phone messages and credentials are RESTRICT children;
- five SET NULL constraints and 33 CASCADE constraints;
- five scenario outcomes and the incomplete ten-cycle snapshots.

Not proven by catalog alone:
- which external process created any pre-existing row;
- whether external storage objects are shared;
- whether a soft-reference ID is consumed by a worker or external integration;
- whether every writer participates in an advisory lock;
- whether a fixture manifest is authentic unless signed and tied to the creation transaction;
- whether an audit/outbox retention policy permits physical deletion.

Unproven items are not assumptions. They remain `OWNERSHIP_UNPROVEN` and cannot be deleted automatically.

## 8. Safe cleanup algorithm — specification only (Round 5)

### 8.1 Operation semantics

The operation must distinguish two outcomes:

1. **DEPROVISION / RETIRE (default):** revoke operational access, fence active attachments, mark endpoints and Phone disconnected/revoked, and preserve all retained rows and parent anchors.
2. **PHYSICAL FIXTURE DELETE (exception):** delete only exact fixture-created rows after signed provenance, complete FK and soft-reference closure, and explicit retention authorization. If any retained child remains, physical deletion is not allowed.

The current schema supports the first outcome safely in principle. It does not support guaranteed physical deletion while retaining RESTRICT children. No automatic cascade is permitted.

### 8.2 Manifest contract

The manifest must be created by fixture creation code, not supplied by the cleanup caller. It must contain:
- immutable fixture UUID and operation UUID;
- TARGET SHA, harness commit, workflow/run ID, PostgreSQL version and creation timestamp;
- exact tenant_id, ai_id, ai_phone_id and principal_id as *scope anchors*, not delete permissions;
- exact IDs for every created endpoint, operator key, credential, attachment, message, mission link, room, participant, session, workload identity/execution, membership, task, lease event, handoff, channel, Team Room message, decision, attachment ref and WS ticket;
- expected complete parent tuple for each row;
- per-row creator, class, retention flag and create result;
- canonical row digest or equivalent creation evidence;
- signed manifest digest and immutable evidence location;
- pre-cleanup snapshot references for every relevant table, including global, retained and soft-reference tables.

A partial manifest is not “best effort”. Missing IDs, unknown fields, duplicate IDs, invalid parent tuples, signature mismatch, TARGET mismatch or manifest digest mismatch cause immediate failure before any write.

### 8.3 Ordered algorithm

1. Receive only a manifest reference and operation UUID.
2. Verify signature, canonical digest, TARGET/harness/run provenance, schema version and allowed table/key names.
3. Begin a SERIALIZABLE transaction with bounded lock and statement timeouts.
4. Acquire the per-Phone transaction advisory lock. Every writer that can create or mutate Phone dependencies or soft references must acquire the same lock; if that writer protocol is not universal, abort.
5. Lock scope anchors and exact rows in a deterministic order: tenant; global AI row; AI/tenant mapping; AI/principal mapping; Phone; principal/participant; room/session/workload; endpoints; credentials; attachments; messages; mission link; task/handoff/channel references.
6. Verify tenant exists and matches the manifest, but mark it permanently non-deletable.
7. Verify global AI identity and AI/tenant mapping exactly; never delete global identity or shared mappings.
8. Verify the Phone tuple `(ai_phone_id, tenant_id, ai_id)`, all endpoint tuples, principal kind/enabled state, AI/principal tuple, participant/principal/tenant tuple, session/room/principal tuple, execution/session/participant tuple and every other parent tuple.
9. For every manifest ID, query by exact primary key and compare the complete row identity and parent tuple. Reject unknown, missing, duplicate, cross-fixture or unlisted IDs.
10. Enumerate all inbound FK references from `pg_constraint` and inspect every known soft-reference consumer, JSON/UUID array and external storage reference. Compare dependent row sets to the manifest.
11. Classify each candidate. Only exact `FIXTURE_EXCLUSIVE` or eligible `PHONE_OWNED` rows may enter a physical delete set. `TENANT_SHARED`, `GLOBAL`, `RETAINED` and `OWNERSHIP_UNPROVEN` rows are excluded.
12. If any retained/unproven child blocks deletion of a parent, preserve the parent as an anchor and switch to DEPROVISION/RETIRE. Do not delete the parent to force FK success.
13. Build and persist an explicit delete/retire plan before the first mutation. The plan lists exact table, PK, expected count, action and dependency proof.
14. Revalidate row counts and ownership tuples immediately before each permitted mutation.
15. Perform exact-ID lifecycle updates/deletes in FK-safe order; never delete by tenant, AI, prefix, label, wildcard, guessed association or unbounded discovery query.
16. Verify postconditions and all protected snapshots before commit.
17. Persist a unique cleanup receipt atomically with the database transaction (or use an equivalent transactional harness journal). Commit only after receipt and postconditions succeed.
18. On any mismatch, unexpected dependent, lock failure, row-count mismatch, soft-reference uncertainty, serialization failure beyond retry policy or timeout: rollback and return a structured failure. Scope must never be broadened.

### 8.4 Pseudocode

```text
cleanup_phone(manifest_ref, operation_id):
    manifest = load_manifest(manifest_ref)
    verify_signature_and_digest(manifest)
    verify_target_harness_schema_and_run(manifest)
    validate_manifest_shape_and_exact_ids(manifest)

    begin transaction SERIALIZABLE
    set bounded lock_timeout and statement_timeout

    acquire advisory_xact_lock("ai-phone-cleanup", manifest.ai_phone_id)

    lock_scope_and_manifest_rows_in_global_order()
    assert tenant_exists_and_matches_manifest()
    assert global_ai_and_ai_tenant_mapping_match()
    assert phone_tuple_matches(ai_phone_id, tenant_id, ai_id)
    assert principal_and_ai_mapping_match()
    assert every_manifest_parent_tuple_matches()
    assert no_unknown_or_cross_fixture_ids()
    assert complete_fk_and_soft_reference_closure()
    assert all writers participate in the lock protocol

    protected = GLOBAL + TENANT_SHARED + RETAINED + OWNERSHIP_UNPROVEN
    delete_set = exact manifest rows classified FIXTURE_EXCLUSIVE or eligible PHONE_OWNED
    assert delete_set excludes tenant, global AI, mappings, audit, security, outbox and shared Team Room/task rows

    if retained_or_unproven_child_blocks_parent(delete_set):
        plan = deprovision_plan_only()
    else:
        plan = exact_physical_delete_plan(delete_set)

    persist_plan_and_expected_row_counts()
    execute only approved exact-ID actions in FK order
    assert row_counts_and_postconditions_match()
    assert protected_snapshots_unchanged()
    write_unique_cleanup_receipt(operation_id, manifest_digest, result)
    commit
    return result

on any exception:
    rollback
    emit JSONL failure with zero committed deletes
    return FAIL_CLOSED
```

### 8.5 SQL transaction skeleton (illustrative, not executable production SQL)

```sql
BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE;
SET LOCAL lock_timeout = '2s';
SET LOCAL statement_timeout = '15s';

-- All participating writers must use this exact lock namespace/key scheme.
SELECT pg_advisory_xact_lock(
  hashtextextended('ai-phone-cleanup:' || :ai_phone_id::text, 0)
);

-- Tenant is a scope anchor only. It is never placed in the delete set.
SELECT tenant_id
  FROM tenants
 WHERE tenant_id = :tenant_id
 FOR UPDATE;

SELECT ai_id, tenant_id
  FROM ai_identity_tenants
 WHERE ai_id = :ai_id AND tenant_id = :tenant_id
 FOR UPDATE;

SELECT ai_id, tenant_id, principal_id
  FROM ai_identity_principals
 WHERE ai_id = :ai_id
   AND tenant_id = :tenant_id
   AND principal_id = :principal_id
 FOR UPDATE;

SELECT ai_phone_id, tenant_id, ai_id, phone_status, endpoint_fence
  FROM ai_phones
 WHERE ai_phone_id = :ai_phone_id
   AND tenant_id = :tenant_id
   AND ai_id = :ai_id
 FOR UPDATE;

-- Lock each exact manifest set in deterministic PK order.
SELECT endpoint_id
  FROM phone_endpoints
 WHERE endpoint_id = ANY(:endpoint_ids)
 ORDER BY endpoint_id
 FOR UPDATE;

SELECT credential_id
  FROM phone_endpoint_credentials
 WHERE credential_id = ANY(:credential_ids)
 ORDER BY credential_id
 FOR UPDATE;

SELECT attachment_id
  FROM phone_attachments
 WHERE attachment_id = ANY(:attachment_ids)
 ORDER BY attachment_id
 FOR UPDATE;

SELECT message_id
  FROM phone_messages
 WHERE message_id = ANY(:message_ids)
 ORDER BY message_id
 FOR UPDATE;

-- Validate exact counts, complete tuples, FK dependents, soft refs and
-- protected snapshots here. Any mismatch raises and rolls back.

-- Default: deprovision, do not physically delete retained history.
UPDATE phone_attachments
   SET attachment_status = 'FENCED',
       detached_at = COALESCE(detached_at, NOW())
 WHERE attachment_id = ANY(:approved_attachment_ids)
   AND ai_phone_id = :ai_phone_id
   AND tenant_id = :tenant_id
   AND ai_id = :ai_id
   AND attachment_status = 'ACTIVE';

UPDATE phone_endpoints
   SET endpoint_status = 'REVOKED',
       detached_at = COALESCE(detached_at, NOW())
 WHERE endpoint_id = ANY(:approved_endpoint_ids)
   AND ai_phone_id = :ai_phone_id
   AND tenant_id = :tenant_id
   AND ai_id = :ai_id
   AND endpoint_status <> 'REVOKED';
-- Existing AFTER UPDATE trigger revokes ACTIVE credentials.

UPDATE ai_phones
   SET endpoint_fence = endpoint_fence + 1,
       phone_status = 'DISCONNECTED',
       version = version + 1,
       updated_at = NOW()
 WHERE ai_phone_id = :ai_phone_id
   AND tenant_id = :tenant_id
   AND ai_id = :ai_id
   AND endpoint_fence = :expected_fence;

-- Do not delete tenants, global AI, mappings, audit/security/outbox,
-- operator keys, tasks, lease events, handoffs or retained messages.
-- Do not delete ai_phones while retained RESTRICT children remain.

-- Write unique operation receipt and verify all protected snapshots.
COMMIT;
```

The SQL is a design skeleton, not an executable script. The final SQL must be derived from the exact manifest schema and tested in disposable PostgreSQL before any implementation authorization.

### 8.6 Physical delete order, only if explicitly approved

Physical deletion is not the default. It is allowed only if every row is manifest-proven, every retention policy explicitly permits deletion, and the exact FK/soft-reference closure is empty outside the delete set.

Candidate child-first order:
1. Exact Phone attachment rows, only if no external or retention dependency exists.
2. Exact endpoint credential rows, only if security retention explicitly permits deletion.
3. Exact endpoint rows.
4. Exact Phone message rows, only if message retention explicitly permits deletion.
5. Exact Phone mission-link row; the referenced Team Task is never deleted.
6. Exact `ai_phones` row, only after all RESTRICT children are gone.
7. No deletion of `ai_identity_tenants`, `ai_identity_principals`, global AI identity, principal or tenant as part of Phone cleanup.

If credentials/messages/attachments are retained, stop before deleting their parents and perform deprovision only. There is no “force delete” fallback.

## 9. Concurrency, locking and failure semantics (Round 6)

### 9.1 Required protections

- **Advisory lock:** one transaction-scoped lock per `ai_phone_id`; every Phone, endpoint, credential, attachment, message, mission-link and soft-reference writer must acquire it before mutation.
- **Row locks:** lock tenant scope, AI/tenant mapping, AI/principal mapping, Phone, endpoint and all exact parent/dependent rows in a deterministic table/PK order.
- **FK protection:** `FOR UPDATE` on referenced parent rows conflicts with FK insert key-share locks and prevents new FK children from racing past the validation window.
- **SERIALIZABLE:** use serializable isolation for predicate/read-set validation, but do not treat it as a substitute for the shared advisory lock on soft-reference writers.
- **Soft references:** because there is no FK lock for JSON/array/external references, require the same advisory lock in every writer or preserve the resource and abort.
- **Ownership changes:** tenant remapping, Phone transfer, endpoint replacement/revocation, principal disablement/mapping changes and mission/task changes must take the same lock and increment/validate fencing/version state.
- **Stable lock order:** all code paths must use the same global order; never lock arbitrary child rows before the Phone anchor.

The reviewed TARGET contains row-locking patterns in Phone lifecycle operations, but the audit has not proven a universal advisory-lock protocol across every writer, especially out-of-band operator-key provisioning and soft-reference writers. This is a blocker, not a presumed capability.

### 9.2 Timeouts, deadlocks and retries

| Condition | Required behavior |
|---|---|
| Ownership mismatch, unknown ID, missing manifest row, extra dependent, bad signature | Roll back immediately; no retry; no delete |
| `40001` serialization failure | Roll back; bounded retry of the identical operation/manifest only; never widen scope |
| `40P01` deadlock | Roll back; at most a small bounded retry after verifying global lock order; repeated deadlock is FAIL_CLOSED |
| Lock timeout / lock unavailable | Roll back and return RETRYABLE_BLOCKED; do not skip the lock |
| Statement timeout | Roll back; return RETRYABLE_BLOCKED; no partial cleanup |
| Row-count mismatch or unexpected FK behavior | Roll back; emit exact evidence; no retry without investigation |
| Lost response after commit | Resolve through unique operation receipt; do not blindly repeat deletes |
| Missing idempotency receipt and missing manifest rows | Treat as inconsistent/unknown; abort, do not infer that cleanup already succeeded |

### 9.3 Idempotency

Use a unique `operation_id` plus manifest digest and result receipt written atomically with the database transaction. A retry with the same operation ID and same digest returns the stored result. Same operation ID with a different digest is rejected. The current schema/catalog has no dedicated cleanup receipt table; this is a design dependency requiring an approved harness-only journal or later schema design. It must not be improvised during implementation.

## 10. Atomicity and isolation test plan (Round 7)

No tests in this section are authorized to run as operational cleanup yet. Tests must use a disposable PostgreSQL 16 database and two complete, independently populated fixtures.

### 10.1 Two-fixture construction

Fixture A and fixture B must use:
- distinct tenant IDs;
- the same global `ai_id` to prove cross-tenant global identity sharing;
- distinct tenant-scoped principals, participants, rooms, sessions and workload identities/executions;
- distinct Phone IDs, endpoints, operator keys, credentials, attachments and messages;
- distinct Team Tasks, lease events and handoffs;
- Team Room channels, memberships, messages, decisions, attachment refs and WS tickets;
- audit_events, security_audit_events and outbox_events with references to both fixtures;
- soft-reference records and external-storage test objects with known ownership labels.

The unique `(tenant_id, ai_id)` Phone key means two Phones sharing one AI must be in different tenants. Do not try to create two Phones for the same AI in one tenant.

### 10.2 Required tests

| Test | Action | Required invariant |
|---|---|---|
| Cleanup A without B impact | Deprovision/cleanup A | Every B row and snapshot hash remains identical; tenant A remains |
| Cleanup B without A impact | Deprovision/cleanup B | Every A row and snapshot hash remains identical; tenant B remains |
| Shared global AI | Clean A while B uses same AI | Global AI row remains; B binding and Phone remain |
| Identity mappings | Clean either fixture | Other tenant mapping remains; no mapping is removed by Phone cleanup |
| Audit history | Clean A and B | audit_events and security_audit_events rows and payloads remain |
| Outbox | Clean A and B | outbox rows, event payloads and published_at remain |
| Sessions/workloads | Clean A | B sessions, workload identities/executions and memberships remain unchanged |
| Team Room | Clean A | B channels, members, messages, decisions, attachment refs and tickets remain unchanged |
| Handoffs/tasks | Clean A | B tasks, lease events and handoffs remain; no SET NULL mutation of B history |
| Soft references | Clean A | B soft refs and external storage objects remain; unknown consumers cause abort |
| Induced rollback | Inject failure after each planned mutation stage | Every table returns byte-for-byte to pre-transaction snapshot; receipt absent |
| Idempotent retry | Repeat same operation ID after committed result | Stored receipt returned; no additional mutations |
| Duplicate operation mismatch | Reuse operation ID with different manifest digest | Rejected before mutation |
| Partial fixture | Omit one created child from manifest | Zero committed writes; explicit INCOMPLETE_MANIFEST |
| Unexpected dependent | Add unlisted RESTRICT or soft-reference child | Zero committed deletes; explicit UNLISTED_DEPENDENT |
| Concurrent transfer | Race cleanup with endpoint/Phone transfer under same advisory lock | One operation serializes; no mixed ownership or cross-fixture delete |
| Concurrent dependency insert | Race cleanup with new FK and soft-reference insert | FK insert blocks or serialization/advisory lock aborts; no unlisted row disappears |

### 10.3 Complete snapshots and invariants

Before and after every test, snapshot all 35 tables, not just Phone tables. For each row capture:
- table name and primary key;
- canonical JSON of every column (including JSONB, arrays, status and timestamps);
- row count and sorted PK set;
- SHA-256 of canonical ordered row serialization;
- external object key, metadata and content digest where applicable.

Protected invariants:
1. Tenant A and B always exist after Phone cleanup.
2. Global AI identity exists before and after.
3. Both AI/tenant mappings and AI/principal mappings remain unless a separate approved mapping-lifecycle test explicitly targets them.
4. Audit, security audit and outbox row sets and payload hashes do not change.
5. All rows belonging to the other fixture remain byte-for-byte unchanged.
6. Retained message/credential history remains and its required parent anchors remain.
7. Team Tasks, lease events, handoffs and Team Room governance history remain.
8. No unmanifested row is deleted, updated, detached, nulled or re-parented.
9. Rollback restores every table and produces no success receipt.
10. A repeated committed operation returns the same receipt without changing any row.

## 11. Adversarial attack matrix (Round 8)

All attacks must be executed only in isolated test databases after the specification is approved. Expected outcome for every attack is fail-closed with zero committed deletion unless the exact resource is proven fixture-exclusive and explicitly eligible.

| Attack | Expected result | State that must remain intact | Required JSONL evidence | Objective PASS condition |
|---|---|---|---|---|
| Tenant A ID with another fixture's tenant binding | Reject TENANT_SCOPE_MISMATCH | Both tenants, mappings, Phones, audit/outbox | manifest IDs, queried tuples, error code, before/after hashes | Zero writes; both fixture snapshots identical |
| AI ID from fixture B with A tenant | Reject AI_TENANT_BINDING_MISMATCH | Global AI and both tenant bindings | ai_id, tenant_id, mapping PKs, FK proof | Zero writes; no mapping removed |
| Phone ID from fixture B | Reject PHONE_FIXTURE_MISMATCH | A and B Phones/endpoints/messages | requested vs manifest Phone ID and tuple | Zero writes; both Phone graphs intact |
| Incompatible principal | Reject PRINCIPAL_MAPPING_MISMATCH | Principal, mapping, sessions, Phone, audit | principal kind/enabled and composite tuple | Zero writes; no tenant cascade |
| Nonexistent operator key | Reject UNKNOWN_OPERATOR_KEY; never infer fallback scope | Tenant, AI binding, Phone and key table | key ID, lookup count, before/after | Zero writes; no valid tenant deleted |
| Endpoint from another Phone | Reject ENDPOINT_PHONE_TUPLE_MISMATCH | Both endpoints, credentials and attachments | endpoint/phone/tenant/AI tuple | Zero writes; all endpoint snapshots unchanged |
| IDs mixed across fixtures B and C | Reject MIXED_FIXTURE_MANIFEST | A, B and C entire snapshots | fixture UUID per ID and mismatch list | Zero writes; all fixtures unchanged |
| Global identity presented as cleanup ID | Reject GLOBAL_RESOURCE_IN_DELETE_SET | Global AI and all tenant bindings | classification and delete-set trace | AI row and all mappings unchanged |
| Shared operator key | Reject TENANT_SHARED_RESOURCE_DELETE | All Phones using same AI/tenant key scope | key scope and consumer list | Key remains; only separately authorized revoke path may act |
| Soft reference without FK | Reject SOFT_REFERENCE_UNPROVEN | Audit/outbox/task lease/Team Room/external object | reference source, target ID, consumer scan | No reference or target is removed |
| Transitive cascade path | Reject CASCADE_OUTSIDE_DELETE_SET | Tenant, room, principal, audit, outbox, tasks, handoffs, channels | FK closure and planned cascade set | No parent delete; protected snapshots identical |
| Concurrent ownership transfer | Serialize or abort | Both old/new owner graphs and credentials | lock wait, fence/version, transaction outcome | No mixed tuple; one winner; loser rolls back |
| Tampered manifest | Reject MANIFEST_SIGNATURE_OR_DIGEST_INVALID | All 35 tables | submitted digest, expected digest, signature result | Zero writes |
| Incomplete manifest | Reject INCOMPLETE_MANIFEST | All 35 tables and external storage | expected vs discovered exact ID sets | Zero writes; no auto-discovery deletion |
| Unknown extra dependent | Reject UNLISTED_DEPENDENT | Parent and new dependent | FK/soft-ref row identity | Zero writes; parent remains |
| Retained message/credential child | Return DEPROVISION_ONLY | Message, credential, endpoint and Phone anchors | retained child PKs and FK definitions | No parent delete; endpoint/credential revoked as policy permits |

### 11.1 JSONL event contract

Every attempt must emit structured evidence with:
- `run_id`, `target_sha`, `harness_sha`, `postgres_version`;
- `fixture_id`, `operation_id`, `manifest_digest`;
- `scenario`, `attack`, `expected_result`, `actual_result`;
- exact resource table/PK and expected/observed ownership tuple;
- ownership class and proof reference;
- lock acquisition result, transaction isolation, SQLSTATE and retry count;
- pre/post row counts and snapshot hashes for all 35 tables;
- protected row IDs changed (must be empty);
- `committed_delete_count`, `committed_update_count`, `rollback_confirmed`;
- final `PASS`, `FAIL` or `BLOCKED`.

A PASS requires both the expected error/result and unchanged protected snapshots. A helper returning an error is not enough if any prior write committed.

## 12. Unresolved risks and hard blockers

1. The current cleanup helper still contains broad tenant deletion in the audit/test cleanup path. It remains rejected and must not be invoked operationally.
2. No fixture_id exists in the applied schema; manifest authenticity and creation provenance are not database-enforced.
3. No universal advisory-lock protocol has been demonstrated for all Phone, operator-key, Team Room and soft-reference writers.
4. External storage ownership and deletion semantics are not established.
5. Retention duration and deletion authorization for Phone messages, endpoint credentials, attachments, audit, outbox, task lease events, handoffs and Team Room history are not formally defined.
6. Retained RESTRICT children make physical deletion of Phone/endpoint anchors impossible until retention is satisfied or an approved archive/decoupling design exists.
7. The existing 10-cycle test is incomplete and treats tenant deletion as successful cleanup.
8. Rollback injection, idempotency receipts, complete dependency closure, soft-reference races and concurrent ownership transfer have not been tested.
9. The five-scenario reproduction remains FAIL (3 unsafe deletions; 2 FK-blocked).
10. No independent implementation review or authorization has occurred.

When ownership cannot be proven, preserve the row and its parent anchors, return an explicit failure/deprovision-only result, and stop. Never widen the delete set to make cleanup succeed.

## 13. Final review and authorization request (Rounds 9–10)

### Evidence verified
- TARGET exact and unchanged in both workflows.
- Workflow 1 and 2 logs independently checked.
- Both original artifact ZIPs downloaded; SHA-256 matched GitHub digests.
- Five scenario JSONL outcomes independently inspected.
- 10-cycle JSONL inspected; incomplete scope and tenant deletion observed.
- PostgreSQL 16.16 catalog JSONL inspected directly.
- 35 public tables, 85 FKs, 33 CASCADE, 29 RESTRICT, 18 NO ACTION, five SET NULL and seven triggers recounted.
- Commit comparisons verified harness/workflow/document provenance.

### Current classification

| Component | Status |
|---|---|
| TARGET immutability | PASS |
| Workflow/artifact provenance | PASS, with harness/workflow commit distinction documented |
| Five-scenario reproduction | FAIL |
| PostgreSQL catalog capture and recount | PASS |
| Prior contract completeness | FAIL; corrected in this specification |
| Complete ownership proof for every resource | BLOCKED |
| Physical-delete semantics with retained Phone children | BLOCKED |
| Safe deprovision algorithm design | SPECIFIED, NOT IMPLEMENTED |
| Concurrency guarantee across every writer | BLOCKED |
| Complete two-fixture isolation/atomicity tests | PENDING |
| Adversarial matrix | SPECIFIED, NOT EXECUTED |
| Production correction | NOT AUTHORIZED |
| Integration and merge | BLOCKED |

### Authorization requested

**GPT-6 approval is requested for the specification only, not for implementation.**

Before implementation can be authorized, the reviewer must explicitly accept:
1. Phone cleanup defaults to deprovision/retire, not physical deletion, while retained RESTRICT children exist.
2. Tenant, global AI, identity/tenant mappings, audit/security/outbox, Team Tasks, lease events, handoffs and Team Room history are outside Phone cleanup scope.
3. All `OWNERSHIP_UNPROVEN` resources are preserved and cause fail-closed behavior if they block a parent deletion.
4. Every writer must participate in the same advisory-lock protocol or the relevant resource remains non-deletable.
5. A complete two-fixture test suite with full 35-table snapshots must pass before any production implementation.
6. Any later physical-delete policy must define retention/archive semantics for messages, credentials, attachments and their parent anchors.

Until that approval is explicit, do not implement the algorithm, modify production code or migrations, execute cleanup operationally, run the 160 races or 20 handoffs, run integration, or merge.

**Final status: BLOCKED.** The current evidence demonstrates the vulnerability and the specification now identifies the principal schema/contract gaps, but ownership closure, retention semantics, writer-wide concurrency guarantees and complete isolation tests remain unproven.


---

# AI-PHONE-02C Lifecycle, Retention and Authorization Addendum

Policy revision based on immutable TARGET ebb8eeecfea087443c5f3eba7021e96cb5302fe7.
Schema evidence: PostgreSQL 16.16 catalog artifact from workflow 43473339618.
Reviewed source files are read at the immutable TARGET, not at the moving default branch.
Status: **POLICY SPECIFIED — IMPLEMENTATION NOT AUTHORIZED — ACCEPTANCE TESTS NOT RUN**.

## 14. Evidence basis and schema compatibility (Round 1)

### 14.1 Directly verified schema facts

The catalog artifact was parsed directly. It contains 35 public tables (34 application tables plus _sqlx_migrations), 85 foreign keys, 33 CASCADE actions, 29 RESTRICT actions, 18 NO ACTION actions, five SET NULL actions and seven non-internal triggers. The trigger list contains no DELETE trigger.

The status CHECK constraints establish the following persisted values:

| Resource | Persisted status values in TARGET | Important consequence |
|---|---|---|
| ai_phones.phone_status | ATTACHED, TRANSFER_REQUESTED, TRANSFERRING, ATTACHED_NEW_ENDPOINT, DISCONNECTED | There is no ACTIVE, DETACHING, DETACHED, DEPROVISIONING, RETIRED or REVOKED Phone status. |
| phone_endpoints.endpoint_status | REGISTERED, ATTACHED, DETACHED, REVOKED | REVOKED is terminal in the current endpoint registration handler; DETACHED can be registered again. |
| phone_attachments.attachment_status | ACTIVE, DETACHED, FENCED | Attachment history can remain after endpoint detach/revoke. |
| phone_endpoint_credentials.credential_status | ACTIVE, REVOKED | Expiration is separately checked by expires_at; an expired row can still say ACTIVE. |
| phone_endpoint_operator_keys.key_status | ACTIVE, REVOKED | expires_at is nullable; scope is tenant + AI, not endpoint or Phone. |
| sessions.status | CREATED, ACTIVE, EXPIRED, REVOKED, CLOSED | Sessions are principal/room context, not Phone-specific credentials. |
| workload_executions.status | ACTIVE, EXPIRED, REVOKED, CLOSED | Execution lifecycle is separate from Phone lifecycle. |
| workload_identities.status | ACTIVE, DISABLED, REVOKED, EXPIRED | Workload identity is principal/tenant scope, not Phone scope. |

The catalog also confirms:
- ai_phones has a unique (tenant_id, ai_id) constraint and a RESTRICT binding to ai_identity_tenants.
- phone_endpoints has RESTRICT parent relations to ai_phones.
- phone_endpoint_credentials has RESTRICT parent relations to both phone_endpoints and ai_phones.
- phone_attachments has RESTRICT relations to ai_phones, phone_endpoints, ai_identity_principals, principals, participants, sessions and workload_executions.
- phone_messages has RESTRICT relations to ai_phones, recipient AI identity and sender principal.
- phone_mission_links has RESTRICT relations to ai_phones and team_tasks.
- phone_endpoint_operator_keys has only the composite AI/tenant FK to ai_identity_tenants; it has no phone_id or endpoint_id.
- team_room_attachment_refs stores storage_ref as text; the catalog does not identify an object-storage provider, object ownership protocol or deletion API.

### 14.2 Directly verified source behavior

Source provenance at TARGET:
- services/ai-team-room/src/phone.rs blob ea0059441656e38a5f638d0939fbf9cfa3a85a86
- services/ai-team-room/src/auth.rs blob c1ccf50f5c9567ad20c786d861339a8d6f0cef10
- services/ai-team-room/src/handoffs.rs blob f6736c77ef66d8c85c1c1ddb261d96649638761e
- services/ai-team-room/src/communication.rs blob 4a89ffd34fe78a8b68fd7ed2c83ed43f18b6eb65
- services/ai-team-room/src/tasks.rs blob b6bd7f33d7a1bcad943060c09b38ec9dcf612819
- services/ai-team-room/migrations/0014_ai_phone_endpoint_credentials.sql blob 7f43f13b7f8b1448d58e3d54dc9ec2ed97f32467

Confirmed behavior:
1. Endpoint status transition to DETACHED or REVOKED triggers revocation of ACTIVE credentials for that endpoint.
2. The endpoint revoke handler fences an active attachment, advances the Phone endpoint fence, marks the endpoint REVOKED and records a Phone event. If already REVOKED, it still revokes any anomalous remaining ACTIVE credentials.
3. Endpoint registration/rotation revokes existing ACTIVE credentials for that endpoint and inserts a new hash-only credential. New endpoint credentials have a 90-day expiry.
4. The endpoint operator authorization query requires key_status ACTIVE and expires_at NULL or later than NOW(). The key is looked up by tenant_id, ai_id and key_hash only.
5. The endpoint operator key migration explicitly says raw keys are provisioned out-of-band by a trusted control plane and are never persisted.
6. Phone detach fences/detaches the active attachment, marks endpoint DETACHED and Phone DISCONNECTED, and advances the fencing generation. The endpoint lifecycle trigger revokes that endpoint's active credentials.
7. Session authentication checks ACTIVE, revoked_at and expires_at. Expired session/workload rows are updated when the token is presented. close_session closes one authenticated session and its execution, not a Phone-specific session.
8. Phone message content and identity fields are immutable under the phone_message_transition_guard trigger.
9. communication.rs stores team_room_attachment_refs.storage_ref but does not establish that the referenced object is owned by a Phone or that the service can safely delete it.
10. Phone and endpoint paths use row locks in some lifecycle operations. This source review did not find a universal Phone advisory-lock protocol across auth, Team Room, task, handoff, outbox, attachment-ref and out-of-band control-plane writers.

### 14.3 Corrections and limits

The previous contract's statement that a safe deprovisioning flow is possible “in principle” must be read as a design possibility, not as a proven property of TARGET. TARGET does not have a persistent retired state or a universal write barrier. Consequently, current code cannot guarantee that a Phone remains retired after the existing endpoint detach/revoke operations.

No schema or source changes are made by this addendum. No external storage contents were inspected. The existence, provider, lifecycle and sharing of objects referenced by storage_ref remain unproven.

## 15. Formal Phone lifecycle policy (Round 2)

### 15.1 State model: current values versus proposed lifecycle

The lifecycle names requested by this mission are not all compatible with the existing phone_status CHECK constraint. The following distinction is mandatory:

| Lifecycle concept | Current TARGET representation | Policy |
|---|---|---|
| ACTIVE | No literal Phone status. Operational availability is derived from Phone, endpoint, attachment, credential, session and execution state. | Treat ACTIVE as a conceptual lifecycle state only. Do not write ACTIVE to phone_status. |
| DETACHING | No persisted Phone state. | Future lifecycle state; must be introduced through a migration or durable lifecycle record before it is used. |
| DETACHED | Phone can be DISCONNECTED; endpoint and attachment rows can be DETACHED. | Endpoint-level detach is reversible and is not Phone retirement. |
| DEPROVISIONING | No persisted Phone state. | Future lifecycle state and write-admission barrier. |
| RETIRED | No persisted Phone state. | Future terminal Phone lifecycle state; cannot be safely represented by DISCONNECTED alone. |
| REVOKED | Exists for endpoint, endpoint credential, operator key, session and workload execution as applicable; not for ai_phones. | Resource-level revocation only. Do not invent a Phone-level REVOKED status in the existing column. |

Proposed future schema: add a separate durable Phone lifecycle field or lifecycle table with an explicit CHECK/enum and monotonically increasing lifecycle_version. Keep phone_status for the existing connection/transfer state machine. A separate lifecycle event/receipt stream should retain who requested, who approved, operation ID, timestamps, reason, prior/new state and manifest digest. This is a design proposal, not an existing capability.

### 15.2 Allowed transitions

| From → To | Meaning | Authorization and preconditions | Required effects |
|---|---|---|---|
| ACTIVE → DETACHING → DETACHED | Disconnect current endpoint/session attachment but retain Phone identity for later reconnection. | Authenticated Phone owner/session under existing detach path; future admin override must have a dedicated permission. | Fence active attachment, advance endpoint fence, detach endpoint; endpoint trigger revokes its active credentials; preserve Phone, endpoint, attachment, messages and history. |
| DETACHED → ACTIVE | Reconnect the same Phone using an eligible non-revoked endpoint or a newly registered endpoint. | Fresh authenticated session and separately authorized endpoint operator key for registration/rotation; all current binding checks must pass. | New attachment/fence and fresh endpoint credential; never reactivate a REVOKED endpoint or old credential. |
| ACTIVE or DETACHED → DEPROVISIONING | Begin permanent retirement and stop all new Phone operations. | Tenant administrator requests; a distinct privileged lifecycle authority approves; future dedicated permission and durable operation record required. Existing phone.endpoint.manage is not sufficient evidence of lifecycle authority. | Atomically close admission first; fence attachments; revoke all endpoint credentials by endpoint lifecycle transition; preserve parent/history rows. |
| DEPROVISIONING → RETIRED | Complete retirement after postconditions. | All endpoints and active attachments are non-operational; no in-flight transfer; writer barrier is active; unresolved ownership does not permit further destructive action. | Persist terminal lifecycle marker and audit receipt; retain Phone and all RESTRICT anchors. |
| DEPROVISIONING → DEPROVISIONING | Retry after timeout, process crash or lost response. | Same operation ID and same request/manifest digest. | Idempotently resume/return recorded outcome; no duplicate event or widened scope. |
| RETIRED → any active state | Not allowed. | No self-service reactivation. | Create a new Phone identity or require a separately reviewed restore design; do not clear the retired marker. |

Endpoint REVOKED is terminal under the current register handler, which returns conflict for an already revoked endpoint. Endpoint DETACHED is not terminal: the registration handler can set it back to REGISTERED and issue a new credential. Therefore a Phone-wide RETIRED state must block endpoint registration, attach, heartbeat, send, receive, ACK and all other Phone operations; revoking the current endpoint alone is insufficient.

### 15.3 Authority model

Current code proves:
- endpoint registration and endpoint revocation require phone.endpoint.manage plus a valid AI/tenant-scoped operator key; attach requires phone.attach (or phone.transfer for transfer), and detach requires phone.detach plus the current endpoint credential/fence;
- session-authenticated Phone operations require a valid tenant/room/principal/session/execution binding;
- the schema has no dedicated phone.lifecycle.manage permission or Phone lifecycle approver.

Proposed policy:
- Detach: authenticated Phone owner or authorized tenant operator; no destructive data deletion.
- Permanent retirement request: tenant administrator or explicitly delegated Phone owner.
- Permanent retirement approval: a distinct privileged lifecycle authority, separate from the requester for high-impact or regulated deployments.
- Operator-key issue/revoke: trusted control plane only, with its own audited authorization and key-rotation process.
- Session/workload revocation: principal/security lifecycle authority only, not automatic Phone cleanup.
- Physical fixture deletion: test harness authority only, only against a verified disposable database and signed fixture manifest; never an operational Phone API.

No new permission is presumed to exist. A future implementation must add and test explicit lifecycle authorization rather than reusing phone.endpoint.manage.

## 16. Retention and parent-anchor policy (Round 3)

### 16.1 Non-negotiable retention default

Until a formally approved retention schedule defines a lawful retention period, legal hold behavior, archive verification and purge authority, all historical rows below are PRESERVE. No age-based purge or cleanup-driven deletion is authorized by this mission.

A retained RESTRICT child requires its parent anchor to survive. Do not delete the parent, disable the FK, null immutable fields, or delete the child merely to make a Phone row deletable.

### 16.2 Phone graph retention matrix

| Resource / relation | Current FK or reference behavior | Lifecycle action | Retention and parent-anchor rule |
|---|---|---|---|
| ai_phones | Parent to endpoints, credentials, attachments, messages and mission links; RESTRICT children; AI/tenant mapping RESTRICT | Keep row; mark separate lifecycle marker RETIRED only after future write barrier | Preserve as durable identity anchor. No physical deletion while retained children exist. |
| phone_endpoints | RESTRICT child of Phone; parent of credentials and attachments | Set endpoint_status REVOKED for retirement; DETACHED only for reversible disconnect | Preserve endpoint row as audit/credential anchor. Never delete to bypass credential/attachment RESTRICT. |
| phone_endpoint_credentials | RESTRICT to endpoint and Phone; stores hash, status, fence, expiry and revoked_at | Revoke active credential; expiry denies use but is not a revocation event | Preserve row and hash-only audit metadata. Do not store or attempt to recover raw secret. |
| phone_attachments | RESTRICT to Phone, endpoint, principal, participant, room/session and workload execution | Set FENCED on security revocation/retirement; DETACHED for ordinary detach | Preserve attachment row. Its retained FKs also preserve the referenced session, participant, principal and execution anchors. |
| phone_messages | RESTRICT to Phone, recipient AI identity and sender principal; immutable content/identity trigger | Stop delivery/acceptance after retirement through lifecycle gate; preserve content and terminal state | Preserve message and all parent anchors. Never delete messages to unlock Phone deletion. |
| phone_mission_links | RESTRICT to Phone and team_tasks; task_id/room/tenant composite relation | Preserve link as historical association; resolve active task through normal task/handoff lifecycle before retirement | Preserve Phone and task anchors. Do not cancel/delete a shared task as a side effect. |
| AI identity and AI/tenant/principal mappings | RESTRICT and shared/global identity graph | No Phone-triggered deletion or revocation | Preserve global AI and identity mappings. Tenant/AI mappings are not Phone cleanup resources. |
| sessions and workload_executions | Shared principal/room/session/execution context; attachment RESTRICT; other resources also reference them | Do not revoke merely because one Phone retires. Revoke only through separate principal/session security policy. | Preserve rows required by retained attachments, handoffs and audit history. |
| audit_events and security_audit_events | Audit rows; some tenant cascade, some soft references/no FK | Preserve; append lifecycle events rather than erase prior events | Tenant must remain. Preserve security history even where it outlives a tenant or Phone. |
| outbox_events | Tenant FK CASCADE; aggregate_type/aggregate_id are soft references; INSERT NOTIFY trigger | Preserve pending, delivered and historical events; publish/ack through normal consumer lifecycle | Never clear outbox by deleting tenant. Keep tenant anchor. |
| team_tasks and task_lease_events | Task is room/tenant scoped; lease events cascade from task | Resolve active task ownership/handoff independently; no Phone cleanup deletion | Preserve task and lease/fencing history. Never delete task to remove a Phone mission link. |
| task_handoffs | Tenant/room/task CASCADE; session/execution fields SET NULL | Complete/cancel/expire through handoff state machine only when authorized | Preserve handoff record and context; do not delete tenant, room or task to remove it. |
| Team Room channels/messages/decisions/reads/members/WS tickets | Room/tenant/principal/session relationships; some cascades, some soft refs | Preserve; expire/consume tickets through their own protocol | These are collaboration records, not Phone-owned resources. |
| team_room_attachment_refs | Room/channel FK graph; message_id has a NO ACTION FK to team_room_messages; uploaded_by_principal_id has a single-column NO ACTION FK; task_id is a soft reference; storage_ref is opaque text | Preserve reference and object; no automatic object deletion | Retain room/channel and referenced message; preserve task_id as an unproven soft reference. Actual object ownership and storage provider are unproven. |
| auth_token_replays and permission_catalog | Global replay/security and global authorization vocabulary | No Phone cleanup action | Preserve; global expiry maintenance remains separate. |
| _sqlx_migrations | Deployment metadata | No Phone cleanup action | Preserve database migration history. |

### 16.3 RESTRICT anchor closure

At minimum, preserving Phone messages requires preserving the Phone, recipient AI identity and sender principal. Preserving endpoint credentials requires preserving both endpoint and Phone. Preserving Phone attachments requires preserving Phone, endpoint, AI/principal mapping, principal, participant, room/session and workload execution. Preserving mission links requires preserving Phone and task.

This closure is why physical Phone deletion is not a valid default. If retention policy later allows archiving, that must be a separately reviewed archive/decoupling design with verified restore, integrity, legal hold and reference migration. No such archive mechanism is proven in TARGET.

## 17. Operator keys and endpoint credentials (Round 4)

### 17.1 Operator key

The operator key row is scoped by tenant_id + ai_id, indexed by that scope and status, and checked by key_hash. It has no phone_id, endpoint_id, owner principal, issuer identity or exclusive Phone provenance. Migration 0014 states that keys are provisioned out-of-band by a trusted control plane.

Policy:
- A Phone retirement MUST NOT automatically revoke an operator key.
- Revocation must be initiated by the trusted control plane after identifying every operator, endpoint and enrollment workflow using that key.
- Rotation creates a new random secret, stores only its hash, distributes it through a secure one-time channel, verifies new use, then revokes the old key. Do not assume the raw key can be recovered from the database.
- Expiration denies authorization when expires_at is reached; the row may remain ACTIVE until explicitly marked REVOKED. Expiry and revocation must be reported separately.
- A key whose actual consumer set or control-plane owner is unknown is OWNERSHIP_UNPROVEN and must be preserved.
- Future explicit Phone binding would require a schema-enforced association and a migration proving exclusivity. Adding a phone_id column alone is insufficient if multiple operator principals or enrollment workflows share the key.

The current unique (tenant_id, ai_id) Phone constraint means at most one ai_phones row exists for that AI/tenant pair in this schema. It does not convert the operator key into an endpoint-specific credential or prove its exclusive control-plane ownership.

### 17.2 Endpoint credential

An endpoint credential is structurally bound to one endpoint, Phone, tenant and AI by composite RESTRICT FKs. It is eligible for Phone lifecycle revocation, but not physical deletion by default.

- Normal detach: endpoint becomes DETACHED; the lifecycle trigger revokes active credentials for that endpoint.
- Endpoint revoke: endpoint becomes REVOKED; the same trigger revokes active credentials.
- Rotation: old ACTIVE credentials for the endpoint are revoked and a new hash-only credential is issued.
- Expiration: runtime authorization rejects expired credentials; an explicit revocation timestamp is not implied unless the row is transitioned to REVOKED.
- Phone retirement: every endpoint under the exact Phone tuple must become non-operational; use endpoint lifecycle transitions so the trigger revokes credentials. Verify zero ACTIVE and unexpired credentials after the transaction.
- A failure to enumerate or lock any endpoint/credential causes rollback or a non-terminal DEPROVISIONING result. Never skip an endpoint.

## 18. Attachments, messages, sessions and external storage (Round 5)

### 18.1 Phone attachments and messages

phone_attachments is an association/fencing record, not evidence of a stored file. It records endpoint, Phone, tenant, AI, principal, participant, room, session, execution and fence. It is retained and changed to DETACHED or FENCED; it is not deleted during retirement.

phone_messages stores JSON content and durable delivery state. The schema and trigger show immutable message identity/content and constrained state transitions. No message archive, purge, encryption-key destruction or retention schedule was proven. Preserve all message rows and Phone/AI/principal parent anchors.

### 18.2 Sessions and workloads

A Phone attachment references a session and workload execution, but these are shared authentication/runtime context for a principal and room. A session can be used by Team Room and other authorized operations. Revoking it because one Phone retires can log out or disrupt unrelated work.

Therefore:
- Phone retirement fences its attachment and rejects Phone operations through a Phone lifecycle admission gate.
- It does not close or revoke the principal's session, workload identity or execution by default.
- If the principal or execution is compromised or separately offboarded, revoke it through the authentication/security lifecycle, recording the broader impact.
- Expiration is evaluated independently from explicit revocation.

### 18.3 Team Room and external objects

communication.rs writes team_room_attachment_refs with a storage_ref string and optional message_id/task_id, plus filename, content type, size and optional hash. A test uses an object:// URI, but that is test data, not proof of a production storage provider. message_id is protected by a NO ACTION FK to team_room_messages; task_id has no FK.

Unproven:
- whether storage_ref points to S3, GCS, Azure, local storage or another provider;
- whether the object exists;
- whether object versioning, retention lock, legal hold or shared references exist;
- whether a different tenant/resource references the same object;
- who is authorized to delete it;
- whether a storage deletion API is integrated.

Policy:
1. Never delete an external object during Phone cleanup based only on storage_ref, filename, message_id, task_id or tenant_id.
2. Preserve the database reference and object unless a separate storage inventory proves immutable object identity, exclusive ownership, complete reference closure, no legal hold and authorized deletion.
3. If archive is proposed, copy and verify bytes, hash, metadata, ACLs and restore path before changing references; retain the original until archive verification and retention approval.
4. If ownership or reference closure is uncertain, retain the object and record an unresolved storage item.
5. Avoid orphan creation by requiring a storage tombstone/reference registry and a two-phase delete protocol in any future storage architecture.

Team Room messages, decisions, channels, handoffs, tasks, outbox, audit and lease history are shared collaboration or governance records. They are not automatically Phone-owned because their IDs appear in a Phone mission pointer or an event aggregate field.

## 19. Universal concurrency and write-admission protocol (Round 6)

### 19.1 What is proven today

The TARGET contains row-locking around several Phone endpoint/attachment lifecycle operations. Endpoint registration and revoke lock the Phone row; credential validation locks Phone, attachment, endpoint and credential rows. Session exchange locks an existing session during rotation. Task and handoff code has its own row-lock/fencing logic.

These are local protections, not a universal Phone lifecycle lock. No source evidence demonstrates that every writer in phone.rs, auth.rs, communication.rs, tasks.rs, handoffs.rs, outbox producers, storage workers and the out-of-band operator-key control plane acquires one common Phone lock. Soft references have no FK to the Phone row, so a Phone row lock alone cannot block their insertion.

### 19.2 Required future barrier

Advisory locks may be supplemental coordination only. They cannot be treated as protection for non-participating writers.

Before implementation can be approved, the architecture must provide a database-enforced lifecycle gate:
- one durable lifecycle row/version for each Phone;
- every Phone-scoped writer must resolve the exact Phone and lock the lifecycle row before creating or mutating a dependent;
- a database trigger or a single privileged write API must reject writes unless lifecycle state is ACTIVE or an explicitly allowed transition;
- retirement changes the lifecycle row to DEPROVISIONING under a conflicting row lock, waits for prior admitted writes, then prevents later writes;
- direct table writes by application roles must be revoked or guarded so no writer can bypass the gate;
- soft-reference writers must either carry a resolvable Phone identity and participate in the gate, gain an FK/association table, or remain outside the Phone cleanup graph and be preserved;
- external workers and control-plane writers must acknowledge the same retirement version/fencing token before acting;
- all writes after RETIRED must fail closed, including endpoint re-registration, attach, heartbeat, send, receive, ACK and background delivery.

A candidate lock order is: Phone lifecycle gate; tenant/AI/identity scope anchors; Phone row; endpoints sorted by endpoint_id; credentials sorted by credential_id; attachments sorted by attachment_id; then explicitly owned dependent rows. All writers must use the same order. The exact order must be checked against FK lock acquisition and deadlock traces before implementation.

### 19.3 Race handling

| Race or failure | Required behavior |
|---|---|
| New endpoint or credential registration | Gate lock serializes it; if DEPROVISIONING has begun, reject. |
| New message or message delivery/ACK | Gate lock/DB guard rejects after retirement barrier; existing messages remain. |
| New attachment or heartbeat | Reject after gate closes; old fence becomes invalid. |
| Transfer requested/in progress | Retirement waits for a terminal transfer outcome or aborts as RETRYABLE_BLOCKED; never changes tenant/AI ownership implicitly. |
| Principal/session/workload revocation | Separate security lifecycle; revalidate before transition; do not revoke unrelated sessions as a Phone side effect. |
| Operator key rotation/revocation | Trusted control plane must coordinate key version and active consumers; Phone retirement does not revoke key. |
| New soft reference | If it cannot be resolved to the Phone gate, it is not safe to delete its target; preserve and block physical deletion. |
| Writer not participating | Retirement cannot claim complete isolation; keep Phone non-deleted and return BLOCKED. |
| Deadlock (40P01) | Roll back; bounded retry only after confirming common lock order; repeated deadlock fails closed. |
| Serialization failure (40001) | Roll back and retry same operation ID and same digest only, with bounded attempts. |
| Lock/statement timeout | Roll back; report RETRYABLE_BLOCKED; no partial retirement or broader cleanup. |
| Lost response after commit | Resolve by durable operation receipt; never repeat side effects blindly. |

SERIALIZABLE isolation does not repair a missing lifecycle protocol. It is an additional defense after every writer is covered by the same enforced gate, not a substitute for it.

## 20. Test teardown review — handoffs.rs (Round 7)

At TARGET, handoffs.rs contains PostgreSQL test fixtures under cfg(test). The fixture() function creates a random tenant and room and opens the DATABASE_URL connection. The first handoff test ends with DELETE FROM tenants for that generated tenant. The same file's continuity test explicitly deletes Phone attachments, endpoint credentials, endpoints, messages, mission links, ai_phones, AI/tenant mappings, AI/principal mappings, the generated global AI identity and finally the generated tenant.

This is test teardown, not an operational Phone lifecycle API. It was created to clean test-created rows after committed integration scenarios. The test source itself does not enforce that DATABASE_URL is disposable. The AI identity deletion is only safe if that identity was created exclusively by that test and no other test/process references it.

The AI-PHONE-02C workflow does provision a disposable PostgreSQL 16 service database named ai_phone_02c_audit for its job. That makes the workflow database disposable; it does not make the same test safe when run manually against an arbitrary DATABASE_URL.

Policy for replacing test teardown:
1. Prefer one disposable database or isolated schema/database per integration test or test worker; discard the whole database after the suite.
2. Add an explicit test-environment guard that refuses destructive fixture teardown unless the database is positively identified as disposable and has a dedicated test marker. A database name alone is not a sufficient security boundary.
3. Where database disposal is impossible, use a fixture registry and exact row-level cleanup for only rows created by that test; never delete tenant, global AI, shared principal or shared identity rows.
4. Keep audit/outbox assertions and snapshots until all assertions finish; discard the disposable database after evidence is captured.
5. Keep operational deprovision, test fixture cleanup and physical fixture deletion as three separate APIs/specifications and authorization domains.

Do not run the existing teardown helper as operational cleanup. No source code was changed and no test helper was executed in this mission.

## 21. Executable acceptance plan and invariants (Round 8)

These are future acceptance tests, not executed tests. They must run only after an implementation is explicitly authorized, against disposable PostgreSQL 16 databases and two complete fixtures A and B. Each fixture must contain real Phone endpoints, active and revoked credentials, attachments, messages, mission link, sessions, workload execution, Team Room rows, tasks, lease events, handoffs, outbox and audit rows. Include one AI identity shared across more than one tenant where schema rules permit, and multiple operator actors/endpoints in one AI/tenant scope.

Before and after every test, snapshot all 35 public tables. For each table capture exact primary keys, row counts, canonical row hashes and all relevant parent tuples. Also snapshot the global identity graph, tenant rows, all audit/security/outbox rows, operator-key hashes/status/expiry, and the external-storage reference registry. The snapshot must include zero-row results and distinguish missing rows from empty result sets.

| Test | Action | Required invariant |
|---|---|---|
| A retires | Retire fixture A | Tenant A, global AI identity, identity mappings, audit, security audit, outbox and all B rows remain unchanged. A Phone and RESTRICT anchors remain; only A's endpoint/attachment/credential operational states change. |
| B retires | Retire fixture B | Symmetric preservation of A. |
| Message retention | Retire A with PENDING, DELIVERED, ACK and FAILED messages | Every message ID, content digest, sender/recipient tuple and status history remains; no message is deleted. |
| Parent retention | Retire A with credentials, attachments and messages present | Phone, endpoint, AI, principal, participant, session and execution anchors remain wherever retained FK children require them. |
| Credential revocation | Retire A with multiple endpoints | All and only A's endpoint credentials become REVOKED; no ACTIVE usable A credential remains; B and other keys are unchanged. |
| Operator-key preservation | Retire A | Operator-key rows and statuses remain unchanged unless a separate control-plane key-revocation test is explicitly run. |
| Global identity | Retire A | ai_identities rows and cross-tenant mappings remain unchanged. |
| Tenant preservation | Retire A and B | All tenant rows remain; no tenant delete is issued or counted as success. |
| Audit/outbox | Retire A | Prior audit/security/outbox row IDs and hashes remain; lifecycle audit event is appended; no event is silently removed. |
| Attachment integrity | Retire A | phone_attachments are FENCED/DETACHED; Team Room attachment refs and external objects remain; every storage_ref is accounted for. |
| Sessions/workloads | Retire A | Phone attachment cannot use old fence; shared principal session/workload remains valid for unrelated resources unless independently revoked. |
| Team Room/handoffs/tasks | Retire A | Channels, messages, decisions, tasks, handoffs and lease history remain byte-for-byte unchanged unless separately transitioned by authorized task policy. |
| Rollback injection | Fail after each mutation boundary | Entire transaction rolls back; no credential revocation, endpoint status, lifecycle state or audit receipt is partially committed. |
| Idempotency | Repeat same operation ID + digest | Same durable result, no duplicate events or changed snapshots; same ID with different digest is rejected. |
| Partial fixture | Omit one endpoint/credential/soft-reference manifest item | Operation rejects before mutation or rolls back; no “best effort” retirement. |
| Concurrent attach/register | Race lifecycle retirement with new endpoint/attach | Exactly one serial order; if retirement wins, new operation is rejected; if attach wins, retirement sees and fences it before RETIRED. |
| Concurrent transfer | Race transfer with retirement | No cross-tenant/AI transfer or ownership mutation; operation blocks until transfer terminal state. |
| Writer bypass | Attempt direct insert/update from every app role and worker | Database gate rejects Phone-scoped writes after DEPROVISIONING/RETIRED. |
| Unauthorized retirement | Request without lifecycle authority or approval | No state change, no credential revocation, immutable denial evidence. |
| Storage uncertainty | Provide missing/duplicate/shared/unknown storage_ref | Preserve reference/object and fail any physical purge. |

Acceptance is PASS only if every invariant passes, no protected snapshot changes, all relevant writers are covered by the database gate, and failure injection proves atomicity. A successful API response or zero remaining ACTIVE endpoint alone is insufficient.

## 22. Adversarial policy review (Round 9)

| Attack | Policy defense | Remaining proof required |
|---|---|---|
| Endpoint credential remains usable | Endpoint DETACHED/REVOKED trigger revokes ACTIVE credentials; retirement must enumerate all endpoints and verify expiry/fence. | Integration test across every endpoint and old credential generation. |
| Session remains usable | Phone attachment is fenced; shared session is intentionally retained for other resources. | Prove every Phone API checks lifecycle gate, not only session validity. |
| Operator key revoked by mistake | Key is AI/tenant scoped and excluded from Phone cleanup. | Control-plane consumer inventory and explicit rotation/revocation workflow. |
| Message loss | Message rows are retained and RESTRICT anchor remains. | Full content-hash snapshots and no-delete assertions. |
| Attachment orphan | Attachment association and storage_ref are retained; no object deletion without registry proof. | Storage provider inventory, reference registry and restore test. |
| Audit/outbox loss | Tenant is never deleted; retained rows are snapshotted and preserved. | Outbox consumer/retention policy and full row digest test. |
| Non-participating writer bypass | Advisory lock is not considered universal; DB-enforced lifecycle gate is mandatory. | Prove every writer and DB role is guarded, including background/out-of-band workers. |
| Retired Phone reactivated | RETIRED is terminal in proposed lifecycle table; all attach/register/API writes reject. | Schema migration, authorization test and database trigger enforcement. |
| Duplicate retry side effects | Operation ID + manifest digest + durable receipt; same request returns same outcome. | Atomic receipt test under lost-response fault injection. |
| Partial manifest deletes unknown rows | Complete signed manifest and DB dependency closure; partial manifest fails before writes. | Tamper, omission, extra-ID and stale-target tests. |
| Transfer races retirement | Shared lifecycle gate and deterministic lock order; transfer in progress blocks retirement. | Concurrent integration test with transfer writer participating. |

### 22.1 Corrections made by this policy review

- ACTIVE is explicitly conceptual; it is not a legal current ai_phones.phone_status value.
- DETACHED is explicitly reversible at endpoint level and is not equivalent to Phone retirement.
- Phone-level DETACHING, DEPROVISIONING and RETIRED are future schema states, not existing states.
- Expiration is distinguished from explicit credential/key/session revocation.
- Endpoint credentials are eligible for Phone-scoped revocation; operator keys are not.
- Sessions/workload identities are not automatically revoked because they are shared principal/runtime context.
- A retained Phone attachment preserves its referenced session/principal/participant/execution anchors under RESTRICT.
- External object existence, provider and deletion rights remain unproven; no storage deletion is allowed.
- The handoffs.rs tenant deletion is identified as test teardown with a disposable-CI-database condition, not an operational cleanup method.
- Advisory locks and SERIALIZABLE isolation are no longer presented as sufficient without a database-enforced writer gate.

## 23. Final decision and authorization boundary (Round 10)

| Component | Status |
|---|---|
| Schema compatibility review | PASS — current status values and FK/trigger behavior checked against captured catalog |
| Current Phone lifecycle implementation | FAIL — no persistent Phone RETIRED state or universal write-admission barrier |
| Endpoint credential revocation | PASS — trigger and handler behavior confirmed; complete retirement coverage untested |
| Operator-key policy | SPECIFIED — key excluded from Phone cleanup; control-plane ownership remains unproven |
| Parent retention policy | SPECIFIED — preserve Phone/endpoint and all RESTRICT anchors |
| External storage policy | BLOCKED — provider, ownership and object reference closure unproven |
| Universal concurrency guarantee | BLOCKED — no common enforced writer gate demonstrated |
| Test teardown classification | PASS — test-only code identified; arbitrary DATABASE_URL is not guarded in the test helper |
| Two-fixture acceptance plan | SPECIFIED, NOT EXECUTED |
| Adversarial policy review | PASS as a design review; implementation defenses not tested |
| Production implementation | NOT AUTHORIZED |
| Integration / merge | BLOCKED |

Final policy:
1. Phone lifecycle default is deprovision/retire, never tenant deletion.
2. Preserve Phone, endpoints, messages, attachments, credentials, audit and all required parent anchors.
3. Revoke only exact endpoint credentials through endpoint lifecycle transitions.
4. Preserve operator keys unless the trusted control plane independently authorizes revocation after consumer analysis.
5. Do not revoke shared sessions, workload identities, tasks, handoffs or Team Room state as a Phone side effect.
6. Do not delete external objects without verified exclusive ownership and a separate approved storage lifecycle.
7. Do not claim retirement is secure until a durable lifecycle state and database-enforced writer barrier cover every writer.
8. Any missing ownership proof, unknown consumer, lock failure, unguarded writer or partial manifest results in fail-closed preservation.

**Final classification: BLOCKED.** The retention and authorization policy is now specified and internally aligned with the captured schema, but external storage ownership, lifecycle schema/barrier, writer-wide participation, complete tests and implementation review remain unresolved.

**GPT-6 authorization requested for policy review only. No corrective implementation, migration, operational cleanup, 160-race run, 20-handoff run, integration or merge is authorized.**


---

## 24. Addendum — closure of workflow 43477503344

This section records the workflow inspection performed for the current mission. It supplements, and does not replace, the earlier workflow evidence in §2.

| Field | Verified result |
|---|---|
| Workflow run | `43477503344` |
| Event / ref | `push` / `refs/heads/audit/ai-phone-02c-execution` |
| Workflow commit / audit harness checkout | `b612076d59af0f69f06f1a5b275abec1fb6c688f` |
| Immutable implementation checkout | `ebb8eeecfea087443c5f3eba7021e96cb5302fe7` |
| Job | `132065863583` — Exact-SHA HTTP + PostgreSQL + WebSocket audit |
| Job conclusion | **FAILURE** |
| PostgreSQL | 16.16, `server_version_num=160016`, database `ai_phone_02c_audit` |
| Artifact | `ai-phone-02c-dynamic-audit-43477503344.zip`, artifact ID `14064122164` |
| Artifact SHA-256 | `990bf2cfc08a2f31298d54a990a592ec3f75b878122d414a350b3b266c675965` |

The job completed its setup, initialized its PostgreSQL service container, checked out the audit branch commit and the immutable TARGET separately, and passed the exact-SHA and source-blob provenance checks. The run-context file confirms both `TARGET_SHA` and `TARGET_HEAD` equal the immutable TARGET. The checked source blobs for `communication.rs`, `tasks.rs`, and `handoffs.rs` matched the pinned values.

### 24.1 Steps and results

| Step | Result | Interpretation |
|---|---|---|
| Set up runner and PostgreSQL service container | PASS | A job-local PostgreSQL 16 service was started. |
| Checkout audit harness/workflow | PASS | Checked out `b612076...`. |
| Checkout immutable implementation SHA | PASS | Checked out `ebb8ee...` separately. |
| Verify exact implementation and harness provenance | PASS | TARGET and pinned source blobs matched. |
| Install Rust stable / Node 22 | PASS | Toolchain setup only. |
| Confirm PostgreSQL 16 disposable database | PASS | Version and database probe succeeded; the workflow's connection string targeted its job-local service. |
| Inject audit-only HTTP test into target worktree | PASS | Copied the audit test into the runner's TARGET worktree and ran rustfmt; this did not commit or push a production source change. |
| Cargo check exact implementation | PASS | Compile/check only. |
| Legacy migration compatibility | PASS | Script ran against the job-local PostgreSQL service. This is not a production migration. |
| Full exact-SHA PostgreSQL/Registry/Team Room/WebSocket test suite | **FAIL** | 12 passed and 3 security assertions failed in the audit network target. |
| PostgreSQL catalog audit | PASS | Catalog was captured after the failing test stage. |
| Frontend production build | SKIPPED | The workflow skipped this step after the earlier suite failure. |
| Build/evidence manifest | PASS | Provenance and source hashes were written. |
| Upload evidence artifact | PASS | Artifact uploaded successfully. |
| Stop containers / complete job | PASS | The job-local database container and network were stopped and removed. |

### 24.2 Executed test outcomes

| Test target | Result |
|---|---:|
| Rust library unit tests | 6 passed, 0 failed |
| Main binary / PostgreSQL and HTTP integration tests | 10 passed, 0 failed |
| Admission PostgreSQL tests | 3 passed, 0 failed |
| `ai_phone_02c_network` | 12 passed, 3 failed |
| **Total completed** | **31 passed, 3 failed** |

The three failed security assertions were:

- `ai_phone_02c_round1_case1_valid_tenant_random_ai_only`
- `ai_phone_02c_round1_case4_valid_tenant_nonexistent_operator_key`
- `ai_phone_02c_round1_case5_tenant_a_ids_from_different_fixtures`

The random-Phone-only and incompatible-principal cases passed because foreign-key constraints rejected the attempted cleanup. They do not establish ownership validation. The other 12 tests in the network target passed, including the ten-cycle fixture-isolation test, Phone transfer continuity, and seven-real-WebSocket revocation/preservation test.

Other executed concurrency/scale-related tests included the ten-concurrent-executor lease test, the handoff candidate/state test, the sixteen-simultaneous-admissions test, and seven-WebSocket tests. **The 160-race test and the 20-handoff test were not present in the executed test output and were not run.** The separate `scale_1_to_7_postgres.rs` target was present in the source-hash manifest but did not appear as an executed test target in the captured suite log; do not claim that this workflow validated the 1→7 scale target.

The workflow did execute destructive fixture teardown code in its disposable CI database. Test sources contain `DELETE FROM tenants` and fixture-scoped resource deletions; the PostgreSQL log records tenant-delete attempts, including attempts rejected by foreign keys. This is a test-environment side effect, not evidence of an operational tenant deletion. No production database URL or production database connection was configured by this workflow. The test helper itself, however, does not independently prove that an arbitrary locally supplied `DATABASE_URL` is disposable; that remains a teardown blocker (§30).

### 24.3 Failure interpretation and effects

The workflow is not green and cannot be treated as architectural approval. Its three failing assertions reproduce the known unsafe cleanup behavior. The successful compile, migration-compatibility script, catalog capture, and passing tests do not offset those failures.

The workflow checked out TARGET detached, injected a test file only into its temporary worktree, ran tests and catalog queries against the job-local PostgreSQL service, and removed that service at job completion. It did not push commits, update the audit branch, modify TARGET, or alter production code. Test SQL did mutate the disposable test database, including fixture teardown attempts. No corrective migration or production cleanup ran.

The uploaded ZIP contains the run context, full test log, PostgreSQL catalog, evidence manifest, cleanup JSONL, WebSocket evidence, and audit harness/workflow snapshots. The ZIP digest above was independently checked against the downloaded file.

---

## 25. Proposed persistent Phone lifecycle (design only)

### 25.1 Separate lifecycle from current operational statuses

The current `ai_phones.phone_status` values remain exactly those already recorded in the catalog: `ATTACHED`, `TRANSFER_REQUESTED`, `TRANSFERRING`, `ATTACHED_NEW_ENDPOINT`, and `DISCONNECTED`. Endpoint, attachment, credential, session, workload and operator-key status domains remain separate. None of the proposed lifecycle values below is asserted to exist in the current schema.

Propose a dedicated, one-row-per-Phone lifecycle aggregate, independent of `ai_phones.phone_status`:

| Proposed lifecycle state | Meaning | New Phone-scoped writes |
|---|---|---|
| `ACTIVE` | Phone may be attached and used under current authorization | Allowed through the lifecycle gate |
| `DETACHING` | A reversible disconnect operation is in progress | Only the idempotent detach operation and safe reads |
| `DETACHED` | No active endpoint attachment; Phone identity and history remain | Attach may be requested with fresh authorization and credentials |
| `DEPROVISIONING` | A separately approved, irreversible deprovision operation has begun | Deny attach, registration, messages, mission-link mutation and normal Phone writes; allow only lifecycle worker operations and reads |
| `RETIRED` | Terminal, non-operational Phone identity retained as an audit/reference anchor | Read-only; no reactivation or normal writes |

Do not add `RETIRED` or `DEPROVISIONING` to the existing `phone_status` check constraint. The lifecycle aggregate is authoritative for lifecycle admission; the existing status fields continue to describe connectivity and endpoint operation.

A separate operation/receipt record should capture `operation_id`, immutable request digest, Phone/tenant IDs, requested transition, requester identity, approver identity, creation/completion timestamps, lifecycle version, result, failure code and evidence-manifest hash. An append-only lifecycle event stream should record every accepted/rejected transition and every worker phase. The lifecycle row should also carry a monotonically increasing version/fence epoch and the current operation ID. A retry with the same operation ID and digest returns the recorded outcome; the same ID with a different digest is rejected.

### 25.2 Explicit transition graph

```text
ACTIVE -> DETACHING -> DETACHED
                       |   ^
                       |   | fresh attach + new valid endpoint credential
                       v   |
                 DEPROVISIONING -> RETIRED
```

- **DETACH** is a reversible connectivity operation. It fences the current attachment, advances the Phone fence, marks the endpoint detached, and relies on the existing endpoint lifecycle trigger to revoke credentials for that endpoint. The Phone and its history remain. A later attach is a new authorization event with a fresh endpoint credential; it does not reactivate the old credential.
- **DEPROVISION** is a separate governance/security operation. It can begin only from `DETACHED`, after transfer/handoff/task prerequisites have been resolved and the lifecycle write barrier is active. It disables further Phone operations but preserves Phone, endpoint, message, attachment, credential, mission-link, audit, outbox and parent-anchor records. It does not mean delete.
- **RETIRE** is the terminal state reached only after all deprovision checks and evidence are complete. It preserves the Phone row as a referential and audit anchor. No ordinary API, tenant administrator, operator key or retry may transition it back.

Prohibited transitions include `ACTIVE -> RETIRED`, `ACTIVE -> DEPROVISIONING` without the detach/precondition phase, `DEPROVISIONING -> ACTIVE`, `DEPROVISIONING -> DETACHED` after any irreversible side effect, and every transition out of `RETIRED`. A failed worker attempt is an operation result, not a new lifecycle state: leave the Phone in `DEPROVISIONING`, persist the failure and retry the same operation safely. Do not silently roll it back to an operational state.

If a detach transaction fails before commit, its transaction rolls back and the Phone remains `ACTIVE`. If the detach commits but a later caller loses its response, retry by operation ID and return the durable receipt. Reattachment after a committed detach always requires a new attach authorization and new credential.

### 25.3 Authority and evidence

The following are proposed authority classes, not current permissions or existing roles:

1. **Requester:** tenant administrator or delegated Phone owner may submit a detach/deprovision request. Existing `phone.detach` and `phone.attach` apply only to their currently implemented operations; they do not authorize retirement.
2. **Independent approver:** a distinct platform/security lifecycle approver reviews deprovision/retire. Requester and approver must be different principals. Approval must be fresh, scoped to the exact Phone, tenant, operation digest and expiry.
3. **Lifecycle executor:** a narrowly privileged service executes only the approved operation. It cannot approve its own request or widen the manifest.
4. **Break-glass authority:** separately authenticated, time-limited and audited; cannot bypass retained-data or ownership invariants.

No existing `phone.lifecycle.manage` permission or lifecycle approver was found. Do not reuse `phone.endpoint.manage` or the AI/tenant operator key as lifecycle approval. A later implementation proposal must define and independently review the actual permission names and control-plane identity.

Evidence must include authenticated requester and approver IDs, tenant/Phone tuple, reason/ticket, request digest, expected lifecycle version, exact endpoint/attachment/task/handoff snapshots, active-credential inventory, transfer quiescence proof, storage-reference inventory, legal-hold result, barrier version, all precondition results, worker receipts, and final snapshot digest. Store no raw credential or operator key.

### 25.4 Exceptional recovery

Treat `RETIRED` as irreversible for the original Phone identity. An exceptional service restoration should create a **successor Phone identity** under a separately approved recovery operation, not change the old row back to `ACTIVE`. Preserve the retired record and link the successor to it through an audited lineage relation.

There is a current schema constraint to resolve before promising same-AI successor provisioning: `ai_phones` has unique `(tenant_id, ai_id)`. Retaining the retired row therefore prevents inserting a second Phone for the same AI/tenant pair. A successor must use a new AI identity unless a separately reviewed future schema design introduces a Phone generation/identity model. Do not assume that dropping the old anchor or reusing the same ID is acceptable.

---

## 26. Universal write barrier — required architecture

### 26.1 Database-enforced gate, not advisory-lock convention

The proposed barrier must not rely solely on advisory locks, application mutexes, request middleware or SERIALIZABLE isolation.

The lifecycle aggregate is also the Phone write gate. Every Phone-scoped write transaction must acquire a shared row lock on the exact lifecycle row **before** locking or modifying dependent rows, verify `lifecycle_state = ACTIVE` and the expected version/fence, then perform the write. A transition to `DETACHING`, `DEPROVISIONING` or `RETIRED` takes an exclusive row lock on that same gate, waits for earlier writers to finish, advances the version/fence and commits the new state. Writers queued behind it then re-read the state and reject. This establishes a serialization point: a write is either committed before the transition or rejected after it; it cannot pass a stale pre-check and commit after the transition.

To make the lock order enforceable:

- Route protected writes through narrowly scoped database write procedures that acquire the lifecycle gate first.
- Revoke direct INSERT/UPDATE/DELETE privileges on Phone-scoped tables from runtime application, worker and integration roles. Grant only EXECUTE on reviewed procedures.
- Use database triggers as defense-in-depth to reject protected table mutations when lifecycle admission is closed or required scope is absent.
- Keep migration ownership and break-glass roles separate from runtime roles; no runtime service account may be a table owner or superuser.
- Use fixed `search_path`, qualified object names, least privilege and reviewed SECURITY DEFINER functions. No caller-controlled dynamic SQL.
- Do not use a caller-settable custom GUC as proof that the caller passed the gate. Runtime roles can set arbitrary custom GUC values; it is not a trusted capability.
- Require every worker, control-plane process, scheduled task and external writer to use the same procedures or an equivalent DB-enforced contract. An unknown writer is a blocker, not an assumed participant.

A trigger that discovers and locks the lifecycle row only after the target child row has already been locked can introduce lock-order inversions against retirement. The implementation must demonstrate a consistent gate-first order for every writer; trigger-only enforcement with uncontrolled direct DML is not sufficient. Deadlocks, serialization failures and timeouts must abort the whole transaction. Bounded retry is permitted only with the same idempotency key and only after a fresh authorization/gate check.

### 26.2 Resource scope and shared resources

A Phone gate can be applied directly where the row contains an enforced `ai_phone_id`: `ai_phones`, `phone_endpoints`, `phone_endpoint_credentials`, `phone_attachments`, `phone_messages`, and `phone_mission_links`.

It cannot safely be inferred from tenant, principal, room, session, workload, task, handoff or opaque aggregate IDs alone. For shared resources, a future implementation must either:

- maintain a normalized, FK-backed resource-to-Phone association and lock all affected Phone gates in deterministic order; or
- explicitly classify the operation as belonging to the shared Team Room/principal domain, not as a Phone-scoped write, and ensure Phone-specific callers cannot use it to bypass the Phone gate.

The current `main.rs` task/handoff routes are principal/session/room based and do not carry a Phone ID. The current `communication.rs` Team Room routes likewise operate on room/principal/channel context. If the product requirement is that retiring a Phone also blocks actions initiated by that Phone through these routes, the current request context is insufficient to identify the Phone. This is a hard blocker until a trusted Phone context or explicit resource association is added. Do not globally block a shared room or principal merely because one Phone is retiring.

---

## 27. Writer inventory and current barrier coverage

The following inventory is based on the immutable TARGET source blobs reviewed for this mission: `phone.rs` (`ea005944...`), `auth.rs` (`c1ccf50f...`), `communication.rs` (`4a89ffd...`), `tasks.rs` (`b6bd7f3...`), `handoffs.rs` (`f6736c7...`), and `main.rs` (`25d72f...`). It identifies observed paths, not every possible external control-plane or database writer.

| Domain / tables | Observed entry points and writes | Existing transaction / protection | Lifecycle bypass or gap |
|---|---|---|---|
| Phone identity and binding: `ai_identities`, `ai_identity_principals`, `ai_identity_tenants`, `ai_phones` | `phone.rs`: identity provisioning and Phone provisioning; Phone event audit/outbox | Provisioning uses transactions and selected row locks; unique/FK constraints protect some identity relationships | No lifecycle row or universal Phone gate. Identity/control-plane provisioning outside this service is not inventoried. Preserve global AI identity and tenant mappings. |
| Endpoint and credential: `phone_endpoints`, `phone_endpoint_credentials` | `phone.rs`: register, rotate, revoke, attach, detach, heartbeat; credential last-used updates | Some operations lock Phone/attachment/endpoint/credential rows. Endpoint status trigger revokes active endpoint credentials | Existing locks are operation-specific, not a common lifecycle gate. External credential/control-plane provisioning is not fully inventoried. |
| Attachment and Phone mailbox: `phone_attachments`, `phone_messages` | `phone.rs`: attach/detach, message send, delivery, ACK, list and fence changes | Several paths use Phone/attachment row locks and conditional updates; message identity/content trigger protects immutability | No Phone lifecycle admission barrier on every path; no proof that background delivery or future workers all participate. |
| Mission/task association: `phone_mission_links`, `ai_phones.current_task_id/current_mission_id` | `phone.rs` mission-link handler; task/handoff modules mutate task state | Phone mission-link operation updates Phone and link rows; tasks/handoffs lock task/handoff rows | Task/handoff routes do not consistently carry Phone identity. Logical links and cross-resource updates need explicit association/gate design. |
| Operator keys: `phone_endpoint_operator_keys` | `phone.rs` validates a key for endpoint operator actions; test/control-plane fixture code provisions keys | Hash-only key lookup checks ACTIVE and expiry; schema is tenant+AI scoped | Provisioning is out-of-band in the reviewed migration; no Phone/endpoint owner or consumer registry. Must not be revoked by Phone lifecycle. |
| Sessions and workload: `sessions`, `workload_identities`, `workload_executions` | `auth.rs`: exchange, replay insertion/expiry cleanup, session/execution creation, expiry-on-use, close; `main.rs`: workload heartbeat and membership presence | Session exchange and close use transactions/conditional updates; authentication validates active/revoked/expiry state | Session and workload are principal/room runtime resources, not Phone-owned. Phone deprovision must not close/revoke them. No Phone gate should be added to independent session lifecycle. |
| Room membership and presence: `room_memberships`, `workload_executions`, `workload_identities` | `main.rs`: join/leave/presence/heartbeat and logout paths | Authenticated session and room-scope checks; selected conditional updates | Shared principal/room operations; no Phone ID. If Phone-originated operations must be blocked, request context is currently inadequate. |
| Team Room channels/members/reads/messages: `team_room_channels`, `team_room_channel_members`, `team_room_message_reads`, `team_room_messages` | `communication.rs`: general/private channel creation, membership, message reads, message insert and outbox | Channel/message writes use transactions; message idempotency unique key; WebSocket ticket consumption is conditional | Shared collaboration data; no Phone gate or Phone ownership. Keep independent unless a specific Phone association is established. |
| Team Room decisions and attachment references: `team_room_decisions`, `team_room_attachment_refs` | `communication.rs`: decision creation, attachment-ref creation, list; storage reference persisted | Decision and attachment-reference plus outbox writes use DB transactions | `storage_ref` is opaque; no provider/version/ownership proof. These are shared room records, not Phone-owned objects. |
| Tasks and lease history: `team_tasks`, `task_lease_events` | `tasks.rs`: create, acquire, renew, checkpoint, complete, recover, release | Task row `FOR UPDATE`; transactional task/event/outbox writes; fencing trigger | Task can be shared/linked. Existing task lock is not a Phone gate. A Phone-related task must resolve its Phone associations and gate them; otherwise treat it as shared. |
| Handoffs: `task_handoffs`, `task_lease_events` | `handoffs.rs`: create, recovery selection, accept, reject, cancel; `tasks.rs` updates pending/accepted handoffs | Transactions lock task and/or handoff rows; state-transition trigger; audit/outbox writes | No universal Phone gate; active transfer/handoff must block retirement when it is demonstrably tied to the Phone. Other principal handoffs remain independent. |
| Audit and outbox: `audit_events`, `security_audit_events`, `outbox_events` | Writers in `phone.rs`, `auth.rs`, `main.rs`, `communication.rs`, `tasks.rs`, `handoffs.rs` | Many business write/outbox pairs share a transaction; audit rows are append-oriented | Outbox aggregate IDs and several audit resource IDs are logical/soft references. No complete outbox consumer inventory or global Phone association was demonstrated. Preserve history. |
| Logical references | `ai_phones.current_task_id/current_mission_id`, `phone_mission_links.task_id`, `task_handoffs`, Team Room attachment `message_id/task_id/storage_ref`, audit resource IDs, outbox aggregate IDs | Some are FK-backed; others are soft references or opaque text | A tenant/room/principal match is not proof of Phone ownership. Preserve unknown references and require explicit association closure. |
| Background, control-plane and external writers | No complete source inventory was established for deployment jobs, external control plane, outbox consumers, storage provider, admin SQL or future workers | Unknown | **BLOCKER:** cannot claim universal writer coverage until each identity, process, DB role, table and transaction path is inventoried and made to participate. |

### 27.1 Required writer admission matrix before implementation

For every writer in the preceding matrix, the implementation review must record: service/process identity; route, queue or scheduler entry point; exact SQL tables and columns; transaction boundary; DB role; lifecycle gate acquisition point; resource-to-Phone resolution; idempotency key; retry policy; trigger/procedure enforcement; and a negative test proving a direct or stale write is rejected. The matrix must include all runtime DB roles and any external control-plane API that can register endpoints, rotate credentials or provision operator keys.

The current service's row locks are useful local concurrency controls, not evidence of a universal gate. The current task and handoff locks protect task/handoff state, not Phone lifecycle. Existing FK and transition triggers protect referential/state invariants, not whether a Phone is retired.

---

## 28. Operator-key lifecycle policy

The current key record is tenant+AI scoped, stores a hash, has ACTIVE/REVOKED state and expiry, and has no `phone_id`, `endpoint_id`, consumer principal or owner identity. The reviewed migration describes raw key provisioning as out-of-band by a trusted control plane. The reviewed service checks key status and expiry when authorizing endpoint operator actions. This proves the lookup behavior, not the full inventory of key consumers.

Policy:

1. **Provisioning authority:** trusted control plane only. Phone API and tenant administrators cannot mint or widen a shared operator key.
2. **Consumer inventory:** before changing a key, enumerate every service, deployment, operator, worker, Phone endpoint-management path and automation that possesses or uses it. Record stable consumer ID, owner principal/service identity, environment, allowed operations, scope, key ID/generation, issued/expiry timestamps, last successful use and migration status.
3. **Per-consumer credentials:** future key issuance should prefer distinct consumer-scoped credentials with least-privilege operation scopes. Do not pretend the current shared AI/tenant key is already per-Phone.
4. **Expiry:** every authorization checks server-side expiry. Expiry must be explicit and bounded; no silent renewal. Expired credentials fail closed and are rotated through the control plane.
5. **Rotation:** issue a new key generation, distribute to each inventoried consumer, verify successful use and old-key non-use, then revoke the exact old key ID. Keep overlap short and explicitly bounded. Store hashes only; never log raw keys.
6. **Revocation authority:** trusted control plane plus independent security approval for shared credentials. Phone lifecycle executor has no authority to revoke the key merely because one Phone is detaching, deprovisioning or retiring.
7. **Selective revocation:** allowed only when the key is proven exclusive to one consumer or when every consumer has migrated and acknowledged. Record consumer inventory snapshot, key ID, generation, approver, reason, usage evidence and revocation result.
8. **Blast-radius proof:** before revocation, compare the complete consumer registry and recent usage with the proposed impact set; run a staged replacement and health verification for every remaining consumer. If consumer inventory is incomplete, preserve the key.
9. **Recovery from mistaken revocation:** do not reactivate or recover the raw secret. Provision a new generation, distribute it to affected consumers, validate health and audit the incident. Preserve the revoked-key record.
10. **Phone retirement:** no automatic operator-key revocation. A Phone retirement may request a control-plane review, but the key decision is independent.

No complete consumer registry or external provisioning/revocation workflow was evidenced in TARGET. **Operator-key implementation remains blocked** until the control plane supplies a complete consumer inventory and proves selective revocation cannot interrupt another authorized operation.

---

## 29. External storage ownership and retention contract

The reviewed service persists Team Room attachment metadata including opaque `storage_ref`, `sha256`, byte length, content type, file name and uploader/message/task references. The reviewed repository paths do not establish the storage provider, account/bucket, immutable object version, object ownership, object-lock/legal-hold status, or a complete external reference graph. A SHA-256 field is useful integrity evidence; it does not prove ownership or deletion authority.

Proposed object registry (future schema, not current):

- provider and provider account/project;
- bucket/container/namespace;
- immutable object key plus immutable generation/version ID;
- tenant and uploader/creator identity;
- content SHA-256, byte length, content type and encryption-key reference;
- creation/upload-intent ID and idempotency key;
- object lifecycle state, retention-until, legal-hold state and source policy;
- every database reference ID and its type;
- ownership/authorization evidence and audit event IDs.

An object is not exclusively Phone-owned merely because one Phone can see an attachment reference. Deduplicated or multiply referenced objects remain shared. `team_room_attachment_refs` is a room-level reference record and must not be deleted as a Phone side effect.

### 29.1 Safe object protocol

**Upload:** create a durable upload intent; allocate a unique immutable object version; upload to a quarantine/pending namespace using a conditional write; verify provider-reported version, size and checksum; then commit the registry row, attachment reference and outbox event in one DB transaction. If upload succeeds but DB commit fails, preserve the object as an uncommitted orphan and reconcile it by upload-intent ID. Do not delete it automatically.

**Reference:** add a reference only while the object registry is in a referenceable state. Lock the registry row and verify tenant, object version, checksum, retention and legal hold. Multi-reference/deduplication must be explicit and reference-counted from a normalized relation, not inferred from a mutable counter alone.

**Retain:** preserve by default. Retention expiration is not deletion authorization. Legal hold, contractual retention, incident preservation, audit obligations or unresolved ownership override ordinary TTL.

**Delete request:** require an independent storage owner and policy approver, a complete reference-closure snapshot, zero live references, no legal hold, expired retention, exact object version, and a durable deletion operation ID. Atomically mark the registry `DELETE_PENDING` and write an outbox intent. New references must be rejected once deletion is pending.

**External delete:** a worker issues a conditional delete for the exact immutable provider version, using the operation ID for idempotency. It records provider request/result IDs and the returned version/tombstone evidence. On timeout or ambiguous response, do not assume deletion succeeded or failed; query the exact object version and reconcile.

**Partial failure:** database rollback cannot undo a successful external deletion. Therefore external deletion occurs only after a committed deletion intent and complete precondition snapshot. If the provider deletes successfully but the final DB update fails, keep the row `DELETE_PENDING`, prevent new references, and let a reconciler verify the provider tombstone and finalize the receipt. If the provider cannot support immutable version addressing or conditional deletion, automatic deletion remains disabled.

**Recovery:** upload or delete failures remain durable pending operations with retryable receipts. Never restore a deleted object by silently changing its state; any restore must use provider versioning/backup and a separate authorized restore record.

Until provider, object identity, ownership, all references and legal-hold authority are proven, preserve the object and block deletion. **Storage remains BLOCKED.**

---

## 30. Sessions, workloads and shared execution policy

The schema and reviewed source show that sessions bind tenant, room and principal; workload identities bind tenant and principal plus issuer/subject; workload executions bind session, participant, principal and room. Phone attachments reference a session and workload execution. That relationship is not exclusive ownership.

| Resource | Observed scope | Phone retirement behavior |
|---|---|---|
| Session | Principal + tenant + room; may support non-Phone room activity | Preserve. Do not close or revoke because one Phone retires. |
| Workload identity | Principal + issuer/subject within tenant | Preserve. Revoke only through its independent identity/security lifecycle. |
| Workload execution | Session + principal + participant + room execution context | Preserve and allow its independent lifecycle to finish. Do not kill by indirect Phone association. |
| Phone attachment | Exact Phone + endpoint + session + execution + participant/principal/room | Fence/detach the exact attachment and retain its history/anchors. |
| Task lease / handoff | Shared Team Room task with principal/session/execution owners and durable handoff history | If the task is proven to be the Phone's active mission, require a separate safe release/handoff before retirement. Do not delete task/history or revoke the underlying session. |

Phone deprovisioning must fence the exact Phone attachment and reject future Phone-scoped operations. It must not call `close_session`, mark a session REVOKED/CLOSED, revoke a workload identity, or mark a workload execution REVOKED/CLOSED solely because an attachment row references it.

An in-flight workload is a precondition blocker, not an invitation to kill it. If a Phone-specific worker process must stop, that must be a separate control-plane operation with proven exclusive worker ownership and its own receipt. A shared principal or session may continue using Team Room through other authorized paths.

Preserve session, workload, task, handoff, lease-event, audit and outbox history. Any independently justified session/workload revocation must use the exact resource ID, independent security authority, reason, approval and audit event. **The non-revocation and retention policy is APPROVED_FOR_IMPLEMENTATION as a policy boundary; the actual Phone gate and task association integration remain dependent on the blocked writer-barrier work.**

---

## 31. Disposable test teardown contract

### 31.1 Observed risk

Test-only teardown code in the reviewed source paths performs direct fixture deletions, including tenant deletion. The 1→7 scale test source also contains cleanup statements for Phone messages, attachments, endpoint credentials, operator keys, endpoints, Phones, AI/tenant bindings, AI identities and tenants. The current test helpers accept the process database pool; they do not independently establish that the target database is disposable.

The inspected workflow did use a job-local PostgreSQL service container and removed it after the run. That is a useful CI boundary, but the test helper's safety must not depend on CI being the only caller or on a database name/URL string.

### 31.2 Required runner-owned environment

The test harness must create and own its database environment. It must not accept an arbitrary externally supplied `DATABASE_URL` as the authority to run destructive teardown.

Required controls:

1. A test orchestrator creates a fresh isolated PostgreSQL cluster/container per execution or worker, with a random run nonce, random credentials, private network/socket and a dedicated non-production database role.
2. The orchestrator records the container/runtime identity and PostgreSQL cluster system identifier immediately after startup. The test process receives a short-lived signed/unguessable lease proving this exact cluster belongs to this run.
3. Before migrations or tests, the harness verifies the live connection against the orchestrator's lease: cluster system identifier, database OID, expected role OID/attributes, non-superuser/non-owner status, read-write state, isolated runtime identity and run nonce. A database name or environment variable alone is never accepted as proof.
4. The harness constructs the connection string itself. If an external `DATABASE_URL` is present, fail closed unless the explicit test orchestrator has replaced it with its own lease-bound connection. No fallback to a developer or deployment URL.
5. Each worker receives a separate database or isolated schema/role with no shared fixture identifiers. A worker cannot delete another worker's data.
6. The runtime test role has only the privileges required by the test; it cannot connect to production, cannot assume migration-owner or superuser roles, and cannot perform cluster-wide operations.
7. At completion, discard the whole ephemeral cluster/container/volume. Prefer environment disposal over SQL deletion of tenants/global identities. If row-level teardown is needed inside a test, use a transaction rollback or a manifest-scoped test-only procedure restricted to the harness-owned database.
8. If cluster identity, lease, role, isolation or ownership cannot be verified, abort before executing any destructive statement. No best-effort fallback.

A strong implementation should include a negative proof: a production-like connection/role cannot be obtained from the harness and cannot execute tenant/identity/Phone cleanup; supplying an external URL causes the harness to abort before opening a write connection. The test should record the cluster lease, system identifier, run ID, worker ID and final disposal receipt.

The current workflow's job-local service was ephemeral, but its probe checked server version and read-only status, not the full cluster-ownership lease above. Local test helpers remain unsafe if pointed at an arbitrary writable database. **Teardown architecture is specified but remains BLOCKED until the runner-owned guard is implemented and independently tested.**

---

## 32. Integrated acceptance plan — not executed

No test in this section was run as part of this mission. The plan must use two complete, independently manifested Phone fixtures A and B, plus a shared principal/session/task/storage fixture where appropriate. Snapshot every relevant table with exact primary keys, row counts, canonical row hashes, tenant/AI/principal tuples, lifecycle version/fence, credential key hashes/status/expiry, audit/security/outbox rows, task/handoff state, attachment refs and external object version metadata.

### 32.1 Architectural tests

- Validate every proposed state transition, forbidden edge, requester/approver separation, operation digest, retry receipt and terminal retirement.
- Prove lifecycle gate row locking serializes concurrent writers and retirement under READ COMMITTED and higher isolation.
- Prove all application/worker DB roles lack direct DML on protected tables and cannot assume owner/superuser.
- Prove every writer in §27 participates and every Phone-associated resource resolves to the correct gate.
- Prove shared Team Room/session/workload operations remain available to unrelated principals/Phones.
- Prove a stale lifecycle version, missing Phone context, missing association or incomplete writer inventory fails closed.

### 32.2 Integration tests

- ACTIVE → DETACHING → DETACHED; retry after lost response; reattach with fresh credential; old credential remains rejected.
- DETACHED → DEPROVISIONING → RETIRED only with complete approval and preconditions; no direct ACTIVE → RETIRED.
- Endpoint credential revocation for all exact endpoint generations while retaining credential rows and hashes.
- Phone A deprovision does not alter Phone B, shared AI/tenant identity, principal, session, workload, task, handoff, Team Room, messages, audit or outbox.
- Operator key rotation across multiple consumers; exact consumer migration; prove unrelated consumers remain healthy.
- External object upload/reference/retention/delete saga with versioned provider, duplicate references, legal hold, timeout and reconciliation.
- Session/workload remains usable by unrelated authorized paths; Phone attachment is fenced; active task is handed off or safely released without deleting history.
- Disposable test harness starts a private cluster, rejects an external URL, proves runtime role cannot reach production, isolates workers and discards the whole environment.

### 32.3 Adversarial tests

- Direct SQL from every runtime DB role attempts INSERT/UPDATE/DELETE on every protected table.
- Writer validates ACTIVE, pauses, lifecycle retirement commits, then writer resumes; write must reject.
- Writer acquires gate first, then retirement waits; writer commits before transition and no write commits after retirement.
- A writer bypasses the Rust API through a queue, scheduler, control-plane endpoint or direct SQL role.
- Shared operator key is used by two consumers; revocation for one Phone must not disable the other.
- A shared session/workload is referenced by A and B; retiring A cannot close it.
- A shared or multiply referenced object is presented as Phone-owned; deletion must reject.
- A stale worker attempts to write using an old fence/version.
- A RETIRED Phone attempts attach, endpoint registration, credential rotation, message send/ACK and mission-link mutation.
- A partial or tampered resource manifest omits a parent/reference or introduces a foreign tenant ID.
- Database commit, provider upload/delete and outbox delivery fail at each boundary; retries must be idempotent and preserve evidence.
- Test harness receives an operational-like connection string, wrong cluster lease, owner role or superuser role; it must stop before cleanup.

### 32.4 Scale tests

Scale tests are a later, separately authorized phase. They must measure 2→7→15 Phones, mixed concurrent attach/detach/register/message/task/handoff operations, gate contention, queue latency, deadlocks, retry amplification, outbox lag and isolation. The 160-race and 20-handoff tests remain explicitly unexecuted and prohibited in this phase. Do not infer scalability from the 7-WebSocket or 16-admission tests in workflow 43477503344.

### 32.5 Review and invariants

A reviewer independent of the implementation author must inspect the schema, privileges, triggers, procedures, writer inventory, control-plane integration, object-provider contract and test harness. Acceptance requires:

- No write commits after the lifecycle transition's serialization point.
- No Phone A operation changes Phone B or shared resources without a separately authorized shared-resource operation.
- No tenant, global AI identity, principal, message, attachment metadata, audit, outbox, task or handoff history is deleted by Phone retirement.
- No operator key, session or workload is revoked solely due to Phone association.
- No external object is deleted without exact ownership, version, reference closure, retention and approval proof.
- Every lifecycle request has a durable idempotent receipt and complete audit evidence.
- Every failure is fail-closed; no automatic reactivation after deprovisioning starts.
- Every destructive test runs only in an orchestrator-owned ephemeral cluster and cannot reach an operational database.

---

## 33. Adversarial review of the six proposals

| Attack scenario | Consequence | Evidence / justification | Severity | Required mitigation and acceptance criterion |
|---|---|---|---|---|
| An unregistered worker or direct SQL role writes after lifecycle pre-check | Retired Phone receives a new message, endpoint or attachment | Current runtime paths use direct SQL; complete external writer/role inventory is absent | Critical | Revoke direct DML; route through gate-first procedures; enumerate every role/worker; direct-SQL negative tests pass for every protected table. |
| Lifecycle transition and writer race | A write passes ACTIVE validation and commits after deprovision | Current code has operation-specific row locks, not a universal lifecycle serialization point | Critical | Gate row lock acquired before dependent locks by every writer; race test proves pre-transition commit or post-transition rejection, never stale commit. |
| Shared operator key revoked with one Phone | Other endpoints/control-plane consumers lose authorization | Key is tenant+AI scoped and has no Phone/endpoint/consumer owner field; raw provisioning is out-of-band | Critical | Complete consumer registry and exact key-generation migration; revoke only after every consumer acknowledges; unrelated consumer health remains PASS. |
| Session/workload closed through attachment association | Another Phone or room participant loses a shared runtime context | Session/workload keys are principal/room/session scoped; attachment is a relation, not ownership | High | Never cascade Phone retirement into session/workload revocation; cross-Phone/shared-session integration test remains PASS. |
| External object deleted through opaque storage_ref | Shared, retained or legally held object is lost | Provider, immutable version, ownership and full references are unproven | Critical | Provider registry, exact version, reference closure, retention/legal-hold check and independent delete approval; ambiguous object always preserved. |
| RETIRED Phone reactivated by ordinary attach/register path | Old identity and stale credentials regain authority | Current schema has no Phone RETIRED state or universal gate | Critical | Terminal lifecycle row plus DB admission enforcement; all ordinary write paths reject RETIRED; recovery creates successor identity. |
| Lifecycle row and existing Phone/endpoint states diverge | API reports disconnected while lifecycle admits writes, or vice versa | Current status domains are separate and no cross-domain lifecycle invariant exists | High | Define one authoritative lifecycle aggregate, transactional transition procedures, consistency constraints/triggers and periodic reconciliation; mismatch fails closed. |
| DB transaction commits but external storage action fails (or vice versa) | Partial deprovision, orphan or irreversible object loss | PostgreSQL and object provider cannot share one atomic transaction | Critical | Durable intent/outbox saga, immutable operation ID, exact-version conditional operation, reconciliation and no deletion on ambiguity. |
| Test teardown points at a writable operational database | Tenant/global identity/history deletion | Existing test helpers accept a database pool and contain direct tenant/resource DELETE statements | Critical | Runner-owned ephemeral cluster lease, non-owner runtime role, no external URL fallback, negative test proves operational-like target is rejected before writes. |
| Rollback after a partially completed multi-step operation | State and external side effects disagree | DB rollback cannot undo provider deletion or already delivered outbox messages | High | Phase receipts and compensating/reconciliation operations; never pretend an external side effect rolled back; retries preserve same operation ID. |
| A shared task/handoff writer does not know the originating Phone | Work continues after Phone retirement or shared Team Room is incorrectly blocked | Current task/handoff routes use principal/session/room and lack Phone ID | Critical | Explicit trusted Phone context or normalized resource association; otherwise declare the operation shared and outside Phone cleanup, and block any requirement to attribute it to a Phone. |
| Successor Phone reuses same AI/tenant tuple | Unique constraint blocks creation or implementer deletes old anchor to make room | `ai_phones` unique `(tenant_id, ai_id)` plus retained Phone anchor | High | New AI identity or separately reviewed generation model; never delete old Phone to bypass uniqueness. |

The adversarial review is a design review, not a test result. No mitigation above has been implemented or verified in production code.

---

## 34. Six-dependency decision and approval request

| Dependency | Decision | Reason |
|---|---|---|
| Persistent lifecycle | **BLOCKED** | State/transition/authority model is now specified, but there is no persistent Phone lifecycle aggregate, lifecycle permission/approver or terminal-state enforcement. Same-AI successor semantics also need schema design. |
| Universal write barrier | **BLOCKED** | Current source has operation-specific locks and direct SQL writers; task/handoff/Team Room paths lack Phone context; external writers and DB roles are not fully inventoried. |
| Operator keys | **BLOCKED** | Correct non-revocation policy is specified, but consumer/provisioning inventory and selective rotation/revocation proof are absent. |
| External storage | **BLOCKED** | Provider, immutable object version, exclusive ownership, legal-hold state and reference closure are unproven. |
| Sessions and workloads | **APPROVED_FOR_IMPLEMENTATION** | Preserve shared sessions/workloads; fence only the exact Phone attachment; use independent security lifecycle for any session/workload revocation. This approval is limited to the non-cascade policy and does not approve Phone lifecycle integration. |
| Disposable test teardown | **BLOCKED** | Runner-owned ephemeral-cluster design is specified, but current helpers accept arbitrary DB pools and perform destructive fixture SQL without independent cluster-ownership proof. |

**Integrated result: BLOCKED — NOT APPROVED FOR IMPLEMENTATION.**

The architecture has a conservative lifecycle model and explicit retention boundaries, but four critical dependencies remain blocked and the shared-resource writer context is incomplete. No implementation, migration, production-code change, operational cleanup, tenant/identity deletion, operator-key revocation, session/workload revocation, 160-race test, 20-handoff test, integration or merge is authorized.

This addendum is documentary only. Any later implementation requires a separate, explicit GPT-6 authorization after the blocked evidence is supplied. A green CI workflow would not, by itself, change this decision.
