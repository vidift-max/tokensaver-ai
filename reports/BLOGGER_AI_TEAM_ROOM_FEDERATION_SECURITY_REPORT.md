# TOKEN SAVER AI — MISSION 110 FEDERATION & SECURITY HARDENING REPORT

**Repository:** `vidift-max/tokensaver-ai`  
**Branch:** `v2-saas-alpha`  
**Service:** `services/ai-team-room`  
**Report date:** 2026-11-11  
**Status:** PARTIAL — SERVICE CI PASS — DESKTOP CI PENDING — KEYCLOAK NOT RUN  
**Production decision:** NO-GO  
**Real AI connection:** NONE CLAIMED

## A. Executive summary

Mission 110 extends the Mission 109 token-exchange path with issuer-pinned OIDC Discovery, bounded discovery/JWKS caching, unknown-`kid` refresh, stronger RSA/JWT checks, explicit workload registration, a granular permission catalog, security audit events, an HTTP rate/payload limit, a loopback-only Keycloak development stack, and HTTP integration tests.

The reference Identity Provider selected is **Keycloak 26.3.5**. The repository contains a local Docker Compose definition and configuration instructions, but Keycloak has **not** been started, configured with a realm/client, or exercised end-to-end in this environment. The automated federation test uses a controlled synthetic OIDC issuer and test-only RSA keys. It is not a real external-provider deployment.

The branch is therefore **PARTIAL** until the final GitHub Actions run validates the latest code and the Keycloak environment is exercised. No Claude, Kimi, GPT-6, or other model runtime has been connected or attested.

## B. Initial state

Mission 109 supplied:
- RS256 JWT verification against a statically configured JWKS URL;
- exact configured issuer/audience validation, `exp`, optional `nbf`, `sub`, `jti`, and `team_room:connect`;
- one-time `(issuer,jti)` replay storage;
- room-scoped opaque local sessions with SHA-256 digest at rest;
- session lifecycle, local revocation/rotation/logout;
- principal permission checks and protected routes;
- integration with Mission 108 transactional admission and capacity 15.

Gaps found at Mission 110 start:
- no OIDC Discovery;
- no JWKS cache/refresh strategy or key-rotation test;
- no first-class workload registry/status;
- no permission vocabulary catalog for future private/task/audio resources;
- no dedicated HTTP router security tests;
- no rate limit;
- incomplete security audit coverage;
- no IdP instance, realm, workload federation or production infrastructure.

The required Mission 106 mission, architecture, functional contract, invitation contract and Mission 108/109 reports were consulted. Mission 106 contracts remain proposals for messaging, DMs, tasks, presence, audio and invitations; none of those runtime features were treated as existing.

## C. Provider selected

**SELECTED: Keycloak 26.3.5 — self-hosted OpenID Connect Provider.**

Reasons:
- standard OIDC Discovery document and JWKS endpoint;
- asymmetric signing keys and published key sets;
- confidential OIDC clients and service accounts using OAuth 2.0 Client Credentials;
- scopes, audience mapping, realm-specific issuer and subject;
- self-hosted/local Docker development;
- no per-seat IdP license requirement for the open-source deployment; infrastructure and operations remain the cost;
- supports a path to brokered Kubernetes service-account identity for deployments that run on Kubernetes.

Official references:
- OIDC discovery and endpoint paths: https://www.keycloak.org/docs/26.3.5/server_admin/
- Keycloak service accounts and client-credentials flow: https://www.keycloak.org/docs/26.3.5/server_admin/#_service_accounts
- OIDC endpoints, including discovery, token, JWKS and introspection: https://www.keycloak.org/docs/26.3.5/server_admin/#_oidc_endpoints
- Keycloak Kubernetes service-account identity provider: https://www.keycloak.org/docs/26.3.5/server_admin/#_identity_broker_kubernetes

## D. Alternatives analyzed

