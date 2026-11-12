#!/usr/bin/env node
/**
 * Real authenticated WebSocket fanout probe for one pre-provisioned room.
 * Requires Node 22 (global WebSocket), a real room-bound bearer token and write/read grants.
 * Opens N sockets, posts M unique messages through the HTTP API, and measures delivery.
 */
import { randomUUID } from "node:crypto";
import { performance } from "node:perf_hooks";

const base = (process.env.TEAM_ROOM_API_URL || "").replace(/\/$/, "");
const roomId = process.env.TEAM_ROOM_ROOM_ID || "";
const token = process.env.TEAM_ROOM_SESSION_TOKEN || "";
const clients = Number(process.env.BENCH_CLIENTS || 8);
const messages = Number(process.env.BENCH_MESSAGES || 25);
const settleMs = Number(process.env.BENCH_SETTLE_MS || 1500);
if (!base || !roomId || !token) {
  console.error("Required: TEAM_ROOM_API_URL, TEAM_ROOM_ROOM_ID, TEAM_ROOM_SESSION_TOKEN");
  process.exit(2);
}
if (![clients, messages, settleMs].every(Number.isSafeInteger) || clients < 1 || messages < 1 || settleMs < 0) {
  console.error("Invalid BENCH_CLIENTS / BENCH_MESSAGES / BENCH_SETTLE_MS");
  process.exit(2);
}
const room = encodeURIComponent(roomId);
const headers = { authorization: `Bearer ${token}`, "content-type": "application/json" };
const sockets = [];
const deliveries = new Map();
const early = new Map();
const sendStarts = new Map();
const duplicates = [];
const failures = [];
let receivedFrames = 0;

async function ticket() {
  const response = await fetch(`${base}/team-room/${room}/ws-ticket`, { method: "POST", headers, body: "{}" });
  if (!response.ok) throw new Error(`ticket_http_${response.status}`);
  const value = await response.json();
  if (!value.ticket) throw new Error("ticket_missing");
  return value.ticket;
}
function waitOpen(socket) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("websocket_open_timeout")), 10000);
    socket.addEventListener("open", () => { clearTimeout(timeout); resolve(); }, { once: true });
    socket.addEventListener("error", () => { clearTimeout(timeout); reject(new Error("websocket_open_error")); }, { once: true });
  });
}
for (let i = 0; i < clients; i++) {
  const oneUseTicket = await ticket();
  const wsUrl = base.replace(/^http/, "ws") + `/team-room/${room}/ws?ticket=${encodeURIComponent(oneUseTicket)}`;
  const socket = new WebSocket(wsUrl, ["team-room.v1"]);
  sockets.push(socket);
  const index = i;
  socket.addEventListener("message", event => {
    receivedFrames++;
    try {
      const frame = JSON.parse(String(event.data));
      if (frame.type !== "MESSAGE_CREATED") return;
      const messageId = frame.payload?.message?.message_id;
      if (!messageId) return;
      const receivedAt = performance.now();
      if (!early.has(messageId)) early.set(messageId, []);
      early.get(messageId).push({ client: index, at: receivedAt });
      if (sendStarts.has(messageId)) record(messageId, index, receivedAt);
    } catch { failures.push("malformed_websocket_frame"); }
  });
  socket.addEventListener("error", () => failures.push("websocket_runtime_error"));
}
function record(messageId, client, at) {
  if (!deliveries.has(messageId)) deliveries.set(messageId, new Map());
  const perClient = deliveries.get(messageId);
  if (perClient.has(client)) duplicates.push({ message_id: messageId, client });
  else perClient.set(client, at);
}

try {
  await Promise.all(sockets.map(waitOpen));
  const writeStart = performance.now();
  for (let i = 0; i < messages; i++) {
    const clientMessageId = randomUUID();
    const started = performance.now();
    const response = await fetch(`${base}/team-room/${room}/messages`, {
      method: "POST", headers,
      body: JSON.stringify({ client_message_id: clientMessageId, body: `fanout benchmark ${i} ${clientMessageId}`, parent_message_id: null }),
      signal: AbortSignal.timeout(10000),
    });
    if (!response.ok) {
      failures.push(`message_write_http_${response.status}`);
      continue;
    }
    const message = await response.json();
    sendStarts.set(message.message_id, started);
    const prior = early.get(message.message_id) || [];
    for (const delivery of prior) record(message.message_id, delivery.client, delivery.at);
  }
  await new Promise(resolve => setTimeout(resolve, settleMs));
  const elapsed = performance.now() - writeStart;
  const latencies = [];
  let expected = 0, observed = 0;
  for (const [messageId, start] of sendStarts) {
    expected += clients;
    const perClient = deliveries.get(messageId) || new Map();
    observed += perClient.size;
    for (const at of perClient.values()) latencies.push(at - start);
  }
  latencies.sort((a, b) => a - b);
  const pct = p => latencies.length ? Number(latencies[Math.max(0, Math.ceil(p * latencies.length) - 1)].toFixed(3)) : null;
  console.log(JSON.stringify({
    benchmark: "team-room-websocket-fanout",
    generated_at: new Date().toISOString(),
    environment: { api_origin: new URL(base).origin, room_id: roomId, clients, message_writes_requested: messages, settle_ms: settleMs },
    summary: {
      websocket_connections_opened: sockets.length,
      message_writes_accepted: sendStarts.size,
      expected_deliveries_same_room: expected,
      observed_deliveries: observed,
      missing_deliveries: expected - observed,
      duplicate_deliveries: duplicates.length,
      received_frames_total: receivedFrames,
      duration_ms: Number(elapsed.toFixed(3)),
      delivery_latency_ms: { p50: pct(0.5), p95: pct(0.95), p99: pct(0.99) },
      delivery_throughput_per_second: elapsed > 0 ? Number((observed / (elapsed / 1000)).toFixed(3)) : null,
    },
    failures: failures.slice(0, 50),
    duplicate_samples: duplicates.slice(0, 20),
  }, null, 2));
  if (sendStarts.size === 0 || observed === 0 || failures.length) process.exitCode = 1;
} finally {
  for (const socket of sockets) { try { socket.close(); } catch {} }
}
