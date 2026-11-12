-- AI-PHONE-01 tenant isolation (forward-only).
-- Global AI_ID remains global. Every tenant binding owns a separate AI_PHONE_ID.
-- The migration fails closed for every already-provisioned phone shared by multiple tenants.
-- It never guesses which tenant keeps a legacy AI_PHONE_ID or silently rekeys another tenant.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM ai_identity_principals GROUP BY ai_id,tenant_id HAVING COUNT(*)>1) THEN
        RAISE EXCEPTION 'AI-PHONE-01 migration blocked: one global AI identity has multiple principal bindings in the same tenant';
    END IF;
    IF EXISTS (
        SELECT p.ai_phone_id FROM ai_phones p JOIN ai_identity_principals ip ON ip.ai_id=p.ai_id
        GROUP BY p.ai_phone_id HAVING COUNT(DISTINCT ip.tenant_id)>1
    ) THEN
        RAISE EXCEPTION 'AI-PHONE-01 migration blocked: an existing AI_PHONE_ID is shared across tenants; an operator must approve an explicit tenant-to-phone identity mapping before migration';
    END IF;
    IF EXISTS (SELECT 1 FROM ai_phones p WHERE NOT EXISTS (SELECT 1 FROM ai_identity_principals ip WHERE ip.ai_id=p.ai_id)) THEN
        RAISE EXCEPTION 'AI-PHONE-01 migration blocked: an existing phone has no tenant principal binding';
    END IF;
    IF EXISTS (
        SELECT 1 FROM ai_phones p
        JOIN (SELECT ai_id FROM ai_identity_principals GROUP BY ai_id HAVING COUNT(DISTINCT tenant_id)>1) shared ON shared.ai_id=p.ai_id
        WHERE EXISTS (SELECT 1 FROM phone_endpoints e WHERE e.ai_phone_id=p.ai_phone_id)
           OR EXISTS (SELECT 1 FROM phone_attachments a WHERE a.ai_phone_id=p.ai_phone_id)
    ) THEN
        RAISE EXCEPTION 'AI-PHONE-01 migration blocked: shared legacy phone has endpoint/attachment history without tenant ownership; reviewed endpoint backfill required';
    END IF;
    IF EXISTS (
        SELECT 1 FROM phone_messages m JOIN ai_phones p ON p.ai_phone_id=m.ai_phone_id
        WHERE NOT EXISTS (SELECT 1 FROM ai_identity_principals ip WHERE ip.ai_id=p.ai_id AND ip.tenant_id=m.tenant_id)
    ) THEN
        RAISE EXCEPTION 'AI-PHONE-01 migration blocked: a legacy message tenant does not match the phone AI identity binding';
    END IF;
    IF EXISTS (
        SELECT 1 FROM phone_attachments a JOIN ai_phones p ON p.ai_phone_id=a.ai_phone_id
        WHERE NOT EXISTS (SELECT 1 FROM ai_identity_principals ip WHERE ip.ai_id=p.ai_id AND ip.tenant_id=a.tenant_id)
    ) THEN
        RAISE EXCEPTION 'AI-PHONE-01 migration blocked: a legacy attachment tenant does not match the phone AI identity binding';
    END IF;
    IF EXISTS (
        SELECT 1 FROM ai_phones p JOIN team_tasks t ON t.task_id=p.current_task_id
        WHERE NOT EXISTS (SELECT 1 FROM ai_identity_principals ip WHERE ip.ai_id=p.ai_id AND ip.tenant_id=t.tenant_id)
    ) THEN
        RAISE EXCEPTION 'AI-PHONE-01 migration blocked: current task tenant does not match the phone AI identity binding';
    END IF;
    IF EXISTS (SELECT 1 FROM phone_mission_links l LEFT JOIN team_tasks t ON t.task_id=l.task_id WHERE t.task_id IS NULL) THEN
        RAISE EXCEPTION 'AI-PHONE-01 migration blocked: mission link references a missing Team Room task';
    END IF;
    IF EXISTS (
        SELECT 1 FROM phone_mission_links l JOIN ai_phones p ON p.ai_phone_id=l.ai_phone_id
        JOIN team_tasks t ON t.task_id=l.task_id
        WHERE NOT EXISTS (SELECT 1 FROM ai_identity_principals ip WHERE ip.ai_id=p.ai_id AND ip.tenant_id=t.tenant_id)
    ) THEN
        RAISE EXCEPTION 'AI-PHONE-01 migration blocked: mission task tenant has no binding for the linked global AI';
    END IF;
