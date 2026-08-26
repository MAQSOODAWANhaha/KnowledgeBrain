# 招投标 Durable Dispatch 与失败恢复

| 项 | 值 |
| --- | --- |
| 状态 | clean-slate V1 已批准实施基线；待 PR8A～PR8F 实施与验收 |
| 所有者 | Bidding |
| 传输依赖 | [`../platform/queue-runtime.md`](../platform/queue-runtime.md) |

本文是招投标 DB→Redis 投递、业务 lease 恢复和 fanout 原子性的唯一活动定义。旧 `system:live-recovery:v1` 两跳恢复、泛化 housekeep 扫描和 commit 后 best-effort enqueue 均被本方案替代，不保留兼容路径。

## 1. 目标与非目标

目标：

- 业务 base/typed target、dispatch intent 与 ready state 同一 PostgreSQL 事务提交；
- API 成功不依赖 Redis 在线，也不存在“target 已提交但恢复系统不知道”的窗口；
- 正常投递和失败恢复使用同一个 durable intent；
- 直接 offer 最终业务 delivery，不先 enqueue recovery job；
- Redis 丢失、重复、延迟和 worker 崩溃只能造成重复 delivery，不能造成重复有效发布；
- snapshot、typed generation/watermark、gate epoch、owner/attempt 全部由 PostgreSQL fencing；
- matching 0..N fanout 原子产生 N 个 target 与 N 个 dispatch intent；零 route 是成功的空 fanout。

本 module 不实现通用队列、任意 DAG、跨领域 scheduler、provider 调用或业务内容生成。Oxana 只通过平台 `WorkTransport` adapter 使用。

## 2. 深 module interface

外部 interface 固定为两个入口：

```text
stage(tx, BidTargetRef) -> DispatchId
run(shutdown) -> Result<(), DispatchUnavailable>
```

- `stage` 为 crate-private，只能由所属 target 的受检 mutation 在现有事务中调用；API/worker 不得在 commit 后直接调用 Redis。
- `run` 由 worker composition root 启动，内部运行 due-intent pump、delivery consumer 和 bounded target repair。
- `drain/reconcile/claim/heartbeat/settle/release/fail` 都是 implementation，不向 route、业务 application service 或测试暴露六步状态机。

调用方只提供 target identity。`stage` 必须从事务内 target 读取并冻结 kind、project、typed generation/watermark、contract version、lane、fence hash 和 dispatch semantics snapshot；调用方不能传 snapshot JSON 或自行构造 Redis payload。

## 3. Durable 数据模型

### 3.1 Stable async target identity

所有可异步执行的招投标 target 先登记到 immutable `bid_async_targets`：

```text
id
project_id
target_kind
created_at
UNIQUE(id,target_kind,project_id)
```

`document_conversion|extraction_target|matching_schedule|matching_job|attachment_preparation|submission_render` 各自的 typed target 表以同一个 `id` 作为 PK/FK，并通过 constant `target_kind`、`(id,target_kind,project_id)` composite FK 和 deferred exact-one verifier 证明一个 base target 恰有一个 typed extension。业务状态、typed generation/watermark、claim 和结果仍在 typed target 表，不塞进 base 表。

base identity 固定为 `(target_kind,id)`；UUID 每个异步执行 identity 唯一，generation/watermark 只作为 typed fence，不重复充当 identity。document conversion 每个 generation 创建新的稳定 target ID，`BidDocument` 只保存 current conversion target pointer；重试不再原地复用 document ID 充当多个 target identity。其它现有 job/target ID 直接成为 base target ID。terminal/superseded/poisoned target 不重开；人工 retry 或新 mutation 创建 successor target identity。

target、typed extension、dispatch intent 与 ready state 必须同一事务提交。普通 API/worker 无权单独插入 `bid_async_targets`，也不能创建没有 typed extension、dispatch intent 或 state 的孤儿。

catalog 约束固定为：

- base composite unique 覆盖 `(id,target_kind,project_id)`；每张 typed 表保存同 project 与 constant kind，并以 composite FK 回指 base；
- deferred constraint verifier 在 base、六张 typed 表、intent 和 state 的 `INSERT/UPDATE/DELETE` 后都验证同一 base 在 commit 时恰有一个 typed extension、一个 intent 和一个 state；整 aggregate retention delete 时 base 已不存在可以通过，单独删除/换 kind/增加第二个 extension 必须失败；
- intent 对 `target_id` 使用 `UNIQUE`，state 以 `dispatch_id` 作为 PK/FK；上述双向 deferred verifier 提供反向存在性，不能只依赖这些“至多一个”的 UNIQUE/FK；
- verifier 使用固定 `search_path` 的受控函数，普通 API/worker 只执行所属 mutation/stage function，没有这些表的直接 DML 权限；
- `SET CONSTRAINTS ALL IMMEDIATE` 必须能拒绝零 extension、多 extension、project/kind mismatch、缺 intent、缺 state、单删 intent 和单删 state。

### 3.2 Immutable intent

