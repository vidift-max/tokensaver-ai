import crypto from "node:crypto";
import { createServer } from "node:http";
import { parentPort } from "node:worker_threads";

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
 const forced=url.pathname.match(/^\/test\/takeover\/([^/]+)$/);
 if(req.method==="POST" && forced){
  const task=[...tasks.values()].find(t=>t.task_id===forced[1]);if(!task)return send(404,{});
  task.status="REASSIGNED";task.fencing_token++;task.owner_principal_id=identity.principal_id;task.owner_participant_id=identity.participant_id;task.owner_session_id=identity.session_id;task.owner_execution_id=identity.execution_id;task.lease_expires_at=new Date(Date.now()+600000).toISOString();
  return send(200,task);
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
parentPort.postMessage({port:server.address().port,identity});
