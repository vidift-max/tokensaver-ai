-- Read-only PostgreSQL profiling for a pre-seeded Team Room dataset.
-- Run with psql -X -v ON_ERROR_STOP=1 and UUID variables supplied explicitly:
-- psql "$DATABASE_URL" -X -v tenant_id=... -v room_id=... -v channel_id=... -f scripts/benchmark/team-room-postgres.sql
-- EXPLAIN ANALYZE executes each SELECT. Run only against a benchmark/staging database.
\echo '=== Dataset cardinality ==='
SELECT 'tenant' AS relation, count(*) FROM tenants WHERE tenant_id=:'tenant_id'::uuid
UNION ALL SELECT 'rooms', count(*) FROM rooms WHERE tenant_id=:'tenant_id'::uuid
UNION ALL SELECT 'participants_in_room', count(DISTINCT participant_id) FROM room_memberships WHERE room_id=:'room_id'::uuid
UNION ALL SELECT 'room_memberships', count(*) FROM room_memberships WHERE room_id=:'room_id'::uuid
UNION ALL SELECT 'active_presence_rows', count(*) FROM room_memberships WHERE room_id=:'room_id'::uuid AND status='ACTIVE'
UNION ALL SELECT 'channels', count(*) FROM team_room_channels WHERE room_id=:'room_id'::uuid
UNION ALL SELECT 'messages', count(*) FROM team_room_messages WHERE tenant_id=:'tenant_id'::uuid AND room_id=:'room_id'::uuid
UNION ALL SELECT 'tasks', count(*) FROM team_tasks WHERE tenant_id=:'tenant_id'::uuid AND room_id=:'room_id'::uuid
UNION ALL SELECT 'decisions', count(*) FROM team_room_decisions WHERE tenant_id=:'tenant_id'::uuid AND room_id=:'room_id'::uuid
UNION ALL SELECT 'attachment_refs', count(*) FROM team_room_attachment_refs WHERE tenant_id=:'tenant_id'::uuid AND room_id=:'room_id'::uuid;

\echo '=== Message history ==='
EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS, FORMAT TEXT)
SELECT m.message_id,m.room_id,m.channel_id,m.sender_principal_id,p.display_name,
       m.parent_message_id,m.body,m.created_at,m.edited_at,m.deleted_at
FROM team_room_messages m
JOIN participants p ON p.principal_id=m.sender_principal_id AND p.tenant_id=m.tenant_id
WHERE m.channel_id=:'channel_id'::uuid AND m.deleted_at IS NULL
ORDER BY m.created_at DESC,m.message_id DESC
LIMIT 51;

\echo '=== Unread count ==='
EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS, FORMAT TEXT)
SELECT count(*)
FROM team_room_messages m
LEFT JOIN team_room_message_reads r
  ON r.channel_id=m.channel_id AND r.principal_id=:'principal_id'::uuid
WHERE m.channel_id=:'channel_id'::uuid
  AND m.sender_principal_id<>:'principal_id'::uuid
  AND m.deleted_at IS NULL
  AND m.created_at>COALESCE(r.last_read_at,'-infinity'::timestamptz);

\echo '=== Presence snapshot ==='
EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS, FORMAT TEXT)
SELECT m.participant_id,m.principal_id,m.session_id,p.display_name,pr.provider,
       m.last_seen_at,w.heartbeat_at,m.presence_state,
       (SELECT t.task_id FROM team_tasks t
        WHERE t.room_id=m.room_id AND t.owner_principal_id=m.principal_id
          AND t.status IN ('OWNED','REASSIGNED') AND t.lease_expires_at>NOW()
        ORDER BY t.updated_at DESC LIMIT 1) AS current_task_id
FROM room_memberships m
JOIN participants p ON p.participant_id=m.participant_id
JOIN principals pr ON pr.principal_id=m.principal_id
LEFT JOIN workload_executions w ON w.session_id=m.session_id
  AND w.status='ACTIVE' AND w.revoked_at IS NULL
WHERE m.room_id=:'room_id'::uuid AND m.status='ACTIVE'
ORDER BY m.joined_at;

\echo '=== Tasks ==='
EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS, FORMAT TEXT)
SELECT task_id,mission_id,status,owner_principal_id,owner_participant_id,
       owner_session_id,owner_execution_id,fencing_token,lease_expires_at,
       checkpoint,checkpoint_version
FROM team_tasks
WHERE tenant_id=:'tenant_id'::uuid AND room_id=:'room_id'::uuid
ORDER BY updated_at DESC
LIMIT 100;

\echo '=== Decisions ==='
EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS, FORMAT TEXT)
SELECT decision_id,author_principal_id,mission_id,title,decision,participants,
       reference_message_id,created_at
FROM team_room_decisions
WHERE tenant_id=:'tenant_id'::uuid AND room_id=:'room_id'::uuid
ORDER BY created_at DESC
LIMIT 100;

\echo '=== Attachment references ==='
EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS, FORMAT TEXT)
SELECT a.attachment_id,a.channel_id,a.message_id,a.task_id,a.file_name,
       a.storage_ref,a.content_type,a.byte_len,a.sha256,a.created_at
FROM team_room_attachment_refs a
JOIN team_room_channels c ON c.channel_id=a.channel_id
WHERE a.tenant_id=:'tenant_id'::uuid AND a.room_id=:'room_id'::uuid
ORDER BY a.created_at DESC
LIMIT 100;

\echo '=== Active database waits / locks (snapshot) ==='
SELECT a.pid,a.usename,a.application_name,a.state,a.wait_event_type,a.wait_event,
       now()-a.query_start AS query_age,l.locktype,l.mode,l.granted
FROM pg_stat_activity a
LEFT JOIN pg_locks l ON l.pid=a.pid
WHERE a.datname=current_database() AND a.pid<>pg_backend_pid()
ORDER BY a.query_start NULLS LAST;
