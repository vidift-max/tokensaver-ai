-- Mission: persistent general/private messaging, decisions, attachment references and WS tickets.
CREATE TABLE team_room_channels (
  channel_id UUID PRIMARY KEY,
  tenant_id UUID NOT NULL,
  room_id UUID NOT NULL,
  channel_type TEXT NOT NULL CHECK (channel_type IN ('GENERAL','PRIVATE')),
  created_by_principal_id UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (tenant_id,room_id) REFERENCES rooms(tenant_id,room_id) ON DELETE CASCADE,
  FOREIGN KEY (tenant_id,created_by_principal_id) REFERENCES principals(tenant_id,principal_id),
  UNIQUE (tenant_id,room_id,channel_id)
);
CREATE UNIQUE INDEX team_room_one_general_channel_idx ON team_room_channels(room_id) WHERE channel_type='GENERAL';
CREATE TABLE team_room_channel_members (
  channel_id UUID NOT NULL REFERENCES team_room_channels(channel_id) ON DELETE CASCADE,
  principal_id UUID NOT NULL REFERENCES principals(principal_id) ON DELETE CASCADE,
  added_by_principal_id UUID NOT NULL REFERENCES principals(principal_id),
  joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY(channel_id,principal_id)
);
CREATE TABLE team_room_messages (
  message_id UUID PRIMARY KEY,
  tenant_id UUID NOT NULL,
  room_id UUID NOT NULL,
  channel_id UUID NOT NULL REFERENCES team_room_channels(channel_id) ON DELETE CASCADE,
  sender_principal_id UUID NOT NULL REFERENCES principals(principal_id),
  client_message_id UUID NOT NULL,
  parent_message_id UUID REFERENCES team_room_messages(message_id),
  body TEXT NOT NULL CHECK (length(trim(body)) BETWEEN 1 AND 12000),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  edited_at TIMESTAMPTZ,
  deleted_at TIMESTAMPTZ,
  UNIQUE(channel_id,sender_principal_id,client_message_id),
  FOREIGN KEY (tenant_id,room_id) REFERENCES rooms(tenant_id,room_id) ON DELETE CASCADE
);
CREATE INDEX team_room_messages_history_idx ON team_room_messages(channel_id,created_at DESC,message_id DESC);
CREATE TABLE team_room_message_reads (
  channel_id UUID NOT NULL REFERENCES team_room_channels(channel_id) ON DELETE CASCADE,
  principal_id UUID NOT NULL REFERENCES principals(principal_id) ON DELETE CASCADE,
  last_read_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY(channel_id,principal_id)
);
CREATE TABLE team_room_decisions (
  decision_id UUID PRIMARY KEY,
  tenant_id UUID NOT NULL,
  room_id UUID NOT NULL,
  author_principal_id UUID NOT NULL REFERENCES principals(principal_id),
  mission_id UUID,
  title TEXT NOT NULL CHECK(length(trim(title)) BETWEEN 1 AND 300),
  decision TEXT NOT NULL CHECK(length(trim(decision)) BETWEEN 1 AND 12000),
  participants UUID[] NOT NULL DEFAULT '{}',
  reference_message_id UUID REFERENCES team_room_messages(message_id),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (tenant_id,room_id) REFERENCES rooms(tenant_id,room_id) ON DELETE CASCADE
);
CREATE TABLE team_room_attachment_refs (
  attachment_id UUID PRIMARY KEY,
  tenant_id UUID NOT NULL,
  room_id UUID NOT NULL,
  channel_id UUID NOT NULL REFERENCES team_room_channels(channel_id) ON DELETE CASCADE,
  uploaded_by_principal_id UUID NOT NULL REFERENCES principals(principal_id),
  message_id UUID REFERENCES team_room_messages(message_id),
  task_id UUID,
  file_name TEXT NOT NULL CHECK(length(trim(file_name)) BETWEEN 1 AND 255),
  storage_ref TEXT NOT NULL CHECK(length(trim(storage_ref)) BETWEEN 1 AND 1024),
  content_type TEXT,
  byte_len BIGINT CHECK(byte_len IS NULL OR byte_len>=0),
  sha256 TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (tenant_id,room_id) REFERENCES rooms(tenant_id,room_id) ON DELETE CASCADE
);
CREATE TABLE team_room_ws_tickets (
  ticket_hash BYTEA PRIMARY KEY,
  tenant_id UUID NOT NULL,
  room_id UUID NOT NULL,
  principal_id UUID NOT NULL,
  session_id UUID NOT NULL,
  expires_at TIMESTAMPTZ NOT NULL,
  consumed_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (tenant_id,room_id) REFERENCES rooms(tenant_id,room_id) ON DELETE CASCADE,
  FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
);
CREATE INDEX team_room_ws_ticket_expiry_idx ON team_room_ws_tickets(expires_at) WHERE consumed_at IS NULL;

-- Transactional outbox notification. The payload remains in PostgreSQL; NOTIFY carries only its UUID.
CREATE OR REPLACE FUNCTION notify_team_room_outbox() RETURNS TRIGGER AS $$
BEGIN
  PERFORM pg_notify('team_room_outbox', NEW.outbox_event_id::text);
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER team_room_outbox_notify
AFTER INSERT ON outbox_events FOR EACH ROW EXECUTE FUNCTION notify_team_room_outbox();
