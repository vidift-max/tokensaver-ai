# First-Contact OCI Console Checklist

## Purpose

Provision only the minimum OCI resources needed for the Team Room first-contact server, while avoiding accidental paid resources.

## Before creating anything

1. Sign in to OCI Console from the Android tablet.
2. Open the tenancy's home region.
3. Confirm the account is not relying on the US$300 trial for this deployment.
4. Confirm the selected compute shape is explicitly marked **Always Free eligible**.
5. Stop if the Console does not show Always Free eligibility.

## Compute target

Preferred initial target:

- Shape: Ampere A1 Flex
- OCPUs: 2
- Memory: 12 GB
- Boot volume: use the smallest practical Always Free-eligible allocation
- OS: Ubuntu LTS ARM64
- Public IPv4: enabled only as required for the HTTPS ingress
- SSH key: user-controlled key; never commit the private key to Git

The exact availability of the shape is determined by OCI capacity and the user's tenancy.

## Network target

Public:

- TCP 80 -> Caddy
- TCP 443 -> Caddy

Private:

- PostgreSQL 5432
- Keycloak 8080
- Team Room 8081

Do not expose PostgreSQL, Keycloak, or Team Room directly to the Internet.

## After VM creation

Do not deploy the application immediately.

First capture:

- VM OCID
- availability domain
- region
- shape
- OCPU/RAM allocation
- public IPv4
- Always Free eligibility shown by OCI

Then verify the network security list/security group permits only the intended public ports.

## Cost safety gate

If OCI shows a paid/non-Always-Free resource, unexpected estimated charges, or an unavailable Always Free allocation:

**STOP. Do not create the resource.**

## Evidence boundary

Creating the VM proves infrastructure availability only. It does not prove:

- Team Room is running;
- OIDC is working;
- Phone-01 is authenticated;
- Phone-02 is authenticated;
- Phone-02 physical identity;
- AI-to-AI communication.

Those require separate runtime evidence.
