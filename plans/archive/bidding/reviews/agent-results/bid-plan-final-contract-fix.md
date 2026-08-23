Implemented the approved final contract convergence for the bid-platform solution plan.

## 修改文件

- `plans/bid-platform-complete-solution.md`：从 1045 行修订为 1088 行，仅收敛既有①～⑥目标方案的局部契约。
- 未修改代码、`PRODUCT.md`、`docs/bid-platform-domain.md` 或其他文件。

## 修复映射

### A. Procedural classification 基数

- classification 主身份改为 `(project_id, clause_id, stable_segment_key)`；同一 clause 可保存多条 segment classification。
- decision 绑定 classification + clause + segment + revision，不再按 clause 唯一。
- extracted segment 复用 SourceSpanV2 stable key；人工 clause 使用 `manual-procedural-segment-v1`，按原始 UTF-8 bytes 的句号/分号/换行/编号边界切分，保存 byte offsets、segment digest 和 deterministic key，不伪造 SourceSpan。
- 同一 clause 的不同 segment 分别进入 SubmissionGate；不可拆的同 segment 多 kind 才进入 review/missing。
- clause 编辑、重分段、取消确认、删除或离开 procedural 时，旧 classification/decision supersede，set revision/digest/stale 同事务更新。
- GateIssueV1 的程序 issue 用 classification UUID 精确定位 segment。

### B. PickSet current 语义

- PickSetV1 唯一成员来源固定为 `bid_pick_current_visible` 等价受检 projection。
- 删除没有仓库来源的 ordinal；item 固定为 unit/product/version/pick snapshot/source projection/source report 六个 UUID identity，并按 bytes 稳定排序。
- pick commit/retire 以及 matching projection/generation/watermark 引起的可见性变化均在 project lock 下重建 PickSet revision/digest并 stale 消费者。

### C. BidShot current asset mapping

- `bid_shots` 增加 transaction-end 非空 `current_render_asset_artifact_id`，使用同项目/shot composite deferred FK 指向 immutable render artifact；每个 live shot 恰有一个 current asset。
- shot 新增、替换、placement 修改和删除均在 project lock 内原子切换 pointer、重建 BidShotSetV1 并 stale part 3。
- artifact/manifest 使用 FK RESTRICT 保留历史资产；正式 renderer 只读 manifest-owned asset。
- 新增 `delete_unreferenced_object_v1` 引用感知 retention seam；raw `drop_blob/delete_object` 降为模块私有，API/worker 不得绕过。
- object ref、display name、byte length、media type及 placement metadata 的 exact schema/range/NULL/allowlist 已固定。

### D. Matching / staging / heartbeat

- 定义 MatchingReportV1 唯一 canonical serialization、stable item sorting、`bid_match_report_artifacts.content_sha256` 和 deferred cross-row verifier；PartDependency 精确引用该 hash。
- 每 `(job_id,claim_token,attempt,route_id)` 最多一个 active staging set；重试复用；第二 set 稳定拒绝。
- failed/expired 不续期；增加每 project 8 active sets、128 MiB chunks、32 MiB evidence、8192 rows 的并发总界限。
- heartbeat 用冻结 lease 和数据库时间校验；任一 SQL/连接/timeout 错误立即停止 stage/commit。
- CommitRouteV2 在刷新 heartbeat 或进入 committing 前强制检查原 `heartbeat_at >= clock_timestamp()-claim_lease` 和 `expires_at > clock_timestamp()`；过期 owner 即使 reaper 未运行也零可见写。
- deterministic validation failure、ownership expiry、transient DB error 的 staging 状态结果分别写死，删除模糊 retry 语义。

### E. Facts / profiles / version identity

- FactIdentityV1 project-create seam 按真实初值构建 revision=0 digest；create request 的 `expires_at` 必须进入 hash。
- submission profile row 自身 revision/content hash 是唯一权威；删除 project mirror 语义。
- `router_version` / `template_version` 固定 bounded version grammar、逐 part applicability、currentness 与 stale 规则。

### F. SourceSpanV2

- 增加 14-key exact JSON fixture、显式 page null、字符串/offset/page bounds和额外键拒绝。
- 新增 immutable converted section artifact，冻结 section parent boundary和digest；scope verifier证明 SourceSpan parent range 与该 section逐值一致。
- shape validator只验JSON形状/顺序/bounds；UTF-8边界、slice、digest、section provenance由唯一 frozen-source scope verifier证明。

### G. System idempotency / fresh runner

- 列出 extraction/matching claim、staging cleanup、manifest claim/succeed/fail/cancel、auto-expired end 的固定 domain-tagged key preimage。
- 只有 heartbeat/lease renew 允许 claim-token CAS 豁免幂等表。
- 固定 `scripts/bid_fresh_schema_acceptance.sh` 为规范入口；PR1创建并接入required CI与`shell -n`，后续schema PR和PR7只调用该路径。

### H. Schema / ACL / PR / tests

- 同步更新0013/0015/0016/0018 slices、DB hard constraints、retention/worker ACL、PR0～PR7 outputs/removed seams/exit gates、强制测试矩阵和完成定义。
- family→kind smoke 修改仍唯一归 PR3；PR7 仅做回归和全链扩展。
- 保持 clean-slate、无 Org、多租户、自动报价、历史迁移及 deployment completion 的既定边界。

