\pset tuples_only on
\pset format unaligned
SELECT 'tenants=' || count(*) FROM lab.inventory_tenants;
SELECT 'identities=' || count(*) FROM lab.inventory_identities;
SELECT 'identity_tenant_bindings=' || count(*) FROM lab.inventory_identity_tenants;
SELECT 'phones=' || count(*) FROM lab.inventory_phones;
SELECT 'endpoints=' || count(*) FROM lab.inventory_endpoints;
SELECT 'attachments=' || count(*) FROM lab.inventory_attachments;
SELECT 'messages_metadata=' || count(*) FROM lab.inventory_messages;
SELECT 'tasks=' || count(*) FROM lab.inventory_tasks;
SELECT 'checkpoints_metadata=' || count(*) FROM lab.inventory_checkpoints;
SELECT 'audit_metadata=' || count(*) FROM lab.inventory_audit;
SELECT 'outbox_metadata=' || count(*) FROM lab.inventory_outbox;
SELECT 'global_identity_tenant_links=' || count(*) FROM lab.inventory_identity_tenants WHERE ai_id='ai-global-1';
SELECT 'synthetic_phones=' || string_agg(ai_phone_id,',' ORDER BY ai_phone_id) FROM lab.inventory_phones;
