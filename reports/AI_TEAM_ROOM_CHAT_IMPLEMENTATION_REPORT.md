# TOKEN SAVER AI — AI TEAM ROOM COMMUNICATION IMPLEMENTATION REPORT

**Repository:** vidift-max/tokensaver-ai  
**Branch:** v2-saas-alpha  
**Mission:** Complete AI Team Room, chat and communication  
**Initial audited HEAD:** 6ceacbf54369862ce822d59b929a4644f33aa9ff  
**Code HEAD immediately before this report:** dfe0fbcb7c4527b18f4aba0f9ee2c389ee668fd4  
**Report status:** IMPLEMENTED PARTIALLY; CI PENDING ON CODE HEAD  
**Decision:** NO-GO FOR PRODUCTION; GO CONDITIONED FOR INTEGRATION TESTING

## 1. Executive result

The communication layer was added to the existing Axum + PostgreSQL AI Team Room service. The work reuses existing authenticated sessions, principal/participant identity, room memberships, permission grants, task service and transactional outbox. It does not replace the distributed Runner, lease, fencing or recovery code.

A React workspace was added to the desktop navigation. It provides controls for general chat, private conversation, task creation/listing, decisions, attachment references, participants/presence and an explicitly disabled audio panel.

No claim is made that the new code has passed tests or CI. The current Actions executions are repeatedly cancelled by the repository's concurrency policy as newer commits arrive. The latest observed runs at the code HEAD were queued/pending, not completed.

## 2. Files changed

### New files
- services/ai-team-room/src/communication.rs — authenticated communication handlers, PostgreSQL message/channel/decision/attachment operations, one-use WebSocket tickets, LISTEN/NOTIFY consumer and PostgreSQL integration test.
- services/ai-team-room/migrations/0007_team_room_communication.sql — general/private channels, memberships, messages, read cursors, decisions, attachment references, one-use WebSocket tickets and outbox NOTIFY trigger.
- services/ai-team-room/migrations/0008_team_room_presence_state.sql — persisted presence preference.
- src/TeamRoom.tsx — desktop communication workspace.

### Modified files
- services/ai-team-room/Cargo.toml — Axum WebSocket and WebSocket test dependencies.
- services/ai-team-room/src/main.rs — mounted communication router; presence updates, member-only presence reads, heartbeat outbox event and authenticated leave endpoint.
- services/ai-team-room/src/lib.rs — room-scoped participant join/leave event payloads.
- services/ai-team-room/src/tasks.rs — TASK_CREATED/TASK_UPDATED outbox records in the same transactions as task changes.
- services/ai-team-room/README.md — communication API and limitations.
- src/App.tsx — AI Team Room navigation and component mount.
- src/styles.css — responsive room layout.
- services/ai-team-room/src/main.rs — existing HTTP security integration assertions extended for presence and leave.
- services/ai-team-room/src/communication.rs — PostgreSQL communication integration scenarios included in the module's test build.

No distributed Runner lease/fencing/recovery implementation was intentionally redesigned.

## 3. APIs added or extended

| API | Behavior | Authorization |
|---|---|---|
| GET /team-room/{room_id}/messages | General history, cursor pagination, author display name, timestamps, unread count | active room member + room.read |
| POST /team-room/{room_id}/messages | Idempotent message create, optional parent reference | active member + room.write |
| GET/POST /team-room/{room_id}/private/{other_principal_id}/messages | Private conversation history and send | both active room members; private_chat; channel-member ACL |
| GET /team-room/{room_id}/participants | Active participant metadata | active member + room.read |
| GET/PUT /team-room/{room_id}/presence | Read/update own presence | room.read / presence.update |
| GET /team-room/{room_id}/tasks | List existing distributed tasks | active member + task.read |
| Existing POST /team-room/tasks | Create canonical task through existing task service | task.write and existing server-side execution/membership validation |
| GET/POST /team-room/{room_id}/decisions | List/create decisions | room.read / decision.propose |
| GET/POST /team-room/{room_id}/attachments | List/create metadata references | attachment.read / attachment.upload |
| POST /team-room/{room_id}/ws-ticket | Issue a 30-second single-use WebSocket ticket | active member + room.read |
| GET /team-room/{room_id}/ws?ticket=... | Authenticated WebSocket upgrade | single-use ticket, live session, execution, membership and room.read grant |
| POST /team-room/{room_id}/leave | Mark membership LEFT and revoke room session | room.join |

