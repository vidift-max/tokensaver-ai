#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { TeamRoomClient } from "./team-room-client.mjs";

const ROOT = path.resolve(process.env.MISSION_STATE_DIR || ".mission");
const STATE_FILE = path.join(ROOT, "STATE.json");
const EVENTS_FILE = path.join(ROOT, "events.ndjson");
const LOCK_FILE = path.join(ROOT, ".state.lock");
const STATUSES = new Set(["CREATED","ASSIGNED","ACKNOWLEDGED","IN_PROGRESS","WAITING_REVIEW","BLOCKED","DONE","REJECTED","CANCELLED","DISCONNECTED","RECOVERY_REQUIRED","WAITING_HANDOFF","RESUMING"]);
const now = () => new Date().toISOString();
const fail = (message, code = 2) => { console.error("MISSION_RUNNER_ERROR: " + message); process.exitCode = code; };
const safeId = value => typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$/.test(value);
function ensureRoot() { fs.mkdirSync(ROOT, { recursive: true }); }
function lock() {
  ensureRoot();
  let fd;
  try { fd = fs.openSync(LOCK_FILE, "wx", 0o600); fs.writeSync(fd, JSON.stringify({ pid: process.pid, at: now() })); }
  catch { throw new Error("state lock exists; another runner may be active. Inspect and remove only after confirming no process owns it."); }
  return () => { try { fs.closeSync(fd); } finally { try { fs.unlinkSync(LOCK_FILE); } catch {} } };
}
function readState() {
  if (!fs.existsSync(STATE_FILE)) throw new Error("mission state not found; run init first");
  let s;
  try { s = JSON.parse(fs.readFileSync(STATE_FILE, "utf8")); } catch { throw new Error("STATE.json is corrupt or invalid JSON"); }
  if (!s || typeof s !== "object" || Array.isArray(s) || s.version !== 1 ||
      !s.missions || typeof s.missions !== "object" || Array.isArray(s.missions) ||
      !safeId(s.current_mission_id) || !Object.hasOwn(s.missions, s.current_mission_id)) {
    throw new Error("unsupported or invalid mission state schema");
  }
  for (const [id, mission] of Object.entries(s.missions)) {
    if (!safeId(id) || !mission || typeof mission !== "object" || Array.isArray(mission) ||
        mission.mission_id !== id || !Number.isSafeInteger(mission.mission_number) ||
        typeof mission.title !== "string" || !STATUSES.has(mission.status) ||
        !Array.isArray(mission.files_changed) || !Array.isArray(mission.commits) ||
        !Array.isArray(mission.tests) || !Array.isArray(mission.evidence) ||
        !Array.isArray(mission.blockers) || !mission.handoff || typeof mission.handoff !== "object" ||
        !mission.prompt || typeof mission.prompt !== "object") {
      throw new Error("invalid mission record in STATE.json: " + id);
    }
  }
  for (const mission of Object.values(s.missions)) {
    mission.presence ||= { state:"CONNECTED",session_id:null,connected_at:null,last_seen:null,timeout_ms:Number(process.env.MISSION_HEARTBEAT_TIMEOUT_MS)||60000,disconnected_at:null,reconnected_at:null };
    mission.execution ||= { lease_id:null,owner_ai:null,participant_id:null,lease_expires_at:null };
    mission.reassignment_queue ||= [];
    mission.checkpoint_history ||= mission.checkpoint ? [mission.checkpoint] : [];
  }
  return s;
}
function writeState(s, event, details = {}) {
  s.updated_at = now();
  const temp = STATE_FILE + "." + crypto.randomUUID() + ".tmp";
  const fd = fs.openSync(temp, "wx", 0o600);
  try { fs.writeFileSync(fd, JSON.stringify(s, null, 2) + "\n", "utf8"); fs.fsyncSync(fd); }
  finally { fs.closeSync(fd); }
  fs.renameSync(temp, STATE_FILE);
  fs.appendFileSync(EVENTS_FILE, JSON.stringify({ at: now(), event, mission_id: s.current_mission_id, details }) + "\n", { mode: 0o600 });
}
function current(s) {
  const m = s.missions[s.current_mission_id];
  if (!m) throw new Error("current mission does not exist");
  return m;
}
function transition(m, status) {
  if (!STATUSES.has(status)) throw new Error("invalid mission status");
  m.status = status; m.updated_at = now();
  if (status === "DONE") m.completed_at = now();
}
function createCheckpoint(m, description) {
  const cp={ checkpoint_id:crypto.randomUUID(), at:now(), mission_id:m.mission_id, task_id:m.current_task, responsible_ai:m.responsible_ai, participant_id:m.participant_id, session_id:m.presence.session_id, status:m.status, progress:m.progress, description, task:m.current_task, files_changed:[...m.files_changed], commits:[...m.commits], tests:[...m.tests], last_result:m.last_result, next_action:m.next_action, blockers:JSON.parse(JSON.stringify(m.blockers)) };
  m.checkpoint=cp; m.checkpoint_history.push(cp); return cp;
}
function usage() {
  console.log("Mission Runner (Team Room remote ownership; local state is metadata only)\nRequired for execution: TEAM_ROOM_API_URL, TEAM_ROOM_SESSION_TOKEN, TEAM_ROOM_ROOM_ID\nCommands:\n  init <id> <number> <title>\n  status\n  list\n  register-next <id> <number> <title>\n  switch <mission_id>\n  assign <responsible_ai> <participant_id>\n  acknowledge\n  start (remote acquire required)\n  heartbeat [session_id] (presence only)\n  renew (remote lease renewal)\n  checkpoint <description> (remote fenced write)\n  resume (remote ownership validation)\n  release (remote fenced release)\n  block <reason>\n  resolve-blockers\n  set-task <task>\n  record-file <path>\n  record-commit <sha>\n  record-test <result>\n  disconnect <reason>\n  handoff-create <target_ai>\n  handoff-accept <participant_id>\n  handoff-complete\n  submit-review <result>\n  approve\n  reject <reason>\n  cancel <reason>\n  complete <result> (direct close; use submit-review for review gate)\n  prompt-ready <next_mission_id> <prompt text>\n  prompt-delivered (disabled until verified provider dispatch exists)");
}
async function main(args) {
  const [cmd, ...a] = args;
  if (!cmd || cmd === "help" || cmd === "--help") return usage();
  const release = lock();
  try {
    if (cmd === "init") {
      if (a.length < 3 || !safeId(a[0]) || !/^\d+$/.test(a[1])) throw new Error("usage: init <safe-id> <number> <title>");
      if (fs.existsSync(STATE_FILE)) throw new Error("state already exists; refusing to overwrite");
      const stamp = now(), id = a[0];
      const mission = { mission_id:id, mission_number:Number(a[1]), title:a.slice(2).join(" "), responsible_ai:null, participant_id:null, current_task:null, subtasks:[], presence:{state:"CONNECTED",session_id:null,connected_at:null,last_seen:null,timeout_ms:Number(process.env.MISSION_HEARTBEAT_TIMEOUT_MS)||60000,disconnected_at:null,reconnected_at:null},execution:{lease_id:null,owner_ai:null,participant_id:null,lease_expires_at:null},reassignment_queue:[],checkpoint_history:[], status:"CREATED", progress:0, last_action:"initialized", last_result:null, files_changed:[], commits:[], tests:[], evidence:[], blockers:[], next_action:"assign responsible AI", checkpoint:null, started_at:null, updated_at:stamp, completed_at:null, handoff:{status:"NONE",target_ai:null,participant_id:null,created_at:null,accepted_at:null,completed_at:null}, prompt:{status:"NONE",next_mission_id:null,path:null,delivery_evidence:null} };
      const s = { version:1, current_mission_id:id, missions:{[id]:mission}, updated_at:stamp };
      writeState(s,"MISSION_CREATED",{mission_number:mission.mission_number}); console.log("CREATED " + id); return;
    }
    const s = readState();
    if (cmd === "status") { console.log(JSON.stringify(s,null,2)); return; }
    if (cmd === "list") {
      const rows=Object.values(s.missions).map(m=>({mission_id:m.mission_id,mission_number:m.mission_number,title:m.title,status:m.status,responsible_ai:m.responsible_ai,updated_at:m.updated_at,next_action:m.next_action}));
      console.log(JSON.stringify(rows,null,2)); return;
    }
    if (cmd === "register-next") {
      const previous=current(s);
      if (previous.status!=="DONE") throw new Error("current mission must be DONE before registering its successor");
      if (a.length<3 || !safeId(a[0]) || !/^\d+$/.test(a[1])) throw new Error("usage: register-next <safe-id> <number> <title>");
      if (s.missions[a[0]]) throw new Error("mission already exists; refusing duplicate registration");
      const stamp=now(), id=a[0];
      s.missions[id]={mission_id:id,mission_number:Number(a[1]),title:a.slice(2).join(" "),responsible_ai:null,participant_id:null,current_task:null,subtasks:[],presence:{state:"CONNECTED",session_id:null,connected_at:null,last_seen:null,timeout_ms:Number(process.env.MISSION_HEARTBEAT_TIMEOUT_MS)||60000,disconnected_at:null,reconnected_at:null},execution:{lease_id:null,owner_ai:null,participant_id:null,lease_expires_at:null},reassignment_queue:[],checkpoint_history:[],status:"CREATED",progress:0,last_action:"registered",last_result:null,files_changed:[],commits:[],tests:[],evidence:[],blockers:[],next_action:"assign responsible AI",checkpoint:null,started_at:null,updated_at:stamp,completed_at:null,handoff:{status:"NONE",target_ai:null,participant_id:null,created_at:null,accepted_at:null,completed_at:null},prompt:{status:"NONE",next_mission_id:null,path:null,delivery_evidence:null}};
      s.current_mission_id=id; writeState(s,"NEXT_MISSION_REGISTERED",{previous_mission_id:previous.mission_id,next_mission_id:id}); return;
    }
    if (cmd === "switch") {
      if (!s.missions[a[0]]) throw new Error("mission does not exist");
      const old=current(s);
      if (!["DONE","CANCELLED","REJECTED"].includes(old.status)) throw new Error("finish or cancel current mission before switching");
      s.current_mission_id=a[0]; writeState(s,"CURRENT_MISSION_SWITCHED",{mission_id:a[0]}); return;
    }
    const m = current(s);
    if (cmd === "assign") {
      if (a.length !== 2 || !a[0].trim() || !a[1].trim()) throw new Error("usage: assign <responsible_ai> <participant_id>");
      m.responsible_ai=a[0]; m.participant_id=a[1]; transition(m,"ASSIGNED"); m.next_action="responsible AI must acknowledge"; writeState(s,"MISSION_ASSIGNED"); return;
    }
    if (cmd === "acknowledge") { if (m.status!=="ASSIGNED") throw new Error("mission must be ASSIGNED"); transition(m,"ACKNOWLEDGED"); m.next_action="start mission"; writeState(s,"MISSION_ACKNOWLEDGED"); return; }
    if (cmd === "start") {
      if (["PENDING","ACCEPTED","HANDOFF_CREATED","HANDOFF_ACCEPTED"].includes(m.handoff.status)) throw new Error("pending or accepted handoff prevents an independent start");
      if (!["ACKNOWLEDGED","DISCONNECTED","BLOCKED","RESUMING"].includes(m.status)) throw new Error("mission must be ACKNOWLEDGED, DISCONNECTED, RESUMING or unblocked BLOCKED");
      if(m.status==="BLOCKED" && m.blockers.length) throw new Error("resolve blockers before start");
      // Local state is metadata only. PostgreSQL must grant ownership before IN_PROGRESS.
      const remote = await new TeamRoomClient().start(m);
      transition(m,"IN_PROGRESS"); m.started_at=m.started_at||now(); m.presence.state="ACTIVE"; m.presence.connected_at=m.presence.connected_at||now(); m.presence.last_seen=now();
      m.presence.session_id=remote.identity.session_id;
      m.execution={task_id:remote.task.task_id,fencing_token:remote.fencing_token,owner_principal_id:remote.identity.principal_id,owner_participant_id:remote.identity.participant_id,owner_session_id:remote.identity.session_id,owner_execution_id:remote.identity.execution_id,lease_expires_at:remote.task.lease_expires_at,ownership_state:"OWNED"};
      m.current_task_id=remote.task.task_id;
      m.next_action="execute only while Team Room lease is valid; renew/checkpoint remotely"; writeState(s,"MISSION_STARTED_REMOTE",{task_id:remote.task.task_id,fencing_token:remote.fencing_token}); return;
    }
    if (cmd === "checkpoint") {
      if (m.status!=="IN_PROGRESS" || !a.length) throw new Error("usage: checkpoint <description>; mission must be IN_PROGRESS");
      const cp=createCheckpoint(m,a.join(" "));
      const lease=m.execution;
      if (!lease?.task_id || !Number.isSafeInteger(lease.fencing_token) || lease.ownership_state==="LOSS_OF_OWNERSHIP") throw new Error("valid remote lease reference required; checkpoint denied");
      try {
        await new TeamRoomClient().checkpoint(lease,{mission_id:m.mission_id,task_id:lease.task_id,session_id:lease.owner_session_id,workload_execution_id:lease.owner_execution_id,fencing_token:lease.fencing_token,checkpoint:cp,context:{title:m.title,current_task:m.current_task,progress:m.progress},timestamp:now()});
      } catch (error) { lease.ownership_state="LOSS_OF_OWNERSHIP"; writeState(s,"REMOTE_CHECKPOINT_REJECTED",{code:error.code||"REMOTE_ERROR"}); throw error; }
      m.last_action="checkpoint"; m.progress=Math.min(99,Math.max(m.progress,1)); m.next_action="resume from latest remote checkpoint"; writeState(s,"CHECKPOINT_CREATED_REMOTE",{checkpoint_id:cp.checkpoint_id,fencing_token:lease.fencing_token}); return;
    }
    if (cmd === "heartbeat") {
      if (!["IN_PROGRESS","ACKNOWLEDGED","DISCONNECTED","RECOVERY_REQUIRED","RESUMING"].includes(m.status)) throw new Error("mission is not accepting a heartbeat in its current state");
      const client=new TeamRoomClient();
      const identity=await client.authenticate();
      if (a[0] && a[0]!==identity.session_id) throw new Error("provided session_id does not match authenticated Team Room session");
      await client.presence();
      const wasDisconnected=["DISCONNECTED","SUSPECTED_DISCONNECTED"].includes(m.presence.state);
      m.presence.session_id=identity.session_id; m.presence.last_seen=now(); m.presence.state=wasDisconnected?"RECONNECTED":"ACTIVE";
      if (!m.presence.connected_at || wasDisconnected) m.presence.connected_at=now();
      if (wasDisconnected) m.presence.reconnected_at=now();
      // Presence heartbeat never renews or fabricates the task lease.
      m.last_action="presence_heartbeat"; writeState(s,"PRESENCE_HEARTBEAT_REMOTE",{presence:m.presence.state}); return;
    }
    if (cmd === "renew") {
      if (m.status!=="IN_PROGRESS" || !m.execution?.task_id || !Number.isSafeInteger(m.execution?.fencing_token) || m.execution.ownership_state==="LOSS_OF_OWNERSHIP") throw new Error("active remote lease required");
      try {
        const task=await new TeamRoomClient().renew(m.execution);
        m.execution.lease_expires_at=task.lease_expires_at; m.execution.ownership_state="OWNED";
        writeState(s,"TASK_LEASE_RENEWED_REMOTE",{task_id:m.execution.task_id,fencing_token:m.execution.fencing_token,lease_expires_at:task.lease_expires_at}); return;
      } catch(error) {
        m.execution.ownership_state="LOSS_OF_OWNERSHIP";
        writeState(s,"TASK_LEASE_RENEW_REJECTED",{code:error.code||"REMOTE_ERROR"});
        throw error;
      }
    }
    if (cmd === "release") {
      if (!m.execution?.task_id || !Number.isSafeInteger(m.execution?.fencing_token)) throw new Error("remote lease reference required");
      try {
        const task=await new TeamRoomClient().release(m.execution);
        m.execution.ownership_state="RELEASED"; m.execution.lease_expires_at=null;
        writeState(s,"TASK_LEASE_RELEASED_REMOTE",{task_id:task.task_id,fencing_token:task.fencing_token}); return;
      } catch(error) {
        m.execution.ownership_state="LOSS_OF_OWNERSHIP";
        writeState(s,"TASK_LEASE_RELEASE_REJECTED",{code:error.code||"REMOTE_ERROR"});
        throw error;
      }
    }
    if (cmd === "presence") {
      if (a[0] && (!/^\d+$/.test(a[0]) || Number(a[0])<1000)) throw new Error("timeout_ms must be an integer >= 1000");
      if (a[0]) m.presence.timeout_ms=Number(a[0]);
      if (m.presence.last_seen) {
        const age=Date.now()-Date.parse(m.presence.last_seen), timeout=m.presence.timeout_ms;
        if (age>timeout*2) {
          m.presence.state="DISCONNECTED";
          if (m.status==="IN_PROGRESS") {
            transition(m,"RECOVERY_REQUIRED");
            m.next_action="recover latest checkpoint and enqueue reassignment";
            createCheckpoint(m,"automatic heartbeat-timeout checkpoint");
          }
          if (!m.presence.disconnected_at) m.presence.disconnected_at=now();
          if (!m.reassignment_queue.some(item=>item.status==="QUEUED"||item.status==="ASSIGNED")) m.reassignment_queue.push({recovery_id:crypto.randomUUID(),status:"QUEUED",created_at:now(),reason:"heartbeat_timeout",checkpoint_id:m.checkpoint?.checkpoint_id||null});
          writeState(s,"PRESENCE_DISCONNECTED",{age_ms:age});
        } else if (age>timeout) {
          m.presence.state="SUSPECTED_DISCONNECTED"; writeState(s,"PRESENCE_SUSPECTED_DISCONNECTED",{age_ms:age});
        } else if (age>timeout/2) {
          m.presence.state="IDLE"; writeState(s,"PRESENCE_IDLE",{age_ms:age});
        } else {
          m.presence.state="ACTIVE"; writeState(s,"PRESENCE_ACTIVE",{age_ms:age});
        }
      }
      console.log(JSON.stringify({presence:m.presence,recovery_required:m.status==="RECOVERY_REQUIRED"},null,2)); return;
    }
    if (cmd === "recover") {
      if (!["DISCONNECTED","RECOVERY_REQUIRED"].includes(m.status)) throw new Error("recovery requires a disconnected mission");
      if (!m.checkpoint) throw new Error("recovery requires a persisted checkpoint");
      transition(m,"RECOVERY_REQUIRED"); m.next_action="assign a replacement responsible AI through handoff";
      if (!m.reassignment_queue.some(item=>item.status==="QUEUED"||item.status==="ASSIGNED")) m.reassignment_queue.push({recovery_id:crypto.randomUUID(),status:"QUEUED",created_at:now(),reason:"explicit_recovery",checkpoint_id:m.checkpoint.checkpoint_id});
      writeState(s,"MISSION_RECOVERY_REQUIRED",{checkpoint_id:m.checkpoint.checkpoint_id}); return;
    }
    if (cmd === "reassign-queue") { console.log(JSON.stringify(m.reassignment_queue,null,2)); return; }
    if (cmd === "resume") {
      if (["ACCEPTED","HANDOFF_ACCEPTED"].includes(m.handoff.status) && (a[0]!==m.responsible_ai || a[1]!==m.participant_id)) throw new Error("TASK_REASSIGNED: only the accepted responsible AI may resume");
      if (["PENDING","HANDOFF_CREATED"].includes(m.handoff.status)) throw new Error("pending handoff must be accepted before resume");
      if (!["DISCONNECTED","BLOCKED","RECOVERY_REQUIRED","RESUMING"].includes(m.status)) throw new Error("mission is not resumable from current status");
      if (m.status==="BLOCKED" && m.blockers.some(blocker=>blocker.status==="OPEN")) throw new Error("resolve blockers before resume");
      if (!m.checkpoint && m.status!=="DISCONNECTED") throw new Error("no checkpoint available");
      if (m.status === "DISCONNECTED") {
        const disconnect = [...m.blockers].reverse().find(item => item.kind === "DISCONNECT" && item.status === "RECORDED");
        if (disconnect) { disconnect.status = "RESUMED"; disconnect.resumed_at = now(); }
      }
      // A local checkpoint is context only; the server decides whether this executor owns the task.
      const remote=await new TeamRoomClient().resume(m,m.execution);
      transition(m,"IN_PROGRESS"); m.presence.state="RECONNECTED"; m.presence.reconnected_at=now(); m.presence.last_seen=now(); m.presence.session_id=remote.identity.session_id;
      m.execution={task_id:remote.task.task_id,fencing_token:remote.fencing_token,owner_principal_id:remote.identity.principal_id,owner_participant_id:remote.identity.participant_id,owner_session_id:remote.identity.session_id,owner_execution_id:remote.identity.execution_id,lease_expires_at:remote.task.lease_expires_at,ownership_state:"OWNED"};
      m.current_task_id=remote.task.task_id;
      m.last_action="resumed_remote"; m.next_action=m.checkpoint?.description || "continue from PostgreSQL checkpoint"; writeState(s,"MISSION_RESUMED_REMOTE",{task_id:remote.task.task_id,fencing_token:remote.fencing_token}); console.log("RESUMED " + m.mission_id); return;
    }
    if (cmd === "resolve-blockers") {
      if (m.status!=="BLOCKED" || !m.blockers.some(b=>b.status==="OPEN")) throw new Error("no open blocker on a BLOCKED mission");
      for (const blocker of m.blockers) if (blocker.status==="OPEN") { blocker.status="RESOLVED"; blocker.resolved_at=now(); }
      m.next_action="resume mission"; writeState(s,"BLOCKERS_RESOLVED"); return;
    }
    if (cmd === "set-task") {
      if (!a.length) throw new Error("task description required");
      m.current_task=a.join(" "); m.last_action="task_updated"; writeState(s,"MISSION_TASK_UPDATED"); return;
    }
    if (cmd === "record-file" || cmd === "record-commit" || cmd === "record-test") {
      if (!a.length) throw new Error("record value required");
      const field=cmd==="record-file"?"files_changed":cmd==="record-commit"?"commits":"tests";
      const value=a.join(" ");
      if (!m[field].includes(value)) m[field].push(value);
      m.last_action=cmd; writeState(s,cmd.toUpperCase().replaceAll("-","_")); return;
    }
    if (cmd === "block" || cmd === "disconnect") {
      if (!a.length) throw new Error("a reason is required");
      if (cmd === "block") m.blockers.push({kind:"BLOCKER",reason:a.join(" "),at:now(),status:"OPEN"});
      else { m.blockers.push({kind:"DISCONNECT",reason:a.join(" "),at:now(),status:"RECORDED",resumed_at:null}); m.presence.state="DISCONNECTED"; m.presence.disconnected_at=now(); }
      transition(m,cmd==="block"?"BLOCKED":"DISCONNECTED");
      if (cmd==="disconnect") { createCheckpoint(m,"automatic disconnect checkpoint"); m.next_action="recover checkpoint and enqueue reassignment"; }
      else m.next_action="resolve blocker and resume";
      writeState(s,cmd==="block"?"MISSION_BLOCKED":"MISSION_DISCONNECTED"); return;
    }
    if (cmd === "handoff-create") {
      if (!["IN_PROGRESS","WAITING_REVIEW"].includes(m.status)) throw new Error("handoff creation requires a live remote owner lease");
      if (a.length < 4 || a.slice(0,4).some(value=>!/^([0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})$/i.test(value))) {
        throw new Error("usage: handoff-create <to_principal_id> <to_participant_id> <to_session_id> <to_execution_id> [reason]");
      }
      if (["PENDING","ACCEPTED"].includes(m.handoff.status)) throw new Error("a handoff is already active");
      const lease=m.execution;
      if (!lease?.task_id || !Number.isSafeInteger(lease.fencing_token) || lease.ownership_state!=="OWNED") throw new Error("current PostgreSQL owner lease required");
      const recipient={principal_id:a[0],participant_id:a[1],session_id:a[2],execution_id:a[3]};
      const handoff=await new TeamRoomClient().createHandoff(lease,recipient,a.slice(4).join(" ")||"owner initiated handoff");
      m.handoff={status:handoff.status,handoff_id:handoff.handoff_id,from_principal_id:handoff.from_principal_id,to_principal_id:handoff.to_principal_id,to_participant_id:handoff.to_participant_id,to_session_id:handoff.to_session_id,to_execution_id:handoff.to_execution_id,created_at:handoff.created_at,expires_at:handoff.expires_at,accepted_at:null,completed_at:null,checkpoint_reference:handoff.checkpoint_reference};
      m.next_action="wait for the exact addressed Team Room session to accept; source retains ownership until acceptance";
      writeState(s,"AUTHENTICATED_HANDOFF_CREATED",{handoff_id:handoff.handoff_id,expires_at:handoff.expires_at}); return;
    }
    if (cmd === "handoff-accept") {
      if (a.length!==1 || !a[0]) throw new Error("usage: handoff-accept <handoff_id>");
      const grant=await new TeamRoomClient().acceptHandoff(a[0]);
      if (grant.task.mission_id!==new TeamRoomClient().missionUuid(m.mission_id)) throw new Error("handoff task mission_id does not match the current local mission");
      m.handoff={...m.handoff,status:"ACCEPTED",handoff_id:grant.handoff.handoff_id,from_principal_id:grant.handoff.from_principal_id,to_principal_id:grant.handoff.to_principal_id,to_participant_id:grant.handoff.to_participant_id,to_session_id:grant.handoff.to_session_id,to_execution_id:grant.handoff.to_execution_id,accepted_at:grant.handoff.accepted_at,checkpoint_reference:grant.handoff.checkpoint_reference};
      const remoteCheckpoint=grant.task.checkpoint;
      if (remoteCheckpoint && typeof remoteCheckpoint==="object" && !Array.isArray(remoteCheckpoint)) {
        m.checkpoint=remoteCheckpoint;
        m.progress=Number.isFinite(remoteCheckpoint.progress)?remoteCheckpoint.progress:m.progress;
        m.current_task=remoteCheckpoint.task||m.current_task;
        m.files_changed=Array.isArray(remoteCheckpoint.files_changed)?[...remoteCheckpoint.files_changed]:m.files_changed;
        m.commits=Array.isArray(remoteCheckpoint.commits)?[...remoteCheckpoint.commits]:m.commits;
        m.tests=Array.isArray(remoteCheckpoint.tests)?[...remoteCheckpoint.tests]:m.tests;
        m.last_result=remoteCheckpoint.last_result??m.last_result;
        m.blockers=Array.isArray(remoteCheckpoint.blockers)?JSON.parse(JSON.stringify(remoteCheckpoint.blockers)):m.blockers;
        m.checkpoint_history.push(remoteCheckpoint);
      }
      m.checkpoint_version=grant.task.checkpoint_version;
      m.responsible_ai=grant.identity.principal_id; m.participant_id=grant.identity.participant_id;
      m.presence.session_id=grant.identity.session_id; m.presence.state="ACTIVE"; m.presence.last_seen=now();
      m.execution={task_id:grant.task.task_id,fencing_token:grant.fencing_token,owner_principal_id:grant.identity.principal_id,owner_participant_id:grant.identity.participant_id,owner_session_id:grant.identity.session_id,owner_execution_id:grant.identity.execution_id,lease_expires_at:grant.task.lease_expires_at,ownership_state:"OWNED"};
      m.current_task_id=grant.task.task_id; transition(m,"IN_PROGRESS");
      m.next_action="continue from the server-preserved PostgreSQL checkpoint; renew and checkpoint with the new fence";
      writeState(s,"AUTHENTICATED_HANDOFF_ACCEPTED",{handoff_id:grant.handoff.handoff_id,task_id:grant.task.task_id,fencing_token:grant.fencing_token}); return;
    }
    if (cmd === "handoff-reject") {
      if (a.length<2) throw new Error("usage: handoff-reject <handoff_id> <reason>");
      const handoff=await new TeamRoomClient().rejectHandoff(a[0],a.slice(1).join(" "));
      m.handoff={...m.handoff,status:handoff.status,handoff_id:handoff.handoff_id};
      writeState(s,"AUTHENTICATED_HANDOFF_REJECTED",{handoff_id:handoff.handoff_id}); return;
    }
    if (cmd === "handoff-cancel") {
      if (a.length<2) throw new Error("usage: handoff-cancel <handoff_id> <reason>");
      const previousStatus=m.handoff.status;
      const handoff=await new TeamRoomClient().cancelHandoff(a[0],a.slice(1).join(" "));
      m.handoff={...m.handoff,status:handoff.status,handoff_id:handoff.handoff_id};
      if (previousStatus==="ACCEPTED" || previousStatus==="HANDOFF_ACCEPTED") {
        if (m.execution) { m.execution.ownership_state="RELEASED"; m.execution.lease_expires_at=null; }
        transition(m,"RECOVERY_REQUIRED");
        m.next_action="request a new authenticated recovery handoff; the previous owner is not restored";
      } else {
        m.next_action="source lease remains authoritative; continue only while PostgreSQL confirms the live fence";
      }
      writeState(s,"AUTHENTICATED_HANDOFF_CANCELLED",{handoff_id:handoff.handoff_id,previous_status:previousStatus}); return;
    }
    if (cmd === "handoff-complete") {
      if (!m.handoff.handoff_id) throw new Error("no server-side handoff reference is stored locally");
      const handoff=await new TeamRoomClient().getHandoff(m.handoff.handoff_id);
      if (handoff.status!=="COMPLETED") throw new Error("handoff is not COMPLETED in PostgreSQL; complete the task using the current fenced owner first");
      m.handoff={...m.handoff,status:"COMPLETED",completed_at:handoff.completed_at};
      writeState(s,"AUTHENTICATED_HANDOFF_COMPLETION_VERIFIED",{handoff_id:handoff.handoff_id}); return;
    }
    if (cmd === "submit-review") {
      if (m.status!=="IN_PROGRESS" || !a.length) throw new Error("usage: submit-review <result>; mission must be IN_PROGRESS");
      m.last_result=a.join(" "); m.last_action="submitted_for_review"; m.progress=100; m.next_action="review and approve or reject"; transition(m,"WAITING_REVIEW"); writeState(s,"MISSION_SUBMITTED_FOR_REVIEW"); return;
    }
    if (cmd === "approve") {
      if (m.status!=="WAITING_REVIEW") throw new Error("mission must be WAITING_REVIEW");
      const lease=m.execution;
      if (!lease?.task_id || !Number.isSafeInteger(lease.fencing_token)) throw new Error("remote lease reference required for approval");
      const finalCheckpoint={mission_id:m.mission_id,task_id:lease.task_id,result:m.last_result||"review approved",progress:100,context:{title:m.title,current_task:m.current_task},timestamp:now()};
      try { await new TeamRoomClient().complete(lease,finalCheckpoint); }
      catch(error) { lease.ownership_state="LOSS_OF_OWNERSHIP"; writeState(s,"REMOTE_APPROVAL_REJECTED",{code:error.code||"REMOTE_ERROR"}); throw error; }
      m.last_action="review_approved"; m.next_action="register next mission"; transition(m,"DONE"); createCheckpoint(m,"final completion checkpoint"); writeState(s,"MISSION_REVIEW_APPROVED_REMOTE",{task_id:lease.task_id,fencing_token:lease.fencing_token}); return;
    }
    if (cmd === "reject" || cmd === "cancel") {
      if (!a.length) throw new Error("reason required");
      if (cmd==="reject" && m.status!=="WAITING_REVIEW") throw new Error("mission must be WAITING_REVIEW to reject");
      if (cmd==="cancel" && ["DONE","CANCELLED","REJECTED"].includes(m.status)) throw new Error("terminal mission cannot be cancelled");
      m.last_result=a.join(" "); m.last_action=cmd; m.next_action=cmd==="reject"?"revise and resubmit":"none"; transition(m,cmd==="reject"?"REJECTED":"CANCELLED"); writeState(s,cmd==="reject"?"MISSION_REJECTED":"MISSION_CANCELLED"); return;
    }
    if (cmd === "complete") {
      if (m.status!=="IN_PROGRESS" || !a.length) throw new Error("usage: complete <result>; mission must be IN_PROGRESS");
      const lease=m.execution;
      if (!lease?.task_id || !Number.isSafeInteger(lease.fencing_token) || lease.ownership_state==="LOSS_OF_OWNERSHIP") throw new Error("valid remote lease reference required; completion denied");
      const finalCheckpoint={mission_id:m.mission_id,task_id:lease.task_id,result:a.join(" "),progress:100,context:{title:m.title,current_task:m.current_task,files_changed:m.files_changed,commits:m.commits,tests:m.tests},timestamp:now()};
      try { await new TeamRoomClient().complete(lease,finalCheckpoint); }
      catch (error) { lease.ownership_state="LOSS_OF_OWNERSHIP"; writeState(s,"REMOTE_COMPLETE_REJECTED",{code:error.code||"REMOTE_ERROR"}); throw error; }
      m.last_result=a.join(" "); m.last_action="completed"; m.progress=100; m.next_action="register next mission"; transition(m,"DONE"); createCheckpoint(m,"final completion checkpoint"); writeState(s,"MISSION_COMPLETED_REMOTE",{task_id:lease.task_id,fencing_token:lease.fencing_token}); return;
    }
    if (cmd === "prompt-ready") {
      if (m.status !== "DONE") throw new Error("current mission must be DONE before preparing its successor prompt");
      if (a.length<2 || !safeId(a[0]) || a[0] === m.mission_id) throw new Error("usage: prompt-ready <next_mission_id> <prompt text>");
      const dir=path.join(ROOT,"prompts"); fs.mkdirSync(dir,{recursive:true});
      const promptPath=path.join(dir,a[0]+".md"); fs.writeFileSync(promptPath,a.slice(1).join(" ")+"\n",{flag:"wx",mode:0o600});
      m.prompt={status:"PROMPT_PENDING_DELIVERY",next_mission_id:a[0],path:path.relative(process.cwd(),promptPath),delivery_evidence:null};
      writeState(s,"PROMPT_READY",{next_mission_id:a[0],path:m.prompt.path}); console.log("PROMPT_READY; delivery remains pending"); return;
    }
    if (cmd === "prompt-delivered") throw new Error("PROMPT_DELIVERED is disabled: no verified external provider delivery adapter is configured; prompt remains PROMPT_PENDING_DELIVERY");
    throw new Error("unknown command: " + cmd);
  } finally { release(); }
}
main(process.argv.slice(2)).catch(error => fail(error instanceof Error ? error.message : String(error)));
