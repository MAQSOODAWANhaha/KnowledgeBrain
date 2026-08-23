Implemented the approved round-1 contract fixes for the bid-platform solution plan.

## 修改文件

- `plans/bid-platform-complete-solution.md`：从 1088 行修订为 1195 行，仅收敛既有①～⑥目标方案的实现契约。
- 未修改代码、`PRODUCT.md`、`docs/bid-platform-domain.md` 或其他文件。

## 修复映射

### 1. Procedural classification 历史、current 与 provenance

- 新增 immutable `ProceduralSegmentSetV1` history，按 `(project_id, clause_id, revision)` 保存，并以 partial unique 保证每个 clause 至多一个 current segment set。
- 将原始 Router 结果拆为 immutable `router_result_status=classified|review`，生命周期拆为 `lifecycle_status=current|superseded`。
- classification 历史允许复用同一 stable segment key；只对 current 行建立 partial unique。revision/ID 保留历史唯一性。
- `superseded_by_classification_id` 使用同 project/clause/segment composite self-FK；定义 successor 必须更高 revision、禁止自引用/环，终止但无 successor 只允许 clause unconfirm/delete/离开 procedural。
- deferred verifier 证明 current segment 恰一 current classification、旧 segment 无 current classification、classification 绑定 current per-clause segment-set revision/hash。
- unconfirm→reconfirm、离开 procedural→回归、classification version 变化均创建新 revision，不复活历史行。
- extracted clause PATCH text 后强制切换为 `manual_after_edit`，按当前 stored UTF-8 text 重分段；current source span 清空，但 immutable extracted origin artifact/SourceSpan 永久保留供 UI/audit，禁止继续宣称 edited text 逐字回源。
- manual/manual_after_edit 使用不含 classification version 的 deterministic segment key，允许同 segment 重分类而保持 stable identity。
- compliance decision 同样分离 resolution 与 lifecycle，允许保留 decision 历史并只约束每 classification 一个 current decision。

### 2. Per-clause 与 project-level procedural identities

- `ProceduralSegmentSetV1` 固定 canonical keys、segment fields、排序和 provenance NULL 矩阵。
- classification `segment_set_revision/sha256` 只绑定所属 clause，不再使用模糊的 project procedural set identity。
- project-level procedural classification digest 明确聚合所有 current segment classifications，避免一个 clause 修改使其它 clause classification 假过期。
- PartDependency 拆分 `kind_router_version` 与 `procedural_router_version`，按 part 显式 required/null；template version 独立 current。

### 3. MatchingReportV1 canonical contract

- 补全顶层 exact keys，以及 route、coverage、score、RequirementDecisionV1、candidate、candidate group、source artifact identity 的所有 nested exact keys、顺序、枚举、NULL、Decimal/UUID/digest格式、排序与 bounds。
- candidate 只引用 `evidence_v1_sha256`，source artifacts 单独冻结 immutable scalar identity，不重复内嵌 chunk bytes。
- `bid_match_report_artifacts.content_sha256` 仅由 `build_matching_report_v1` 生成；deferred verifier 逐字段证明 payload 与 report/decision/candidate/group/source rows 完全一致且同 report scope。
- PartDependency 的 matching report hash 精确引用该列；跨 Rust/SQL fixtures 覆盖 empty、select、review/reject、多 candidate、大 groups、NULL AI 和错误键序/NULL/digest。

### 4. StageRouteBatchV1 / CommitRouteV2 / lease / idempotency

- claim transaction 冻结 bounded `claim_lease_ms` 与 `lease_policy_generation` 到 job attempt/claim receipt；heartbeat、stage、commit、cleanup、reaper 只读同一 DB 值，删除自由 `stale_secs` seam。
- `StageRouteBatchV1` 定义 exact wire schema、actor identity、operation、idempotency key、payload hash、collection kind 和 typed items；覆盖 source/candidate/evidence/decision/candidate groups/reason codes 所有变长集合。
- 同 batch key/hash 返回首次 receipt，不同 hash 稳定 mismatch。
- `CommitRouteV2` 固定 compact header，包括 report ID、fixed report header、每种 collection totals、bytes totals 和 expected report hash；不携带变长集合。
- commit 成功响应丢失后按 receipt 重放；不同 payload mismatch。
- `bid_idempotency_results` 与 actor identity基础 schema 前移到 PR1/0013，避免 matching stage/commit 依赖未来 PR4；PR4 只复用并增加 domain audit/fact operations。
- heartbeat、stage、commit 使用 DB-time lease/TTL；cleanup 固定 project→job→set 锁序并锁后重验；reaper使用相同 frozen lease。
- system operation矩阵补齐 stage batch、route commit、reaper/cleanup；领域 receipt 与全局幂等 row 同事务互证。只有 heartbeat/lease renew 豁免。

