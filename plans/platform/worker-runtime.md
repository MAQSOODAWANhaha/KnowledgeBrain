# Worker 运行时边界整改

| 项 | 值 |
| --- | --- |
| 状态 | **已实施**（S0–S5；`consume.rs` 已删除） |
| 落地偏差 | 见文末「落地偏差」：`simple_worker!` 保留（已带 timeout）；`AppCtx.pool` 仍为 `Option`；`JobErr` 仍为 newtype |
| 所有者 | Shared Platform（进程、Oxana 注册、停机、helper） |
| 使用方端口 | `knowledge` ingest / semantic index；`bidding` content generate / submission export |
| 已锁定决策 | Housekeep 挂 `LowQueue` 且仍受 env 门闩；删除 question/extract 死 handler；content/export 领域本计划下沉 `bidding` |

## Context

`crates/worker` 作为进程是对的：API 入队，worker 消费；Postgres 是业务真源，Oxana 是 transport。`main.rs` / `probe.rs`、投标 `run_owned_handler`、Unix helper 进程组已经按 [queue-runtime](queue-runtime.md) 落地。没有其它 crate 依赖 worker 的 Rust API（只有 `main.rs` 和若干 `include_str!` 合同测试），内部边界可以一次收干净。

历史问题（实施前）：`src/consume.rs` 约 9934 行，同时堆了三件事：

1. 运行时（deadline、shutdown、staging cleanup、helper 沙箱、7 个 Oxana runtime）
2. 知识库 ingest 编排（`convert_document`、fanout、delete/reparse、与 `knowledge::pipeline` 重复的 semantic-index 调度）
3. 投标 content generate / submission export 领域逻辑（tender / requirement / docx 已经下沉，这两条还留在 worker）

已核对的具体裂缝：

- 知识 `process` 不用 `run_owned_handler`，`convert_document` 不收 `CancellationToken`；`docparser::convert_with` / gRPC 也不可取消。SIGTERM 可能等到 2h。
- `fail_now` 把文档标成 `failed` 后仍对 Oxana 返回 `Err`，违反 queue-runtime「确定性失败 → 业务终态 + envelope `Ok`」。
- `deploy/queue-registry.toml` 与 `run_transport_group` 不一致：`HousekeepWorker` 写了但没挂；`SummaryQueue` 被 ENRICHMENT 与 SHARED 两个 runtime 各消费一次；`QuestionWorker` / `ExtractWorker` 在 `declared_disabled` 下仍是死代码。
- `schedule_semantic_index_v2_if_ready` / `maybe_start_postprocess` 在 worker 与 `knowledge::pipeline` 各一份。
- `process_semantic_index_intent_v2` 只是 `knowledge_index_v2` 的编排层，却放在 worker。
- 多模态 `set_pending_count` / `decr_pending_count` 每次新建空 `HashMap`；Redis `DECR` 失败被当成「做完」。worker 启动已要求 Redis，这条记忆回退是编排漏洞。
- 注册测试靠 `include_str!("consume.rs").contains(...)`；`crates/bidding/tests/bidding_v2_baseline_contract.rs` 也整文件 include 了 `consume.rs`。
- `platform::queue_for` 没有 housekeep 的 `task_type` 常量；cron job 与 registry `system:maintenance-housekeep:v1` 对不上锁。

### 对照当前工作区（本计划以这份代码为准）

已发生、本计划不再当风险或工作项：

- `persist.rs` 已删除，`catalog/` / `identity/` 已存在。
- 进程级 `Store` 已不在：`store.rs` 只剩 DTO；`pending.rs` / worker wiki 测试已不用 `Store::default` / `hydrate_version`。
- `pipeline::run_post_process` 已是 `DocJob::from_pool`，不是全局 Store。
- worker 生产路径本来就没有 `Store::default`。

上述裂缝已在 S0–S5 处理。当前入口是 `knowledge.rs` / `bidding.rs` / `runtime.rs` / `helpers.rs`，没有 `consume.rs`。

目标：worker 只剩 Oxana adapter + 子进程隔离；知识 ingest / semantic index 进 `knowledge`；content / export 领域进 `bidding`；registry、runtime、停机合同三者闭环。新增任务有固定四步，不必再往 god file 里堆。

## Approach

