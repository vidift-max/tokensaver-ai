#!/usr/bin/env node
/**
 * One-shot Team Room recovery worker.
 * It uses its own authenticated task.assign identity to mark expired leases
 * for recovery and select a live replacement candidate. The candidate Runner
 * must still acquire its own lease; this worker never impersonates it.
 */
const [roomId, taskId, previousOwnerPrincipalId] = process.argv.slice(2);
const baseUrl=(process.env.TEAM_ROOM_API_URL||"").replace(/\/+$/,"");
const token=process.env.TEAM_ROOM_SESSION_TOKEN||"";
if(!baseUrl||!token||!roomId||!taskId||!previousOwnerPrincipalId){
  console.error("usage: TEAM_ROOM_API_URL=... TEAM_ROOM_SESSION_TOKEN=... node team-room-recovery-worker.mjs <room_id> <task_id> <previous_owner_principal_id>");
  process.exit(2);
}
async function request(method,path){
  const response=await fetch(baseUrl+path,{method,headers:{authorization:"Bearer "+token,accept:"application/json"}});
  const body=await response.text();
  let value={};try{value=body?JSON.parse(body):{};}catch{}
  if(!response.ok) throw new Error("Team Room recovery request rejected: "+method+" "+path+" HTTP "+response.status);
  return value;
}
try{
  const recovery=await request("POST",`/team-room/${roomId}/tasks/recover`);
  const candidates=await request("GET",`/team-room/${roomId}/tasks/${taskId}/candidates`);
  const selected=candidates.find(candidate=>candidate.principal_id!==previousOwnerPrincipalId);
  if(!selected) throw new Error("no eligible replacement candidate distinct from the expired owner");
  console.log(JSON.stringify({
    worker:"Team Room recovery worker",
    recovery_required:recovery.recovery_required,
    task_id:taskId,
    previous_owner_principal_id:previousOwnerPrincipalId,
    selected_candidate_principal_id:selected.principal_id,
    selected_candidate_participant_id:selected.participant_id,
    selected_candidate_session_id:selected.session_id,
    selected_candidate_execution_id:selected.execution_id,
    result:"CANDIDATE_SELECTED"
  }));
}catch(error){
  console.error(error instanceof Error?error.message:String(error));
  process.exit(1);
}
