# AI-PHONE-06 — Final P1 Hardening Report

## 1. Identity and scope

- Repository: `vidift-max/tokensaver-ai`
- Mission branch: `mission/ai-phone-06-p1-hardening`
- Base branch: `v2-saas-alpha`
- Base SHA at audit: `d1f2bd764361b14af0604104db8e670ee6f4f361`
- Starting SHA: `a906444535e9e81418209122441f303b7a2be43b`
- Code SHA validated by the final full CI run: `d679414937e7e1fe30d6b48557a05aa5c3d5934f`
- Migrations: `0001` through `0013`; no new migration was required.
- Pull request for this mission: none created.
- Merge: none.
- Historical PRs #7, #8, #9 and #10: left open, Draft and unmerged.
- Production database: not accessed.

The final report is committed separately from the validated code changes. The final branch HEAD must be read from GitHub after this report commit; the full CI workflow is configured to run on every push to this mission branch.

## 2. P1 endpoint revocation

### Changes

Files:
- `services/ai-team-room/src/phone.rs`
- `services/ai-team-room/tests/phone_postgres.rs`

Backend operations that require an attached endpoint now check the persistent endpoint record and require `endpoint_status='ATTACHED'` in addition to the active attachment, tenant/AI binding, session/execution and current phone fence:

- heartbeat;
- pending inbox read/delivery;
- ACK, including duplicate-ACK handling;
- idempotent attach retry;
- reconnect/reattach through the existing registration and attach paths.

A revoked endpoint cannot be downgraded to `DETACHED` during disconnect or transfer. The transfer path preserves `REVOKED`.

The `send` operation intentionally remains identity-scoped and may queue a durable message while the endpoint is offline or revoked. It does not require an active endpoint; this preserves the documented offline mailbox behavior and is not an endpoint-authority operation.

### Adversarial test

Test:
`phone_identity_survives_session_rotation_and_endpoint_change`

The PostgreSQL/HTTP test now exercises:
1. provision phone;
2. register and attach endpoint C;
3. create a delivered message;
4. persist endpoint C as `REVOKED`;
5. attempt heartbeat, inbox read and ACK;
6. attempt attach and reconnect using endpoint C;
7. verify the endpoint remains `REVOKED`;
8. verify identity-scoped offline send remains available;
9. register and transfer to a different valid endpoint D;
10. verify D can heartbeat and the original `AI_PHONE_ID` is unchanged;
11. verify transfer did not reactivate C.

Observed expectations:
- heartbeat after revocation: HTTP 412;
- inbox read after revocation: HTTP 412;
- ACK after revocation: HTTP 412;
- attach replay: HTTP 409;
- reconnect replay: HTTP 409;
- valid endpoint D registration/transfer/heartbeat: success;
- permanent phone identity: unchanged.

**Qualification:** this proves enforcement of the persisted endpoint state and stale-attachment fencing. It does not provide cryptographic proof that a particular physical/browser client possesses an endpoint key. The current bearer session remains the principal authority; see PoP decision below.

## 3. WebSocket revocation

File:
- `services/ai-team-room/src/communication.rs`

The existing 15-second periodic session revalidation remains as an idle-connection cleanup mechanism. In addition, each outbox event is revalidated at the delivery boundary.

The dispatcher acquires shared row locks on the authenticated session, workload execution, room membership and `room.read` permission while it performs the socket write. A concurrent revocation therefore linearizes before the delivery check (delivery denied) or after the in-flight write has completed. The next event cannot be emitted using a revoked session.

Test:
`communication::postgres_tests::postgres_general_private_acl_and_message_idempotency`

The test:
- opens an authorized WebSocket;
- receives a legitimate event;
- revokes the session;
- inserts and explicitly notifies a new room event;
- asserts the connection receives a close frame within two seconds, before the event payload can be delivered.

The idle connection may remain open until the next periodic check if no event arrives, but it cannot receive a new event without passing the delivery-boundary check. This is not a post-revocation data-delivery window.

## 4. AI-PHONE outbox and notification architecture

AI-PHONE emits `aggregate_type='ai_phone'` outbox events. The Team Room WebSocket dispatcher intentionally continues to handle room, channel, task and handoff aggregates only.

