ALTER TABLE participants
    ADD CONSTRAINT participants_tenant_participant_principal_unique
    UNIQUE (tenant_id, participant_id, principal_id);

ALTER TABLE sessions
    ADD CONSTRAINT sessions_room_session_principal_unique
    UNIQUE (room_id, session_id, principal_id);

CREATE TABLE workload_executions (
    execution_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    room_id UUID NOT NULL,
    participant_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    session_id UUID NOT NULL,
    worker_subject TEXT NOT NULL,
    heartbeat_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (expires_at > heartbeat_at),
    FOREIGN KEY (tenant_id, room_id) REFERENCES rooms(tenant_id, room_id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, participant_id, principal_id)
        REFERENCES participants(tenant_id, participant_id, principal_id) ON DELETE CASCADE,
    FOREIGN KEY (room_id, session_id, principal_id)
        REFERENCES sessions(room_id, session_id, principal_id) ON DELETE CASCADE
);

CREATE INDEX workload_executions_live_session_idx
    ON workload_executions(session_id, expires_at DESC)
    WHERE revoked_at IS NULL;
