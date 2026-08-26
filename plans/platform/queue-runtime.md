# 共享队列传输方案

| 项 | 值 |
| --- | --- |
| 状态 | clean-slate V1 已批准实施基线；待 PR8A 实施与验收 |
| 所有者 | Shared Platform |
| 消费方 | 知识库、招投标 durable dispatch |

本文只定义队列传输 module。业务 target、业务 snapshot、generation、retry budget、terminal receipt 和恢复资格由所属领域定义；Redis/Oxana 不是业务事实源。

## 1. Module 与 seam

平台提供一个内部 `WorkTransport` seam：

```text
领域 durable intent（PostgreSQL）
          |
          | DeliveryOffer
          v
WorkTransport interface
          |
          +-- OxanaRedisAdapter（生产）
          `-- RecordingTransport（合同测试）
```

`WorkTransport` 的最小 interface 语义为：

```text
offer(
  physical_queue,
  task_type,
  unique_identity,
  payload_version,
  bounded_payload,
  deadline
)
-> accepted(transport_id, receipt_fingerprint, inserted|duplicate_equivalent)
 | unavailable
 | identity_conflict
 | adapter_mismatch
 | payload_rejected

probe(transport_id, receipt_fingerprint)
-> present(queued|processing, phase_since)
 | absent
 | unavailable
 | identity_conflict
 | adapter_mismatch
