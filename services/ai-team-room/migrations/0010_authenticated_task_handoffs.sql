-- Authenticated, recipient-bound task handoffs. Existing automatic RESUMED
-- records are retained as ACCEPTED history; no existing task ownership is reset.
ALTER TABLE task_handoffs DROP CONSTRAINT IF EXISTS task_handoffs_status_check;

UPDATE task_handoffs SET status = CASE status
  WHEN 'HANDOFF_PENDING' THEN 'PENDING'
  WHEN 'HANDOFF_ACCEPTED' THEN 'ACCEPTED'
  WHEN 'RESUMED' THEN 'ACCEPTED'
  ELSE status
END;

ALTER TABLE task_handoffs
  ADD COLUMN room_id UUID,
  ADD COLUMN from_participant_id UUID,
  ADD COLUMN from_session_id UUID,
  ADD COLUMN from_execution_id UUID,
  ADD COLUMN to_participant_id UUID,
  ADD COLUMN to_session_id UUID,
  ADD COLUMN to_execution_id UUID,
  ADD COLUMN checkpoint_reference JSONB NOT NULL DEFAULT '{}'::JSONB,
  ADD COLUMN reason TEXT NOT NULL DEFAULT 'legacy_resume',
  ADD COLUMN expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '5 minutes');

UPDATE task_handoffs h SET room_id=t.room_id FROM team_tasks t WHERE t.task_id=h.task_id;

UPDATE task_handoffs
SET checkpoint_reference = COALESCE(context, '{}'::JSONB),
    reason = CASE WHEN reason = 'legacy_resume' THEN 'legacy ownership resume' ELSE reason END;

ALTER TABLE task_handoffs
  ALTER COLUMN room_id SET NOT NULL,
  ADD CONSTRAINT task_handoffs_room_fk FOREIGN KEY (tenant_id,room_id) REFERENCES rooms(tenant_id,room_id) ON DELETE CASCADE,
  ADD CONSTRAINT task_handoffs_status_check
    CHECK (status IN ('PENDING','ACCEPTED','REJECTED','EXPIRED','CANCELLED','COMPLETED')),
  ADD CONSTRAINT task_handoffs_reason_length_check
    CHECK (length(trim(reason)) BETWEEN 1 AND 1000),
  ADD CONSTRAINT task_handoffs_expiry_check
    CHECK (expires_at > created_at),
  ADD CONSTRAINT task_handoffs_from_participant_fk
    FOREIGN KEY (tenant_id, from_participant_id)
    REFERENCES participants(tenant_id, participant_id),
  ADD CONSTRAINT task_handoffs_from_session_fk FOREIGN KEY (from_session_id) REFERENCES sessions(session_id) ON DELETE SET NULL,
  ADD CONSTRAINT task_handoffs_from_execution_fk FOREIGN KEY (from_execution_id) REFERENCES workload_executions(execution_id) ON DELETE SET NULL,
  ADD CONSTRAINT task_handoffs_to_session_fk FOREIGN KEY (to_session_id) REFERENCES sessions(session_id) ON DELETE SET NULL,
  ADD CONSTRAINT task_handoffs_to_execution_fk FOREIGN KEY (to_execution_id) REFERENCES workload_executions(execution_id) ON DELETE SET NULL,
  ADD CONSTRAINT task_handoffs_to_participant_fk
    FOREIGN KEY (tenant_id, to_participant_id)
    REFERENCES participants(tenant_id, participant_id);

DROP INDEX IF EXISTS task_handoff_one_pending_idx;
CREATE UNIQUE INDEX task_handoff_one_pending_idx
  ON task_handoffs(task_id) WHERE status = 'PENDING';
CREATE INDEX task_handoff_target_status_idx
  ON task_handoffs(tenant_id, room_id, to_principal_id, status, expires_at);

CREATE OR REPLACE FUNCTION enforce_task_handoff_state_transition() RETURNS TRIGGER AS $$
BEGIN
  IF NEW.status = OLD.status THEN
    RETURN NEW;
  END IF;
  IF OLD.status = 'PENDING' AND NEW.status IN ('ACCEPTED','REJECTED','EXPIRED','CANCELLED') THEN
    RETURN NEW;
  END IF;
  IF OLD.status = 'ACCEPTED' AND NEW.status IN ('COMPLETED','CANCELLED') THEN
    RETURN NEW;
  END IF;
  RAISE EXCEPTION 'invalid task handoff state transition: % -> %', OLD.status, NEW.status
    USING ERRCODE = '23514';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER task_handoff_state_transition_guard
BEFORE UPDATE OF status ON task_handoffs
FOR EACH ROW EXECUTE FUNCTION enforce_task_handoff_state_transition();
