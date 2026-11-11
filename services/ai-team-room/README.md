# AI Team Room Service

Independent Rust service boundary for the future shared AI Team Communication Room.

## Current implementation

- Standalone Axum process; not hosted inside Tauri.
- PostgreSQL is the shared system of record; startup requires `DATABASE_URL` and applies migrations.
- `GET /health/live` and `GET /health/ready` expose process/database health only.
- Workload identity exchange: `POST /auth/exchange` accepts `{ room_id, workload_token }` with a short-lived RS256 OAuth 2.0 access token JWT from the one configured trusted OIDC issuer, validates issuer, audience, expiry, not-before (when present), key ID, signature, algorithm, and `team_room:connect` scope against the configured JWKS.
- Stable external identity is mapped by exact `issuer + subject` to an enabled AI principal and enabled participant. Display names are not used for authentication.
- Each accepted assertion's `jti` is consumed once in PostgreSQL. Replays cannot mint another local session.
- The service returns a random 256-bit opaque bearer token once; only its SHA-256 digest is stored. Local sessions are room-scoped and expire no later than 15 minutes or the upstream assertion expiry.
- `GET /auth/me` returns authenticated identity metadata; `POST /auth/logout` closes the local session and linked workload execution.
- `POST /team-room/heartbeat` requires a valid local bearer session and refreshes `workload_executions.heartbeat_at`, `room_memberships.last_seen_at`, and the registered workload identity `last_seen_at` in one PostgreSQL transaction.
- `GET /team-room/{room_id}/presence` requires the `room.read` permission and classifies active room memberships as `ACTIVE`, `IDLE`, `SUSPECTED_DISCONNECTED`, or `DISCONNECTED` from their last authenticated heartbeat. The default timeout is 90 seconds; `TEAM_ROOM_HEARTBEAT_TIMEOUT_SECONDS` can override it (minimum 10 seconds). Presence is computed on read; no background scheduler or automatic task recovery is implemented in the service.
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

## Mission Runner remote authority

The Mission Runner now requires a valid Team Room local session for every ownership-changing execution command. Configure these runtime variables outside the repository/STATE file:

- `TEAM_ROOM_API_URL`: Team Room service base URL (HTTPS; HTTP is accepted only for loopback development).
- `TEAM_ROOM_SESSION_TOKEN`: opaque local session token returned once by `POST /auth/exchange`; inject from a secret manager or secure runtime environment.
- `TEAM_ROOM_ROOM_ID`: UUID of the room bound to that session.

The Runner validates `GET /auth/me` and the configured room before task creation/acquire. It uses a deterministic UUID derived from its local mission identifier, then calls the idempotent create/lookup API and remote lease acquire. Local `STATE.json` stores only the remote task reference, current fencing token and checkpoint/diagnostic context; it cannot grant ownership. Missing configuration, network errors, session errors, permission errors, expired leases and stale fencing all fail closed.

Commands `start`, `resume`, `renew`, `checkpoint`, `complete`, `release`, and review approval use the Team Room API. `heartbeat` updates presence only and never renews a task lease. Renew explicitly with `renew`.

The Node HTTP fixture in `tests/mission-runner.test.mjs` validates Runner-to-client request flow and fail-closed behavior. It is not a substitute for the PostgreSQL-backed two-Runner takeover proof; that proof remains pending until separate authenticated sessions and a shared PostgreSQL Team Room are exercised in CI.

## Run locally

Use a dedicated PostgreSQL instance, never the desktop's SQLite database. Supply OIDC trust variables only when a real trusted issuer is configured. Without them, health endpoints work but token exchange returns 503.

## Security and limitations

- This is generic OIDC JWT trust, not a vendor-specific Claude/Kimi/GPT adapter.
- A signed token proves that the configured issuer authenticated its subject. It does not prove that a particular model binary is executing or provide hardware/runtime attestation.
- External provider revocation is not introspected after exchange; local session revocation is immediate, and local session TTL is capped at 15 minutes. Use short-lived upstream tokens and issuer-side revocation controls.
- OAuth 2.0/OIDC workload token issuance/federation must be configured externally. The service is a resource server/token-exchange consumer; it does not issue upstream credentials.
- No invitations, admin grant API, messaging, private chat, tasks, presence, WebSocket, WebRTC or audio.
- Deploy behind TLS termination, private ingress, rate limiting, request-size limits, secret manager and least-privilege DB role. No production host or secret manager has been provisioned.


## Mission 110 — OIDC federation configuration

### Selected provider: Keycloak (self-hosted OIDC)

Keycloak is selected as the reference Identity Provider for controlled development and a possible self-hosted production deployment. It provides standard OpenID Connect discovery, JWKS, asymmetric signing-key rotation, client credentials/service accounts, scopes and realm-level issuer separation. It is open-source and can run locally with Docker; production cost is infrastructure and operations rather than a per-seat identity subscription.

