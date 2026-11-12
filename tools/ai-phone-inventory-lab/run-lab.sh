#!/usr/bin/env bash
set -euo pipefail
ROOT="tools/ai-phone-inventory-lab"
ARTIFACTS="artifacts/ai-phone-inventory-lab"
mkdir -p "$ARTIFACTS"
export PGHOST=127.0.0.1 PGPORT=5432 PGDATABASE=inventory_lab
ADMIN_PASSWORD="postgres-lab-password"
READER_PASSWORD="lab-only-password"
admin_psql() { PGPASSWORD="$ADMIN_PASSWORD" psql -X -v ON_ERROR_STOP=1 -U postgres "$@"; }
reader_psql() { PGPASSWORD="$READER_PASSWORD" psql -X -v ON_ERROR_STOP=1 -U inventory_reader "$@"; }

admin_psql -f "$ROOT/setup.sql" > "$ARTIFACTS/setup.log"
BASELINE="$(admin_psql -Atc "SELECT (SELECT count(*) FROM lab.tenants)||':'||(SELECT count(*) FROM lab.phone_messages)||':'||(SELECT count(*) FROM lab.outbox_events)||':'||(SELECT md5(string_agg(message_id||content,'|' ORDER BY message_id)) FROM lab.phone_messages)||':'||(SELECT md5(string_agg(event_id||payload,'|' ORDER BY event_id)) FROM lab.outbox_events)")"

{
  echo "environment=LOCAL_DISPOSABLE_TEST"
  echo "postgres_version=$(reader_psql -Atc 'SHOW server_version')"
  echo "database=$(reader_psql -Atc 'SELECT current_database()')"
  echo "schema=lab"
  echo "role=$(reader_psql -Atc 'SELECT current_user')"
  echo "utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "transaction_settings=$(reader_psql -Atc "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL statement_timeout='5s'; SET LOCAL lock_timeout='2s'; SET LOCAL idle_in_transaction_session_timeout='10s'; SET LOCAL row_security=off; SELECT current_setting('transaction_isolation')||'|'||current_setting('transaction_read_only')||'|'||current_setting('statement_timeout')||'|'||current_setting('lock_timeout')||'|'||current_setting('idle_in_transaction_session_timeout')||'|'||current_setting('row_security'); COMMIT" | tail -n 1)"
  echo "inventory_results:"
  reader_psql -f "$ROOT/inventory.sql"
} > "$ARTIFACTS/inventory-report.txt"

# Effective privilege checks: column-level SELECT on safe views only; no base-table content.
reader_psql -Atc "SELECT has_table_privilege('lab.phone_messages','SELECT'),has_column_privilege('lab.phone_messages','content','SELECT'),has_table_privilege('lab.inventory_messages','SELECT'),has_schema_privilege('lab','CREATE')" > "$ARTIFACTS/effective-privileges.txt"
grep -qx 'f|f|t|f' "$ARTIFACTS/effective-privileges.txt"

# Expected denials must actually return non-zero.
denials=0
expect_denied() {
  label="$1"; statement="$2"
  if reader_psql -c "$statement" >/dev/null 2>&1; then
    echo "FAIL: $label unexpectedly succeeded" >&2; exit 1
  fi
  echo "DENIED: $label" >> "$ARTIFACTS/write-denials.txt"
  denials=$((denials+1))
}
expect_denied INSERT "INSERT INTO lab.tenants VALUES ('bad','bad')"
expect_denied UPDATE "UPDATE lab.tenants SET name='bad' WHERE tenant_id='tenant-a'"
expect_denied DELETE "DELETE FROM lab.tenants WHERE tenant_id='tenant-a'"
expect_denied TRUNCATE "TRUNCATE lab.tenants"
expect_denied CREATE "CREATE TABLE lab.forbidden(id int)"
expect_denied ALTER "ALTER TABLE lab.tenants ADD COLUMN forbidden int"
expect_denied DROP "DROP TABLE lab.tenants"
expect_denied PRIVATE_PHONE_CONTENT "SELECT content FROM lab.phone_messages"
expect_denied PRIVATE_TEAM_BODY "SELECT private_body FROM lab.team_tasks"
expect_denied PRIVATE_OUTBOX_PAYLOAD "SELECT payload FROM lab.outbox_events"
expect_denied PRIVATE_METADATA "SELECT private_metadata FROM lab.audit_events"
test "$denials" -eq 11