`bid_dispatch_intents` 至少保存：

```text
id
target_id, target_kind, project_id
contract_key, contract_version
physical_lane
target_fence_sha256
dispatch_semantics_snapshot_id
created_at
```

`(target_id,target_kind,project_id)` 使用 composite FK 指向 `bid_async_targets`。dispatch 唯一身份为：

```text
(target_kind, target_id)
```

同 identity、同 fence 的 `stage` 幂等返回原 `dispatch_id`；同 identity、不同 fence 返回 `DISPATCH_IDENTITY_CONFLICT`。intent immutable，不保存可变 owner/heartbeat。

每种 target 的受检 stage function 必须证明 typed target 存在、属于同 project、typed generation/watermark 合法、snapshot FK 完整且当前可投递。最终 schema 用 base-target FK、typed extension FK、UNIQUE 和 deferred verifier 保证每个 target 恰有一个 intent/state；runtime auditor 不能代替 catalog 约束。

每个 typed adapter 定义唯一 `TargetFenceV1` canonical bytes：固定包含 schema version、target kind/ID、project、generation/watermark、全部 immutable snapshot/source identity 和 target-specific policy identity，禁止额外键或 live value。Rust builder 与 DB verifier 必须独立重算同一 `target_fence_sha256`；stage、consumer begin、repair 和 publish 均验证该 hash，不能只比较若干 UUID。

### 3.3 Mutable state 与 attempts

`bid_dispatch_states` 与 intent 一对一：

```text
status = ready|offering|observing|terminal|superseded|poisoned
next_offer_at
offer
offer_gate_epoch
offer_claim_token, offer_claimed_by, offer_lease_expires_at
expected_transport_id
expected_receipt_fingerprint
current_offer_attempt_id
active_receipt_attempt_id
last_transport_code
terminal_code, completed_at
```

`bid_dispatch_attempts` 每次外部 offer/probe 调用一行，至少保存 `id,dispatch_id,offer,attempt_seq,attempt_kind,gate_epoch,runtime_governor_generation,claim_token,started_at,settled_at,outcome,transport_id,receipt_fingerprint,receipt_phase`；行先由 claim 创建，再只允许一次 CAS finalize，finalize 后不可改写或复用。`UNIQUE(dispatch_id,offer,attempt_seq)` 防止覆盖历史，另有 `UNIQUE(dispatch_id,offer,id)` 供 state composite FK 使用。claim 创建 attempt 后把 `current_offer_attempt_id` 指向它；在同一 offer 内，state 的 `active_receipt_attempt_id` 只可由 null 单调写成该 current attempt，或验证为同一值；跨 offer 只能由下述 `advance_offer` 清空。consumer 不得从多条历史 attempt 中挑选一个 receipt。多次 probe 各写一条 bounded event，或写入独立的 bounded probe event partition，不反复覆盖 offer receipt。

`bid_dispatch_inbound_receipts` 保存所有可向平台返回 `ack_settled` 的 durable 证明，至少包含 `settlement_key,dispatch_id,offer,gate_epoch,transport_id,receipt_fingerprint,outcome_code,business_attempt_id,terminal_receipt_id,offer_advanced_settlement_id,settled_at`。`settlement_key` 是 exact delivery identity、outcome code 及其引用的 business attempt/terminal receipt/offer-advanced settlement/gate epoch 的 canonical SHA-256；重复相同语义 insert-or-read，同一 transport receipt 的不同合法结算语义各自保留。所有 success/failed/retry_scheduled/noop/poison ACK 必须先在同一事务写入或复用 exact inbound receipt；DB 写失败只能返回平台 `unsettled`。该表属于 dispatch retention aggregate，不能绕过审计窗口独立删除。

offer claim 使用 DB time、`FOR UPDATE SKIP LOCKED`、固定 batch 和短 lease。claim 在 Redis I/O 前原子写 `status=offering`、claim token、`offer_lease_expires_at`、由 unique identity 唯一导出的 `expected_transport_id`，以及按平台 canonical envelope 合同重算的 `expected_receipt_fingerprint`，并令 `next_offer_at=offer_lease_expires_at`；Oxana adapter 必须原子创建/返回该 exact ID/fingerprint，不允许 enqueue 后才产生 PostgreSQL 无法预知的随机 ID 或内容身份。expired-offering 由同一个 bounded pump 或 `(offer_lease_expires_at,id) WHERE status='offering'` partial index 精确回收到 `ready`，保持同一 offer 并 finalize 旧 enqueue attempt。Redis enqueue deadline 必须显著短于 offer lease，因此 publisher 不运行第二套长时间 heartbeat；超时或 lease lost 只释放/丢弃当前 offer。

