#!/usr/bin/env node
/**
 * Reproducible HTTP load probe for an already provisioned Team Room.
 * Requires a real API URL, room-bound session token, and pre-seeded room.
 * This script never provisions identities, grants, tenants, rooms or data.
 */
import { randomUUID } from "node:crypto";
import { performance } from "node:perf_hooks";
import { benchNumber, benchBoolean, benchValue, createRateGate, messageOfBytes } from "./benchmark-config.mjs";

const apiUrl = (process.env.TEAM_ROOM_API_URL || "").replace(/\/$/, "");
const roomId = process.env.TEAM_ROOM_ROOM_ID || "";
const tokenList = process.env.TEAM_ROOM_SESSION_TOKENS_JSON
  ? JSON.parse(process.env.TEAM_ROOM_SESSION_TOKENS_JSON)
  : [process.env.TEAM_ROOM_SESSION_TOKEN || ""];
const tokens = Array.isArray(tokenList) ? tokenList.filter(value => typeof value === "string" && value.length > 0) : [];
const participantCount = benchNumber("BENCH_PARTICIPANTS", tokens.length, { min: 1 });
const requireDistinct = benchBoolean("BENCH_REQUIRE_DISTINCT_PARTICIPANTS", false);
const clients = benchNumber("BENCH_CLIENTS", 8, { min: 1 });
const events = benchNumber("BENCH_EVENTS", 100, { min: 1 });
const messages = benchNumber("BENCH_MESSAGES", 0);
const timeoutMs = benchNumber("BENCH_TIMEOUT_MS", 10000, { min: 1 });
const durationSeconds = benchNumber("BENCH_DURATION_SECONDS", 0, { integer: false });
const ratePerSecond = benchNumber("BENCH_RATE_PER_SECOND", 0, { integer: false });
const messageBytes = benchNumber("BENCH_MESSAGE_BYTES", 256, { min: 1 });

if (!apiUrl || !roomId || tokens.length === 0) {
  console.error("Required: TEAM_ROOM_API_URL, TEAM_ROOM_ROOM_ID, and TEAM_ROOM_SESSION_TOKENS_JSON or TEAM_ROOM_SESSION_TOKEN");
  process.exit(2);
}
if (![clients, events, messages, timeoutMs].every(Number.isSafeInteger) ||
    clients < 1 || events < 1 || messages < 0 || timeoutMs < 1 || participantCount < 1 || (requireDistinct && new Set(tokens).size < clients)) {
  console.error("Invalid BENCH_CLIENTS / BENCH_EVENTS / BENCH_MESSAGES / BENCH_TIMEOUT_MS");
  process.exit(2);
}

const room = encodeURIComponent(roomId);
const routes = [
  ["messages_history_unread", "GET", `/team-room/${room}/messages?limit=50`],
  ["presence", "GET", `/team-room/${room}/presence`],
  ["heartbeat", "POST", "/team-room/heartbeat", {}],
  ["participants", "GET", `/team-room/${room}/participants`],
  ["tasks", "GET", `/team-room/${room}/tasks`],
  ["decisions", "GET", `/team-room/${room}/decisions`],
  ["attachment_references", "GET", `/team-room/${room}/attachments`],
];
const jobs = [];
for (const [name, method, path] of routes) {
  for (let i = 0; i < events; i++) jobs.push({ name, method, path });
}
for (let i = 0; i < messages; i++) jobs.push({
  name: "message_write",
  method: "POST",
  path: `/team-room/${room}/messages`,
  body: { client_message_id: randomUUID(), body: messageOfBytes(`benchmark message ${i} ${randomUUID()} `, messageBytes), parent_message_id: null },
});

const measurements = new Map();
for (const job of jobs) {
  if (!measurements.has(job.name)) measurements.set(job.name, []);
}
const failures = [];
let cursor = 0;
const started = performance.now();
const rateGate = createRateGate(ratePerSecond);
const deadline = durationSeconds > 0 ? started + durationSeconds * 1000 : Number.POSITIVE_INFINITY;

