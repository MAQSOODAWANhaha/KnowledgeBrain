# 共享队列运行时方案

| 项 | 值 |
| --- | --- |
| 状态 | 已批准；按 Oxana 2.1.3 原生能力简化方案实施 |
| 所有者 | Shared Platform |
| 发布依赖 | crates.io `oxana = "=2.1.3"`、`oxana-web = "=2.1.3"` |
| 消费方 | 招投标异步任务、retention；知识库现有任务 |

本文只定义队列运行时。原则只有一条：Oxana 已提供的能力直接使用，不在 PostgreSQL 或业务代码中再实现一遍。

## 1. 决策

V1 直接使用 crates.io 发布的 Oxana 2.1.3：

- `Cargo.toml` 精确锁定 `=2.1.3`；
- `Cargo.lock` 是版本、source 和 checksum 的唯一真源；
- 构建、测试和 CI 使用 `cargo --locked`；
- 不 fork、不 vendor、不使用 Git/path dependency、`[patch.crates-io]` 或 Cargo source replacement；
- 升级 Oxana 时单独评审并重跑本文件验收，不为依赖版本再造 promotion 协议。

## 2. 职责边界

| 能力 | 唯一所有者 |
| --- | --- |
| enqueue、scheduled job、并发消费 | Oxana |
| handler retry、retry delay、retry counter | Oxana |
| worker 进程 heartbeat、失联检测和 in-flight resurrection | Oxana |
| unique job 冲突处理 | Oxana |
| dead queue、查看与人工 `revive_all_dead` | Oxana / oxana-web |
| 业务任务是否仍有效、输入 revision 和最终结果 | PostgreSQL 业务 target |
| artifact/current pointer 的幂等原子发布 | PostgreSQL 业务事务 |

业务 target 只保存业务事实，不保存 transport 状态。最终发布必须校验 target/current revision，并依靠 unique key、CAS 和 immutable artifact 保证重复执行不会重复发布。

明确禁止：

- 自建 delivery generation、delivery attempt、queue membership、retry schedule、dead queue 或 resurrection 状态机；
- 用 PostgreSQL attempt counter、max attempts、`next_enqueue_at`、claim lease 或 heartbeat 接管 Oxana 的执行与失败重试；
- 扫描 `pending|running` target 自动重新 enqueue，形成第二套恢复循环；
- 读取或修改 `oxanus:*` 私有 Redis key；
- 用 `get_job`、queue list、stats 或 Redis membership 判定业务是否完成；
- 在数据库中镜像 queued、processing、retrying、dead 等 Oxana phase；
- 在 transport adapter 内隐藏 probe、repair、reaper 或无限重投。

该规则适用于后续新增和本轮改造的所有异步 consumer，并作为 code review 阻断项。“增强可靠性”不是重复实现 Oxana 能力的例外理由。只有业务幂等、业务 current/revision 校验、不可变结果发布和业务临时数据清理可以留在 PostgreSQL。

## 3. 唯一队列接口

招投标只注册一个稳定 typed job：

```text
name        = bid:delivery:v1
queue       = bid-delivery-v1
concurrency = Dynamic(4)
payload     = { target_kind, target_id, target_revision }
unique_id   = <target_kind>:<target_id>:<target_revision>
on_conflict = Skip
resurrect   = true
```

`target_kind` 只允许 `document_conversion|extraction_target|matching_schedule|matching_job|attachment_preparation|submission_render`。`target_revision` 是所属领域已经存在的 conversion generation、watermark 或 immutable target revision，不是 transport retry generation。payload 不携带对象内容、claim token、执行结果或可漂移 snapshot；worker 必须从 PostgreSQL 读取冻结业务输入。

平台 adapter 只暴露一个小 interface：

```text
enqueue(target_kind, target_id, target_revision)
  -> accepted(job_id)
   | unavailable(error)
```

一次调用只执行一次 `Storage::enqueue`。不查询 Redis 反推结果，不自动 probe 或补投。

## 4. 提交与 enqueue

PostgreSQL 与 Redis 没有跨库事务，V1 采用最小、可说明的顺序：

1. 业务事务幂等创建或取得同一个 target；
2. transaction commit 后调用一次 `enqueue`；
3. enqueue 成功才返回 `202`；
4. enqueue 失败或结果未知时返回`503 QUEUE_UNAVAILABLE`，响应携带稳定target ID和`retry_same_idempotency_key=true`；调用方必须使用同一idempotency key重试，并以同一unique ID再次enqueue。

