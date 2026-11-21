# First-Contact Administrative Access Plan

## Constraint

The infrastructure must be manageable from an Android tablet. No Windows desktop, local Docker Desktop, PowerShell, or local development environment is required for cloud provisioning.

## Preferred access path

1. OCI Console in the tablet browser.
2. OCI Bastion or another restricted OCI-supported administrative path for SSH.
3. SSH private key remains under administrator control and is never committed to Git.
4. Application traffic is separate from administrative traffic.

## Security boundary

Administrative access must not be used as evidence that Phone-01 or Phone-02 is connected.

The following remain separate gates:

- VM administration;
- Team Room service health;
- OIDC/session authentication;
- room membership;
- bidirectional AI message exchange;
- physical Phone-02 identity.

## Recovery rule

If the chosen OCI administrative path cannot be operated reliably from the tablet, stop before public application exposure and choose another supported remote-access path. Do not weaken security by opening unrestricted SSH merely to make tablet administration easier.

## Current status

This is a preparation document only. No OCI VM or administrative endpoint is claimed to exist.
