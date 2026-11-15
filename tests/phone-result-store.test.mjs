import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, rm, stat } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { FilePhoneResultStore } from "../scripts/phone-result-store.mjs";
import { PhoneRuntimeWorker } from "../scripts/phone-runtime-worker.mjs";

const id = "30000000-0000-4000-8000-000000000003";
async function withStore(run) {
  const root = await mkdtemp(path.join(os.tmpdir(), "tokensaver-phone-store-"));
  try {
    const store = new FilePhoneResultStore(path.join(root, "private-results"));
    await run(store, path.join(root, "private-results"));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("commits a result atomically and reads it after reopening", async () => {
  await withStore(async (store, directory) => {
    const committed = await store.commit(id, { output: "done" });
    assert.equal(committed.durable, true);
    assert.deepEqual((await new FilePhoneResultStore(directory).get(id)).result, { output: "done" });
    const files = await readdir(directory);
    assert.deepEqual(files, [id + ".json"]);
  });
});

test("first committed result wins and cannot be overwritten", async () => {
  await withStore(async store => {
    await store.commit(id, { version: 1 });
    const second = await store.commit(id, { version: 2 });
    assert.deepEqual(second.result, { version: 1 });
  });
});

test("concurrent commits publish exactly one complete record", async () => {
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

test("rejects malformed IDs and detects corrupted records", async () => {
  await withStore(async (store, directory) => {
    await assert.rejects(() => store.get("../escape"), /Invalid message UUID/);
    await store.initialize();
    const file = path.join(directory, id + ".json");
    await import("node:fs/promises").then(fs => fs.writeFile(file, "{broken"));
    await assert.rejects(() => store.get(id), SyntaxError);
  });
});

test("creates a private directory and file on POSIX", async () => {
  if (process.platform === "win32") return;
  await withStore(async (store, directory) => {
    await store.commit(id, { safe: true });
    assert.equal((await stat(directory)).mode & 0o777, 0o700);
    assert.equal((await stat(path.join(directory, id + ".json"))).mode & 0o777, 0o600);
  });
});

test("worker restart reuses the real durable store after ACK failure", async () => {
  await withStore(async store => {
    let executions = 0;
    let failAck = true;
    const message = { message_id: id, message_state: "DELIVERED", content: { task: "safe test" } };
    const client = {
      poll: async () => [message],
      acknowledge: async () => {
        if (failAck) throw new Error("simulated network loss after durable commit");
      }
    };
    const executor = { execute: async () => { executions++; return { report: "persisted" }; } };
    const firstWorker = new PhoneRuntimeWorker({ client, executor, resultStore: store });
    await assert.rejects(() => firstWorker.runOnce(), error => error.code === "ACK_FAILED");

    failAck = false;
    const restartedWorker = new PhoneRuntimeWorker({ client, executor, resultStore: store });
    const outcomes = await restartedWorker.runOnce();
    assert.equal(executions, 1);
    assert.equal(outcomes[0].recovered, true);
    assert.equal(outcomes[0].state, "ACKNOWLEDGED");
  });
});