| Provider / pattern | Advantages | Disadvantages / decision |
|---|---|---|
| Keycloak | Self-hosted, OIDC discovery/JWKS, service accounts, scopes, portable local/prod deployment | **SELECTED** as reference IdP; operator must harden and operate it |
| Auth0 / Okta | Managed OIDC, reduced IdP operations | Not selected: vendor SaaS dependency and workload federation/pricing depend on plan and deployment |
| Microsoft Entra Workload ID | Strong workload federation in Azure/Kubernetes/GitHub ecosystems | Not selected as default because the TokenSaver deployment platform is not yet chosen and should remain provider-neutral |
| SPIFFE/SPIRE | Strong workload identity, SVID/mTLS and workload attestation primitives | Not selected as direct OIDC IdP: requires OIDC federation/token-exchange integration and a SPIRE control plane |
| Static API key/shared secret | Easy to bootstrap | Rejected as production identity proof; long-lived and copyable |
| Self-signed/client-provided JWT key | Easy to implement | Rejected: no prior trust binding; a client cannot choose its own trust anchor |

The local Keycloak client-secret example is a development bootstrap only. A production Kubernetes deployment should prefer a brokered service-account token or cloud workload identity assertion, with exact issuer/subject trust policy. A client secret proves possession of a credential; it does not attest a process or AI model.

## E. Federation architecture

```text
External workload credential / brokered service-account assertion
        |
        v
Keycloak (selected OIDC reference issuer; not yet provisioned)
        |
        v
OIDC Discovery from deployment-pinned issuer
        |
        v
JWKS / RS256 verification
        |
        v
issuer + subject + audience + scope + time claims
        |
        v
ACTIVE workload_identities registration
        |
        v
principal + participant mapping
        |
        v
one-time opaque local session + workload execution
        |
        v
explicit permission check (default deny)
        |
        v
room-scoped transactional admission (capacity 15)
```

The service is a resource server and token-exchange consumer. It does not issue upstream workload credentials or configure Keycloak's client secret for a workload.

## F. OIDC Discovery

**IMPLEMENTED; PARTIALLY TESTED.**

- Discovery URL is derived from the deployment-configured exact issuer: `{issuer}/.well-known/openid-configuration`.
- No issuer or discovery URL is accepted from the request/token as a trust source.
- Metadata `issuer` must exactly equal configured issuer.
- `id_token_signing_alg_values_supported` must explicitly include RS256.
- `token_endpoint` is required and must be HTTPS; `authorization_endpoint`, when present, must also be HTTPS.
- `jwks_uri` must be HTTPS.
- `OIDC_JWKS_URL`, when configured, is an optional exact URI pin and must match discovered `jwks_uri`.
- Explicit `OIDC_ALLOW_HTTP_LOCALHOST=true` permits HTTP only for localhost/127.0.0.1 development. It must never be enabled in production.
- Discovery HTTP redirects are disabled; request timeout is five seconds; metadata body limit is 1 MiB.

## G. JWKS

**IMPLEMENTED; ROTATION TESTED; CACHE-EXPIRY/WEAK-KEY/OVERSIZE TESTS PENDING.**

- Discovery metadata and JWKS are cached in-process for 300 seconds.
- Unknown `kid` forces one metadata/JWKS refresh, then fails closed if still unknown.
- HTTP redirects are disabled; request timeout is five seconds; JWKS body limit is 1 MiB.
- JWKS key count is capped at 128.
- Matching `kid` must be unique; duplicate matches are rejected.
- Only RSA signing keys with `alg=RS256` are accepted; `use`, when present, must be `sig`; `key_ops`, when present, must include `verify`.
- RSA modulus must be at least 2048 bits; exponent must be 65537.
- No token-provided JWK, `jku`, `x5u`, issuer or JWKS URI is used.
- Rotation test now uses two distinct synthetic RSA-2048 keypairs, publishes a new `kid`, confirms the new signature, and confirms the old key is rejected after the forced refresh.

