#!/usr/bin/env node
/**
 * Real authenticated WebSocket fanout probe for one pre-provisioned room.
 * Requires Node 22 (global WebSocket), a real room-bound bearer token and write/read grants.
 * Opens N sockets, posts M unique messages through the HTTP API, and measures delivery.
 */
import { randomUUID } from "node:crypto";
import { performance } from "node:perf_hooks";
import { benchNumber, benchBoolean, benchValue, createRateGate, messageOfBytes } from "./benchmark-config.mjs";

const base = (process.env.TEAM_ROOM_API_URL || "").replace(/\/$/, "");
const roomId = process.env.TEAM_ROOM_ROOM_ID || "";
const tokenList = process.env.TEAM_ROOM_SESSION_TOKENS_JSON
  ? JSON.parse(process.env.TEAM_ROOM_SESSION_TOKENS_JSON)
  : [process.env.TEAM_ROOM_SESSION_TOKEN || ""];
const tokens = Array.isArray(tokenList) ? tokenList.filter(value => typeof value === "string" && value.length > 0) : [];
const participantCount = benchNumber("BENCH_PARTICIPANTS", tokens.length, { min: 1 });
const requireDistinct = benchBoolean("BENCH_REQUIRE_DISTINCT_PARTICIPANTS", false);
const otherRoomIdsValue = process.env.TEAM_ROOM_OTHER_ROOM_IDS_JSON ? JSON.parse(process.env.TEAM_ROOM_OTHER_ROOM_IDS_JSON) : [];
const otherRoomTokensValue = process.env.TEAM_ROOM_OTHER_ROOM_SESSION_TOKENS_JSON ? JSON.parse(process.env.TEAM_ROOM_OTHER_ROOM_SESSION_TOKENS_JSON) : [];
const otherRoomIds = Array.isArray(otherRoomIdsValue) ? otherRoomIdsValue.filter(value => typeof value === "string" && value.length > 0) : [];
const otherRoomTokens = Array.isArray(otherRoomTokensValue) ? otherRoomTokensValue.filter(value => typeof value === "string" && value.length > 0) : [];
const irrelevantWritesPerRoom = benchNumber("BENCH_IRRELEVANT_WRITES_PER_ROOM", 0);
const clients = benchNumber("BENCH_CLIENTS", 8, { min: 1 });
const messages = benchNumber("BENCH_MESSAGES", 25, { min: 1 });
const settleMs = benchNumber("BENCH_SETTLE_MS", 1500);
const heartbeats = benchNumber("BENCH_HEARTBEATS", 0);
const durationSeconds = benchNumber("BENCH_DURATION_SECONDS", 0, { integer: false });
const ratePerSecond = benchNumber("BENCH_RATE_PER_SECOND", 0, { integer: false });
const messageBytes = benchNumber("BENCH_MESSAGE_BYTES", 256, { min: 1 });
if (!base || !roomId || tokens.length === 0) {
  console.error("Required: TEAM_ROOM_API_URL, TEAM_ROOM_ROOM_ID, and TEAM_ROOM_SESSION_TOKENS_JSON or TEAM_ROOM_SESSION_TOKEN");
  process.exit(2);
}
if (![clients, messages, settleMs, heartbeats, participantCount, irrelevantWritesPerRoom].every(Number.isSafeInteger) || clients < 1 || messages < 1 || settleMs < 0 || heartbeats < 0 || participantCount < 1 || irrelevantWritesPerRoom < 0 || (requireDistinct && new Set(tokens).size < clients) || otherRoomIds.length !== otherRoomTokens.length) {
  console.error("Invalid BENCH_CLIENTS / BENCH_MESSAGES / BENCH_SETTLE_MS");
  process.exit(2);
}
const room = encodeURIComponent(roomId);
const headers = { "content-type": "application/json" };
const sockets = [];
const deliveries = new Map();
const early = new Map();
const sendStarts = new Map();
const duplicates = [];
const failures = [];
let receivedFrames = 0;
let presenceChangedFrames = 0;
let irrelevantRoomFrames = 0;

