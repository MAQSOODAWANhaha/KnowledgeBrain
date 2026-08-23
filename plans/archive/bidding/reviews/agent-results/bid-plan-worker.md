Implemented the approved bid-platform plan rewrite.

## 修改文件

- `plans/bid-platform-complete-solution.md`：完整重写为 777 行自洽目标方案。
- 未修改代码、`PRODUCT.md`、`docs/bid-platform-domain.md` 或其他产品/领域文件。

## 主要方案决策

- 明确最终正式范围为①～⑥，区分目标范围与分阶段实现，并将产品真源对齐设为 PR0。
- 采用 clean-slate fresh redeploy；排除历史安装迁移、旧 binary、灰度、down migration 及部署 completion 链，只保留空库 schema 顺序、约束、最小 ACL 和 fresh DB 验收。
- fact suggestion candidate 纳入 0010 target/generation/CAS/current publication；clause/fact/disposition 在 section publisher 事务内原子发布。
- 定义 current suggestion 的 pending/accepted/rejected/superseded、fact revision CAS、覆盖确认、幂等、actor audit、冲突和 stale 事务。
- PR1 重新基线为 0012 runtime/matching correctness closure，明确现有 scheduler/v1 worker 不重做；闭合 FROZEN_SCOPE、最终 decision 透传、多候选、无候选 review、商务读取和 EvidenceV1。
- EvidenceV1 冻结 document ID/display name/chunk/quote/offset/offset unit，不依赖 live file-name join。
- KindRouter 改为句级 routed segments、组合付款信号和技术语境 veto；支持同 segment 多 facts 及 fact+clause 共存，并列出混合 golden。
- 完整定义人工报价 draft/finalized/reopened、Decimal、currency、税含/税外、舍入、并发、ceiling、immutable snapshot/hash 和 reopen；系统建议不能自动成为正式价格。
- 定义 company/submission profile、submission attachments、⑥ part keys、stale 依赖图、submission manifest、Word warning 和正式 PDF hard gate。
- 保持单公司、无 Org、authenticated-global 边界，同时对 facts、quotes、profiles 和 exports 写 append-only audit。
- 将 `/api/v1/files`、LDAP/JWT、不可信文件解析和外部处理器作为独立 PR-S 前置安全基线，不修改核心领域架构。

## 命令与退出码

- `git diff --check -- plans/bid-platform-complete-solution.md`：exit 0。
- `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`：原生命令 exit 1（表示新文件有差异），无 whitespace diagnostics；包装校验将 0/1 视为正常并 exit 0。
- Python 内容断言与 Markdown table consistency 检查：最终 exit 0；验证 14 个必需决策、8 个表格、777 行。
- `grep -n '^##\|^###' ...` + stale placeholder 检查：exit 0；未发现 `同前`、`待定`、`TBD`、`TODO`、`Open Questions`、`公司四夹`、`四轮评审`。
- `git status --short -- plans/... PRODUCT.md docs/...`：exit 0；只显示方案文件 `??`，PRODUCT/领域文件无 scoped change。
- `git diff --cached --name-only`：exit 0，无输出，无 staged files。
- 一次早期自定义 table checker 因正则把普通字符误算为 pipe 而 exit 1；修正为逐字符统计 unescaped pipe 后最终检查通过，非文档缺陷。

## 验证证据

- `git diff --check` 无错误。
- 对 untracked 文件使用 `--no-index --check`，无空白错误输出。
- 内容断言确认正式①～⑥、产品真源前置、0010 fact fencing、0012 closure、人工报价状态、clean-slate、支付 veto、Evidence offsets、stale 图、安全基线、单公司和 currency pairing 均存在。
- 所有 Markdown 表格的未转义分隔符数量一致。
- 方案文件为 untracked，符合开始时状态；未 stage、未 commit。

## 意外发现

- 方案文件开始时就是 untracked，因此普通 `git diff --check -- <file>` 不会检查其正文；补用了 `git diff --no-index --check /dev/null <file>` 和直接内容断言。
- 独立检查时发现最初 Evidence 草案引入了仓库不存在的 `document_artifact_id`，已收敛为 EvidenceV1 自身冻结 display name/quote/offset，避免无必要新 artifact。
- 自查发现 ceiling 与 quote currency 可能不可比较，已补 budget/ceiling currency pairing、ISO 4217 allowlist 和 finalize 同币种门禁。

## 未完成事项

- 本次按要求未修改 `PRODUCT.md`、领域文档或任何代码；这些由后续 PR0/实施 PR 完成。
- 安全问题只记录为方案前置，尚未修复。
- 具体 SQL/Rust/API 实现及完整测试仍需后续工作和独立 reviewer 验收。

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
      "evidence": "Only plans/bid-platform-complete-solution.md was rewritten; scoped status shows no PRODUCT.md or domain-doc changes, and all approved decisions are represented."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "Whitespace checks, direct content assertions, Markdown table consistency checks, stale-placeholder checks, scoped git status, and no-staged-files evidence were recorded."
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
      "summary": "Exited 0 with no whitespace diagnostics."
    },
    {
      "command": "git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md (wrapper accepts expected diff status 1 and requires empty diagnostics)",
      "result": "passed",
      "summary": "The untracked file differs as expected; no whitespace diagnostics; wrapper exited 0."
    },
    {
      "command": "python3 inline required-decision and Markdown-table consistency assertions",
      "result": "passed",
      "summary": "Validated 14 required decisions and 8 Markdown tables across 777 lines."
    },
    {
      "command": "grep heading structure and stale placeholders",
      "result": "passed",
      "summary": "All major sections present; no stale placeholders or superseded phrases found."
    },
    {
      "command": "git status --short -- plan PRODUCT.md domain-doc && git diff --cached --name-only",
      "result": "passed",
      "summary": "Only the already-untracked plan is shown in scoped status; no staged files."
    }
  ],
  "validationOutput": [
    "content assertions passed: 14 required decisions, 8 consistent Markdown tables, 777 lines",
    "git diff --check exit=0",
    "untracked no-index whitespace check: no diagnostics",
    "stale placeholder scan: none",
    "scoped status: ?? plans/bid-platform-complete-solution.md",
    "staged files: none"
  ],
  "residualRisks": [
    "PRODUCT.md and docs/bid-platform-domain.md still describe ①～⑤ until the planned PR0 updates product truth.",
    "Security blockers are documented but intentionally not fixed in this plan-only change.",
    "SQL/Rust/API feasibility and acceptance tests remain for implementation and independent review."
  ],
  "noStagedFiles": true,
  "diffSummary": "Replaced the 331-line placeholder-heavy untracked plan with a 777-line decision-converged architecture covering formal ①～⑥, fenced facts, matching closure, manual quote snapshots, stale/audit/export gates, clean-slate schema/ACL, security baseline, rollout, and acceptance tests.",
  "reviewFindings": [
    "no blocker found in worker self-review; independent reviewer gate remains required"
  ],
  "manualNotes": "The plan file was already untracked at task start, so validation included direct file checks and no-index diff checks. No commit or PR was created."
}
```
