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
run(shutdown) -> Result<(), DispatchFatal>
```

- `stage` 为 crate-private，只能由所属 target 的受检 mutation 在现有事务中调用；API/worker 不得在 commit 后直接调用 Redis。
- `run` 由 worker composition root 启动，内部运行 due-intent pump、delivery consumer、receipt retirement 和 bounded target repair。Redis unavailable/timeout 由 durable claim 按冻结 backoff 内部收敛，不终止 `run`；只有 adapter/registry/capability mismatch 或 transport identity conflict 返回 `DispatchFatal` 并使 readiness fail closed。
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

六张 final dispatch typed extension 固定为 `bid_async_document_conversion_targets`、`bid_async_extraction_targets`、`bid_async_matching_schedules`、`bid_async_matching_jobs`、`bid_async_attachment_preparations`、`bid_async_submission_renders`。每张以 base `id` 为 PK/FK，保存同 project 与 constant kind，并以另一个 `(id,project_id)` composite FK 指向真实 executable domain target：分别是新建的 `bid_document_conversion_targets`，以及现有的 `bid_extraction_targets`、`bid_matching_schedule_intents`、`bid_matching_jobs`、`bid_attachment_preparation_jobs`、`bid_submission_render_jobs`。extension 只证明 dispatch identity/relation，不复制业务状态、generation、claim 或结果。

base identity 固定为 `(target_kind,id)`；UUID 每个异步执行 identity 唯一，generation/watermark 只作为 typed fence，不重复充当 identity。document conversion 每个 generation 创建新的稳定 target ID，`BidDocument` 只保存 current conversion target pointer；重试不再原地复用 document ID 充当多个 target identity。其它现有 job/target ID 直接成为 base target ID。terminal/superseded/poisoned target 不重开；人工 retry 或新 mutation 创建 successor target identity。

target、typed extension、dispatch intent 与 ready state 必须同一事务提交。普通 API/worker 无权单独插入 `bid_async_targets`，也不能创建没有 typed extension、dispatch intent 或 state 的孤儿。

PR8B 只建立 dormant base/extensions/catalog；由于 conversion extension 的 FK 必须在建表时可解析，PR8B 同时建立空的 `bid_document_conversion_targets` domain table 形状，但不在 `BidDocument` 安装 current pointer、不创建行也不注册 owner。PR8B 不给六张 domain target 表安装“必须存在 extension”的反向 trigger，不补建旧行、不双写。PR8C～PR8E 每切换一个 family，才让其 mutation 原子创建 domain target+base+extension+intent+offer 0+state，同时安装 deferred reverse verifier，要求该 family 每个 dispatchable domain row commit 时有且仅有一个 extension，并删除旧 owner。`document_conversion` producer/current pointer 从 PR8C 激活。PR8F 在 fresh final baseline 证明六类 reverse verifier 全部启用且没有 legacy owner。

catalog 约束固定为：

- base composite unique 覆盖 `(id,target_kind,project_id)`；每张 typed 表保存同 project 与 constant kind，并以 composite FK 回指 base；
- 六张真实 domain target 表都提供 `UNIQUE(id,project_id)`，供 typed extension 与 settlement evidence 的 composite FK 引用；target ID/project identity immutable，不能靠单列 ID FK 后再由应用比较 project；
- deferred constraint verifier 在 base、六张 extension、intent/state/offer 的 `INSERT/UPDATE/DELETE` 后验证同一 base 在 commit 时恰有一个 extension、一个 intent、一个 state 和 current offer；family cutover 后 domain target reverse verifier 同时启用；
- intent 对 `target_id` 使用 `UNIQUE`，state 以 `dispatch_id` 作为 PK/FK；上述双向 deferred verifier 提供反向存在性，不能只依赖这些“至多一个”的 UNIQUE/FK；
- verifier 使用固定 `search_path` 的受控函数，普通 API/worker 只执行所属 mutation/stage function，没有这些表的直接 DML 权限；
- base/extension/intent/offer 的 identity 列全部 immutable；禁止把一行 UPDATE/move 到另一个 aggregate。`SET CONSTRAINTS ALL IMMEDIATE` 必须拒绝零/多 extension、extension→domain FK mismatch、project/kind mismatch、缺 intent/state/current offer、单删和 identity move/swap。

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

V1 codec 固定为 binary：ASCII `KBTF` + `u16be(schema=1)` + `u8(kind_tag)` + `u16be(field_count)`，随后按下表顺序为每个字段编码 `[u8 type_tag,u32be length,value]`。type tag 固定为 `1=uuid_send(16 bytes)`、`2=nonnegative int8send(8 bytes)`、`3=lowercase-hex decode 后的 SHA-256(32 bytes)`、`4=exact UTF-8 text`；text 禁止 trim、Unicode normalization 或 locale 转换。tag 1 固定用于名称以 `_id` 结尾的 UUID 字段及 `target_id,project_id,document_id,attachment_id,requested_by,route_id`；tag 2 固定用于 generation/revision/watermark/count/length/attempt/lease 数值字段；tag 3 固定用于名称以 `_sha256` 结尾的 digest；其余字段固定为 tag 4。任何 `_id` 若领域实际不是 UUID，必须在下表直接改名为明确 text 字段，禁止实现时自行改变 tag。缺字段、多字段、null、负数、非 canonical 数值、无效 UTF-8 或错误长度全部拒绝；digest 为完整 bytes 的 lowercase hex SHA-256。六类 kind tag/字段顺序是唯一权威输入：

| kind tag | target kind | ordered fields |
| --- | --- | --- |
| 1 | `document_conversion` | `target_id,project_id,document_id,conversion_generation,original_object_ref,original_sha256,file_name,media_type,byte_length,conversion_snapshot_id,feature_snapshot_id,max_attempts,claim_lease_ms` |
| 2 | `extraction_target` | `target_id,project_id,document_id,source_artifact_id,conversion_generation,extraction_generation,source_markdown_sha256,source_byte_length,source_image_asset_set_sha256,converter_contract_version,target_config_snapshot_id,feature_snapshot_id,router_contract_version,policy_version,prompt_version,output_schema_version,expected_section_count,max_attempts,claim_lease_ms` |
| 3 | `matching_schedule` | `target_id,project_id,generation,mutation_watermark,matching_config_snapshot_id,feature_snapshot_id,score_policy_snapshot_id,verifier_policy_snapshot_id,retrieval_contract_version,retrieval_policy_sha256,max_attempts,claim_lease_ms` |
| 4 | `matching_job` | `target_id,project_id,manifest_id,manifest_content_sha256,generation,mutation_watermark,route_id,route_scope_sha256,matching_config_snapshot_id,feature_snapshot_id,score_policy_snapshot_id,verifier_policy_snapshot_id,max_attempts,claim_lease_ms,lease_policy_generation` |
| 5 | `attachment_preparation` | `target_id,project_id,attachment_id,attachment_revision,attachment_kind,source_object_ref,source_content_sha256,source_validation_sha256,media_type,byte_length,conversion_snapshot_id,feature_snapshot_id,preparation_contract_version,max_attempts,claim_lease_ms` |
| 6 | `submission_render` | `target_id,project_id,manifest_id,expected_manifest_sha256,render_job_snapshot_id,render_job_snapshot_sha256,render_config_snapshot_id,feature_snapshot_id,requested_by,idempotency_key_sha256,max_attempts,claim_lease_ms` |

PR8B 先建立无 producer 的 stable conversion target 表及其 FK 形状；PR8C 才安装 `BidDocument` current pointer/producer并冻结源元数据与 retry policy。extraction 必须冻结 router/retry identity并按 exact source artifact 读取。PR8D 必须新增 attachment preparation contract identity和 render idempotency digest verifier。PR8E 必须冻结 matching schedule 的 retrieval contract + policy digest、补齐 schedule execution identity；matching job 可从 immutable manifest relation 重算字段，但不得读取 current projection。`dispatch_semantics_snapshot_id` 属于 intent identity，另行受检，不重复放入 typed fence。

以下 live 值明确不入 fence：project/document/attachment/job status，current pointers/current watermark，attempt count，claim token/owner/heartbeat/lease expiry，published count，staging/output/current report ID，error/timestamp，maintenance gate epoch，ObjectRegistry availability/owner state。它们在 begin/heartbeat/publish 的 live CAS 中验证。六个 Rust/SQL golden 必须消费同一份只含输入与固定 expected bytes/hash 的 fixture；禁止由一方运行时生成另一方预期。

### 3.3 Mutable state 与 attempts

`bid_dispatch_states` 与 intent 一对一：

```text
status = ready|offering|observing|terminal|superseded|poisoned
next_offer_at
next_probe_at
observing_since
repair_requested_at
offer
offer_gate_epoch
offer_claim_token, offer_claimed_by, offer_lease_expires_at
current_offer_attempt_id
active_transport_receipt_id
probe_claim_token, probe_claimed_by, probe_lease_expires_at
current_probe_attempt_id
last_transport_code
terminal_code, terminal_settlement_id, completed_at
```

state 以 `dispatch_id` 为 PK/FK，`(dispatch_id,offer)` FK 到 current immutable offer；current offer/probe attempt、active receipt、terminal settlement 都携带 dispatch+offer composite FK。deferred verifier 同时检查 pointer kind、status/null matrix和 current offer一致性；任何单列 UUID FK 或无 kind 的多态引用都不合格。

`bid_dispatch_offers` 保存完整 `PreparedDelivery`：`dispatch_id,offer,gate_epoch,contract_version,transport_policy_version,physical_lane,task_type,unique_identity,payload_version,canonical_payload_bytes,resurrect,on_conflict,tombstone_retention_ms,transport_id,receipt_fingerprint,created_at`。`dispatch_id` 以 DEFERRABLE FK 指向 intent，主键 `(dispatch_id,offer)`，并提供 `UNIQUE(transport_id)`、`UNIQUE(dispatch_id,offer,transport_id,receipt_fingerprint)`。`stage/advance_offer` 分别创建 offer 0/successor，PostgreSQL 独立重算 `KBWT` bytes并验证完整 transport policy tuple；同 offer retry 只能读取逐字节等价记录。

`bid_dispatch_attempts` 每次外部 offer/probe/retire 调用一行，至少保存 `id,dispatch_id,offer,attempt_seq,attempt_kind,gate_epoch,runtime_governor_generation,claim_token,started_at,settled_at,outcome,transport_id,receipt_fingerprint,receipt_phase,phase_epoch,phase_age_ms`；`(dispatch_id,offer)` 以 DEFERRABLE composite FK 指向 exact immutable offer，行先由 claim 创建，再只允许一次 CAS finalize。提供 `UNIQUE(dispatch_id,offer,attempt_seq)`、`UNIQUE(id,dispatch_id,offer)`，并以 deferred verifier 检查 pointer 的 exact `attempt_kind`。state 的 `(current_offer_attempt_id,dispatch_id,offer)`、`(current_probe_attempt_id,dispatch_id,offer)` 与 retirement 的 `(current_retire_attempt_id,dispatch_id,old_offer)` 分别使用 composite FK；建立 `ON (dispatch_id) WHERE settled_at IS NULL AND attempt_kind='offer'`、`ON (dispatch_id) WHERE settled_at IS NULL AND attempt_kind='probe'`、`ON (dispatch_id,offer) WHERE settled_at IS NULL AND attempt_kind='retire'` 三个 partial unique index，保证每个 dispatch 同时至多一个未结算 offer/probe attempt、每个 retirement 至多一个未结算 retire attempt。attempt kind 与其不适用的 receipt phase 字段使用受检 NULL matrix。

`bid_dispatch_attempt_observations` append-only 保存任一 attempt 已 finalize 后才到达的同 identity offer/probe/retire result；它以 `(attempt_id,dispatch_id,offer)` composite FK 引用 exact attempt，`UNIQUE(attempt_id,result_sha256)` 使同结果 insert-or-read，每 attempt 最多 8 条不同 observation，超额只记 bounded metric且不能改变 attempt disposition、claim 或 current state。consumer-first 可 finalize enqueue attempt 为 `accepted_consumer_first` 并清 publisher claim；publisher 随后同 receipt 只 insert-or-read observation并返回幂等成功，不同 fingerprint 始终 fatal。

`result_sha256` 固定为 `AttemptObservationV1` bytes 的 lowercase SHA-256：ASCII `KBAO` + `u16be(1)` + `uuid_send(attempt_id)` + `uuid_send(dispatch_id)` + `u64be(offer)` + `lp_ascii(attempt_kind)` + `lp_ascii(result_code)`，随后按 result shape 以 `u8(0|1)` 编码 optional receipt identity、phase和 terminal code。receipt present 写 `lp_ascii(contract_version)+lp_ascii(policy_version)+lp_ascii(transport_id)+32-byte fingerprint`；phase present 写 `lp_ascii(queued|processing)+u64be(phase_epoch)`；terminal present 写 allowlisted terminal code。`phase_age_ms`、transport audit time、latency和本地 receive timestamp排除，避免同一语义因观测时间不同产生新 row。Rust/SQL 从 attempt 与 bounded result fields 独立重算并用 golden/shape verifier拒绝游离 digest。

`bid_dispatch_transport_receipts` 保存 `id,dispatch_id,offer,transport_id,receipt_fingerprint,first_observed_at`，以 composite FK 回指 exact offer，并使用 `UNIQUE(dispatch_id,offer)`、`UNIQUE(transport_id)`、`UNIQUE(id,dispatch_id,offer)`。publisher/consumer 只能 insert-if-null-or-equal；state 的 `(active_transport_receipt_id,dispatch_id,offer)` composite FK 绑定 current offer。`first_observed_at` 是 DB `observing_since` 的来源，不与 Redis timestamp 相减。

所有可 ACK 的业务语义归一为 immutable `bid_dispatch_settlements`：保存 `id,settlement_key,dispatch_id,offer,settlement_kind=offer_advanced|terminal|superseded|poisoned|noop,outcome_code,new_offer,gate_epoch,transport_receipt_id,rejected_delivery_id,predecessor_settlement_id,reason_code,created_at`。提供 `UNIQUE(settlement_key)` 与 `UNIQUE(id,dispatch_id,offer)`；old offer 使用 `(dispatch_id,offer)` FK，可选 exact receipt 使用 `(transport_receipt_id,dispatch_id,offer)` FK，可选 successor 使用 `(dispatch_id,new_offer)` FK，可选 rejected delivery 使用 `(rejected_delivery_id,dispatch_id)` FK；historical/stale noop 的 `(predecessor_settlement_id,dispatch_id,offer)` 以另一个 composite FK 指向使同一 exact offer 失效的 `offer_advanced|terminal|superseded|poisoned` settlement，全部 DEFERRABLE。`offer_advanced` 检查 `new_offer=offer+1`，并建立 `CREATE UNIQUE INDEX ... ON bid_dispatch_settlements(dispatch_id,offer) WHERE settlement_kind='offer_advanced'`；另建 `CREATE UNIQUE INDEX ... ON bid_dispatch_settlements(dispatch_id) WHERE settlement_kind IN ('terminal','superseded','poisoned')`，分别保证每个 old offer 至多一个 advance、每个 dispatch 至多一个 absorbing settlement。state 的 `(terminal_settlement_id,dispatch_id,offer)` 与 retirement 的 `(trigger_settlement_id,dispatch_id,old_offer)` 使用 composite FK；deferred verifier分别要求 absorbing kind、允许触发 retirement 的 kind/outcome，并检查 successor/receipt/rejected-delivery/predecessor NULL matrix。并发相同语义只能通过 `INSERT ... ON CONFLICT(settlement_key)` insert-or-read 同一 settlement ID，不能先随机生成两个 sidecar再事后合并。

typed evidence 使用六张 extension 表 `bid_dispatch_{conversion,extraction,matching_schedule,matching_job,attachment_preparation,submission_render}_settlement_evidence`。每张以 `settlement_id` 为 PK，并统一保存 `settlement_id,dispatch_id,offer,target_id,project_id,evidence_kind=normal|contract_poison,evidence_sha256`；normal 分支另含互斥的 finalized-attempt/owner-observation 字段与可选 result artifact，contract-poison 分支另含 mismatch code 和 stored/recomputed digest。`(settlement_id,dispatch_id,offer)` FK 指向 exact settlement、`(target_id,project_id)` FK 指向该 family 的真实 target；attempt/result 非空时再用 family-specific composite FK 指向 append-only attempt/immutable result。deferred exact-one verifier 按下表要求零或一张正确 extension，检查两种 evidence kind 的 NULL matrix，并独立重算 settlement key。

normal `evidence_sha256` 固定为 `SettlementEvidenceV1` bytes 的 lowercase SHA-256：ASCII `KBEV` + `u16be(1)` + `u8(family_kind_tag)` + `uuid_send(dispatch_id)` + `u64be(offer)` + immutable offer 的 `u64be(gate_epoch)` + `uuid_send(target_id)` + `uuid_send(project_id)` + 32-byte `TargetFenceV1` digest，随后写 attempt evidence variant 与 result artifact optional。`family_kind_tag` 必须逐值复用上文 `TargetFenceV1/KBTF` 的 1..6 映射，不建立第二套编号。attempt variant 固定为：`u8(0)` 无 attempt；`u8(1)+uuid_send(attempt_id)+lp_ascii(immutable_finalized_outcome)` 已终结结果；`u8(2)+uuid_send(attempt_id)+lp_ascii(fresh|expired)` owner observation。owner observation 只能由受检函数在锁定 exact running attempt、验证当时 owner 三态后派生并冻结到 immutable evidence row，调用方不能提交 owner class；后续 heartbeat、success 或 reap 不再改变该 observation。result present 写 `u8(1)+uuid_send(result_artifact_id)+32-byte immutable_result_content_sha256`，否则写 `u8(0)`。`settlement_id/settlement_key`、live target status、claim、timestamp和错误文本明确排除，避免循环。PostgreSQL creation verifier 沿 family-specific FK 读取真实 target/attempt/result，独立重算 TargetFence；finalized branch 读取已终结 outcome，owner-observation branch 只在创建时从锁定行派生 class，后续 verifier 从 immutable evidence row 重算 bytes并继续验证 attempt→target FK，禁止退回读取已变化的 outcome或信任 caller 提供的游离值。

contract-poison 使用独立 `ContractPoisonEvidenceV1`，其 `evidence_sha256` 是 ASCII `KBCP` + `u16be(1)` + `u8(family_kind_tag)` + `uuid_send(dispatch_id)` + `u64be(offer)` + immutable offer 的 `u64be(gate_epoch)` + `uuid_send(target_id)` + `uuid_send(project_id)` + `lp_ascii(mismatch_code)` + 32-byte stored digest + 32-byte recomputed digest 的 lowercase SHA-256。`family_kind_tag` 同样复用 KBTF 1..6；mismatch code 与两个 digest 的唯一来源如下：

| mismatch code | stored digest | recomputed digest |
| --- | --- | --- |
| `target_fence` | hex-decode `bid_dispatch_intents.target_fence_sha256` | SHA-256(exact typed target 重建的 KBTF bytes) |
| `canonical_payload` | SHA-256(`bid_dispatch_offers.canonical_payload_bytes`) | SHA-256(dispatch/offer/lane 重建的 KBBD bytes) |
| `prepared_delivery` | hex-decode `bid_dispatch_offers.receipt_fingerprint` | SHA-256(immutable offer 全字段重建的 KBWT bytes) |

受检函数必须从锁定的 durable row取得两侧输入并独立重算，且 stored/recomputed 必须不等；caller 不能提交 digest。该分支禁止 attempt/result 字段，并替代 normal KBEV 的“TargetFence 必须一致”规则。FK/typed relation、catalog/registry 或 adapter closure 损坏不是 item poison：这些结构性错误必须事务零写入并使 readiness fail closed。

| settlement kind/outcome | exact receipt | typed evidence |
| --- | --- | --- |
| `offer_advanced/business_retry`, `offer_advanced/owner_reaped` | required | required；attempt outcome 分别为 retry-scheduled/reaped |
| `offer_advanced/receipt_absent`, `offer_advanced/gate_rebase` | optional/equal if observed | forbidden；expired owner 必须先转成 `owner_reaped` |
| `offer_advanced/processing_stalled`, `offer_advanced/transport_terminal_without_settlement` | required | forbidden；`reason_code` 冻结 exact terminal code，expired owner 必须先转成 `owner_reaped` |
| `terminal/success`, `terminal/deterministic_failure`, `terminal/retry_exhausted` | required for handler-origin | required；success 还要求 immutable result FK，failure 要求 exact attempt outcome |
| `superseded/target_cancelled`, `superseded/target_superseded`, `superseded/target_stale` | optional | required target evidence；active attempt 可选但若存在必须 exact |
| `poisoned/contract_poison` | optional | required `KBCP` mismatch evidence；禁止 normal KBEV |
| `noop/duplicate_fresh_owner`, `noop/owner_expired` | required | required owner-observation KBEV，class 分别为 fresh/expired；不得读取其未来 outcome |
| `noop/gate_stale`, `noop/target_stale`, `noop/target_terminal`, `noop/historical_offer` | required | target_stale/target_terminal required target evidence；gate_stale/historical_offer forbidden；除 gate_stale 外均须引用使 target/offer 失效的 exact advance/absorbing predecessor |
| `noop/transport_mismatch` | forbidden | forbidden；必须引用 rejected delivery |

`bid_dispatch_rejected_deliveries` append-only 保存 `id,dispatch_id,actual_physical_queue,actual_task_type,actual_transport_id,actual_receipt_fingerprint,actual_payload_bytes,reason_code,canonical_sha256,created_at`；payload 仍受 `bid-delivery/v1` 的 128-byte 上限。表提供 `UNIQUE(id,dispatch_id)` 与 `UNIQUE(dispatch_id,canonical_sha256)` 并 FK 到 dispatch，但不冒充 exact offer receipt；同一 dispatch 最多保留 64 个不同 rejected identity，超限时不新增 Bid 行、返回 `unsettled` 让 transport 写 terminal audit并增加 overflow metric。

`canonical_sha256` 固定为 `RejectedDeliveryV1` bytes 的 lowercase SHA-256：ASCII `KBRJ` + `u16be(1)` + `uuid_send(dispatch_id)` + `lp_ascii(actual_physical_queue)` + `lp_ascii(actual_task_type)` + `lp_ascii(actual_transport_id)` + 32-byte actual receipt fingerprint + `u32be(payload_length)+exact actual_payload_bytes` + `lp_ascii(reason_code)`。surrogate `id`、authoritative current offer、timestamp与日志文本全部排除。PostgreSQL 从受限行字段独立重算，Rust/SQL golden 固定 bytes/hash；调用方不能直接提交 canonical hash。

`noop_transport_mismatch` settlement 的 `offer` 固定为事务内锁定的 authoritative current offer，actual unknown/wrong offer 只写 rejected delivery；因此 settlement 的 offer FK 永远可满足且不把 actual context 伪装成合法 receipt。`bid_dispatch_inbound_receipts` 仍只是 ACK sidecar，保存 `settlement_id` PK、`dispatch_id,offer,settled_at`，并以 `(settlement_id,dispatch_id,offer)` composite FK 指向 exact settlement。所有 ACK 必须先在同一事务创建或幂等读取 settlement+所需 evidence+sidecar；同一 receipt 可以先后证明合法 noop 与最终业务 settlement，不能靠 receipt-level UNIQUE 丢掉其中一条，但相同 settlement key 必须复用同一 sidecar。DB 写失败只能返回 `unsettled`。

`settlement_key` 固定为 `SettlementKeyV1` canonical bytes 的 lowercase SHA-256。bytes grammar 为 ASCII `KBDS` + `u16be(1)`，随后依次写 `lp_ascii(settlement_kind),lp_ascii(outcome_code),uuid_send(dispatch_id),u64be(offer),u64be(gate_epoch)`，再按固定顺序写六个 optional：exact receipt、rejected delivery、new offer、predecessor settlement、reason code、typed evidence。`gate_epoch` 必须取 settlement 所引用 immutable offer 的 frozen epoch，即使 current gate 已 promotion 也不能改用 live epoch。optional 统一先写 `u8(0|1)`；present receipt 写 `lp_ascii(transport_id)+32-byte fingerprint`，present rejected 写其 32-byte canonical SHA-256，present new offer 写 `u64be`，present predecessor 写 predecessor 的 32-byte settlement key，present reason 写 `lp_ascii(reason_code)`，present evidence 写 32-byte evidence digest。`lp_ascii` 是 `u32be length + allowlisted ASCII bytes`；所有枚举、数值范围与 optional shape 由上表唯一决定，禁止 surrogate row ID、timestamp、JSON、trim、默认 null或额外字段进入 bytes。Rust builder 与 PostgreSQL 受检函数独立重算并消费固定 expected bytes/hash golden；缺失、多余、顺序错误或 evidence shape 错误一律拒绝。

`bid_dispatch_transport_retirements` 保存 `id,trigger_settlement_id,dispatch_id,old_offer,status=pending|claiming|settled,next_attempt_at,claim_token,claimed_by,lease_expires_at,current_retire_attempt_id,outcome=retired|already_terminal,settled_at,reopen_count,last_reopened_at`；`(dispatch_id,old_offer)` FK 引用 immutable old offer，`(trigger_settlement_id,dispatch_id,old_offer)` FK 引用允许触发 retirement 的 exact settlement，`UNIQUE(dispatch_id,old_offer)`。任何 `advance_offer` 或吸收态 settlement 都创建 obligation，包括 current handler terminal；它引用 immutable offer identity而不是是否已 observed receipt。

retirement transition 固定为：

| 事件 | 前置与 CAS | 原子结果 |
| --- | --- | --- |
| create | `advance_offer` 或 absorbing settlement 同一事务 | insert-or-equal `pending,next_attempt_at=now`；trigger 不同即 invariant failure |
| claim | `pending,next_attempt_at<=now` 且无 current attempt | 插入 `attempt_kind=retire`，设置 `claiming`、exact token/lease/current attempt后提交 |
| `retired`, `already_terminal` | exact token/lease/attempt | finalize attempt，清 claim/current attempt，设置 `settled` 与 outcome/settled_at |
| unavailable | exact token/lease/attempt | finalize attempt，清 claim/current attempt，回 `pending` 并按 frozen backoff 设置 next due |
| identity conflict/adapter mismatch | exact token/lease/attempt | finalize attempt，清 claim/current attempt，保持 `pending`、记录 fatal code并设置 bounded fatal backoff；`run` fail closed，修复 capability 后仍可重试 |
| lease expired | DB time 已过 exact lease | finalize exact attempt=`lease_expired`，清 claim/current attempt，回 `pending` 并设置 bounded next due |
| reopen after transport loss | obligation 已 `settled`，但 historical/absorbing offer 又被 exact `accepted`、`present` observation或 handler context 证明 active | 保留原 trigger，清 outcome/settled_at，递增 reopen_count并回 `pending,next_attempt_at=now`；同一事务记录 observation/noop，不恢复 dispatch/business 权限 |
| late result | attempt 已 finalize或 obligation 已 settled | 只追加 attempt observation；同 identity 的 terminal/retired 结果幂等，不改当前状态，不同 fingerprint fatal |

claim 后无锁调用 `retire_exact`，任何 transition 都不在 Redis I/O 期间持 PostgreSQL lock。handler finish、resurrection 与 retire 使用同一 transport 原子边界：retire 先线性化时后续 finish/resurrection 观察 retired tombstone，finish 先线性化时 retire 返回 `already_terminal(TerminalView)`；两种次序都结算同一 obligation。完整 Redis volume 丢失会同时删除 tombstone，平台不伪装跨 volume durability；若旧 publisher/handler 此后让 historical receipt 重新 active，上述 reopen transition 保证它只能 durable noop并再次 retire，不能恢复业务执行权。

offer claim 使用 DB time、`FOR UPDATE SKIP LOCKED`、固定 batch 和短 lease。dispatcher 复验 current prepared bytes，在 claim 事务中写 `status=offering`、token/lease/current attempt/`next_offer_at=lease expiry`，提交后才调用 Redis。offer-claim reaper 按 `offer_claim_token IS NOT NULL AND offer_lease_expires_at<=now` 的 partial index领取，不以 `status='offering'` 为唯一条件；它 finalize exact attempt并清 claim，已有 receipt/owner时保持 observing，否则回 ready。Redis deadline 必须短于 offer lease。

probe 使用独立短租约：`claim_probe` 只从 `observing,next_probe_at<=now,current_probe_attempt_id IS NULL` 领取；过期 attempt 必须先由 reclaim 原子 finalize，不能被新 claim覆盖。每个保持 observing 的 settle 都设置新的 `next_probe_at`；unavailable/expired 使用 bounded backoff。Redis probe 期间不持 PostgreSQL row lock，late result 只能写 observation。

state NULL/timer matrix 是 deferred verifier 的一部分：`ready` 只有 `next_offer_at` 必填；`offering` 要求 exact offer claim/current attempt/lease，且没有 active receipt/probe claim；`observing` 要求 active receipt、`observing_since`、`next_probe_at`，offer claim 必须已清，probe claim 与 current probe attempt 要么同时为空要么同时有效；`repair_requested_at` 只允许在 observing 且 owner/gate 需要 repair 时存在。三个吸收态只保留 current immutable offer 与 terminal settlement pointer，所有 due、claim、attempt、active receipt、observing/repair 字段必须为空。每次 insert/transition 都在同一事务设置或清理完整字段组，禁止依赖后续 sweep 修 NULL matrix。

`offer` 只在业务 retry、target-local repair、gate epoch rebase，observing probe=`absent` 且 owner=`none`，或 probe=`present(processing)`、owner=`none` 且超过 `consumer_start_deadline/max_observing_age` 时单调增加。`present(queued)+none` 表示 lane backlog，不复制 delivery；dispatcher 降低 admission/readiness 并继续 bounded probe。probe=`present` 且 owner=`fresh` 由 business lease 控制；owner=`expired` 必须先精确 reap。probe=`unavailable` 保持 observing，不猜测丢失。尚未得到可靠 Redis receipt 的 unavailable/timeout 只释放当前 offer，并且只能在相同 `offer_gate_epoch` 下重试同一 unique identity；即使第一次请求其实已被 Redis 接受，Oxana duplicate 与 consumer CAS 仍保持幂等。gate epoch 改变后禁止重试旧 identity，必须先完成 §4.5 的 epoch rebase 并使用 `offer + 1`。

所有 `offer + 1` 路径只能调用一个内部 `advance_offer(reason,new_gate_epoch,prepared_successor)` 原语。prepared successor 在事务外只用 pure `prepare` 计算；原语在统一锁序下独立验证 bytes、插入 successor offer、追加 settlement、finalize/reap exact旧 attempt、创建 old-offer retirement，推进 offer/epoch，并清空 offer/probe claims、current attempts、active receipt、`next_probe_at/observing_since/repair_requested_at`，最后设置 `ready,next_offer_at=due`。

所有 `terminal|superseded|poisoned` 路径只能调用 `settle_dispatch_absorbing(kind,outcome,typed_evidence,inbound)`：同事务 finalize/clear current offer/probe attempt 与 claim，写唯一 absorbing settlement/evidence和可选 inbound sidecar，为 current offer 创建 retirement obligation，设置 `terminal_settlement_id/completed_at` 并进入吸收态；同时把 `next_offer_at,next_probe_at,observing_since,repair_requested_at,active_transport_receipt_id` 清为 null。即使 current handler 已提交业务结果但在 ACK 前卡死，retirement pump 仍会清理 processing receipt；late publisher/probe/handler 只能形成幂等 observation/noop，不能复活 state。

### 3.4 Minimal delivery

Redis payload 固定为：

```text
schema = bid-delivery/v1
dispatch_id
offer
lane_key
payload_version = 1
```

canonical payload bytes 固定为 ASCII `KBBD` + `u16be(1)` + `uuid_send(dispatch_id)` + `u64be(offer)` + `u16be lane_length + exact ASCII lane_key`；字段无 null/extra，最大 128 bytes。它作为平台 `prepare` 的 `canonical_payload_bytes`，transport policy 固定 `bid-durable-transport/v1`。

unique identity：

```text
bid:delivery:v1:<dispatch_id>:<offer>
```

Bid V1 immutable offer 的 `transport_id` 由该 unique identity 按平台 canonical transport identity 合同确定；`receipt_fingerprint` 由完整 canonical offer envelope 确定。同 identity/envelope 必须在 Redis I/O 前得到相同 ID/fingerprint，Oxana create/duplicate、probe、retire 和 handler context 都必须返回并验证这两个值。若平台 adapter 不能预先确定 exact ID/fingerprint，本方案不得切换到该 adapter。

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
| stage | base/typed target 可投递且 exact-one 完整；prepared offer 0 通过 DB verifier | 创建 intent+immutable offer 0；state=`ready,offer=0,offer_gate_epoch=current,next_offer_at=now` |
| claim offer | state=`ready`、due、gate=`open`、state epoch=current；prepared bytes 等于 immutable current offer | 插入 enqueue attempt；state=`offering`；写 token/lease/current attempt/`next_offer_at=lease expiry` |
| current offer accepted/duplicate | exact current token/attempt/offer/epoch/identity | insert-if-null-or-equal receipt；finalize attempt并清 claim；state 非吸收且仍为 current offer时进入 `observing`，设置 `observing_since/next_probe_at`；不同值拒绝 |
| late accepted，当前无新 offer claim | old attempt 已被 consumer-first/timeout finalize，state 是 `ready` 或 `observing` 且仍为同一 current offer，`current_offer_attempt_id/offer_claim_token` 均为空 | 追加 old-attempt observation；insert-if-null-or-equal receipt并进入/保持 observing；若 consumer-first 已完成则不回退 state |
| late accepted，与新 claim B 竞争 | old attempt A 已 finalize，但同一 offer 已有另一个 current attempt/claim B | A 只追加 observation，不绑定 state receipt、不 finalize/clear B；由 B result、consumer-first或 B lease reaper继续收敛 |
| offer terminal without settlement | exact current/historical offer 返回 `handler_unsettled`、`killed`、`invalid_envelope` | 先 insert-if-null exact receipt；若同 offer 已有新 claim B，terminal tombstone 证明 B 不会再 accepted，故 finalize B=`terminal_observed_via_peer`并清 claim；current nonterminal 再按 owner 三态：none 调用 `advance_offer(transport_terminal_without_settlement,exact_code,...)`，fresh 保持 observing并设 next probe，expired 先 reap再推进；historical/absorbing state只结算 observation |
| offer terminal ACK-settled | exact current/historical offer 返回 `ack_settled` | 先只读查找已存在的 exact receipt+durable inbound settlement；缺证明时整个 DB transaction 零写入回滚，不能插 receipt、清 claim B或改变 timer，并返回 invariant fatal。证明存在后，才可 finalize同 offer新 claim B=`terminal_observed_via_peer`并清 claim，再按 settlement、target/owner/gate状态收敛；historical/absorbing 时只结算 observation，禁止盲目推进 |
| offer retired | exact offer 返回 retired tombstone | historical/absorbing offer 按 stale attempt结算；若仍是 current nonterminal offer则 finalize/release exact claim、保留同 offer并触发 invariant fatal，不能当 unavailable 重试或自动推进 |
| offer unavailable/timeout | exact current token/offer/epoch | finalize attempt并清 offer claim；已有 exact receipt则保持 `observing`，否则回 `ready`并按 frozen backoff 设置 due |
| offer lease expired | exact old token 且 DB time 已过 lease | finalize attempt并清 offer claim；已有 exact receipt则保持 `observing`，否则回 `ready` |
| 其它 late publisher result | exact attempt 已 finalize | 同 identity 只追加 immutable observation，不改 attempt/state；若 historical/absorbing offer 被证明重新 accepted/present，则同时 reopen exact retirement obligation；不同 fingerprint fatal |
| adapter/registry mismatch | exact current offer claim | finalize/release attempt；state=`ready`，保持 offer；`run` global fatal/not-ready |
| transport identity conflict | offer/probe/consumer 返回 exact transport ID 的不同 fingerprint，或 adapter 明确返回 `identity_conflict` | 不修改/poison typed target；offering 时 finalize/release claim 并保持同 offer ready，observing 时保持 exact state/receipt；`run` global fatal/not-ready |
| payload rejected/contract poison | exact current offer claim且 immutable item 自身损坏 | 调用 `settle_dispatch_absorbing(poisoned,...)`，写 typed failed evidence与 retirement |
| claim probe | state=`observing`、`next_probe_at<=now`、无有效 probe claim、exact active receipt | 插入 probe attempt；写独立 probe token/lease/current attempt |
| probe queued + owner none | exact probe token/lease、active receipt | finalize probe；保持 observing并设置 bounded `next_probe_at`；报告 lane stall/readiness |
| probe processing + owner none | 同上；Redis `phase_age_ms` 与 DB `observing_since` 分别受检 | finalize probe；未到 consumer/max age 时延后，达到上限时调用 `advance_offer(processing_stalled,current_epoch,prepared_successor)` |
| probe present + owner fresh | exact probe claim | finalize probe；保持 observing，由 business lease结算 |
| probe present + owner expired | exact probe claim；锁定 business attempt | finalize probe；reap attempt，再调用 `advance_offer(owner_reaped,current_epoch,prepared_successor)` |
| probe unavailable | exact probe claim | finalize/release probe，按 backoff 设置 `next_probe_at`；保持 observing |
| probe absent + owner none | exact probe claim；target pending、owner none | finalize probe；调用 `advance_offer(receipt_absent,current_epoch,prepared_successor)` |
| probe absent + owner fresh | exact probe claim | finalize probe；保持 observing |
| probe absent + owner expired | exact probe claim；锁定 business attempt | finalize probe；reap attempt，再调用 `advance_offer(owner_reaped,current_epoch,prepared_successor)` |
| probe terminal without settlement + owner none | exact probe claim；code 是 `handler_unsettled`、`killed`、`invalid_envelope` 之一 | finalize probe；调用 `advance_offer(transport_terminal_without_settlement,exact_code,current_epoch,prepared_successor)` |
| probe terminal without settlement + owner fresh | exact probe claim | finalize probe；保持 observing并设置 next probe，等待 business lease |
| probe terminal without settlement + owner expired | exact probe claim；锁定 business attempt | finalize probe；reap attempt，再调用 `advance_offer(owner_reaped,current_epoch,prepared_successor)` |
| probe terminal ACK-settled | exact probe claim；code=`ack_settled` | 先只读要求 exact durable inbound settlement；缺证明则整个 transaction 回滚、probe claim/timer保持原值并 invariant fatal。证明存在后才 finalize probe，再按该 settlement与 target/owner/gate状态收敛并设置 next action |
| probe retired | exact current offer | finalize probe；runtime invariant fatal，不当作 absent/terminal |
| probe lease expired | exact probe token 且 DB time 已过 lease | finalize exact probe attempt，清 probe claim，保持 offer/owner并设置 bounded `next_probe_at` |
| consumer before publisher settle | exact context；state 为 current `ready/offering/observing`；offer/epoch/identity 同值 | insert-if-null-or-equal receipt；若有 current enqueue attempt则 finalize=`accepted_consumer_first`，清 offer claim；state=`observing`并设置 receipt/observing_since/next_probe_at，再取得 business claim |
| consumer begin | exact current offer/epoch/identity，target pending、owner none、全部 fence 成立 | ready/offering/observing 三分支都原子进入/保持 `observing`、绑定 receipt/probe due；target=`running` |
| duplicate delivery | common receipt CAS 已令 exact current offer observing，但同 target 已有 fresh owner | 写幂等 inbound `noop_duplicate` receipt；receipt CAS 之后不再改变 target/dispatch，然后返回 `ack_settled` |
| consumer sees owner expired | common receipt CAS 已完成，且存在 expired business attempt | 写 typed `noop_owner_expired` settlement+inbound，设置 `repair_requested_at=now`并 NOTIFY hint，然后 ACK；不得偷取旧 attempt |
| current gate stale | common receipt CAS 已完成，但 maintenance gate/epoch 已失效 | 写 `noop_gate_stale` settlement+inbound，设置 `repair_requested_at=now`并 ACK；由 gate rebase 推进 successor，不执行外部依赖 |
| historical/absorbing delivery | exact immutable old offer/receipt，但已非 current 或 dispatch 已吸收 | 写或复用 `noop_historical_offer`、`noop_target_stale`、`noop_target_terminal` settlement+inbound，只读验证其 advance/absorbing predecessor；若 retirement 已 settled则 reopen pending，不改变 current dispatch/business state |
| business retry | exact fresh business claim，gate/fence 仍成立 | 旧 attempt 终结；调用 `advance_offer(...,prepared_successor)`并取得 settlement ID；写 inbound sidecar 后返回 `ack_settled` |
| business success/deterministic failure/exhausted | exact fresh business claim，gate/fence 仍成立 | target 结果与 `settle_dispatch_absorbing(terminal,...)` 同事务；清 claims、写 evidence/inbound/retirement 后才 ACK |
| target cancelled/superseded | domain mutation 持有 exact target fence | 终结 active attempt并调用 `settle_dispatch_absorbing(superseded,...)`；successor 使用新 target ID |
| epoch rebase | nonterminal state epoch != current open epoch | 按 §4.5 处理 owner；必要时 reap 后调用 `advance_offer(gate_rebase,current_epoch,prepared_successor)` |

immutable offer 必须在 Redis I/O 前持久化 ID/fingerprint。consumer 可从 current `ready|offering|observing` begin；consumer-first finalize current enqueue attempt并清 claim，晚到 publisher 对同 receipt只追加 observation并幂等成功。timeout/reaper 先检查 exact receipt：存在则保持 observing，不存在才回 ready。不同 fingerprint 始终 fatal；已推进 offer、不同 epoch或已吸收只能走 exact durable noop，不能 execute，也不能伪装 transport mismatch。

### 4.3 Consumer begin

consumer 在执行任何外部工作前先归一实际 context：无法解析 schema/dispatch ID 或数据库中不存在的 dispatch 由平台写 `invalid_envelope` terminal audit，不伪造 Bid settlement；已知 dispatch 但无法对应 exact immutable offer 的 lane/task/offer context 才进入带 dispatch FK 的 bounded rejected-delivery 路径，其 noop settlement 锁定并引用 authoritative current offer，actual context 只在 rejected row；同 transport ID 不同 fingerprint 始终是 fatal `identity_conflict`。能对应 historical/absorbing offer 的 exact receipt 进入 durable noop。exact current offer 先执行统一 receipt CAS：insert-if-null receipt、finalize/clear可选 publisher attempt/claim、进入 observing并设置 `observing_since/next_probe_at`；随后在同一事务继续验证：

- dispatch status=`ready|offering|observing`、exact current offer/lane、handler ID/fingerprint 与 immutable offer 同值；receipt 为 null 时 insert exact identity，为同值时幂等复用，任何不同 fingerprint 均返回 `identity_conflict`；
- maintenance gate=`open` 且 state/attempt gate epoch=current；
- target 仍是同 project 和 exact typed generation/watermark；
- target fence hash 和 immutable snapshot relation 未变；
- target 未 terminal/superseded/cancelled；
- business owner 分类为 `none|fresh|expired`；只有 none 可 execute，fresh/expired 走下述 durable noop。

结果只能是：

```text
execute(exact business claim)
noop_duplicate
noop_owner_expired
noop_gate_stale
noop_target_stale
noop_terminal
noop_historical_offer
noop_transport_mismatch
fatal_transport_identity_conflict
poison_contract
```

`noop_owner_expired` 写 exact expired owner-observation evidence、请求 repair并 ACK，不能偷取旧 owner。`noop_gate_stale` 不授权旧 epoch并请求 rebase。current nonterminal target 已 terminal/stale 时先通过 absorbing primitive 收敛；dispatch 已吸收或 offer 已 historical 时，`noop_target_stale|noop_terminal` 只写引用既有 advance/absorbing predecessor 的 nonmutating settlement。`noop_transport_mismatch` 写 rejected-delivery+settlement，不把合法 target 置 failed。所有 noop 先写 settlement/evidence/inbound sidecar；DB 失败返回 `unsettled`。exact ID 对应不同 fingerprint 返回平台独立 `identity_conflict`，不能映射成 `adapter_mismatch`、业务 noop 或 poison。`poison_contract` 只用于结构关系仍成立但 immutable target fence、canonical payload 或 prepared delivery 的 stored digest 与受检重算值不等，并通过 `KBCP` evidence + absorbing primitive 提交；FK/typed relation/catalog/adapter 错误必须零写入并 fail closed。除 execute 外不得调用业务外部依赖。

### 4.4 Business execution 与恢复

业务 execution lease 仍由所属 target 保存；它与短 offer lease 不可合并：

- worker 按冻结 lease 周期 heartbeat；
- publish/fail/cancel 必须验证 target、typed generation/watermark、attempt、claim token、owner gate epoch 和 lease 未过期；
- 确定性失败或 retry budget 耗尽写 target failed + dispatch terminal；
- retryable failure 精确终结旧 attempt，把 target 恢复为 pending，在同一事务调用 `advance_offer(...,prepared_successor)` 并写 settlement + inbound sidecar；提交后才返回 `ack_settled`；
- 进程仍活但单个 task heartbeat 过期时，target-local repair 精确 reap 旧 owner，再推进新 offer；
- 旧 worker 恢复后不能 heartbeat、publish 或改变 dispatch state。

business owner 分类是 typed adapter 的单一受检判断：`none` 表示 target pending 且没有 running attempt；`fresh` 表示 exact running attempt 的 claim token、lease、typed fence 和 gate epoch 均为 current；`expired` 表示存在 running attempt但 lease 已过期、epoch 已失效或 exact fence 已失效。`expired` 不能等同 `none`，必须先在 target-local repair 中把 exact attempt 终结为 `reaped`。consumer begin 不隐式偷取 running attempt。

target repair adapter 与各业务 module 同目录，中央 dispatcher 只调 typed registry。禁止恢复时读取“当前 snapshot”替代 target 已冻结 snapshot。

### 4.5 Gate、dispatch semantics 与 runtime governor

- gate 非 `open` 时不 claim 新 offer；在途 worker 下一次 heartbeat/publish 因 epoch 不同而失效；
- `offer_gate_epoch` 绑定整个 offer identity，不是单次 Redis 调用的可替换标签；同一 offer 的 enqueue retries 必须使用相同 epoch；
- gate 以新 epoch 恢复 open 后，dispatcher 对旧 epoch nonterminal state 做 lazy bounded rebase：owner=`none` 时调用 `advance_offer(...,prepared_successor)`；owner=`expired` 时同事务精确 reap 后调用同一原语；owner=`fresh` 时保持等待；target terminal/superseded 时结算 dispatch；
- rebase 同时写 current epoch 和新 offer identity，所有旧 epoch envelope 因 offer/epoch mismatch 永久 stale noop；旧 publisher 的 accepted settlement 也因 token/lease/epoch CAS 失败；
- intent 冻结的 dispatch semantics snapshot 包含 enqueue deadline、offer lease、consumer start deadline、probe/retirement interval+backoff、max observing age、dispatch replay window、transport policy version 和 clock-skew budget；V1 固定 replay=7 days、offer lease<=10 minutes、skew budget=5 minutes，并验证平台 tombstone 30 days覆盖三者，且所有 interval 有界不 busy-loop；
- poll interval、batch size、global/per-kind concurrency 属于 live runtime governor，不冻结到各 intent；它对所有 policy generation 使用同一 shared cap，验证 `poll_interval_ms=250..5000`、`batch_size=1..128` 且 concurrency 为小的正数硬上限；attempt 记录实际 runtime governor generation 供审计；
- semantics promotion 只在 maintenance，不能改写已有 intent；successor offer 使用 intent 冻结的 semantics contract，但 effective concurrency 始终不得超过 current shared governor；
- promotion/readiness 必须证明所有 nonterminal intent 引用的 semantics contract、lane 与 typed adapter 仍在 registry closure；引用归零前不能删除旧 decoder/verifier/lane。该要求补充而不改写平台 [`WorkTransport`](../platform/queue-runtime.md) 的 `prepare/offer/probe/retire_exact` interface。

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
DISPATCH_ADAPTER_MISMATCH
```

