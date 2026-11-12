# AI-PHONE-08 — Tenant Context + PostgreSQL Roles

## Final classification

`TENANT_CONTEXT_ROLES_PARTIAL`

`RLS_NOT_YET_ENABLED`

`POP_NOT_IMPLEMENTED`

The branch was created from the exact requested AI-PHONE-07 HEAD and the repository's runtime/authentication/worker paths were inspected. The implementation gate is **not ready**: no trusted tenant bootstrap, transaction context helper, role fixture, worker role integration, or restricted-role CI execution was completed in this mission run. No claim of tenant-context or role-isolation proof is made.

This is a fail-closed result. It would be unsafe to introduce a helper that merely accepts an arbitrary `tenant_id` and writes `SET LOCAL app.tenant_id`, because a writable custom GUC is not an authenticated tenant authority. Likewise, declaring roles without actually running the backend as those roles would not prove the required boundary.

## 1. Rebaseline

- Repository: `vidift-max/tokensaver-ai`
- Source branch: `mission/ai-phone-07-rls-production-gate`
- Source HEAD: `9da1830ae4a6f097c91fbf264c7325e15114d496`
- New branch: `mission/ai-phone-08-tenant-context-roles`
- Initial SHA: `9da1830ae4a6f097c91fbf264c7325e15114d496`
- AI-PHONE-06 ancestor: `0ea7f2cd0f5054ef97bd41ca9db25c998176512f`
- AI-PHONE-07 commit parent: `0ea7f2cd0f5054ef97bd41ca9db25c998176512f`; therefore AI-PHONE-07 is a direct child of AI-PHONE-06 and the new branch starts from its exact HEAD.
- Architectural base: `d1f2bd764361b14af0604104db8e670ee6f4f361`
- Existing migrations: `0001`–`0013`

PR status checked before branch creation:
- #7: OPEN, DRAFT, unmerged
- #8: OPEN, DRAFT, unmerged
- #9: OPEN, DRAFT, unmerged
- #10: OPEN, DRAFT, unmerged

No historical PR was modified. No PR was created or merged. No production database was accessed.

## 2. Source-level access inventory

Inspected:
- `services/ai-team-room/src/lib.rs`
- `services/ai-team-room/src/main.rs`
- `services/ai-team-room/src/auth.rs`
- `services/ai-team-room/src/phone.rs`
- `services/ai-team-room/src/tasks.rs`
- `services/ai-team-room/src/handoffs.rs`
- `services/ai-team-room/src/communication.rs`
- PostgreSQL integration tests and current GitHub Actions workflow.

A static call-site scan across these Rust modules found the following syntactic call counts (these include test code in modules that embed unit/integration fixtures; they are not claimed as production-only query totals):

| Module | fetch_one | fetch_optional | fetch_all | execute | begin | PgListener |
|---|---:|---:|---:|---:|---:|---:|
| lib.rs | 5 | 4 | 0 | 9 | 2 | 0 |
| main.rs | 4 | 2 | 1 | 20 | 2 | 0 |
| auth.rs | 1 | 3 | 0 | 18 | 2 | 0 |
| phone.rs | 21 | 14 | 4 | 24 | 13 | 0 |
| tasks.rs | 18 | 3 | 7 | 23 | 7 | 0 |
| handoffs.rs | 20 | 7 | 1 | 59 | 5 | 0 |
| communication.rs | 27 | 7 | 6 | 35 | 6 | 2 |

The syntactic scan is an inventory aid, not a substitute for a complete AST/query-by-query review. Dynamic SQL, multiline chains and test-only call sites require manual classification before a final READY verdict.

### Access classification

