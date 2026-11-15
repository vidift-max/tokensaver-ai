import test from "node:test";
import assert from "node:assert/strict";
import { createPhoneRuntimeWorker } from "../scripts/create-phone-runtime-worker.mjs";

test("runtime bootstrap refuses to invent or omit an AI executor", async () => {
  await assert.rejects(
    () => createPhoneRuntimeWorker({ env: {} }),
    error => error.code === "EXECUTOR_REQUIRED"
  );
});

test("runtime bootstrap fails closed when Phone or Team Room credentials are absent", async () => {
  const executor = { execute: async () => ({}) };
  await assert.rejects(
    () => createPhoneRuntimeWorker({ env: {}, executor }),
    error => error.code === "CONFIG_REQUIRED" || error.code === "AUTHORIZER_CONFIG_REQUIRED"
  );
});