**一个进程、领域下沉到已有 crate、最后再拆 worker 文件。不新增 worker crate，不把 knowledge-worker / bid-worker 拆成两个二进制，不自建队列框架。**

### 锁定决策

1. **Housekeep（A）**：在现有 MAINTENANCE / `LowQueue` runtime 上注册 `HousekeepWorker`。`KNOWLEDGEBRAIN_HOUSEKEEPING_ENABLED` 缺省关；关闭时 cron 仍可能投递，`process` 立即 `Ok`。打开开关不必改注册，但改 env 后仍须重启才能让新进程读到值——保持现有 `process()` 内读取。补 `TYPE_MAINTENANCE_HOUSEKEEP` 与 `queue_for` → `low`。
2. **Question / Extract（A）**：删除 worker 侧 `QuestionWorker` / `ExtractWorker` / `process_questions_pg` / `process_extract_pg`。registry 保持 `declared_disabled`；`enqueue_*` 继续 `rejected:declared-disabled`；`knowledge::pipeline::run_questions` / `run_extract` 保留给以后启用。不注册 `question` / `graph` runtime。
3. **Content / export（A）**：本计划迁入 `bidding`，对标 `tender_process` 与 `docx_composition::runtime`（`ObjectIo` 由 worker helper 实现）。

### 复审后补上的边界

- 进程级 Store 已删除：ingest SQL 与 pipeline `DocJob` 共存，本计划不恢复 Store、不改 `DocJob`。
- semantic index 的 embedding client 在 worker `FromContext` 构造；job 体进 `knowledge_index_v2`。
- 投标 `execute` 不含 Oxana hard timeout；timeout / `WORKER_SHUTDOWN` / export·match terminalize 留 adapter。
- 只删 SHARED，不把 6 个 runtime 再合并。
- `HousekeepJob` 的 oxana 属性不动。
- `pending.rs` Redis 失败改为瞬时错误（fanout 与 `run_image` 同一漏洞）。
- convert 图片 `spawn_blocking` 不随 cancel 抢占。
- S5 更新 tracing 计划中的 `consume.rs` 路径，并按引用收敛 worker 依赖。

### 目标调用形态

```text
Oxana Worker (crates/worker)
  → HandlerDeadline / cancel / helper 进程组 / StagedObjectCleanupTracker
  → knowledge::ingest::run_convert
    | knowledge::pipeline::run_*
    | knowledge::knowledge_index_v2::run_semantic_index_job
    | bidding::content_generate::execute
    | bidding::submission_export::execute
    | bidding::tender_process / tender_analysis / docx_composition（已存在）
  → 确定性失败：写 PG 终态后 Ok
  → 瞬时失败：JobErr::Transient → Oxana Err
  → 停机：JobErr::Shutdown
```

### 职责矩阵

| 事情 | 唯一归属 |
| --- | --- |
| 进程监督、probe、SIGINT/SIGTERM、fatal `root_cancel` | `crates/worker` `main.rs` |
| Oxana runtime 连接、并发、worker 注册 | `crates/worker` `runtime.rs` + `deploy/queue-registry.toml` |
| Job 类型、unique_id、retry 常数、`queue_for` | `crates/platform` |
| object retention / upload expire | `crates/retention`（本计划不改） |
| Unix helper（render / object / pdf raster / `killpg`） | `crates/worker` `helpers.rs` |
| 文档 convert、passage index、fanout、kb delete、reparse | `crates/knowledge` `ingest.rs` |
| postprocess / wiki / summary / image / datatable / list_delete | `crates/knowledge` `pipeline.rs`（已在） |
| semantic index job 体 | `crates/knowledge` `knowledge_index_v2` |
| clone 算法 | `crates/knowledge` `clone`；follow-up **enqueue 留 worker adapter** |
| content generate、证据校验、Agent 循环、lease/yield | `crates/bidding` `content_generate` |
| export 预检、layout、publish | `crates/bidding` `submission_export` |
| export/docx 的对象读写与渲染子进程 | worker 实现 `ObjectIo` / `RenderIo` |
| AgentRun fence、Request 终态 | `bidding` + SQL（搬家不改语义） |
| 多模态 pending 计数 | `knowledge::enrichment`；convert fanout 只走 Redis，DECR 失败当瞬时错误 |

### 目标目录

