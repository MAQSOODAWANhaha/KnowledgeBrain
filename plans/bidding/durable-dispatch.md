# 招投标 Durable Dispatch 与失败恢复

| 项 | 值 |
| --- | --- |
| 状态 | clean-slate V1 稳定版修订已批准并固化；尚未实施或验收 |
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

本 module 跨 PostgreSQL 与 Rust 两层，但只暴露一套受检 interface。所属领域已经由 PostgreSQL function 持有原子 mutation 的路径，必须由该 SECURITY DEFINER domain mutation 在同一个数据库 transaction 内直接调用 owner-only SQL entry。Rust 不直接调用、也不需要一一映射这些 internal SQL entry；若某条完整 domain mutation 实际由 Rust 持有 transaction，Rust 只提供包住整个 domain mutation 的 `pub(crate)` transaction adapter，不能在 domain mutation commit 后补调 dispatch。运行时由 composition root 注入各自 DB pool、`WorkTransport`、private typed-adapter registry、clock 与 shutdown token，module implementation 不从全局状态重新创建这些依赖。

逻辑 interface 固定为：

```text
stage(tx, NewBidTargetRef) -> DispatchId
replace_current_target(tx, ExpectedCurrentTarget, NewBidTargetRef, reason)
  -> ReplacementOutcome
cancel_target(tx, ExpectedCurrentTarget, reason)
  -> CancelOutcome
run(shutdown) -> Result<(), DispatchFatal>
handle(ObservedDelivery {
  dispatch_id,
  payload_version,
  observed_job_id
}) -> HandlerOutcome
```

- `stage/replace_current_target/cancel_target` 在 SQL 层是由 `kb_app_owner` 拥有的 internal function：`PUBLIC`、`kb_runtime_api`、`kb_runtime_worker`、`kb_runtime_bid_dispatcher` 与 `kb_runtime_retention` 均无直接 `EXECUTE`；只有所属 SECURITY DEFINER domain mutation 能在其现有 transaction 中直接调用。Rust store/run/handle seam不得获得这些 internal entry 的直调能力；存在 Rust mutation 调用方时，其 `pub(crate)` adapter只能调用完整受检 domain mutation并接收调用方现有 `Transaction`，不能提交、开启第二个 transaction或提升为普通 storage façade。
- 三个 mutation entry 原子 stage、cross-target replace 或 cancel 完整 aggregate，调用方不能取得 claim/settlement 子步骤。API/worker不得在 commit 后直接调用 Redis，也不得用“domain mutation 成功后再调用 Rust adapter”伪造同事务原子性。
- `stage` 从事务内 typed target 读取并冻结 target fence、gate epoch、lane、固定task/payload contract 和 dispatch semantics；调用方不能传 snapshot JSON 或自行构造队列 payload。
- `run` 内部拥有 due scan、一次性 offer、delivery-start reaper 和 target-local repair。Redis unavailable/timeout 是单条 indeterminate outcome，不终止进程；registry/codec/queue closure mismatch 才返回 `DispatchFatal` 并使 readiness fail closed。
- `handle` 是 `BidDeliveryV1Job` worker唯一调用的领域entry；在任何provider/object store/DocReader/renderer I/O前内部完成begin CAS，取得owner后选择private typed target adapter，启动scoped background heartbeat，执行外部调用，并在返回前内部publish/advance/settle。`observed_job_id`必须原样取自Oxana 2.1.3公共`JobContext.meta.id`，`payload_version`必须取自实际解码payload；禁止根据dispatch ID重新推导observed ID后再自证相等。
- `begin/claim/heartbeat/publish/advance/reap/settle` 都是 module implementation的内部原语；typed adapters只收到受限`ExecutionContext`与immutable target input，不能取得SQL store或自行迁移state。route、worker和普通业务module均不能拼装多步状态机。

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

base identity 固定为全局唯一 `id`；`(id,target_kind,project_id)` composite unique 只用于 exact relation FK。generation/watermark 是 fence，不兼作 identity。document conversion 每个 generation 创建新 target ID，`BidDocument` 只保存 current conversion target pointer。terminal/superseded/poisoned target 不重开；人工 retry 或新 mutation创建新的 target identity。若它替换同 kind 的 nonabsorbing current target，必须调用第 5 节通用 cross-target replacement 原语；若旧 target 已 absorbing，则保持旧 settlement 不可变并独立 stage 新 target。

catalog 约束：

- base、typed extension、domain target、dispatch head 使用 composite FK 保证 project/kind 一致；
- deferred verifier 要求每个已切换 family 的 domain target 在 commit 时恰有一个 base、一个正确 extension 和一个 dispatch head；
- base/extension identity immutable，禁止 UPDATE 到另一个 aggregate；
- 普通 API/worker 无这些表的直接 DML，只能调用所属受检函数；
- `SET CONSTRAINTS ALL IMMEDIATE` 必须拒绝零/多 extension、project/kind mismatch、单删和 identity move/swap。

PR8B 可建立 dormant base/extensions 与空 conversion target 表，但不补建旧行、不双写、不注册第二 owner。PR8C～PR8E 每切换一个 family 时才安装其反向 verifier并删除旧 producer/consumer。

PR8B 的 synthetic fixture 不增加 `synthetic` target kind、extension、queue task 或生产 registry entry。它只在隔离 fresh test schema 中建立一条真实 `bid_document_conversion_targets` domain row，并通过测试 owner fixture调用同一 owner-only SQL mutation建立 `document_conversion` base/typed extension/head/initial intent/state；测试 composition root 可为该真实 kind注入RecordingTransport与测试私有typed adapter。每个fixture必须使用可丢弃的独立数据库并在trap/finally中销毁，或完全包含在最终显式rollback的transaction中；需要多连接commit/race的测试只能使用可丢弃独立数据库。测试返回前必须证明base/typed/head/intent/state及role/database资源零残留。PR8B生产composition root不注册六类真实typed adapter、不创建任何真实target aggregate，也不启动dispatcher；因此fixture不能成为生产owner或需要后续兼容的数据格式。

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
UNIQUE(id,expected_oxana_job_id)
UNIQUE(id,expected_oxana_job_id,payload_version)
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

六类 fence共同验证`1 <= max_attempts <= 10`与`30_000 <= claim_lease_ms <= 1_800_000`（毫秒）；Rust builder与SQL verifier都拒绝0、低于最小值、超上限及类型溢出。heartbeat interval必须不大于`min(claim_lease_ms/3,60_000)`，不能用超长lease掩盖缺失heartbeat。

### 3.4 Mutable state 与一次性 delivery attempt

每条 intent 恰有一条 `bid_dispatch_states`：

```text
status = ready|offering|awaiting_start|running|terminal|superseded|poisoned
next_offer_at
offer_claim_token, offer_claimed_by, offer_lease_expires_at
delivery_start_deadline_at
delivery_attempt_id
delivery_attempt_phase
delivery_attempt_outcome
business_attempt_id
business_attempt_status
absorbing_settlement_id
absorbing_settlement_kind
last_transport_code
completed_at
```

状态 NULL matrix：

