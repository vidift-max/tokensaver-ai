# First-Contact Post-VM Checklist

Run this checklist only after the OCI VM exists.

## 1. Confirm infrastructure

Record the VM evidence required by FIRST_CONTACT_VM_CREATION_EVIDENCE.md.

## 2. Confirm administration

Establish the approved tablet-compatible administrative path. Do not publish private service ports for convenience.

## 3. Confirm host baseline

Verify:

- Ubuntu ARM64 is running;
- system clock is sane;
- available CPU/RAM/storage match the recorded VM configuration;
- Docker/Compose can be installed for the target architecture.

## 4. Confirm network boundary

Verify that only the intended public ingress is reachable. PostgreSQL, Keycloak, and Team Room must remain private.

## 5. Prepare application

Only after the above checks pass:

- transfer/checkout the approved repository revision;
- create runtime environment configuration;
- supply secrets outside Git;
- start the Compose stack;
- run the Team Room health checks.

## Evidence boundary

A successful VM and host check is not an AI communication test. The first-contact evidence matrix remains authoritative for the final PASS.

## Stop conditions

Stop if the VM is not Always Free eligible, the host architecture is incompatible, private services become publicly reachable, or secrets cannot be supplied safely.
