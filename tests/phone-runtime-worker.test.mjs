import test from "node:test";
import assert from "node:assert/strict";
import { PhoneRuntimeWorker, PhoneRuntimeWorkerError } from "../scripts/phone-runtime-worker.mjs";

const message = {
  message_id: "20000000-0000-4000-8000-000000000002",
  message_state: "DELIVERED",
  content: { kind: "mission", text: "authorized test mission" }
};
function setup({ stored = null, execute, commit, ack, signal } = {}) {
  const events = [];
  const client = {
    poll: async () => { events.push("poll"); return [message]; },
    acknowledge: async id => { events.push("ack:" + id); return ack?.(id); }
  };
  const executor = {
    execute: async (msg, options) => {
      events.push("execute:" + options.idempotencyKey);
      return execute ? execute(msg, options) : { accepted: true };
    }
  };
  const resultStore = {
    get: async id => { events.push("get:" + id); return stored; },
    commit: async (id, result) => {
      events.push("commit:" + id);
      return commit ? commit(id, result) : { messageId: id, result, durable: true };
    }
  };
  return { worker: new PhoneRuntimeWorker({ client, executor, resultStore, signal }), events };
}

test("executes, durably commits, then ACKs in strict order", async () => {
  const { worker, events } = setup();
  const outcome = await worker.runOnce();
  assert.equal(outcome[0].state, "ACKNOWLEDGED");
  assert.deepEqual(events.map(x => x.split(":")[0]), ["poll", "get", "execute", "commit", "ack"]);
});

test("reuses an existing durable result and does not execute again", async () => {
  const stored = { messageId: message.message_id, result: { ok: true }, durable: true };
  const { worker, events } = setup({ stored });
  await worker.runOnce();
  assert.equal(events.some(x => x.startsWith("execute:")), false);
  assert.equal(events.at(-1), "ack:" + message.message_id);
});

test("does not ACK if executor fails or durable commit fails", async () => {
  const executorFailure = setup({ execute: async () => { throw new Error("provider down"); } });
  await assert.rejects(() => executorFailure.worker.runOnce(), e => e.code === "EXECUTOR_FAILED");
  assert.equal(executorFailure.events.some(x => x.startsWith("ack:")), false);

  const commitFailure = setup({ commit: async () => { throw new Error("disk unavailable"); } });
  await assert.rejects(() => commitFailure.worker.runOnce(), e => e.code === "DURABLE_COMMIT_FAILED");
  assert.equal(commitFailure.events.some(x => x.startsWith("ack:")), false);
});

test("does not ACK unless the store explicitly confirms durability", async () => {
  const { worker, events } = setup({ commit: async id => ({ messageId: id, durable: false }) });
  await assert.rejects(() => worker.runOnce(), e => e.code === "DURABILITY_UNCONFIRMED");
  assert.equal(events.some(x => x.startsWith("ack:")), false);
});

test("durable result survives ACK failure and retry skips executor", async () => {
  let stored = null;
  let executions = 0;
  let failAck = true;
  const events = [];
  const client = {
    poll: async () => [message],
    acknowledge: async () => {
      events.push("ack");
      if (failAck) throw new Error("temporary network issue");
    }
  };
  const executor = { execute: async () => { executions++; return { ok: true }; } };
  const resultStore = {
    get: async () => stored,
    commit: async (id, result) => {
      stored = { messageId: id, result, durable: true };
      return stored;
    }
  };
  const worker = new PhoneRuntimeWorker({ client, executor, resultStore });
  await assert.rejects(() => worker.runOnce(), e => e.code === "ACK_FAILED");
  failAck = false;
  await worker.runOnce();
  assert.equal(executions, 1);
  assert.equal(events.length, 2);
});

test("stops on authorization loss and abort signal", async () => {
  const denied = setup({ ack: async () => { const e = new Error("forbidden"); e.code = "AUTHORIZATION_LOST"; throw e; } });
  await assert.rejects(() => denied.worker.runOnce(), e => e.code === "AUTHORIZATION_LOST");

  const controller = new AbortController();
  controller.abort();
  const aborted = setup({ signal: controller.signal });
  await assert.rejects(() => aborted.worker.runOnce(), e => e.code === "ABORTED");
});

test("requires an explicit executor and durable result store", () => {
  assert.throws(() => new PhoneRuntimeWorker({}), e => e instanceof PhoneRuntimeWorkerError && e.code === "CLIENT_REQUIRED");
});
