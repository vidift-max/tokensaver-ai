-- P2.33 identity lifecycle and directional phone-pair authorization.
-- No identity is inferred from client data. Revoked identities/grants are terminal.

ALTER TABLE ai_identities
  ADD COLUMN identity_status TEXT NOT NULL DEFAULT 'ACTIVE'
    CHECK (identity_status IN ('ACTIVE','RETIRED')),
  ADD COLUMN retired_at TIMESTAMPTZ,
  ADD COLUMN retired_by_principal_id UUID,
  ADD COLUMN retirement_reason TEXT,
  ADD CONSTRAINT ai_identity_retirement_state_check
    CHECK ((identity_status='RETIRED') = (retired_at IS NOT NULL)),
  ADD CONSTRAINT ai_identity_retirer_fk
    FOREIGN KEY (retired_by_principal_id) REFERENCES principals(principal_id) ON DELETE RESTRICT;

ALTER TABLE ai_identity_principals
  ADD COLUMN binding_status TEXT NOT NULL DEFAULT 'ACTIVE'
    CHECK (binding_status IN ('ACTIVE','REVOKED')),
  ADD COLUMN binding_version BIGINT NOT NULL DEFAULT 1 CHECK (binding_version > 0),
  ADD COLUMN revoked_at TIMESTAMPTZ,
  ADD COLUMN revoked_by_principal_id UUID,
  ADD COLUMN revocation_reason TEXT,
  ADD CONSTRAINT ai_identity_principal_revocation_state_check
    CHECK ((binding_status='REVOKED') = (revoked_at IS NOT NULL)),
  ADD CONSTRAINT ai_identity_principal_revoker_fk
    FOREIGN KEY (tenant_id,revoked_by_principal_id)
    REFERENCES principals(tenant_id,principal_id) ON DELETE RESTRICT;

ALTER TABLE ai_phones
  DROP CONSTRAINT ai_phones_phone_status_check,
  ADD CONSTRAINT ai_phones_phone_status_check
    CHECK (phone_status IN ('ATTACHED','TRANSFER_REQUESTED','TRANSFERRING','ATTACHED_NEW_ENDPOINT','DISCONNECTED','REVOKED')),
  ADD COLUMN revoked_at TIMESTAMPTZ,
  ADD COLUMN revoked_by_principal_id UUID,
  ADD COLUMN revocation_reason TEXT,
  ADD CONSTRAINT ai_phone_revocation_state_check
    CHECK ((phone_status='REVOKED') = (revoked_at IS NOT NULL)),
  ADD CONSTRAINT ai_phone_revoker_fk
    FOREIGN KEY (tenant_id,revoked_by_principal_id)
    REFERENCES principals(tenant_id,principal_id) ON DELETE RESTRICT;

CREATE TABLE ai_identity_binding_events (
  binding_event_id UUID PRIMARY KEY,
  ai_id UUID NOT NULL,
  tenant_id UUID NOT NULL,
  principal_id UUID NOT NULL,
  binding_version BIGINT NOT NULL,
  event_type TEXT NOT NULL CHECK (event_type IN ('CREATED','REVOKED')),
  actor_principal_id UUID,
  reason TEXT NOT NULL CHECK (length(trim(reason)) BETWEEN 1 AND 1000),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (ai_id,tenant_id,principal_id)
    REFERENCES ai_identity_principals(ai_id,tenant_id,principal_id) ON DELETE RESTRICT,
  FOREIGN KEY (tenant_id,actor_principal_id)
    REFERENCES principals(tenant_id,principal_id) ON DELETE RESTRICT,
  UNIQUE (ai_id,tenant_id,binding_version,event_type)
);

INSERT INTO ai_identity_binding_events
  (binding_event_id,ai_id,tenant_id,principal_id,binding_version,event_type,reason)
SELECT gen_random_uuid(),ai_id,tenant_id,principal_id,binding_version,'CREATED','P2.33 baseline import'
FROM ai_identity_principals;

