# MISSION 9 — PERFORMANCE BENCHMARK LAB & REPORT DELIVERY

**Repository:** `vidift-max/tokensaver-ai`  
**Target branch:** `v2-saas-alpha`  
**Working branch:** `mission-9/performance-benchmark-lab`  
**PR:** [#6 — Team Room Performance Benchmark Lab](https://github.com/vidift-max/tokensaver-ai/pull/6)  
**Initial SHA:** `e18557c65753714bf926845639526b22c0309826`  
**Candidate implementation SHA (before this report commit):** `7af5992de1329a2e1f7107b76ef1ea285487c838`  
**Decision:** **NO-GO** for performance sign-off or production report delivery; benchmark tooling is prepared, live measurements are pending.

> This report separates source inspection, automated correctness tests, benchmark tooling, and actual performance measurements. No latency, throughput, delivery, or database performance number is invented.

## Executive summary

Mission 9 audited the actual `v2-saas-alpha` branch, mapped Team Room communication and Runner/report state, and added a reproducible benchmark lab. The lab contains authenticated HTTP and WebSocket probes and read-only PostgreSQL profiling queries. CI now syntax-checks the Node probes on Linux and Windows.

**No live performance benchmark was executed.** This execution path had no provisioned benchmark API endpoint, room-bound bearer token, dedicated dataset, or direct database connection. PostgreSQL 16 in GitHub Actions is used by correctness/integration workflows, not by a representative load benchmark. The scripts must be run against a provisioned staging environment before any performance conclusion is valid.

No backend optimization, index, schema, heartbeat cadence, notification routing, Runner, Recovery Worker, lease, fencing, identity, authorization, or billing change was made. This is intentional: the mission prohibits optimizing without measurements.

## A. SHA inicial

- Audited target branch: `v2-saas-alpha`.
- Initial HEAD: `e18557c65753714bf926845639526b22c0309826`.
- Relevant open PRs at audit: #3 (AI Provider Gateway), #4 (Mission 8 scheduler optimization), #5 (AI Gateway hardening). None was merged by this mission.
- Mission 8 PR #4 is on `mission-8/performance-communication-optimization`; its scheduler file was not present in the audited `v2-saas-alpha` tree. The main branch's `src/TeamRoom.tsx` still calls `refresh()` for recognized WebSocket events directly. This mission did not import or reimplement Mission 8.

**Classification:** branch/SHA and relevant PR audit — **VALIDADO**. Mission 8 scheduler present on target branch — **PENDENTE** (not present at initial SHA).

## B. SHA final

- Benchmark tooling candidate SHA: `7af5992de1329a2e1f7107b76ef1ea285487c838`.
- Report file is committed separately after this code/tooling SHA. The final report commit SHA is supplied in the PR and mission handoff so this document does not claim to contain its own commit hash.
- PR #6 remains a draft and is not merged.

**Classification:** code/tooling candidate SHA — **IMPLEMENTADO**; final PR merge — **PENDENTE**.

## C. Ambiente

**Source audit:** GitHub repository files and commit/branch metadata.  
**CI:** GitHub Actions, Linux and Windows foundation jobs; separate PostgreSQL 16 Team Room service job; ephemeral Keycloak OIDC smoke job.  
**Benchmark runtime:** NOT provisioned in this execution path. No reachable live Team Room URL, short-lived room session token, representative tenant/room dataset, or direct staging PostgreSQL connection was available.

The PostgreSQL 16 CI service is an integration-test database with fixture data. It is not the representative dataset specified for this mission. The CI run does not execute the new HTTP, WebSocket or EXPLAIN benchmark scripts.

**Classification:** CI environment — **TESTADO**; representative benchmark environment — **PENDENTE**.

## D. Dataset

The lab requires a pre-seeded tenant and room and records counts for:
- tenant and room;
- distinct participants and room memberships;
- active presence memberships;
- channels and messages;
- tasks;
- decisions;
- attachment references.

The benchmark does not create test identities, permission grants, tenants, rooms or data. No representative dataset was supplied or created in this mission. No dataset cardinalities are claimed.

**Classification:** dataset-counting SQL — **IMPLEMENTADO**; representative dataset — **PENDENTE**.

## E. Benchmark HTTP

File: `scripts/benchmark/team-room-http.mjs`.

The probe uses authenticated HTTP requests and configurable concurrency. It exercises:
- message history and unread count (the current API returns unread count in the history response; there is no separate unread route in the inspected client path);
- presence and heartbeat;
- participants;
- tasks;
- decisions;
- attachment references;
- optional unique message writes using `client_message_id`.

It reports request counts, successes, errors, error rate, elapsed duration, throughput and per-endpoint p50/p95/p99/max latency. Configuration includes client concurrency, calls per read endpoint, message-write count and timeout.

No live HTTP requests were sent. Consequently p50/p95/p99, throughput, HTTP error rate, and concurrent-client performance are **not measured**.

**Classification:** HTTP probe — **IMPLEMENTADO**; live HTTP benchmark — **PENDENTE**.

## F. WebSocket / fanout

File: `scripts/benchmark/team-room-websocket.mjs`.

The probe obtains a distinct one-use WebSocket ticket for each connection, opens multiple authenticated sockets, writes unique general-room messages over HTTP, and counts expected versus observed same-room deliveries, missing deliveries, duplicates, delivery latency and throughput. It can also send heartbeat requests and count `PRESENCE_CHANGED` frames.

Limitations:
- Current probe covers multiple connections to one room using the same authenticated principal/session.
- It is a connection/fanout test, not a distinct-participant simulation.
- It does not yet benchmark multiple rooms or prove isolation between independent room identities.
- It was not run against a live service.

Source inspection of `communication.rs` confirms each WebSocket connection listens on the same PostgreSQL `team_room_outbox` notification channel. On each notification, each connection fetches the outbox row and then filters by room; private-channel events receive an additional member/permission check. This is a potential fanout amplification point, but no cost or latency is claimed without measurement.

**Classification:** one-room WebSocket probe — **IMPLEMENTADO**; global-channel behavior — **VALIDADO** by source inspection; fanout benchmark and multi-room isolation benchmark — **PENDENTE**.

## G. Heartbeat / presence

Source inspection of `services/ai-team-room/src/main.rs` confirms `POST /team-room/heartbeat`:
1. authenticates the current session;
2. updates active workload execution heartbeat;
3. updates the current room membership's last-seen timestamp;
4. updates workload identity last-seen;
5. inserts a `PRESENCE_CHANGED` row into `outbox_events` in the same transaction;
6. does not renew task leases.

Thus each accepted heartbeat emits one durable outbox event in the inspected handler. The database trigger/notification path and each WebSocket connection's global LISTEN loop can amplify that event into database reads and room broadcasts. This is a source-level finding, **not** a measured broadcast count, PostgreSQL load, or latency.

The HTTP probe includes heartbeat load. The WebSocket probe counts resulting `PRESENCE_CHANGED` frames. Neither was executed. Heartbeat cadence and presence semantics were not changed.

**Classification:** heartbeat transaction behavior — **VALIDADO** by source inspection; measurement of heartbeat/event/broadcast/HTTP/DB cost — **PENDENTE**; heartbeat optimization — **BLOQUEADO** pending measurement.

## H. PostgreSQL

File: `scripts/benchmark/team-room-postgres.sql`.

The read-only profiling script records selected tenant/room cardinalities and runs `EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS)` on the service query shapes for:
- message history;
- unread count;
- presence snapshot including current-task lookup;
- task list;
- decision list;
- attachment-reference list.

It also captures a `pg_stat_activity` / `pg_locks` wait snapshot.

No representative dataset was available and the SQL was not run. There are no measured execution times, buffer counts, index-use findings, sequential-scan findings, lock waits, or before/after plans in this report. No index was created or changed.

The PostgreSQL integration tests in CI prove selected transactional and authorization behaviors; they are not database benchmarks.

**Classification:** EXPLAIN lab — **IMPLEMENTADO**; representative PostgreSQL benchmark — **PENDENTE**; index optimization — **BLOQUEADO** pending evidence.

## I. Entrega de relatórios

The inspected implementation contains two distinct mechanisms:

1. **Local Mission Runner state:** `scripts/mission-runner.mjs` persists local `STATE.json`, append-only `events.ndjson`, checkpoints and prepared prompt files. The `prompt-delivered` transition explicitly fails closed while no verified external provider delivery adapter exists. A caller-supplied string is not accepted as delivery proof.
2. **Team Room chat:** general/private messages are persisted in PostgreSQL. Message writes use `client_message_id` idempotency and insert the `MESSAGE_CREATED` outbox event in the same database transaction. WebSocket notifications are delivered to connected authorized clients; the frontend reconnect path reloads authoritative snapshots/history.

There is no verified end-to-end pipeline that takes an AI-generated report, dispatches it to an external provider/recipient, persists a delivery receipt, collects a recipient ACK, retries unacknowledged delivery, deduplicates recipient processing, and recovers pending report deliveries after disconnection. The WebSocket loop consumes global NOTIFY events but has no application-level recipient ACK protocol or durable per-recipient delivery cursor. Chat message persistence and idempotent writes must not be mislabeled as report delivery.

No external AI prompt dispatch or external model receipt is claimed. No delivery infrastructure was invented in this mission.

**Classification:** local report/prompt preparation — **IMPLEMENTADO**; durable Team Room message persistence/outbox — **IMPLEMENTADO** and covered by integration tests; end-to-end report dispatch/recipient ACK/retry/recovery — **BLOQUEADO**; external AI receipt — **PENDENTE**.

## J. Comunicação entre participantes

The Team Room service contains authenticated workload sessions, room membership/permission checks, persistent general/private messages, one-use WebSocket tickets, room/private-channel fanout checks, message idempotency, decisions, attachment references and transactional outbox events.

The service CI PostgreSQL integration suite passed the general/private ACL and message-idempotency test. Keycloak OIDC smoke also passed in the same CI run. This establishes the tested infrastructure path, not a real connection to external AI model providers.

The communication service can represent independent authenticated principals when separately provisioned. The WebSocket probe added here does not simulate independent participants: its connections share one principal/session. No claim is made that GPT, Claude, Gemini, Kimi, or any external model actually exchanged messages.

**Classification:** authenticated communication infrastructure — **IMPLEMENTADO**; PostgreSQL communication integration test — **TESTADO**; independent external AI-to-AI conversation — **PENDENTE**.

## K. Antes/depois

No comparable baseline or post-change performance run exists. No latency/throughput/error values are available. No measured improvement percentage is reported.

No backend optimization was made because the mission's own evidence gate was not met. The only changes are benchmark tooling, documentation and CI syntax checks.

**Classification:** before/after performance comparison — **PENDENTE**; performance improvement — **NÃO MEDIDO**.

## L. Regressão

The TokenSaver AI CI run #623 on the pre-report tooling SHA `7ca85bac4dab5b1737c2b8ae119beffa09696b04` completed the Team Room service, Keycloak/OIDC, Linux and Windows foundation jobs successfully. The service job reported:
- authentication/JWT suite: 6 passed, 0 failed;
- HTTP security, handoff, distributed lease/fencing and communication PostgreSQL suite: 8 passed, 0 failed;
- admission suite: 3 passed, 0 failed;
- session/OIDC suite: 4 passed, 0 failed.

The communication PostgreSQL test `postgres_general_private_acl_and_message_idempotency` passed. The run also included the existing desktop/mission regression and build jobs; the final report-commit SHA receives a separate exact-SHA CI run.

The separate Distributed Runner PostgreSQL Lab #146 completed successfully for its Runner lease/fencing, handoff/recovery and independent-process integration proof. This is regression/integration evidence, not performance data.

**Classification:** service integration regression — **TESTADO**; full final-SHA regression — **PENDENTE** until the CI run for the report commit completes.

## M. CI

- TokenSaver AI CI #623, run ID `43202672926`, candidate tooling SHA `7ca85bac4dab5b1737c2b8ae119beffa09696b04`: completed successfully.
- Distributed Runner PostgreSQL Lab #146, run ID `43202672986`: completed successfully.
- A subsequent PostgreSQL dataset-scope correction was committed as `7af5992de1329a2e1f7107b76ef1ea285487c838`. CI #625 and Runner #148 were triggered for that SHA; at report preparation they were pending/in progress.
- The report commit itself triggers another exact-SHA CI run. Its final status is recorded in the mission handoff; no earlier SHA result is silently substituted for it.

CI's benchmark-specific check is `node --check` for the HTTP and WebSocket probe scripts. This verifies JavaScript syntax only; it does not execute benchmark traffic or validate SQL against a live PostgreSQL schema.

**Classification:** CI #623 — **VALIDADO** on its exact SHA; CI #625/#148 — **PENDENTE** at report preparation; live benchmark execution from CI — **PENDENTE**.

## N. Gargalos encontrados

Static source findings, not measured bottlenecks:
1. All WebSocket connections listen on the same global PostgreSQL notification channel.
2. Each connection fetches the outbox row for every notification before room filtering.
3. Heartbeat inserts a `PRESENCE_CHANGED` outbox event on every accepted heartbeat.
4. The current presence snapshot includes a correlated lookup for a participant's current active task.
5. Unread count uses a count over messages after the stored read cursor; no performance conclusion can be drawn without realistic cardinality and EXPLAIN output.
6. Report delivery has no verified recipient-acknowledged dispatch/retry pipeline.

No source finding is called a proven performance bottleneck until measured.

**Classification:** candidate hot paths — **VALIDADO** by source inspection; measured bottleneck ranking — **PENDENTE**.

## O. Otimizações implementadas

No performance optimization was justified by measured data, so none was implemented. The committed work consists only of:
- authenticated HTTP load probe;
- authenticated one-room WebSocket fanout probe;
- PostgreSQL profiling SQL;
- reproducibility instructions;
- CI syntax checks.

No changes to Runner, Recovery Worker, leases, fencing, heartbeat cadence, authorization, authentication, identity, outbox semantics or indexes.

**Classification:** benchmark lab tooling — **IMPLEMENTADO**; performance optimization — **PENDENTE**.

## P. Pendências

1. Provision an isolated staging Team Room service and room-bound short-lived identities.
2. Seed and document tenant/room/participant/message/task/presence/decision/reference cardinalities.
3. Run repeated HTTP load scenarios and retain raw JSON output.
4. Run WebSocket fanout with independent participants and separate room-bound identities; measure cross-room irrelevant-event cost.
5. Measure heartbeat event count, WebSocket broadcasts, HTTP cost and PostgreSQL write/read cost.
6. Run the PostgreSQL EXPLAIN script on the representative dataset and retain plans/settings.
7. Design/implement report delivery only after product-level recipient, acknowledgement and retention semantics are specified.
8. Validate end-to-end report persistence, notification, receipt, ACK, retry, idempotency and reconnect recovery.
9. Repeat performance measurements after any evidence-backed optimization.

## Q. Riscos

- Global NOTIFY fanout can amplify notification work as WebSocket connection count grows.
- Heartbeat event generation can create unnecessary presence fanout if the cadence is aggressive; current cadence and production client behavior were not measured here.
- A connected WebSocket delivery is not equivalent to a durable recipient ACK.
- A persistent chat message is not equivalent to verified external AI report delivery.
- The single-room WebSocket probe cannot prove multi-room isolation or multi-tenant fanout cost.
- No benchmark dataset means no valid query-plan or index decision.
- Mission 8 scheduler code remains on its separate PR branch and is not in the audited target branch SHA.

No security boundary was relaxed to improve benchmark numbers.

## R. Evidências

- PR #6: https://github.com/vidift-max/tokensaver-ai/pull/6
- HTTP probe: `scripts/benchmark/team-room-http.mjs`
- WebSocket probe: `scripts/benchmark/team-room-websocket.mjs`
- PostgreSQL lab: `scripts/benchmark/team-room-postgres.sql`
- Runbook: `scripts/benchmark/README.md`
- CI workflow: `.github/workflows/ci.yml`
- Existing communication implementation report: `reports/AI_TEAM_ROOM_CHAT_IMPLEMENTATION_REPORT.md`
- Existing Runner continuity report: `reports/AI_WORK_CONTINUITY_RUNNER_REPORT.md`
- CI #623: https://github.com/vidift-max/tokensaver-ai/actions/runs/43202672926
- Distributed Runner #146: https://github.com/vidift-max/tokensaver-ai/actions/runs/43202672986

## S. Decisão final

<summary>
**NO-GO** for performance sign-off and end-to-end report delivery.

The reproducible benchmark tools are committed, and existing correctness/integration CI evidence is positive. However, the mission's core performance requirements remain unmeasured: live HTTP p50/p95/p99, WebSocket fanout/loss/duplicates, heartbeat amplification, representative PostgreSQL EXPLAIN/BUFFERS, and before/after comparison. The AI report delivery path also lacks verified external dispatch and recipient ACK/retry/recovery.

Do not merge PR #6 as proof of performance improvement. Run the probes against a dedicated provisioned staging environment, attach raw outputs and exact-SHA provenance, then make only evidence-backed optimizations.
</summary>


---

# CHECKPOINT_PARTIAL — 2026-11-12

## Estado retomável da execução Mission 9.1

**CHECKPOINT_PARTIAL — missão NÃO concluída.** Nenhum merge foi realizado. Nenhum trabalho, migration, teste, relatório ou artefato foi revertido ou descartado.

- Branch: `mission-9/performance-benchmark-lab`
- PR: #6 — aberto, sem merge
- SHA exato observado antes deste registro: `b167b5f53f86d8634ebd7b66cf4bd9f7482af9b4`
- Este commit altera somente este relatório. O SHA do commit de checkpoint será o novo HEAD; o SHA acima identifica o estado de código antes da atualização documental.

### O que foi concluído

1. O laboratório isolado foi executado no GitHub Actions, não apenas validado sintaticamente.
2. No workflow TokenSaver AI CI #722, run `43219293235`, o job `team-room-performance-benchmark` terminou **success**. Os logs comprovam:
   - PostgreSQL 16 iniciado em container;
   - Team Room service compilado e iniciado;
   - fixture de benchmark semeada;
   - geração de planos PostgreSQL via `scripts/benchmark/team-room-postgres.sql`;
   - snapshots de estatísticas PostgreSQL antes/depois da carga HTTP e após WebSocket;
   - três execuções do benchmark HTTP read + heartbeat;
   - uma execução de HTTP message writes;
   - execução do benchmark WebSocket fanout/heartbeat com oito participantes independentes;
   - coleta de amostras de recursos e logs sanitizados;
   - upload dos resultados brutos.
3. Artefato bruto do job:
   - Nome: `team-room-performance-benchmark-43219293235`
   - Artifact ID: `13944119889`
   - SHA-256 do ZIP: `4ad05211bf5f61e825853c52d19aff70d88999dfa5a1368ddba642e326f89f81`
   - Retenção configurada pelo workflow: 14 dias.
4. No SHA atual `b167b5f...`, o CI #736 / run `43223918572` terminou **success**; os jobs Linux foundation, Windows foundation, Team Room service e Keycloak/OIDC terminaram sem falhas.
5. No mesmo SHA, Distributed Runner PostgreSQL Lab #259 / run `43223918506` terminou **success**.
6. O PR #6 continua aberto. Nenhum merge foi executado.

### O que está parcialmente concluído

- **HTTP:** carga real contra o Team Room service foi executada em três repetições de leitura/heartbeat e uma carga de escrita de mensagens. Os arquivos JSON brutos foram preservados no artefato. Os percentis, throughput e taxa de erro ainda precisam ser extraídos, conferidos e incorporados ao relatório final.
- **WebSocket/fanout:** o probe real com oito participantes foi executado e o JSON bruto foi preservado. O cenário executado foi de uma sala; não comprova fanout multi-room, isolamento sob carga multi-room nem custo comparativo do canal global.
- **PostgreSQL:** SQL de planos e snapshots de estatísticas foram executados e preservados. Os planos, buffers, WAL, índices, waits e cardinalidades ainda precisam ser examinados antes de qualquer conclusão de gargalo.
- **Heartbeat/presence:** foi exercitado nos cenários HTTP e WebSocket. A quantificação separada de eventos `PRESENCE_CHANGED`, broadcasts e writes atribuíveis exclusivamente ao heartbeat ainda não foi consolidada.
- **Ambiente:** trata-se de benchmark isolado em CI com PostgreSQL 16 e credenciais efêmeras; não é benchmark de produção nem capacidade de produção.
- **Entrega de relatórios:** não há evidência de fluxo externo completo com destinatário IA externo, ACK de destinatário, retry durável e recovery end-to-end. Persistência de mensagem no Team Room não comprova entrega externa.
- **Comunicação entre IAs externas:** não validada. O teste de WebSocket comprova transporte/fanout entre sessões autenticadas de teste, não agentes externos independentes conversando.

### O que não foi iniciado ou não foi comprovado

- Extração/análise numérica dos arquivos JSON e SQL do artefato #722.
- Benchmark multi-room com eventos simultâneos e mensuração quantitativa do custo de notificações globais irrelevantes.
- Experimento isolado de heartbeat com contagem exata de `PRESENCE_CHANGED`, broadcasts e writes.
- Análise detalhada de locks/waits e cardinalidades do PostgreSQL sob cenários concorrentes.
- Fluxo de entrega de relatório a destinatário externo com ACK/retry/recovery/idempotência/ordenação.
- Benchmark antes/depois de qualquer otimização. Nenhuma otimização de desempenho foi justificada ou implementada nesta execução.
- Regressão de benchmark comparativa pós-otimização.

### Testes e workflows — evidência disponível

**Passaram:**
- Job `team-room-performance-benchmark` do CI #722 / run `43219293235`: success.
- CI #736 / run `43223918572`, SHA `b167b5f...`: success; Linux, Windows, Team Room service e Keycloak/OIDC.
- Distributed Runner PostgreSQL Lab #259 / run `43223918506`, SHA `b167b5f...`: success.

**Falhas intermediárias conhecidas:**
- Uma execução anterior do benchmark (#647, conforme histórico do PR) falhou durante o seeding por literais UUID não delimitados; a falha ocorreu antes de enviar carga. O seeder foi corrigido e uma execução posterior (#722) concluiu o job de benchmark com sucesso.
- Houve falha transitória de provisionamento de dependências Linux por mirror HTTP indisponível; o workflow foi ajustado para usar mirror HTTPS e o CI #736 no SHA atual passou.
- Não há falha conhecida nos workflows finais #736/#259 do SHA atual.

**Ainda executando no momento deste checkpoint:** nenhum workflow associado ao SHA atual `b167b5f...` — ambos os workflows observados terminaram success. A execução longa está interrompida por solicitação; não foram iniciadas novas execuções.

### Comandos e operações executados (resumo verificável nos logs)

- `cargo build` do serviço Team Room, conforme step “Build Team Room service”.
- Inicialização do serviço Team Room em `127.0.0.1:8081`.
- `node scripts/benchmark/seed-team-room.mjs`.
- `psql ... -f scripts/benchmark/team-room-postgres.sql`.
- `psql ... -f scripts/benchmark/team-room-postgres-stats.sql` antes/depois das cargas.
- `node scripts/benchmark/team-room-http.mjs` — três repetições read/heartbeat e uma execução message writes.
- `node scripts/benchmark/team-room-websocket.mjs`.
- Coleta de CPU/memória, logs sanitizados e upload do diretório `benchmark-artifacts/`.
- CI e Distributed Runner workflows executados pelo GitHub Actions.

### Próximos passos obrigatórios para retomada

1. Baixar e inspecionar o artefato #722 (`13944119889`); validar JSONs, SQL plans, snapshots e amostras de recursos.
2. Extrair e registrar requests, throughput, erros, p50/p95/p99/max, concorrência e duração para cada cenário HTTP; preservar os arquivos brutos e identificar a configuração exata.
3. Extrair métricas WebSocket de entrega, atraso, perda e duplicação. Registrar explicitamente que o cenário atual é single-room.
4. Interpretar EXPLAIN/ANALYZE/BUFFERS/WAL e estatísticas PostgreSQL; não inferir gargalos sem os planos e counters.
5. Delimitar o que o experimento atual permite afirmar sobre heartbeat; executar cenário dedicado se for necessário atribuir causalidade.
6. Implementar multi-room somente se necessário e seguro para testar o custo do canal global; não alterar o mecanismo de notificação sem evidência.
7. Documentar a lacuna de entrega externa de relatórios e especificar o destinatário/contrato de ACK antes de implementar infraestrutura.
8. Executar regressão completa e novo benchmark apenas após alterações justificadas por dados.
9. Atualizar este relatório com resultados numéricos derivados dos artefatos e decidir GO / GO CONDITIONED / NO-GO. Até lá, manter **NO-GO** para sign-off de desempenho e entrega externa.

**Classificação do checkpoint:** laboratório executado e artefato preservado — **TESTADO**; resultados quantitativos ainda não analisados — **PENDENTE**; entrega externa de relatórios — **PENDENTE**; missão — **CHECKPOINT_PARTIAL / NO-GO**.
