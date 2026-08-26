# 两路匹配与不可变发布

本文定义 `MatchingPublication` 的最终 V1。它消费 confirmed matching clauses 和知识库检索端口，发布可验证的 immutable report，并管理人工选择集。

## 1. 边界

Matching 拥有：

- project matching mutation watermark；
- frozen manifest、route 与 route membership；
- job/attempt/claim/lease；
- frozen retrieved source chunks、candidate、evidence、decision、report；
- current report projection；
- `RoutePickSetV1` 与 `ProjectPickSetV1`。

Matching 不拥有：

- 知识库 Workspace/Product/Document/index；
- clause kind/family 状态机；
- quote 或 submission part；
- 通用平台队列协议。

Open/Stage/Commit 是 Matching storage adapter 的内部发布机制。application service 接口只表达：

```text
schedule_dirty_project(project_identity)
claim_route(job_identity)
execute_route(frozen_route_scope)
publish_route(matching_report)
```

## 2. 两路知识检索

### 2.1 消费 KnowledgeRetrievalPort

端口 operation、request 和返回 DTO 只由 [`../../docs/knowledge-base/domain.md`](../../docs/knowledge-base/domain.md) 定义。Matching adapter 收到 `KnowledgeEvidenceBatchV1` 后，分别校验完整 `eligible_versions` 和有界 `hits`：

- route membership 只从 `eligible_versions` 建立，不从 hit 反推；
- hit 必须属于同 batch 的 eligible version，但 eligible version 不要求产生 hit；
- hit quota 只限制 `hits`，不能截断 eligible scope；
- 65 个 eligible version、0 hit 是合法输入，65 个 membership 全部冻结，各 requirement 产生 `NO_EVIDENCE` decision；
- product_line/company scope 分别进入对应 route，不能跨 workspace kind 混用。

每个 hit 随后映射为内部 `FrozenRetrievedHitV1`：

```text
document_id, source_chunk_id
product_id/product_version_id 或 company-library identity
frozen_document_display_name
chunk_utf8, chunk_sha256, chunk_byte_length
retrieval_rank, retrieval_raw_score
quote start/end UTF-8 byte offsets
retrieval_contract_version
```

映射不得补读 live Document/chunk，也不得把 BidProject/route 模型传回知识库。内部值随即进入 StageRouteBatch；live Document/chunk 后来被删除或改名，不影响已冻结报告。

### 2.2 route

| route | unit identity | 输入 |
| --- | --- | --- |
| technical unit | 真实非 nil UUID | 该 current technical unit 的 confirmed requirements |
| technical unsectioned | lowercase nil UUID | 未归入普通 unit 的 current technical requirements |
| commercial | `NULL` | 全项目 confirmed qualification/service requirements |

每个 manifest 冻结 project、generation、matching watermark、requirement set、完整 eligible product version/company scope、有限 frozen hits 和 route membership ordinal。

### 2.3 knowledge scope attestation

schedule 把端口快照交给知识库拥有的 `kb_knowledge_attest_matching_scope_v1` 合同。该合同是唯一允许核对 live Workspace/Product/ProductVersion/Document/chunk 的边界；招投标只冻结返回的 attestation ID/hash，并在 manifest deferred verifier 中以 ID/hash/同一 payload 调用 `kb_knowledge_verify_matching_scope_v1`。

因此招投标 schema/function 不直接 join 知识库业务表。attestation 失败时 schedule 全事务回滚；attestation 成功后 live 知识资料的修改或删除不改写已冻结的 manifest、report 或正式输出。

## 3. RequirementDecisionV1

每个 eligible requirement 恰有一条 typed decision。候选最终 support 聚合固定为：

| 候选集合 | final support | system decision | quality | reason | selected candidate |
| --- | --- | --- | --- | --- | --- |
| 任一 `supported` | `supported` | `select` | `pass` | `SUPPORTED` | 必须 |
| 无 supported，任一 `unresolved` | `unresolved` | `review` | `review` | `UNRESOLVED` | NULL |
| 仅 insufficient/contradicted 且有 insufficient | `insufficient` | `review` | `review` | `INSUFFICIENT` | NULL |
| 有候选且全 contradicted | `contradicted` | `reject` | `block` | `CONTRADICTED` | NULL |
| 无候选 | `insufficient` | `review` | `review` | `NO_EVIDENCE` | NULL |

优先级为：

```text
supported > unresolved > insufficient > contradicted > no-evidence
```

有 supported 时，recommended candidate 是下列冻结 tuple 的升序第一条：

```text
(route_product_ordinal,
 retrieval_rank,
 candidate_identity_sha256,
 evidence_v1_sha256)
```

- ordinal 必须来自该 route membership，不得误用 manifest 全局 ordinal；
- tuple 在同 report/requirement 内唯一；完全重复 hit 先去重；
- selected 指向 candidate artifact ID，不是 product/version ID；
- 其它 supported candidates 全部保留，允许用户选择 1..N。

