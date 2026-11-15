/**
 * Server-backed Phone mission authorization adapter.
 *
 * It never creates tasks or acquires leases from inbox content. The sender
 * principal is pinned by deployment configuration; the executor identity,
 * task owner and live fencing token are verified against Team Room.
 */
import { TeamRoomClient } from "./team-room-client.mjs";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export class PhoneMissionAuthorizerError extends Error {
  constructor(message, code = "MISSION_AUTHORIZATION_FAILED", cause) {
    super(message, cause ? { cause } : undefined);
    this.name = "PhoneMissionAuthorizerError";
    this.code = code;
  }
}

export class TeamRoomPhoneMissionAuthorizer {
  constructor({
    teamRoomClient = new TeamRoomClient(),
    authorizedSenderPrincipalId = process.env.TEAM_ROOM_AUTHORIZED_SENDER_PRINCIPAL_ID
  } = {}) {
    if (typeof authorizedSenderPrincipalId !== "string" || !UUID.test(authorizedSenderPrincipalId)) {
      throw new PhoneMissionAuthorizerError(
        "TEAM_ROOM_AUTHORIZED_SENDER_PRINCIPAL_ID must be a configured UUID",
        "AUTHORIZER_CONFIG_REQUIRED"
      );
    }
    if (!teamRoomClient || typeof teamRoomClient.authenticate !== "function" ||
        typeof teamRoomClient.lookup !== "function" ||
        typeof teamRoomClient.assertCurrentOwner !== "function") {
      throw new PhoneMissionAuthorizerError("A trusted TeamRoomClient is required", "AUTHORIZER_CONFIG_INVALID");
    }
    this.teamRoom = teamRoomClient;
    this.authorizedSenderPrincipalId = authorizedSenderPrincipalId;
  }

  async authorize(message, envelope) {
    if (message?.sender_principal_id !== this.authorizedSenderPrincipalId) {
      throw new PhoneMissionAuthorizerError(
        "Mailbox sender is not the configured mission coordinator",
        "AUTHORIZATION_LOST"
      );
    }

    let identity;
    let task;
    try {
      identity = await this.teamRoom.authenticate();
      task = await this.teamRoom.lookup(envelope.mission_id);
      if (!task || task.task_id !== envelope.task_id) {
        throw new PhoneMissionAuthorizerError(
          "Mission envelope task does not match the authoritative Team Room task",
          "AUTHORIZATION_LOST"
        );
      }
      this.teamRoom.assertCurrentOwner(task, identity, task.fencing_token);
    } catch (error) {
      if (error instanceof PhoneMissionAuthorizerError) throw error;
      throw new PhoneMissionAuthorizerError(
        "Team Room could not confirm current mission ownership",
        error?.code === "LOSS_OF_OWNERSHIP" || error?.code === "IDENTITY_MISMATCH"
          ? "AUTHORIZATION_LOST" : "TEAM_ROOM_UNAVAILABLE",
        error
      );
    }

    return {
      missionId: envelope.mission_id,
      taskId: task.task_id,
      senderPrincipalId: message.sender_principal_id,
      ownerPrincipalId: identity.principal_id,
      fencingToken: task.fencing_token,
      leaseExpiresAt: task.lease_expires_at
    };
  }
}