`DISPATCH_ADAPTER_MISMATCH` 只对应 platform `prepare` 发现 binary/sealed registry/capability closure 不一致：业务 mutation 整体回滚，并使 runtime readiness fail closed；不能降级成单条 target poison或 `DISPATCH_CONTRACT_INVALID`。其它 stage error 同样整体回滚，不能保留缺 typed extension、intent 或 state 的 target。

### 6.2 Delivery outcome

- Redis unavailable/timeout：attempt `released`，若 exact receipt 已被 consumer 观察则保持 observing，否则按冻结 backoff 设置 `next_offer_at`；仅当 gate epoch 未变时重试同一 immutable offer identity；
- accepted/duplicate：记录 exact ReceiptView；consumer 已先 finalize attempt 时只追加同值 observation，state 不回退；
- offer/probe=`terminal(handler_unsettled|killed|invalid_envelope)`：同一 tombstoned offer不得重投；owner none 以 `transport_terminal_without_settlement` 和 exact reason 推进，fresh 等待 lease，expired reap 后推进；
- offer/probe=`terminal(ack_settled)`：必须存在匹配 exact receipt 的 durable inbound settlement；按该 settlement与 target/owner/gate 当前状态收敛，不能盲目推进 successor；缺失证明时 DB transaction 零写入回滚并 readiness fail closed；
- offer 返回 `retired`：该 publisher 已失去数据库资格，只 finalize 历史 attempt，不改变 current state；若 state 仍错误指向该 retired offer则 readiness fail closed；
- adapter/registry/task→queue mismatch：释放当前 offer，`run` 返回 global fatal/readiness failure，不改变 target terminal state；
- transport identity conflict：按 §4.2 保持业务 target 与合法 receipt，不把 fingerprint 冲突当 duplicate、absent 或业务 poison；`run` 返回 global fatal/readiness failure；
- 单条 identity/version/canonical payload rejected：同一事务把 dispatch 置 `poisoned`、target 置 typed failed，并告警阻断完成声明；
- publisher accepted 后 DB settle 失败：consumer 可独立观察 receipt；receipt 仍 active/absent 时允许同 epoch重试同 offer，若已 terminal则必须 successor，epoch 已变则 rebase；
- handler 返回 `unsettled`：Oxana 写 `handler_unsettled` terminal tombstone；不得把它当 absent、`ack_settled` 或同 offer retry；
- observing probe=`present(queued)` 且 owner=`none`：保持并报告 lane stall；`processing+none` 超过上限后调用 `advance_offer(...,prepared_successor)`；owner=`fresh` 保持，owner=`expired` 先 reap 再推进；
- observing probe=`absent`：owner=`none` 才调用 `advance_offer(receipt_absent,current_epoch,prepared_successor)`；owner=`fresh` 保持 observing；owner=`expired` 先精确 reap 再推进；probe unavailable 时保持 observing。current offer probe 返回 `retired` 是状态/retirement invariant failure，不能当普通 absent。

