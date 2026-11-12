# AI-PHONE-07 — RLS Production Gate Report

## Final classification

`RLS_PRODUCTION_GATE_PARTIAL`

**Production status: `PRODUCTION_BLOCKED_BY_RLS` and `PRODUCTION_BLOCKED_BY_POP`.**

This mission established the exact baseline, created the isolated mission branch, and completed a read-only RLS architecture inventory. It deliberately did **not** activate RLS or add an RLS migration. The current application connection model cannot safely support fail-closed RLS without first refactoring authentication, request transactions, workers, WebSocket listeners, and CI database roles. Enabling policies before those changes would break legitimate flows or encourage a bypass role/policy.

No PoP, voice, PSTN, WebRTC, or additional phones were implemented.

## 1. Rebaseline

- Repository: `vidift-max/tokensaver-ai`
- Source branch: `mission/ai-phone-06-p1-hardening`
- Source HEAD: `0ea7f2cd0f5054ef97bd41ca9db25c998176512f`
- New branch: `mission/ai-phone-07-rls-production-gate`
- Initial SHA of new branch: `0ea7f2cd0f5054ef97bd41ca9db25c998176512f`
- Base architectural SHA: `d1f2bd764361b14af0604104db8e670ee6f4f361`
- Existing migrations: `0001`–`0013`
- Run #92: `43319511695`, conclusion `success`, exact head SHA `0ea7f2cd0f5054ef97bd41ca9db25c998176512f`

Historical PR state at rebaseline:
- #7: OPEN, DRAFT, unmerged
- #8: OPEN, DRAFT, unmerged
- #9: OPEN, DRAFT, unmerged
- #10: OPEN, DRAFT, unmerged

No historical PR was modified. No PR was created or merged. No production database was accessed.

## 2. Read-only architecture inventory

### Connection and transaction model

- SQLx `PgPool` is created by `connect_pool(database_url)` in `services/ai-team-room/src/lib.rs`, with up to 20 pooled connections.
- The service passes the same pool through the HTTP service, AI-PHONE router, Team Room modules, authentication and task/handoff functions.
- Runtime SQL uses both explicit `pool.begin()` transactions and direct `fetch_one/fetch_optional/fetch_all(pool)` calls.
- `SET LOCAL app.tenant_id` is not currently present.
- No centralized tenant-scoped transaction helper exists.
- Authentication currently resolves the tenant from a token hash in `authenticate_session`; that lookup itself runs before any tenant context exists.
- OIDC exchange resolves a tenant by issuer/subject plus room and creates the local session in one transaction.
- WebSocket delivery uses a dedicated `PgListener::connect_with(&pool)` connection and multiple direct pool queries; listener/notification processing has no tenant transaction context.
- Task recovery is callable as `recover_expired(pool, room_id)` without an authenticated identity or explicit tenant argument.
- CI currently creates `POSTGRES_USER=ai_phone_ci`. The official PostgreSQL image creates that initial role as a superuser. Thus the current CI role is not a valid non-owner/non-BYPASSRLS application-role test.
- The current migration function runs SQLx migrations against a supplied pool. The application and migration connection roles are not separated by the existing service bootstrap.

### Table matrix