business value 只能来自 selected supported candidate 的 typed frozen value；无值统一为 `not_scored/NO_EVIDENCE`。

## 4. MatchingReportV1

### 4.1 报告级聚合

report header 只能由 `RequirementDecisionV1[]` 确定性重算：

1. `coverage.total = coverage.eligible = decisions.length`；
2. supported/contradicted/insufficient/unresolved 是 final support 计数，和必须等于 total；
3. 任一 block -> report block；否则任一 review 或 total=0 -> review；只有 total>0 且全 pass -> pass；
4. `degraded = quality_status != pass`；
5. `reason_codes` 是固定 `FROZEN_SCOPE`、全部 decision reason，以及空 route 时 `EMPTY_ROUTE|SKIP_UNIT` 的排序去重并集；
6. `empty_disposition=clear_route|skip_unit|null` 只由冻结空 route 策略决定，不能覆盖质量算法。

worker 不能自由传一份不一致 header；Rust builder 与 DB deferred verifier 都要从 decision rows 重算。

### 4.2 canonical payload

顶层固定键序：

```text
schema_version,report_id,manifest_id,job_id,route_id,route,generation,
mutation_watermark,empty_disposition,coverage,quality_status,degraded,
reason_codes,score,requirement_decisions,candidates,candidate_groups,
source_artifacts,ai_run_id,ai_span_id
```

规则：

- UTF-8、无 BOM、无额外空白/额外键；
- nullable 键始终存在并显式 `null`；
- UUID 小写连字符，digest 64 位小写 hex；
- Decimal 固定 scale string，禁止指数、`+`、`-0`；
- arrays 使用规范 comparator 排序并有总量/字节上限；
- report content hash 只由唯一 storage builder 计算；
- verifier 证明 payload 与 report/decision/candidate/group/source relation rows 集合完全相等。

### 4.3 current projection

current report 必须同时满足：

- final commit 成功；
- report generation/watermark 等于 project current；
- route still current；
- 没有被更新 manifest 替代；
- project open（历史读取除外）。

页面只读 current projection；历史报告按 immutable artifact API 读取，不得用 live candidate rows重建。

## 5. EvidenceV1

### 5.1 source chunk artifact

每个被采用的知识库命中在 final report 中变为招投标拥有的 immutable source artifact：

```text
id, report_id, product_version_artifact_id
document_id, source_chunk_id
frozen_document_display_name
chunk_utf8, chunk_sha256, chunk_byte_length
created_at
```

`document_id/source_chunk_id` 是冻结 scalar，不 FK 到可删除 live rows。deferred verifier 仍证明 source artifact 的 product version 属于 report frozen route scope。

### 5.2 candidate evidence

```json
{
  "schema_version": 1,
  "items": [
    {
      "source_chunk_artifact_id": "<uuid>",
      "document_id": "<uuid>",
      "document_display_name": "冻结时文件名",
      "source_chunk_id": "<uuid>",
      "source_chunk_sha256": "<64 lowercase hex>",
      "quote": "连续原文",
      "start_offset": 12,
      "end_offset": 42,
      "offset_unit": "utf8_byte"
    }
  ]
}
```

items 按 `(source_chunk_artifact_id,start_offset,end_offset,quote bytes)` 排序。quote 必须逐字等于 frozen chunk 的 byte slice。candidate 保存 `evidence_v1_sha256`；报告只投影 source identity，不重复内嵌 chunk bytes。

任何 retrieve 后、freeze 前的 live re-read 都禁止。

## 6. Matching 内部发布协议

大报告不能放入单个 JSON commit 请求，因此 adapter 使用 Open -> Stage -> Commit；这三个 wire DTO 不向业务模块泄漏。

### 6.1 claim 与 lease

claim 成功时冻结：

```text
claim_token, attempt
claimed_at, heartbeat_at
claim_lease_ms, lease_policy_generation
```

同一 attempt 不可修改 lease。heartbeat、stage、commit、cleanup 和 reaper 都用 DB time 与冻结 lease；调用者不能自由传 `stale_secs`。

### 6.2 OpenStagingSetV1

Open 显式创建 active staging set，绑定：

```text
job_id, route_id, claim_token, attempt
manifest_id, project_id, generation, mutation_watermark
report_nonce, claim lease/policy
status, expires_at
```

每个 `(job,claim,attempt,route)` 最多一个 active set。同 key/payload 幂等重放首次 receipt；异 payload 返回稳定 mismatch。project 级 active sets、rows、chunk bytes 和 evidence bytes 有硬上限。

### 6.3 StageRouteBatchV1

每批只承载一种 collection：

```text
source_artifacts
candidates
evidences
requirement_decisions
candidate_groups
reason_codes
```

request 绑定 staging set、job/route/token/attempt、report nonce、batch ordinal、collection kind、canonical items 和 payload hash。单批 canonical JSON 有硬上限；`(staging_set_id,batch_ordinal)` 唯一，同 hash 重放首次 receipt，异 hash 冲突。

