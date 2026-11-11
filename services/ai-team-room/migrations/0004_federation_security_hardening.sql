-- Mission 110: explicit workload registration, permission vocabulary and security audit.
CREATE TABLE workload_identities (
    workload_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'ACTIVE'
        CHECK (status IN ('ACTIVE', 'DISABLED', 'REVOKED', 'EXPIRED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    FOREIGN KEY (tenant_id, principal_id)
        REFERENCES principals(tenant_id, principal_id) ON DELETE CASCADE,
    UNIQUE (tenant_id, issuer, subject),
    UNIQUE (tenant_id, principal_id)
);

-- Existing Mission 109 registered principals become explicitly registered workloads.
INSERT INTO workload_identities
    (workload_id, tenant_id, principal_id, issuer, subject, status)
SELECT gen_random_uuid(), tenant_id, principal_id, provider, external_subject,
       CASE WHEN enabled THEN 'ACTIVE' ELSE 'DISABLED' END
FROM principals
WHERE principal_kind = 'AI'
ON CONFLICT (tenant_id, principal_id) DO NOTHING;

CREATE INDEX workload_identities_status_idx
    ON workload_identities(tenant_id, status, last_seen_at DESC);

CREATE TABLE permission_catalog (
    permission TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    privileged BOOLEAN NOT NULL DEFAULT FALSE
);
INSERT INTO permission_catalog(permission, description, privileged) VALUES
 ('room.join', 'Join the assigned room', FALSE),
 ('room.read', 'Read room metadata and permitted general resources', FALSE),
 ('room.write', 'Write general room messages', FALSE),
 ('task.read', 'Read tasks explicitly visible to the principal', FALSE),
 ('task.write', 'Create or update tasks within explicit scope', FALSE),
 ('private_chat', 'Read/write only private channels where principal is a member', FALSE),
 ('audio', 'Publish or receive authorized live audio', FALSE),
 ('admin', 'Administrative operations; explicit human-controlled grant only', TRUE)
ON CONFLICT (permission) DO NOTHING;

CREATE TABLE security_audit_events (
    security_event_id UUID PRIMARY KEY,
    tenant_id UUID,
    room_id UUID,
    principal_id UUID,
    action TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('SUCCESS', 'DENIED', 'ERROR')),
    reason_code TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX security_audit_events_time_idx
    ON security_audit_events(created_at DESC);
CREATE INDEX security_audit_events_room_time_idx
    ON security_audit_events(room_id, created_at DESC);