| Table | Tenant scope | Writer | Reader | Worker / async path | RLS requirement |
|---|---|---|---|---|---|
| tenants | Global control-plane root | Provisioning/admin | Auth bootstrap | Migration/admin | Explicitly exclude from tenant RLS or expose through narrowly scoped control-plane functions |
| rooms | Direct tenant_id | Admission/admin | Auth, Team Room, AI-PHONE | Recovery / WS aggregate validation | Tenant RLS |
| principals | Direct tenant_id | Identity provisioning | Auth, permissions, Team Room | None identified | Tenant RLS |
| participants | Direct tenant_id | Identity provisioning | Auth, presence, admission | None identified | Tenant RLS |
| sessions | Direct tenant_id | OIDC exchange, logout, rotation | Auth, heartbeat, WS, AI-PHONE | WS revalidation | Tenant RLS; auth bootstrap needs narrow resolution |
| room_memberships | Direct tenant_id | Admission, leave, presence | Auth, Team Room, WS, AI-PHONE | WS delivery | Tenant RLS |
| audit_events | Direct tenant_id | Admission/leave and service mutations | Audit/operations | Potential audit consumer | Tenant RLS; append-only write grant preferred |
| outbox_events | Direct tenant_id | AI-PHONE, Team Room, task/handoff transactions | WS dispatcher / event consumers | Yes; listener-driven | Tenant RLS plus dedicated scoped worker design |
| workload_executions | Direct tenant_id | OIDC exchange, heartbeat, close/revoke | Auth, WS, AI-PHONE | WS revalidation | Tenant RLS |
| workload_identities | Direct tenant_id | Provisioning / identity lifecycle | OIDC exchange and auth | None identified | Tenant RLS; OIDC bootstrap resolver required |
| principal_permissions | Direct tenant_id | Authorization administration | Request ACL checks, WS | WS delivery | Tenant RLS |
| security_audit_events | Direct tenant_id nullable in schema | Auth/authorization denial and security events | Security operations | Potential security consumer | Split tenant-scoped rows from global denial rows; explicit worker policy |
| auth_token_replays | Issuer + token JTI; global replay namespace | OIDC exchange | OIDC exchange | Cleanup in exchange transaction | Global security table; do not fake tenant ownership |
| team_tasks | Direct tenant_id and room_id | Task APIs, handoffs, recovery | Task APIs, AI-PHONE, WS | Recovery worker | Tenant RLS |
| task_lease_events | Direct tenant_id and room_id | Task lease/checkpoint transitions | Task history / audit | Recovery/lease paths | Tenant RLS |
| task_handoffs | Direct tenant_id and room_id | Handoff APIs and recovery | Handoff APIs, WS | Recovery worker | Tenant RLS |
| team_room_channels | Direct tenant_id and room_id | Communication setup | Communication, WS | WS aggregate validation | Tenant RLS |
| team_room_channel_members | Channel/principal relationship; tenant derived by channel | Private-channel membership | ACL and WS | WS ACL | Add/retain explicit tenant ownership or use a validated parent relationship; RLS required |
| team_room_messages | Direct tenant_id and room_id | Communication API | History, private history, WS | Outbox delivery | Tenant RLS |
| team_room_message_reads | Channel/principal relationship; tenant derived by channel | History read marker | History/unread count | None | Tenant-safe parent policy required |
| team_room_decisions | Tenant/room scope in schema | Decision API | Decision API | Outbox | Tenant RLS |
| team_room_attachment_refs | Direct tenant_id and room_id | Attachment API | Attachment listing | Outbox | Tenant RLS |
| team_room_ws_tickets | Direct tenant_id and room_id | Ticket issue/consume | WS handshake | WS lifecycle | Tenant RLS; one-use consume must run with authenticated scope |
| ai_identities | Global AI identity namespace | Trusted provisioning | Identity provisioning | None | Global control-plane table; no caller-wide read grant |
| ai_identity_principals | Direct tenant_id plus global ai_id | Trusted provisioning | AI-PHONE ownership/auth | None | Tenant RLS |
| ai_identity_tenants | Direct tenant_id plus global ai_id | Trusted provisioning/migration | AI-PHONE ownership | None | Tenant RLS |
| ai_phones | Direct tenant_id | AI-PHONE provisioning/state transitions | AI-PHONE | None | Tenant RLS |
| phone_endpoints | Direct tenant_id | Registration/attach/detach/revoke | AI-PHONE | None | Tenant RLS |
| phone_attachments | Direct tenant_id, room_id | Attach/transfer/fencing | AI-PHONE | None | Tenant RLS |
| phone_messages | Direct tenant_id | Send/delivery/ACK | Inbox/history/sent | Polling clients | Tenant RLS |
| phone_mission_links | Direct tenant_id and room_id | AI-PHONE mission link | AI-PHONE/task continuity | None | Tenant RLS |
| permission_catalog | Global permission definitions | Migration/admin | Authorization setup | None | Global reference table; least-privilege read only |

