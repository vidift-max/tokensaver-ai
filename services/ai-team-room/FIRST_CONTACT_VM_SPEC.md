# First-Contact VM Specification

## Initial target

Create one small ARM64 Linux VM for the first-contact Team Room.

Target:

- OCI Ampere A1 Flex
- 2 OCPUs
- 12 GB RAM
- Ubuntu LTS ARM64
- Always Free eligible allocation only

## Workload

The VM is intended to run:

- Caddy
- ai-team-room-service
- PostgreSQL 16
- Keycloak

Docker Compose provides service isolation.

## Storage

Use the smallest practical Always Free-eligible boot volume that is sufficient for the initial workload. Do not create additional paid storage without explicit authorization.

## Scaling boundary

This is an initial communication server, not a production-scale deployment. Do not add replicas, load balancers, managed databases, or other paid infrastructure at this stage.

## Evidence required after creation

Record the actual:

- region
- availability domain
- VM shape
- OCPU count
- memory
- boot volume size
- public IPv4
- Always Free eligibility

The VM creation itself is infrastructure evidence only. It does not prove that the Team Room or either AI is connected.
