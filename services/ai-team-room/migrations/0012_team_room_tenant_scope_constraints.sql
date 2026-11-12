-- Team Room tenant-boundary hardening.
-- This migration is deliberately fail-closed: it never rewrites or reassigns
-- historical rows. If any existing relationship crosses tenant/room scope,
-- the preflight raises and the migration must be reconciled manually.

DO $$
BEGIN
  IF EXISTS (
    SELECT 1 FROM task_lease_events e
    JOIN team_tasks t ON t.task_id=e.task_id
    WHERE (e.tenant_id,e.room_id) IS DISTINCT FROM (t.tenant_id,t.room_id)
  ) THEN
    RAISE EXCEPTION 'TEAM_ROOM_0012_BLOCKED: task_lease_events tenant/room does not match team_tasks';
  END IF;

  IF EXISTS (
    SELECT 1 FROM task_handoffs h
    JOIN team_tasks t ON t.task_id=h.task_id
    WHERE (h.tenant_id,h.room_id) IS DISTINCT FROM (t.tenant_id,t.room_id)
  ) THEN
    RAISE EXCEPTION 'TEAM_ROOM_0012_BLOCKED: task_handoffs tenant/room does not match team_tasks';
  END IF;

  IF EXISTS (
    SELECT 1 FROM team_room_messages m
    JOIN team_room_channels c ON c.channel_id=m.channel_id
    WHERE (m.tenant_id,m.room_id) IS DISTINCT FROM (c.tenant_id,c.room_id)
  ) THEN
    RAISE EXCEPTION 'TEAM_ROOM_0012_BLOCKED: message tenant/room does not match channel';
  END IF;

  IF EXISTS (
    SELECT 1 FROM team_room_messages m
    JOIN team_room_messages p ON p.message_id=m.parent_message_id
    WHERE (m.tenant_id,m.room_id,m.channel_id) IS DISTINCT FROM (p.tenant_id,p.room_id,p.channel_id)
  ) THEN
    RAISE EXCEPTION 'TEAM_ROOM_0012_BLOCKED: parent message belongs to another tenant/room/channel';
  END IF;

  IF EXISTS (
    SELECT 1 FROM team_room_messages m
    JOIN principals p ON p.principal_id=m.sender_principal_id
    WHERE m.tenant_id<>p.tenant_id
  ) THEN
    RAISE EXCEPTION 'TEAM_ROOM_0012_BLOCKED: message sender belongs to another tenant';
  END IF;

  IF EXISTS (
    SELECT 1 FROM team_room_attachment_refs a
    JOIN team_room_channels c ON c.channel_id=a.channel_id
    WHERE (a.tenant_id,a.room_id) IS DISTINCT FROM (c.tenant_id,c.room_id)
  ) THEN
    RAISE EXCEPTION 'TEAM_ROOM_0012_BLOCKED: attachment tenant/room does not match channel';
  END IF;

  IF EXISTS (
    SELECT 1 FROM team_room_attachment_refs a
    JOIN team_room_messages m ON m.message_id=a.message_id
    WHERE (a.tenant_id,a.room_id,a.channel_id) IS DISTINCT FROM (m.tenant_id,m.room_id,m.channel_id)
  ) THEN
    RAISE EXCEPTION 'TEAM_ROOM_0012_BLOCKED: attachment message reference crosses tenant/room/channel';
  END IF;

  IF EXISTS (
    SELECT 1 FROM team_room_attachment_refs a
    JOIN team_tasks t ON t.task_id=a.task_id
    WHERE (a.tenant_id,a.room_id) IS DISTINCT FROM (t.tenant_id,t.room_id)
  ) THEN
    RAISE EXCEPTION 'TEAM_ROOM_0012_BLOCKED: attachment task reference crosses tenant/room';
  END IF;

  IF EXISTS (
    SELECT 1 FROM team_room_attachment_refs a
    JOIN principals p ON p.principal_id=a.uploaded_by_principal_id
    WHERE a.tenant_id<>p.tenant_id
  ) THEN
    RAISE EXCEPTION 'TEAM_ROOM_0012_BLOCKED: attachment uploader belongs to another tenant';
  END IF;

  IF EXISTS (
    SELECT 1 FROM team_room_ws_tickets w
    JOIN sessions s ON s.session_id=w.session_id
    WHERE (w.tenant_id,w.room_id,w.principal_id) IS DISTINCT FROM (s.tenant_id,s.room_id,s.principal_id)
  ) THEN
    RAISE EXCEPTION 'TEAM_ROOM_0012_BLOCKED: WebSocket ticket does not match session tenant/room/principal';
  END IF;