```text
crates/worker/src/
  lib.rs              薄门面；保留 recursion_limit（Oxana 类型链仍在 runtime）
  main.rs             helper argv 分流 + supervisor（不改结构）
  probe.rs            保持
  runtime.rs          AppCtx, JobErr, owned handler, run_core, transport group
  helpers.rs          四个 --kb-*-helper-v1 + process group
  knowledge.rs        知识 Worker impl：timeout/cancel/retry/enqueue follow-up
  bidding.rs          五个 bid-authoring-v2 Worker impl + Helper* Io

crates/knowledge/src/
  ingest.rs           run_convert / fanout / passage index / kb_delete / reparse
  pipeline.rs         现有 run_*；唯一 schedule_semantic_index / maybe_start_postprocess
  knowledge_index_v2  吸收 process_semantic_index_intent_v2
  catalog/            仅当 delete SQL 需要新助手时追加，不把 SQL 留在 worker

crates/bidding/src/
  content_generate.rs     execute + candidate/evidence；HTTP 仍用 content_runtime.rs
  submission_export.rs    execute；预检/layout/publish；RenderIo + 复用 ObjectIo
```

拆文件放在领域迁出之后，避免先机械切 9934 行再改语义。迁出后 `consume.rs` 只剩 adapter，再拆成上表。

### 运行时合同（知识与投标共用）

- 所有 `Worker::process` 走 `run_owned_handler`：shutdown 优先、hard timeout、cleanup 共用 `hard+30s`。
- 领域 `execute` / `run_*` 接收 `CancellationToken`。
- `docparser::convert_with` / gRPC `read` 增加 cancel；禁止只 `drop` future 而让 DocReader 跑完。
- 知识任务没有 AgentRun：可以用 `ctx.meta.retries` 判断最后一次 transport retry，然后 `fail_now` + **`Ok`**。投标任务继续禁止用 oxana retry 冒充 AgentRun attempt。
- 投标 adapter **保留** per-operation `HandlerDeadline` 与 timeout 后的 terminalize（export 30m、content generate 45m、match_only 10m）。迁入 `bidding` 的是 `execute` 管道，不是把 Oxana 超时策略下沉。
- semantic index：`FromContext` 里构造 `StrictVectorEmbeddingClientV2` **留在 worker**；迁走的是 `process_semantic_index_intent_v2` 的 job 体。
- `AppCtx.pool` 生产路径必有库；类型仍为 `Option<PgPool>`（测试与 FromContext 构造用 `None`）。
- `JobErr(String)` 只活在 worker。领域 crate 返回自己的错误；adapter 映射。`WORKER_SHUTDOWN` / `TRANSIENT_HANDLER:` 前缀承担分类，未再拆 enum。
- 不把 6 个 Oxana runtime 再合成 1 个；只删重复消费 `SummaryQueue` 的 SHARED。CORE 继续同时挂 default + bid 两条队列。
- 不改 `HousekeepJob` 的 oxana attribute（`resurrect = false`、无 unique_id）；只注册 Worker。
- convert 里 `spawn_blocking` 写图片：cancel 最多等当前 write 结束，不把对象 IO 改成可抢占。

### Registry 闭环

`required_enabled` 且 `physical_queue != "retention"` 的 task 必须在 worker `run_transport_group` 注册。retention 仍归 `crates/retention`。

`declared_disabled`（question / extract）禁止出现 Worker impl 与 runtime 注册。

`maintenance_only`（housekeep）必须注册在 `LowQueue`。

同一物理 `SummaryQueue` 只由 **一个** runtime 消费：保留 `runtime_concurrency("ENRICHMENT", 12)`，删除 SHARED runtime。不把 6 和 12 相加。`TRANSPORT_RUNTIME_COUNT` 7→6。`POOL_SHARED` 常量本计划不删。

权威锁：`crates/worker/tests/registry_lock.rs` 解析 `QueueRegistry`，与 `run_transport_group` 旁的静态表比对。禁止再 `include_str!(consume.rs).contains("PostProcessWorker")`。`bidding_v2_baseline_contract.rs` 里对 `consume.rs` 的 include 改为只断言 registry 五条 bid task；注册集合由 worker 测试负责。

### 后续怎么加任务

