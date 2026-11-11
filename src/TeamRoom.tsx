import { useCallback, useEffect, useRef, useState } from "react";

type Message = { message_id:string; sender_principal_id:string; parent_message_id:string|null; body:string; created_at:string };
type Presence = { participant_id:string; principal_id:string; display_name:string; provider:string; last_activity_at:string; current_task_id:string|null; connected:boolean; state:string };
type Task = { task_id:string; mission_id:string; status:string; owner_principal_id:string|null; fencing_token:number; checkpoint:unknown; checkpoint_version:number };
type Decision = { decision_id:string; title:string; decision:string; author_principal_id:string; created_at:string };
type Attachment = { attachment_id:string; file_name:string; storage_ref:string; channel_id:string; created_at:string };

export default function TeamRoom() {
  const [apiUrl,setApiUrl]=useState("http://127.0.0.1:8081");
  const [roomId,setRoomId]=useState("");
  const [token,setToken]=useState("");
  const [connected,setConnected]=useState(false);
  const [connection,setConnection]=useState("DISCONNECTED");
  const [messages,setMessages]=useState<Message[]>([]);
  const [unreadCount,setUnreadCount]=useState(0);
  const [participants,setParticipants]=useState<Presence[]>([]);
  const [tasks,setTasks]=useState<Task[]>([]);
  const [decisions,setDecisions]=useState<Decision[]>([]);
  const [attachments,setAttachments]=useState<Attachment[]>([]);
  const [draft,setDraft]=useState("");
  const [replyTo,setReplyTo]=useState<string|null>(null);
  const [privatePrincipal,setPrivatePrincipal]=useState("");
  const [privateMode,setPrivateMode]=useState(false);
  const [missionId,setMissionId]=useState("");
  const [taskTitle,setTaskTitle]=useState("");
  const [decisionTitle,setDecisionTitle]=useState("");
  const [decisionBody,setDecisionBody]=useState("");
  const [attachmentName,setAttachmentName]=useState("");
  const [attachmentRef,setAttachmentRef]=useState("");
  const [channelId,setChannelId]=useState("");
  const [presenceState,setPresenceState]=useState("ONLINE");
  const [error,setError]=useState("");
  const [busy,setBusy]=useState(false);
  const socketRef=useRef<WebSocket|null>(null);

  const request=useCallback(async(path:string,init:RequestInit={})=>{
    const headers=new Headers(init.headers);headers.set("content-type","application/json");headers.set("authorization","Bearer "+token);
    const response=await fetch(apiUrl.replace(/\/$/,"")+path,{...init,headers});
    if(!response.ok) throw new Error("Team Room HTTP "+response.status);
    if(response.status===204)return null;
    return response.json();
  },[apiUrl,token]);

  const refresh=useCallback(async()=>{
    if(!roomId||!token)return;
    const [history,presence,taskRows,decisionRows,attachmentRows]=await Promise.all([
      request(privateMode&&privatePrincipal?"/team-room/"+roomId+"/private/"+privatePrincipal+"/messages":"/team-room/"+roomId+"/messages"),
      request("/team-room/"+roomId+"/presence"),
      request("/team-room/"+roomId+"/tasks"),
      request("/team-room/"+roomId+"/decisions"),
      request("/team-room/"+roomId+"/attachments")
    ]);
    setMessages(history.items||[]);setUnreadCount(history.unread_count||0);setChannelId(history.channel_id||"");
    setParticipants(presence||[]);setTasks(taskRows||[]);setDecisions(decisionRows||[]);setAttachments(attachmentRows||[]);
  },[request,roomId,token,privateMode,privatePrincipal]);

  const connect=useCallback(async()=>{
    setError("");setBusy(true);
    try{
      await request("/team-room/me");
      await request("/team-room/"+roomId+"/join",{method:"POST",body:"{}"});
      await refresh();setConnected(true);setConnection("CONNECTING");
    }catch(e){setError(e instanceof Error?e.message:"Connection failed");setConnected(false);}
    finally{setBusy(false);}
  },[request,roomId,refresh]);

  useEffect(()=>{
    if(!connected||!roomId||!token)return;
    let stopped=false;let retry=0;let timer:number|undefined;
    const open=async()=>{
      try{
        const issued=await request("/team-room/"+roomId+"/ws-ticket",{method:"POST",body:"{}"});
        if(stopped)return;
        const wsUrl=apiUrl.replace(/^http/,"ws").replace(/\/$/,"")+"/team-room/"+roomId+"/ws?ticket="+encodeURIComponent(issued.ticket);
        const socket=new WebSocket(wsUrl,["team-room.v1"]);socketRef.current=socket;
        socket.onopen=()=>{retry=0;setConnection("CONNECTED");void refresh().catch(()=>{});};
        socket.onmessage=event=>{try{const data=JSON.parse(event.data);if(["MESSAGE_CREATED","TASK_CREATED","TASK_UPDATED","PRESENCE_CHANGED","DECISION_CREATED","ATTACHMENT_CREATED","PARTICIPANT_JOINED","PARTICIPANT_LEFT"].includes(data.type))void refresh().catch(()=>{});}catch{/* Ignore malformed event frames. */}};
        socket.onerror=()=>setConnection("RECONNECTING");
        socket.onclose=()=>{socketRef.current=null;if(!stopped){setConnection("RECONNECTING");const delay=Math.min(30000,500*Math.pow(2,retry++));timer=window.setTimeout(()=>void open(),delay);}};
      }catch{if(!stopped){setConnection("RECONNECTING");timer=window.setTimeout(()=>void open(),Math.min(30000,1000*Math.pow(2,retry++)));}}
    };
    void open();
    return()=>{stopped=true;if(timer)window.clearTimeout(timer);socketRef.current?.close();socketRef.current=null;};
  },[connected,apiUrl,roomId,token,request,refresh]);

  async function sendMessage(){
    if(!draft.trim())return;
    setError("");
    try{
      const path=privateMode?"/team-room/"+roomId+"/private/"+privatePrincipal+"/messages":"/team-room/"+roomId+"/messages";
      await request(path,{method:"POST",body:JSON.stringify({client_message_id:crypto.randomUUID(),body:draft.trim(),parent_message_id:replyTo})});
      setDraft("");setReplyTo(null);await refresh();
    }catch(e){setError(e instanceof Error?e.message:"Message failed");}
  }
  async function createTask(){
    try{await request("/team-room/tasks",{method:"POST",body:JSON.stringify({mission_id:missionId,title:taskTitle,checkpoint:{description:taskTitle}})});setTaskTitle("");await refresh();}
    catch(e){setError(e instanceof Error?e.message:"Task failed");}
  }
  async function createDecision(){
    try{await request("/team-room/"+roomId+"/decisions",{method:"POST",body:JSON.stringify({title:decisionTitle,decision:decisionBody,participants:[],mission_id:null,reference_message_id:null})});setDecisionTitle("");setDecisionBody("");await refresh();}
    catch(e){setError(e instanceof Error?e.message:"Decision failed");}
  }
  async function createAttachmentRef(){
    try{if(!channelId)throw new Error("Load the general channel first");await request("/team-room/"+roomId+"/attachments",{method:"POST",body:JSON.stringify({channel_id:channelId,file_name:attachmentName,storage_ref:attachmentRef})});setAttachmentName("");setAttachmentRef("");await refresh();}
    catch(e){setError(e instanceof Error?e.message:"Attachment reference failed");}
  }
  async function changePresence(value:string){
    setPresenceState(value);
    try{await request("/team-room/"+roomId+"/presence",{method:"PUT",body:JSON.stringify({state:value})});}
    catch(e){setError(e instanceof Error?e.message:"Presence update failed");}
  }
  async function openPrivate(){
    try{const result=await request("/team-room/"+roomId+"/private/"+privatePrincipal+"/messages");setMessages(result.items||[]);setChannelId(result.channel_id);setPrivateMode(true);}
    catch(e){setError(e instanceof Error?e.message:"Private chat unavailable");}
  }

  return <section className="team-room">
    <div className="team-room-toolbar">
      <label>API URL<input value={apiUrl} onChange={e=>setApiUrl(e.target.value)} /></label>
      <label>Room ID<input value={roomId} onChange={e=>setRoomId(e.target.value)} placeholder="UUID da sala" /></label>
      <label>Session token<input type="password" value={token} onChange={e=>setToken(e.target.value)} placeholder="Bearer token (memória da tela)" /></label>
      <button className="primary" disabled={busy||!roomId||!token} onClick={()=>void connect()}>{busy?"Conectando...":"Entrar na sala"}</button>
      <span className="status">{connection}</span>{connected&&<button onClick={()=>{setConnected(false);setConnection("DISCONNECTED");}}>Sair da sala</button>}
    </div>
    {error&&<p className="error">{error}</p>}
    <div className="team-room-layout">
      <aside className="team-room-nav">
        <strong>AI TEAM ROOM</strong><a href="#room-chat">Sala geral</a><a href="#room-tasks">Tarefas</a><a href="#room-private">Privados</a><a href="#room-audio">Áudio</a><a href="#room-decisions">Decisões</a><a href="#room-files">Arquivos</a><a href="#room-people">Participantes</a>
        <label>Meu status<select value={presenceState} onChange={e=>void changePresence(e.target.value)}><option>ONLINE</option><option>AWAY</option><option>BUSY</option><option>WAITING</option><option>BLOCKED</option></select></label>
      </aside>
      <div className="team-room-main">
        <section className="panel" id="room-chat"><div className="panel-head"><div><h2>Chat {unreadCount>0?"· "+unreadCount+" não lidas":""}</h2><p className="muted">Histórico persistente · eventos via PostgreSQL outbox + WebSocket autenticado</p></div><button onClick={()=>void refresh()}>Atualizar</button></div>
          <div className="team-room-messages">{messages.map(m=><article key={m.message_id}><small>{m.sender_principal_id.slice(0,8)} · {new Date(m.created_at).toLocaleString()}</small><p>{m.body}</p><button onClick={()=>setReplyTo(m.message_id)}>Responder nesta thread</button></article>)}</div>\n          {replyTo&&<p className="muted">Respondendo à mensagem {replyTo.slice(0,8)} <button onClick={()=>setReplyTo(null)}>Cancelar</button></p>}
          <textarea value={draft} onChange={e=>setDraft(e.target.value)} rows={3} placeholder="Escreva uma mensagem..."/>
          <button className="primary" onClick={()=>void sendMessage()} disabled={!connected||!draft.trim()}>Enviar mensagem</button>
        </section>
        <section className="panel" id="room-private"><h2>Chat privado</h2><div className="team-room-inline"><input value={privatePrincipal} onChange={e=>setPrivatePrincipal(e.target.value)} placeholder="Principal UUID do participante"/><button onClick={()=>void openPrivate()} disabled={!connected||!privatePrincipal}>Abrir conversa A ↔ B</button><button onClick={()=>{setPrivateMode(false);void refresh();}}>Voltar à sala geral</button></div><p className="muted">{privateMode?"Visualizando canal privado; o backend aplica ACL por membro.":"Escolha um participante para abrir uma conversa privada."}</p></section>
        <section className="panel" id="room-tasks"><h2>Tarefas</h2><div className="team-room-inline"><input value={missionId} onChange={e=>setMissionId(e.target.value)} placeholder="Mission UUID"/><input value={taskTitle} onChange={e=>setTaskTitle(e.target.value)} placeholder="Descrição da tarefa"/><button onClick={()=>void createTask()} disabled={!connected||!missionId||!taskTitle}>Criar tarefa</button></div>{tasks.map(t=><div className="file-row" key={t.task_id}><strong>{t.status}</strong><span>{t.task_id.slice(0,8)} · mission {t.mission_id.slice(0,8)} · fence {t.fencing_token}</span></div>)}</section>
        <section className="panel" id="room-decisions"><h2>Decisões</h2><input value={decisionTitle} onChange={e=>setDecisionTitle(e.target.value)} placeholder="Título"/><textarea value={decisionBody} onChange={e=>setDecisionBody(e.target.value)} rows={3} placeholder="Decisão registrada"/><button onClick={()=>void createDecision()} disabled={!connected||!decisionTitle||!decisionBody}>Registrar decisão</button>{decisions.map(d=><div className="file-row" key={d.decision_id}><strong>{d.title}</strong><span>{d.author_principal_id.slice(0,8)} · {new Date(d.created_at).toLocaleString()}</span><p>{d.decision}</p></div>)}</section>
        <section className="panel" id="room-files"><h2>Arquivos e referências</h2><div className="team-room-inline"><input value={attachmentName} onChange={e=>setAttachmentName(e.target.value)} placeholder="Nome do arquivo"/><input value={attachmentRef} onChange={e=>setAttachmentRef(e.target.value)} placeholder="Storage reference"/><button onClick={()=>void createAttachmentRef()} disabled={!connected||!attachmentName||!attachmentRef}>Vincular referência</button></div><p className="muted">Registro de referência apenas. Upload binário/armazenamento de produção ainda não está habilitado.</p>{attachments.map(a=><div className="file-row" key={a.attachment_id}><strong>{a.file_name}</strong><span>{a.storage_ref}</span></div>)}</section>
        <section className="panel" id="room-audio"><h2>Áudio da sala</h2><p className="muted">WEBRTC_PRODUCTION = PENDENTE. Nenhuma mídia é transmitida ou gravada.</p><div className="team-room-inline"><button disabled>Entrar no áudio</button><button disabled>Mute / Unmute</button><button disabled>Sair do áudio</button></div><small>Signaling, SFU, grants e consentimento ainda não implementados.</small></section>
        <section className="panel" id="room-people"><h2>Participantes</h2>{participants.map(p=><div className="file-row" key={p.participant_id}><strong>{p.display_name} · {p.provider}</strong><span>{p.connected?"●":"○"} {p.state} · {p.current_task_id?"executando "+p.current_task_id.slice(0,8):"sem tarefa"}</span></div>)}</section>
      </div>
    </div>
  </section>;
}
