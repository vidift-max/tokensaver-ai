#!/usr/bin/env bash
set -euo pipefail

: "${DATABASE_URL:?DATABASE_URL must point to a disposable PostgreSQL 16 database}"

ROOT="$(git rev-parse --show-toplevel)"
MIGRATIONS="$ROOT/services/ai-team-room/migrations"
SCHEMA="ai_phone_legacy_compatible_fixture"
TENANT="10000000-0000-4000-8000-000000000001"
PHONE="90000000-0000-4000-8000-000000000009"
AI="90000000-0000-4000-8000-000000000001"
# Explicit fixture approval: this legacy phone has exactly one tenant binding.
APPROVED_MAPPING="$TENANT:$PHONE"

psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -c "DROP SCHEMA IF EXISTS $SCHEMA CASCADE"
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -c "CREATE SCHEMA $SCHEMA"
export PGOPTIONS="-c search_path=$SCHEMA"

for migration in "$MIGRATIONS"/000{1..9}_*.sql "$MIGRATIONS"/0010_*.sql "$MIGRATIONS"/0011_ai_phone_foundation.sql; do
  psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -1 -f "$migration"
done

psql "$DATABASE_URL" -v ON_ERROR_STOP=1 <<'SQL'
INSERT INTO tenants(tenant_id,name) VALUES
 ('10000000-0000-4000-8000-000000000001','legacy-compatible-tenant');
INSERT INTO rooms(room_id,tenant_id,name) VALUES
 ('10000000-0000-4000-8000-000000000011','10000000-0000-4000-8000-000000000001','legacy-compatible-room');
INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES
 ('10000000-0000-4000-8000-000000000101','10000000-0000-4000-8000-000000000001','AI','legacy-test','legacy-compatible-ai');
INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES
 ('10000000-0000-4000-8000-000000000111','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000101','legacy-compatible-ai');
INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status) VALUES
 ('10000000-0000-4000-8000-000000000121','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000011','10000000-0000-4000-8000-000000000101',NOW()-INTERVAL '1 minute',NOW()+INTERVAL '1 hour','ACTIVE');
INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES
 ('10000000-0000-4000-8000-000000000131','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000011','10000000-0000-4000-8000-000000000111','10000000-0000-4000-8000-000000000101','10000000-0000-4000-8000-000000000121','legacy-compatible',NOW(),NOW()+INTERVAL '1 hour','ACTIVE');
INSERT INTO team_tasks(task_id,tenant_id,room_id,mission_id,title,created_by_principal_id) VALUES
 ('10000000-0000-4000-8000-000000000141','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000011','10000000-0000-4000-8000-000000000151','legacy compatible task','10000000-0000-4000-8000-000000000101');
INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES
 ('90000000-0000-4000-8000-000000000001','legacy:compatible-ai','TEST','Legacy compatible AI');
INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES
 ('90000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000101');
INSERT INTO ai_phones(ai_phone_id,ai_id,endpoint_fence) VALUES
 ('90000000-0000-4000-8000-000000000009','90000000-0000-4000-8000-000000000001',4);
INSERT INTO phone_endpoints(endpoint_id,ai_phone_id,endpoint_type,endpoint_status) VALUES
 ('10000000-0000-4000-8000-000000000161','90000000-0000-4000-8000-000000000009','TEST','ATTACHED');
INSERT INTO phone_attachments(attachment_id,ai_phone_id,ai_id,endpoint_id,tenant_id,room_id,principal_id,participant_id,session_id,execution_id,endpoint_fence,attachment_status) VALUES
 ('10000000-0000-4000-8000-000000000171','90000000-0000-4000-8000-000000000009','90000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000161','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000011','10000000-0000-4000-8000-000000000101','10000000-0000-4000-8000-000000000111','10000000-0000-4000-8000-000000000121','10000000-0000-4000-8000-000000000131',4,'ACTIVE');
INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content,message_state) VALUES
 ('10000000-0000-4000-8000-000000000181','90000000-0000-4000-8000-000000000009','10000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000101','90000000-0000-4000-8000-000000000001',1,'legacy-compatible-message','{"text":"preserve legacy history"}','ACK');
INSERT INTO phone_mission_links(ai_phone_id,task_id) VALUES
 ('90000000-0000-4000-8000-000000000009','10000000-0000-4000-8000-000000000141');
SQL

# Refuse to call the fixture "approved" unless its explicit mapping is present
# and the database proves the legacy phone has exactly one tenant principal binding.
[[ "$APPROVED_MAPPING" == "$TENANT:$PHONE" ]]
bindings="$(psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -Atc "SELECT COUNT(DISTINCT tenant_id) FROM ai_identity_principals WHERE ai_id='$AI'")"
[[ "$bindings" == "1" ]] || { echo "FAIL: legacy identity is not unambiguously bound"; exit 1; }

psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -1 -f "$MIGRATIONS/0012_ai_phone_tenant_isolation.sql"
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -1 -f "$MIGRATIONS/0013_team_room_tenant_scope_constraints.sql"

actual="$(psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -Atc "SELECT
 (SELECT count(*) FROM ai_phones WHERE ai_phone_id='$PHONE' AND ai_id='$AI' AND tenant_id='$TENANT')||'|'||
 (SELECT count(*) FROM phone_endpoints WHERE endpoint_id='10000000-0000-4000-8000-000000000161' AND ai_phone_id='$PHONE' AND tenant_id='$TENANT' AND ai_id='$AI')||'|'||
 (SELECT count(*) FROM phone_attachments WHERE attachment_id='10000000-0000-4000-8000-000000000171' AND ai_phone_id='$PHONE' AND tenant_id='$TENANT' AND ai_id='$AI' AND attachment_status='ACTIVE')||'|'||
 (SELECT count(*) FROM phone_messages WHERE message_id='10000000-0000-4000-8000-000000000181' AND ai_phone_id='$PHONE' AND tenant_id='$TENANT' AND message_state='ACK')||'|'||
 (SELECT count(*) FROM phone_mission_links WHERE ai_phone_id='$PHONE' AND tenant_id='$TENANT' AND task_id='10000000-0000-4000-8000-000000000141')"
)"
expected="1|1|1|1|1"
if [[ "$actual" != "$expected" ]]; then
  echo "FAIL: compatible legacy upgrade lost or mis-scoped rows; actual=$actual expected=$expected"
  exit 1
fi

# Verify tenant-scoped foreign keys exist after both migrations.
fk_count="$(psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -Atc "SELECT COUNT(*) FROM pg_constraint WHERE connamespace='$SCHEMA'::regnamespace AND contype='f' AND conname IN ('ai_phones_tenant_binding_fk','phone_endpoints_phone_tenant_ai_fk','phone_attachments_phone_tenant_ai_fk','phone_attachments_endpoint_phone_tenant_ai_fk','phone_messages_phone_tenant_ai_fk','phone_mission_links_phone_tenant_fk','phone_mission_links_task_tenant_room_fk')")"
[[ "$fk_count" == "7" ]] || { echo "FAIL: expected 7 tenant-scoped AI-PHONE FKs, found $fk_count"; exit 1; }

echo "PASS: explicit unambiguous legacy mapping $APPROVED_MAPPING"
echo "PASS: migrations 0012 and 0013 applied successfully to compatible legacy schema"
echo "PASS: AI_PHONE_ID preserved; endpoint, attachment, ACKed message and mission link preserved with tenant scope"
echo "PASS: 7 tenant-scoped AI-PHONE foreign keys present"

unset PGOPTIONS
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -c "DROP SCHEMA $SCHEMA CASCADE"