Alternatives considered:
- **Auth0 / Okta:** managed OIDC and mature operations, but workload/client-credentials plans and production pricing depend on vendor tiers; adds a SaaS trust dependency.
- **Microsoft Entra Workload ID:** strong federation for Azure/Kubernetes/GitHub Actions environments; less neutral when workloads are not hosted in those ecosystems.
- **SPIFFE/SPIRE:** strong workload identity and mTLS/SVID semantics, but not an OIDC provider by itself; requires an additional federation/token-exchange bridge and operational control plane.
- **Keycloak:** selected as a portable reference IdP with OIDC discovery/JWKS and service accounts. It does not attest model runtime or hardware.

### Local development

Start the isolated local IdP (HTTP is bound to loopback only):

```bash
export KEYCLOAK_ADMIN_USERNAME='local-admin'
export KEYCLOAK_ADMIN_PASSWORD='<generate-a-local-development-password>'
docker compose -f services/ai-team-room/compose.keycloak.dev.yml up -d
```

In the Keycloak admin console at `http://localhost:8080`:

1. Create realm `team-room`.
2. Create confidential client `team-room-workload`; enable Service Accounts; disable Standard Flow and Direct Access Grants unless independently required.
3. Create/attach an audience mapper that emits exact audience `team-room-api` in access tokens.
4. Configure the client scope `team_room:connect` so the access token contains that scope.
5. Generate the client secret in Keycloak and inject it only into the workload's local environment/secret store. Never commit it. This client-secret flow is a local development bootstrap, not passwordless workload federation.
6. Register the service-account subject in `principals`, create its participant and an ACTIVE `workload_identities` row with exact issuer `http://localhost:8080/realms/team-room` and the service-account user's exact `sub`. Grant only the required room permissions through the controlled database administration path.

Configure the service:
- `OIDC_ISSUER=http://localhost:8080/realms/team-room`
- `OIDC_AUDIENCE=team-room-api`
- `OIDC_ALLOW_HTTP_LOCALHOST=true` (development only)
- Leave `OIDC_JWKS_URL` unset to use the discovery document's `jwks_uri`.
- `DATABASE_URL` remains a secret-managed PostgreSQL connection string.

The service resolves `/.well-known/openid-configuration` from the configured issuer, verifies exact metadata issuer, requires an HTTPS JWKS URI (or explicit localhost HTTP for local development), and pins RS256. An optional `OIDC_JWKS_URL` is an additional exact URI pin, not a client-controlled URL.

### JWKS and JWT policy

- OIDC discovery metadata and JWKS are cached in-process for up to 5 minutes.
- Unknown `kid` triggers one forced metadata/JWKS refresh before rejection.
- HTTP redirects are disabled; requests time out after 5 seconds; JSON metadata/JWKS bodies are capped at 1 MiB.
- RSA-2048 minimum + RS256 only; `kid`, `iss`, `sub`, exactly one `aud`, `exp`, `iat`, `jti`, and `team_room:connect` are required; `nbf` is validated when present. RSA exponent must be 65537, duplicate matching `kid` values are rejected, and optional `key_ops` must permit `verify`.
- Upstream token maximum is 16 KiB. No token-supplied issuer, JWKS URI, or public key is trusted.
- A new key must be published by the trusted IdP under its JWKS. The service refreshes once when a `kid` is not cached.

### Workload registration and permissions

Migration `0004_federation_security_hardening.sql` adds:
- `workload_identities`: explicit tenant/principal/issuer/subject registration with ACTIVE, DISABLED, REVOKED and EXPIRED states, last-seen timestamp, revocation timestamp and non-sensitive metadata.
- Existing enabled AI principals are backfilled as ACTIVE workload registrations; disabled principals are backfilled as DISABLED.
- `permission_catalog`: room, task, private-chat, audio and admin permission vocabulary. ADMIN is privileged and is never granted by display name or workload claim.
- `security_audit_events`: security event ledger; metadata must never contain bearer tokens, JWTs, client secrets or private key material.

The identity exchange authenticates the registered workload and creates a session without implicitly granting room membership. The subsequent join route checks `room.join`; room reads independently check `room.read`. The registry status is checked both at exchange and local-session authentication, so disabling/revoking a workload invalidates its sessions on the next request.

### Rate limiting and transport

The token exchange has a process-local limit of 10 attempts per peer IP per rolling 60 seconds, in addition to a 20,000-byte request-body limit. This is defense in depth only: configure a distributed ingress/API-gateway limit, TLS, request/connection limits, and trusted client-IP forwarding policy in production. The service does not trust arbitrary `X-Forwarded-For` headers.

### Production requirements — NOT PROVISIONED