CREATE FUNCTION record_ai_identity_binding_update() RETURNS TRIGGER AS $$
BEGIN
  IF NEW.ai_id<>OLD.ai_id OR NEW.tenant_id<>OLD.tenant_id OR NEW.principal_id<>OLD.principal_id THEN
    RAISE EXCEPTION 'AI identity principal binding is immutable; revoke and provision a new identity' USING ERRCODE='23514';
  END IF;
  IF OLD.binding_status='REVOKED' AND (
    NEW.binding_status<>'REVOKED' OR NEW.revoked_at IS DISTINCT FROM OLD.revoked_at OR
    NEW.revoked_by_principal_id IS DISTINCT FROM OLD.revoked_by_principal_id OR
    NEW.revocation_reason IS DISTINCT FROM OLD.revocation_reason OR NEW.binding_version<>OLD.binding_version
  ) THEN
    RAISE EXCEPTION 'revoked AI identity principal binding is immutable' USING ERRCODE='23514';
  END IF;
  IF OLD.binding_status='ACTIVE' AND NEW.binding_status='REVOKED' THEN
    IF NEW.revoked_at IS NULL OR NEW.revocation_reason IS NULL OR length(trim(NEW.revocation_reason))=0 THEN
      RAISE EXCEPTION 'binding revocation requires timestamp and reason' USING ERRCODE='23514';
    END IF;
    NEW.binding_version:=OLD.binding_version+1;
    INSERT INTO ai_identity_binding_events(binding_event_id,ai_id,tenant_id,principal_id,binding_version,event_type,actor_principal_id,reason)
    VALUES(gen_random_uuid(),OLD.ai_id,OLD.tenant_id,OLD.principal_id,NEW.binding_version,'REVOKED',NEW.revoked_by_principal_id,NEW.revocation_reason);
    UPDATE phone_pair_grants SET grant_status='REVOKED',revoked_at=NOW(),
      revoked_by_principal_id=NEW.revoked_by_principal_id,revocation_reason='identity binding revoked: '||NEW.revocation_reason
      WHERE tenant_id=OLD.tenant_id AND grant_status='ACTIVE'
        AND (source_principal_id=OLD.principal_id OR destination_principal_id=OLD.principal_id);
    UPDATE ai_phones SET phone_status='REVOKED',revoked_at=NOW(),
      revoked_by_principal_id=NEW.revoked_by_principal_id,revocation_reason='identity binding revoked: '||NEW.revocation_reason
      WHERE ai_id=OLD.ai_id AND tenant_id=OLD.tenant_id AND phone_status<>'REVOKED';
  END IF;
  RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER ai_identity_binding_update_guard
BEFORE UPDATE ON ai_identity_principals
FOR EACH ROW EXECUTE FUNCTION record_ai_identity_binding_update();

CREATE FUNCTION record_ai_identity_binding_insert() RETURNS TRIGGER AS $$
BEGIN
  INSERT INTO ai_identity_binding_events(binding_event_id,ai_id,tenant_id,principal_id,binding_version,event_type,reason)
  VALUES(gen_random_uuid(),NEW.ai_id,NEW.tenant_id,NEW.principal_id,NEW.binding_version,'CREATED','Provisioned by trusted control plane');
  RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER ai_identity_binding_insert_audit
AFTER INSERT ON ai_identity_principals
FOR EACH ROW EXECUTE FUNCTION record_ai_identity_binding_insert();

CREATE FUNCTION enforce_active_ai_identity_binding() RETURNS TRIGGER AS $
DECLARE identity_state TEXT;
BEGIN
  SELECT identity_status INTO identity_state FROM ai_identities WHERE ai_id=NEW.ai_id;
  IF identity_state IS DISTINCT FROM 'ACTIVE' THEN
    RAISE EXCEPTION 'retired or missing AI identity cannot receive a tenant principal binding' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $ LANGUAGE plpgsql;

CREATE TRIGGER ai_identity_binding_requires_active_ai
BEFORE INSERT OR UPDATE ON ai_identity_principals
FOR EACH ROW EXECUTE FUNCTION enforce_active_ai_identity_binding();

CREATE FUNCTION revoke_ai_binding_when_principal_disabled() RETURNS TRIGGER AS $
BEGIN
  IF OLD.enabled=TRUE AND NEW.enabled=FALSE THEN
    UPDATE ai_identity_principals
       SET binding_status='REVOKED',revoked_at=NOW(),revocation_reason='principal disabled'
     WHERE tenant_id=NEW.tenant_id AND principal_id=NEW.principal_id AND binding_status='ACTIVE';
  END IF;
  RETURN NEW;