| 状态 | 必须存在 | 必须为空 |
| --- | --- | --- |
| `ready` | `next_offer_at` | offer claim、start deadline、delivery/business attempt |
| `offering` | exact claim/token/lease、`inflight` delivery attempt、start deadline | delivery outcome、business attempt、absorbing settlement |
| `awaiting_start` | exact `settled` delivery attempt及允许的 awaiting outcome、start deadline | offer claim、business attempt、absorbing settlement |
| `running` | exact `settled` delivery attempt、exact running business attempt | offer claim、start deadline、absorbing settlement |
| `terminal` 或 `poisoned` | exact absorbing settlement、completed_at；若保留 delivery pointer则必须 exact `settled` | 所有 due/claim/deadline/active attempt |
| `superseded` | exact `advanced` 或 `superseded` settlement、completed_at；若保留 delivery pointer则必须 exact `settled` | 所有 due/claim/deadline/active attempt |

`bid_dispatch_delivery_attempts` 每个 dispatch 至多一行：

```text
id, dispatch_id UNIQUE
target_id, target_kind, project_id
claim_token UNIQUE
phase = inflight|settled
started_at, lease_expires_at
settled_at
outcome = enqueue_returned|enqueue_indeterminate|consumer_started|publisher_lost|superseded|cancelled
returned_job_id
error_code
UNIQUE(id,dispatch_id,claim_token,phase)
UNIQUE(id,dispatch_id,phase,outcome)
UNIQUE(id,dispatch_id)
```

attempt 以 `(dispatch_id,target_id,target_kind,project_id)` composite FK 指向 exact intent。`enqueue_returned` 的 `(dispatch_id,returned_job_id)` 必须以 composite FK指向intent的`(id,expected_oxana_job_id)` exact unique key。state 的 offering shape 以 `(delivery_attempt_id,dispatch_id,offer_claim_token,delivery_attempt_phase)` exact FK 指向同 dispatch、同 token 的 `inflight` attempt；其它持有 pointer 的 shape 以 `(delivery_attempt_id,dispatch_id,delivery_attempt_phase,delivery_attempt_outcome)` exact FK 指向同 dispatch 的 `settled` attempt。deferred verifier补足 nullable composite FK，并拒绝 cross-dispatch pointer。

attempt NULL/outcome matrix 固定为：

| phase/outcome | 必须存在 | 必须为空 |
| --- | --- | --- |
| `inflight` | token、started、lease | settled、outcome、returned job、error |
| `settled/enqueue_returned` | token、started、settled、returned job | active lease、error |
| `settled/enqueue_indeterminate` | token、started、settled、bounded error | active lease、returned job |
| `settled/consumer_started\|publisher_lost\|superseded\|cancelled` | token、started、settled | active lease、returned job、error |

`awaiting_start` 只允许 `enqueue_returned|enqueue_indeterminate|publisher_lost`；consumer-first 必须在同一事务把 attempt结算为`consumer_started`并直接进入`running`，不得经过 awaiting。publisher-first后consumer从awaiting直接创建business owner，不再改写已settled delivery attempt。claim transaction提交时即插入attempt并进入`offering`；从此identity视为已暴露，`UNIQUE(dispatch_id)`与受检函数共同拒绝第二次claim/enqueue。唯一合法attempt更新是一次 `inflight -> settled` CAS；settled disposition不可改写。

publisher、consumer、publisher lease reaper、same-target advance、replacement和cancel只在争夺同一个inflight disposition时竞争CAS；该CAS败方的当前事务不能继续用旧观察改state/head/owner，只能追加observation或重读。重读后仍按状态机继续：consumer见合法settled awaiting outcome可begin；advance/replacement/cancel见settled attempt仍必须吸收target并保留attempt作audit；见已absorbing则幂等读回settlement。因此“attempt CAS败方”不等于永久禁止合法后续状态迁移。

CAS败方需要保留transport事实时，只可追加 `bid_dispatch_delivery_observations`：

```text
id, delivery_attempt_id, dispatch_id
observer_kind = publisher|consumer|publisher_lease_reaper|replacement|cancel
observation_kind = race_lost|adapter_mismatch
observed_outcome
returned_job_id, error_code, observed_at
UNIQUE(delivery_attempt_id,observer_kind)
UNIQUE(id,delivery_attempt_id,dispatch_id)
```

observation 以 `(delivery_attempt_id,dispatch_id)` exact FK 指向 attempt，字段有固定长度上限。`race_lost`保存败方观察到的候选disposition并使用对应returned/error shape；`adapter_mismatch`只允许publisher，必须保存 `returned_job_id_mismatch(actual_job_id)`携带的实际ID与固定error code。prepare/registry/codec closure mismatch发生在offer前，不创建delivery attempt observation。每个 attempt最多五条 observation，普通 duplicate delivery只写 inbound noop，不膨胀 observation。publisher晚到结果只能写该 append-only bounded row，不能改变 attempt disposition、current head或业务owner。

observation CHECK matrix固定为：publisher的`race_lost`只允许`enqueue_returned|enqueue_indeterminate`并分别遵守returned/error XOR；consumer只允许`race_lost/consumer_started`且returned/error为空；publisher lease reaper只允许`race_lost/publisher_lost`；replacement只允许`race_lost/superseded`；cancel只允许`race_lost/cancelled`。`adapter_mismatch`只允许publisher且`observed_outcome=enqueue_indeterminate`，actual returned ID与固定error必须同时存在；deferred verifier沿attempt→intent证明actual ID不等于`expected_oxana_job_id`。其它observer/outcome组合、actual=expected或NULL shape全部拒绝。

所有进入`terminal|superseded|poisoned`的原语共用一个private absorbing-cleanup：锁state后若存在inflight delivery attempt，带exact `ObservedDelivery`的handler先结算为`consumer_started`；cancel结算为`cancelled`；replacement、gate/fence repair及其它非timeout吸收结算为`superseded`；只有publisher lease expiry或delivery-start timeout结算为`publisher_lost`。已有settled attempt只保留作audit。cleanup随后才允许写absorbing settlement/state并解析全部repair obligations。任何absorbing state与inflight attempt共存都由deferred verifier拒绝。

`bid_dispatch_business_attempts` 是六类 executor 共用的唯一 owner lease：

```text
id, dispatch_id UNIQUE
target_id, target_kind, project_id
attempt_ordinal CHECK (attempt_ordinal >= 1)
claim_token UNIQUE
runtime_governor_generation
status = running|succeeded|retryable_failed|deterministic_failed|expired|superseded|cancelled|poisoned
started_at, heartbeat_at, lease_expires_at, terminal_at
terminal_code
UNIQUE(target_id,target_kind,attempt_ordinal)
```

attempt NULL matrix 固定为：

| status | 必须存在 | 必须为空 |
| --- | --- | --- |
| `running` | token、started/heartbeat/lease | terminal_at、terminal_code |
| 七种 terminal status | token、started/last heartbeat、terminal_at、terminal_code | active lease |

