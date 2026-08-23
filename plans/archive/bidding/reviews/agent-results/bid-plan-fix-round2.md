Implemented the round-2 targeted revision of the bid-platform solution plan.

## 修改文件

- `plans/bid-platform-complete-solution.md`：从 907 行修订为 956 行，闭合第2轮评审指出的实现级歧义与验收旁路。
- 未修改代码、`PRODUCT.md`、`docs/bid-platform-domain.md` 或其他文件。

## 修复映射

### 1. EvidenceV1 与 matching decision

- 删除不存在的 `eligibility_snapshot_id` 概念；source chunk artifact 使用现有 `report_id` 与 `product_version_artifact_id` 两条真实 FK，并由 deferred verifier 通过 report route、`bid_match_route_product_versions` 和 candidate 三列身份证明同一冻结 scope。
- retriever 的 `FrozenRetrievedHitV1` 以及 worker→`CommitRoute` payload 在检索读取事务内携带完整 bounded `chunk_utf8`、冻结展示名、chunk digest 和相对 chunk 的 UTF-8 byte offsets；commit 禁止回读 live chunks/documents，覆盖 retrieve→commit 删除竞态。
- 明确 source chunk、展示名、item/quote、每 report artifact/chunk/evidence bytes 的具体上限，并与 frozen verifier policy 取更严格值。
- 定义 EvidenceV1 canonical bytes、稳定 item 排序和 `evidence_v1_sha256` 列。
- 统一命名为 `RequirementDecisionV1`，报告级为 `MatchingReportV1`；DB deferred verifier证明 selected candidate是全部 supported candidates按稳定四元组排序的第一条，并验证候选 support/decision/quality 真值映射及 scored business value 一致。

### 2. fact publication、决策与幂等

- 修复 PR3/PR4 循环：PR3/0015 一次创建 immutable fact candidate、完整基础 decision ledger、current-pending view与publisher pending/supersede语义；PR4/0016不 ALTER 0015 ledger，仅增加BidProject事实/revision、audit/idempotency和mutation函数/runtime/API。
- 幂等 identity 统一为 `(actor_kind, actor_id, operation, idempotency_key)`，明确 user/api_key 不同身份域即使 UUID 相同也不能互相命中。
- 增加独立 `ceiling_revision`：只有 ceiling value/currency/有无状态变化才 bump；accept 和人工 set/clear 使用同一规则。普通预算、开标、截止、有效期变化不迫使报价重新 finalize。

### 3. QuoteSnapshotV1

- snapshot冻结 `ceiling_revision`，全局 `fact_revision`只作finalize CAS/provenance；持续eligibility/PDF只比较current ceiling value/currency/revision与pricing revision/digest。
- 固定 `ceiling` 与 `no_ceiling_review` 的完整 nested JSON schema、键序、null规则、actor/reason/time与互斥 CHECK；跨层fixture覆盖有/无 ceiling 两分支。
- 明确payload/hash/totals/provenance不可变；只有SECURITY DEFINER函数可按单向eligibility图变化，并增加multiple-inputs-changed状态防止丢失多重失效原因。

### 4. 程序材料、附件、parts 与 manifest

- 将权威版本化 procedural requirement classification 与 compliance resolution 分离：Router产生原分类，受控人工override记录from/to/actor/reason/time；resolution不得提交requirement_kind。
- 附件 identity 拆成 authorization 与 procedural 两个per-part子集revision/digest，stale edge和dependency identity一致。
- 为profile/classification/decision/attachment定义CanonicalSetV1字段、排序、NULL/时间/string规则和domain-tagged空集合hash，避免Rust/SQL漂移。
- evaluation memo正式归属part 1并进入revision/digest/stale。
- 定义 `RequiredPartSetV1`：固定1/3/4/5/全部6子part、每个current technical unit的2:*，并在有未归段confirmed technical时强制生成且仅生成一个`2:unsectioned`；formal export拒绝缺失、重复、未知、额外key。
- manifest状态机加入pending→cancelled与审计；手工`end_project`和worker `end_expired_projects`统一调用同一project-lock DB函数，rendering阻止/延后结束、pending先受控取消。
- 增加可审计allowlisted system identity，仅用于worker render/end等机械状态转换；面向用户正式mutation仍只允许durable user/api_key并拒绝Bootstrap。

### 5. Router、SourceSpan与实施验收

- qualification枚举材料guard提前并优先于泛化提交义务；procedural只匹配明确程序对象，新增“须提交ISO证书”“许可证复印件须盖章”重叠golden。
- SourceSpanV2四个offset统一相对同一份target冻结converted UTF-8 bytes；document generation明确为`expected_conversion_generation`；JSON validator只验shape/边界，publication scope verifier证明quote slice。
- 新增独立纯空库`bid_fresh_schema_acceptance`入口，覆盖ledger/checksum/catalog/ACL/API-worker role矩阵；不读取runtime-completion、不启动运行拓扑。现有compose first-launch脚本不再冒充schema唯一证据。
- PR7明确新增最小Playwright runner与required CI job，固定fixture并通过`actions/upload-artifact`上传report/screenshots/video/trace；人工补充证据若需要，必须经真实workflow_dispatch下载、hash/双签校验链。
- 修正PR prerequisites：PR1依赖PR0+当前0012；PR4依赖PR3+PR-S-code；publisher措辞改为Router产出kind、统一domain/storage seam派生并持久化kind+family。
- PR7以目标commit全部required GitHub checks green及check-suite/run URL为证据；本地命令不替代CI。