END $ LANGUAGE plpgsql;

CREATE TRIGGER principal_disable_revokes_ai_phone
AFTER UPDATE OF enabled ON principals
FOR EACH ROW EXECUTE FUNCTION revoke_ai_binding_when_principal_disabled();

CREATE FUNCTION enforce_ai_identity_retirement() RETURNS TRIGGER AS $$
BEGIN
  IF NEW.ai_id<>OLD.ai_id OR NEW.identity_key<>OLD.identity_key OR NEW.identity_kind<>OLD.identity_kind THEN
    RAISE EXCEPTION 'global AI identity keys and kind are immutable' USING ERRCODE='23514';
  END IF;
  IF OLD.identity_status='RETIRED' AND (
    NEW.identity_status<>'RETIRED' OR NEW.retired_at IS DISTINCT FROM OLD.retired_at OR
    NEW.retired_by_principal_id IS DISTINCT FROM OLD.retired_by_principal_id OR
    NEW.retirement_reason IS DISTINCT FROM OLD.retirement_reason
  ) THEN
    RAISE EXCEPTION 'retired AI identity lifecycle record is immutable' USING ERRCODE='23514';
  END IF;
  IF NEW.identity_status='RETIRED' AND OLD.identity_status<>'RETIRED' THEN
    IF EXISTS (SELECT 1 FROM ai_identity_principals WHERE ai_id=OLD.ai_id AND binding_status='ACTIVE') OR EXISTS (SELECT 1 FROM ai_phones WHERE ai_id=OLD.ai_id AND phone_status<>'REVOKED') THEN
      RAISE EXCEPTION 'revoke every tenant binding and AI phone before retiring the global AI identity' USING ERRCODE='23514';
    END IF;
    IF NEW.retired_at IS NULL OR NEW.retirement_reason IS NULL OR length(trim(NEW.retirement_reason))=0 THEN
      RAISE EXCEPTION 'AI retirement requires timestamp and reason' USING ERRCODE='23514';
    END IF;
  END IF;
  RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER ai_identity_retirement_guard
BEFORE UPDATE ON ai_identities
FOR EACH ROW EXECUTE FUNCTION enforce_ai_identity_retirement();

CREATE FUNCTION enforce_ai_phone_revocation() RETURNS TRIGGER AS $$
BEGIN
  IF NEW.ai_phone_id<>OLD.ai_phone_id OR NEW.ai_id<>OLD.ai_id OR NEW.tenant_id<>OLD.tenant_id THEN
    RAISE EXCEPTION 'AI_PHONE_ID ownership tuple is immutable' USING ERRCODE='23514';
  END IF;
  IF OLD.phone_status='REVOKED' AND (
    NEW.phone_status<>'REVOKED' OR NEW.revoked_at IS DISTINCT FROM OLD.revoked_at OR
    NEW.revoked_by_principal_id IS DISTINCT FROM OLD.revoked_by_principal_id OR
    NEW.revocation_reason IS DISTINCT FROM OLD.revocation_reason
  ) THEN
    RAISE EXCEPTION 'revoked AI_PHONE_ID lifecycle record is immutable' USING ERRCODE='23514';
  END IF;
  IF NEW.phone_status='REVOKED' AND OLD.phone_status<>'REVOKED' THEN
    IF NEW.revoked_at IS NULL OR NEW.revocation_reason IS NULL OR length(trim(NEW.revocation_reason))=0 THEN
      RAISE EXCEPTION 'phone revocation requires timestamp and reason' USING ERRCODE='23514';
    END IF;
    NEW.endpoint_fence:=OLD.endpoint_fence+1;
    NEW.version:=OLD.version+1;
    NEW.updated_at:=NOW();
    UPDATE phone_attachments SET attachment_status='FENCED',detached_at=NOW()
      WHERE ai_phone_id=OLD.ai_phone_id AND tenant_id=OLD.tenant_id AND attachment_status='ACTIVE';
    UPDATE phone_endpoints SET endpoint_status='REVOKED',detached_at=NOW()
      WHERE ai_phone_id=OLD.ai_phone_id AND endpoint_status<>'REVOKED';
    UPDATE sessions SET status='REVOKED',revoked_at=NOW()
      WHERE session_id IN (SELECT session_id FROM phone_attachments WHERE ai_phone_id=OLD.ai_phone_id AND tenant_id=OLD.tenant_id);
    UPDATE workload_executions SET status='REVOKED',revoked_at=NOW()
      WHERE execution_id IN (SELECT execution_id FROM phone_attachments WHERE ai_phone_id=OLD.ai_phone_id AND tenant_id=OLD.tenant_id);
  END IF;
  RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER ai_phone_revocation_guard
