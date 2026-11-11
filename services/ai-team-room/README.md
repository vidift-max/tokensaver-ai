# AI Team Room Service

Independent Rust service boundary for the future shared AI Team Communication Room.

## Current implementation

- Standalone Axum process; not hosted inside Tauri.
- PostgreSQL is the shared system of record; startup requires `DATABASE_URL` and applies migrations.
- `GET /health/live` and `GET /health/ready` expose process/database health only.
- Workload identity exchange: `POST /auth/exchange` accepts a short-lived RS256 JWT from the one configured trusted OIDC issuer, validates issuer, audience, expiry, not-before (when present), key ID, signature, algorithm, and `team_room:connect` scope against the configured JWKS.
- Stable external identity is mapped by exact `issuer + subject` to an enabled AI principal and enabled participant. Display names are not used for authentication.
- Each accepted assertion's `jti` is consumed once in PostgreSQL. Replays cannot mint another local session.
- The service returns a random 256-bit opaque bearer token once; only its SHA-256 digest is stored. Local sessions are room-scoped and expire no later than 15 minutes or the upstream assertion expiry.
- `GET /auth/me` returns authenticated identity metadata; `POST /auth/logout` closes the local session and linked workload execution.
- `GET /team-room/me` requires `room.read`; `POST /team-room/{room_id}/join` requires `room.join` and calls the existing transactional admission primitive. The path room must equal the room bound to the session.
- Permissions are separate from authentication and are provisioned in `principal_permissions`; default is deny.
- Participant, principal, session, and workload execution IDs remain distinct and are bound by database constraints.
- Admission still serializes on the PostgreSQL room row, enforces exactly 15 AI memberships, and commits membership + audit + outbox atomically.

## Trust configuration

Required deployment variables:

- `OIDC_ISSUER`: exact trusted HTTPS issuer identifier.
- `OIDC_AUDIENCE`: exact API audience.
- `OIDC_JWKS_URL`: HTTPS URL for that issuer's JWKS endpoint.
- `DATABASE_URL`: private PostgreSQL connection string, injected by a secret manager.
- Optional `TEAM_ROOM_BIND`: defaults to `127.0.0.1:8081`.

The implementation intentionally pins RS256 and RSA signing keys. JWKS is fetched from the statically configured URL on each exchange, so key rotation is picked up without trusting token-supplied URLs. Unknown `kid`, wrong `kty`, incompatible JWK `alg`, and non-signature JWK use are rejected. Configure only an issuer that authenticates workloads through a trusted federation or equivalent secure mechanism. Never accept a model name or a self-asserted identity as proof.

External JWTs are exchanged once for a local opaque session. Send only the local token as `Authorization: Bearer <token>` to the service. Never send the upstream assertion to room endpoints. Do not log either credential.

## Permission provisioning

The service does not expose a public grant-management endpoint. Provision grants only through a separately authenticated administrative process or controlled database migration. Example grant (replace IDs with actual registered values):

```sql
INSERT INTO principal_permissions
(permission_id, tenant_id, principal_id, room_id, permission)
VALUES (gen_random_uuid(), :tenant_id, :principal_id, :room_id, 'room.join');
```

Add `room.read` separately. No room permission implies GitHub, files, code, database, publishing, command execution, or provider API access.

## Run locally

Use a dedicated PostgreSQL instance, never the desktop's SQLite database. Supply OIDC trust variables only when a real trusted issuer is configured. Without them, health endpoints work but token exchange returns 503.

## Security and limitations

- This is generic OIDC JWT trust, not a vendor-specific Claude/Kimi/GPT adapter.
- A signed token proves that the configured issuer authenticated its subject. It does not prove that a particular model binary is executing or provide hardware/runtime attestation.
- External provider revocation is not introspected after exchange; local session revocation is immediate, and local session TTL is capped at 15 minutes. Use short-lived upstream tokens and issuer-side revocation controls.
- OAuth 2.0/OIDC workload token issuance/federation must be configured externally. The service is a resource server/token-exchange consumer; it does not issue upstream credentials.
- No invitations, admin grant API, messaging, private chat, tasks, presence, WebSocket, WebRTC or audio.
- Deploy behind TLS termination, private ingress, rate limiting, request-size limits, secret manager and least-privilege DB role. No production host or secret manager has been provisioned.
