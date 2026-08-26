# 招投标 Durable Dispatch 与失败恢复

| 项 | 值 |
| --- | --- |
| 状态 | clean-slate V1 稳定版修订；待交叉 review，尚未实施 |
| 所有者 | Bidding |
| 传输依赖 | [`../platform/queue-runtime.md`](../platform/queue-runtime.md) |

本文是招投标 DB→Redis 投递、业务 lease 恢复和 fanout 原子性的唯一活动定义。旧 `system:live-recovery:v1` 两跳恢复、泛化业务 housekeep 扫描和 commit 后 best-effort enqueue 均被本方案替代，不保留 Bid 兼容路径。

## 1. 目标与非目标

目标：

- 业务 base/typed target、current dispatch head、initial intent 与 ready state 同一 PostgreSQL 事务提交；
- API 成功不依赖 Redis 在线，不存在“target 已提交但恢复系统不知道”的窗口；
- 每个 transport delivery identity 最多发起一次外部 enqueue，未知结果通过新 dispatch identity 恢复；
- Redis 丢失、部分写、重复、延迟、cleanup 和 worker 崩溃只会增加 historical delivery/noop，不能产生重复有效发布；
- target fence、typed generation/watermark、gate epoch、owner token、attempt 和 lease 全部由 PostgreSQL CAS；
- matching 0..N fanout 原子产生 N 个 target 与 N 个 initial dispatch；零 route 是成功空 fanout；
- provider retry、业务 retry、transport successor 三层所有权清楚且不相乘。

本 module 不实现通用队列、任意 DAG、跨领域 scheduler、provider 调用或业务内容生成。Oxana 只通过平台 `WorkTransport` adapter 使用；Redis 状态不参与业务正确性判断。

## 2. 深 module interface

外部 interface 固定为：

```text
stage(tx, BidTargetRef) -> DispatchId
run(shutdown) -> Result<(), DispatchFatal>
begin(ObservedDelivery {
  dispatch_id,
  payload_version,
  observed_job_id
}) -> BeginOutcome
```

- `stage` 为 crate-private，只能由所属 target 的受检 mutation 在现有事务中调用；API/worker 不得在 commit 后直接调用 Redis。
- `stage` 从事务内 typed target 读取并冻结 target fence、gate epoch、lane、delivery contract 和 dispatch semantics；调用方不能传 snapshot JSON 或自行构造队列 payload。
- `run` 内部拥有 transport admission、due scan、一次性 offer、delivery-start reaper 和 target-local repair。Redis unavailable/timeout 是单条 indeterminate outcome，不终止进程；registry/codec/queue closure mismatch 才返回 `DispatchFatal` 并使 readiness fail closed。
- `begin` 只由 `BidDeliveryV1Job` worker 调用，在任何 provider/object store/DocReader/renderer I/O 前完成 current-head、gate、fence 和 owner CAS。`observed_job_id` 必须原样取自 Oxana 2.1.3 公共 `JobContext.meta.id`，`payload_version` 必须取自实际解码 payload；禁止根据 dispatch ID 重新推导 observed ID 后再自证相等。
- claim/heartbeat/publish/advance/reap/settle 是 module 内部原语，不向 route 或普通业务 service 暴露可随意拼装的多步状态机。

## 3. Durable 数据模型

### 3.1 Stable async target identity

所有可异步执行的招投标 target 先登记到 immutable `bid_async_targets`：

```text
id
project_id
target_kind
created_at
PRIMARY KEY(id)
UNIQUE(id,target_kind,project_id)
```

六张 typed extension 固定为：

- `bid_async_document_conversion_targets`；
- `bid_async_extraction_targets`；
- `bid_async_matching_schedules`；
- `bid_async_matching_jobs`；
- `bid_async_attachment_preparations`；
- `bid_async_submission_renders`。

每张以 base `id` 为 PK/FK，保存相同 project 与 constant kind，并以 `(id,project_id)` composite FK 指向真实 executable domain target：分别是 `bid_document_conversion_targets`、`bid_extraction_targets`、`bid_matching_schedule_intents`、`bid_matching_jobs`、`bid_attachment_preparation_jobs` 和 `bid_submission_render_jobs`。extension 只证明 dispatch relation，不复制业务 status、generation、claim 或结果。

base identity 固定为全局唯一 `id`；`(id,target_kind,project_id)` composite unique 只用于 exact relation FK。generation/watermark 是 fence，不兼作 identity。document conversion 每个 generation 创建新 target ID，`BidDocument` 只保存 current conversion target pointer。terminal/superseded/poisoned target 不重开；人工 retry 或新 mutation 创建新的 target identity。

catalog 约束：

- base、typed extension、domain target、dispatch head 使用 composite FK 保证 project/kind 一致；
- deferred verifier 要求每个已切换 family 的 domain target 在 commit 时恰有一个 base、一个正确 extension 和一个 dispatch head；
- base/extension identity immutable，禁止 UPDATE 到另一个 aggregate；
- 普通 API/worker 无这些表的直接 DML，只能调用所属受检函数；
- `SET CONSTRAINTS ALL IMMEDIATE` 必须拒绝零/多 extension、project/kind mismatch、单删和 identity move/swap。