END $$;

ALTER TABLE ai_identity_principals
    ADD CONSTRAINT ai_identity_principals_ai_tenant_unique UNIQUE (ai_id,tenant_id);

CREATE TABLE ai_identity_tenants (
    ai_id UUID NOT NULL REFERENCES ai_identities(ai_id) ON DELETE RESTRICT,
    tenant_id UUID NOT NULL REFERENCES tenants(tenant_id) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (ai_id, tenant_id),
    FOREIGN KEY (ai_id,tenant_id) REFERENCES ai_identity_principals(ai_id,tenant_id) ON DELETE RESTRICT
);
INSERT INTO ai_identity_tenants(ai_id, tenant_id) SELECT DISTINCT ai_id, tenant_id FROM ai_identity_principals;

ALTER TABLE ai_phones ADD COLUMN tenant_id UUID;
ALTER TABLE ai_phones DROP CONSTRAINT ai_phones_ai_id_key;
CREATE TEMP TABLE ai_phone_tenant_backfill ON COMMIT DROP AS
SELECT p.ai_phone_id AS legacy_phone_id, p.ai_id, p.current_task_id AS legacy_current_task_id,
       p.current_mission_id AS legacy_current_mission_id, ip.tenant_id,
       p.ai_phone_id AS target_phone_id
FROM ai_phones p
JOIN ai_identity_principals ip ON ip.ai_id=p.ai_id;

-- Existing AI_PHONE_ID values are preserved exactly. Shared legacy phone rows
-- were rejected above and require an operator-approved tenant rekey plan.
UPDATE ai_phones p
SET tenant_id=m.tenant_id
FROM ai_phone_tenant_backfill m
WHERE p.ai_phone_id=m.legacy_phone_id;

ALTER TABLE phone_endpoints ADD COLUMN tenant_id UUID;
ALTER TABLE phone_endpoints ADD COLUMN ai_id UUID;
UPDATE phone_endpoints e SET tenant_id=b.tenant_id,ai_id=b.ai_id
FROM ai_phone_tenant_backfill b
WHERE b.legacy_phone_id=e.ai_phone_id;

ALTER TABLE phone_attachments DROP CONSTRAINT phone_attachments_ai_phone_id_endpoint_id_fkey;
ALTER TABLE phone_mission_links ADD COLUMN tenant_id UUID;
ALTER TABLE phone_mission_links ADD COLUMN room_id UUID;
UPDATE phone_mission_links l SET tenant_id=t.tenant_id,room_id=t.room_id
FROM team_tasks t WHERE t.task_id=l.task_id;
UPDATE ai_phones p SET current_task_id=l.task_id,current_mission_id=t.mission_id
FROM phone_mission_links l JOIN team_tasks t ON t.task_id=l.task_id AND t.tenant_id=l.tenant_id
WHERE p.ai_phone_id=l.ai_phone_id AND p.tenant_id=l.tenant_id;

ALTER TABLE ai_phones ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE phone_endpoints ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE phone_endpoints ALTER COLUMN ai_id SET NOT NULL;
ALTER TABLE phone_mission_links ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE phone_mission_links ALTER COLUMN room_id SET NOT NULL;

ALTER TABLE ai_phones
    ADD CONSTRAINT ai_phones_tenant_ai_unique UNIQUE (tenant_id,ai_id),
    ADD CONSTRAINT ai_phones_phone_tenant_unique UNIQUE (ai_phone_id,tenant_id),
    ADD CONSTRAINT ai_phones_phone_tenant_ai_unique UNIQUE (ai_phone_id,tenant_id,ai_id),
    ADD CONSTRAINT ai_phones_tenant_binding_fk FOREIGN KEY (ai_id,tenant_id)
        REFERENCES ai_identity_tenants(ai_id,tenant_id) ON DELETE RESTRICT;