### 5. BidShot pointer、placement、manifest asset FK

- 采用预分配 shot/artifact UUID：首次 INSERT shot 时直接写非空 pointer/placement，随后插 artifact，依赖 INITIALLY DEFERRED composite FK；不存在 NOT NULL 中间状态。
- `current_placement_ordinal` 是真实 shot 列；`UNIQUE(project_id,current_placement_ordinal)` 直接落在关系表。
- artifact 提供 `(id,project_id,source_shot_id,placement_ordinal)` composite parent key，pointer FK 同时证明 artifact/shot/project/placement一致。
- `build_render_placement_v1` 与 deferred verifier 证明 metadata JSON ordinal/caption/width_hint 等于标量列，hash来自规定 canonical bytes。
- 新增 normalized `bid_submission_manifest_render_assets`，通过 composite FK ON DELETE RESTRICT 引用 immutable artifact；manifest payload与关系行同事务写入并逐项互证，renderer不能只信 JSON。

### 6. Object registry 与引用感知删除

- 新增 `bid_object_registry` available→deleting→deleted 状态机和 durable delete outbox。
- 所有 document/shot/attachment/artifact/manifest 引用创建与 retention 使用同一 object registry row/advisory digest lock。
- 删除先持久化 deleting/tombstone/outbox，deleting 后禁止新增引用；物理删除成功才完成 deleted，失败可幂等重试。
- raw `drop_blob`、`delete_object`、`release_object_ref` 及所有间接物理删除入口均降为 retention 模块私有；API/普通 worker 无权限绕过。

### 7. Profiles、versions、fresh runner 与实施矩阵

- `CompanyProfileV1`、`SubmissionProfileV1` 分别固定完整 canonical keys、identity、字段范围和 NULL 规则；submission profile row仍是唯一权威。
- FactIdentity project-create 继续要求真实 `expires_at` 进入 revision=0 digest；PickSet继续只读 current-visible projection且无 ordinal。
- `scripts/bid_fresh_schema_acceptance.sh` 固定为自包含入口：自行创建/等待/trap清理PG16 cluster、roles、DB与env；不依赖 workflow 前置。
- PR1 required job固定 `bid-fresh-schema`，签收记录check name、target SHA、ruleset/required aggregation和run URL。
- 同步更新0013/0016/0018 schema slices、DB约束、ACL、PR0～PR7 outputs/removed seams/exit gates、强制测试和完成定义。
- 保持 clean-slate、无Org/多租户、无历史迁移/deployment completion的已批准边界。

## 命令与退出码

1. `git diff --check -- plans/bid-platform-complete-solution.md`
   - exit 0。
2. `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`
   - native exit 1（untracked新文件存在diff的预期状态）；diagnostics 0 bytes；包装检查通过。
3. 初版 Python contract/table checker
   - exit 1；stale检查错误地把 `procedural_router_version, template_version` 的子串当成旧单一router字段。只修正checker，未因此修改正文契约。
4. 最终 Python 26组关键契约断言 + Markdown table consistency
   - exit 0；26组contracts、13个一致表格、1195行。
5. targeted stale scan
   - exit 0；0 matches。
6. scoped status / staged check / line count / SHA-256
   - exit 0；仅方案文件仍为untracked，无staged files；1195行；SHA-256 `4ede04a2aa130b679f7a809ab7e9fef536262a1f7cfd1aa097095931a23b73f7`。

## 验证证据

