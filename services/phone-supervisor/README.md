# Phone Supervisor Windows Service — P2.36 skeleton

Windows-only native service and local RPC host. Build/run instructions and frozen trust boundary are in [P2.36 contract](../../docs/phone-02/P2.36-SUPERVISOR-CONTRACT.md).

This directory is deliberately isolated from the experimental P2.34 harness. The production service must not inherit P2.34 global nonce/session state, disposable process lifecycle or test-only package assumptions.

P2.36 implementation is not a production installer. The Windows CI job is the only intended execution environment.
