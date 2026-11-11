import test, { after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Worker } from "node:worker_threads";
import { spawnSync } from "node:child_process";
import { TeamRoomClient } from "../scripts/team-room-client.mjs";

const script = path.resolve("scripts/mission-runner.mjs");
const roomId="10000000-0000-4000-8000-000000000001";
const identity={tenant_id:"10000000-0000-4000-8000-000000000002",participant_id:"10000000-0000-4000-8000-000000000003",principal_id:"10000000-0000-4000-8000-000000000004",session_id:"10000000-0000-4000-8000-000000000005",execution_id:"10000000-0000-4000-8000-000000000006",room_id:roomId};
const serverWorker=new Worker(new URL("./team-room-http-fixture.mjs",import.meta.url));
const {port,identity:fixtureIdentity}=await new Promise((resolve,reject)=>{
 serverWorker.once("message",resolve);
 serverWorker.once("error",reject);
});
const apiUrl=`http://127.0.0.1:${port}`;
after(()=>serverWorker.terminate());
function env() { const dir=fs.mkdtempSync(path.join(os.tmpdir(),"mission-runner-")); return {dir,env:{...process.env,MISSION_STATE_DIR:dir,TEAM_ROOM_API_URL:apiUrl,TEAM_ROOM_SESSION_TOKEN:"test-session-token",TEAM_ROOM_ROOM_ID:roomId}}; }
function run(ctx,...args) { return spawnSync(process.execPath,[script,...args],{encoding:"utf8",env:ctx.env}); }

test("persists mission, checkpoint and resume across separate processes", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 assert.equal(run(c,"init","M-120","120","Continuity test").status,0);
 let initial=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 assert.equal(initial.missions["M-120"].started_at,null);
 assert.equal(run(c,"assign","GPT-6","participant-test").status,0);
 assert.equal(run(c,"acknowledge").status,0);
 assert.equal(run(c,"start").status,0);
 let started=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 assert.ok(started.missions["M-120"].started_at);
 assert.equal(run(c,"checkpoint","after audit").status,0);
 assert.equal(run(c,"disconnect","session ended").status,0);
 assert.equal(run(c,"resume").status,0);
 const s=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 const m=s.missions["M-120"];
 assert.equal(m.status,"IN_PROGRESS"); assert.equal(m.checkpoint.description,"automatic disconnect checkpoint"); assert.equal(m.checkpoint_history.length,2); assert.equal(m.checkpoint_history[0].description,"after audit");
 assert.equal(m.responsible_ai,"GPT-6"); assert.equal(m.blockers.length,1);
 assert.equal(m.blockers[0].kind,"DISCONNECT"); assert.equal(m.blockers[0].status,"RESUMED");
});

test("handoff is never granted by local state without authenticated Team Room confirmation", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-121","121","Authenticated handoff");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start"); run(c,"checkpoint","before handoff");
 const before=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-121"];
 const target=["20000000-0000-4000-8000-000000000001","20000000-0000-4000-8000-000000000002","20000000-0000-4000-8000-000000000003","20000000-0000-4000-8000-000000000004"];
 assert.equal(run(c,"handoff-create",...target,"planned transfer").status,2,"HTTP fixture does not implement authenticated handoff; Runner must fail closed");
 assert.equal(run(c,"handoff-accept","20000000-0000-4000-8000-000000000005").status,2,"an unverified handoff id cannot authorize acceptance");
 assert.equal(run(c,"handoff-complete").status,2,"local metadata cannot mark a handoff complete");
 const after=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-121"];
 assert.equal(after.handoff.status,"NONE");
 assert.equal(after.execution.ownership_state,"OWNED");
 assert.equal(after.execution.fencing_token,before.execution.fencing_token);
});