PR8B 可建立 dormant base/extensions 与空 conversion target 表，但不补建旧行、不双写、不注册第二 owner。PR8C～PR8E 每切换一个 family 时才安装其反向 verifier 并删除旧 producer/consumer。

### 3.2 Current head 与 immutable dispatch intent

每个 target 恰有一条 `bid_dispatch_heads`：

```text
target_id, target_kind, project_id
current_dispatch_id
revision
updated_at
PRIMARY KEY(target_id,target_kind,project_id)
UNIQUE(current_dispatch_id)
```

`current_dispatch_id` 以 composite FK 证明该 dispatch 属于同一个 target。head 是 transport generation 的唯一 current pointer；terminal 后保留最终 head 直到 replay/audit retention 结束。

每次 delivery generation 创建一条 immutable `bid_dispatch_intents`：

```text
id                         // dispatch_id
target_id, target_kind, project_id
predecessor_dispatch_id    // initial 为 NULL
dispatch_generation        // initial 为 0
successor_reason           // initial 为 NULL
gate_epoch
target_fence_sha256
dispatch_semantics_snapshot_id
physical_lane
task_type                  // bid:delivery:v1
payload_version            // 1
canonical_payload_bytes
canonical_payload_sha256
unique_identity
expected_oxana_job_id
created_at
```

关键约束：

- initial intent 为 `predecessor IS NULL AND generation=0`；successor 必须引用同 target 的 exact predecessor 且 generation=`predecessor+1`；
- `CHECK ((dispatch_generation=0)=(predecessor_dispatch_id IS NULL))`，并提供 `UNIQUE(id,target_id,target_kind,project_id,dispatch_generation)` 供 replacement exact FK；
- `UNIQUE(predecessor_dispatch_id)` 保证每个旧 dispatch 至多一个 successor；
- `UNIQUE(target_id,target_kind,dispatch_generation)`、`UNIQUE(expected_oxana_job_id)`，以及供其它 exact relation 使用的 `UNIQUE(id,target_id,target_kind,project_id)`；
- successor 使用新随机 UUID、新 unique identity 和新 job ID，旧 dispatch ID 永不复用；
- intent 全部 immutable，不保存 owner/heartbeat、transport phase 或 Redis receipt；
- `unique_identity` 固定为 dispatch UUID canonical lowercase，`expected_oxana_job_id` 固定为 `bid:delivery:v1/<dispatch_id>`。

application delivery codec 固定为：

```text
ASCII KBDL
u16be schema_version = 1
uuid_send dispatch_id = 16 bytes
u16be payload_version = 1
```

`canonical_payload_sha256` 是以上 bytes 的 lowercase SHA-256。Rust builder 与 SQL verifier 独立重算；它不是 Redis receipt fingerprint。`BidDeliveryV1Job` JSON 只携带 `dispatch_id,payload_version`，consumer 从 PostgreSQL 恢复全部业务上下文。

### 3.3 Target fence

每种 adapter 定义唯一 `TargetFenceV1`，固定包含 immutable target、project、generation/watermark、source/snapshot 和 target-specific policy identity。Rust builder 与 DB verifier 独立重算 `target_fence_sha256`；stage、begin、heartbeat 和 publish 都验证同一 digest。

V1 binary 为 `KBTF + u16be(1) + u8(kind_tag) + u16be(field_count)`，随后按固定顺序写 `[u8 type_tag,u32be length,value]`。type tag 固定为 `1=UUID(16 bytes)`、`2=nonnegative int8send(8 bytes)`、`3=SHA-256 bytes(32 bytes)`、`4=exact UTF-8`；不做 trim、Unicode normalization 或 locale 转换。六类 ordered fields 是唯一权威输入：

| tag | target kind | ordered fields |
| --- | --- | --- |
| 1 | `document_conversion` | `target_id,project_id,document_id,conversion_generation,original_object_ref,original_sha256,file_name,media_type,byte_length,conversion_snapshot_id,feature_snapshot_id,max_attempts,claim_lease_ms` |
| 2 | `extraction_target` | `target_id,project_id,document_id,source_artifact_id,conversion_generation,extraction_generation,source_markdown_sha256,source_byte_length,source_image_asset_set_sha256,converter_contract_version,target_config_snapshot_id,feature_snapshot_id,router_contract_version,policy_version,prompt_version,output_schema_version,expected_section_count,max_attempts,claim_lease_ms` |
| 3 | `matching_schedule` | `target_id,project_id,generation,mutation_watermark,matching_config_snapshot_id,feature_snapshot_id,score_policy_snapshot_id,verifier_policy_snapshot_id,retrieval_contract_version,retrieval_policy_sha256,max_attempts,claim_lease_ms` |
| 4 | `matching_job` | `target_id,project_id,manifest_id,manifest_content_sha256,generation,mutation_watermark,route_id,route_scope_sha256,matching_config_snapshot_id,feature_snapshot_id,score_policy_snapshot_id,verifier_policy_snapshot_id,max_attempts,claim_lease_ms,lease_policy_generation` |
| 5 | `attachment_preparation` | `target_id,project_id,attachment_id,attachment_revision,attachment_kind,source_object_ref,source_content_sha256,source_validation_sha256,media_type,byte_length,conversion_snapshot_id,feature_snapshot_id,preparation_contract_version,max_attempts,claim_lease_ms` |
| 6 | `submission_render` | `target_id,project_id,manifest_id,expected_manifest_sha256,render_job_snapshot_id,render_job_snapshot_sha256,render_config_snapshot_id,feature_snapshot_id,requested_by,idempotency_key_sha256,max_attempts,claim_lease_ms` |

