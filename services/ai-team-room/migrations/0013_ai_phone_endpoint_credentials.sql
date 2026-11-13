-- AI-PHONE-02B endpoint-scoped authentication.
-- Operator key hashes are provisioned out-of-band by a trusted control plane.
-- Raw operator keys and endpoint credentials are never persisted.
CREATE TABLE phone_endpoint_operator_keys (
    operator_key_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    ai_id UUID NOT NULL,
    key_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(key_hash)=32),
    key_status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (key_status IN ('ACTIVE','REVOKED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    FOREIGN KEY (ai_id,tenant_id) REFERENCES ai_identity_tenants(ai_id,tenant_id) ON DELETE RESTRICT,
    CHECK ((key_status='REVOKED') = (revoked_at IS NOT NULL))
);
CREATE INDEX phone_endpoint_operator_keys_scope_idx
    ON phone_endpoint_operator_keys(tenant_id,ai_id,key_status);

CREATE TABLE phone_endpoint_credentials (
    credential_id UUID PRIMARY KEY,
    endpoint_id UUID NOT NULL,
    ai_phone_id UUID NOT NULL,
    tenant_id UUID NOT NULL,
    ai_id UUID NOT NULL,
    credential_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(credential_hash)=32),
    credential_status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (credential_status IN ('ACTIVE','REVOKED')),
    endpoint_fence BIGINT CHECK (endpoint_fence IS NULL OR endpoint_fence>0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    FOREIGN KEY (endpoint_id,ai_phone_id,tenant_id,ai_id)
        REFERENCES phone_endpoints(endpoint_id,ai_phone_id,tenant_id,ai_id) ON DELETE RESTRICT,
    FOREIGN KEY (ai_phone_id,tenant_id,ai_id)
        REFERENCES ai_phones(ai_phone_id,tenant_id,ai_id) ON DELETE RESTRICT,
    CHECK ((credential_status='REVOKED') = (revoked_at IS NOT NULL))
);
CREATE UNIQUE INDEX phone_endpoint_credentials_one_active_idx
    ON phone_endpoint_credentials(endpoint_id) WHERE credential_status='ACTIVE';
CREATE INDEX phone_endpoint_credentials_scope_idx
    ON phone_endpoint_credentials(tenant_id,ai_id,ai_phone_id,endpoint_id,credential_status);

CREATE OR REPLACE FUNCTION revoke_phone_endpoint_credentials_on_lifecycle()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.endpoint_status IN ('DETACHED','REVOKED')
       AND OLD.endpoint_status IS DISTINCT FROM NEW.endpoint_status THEN
        UPDATE phone_endpoint_credentials
           SET credential_status='REVOKED',revoked_at=COALESCE(revoked_at,NOW())
         WHERE endpoint_id=NEW.endpoint_id AND credential_status='ACTIVE';
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER phone_endpoint_credentials_lifecycle_revoke
AFTER UPDATE OF endpoint_status ON phone_endpoints
FOR EACH ROW EXECUTE FUNCTION revoke_phone_endpoint_credentials_on_lifecycle();

INSERT INTO permission_catalog(permission,description,privileged) VALUES
 ('phone.endpoint.manage','Register and rotate endpoint identities using an independently provisioned operator key',TRUE)
ON CONFLICT (permission) DO NOTHING;

COMMENT ON TABLE phone_endpoint_operator_keys IS
 'Hash-only out-of-band operator credentials. AI bearer possession alone cannot enroll or rotate endpoint identities.';
COMMENT ON TABLE phone_endpoint_credentials IS
 'Hash-only endpoint credentials bound to one endpoint, AI phone, AI identity, tenant and active fencing generation.';
COMMENT ON COLUMN phone_endpoint_credentials.endpoint_fence IS
 'Generation bound at attach/transfer; old credential is revoked in the same transaction as lifecycle change.';
