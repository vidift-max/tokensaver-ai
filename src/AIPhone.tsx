import { useCallback, useEffect, useState } from "react";

type PhoneSnapshot = {
  phone: { ai_phone_id: string; ai_id: string; phone_status: string; endpoint_fence: number };
  attachment: { endpoint_id: string; endpoint_fence: number; attachment_status: string } | null;
  mission: unknown | null;
};
type PhoneMessage = {
  message_id: string;
  ai_phone_id: string;
  sender_principal_id: string;
  sequence_no: number;
  message_state: string;
  content: Record<string, unknown>;
  created_at: string;
  delivered_at: string | null;
  acknowledged_at: string | null;
};
type AuthIdentity = { principal_id: string };
type PhonePresence = { ai_phone_id: string; presence_state: string };
type Attachment = { endpoint_id: string; endpoint_fence: number; attachment_status: string };

type Props = { apiUrl: string; token: string };

function endpointFor(phoneId: string): string {
  const key = "tokensaver.ai-phone.endpoint." + phoneId;
  const saved = window.localStorage.getItem(key);
  if (saved) return saved;
  const created = crypto.randomUUID();
  window.localStorage.setItem(key, created);
  return created;
}

export default function AIPhone({ apiUrl, token }: Props) {
  const [snapshot, setSnapshot] = useState<PhoneSnapshot | null>(null);
  const [principalId, setPrincipalId] = useState("");
  const [endpointId, setEndpointId] = useState("");
  const [endpointFence, setEndpointFence] = useState<number | null>(null);
  const [presence, setPresence] = useState("UNKNOWN");
  const [attached, setAttached] = useState(false);
  const [messages, setMessages] = useState<PhoneMessage[]>([]);
  const [targetPhoneId, setTargetPhoneId] = useState("");
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const request = useCallback(async <T,>(path: string, init: RequestInit = {}): Promise<T> => {
    const headers = new Headers(init.headers);
    headers.set("authorization", "Bearer " + token);
    if (init.body) headers.set("content-type", "application/json");
    const response = await fetch(apiUrl.replace(/\/$/, "") + path, { ...init, headers });
    if (!response.ok) throw new Error("HTTP " + response.status);
    if (response.status === 204) return null as T;
    return await response.json() as T;
  }, [apiUrl, token]);

  const refresh = useCallback(async () => {
    if (!token) return;
    setError("");
    const identity = await request<AuthIdentity>("/auth/me");
    setPrincipalId(identity.principal_id);
    try {
      const current = await request<PhoneSnapshot>("/ai-phone/me");
      setSnapshot(current);
      setTargetPhoneId(value => value || current.phone.ai_phone_id);
      const currentEndpoint = endpointFor(current.phone.ai_phone_id);
      setEndpointId(currentEndpoint);
      const currentPresence = await request<PhonePresence>("/ai-phone/" + current.phone.ai_phone_id + "/presence");
      setPresence(currentPresence.presence_state);
      const inboxPromise = current.attachment?.attachment_status === "ACTIVE"
        ? request<PhoneMessage[]>("/ai-phone/" + current.phone.ai_phone_id + "/messages")
        : Promise.resolve([] as PhoneMessage[]);
      const sentPromise = request<PhoneMessage[]>("/ai-phone/sent").catch(() => [] as PhoneMessage[]);
      const [inbox, sent] = await Promise.all([inboxPromise, sentPromise]);
      const unique = new Map<string, PhoneMessage>();
      for (const message of [...inbox, ...sent]) unique.set(message.message_id, message);
      setMessages([...unique.values()].sort((a, b) => a.created_at.localeCompare(b.created_at)));
      const active = current.attachment?.attachment_status === "ACTIVE";
      setAttached(Boolean(active));
      setEndpointFence(active ? current.attachment!.endpoint_fence : null);
    } catch (e) {
      if (e instanceof Error && e.message === "HTTP 404") {
        setSnapshot(null);
        setPresence("NOT_PROVISIONED");
        setAttached(false);
        setEndpointFence(null);
        setMessages([]);
        setError("");
      } else {
        throw e;
      }
    }
  }, [request, token]);

  useEffect(() => {
    if (!token) {
      setSnapshot(null);
      setMessages([]);
      setPresence("DISCONNECTED");
      setAttached(false);
      return;
    }
    void refresh().catch(e => setError(e instanceof Error ? e.message : "Falha ao carregar telefone"));
  }, [token, refresh]);

  useEffect(() => {
    if (!attached || endpointFence === null || !token) return;
    const heartbeatTimer = window.setInterval(() => {
      void request("/ai-phone/heartbeat", {
        method: "POST",
        body: JSON.stringify({ endpoint_fence: endpointFence })
      }).catch(() => setPresence("RECONNECTING"));
    }, 15000);
    const pollTimer = window.setInterval(() => {
      void refresh().catch(e => setError(e instanceof Error ? e.message : "Falha ao atualizar mensagens"));
    }, 5000);
    return () => {
      window.clearInterval(heartbeatTimer);
      window.clearInterval(pollTimer);
    };
  }, [attached, endpointFence, request, refresh, token]);

  async function provision() {
    if (!token) return;
    setBusy(true); setError(""); setNotice("");
    try {
      const identity = await request<AuthIdentity>("/auth/me");
      setPrincipalId(identity.principal_id);
      await request<PhoneSnapshot>("/ai-phone/provision", {
        method: "POST",
        body: JSON.stringify({ principal_id: identity.principal_id })
      });
      setNotice("Telefone provisionado. A identidade AI_ID foi preservada.");
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Não foi possível provisionar o telefone. Verifique a associação AI_ID e a permissão phone.provision.");
    } finally {
      setBusy(false);
    }
  }

  async function connect(reconnect = false) {
    if (!snapshot || !token) return;
    setBusy(true); setError(""); setNotice("");
    try {
      const id = endpointId || endpointFor(snapshot.phone.ai_phone_id);
      setEndpointId(id);
      const route = reconnect ? "/ai-phone/reconnect" : "/ai-phone/attach";
      const attachment = await request<Attachment>(route, {
        method: "POST",
        body: JSON.stringify({ endpoint_id: id, endpoint_type: "WEB", metadata: { label: "TokenSaver AI Web" } })
      });
      setEndpointFence(attachment.endpoint_fence);
      setAttached(true);
      setPresence("CONNECTED");
      setNotice(reconnect ? "Sessão reconectada; identidade e histórico preservados." : "Telefone conectado.");
      await refresh();
    } catch (e) {
      setPresence("RECONNECTING");
      setError(e instanceof Error ? e.message : "Não foi possível conectar o telefone");
    } finally {
      setBusy(false);
    }
  }

  async function disconnect() {
    if (endpointFence === null) return;
    setBusy(true); setError("");
    try {
      await request("/ai-phone/detach", {
        method: "POST",
        body: JSON.stringify({ endpoint_fence: endpointFence })
      });
      setAttached(false);
      setEndpointFence(null);
      setPresence("DISCONNECTED");
      setNotice("Telefone desconectado. Mensagens e histórico continuam persistidos.");
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Não foi possível desconectar");
    } finally {
      setBusy(false);
    }
  }

  async function sendMessage() {
    const body = draft.trim();
    const destination = targetPhoneId.trim();
    if (!body || !destination) return;
    setBusy(true); setError(""); setNotice("");
    try {
      await request<PhoneMessage>("/ai-phone/" + encodeURIComponent(destination) + "/messages", {
        method: "POST",
        body: JSON.stringify({
          idempotency_key: crypto.randomUUID(),
          content: { text: body }
        })
      });
      setDraft("");
      setNotice("Mensagem armazenada no telefone de destino.");
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Falha ao enviar mensagem. O destino precisa pertencer ao mesmo tenant.");
    } finally {
      setBusy(false);
    }
  }

  async function acknowledge(messageId: string) {
    if (!snapshot) return;
    try {
      await request("/ai-phone/" + snapshot.phone.ai_phone_id + "/messages/" + messageId + "/ack", { method: "POST" });
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Falha ao confirmar recebimento");
    }
  }

  const phone = snapshot?.phone;
  return <section className="ai-phone">
    <div className="ai-phone-heading">
      <div>
        <h2>Telefone virtual da IA</h2>
        <p className="muted">Identidade permanente · mensagens duráveis · conexão recuperável · sem PSTN ou chamadas de voz nesta etapa.</p>
      </div>
      <span className={"ai-phone-status " + (attached ? "is-connected" : "")}>{presence}</span>
    </div>
    {!token ? <p className="muted">Informe e conecte uma sessão válida do Team Room para acessar o telefone.</p> : !phone ? (
      <div className="ai-phone-empty">
        <p>Esta identidade ainda não possui telefone provisionado. A consulta é somente leitura; nenhum telefone será criado automaticamente.</p>
        <button className="primary" onClick={() => void provision()} disabled={busy}>Provisionar meu telefone</button>
        <small>Exige associação AI_ID já existente e permissão explícita <code>phone.provision</code>.</small>
      </div>
    ) : <>
      <div className="ai-phone-identity">
        <div><small>AI_ID</small><strong>{phone.ai_id}</strong></div>
        <div><small>AI_PHONE_ID</small><strong>{phone.ai_phone_id}</strong></div>
        <div><small>Fencing</small><strong>{endpointFence ?? phone.endpoint_fence}</strong></div>
      </div>
      <div className="ai-phone-actions">
        <button className="primary" onClick={() => void connect(false)} disabled={busy || attached}>Conectar</button>
        <button onClick={() => void connect(true)} disabled={busy}>Reconectar</button>
        <button onClick={() => void disconnect()} disabled={busy || !attached}>Desconectar</button>
        <button onClick={() => void refresh()} disabled={busy}>Atualizar</button>
      </div>
      <div className="ai-phone-compose">
        <label>Telefone de destino (mesmo tenant)<input value={targetPhoneId} onChange={e => setTargetPhoneId(e.target.value)} placeholder="AI_PHONE_ID de destino" /></label>
        <label>Mensagem<textarea value={draft} onChange={e => setDraft(e.target.value)} rows={3} placeholder="Escreva uma mensagem..." /></label>
        <button className="primary" onClick={() => void sendMessage()} disabled={busy || !draft.trim() || !targetPhoneId.trim()}>Enviar mensagem</button>
        <small>O envio é persistido mesmo quando o telefone de destino está offline. A entrega ocorre quando ele reconectar.</small>
      </div>
      <div className="ai-phone-history">
        <div className="panel-head"><div><h3>Histórico</h3><p className="muted">{messages.length} mensagens · entrada e saída · armazenadas no PostgreSQL</p></div><button onClick={() => void refresh()} disabled={busy}>Sincronizar</button></div>
        {messages.length === 0 ? <p className="muted">Nenhuma mensagem no histórico.</p> : messages.map(message => {
          const outgoing = message.sender_principal_id === principalId;
          const text = typeof message.content?.text === "string" ? message.content.text : JSON.stringify(message.content);
          return <article className={"ai-phone-message " + (outgoing ? "outgoing" : "incoming")} key={message.message_id}>
            <div className="ai-phone-message-meta"><strong>{outgoing ? "Enviada" : "Recebida"}</strong><span>{new Date(message.created_at).toLocaleString()} · #{message.sequence_no}</span></div>
            <p>{text}</p>
            <div className="ai-phone-message-meta"><span>{message.message_state}</span>{!outgoing && message.message_state === "DELIVERED" && attached && <button onClick={() => void acknowledge(message.message_id)}>Confirmar recebimento</button>}</div>
          </article>;
        })}
      </div>
    </>}
    {notice && <p className="ai-phone-notice">{notice}</p>}
    {error && <p className="error">{error}</p>}
    {phone && <small className="muted">Sessão principal: {principalId || "não identificada"} · endpoint: {endpointId || "ainda não registrado"}</small>}
  </section>;
}