Residual: a removed signing key may remain accepted from a previously cached JWKS until cache expiry or until an unknown-`kid` refresh occurs. The maximum normal stale-key window is five minutes. Emergency key compromise requires cache invalidation/restart or a future push invalidation mechanism.

## H. JWT hardening

**IMPLEMENTED; CORE NEGATIVE TESTS PASSED; CLAIM-EDGE TESTS PENDING.**

Accepted algorithm: RS256 only. Required claims: `iss`, `sub`, `aud`, `exp`, `iat`, `jti`, `scope`; `nbf` is validated when present.

Checks:
- signature verified against the issuer's trusted RSA JWKS;
- exact issuer;
- exactly one audience and it must equal configured audience;
- expiry and not-before with zero library leeway;
- `iat` cannot be more than 60 seconds in the future and must precede `exp`;
- non-empty bounded subject and JTI;
- required `team_room:connect` scope;
- required `kid`, unique matching JWK, RS256, RSA-2048 minimum, exponent 65537;
- maximum upstream JWT size 16 KiB.

Rejected: `alg=none`, non-RS256, bad signature, wrong issuer/audience, expired/not-yet-valid token, missing required claims, empty subject/JTI/scope, unknown/ambiguous key ID, weak RSA modulus, wrong JWK use/key operations.

## I. Workload identity

**IMPLEMENTED; ACTIVE/DISABLED/REVOKED/EXPIRED EXCHANGE STATES TESTED; REAL FEDERATION PENDING.**

Migration `0004_federation_security_hardening.sql` adds `workload_identities`:
- workload_id;
- tenant_id / principal_id;
- exact issuer and subject;
- status: ACTIVE, DISABLED, REVOKED, EXPIRED;
- created_at, last_seen_at, revoked_at;
- non-sensitive JSON metadata.

Existing AI principals are backfilled into the registry. A unique index enforces one participant mapping per principal per tenant. New workload exchanges require an ACTIVE matching registry row and enabled principal/participant. Local session authentication rechecks workload status on every request, so disabling/revoking the workload blocks existing sessions at the next request.

Identity layers remain distinct: external issuer/subject, principal, participant, session and workload execution. A display name or model label is never used to select a principal.

## J. Sessions

**IMPLEMENTED; REGRESSION TESTS EXIST; LATEST CI PENDING.**

Mission 109 opaque session design is retained:
- 256-bit CSPRNG bearer token returned once;
- SHA-256 digest stored, not plaintext;
- local session expiry is capped by upstream `exp` and 15 minutes;
- session/workload execution bind tenant, room, principal, participant and IDs;
- new exchange rotates previous active room/principal session;
- logout closes session and execution;
- EXPIRED/REVOKED/CLOSED sessions fail authentication;
- disabled/revoked/expired workload registry entry invalidates existing local sessions;
- replay protection remains database-backed on `(issuer,jti)`.

## K. Authorization

**PARTIALLY TESTED; RESOURCE APIs AND ADMIN OPERATIONS PENDING.**

Authentication is now separate from room grants: a registered workload can obtain a local session without receiving `room.join`. Every protected action must separately check explicit permissions.

Migration adds a `permission_catalog` with room, message, channel, task, private-chat, audio, presence, decision, attachment, audit and GitHub permission names. Privileged grants include `admin`, `decision.approve`, `participant.revoke`, `audit.read` and `github.publish`. Catalog FK prevents arbitrary permission strings. No role or display name grants ADMIN.

Existing endpoints:
- `/auth/me`: authenticated session;
- `/team-room/me`: requires `room.read`;
- `/team-room/{room_id}/join`: room binding + `room.join` + Mission 108 admission.

The catalog is a vocabulary for future endpoints; it does not mean task, DM, audio, GitHub or admin APIs are implemented.

## L. Room isolation and private resources

**PARTIAL — ROOM JOIN ISOLATION TESTED; RESOURCE-LEVEL ISOLATION NOT IMPLEMENTED.**

