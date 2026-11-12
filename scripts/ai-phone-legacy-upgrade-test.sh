#!/usr/bin/env bash
set -euo pipefail

: "${DATABASE_URL:?DATABASE_URL must point to a disposable PostgreSQL 16 database}"

ROOT="$(git rev-parse --show-toplevel)"
MIGRATIONS="$ROOT/services/ai-team-room/migrations"
SCHEMA="ai_phone_legacy_fixture"
EXPECTED="AI-PHONE-01 migration blocked: an existing AI_PHONE_ID is shared across tenants"

psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -c "DROP SCHEMA IF EXISTS $SCHEMA CASCADE"
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -c "CREATE SCHEMA $SCHEMA"
export PGOPTIONS="-c search_path=$SCHEMA"

# Build the actual pre-isolation schema from the repository's original migrations.
for migration in "$MIGRATIONS"/000{1..9}_*.sql "$MIGRATIONS"/0010_*.sql "$MIGRATIONS"/0011_ai_phone_foundation.sql; do
  psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -1 -f "$migration"
done

# Seed a real legacy collision: one global AI_ID and AI_PHONE_ID mapped to two tenants.
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 <<'SQL'
INSERT INTO tenants(tenant_id,name) VALUES
 ('10000000-0000-4000-8000-000000000001','legacy-tenant-a'),
 ('20000000-0000-4000-8000-000000000002','legacy-tenant-b');
INSERT INTO rooms(room_id,tenant_id,name) VALUES
 ('10000000-0000-4000-8000-000000000011','10000000-0000-4000-8000-000000000001','legacy-room-a'),
 ('20000000-0000-4000-8000-000000000022','20000000-0000-4000-8000-000000000002','legacy-room-b');
INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES
 ('10000000-0000-4000-8000-000000000101','10000000-0000-4000-8000-000000000001','AI','legacy-test','ai-a'),
 ('20000000-0000-4000-8000-000000000202','20000000-0000-4000-8000-000000000002','AI','legacy-test','ai-b');
INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES
 ('10000000-0000-4000-8000-000000000111','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000101','legacy-ai-a'),
 ('20000000-0000-4000-8000-000000000222','20000000-0000-4000-8000-000000000002','20000000-0000-4000-8000-000000000202','legacy-ai-b');
INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status) VALUES
 ('10000000-0000-4000-8000-000000000121','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000011','10000000-0000-4000-8000-000000000101',NOW()-INTERVAL '1 minute',NOW()+INTERVAL '1 hour','ACTIVE'),
 ('20000000-0000-4000-8000-000000000222','20000000-0000-4000-8000-000000000002','20000000-0000-4000-8000-000000000022','20000000-0000-4000-8000-000000000202',NOW()-INTERVAL '1 minute',NOW()+INTERVAL '1 hour','ACTIVE');
INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES
 ('10000000-0000-4000-8000-000000000131','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000011','10000000-0000-4000-8000-000000000111','10000000-0000-4000-8000-000000000101','10000000-0000-4000-8000-000000000121','legacy-a',NOW(),NOW()+INTERVAL '1 hour','ACTIVE'),
 ('20000000-0000-4000-8000-000000000232','20000000-0000-4000-8000-000000000002','20000000-0000-4000-8000-000000000022','20000000-0000-4000-8000-000000000222','20000000-0000-4000-8000-000000000202','20000000-0000-4000-8000-000000000222','legacy-b',NOW(),NOW()+INTERVAL '1 hour','ACTIVE');
INSERT INTO team_tasks(task_id,tenant_id,room_id,mission_id,title,created_by_principal_id) VALUES
 ('10000000-0000-4000-8000-000000000141','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000011','10000000-0000-4000-8000-000000000151','legacy task A','10000000-0000-4000-8000-000000000101');
INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES
 ('90000000-0000-4000-8000-000000000001','legacy:shared-ai','TEST','Legacy shared AI');
INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES
 ('90000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000101'),
 ('90000000-0000-4000-8000-000000000001','20000000-0000-4000-8000-000000000002','20000000-0000-4000-8000-000000000202');
INSERT INTO ai_phones(ai_phone_id,ai_id,endpoint_fence) VALUES
 ('90000000-0000-4000-8000-000000000009','90000000-0000-4000-8000-000000000001',2);