以下 live 值不入 fence：status/current pointer、attempt count、claim token、owner/heartbeat/lease、published count、staging/output pointer、error/time、maintenance gate epoch 和 ObjectRegistry 可用性。它们在 live CAS 中验证。

### 3.4 Mutable state 与一次性 delivery attempt

每条 intent 恰有一条 `bid_dispatch_states`：

```text
status = ready|offering|awaiting_start|running|terminal|superseded|poisoned
next_offer_at
offer_claim_token, offer_claimed_by, offer_lease_expires_at
delivery_start_deadline_at
delivery_attempt_id
business_attempt_id
repair_requested_at
absorbing_settlement_id
absorbing_settlement_kind
last_transport_code
completed_at
```

状态 NULL matrix：

| 状态 | 必须存在 | 必须为空 |
| --- | --- | --- |
| `ready` | `next_offer_at` | offer claim、start deadline、delivery/business attempt |
| `offering` | exact claim/token/lease、delivery attempt、start deadline | business attempt、absorbing settlement |
| `awaiting_start` | finalized/consumer-first delivery attempt、start deadline | offer claim、business attempt、absorbing settlement |
| `running` | exact business attempt | offer claim、start deadline、absorbing settlement |
| `terminal` 或 `poisoned` | exact absorbing settlement、completed_at | 所有 due/claim/deadline/active attempt |
| `superseded` | exact `advanced` 或 `superseded` settlement、completed_at | 所有 due/claim/deadline/active attempt |

`bid_dispatch_delivery_attempts` 每个 dispatch 至多一行：

```text
id, dispatch_id UNIQUE
claim_token
runtime_governor_generation
started_at, lease_expires_at
settled_at
outcome = enqueue_returned|enqueue_indeterminate|consumer_started|publisher_lost
returned_job_id
error_code
```

claim transaction 提交时即插入 attempt 并进入 `offering`；从此 identity 视为已暴露，attempt 永远不能被 reclaim 为第二次 enqueue。publisher lease 过期只将同 attempt 结算为 `publisher_lost` 并进入 `awaiting_start`。consumer-first 可结算为 `consumer_started`。publisher 晚到结果写 bounded append-only `bid_dispatch_delivery_observations`，不能改变 attempt disposition、current head 或业务 owner。

### 3.5 Settlement、inbound outcome 与 evidence

`bid_dispatch_settlements` 保存每个 dispatch 的唯一 disposition：

```text
id, dispatch_id
settlement_kind = advanced|terminal|superseded|poisoned|cancelled
successor_dispatch_id
replacement_target_id, replacement_target_kind, replacement_project_id
replacement_initial_dispatch_id, replacement_dispatch_generation
reason_code
gate_epoch
business_attempt_id
created_at
UNIQUE(dispatch_id)
```

disposition XOR 固定为：

- `advanced` 只允许 `successor_dispatch_id` 非空；successor 必须属于同 target、generation=`old+1`，且 predecessor 反向指回 old dispatch；全部 replacement 字段为空；
- `superseded` 禁止 successor，必须填满 replacement target/dispatch 五元组，且 `replacement_dispatch_generation=0`。该五元组分别以 composite FK 指向新的 async target，以及 intent 的 `(id,target_id,target_kind,project_id,dispatch_generation)` unique key；intent 的 initial CHECK 因而同时证明 predecessor 为空。replacement 与 old target 同 project/kind、ID 不同；
- `terminal|poisoned|cancelled` 的 successor 与全部 replacement 字段均为空。

`UNIQUE(replacement_initial_dispatch_id) WHERE settlement_kind='superseded'` 防止一个新 target 吸收多个旧 current target。settlement 提供 `UNIQUE(id,dispatch_id,settlement_kind)`；state 持久化 `absorbing_settlement_id,absorbing_settlement_kind` 并以三列 composite FK 指回 exact settlement。state CHECK 规定：`superseded` 只接受 `advanced|superseded`，`terminal` 只接受 `terminal|cancelled`，`poisoned` 只接受 `poisoned`，nonabsorbing state 两列均为空。并发 reaper、consumer、gate rebase 和 replacement mutation 通过 predecessor、replacement 与 dispatch 唯一约束 insert-or-read 同一结果。

每次准备向 Oxana 返回成功前必须写或复用 `bid_dispatch_inbound_outcomes`：