| Path / operation | Classification | Current model / gap |
|---|---|---|
| `readiness()` | GLOBAL_CONTROL_PLANE | `SELECT 1`; no tenant data |
| `migrate()` | MIGRATION_ADMIN | Runs SQLx migrations on whichever pool is supplied; no distinct migration pool enforced by service bootstrap |
| OIDC token verification (JWKS/discovery) | GLOBAL_CONTROL_PLANE | External cryptographic verification, before DB tenant is resolved |
| OIDC replay insert/cleanup | GLOBAL_CONTROL_PLANE | Global replay namespace; must remain global and have a narrowly granted role path |
| OIDC issuer/subject + room registration lookup | AUTHENTICATION_BOOTSTRAP | Derives registered tenant from verified issuer/subject and room; currently executes in ordinary transaction without DB-enforced tenant context |
| `authenticate_session` token-hash expiry updates and session join | AUTHENTICATION_BOOTSTRAP | Looks up tenant from bearer credential; currently direct pool operations, including global security audit fallback |
| `require_permission` | TENANT_SCOPED_REQUEST | Receives `AuthenticatedIdentity`, but direct pool query and audit insert outside a shared scoped transaction |
| `close_session` | TENANT_SCOPED_REQUEST | Explicit transaction, but no tenant context is established |
| AI-PHONE `/me`, attach, detach, heartbeat, provision, messages, ACK, history, mission links | TENANT_SCOPED_REQUEST | Many explicit transactions; several direct pool reads remain; no authenticated DB context |
| Team Room presence, presence update, join/leave, task and handoff HTTP handlers | TENANT_SCOPED_REQUEST | Mixed explicit transactions and direct pool queries |
| Task create/acquire/renew/checkpoint/complete/release | TENANT_SCOPED_REQUEST | Most mutation paths use transactions, but do not establish tenant context |
| Task mission lookup and candidate listing | TENANT_SCOPED_REQUEST | Direct pool reads remain |
| `recover_expired(pool, room_id)` | TENANT_SCOPED_WORKER (intended) | Current signature accepts only pool + room UUID; it has no worker identity, tenant capability or authenticated scope |
| Handoff create/accept/reject/cancel/get | TENANT_SCOPED_REQUEST | Mutation paths use transactions; several preflight reads execute directly on pool |
| Communication ACL/history/private channel/message/ticket/decision/attachment/task listing | TENANT_SCOPED_REQUEST | Several direct pool reads/writes and independent transactions; not unified under one authenticated context |
| WebSocket ticket authentication | AUTHENTICATION_BOOTSTRAP / TENANT_SCOPED_REQUEST | Ticket lookup then session checks use direct pool queries |
| WebSocket `PgListener` event dispatch | TENANT_SCOPED_WORKER (intended) | Dedicated listener connection plus global NOTIFY channel; outbox and aggregate reads use the application pool directly |
| WebSocket delivery-boundary revalidation | TENANT_SCOPED_REQUEST | Opens a transaction for authorization locks, but does not establish a DB-authenticated tenant context |
| Test fixtures and cleanup | TEST_ADMIN | Direct inserts/updates/deletes and migration calls; must be separated from runtime-role tests, not mistaken for production request paths |

### Tenant ownership and global namespaces

- Direct tenant ownership exists on the core tenant-scoped tables documented in AI-PHONE-07, including sessions, workload executions, memberships, tasks, handoffs, channels, messages, audit/outbox and AI-PHONE tables.
- Child relationships such as private channel members and channel read markers derive scope through a parent relationship and need explicit parent-validated worker/request access.
- `ai_identities`, `auth_token_replays`, `permission_catalog` and the tenant registry are global/control-plane namespaces. They must not receive fabricated tenant ownership.
- Composite foreign keys remain relational integrity controls; they do not authenticate a tenant context and do not replace RLS.

## 3. PostgreSQL pool and transaction findings

- `connect_pool()` creates a SQLx `PgPool` with a maximum of 20 connections.
- The production `main()` currently reads one `DATABASE_URL`, creates one pool and calls `migrate(&pool)` on that same pool before serving HTTP.
- There is no separate migration-owner pool or runtime application pool.
- Existing handlers and modules pass `PgPool` broadly, so direct query execution remains easy.
- Explicit transactions exist, but none of the inspected production paths calls a central tenant-context helper.
- No `SET LOCAL app.tenant_id` helper exists.
- No persistent `SET app.tenant_id` was introduced.
- `communication.rs` creates `PgListener::connect_with(&pool)`; notification handling then reads outbox/aggregate/session/ACL rows via the pool.
- Worker recovery has no formal worker principal or capability parameter.

## 4. Authentication bootstrap decision

The bootstrap must be derived from a cryptographically verified bearer credential, not from a request body, query parameter, freely supplied header, or caller-selected tenant UUID.

A safe future design needs a database-verifiable assertion:
1. Backend hashes the presented bearer token.
2. A narrowly privileged database function resolves the active session and derives its tenant/principal/session/execution tuple from stored rows.
3. The function establishes a transaction-local signed context, or returns a database-verifiable assertion that the context helper can validate.
4. Tenant-scoped queries execute only after this bootstrap in the same transaction.

A plain custom GUC is not sufficient: the application role can ordinarily set custom GUC values itself. Merely moving `SET LOCAL app.tenant_id = $tenant` into a Rust helper does not prevent a programming error or malicious code running as that role from selecting another tenant. The next implementation must cryptographically bind context to a verified session/capability, with the signing material inaccessible to application and worker roles.

No SECURITY DEFINER function or signing secret was added in this mission run. This avoids shipping an untested privilege-escalation surface.

## 5. Role separation decision