1. `platform`：Job 类型 + `queue-registry.toml` 一条 + 如需则 `queue_for`。
2. 领域 crate：`run_*` / `execute`（需要对象或渲染时定义 trait，不要调 worker）。
3. `crates/worker`：薄 `Worker` impl（deadline / cancel / Io 实现）。
4. `run_transport_group` 注册，并更新 registry_lock 静态表——表对不上则测试红。

启用 question/extract 时按上面四步走，另开计划；本计划只保证禁用车道没有死 handler。

### 与 knowledge 内部模型的关系

进程级 `Store` **已经不在代码里**（不是本计划要做的事，也不再预留冲突）。合同：

- 不恢复 `Store::default` / `hydrate_version` / `write_back`。
- convert 进新文件 `ingest.rs`，走 SQL（与现在 worker 内 convert 相同）。
- postprocess / wiki / summary 维持现有 `DocJob` 单文档工作集；工作区里 `pipeline.rs` 已有大 diff，S3 **不要再改它的函数体**。

## Files to modify

- `crates/worker/src/consume.rs`：按步骤掏空后删除。
- `crates/worker/src/{lib,main,runtime,helpers,knowledge,bidding}.rs`
- `crates/worker/tests/`：`registry_lock.rs`；helper / launch 保留；从 consume 迁出的合同测试按归属进 knowledge 或 bidding。
- `crates/knowledge/src/ingest.rs`（新）、`pipeline.rs`、`knowledge_index_v2`、`enrichment/pending.rs`（fanout 错误语义）、`lib.rs` 导出。
- `crates/docparser/src/{lib.rs,grpc.rs}`：`convert_with` / `read` 接 `CancellationToken`（不改解析结果）。
- `crates/bidding/src/{lib.rs,content_generate.rs,submission_export.rs}`；`content_runtime.rs` 不改 HTTP 合同。
- `crates/bidding/tests/bidding_v2_baseline_contract.rs`：去掉对 `consume.rs` 的整文件 include。
- `crates/platform/src/{lib.rs,jobs.rs}`：`TYPE_MAINTENANCE_HOUSEKEEP`、`queue_for`。
- `deploy/queue-registry.toml`：housekeep 声明已存在则不改语义；只补与代码对齐的缺口（若 identity/handler 名不一致）。
- `plans/platform/README.md`、`plans/knowledge-base/README.md`（相关链）、`plans/platform/tracing-observability.md`（consume.rs 路径）、`docs/bidding/backend-runbook.md`。
- `crates/worker/Cargo.toml`：S5 按实际引用收敛依赖。

不改：Oxana 版本、retention 进程、DocReader 解析语义、V3 检索、前端、`Store`/`hydrate_*`、`HousekeepJob` oxana 属性、6 个 runtime 拓扑（只删 SHARED）。

## Reuse

- 投标 adapter：`TenderDocumentProcessV2Worker` + `run_owned_handler` + `StagedObjectCleanupTracker`。
- 领域 + Io trait：`bidding::docx_composition::runtime::{execute, ObjectIo}`、`HelperCompositionObjects`、`HelperTenderObjectReader`。
- 知识薄包装：`process_post_process` → `pipeline::run_post_process`。
- 唯一调度：`pipeline::{schedule_semantic_index_v2_if_ready, maybe_start_postprocess}`。
- clone：`knowledge::clone::run_clone` 已返回 follow；worker 只 enqueue。
- catalog：`housekeep_documents`、`purge_document_index`、`set_parse_status`、`try_set_processing` 已在 `knowledge::catalog`。
- content HTTP：`bidding::content_runtime::{turn_once_with, ContentAgentRuntimeContractV1}`。
- export 发布：`bid_authoring_v2::{load_submission_export_input_v2, prepare_submission_export_v2, publish_submission_export_v2, mark_submission_export_failed_v2}`。
- 政策常数：`DOCUMENT_PROCESS_*`、`SEMANTIC_INDEX_V2_*`、`BID_AUTHORING_V2_*`、`HOUSEKEEP_*`、`runtime_concurrency`。
- 测试：`tests/launch.rs`、`object_write_helper.rs`、`render_helper_shutdown.rs`；consume 内 late-write / wiki 幂等 / content verifier 按归属搬家，断言不放宽。

## Steps

每笔提交可独立回滚。S3/S4 禁止夹带 retry/停机语义改动。

