# First-Contact Secret Handling

This deployment must keep credentials outside Git.

## Required deployment secrets

- POSTGRES_PASSWORD
- KEYCLOAK_ADMIN_PASSWORD

Generate them on the deployment host with a cryptographically secure generator. Do not reuse repository credentials.

## Public configuration

The following values are configuration, not secrets:

- TEAM_ROOM_DOMAIN
- OIDC_ISSUER
- OIDC_AUDIENCE
- OIDC_JWKS_URL

The OIDC issuer and JWKS URL must resolve to the deployed Keycloak instance through the configured public domain.

## Rules

1. Never commit the real .env file.
2. Never put session tokens, workload credentials, private keys, or passwords in source files.
3. Do not paste production secrets into GitHub issues, pull requests, or chat logs.
4. Rotate bootstrap credentials after initial Keycloak administration is established.
5. Phone-01 and Phone-02 session tokens are runtime credentials and must be provisioned only on their respective clients.
6. A secret existing in configuration does not prove that a client successfully authenticated.
7. Authentication and first-contact acceptance require runtime evidence from the deployed service.

## Current status

This repository contains only a non-secret environment template. No production credential is claimed to exist, and no server deployment is claimed by this document.
