# KnowledgeBrain Agent 稳定性与准确性完整解决方案

| 项 | 值 |
|---|---|
| 状态 | Round-5 最终收敛实施计划；首次上线前一次性 clean-slate cutover；新能力在维护门禁内默认关闭，首次开放采用已批准 intended feature state，且包含完整最终 Bid Matching |
| 依据 | 当前工作树、0001–0010 PostgreSQL schema、Oxana job/worker、API、Bid booklet、retrieval/OCR 源码契约 |
| 总目标 | 用 durable identity、immutable snapshot、route/generation fencing、可验证证据和应用级维护门禁，阻止过期任务、降级结果或错误 scope 被呈现为当前事实 |

## 0. 已定决策、非目标与术语

1. 不迁移到 WeKnora，不让开放 ReAct Agent 接管数据库写路径。Rust/PostgreSQL/Oxana 继续控制调度、claim、CAS、发布、评分和门禁。
2. full、document、Section retry 统一使用 `ExtractionPublicationStore`。candidate 隐藏；只有 fenced publisher 可写当前 `bid_sections`/`bid_clauses`。
3. `expected_conversion_generation` 隔离源内容变化；每文档单调 `extraction_generation` 隔离同一内容上的发布顺序。两者都必须 CAS。
4. Matching 是 route-discriminated workflow。Technical route 只处理一个 `unit_id`；Commercial route 只处理商业投影。不存在“technical + commercial 跨 route 原子替换”。
5. Matching evidence/result 是 append-only artifact；commercial current rows 和 technical current JSON 只是可重建 projection。
6. `business_value` 是唯一价值字段；删除新契约中的 `calibrated_value` 拼写。首次上线使用 0012 最终 matching workflow；若签署的 launch-mode artifact 选择兼容 score/ranking/serialization，它也必须由新 typed policy 实现，不得包装、调用或 dual-write 旧 matching 实现。
7. Bid 与 Grounded Answer 当前授权边界按 **authenticated-global** 描述。本文不虚构 workspace/project ACL，也不承诺 403。更强 knowledge authorization 是独立 domain migration。
8. publication fencing 与 production no-stub 是全局 cutover；preflight 前 default-off，不做 cohort。matching verifier 与 Grounded Answer 的生产 promotion 另受签署 eval gate 控制。
9. incoming `trace_context` 只是 untrusted correlation metadata。domain、operator、project、product、workspace 和 feature scope 全由服务端从认证主体与数据库资源派生；不得从 trace/span ID 授权。
10. **当前没有生产数据，也不存在需要保留的生产队列或 volume。首次上线只允许 clean-slate：Phase 0、1A、1B、2、1C、prelaunch 1D（含 schema-only 0011 与完整 0012 Bid Matching）全部完成并通过门禁后，才启动唯一最终 artifact/topology。Phase PR 可合并并跑 CI，但任何中间 binary 都不部署给用户或生产。**
11. 首次上线前，经双人显式确认，销毁development/staging PostgreSQL与Redis volumes；按version-controlled image lock的platform/runtime/build-base闭包启动exact digest镜像；对空库执行最终fresh chain（删除`migrations/0008_backfill.sql`，将实际Bid running-run constraint作为唯一、命名清晰且有checksum的`0008_bid_extract_running_constraints.sql`，再执行0010→0011→0012）；seed modern immutable snapshots与closed controls；在零key Redis上只注册canonical registry；在无外部流量时完成maintenance-safe preflight、审计开门，并只经隔离operator smoke socket运行mutation/claim/publish/matching smoke；关闭smoke endpoint后紧邻exposure重跑fresh API+worker readiness，单次消费evidence后才路由production traffic。
12. **one-shot** 指一个`cutover_id`的单次尝试：从final preflight evidence冻结开始，直到traffic exposure CAS与首条routed request evidence提交完成，exact image digests、migration head、registry release/intended state与rendered topology不变，没有intermediate deployment或user traffic。route尚未连接且exposure/revocation/request markers全为空时，才允许按cutover/launch state/gate epoch/intended hash与双签procedure进行open_unrouted→maintenance修复/reopen或destructive retry；routing tool在连接任何production route **之前**单次消费fresh readiness并原子写exposure、永久撤销reset authority。之后只能按独立评审的post-launch non-destructive policy演进，prelaunch recovery与destructive reset authority永久不得恢复。
13. 本文只规划实施；本次变更不修改 Rust、SQL、配置或测试。

术语：

- **artifact**：append-only、job/generation keyed 的不可变事实。
- **projection**：由 artifact 选择出的 current commercial rows 或 technical JSON，可由新 generation 替换。
- **partial publication**：target 已提交至少一个、但未提交其冻结 scope 中全部可发布 Sections，随后仍在执行、失败或被新 target supersede；完整发布的 target 为 `partial_publication=false`，已提交事实不回滚。
- **clean-slate**：首次上线前销毁 final topology 的非生产 PostgreSQL、Redis、MinIO/object及其他有状态 volumes，从空库执行完整 migration chain；不存在生产历史行、历史 payload/customer object 或数据版本兼容契约。
- **temporary prelaunch v0**：仅为让中间 commit 编译与 CI 通过而保留的 v0 seam；不得部署，且必须在首次上线前删除。它不是生产历史兼容能力。

---

## 1. 源码核对后的基线事实

| 领域 | 当前事实 | 计划修复 |
|---|---|---|
| Extraction coordination | `bid_extract_runs` 自身有 `claim_token`、`heartbeat_at`；0009 以 partial unique expression index 限制 running run。项目表的 `extract_lock_*` 不是 target lease | 迁移到 target run-row token/heartbeat + 每文档 active target partial unique index；temporary prelaunch v0 seam 只可用于中间 CI，首次上线前删除 |
| Extraction publication | full/document 会直接替换文档 draft；Section retry 也直接 upsert Section/Clause | hidden candidate + 单一 fenced publisher；失败保留旧稿 |
| Matching | `bid_match_jobs` 已区分 `job_kind=technical&#124;commercial`，technical 可带 `unit_id`；commercial 有单独 replace seam，但 current contract 仍混合状态字段 | route-discriminated request/report/store；每个 job 只能替换自己的 route projection |
| Dirty scheduling | confirmed Clause mutation 增加 `match_generation` 并置 `match_dirty`；存在单独 clear | 改为 matching-relevant mutation 在同一 project 锁/事务递增单调 `mutation_watermark`、置 dirty 并立即隐藏旧 projection；scheduler 原子固化同一 watermark 的 manifest/jobs 后才记录 `scheduled_watermark` 并 clear；route commit 对两个 watermark 做 CAS，且不 clear dirty |
| Booklet | Part 4 只取 confirmed commercial hit；Part 5 当前只取 confirmed commercial **must miss** | 保留 must qualifier；新增 confirmed commercial must review 的不同文案；non-must miss/review 留在 workflow，不能进入 Parts 4/5 |
| Grounded Answer | endpoint 需要认证；无 current version 在检索前 validation；PG fallback 使用 product current summary model | 按 authenticated-global 收敛，不添加 workspace/project ACL/403；保持 no-current/provider ordering 和 current model 语义 |
| Queue | Bid convert/extract/retry/match 当前注册在 `default`，身份契约不符合最终架构 | 首次上线只注册最终 v1 task type 与独立物理 queues；不迁移历史 envelope，也不允许多版本 worker 共存 |
| Startup | 当前树已有 API binary/domain exports，但 production profile、独立 readiness 和完整 Compose readiness contract 尚未形成 | 把 `crates/api/src/main.rs`、`crates/domain/src/lib.rs`、API readiness、Compose healthcheck、eval profile parsing 全纳入 Phase 2 |
| OCR | 已有部分 PostgreSQL successful-path coverage；knowledge no-VLM 行为也已有覆盖 | 补 no-fake-row、Bid image fixture 和 configured provider failure classification，不重复声称 PG success 全缺失 |
| Graph | PostgreSQL 与 in-memory 两条 path 都需要 scope 对齐；映射 chunk 的稳定完整顺序没有完整契约 | 两条 path 均校验 enabled/document/version/auth scope，并按稳定 ordinal/id 遍历合法 chunks |
| Migration | 当前工作树已加入 0010 ledger，但仍含 `0008_backfill.sql` 的 `schema_flags/0013_bid_backfill`、0009 历史起点识别、固定 checksum、含数据 fixture 与兼容 seed | 保留 advisory lock 与 immutable ledger；删除 `0008_backfill.sql`、`schema_flags` 与 `0013_bid_backfill` 的所有 source/fixture/seed 分支；把实际 running-run Bid constraint 重命名为 ledger version 0008 的 `0008_bid_extract_running_constraints.sql` 并锁定 name+checksum；首次上线只走显式 fresh allow-list 后执行 schema-only 0011 与完整 0012 |

Evaluation artifact 必须记录 `git rev-parse HEAD`、dirty state、Rust/toolchain、命令、provider/model/config snapshot ID、fixture hash、退出码和原始 eval report。本文不伪称已跑真实 provider evaluation。

---

## 2. Extraction Publication Protocol（0010，P0）

### 2.1 单一 Interface 与 CAS

```rust
trait ExtractionPublicationStore {
    async fn schedule_targets(ScheduleExtraction) -> Result<ScheduledTargets, PublicationError>;
    async fn claim_target(ClaimTarget) -> Result<Option<TargetLease>, PublicationError>;
    async fn heartbeat_target(HeartbeatTarget) -> Result<LeaseState, PublicationError>;
    async fn persist_section_candidate(PersistCandidate) -> Result<CandidateReceipt, PublicationError>;
    async fn publish_section(PublishCandidate) -> Result<PublishReceipt, PublicationError>;
    async fn finish_target(FinishTarget) -> Result<TargetReceipt, PublicationError>;
    async fn finish_run(FinishRun) -> Result<RunReceipt, PublicationError>;
}
```

每次 claim/persist/publish/cleanup/finish 都验证：

- `run_id + target_id + project_id + document_id + claim_token + attempt` 一致；
- target 状态允许、heartbeat 未过期、project open、maintenance gate 允许；
- `bid_documents.conversion_generation = expected_conversion_generation`；
- extraction head 的 current generation 等于 target generation；
- target、run、document 的组合 FK/constraint trigger 一致；
- 同文档不存在另一 running/publishing publisher。

当前 extraction 使用 **run/target row token + heartbeat + partial unique index**；不得将旧 `bid_projects.extract_lock_*` 描述成新协议 lease。generation 不匹配时把旧 target 终结为 stale/superseded，绝不改写成最新值。

### 2.2 调度与 partially published supersede

- Project-wide schedule 在事务内冻结 eligible completed document 集合；新文档不被同一 run 吸收。
- Document schedule 锁指定 document，验证 completed，递增 head generation，创建一个 target。
- Section retry 从 current published Section 解析 document/section scope，递增同一 document head，创建 `target_kind=section_retry`；它不直接写 domain row。
- schedule 锁 head，先使旧 pending/running/publishing target 失去 active ownership，再递增 generation、写新 target。旧 publisher 持行锁时 schedule 等待其 publication transaction 完成。
- **若旧 target 已发布部分 Sections，再被新 target supersede：旧 target 的最终 status 永远是 `superseded`，不是 `published`；其 durable `published_section_count > 0`、`partial_publication=true`、published candidate receipt 和 Section publication state 保留。** schedule 不撤销已提交 Section；在新 target 尚未发布同一 Section 前，该 Section 仍是 current、对普通 Section API 可见。新 target 后续发布同一 Section 时 projection 才前移，历史 artifact 仍保留。
- 普通 retry 复制来源 immutable snapshots；用户明确选择新配置时创建新 target/snapshots，不修改历史。
- DB target 是 durable enqueue intent；open 期间只有 final `system:live-recovery:v1` lane 可按 fenced recovery protocol 重投同一 `target_id`，不能重分配 generation；maintenance-only housekeeping 在 open 时机械 inactive。

必须实现以下精确数据库 barrier race（不得只 sleep）：

```text
T-old claim(target=A, generation=7)
T-old persist candidate(S1)
T-old publish(S1) COMMIT                         # published_section_count(A)=1
barrier: after Section domain/publication-state commit
T-new schedule section-retry/full target=B      # locks head, generation=8, COMMIT
T-old resume heartbeat/persist(S2)/finish
  -> generation/status CAS lost; no S2 writes; A.status=superseded
assert A.published_section_count=1
assert A.partial_publication=true
assert S1 remains visible/current until B publishes S1
assert run aggregate includes A/S1 despite A.status=superseded
```

还要覆盖相反 interleaving：schedule 在 old publish lock 前先提交，则 old Section commit 必须 CAS fail，count 为 0。

### 2.3 0010 可执行 schema

`migrations/0010_bid_extract_publication.sql` 是 additive migration，并同时建立 snapshot/control prerequisites：

```text
execution_config_snapshots
  id uuid PK                              # DB 生成 opaque ID，无内容 hash 语义
  kind text NOT NULL                     # extract|match|answer|provider_policy
  schema_version smallint NOT NULL
  payload jsonb NOT NULL                  # kind-discriminated typed、bounded object
  created_at timestamptz NOT NULL
  retain_until timestamptz NOT NULL
  CHECK payload byte size、known kind/version；禁止任意 open JSON

feature_snapshots
  id uuid PK                              # opaque ID
  kind text NOT NULL                     # enqueue|shadow|publication|answer|matching
  schema_version smallint NOT NULL
  payload jsonb NOT NULL                  # typed feature state、kill decision、promotion artifact ID
  created_at timestamptz NOT NULL
  retain_until timestamptz NOT NULL

application_maintenance_gate
  singleton_key boolean PK DEFAULT true
  mode text NOT NULL                      # open|draining|maintenance|rollback
  epoch bigint NOT NULL
  minimum_worker_protocol integer NOT NULL
  reason_code text NOT NULL               # bounded enum
  changed_by uuid NOT NULL
  changed_at timestamptz NOT NULL
  audit_ref uuid NOT NULL

feature_states / feature_promotions
  feature_key + environment current state
  default_off, kill_switch, approved_snapshot_id, updated_at
  append-only promotion audit: actor, from/to, signed_gate_artifact_id, reason, time
```

Snapshot payload 只能含版本、feature decision、typed numeric/enum knobs、opaque secret **reference** 和 provider/model identifiers；严格禁止 credential/token/header/URL userinfo、prompt/query/document/customer text、evidence quote、图片、customer-derived digest。Rust validator 先拒绝，DB kind/version/size CHECK 再拒绝。FK 使用 `ON DELETE RESTRICT`；0010 让 extraction target、`bid_match_jobs` 和 durable queue-enqueue manifest 引用现代 snapshot，v1 queue payload 携带对应 opaque IDs；活跃 target/job/queue manifest/artifact 引用时不得删除。`retain_until` 必须晚于：`queue_max_age + max_run_duration + run_retention + artifact_retention` 的总和；purger 只能删除无 FK 引用且超过该边界的 snapshot。空库只 seed 版本化 modern snapshots，worker cache miss 不得回默认配置。

