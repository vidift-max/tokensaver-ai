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


---

# QUANTITATIVE ARTIFACT ANALYSIS — 2026-11-12

**Status:** `PERFORMANCE_PARTIAL`  
**Artifact:** `13944119889` — `team-room-performance-benchmark-43219293235`  
**Workflow run:** `43219293235`  
**Artifact SHA-256 verified after download:** `4ad05211bf5f61e825853c52d19aff70d88999dfa5a1368ddba642e326f89f81`  
**Workflow source SHA:** `23ac3c138562875736f831a5401b457d1dd5c87e`  
**Report branch HEAD at this update:** `2129750de88bf04117bde1be568c104ffd420ae6`

This section supersedes earlier statements in sections E–H and the prior checkpoint that the artifact had not yet been analyzed or that no multi-room noise-isolation scenario had run. The benchmark itself is not rerun here. The workflow artifact was downloaded, its ZIP digest was independently checked, and all 12 files listed below were inspected. The run's recorded source SHA is `23ac3c1...`; the current branch HEAD `2129750...` is a later report/checkpoint commit, not the SHA on which the benchmark workload ran.

## 1. Artifact inventory

All files found in the ZIP (12 total):

1. `http-read-1.json`
2. `http-read-2.json`
3. `http-read-3.json`
4. `http-message-writes.json`
5. `websocket-fanout.json`
6. `postgres-explain.txt`
7. `postgres-stats-before-http.json`
8. `postgres-stats-after-http.json`
9. `postgres-stats-after-websocket.json`
10. `resource-samples.csv`
11. `seed-metadata.json`
12. `team-room-service.log`

The ZIP digest matched the GitHub artifact digest exactly. The artifact is retained in GitHub Actions; it was not deleted or overwritten.

## 2. Dataset and execution envelope

The benchmark ran against the isolated Team Room service at loopback `127.0.0.1:8081`, backed by PostgreSQL `16.15`. The fixture metadata records:

- 20 tenants and 20 rooms;
- 8 distinct authenticated participants/sessions in the target room;
- 4 additional room IDs with 4 other-room participants/tokens;
- 10,000 seeded messages;
- 250 tasks, 250 decisions and 250 attachment references;
- 8 active target-room presence memberships.

This is an isolated CI fixture, not a production-like network deployment or a production capacity test.

## 3. HTTP results

### Read + heartbeat repetitions

Each repetition used 8 concurrent workers and 100 requests per endpoint across seven endpoints (messages/history+unread, presence, heartbeat, participants, tasks, decisions and attachment references): 700 requests per repetition, 2,100 total. All three runs reported zero errors.

| Run | Requests | Duration | Throughput | Errors |
|---|---:|---:|---:|---:|
| Read 1 | 700 | 2,427.582 ms | 288.353 req/s | 0 / 700 |
| Read 2 | 700 | 1,042.315 ms | 671.582 req/s | 0 / 700 |
| Read 3 | 700 | 993.634 ms | 704.484 req/s | 0 / 700 |
| Combined active durations | 2,100 | 4,463.531 ms | 470.480 req/s | 0 / 2,100 |

The combined throughput is total requests divided by the sum of the three measured run durations; it is not a continuous soak-test throughput. Percentiles below are the **median of the three per-run percentile summaries**, not pooled percentiles. Raw per-request latency samples were not included in the artifact, so exact pooled p50/p95/p99 are `NOT_AVAILABLE`.

| Endpoint (300 requests total) | Median run p50 | Median run p95 | Median run p99 | Maximum observed |
|---|---:|---:|---:|---:|
| Messages history + unread | 18.673 ms | 33.581 ms | 40.419 ms | 234.197 ms |
| Presence | 7.559 ms | 10.351 ms | 13.751 ms | 17.061 ms |
| Heartbeat | 8.565 ms | 13.238 ms | 14.811 ms | 392.842 ms |
| Participants | 7.281 ms | 10.808 ms | 13.185 ms | 17.644 ms |
| Tasks | 12.201 ms | 16.304 ms | 17.973 ms | 19.718 ms |
| Decisions | 11.824 ms | 15.315 ms | 16.817 ms | 52.428 ms |
| Attachment references | 12.197 ms | 15.465 ms | 16.794 ms | 21.001 ms |

Important tail observations:
- Messages/history+unread run-1 p95/p99 were 227.070/233.528 ms, versus 33.351/40.419 ms and 33.581/37.194 ms in runs 2 and 3.
- Heartbeat run-1 p95/p99/max were 286.120/376.737/392.842 ms. Runs 2 and 3 were 13.238/14.811/16.189 ms and 9.401/10.959/12.079 ms.
- This first-run tail spike is real in the artifact. Its cause is **not established**; warm-up, contention or another transient cause must not be asserted without an isolated repeat and correlated traces.