```text
id, settlement_key
dispatch_id
observed_job_id
observed_payload_version
outcome_kind = business_success|business_failed|retry_scheduled|noop|poison
reason_code
business_attempt_id
dispatch_settlement_id
evidence_sha256
created_at
UNIQUE(settlement_key)
```

durable noop reason 固定为：

```text
duplicate_fresh_owner
owner_expired
historical_dispatch
target_terminal
target_stale
gate_stale
delivery_mismatch
```

重复相同语义使用 canonical `settlement_key` insert-or-read；不同语义不能覆盖已有 row。historical noop 必须保存并 FK 引用 old dispatch 的 exact absorbing settlement，不能把 cross-target `superseded` 伪装成 same-target `advanced`。known dispatch 但 job ID/payload 不匹配时先写 bounded `bid_dispatch_rejected_deliveries`，再写 `delivery_mismatch` noop；无法解析或不存在的 dispatch 只形成平台 dead/metric，current DB intent 仍由 deadline 恢复。

六张 typed settlement evidence extension 继续证明 normal/contract-poison outcome 与真实 domain target、finalized business attempt 和 result artifact 的 exact 关系。Rust/SQL golden 必须独立验证 evidence bytes/hash；transport returned、Redis timestamp、queue phase 和 hostname/PID 不进入业务 evidence。

### 3.6 Dispatch semantics 与 runtime governor

immutable semantics snapshot 至少冻结：

```text
initial_offer_delay_ms
offer_call_deadline_ms
publisher_lease_ms
delivery_start_timeout_ms
successor_backoff_base_ms
successor_backoff_cap_ms
historical_replay_window_ms
business_claim_lease_ms
```

所有值正数且有上限；publisher lease 覆盖正常 offer deadline，delivery-start timeout 大于 publisher lease；successor backoff 按 generation 指数增长并 cap，避免 lane 不可用时高频 duplicate。generation 超过告警阈值使 readiness 降级但不把基础设施故障改成业务 terminal。

poll interval、batch size、global/per-kind concurrency 属于 live shared governor。每批 `FOR UPDATE SKIP LOCKED` 且有硬上限；semantics promotion 只在 maintenance，不能改写已有 intent。所有 nonterminal intent 引用的 semantics、lane 与 typed adapter 在引用归零前必须保留 registry closure。

## 4. 状态机与恢复

### 4.1 Initial stage 与一次性 offer

| 事件 | CAS 前置 | 原子结果 |
| --- | --- | --- |
| stage initial | typed target 可投递且无 head | target/base/extension + generation 0 intent/state/head 同事务；state=`ready` |
| claim offer | current head、ready、due、gate open、transport admission 允许 | 插入唯一 delivery attempt；state=`offering`；同时写 claim lease 与 delivery-start deadline |
| enqueue returned | exact claim，仍 current offering | attempt=`enqueue_returned`，验证 job ID，清 claim，state=`awaiting_start` |
| enqueue indeterminate | exact claim，仍 current offering | attempt=`enqueue_indeterminate`，清 claim，state=`awaiting_start`；禁止同 ID 重试 |
| publisher lease expired | exact offering claim 已过期 | attempt=`publisher_lost`，清 claim，state=`awaiting_start` |
| late publisher result | attempt 已 finalize 或 dispatch 已非 current | append observation；不改 head/state/owner |

`ready` 是唯一可以调用 transport 的状态。进入 `offering` 后无论外部 I/O 是否真的发生，都没有返回 `ready` 的 transition。

### 4.2 Consumer begin

consumer 接收完整 `ObservedDelivery`，按全局锁序锁定 target、业务 attempt、head、dispatch 并重新验证：

1. payload version、observed job ID 与 intent expected job ID 完全相等；
2. dispatch 仍是同 target current head；
3. target pending 且 typed fence 仍相等；
4. maintenance gate open 且 epoch 等于 intent；
5. business owner 分类为 `none|fresh|expired_or_fenced`。

transition：

| 观察 | 结果 |
| --- | --- |
| current `offering` 或 `awaiting_start` + owner none | finalize 可选 delivery attempt=`consumer_started`，清 publisher claim/deadline，创建 business attempt，target/dispatch=`running` |
| current running + owner fresh | durable `noop/duplicate_fresh_owner`，不改变 owner |
| owner expired_or_fenced | durable `noop/owner_expired` 并设置 repair hint；可由同一受检原语 reap+advance，但当前 handler 不得偷取旧 token |
| dispatch 非 head 或已 superseded | 引用其 exact absorbing settlement（`advanced`、`superseded`、`terminal`、`poisoned` 或 `cancelled`）写 `noop/historical_dispatch` |
| target terminal/poisoned/cancelled | 写或复用对应 terminal noop |
| gate/fence stale | 写 noop 与 repair hint，不执行外部依赖 |
| known dispatch 但 job ID/version 不匹配 | rejected-delivery + `noop/delivery_mismatch` |

只有取得 exact business claim 的分支可以调用外部依赖。其它分支在 durable inbound outcome 提交后直接向 Oxana 返回成功。

