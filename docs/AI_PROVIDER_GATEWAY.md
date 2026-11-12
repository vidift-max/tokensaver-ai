# AI Provider Gateway

## Scope and trust boundary

The desktop Rust/Tauri backend owns provider HTTP requests. React receives typed IPC contracts only; it never receives provider credentials. The AI Team Room and distributed Runner are not wired to this gateway in this mission.

## Providers and endpoints

| Provider | Endpoint family | Authentication | Request mapping |
|---|---|---|---|
| OpenAI | \`api.openai.com/v1/chat/completions\` | Bearer token | OpenAI chat messages |
| Anthropic | \`api.anthropic.com/v1/messages\` | \`x-api-key\` plus version header | System instruction separated from user/assistant messages |
| Gemini | \`generativelanguage.googleapis.com/v1beta/models/{model}:generateContent\` | \`x-goog-api-key\` | contents/parts and systemInstruction |
| Kimi | \`api.moonshot.ai/v1/chat/completions\` | Bearer token | Moonshot OpenAI-compatible chat format |

A protocol similarity does not imply feature or model equivalence. Kimi remains a distinct provider entry.

## Runtime configuration

The gateway reads credentials and model identifiers from the desktop process environment:

- \`TOKENSAVER_OPENAI_API_KEY\` and \`TOKENSAVER_OPENAI_MODEL\`
- \`TOKENSAVER_ANTHROPIC_API_KEY\` and \`TOKENSAVER_ANTHROPIC_MODEL\`
- \`TOKENSAVER_GEMINI_API_KEY\` and \`TOKENSAVER_GEMINI_MODEL\`
- \`TOKENSAVER_KIMI_API_KEY\` and \`TOKENSAVER_KIMI_MODEL\`
- Optional \`TOKENSAVER_PROVIDER_TIMEOUT_SECS\` (clamped to 1–120 seconds; default 45)

No credential is written to SQLite, frontend state, repository files, or gateway logs. The registry reports whether a non-empty credential and valid configured model are present; it does not validate the key with the provider and does not discover provider models.

**Security limitation:** process environment variables are a bootstrap mechanism, not a secure desktop credential vault. OS keychain/credential-manager integration and credential rotation UX are pending. Do not treat this mechanism as production-grade secret storage.

## Unified request

The Tauri IPC command \`provider_chat\` accepts a provider, optional model override, optional system instruction, ordered user/assistant messages, optional temperature, optional max output tokens, and a stream flag. \`provider_registry\` returns configured status and explicit capabilities.

The gateway enforces at most 128 messages and 1 MB aggregate input text, validates model identifiers, uses fixed HTTPS provider hosts, disables redirects, applies a bounded timeout, rejects oversized responses while reading chunks, and returns sanitized errors without forwarding provider error bodies.

## Normalized response and usage

Responses include provider, model, generated text, provider-reported usage when supplied, latency, internal request ID, provider request ID when present, and a separate estimated-cost field. Estimated cost is always null in this implementation. Usage is returned to the caller only; no durable usage ledger is implemented.

## Capability truth table

Currently implemented: non-streaming text chat and provider-reported token usage normalization for supported response shapes.

Currently unavailable: streaming, tools/function calling, structured output, vision, embeddings, provider model discovery, automatic retries, circuit breaker, idempotency/deduplication, durable usage ledger, pricing, and OS credential vault. The registry reports unsupported capabilities as false.

Retryability is classified for HTTP 429 and 5xx responses only. No automatic retry is performed because chat POST requests are not assumed idempotent. Timeout and transport failures are not automatically retried.

## Validation

Unit tests cover request validation, provider request/response mapping, usage normalization, capability declarations, model-name validation and retry classification. They are structural tests and do not call external providers.

Live integration requires separately provisioned provider credentials and explicit execution. No provider API call is claimed by this mission until such a call is actually performed.
