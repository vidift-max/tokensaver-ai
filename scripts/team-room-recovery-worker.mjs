#!/usr/bin/env node
/**
 * One-shot Team Room recovery worker.
 * It uses its own authenticated task.assign identity to mark expired leases
 * for recovery and select a live replacement candidate. The candidate Runner
 * must still acquire its own lease; this worker never impersonates it.
 */
const args=process.argv.slice(2);
const watch=args.includes("--watch");
const positional=args.filter(value=>value!=="--watch");
const [roomId, taskId, previousOwnerPrincipalId] = positional;
const pollMs=Number(process.env.TEAM_ROOM_RECOVERY_POLL_MS||"2000");
const maxWaitMs=Number(process.env.TEAM_ROOM_RECOVERY_MAX_WAIT_MS||"300000");
const baseUrl=(process.env.TEAM_ROOM_API_URL||"").replace(/\/+$/,"");
const token=process.env.TEAM_ROOM_SESSION_TOKEN||"";
if(!baseUrl||!token||!roomId||!taskId||!previousOwnerPrincipalId||!Number.isInteger(pollMs)||pollMs<250||!Number.isInteger(maxWaitMs)||maxWaitMs<1000){
  console.error("usage: TEAM_ROOM_API_URL=... TEAM_ROOM_SESSION_TOKEN=... [TEAM_ROOM_RECOVERY_POLL_MS=2000] [TEAM_ROOM_RECOVERY_MAX_WAIT_MS=300000] node team-room-recovery-worker.mjs <room_id> <task_id> <previous_owner_principal_id> [--watch]");
  process.exit(2);
}
async function request(method,path){
  const response=await fetch(baseUrl+path,{method,headers:{authorization:"Bearer "+token,accept:"application/json"}});
  const body=await response.text();
  let value={};try{
  const deadline=Date.now()+(watch?maxWaitMs:0);
  let iteration=0;
  for(;;){
    iteration++;
    const recovery=await request("POST",`/team-room/${roomId}/tasks/recover`);
    const candidates=await request("GET",`/team-room/${roomId}/tasks/${taskId}/candidates`);
    const selected=candidates.find(candidate=>candidate.principal_id!==previousOwnerPrincipalId);
    if(selected){
      console.log(JSON.stringify({
        worker:"Team Room recovery worker",
        mode:watch?"watch":"once",
        iteration,
        recovery_required:recovery.recovery_required,
        task_id:taskId,
        previous_owner_principal_id:previousOwnerPrincipalId,
        selected_candidate_principal_id:selected.principal_id,
        selected_candidate_participant_id:selected.participant_id,
        selected_candidate_session_id:selected.session_id,
        selected_candidate_execution_id:selected.execution_id,
        result:"CANDIDATE_SELECTED",
        ownership_note:"candidate must authenticate independently and acquire the task; this worker never impersonates the candidate"
      }));
      break;
    }
    if(!watch||Date.now()>=deadline) throw new Error("no eligible replacement candidate before recovery worker deadline");
    await new Promise(resolve=>setTimeout(resolve,pollMs));
  }
}catch(error){
  console.error(error instanceof Error?error.message:String(error));
  process.exit(1);
}