All routes use the same room_id, participant_id, principal_id and session_id established by the existing authentication path. Private channels are tenant/room scoped and use a deterministic principal pair protected by a PostgreSQL transaction advisory lock.

## 4. WebSocket and outbox

**IMPLEMENTADO in source; NOT VALIDATED by completed CI yet.**

- Axum WebSocket endpoint is real; it is not a polling facade.
- Clients first obtain a short-lived, single-use ticket over authenticated HTTP. The bearer token itself is not placed in the WebSocket URL.
- PostgreSQL outbox trigger sends NOTIFY with only the outbox event UUID.
- Each WebSocket process uses PgListener, retrieves the durable outbox row and checks the target room.
- Private channel events are filtered using the channel's persisted type and team_room_channel_members. This check also covers attachment events, not only message events.
- Session, execution, membership and room.read permission are revalidated every 15 seconds. Invalidated sessions close the socket.
- Frontend reconnects with bounded exponential backoff, obtains a fresh ticket and reloads permitted history after reconnect.
- Message retry uses (channel_id, sender_principal_id, client_message_id). A repeated identical request returns the canonical message without another outbox event; reusing the key with different content returns conflict.

### Event coverage
Implemented/published:
- MESSAGE_CREATED
- PARTICIPANT_JOINED (mapped from admission/reconnection outbox events)
- PARTICIPANT_LEFT
- PRESENCE_CHANGED
- TASK_CREATED
- TASK_UPDATED
- DECISION_CREATED
- ATTACHMENT_CREATED

Not implemented / not claimed:
- MESSAGE_UPDATED
- MESSAGE_DELETED
- HANDOFF_CREATED
- HANDOFF_UPDATED

## 5. Messages and private chat

**IMPLEMENTADO:** persistent general messages, paginated history, author display name, timestamp, parent_message_id thread reference, idempotency key, read cursor and unread count.

**IMPLEMENTADO:** private A↔B channel creation, send and history. Both principals must have active membership in the same room. Third-party HTTP history resolves to a different principal pair/channel; it does not expose A↔B content. WebSocket delivery checks the database membership for every private channel event, including private attachment events.

**TESTED IN SOURCE, NOT YET EXECUTED:** PostgreSQL integration test creates A, B and C with independent principals/sessions; sends general and private messages; checks A/B history, C isolation, idempotent retry and duplicate outbox suppression; opens a real WebSocket connection and checks live delivery; then proves a third participant does not receive private-channel events.

## 6. Presence

Migration 0008 adds ONLINE, AWAY, BUSY, WAITING and BLOCKED preferences to room_memberships. DISCONNECTED is derived from heartbeat age. EXECUTING is derived from a live task lease owned by the participant.

The heartbeat endpoint remains separate from task lease renewal. It does not extend task leases and does not overwrite the participant's explicit presence preference. Presence snapshots require active room membership and room.read.

**Limitation:** the endpoint lists active room members, not all registered-but-never-admitted identities. Provider is sourced from the existing principal provider/issuer field; it is not proof of which model is currently generating output. EXECUTING is task-ownership evidence, not provider runtime attestation.

## 7. Tasks

Task creation and listing reuse the existing team_tasks domain. Task create and lease/checkpoint/complete/recovery mutations publish TASK_CREATED/TASK_UPDATED through the existing outbox in the same PostgreSQL transaction.

The communication UI does not mint permissions or bypass the existing task authorization/fencing. It can create a task using mission_id/title and display existing task status, owner and fencing/checkpoint metadata.

**PARCIAL:** UI does not yet provide a full task assignment form with recipient, priority, dependencies, detailed context/result/evidence editor, or the requested CREATED/ASSIGNED/ACKNOWLEDGED/IN_PROGRESS/BLOCKED/WAITING_REVIEW/DONE/REJECTED/CANCELLED state machine. The existing distributed task statuses and Runner ownership path remain authoritative.

## 8. Decisions and attachments

