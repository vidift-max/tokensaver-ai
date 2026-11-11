ALTER TABLE room_memberships
  ADD COLUMN presence_state TEXT NOT NULL DEFAULT 'ONLINE'
  CHECK (presence_state IN ('ONLINE','AWAY','BUSY','EXECUTING','WAITING','BLOCKED'));
CREATE INDEX room_memberships_presence_idx ON room_memberships(room_id,presence_state,last_seen_at DESC);