### 4.3 Business execution、heartbeat 与 publish

- executor 只从 immutable target/snapshot 读取输入；Redis payload 不携带业务数据。
- 长时间 DocReader/provider/object/render 调用期间运行独立 background heartbeat；heartbeat 与 publish 都验证 target 仍为 executable/running、dispatch state=`running`、absorbing settlement 为空，以及 exact head、attempt token、lease、gate epoch 和 target fence。
- staging 外部产物在 publish 前登记 owner；lease-lost/fenced owner 不能绑定 current pointer，孤儿由 ObjectRegistry/retention 回收。
- success 或 deterministic failure：domain result、target terminal、dispatch terminal settlement、typed evidence 和 inbound outcome 同一事务；提交后才向 Oxana 返回成功。
- retryable failure：终结旧 business attempt、target 回 pending、调用 `advance_dispatch` 创建新 identity，并写 `retry_scheduled` inbound outcome；业务 attempt budget 只在这里消耗。
- DB commit 结果未知时 handler 返回 error；若事务实际已提交，任何晚到 delivery 只读取 exact absorbing settlement 并 noop。
- stale owner 恢复后 heartbeat/publish 必须因 token/head/lease/gate/fence 任一不等而失败，不能覆盖 successor 结果。

### 4.4 统一 successor 原语

所有未开始恢复、owner reap、业务 retry 和 gate rebase 只调用：

```text
advance_dispatch(old_dispatch_id, reason, new_gate_epoch, due_at)
  -> new_dispatch_id
```

原语必须在一个 transaction 中：

1. 按固定顺序锁 target、exact business attempt、head、old dispatch state；
2. 验证 old 仍是 current head 且尚无 absorbing settlement；
3. 对 fresh owner 返回 `OWNER_STILL_FRESH`，不得抢占；
4. 对 expired/fenced owner 精确终结旧 attempt 并清理其 claim；
5. 生成新 UUID 并插入 generation+1 immutable intent 与 `ready` state；
6. 以 `UNIQUE(predecessor_dispatch_id)` insert-or-read 并发唯一 successor；
7. old state=`superseded`，写 advanced settlement，target 回 pending，head 指向 successor；
8. 清 old due/claim/deadline/repair 字段并写审计。

delivery-start reaper 只处理 current `offering|awaiting_start` 且 DB 无 fresh owner的 row：

- deadline 未到不推进；
- deadline 到且 owner none：`advance_dispatch(delivery_not_started,...)`；
- owner fresh：归一为 running 或保持业务 owner，不推进；
- owner expired/fenced：精确 reap 后推进；
- Redis queue depth/get_job/stats 不得改变单条决定。

### 4.5 Gate 与 owner 三态

fresh owner 必须同时满足：exact current head、dispatch/gate/fence 相等、attempt running、token 相等且 DB lease 未过期。只要 gate epoch 改变或 head 改变，即使 heartbeat 时间尚新也属于 `expired_or_fenced`，不能继续发布。

gate 关闭时停止新 offer/begin 并阻断 heartbeat/publish；旧外部调用可结束但结果不能成为 current。gate 以新 epoch 开放后，none 直接 advance，expired/fenced 先精确 reap 再 advance。旧 epoch owner 不允许分类为 fresh 并无限等待。

## 5. Target adapters 与原子后继

| target kind | stable target | 冻结 fence | lane | 成功后的原子后继 |
| --- | --- | --- | --- | --- |
| `document_conversion` | immutable conversion target ID | document + conversion generation + conversion/feature snapshots | `bid-convert-v1` | converted source + extraction target/base/extension/initial dispatch |
| `extraction_target` | target + extraction generation | source artifact/target config/feature/router policy | `bid-extract-v1` | section/fact/clause publication；无异步后继时 terminal |
| `matching_schedule` | immutable schedule intent | watermark + config/feature/score/verifier/retrieval snapshots | `bid-matching-v1` | manifest + 0..N jobs + 0..N initial dispatches |
| `matching_job` | immutable job ID | manifest/generation/watermark + operation snapshots | `bid-matching-v1` | immutable report/current projection + terminal |
| `attachment_preparation` | preparation job ID | source attachment + conversion/feature snapshots | `bid-convert-v1` | frozen pages + owner references + terminal |
| `submission_render` | render job ID | manifest/render-contract aggregate snapshot | `bid-render-v1` | output artifact/current pointer + terminal |

所有“完成 A 后创建 B”路径必须在 A 的 fenced settlement 事务中同时创建 B domain target、base/typed extension、head、generation 0 intent 与 ready state。禁止先把 A 标 completed，再用第二次 DB 调用或 commit 后 enqueue B。