**Decisions IMPLEMENTED:** durable room-scoped record with author, timestamp, mission reference, participant list and optional message reference. Creation requires decision.propose. Participant and referenced-message visibility are checked before persistence. Decision row and outbox event are written transactionally.

**Attachment references IMPLEMENTED:** metadata record relates channel, message/task, filename, storage reference, MIME type, size and optional SHA-256. Read filters private channel records through membership ACL. The reference and event are persisted together.

**Binary upload PENDING:** no object store, binary transfer, malware scan, signed URL or download proxy is implemented. UI explicitly labels this as reference-only.

## 9. Audio

**WEBRTC_PRODUCTION = PENDING.**

The UI shows audio area and disabled controls. No WebRTC signaling, SFU, media grants, mute/unmute transport, speaker detection or audio recording exists. No audio is captured automatically or otherwise.

## 10. Authentication and security

- Existing bearer session authentication and permission catalogue are reused.
- General messages require room.write; private channels require private_chat; task/decision/attachment routes require their corresponding existing grants.
- All communication routes verify active room membership and room scope.
- WebSocket tickets are random, hashed at rest, single-use and expire after 30 seconds.
- WebSocket revalidates session, execution, membership and room.read permission every 15 seconds.
- Private fanout is checked against the database channel membership, including non-message events.
- Message idempotency prevents duplicate persistence and duplicate outbox notifications.
- UI keeps the user-pasted opaque session token in component memory only; it does not persist the token.
- Desktop OIDC exchange, refresh and secure credential storage are NOT integrated into the desktop authentication UX.
- No administrative permission is granted by display name, provider name or UI state.

Residual security work: rate limiting for messaging, message edit/delete authorization, WebSocket origin policy, attachment storage/download security, audit retention and penetration testing.

## 11. Tests

### Added/extended tests
- PostgreSQL general message persistence and history.
- Message idempotency and exactly one MESSAGE_CREATED outbox event.
- A↔B private conversation visibility to A and B.
- C does not receive A↔B private history.
- Authenticated WebSocket ticket and real WebSocket handshake.
- PostgreSQL outbox notification delivered to WebSocket after a real HTTP message write.
- C does not receive private A↔B WebSocket events.
- Decision creation/list with an empty participant list.
- Attachment metadata creation/list with room/channel/message reference.
- Invalid bearer session rejected.
- Presence preference AWAY survives heartbeat; stale heartbeat becomes DISCONNECTED.
- Authenticated leave changes membership to LEFT.
- Existing session expiry/revocation and permission tests remain in the service suite.

### Execution status
No local shell/checkout was available in this execution path. No cargo test, cargo check, npm install or npm run build was executed locally.

GitHub Actions runs for earlier SHAs were cancelled by the workflow concurrency group as newer commits arrived. For the latest code SHA before this report, dfe0fbcb7c4527b18f4aba0f9ee2c389ee668fd4:
- TokenSaver AI CI run 551 (ID 43104241952): queued at last observation.
- Distributed Runner PostgreSQL Lab run 74 (ID 43104241780): pending at last observation.
- Neither is a completed test result.
- The final report commit will create a new HEAD and require a new exact-SHA CI run. No check result is inferred from an earlier SHA.

## 12. Classification

### IMPLEMENTADO
- Persistent general messages, history pagination, author display name, timestamp and thread parent reference.
- Private channels and server-side member ACL.
- WebSocket transport, one-use ticket, PostgreSQL NOTIFY/outbox delivery and private fanout filtering.
- Frontend reconnect flow and history refresh.
- Presence preference and heartbeat freshness, with task-owned EXECUTING indication.
- Existing task service integration and task lifecycle outbox events.
- Decision records and authorized attachment references.
- AI Team Room React workspace and navigation.
- Authenticated leave lifecycle.

### TESTADO
- Test code was added for real PostgreSQL and WebSocket behavior.
- **No tests have been executed in this mission's environment yet.**

### VALIDADO
- No new communication behavior is marked validated.
- Existing identity/session/admission/Runner validations from earlier exact SHAs remain historical evidence only and are not reused as proof for this communication code.

### PARCIAL
- Task assignment/state editor.
- Complete event matrix.
- Presence of registered-but-not-admitted agents.
- Desktop identity/OIDC session acquisition.
- Unread count is available per history response; push notifications are not implemented.
- Attachment metadata only.
- Audio UI only.

