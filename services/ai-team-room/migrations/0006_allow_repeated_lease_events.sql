-- Lease event streams must allow repeated renewals and checkpoints within one fencing generation.
-- task_lease_events.lease_event_id remains the unique event identifier.
ALTER TABLE task_lease_events
    DROP CONSTRAINT IF EXISTS task_lease_events_task_id_fencing_token_event_type_key;
