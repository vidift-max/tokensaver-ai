# AI Team Room Service — Architecture Record

Status: MISSION 109 AUTHENTICATION FOUNDATION IMPLEMENTED; EXTERNAL TRUST AND PRODUCTION DEPLOYMENT PENDING.

## Service boundary

Standalone Rust/Axum process under `services/ai-team-room`; not a Tauri plugin or desktop-hosted server. PostgreSQL is the shared source of truth. The desktop remains a future HTTPS client.

## Identity-provider decision

**Selected:** standards-based OIDC JWT workload assertion validation using a single explicitly configured issuer, fixed API audience, and that issuer's HTTPS JWKS. The service acts as a resource server and one-time assertion exchange endpoint. It does not issue external/provider credentials.

**Why:** AI providers and execution platforms do not share one universal workload identity API. A static trust boundary (issuer + audience + JWKS URL) allows a future adapter or federation broker to authenticate supported workloads without pretending that a Claude/Kimi/GPT display name is a credential. RS256 is deliberately allowlisted; the configured issuer rotates public keys through JWKS.

### Alternatives considered

| Alternative | Advantages | Disadvantages / decision |
|---|---|---|
| OIDC JWT + JWKS | Standard issuer/subject/audience/expiry model; asymmetric signatures; key rotation; federates with cloud workload identity | Selected. Requires an upstream issuer that can genuinely authenticate the workload; vendor support is not assumed. |
| OAuth 2.0 introspection | Central revocation and opaque tokens; issuer controls active state | Not selected as the primary verifier because it adds an online introspection dependency per assertion and still needs a trusted client credential / workload bootstrap. Can be added for providers that only issue opaque tokens. |
| Static API keys / shared secrets | Simple integration | Rejected: long-lived secrets are copyable, weakly attributable, hard to scope/rotate safely, and unsuitable for agent identity. |
| Self-signed JWT or client-supplied public key | Asymmetric signatures | Rejected: without a prior trust binding, anyone can register a key and impersonate a name. |
| mTLS / SPIFFE-style workload identity | Strong service-to-service identity; short-lived X.509 SVIDs and workload attestation integrations | Valuable future option for controlled infrastructure. Not selected now because no SPIRE/cloud workload platform or certificate authority is provisioned. |
| Hardware/runtime attestation | Can bind execution to measured environment | PENDING: no attestation provider or trusted measurement policy exists. |

## Trust and token flow

1. Deployment config pins `OIDC_ISSUER`, `OIDC_AUDIENCE`, `OIDC_JWKS_URL`; issuer and JWKS URL must be HTTPS.
2. Workload obtains a short-lived upstream JWT from a trusted federation/identity provider. Required claims: exact `iss`, stable `sub`, matching `aud`, `exp`, optional `nbf`, unique `jti`, and `scope` containing `team_room:connect`.
3. Service decodes only RS256, requires `kid`, fetches configured JWKS, selects matching RSA signing key, verifies signature and claims. Token-provided issuer/JWKS URLs are never followed.
4. In one PostgreSQL transaction, consume `(issuer,jti)` once, map exact issuer+subject to enabled AI principal + participant, require room-scoped `room.join`, revoke prior active session/execution, create new session and execution.
5. Generate 32 random bytes for a local opaque bearer token; persist only SHA-256 digest. Return token once. Session is bound to room/principal/participant/execution and expires at min(upstream exp, now+15 minutes).
6. Protected requests hash the presented local token and load active session, participant, principal and execution binding from PostgreSQL. Authorization separately checks a permission grant.
7. Join checks that path room equals session room, checks `room.join`, then calls Mission 108 admission. PostgreSQL room-row lock and hard cap 15 remain authoritative.

## Identity mapping

- `principal_id`: stable registered identity, unique by tenant + provider issuer + external subject.
- `participant_id`: enabled room participant record attached to that principal; display name is metadata only.
- `session_id`: unique local bearer session, scoped to one room/principal.
- `execution_id`: one workload execution instance bound by composite FKs to tenant/room/participant/principal/session and worker subject.

No request accepts caller-supplied principal_id, participant_id, session_id or execution_id for authentication. Those identifiers are derived from the verified token mapping and server-side rows.

## Session lifecycle

`CREATED → ACTIVE → EXPIRED | REVOKED | CLOSED`. Issuance creates ACTIVE directly after all checks in the transaction; CREATED remains a reserved lifecycle state. Expired sessions are marked EXPIRED on the next token use. Logout marks session and execution CLOSED. A new exchange for the same principal/room revokes the previous active session/execution and records `rotated_from_session_id`. A replayed upstream `jti` is rejected by a unique PostgreSQL key.

## Authorization

Default deny. Implemented permission strings include `room.join` and `room.read`; the schema is generic for future `message.create`, `task.create`, `task.assign`, `channel.read`, `audio.publish`, `audio.subscribe`, `audit.read`, and administrative actions. No role name grants permissions. Grant management is intentionally not exposed over HTTP.

## HTTP routes

- `GET /health/live`, `GET /health/ready`
- `POST /auth/exchange`
- `GET /auth/me`
- `POST /auth/logout`
- `GET /team-room/me` (requires `room.read`)
- `POST /team-room/{room_id}/join` (requires `room.join`, session-room match, Mission 108 transaction)

No invite redemption or administrative endpoints are present.

## Security boundaries and residual threats

- RS256 only; rejects HS/none/other algorithms, missing/unknown kid, non-RSA JWK, wrong issuer/audience, expired/not-yet-valid token, missing subject/jti/scope, invalid signature.
- JWKS URL is trusted config, not a JWT header/claim. No arbitrary URL fetch from user input.
- Upstream assertion replay: unique issuer+jti consume in same DB transaction as local session issuance.
- Local token theft: 256-bit opaque token, SHA-256 digest at rest, short TTL, logout/reconnect revocation; TLS and client secure storage still required.
- Session fixation: local token generated server-side after successful upstream verification; no caller-selected session IDs.
- Impersonation/IDOR: identity IDs derived server-side; room path must equal session room; permission checks use principal + tenant + room.
- Confused deputy/privilege escalation: upstream scope only permits exchange; local grants separately authorize room actions; no external service privileges inferred.
- Issuer/audience confusion: exact deployment-pinned values and one configured trust domain.
- Credential leakage: no tokens in logs/audit/DB plaintext; local token returned once; only hash stored. Ensure reverse proxy access logs redact exchange bodies and Authorization headers.
- Cross-room access: sessions are room-scoped and permission queries constrain room/global grant. All future resource APIs must apply tenant + room + resource ACL.
- Revocation gap: upstream revocation after exchange is not immediately introspected; local session max 15 minutes and explicit local revocation are current controls.
- Workload authenticity: depends on how upstream issuer issues the assertion. No proof of model process, runtime, or hardware attestation is claimed.

## Operational requirements

Configure issuer federation to bind the expected workload identity (not a public client secret), audience restricted to this API, short upstream token TTL, JWKS key rotation overlap, secret manager for database credentials, TLS/private ingress, rate limits, body limits, secure token storage, DB least privilege, monitoring, backups and incident response. Never place credentials in GitHub, source, invitations, or messages.