## 命令与退出码

- `git diff --check -- plans/bid-platform-complete-solution.md`：exit 0。
- `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`：native exit 1（新文件存在差异的预期状态），无 whitespace diagnostics；包装检查 exit 0。
- 最终 Python 26组关键契约断言 + Markdown table consistency：exit 0；验证12个表、956行。
- stale/contradiction scan：最终 exit 0，无 `MatchDecisionV1`、假eligibility ID、旧幂等identity、publisher kind-only、旧attachment-set等残留。
- scoped `git status` + staged check：exit 0；只显示方案文件 `??`，无 staged files。
- `wc -l` / `sha256sum`：exit 0；956行，SHA-256 `9f8390586d0d840e8710521c88cc2492b34011dba43ec1a468029a042d1e6555`。

## 验证证据

- 26组关键契约全部存在，包括Evidence真实身份/冻结输入/边界/canonical hash、DB comparator、fact PR切分、manifest cancel与自动结束、ceiling-only eligibility、actor-kind幂等、per-part附件identity、RequiredPartSet、Quote nested schema、程序分类分离、Router重叠规则、SourceSpan基准、纯fresh bootstrap、真实browser runner、PR prerequisites及operator外部门禁。
- 12个Markdown表格的未转义pipe数量一致。
- 普通和no-index whitespace检查均无diagnostic。
- scoped status证明未触碰PRODUCT/领域文档；无staged/commit/PR。

## 意外发现

- 当前`bid_match_product_version_artifacts`并没有`report_id`复合父键；因此没有虚构composite FK，而是使用两条现有真实FK，再由deferred verifier连接report route与`bid_match_route_product_versions`证明report/product关系。
- quote可能先后因ceiling和pricing变化失效；仅两个单原因enum会覆盖并丢失历史原因，因此增加`ineligible_multiple_inputs_changed`单向状态。
- 方案文件仍为untracked，普通`git diff --check -- <path>`不检查正文，所以继续使用no-index检查和直接内容断言。
- 一次初始stale scan把正文中正确的“runtime-completion不列为交付项”当作命中；收窄为真正废弃短语后最终无匹配，非文档缺陷。

## 未完成事项 / residual risks

- 本轮只修订方案；`PRODUCT.md`、领域文档、SQL/Rust/API/Web/CI/smoke尚未实现，按PR0～PR7执行。
- `bid_fresh_schema_acceptance`与Playwright runner目前只是方案中确定的PR输出，仓库现状尚不存在这些脚本/job。
- schema数字按实施时head可能顺延，但每个slice的单PR归属和依赖不得改变。
- operator/governance checklist尚未执行；它只在共享环境开放前签署，不属于本地领域/schema PR completion。

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
      "evidence": "Only plans/bid-platform-complete-solution.md was revised; all 17 synthesized round-2 correction groups were incorporated while preserving formal ①～⑥, 0010 facts, manual CNY quote, single-company and clean-slate boundaries."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "No-index whitespace validation, 26 contract assertions, 12-table consistency validation, stale scan, scoped status, SHA-256 and no-staged-files evidence all passed."
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
      "command": "git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md (wrapper accepts native new-file diff status 1 and requires empty diagnostics)",
      "result": "passed",
      "summary": "Native exit 1 was expected; no whitespace diagnostics; wrapper exited 0."
    },
    {
      "command": "python3 final 26-contract assertions and Markdown table consistency checker",
      "result": "passed",
      "summary": "Validated 26 contract groups, 12 consistent tables and 956 lines; exited 0."
    },
    {
      "command": "rg stale/contradiction scan",
      "result": "passed",
      "summary": "No superseded contract names, fake eligibility identity, old idempotency scope or stale attachment/publisher wording; exited 0."
    },
    {
      "command": "git status --short -- scoped files && git diff --cached --name-only",
      "result": "passed",
      "summary": "Only the already-untracked plan is shown; no staged files; exited 0."
    }
  ],
  "validationOutput": [
    "git diff --check exit=0",
    "noindex native exit=1 (expected new-file diff), no whitespace diagnostics",
    "content assertions passed: 26 contract checks; 12 consistent tables; 956 lines",
    "stale_contradiction_scan: none",
    "scoped status: ?? plans/bid-platform-complete-solution.md",
    "no staged files",
    "sha256=9f8390586d0d840e8710521c88cc2492b34011dba43ec1a468029a042d1e6555"
  ],
  "residualRisks": [
    "Product/domain truth and all SQL/Rust/API/Web/CI implementation remain for PR0-PR7.",
    "The pure fresh-schema acceptance script and Playwright CI runner are specified but not yet implemented.",
    "Schema numbers may shift with repository head, but slice ownership and dependencies must remain stable.",
    "Operator/governance evidence remains an external shared-environment gate."
  ],
  "noStagedFiles": true,
  "diffSummary": "Revised the plan to close round-2 blockers across real EvidenceV1 frozen inputs and identities, deterministic DB matching verification, fact PR ownership, ceiling-specific quote eligibility, canonical quote/procedural/attachment contracts, authoritative procedural classification, exact required parts, manifest cancellation and unified expiration ending, pure fresh bootstrap evidence, real browser CI and corrected PR prerequisites.",
  "reviewFindings": [
    "Worker self-review found no remaining blocker in the requested correction list; independent round-3 reviewer gate remains required."
  ],
  "manualNotes": "No code, PRODUCT.md, domain docs, staged files, commit or PR were changed."
}
```