test("prompt is stored but not marked delivered without delivery evidence", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-122","122","Prompt");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start"); run(c,"complete","finished");
 let m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-122"];
 assert.equal(m.checkpoint.status,"DONE");
 assert.equal(m.checkpoint.description,"final completion checkpoint");
 run(c,"prompt-ready","M-123","Continue the next task");
 m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-122"];
 assert.equal(m.prompt.status,"PROMPT_PENDING_DELIVERY");
 assert.equal(run(c,"prompt-delivered").status,2);
 assert.equal(run(c,"prompt-delivered","fake-message-id").status,2);
 m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-122"];
 assert.equal(m.prompt.status,"PROMPT_PENDING_DELIVERY");
 assert.equal(m.prompt.delivery_evidence,null);
});

test("refuses duplicate initialization and corrupt state", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 assert.equal(run(c,"init","M-124","124","Corruption").status,0);
 assert.equal(run(c,"init","M-124","124","Overwrite").status,2);
 fs.writeFileSync(path.join(c.dir,"STATE.json"),"{broken");
 assert.match(run(c,"status").stderr,/corrupt/);
});

test("records block and refuses resume until blocker is resolved", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-125","125","Block");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start");
 assert.equal(run(c,"block","missing dependency").status,0);
 assert.equal(run(c,"resume").status,2);
});

test("blockers can be explicitly resolved before resume", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-126","126","Unblock");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start");
 run(c,"checkpoint","before block"); run(c,"block","dependency unavailable");
 assert.equal(run(c,"resume").status,2);
 assert.equal(run(c,"resolve-blockers").status,0);
 assert.equal(run(c,"resume").status,0);
 const m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-126"];
 assert.equal(m.blockers[0].status,"RESOLVED"); assert.equal(m.status,"IN_PROGRESS");
});

test("supervisor lists missions and registers successor only after completion", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-127","127","First");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start"); run(c,"submit-review","done"); run(c,"approve");
 assert.equal(run(c,"register-next","M-128","128","Second").status,0);
 const s=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 assert.equal(s.current_mission_id,"M-128");
 assert.equal(s.missions["M-127"].status,"DONE");
 assert.equal(s.missions["M-128"].status,"CREATED");
 assert.equal(run(c,"list").status,0);
 assert.equal(run(c,"register-next","M-128","128","Duplicate").status,2);
});

test("review gate keeps mission out of DONE until explicit approval", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-129","129","Review");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start");
 assert.equal(run(c,"submit-review","result ready").status,0);
 let m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-129"];
 assert.equal(m.status,"WAITING_REVIEW"); assert.equal(m.completed_at,null);
 assert.equal(run(c,"approve").status,0);
 m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-129"];
 assert.equal(m.status,"DONE"); assert.ok(m.completed_at);
});

test("rejects nonexistent mission selection and duplicate execution", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 assert.equal(run(c,"status").status,2);
 run(c,"init","M-130","130","Missing mission");
 assert.equal(run(c,"switch","M-999").status,2);
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start");
 assert.equal(run(c,"start").status,2);
 assert.equal(run(c,"resume").status,2);
 const state=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 assert.equal(state.current_mission_id,"M-130");
 assert.equal(state.missions["M-130"].status,"IN_PROGRESS");
});

test("rejects a held state lock without modifying persistent state", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-131","131","Lock contention");
 const before=fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8");
 fs.writeFileSync(path.join(c.dir,".state.lock"),JSON.stringify({pid:999999,at:new Date().toISOString()}));
 const result=run(c,"assign","AI-A","p-a");
 assert.equal(result.status,2);
 assert.match(result.stderr,/state lock exists/);
 assert.equal(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"),before);
});

test("rejects structurally invalid state schema", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-132","132","Schema");
 fs.writeFileSync(path.join(c.dir,"STATE.json"),JSON.stringify({version:1,current_mission_id:"M-132",missions:[]}));
 const result=run(c,"status");
 assert.equal(result.status,2);
 assert.match(result.stderr,/invalid mission state schema/);
});

test("requires a completed mission before preparing a successor prompt", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-133","133","Prompt gate");
 assert.equal(run(c,"prompt-ready","M-134","Do next mission").status,2);
});


