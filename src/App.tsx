import { useEffect, useState } from "react";
import { api } from "./api";

const modules = [
  "Dashboard","File Center","Token Counter","Smart Context","Safe Optimizer",
  "Project Memory","Claude Connection","Template Master","Editorial Engine",
  "Article Library","Internal Links","Final Page Builder","OneShot Capture",
  "Cost Simulator","Integrity Guardian","Settings","History"
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

  useEffect(() => {
    api.health().then(() => setBackend("online")).catch(() => setBackend("offline"));
    loadImportRoots();
  }, []);

  async function loadImportRoots() {
    try {
      const roots = await api.listImportRoots();
      setImportRoots(roots);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Não foi possível carregar as áreas de importação.");
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
      setError(e instanceof Error ? e.message : "Não foi possível autorizar a área de importação.");
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
      setError(e instanceof Error ? e.message : "Não foi possível remover a área de importação.");
    } finally {
      setRootBusy(false);
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
      setError(e instanceof Error ? e.message : "Não foi possível carregar os arquivos.");
    } finally {
      setLoadingFiles(false);
    }
  }

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand"><span>TS</span><div><strong>TokenSaver</strong><small>AI Foundation</small></div></div>
        <nav>{modules.map(m => <button className={active===m ? "active" : ""} onClick={() => setActive(m)} key={m}>{m}</button>)}</nav>
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
