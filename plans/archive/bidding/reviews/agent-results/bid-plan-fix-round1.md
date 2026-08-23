Implemented the round-1 targeted revision of the bid-platform solution plan.

## 修改文件

- `plans/bid-platform-complete-solution.md`：从 777 行修订为 907 行，闭合 reviewer 指出的 implementation-readiness 契约。
- 未修改代码、`PRODUCT.md`、`docs/bid-platform-domain.md` 或其他文件。

## 逐项修复摘要

### A. Dependency identity / stale / export

- 在 `bid_projects` 定义独立于 matching watermark 的项目级 `pricing_revision`、`schedule_payment_revision`、`schedule_delivery_revision`、`evaluation_revision`、`procedural_revision` 及各自 canonical set SHA-256；定义 old/new membership、进入/离开/集合内变化、跨 kind 双 bump、finalize CAS、snapshot frozen identity 和 PDF 重比。
- 为 procedural decisions、validated attachments 定义项目级 revision+digest；补 upload/replace/delete/validate/confirm/reject stale edge 和 authorization/procedural part dependency。
- 为每个 booklet part 增加 `content_revision/content_sha256/edited_by/edit audit`；formal export 锁定并冻结 part revision/hash 与 Markdown bytes，render 不再读取 live Markdown。
- 定义 manifest `pending → rendering → succeeded|failed`、failed 同 manifest 重试、succeeded 只下载，以及 `end_project` 与 rendering/pending 的锁定规则。

### B. Fact publication / acceptance

- 定义 publisher/accept/reject 统一锁序：project → document extraction head → section publication state → suggestion → conflicts。
- accept/reject 使用锁住的 `(target, section candidate, generation)` identity + pending 条件更新；CAS loss 零领域写。
- 幂等改为 mutable precondition 之前预占 actor+operation+key+payload hash；并发 unique 冲突等待事务并重读，completed replay 直接返回首次结果。
- reject 明确不要求 fact revision，但必须 current+pending CAS、reason、durable actor/audit。
- 增加 `set_project_fact/clear_project_fact`，覆盖 typed validation、expected revision、override reason、quote eligibility、stale 和 audit 同事务。
- 分离 current pending view 与分页 history ledger。

### C. Evidence / matching / clause writes

- EvidenceV1 采用真实可实现的 immutable bounded source chunk artifact；frozen document/chunk IDs 不 FK 到可删除 live rows，artifact 对 matching eligibility artifact 使用 RESTRICT。
- 定义 UTF-8 byte 半开区间、字符边界、digest、eligibility、quote slice commit-time 校验，以及 Rust DTO、SQL validator、中文多字节和 live document 删除后重放测试。
- 定义 MatchDecisionV1 完整真值表与 `supported > unresolved > insufficient > contradicted > no-evidence` 聚合优先级。
- stable comparator 使用 frozen `(product_ordinal, retrieval_rank, candidate_identity_sha256, evidence_v1_sha256)`，完全重复 hit 先去重，不用随机 UUID tie-breaker。
- CommitRoute 原样接收 typed requirement decisions；DB 验证 select 指向同 report/requirement 的 supported/select/pass candidate。
- 明确升级已有 `bid_match_current_commercial_decisions`/storage query，不创建平行查询，并移除 live file-name 权威依赖。
- manual create 与 PATCH、publisher、Rust/API/Web/smoke 全部改为客户端只提交 kind、服务端派生 family。

### D. Quote / formal ⑥ contract

- QuoteSnapshotV1 锁定唯一 canonical UTF-8 JSON serialization：固定 object/line key order、NULL、Unicode escaping、UUID/time 和 Decimal string 格式；唯一 storage seam 生成 bytes/hash。
- v1 仅 CNY/scale=2；明确逐行舍入后求和、precision/overflow 上界、gross ceiling 同币种比较。
- quote aggregate 指针改为首个 draft 后恰一非空；finalize 清 draft/set active；ceiling/pricing 变化保留 active identity 但标 ineligible；`reopen(expected_snapshot_id)` 复制下一 draft 并清 active。
- 固定 company/submission profile 字段、singleton/project uniqueness 与 content hash。
- 动态 procedural decision 绑定 current clause revision/digest，并有 requirement kind、三态、附件兼容和 PDF allow/reject 矩阵。
- 附件引用同项目 validated immutable object artifact，包含 digest/size/media type/policy version/status；authorization support 纳入授权 part。
- profile/attachment/procedural mutation 均要求 expected revision、idempotency 和 durable actor；audit 支持 company-global scope。
- fact/quote/profile/procedural/booklet/export 明确禁止 Bootstrap actor。

### E. Router / SourceSpan / schema / PR acceptance

- Router 改为可执行 guard/veto 表；技术性能主谓优先于“到货后”条件从句，锁定 `CONDITIONAL_DELIVERY_CONTEXT` golden；保留 payment 组合信号和技术 veto。
- 新增 SourceSpanV2：parent boundary、precise subrange、UTF-8 offset unit、stable span key，clause/fact/disposition 共用 validator。
- quote 与 submission/booklet 拆为独立 clean-slate schema slices；fact publication 与 fact decision 也按 PR runtime seam 分离。
- 对每个 schema slice要求同 PR机械接线 embedded runner/schema head/fresh manifest/catalog/ACL tests；明确这只是 empty-database bootstrap wiring，不包含历史迁移或 deployment completion。
- 增加逐 PR prerequisites/output/removed seam/exit gate 矩阵。
- PR7 明确更新 smoke 的 family→kind 与完整 PDF gate；无 browser runner 时采用产品+QA 双签、步骤和 evidence artifact 的人工协议。
- PR-S 拆为仓库内 `PR-S-code` 自动测试和上线前 operator/governance checklist；DPA/数据驻留/凭据轮换不冒充领域代码测试。