- Local sessions are room-scoped.
- Join rejects a path room different from the authenticated session room.
- Permission queries scope grants to the authenticated tenant/principal and the session room.
- HTTP integration test covers cross-room join denial.
- Cross-room denial and permission denial create redacted security audit events.

Not implemented: task/message/private-channel resources, search, WebSocket, notification or attachment routes. Future private chat must enforce a channel-member ACL on every read/write/search/subscribe/notification/attachment path. A general `private_chat` permission alone is not sufficient. No private chat is claimed.

## M. Security hardening

**PARTIAL IMPLEMENTATION.**

- Body limit: 20,000 bytes at router.
- Upstream JWT limit: 16,384 bytes.
- Discovery/JWKS timeout: 5 seconds.
- Discovery/JWKS JSON response limit: 1 MiB.
- JWKS maximum keys: 128.
- RSA-2048 minimum, RS256 only.
- Exchange rate: 10 requests per peer IP per rolling 60 seconds per process; stale peer entries are pruned when map size grows.
- Service does not trust `X-Forwarded-For`.
- Local Keycloak port binds only to 127.0.0.1.
- No real tokens, client secrets, API keys or personal credentials are committed.

The rate limiter is process-local, not distributed. A production ingress/API gateway must enforce the authoritative fleet-wide limit.

## N. HTTP security tests

**IMPLEMENTED; SERVICE CI TESTS PASSED ON 2026-11-11.**

New router tests cover:
- public liveness route without authentication;
- `/auth/me` and `/team-room/me` reject missing bearer token;
- exchange is disabled when trusted OIDC configuration is absent;
- exchange rate limit returns 429 after 10 attempts from one synthetic peer;
- request body limit returns 413;
- authenticated session works over HTTP;
- missing `room.read` returns 403;
- cross-room join attempt returns 403;
- same-room authorized join succeeds;
- revoked workload identity returns 401;
- revoked local session returns 401;
- denial audit rows are persisted.

PostgreSQL test fixtures use synthetic keys and tokens only. They do not represent a real external agent.

## O. Rate limiting

**IMPLEMENTED, LIMITED.**

A process-local 10-attempt / 60-second sliding window protects `POST /auth/exchange`, keyed by remote peer address. The exchange body is limited to 20 KiB. No forwarded header is trusted.

Pending: distributed rate limiting, principal-level quotas, invite throttling, messaging quotas and WAF/API-gateway configuration. The service currently has no invite or messaging endpoints.

## P. Audit

**PARTIAL IMPLEMENTATION.**

Migration adds `security_audit_events`. Implemented events include:
- successful OIDC session issuance;
- invalid credential/provider failure;
- replay rejection;
- unregistered/disabled identity;
- invalid/expired/revoked local session;
- logout/session closure;
- permission denial;
- cross-room scope denial.

Audit metadata contains IDs and reason codes only; never tokens, secrets, private keys or full assertions. Rate-limit events, upstream provider administrative events, invitation events and resource-level DM/task events remain pending.

## Q. TLS and production

**PENDING — NO PRODUCTION INFRASTRUCTURE.**

Local Keycloak compose binds only to loopback and takes bootstrap admin credentials from required environment variables. No credentials are in the compose file.

Production still requires:
- HTTPS OIDC issuer and JWKS; localhost HTTP switch disabled;
- TLS 1.2+ ingress (prefer TLS 1.3), managed certificate renewal and HSTS at edge;
- secret manager for database and IdP workload credentials;
- hardened Keycloak HA deployment, encrypted persistent database, admin MFA, backups and key rotation runbook;
- private PostgreSQL, encryption, least-privilege DB role, backup/restore drills and network allowlists;
- distributed rate limits, WAF, request/connection limits;
- structured redacted logs, metrics, alerts, monitoring and incident response.

## R. Tests

