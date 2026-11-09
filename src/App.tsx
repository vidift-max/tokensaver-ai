import { useState } from "react";

const modules = [
  "Dashboard","File Center","Token Counter","Smart Context","Safe Optimizer",
  "Project Memory","Claude Connection","Template Master","Editorial Engine",
  "Article Library","Internal Links","Final Page Builder","OneShot Capture",
  "Cost Simulator","Integrity Guardian","Settings","History"
];

export default function App() {
  const [active, setActive] = useState("Dashboard");
  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand"><span>TS</span><div><strong>TokenSaver</strong><small>AI Foundation</small></div></div>
        <nav>{modules.map(m => <button className={active===m ? "active" : ""} onClick={() => setActive(m)} key={m}>{m}</button>)}</nav>
      </aside>
      <section className="content">
        <header><div><p className="eyebrow">FOUNDATION / {active.toUpperCase()}</p><h1>{active}</h1><p className="muted">Base segura para economia de tokens, contexto determinístico e operações rastreáveis.</p></div><div className="status">● Foundation</div></header>
        <div className="cards">
          <article><b>Projeto</b><strong>TokenSaver AI</strong><span>Arquitetura Tauri 2 + React + TypeScript + Rust + SQLite.</span></article>
          <article><b>Integridade</b><strong>Original imutável</strong><span>Versões futuras usarão SHA-256 e armazenamento por conteúdo.</span></article>
          <article><b>Tokens</b><strong>Estimativa explícita</strong><span>Contagem sempre identificará método, modelo e limitações.</span></article>
        </div>
        <section className="panel"><h2>Próxima etapa</h2><p>Foundation criada. O backend Rust, banco SQLite, contratos e operações de arquivos serão adicionados sem executar conteúdo importado.</p></section>
      </section>
    </main>
  );
}
