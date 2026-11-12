use reqwest::{header, Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{env, sync::atomic::{AtomicUsize, Ordering}, time::Instant};
use uuid::Uuid;

const MAX_MESSAGES: usize = 128;
const MAX_INPUT_BYTES: usize = 1_000_000;
const DEFAULT_TIMEOUT_SECS: u64 = 45;
const MAX_TIMEOUT_SECS: u64 = 120;
const MAX_CONCURRENT_REQUESTS: usize = 8;
static ACTIVE_REQUESTS: AtomicUsize = AtomicUsize::new(0);

struct RequestPermit;

impl Drop for RequestPermit {
    fn drop(&mut self) { ACTIVE_REQUESTS.fetch_sub(1, Ordering::AcqRel); }
}

fn acquire_request_permit(request_id: &str) -> Result<RequestPermit, GatewayError> {
    let acquired = ACTIVE_REQUESTS.fetch_update(Ordering::AcqRel, Ordering::Relaxed, |active| {
        (active < MAX_CONCURRENT_REQUESTS).then_some(active + 1)
    });
    acquired.map(|_| RequestPermit).map_err(|_| GatewayError::new(
        "CONCURRENCY_LIMIT", "Gateway concurrency limit reached; retry later.", true, request_id, None
    ))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider { Openai, Anthropic, Gemini, Kimi }

impl Provider {
    fn key_env(self) -> &'static str {
        match self { Self::Openai => "TOKENSAVER_OPENAI_API_KEY", Self::Anthropic => "TOKENSAVER_ANTHROPIC_API_KEY", Self::Gemini => "TOKENSAVER_GEMINI_API_KEY", Self::Kimi => "TOKENSAVER_KIMI_API_KEY" }
    }
    fn model_env(self) -> &'static str {
        match self { Self::Openai => "TOKENSAVER_OPENAI_MODEL", Self::Anthropic => "TOKENSAVER_ANTHROPIC_MODEL", Self::Gemini => "TOKENSAVER_GEMINI_MODEL", Self::Kimi => "TOKENSAVER_KIMI_MODEL" }
    }
    fn label(self) -> &'static str {
        match self { Self::Openai => "OpenAI", Self::Anthropic => "Anthropic", Self::Gemini => "Google Gemini", Self::Kimi => "Moonshot Kimi" }
    }
    fn endpoint(self, model: &str) -> String {
        match self {
            Self::Openai => "https://api.openai.com/v1/chat/completions".into(),
            Self::Anthropic => "https://api.anthropic.com/v1/messages".into(),
            Self::Kimi => "https://api.moonshot.ai/v1/chat/completions".into(),
            Self::Gemini => format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"),
        }
    }

}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub chat: bool,
    pub limits_known: bool,
    pub streaming: bool,
    pub tools: bool,
    pub structured_output: bool,
    pub vision: bool,
    pub embeddings: bool,
    pub model_discovery: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub provider: Provider,
    pub label: String,
    pub configured: bool,
    pub model: Option<String>,
    pub credential_source: &'static str,
    pub capabilities: ProviderCapabilities,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderRegistry {
    pub providers: Vec<ProviderStatus>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole { User, Assistant }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub provider: Provider,
    pub model: Option<String>,
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub provider: Provider,
    pub model: String,
    pub text: String,
    pub usage: Option<Usage>,
    pub latency_ms: u128,
    pub request_id: String,
    pub provider_request_id: Option<String>,
    pub estimated_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub request_id: String,
    pub provider_status: Option<u16>,
}

impl GatewayError {
    fn new(code: &str, message: &str, retryable: bool, request_id: &str, status: Option<u16>) -> Self {
        Self { code: code.into(), message: message.into(), retryable, request_id: request_id.into(), provider_status: status }
    }
}

fn configured_model(provider: Provider) -> Option<String> {
    env::var(provider.model_env()).ok().map(|s| s.trim().to_string()).filter(|s| valid_model_name(s))
}