```text
bid_document_extraction_heads
  document_id PK/FK
  current_extraction_generation bigint NOT NULL DEFAULT 0
  active_target_id uuid NULL
  updated_at timestamptz NOT NULL

bid_extract_run_targets
  id, run_id, project_id, document_id
  target_kind full|document|section_retry, section_key NULL, source_section_id NULL
  expected_conversion_generation, extraction_generation
  status pending|running|publishing|published|failed|stale|superseded|cancelled
  claim_token, heartbeat_at, attempt, max_attempts
  config_snapshot_id FK RESTRICT, feature_snapshot_id FK RESTRICT
  quality_status pass|review|block, degraded boolean, bounded reason_codes[]
  outline_complete, cleanup_completed
  scoped_section_count integer NULL                 # outline冻结后写定；section retry为1
  published_section_count integer NOT NULL DEFAULT 0
  partial_publication boolean NOT NULL DEFAULT false
  partial_failure boolean NOT NULL DEFAULT false
  worst_quality_status pass|review|block
  aggregate_degraded boolean NOT NULL DEFAULT false
  aggregate_reason_codes text[] NOT NULL          # bounded count/allow-list
  provider/fallback/policy/prompt/schema versions, safe first-failure metadata
  created_at, claimed_at, finished_at, updated_at
  UNIQUE(document_id, extraction_generation)
  UNIQUE(id, run_id, project_id, document_id)      # composite FK target

bid_extract_section_candidates
  id, target_id, run_id, project_id, document_id
  expected_conversion_generation, extraction_generation
  section_key, heading_path[], outline_ordinal
  status pending|running|succeeded|failed|published|stale|superseded|cancelled
  quality/degraded/bounded reasons/provider versions/typed diagnostics
  idempotency_key, created_at, finished_at, published_at
  UNIQUE(target_id, section_key), UNIQUE(idempotency_key)
  UNIQUE(id, target_id)                            # span composite FK

bid_extract_span_candidates
  id, section_candidate_id, target_id, span_key, outline_ordinal
  source_span, disposition clause|non_requirement|unresolved
  disposition_reason bounded enum NULL, status
  UNIQUE(section_candidate_id, span_key)
  UNIQUE(id, section_candidate_id)

bid_extract_clause_candidates
  id, section_candidate_id, span_candidate_id
  raw_text, text, family, must, source_span, typed extraction_meta, quality, created_at

bid_section_publication_state
  document_id, section_key
  current_run_id, current_target_id, current_section_id
  published_extraction_generation, stale, removed, quality/degraded/bounded reasons
  last_attempt_run_id, last_attempt_target_id, safe first-failure metadata, updated_at
  PRIMARY KEY(document_id, section_key)
```

Schema 规则必须以可执行 PostgreSQL DDL 表达：

```sql
CREATE UNIQUE INDEX bid_extract_target_scope_uidx
ON bid_extract_run_targets
  (run_id, document_id, target_kind, COALESCE(section_key, ''));

CREATE UNIQUE INDEX bid_extract_target_active_document_uidx
ON bid_extract_run_targets (document_id)
WHERE status IN ('running', 'publishing');
```

不得在 table `UNIQUE(...)` constraint 中放 `COALESCE`。若列类型/业务语义更适合 nullable equality，可用命名 `NULLS NOT DISTINCT` unique index，但不能写不可执行的 table constraint。

- section candidate 通过 composite FK `(target_id,run_id,project_id,document_id)` 引用 target 同列 unique key。
- target 通过 composite FK/deferrable constraint trigger 验证 run.project、document.project 一致；publication state trigger 验证 current target/run/document/section 一致。
- span 通过 composite FK 绑定 section candidate；Clause 通过 composite FK 绑定同一 section candidate/span。
- 因普通 FK 不能表达 predicate，增加 deferrable constraint trigger：Clause insert/update 时对应 Span 必须 `disposition='clause'`；non-clause Span 禁止 Clause；每 clause disposition 恰有合法 Clause 的 cardinality 在 candidate terminal trigger/transaction validator 检查。
- heading、body/quote、arrays、reason count、diagnostics bytes 和版本 ID 都有 DB/API 上限；unknown reason 映射 `OTHER_BOUNDED`，自由 provider error 不入库。

`bid_extract_runs` 也 additive 增加 `target_count`、`published_target_count`、`published_section_count`、`partial_publication`、`partial_failure`、`worst_quality_status`、`degraded`、bounded `reason_codes`。首次上线从空库应用约束和默认值，不转换历史 run/Section/Clause。

### 2.4 Publish、coverage 与聚合

1. persist 一个 Section 的全部 Span disposition 和 Clauses；先做 scope、连续 quote、typed schema、hard coverage，再 terminal。
2. publish 事务重新 CAS lease、attempt、双 generation、head、project/gate；upsert Section，只 supersede 同 Section 的旧 draft，不改 confirmed/rejected；写 publication state；candidate=`published`；原子递增 target `published_section_count`。在 target active 且尚未覆盖冻结 scope 全部可发布 Sections 时令 `partial_publication=true`；成功完整 finish 时改为 false，被 supersede/failed 时按 durable count 与冻结 scope保留 true。
3. uncovered/duplicate/invalid Span 是 `failed+block`，旧 domain row 不动。deterministic non-requirement 必须使用 versioned bounded rule；模型不能自行 disposition。
4. heuristic/family conflict 只有 coverage closed 才能 `succeeded+review`。零 Span 的有效 outline Section 可发布空 Clause；target 零 Section 为 `NO_EXTRACTABLE_SECTION` block，不执行删除。
5. full/document cleanup 只在 outline complete、所有 candidate terminal、lease/generation 仍有效时执行；Section retry 永不做全文删除。
6. heartbeat timeout 用 token+attempt CAS recovery；较新 generation 存在则旧 target superseded，不重跑。

**聚合不能从 target status 单独推导。** `finish_target/finish_run` 在同一锁顺序下从 `status='published'` 的 Section candidates、durable publish receipts 与 `bid_section_publication_state` 计算；target status 只表达 owner 生命周期。规则：

- target 有已发布 Section 后失败：可为 `failed`；有已发布 Section 后被替代：必须为 `superseded`；两者都保留 count 和 `partial_publication=true`。
- `partial_failure=true` 表示 scope 中存在 failed/block/cancelled candidate；`partial_publication=true` 表示 `0 < published_section_count < scoped_section_count`，或已有publication后在scope冻结/完成前被supersede/fail。完整成功target为false；两者可独立。run的partial flag由其冻结targets/Sections按同一规则聚合。
- run 只在无任何 published Section，或 run-global snapshot/scope/migration invariant 破坏时为 failed；否则 done，即使其 published candidate 所属 target 后来 superseded。
- worst quality、degraded、bounded reasons 从 published candidates + publication state 和失败 aggregate 计算；API 返回 target/run status 与 aggregate 字段，不能把 superseded+partial 隐藏成“无输出”。
- project-wide 零 target 为 `NO_ELIGIBLE_DOCUMENT` block；重复 queue/persist/publish/finish 返回同一 receipt。

---

## 3. MatchingWorkflow route contract（0012，首次上线 P0）

### 3.1 Route-discriminated Interface

```rust
enum MatchRoute {
    Technical { unit_id: Uuid },
    Commercial,
}

struct MatchingRequest {
    project_id: Uuid,
    job_id: Uuid,
    generation: i64,
    expected_mutation_watermark: i64,
    claim_token: Uuid,
    route: MatchRoute,
    requirement_artifact_ids: Vec<Uuid>,
    product_version_scope: Vec<Uuid>,
    config_snapshot_id: Uuid,
    feature_snapshot_id: Uuid,
    deadline: Instant,
}

struct MatchingReport {
    contract_version: MatchContractVersion,
    job_id: Uuid,
    generation: i64,
    route: MatchRoute,
    groups: Vec<CandidateGroup>,
    score: TypedScore,
    quality_status: QualityStatus,
    degraded: bool,
    reason_codes: Vec<ReasonCode>,
    empty_disposition: Option<EmptyDisposition>, // clear_route | skip_unit
}

trait MatchResultStore {
    async fn persist_route_generation_fenced(
        &self, request: PersistMatchRouteReport
    ) -> Result<PersistMatchReceipt, PersistMatchError>;
}
```

Persistence 在单事务按固定顺序锁 project/manifest/job，并先用一条 guarded claim/commit CAS 同时验证：`job_id + project_id + route(+technical unit_id) + claim_token + attempt + status=running`、`job.generation=manifest.generation=current project matching generation`、request 中 requirement artifact IDs/product-version scope/config/feature snapshot IDs 与 job/manifest 冻结值 exact equal、`project.mutation_watermark=job.expected_mutation_watermark`、`project.scheduled_watermark=job.expected_mutation_watermark`、`match_dirty=false`、project open 且 gate permits。只有该 CAS 命中后，才在同一事务 append candidate/report completion artifacts、替换该 route projection、把新 projection 标为 current 并 finish job；任何条件不符返回 affected-row count 0 的 stale receipt，**不写 artifact、projection 或 job 状态**。

- Commercial report 只能 replace/clear commercial projection；empty commercial 明确清空 commercial current rows，不触碰任何 technical unit。
- Technical report 只能 replace 指定 `unit_id` 的 technical current JSON；empty technical 必须显式 `clear_route` 清该 unit，或按 versioned policy `skip_unit` 只保留 prior artifact/history、让该 unit 继续 non-current/hidden 并记录 review reason；不得影响其他 unit 或 commercial。
- 不同 route 可独立成功/失败并各自变为 current；不作 Technical+Commercial 跨 route 原子替换声明。mutation 对所有旧 route 的统一失效是 project watermark fence，不是跨 route replacement。
- route result commit **绝不 clear `match_dirty`**、推进 `scheduled_watermark` 或改写 `expected_mutation_watermark`。

### 3.2 Atomic generation scheduling manifest

所有 matching-relevant mutation（confirmed requirement Clause 的创建/删除或 matching 输入字段/状态变化，以及 eligible product/version scope、content revision 或 enabled/current 状态变化）都必须走唯一 mutation seam。该 seam 在 domain mutation **同一事务**先锁受影响 project row（多 project 按 UUID 稳定顺序），对每个 project 执行 `mutation_watermark = mutation_watermark + 1`（单调且不得回退/复用）、`match_dirty=true`，并立即把该 project 当前 Technical/Commercial route projections 全部标为 `is_current=false`。domain mutation、watermark、dirty 与 projection invalidation 任一失败则全部回滚；不得有绕过该 seam 的 matching-relevant writer。

因此从 mutation commit 到对应 watermark 的 route 成功替换之间，API/UI/workflow/booklet 只可按 `is_current=true AND projection_mutation_watermark=project.mutation_watermark AND projection_generation=current matching generation AND match_dirty=false` 读取 current matching。旧 artifact/pick 继续可审计，但旧 projection 不得 fallback、不得进入 Part 4/5，也不得被描述为 current；每个 route 成功后仅该 route 恢复可见，尚未成功的 route 继续 hidden。`clear_route` 以 current empty receipt 表示成功空替换；`skip_unit` 不恢复旧 projection 可见性。这是 route-by-route visibility，不是跨 route 原子发布。

scheduler 也在同一 project lock/事务中：

1. 锁 project，确认 open/gate/snapshot prerequisites，且 `match_dirty=true`、`mutation_watermark > scheduled_watermark`；精确捕获当前 `mutation_watermark=W`；
2. 按工作流适用范围前移 generation：本节必须分配下一 project matching generation；若同一入口也调度 extraction，则按第2节 head/CAS 前移相应 extraction generation，并把该 generation 固化进 requirement snapshot，不得把旧 generation 改写成新值；
3. 写 immutable `bid_match_generation_manifests`，含 `mutation_watermark=W`、完整 requirement snapshot IDs、product-version scope/snapshot IDs、route set、config/feature snapshots；
4. 为每个 Technical unit 和 Commercial route 写 durable job，每行都写 `expected_mutation_watermark=W` 并 FK/引用同一 manifest 与冻结 snapshot IDs；
5. 只有 manifest 与完整 route job set 都成功后，才以 `WHERE mutation_watermark=W AND match_dirty=true` 在同一事务设置 `scheduled_watermark=W, match_dirty=false`；任何缺 route/job/snapshot 或 CAS 0 行都回滚 generation、manifest、jobs 和 dirty 更新；
6. commit 后 enqueue job IDs。mutation 无法穿过 project lock；若 scheduler 先提交，随后 mutation 必产生 `W+1`、重新置 dirty 并隐藏任何已发布或将发布的 W projection，因而 W job 在下一 scheduler pass 前已不能 commit。open 期间仅 final `system:live-recovery:v1` lane 可用同样 CAS 重建缺失 dirty manifest，maintenance-only housekeeping 不参与。

这使 dirty ownership 可机械测试：individual route commit 不能修改 dirty/watermark；scheduler 只能消费它实际冻结的 exact watermark。不存在“先 clear dirty、后补 jobs”的窗口。

确定性 race tests 必须用 project-row/transaction barrier，不用 sleep：

```text
schedule generation N at watermark W; claim a Technical or Commercial route
pause generation-N route after report is ready, immediately before guarded commit
matching-relevant mutation transaction locks project:
  mutate domain input; mutation_watermark=W+1; match_dirty=true
  mark every current Technical/Commercial projection is_current=false; COMMIT
resume generation-N commit before any scheduler N+1 pass
  -> watermark/dirty CAS affects 0 rows
  -> no report/candidate artifact, projection, current marker, or job-state write
  -> APIs/UI/booklet cannot read the old projection as current
schedule next manifest; assert it captures W+1 and every job.expected_mutation_watermark=W+1
attempt W and mismatched-snapshot jobs, then W+1 jobs
  -> only jobs whose expected watermark and all frozen snapshot IDs equal current project/manifest state can publish
```

还必须用可控 barrier 覆盖：(a) scheduler 持 project lock 时 mutation 等待；scheduler W commit 后 mutation 提交 W+1，W jobs 随即 stale/旧 projection hidden；(b) mutation 先持锁时 scheduler 等待并只能捕获新 watermark；(c) 同一 W 的 Technical 与 Commercial route 同时 commit 时各自只替换自己的 projection，无 lost update 且不宣称共同原子提交；(d) 一个 route 先 commit、随后 mutation、另一个旧 route 再 commit时，前者立即变 non-current，后者零写；(e) route commit 与 mutation 同时争锁时，commit 若先成功也被随后 mutation 隐藏，mutation 若先成功则旧 commit CAS 0。

### 3.3 Immutable artifacts 与 current projections

0012 添加/收敛 matching control 与 append-only 表：

