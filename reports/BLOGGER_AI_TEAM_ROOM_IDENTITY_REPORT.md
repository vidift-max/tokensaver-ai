# TOKEN SAVER AI — MISSION 109 IDENTITY & WORKLOAD AUTHENTICATION REPORT

Branch: `v2-saas-alpha`
Service: `services/ai-team-room`
Report status: IMPLEMENTATION PRESENT; SECURITY REVIEW PARTIAL; PRODUCTION NO-GO
Review date: 2026-11-11

## A. Executive summary

The branch already contains a working OIDC/JWKS workload-token validation path, one-time assertion exchange, opaque room-scoped local sessions, PostgreSQL replay protection, permission checks, protected HTTP routes, and integration with Mission 108 admission. The implementation is not a vendor-specific Claude/Kimi/GPT connector and does not prove that a particular model process is executing. It authenticates a workload subject only to the trust actually established by the configured upstream issuer.

The Mission 109 implementation is **IMPLEMENTED** and its service tests plus Linux/Windows desktop CI were **VALIDATED** on commit `605ea4eba1cf28de29b251137f196a2496bd76ec` (run 42971073655). The current branch head later gained documentation-only changes; its newest CI run 42971450340 was still in progress at the time of this report.

Operational readiness is **NO-GO**: no external issuer/federation, production JWKS, hosted PostgreSQL, secret manager, TLS ingress, rate limiting, or real AI workload has been provisioned.

## B. Initial state

Mission 108 provided the independent Axum/Tokio service, PostgreSQL migrations, participants/principals/sessions, workload execution binding, audit/outbox, and transactional admission with hard capacity 15. The previous implementation had no trusted external token exchange or protected identity routes.

The Mission 108 admission function remains the final capacity authority. Its PostgreSQL room-row lock and capacity constraint were retained.

## C. Selected identity-provider strategy

**SELECTED — OIDC-compatible signed JWT access-token validation using a deployment-pinned issuer, exact audience, and that issuer's HTTPS JWKS.** The service is a resource server and token-exchange consumer. It does not issue upstream workload credentials.

The service accepts RS256 only. It validates the JWT signature against an RSA public key selected by `kid` from the configured JWKS, then validates exact issuer, audience, expiry, not-before when present, non-empty subject/JTI, and the `team_room:connect` scope.

## D. Alternatives analyzed

| Alternative | Advantages | Disadvantages / decision |
|---|---|---|
| OIDC JWT + JWKS | Standard issuer/subject/audience/expiry claims; asymmetric verification; key rotation; can federate cloud workload identity | **SELECTED**; requires a genuine trusted upstream issuer |
| OAuth 2.0 introspection | Central active/revoked check and opaque tokens | Not primary; online dependency and trusted client bootstrap still required; possible future adapter |
| Static API keys/shared secrets | Simple | Rejected: long-lived, copyable, hard to attribute and rotate safely |
| Self-signed JWT/client-supplied key | Asymmetric signature | Rejected: no prior trust binding, so an attacker can self-register a key |
| mTLS/SPIFFE | Strong service identity and short-lived certificates | Future option; no SPIRE, CA, or workload platform configured |
| Hardware/runtime attestation | Can bind identity to measured runtime | Pending; no attestation provider or measurement policy |

## E. Architecture

1. Deployment supplies `OIDC_ISSUER`, `OIDC_AUDIENCE`, and `OIDC_JWKS_URL`.
2. Workload obtains a short-lived upstream access-token JWT from a trusted federation/issuer.
3. `POST /auth/exchange` verifies the assertion and consumes its `(issuer,jti)` once in PostgreSQL.
4. Exact issuer + subject maps to an enabled AI principal and participant.
5. The service issues a random 256-bit opaque bearer token; only its SHA-256 digest is stored.
6. The local session and workload execution are bound to tenant, room, principal, participant, and each other's IDs.
7. Protected endpoints authenticate the opaque token, then independently check the required database permission.
8. Join calls the Mission 108 transactional admission primitive; PostgreSQL continues to enforce 15 active AI memberships.

## F. Identity

**IMPLEMENTED.** Display name is metadata only. External identity is resolved from verified issuer + subject. The client cannot submit a principal/participant/session/execution ID to choose its identity.

## G. Principal

**IMPLEMENTED.** A principal is looked up by exact `provider = issuer` and `external_subject = sub`, with `principal_kind = AI` and enabled state. It must belong to an active room tenant and have an enabled participant record.

## H. Workload

**PARTIAL.** A `workload_execution` row is created per accepted exchange and linked by composite database relationships to participant, principal, session, room, and tenant. Its `worker_subject` is issuer + subject. This is an auditable workload-session binding, **not** proof of a specific model binary, process, host, container, or hardware. No Claude/Kimi/GPT execution attestation is claimed.

## I. Sessions

