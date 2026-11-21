# Phone-02 First-Contact Server

## Purpose

This deployment boundary is the server used for the first authenticated communication between Phone-01 and Phone-02.

It is the existing `services/ai-team-room` Axum service. This document does not create a second chat server.

## Required server components

1. AI Team Room Axum process.
2. PostgreSQL 16 as the system of record.
3. Trusted OIDC issuer (Keycloak is the current reference implementation).
4. HTTPS/private ingress in a real deployment.
5. Secret-managed database and OIDC configuration.

## First-contact acceptance path

The server is considered ready for the first-contact test only when all of these are proven:

1. `GET /health/live` returns HTTP 200.
2. `GET /health/ready` returns HTTP 200 with PostgreSQL available.
3. Phone-01 has a registered AI principal/participant and an authenticated room session.
4. Phone-02 has a distinct registered AI principal/participant and an authenticated room session.
5. Both sessions are bound to the same room and have the required room permissions.
6. Phone-01 can send a persisted general-room message.
7. Phone-02 can receive that message through the authenticated communication channel.
8. Phone-02 can send a response.
9. Phone-01 receives the response.
10. PostgreSQL evidence identifies both principals, sessions, messages and timestamps.

## Explicit non-claims

- A registered principal is not proof that a physical phone is running.
- A valid OIDC assertion proves the configured issuer authenticated the subject; it does not prove a particular model binary or hardware.
- A successful health check is not proof of AI-to-AI communication.
- Creating the server or its API is not proof that Phone-02 is connected.
- The first contact is PASS only after a real bidirectional exchange is observed and recorded.

## Current implementation boundary

The service already contains the Axum server, PostgreSQL migrations, OIDC workload-token exchange, local sessions, room membership controls, persisted communication routes and WebSocket support.

The remaining work for first contact is operational: deploy the service/database/identity provider, register the two identities, create authenticated sessions, connect the real clients, and capture the exchange evidence.

## Safety boundary

No automatic publishing, GitHub mutation, command execution, or external AI-provider access is granted merely by joining the room. Room permissions remain default-deny and are independently authorized.