matching mutation 使用独立的 cross-target replacement 原语，不调用 same-target `advance_dispatch`。事务先锁 project/current schedule revision，再按全局锁序锁旧 nonterminal target、exact running business attempt、head 与 dispatch state；随后创建新 schedule domain target、base/extension/head/generation-0 intent/state。若旧 target nonterminal，则精确终结旧 attempt 为 `superseded`、清 owner/lease，把旧 target/state 标为 superseded，并写以 replacement 五元组指向新 target/initial dispatch 的 `superseded` settlement。旧 head保留指向旧 final dispatch用于 audit，新 target拥有自己的 head。若旧 target 已有 absorbing settlement，则保持其终态不可变，只创建由新 watermark 驱动的新 target；不存在可晚到执行的旧 owner。

replacement mutation 与 heartbeat/publish 使用同一锁序和 CAS。mutation 先提交时，旧 owner 因 target/state/absorbing 检查失败；publish 先提交时，mutation 观察旧 absorbing terminal 并不得改写它。并发 mutation 只能有一个 current revision CAS 成功，失败方不得留下 target 或 intent。旧 nonterminal delivery 晚到时引用 exact `superseded` settlement durable noop，不能执行 schedule。

schedule executor 原子创建 manifest 与 0..N fanout；零 route 时 manifest 和 schedule/dispatch 成功 terminal，不创建 child。

## 6. 错误与返回合同

### 6.1 Stage error

```text
TARGET_MISSING
TARGET_NOT_DISPATCHABLE
DISPATCH_FENCE_CONFLICT
DISPATCH_IDENTITY_CONFLICT
DISPATCH_CONTRACT_INVALID
DISPATCH_ADAPTER_MISMATCH
```

任一 stage error 使所属业务 mutation 整体回滚，不能保留孤儿 target/head/intent/state。adapter mismatch 触发 readiness fail closed，不降级成单条 target poison。

### 6.2 Transport outcome

- `enqueue_returned`：只记录 API 返回与 expected job ID 相等，不记录 accepted/inserted/duplicate；
- `enqueue_indeterminate`：错误 code bounded，仍等待 start deadline，禁止同 ID 重投；
- publisher result 晚到：只追加 bounded observation；
- pure prepare payload rejected：业务 transaction 回滚；
- runtime adapter/registry mismatch：global fatal/not-ready；已经暴露的 identity 仍只能由 deadline successor；
- transport health 不可用：未 claim 的 ready row 保持 ready，不创建 attempt；health 只是 admission hint，不是单条 membership proof。

### 6.3 Handler outcome

- durable business/noop/inbound settlement 已提交：`Ok(())`；
- DB unavailable、transaction 结果未知或尚无 durable outcome：worker error；
- `max_retries=0` 防止 Oxana handler retry 与 DB successor 相乘；
- `resurrect=true` 只作为加速器，不能延后或取消 DB deadline/lease 恢复。

## 7. 并发、性能与 retention

- due scan 分别覆盖 `ready next_offer_at`、`offering publisher lease expired`、`offering|awaiting_start delivery_start_deadline`、business lease expired 和 repair requested；每个条件有 partial index。
- 每批固定上限，`FOR UPDATE SKIP LOCKED`；global/per-kind successor 与 execution concurrency 由 current governor 限制。
- target repair 按 typed adapter round-robin，每 kind 每轮固定 batch，不能用无界中央 UNION 让大 backlog 饿死其它 kind。
- 新 ready intent commit 后发 bounded NOTIFY hint，polling 是漏通知兜底。
- queue backlog 无法与丢消息区分；successor 按 frozen exponential backoff 并受 lane/global rate limit。历史消息可能增加，但全部在 DB begin 前 noop。
- aggregate 包含 domain target/base/extension/head、全部 dispatch intents/states/delivery attempts/observations、business attempts、settlements、inbound outcomes/rejected deliveries/evidence。
- nonterminal aggregate、current head、replay 窗口内 historical dispatch 和 terminal audit 不得单删；终态且外部引用释放后按 retention 整体删除。
- Redis job/dead list、get_job 和 delete_job 不参与 retention proof。

全局锁序：

```text
project/current domain pointer（需要时）
  -> domain target/base/extension
  -> exact business attempt
  -> dispatch head
  -> dispatch intent/state
  -> settlement/inbound/evidence
```

- consumer 可先无锁定位 target，但加锁后按上述顺序重验全部 fence；不需要 project/current pointer 的原语从 domain target 开始，禁止反向补锁；
- Redis/provider/object/DocReader/renderer 调用期间不持 PostgreSQL row lock；
- publisher result settlement 不得在持 dispatch 锁时反向获取 target；late result 只追加 observation；
- fanout 多 target 按 `(project_id,target_kind,target_id)` 稳定排序；
- PostgreSQL `40P01` 只允许 bounded whole-transaction retry，不消耗业务 retry budget。

## 8. 权限与可观测性

- API role 只能通过所属 domain mutation 间接 stage，不能直接改 head/state/attempt；
- worker 只能调用 bounded offer claim/settle、delivery begin、heartbeat 和 exact target settlement；
- maintenance、runtime、retention role 分离；maintenance lane 不能代替 live dispatch；
- 日志字段限于 dispatch/target ID、kind、generation、lane、attempt、gate、reason code 和 latency，不记录 payload 内容或 secret；
- 指标至少包含 ready、offering/awaiting-start overdue、successor generation、enqueue returned/indeterminate、duplicate/historical noop、business lease repair、poison 和 per-kind concurrency；
- readiness 验证 Oxana registry 版本、transport/handler/lane closure、semantics/current governor、nonterminal target adapter 和 DB invariant；queue depth 不能替代 dispatch backlog。