### 6.3 Business outcome

provider 内部 bounded retry、业务 target retry 与 Oxana transport retry 按 [`../platform/queue-runtime.md`](../platform/queue-runtime.md) 分层。Oxana handler `max_retries=0`、delivery `resurrect=true`；所有已 durable 结算的业务结果向队列返回成功，避免乘法重试。

## 7. 并发、性能与 retention

- due scan 分别使用 ready due、observing probe due、`offer_claim_token IS NOT NULL` expired、`probe_claim_token IS NOT NULL` expired、repair requested 和 retirement due partial index；reaper 不依赖 status字符串猜 claim；
- 每批固定上限，`FOR UPDATE SKIP LOCKED`，global/per-kind concurrency 使用 current shared runtime governor 硬限制，不能按多个 intent snapshot 分别累计；
- target repair 按 typed adapter round-robin，每 kind 每轮最多一个冻结 batch；不得把六类 target 重新拼成无界中央 UNION，也不得让单一 backlog 饿死其它 kind；
- 新 intent commit 后发 bounded NOTIFY hint，polling 周期只作漏通知兜底；
- observing 到期不直接执行任务；先以独立 probe lease 查询 exact receipt，再按 owner 三态推进；
- base/extension/domain target、intent/state/offers/receipts/attempt observations/rejected deliveries/settlements/evidence/inbound/retirements 是一个 aggregate；只有 target terminal、外部引用释放、每个 offer 已 terminal/retired且7-day replay window结束时才能整体删除；
- 非 active 历史 attempts 可更早分批清理，但 current attempts/receipt/terminal settlement、未结算 retirement、ACK 证明和重放窗口内记录不得删除；
- 不创建 `system:live-recovery:v1` queue hop，不使用全局业务 housekeep，也不扫描 Oxana 私有 key。