row 以 composite FK 同时指向 exact dispatch intent 与 async target，并提供含 status 的 unique key。state 持久化 `business_attempt_id,business_attempt_status`：`running` state只能 composite-FK到同 dispatch/target的 running attempt；其它 state不得指向 running attempt。settlement/inbound/typed evidence 如引用 attempt，也持久化 attempt dispatch/target/status并只能 composite-FK到同 dispatch/target的 finalized attempt。

`begin` 在锁住 domain target后按 existing max ordinal+1插入，ordinal 即 `business_attempts_started`，不得维护第二个可漂移 counter。受检函数与 deferred verifier共同保证 ordinal从1连续、不得超过 target fence frozen `max_attempts`；普通 API/worker无直接 DML。terminal transition清 active lease并冻结 status/code/time，后续 heartbeat必须返回 fenced。各 typed executor/domain attempt以 exact FK扩展该 owner row，不能另存一套 claim token/lease owner；PR8C～PR8E纵切时删除对应旧 claim owner。

### 3.5 Settlement、inbound outcome 与 evidence

`bid_dispatch_settlements` 保存每个 dispatch 的唯一 disposition：

```text
id, dispatch_id
old_target_id, old_target_kind, old_project_id
settlement_kind = advanced|terminal|superseded|poisoned|cancelled
successor_dispatch_id
replacement_target_id, replacement_target_kind, replacement_project_id
replacement_initial_dispatch_id, replacement_dispatch_generation
reason_code
gate_epoch
business_attempt_id
business_attempt_status
created_at
UNIQUE(dispatch_id)
```

所有 settlement 先以 `(dispatch_id,old_target_id,old_target_kind,old_project_id)` composite FK 指向 old exact intent，避免只凭全局 UUID 推断 old relation。disposition XOR 固定为：

- `advanced` 只允许 `successor_dispatch_id` 非空；successor 必须属于同 target、generation=`old+1`，且 predecessor 反向指回 old dispatch；全部 replacement 字段为空；
- `superseded` 禁止 successor，必须填满 replacement target/dispatch 五元组，且 `replacement_dispatch_generation=0`。该五元组分别以 composite FK 指向新的 async target，以及 intent 的 `(id,target_id,target_kind,project_id,dispatch_generation)` unique key；intent 的 initial CHECK 因而同时证明 predecessor 为空。row CHECK 强制 `replacement_project_id=old_project_id`、`replacement_target_kind=old_target_kind` 且 `replacement_target_id<>old_target_id`；
- `terminal|poisoned|cancelled` 的 successor 与全部 replacement 字段均为空。

`UNIQUE(replacement_initial_dispatch_id) WHERE settlement_kind='superseded'` 防止一个新 target 吸收多个旧 current target。settlement 提供`UNIQUE(id,dispatch_id)`与`UNIQUE(id,dispatch_id,settlement_kind)`；state持久化`absorbing_settlement_id,absorbing_settlement_kind`并以三列composite FK指回exact settlement。state CHECK规定：`superseded`只接受`advanced|superseded`，`terminal`只接受`terminal|cancelled`，`poisoned`只接受`poisoned`，nonabsorbing state两列均为空。并发reaper、consumer、gate rebase和replacement mutation通过predecessor、replacement与dispatch唯一约束insert-or-read同一结果。

每次准备向 Oxana 返回成功前必须写或复用 `bid_dispatch_inbound_outcomes`：

```text
id, settlement_key
dispatch_id
observed_job_id
observed_payload_version
outcome_kind = business_success|business_failed|retry_scheduled|noop|poison
reason_code
business_attempt_id
business_attempt_status
dispatch_settlement_id
repair_obligation_id
rejected_delivery_id
rejected_delivery_mismatch_kind
evidence_sha256
created_at
UNIQUE(settlement_key)
```

`bid_dispatch_repair_obligations` durable 地表示 current nonabsorbing noop 后必须完成的修复：

```text
id, dispatch_id
reason = owner_expired|gate_stale|target_stale
observed_gate_epoch, observed_target_fence_sha256
requested_at
resolved_settlement_id, resolved_at
UNIQUE(dispatch_id,reason,observed_gate_epoch,observed_target_fence_sha256)
UNIQUE(id,dispatch_id,reason)
```

inbound以`(repair_obligation_id,dispatch_id,reason_code)` composite-FK到obligation的`(id,dispatch_id,reason)` exact unique key，不能让owner/gate/target stale observation互相借用义务；resolution以`(resolved_settlement_id,dispatch_id)` composite-FK到settlement的同名unique key。NULL matrix要求unresolved时settlement/time都为空，resolved时两者都存在且settlement属于同dispatch。repair due scan直接以`resolved_at IS NULL` partial index读取 obligation，state不复制 pointer或requested time作为第二真源。

同一 dispatch允许多个不同观察产生多个 unresolved obligation。current stale handler只在锁住target/head/state并确认仍nonabsorbing后，原子插入或复用 exact unresolved obligation及引用它的 inbound，然后提交成功；handler不得在同一事务即时advance/replacement而把刚提交的obligation变为resolved。repair runner按`target_stale > gate_stale > owner_expired`优先级重算当前权威事实，不把旧reason当命令；它按ID锁住该dispatch全部unresolved obligations，并在advance、replacement、cancel、terminal或poison吸收事务中把全部obligation绑定到同一exact settlement并设置同一resolved time。若吸收先提交，并发handler重验后只能写historical/terminal noop，不得再创建unresolved obligation；若handler先提交，吸收事务必须看见并解析它。因此任一absorbing state都不允许残留该dispatch的unresolved obligation。

`bid_dispatch_rejected_deliveries` 最小schema固定为：

```text
id, dispatch_id
observed_job_id, observed_payload_version
expected_oxana_job_id, expected_payload_version
mismatch_kind = job_id|payload_version|job_and_payload
reason_code = delivery_mismatch
observed_at
UNIQUE(dispatch_id,observed_job_id,observed_payload_version,mismatch_kind)
UNIQUE(id,dispatch_id,observed_job_id,observed_payload_version,mismatch_kind)
```

row 的 `(dispatch_id,expected_oxana_job_id,expected_payload_version)` composite FK必须指向intent的`(id,expected_oxana_job_id,payload_version)` exact unique key；CHECK要求observed tuple至少一项不等，并要求`mismatch_kind`与实际不等字段完全一致。known dispatch的observed/expected identity不一致时先插入或复用该bounded row；inbound以`(rejected_delivery_id,dispatch_id,observed_job_id,observed_payload_version,rejected_delivery_mismatch_kind)` exact FK引用它，且CHECK要求 inbound `reason_code=delivery_mismatch`。所有identity与reason字段有固定长度上限。无法解析或unknown dispatch不造业务row，只形成平台bounded dead/metric。