**IMPLEMENTED.** Lifecycle states in the schema: CREATED, ACTIVE, EXPIRED, REVOKED, CLOSED. Issuance creates ACTIVE only after successful upstream verification and database checks. Session expiry is capped at the earlier of upstream `exp` and 15 minutes. Logout closes the session and linked execution. A new assertion for the same principal/room rotates the prior active session and revokes the prior execution.

## J. Authentication

**IMPLEMENTED.** `POST /auth/exchange`, `GET /auth/me`, and `POST /auth/logout`. Local bearer token is generated server-side using 32 random bytes; plaintext is returned once and is not persisted. The local token is not a JWT and its digest is stored in PostgreSQL.

## K. Authorization

**IMPLEMENTED, LIMITED SCOPE.** `principal_permissions` is default-deny. `GET /team-room/me` requires `room.read`; `POST /team-room/{room_id}/join` requires `room.join` and enforces that the path room matches the session room. No role name grants privileges. Grant-management HTTP endpoints are not exposed. Future permissions are schema-compatible but not implemented as endpoints.

## L. Revocation

**IMPLEMENTED LOCALLY.** Logout and session replacement revoke local sessions and workload executions. Disabled principals/participants are rejected on authentication. Upstream issuer revocation is not introspected after exchange; the local maximum 15-minute TTL bounds this residual window.

## M. Rotation

**IMPLEMENTED.** New upstream assertion rotates the active room-scoped local session. JWKS is fetched from the statically configured URL for each exchange, so a newly published key can be observed without trusting a token-provided URL. No production issuer key-rotation drill has been run.

## N. Replay protection

**IMPLEMENTED.** PostgreSQL primary key on `(issuer, token_jti)` consumes each assertion once. Concurrent exchange test proves exactly one of two simultaneous exchanges using the same assertion succeeds. Expired replay rows are pruned during exchange.

## O. PostgreSQL integration

**IMPLEMENTED.** Migration `0003_identity_auth_sessions.sql` adds session lifecycle/token digest fields, workload execution status, replay table, and permission table. The service persists replay consumption, session issuance/rotation, execution binding, and grants centrally.

## P. Admission integration

**IMPLEMENTED.** `POST /team-room/{room_id}/join` authenticates local session, checks room binding and `room.join`, then calls `admit_authenticated_participant`. The original PostgreSQL room lock and hard cap 15 remain unchanged.

## Q. Tests

**IMPLEMENTED / TESTED.** Service test suite contains:
- JWT verification tests for valid signature, expired token, wrong issuer, wrong audience, unknown `kid`, invalid signature, and rejection of algorithms other than RS256.
- PostgreSQL integration flow: signed identity → session → permission → admission → replay rejection → logout/revocation.
- Concurrent duplicate assertion exchange: exactly one winner.
- No-grant principal and disabled principal rejection.
- Expired upstream assertion rejected before session creation.
- Local session expiry, session rotation, old-session invalidation, and unknown subject rejection.
- Mission 108 admission tests remain in place: 15-seat cap, 16 concurrent admissions, exit/re-entry, duplicate session, and disabled/expired/unbound identity rejection.

**PARTIAL coverage:** there are no dedicated HTTP-router tests for `/auth/me`, `/team-room/me`, room-path mismatch, or `/join` authorization failures. No explicit tests cover future `nbf`, malformed/missing `jti`, token-size limits at the HTTP layer, key rotation across multiple JWKS versions, cross-tenant IDOR, or provider-side revocation propagation.

## R. Results

- JWT signature / issuer / audience / expiry / key ID / algorithm validation: **TESTED**.
- Identity lookup and disabled/nonexistent identity rejection: **TESTED**.
- Session issuance, expiry, rotation, closure, local revocation: **TESTED**.
- Replay and concurrent replay: **TESTED**.
- Authorization grants and default-deny behavior: **TESTED**.
- Full identity → session → permission → admission flow: **TESTED**.
- Capacity 15 regression: **TESTED** in Mission 108 integration tests.
- External provider/federation and real AI workload: **PENDENTE / NÃO TESTADO**.
- Production deployment and secrets integration: **PENDENTE / NÃO TESTADO**.

## S. Evidence

- Validated code CI run: https://github.com/vidift-max/tokensaver-ai/actions/runs/42971073655
  - Commit: `605ea4eba1cf28de29b251137f196a2496bd76ec`
  - Linux foundation: SUCCESS (Rust check/test, npm build, source artifact)
  - Windows foundation: SUCCESS (Rust check/test, npm build)
  - PostgreSQL 16 service job: SUCCESS (cargo check + cargo test)
- Newer documentation-head run: https://github.com/vidift-max/tokensaver-ai/actions/runs/42971450340
  - Head: `db21b870930b869b179d9adaff8ee1438c62de6f`
  - Status at report creation: IN_PROGRESS; do not treat as passed until GitHub reports completion.