- [x] **S0 冻结对照表（本计划即真源，实施时核对代码）**

  | task_type | launch_mode | 物理队列 | 进程 | 实施后 Worker |
  | --- | --- | --- | --- | --- |
  | `document:process` / `manual:process` | required | default | worker CORE | `DocumentProcessWorker` |
  | `bid:tender_document_process:v2` 等五条 | required | bid-authoring-v2 | worker CORE | 现有五个 V2 Worker |
  | `knowledge:post_process` / `knowledge:semantic_index:v2` | required | postprocess | worker POST | 现有 |
  | `summary:generation` / `datatable:summary` | required | summary | worker ENRICHMENT **仅此一个** | Summary / Datatable |
  | `image:multimodal` | required | multimodal | worker | 现有 |
  | `wiki:ingest` / `wiki:finalize` | required | wiki | worker | 现有 |
  | `version:clone` / `kb:delete` / `knowledge:list_delete` / `index:delete` / `knowledge:list_reparse` | required | low | worker MAINTENANCE | 现有 |
  | `system:maintenance-housekeep:v1` | maintenance_only | low | worker MAINTENANCE | **挂上** `HousekeepWorker` |
  | `object:retention` / `object:upload_expire` | required | retention | **retention 进程** | 不在 worker 注册 |
  | `question:generation` / `chunk:extract` | declared_disabled | question / graph | 无 | **删除死 handler** |

  禁止再往 `consume.rs` 加领域函数。

- [x] **S1 合同闭环**  
  删除 SHARED `SummaryQueue` runtime。MAINTENANCE 注册 `HousekeepWorker`。删除 Question/Extract worker 与 worker 内 `process_questions_pg` / `process_extract_pg`（测试改调 `knowledge::pipeline`）。worker 内 `schedule_semantic_index_v2_if_ready` / `maybe_start_postprocess` 改为调用 `knowledge::pipeline`。补 `TYPE_MAINTENANCE_HOUSEKEEP` + `queue_for`。落地 `registry_lock.rs`。**同一提交内**把 `crates/bidding/tests/bidding_v2_baseline_contract.rs` 的 `include_str!("../../worker/src/consume.rs")` 改成只断言 registry 五条 bid task（注册集合改由 `registry_lock.rs` 负责），避免 S5 删 `consume.rs` 时 bidding 误红。行为：housekeep 缺省仍关闭；summary 不再双消费者。S1 可独立回滚；若 summary 积压，用 `KNOWLEDGEBRAIN_ENRICHMENT_CONCURRENCY` 上调，不把 6+12 写回代码。

- [x] **S3 知识领域下沉（不停机语义）**  
  迁前把下面读/写/enqueue 边抄进 `ingest.rs` 顶部注释，迁后用 `rg` 确认 worker 不再出现这些 SQL/enqueue（只剩对 `knowledge::ingest` / `pipeline` 的调用）：
  - 读：`documents ⇔ product_versions`、`parse_status`、`attempt`、`source_passages`、`file_name`、spans
  - 写：`try_set_processing` / `open_attempt` / `set_parse_status` / `set_index_ready` / span start-finish-skip / `persist_passage_index` / kb delete 的 `deleting`/`archived`/`current_version_id`
  - enqueue：`enqueue_image_multimodal`、`enqueue_post_process`、`enqueue_semantic_index_v2`、reparse 的 process/manual/index_delete/datatable、wiki retract
  然后：`convert_document`、`persist_passage_index`、`after_index_fanout`、kb delete / reparse → `knowledge::ingest`。`process_semantic_index_intent_v2` → `knowledge_index_v2`。fanout 只通过带 Redis 连接的 pending API；Redis 不可用或 `DECR` 失败返回瞬时错误，禁止空 `HashMap` 回退把任务标完成。convert 测试迁到 `knowledge` PG suite。worker 知识 Worker 只调 `run_*`。工作区 `pipeline.rs` 已有 persist 拆分 diff：本步只允许删除与 worker 重复的调度副本、`pub use` ingest，**不改** `run_post_process` / wiki / summary 函数体。`pending.rs`：Redis `DECR`/`SET` 失败改为 `Result::Err`（今日仍把 Redis 错误当成 last-decr）；`run_image` 与 ingest fanout 都当瞬时失败。