`offer` 只在业务 retry、target-local repair、gate epoch rebase，observing probe=`absent` 且 owner=`none`，或 probe=`present(processing)`、owner=`none` 且超过 `consumer_start_deadline/max_observing_age` 时单调增加。`present(queued)+none` 表示 lane backlog，不复制 delivery；dispatcher 降低 admission/readiness 并继续 bounded probe。probe=`present` 且 owner=`fresh` 由 business lease 控制；owner=`expired` 必须先精确 reap。probe=`unavailable` 保持 observing，不猜测丢失。尚未得到可靠 Redis receipt 的 unavailable/timeout 只释放当前 offer，并且只能在相同 `offer_gate_epoch` 下重试同一 unique identity；即使第一次请求其实已被 Redis 接受，Oxana duplicate 与 consumer CAS 仍保持幂等。gate epoch 改变后禁止重试旧 identity，必须先完成 §4.5 的 epoch rebase 并使用 `offer + 1`。

所有 `offer + 1` 路径只能调用一个内部 `advance_offer(reason,new_gate_epoch)` 原语。它在统一锁序下同事务追加 `offer_advanced` settlement，finalize/reap 所需的 exact 旧 attempt，把旧 active receipt 留作不可变历史，推进 offer/epoch，并清空 `offer_claim_token/claimed_by/lease_expires_at/expected_transport_id/expected_receipt_fingerprint/current_offer_attempt_id/active_receipt_attempt_id`，最后设置 `status=ready,next_offer_at=due`。任何转换不得只更新数字 offer，防止 composite FK 指向旧 receipt 或新 offer 复用旧 transport identity。

### 3.4 Minimal delivery

Redis payload 固定为：

```text
schema = bid-delivery/v1
dispatch_id
offer
lane_key
payload_version = 1
```

unique identity：

```text
bid:delivery:v1:<dispatch_id>:<offer>
```

Bid V1 的 `expected_transport_id` 由该 unique identity 按平台 canonical transport identity 合同确定；`expected_receipt_fingerprint` 由完整 canonical offer envelope 确定。同 identity/envelope 必须在 Redis I/O 前得到相同 ID/fingerprint，Oxana create/duplicate、probe 和 handler context 都必须返回并验证这两个值。若平台 adapter 不能预先确定 exact ID/fingerprint，本方案不得切换到该 adapter。

`lane_key` 从 intent 的 allowlisted `physical_lane` 精确复制。消息不携带 business snapshot、project、generation、actor、route 或 object key；consumer 按 `dispatch_id + offer + lane_key` 回 PostgreSQL begin，并拒绝 lane mismatch。

queue registry 固定为四个一对一 route，避免一个 task type 隐式落入多个 physical queue：

| task type | payload schema | physical queue |
| --- | --- | --- |
| `bid:delivery:convert:v1` | `bid-delivery/v1` | `bid-convert-v1` |
| `bid:delivery:extract:v1` | `bid-delivery/v1` | `bid-extract-v1` |
| `bid:delivery:matching:v1` | `bid-delivery/v1` | `bid-matching-v1` |
| `bid:delivery:render:v1` | `bid-delivery/v1` | `bid-render-v1` |

四个 typed wrapper 共用同一 canonical payload decoder 和 `DurableBidDispatch` handler interface；unknown task、task/queue/lane mismatch 与 fallback-to-default 全部拒绝。

## 4. 状态机与顺序

### 4.1 正常路径

```text
domain mutation
  -> 创建 target 或 successor target
  -> stage immutable dispatch intent + ready state
  -> commit
  -> NOTIFY hint / indexed polling
  -> claim due offer
  -> WorkTransport.offer(final delivery)
  -> publisher accepted CAS 或 consumer receipt CAS（先到者）
  -> observing + consumer begin CAS
  -> target executor claim/heartbeat/execute/publish
  -> target terminal + dispatch terminal
```

`LISTEN/NOTIFY` 或进程内通知只降低延迟；通知丢失不影响正确性。partial index polling 是 durability fallback，不允许每五分钟扫描六张业务表作为主发布路径。

### 4.2 Dispatch state transition table

所有状态变化只允许由下表受检函数执行；表中 CAS 均使用 DB time。`terminal|superseded|poisoned` 是吸收态，不能回到 `ready`。target、business attempt 与 dispatch 同时变化时必须是一个事务。