Notes:
- `ai_identities` and `auth_token_replays` are intentionally global namespaces. Assigning them an arbitrary tenant would create false ownership.
- Foreign keys added by migrations 0012 and 0013 enforce relational tenant consistency but are not substitutes for RLS.
- Some child tables inherit tenant scope through a channel or other parent rather than carrying `tenant_id` directly. These need a deliberate policy design, not a permissive default.
- Worker ownership is not yet a formal DB principal or capability boundary.

## 3. Critical blockers discovered

### B1 — Authentication bootstrap has no tenant context

`authenticate_session` hashes the bearer token and queries `sessions` to discover `tenant_id`. Under tenant RLS, the session row is invisible before context is set. Taking tenant_id from a request body/header would be an unsafe fix.

OIDC exchange has a similar bootstrap requirement: the verified issuer/subject and room identify the registered tenant, but this query currently occurs inside a transaction without a tenant GUC.

**Required design:** a narrow database authentication resolver (or equivalent cryptographically verified context bootstrap) that derives tenant ownership from the credential/session record itself. It must not accept a caller-selected tenant as authority. Its privileges and function search_path must be fixed and audited.

### B2 — `app.tenant_id` alone is not tamper-proof

A custom PostgreSQL GUC such as `app.tenant_id` is ordinarily settable by the connected application role. A policy that trusts only `current_setting('app.tenant_id')` does not prove adversarial case 6 (tenant A changing the setting to B).

**Required design:** the tenant context must be established by a database-verifiable mechanism tied to an authenticated session or an unforgeable server assertion. Do not assume `SET LOCAL` itself grants authority. A SECURITY DEFINER bootstrap function may be appropriate only with a fixed search_path, narrow EXECUTE grants, protected verification material, and tests proving the caller cannot choose another tenant.

### B3 — Tenant context is not threaded through all SQL paths

The inventory found numerous direct pool queries and functions without a tenant-scoped transaction:
- AI-PHONE includes direct pool reads in endpoint registration and list handlers, in addition to explicit transactions.
- Task APIs include direct pool reads such as mission lookup and candidate listing; recovery has only a room_id parameter.
- Handoff APIs include multiple direct pool reads before transactions.
- Communication includes direct pool reads for ACL, history, ticket issue/consume, WebSocket authentication and outbox dispatch.
- Auth session resolution and permission checks use direct pool queries.

RLS cannot be safely activated until every protected request path uses a transaction whose tenant context was established from authenticated server-side identity. Read-only calls must also use that transaction. Returning a connection to the pool must clear context by transaction end.

### B4 — Background workers and WebSocket listener lack explicit tenant authority

Task recovery currently accepts a room identifier without a tenant identity. WebSocket listener connections receive notifications globally and fetch outbox rows using the shared pool. These need explicit worker identities and narrowly scoped access. A generic worker with BYPASSRLS or owner privileges would defeat the objective.

### B5 — CI does not model the production role boundary

The current disposable PostgreSQL workflow initializes `ai_phone_ci` as the database bootstrap superuser and runs migrations/tests with that connection. Passing the current suite therefore cannot prove behavior for a non-owner application role subject to FORCE RLS.

A new disposable fixture must create distinct migration/owner, application, and worker roles, then run the application tests using the actual least-privilege role. Superuser/owner-based test success is not acceptable evidence.

## 4. Safe implementation sequence required before enabling policies