### Message writes

The write run used 8 workers, 100 message writes and one request to each of the seven read endpoints: 107 total requests. It recorded 107 successes, zero errors, 0% error rate and 409.753 req/s over 261.133 ms.

The `message_write` endpoint recorded:
- p50: 14.531 ms
- p95: 38.742 ms
- p99: 90.337 ms
- max: 92.811 ms
- errors: 0 / 100

The 409.753 req/s figure includes the seven auxiliary read requests; it is not message-write-only throughput.

## 4. WebSocket and fanout

The artifact records 8 sockets with 8 distinct session tokens and distinct participants, 50 accepted target-room message writes and 3,000 ms settling time.

Observed:
- Expected same-room deliveries: 400
- Observed same-room deliveries: 400
- Missing deliveries: 0
- Duplicate deliveries: 0
- Total received frames: 464
- Delivery latency: p50 6.761 ms, p95 52.142 ms, p99 107.602 ms
- Observed delivery throughput: 109.173 deliveries/s over 3,663.905 ms
- Failures: none; duplicate samples: none

The 464 frames reconcile to 400 message deliveries plus 64 presence frames. The test observed 8 accepted heartbeat writes and 64 `PRESENCE_CHANGED` frames (8 per heartbeat), with 0 missing presence frames.

This is measured evidence for the tested eight-participant, one-target-room fanout scenario. It is not evidence of sustained high connection counts, reconnect/replay guarantees, or production network latency.

## 5. Multi-room isolation — existing scenario found; no rerun required

The existing workflow already passed four other room IDs and credentials to the WebSocket probe. During the target-room socket test it submitted 5 writes to each of those four rooms:

- Other-room events requested: 20
- Other-room writes accepted: 20
- Frames from other rooms received by target-room sockets: 0

This is a real cross-room noise/isolation check, not merely a source-level assertion. Therefore the requested conditional implementation was unnecessary and no new multi-room benchmark was added or repeated.

Scope limitation: the probe did not open subscriber sockets in all four other rooms and verify each room's positive delivery independently. It proves that, in this execution, events written to four other rooms did not leak into the eight target-room sockets. It does not establish multi-room throughput scaling or quantify global PostgreSQL notification-channel cost.

Source inspection at the artifact's code SHA also found `event_id_for_room_notification`, which rejects a notification carrying a different room hint before fetching its outbox row. The test `room_hint_filters_unrelated_notifications_before_outbox_fetch` covers that filter in code. The artifact's zero cross-room frames is runtime evidence for the exercised path.

## 6. Heartbeat attribution and contamination

HTTP heartbeat was not isolated from the other read routes: each of the three HTTP repetitions concurrently exercised 100 heartbeat calls and 100 calls to each of six other endpoints. Therefore overall read-run throughput is a mixed workload, not pure read throughput.

The endpoint-specific heartbeat measurements are available above. The first repetition has a pronounced tail spike (p95 286.120 ms, p99 376.737 ms, max 392.842 ms), while repetitions 2 and 3 are materially lower. This is sufficient to flag heartbeat tail latency as an investigation target, but not to attribute the spike to database writes, outbox processing, or WebSocket fanout.

The WebSocket phase provides a narrower attribution:
- 8 heartbeat requests were accepted;
- the outbox event-type count increased from 301 to 309 `PRESENCE_CHANGED` between the after-HTTP and after-WebSocket snapshots (+8);
- 64 `PRESENCE_CHANGED` frames were observed, exactly 8 per accepted heartbeat across 8 sockets.

This supports the expected 1 event per accepted heartbeat and 1 broadcast per connected target-room socket for this eight-heartbeat scenario. It does not measure isolated heartbeat request latency or isolate heartbeat-only PostgreSQL CPU/WAL cost. A dedicated heartbeat-only load test remains pending; do not repeat the existing mixed HTTP/WebSocket workloads.

## 7. PostgreSQL query-plan analysis

The captured fixture cardinalities were: 10,000 messages, 250 tasks, 250 decisions, 250 attachment references, 8 active room memberships and 1 target room/channel. Six query plans were captured with `EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS)`.

