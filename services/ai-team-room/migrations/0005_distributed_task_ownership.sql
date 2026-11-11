-- Distributed task ownership, leases and monotonic fencing (Mission: distributed continuity).
CREATE TABLE team_tasks (
    task_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    room_id UUID NOT NULL,
    mission_id UUID NOT NULL,
    title TEXT NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 500),
    status TEXT NOT NULL DEFAULT 'UNOWNED'
        CHECK (status IN ('UNOWNED','OWNED','LEASE_EXPIRED','RECOVERY_REQUIRED','HANDOFF_PENDING','REASSIGNED','COMPLETED','CANCELLED')),
    owner_principal_id UUID,
    owner_participant_id UUID,
    owner_session_id UUID,
    owner_execution_id UUID,
    fencing_token BIGINT NOT NULL DEFAULT 0 CHECK (fencing_token >= 0),
    lease_issued_at TIMESTAMPTZ,
    lease_expires_at TIMESTAMPTZ,
    lease_renewed_at TIMESTAMPTZ,
    checkpoint JSONB NOT NULL DEFAULT '{}'::JSONB,
    checkpoint_version BIGINT NOT NULL DEFAULT 0 CHECK (checkpoint_version >= 0),
    created_by_principal_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    CHECK (
      (status IN ('OWNED','REASSIGNED') AND owner_principal_id IS NOT NULL
       AND owner_participant_id IS NOT NULL AND owner_session_id IS NOT NULL
       AND owner_execution_id IS NOT NULL AND lease_issued_at IS NOT NULL
       AND lease_expires_at IS NOT NULL)
      OR
      (status NOT IN ('OWNED','REASSIGNED') AND
       ((owner_principal_id IS NULL AND owner_participant_id IS NULL AND owner_session_id IS NULL
         AND owner_execution_id IS NULL AND lease_issued_at IS NULL AND lease_expires_at IS NULL)
        OR status IN ('LEASE_EXPIRED','RECOVERY_REQUIRED','HANDOFF_PENDING')))
    ),
    FOREIGN KEY (tenant_id, room_id) REFERENCES rooms(tenant_id, room_id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, created_by_principal_id) REFERENCES principals(tenant_id, principal_id),
    FOREIGN KEY (tenant_id, owner_participant_id, owner_principal_id)
        REFERENCES participants(tenant_id, participant_id, principal_id),
    FOREIGN KEY (room_id, owner_session_id, owner_principal_id)
        REFERENCES sessions(room_id, session_id, principal_id)
);
CREATE INDEX team_tasks_room_status_idx ON team_tasks(room_id,status,updated_at DESC);
CREATE INDEX team_tasks_owner_lease_idx ON team_tasks(owner_principal_id,lease_expires_at)
    WHERE status IN ('OWNED','REASSIGNED');

CREATE TABLE task_lease_events (
    lease_event_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    room_id UUID NOT NULL,
    task_id UUID NOT NULL REFERENCES team_tasks(task_id) ON DELETE CASCADE,
    principal_id UUID,
    participant_id UUID,
    session_id UUID,
    fencing_token BIGINT NOT NULL CHECK (fencing_token > 0),
    event_type TEXT NOT NULL CHECK (event_type IN ('ACQUIRED','RENEWED','CHECKPOINTED','EXPIRED','RELEASED','FENCED','COMPLETED','RECOVERY_QUEUED')),
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(task_id,fencing_token,event_type)
);
CREATE INDEX task_lease_events_task_time_idx ON task_lease_events(task_id,created_at DESC);

CREATE TABLE task_handoffs (
    handoff_id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES team_tasks(task_id) ON DELETE CASCADE,
    tenant_id UUID NOT NULL,
    from_principal_id UUID,
    to_principal_id UUID,
    from_fencing_token BIGINT NOT NULL,
    to_fencing_token BIGINT,
    status TEXT NOT NULL CHECK (status IN ('HANDOFF_PENDING','HANDOFF_ACCEPTED','RESUMED','COMPLETED','CANCELLED')),
    context JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    accepted_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    FOREIGN KEY (tenant_id,from_principal_id) REFERENCES principals(tenant_id,principal_id),
    FOREIGN KEY (tenant_id,to_principal_id) REFERENCES principals(tenant_id,principal_id)
);
CREATE UNIQUE INDEX task_handoff_one_pending_idx ON task_handoffs(task_id)
    WHERE status='HANDOFF_PENDING';

-- Every mutation of protected task state must compare the current fencing token.
-- The task row is the serialization point; no local process lock is authoritative.


-- Database-level monotonicity guard: ownership cannot move to another executor
-- without advancing the fence exactly once. Completed work is immutable.
CREATE FUNCTION enforce_team_task_fencing() RETURNS TRIGGER AS $$
BEGIN
    IF OLD.status IN ('COMPLETED','CANCELLED') AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'terminal task is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.fencing_token < OLD.fencing_token OR NEW.fencing_token > OLD.fencing_token + 1 THEN
        RAISE EXCEPTION 'fencing token must be monotonic and advance by at most one' USING ERRCODE='23514';
    END IF;
    IF NEW.owner_principal_id IS NOT NULL AND
       (NEW.owner_principal_id,NEW.owner_participant_id,NEW.owner_session_id,NEW.owner_execution_id)
       IS DISTINCT FROM
       (OLD.owner_principal_id,OLD.owner_participant_id,OLD.owner_session_id,OLD.owner_execution_id)
       AND NEW.fencing_token <> OLD.fencing_token + 1 THEN
        RAISE EXCEPTION 'new owner requires a strictly newer fencing token' USING ERRCODE='23514';
    END IF;
    IF NEW.checkpoint_version < OLD.checkpoint_version OR NEW.checkpoint_version > OLD.checkpoint_version + 1 THEN
        RAISE EXCEPTION 'checkpoint version must be monotonic and advance by at most one' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER team_tasks_fencing_guard
BEFORE UPDATE ON team_tasks
FOR EACH ROW EXECUTE FUNCTION enforce_team_task_fencing();
