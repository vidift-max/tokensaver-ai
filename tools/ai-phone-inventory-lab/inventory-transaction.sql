BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout='5s';
SET LOCAL lock_timeout='2s';
SET LOCAL idle_in_transaction_session_timeout='10s';
SET LOCAL row_security=off;
SELECT 'transaction_settings=' || current_setting('transaction_isolation') || '|' ||
       current_setting('transaction_read_only') || '|' ||
       current_setting('statement_timeout') || '|' ||
       current_setting('lock_timeout') || '|' ||
       current_setting('idle_in_transaction_session_timeout') || '|' ||
       current_setting('row_security');
\i tools/ai-phone-inventory-lab/inventory.sql
COMMIT;