## 9. 删除矩阵

每个 target family 纵切时，同一改动删除该类旧 enqueue/recovery owner。最终 Bid 路径必须删除：

- `LiveRecoveryV1Job`、recovery kind/target/stage/snapshot mirror DTO 与 `enqueue_live_recovery_v1`；
- 旧 `bid:convert`、`bid:extract`、`bid:match-route:v1`、`bid:prepare-attachment:v1`、`bid:render-submission:v1` wire DTO/registry entry；
- worker recovery producer/consumer、candidate↔job mapping、recovery `dispatch_action` 和第二套 heartbeat；
- `storage::bid_recovery` 公开 snapshot getter 与 `discover/claim/heartbeat/complete/release/fail`；
- `system_live_recovery_claims`、recovery-only policy/feature ledger 及对应 SQL 函数；
- API/worker commit 后 `enqueue_bid_*_with_snapshots` 和“durable intent remains pending”分支；
- dirty manifest 大 UNION、临时 generation/snapshot 推算和 `schedule_recovery_intent`；
- 无调用且只增加 watermark、不创建 schedule target 的 mutation seam；
- Bid 业务 housekeep/reaper sweep 及只验证旧两跳 recovery 的 tests/fixtures/registry entries；
- 旧方案的 transport receipt/probe/retire/tombstone/boot UUID/vendor Oxana 相关 schema、adapter 和 tests。

必须保留并接入：

- immutable operation/source/render snapshots；
- target generation/watermark 和 business claim/attempt/heartbeat/publish fencing；
- immutable matching schedule intent；
- object staging、owner reference 和 retention；
- append-only 业务 attempt、audit 和 terminal receipt；
- 四个物理 lane 的 concurrency/backpressure；
- Shared Platform 暂留的 `replay_orphaned_local_jobs`；它当前没有领域过滤，可能触碰 Bid transport membership，但 Bid 代码不得主动调用、读取其结果或依赖它恢复。独立 Knowledge Base/platform cutover 满足 [`queue-runtime.md`](../platform/queue-runtime.md) 第 5 节条件后再删除。

最终 Bid catalog、queue registry、producer/handler manifest、ACL、tests 和文档不得同时出现新旧两套业务恢复 owner。

## 10. 实施顺序

1. **PR8A — stable transport**：精确锁定 registry Oxana 2.1.3；实现 pure prepare、显式 job name/unique ID 和最薄 `offer` adapter；验证一次 adapter invocation 内 enqueue count 只能为 0 或 1（正常有效路径为 1）以及 `Skip/resurrect/max_retries=0` 真实语义；不 vendor、不 patch、不切换业务 owner。
2. **PR8B — dormant durable core**：fresh baseline 建立 base/extensions/head/intent/state/delivery attempt/observation/settlement/inbound/evidence/semantics/governor 与 ACL；用 synthetic aggregate 验证每 dispatch 最多一次 offer、same-target advance、cross-target supersede 和其余状态机，不激活真实 producer/consumer。
3. **PR8C — conversion/extraction**：安装 reverse verifier，原子切换 owner，删除该 family 旧 enqueue/live-recovery；验证 conversion→extraction 后继同事务。
4. **PR8D — attachment/render**：原子切换 owner，删除旧 enqueue/recovery，验证附件 staging 与 render publish fencing。
5. **PR8E — matching**：原子切换 schedule/job owner，落位 0..N fanout 并删除 dirty/orphan recovery。
6. **PR8F — Bid single-owner closure**：删除剩余 Bid live-recovery/wire DTO/housekeep；重生成 baseline checksum/catalog/queue closure；证明 Bid 与 WorkTransport 不访问 private Redis key 并跑全量强制活库。
7. **PR9 — fresh deploy/runtime acceptance**：空 PostgreSQL/Redis/object volume、真实 API/Web/worker 链、故障矩阵与资源 cleanup 证据。

任何阶段不得让同一 target 同时受旧 live-recovery 和新 dispatcher 驱动。最终部署为 fresh redeploy，不创建历史 payload converter、数据 backfill、双写或兼容 view。

## 11. 验收矩阵

### 11.1 原子性与 schema