- HTTPS issuer and JWKS only; never set `OIDC_ALLOW_HTTP_LOCALHOST` in production.
- Private ingress with TLS 1.2+ (prefer TLS 1.3), managed certificate renewal, HSTS at the public edge.
- Secret manager for IdP client credentials and database credentials; least-privilege service DB role.
- Managed PostgreSQL with encryption at rest/in transit, automated backups, restore drills, network allowlists and migrations.
- Distributed rate limiting, WAF/request-size controls, structured redacted logs, metrics, alerts and incident response.
- Keycloak HA, persistent encrypted database, admin MFA, realm export/backup, signing-key rotation and emergency key-revocation runbook.
- Upstream token revocation is not continuously introspected; local sessions remain capped at 15 minutes. Use short-lived upstream tokens and disable the workload registry entry for immediate local denial.

**Trust statement:** this federation proves that the configured Keycloak issuer signed an assertion for its service-account subject and that the subject maps to a pre-registered workload principal. It does not prove which AI model, binary, process, host or hardware produced the request. `ATTESTATION REAL — PENDENTE`. No Claude, Kimi or GPT-6 connection is claimed.


### Federation trust boundary

The local Keycloak service-account example uses OAuth 2.0 Client Credentials with a secret. This proves possession of a client credential to Keycloak, not attestation of the workload runtime. For production Kubernetes deployments, prefer a brokered workload assertion such as a projected Kubernetes service-account token validated by Keycloak's Kubernetes identity provider, or the cloud platform's equivalent workload federation. Use a dedicated service account per workload, short token lifetimes and an exact issuer/subject allowlist. Do not use the namespace's default service account. For workloads outside a supported federation environment, provision a separate managed identity adapter rather than treating a model name or shared static secret as proof.

The HTTP token exchange is limited to 10 attempts per peer address per 60 seconds in one service process. This is not a distributed quota; the ingress/API gateway must enforce the authoritative fleet-wide limit. Requests do not trust `X-Forwarded-For`. The exchange body is capped at 20 KiB and the upstream token at 16 KiB.

Private-chat authorization is only prepared in the permission vocabulary. A future private channel must enforce a server-side channel-member ACL on every read, write, search, notification, attachment, and realtime subscription. A broad `private_chat` grant alone must never disclose a channel. No private-chat, task, message, audio, or invitation endpoint exists in this service yet.


## Distributed task ownership, leases and fencing (migration 0005)

PostgreSQL is the authority for shared task ownership. The local Mission Runner is not an ownership authority.

- `team_tasks` stores mission/task identity, owner principal/participant/session/execution, lease timestamps, monotonically increasing `fencing_token`, checkpoint JSON and checkpoint version.
- Every acquire locks the task row with `SELECT ... FOR UPDATE`, validates the caller's current active session, workload execution and room membership against PostgreSQL, then increments the fencing token and commits the new owner and lease atomically.
- Lease durations are constrained to 15–300 seconds. All issue/expiry/renewal timestamps use PostgreSQL `NOW()`; client clocks are not authoritative.
- Authenticated `POST /team-room/heartbeat` updates presence/workload liveness only. It does not renew task leases. Only `POST /lease/renew` extends a task lease, with the current fencing token; a late renewal cannot resurrect an expired lease.
- `POST /team-room/{room_id}/tasks/recover` moves expired leases to `RECOVERY_REQUIRED`. It is permission-gated by `task.assign`.
- `POST /team-room/tasks` creates or idempotently returns the canonical task for the same `(tenant_id, room_id, mission_id)` (`task.write`). PostgreSQL unique index `team_tasks_tenant_room_mission_uidx` is the concurrency authority; a retry does not reset owner, fencing token, or checkpoint.
- `GET /team-room/tasks/by-mission/{mission_id}` returns the authenticated room's task state (status, owner principal/participant/session/execution, lease expiry, fencing token and checkpoint) under `task.read`; it never grants ownership.
- `POST /team-room/tasks/{task_id}/lease/acquire` acquires/reassigns a task (`task.transition`); response includes the fencing token.
- `POST /team-room/tasks/{task_id}/lease/renew`, `.../lease/release`, `.../checkpoint`, and `.../complete` require the current fencing token and the same authenticated principal, participant, session and workload execution.
- `GET /team-room/{room_id}/tasks/{task_id}/candidates` returns only currently connected, active, permitted room members with a live workload session and fewer than three active task leases. It is discovery only; selection remains an explicit acquire by the chosen authenticated participant.
- A successful acquire after expiry records a durable handoff/resume row and increments the fencing token in the same transaction. Checkpoint and completion mutations compare the current owner/session/execution and exact token while holding the task row lock.
- A PostgreSQL trigger prevents fencing-token rollback/jumps, requires a token increment when ownership changes to a new executor, keeps checkpoint versions monotonic, and makes completed/cancelled tasks immutable.
- Lease history is retained in `task_lease_events`; handoff context is retained in `task_handoffs`. The task row is the single serialization point, avoiding reliance on local locks.