## T. Commits

Mission 109 implementation commits found on `v2-saas-alpha`:
- `4adde822c2bf28dc9b0971bf969775449c65704c` — OIDC verification dependencies.
- `d474a8cb8b4721606f4ca0e5c119852423bcf6ce` — revocable sessions and authorization schema.
- `3b9fa35383c32ab0379c211480815fe78f52d5fa` — OIDC workload verification and scoped sessions.
- `71f82e47e1be2a2cadc9fcb7814bc02ba323a45a` — trusted identity module exposure.
- `35b56d2a5b5dda7d0756ac2014b97d6e92c5f6de` — clarify upstream credential naming.
- `db21b870930b869b179d9adaff8ee1438c62de6f` — clarify workload access token exchange contract.

These are existing real repository commits; this report does not attribute their authorship to this review.

## U. Limitations

- No real OIDC provider has been configured.
- No hosted PostgreSQL or production environment exists.
- No TLS ingress, secret manager, rate limiter, or production log-redaction configuration has been deployed.
- No real Claude, Kimi, GPT-6, QA or other agent is connected.
- No runtime/hardware attestation.
- No immediate upstream revocation introspection.
- JWKS is fetched on every exchange; no bounded cache or production key-rotation exercise.
- Local session bearer-token theft remains possible if the client device or transport is compromised; TLS and secure client storage are mandatory.
- The committed RSA private key in auth tests is a test fixture only; it must never be reused by a deployment or issuer.

## V. Pending

1. Configure an actual workload federation issuer and bind its subject to approved workload identities.
2. Provision PostgreSQL, secret manager, TLS ingress, request/rate limits, and redacted access logs.
3. Add HTTP router tests and explicit cross-room/cross-tenant impersonation/IDOR tests.
4. Add JWKS cache/timeout/size limits and production key-rotation tests.
5. Decide whether the deployment needs SPIFFE/mTLS or hardware/runtime attestation.
6. Implement invitation validation only after trusted identity and admin authorization are deployed.
7. Add a controlled, audited permission-grant administration path.

## W. Risks

Threats considered:
- impersonation and client-selected IDs: server derives identity from verified issuer/subject and DB mappings;
- token theft: short-lived opaque token, digest at rest, TLS and secure storage still required;
- replay: unique issuer/JTI database key and concurrency test;
- confused deputy / privilege escalation: upstream connect scope does not grant GitHub, code, DB, publishing, or admin powers; protected routes check explicit grants;
- session fixation: session/token IDs generated server-side;
- weak algorithm / key confusion: RS256 allowlist, RSA JWKS key, kid required;
- issuer/audience confusion: deployment-pinned exact values;
- IDOR / cross-room: session room binding and room-scoped permission queries;
- credential leakage: no production secrets in code; upstream/local tokens are not stored plaintext or included in audit metadata. Reverse proxy logs still require deployment configuration;
- upstream revocation gap: bounded by local session TTL (maximum 15 minutes);
- false workload attestation: explicitly not claimed.

Residual risk remains until infrastructure and HTTP-level security tests are complete.

## X. Acceptance criteria

| Criterion | Status |
|---|---|
| Cryptographic JWT validation and allowlisted algorithm | TESTADO |
| Exact issuer and audience | TESTADO |
| Expiration and not-before validation logic | IMPLEMENTADO; future-`nbf` test PENDENTE |
| Unknown key ID and invalid signature rejected | TESTADO |
| Identity resolved from issuer + subject, not display name | TESTADO |
| Separate participant/principal/session/execution IDs | TESTADO |
| Session expiry/rotation/revocation/closure | TESTADO |
| Assertion replay and concurrent replay protection | TESTADO |
| Explicit authorization grants | TESTADO |
| Protected identity and room routes | IMPLEMENTADO; HTTP router tests PARCIAL |
| Integration with Mission 108 admission | TESTADO |
| Hard cap 15 preserved | TESTADO |
| Real external workload issuer | PENDENTE |
| Attestation of actual AI runtime | PENDENTE |
| Production deployment/security controls | PENDENTE |

## Y. GO / NO-GO

**GO — next engineering phase:** configure and validate one real workload identity provider/federation in a controlled non-production environment, then add HTTP-level adversarial tests and operational controls.

**NO-GO — production or real AI onboarding:** no external issuer, workload federation, hosted DB, secret manager, TLS ingress, or real AI workload is configured. A passing CI proves the code and test fixtures work; it does not prove a provider has authenticated Claude/Kimi/GPT or that those agents are connected.

Recommended next mission: **MISSION 110 — External Workload Federation & Security Hardening**, covering a selected real issuer, trust-policy configuration, JWKS cache/timeouts, route-level adversarial tests, cross-tenant isolation, deployment secrets, TLS, rate limits, and a staged identity-exchange drill. Invitations should follow only after that gate.
