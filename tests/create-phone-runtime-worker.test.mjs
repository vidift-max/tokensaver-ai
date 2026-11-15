import test from "node:test";
import assert from "node:assert/strict";
import { createPhoneRuntimeWorker } from "../scripts/create-phone-runtime-worker.mjs";

test("runtime bootstrap refuses to invent or omit an AI executor", async () => {
  await assert.rejects(
    () => createPhoneRuntimeWorker({ env: {} }),
    error => error.code === "EXECUTOR_REQUIRED"
  );
});

test("runtime bootstrap fails closed when Phone or Team Room credentials are absent", async () => {
  const executor = { execute: async () => ({}) };
  await assert.rejects(
    () => createPhoneRuntimeWorker({
      env: { AI_PHONE_RESULT_DIRECTORY: "phone-bootstrap-test-results" },
      executor
    }),
    error => error.code === "CONFIG_REQUIRED" || error.code === "AUTHORIZER_CONFIG_REQUIRED"
  );
});

test("runtime bootstrap constructs Team Room from the supplied environment", async () => {
  const env = {
    AI_PHONE_API_URL: "https://phone.example.test",
    AI_PHONE_SESSION_TOKEN: "phone-session-token",
    AI_PHONE_ENDPOINT_CREDENTIAL: "x".repeat(48),
    AI_PHONE_ID: "10000000-0000-4000-8000-000000000001",
    AI_PHONE_RESULT_DIRECTORY: "/tmp/tokensaver-phone-bootstrap-test",
    TEAM_ROOM_API_URL: "https://team-room.example.test",
    TEAM_ROOM_SESSION_TOKEN: "team-room-session-token",
    TEAM_ROOM_ROOM_ID: "20000000-0000-4000-8000-000000000002",
    TEAM_ROOM_AUTHORIZED_SENDER_PRINCIPAL_ID: "30000000-0000-4000-8000-000000000003"
  };
  const resultStore = { initialize: async () => {}, get: async () => null, commit: async () => ({}) };
  const worker = await createPhoneRuntimeWorker({
    env,
    executor: { execute: async () => ({}) },
    resultStore
  });

  assert.equal(worker.authorizer.teamRoom.baseUrl, env.TEAM_ROOM_API_URL);
  assert.equal(worker.authorizer.teamRoom.token, env.TEAM_ROOM_SESSION_TOKEN);
  assert.equal(worker.authorizer.teamRoom.roomId, env.TEAM_ROOM_ROOM_ID);
});

test("runtime bootstrap refuses to start without an injected encrypted result store", async () => {
  const env = {
    AI_PHONE_API_URL: "https://phone.example.test",
    AI_PHONE_SESSION_TOKEN: "phone-session-token",
    AI_PHONE_ENDPOINT_CREDENTIAL: "x".repeat(48),
    AI_PHONE_ID: "10000000-0000-4000-8000-000000000001",
    TEAM_ROOM_API_URL: "https://team-room.example.test",
    TEAM_ROOM_SESSION_TOKEN: "team-room-session-token",
    TEAM_ROOM_ROOM_ID: "20000000-0000-4000-8000-000000000002",
    TEAM_ROOM_AUTHORIZED_SENDER_PRINCIPAL_ID: "30000000-0000-4000-8000-000000000003"
  };
  await assert.rejects(
    () => createPhoneRuntimeWorker({ env, executor: { execute: async () => ({}) } }),
    error => error.code === "ENCRYPTED_RESULT_STORE_REQUIRED"
  );
});
