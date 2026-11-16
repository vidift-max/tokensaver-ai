import { randomBytes, randomUUID } from "node:crypto";

const SERVICE = "com.tokensaver.ai.phone-02.result-store";
const CURRENT_KEY_ACCOUNT = "current-key-id";
const KEY_ACCOUNT_PREFIX = "aes-256-gcm-key:";
const KEY_ID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function isMissingEntry(error) {
  return error?.code === "NoEntry" || error?.name === "NoEntry" ||
    /(?:^|\\b)NoEntry(?:\\b|$)/i.test(String(error?.message ?? ""));
}

function decodeKey(value, keyId) {
  if (typeof value !== "string" || !/^[A-Za-z0-9+/]{43}=$/.test(value)) {
    throw new Error("OS keyring contains an invalid key record: " + keyId);
  }
  const key = Buffer.from(value, "base64");
  if (key.length !== 32 || key.toString("base64") !== value) {
    throw new Error("OS keyring contains an invalid AES-256 key: " + keyId);
  }
  return key;
}

/**
 * Persistent Phone-02 key provider backed by the current user's OS credential
 * store (Windows Credential Manager/DPAPI, macOS Keychain, or Linux Secret
 * Service). Linux is pinned to Secret Service: no volatile keyutils fallback.
 *
 * The package is loaded lazily so unit tests can inject a deterministic Entry
 * implementation. No key material is written to files, JSON, env vars or logs.
 */
export class OsKeyringPhoneResultKeyProvider {
  constructor({ Entry, platform = process.platform, service = SERVICE } = {}) {
    if (typeof Entry !== "function") throw new TypeError("An OS keyring Entry implementation is required");
    if (!["win32", "darwin", "linux"].includes(platform)) {
      throw new Error("No supported persistent OS credential store for platform: " + platform);
    }
    this.Entry = Entry;
    this.platform = platform;
    this.service = service;
    this.initialization = Promise.resolve();
  }

  static async create(options = {}) {
    const { Entry } = await import("@napi-rs/keyring");
    return new OsKeyringPhoneResultKeyProvider({ ...options, Entry });
  }

  entry(account) {
    const options = this.platform === "linux" ? { linux: { store: "secret-service" } } : undefined;
    return options
      ? new this.Entry(this.service, account, options)
      : new this.Entry(this.service, account);
  }

  read(account) {
    try {
      return this.entry(account).getPassword();
    } catch (error) {
      if (isMissingEntry(error)) return null;
      throw new Error("OS credential store read failed", { cause: error });
    }
  }

  write(account, value) {
    try {
      this.entry(account).setPassword(value);
    } catch (error) {
      throw new Error("OS credential store write failed", { cause: error });
    }
  }

  async getCurrentKey() {
    let keyId = this.read(CURRENT_KEY_ACCOUNT);
    if (keyId === null) {
      // Serialize first-run initialization within this provider instance.
      const initialized = this.initialization.then(() => {
        const existing = this.read(CURRENT_KEY_ACCOUNT);
        if (existing !== null) return existing;
        const createdId = randomUUID();
        const key = randomBytes(32);
        this.write(KEY_ACCOUNT_PREFIX + createdId, key.toString("base64"));
        this.write(CURRENT_KEY_ACCOUNT, createdId);
        return createdId;
      });
      this.initialization = initialized.then(() => undefined, () => undefined);
      keyId = await initialized;
    }
    if (typeof keyId !== "string" || !KEY_ID_PATTERN.test(keyId)) {
      throw new Error("OS credential store contains an invalid current key identifier");
    }
    return { keyId, key: decodeKey(this.read(KEY_ACCOUNT_PREFIX + keyId), keyId) };
  }

  async getKey(keyId) {
    if (typeof keyId !== "string" || !KEY_ID_PATTERN.test(keyId)) {
      throw new TypeError("Invalid Phone-02 key identifier");
    }
    const stored = this.read(KEY_ACCOUNT_PREFIX + keyId);
    if (stored === null) throw new Error("Phone-02 encryption key is missing from the OS credential store: " + keyId);
    return decodeKey(stored, keyId);
  }

  async rotateKey() {
    const keyId = randomUUID();
    const key = randomBytes(32);
    this.write(KEY_ACCOUNT_PREFIX + keyId, key.toString("base64"));
    // Publish the new current key only after its key material is stored.
    this.write(CURRENT_KEY_ACCOUNT, keyId);
    return { keyId, key };
  }
}