每次 batch 在 `project -> job -> staging set` 锁内检查 current claim、heartbeat freshness、TTL、scope、item schema 和累计配额。

### 6.4 heartbeat

worker 在 claim 后周期 heartbeat，间隔为：

```text
min(claim_lease_ms / 3, staging_ttl / 3, 5 minutes)
```

heartbeat 用 token CAS 更新 DB time，并延长该 claim 唯一 active staging TTL。SQL/连接/timeout、CAS loss、lease 过期或 set 非 active 都立即停止 stage/commit。

### 6.5 CommitRouteV2

Commit 只发送 fixed header、expected counts/bytes、expected batch count、report ID/nonce 和 expected report hash。所有变长集合都来自 staged rows。

单事务验证：

1. completed idempotency receipt 是否可直接重放；
2. project/job/route/claim/attempt/manifest/generation/watermark current；
3. staging active、未过期、nonce 一致；
4. batch ordinals 连续且 counts/bytes 完全相等；
5. source/evidence/candidate/decision/group FK 与 scope；
6. 从 decisions 重算 coverage/quality/degraded/reasons；
7. 生成 canonical MatchingReportV1 并匹配 expected hash；
8. promote staged rows 为 immutable artifacts；
9. 原子切 current report pointer、更新 job/route status、释放配额；
10. staging -> consumed，写 audit 与首次 receipt。

任何失败不发布 current report。旧单 JSON `CommitRoute` 路径必须删除。

### 6.6 cleanup/reaper

- active set TTL 到期 -> expired 并释放配额；
- claim lease 过期由 reaper 以冻结 policy 回收 attempt；
- failed/expired staged rows 可物理清理，但 immutable committed artifacts 不受影响；
- cleanup/reaper 使用 token/CAS 和 allowlisted system actor；
- terminal staging 不可续期或继续收 batch。

## 7. 人工选择

### 7.1 RoutePickSetV1

每个 route 的 current 选择集绑定：

```text
schema_version
project_id, route_id
source_report_artifact_id
report_generation, report_sha256
route_unit_id
revision
items[]
```

每个 item 至少冻结：

```text
requirement_artifact_id
candidate_artifact_id
product_id, product_version_id
source_report_artifact_id
unit_id
selected_by, selected_at
```

规则：

- item 必须指向该 current report/requirement 的 current visible supported candidate；
- 每个 requirement 可选 1..N，重复 candidate 去重；
- recommended 只是默认提示，不自动成为用户选择；
- report 替换后旧 RoutePickSet 保留历史但不 current；
- mutation 使用 expected revision + idempotency，并 stale 下游 parts。

### 7.2 ProjectPickSetV1

项目集是所有 current `RoutePickSetV1` items 的 canonical 并集，并冻结来源 route/report identity。它不是一张可独立随意编辑的“解决方案表”，也不能丢失 route provenance。

用途：总体产品方案、实施计划、manifest 与项目级产品列表。每次 route pick 变化同事务重算 project revision/digest。

### 7.3 unsectioned 精确规则

1. 找到唯一 current unsectioned technical report `R`；其 route kind=technical、unit ID 为 lowercase nil UUID。
2. 定义 `S = ProjectPickSetV1.items WHERE source_report_artifact_id = R.id`，并验证 `S` 逐项等于 `R` 对应的 current `RoutePickSetV1.items`。
3. 只验证 `S` 中每项 `unit_id=nil`，且它们只进入 part `2:unsectioned`。
4. 普通 unit items 必须带真实非 nil UUID并进入各自 `2:{unit}`。
5. `S` 可以为空；若 `R` 有 required supported selections，则 gate 按缺失选择处理。
6. 同一个 ProjectPickSetV1 可以同时含普通 unit 与 unsectioned items；禁止对整个集合做 nil 约束。

## 8. 读取视图

- technical current projection 展示全部 supported candidates，仅 comparator 第一项标 `recommended=true`；
- commercial current projection 按 requirement 展示 supported/review/reject/no-evidence 及证据，不做产品排名；
- 缺件 view 从 current decisions 派生，不自行重跑检索；
- booklet dependency 引用 report/pick content hash，不引用 live view 查询时间。

## 9. 专题验收

- empty、全 select、select+review、select+reject、review+reject 报告聚合；
- report canonical bytes 的 Rust/SQL exact fixture；
- source quote 多字节 offset、live 文档删除后历史重放；
- route ordinal 与 selected comparator 负例；
- Open/Stage/Commit 同 key replay、异 payload、lease loss、TTL、配额、batch gap；
- commit header 错配、report hash 错配、CAS loss 零 current 写；
- reaper/cleanup/terminal purge/counter release；
- technical 全 supported 可选择 1..N；
- 普通 unit + unsectioned 混合 ProjectPickSet 正例，以及错误 report/nil/part 映射负例；
- 最终代码中不存在旧 `CommitRoute`、live evidence fallback 或按 requirement 任取首个 candidate 的重建路径。
