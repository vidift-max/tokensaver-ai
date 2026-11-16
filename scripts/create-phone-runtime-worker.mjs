/**
 * Production dependency wiring for the Phone-02 mailbox worker.
 *
 * The AI executor is always explicitly injected. The durable result store is
 * either explicitly injected or constructed from AI_PHONE_RESULT_DIRECTORY
 * with the current user's persistent OS credential store. There is no
 * plaintext fallback.
 */
import { PhoneRuntimeClient } from "./phone-runtime-client.mjs";
import { TeamRoomClient } from "./team-room-client.mjs";
import { TeamRoomPhoneMissionAuthorizer } from "./phone-mission-authorizer.mjs";
import { PhoneRuntimeWorker } from "./phone-runtime-worker.mjs";
import { createOsBackedPhoneResultStore } from "./create-os-backed-phone-result-store.mjs";

export async function createPhoneRuntimeWorker({
  env = process.env,
  executor,
  signal,
  fetchImpl = globalThis.fetch,
  teamRoomClient,
  resultStore,
  keyringOptions
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

  const configuredResultStore = resultStore || (env.AI_PHONE_RESULT_DIRECTORY
    ? await createOsBackedPhoneResultStore(env.AI_PHONE_RESULT_DIRECTORY, keyringOptions)
    : null);
  if (!configuredResultStore || typeof configuredResultStore.initialize !== "function" ||
      typeof configuredResultStore.get !== "function" || typeof configuredResultStore.commit !== "function") {
    const error = new Error("An OS-keyring-backed encrypted result store or an explicitly configured secure store is required");
    error.code = "ENCRYPTED_RESULT_STORE_REQUIRED";
    throw error;
  }
  await configuredResultStore.initialize();
  return new PhoneRuntimeWorker({ client, executor, resultStore: configuredResultStore, authorizer, signal });
}
