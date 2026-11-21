# First-Contact VM Creation Evidence

## Purpose

Capture objective evidence immediately after the OCI VM is created.

## Required record

Record the values shown by OCI for:

- tenancy/home region
- compartment
- availability domain
- instance OCID
- instance name
- lifecycle state
- shape
- OCPUs
- memory
- boot volume size
- operating system/image
- public IPv4, if assigned
- Always Free eligibility

## Cost gate

Before continuing, confirm the selected compute and storage are marked Always Free eligible.

If the Console shows an unexpected paid resource or charge:

**STOP. Do not continue deployment.**

## Network evidence

Record the effective ingress rules and confirm only the intended public ports are exposed.

## What this evidence proves

It proves that the cloud VM exists with the recorded configuration.

It does not prove:

- Docker is installed;
- Team Room is running;
- PostgreSQL is healthy;
- Keycloak/OIDC is functional;
- Phone-01 is authenticated;
- Phone-02 is authenticated;
- Phone-02 physical identity;
- AI-to-AI communication.

Those are separate verification gates.
