# AI Collaboration — TokenSaver AI

## Objetivo

Este arquivo é o canal assíncrono de colaboração entre diferentes IAs/engenheiros que trabalham no TokenSaver AI.

Repositório: vidift-max/tokensaver-ai
Branch de trabalho: v2-saas-alpha

## Regras

1. Não apagar o histórico de decisões.
2. Cada IA deve identificar claramente sua seção.
3. Antes de propor uma alteração, verificar o código real da branch.
4. Não considerar uma sugestão como correta sem confirmar no código.
5. Alterações de código devem ser feitas somente pelo engenheiro responsável pela integração, salvo acordo explícito.
6. Registrar problemas reais, falsos positivos e testes necessários.
7. Quando uma sugestão for incorporada, marcar como INTEGRADA e informar o commit.
8. Não colocar segredos, tokens, chaves de API ou dados pessoais neste arquivo.

---

## ESTADO ATUAL

A fundação do TokenSaver está sendo endurecida antes do início do projeto Blogger SEO Intelligence/SaaS.

Áreas principais:
- Tauri 2 / React 19 / TypeScript / Rust / SQLite
- isolamento por projeto
- blobs content-addressed por SHA-256
- versões imutáveis de arquivos
- File Center
- Smart Context
- Safe Optimizer
- Project Memory
- Claude Config
- Templates
- Editorial Engine
- Article Library / Internal Links
- Final Page Builder
- OneShot Capture
- Cost Simulator
- Integrity Guardian
- Backup / Restore
- Settings / History

---

## TAREFAS PRIORITÁRIAS

1. Verificar o CI do commit mais recente.
2. Auditar todos os comandos Tauri.
3. Auditar Smart Context para limites de arquivos, bytes e CPU.
4. Auditar File Center para ownership, leitura e limites de saída.
5. Auditar OneShot Capture e reutilização segura do helper de blobs.
6. Auditar concorrência/atomicidade do Project Memory.
7. Auditar limites de list_templates, list_articles e internal links.
8. Auditar Backup/Restore e isolamento SQLite.
9. Revisar exposição dos comandos Tauri.
10. Fortalecer testes de regressão.
11. Rodar CI completo e fechar a fundação.

---

## CORREÇÕES JÁ FEITAS

- validação de caminhos lógicos contra traversal;
- proteção dos blobs contra symlinks/races;
- limite de Project Memory em 1000 entradas;
- proteção de overflow de offset no File Center;
- ranking de candidatos de internal links;
- limite de entrada de sha256_text e estimate_tokens em 2 MB.

---

# QUADRO DE COLABORAÇÃO

## IA EXTERNA — ANÁLISE

Status: AGUARDANDO

Use esta seção para registrar:
- problemas reais;
- severidade;
- arquivo/função;
- explicação;
- correção recomendada;
- testes necessários;
- falsos positivos identificados.

### Problemas encontrados

_Nenhum registrado ainda._

### Falsos positivos

_Nenhum registrado ainda._

### Recomendações

_Nenhuma registrada ainda._

---

## IA INTEGRADORA — VALIDAÇÃO

Status: AGUARDANDO ANÁLISE EXTERNA

A IA integradora deve conferir cada recomendação contra o código real antes de implementar.

Para cada item:
- ACEITO
- REJEITADO
- JÁ CORRIGIDO
- PRECISA INVESTIGAÇÃO

Registrar o commit quando uma correção for integrada.

---

## LOG DE CONVERSA

Formato recomendado:

### [DATA/HORA] — IA EXTERNA
Mensagem/achado.

### [DATA/HORA] — IA INTEGRADORA
Resposta, validação ou decisão.

### [DATA/HORA] — IA EXTERNA
Nova análise.

Não remover entradas anteriores.

---

## DEFINIÇÃO DE PRONTO

A fundação só será considerada concluída quando:
- os riscos reais de segurança/DoS identificados forem tratados;
- os testes de regressão necessários existirem;
- o CI estiver verde;
- não houver comandos Tauri importantes sem validação adequada;
- isolamento entre projetos estiver comprovado;
- backup/restore estiverem validados;
- limites de processamento e saída estiverem adequados.

Depois disso, iniciar o Blogger SEO Intelligence/SaaS.
