#!/usr/bin/env bash
set -euo pipefail

: "${DATABASE_URL:?DATABASE_URL must point to the disposable PostgreSQL 16 CI database}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
MIGRATIONS="$ROOT/services/ai-team-room/migrations"
MIGRATION_0012="$MIGRATIONS/0012_ai_phone_tenant_isolation.sql"
RUN_SUFFIX="${GITHUB_RUN_ID:-$$}"

create_legacy_db() {
  local scenario="$1"
  local db="ai_phone_legacy_${scenario}_${RUN_SUFFIX}"
  local url="${DATABASE_URL%/*}/$db"
  createdb --maintenance-db="$DATABASE_URL" "$db"
  for migration in "$MIGRATIONS"/[0-9][0-9][0-9][0-9]_*.sql; do
    [[ "$(basename "$migration")" < "0012_"* ]] || continue
    psql "$url" -v ON_ERROR_STOP=1 -f "$migration" >/dev/null
  done
  printf '%s\n' "$url"
}

cleanup_dbs=()
cleanup() {
  for db in "${cleanup_dbs[@]}"; do
    dropdb --maintenance-db="$DATABASE_URL" --if-exists "$db" || true
  done
}
trap cleanup EXIT

seed_legacy() {
  local url="$1"
  local shared="$2"
  psql "$url" -v ON_ERROR_STOP=1 -v shared="$shared" <<'SQL'
INSERT INTO tenants(tenant_id,name) VALUES
 ('10000000-0000-0000-0000-000000000001','legacy-tenant-a'),
 ('10000000-0000-0000-0000-000000000002','legacy-tenant-b');
INSERT INTO rooms(room_id,tenant_id,name) VALUES
 ('20000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','legacy-room-a'),
 ('20000000-0000-0000-0000-000000000002','10000000-0000-0000-0000-000000000002','legacy-room-b');
INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES
 ('30000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','AI','legacy-test','ai-a'),
 ('30000000-0000-0000-0000-000000000002','10000000-0000-0000-0000-000000000002','AI','legacy-test','ai-b');
INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES
 ('40000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001','legacy-participant-a'),
 ('40000000-0000-0000-0000-000000000002','10000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000002','legacy-participant-b');
INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status) VALUES
 ('50000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',NOW(),NOW()+INTERVAL '1 hour','ACTIVE'),
 ('50000000-0000-0000-0000-000000000002','10000000-0000-0000-0000-000000000002','20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000002',NOW(),NOW()+INTERVAL '1 hour','ACTIVE');
INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES
 ('60000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001','40000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000001','legacy-worker-a',NOW(),NOW()+INTERVAL '1 hour','ACTIVE'),
 ('60000000-0000-0000-0000-000000000002','10000000-0000-0000-0000-000000000002','20000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000002','50000000-0000-0000-0000-000000000002','legacy-worker-b',NOW(),NOW()+INTERVAL '1 hour','ACTIVE');
INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES
 ('70000000-0000-0000-0000-000000000001','legacy:shared-ai','TEST','Legacy shared AI');
INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES
 ('70000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001');
\if :shared
INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES
 ('70000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000002');
\endif
INSERT INTO team_tasks(task_id,tenant_id,room_id,mission_id,title,created_by_principal_id,checkpoint,checkpoint_version) VALUES
 ('80000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001','90000000-0000-0000-0000-000000000001','Legacy task A','30000000-0000-0000-0000-000000000001','{"checkpoint":"preserve-a"}',1);
INSERT INTO ai_phones(ai_phone_id,ai_id,phone_status,endpoint_fence,current_mission_id,current_task_id) VALUES
 ('a0000000-0000-0000-0000-000000000001','70000000-0000-0000-0000-000000000001','ATTACHED',2,'90000000-0000-0000-0000-000000000001','80000000-0000-0000-0000-000000000001');
INSERT INTO phone_endpoints(endpoint_id,ai_phone_id,endpoint_type,endpoint_status,metadata) VALUES
 ('b0000000-0000-0000-0000-000000000001','a0000000-0000-0000-0000-000000000001','TEST','ATTACHED','{"owner":"tenant-a"}');
\if :shared
INSERT INTO phone_endpoints(endpoint_id,ai_phone_id,endpoint_type,endpoint_status,metadata) VALUES
 ('b0000000-0000-0000-0000-000000000002','a0000000-0000-0000-0000-000000000001','TEST','DETACHED','{"owner":"tenant-b"}');
\endif
INSERT INTO phone_attachments(attachment_id,ai_phone_id,ai_id,endpoint_id,tenant_id,room_id,principal_id,participant_id,session_id,execution_id,endpoint_fence,attachment_status) VALUES
 ('c0000000-0000-0000-0000-000000000001','a0000000-0000-0000-0000-000000000001','70000000-0000-0000-0000-000000000001','b0000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001','40000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000001','60000000-0000-0000-0000-000000000001',1,'ACTIVE');