```text
bid_projects matching control
  match_generation bigint NOT NULL                 # existing/current; only scheduler advances
  mutation_watermark bigint NOT NULL DEFAULT 0
  scheduled_watermark bigint NOT NULL DEFAULT 0
  match_dirty boolean NOT NULL DEFAULT false
  CHECK 0 <= scheduled_watermark AND scheduled_watermark <= mutation_watermark

bid_match_generation_manifests(project_id, generation, mutation_watermark,
  requirement_snapshot_ids, product_version_snapshot_ids, route_set,
  config_snapshot_id, feature_snapshot_id, created_at,
  PRIMARY KEY(project_id, generation), UNIQUE(project_id, mutation_watermark))

bid_match_jobs additions
  manifest project/generation FK, expected_mutation_watermark bigint NOT NULL,
  frozen requirement/product/config/feature snapshot identities

bid_match_requirement_artifacts(id, project_id, generation, job_id, route, unit_id,
  clause_id, immutable typed requirement snapshot, created_at)
bid_match_candidate_artifacts(id, requirement_artifact_id, project_id, generation, job_id,
  route, unit_id, candidate_identity, product/version metadata, evidence, support,
  decision, quality, business_value, grouping metadata, versions, created_at)
bid_match_report_artifacts(id, project_id, generation, job_id, route, unit_id,
  mutation_watermark, typed score/coverage/quality/degraded/reasons, created_at)

commercial/technical current projections
  project_id, route discriminator (+ technical unit_id), projection_generation,
  projection_mutation_watermark, is_current boolean, selected report/candidate artifact FKs
```

`expected_mutation_watermark` 必须等于引用 manifest 的 `mutation_watermark`，用 composite FK/constraint 表达，不能由 worker payload 自报后覆盖。projection 的 current uniqueness 按 route 区分：Commercial 每 project 至多一个 current head，Technical 每 `(project_id,unit_id)` 至多一个 current head；mutation seam 可在 project lock 下统一失效这些 heads，但 publisher 仍只替换自己 route。

Artifacts 禁止 UPDATE/DELETE（retention purge 仅在无 projection/pick/audit FK 且到期后由受审计 procedure 执行）。Commercial current rows 与 technical current JSON 明确命名为 projection，并 FK 指向所选 report/candidate artifacts。所有 current reads 还必须 join project matching control 执行第3.2节 watermark/generation/dirty visibility predicate，不能只信 projection 自身的 `is_current`。

`bid_picks.clauses` 不再是会被后续 matching 改写的 evidence。最终契约中的 pick 在创建时保存 **immutable typed contract snapshot**，包括 requirement、candidate、evidence/support/decision/quality、route/job/generation/schema/version；后续 current projection 变化不得改写它。首次上线数据库为空，不创建历史语义标记或推断转换。

每个 typed candidate/report 必须包含：

- `candidate_id`（opaque artifact identity）、`product_id`、`product_version_id`、version/content revision；
- retrieval rank/raw score、verifier support、system decision、quality；
- `score` typed scored/not_scored、requirement coverage `{eligible,total,supported,contradicted,insufficient,unresolved}`；
- deterministic `group_key`、group rank/size/dedup reason；
- route `{kind:"technical",unit_id}` 或 `{kind:"commercial"}`、job ID、generation；
- retrieval/verifier/score/config/feature schema versions。

Candidate grouping contract：先按 route 隔离；Technical 再按 `unit_id + product_version_id`，Commercial 按 `product_version_id`；同 requirement/product-version 的 evidence 以 `(document_id,version_id,chunk_id,quote offsets)` 去重；组内按 requirement artifact ID、retrieval rank、chunk ordinal、chunk ID 稳定排序；组间按 typed score 降序、coverage 降序、product/version UUID 升序。不同 product versions 永不合组。

### 3.4 唯一 value、rounding 与 score

新契约只允许：

```json
{"business_value":{"status":"scored","value":"0.750000","source":"policy|verifier|operator"}}
{"business_value":{"status":"not_scored","reason":"NO_WEIGHT|NO_EVIDENCE","source":"policy"}}
```

- `value` 是 `[0,1]` fixed decimal；DB `numeric` CHECK，Rust typed decimal，不用 binary float 聚合。
- internal multiplication/sum 保持高精度；只在 report serialization 边界使用 round-half-even 到 6 位，canonical JSON 固定 6 位字符串。最终 0–100 score 同样 6 位字符串。
- 不同时出现 `business_value` 与 `calibrated_value`；unknown source/reason 拒绝。
- `TypedScore` 是 `scored{value,version,denominator,unresolved_count}` 或 `not_scored{reason,version}`；空 denominator 不是 0。
- score-v2 分母是全部有权重 requirement；仅 `supported+hit` 贡献，clean must 还必须 `quality=pass`。must review/miss/insufficient/contradicted 都 unresolved。
- 0012 最终实现内同时支持签署的 first-launch score policy 与可选 score-v2 shadow。若 launch mode 选择 v1-compatible policy，`bid_picks.score`、排序、rounding 和 API serialization 由新 typed artifact/projection 路径生成；只有另一个签署 cutover artifact 后版本化 API/UI 才切 score-v2。不得保留旧 matcher wrapper、旧 store、旧 queue handler、dual-write 或 fallback。

### 3.5 Booklet Part 5 与人工 workflow

- Part 4：仅 confirmed commercial `hit`，维持现行语义。
- Part 5：仅 confirmed commercial **must** `miss` 或 `review`。miss 文案为“必须商务材料缺失”；review 文案为“必须商务材料待复核（尚不能认定缺失或满足）”，两者不可混写。
- confirmed commercial non-must miss/review 仍在 matching/workflow API/UI 可见，但在 Parts 4、5 之外；draft/rejected commercial 也不进入。
- 不得借本计划把 non-must 加入 Part 5；那需要单独批准的产品变更。
- human `meet|partial|deviate|fail` 不改 immutable verifier evidence/support。现行 export/disclosure 行为保留；只有 clean 声明使用 `supported+hit+pass` must gate。

---

## 4. Runtime、snapshot、queue 与 retry safeguards（Phase 2，1C 前置）

### 4.1 Mandatory feature/snapshot prerequisites

在任何 v1 enqueue、shadow write 或 publication 前，必须已有：

- PostgreSQL basic feature-state store；
- immutable execution config + feature snapshots；
- audited operator promotion 与 signed gate artifact reference；
- server-enforced kill switch；
- application maintenance gate、resolved capability 和 readiness pass。

缺任何一项均 fail closed，不创建 v1 queue envelope。自动 cohort registry 和 promotion UI 是 optional；上述基础 store/snapshot/audit/kill switch 不是 optional。

### 4.2 Canonical complete queue registry 与 final identity

唯一 authority 是 version-controlled `deploy/queue-registry.toml`；registry 有 schema version、release ID、exact `minimum_worker_protocol`，且每一 entry 都含：physical queue、task type、payload schema/version、unique identity formula、worker protocol minimum、owning handler、required snapshots、resolved capabilities 与 `launch_mode`。首次上线 protocol minimum 为 literal integer `1`；`launch_mode` 只允许：`required_enabled`（签署 intended state 必须启用且 readiness 必须验证依赖）、`declared_disabled`（本 registry release 明确不启用；enqueue/claim 均 fail closed，readiness 验证其无 active subscription/Redis registration）和 `maintenance_only`（只允许受审计 maintenance procedure/epoch 调度与 claim）。Graph、multimodal、question 等非 Bid 产品 lane 可在首次 registry 中为 `declared_disabled`，不会成为首次开放 blocker；要启用必须以独立评审的 registry release + signed intended feature state 改为 `required_enabled`，不能运行时猜测或 fallback。

签署的 intended feature state 按 registry entry identity 逐项记录 `enabled|disabled|maintenance_only`，必须与 `launch_mode` 相容；它选择本次真正启用的 product lanes，而 final Bid conversion/extraction/matching 必须为 enabled。readiness 展开所有 enabled lane 的 snapshot/capability，同时验证 disabled lane 不可 enqueue/claim。首次上线 registry 必须逐项等于下表；表中 payload 全是 bounded tagged DTO，`v1` 表示 payload 内也必须有 `payload_version=1`：

| physical queue | task type | payload schema；unique identity formula | protocol | owning handler | required snapshots / capabilities | launch mode |
|---|---|---|---:|---|---|---|
| `default` | `document:process` | `document-process/v1`；`document:process:{document_id}:{attempt}` | 1 | `DocumentProcessV1Handler` | process snapshot；PostgreSQL、object store、docreader | `required_enabled` |
| `default` | `manual:process` | `manual-process/v1`；`manual:process:{document_id}:{attempt}` | 1 | `ManualProcessV1Handler` | process snapshot；PostgreSQL、object store、docreader | `required_enabled` |
| `postprocess` | `knowledge:post_process` | `post-process/v1`；`knowledge:post_process:{document_id}` | 1 | `PostProcessV1Handler` | process + feature snapshots；PostgreSQL、embedding/index | `required_enabled` |
| `summary` | `summary:generation` | `summary/v1`；`summary:generation:{document_id}:{attempt}` | 1 | `SummaryV1Handler` | generation + feature snapshots；chat | `required_enabled` |
| `summary` | `datatable:summary` | `datatable/v1`；`datatable:summary:{document_id}` | 1 | `DatatableV1Handler` | generation + feature snapshots；chat | `required_enabled` |
| `multimodal` | `image:multimodal` | `image-multimodal/v1`；`image:multimodal:{document_id}:{image_key}:{attempt}` | 1 | `ImageMultimodalV1Handler` | multimodal + feature snapshots；VLM/OCR、object store | `declared_disabled` |
| `graph` | `chunk:extract` | `chunk-extract/v1`；`chunk:extract:{chunk_id}` | 1 | `ChunkExtractV1Handler` | graph + feature snapshots；embedding、graph store | `declared_disabled` |
| `question` | `question:generation` | `question-generation/v1`；`question:generation:{document_id}:{batch}` | 1 | `QuestionV1Handler` | generation + feature snapshots；chat | `declared_disabled` |
| `wiki` | `wiki:ingest` | `wiki-ingest/v1`；`wiki:ingest:{product_version_id}` | 1 | `WikiIngestV1Handler` | wiki + feature snapshots；PostgreSQL、embedding/index | `required_enabled` |
| `wiki` | `wiki:finalize` | `wiki-finalize/v1`；`wiki:finalize:{product_version_id}` | 1 | `WikiFinalizeV1Handler` | wiki + feature snapshots；chat、PostgreSQL | `required_enabled` |
| `low` | `version:clone` | `version-clone/v1`；`version:clone:{target_version_id}` | 1 | `VersionCloneV1Handler` | clone snapshot；PostgreSQL、object/index store | `required_enabled` |
| `low` | `kb:delete` | `kb-delete/v1`；`kb:delete:{product_version_id}` | 1 | `KbDeleteV1Handler` | operator feature snapshot；PostgreSQL、object/index store | `required_enabled` |
| `low` | `knowledge:list_delete` | `list-delete/v1`；`knowledge:list_delete:{document_id}` | 1 | `ListDeleteV1Handler` | operator feature snapshot；PostgreSQL、object/index store | `required_enabled` |
| `low` | `index:delete` | `index-delete/v1`；`index:delete:{document_id}` | 1 | `IndexDeleteV1Handler` | operator feature snapshot；index store | `required_enabled` |
| `low` | `knowledge:list_reparse` | `list-reparse/v1`；`knowledge:list_reparse:{document_id}` | 1 | `ListReparseV1Handler` | process + feature snapshots；PostgreSQL、object store、docreader | `required_enabled` |
| `low` | `system:maintenance-housekeep:v1` | `maintenance-housekeep/v1`；`system:maintenance-housekeep:v1:{activation_epoch}:{scheduled_epoch_5m}` | 1 | `MaintenanceHousekeepV1Handler` | maintenance activation + feature snapshots；PostgreSQL、Redis | `maintenance_only` |
| `low` | `system:live-recovery:v1` | `live-recovery/v1`；`system:live-recovery:v1:{recovery_kind}:{durable_id}:{generation}:{recovery_epoch}` | 1 | `LiveRecoveryV1Handler` | recovery policy + feature snapshots；PostgreSQL、Redis | `required_enabled` |
| `bid-conversion-v1` | `bid:convert:v1` | `bid-convert/v1`；`bid:convert:v1:{document_id}:{requested_conversion_generation}` | 1 | `BidConvertV1Handler` | conversion + feature snapshots；PostgreSQL、object store、docreader | `required_enabled` |
| `bid-extraction-v1` | `bid:extract-target:v1` | `bid-extract-target/v1`；`bid:extract-target:v1:{target_id}` | 1 | `BidExtractTargetV1Handler` | target config + feature snapshots；chat/extraction policy、PostgreSQL | `required_enabled` |
| `bid-matching-v1` | `bid:match-route:v1` | `bid-match-route/v1`；`bid:match-route:v1:{job_id}` | 1 | `BidMatchRouteV1Handler` | matching config + feature + score/verifier policy snapshots；retrieval、verifier/chat、PostgreSQL | `required_enabled` |

Section retry **没有独立 task 或 queue**：full/document/retry 都创建 target，唯一身份都是 `target_id`。旧 `bid:convert`、`bid:extract`、`bid:section-retry`、`bid:match` task registrations 全部删除；不得在 `default` 注册任何 Bid handler。`sync` queue、未映射 task、unknown-task→`default` fallback 与任何未列出的 queue 明确禁止。

**Queue closure 是全 seam 双向精确相等，不只是 worker↔registry。** Producer task constants/typed task→queue mappings、每个 enqueue call site 与生成的 enqueue manifest、canonical registry entries、handler registrations、rendered worker subscription manifest 以及 Redis actual registrations 必须以同一 `(physical_queue,task_type,payload_schema/version,identity_formula,handler,protocol,snapshots,capabilities,launch_mode)` set 做 bidirectional equality；任一 missing、extra、duplicate、fallback、unreachable constant、无 producer handler、无 handler producer、task→queue mismatch 或字段 mismatch 均拒绝。`declared_disabled` entry 仍出现在所有静态/declarative sets，rendered subscription 明确为 `active=false`，Redis active set 中必须不存在；`required_enabled` 的 active subset 必须 exact 出现，`maintenance_only` 只按受审计 mode/epoch 激活。CI 做 compile-time/static exhaustiveness（sealed task enum、无 wildcard mapping、全 enqueue call-site manifest 扫描）并用集成测试从 rendered Compose 启动 worker、读取 Redis registrations 做正反向断言。

Phase 1B 只建立完整 registry schema/data declarations、typed/static readers 与 final topology 声明；它的 closure 仅覆盖当时已实现的 producer/envelope/store 子集和静态 schema/exhaustiveness，不安装或注册 matching handler，也不声称 producer↔registry↔handler↔subscription↔Redis 完整相等。Phase 1D 在所有 final handlers 存在后才执行并关闭首次上线全 seam gate：producer constants/mappings、enqueue manifests、registry、handlers、rendered subscriptions 与 Redis registrations 必须 complete exact equality。payload 只携带 durable identity、required opaque snapshot IDs 和 bounded trace correlation，domain scope 一律从 PostgreSQL identity 解析。契约测试断言 serialized envelope 的 **physical queue + task type + payload schema/version + unique identity**，不能只断言 body。

首次上线没有历史 Redis/Oxana payload、pending identity 或在途 owner：双人确认销毁 Redis volume 后，先机械断言 `DBSIZE=0`、所有 queue/job/dead-letter/cron key count 为零，再一次性注册 final registry。不得创建历史 envelope converter、默认 Bid queue migration、无法解析 payload 收尾、多版本 worker共存或 production writer migration。最终 production DB role 从首次启动即按 fenced procedure 最小权限创建。

