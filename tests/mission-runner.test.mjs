import test, { after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { createServer } from "node:http";
import { spawnSync } from "node:child_process";

const script = path.resolve("scripts/mission-runner.mjs");
const roomId="10000000-0000-4000-8000-000000000001";
const identity={tenant_id:"10000000-0000-4000-8000-000000000002",participant_id:"10000000-0000-4000-8000-000000000003",principal_id:"10000000-0000-4000-8000-000000000004",session_id:"10000000-0000-4000-8000-000000000005",execution_id:"10000000-0000-4000-8000-000000000006",room_id:roomId};
const tasks=new Map();
const server=createServer(async(req,res)=>{
 const send=(status,value)=>{res.writeHead(status,{"content-type":"application/json"});res.end(JSON.stringify(value));};
 const url=new URL(req.url,"http://127.0.0.1");
 if(req.headers.authorization!=="Bearer test-session-token") return send(401,{});
 if(req.method==="GET" && url.pathname==="/auth/me") return send(200,identity);
 if(req.method==="POST" && url.pathname==="/team-room/heartbeat") return send(200,{session_id:identity.session_id,execution_id:identity.execution_id,last_seen_at:new Date().toISOString()});
 let body={}; if(req.method==="POST"){let raw="";for await(const chunk of req)raw+=chunk;try{body=raw?JSON.parse(raw):{};}catch{return send(400,{});}}
 if(req.method==="POST" && url.pathname==="/team-room/tasks"){
  let task=tasks.get(body.mission_id);
  if(!task){task={task_id:crypto.randomUUID(),mission_id:body.mission_id,status:"UNOWNED",owner_principal_id:null,owner_participant_id:null,owner_session_id:null,owner_execution_id:null,fencing_token:0,lease_expires_at:null,checkpoint:body.checkpoint||{},checkpoint_version:0};tasks.set(body.mission_id,task);}
  return send(201,task);
 }
 const lookup=url.pathname.match(/^\/team-room\/tasks\/by-mission\/([^/]+)$/);
 if(req.method==="GET" && lookup){const task=tasks.get(lookup[1]);return task?send(200,task):send(404,{});}
 const taskRoute=url.pathname.match(/^\/team-room\/tasks\/([^/]+)\/(lease\/acquire|lease\/renew|lease\/release|checkpoint|complete)$/);
 if(taskRoute){
  const task=[...tasks.values()].find(t=>t.task_id===taskRoute[1]);if(!task)return send(404,{});
  const action=taskRoute[2], live=task.lease_expires_at&&Date.parse(task.lease_expires_at)>Date.now();
  const owned=task.owner_session_id===identity.session_id&&task.owner_execution_id===identity.execution_id;
  if(action==="lease/acquire"){
   if(live)return send(409,{});
   task.status=task.fencing_token?"REASSIGNED":"OWNED";task.fencing_token++;task.owner_principal_id=identity.principal_id;task.owner_participant_id=identity.participant_id;task.owner_session_id=identity.session_id;task.owner_execution_id=identity.execution_id;task.lease_expires_at=new Date(Date.now()+600000).toISOString();
   return send(200,{task,lease_seconds:60,reassigned:task.fencing_token>1});
  }
  if(!owned||!live||body.fencing_token!==task.fencing_token)return send(412,{});
  if(action==="lease/renew"){task.lease_expires_at=new Date(Date.now()+600000).toISOString();return send(200,task);}
  if(action==="checkpoint"){task.checkpoint=body.checkpoint;task.checkpoint_version++;return send(200,task);}
  if(action==="complete"){task.status="COMPLETED";task.checkpoint=body.checkpoint;task.checkpoint_version++;task.owner_principal_id=null;task.owner_participant_id=null;task.owner_session_id=null;task.owner_execution_id=null;task.lease_expires_at=null;return send(200,task);}
  if(action==="lease/release"){task.status="UNOWNED";task.owner_principal_id=null;task.owner_participant_id=null;task.owner_session_id=null;task.owner_execution_id=null;task.lease_expires_at=null;return send(200,task);}
 }
 return send(404,{});
});
await new Promise(resolve=>server.listen(0,"127.0.0.1",resolve));
const apiUrl=`http://127.0.0.1:${server.address().port}`;
after(()=>server.close());
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

test("handoff requires explicit acceptance and records lifecycle", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-121","121","Handoff");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start"); run(c,"checkpoint","before handoff");
 assert.equal(run(c,"handoff-create","AI-B").status,0);
 assert.equal(run(c,"handoff-complete").status,2);
 assert.equal(run(c,"handoff-accept","p-b").status,0);
 assert.equal(run(c,"resume","AI-B","p-b").status,0);
 assert.equal(run(c,"handoff-complete").status,0);
 const m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-121"];
 assert.equal(m.handoff.status,"HANDOFF_COMPLETED"); assert.equal(m.responsible_ai,"AI-B");
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
 assert.equal(run(c,"heartbeat","session-a").status,0);
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

test("recovery preserves checkpoint and handoff blocks the previous executor", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-135","135","Recovery handoff");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start");
 run(c,"set-task","implement feature"); run(c,"record-file","src/feature.rs");
 run(c,"record-commit","abc123"); run(c,"record-test","unit tests pending");
 run(c,"disconnect","heartbeat timeout");
 assert.equal(run(c,"recover").status,0);
 assert.equal(run(c,"handoff-create","AI-B").status,0);
 assert.equal(run(c,"handoff-create","AI-C").status,2);
 assert.equal(run(c,"handoff-accept","p-b").status,0);
 assert.equal(run(c,"resume").status,2);
 assert.equal(run(c,"resume","AI-A","p-a").status,2);
 assert.equal(run(c,"resume","AI-B","p-b").status,0);
 assert.equal(run(c,"handoff-complete").status,0);
 const m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-135"];
 assert.equal(m.status,"IN_PROGRESS");
 assert.equal(m.responsible_ai,"AI-B");
 assert.equal(m.checkpoint.description,"automatic disconnect checkpoint");
 assert.equal(m.checkpoint.task_id,"implement feature");
 assert.deepEqual(m.checkpoint.files_changed,["src/feature.rs"]);
 assert.deepEqual(m.checkpoint.commits,["abc123"]);
 assert.equal(m.handoff.context.mission_id,"M-135");
 assert.equal(m.handoff.context.checkpoint.checkpoint_id,m.checkpoint.checkpoint_id);
 assert.equal(m.reassignment_queue[0].status,"COMPLETED");
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
