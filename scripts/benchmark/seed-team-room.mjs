#!/usr/bin/env node
/**
 * Seed an ephemeral, isolated Team Room benchmark fixture.
 * Generates short-lived opaque sessions and stores only SHA-256 token digests in PostgreSQL.
 * The raw tokens are exported only to the current GitHub Actions job environment.
 */
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";

const databaseUrl = process.env.DATABASE_URL;
if (!databaseUrl) throw new Error("DATABASE_URL is required");
const connection = new URL(databaseUrl);
const participantCount = Number(process.env.BENCH_PARTICIPANTS || 8);
const tenantCount = Number(process.env.BENCH_TENANTS || 20);
const messageCount = Number(process.env.BENCH_SEED_MESSAGES || 10000);
const taskCount = Number(process.env.BENCH_SEED_TASKS || 250);
const decisionCount = Number(process.env.BENCH_SEED_DECISIONS || 250);
const attachmentCount = Number(process.env.BENCH_SEED_ATTACHMENTS || 250);
if (![participantCount, tenantCount, messageCount, taskCount, decisionCount, attachmentCount].every(Number.isSafeInteger) ||
    participantCount < 2 || participantCount > 15 || tenantCount < 1 || messageCount < 1 ||
    taskCount < 1 || decisionCount < 1 || attachmentCount < 1) {
  throw new Error("Invalid benchmark fixture cardinality");
}

const ids = n => Array.from({ length: n }, () => randomUUID());
const tenants = ids(tenantCount);
const rooms = ids(tenantCount);
const principals = ids(participantCount);
const participants = ids(participantCount);
const sessions = ids(participantCount);
const executions = ids(participantCount);
const workloads = ids(participantCount);
const tokens = Array.from({ length: participantCount }, () => randomBytes(32).toString("base64url"));
const tokenHashes = tokens.map(token => createHash("sha256").update(token).digest("hex"));
const otherRoomCount = Math.min(4, tenantCount - 1);
const otherPrincipals = ids(otherRoomCount);
const otherParticipants = ids(otherRoomCount);
const otherSessions = ids(otherRoomCount);
const otherExecutions = ids(otherRoomCount);
const otherWorkloads = ids(otherRoomCount);
const otherTokens = Array.from({ length: otherRoomCount }, () => randomBytes(32).toString("base64url"));
const otherTokenHashes = otherTokens.map(token => createHash("sha256").update(token).digest("hex"));
const otherSubjects = otherPrincipals.map(id => "benchmark-noise-" + id);
const otherChannelIds = ids(otherRoomCount);
const subjects = principals.map(id => `benchmark-${id}`);
const issuer = "https://benchmark.invalid";
const channelId = randomUUID();
const quote = value => "'" + String(value).replaceAll("'", "''") + "'";
const uuidArray = values => "ARRAY[" + values.map(value => quote(value) + "::uuid").join(",") + "]::uuid[]";

