import {
  createCipheriv,
  createDecipheriv,
  randomBytes,
  randomUUID
} from "node:crypto";
import {
  chmod,
  lstat,
  open,
  link,
  mkdir,
  readFile,
  realpath,
  rm,
  stat
} from "node:fs/promises";
import path from "node:path";
import { secureWindowsResultDirectory } from "./phone-windows-acl.mjs";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const KEY_ID = /^[A-Za-z0-9._-]{1,128}$/;
const FORMAT_VERSION = 2;
const ALGORITHM = "A256GCM";
const NONCE_BYTES = 12;
const TAG_BYTES = 16;

function requireKey(value, keyId) {
  if (!Buffer.isBuffer(value) && !(value instanceof Uint8Array)) {
    throw new TypeError("Key provider must return 32 bytes of key material");
  }
  const key = Buffer.from(value);
  if (key.length !== 32) throw new TypeError("AES-256-GCM requires exactly 32 key bytes");
  if (typeof keyId !== "string" || !KEY_ID.test(keyId)) {
    throw new TypeError("Key provider returned an invalid key identifier");
  }
  return key;
}

function decodeBase64(value, expectedLength, label) {
  if (typeof value !== "string" || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) {
    throw new Error("Invalid encrypted result " + label);
  }
  const decoded = Buffer.from(value, "base64");
  if (decoded.toString("base64") !== value || (expectedLength !== undefined && decoded.length !== expectedLength)) {
    throw new Error("Invalid encrypted result " + label);
  }
  return decoded;
}

function associatedData(messageId, keyId) {
  return Buffer.from("tokensaver-phone-result:v2:" + messageId + ":" + keyId, "utf8");
}

/**
 * Encrypted local result journal.
 *
 * keyProvider is an explicit trust boundary and must implement:
 *   getCurrentKey() -> Promise<{ keyId: string, key: Buffer|Uint8Array }>
 *   getKey(keyId)   -> Promise<Buffer|Uint8Array>
 *
 * The provider must use an OS-backed or remote KMS/HSM-backed key store in
 * production. This class deliberately does not read keys from files, JSON,
 * environment variables, or source code. Legacy plaintext v1 records are
 * rejected rather than silently treated as protected data.
 */
export class FilePhoneResultStore {
  constructor(directory, keyProvider) {
    if (typeof directory !== "string" || !directory.trim()) {
      throw new TypeError("A private result directory is required");
    }
    if (!keyProvider || typeof keyProvider.getCurrentKey !== "function" ||
        typeof keyProvider.getKey !== "function") {
      const error = new Error("An explicit secure result key provider is required");
      error.code = "RESULT_KEY_PROVIDER_REQUIRED";
      throw error;
    }
    this.directory = path.resolve(directory);
    this.keyProvider = keyProvider;
    this.initialized = false;
  }

  async initialize() {
    if (this.initialized) return this;
    await mkdir(this.directory, { recursive: true, mode: 0o700 });
    if (process.platform !== "win32") await chmod(this.directory, 0o700);
    const pathInfo = await lstat(this.directory);
    if (!pathInfo.isDirectory() || pathInfo.isSymbolicLink()) {
      throw new Error("Result store path must be a real directory, not a symlink or reparse point");
    }
    // Check the unresolved final path before realpath can hide a directory junction.
    if (process.platform === "win32") secureWindowsResultDirectory(this.directory);
    const resolved = await realpath(this.directory);
    const info = await stat(resolved);
    if (!info.isDirectory()) throw new Error("Result store path is not a directory");
    this.directory = resolved;
    this.initialized = true;
    return this;
  }

  filePath(messageId) {
    if (!UUID.test(messageId)) throw new TypeError("Invalid message UUID");
    return path.join(this.directory, messageId + ".json");
  }