- [x] **S4 投标领域下沉（不改 fence）**  
  整函数搬移，禁止改分支条件、错误码、lease/budget/yield allowlist。content → `bidding::content_generate::execute`（含 `process_content_generation_v2`、candidate/evidence、`run_content_agent_v1`、`ContentOwned*` / yield）。export → `bidding::submission_export::execute` + `RenderIo`；对象读写复用已有 `ObjectIo`。worker 只留 helper 实现 + `run_owned_handler` + 错误映射。S4 合并门槛：`cargo test -p bidding --test content_agent_run_postgres` 与现有 late-write 断言字节级保持；`git diff -U0` 不得出现 `max_retries` / `claim_content` / `yield_for_retry` / 错误码字符串的语义改动。unit 测试随函数走 bidding；helper 杀树留 worker。

- [x] **S2 统一生命周期**  
  顺序固定，避免「先改返回码、gRPC 还在跑」：
  1. `docparser::grpc::read` / `convert_with` 增加 `&CancellationToken`；流式循环里 `select!` cancel 与下一帧；未取消时行为与现在相同（仍受 `DOCREADER_TIMEOUT`）。
  2. `knowledge::ingest::run_convert` 把 token 传到 convert/ASR/blob。
  3. 知识 Worker 改走 `run_owned_handler`。无超时的旧 `simple_worker!` 已废；现宏带 `HandlerDeadline` + owned handler，覆盖 Datatable / ListDelete / KbDelete / ListReparse / IndexDelete。`SummaryWorker` 因要用 `ctx.meta.retries` 做 fallback，保持手写。
  4. 最后一次 transport retry 且 `fail_now` 成功后对 Oxana 返回 `Ok`（先写测试再改返回）。
  投标 adapter 不改 Request/AgentRun fence。gRPC 测试：cancel 后 `read` 在 `FRAME_IDLE` 之前返回错误，且不得只 `drop` future。launch 测试继续覆盖 SIGTERM。

- [x] **S5 拆文件与清理**  
  删除 `consume.rs`；按目标目录拆剩余 adapter。测试离开生产模块（`adapter_tests.rs`）。export 的 `MAX_*`：表格/表单/资产预算随 `submission_export` 走 bidding；`MAX_RENDER_OUTPUT_BYTES` / helper 输入上限留 worker。清理 worker `Cargo.toml` 中仅 convert/content 用过、adapter 不再需要的直接依赖（`docparser` 若只被 knowledge 调用则可从 worker 去掉，以 `cargo tree -p worker` 为准）。`plans/platform/tracing-observability.md` 里对 `consume.rs` 的路径改成 `runtime.rs` / `knowledge.rs` / `bidding.rs`。文档：backend-runbook 写明 worker 只含 adapter、queue ACK ≠ 业务成功、housekeep env 需重启、retention 不在本进程。

### 明确不做

- 不拆两个 worker 进程，不在 worker 包调度/补偿，不扫 PG pending 补 enqueue。
- 不启用 question/extract。
- **不恢复进程级 `Store` / `hydrate_version` / `write_back`**；enrichment/wiki 维持现有 `DocJob`。
- 不改 DocReader 解析结果、V3 检索、Oxana 2.1.3、retention 清理语义、`HousekeepJob` oxana 属性。
- 不把 helper 子进程下沉到 bidding。
- 不把 6 个 runtime 合成 1 个；不把 6+12 写成 SHARED 并发；不删除未使用的 `POOL_SHARED` 常量。
- 不把 clone follow-up enqueue 放进 `knowledge::clone`（enqueue 留 worker）。
- 不把 Oxana hard timeout / terminalize 下沉进 `bidding::execute`。

### 落地偏差

实施后与原文的三点差异，不再另开计划：

- **`simple_worker!` 保留。** 旧宏无 timeout，那才是必须删的。现宏带 `HandlerDeadline` + `run_owned_handler`，与手写知识 Worker 停机合同相同，只生成 Datatable / ListDelete / KbDelete / ListReparse / IndexDelete。`SummaryWorker` 因 `ctx.meta.retries` fallback 保持手写。五个 adapter 开始分叉（不同 retry / cancel / `fail_now`）时再展开。
- **`AppCtx.pool` 仍为 `Option<PgPool>`。** supervisor 启动仍强制有库；`None` 留给测试与 FromContext。
- **`JobErr` 仍为 `pub struct JobErr(pub String)`。** `WORKER_SHUTDOWN` / `TRANSIENT_HANDLER:` 前缀分类，未拆 Transient/Deterministic/Shutdown enum。