- ordinary/no-index whitespace均无diagnostic。
- 26组断言覆盖router-result/lifecycle、partial-current history、manual_after_edit、ProceduralSegmentSet、MatchingReport nested schema、Stage/Commit operations与receipts、frozen lease、manifest asset relation、object registry/outbox、shot placement pointer、profiles、双router versions及fresh required job。
- 13个Markdown表格未转义pipe数一致。
- stale scan未发现旧classification enum、无条件segment unique、不可执行shot插入顺序、未定义MatchingReport摘要、旧profile/router占位或TODO/TBD。
- scoped status证明未修改PRODUCT/领域文档；无staged/commit/PR。

## 意外发现

- 将统一stage/commit幂等落入PR1后，发现原计划把idempotency基础表放在PR4会形成前向依赖；已将共享actor/idempotency基础表明确归入0013，PR4仅复用。
- shot NOT NULL pointer可以通过预分配UUID+deferred FK实现，不需要nullable过渡列。
- 第一版stale checker因substring过宽误报，正文契约无缺陷；收窄checker后最终验证通过。
- 方案文件仍是untracked，普通`git diff --check -- <path>`不会检查正文，因此保留no-index和直接内容验证。

## 未完成事项 / residual risks

- 本次仅修方案；SQL/Rust/API/Web/CI实现尚未开始。
- `PRODUCT.md`与领域文档仍待PR0对齐。
- MatchingReport exact-byte、stage/commit receipts、lease barriers、procedural history、manifest-asset FK和object deletion state machine仍需后续实现PR用PostgreSQL/Rust cross-layer fixtures验证。
- schema编号可随实施时head顺延，但slice ownership不得改变。

## Commit / PR状态

- 未创建commit。
- 未创建PR。
- 无staged files。

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Only plans/bid-platform-complete-solution.md was modified. All approved round-1 findings were incorporated across procedural lifecycle/provenance, exact MatchingReport and stage/commit contracts, lease fencing, BidShot/manifest assets, retention, profiles, versions, schema ownership and validation without widening product scope."
    }
  ],
  "changedFiles": [
    "plans/bid-platform-complete-solution.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "git diff --check -- plans/bid-platform-complete-solution.md",
      "result": "passed",
      "summary": "Exited 0."
    },
    {
      "command": "git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md",
      "result": "passed",
      "summary": "Native exit 1 was expected for the untracked file; diagnostics were empty."
    },
    {
      "command": "initial Python contract/table checker",
      "result": "failed",
      "summary": "The checker used an over-broad router-version substring; the document was already correct. The checker only was corrected."
    },
    {
      "command": "final Python contract assertions and Markdown table consistency checker",
      "result": "passed",
      "summary": "Validated 26 contract groups, 13 consistent tables and 1195 lines."
    },
    {
      "command": "targeted stale scan and scoped git status/no-staged check",
      "result": "passed",
      "summary": "No stale contract wording; only the untracked plan is in scoped status; no staged files."
    }
  ],
  "validationOutput": [
    "ordinary_diff_check=0",
    "noindex_native=1 diagnostics_bytes=0",
    "contract_assertions=26 tables=13 lines=1195",
    "stale_scan_matches=0",
    "sha256=4ede04a2aa130b679f7a809ab7e9fef536262a1f7cfd1aa097095931a23b73f7",
    "scoped status: ?? plans/bid-platform-complete-solution.md",
    "no staged files"
  ],
  "residualRisks": [
    "All SQL/Rust/API/Web/CI behavior remains to be implemented and verified in PR0-PR7.",
    "PRODUCT.md and docs/bid-platform-domain.md still require PR0 alignment.",
    "The plan remains untracked and uncommitted."
  ],
  "noStagedFiles": true,
  "diffSummary": "Expanded the plan from 1088 to 1195 lines to close immutable/current procedural history, manual-after-edit provenance, exact MatchingReport and staged commit wire contracts, frozen lease/idempotency, executable BidShot and manifest asset relations, object-registry deletion fencing, exact profiles/router versions, self-contained fresh schema acceptance and synchronized PR/test gates.",
  "reviewFindings": [
    "no blocker found in worker self-review; fresh independent review remains required"
  ],
  "manualNotes": "No code, PRODUCT.md, domain docs, staged files, commit or PR were changed."
}
```