BEFORE UPDATE ON ai_phones
FOR EACH ROW EXECUTE FUNCTION enforce_ai_phone_revocation();

CREATE TABLE ai_phone_lifecycle_events (
  event_id UUID PRIMARY KEY,
  ai_phone_id UUID NOT NULL,
  tenant_id UUID NOT NULL,
  ai_id UUID NOT NULL,
  event_type TEXT NOT NULL CHECK (event_type IN ('CREATED','REVOKED')),
  actor_principal_id UUID,
  reason TEXT NOT NULL CHECK (length(trim(reason)) BETWEEN 1 AND 1000),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (ai_phone_id,tenant_id,ai_id)
    REFERENCES ai_phones(ai_phone_id,tenant_id,ai_id) ON DELETE RESTRICT,
  FOREIGN KEY (tenant_id,actor_principal_id)
    REFERENCES principals(tenant_id,principal_id) ON DELETE RESTRICT
);
INSERT INTO ai_phone_lifecycle_events(event_id,ai_phone_id,tenant_id,ai_id,event_type,reason)
SELECT gen_random_uuid(),ai_phone_id,tenant_id,ai_id,'CREATED','P2.33 baseline import' FROM ai_phones;

CREATE FUNCTION record_ai_phone_lifecycle_event() RETURNS TRIGGER AS $
BEGIN
  IF TG_OP='INSERT' THEN
    INSERT INTO ai_phone_lifecycle_events(event_id,ai_phone_id,tenant_id,ai_id,event_type,reason)
    VALUES(gen_random_uuid(),NEW.ai_phone_id,NEW.tenant_id,NEW.ai_id,'CREATED','Provisioned by trusted control plane');
  ELSIF OLD.phone_status<>'REVOKED' AND NEW.phone_status='REVOKED' THEN
    INSERT INTO ai_phone_lifecycle_events(event_id,ai_phone_id,tenant_id,ai_id,event_type,actor_principal_id,reason)
    VALUES(gen_random_uuid(),OLD.ai_phone_id,OLD.tenant_id,OLD.ai_id,'REVOKED',NEW.revoked_by_principal_id,NEW.revocation_reason);
  END IF;
  RETURN NEW;
END $ LANGUAGE plpgsql;

CREATE TRIGGER ai_phone_lifecycle_insert_audit
AFTER INSERT ON ai_phones
FOR EACH ROW EXECUTE FUNCTION record_ai_phone_lifecycle_event();

CREATE TRIGGER ai_phone_lifecycle_revoke_audit
AFTER UPDATE OF phone_status ON ai_phones
FOR EACH ROW EXECUTE FUNCTION record_ai_phone_lifecycle_event();

CREATE TRIGGER ai_phone_lifecycle_events_immutable
BEFORE UPDATE OR DELETE ON ai_phone_lifecycle_events
FOR EACH ROW EXECUTE FUNCTION reject_phone_identity_audit_mutation();

