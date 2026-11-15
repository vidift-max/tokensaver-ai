import test from "node:test";
import assert from "node:assert/strict";
import { OsKeyringPhoneResultKeyProvider } from "../scripts/phone-result-key-provider.mjs";
import { createOsBackedPhoneResultStore } from "../scripts/create-os-backed-phone-result-store.mjs";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

class MemoryEntry {
  static values = new Map();
  constructor(service, account, options) {
    this.id = service + ":" + account;
    this.options = options;
  }
  getPassword() {
    if (!MemoryEntry.values.has(this.id)) {
      const error = new Error("NoEntry");
      error.code = "NoEntry";
      throw error;
    }
    return MemoryEntry.values.get(this.id);
  }
  setPassword(value) { MemoryEntry.values.set(this.id, value); }
}

test.beforeEach(() => MemoryEntry.values.clear());

test("creates a random AES-256 key in the OS credential store and reuses it after restart", async () => {
  const first = new OsKeyringPhoneResultKeyProvider({ Entry: MemoryEntry, platform: "win32" });
  const created = await first.getCurrentKey();
  assert.equal(created.key.length, 32);
  assert.match(created.keyId, /^[0-9a-f-]{36}$/i);
  assert.equal(MemoryEntry.values.has("com.tokensaver.ai.phone-02.result-store:aes-256-gcm-key:" + created.keyId), true);

  const restarted = new OsKeyringPhoneResultKeyProvider({ Entry: MemoryEntry, platform: "win32" });
  const reopened = await restarted.getCurrentKey();
  assert.equal(reopened.keyId, created.keyId);
  assert.deepEqual(reopened.key, created.key);
  assert.deepEqual(await restarted.getKey(created.keyId), created.key);
});

test("pins Linux to the persistent Secret Service backend", async () => {
  const seen = [];
  class LinuxEntry extends MemoryEntry {
    constructor(service, account, options) {
      super(service, account, options);
      seen.push(options);
    }
  }
  const provider = new OsKeyringPhoneResultKeyProvider({ Entry: LinuxEntry, platform: "linux" });
  await provider.getCurrentKey();
  assert.ok(seen.length > 0);
  assert.ok(seen.every(option => option?.linux?.store === "secret-service"));
});

test("rotates keys without deleting old keys needed to decrypt prior records", async () => {
  const provider = new OsKeyringPhoneResultKeyProvider({ Entry: MemoryEntry, platform: "darwin" });
  const oldKey = await provider.getCurrentKey();
  const newKey = await provider.rotateKey();
  const current = await provider.getCurrentKey();
  assert.notEqual(newKey.keyId, oldKey.keyId);
  assert.equal(current.keyId, newKey.keyId);
  assert.deepEqual(await provider.getKey(oldKey.keyId), oldKey.key);
  assert.deepEqual(await provider.getKey(newKey.keyId), newKey.key);
});

test("fails closed for unsupported platforms and missing/corrupt key material", async () => {
  assert.throws(
    () => new OsKeyringPhoneResultKeyProvider({ Entry: MemoryEntry, platform: "android" }),
    /No supported persistent OS credential store/
  );
  const provider = new OsKeyringPhoneResultKeyProvider({ Entry: MemoryEntry, platform: "win32" });
  await assert.rejects(() => provider.getKey("00000000-0000-4000-8000-000000000000"), /key is missing/);
});

test("does not hide credential-store backend failures as missing entries", async () => {
  class BrokenEntry {
    getPassword() { throw new Error("backend locked"); }
    setPassword() { throw new Error("backend locked"); }
  }
  const provider = new OsKeyringPhoneResultKeyProvider({ Entry: BrokenEntry, platform: "win32" });
  await assert.rejects(() => provider.getCurrentKey(), /OS credential store read failed/);
});

test("factory connects OS keyring keys to the AES-GCM result store", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "tokensaver-os-keyring-"));
  try {
    const store = await createOsBackedPhoneResultStore(root, { Entry: MemoryEntry, platform: "win32" });
    const messageId = "70000000-0000-4000-8000-000000000007";
    await store.commit(messageId, { result: "persisted securely" });
    const reopened = await createOsBackedPhoneResultStore(root, { Entry: MemoryEntry, platform: "win32" });
    assert.deepEqual((await reopened.get(messageId)).result, { result: "persisted securely" });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});


test("rejects malformed current-key identifiers and corrupted key material", async () => {
  const provider = new OsKeyringPhoneResultKeyProvider({ Entry: MemoryEntry, platform: "win32" });
  MemoryEntry.values.set("com.tokensaver.ai.phone-02.result-store:current-key-id", "not-a-uuid");
  await assert.rejects(() => provider.getCurrentKey(), /invalid current key identifier/);

  MemoryEntry.values.clear();
  const keyId = "70000000-0000-4000-8000-000000000007";
  MemoryEntry.values.set("com.tokensaver.ai.phone-02.result-store:current-key-id", keyId);
  MemoryEntry.values.set("com.tokensaver.ai.phone-02.result-store:aes-256-gcm-key:" + keyId, "not-base64!");
  await assert.rejects(() => provider.getCurrentKey(), /invalid key record/);
});

test("serializes concurrent first initialization within one provider instance", async () => {
  const provider = new OsKeyringPhoneResultKeyProvider({ Entry: MemoryEntry, platform: "win32" });
  const [first, second, third] = await Promise.all([
    provider.getCurrentKey(), provider.getCurrentKey(), provider.getCurrentKey()
  ]);
  assert.equal(first.keyId, second.keyId);
  assert.equal(second.keyId, third.keyId);
  assert.deepEqual(first.key, second.key);
});


test("concurrent rotations retain every generated key and publish one valid current key", async () => {
  const provider = new OsKeyringPhoneResultKeyProvider({ Entry: MemoryEntry, platform: "win32" });
  await provider.getCurrentKey();
  const rotated = await Promise.all([
    provider.rotateKey(),
    provider.rotateKey(),
    provider.rotateKey()
  ]);
  const current = await provider.getCurrentKey();
  assert.ok(rotated.some(item => item.keyId === current.keyId));
  for (const item of rotated) {
    assert.deepEqual(await provider.getKey(item.keyId), item.key);
  }
});
