# First-Contact Pre-Deploy Gate

The server must not be deployed until all preparation gates below are satisfied.

## Gate A — Account and cost

- OCI account is accessible from the tablet.
- Home region is known.
- Selected VM shape is explicitly Always Free eligible.
- Selected storage is explicitly Always Free eligible.
- No paid resource is intentionally selected.

## Gate B — Administrative access

- A safe tablet-compatible administrative path is available.
- SSH private key is controlled by the administrator.
- No private key is committed to Git.
- Administrative access is separate from application ingress.

## Gate C — Network

- Public ingress is limited to the intended HTTPS path.
- PostgreSQL, Keycloak, and Team Room are not publicly exposed.
- DNS is not invented or assumed.

## Gate D — Application

- The repository branch contains the prepared Team Room stack.
- Secrets are supplied at runtime, not committed.
- Caddy is the only public application ingress.

## Gate E — Evidence

After VM creation, capture the VM configuration and cost eligibility before installing the application.

## Explicit non-goals

Passing this gate does not mean the first contact has happened. Phone-01 and Phone-02 remain separate verification stages.

## Stop conditions

Stop immediately if:

- OCI reports unexpected cost;
- Always Free eligibility is unavailable;
- administrative access cannot be operated safely from the tablet;
- a required secret would have to be committed to Git;
- a service would need to be exposed publicly without a documented reason.
