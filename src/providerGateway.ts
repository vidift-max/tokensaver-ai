import { invoke } from "@tauri-apps/api/core";

export type ProviderId = "openai" | "anthropic" | "gemini" | "kimi";
export type ProviderCapabilities = {
  streaming: boolean; tools: boolean; structured_output: boolean;
  vision: boolean; embeddings: boolean; model_discovery: boolean;
};
export type ProviderStatus = {
  provider: ProviderId; label: string; configured: boolean; model: string | null;
  credential_source: "process_environment"; capabilities: ProviderCapabilities; note: string;
};
export type ProviderRegistry = { providers: ProviderStatus[] };
export type ProviderChatRequest = {
  provider: ProviderId; model?: string | null; system?: string | null;
  messages: { role: "user" | "assistant"; content: string }[];
  temperature?: number | null; max_output_tokens?: number | null; stream?: boolean;
};
export type ProviderUsage = {
  input_tokens: number | null; output_tokens: number | null; total_tokens: number | null; source: "provider_reported";
};
export type ProviderChatResponse = {
  provider: ProviderId; model: string; text: string; usage: ProviderUsage | null;
  latency_ms: number; request_id: string; provider_request_id: string | null; estimated_cost: number | null;
};
export type ProviderGatewayError = {
  code: string; message: string; retryable: boolean; request_id: string; provider_status: number | null;
};
export const providerGateway = {
  registry: () => invoke<ProviderRegistry>("provider_registry"),
  complete: (request: ProviderChatRequest) => invoke<ProviderChatResponse>("provider_chat", { request })
};
