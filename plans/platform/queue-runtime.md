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
DeliverySpec {
  physical_queue,
  task_type,
  unique_identity,
  payload_version,
  canonical_payload_bytes
}

TransportPolicy {
  transport_policy_version,
  resurrect,
  on_conflict,
  tombstone_retention_ms
}

prepare(delivery_spec: DeliverySpec, policy: TransportPolicy)
-> prepared(contract_version, policy_version, transport_id, receipt_fingerprint, canonical_envelope)
 | adapter_mismatch
 | payload_rejected

offer(prepared_delivery, deadline)
-> accepted(receipt_view, inserted|duplicate_equivalent)
 | terminal(terminal_view)
 | retired
 | unavailable
 | identity_conflict
 | adapter_mismatch

probe(receipt_identity, deadline)
-> present(receipt_view)
 | terminal(terminal_view)
 | retired(retired_at)
 | absent
 | unavailable
 | identity_conflict
 | adapter_mismatch

retire_exact(receipt_identity, reason_code, deadline)
-> retired(inserted|already_retired)
 | already_terminal(terminal_view)
 | unavailable
 | identity_conflict
 | adapter_mismatch
```

- `prepare` 是无 Redis/网络/时钟/随机数的纯函数；相同 `DeliverySpec + TransportPolicy` 必须得到逐字节相同的 `PreparedDelivery`。`TransportPolicy` 是显式、不可拆分的值对象，必须与 sealed registry 中该 version 的记录逐字段相等；policy 未注册是 `adapter_mismatch`，payload 超出该 policy 的固定上限才是 `payload_rejected`。领域在 stage/advance 事务中冻结 exact prepared value，publisher 只读取并复验后调用 `offer`。禁止 PostgreSQL、dispatcher 和 Oxana wrapper 各自发明 identity/fingerprint 算法或从 live default 补 hidden policy value。
- canonical contract 固定为 `work-transport-envelope/v1` binary：ASCII `KBWT` + `u16be(1)`，随后依次对 `physical_queue,task_type,unique_identity,transport_policy_version` 写 `u32be length + exact UTF-8`，写 `u32be(payload_version)`、`u32be payload_length + canonical_payload_bytes`、`u8(resurrect)`、`u8(on_conflict_tag)`、`u64be(tombstone_retention_ms)`；V1 数值映射固定为 `resurrect=false=0,true=1`、`on_conflict=reject_non_equivalent=1`，其它 tag 拒绝。fingerprint 是完整 bytes 的 lowercase hex SHA-256。`created_at`、deadline、attempt、随机数和 adapter 私有字段不进入 fingerprint。
- V1 registry 的 Bid policy 固定为 `bid-durable-transport/v1`：`resurrect=true`、`on_conflict=reject_non_equivalent`、payload 最大 4096 bytes、tombstone retention 30 days。改变任一值必须注册新 policy version，并升级引用它的 task contract；旧 prepared offer 永远按原 policy 验证。task type/unique identity 使用 ASCII `[a-z0-9][a-z0-9:_-]*`，分别不超过 128/256 bytes且不得含 `/`；physical queue 同字符集且不超过 64 bytes；`transport_id=task_type + "/" + unique_identity` 不超过 385 bytes。
- `ReceiptIdentity` 固定包含 `contract_version,policy_version,transport_id,receipt_fingerprint`。`ReceiptView` 在此基础上返回 `phase=queued|processing,phase_epoch,phase_age_ms`；`phase_epoch` 每次 phase transition 单调变化，`phase_age_ms` 必须在同一个 Redis 原子操作中使用 Redis `TIME` 计算。领域不得用 PostgreSQL time 减 Redis timestamp。`TerminalView` 返回 `terminal_code=ack_settled|handler_unsettled|killed|invalid_envelope` 与 transport-local audit time。
- `accepted(inserted)` 表示 exact receipt 已原子写入并进入 physical queue；`accepted(duplicate_equivalent)` 表示同 identity 的现存 receipt fingerprint、queue、task 和 payload 全部等价。两者都不证明业务开始或完成。
- `offer -> terminal|retired` 表示 exact identity 已有相同 fingerprint 的 tombstone，不能重新入队；publisher 必须按 terminal/stale attempt 结算。同 ID 不同 fingerprint 仍是 `identity_conflict`。
- `identity_conflict` 表示同 transport ID 已存在但 fingerprint、queue、task 或 canonical payload 不等价。这是 transport invariant/readiness failure，不能作为 duplicate 接受，也不能据此 poison 合法业务 target。
- `probe` 通过受控 Oxana patch 暴露的公开 atomic receipt 查询读取 exact receipt 及 `queued|processing` phase；不得以当前 `Storage::get_job` 的 hash 存在性冒充 `present`，也不得由平台 adapter 或业务代码扫描 queue/processing 私有 key。
- `absent` 只表示该 exact fingerprint 的 receipt 已不在 Redis，不表示业务失败；同 ID、不同 fingerprint 必须返回 `identity_conflict`，不能降级为 `absent`。
- `retire_exact` 只处理调用方给出的 exact ID/fingerprint：原子移除 queued/processing membership 与 active receipt，并写入 bounded retired tombstone；即使调用时 receipt absent，也必须创建同一 tombstone后返回 `retired(inserted)`。在同一 Redis volume/incarnation 内，tombstone 阻止已经失去数据库资格的旧 publisher 事后创建同一 identity；后续 exact `offer` 返回 `retired`，晚到 handler 仍必须由业务 CAS 变成 noop。不同 fingerprint 必须返回 `identity_conflict`，不得按 ID 粗暴取消其它 delivery。
- `terminal` 表示 transport 已结束而非业务必然完成。`ack_settled` 要求领域已经存在至少一条匹配 exact receipt 的 durable ACK settlement，领域随后按该 settlement、target 与 owner 当前状态继续收敛，不能盲目创建 successor；缺少匹配 settlement 是 invariant failure。`handler_unsettled|killed|invalid_envelope` 没有这项证明，owner none/expired 时由领域推进 successor，owner fresh 时等待 business lease，且都不能重投同一 tombstoned offer。`retired` 只表示领域主动使该 offer 失效；current nonterminal offer 看到 retired 才是 invariant failure。
- `retired`/`already_terminal` 是当前 Redis volume/incarnation 中 exact receipt 已结束的可靠证明；active receipt 不能靠 TTL 伪装。完整 volume 丢失会同时删除 active receipt 和 tombstone，平台不得声称跨 volume 保留该证明；领域若再次观察 historical identity active，必须 durable noop并重新发起 retirement。V1 30-day tombstone 必须满足领域 verifier 的 `transport_tombstone_retention >= dispatch_replay_window + max_offer_lease + max_clock_skew`；Bid V1 固定 replay window 7 days、max offer lease 10 minutes、clock-skew budget 5 minutes。
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

`transport_id` 必须在 Redis I/O 前由 allowlisted `task_type + "/" + unique_identity` 确定；四个 wrapper 的 Oxana `Job::name()` 必须显式等于冻结的 `task_type`，禁止依赖 Rust 默认类型路径。`prepare` 返回的 ID 必须与该 mapping 完全相同，使领域能先冻结 immutable offer identity，consumer-before-publisher-settle 时仍可验证 exact delivery。

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
5. handler `ack_settled`、`unsettled`、kill 和 invalid-envelope 都原子移除 exact membership 与 active receipt并写带 exact terminal code 的 tombstone；不得留下 hash-only receipt。`unsettled` 写 `handler_unsettled` 而不是伪装成功。
6. public probe 原子验证 receipt fingerprint 和唯一 membership，返回 active phase、retired/terminal tombstone 或 absent；检测到同 ID 不同内容、多 membership 或 hash-only receipt 时返回 `identity_conflict`/capability failure，不能返回 `present`。
7. public `retire_exact` 原子验证 fingerprint、移除 exact active membership/receipt 并写 retired tombstone；同一 Redis volume/incarnation 内，并发 offer/retire 只能线性化为 active 或 retired，retire 胜出后同 identity 在 tombstone window 内不能复活。

这些能力通过新增的 Oxana durable-receipt public path 暴露给 `OxanaRedisAdapter`。现有非 durable `Storage::enqueue` 与 unique `Skip` 行为保持不变，防止 PR8A 改坏知识库等 legacy jobs；boot UUID、atomic dequeue/resurrection 等安全修复可由两条路径共享。Redis key、Lua script 和 membership repair 不能泄漏到调用方。

### 3.2 Receipt phase 与 max observing

`present` 只是 transport hint，不能证明 consumer 已建立 business owner，也不能永久压制领域恢复。领域使用冻结的 `consumer_start_deadline` 与 `max_observing_age` 组合判断：

| probe phase | business owner | 未超时 | 超过 consumer-start/max-observing |
| --- | --- | --- | --- |
| `queued` | none | 保持当前 offer | 不盲目复制到同一拥塞 lane；报告 queue stall/readiness，并由领域 admission/backpressure 合同处理 |
| `processing` | none | 保持当前 offer | 允许领域 CAS successor offer；旧 receipt 晚到必须业务 noop |
| 任一 present | fresh | 保持当前 offer | 仍由 business lease/heartbeat 决定，不因 transport age 抢占 fresh owner |
| 任一 present | expired | 不适用 | 领域先精确 reap 旧 owner，再 CAS successor offer |

- `consumer_start_deadline` 只比较 Redis 原子返回的 `phase_age_ms`，不能执行 `PostgreSQL now - Redis phase_since`。
- `max_observing_age` 使用领域在 PostgreSQL 写入的 `observing_since` 与 DB time 比较；它必须有限且不小于 consumer-start deadline，是无 business owner 的逃生上限，不是 business lease。
- queued backlog 的恢复不是创建更多相同 lane delivery；若 queue 长期不推进，应使 queue/runtime readiness 降级并阻止无界 offer admission。
- `terminal(handler_unsettled|killed|invalid_envelope)` 且 owner none/expired 时必须推进 successor offer；owner fresh 时等待 business lease。`terminal(ack_settled)` 必须先找到 exact durable ACK settlement，再按 target/owner/gate 状态收敛；不得把一个已 ACK 的 noop 或 business result 重新解释成 transport failure。`absent` 且无 fresh owner时可推进；`unavailable` 时不能根据 age 猜测 receipt 已丢失。`retired` 不能等同 terminal/absent。

## 4. 进程 identity 与 resurrection

Oxana worker instance identity 必须包含每次 runtime boot 都不同的 boot UUID，不能只使用 `hostname + pid`。Docker/容器重启复用 hostname/PID 时，新进程不得覆盖旧 owner 的 heartbeat。

V1 固定使用仓库内 `vendor/oxana` 受控 patch：保持已锁定的 upstream 2.1.x wire contract，增加 atomic receipt 能力，并在每次 Runtime 构造时生成 boot UUID、创建 runtime-scoped Storage view，把 process identity 固定为 `hostname-pid-boot_uuid`。Cargo 使用本地 `[patch.crates-io]` 路径并由 `Cargo.lock`、仓库 tree hash 和 review 证据锁定；vendor 必须保留 upstream version/revision、原许可证和本地 patch manifest。不得依赖未发布版本、运行时下载或部署脚本改 hostname。

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

Oxana wrapper 收到消息后必须把实际 `ReceiptView` 连同 bounded payload 交给领域 consumer；consumer-before-publisher-settle 是合法顺序，平台不能要求 publisher 已先持久化 `accepted` 才调用领域。领域负责用 exact offer/attempt CAS 吸收该竞态。

领域 handler 返回给平台的结果固定为：

```text
ack_settled(code)     // durable success/failed/retry_scheduled/noop/poison receipt 已提交
unsettled             // DB 不可用、deadline 或尚无 durable settlement
identity_conflict     // exact transport ID 的 fingerprint/identity 不一致
adapter_mismatch      // binary/registry/queue closure 不成立
```

- `ack_settled` 才向 Oxana 返回 success；code 仅用于 bounded metric，不改变 transport ack 语义。
- `unsettled` 向 Oxana 返回 handler error并形成 `handler_unsettled` terminal tombstone；delivery worker 的 `max_retries=0` 不做 handler retry。领域只能在 owner 三态检查后推进 successor offer，不能重投已 tombstoned 的同 identity。
- `identity_conflict` 与 `adapter_mismatch` 都触发 runtime cancellation/readiness failure、都不 ACK，也不能写业务 target failed；二者必须保留不同 error code、metric 和 audit evidence，禁止互相映射。
- wrapper 从收到 envelope 到首次领域 begin 必须有硬 deadline，之前不得调用 provider、object store 或其它业务外部依赖。进程存活但卡在 begin 前时，不能假设已有 business lease 可供 repair。
- publisher accepted settlement 与 inbound begin 都必须是单调幂等；平台只传递 observed receipt，不替领域决定二者的数据库锁序或状态转换。

## 7. 故障责任矩阵

| 故障 | 正确行为 |
| --- | --- |
| 业务事务提交后 Redis 不可用 | durable intent 保持 ready，稍后重新 offer |
| consumer 在 publisher settle 前或 publisher timeout 后开始 | 传 actual receipt 给领域；领域从 current ready/offering/observing 单调 begin，publisher 后续 settle 幂等 |
| Redis accepted 后 publisher 在 DB settle 前崩溃 | consumer 可先建立 durable business claim；否则 offer lease 到期后重投同 identity |
| worker 取走消息后进程死亡 | Oxana resurrection；业务 lease 同时阻断旧 owner 发布 |
| handler 因 DB 不可用返回 unsettled | 写 `handler_unsettled` terminal tombstone；owner none时 successor，fresh时等待 lease，expired时 reap 后 successor |
| 进程仍活且已有 business owner 的任务卡死 | 领域按业务 heartbeat/lease repair 终结旧 owner，再产生 successor offer |
| processing receipt 超过 consumer-start deadline 且始终没有 business owner | 领域在 max observing age 后可 CAS successor offer并持久化 exact retirement obligation；旧 delivery 晚到由业务 CAS noop |
| queued receipt 长时间 present | 不因单条 age 盲目复制到同一拥塞 lane；报告 queue stall/readiness，由领域冻结的 admission/backpressure 合同处理 |
| receipt present 且 business owner fresh | 不创建重复 offer，只延后下一次 probe |
| 领域已推进 successor 或进入吸收态 | bounded retirement pump 调用 `retire_exact`；Redis 不可用时保留 durable obligation，不能靠 TTL 丢弃旧 receipt |
| Redis volume 丢失或 exact transport ID absent，且无 fresh owner | 领域从 PostgreSQL intent 推进新 offer；若旧 historical identity 后续重新 active，领域只 durable noop、reopen retirement并再次 `retire_exact` |
| 同 transport ID 的 fingerprint/queue/task 不同 | identity_conflict，全局 not-ready/fatal；不接受 duplicate、不 poison 业务 target |
| 旧 offer 晚到 | consumer 对 offer、gate epoch、generation、owner 做 CAS 后 noop |
| binary/queue registry/handler closure 不一致 | adapter_mismatch，全局 not-ready/fatal；不 poison 业务 target |
| PostgreSQL 中单条 immutable business payload/identity 损坏 | 领域 durable poison + typed target terminal/告警，不无限重试；不得与 Redis identity_conflict 混同 |

## 8. Retention、安全、可观测性与资源

Upstream Oxana 2.1.3 会按 job `created_at` 在 7 天后直接删除 jobs hash，而不证明 queue/processing membership 已终结；该行为不满足本文 receipt 合同。受控 patch 必须固定：

- active `queued|processing` receipt 不得仅因达到 7 天而删除；
- terminal `ack_settled|handler_unsettled|killed|invalid_envelope` 与 retired 分别在原子 transition 中删除 active receipt并写类型明确的 bounded tombstone，dead/audit 展示使用独立 bounded retention；
- orphan cleanup 必须先原子证明没有 queue/processing membership；发现 hash-only、多 membership 或 fingerprint 损坏时计入 invariant failure 并触发 readiness，不能静默把 `present` 变成 `absent`；
- policy verifier 必须约束 tombstone/dead/audit retention、probe interval 与可观测窗口，但不得用 TTL 代替 active receipt lifecycle；
- fresh deploy 不保留 upstream 7 天 active-receipt 兼容分支。

- queue/task/版本/lane 使用 allowlist，不允许 unknown-task fallback；
- payload、日志、metric 不记录文档内容、secret 或完整 snapshot bytes；
- transport adapter 使用短 enqueue deadline；外部 Redis I/O 不得发生在业务 PostgreSQL 事务内；
- queue depth 只表示 transport 状态，不能替代 durable dispatch backlog；
- 平台指标至少提供 queue depth、oldest queued/processing phase age、resurrection count、offer latency、duplicate-equivalent、identity-conflict、unavailable/rejected 和 receipt-invariant failure count；
- 领域另外提供 ready backlog、oldest due intent、business lease 与 poison 指标；
- Redis/Oxana 测试容器和 volume 在验收命令结束后立即删除。

## 9. 平台验收

1. allowlisted queue/task/policy version 正反向 closure；pure `prepare` binary Rust/SQL golden、字符/长度/payload/policy 篡改负例；
2. atomic offer 在 jobs write、queue membership 和 response 丢失的每个故障点都只得到完整 receipt 或无 receipt；
3. 高并发同 identity 只产生一个 membership；相同 fingerprint 返回 duplicate-equivalent，不同 fingerprint/queue/task 返回 identity-conflict；
4. public probe 验证 exact fingerprint 并区分 queued/processing/terminal/retired/absent；`phase_age_ms` 同 Redis 时钟计算，hash-only、多 membership 和损坏 envelope 不得返回 present；
5. dequeue、四类 terminal、`retire_exact` 与 resurrection 的每个命令边界均保持 receipt invariant；offer/retire 与 retire/finish/resurrection 并发后 retired identity 不复活；
6. worker crash 后 in-flight job 原子 resurrection，phase 重置 queued；
7. 相同 hostname/PID、不同 boot UUID 的新进程不会吞掉旧 processing owner，Storage clones 与连续 runtime identity 符合全生命周期合同；
8. `max_retries=0` 不阻止 `resurrect=true` 的进程崩溃恢复；
9. consumer-before-publisher-settle、ready late consumer、`handler_unsettled` terminal、ack-settled、identity-conflict 和 adapter-mismatch 的 inbound mapping 可重复收敛；
10. processing 卡在领域 begin 前会形成有界、可观察的 stall outcome，不依赖尚不存在的 business lease；queued backlog 不因 age 产生无界 duplicate；
11. active receipt 超过 upstream 7 天阈值仍可 probe/execute/resurrect；30-day retired/terminal tombstone、7-day Bid replay、offer lease/skew margin verifier 与 dead/audit retention 独立生效；
12. adapter/业务代码均不读取 `oxanus:*` 私有 key；
13. Redis volume 清空后平台不声称恢复业务，由领域 durable-dispatch 验收补足；
14. bounded payload、enqueue deadline、lane concurrency 与日志脱敏；
15. legacy `Storage::enqueue`/unique `Skip` 行为回归不变，durable public path 独立验收；
16. fresh Redis/Compose 实测并保存 queue registration、故障步骤和结果，Redis/DB 正负时钟偏移不影响恢复决定。

本地合同测试、fresh Compose、部署和 runtime accepted 必须分别报告。
