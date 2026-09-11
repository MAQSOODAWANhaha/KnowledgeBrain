# 共享队列运行时方案

| 项 | 值 |
| --- | --- |
| 状态 | **方案已确认；普通实施与隔离开发验证已授权，任务见[实施台账](../implementation-tasks.md)** |
| 所有者 | Shared Platform |
| 发布依赖 | crates.io `oxana = "=2.1.3"`、`oxana-web = "=2.1.3"` |
| 消费方 | 招投标异步任务、retention、知识库现有任务 |

本文定义 Oxana transport 与 PostgreSQL 业务状态的边界。**以简单复用为默认：优先配置并直接调用 Oxana 的公开能力，我方只保留 typed job 注册/输入校验、业务处理及必要的业务幂等和副作用围栏。** 不再包一层调度、重试、补偿或队列状态管理框架；AgentRun fence 不能替代或复制 queue transport。已有实现和本地验证记录仅为现状，不代表整体运行验收；已确认方案的授权边界与执行状态见[实施台账](../implementation-tasks.md)。

## 1. Authority 与核心决策

Oxana 2.1.3 唯一拥有：

- enqueue、scheduled job、并发消费；
- transport retry、retry delay/counter；
- Worker process heartbeat、失联检测和 in-flight resurrection；
- unique conflict、dead queue 与 operator tooling。

PostgreSQL 业务领域唯一拥有：

- durable business Request/target、frozen input 与 result；
- current revision、CAS、immutable artifact 与 terminal transaction；
- 对需要多次外部 Agent 边界的 Request，使用 AgentRun token/lease 限制业务副作用。

AgentRun attempt/lease 不表示 Oxana queued/processing/retrying/dead phase。PostgreSQL 不扫描、claim、lease、retry 或 dead-letter transport work。

## 2. 依赖与版本

- `Cargo.toml` 精确锁定 `=2.1.3`；
- `Cargo.lock` 是 version/source/checksum 真源；
- build/test/CI 使用 `cargo --locked`；
- 不 fork/vendor/Git/path dependency，不使用 `[patch.crates-io]` 或 source replacement；
- 升级 Oxana 必须单独评审并重跑本文 fault matrix。

## 3. Transport/业务职责矩阵

| 能力 | 唯一所有者 |
| --- | --- |
| enqueue、retry、delay、transport attempt | Oxana |
| Worker process crash resurrection | Oxana |
| unique conflict/dead queue | Oxana |
| Request 输入/业务状态/结果 | PostgreSQL domain |
| external Agent effect owner | PostgreSQL AgentRun token/lease |
| physical Agent call budget | PostgreSQL logical-boundary ledger |
| result/current pointer atomic publish | PostgreSQL domain transaction |
| enqueue/replay/reconnect/retry/dead recovery | Oxana native queue |

明确禁止：

- 自建 queue membership、retry schedule、dead queue 或 process resurrection；
- 把 Oxana retry counter 当作 DB AgentRun attempt；
- 读取或修改 `oxanus:*` 私有 key；
- 用 queue list/stats 判定业务成功；
- PostgreSQL 保存 queued/processing/retrying/dead；
- 通用 outbox/dispatcher/transport exactly-once 框架；
- PostgreSQL queue/outbox/pending-op claim、lease、retry 或 dead-letter 表和函数。

明确允许且要求：

- Agent-bound Request 的 DB-generated attempt/token/lease；
- attempt-independent physical-call reservation；
- exact Request/current/terminal fence；
- 用户显式 HTTP replay 重新加载同一冻结 payload 并调用 pinned Oxana 2.1.3 官方 `enqueue` API。

## 4. Stable typed job

现有招投标注册使用四类粗粒度 typed jobs；每类 unique identity 都由 frozen request identity 导出：

```text
bid:tender_document_process:v2
bid:requirement_set_compile:v2
bid:content_generate:v2
bid:submission_export:v2
```

