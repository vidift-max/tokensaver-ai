-- AI-PHONE-02B directional Pair Authorization.
-- Grants are server-side and directional: sender AI_PHONE_ID -> recipient AI_PHONE_ID.
-- The authenticated caller never supplies a sender identity; it is derived from the session.
CREATE TABLE phone_pair_grants (
    grant_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    sender_ai_phone_id UUID NOT NULL,
    recipient_ai_phone_id UUID NOT NULL,
    created_by_principal_id UUID NOT NULL,
    grant_status TEXT NOT NULL DEFAULT 'ACTIVE'
        CHECK (grant_status IN ('ACTIVE','REVOKED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,
    FOREIGN KEY (sender_ai_phone_id,tenant_id)
        REFERENCES ai_phones(ai_phone_id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (recipient_ai_phone_id,tenant_id)
        REFERENCES ai_phones(ai_phone_id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,created_by_principal_id)
        REFERENCES principals(tenant_id,principal_id) ON DELETE RESTRICT,
    CHECK (sender_ai_phone_id <> recipient_ai_phone_id),
    CHECK ((grant_status='REVOKED') = (revoked_at IS NOT NULL))
);

CREATE UNIQUE INDEX phone_pair_grants_pair_idx
    ON phone_pair_grants(tenant_id,sender_ai_phone_id,recipient_ai_phone_id);

CREATE INDEX phone_pair_grants_recipient_idx
    ON phone_pair_grants(tenant_id,recipient_ai_phone_id,grant_status);

INSERT INTO permission_catalog(permission,description,privileged) VALUES
 ('phone.pair.grant','Authorize a directional AI Phone communication pair from the authenticated sender phone',TRUE),
 ('phone.pair.revoke','Revoke a directional AI Phone communication pair from the authenticated sender phone',TRUE)
ON CONFLICT (permission) DO NOTHING;

COMMENT ON TABLE phone_pair_grants IS
 'Server-derived directional Pair Authorization. A grant is required for message send, delivery, read and ACK; revocation immediately removes access to previously stored messages.';
COMMENT ON COLUMN phone_pair_grants.sender_ai_phone_id IS
 'Derived from the authenticated principal/session; never accepted as caller-selected identity.';
COMMENT ON COLUMN phone_pair_grants.recipient_ai_phone_id IS
 'Explicit recipient phone selected by the authorized sender-side control action.';