1. Add separate DB URLs/roles for migration owner, application, and worker; ensure the runtime application role is neither owner, SUPERUSER nor BYPASSRLS.
2. Design and test the authentication bootstrap that derives tenant from verified session credentials; do not read tenant from request payload.
3. Introduce a single SQLx helper for tenant-scoped transactions. It must begin a transaction, set transaction-local context through the trusted DB bootstrap, and pass only the transaction to tenant-owned queries.
4. Refactor auth, AI-PHONE, Team Room, tasks, handoffs, communication and WebSocket code so no protected query uses the bare pool.
5. Change recovery and outbox consumers to receive explicit scoped worker context/capability; define what each worker may read/write.
6. Add role grants and FORCE RLS policies with no permissive `USING (true)` / `WITH CHECK (true)` policy.
7. Run tests as the non-owner application/worker roles, including missing context, forged tenant context, pool reuse, rollback and transaction abort.
8. Run all existing AI-PHONE, Team Room and legacy fixtures using the constrained roles.
9. Only then enable RLS in a forward-only migration.

## 5. Round status

| Round | Status | Evidence / reason |
|---|---|---|
| 1 — Rebaseline and inventory | COMPLETE | Branch, SHA, PRs and Run #92 verified; table and SQL-path inventory recorded |
| 2 — Transaction context | BLOCKED | No authenticated tenant bootstrap; GUC-only context is caller-modifiable |
| 3 — PostgreSQL roles | BLOCKED | Existing CI uses bootstrap superuser; runtime/migration roles not separated |
| 4 — AI-PHONE RLS | NOT IMPLEMENTED | Enabling policies before context refactor would break legitimate access or require bypass |
| 5 — Team Room RLS | NOT IMPLEMENTED | Same; worker/WS paths need explicit context |
| 6 — Adversarial RLS tests | NOT RUN | No safe production-role/policy implementation exists to test |
| 7 — AI-PHONE compatibility | NOT RUN UNDER RLS | Existing non-RLS Run #92 is not RLS evidence |
| 8 — Team Room + legacy | NOT RUN UNDER RLS | Legacy fixtures do not exercise RLS roles/policies |
| 9 — Rollback/security review | PARTIAL STATIC REVIEW | No RLS migration/policies added; detailed bypass scan must follow implementation |
| 10 — Final CI | PARTIAL | No RLS CI run or SHA-specific RLS evidence |

## 6. Changes in this mission

- Created branch `mission/ai-phone-07-rls-production-gate` from the exact AI-PHONE-06 HEAD.
- Added this architecture/gate report.
- No application code, migrations, roles, policies, or CI workflows were changed.
- No test result is claimed for RLS.

## 7. Residual bypass assessment

No new RLS bypass was introduced because no RLS policy or privileged role was added.

Existing architecture is **not RLS-protected**:
- Current CI role is superuser in its disposable PostgreSQL instance.
- Runtime role separation is not established by the checked-in service bootstrap.
- App-level tenant predicates and composite foreign keys remain the current controls.
- A future policy based only on a writable `app.tenant_id` GUC would be bypassable by the application role and is explicitly rejected by this report.
- The final scan of an implemented RLS migration is NOT APPLICABLE because no such migration exists.

## 8. AI-PHONE / Team Room / legacy impact

- AI-PHONE code paths and endpoint revocation behavior were not changed.
- Team Room task, handoff, ACL, outbox and WebSocket behavior were not changed.
- Migrations 0001–0013 remain unchanged.
- Ambiguous legacy migration behavior remains fail-closed; compatible legacy migration behavior is unchanged.
- Run #92 remains the prior non-RLS validation only.

## 9. Final verdict

`RLS_PRODUCTION_GATE_PARTIAL`

This is an intentional fail-closed stop, not a successful RLS implementation. The current architecture does not yet have the transaction-scoped, tamper-resistant tenant context and least-privilege role model needed to enable RLS without introducing a bypass or breaking legitimate workflows.

**Production remains blocked by RLS. Production also remains blocked by PoP before external endpoints**, as recorded in AI-PHONE-06. PoP was not implemented in this mission.
