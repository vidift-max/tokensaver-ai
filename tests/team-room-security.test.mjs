import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { TeamRoomClient } from "../scripts/team-room-client.mjs";
import { normalizeTeamRoomBaseUrl, resolveTeamRoomRequestUrl } from "../scripts/team-room-security.mjs";

const validEnv = (url = "https://team-room.example.test") => ({
  TEAM_ROOM_API_URL: url,
  TEAM_ROOM_SESSION_TOKEN: "synthetic-session-token",
  TEAM_ROOM_ROOM_ID: "10000000-0000-4000-8000-000000000001"
});

test("accepts HTTPS origins and explicit loopback-only HTTP development", () => {
  assert.equal(normalizeTeamRoomBaseUrl("https://team-room.example.test"), "https://team-room.example.test");
  assert.equal(normalizeTeamRoomBaseUrl("https://team-room.example.test/"), "https://team-room.example.test");
  assert.equal(normalizeTeamRoomBaseUrl("http://127.0.0.1:8081"), "http://127.0.0.1:8081");
  assert.equal(normalizeTeamRoomBaseUrl("http://localhost:8081"), "http://localhost:8081");
  assert.equal(normalizeTeamRoomBaseUrl("http://[::1]:8081"), "http://[::1]:8081");
});

test("rejects insecure or ambiguous Team Room origins", () => {
  for (const value of [
    "",
    "not-a-url",
    "http://team-room.example.test",
    "http://192.168.1.20:8081",
    "http://localhost.evil.test",
    "https://user:password@team-room.example.test",
    "https://team-room.example.test/path",
    "https://team-room.example.test?next=https://evil.test",
    "https://team-room.example.test#fragment",
    "https://team-room.example.test?",
    "https://team-room.example.test#"
  ]) {
    assert.throws(() => normalizeTeamRoomBaseUrl(value), undefined, value);
  }
});

test("request routes cannot escape the configured origin or traverse paths", () => {
  const base = "https://team-room.example.test";
  assert.equal(resolveTeamRoomRequestUrl(base, "/auth/me"), "https://team-room.example.test/auth/me");
  for (const route of ["//evil.test/steal", "https://evil.test/steal", "/../steal", "/%2e%2e/steal", "/auth/me?x=1", "/auth/me#x", "/\\evil.test"]) {
    assert.throws(() => resolveTeamRoomRequestUrl(base, route), undefined, route);
  }
});

test("Team Room client rejects unsafe URL configuration before making requests", () => {
  for (const url of ["http://team-room.example.test", "https://user@team-room.example.test", "https://team-room.example.test/path"]) {
    assert.throws(() => new TeamRoomClient(validEnv(url)), error => error.code === "CONFIG_INVALID");
  }
});

test("Team Room requests reject redirects and keep the bearer on the configured origin", async () => {
  const client = new TeamRoomClient(validEnv());
  const originalFetch = globalThis.fetch;
  let observed;
  globalThis.fetch = async (url, options) => {
    observed = { url: String(url), options };
    return new Response(JSON.stringify({ session_id: "s" }), { status: 200, headers: { "content-type": "application/json" } });
  };
  try {
    await client.request("GET", "/auth/me");
    assert.equal(observed.url, "https://team-room.example.test/auth/me");
    assert.equal(observed.options.redirect, "error");
    assert.equal(observed.options.headers.authorization, "Bearer synthetic-session-token");
    await assert.rejects(client.request("GET", "//attacker.test/collect"), error => error.code === "CONFIG_INVALID");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("401 revokes the in-memory client token and requires fresh authentication", async () => {
  const client = new TeamRoomClient(validEnv());
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => new Response("{}", { status: 401 });
  try {
    await assert.rejects(client.request("GET", "/auth/me"), error => error.code === "SESSION_INVALID");
    assert.equal(client.token, "");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("Recovery Worker rejects remote HTTP configuration without printing the session token", () => {
  const worker = fileURLToPath(new URL("../scripts/team-room-recovery-worker.mjs", import.meta.url));
  const result = spawnSync(process.execPath, [worker, "room-id", "task-id", "previous-owner"], {
    encoding: "utf8",
    env: {
      ...process.env,
      TEAM_ROOM_API_URL: "http://api.example.test",
      TEAM_ROOM_SESSION_TOKEN: "synthetic-recovery-token"
    }
  });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /TEAM_ROOM_API_URL is invalid or insecure/);
  assert.doesNotMatch(result.stderr, /synthetic-recovery-token/);
});

test("desktop Team Room UI fails closed and contains no manual endpoint or bearer input", () => {
  const source = fs.readFileSync(new URL("../src/TeamRoom.tsx", import.meta.url), "utf8");
  assert.match(source, /acesso desktop suspenso/);
  assert.doesNotMatch(source, /fetch\s*\(|new WebSocket|Authorization|Bearer|apiUrl|setToken|http:\/\//i);
});