END;
$$;

-- Composite parent keys are explicit targets for tenant-aware foreign keys.
ALTER TABLE team_tasks
  ADD CONSTRAINT team_tasks_tenant_room_task_unique
  UNIQUE (tenant_id,room_id,task_id);

ALTER TABLE sessions
  ADD CONSTRAINT sessions_tenant_room_principal_session_unique
  UNIQUE (tenant_id,room_id,principal_id,session_id);

ALTER TABLE team_room_messages
  ADD CONSTRAINT team_room_messages_scope_unique
  UNIQUE (tenant_id,room_id,channel_id,message_id);

-- Task history and handoffs cannot point at a task in another tenant or room.
ALTER TABLE task_lease_events
  DROP CONSTRAINT IF EXISTS task_lease_events_task_id_fkey,
  ADD CONSTRAINT task_lease_events_task_scope_fk
    FOREIGN KEY (tenant_id,room_id,task_id)
    REFERENCES team_tasks(tenant_id,room_id,task_id) ON DELETE CASCADE;

ALTER TABLE task_handoffs
  DROP CONSTRAINT IF EXISTS task_handoffs_task_id_fkey,
  ADD CONSTRAINT task_handoffs_task_scope_fk
    FOREIGN KEY (tenant_id,room_id,task_id)
    REFERENCES team_tasks(tenant_id,room_id,task_id) ON DELETE CASCADE;

-- Messages are bound to the exact channel and sender tenant; replies remain
-- inside the same tenant, room and channel.
ALTER TABLE team_room_messages
  DROP CONSTRAINT IF EXISTS team_room_messages_channel_id_fkey,
  DROP CONSTRAINT IF EXISTS team_room_messages_sender_principal_id_fkey,
  DROP CONSTRAINT IF EXISTS team_room_messages_parent_message_id_fkey,
  ADD CONSTRAINT team_room_messages_channel_scope_fk
    FOREIGN KEY (tenant_id,room_id,channel_id)
    REFERENCES team_room_channels(tenant_id,room_id,channel_id) ON DELETE CASCADE,
  ADD CONSTRAINT team_room_messages_sender_tenant_fk
    FOREIGN KEY (tenant_id,sender_principal_id)
    REFERENCES principals(tenant_id,principal_id),
  ADD CONSTRAINT team_room_messages_parent_scope_fk
    FOREIGN KEY (tenant_id,room_id,channel_id,parent_message_id)
    REFERENCES team_room_messages(tenant_id,room_id,channel_id,message_id);

-- Attachment references cannot cross the channel, message, task or uploader
-- tenant boundary. Existing NULL optional references remain valid.
ALTER TABLE team_room_attachment_refs
  DROP CONSTRAINT IF EXISTS team_room_attachment_refs_channel_id_fkey,
  DROP CONSTRAINT IF EXISTS team_room_attachment_refs_uploaded_by_principal_id_fkey,
  DROP CONSTRAINT IF EXISTS team_room_attachment_refs_message_id_fkey,
  ADD CONSTRAINT team_room_attachment_channel_scope_fk
    FOREIGN KEY (tenant_id,room_id,channel_id)
    REFERENCES team_room_channels(tenant_id,room_id,channel_id) ON DELETE CASCADE,
  ADD CONSTRAINT team_room_attachment_uploader_tenant_fk
    FOREIGN KEY (tenant_id,uploaded_by_principal_id)
    REFERENCES principals(tenant_id,principal_id),
  ADD CONSTRAINT team_room_attachment_message_scope_fk
    FOREIGN KEY (tenant_id,room_id,channel_id,message_id)
    REFERENCES team_room_messages(tenant_id,room_id,channel_id,message_id),
  ADD CONSTRAINT team_room_attachment_task_scope_fk
    FOREIGN KEY (tenant_id,room_id,task_id)
    REFERENCES team_tasks(tenant_id,room_id,task_id);

-- A ticket is valid only for the exact tenant, room, principal and session tuple.
ALTER TABLE team_room_ws_tickets
  DROP CONSTRAINT IF EXISTS team_room_ws_tickets_session_id_fkey,
  ADD CONSTRAINT team_room_ws_ticket_session_scope_fk
    FOREIGN KEY (tenant_id,room_id,principal_id,session_id)
    REFERENCES sessions(tenant_id,room_id,principal_id,session_id) ON DELETE CASCADE;