| 事件 | 前置与 CAS | 原子结果 |
| --- | --- | --- |
| stage | base/typed target 可投递且 exact-one 完整 | 创建 intent；state=`ready,offer=0,offer_gate_epoch=current,next_offer_at=now` |
| claim offer | state=`ready`、due、gate=`open`、state epoch=current | 插入 enqueue attempt；state=`offering`；写 token/lease/expected transport ID+fingerprint/current attempt/`next_offer_at=lease expiry` |
| offer accepted/duplicate | exact token/offer/epoch/expected transport ID+fingerprint，offer lease 未过期，gate 仍 open/current | insert-if-null-or-equal receipt；state=`observing`，指向 exact `active_receipt_attempt_id`，清 offer claim；已由 consumer 写入同值时为幂等成功 |
| offer unavailable/timeout | exact token/offer/epoch，lease 未丢失 | finalize attempt；state=`ready`，保持 offer/epoch，按 frozen backoff 设置 due |
| offer lease expired | state=`offering`、exact old token、DB time 已过 lease | finalize attempt=`lease_expired`；state=`ready`，保持 offer/epoch |
| adapter/registry mismatch | exact current offer claim | finalize/release attempt；state=`ready`，保持 offer；`run` global fatal/not-ready |
| transport identity conflict | offer/probe/consumer 返回 exact transport ID 的不同 fingerprint，或 adapter 明确返回 `identity_conflict` | 不修改/poison typed target；offering 时 finalize/release claim 并保持同 offer ready，observing 时保持 exact state/receipt；`run` global fatal/not-ready |
| payload rejected/contract poison | exact current offer claim且 immutable item 自身损坏 | typed target failed；dispatch=`poisoned`；同事务 terminal receipt |
| probe queued + owner none | state=`observing`、exact active receipt/offer/fingerprint | 保持 observing，延后 bounded probe；报告 lane stall/readiness，禁止为同一拥塞 lane 复制 delivery |
| probe processing + owner none | 同上；receipt phase age 超过 consumer-start deadline | 未到 `max_observing_age` 时延后 probe；达到上限时调用 `advance_offer(processing_stalled,current_epoch)` |
| probe present + owner fresh | 同上 | 保持 observing；由 business heartbeat/publish 结算 |
| probe present + owner expired | 同上；锁定 exact business attempt | attempt `running->reaped`、target `running->pending`，再调用 `advance_offer(owner_reaped,current_epoch)` |
| probe unavailable | 同上 | 保持 observing，不改变 offer |
| probe absent + owner none | 同上；target 仍 pending、没有 active attempt | 调用 `advance_offer(receipt_absent,current_epoch)` |
| probe absent + owner fresh | 同上 | 保持 observing；不得产生新 offer |
| probe absent + owner expired | 同上；锁定 exact business attempt | attempt `running->reaped`、target `running->pending`，再调用 `advance_offer(owner_reaped,current_epoch)` |
| consumer before publisher settle | exact transport context/envelope；state 为 exact `offering|observing`；transport ID/fingerprint 等于预写 expected 值 | 与 publisher 共用 insert-if-null-or-equal receipt CAS，把 state 单调推进/保持 `observing`，再创建 exact business claim；不等待 publisher 二次结算 |
| consumer begin | exact `offering|observing` offer/epoch/expected transport ID/fingerprint/receipt，target pending、owner none、全部 fence 成立 | receipt 与 business claim 在同一事务成立；target=`running`；dispatch=`observing` |
| duplicate delivery | exact current offer，但同 target 已有 fresh owner | 写幂等 inbound `noop_duplicate` receipt；零 target/dispatch 状态变化，然后返回 `ack_settled` |
| business retry | exact fresh business claim，gate/fence 仍成立 | 旧 attempt 终结；target=`pending`；调用 `advance_offer(business_retry,current_epoch)`；写引用旧 attempt、旧 receipt 与 offer-advanced settlement 的 inbound `retry_scheduled` receipt，然后返回 `ack_settled` |
| business success/deterministic failure/exhausted | exact fresh business claim，gate/fence 仍成立 | target 与 dispatch 同事务进入相应 terminal，冻结 terminal receipt 与 exact inbound receipt，然后才返回 `ack_settled` |
| target cancelled/superseded | domain mutation 持有 exact target fence | 终结/失效 active business attempt；target 与 dispatch 同事务=`superseded`；successor 使用新 target ID |
| epoch rebase | nonterminal state epoch != current open epoch | 按 §4.5 处理 owner；必要时 reap 后调用 `advance_offer(gate_rebase,current_epoch)` |

offer claim 必须在 Redis I/O 前持久化 expected transport ID/fingerprint。consumer 从 Oxana handler context 取得 actual ID/fingerprint/phase，不从业务 payload 取信；当 exact message 先于 publisher settle 到达时，它与 publisher 调用同一个 insert-if-null-or-equal receipt CAS，将 `offering|observing` 单调推进/保持为 `observing`，并在同一事务取得 business claim。publisher 随后 settle 同值是幂等成功，不同值或旧 token/epoch 失败。这样 consumer-before-settle 不依赖 handler retry 次数，也不产生“消息已被 `max_retries=0` 移除但 receipt 尚未成立”的窗口；该原子 transport identity 是业务 target 切换前必须先由平台 [`WorkTransport`](../platform/queue-runtime.md) 和受控 Oxana adapter 验收的前置合同。

### 4.3 Consumer begin

consumer 在执行任何外部工作前，以一个事务验证：

- dispatch status=`offering|observing`、exact `offer`、`lane_key`、handler transport ID/fingerprint 与 state 预写的 expected 值；receipt 为 null 时只允许 insert exact expected ID/fingerprint，为同值时幂等复用，任何不同值均拒绝；
- maintenance gate=`open` 且 state/attempt gate epoch=current；
- target 仍是同 project 和 exact typed generation/watermark；
- target fence hash 和 immutable snapshot relation 未变；
- target 未 terminal/superseded/cancelled；
- business owner 分类为 `none`。

