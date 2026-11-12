# AI-PHONE-01 — Virtual Phone Architecture

**Scope:** internal text-only virtual phone inside TokenSaver AI. This is not a PSTN number, telephone carrier integration, or voice/WebRTC implementation.

## 1. Identity model

The global AI identity is stable and may be bound to multiple tenants. It is not itself a resource authorization key.

```text
global AI identity (ai_id)
   ├── tenant A binding (tenant_id, ai_id) -> AI_PHONE_ID_A
   └── tenant B binding (tenant_id, ai_id) -> AI_PHONE_ID_B
```

The database enforces:

- `ai_identity_principals`: unique `(ai_id, tenant_id)`, plus the tenant/principal binding.
- `ai_identity_tenants`: primary key `(ai_id, tenant_id)`, FK to the tenant and principal binding.
- `ai_phones`: unique `(tenant_id, ai_id)`; globally unique `ai_phone_id`.
- A tenant may not bind the same global AI identity to multiple principals.

This supports up to 15 AI identities without a hard-coded 15-phone schema limit. Each AI gets one phone per tenant. The same AI in another tenant gets another phone.

## 2. Phone identity and endpoint continuity

`AI_PHONE_ID` belongs to the tenant/AI pair, not to a session, endpoint, machine, or browser.

- A phone is created only by explicit `POST /ai-phone/provision`.
- `GET /ai-phone/me` is read-only.
- An endpoint is separately registered with a stable endpoint UUID.
- Attach/reconnect binds the endpoint to the current Team Room session and execution.
- Every successful attach advances the phone's monotonic endpoint fence.
- Detach, endpoint revocation, stale session, and stale fence prevent old endpoints from acting as the current endpoint.
- The browser UI stores its endpoint UUID per `AI_PHONE_ID`; another device can register a different endpoint and reconnect to the same phone identity.

The phone does not own Team Room work. Its optional mission pointer references an existing Team Room task. Team Room remains authoritative for task ownership, leases, fencing, checkpoints, handoff, and recovery.

## 3. Durable messaging

PostgreSQL is the source of truth. The UI polls the durable API; WebSocket notifications may be added later but must not become the message store.

Message lifecycle:

1. Sender submits a tenant-scoped destination phone, idempotency key, and JSON content.
2. The server locks the destination phone row and allocates the next per-phone sequence.
3. The message is committed with a durable state before the sender receives success.
4. A disconnected recipient does not block the send; the message remains pending.
5. After attach/reconnect, the recipient reads the persistent inbox.
6. The recipient ACKs the message; repeated idempotent requests return the existing result.
7. A sender can retrieve durable sent history scoped to its tenant and principal.

A repeated idempotency key with the same payload returns the original message. Reusing it with a different payload is a conflict.

## 4. Tenant isolation

Every API request derives `tenant_id` and principal identity from the authenticated session. The caller cannot choose a tenant by passing a phone UUID.

Database constraints protect the same boundary:

- Phone-to-tenant and phone-to-AI composite keys.
- Endpoint-to-phone composite keys.
- Attachment-to-phone and attachment-to-endpoint composite keys.
- Message-to-phone composite keys.
- Mission link to phone and Team Room task composite keys.
- The phone's `current_task_id/current_mission_id` pair references the same tenant-owned Team Room task and mission; the pair must be null or complete.

Handlers also scope reads and mutations by the authenticated tenant. Foreign phone IDs are concealed as not found where appropriate. ACK and stale-session failures are rejected.

## 5. API surface

| Method | Route | Purpose |
|---|---|---|
| POST | `/ai-phone/identities` | Bind an existing AI principal to a tenant/global AI identity |
| POST | `/ai-phone/provision` | Explicitly create the tenant-specific phone |
| GET | `/ai-phone/me` | Read the caller's existing phone and attachment state |
| POST | `/ai-phone/endpoints/register` | Register an endpoint for the caller's phone |
| GET | `/ai-phone/{phone_id}/endpoints` | Read endpoint registry, tenant-scoped |
| POST | `/ai-phone/attach` | Attach current endpoint/session |
| POST | `/ai-phone/reconnect` | Reconnect with current session and a new fence |
| POST | `/ai-phone/detach` | Disconnect the current attachment |
| POST | `/ai-phone/heartbeat` | Refresh the current endpoint heartbeat |
| GET | `/ai-phone/{phone_id}/presence` | Read phone presence, tenant-scoped |
| POST | `/ai-phone/{phone_id}/messages` | Persist a message to a same-tenant phone |
| GET | `/ai-phone/{phone_id}/messages` | Read the attached phone's durable inbox |
| GET | `/ai-phone/sent` | Read durable sent history for the authenticated principal |
| POST | `/ai-phone/{phone_id}/messages/{message_id}/ack` | ACK a received message |
| POST | `/ai-phone/mission` | Link a same-tenant Team Room task to the phone |

All routes require a valid authenticated session; permission checks are applied per operation.

## 6. UI behavior

The Team Room now includes a responsive virtual-phone panel for desktop and mobile:

- AI identity and phone UUID.
- Connection/presence status and fencing value.
- Explicit provisioning state.
- Connect, reconnect, disconnect, and refresh controls.
- Destination phone ID and text composer.
- Durable incoming and outgoing history.
- Message state and ACK control.
- Offline sending and recovery after reconnection.

No voice-call control is presented as functional. PSTN, external numbers, audio, and WebRTC are explicitly out of scope for this MVP.