inbound shape固定：historical/target-terminal noop必须只引用exact absorbing settlement；owner-expired/gate-stale/target-stale current noop必须只引用repair obligation且初始settlement为空；delivery-mismatch必须只引用rejected-delivery；duplicate-fresh-owner三者都为空；business success/failure/retry只引用对应settlement。repair完成后只更新obligation的resolved settlement/time，不回写或伪造原始inbound observation。

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
```

以上列表不包含 `business_claim_lease_ms`；business claim lease 的唯一权威值是 target fence 已冻结的 `claim_lease_ms`，dispatch semantics 不得复制第二份。所有 transport 值正数且有上限；publisher lease 覆盖正常 offer deadline，delivery-start timeout 大于 publisher lease；successor backoff 按 generation 指数增长并 cap，避免 lane 不可用时高频 duplicate。generation 超过告警阈值使 readiness 降级但不把基础设施故障改成业务 terminal。

poll interval、batch size、global/per-kind concurrency属于live shared governor。每批`FOR UPDATE SKIP LOCKED`且有硬上限；semantics promotion只在maintenance，不能改写已有intent。所有nonterminal intent引用的semantics、lane与typed adapter在引用归零前必须保留registry closure。

execution concurrency是DB硬上限，不是假定Oxana单进程配置。governor config是immutable version+current pointer；global与每kind各有一行跨generation稳定aggregate counter`limit/current_count`。`begin`在锁target后严格按current/config pointer→global counter→kind counter取锁，只有两个`current_count < limit`才在同事务各+1并创建记录所用config generation的business attempt。容量不足不创建attempt、不清start deadline，handler返回worker error；delivery随后由start deadline用新identity恢复。attempt任一terminal/reap/replacement/cancel在同事务按相同子序对稳定counters各-1；counter不得为负或超过limit。

governor promotion按current/config pointer→global counter→kind counters稳定排序取锁；新global/per-kind limit不得低于当时对应`current_count`，否则promotion失败并等待drain，不能为新generation另开一套从0开始的容量。旧config在引用归零前保留作audit，但不拥有独立counter。deferred verifier要求每个stable counter等于所有active config generations对应running attempts的聚合数。

## 4. 状态机与恢复

### 4.1 Initial stage 与一次性 offer

| 事件 | CAS 前置 | 原子结果 |
| --- | --- | --- |
| stage initial | typed target 可投递且无 head | target/base/extension + generation 0 intent/state/head 同事务；state=`ready` |
| claim offer | current head、ready、due、gate open | 插入唯一 delivery attempt；state=`offering`；同时写 claim lease 与 delivery-start deadline |
| enqueue returned | exact claim，仍 current offering | attempt=`enqueue_returned`，验证 job ID，清 claim，state=`awaiting_start` |
| enqueue indeterminate | exact claim，仍 current offering | attempt=`enqueue_indeterminate`，清 claim，state=`awaiting_start`；禁止同 ID 重试 |
| publisher lease expired | exact offering claim 已过期 | attempt=`publisher_lost`，清 claim，state=`awaiting_start` |
| late publisher result | attempt 已 finalize 或 dispatch 已非 current | append observation；不改 head/state/owner |

`ready` 是唯一可以调用 transport 的状态。进入 `offering` 后无论外部 I/O 是否真的发生，都没有返回 `ready` 的 transition。

### 4.2 Consumer begin

consumer 接收完整 `ObservedDelivery`，按全局锁序锁定 target、业务 attempt、head、dispatch并按固定优先级重新验证：

1. payload version、observed job ID与intent expected job ID完全相等，并证明intent→target/project immutable relation；
2. 读取 target status、dispatch state/head和可选 absorbing settlement；若dispatch已非head或任一方absorbing，先走historical/terminal noop，不要求target仍pending或当前gate仍等于旧intent；
3. 仅对 current nonabsorbing dispatch重算typed fence、检查target/dispatch status matrix和maintenance gate/epoch；
4. 只有 current `offering|awaiting_start` + target pending 或 current running + target running是可执行形状；其它组合contract poison；
5. 调用唯一 `classify_owner` 得到 `none|fresh|expired_or_fenced`。

transition：

`business_attempts_started` 是 stable target 上的单调计数；每次成功取得 business owner 的 `begin` 原子分配下一 ordinal 并立即消耗一次 budget。begin 前 delivery 丢失不消耗，provider 在同一 owner 内的 bounded retry 不另计；begin 后 crash、lease expiry 或 retryable failure 都已消耗该次 attempt。

| 观察 | 结果 |
| --- | --- |
| current `offering` 或 `awaiting_start` + owner none + `started < max_attempts` | finalize 可选 delivery attempt=`consumer_started`，清 publisher claim/deadline，分配下一 business attempt，target/dispatch=`running` |
| current `offering` 或 `awaiting_start` + owner none + `started >= max_attempts` | 该 state按构造合同不可达；不创建owner，先以ObservedDelivery执行统一absorbing cleanup，再原子写 `DISPATCH_BUDGET_ORPHAN` poison settlement/evidence/inbound并使 readiness失败 |
| current running + owner fresh | durable `noop/duplicate_fresh_owner`，不改变 owner |
| owner expired_or_fenced | durable `noop/owner_expired` 与 unresolved repair obligation；当前 handler 不得偷取旧 token或即时repair |
| dispatch 非 head 或已 superseded | 引用其 exact absorbing settlement（`advanced`、`superseded`、`terminal`、`poisoned` 或 `cancelled`）写 `noop/historical_dispatch` |
| target terminal/poisoned/cancelled | 写或复用对应 terminal noop |
| gate stale、target fence仍有效 | 写 noop 与 unresolved gate-rebase obligation，不执行外部依赖 |
| target fence stale | 写 `noop/target_stale` 与 unresolved target-replacement obligation；禁止 same-target advance |
| known dispatch 但 job ID/version 不匹配 | rejected-delivery + `noop/delivery_mismatch` |
| execution governor capacity unavailable | 不改state/attempt、不写成功inbound；返回worker error，现有start deadline最终以新identity恢复 |

只有取得 exact business claim 的分支可以调用外部依赖。除 capacity unavailable 返回worker error外，其它非执行分支都在 durable inbound outcome 提交后直接向 Oxana 返回成功。

### 4.3 Business execution、heartbeat 与 publish

- executor 只从 immutable target/snapshot 读取输入；Redis payload 不携带业务数据。
- 长时间 DocReader/provider/object/render 调用期间运行独立 background heartbeat；heartbeat 与 publish 只调用第 4.5 节的同一个 `classify_owner` 权威原语，不复制或缩短 fresh 条件。
- heartbeat由`handle`持有的structured-concurrency guard管理，禁止detach：正常完成/错误/worker shutdown先取消并join heartbeat；panic、timeout、future cancel/drop由abort-on-drop guard同步终止。heartbeat任何一次返回fenced或无法从DB证明owner fresh，都取消传给typed adapter的execution token；adapter必须停止后续外部工作，`handle`禁止publish并返回worker error或读取已存在durable disposition。heartbeat task不得在`handle`生命周期外继续续租。
- staging 外部产物在 publish 前登记 owner；lease-lost/fenced owner 不能绑定 current pointer，孤儿由 ObjectRegistry/retention 回收。
- success 或 deterministic failure：domain result、target terminal、dispatch terminal settlement、typed evidence 和 inbound outcome 同一事务；提交后才向 Oxana 返回成功。
- retryable failure：终结已计数的 business attempt并调用 `advance_dispatch`；尚有 budget 时创建新 identity并写 `retry_scheduled` inbound outcome，已耗尽时原子 terminal。owner crash/lease expiry 使用同一边界，不额外或遗漏计数。
- DB commit 结果未知时 handler 返回 error；若事务实际已提交，任何晚到 delivery 只读取 exact absorbing settlement 并 noop。
- stale owner 恢复后 heartbeat/publish 必须因 token/head/lease/gate/fence 任一不等而失败，不能覆盖 successor 结果。

### 4.4 统一 successor 原语

所有未开始恢复、owner reap、业务 retry 和 gate rebase 只调用：

```text
advance_dispatch(
  old_dispatch_id,
  reason,
  new_gate_epoch,
  due_at,
  observed_delivery: Option<ObservedDelivery>
)
  -> advanced(new_dispatch_id)
   | terminal_exhausted(settlement_id)
   | owner_still_fresh
   | target_stale
   | contract_poison(settlement_id)
