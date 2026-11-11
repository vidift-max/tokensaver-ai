-- Mission 109: trusted OIDC workload token exchange and revocable opaque sessions.
ALTER TABLE sessions
    ADD COLUMN status TEXT NOT NULL DEFAULT 'ACTIVE'
        CHECK (status IN ('CREATED', 'ACTIVE', 'EXPIRED', 'REVOKED', 'CLOSED')),
    ADD COLUMN token_hash BYTEA,
    ADD COLUMN closed_at TIMESTAMPTZ,
    ADD COLUMN rotated_from_session_id UUID;

CREATE UNIQUE INDEX sessions_token_hash_unique
    ON sessions(token_hash) WHERE token_hash IS NOT NULL;

ALTER TABLE workload_executions
    ADD COLUMN status TEXT NOT NULL DEFAULT 'ACTIVE'
        CHECK (status IN ('ACTIVE', 'EXPIRED', 'REVOKED', 'CLOSED'));

CREATE TABLE auth_token_replays (
    issuer TEXT NOT NULL,
    token_jti TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (issuer, token_jti)
);
CREATE INDEX auth_token_replays_expiry_idx ON auth_token_replays(expires_at);

CREATE TABLE principal_permissions (
    permission_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    room_id UUID,
    permission TEXT NOT NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,
    FOREIGN KEY (tenant_id, principal_id) REFERENCES principals(tenant_id, principal_id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, room_id) REFERENCES rooms(tenant_id, room_id) ON DELETE CASCADE,
    UNIQUE (tenant_id, principal_id, room_id, permission)
);
CREATE INDEX principal_permissions_lookup_idx
    ON principal_permissions(tenant_id, principal_id, permission)
    WHERE revoked_at IS NULL;

-- Existing trusted sessions remain valid for the legacy internal admission tests.
-- Authenticated sessions created by Mission 109 always carry token_hash and execution binding.