中间 commit 如为 CI 暂留 default/v0 handler 或 DTO，必须标记 **temporary prelaunch v0**；它不能进入 final artifact，Phase 1C 删除 extraction old paths，Phase 1D 删除 matching old paths。最终 Bid fixtures 仅保留：

- `testdata/oxana/bid-v1-convert-envelope.json`；
- `testdata/oxana/bid-v1-target-envelope.json`；
- `testdata/oxana/bid-v1-match-route-envelope.json`。

Housekeeping 拆成两个不可互换的 system lane；0010同时创建append-only `maintenance_housekeeping_activation_epochs`（activation epoch、gate epoch、allowed operations、actor/ticket/signature refs、issued/expires/deactivated timestamps）和durable `system_live_recovery_claims`（recovery kind/identity/generation/epoch、policy/feature/original snapshot FKs、status、claim token/attempt/heartbeat、bounded receipt），两者identity、FK retention和role grants互不复用：

- `system:maintenance-housekeep:v1` 是 maintenance-only operator purge/cutover lane。只有 gate=`maintenance` 且 `activate_maintenance_housekeeping` security-definer procedure 写入独立、单调、不可复用的 activation epoch、operator/ticket/signed authorization、允许操作集合与 expiry 后，才可注册、enqueue、claim；转回 open 或 epoch/expiry 不匹配立即 inactive。每次 activate/deactivate 都写独立 append-only audit。它可执行获批 retention purge/cutover cleanup，但不得作为 open-operation recovery。
- `system:live-recovery:v1` 是 final、versioned open-operation recovery lane，只处理 tagged `dirty_manifest|orphan_target|orphan_match_job`。envelope 固定 `recovery_kind + durable_id + observed generation/watermark/stage/heartbeat + recovery_epoch + original snapshot IDs`；handler 有自己的 registry identity、protocol minimum、`LiveRecoveryV1Handler` 和 recovery-policy/feature snapshots。它只在 gate=`open` 且 intended state enabled 时 claim；claim事务锁gate、recovery claim与domain row，并CAS gate epoch、recovery epoch、generation/watermark、claim token/attempt及owner/lease已orphan。commit前重复同一CAS；gate/epoch/generation/owner任一变化则no-op terminal。它只能原子创建缺失manifest/jobs，或在确认旧delivery/lease不active后重投同一`target_id`/`job_id`与原snapshots；cache miss回PostgreSQL，snapshot miss terminal fail。它禁止DELETE/purge、promotion/kill-switch/launch-state/registry/maintenance-gate mutation及其他control-plane mutation，DB role也不授予这些权限。
- live recovery concurrency 由 signed recovery-policy snapshot 对每 `recovery_kind` 给出小的硬上限，并受全局 semaphore/backpressure 限制；同一 durable identity 的 partial unique claim + token/attempt CAS 保证至多一个 active recovery owner。测试覆盖 duplicate/redelivery、dirty mutation race、heartbeat-owner resurrection、gate/epoch transition、bounded concurrency、snapshot miss、权限拒绝，以及 maintenance lane 在 open 无 registration/claim、live lane 永不 purge/control-plane mutation。

### 4.3 Provider retry ownership可注入

`RetryPolicy` 注入 seeded jitter RNG、`RetrySleeper`、clock/deadline；测试断言 exact delays、Retry-After clamp、heartbeat 和 cancellation，不依赖真实 sleep。adapter 每个 attempt 只发一次 HTTP。

- workflow 把 provider-stage attempt/result receipt 持久化到 target/job artifact；semantic/provider transient/permanent/contract failure 在内部 bounded policy 完成后先 DB finalization。
- 新 provider-stage Oxana workers `max_retries() = 0`。DB 已 finalization 的 semantic/provider failure（包括 approved fallback、permanent、contract-invalid、deadline exhausted、retry budget exhausted）返回 **queue success**，避免 Oxana 乘法重试。
- generation stale、lease lost、project ended、kill switch cancellation在 durable terminal/superseded/cancelled finalization成功后也返回 queue success。
- 只有尚未能写durable receipt的process crash、Redis transport redelivery或DB unavailable才可能redeliver；claim时读取stage receipt，已完成则短路。open期间由`system:live-recovery:v1`按target/job stage receipt与heartbeat找未完成durable intent，不依赖Oxana provider retry。
- 若DB finalization自身失败，worker返回infrastructure error用于观测，但仍不启用Oxana provider retry；process crash/orphan transport可能产生redelivery，其他情况由live recovery在CAS证明旧delivery/lease不再active后以同一durable identity重新enqueue。claim token/attempt/idempotency确保不重新执行已有terminal provider receipt。

Typed provider errors区分 transient/permanent/cancelled/contract_invalid；bounded retry、deadline、semaphore、backpressure、breaker、cancellation、lease heartbeat 和 redaction都由 immutable config snapshot决定。

### 4.4 Startup、liveness、readiness 与错误优先级

实施 scope 必须包含：

- `crates/api/src/main.rs`：profile parsing、startup validation、separate graceful shutdown、`/live`/`/ready` state；
- `crates/domain/src/lib.rs`：共享 typed runtime profile/config validation，不让各 binary 猜环境；
- `crates/worker/src/main.rs`：profile、gate/protocol registration、worker startup/liveness/readiness；
- `crates/bid/src/bin/bid_extract_eval.rs`：显式 `development|test|production` profile parsing，非法/缺失失败；
- `deploy/health/mode-aware-probe.sh`：统一可执行 probe contract，显式 `startup|liveness|readiness` kind 与 API/worker target；
- 根 `docker-compose.yml` 是仅含 `include: deploy/docker-compose.yml` 的 delegator，`deploy/docker-compose.yml` 是 final service definition；service healthcheck 只调用 startup/liveness probe，maintenance 下必须成功且不得触发 restart/rollback；readiness 是独立 deployment/traffic-gate probe，不作为 maintenance bootstrap 的 Compose healthcheck。rendered topology verifier 必须从根 delegator 展开 include，并把根文件、被 include 文件及签署的 final override 一并记录为 exact Compose input closure；任何隐式 `docker-compose.override.yml`、额外 `-f`/`COMPOSE_FILE` 或未列入签署 final input manifest 的 override 均拒绝。

`/live` 只表明 event loop/process 活着，不访问 provider/DB；`/ready` 才验证 migration version、PostgreSQL、maintenance mode、approved intended feature snapshot、canonical registry closure、该 intended state 展开的 enabled queue/capability dependencies、disabled lane enforcement、production no-stub 和必要 secret references。不能只检查“当前已有 job”或切换前仍 enabled 的 lane。draining/maintenance 时 API/worker startup+liveness 必须成功，readiness 必须返回明确的 not-ready reason；orchestrator 对该预期 readiness failure 只阻止 traffic，不 restart、不 rollback。审计转为 `open` 后，独立 readiness probe 必须 positive；unrouted smoke cleanup 后、exposure CAS 紧前还必须重跑一次 API+worker readiness，并按第7节生成短 TTL、单次消费的绑定 evidence，较早 readiness 不能复用作 exposure gate。任何“maintenance bootstrap 期间 Compose 静态 `/ready` healthcheck”的配置或文字均禁止。

Smoke 与 production ingress 的信任边界必须由 topology 而非 header 表达：final API 暴露两个不同 listener。operator smoke listener 使用只挂载给 audited smoke-runner service 的 Unix socket（或等价独立 network namespace/mTLS service endpoint），不发布 host/cluster port，production route 物理断开时仍可用；production listener 不接受 smoke bypass，且其 middleware 链第一步始终是 `mark_first_production_request`，在 auth、body/domain validation 和任何 business handler 前提交/确认 marker。`X-Smoke`、`X-Internal` 或任何 client header 都不能选择 listener、跳过 marker或取得 smoke 权限。Compose/network policy/firewall 拒绝 external network、普通 workload 与 operator workstation 直连 API service/container port；外部流量只能经 routing control plane 到 production listener。smoke cleanup 后由受审计 control 禁用 listener、关闭并删除 socket/撤销 runner credential，拓扑探针确认不可连接，然后才允许 final readiness 与 exposure CAS。测试必须证明 smoke 请求永不写 production first-request marker、production route 无可绕过 marker的 path、client header 不能 spoof 任一路径、smoke runner之外无法连接 smoke socket、external direct service access失败，以及 marker DB failure/timeout/mismatch时 production request被拒且 auth/business call counters为0。

所有 deployable binary 必须显式 `KNOWLEDGEBRAIN_RUNTIME_PROFILE=development|test|production`。production 最终 resolved adapter 禁止 hashed/echo/fake VLM/OCR；approved extraction heuristic 是 typed degraded/review policy，不冒充 provider。matching verifier及signed intended state中显式启用的Answer lane必须有真实chat capability；disabled Answer不成为first-launch capability blocker。

Grounded Answer 在 later activation（或 signed intended state 显式启用）时的 deterministic precedence 固定为：

1. authentication；
2. input/resource validation（含 product missing/invalid、no current、version selector）；
3. resolved capability；
4. retrieval；
5. generation；
6. verification；
7. server rendering。

因此 no-current 是 provider/retrieval 前的 validation，测试注入 call counters 断言零 provider calls。错误不可被后阶段覆盖。

### 4.5 Immutable image supply-chain contract

`deploy/images.lock.json` 是首次上线唯一 image authority，version-controlled、reviewed、signed artifact referenced。schema 必须按 platform 显式分区，至少为 `{platform, runtime_deployable[]}` 与 `{platform, build_base[]}` 两个互斥 set。runtime entry 含稳定 `lock_id + compose_service + deploy_target + repository@sha256:digest + architecture`；build-base entry 含稳定 `lock_id + dockerfile + stage + repository@sha256:digest + architecture`，不允许隐式 multi-arch tag。`runtime_deployable` 精确包含 PostgreSQL/pgvector 0.8.6、Redis、MinIO、final rendered Compose 中的 Neo4j、同一 release 的 API/worker Rust deployable targets、docreader及其他实际运行 service image；API 与 worker 即便共用 digest，也必须保留两个不同的 `(compose_service,deploy_target,lock_id)` identity。`build_base` 精确包含 `deploy/Dockerfile.rust`、`deploy/Dockerfile.docreader` 每个解析后 `FROM` stage/base identity。`(platform,set_kind,lock_id)` 唯一，runtime-entry identity 与 build-base entry identity 必须无交集；同一 digest 可因不同 runtime target 或 build stage 出现多次，但每个 source occurrence 只能精确匹配一个 lock entry，不能用 digest 相同合并 target，也不能让一个 entry 匹配多个 occurrence。

闭包按不同 identity domain 分别验证，再做总闭包：

1. 对每个 platform，从签署的 final Compose input closure 渲染 target-aware runtime set；rendered `(compose_service,deploy_target,lock_id,repository@sha256:digest,architecture)` 与实际 runtime inspect 的同形 set 必须各自 **exact equal** `runtime_deployable` target-aware subset。API/worker 共 digest 时仍是两个 identity；不得按 digest 去重，也不得把 build base 混入 runtime equality。
2. `docker compose config --images` 另做非 target identity 比较：把每行 registry resolution 规范化为 canonical `repository@sha256:digest`，形成忽略顺序但保留重复计数的 multiset；它必须 exact equal runtime lock 按每个 rendered `(compose_service,deploy_target)` 投影出的 digest-reference multiset。每个 rendered service/target 恰贡献一次；API/worker 共 digest 时该 digest multiplicity=2，不能以 set 去重，也不能用此 projection 代替第1项 target-aware equality。
3. 对每个 platform，所有 Dockerfile 的解析后 `FROM`（含 named stages、ARG 展开）identity及 reproducible-build provenance/materials identity，必须各自 **exact equal** `build_base` subset；每个 platform 的 runtime subset 与 build-base subset entry identities 均各自完整、无 missing/extra/duplicate，不得以 runtime image 补足 build provenance。
4. 验证器收集的 runtime matched entry identities 与 build-base matched entry identities 必须无交集、无重复；其 verified per-platform union 必须 **exact equal** 该 platform 完整 lock subset，全部 supported platforms 的 union 再 **exact equal** 整个 lock 文件且无 extra/orphan。任一 missing、extra、duplicate entry/reference、错误 subset/platform、digest/architecture mismatch、tag-only、未锁定 `FROM`、Compose input/override 漂移或 orphan lock entry 均 stop。

Reproducible build 产出 API/worker/docreader digest 后，deployment 只能 pull/inspect exact runtime subset；registry resolution、local image config/architecture 与 lock 任一不一致即 stop，禁止运行时重新 resolve tag。tag 可作 developer convenience metadata，但 tag、floating tag、build context hash 或本地 image ID 都不能成为 first-launch pin/evidence。cutover evidence 保存 lock 文件 hash、每 platform 两个 exact subset及verified union、target-aware rendered/inspect equality、normalized `config --images` multiset equality、每目标 digest、build provenance、registry result、完整 Compose input closure与最终 rendered topology。CI fixtures必须分别覆盖 runtime target extra/missing、API/worker同digest但identity或multiplicity丢失、`config --images`重复计数错误、build-base extra/missing、跨set identity重复、错误platform、per-platform/cross-platform union orphan/duplicate、非final override及合法同digest不同用途 identity。

---

## 5. Grounded Answer、retrieval、graph、OCR（later by default）

本节全部 runtime 默认属于 Phase 4/5，不阻塞首次上线；只有 signed intended feature state 显式启用对应 lane 时才成为首次 barrier。`declared_disabled` lane 只需满足不能 enqueue/claim 与 readiness disabled-enforcement。

### 5.1 Authenticated-global Answer

`POST /api/v1/answer` 只要求有效 authenticated actor，按现有 global knowledge visibility 解析 product/version；本文删除 workspace/project ACL 和 403 分支声明。missing/invalid resource、无 current 或非法 selector 仍在 validation 拒绝，使用统一 not-found/validation body，不返回 title/content/existence detail。更强 workspace/product authorization、ownership、row policy 是独立 domain migration，不能混入 grounding PR。

Candidate endpoint 与 trace endpoint 对 end user 继续 unavailable；trace query 默认 disabled 或仅 operator-internal + audit。不得把 incoming trace IDs 当 actor/scope。

保持现行 model contract：product 无 current 时 `422 VALIDATION_NO_CURRENT_VERSION`；即使 retrieval selector 指 explicit other/all-active，generation model仍取 product current summary model。若改变，必须另批产品/API migration。

### 5.2 Grounded engine outcomes

流程按上节优先级：resolved retrieval scope → bounded retrieval → typed claims → deterministic citation validation → bounded semantic verifier →最多一次 repair → server render/abstain。

- no hits：200非空 abstention，`NO_EVIDENCE`, review；
- missing chat：503 capability，不用 echo；
- 部分 approved channel失败：可 degraded 继续，仅输出 verified claims；
- 全 retrieval失败：503；generation provider失败：503；
- verifier失败：删除未验证 claims；若空则200明确 abstention/review；
- end-user response 不暴露 candidate artifacts或internal trace。

测试覆盖 unknown ID、非连续 quote、跨 document/version URL、prompt injection、render插入新事实和错误 precedence。

