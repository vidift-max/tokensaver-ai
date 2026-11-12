# Security and Distribution Configuration

## Status

This document describes the security boundary implemented by the Mission 7 hardening branch. It is not a production deployment guide and does not certify a signed release.

## Environments

The Team Room service accepts these explicit values for `TEAM_ROOM_ENV`. If the variable is absent, the service defaults to `production` and requires valid OIDC configuration before opening PostgreSQL or applying migrations:

| Environment | OIDC requirement | HTTP issuer |
|---|---|---|
| development | Optional | Only with `OIDC_ALLOW_HTTP_LOCALHOST=true` and loopback issuer |
| test | Optional | Only synthetic/local test setup |
| staging | Mandatory at startup and readiness | Not allowed except no override; use HTTPS |
| production | Mandatory at startup and readiness | Not allowed except no override; use HTTPS |

Unknown environment names fail startup. Do not use `development` or `test` for a deployed service. Developers and CI must opt in explicitly by setting `TEAM_ROOM_ENV=development` or `TEAM_ROOM_ENV=test`.

Staging/production require valid `OIDC_ISSUER` and `OIDC_AUDIENCE`; `OIDC_JWKS_URL`, when supplied, must use HTTPS. Do not place OIDC client secrets, private keys, bearer tokens, or database passwords in source files, mission state, screenshots, logs, or release artifacts. Inject operational credentials through the deployment secret manager.

## Desktop and Team Room

The desktop Team Room connection is disabled in this build. The former UI accepted an arbitrary API URL and a manually pasted bearer token; the desktop does not yet have OIDC Authorization Code + PKCE, secure OS credential storage, token refresh, or a trusted endpoint allowlist.

The remote Runner client remains separate. It accepts only an HTTPS origin, except explicit loopback HTTP for local development; rejects URL userinfo, path, query and fragment; constrains request paths to the configured origin; rejects redirects; times out requests; and clears its in-memory token on HTTP 401.

Do not re-enable desktop Team Room networking until all of the following are implemented and tested:
1. native OIDC Authorization Code + PKCE with state and nonce validation;
2. trusted issuer and API origin supplied by signed deployment configuration, not a user-editable field;
3. tokens held in Windows Credential Manager / OS keychain, never localStorage or project SQLite;
4. refresh, logout, revocation, expiry and device/session lifecycle;
5. HTTPS/WSS only outside explicit local development;
6. restrictive CSP and exact-origin CORS;
7. WebView integration tests proving bearer tokens are not sent to an untrusted origin.

## Installer, signing and updates

The CI pipeline builds an NSIS installer and uploads it as an artifact explicitly named `UNSIGNED` for internal QA only. This artifact is not approved for customer distribution.

Code signing is **PENDING — EXTERNAL DEPENDENCY**: an owner-controlled Authenticode certificate, secure certificate custody, timestamping endpoint, and protected CI environment/secrets are required. No signing identity or signature is fabricated by this repository.

The Tauri updater is **PENDING**. No updater plugin, signed update manifest, update endpoint, rollback protocol, or downgrade protection is configured. Do not enable automatic updates or distribute an updater package until signing and update verification are implemented.

Rollback is currently manual: uninstall the test build and install a previously retained, independently verified build. This is not an automatic or production rollback mechanism.

## Build integrity and secrets

The source archive is produced with `git archive`, so only tracked files enter the ZIP. The repository's synthetic RSA private keys are test fixtures used by OIDC tests; they are not production credentials and must never be reused. The Mission 7 audit's GitHub code searches are scoped pattern searches, not a complete historical secret scan, Gitleaks run, SAST, SCA, or SBOM.

Before a customer release, run a full-history secret scan and dependency/security analysis, pin third-party GitHub Actions to immutable commit SHAs, generate an SBOM, sign the installer, and verify the signature on a clean Windows machine.
