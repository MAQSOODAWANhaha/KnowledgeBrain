Implemented the authorized local contract closure for the bid-platform solution plan.

## 修改文件

- `plans/bid-platform-complete-solution.md`：从 1010 行修订为 1045 行，仅收尾既有①～⑥目标方案的局部契约。
- 未修改代码、`PRODUCT.md`、`docs/bid-platform-domain.md` 或其他产品/领域文件。

## 逐项修复映射

### 1. ProceduralRequirementRouterV1 review override

- 将原始 Router 状态与生效结果拆开：
  - `classified` 原始 kind 非空、review reason 为空；未 override 时 effective 等于 router kind。
  - `review` 只要求 router kind 为空、review reason 非空；未 override 时 effective 为空并派生 missing。
  - 任一 override 要求 `override_to == effective_requirement_kind`，完整 actor/reason/time；review 来源的 `override_from` 可空。
- 原始 classification status/router result/review reason 永不改写。
- SubmissionGate 和 resolution 只读取 current effective kind；effective 为空禁止 resolution 并阻断 PDF。
- PR0、PR6 和强制测试同步加入完整状态矩阵。

### 2. Pick/BidShot 与正式渲染资产

- 定义项目级 `PickSetV1` 和 `BidShotSetV1` revision/digest，分别固定 item 字段、排序、进入/离开/替换 mutation seam。
- 增加 immutable、content-addressed `bid_booklet_render_asset_artifacts`：冻结 object ref、SHA-256、byte length、media type、display name 和 typed placement metadata。
- `PartDependencyV1` 新增 `pick_set`、`bid_shot_set`；明确 part 2、3、`6:implementation_plan` 的实际依赖。
- `SubmissionManifestInputV1.render_assets` 冻结正式图片资产清单；v1 BidShot 只能进入 part `3`。
- renderer 只能调用 `read_manifest_render_asset`，逐项回验 digest/length/media；禁止查询 live `bid_shots`、live `object_key` 或替代路径。
- 增加 shot 新增/替换/删除/placement stale、asset 缺失/摘要错误、live shot 删除替换、shot/export barrier 和 retention/ACL 门禁。

### 3. PartDependency/Manifest canonical identity

- 增加 `FactIdentityV1` 与项目级 `fact_set_sha256`；事实 accept/set/clear 与 `fact_revision` 同事务重建 canonical digest。
- FactIdentityV1 固定字段、NULL、Decimal、UUID、UTC 时间和 digest seam，不再使用 revision-only facts identity。
- PartDependency 中 company/submission profile 分别使用各自 revision/content hash，删除未定义的聚合 profile hash语义。
- 定义严格 `GateIssueV1`：固定五个键、code/entity kind allowlist、entity/field/reason NULL 规则、UTF-8 bounds、每数组 512 项上限和稳定排序；本地化文案不进入 canonical payload。
- SubmissionGate 分别冻结 company/submission profile hash。
- Manifest 保存完整 PartDependency、GateIssue、Pick/Shot identities、render assets，并由唯一 storage seam 生成 canonical bytes/hash。

### 4. Matching heartbeat/staging TTL

- PR1 matching worker 在 claim 后周期调用 `heartbeat_claim_and_extend_staging`，安全间隔为 `min(claim_lease/3, staging_ttl/3)` 且最长 5 分钟。
- 同一事务先验证现有 heartbeat claim identity，再延长该 claim 未 consumed staging TTL。
- heartbeat false、lease/CAS loss 或连续失败立即停止 stage/commit；final commit 再验最后 heartbeat/claim。
- cleanup、heartbeat、finalize 使用兼容锁序，不能删除刚续期或正在 finalize 的 set。
- PR1 和测试加入跨初始 TTL、续期成功、claim loss、heartbeat/cleanup/finalize barrier。

### 5. SourceSpanV2 validator 职责