### Security and operational boundaries

This is a server-side shared ownership foundation, not model-provider attestation. A valid OIDC-backed Team Room session establishes an authenticated workload identity, not proof of a particular model runtime. Candidate discovery does not automatically choose or start a provider. Provider adapters, external prompt delivery receipts, and execution attestation remain out of scope. Configure the explicit `task.write`, `task.transition`, and `task.assign` grants through the controlled permission provisioning process; default-deny remains in force.

## Communication layer (AI Team Room chat)

The existing Axum service now exposes authenticated communication routes. They reuse the same sessions, principals, participants, room_memberships, principal_permissions, PostgreSQL pool and transactional outbox_events. They do not create a second identity or task system.

### HTTP contracts

- GET /team-room/{room_id}/messages?before={RFC3339}&limit=50 — paginated general-room history; requires active room membership and room.read.
- POST /team-room/{room_id}/messages — idempotent message using client_message_id, body and optional parent_message_id; requires room.write.
- GET|POST /team-room/{room_id}/private/{other_principal_id}/messages — private conversation/history. Both principals must be active room members; only the two channel members can read/write; requires private_chat.
- GET /team-room/{room_id}/participants — authorized participant metadata and connection/execution snapshot; requires room.read.
- GET|PUT /team-room/{room_id}/presence — read or update own ONLINE|AWAY|BUSY|WAITING|BLOCKED presence; update requires presence.update. Stale heartbeat overrides visual state to DISCONNECTED; an active owned task is shown as EXECUTING.
- POST /team-room/{room_id}/leave — authenticated leave, membership transition to LEFT and session revocation; requires room.join.
- GET /team-room/{room_id}/tasks — existing distributed tasks; requires task.read. Creation continues through existing POST /team-room/tasks and task.write; ownership/fencing remains exclusively in the existing task service.
- GET|POST /team-room/{room_id}/decisions — list/create decisions; create requires decision.propose, and participant/message references are checked against room/channel ACL.
- GET|POST /team-room/{room_id}/attachments — list/create metadata references; requires attachment.read / attachment.upload; private references are filtered by channel membership.
- POST /team-room/{room_id}/ws-ticket — issue a 30-second single-use WebSocket ticket for the authenticated active room session.
- GET /team-room/{room_id}/ws?ticket=... — authenticated WebSocket upgrade; ticket is consumed once, session/membership is revalidated every 15 seconds, and the connection receives authorized PostgreSQL outbox events.

### Event delivery and idempotency

Migration 0007_team_room_communication.sql adds general/private channel metadata, persistent messages, read cursors, decisions, attachment references and one-use WebSocket tickets. Its PostgreSQL trigger emits NOTIFY team_room_outbox with only the outbox event UUID. The WebSocket process reads the durable outbox row and filters private-channel events against team_room_channel_members; notification payloads do not contain bearer credentials. Message retries with the same sender/channel/client_message_id return the canonical message and do not create a second outbox event. Reusing that key with different content returns conflict.

Migration 0008_team_room_presence_state.sql adds explicit participant presence preference. The pre-existing heartbeat remains separate from task lease renewal. Task create/lease/checkpoint/complete/recovery transitions now publish TASK_CREATED / TASK_UPDATED in the same PostgreSQL transaction as their task mutation.

### Desktop UI

The React desktop navigation includes an AI Team Room workspace with general chat, private conversation selector, thread reply reference, task creation/list, decisions, attachment references, participant/presence view and an explicit audio status panel. The user-supplied opaque Team Room session token is held only in component memory and is not persisted by this UI. OIDC token acquisition/refresh is not yet integrated into the desktop login flow.

### Explicit limitations

- WebSocket: implemented as a real Axum WebSocket endpoint backed by PostgreSQL LISTEN/NOTIFY + durable outbox. Runtime behavior is only validated if the dedicated PostgreSQL communication test passes in CI.
- Message edits/deletes: not exposed yet; MESSAGE_UPDATED and MESSAGE_DELETED are not claimed.
- Handoff events: existing task handoff lifecycle is not expanded by this communication layer; HANDOFF_CREATED / HANDOFF_UPDATED are not claimed.
- Upload: only authorized attachment metadata/reference records exist. Binary upload, object storage, malware scanning, download proxy and signed URLs are pending.
- Audio: no WebRTC/SFU/signaling or audio grants are implemented. WEBRTC_PRODUCTION = PENDING; controls remain disabled and no audio is captured.
- Unread: read cursors are stored, but aggregate unread counts and push notifications are not yet exposed.
- Presence: REGISTERED, CONNECTED and EXECUTING remain separate concepts. The API exposes connection freshness and task ownership; it does not claim a provider/model is actively responding.