### 5.3 两条 graph path 与稳定 chunk ordering

PostgreSQL 与 in-memory graph path 都必须重新验证 node/chunk：requested version、same document/version、document enabled且未删除、product/tag/authenticated-global selector scope一致。命中 node 后遍历全部合法 mapped chunks；稳定顺序为 stored chunk ordinal升序，再 chunk UUID升序，limit在排序后应用。不能未验证地只取首个 chunk。keyword-only不生成 query embedding；RRF/rerank score不等同semantic support。

### 5.4 OCR准确范围

承认已存在部分 PostgreSQL successful-path测试。新增且仅新增缺口：

1. PostgreSQL persistence在 no-VLM/empty/error时不生成 fake OCR/caption row；
2. Bid含图 PDF/DOCX fixture覆盖 no VLM、成功 VLM和source span；
3. configured provider的429/timeout/5xx/401/invalid schema/empty response映射为typed failure class，并验证durable receipt、retry sleeper和no fake row。

---

## 6. Trace metadata（0011 schema / later runtime split）

**首次上线 schema prerequisite：** migration ordering 固定要求 additive `0011_ai_runs.sql` 在 0012 前执行。0011 必须创建下述 typed/bounded tables、enums/checks/FKs/indexes/retention metadata，使 0012 可保存 nullable typed trace references 并让 fresh chain 可执行；它不读取或转换历史数据。首次上线可让这些引用为空，且不要求 trace propagation、recorder、purger 或 operator query runtime。

**later observability runtime：** Phase 3 才实现 propagation/redactor、post-commit recorder、retention purge 与 operator-internal audited query。这些 runtime failure 不是 publication/matching correctness failure，也不得被首次上线 checklist 误列为 prerequisite。

`ai_runs/ai_run_spans` schema/runtime 最终只保存 typed/versioned/bounded metadata：opaque run/trace/domain IDs、server-derived scope IDs、kind/stage/status/quality/degraded、bounded reason/error code、provider/model/prompt/policy/verifier/score/config/feature versions、attempt/count/timing/cost。

- incoming `trace_context` 先校验格式/长度、生成本地 correlation link；不接受其 actor/operator/project/workspace/feature字段，不据此授权。
- 禁止 raw prompt/response/query/content/evidence/headers/credentials/图片和 unsalted content digest。未来 HMAC correlation需独立security migration。
- 每 kind 用Rust tagged DTO + schema_version；DB限制known kind/version/bytes/count。
- domain commit先完成；trace是timeout-bounded best effort副本，失败不回滚publication/matching/answer。
- retention mandatory；purge child后parent，带audit/metrics。candidate/trace无end-user endpoint；operator query需internal policy和访问审计。

---

## 7. PostgreSQL application maintenance gate 与 singleton cutover

不能只依赖 process shutdown timeout。`application_maintenance_gate` 必须被以下路径每次事务性检查：

- API scheduling、Bid/knowledge mutation endpoints；
- v1 enqueue intent creation；
- worker claim（claim transaction锁/read gate epoch）；
- publisher CAS；
- open-operation live recovery 的 dirty-manifest/orphan claim/requeue，以及 maintenance-only housekeeping 的 audited purge/cutover activation；
- operator promotion/rollback工具。

Mode语义：`open`允许批准功能；`draining`拒绝新 schedule/mutation/claim 但允许已 claim owner heartbeat/fenced finish；`maintenance`只允许审计 maintenance procedure；`rollback`只允许 kill-switch recovery，不切换 schema 或数据版本。每次改变写 append-only operator audit（actor、ticket、reason、old/new epoch、counts、evidence reference）。

0010 同时创建 durable singleton `production_launch_state`，最少包含：`singleton_key`、`cutover_id uuid`、`cutover_epoch bigint`、`state preflight|maintenance_unrouted|open_unrouted|reset_in_progress|exposure_committed|live`、`traffic_exposure_started_at`、`reset_authority_revoked_at`、`first_production_request_at`、`preflight_evidence_ref`、`open_evidence_ref`、`intended_state_hash`、`final_readiness_evidence_ref`、`reset_authorization_ref`、`exposure_evidence_ref`、`first_request_evidence_ref`、`evidence_epoch`、`updated_at`，以及 CHECK：marker 按序出现、exposure 与 revocation 必须同一事务同时非空、revocation 不可清空、first request 必须晚于 exposure。append-only `production_launch_state_events` 保存 old/new state、cutover/launch epoch、old/new gate epoch、actor/ticket、双人签名 refs、intended-state hash、route-disconnect attestation 与 evidence refs。应用/运维 role 不得直接 INSERT/UPDATE/DELETE；只有 owner-controlled、audited `SECURITY DEFINER` procedures 可转换，procedure 固定 `search_path`、校验调用者授权并按固定顺序锁 gate和singleton，且 DB revoke 所有绕过权限。

受审计 procedures 最少包括：`begin_cutover`、`record_preflight`、`open_unrouted`、`suspend_open_unrouted`、`reopen_unrouted`、`record_final_readiness`、`commit_traffic_exposure`、`mark_first_production_request` 与 `authorize_pre_exposure_reset`。`open_unrouted`只允许初次`preflight → open_unrouted`；`reopen_unrouted`只允许修复后的`maintenance_unrouted → open_unrouted`；两者都CAS `cutover_id + cutover_epoch + expected launch_state + expected gate epoch + gate mode=maintenance + intended_state_hash`，在同一事务把gate改为`open`并递增gate epoch。`suspend_open_unrouted`只允许`open_unrouted → maintenance_unrouted`，CAS同一组identity/state/gate epoch，在同一事务把gate改为`maintenance`并递增gate epoch。三个procedure都要求production route仍由control plane机械断开且attestation新鲜，三个marker全NULL、smoke endpoint当前状态已被evidence明确记录、intended/evidence hash一致，并由两个不同授权人签同一transition evidence；event保存失败原因/修复证据。maintenance修复后只能用`reopen_unrouted`进入同一cutover，必须重新验证intended-state hash且生成新open evidence；若intended state、image、migration head、registry或topology变化则旧one-shot evidence失效，必须按获批流程建立新preflight/cutover，不能悄悄reopen。

`suspend_open_unrouted`、`reopen_unrouted` 与 exposure/reset 并发时由 gate+singleton row lock和 expected gate epoch/launch_state 决胜：suspend先提交则旧 readiness及 exposure CAS因state/epoch mismatch失败；exposure先提交则 suspend/reopen 永久失败；两个 reopen只有一个可递增gate epoch。任一 exposure/revocation/request marker 非 NULL 时，这套 **prelaunch** suspend/reopen procedure 永久 forbidden，不存在清 marker或恢复路径。上线后的 draining/maintenance/reopen 必须走第7节 post-launch non-destructive rollout/recovery policy的独立 procedure、release evidence和gate epoch，绝不调用或扩展 prelaunch procedure。

`production_final_readiness_evidence` 是 append-only、单次消费表：每行精确绑定 `cutover_id + gate_epoch + migration_head + image_lock_hash + registry_hash + intended_state_hash + topology_hash + smoke_cleanup_evidence_hash + route_disconnected_attestation_id + route_attestation_issued_at + route_attestation_expires_at`，并保存签署 policy 给定的 route-attestation TTL、API/worker probe result/hash、evidence `issued_at`、`expires_at`、`consumed_at`、`consumed_by_exposure_event`。route attestation 必须在调用时满足 `issued_at <= now < expires_at`、`expires_at-issued_at` 不超过签署 TTL，且其 ID 未被其他 cutover/gate/evidence绑定或消费。只有 smoke cleanup、smoke socket关闭与production route-disconnect attestation均确认后，`record_final_readiness` 才接受同一时刻窗口内 API+worker 均 positive 的结果；它必须锁 gate/singleton，并把调用参数、当前 migration/image lock/registry/intended state、rendered `topology_hash`、当前 smoke cleanup/evidence hash及 fresh route-attestation完整 tuple 做 exact equality 后才写行。readiness TTL 由签署 launch policy固定且不得超过5分钟。

`commit_traffic_exposure` 必须在 **连接任何 production route 前**锁 gate、singleton、evidence及attestation，并再次把调用参数、singleton/current verifier facts和该 evidence 的上述完整 tuple做 exact equality；同时要求 state=`open_unrouted`、evidence与route attestation均未过期/未消费、attestation未被其他 evidence/transition复用、smoke endpoint仍关闭且route仍机械断开。任一字段 mismatch，任一 evidence/attestation expiry、消费或复用均拒绝；任何后续 gate epoch、migration head、image lock、registry、intended state、rendered topology、smoke cleanup/evidence或route/attestation变化都立即使 readiness evidence失效。通过时在同一事务消费 readiness与route attestation，并单次写 `traffic_exposure_started_at`、`reset_authority_revoked_at`、state/evidence/event。routing tool 只有读回 commit 且完整 tuple匹配后才能连接 route；connect 失败也不得清除 revocation或复用 readiness/attestation。较早 readiness、部分 probe或替换 attestation一律不能 exposure。

`mark_first_production_request` 必须由 production ingress listener 的第一层 middleware 在任何认证、body/domain validation 或 business processing 前调用。procedure 在一个事务内原子写 `first_production_request_at`、`first_request_evidence_ref`、对应 append-only launch event/audit row，并把 state 推进到`live`；并发/重试只能确认同一 cutover/epoch/evidence 已提交的完整四项结果，不能只看到 timestamp 就放行。smoke listener永不调用此 procedure。事务失败、超时、部分写入或 evidence mismatch 时拒绝 production request，且 authentication、validation与business handler call counter全部为零。

`authorize_pre_exposure_reset`/reset procedure 必须同时证明：gate=`maintenance`且 launch state=`preflight|maintenance_unrouted`；production route由control plane机械断开并有新鲜attestation；`traffic_exposure_started_at`、`reset_authority_revoked_at`、`first_production_request_at`全为NULL；请求cutover_id/epoch、gate epoch、intended-state hash与singleton匹配；两名不同授权人对同一reset evidence签名。offline cutover reset dispatcher 必须先在固定锁顺序下检查 launch state：`preflight|maintenance_unrouted` 才可直接调用 reset；`open_unrouted` 必须先以匹配 epoch/evidence CAS 成功调用 `suspend_open_unrouted`，进入 `maintenance_unrouted` 后再以新 gate epoch调用 reset；`exposure_committed|live` 或任一 exposure/reset-revocation/request marker 已存在时永久 hard-stop；`reset_in_progress` 只能继续读取并执行同一不可复用 token，不能再次派发 reset。不存在“发现任意 singleton 就 reset”或可歧义的 singleton 分支。reset procedure在固定锁顺序下CAS到`reset_in_progress`并写不可复用的`reset_authorization_ref`/event；`commit_traffic_exposure`只接受`open_unrouted`，因此reset authorization commit后任何routing CAS失败，reset tool也只接受读回匹配token后销毁。任一marker已存在就永久拒绝，procedure不提供从`reset_in_progress`回open、清空marker、回退epoch或恢复reset authority的路径；失败重来必须在fresh topology创建新cutover。首次上线 clean-slate cutover由version-controlled runbook/tool执行，严格分为两个区间。

**A. maintenance-safe offline preflight（无 external route/traffic）：**

1. 验证 Phase 0、1A、1B、2、1C、1D（schema-only 0011 + complete 0012）全绿，final artifact 已由 CI 签署；Phase 1D 已删除 old matching implementation/handler/store/DTO，candidate/trace 无 end-user route。确认从未部署 Phase 中间 binary。
2. cutover tool 记录 environment UUID、ticket、proposed new `cutover_id`/epoch、image-lock hash、queue-registry release/hash、migration head、artifact digest、rendered topology 与全部待销毁 development/staging stateful volumes。两名不同授权人以独立 credential 对同一 evidence 签名。若已有 launch singleton（包括失败的前次尝试），tool 必须按上文 reset dispatcher检查 state：`preflight|maintenance_unrouted` 可调 reset，`open_unrouted` 必须先 CAS `suspend_open_unrouted` 后再 reset，`reset_in_progress` 只能继续同一 token，而 `exposure_committed|live` 或任一 exposure/revocation/request marker 非NULL永久 hard-stop；不得以“存在任意 singleton”为由直接 reset。只有首次建立且机械证明从未存在该 schema 的非生产 bootstrap 才走同等双签/route-disconnect外部 bootstrap authorization。tool 验证目标绝非已有 production traffic 环境后才销毁 stateful volumes；fresh migrate 后再由 `begin_cutover` 绑定 proposed ID/epoch。
3. 按 `deploy/images.lock.json` 每 platform 分别验证 Dockerfile/provenance target identities=`build_base` exact subset、target-aware rendered Compose/runtime inspect=`runtime_deployable` exact subset，并单独验证 normalized `docker compose config --images` digest-reference multiset（每service/target一次、同digest保留multiplicity）；验证runtime/build entry identities无交集、per-platform union及cross-platform union exact equal完整lock。render必须从根 `docker-compose.yml` include delegator展开完整签署Compose input closure，任何non-final override都stop。只启动runtime subset中的fresh PostgreSQL/pgvector 0.8.6、Redis、MinIO、必要graph/docreader与final services。任何tag-only、missing/extra/duplicate identity或multiset occurrence、subset/platform错配都stop。此时load balancer/service route必须物理未连接。
4. migration 前机械断言 PostgreSQL 除系统 catalog/extension 外 **无 user table、无 domain row**；Redis `DBSIZE=0` 且 queue/job/dead-letter/cron key 全为零；object/graph stores 无 customer object。取得 advisory/operator lock，按显式 manifest 执行 `0001`–`0007`→`0008_bid_extract_running_constraints.sql`→0010→0011→0012 并逐条写 ledger；bundle 和 embedded sources 中不得存在 `migrations/0008_backfill.sql`、`schema_flags`、`0013_bid_backfill` 或历史 0009 alias。repeat apply 为零变化，concurrent/tamper/name/order/manifest-missing-or-extra/unknown evidence 来自同一 artifact CI。
5. 只 seed `deploy/first-launch/catalog-row-allowlist.toml` 精确列出的 modern execution/feature/policy snapshots、maintenance/launch singleton、kill switch、feature-state/promotion audit 起点、minimum roles/protocol metadata。紧接 migration/seed 且在 smoke 前，将 `pg_class/pg_namespace/pg_attribute/pg_constraint/pg_indexes` 的全部 user schema object identity/type/owner 与每个 user table 的 exact row count、seed PK/natural key/value hash 双向比对 allow-list；任何 missing/extra object、table、column、constraint、index、trigger、row 都 stop。显式断言 `to_regclass('schema_flags') IS NULL`，全 catalog、ledger、seed key/value 中均无 `schema_flags`、`0013_bid_backfill`、backfill/compatibility marker/table/row、legacy placeholder 或 `frozen-0009` fixture。
6. gate保持`maintenance`。在其后仍closed的gate内，通过audited procedures创建/记录`production_launch_state` preflight evidence，并将 **approved intended feature state** 写为final snapshot；首次launch intended state必须启用final Bid conversion/extraction/matching、`system:live-recovery:v1`及签署的score/verifier/recovery policy，其他product lane可按registry标为`declared_disabled`，maintenance housekeeping保持`maintenance_only`且无activation epoch时inactive。preflight根据intended snapshot展开enabled lane的全部capabilities/secret refs/queues，并验证disabled lane enqueue/claim fail closed，不得只看变更前enabled lane或当前Redis jobs。
7. 在仍为零key的Redis一次性应用`deploy/queue-registry.toml`；对producer constants/mappings、enqueue call-sites/manifests、registry、handlers、rendered subscriptions与Redis registrations做第4.2节全seam closure。enabled active subset exact注册，declared-disabled无active registration；maintenance-only仅在另行审计的有效activation epoch下临时active，普通cutover bootstrap保持inactive；`sync`、旧Bid tasks、旧`system:housekeep:v1`、Section-retry task、unknown fallback或任何extra registration都stop。
8. 启动唯一 final API/worker binary set；本 `cutover_id` 从 final preflight 到 first-request evidence 不再替换 image、binary、migration head、registry/intended state 或 topology。Compose/orchestrator startup+liveness probe 必须成功且保持服务运行；独立 `/ready` 因 `maintenance` **按预期失败**，只阻止 traffic，绝不触发 restart/rollback。运行不需 claim/mutation 的 migration、image-lock、registry、role/no-stub、security/static、signed evaluation 与 capacity preflight。