\if :shared
INSERT INTO phone_attachments(attachment_id,ai_phone_id,ai_id,endpoint_id,tenant_id,room_id,principal_id,participant_id,session_id,execution_id,endpoint_fence,attachment_status) VALUES
 ('c0000000-0000-0000-0000-000000000002','a0000000-0000-0000-0000-000000000001','70000000-0000-0000-0000-000000000001','b0000000-0000-0000-0000-000000000002','10000000-0000-0000-0000-000000000002','20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000002','50000000-0000-0000-0000-000000000002','60000000-0000-0000-0000-000000000002',2,'DETACHED');
\endif
INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES
 ('d0000000-0000-0000-0000-000000000001','a0000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001','70000000-0000-0000-0000-000000000001',1,'legacy-a','{"text":"tenant A message"}');
\if :shared
INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES
 ('d0000000-0000-0000-0000-000000000002','a0000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000002','70000000-0000-0000-0000-000000000001',2,'legacy-b','{"text":"tenant B message"}');
\endif
INSERT INTO phone_mission_links(ai_phone_id,task_id) VALUES
 ('a0000000-0000-0000-0000-000000000001','80000000-0000-0000-0000-000000000001');
SQL
}

snapshot() {
  psql "$1" -v ON_ERROR_STOP=1 -Atc "
    SELECT
      (SELECT COUNT(*) FROM ai_phones)||':'||
      (SELECT COUNT(*) FROM phone_endpoints)||':'||
      (SELECT COUNT(*) FROM phone_attachments)||':'||
      (SELECT COUNT(*) FROM phone_messages)||':'||
      (SELECT COUNT(*) FROM phone_mission_links)"
}

run_shared_legacy_case() {
  local db="ai_phone_legacy_shared_${RUN_SUFFIX}"
  local url
  url="$(create_legacy_db shared)"
  cleanup_dbs+=("$db")
  seed_legacy "$url" 1
  local before after output status
  before="$(snapshot "$url")"
  [[ "$before" == "1:2:2:2:1" ]] || { echo "Unexpected shared legacy fixture counts: $before" >&2; exit 1; }

  set +e
  output="$(psql "$url" -v ON_ERROR_STOP=1 --single-transaction -f "$MIGRATION_0012" 2>&1)"
  status=$?
  set -e
  if [[ "$status" -eq 0 ]]; then
    echo "FAIL: migration unexpectedly accepted a shared legacy AI_PHONE_ID" >&2
    exit 1
  fi
  grep -F "AI-PHONE-01 migration blocked: an existing AI_PHONE_ID is shared across tenants" <<<"$output"
  after="$(snapshot "$url")"
  [[ "$after" == "$before" ]] || { echo "FAIL: rollback changed legacy row counts: before=$before after=$after" >&2; exit 1; }
  local tenant_column
  tenant_column="$(psql "$url" -Atc "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema='public' AND table_name='ai_phones' AND column_name='tenant_id'")"
  [[ "$tenant_column" == "0" ]] || { echo "FAIL: migration DDL survived rollback" >&2; exit 1; }
  echo "PASS: shared legacy migration failed closed; exact PostgreSQL error captured; transaction rollback preserved phones/endpoints/attachments/messages/mission link ($after)"
}

run_compatible_legacy_case() {
  local db="ai_phone_legacy_compatible_${RUN_SUFFIX}"
  local url
  url="$(create_legacy_db compatible)"
  cleanup_dbs+=("$db")
  seed_legacy "$url" 0
  local before after
  before="$(snapshot "$url")"
  [[ "$before" == "1:1:1:1:1" ]] || { echo "Unexpected compatible legacy fixture counts: $before" >&2; exit 1; }
  psql "$url" -v ON_ERROR_STOP=1 --single-transaction -f "$MIGRATION_0012"
  after="$(snapshot "$url")"
  [[ "$after" == "$before" ]] || { echo "FAIL: compatible migration changed row counts: before=$before after=$after" >&2; exit 1; }
  local mapped
  mapped="$(psql "$url" -Atc "
    SELECT
      (SELECT tenant_id FROM ai_phones WHERE ai_phone_id='a0000000-0000-0000-0000-000000000001')||':'||
      (SELECT tenant_id FROM phone_endpoints WHERE endpoint_id='b0000000-0000-0000-0000-000000000001')||':'||
      (SELECT tenant_id FROM phone_attachments WHERE attachment_id='c0000000-0000-0000-0000-000000000001')||':'||
      (SELECT tenant_id FROM phone_messages WHERE message_id='d0000000-0000-0000-0000-000000000001')||':'||
      (SELECT tenant_id FROM phone_mission_links WHERE ai_phone_id='a0000000-0000-0000-0000-000000000001')")"
  [[ "$mapped" == "10000000-0000-0000-0000-000000000001:10000000-0000-0000-0000-000000000001:10000000-0000-0000-0000-000000000001:10000000-0000-0000-0000-000000000001:10000000-0000-0000-0000-000000000001" ]] || {
    echo "FAIL: compatible legacy ownership backfill mismatch: $mapped" >&2
    exit 1
  }
  echo "PASS: compatible legacy migration applied; all legacy resource rows preserved and mapped to tenant A ($after)"
}

run_shared_legacy_case
run_compatible_legacy_case
