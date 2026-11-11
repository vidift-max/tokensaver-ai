#!/usr/bin/env node
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { TeamRoomClient } from "./team-room-client.mjs";

const repositoryRoot=process.cwd();
const runnerScript=path.join(repositoryRoot,"scripts/mission-runner.mjs");
const recoveryScript=path.join(repositoryRoot,"scripts/team-room-recovery-worker.mjs");
const databaseUrl=process.env.DATABASE_URL;
const apiUrl=process.env.TEAM_ROOM_API_URL||"http://127.0.0.1:8081";
assert.ok(databaseUrl,"DATABASE_URL required");
const missionLabel="TEST-MISSION-DISTRIBUTED-001";
const roomId=crypto.randomUUID();
const tenantId=crypto.randomUUID();
const roles=["runner-a","runner-b","recovery-worker"];
const identities=roles.map(role=>({
  role,
  principalId:crypto.randomUUID(),
  participantId:crypto.randomUUID(),
  sessionId:crypto.randomUUID(),
  executionId:crypto.randomUUID(),
  workloadId:crypto.randomUUID(),
  token:crypto.randomBytes(32).toString("hex")
}));
const runnerA=identities[0],runnerB=identities[1],recoveryWorker=identities[2];
for(const identity of identities) process.stdout.write("::add-mask::"+identity.token+"\n");

