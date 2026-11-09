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

export type FileSummary = {
  id: string;
  logical_path: string;
  latest_version: number;
  byte_len: number;
  sha256: string;
};

export type Page<T> = {
  items: T[];
  offset: number;
  limit: number;
  has_more: boolean;
};

export type FileChunk = {
  file_id: string;
  version_no: number;
  sha256: string;
  byte_len: number;
  offset: number;
  data: string;
  eof: boolean;
};

export const api = {
  health: () => invoke<{ ok: boolean; version: string }>("health"),
  sha256Text: (input: string) => invoke<string>("sha256_text", { input }),
  estimateTokens: (input: string) => invoke<TokenEstimate>("estimate_tokens", { input }),
  initializeProject: (projectId: string) =>
    invoke<{ id: string; path: string }>("initialize_project", { projectId }),
  importTextFile: (projectId: string, logicalPath: string, sourcePath: string) =>
    invoke<string>("import_text_file", { projectId, logicalPath, sourcePath }),
  listProjectFiles: (projectId: string, offset: number, limit: number) =>
    invoke<Page<FileSummary>>("list_project_files", { projectId, offset, limit }),
  readFileVersion: (
    projectId: string,
    fileId: string,
    versionNo: number,
    offset: number,
    limit: number
  ) =>
    invoke<FileChunk>("read_file_version", {
      projectId,
      fileId,
      versionNo,
      offset,
      limit,
    }),
};