**B. audited maintenance→open 与 unrouted smoke：**

9. 两名 operator 核对 offline evidence 后，以 `open_unrouted` security-definer procedure CAS launch state/gate epoch并将 gate `maintenance→open`，把 cutover/epoch、final intended-state hash、image/migration/registry/artifact/topology hashes、route-disconnect attestation、ticket与签名写入append-only audit/launch event。禁止先开endpoint再补audit，且开门不等于暴露traffic。若随后失败，只能在三个marker全NULL、route仍机械断开时以双签 `suspend_open_unrouted` 回到 `maintenance_unrouted`；修复后用匹配新gate epoch/evidence的 `reopen_unrouted`，不得直接改gate或state。
10. gate open 后要求 API 与 worker独立 readiness probe 都为 positive；readiness 必须覆盖 intended enabled lanes（尤其`system:live-recovery:v1`、`bid-conversion-v1`、`bid-extraction-v1`、`bid-matching-v1`）、registry closure、maintenance housekeeping inactive、disabled enforcement、snapshots、minimum protocol、real capabilities与no-stub。任何一项失败则按步骤9的suspend procedure修复或在满足reset条件时destructive retry；此处结果只允许进入smoke，不能作为最终exposure evidence。
11. production route仍物理关闭时，仅通过独立Unix socket/隔离service endpoint且只对audited smoke-runner可达的operator smoke listener，使用专用审计smoke actor/tenant跑完整mutation→durable enqueue→claim→candidate→publication→matching manifest→Technical/Commercial route claim→artifact/projection commit→API/UI/booklet readback；断言stale/route fencing、human assessment与immutable verifier evidence分离、Part 4/5 semantics、无candidate endpoint，且production first-request marker仍为空。smoke创建的现代domain rows保留为审计evidence或按已批准non-destructive cleanup procedure删除；不得伪称仍是post-seed zero-row checkpoint。再次跑security与bounded load/queue-clear smoke并签署launch report。随后完成cleanup，审计关闭/删除smoke socket并撤销runner credential，拓扑/firewall探针证明smoke endpoint和external direct service path均不可达。
12. smoke cleanup后且紧邻exposure CAS，重新执行一次完整 API+worker readiness，并取得fresh、短TTL且不可复用的 route-disconnected attestation。`record_final_readiness`与`commit_traffic_exposure`都必须 exact 验证并绑定 `cutover_id/gate_epoch/migration_head/image_lock_hash/registry_hash/intended_state_hash/topology_hash/smoke_cleanup_evidence_hash/route_disconnected_attestation_id+issued_at+expires_at/TTL`；readiness TTL≤5分钟。routing tool只可把这条fresh evidence及同一attestation传给`commit_traffic_exposure`：在route仍机械断开且smoke endpoint已关闭时，以完整 tuple匹配的不可逆CAS同一事务消费readiness/attestation并写`traffic_exposure_started_at`+`reset_authority_revoked_at`+event，commit/readback成功后才连接load balancer/service route。任一 mismatch/expiry/reuse，或 topology/smoke/route 在 record 后变化都 fail closed；CAS或route connect失败也fail closed，一旦CAS commit，即使尚无request或connect失败也永久禁止destructive reset。production listener的首条routed request由第一层middleware在auth/body-domain validation/business前调用`mark_first_production_request`，并在一个事务原子写 timestamp、evidence ref、append-only event/audit及`live` state；失败时拒绝请求且全部handler call为零。

**one-shot 的机械定义：** exact image digests、migration head、registry release/intended state 与 rendered topology 只在本 `cutover_id` 从步骤8 final preflight evidence到步骤12 exposure及首条request marker evidence的有界窗口内冻结。pre-exposure suspend/reopen不允许改变这些值，并使旧readiness失效；窗口完成后可按下述post-launch policy演进，不存在“以后永远固定同一digests/head/registry/topology”的要求。destructive retry仅可在route机械断开、gate=maintenance、launch state=`preflight|maintenance_unrouted`、三个markers全NULL、matching cutover/gate epoch且双人evidence通过时发生。

**Post-launch rollout policy（另行逐次 review）：** 每个release有新release/evidence ID、备份与forward/rollback-compatible演练、image/registry/topology diff、intended-state diff、readiness/canary/rollback gate和审计；只允许non-destructive binary/config/topology release及ledgered additive forward migration。迁移仍immutable checksum、单向且先兼容旧读写再切流；不得drop/truncate/recreate production state、回退ledger、清launch markers或恢复reset authority。只要exposure marker存在，prelaunch `suspend_open_unrouted/reopen_unrouted/authorize_pre_exposure_reset`全部拒绝；正常postlaunch maintenance/recovery使用独立non-destructive procedures和release/gate epochs，不能复用首次上线state machine。任何destructive data migration必须走独立批准的data/backup/recovery policy。

Stop 条件：双人确认/环境identity不完整；pre-migration出现user table/domain row；post-seed catalog/row超出exact allow-list；出现`schema_flags`/`0013_bid_backfill`/compat marker；Redis注册前非零；发现历史payload/customer object；target-aware image runtime/build subsets、`config --images` multiplicity、entry-disjoint、per-platform/cross-platform union或根Compose input closure非exact equality；queue closure非exact equality；ledger/checksum/name/order/manifest missing-extra、fresh/repeat/concurrent失败；snapshot/FK/policy缺失；temporary v0/old matching/Section-retry handler仍存在；role可绕过fenced/security-definer procedure；maintenance下startup/liveness失败或readiness意外成功；open后readiness失败；maintenance housekeeping在open active；live recovery越权purge/control-plane；disabled lane可enqueue/claim；intended capability/provider失败；candidate可见；smoke/production ingress未拓扑隔离、smoke endpoint未关闭；final API+worker readiness完整tuple或route-attestation不fresh、mismatch、过期、复用、已消费，或topology/smoke/route变化；reset dispatch state不明确；CAS/route/first-request四项原子写 race、matching smoke、evaluation/security/capacity gate未签署；audit缺失；或任一marker已存在却请求prelaunch suspend/reopen/reset。

Launch-state测试必须使用DB/control-plane barriers而非sleep：两个routing process对同一/不同epoch并发CAS只有一个成功；suspend/reopen分别与exposure/reset竞争且由expected gate epoch+launch state唯一决胜，exposure先提交后所有prelaunch recovery永久失败；两个reopen只有一个成功且旧readiness失效；reset dispatcher对`preflight|maintenance_unrouted`直达、`open_unrouted`先suspend、`reset_in_progress`只续同token，并对`exposure_committed|live`/任一marker永久hard-stop，不存在any-singleton reset；reset与exposure CAS竞争时只能reset胜出并提交`reset_in_progress`使exposure永久失败，或exposure胜出后reset永久失败。`record_final_readiness`和`commit_traffic_exposure`都分别覆盖完整绑定 tuple 的字段 mismatch，evidence/route-attestation过期或复用，topology/smoke cleanup hash/route attestation在两调用间变化，API/worker任一缺失；两个exposure只能单次消费同一readiness+attestation。CAS commit后route-connect失败仍保持revoked；route不得先于CAS commit；smoke请求从不写production marker，production listener不能绕过marker且headers不能spoof任一路径；marker DB failure/timeout/evidence mismatch/append-event失败时request被拒且auth/validation/business call counter=0；两个首请求并发只提交一组 `first_production_request_at + first_request_evidence_ref + append-only event/audit + live`，且都只能在完整事务确认后进入handler；错误cutover/epoch、单签名、陈旧route-disconnect evidence、非maintenance、任一marker非NULL均拒绝reset；direct table DML与search-path/privilege escalation失败；process crash/retry保持state/event幂等且marker不倒退。

---

## 8. Evaluation gates（shadow 前预注册）

每个 feature 在其**任何 shadow 前**必须有 immutable signed gate artifact schema。首次上线必须实际签署并通过 publication extraction、完整 final matching 及 approved intended feature state 所依赖的 security/load/capability gates；trace runtime、Grounded Answer、retrieval/rerank、graph/multimodal/OCR 若不在 first-launch intended state，则其 gate 只约束各自 later roadmap activation，不能阻塞或被计入首次上线完成度。artifact预注册：

- primary metric及方向；
- 95% confidence level，或明确写出获批的其他 level与批准人；
- source-group clustered paired bootstrap（默认，以source document/customer family为cluster，固定seed和resamples），或另一种预先命名、论证的方法；
- total sample minimum、每关键slice minimum、source-group minimum；
- non-inferiority margins；
- extraction/matching false-clean上限、Answer unsupported-claim上限；
- correct-abstention下限；
- p95端到端/provider latency上限；
- sustained capacity、queue-clear latency/backpressure 边界；
- per-run/request cost上限；
- required slices、dedupe、annotation/adjudication、train/calibration/test group隔离；
- metric source availability、owner、expiry和promotion action。

所有数值在signed gate artifact存在前都标 **provisional**，不能在代码/本文中假装批准。CI跨margin、sample/slice不足、metric缺失或artifact过期均=`inconclusive`；inconclusive绝不能promote。false-clean和unsupported-claim breach是hard fail，即使平均primary metric通过。first-launch matching artifact 还必须命名 launch mode（含 score/verifier policy version）、fixture/report hashes 与 API/UI/booklet contract version；选择 v1-compatible score mode不允许退回旧 matcher。

---

## 9. Migration 与实施阶段顺序

Fresh migration 顺序由 version-controlled `deploy/first-launch/migration-manifest.toml` 精确固定，是首次上线 barrier 的一部分；**不再声称 0001–0009 原样保留**：

1. 保留 fresh-only `0001_domain.sql`–`0007_bid.sql`；
2. **删除** `migrations/0008_backfill.sql`，并从 embedded bundle、runner、fixtures、tests、seed、catalog 中彻底删除 `schema_flags` 与 `0013_bid_backfill`；clean-slate 不需要 backfill/idempotence compatibility marker；
3. 将当前实际 Bid running-run expression constraint SQL 从 `0009_bid_extract_running.sql` 重命名/收敛为唯一 ledger version 0008：`0008_bid_extract_running_constraints.sql`。该文件只含约束/index DDL，是原子 migration；manifest/ledger 固定精确 `{version=8,name="bid_extract_running_constraints",sha256=<raw-bytes>}`，不存在 0009 alias、双登记或名称歧义；
4. `0010_bid_extract_publication.sql`：publication schema + immutable snapshots/feature audit/maintenance gate + `production_launch_state`/events/security-definer procedures；
5. `0011_ai_runs.sql`：additive typed trace **schema prerequisite**；
6. `0012_bid_match_contract.sql`：首次上线 required matching artifacts/projections/typed contract。

`crates/storage/src/persist.rs`继续在同一 session advisory lock 下按 manifest 顺序 `include_str!`。空库 runner 先创建空 ledger contract，再逐个 migration 执行并仅在该 migration 成功后原子写入 `{version,name,checksum,applied_at}`；不先执行一组 migration 再批量登记。checksum 是 raw embedded UTF-8 bytes 的 SHA-256，不做换行归一化。已登记版本绝不重放；checksum/name mismatch、重复 version/name、manifest missing/extra/reorder（包括任何 0009 alias）、未知更高 DB version 或部分写入均 fail closed，不自动修正。版本连续性按 manifest 的 exact ordered versions 校验（显式序列 `1..8,10,11,12`，不把有意不存在的 0009 误判为可补 compatibility migration）；0011 必须先登记成功，0012 才可执行。

首次上线验证：pre-migration 无 user tables/domain rows；空库完整执行 manifest；post-migration/seed catalog 与 rows 双向精确等于 `deploy/first-launch/catalog-row-allowlist.toml`；第二次 apply 零 DDL/零 seed 变化；两个 concurrent startup 得到同一完整 ledger；失败 migration 不留半条 ledger；checksum tamper/manifest missing-extra-reorder/unknown fail closed；0010/0011/0012 constraints 与 `ON DELETE RESTRICT` 正确。negative source/bundle/catalog/row scan 必须证明 `0008_backfill.sql`、`schema_flags`、`0013_bid_backfill`、0009 alias、`testdata/migrations/frozen-0009.sql`、含数据 baseline/compat seed 分支、含历史行 upgrade criteria 与 multi-binary compatibility tests全不存在。ledger 作为上线后 additive forward migration 基础保留，并受第7节 post-launch rollout policy 约束。

### Phase 0 / PR 0 — Existing matching safety foundation

- 把 empty-commercial clear 收进 generation-fenced route seam，补 confirmed mutation/schedule race，建立 stale/route behavior 的 test oracle；不声称跨 route atomic replace。
- 这是 prelaunch foundation，不是 final architecture。Phase 1D 必须删除其 old matching handler/store/DTO；final v1 不得 wrap/call/dual-write 它。
- Exit：stale commercial job 零 write、empty 只清 commercial，且 tests 可在 0012 实现上原样通过。

### Phase 1A / PR 1A — 0010 fresh schema

- 删除 `0008_backfill.sql`/`schema_flags`/`0013_bid_backfill`，把实际 Bid constraint 收敛为 ledgered `0008_bid_extract_running_constraints.sql`；实现 manifest exact chain、ledger、heads/targets/candidates/publication state、aggregates、named executable indexes/composite consistency constraints；
- snapshot/feature state/promotion audit/maintenance gate、`production_launch_state`/events 与仅 security-definer transitions；空库 modern seed + catalog/row exact allow-list contract；无 candidate routes；
- 只 merge + CI，不部署。direct writer/v0 DTO 如为中间编译暂留，只能标 temporary prelaunch v0；
- Exit：fresh/repeat/concurrent/tamper/manifest mismatch、catalog/row allow-list、no compatibility marker、launch-state privilege及suspend/reopen/reset/readiness-consumption/exposure/first-request CAS schema races与modern seed tests通过；final read contract对modern publication facts通过。

