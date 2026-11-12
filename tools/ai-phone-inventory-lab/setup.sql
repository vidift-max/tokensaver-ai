\set ON_ERROR_STOP on
CREATE ROLE inventory_reader LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS NOINHERIT;
ALTER ROLE inventory_reader PASSWORD 'lab-only-password';
CREATE SCHEMA lab AUTHORIZATION postgres;
REVOKE ALL ON SCHEMA lab FROM PUBLIC;
GRANT USAGE ON SCHEMA lab TO inventory_reader;

CREATE TABLE lab.tenants(tenant_id text PRIMARY KEY, name text NOT NULL);
CREATE TABLE lab.identities(ai_id text PRIMARY KEY, identity_kind text NOT NULL);
CREATE TABLE lab.identity_tenants(ai_id text NOT NULL, tenant_id text NOT NULL, principal_id text NOT NULL);
CREATE TABLE lab.ai_phones(ai_phone_id text PRIMARY KEY, ai_id text NOT NULL, status text NOT NULL);
CREATE TABLE lab.endpoints(endpoint_id text PRIMARY KEY, ai_phone_id text NOT NULL, endpoint_type text NOT NULL, private_metadata text NOT NULL);
CREATE TABLE lab.attachments(attachment_id text PRIMARY KEY, ai_phone_id text NOT NULL, endpoint_id text NOT NULL, tenant_id text NOT NULL, status text NOT NULL);
CREATE TABLE lab.phone_messages(message_id text PRIMARY KEY, ai_phone_id text NOT NULL, tenant_id text NOT NULL, sequence_no bigint NOT NULL, state text NOT NULL, created_at timestamptz NOT NULL, content text NOT NULL);
CREATE TABLE lab.team_tasks(task_id text PRIMARY KEY, tenant_id text NOT NULL, status text NOT NULL, checkpoint_version integer NOT NULL, private_body text NOT NULL);
CREATE TABLE lab.checkpoints(task_id text PRIMARY KEY, version integer NOT NULL, updated_at timestamptz NOT NULL, private_checkpoint text NOT NULL);
CREATE TABLE lab.audit_events(event_id text PRIMARY KEY, tenant_id text NOT NULL, action text NOT NULL, result text NOT NULL, created_at timestamptz NOT NULL, private_metadata text NOT NULL);
CREATE TABLE lab.outbox_events(event_id text PRIMARY KEY, tenant_id text NOT NULL, aggregate_type text NOT NULL, created_at timestamptz NOT NULL, payload text NOT NULL);

INSERT INTO lab.tenants VALUES ('tenant-a','Synthetic Tenant A'),('tenant-b','Synthetic Tenant B');
INSERT INTO lab.identities VALUES ('ai-global-1','TEST');
INSERT INTO lab.identity_tenants VALUES
 ('ai-global-1','tenant-a','principal-a'),('ai-global-1','tenant-b','principal-b');
INSERT INTO lab.ai_phones VALUES
 ('phone-a','ai-global-1','ATTACHED'),('phone-b','ai-global-1','DISCONNECTED');
INSERT INTO lab.endpoints VALUES
 ('endpoint-a','phone-a','WEB','PRIVATE_ENDPOINT_META_CANARY'),
 ('endpoint-b','phone-b','MOBILE','PRIVATE_ENDPOINT_META_CANARY');
INSERT INTO lab.attachments VALUES
 ('attachment-a','phone-a','endpoint-a','tenant-a','ACTIVE'),
 ('attachment-b','phone-b','endpoint-b','tenant-b','DETACHED');
INSERT INTO lab.phone_messages VALUES
 ('message-a','phone-a','tenant-a',1,'PENDING',now(),'PRIVATE_PHONE_CONTENT_CANARY'),
 ('message-b','phone-b','tenant-b',2,'ACK',now(),'PRIVATE_PHONE_CONTENT_CANARY');
INSERT INTO lab.team_tasks VALUES
 ('task-a','tenant-a','RUNNING',4,'PRIVATE_TEAM_MESSAGE_BODY_CANARY'),
 ('task-b','tenant-b','DONE',8,'PRIVATE_TEAM_MESSAGE_BODY_CANARY');
INSERT INTO lab.checkpoints VALUES
 ('task-a',4,now(),'PRIVATE_CHECKPOINT_CANARY'),
 ('task-b',8,now(),'PRIVATE_CHECKPOINT_CANARY');
INSERT INTO lab.audit_events VALUES
 ('audit-a','tenant-a','phone.attach','SUCCESS',now(),'PRIVATE_AUDIT_METADATA_CANARY');
INSERT INTO lab.outbox_events VALUES
 ('outbox-a','tenant-a','phone',now(),'PRIVATE_OUTBOX_PAYLOAD_CANARY');

CREATE VIEW lab.inventory_tenants AS SELECT tenant_id,name FROM lab.tenants;
CREATE VIEW lab.inventory_identities AS SELECT ai_id,identity_kind FROM lab.identities;
CREATE VIEW lab.inventory_identity_tenants AS SELECT ai_id,tenant_id,principal_id FROM lab.identity_tenants;
CREATE VIEW lab.inventory_phones AS SELECT ai_phone_id,ai_id,status FROM lab.ai_phones;
CREATE VIEW lab.inventory_endpoints AS SELECT endpoint_id,ai_phone_id,endpoint_type FROM lab.endpoints;
CREATE VIEW lab.inventory_attachments AS SELECT attachment_id,ai_phone_id,endpoint_id,tenant_id,status FROM lab.attachments;
CREATE VIEW lab.inventory_messages AS SELECT message_id,ai_phone_id,tenant_id,sequence_no,state,created_at FROM lab.phone_messages;
CREATE VIEW lab.inventory_tasks AS SELECT task_id,tenant_id,status,checkpoint_version FROM lab.team_tasks;
CREATE VIEW lab.inventory_checkpoints AS SELECT task_id,version,updated_at FROM lab.checkpoints;
CREATE VIEW lab.inventory_audit AS SELECT event_id,tenant_id,action,result,created_at FROM lab.audit_events;
CREATE VIEW lab.inventory_outbox AS SELECT event_id,tenant_id,aggregate_type,created_at FROM lab.outbox_events;

GRANT SELECT ON lab.inventory_tenants,lab.inventory_identities,lab.inventory_identity_tenants,
 lab.inventory_phones,lab.inventory_endpoints,lab.inventory_attachments,lab.inventory_messages,
 lab.inventory_tasks,lab.inventory_checkpoints,lab.inventory_audit,lab.inventory_outbox
 TO inventory_reader;

CREATE TABLE lab.rls_probe(tenant_id text NOT NULL, safe_marker text NOT NULL);
INSERT INTO lab.rls_probe VALUES ('tenant-a','VISIBLE_SYNTHETIC_ROW'),('tenant-b','HIDDEN_SYNTHETIC_ROW');
ALTER TABLE lab.rls_probe ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_a_only ON lab.rls_probe FOR SELECT TO inventory_reader USING (tenant_id='tenant-a');
GRANT SELECT (tenant_id,safe_marker) ON lab.rls_probe TO inventory_reader;

DO $$
BEGIN
 IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='inventory_reader' AND (rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls OR rolinherit)) THEN
   RAISE EXCEPTION 'inventory_reader role attributes invalid';
 END IF;
 IF pg_has_role('inventory_reader','pg_read_all_data','MEMBER') THEN
   RAISE EXCEPTION 'inventory_reader unexpectedly inherits pg_read_all_data';
 END IF;
END $$;