ALTER TABLE phone_endpoints
    ADD CONSTRAINT phone_endpoints_phone_tenant_ai_fk FOREIGN KEY (ai_phone_id,tenant_id,ai_id)
        REFERENCES ai_phones(ai_phone_id,tenant_id,ai_id) ON DELETE RESTRICT,
    ADD CONSTRAINT phone_endpoints_endpoint_phone_tenant_ai_unique UNIQUE (endpoint_id,ai_phone_id,tenant_id,ai_id);
CREATE INDEX phone_endpoints_tenant_phone_idx ON phone_endpoints(tenant_id,ai_phone_id,created_at DESC);

ALTER TABLE phone_attachments
    ADD CONSTRAINT phone_attachments_phone_tenant_ai_fk FOREIGN KEY (ai_phone_id,tenant_id,ai_id)
        REFERENCES ai_phones(ai_phone_id,tenant_id,ai_id) ON DELETE RESTRICT,
    ADD CONSTRAINT phone_attachments_endpoint_phone_tenant_ai_fk FOREIGN KEY (endpoint_id,ai_phone_id,tenant_id,ai_id)
        REFERENCES phone_endpoints(endpoint_id,ai_phone_id,tenant_id,ai_id) ON DELETE RESTRICT;
CREATE INDEX phone_attachments_tenant_phone_idx ON phone_attachments(tenant_id,ai_phone_id,attachment_status);

ALTER TABLE phone_messages
    ADD CONSTRAINT phone_messages_phone_tenant_ai_fk FOREIGN KEY (ai_phone_id,tenant_id,recipient_ai_id)
        REFERENCES ai_phones(ai_phone_id,tenant_id,ai_id) ON DELETE RESTRICT;
CREATE INDEX phone_messages_tenant_inbox_idx ON phone_messages(tenant_id,ai_phone_id,sequence_no)
    WHERE message_state IN ('PENDING','DELIVERED');
CREATE INDEX phone_messages_sender_history_idx ON phone_messages(tenant_id,sender_principal_id,created_at DESC);

CREATE UNIQUE INDEX team_tasks_task_tenant_room_unique ON team_tasks(task_id,tenant_id,room_id);
ALTER TABLE phone_mission_links
    ADD CONSTRAINT phone_mission_links_phone_tenant_fk FOREIGN KEY (ai_phone_id,tenant_id)
        REFERENCES ai_phones(ai_phone_id,tenant_id) ON DELETE RESTRICT,
    ADD CONSTRAINT phone_mission_links_task_tenant_room_fk FOREIGN KEY (task_id,tenant_id,room_id)
        REFERENCES team_tasks(task_id,tenant_id,room_id) ON DELETE RESTRICT;
CREATE INDEX phone_mission_links_tenant_task_idx ON phone_mission_links(tenant_id,room_id,task_id);

ALTER TABLE ai_phones DROP CONSTRAINT ai_phones_current_task_id_fkey;
ALTER TABLE team_tasks ADD CONSTRAINT team_tasks_task_tenant_unique UNIQUE (task_id,tenant_id);
ALTER TABLE ai_phones
    ADD CONSTRAINT ai_phones_current_task_tenant_fk FOREIGN KEY (current_task_id,tenant_id)
        REFERENCES team_tasks(task_id,tenant_id) ON DELETE SET NULL (current_task_id);

INSERT INTO permission_catalog(permission,description,privileged) VALUES
 ('phone.provision','Provision a permanent AI phone for an enabled AI principal in the authenticated tenant',TRUE)
ON CONFLICT (permission) DO NOTHING;

COMMENT ON TABLE ai_identity_tenants IS 'Canonical tenant-scoped binding of a global AI identity; one AI phone is owned by each row.';
COMMENT ON COLUMN ai_phones.tenant_id IS 'Ownership boundary. AI_ID remains global; AI_PHONE_ID is unique per tenant/AI binding.';
