import { chmod, lstat, open, link, mkdir, readFile, realpath, rm, stat } from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

/**
 * Local durable result journal.
 *
 * Uses exclusive hard-link publication so a committed message result is never
 * overwritten. The temporary file is fsynced before publication. On POSIX the
 * containing directory is also fsynced before durability is reported.
 * This store is local plaintext storage: callers must choose a private,
 * access-controlled directory and must not place credentials in results.
 */
export class FilePhoneResultStore {
  constructor(directory) {
    if (typeof directory !== "string" || !directory.trim()) {
      throw new TypeError("A private result directory is required");
    }
    this.directory = path.resolve(directory);
    this.initialized = false;
  }

  async initialize() {
    if (this.initialized) return this;
    await mkdir(this.directory, { recursive: true, mode: 0o700 });
    if (process.platform !== "win32") await chmod(this.directory, 0o700);
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
    if (record?.messageId !== messageId || record?.version !== 1 ||
        !Object.prototype.hasOwnProperty.call(record, "result")) {
      throw new Error("Corrupt or mismatched durable result record");
    }
    return { messageId, result: record.result, durable: true, committedAt: record.committedAt };
  }

  async commit(messageId, result) {
    await this.initialize();
    const destination = this.filePath(messageId);
    const existing = await this.get(messageId);
    if (existing) return existing;

    const serialized = JSON.stringify({
      version: 1,
      messageId,
      committedAt: new Date().toISOString(),
      result
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