  async encrypt(messageId, result) {
    const current = await this.keyProvider.getCurrentKey();
    const keyId = current?.keyId;
    const key = requireKey(current?.key, keyId);
    const nonce = randomBytes(NONCE_BYTES);
    const cipher = createCipheriv("aes-256-gcm", key, nonce, { authTagLength: TAG_BYTES });
    cipher.setAAD(associatedData(messageId, keyId));
    const plaintext = Buffer.from(JSON.stringify(result), "utf8");
    const ciphertext = Buffer.concat([cipher.update(plaintext), cipher.final()]);
    return {
      version: FORMAT_VERSION,
      algorithm: ALGORITHM,
      keyId,
      nonce: nonce.toString("base64"),
      ciphertext: ciphertext.toString("base64"),
      authTag: cipher.getAuthTag().toString("base64")
    };
  }

  async decrypt(messageId, record) {
    if (record?.version !== FORMAT_VERSION || record?.algorithm !== ALGORITHM ||
        typeof record.keyId !== "string" || !KEY_ID.test(record.keyId)) {
      throw new Error("Unsupported or corrupt encrypted result record");
    }
    const keyMaterial = await this.keyProvider.getKey(record.keyId);
    const key = requireKey(keyMaterial, record.keyId);
    const nonce = decodeBase64(record.nonce, NONCE_BYTES, "nonce");
    const ciphertext = decodeBase64(record.ciphertext, undefined, "ciphertext");
    const authTag = decodeBase64(record.authTag, TAG_BYTES, "authentication tag");
    try {
      const decipher = createDecipheriv("aes-256-gcm", key, nonce, { authTagLength: TAG_BYTES });
      decipher.setAAD(associatedData(messageId, record.keyId));
      decipher.setAuthTag(authTag);
      const plaintext = Buffer.concat([decipher.update(ciphertext), decipher.final()]);
      return JSON.parse(plaintext.toString("utf8"));
    } catch {
      throw new Error("Encrypted result authentication failed");
    }
  }

  async get(messageId) {
    await this.initialize();
    const file = this.filePath(messageId);
    let raw;
    try {
      const metadata = await lstat(file);
      if (!metadata.isFile() || metadata.isSymbolicLink()) throw new Error("Result record must be a regular file");
    } catch (error) {
      if (error.code === "ENOENT") return null;
      throw error;
    }
    try {
      raw = await readFile(file, "utf8");
    } catch (error) {
      if (error.code === "ENOENT") return null;
      throw error;
    }
    const record = JSON.parse(raw);
    if (record?.version !== FORMAT_VERSION || record?.messageId !== messageId || !record?.encrypted || typeof record.encrypted !== "object") {
      throw new Error("Corrupt, plaintext, or mismatched durable result record");
    }
    const result = await this.decrypt(messageId, record.encrypted);
    return { messageId, result, durable: true, committedAt: record.committedAt };
  }

  async commit(messageId, result) {
    await this.initialize();
    const destination = this.filePath(messageId);
    const existing = await this.get(messageId);
    if (existing) return existing;

    const encrypted = await this.encrypt(messageId, result);
    const serialized = JSON.stringify({
      version: FORMAT_VERSION,
      messageId,
      committedAt: new Date().toISOString(),
      encrypted
    });
    if (typeof serialized !== "string") throw new TypeError("Result must be JSON-serializable");

    const temporary = path.join(this.directory, "." + messageId + "." + randomUUID() + ".tmp");
    let handle;
    try {
      handle = await open(temporary, "wx", 0o600);
      await handle.writeFile(serialized, "utf8");
      await handle.sync();
      await handle.close();
      handle = undefined;

      try {
        await link(temporary, destination);
      } catch (error) {
        if (error.code !== "EEXIST") throw error;
        const winner = await this.get(messageId);
        if (winner) return winner;
        throw new Error("Concurrent result publication did not produce a readable record");
      }

      if (process.platform !== "win32") {
        const directoryHandle = await open(this.directory, "r");
        try {
          await directoryHandle.sync();
        } finally {
          await directoryHandle.close();
        }
      }

      return await this.get(messageId);
    } finally {
      if (handle) await handle.close().catch(() => {});
      await rm(temporary, { force: true }).catch(() => {});
    }
  }
}