async function execute(job, index, clientIndex) {
  const begin = performance.now();
  try {
    const response = await fetch(apiUrl + job.path, {
      method: job.method,
      headers: {
        authorization: `Bearer ${tokens[clientIndex % tokens.length]}`,
        "content-type": "application/json",
      },
      body: job.body ? JSON.stringify(job.body) : undefined,
      signal: AbortSignal.timeout(timeoutMs),
    });
    const text = await response.text();
    const elapsed = performance.now() - begin;
    if (!response.ok) failures.push({ endpoint: job.name, status: response.status });
    measurements.get(job.name).push({ ms: elapsed, ok: response.ok });
    if (job.name === "message_write" && response.ok) {
      try { JSON.parse(text); } catch { failures.push({ endpoint: job.name, status: "invalid_json" }); }
    }
  } catch (error) {
    measurements.get(job.name).push({ ms: performance.now() - begin, ok: false });
    failures.push({ endpoint: job.name, status: error?.name || "network_error" });
  }
}

async function worker(clientIndex) {
  while (true) {
    const index = cursor++;
    if (index >= jobs.length || performance.now() >= deadline) return;
    await rateGate();
    if (performance.now() >= deadline) return;
    await execute(jobs[index], index, clientIndex);
  }
}
await Promise.all(Array.from({ length: clients }, (_, clientIndex) => worker(clientIndex)));
const elapsedMs = performance.now() - started;

function percentile(sorted, p) {
  if (!sorted.length) return null;
  return Number(sorted[Math.max(0, Math.ceil(p * sorted.length) - 1)].toFixed(3));
}
const results = {};
for (const [name, samples] of measurements) {
  const sorted = samples.map(x => x.ms).sort((a, b) => a - b);
  const ok = samples.filter(x => x.ok).length;
  results[name] = {
    requests: samples.length,
    success: ok,
    errors: samples.length - ok,
    latency_ms: {
      p50: percentile(sorted, 0.50),
      p95: percentile(sorted, 0.95),
      p99: percentile(sorted, 0.99),
      max: sorted.length ? Number(sorted.at(-1).toFixed(3)) : null,
    },
  };
}
const total = [...measurements.values()].reduce((sum, samples) => sum + samples.length, 0);
const successful = [...measurements.values()].flat().filter(x => x.ok).length;
console.log(JSON.stringify({
  benchmark: "team-room-http",
  generated_at: new Date().toISOString(),
  environment: {
    api_origin: new URL(apiUrl).origin,
    room_id: roomId,
    clients,
    participant_count: participantCount,
    distinct_session_tokens: new Set(tokens).size,
    distinct_participant_per_worker: new Set(tokens).size >= clients,
    dataset: {
      tenants: Number(process.env.BENCH_TENANTS || 0),
      rooms: Number(process.env.BENCH_ROOMS || 0),
      seeded_messages: Number(process.env.BENCH_SEED_MESSAGES || 0),
      seeded_tasks: Number(process.env.BENCH_SEED_TASKS || 0),
      seeded_decisions: Number(process.env.BENCH_SEED_DECISIONS || 0),
      seeded_attachment_references: Number(process.env.BENCH_SEED_ATTACHMENTS || 0),
    },
    events_per_read_endpoint: events,
    message_writes_requested: messages,
    duration_limit_seconds: durationSeconds,
    rate_limit_requests_per_second: ratePerSecond,
    message_size_bytes: messageBytes,
    timeout_ms: timeoutMs,
    data_seeded_by_probe: false,
    fixture_seeded_by_lab: Number(process.env.BENCH_SEED_MESSAGES || 0) > 0,
  },
  summary: {
    requests: total,
    requests_planned: jobs.length,
    success: successful,
    errors: total - successful,
    duration_ms: Number(elapsedMs.toFixed(3)),
    throughput_requests_per_second: Number((total / (elapsedMs / 1000)).toFixed(3)),
    error_rate: total ? Number(((total - successful) / total).toFixed(6)) : 0,
  },
  endpoints: results,
  failure_samples: failures.slice(0, 50),
}, null, 2));
if (total === 0 || successful === 0) process.exitCode = 1;