async function ticket(token) {
  const response = await fetch(`${base}/team-room/${room}/ws-ticket`, { method: "POST", headers: { ...headers, authorization: `Bearer ${token}` }, body: "{}" });
  if (!response.ok) throw new Error(`ticket_http_${response.status}`);
  const value = await response.json();
  if (!value.ticket) throw new Error("ticket_missing");
  return value.ticket;
}
function waitOpen(socket) {
  return new Promise((resolve, reject) => {
    let settled = false;
    const timeout = setTimeout(() => {
      if (!settled) { settled = true; reject(new Error("websocket_open_timeout")); }
    }, 10000);
    socket.addEventListener("open", () => {
      if (!settled) { settled = true; clearTimeout(timeout); resolve(); }
    }, { once: true });
    socket.addEventListener("error", () => {
      if (!settled) { settled = true; clearTimeout(timeout); reject(new Error("websocket_open_error")); }
    }, { once: true });
    socket.addEventListener("close", event => {
      if (!settled) {
        settled = true;
        clearTimeout(timeout);
        reject(new Error("websocket_closed_before_open_" + event.code + "_" + String(event.reason || "").slice(0,80)));
      }
    }, { once: true });
  });
}
const openPromises = [];
for (let i = 0; i < clients; i++) {
  const oneUseTicket = await ticket(tokens[i % tokens.length]);
  const wsUrl = base.replace(/^http/, "ws") + `/team-room/${room}/ws?ticket=${encodeURIComponent(oneUseTicket)}`;
  const socket = new WebSocket(wsUrl, ["team-room.v1"]);
  sockets.push(socket);
  openPromises.push(waitOpen(socket));
  const index = i;
  socket.addEventListener("message", event => {
    receivedFrames++;
    try {
      const frame = JSON.parse(String(event.data));
      const frameRoomId = frame.payload?.room_id || frame.payload?.message?.room_id;
      if (frameRoomId && frameRoomId !== roomId) { irrelevantRoomFrames++; return; }
      if (frame.type === "PRESENCE_CHANGED") { presenceChangedFrames++; return; }
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
  await Promise.all(openPromises);
  // Let each upgraded connection finish opening its dedicated PostgreSQL LISTEN subscription.
  await new Promise(resolve => setTimeout(resolve, 500));
  const writeStart = performance.now();
  const rateGate = createRateGate(ratePerSecond);
  const deadline = durationSeconds > 0 ? writeStart + durationSeconds * 1000 : Number.POSITIVE_INFINITY;
  const scheduleWrite = async () => {
    await rateGate();
    return performance.now() < deadline;
  };
  for (let i = 0; i < messages; i++) {
    if (!(await scheduleWrite())) break;
    const clientMessageId = randomUUID();
    const started = performance.now();
    const response = await fetch(`${base}/team-room/${room}/messages`, {
      method: "POST", headers: { ...headers, authorization: `Bearer ${tokens[i % tokens.length]}` },
      body: JSON.stringify({ client_message_id: clientMessageId, body: messageOfBytes(`fanout benchmark ${i} ${clientMessageId} `, messageBytes), parent_message_id: null }),
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
  let irrelevantRoomWritesAccepted = 0;
  for (let roomIndex = 0; roomIndex < otherRoomIds.length; roomIndex++) {
    for (let messageIndex = 0; messageIndex < irrelevantWritesPerRoom; messageIndex++) {
      if (!(await scheduleWrite())) break;
      const clientMessageId = randomUUID();
      const response = await fetch(base + "/team-room/" + encodeURIComponent(otherRoomIds[roomIndex]) + "/messages", {
        method: "POST",
        headers: { ...headers, authorization: "Bearer " + otherRoomTokens[roomIndex] },
        body: JSON.stringify({ client_message_id: clientMessageId, body: "irrelevant-room event " + roomIndex + "-" + messageIndex + " " + clientMessageId, parent_message_id: null }),
        signal: AbortSignal.timeout(10000),
      });
      if (response.ok) irrelevantRoomWritesAccepted++;
      else failures.push("irrelevant_room_write_http_" + response.status);
    }
  }
  let heartbeatWritesAccepted = 0;
  for (let i = 0; i < heartbeats; i++) {
    if (!(await scheduleWrite())) break;
    const response = await fetch(`${base}/team-room/heartbeat`, { method: "POST", headers: { ...headers, authorization: `Bearer ${tokens[i % tokens.length]}` }, body: "{}", signal: AbortSignal.timeout(10000) });
    if (response.ok) heartbeatWritesAccepted++;
    else failures.push(`heartbeat_http_${response.status}`);
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
    environment: { api_origin: new URL(base).origin, room_id: roomId, clients, participant_count: participantCount, distinct_session_tokens: new Set(tokens).size, distinct_participant_per_socket: new Set(tokens).size >= clients, dataset: { tenants: Number(process.env.BENCH_TENANTS || 0), rooms: Number(process.env.BENCH_ROOMS || 0), seeded_messages: Number(process.env.BENCH_SEED_MESSAGES || 0), seeded_tasks: Number(process.env.BENCH_SEED_TASKS || 0), seeded_decisions: Number(process.env.BENCH_SEED_DECISIONS || 0), seeded_attachment_references: Number(process.env.BENCH_SEED_ATTACHMENTS || 0) }, message_writes_requested: messages, heartbeat_writes_requested: heartbeats, other_room_ids: otherRoomIds, irrelevant_writes_per_room: irrelevantWritesPerRoom, settle_ms: settleMs, duration_limit_seconds: durationSeconds, rate_limit_writes_per_second: ratePerSecond, message_size_bytes: messageBytes },
    summary: {
      websocket_connections_opened: sockets.length,
      message_writes_accepted: sendStarts.size,
      expected_deliveries_same_room: expected,
      observed_deliveries: observed,
      missing_deliveries: expected - observed,
      duplicate_deliveries: duplicates.length,
      received_frames_total: receivedFrames,
      heartbeat_writes_accepted: heartbeatWritesAccepted,
      presence_changed_frames: presenceChangedFrames,
      presence_frames_expected: heartbeatWritesAccepted * clients,
      presence_frames_missing: Math.max(0, heartbeatWritesAccepted * clients - presenceChangedFrames),
      presence_frames_per_accepted_heartbeat: heartbeatWritesAccepted ? Number((presenceChangedFrames / heartbeatWritesAccepted).toFixed(3)) : null,
      irrelevant_room_events_requested: otherRoomIds.length * irrelevantWritesPerRoom,
      irrelevant_room_events_accepted: irrelevantRoomWritesAccepted,
      irrelevant_room_frames_received: irrelevantRoomFrames,
      duration_ms: Number(elapsed.toFixed(3)),
      delivery_latency_ms: { p50: pct(0.5), p95: pct(0.95), p99: pct(0.99) },
      delivery_throughput_per_second: elapsed > 0 ? Number((observed / (elapsed / 1000)).toFixed(3)) : null,
    },
    failures: failures.slice(0, 50),
    duplicate_samples: duplicates.slice(0, 20),
  }, null, 2));
  if (sendStarts.size === 0 || observed === 0 || observed !== expected || duplicates.length > 0 || failures.length || presenceChangedFrames < heartbeatWritesAccepted * clients || irrelevantRoomWritesAccepted !== otherRoomIds.length * irrelevantWritesPerRoom || irrelevantRoomFrames > 0) process.exitCode = 1;
} finally {
  for (const socket of sockets) { try { socket.close(); } catch {} }
}
