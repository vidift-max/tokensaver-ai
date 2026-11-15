/**
 * Production dependency wiring for the Phone-02 mailbox worker.
 *
 * The AI executor is intentionally injected: this repository does not yet
 * provide a verified external AI-provider adapter. No placeholder executor
 * is created, and missing runtime configuration fails closed.
 */
import { PhoneRuntimeClient } from "./phone-runtime-client.mjs";
import { FilePhoneResultStore } from "./phone-result-store.mjs";
import { TeamRoomClient } from "./team-room-client.mjs";
import { TeamRoomPhoneMissionAuthorizer } from "./phone-mission-authorizer.mjs";
import { PhoneRuntimeWorker } from "./phone-runtime-worker.mjs";

export async function createPhoneRuntimeWorker({
  env = process.env,
  executor,
  signal,
  fetchImpl = globalThis.fetch,
  teamRoomClient,
  resultStore
} = {}) {
  if (!executor || typeof executor.execute !== "function") {
    const error = new Error("A real, explicitly configured AI executor is required; no default executor is available");
    error.code = "EXECUTOR_REQUIRED";
    throw error;
  }
  if (typeof env.AI_PHONE_RESULT_DIRECTORY !== "string" || !env.AI_PHONE_RESULT_DIRECTORY.trim()) {
    const error = new Error("AI_PHONE_RESULT_DIRECTORY must point to a private local result directory");
    error.code = "RESULT_DIRECTORY_REQUIRED";
    throw error;
  }
  const client = new PhoneRuntimeClient(env, fetchImpl);
  // Use the same explicit environment object as the Phone client. This keeps
  // injected/test deployments deterministic and prevents accidental fallback
  // to unrelated process.env credentials.
  const trustedTeamRoomClient = teamRoomClient || new TeamRoomClient(env);
  const authorizer = new TeamRoomPhoneMissionAuthorizer({
    teamRoomClient: trustedTeamRoomClient,
    authorizedSenderPrincipalId: env.TEAM_ROOM_AUTHORIZED_SENDER_PRINCIPAL_ID
  });
  const store = resultStore || new FilePhoneResultStore(env.AI_PHONE_RESULT_DIRECTORY);
  await store.initialize();
  return new PhoneRuntimeWorker({ client, executor, resultStore: store, authorizer, signal });
}