```

- `receipt_fingerprint` 是 `schema + physical_queue + task_type + unique_identity + payload_version + canonical bounded_payload + resurrect/conflict policy` 的 canonical SHA-256；不得包含 `created_at`、随机数或其它每次 enqueue 会变化的 envelope 字段。
- `accepted(inserted)` 表示 exact receipt 已原子写入并进入 physical queue；`accepted(duplicate_equivalent)` 表示同 identity 的现存 receipt fingerprint、queue、task 和 payload 全部等价。两者都不证明业务开始或完成。
- `identity_conflict` 表示同 transport ID 已存在但 fingerprint、queue、task 或 canonical payload 不等价。这是 transport invariant/readiness failure，不能作为 duplicate 接受，也不能据此 poison 合法业务 target。
- `probe` 通过受控 Oxana patch 暴露的公开 atomic receipt 查询读取 exact receipt 及 `queued|processing` phase；不得以当前 `Storage::get_job` 的 hash 存在性冒充 `present`，也不得由平台 adapter 或业务代码扫描 queue/processing 私有 key。
- `phase_since` 使用 Redis/Oxana 写入的 phase 时间，只用于 bounded stall 判断和观测，不是业务 owner、lease 或完成事实。
- `absent` 只表示该 exact fingerprint 的 receipt 已不在 Redis，不表示业务失败；同 ID、不同 fingerprint 必须返回 `identity_conflict`，不能降级为 `absent`。
- `unavailable` 表示没有可靠 transport receipt，调用方必须按自己的 durable intent 释放 delivery claim 并稍后重试。
- `adapter_mismatch` 表示当前 binary、queue registry、handler registration 或 task→queue closure 不一致；这是全局 readiness/fatal error，不能污染单条业务 target。
- `payload_rejected` 只用于单条 identity/version/size/canonical payload 违反已加载合同；这是确定性 item error，不得伪装成瞬时失败。
- 平台公共 interface 不使用 `Option<String>` 同时表达成功、跳过和 Redis 不可用。

PostgreSQL 与 Redis 都是 local-substitutable dependency。真实 PostgreSQL/Redis 合同测试覆盖事务、锁、Oxana atomic receipt/unique identity、phase probe 和进程恢复；`RecordingTransport` 只用于故障注入，不能代替活 Redis 验收。

## 2. Oxana 的职责

Oxana/Redis 负责：

- physical queue、注册、并发和 backpressure；
- 已进入 Redis 的 pending/processing 消息；
- worker 进程死亡后的 in-flight resurrection；
- transport dashboard、queue depth 和 transport receipt；
- 同一 delivery offer 的 Redis 唯一身份与 fingerprint 等价去重。

Oxana/Redis 不负责：

- 推断 PostgreSQL 中应该存在但从未成功入队的业务任务；
- 保存业务 target、generation、watermark、snapshot 或 gate 真源；
- 判断业务 lease 是否过期、旧 owner 是否仍可发布；
- 生成 matching manifest 或其它 1..N 业务 fanout；
- 决定业务错误是 retry、failed、superseded 还是 terminal noop；
- Redis volume 丢失后的业务重建。

因此平台队列提供 at-least-once transport，不承诺跨 PostgreSQL/Redis exactly-once。领域必须使用 durable intent、幂等 identity 和业务 CAS fencing 得到正确结果。

## 3. Delivery identity 与 payload

每个 durable dispatch offer 使用不可复用的单调身份：

```text
<domain>:delivery:<dispatch_id>:<offer>
```

`transport_id` 必须在 Redis I/O 前由 allowlisted `task_type + "/" + unique_identity` 确定；四个 wrapper 的 Oxana `Job::name()` 必须显式等于冻结的 `task_type`，禁止依赖 Rust 默认类型路径。`offer accepted` 返回的 ID 必须与该 deterministic mapping 完全相同，使领域能在 offer claim 事务中预写 `expected_transport_id`，consumer-before-publisher-settle 时仍可验证 exact attempt。

同一 `dispatch_id` 的新 `offer` 必须能在旧 Oxana processing item 尚未清除时进入队列；consumer 再由 PostgreSQL CAS 判定新旧。禁止只用 target ID 作为永久唯一键，否则旧 Redis identity 可能阻断合法重投。

payload 只携带恢复 PostgreSQL identity 所需的 bounded opaque 字段，例如：

```text
dispatch_id, offer, lane_key, payload_version
```

`lane_key` 只用于证明消息经过 intent 冻结的 allowlisted physical lane；consumer 必须与 PostgreSQL intent 精确比较。业务 snapshot、actor、generation、route、object reference 和 retry policy 不从 Redis payload 取信。完整合同由 PostgreSQL durable intent 和 target 关系冻结。

### 3.1 Atomic transport receipt

当前 upstream Oxana 2.1.3 的 unique enqueue 是独立 `HEXISTS` 后再用非原子 pipeline 写 jobs hash 与 queue，`Storage::get_job` 也只读取 jobs hash；它们不足以实现本文的 `accepted/probe` 合同。V1 的受控 patch 必须在 Oxana 内部建立以下不变量：

1. 首次 offer 以单个 Lua/等价原子操作完成“identity 不存在检查、envelope/fingerprint 写入、queue membership 建立”；任一子步骤失败不得留下 hash-only receipt。
2. 并发相同 identity 只能得到一个 queue membership；后到请求只能得到 `duplicate_equivalent` 或 `identity_conflict`。
3. dequeue 使用原子 queue→processing move；processing receipt 写入 phase、phase timestamp 和 exact process instance identity。
4. resurrection 使用单个原子 transition 将 exact receipt 从 dead processing owner 移回原 queue，并把 phase 重置为 `queued`；不得先 `LPOP` 后 best-effort `LPUSH`。
5. success、kill、cancel 和 invalid-envelope 清理原子移除 exact membership 与 receipt；不得留下 hash-only receipt。不存在 receipt 但残留的 membership 只能被 Oxana 内部 bounded repair 删除，不能重新解释为合法 delivery。
6. public probe 原子验证 receipt fingerprint 和唯一 membership，返回 phase；检测到同 ID 不同内容、多 membership 或 hash-only receipt 时返回 `identity_conflict`/capability failure，不能返回 `present`。

这些能力只通过 Oxana public interface 暴露给 `OxanaRedisAdapter`。Redis key、Lua script 和 membership repair 都是 Oxana implementation，不能泄漏到 `WorkTransport` 调用方。

### 3.2 Receipt phase 与 max observing

`present` 只是 transport hint，不能证明 consumer 已建立 business owner，也不能永久压制领域恢复。领域使用冻结的 `consumer_start_deadline` 与 `max_observing_age` 组合判断：

| probe phase | business owner | 未超时 | 超过 consumer-start/max-observing |
| --- | --- | --- | --- |
| `queued` | none | 保持当前 offer | 不盲目复制到同一拥塞 lane；报告 queue stall/readiness，并由领域 admission/backpressure 合同处理 |
| `processing` | none | 保持当前 offer | 允许领域 CAS successor offer；旧 receipt 晚到必须业务 noop |
| 任一 present | fresh | 保持当前 offer | 仍由 business lease/heartbeat 决定，不因 transport age 抢占 fresh owner |
| 任一 present | expired | 不适用 | 领域先精确 reap 旧 owner，再 CAS successor offer |

- `consumer_start_deadline` 从 receipt 首次进入 `processing` 的 `phase_since` 计算，不能从 publisher 本地时钟或 enqueue 返回时间猜测。
- `max_observing_age` 必须是有限正值且不小于 consumer-start deadline；它是无 business owner 的逃生上限，不是 active business execution lease。
- queued backlog 的恢复不是创建更多相同 lane delivery；若 queue 长期不推进，应使 queue/runtime readiness 降级并阻止无界 offer admission。
- `absent` 且无 fresh owner 时，领域可立即按自己的 durable intent 推进；`unavailable` 时不能根据 age 猜测 receipt 已丢失。

## 4. 进程 identity 与 resurrection

Oxana worker instance identity 必须包含每次 runtime boot 都不同的 boot UUID，不能只使用 `hostname + pid`。Docker/容器重启复用 hostname/PID 时，新进程不得覆盖旧 owner 的 heartbeat。

V1 固定使用仓库内 `vendor/oxana` 受控 patch：保持已锁定的 upstream 2.1.x wire contract，增加 atomic receipt 能力，并在每次 Runtime 构造时生成 boot UUID、创建 runtime-scoped Storage view，把 process identity 固定为 `hostname-pid-boot_uuid`。Cargo 使用本地 `[patch.crates-io]` 路径并由 `Cargo.lock`、仓库 tree hash 和 review 证据锁定；不得依赖未发布版本、运行时下载或部署脚本改 hostname。

boot identity 的全生命周期合同为：

- 同一 runtime 持有的 Storage 及其所有 clone 共享一个 immutable boot UUID；producer-only Storage 不得注册 worker heartbeat 或 processing owner；
- `Process` serialization/ID、ping、dequeue destination、started phase、finish/kill 和 self-cleanup 必须使用同一个 exact process instance identity；
- 从同一 base Storage 连续构造两个 runtime 时必须生成两个 identity，禁止沿用首次 runtime 的 clone；
- self-cleanup 只能删除本 boot identity 的 heartbeat，不能删除相同 hostname/PID 的旧或新 boot；
- resurrection 必须先按 exact boot identity 判定 owner dead，再执行上一节的原子 processing→queue transition。

patch 必须保留并运行 upstream Oxana 测试，并增加相同 hostname/PID 不同 boot UUID、Storage clone 一致性、连续 runtime identity 和旧 owner resurrection 回归。

应用代码禁止：

- 读取、扫描、移动或删除 `oxanus:*` 私有 key；
- 根据 hostname/PID 手写 processing-list replay；
- 通过 Redis 私有布局判断业务 target 是否 active。

该 patch 落位时删除所有 `replay_orphaned_local_jobs` 类 workaround。队列库的内部 key 变化不得要求业务 module 修改。

## 5. Retry 所有权

Retry 分三层且不得相乘：

| 层次 | 所有者 | 允许行为 |
| --- | --- | --- |
| provider 调用 | target executor | 按冻结 provider policy 做 bounded retry/deadline |
| 业务 target | 所属领域 | 按 attempt、lease、generation 决定 retry/failed/superseded |
| Redis transport | Oxana | 仅处理未形成 durable业务结算的 transport redelivery 和进程死亡 resurrection |

业务 handler 的 retry-scheduled、deterministic、stale、cancelled、lease-lost 或 retry-budget-exhausted 结果必须先写 durable settlement，再向 Oxana 返回成功。DB 不可用、进程崩溃或尚未形成 durable receipt 的失败可留下 transport error，但业务重投仍由 durable dispatcher 决定。

采用 PostgreSQL durable intent 的 delivery job 使用 `resurrect=true`，Oxana handler-level `max_retries=0`。这样保留进程崩溃 resurrection，又不让 Oxana retry 与数据库业务 retry 相乘。尚未接入 durable intent 的其它领域 task 必须在自己的活动方案声明 retry owner，不能借用本文声称 DB→Redis 不丢。

## 6. Inbound delivery outcome

Oxana wrapper 收到消息后必须把实际 `transport_id + receipt_fingerprint + receipt_phase` 连同 bounded payload 交给领域 consumer；consumer-before-publisher-settle 是合法顺序，平台不能要求 publisher 已先持久化 `accepted` 才调用领域。领域负责用 exact offer/attempt CAS 吸收该竞态。

领域 handler 返回给平台的结果固定为：

```text
ack_settled(code)     // durable success/failed/retry_scheduled/noop/poison receipt 已提交
unsettled             // DB 不可用、deadline 或尚无 durable settlement
adapter_mismatch      // binary/registry/queue closure 不成立
```

- `ack_settled` 才向 Oxana 返回 success；code 仅用于 bounded metric，不改变 transport ack 语义。
- `unsettled` 向 Oxana 返回 handler error；delivery worker 的 `max_retries=0` 会结束当前 receipt，领域 dispatcher 再按 durable intent 决定同 offer 重投或 successor offer。不得在 handler 内 sleep/retry。
- `adapter_mismatch` 触发 runtime cancellation/readiness failure，并向 Oxana 返回 error；不能写业务 target failed。
- wrapper 从收到 envelope 到首次领域 begin 必须有硬 deadline，之前不得调用 provider、object store 或其它业务外部依赖。进程存活但卡在 begin 前时，不能假设已有 business lease 可供 repair。
- publisher accepted settlement 与 inbound begin 都必须是单调幂等；平台只传递 observed receipt，不替领域决定二者的数据库锁序或状态转换。

## 7. 故障责任矩阵

| 故障 | 正确行为 |
| --- | --- |
| 业务事务提交后 Redis 不可用 | durable intent 保持 ready，稍后重新 offer |
| consumer 在 publisher accepted settle 前开始 | 传 actual receipt 给领域；领域从 offering/observing exact attempt 单调 begin，publisher 后续 settle 幂等 |
| Redis accepted 后 publisher 在 DB settle 前崩溃 | consumer 可先建立 durable business claim；否则 offer lease 到期后重投同 identity |
| worker 取走消息后进程死亡 | Oxana resurrection；业务 lease 同时阻断旧 owner 发布 |
| 进程仍活且已有 business owner 的任务卡死 | 领域按业务 heartbeat/lease repair 终结旧 owner，再产生 successor offer |
| processing receipt 超过 consumer-start deadline 且始终没有 business owner | 领域在 max observing age 后可 CAS successor offer；旧 delivery 晚到由业务 CAS noop |
| queued receipt 长时间 present | 不因单条 age 盲目复制到同一拥塞 lane；报告 queue stall/readiness，由领域冻结的 admission/backpressure 合同处理 |
| receipt present 且 business owner fresh | 不创建重复 offer，只延后下一次 probe |
| Redis volume 丢失或 exact transport ID absent，且无 fresh owner | 领域从 PostgreSQL intent 推进新 offer |
| 同 transport ID 的 fingerprint/queue/task 不同 | identity_conflict，全局 not-ready/fatal；不接受 duplicate、不 poison 业务 target |
| 旧 offer 晚到 | consumer 对 offer、gate epoch、generation、owner 做 CAS 后 noop |
| binary/queue registry/handler closure 不一致 | adapter_mismatch，全局 not-ready/fatal；不 poison 业务 target |
| PostgreSQL 中单条 immutable business payload/identity 损坏 | 领域 durable poison + typed target terminal/告警，不无限重试；不得与 Redis identity_conflict 混同 |

## 8. Retention、安全、可观测性与资源

Upstream Oxana 2.1.3 会按 job `created_at` 在 7 天后直接删除 jobs hash，而不证明 queue/processing membership 已终结；该行为不满足本文 receipt 合同。受控 patch 必须固定：

- active `queued|processing` receipt 不得仅因达到 7 天而删除；
- terminal success/kill/cancel 在原子 settlement 中删除 active receipt，dead/audit 展示使用独立 bounded retention；
- orphan cleanup 必须先原子证明没有 queue/processing membership；发现 hash-only、多 membership 或 fingerprint 损坏时计入 invariant failure 并触发 readiness，不能静默把 `present` 变成 `absent`；
- policy verifier 必须约束 dead/audit retention、probe interval 与可观测窗口，但不得用 TTL 代替 active receipt lifecycle；
- fresh deploy 不保留 upstream 7 天 active-receipt 兼容分支。

- queue/task/版本/lane 使用 allowlist，不允许 unknown-task fallback；
- payload、日志、metric 不记录文档内容、secret 或完整 snapshot bytes；
- transport adapter 使用短 enqueue deadline；外部 Redis I/O 不得发生在业务 PostgreSQL 事务内；
- queue depth 只表示 transport 状态，不能替代 durable dispatch backlog；
- 平台指标至少提供 queue depth、oldest queued/processing phase age、resurrection count、offer latency、duplicate-equivalent、identity-conflict、unavailable/rejected 和 receipt-invariant failure count；
- 领域另外提供 ready backlog、oldest due intent、business lease 与 poison 指标；
- Redis/Oxana 测试容器和 volume 在验收命令结束后立即删除。

## 9. 平台验收

1. allowlisted queue/task/version 正反向 closure；
2. atomic offer 在 jobs write、queue membership 和 response 丢失的每个故障点都只得到完整 receipt 或无 receipt；
3. 高并发同 identity 只产生一个 membership；相同 fingerprint 返回 duplicate-equivalent，不同 fingerprint/queue/task 返回 identity-conflict；
4. public probe 验证 exact fingerprint 并区分 queued/processing/absent；hash-only、多 membership 和损坏 envelope 不得返回 present；
5. dequeue、success、kill、cancel、invalid-envelope 与 resurrection 的每个命令边界均保持 receipt invariant；
6. worker crash 后 in-flight job 原子 resurrection，phase 重置 queued；
7. 相同 hostname/PID、不同 boot UUID 的新进程不会吞掉旧 processing owner，Storage clones 与连续 runtime identity 符合全生命周期合同；
8. `max_retries=0` 不阻止 `resurrect=true` 的进程崩溃恢复；
9. consumer-before-publisher-settle、unsettled、ack-settled 和 adapter-mismatch 的 inbound mapping 可重复收敛；
10. processing 卡在领域 begin 前会形成有界、可观察的 stall outcome，不依赖尚不存在的 business lease；queued backlog 不因 age 产生无界 duplicate；
11. active receipt 超过 upstream 7 天阈值仍可 probe/execute/resurrect；terminal/dead/audit retention 独立生效；
12. adapter/业务代码均不读取 `oxanus:*` 私有 key；
13. Redis volume 清空后平台不声称恢复业务，由领域 durable-dispatch 验收补足；
14. bounded payload、enqueue deadline、lane concurrency 与日志脱敏；
15. fresh Redis/Compose 实测并保存 queue registration、故障步骤和结果。

本地合同测试、fresh Compose、部署和 runtime accepted 必须分别报告。
