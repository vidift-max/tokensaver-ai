# First-Contact OCI Network Plan

## Goal

Expose only the HTTPS reverse proxy while keeping application and data services private.

## Public ingress

The VM security rules should permit:

- TCP 80 from the Internet: HTTP challenge/redirect
- TCP 443 from the Internet: HTTPS application ingress

SSH should not be opened broadly when an OCI Bastion/private access path is available. If temporary public SSH is required during initial provisioning, restrict it to the administrator's source IP and remove it after access is established.

## Private services

These ports must not be exposed by the OCI public security rules:

- PostgreSQL: 5432
- Keycloak: 8080
- Team Room: 8081

Docker Compose provides the private service network. The host firewall/security rules provide the cloud boundary.

## Routing

Internet -> OCI public IP -> Caddy :80/:443 -> Team Room :8081

Caddy is the only application ingress.

## DNS

No DNS name is invented in this plan. A real domain/subdomain must be supplied/configured before automatic public TLS can be considered operational.

## Evidence

After provisioning, record the actual OCI security rules and verify from an external client that:

1. 80/443 are reachable as intended.
2. 5432/8080/8081 are not publicly reachable.
3. Caddy can reach Team Room internally.

Network reachability alone is not evidence of AI-to-AI communication.
