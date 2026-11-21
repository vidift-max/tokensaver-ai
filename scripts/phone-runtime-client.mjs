/**
 * Fail-closed HTTP client for the AI Phone mailbox.
 * It does not execute missions or call an AI provider; callers must supply
 * an independently authorized executor and ACK only after durable handling.
 */
export class PhoneRuntimeError extends Error {
  constructor(message, status = 0, code = "PHONE_RUNTIME_ERROR") {
    super(message);
    this.name = "PhoneRuntimeError";
    this.status = status;
    this.code = code;
  }
}

export class PhoneRuntimeClient {
  constructor(env = process.env, fetchImpl = globalThis.fetch) {
    this.baseUrl = (env.AI_PHONE_API_URL || "").replace(/\/+$/, "");
    this.sessionToken = env.AI_PHONE_SESSION_TOKEN || "";
    this.endpointCredential = env.AI_PHONE_ENDPOINT_CREDENTIAL || "";
    this.phoneId = env.AI_PHONE_ID || "";
    this.fetch = fetchImpl;
    if (!this.baseUrl || !this.sessionToken || !this.endpointCredential || !this.phoneId) {
      throw new PhoneRuntimeError(
        "AI_PHONE_API_URL, AI_PHONE_SESSION_TOKEN, AI_PHONE_ENDPOINT_CREDENTIAL and AI_PHONE_ID are required",
        0, "CONFIG_REQUIRED"
      );
    }
    let parsed;
    try { parsed = new URL(this.baseUrl); }
    catch { throw new PhoneRuntimeError("AI_PHONE_API_URL must be an absolute URL", 0, "CONFIG_INVALID"); }
    if (!["https:", "http:"].includes(parsed.protocol) ||
        (parsed.protocol === "http:" && !["localhost", "127.0.0.1", "::1"].includes(parsed.hostname))) {
      throw new PhoneRuntimeError("AI Phone API must use HTTPS except loopback development", 0, "INSECURE_API_URL");
    }
    if (this.endpointCredential.length < 40 || this.endpointCredential.length > 128) {
      throw new PhoneRuntimeError("AI_PHONE_ENDPOINT_CREDENTIAL has invalid length", 0, "CONFIG_INVALID");
    }
    if (typeof this.fetch !== "function") {
      throw new PhoneRuntimeError("Fetch implementation unavailable", 0, "CONFIG_INVALID");
    }
  }

  async request(method, route, body) {
    let response;
    try {
      response = await this.fetch(this.baseUrl + route, {
        method,
        redirect: "error",
        headers: {
          authorization: "Bearer " + this.sessionToken,
          "x-ai-phone-endpoint-credential": this.endpointCredential,
          accept: "application/json",
          ...(body === undefined ? {} : { "content-type": "application/json" })
        },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
        signal: AbortSignal.timeout(10000)
      });
    } catch {
      throw new PhoneRuntimeError("AI Phone request failed; processing must stop", 0, "NETWORK_FAILURE");
    }
    const raw = await response.text();
    let payload = null;
    if (raw) {
      try { payload = JSON.parse(raw); }
      catch { throw new PhoneRuntimeError("AI Phone returned invalid JSON", response.status, "INVALID_RESPONSE"); }
    }
    if (!response.ok) {
      const code = response.status === 401 || response.status === 403 || response.status === 412
        ? "AUTHORIZATION_LOST" : "HTTP_ERROR";
      throw new PhoneRuntimeError(`AI Phone rejected ${method} ${route} (HTTP ${response.status})`, response.status, code);
    }
    return payload;
  }

  async poll() {
    const messages = await this.request("GET", `/ai-phone/${encodeURIComponent(this.phoneId)}/messages`);
    if (!Array.isArray(messages)) {
      throw new PhoneRuntimeError("AI Phone mailbox response must be an array", 502, "INVALID_RESPONSE");
    }
    return messages;
  }

  async acknowledge(messageId) {
    if (typeof messageId !== "string" || !/^[0-9a-f-]{36}$/i.test(messageId)) {
      throw new PhoneRuntimeError("messageId must be a UUID", 0, "INVALID_ARGUMENT");
    }
    await this.request("POST",
      `/ai-phone/${encodeURIComponent(this.phoneId)}/messages/${encodeURIComponent(messageId)}/ack`);
  }

  async send(recipientPhoneId, idempotencyKey, content) {
    if (typeof recipientPhoneId !== "string" || !/^[0-9a-f-]{36}$/i.test(recipientPhoneId) ||
        typeof idempotencyKey !== "string" || !idempotencyKey.trim() || idempotencyKey.length > 200 ||
        content === undefined) {
      throw new PhoneRuntimeError("recipientPhoneId, idempotencyKey and content are required", 0, "INVALID_ARGUMENT");
    }
    return this.request("POST", `/ai-phone/${encodeURIComponent(recipientPhoneId)}/messages`, {
      idempotency_key: idempotencyKey,
      content
    });
  }
}
