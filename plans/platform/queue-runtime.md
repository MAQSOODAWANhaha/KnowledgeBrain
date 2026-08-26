# 共享队列传输方案

| 项 | 值 |
| --- | --- |
| 状态 | clean-slate V1 稳定版依赖修订已批准并固化；运行时代码尚未实施或验收 |
| 所有者 | Shared Platform |
| 发布依赖 | crates.io `oxana = "=2.1.3"` |
| 消费方 | 招投标 durable dispatch；知识库旧 jobs 仍直接使用 Oxana，不属于本 WorkTransport seam |

本文只定义应用到 Oxana 的 transport seam 与发布版事实。业务 target、current head、generation、gate、business lease、successor、noop、settlement 和恢复资格必须由所属领域定义；招投标的唯一权威状态机见 [`../bidding/durable-dispatch.md`](../bidding/durable-dispatch.md)。

## 1. 决策与边界

V1 直接使用 crates.io 已发布的 Oxana 2.1.3：

- `oxana`、`oxana-macros`、`oxana-web` 精确锁定 2.1.3；
- 不 fork、不 vendor、不使用 path dependency 或 `[patch.crates-io]`，不依赖未发布 API；
- 只使用公开的 typed `Storage::enqueue`、worker runtime、queue registry、`on_conflict=Skip` 和 `resurrect=true`；
- 不读取或修改 `oxanus:*` 私有 key，不把 `get_job`、queue list、stats 或 `delete_job` 当 correctness proof；
- 不在平台层伪造 exact receipt、fingerprint probe、retire、terminal tombstone 或 boot UUID。

Oxana 2.1.3 只提供 best-effort queue delivery 和 best-effort resurrection。它不拥有业务事实，也不能单独证明严格 at-least-once、exactly-once 或至多一次有效业务发布。任何需要跨 Redis 丢失、部分写、重复、延迟或 cleanup 收敛的领域，必须在自己的 durable store 中定义恢复合同。

## 2. WorkTransport 深 module

平台只暴露一个小的内部 seam：

```text
领域 durable dispatcher
          |
          | DeliverySpec
          v
WorkTransport
          |
          +-- OxanaStableAdapter（生产）
          `-- RecordingTransport（调用次数/故障注入测试）
```

interface 固定为：

```text
DeliverySpec {
  physical_lane,
  task_type,
  dispatch_id,
  payload_version
}

prepare(spec: DeliverySpec)
-> prepared(
     expected_job_id,
     canonical_payload_bytes,
     canonical_payload_sha256,
     resurrect=true,
     on_conflict=skip
   )
 | payload_rejected
 | adapter_mismatch

offer(prepared: PreparedDelivery, deadline)
-> returned(job_id)
 | indeterminate(error_class)
 | returned_job_id_mismatch(actual_job_id)
