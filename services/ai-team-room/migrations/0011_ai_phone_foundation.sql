-- AI-PHONE-01: global AI identity, permanent phone, fenced endpoint attachments and durable inbox.
-- Identity records are provisioned by trusted control-plane code; they are not self-registered
-- from a caller-supplied display name or provider label.

CREATE TABLE ai_identities (
    ai_id UUID PRIMARY KEY,
    identity_key TEXT NOT NULL UNIQUE CHECK (length(trim(identity_key)) BETWEEN 1 AND 200),
    identity_kind TEXT NOT NULL CHECK (identity_kind IN ('TEST', 'EXTERNAL')),
    display_label TEXT NOT NULL CHECK (length(trim(display_label)) BETWEEN 1 AND 120),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE ai_identity_principals (
    ai_id UUID NOT NULL REFERENCES ai_identities(ai_id) ON DELETE RESTRICT,
    tenant_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (ai_id, tenant_id, principal_id),
    UNIQUE (tenant_id, principal_id),
    FOREIGN KEY (tenant_id, principal_id)
        REFERENCES principals(tenant_id, principal_id) ON DELETE RESTRICT
);

CREATE TABLE ai_phones (
    ai_phone_id UUID PRIMARY KEY,
    ai_id UUID NOT NULL UNIQUE REFERENCES ai_identities(ai_id) ON DELETE RESTRICT,
    phone_status TEXT NOT NULL DEFAULT 'DISCONNECTED'
        CHECK (phone_status IN ('ATTACHED','TRANSFER_REQUESTED','TRANSFERRING','ATTACHED_NEW_ENDPOINT','DISCONNECTED')),
    version BIGINT NOT NULL DEFAULT 1 CHECK (version > 0),
    endpoint_fence BIGINT NOT NULL DEFAULT 0 CHECK (endpoint_fence >= 0),
    current_mission_id UUID,
    current_task_id UUID REFERENCES team_tasks(task_id) ON DELETE SET NULL,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE phone_endpoints (
    endpoint_id UUID PRIMARY KEY,
    ai_phone_id UUID NOT NULL REFERENCES ai_phones(ai_phone_id) ON DELETE RESTRICT,
    endpoint_type TEXT NOT NULL CHECK (endpoint_type IN ('WEB','DESKTOP','MOBILE','WORKER','TEST')),
    endpoint_status TEXT NOT NULL DEFAULT 'REGISTERED'
        CHECK (endpoint_status IN ('REGISTERED','ATTACHED','DETACHED','REVOKED')),
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB CHECK (jsonb_typeof(metadata) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ,
    detached_at TIMESTAMPTZ
);
CREATE INDEX phone_endpoints_phone_idx ON phone_endpoints(ai_phone_id, created_at DESC);

CREATE TABLE phone_attachments (
    attachment_id UUID PRIMARY KEY,
    ai_phone_id UUID NOT NULL REFERENCES ai_phones(ai_phone_id) ON DELETE RESTRICT,
    endpoint_id UUID NOT NULL REFERENCES phone_endpoints(endpoint_id) ON DELETE RESTRICT,
    tenant_id UUID NOT NULL,
    room_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    participant_id UUID NOT NULL,
    session_id UUID NOT NULL,
    execution_id UUID NOT NULL,
    endpoint_fence BIGINT NOT NULL CHECK (endpoint_fence > 0),
    attachment_status TEXT NOT NULL CHECK (attachment_status IN ('ACTIVE','DETACHED','FENCED')),
    attached_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    detached_at TIMESTAMPTZ,
    FOREIGN KEY (tenant_id, principal_id)
        REFERENCES principals(tenant_id, principal_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, participant_id, principal_id)
        REFERENCES participants(tenant_id, participant_id, principal_id) ON DELETE RESTRICT,
    FOREIGN KEY (room_id, session_id, principal_id)
        REFERENCES sessions(room_id, session_id, principal_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, room_id, participant_id, principal_id, session_id, execution_id)
        REFERENCES workload_executions(tenant_id, room_id, participant_id, principal_id, session_id, execution_id)
        ON DELETE RESTRICT,
    UNIQUE (ai_phone_id, endpoint_fence)
);
CREATE UNIQUE INDEX phone_one_active_attachment_idx
    ON phone_attachments(ai_phone_id) WHERE attachment_status = 'ACTIVE';
CREATE UNIQUE INDEX phone_one_active_endpoint_idx
    ON phone_attachments(endpoint_id) WHERE attachment_status = 'ACTIVE';

CREATE TABLE phone_messages (
    message_id UUID PRIMARY KEY,
    ai_phone_id UUID NOT NULL REFERENCES ai_phones(ai_phone_id) ON DELETE RESTRICT,
    tenant_id UUID NOT NULL,
    sender_principal_id UUID NOT NULL,
    recipient_ai_id UUID NOT NULL REFERENCES ai_identities(ai_id) ON DELETE RESTRICT,
    sequence_no BIGINT NOT NULL CHECK (sequence_no > 0),
    idempotency_key TEXT NOT NULL CHECK (length(trim(idempotency_key)) BETWEEN 1 AND 200),
    message_state TEXT NOT NULL DEFAULT 'PENDING'
        CHECK (message_state IN ('PENDING','DELIVERED','ACK','FAILED')),
    content JSONB NOT NULL CHECK (jsonb_typeof(content) IN ('object','array','string','number','boolean','null')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at TIMESTAMPTZ,
    acknowledged_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    FOREIGN KEY (tenant_id, sender_principal_id)
        REFERENCES principals(tenant_id, principal_id) ON DELETE RESTRICT,
    UNIQUE (ai_phone_id, sender_principal_id, idempotency_key),
    UNIQUE (ai_phone_id, sequence_no)
);
CREATE INDEX phone_messages_inbox_idx
    ON phone_messages(ai_phone_id, sequence_no) WHERE message_state IN ('PENDING','DELIVERED');

CREATE TABLE phone_mission_links (
    ai_phone_id UUID PRIMARY KEY REFERENCES ai_phones(ai_phone_id) ON DELETE RESTRICT,
    task_id UUID NOT NULL REFERENCES team_tasks(task_id) ON DELETE RESTRICT,
    linked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE FUNCTION enforce_phone_attachment_fence() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.endpoint_fence < OLD.endpoint_fence THEN
        RAISE EXCEPTION 'phone endpoint fence cannot move backwards' USING ERRCODE='23514';
    END IF;
    IF NEW.endpoint_fence > OLD.endpoint_fence + 1 THEN
        RAISE EXCEPTION 'phone endpoint fence must advance by at most one' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER phone_attachment_fence_guard
BEFORE UPDATE ON ai_phones
FOR EACH ROW EXECUTE FUNCTION enforce_phone_attachment_fence();

CREATE FUNCTION enforce_phone_message_transition() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.ai_phone_id <> OLD.ai_phone_id OR NEW.sequence_no <> OLD.sequence_no
       OR NEW.sender_principal_id <> OLD.sender_principal_id
       OR NEW.idempotency_key <> OLD.idempotency_key
       OR NEW.content <> OLD.content THEN
        RAISE EXCEPTION 'phone message identity and content are immutable' USING ERRCODE='23514';
    END IF;
    IF NOT (
        NEW.message_state = OLD.message_state
        OR (OLD.message_state = 'PENDING' AND NEW.message_state IN ('DELIVERED','FAILED'))
        OR (OLD.message_state = 'DELIVERED' AND NEW.message_state IN ('ACK','FAILED','PENDING'))
    ) THEN
        RAISE EXCEPTION 'invalid phone message state transition' USING ERRCODE='23514';
    END IF;
    IF NEW.attempt_count < OLD.attempt_count THEN
        RAISE EXCEPTION 'phone message attempts cannot decrease' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER phone_message_transition_guard
BEFORE UPDATE ON phone_messages
FOR EACH ROW EXECUTE FUNCTION enforce_phone_message_transition();


INSERT INTO permission_catalog(permission,description,privileged) VALUES
 ('phone.read','Read the AI-owned phone and its authorized continuity pointer',FALSE),
 ('phone.attach','Attach an authenticated execution endpoint to its own AI phone',FALSE),
 ('phone.detach','Detach the caller current endpoint from its own AI phone',FALSE),
 ('phone.transfer','Explicitly transfer an AI phone endpoint and fence the previous attachment',TRUE),
 ('phone.message.read','Read messages from the caller own AI phone inbox',FALSE),
 ('phone.message.send','Send a durable message to a phone when separately granted',FALSE),
 ('phone.message.ack','Acknowledge delivery on the currently attached phone endpoint',FALSE),
 ('ai.identity.provision','Provision global AI identities and tenant principal bindings',TRUE)
ON CONFLICT (permission) DO NOTHING;

-- A global AI identity may only bind to an enabled AI principal, never a human account.
CREATE FUNCTION enforce_ai_identity_principal_kind() RETURNS TRIGGER AS $$
DECLARE
    kind TEXT;
    is_enabled BOOLEAN;
BEGIN
    SELECT principal_kind, enabled INTO kind, is_enabled
      FROM principals
     WHERE tenant_id=NEW.tenant_id AND principal_id=NEW.principal_id;
    IF kind IS DISTINCT FROM 'AI' OR is_enabled IS DISTINCT FROM TRUE THEN
        RAISE EXCEPTION 'AI identity mapping requires an enabled AI principal' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER ai_identity_principal_kind_guard
BEFORE INSERT OR UPDATE ON ai_identity_principals
FOR EACH ROW EXECUTE FUNCTION enforce_ai_identity_principal_kind();
