# AI Team Room Service — Mission 108 Architecture Record

Status: IMPLEMENTED FOUNDATION / NOT OPERATIONAL FOR MULTI-AGENT TRAFFIC

## Service boundary

The service is a standalone Rust process under `services/ai-team-room`. It is not a Tauri plugin, child process, desktop-hosted server, or shared use of project SQLite. A container image can be built with the adjacent Dockerfile. The desktop will later be a client of a deployed HTTPS service; it does not host the service.

## Technology decisions

### Rust + Axum

The product already uses Rust, so the service can share language-level expertise and domain conventions without linking its lifecycle to Tauri. Axum provides an explicit HTTP router and Tokio runtime. The current HTTP surface is deliberately limited to liveness/readiness; identity-protected room APIs are not exposed until authentication middleware exists.

### PostgreSQL 16+

PostgreSQL is the authoritative shared store because admission changes membership, session state, audit and outbox records as one unit. Row-level locking and transactions are available across service instances. The admission path locks the room row with `SELECT ... FOR UPDATE`; all admission and leave operations for a room serialize on that row before reading the active count. A database unique constraint prevents duplicate room/principal membership rows. The capacity column is constrained to exactly 15.

SQLite remains the desktop's local project store and is not read or written by this service.

### No Redis yet

Presence, distributed fanout, and cache invalidation are not implemented. Adding Redis now would introduce an operational dependency without a current consumer. Re-evaluate when leases/pub-sub are implemented. PostgreSQL remains the source of truth.

### No WebSocket or WebRTC yet

No messaging API exists. WebSocket is deferred until identity, authorization, message persistence, and outbox consumers exist. WebRTC/SFU is later still and is not simulated.

## Identity model

The schema distinguishes `participant_id`, `principal_id`, and `session_id`. A participant registration is metadata and cannot authenticate. Admission verifies that the participant maps to an enabled AI principal and that a non-revoked, unexpired room-scoped session exists for that same principal. Workload identity verification and secure session issuance are not yet implemented; only trusted internal callers may invoke the admission primitive.

The `workload_executions` table now gives `execution_id` a separate persisted identity, with composite foreign keys binding it to the participant/principal/session tuple and worker subject, heartbeat, expiry and revocation fields. No trusted runtime adapter writes this table and no execution state is exposed by this service yet.

## Admission invariant

- The room row is locked before checking or changing membership.
- Session/participant/principal binding is validated inside the same transaction.
- New memberships and re-entry from LEFT count active AI members and reject at 15.
- A currently active principal can reconnect with a fresh session without consuming another slot; the previous room-scoped session is revoked.
- Reuse of the same active session is rejected.
- Membership, audit event and outbox event commit atomically.
- Leaving revokes the current room-scoped session and marks membership LEFT in one transaction.
- The same locking protocol is mandatory for every future path that mutates active membership. Database credentials must be restricted to the service; clients must never write these tables directly.

This is a service-level concurrency invariant, not a UI count. PostgreSQL integration tests run 16 simultaneous admissions and assert exactly 15 succeed.

## Current HTTP surface

- `GET /health/live`: process liveness only.
- `GET /health/ready`: confirms PostgreSQL is reachable.
- No invitation, login, token issuance, join, messages, private chat, tasks, presence, audit query, or media routes.

## Deployment and security boundaries

- `DATABASE_URL` is required at startup and must be injected from a deployment secret manager.
- Do not commit production credentials or use the CI-only password outside ephemeral CI.
- Bind defaults to loopback for local development; container sets 0.0.0.0 inside its network namespace.
- Deploy behind TLS termination, private network controls, and rate limiting.
- Database role must be least-privilege and scoped to the service schema.
- Do not expose PostgreSQL publicly.
- Health endpoints return no tenant, identity, database error, or secret detail.
- No GitHub, code, file, publishing, command execution, or credential grants are conferred by room membership.

## Recovery and consistency

PostgreSQL WAL and backups are the durability mechanism to configure in deployment. Membership, audit and outbox are in one transaction, so a committed admission cannot lose its corresponding audit/outbox record. Outbox delivery/retention/replay worker is not yet implemented. Backup schedules, PITR, restore drills and production monitoring remain deployment gates.