结果只能是：

```text
execute(exact business claim)
noop_duplicate
noop_gate_stale
noop_target_stale
noop_terminal
noop_transport_mismatch
fatal_transport_identity_conflict
poison_contract
```

`noop_gate_stale` 不授权旧 epoch message；dispatcher 随后按 §4.5 rebase。`noop_target_stale|noop_terminal` 必须在同一事务把 dispatch 结算到 `superseded|terminal`，不能留在 observing 后无限 reoffer。`noop_transport_mismatch` 只用于 wrong lane/task/offer/transport ID 的晚到或错误 envelope：记录 bounded security/contract event，由正确 intent 后续重新 offer，不把合法 target 置 failed。所有 noop 在返回成功前都按 §3.3 写入幂等 inbound receipt；DB settlement 失败返回平台 `unsettled`，不能 ACK。exact transport ID 对应不同 fingerprint，或 Oxana adapter 返回 `identity_conflict`，必须返回 `fatal_transport_identity_conflict` 并按平台 `adapter_mismatch` 停止 runtime/readiness；它不能降级成业务 noop，也不能 poison target。`poison_contract` 只用于 PostgreSQL immutable target/intent/fence 自身不成立，并与 typed target/dispatch terminal、inbound poison receipt 同事务提交。除 `execute` 外均不得调用 DocReader、provider、object store、renderer 或知识检索。

### 4.4 Business execution 与恢复

业务 execution lease 仍由所属 target 保存；它与短 offer lease 不可合并：

- worker 按冻结 lease 周期 heartbeat；
- publish/fail/cancel 必须验证 target、typed generation/watermark、attempt、claim token、owner gate epoch 和 lease 未过期；
- 确定性失败或 retry budget 耗尽写 target failed + dispatch terminal；
- retryable failure 精确终结旧 attempt，把 target 恢复为 pending，在同一事务调用 `advance_offer(business_retry,current_epoch)` 并写 inbound `retry_scheduled` receipt；提交后才返回 `ack_settled`，DB 失败返回 `unsettled`；
- 进程仍活但单个 task heartbeat 过期时，target-local repair 精确 reap 旧 owner，再推进新 offer；
- 旧 worker 恢复后不能 heartbeat、publish 或改变 dispatch state。

business owner 分类是 typed adapter 的单一受检判断：`none` 表示 target pending 且没有 running attempt；`fresh` 表示 exact running attempt 的 claim token、lease、typed fence 和 gate epoch 均为 current；`expired` 表示存在 running attempt但 lease 已过期、epoch 已失效或 exact fence 已失效。`expired` 不能等同 `none`，必须先在 target-local repair 中把 exact attempt 终结为 `reaped`。consumer begin 不隐式偷取 running attempt。

target repair adapter 与各业务 module 同目录，中央 dispatcher 只调 typed registry。禁止恢复时读取“当前 snapshot”替代 target 已冻结 snapshot。

### 4.5 Gate、dispatch semantics 与 runtime governor

- gate 非 `open` 时不 claim 新 offer；在途 worker 下一次 heartbeat/publish 因 epoch 不同而失效；
- `offer_gate_epoch` 绑定整个 offer identity，不是单次 Redis 调用的可替换标签；同一 offer 的 enqueue retries 必须使用相同 epoch；
- gate 以新 epoch 恢复 open 后，dispatcher 对旧 epoch nonterminal state 做 lazy bounded rebase：owner=`none` 时调用 `advance_offer(gate_rebase,current_epoch)`；owner=`expired` 时同事务精确 reap 后调用同一原语；owner=`fresh` 时保持等待；target terminal/superseded 时结算 dispatch；
- rebase 同时写 current epoch 和新 offer identity，所有旧 epoch envelope 因 offer/epoch mismatch 永久 stale noop；旧 publisher 的 accepted settlement 也因 token/lease/epoch CAS 失败；
- intent 冻结的 dispatch semantics snapshot 只包含单 offer 的 enqueue deadline、offer lease、consumer start deadline、probe interval、backoff 和 max observing age；verifier 强制 `enqueue_deadline < offer_lease / 3`、`0 < consumer_start_deadline <= max_observing_age`，且 interval/backoff 有界并不会形成 busy loop；
- poll interval、batch size、global/per-kind concurrency 属于 live runtime governor，不冻结到各 intent；它对所有 policy generation 使用同一 shared cap，验证 `poll_interval_ms=250..5000`、`batch_size=1..128` 且 concurrency 为小的正数硬上限；attempt 记录实际 runtime governor generation 供审计；
- semantics promotion 只在 maintenance，不能改写已有 intent；successor offer 使用 intent 冻结的 semantics contract，但 effective concurrency 始终不得超过 current shared governor；
- promotion/readiness 必须证明所有 nonterminal intent 引用的 semantics contract、lane 与 typed adapter 仍在 registry closure；引用归零前不能删除旧 decoder/verifier/lane。该要求补充而不改写平台 [`WorkTransport`](../platform/queue-runtime.md) 的 `offer/probe` interface。

