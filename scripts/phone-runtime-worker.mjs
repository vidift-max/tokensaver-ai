/**
 * Sequential, fail-closed mailbox worker.
 *
 * The executor MUST implement idempotency for the supplied message ID.
 * The result store MUST durably and atomically commit one result per message ID.
 * ACK is sent only after the durable commit has completed.
 */
export class PhoneRuntimeWorkerError extends Error {
  constructor(message, code = "PHONE_WORKER_ERROR", cause) {
    super(message, cause ? { cause } : undefined);
    this.name = "PhoneRuntimeWorkerError";
    this.code = code;
  }
}

export class PhoneRuntimeWorker {
  constructor({ client, executor, resultStore, signal } = {}) {
    if (!client || typeof client.poll !== "function" ||
        typeof client.acknowledge !== "function") {
      throw new PhoneRuntimeWorkerError("A PhoneRuntimeClient is required", "CLIENT_REQUIRED");
    }
    if (!executor || typeof executor.execute !== "function") {
      throw new PhoneRuntimeWorkerError("An explicitly configured executor is required", "EXECUTOR_REQUIRED");
    }
    if (!resultStore || typeof resultStore.get !== "function" ||
        typeof resultStore.commit !== "function") {
      throw new PhoneRuntimeWorkerError("A durable resultStore with get and commit is required", "STORE_REQUIRED");
    }
    this.client = client;
    this.executor = executor;
    this.resultStore = resultStore;
    this.signal = signal;
  }

  assertRunning() {
    if (this.signal?.aborted) {
      throw new PhoneRuntimeWorkerError("Worker stopped by abort signal", "ABORTED");
    }
  }

  async processMessage(message) {
    this.assertRunning();
    if (!message || typeof message.message_id !== "string" ||
        !/^[0-9a-f-]{36}$/i.test(message.message_id) ||
        !["PENDING", "DELIVERED"].includes(message.message_state)) {
      throw new PhoneRuntimeWorkerError("Malformed or non-processable mailbox message", "INVALID_MESSAGE");
    }

    const messageId = message.message_id;
    let record = await this.resultStore.get(messageId);
    this.assertRunning();

    if (!record) {
      let result;
      try {
        result = await this.executor.execute(message, {
          idempotencyKey: messageId,
          signal: this.signal
        });
      } catch (error) {
        throw new PhoneRuntimeWorkerError(
          "Executor failed; message remains unacknowledged",
          "EXECUTOR_FAILED",
          error
        );
      }
      this.assertRunning();

      try {
        record = await this.resultStore.commit(messageId, result);
      } catch (error) {
        throw new PhoneRuntimeWorkerError(
          "Durable result commit failed; message remains unacknowledged",
          "DURABLE_COMMIT_FAILED",
          error
        );
      }
      if (!record || record.messageId !== messageId || record.durable !== true) {
        throw new PhoneRuntimeWorkerError(
          "Result store did not confirm a durable commit",
          "DURABILITY_UNCONFIRMED"
        );
      }
    } else if (record.messageId !== messageId || record.durable !== true) {
      throw new PhoneRuntimeWorkerError("Stored result is not a valid durable record", "INVALID_STORED_RESULT");
    }

    this.assertRunning();
    try {
      await this.client.acknowledge(messageId);
    } catch (error) {
      if (error?.code === "AUTHORIZATION_LOST") {
        throw new PhoneRuntimeWorkerError("Authorization lost; worker must stop", "AUTHORIZATION_LOST", error);
      }
      throw new PhoneRuntimeWorkerError(
        "Result is durable but ACK failed; retry will reuse stored result",
        "ACK_FAILED",
        error
      );
    }
    return { messageId, state: "ACKNOWLEDGED", recovered: false };
  }

  async runOnce() {
    this.assertRunning();
    let messages;
    try {
      messages = await this.client.poll();
    } catch (error) {
      if (error?.code === "AUTHORIZATION_LOST") {
        throw new PhoneRuntimeWorkerError("Authorization lost; worker must stop", "AUTHORIZATION_LOST", error);
      }
      throw error;
    }
    const outcomes = [];
    for (const message of messages) {
      this.assertRunning();
      outcomes.push(await this.processMessage(message));
    }
    return outcomes;
  }
}
