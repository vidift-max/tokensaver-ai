-- Snapshot PostgreSQL counters for the isolated benchmark window.
-- Run before and after the WebSocket/heartbeat probe in the same fresh database.
SELECT json_build_object(
  'captured_at', clock_timestamp(),
  'outbox_table', (
    SELECT json_build_object(
      'seq_scan', seq_scan,
      'seq_tup_read', seq_tup_read,
      'idx_scan', idx_scan,
      'idx_tup_fetch', idx_tup_fetch,
      'n_tup_ins', n_tup_ins,
      'n_tup_upd', n_tup_upd,
      'n_tup_del', n_tup_del
    )
    FROM pg_stat_user_tables
    WHERE relname='outbox_events'
  ),
  'outbox_event_counts', (
    SELECT COALESCE(json_object_agg(event_type,event_count),'{}'::json)
    FROM (
      SELECT event_type,count(*) AS event_count
      FROM outbox_events
      GROUP BY event_type
    ) counts
  ),
  'database', (
    SELECT json_build_object(
      'xact_commit',xact_commit,
      'xact_rollback',xact_rollback,
      'blks_hit',blks_hit,
      'blks_read',blks_read,
      'deadlocks',deadlocks,
      'temp_files',temp_files,
      'temp_bytes',temp_bytes
    )
    FROM pg_stat_database
    WHERE datname=current_database()
  ),
  'wal', (
    SELECT json_build_object(
      'wal_records',wal_records,
      'wal_fpi',wal_fpi,
      'wal_bytes',wal_bytes
    )
    FROM pg_stat_wal
  ),
  'active_waits', (
    SELECT count(*)
    FROM pg_stat_activity
    WHERE datname=current_database()
      AND pid<>pg_backend_pid()
      AND wait_event_type IS NOT NULL
  )
)::text;