CREATE TABLE phone_pair_grants (
  grant_id UUID PRIMARY KEY,
  tenant_id UUID NOT NULL,
  room_id UUID NOT NULL,
  source_ai_phone_id UUID NOT NULL,
  destination_ai_phone_id UUID NOT NULL,
  source_principal_id UUID NOT NULL,
  destination_principal_id UUID NOT NULL,
  permission TEXT NOT NULL CHECK (permission='phone.message.send'),
  grant_status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (grant_status IN ('ACTIVE','REVOKED','EXPIRED')),
  grant_version BIGINT NOT NULL DEFAULT 1 CHECK (grant_version>0),
  source_binding_version BIGINT NOT NULL CHECK (source_binding_version>0),
  destination_binding_version BIGINT NOT NULL CHECK (destination_binding_version>0),
  valid_from TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  expires_at TIMESTAMPTZ,
  granted_by_principal_id UUID NOT NULL,
  granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  revoked_by_principal_id UUID,
  revoked_at TIMESTAMPTZ,
  revocation_reason TEXT,
  FOREIGN KEY (tenant_id,room_id) REFERENCES rooms(tenant_id,room_id) ON DELETE RESTRICT,
  FOREIGN KEY (source_ai_phone_id,tenant_id) REFERENCES ai_phones(ai_phone_id,tenant_id) ON DELETE RESTRICT,
  FOREIGN KEY (destination_ai_phone_id,tenant_id) REFERENCES ai_phones(ai_phone_id,tenant_id) ON DELETE RESTRICT,
  FOREIGN KEY (tenant_id,source_principal_id) REFERENCES principals(tenant_id,principal_id) ON DELETE RESTRICT,
  FOREIGN KEY (tenant_id,destination_principal_id) REFERENCES principals(tenant_id,principal_id) ON DELETE RESTRICT,
  FOREIGN KEY (tenant_id,granted_by_principal_id) REFERENCES principals(tenant_id,principal_id) ON DELETE RESTRICT,
  FOREIGN KEY (tenant_id,revoked_by_principal_id) REFERENCES principals(tenant_id,principal_id) ON DELETE RESTRICT,
  UNIQUE (grant_id,tenant_id,room_id,source_ai_phone_id,destination_ai_phone_id,source_principal_id,destination_principal_id,grant_version,source_binding_version,destination_binding_version),
  CHECK (source_ai_phone_id<>destination_ai_phone_id),
  CHECK (expires_at IS NULL OR expires_at>valid_from),
  CHECK ((grant_status='REVOKED')=(revoked_at IS NOT NULL))
);
CREATE UNIQUE INDEX phone_pair_one_active_grant_idx
  ON phone_pair_grants(tenant_id,room_id,source_ai_phone_id,destination_ai_phone_id,permission)
  WHERE grant_status='ACTIVE';
CREATE INDEX phone_pair_grants_lookup_idx
  ON phone_pair_grants(tenant_id,room_id,source_ai_phone_id,destination_ai_phone_id,grant_status);

