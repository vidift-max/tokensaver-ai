import test from "node:test";
import assert from "node:assert/strict";
import { PhoneRuntimeClient, PhoneRuntimeError } from "../scripts/phone-runtime-client.mjs";

const phoneId = "10000000-0000-4000-8000-000000000001";
const messageId = "20000000-0000-4000-8000-000000000002";
const config = {
  AI_PHONE_API_URL: "https://phone.example.test",
  AI_PHONE_SESSION_TOKEN: "session-secret",
  AI_PHONE_ENDPOINT_CREDENTIAL: "x".repeat(48),
  AI_PHONE_ID: phoneId
};
function response(status, payload) {
  return { ok: status >= 200 && status < 300, status, text: async () => payload === null ? "" : JSON.stringify(payload) };
}

test("requires explicit scoped runtime configuration and rejects insecure remote HTTP", () => {
  assert.throws(() => new PhoneRuntimeClient({}), e => e instanceof PhoneRuntimeError && e.code === "CONFIG_REQUIRED");
  assert.throws(() => new PhoneRuntimeClient({...config, AI_PHONE_API_URL:"http://phone.example.test"}), e => e.code === "INSECURE_API_URL");
});

test("polls the configured phone mailbox with session and endpoint credential", async () => {
  let captured;
  const client = new PhoneRuntimeClient(config, async (url, init) => {
    captured = {url, init};
    return response(200, [{message_id:messageId,message_state:"DELIVERED"}]);
  });
  const messages = await client.poll();
  assert.equal(messages.length, 1);
  assert.equal(captured.url, `https://phone.example.test/ai-phone/${phoneId}/messages`);
  assert.equal(captured.init.redirect, "error");
  assert.equal(captured.init.headers.authorization, "Bearer session-secret");
  assert.equal(captured.init.headers["x-ai-phone-endpoint-credential"], config.AI_PHONE_ENDPOINT_CREDENTIAL);
});

test("ACKs only an explicitly supplied UUID and treats authorization loss as terminal", async () => {
  let called = false;
  const client = new PhoneRuntimeClient(config, async () => { called = true; return response(204, null); });
  await assert.rejects(() => client.acknowledge("not-a-uuid"), e => e.code === "INVALID_ARGUMENT");
  assert.equal(called, false);
  const denied = new PhoneRuntimeClient(config, async () => response(403, {error:"forbidden"}));
  await assert.rejects(() => denied.poll(), e => e.code === "AUTHORIZATION_LOST" && e.status === 403);
});

test("sends idempotent JSON messages without inventing an executor response", async () => {
  let captured;
  const client = new PhoneRuntimeClient(config, async (url, init) => {
    captured = {url, init};
    return response(201, {message_id:messageId,message_state:"PENDING"});
  });
  const result = await client.send(phoneId, "mission-42-attempt-1", {kind:"mission",text:"Execute authorized work"});
  assert.equal(result.message_id, messageId);
  assert.equal(captured.init.method, "POST");
  assert.deepEqual(JSON.parse(captured.init.body), {
    idempotency_key:"mission-42-attempt-1",
    content:{kind:"mission",text:"Execute authorized work"}
  });
});
