# Phone Supervisor Windows Service — P2.37 controlled bridge fixture

Windows-only native service and local RPC host. The P2.36 Windows identity boundary remains the only local authorization authority.

P2.37 adds a test-only Supervisor-owned adapter invocation. The AppContainer can request a fixed scenario through RPC after the Supervisor re-observes and validates the Windows caller token and the already-authorized local session. It cannot supply AI_PHONE_ID, principal_id, endpoint identity, sender identity, workload assertion, session bearer, endpoint credential or operator key.

The Fake Supervisor Team Room adapter is deterministic and in-memory. It has no socket, HTTP, credential-store or filesystem implementation. Its test assertion, session token and endpoint credential are fake values held in private adapter fields. The fake scenario proves the local RPC-to-adapter seam only; it does not claim HTTPS, OIDC federation, a live Team Room exchange, database writes, or a Phone-01 connection.

Build/run instructions and the frozen P2.36 trust boundary are in the P2.36 contract. The P2.37 bridge contract is in docs/phone-02/P2.37-TEAM-ROOM-BRIDGE.md.

This directory remains isolated from production. The Windows CI job is the only intended execution environment.
