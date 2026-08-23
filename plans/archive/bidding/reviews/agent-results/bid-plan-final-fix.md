Implemented the authorized final document-only closure for the bid-platform solution plan.

## 修改文件

- `plans/bid-platform-complete-solution.md`：从 956 行修订为 1010 行，精确闭合第3轮评审遗留的契约、PR归属和验收问题。
- 未修改代码、`PRODUCT.md`、`docs/bid-platform-domain.md` 或其他文件。

## 修复映射

### 1. SourceSpanV2 冻结来源

- 新增真实不可变 `bid_converted_source_artifacts` 契约：project/document/generation、content-addressed object ref、SHA-256、byte length、UTF-8 encoding、唯一键和删除保护。
- extraction target 冻结 `converted_source_artifact_id + sha256 + byte_length + expected_conversion_generation`，使用复合 FK `ON DELETE RESTRICT`。
- 定义唯一 `read_frozen_converted_source` seam；publication 只能通过该 seam 回源和重验 digest/length，禁止读取 live `bid_documents.markdown_ref`。
- SourceSpanV2 增加 converted artifact identity；`stable_span_key` 使用 `kb:source-span:v2` domain tag、typed tag、big-endian length framing和固定字段顺序，消除字符串连接歧义。
- PR3/schema/exit gate/强制测试同步加入 artifact、target FK、对象缺失/摘要损坏/live ref替换和删除保护矩阵。

### 2. system actor 幂等唯一性

- `bid_domain_audit` 增加非空 canonical `actor_identity`。
- 统一格式为 `user:<lowercase-uuid>`、`api_key:<lowercase-uuid>`、allowlisted `system:<bounded-name>`，并由唯一函数回验 actor kind/id/system identity。
- `bid_idempotency_results` 唯一键改为 `(actor_identity,operation,idempotency_key)`，不再依赖 nullable `actor_id` 的 PostgreSQL UNIQUE 语义。
- query、conflict wait、audit、manifest和worker receipt统一使用同一identity。
- 只对纯 heartbeat/lease renew 明确按 claim-token CAS 豁免幂等表；其它可重试system机械转换使用非空system identity。
- PR4和测试矩阵加入同UUID跨kind、system重复key、不同system identity和NULL绕过负例。

### 3. Evidence容量与外层4 MiB

- 选择受控staging方案，不简单提高现有单DTO限制。
- PR1定义 `StageRouteBatchV1` 和 hidden route commit staging：每批encoded JSON仍`<=4 MiB`，按claim/attempt/route/report nonce归属，带batch ordinal、累计上限、30分钟TTL和heartbeat续期。
- `CommitRouteV2`只提交compact staging identity/totals，保持外层`<=4 MiB`；final transaction整体验证累计source chunks `<=64 MiB`、Evidence canonical bytes `<=16 MiB`后原子promote。
- CAS loss/校验失败零visible artifact/domain/projection写；expired staging由与finalize互斥的受控cleanup清理。
- PR1 removed seams、exit gate和测试加入乱序、缺batch、超TTL、claim盗用、cleanup/finalize barrier和累计边界。

### 4. RequiredPartSetV1

- 普通technical units明确为distinct non-nil resolved UUID。
- nil sentinel只能条件生成`2:unsectioned`；明确禁止`2:00000000-0000-0000-0000-000000000000`。
- PR6 exit gate和测试加入集合相等、nil双计数和非法key负例。

### 5. QuoteSnapshotV1正式内容

- 明确title和notes属于正式`6:quote`：title trim后1..256 UTF-8 bytes，notes最多4096 bytes，空白规范化为NULL。
- snapshot表、canonical顶层键序、finalize和reopen逐字段复制全部包含title/notes；禁止从后续live revision回填。
- `pricing_set_sha256`及所有JSON digest固定为64位小写hex string；DB bytea仅在storage seam边界转换。
- PR5和强制fixture加入中文、转义、notes NULL/非NULL、reopen复制与hex digest。

### 6. ProceduralRequirementRouterV1

- 增加版本化二级分类表，固定优先级、guard、veto和review规则：bid bond、authorization support、seal sample、procedural support、confirmation。
- classification增加`classified|review`状态；多义或无法确定时kind为NULL、派生missing并阻断PDF，不允许默认放行。
- 固定golden覆盖授权委托书、投标函签字盖章、密封袋骑缝章、保证金缴纳回执、平台回执、普通投标递交支持附件及ISO资格材料veto。
- resolution仍不能自报kind；人工override保留原分类、目标、actor、reason/time并冻结进manifest。
- PR0/PR6、SubmissionGate和测试矩阵同步更新。

### 7. PartDependencyV1 与 SubmissionManifestInputV1

- 定义 `PartDependencyV1` 顶层键序、固定identities键序、适用/不适用NULL规则、revision/digest/artifact结构、64位hex格式及唯一storage seam。
- 每个part的`expected_dependency_hash`明确等于其PartDependencyV1 canonical UTF-8 payload SHA-256；API改为per-part形状。
- 定义 `SubmissionManifestInputV1`：固定project/actor/time/RequiredPartSet/SubmissionGate/quote/parts schema、part排序、gate排序、content UTF-8编码和每part/总量上限。
- manifest保存immutable payload/hash；renderer只读manifest-owned content。
- PR6和跨Rust/SQL fixture加入错误排序、NULL、digest、内容上限和CAS竞态。

### 8. fresh-schema runner归属

- 明确PR1首次创建并执行纯`bid_fresh_schema_acceptance`入口。
- PR3～PR6每个schema PR更新并运行；PR7只在目标commit最终复跑，不能首次创建。
- 历史迁移、旧binary、灰度、down migration、runtime-completion/image/topology继续排除。

