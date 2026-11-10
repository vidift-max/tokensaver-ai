import { useEffect, useState } from "react";
import { api } from "./api";

const modules = [
  {name:"Dashboard",ready:true},{name:"File Center",ready:true},{name:"Token Counter",ready:true},
  {name:"Smart Context",ready:true},{name:"Safe Optimizer",ready:true},{name:"Project Memory",ready:true},
  {name:"Claude Connection",ready:true},{name:"Template Master",ready:true},{name:"Editorial Engine",ready:true},
  {name:"Article Library",ready:true},{name:"Internal Links",ready:true},{name:"Final Page Builder",ready:true},
  {name:"OneShot Capture",ready:true},{name:"Cost Simulator",ready:true},{name:"Integrity Guardian",ready:true},{name:"Backup Center",ready:true},
  {name:"Settings",ready:true},{name:"History",ready:true}
];

type FileRow = { id:string; logical_path:string; latest_version:number; byte_len:number; sha256:string };

export default function App() {
  const [active, setActive] = useState("Dashboard");
  const [backend, setBackend] = useState("checking");
  const [projectId, setProjectId] = useState("default");
  const [files, setFiles] = useState<FileRow[]>([]);
  const [loadingFiles, setLoadingFiles] = useState(false);
  const [error, setError] = useState("");
  const [importRoots, setImportRoots] = useState<{path:string}[]>([]);
  const [rootInput, setRootInput] = useState("");
  const [rootBusy, setRootBusy] = useState(false);
  const [integrity, setIntegrity] = useState<Awaited<ReturnType<typeof api.verifyProject>> | null>(null);
  const [integrityBusy, setIntegrityBusy] = useState(false);
  const [backupBusy, setBackupBusy] = useState(false);
  const [backupResult, setBackupResult] = useState<Awaited<ReturnType<typeof api.createProjectBackup>> | null>(null);
  const [restoreBackupId, setRestoreBackupId] = useState("");
  const [restoreTargetId, setRestoreTargetId] = useState("");
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [restoreResult, setRestoreResult] = useState<Awaited<ReturnType<typeof api.restoreProjectBackup>> | null>(null);
  const [contextQuery, setContextQuery] = useState("");
  const [contextHits, setContextHits] = useState<Awaited<ReturnType<typeof api.smartContext>>>([]);
  const [contextBusy, setContextBusy] = useState(false);
  const [optimizerInput, setOptimizerInput] = useState("");
  const [optimizerResult, setOptimizerResult] = useState<Awaited<ReturnType<typeof api.optimizeText>> | null>(null);
  const [memory, setMemory] = useState<Awaited<ReturnType<typeof api.listProjectMemory>>>([]);
  const [memoryKey, setMemoryKey] = useState(""); const [memoryValue, setMemoryValue] = useState("");
  const [history, setHistory] = useState<Awaited<ReturnType<typeof api.listOperations>>>([]);
  const [costInput,setCostInput]=useState("10000"), [costOutput,setCostOutput]=useState("2000"), [costInPrice,setCostInPrice]=useState("3"), [costOutPrice,setCostOutPrice]=useState("15");
  const [costResult,setCostResult]=useState<Awaited<ReturnType<typeof api.simulateCost>>|null>(null);
  const [templates,setTemplates]=useState<Awaited<ReturnType<typeof api.listTemplates>>>([]), [templateName,setTemplateName]=useState(""), [templateContent,setTemplateContent]=useState("<article><h1>{{title}}</h1><div>{{body}}</div></article>");
  const [editorialTitle,setEditorialTitle]=useState(""), [editorialBody,setEditorialBody]=useState(""), [editorial,setEditorial]=useState<Awaited<ReturnType<typeof api.editorialValidate>>|null>(null);
  const [articleQuery,setArticleQuery]=useState(""), [articles,setArticles]=useState<Awaited<ReturnType<typeof api.listArticles>>>([]);
  const [linkQuery,setLinkQuery]=useState(""), [links,setLinks]=useState<Awaited<ReturnType<typeof api.findInternalLinks>>>([]);
  const [pageTitle,setPageTitle]=useState(""), [pageBody,setPageBody]=useState(""), [pageTemplate,setPageTemplate]=useState(""), [pageHtml,setPageHtml]=useState("");
  const [capturePath,setCapturePath]=useState("captured.md"), [captureContent,setCaptureContent]=useState("");
  const [claude,setClaude]=useState<Awaited<ReturnType<typeof api.getClaudeConfig>>|null>(null), [claudeEndpoint,setClaudeEndpoint]=useState(""), [claudeModel,setClaudeModel]=useState("claude-3-5-sonnet"), [claudeEnabled,setClaudeEnabled]=useState(false);

  function errorMessage(error: unknown, fallback: string) {
    const normalized = api.normalizeError(error);
    return normalized.code === "INTERNAL_ERROR" ? fallback : `[${normalized.code}] ${normalized.message}`;
  }

  useEffect(() => {
    api.health().then(() => setBackend("online")).catch(() => setBackend("offline"));
    loadImportRoots();
  }, []);

  async function loadImportRoots() {
    try {
      const roots = await api.listImportRoots();
      setImportRoots(roots);
    } catch (e) {
      setError(errorMessage(e, "Não foi possível carregar as áreas de importação."));
    }
  }

  async function addImportRoot() {
    const path = rootInput.trim();
    if (!path) return;
    setRootBusy(true);
    setError("");
    try {
      const roots = await api.addImportRoot(path);
      setImportRoots(roots);
      setRootInput("");
    } catch (e) {
      setError(errorMessage(e, "Não foi possível autorizar a área de importação."));
    } finally {
      setRootBusy(false);
    }
  }

  async function removeImportRoot(path: string) {
    setRootBusy(true);
    setError("");
    try {
      const roots = await api.removeImportRoot(path);
      setImportRoots(roots);
    } catch (e) {
      setError(errorMessage(e, "Não foi possível remover a área de importação."));
    } finally {
      setRootBusy(false);
    }
  }

  async function verifyIntegrity() {
    setIntegrityBusy(true);
    setError("");
    try {
      await api.initializeProject(projectId);
      setIntegrity(await api.verifyProject(projectId));
    } catch (e) {
      setError(errorMessage(e, "Não foi possível verificar a integridade do projeto."));
    } finally {
      setIntegrityBusy(false);
    }
  }

  async function restoreBackup() {
    if (!restoreBackupId.trim() || !restoreTargetId.trim()) return;
    if (!window.confirm(`Restaurar o backup para um NOVO projeto chamado "${restoreTargetId}"? O projeto original não será substituído.`)) return;
    setRestoreBusy(true);
    setError("");
    setRestoreResult(null);
    try {
      setRestoreResult(await api.restoreProjectBackup(projectId, restoreBackupId.trim(), restoreTargetId.trim()));
    } catch (e) {
      setError(errorMessage(e, "Não foi possível restaurar o backup."));
    } finally {
      setRestoreBusy(false);
    }
  }

  async function createBackup() {
    setBackupBusy(true);
    setError("");
    setBackupResult(null);
    try {
      await api.initializeProject(projectId);
      const created = await api.createProjectBackup(projectId);
      setBackupResult(created);
      setRestoreBackupId(created.backup_id);
      setRestoreTargetId(`${projectId}-restored`);
    } catch (e) {
      setError(errorMessage(e, "Não foi possível criar um backup verificado."));
    } finally {
      setBackupBusy(false);
    }
  }

  async function runSmartContext() {
    if (!contextQuery.trim()) return;
    setContextBusy(true); setError("");
    try { await api.initializeProject(projectId); setContextHits(await api.smartContext(projectId, contextQuery, 8)); }
    catch (e) { setError(errorMessage(e, "Não foi possível montar o contexto.")); }
    finally { setContextBusy(false); }
  }

  async function runOptimizer() {
    setError("");
    try { setOptimizerResult(await api.optimizeText(optimizerInput)); } catch (e) { setError(errorMessage(e, "Não foi possível otimizar o texto.")); }
  }

  async function loadMemory() { try { await api.initializeProject(projectId); setMemory(await api.listProjectMemory(projectId)); } catch(e){setError(errorMessage(e,"Não foi possível carregar a memória."));} }
  async function saveMemory(){ if(!memoryKey.trim()) return; try { await api.initializeProject(projectId); await api.setProjectMemory(projectId,memoryKey.trim(),memoryValue); setMemory(await api.listProjectMemory(projectId)); setMemoryKey(""); setMemoryValue(""); } catch(e){setError(errorMessage(e,"Não foi possível salvar a memória."));} }
  async function loadTemplates(){try{await api.initializeProject(projectId);setTemplates(await api.listTemplates(projectId));}catch(e){setError(errorMessage(e,"Não foi possível carregar templates."));}}
  async function saveTemplate(){try{await api.initializeProject(projectId);await api.saveTemplate(projectId,null,templateName,templateContent);setTemplateName("");setTemplates(await api.listTemplates(projectId));}catch(e){setError(errorMessage(e,"Não foi possível salvar o template."));}}
  async function runCost(){try{setCostResult(await api.simulateCost(Number(costInput)||0,Number(costOutput)||0,Number(costInPrice)||0,Number(costOutPrice)||0));}catch(e){setError(errorMessage(e,"Não foi possível calcular o custo."));}}
  async function runEditorial(){try{setEditorial(await api.editorialValidate(editorialTitle,editorialBody));}catch(e){setError(errorMessage(e,"Não foi possível validar o editorial."));}}
  async function loadArticles(){try{await api.initializeProject(projectId);setArticles(await api.listArticles(projectId,articleQuery));}catch(e){setError(errorMessage(e,"Não foi possível carregar a biblioteca."));}}
  async function runLinks(){try{await api.initializeProject(projectId);setLinks(await api.findInternalLinks(projectId,linkQuery,20));}catch(e){setError(errorMessage(e,"Não foi possível encontrar links."));}}
  async function runPage(){try{setPageHtml(await api.buildPage(pageTitle,pageBody,pageTemplate));}catch(e){setError(errorMessage(e,"Não foi possível montar a página."));}}
  async function runCapture(){try{await api.initializeProject(projectId);await api.oneshotCapture(projectId,capturePath,captureContent);setError("");}catch(e){setError(errorMessage(e,"Não foi possível capturar o conteúdo."));}}
  async function loadClaude(){try{await api.initializeProject(projectId);const x=await api.getClaudeConfig(projectId);setClaude(x);setClaudeEnabled(x.enabled);setClaudeEndpoint(x.endpoint);setClaudeModel(x.model);}catch(e){setError(errorMessage(e,"Não foi possível carregar a configuração Claude."));}}
  async function saveClaude(){try{await api.initializeProject(projectId);setClaude(await api.saveClaudeConfig(projectId,claudeEnabled,claudeEndpoint,claudeModel));}catch(e){setError(errorMessage(e,"Não foi possível salvar a configuração Claude."));}}
  async function loadHistory(){ try { await api.initializeProject(projectId); setHistory(await api.listOperations(projectId,50)); } catch(e){setError(errorMessage(e,"Não foi possível carregar o histórico."));} }

  async function loadFiles() {
    setLoadingFiles(true);
    setError("");
    try {
      await api.initializeProject(projectId);
      const page = await api.listProjectFiles(projectId, 0, 50);
      setFiles(page.items);
    } catch (e) {
      setError(errorMessage(e, "Não foi possível carregar os arquivos."));
    } finally {
      setLoadingFiles(false);
    }
  }

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand"><span>TS</span><div><strong>TokenSaver</strong><small>AI Foundation</small></div></div>
        <nav>{modules.map(m => <button className={active===m.name ? "active" : ""} onClick={() => m.ready && setActive(m.name)} disabled={!m.ready} key={m.name}>{m.name}{!m.ready && " · futuro"}</button>)}</nav>
      </aside>
      <section className="content">
        <header>
          <div><p className="eyebrow">FOUNDATION / {active.toUpperCase()}</p><h1>{active}</h1><p className="muted">Base segura para economia de tokens, contexto determinístico e operações rastreáveis.</p></div>
          <div className="status">{backend === "online" ? "● Backend online" : backend === "offline" ? "● Backend offline" : "● Verificando"}</div>
        </header>

        <div className="cards">
          <article><b>Projeto</b><strong>TokenSaver AI</strong><span>Tauri 2 + React + TypeScript + Rust + SQLite.</span></article>
          <article><b>Integridade</b><strong>Original imutável</strong><span>Versões usam SHA-256 e armazenamento por conteúdo.</span></article>
          <article><b>Tokens</b><strong>Estimativa explícita</strong><span>Método, modelo e limitações acompanham cada contagem.</span></article>
        </div>

        {active === "Project Memory" ? (
          <section className="panel"><div className="panel-head"><div><h2>Project Memory</h2><p className="muted">Notas persistentes do projeto, armazenadas localmente.</p></div><button className="primary" onClick={() => void loadMemory()}>Atualizar</button></div><div className="project-row"><input value={projectId} onChange={e=>setProjectId(e.target.value)} placeholder="Projeto"/><input value={memoryKey} onChange={e=>setMemoryKey(e.target.value)} placeholder="chave"/><input value={memoryValue} onChange={e=>setMemoryValue(e.target.value)} placeholder="valor"/><button onClick={()=>void saveMemory()}>Salvar</button></div>{memory.map(m=><div className="file-row" key={m.key}><strong>{m.key}</strong><span>{m.value}</span></div>)}</section>
        ) : active === "History" ? (
          <section className="panel"><div className="panel-head"><div><h2>History</h2><p className="muted">Operações registradas pelo núcleo do projeto.</p></div><button className="primary" onClick={()=>void loadHistory()}>Atualizar</button></div>{history.length===0?<p className="muted">Nenhuma operação registrada.</p>:history.map(h=><div className="file-row" key={h.id}><strong>{h.kind}</strong><span>{h.status} · {h.progress}% · {h.created_at}</span></div>)}</section>
        ) : active === "Safe Optimizer" ? (
          <section className="panel">
            <div className="panel-head"><div><h2>Safe Optimizer</h2><p className="muted">Normalização conservadora: remove espaços no fim das linhas e linhas vazias redundantes, sem reescrever o conteúdo.</p></div><button className="primary" onClick={() => void runOptimizer()} disabled={!optimizerInput}>Otimizar</button></div>
            <textarea value={optimizerInput} onChange={e=>setOptimizerInput(e.target.value)} rows={14} placeholder="Cole aqui o conteúdo que deseja otimizar..." />
            {error && <p className="error">{error}</p>}
            {optimizerResult && <div className="cards"><article><b>Antes</b><strong>~{optimizerResult.estimated_tokens_before} tokens</strong><span>{optimizerResult.original_chars} caracteres</span></article><article><b>Depois</b><strong>~{optimizerResult.estimated_tokens_after} tokens</strong><span>{optimizerResult.optimized_chars} caracteres</span></article><article><b>Economia</b><strong>{optimizerResult.saved_chars} caracteres</strong><span>{optimizerResult.changed ? "Conteúdo normalizado" : "Nenhuma alteração"}</span></article></div>}
            {optimizerResult && <textarea value={optimizerResult.content} readOnly rows={14} />}
          </section>
        ) : active === "Smart Context" ? (
          <section className="panel">
            <div className="panel-head"><div><h2>Smart Context</h2><p className="muted">Busca local e determinística nos arquivos importados. O conteúdo não é enviado para APIs externas.</p></div><button className="primary" onClick={() => void runSmartContext()} disabled={contextBusy || !contextQuery.trim()}>{contextBusy ? "Montando..." : "Montar contexto"}</button></div>
            <div className="project-row"><label>Projeto <input value={projectId} onChange={e=>setProjectId(e.target.value)} maxLength={64}/></label><label style={{flex:1}}>Consulta <input value={contextQuery} onChange={e=>setContextQuery(e.target.value)} onKeyDown={e=>{if(e.key==="Enter") void runSmartContext();}} placeholder="ex.: title, schema, produto" /></label></div>
            {error && <p className="error">{error}</p>}
            {contextHits.length===0 ? <p className="muted">Nenhum arquivo relevante encontrado.</p> : <div className="file-list">{contextHits.map(h=><div className="file-row" key={h.file_id}><strong>{h.logical_path}</strong><span>score {h.score} · v{h.version_no} · ~{h.token_estimate} tokens</span><p className="muted" style={{margin:"6px 0 0"}}>{h.excerpt}</p></div>)}</div>}
          </section>
        ) : active === "File Center" ? (
          <section className="panel">
            <div className="panel-head"><div><h2>Arquivos do projeto</h2><p className="muted">Índice paginado do armazenamento local.</p></div><button className="primary" onClick={loadFiles} disabled={loadingFiles}>{loadingFiles ? "Carregando..." : "Atualizar"}</button></div>
            <div className="project-row"><label>Projeto <input value={projectId} onChange={e=>setProjectId(e.target.value)} maxLength={64}/></label></div>
            {error && <p className="error">{error}</p>}
            {files.length === 0 ? <p className="muted">Nenhum arquivo importado ainda.</p> : <div className="file-list">{files.map(file => <div className="file-row" key={file.id}><strong>{file.logical_path}</strong><span>v{file.latest_version} · {file.byte_len} bytes · {file.sha256.slice(0,12)}…</span></div>)}</div>}
          </section>
        ) : active === "Integrity Guardian" ? (
          <section className="panel">
            <div className="panel-head"><div><h2>Integrity Guardian</h2><p className="muted">Verifica blobs, referências e integridade SHA-256 sem alterar o conteúdo.</p></div><button className="primary" onClick={() => void verifyIntegrity()} disabled={integrityBusy}>{integrityBusy ? "Verificando..." : "Verificar agora"}</button></div>
            <div className="project-row"><label>Projeto <input value={projectId} onChange={e=>setProjectId(e.target.value)} maxLength={64}/></label></div>
            {error && <p className="error">{error}</p>}
            {!integrity ? <p className="muted">Nenhuma verificação executada ainda.</p> : <div className="cards">
              <article><b>Estado</b><strong>{integrity.healthy ? "Íntegro" : "Atenção"}</strong><span>{integrity.checked_blobs} blobs verificados.</span></article>
              <article><b>Problemas</b><strong>{integrity.missing_blobs + integrity.corrupt_blobs + integrity.broken_references}</strong><span>ausentes, corrompidos ou com referência quebrada.</span></article>
              <article><b>Órfãos</b><strong>{integrity.orphan_blobs}</strong><span>blobs sem versão lógica referenciando-os.</span></article>
            </div>}
          </section>
        ) : active === "Backup Center" ? (
          <section className="panel">
            <div className="panel-head"><div><h2>Backup verificado</h2><p className="muted">Cria uma cópia consistente do SQLite usando a API online de backup e valida SHA-256 de cada blob antes de publicar o backup.</p></div><button className="primary" onClick={() => void createBackup()} disabled={backupBusy}>{backupBusy ? "Criando backup..." : "Criar backup"}</button></div>
            <div className="project-row"><label>Projeto <input value={projectId} onChange={e=>setProjectId(e.target.value)} maxLength={64}/></label></div>
            {error && <p className="error">{error}</p>}
            {!backupResult ? <p className="muted">Nenhum backup criado nesta sessão.</p> : <div className="cards">
              <article><b>Resultado</b><strong>Backup concluído</strong><span>ID: {backupResult.backup_id}</span></article>
              <article><b>Blobs</b><strong>{backupResult.blob_count}</strong><span>Arquivos verificados por SHA-256.</span></article>
              <article><b>Banco SQLite</b><strong>{backupResult.database_bytes} bytes</strong><span>Integridade SQLite validada.</span></article>
              <article><b>Local</b><span style={{overflowWrap:"anywhere"}}>{backupResult.backup_path}</span></article>
            </div>}
            <hr />
            <h3>Restaurar para um novo projeto</h3>
            <p className="muted">A restauração valida o manifesto, o SHA-256 do banco e de todos os blobs. O projeto existente nunca é sobrescrito.</p>
            <div className="project-row">
              <label style={{flex:1}}>ID do backup <input value={restoreBackupId} onChange={e=>setRestoreBackupId(e.target.value)} placeholder="UUID do backup" /></label>
              <label style={{flex:1}}>Novo ID do projeto <input value={restoreTargetId} onChange={e=>setRestoreTargetId(e.target.value)} maxLength={64} placeholder={`${projectId}-restored`} /></label>
              <button className="primary" onClick={() => void restoreBackup()} disabled={restoreBusy || !restoreBackupId.trim() || !restoreTargetId.trim()}>{restoreBusy ? "Restaurando..." : "Restaurar backup"}</button>
            </div>
            {restoreResult && <p className="success">Projeto restaurado: <strong>{restoreResult.project_id}</strong> · {restoreResult.blob_count} blobs validados.</p>}
          </section>
        ) : active === "Cost Simulator" ? (
          <section className="panel"><div className="panel-head"><div><h2>Cost Simulator</h2><p className="muted">Cálculo local e determinístico; preços são informados por você.</p></div><button className="primary" onClick={()=>void runCost()}>Calcular</button></div><div className="project-row"><input value={costInput} onChange={e=>setCostInput(e.target.value)} placeholder="Tokens de entrada"/><input value={costOutput} onChange={e=>setCostOutput(e.target.value)} placeholder="Tokens de saída"/><input value={costInPrice} onChange={e=>setCostInPrice(e.target.value)} placeholder="USD/M tokens entrada"/><input value={costOutPrice} onChange={e=>setCostOutPrice(e.target.value)} placeholder="USD/M tokens saída"/></div>{costResult&&<div className="cards"><article><b>Entrada</b><strong>US$ {costResult.input_cost.toFixed(6)}</strong></article><article><b>Saída</b><strong>US$ {costResult.output_cost.toFixed(6)}</strong></article><article><b>Total</b><strong>US$ {costResult.total_cost.toFixed(6)}</strong></article></div>}</section>
        ) : active === "Template Master" ? (
          <section className="panel"><div className="panel-head"><div><h2>Template Master</h2><p className="muted">Templates locais com placeholders {"{{title}}"} e {"{{body}}"}.</p></div><button className="primary" onClick={()=>void saveTemplate()} disabled={!templateName.trim()}>Salvar template</button></div><div className="project-row"><input value={templateName} onChange={e=>setTemplateName(e.target.value)} placeholder="Nome"/><textarea value={templateContent} onChange={e=>setTemplateContent(e.target.value)} rows={4}/></div><button onClick={()=>void loadTemplates()}>Atualizar</button>{templates.map(t=><div className="file-row" key={t.id}><strong>{t.name}</strong><span>{t.updated_at}</span></div>)}</section>
        ) : active === "Editorial Engine" ? (
          <section className="panel"><div className="panel-head"><div><h2>Editorial Engine</h2><p className="muted">Validação local de título, corpo, tamanho e contagem de palavras.</p></div><button className="primary" onClick={()=>void runEditorial()}>Validar</button></div><input value={editorialTitle} onChange={e=>setEditorialTitle(e.target.value)} placeholder="Título"/><textarea value={editorialBody} onChange={e=>setEditorialBody(e.target.value)} rows={14} placeholder="Conteúdo"/>{editorial&&<div className="cards"><article><b>Estado</b><strong>{editorial.valid?"Válido":"Atenção"}</strong></article><article><b>Palavras</b><strong>{editorial.word_count}</strong></article><article><b>Avisos</b><span>{editorial.warnings.join(" · ")||"Nenhum"}</span></article></div>}</section>
        ) : active === "Article Library" ? (
          <section className="panel"><div className="panel-head"><div><h2>Article Library</h2><p className="muted">Índice dos arquivos com versão mais recente.</p></div><button className="primary" onClick={()=>void loadArticles()}>Pesquisar</button></div><input value={articleQuery} onChange={e=>setArticleQuery(e.target.value)} placeholder="Filtrar caminho"/>{articles.map(a=><div className="file-row" key={a.file_id}><strong>{a.logical_path}</strong><span>v{a.version_no} · {a.byte_len} bytes · {a.sha256.slice(0,12)}…</span></div>)}</section>
        ) : active === "Internal Links" ? (
          <section className="panel"><div className="panel-head"><div><h2>Internal Links</h2><p className="muted">Sugestões por correspondência de termos nos caminhos dos artigos.</p></div><button className="primary" onClick={()=>void runLinks()}>Encontrar</button></div><input value={linkQuery} onChange={e=>setLinkQuery(e.target.value)} placeholder="Termos do artigo"/>{links.map(l=><div className="file-row" key={l.file_id}><strong>{l.logical_path}</strong><span>score {l.score}</span></div>)}</section>
        ) : active === "Final Page Builder" ? (
          <section className="panel"><div className="panel-head"><div><h2>Final Page Builder</h2><p className="muted">Monta HTML determinístico e escapa título/corpo antes da publicação.</p></div><button className="primary" onClick={()=>void runPage()}>Montar</button></div><input value={pageTitle} onChange={e=>setPageTitle(e.target.value)} placeholder="Título"/><textarea value={pageBody} onChange={e=>setPageBody(e.target.value)} rows={10} placeholder="Corpo"/><textarea value={pageTemplate} onChange={e=>setPageTemplate(e.target.value)} rows={5} placeholder="Template opcional com {{title}} e {{body}}"/>{pageHtml&&<textarea value={pageHtml} readOnly rows={12}/>}</section>
        ) : active === "OneShot Capture" ? (
          <section className="panel"><div className="panel-head"><div><h2>OneShot Capture</h2><p className="muted">Captura texto diretamente em uma nova versão do projeto.</p></div><button className="primary" onClick={()=>void runCapture()} disabled={!capturePath.trim()}>Capturar</button></div><input value={capturePath} onChange={e=>setCapturePath(e.target.value)} placeholder="caminho.md"/><textarea value={captureContent} onChange={e=>setCaptureContent(e.target.value)} rows={16} placeholder="Cole o conteúdo aqui"/></section>
        ) : active === "Claude Connection" ? (
          <section className="panel"><div className="panel-head"><div><h2>Claude Connection</h2><p className="muted">Configuração segura de conexão. Nenhuma chave secreta é armazenada pelo núcleo.</p></div><button className="primary" onClick={()=>void saveClaude()}>Salvar</button></div><label><input type="checkbox" checked={claudeEnabled} onChange={e=>setClaudeEnabled(e.target.checked)}/> Habilitar conexão</label><input value={claudeEndpoint} onChange={e=>setClaudeEndpoint(e.target.value)} placeholder="https://endpoint-do-provedor"/><input value={claudeModel} onChange={e=>setClaudeModel(e.target.value)} placeholder="Modelo"/><button onClick={()=>void loadClaude()}>Carregar configuração</button>{claude&&<p className="muted">Configurado: {claude.enabled?"sim":"não"} · {claude.model||"sem modelo"}</p>}</section>
        ) : active === "Settings" ? (
          <section className="panel">
            <div className="panel-head"><div><h2>Áreas de importação autorizadas</h2><p className="muted">Somente arquivos dentro destas pastas podem ser importados. Links, junctions, UNC e atalhos são rejeitados pelo backend.</p></div><button className="primary" onClick={loadImportRoots} disabled={rootBusy}>Atualizar</button></div>
            <div className="project-row">
              <label style={{flex:1}}>Caminho absoluto <input value={rootInput} onChange={e=>setRootInput(e.target.value)} placeholder="C:\\Projetos\\MeuProjeto" onKeyDown={e=>{if(e.key==="Enter") void addImportRoot();}} /></label>
              <button className="primary" onClick={addImportRoot} disabled={rootBusy || !rootInput.trim()}>{rootBusy ? "Salvando..." : "Autorizar pasta"}</button>
            </div>
            {error && <p className="error">{error}</p>}
            {importRoots.length === 0 ? <p className="muted">Nenhuma área autorizada. Configure pelo menos uma pasta antes de importar arquivos.</p> : <div className="file-list">{importRoots.map(root => <div className="file-row" key={root.path}><strong>{root.path}</strong><button onClick={() => void removeImportRoot(root.path)} disabled={rootBusy}>Remover</button></div>)}</div>}
          </section>
        ) : (
          <section className="panel"><h2>Fundação funcional</h2><p>O backend Rust já expõe saúde, SHA-256, estimativa de tokens, inicialização de projeto, importação e leitura paginada de arquivos. Os módulos avançados serão ativados sobre esses contratos, sem enviar conteúdo para APIs externas nesta fase.</p></section>
        )}
      </section>
    </main>
  );
}