```

原语必须在一个 transaction 中：

1. 按固定顺序锁 target、存在running owner时的exact governor counters、可选exact business attempt、head、old dispatch state、可选exact delivery attempt及全部unresolved repair obligations；
2. 验证 old仍是 current head、尚无 absorbing settlement且 immutable target fence仍与 domain target一致；fence stale返回 `target_stale`，禁止复制旧 fence创建 successor；
3. 对 fresh owner 返回 `OWNER_STILL_FRESH`，不得抢占；
4. 对 expired/fenced owner 精确终结已经在 begin 时计数的旧 attempt 并清理其 claim，不在 reaper 再加一次；
5. 在任何后续absorbing分支前先执行统一delivery cleanup：若old state=`offering`且attempt仍inflight，`Some(observed_delivery)`结算为`consumer_started`；`reason=publisher_lease_expired|delivery_not_started`结算为`publisher_lost`；gate rebase及其它非timeout advance结算为`superseded`。`awaiting_start|running`必须保留已有settled disposition，ready可无attempt；
6. 若 `business_attempts_started >= max_attempts`，先验证current dispatch是否存在exact finalized `expired|retryable_failed` attempt：存在时原子把target/dispatch置terminal，写resultless `attempts_exhausted` settlement/evidence，清due/claim/deadline、解析全部repair obligations并返回`terminal_exhausted`；handler传`Some(observed_delivery)`时还在同事务写`business_failed/attempts_exhausted` inbound，提交后才向Oxana成功；reaper传`None`时不伪造inbound，只写settlement/evidence/audit；
7. 若已达max但current dispatch没有上述exact finalized attempt（例如非法offering/awaiting后继），禁止伪造exhausted：写`DISPATCH_BUDGET_ORPHAN` poison settlement/evidence/readiness failure，解析全部repair obligations并返回`contract_poison`；有ObservedDelivery时同事务写poison inbound；
8. 否则生成新 UUID并插入 generation+1 immutable intent 与 `ready` state；
9. 以 `UNIQUE(predecessor_dispatch_id)` insert-or-read 并发唯一 successor；
10. old state=`superseded`，写 advanced settlement，target回 pending，head指向 successor；
11. 清 old due/claim/deadline，把 old dispatch 全部 unresolved repair obligations 解析到该 exact settlement，然后写审计。

`max_attempts` 是 target fence 的不可变正整数，跨同 target 的所有 dispatch generation 共享；cross-target 人工 retry 创建新 target时才按新的受检 policy 取得新 budget。transport successor 在 begin 前不消耗 budget，但不能越过已耗尽检查创建一个永远不可 claim 的 dispatch。

`attempts_exhausted` evidence 是合法的 resultless variant：引用同 dispatch/target的 finalized `expired|retryable_failed` business attempt、frozen max/ordinal 与 fence hash，result artifact必须为空。attempt/status/result形状不匹配必须拒绝整个 terminal transaction。若 begin观察到“无 owner但 ordinal已达max”的后继 dispatch，说明先前错误创建了不可claim successor，只能按 `DISPATCH_BUDGET_ORPHAN` contract poison收敛，不能伪造 exhausted evidence。

delivery-start reaper 只处理 current `offering|awaiting_start` 且 DB 无 fresh owner的 row；repair runner独立消费 unresolved obligations，stale delivery handler本身不调用reaper：

- deadline 未到不推进；
- deadline 到且 owner none：`advance_dispatch(delivery_not_started,...)`；
- owner fresh：归一为 running 或保持业务 owner，不推进；
- owner expired/fenced：精确 reap 后按剩余 budget advance 或 terminal exhausted；
- Redis queue depth/get_job/stats 不得改变单条决定。

### 4.5 Gate 与 owner 三态

`classify_owner` 是 begin、heartbeat、publish、advance、replacement 和 reaper 共用的唯一权威定义。fresh 必须同时满足：target 仍为 executable/running；dispatch state=`running` 且 absorbing settlement 为空；exact current head；dispatch/gate/fence 相等；business attempt status=`running`、token相等且 DB lease 未过期。缺少 attempt 为 `none`；曾有 owner但任一 fresh 条件不满足为 `expired_or_fenced`。不得在其它函数复制较短条件。只要 target/head/gate/fence/settlement 任一改变，即使 heartbeat 时间尚新也不能继续发布。

gate 关闭时停止新 offer/begin 并阻断 heartbeat/publish；旧外部调用可结束但结果不能成为 current。gate 以新 epoch 开放后，none 直接 advance，expired/fenced 先精确 reap 再 advance。旧 epoch owner 不允许分类为 fresh 并无限等待。

gate rebase只改变 gate epoch，可使用同 target `advance_dispatch`。target fence stale表示 immutable target输入已失效，只能由所属领域 checked mutation调用 `replace_current_target`。repair重验时若已经存在新 current target，则旧 delivery按其 absorbing disposition noop；若 stale target仍是 current且没有 replacement，说明原子 mutation contract已破坏，必须先执行统一absorbing cleanup，再以 `DISPATCH_FENCE_ORPHAN` poison settlement/evidence终结并使 readiness失败，绝不循环创建同一 stale fence的 dispatch。

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

所有“人工 retry 或新 mutation 以新 target 取代同 kind current target”的路径都使用 crate-private `replace_current_target`，不调用 same-target `advance_dispatch`。所属领域事务先锁 project/current domain pointer revision，再按全局锁序锁全部相关旧 targets、存在running owner时的governor counters、可选 exact business attempts、heads、dispatch states、delivery attempts与unresolved repair obligations；随后创建新domain target、base/extension/head/generation-0 intent/state，并原子切换所属领域current pointer。旧dispatch按状态分支：

- `ready`：清 `next_offer_at`；
- `offering`：把 exact delivery attempt 结算为 `superseded`，清 publisher claim/start deadline；已经在途的 publisher 结果只能追加 observation；
- `awaiting_start`：保留 finalized delivery attempt作 audit，清 start deadline；
- `running`：精确终结 business attempt 为 `superseded` 并清 owner/lease；
- 已有 absorbing settlement：保持旧 target/state/settlement 不可变，新 target 由新 watermark 独立 stage。

前四种 nonabsorbing 分支都把旧 target/state 标为 superseded，并在同一事务写 replacement 五元组指向新 target/initial dispatch 的 `superseded` settlement；所有 due/active claim/deadline 清空，finalized attempt pointer 可保留作 audit，旧dispatch全部unresolved repair obligations解析到该settlement。旧 head保留指向旧 final dispatch，新 target拥有自己的 head。

replacement mutation 与 heartbeat/publish 使用同一锁序和 CAS。mutation 先提交时，旧 owner 因 `classify_owner` 失败；publish 先提交时，mutation 观察旧 absorbing terminal 并不得改写它。并发 mutation 只能有一个 current revision CAS 成功，失败方不得留下 target 或 intent。旧 nonterminal delivery 晚到时引用 exact `superseded` settlement durable noop，不能执行 target；晚到 publisher 只追加 observation。旧 target 已 absorbing 时，新 target的 domain audit记录 retry/mutation reason与旧 target ID，但不得伪造第二条 dispatch settlement。

matching input mutation 必须在推进 watermark、stale 旧 projections/picks和创建新 schedule target的同一事务调用该原语。conversion/extraction 人工 retry、attachment preparation重新提交和 render重新请求在各自 current pointer存在时使用同一原语；目标 family 切换 PR 必须验证其 domain pointer CAS与 generic replacement relation一致。

`cancel_target` 是同 module 的第二个 absorbing 原语，用于“旧 target 不存在一对一 replacement，但已失去执行资格”。它复用统一锁序并按五态处理：`ready`无delivery attempt；`offering`才把exact inflight attempt结算为`cancelled`并清publisher claim/deadline；`awaiting_start`保留已有settled attempt作audit并清deadline；`running`保留settled delivery attempt、把exact business attempt终结为`cancelled`并清owner/lease；已absorbing保持旧state/settlement不可变。前四种nonabsorbing状态都把target/dispatch置terminal，写无successor/replacement的`cancelled` settlement，并把该dispatch全部unresolved repair obligations解析到同一settlement。晚到publisher只追加observation，晚到delivery在外部I/O前引用cancelled settlement noop。

matching input mutation除以 `replace_current_target` 替换旧 nonterminal schedule外，还必须按稳定 target ID顺序锁定并 `cancel_target(reason=matching_input_changed)` 旧 current manifest下全部 nonterminal matching jobs，再创建新 schedule。0/1/N jobs都在推进 watermark的同一事务完成；不能等待新 schedule fanout后再异步清旧 jobs，也不能让旧 job用 stale watermark发布。

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
- `offer returned_job_id_mismatch`：attempt结算为`enqueue_indeterminate(error=adapter_mismatch)`并写含实际returned job ID的bounded observation，同时runtime readiness fail closed；该dispatch仍不得重投，只能由start deadline以新identity恢复；
- publisher result 晚到：只追加 bounded observation；
- pure prepare payload rejected：业务 transaction 回滚；
- runtime adapter/registry mismatch：global fatal/not-ready；已经暴露的 identity 仍只能由 deadline successor；
- Redis unavailable/timeout 只能在调用 `offer` 后形成 `enqueue_indeterminate`；固定 seam 没有 claim 前 membership/health proof，dispatcher 不得据未公开的 admission 状态跳过或反复重用 identity。

### 6.3 Handler outcome

- durable business/noop/inbound settlement 已提交：`Ok(())`；
- DB unavailable、transaction 结果未知或尚无 durable outcome：worker error；
- `max_retries=0` 防止 Oxana handler retry 与 DB successor 相乘；
- `resurrect=true` 只作为加速器，不能延后或取消 DB deadline/lease 恢复。

## 7. 并发、性能与 retention

- due scan 分别覆盖 `ready next_offer_at`、`offering publisher lease expired`、`offering|awaiting_start delivery_start_deadline`、business lease expired 和 unresolved repair obligation；每个条件有 partial index。
- 每批固定上限，`FOR UPDATE SKIP LOCKED`；global/per-kind successor 与 execution concurrency 由 current governor 限制。
- target repair 按 typed adapter round-robin，每 kind 每轮固定 batch，不能用无界中央 UNION 让大 backlog 饿死其它 kind。
- 新 ready intent commit 后发 bounded NOTIFY hint，polling 是漏通知兜底。
- queue backlog 无法与丢消息区分；successor 按 frozen exponential backoff 并受 lane/global rate limit。历史消息可能增加，但全部在 DB begin 前 noop。
- aggregate 包含 domain target/base/extension/head、全部 dispatch intents/states/delivery attempts/observations、business attempts、settlements、inbound outcomes、repair obligations、rejected deliveries与evidence。
- nonterminal aggregate、current head、replay 窗口内 historical dispatch 和 terminal audit 不得单删；终态且外部引用释放后按 retention 整体删除。
- Redis job/dead list、get_job 和 delete_job 不参与 retention proof。

全局锁序：

```text
project/current domain pointer（需要时）
  -> domain target/base/extension
  -> runtime governor current/config pointer（需要 owner begin 或 promotion 时）
  -> runtime governor global counter（需要 owner transition 时）
  -> runtime governor kind counter
  -> exact business attempt
  -> dispatch head
  -> dispatch intent/state
  -> exact delivery attempt
  -> unresolved repair obligations（按 ID）
  -> settlement/delivery observation/inbound/evidence