### PENDENTE
- WebRTC/SFU/audio grants.
- Binary upload/storage/download.
- Message edit/delete.
- Full handoff events and workflow.
- Full task state machine and recipient/priority/dependency/evidence UI.
- OIDC login/refresh integration in desktop UI.
- Room-wide notification preferences and push notifications.
- Production deployment, observability and operational controls.

### BLOQUEADO
- Production rollout of the communication layer until exact-SHA CI passes, service is deployed behind TLS/private ingress, identity UX is integrated, and attachment/audio operational choices are made.

### NÃO TESTADO
- Exact-SHA CI on the report HEAD.
- End-to-end desktop UI against a deployed service and real provider identities.
- Cross-instance WebSocket fanout under load.
- WebSocket reconnect during server restart and permission revocation.
- Production rate/abuse limits, retention, backup/restore, penetration test.
- WebRTC and binary upload (not implemented).

## 13. Risk register

1. **No CI pass on final SHA:** compile/runtime failures remain possible until run 551 equivalent completes on the final report SHA.
2. **OIDC UI gap:** manual token paste is not an acceptable production sign-in flow.
3. **Outbox fanout scale:** each live connection owns a PostgreSQL listener; validate connection counts, reconnect storms and database limits before scale.
4. **WebSocket ticket in query string:** ticket is one-use and short-lived, but ingress/access logs must redact query parameters.
5. **Private ACL:** message history and live fanout are protected; any future search, export, notification or attachment download must reuse the same ACL check.
6. **Attachment refs:** metadata is not a file store; do not expose raw storage_ref as a downloadable URL.
7. **Presence semantics:** active lease means task ownership, not verified provider execution.
8. **Partial task/handoff UI:** do not imply the room UI can perform every Runner transition.
9. **Audio:** do not enable controls until a real signaling/media architecture and consent policy exist.

## 14. Main implementation commits

- 071bb0296effd78909489fee57ce98b52074dcff — communication schema and outbox trigger.
- eba9c76e3dd348fc9c2cd90680d62d14fdf07a9d — authenticated communication backend and WebSocket foundation.
- 82a08b04d8c4d77e3755c8b3be4ea55d17aba170 — mount communication routes into existing Axum service.
- 61890617b8a3255f77885f1cb1893d3971fea29e — explicit presence schema.
- 8a42a4798d72f90186e6ff730d6e4c71eb5c383b — task lifecycle outbox events.
- 11017b9f9abbdcb6f4ab3aaffea42dadd16606e3 — React AI Team Room workspace.
- c3237853f48d15ea357feb17bbc924c48bc85b48 — PostgreSQL private ACL/idempotency integration test.
- 5fb18bda21ef1cb4a622485dbadebd69a5143701 — real WebSocket integration test.
- 287f549a1c8dc072795a5115ab0a82056e96d9f0 — private-channel outbox filtering hardening.
- 6cd352dee0ab747ecbb3c4c845c8bd226702e71a — third-member private WebSocket fanout regression.
- 3ced1fce864ab241cddfa9e741ff13c2b84afcf4 — authenticated leave endpoint.
- df4d66afebfae00deb5ce50ef5602c583a63405d — leave regression test.
- 4c1ab0cbc6148b044198f8a236d572369daeec08 — revalidate WebSocket room.read grant.
- dfe0fbcb7c4527b18f4aba0f9ee2c389ee668fd4 — document leave contract (code HEAD immediately before this report).

## 15. Final decision

**NO-GO FOR PRODUCTION. GO CONDITIONED FOR CI VALIDATION AND INTEGRATION HARDENING.**

The communication backend and desktop workspace are implemented as a coherent extension of the existing Team Room. The real WebSocket path and PostgreSQL integration tests are present in source. They must not be called validated until the test suite and frontend build pass on the exact final report SHA.

The next hard gate is to let GitHub Actions complete on the report commit SHA, inspect the service job logs and confirm:
- cargo check passes;
- PostgreSQL communication integration test passes;
- real WebSocket delivery passes;
- C receives no private A↔B event;
- npm run build passes;
- all CI jobs are green for the exact final SHA.
