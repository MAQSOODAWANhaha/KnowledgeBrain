# 招投标异步任务与恢复方案

| 项 | 值 |
| --- | --- |
| 状态 | 简化方案已批准并固化，待实施与验收 |
| 所有者 | Bidding |
| 队列依赖 | [`../platform/queue-runtime.md`](../platform/queue-runtime.md) |

本文定义招投标异步业务的最小 PostgreSQL 正确性。Oxana 负责 enqueue、retry、retry delay、并发消费和 worker crash resurrection；本文不复制这些能力。

## 1. 目标

- 业务 target 与产生它的 mutation 在同一 PostgreSQL 事务提交；
- API 成功不依赖 Redis 当时在线；
- duplicate、旧 generation、worker crash 和 lease lost 不会重复发布业务结果；
- Redis volume 完全丢失后，未完成 target 可从 PostgreSQL 重新投递；
- conversion、extraction、matching schedule/job、attachment preparation 和 submission render 使用同一套小流程；
- 删除旧 `system:live-recovery:v1`、best-effort commit 后 enqueue 和泛化 orphan 扫描。

非目标：通用工作流引擎、第二套队列、任意 DAG、队列 membership 镜像、exactly-once transport。

## 2. 最小模型

不新增 `dispatch head`、`dispatch intent`、`delivery attempt`、`settlement`、`successor` 或 `repair obligation` 表。六类现有业务 target 继续是唯一业务真源，只补齐一致的投递字段：

```text
status                 pending | running | completed | failed | cancelled | superseded
delivery_generation    bigint >= 0
next_enqueue_at        timestamptz
attempt_count          integer
max_attempts           integer
claim_token            uuid nullable
claim_lease_ms          integer
heartbeat_at           timestamptz nullable
result/artifact pointer nullable
error_code              nullable bounded text
error_detail            nullable bounded text
```

现有独立 attempt 表可以在业务审计确实需要时保留，但只记录业务执行 attempt；不得保存 Oxana queued/retrying/dead phase，也不得控制队列 retry。

约束：

- target 创建时 `status=pending, delivery_generation=0, next_enqueue_at=now()`；
- terminal target 没有 claim，且不再进入 due scan；
- `running` 必须有 `claim_token + heartbeat_at`；非 running 不得保留有效 claim；
- `attempt_count <= max_attempts`；当前 attempt 失败且已达到上限时，必须立即结算为 `failed`；
- current pointer、source generation/watermark 和 artifact publish 继续使用各业务模块已有 FK/CAS；
- target、audit、幂等 receipt 与初始 due intent 在同一业务事务完成，不在 commit 后补写。

## 3. 唯一流程

```text
业务事务创建 pending target
        |
        v
worker 内轻量 reconciler 扫描 due target
        |
        | reserve 新 delivery_generation
        v
Oxana 2.1.3 enqueue(target_id, generation)
        |
        | 原生 retry / delay / resurrect / concurrency
        v
handler 从 PostgreSQL claim
        |
        | heartbeat + 外部工作
        v
fenced transaction 原子 publish 或 fail
```

### 3.1 Due reconciler

reconciler 是 Redis 完全丢失时的最后兜底，不是第二套队列：

1. 每 30 秒从六类 target 中各取固定上限的 due row，使用 `FOR UPDATE SKIP LOCKED`；
2. 对 `pending AND next_enqueue_at <= now()` 的 row 原子增加 `delivery_generation`，并把 `next_enqueue_at` 推迟 5 分钟；
3. transaction commit 后调用一次平台 `enqueue(target_id,generation)`；
4. enqueue 返回错误或结果未知时不查询 Redis，也不建立 delivery attempt；target 到期后会产生下一 generation；
5. 对 lease 已过期的 `running` row，先精确终结旧业务 attempt、清 claim并回到 `pending`；达到 `max_attempts` 则直接 `failed`；
6. `completed|failed|cancelled|superseded` 永不重新投递。

reconciler 与 Oxana consumer 运行在现有 worker 进程中，复用 worker 的数据库连接和受检函数。V1 不新增 `bid-dispatcher` service、login role、DSN、activation hold 或独立控制面。

### 3.2 Handler claim

job payload 只有 `target_id + delivery_generation`。handler 在任何对象存储、DocReader、provider 或 renderer I/O 前执行一次 claim transaction：

- target 不存在、已 terminal 或 job generation 小于当前 generation：`Ok` noop；
- job generation 大于当前 generation：记录 bounded invariant error并 `Ok`，不无限 retry；
- 同 generation 已由未过期 owner 运行：duplicate `Ok` noop；
- 同 generation 的旧 owner lease 已过期：在同一 transaction 精确终结旧 attempt；未耗尽时由新 token接管，已耗尽时置 `failed`；
- target 为 current pending 且未耗尽 attempt：增加 `attempt_count`，写入新 `claim_token`、`heartbeat_at` 并进入 `running`；
- project ended、source/current fence 已失效：原子 `cancelled|superseded` 后 `Ok`。

同一 generation 的 duplicate delivery 不创建第二个业务 owner。

### 3.3 Heartbeat 与长任务

- heartbeat 间隔不大于 `claim_lease_ms / 3`；
- heartbeat 只在 `status=running AND generation/token` 均匹配且 lease 未过期时续租；
- 外部转换、检索和完整 render 期间由独立 background heartbeat 持续续租；
- handler 的所有外部子进程和 heartbeat 归属同一 task scope；
- 正常、错误、timeout、panic 或 shutdown 都必须停止 heartbeat并回收子进程；
- heartbeat 失败或 lease 过期后，本 owner 永远不能发布。

