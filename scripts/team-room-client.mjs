/**
 * Minimal fail-closed client for the authenticated Team Room task API.
 * The opaque local session token is injected by the runtime; it is never
 * persisted in mission state or included in diagnostics.
 */
import crypto from "node:crypto";

const UUID_NAMESPACE = Buffer.from("8f3d7a4b6c2e41a9b5d0f1738c4e2a61", "hex");
const uuidFromMission = (missionId) => {
  const digest = crypto.createHash("sha1").update(UUID_NAMESPACE).update(String(missionId), "utf8").digest();
  digest[6] = (digest[6] & 0x0f) | 0x50;
  digest[8] = (digest[8] & 0x3f) | 0x80;
  const hex = digest.subarray(0, 16).toString("hex");
  return `${hex.slice(0,8)}-${hex.slice(8,12)}-${hex.slice(12,16)}-${hex.slice(16,20)}-${hex.slice(20)}`;
};

export class TeamRoomError extends Error {
  constructor(message, status = 0, code = "TEAM_ROOM_ERROR") {
    super(message);
    this.name = "TeamRoomError";
    this.status = status;
    this.code = code;
  }
}

export class TeamRoomClient {
  constructor(env = process.env) {
    this.baseUrl = (env.TEAM_ROOM_API_URL || "").replace(/\/+$/, "");
    this.token = env.TEAM_ROOM_SESSION_TOKEN || "";
    this.roomId = env.TEAM_ROOM_ROOM_ID || "";
    const configuredLeaseSeconds = env.TEAM_ROOM_LEASE_SECONDS ? Number(env.TEAM_ROOM_LEASE_SECONDS) : 60;
    if (!Number.isInteger(configuredLeaseSeconds) || configuredLeaseSeconds < 15 || configuredLeaseSeconds > 300) {
      throw new TeamRoomError("TEAM_ROOM_LEASE_SECONDS must be an integer from 15 to 300", 0, "CONFIG_INVALID");
    }
    this.leaseSeconds = configuredLeaseSeconds;
    if (!this.baseUrl || !this.token || !this.roomId) {
      throw new TeamRoomError("TEAM_ROOM_API_URL, TEAM_ROOM_SESSION_TOKEN and TEAM_ROOM_ROOM_ID are required; remote authority is unavailable", 0, "CONFIG_REQUIRED");
    }
    let parsed;
    try { parsed = new URL(this.baseUrl); } catch { throw new TeamRoomError("TEAM_ROOM_API_URL must be an absolute URL", 0, "CONFIG_INVALID"); }
    if (!["https:", "http:"].includes(parsed.protocol) || (parsed.protocol === "http:" && !["localhost","127.0.0.1","::1"].includes(parsed.hostname))) {
      throw new TeamRoomError("Team Room API must use HTTPS except loopback development", 0, "INSECURE_API_URL");
    }
  }

  async request(method, route, body) {
    let response;
    try {
      response = await fetch(this.baseUrl + route, {
        method,
        headers: { authorization: "Bearer " + this.token, accept: "application/json", ...(body === undefined ? {} : {"content-type":"application/json"}) },
        ...(body === undefined ? {} : {body: JSON.stringify(body)}),
        signal: AbortSignal.timeout(10000)
      });
    } catch {
      throw new TeamRoomError("Team Room request failed; execution is fail-closed", 0, "NETWORK_FAILURE");
    }
    const raw = await response.text();
    let payload = {};
    if (raw) { try { payload = JSON.parse(raw); } catch { throw new TeamRoomError("Team Room returned invalid JSON", response.status, "INVALID_RESPONSE"); } }
    if (!response.ok) {
      const code = response.status === 412 || response.status === 401 || response.status === 403 ? "LOSS_OF_OWNERSHIP" : "HTTP_ERROR";
      throw new TeamRoomError(`Team Room rejected ${method} ${route} (HTTP ${response.status})`, response.status, code);
    }
    return payload;
  }

  async authenticate() {
    const identity = await this.request("GET", "/auth/me");
    if (!identity.session_id || !identity.execution_id || !identity.participant_id || !identity.principal_id || identity.room_id !== this.roomId) {
      throw new TeamRoomError("Authenticated session identity does not match configured room", 403, "IDENTITY_MISMATCH");
    }
    return identity;
  }

  missionUuid(missionId) { return uuidFromMission(missionId); }

  async lookup(missionId) {
    return this.request("GET", `/team-room/tasks/by-mission/${this.missionUuid(missionId)}`);
  }

