/**
 * Production dependency wiring for the Phone-02 mailbox worker.
 *
 * The AI executor and encrypted result store are intentionally injected.
 * The result store must be configured with an explicit secure key provider;
 * this bootstrap does not invent a key source or fall back to plaintext.
 */
import { PhoneRuntimeClient } from "./phone-runtime-client.mjs";
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

  const client = new PhoneRuntimeClient(env, fetchImpl);
  // Use the same explicit environment object as the Phone client. This keeps
  // injected/test deployments deterministic and prevents accidental fallback
  // to unrelated process.env credentials.
  const trustedTeamRoomClient = teamRoomClient || new TeamRoomClient(env);
  const authorizer = new TeamRoomPhoneMissionAuthorizer({
    teamRoomClient: trustedTeamRoomClient,
    authorizedSenderPrincipalId: env.TEAM_ROOM_AUTHORIZED_SENDER_PRINCIPAL_ID
  });

  if (!resultStore || typeof resultStore.initialize !== "function" ||
      typeof resultStore.get !== "function" || typeof resultStore.commit !== "function") {
    const error = new Error("An explicitly configured encrypted result store with a secure key provider is required");
    error.code = "ENCRYPTED_RESULT_STORE_REQUIRED";
    throw error;
  }
  await resultStore.initialize();
  return new PhoneRuntimeWorker({ client, executor, resultStore, authorizer, signal });
}