- JSON-only shape validator 仅验证 version、字段、类型、有界整数和 offset 次序。
- 明确禁止 shape validator 声称能验证外部 bytes 的 UTF-8 字符边界。
- 唯一 publication scope verifier 调用 `read_frozen_converted_source`，负责 UTF-8 解码/边界、quote slice、digest/length 及 document/generation/section scope。
- PR3 removed seam、exit gate 和测试同步更新。

### 6. Actor identity 编码

- `system_identity` 明确保存裸 allowlisted name，例如 `end-expired:v1`。
- canonical `actor_identity` 明确为 `system:<system_identity>`，例如 `system:end-expired:v1`。
- `kb_actor_identity_v1(actor_kind, actor_id, system_identity)` 的 user/api_key/system 输入输出和反向 CHECK 固定；含 `system:` 前缀的 system_identity 为负例。
- audit、idempotency、manifest 和 worker receipt 统一使用 canonical actor identity。

### 7. RequiredPartSet/Manifest bounds

- 普通 technical unit 最大 256；formal part 总数最大 266。
- `SubmissionManifestInputV1` canonical bytes 最大 32 MiB，独立于 Markdown 每 part 2 MiB/总计 16 MiB。
- render asset 最大 2048 项、单项 20 MiB、对象 bytes 合计 256 MiB。
- GateIssue 数量和 render metadata 均有界；大量空 content parts 的 identity/JSON 开销进入测试。

### 8. PR2 pure fresh-schema runner

- 若 PR2 实际创建 0014 schema slice，明确要求同 PR 更新/运行 PR1 创建的 pure fresh-schema runner、catalog/ACL matrix。
- §11.1、PR2 exit gate、Fresh DB test matrix 都加入条件式门禁。
- 继续排除历史升级、旧 binary、down migration、runtime-completion/image/topology。

### 9. family→kind ownership

- family→kind smoke 修改仍唯一归 PR3。
- PR7 只作 PR3 seam 的回归断言，并扩展 RequiredPartSet/PDF/Playwright 全链，不重复 ownership。

### 10. PR/测试/完成定义

- PR0 增加 FactIdentityV1、GateIssueV1、PickSetV1/BidShotSetV1 契约。
- PR1 增加 heartbeat/staging 长任务输出与测试。
- PR3 增加 SourceSpan shape/scope 职责分离。
- PR4 增加 FactIdentityV1 和 canonical actor fixtures。
- PR6 增加 pick/shot identities、immutable render assets、GateIssue、manifest bounds和正式 renderer seam。
- DB constraints、ACL、强制测试和完成定义全部同步。

## 命令与退出码

1. `git diff --check -- plans/bid-platform-complete-solution.md`
   - exit 0。
2. `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`
   - native exit 1（untracked 新文件存在差异的预期状态）；diagnostics 0 bytes；包装 whitespace 检查 exit 0。
3. 第一版 Python contract/table checker
   - exit 1；checker 期待错误的中文子串，文档已有等价精确契约。只修正 checker，不修改该契约。
4. 最终 Python 11 组关键契约断言 + Markdown table consistency
   - exit 0；13 个表格一致，1045 行。
5. targeted stale/contradiction scan
   - exit 0；0 matches。
6. scoped `git status --short` + `git diff --cached --name-only`
   - exit 0；只显示 `?? plans/bid-platform-complete-solution.md`；无 staged files。
7. `wc -l` / `sha256sum`
   - exit 0；1045 行；SHA-256 `b7e0d9f6bbf7892a93ed7d88b480a03c4c3bd07f51ddbe87cd26d282fe1a2143`。

## 验证证据

- 普通及 no-index whitespace 检查无 diagnostics。
- 11 组最终 contract assertions 覆盖 review/effective override、pick/shot/render assets、FactIdentity、GateIssue、matching heartbeat、SourceSpan scope、actor 格式、manifest bounds、PR2 runner、PR3/PR7 ownership 和 asset race tests。
- 13 个 Markdown 表格未转义 pipe 数一致。
- stale scan 未发现旧 review NULL 不变量、旧 system suffix 示例、PR7 首次创建 fresh runner、TODO/TBD/Open Questions。
- scoped status 证明未触碰 PRODUCT 或领域文档；无 staged/commit/PR。