Tests added or retained:
- Mission 108 admission tests: sequential 15/16, 16 concurrent admissions, leave/re-entry/reconnect and invalid sessions.
- Mission 109 JWT and PostgreSQL tests: signature, issuer, audience, expiry, key ID, algorithm, identity mapping, session issuance, replay, concurrent replay, logout, disabled principal and expiry.
- Mission 110 discovery/JWKS rotation test with two distinct synthetic RSA keypairs.
- Mission 110 HTTP router tests for authentication, authorization, rate/body limits, room isolation, admission and revocation.

The service-specific job passed on commit `874fe6dddf5ff7a31efba488d510f841da47edce` in run `42975578184`. Its four suites reported 5/5 unit OIDC/JWT tests, 5/5 HTTP security tests, 3/3 PostgreSQL admission tests and 4/4 PostgreSQL auth/session tests; doc-tests had zero failures. The Linux and Windows foundation jobs were still in progress at the time of this report update. The report commit itself triggers a new CI run and must be checked separately. Thus service tests are **TESTED**, but full branch-head CI is not yet **VALIDATED**.

## S. Results and evidence

- CI `42974242735` for prior report commit `9a1431905d22255eeaf1405843eb5fde7ed1c5c8` was cancelled after concurrency replacement; its service job had exposed Axum `Handler` compile error `E0277` in `exchange`.
- CI `42975311859` for `667c37665aea4bd8df549056fecde51a2ee8d9e9` passed service `cargo check` but failed compiling test code: ambiguous `SocketAddr::parse` inference (`E0284`) and a moved `Router` (`E0382`). Both were fixed in `92b9cad404f7f8584dcfe53e7105da90a49371fd`.
- CI `42975578184` for `874fe6dddf5ff7a31efba488d510f841da47edce`: **ai-team-room-service SUCCESS** with all PostgreSQL/unit/HTTP test suites passing; Linux and Windows foundation jobs were still in progress at report update. https://github.com/vidift-max/tokensaver-ai/actions/runs/42975578184
- The report update creates a new commit and a new workflow; its head CI is not assumed to pass.
- Earlier Mission 109 validated run: https://github.com/vidift-max/tokensaver-ai/actions/runs/42972106698 — SUCCESS, including PostgreSQL service tests and Linux desktop foundation; Windows job was still in progress at the last recorded inspection.
- Intermediate Mission 110 attempt https://github.com/vidift-max/tokensaver-ai/actions/runs/42973080791 failed at compilation because the first rate-limit middleware signature did not satisfy Axum's `Service` bounds. It was replaced with a typed handler-level peer check in commit `595b913ac341b22abc5870a43a69618ed86ca0a3`. The final replacement still requires a successful CI run.

## T. Commits

