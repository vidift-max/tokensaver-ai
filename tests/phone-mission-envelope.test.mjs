import test from "node:test";
import assert from "node:assert/strict";
import { validatePhoneMissionEnvelope, PhoneMissionEnvelopeError } from "../scripts/phone-mission-envelope.mjs";

const valid = () => ({
  message_id: "20000000-0000-4000-8000-000000000002",
  message_state: "DELIVERED",
  content: {
    version: 1,
    kind: "mission",
    mission_id: "30000000-0000-4000-8000-000000000003",
    task_id: "40000000-0000-4000-8000-000000000004",
    payload: { action: "analyze", input: { text: "safe test input" } }
  }
});

test("accepts a valid versioned mission envelope without granting authority", () => {
  assert.equal(validatePhoneMissionEnvelope(valid()).version, 1);
});

test("rejects unsupported versions, wrong kinds and invalid identifiers", () => {
  for (const mutate of [
    m => { m.content.version = 2; },
    m => { m.content.kind = "shell"; },
    m => { m.content.task_id = "not-a-uuid"; },
    m => { m.message_state = "ACKNOWLEDGED"; }
  ]) {
    const message = valid();
    mutate(message);
    assert.throws(() => validatePhoneMissionEnvelope(message), PhoneMissionEnvelopeError);
  }
});

test("rejects oversized envelopes, excessive nesting and oversized collections", () => {
  const oversized = valid();
  oversized.content.payload.text = "x".repeat(70 * 1024);
  assert.throws(() => validatePhoneMissionEnvelope(oversized), e => e.code === "ENVELOPE_TOO_LARGE");

  const deep = valid();
  let cursor = deep.content.payload;
  for (let i = 0; i < 14; i++) { cursor.child = {}; cursor = cursor.child; }
  assert.throws(() => validatePhoneMissionEnvelope(deep), e => e.code === "PAYLOAD_TOO_DEEP");

  const wide = valid();
  wide.content.payload.items = Array(257).fill("x");
  assert.throws(() => validatePhoneMissionEnvelope(wide), e => e.code === "PAYLOAD_ARRAY_TOO_LARGE");
});

test("rejects non-finite numbers and prototype-sensitive keys", () => {
  const badNumber = valid();
  badNumber.content.payload.value = Infinity;
  assert.throws(() => validatePhoneMissionEnvelope(badNumber), e => e.code === "INVALID_PAYLOAD_NUMBER");

  const badKey = valid();
  Object.defineProperty(badKey.content.payload, "__proto__", { value: "blocked", enumerable: true });
  assert.throws(() => validatePhoneMissionEnvelope(badKey), e => e.code === "FORBIDDEN_PAYLOAD_KEY");
});