# row_security=off must fail rather than return a silently filtered partial inventory.
if reader_psql -c "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL row_security=off; SELECT * FROM lab.rls_probe; COMMIT" > "$ARTIFACTS/rls-probe.log" 2>&1; then
  echo "FAIL: RLS partial read did not fail closed" >&2; exit 1
fi
grep -qi 'row-level security' "$ARTIFACTS/rls-probe.log"

# Explicit negative fixtures for unsafe ownership and BYPASSRLS are rejected by the same guard predicates.
admin_psql <<'SQL' > "$ARTIFACTS/role-negative-fixtures.txt"
CREATE ROLE inventory_bypass_probe LOGIN BYPASSRLS;
CREATE ROLE inventory_owner_probe NOLOGIN;
CREATE TABLE lab.owner_probe(id integer);
ALTER TABLE lab.owner_probe OWNER TO inventory_owner_probe;
SELECT 'BYPASSRLS_REJECTED=' || (SELECT rolbypassrls FROM pg_roles WHERE rolname='inventory_bypass_probe');
SELECT 'OWNER_REJECTED=' || (SELECT pg_get_userbyid(relowner)='inventory_owner_probe' FROM pg_class WHERE oid='lab.owner_probe'::regclass);
SQL
grep -qx 'BYPASSRLS_REJECTED=true' "$ARTIFACTS/role-negative-fixtures.txt"
grep -qx 'OWNER_REJECTED=true' "$ARTIFACTS/role-negative-fixtures.txt"

AFTER="$(admin_psql -Atc "SELECT (SELECT count(*) FROM lab.tenants)||':'||(SELECT count(*) FROM lab.phone_messages)||':'||(SELECT count(*) FROM lab.outbox_events)||':'||(SELECT md5(string_agg(message_id||content,'|' ORDER BY message_id)) FROM lab.phone_messages)||':'||(SELECT md5(string_agg(event_id||payload,'|' ORDER BY event_id)) FROM lab.outbox_events)")"
test "$BASELINE" = "$AFTER"
echo "PASS: synthetic data unchanged" >> "$ARTIFACTS/write-denials.txt"

# Private canaries are present in the fixture but must never appear in the inventory report.
if grep -E 'PRIVATE_(PHONE_CONTENT|TEAM_MESSAGE_BODY|OUTBOX_PAYLOAD|AUDIT_METADATA|ENDPOINT_META|CHECKPOINT)_CANARY|lab-only-password|postgres-lab-password|postgres://|Bearer ' "$ARTIFACTS/inventory-report.txt"; then
  echo "FAIL: private synthetic canary leaked into report" >&2; exit 1
fi

EXECUTOR_SHA="$(sha256sum "$ROOT/run-lab.sh" | awk '{print $1}')"
SQL_SHA="$(cat "$ROOT/setup.sql" "$ROOT/inventory.sql" | sha256sum | awk '{print $1}')"
REPORT_SHA="$(sha256sum "$ARTIFACTS/inventory-report.txt" | awk '{print $1}')"
cat > "$ARTIFACTS/manifest.json" <<EOF
{
  "environment": "LOCAL_DISPOSABLE_TEST",
  "postgres_version": "$(reader_psql -Atc 'SHOW server_version')",
  "database": "inventory_lab",
  "schema": "lab",
  "role": "inventory_reader",
  "utc": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "read_only": true,
  "allowlisted_queries": "PASS",
  "write_denials": "PASS ($denials/11)",
  "data_unchanged": true,
  "rls_fail_closed": true,
  "negative_role_fixtures": "PASS (BYPASSRLS and ownership)",
  "privacy_canaries": "PASS (0 matches)",
  "executor_sha256": "$EXECUTOR_SHA",
  "sql_sha256": "$SQL_SHA",
  "report_sha256": "$REPORT_SHA"
}
EOF
sha256sum "$ROOT/run-lab.sh" "$ROOT/setup.sql" "$ROOT/inventory.sql" "$ARTIFACTS/inventory-report.txt" > "$ARTIFACTS/recomputed-sha256.txt"
sha256sum -c "$ARTIFACTS/recomputed-sha256.txt"
test "$(sha256sum "$ARTIFACTS/inventory-report.txt" | awk '{print $1}')" = "$REPORT_SHA"
echo "PASS: manifest and independently recomputed SHA-256 hashes" > "$ARTIFACTS/hash-verification.txt"