业务 receipt 只冻结 target identity和业务 mutation结果，不记录“已入队”或最终HTTP `202`。命中completed receipt的重放仍必须执行第2步enqueue；因此首次enqueue失败、响应丢失和正常重复请求都走同一条小路径。

worker 在业务事务中创建确定性的 successor target 后，再逐个 enqueue。任一 enqueue 失败时向 Oxana 返回 `Err`；父 job 重试时只重放 successor enqueue，不重复已经完成的外部计算或业务发布。`on_conflict=Skip` 使该重放幂等。

这一取舍明确接受“投递时 Redis 必须可用”。V1 不承诺 Redis volume 被删除后的自动数据库重建，也不为该极端场景增加 outbox/reconciler。Redis 数据恢复使用基础设施备份；dead job 使用 oxana-web 的原生 `revive_all_dead`；尚未成功 enqueue 的请求由原 idempotent operation 重试。

## 5. Worker 合同

发布版固定：

```text
max_retries = 3
retry_delay = 10 seconds
resurrect = true
on_conflict = Skip
```

handler 只按业务结果返回：

| 结果 | PostgreSQL 动作 | 返回 Oxana |
| --- | --- | --- |
| 成功 | current/revision fenced transaction 原子发布 | `Ok` |
| stale revision、duplicate、已完成或已取消 | 不覆盖业务结果 | `Ok` |
| 确定性业务失败 | 原子记录业务失败 | `Ok` |
| 可重试外部错误或 timeout | 保持业务 target `pending`，只记录 bounded `last_error` | `Err` |

业务 target 不以 `running`、claim、lease 或 heartbeat 控制 Oxana 是否执行。Oxana 的进程 heartbeat 与 resurrection 负责 crash recovery；极端重复执行由最终 current/revision CAS 变成安全 noop。

Oxana retry 耗尽后 job 进入 dead queue，业务 target 仍是 `pending` 并保留最后错误；这是“业务意图未完成”，不是在 PostgreSQL 镜像 `dead`。修复外部原因后由管理员在现有 `/api/v1/ops/oxana/web` 门禁内使用原生 `revive_all_dead`。该操作保留原 envelope 和 retry count；revive 后若再次失败会重新进入 dead queue，不另造重试预算。

## 6. 生命周期与资源

- worker shutdown 直接使用 Oxana runtime 的 graceful shutdown；不再包一层队列监管框架；
- DocReader、provider、对象存储和 renderer 使用各领域已经定义的固定 timeout；timeout 直接返回 `Err`，不建立 watchdog 状态机；
- handler 自己启动的外部子进程必须在成功、错误、panic cancellation 和 shutdown 时停止并 wait/reap；
- matching 大结果的 staging 以 immutable target/job identity 归属；每次Oxana执行开始时清理同job未消费staging并重新Open，不跨retry恢复部分provider输出；staging TTL只回收crash遗留临时数据，不参与任务retry；
- Redis/PostgreSQL/Compose 测试结束后立即清理本轮 container、volume、network 和临时 image，并断言零残留。

## 7. 验收

只保留能证明职责边界的关键测试：

1. `cargo metadata --locked` 证明 Oxana 2.1.3 来自 crates.io 且无覆盖；
2. 活 Redis 证明失败 handler 执行初次加三次 retry，间隔使用 10 秒策略；
3. kill worker 后，`resurrect=true` 的 in-flight job 由 Oxana 继续消费；
4. 同一 unique ID 重复 enqueue 使用 `Skip`，业务结果只发布一次；
5. retry 耗尽进入 Oxana dead queue，oxana-web 原生 `revive_all_dead` 可再次执行且保留 retry count；
6. API enqueue 失败返回 `503`，同一 idempotency key 重试取得同一 target 并成功 enqueue；
7. 外部调用 timeout 返回 `Err` 并进入同一 Oxana retry，不遗留业务 `running` 或子进程；
8. successor enqueue 失败时，父 job 的 Oxana retry 只重放相同 successor enqueue；
9. transport 和业务代码中不存在 delivery generation、due reconciler、claim/lease heartbeat、private Redis key 或第二套 retry/resurrection；
10. 活库测试结束后本轮容器资源零残留。

队列测试只证明 transport 行为；业务 current/revision fencing、幂等发布和 successor 原子创建由 [`../bidding/durable-dispatch.md`](../bidding/durable-dispatch.md) 验收。
