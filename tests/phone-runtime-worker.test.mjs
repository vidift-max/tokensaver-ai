import test from "node:test";
import assert from "node:assert/strict";
import { PhoneRuntimeWorker, PhoneRuntimeWorkerError } from "../scripts/phone-runtime-worker.mjs";

const message = {
  message_id: "20000000-0000-4000-8000-000000000002",
  sender_principal_id: "50000000-0000-4000-8000-000000000005",
  message_state: "DELIVERED",
  content: {
    version: 1,
    kind: "mission",
    mission_id: "30000000-0000-4000-8000-000000000003",
    task_id: "40000000-0000-4000-8000-000000000004",
    payload: { action: "analyze", text: "authorized test mission" }
  }
};
const authorization = () => ({
  missionId: message.content.mission_id,
  taskId: message.content.task_id,
  senderPrincipalId: message.sender_principal_id,
  ownerPrincipalId: "60000000-0000-4000-8000-000000000006",
  fencingToken: 7,
  leaseExpiresAt: new Date(Date.now() + 60000).toISOString()
});
function setup({ stored = null, execute, commit, ack, authorize, signal } = {}) {
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
  const authorizer = {
    authorize: async (msg, envelope, options) => {
      events.push("authorize:" + envelope.mission_id);
      if (authorize) return authorize(msg, envelope, options);
      return authorization();
    }
  };
  return { worker: new PhoneRuntimeWorker({ client, executor, resultStore, authorizer, signal }), events };
}

test("authorizes before store read, revalidates before execution and before ACK", async () => {
  const { worker, events } = setup();
  const outcome = await worker.runOnce();
  assert.equal(outcome[0].state, "ACKNOWLEDGED");
  assert.deepEqual(events.map(x => x.split(":")[0]), [
    "poll", "authorize", "get", "authorize", "execute", "commit", "authorize", "ack"
  ]);
});

test("reuses an existing durable result only after fresh authorization", async () => {
  const stored = { messageId: message.message_id, result: { ok: true }, durable: true };
  const { worker, events } = setup({ stored });
  await worker.runOnce();
  assert.equal(events.filter(x => x.startsWith("authorize:")).length, 2);
  assert.equal(events.some(x => x.startsWith("execute:")), false);
  assert.equal(events.at(-1), "ack:" + message.message_id);
});

test("does not read result store, execute or ACK when sender authorization fails", async () => {
  const denied = setup({ authorize: async () => { const e = new Error("denied"); e.code = "AUTHORIZATION_LOST"; throw e; } });
  await assert.rejects(() => denied.worker.runOnce(), e => e.code === "AUTHORIZATION_LOST");
  assert.equal(denied.events.some(x => x.startsWith("get:")), false);
  assert.equal(denied.events.some(x => x.startsWith("execute:")), false);
  assert.equal(denied.events.some(x => x.startsWith("ack:")), false);
});

test("rejects authorization evidence bound to a different task or sender", async () => {
  for (const alter of [
    a => { a.taskId = "70000000-0000-4000-8000-000000000007"; },
    a => { a.senderPrincipalId = "70000000-0000-4000-8000-000000000007"; },
    a => { a.fencingToken = 0; },
    a => { a.leaseExpiresAt = new Date(Date.now() - 1000).toISOString(); }
  ]) {
    const denied = setup({ authorize: async () => { const a = authorization(); alter(a); return a; } });
    await assert.rejects(() => denied.worker.runOnce(), e => e.code === "MISSION_AUTHORIZATION_INVALID");
    assert.equal(denied.events.some(x => x.startsWith("get:")), false);
  }
});

test("rejects a changed fencing token before execution", async () => {
  let calls = 0;
  const changed = setup({
    authorize: async () => {
      calls++;
      const a = authorization();
      if (calls === 2) a.fencingToken++;
      return a;
    }
  });
  await assert.rejects(() => changed.worker.runOnce(), e => e.code === "AUTHORIZATION_FENCE_CHANGED");
  assert.equal(changed.events.some(x => x.startsWith("execute:")), false);
});

test("does not ACK a durable result if the fencing token changes before ACK", async () => {
  let calls = 0;
  const changedAfterCommit = setup({
    authorize: async () => {
      calls++;
      const a = authorization();
      if (calls === 3) a.fencingToken++;
      return a;
    }
  });
  await assert.rejects(
    () => changedAfterCommit.worker.runOnce(),
    e => e.code === "AUTHORIZATION_FENCE_CHANGED"
  );
  assert.equal(changedAfterCommit.events.some(x => x.startsWith("ack:")), false);

  calls = 0;
  const changedDuringRecovery = setup({
    stored: { messageId: message.message_id, result: { ok: true }, durable: true },
    authorize: async () => {
      calls++;
      const a = authorization();
      if (calls === 2) a.fencingToken++;
      return a;
    }
  });
  await assert.rejects(
    () => changedDuringRecovery.worker.runOnce(),
    e => e.code === "AUTHORIZATION_FENCE_CHANGED"
  );
  assert.equal(changedDuringRecovery.events.some(x => x.startsWith("ack:")), false);
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
  const authorizer = { authorize: async () => authorization() };
  const worker = new PhoneRuntimeWorker({ client, executor, resultStore, authorizer });
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

test("requires an explicit executor, result store and authorizer", () => {
  assert.throws(() => new PhoneRuntimeWorker({}), e => e instanceof PhoneRuntimeWorkerError && e.code === "CLIENT_REQUIRED");
  assert.throws(() => new PhoneRuntimeWorker({
    client: { poll() {}, acknowledge() {} },
    executor: { execute() {} },
    resultStore: { get() {}, commit() {} }
  }), e => e.code === "AUTHORIZER_REQUIRED");
});
