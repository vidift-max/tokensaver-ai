import test from "node:test";
import assert from "node:assert/strict";
import { createCoalescedRefreshScheduler } from "../src/teamRoomRefreshScheduler.mjs";

const wait = ms => new Promise(resolve => setTimeout(resolve, ms));

test("coalesces a burst of 100 WebSocket notifications into one refresh", async () => {
  let calls = 0;
  const scheduler = createCoalescedRefreshScheduler(async () => { calls++; }, 20);
  let accepted = 0;
  for (let i = 0; i < 100; i++) if (scheduler.schedule()) accepted++;
  assert.equal(accepted, 1, "only one timer should be armed for the burst");
  await wait(40);
  assert.equal(calls, 1);
  scheduler.cancel();
});

test("events arriving during a refresh produce one trailing refresh, never overlap", async () => {
  let calls = 0;
  let active = 0;
  let maxActive = 0;
  const scheduler = createCoalescedRefreshScheduler(async () => {
    calls++;
    active++;
    maxActive = Math.max(maxActive, active);
    await wait(25);
    active--;
  }, 5);

  scheduler.schedule();
  await wait(10);
  for (let i = 0; i < 20; i++) scheduler.schedule();
  await wait(70);
  assert.equal(calls, 2, "in-flight events should collapse into one trailing refresh");
  assert.equal(maxActive, 1, "refresh operations must not overlap");
  scheduler.cancel();
});

test("cancel prevents a scheduled refresh after unmount", async () => {
  let calls = 0;
  const scheduler = createCoalescedRefreshScheduler(async () => { calls++; }, 20);
  scheduler.schedule();
  scheduler.cancel();
  await wait(40);
  assert.equal(calls, 0);
});

test("rejects invalid scheduling windows", () => {
  assert.throws(() => createCoalescedRefreshScheduler(() => Promise.resolve(), -1), RangeError);
  assert.throws(() => createCoalescedRefreshScheduler(() => Promise.resolve(), 5001), RangeError);
});