### 9. Playwright和smoke归属

- 删除“首选”歧义，固定PR7新增Playwright `bid-browser-e2e` CI job。
- 明确branch ruleset/branch protection或required aggregation落地方式及check name/SHA/ruleset/check-suite/run URL证据。
- artifact上传必须`if: always()`；report和fixture digest缺失失败；绑定实际checkout SHA；保留screenshots及失败video/trace；使用`if-no-files-found: error`或等价强校验。
- family→kind脚本修改只归PR3；PR7只作回归断言并扩展RequiredPartSet/PDF hard gate/全链场景。

## 命令与退出码

1. `git diff --check -- plans/bid-platform-complete-solution.md`
   - exit 0。
2. `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`
   - native exit 1（untracked新文件存在diff的预期状态）；diagnostics 0 bytes；包装whitespace检查exit 0。
3. Python最终关键契约断言与Markdown table consistency checker
   - exit 0；20个最终契约断言、13个一致表格、1010行。
4. `rg` stale/矛盾扫描
   - exit 0；未发现旧nullable actor unique、`首选 Playwright`、PR7首次创建runner、smoke重复归属、假eligibility ID、旧契约名或TODO/TBD。
5. scoped `git status`、`git diff --cached --name-only`
   - exit 0；只显示`?? plans/bid-platform-complete-solution.md`；无staged files。
6. `wc -l` / `sha256sum`
   - exit 0；1010行；SHA-256 `795656de003c0b1cb56067f7c08da385a5f10b54709307c81550375f0b41c7f2`。

## 验证证据

- 普通和no-index whitespace检查均无diagnostic。
- 20项最终契约断言全部存在，包括converted artifact、唯一读取seam、framed span key、non-null actor identity、route staging、nil sentinel、正式title/notes、hex digest、procedural subtype router、两个canonical export contract、PR1 runner ownership和Playwright evidence规则。
- 13个Markdown表格未转义pipe数量一致。
- stale/矛盾扫描无匹配。
- scoped status证明未触碰产品真源或领域文档；无staged/commit/PR。

## 意外发现

- 方案文件仍是untracked，普通`git diff --check -- <path>`不会检查正文；因此继续使用`--no-index --check`和直接内容断言。
- Evidence累计容量无法与现有4 MiB单JSON门禁同时成立；最终采用bounded staging + compact final CAS，而不是把单DTO扩大到数十MiB。
- Procedural二级分类遇到不可拆的多种满足方式时，单一enum无法安全表达；增加`classification_status=review`并派生PDF missing，未引入新产品范围。

## 未完成事项 / residual risks

- 本次仅修改方案；SQL/Rust/API/Web/CI/Playwright及纯fresh-schema runner均尚未实现。
- `PRODUCT.md`和领域文档仍按PR0后续对齐。
- schema编号可随实施时head顺延，但每个slice的单PR归属不得改变。
- 需要独立第4轮reviewer对最终文档做acceptance review。

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
      "evidence": "Only plans/bid-platform-complete-solution.md was modified. All 11 authorized final closure groups were implemented without changing the approved ①～⑥, single-company, manual-CNY-quote, 0010-fenced-facts or clean-slate boundaries."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "Final whitespace checks, 20 contract assertions, 13-table consistency validation, stale scan, scoped status, SHA-256 and no-staged-files evidence all passed and are recorded above."
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
      "command": "git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md (wrapper accepts expected native diff exit 1 and requires empty diagnostics)",
      "result": "passed",
      "summary": "Native exit 1 was expected for an untracked new file; diagnostics were empty and wrapper exited 0."
    },
    {
      "command": "python3 final contract assertions and Markdown table consistency checker",
      "result": "passed",
      "summary": "Validated 20 final contracts, 13 consistent tables and 1010 lines."
    },
    {
      "command": "rg stale/contradiction scan",
      "result": "passed",
      "summary": "No superseded actor identity, Playwright, runner ownership, smoke ownership, fake eligibility or placeholder wording remained."
    },
    {
      "command": "git status --short -- scoped files && git diff --cached --name-only",
      "result": "passed",
      "summary": "Only the already-untracked plan is shown; no staged files."
    }
  ],
  "validationOutput": [
    "git_diff_check_exit=0",
    "noindex_native_exit=1 (expected new-file diff), diagnostics_bytes=0, wrapper_exit=0",
    "content assertions passed: 20 contracts; 13 tables; 1010 lines",
    "stale_scan_exit=0 matches=0",
    "scoped status: ?? plans/bid-platform-complete-solution.md",
    "no staged files",
    "sha256=795656de003c0b1cb56067f7c08da385a5f10b54709307c81550375f0b41c7f2"
  ],
  "residualRisks": [
    "All SQL/Rust/API/Web/CI behavior remains to be implemented in PR0-PR7.",
    "The final plan still requires the authorized independent round-4 acceptance review."
  ],
  "noStagedFiles": true,
  "diffSummary": "Expanded the plan from 956 to 1010 lines to close immutable converted-source provenance, non-null durable actor idempotency, bounded matching staging, nil/unsectioned semantics, formal quote title/notes and hex digests, deterministic procedural subtype routing, canonical part/manifest hashes, PR1 fresh-runner ownership, mandatory Playwright evidence and PR3-only smoke ownership.",
  "reviewFindings": [
    "Worker self-review found no remaining blocker in the authorized final-fix checklist; independent reviewer gate remains required."
  ],
  "manualNotes": "No code, product/domain truth, staged files, commit or PR was changed."
}
```