function psql(sql){
  return spawnSync("psql",[databaseUrl,"-v","ON_ERROR_STOP=1","-At","-c",sql],{encoding:"utf8",timeout:20000});
}
function sqlOk(sql){
  const result=psql(sql);
  if(result.status!==0) throw new Error("psql failed: "+(result.stderr||result.stdout));
  return result.stdout.trim();
}
function quote(value){return "'"+String(value).replaceAll("'","''")+"'";}
function seed(){
  sqlOk(`INSERT INTO tenants(tenant_id,name) VALUES(${quote(tenantId)},'distributed-runner-process-e2e');
    INSERT INTO rooms(room_id,tenant_id,name) VALUES(${quote(roomId)},${quote(tenantId)},'distributed-runner-process-e2e-room');`);
  for(const identity of identities){
    const issuer="ci-fixture";
    const subject="subject-"+identity.role;
    const tokenHash=crypto.createHash("sha256").update(identity.token).digest("hex");
    sqlOk(`INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject)
      VALUES(${quote(identity.principalId)},${quote(tenantId)},'AI',${quote(issuer)},${quote(subject)});
      INSERT INTO participants(participant_id,tenant_id,principal_id,display_name)
      VALUES(${quote(identity.participantId)},${quote(tenantId)},${quote(identity.principalId)},${quote(identity.role)});
      INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status)
      VALUES(${quote(identity.workloadId)},${quote(tenantId)},${quote(identity.principalId)},${quote(issuer)},${quote(subject)},'ACTIVE');
      INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,token_hash,issued_at,expires_at,status)
      VALUES(${quote(identity.sessionId)},${quote(tenantId)},${quote(roomId)},${quote(identity.principalId)},decode(${quote(tokenHash)},'hex'),NOW(),NOW()+INTERVAL '10 minutes','ACTIVE');
      INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status)
      VALUES(${quote(identity.executionId)},${quote(tenantId)},${quote(roomId)},${quote(identity.participantId)},${quote(identity.principalId)},${quote(identity.sessionId)},${quote(subject)},NOW(),NOW()+INTERVAL '10 minutes','ACTIVE');
      INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status)
      VALUES(gen_random_uuid(),${quote(tenantId)},${quote(roomId)},${quote(identity.participantId)},${quote(identity.principalId)},${quote(identity.sessionId)},'ACTIVE');`);
    const grants=identity.role==="recovery-worker"?["task.assign"]:["task.read","task.write","task.transition"];
    for(const permission of grants){
      sqlOk(`INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission)
        VALUES(gen_random_uuid(),${quote(tenantId)},${quote(identity.principalId)},${quote(roomId)},${quote(permission)})`);
    }
  }
}
function runnerEnv(identity,stateDir){
  return {...process.env,MISSION_STATE_DIR:stateDir,TEAM_ROOM_API_URL:apiUrl,TEAM_ROOM_SESSION_TOKEN:identity.token,TEAM_ROOM_ROOM_ID:roomId};
}
function runSync(identity,stateDir,...args){
  const result=spawnSync(process.execPath,[runnerScript,...args],{encoding:"utf8",env:runnerEnv(identity,stateDir),timeout:20000,maxBuffer:1024*1024});
  return {status:result.status,stdout:result.stdout||"",stderr:result.stderr||"",pid:result.pid};
}
function runAsync(identity,stateDir,...args){
  return new Promise((resolve,reject)=>{
    const child=spawn(process.execPath,[runnerScript,...args],{encoding:"utf8",env:runnerEnv(identity,stateDir),stdio:["ignore","pipe","pipe"]});
    let stdout="",stderr="";
    child.stdout.on("data",chunk=>stdout+=chunk);
    child.stderr.on("data",chunk=>stderr+=chunk);
    child.once("error",reject);
    child.once("close",(status)=>resolve({status,stdout,stderr,pid:child.pid}));
  });
}
function stateFor(stateDir){
  return JSON.parse(fs.readFileSync(path.join(stateDir,"STATE.json"),"utf8")).missions[missionLabel];
}
function mustRun(identity,stateDir,...args){
  const result=runSync(identity,stateDir,...args);
  assert.equal(result.status,0,`Runner command failed: ${args.join(" ")}\n${result.stderr}\n${result.stdout}`);
  return result;
}
const dirs=[];
try{
  seed();
  const client=new TeamRoomClient({TEAM_ROOM_API_URL:apiUrl,TEAM_ROOM_SESSION_TOKEN:runnerA.token,TEAM_ROOM_ROOM_ID:roomId});
  const missionUuid=client.missionUuid(missionLabel);
  const stateA=fs.mkdtempSync(path.join(os.tmpdir(),"runner-a-"));dirs.push(stateA);
  const stateB=fs.mkdtempSync(path.join(os.tmpdir(),"runner-b-"));dirs.push(stateB);
  for(const [identity,stateDir,label] of [[runnerA,stateA,"Runner A"],[runnerB,stateB,"Runner B"]]){
    mustRun(identity,stateDir,"init",missionLabel,"1","PostgreSQL distributed process proof");
    mustRun(identity,stateDir,"assign",label,identity.participantId);
    mustRun(identity,stateDir,"acknowledge");
  }

  const [startA,startB]=await Promise.all([
    runAsync(runnerA,stateA,"start"),
    runAsync(runnerB,stateB,"start")
  ]);
  assert.equal([startA,startB].filter(result=>result.status===0).length,1,"two independent Runner processes must produce exactly one initial owner");
  const ownerIsA=startA.status===0;
  const owner=ownerIsA?runnerA:runnerB;
  const replacement=ownerIsA?runnerB:runnerA;
  const ownerDir=ownerIsA?stateA:stateB;
  const replacementDir=ownerIsA?stateB:stateA;
  const ownerStart=ownerIsA?startA:startB;
  const replacementStart=ownerIsA?startB:startA;
  const ownerState=stateFor(ownerDir);
  const taskId=ownerState.execution.task_id;
  const fencingA=ownerState.execution.fencing_token;
  assert.ok(taskId,"initial owner must persist the PostgreSQL task reference");
  assert.equal(fencingA,1);
  assert.equal(replacementStart.status,2,"second Runner must be denied while first lease is live");
  const initialTask=await (await fetch(apiUrl+`/team-room/tasks/by-mission/${missionUuid}`,{headers:{authorization:"Bearer "+owner.token}})).json();
  assert.equal(initialTask.task_id,taskId);
  assert.equal(initialTask.owner_principal_id,owner.principalId);

  sqlOk(`UPDATE team_tasks SET lease_expires_at=NOW()-INTERVAL '1 second' WHERE task_id=${quote(taskId)} AND fencing_token=1`);
  const recoveryResult=spawnSync(process.execPath,[recoveryScript,roomId,taskId,owner.principalId],{
    encoding:"utf8",
    env:{...process.env,TEAM_ROOM_API_URL:apiUrl,TEAM_ROOM_SESSION_TOKEN:recoveryWorker.token},
    timeout:20000,maxBuffer:1024*1024
  });
  assert.equal(recoveryResult.status,0,"recovery worker must mark expiry and select a different eligible candidate: "+recoveryResult.stderr);
  const recoveryEvidence=JSON.parse(recoveryResult.stdout.trim().split("\n").at(-1));
  assert.equal(recoveryEvidence.result,"CANDIDATE_SELECTED");
  assert.equal(recoveryEvidence.selected_candidate_principal_id,replacement.principalId);

  const takeover=mustRun(replacement,replacementDir,"start");
  const replacementState=stateFor(replacementDir);
  const fencingB=replacementState.execution.fencing_token;
  assert.equal(replacementState.execution.task_id,taskId);
  assert.equal(fencingB,2,"replacement Runner must receive the next fencing generation");

  const staleResults={};
  for(const [key,args] of [
    ["renew",["renew"]],
    ["checkpoint",["checkpoint","stale checkpoint"]],
    ["complete",["complete","stale completion"]],
    ["release",["release"]]
  ]){
    const isolated=fs.mkdtempSync(path.join(os.tmpdir(),"runner-a-stale-"));dirs.push(isolated);
    fs.writeFileSync(path.join(isolated,"STATE.json"),JSON.stringify(JSON.parse(fs.readFileSync(path.join(ownerDir,"STATE.json"),"utf8"))));
    const result=runSync(owner,isolated,...args);
    assert.equal(result.status,2,`stale owner ${key} must be rejected by Team Room`);
    assert.match(result.stderr,/HTTP 412/,result.stderr);
    staleResults[key]="REJECTED";
  }

  mustRun(replacement,replacementDir,"complete","Runner B completed after recovery");
  const finalResponse=await fetch(apiUrl+`/team-room/tasks/by-mission/${missionUuid}`,{headers:{authorization:"Bearer "+replacement.token}});
  assert.equal(finalResponse.status,200);
  const finalTask=await finalResponse.json();
  assert.equal(finalTask.status,"COMPLETED");
  assert.equal(finalTask.fencing_token,2);

  const evidence={
    mission_label:missionLabel,
    mission_id:missionUuid,
    task_id:taskId,
    runner_a_process_pid:ownerStart.pid,
    runner_b_process_pid:replacementStart.pid,
    runner_a_principal_id:owner.principalId,
    runner_a_session_id:owner.sessionId,
    runner_a_execution_id:owner.executionId,
    runner_b_principal_id:replacement.principalId,
    runner_b_session_id:replacement.sessionId,
    runner_b_execution_id:replacement.executionId,
    recovery_worker_principal_id:recoveryWorker.principalId,
    recovery_worker_selected_b:recoveryEvidence.selected_candidate_principal_id,
    fencing_a:fencingA,
    fencing_b:fencingB,
    initial_acquire:"ACCEPTED",
    concurrent_second_acquire:"REJECTED",
    takeover:"ACCEPTED",
    stale_a_renew:staleResults.renew,
    stale_a_checkpoint:staleResults.checkpoint,
    stale_a_complete:staleResults.complete,
    stale_a_release:staleResults.release,
    runner_b_complete:"ACCEPTED",
    final_task_status:finalTask.status,
    distinct_runner_sessions:owner.sessionId!==replacement.sessionId,
    distinct_runner_processes:ownerStart.pid!==replacementStart.pid,
    database:"PostgreSQL 16 shared Team Room service",
    result:"PASS"
  };
  const outputDir=process.env.DISTRIBUTED_EVIDENCE_DIR||"distributed-runner-evidence";
  fs.mkdirSync(outputDir,{recursive:true});
  fs.writeFileSync(path.join(outputDir,"runner-process-evidence.json"),JSON.stringify({commit_sha:process.env.GITHUB_SHA||null,...evidence},null,2)+"\n");
  console.log("RUNNER_PROCESS_EVIDENCE="+JSON.stringify(evidence));
}catch(error){
  console.error(error instanceof Error?error.stack:String(error));
  process.exitCode=1;
}finally{
  for(const dir of dirs) fs.rmSync(dir,{recursive:true,force:true});
}
