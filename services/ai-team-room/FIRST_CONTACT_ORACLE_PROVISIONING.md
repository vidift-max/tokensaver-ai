# First-Contact Server — Provisioning Runbook

## Purpose

Provision the first-contact Team Room server on a free/eligible cloud VM without requiring a local Windows computer. The tablet is used only for browser-based administration and GitHub changes.

## Target architecture

Internet -> HTTPS :443 -> Caddy -> Team Room :8081
                                      |-> PostgreSQL :5432 (private)
                                      |-> Keycloak :8080 (private)

Only Caddy exposes ports 80/443. PostgreSQL and Keycloak must not have public port mappings.

## Provisioning sequence

1. Create or select an eligible Always Free VM in the chosen cloud account.
2. Assign a stable public IP or equivalent stable address.
3. Create DNS A/AAAA records for TEAM_ROOM_DOMAIN pointing to that address.
4. Install Docker Engine and Compose on the VM using the provider's browser/cloud-shell or other remote administration facility available to the account.
5. Clone the approved repository branch `mission/phone-02-first-contact-server`.
6. Create the deployment `.env` from `.env.first-contact.example` on the VM only.
7. Generate unique PostgreSQL and Keycloak bootstrap passwords.
8. Set TEAM_ROOM_DOMAIN and the matching OIDC issuer/JWKS values.
9. Start the stack with Docker Compose.
10. Verify Caddy obtains TLS and that `/health/live` is reachable through HTTPS.
11. Verify `/health/ready` only after PostgreSQL and the Team Room service are healthy.
12. Configure Keycloak realm/client/issuer according to the existing Team Room authentication contract.
13. Register Phone-01 and Phone-02 identities and sessions only after the server is healthy.

## Tablet-only boundary

This runbook does not require the user to own a Windows PC or desktop. It does require a cloud VM that can run the Linux/Docker stack. If the selected provider does not expose a browser/cloud-shell path suitable for the tablet, use another supported remote administration method rather than pretending the tablet can execute Docker locally.

## Acceptance evidence

Record the exact VM/deployment revision, public hostname, health responses, service logs, and authentication evidence. Do not treat a successful browser page, DNS record, or TLS certificate alone as proof of AI-to-AI communication.

## Security boundary

Do not expose PostgreSQL or Keycloak directly to the Internet. Do not commit `.env`, passwords, tokens, certificates, or private keys. Do not activate Phone-02 merely because the server is online.

## Current status

This document is a provisioning plan only. No cloud VM, DNS record, TLS certificate, or live deployment is claimed by this file.