跨表事务使用一个全局锁序：

```text
domain target/base/extension
  -> exact business attempt
  -> dispatch state
  -> dispatch offer/attempt/receipt/settlement/evidence/retirement
```

- consumer 可以先无锁读取 intent 找到 target ID，但加锁后必须按该顺序重新验证全部 fence；
- offer claim/settle 只锁 dispatch state→dispatch attempt，不得持有 dispatch lock 后再获取 target lock；
- probe/Redis/provider/object store/DocReader/renderer 调用期间不持 PostgreSQL row lock；probe settlement 重新按 target→dispatch state 顺序 CAS exact offer/receipt；
- retirement 独立按 retirement row→retire attempt 锁定，绝不反向获取 target/state；late transport result 只追加 observation；
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
- 业务 housekeep/reaper sweep；对 Oxana `oxanus:*` 私有 key 的 `replay_orphaned_local_jobs` 及启动调用必须在 PR8A 删除，PR8F 只复证 denylist；
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

1. PR8A 先在受控 Oxana durable public path/平台 `WorkTransport` 落位并验收 pure `prepare`、exact ID、atomic offer、terminal/probe/retire、tombstone、boot identity 与 resurrection；同一改动删除私有-key replay及启动调用并回归 legacy enqueue；
2. PR8B 在 fresh baseline 建立 dormant async target identity、空 conversion domain target 表、dispatch intent/offer/state/attempt/receipt/settlement/retirement、semantics snapshot、runtime governor、受检函数和 ACL，并实现深 module/PostgreSQL store；只用 synthetic aggregate 验证 publisher/consumer CAS，不激活 conversion producer/current pointer，也不切换任何业务 owner；
3. PR8C 安装 conversion/extraction reverse verifier、切换真实 owner并删除其旧 enqueue/recovery；
4. PR8D 安装 attachment preparation/render reverse verifier、切换真实 owner并删除其旧 enqueue/recovery；
5. PR8E 安装 matching schedule/job reverse verifier、切换真实 owner，落位 0..N fanout并删除 dirty-manifest/orphan-match recovery；
6. PR8F 复证私有 Redis replay denylist，重生成 baseline checksum/catalog/queue closure并完成全量强制活库；PR9 再完成 fresh runtime 验收。