## Verification

- `cargo test -p worker`：launch、helper 杀进程组、late-write、wiki 幂等（若仍属 worker）、**registry_lock**。
- `cargo test -p knowledge`：ingest / postprocess / semantic index / convert 迁入后的 PG 回归。
- `cargo test -p bidding`：content agent / export 合同；tender/docx 零 diff；baseline 不再 include `consume.rs`。
- `cargo test -p docparser`：cancel 打断 gRPC/convert，不改变成功路径输出。
- `cargo check -p api -p worker -p bidding -p knowledge -p retention`。
- registry_lock：`required_enabled \ retention` ∪ housekeep = worker 静态表 = `run_transport_group` 注册；`declared_disabled` 不在 worker src。
- `rg 'fn convert_document|fn process_semantic_index_intent_v2|fn process_content_generation_v2|QuestionWorker|ExtractWorker|SHARED' crates/worker` 仅剩允许的注册/注释。
- `rg 'schedule_semantic_index_v2_if_ready|maybe_start_postprocess' crates/worker` 只有对 `knowledge::pipeline` 的调用。
- 停机：SIGTERM 在 cleanup 30s 内结束；convert 中途观察到 cancel。
- 确定性失败：文档 `failed` 后 Oxana envelope success；瞬时失败仍 retry。
- housekeep：env 关时 cron 投递无业务写入；env 开时调用 `housekeep_documents`。
- 严格 Clippy / fmt 仅覆盖本计划改动 crate。

## 风险与对策

每条都有触发条件、具体做法、验收闸门、失败回滚。闸门不过的提交不算完成。

### 1. S3 convert 读集漏边

**风险：** 这条路径不用 `Store`，但 fanout / span / pending / postprocess 交叉。漏一条 enqueue 或漏写 `index_ready`，文档会停在 `processing`。

**做法：** S3 先把读/写/enqueue 边写进 `ingest.rs` 顶部注释（步骤已列出），再整段搬函数，搬家时不改分支。pending 只走 Redis；`DECR` 失败返回瞬时错误，让 Oxana 重试，而不是当成 multimodal 完成。

**闸门：** `rg 'INSERT INTO chunks|UPDATE documents|enqueue_image_multimodal|enqueue_post_process' crates/worker` 只命中对 `knowledge::` 的调用或测试夹具。迁入前后各跑现有 convert/wiki/postprocess PG 测试，`parse_status` / span 名 / `index_ready` 断言不变。

**回滚：** 只回 S3。ingest 未接线则 worker 仍有旧函数。

### 2. S2 停机变成假取消

**风险：** 现在 SIGTERM 可能等 2h。若只 `drop` `convert_with` future，DocReader gRPC 仍跑到 `DOCREADER_TIMEOUT` / `FRAME_IDLE`。

**做法：** 固定顺序见 S2：先给 `grpc::read` 的 stream 循环加 `select! { cancel, next_frame }`，再传到 `run_convert`，最后 Worker 才走 `run_owned_handler`。`read` 返回后 drop tonic 客户端以结束 HTTP/2 流。未取消路径保持原 timeout。

**闸门：** docparser 单测：发 cancel 后 `read` 在 `FRAME_IDLE`（120s）之前返回；成功路径 markdown 不变。worker launch：SIGTERM 后进程在 30s cleanup 内退出。禁止用「睡满超时」当通过。

**回滚：** docparser 加参在未取消时与现在等价，可单独回。

### 3. S4 搬破 AgentRun fence

**风险：** content 约 2000 行含 lease / 三次 physical call / yield allowlist。搬家时改一个 `if` 就会让重复 envelope 再打模型。

**做法：** 整函数剪切到 `content_generate.rs` / `submission_export.rs`，worker 只 `execute` + 映射错误。不允许顺手改错误码或 `RetryDisposition`。

**闸门：** `cargo test -p bidding --test content_agent_run_postgres` 全过；worker late-write 全过；对该提交 diff 搜索 `claim_content`、`yield_for_retry`、`AGENT_TURN_TIMEOUT`、`max_retries` 无语义 hunk。有则拆出 S4。

