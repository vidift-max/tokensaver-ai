#!/usr/bin/env node
/**
 * Authenticated Team Room recovery worker.
 * In --watch mode it polls for an eligible replacement until the bounded deadline.
 * It never impersonates the candidate: the candidate Runner acquires its own lease.
 */
const args=process.argv.slice(2);
const watch=args.includes("--watch");
const positional=args.filter(value=>value!=="--watch");
const [roomId,taskId,previousOwnerPrincipalId]=positional;
const pollMs=Number(process.env.TEAM_ROOM_RECOVERY_POLL_MS||"2000");
const maxWaitMs=Number(process.env.TEAM_ROOM_RECOVERY_MAX_WAIT_MS||"300000");
const baseUrl=(process.env.TEAM_ROOM_API_URL||"").replace(/\/+$/,"");
const token=process.env.TEAM_ROOM_SESSION_TOKEN||"";
if(!baseUrl||!token||!roomId||!taskId||!previousOwnerPrincipalId||
   !Number.isSafeInteger(pollMs)||pollMs<250||pollMs>60000||
   !Number.isSafeInteger(maxWaitMs)||maxWaitMs<1000||maxWaitMs>900000){
  console.error("usage: TEAM_ROOM_API_URL=... TEAM_ROOM_SESSION_TOKEN=... [TEAM_ROOM_RECOVERY_POLL_MS=2000] [TEAM_ROOM_RECOVERY_MAX_WAIT_MS=300000] node team-room-recovery-worker.mjs <room_id> <task_id> <previous_owner_principal_id> [--watch]");
  process.exit(2);
}
async function request(method,route){
  const response=await fetch(baseUrl+route,{
    method,
    headers:{authorization:"Bearer "+token,accept:"application/json"},
    signal:AbortSignal.timeout(10000)
  });
  const body=await response.text();
  let value={};
  try{value=body?JSON.parse(body):{};}catch{}
  if(!response.ok) throw new Error("Team Room recovery request rejected: "+method+" "+route+" HTTP "+response.status);
  return value;
}
async function main(){
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
      return;
    }
    if(!watch||Date.now()>=deadline) throw new Error("no eligible replacement candidate before recovery worker deadline");
    await new Promise(resolve=>setTimeout(resolve,pollMs));
  }
}
main().catch(error=>{
  console.error(error instanceof Error?error.message:String(error));
  process.exitCode=1;
});
