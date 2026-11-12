#!/usr/bin/env node
/**
 * Write exact-SHA run provenance without serializing credentials.
 */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";

const sha = process.env.BENCH_CODE_SHA || process.env.GITHUB_SHA || "";
if (!/^[a-f0-9]{40}$/i.test(sha)) throw new Error("BENCH_CODE_SHA or GITHUB_SHA must be the exact 40-character source SHA");
const api = process.env.TEAM_ROOM_API_URL || "";
const databaseUrl = process.env.DATABASE_URL || "";
if (!api || !databaseUrl) throw new Error("TEAM_ROOM_API_URL and DATABASE_URL are required");
const apiUrl = new URL(api);
if (apiUrl.username || apiUrl.password) throw new Error("TEAM_ROOM_API_URL must not contain embedded credentials");
const dbUrl = new URL(databaseUrl);
const pg = spawnSync("psql", [databaseUrl, "-X", "-At", "-v", "ON_ERROR_STOP=1", "-c",
  "SELECT json_build_object('server_version',current_setting('server_version'),'settings',(SELECT json_object_agg(name,setting) FROM pg_settings WHERE name IN ('shared_buffers','work_mem','max_connections','effective_cache_size','track_io_timing','jit')))::text"],
  { encoding: "utf8", env: process.env });
if (pg.status !== 0) throw new Error(pg.stderr || "Unable to query PostgreSQL provenance");
const configPath = process.env.BENCH_CONFIG_FILE;
const config = configPath ? JSON.parse(readFileSync(configPath, "utf8")) : {};
const safeNames = ["BENCH_CLIENTS","BENCH_PARTICIPANTS","BENCH_REQUIRE_DISTINCT_PARTICIPANTS","BENCH_EVENTS","BENCH_MESSAGES","BENCH_DURATION_SECONDS","BENCH_RATE_PER_SECOND","BENCH_MESSAGE_BYTES","BENCH_TIMEOUT_MS","BENCH_HEARTBEATS","BENCH_SETTLE_MS","BENCH_IRRELEVANT_WRITES_PER_ROOM","BENCH_TENANTS","BENCH_ROOMS","BENCH_SEED_MESSAGES","BENCH_SEED_TASKS","BENCH_SEED_DECISIONS","BENCH_SEED_ATTACHMENTS"];
const workload = Object.fromEntries(safeNames.filter(name => process.env[name] !== undefined || config[name] !== undefined).map(name => [name, process.env[name] ?? config[name]]));
const artifactDir = resolve(process.env.BENCH_ARTIFACT_DIR || "benchmark-artifacts");
mkdirSync(artifactDir, { recursive: true });
const manifest = {
  run_id: process.env.BENCH_RUN_ID || process.env.GITHUB_RUN_ID || "local-" + new Date().toISOString().replace(/[:.]/g,"-"),
  captured_at: new Date().toISOString(),
  source: { sha, branch: process.env.GITHUB_REF_NAME || process.env.BENCH_BRANCH || null },
  environment: { runner_os: process.platform, node: process.version, psql: spawnSync("psql", ["--version"], { encoding: "utf8" }).stdout.trim(), api_origin: apiUrl.origin, database_host: dbUrl.hostname, database_port: dbUrl.port || "5432", database_name: decodeURIComponent(dbUrl.pathname.slice(1)) },
  postgres: JSON.parse(pg.stdout.trim()),
  workload,
  dataset: (() => { try { return JSON.parse(readFileSync(resolve(artifactDir,"seed-metadata.json"),"utf8")).fixture; } catch { return null; } })(),
  secrets_included: false
};
writeFileSync(resolve(artifactDir,"run-manifest.json"), JSON.stringify(manifest,null,2)+"\n");
process.stdout.write(JSON.stringify({manifest:"run-manifest.json",source_sha:sha,postgres:manifest.postgres.server_version,secrets_included:false})+"\n");