### Phase 1B / PR 1B — Snapshots + canonical registry declarations + isolated envelopes

- immutable snapshot store/API、kill switch/operator audit mandatory；建立 `deploy/queue-registry.toml` 的完整schema/data declarations及domain/runtime typed/static readers，声明所有non-Bid/final Bid/system lanes和`required_enabled|declared_disabled|maintenance_only` + signed intended state；
- 建立 dedicated `bid-conversion-v1`、`bid-extraction-v1` envelope/identity；Section retry仅target identity。Phase 1B不得安装、注册或伪造尚不存在的matching handler；完整producer↔registry↔handler↔subscription↔Redis equality是Phase 1D/first-launch gate；
- 只运行隔离的schema/store/queue-envelope serialization tests。不得启动scheduler/provider/workflow，不得写target/candidate/matching shadow，不得执行Redis registration或rendered active subscription；中间binary不部署、不接共享Redis；
- Exit：缺snapshot/gate的isolated store/envelope构造fail closed；完整registry declarations通过schema、唯一性、launch-mode、platform-independent static validation；仅对已实现producer/envelope/store子集做static equality/exhaustiveness和convert/target fixtures。不得把缺少final handlers/Redis的1B结果称为complete closure。

### Phase 2 / PR 2 — Runtime、supply chain、maintenance 与 readiness safeguards（1C 前）

- runtime profile、`ResolvedCapabilities`、API/domain/eval startup、separate startup/liveness/readiness、mode-aware probe script与API/worker checks；readiness按intended state +当前已实现subset，maintenance not-ready不得触发restart/rollback；first-launch complete queue equality仍留到1D；
- typed provider errors、seeded RNG/sleeper、provider-stage`max_retries=0`、receipt recovery、backpressure/breaker/cancel；
- maintenance/launch-state gate、最小权限roles、protocol fencing；实现独立audited activation epoch的maintenance-only purge/cutover lane与final `system:live-recovery:v1` fenced dirty-manifest/orphan recovery lane、bounded concurrency和权限隔离；
- image lock按platform做target-aware runtime/build-base entry identity exact subsets、独立 `config --images` digest multiset及per-platform/cross-platform verified union；从根Compose delegator验证签署input closure并拒绝non-final override；Dockerfiles/Compose/env/README与CI closure；双人cutover runbook/tool、按launch state精确dispatch reset、机械隔离smoke listener、unrouted smoke、smoke cleanup后绑定完整tuple与fresh route-attestation的single-use API+worker readiness、race-free exposure/原子first-request marker tool与post-launch rollout policy；
- shadow execution ownership从1B移到本phase：只有runtime profile、resolved capabilities、gate与readiness机制已存在后，才允许prelaunch/default-off的disabled target/candidate/matching shadow execution；它们使用隔离DB/Redis、signed shadow gate和现代snapshots，不注册production route、不成为1B exit条件。Phase2 matching shadow只能针对Phase0 generation-fenced test oracle且不产出launch evidence；1D安装0012后必须在final artifact path重跑并以该结果替换它，随后删除Phase0 seam。score-v2 shadow也只在1D final 0012 path执行；任何旧matcher shadow都不进入final binary/topology；
- 只merge + CI，不部署；
- Exit：production no-stub、retry非乘法、system lanes gate/CAS/权限测试、role bypass失败、image target-aware subsets/config-images multiset/entry-disjoint/per-platform+cross-platform union及Compose-input exact closure、maintenance startup/live=true且ready=false而无restart/rollback、open readiness机制、smoke/production ingress隔离、完整final-readiness/attestation消费、reset dispatch与原子first-request的launch-state并发/故障tests、load/security report签署。

### Phase 1C / PR 1C — Final extraction publisher cutover

- 单一 publisher、hard coverage、recovery、partial/superseded aggregation；
- 删除 direct full/document/Section retry writers 与 extraction temporary v0 handlers/DTO；只保留 target-based extraction；
- publication preflight 覆盖 migration/CAS/no-candidate-route/evaluation；gate 内 default-off，等待 final intended snapshot。
- Exit：0010 immutable publication contract、all races、API read model、fenced claim/publish 与 signed extraction gate 全绿。

### Phase 1D / PR 1D — Prelaunch complete final Bid Matching

按可执行顺序完成，不能拆成 post-launch activation wrapper：

1. 先提交 additive `0011_ai_runs.sql` 与 runner/schema tests；只要求 typed trace schema，不要求 propagation runtime；
2. 再提交 `0012_bid_match_contract.sql`：generation manifests、immutable requirement/report/candidate artifacts、current projections、immutable pick snapshots、约束/index/retention；
3. 实现 immutable `MatchingRequest`/`MatchingReport`/candidate/value/score DTO；所有 matching-relevant mutation 在同一 project lock/事务递增 `mutation_watermark`、置 dirty 并隐藏 current projections；atomic dirty manifest scheduler 固化 exact watermark/snapshots、route jobs 写 `expected_mutation_watermark` 后才更新 `scheduled_watermark`/clear dirty；route-fenced `MatchResultStore` 执行双 watermark+snapshot CAS 与 Technical/Commercial clear/skip semantics；
4. 安装 dedicated `bid-matching-v1` physical queue、v1 envelope/handler、claim/heartbeat/recovery、required config/feature/score/verifier snapshots 与 capability checks；
5. 实现 approved first-launch score/verifier policy与可选 score-v2 shadow，签署 launch-mode eval artifact；无论选择何种 score mode都只能走0012 artifact/projection path；
6. 完成 API/UI/workflow/booklet 对 immutable machine evidence 与 human assessment 的分离、Part 4/5规则、immutable pick readback；
7. 删除 old matching implementation、old task/constants/queue mapping、store/DTO/serializer paths和 Phase 0 temporary seam；禁止 wrapper、dual-write、fallback；
8. 关闭 canonical registry，使 actual handler/queue/task set 与 registry exact equality，并完成 conversion/extraction/matching end-to-end smoke、security/load/evaluation签署。

Exit：第3节全部 contract、0011→0012 migration、Bid final registry/fixtures、exact stale-commit watermark races（含 scheduler/mutation 与 Technical/Commercial route interleavings）、append-only/pick immutability、score rounding、API/UI/booklet hidden-current/human-evidence tests和signed matching launch gate全绿。

**首次 launch barrier：** Phase 0、1A、1B、2、1C、1D 以及 exact fresh manifest（0001–0007→renamed atomic 0008 constraints→0010→0011→0012）、catalog/row allow-list、image/registry locks、readiness/security/load/evaluation gates 全部完成签署前，不启动 production binary。首次只启动含 1D 的唯一 final artifact/topology；不存在 Phase 1A/1B/2/1C intermediate deployment 或 user traffic。

### Phase 3 — Later trace runtime

- 在已上线 0011 schema 上实现 redactor、传播/post-commit recorder、retention/operator-internal audit；不是 first-launch publication/matching prerequisite。

### Phase 4 — Later retrieval/graph/OCR

- independent channels、两条 graph scope/stable chunks、指定 OCR 缺口；各自 shadow 前注册 signed gate，不回溯为 first-launch prerequisite。

### Phase 5 — Later Grounded Answer

- authenticated-global engine、claim verifier/server rendering/error precedence；shadow 后按 signed gate promotion，不是 first-launch prerequisite。

### Phase 6 — Optional control plane UI

- automated cohort registry/promotion UI 可选。basic feature store、snapshots、audit promotion、kill switch 已在 first launch 完成。

---

## 10. 文件与窄 PR scope

完整 implementation/delivery 落点（本次只改计划，不修改这些文件）：

```text
# dependency manifests（TOML/JSON registry/image parsers、SHA/probe support；lockfile随依赖变化提交）
Cargo.toml
Cargo.lock
crates/storage/Cargo.toml
crates/models/Cargo.toml
crates/domain/Cargo.toml
crates/runtime/Cargo.toml
crates/bid/Cargo.toml
crates/api/Cargo.toml
crates/worker/Cargo.toml

# conditional dependency manifests（仅对应 dependency graph 发生变化时提交；否则保持 untouched）
web/package.json
web/package-lock.json
services/docreader/pyproject.toml
services/docreader/uv.lock

# first-launch migrations / storage
DELETE migrations/0008_backfill.sql
RENAME migrations/0009_bid_extract_running.sql -> migrations/0008_bid_extract_running_constraints.sql
migrations/0010_bid_extract_publication.sql          # includes launch-state schema/procedures/events
migrations/0011_ai_runs.sql
migrations/0012_bid_match_contract.sql
crates/storage/src/persist.rs
crates/storage/src/bid.rs
crates/storage/tests/launch_state_races.rs            # suspend/reopen/reset/readiness/exposure DB barriers
DELETE testdata/migrations/frozen-0009.sql

# typed contracts, publication, complete matching
crates/models/src/lib.rs
crates/models/src/http.rs
crates/domain/src/lib.rs
crates/domain/src/status.rs
crates/domain/src/queue_registry.rs                 # new canonical reader/types
crates/bid/src/lib.rs
crates/bid/src/extraction/*
crates/bid/src/matching/*                           # new final request/report/store/policy
crates/bid/src/booklet.rs
crates/bid/src/export.rs
crates/bid/src/bin/bid_extract_eval.rs
crates/api/src/main.rs
crates/api/src/launch_state.rs                        # production first-request pre-auth/business marker middleware
crates/api/src/smoke_ingress.rs                       # isolated socket listener; never header-selected
crates/api/tests/launch_state_ingress.rs              # listener/header/bypass/marker failure call counters
crates/api/src/routes.rs
web/src/api.ts
web/src/bid/*

# first-launch runtime / worker / capabilities
crates/runtime/src/lib.rs
crates/runtime/src/jobs.rs
crates/runtime/src/queue_registry.rs                # all-seam registration/readiness equality
crates/runtime/src/maintenance_housekeeping.rs       # audited activation-epoch purge/cutover lane
crates/runtime/src/live_recovery.rs                   # fenced dirty-manifest/orphan recovery lane
crates/runtime/tests/queue_registry_closure.rs       # static/rendered/Redis positive+negative closure
crates/runtime/tests/system_lane_fencing.rs          # gate/epoch/CAS/concurrency/privilege barriers
crates/worker/src/main.rs
crates/worker/src/consume.rs

# first-launch immutable delivery and cutover artifacts
deploy/images.lock.json                             # per-platform runtime_deployable/build_base sets
deploy/queue-registry.toml                          # sole queue/task authority + three launch modes
deploy/first-launch/migration-manifest.toml         # exact ordered versions/names/checksums
deploy/first-launch/catalog-row-allowlist.toml      # exact post-migration catalog + seed rows
deploy/health/mode-aware-probe.sh                   # startup/liveness/readiness contract
deploy/Dockerfile.rust
deploy/Dockerfile.docreader
docker-compose.yml                                    # root include-only delegator; mandatory final Compose input
deploy/docker-compose.yml                             # included final service definition
deploy/.env.example
deploy/README.md
deploy/first-launch/README.md                       # audited runbook + launch-state checklist
deploy/post-launch/rollout-policy.md                # non-destructive releases/additive migrations only
scripts/first_launch_cutover.sh                     # two-person suspend/reopen/reset/final-readiness/exposure CAS tool
scripts/first_launch_smoke.sh                       # isolated-socket unrouted smoke + cleanup/disable
scripts/mark_first_production_request.sh            # production-ingress marker contract/test harness
scripts/test_mode_aware_probe.sh                    # maintenance/open orchestrator semantics
scripts/test_ingress_topology.sh                    # smoke socket isolation/direct-access/header negatives
scripts/verify_deploy_locks.sh                      # target-aware runtime/build, config-images multiset, Compose-input and cross-platform union closure
scripts/verify_queue_registry_closure.sh            # producer→Redis bidirectional equality
.github/workflows/ci.yml

# first-launch final fixtures/evidence
testdata/oxana/bid-v1-convert-envelope.json
testdata/oxana/bid-v1-target-envelope.json
testdata/oxana/bid-v1-match-route-envelope.json
testdata/oxana/system-maintenance-housekeep-v1-envelope.json
testdata/oxana/system-live-recovery-v1-envelope.json
testdata/evals/*

# later roadmap implementation only（not delivered for first launch unless signed intended state enables its lane）
crates/index/src/lib.rs
crates/enrichment/src/chat.rs
crates/enrichment/src/lib.rs
crates/search/src/lib.rs
crates/search/src/answer.rs
crates/obs/src/*
```

同一删除清单是 first-launch deliverable：删除/重命名上述 migration 文件，并从 runner/bundle/catalog 中删除 `schema_flags`、`0013_bid_backfill`、0009 alias；从 `crates/runtime/src/jobs.rs`、`crates/domain/src/status.rs`、worker/api/bid/storage 中删除 old `BidConvertJob`、`BidExtractJob`、`BidSectionRetryJob`、`BidMatchOxanaJob`、旧 Bid task constants/mappings、direct extraction writers、old matching matcher/store/serializer/handler、unknown→default fallback、historical baseline/compat seed code与相应 fixtures/tests。删除应在拥有 replacement 的 phase 完成：1A 清 migration compatibility，1C 删除 extraction paths，1D 删除 matching paths；final CI 用 negative grep、catalog allow-list与全 seam registration tests证明不存在。

PR 按 phase review，允许 1D 内按“0011 schema→0012 schema/artifacts→runtime/API/UI→old path deletion”做可执行 commits；所有中间 PR 只 merge + CI，不部署。delivery review 必须同时看 migrations、runtime readers、Dockerfiles、根 include-only `docker-compose.yml` 与 `deploy/docker-compose.yml` 的 exact input closure、env/README、locks、CI、runbook/tool、smoke 与删除清单，不能把它们留为运维后补。`web/package.json`/`web/package-lock.json` 及 `services/docreader/pyproject.toml`/`uv.lock` 只在各自 dependency graph 实际变化时进入 PR；无依赖变化不得为“closure”制造无关 churn。

---

## 11. Acceptance matrix

标注 `first launch` 的行全部是首次开放 barrier；标注 `later` 的行默认不阻塞首次上线，只有 signed intended feature state 显式启用对应 lane 时才提升为本次 launch barrier。未标注的 publication/matching rows 亦属于 first launch。

