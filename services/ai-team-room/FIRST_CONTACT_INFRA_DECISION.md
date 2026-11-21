# First-Contact Infrastructure Decision

## Decision

Use Oracle Cloud Infrastructure (OCI) Always Free as the preferred hosting target for the first-contact Team Room, subject to account eligibility and available capacity.

## Verified constraints

- OCI Always Free resources do not expire.
- Ampere A1 Always Free capacity is equivalent to up to 2 OCPUs and 12 GB RAM in an Always Free tenancy.
- Compute must be provisioned in the tenancy's home region for Always Free eligibility.
- OCI may report `out of host capacity`; this is a capacity condition, not a software failure.
- A public IPv4 address is available for a compute instance when required by the chosen network design.
- OCI Bastion is an Always Free option for restricted SSH access.
- The separate US$300 / 30-day trial is not the basis of this server plan; the target is Always Free resources.

## Architecture fit

The proposed single VM can run the prepared Docker Compose stack:

Caddy -> Team Room -> PostgreSQL + Keycloak

The application services remain isolated on the Docker private network. Only the public HTTPS ingress is exposed.

## Tablet constraint

The user does not have a desktop/Windows computer. Infrastructure administration must therefore be performed through the browser/cloud console or another remote mechanism available from the tablet. The server itself, not the tablet, runs Linux/Docker.

## Cost boundary

Do not create paid resources intentionally. Before provisioning, verify that the selected compute shape and storage are marked Always Free eligible in the OCI Console. Do not rely on the 30-day promotional credit for long-term operation.

## Current status

This is an infrastructure decision and readiness record. No OCI VM has been created by this commit, and no billing status is claimed.
