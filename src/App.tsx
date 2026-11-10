import { useEffect, useState } from "react";
import { api } from "./api";

const modules = [
  {name:"Dashboard",ready:true},{name:"File Center",ready:true},{name:"Token Counter",ready:true},
  {name:"Smart Context",ready:false},{name:"Safe Optimizer",ready:false},{name:"Project Memory",ready:false},
  {name:"Claude Connection",ready:false},{name:"Template Master",ready:false},{name:"Editorial Engine",ready:false},
  {name:"Article Library",ready:false},{name:"Internal Links",ready:false},{name:"Final Page Builder",ready:false},
  {name:"OneShot Capture",ready:false},{name:"Cost Simulator",ready:false},{name:"Integrity Guardian",ready:true},{name:"Backup Center",ready:true},
  {name:"Settings",ready:true},{name:"History",ready:false}
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

        {active === "File Center" ? (
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
