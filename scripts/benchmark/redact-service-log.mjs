#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";

const [source, destination] = process.argv.slice(2);
if (!source || !destination) throw new Error("Usage: redact-service-log.mjs source destination");
let log = readFileSync(source, "utf8");
let tokens = [];
try {
  const target = JSON.parse(process.env.TEAM_ROOM_SESSION_TOKENS_JSON || "[]");
  const other = JSON.parse(process.env.TEAM_ROOM_OTHER_ROOM_SESSION_TOKENS_JSON || "[]");
  tokens = [...(Array.isArray(target) ? target : []), ...(Array.isArray(other) ? other : [])].filter(token => typeof token === "string" && token.length > 0);
} catch {}
for (const token of tokens) log = log.split(token).join("[REDACTED_SESSION]");
log = log.replace(/Bearer\s+[^\s]+/gi, "Bearer [REDACTED]");
log = log.replace(/postgres(?:ql)?:\/\/[^\s]+/gi, "[REDACTED_DATABASE_URL]");
if (tokens.some(token => log.includes(token))) throw new Error("Session token remained in sanitized service log");
writeFileSync(destination, log);
process.stdout.write("Sanitized service diagnostics saved; raw session tokens were checked and absent.\n");