## 命令与退出码

- `git diff --check -- plans/bid-platform-complete-solution.md`：exit 0。
- `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`：原生 exit 1（新文件有差异的预期状态），无 whitespace diagnostics；包装检查 exit 0。
- 最终 Python 25 组关键契约断言 + Markdown table consistency：exit 0；验证 12 个表、907 行。
- stale/contradiction scan（`同前|待定|TBD|TODO|Open Questions|pricing_watermark|0014_bid_quote_submission|...`）：exit 0，无匹配。
- heading scan：exit 0，主要章节/子章节存在且顺序完整。
- scoped status + staged check：exit 0；只显示 `?? plans/bid-platform-complete-solution.md`，无 staged files。
- `wc -l` / `sha256sum`：exit 0；907 行，SHA-256 `af8d29efe3d8515141b788696dae7a131cb64031d777745b6953a437d044aff6`。
- 一次早期自定义内容断言因 checker 期待“逐行已经舍入”而正文写作“每条已经舍入”导致 exit 1；调整 checker 为语义匹配后最终 exit 0，非文档缺陷。

## 验证证据

- 25 组必需契约全部存在：pricing/nonmatching identities、attachment/procedural identities、part content identity、manifest state、fact lock/idempotency/manual API、current/history、EvidenceV1、MatchDecisionV1、existing view upgrade、kind-only、QuoteSnapshotV1/CNY/pointers、SubmissionGateV1、validated attachments、durable actor、Router precedence、SourceSpanV2、schema split、PR matrix、安全拆分和 fresh bootstrap wiring。
- 12 个 Markdown 表格的未转义 pipe 数一致。
- no-index whitespace 检查无 diagnostics。
- stale placeholder/旧编号/旧 pricing watermark 扫描无匹配。
- scoped status证明未触碰 PRODUCT/领域文档；无 staged/commit/PR。

## 意外发现

- 方案文件仍是 untracked，普通 `git diff --check -- <path>` 不会检查其正文，因此继续使用 no-index 检查和直接内容断言。
- reviewer建议的 UUID 最终 tie-breaker若直接采用会跨执行不稳定；最终改成 canonical evidence hash 并要求完全重复 hit 去重。
- validated object 若是全局 artifact，异步 validation 会造成跨项目 set revision fan-out；最终把 validation artifact纳入project composite scope，使validation/stale在单项目锁内线性化。

## 未完成事项 / residual risks

- 本轮只修方案；`PRODUCT.md`、领域文档、SQL/Rust/API/Web/CI/smoke仍未实现，按PR0～PR7执行。
- schema数字是当前head后的计划名，若PR2无需DDL，实际编号可顺延，但每个文件的单PR归属不可改变。
- operator/governance checklist尚未执行；它是共享环境开放前的外部签署项。
- 没有运行cargo/npm/PostgreSQL测试，因为本次没有代码或schema实现。

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
      "evidence": "Only plans/bid-platform-complete-solution.md was revised. All 25 synthesized round-1 correction groups were incorporated without widening beyond the approved ①～⑥, 0010 facts, manual immutable quote, single-company, and clean-slate boundaries."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "Final no-index whitespace validation, 25 contract assertions, 12-table consistency validation, stale scan, heading scan, scoped status, SHA-256, and no-staged-files evidence all passed."
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
      "command": "python3 final 25-contract assertions and Markdown table consistency checker",
      "result": "passed",
      "summary": "Validated 25 contract groups, 12 consistent tables, and 907 lines; exited 0."
    },
    {
      "command": "rg stale/contradiction scan and heading scan",
      "result": "passed",
      "summary": "No stale placeholders, old pricing watermark, or superseded combined schema name; heading structure present; exited 0."
    },
    {
      "command": "git status --short -- scoped files && git diff --cached --name-only",
      "result": "passed",
      "summary": "Only the already-untracked plan is shown; no staged files; exited 0."
    },
    {
      "command": "preliminary custom phrase assertion",
      "result": "failed",
      "summary": "Exited 1 because the checker expected a different Chinese phrase for the same rounding rule; corrected checker passed, and no document defect was found."
    }
  ],
  "validationOutput": [
    "git_diff_check_exit=0",
    "noindex_native_exit=1 (expected new-file diff), noindex_whitespace_exit=0",
    "content assertions passed: 25 contract groups; 12 consistent tables; 907 lines",
    "stale_contradiction_scan_exit=0",
    "heading_scan_exit=0",
    "scoped status: ?? plans/bid-platform-complete-solution.md",
    "no_staged_files_exit=0",
    "sha256=af8d29efe3d8515141b788696dae7a131cb64031d777745b6953a437d044aff6"
  ],
  "residualRisks": [
    "Product/domain truth and all SQL/Rust/API/Web/CI implementation remain for PR0-PR7.",
    "Schema numbers may shift with the repository head, but each slice must retain single-PR ownership.",
    "Operator/governance evidence is not yet executed because this was a plan-only revision."
  ],
  "noStagedFiles": true,
  "diffSummary": "Revised the untracked solution plan to close round-1 blockers across dependency identity, fact linearization, immutable evidence, deterministic matching decisions, CNY quote snapshots, submission gates, attachment/procedural identities, editable part freezing, manifest races, source spans, schema ownership, PR acceptance, and security/governance separation.",
  "reviewFindings": [
    "Worker self-review found no remaining blocker in the requested correction list; independent reviewer gate remains required."
  ],
  "manualNotes": "No code, PRODUCT.md, domain docs, staged files, commit, or PR were changed."
}
```
