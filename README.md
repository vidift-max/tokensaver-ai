# TokenSaver AI

Aplicativo desktop Windows para economia de tokens, otimização determinística de contexto e montagem de páginas editoriais.

## Foundation

- Tauri 2 + React + TypeScript + Rust
- SQLite isolado por projeto
- blobs SHA-256 fora do SQLite
- versões de arquivos imutáveis
- contratos JSON Schema
- slots tipados e estáveis
- contagem de tokens com método e limitações explícitos
- arquitetura preparada para provedores Claude/Anthropic e outros, sem API de rede nesta fase
- conteúdo importado tratado como dado, nunca como instrução

## Módulos planejados

Dashboard, File Center, Token Counter, Smart Context, Safe Optimizer, Project Memory, Claude Connection, Template Master, Editorial Engine, Article Library, Internal Links, Final Page Builder, OneShot Capture, Cost Simulator, Integrity Guardian, Settings e History.

## Armazenamento

Cada projeto terá seu próprio banco em `storage/projects/{project_id}/db.sqlite`. Conteúdo grande fica em blobs endereçados por SHA-256. O arquivo lógico, suas versões e o blob são entidades separadas. Conteúdo idêntico reutiliza o blob; o original nunca é sobrescrito.

## Segurança

A implementação deve bloquear traversal, escapes por symlink/atalhos do Windows, arquivos acima dos limites, conteúdo UTF-8 inválido e execução de scripts/macros importados. Credenciais nunca ficam no frontend ou logs.

## Estado

A branch ativa de desenvolvimento é `v2-saas-alpha`. Esta etapa inicial cria somente a fundação; recursos de IA, rede, OCR, OneShot completo e publicação serão adicionados em fases posteriores com testes.

## Estrutura

- `src/` — frontend React
- `src-tauri/` — backend Rust/Tauri
- `contracts/schemas/` — contratos formais
- `database/` — regras de persistência
- `storage/` — política de blobs e versões
- `tests/` — plano de testes
- `templates/` — fronteira futura do Template Master

## Foundation status

Current branch: `v2-saas-alpha`.

Implemented in the foundation:

- Tauri 2 desktop shell with React + TypeScript.
- Rust backend with SQLite WAL and transactional schema migration.
- Content-addressed project blobs using SHA-256.
- Immutable logical-file versions with duplicate-content reuse.
- Centralized file/batch/path limits.
- Stable IPC error contract.
- UTF-8 validation and sanitized storage/database errors.
- Paginated File Center listing and streamed blob reads.
- Explicit approximate token estimation metadata.
- Rust regression tests for paths, token estimates and migrations.
- GitHub Actions CI for Rust and frontend builds.
- File Center UI connected to the real backend health/project index.

The CI workflow is intentionally part of the repository so every subsequent module is checked before we move toward Claude, optimization and editorial layers.