The current AI-PHONE UI uses HTTP polling (five-second refresh while attached) for phone presence and messages. For this MVP, polling is the official delivery mechanism; AI-PHONE WebSocket delivery is not implemented and is not claimed as working.

This is a functional architecture limitation, not a cross-tenant security defect. The Team Room WebSocket path retains its tenant, room, aggregate and private-channel authorization checks.

## 5. Message history and mailbox

Files:
- `services/ai-team-room/src/phone.rs`
- `src/AIPhone.tsx`
- `services/ai-team-room/tests/phone_postgres.rs`

The API now distinguishes:
- `GET /ai-phone/{phone_id}/messages`: active endpoint's pending/delivered inbox; ACKed messages are excluded from the queue;
- `GET /ai-phone/received`: durable incoming history, including ACKed messages;
- `GET /ai-phone/sent`: durable outgoing history.

The UI merges pending inbox, received history and sent history by `message_id`, avoiding duplicate display while retaining acknowledged incoming messages.

Additional PostgreSQL/HTTP assertions cover:
- duplicate send with the same idempotency key and payload;
- three simultaneous sends with the same key and payload;
- same key with different payload returns 409 and preserves original content;
- ACK outside sequence order;
- sequence gaps remain gaps and do not create phantom messages;
- inbox ordering remains strictly increasing by sequence;
- ACK removes an item from the pending queue but not from received history;
- read and ACK are rejected after endpoint revocation;
- failure injected during the outbox insert rolls back the earlier message insert;
- retry with the same idempotency key after rollback succeeds and creates exactly one row.

Not proven:
- reconnect specifically after a network failure between a partially completed external delivery and client receipt;
- arbitrary power-loss/process-kill fault injection between PostgreSQL commit and client response.
These remain outside the demonstrated test boundary.

## 6. Legacy migration and reconciliation

New fixture:
- `scripts/ai-phone-legacy-compatible-upgrade-test.sh`

Workflow:
- `.github/workflows/ai-phone-tenant-isolation-fix.yml`

The workflow now executes both legacy fixtures against disposable PostgreSQL 16:

1. **Ambiguous/shared legacy identity:** actual migration 0012 fails closed and transaction rollback preserves the old schema and rows.
2. **Unambiguous legacy identity:** the compatible fixture builds the actual pre-0012 schema from migrations 0001–0011, uses one explicit tenant-to-phone mapping, then applies actual migrations 0012 and 0013.

The compatible fixture verifies:
- one legacy AI identity has exactly one tenant binding;
- explicit mapping `10000000-0000-4000-8000-000000000001:90000000-0000-4000-8000-000000000009`;
- original `AI_PHONE_ID` preserved;
- endpoint and active attachment preserved;
- ACKed message preserved;
- mission/task link preserved;
- tenant scope populated;
- seven tenant-scoped AI-PHONE foreign keys exist.

This fixture proves the safe migration path for an unambiguous legacy phone. It is not a generic backfill algorithm and does not authorize guessing. In a real legacy environment, the operator must still inventory the database, approve the exact tenant-to-phone mapping, review all endpoint/attachment/message/task references, back up the database, test on a disposable copy and verify post-migration counts and constraints. Ambiguous/shared phones remain fail-closed. No automatic rekey/backfill was added.

The compatible fixture proves successful application of 0012 and 0013. A production rollback of an already committed successful migration and an operator-run recovery exercise remain NOT PROVEN.

## 7. RLS decision

**Classification: `REQUIRED_BEFORE_PRODUCTION`.**

RLS was not implemented in this mission. The current application-level tenant predicates and composite foreign keys remain the active isolation controls. No new P0 cross-tenant defect was demonstrated.

Before production, RLS should cover at least the tenant-owned AI-PHONE and Team Room data:
- `ai_phones`;
- `ai_identity_tenants` and `ai_identity_principals`;
- `phone_endpoints`;
- `phone_attachments`;
- `phone_messages`;
- `phone_mission_links`;
- `team_tasks`, `task_lease_events`, `task_handoffs`;
- `sessions`, `workload_executions`, `room_memberships`;
- `team_room_channels`, `team_room_messages`, `team_room_attachment_refs`;
- `team_room_ws_tickets`;
- tenant-scoped `audit_events` and `outbox_events`.