Required future roles:
- `migration_owner`: owns application schema objects and executes migrations; never used for HTTP request queries.
- `application_role`: login role used by runtime requests; not SUPERUSER, not object owner, no BYPASSRLS; grants limited to application operations.
- `worker_role`: separate login role for recovery/outbox processing; not SUPERUSER, not object owner, no BYPASSRLS; grants limited to explicitly defined worker operations.

Current state:
- The checked-in workflow initializes PostgreSQL with `POSTGRES_USER=ai_phone_ci`, the bootstrap role for the disposable PostgreSQL service. The workflow does not create or assert separate migration/application/worker roles.
- Current `main()` does not read a separate migration URL.
- No role fixture, grants matrix, or live role-attribute assertion was added.
- Existing PostgreSQL tests call `migrate(&pool)` and use direct fixture DML. A safe role split must migrate as owner first, then run application integration tests on the non-owner application URL; test fixture setup/cleanup privileges need an explicit design. Worker tests must connect as worker role, not simply use application pool.

No application/worker privilege is claimed as proven.

## 6. Worker and WebSocket authority

### Recovery worker

Current API:
`recover_expired(pool, room_id)`

This is insufficient as an authority boundary. A room UUID is a selector, not a worker credential. The future API must accept an explicit worker capability or a tenant-scoped authenticated work item derived from trusted database state. The database context must be established from that capability, and the worker role must not be able to enumerate arbitrary tenant work.

### Outbox/WebSocket dispatcher

The current WebSocket listener is a global notification subscriber. NOTIFY payloads contain event identifiers and are not trusted as tenant authority; the outbox row is currently used to resolve tenant and aggregate scope. That is an application-level validation path, not a database role boundary.

A safe design must separate:
- a narrow global notification ingress that can discover only an opaque event locator;
- a trusted worker bootstrap that resolves event tenant from the durable outbox row;
- tenant-scoped delivery transactions for the specific tenant and room;
- application session/permission revalidation before socket delivery.

The worker must not use SUPERUSER, object-owner bypass or BYPASSRLS. No dispatcher redesign was implemented.

## 7. Round status

| Round | Status | Evidence |
|---|---|---|
| 1 — Rebaseline and complete access map | PARTIAL | Exact source branch/HEAD/ancestry/PR status confirmed; source files inspected and static call counts recorded; exhaustive AST-level classification remains incomplete |
| 2 — PostgreSQL roles | BLOCKED | No role fixture or live role assertions added |
| 3 — Authentication bootstrap | BLOCKED | No trusted DB bootstrap function or signed context implemented |
| 4 — Central tenant transaction helper | BLOCKED | No helper added; GUC-only approach explicitly rejected |
| 5 — AI-PHONE refactor | NOT STARTED | Existing direct pool calls remain |
| 6 — Team Room/Auth/Handoff refactor | NOT STARTED | Existing direct pool calls remain |
| 7 — Worker/WebSocket authority | BLOCKED | Worker identity/capability and listener dispatcher redesign not implemented |
| 8 — Restricted-role CI | BLOCKED | Existing CI role model unchanged; no runtime suite executed as non-owner application role |
| 9 — Adversarial context tests | NOT RUN | No trusted context mechanism exists to test |
| 10 — Full CI and decision | NOT RUN | No code changes requiring CI were made; no CI pass is claimed for this branch |

## 8. Changes and validation

- New branch created at the exact AI-PHONE-07 HEAD.
- This report is the only file added by this mission run.
- No Rust application code changed.
- No SQL migration changed or added.
- No role, grant, function, or policy was created.
- RLS remains disabled.
- No PostgreSQL integration test, cargo check, cargo test, worker test, or CI run was executed for AI-PHONE-08.
- No production database was accessed.

## 9. Residual risks and release gates

- The application still uses one pool for migrations and runtime.
- Authentication bootstrap still resolves tenant through ordinary application queries.
- The custom-GUC trust issue remains unresolved.
- Tenant-scoped direct pool queries remain across auth, AI-PHONE, Team Room and communication.
- Recovery worker still accepts an untrusted room selector without worker authority.
- WebSocket listener still consumes a global notification channel and performs direct pool reads.
- The CI role model still does not prove non-owner runtime execution.
- Because RLS is not enabled, this mission does not claim database-enforced cross-tenant isolation.

## 10. Final decision

`TENANT_CONTEXT_ROLES_PARTIAL`

The backend cannot yet be said to execute every tenant-scoped operation in a trusted transaction under a non-owner application role. The correct answer to the mission's final question is **NO — not proven**.

`RLS_NOT_YET_ENABLED`

`POP_NOT_IMPLEMENTED`

Production remains blocked by the RLS infrastructure gate and by PoP before external endpoints. Do not proceed to RLS policy activation until the blockers in sections 4–7 are implemented and tested under actual PostgreSQL roles.