CREATE FUNCTION enforce_phone_pair_grant_binding() RETURNS TRIGGER AS $
BEGIN
  IF TG_OP='INSERT' AND NOT EXISTS (
    SELECT 1 FROM principals p JOIN principal_permissions pp
      ON pp.tenant_id=p.tenant_id AND pp.principal_id=p.principal_id
    WHERE p.tenant_id=NEW.tenant_id AND p.principal_id=NEW.granted_by_principal_id AND p.enabled=TRUE
      AND pp.permission='phone.pair.grant' AND pp.revoked_at IS NULL
      AND (pp.room_id IS NULL OR pp.room_id=NEW.room_id)
  ) THEN
    RAISE EXCEPTION 'pair grant author lacks phone.pair.grant permission' USING ERRCODE='42501';
  END IF;
  IF TG_OP='UPDATE' AND OLD.grant_status='ACTIVE' AND NEW.grant_status='REVOKED' AND NOT EXISTS (
    SELECT 1 FROM principals p JOIN principal_permissions pp
      ON pp.tenant_id=p.tenant_id AND pp.principal_id=p.principal_id
    WHERE p.tenant_id=NEW.tenant_id AND p.principal_id=NEW.revoked_by_principal_id AND p.enabled=TRUE
      AND pp.permission='phone.pair.grant' AND pp.revoked_at IS NULL
      AND (pp.room_id IS NULL OR pp.room_id=NEW.room_id)
  ) THEN
    RAISE EXCEPTION 'pair grant revoker lacks phone.pair.grant permission' USING ERRCODE='42501';
  END IF;
  IF TG_OP='UPDATE' AND (
    NEW.tenant_id<>OLD.tenant_id OR NEW.room_id<>OLD.room_id OR
    NEW.source_ai_phone_id<>OLD.source_ai_phone_id OR NEW.destination_ai_phone_id<>OLD.destination_ai_phone_id OR
    NEW.source_principal_id<>OLD.source_principal_id OR NEW.destination_principal_id<>OLD.destination_principal_id OR
    NEW.permission<>OLD.permission OR NEW.grant_version<>OLD.grant_version OR
    NEW.source_binding_version<>OLD.source_binding_version OR NEW.destination_binding_version<>OLD.destination_binding_version OR
    NEW.valid_from<>OLD.valid_from OR NEW.expires_at IS DISTINCT FROM OLD.expires_at OR
    NEW.granted_by_principal_id<>OLD.granted_by_principal_id OR NEW.granted_at<>OLD.granted_at
  ) THEN
    RAISE EXCEPTION 'phone pair grant identity is immutable' USING ERRCODE='23514';
  END IF;
  IF TG_OP='UPDATE' AND OLD.grant_status<>'ACTIVE' AND NEW.grant_status='ACTIVE' THEN
    RAISE EXCEPTION 'revoked or expired pair grants cannot be reactivated' USING ERRCODE='23514';
  END IF;
  IF TG_OP='UPDATE' AND OLD.grant_status='REVOKED' AND (
    NEW.revoked_at IS DISTINCT FROM OLD.revoked_at OR
    NEW.revoked_by_principal_id IS DISTINCT FROM OLD.revoked_by_principal_id OR
    NEW.revocation_reason IS DISTINCT FROM OLD.revocation_reason
  ) THEN
    RAISE EXCEPTION 'revoked pair grant audit data is immutable' USING ERRCODE='23514';
  END IF;
  IF TG_OP='UPDATE' AND OLD.grant_status='ACTIVE' AND NEW.grant_status='REVOKED' THEN
    IF NEW.revoked_at IS NULL OR NEW.revocation_reason IS NULL OR length(trim(NEW.revocation_reason))=0 THEN
      RAISE EXCEPTION 'grant revocation requires timestamp and reason' USING ERRCODE='23514';
    END IF;
  END IF;
  IF NEW.grant_status='ACTIVE' AND NOT EXISTS (
    SELECT 1 FROM ai_phones s
    JOIN ai_identity_principals sip ON sip.ai_id=s.ai_id AND sip.tenant_id=s.tenant_id AND sip.principal_id=NEW.source_principal_id AND sip.binding_status='ACTIVE'
    JOIN ai_phones d ON d.ai_phone_id=NEW.destination_ai_phone_id AND d.tenant_id=NEW.tenant_id
    JOIN ai_identity_principals dip ON dip.ai_id=d.ai_id AND dip.tenant_id=d.tenant_id AND dip.principal_id=NEW.destination_principal_id AND dip.binding_status='ACTIVE'
    JOIN rooms r ON r.room_id=NEW.room_id AND r.tenant_id=NEW.tenant_id AND r.status='ACTIVE'
    JOIN room_memberships sm ON sm.room_id=r.room_id AND sm.tenant_id=r.tenant_id AND sm.principal_id=NEW.source_principal_id AND sm.status='ACTIVE'
    JOIN sessions ss ON ss.session_id=sm.session_id AND ss.status='ACTIVE' AND ss.revoked_at IS NULL AND ss.expires_at>NOW()
    JOIN room_memberships dm ON dm.room_id=r.room_id AND dm.tenant_id=r.tenant_id AND dm.principal_id=NEW.destination_principal_id AND dm.status='ACTIVE'
    JOIN sessions ds ON ds.session_id=dm.session_id AND ds.status='ACTIVE' AND ds.revoked_at IS NULL AND ds.expires_at>NOW()
    WHERE s.ai_phone_id=NEW.source_ai_phone_id AND s.tenant_id=NEW.tenant_id AND s.phone_status<>'REVOKED'
      AND d.phone_status<>'REVOKED' AND sip.binding_version=NEW.source_binding_version AND dip.binding_version=NEW.destination_binding_version
  ) THEN
    RAISE EXCEPTION 'pair grant principals must be the active principals bound to both phones in the same tenant and room' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER phone_pair_grant_binding_guard
BEFORE INSERT OR UPDATE ON phone_pair_grants
FOR EACH ROW EXECUTE FUNCTION enforce_phone_pair_grant_binding();

