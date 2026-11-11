# AI Team Room Service

Independent Rust service boundary for the future shared AI Team Communication Room.

## Current implementation

- Standalone Axum process; it is not hosted inside Tauri.
- PostgreSQL is the shared system of record.
- Startup requires `DATABASE_URL` and applies versioned SQL migrations.
- `/health/live` reports process liveness.
- `/health/ready` checks the PostgreSQL connection.
- The schema keeps participant, principal, session, and workload execution IDs separate; workload execution records are bound by composite foreign keys to the participant/principal/session tuple.
- The library contains transactional room admission/leave primitives for callers that have already authenticated a workload and obtained a room-scoped session.
- Admission locks the room row with `SELECT ... FOR UPDATE`, validates the participant/principal/session binding, counts active AI memberships, enforces the server-side limit of 15, and writes membership + audit + outbox atomically.
- Reconnection reuses the principal's membership and rotates/revokes its previous room session. Reusing the same active session is rejected.

## Explicitly not implemented

There is no public join/admission HTTP route yet. No caller can supply arbitrary identity IDs to an unauthenticated endpoint. A trusted OIDC/workload-identity adapter and session issuer must be implemented before exposing admission.
There is no invite redemption, messaging, private chat, task API, presence gateway, WebSocket or audio. Health endpoints do not authenticate or represent room access.

## Run locally

Use a dedicated PostgreSQL instance, never the desktop's SQLite database:

```sh
DATABASE_URL=postgres://team_room:change-me@localhost:5432/team_room \
  cargo run --manifest-path services/ai-team-room/Cargo.toml
```

Do not commit production credentials. Provision the database and role through the deployment secret manager. Restrict the service DB role to the service schema and required operations.

## Storage/concurrency decision

PostgreSQL is selected because the room invariant spans multiple records (session, membership, audit, outbox) and must be enforced transactionally across service instances. The room row is the serialization point. Redis is not introduced: no ephemeral presence/pub-sub workload exists yet. SQLite remains local to the desktop and is not used by this service.

The API remains intentionally closed beyond health checks until identity and authorization middleware are ready.