任何阶段不得让同一 target 同时受旧 live-recovery 和新 dispatcher 驱动。由于最终部署是 fresh redeploy，不创建历史 payload converter、数据 backfill、双写或兼容 view。

## 11. 验收矩阵

### 11.1 原子性与 fanout

- base target+typed extension+intent+offer 0+ready state 同事务 commit/rollback；PR8B dormant catalog 不双写旧 target，PR8C～PR8E 切换后 verifier 拒绝孤儿或单删；
- state/attempt/receipt/settlement/successor/retirement 全部 composite FK 可建立；`SET CONSTRAINTS ALL IMMEDIATE` 拒绝单删 intent/offer、伪造 historical attempt、pointer 跨 offer、错误 attempt kind/successor/trigger/predecessor、第二条 absorbing settlement以及缺失/多余 typed evidence；
- family reverse verifier 在对应 cutover 前允许既有 domain target 无 extension，cutover 后拒绝 domain identity move/swap、零/多 extension与 project/kind mismatch；
- 同 identity 同 fence 幂等，不同 fence 冲突；
- 六类 `TargetFenceV1` Rust/SQL canonical golden 与任一字段篡改负例；
- `SettlementKeyV1/KBDS` Rust/SQL golden 覆盖 outcome matrix 的全部合法 optional shape，字段顺序/null/receipt/rejected/predecessor/evidence 任一篡改均拒绝；
- `SettlementEvidenceV1/KBEV`、`ContractPoisonEvidenceV1/KBCP`、`RejectedDeliveryV1/KBRJ`、`AttemptObservationV1/KBAO` 分别使用固定 expected bytes/hash 的 Rust/SQL golden；每个参与字段、variant/optional presence tag、枚举、长度与顺序的单字段篡改都必须被 verifier 拒绝；running owner 在 noop 后 success/reap 也不能改变已冻结 owner-observation bytes/hash；
- 两个并发事务对同一 `settlement_key` 执行 insert-or-read 时只能得到同一 settlement ID、同一 evidence 与同一 inbound sidecar；不同 canonical bytes 不能借 hash/key 冲突覆盖既有语义；
- conversion completion 与 extraction base/typed target+intent/state 原子；
- matching schedule 创建 manifest、0..N jobs 与等量 intents/states 原子；零 route 成功 terminal；
- attachment/render base/typed target 创建与 intent/state 原子；
- terminal identity 不可重开；successor 使用新 target ID，typed generation/watermark 只进入 fence，不参与通用 dispatch identity。

