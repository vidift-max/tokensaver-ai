import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

const script = path.resolve("scripts/mission-runner.mjs");
function env() { const dir=fs.mkdtempSync(path.join(os.tmpdir(),"mission-runner-")); return {dir,env:{...process.env,MISSION_STATE_DIR:dir}}; }
function run(ctx,...args) { return spawnSync(process.execPath,[script,...args],{encoding:"utf8",env:ctx.env}); }

test("persists mission, checkpoint and resume across separate processes", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 assert.equal(run(c,"init","M-120","120","Continuity test").status,0);
 assert.equal(run(c,"assign","GPT-6","participant-test").status,0);
 assert.equal(run(c,"acknowledge").status,0);
 assert.equal(run(c,"start").status,0);
 assert.equal(run(c,"checkpoint","after audit").status,0);
 assert.equal(run(c,"disconnect","session ended").status,0);
 assert.equal(run(c,"resume").status,0);
 const s=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 const m=s.missions["M-120"];
 assert.equal(m.status,"IN_PROGRESS"); assert.equal(m.checkpoint.description,"after audit");
 assert.equal(m.responsible_ai,"GPT-6"); assert.equal(m.blockers.length,1);
});

test("handoff requires explicit acceptance and records lifecycle", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-121","121","Handoff");
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start");
 assert.equal(run(c,"handoff-create","AI-B").status,0);
 assert.equal(run(c,"handoff-complete").status,2);
 assert.equal(run(c,"handoff-accept","p-b").status,0);
 assert.equal(run(c,"handoff-complete").status,0);
 const m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-121"];
 assert.equal(m.handoff.status,"HANDOFF_COMPLETED"); assert.equal(m.responsible_ai,"AI-B");
});

test("prompt is stored but not marked delivered without delivery evidence", t => {
 const c=env(); t.after(()=>fs.rmSync(c.dir,{recursive:true,force:true}));
 run(c,"init","M-122","122","Prompt");
 run(c,"prompt-ready","M-123","Continue the next task");
 let m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-122"];
 assert.equal(m.prompt.status,"PROMPT_PENDING_DELIVERY");
 assert.equal(run(c,"prompt-delivered").status,2);
 assert.equal(run(c,"prompt-delivered","external-message-id-123").status,0);
 m=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8")).missions["M-122"];
 assert.equal(m.prompt.status,"PROMPT_DELIVERED");
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
 run(c,"assign","AI-A","p-a"); run(c,"acknowledge"); run(c,"start"); run(c,"complete","done");
 assert.equal(run(c,"register-next","M-128","128","Second").status,0);
 const s=JSON.parse(fs.readFileSync(path.join(c.dir,"STATE.json"),"utf8"));
 assert.equal(s.current_mission_id,"M-128");
 assert.equal(s.missions["M-127"].status,"DONE");
 assert.equal(s.missions["M-128"].status,"CREATED");
 assert.equal(run(c,"list").status,0);
 assert.equal(run(c,"register-next","M-128","128","Duplicate").status,2);
});
