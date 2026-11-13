# AI-PHONE-02C — Adversarial Ownership Review and Safe Cleanup Specification

Status: **BLOCKED — NOT APPROVED FOR IMPLEMENTATION OR EXECUTION**  
Immutable implementation TARGET: \`ebb8eeecfea087443c5f3eba7021e96cb5302fe7\`  
Audit branch: \`audit/ai-phone-02c-execution\`  
Harness scenario commit: \`007f1de0c4670b3442cd76980e2a64df143b883d\`  
Catalog workflow commit: \`9cacad2604a7b3208d283accce6edcd2f3d07065\`  
Current document baseline: \`212ceed36bbcc17b168b33d94628caf5acde1a5a\`  
No production code is changed by this specification.

## 1. Executive decision

The current cleanup implementation is unsafe. Three of five independent adversarial scenarios successfully deleted a valid tenant and cascaded deletion into audit/outbox data. The other two were stopped by RESTRICT constraints; they did not prove that cleanup validated ownership.

The existing 10-cycle isolation test is not a valid safety proof: its snapshots show zero endpoints, credentials, attachments, messages and mission links before cleanup, and it treats deletion of the tenant and identity mappings as successful cleanup. It does not cover the required resource graph.

**A safe physical deletion of an AI Phone is not currently specifiable for every state in the applied schema while simultaneously preserving retained children.** In particular, \`phone_messages\` has RESTRICT FKs to \`ai_phones\`; endpoint credentials have RESTRICT FKs to both endpoint and Phone; attachments have multiple RESTRICT parent FKs. If these rows must be retained, their Phone/endpoint parents must remain as anchors. The safe default for the current schema is therefore *deprovision/retire*, not unconditional physical deletion.

No implementation, migration, production-code change, operational cleanup, tenant deletion, 160-race run, 20-handoff run, integration, or merge is authorized by this document.

## 2. Evidence verification (Round 1)

### 2.1 Workflow and commit provenance

| Item | Verified value |
|---|---|
| TARGET | \`ebb8eeecfea087443c5f3eba7021e96cb5302fe7\` |
| Workflow 1 | \`43472486661\`, job \`132047686824\`, branch \`audit/ai-phone-02c-execution\`, head \`007f1de0c4670b3442cd76980e2a64df143b883d\` |
| Workflow 2 | \`43473339618\`, job \`132050673893\`, branch \`audit/ai-phone-02c-execution\`, head \`9cacad2604a7b3208d283accce6edcd2f3d07065\` |
| Current audit branch HEAD before this document update | \`212ceed36bbcc17b168b33d94628caf5acde1a5a\` |
| PostgreSQL | \`16.16\`, \`server_version_num=160016\` |
| Event | \`push\` |

The commit comparison \`007f1de...\` → \`9cacad...\` reports exactly one commit and one changed file: \`.github/workflows/ai-phone-02c-dynamic-audit.yml\` (+59 lines, no deletions). The five-scenario harness source was not changed by that commit. The later \`9cacad...\` → \`212ceed...\` comparison reports only the ownership-contract document changed. Thus the second workflow's \`HARNESS_SHA=9cacad...\` is the workflow/harness checkout commit, while the five scenario test source is inherited unchanged from \`007f1de...\`. This distinction must remain explicit in future reports.

The second run manifest records:
- \`TARGET_SHA=ebb8eeecfea087443c5f3eba7021e96cb5302fe7\`
- \`TARGET_HEAD=ebb8eeecfea087443c5f3eba7021e96cb5302fe7\`
- \`HARNESS_SHA=9cacad2604a7b3208d283accce6edcd2f3d07065\`
- \`WORKFLOW_SHA=9cacad2604a7b3208d283accce6edcd2f3d07065\`
- \`RUN_ID=43473339618\`
- \`EVENT=push\`
- PostgreSQL 16.16 build string as above.

### 2.2 Test results read from both workflow job logs

Both workflows produced the same five scenario outcomes and the same test-target summary:

| Test target | Result |
|---|---:|
| Rust unit tests | 6 passed, 0 failed |
| Main integration test target | 10 passed, 0 failed |
| Admission PostgreSQL target | 3 passed, 0 failed |
| \`ai_phone_02c_network\` | 12 passed, 3 failed |
| Total completed before suite abort | 31 passed, 3 failed |

The three failing assertions are the expected security failures:
- \`ai_phone_02c_round1_case1_valid_tenant_random_ai_only\`
- \`ai_phone_02c_round1_case4_valid_tenant_nonexistent_operator_key\`
- \`ai_phone_02c_round1_case5_tenant_a_ids_from_different_fixtures\`

The two tests reported as passing are negative tests whose expected behavior was a database FK error:
- \`ai_phone_02c_round1_case2_valid_tenant_random_phone_only\`
- \`ai_phone_02c_round1_case3_valid_tenant_incompatible_principal\`

The full suite stopped at the failing network test target. The frontend production build was skipped. \`cargo check\` and legacy migration compatibility completed successfully; neither is security approval.

### 2.3 Five scenario records verified in the original JSONL

The artifact \`ai-phone-02c-dynamic-audit-43473339618.zip\` contains \`audit-evidence/cleanup-isolation-results.jsonl\` with 26 JSONL records. The first six are the combined reproduction plus the five individual scenarios; the remaining 20 are ten A/B cleanup cycles.

| Scenario | Direct artifact observation | Assessment |
|---|---|---|
| Combined reproduction | \`cleanup_returned_ok=true\`; valid tenant no longer exists | FAIL |
| 1. Valid tenant + random nonexistent AI only | tenant true→false; audit 1→0; outbox 1→0; helper returned OK | FAIL |
| 2. Valid tenant + valid AI + random nonexistent Phone | FK \`ai_phones_tenant_binding_fk\` rejected removal of AI/tenant mapping; all snapshots unchanged | BLOCKED BY FK, not ownership proof |
| 3. Valid tenant + incompatible principal | FK \`ai_identity_tenants_tenant_id_fkey\` rejected tenant deletion; all snapshots unchanged | BLOCKED BY FK, not ownership proof |
| 4. Valid tenant + nonexistent operator key | helper returned OK; tenant, principal, Phone and audit/outbox rows disappeared; global AI survived | FAIL |
| 5. Tenant A + AI/Phone/principal IDs from fixtures B/C | helper returned OK; tenant A and its audit/outbox disappeared; B/C fixture records remained | FAIL |

The JSONL fields directly support these conclusions: \`before\`, \`after\`, \`cleanup_error\`, \`cleanup_returned_ok\`, \`scenario\`, \`target_sha\`, and \`harness_sha\`.

### 2.4 Artifact identity and hashes

GitHub's artifact API reported the following digests; local SHA-256 verification of the downloaded ZIPs matched them:

| Workflow | Artifact ID / name | SHA-256 |
|---|---|---|
| \`43472486661\` | \`14059889216\` / \`ai-phone-02c-dynamic-audit-43472486661\` | \`0cdb65486aeacbc0acb84cbd3745182224356135521f667d00204e86c65b466f\` |
| \`43473339618\` | \`14062092447\` / \`ai-phone-02c-dynamic-audit-43473339618\` | \`67b4a4192a80337184c946ae31325da5dd364e85ca90dd469b3c0c955af19d06\` |

The second ZIP contains:
- \`ai-phone-02c-run-context.txt\`
- \`ai-phone-02c-evidence-manifest.txt\`
- \`audit-evidence/cleanup-isolation-results.jsonl\`
- WebSocket evidence JSONL files
- \`ai-phone-02c-full-suite.log\`
- \`ai-phone-02c-postgres-catalog.jsonl\`

The catalog JSONL has 524 lines. The manifest includes SHA-256 entries for the audit harness files, target source files and migrations. The PostgreSQL catalog was captured after the test stage even though the security test target failed.

### 2.5 Ten-cycle test is not sufficient

The 20 records labelled \`cleanup_A_preserves_B\` and \`cleanup_B_complete\` show ten cycles. They are not complete Phone graphs:
- \`phone_endpoints\`, \`phone_endpoint_credentials\`, \`phone_attachments\`, \`phone_messages\`, and \`phone_mission_links\` are zero in the pre-cleanup snapshots.
- No complete populated session/workload/task/handoff/Team Room dependency closure is snapshotted.
- The \`cleanup_B_complete\` records show tenant, principal, Phone and AI mapping counts reaching zero. That test therefore accepts tenant/mapping deletion as completion, contrary to this mission's hard boundary.
- The snapshot does not include complete row contents or all 35 public tables.

These cycles demonstrate only a narrow synthetic A/B behavior under an incomplete fixture graph. They do not prove safe cleanup.

## 3. PostgreSQL catalog re-count and corrections

The actual catalog JSONL contains **35 public tables**, not 34:
- 34 application tables.
- 1 infrastructure table: \`_sqlx_migrations\`.

The constraint record counts are:
- 35 PRIMARY KEY constraints.
- 28 UNIQUE constraints.
- 52 CHECK constraints.
- 85 FOREIGN KEY constraints.
- 33 FK actions \`ON DELETE CASCADE\`.
- 29 FK actions \`ON DELETE RESTRICT\`.
- 18 FK actions \`NO ACTION\`.
- **5 FK actions \`ON DELETE SET NULL\`**, not four.

The five SET NULL constraints are:
1. \`ai_phones_current_task_mission_tenant_fk\` — nulls only \`current_task_id\` and \`current_mission_id\`.
2. \`task_handoffs_from_execution_fk\`.
3. \`task_handoffs_from_session_fk\`.
4. \`task_handoffs_to_execution_fk\`.
5. \`task_handoffs_to_session_fk\`.

The previous contract's “34 public tables” and “four SET NULL constraints” statements are incorrect. The earlier table matrix also omitted \`_sqlx_migrations\`. The matrix below accounts for all 35 catalog tables.

Seven non-internal triggers were captured. No non-internal DELETE trigger was found. Trigger definitions are in the catalog artifact.

## 4. Ownership classes and non-negotiable semantics

The required classifications are:

- \`FIXTURE_EXCLUSIVE\`: a *row-level* classification only. The exact row was created by this fixture, appears in a signed/immutable manifest produced at creation time, and every parent/ownership edge matches. No table is automatically fixture-exclusive.
- \`PHONE_OWNED\`: the row is structurally scoped to one exact Phone by enforced key relationships. This does not override retention, child references, or fixture-provenance requirements.
- \`TENANT_SHARED\`: tenant-scoped, AI/tenant-scoped, room-scoped, principal-scoped or otherwise available to more than one Phone/workflow.
- \`GLOBAL\`: global identity, permission catalog, replay protection or infrastructure state.
- \`RETAINED\`: security, audit, outbox, durable message, lease, handoff or governance history that must not be erased by Phone cleanup.
- \`OWNERSHIP_UNPROVEN\`: soft reference, missing FK, external object, incomplete provenance, unknown consumer or cross-resource relationship without a complete proof.

A row may be tagged \`FIXTURE_EXCLUSIVE\` in a manifest only if the underlying table's default classification and the row's actual relationships permit it. This does not permit deleting a \`GLOBAL\`, \`TENANT_SHARED\`, or \`RETAINED\` row. An ID supplied by a caller is not evidence of fixture provenance. An FK proves referential integrity, not fixture ownership.

**Tenant is never a Phone cleanup resource.** The tenant may be locked and validated as a scope anchor; it is never added to the delete set. AI identities, AI/tenant bindings, principals and audit/history are also preserved by default.

## 5. Complete 35-table ownership matrix (Rounds 2 and 4)

“Creator” below identifies the observed application writer/provisioning path where the reviewed TARGET source exposes one. The catalog itself cannot establish who created an existing row. Where a control-plane or external writer is not present in the reviewed runtime source, that uncertainty is stated explicitly rather than guessed.

| Table / primary key | Observed creator or writer | Logical owner and tenant boundary | Class / shared? | Survives Phone deletion? / tenant deletion? | FK guarantee and soft references | Dependencies, cascade effects and safe policy |
|---|---|---|---|---|---|---|
| \`_sqlx_migrations\` / \`version\` | SQLx migration runner | Database deployment metadata; no tenant | \`GLOBAL\`; infrastructure | Yes / yes | No tenant FK; not application data | Never include in fixture manifest or cleanup. Preserve deployment history. |
| \`tenants\` / \`tenant_id\` | Tenant provisioning/control plane and test fixtures; no Phone runtime creator | Root security boundary | \`TENANT_SHARED\`; shared by all tenant resources | Yes / No; tenant is the deletion root | Parent FK to rooms, principals, audit, outbox and handoffs; AI tenant binding is RESTRICT | Direct cascades can erase large tenant graph and retained history. Lock only for validation; never delete. |
| \`ai_identities\` / \`ai_id\` | \`phone.rs\` identity registration path | Global AI identity; may bind to multiple tenants | \`GLOBAL\`; cross-tenant shared | Yes / yes | Referenced by identity mappings, Phones, messages and other scope rows with RESTRICT | Never delete or infer ownership from tenant/Phone ID. |
| \`ai_identity_principals\` / \`(ai_id, tenant_id, principal_id)\` | \`phone.rs\` identity registration/binding | AI-to-principal mapping within tenant; unique \`(ai_id, tenant_id)\` | \`TENANT_SHARED\`; shared binding | Yes / tenant deletion is blocked until mapping is removed | FK to global AI and composite tenant/principal; trigger requires enabled AI principal | Not Phone-owned. Preserve by default; never remove to make tenant deletion succeed. |
| \`ai_identity_tenants\` / \`(ai_id, tenant_id)\` | \`phone.rs\` identity registration/binding | Global AI-to-tenant binding | \`TENANT_SHARED\`; cross-Phone binding | Yes / tenant deletion is blocked while binding exists | FKs to AI, AI/principal mapping and tenant, all RESTRICT | Never delete in Phone cleanup. It is an explicit boundary relation. |
| \`principals\` / \`principal_id\` | Tenant identity provisioning; fixture setup in reviewed tests | Tenant-scoped identity | \`TENANT_SHARED\` | Yes / No; tenant CASCADE | Tenant FK CASCADE; inbound relations include participant/session/permission/workload and Team Room rows; some principal FKs are single-column | Deletion can cascade many resources and may be blocked by Phone RESTRICT. Preserve; no principal-wide cleanup. |
| \`participants\` / \`participant_id\` | Tenant/room provisioning and fixture setup | Tenant participant identity | \`TENANT_SHARED\` | Yes / No; cascades from principal | FK to tenant+principal CASCADE; inbound memberships/workloads CASCADE; Phone attachment RESTRICT; task/handoff NO ACTION | Never delete as Phone child. Preserve unless a separate tenant lifecycle explicitly authorizes it. |
| \`rooms\` / \`room_id\` | Room provisioning/control plane and fixture setup | Tenant-scoped collaboration room | \`TENANT_SHARED\` | Yes / No; tenant CASCADE | Tenant FK CASCADE; inbound sessions, memberships, workloads, tasks, handoffs, channels, decisions, messages, attachments, WS tickets and permissions | A room delete is a broad graph deletion. Never delete from Phone cleanup. |
| \`sessions\` / \`session_id\` | \`auth.rs\` verified session exchange | Tenant + room + principal + workload context | \`TENANT_SHARED\` | Yes / No; room/principal cascades | Composite tenant/principal and tenant/room FKs CASCADE; Phone attachments RESTRICT; handoff refs SET NULL; WS tickets/workloads cascade | Revoke/close via lifecycle path if needed; preserve session history and dependent audit. Do not delete from Phone cleanup. |
| \`room_memberships\` / \`membership_id\` | Room join/presence path in communication service and fixtures | Tenant + room + participant + principal + session | \`TENANT_SHARED\` | Yes / No; room/participant/principal cascades | Composite room/participant/principal FKs CASCADE; session NO ACTION | Shared membership state. Do not delete based on Phone ID. |
| \`workload_identities\` / \`workload_id\` | Trusted workload/OIDC provisioning; \`auth.rs\` reads and updates last-seen/revocation | Tenant + principal + issuer/subject | \`TENANT_SHARED\` | Yes / No; principal CASCADE | FK tenant/principal CASCADE; external issuer/subject semantics are application-level | Revoke through identity lifecycle; preserve identity history; never infer Phone ownership. |
| \`workload_executions\` / \`execution_id\` | \`auth.rs\` session exchange | Tenant + room + participant + principal + session | \`TENANT_SHARED\` | Yes / No; session/participant/room cascades | Composite FKs to session, participant and room CASCADE; Phone attachments RESTRICT; task owner NO ACTION; handoff refs SET NULL | Shared execution context. Preserve unless independently approved lifecycle policy; Phone cleanup must not delete it. |
| \`principal_permissions\` / \`permission_id\` | Permission provisioning/control plane and fixtures | Principal/room permission grant | \`TENANT_SHARED\` | Yes / No; principal/room cascades | Principal and room FKs CASCADE; permission catalog NO ACTION | Revoke exact grant through authorization lifecycle; no wildcard deletion. |
| \`permission_catalog\` / \`permission\` | Migration seed / schema deployment | Global authorization vocabulary | \`GLOBAL\` | Yes / yes | Referenced by principal_permissions with NO ACTION | Never delete from Phone cleanup. |
| \`auth_token_replays\` / \`(issuer, token_jti)\` | \`auth.rs\` verified token exchange; expiry sweeper is global | Global issuer/JTI replay protection | \`GLOBAL\` | Yes / yes | No inbound or outbound FKs; no tenant column | Never delete in Phone cleanup. Existing expiry maintenance is a separate global TTL policy. |
| \`security_audit_events\` / \`security_event_id\` | \`auth.rs\`, \`handoffs.rs\` | Security history; tenant/room/principal are metadata references | \`RETAINED\` | Yes / yes; rows can outlive tenant | No inbound/outbound FKs; tenant_id, room_id and principal_id are soft refs | Preserve. Tenant deletion will not cascade to these rows and can leave orphaned scope identifiers. |
| \`audit_events\` / \`audit_event_id\` | \`phone.rs\` Phone event recorder | Tenant audit history; resource IDs are logical references | \`RETAINED\` | Yes / No; direct tenant CASCADE | Tenant FK CASCADE; room_id, actor_principal_id and resource_id are soft refs | Preserve; tenant deletion erases history. Phone deletion alone does not delete these rows. |
| \`outbox_events\` / \`outbox_event_id\` | \`phone.rs\`, \`communication.rs\`, \`tasks.rs\`, \`handoffs.rs\`, \`main.rs\` | Tenant event stream and downstream delivery history | \`RETAINED\` | Yes / No; direct tenant CASCADE | Tenant FK CASCADE; aggregate_type/aggregate_id are soft refs; INSERT trigger emits NOTIFY | Preserve pending and published events. Never delete tenant to clear Phone. |
| \`team_tasks\` / \`task_id\` | \`tasks.rs\` task creation | Tenant + room task; owner is principal/participant/session/execution tuple | \`TENANT_SHARED\` | Yes / No; room CASCADE | Composite owner FKs are NO ACTION; room FK CASCADE; Phone mission link references task RESTRICT | Never delete task as a Phone side effect. Lease events and handoffs depend on it. |
| \`task_lease_events\` / \`lease_event_id\` | \`tasks.rs\`, \`handoffs.rs\` | Durable lease/fencing history | \`RETAINED\` | Yes / No when task is removed through room cascade | FK to task CASCADE; tenant/room/principal/participant/session references are soft refs | Preserve. Task deletion cascades lease history; never use task deletion as cleanup. |
| \`task_handoffs\` / \`handoff_id\` | \`handoffs.rs\` | Durable task transfer history involving two principals/participants/sessions | \`RETAINED\` | Yes / No; direct tenant and room/task cascades | Task, tenant and room FKs CASCADE; participant/principal NO ACTION; session/execution refs SET NULL | Preserve. Deleting tenant, room or task erases handoff history; deleting session/execution can null history fields. |
| \`team_room_channels\` / \`channel_id\` | \`communication.rs\` | Tenant + room collaboration channel | \`TENANT_SHARED\` | Yes / No; room CASCADE | Room FK CASCADE; creator principal composite FK NO ACTION | Shared channel; channel deletion cascades members, reads, messages and attachment refs. Never delete from Phone cleanup. |
| \`team_room_channel_members\` / \`(channel_id, principal_id)\` | \`communication.rs\` | Channel membership for principal | \`TENANT_SHARED\` | Yes / No; channel/principal cascades | Channel and principal FKs CASCADE; added_by_principal_id single-column NO ACTION, not a same-tenant composite proof | Preserve unless exact membership lifecycle is separately approved. |
| \`team_room_messages\` / \`message_id\` | \`communication.rs\` | Durable tenant/room/channel conversation history | \`RETAINED\` | Yes / No; room/channel cascades | Room/channel FKs CASCADE; sender and parent-message FKs NO ACTION; sender FK is not tenant-composite | Preserve. Tenant/room/channel deletion can erase conversation history. |
| \`team_room_message_reads\` / \`(channel_id, principal_id)\` | \`communication.rs\` | Per-principal read state | \`TENANT_SHARED\` | Yes / No; channel/principal cascades | Channel and principal single-column FKs CASCADE; no composite same-tenant ownership proof | Ephemeral/shared state; not a Phone resource. |
| \`team_room_decisions\` / \`decision_id\` | \`communication.rs\` | Room governance/decision history | \`RETAINED\` | Yes / No; room CASCADE | Room FK CASCADE; author and reference-message NO ACTION; mission_id and participants[] are soft refs | Preserve. Soft mission/participant references require explicit policy; room/tenant deletion erases the record. |
| \`team_room_attachment_refs\` / \`attachment_id\` | \`communication.rs\` | Room/channel attachment metadata and external storage reference | \`OWNERSHIP_UNPROVEN\` | Yes / No; room/channel cascades | Channel and room FKs CASCADE; message/uploader NO ACTION; task_id and storage_ref are soft/external refs | Preserve. Never delete external storage object or reference without an independent storage ownership and retention proof. |
| \`team_room_ws_tickets\` / \`ticket_hash\` | \`communication.rs\` | Room/session ephemeral ticket; principal association | \`OWNERSHIP_UNPROVEN\` | Yes / No; room/session cascades | Room and session FKs CASCADE; principal_id is a soft reference | Exact ticket expiry/consumption is separate lifecycle. Do not infer Phone ownership. |
| \`ai_phones\` / \`ai_phone_id\` | \`phone.rs\` Phone creation/ensure path | One Phone row scoped by tenant + global AI | \`PHONE_OWNED\` | Parent anchor for Phone children; cannot be removed while retained RESTRICT children remain / tenant removal is blocked by AI binding until this graph is resolved | AI and AI/tenant binding FKs RESTRICT; current task/mission composite FK SET NULL on two columns | Candidate for exact Phone lifecycle only. Preserve if messages, credentials, attachments or other retained/unproven children depend on it. Never delete tenant or AI. |
| \`phone_endpoints\` / \`endpoint_id\` | \`phone.rs\` endpoint enrollment | Exact Phone + tenant + AI endpoint | \`PHONE_OWNED\` | No while credentials/attachments remain; endpoint is a credential parent / not a tenant cascade row | Phone and composite Phone/tenant/AI FKs RESTRICT; endpoint lifecycle trigger revokes active credentials on DETACHED/REVOKED | Deprovision by status transition. Physical deletion only after every child and retention decision is proven. |
| \`phone_endpoint_operator_keys\` / \`operator_key_id\` | Trusted control plane out-of-band (migration comment); no Phone service insert path identified | AI + tenant authorization scope, not endpoint/Phone scope | \`TENANT_SHARED\` | Yes / tenant deletion blocked by mapping until key removed | Only FK is AI/tenant binding RESTRICT; no FK to Phone or endpoint | Shared authorization credential. Revoke exact key via control plane; never delete because a Phone is removed. |
| \`phone_endpoint_credentials\` / \`credential_id\` | \`phone.rs\` endpoint enrollment/rotation | Exact endpoint + Phone + tenant + AI; security-sensitive credential history | \`RETAINED\` | Must remain with endpoint and Phone if retention is required / no direct tenant cascade, but blocks parent deletion | Composite endpoint/Phone/tenant/AI and Phone FKs RESTRICT; lifecycle trigger revokes active credential on endpoint detach/revoke | Revoke via endpoint status trigger; retain hashes/history by default. If retained, endpoint and Phone rows remain as anchors. Delete only under explicit retention approval. |
| \`phone_attachments\` / \`attachment_id\` | \`phone.rs\` attach/transfer lifecycle | Phone/endpoint binding to principal, participant, room, session and workload execution | \`OWNERSHIP_UNPROVEN\` until manifest and every parent tuple match | It is a Phone child but cross-resource parents remain / can block parent deletion | Seven composite/scope FKs use RESTRICT; references exact Phone, endpoint, AI mapping, principal, participant, session and workload execution | Prefer FENCED/DETACHED lifecycle and retain evidence. Exact physical delete only if fixture-exclusive and no policy/external consumer; never delete its parent session/workload. |
| \`phone_messages\` / \`message_id\` | \`phone.rs\` send path | Durable Phone message; sender principal and recipient global AI | \`RETAINED\` | Message row cannot outlive Phone under current FK; Phone cannot be deleted while message is retained / no direct tenant cascade, but sender principal may block tenant cascade | RESTRICT FKs to Phone, Phone/tenant/recipient AI tuple, global AI identity and tenant/principal sender | Preserve message and Phone anchor. Physical Phone deletion requires an explicit message retention/archive decision; current schema cannot both delete Phone and retain its messages. |
| \`phone_mission_links\` / \`ai_phone_id\` | \`phone.rs\` and \`handoffs.rs\` mission link/upsert | Phone-to-Team-Task relationship row; task itself is not Phone-owned | \`PHONE_OWNED\` relationship only when manifest-proven | Link cannot outlive Phone; task must survive / task and room may not survive tenant deletion | RESTRICT FKs to Phone and Team Task including tenant/room tuple | Exact link may be removed only with provenance; never delete task, lease events or handoffs. |

### 5.1 Important ownership conclusions

1. \`phone_endpoint_operator_keys\` are scoped to \`(ai_id, tenant_id)\`, not to a Phone or endpoint. The table has no Phone/endpoint FK. They are shared authorization material.
2. \`ai_identity_principals\` and \`ai_identity_tenants\` are identity/tenant boundary mappings, not Phone children.
3. \`phone_messages\` and \`phone_endpoint_credentials\` have RESTRICT dependencies that conflict with physical parent deletion when their history is retained.
4. \`phone_attachments\` bind several independently owned resources. Their Phone FK does not establish fixture-exclusive ownership of the principal, participant, session or execution.
5. \`phone_mission_links\` is a Phone-to-task relationship. The link may be Phone-owned; the task, lease history and handoff history are not.
6. \`security_audit_events\` and \`auth_token_replays\` have no FKs. They survive tenant/Phone deletion and must not be swept by inferred scope.
7. A Phone cleanup may not delete a tenant, room, principal, AI identity, AI/tenant mapping, Team Task, global replay state, audit/security history, outbox history, or Team Room governance history.

## 6. Cascade and FK action matrix (Round 2)

### 6.1 All 33 CASCADE edges

The catalog reports 33 FK constraints with \`ON DELETE CASCADE\`. They are grouped by referenced parent:

| Referenced parent | Cascading child tables |
|---|---|
| \`tenants\` | \`rooms\`, \`principals\`, \`audit_events\`, \`outbox_events\`, \`task_handoffs\` |
| \`principals\` | \`participants\`, \`principal_permissions\`, \`room_memberships\`, \`sessions\`, \`team_room_channel_members\`, \`team_room_message_reads\`, \`workload_identities\` |
| \`participants\` | \`room_memberships\`, \`workload_executions\` |
| \`rooms\` | \`principal_permissions\`, \`room_memberships\`, \`sessions\`, \`task_handoffs\`, \`team_room_attachment_refs\`, \`team_room_channels\`, \`team_room_decisions\`, \`team_room_messages\`, \`team_room_ws_tickets\`, \`team_tasks\`, \`workload_executions\` |
| \`team_tasks\` | \`task_handoffs\`, \`task_lease_events\` |
| \`team_room_channels\` | \`team_room_attachment_refs\`, \`team_room_channel_members\`, \`team_room_message_reads\`, \`team_room_messages\` |
| \`sessions\` | \`team_room_ws_tickets\`, \`workload_executions\` |

Some rows have multiple cascading parent FKs; this is why the child may be deleted by more than one path. The list above groups all 33 actual FK constraints, not merely unique child table names.

### 6.2 Tenant deletion is never a cleanup primitive

Direct tenant cascades erase:
- rooms and their sessions/memberships/workloads/tasks/channels/messages/decisions/attachments/WS tickets;
- principals and their participants, permissions, sessions, workload identities, memberships and Team Room memberships/reads;
- audit_events and outbox_events directly;
- task_handoffs directly and through task/room paths.

Other RESTRICT mappings may first block tenant deletion. Removing those mappings to “unblock” the delete does not make the operation safe; it opens the destructive cascade paths above.

### 6.3 SET NULL, RESTRICT and NO ACTION

- Five SET NULL constraints exist, enumerated in Section 3. The Phone composite pointer nulls only \`current_task_id\` and \`current_mission_id\`; it does not null Phone \`tenant_id\`.
- 29 RESTRICT FKs protect cross-resource Phone rows and AI/tenant bindings.
- 18 NO ACTION FKs protect relationships such as task ownership, parent messages, channel creator and some Team Room actors.
- FK enforcement is not fixture provenance. A successful FK check does not prove the referenced row belongs to the same fixture.
- A failed statement may follow earlier statements in the transaction; the whole transaction must roll back. A successful DELETE can still trigger unrelated cascades.

## 7. Independent adversarial review findings (Round 3)

| ID | Document location | Affected relation | Risk and evidence | Required correction |
|---|---|---|---|---|
| A-01 | Previous §2 catalog count | Entire public schema | Previous contract said 34 public tables. Artifact has 35, including 34 application tables plus \`_sqlx_migrations\`. | Correct count; inventory all 35, including infrastructure table. |
| A-02 | Previous §2 SET NULL count | Phone and task handoff pointers | Previous contract said four SET NULL FKs. Catalog contains five: one Phone composite pointer and four handoff session/execution pointers. | Correct count and enumerate all five. |
| A-03 | Previous §1 ownership classes and §3 matrix | Every table | Previous labels did not use the six required enum values and did not distinguish row-level fixture provenance from table-level default ownership. | Use exactly the six classifications in §4; assign FIXTURE_EXCLUSIVE only to exact manifest rows. |
| A-04 | Previous §3 Phone rows and §8 delete order | \`phone_messages\` → \`ai_phones\`; credentials → endpoints → Phone | Previous text says preserve message/credential history but suggests deleting Phone/endpoint after children are “resolved”. Actual RESTRICT FKs make physical parent deletion impossible while retained children remain. | Define deprovision/retire with retained parent anchors as default. Hard delete only after explicit retention approval and zero retained children. |
| A-05 | Previous §3 table matrix | All 35 tables | Matrix omitted creator, logical owner, tenant boundary, shared status, survival after Phone/tenant removal, soft refs and fail-safe condition as separate answers. It also omitted \`_sqlx_migrations\`. | Replace with the 35-row matrix in §5. |
| A-06 | Previous §2/§6 soft-reference list | Audit, outbox, task lease, Team Room and external storage | Prior list omitted \`task_lease_events.participant_id\` and did not fully spell out single-column principal FKs that fail to prove same-tenant ownership; external storage_ref was not connected to a concrete preserve rule. | Treat every listed scalar/array/external reference as OWNERSHIP_UNPROVEN; require application-level consumer audit. |
| A-07 | Previous §7 fixture manifest | All manifest resources | A list of IDs and expected tuples is insufficient if the manifest can be caller-authored or altered after creation. No persisted fixture_id exists in TARGET. | Generate manifest at fixture creation, canonicalize, hash/sign, bind to TARGET/harness/run, reject tampering or partial manifests. |
| A-08 | Previous §8 lock proposal | All Phone writers and soft-reference writers | Locking only currently known parent rows cannot prevent a new soft reference from being inserted by a writer that does not acquire the same lock. Catalog has no FK to serialize those writes. | Require a shared advisory-lock protocol across every writer or preserve/abort where that protocol cannot be proven. |
| A-09 | Previous §9 test evidence | Ten-cycle A/B cleanup | JSONL shows zero Phone descendants in the 10-cycle snapshots; cleanup_B_complete reports tenant and identity mappings deleted. This tests tenant deletion, not safe Phone-only cleanup. | Replace with two fully populated fixtures and all-table snapshots; tenant must remain after either cleanup. |
| A-10 | Previous §3 operator-key row | \`phone_endpoint_operator_keys\` | Table has only AI/tenant binding FK and no Phone/endpoint FK; migration says trusted control plane provisions keys out-of-band. Treating the key as Phone-owned risks revoking another endpoint/operator. | Classify TENANT_SHARED; exact revoke only through its control-plane lifecycle, not Phone cleanup. |
| A-11 | Previous §3 security/replay rows | \`security_audit_events\`, \`auth_token_replays\` | Both tables have no inbound/outbound FK. Tenant/Phone deletion will not cascade to them; a tenant_id field is only a soft reference. | Preserve; never sweep by tenant or Phone ID. |
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

Unproven items are not assumptions. They remain \`OWNERSHIP_UNPROVEN\` and cannot be deleted automatically.

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
8. Verify the Phone tuple \`(ai_phone_id, tenant_id, ai_id)\`, all endpoint tuples, principal kind/enabled state, AI/principal tuple, participant/principal/tenant tuple, session/room/principal tuple, execution/session/participant tuple and every other parent tuple.
9. For every manifest ID, query by exact primary key and compare the complete row identity and parent tuple. Reject unknown, missing, duplicate, cross-fixture or unlisted IDs.
10. Enumerate all inbound FK references from \`pg_constraint\` and inspect every known soft-reference consumer, JSON/UUID array and external storage reference. Compare dependent row sets to the manifest.
11. Classify each candidate. Only exact \`FIXTURE_EXCLUSIVE\` or eligible \`PHONE_OWNED\` rows may enter a physical delete set. \`TENANT_SHARED\`, \`GLOBAL\`, \`RETAINED\` and \`OWNERSHIP_UNPROVEN\` rows are excluded.
12. If any retained/unproven child blocks deletion of a parent, preserve the parent as an anchor and switch to DEPROVISION/RETIRE. Do not delete the parent to force FK success.
13. Build and persist an explicit delete/retire plan before the first mutation. The plan lists exact table, PK, expected count, action and dependency proof.
14. Revalidate row counts and ownership tuples immediately before each permitted mutation.
15. Perform exact-ID lifecycle updates/deletes in FK-safe order; never delete by tenant, AI, prefix, label, wildcard, guessed association or unbounded discovery query.
16. Verify postconditions and all protected snapshots before commit.
17. Persist a unique cleanup receipt atomically with the database transaction (or use an equivalent transactional harness journal). Commit only after receipt and postconditions succeed.
18. On any mismatch, unexpected dependent, lock failure, row-count mismatch, soft-reference uncertainty, serialization failure beyond retry policy or timeout: rollback and return a structured failure. Scope must never be broadened.

### 8.4 Pseudocode

\`\`\`text
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
\`\`\`

### 8.5 SQL transaction skeleton (illustrative, not executable production SQL)

\`\`\`sql
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
\`\`\`

The SQL is a design skeleton, not an executable script. The final SQL must be derived from the exact manifest schema and tested in disposable PostgreSQL before any implementation authorization.

### 8.6 Physical delete order, only if explicitly approved

Physical deletion is not the default. It is allowed only if every row is manifest-proven, every retention policy explicitly permits deletion, and the exact FK/soft-reference closure is empty outside the delete set.

Candidate child-first order:
1. Exact Phone attachment rows, only if no external or retention dependency exists.
2. Exact endpoint credential rows, only if security retention explicitly permits deletion.
3. Exact endpoint rows.
4. Exact Phone message rows, only if message retention explicitly permits deletion.
5. Exact Phone mission-link row; the referenced Team Task is never deleted.
6. Exact \`ai_phones\` row, only after all RESTRICT children are gone.
7. No deletion of \`ai_identity_tenants\`, \`ai_identity_principals\`, global AI identity, principal or tenant as part of Phone cleanup.

If credentials/messages/attachments are retained, stop before deleting their parents and perform deprovision only. There is no “force delete” fallback.

## 9. Concurrency, locking and failure semantics (Round 6)

### 9.1 Required protections

- **Advisory lock:** one transaction-scoped lock per \`ai_phone_id\`; every Phone, endpoint, credential, attachment, message, mission-link and soft-reference writer must acquire it before mutation.
- **Row locks:** lock tenant scope, AI/tenant mapping, AI/principal mapping, Phone, endpoint and all exact parent/dependent rows in a deterministic table/PK order.
- **FK protection:** \`FOR UPDATE\` on referenced parent rows conflicts with FK insert key-share locks and prevents new FK children from racing past the validation window.
- **SERIALIZABLE:** use serializable isolation for predicate/read-set validation, but do not treat it as a substitute for the shared advisory lock on soft-reference writers.
- **Soft references:** because there is no FK lock for JSON/array/external references, require the same advisory lock in every writer or preserve the resource and abort.
- **Ownership changes:** tenant remapping, Phone transfer, endpoint replacement/revocation, principal disablement/mapping changes and mission/task changes must take the same lock and increment/validate fencing/version state.
- **Stable lock order:** all code paths must use the same global order; never lock arbitrary child rows before the Phone anchor.

The reviewed TARGET contains row-locking patterns in Phone lifecycle operations, but the audit has not proven a universal advisory-lock protocol across every writer, especially out-of-band operator-key provisioning and soft-reference writers. This is a blocker, not a presumed capability.

### 9.2 Timeouts, deadlocks and retries

| Condition | Required behavior |
|---|---|
| Ownership mismatch, unknown ID, missing manifest row, extra dependent, bad signature | Roll back immediately; no retry; no delete |
| \`40001\` serialization failure | Roll back; bounded retry of the identical operation/manifest only; never widen scope |
| \`40P01\` deadlock | Roll back; at most a small bounded retry after verifying global lock order; repeated deadlock is FAIL_CLOSED |
| Lock timeout / lock unavailable | Roll back and return RETRYABLE_BLOCKED; do not skip the lock |
| Statement timeout | Roll back; return RETRYABLE_BLOCKED; no partial cleanup |
| Row-count mismatch or unexpected FK behavior | Roll back; emit exact evidence; no retry without investigation |
| Lost response after commit | Resolve through unique operation receipt; do not blindly repeat deletes |
| Missing idempotency receipt and missing manifest rows | Treat as inconsistent/unknown; abort, do not infer that cleanup already succeeded |

### 9.3 Idempotency

Use a unique \`operation_id\` plus manifest digest and result receipt written atomically with the database transaction. A retry with the same operation ID and same digest returns the stored result. Same operation ID with a different digest is rejected. The current schema/catalog has no dedicated cleanup receipt table; this is a design dependency requiring an approved harness-only journal or later schema design. It must not be improvised during implementation.

## 10. Atomicity and isolation test plan (Round 7)

No tests in this section are authorized to run as operational cleanup yet. Tests must use a disposable PostgreSQL 16 database and two complete, independently populated fixtures.

### 10.1 Two-fixture construction

Fixture A and fixture B must use:
- distinct tenant IDs;
- the same global \`ai_id\` to prove cross-tenant global identity sharing;
- distinct tenant-scoped principals, participants, rooms, sessions and workload identities/executions;
- distinct Phone IDs, endpoints, operator keys, credentials, attachments and messages;
- distinct Team Tasks, lease events and handoffs;
- Team Room channels, memberships, messages, decisions, attachment refs and WS tickets;
- audit_events, security_audit_events and outbox_events with references to both fixtures;
- soft-reference records and external-storage test objects with known ownership labels.

The unique \`(tenant_id, ai_id)\` Phone key means two Phones sharing one AI must be in different tenants. Do not try to create two Phones for the same AI in one tenant.

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
- \`run_id\`, \`target_sha\`, \`harness_sha\`, \`postgres_version\`;
- \`fixture_id\`, \`operation_id\`, \`manifest_digest\`;
- \`scenario\`, \`attack\`, \`expected_result\`, \`actual_result\`;
- exact resource table/PK and expected/observed ownership tuple;
- ownership class and proof reference;
- lock acquisition result, transaction isolation, SQLSTATE and retry count;
- pre/post row counts and snapshot hashes for all 35 tables;
- protected row IDs changed (must be empty);
- \`committed_delete_count\`, \`committed_update_count\`, \`rollback_confirmed\`;
- final \`PASS\`, \`FAIL\` or \`BLOCKED\`.

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
3. All \`OWNERSHIP_UNPROVEN\` resources are preserved and cause fail-closed behavior if they block a parent deletion.
4. Every writer must participate in the same advisory-lock protocol or the relevant resource remains non-deletable.
5. A complete two-fixture test suite with full 35-table snapshots must pass before any production implementation.
6. Any later physical-delete policy must define retention/archive semantics for messages, credentials, attachments and their parent anchors.

Until that approval is explicit, do not implement the algorithm, modify production code or migrations, execute cleanup operationally, run the 160 races or 20 handoffs, run integration, or merge.

**Final status: BLOCKED.** The current evidence demonstrates the vulnerability and the specification now identifies the principal schema/contract gaps, but ownership closure, retention semantics, writer-wide concurrency guarantees and complete isolation tests remain unproven.