**回滚：** S4 单提交。tender/docx 本步零 diff，可作对照。

### 4. fail_now 后仍 Err，改 Ok 会改 Oxana 命运

**风险：** 今天最后一次 retry 已把文档标 `failed`，仍 `Err` 给 Oxana，可能 dead-letter / 再投。改 `Ok` 是合同修复，也是行为变化。

**做法：** S2 先写测试再改代码。adapter 抽 `finish_knowledge_job`：未达 max retry → `Transient`；达 max 且 `fail_now` 成功且 status≠completed → `Ok`；`fail_now` 失败 → 仍 `Transient`。不把瞬时错误写成 `failed`。

**闸门：** 单测覆盖上述三分支。PG：文档 `failed` 后同一 unique_id 不再重复 convert（`on_conflict=Skip` + 不再 `Err` 表示 envelope 结束）。

**回滚：** 只回 S2 里这个 helper；cancel 接线可留。

### 5. 合并 SummaryQueue 后并发从最多 18 变成 12

**风险：** 两个 runtime 同时消费同一物理队列时，Oxana 并发是每 runtime 一份（12+6）。删 SHARED 后默认 12，summary 可能更慢，不是静默丢任务。

**做法：** 故意回到声明的 `ENRICHMENT` 默认值。积压用已有 `KNOWLEDGEBRAIN_ENRICHMENT_CONCURRENCY` 上调，不在代码写 18，不把 SHARED 加回来。S1 提交说明写明这条 env。

**闸门：** `registry_lock` 断言 `SummaryQueue` 只出现一次；`run_transport_group` 里 `rg SHARED` 为空。不把 summary 延迟当 S1 失败条件。

**回滚：** 整份回 S1。只调 env 不需要回滚代码。

### 6. Housekeep 关闭时 cron 仍投递

**风险：** 注册后 Oxana 每 5 分钟仍可能 enqueue；env 关时必须零业务写入。按 env 决定是否注册会让 `registry_lock` 与运行进程分叉。

**做法：** 始终注册 `HousekeepWorker`。`process` 开头 `housekeep_enabled()` 为假则立即 `Ok`，不碰 PG。`HousekeepJob.resurrect = false` 保持。打开开关必须重启 worker（与现有启动读 env 一致，写进 runbook）。

**闸门：** env 关时 `housekeep_documents` 零 UPDATE；env 开时才更新 stale `processing` 行。`registry_lock` 含 housekeep，与 env 无关。

**回滚：** 从 `LowQueue` 去掉该 worker 即回到今天「开关无效」；S1 可整提交回滚。

### 7. pending 计数 Redis 失败语义

**风险：** 今日 `decr_pending` 在 Redis 错误时返回「做完」，fanout 与 `run_image` 都会提前进 postprocess。改成 `Result::Err` 会影响 image 路径，这是同一漏洞的修复，不是无关重构。

**做法：** S3 与 ingest 同提交改 `pending.rs`；`run_image` 对 Err 当瞬时失败（Oxana retry），不把 Err 当成 last-decr。`redis_namespace.rs` 补 Redis 失败用例。

**闸门：** 无 Redis 或 DECR 失败时不出现 `parse_status=completed` / 不 `enqueue_post_process`。有 Redis 时原 last-decr 仍触发 postprocess。

**回滚：** 只回 `pending.rs` + 调用方映射，不影响 convert 搬家。

### 8. bidding 合同测试整文件 include consume.rs

**风险：** S5 删除 `consume.rs` 会使 `bidding_v2_baseline_contract.rs` 编译失败，看起来像 bidding 回退。

**做法：** S1 同一提交去掉该 `include_str!`。bid 五条 task 由 `platform` `QueueRegistry` 测试覆盖；worker 是否注册由 `crates/worker/tests/registry_lock.rs` 覆盖。bidding 测试不再依赖 worker 源码路径。

**闸门：** S1 后 `rg include_str!.*consume.rs` 为空。`cargo test -p bidding --test bidding_v2_baseline_contract` 与 `cargo test -p worker --test registry_lock` 均过。S5 删文件时 bidding 不必改。

**回滚：** S1 整提交回滚会把 include 带回来，与恢复 SHARED runtime 一起。