Mission 110 code/documentation commits on `v2-saas-alpha` include:
- `cd032c30a9dd503c40be51f54f184940717e3285` — pinned OIDC discovery and cached JWKS refresh;
- `ed64b5d87de29ce64ab63c444b377038b62bde30` — update OIDC test config/claims;
- `efce1c2626e302cd9eaa15e747f4cd953aac8739` — issued-at PostgreSQL fixture;
- `44a55939c1a8464369bf75c46de84195e842d88f` — workload registry and permission catalog migration;
- `3b4f8e1fc096e83b5de93dbcb6c6d31e4ba112b5` — workload registry enforcement;
- `fe9882fe445b62fe76c2c06a66c449adde862485` — HTTP security tests;
- `4da6856b1bb42e14b9c0803b5cef54f7cd705c74` — initial exchange rate limit (superseded after CI compile failure);
- `d214747559a3b951df516fe59e69bb1af62cbd58` — local Keycloak compose;
- `cc9671342979e0e0db6003c0d885c90f347f20fa` — Keycloak federation/hardening documentation;
- `1ef077a95a63906d7c5beabb8e987ac2815b8dbc` — exact audience and auth denial audit;
- `bca651bc6cc8a658548ddf3582d378b6de0661d0` — OIDC discovery/JWKS rotation test;
- `9859ce96320c38791d5be3a7518755bb78e309f8` — permission catalog FK;
- `637d7692c0028b9510bfd6ee9eecb9126a77adc4` — expanded granular permission catalog;
- `39bc642397ed8017a1b29dd5652616a3e295a217` — authenticated HTTP admission and cross-room tests;
- `595b913ac341b22abc5870a43a69618ed86ca0a3` — replace failing Axum middleware with typed handler rate check;
- `e9b4dbe9075178d13fbbad1c5e7f735085e8a07e` — RSA key-strength and unique-kid checks;
- `7900f2c90bf118cac4248ee138623b6dd49598d8` — distinct RSA keypair rollover fixture;
- `1192297b14119e182cbbd0275e6490fcc04e75fc` — reject mismatched discovery issuer;
- `c48242a12dde84f8d20db1e2b9631018f6dbb02b` — permission denial audit;
- `6d3ae5af022a9fcae003f79cca84f0b90af64ed3` — cross-room denial audit;
- `395ca495cd4bbc804f3a0888b722d9629a9c2d9c` — assert audit and revoked-session HTTP behavior;
- `22c635c180bc5cfb50343664790cce5c641c78ae` — Keycloak image pin;
- `7297f48ccb7acf73f069a3f03080d6e27e7fc1a5` — federation trust-boundary and ACL documentation;
- `9c46b8bb94137dcca47e76da923ec1575c3fd3fa` — reject lookalike localhost OIDC token/authorization endpoints;
- `667c37665aea4bd8df549056fecde51a2ee8d9e9` — replace unsupported optional `ConnectInfo` extractor with request-extension lookup;
- `92b9cad404f7f8584dcfe53e7105da90a49371fd` — fix HTTP test type inference and clone router before repeated `oneshot`;
- `874fe6dddf5ff7a31efba488d510f841da47edce` — test all non-active workload registry states and default-deny grants.

Additional fixture and README commits are present in the branch history. This report does not claim a live IdP configuration or production deployment.

## U. External federation test

**CONTROLLED OIDC PROTOCOL TEST — IMPLEMENTED; REAL KEYCLOAK TEST — PENDING.**

The controlled test runs an HTTP issuer only on loopback, serves synthetic discovery metadata and JWKS, signs with test-only RSA keys, verifies the assertion, refreshes to a second signing key, rejects the old key after refresh, and rejects mismatched discovery issuer metadata.

The test path demonstrates protocol behavior through signature validation. It does not demonstrate a deployed Keycloak realm, real client credentials, real federated Kubernetes/cloud identity, or a vendor AI agent.

## V. Limitations

- Keycloak container has not been started or configured; no realm/client/secret is provisioned. Local validation environment has no Docker executable (`docker: command not found`), so compose/health/token/discovery/service exchange cannot be executed here.
- No real workload federation credential has been issued by an external IdP.
- Local development client-secret flow is not secretless federation.
- No Claude/Kimi/GPT model adapter, process identity, runtime attestation or hardware attestation.
- No provider revocation introspection after exchange; local session TTL remains at most 15 minutes.
- JWKS cache is process-local and stale-key invalidation can take up to five minutes.
- Rate limit is process-local, not cluster-wide.
- No invitation, message, private channel, task, presence, WebSocket, WebRTC, audio, attachment, admin or GitHub publishing endpoints.
- No TLS ingress, secret manager, hosted database, production logging/alerting or backup environment.
- HTTP tests are newly added and have not yet been validated by the final branch-head CI.

## W. Pending work