test("heartbeat presence timeout requires missed heartbeats, not a long-running task", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-134","134","Heartbeat");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start");
 assert.equal(run(c,"heartbeat",fixtureIdentity.session_id).status,0);
 let s=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 assert.equal(s.missions["M-134"].presence.state,"ACTIVE");
 s.missions["M-134"].presence.last_seen=new Date(Date.now()-500).toISOString();
 fs.writeFileSync(path.join(c.dir,"STATE.json"),JSON.stringify(s));
 assert.equal(run(c,"presence","1000").status,0);
 s=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 assert.equal(s.missions["M-134"].presence.state,"IDLE");
 assert.equal(s.missions["M-134"].status,"IN_PROGRESS");
 s.missions["M-134"].presence.last_seen=new Date(Date.now()-1500).toISOString();
 fs.writeFileSync(path.join(c.dir,"STATE.json"),JSON.stringify(s));
 assert.equal(run(c,"presence","1000").status,0);
 s=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 assert.equal(s.missions["M-134"].presence.state,"SUSPECTED_DISCONNECTED");
 assert.equal(s.missions["M-134"].status,"IN_PROGRESS");
 s.missions["M-134"].presence.last_seen=new Date(Date.now()-3000).toISOString();
 fs.writeFileSync(path.join(c.dir,"STATE.json"),JSON.stringify(s));
 assert.equal(run(c,"presence","1000").status,0);
 s=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 assert.equal(s.missions["M-134"].presence.state,"DISCONNECTED");
 assert.equal(s.missions["M-134"].status,"RECOVERY_REQUIRED");
 assert.equal(s.missions["M-134"].checkpoint.description,"automatic heartbeat-timeout checkpoint");
 assert.equal(s.missions["M-134"].checkpoint.status,"RECOVERY_REQUIRED");
 assert.equal(s.missions["M-134"].checkpoint_history.length,1);
 assert.equal(s.missions["M-134"].reassignment_queue[0].checkpoint_id,s.missions["M-134"].checkpoint.checkpoint_id);
});

test("recovery preserves checkpoint but cannot fabricate a remote handoff", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-135","135","Recovery handoff");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start");
 run(c,"set-task","implement feature"); run(c,"record-file","src/feature.rs");
 run(c,"record-commit","abc123"); run(c,"record-test","unit tests pending");
 run(c,"disconnect","heartbeat timeout");
 assert.equal(run(c,"recover").status,0);
 const target=["20000000-0000-4000-8000-000000000011","20000000-0000-4000-8000-000000000012","20000000-0000-4000-8000-000000000013","20000000-0000-4000-8000-000000000014"];
 assert.equal(run(c,"handoff-create",...target,"recovery transfer").status,2,"recovery handoff must be created by the authenticated recovery endpoint, not local state");
 assert.equal(run(c,"handoff-accept","20000000-0000-4000-8000-000000000015").status,2,"only the exact server-addressed session can accept");
 const m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-135"];
 assert.equal(m.status,"RECOVERY_REQUIRED");
 assert.equal(m.handoff.status,"NONE");
 assert.equal(m.checkpoint.description,"automatic disconnect checkpoint");
 assert.equal(m.checkpoint.task_id,"implement feature");
 assert.deepEqual(m.checkpoint.files_changed,["src/feature.rs"]);
 assert.deepEqual(m.checkpoint.commits,["abc123"]);
 assert.equal(m.reassignment_queue[0].status,"QUEUED");
});

test("legacy state is upgraded in memory with continuity fields", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-136","136","Legacy compatibility");
 const p=path.join(c.dir,"STATE.json"), s=JSON.parse(fs.readFileSync(p,"utf8"));
 delete s.missions["M-136"].presence; delete s.missions["M-136"].execution;
 delete s.missions["M-136"].reassignment_queue; delete s.missions["M-136"].checkpoint_history;
 fs.writeFileSync(p,JSON.stringify(s));
 assert.equal(run(c,"status").status,0);
});


test("start fails closed without Team Room credentials and never creates local ownership", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-201","201","Fail closed");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge");
 delete c.env.TEAM_ROOM_SESSION_TOKEN;
 const result=run(c,"start");
 assert.equal(result.status,2);
 assert.match(result.stderr,/TEAM_ROOM_API_URL/);
 const m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-201"];
 assert.equal(m.status,"ACKNOWLEDGED");
 assert.equal(m.execution.lease_id,null);
 assert.equal(m.execution.fencing_token,undefined);
});

