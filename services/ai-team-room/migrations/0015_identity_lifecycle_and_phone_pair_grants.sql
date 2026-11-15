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
  IF OLD.binding_status='REVOKED' AND NEW.binding_status<>'REVOKED' THEN
    RAISE EXCEPTION 'revoked AI identity principal binding cannot be reactivated' USING ERRCODE='23514';
  END IF;
  IF OLD.binding_status='ACTIVE' AND NEW.binding_status='REVOKED' THEN
    IF NEW.revoked_at IS NULL OR NEW.revocation_reason IS NULL OR length(trim(NEW.revocation_reason))=0 THEN
      RAISE EXCEPTION 'binding revocation requires timestamp and reason' USING ERRCODE='23514';
    END IF;
    NEW.binding_version:=OLD.binding_version+1;
    INSERT INTO ai_identity_binding_events(binding_event_id,ai_id,tenant_id,principal_id,binding_version,event_type,actor_principal_id,reason)
    VALUES(gen_random_uuid(),OLD.ai_id,OLD.tenant_id,OLD.principal_id,NEW.binding_version,'REVOKED',NEW.revoked_by_principal_id,NEW.revocation_reason);
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

CREATE FUNCTION enforce_ai_identity_retirement() RETURNS TRIGGER AS $$
BEGIN
  IF NEW.ai_id<>OLD.ai_id OR NEW.identity_key<>OLD.identity_key OR NEW.identity_kind<>OLD.identity_kind THEN
    RAISE EXCEPTION 'global AI identity keys and kind are immutable' USING ERRCODE='23514';
  END IF;
  IF OLD.identity_status='RETIRED' AND NEW.identity_status<>'RETIRED' THEN
    RAISE EXCEPTION 'retired AI identity cannot be reactivated' USING ERRCODE='23514';
  END IF;
  IF NEW.identity_status='RETIRED' AND OLD.identity_status<>'RETIRED' THEN
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
  IF OLD.phone_status='REVOKED' AND NEW.phone_status<>'REVOKED' THEN
    RAISE EXCEPTION 'revoked AI_PHONE_ID cannot be reactivated' USING ERRCODE='23514';
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
  UNIQUE (grant_id,tenant_id,room_id,source_ai_phone_id,destination_ai_phone_id,source_principal_id,destination_principal_id,grant_version),
  CHECK (source_ai_phone_id<>destination_ai_phone_id),
  CHECK (expires_at IS NULL OR expires_at>valid_from),
  CHECK ((grant_status='REVOKED')=(revoked_at IS NOT NULL))
);
CREATE UNIQUE INDEX phone_pair_one_active_grant_idx
  ON phone_pair_grants(tenant_id,room_id,source_ai_phone_id,destination_ai_phone_id,permission)
  WHERE grant_status='ACTIVE';
CREATE INDEX phone_pair_grants_lookup_idx
  ON phone_pair_grants(tenant_id,room_id,source_ai_phone_id,destination_ai_phone_id,grant_status);

CREATE FUNCTION enforce_phone_pair_grant_binding() RETURNS TRIGGER AS $$
BEGIN
  IF TG_OP='UPDATE' AND (
    NEW.tenant_id<>OLD.tenant_id OR NEW.room_id<>OLD.room_id OR
    NEW.source_ai_phone_id<>OLD.source_ai_phone_id OR NEW.destination_ai_phone_id<>OLD.destination_ai_phone_id OR
    NEW.source_principal_id<>OLD.source_principal_id OR NEW.destination_principal_id<>OLD.destination_principal_id OR
    NEW.permission<>OLD.permission OR NEW.grant_version<>OLD.grant_version
  ) THEN
    RAISE EXCEPTION 'phone pair grant identity is immutable' USING ERRCODE='23514';
  END IF;
  IF TG_OP='UPDATE' AND OLD.grant_status<>'ACTIVE' AND NEW.grant_status='ACTIVE' THEN
    RAISE EXCEPTION 'revoked or expired pair grants cannot be reactivated' USING ERRCODE='23514';
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
    WHERE s.ai_phone_id=NEW.source_ai_phone_id AND s.tenant_id=NEW.tenant_id AND s.phone_status<>'REVOKED'
      AND d.phone_status<>'REVOKED' AND sip.binding_version>0 AND dip.binding_version>0
  ) THEN
    RAISE EXCEPTION 'pair grant principals must be the active principals bound to both phones in the same tenant and room' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER phone_pair_grant_binding_guard
BEFORE INSERT OR UPDATE ON phone_pair_grants
FOR EACH ROW EXECUTE FUNCTION enforce_phone_pair_grant_binding();

ALTER TABLE phone_messages
  ADD COLUMN source_ai_phone_id UUID,
  ADD COLUMN destination_principal_id UUID,
  ADD COLUMN room_id UUID,
  ADD COLUMN pair_grant_id UUID,
  ADD COLUMN pair_grant_version BIGINT;

ALTER TABLE phone_messages
  ADD CONSTRAINT phone_messages_source_phone_scope_fk
    FOREIGN KEY (source_ai_phone_id,tenant_id) REFERENCES ai_phones(ai_phone_id,tenant_id) ON DELETE RESTRICT,
  ADD CONSTRAINT phone_messages_room_scope_fk
    FOREIGN KEY (tenant_id,room_id) REFERENCES rooms(tenant_id,room_id) ON DELETE RESTRICT,
  ADD CONSTRAINT phone_messages_pair_grant_scope_fk
    FOREIGN KEY (pair_grant_id,tenant_id,room_id,source_ai_phone_id,ai_phone_id,sender_principal_id,destination_principal_id,pair_grant_version)
    REFERENCES phone_pair_grants(grant_id,tenant_id,room_id,source_ai_phone_id,destination_ai_phone_id,source_principal_id,destination_principal_id,grant_version)
    ON DELETE RESTRICT;

CREATE INDEX phone_messages_pair_grant_idx ON phone_messages(pair_grant_id,message_state);

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