## 5. Target adapters 与原子后继

| target kind | stable target | 冻结 fence | lane | 成功后的原子后继 |
| --- | --- | --- | --- | --- |
| `document_conversion` | immutable conversion target ID | document + conversion generation + conversion/feature snapshots | `bid-convert-v1` | converted source + extraction base/typed target + extraction dispatch intent/state |
| `extraction_target` | target + extraction generation | source artifact/target config/feature | `bid-extract-v1` | section/fact/clause publication；无异步后继时 terminal |
| `matching_schedule` | immutable schedule intent | watermark + config/feature/score/verifier snapshots | `bid-matching-v1` | manifest + 0..N matching jobs + 0..N dispatch intents/states |
| `matching_job` | immutable job ID | manifest/generation/watermark + four operation snapshots | `bid-matching-v1` | immutable report/current projection + terminal |
| `attachment_preparation` | preparation job ID | conversion/feature snapshots + source attachment identity | `bid-convert-v1` | frozen pages + owner references + terminal |
| `submission_render` | render job ID | manifest/render-contract aggregate snapshot | `bid-render-v1` | output artifact/current pointer + terminal |

所有“完成 A 后创建 B”的路径必须在 A 的 fenced settlement 事务中同时创建 B base/typed target、B dispatch intent 与 ready state。禁止先把 A 标为 completed，再在第二次 DB 调用或 commit 后 enqueue B。

matching mutation 必须在原 mutation 事务中使旧 current schedule/target/dispatch 同步 superseded，并创建 immutable successor `matching_schedule` target。schedule executor 原子创建 manifest 与 0..N fanout；零 route 时 manifest 与 schedule/dispatch 成功 terminal，不创建 child；不得通过扫描 dirty project 临时推算 generation 或 current snapshot。

## 6. Retry、错误与返回合同

### 6.1 Stage error

```text
TARGET_MISSING
TARGET_NOT_DISPATCHABLE
DISPATCH_FENCE_CONFLICT
DISPATCH_IDENTITY_CONFLICT
DISPATCH_CONTRACT_INVALID
```

业务 mutation 遇到 stage error 必须整体回滚，不能保留缺 typed extension、intent 或 state 的 target。

### 6.2 Delivery outcome

- Redis unavailable/timeout：attempt `released`，按冻结 backoff 设置 `next_offer_at`；仅当 gate epoch 未变时重试同一 offer identity；
- accepted/duplicate：attempt 记录 exact transport ID/fingerprint/phase receipt，state 以 composite FK 指向该 attempt 后进入 `observing`；
- adapter/registry/task→queue mismatch：释放当前 offer，`run` 返回 global fatal/readiness failure，不改变 target terminal state；
- transport identity conflict：按 §4.2 保持业务 target 与合法 receipt，不把 fingerprint 冲突当 duplicate、absent 或业务 poison；`run` 返回 global fatal/readiness failure；
- 单条 identity/version/canonical payload rejected：同一事务把 dispatch 置 `poisoned`、target 置 typed failed，并告警阻断完成声明；
- accepted 后 DB settle 失败：exact consumer 可独立 insert-if-null-or-equal receipt 并执行；consumer 未到达时 offer lease 到期后在同 epoch 重投同一 offer，epoch 已变则 rebase 到新 offer；
- observing probe=`present(queued)` 且 owner=`none`：保持并报告 lane stall，不复制 delivery；probe=`present(processing)` 且 owner=`none`：在 consumer-start/max-observing 时限内延后，超过上限后调用 `advance_offer(processing_stalled,current_epoch)`；owner=`fresh` 时保持，owner=`expired` 时先 reap 再调用 `advance_offer(owner_reaped,current_epoch)`；
- observing probe=`absent`：owner=`none` 才调用 `advance_offer(receipt_absent,current_epoch)`；owner=`fresh` 保持 observing；owner=`expired` 先精确 reap 再调用 `advance_offer(owner_reaped,current_epoch)`；probe unavailable 时保持 observing。

### 6.3 Business outcome

provider 内部 bounded retry、业务 target retry 与 Oxana transport retry 按 [`../platform/queue-runtime.md`](../platform/queue-runtime.md) 分层。Oxana handler `max_retries=0`、delivery `resurrect=true`；所有已 durable 结算的业务结果向队列返回成功，避免乘法重试。

## 7. 并发、性能与 retention

