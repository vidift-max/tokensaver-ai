/**
 * Sequential, fail-closed mailbox worker.
 *
 * Every message must pass strict envelope validation and a server-backed
 * authorizer before any result-store read, executor call, or ACK. The
 * authorizer must verify sender identity, task assignment, live lease and
 * fencing token against the authoritative Team Room service.
 */
import { validatePhoneMissionEnvelope, PhoneMissionEnvelopeError } from "./phone-mission-envelope.mjs";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export class PhoneRuntimeWorkerError extends Error {
  constructor(message, code = "PHONE_WORKER_ERROR", cause) {
    super(message, cause ? { cause } : undefined);
    this.name = "PhoneRuntimeWorkerError";
    this.code = code;
  }
}

function assertAuthorization(authorization, message, envelope) {
  const expiry = Date.parse(authorization?.leaseExpiresAt);
  if (!authorization || authorization.missionId !== envelope.mission_id ||
      authorization.taskId !== envelope.task_id ||
      authorization.senderPrincipalId !== message.sender_principal_id ||
      !UUID.test(authorization.senderPrincipalId || "") ||
      !UUID.test(authorization.ownerPrincipalId || "") ||
      !Number.isSafeInteger(authorization.fencingToken) || authorization.fencingToken < 1 ||
      !Number.isFinite(expiry) || expiry <= Date.now()) {
    throw new PhoneRuntimeWorkerError(
      "Team Room did not provide current, message-bound authorization",
      "MISSION_AUTHORIZATION_INVALID"
    );
  }
  return {
    missionId: authorization.missionId,
    taskId: authorization.taskId,
    senderPrincipalId: authorization.senderPrincipalId,
    ownerPrincipalId: authorization.ownerPrincipalId,
    fencingToken: authorization.fencingToken,
    leaseExpiresAt: authorization.leaseExpiresAt
  };
}

function sameFence(a, b) {
  return a.missionId === b.missionId && a.taskId === b.taskId &&
    a.senderPrincipalId === b.senderPrincipalId &&
    a.ownerPrincipalId === b.ownerPrincipalId &&
    a.fencingToken === b.fencingToken;
}

export class PhoneRuntimeWorker {
  constructor({ client, executor, resultStore, authorizer, signal } = {}) {
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
    if (!authorizer || typeof authorizer.authorize !== "function") {
      throw new PhoneRuntimeWorkerError("A server-backed mission authorizer is required", "AUTHORIZER_REQUIRED");
    }
    this.client = client;
    this.executor = executor;
    this.resultStore = resultStore;
    this.authorizer = authorizer;
    this.signal = signal;
  }

  assertRunning() {
    if (this.signal?.aborted) {
      throw new PhoneRuntimeWorkerError("Worker stopped by abort signal", "ABORTED");
    }
  }

  async authorize(message, envelope) {
    this.assertRunning();
    let evidence;
    try {
      evidence = await this.authorizer.authorize(message, envelope, { signal: this.signal });
    } catch (error) {
      throw new PhoneRuntimeWorkerError(
        "Mission authorization failed; message remains unacknowledged",
        error?.code === "AUTHORIZATION_LOST" || error?.code === "LOSS_OF_OWNERSHIP"
          ? "AUTHORIZATION_LOST" : "MISSION_AUTHORIZATION_FAILED",
        error
      );
    }
    this.assertRunning();
    return assertAuthorization(evidence, message, envelope);
  }

  async processMessage(message) {
    this.assertRunning();
    let envelope;
    try {
      envelope = validatePhoneMissionEnvelope(message);
    } catch (error) {
      throw new PhoneRuntimeWorkerError(
        "Malformed mission envelope; message remains unacknowledged",
        error instanceof PhoneMissionEnvelopeError ? error.code : "INVALID_MISSION_ENVELOPE",
        error
      );
    }

    // Authority is checked before reading any persisted result. A stored result
    // is not a capability and cannot bypass current sender/owner authorization.
    const initialAuthorization = await this.authorize(message, envelope);
    const messageId = message.message_id;
    let record = await this.resultStore.get(messageId);
    this.assertRunning();
    const recovered = Boolean(record);
    let ackAuthorizationBaseline = initialAuthorization;

    if (!record) {
      // Revalidate immediately before invoking the executor to reduce the
      // authorization-to-execution window and reject lease/fence changes.
      const executionAuthorization = await this.authorize(message, envelope);
      if (!sameFence(initialAuthorization, executionAuthorization)) {
        throw new PhoneRuntimeWorkerError("Mission ownership changed before execution", "AUTHORIZATION_FENCE_CHANGED");
      }
      ackAuthorizationBaseline = executionAuthorization;

      let result;
      try {
        result = await this.executor.execute(message, {
          idempotencyKey: messageId,
          authorization: executionAuthorization,
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

    // Recheck live authority before ACK, including recovery of an old result.
    // The current owner must still hold the exact fencing generation that
    // authorized execution (or the stored-result recovery), not merely be the
    // same principal with a newer lease generation.
    const ackAuthorization = await this.authorize(message, envelope);
    if (!sameFence(ackAuthorizationBaseline, ackAuthorization)) {
      throw new PhoneRuntimeWorkerError(
        "Mission ownership changed before ACK; durable result remains unacknowledged",
        "AUTHORIZATION_FENCE_CHANGED"
      );
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
    return { messageId, state: "ACKNOWLEDGED", recovered };
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
    if (!Array.isArray(messages)) {
      throw new PhoneRuntimeWorkerError("Mailbox poll must return an array", "INVALID_MAILBOX_RESPONSE");
    }
    const outcomes = [];
    for (const message of messages) {
      this.assertRunning();
      outcomes.push(await this.processMessage(message));
    }
    return outcomes;
  }
}