| 领域 | 必测场景 |
|---|---|
| Publication identity | full/document/retry递增generation；target_id重投；双generation CAS；project-wide冻结target set |
| Exact partial race | S1 commit → retry schedule generation+1 → old target superseded；status、count、partial flag、S1 current visibility、run aggregation全部精确断言；反向interleaving old commit失败 |
| Lease/recovery | run/target row token+heartbeat；active partial unique index；stale reclaim；old token；lease lost provider cancellation；不声称project full lease |
| Coverage/domain | Clause或deterministic non-requirement；invalid/uncovered failed保旧；confirmed/rejected；cleanup；candidate无route |
| Aggregates | superseded+published count；failed+partial；published candidates/state而非status聚合；worst quality/degraded/bounded reasons；空库 modern API contract |
| Schema | named expression index或NULLS NOT DISTINCT；composite target/run/document FKs/triggers；Clause仅clause disposition；FK delete/retention |
| Snapshot/feature | opaque typed bounded rows；credential/customer-content拒绝；modern seed幂等；retention公式；cache miss；promotion audit/kill switch prerequisites |
| Canonical queue registry（first launch） | 1B仅完整schema/data declarations+implemented subset static closure且无matching handler/Redis registration；1D后producer constants/mappings↔enqueue call-sites/manifests↔registry↔handlers↔rendered subscriptions↔Redis双向exact equality；三种launch_mode+signed intended state；missing/extra/fallback拒绝；disabled不能enqueue/claim且无active registration；Redis注册前零key/jobs |
| Matching routing | Technical unit只替换/clear/skip该unit；Commercial empty只清commercial；并发routes各自发布且不宣称跨route原子；同watermark route/route、route/mutation interleavings用barrier证明无lost update，stale commit对artifact/projection/job均零write |
| Dirty manifest / stale commit | 每个matching-relevant mutation在同一project锁/事务递增单调`mutation_watermark`、置dirty并立即把全部current route projection标non-current；scheduler精确捕获watermark、推进适用generation、写manifest和完整jobs的`expected_mutation_watermark`后才原子设置`scheduled_watermark`/clear；暂停N route→mutation→在N+1 schedule前resume必须CAS 0且API/booklet隐藏旧projection，随后只有watermark+snapshots全匹配的jobs可发布；同时覆盖scheduler/mutation两种锁顺序 |
| Matching artifact（first launch） | 0012 append-only requirement/candidate/report；current projection可重建；pick immutable snapshot；job/generation keys；old matcher/store/handler/dual-write不存在 |
| Typed candidate/value（first launch） | product/version identity、coverage/group/route metadata；stable grouping/order；business_value source/range；half-even 6位；typed not_scored；无calibrated_value；signed score/verifier launch mode由final path生成 |
| API/UI/booklet（first launch） | immutable machine evidence与human assessment分离；Part4 confirmed commercial hit；Part5仅confirmed commercial must miss/review且文案区分；non-must在workflow但不在4/5；pick readback不随projection变化 |
| Runtime/retry（first launch） | profile parsing API/worker/eval；startup/liveness vs readiness；mode-aware Compose probe；maintenance ready=false不restart/rollback；intended-state capabilities；seed RNG/sleeper；max_retries=0；terminal failure queue success；receipt crash/redelivery |
| System lanes（first launch） | maintenance housekeeping只在独立audited activation epoch+maintenance注册/claim，open inactive；live recovery有独立identity/protocol/snapshot，只在open fenced重建dirty manifest/恢复orphan；gate/generation/owner CAS、bounded concurrency、重复投递；DB权限禁止live purge/control-plane mutation |
| Image supply chain（first launch） | 每platform target-aware rendered service/target/inspect identity exact=runtime subset并保留API/worker同digest独立identity；`config --images` canonical digest-reference multiset按每service/target一次且保留multiplicity；Dockerfile stage/provenance exact=build subset；runtime/build entry identity无交集，各platform subset与union、cross-platform union exact=complete lock；根Compose delegator/input closure exact且non-final override拒绝；missing/extra/duplicate/wrong-platform/mismatch/tag-only拒绝 |
| Offline bootstrap（first launch） | 双人确认；reset dispatch按state精确分支且open_unrouted先suspend，exposure/revoked/live hard-stop；destructive empty volumes；pre-migration无user table/domain row；exact manifest且无0008_backfill/schema_flags/0013；post-seed catalog+rows双向allow-list；Redis注册前零keys/jobs；single final binary；maintenance startup/live=true、ready=false且不重启 |
| Launch state/expose（first launch） | security-definer-only DML；open_unrouted↔maintenance_unrouted按cutover/launch state/gate epoch/intended hash/route断开/双签CAS，marker后永久禁用且postlaunch另走non-destructive policy；`record_final_readiness`与`commit_traffic_exposure`都exact验证cutover/gate/migration/image/registry/intended/topology/smoke hash及fresh route-attestation ID+issued/expires/TTL，mismatch/expiry/reuse/后续变化拒绝；readiness TTL≤5m且单次消费；exposure+revocation在connect前原子CAS；首请求四项写入同事务；并发/crash/failure/privilege tests |
| Ingress trust boundary（first launch） | smoke独立socket仅audited runner可达且永不标production；cleanup后关闭；production listener第一middleware不可绕过marker；headers不能spoof；external direct service access被topology/firewall拒绝；first request的timestamp+evidence ref+append-only event/audit+live原子提交失败时auth/validation/business calls全为零 |
| Eval（first launch） | extraction + final matching + signed intended state中enabled lanes的security/load/capability gates必过；95%或approved level、cluster bootstrap、slice/margins/false-clean/p95/cost；inconclusive不promote |
| Eval（later） | trace runtime/retrieval/graph/multimodal/OCR/Grounded Answer各自gate不阻塞first launch，除非signed intended state显式启用对应lane |
| Trace schema（first launch） | 0011 typed/bounded additive schema先于0012；nullable matching trace refs；无content/credential/digest；不要求propagation/recorder runtime |
| Answer auth/order（later） | authenticated-global，无ACL/403 claim；invalid/missing不泄漏；no-current在capability/provider前；candidate/trace unavailable |
| Graph/OCR（later） | PG+memory同document/version/enabled scope与stable chunk order；PG no-fake-row、Bid fixtures、configured failure class |
| Trace runtime（later） | untrusted correlation only；server-derived scope；post-commit；retention；operator-only/disabled query |
| Migration/deploy（first launch） | exact `1..8,10,11,12` manifest（0008为命名清晰的Bid constraint）、repeat零变化、concurrent、失败原子、checksum/name/order/missing/extra/unknown fail closed；删除0008_backfill/schema_flags/0013/frozen-0009/compat branches；cutover窗口locks及根delegator→final Compose input closure exact |

Race tests用DB barriers停在claim后、candidate commit后、publication lock后、Section commit后、schedule head commit后和finish前，不用概率sleep。

---

## 12. Definition of Done

### 12.1 First production launch DoD（blocking checklist）

只有以下全部签署才能首次开放 traffic：

- [ ] Phase 0、1A、1B、2、1C、1D 全部完成；从未部署 intermediate binary，final artifact/topology 同时包含 complete extraction publication 与 complete final Bid Matching；
- [ ] 0010 的 indexes/FKs/triggers/aggregates/snapshots/feature audit/kill switch/maintenance gate及`production_launch_state`/events/security-definer procedures完整；full/document/retry无direct writer，统一target/candidate/publisher、double-generation CAS与正确partial/superseded aggregates；
- [ ] additive schema-only 0011 已在0012前执行并ledger登记；0012 manifests、immutable requirement/report/candidate/pick artifacts、projections与retention constraints完整；trace propagation runtime明确不在此checklist；
- [ ] final `MatchingRequest`/`MatchingReport`/route-fenced store、dirty manifest、Technical/Commercial routes、score/verifier launch policy、snapshots、`bid-matching-v1` envelope/claim/handler完整；matching-relevant mutation在同一project锁/事务递增单调`mutation_watermark`、置dirty并立即隐藏current projections，scheduler仅在exact watermark的manifest+完整jobs（均含`expected_mutation_watermark`）落库后更新`scheduled_watermark`/clear，route commit CAS claim/route/project/generation/snapshot IDs/双watermark/dirty；old matcher/store/DTO/serializer/task/handler、wrapper、dual-write、fallback全部删除；
- [ ] deterministic DB barriers 证明：暂停generation-N route commit，mutation提交后且scheduler N+1前旧commit对artifact/projection/job零写、旧projection在API/UI/booklet均non-current/hidden；下一manifest只允许expected watermark与冻结snapshot IDs全匹配的jobs发布；scheduler/mutation两种锁顺序及Technical/Commercial route/route、route/mutation interleavings全绿且不声称跨route原子替换；
- [ ] API/UI/workflow/booklet把 machine verifier evidence与human assessment分离；matching mutation到对应route成功替换之间不fallback旧projection，Part4/Part5、non-must visibility、immutable pick readback与business_value/typed score/half-even规则全绿；
- [ ] `deploy/queue-registry.toml`列出表4.2全部且仅这些entries并支持三种launch_mode；1B只做完整declarations/implemented subset static closure且不注册matching/Redis，1D所有final handlers后producer constants/mappings、enqueue call-sites/manifests、registry、handlers、rendered subscriptions、Redis registrations双向exact；signed intended state启用final Bid/live recovery且可声明关闭graph/multimodal等lane；missing/extra/fallback拒绝，disabled enqueue/claim失败；
- [ ] maintenance housekeeping有独立audited activation epoch且open时inactive；final `system:live-recovery:v1`有独立registry identity/protocol/snapshots，open时以gate/generation/owner CAS和bounded concurrency恢复dirty manifest/orphan，角色机械禁止purge/control-plane mutation；
- [ ] `deploy/images.lock.json`按每platform显式分runtime/deployable与build-base；target-aware rendered service/target/inspect exact等于runtime subset并保留API/worker同digest独立identity，`config --images`另以每service/target一次且保留multiplicity的canonical digest-reference multiset exact比较；Dockerfile stage/provenance exact等于build subset；runtime/build entry identities无交集、各subset exact，per-platform及cross-platform verified union exact等于complete lock；tag不能充当evidence；
- [ ] production profile/no-stub、minimal roles/protocol、seeded retry/receipt recovery、maintenance transaction checks、mode-aware API/worker startup/liveness/readiness probes、intended-state capability expansion、security/load/capacity tests通过；maintenance not-ready不触发restart/rollback且不存在bootstrap静态`/ready` Compose healthcheck；
- [ ] CI包含migrations、publication CAS、launch-state精确reset dispatch、完整final-readiness tuple/exposure/first-request原子性的并发与故障races、system-lane fencing、queue closure、target-aware image/config-images multiset/runtime-build/per-platform+cross-platform union closure、negative stale-path grep、final envelopes、API/UI/booklet及ingress smoke tests；delivery包含全部受影响root/crate `Cargo.toml`与`Cargo.lock`、migration/catalog manifests、mode-aware probe、Dockerfiles、根include-only `docker-compose.yml`+`deploy/docker-compose.yml` final input closure、env/README、双人runbook/tool、隔离socket unrouted smoke及post-launch rollout policy；web package manifests与docreader pyproject/lock仅在对应依赖变化时提交；
- [ ] `migrations/0008_backfill.sql`删除，actual Bid constraint原子重命名为ledgered `0008_bid_extract_running_constraints.sql`；source/bundle/catalog/rows中无`schema_flags`、`0013_bid_backfill`、0009 alias；`frozen-0009`和compat/history branches删除；
- [ ] destructive bootstrap双人确认；migration前无user tables/domain rows，fresh exact manifest、repeat/concurrent/tamper/name/order/missing/extra通过，post-seed catalog与rows双向精确等于allow-list；Redis注册前零keys/jobs，active registry符合intended modes；
- [ ] closed maintenance gate内建立approved intended feature snapshot，且启用final Bid conversion/extraction/matching、`system:live-recovery:v1`与signed score/verifier/recovery policies；对enabled lanes全部queues/capabilities预检，对declared-disabled lanes验证不能enqueue/claim，maintenance housekeeping无activation epoch时inactive；
- [ ] 只启动single final binary set；maintenance下API/worker startup/live=true且ready=false但不restart/rollback；审计maintenance→open后只通过仅smoke-runner可达的独立socket跑mutation/claim/publish/Technical+Commercial matching/API/UI/booklet smoke，production marker保持空；cleanup后关闭smoke endpoint并证明direct service不可达，随后紧邻exposure重跑API+worker readiness；
- [ ] extraction、matching与intended state所需evaluation/security/load artifacts签署，`inconclusive`不promote；later trace runtime/retrieval/graph/multimodal/OCR/Grounded Answer artifacts除非在signed intended state显式启用，否则不在first-launch evidence中冒充prerequisite；
- [ ] open_unrouted失败恢复只能在route断开、三marker NULL时以cutover/launch state/gate epoch/intended hash/evidence/双签CAS到maintenance_unrouted并按规则reopen；offline reset dispatcher仅对`preflight|maintenance_unrouted`直调，`open_unrouted`先CAS suspend，`reset_in_progress`只续同token，exposure/revoked/live永久hard-stop且不存在any-singleton reset；postlaunch maintenance使用独立non-destructive policy；
- [ ] routing tool只消费由`record_final_readiness`和`commit_traffic_exposure`都exact验证的 `cutover_id/gate_epoch/migration_head/image_lock_hash/registry_hash/intended_state_hash/topology_hash/smoke_cleanup_evidence_hash/route-disconnected attestation ID+issued_at+expires_at/TTL` tuple、TTL≤5分钟且未消费的fresh API+worker readiness；任一mismatch/expiry/reuse及topology/smoke/route变化拒绝，并在连接route前CAS同时写exposure+reset revocation；production listener第一middleware在auth/validation/business前以同一事务写first-request timestamp、evidence ref、append-only event/audit和live，失败时全部handler calls为零，header不可绕过/伪装smoke；
- [ ] one-shot exact digests/head/registry/intended state/topology只冻结到本cutover的exposure+first-request evidence；其后仅按已评审post-launch rollout policy做non-destructive releases/additive migrations，destructive reset authority永久revoked。

### 12.2 Later roadmap DoD（non-blocking for first launch）

以下各项只在对应phase完成时验收，默认未完成不影响12.1；若 signed intended feature state 显式启用对应 lane，则其完整实现、依赖与signed gate自动成为12.1 blocker：

- [ ] Phase 3 trace propagation/redactor/post-commit recorder/retention/operator audit在0011 schema上完成；incoming trace不授权且不保存content/credentials；
- [ ] Phase 4 retrieval/graph/OCR的scope、stable chunk、no-fake-row/provider failure tests与各自signed shadow gates完成；
- [ ] Phase 5 Grounded Answer按authenticated-global与deterministic precedence实施，no-current零provider call，无unsupported ACL/403 claim，并经独立signed gate promotion；
- [ ] Phase 6 automated cohort registry/promotion UI如选择实施，通过独立验收；基础feature store/snapshots/audit/kill switch不属于延期项。

## 13. Deferred / 不做

- 不做WeKnora/ReAct核心写路径迁移；
- 不在本计划创建workspace/project/product ownership ACL或403契约；stronger knowledge authorization另做domain migration；
- 不开放candidate/trace end-user endpoints；
- 不保存raw prompt/response/content/evidence/credential或content-derived raw digest；
- 不做graph relation reasoning；
- 不把RRF、相似度、human meet当semantic support；
- 不在signed gate前production activate任何 verifier/Answer/rerank/score policy；首次上线必须有已签署的 matching verifier/score launch mode，未获批的 score-v2、Answer、rerank保持关闭；
- automated cohort registry和promotion UI可延期，但feature store/snapshots/audit/kill switch不可延期；
- 不在 1C 后保留 extraction direct writer，不在 1D 后保留任何 temporary v0/old matching seam；不以 Serde unknown fields、shutdown timeout 或 process 停机本身充当安全边界；
- 不为首次上线实现历史数据、队列或 volume 的升级/转换路径；上线后策略另案设计。