const values = rows => rows.map(row => "(" + row.map(value => value === null ? "NULL" : typeof value === "number" ? String(value) : quote(value)).join(",") + ")").join(",\n");
const sql = [
  "BEGIN;",
  "INSERT INTO tenants(tenant_id,name) VALUES " + values(tenants.map((id,i) => [id, `benchmark-tenant-${i+1}`])) + ";",
  "INSERT INTO rooms(room_id,tenant_id,name) VALUES " + values(rooms.map((id,i) => [id,tenants[i],`benchmark-room-${i+1}`])) + ";",
  "INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES " +
    values(principals.map((id,i) => [id,tenants[0],"AI",issuer,subjects[i]])) + ";",
  "INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES " +
    values(participants.map((id,i) => [id,tenants[0],principals[i],`Benchmark Agent ${i+1}`])) + ";",
  "INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES " +
    values(workloads.map((id,i) => [id,tenants[0],principals[i],issuer,subjects[i],"ACTIVE"])) + ";",
  "INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status,token_hash) VALUES " +
    sessions.map((id,i) => "(" + [quote(id),quote(tenants[0]),quote(rooms[0]),quote(principals[i]),"NOW()","NOW()+INTERVAL '2 hours'","'ACTIVE'","decode("+quote(tokenHashes[i])+",'hex')"].join(",") + ")").join(",") + ";",
  "INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES " +
    executions.map((id,i) => "(" + [quote(id),quote(tenants[0]),quote(rooms[0]),quote(participants[i]),quote(principals[i]),quote(sessions[i]),quote(issuer+"|"+subjects[i]),"NOW()","NOW()+INTERVAL '2 hours'","'ACTIVE'"].join(",") + ")").join(",") + ";",
  "INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES " +
    values(participants.map((id,i) => [randomUUID(),tenants[0],rooms[0],id,principals[i],sessions[i],"ACTIVE"])) + ";",
  "INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES " +
    values(otherPrincipals.map((id,i) => [id,tenants[i+1],"AI",issuer,otherSubjects[i]])) + ";",
  "INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES " +
    values(otherParticipants.map((id,i) => [id,tenants[i+1],otherPrincipals[i],"Noise Agent "+(i+1)])) + ";",
  "INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES " +
    values(otherWorkloads.map((id,i) => [id,tenants[i+1],otherPrincipals[i],issuer,otherSubjects[i],"ACTIVE"])) + ";",
  "INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status,token_hash) VALUES " +
    otherSessions.map((id,i) => "(" + [quote(id),quote(tenants[i+1]),quote(rooms[i+1]),quote(otherPrincipals[i]),"NOW()","NOW()+INTERVAL '2 hours'","'ACTIVE'","decode("+quote(otherTokenHashes[i])+",'hex')"].join(",") + ")").join(",") + ";",
  "INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES " +
    otherExecutions.map((id,i) => "(" + [quote(id),quote(tenants[i+1]),quote(rooms[i+1]),quote(otherParticipants[i]),quote(otherPrincipals[i]),quote(otherSessions[i]),quote(issuer+"|"+otherSubjects[i]),"NOW()","NOW()+INTERVAL '2 hours'","'ACTIVE'"].join(",") + ")").join(",") + ";",
  "INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES " +
    values(otherParticipants.map((id,i) => [randomUUID(),tenants[i+1],rooms[i+1],id,otherPrincipals[i],otherSessions[i],"ACTIVE"])) + ";",
  "INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES " +
    values(otherPrincipals.flatMap((id,i) => ["room.read","room.write","task.read","attachment.read","attachment.upload","decision.propose","presence.update"].map(permission => [randomUUID(),tenants[i+1],id,rooms[i+1],permission]))) + ";",
  "INSERT INTO team_room_channels(channel_id,tenant_id,room_id,channel_type,created_by_principal_id) VALUES " +
    values(otherChannelIds.map((id,i) => [id,tenants[i+1],rooms[i+1],"GENERAL",otherPrincipals[i]])) + ";",

  "INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) SELECT gen_random_uuid()," +
    quote(tenants[0]) + "::uuid,actor.principal_id," + quote(rooms[0]) + "::uuid,perms.permission FROM unnest(" + uuidArray(principals) + ") AS actor(principal_id) CROSS JOIN unnest(ARRAY['room.read','room.write','task.read','attachment.read','attachment.upload','decision.propose','presence.update']::text[]) AS perms(permission);",
  "INSERT INTO team_room_channels(channel_id,tenant_id,room_id,channel_type,created_by_principal_id) VALUES (" +
    [quote(channelId),quote(tenants[0]),quote(rooms[0]),"'GENERAL'",quote(principals[0])].join(",") + ");",
  "INSERT INTO team_room_messages(message_id,tenant_id,room_id,channel_id,sender_principal_id,client_message_id,body,created_at) " +
    "SELECT md5('bench-message-'||g)::uuid," + quote(tenants[0]) + "::uuid," + quote(rooms[0]) + "::uuid," + quote(channelId) + "::uuid," +
    " (" + uuidArray(principals) + ")[((g-1)%" + participantCount + ")+1],md5('bench-client-'||g)::uuid,'seeded benchmark message '||g,NOW()-(g||' seconds')::interval FROM generate_series(1," + messageCount + ") g;",
  "INSERT INTO team_room_message_reads(channel_id,principal_id,last_read_at) SELECT " + quote(channelId) + "::uuid,actor.principal_id,NOW()-INTERVAL '1 day' FROM unnest(" + uuidArray(principals) + ") AS actor(principal_id);",
  "INSERT INTO team_tasks(task_id,tenant_id,room_id,mission_id,title,created_by_principal_id,created_at,updated_at) " +
    "SELECT md5('bench-task-'||g)::uuid," + quote(tenants[0]) + "::uuid," + quote(rooms[0]) + "::uuid,md5('bench-mission-'||g)::uuid,'Benchmark task '||g," + quote(principals[0]) + "::uuid,NOW()-(g||' seconds')::interval,NOW()-(g||' seconds')::interval FROM generate_series(1," + taskCount + ") g;",
  "INSERT INTO team_room_decisions(decision_id,tenant_id,room_id,author_principal_id,title,decision,created_at) " +
    "SELECT md5('bench-decision-'||g)::uuid," + quote(tenants[0]) + "::uuid," + quote(rooms[0]) + "::uuid," + quote(principals[0]) + "::uuid,'Benchmark decision '||g,'Seeded decision record for query profiling',NOW()-(g||' seconds')::interval FROM generate_series(1," + decisionCount + ") g;",
  "INSERT INTO team_room_attachment_refs(attachment_id,tenant_id,room_id,channel_id,uploaded_by_principal_id,file_name,storage_ref,content_type,byte_len,created_at) " +
    "SELECT md5('bench-attachment-'||g)::uuid," + quote(tenants[0]) + "::uuid," + quote(rooms[0]) + "::uuid," + quote(channelId) + "::uuid," + quote(principals[0]) + "::uuid,'benchmark-'||g||'.bin','benchmark://fixture/'||g,'application/octet-stream',1024,NOW()-(g||' seconds')::interval FROM generate_series(1," + attachmentCount + ") g;",
  "COMMIT;"
].join("\n");