## 意外发现

- 方案文件仍为 untracked，普通 `git diff --check -- <path>` 不检查正文，因此继续使用 no-index whitespace 和直接内容断言。
- 首次自定义 checker 因要求与正文不同的中文词序而失败；正文契约无缺陷，修正 checker 后通过。
- 当前 BidShot 是 live mutable object key；为了兑现正式 PDF 可重放，方案必须新增 immutable render artifact，而不能只增加一个 shot revision。

## 未完成事项 / residual risks

- 本次仅修方案，SQL/Rust/API/Web/CI 实现均尚未开始。
- `PRODUCT.md` 与领域文档仍待 PR0 对齐。
- Pick/BidShot canonical set、renderer asset seam、heartbeat staging 和 pure fresh runner 需要后续实现 PR 的 PostgreSQL/Rust race tests 验证。
- schema 编号可随实施时 head 顺延，但 slice ownership 不得变化。

## Commit / PR 状态

- 未创建 commit。
- 未创建 PR。
- 无 staged files。

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Only plans/bid-platform-complete-solution.md was revised. The ten authorized local contract groups were closed without changing the approved ①～⑥, single-company, manual-CNY, 0010-facts or clean-slate boundaries."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "Final whitespace checks, 11 contract assertions, 13-table consistency validation, targeted stale scan, scoped status, SHA-256 and no-staged-files evidence all passed and are recorded above."
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
      "command": "git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md (wrapper accepts expected native exit 1 and requires empty diagnostics)",
      "result": "passed",
      "summary": "Native exit 1 was expected for the untracked file; diagnostics were empty and wrapper exited 0."
    },
    {
      "command": "initial Python contract/table checker",
      "result": "failed",
      "summary": "Exited 1 because the checker expected a different Chinese phrase order; the document already contained the required invariant. The checker was corrected."
    },
    {
      "command": "final Python 11-contract assertions and Markdown table checker",
      "result": "passed",
      "summary": "Validated 11 contract groups, 13 consistent tables and 1045 lines."
    },
    {
      "command": "targeted stale/contradiction rg scan",
      "result": "passed",
      "summary": "No stale review invariant, actor encoding, runner ownership or placeholder wording remained."
    },
    {
      "command": "scoped git status and staged-files check",
      "result": "passed",
      "summary": "Only the plan is shown as untracked; no staged files."
    }
  ],
  "validationOutput": [
    "whitespace ordinary=0 noindex_native=1 diagnostics_bytes=0 wrapper=0",
    "contract_assertions=11 tables=13 lines=1045",
    "stale_scan_matches=0",
    "scoped status: ?? plans/bid-platform-complete-solution.md",
    "no staged files",
    "sha256=b7e0d9f6bbf7892a93ed7d88b480a03c4c3bd07f51ddbe87cd26d282fe1a2143"
  ],
  "residualRisks": [
    "All SQL/Rust/API/Web/CI behavior remains to be implemented in PR0-PR7.",
    "The newly specified pick/shot identities, immutable render assets, heartbeat staging and pure fresh runner require independent implementation tests."
  ],
  "noStagedFiles": true,
  "diffSummary": "Closed the remaining plan contracts for procedural review overrides, pick/shot and render-asset freezing, canonical fact/profile/gate/manifest identities, matching heartbeat staging, SourceSpan validator scope, actor identity encoding, bounded formal manifests, conditional PR2 fresh-schema gates and PR3/PR7 ownership.",
  "reviewFindings": [
    "Worker self-review found no remaining blocker in the authorized cleanup list; one fresh reviewer round remains required."
  ],
  "manualNotes": "No code, product/domain truth, staged files, commit or PR was changed."
}
```