- target/base/typed extension/head/initial intent/state 同事务 commit/rollback；
- reverse verifier 拒绝零/多 extension、缺 head、head 跨 target、单删和 identity move；
- predecessor/generation/head composite FK 可建立，`UNIQUE(predecessor_dispatch_id)` 拒绝双 successor；
- base `PRIMARY KEY(id)` 拒绝跨 kind/project 重用 UUID，composite FK 仍证明 exact project/kind；
- intent initial CHECK 与 generation-bearing composite FK 拒绝 replacement 指向 generation>0；state 三列 FK/NULL matrix 拒绝 status 与 absorbing settlement kind 错配；
- `advanced` 只指向同 target successor；`superseded` 只指向不同 target 的 exact generation-0 replacement，XOR/FK/unique 拒绝混填、缺填和共享 replacement；
- dispatch intent immutable，state NULL matrix 覆盖全部 transition；
- 同 target/fence initial stage 幂等，不同 fence 冲突；terminal target 不重开；
- 六类 `TargetFenceV1/KBTF` Rust/SQL fixed golden 和逐字段篡改负例；
- `BidDeliveryV1/KBDL` Rust/SQL fixed golden、explicit job name、expected job ID 与 payload/version 篡改负例；
- settlement、inbound outcome 和 typed evidence canonical key/hash 并发 insert-or-read 只复用同一语义；
- conversion completion→extraction 与 matching schedule→manifest+0..N jobs/dispatches 原子，零 route 成功 terminal。
- matching mutation 的新 target+initial dispatch 与旧 nonterminal target attempt reap/`superseded` settlement 同事务；replacement-vs-heartbeat 与 replacement-vs-publish 两种锁序结果都证明旧 owner不能发布；并发 mutation 只创建一个 replacement，旧 delivery 的 historical noop 引用 exact superseded settlement。

### 11.2 一次性 transport 与 start deadline

- offer claim 一旦 commit，同 dispatch 永远不能第二次 claim 或外部 enqueue；
- claim 后、Redis 前 crash；enqueue Ok、Err、timeout、response lost 四种路径都只进入 awaiting/running/absorbing，不回 ready；
- Redis HSET-only/hash-only unique job 模拟后，successor 新 ID 仍执行；旧 ID 永不复用；
- enqueue succeeded 但 publisher DB settle 前 crash，consumer-first 可从 offering begin；晚到 publisher 只写 observation；
- publisher lease expiry 不会重投同 ID；delivery-start deadline 才创建新 dispatch；
- deadline-vs-consumer begin 并发只得到“旧 dispatch 运行”或“successor current+旧 delivery noop”；
- Redis flush、membership-only、七天 cleanup 和相同 hostname/PID restart 后，新 dispatch 在时限内恢复；
- `get_job/list/stats/delete_job` 不参与正确性，source denylist 覆盖 Bid/WorkTransport；
- 连续未 start 遵守指数 backoff、lane/global governor 和 readiness 告警，不 busy-loop。

### 11.3 Consumer、lease 与 fencing

- duplicate 并发 begin 只有一个 business owner，其余 durable noop；
- historical advanced、cross-target superseded、terminal、gate stale、fence stale、wrong job ID/version 均在任何外部调用前 settle/noop，并引用 exact absorbing disposition；正确 payload + 错误 `JobContext.meta.id` 的 external I/O count 必须为 0；
- begin 前 worker crash 由 start deadline 恢复；begin 后 crash/卡死由 business lease 精确 reap；
- owner fresh 不被 deadline/reaper 抢占；gate/head 改变后旧 owner 立即 fenced 且不能 heartbeat/publish；
- business retry、owner reap、delivery not started 和 gate rebase 全部只通过 `advance_dispatch`；
- heartbeat 与 publish race、lease 边界、旧 worker 恢复、DB 连接丢失均不能覆盖 successor；
- 长时间 conversion/render/provider 调用有独立 heartbeat 且 claim token/gate/fence 丢失即停止 publish；
- success/deterministic/retry/noop 只有在 domain result+settlement+inbound outcome 提交后才向 Oxana 返回成功。

### 11.4 Fanout、retry、安全与 retention

- provider retry、business retry、transport successor 不相乘；begin 前 delivery loss 不消耗 business attempt；
- typed target retry budget、deterministic/poison/exhausted terminal code 固定；
- API/worker/maintenance/retention role allow-deny；
- retention 不能单删 nonterminal aggregate、current head、replay 窗口内 historical settlement 或 ObjectRegistry owner；
- payload/日志 bounded 且不泄露内容/secret；
- catalog/registry/source denylist 证明旧 Bid wire、live-recovery、business housekeep 和 vendor/patch receipt 设计已删除；
- Shared Platform legacy replay 不被 Bid 调用，其未来删除条件单独验收。

### 11.5 Fresh runtime

空 PostgreSQL/Redis/object volumes 走完 convert→extract→matching→attachment preparation→render；每个 target 的 commit/offer/begin/publish 故障点至少注入一次，并证明最终只有一个有效业务结果。额外执行 Redis flush、hash-only、duplicate membership、worker 同容器 restart 和 DB unavailable 场景。

验收脚本必须在 `success|failure|timeout|cancel|SIGINT|SIGTERM` 六种退出模式删除本轮启动的 container、volume、network 与临时 image，每种模式保存独立 cleanup receipt；shell trap 与 CI `if: always()` 双层执行/验证 cleanup。只有 implemented、locally verified、committed、pushed、deployed、runtime accepted 六层分别有真实证据，且 `phase_1d_runtime_complete=true` 的受审计 cutover 完成后，才能声明本方案完成。
