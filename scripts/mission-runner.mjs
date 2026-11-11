#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";

const ROOT = path.resolve(process.env.MISSION_STATE_DIR || ".mission");
const STATE_FILE = path.join(ROOT, "STATE.json");
const EVENTS_FILE = path.join(ROOT, "events.ndjson");
const LOCK_FILE = path.join(ROOT, ".state.lock");
const STATUSES = new Set(["CREATED","ASSIGNED","ACKNOWLEDGED","IN_PROGRESS","WAITING_REVIEW","BLOCKED","DONE","REJECTED","CANCELLED","DISCONNECTED"]);
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
  if (s.version !== 1 || !s.missions || typeof s.missions !== "object") throw new Error("unsupported or invalid mission state schema");
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
function usage() {
  console.log("Mission Runner (local state orchestration; no AI dispatch)\nCommands:\n  init <id> <number> <title>\n  status\n  list\n  register-next <id> <number> <title>\n  switch <mission_id>\n  assign <responsible_ai> <participant_id>\n  acknowledge\n  start\n  checkpoint <description>\n  resume\n  block <reason>\n  resolve-blockers\n  set-task <task>\n  record-file <path>\n  record-commit <sha>\n  record-test <result>\n  disconnect <reason>\n  handoff-create <target_ai>\n  handoff-accept <participant_id>\n  handoff-complete\n  submit-review <result>\n  approve\n  reject <reason>\n  cancel <reason>\n  complete <result> (direct close; use submit-review for review gate)\n  prompt-ready <next_mission_id> <prompt text>\n  prompt-delivered <evidence_reference>");
}
function main(args) {
  const [cmd, ...a] = args;
  if (!cmd || cmd === "help" || cmd === "--help") return usage();
  const release = lock();
  try {
    if (cmd === "init") {
      if (a.length < 3 || !safeId(a[0]) || !/^\d+$/.test(a[1])) throw new Error("usage: init <safe-id> <number> <title>");
      if (fs.existsSync(STATE_FILE)) throw new Error("state already exists; refusing to overwrite");
      const stamp = now(), id = a[0];
      const mission = { mission_id:id, mission_number:Number(a[1]), title:a.slice(2).join(" "), responsible_ai:null, participant_id:null, current_task:null, subtasks:[], status:"CREATED", progress:0, last_action:"initialized", last_result:null, files_changed:[], commits:[], tests:[], evidence:[], blockers:[], next_action:"assign responsible AI", checkpoint:null, started_at:stamp, updated_at:stamp, completed_at:null, handoff:{status:"NONE",target_ai:null,participant_id:null,created_at:null,accepted_at:null,completed_at:null}, prompt:{status:"NONE",next_mission_id:null,path:null,delivery_evidence:null} };
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
      s.missions[id]={mission_id:id,mission_number:Number(a[1]),title:a.slice(2).join(" "),responsible_ai:null,participant_id:null,current_task:null,subtasks:[],status:"CREATED",progress:0,last_action:"registered",last_result:null,files_changed:[],commits:[],tests:[],evidence:[],blockers:[],next_action:"assign responsible AI",checkpoint:null,started_at:null,updated_at:stamp,completed_at:null,handoff:{status:"NONE",target_ai:null,participant_id:null,created_at:null,accepted_at:null,completed_at:null},prompt:{status:"NONE",next_mission_id:null,path:null,delivery_evidence:null}};
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
    if (cmd === "start") { if (!["ACKNOWLEDGED","DISCONNECTED","BLOCKED"].includes(m.status)) throw new Error("mission must be ACKNOWLEDGED, DISCONNECTED or unblocked BLOCKED"); if(m.status==="BLOCKED" && m.blockers.length) throw new Error("resolve blockers before start"); transition(m,"IN_PROGRESS"); m.started_at=m.started_at||now(); m.next_action="execute current_task and checkpoint"; writeState(s,"MISSION_STARTED"); return; }
    if (cmd === "checkpoint") {
      if (m.status!=="IN_PROGRESS" || !a.length) throw new Error("usage: checkpoint <description>; mission must be IN_PROGRESS");
      const cp={ checkpoint_id:crypto.randomUUID(), at:now(), description:a.join(" "), task:m.current_task, files_changed:[...m.files_changed], commits:[...m.commits], tests:[...m.tests], result:m.last_result };
      m.checkpoint=cp; m.last_action="checkpoint"; m.progress=Math.min(99,Math.max(m.progress,1)); m.next_action="resume from latest checkpoint"; writeState(s,"CHECKPOINT_CREATED",{checkpoint_id:cp.checkpoint_id}); return;
    }
    if (cmd === "resume") {
      if (!["IN_PROGRESS","DISCONNECTED","BLOCKED"].includes(m.status)) throw new Error("mission is not resumable from current status");
      if (m.status==="BLOCKED" && m.blockers.length) throw new Error("resolve blockers before resume");
      if (!m.checkpoint && m.status!=="DISCONNECTED") throw new Error("no checkpoint available");
      transition(m,"IN_PROGRESS"); m.last_action="resumed"; m.next_action=m.checkpoint?.description || "continue assigned task"; writeState(s,"MISSION_RESUMED"); console.log("RESUMED " + m.mission_id); return;
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
      m.blockers.push({reason:a.join(" "),at:now(),status:"OPEN"}); transition(m,cmd==="block"?"BLOCKED":"DISCONNECTED"); m.next_action="resolve blocker and resume"; writeState(s,cmd==="block"?"MISSION_BLOCKED":"MISSION_DISCONNECTED"); return;
    }
    if (cmd === "handoff-create") {
      if (!a[0] || !a[0].trim()) throw new Error("target AI required");
      m.handoff={status:"HANDOFF_CREATED",target_ai:a[0],participant_id:null,created_at:now(),accepted_at:null,completed_at:null}; m.next_action="await explicit handoff acceptance"; writeState(s,"HANDOFF_CREATED"); return;
    }
    if (cmd === "handoff-accept") {
      if (m.handoff.status!=="HANDOFF_CREATED" || !a[0]) throw new Error("no pending handoff or participant_id missing");
      m.handoff.status="HANDOFF_ACCEPTED"; m.handoff.participant_id=a[0]; m.handoff.accepted_at=now(); m.responsible_ai=m.handoff.target_ai; m.participant_id=a[0]; writeState(s,"HANDOFF_ACCEPTED"); return;
    }
    if (cmd === "handoff-complete") {
      if (m.handoff.status!=="HANDOFF_ACCEPTED") throw new Error("handoff must be accepted first");
      m.handoff.status="HANDOFF_COMPLETED"; m.handoff.completed_at=now(); writeState(s,"HANDOFF_COMPLETED"); return;
    }
    if (cmd === "submit-review") {
      if (m.status!=="IN_PROGRESS" || !a.length) throw new Error("usage: submit-review <result>; mission must be IN_PROGRESS");
      m.last_result=a.join(" "); m.last_action="submitted_for_review"; m.progress=100; m.next_action="review and approve or reject"; transition(m,"WAITING_REVIEW"); writeState(s,"MISSION_SUBMITTED_FOR_REVIEW"); return;
    }
    if (cmd === "approve") {
      if (m.status!=="WAITING_REVIEW") throw new Error("mission must be WAITING_REVIEW");
      m.last_action="review_approved"; m.next_action="register next mission"; transition(m,"DONE"); writeState(s,"MISSION_REVIEW_APPROVED"); return;
    }
    if (cmd === "reject" || cmd === "cancel") {
      if (!a.length) throw new Error("reason required");
      if (cmd==="reject" && m.status!=="WAITING_REVIEW") throw new Error("mission must be WAITING_REVIEW to reject");
      if (cmd==="cancel" && ["DONE","CANCELLED","REJECTED"].includes(m.status)) throw new Error("terminal mission cannot be cancelled");
      m.last_result=a.join(" "); m.last_action=cmd; m.next_action=cmd==="reject"?"revise and resubmit":"none"; transition(m,cmd==="reject"?"REJECTED":"CANCELLED"); writeState(s,cmd==="reject"?"MISSION_REJECTED":"MISSION_CANCELLED"); return;
    }
    if (cmd === "complete") {
      if (m.status!=="IN_PROGRESS" || !a.length) throw new Error("usage: complete <result>; mission must be IN_PROGRESS");
      m.last_result=a.join(" "); m.last_action="completed"; m.progress=100; m.next_action="register next mission"; transition(m,"DONE"); writeState(s,"MISSION_COMPLETED"); return;
    }
    if (cmd === "prompt-ready") {
      if (a.length<2 || !safeId(a[0])) throw new Error("usage: prompt-ready <next_mission_id> <prompt text>");
      const dir=path.join(ROOT,"prompts"); fs.mkdirSync(dir,{recursive:true});
      const promptPath=path.join(dir,a[0]+".md"); fs.writeFileSync(promptPath,a.slice(1).join(" ")+"\n",{flag:"wx",mode:0o600});
      m.prompt={status:"PROMPT_PENDING_DELIVERY",next_mission_id:a[0],path:path.relative(process.cwd(),promptPath),delivery_evidence:null};
      writeState(s,"PROMPT_READY",{next_mission_id:a[0],path:m.prompt.path}); console.log("PROMPT_READY; delivery remains pending"); return;
    }
    if (cmd === "prompt-delivered") {
      if (m.prompt.status!=="PROMPT_PENDING_DELIVERY" || !a.length) throw new Error("pending prompt and delivery evidence reference required");
      m.prompt.status="PROMPT_DELIVERED"; m.prompt.delivery_evidence=a.join(" "); writeState(s,"PROMPT_DELIVERED"); return;
    }
    throw new Error("unknown command: " + cmd);
  } finally { release(); }
}
try { main(process.argv.slice(2)); } catch (error) { fail(error instanceof Error ? error.message : String(error)); }
