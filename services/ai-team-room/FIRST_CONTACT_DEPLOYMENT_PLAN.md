# Phone-02 First-Contact Deployment Plan

## Decision

For the first-contact server, do not create a second application server.

Use the existing `services/ai-team-room` Axum service and deploy the required stack to a persistent VM-style host rather than relying on an expiring managed PostgreSQL free tier.

The preferred free-first target is an Oracle Cloud Always Free compute VM, subject to account availability and regional capacity. Oracle documents Always Free compute resources for the tenancy's home region. The deployment must remain inside the provider's documented Always Free limits.

## Why this target

- The project already has a Dockerfile for `ai-team-room-service`.
- The service requires PostgreSQL and an OIDC issuer.
- A VM allows the three components to be operated together without depending on a 30-day database expiration.
- All setup can be performed from the user's Android tablet through a web console and GitHub.
- No Windows computer is required for server deployment.

## Planned server stack

1. `ai-team-room-service` — Rust/Axum API and WebSocket endpoint.
2. PostgreSQL 16 — persistent system of record.
3. Keycloak — OIDC issuer for authenticated workload/session flows.
4. Reverse proxy/TLS ingress — only the required public endpoints.
5. Docker Compose — process supervision and service networking.

## Isolation

Only the reverse proxy/API ingress is public.

- PostgreSQL: private Docker network only.
- Keycloak: private network unless a specific bootstrap/admin route is required.
- AI Team Room: public HTTPS endpoint.
- Administrative interfaces: not exposed publicly unless explicitly required and protected.

## First-contact acceptance

Deployment alone is NOT a first-contact PASS.

The operational sequence remains:

1. prove API liveness;
2. prove PostgreSQL readiness;
3. configure the trusted OIDC issuer;
4. register distinct Phone-01 and Phone-02 AI principals;
5. create authenticated sessions;
6. bind both participants to the same room;
7. send Phone-01 -> Phone-02 message;
8. receive it on Phone-02;
9. send Phone-02 -> Phone-01 response;
10. preserve database evidence with timestamps and identifiers.

## Tablet-only constraint

The deployment procedure must not require:

- Windows;
- a local desktop IDE;
- PowerShell;
- local Docker Desktop;
- physical Phone-02 execution during the server-construction phase.

Physical Phone-02 execution remains a separate gate. If the existing Phone-02 runtime requires Windows, that gate stays BLOCKED until an appropriate execution environment exists.

## Security constraints

- No automatic GitHub mutation.
- No automatic publishing.
- No external AI-provider access merely because a participant joins the room.
- Secrets must not be committed to Git.
- Default-deny room permissions remain in force.
- Do not expose PostgreSQL publicly.
- Do not expose Keycloak administration publicly unless explicitly required for bootstrap and protected.

## Important caveat

Oracle Always Free availability is provider-controlled and can vary by region/account capacity. This document records it as the preferred target, not as proof that an account, VM, public IP, DNS name, or deployed service already exists.

No deployment has been performed by this commit.