  async ensureTask(mission, identity) {
    const missionId = this.missionUuid(mission.mission_id);
    try { return await this.lookup(mission.mission_id); }
    catch (error) {
      if (!(error instanceof TeamRoomError) || error.status !== 404) throw error;
    }
    const created = await this.request("POST", "/team-room/tasks", {
      mission_id: missionId,
      title: String(mission.title || mission.mission_id).slice(0, 500),
      checkpoint: mission.checkpoint || {}
    });
    if (created.mission_id !== missionId) throw new TeamRoomError("Task create returned a different mission identity", 502, "IDENTITY_MISMATCH");
    return created;
  }

  assertCurrentOwner(task, identity, fencingToken) {
    const expires = task.lease_expires_at ? Date.parse(task.lease_expires_at) : 0;
    const valid = task.status === "OWNED" || task.status === "REASSIGNED";
    if (!valid || task.owner_principal_id !== identity.principal_id ||
        task.owner_participant_id !== identity.participant_id ||
        task.owner_session_id !== identity.session_id ||
        task.owner_execution_id !== identity.execution_id ||
        !Number.isSafeInteger(task.fencing_token) || task.fencing_token < 1 ||
        (fencingToken !== undefined && task.fencing_token !== fencingToken) ||
        !Number.isFinite(expires) || expires <= Date.now()) {
      throw new TeamRoomError("PostgreSQL does not confirm this Runner as the current live owner", 412, "LOSS_OF_OWNERSHIP");
    }
    return task;
  }

  async acquire(taskId, leaseSeconds = this.leaseSeconds) {
    return this.request("POST", `/team-room/tasks/${taskId}/lease/acquire`, {lease_seconds: leaseSeconds});
  }

  async start(mission) {
    const identity = await this.authenticate();
    const task = await this.ensureTask(mission, identity);
    const grant = await this.acquire(task.task_id,this.leaseSeconds);
    this.assertCurrentOwner(grant.task, identity);
    return {identity, task: grant.task, fencing_token: grant.task.fencing_token};
  }

  async resume(mission, localExecution) {
    const identity = await this.authenticate();
    const task = await this.lookup(mission.mission_id);
    const expiry = task.lease_expires_at ? Date.parse(task.lease_expires_at) : 0;
    const isSelf = task.owner_principal_id === identity.principal_id &&
      task.owner_participant_id === identity.participant_id &&
      task.owner_session_id === identity.session_id &&
      task.owner_execution_id === identity.execution_id;
    if (isSelf && expiry > Date.now()) {
      const localFence = localExecution?.fencing_token;
      if (!Number.isSafeInteger(localFence) || localFence !== task.fencing_token) {
        throw new TeamRoomError("Local checkpoint fencing reference differs from PostgreSQL; resume denied", 412, "LOSS_OF_OWNERSHIP");
      }
      this.assertCurrentOwner(task, identity, localFence);
      return {identity, task, fencing_token: task.fencing_token};
    }
    if (task.owner_principal_id && expiry > Date.now()) {
      throw new TeamRoomError("Task is owned by another authenticated executor; resume denied", 412, "LOSS_OF_OWNERSHIP");
    }
    // An unowned or expired task still requires an explicit PostgreSQL acquire.
    const grant = await this.acquire(task.task_id,this.leaseSeconds);
    this.assertCurrentOwner(grant.task, identity);
    return {identity, task: grant.task, fencing_token: grant.task.fencing_token};
  }

  async presence() { return this.request("POST", "/team-room/heartbeat"); }

  async renew(lease) {
    const identity = await this.authenticate();
    const task = await this.request("POST", `/team-room/tasks/${lease.task_id}/lease/renew`, {fencing_token: lease.fencing_token});
    this.assertCurrentOwner(task, identity, lease.fencing_token);
    return task;
  }

  async checkpoint(lease, checkpoint) {
    const identity = await this.authenticate();
    const task = await this.request("POST", `/team-room/tasks/${lease.task_id}/checkpoint`, {fencing_token: lease.fencing_token, checkpoint});
    this.assertCurrentOwner(task, identity, lease.fencing_token);
    return task;
  }

  async complete(lease, checkpoint) {
    const identity = await this.authenticate();
    const task = await this.request("POST", `/team-room/tasks/${lease.task_id}/complete`, {fencing_token: lease.fencing_token, checkpoint});
    if (task.status !== "COMPLETED" || task.fencing_token !== lease.fencing_token) throw new TeamRoomError("Team Room did not confirm completion", 502, "INVALID_RESPONSE");
    return task;
  }

  async release(lease) {
    const identity = await this.authenticate();
    const task = await this.request("POST", `/team-room/tasks/${lease.task_id}/lease/release`, {fencing_token: lease.fencing_token});
    if (task.status !== "UNOWNED" || task.fencing_token !== lease.fencing_token) throw new TeamRoomError("Team Room did not confirm release", 502, "INVALID_RESPONSE");
    return task;
  }
}