CREATE TABLE phone_pair_grant_events (
  event_id UUID PRIMARY KEY,
  grant_id UUID NOT NULL REFERENCES phone_pair_grants(grant_id) ON DELETE RESTRICT,
  tenant_id UUID NOT NULL,
  actor_principal_id UUID,
  event_type TEXT NOT NULL CHECK (event_type IN ('GRANTED','REVOKED')),
  reason TEXT NOT NULL CHECK (length(trim(reason)) BETWEEN 1 AND 1000),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (tenant_id,actor_principal_id) REFERENCES principals(tenant_id,principal_id) ON DELETE RESTRICT
);

CREATE FUNCTION record_phone_pair_grant_event() RETURNS TRIGGER AS $
BEGIN
  IF TG_OP='INSERT' THEN
    INSERT INTO phone_pair_grant_events(event_id,grant_id,tenant_id,actor_principal_id,event_type,reason)
    VALUES(gen_random_uuid(),NEW.grant_id,NEW.tenant_id,NEW.granted_by_principal_id,'GRANTED','Explicit directional phone grant');
    RETURN NEW;
  END IF;
  IF OLD.grant_status='ACTIVE' AND NEW.grant_status='REVOKED' THEN
    INSERT INTO phone_pair_grant_events(event_id,grant_id,tenant_id,actor_principal_id,event_type,reason)
    VALUES(gen_random_uuid(),OLD.grant_id,OLD.tenant_id,NEW.revoked_by_principal_id,'REVOKED',NEW.revocation_reason);
  END IF;
  RETURN NEW;
END $ LANGUAGE plpgsql;

CREATE TRIGGER phone_pair_grant_audit_insert
AFTER INSERT ON phone_pair_grants
FOR EACH ROW EXECUTE FUNCTION record_phone_pair_grant_event();

CREATE TRIGGER phone_pair_grant_audit_revoke
AFTER UPDATE OF grant_status ON phone_pair_grants
FOR EACH ROW EXECUTE FUNCTION record_phone_pair_grant_event();

CREATE FUNCTION reject_phone_identity_audit_mutation() RETURNS TRIGGER AS $
BEGIN
  RAISE EXCEPTION 'identity and grant audit history is append-only' USING ERRCODE='55000';
END $ LANGUAGE plpgsql;

CREATE TRIGGER ai_identity_binding_events_immutable
BEFORE UPDATE OR DELETE ON ai_identity_binding_events
FOR EACH ROW EXECUTE FUNCTION reject_phone_identity_audit_mutation();

CREATE TRIGGER phone_pair_grant_events_immutable
BEFORE UPDATE OR DELETE ON phone_pair_grant_events
FOR EACH ROW EXECUTE FUNCTION reject_phone_identity_audit_mutation();

INSERT INTO permission_catalog(permission,description,privileged) VALUES
 ('phone.pair.grant','Create or revoke a directional phone-to-phone messaging grant in the authenticated room',TRUE)
ON CONFLICT (permission) DO NOTHING;

ALTER TABLE phone_messages
  ADD COLUMN source_ai_phone_id UUID,
  ADD COLUMN destination_principal_id UUID,
  ADD COLUMN room_id UUID,
  ADD COLUMN pair_grant_id UUID,
  ADD COLUMN pair_grant_version BIGINT,
  ADD COLUMN source_binding_version BIGINT,
  ADD COLUMN destination_binding_version BIGINT;

ALTER TABLE phone_messages
  ADD CONSTRAINT phone_messages_source_phone_scope_fk
    FOREIGN KEY (source_ai_phone_id,tenant_id) REFERENCES ai_phones(ai_phone_id,tenant_id) ON DELETE RESTRICT,
  ADD CONSTRAINT phone_messages_room_scope_fk
    FOREIGN KEY (tenant_id,room_id) REFERENCES rooms(tenant_id,room_id) ON DELETE RESTRICT,
  ADD CONSTRAINT phone_messages_pair_fields_all_or_none CHECK (
    (pair_grant_id IS NULL AND source_ai_phone_id IS NULL AND destination_principal_id IS NULL AND room_id IS NULL
      AND pair_grant_version IS NULL AND source_binding_version IS NULL AND destination_binding_version IS NULL)
    OR
    (pair_grant_id IS NOT NULL AND source_ai_phone_id IS NOT NULL AND destination_principal_id IS NOT NULL AND room_id IS NOT NULL
      AND pair_grant_version IS NOT NULL AND source_binding_version IS NOT NULL AND destination_binding_version IS NOT NULL)
  ),
  ADD CONSTRAINT phone_messages_pair_grant_scope_fk
    FOREIGN KEY (pair_grant_id,tenant_id,room_id,source_ai_phone_id,ai_phone_id,sender_principal_id,destination_principal_id,pair_grant_version,source_binding_version,destination_binding_version)
    REFERENCES phone_pair_grants(grant_id,tenant_id,room_id,source_ai_phone_id,destination_ai_phone_id,source_principal_id,destination_principal_id,grant_version,source_binding_version,destination_binding_version)
    ON DELETE RESTRICT;

