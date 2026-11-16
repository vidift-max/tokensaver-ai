import { FilePhoneResultStore } from "./phone-result-store.mjs";
import { OsKeyringPhoneResultKeyProvider } from "./phone-result-key-provider.mjs";

/**
 * Creates the production Phone-02 encrypted result store using the current
 * user's persistent OS credential store for AES-256-GCM key material.
 */
export async function createOsBackedPhoneResultStore(directory, keyringOptions = {}) {
  if (typeof directory !== "string" || !directory.trim()) {
    throw new TypeError("AI_PHONE_RESULT_DIRECTORY must be configured");
  }
  const keyProvider = keyringOptions.Entry
    ? new OsKeyringPhoneResultKeyProvider(keyringOptions)
    : await OsKeyringPhoneResultKeyProvider.create(keyringOptions);
  const store = new FilePhoneResultStore(directory, keyProvider);
  await store.initialize();
  return store;
}
