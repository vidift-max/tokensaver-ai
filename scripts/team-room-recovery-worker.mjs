#!/usr/bin/env node
import { normalizeTeamRoomBaseUrl, resolveTeamRoomRequestUrl } from "./team-room-security.mjs";
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
const configuredBaseUrl=process.env.TEAM_ROOM_API_URL||"";
const token=process.env.TEAM_ROOM_SESSION_TOKEN||"";
const allowHttpLoopback=process.env.TEAM_ROOM_ALLOW_HTTP_LOOPBACK==="true";
const environment=process.env.TEAM_ROOM_ENV||"production";
if(!configuredBaseUrl||!token||!roomId||!taskId||!previousOwnerPrincipalId||
   !Number.isSafeInteger(pollMs)||pollMs<250||pollMs>60000||
   !Number.isSafeInteger(maxWaitMs)||maxWaitMs<1000||maxWaitMs>900000){
  console.error("usage: TEAM_ROOM_API_URL=... TEAM_ROOM_SESSION_TOKEN=... [TEAM_ROOM_RECOVERY_POLL_MS=2000] [TEAM_ROOM_RECOVERY_MAX_WAIT_MS=300000] node team-room-recovery-worker.mjs <room_id> <task_id> <previous_owner_principal_id> [--watch]");
  process.exit(2);
}
let baseUrl;
try { baseUrl=normalizeTeamRoomBaseUrl(configuredBaseUrl,{allowHttpLoopback,environment}); }
catch { console.error("TEAM_ROOM_API_URL is invalid or insecure"); process.exit(2); }
async function request(method,route,body){
  let requestUrl;
  try { requestUrl=resolveTeamRoomRequestUrl(baseUrl,route,{allowHttpLoopback,environment}); }
  catch { throw new Error("invalid Team Room API route"); }
  let response;
  try {
    response=await fetch(requestUrl,{
      method,
      headers:{authorization:"Bearer "+token,accept:"application/json",...(body?{"content-type":"application/json"}:{})},
      ...(body?{body:JSON.stringify(body)}:{}),
      redirect:"error",
      signal:AbortSignal.timeout(10000)
    });
  } catch {
    throw new Error("Team Room recovery request failed or redirect was rejected");
  }
  const responseText=await response.text();
  let value={};
  try{value=responseText?JSON.parse(responseText):{};}catch{value={error:"non-JSON response"};}
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
      const handoff=await request("POST",`/team-room/tasks/${taskId}/handoffs/recovery`,{
        to_principal_id:selected.principal_id,
        to_participant_id:selected.participant_id,
        to_session_id:selected.session_id,
        to_execution_id:selected.execution_id,
        reason:"lease_expired_recovery"
      });
      console.log(JSON.stringify({
        worker:"Team Room recovery worker",
        mode:watch?"watch":"once",
        iteration,
        recovery_required:recovery.recovery_required,
        task_id:taskId,
        handoff_id:handoff.handoff_id,
        handoff_status:handoff.status,
        previous_owner_principal_id:previousOwnerPrincipalId,
        selected_candidate_principal_id:selected.principal_id,
        selected_candidate_participant_id:selected.participant_id,
        selected_candidate_session_id:selected.session_id,
        selected_candidate_execution_id:selected.execution_id,
        result:"HANDOFF_OFFERED",
        ownership_note:"the addressed candidate must authenticate independently and accept; this worker never impersonates the candidate"
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