```

- `prepare` 是无 Redis、网络、时钟和随机数的纯函数。它验证 allowlist，构造 typed job，并冻结应用 payload bytes/digest 与预期 Oxana job ID；digest 只证明应用 codec 一致，不称为 Redis receipt fingerprint。
- V1不另设可漂移的`contract_version`字段：`task_type=bid:delivery:v1`本身固定delivery contract，`payload_version=1`固定codec。未来不原地promotion这两个值；新合同使用新task type并先修订领域方案与registry closure。
- `offer` 的一次调用内部最多发起一次发布版 `Storage::enqueue`，绝不内部 retry、probe、delete 或 private-key repair。deadline 已过或 future 首次 poll 前被取消时允许零次 I/O；未取消且 deadline 有效的正常路径必须恰好一次。跨调用的一次性业务 identity 由调用方领域负责，平台 adapter 不声称拥有该状态。
- deadline 取消后结果仍是 `indeterminate`，不能推断 Redis 未写入。
- `returned(job_id)` 只表示公开 API 返回，可能是首次写入，也可能是 unique `Skip` 返回已有 ID；它不证明 inserted、queued、processing、单一 membership 或 payload 等价。
- `indeterminate` 覆盖连接错误、timeout、response lost 和其它 enqueue error。调用方不得据此断言无 Redis side effect。
- 返回的 `job_id` 必须逐字节等于 `expected_job_id`；不等返回携带实际ID的 `returned_job_id_mismatch` 并使 runtime readiness fail closed。领域必须把该次已发生的enqueue视为indeterminate，禁止原identity重投；实际ID只可进入bounded runtime observation，不能成为业务receipt。prepare/registry/codec closure mismatch在offer前作为无enqueue的fatal处理，不伪造实际ID或delivery observation。
- 平台接口不使用 `Option<String>` 同时表达成功、跳过和 Redis 不可用。

生产 adapter 内聚 queue mapping、typed job 构造、Oxana error 分类和 deadline；业务 module 不接触 Oxana `Storage`、`JobEnvelope` 或 Redis key。

## 3. Oxana 2.1.3 的真实语义

| 能力 | V1 可依赖语义 | 不得假设 |
| --- | --- | --- |
| unique ID | `Job::name() + "/" + unique_id` 的确定值 | Redis 会比较相同 ID 的 payload |
| `Skip` | jobs hash 已存在时返回同一确定 ID | 原子去重、单一 queue membership、duplicate equivalent |
| enqueue `Ok` | 调用返回一个 JobId | 首次 inserted、queued、processing 或 durable accepted |
| enqueue `Err/timeout` | 结果未知 | Redis 完全没有写入 |
| dequeue | `LMOVE` 原子把一个 ID 从 queue 移到当前 processing list | started/finish/resurrection 合成一个原子状态机 |
| worker `Ok` | Oxana 尝试删除 job hash 和当前 processing membership | 所属领域业务必然已成功 |
| worker `Err` + `max_retries=0` | 不做 Oxana handler retry，job 进入 dead/结束路径 | 存在领域可查询的 terminal receipt |
| `resurrect=true` | dead process scan 可尝试把 processing job 放回 queue | 原子 resurrection、跨 volume 恢复、hostname/PID 复用安全 |
| `get_job`/list/stats | 监控 hint | exact phase、absence 或 delivery proof |
| cleanup | 七天后可删除旧 jobs hash | active job 必然保留或 membership 同步清理 |

Oxana unique enqueue 先检查 jobs hash，再执行写 hash/压 queue 的 pipeline。并发或网络中断可能得到重复 membership 或 hash-only job。因此 `Skip` 只降低 transport 噪声，不能成为业务正确性边界。

## 4. 发布版 typed delivery 合同

招投标只在平台注册一个稳定 typed job：

```text
task_type = bid:delivery:v1
unique_id = dispatch_id canonical lowercase UUID
job_id    = bid:delivery:v1/<dispatch_id>
payload   = { dispatch_id, payload_version }
```

`BidDeliveryV1Job` 必须显式实现 `Job::name()`，显式固定 `on_conflict=Skip` 与 `resurrect=true`；worker 固定 `max_retries=0`。payload 不携带 snapshot、target kind、project、generation、gate、owner、retry policy 或对象引用。

dispatch ID 的创建、是否允许调用 `offer`、consumer begin、历史 delivery 吸收和业务重试均不在本文定义，只链接招投标 [`durable-dispatch.md`](../bidding/durable-dispatch.md)。平台只保证同一次 adapter invocation 不隐藏第二次 enqueue。

## 5. Resurrection 与 legacy replay 边界

发布版原生 `resurrect=true` 是延迟优化，不是 correctness owner。它可能在 processing membership 部分丢失、Redis volume 清空、七天 cleanup 或相同 hostname/PID 重启时漏恢复。

仓库现有 `replay_orphaned_local_jobs` 没有按领域或 registry 过滤：它遍历 processing list，只要对应 job hash 存在且 metadata 没有显式 `resurrect=false`（字段缺失默认 true）就可能移动 membership。因此在 Bid 新 job 出现后也可能触碰其 transport membership。边界必须如实定义：

- 新 `WorkTransport`、Bid dispatcher 和 Bid consumer 不主动调用、不读取其结果，也不依赖它完成 Bid 恢复；
- 它触碰 Bid membership 只能作为额外 best-effort delivery，加速或产生 duplicate，不能改变 PostgreSQL 决策；
- PR8 暂不删除该共享函数，因为仍启用的非 Bid jobs 尚无替代恢复证明；
- 最终删除属于独立 Shared Platform/Knowledge Base cutover，必须证明所有仍启用 job 已有独立 durable owner，或未来发布版 Oxana 已公开修复并通过对应 restart/partial-state 验收。

Bid clean-slate cutover 删除 `system:live-recovery:v1` 业务两跳和全部 Bid private-key 依赖，但不会把共享 replay 的存在描述成“只服务 non-Bid”或用它作为 Bid 验收证据。

## 6. 安全、可观测性与资源

- queue/task/version/lane 全部 allowlist；unknown task 没有 fallback。
- payload、日志和 metric 不得记录文档内容、secret 或完整 snapshot bytes。
- Redis I/O 永远发生在 PostgreSQL 事务外；adapter 使用硬 deadline。
- transport 指标至少包含 health、queue depth、enqueue returned/indeterminate/latency、`resurrection_enabled` 和 dead count；这些只用于运维。Oxana 2.1.3 的公开 API 不提供原生 resurrection 次数，因此 V1 只从冻结的 `Job::should_resurrect()` 暴露配置 gauge，并以活 Redis 行为回执证明实际复活；禁止用测试或调用方手工累加伪造生产 resurrection counter，也禁止为取得该计数读取 private key 或修改发布依赖。
- readiness 验证 Cargo 来源/版本以及 queue/task/payload/handler/worker retry 的 registry closure。PR8A 活 Redis验收必须真实注册 worker 并证明该合同；PR8A/PR8B 生产 composition 保持 dormant，不能把测试态注册描述成生产 worker 已注册，只有目标 owner 切换的纵切才能把对应生产 registration 纳入 readiness。queue depth 不代表任何领域 backlog。
- 所有启动 Redis/PostgreSQL/Compose 的测试都必须覆盖 `success|failure|timeout|cancel|SIGINT|SIGTERM`，使用 shell trap + CI `if: always()` 双层 cleanup，并证明本轮 container/volume/network/临时 image 零残留；领域验收只能消费并加严该平台合同，不能反向拥有它。

## 7. 平台验收

PR8A 只验收发布版 transport seam，不提前声称验证 PR8B 的 PostgreSQL dispatcher：

1. fail-closed verifier 证明三个 Oxana package 唯一为 registry 2.1.3、checksum 固定，且无 path/vendor/patch；
2. pure `prepare` golden 覆盖显式 job name、deterministic ID、bounded payload、digest、lane/task/version allowlist 与篡改负例；
3. RecordingTransport 证明一次 `offer` invocation 的 enqueue count 只能为 0 或 1，绝不大于 1；未取消且 deadline 有效的正常路径为 1，deadline/cancel 零 I/O路径与 Err/timeout 路径均无内部 retry/probe/delete；
4. 活 Redis 证明发布版 `Storage::enqueue`、unique `Skip`、`resurrect=true` 和真实注册 worker 的 `max_retries=0` 行为；失败 handler 只执行一次、retry queue 为零且 dead 为一，并且不把 enqueue 返回值提升为 receipt；
5. adapter mismatch、Redis unavailable、deadline 与 registry closure 有 bounded error、metric 和 readiness 行为；
6. `get_job`、queue list、stats、`delete_job` 和 `oxanus:*` 不出现在 WorkTransport/Bid correctness 路径；
7. legacy replay 混合 processing-list fixture 证明 hash metadata 缺失/true 时可能移动 Bid membership、显式 false 时不移动，并静态证明 WorkTransport/Bid 不调用它；启用/禁用 replay 后业务仍收敛的证明属于 PR8B synthetic 与 PR9，不由 PR8A 冒充；
8. 本地测试、fresh Compose、部署和 runtime accepted 分别报告；`bid-durable-dispatch` 从PR8A起必须调用统一cleanup harness实际触发固定六种退出模式，shell trap与CI独立`if: always()`步骤分别产生receipt，并在每种模式后断言本轮container/volume/network/临时image零残留；缺模式、cleanup失败或残留均使required job失败。

Bid 的 once-per-dispatch、consumer-before-publisher、deadline、lease、successor、historical noop 和 fault matrix 只由 [`durable-dispatch.md`](../bidding/durable-dispatch.md) 及其 PR8B～PR9 验收定义。