### 11.2 Transport 故障

- commit 时 Redis down，API 成功且 target 在 Redis 恢复后被 offer；
- NOTIFY 丢失仍由 indexed polling 在两个 current runtime-governor poll interval 内投递；
- `prepare` 与 SQL golden 在 Redis I/O 前冻结 immutable offer；Oxana accepted、probe、retire 和 handler context 必须 exact equal；
- Redis accepted 后 consumer 先于 publisher settle、publisher timeout 后 consumer 从 ready 晚到、publisher lease 已过后 late-same-receipt settle 三种顺序均收敛且不回退 state；
- publisher 在 accepted 后、DB settle 前崩溃，consumer 未到达时由 expired-offering/同 epoch identity 收敛；
- success/failed/retry_scheduled/noop/poison 只有在 exact inbound receipt 提交后才返回 `ack_settled`；DB settlement 失败返回 `unsettled`，并发重复 ACK 幂等复用相同 settlement key；
- `terminal(ack_settled)` 必须找到 exact durable inbound settlement并按其语义收敛；缺证明的 offer/probe 路径必须比较事务前后快照，断言 receipt、active claim B、probe claim、attempt disposition、`observing_since/next_probe_at/repair_requested_at/next_offer_at` 全部不变且无 observation/settlement/sidecar 新行；`handler_unsettled|killed|invalid_envelope` 只能按 owner 三态推进；terminal 与 retired 的任意互换都 fail closed；
- probe claim/lease/expired reclaim 并发只结算一个 attempt；exact queued+none 不复制，processing+none 超时推进，present+fresh 不重复，present+expired 先 reap；
- ready/offering/observing 三态的 exact late consumer、owner expired noop→repair→reap→successor、current handler 在 ACK 前崩溃都在时限内收敛；
- 每个 offer 前进/cancel/supersede/任一 absorbing terminal 原子创建 retirement obligation；`pending→claiming→settled`、unavailable/expired reclaim、late result 均按 exact token/attempt 收敛；同一 Redis volume内 absent 也写 tombstone并阻止 late publisher，完整 volume 丢失后 historical accepted/consumer 会 reopen obligation、durable noop并再次 retire，始终不能恢复业务执行权；
- late attempt A 已 finalize而同 offer 新 claim B active 时，A 只写 observation，不绑定 receipt、不清理 B；B result/consumer/reaper 继续收敛；
- retire 与 handler finish/resurrection 两种线性化顺序最终均为 exact terminal/retired tombstone，current handler 已提交的业务结果不丢失且旧 offer 不复活；
- Redis volume 清空后 probe absent，使所有未 terminal intent 按 owner 三态恢复；
- Redis 与 PostgreSQL 分别施加正负 5 分钟时钟偏移，`phase_age_ms` 与 DB `observing_since` 仍不跨时钟相减、不提前抢占 fresh owner；
- duplicate、乱序、旧 offer、wrong lane 和未知 payload version 稳定处理；
- 相同 hostname/PID 重启不需要读取 `oxanus:*` 私有 key。