1. Wait for and fix the final branch-head GitHub Actions run; validate Linux, Windows desktop and PostgreSQL service jobs.
2. Start Keycloak using `compose.keycloak.dev.yml`, create realm/client/audience/scope, and run a live client-credentials → discovery/JWKS → service exchange → permission → admission drill.
3. Add a brokered Kubernetes/cloud workload federation configuration; avoid treating the local static client secret as production workload attestation.
4. Add tests for future `nbf`, missing/extra JWT claims, malformed/duplicate JWKs, JWKS outage, stale-key expiry and issuer endpoint failure.
5. Add distributed rate limiting and rate-limit audit/metrics.
6. Add HTTP tests for expired/invalid upstream tokens, invalid signatures, unknown `kid`, cross-tenant isolation and all session lifecycle transitions.
7. Implement resource APIs only after private channel ACL and task/message schemas are approved.
8. Keep invitation redemption pending until workload federation and administrative grants are real.
9. Provision production TLS, secrets, PostgreSQL, backups, monitoring and incident controls.
10. Obtain a separate attestation design if proof of a specific model/runtime is required.

## X. Risks

- **Issuer compromise:** attacker can mint assertions; protect Keycloak admin plane, signing keys and realm database.
- **Client-secret theft:** local development service-account secret is copyable; use short-lived federation in production.
- **Stale JWKS cache:** key removal may take up to five minutes to propagate.
- **Token theft:** opaque bearer sessions require TLS and secure client storage.
- **Cross-room/tenant IDOR:** only currently implemented room routes are covered; future resource APIs need tenant + room + resource ACL on every path.
- **Permission expansion:** catalog entries do not themselves grant access; grant administration must remain controlled and audited.
- **Process-local rate limits:** multi-replica deployments need a shared limiter.
- **False agent identity:** issuer + subject proves the configured issuer's assertion, not a model's runtime.
- **Audit gaps:** some edge failures and rate limits are not yet persisted.
- **Operational exposure:** local Keycloak development mode is not a production deployment.

## Y. Acceptance criteria

| Criterion | Status |
|---|---|
| Provider evaluated and selected | VALIDADO DOCUMENTALMENTE — Keycloak 26.3.5 |
| Local IdP configuration prepared without embedded credentials | IMPLEMENTADO; NOT RUN |
| OIDC discovery pinned to configured issuer | IMPLEMENTADO; CI PENDING |
| Authorization/token/JWKS endpoint validation | IMPLEMENTADO; CI PENDING |
| JWKS cache and bounded refresh | IMPLEMENTADO; CI PENDING |
| Real distinct-key rotation test | IMPLEMENTADO; CI PENDING |
| JWT RS256, issuer, exact audience, exp, nbf, iat, jti, scope | IMPLEMENTADO; CI PENDING |
| Workload registry and lifecycle status | IMPLEMENTADO; CI PENDING |
| Session lifecycle/revocation/replay | RETAINED; TEST COVERAGE EXISTS; CI PENDING |
| Granular permission catalog / no implicit ADMIN | IMPLEMENTADO; CI PENDING |
| Cross-room HTTP denial and audit | IMPLEMENTADO; CI PENDING |
| HTTP authentication, payload and rate-limit tests | IMPLEMENTADO; CI PENDING |
| Distributed rate limiting | PENDENTE |
| Real Keycloak federation exchange | PENDENTE |
| Real cloud/Kubernetes workload federation | PENDENTE |
| Model/runtime attestation | PENDENTE |
| Private chat ACL enforcement | PENDENTE |
| Invitations | PENDENTE |
| Production TLS/secrets/DB/backup/observability | PENDENTE |
| Latest Linux/Windows/PostgreSQL CI | PENDENTE — DO NOT CLAIM PASS |

## Z. GO / NO-GO

**NO-GO PARA MISSÃO 111 / PRODUÇÃO.** O job do serviço e os testes críticos de serviço passaram no run `42975578184`, mas a CI Linux/Windows completa ainda estava em andamento no momento deste registro. O ambiente local não possui Docker, impedindo o teste real de Keycloak. Permanecem lacunas de cobertura para cache expiry, chave RSA fraca/ambígua, `nbf` futuro/claims ausentes, concorrência/distribuição do rate limit, isolamento de recursos sem API e auditoria de todos os eventos.