共享属性：

```text
receipt deployment_namespace_id = canonical UUID
Oxana StorageBuilder namespace = kb:<32 lowercase hex derived from deployment_namespace_id>
physical Redis key prefix = kb:<32 lowercase hex>:
unique_id = <job-kind>:<request_artifact_id>:<request_revision>
on_conflict = Skip
resurrect = true
payload = frozen request identity only
```

Payload 不携带大对象、结果、queue phase 或可漂移 snapshot。Worker 从 PostgreSQL 读取冻结业务输入。

连接入口复用 typed `DeploymentNamespaceV1` 构造官方 Oxana Storage；`REDIS_URL` 和 `KB_DEPLOYMENT_NAMESPACE_ID` 都必须显式配置，缺失时不生成默认部署、不连接默认本机端口。旧 raw UUID 前缀不自动读取或重放；已有安装需要独立的停机与数据处置安排，不能混跑不同前缀的 producer/consumer。源码及专属 Redis 实测记录见[namespace 隔离](../../docs/platform/namespace-isolation.md#redisoxana-与多模态计数器隔离补充)。不修改 Oxana 私有键结构或增加调度层。

清理任务是上述 transport unique/Skip 的明确例外：ObjectUploadExpireJob 携带 `staging_id`，ObjectRetentionJob 的 payload 固定包含 `deletion_id/object_ref/digest/byte_length`，PostgreSQL 仅保存不可变 deletion business identity 与完成 tombstone。Oxana 2.1.3 的 Skip 会返回既有 JobId，无法作为本次交接确认，因此仅这两类 job 不设 transport unique_id，使用 Oxana 原生独立 JobId；重复 envelope 由数据库业务身份/receipt 幂等收敛，原生 retry/resurrection 不变，不自建交接计数器或状态层。Knowledge Wiki 使用 `wiki:ingest:{product_version_id}:{document_id}:{operation}` 和 `wiki:finalize:{product_version_id}:{document_id}` typed jobs；页面/目录/日志与文档终态属于 PostgreSQL business state，但不存在 pending-op/dead-letter transport table。

`Skip` 只用于减少重复 envelope，不能证明现有 job 仍可执行，也不能作为 delivery receipt。

## 5. Request commit 与 enqueue

PostgreSQL 与 Redis 无跨库事务，且 PostgreSQL 只保存业务真源，不充当 transport backlog、outbox 或 poller：

1. 业务事务幂等创建或取得同一个 frozen Request 后 commit；
2. API 从该 Request 的 immutable `request_payload` 加载并 exact 校验 closed `BidAuthoringJobPayloadV2`，随后直接调用 pinned Oxana 2.1.3 官方 `enqueue`；
3. enqueue 成功返回 `202`；Redis unavailable/unknown 返回 `503 QUEUE_UNAVAILABLE`，携带同一 Request identity 与 `retry_same_idempotency_key=true`；
4. 同 key replay 可再次调用官方 `enqueue`；唯一 `Skip` identity 限制重复执行。Oxana 2.1.3 的 existence check 与写入不是一个 Redis 原子命令，故并发 enqueue/unknown-response 有一个已知窄窗口；本项目不 fork Oxana、不暴露 Redis keys，也不以 PostgreSQL queue/outbox 补偿；
5. 不在 Request row 保存 enqueue reservation、queue phase、delivery generation、`next_enqueue_at` 或自动 recovery budget；不从 PostgreSQL 扫描 pending Request 重建 transport envelope；
6. 普通同幂等键 HTTP replay 即为恢复入口；不另设 reservation、operator budget 或后台 redispatch loop。

`request_payload`/`request_sha256` 是精确 closed tagged `BidAuthoringJobPayloadV2`，不是 HTTP mutation body 或 Oxana-native `JobData.args`。Worker 只把 PostgreSQL 用于 Request/AgentRun/stage/result 等业务持久化。

## 6. Oxana-native delivery recovery

- enqueue、queue uniqueness、transport retry、delayed retry、dead/process resurrection 与 Redis reconnect 均由 Oxana 原生实现负责；
- Worker supervisor 只 cancel/join Oxana runtime tasks 和业务 handler，不运行 PostgreSQL delivery reconciler；
- 禁止 `pending Request` scan → enqueue，禁止 request-scoped automatic reservation，禁止把 PostgreSQL 变成另一个 queue；
- process crash 时，Oxana 恢复 transport delivery；Agent-bound Request 的 DB owner 仍只由 frozen lease/attempt/token fence 管理；
- 用户显式 HTTP replay 可再次直接 enqueue 同一冻结 payload；浏览器后台不得形成独立 mutation retry loop。

### 6.1 Frozen handler deadlines

所有 deadline 从 handler `process` 入口开始，覆盖 terminal preflight、Request/AgentRun claim、业务 pipeline 与 publication；cleanup 使用同一个绝对 `hard deadline + 30s`，不能在阶段之间重新获得 30s。

| Request kind | Agent-bound | handler hard timeout |
| --- | --- | ---: |
| `TenderDocumentProcess` | no | 30m |
| `RequirementSetCompile` | yes | 45m |
| `ContentGenerate(match_only)` | no | 10m |
| `ContentGenerate(generate)` | yes | 45m |
| `SubmissionExport` | no | 30m |

## 7. Agent-bound execution fence

对招标分析及内容生成等显式声明的 Agent-bound Request，AgentRun per attempt 保存：

```text
status = running|retry_yielded|superseded|succeeded|failed
execution_owner_token
lease_acquired_at
lease_expires_at
heartbeat_at
hard_deadline_at
```

DB claim 在 Request row lock 下生成 attempt/token。Oxana retry metadata 不进入该 identity。

所有 business-effect 操作必须先锁 immutable Request row、再锁 current AgentRun row，之后才读取一次数据库 `clock_timestamp()`；禁止在等待任一 row lock 前缓存时间。锁后必须验证：

```text
request + frozen input + attempt + token
AgentRun.status = running
lease_expires_at > clock_timestamp()
hard_deadline_at > clock_timestamp()
```

Lease 只限制外部副作用 owner，不限制 Oxana 是否投递/重试 envelope。

### 7.1 Frozen timing

```text
heartbeat interval = 5s
max jitter = 1s
soft lease = 30s
single Agent HTTP timeout = 180s
handler hard timeout = 45m
cleanup margin = 30s
immutable hard deadline = claim + 46m
max run attempts = 4
```

Heartbeat 不能复活过期 lease，也不能移动 hard deadline；尤其 Request/AgentRun lock contention 跨过 expiry 时，锁后时钟必须拒绝 heartbeat/effect，而不是使用等待前时间复活 owner。

### 7.2 Retry yield

`yield_for_retry` 的 closed typed allowlist 只有 `INTERNAL`。SQL 必须拒绝 `AGENT_PROVIDER_UNAVAILABLE`、`AGENT_TURN_TIMEOUT` 和任意 unknown code；Rust 调用面必须使用不能表达其它值的 closed enum。Provider unavailable/turn timeout 只在同一 logical boundary 的 attempt-independent 三次 physical-call ledger 内重试，耗尽后以原 code 原子 terminalize Request+AgentRun，绝不能 yield、创建新 DB attempt 或重置 ledger。

Retryable handler 返回 Oxana `Err` 前必须：

1. cancellation-safe cancel/join provider work 和 heartbeat；
2. token/attempt/live-lease fenced 记录 typed transient error；
3. AgentRun → `retry_yielded`；
4. 立即到期 lease；
5. Request 保持 pending；
6. 返回 `Err`，由 Oxana 决定 transport retry。

进程崩溃不能 yield 时，DB owner 只由 lease 自然过期；Oxana process resurrection 仍由 Oxana 负责。

### 7.3 Physical call budget

Physical Agent call 在 HTTP 前由 logical-boundary ledger reserve：

```text
request + frozen input + stage + batch + input SHA
+ prompt/schema/agent/model identity + call ordinal
```

不包含 execution attempt。最多 3 个成功 reservation；reservation 后崩溃仍消费 slot。

## 8. Handler outcome

| 业务结果 | PostgreSQL | 返回 Oxana |
| --- | --- | --- |
| success | owner/current fenced 原子 publish | `Ok` |
| stale/terminal/duplicate | no business mutation | `Ok` |
| deterministic failure | Request+AgentRun 原子 failed | `Ok` |
| retryable transient | `yield_for_retry` | `Err` |
| live owner duplicate | zero external calls | `Ok` |

Queue `Job finished success=true` 只表示 envelope 已被确认，不表示业务 Request succeeded。

### 8.1 AgentRun 诊断字节边界

**保留的实现增量已修复五 writer，且此前在隔离 PostgreSQL 16 执行过真实 procedure 回归；当前阶段 A 不重跑测试，后续按实施台账复核；这不是生产部署或整体 CI 已绿的声明。** `migrations/bidding_v2_baseline.sql` 的 `kb_bid_v2_diagnostic_prefix` 统一限制 outline terminal/progress/retry、Content(generate) terminal/retry 的诊断。原 `left(message,8192)` 按字符截断，与 AgentRun 的 `octet_length<=8192` CHECK 冲突；原 baseline 的 Outline/Content terminal 均已用3000个“中”（9000 UTF-8字节）实际复现 CHECK 失败，修后可原子提交。上游 Rust 仍原样绑定，SQL 是最终防线。按 [未发布 baseline 策略](runtime-foundation.md#2-fresh-baseline)直接修所属 baseline，不增加全平台错误框架。

- 上述五条写入路径共用一个 UTF-8 安全前缀逻辑：结果不超过8192字节、不切断多字节字符，短消息不变；保留各路径现有 NULL/空串约定。SQL 是最终防线，Rust 提前处理不能替代数据库保证。
- **仅 Outline terminal、yield-retry 和 retrying progress 的字符串消息**保证 `progress_detail.last_error_message` 与独立列使用同一受限结果。Outline retrying progress 的非字符串 JSON 保留原值，列对原 `->>` 文本限长，不承诺原 JSON 也已截断或与列逐字相同；非 retrying progress 的 detail 与诊断列保持原行为，不承诺镜像一致。Content 当前不含该镜像的 JSON 不新增字段，普通 Content progress 不写此列，不凭名称扩入本修复。其他 JSON 字段沿原合同；诊断不得携带 secret 或整份文档。冻结错误码不截断、不归一化，非法码仍拒绝。
- NULL 沿原行为：合法 terminal/yield-retry 将 NULL 消息转为空串。Outline **retrying progress** 的 absent/JSON null 消息，配合法码或缺失/NULL码，原来即违反 code/message/time 同步 CHECK；回归断言 SQLSTATE 23514 与 Request+AgentRun 零写入，不放宽约束。非 retrying progress 仍可保存含这些值的 detail，且不改诊断列。此兼容拒绝不是残留字节 P1。
- 保留各自 owner/attempt/token/lease/deadline 围栏、调用预算和原子 Request+AgentRun 终态；截断只限制诊断，不将实际失败伪装成成功。retry 保持 Request pending 与既有 yield 语义；Content(match_only) 不创建 AgentRun，不改变其无 owner 分支。
- `crates/bidding/tests/tender_analysis_postgres.rs` 与 `crates/bidding/tests/content_agent_run_postgres.rs` 已补并本地执行真实 procedure 回归：ASCII 的8191/8192/8193字节，中文、emoji及混合消息在8192前后，短/空/NULL消息；五路径均核对字节长度、合法 UTF-8、已有诊断镜像与原状态语义。合法 terminal 错误须原子提交，非法或过期 owner 须零写入，Content match_only 分支保持不变。静态反例或字符串匹配不代替 SQL 执行，必跑接线见 [平台 §7.1](runtime-foundation.md#71-聚焦数据库-gate)。

## 9. Lifecycle 与资源

- Worker supervisor 只管理 Oxana runtimes 与 business handlers；Redis reconnect/retry/resurrection 由 Oxana native runtime 负责；
- external SIGINT/SIGTERM token 与 process-local fatal cancellation 必须分离；fatal child 必须导致 nonzero process exit；
- handler 的 timeout/shutdown/lease-loss 必须 cancel/join provider future、heartbeat、helper process 和 subprocess tree；
- completion 后 cleanup 共享 `min(completion+30s, hard+30s)` 的同一绝对 deadline，并传播至 terminal write、tracker 与 supervisor；abort 后不得在该 deadline 外继续等待；
- 未发布 object staging 先以携带 `staging_id` 的非 unique typed upload-expire job 调用官方 enqueue（§4 的清理例外）；只有拿到本次 `Some(job_id)` 确认才 disarm cleanup tracker，这只表示交接确认，不表示 staging 或 blob 已删除。Redis unavailable/结果未知时保留 staging 与可恢复身份并显式失败；清理消费、最终回收及已有 staging 到期业务处理统一见 [平台 §6](runtime-foundation.md#6-retention-consumer)。投递、重试、延迟、去重与崩溃恢复只复用 Oxana，不增加 scanner/outbox 或额外清理编排层；
- Redis 使用 deployment namespace；AOF/managed durability 应配置并 crash-test；
- PostgreSQL 不运行 delivery poll/re-enqueue；测试结束清理当前 namespace/container/network，不删除其它 deployment 数据。

## 10. 验收

1. `cargo metadata --locked` 证明 Oxana 2.1.3 来源与 checksum；
2. Oxana retry/delay/process resurrection/dead queue/Redis reconnect 使用原生实现；
3. DB attempt 不读取 `ctx.meta.retries`；same-attempt duplicate 只有一个 owner；
4. expired owner 在 replacement 前也不能 heartbeat/reserve/stage/publish/fail；
5. transient yield 后 transport retry 获得新 DB attempt；crash 只在 lease expiry 后 supersede；
6. six handler deadlines 从 process entry 起算，shutdown 分支 biased，cleanup 共用唯一 absolute deadline，且无 late write；
7. four concurrent boundary claims 最多三个 reservation；
8. API commit 后直接调用官方 `enqueue`；queue unavailable 的同-key replay 保持同一 Request identity；不存在 PostgreSQL pending scan/reconciler/automatic re-enqueue；
9. fatal runtime/probe child 令 Worker nonzero 退出；SIGTERM/SIGINT 正常退出并 consume/join 所有 handles；
10. Tender source read、VLM、render helper 和 PDF raster subprocess tree 都可取消、kill/reap，且 oversized output 在 helper 完成前被终止；
11. duplicate/revived envelope 不能重复 Agent calls 或 publish；
12. queue tests 只证明 transport；业务 terminal/current/call budget 由 SQL concurrency tests 证明；清理按 [平台 §6](runtime-foundation.md#6-retention-consumer) 分层验收，不能以 enqueue 或固定 sleep 代替真实 consumer 和最终业务结果。

旧大纲生成链撤除后，诊断回归迁至 `tender_analysis_postgres::analysis_diagnostics_preserve_utf8_and_reject_foreign_owners`：新 Agent 的失败与重试两条写入路径验证同一 8192 字节边界、中文/emoji、NULL 行为和错误 owner 零写入；原 Content 路径继续由 `content_agent_run_postgres` 验证。新 Agent 的读取/复核与发布测试独立于这些诊断用例。