CREATE INDEX phone_messages_pair_grant_idx ON phone_messages(pair_grant_id,message_state);

CREATE OR REPLACE FUNCTION enforce_phone_message_transition() RETURNS TRIGGER AS $
BEGIN
  IF NEW.ai_phone_id<>OLD.ai_phone_id OR NEW.tenant_id<>OLD.tenant_id OR
     NEW.sender_principal_id<>OLD.sender_principal_id OR NEW.recipient_ai_id<>OLD.recipient_ai_id OR
     NEW.sequence_no<>OLD.sequence_no OR NEW.idempotency_key<>OLD.idempotency_key OR
     NEW.content<>OLD.content OR
     NEW.source_ai_phone_id IS DISTINCT FROM OLD.source_ai_phone_id OR
     NEW.destination_principal_id IS DISTINCT FROM OLD.destination_principal_id OR
     NEW.room_id IS DISTINCT FROM OLD.room_id OR NEW.pair_grant_id IS DISTINCT FROM OLD.pair_grant_id OR
     NEW.pair_grant_version IS DISTINCT FROM OLD.pair_grant_version OR
     NEW.source_binding_version IS DISTINCT FROM OLD.source_binding_version OR
     NEW.destination_binding_version IS DISTINCT FROM OLD.destination_binding_version THEN
    RAISE EXCEPTION 'phone message identity, routing and content are immutable' USING ERRCODE='23514';
  END IF;
  IF NOT (
    NEW.message_state=OLD.message_state OR
    (OLD.message_state='PENDING' AND NEW.message_state IN ('DELIVERED','FAILED')) OR
    (OLD.message_state='DELIVERED' AND NEW.message_state IN ('ACK','FAILED','PENDING'))
  ) THEN
    RAISE EXCEPTION 'invalid phone message state transition' USING ERRCODE='23514';
  END IF;
  IF NEW.attempt_count<OLD.attempt_count THEN
    RAISE EXCEPTION 'phone message attempts cannot decrease' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $ LANGUAGE plpgsql;


CREATE FUNCTION fail_pending_messages_on_pair_grant_revocation() RETURNS TRIGGER AS $$
BEGIN
  IF OLD.grant_status='ACTIVE' AND NEW.grant_status IN ('REVOKED','EXPIRED') THEN
    UPDATE phone_messages SET message_state='FAILED',updated_at=NOW()
      WHERE pair_grant_id=OLD.grant_id AND message_state IN ('PENDING','DELIVERED');
  END IF;
  RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER phone_pair_grant_message_revoke
AFTER UPDATE OF grant_status ON phone_pair_grants
FOR EACH ROW EXECUTE FUNCTION fail_pending_messages_on_pair_grant_revocation();

COMMENT ON TABLE phone_pair_grants IS 'Directional, tenant- and room-scoped authorization between permanent AI phones. No grant means deny.';
COMMENT ON COLUMN phone_messages.source_ai_phone_id IS 'Server-derived sender phone; never accepted from client payload.';
COMMENT ON COLUMN phone_messages.pair_grant_id IS 'Exact directional grant used at send time; revocation fails undelivered messages.';
COMMENT ON COLUMN phone_pair_grants.source_binding_version IS 'Version of the authenticated source phone/principal binding at grant creation.';
COMMENT ON COLUMN phone_pair_grants.destination_binding_version IS 'Version of the authenticated destination phone/principal binding at grant creation.';