### 3.4 结算

所有 post-claim 路径都必须进入统一结算：

| 结果 | 数据库动作 | 返回 Oxana |
| --- | --- | --- |
| 成功 | 同一 transaction 验证 generation/token/lease/current fence，发布 artifact/current pointer并置 `completed` | `Ok` |
| 可重试错误 | 终结本次业务 attempt，清 claim，置 `pending`；达到业务上限则 `failed` | 未耗尽时 `Err`，耗尽时 `Ok` |
| 确定性错误 | 终结 attempt并置 `failed` | `Ok` |
| cancelled/superseded | 清 claim并写 terminal 状态 | `Ok` |
| lease lost | 不发布、不覆盖新 owner | `Ok` |

conversion 完成并产生 extraction target、matching schedule 产生 0..N matching job、attachment pages 完成以及 render output 发布，都必须在各自成功 transaction 中原子完成。不能先把父 target 标为 completed，再在第二次数据库调用中创建后继 target。

## 4. Oxana 与 PostgreSQL 如何配合

| 场景 | 收敛方式 |
| --- | --- |
| handler 返回可重试错误 | Oxana 对同 generation 原生 retry |
| worker 进程崩溃 | Oxana重新投递；旧DB lease过期后同generation接管，或由reconciler创建新generation |
| duplicate membership | DB generation/claim/terminal 检查后 noop |
| enqueue response lost | 可能 duplicate；DB fencing 保证有效发布一次 |
| Oxana retry 耗尽 | target 仍非 terminal，5 分钟到期后新 generation |
| Redis flush/volume 丢失 | due target 新 generation 重新 enqueue |
| 旧 job 延迟到达 | generation 小于 current，noop |
| worker 在 lease 后恢复 | publish CAS 失败，noop |

这套设计只保证“业务最终可恢复且有效发布至多一次”，不声称 transport exactly once。

## 5. 并发与幂等

- due reservation、claim、heartbeat、fail 和 publish 都使用 generation/token CAS；
- Redis I/O 不放在 PostgreSQL transaction 内；
- enqueue job ID 由 `target_id:generation` 确定，重复调用使用 Oxana `Skip`；
- 业务发布使用已有 unique key/current pointer CAS/ObjectRegistry owner reference，重复 bytes 不等于重复业务发布；
- matching 0..N fanout 在一个 transaction 内创建全部 matching target；零 route 是成功空 fanout；
- 同 target 不允许同时由旧 recovery 和新 handler 驱动，切换时删除旧 owner，不双写。

## 6. 需要删除

| 旧内容 | 处理 |
| --- | --- |
| `system:live-recovery:v1` envelope、claim ledger、handler | 删除 |
| commit 后 `enqueue_bid_*` best-effort 调用 | 删除 |
| dirty-manifest/orphan-target/orphan-match 泛化 recovery UNION | 删除 |
| Bid 对 `oxanus:*`、hostname/PID replay 的 correctness 依赖 | 删除 |
| 新草稿中的 async base/typed extension/head/intent/state | 不实施；已产生的未用代码删除 |
| delivery attempt/observation/settlement/inbound/repair obligation/rejected delivery | 不实施；已产生的未用代码删除 |
| successor/gate/governor/activation hold/独立 dispatcher service | 不实施；已产生的未用代码删除 |

只删除被本方案替代的招投标路径；知识库仍在使用的队列代码不因本切换顺手重构。

## 7. 实施顺序

1. 固化 [`../platform/queue-runtime.md`](../platform/queue-runtime.md) 的 Oxana 2.1.3 合同；
2. 为六类现有 target 补齐统一 delivery 字段和受检 due/claim/heartbeat/settle 函数；
3. 在现有 worker 中加入 bounded reconciler 和一个 `BidDeliveryV1Job` handler；
4. 逐类切换 conversion/extraction、matching、attachment/render，并在同一改动删除旧 enqueue/recovery owner；
5. 删除所有剩余 Bid live-recovery、private Redis correctness 和未采用的复杂 dispatch 草稿；
6. 从空库和空 Redis/object volume 执行最终验收。

## 8. 验收矩阵

必须覆盖以下关键行为，不建立庞大的内部状态证据协议：

1. target 与业务 mutation 同事务：commit 全可见，rollback 全不可见；
2. retryable handler error 实际由 Oxana 重试，数据库不调度同 generation 的 retry；
3. worker crash 后 Oxana resurrection 能继续处理；
4. duplicate delivery 只有一个 claim owner和一个有效业务发布；
5. stale generation noop；
6. lease lost owner不能 publish，新 owner可以完成；
7. 任意 post-claim 错误都结算，不遗留永久 running；
8. conversion→extraction 与 matching 0..N fanout 原子；
9. 清空整个 Redis volume 后，非 terminal target 由 reconciler 新 generation 恢复；
10. `rg`、registry 和 catalog denylist 证明旧 live-recovery、private key依赖和复杂 dispatch 草稿已删除；
11. 所有活库/Compose 测试完成后，本轮 container、volume、network 和临时 image 为零。

验收分别报告本地测试、已提交、已推送、已部署和 runtime accepted；不得用其中一个状态代替另一个。