---

## AI-PHONE-08B continuation — execution checkpoint (2026-11-12)

### Continuation baseline revalidation

- Work branch: `mission/ai-phone-08-tenant-context-roles`
- Verified branch HEAD before this checkpoint: `2f9ea8ce4213f5afcad520f0367196de42769fab`
- Required AI-PHONE-07 baseline: `9da1830ae4a6f097c91fbf264c7325e15114d496`
- Compare result: `ahead`, 1 commit, 0 behind; merge base equals the required baseline.
- The one commit is the report-only AI-PHONE-08 commit; no implementation changes were present.
- PRs #7, #8, #9 and #10 were rechecked: all OPEN, DRAFT, and unmerged. No PR was modified, created or merged.

### Additional implementation/CI inspection

The continuation inspected the existing service sources, migration set 0001–0013, Cargo manifest and CI workflow.

| Area | Current access/authority classification | Evidence-based gap |
|---|---|---|
| Pool/bootstrap | One SQLx `PgPool`, max 20 | No distinct migration, application and worker pools/URLs |
| Migration | `migrate(&pool)` during service startup | Runs through the same pool used by runtime |
| Authentication | OIDC verification, replay state, token-hash session lookup | Bootstrap is not a DB-verified tenant capability; session lookup uses ordinary pool access |
| AI-PHONE | `phone.rs` and handlers in `main.rs` | Tenant-owned phone, endpoint, attachment, inbox/message, mission-link and fencing paths use mixed direct pool and transaction access; no central authenticated tenant transaction |
| Team Room / tasks | `main.rs`, `tasks.rs` | Presence, task lease, checkpoint and task reads/writes use mixed pool and transaction access |
| Handoffs | `handoffs.rs` | Tenant-scoped preflight reads and mutations are not uniformly bound to one trusted context |
| Communication | `communication.rs` | Private channel ACL, message/history, attachment and WebSocket ticket paths are not uniformly context-bound |
| Recovery | Recovery API accepts `pool` and `room_id` | Room UUID is a selector, not worker authority; no explicit worker identity/capability |
| WebSocket | Global `PgListener` notification ingress, followed by DB reads | No separate worker role or DB-authenticated per-event tenant capability |
| Global/control plane | `ai_identities`, `auth_token_replays`, `permission_catalog`, tenant registry | Must remain intentionally global; must not receive fabricated tenant context |
| Migration/admin | SQLx migration runner and test fixture DML | Must be separated from request/worker credentials |

The table is a module/path classification, not a completed query-by-query AST audit. Dynamic query construction, every SQL call-site, all transaction boundaries and every protected route were not exhaustively classified. Therefore Round 1 remains incomplete under the mission's own stop condition.

### CI findings

- Existing `.github/workflows/ci.yml` runs on pushes to `v2-saas-alpha` and pull requests targeting `v2-saas-alpha`; it does not provide a branch-local workflow-dispatch path for this work branch.
- The AI Team Room PostgreSQL service initializes a single `POSTGRES_USER=team_room` and supplies one `DATABASE_URL` to both migration and runtime tests.
- The workflow does not create/assert separate `migration_owner`, `application_role` and `worker_role`.
- The PostgreSQL service bootstrap user is not evidence of a restricted runtime role.
- No role attributes, object ownership, grants or runtime-role execution are currently asserted by this workflow.

### Execution boundary and stop decision

The available GitHub integration in this execution can inspect and commit repository files, but does not expose a shell, a disposable PostgreSQL connection, or a command runner for `psql`, `cargo check`, `cargo test` or arbitrary CI commands. The existing CI workflow also does not target this branch and has no workflow-dispatch trigger.

No code, SQL migration, role, grant, function, policy or workflow was changed in this continuation. No PostgreSQL fixture was started. No cargo command, database query, adversarial test or CI run was executed. No test pass is claimed.

Per the explicit instruction “do not alter code if the map is incomplete”, implementation was stopped before Round 2. Introducing unverified SECURITY DEFINER/bootstrap logic, signing material, pool privilege changes or tenant transaction wrappers through unexecuted edits would create an unacceptable security risk.

### AI-PHONE-08B checkpoint verdict

`TENANT_CONTEXT_ROLES_BLOCKED`

- `RLS_NOT_YET_ENABLED`
- `POP_NOT_IMPLEMENTED`
- Production readiness is NOT claimed.
- The existing branch remains unchanged except for this report addendum.
- Next valid execution prerequisite: provide/enable a reproducible disposable PostgreSQL + shell/CI execution path that can run against this branch, then finish the query-by-query access inventory before role/bootstrap implementation.
