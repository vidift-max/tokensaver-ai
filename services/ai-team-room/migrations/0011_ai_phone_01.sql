-- AI-Phone-01: permanent AI identity, phone address, endpoint registry and fenced attachments.
-- This domain is intentionally separate from team_tasks.fencing_token.
CREATE TABLE ai_identities (
    ai_id UUID PRIMARY KEY,
    display_key TEXT NOT NULL UNIQUE CHECK (length(trim(display_key)) BETWEEN 1 AND 64),
    status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','SUSPENDED','REVOKED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE ai_principal_bindings (
    ai_id UUID NOT NULL REFERENCES ai_identities(ai_id) ON DELETE RESTRICT,
    tenant_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','REVOKED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,
    PRIMARY KEY (ai_id, tenant_id, principal_id),
    UNIQUE (tenant_id, principal_id),
    FOREIGN KEY (tenant_id, principal_id) REFERENCES principals(tenant_id, principal_id) ON DELETE RESTRICT
);
CREATE INDEX ai_principal_bindings_ai_idx ON ai_principal_bindings(ai_id, status);

CREATE TABLE ai_phones (
    ai_phone_id UUID PRIMARY KEY,
    ai_id UUID NOT NULL UNIQUE REFERENCES ai_identities(ai_id) ON DELETE RESTRICT,
    phone_status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (phone_status IN ('ACTIVE','SUSPENDED','REVOKED','RETIRED')),
    binding_generation BIGINT NOT NULL DEFAULT 0 CHECK (binding_generation >= 0),
    version BIGINT NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (ai_phone_id, ai_id)
);

CREATE TABLE ai_phone_endpoints (
    endpoint_id UUID PRIMARY KEY,
    ai_phone_id UUID NOT NULL REFERENCES ai_phones(ai_phone_id) ON DELETE CASCADE,
    endpoint_key UUID NOT NULL,
    endpoint_kind TEXT NOT NULL CHECK (endpoint_kind IN ('MOBILE','DESKTOP','WORKER','BROWSER')),
    status TEXT NOT NULL DEFAULT 'REGISTERED' CHECK (status IN ('REGISTERED','ACTIVE','REVOKED','CLOSED')),
    registered_by_tenant_id UUID NOT NULL,
    registered_by_principal_id UUID NOT NULL,
    last_session_id UUID REFERENCES sessions(session_id) ON DELETE SET NULL,
    last_execution_id UUID REFERENCES workload_executions(execution_id) ON DELETE SET NULL,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    closed_at TIMESTAMPTZ,
    UNIQUE (ai_phone_id, endpoint_key),
    UNIQUE (endpoint_id, ai_phone_id),
    FOREIGN KEY (registered_by_tenant_id, registered_by_principal_id)
      REFERENCES principals(tenant_id, principal_id) ON DELETE RESTRICT
);
CREATE INDEX ai_phone_endpoints_phone_status_idx ON ai_phone_endpoints(ai_phone_id, status);

CREATE TABLE ai_phone_attachments (
    attachment_id UUID PRIMARY KEY,
    ai_phone_id UUID NOT NULL,
    ai_id UUID NOT NULL,
    endpoint_id UUID NOT NULL,
    tenant_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    participant_id UUID NOT NULL,
    session_id UUID NOT NULL REFERENCES sessions(session_id) ON DELETE RESTRICT,
    execution_id UUID NOT NULL REFERENCES workload_executions(execution_id) ON DELETE RESTRICT,
    fencing_generation BIGINT NOT NULL CHECK (fencing_generation > 0),
    idempotency_key UUID NOT NULL,
    status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','DETACHED','EXPIRED','TRANSFERRED')),
    lease_expires_at TIMESTAMPTZ NOT NULL,
    attached_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at TIMESTAMPTZ,
    FOREIGN KEY (ai_phone_id, ai_id) REFERENCES ai_phones(ai_phone_id, ai_id) ON DELETE CASCADE,
    FOREIGN KEY (endpoint_id, ai_phone_id) REFERENCES ai_phone_endpoints(endpoint_id, ai_phone_id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, principal_id) REFERENCES principals(tenant_id, principal_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, participant_id, principal_id)
      REFERENCES participants(tenant_id, participant_id, principal_id) ON DELETE RESTRICT,
    UNIQUE (ai_phone_id, idempotency_key),
    UNIQUE (ai_phone_id, fencing_generation)
);
CREATE UNIQUE INDEX ai_phone_one_active_attachment_idx
    ON ai_phone_attachments(ai_phone_id) WHERE status = 'ACTIVE';
CREATE INDEX ai_phone_attachment_expiry_idx
    ON ai_phone_attachments(lease_expires_at) WHERE status = 'ACTIVE';
CREATE INDEX ai_phone_attachment_session_idx
    ON ai_phone_attachments(session_id, status);

CREATE TABLE ai_phone_audit_events (
    event_id UUID PRIMARY KEY,
    ai_phone_id UUID NOT NULL REFERENCES ai_phones(ai_phone_id) ON DELETE CASCADE,
    ai_id UUID NOT NULL REFERENCES ai_identities(ai_id) ON DELETE RESTRICT,
    endpoint_id UUID,
    attachment_id UUID,
    actor_tenant_id UUID NOT NULL,
    actor_principal_id UUID NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('REGISTERED','ENDPOINT_REGISTERED','ATTACHED','DETACHED','EXPIRED','TRANSFERRED','HEARTBEAT')),
    outcome TEXT NOT NULL CHECK (outcome IN ('SUCCESS','DENIED')),
    fencing_generation BIGINT,
    reason_code TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX ai_phone_audit_phone_time_idx ON ai_phone_audit_events(ai_phone_id, created_at DESC);

CREATE OR REPLACE FUNCTION enforce_ai_phone_generation() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.binding_generation < OLD.binding_generation
       OR NEW.binding_generation > OLD.binding_generation + 1 THEN
        RAISE EXCEPTION 'AI-Phone generation must be monotonic and advance by at most one'
          USING ERRCODE = '23514';
    END IF;
    IF NEW.ai_id <> OLD.ai_id THEN
        RAISE EXCEPTION 'AI-Phone identity is immutable' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER ai_phone_generation_guard
BEFORE UPDATE ON ai_phones
FOR EACH ROW EXECUTE FUNCTION enforce_ai_phone_generation();

CREATE OR REPLACE FUNCTION enforce_ai_phone_attachment_transition() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.status = OLD.status THEN RETURN NEW; END IF;
    IF OLD.status = 'ACTIVE' AND NEW.status IN ('DETACHED','EXPIRED','TRANSFERRED') THEN RETURN NEW; END IF;
    RAISE EXCEPTION 'invalid AI-Phone attachment transition: % -> %', OLD.status, NEW.status
      USING ERRCODE = '23514';
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER ai_phone_attachment_transition_guard
BEFORE UPDATE OF status ON ai_phone_attachments
FOR EACH ROW EXECUTE FUNCTION enforce_ai_phone_attachment_transition();