Implementation plan:
1. Introduce a dedicated application database role that is neither table owner nor superuser and has no `BYPASSRLS`.
2. Keep a separate migration/owner role.
3. Propagate tenant context from authenticated backend identity using `SET LOCAL app.tenant_id = ...` inside every transaction; never use session-level state on pooled connections.
4. Enable and FORCE RLS on tenant-owned tables with policies tied to that transaction-local context.
5. Give background outbox/notification workers explicit narrowly scoped policies or a dedicated constrained role.
6. Convert pool-level reads and writes that require tenant context to transaction-scoped access before enabling policies.
7. Test missing tenant context, context reset on pool reuse, cross-tenant SELECT/UPDATE/DELETE and worker access.

Partial RLS activation without the transaction/context refactor would risk breaking legitimate authorization paths and is not safe to improvise.

## 8. Proof-of-possession decision

**Classification: `REQUIRED_BEFORE_EXTERNAL_ENDPOINTS`.**

The current endpoint identifier is a client-generated UUID persisted in browser local storage. It is not a secret and is not cryptographic proof of possession. The bearer session authenticates the principal/session, while the phone fence rejects stale attachment generations. Neither binds the caller to a device-held private key.

Before exposing WEB, DESKTOP, MOBILE or other untrusted external endpoints, implement a complete endpoint key lifecycle:
- per-endpoint asymmetric key pair (for example Ed25519);
- private key retained by the endpoint;
- registration of the public key against tenant, AI_PHONE_ID and endpoint ID;
- nonce/challenge signing on attach/reconnect and sensitive endpoint operations;
- server-side verification and replay protection;
- rotation and revocation of the endpoint key;
- key deletion/recovery rules and audit events.

Do not treat the endpoint UUID, a client-supplied endpoint ID or the phone fence as PoP. No incomplete cryptographic implementation was added.

## 9. UI validation

**`BROWSER_VALIDATION_NOT_AVAILABLE`.**

The UI code was updated to fetch durable received history, but this mission did not have an executable browser/viewport automation environment. No mobile or desktop visual test is claimed.

## 10. Final CI evidence

Workflow:
`AI-PHONE-03 Consolidated AI Phone + Team Room`

Final code SHA tested:
`d679414937e7e1fe30d6b48557a05aa5c3d5934f`

Run:
- Run number: 91
- Run ID: `43318802849`
- Result: SUCCESS
- PostgreSQL: 16
- `cargo check`: PASS
- Full PostgreSQL integration suite: PASS
- Test groups: 6 + 9 + 3 + 4 + 1 = 23 tests passed; zero failed.
- Ambiguous legacy fail-closed fixture: PASS.
- Compatible legacy 0012 + 0013 fixture: PASS.
- Endpoint revocation/history/mailbox integration test: PASS.
- Team Room WebSocket/revocation integration test: PASS.

Run URL:
https://github.com/vidift-max/tokensaver-ai/actions/runs/43318802849

## 11. Final status

- Multi-tenant P0: not reopened; no new cross-tenant exploit confirmed.
- Endpoint revocation: persisted-state checks and adversarial HTTP coverage implemented and passing.
- Session/fencing: existing behavior retained; stale generations remain rejected.
- WebSocket: per-delivery authorization and revocation serialization implemented and passing.
- AI-PHONE WebSocket notifications: not part of current MVP; HTTP polling remains the documented behavior.
- Mailbox: history, ACK, ordering, concurrency and injected transaction rollback coverage expanded and passing.
- Legacy: ambiguous state remains fail-closed; unambiguous migration path now has a PostgreSQL fixture.
- RLS: required before production; not implemented.
- PoP: required before external endpoints; not implemented.
- Browser/viewport: not validated.
- Production readiness: not claimed.
- Merge: not performed.

Recommended disposition: `REVIEW_READY_WITH_MINOR_RISKS` for controlled continuation of the internal MVP and replication work, subject to the explicit RLS and PoP release gates above. This is not production approval.
