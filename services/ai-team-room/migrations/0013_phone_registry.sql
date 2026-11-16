-- Tenant-scoped read access to the operational AI Phone registry.
-- Tenant ownership is always derived from the authenticated session, never a query parameter.
INSERT INTO permission_catalog(permission, description, privileged) VALUES
 ('phone.registry.read', 'List AI phones and operational presence inside the authenticated tenant', TRUE)
ON CONFLICT (permission) DO NOTHING;