| Query | Execution time | Shared buffers | Observed plan |
|---|---:|---:|---|
| Message history | 0.054 ms | 20 hits | `team_room_messages_history_idx` index scan; nested loop and Memoize (43 hits, 8 misses) |
| Unread count | 2.054 ms | 214 hits | Sequential scan of 10,000-message relation; 8,750 rows passed filter, 1,250 removed; nested-loop left join |
| Presence snapshot | 0.127 ms | 23 hits | Hash joins, 8 output rows, 26 kB quicksort; task subquery uses `team_tasks_owner_lease_idx` |
| Tasks | 0.089 ms | 6 hits | Sequential scan of 250 rows; top-N heapsort, 38 kB |
| Decisions | 0.072 ms | 6 hits | Sequential scan of 250 rows; top-N heapsort, 50 kB |
| Attachment references | 0.125 ms | 8 hits | Sequential scan of 250 rows, hash join, top-N heapsort, 50 kB |

Findings:
1. Message history uses the intended history index and stops after 51 rows; no missing index is demonstrated by this plan.
2. Unread count is the dominant measured query in this six-query sample at 2.054 ms. Its message-table sequential scan reads 213 shared buffers and processes 8,750 rows. The join estimate is 2,917 rows versus 8,750 actual (approximately 3x under-estimation). This is a candidate for later index/query-shape evaluation at larger message cardinalities, not proof that an index is currently missing or that the query is a production bottleneck.
3. Sequential scans on 250-row task, decision and attachment fixtures are not unexpected at this small scale; their measured execution times are below 0.13 ms.
4. Presence uses an index for the correlated task lease lookup; no expensive sort/hash spill is visible. All observed sorts are small in-memory quicksort/top-N operations.
5. The captured plans show shared-buffer hits; no per-plan disk read buffers or sort spills are shown. The SELECT plans do not provide write-WAL evidence.
6. No lock wait or ungranted lock was observed in the final query/lock snapshot. This is a point-in-time snapshot, not a contention profile.

PostgreSQL 16.15 counters:
- Before HTTP: 252,174 shared hits, 354 shared reads, 79 commits, 0 deadlocks, 0 rollbacks, 0 temp files/bytes; outbox event count empty.
- After HTTP: 425,020 shared hits, 393 shared reads, 11,833 commits, 0 deadlocks, 0 rollbacks, 0 temp files/bytes; event counts 301 `PRESENCE_CHANGED` and 100 `MESSAGE_CREATED`.
- After WebSocket: 452,468 shared hits, 395 shared reads, 16,205 commits, 0 deadlocks, 0 rollbacks, 0 temp files/bytes; event counts 309 `PRESENCE_CHANGED` and 170 `MESSAGE_CREATED`.
- WAL bytes reported by `pg_stat_wal`: 16,765,424 before HTTP; 17,321,836 after HTTP; 17,659,235 after WebSocket.
- The outbox table snapshot reported 464 index scans after WebSocket and 401 sequential tuples read. The snapshots do not fully reconcile all table insertion counters with event-type counts (notably after HTTP); therefore do not assign every outbox counter increment to a specific endpoint.

No SQL, schema, index, or service optimization was made.

## 8. CPU and memory samples

The artifact contains 8 one-second-ish samples, not a high-resolution profiler:
- Team Room process RSS: 17,348–21,248 KiB.
- Team Room process CPU sample: 0.1–43.4% (the CSV column is process CPU average since start).
- PostgreSQL CPU sample: 0.03–68.54%.
- PostgreSQL container memory: 37.18–125.9 MiB.

These are coarse observations for this short CI run. They cannot be used as steady-state capacity, peak allocation, or production resource sizing.

## 9. External report delivery

Source/report inspection confirms distinct mechanisms:
- Mission Runner stores local mission state, event history, checkpoints and prompt files.
- Its `prompt-delivered` transition is disabled/fail-closed until a verified provider dispatch adapter exists.
- Team Room persists general/private messages, uses `client_message_id` idempotency and a transactional outbox, and sends authorized notifications to connected WebSocket clients.

Not found/proven in this mission:
- external report dispatch to an AI/provider recipient;
- durable recipient-level ACK/receipt;
- retry policy and durable retry state;
- idempotent recipient processing;
- ordering/delivery cursor;
- recovery/replay of pending external report deliveries after disconnection.

**Classification:** local report/prompt preparation — IMPLEMENTED; Team Room message persistence/outbox/WebSocket — IMPLEMENTED; end-to-end external report delivery + ACK + retry + recovery — BLOCKED (no verified provider delivery adapter or recipient-ACK pipeline in the audited code).

## 10. Communication between AIs