INSERT INTO phone_endpoints(endpoint_id,ai_phone_id,endpoint_type,endpoint_status) VALUES
 ('10000000-0000-4000-8000-000000000161','90000000-0000-4000-8000-000000000009','TEST','DETACHED'),
 ('20000000-0000-4000-8000-000000000262','90000000-0000-4000-8000-000000000009','TEST','ATTACHED');
INSERT INTO phone_attachments(attachment_id,ai_phone_id,ai_id,endpoint_id,tenant_id,room_id,principal_id,participant_id,session_id,execution_id,endpoint_fence,attachment_status) VALUES
 ('10000000-0000-4000-8000-000000000171','90000000-0000-4000-8000-000000000009','90000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000161','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000011','10000000-0000-4000-8000-000000000101','10000000-0000-4000-8000-000000000111','10000000-0000-4000-8000-000000000121','10000000-0000-4000-8000-000000000131',1,'DETACHED'),
 ('20000000-0000-4000-8000-000000000272','90000000-0000-4000-8000-000000000009','90000000-0000-4000-8000-000000000001','20000000-0000-4000-8000-000000000262','20000000-0000-4000-8000-000000000002','20000000-0000-4000-8000-000000000022','20000000-0000-4000-8000-000000000202','20000000-0000-4000-8000-000000000222','20000000-0000-4000-8000-000000000222','20000000-0000-4000-8000-000000000232',2,'ACTIVE');
INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES
 ('10000000-0000-4000-8000-000000000181','90000000-0000-4000-8000-000000000009','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000101','90000000-0000-4000-8000-000000000001',1,'legacy-message-a','{"text":"tenant A history"}'),
 ('20000000-0000-4000-8000-000000000282','90000000-0000-4000-8000-000000000009','20000000-0000-4000-8000-000000000002','20000000-0000-4000-8000-000000000202','90000000-0000-4000-8000-000000000001',2,'legacy-message-b','{"text":"tenant B history"}');
INSERT INTO phone_mission_links(ai_phone_id,task_id) VALUES
 ('90000000-0000-4000-8000-000000000009','10000000-0000-4000-8000-000000000141');
SQL

# Apply the actual repository migration. It must fail for the intended guard,
# and PostgreSQL must roll back the complete migration transaction.
set +e
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -1 -f "$MIGRATIONS/0012_ai_phone_tenant_isolation.sql" > /tmp/ai-phone-legacy-migration.log 2>&1
migration_status=$?
set -e
if [[ $migration_status -eq 0 ]]; then
  echo "FAIL: legacy shared phone migration unexpectedly succeeded"
  cat /tmp/ai-phone-legacy-migration.log
  exit 1
fi
if ! grep -Fq "$EXPECTED" /tmp/ai-phone-legacy-migration.log; then
  echo "FAIL: migration failed for a reason other than the expected legacy guard"
  cat /tmp/ai-phone-legacy-migration.log
  exit 1
fi

# The failed transaction must leave the legacy schema and every history row intact.
actual="$(psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -Atc "SELECT
 (SELECT count(*) FROM ai_phones)||'|'||
 (SELECT count(*) FROM phone_endpoints)||'|'||
 (SELECT count(*) FROM phone_attachments)||'|'||
 (SELECT count(*) FROM phone_messages)||'|'||
 (SELECT count(*) FROM phone_mission_links)||'|'||
 (SELECT count(*) FROM information_schema.columns WHERE table_schema='$SCHEMA' AND table_name='ai_phones' AND column_name='tenant_id')")"
expected="1|2|2|2|1|0"
if [[ "$actual" != "$expected" ]]; then
  echo "FAIL: legacy DB changed or lost history after fail-closed migration; actual=$actual expected=$expected"
  exit 1
fi

echo "PASS: actual migration 0012 failed closed with expected shared-phone guard"
echo "PASS: transaction rollback preserved 1 phone, 2 endpoints, 2 attachments, 2 messages, 1 mission link"
echo "PASS: pre-isolation ai_phones schema remains unchanged (tenant_id column absent)"
cat /tmp/ai-phone-legacy-migration.log

unset PGOPTIONS
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -c "DROP SCHEMA $SCHEMA CASCADE"