- due scan 访问 `bid_dispatch_states(status,next_offer_at,id)` partial index；expired offering 由同一 due 值或独立 `(offer_lease_expires_at,id) WHERE status='offering'` partial index bounded reclaim；
- 每批固定上限，`FOR UPDATE SKIP LOCKED`，global/per-kind concurrency 使用 current shared runtime governor 硬限制，不能按多个 intent snapshot 分别累计；
- target repair 按 typed adapter round-robin，每 kind 每轮最多一个冻结 batch；不得把六类 target 重新拼成无界中央 UNION，也不得让单一 backlog 饿死其它 kind；
- 新 intent commit 后发 bounded NOTIFY hint，polling 周期只作漏通知兜底；
- observing 到期不直接执行任务；先 probe exact transport receipt，再按 owner=`none|fresh|expired` 的唯一规则推进；consumer begin 的业务 CAS 决定唯一有效 owner；
- base target、typed extension、intent、state 与 inbound receipts 是一个 retention aggregate；只有 target 已 terminal、所有业务/current/audit/object owner 引用已释放且重放窗口结束时，retention role 才能在同一事务删除整个 aggregate，不能先删 intent/state/inbound receipt 留下 target；
- aggregate 内的非 active 历史 attempts 可使用更短的分批 retention，但 state 指向的 active receipt attempt、terminal receipt、ACK 证明和审计/重放窗口内记录不得删除；删除前必须先清除合法引用或冻结受检 terminal summary；
- 不创建 `system:live-recovery:v1` queue hop，不使用全局业务 housekeep，也不扫描 Oxana 私有 key。

跨表事务使用一个全局锁序：

```text
base/typed target
  -> exact business attempt
  -> dispatch state
  -> dispatch attempt
```

- consumer 可以先无锁读取 intent 找到 target ID，但加锁后必须按该顺序重新验证全部 fence；
- offer claim/settle 只锁 dispatch state→dispatch attempt，不得持有 dispatch lock 后再获取 target lock；
- probe/Redis/provider/object store/DocReader/renderer 调用期间不持 PostgreSQL row lock；probe settlement 重新按 target→dispatch state 顺序 CAS exact offer/receipt；
- 同一事务处理多个 existing target 时按 `(project_id,target_kind,target_id)` 稳定排序；fanout 新行也按稳定 identity 顺序插入；
- PostgreSQL `40P01` 只允许 bounded whole-transaction retry，不消耗业务 target retry budget，也不能拆开原子后继。

## 8. 权限与可观测性

- API role 只能通过所属 domain mutation 间接 stage，不能直接改 dispatch state/attempt；
- worker 只能调用 bounded offer claim/settle、delivery begin 和 exact target settlement；
- runtime/retention/maintenance role 权限分离；maintenance lane 不能代替 live dispatch；
- 日志字段限于 dispatch ID、target kind/ID、typed generation/watermark、offer、attempt、lane、error code、latency；
- 指标至少包含 ready/observing/poisoned 数、oldest due age、offer outcome、duplicate/stale noop、business lease repair、per-kind concurrency；
- readiness 验证 dispatch semantics/current runtime governor、所有 nonterminal contract/lane、typed target adapter registry、queue registration closure 和 Redis capability；queue depth 不能代替 dispatch backlog。

## 9. 删除矩阵

替换必须按 target 纵切；每类新路径落位时，同一改动删除该类旧 enqueue/recovery 分支。最终必须删除：

- `LiveRecoveryV1Job`、recovery kind/target/stage/snapshot mirror DTO 与 `enqueue_live_recovery_v1`；
- 旧 `bid:convert`、`bid:extract`、`bid:match-route:v1`、`bid:prepare-attachment:v1`、`bid:render-submission:v1` wire DTO/registry entry；
- worker recovery producer/consumer、candidate↔job mapping、recovery `dispatch_action` 和第二套 heartbeat；
- `storage::bid_recovery` 的公开 snapshot getter 与 `discover/claim/heartbeat/complete/release/fail` interface；
- `system_live_recovery_claims`、recovery-only policy/feature ledger 及对应 SQL payload/discover/claim/settle functions；
- API/worker commit 后 `enqueue_bid_*_with_snapshots` 和“durable intent remains pending”分支；
- dirty manifest 大 UNION、临时 generation/snapshot 推算和 `schedule_recovery_intent`；
- 无调用且只增加 watermark、不创建 schedule intent 的 mutation seam；
- 业务 housekeep/reaper sweep 和对 Oxana `oxanus:*` 私有 key 的手写 replay；
- 只验证旧两跳 recovery envelope/mapping 的 tests/fixtures/registry entries。

必须保留并接入新 module：

- immutable operation/source/render snapshots；
- target generation/watermark 和 business claim/attempt/heartbeat/publish fencing；
- immutable matching schedule intent；
- object upload staging、owner reference 和 retention；
- append-only业务 attempt、audit 和 terminal receipt；
- 四个物理 lane 的并发/backpressure。

最终 catalog、queue registry、producer/handler manifest、ACL、tests 和文档不得同时出现新旧两套恢复 owner。

## 10. 实施顺序

1. 在 fresh baseline 直接建立 async target identity、dispatch intent/state/attempt、semantics snapshot、runtime governor、受检函数和 ACL；
2. 先在受控 Oxana patch/平台 `WorkTransport` 落位并验收 exact predeclared transport ID、atomic unique offer、`probe`、boot instance identity 与 resurrection；在此之前不得切换任一业务 target；
3. 实现深 module 与 PostgreSQL store，联合验证 publisher/consumer insert-if-null-or-equal receipt CAS；
4. 替换 conversion/extraction，并删除其旧 enqueue/recovery；
5. 替换 attachment preparation/render，并删除其旧 enqueue/recovery；
6. 替换 matching schedule/0..N fanout/job，并删除 dirty-manifest/orphan-match recovery；
7. 删除私有 Redis replay，重生成 baseline checksum/catalog/queue closure，完成强制活库和 fresh runtime 验收。