test("start persists only the remote task and fencing reference; presence does not renew lease", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-202","202","Remote authority");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge");
 assert.equal(run(c,"start").status,0);
 let m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-202"];
 assert.ok(m.execution.task_id);
 assert.equal(m.execution.fencing_token,1);
 assert.equal(m.execution.ownership_state,"OWNED");
 const expires=m.execution.lease_expires_at;
 assert.equal(run(c,"heartbeat").status,0);
 m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-202"];
 assert.equal(m.execution.lease_expires_at,expires);
 assert.equal(run(c,"renew").status,0);
 m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-202"];
 assert.equal(m.execution.ownership_state,"OWNED");
 assert.equal(run(c,"checkpoint","remote checkpoint").status,0);
 assert.equal(run(c,"release").status,0);
 m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-202"];
 assert.equal(m.execution.ownership_state,"RELEASED");
});


test("stale fencing is rejected remotely for renew, checkpoint and complete after takeover", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-203","203","Fencing takeover");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); assert.equal(run(c,"start").status,0);
 const statePath=path.join(c.dir,"STATE.json");
 const original=JSON.parse(fs.readFileSync(statePath,"utf8"));
 const mission=original.missions["M-203"];
 const token1=mission.execution.fencing_token;
 const takeover=spawnSync(process.execPath,["-e",`fetch(${JSON.stringify(apiUrl+"/test/takeover/"+mission.execution.task_id)},{method:"POST",headers:{authorization:"Bearer test-session-token"}}).then(r=>process.exit(r.ok?0:1))`],{encoding:"utf8",env:c.env});
 assert.equal(takeover.status,0);
 const renew=run(c,"renew");
 assert.equal(renew.status,2);
 let afterRenew=JSON.parse(fs.readFileSync(statePath,"utf8"));
 assert.equal(afterRenew.missions["M-203"].execution.ownership_state,"LOSS_OF_OWNERSHIP");
 for(const command of ["checkpoint","complete"]){
  const isolated=env();t.after(()=>fs.rmSync(isolated.dir,{recursive:true,force:true}));
  fs.writeFileSync(path.join(isolated.dir,"STATE.json"),JSON.stringify(original));
  const result=run(isolated,command,...(command==="checkpoint"?["stale checkpoint"]:["stale completion"]));
  assert.equal(result.status,2);
  assert.match(result.stderr,/HTTP 412/);
 }
 original.missions["M-203"].execution.fencing_token=token1+1;
 original.missions["M-203"].execution.ownership_state="OWNED";
 fs.writeFileSync(statePath,JSON.stringify(original));
 assert.equal(run(c,"complete","new owner completion").status,0);
});


test("two independent Runner state directories resolve one mission task and only one acquires", async t => {
 const a=env(),b=env();t.after(()=>fs.rmSync(a.dir,{recursive:true,force:true}));t.after(()=>fs.rmSync(b.dir,{recursive:true,force:true}));
 for(const ctx of [a,b]){run(ctx,"init","M-204","204","Same mission");run(ctx,"assign","AI-A","p-a");run(ctx,"acknowledge");}
 assert.equal(run(a,"start").status,0);
 const first=JSON.parse(fs.readFileSync(path.join(a.dir,"STATE.json"),"utf8")).missions["M-204"];
 assert.ok(first.execution.task_id);
 const secondStart=run(b,"start");
 assert.equal(secondStart.status,2);
 assert.match(secondStart.stderr,/HTTP 409/);
 const client=new TeamRoomClient(a.env);
 const missionUuid=client.missionUuid("M-204");
 const response=await fetch(apiUrl+"/team-room/tasks/by-mission/"+missionUuid,{headers:{authorization:"Bearer test-session-token"}});
 assert.equal(response.status,200);
 const canonical=await response.json();
 assert.equal(canonical.task_id,first.execution.task_id);
});
