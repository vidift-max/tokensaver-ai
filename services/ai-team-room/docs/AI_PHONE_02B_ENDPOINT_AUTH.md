# AI-PHONE-02B — Endpoint Authentication Contract

## Security model

The service separates three independent authorities:

- **AI authority**: the authenticated AI session, tenant binding, and Team Room permissions. It authorizes AI-owned actions such as durable message submission and task/mission linkage.
- **Endpoint authority**: a random per-endpoint credential. It authorizes operations that represent the active endpoint instance.
- **Tenant authority**: every operator key and endpoint credential is scoped to one `tenant_id`, `ai_id`, and `ai_phone_id`.

A valid AI bearer is not an endpoint credential. A fencing number is not a credential. Sensitive endpoint routes require both the AI bearer and the endpoint credential.

## Credential format and persistence

- Endpoint credentials are 32 bytes from the operating-system CSPRNG, encoded as URL-safe Base64 without padding.
- The endpoint credential is returned once in the endpoint-registration response to the separately authorized operator for secure delivery to that endpoint.
- PostgreSQL stores only SHA-256 digests in `phone_endpoint_credentials.credential_hash`; raw endpoint credentials are never stored.
- The hash is unique. One active credential per endpoint is enforced by a partial unique index.
- Credentials are bound to `endpoint_id`, `ai_phone_id`, `tenant_id`, `ai_id`, and the active `endpoint_fence`.
- Default expiry is 90 days. Rotation revokes the previous credential.
- Operator keys are also high-entropy secrets; only their SHA-256 digests are stored in `phone_endpoint_operator_keys.key_hash`.

## Out-of-band operator provisioning

A trusted control plane provisions a 32-byte random operator key for the AI/tenant scope and inserts only its SHA-256 digest:

```sql
INSERT INTO phone_endpoint_operator_keys
    (operator_key_id, tenant_id, ai_id, key_hash, expires_at)
VALUES
    ('<operator-key-uuid>', '<tenant-uuid>', '<ai-uuid>',
     decode('<64-hex-character-sha256>', 'hex'), NOW() + INTERVAL '90 days');
```

The raw operator key is delivered through the operator's secure secret channel and is not issued by the AI bearer. The `phone.endpoint.manage` permission is also required. The service deliberately has no AI-authenticated API that can mint or read operator keys.

## HTTP contract

All routes still require `Authorization: Bearer <AI-session-token>`.

| Route | Additional credential | Authority |
|---|---|---|
| `POST /ai-phone/endpoints/register` | `X-AI-Phone-Operator-Key` | Operator enrollment/rotation |
| `POST /ai-phone/attach` | `X-AI-Phone-Endpoint-Credential` | Target endpoint proof |
| `POST /ai-phone/reconnect` | `X-AI-Phone-Endpoint-Credential` | Target endpoint proof |
| `POST /ai-phone/transfer` | `X-AI-Phone-Endpoint-Credential` | Target endpoint proof |
| `POST /ai-phone/detach` | `X-AI-Phone-Endpoint-Credential` | Current endpoint proof |
| `POST /ai-phone/heartbeat` | `X-AI-Phone-Endpoint-Credential` | Current endpoint proof |
| `GET /ai-phone/me` | Required while an attachment is active | Current endpoint proof |
| `GET /ai-phone/{phone_id}/messages` | `X-AI-Phone-Endpoint-Credential` | Mailbox delivery/read |
| `POST /ai-phone/{phone_id}/messages/{message_id}/ack` | `X-AI-Phone-Endpoint-Credential` | Mailbox acknowledgement |
| `POST /ai-phone/{phone_id}/messages` | None beyond AI bearer and permission | AI-authorized durable send |
| `POST /ai-phone/mission` | None beyond AI bearer and `task.transition` | Team Room task authority |
| Presence and endpoint-list reads | None beyond AI bearer and `phone.read` | Control-plane metadata only |

Registration returns `{ endpoint, endpoint_credential }`. The secret must be transferred to the target endpoint over the operator's secure channel; do not log it, store it in endpoint metadata, or expose it to the old endpoint process.

## Lifecycle and fencing

1. Operator enrollment creates a `REGISTERED` endpoint and a hash-only credential.
2. Attach/reconnect/transfer proves the target endpoint credential and, in one transaction, binds it to the next fence and creates the active attachment.
3. Transfer changes the old attachment to `FENCED`, changes the old endpoint to `DETACHED`, advances the phone fence, and attaches the new endpoint. The lifecycle trigger revokes the old endpoint credential in the same transaction.
4. Detach advances the fence and transitions the endpoint to `DETACHED`; the trigger revokes its credential.
5. `REVOKED` is terminal. The lifecycle trigger revokes credentials; reconnect/transfer cannot reuse that endpoint ID.
6. Reconnecting a detached endpoint requires a new operator enrollment/credential rotation.

Endpoint-sensitive handlers lock the phone, attachment, endpoint, and credential rows together. Transfer, detach, heartbeat, mailbox delivery, and ACK therefore serialize on the phone row; an old credential cannot race a generation change.

## Deliberate authority boundary

- AI identity authority remains available for durable outbound messages, Team Room task ownership, and mission linkage subject to their existing permissions and tenant constraints.
- Endpoint identity authority is mandatory for heartbeat, active endpoint snapshot, mailbox delivery/read, ACK, and lifecycle proof.
- Presence and endpoint-list routes expose control-plane metadata only; they do not return the endpoint fence or credential secret.