## 命令与退出码

1. `git diff --check -- plans/bid-platform-complete-solution.md`
   - exit 0。
2. `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`
   - native exit 1（untracked 新文件有 diff 的预期状态）；diagnostics 0 bytes；wrapper exit 0。
3. 第一版最终 Python contract/table checker
   - exit 1；checker 错误要求带独立反引号的 `procedural_segment` 子串，正文已在 enum 中精确定义。只修 checker，未因该失败改正文。
4. 最终 Python 23 组关键契约断言 + Markdown table checker
   - exit 0；23 个contract groups、13个一致表格、1088行。
5. stale/placeholder contradiction scan
   - exit 0；0 matches。
6. scoped status、staged-files check
   - exit 0；只显示 `?? plans/bid-platform-complete-solution.md`；无 staged files。
7. `wc -l` / `sha256sum`
   - exit 0；1088行；SHA-256 `151a0a30d246c2cf48cdcbec7728d89f80566f962cf74eddbed77eaed6316c45`。

## 验证证据

- 普通/no-index whitespace检查无diagnostic。
- 23组断言覆盖segment cardinality/manual provenance、current-visible PickSet、current shot pointer/retention、MatchingReport hash、lease/TTL fence、单active staging与project bounds、project-create FactIdentity、profile authority、version currentness、SourceSpan exact fixture/section artifact、system key matrix、render bounds和规范fresh runner。
- 13个Markdown表格的未转义pipe数一致。
- stale scan未发现旧clause唯一分类、PickSet ordinal、可选fresh runner、连续heartbeat容忍、旧manual hash或TODO/TBD/Open Questions。
- scoped status证明未修改产品真源或领域文档；无staged/commit/PR。

## 意外发现

- 方案文件仍是untracked；普通`git diff --check -- <path>`不检查正文，因此继续使用no-index check与直接content assertions。
- 引入segment级classification后，GateIssue若仍只定位clause会失去唯一性；已同步改为classification UUID定位segment。
- 原manual key若包含整条clause hash，会使无关segment编辑导致全量identity变化；已改为segment digest + byte offsets + segmentation version。
- final staging失败语义原本仍有“failed或保持active”的模糊描述；已区分deterministic invalid、ownership expiry与transient transaction rollback。

## 未完成事项 / residual risks

- 本次只修改方案；SQL/Rust/API/Web/CI实现仍未开始。
- `PRODUCT.md`和领域文档仍待PR0对齐。
- 新契约需后续实现PR以PostgreSQL/Rust cross-layer fixtures和精确barrier tests验证。
- 方案文件仍为untracked，未commit。

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
      "evidence": "Only plans/bid-platform-complete-solution.md was modified. The approved segment-level procedural model, current-visible PickSet, BidShot current asset pointer/retention, MatchingReport hash, staging/lease fences, Fact/profile/version/SourceSpan/system-key contracts and PR/test wiring were all added without widening product scope."
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
      "summary": "Native exit 1 was expected; diagnostics were empty and wrapper exited 0."
    },
    {
      "command": "initial final Python contract/table checker",
      "result": "failed",
      "summary": "Checker expected an incorrectly quoted procedural_segment substring; document contract was already present. Checker only was corrected."
    },
    {
      "command": "final Python 23-contract assertions and Markdown table consistency checker",
      "result": "passed",
      "summary": "Validated 23 contract groups, 13 consistent tables and 1088 lines."
    },
    {
      "command": "targeted stale/placeholder scan",
      "result": "passed",
      "summary": "No stale cardinality, PickSet ordinal, optional runner, heartbeat tolerance or placeholder wording remained."
    },
    {
      "command": "scoped git status and staged-files check",
      "result": "passed",
      "summary": "Only the untracked plan is shown; no staged files."
    }
  ],
  "validationOutput": [
    "ordinary_diff_check_exit=0",
    "noindex_native_exit=1 diagnostics_bytes=0 wrapper_exit=0",
    "final contract assertions=23 tables=13 lines=1088",
    "stale_scan_exit=0 matches=0",
    "sha256=151a0a30d246c2cf48cdcbec7728d89f80566f962cf74eddbed77eaed6316c45",
    "scoped status: ?? plans/bid-platform-complete-solution.md",
    "no staged files"
  ],
  "residualRisks": [
    "All SQL/Rust/API/Web/CI behavior remains to be implemented and verified in PR0-PR7.",
    "PRODUCT.md and the domain document still require PR0 alignment.",
    "The plan remains untracked and uncommitted."
  ],
  "noStagedFiles": true,
  "diffSummary": "Closed the approved final local contracts for segment-level procedural classifications, authoritative current-visible picks, current immutable BidShot assets and retention, MatchingReport content identity, bounded staging/heartbeat fencing, factual/profile/version/source-span identities, system idempotency and canonical fresh-schema execution.",
  "reviewFindings": [
    "no blocker found in worker self-review; fresh independent reviewer gate remains required"
  ],
  "manualNotes": "No code, PRODUCT.md, domain docs, staged files, commit or PR were changed."
}
```