const env = {
  ...process.env,
  PGHOST: connection.hostname,
  PGPORT: connection.port || "5432",
  PGUSER: decodeURIComponent(connection.username),
  PGPASSWORD: decodeURIComponent(connection.password),
  PGDATABASE: decodeURIComponent(connection.pathname.replace(/^\//, "")),
};
const result = spawnSync("psql", ["-X", "-v", "ON_ERROR_STOP=1"], { input: sql, encoding: "utf8", env });
if (result.status !== 0) {
  process.stderr.write(result.stderr || "psql benchmark fixture failed\n");
  process.exit(result.status || 1);
}

const envFile = process.env.GITHUB_ENV;
if (!envFile) throw new Error("GITHUB_ENV is required; refusing to print raw session tokens");
for (const token of [...tokens, ...otherTokens]) process.stdout.write("::add-mask::" + token + "\n");
appendFileSync(envFile, [
  `TEAM_ROOM_API_URL=http://127.0.0.1:8081`,
  `TEAM_ROOM_ROOM_ID=${rooms[0]}`,
  `TEAM_ROOM_TENANT_ID=${tenants[0]}`,
  `TEAM_ROOM_CHANNEL_ID=${channelId}`,
  `TEAM_ROOM_PRINCIPAL_ID=${principals[0]}`,
  `TEAM_ROOM_SESSION_TOKENS_JSON=${JSON.stringify(tokens)}`,
  `TEAM_ROOM_OTHER_ROOM_IDS_JSON=${JSON.stringify(rooms.slice(1, otherRoomCount + 1))}`,
  `TEAM_ROOM_OTHER_ROOM_SESSION_TOKENS_JSON=${JSON.stringify(otherTokens)}`,
  `BENCH_OTHER_ROOMS=${otherRoomCount}`,
  `BENCH_PARTICIPANTS=${participantCount}`,
  `BENCH_TENANTS=${tenantCount}`,
  `BENCH_ROOMS=${tenantCount}`,
  `BENCH_SEED_MESSAGES=${messageCount}`,
  `BENCH_SEED_TASKS=${taskCount}`,
  `BENCH_SEED_DECISIONS=${decisionCount}`,
  `BENCH_SEED_ATTACHMENTS=${attachmentCount}`,
  ""
].join("\n"));

const artifactDir = resolve(process.env.BENCH_ARTIFACT_DIR || "benchmark-artifacts");
mkdirSync(artifactDir, { recursive: true });
const metadata = {
  generated_at: new Date().toISOString(),
  fixture: {
    tenants: tenantCount,
    rooms: tenantCount,
    participants_in_target_room: participantCount,
    other_room_participants: otherRoomCount,
    messages: messageCount,
    tasks: taskCount,
    active_presence_memberships: participantCount,
    decisions: decisionCount,
    attachment_references: attachmentCount,
  },
  target: { tenant_id: tenants[0], room_id: rooms[0], channel_id: channelId },
  credentials: { raw_tokens_written_to_artifact: false, target_room_tokens: tokens.length, other_room_tokens: otherTokens.length, expires_in: "2 hours" },
};
writeFileSync(resolve(artifactDir, "seed-metadata.json"), JSON.stringify(metadata, null, 2));
process.stdout.write(JSON.stringify(metadata, null, 2) + "\n");