```

- consumer 可先无锁定位target，但加锁后按上述顺序重验全部fence；不需要project/current pointer或governor counter的原语跳过对应行，禁止反向补锁；
- governor promotion不锁domain target，直接从current/config pointer开始并按global→kind稳定顺序取得全部counter；begin使用target→pointer→global→kind，terminal/reap/replacement/cancel若无需pointer可跳过它，但不得在取得counter后反向获取pointer；
- Redis/provider/object/DocReader/renderer 调用期间不持 PostgreSQL row lock；
- publisher result settlement从dispatch state开始使用相同后缀`state -> delivery attempt -> observation`，不得先锁attempt再反向获取state或target；late result只追加observation；
- fanout 多 target 按 `(project_id,target_kind,target_id)` 稳定排序；
- PostgreSQL `40P01` 只允许 bounded whole-transaction retry，不消耗业务 retry budget。

## 8. 权限与可观测性

- API role 只能通过所属 SECURITY DEFINER domain mutation 间接进入 owner-only `stage/replace_current_target/cancel_target`，不能直接执行这些 internal function 或改 head/state/attempt；
- PR8B 沿用既有 first-launch trust topology新增独立 login role `kb_runtime_bid_dispatcher`：`deploy/postgres-init/010-runtime-identities.sh` 必须要求 `KNOWLEDGEBRAIN_BID_DISPATCHER_DB_PASSWORD`，并以 `LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS`、默认 connection limit、无 valid-until 创建；独立 DSN 固定为 `BID_DISPATCH_DATABASE_URL=postgres://kb_runtime_bid_dispatcher:<password>@<host>:<port>/<database>`，不能复用 worker DSN；
- 该 role 必须加入bootstrap脚本全部governed数组、password-posture helper允许名与初始`EXECUTE` grantee、handoff/finalizer检查、Rust `GOVERNED_ROLES`与runtime reachability集合、catalog allowlist；finalized governed role exact count由13改为14。dispatcher自身始终零membership且不得`MEMBER/SET`到owner/governed role；handoff阶段仍只允许verifier→`kb_app_owner`/`kb_launch_owner`两条临时SET edge，finalizer后全部governed membership精确为零。handoff后dispatcher仍为login，finalizer只移除migrator/verifier临时authority，不得授予dispatcher owner可达性；
- `kb_app_owner` 只授予dispatcher对目标database的非grantable `CONNECT`和`public` schema的非grantable `USAGE`，再授予`run`背后的最小受检function；`PUBLIC`及dispatcher对表/sequence/internal mutation/worker `handle` entry均deny。first-launch verifier的expected database/schema ACL、role attributes、password posture、membership和post-finalizer exact topology必须同步，不能只改role seed；
- `deploy/docker-compose.yml`在PR8B把password传入PostgreSQL bootstrap并生成dormant dispatcher DSN配置；`.github/workflows/ci.yml`、`scripts/fresh_schema_acceptance.sh`、`scripts/compose_first_launch_acceptance.sh`与`crates/storage/src/first_launch.rs`的bootstrap参数、catalog/ACL/password posture、handoff/post-finalizer常量和fixture必须同时传入并验证该identity。PR8B不启动使用该DSN的进程；未来可以由同一worker binary持有两个pool，但凭证、pool与grants必须分离；
- `kb_runtime_worker` 只能调用 `handle(ObservedDelivery)` 背后的受检函数，`kb_runtime_bid_dispatcher` 只能调用 `run` 背后的 due scan/offer/reaper/repair 受检函数；双方均无对方入口的 `EXECUTE`，也无 attempt/head/settlement 直接 DML 权限；
- PR8B 只建立 dispatcher role、DSN contract 和 ACL 负例，不在 worker main、`run_core`、API 或任何 domain mutation 中 spawn `run`；首次真实启动只能与 PR8C～PR8E 对应 family 的旧 owner 删除发生在同一纵切；
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
2. **PR8B — dormant durable core**：fresh baseline 建立 base/extensions/head/intent/state/delivery attempt/business attempt/observation/settlement/inbound/repair obligation/rejected delivery/evidence/semantics/governor 与独立 dispatcher role/ACL；实现 owner-only SQL mutation、完整 domain mutation wrapper（仅实际 Rust mutation 调用方需要）与受检 run/handle store seam，用真实 `document_conversion` typed target 的隔离 synthetic fixture 通过同一 interface 验证每 dispatch 最多一次 offer、same-target advance、cross-target supersede 和其余状态机，不注册真实 adapter、不启动 dispatcher或切换 producer/consumer。
3. **PR8C — conversion/extraction**：安装 reverse verifier，原子切换 owner，删除该 family 旧 enqueue/live-recovery；验证 conversion→extraction 后继同事务。
4. **PR8D — attachment/render**：原子切换 owner，删除旧 enqueue/recovery，验证附件 staging 与 render publish fencing。
5. **PR8E — matching**：原子切换 schedule/job owner，落位 0..N fanout 并删除 dirty/orphan recovery。
6. **PR8F — Bid single-owner closure**：删除剩余 Bid live-recovery/wire DTO/housekeep；重生成 baseline checksum/catalog/queue closure；证明 Bid 与 WorkTransport 不访问 private Redis key 并跑全量强制活库。
7. **PR9 — fresh deploy/runtime acceptance**：空 PostgreSQL/Redis/object volume、真实 API/Web/worker 链、故障矩阵与资源 cleanup 证据。

