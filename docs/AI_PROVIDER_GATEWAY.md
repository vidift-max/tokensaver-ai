# AI Provider Gateway — Runtime and Hardening Notes

## Scope and trust boundary

The desktop Rust/Tauri backend owns outbound provider HTTP requests. React receives typed IPC contracts only and does not receive provider credentials. The gateway is not wired into AI Team Room, distributed Runner, or Recovery Worker.

## Providers

| Provider | Endpoint | Authentication | Request protocol |
|---|---|---|---|
| OpenAI | `https://api.openai.com/v1/chat/completions` | Bearer | OpenAI chat completions |
| Anthropic | `https://api.anthropic.com/v1/messages` | `x-api-key` + version header | Anthropic Messages |
| Gemini | `https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent` | `x-goog-api-key` | Gemini generateContent |
| Kimi | `https://api.moonshot.ai/v1/chat/completions` | Bearer | Moonshot OpenAI-compatible chat |

Kimi is a distinct provider identity. Protocol similarity does not imply identical models, capabilities, limits, pricing, or service behavior.

## Credentials — current limitation

The current adapter reads provider keys from process environment variables:
- `TOKENSAVER_OPENAI_API_KEY`
- `TOKENSAVER_ANTHROPIC_API_KEY`
- `TOKENSAVER_GEMINI_API_KEY`
- `TOKENSAVER_KIMI_API_KEY`

Model IDs are operator-configured using the corresponding `TOKENSAVER_*_MODEL` variables. The registry reports only whether a non-empty credential and valid model setting are present; it does not validate the credential.

**OS credential vault is PENDING.** No keychain/Windows Credential Manager integration or credential-management UI is present. Environment variables are not approved production-grade desktop secret storage. Do not pass keys through Team Room or Runner payloads. The gateway does not intentionally log or persist credentials, but process-environment isolation from other child processes has not been proven.

## Request and response

The normalized contract contains provider, model, optional system instruction, ordered messages, optional temperature and output limit, stream intent, normalized text, provider-reported usage, latency, internal request ID, external request ID and optional estimated cost. Provider-specific fields are omitted or mapped according to each provider's protocol.

Fixed HTTPS endpoints are used. User-supplied endpoint URLs are not accepted. Redirects are disabled. Model identifiers are restricted to a safe character set. Input is limited to 128 messages and 1 MB of aggregate text. Successful response bodies are read incrementally with a 4 MB cap; error response bodies are discarded. The transport timeout is bounded to 1–120 seconds (default 45).

## Streaming and cancellation

**Streaming is not implemented.** Requests with `stream=true` return `UNSUPPORTED_CAPABILITY`. The existing Tauri command returns a single completed response and has no request-scoped cancellation token, event channel, or downstream backpressure contract. Dropping a Rust future can cancel its in-flight request, but the current frontend IPC does not expose a verified cancellation lifecycle. Do not advertise streaming or cancellation as supported.

## Resilience

A process-wide in-flight request cap of eight is enforced. Excess requests receive a bounded concurrency error. There is no automatic retry. HTTP 429 is classified as retryable metadata; HTTP 5xx is not marked retryable because a failed response can follow provider acceptance or billing of a POST. Authentication, authorization, invalid request, timeout and transport failures are not automatically replayed. Exponential backoff and circuit breaker remain pending.

## Usage ledger

The Tauri command creates a local SQLite usage row before sending the provider request, then finalizes it as succeeded or failed. The ledger records provider, model when known, internal request ID, external request ID when returned, provider-reported token counts when available, timestamp, latency, status, normalized error code, and origin (`tauri_ipc`). It does not store prompts, response text, or credentials.

Project and actor IDs are NULL because the current desktop command has no authenticated project/actor context. This is an explicit attribution limitation, not an implied global authorization model. A ledger row left in `started` indicates a process interruption or failed finalization and must not be counted as successful usage.

## Cost engine

The ledger contains a versioned, effective-dated `provider_model_prices` table with provider, exact model ID, input/output USD per million tokens, effective timestamp, source URL and recorded timestamp. No prices are seeded or invented. Cost is calculated only when complete provider-reported input/output usage and both sourced rates are available for the exact provider/model and effective date. Otherwise cost remains NULL/unknown. This is an estimate, not billing or a financial charge.

## Validation

Structural tests cover request/response mapping, usage normalization, limits, capability declarations, retry classification, concurrency permits, ledger persistence and versioned cost calculation. Four ignored live smoke tests make actual provider requests and require operator-supplied credentials and model IDs. They are not executed by default.

No provider API call is claimed until a live request is executed and its redacted result is recorded. Current provider live status: **API REAL BLOQUEADA — CREDENCIAL AUSENTE**.

## Remaining release blockers

- OS keychain / Windows Credential Manager integration and rotation lifecycle.
- Proven secret isolation from all other processes and CI artifacts.
- Streaming event contract, cancellation token propagation and backpressure.
- Authenticated project/actor attribution and tenant authorization.
- Circuit breaker and bounded retry/backoff policy with provider-specific idempotency guarantees.
- Live authenticated validation for each provider.
- Sourced price entries for each exact model/version.
- Full cross-platform regression and benchmark evidence.