fn valid_model_name(model: &str) -> bool {
    !model.is_empty() && model.len() <= 128 && model.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

fn provider_capabilities(_provider: Provider) -> ProviderCapabilities {
    // Only features implemented by this gateway are advertised. Do not infer model-specific features.
    ProviderCapabilities { chat: true, limits_known: false, streaming: false, tools: false, structured_output: false, vision: false, embeddings: false, model_discovery: false }
}

pub fn registry() -> ProviderRegistry {
    let providers = [Provider::Openai, Provider::Anthropic, Provider::Gemini, Provider::Kimi].into_iter().map(|provider| {
        let model = configured_model(provider);
        let has_key = env::var(provider.key_env()).map(|s| !s.trim().is_empty()).unwrap_or(false);
        ProviderStatus {
            provider,
            label: provider.label().into(),
            configured: has_key && model.is_some(),
            model,
            credential_source: "process_environment",
            capabilities: provider_capabilities(provider),
            note: "Model name is operator-configured; provider model discovery and feature negotiation are not implemented.".into(),
        }
    }).collect();
    ProviderRegistry { providers }
}

fn validate_request(req: &ChatRequest, request_id: &str) -> Result<(), GatewayError> {
    let total = req.messages.iter().map(|m| m.content.len()).sum::<usize>() + req.system.as_ref().map_or(0, String::len);
    if req.messages.is_empty() || req.messages.len() > MAX_MESSAGES || total > MAX_INPUT_BYTES {
        return Err(GatewayError::new("INVALID_REQUEST", "Request exceeds gateway message or input limits.", false, request_id, None));
    }
    if req.stream {
        return Err(GatewayError::new("UNSUPPORTED_CAPABILITY", "Streaming is not implemented by this gateway.", false, request_id, None));
    }
    if req.temperature.is_some_and(|v| !v.is_finite() || !(0.0..=2.0).contains(&v)) {
        return Err(GatewayError::new("INVALID_REQUEST", "Temperature must be finite and between 0 and 2.", false, request_id, None));
    }
    if req.max_output_tokens.is_some_and(|v| v == 0 || v > 1_000_000) {
        return Err(GatewayError::new("INVALID_REQUEST", "max_output_tokens is outside the supported range.", false, request_id, None));
    }
    Ok(())
}

fn model_for(req: &ChatRequest, request_id: &str) -> Result<String, GatewayError> {
    let model = req.model.clone().or_else(|| configured_model(req.provider))
        .ok_or_else(|| GatewayError::new("PROVIDER_NOT_CONFIGURED", "Provider model is not configured.", false, request_id, None))?;
    if !valid_model_name(&model) {
        return Err(GatewayError::new("INVALID_REQUEST", "Model identifier contains unsupported characters.", false, request_id, None));
    }
    Ok(model)
}

fn messages_openai(req: &ChatRequest) -> Vec<Value> {
    let mut messages = Vec::new();
    if let Some(system) = req.system.as_ref().filter(|s| !s.is_empty()) { messages.push(json!({"role":"system","content":system})); }
    messages.extend(req.messages.iter().map(|m| json!({"role":match m.role { MessageRole::User => "user", MessageRole::Assistant => "assistant" },"content":m.content})));
    messages
}

fn messages_anthropic(req: &ChatRequest) -> Vec<Value> {
    req.messages.iter().map(|m| json!({"role":match m.role { MessageRole::User => "user", MessageRole::Assistant => "assistant" },"content":m.content})).collect()
}

fn parse_usage(provider: Provider, body: &Value) -> Option<Usage> {
    let (input, output, total) = match provider {
        Provider::Openai | Provider::Kimi => {
            let u = body.get("usage")?;
            (u.get("prompt_tokens").and_then(Value::as_u64), u.get("completion_tokens").and_then(Value::as_u64), u.get("total_tokens").and_then(Value::as_u64))
        }
        Provider::Anthropic => {
            let u = body.get("usage")?;
            let i = u.get("input_tokens").and_then(Value::as_u64);
            let o = u.get("output_tokens").and_then(Value::as_u64);
            (i, o, i.zip(o).map(|(a,b)| a.saturating_add(b)))
        }
        Provider::Gemini => {
            let u = body.get("usageMetadata")?;
            let i = u.get("promptTokenCount").and_then(Value::as_u64);
            let o = u.get("candidatesTokenCount").and_then(Value::as_u64);
            (i, o, u.get("totalTokenCount").and_then(Value::as_u64).or_else(|| i.zip(o).map(|(a,b)| a.saturating_add(b))))
        }
    };
    Some(Usage { input_tokens: input, output_tokens: output, total_tokens: total, source: "provider_reported".into() })
}

fn response_text(provider: Provider, body: &Value) -> Option<String> {
    match provider {
        Provider::Openai | Provider::Kimi => body.pointer("/choices/0/message/content")?.as_str().map(str::to_string),
        Provider::Anthropic => body.get("content")?.as_array()?.iter().filter_map(|v| {
            (v.get("type")?.as_str()? == "text").then(|| v.get("text")?.as_str().map(str::to_string)).flatten()
        }).collect::<Vec<_>>().into_iter().reduce(|a,b| format!("{a}{b}")),
        Provider::Gemini => body.pointer("/candidates/0/content/parts")?.as_array()?.iter().filter_map(|v| v.get("text")?.as_str().map(str::to_string)).reduce(|a,b| format!("{a}{b}")),
    }
}

fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

pub async fn complete(req: ChatRequest) -> Result<ChatResponse, GatewayError> {
    let request_id = Uuid::new_v4().to_string();
    validate_request(&req, &request_id)?;
    let model = model_for(&req, &request_id)?;
    let key = env::var(req.provider.key_env()).ok().filter(|s| !s.trim().is_empty())
        .ok_or_else(|| GatewayError::new("CREDENTIAL_MISSING", "Provider credential is not available in the process environment.", false, &request_id, None))?;
    let timeout = env::var("TOKENSAVER_PROVIDER_TIMEOUT_SECS").ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS);
    let client = Client::builder().timeout(std::time::Duration::from_secs(timeout))
        .redirect(reqwest::redirect::Policy::none()).build()
        .map_err(|_| GatewayError::new("TRANSPORT_ERROR", "Could not initialize provider transport.", false, &request_id, None))?;

    let _permit = acquire_request_permit(&request_id)?;
    let endpoint = req.provider.endpoint(&model);
    let mut builder = client.post(endpoint).header(header::CONTENT_TYPE, "application/json");
    let body = match req.provider {
        Provider::Openai | Provider::Kimi => {
            builder = builder.bearer_auth(&key);
            let mut payload = json!({"model":model,"messages":messages_openai(&req)});
            if let Some(temp) = req.temperature { payload["temperature"] = json!(temp); }
            if let Some(max) = req.max_output_tokens { payload["max_tokens"] = json!(max); }
            payload
        }
        Provider::Anthropic => {
            builder = builder.header("x-api-key", &key).header("anthropic-version", "2023-06-01");
            let mut payload = json!({"model":model,"messages":messages_anthropic(&req),"max_tokens":req.max_output_tokens.unwrap_or(1024)});
            if let Some(system) = req.system.as_ref().filter(|s| !s.is_empty()) { payload["system"] = json!(system); }
            if let Some(temp) = req.temperature { payload["temperature"] = json!(temp); }
            payload
        }
        Provider::Gemini => {
            builder = builder.header("x-goog-api-key", &key);
            let contents: Vec<Value> = req.messages.iter().map(|m| json!({"role":match m.role { MessageRole::User => "user", MessageRole::Assistant => "model" },"parts":[{"text":m.content}]})).collect();
            let mut payload = json!({"contents":contents});
            if let Some(system) = req.system.as_ref() { payload["systemInstruction"] = json!({"parts":[{"text":system}]}); }
            let mut config = json!({});
            if let Some(temp) = req.temperature { config["temperature"] = json!(temp); }
            if let Some(max) = req.max_output_tokens { config["maxOutputTokens"] = json!(max); }
            if !config.as_object().is_some_and(|o| o.is_empty()) { payload["generationConfig"] = config; }
            payload
        }
    };

    let started = Instant::now();
    let mut response = builder.json(&body).send().await.map_err(|e| {
        let timed_out = e.is_timeout();
        GatewayError::new(if timed_out { "TIMEOUT" } else { "TRANSPORT_ERROR" },
            if timed_out { "Provider request timed out." } else { "Provider transport failed." }, false, &request_id, None)
    })?;
    let status = response.status();
    let provider_request_id = response.headers().get("x-request-id").or_else(|| response.headers().get("request-id")).or_else(|| response.headers().get("x-goog-request-id"))
        .and_then(|v| v.to_str().ok()).map(str::to_string);
    if !status.is_success() {
        // Drop error bodies: they can contain user content or sensitive diagnostics.
        return Err(GatewayError::new("PROVIDER_HTTP_ERROR", "Provider rejected the request.", retryable_status(status), &request_id, Some(status.as_u16())));
    }
    if response.content_length().is_some_and(|length| length > 4_000_000) {
        return Err(GatewayError::new("RESPONSE_TOO_LARGE", "Provider response exceeded the gateway limit.", false, &request_id, Some(status.as_u16())));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| GatewayError::new("RESPONSE_READ_ERROR", "Could not read provider response.", false, &request_id, Some(status.as_u16())))? {
        if bytes.len().saturating_add(chunk.len()) > 4_000_000 {
            return Err(GatewayError::new("RESPONSE_TOO_LARGE", "Provider response exceeded the gateway limit.", false, &request_id, Some(status.as_u16())));
        }
        bytes.extend_from_slice(&chunk);
    }
    let parsed: Value = serde_json::from_slice(&bytes).map_err(|_| GatewayError::new("MALFORMED_RESPONSE", "Provider returned malformed JSON.", false, &request_id, Some(status.as_u16())))?;
    let text = response_text(req.provider, &parsed).ok_or_else(|| GatewayError::new("MALFORMED_RESPONSE", "Provider response did not contain supported text output.", false, &request_id, Some(status.as_u16())))?;
    Ok(ChatResponse { provider: req.provider, model, text, usage: parse_usage(req.provider, &parsed), latency_ms: started.elapsed().as_millis(), request_id, provider_request_id, estimated_cost: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(provider: Provider) -> ChatRequest {
        ChatRequest { provider, model: Some("model-test".into()), system: Some("system".into()), messages: vec![ChatMessage { role: MessageRole::User, content: "hello".into() }], temperature: Some(0.2), max_output_tokens: Some(100), stream: false }
    }

    #[test]
    fn provider_contract_covers_all_targets_without_claiming_discovery() {
        let providers = [Provider::Openai, Provider::Anthropic, Provider::Gemini, Provider::Kimi];
        assert_eq!(providers.len(), 4);
        for provider in providers {
            let caps = provider_capabilities(provider);
            assert!(caps.chat && !caps.limits_known && !caps.streaming && !caps.model_discovery);
        }
    }

    #[test]
    fn provider_request_mappings_preserve_anthropic_system_separation() {
        let req = request(Provider::Anthropic);
        let messages = messages_anthropic(&req);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(req.system.as_deref(), Some("system"));
    }

    #[test]
    fn openai_compatible_mapping_has_system_message() {
        let messages = messages_openai(&request(Provider::Kimi));
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
    }

    #[test]
    fn usage_normalization_keeps_provider_reported_counts() {
        let openai = json!({"usage":{"prompt_tokens":10,"completion_tokens":4,"total_tokens":14}});
        let usage = parse_usage(Provider::Openai, &openai).expect("usage");
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(4));
        let anthropic = json!({"usage":{"input_tokens":8,"output_tokens":3}});
        let usage = parse_usage(Provider::Anthropic, &anthropic).expect("usage");
        assert_eq!(usage.total_tokens, Some(11));
        let gemini = json!({"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":2,"totalTokenCount":9}});
        assert_eq!(parse_usage(Provider::Gemini, &gemini).expect("usage").total_tokens, Some(9));
    }

    #[test]
    fn response_normalization_handles_provider_shapes() {
        assert_eq!(response_text(Provider::Openai, &json!({"choices":[{"message":{"content":"ok"}}]})).as_deref(), Some("ok"));
        assert_eq!(response_text(Provider::Anthropic, &json!({"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]})).as_deref(), Some("ab"));
        assert_eq!(response_text(Provider::Gemini, &json!({"candidates":[{"content":{"parts":[{"text":"ok"}]}}]})).as_deref(), Some("ok"));
    }

    #[test]
    fn validation_rejects_streaming_and_oversized_requests() {
        let mut req = request(Provider::Openai);
        req.stream = true;
        assert_eq!(validate_request(&req, "test").expect_err("stream unsupported").code, "UNSUPPORTED_CAPABILITY");
        req.stream = false;
        req.messages[0].content = "x".repeat(MAX_INPUT_BYTES + 1);
        assert_eq!(validate_request(&req, "test").expect_err("input too large").code, "INVALID_REQUEST");
    }

    #[test]
    fn model_names_are_restricted_and_retry_policy_is_classified() {
        assert!(valid_model_name("vendor-model.1"));
        assert!(!valid_model_name("https://evil.example"));
        assert!(retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(!retryable_status(StatusCode::BAD_GATEWAY));
        assert!(!retryable_status(StatusCode::BAD_REQUEST));
    }
    fn concurrency_permit_is_released_and_limit_is_explicit() {
        let permit = acquire_request_permit("test").expect("permit under limit");
        assert!(ACTIVE_REQUESTS.load(Ordering::Acquire) > 0);
        drop(permit);
        assert_eq!(ACTIVE_REQUESTS.load(Ordering::Acquire), 0);
    }

    fn live_request(provider: Provider) -> ChatRequest {
        ChatRequest {
            provider,
            model: None,
            system: Some("Answer with a short provider connectivity confirmation.".into()),
            messages: vec![ChatMessage { role: MessageRole::User, content: "Reply with the word connected.".into() }],
            temperature: Some(0.0),
            max_output_tokens: Some(24),
            stream: false,
        }
    }

    #[tokio::test]
    #[ignore = "requires TOKENSAVER_OPENAI_API_KEY and TOKENSAVER_OPENAI_MODEL; sends a real provider request"]
    async fn live_openai_chat_smoke() {
        let response = complete(live_request(Provider::Openai)).await.expect("live OpenAI request");
        assert_eq!(response.provider, Provider::Openai);
        assert!(!response.text.trim().is_empty());
    }

    #[tokio::test]
    #[ignore = "requires TOKENSAVER_ANTHROPIC_API_KEY and TOKENSAVER_ANTHROPIC_MODEL; sends a real provider request"]
    async fn live_anthropic_chat_smoke() {
        let response = complete(live_request(Provider::Anthropic)).await.expect("live Anthropic request");
        assert_eq!(response.provider, Provider::Anthropic);
        assert!(!response.text.trim().is_empty());
    }

    #[tokio::test]
    #[ignore = "requires TOKENSAVER_GEMINI_API_KEY and TOKENSAVER_GEMINI_MODEL; sends a real provider request"]
    async fn live_gemini_chat_smoke() {
        let response = complete(live_request(Provider::Gemini)).await.expect("live Gemini request");
        assert_eq!(response.provider, Provider::Gemini);
        assert!(!response.text.trim().is_empty());
    }

    #[tokio::test]
    #[ignore = "requires TOKENSAVER_KIMI_API_KEY and TOKENSAVER_KIMI_MODEL; sends a real provider request"]
    async fn live_kimi_chat_smoke() {
        let response = complete(live_request(Provider::Kimi)).await.expect("live Kimi request");
        assert_eq!(response.provider, Provider::Kimi);
        assert!(!response.text.trim().is_empty());
    }


}