任何阶段不得让同一 target 同时受旧 live-recovery 和新 dispatcher 驱动。最终部署为 fresh redeploy，不创建历史 payload converter、数据 backfill、双写或兼容 view。

PR8B 保持一个合并门，但内部固定按下列八个 vertical slices 顺序收敛；后续 slice 只能依赖前一 slice 已冻结的 interface，不得平行发明第二套状态原语：

| PR8B 内部 slice | 范围 | 本 slice 验证 |
| --- | --- | --- |
| B1 Canonical identity 与 role | KBDL/KBTF builder/verifier、`kb_runtime_bid_dispatcher`、独立 DSN 与基础 deny-by-default grants | fixed golden/逐字段篡改、role catalog、worker/dispatcher/API 交叉 deny |
| B2 Dormant schema skeleton | base、六 typed extension、空 conversion target、head、initial intent/state、owner-only SQL mutation、完整 domain mutation wrapper 与 Rust run/handle store seam | commit/rollback、exact relation、immutability、NULL matrix、runtime role不能直调internal mutation、Rust无直调路径 |
| B3 Delivery one-shot | due claim、唯一 delivery attempt、one-shot offer及returned/indeterminate outcome | claim 后 crash/Ok/Err/timeout/response lost均不二次 offer，attempt outcome矩阵闭合 |
| B4 Settlement 与 inbound integrity | late observation、settlement、rejected delivery、repair obligation、inbound、typed evidence | late publisher/consumer/reaper observation、XOR/exact FK、bounded uniqueness、canonical insert-or-read |
| B5 Business owner 与 governor | begin CAS、ordinal/budget、lease、global/per-kind stable counters、promotion | capacity 不建 attempt、N 边界、并发 slot 与 promotion 双序 |
| B6 Successor 与 absorbing primitives | `advance_dispatch`、replacement、cancel、统一 absorbing cleanup | 五态、双序 race、late publisher/delivery、无 inflight/未解析 obligation 残留 |
| B7 Handle lifecycle | private typed adapter dispatch、scoped heartbeat、publish/settle、reap | normal/error/timeout/drop/panic/shutdown/fenced/DB failure均 join/cancel并最终收敛 |
| B8 Dormant closure | required job、fresh catalog/ACL/checksum、source/registry denylist | 真实 conversion fixture只存在于测试；生产无 target/adapter/dispatcher spawn，旧 owner仍唯一 |

## 11. 验收矩阵

PR8B 的测试责任固定分层，不为 integration test 扩张生产 interface：

- `cargo test -p bid --test durable_dispatch_sql -- --nocapture --test-threads=1` 以 fresh PostgreSQL owner fixture验证 owner-only SQL mutation、schema/FK/CHECK/deferred verifier、ACL、并发与 synthetic `document_conversion` aggregate；它不经过Rust直调或映射internal SQL entry；
- `cargo test -p bid --lib dispatch::tests -- --nocapture` 由 crate unit tests验证完整 Rust domain mutation wrapper（若存在）、run/handle store seam、private runner原语、注入clock、RecordingTransport及once-per-dispatch；它明确证明Rust不能直调internal SQL mutation，也不是可由其它crate复用的公开façade；
- `cargo test -p worker --test durable_dispatch_worker -- --nocapture --test-threads=1` 只经 worker 可见 `handle` seam验证 ObservedDelivery、private test adapter、structured heartbeat与 lifecycle 收敛，不能直接编排 begin/heartbeat/publish/settle；
- 三个入口从 PR8B 起全部进入同一 required job；任一缺失、skip 或改用另一绿色入口均失败。生产 PR8B composition root 不注册真实 adapter、不 spawn `run`，由 B8 静态扫描与 fresh runtime registry共同证明。