### 11.3 Lease 与 fencing

- worker 进程死亡、进程存活但单 task 卡死、DB 连接丢失；
- offering publisher crash、offer lease expiry 与 exact token reclaim；
- ready/offering/observing/terminal/superseded/poisoned 的 due、claim、attempt、receipt、`observing_since/next_probe_at/repair_requested_at` NULL matrix 在每个 transition 后成立；
- business retry、owner reap、receipt absent、processing stall 和 gate rebase 全部只通过 `advance_offer` 推进，并创建 successor offer/settlement/retirement、清空旧 current 指针；
- heartbeat 与 publish race、lease expiry 边界、owner=`none|fresh|expired` 的三态矩阵、旧 owner 恢复；
- typed generation/watermark/snapshot 改变后旧 delivery noop；gate close/open 之间同 offer 不得换绑新 epoch，rebase 后旧 envelope 永久 noop；
- ended/cancelled/terminal target 不复活；
- begin/publish/repair/supersede/probe settlement 并发遵守统一锁序，无未处理 deadlock；`40P01` 不消耗业务 retry；
- bounded batch、current global/per-kind governor、混合新旧 semantics snapshot 和 backlog 超过一批。

### 11.4 Retry 与安全

- provider retry、business retry、transport resurrection 不相乘；
- retryable、deterministic、poison、exhausted 的 exact terminal code；
- API/worker/maintenance/retention role allow-deny；
- retention 不能单删 aggregate；未 terminal/retired offer、未结算 retirement、terminal/ACK/audit/replay 引用均阻止清理；
- payload/日志 bounded 且不泄露内容或 secret；
- `rg`、catalog denylist 和四条 task→queue registry closure 证明旧业务 wire DTO、recovery/housekeep/private-key replay 已删除。

### 11.5 Fresh runtime

空 PostgreSQL/Redis/object volumes 下走完 convert→extract→matching→attachment preparation→render；在每个 target 的 commit/offer/begin/publish 故障点注入一次中断并证明最终收敛。测试容器结束后立即删除。

只有 implemented、locally verified、committed、pushed、deployed、runtime accepted 六层分别有实际证据，且 `phase_1d_runtime_complete=true` 的受审计 cutover 完成后，才能声明本方案完成；方案文档批准或本地测试不能提前提升状态。
