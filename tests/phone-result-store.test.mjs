import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, readdir, rm, stat, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { FilePhoneResultStore } from "../scripts/phone-result-store.mjs";
import { PhoneRuntimeWorker } from "../scripts/phone-runtime-worker.mjs";

const id = "30000000-0000-4000-8000-000000000003";
const keyA = Buffer.alloc(32, 0x41);
const keyB = Buffer.alloc(32, 0x42);

function makeKeyProvider(keys = { "key-a": keyA, "key-b": keyB }) {
  let currentKeyId = Object.keys(keys)[0];
  return {
    setCurrentKey(keyId) { currentKeyId = keyId; },
    async getCurrentKey() {
      return { keyId: currentKeyId, key: keys[currentKeyId] };
    },
    async getKey(keyId) { return keys[keyId]; }
  };
}

async function withStore(run) {
  const root = await mkdtemp(path.join(os.tmpdir(), "tokensaver-phone-store-"));
  try {
    const keyProvider = makeKeyProvider();
    const directory = path.join(root, "private-results");
    const store = new FilePhoneResultStore(directory, keyProvider);
    await run(store, directory, keyProvider);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("commits an authenticated encrypted result and reads it after reopening", async () => {
  await withStore(async (store, directory, keyProvider) => {
    const committed = await store.commit(id, { output: "done" });
    assert.equal(committed.durable, true);
    assert.deepEqual((await new FilePhoneResultStore(directory, keyProvider).get(id)).result, { output: "done" });
    const files = await readdir(directory);
    assert.deepEqual(files, [id + ".json"]);
    const raw = await readFile(path.join(directory, id + ".json"), "utf8");
    assert.equal(raw.includes("done"), false);
    const record = JSON.parse(raw);
    assert.equal(record.encrypted.version, 2);
    assert.equal(record.encrypted.algorithm, "A256GCM");
    assert.equal(typeof record.encrypted.authTag, "string");
  });
});

test("first committed result wins and cannot be overwritten", async () => {
  await withStore(async store => {
    await store.commit(id, { version: 1 });
    const second = await store.commit(id, { version: 2 });
    assert.deepEqual(second.result, { version: 1 });
  });
});

test("concurrent commits publish exactly one complete encrypted record", async () => {
  await withStore(async store => {
    const results = await Promise.all([
      store.commit(id, { writer: "a" }),
      store.commit(id, { writer: "b" })
    ]);
    assert.equal(results.every(x => x.durable === true && x.messageId === id), true);
    const final = await store.get(id);
    assert.ok(["a", "b"].includes(final.result.writer));
  });
});

test("rejects malformed IDs and detects corrupt or plaintext records", async () => {
  await withStore(async (store, directory) => {
    await assert.rejects(() => store.get("../escape"), /Invalid message UUID/);
    await store.initialize();
    const file = path.join(directory, id + ".json");
    await writeFile(file, JSON.stringify({ version: 1, messageId: id, result: { secret: "plaintext" } }));
    await assert.rejects(() => store.get(id), /plaintext|Corrupt/);
  });
});

test("rejects ciphertext tampering and message-ID substitution", async () => {
  await withStore(async (store, directory) => {
    await store.commit(id, { output: "authenticated" });
    const file = path.join(directory, id + ".json");
    const record = JSON.parse(await readFile(file, "utf8"));
    const originalEncrypted = { ...record.encrypted };
    await assert.rejects(
      () => store.decrypt("40000000-0000-4000-8000-000000000004", originalEncrypted),
      /authentication failed/
    );
    const bytes = Buffer.from(record.encrypted.ciphertext, "base64");
    bytes[0] ^= 0x01;
    record.encrypted.ciphertext = bytes.toString("base64");
    await writeFile(file, JSON.stringify(record));
    await assert.rejects(() => store.get(id), /authentication failed/);
  });
});

test("supports key rotation while retaining access to records encrypted with old keys", async () => {
  await withStore(async (store, directory, keyProvider) => {
    await store.commit(id, { generation: "old-key" });
    keyProvider.setCurrentKey("key-b");
    const nextId = "40000000-0000-4000-8000-000000000004";
    await store.commit(nextId, { generation: "new-key" });
    const reopened = new FilePhoneResultStore(directory, keyProvider);
    assert.deepEqual((await reopened.get(id)).result, { generation: "old-key" });
    assert.deepEqual((await reopened.get(nextId)).result, { generation: "new-key" });
  });
});

test("fails closed when the key provider or referenced key is unavailable", async () => {
  await assert.rejects(
    async () => new FilePhoneResultStore("/tmp/not-created", undefined),
    error => error.code === "RESULT_KEY_PROVIDER_REQUIRED"
  );
  await withStore(async (store, directory) => {
    await store.commit(id, { protected: true });
    const unavailable = {
      getCurrentKey: async () => ({ keyId: "missing", key: keyA }),
      getKey: async () => undefined
    };
    await assert.rejects(() => new FilePhoneResultStore(directory, unavailable).get(id), /32 bytes of key material/);
  });
});

test("rejects a symlink as the result-store root before resolving it", async t => {
  const root = await mkdtemp(path.join(os.tmpdir(), "tokensaver-phone-store-link-"));
  try {
    const realDirectory = path.join(root, "real-results");
    const linkDirectory = path.join(root, "linked-results");
    await mkdir(realDirectory);
    try {
      await symlink(realDirectory, linkDirectory, "dir");
    } catch (error) {
      if (process.platform === "win32" && ["EPERM", "EACCES", "ENOTSUP"].includes(error.code)) {
        return t.skip("Runner does not permit creating a directory symlink");
      }
      throw error;
    }
    const store = new FilePhoneResultStore(linkDirectory, makeKeyProvider());
    await assert.rejects(
      () => store.initialize(),
      /real directory, not a symlink or reparse point/
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("creates a private directory and file on POSIX", async () => {
  if (process.platform === "win32") return;
  await withStore(async (store, directory) => {
    await store.commit(id, { safe: true });
    assert.equal((await stat(directory)).mode & 0o777, 0o700);
    assert.equal((await stat(path.join(directory, id + ".json"))).mode & 0o777, 0o600);
  });
});

test("worker restart reuses the encrypted durable store after ACK failure", async () => {
  await withStore(async store => {
    let executions = 0;
    let failAck = true;
    const message = {
      message_id: id,
      sender_principal_id: "50000000-0000-4000-8000-000000000005",
      message_state: "DELIVERED",
      content: {
        version: 1,
        kind: "mission",
        mission_id: "30000000-0000-4000-8000-000000000003",
        task_id: "40000000-0000-4000-8000-000000000004",
        payload: { task: "safe test" }
      }
    };
    const authorizer = {
      authorize: async () => ({
        missionId: message.content.mission_id,
        taskId: message.content.task_id,
        senderPrincipalId: message.sender_principal_id,
        ownerPrincipalId: "60000000-0000-4000-8000-000000000006",
        fencingToken: 7,
        leaseExpiresAt: new Date(Date.now() + 60000).toISOString()
      })
    };
    const client = {
      poll: async () => [message],
      acknowledge: async () => {
        if (failAck) throw new Error("simulated network loss after durable commit");
      }
    };
    const executor = { execute: async () => { executions++; return { report: "persisted" }; } };
    const firstWorker = new PhoneRuntimeWorker({ client, executor, resultStore: store, authorizer });
    await assert.rejects(() => firstWorker.runOnce(), error => error.code === "ACK_FAILED");

    failAck = false;
    const restartedWorker = new PhoneRuntimeWorker({ client, executor, resultStore: store, authorizer });
    const outcomes = await restartedWorker.runOnce();
    assert.equal(executions, 1);
    assert.equal(outcomes[0].recovered, true);
    assert.equal(outcomes[0].state, "ACKNOWLEDGED");
  });
});


test("rejects altered key identifiers, malformed base64, and corrupted JSON records", async () => {
  await withStore(async (store, directory) => {
    await store.commit(id, { output: "integrity" });
    const file = path.join(directory, id + ".json");
    const original = JSON.parse(await readFile(file, "utf8"));

    const alteredKey = structuredClone(original);
    alteredKey.encrypted.keyId = "unknown-key";
    await writeFile(file, JSON.stringify(alteredKey));
    await assert.rejects(() => store.get(id), /32 bytes of key material/);

    const invalidNonce = structuredClone(original);
    invalidNonce.encrypted.nonce = "%%%";
    await writeFile(file, JSON.stringify(invalidNonce));
    await assert.rejects(() => store.get(id), /Invalid encrypted result nonce/);

    const invalidTag = structuredClone(original);
    invalidTag.encrypted.authTag = "AAAA";
    await writeFile(file, JSON.stringify(invalidTag));
    await assert.rejects(() => store.get(id), /Invalid encrypted result authentication tag/);

    await writeFile(file, "{broken-json");
    await assert.rejects(() => store.get(id), SyntaxError);
  });
});

test("rejects authentication-tag tampering and ciphertext substitution", async () => {
  await withStore(async (store, directory) => {
    await store.commit(id, { output: "must remain authentic" });
    const file = path.join(directory, id + ".json");
    const original = JSON.parse(await readFile(file, "utf8"));
    const tampered = structuredClone(original);
    const tag = Buffer.from(tampered.encrypted.authTag, "base64");
    tag[0] ^= 0x80;
    tampered.encrypted.authTag = tag.toString("base64");
    await writeFile(file, JSON.stringify(tampered));
    await assert.rejects(() => store.get(id), /authentication failed/);
  });
});


test("rejects a valid alternate key ID because key ID is authenticated as AAD", async () => {
  await withStore(async (store, directory) => {
    await store.commit(id, { output: "bound to key-a" });
    const file = path.join(directory, id + ".json");
    const record = JSON.parse(await readFile(file, "utf8"));
    assert.equal(record.encrypted.keyId, "key-a");
    record.encrypted.keyId = "key-b";
    await writeFile(file, JSON.stringify(record));
    await assert.rejects(() => store.get(id), /authentication failed/);
  });
});

test("rejects empty ciphertext and non-canonical ciphertext encoding", async () => {
  await withStore(async (store, directory) => {
    await store.commit(id, { output: "ciphertext validation" });
    const file = path.join(directory, id + ".json");
    const original = JSON.parse(await readFile(file, "utf8"));

    const emptyCiphertext = structuredClone(original);
    emptyCiphertext.encrypted.ciphertext = "";
    await writeFile(file, JSON.stringify(emptyCiphertext));
    await assert.rejects(() => store.get(id), /authentication failed/);

    const malformedCiphertext = structuredClone(original);
    malformedCiphertext.encrypted.ciphertext = "AB==junk";
    await writeFile(file, JSON.stringify(malformedCiphertext));
    await assert.rejects(() => store.get(id), /Invalid encrypted result ciphertext/);
  });
});
