import { invoke } from "@tauri-apps/api/core";

export type AppError = {
  code: string;
  message: string;
  details: Record<string, unknown>;
};

export type TokenEstimate = {
  method: "local_estimate" | "tokenizer" | "api_reported" | "provider_usage";
  count: number;
  approximate: boolean;
  provider: string | null;
  model: string | null;
  limitation: string | null;
};

export const api = {
  health: () => invoke<{ ok: boolean; version: string }>("health"),
  sha256Text: (input: string) => invoke<string>("sha256_text", { input }),
  estimateTokens: (input: string) => invoke<TokenEstimate>("estimate_tokens", { input }),
  initializeProject: (projectId: string) =>
    invoke<{ id: string; path: string }>("initialize_project", { projectId }),
  importTextFile: (projectId: string, logicalPath: string, sourcePath: string) =>
    invoke<string>("import_text_file", { projectId, logicalPath, sourcePath }),
};
