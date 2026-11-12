-- Carry a non-authoritative room hint in NOTIFY so listeners can discard
-- unrelated room events before performing an outbox row lookup.
-- The listener still reloads the row and verifies payload.room_id before delivery.
CREATE OR REPLACE FUNCTION notify_team_room_outbox() RETURNS TRIGGER AS $$
DECLARE
  room_hint TEXT;
BEGIN
  room_hint := NEW.payload->>'room_id';
  IF room_hint IS NOT NULL AND room_hint <> '' THEN
    PERFORM pg_notify('team_room_outbox', NEW.outbox_event_id::text || '|' || room_hint);
  ELSE
    PERFORM pg_notify('team_room_outbox', NEW.outbox_event_id::text);
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
