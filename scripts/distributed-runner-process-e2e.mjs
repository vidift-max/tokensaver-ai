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
const roles=["runner-a","runner-b","recovery-worker-a","recovery-worker-b"];
const identities=roles.map(role=>({
  role,
  principalId:crypto.randomUUID(),
  participantId:crypto.randomUUID(),
  sessionId:crypto.randomUUID(),
  executionId:crypto.randomUUID(),
  workloadId:crypto.randomUUID(),
  token:crypto.randomBytes(32).toString("hex")
}));
const runnerA=identities[0],runnerB=identities[1],recoveryWorkerA=identities[2],recoveryWorkerB=identities[3];
// Session tokens are ephemeral local fixtures. Never emit credentials or GitHub mask directives
// into stdout: this process output is captured verbatim in the CI evidence artifact.

function psql(sql){
  return spawnSync("psql",[databaseUrl,"-v","ON_ERROR_STOP=1","-At","-c",sql],{encoding:"utf8",timeout:20000});
}
function sqlOk(sql){
  const result=psql(sql);
  if(result.status!==0) throw new Error("psql failed: "+(result.stderr||result.stdout));
  return result.stdout.trim();
}
function candidateEligible(identity){
  const count=sqlOk(`SELECT COUNT(*) FROM room_memberships m
    JOIN sessions s ON s.session_id=m.session_id AND s.principal_id=m.principal_id
    JOIN participants pa ON pa.participant_id=m.participant_id AND pa.principal_id=m.principal_id AND pa.tenant_id=m.tenant_id
    JOIN principals p ON p.principal_id=m.principal_id AND p.tenant_id=m.tenant_id
    JOIN workload_executions w ON w.session_id=s.session_id AND w.principal_id=p.principal_id AND w.participant_id=pa.participant_id
    JOIN principal_permissions pp ON pp.tenant_id=p.tenant_id AND pp.principal_id=p.principal_id AND pp.room_id=m.room_id AND pp.permission='task.transition' AND pp.revoked_at IS NULL
    WHERE m.tenant_id=${quote(tenantId)} AND m.room_id=${quote(roomId)}
      AND m.principal_id=${quote(identity.principalId)} AND m.participant_id=${quote(identity.participantId)}
      AND m.session_id=${quote(identity.sessionId)} AND w.execution_id=${quote(identity.executionId)}
      AND m.status='ACTIVE' AND m.last_seen_at>NOW()-INTERVAL '90 seconds'
      AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW()
      AND p.enabled=TRUE AND pa.enabled=TRUE AND w.status='ACTIVE' AND w.revoked_at IS NULL
      AND w.expires_at>NOW() AND w.heartbeat_at>NOW()-INTERVAL '90 seconds'`);
  return count==="1";
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
    const grants=identity.role.startsWith("recovery-worker")?["task.assign","task.transition","task.read"]:["task.read","task.write","task.transition"];
    for(const permission of grants){
      sqlOk(`INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission)
        VALUES(gen_random_uuid(),${quote(tenantId)},${quote(identity.principalId)},${quote(roomId)},${quote(permission)})`);
    }
  }
}
function runnerEnv(identity,stateDir){
  return {...process.env,MISSION_STATE_DIR:stateDir,TEAM_ROOM_ENV:"test",TEAM_ROOM_API_URL:apiUrl,TEAM_ROOM_ALLOW_HTTP_LOOPBACK:"true",TEAM_ROOM_SESSION_TOKEN:identity.token,TEAM_ROOM_ROOM_ID:roomId,TEAM_ROOM_LEASE_SECONDS:"15"};
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
function stateForMission(stateDir,id){
  return JSON.parse(fs.readFileSync(path.join(stateDir,"STATE.json"),"utf8")).missions[id];
}
function runRecoveryAsync(identity,...args){
  return new Promise((resolve,reject)=>{
    const child=spawn(process.execPath,[recoveryScript,...args],{
      env:{...process.env,TEAM_ROOM_ENV:"test",TEAM_ROOM_API_URL:apiUrl,TEAM_ROOM_ALLOW_HTTP_LOOPBACK:"true",TEAM_ROOM_SESSION_TOKEN:identity.token},
      stdio:["ignore","pipe","pipe"]
    });
    let stdout="",stderr="";
    child.stdout.on("data",chunk=>stdout+=chunk);
    child.stderr.on("data",chunk=>stderr+=chunk);
    child.once("error",reject);
    child.once("close",status=>resolve({status,stdout,stderr,pid:child.pid}));
  });
}
function runCompetingHandoffAccept(identity,handoffId){
  const source=`const response=await fetch(process.env.TEAM_ROOM_API_URL+"/team-room/handoffs/"+process.env.HANDOFF_ID+"/accept",{method:"POST",headers:{authorization:"Bearer "+process.env.TEAM_ROOM_SESSION_TOKEN}}); console.log(JSON.stringify({status:response.status})); process.exitCode=response.status===403?0:1;`;
  return new Promise((resolve,reject)=>{
    const child=spawn(process.execPath,["--input-type=module","-e",source],{
      env:{...process.env,TEAM_ROOM_API_URL:apiUrl,TEAM_ROOM_SESSION_TOKEN:identity.token,HANDOFF_ID:handoffId},
      stdio:["ignore","pipe","pipe"]
    });
    let stdout="",stderr="";
    child.stdout.on("data",chunk=>stdout+=chunk);
    child.stderr.on("data",chunk=>stderr+=chunk);
    child.once("error",reject);
    child.once("close",status=>resolve({status,stdout,stderr,pid:child.pid}));
  });
}
function mustRun(identity,stateDir,...args){
  const result=runSync(identity,stateDir,...args);
  assert.equal(result.status,0,`Runner command failed: ${args.join(" ")}\n${result.stderr}\n${result.stdout}`);
  return result;
}
const dirs=[];
try{
  seed();
  const client=new TeamRoomClient({TEAM_ROOM_ENV:"test",TEAM_ROOM_API_URL:apiUrl,TEAM_ROOM_ALLOW_HTTP_LOOPBACK:"true",TEAM_ROOM_SESSION_TOKEN:runnerA.token,TEAM_ROOM_ROOM_ID:roomId});
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
  assert.equal([startA,startB].filter(result=>result.status===0).length,1,"two independent Runner processes starting concurrently must produce exactly one initial owner");
  const ownerIsA=startA.status===0;
  const owner=ownerIsA?runnerA:runnerB;
  const replacement=ownerIsA?runnerB:runnerA;
  const ownerDir=ownerIsA?stateA:stateB;
  const replacementDir=ownerIsA?stateB:stateA;
  const ownerStart=ownerIsA?startA:startB;
  const replacementStart=ownerIsA?startB:startA;
  assert.equal(replacementStart.status,2,"the losing Runner must be denied while the initial lease is live");
  assert.match(replacementStart.stderr,/HTTP 409/,"the losing Runner must be rejected by the live PostgreSQL lease");
  const ownerState=stateFor(ownerDir);
  const taskId=ownerState.execution.task_id;
  const fencingA=ownerState.execution.fencing_token;
  assert.ok(taskId,"Runner A must persist the PostgreSQL task reference");
  assert.equal(fencingA,1);
  assert.equal(sqlOk(`SELECT COUNT(*) FROM team_tasks WHERE tenant_id=${quote(tenantId)} AND room_id=${quote(roomId)} AND mission_id=${quote(missionUuid)}`),"1","Runner A and B must resolve to one canonical PostgreSQL task");
  const initialTask=await (await fetch(apiUrl+`/team-room/tasks/by-mission/${missionUuid}`,{headers:{authorization:"Bearer "+owner.token}})).json();
  assert.equal(initialTask.task_id,taskId);
  assert.equal(initialTask.owner_principal_id,owner.principalId);

  console.log("Runner A process exited; waiting for the real 15-second PostgreSQL lease to expire.");
  await new Promise(resolve=>setTimeout(resolve,16000));
  assert.equal(sqlOk(`SELECT CASE WHEN lease_expires_at<=NOW() THEN 'EXPIRED' ELSE 'LIVE' END FROM team_tasks WHERE task_id=${quote(taskId)}`),"EXPIRED","the initial lease must expire by database time, without mutating its expiry timestamp");
  // Make B the first live candidate and Recovery Worker A a second live,
  // independently authorized contender. No preflight recovery request is made:
  // the two real workers must race to persist RECOVERY_REQUIRED themselves.
  for(const [identity,age] of [[owner,60],[replacement,5],[recoveryWorkerA,25],[recoveryWorkerB,35]]){
    sqlOk(`UPDATE room_memberships SET last_seen_at=NOW()-INTERVAL '${age} seconds' WHERE session_id=${quote(identity.sessionId)};
      UPDATE workload_executions SET heartbeat_at=NOW()-INTERVAL '${age} seconds' WHERE execution_id=${quote(identity.executionId)};`);
  }
  assert.ok(candidateEligible(replacement),"addressed replacement must be an eligible live candidate before recovery starts");
  assert.ok(candidateEligible(recoveryWorkerA),"competing candidate must independently be eligible before recovery starts");
  const [recoveryResultA,recoveryResultB]=await Promise.all([
    runRecoveryAsync(recoveryWorkerA,roomId,taskId,owner.principalId,"--watch"),
    runRecoveryAsync(recoveryWorkerB,roomId,taskId,owner.principalId,"--watch")
  ]);
  assert.equal(recoveryResultA.status,0,"recovery worker A failed: "+recoveryResultA.stderr);
  assert.equal(recoveryResultB.status,0,"recovery worker B failed: "+recoveryResultB.stderr);
  const recoveryEvidenceA=JSON.parse(recoveryResultA.stdout.trim().split("\n").at(-1));
  const recoveryEvidenceB=JSON.parse(recoveryResultB.stdout.trim().split("\n").at(-1));
  assert.equal(recoveryEvidenceA.result,"HANDOFF_OFFERED");
  assert.equal(recoveryEvidenceB.result,"HANDOFF_OFFERED");
  assert.equal(recoveryEvidenceA.handoff_id,recoveryEvidenceB.handoff_id,"recovery workers must converge on one pending handoff");
  assert.equal(recoveryEvidenceA.handoff_status,"PENDING");
  assert.equal(recoveryEvidenceA.selected_candidate_principal_id,replacement.principalId);
  assert.equal(recoveryEvidenceB.selected_candidate_principal_id,replacement.principalId);
  assert.deepEqual([recoveryEvidenceA.recovery_required,recoveryEvidenceB.recovery_required].sort((a,b)=>a-b),[0,1],"exactly one independent Recovery Worker must win the PostgreSQL recovery transition");
  const recoveryTaskResponse=await fetch(apiUrl+`/team-room/tasks/by-mission/${missionUuid}`,{headers:{authorization:"Bearer "+replacement.token}});
  assert.equal(recoveryTaskResponse.status,200);
  const recoveryTask=await recoveryTaskResponse.json();
  assert.equal(recoveryTask.status,"RECOVERY_REQUIRED","PostgreSQL must persist recovery state before replacement acquire");

  const handoffId=recoveryEvidenceA.handoff_id;
  const [takeover,competingAccept]=await Promise.all([
    runAsync(replacement,replacementDir,"handoff-accept",handoffId),
    runCompetingHandoffAccept(recoveryWorkerA,handoffId)
  ]);
  assert.equal(takeover.status,0,"addressed Runner B must accept the authenticated recovery handoff: "+takeover.stderr);
  assert.equal(competingAccept.status,0,"unaddressed candidate must be rejected: "+competingAccept.stderr);
  assert.equal(JSON.parse(competingAccept.stdout.trim()).status,403,"competing candidate must receive HTTP 403");
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

  mustRun(replacement,replacementDir,"renew");
  mustRun(replacement,replacementDir,"checkpoint","recovered checkpoint accepted by PostgreSQL");
  mustRun(replacement,replacementDir,"complete","Runner B completed after authenticated handoff");
  const handoffResponse=await fetch(apiUrl+`/team-room/handoffs/${handoffId}`,{headers:{authorization:"Bearer "+replacement.token}});
  assert.equal(handoffResponse.status,200);
  const finalHandoff=await handoffResponse.json();
  assert.equal(finalHandoff.status,"COMPLETED","task completion must atomically complete its handoff");
  const finalResponse=await fetch(apiUrl+`/team-room/tasks/by-mission/${missionUuid}`,{headers:{authorization:"Bearer "+replacement.token}});
  assert.equal(finalResponse.status,200);
  const finalTask=await finalResponse.json();
  assert.equal(finalTask.status,"COMPLETED");
  assert.equal(finalTask.fencing_token,2);

  // Separate owner-initiated transfer: A retains fence 1 while the
  // offer is pending; only the exact B session can atomically receive fence 2.
  const ownerHandoffMission="TEST-MISSION-OWNER-HANDOFF-001";
  const ownerHandoffUuid=client.missionUuid(ownerHandoffMission);
  const ownerHandoffA=fs.mkdtempSync(path.join(os.tmpdir(),"owner-handoff-a-"));dirs.push(ownerHandoffA);
  const ownerHandoffB=fs.mkdtempSync(path.join(os.tmpdir(),"owner-handoff-b-"));dirs.push(ownerHandoffB);
  for(const [identity,stateDir,label] of [[owner,ownerHandoffA,"Owner A"],[replacement,ownerHandoffB,"Recipient B"]]){
    mustRun(identity,stateDir,"init",ownerHandoffMission,"2","Owner-initiated authenticated handoff proof");
    mustRun(identity,stateDir,"assign",label,identity.participantId);
    mustRun(identity,stateDir,"acknowledge");
  }
  mustRun(owner,ownerHandoffA,"start");
  mustRun(owner,ownerHandoffA,"checkpoint","owner checkpoint before authenticated transfer");
  const ownerHandoffTask=stateForMission(ownerHandoffA,ownerHandoffMission).execution.task_id;
  const persistedOwnerCheckpoint=JSON.parse(sqlOk(`SELECT checkpoint::text FROM team_tasks WHERE task_id=${quote(ownerHandoffTask)}`));
  assert.equal(persistedOwnerCheckpoint.description,"owner checkpoint before authenticated transfer","PostgreSQL must store the checkpoint document without an extra wrapper");
  assert.equal(stateForMission(ownerHandoffA,ownerHandoffMission).execution.fencing_token,1);
  for(const [identity,age] of [[owner,40],[replacement,5],[recoveryWorkerA,15],[recoveryWorkerB,25]]){
    sqlOk(`UPDATE room_memberships SET last_seen_at=NOW()-INTERVAL '${age} seconds' WHERE session_id=${quote(identity.sessionId)};
      UPDATE workload_executions SET heartbeat_at=NOW()-INTERVAL '${age} seconds' WHERE execution_id=${quote(identity.executionId)};`);
  }
  assert.ok(candidateEligible(replacement),"addressed recipient must have an active authenticated session, execution, membership and task.transition permission");
  assert.ok(candidateEligible(recoveryWorkerA),"competing recipient must independently satisfy the same live-candidate authorization checks");
  mustRun(owner,ownerHandoffA,"handoff-create",replacement.principalId,replacement.participantId,replacement.sessionId,replacement.executionId,"planned owner-initiated transfer");
  const ownerHandoffId=stateForMission(ownerHandoffA,ownerHandoffMission).handoff.handoff_id;
  const pendingResponse=await fetch(apiUrl+`/team-room/handoffs/${ownerHandoffId}`,{headers:{authorization:"Bearer "+owner.token}});
  assert.equal(pendingResponse.status,200);
  assert.equal((await pendingResponse.json()).status,"PENDING","source must retain ownership while offer is pending");
  const [ownerHandoffAccept,ownerHandoffCompetitor]=await Promise.all([
    runAsync(replacement,ownerHandoffB,"handoff-accept",ownerHandoffId),
    runCompetingHandoffAccept(recoveryWorkerA,ownerHandoffId)
  ]);
  assert.equal(ownerHandoffAccept.status,0,"addressed recipient B must accept owner-created handoff: "+ownerHandoffAccept.stderr);
  assert.equal(ownerHandoffCompetitor.status,0,"second eligible candidate must be denied: "+ownerHandoffCompetitor.stderr);
  assert.equal(JSON.parse(ownerHandoffCompetitor.stdout.trim()).status,403);
  const ownerHandoffFenceB=stateForMission(ownerHandoffB,ownerHandoffMission).execution.fencing_token;
  assert.equal(ownerHandoffFenceB,2);
  assert.equal(stateForMission(ownerHandoffB,ownerHandoffMission).checkpoint.description,"owner checkpoint before authenticated transfer","recipient must restore the exact server checkpoint before continuing");
  assert.equal(stateForMission(ownerHandoffB,ownerHandoffMission).checkpoint_version,1);
  const ownerHandoffStale={};
  for(const [key,args] of [["renew",["renew"]],["checkpoint",["checkpoint","stale owner handoff checkpoint"]],["complete",["complete","stale owner handoff complete"]],["release",["release"]]]){
    const isolated=fs.mkdtempSync(path.join(os.tmpdir(),"owner-handoff-stale-"));dirs.push(isolated);
    fs.writeFileSync(path.join(isolated,"STATE.json"),JSON.stringify(JSON.parse(fs.readFileSync(path.join(ownerHandoffA,"STATE.json"),"utf8"))));
    const result=runSync(owner,isolated,...args);
    assert.equal(result.status,2,`previous owner A ${key} must be rejected after authenticated handoff`);
    assert.match(result.stderr,/HTTP 412/,result.stderr);
    ownerHandoffStale[key]="REJECTED";
  }
  mustRun(replacement,ownerHandoffB,"renew");
  mustRun(replacement,ownerHandoffB,"checkpoint","B checkpoint accepted after owner-initiated handoff");
  mustRun(replacement,ownerHandoffB,"complete","B completes owner-initiated handoff");
  mustRun(replacement,ownerHandoffB,"handoff-complete");
  const ownerHandoffFinalResponse=await fetch(apiUrl+`/team-room/handoffs/${ownerHandoffId}`,{headers:{authorization:"Bearer "+replacement.token}});
  assert.equal(ownerHandoffFinalResponse.status,200);
  const ownerHandoffFinal=await ownerHandoffFinalResponse.json();
  assert.equal(ownerHandoffFinal.status,"COMPLETED");
  const ownerHandoffFinalTask=await (await fetch(apiUrl+`/team-room/tasks/by-mission/${ownerHandoffUuid}`,{headers:{authorization:"Bearer "+replacement.token}})).json();
  assert.equal(ownerHandoffFinalTask.status,"COMPLETED");
  assert.equal(ownerHandoffFinalTask.fencing_token,2);

  const evidence={
    mission_label:missionLabel,
    mission_id:missionUuid,
    task_id:taskId,
    runner_a_process_pid:ownerStart.pid,
    runner_b_initial_rejected_process_pid:replacementStart.pid,
    runner_b_process_pid:takeover.pid,
    competing_candidate_process_pid:competingAccept.pid,
    runner_a_principal_id:owner.principalId,
    runner_a_session_id:owner.sessionId,
    runner_a_execution_id:owner.executionId,
    runner_b_principal_id:replacement.principalId,
    runner_b_session_id:replacement.sessionId,
    runner_b_execution_id:replacement.executionId,
    recovery_worker_a_principal_id:recoveryWorkerA.principalId,
    recovery_worker_a_session_id:recoveryWorkerA.sessionId,
    recovery_worker_b_principal_id:recoveryWorkerB.principalId,
    recovery_worker_b_session_id:recoveryWorkerB.sessionId,
    recovery_worker_a_pid:recoveryResultA.pid,
    recovery_worker_b_pid:recoveryResultB.pid,
    concurrent_recovery_workers:2,
    recovery_transition_count:recoveryEvidenceA.recovery_required+recoveryEvidenceB.recovery_required,
    recovery_status_persisted:recoveryTask.status,
    recovery_worker_selected_b:recoveryEvidenceA.selected_candidate_principal_id,
    fencing_a:fencingA,
    fencing_b:fencingB,
    lease_expiry_wait_seconds:16,
    lease_expired_by_database_clock:true,
    initial_acquire:"ACCEPTED",
    runner_b_initial_acquire:"REJECTED",
    runner_initial_start_concurrent:true,
    task_count_for_mission:1,
    takeover:"ACCEPTED",
    handoff_id:handoffId,
    handoff_status:finalHandoff.status,
    handoff_from_fencing_token:finalHandoff.from_fencing_token,
    handoff_to_fencing_token:finalHandoff.to_fencing_token,
    competing_candidate_accept:"REJECTED_HTTP_403",
    competing_candidate_was_eligible:true,
    runner_b_renew:"ACCEPTED",
    stale_a_renew:staleResults.renew,
    stale_a_checkpoint:staleResults.checkpoint,
    stale_a_complete:staleResults.complete,
    stale_a_release:staleResults.release,
    runner_b_checkpoint:"ACCEPTED",
    runner_b_checkpoint_version:finalTask.checkpoint_version,
    runner_b_complete:"ACCEPTED",
    final_task_status:finalTask.status,
    owner_initiated_handoff_id:ownerHandoffId,
    owner_initiated_handoff_task_id:ownerHandoffTask,
    owner_initiated_handoff_status:ownerHandoffFinal.status,
    owner_initiated_handoff_fencing_a:1,
    owner_initiated_handoff_fencing_b:ownerHandoffFenceB,
    owner_initiated_checkpoint_preserved:true,
    owner_initiated_competing_candidate:"REJECTED_HTTP_403",
    owner_initiated_competing_candidate_was_eligible:true,
    owner_initiated_stale_a:ownerHandoffStale,
    owner_initiated_runner_b_renew:"ACCEPTED",
    owner_initiated_runner_b_checkpoint:"ACCEPTED",
    owner_initiated_runner_b_complete:"ACCEPTED",
    owner_initiated_final_task_status:ownerHandoffFinalTask.status,
    distinct_runner_sessions:owner.sessionId!==replacement.sessionId,
    distinct_runner_processes:ownerStart.pid!==takeover.pid,
    database:"PostgreSQL 16 shared Team Room service",
    result:"PASS"
  };
  const outputDir=process.env.DISTRIBUTED_EVIDENCE_DIR||"distributed-runner-evidence";
  fs.mkdirSync(outputDir,{recursive:true});
  fs.writeFileSync(path.join(outputDir,"runner-process-evidence.json"),JSON.stringify({commit_sha:process.env.MISSION_SOURCE_SHA||process.env.GITHUB_SHA||null,merge_commit_sha:process.env.GITHUB_SHA||null,...evidence},null,2)+"\n");
  console.log("RUNNER_PROCESS_EVIDENCE="+JSON.stringify(evidence));
}catch(error){
  console.error(error instanceof Error?error.stack:String(error));
  process.exitCode=1;
}finally{
  for(const dir of dirs) fs.rmSync(dir,{recursive:true,force:true});
}