The evidence distinguishes the layers:
- **Team Room:** authenticated principals/participants, room membership and permission checks, persistent messages, idempotent message writes and outbox events — implemented and integration-tested.
- **Mailbox:** no independent external-agent mailbox with recipient delivery receipts/ACK/retry was demonstrated by this mission's evidence.
- **WebSocket:** authenticated transport and room-scoped fanout to connected test sessions — runtime-tested with 8 distinct test participants.
- **Model/provider integration:** no evidence in this benchmark run that a model provider was invoked as an independent agent and that its generated response was delivered back into Team Room.
- **Effective external AI-to-AI communication:** NOT VALIDATED. Endpoints and transport infrastructure alone do not prove two independently running external agents exchanged and acknowledged messages.

## 11. Baseline comparison and optimization decision

`BASELINE_COMPARISON_NOT_AVAILABLE`

The artifact contains three repetitions of the same workload, which provide within-run repeatability observations, not a pre-change baseline. No comparable pre-optimization run, post-optimization run, or production workload baseline exists. No before/after latency, throughput, error, resource, database or WebSocket improvement can be calculated.

No optimization is implemented in this update. Evidence-backed candidates for a future controlled experiment:
1. Isolate heartbeat-only traffic and correlate each request with outbox insert count, `PRESENCE_CHANGED`, WebSocket sends, DB time and WAL. Investigate the first-run tail spike before changing cadence or semantics.
2. Benchmark unread count at materially larger per-channel message cardinalities; compare the current scan and row-estimate error with candidate indexes/query rewrites using before/after plans.
3. Scale multi-room subscribers and irrelevant events independently to quantify global LISTEN/fanout cost. The current four-room noise test proves no observed leakage but not scaling cost.
4. Retain raw per-request latency samples in future runs so pooled percentiles and histograms can be calculated without reconstructing distributions from summaries.
5. Keep message history and small task/decision/attachment scans unchanged unless larger realistic datasets show a regression.

## 12. Final verdict

`PERFORMANCE_PARTIAL`

Reason: the existing artifact provides real HTTP, WebSocket, limited cross-room isolation, heartbeat-event and PostgreSQL measurements with verified provenance. However, the benchmark is a short isolated CI run; heartbeat was not tested in isolation; multi-room subscribers/scaling were not tested; resource sampling is coarse; no comparable baseline exists; external report ACK/retry/recovery and effective communication between independently running external AIs are not proven. Therefore the evidence is insufficient for `PERFORMANCE_VALIDATED`.

**No benchmark was repeated. No production code, migration, schema, index, or workflow was changed. PR #6 was not merged or closed.**


---

# WORKFLOW CLOSURE — 2026-11-12

Both requested workflows have now completed. No benchmark was rerun as part of this closure, and no production code, migrations, schema, indexes, or workflows were modified.

## CI #748 — run `43330035775`

**Final status: SUCCESS.** All four jobs passed; no failed jobs or failed steps were reported.

| Job | Result | Relevant completed checks |
|---|---|---|
| `ai-team-room-service` | PASS | Independent service check; service unit and PostgreSQL integration tests |
| `keycloak-oidc-smoke` | PASS | Keycloak readiness; realm/client creation; OIDC discovery, client-credentials token and JWKS signature; Team Room service build; real Keycloak token exchange, session, authorization and PostgreSQL admission |
| `foundation-windows` | PASS | Windows BAT smoke test; Rust cargo check/test; npm install; mission tests; benchmark-probe syntax; build |
| `foundation` | PASS | Linux dependencies; Rust cargo check/test; npm install; mission tests; benchmark-probe syntax; build; source ZIP package/upload |

## Runner PostgreSQL #271 — run `43330034918`

**Final status: SUCCESS.** The single job passed; no failed steps were reported.

Job: `PostgreSQL 16 + Team Room + independent Runner processes`.

Completed checks included PostgreSQL 16 readiness, Rust PostgreSQL lease/fencing integration, authenticated PostgreSQL handoff state/authorization/recovery integration, Team Room service build, two independent Mission Runner processes against shared PostgreSQL, proof-gate consolidation, evidence upload, and enforcement of all integration proof gates.

## Failure record

No job failures were reported in either workflow. This is a statement about these two workflow runs only; it does not erase previously recorded failures from earlier attempts.

## Final mission verdict

`PERFORMANCE_PARTIAL`

The successful CI and Runner workflows add confidence in build, platform, OIDC, PostgreSQL lease/fencing, handoff/recovery and independent Runner integration. They do not close the benchmark evidence gaps already documented: no comparable performance baseline; no isolated heartbeat-only load; no multi-room positive subscriber/scaling test; coarse resource sampling; no end-to-end external report delivery with recipient ACK/retry/recovery; and no demonstrated exchange between independently running external model agents. The original benchmark artifact remains the only performance workload evidence in this report. Therefore `PERFORMANCE_VALIDATED` is not justified.

PR #6 remains unmerged and open. No benchmark was repeated and no production code or database structure was changed.
