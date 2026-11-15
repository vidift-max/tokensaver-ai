/**
 * Strict, versioned envelope validation for Phone-02 mission messages.
 * This validates structure and resource bounds only; it does NOT grant authority.
 * A server-backed authorizer must approve the sender, task owner and live fencing
 * token before the worker may inspect stored results or invoke an executor.
 */
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const MAX_ENVELOPE_BYTES = 64 * 1024;
const MAX_DEPTH = 12;
const MAX_ARRAY_ITEMS = 256;
const MAX_OBJECT_KEYS = 128;
const MAX_STRING_LENGTH = 16 * 1024;
const FORBIDDEN_KEYS = new Set(["__proto__", "prototype", "constructor"]);

export class PhoneMissionEnvelopeError extends Error {
  constructor(message, code = "INVALID_MISSION_ENVELOPE") {
    super(message);
    this.name = "PhoneMissionEnvelopeError";
    this.code = code;
  }
}

function inspectValue(value, depth = 0) {
  if (depth > MAX_DEPTH) throw new PhoneMissionEnvelopeError("Mission payload exceeds maximum nesting depth", "PAYLOAD_TOO_DEEP");
  if (typeof value === "string") {
    if (value.length > MAX_STRING_LENGTH) throw new PhoneMissionEnvelopeError("Mission string exceeds maximum length", "PAYLOAD_STRING_TOO_LARGE");
    return;
  }
  if (value === null || typeof value === "boolean") return;
  if (typeof value === "number") {
    if (!Number.isFinite(value)) throw new PhoneMissionEnvelopeError("Mission numbers must be finite", "INVALID_PAYLOAD_NUMBER");
    return;
  }
  if (Array.isArray(value)) {
    if (value.length > MAX_ARRAY_ITEMS) throw new PhoneMissionEnvelopeError("Mission array exceeds maximum item count", "PAYLOAD_ARRAY_TOO_LARGE");
    for (const item of value) inspectValue(item, depth + 1);
    return;
  }
  if (typeof value === "object") {
    const keys = Object.keys(value);
    if (keys.length > MAX_OBJECT_KEYS) throw new PhoneMissionEnvelopeError("Mission object exceeds maximum property count", "PAYLOAD_OBJECT_TOO_LARGE");
    for (const key of keys) {
      if (FORBIDDEN_KEYS.has(key)) throw new PhoneMissionEnvelopeError("Mission payload contains a forbidden property name", "FORBIDDEN_PAYLOAD_KEY");
      inspectValue(value[key], depth + 1);
    }
    return;
  }
  throw new PhoneMissionEnvelopeError("Mission payload contains an unsupported value", "INVALID_PAYLOAD_VALUE");
}

/**
 * Expected message.content:
 * { version: 1, kind: "mission", mission_id: UUID, task_id: UUID, payload: object }
 * The mailbox message_id is the durable idempotency key; it is not a mission ID.
 */
export function validatePhoneMissionEnvelope(message) {
  if (!message || typeof message !== "object" || Array.isArray(message) ||
      typeof message.message_id !== "string" || !UUID.test(message.message_id) ||
      !["PENDING", "DELIVERED"].includes(message.message_state)) {
    throw new PhoneMissionEnvelopeError("Malformed or non-processable mailbox message", "INVALID_MESSAGE");
  }
  const content = message.content;
  if (!content || typeof content !== "object" || Array.isArray(content) ||
      content.version !== 1 || content.kind !== "mission" ||
      typeof content.mission_id !== "string" || !UUID.test(content.mission_id) ||
      typeof content.task_id !== "string" || !UUID.test(content.task_id) ||
      !content.payload || typeof content.payload !== "object" || Array.isArray(content.payload)) {
    throw new PhoneMissionEnvelopeError("Mission envelope does not match the supported v1 schema", "INVALID_MISSION_ENVELOPE");
  }
  let serialized;
  try { serialized = JSON.stringify(content); }
  catch { throw new PhoneMissionEnvelopeError("Mission envelope is not JSON-serializable", "INVALID_MISSION_ENVELOPE"); }
  if (Buffer.byteLength(serialized, "utf8") > MAX_ENVELOPE_BYTES) {
    throw new PhoneMissionEnvelopeError("Mission envelope exceeds 64 KiB", "ENVELOPE_TOO_LARGE");
  }
  inspectValue(content.payload);
  return content;
}