**GO limitado apenas para continuar a validação da Missão 110**, não para avançar à Missão 111. Produção, convites, chat privado e onboarding de IA continuam NO-GO.

**ATTESTATION REAL — PENDENTE.**

No Claude, Kimi, GPT-6 or other AI agent is connected. A controlled synthetic issuer test is not a real AI connection.

## AA. Mission 110 closure verification addendum — 2026-11-11

### Evidence classification

| Area | Classification | Evidence / gap |
|---|---|---|
| OIDC Discovery | TESTED / PARTIAL | Exact issuer mismatch and insecure lookalike HTTP endpoint rejection tests pass; discovery timeout, malformed JSON, redirect and every endpoint shape are not separately tested. |
| JWKS cache | IMPLEMENTED / PARTIAL | Cache and unknown-kid forced refresh exercised by rotation test; 300-second expiry, oversize response, timeout and malformed JWKS tests remain pending. |
| Key rotation | TESTED | Test signs with two distinct RSA-2048 keypairs, accepts new kid after refresh and rejects old kid. |
| RSA/JWT hardening | TESTED / PARTIAL | RS256 success, non-RS256 rejection, issuer, audience, expiry, invalid signature, unknown kid and replay are tested; weak/ambiguous JWK, future nbf and missing/extra claim matrix remains incomplete. |
| Workload registry | TESTED | ACTIVE exchange succeeds; DISABLED, REVOKED and EXPIRED exchange states reject. Existing-session denial is covered for REVOKED; complete per-state session regression matrix remains pending. |
| Permissions | TESTED / PARTIAL | Explicit room.join/read work; absent admin, task.write, private_chat and audio.publish grants are denied by permission helper. No admin/task/DM/audio resource endpoints exist, so no operation-level privilege-escalation test is possible yet. |
| Room isolation | TESTED / PARTIAL | Cross-room join is denied and audited. Cross-room message/task/private-chat/audio/admin operations cannot be tested because those APIs do not exist. |
| Admission capacity 15 | TESTED | PostgreSQL integration tests: 15 sequential accepted, 16th rejected; 16 concurrent requests yield exactly 15 accepted; leave, re-entry, reconnect and duplicate active session tested. |
| Rate limiting | TESTED / PARTIAL | HTTP test confirms 11th sequential exchange attempt from same peer gets 429. Concurrent load and multiple processes/replicas are not tested; limiter remains process-local and bypassable across processes. |
| Audit | TESTED / PARTIAL | Permission denial and cross-room denial audit rows are asserted; replay and session lifecycle paths create audit events in code. Full event matrix and redaction/secret-leak automated scan are pending. |
| Keycloak | BLOCKED | Docker is absent in the local execution environment. Compose exists, but container health, realm/client setup, token issuance and live exchange were not run. |
| Real AI federation / runtime attestation | PENDING | No Claude, Kimi, GPT-6 or actual AI workload is connected or attested. |
| Full CI | PARTIAL | Service job succeeded in run `42975578184`; Linux and Windows foundation jobs had not completed at report update. The report commit starts another CI run. |

### CI corrections discovered during closure

- `E0277` in the Axum exchange handler was fixed by reading `ConnectInfo<SocketAddr>` from request extensions and parsing the bounded request body explicitly.
- `E0284` in HTTP tests was fixed with explicit `parse::<SocketAddr>()` annotations.
- `E0382` in the HTTP test router was fixed by cloning the router before the second `oneshot` call.
- The endpoint-validation bug allowing `localhost.*` lookalike HTTP hosts was fixed by applying the strict loopback parser to token and authorization endpoints.

### Environment and final decision

`docker` is not installed in the local runner, and `cargo` is also unavailable locally; Rust compilation and tests were executed by GitHub Actions. The service CI job is objectively green, but full desktop CI and live Keycloak validation remain outstanding. **Final decision: NO-GO for Mission 111.** Do not interpret this as a failed service implementation; it is a deliberately conservative acceptance decision because the mission's full acceptance criteria are not yet met.