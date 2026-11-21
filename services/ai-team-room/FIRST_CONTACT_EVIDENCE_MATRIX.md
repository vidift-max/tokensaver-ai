# First-Contact Evidence Matrix

| Gate | Required evidence | PASS condition |
|---|---|---|
| Server live | HTTPS request + status | `/health/live` returns 200 |
| Server ready | HTTPS request + status | `/health/ready` returns 200 |
| Database | service health/log evidence | PostgreSQL reachable by Team Room |
| Identity | Keycloak/OIDC evidence | authenticated principals for Phone-01 and Phone-02 |
| Sessions | Team Room session evidence | valid sessions exist for both participants |
| Room admission | room membership evidence | both participants admitted to the same room |
| Phone-01 -> Phone-02 | persisted message + recipient delivery evidence | Phone-02 receives the exact test message |
| Phone-02 -> Phone-01 | persisted reply + recipient delivery evidence | Phone-01 receives the reply |
| Persistence | PostgreSQL message records | both directions have durable records and timestamps |
| Authorization | negative test | unauthorized sender cannot inject into the room |
| Hardware designation | runtime/device evidence | only claim if the evidence actually identifies physical Phone-02 |

## PASS rule

The first-contact mission is PASS only when all communication gates succeed and the evidence is tied to the same deployed server, room, authenticated sessions, and test window.

A server health check, DNS record, TLS certificate, Keycloak login, principal registration, or session creation alone is not first-contact proof.

## Required evidence package

Record:

- deployment revision/commit;
- public hostname;
- health-check output;
- authenticated principal identifiers (redacted where appropriate);
- room identifier;
- session identifiers or hashes where safe;
- message IDs/timestamps;
- server logs showing authenticated delivery without exposing secrets;
- PostgreSQL evidence for both messages;
- negative authorization test result.

Do not record passwords, bearer tokens, private keys, or session secrets in the evidence package.

## Current status

This matrix defines acceptance criteria only. No PASS is claimed for the physical Phone-02 connection at this stage.