### 11.1 原子性与 schema

- target/base/typed extension/head/initial intent/state 同事务 commit/rollback；
- reverse verifier 拒绝零/多 extension、缺 head、head 跨 target、单删和 identity move；
- predecessor/generation/head composite FK 可建立，`UNIQUE(predecessor_dispatch_id)` 拒绝双 successor；
- base `PRIMARY KEY(id)` 拒绝跨 kind/project 重用 UUID，composite FK 仍证明 exact project/kind；
- intent initial CHECK 与 generation-bearing composite FK 拒绝 replacement 指向 generation>0；old relation composite FK 与 row CHECK 拒绝 cross-project、cross-kind、same-target replacement；state 三列 FK/NULL matrix 拒绝 status 与 absorbing settlement kind 错配；
- delivery attempt 的 exact intent relation、claim token/phase/state composite FK与deferred verifier拒绝cross-dispatch pointer、offering引用settled、awaiting引用inflight/consumer_started、第二次claim和settled outcome改写；returned job composite FK必须等于intent expected ID，六种outcome逐项满足NULL matrix；
- 统一absorbing cleanup覆盖normal terminal、advanced、replacement、cancel、`DISPATCH_BUDGET_ORPHAN`与`DISPATCH_FENCE_ORPHAN`；ObservedDelivery/replacement/cancel/reaper对应disposition正确，任何absorbing state残留inflight attempt都失败；
- fresh schema实际建立intent expected-job、attempt/state与observation的全部exact composite FK；缺少任一referenced unique key、列序/类型不一致或deferred verifier缺失均使catalog test失败；
- delivery observation以exact attempt/dispatch FK、每observer唯一性及observer/outcome/NULL CHECK matrix证明bounded append-only；publisher result与consumer-first、lease-expiry、replacement、cancel分别做双序race：attempt CAS败方不得覆盖settled disposition，只能留下合法observation或重读；重读后consumer begin、replacement/cancel吸收与absorbing幂等返回仍必须完成；
- `advanced` 只指向同 target successor；`superseded` 只指向不同 target 的 exact generation-0 replacement，XOR/FK/unique 拒绝混填、缺填和共享 replacement；
- dispatch intent immutable，state NULL matrix 覆盖全部 transition；
- 同 target/fence initial stage 幂等，不同 fence 冲突；terminal target 不重开；
- 六类 `TargetFenceV1/KBTF` Rust/SQL fixed golden 和逐字段篡改负例；
- `BidDeliveryV1/KBDL` Rust/SQL fixed golden、explicit job name、expected job ID 与 payload/version 篡改负例；
- settlement、inbound outcome 和 typed evidence canonical key/hash 并发 insert-or-read 只复用同一语义；
- conversion completion→extraction 与 matching schedule→manifest+0..N jobs/dispatches 原子，零 route 成功 terminal。
- generic replacement 的新 target+initial dispatch/current pointer与旧 nonabsorbing target cleanup/`superseded` settlement同事务；ready/offering/awaiting/running/absorbing 五态、late publisher、replacement-vs-heartbeat与replacement-vs-publish均有并发测试并证明旧 owner不能发布；并发 mutation只创建一个 replacement，旧 delivery的 historical noop引用 exact superseded settlement；各 family 纵切再验证真实 current pointer。
- generic cancel覆盖 ready/offering/awaiting/running/absorbing五态与late publisher/delivery；matching mutation在同事务替换旧 schedule并取消旧 manifest下0/1/N nonterminal jobs。

### 11.2 一次性 transport 与 start deadline

- offer claim 一旦 commit，同 dispatch 永远不能第二次 claim 或外部 enqueue；
- claim 后、Redis 前 crash；enqueue Ok、Err、timeout、response lost 四种路径都只进入 awaiting/running/absorbing，不回 ready；
- Redis HSET-only/hash-only unique job 模拟后，successor 新 ID 仍执行；旧 ID 永不复用；
- enqueue succeeded 但 publisher DB settle 前 crash，consumer-first 可从 offering begin；晚到 publisher 只写 observation；
- publisher先把attempt结算为`enqueue_returned`并进入awaiting后，consumer仍创建恰好一个business owner并进入running，delivery attempt保持原settled disposition不变；
- publisher lease expiry 不会重投同 ID；delivery-start deadline 才创建新 dispatch；
- delivery-start deadline与publisher settle双序race只产生一个successor：reaper先提交时inflight attempt结算为`publisher_lost`且晚publisher只observation；publisher先提交时保留其settled disposition再吸收；old state/attempt matrix始终合法；
- gate rebase与publisher settle双序race也只产生一个successor：rebase先提交时inflight attempt结算为`superseded`且晚publisher只observation；publisher先提交时保留其settled disposition后advance；不得伪造`publisher_lost`；
- deadline-vs-consumer begin 并发只得到“旧 dispatch 运行”或“successor current+旧 delivery noop”；
- Redis flush、membership-only、七天 cleanup 和相同 hostname/PID restart 后，新 dispatch 在时限内恢复；
- `get_job/list/stats/delete_job` 不参与正确性，source denylist 覆盖 Bid/WorkTransport；
- 连续未 start 遵守指数 backoff、lane/global governor 和 readiness 告警，不 busy-loop。

### 11.3 Consumer、lease 与 fencing

- duplicate 并发 begin 只有一个 business owner，其余 durable noop；
- historical advanced、cross-target superseded与terminal delivery在任何外部调用前noop并引用 exact absorbing disposition；current gate/target stale与owner-expired noop引用 exact repair obligation而不伪造尚不存在的 settlement；wrong job ID/version引用 rejected-delivery；正确payload + 错误`JobContext.meta.id`的 external I/O count必须为0；
- owner-expired→gate-stale、gate-stale→target-stale与三类handler-vs-repair双序race证明每个current stale inbound提交时只引用unresolved exact obligation；任一后续absorbing transaction按dispatch锁定并解析全部obligations，absorbing state残留unresolved、cross-dispatch resolution和修改历史inbound均被拒绝；
- rejected delivery的expected tuple必须exact FK到intent，observed tuple必须不同且mismatch kind与差异字段一致；expected、mismatch kind、inbound reason任一错配均被拒绝；
- begin 前 worker crash 由 start deadline 恢复；begin 后 crash/卡死由 business lease 精确 reap；
- business attempt在 begin 取得 owner时计数；N-1/N边界、retryable failure与连续 crash/reap都只能 advance到剩余 budget或原子 `attempts_exhausted` terminal，绝不创建不可 claim successor；
- owner fresh 不被 deadline/reaper 抢占；gate/head 改变后旧 owner 立即 fenced 且不能 heartbeat/publish；
- gate stale可在 fence仍有效时rebase；target fence stale只能走 domain replacement/cancel，orphan stale target必须 poison且 readiness fail，same-target successor count=0；
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