任何阶段不得让同一 target 同时受旧 live-recovery 和新 dispatcher 驱动。由于最终部署是 fresh redeploy，不创建历史 payload converter、数据 backfill、双写或兼容 view。

## 11. 验收矩阵

### 11.1 原子性与 fanout

- base target+typed extension+intent+ready state 同事务 commit/rollback；FK/catalog/verifier 拒绝任一孤儿、多重 typed extension、project/kind mismatch、缺 intent/state，以及单删 intent/state；
- 同 identity 同 fence 幂等，不同 fence 冲突；
- 六类 `TargetFenceV1` Rust/SQL canonical golden 与任一字段篡改负例；
- conversion completion 与 extraction base/typed target+intent/state 原子；
- matching schedule 创建 manifest、0..N jobs 与等量 intents/states 原子；零 route 成功 terminal；
- attachment/render base/typed target 创建与 intent/state 原子；
- terminal identity 不可重开；successor 使用新 target ID，typed generation/watermark 只进入 fence，不参与通用 dispatch identity。

### 11.2 Transport 故障

- commit 时 Redis down，API 成功且 target 在 Redis 恢复后被 offer；
- NOTIFY 丢失仍由 indexed polling 在两个 current runtime-governor poll interval 内投递；
- offer claim 在 Redis I/O 前预写 expected transport ID/fingerprint；Oxana accepted、probe 和 handler context 必须 exact equal，任一不一致触发 global adapter mismatch/identity conflict；
- Redis accepted 后 consumer 先于 publisher settle：consumer insert-if-null receipt 并取得唯一 owner，publisher 同值 settle 幂等；publisher 先 settle 的反向顺序结果相同；
- publisher 在 accepted 后、DB settle 前崩溃，consumer 未到达时由 expired-offering/同 epoch identity 收敛；
- success/failed/retry_scheduled/noop/poison 只有在 exact inbound receipt 提交后才返回 `ack_settled`；DB settlement 失败返回 `unsettled`，并发重复 ACK 幂等复用相同 settlement key；
- exact transport `queued` 且 owner=`none`：不复制 delivery并触发 lane-stall readiness；`processing+none` 在 consumer-start/max-observing 时限后推进新 offer；present+fresh 不重复，present+expired 先 reap；
- Redis volume 清空后 probe absent，使所有未 terminal intent 按 owner 三态恢复；
- duplicate、乱序、旧 offer、wrong lane 和未知 payload version 稳定处理；
- 相同 hostname/PID 重启不需要读取 `oxanus:*` 私有 key。

### 11.3 Lease 与 fencing

- worker 进程死亡、进程存活但单 task 卡死、DB 连接丢失；
- offering publisher crash、offer lease expiry 与 exact token reclaim；
- business retry、owner reap、receipt absent、processing stall 和 gate rebase 全部只通过 `advance_offer` 推进，并清空旧 claim/expected/current/active 指针；
- heartbeat 与 publish race、lease expiry 边界、owner=`none|fresh|expired` 的三态矩阵、旧 owner 恢复；
- typed generation/watermark/snapshot 改变后旧 delivery noop；gate close/open 之间同 offer 不得换绑新 epoch，rebase 后旧 envelope 永久 noop；
- ended/cancelled/terminal target 不复活；
- begin/publish/repair/supersede/probe settlement 并发遵守统一锁序，无未处理 deadlock；`40P01` 不消耗业务 retry；
- bounded batch、current global/per-kind governor、混合新旧 semantics snapshot 和 backlog 超过一批。

### 11.4 Retry 与安全

- provider retry、business retry、transport resurrection 不相乘；
- retryable、deterministic、poison、exhausted 的 exact terminal code；
- API/worker/maintenance/retention role allow-deny；
- retention 不能单删 target/typed/intent/state/inbound receipt aggregate；active receipt、terminal/ACK receipt、audit/replay 引用阻止 attempt 清理；
- payload/日志 bounded 且不泄露内容或 secret；
- `rg`、catalog denylist 和四条 task→queue registry closure 证明旧业务 wire DTO、recovery/housekeep/private-key replay 已删除。

### 11.5 Fresh runtime

空 PostgreSQL/Redis/object volumes 下走完 convert→extract→matching→attachment preparation→render；在每个 target 的 commit/offer/begin/publish 故障点注入一次中断并证明最终收敛。测试容器结束后立即删除。

只有 implemented、locally verified、committed、pushed、deployed、runtime accepted 六层分别有实际证据，且 `phase_1d_runtime_complete=true` 的受审计 cutover 完成后，才能声明本方案完成；方案文档批准或本地测试不能提前提升状态。
