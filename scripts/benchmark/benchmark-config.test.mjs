import test from "node:test";
import assert from "node:assert/strict";
import { benchNumber, benchBoolean, messageOfBytes } from "./benchmark-config.mjs";

test("configuration defaults and numeric validation", () => {
  assert.equal(benchNumber("BENCH_CONFIG_TEST_NUMBER", 8, { min: 1 }), 8);
  process.env.BENCH_CONFIG_TEST_NUMBER = "12";
  assert.equal(benchNumber("BENCH_CONFIG_TEST_NUMBER", 8, { min: 1 }), 12);
  process.env.BENCH_CONFIG_TEST_NUMBER = "-1";
  assert.throws(() => benchNumber("BENCH_CONFIG_TEST_NUMBER", 8, { min: 0 }), /Invalid BENCH_CONFIG_TEST_NUMBER/);
  delete process.env.BENCH_CONFIG_TEST_NUMBER;
});

test("boolean config accepts only explicit true/false", () => {
  process.env.BENCH_CONFIG_TEST_BOOL = "true";
  assert.equal(benchBoolean("BENCH_CONFIG_TEST_BOOL", false), true);
  process.env.BENCH_CONFIG_TEST_BOOL = "maybe";
  assert.throws(() => benchBoolean("BENCH_CONFIG_TEST_BOOL", false), /expected true or false/);
  delete process.env.BENCH_CONFIG_TEST_BOOL;
});

test("message payload is padded to the requested UTF-8 byte count", () => {
  assert.equal(Buffer.byteLength(messageOfBytes("hello", 32), "utf8"), 32);
  assert.equal(Buffer.byteLength(messageOfBytes("abcdef", 3), "utf8"), 3);
});
