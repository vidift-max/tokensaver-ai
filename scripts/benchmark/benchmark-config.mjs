#!/usr/bin/env node
/**
 * Shared benchmark configuration helpers.
 * Non-secret values may come from BENCH_CONFIG_FILE (JSON); environment variables
 * override file values. Credentials are accepted only through process environment.
 */
import { readFileSync } from "node:fs";
import { performance } from "node:perf_hooks";

const forbidden = /(^|_)(TOKEN|SECRET|PASSWORD|DATABASE_URL|PRIVATE_KEY|API_KEY)($|_)/i;
let fileConfig = {};
const configPath = process.env.BENCH_CONFIG_FILE;
if (configPath) {
  fileConfig = JSON.parse(readFileSync(configPath, "utf8"));
  if (!fileConfig || Array.isArray(fileConfig) || typeof fileConfig !== "object") {
    throw new Error("BENCH_CONFIG_FILE must contain a JSON object");
  }
  const secrets = Object.keys(fileConfig).filter(key => forbidden.test(key));
  if (secrets.length) throw new Error("Secret-bearing keys are forbidden in BENCH_CONFIG_FILE: " + secrets.join(", "));
}

export function benchValue(name, fallback) {
  return process.env[name] ?? fileConfig[name] ?? fallback;
}
export function benchNumber(name, fallback, { min = 0, integer = true } = {}) {
  const raw = benchValue(name, fallback);
  const value = Number(raw);
  if (!Number.isFinite(value) || (integer && !Number.isSafeInteger(value)) || value < min) {
    throw new Error(`Invalid ${name}: expected ${integer ? "integer" : "number"} >= ${min}`);
  }
  return value;
}
export function benchBoolean(name, fallback = false) {
  const raw = benchValue(name, fallback);
  if (raw === true || raw === "true") return true;
  if (raw === false || raw === "false") return false;
  throw new Error(`Invalid ${name}: expected true or false`);
}
export function messageOfBytes(prefix, requestedBytes) {
  const prefixBytes = Buffer.byteLength(prefix, "utf8");
  if (prefixBytes >= requestedBytes) return Buffer.from(prefix, "utf8").subarray(0, requestedBytes).toString("utf8");
  return prefix + "x".repeat(requestedBytes - prefixBytes);
}
export function createRateGate(ratePerSecond) {
  let nextAt = 0;
  return async function waitForSlot() {
    if (ratePerSecond <= 0) return;
    const now = performance.now();
    const slot = Math.max(now, nextAt);
    nextAt = slot + 1000 / ratePerSecond;
    const delay = slot - now;
    if (delay > 0) await new Promise(resolve => setTimeout(resolve, delay));
  };
}
