# 草稿模板通道：大纲 → 按章填内容 → 草稿 DOCX

日期：2026-09-17。状态：产品主路径；按 T1–T5 实施。

**统一结论：** 日常自动出稿走本文。独立复核、32 项、同版三件套走 [agent-runtime-rig.md](agent-runtime-rig.md)，且必须是**另一次用户请求**。两条路径不得混用同一套发布门。草稿 DOCX 不能销终稿验收。

装包 Rechecker（rig §4.5）只服务终稿。extract-6 已证明：装包开了仍 416 回合。禁止用装包/加 `max_turns`/`draft_channel` 当小文件产品方案。

## 1. 目标

1. 先生成投标文件大纲（招标规定的组成、顺序、指定格式/附表）。
2. 再按大纲铺模板内容（固定文字、表格、签署位、投标留白）。这是产品重点。
3. 小文件必须在分钟级出草稿，而不是数小时。42KB 合成稿 416 回合仍完不成、106 页 Main 1604 回合约 4 小时，必须修掉。

40 分钟只是量级，不是 SLA。

## 1.1 Agent 解析流程：哪些步骤必要

从编译器要的输入往回推。现网 `compile()` 真正消费的是：冻结原文 + `Template` 区域（固定文字/留白/签章）+ 章节顺序。Fact/Rule/Requirement 全图、关系、disposition、global_checks、独立 Rechecker、编制 Agent 的 `put_section`，都不是草稿 DOCX 的输入。

| 步骤 | 现网 | 草稿要不要 | 谁做 |
| --- | --- | --- | --- |
| DocReader 冻结页/表/网格 | 有 | **必须** | 宿主，无模型 |
| 按 source_unit 抽 Fact/Rule/Requirement/Unresolved | Main 数百回合 | **不要** | — |
| 关系、disposition、五项 global_checks | Main | **不要** | — |
| 独立 Rechecker / repair | Reviewer 可再数百回合 | **不要** | 终稿另请求 |
| 编制 plan 全义务 + `put_section` | 编制 Agent | **不要** | 宿主用 `draft_plan`+Template 合成 Section |
| 编制 Rechecker | 有 | **不要** | 终稿 |
| 出件 Rechecker | 有 | **不要** | 终稿 |
| 大纲（组成/顺序） | 混在抽取里 | **要** | 宿主检索预装，模型 1～3 回合写树 |
| 章内区域（fixed vs blank vs 签章） | 混在 `put_record` | **要，这是唯一核心模型活** | 一章一回合 `put_chapter_template` |
| 引用/blank 盖标签/禁止编造 | 弱，靠 Reviewer | **要** | 宿主硬规则 §7.1 |
| 排版进 DOCX | 模型可调 compile | **要** | 宿主 `compile()`，模型无此工具 |

因此草稿 job 里只允许 **两类模型回合**：大纲、填章。没有分析 Main 抽取、没有 Reviewer、没有编制 Agent。现网整条「抽取→复核→编制→出件复核」对日常出模板不是必要链，是终稿链。

宿主合成稿（无编制 Agent）：

```
每个 filled 章 → Section{title, order, grounds, content:[Template{record_id}]}
presentation：标题取招标文件名或组成条款，默认正文样式
plan：一章一项，obligation_ref = 该章 Template
然后 compile()
```

`validate_draft_basis` 只检查：frozen digest、`review.draft=true`、`quality=needs_review`、`draft_plan` **非空**（到期后 pending 会变成 omitted(deadline)，仍算有大纲；**不**调用现网 `outline::ready`）、已填（Filled）章的 Template 仍在 records。不跑 `gaps` / Rechecker / disposition 覆盖。

现网 `compile()` → `Draft::validate_basis` 会无条件走官方 `validate_basis`，从而拒绝 `review.draft`。T4 必须改成：`review.draft` 时只走 `validate_draft_basis`，并允许宿主写入的默认 `presentation`（标题=招标文件名，`explanation`=`草稿默认版式`）。不改这一支就编不出 DOCX。

## 2. 现方案为什么解决不了

自动链仍是：每个冻结 SourceUnit 抽取 → 独立 Rechecker 再读一遍 → `validate_basis` → 编制再 plan 完全部义务 → `put_section`。

| 闸门 | 位置 | 效果 |
| --- | --- | --- |
| 分析必须独立复核才能返回 | `tender_analysis/agent.rs` `run()` + `review_complete` | 没有 Rechecker 就没有 AnalysisResult |
| 编制必须无 finding、无 gaps | `docx_composition/mod.rs` `validate_basis` | 草稿分析进不了编制 |
| SQL 发布要 source_review 全过 | `kb_bid_v2_publish_requirement_set_v4` | 未复核不能发布要求集 |
| UI 要分析 `succeeded` 才打开编制 | `AnalysisProgress.tsx` | 用户看不到模板 |
| 工作单位是页/表碎片 | `source_review::tasks()`、`main_dispatch` | 小文件也被切成几十个串行 LLM 会话 |
| 预装外的新读本回合不能写 | `pending_coverage` | 每碎片至少 2 回合 |
| 工具整场冻结 | 分析 schema 仍大；`put_record` 是大头 | 每回合重发大 schema |
| pack 只改账 | extract-6 已 `pack_max_units=8`、`pack_max_turns=3` | 仍 416 回合、Reviewer 363、`review_rounds=0` |
| 额度耗尽变 blocker | `AGENT_TURN_BUDGET_EXCEEDED` | 无产物 |
| 45 分钟 × 4 次 attempt | worker 硬截止 + SQL fence | 小文件墙钟仍是小时级 |
| 半截捷径 | `Limits.draft_channel`：大纲 ready 就跳过 Rechecker | 派发/工具/预装未改，会复发 416 回合 |

只加 `draft_channel=true` **不算本方案**。产品开关用新字段 `limits.draft_path`（默认 false，测试/终稿保持现网抽取）。`draft_path=true` 时：`role` 始终 Main、`dispatch.entries` 保持 `{}`（否则 SQL 要求 entries 数=source_units+1）、reserve 跳过「首源 owner」检查、heartbeat 20 分钟、`max_turns=80` 且 `at_least_for` 不抬额度。现有 `draft_channel` 禁止当产品默认。

## 3. 已遇问题 → 必须有的落点

| ID | 已遇问题 | 解法 | 验收 |
| --- | --- | --- | --- |
| P1 | 先抽完全文才能编制 | 草稿只认大纲 + 已填章 | 无 Rechecker 能发布并编译 DOCX |
| P2 | 28/148 碎片串行 | 章 = **招标组成项**，不是 `page:N` / heading | 最小稿不以 28 个 SourceUnit 开会话 |
| P3 | 读完本回合不能写 | 预装当前章本回合可写 | 一章常态 1 回合，缺续页 ≤3 |
| P4 | Rechecker 再读一遍 | 草稿不启动 Rechecker | `role` 保持 Main；零 `source_review.results` |
| P5 | 分析+编制两条 45min job | 一条 job：大纲→填章→宿主编译 | 一次请求出分析草稿+DOCX |
| P6 | 工具/schema 太大 | 每阶段 ≤6 个工具；无 `put_record` | 请求 `tools.length≤6` |
| P7 | 预算耗尽无产物 | 剩余章 `omitted(deadline)`，仍发布 | 终态 succeeded 草稿，有文件 |
| P8 | Rechecker 按页/表切 | 草稿无此库存 | 零 `put_source_review` |
| P9 | `plan_complete` 才能 `put_section` | 无编制 Agent；published 时宿主合成 Section 并编译一次 | 结束时一份 DOCX，中途不每章编 |
| P10 | 大纲预览被当成编制依据 | 编制只消费 `draft_plan` | `GET /tender-outline` 标明非编制依据 |
| P11 | 编造投标方事实 | schema 无报价/人员/证书正文 | 工具拒绝这类字段 |
| P12 | Main 自批 | `review.draft=true` 禁止齐套 source_review | SQL/Rust 双拒 |
| P13 | PDF 无 heading 绑错章 | **宿主**绑定 `source_ids`，模型不能改集合 | 失败有 omitted 原因 |
| P14 | 固定标签被 blank 吃掉（api-v4） | 宿主拒绝盖住未选标签的 blank；草稿验收看固定文字可见 | 见 §7.1 |
| P15 | 模型拿 compile 当推进 | 工具列表无 `compile_docx` | 章提交后宿主编 |
| P16 | 草稿当终稿 | 文件名/manifest/导出门标 draft | 终稿入口拒绝 |
| P17 | 解析/OCR 切错 | 不修解析器；失败进 omitted | 不记成模型漏提 |
| P18 | 小文件数小时 | 章派发 + 墙钟/attempt 硬顶 | §9；T5 数字门 |
| P19 | 模型自己 search/inspect 绑源 | 大纲阶段宿主先检索并预装候选；模型只写树 | 大纲 ≤5 回合（小文件 ≤3） |
| P20 | 一章绑上 20 页撑爆窗口 | 章内窗口：≤8k 字 + 邻近表 | 超限分片填同一章，不新开 20 任务 |
| P21 | 表永远单包 | 表跟所在章走 | 空表不单独开会话 |
| P22 | 45min×4 attempt | 草稿墙钟 20 分钟、最多 2 次 attempt | 到期发布已填部分 |
| P23 | 同 run 热换 tools → digest 不一致 | 大纲/填章两个冻结合同，可同 job 两次预约 | 一次 prepared body 内 tools 不变 |
| P24 | UI 仍等终稿 succeeded | 草稿 job succeeded 即可进入生成 | T4 改 AnalysisProgress/编制入口 |

没有落点的不算已解决。

## 4. 主链（草稿 job 内只有两类模型回合）

```
DocReader 冻结（tender_document_process，无模型）
  → 大纲合同（Config A）
       宿主检索组成/指定格式/目录并预装
       模型只 put_outline_item / omit_outline_item
       宿主写每章 source_ids
  → 填章合同（Config B）
       当前 window 预装 → put_chapter_template（多窗 merge）
       宿主 §7.1 校验后写入 Template record
  → published 时宿主合成 Draft.sections + presentation，compile 一次，写入本 job 的对象存储
  → 发布 analysis.draft + docx.draft（同一 requirement_set_compile，不另开 45 分钟编制 job）
  → （另一次用户请求）终稿：以 draft_plan + 已填 Template 为库存，禁止重开 source_unit 抽取
```

禁止：没大纲就扫全文 source_unit；草稿 job 里跑 Main 抽取/Reviewer/编制 Agent；用 Main coverage 伪造 Reviewer；同一 prepared body 改 tools。

## 5. 工作单位：招标组成项，不是解析碎片

`draft_plan` 同时在 `Checkpoint`（运行时）和 `Analysis`（发布产物，`serde(default)`）里，不新建表。`validate_draft_basis` 读 `result.analysis.draft_plan`，不读现网 `outline::ready`。

```text
DraftPlanItem {
  id, parent, order, title, prescribed,
  source_ids,          // 仅宿主写，本章全部冻结源
  windows: [],         // 有序分片，每片 source_id 子集；当前预装=windows[window_index]
  window_index,        // 0-based
  template_id,         // 已有 Template record，供后续 window merge
  status: pending | filled | omitted,
  omit_reason          // bind_failed | deadline | not_applicable | parse_failed | blank_rule | window_exceeded
}
```

章 = 招标规定的组成项（投标函、授权书、报价表、资格文件…），**不是** `page:12:section:0`，也不是每个 heading_path。解析单元只作引用地址。

规则：

1. 大纲未完成（至少 1 个非 omitted 章）前不派填章。
2. 同时只填一章。
3. `source_ids` 仅宿主写。绑定顺序：组成条款点名 → 指定格式/表名精确匹配冻结 text/grid → 同文档邻近页。全部失败 → `omitted(bind_failed)`，禁止模型猜页。
4. 预装当前 `windows[window_index]`：合计 text ≤ 8000 字，表跟邻近正文走，空表不单独开会话。`source_ids` 超限则切成多窗，**同一 chapter_id**。
5. `put_chapter_template`：`chapter_id` 必须等于 `draft_active_id`。通过 §7.1 后 **merge** 进该章 Template（按 form_id+cells 或 source span 去重，后写覆盖同键）。仅当 `window_index` 已是最后一窗才 `filled`；否则 `pending` 并 +1 窗。
6. 额度或墙钟尽：剩余 `pending` → `omitted(deadline)`。**只在 published 编译一次**，不在每章提交时编译。

这替换 `main_dispatch` 扫全文，以及编制 `plan_complete` 全义务前置。

## 6. 工具：换阶段 = 换冻结合同

大纲合同与填章合同在**同一 job** 内两段预约：checkpoint 存 `outline_config_sha256` 与 `fill_config_sha256`。每段 `body.tools` 不变。禁止同一 prepared body 热换工具，禁止拆成两个用户可见 job。

### 6.1 大纲（小文件 4 项，大文件最多 5 项）

T2 宿主预装不得把行业词表写进代码常量。小文件（全文 ≤8k 字）预装整份冻结正文+表。大文件按 `ordinal` 连续装到 8k 字窗口，表跟邻近正文。组成检索词若使用，必须来自 `limits.draft_bind_terms`（可空配置）；缺省只按 `title` 与冻结 `text`/表名精确包含匹配。命不中 `omitted(bind_failed)`，禁止猜页、禁止内置「投标文件组成」等样稿词。

- **必有（2）：** `put_outline_item`、`omit_outline_item`
- `read_source` / `read_form`：**仅当本回合预装未覆盖 grounds 所需源时才广告**；小文件预装全文，默认不广告。
- 不提供 `search_sources`。禁止再加导航工具。

禁止：`inspect_analysis`、`check_gaps`、`set_work_note`、`request_review`、`put_record`、`put_source_review`、repair、`compile_docx`。

小文件大纲目标 ≤3 回合。

### 6.2 填章（默认 2 项）

- **必有：** `put_chapter_template`、`put_chapter_omission`
- `read_source` / `read_form` / `read_source_view` 默认不广告。仅当当前 window 预装缺块（扫描页、印章、续页未装入）且用户包仍放得下，本回合才临时加入对应只读工具，且每种最多 1 个。禁止常驻 4～6 个工具。

禁止：`put_record`、`compile_docx`。

### 6.3 终稿

另一次用户请求。库存 = 已发布 `draft_plan` + 已填 Template + 草稿 DOCX。Rechecker 按章对照原文/DOCX，**禁止**再按 FrozenInput 的每个 source_unit 派发抽取。现 `validate_basis` 不放宽。

## 7. 回合协议

保留 Journal v3。

| 证据 | 本回合可写入？ |
| --- | --- |
| 本请求 `preloaded_evidence`（当前 window） | 可以立刻 `put_chapter_template` |
| 本回合新 `read_*` | 仍等下一回合 |
| 其他章 / 非当前 window | 工具拒绝 |

一章常态 1 回合：预装 → 写区域。缺续页最多再 2 回合读+写。

### 7.1 固定文字宿主校验（挡住 api-v4 类空壳）

`put_chapter_template` 成功前宿主检查：

- `fixed_text` 的 citation 必须落在已交付 window，且引用字节/单元格原文非空。
- `bidder_blank` 不得把非空原文的全部非空白字都抹掉。整格/整段 blank 仅当源文本 `trim` 为空；`blank_ranges` 只能盖住明确选出的待填区间，未选中的原文必须保留。不用名称/单位等词表判断标签。
- 投标方事实靠 schema：`additionalProperties:false`，且 `put_chapter_template` 无 fact/报价/人员字段。出现额外字段或 `kind=fact` → 拒绝。不在代码里写「报价」「证书编号」词表。

不通过不得标 `filled`。草稿验收：打开 DOCX 至少一处招标侧固定文字或表结构可见。不要求 R03/R06 整项通过。

## 8. 两套门

### 草稿

- `review.draft = true`，`quality = needs_review`
- 不要求 `source_review.results`、gaps 空、五项 global_checks
- 要求：frozen digest 一致、`quality=needs_review`、`draft_plan` ≥1 个非 omitted 章、已填 Template 仍在 records
- 未覆盖 = `draft_plan` omitted / 「草稿未覆盖」，不是「来源证明无此义务」
- `compile()` 在 `review.draft` 时禁止调用官方 `validate_basis`，并 **跳过 `validate_plan`/`plan_complete` 以及 `account()` 对义务库存的完整性**（未 Filled 章被 `omitted(deadline)` 后 records 里可能残留 Template，不算编制缺口）。仍校验 `validate_plan_sections` 与 grounds。
- SQL `kb_bid_v2_publish_requirement_set_v4`：`review.draft=true` 分支校验 checkpoint/`draft_plan`/`source_review` 为 null 后 **END IF**，再走共享 INSERT。草稿 **跳过** source_review 完整性、global_review、以及 `dispositions` 必须覆盖全部 source_unit。终稿 `ELSE` 分支不放宽。
- 草稿 DOCX：`compile` 成功后把字节写入 `checkpoint.draft_docx_base64`，`draft_compile_object_id=objects/{sha256}`。禁止只存 hash、丢掉字节。
- 取消/墙钟尽（`AGENT_DEADLINE_EXCEEDED`、drive 因 cancel 得到的 `INTERNAL`）与 `AGENT_TURN_BUDGET_EXCEEDED` 一样：`omit_pending_deadline` 后 `finish_draft_path`。草稿路径上除 `FROZEN_INPUT_DIGEST_MISMATCH` / `AGENT_OUTPUT_INVALID` / `AGENT_PROVIDER_UNAVAILABLE` 外的中断都发布已有大纲，禁止无产物。
- 草稿 reserve：`dispatch.entries` 为空时不校验「首源 ordinary owner」。

### 终稿

现 `validate_basis` + `kb_bid_v2_publish_requirement_set_v4` **不放宽**。  
`review.draft=true` 禁止终稿编制和正式导出。

### 编制

- `validate_basis`：终稿
- `validate_draft_basis`：草稿
- 草稿：无 `plan_complete`、无编制 Agent；published 时宿主合成 Section/presentation/plan 后 `compile()` 一次
- `requirement_set_compile` worker 有权写入 `draft_compile_object_id`（对象存储），不另开编制 job

## 9. Job、墙钟、attempt

草稿复用 `requirement_set_compile`（worker 内接编译），避免 45+45。

| 项 | 草稿 | 终稿 |
| --- | --- | --- |
| 墙钟 | `draft_deadline_secs(true)=1200`：heartbeat 20 分钟取消；worker Oxana 外层仍 45 分钟 | 45 分钟 |
| `max_turns` | **80**（`at_least_for` 不得抬到终稿 1200） | 现合同 |
| attempt | 表 CHECK 仍为 4；`draft_claim_exhausted(true, attempt)` 在 **attempt>2** 时不再执行（claim 截断） | 终稿 4 |
| 到期 | 已填则 compile 一次并 succeeded；仅大纲则目录骨架 | 按 rig |
| 禁止 | 共用终稿 max_turns 或加到 1200/4000 | 用草稿销 32 项；从 source_unit 重抽 |

有大纲无任何 filled 章：仍可发布「仅大纲」草稿，DOCX 可以是目录骨架，报告写明未填。有 filled 章则必须带 DOCX。禁止终态为 `AGENT_TURN_BUDGET_EXCEEDED` 且无分析产物。

## 10. 保留与不做

保留：DocReader 冻结、SourceUnit/网格身份、Journal v3、对象存储、正式 `submission_export`、终稿 Rechecker、投标方事实留白、大纲预览 API。

草稿自动路径不做：source_unit 抽取；独立 Rechecker；编制 Agent；发布前扫全文；整场 15+21 工具；预装不能写；编制前全义务 plan；加总预算代替改工作单位；preview GET 当编制输入；把现有 `draft_channel` 当本方案。

## 11. 任务切片

未完成不算问题已解决。现有 `draft_channel` 不得在 T3 完成前设为产品默认。

### T0 本文

P1–P24 均有落点。rig 标明日常自动出稿走本文。

### T1 草稿发布门

`Review.draft`、`quality=needs_review`、`validate_draft_basis` 看 `draft_plan` 不看 `outline::ready`；SQL draft 分支（非 draft 行为不变）。`compile()` 草稿分支不走官方 `validate_basis`。  
**验证：** 有 `draft_plan` 无 Rechecker、无 disposition 能发布；`draft=false` 缺 Rechecker 仍拒绝；旧 postgres 官方路径仍过。

### T2 大纲合同

小文件预装全文；绑定按 title 包含匹配（可配置 `draft_bind_terms`，代码无内置词表）；无 `search_sources`。  
**验证：** 最小稿大纲 ≤3 回合出组成树；title 在冻结正文中不存在 → `omitted(bind_failed)`，不猜页。

### T3 填章 + 预装可写 + §7.1

当前 window 预装直接记 coverage（不走 `pending_coverage`）；同批可写；多窗 merge；blank 盖标签被拒。  
**验证：** 脚本 1 回合填一章；同批新读不能写；跨章 read 失败；故意整格 blank 含标签 → 拒绝；两窗同章 merge 后才 `filled`。

### T4 宿主合成稿 + 编译 + UI

无编制 Agent、无 `put_section`/`compile_docx`。published 时宿主合成 Section/presentation 后 `compile()` **一次**，由本分析 job 写入对象存储。草稿 succeeded 可打开；文案标草稿；文件名/manifest 带 draft。  
**验证：** 打开 DOCX 能看到固定文字或表；终稿入口拒绝 draft；编制 Agent 未启动；分析 job 内有 `draft_compile_object_id`。

### T5 时间回归（关闭 P18/P22）

最小合成稿：

- 必须产出草稿 DOCX
- 回合数相对 extract-6 的 416 **低一个数量级**（大纲 ≤5 + 章数×1～3）
- 墙钟远低于 45×4 分钟
- `tools.length≤6` 且不含 `put_record` / `inspect_analysis` / `put_source_review`

`BiddingFile.pdf`：带组成章节的草稿（允许 omitted）；若仍数小时 = T2/T3 失败，禁止加预算结案。记录回合、墙钟、已填章、omitted 原因。

### T6 终稿（不挡 T5）

另一次请求。输入 = 已发布草稿的 `draft_plan` + Template + DOCX。按章复核，不重开 source_unit 抽取。官方 `validate_basis` 不放宽。不挡 T5。

## 12. 不在 T1–T5 解决

DocReader 解析/OCR/列宽、32 项与 R03/R06 整项、同版 PDF 与终检报告、ONLYOFFICE 编辑保存、终稿 Rechecker 加速。记在 rig §19，不能用草稿 DOCX 销项。

## 13. 实施前对齐

- [ ] 草稿与终稿两套门，互不冒充
- [ ] 产品重点是按章模板，大纲只是第一阶段
- [ ] 章 = 组成项，不是解析页/heading
- [ ] 宿主绑源、宿主校验 blank、宿主编译
- [ ] 预装可写；阶段换合同不热换 tools
- [ ] 20 分钟 / 2 attempt；到期有草稿产物
- [ ] 禁止用现有 `draft_channel` 当本方案
- [ ] T5 前不宣称 106 页/32 项已通过

## 14. 工具定义（冻结合同里必须有的字段）

复用现有 `read_source` / `read_form` / `read_source_view` / `search_sources` 的参数与回执，不改名字。草稿**新增**四个写入工具；全部 `additionalProperties: false`。Citation 与现网一致：`citation_ref` 或 `{source_id,start,end}` / `{form_id,row,column}`。

### 14.1 大纲合同 `tools`（默认 2 项）

默认只有 `put_outline_item`、`omit_outline_item`。预装已覆盖当前工作源时不广告 `read_*`。

**`put_outline_item`**

```text
id: string | null          // 空=新建，非空=替换已返回 id
parent: string | null      // 已分配的大纲 id，根为 null
order: integer >= 0
title: string minLength 1  // 招标原文用语，禁止翻译成通用目录名
prescribed: boolean        // 是否招标点名的组成/格式
grounds: Span[] minItems 1
```

返回 `{id, saved:true}`。同一 `(parent, order)` 后写覆盖先写。`title` 不得为空或纯标点。

**`omit_outline_item`**

```text
id: string | null          // 已有项或 null 表示「此处本应有章但无法立项」
title: string minLength 1
reason: bind_failed | not_applicable | parse_failed
grounds: Span[] minItems 1
```

禁止无 grounds 的 omit。omit 不生成 Template。

提示（大纲，≤2KB）：证据已预装组成/格式条款；把招标点名的组成写成树；不要 invent 通用目录；不要检索当主循环。

### 14.2 填章合同 `tools`（默认 2 项）

默认只有 `put_chapter_template`、`put_chapter_omission`。缺块时才临时加入只读工具。

**`put_chapter_template`**

```text
chapter_id: string         // draft_plan.id，必须等于当前活动章
id: string | null          // Template record id
title: string
purpose: string
regions: TemplateRegion[]  // 复用现有结构：source, role, form_id?, cells, blank_ranges, instruction
```

`role` 仅 `fixed_text | bidder_blank | instruction | signature`。`chapter_id` ≠ `draft_active_id` → 拒绝。不得出现 fact/requirement 字段。通过 §7.1 后 merge 进该章 Template；最后一窗才 `filled`。

**`put_chapter_omission`**

```text
chapter_id: string
reason: bind_failed | deadline | not_applicable | parse_failed | blank_rule | window_exceeded
summary: string minLength 1
grounds: Span[]            // deadline 可空；其余 minItems 1
```

把该章标 `omitted`，不写 Template。

提示（填章，≤2KB）：只处理当前章 window；预装已读；固定文字必须引用原文；待填用 blank_ranges，不要抹掉标签；不要写投标方姓名/价格/人员。

### 14.3 明确没有的工具

草稿两段都没有：`put_record`、`put_relation`、`set_disposition`、`set_work_note`、`request_review`、`inspect_analysis`、`check_gaps`、`source_index`、`collection_index`、`put_source_review`、`complete_review_check`、`put_repair_result`、`compile_docx`、`put_section`。草稿 `apply` 也不得再接受这些名字。

此条**只**约束 `limits.draft_path`：模型请求里的 `tools` 用 `tender-draft-outline/fill-tools-v1`，默认各 2 个写入工具。终稿请求的 `tools` 仍是 [agent-runtime-rig.md](agent-runtime-rig.md) §8 与 `tender-analysis-tools-v1`。禁止把终稿工具塞进草稿请求，也禁止为了草稿去改终稿合同。T1–T5 不删终稿工具；T6 按「不重开抽取」再收。

### 14.4 每个工具是否必要（评估结论）

编译器要的输入只有：冻结招标原文 + 章顺序 + Template 区域。下面按「日常出投标草稿」评估；不是按「工具已经存在」保留。

**草稿请求（T1–T5，必须短）**

| 工具 | 必要？ | 理由 |
| --- | --- | --- |
| `put_outline_item` | **要** | 招标点名的投标文件组成/顺序，模型唯一写入 |
| `omit_outline_item` | **要** | 组不出来必须带 grounds 的省略，禁止空 omit |
| `put_chapter_template` | **要** | 产品核心：固定文字 vs 投标留白 |
| `put_chapter_omission` | **要** | 一章填不完要有原因，到期走 deadline |
| `read_source` / `read_form` / `search_sources` / `read_source_view` | 缺块才临时加 | 小文件预装全文则不要；大文件缺续页才加，不是主循环 |

**草稿请求里不要（已写入 14.3）**

| 工具 | 为什么不要 |
| --- | --- |
| `put_record` / `put_relation` / `delete_*` / `set_disposition` | 抽 Fact/Rule/Requirement/关系。编译器不用这些图 |
| `set_work_note` / `request_review` | 宿主派章，不由模型选下一包或切 Rechecker |
| `inspect_analysis` / `check_gaps` / `source_index` / `collection_index` | 给抽取循环找 ID/补洞。草稿预装当前章，禁止当主循环 |
| `put_source_review` / `complete_review_check` / `put_repair_result` | Rechecker/修复。日常出稿不启动 |
| `compile_docx` / `put_section` | 编制 Agent。草稿由宿主合成 Section 后 `compile()` 一次 |
| `put_analysis_check` | 五项 global_checks。草稿 `quality=needs_review` |

**终稿（T6，本目标不做）**

方案写明：输入是已发布草稿的 `draft_plan` + Template + DOCX，**按章复核，不重开 source_unit 抽取**。因此终稿也不该再跑 23 个分析抽取工具当主循环。T6 应收成：按章核对已有 Template、正式编制覆盖、出件复核（现 `export-review-tools-v1` 已是 4 个）。现网 `tender-analysis-tools-v1` 23 个、`docx-composition-tools-v1` 21 个是旧抽取/编制循环，留给 T6 按上表删，T1–T5 不改终稿合同以免和现网测试缠在一起。

`Config.tools_sha256` / `main_prompt_sha256` 按当前阶段计算。checkpoint 保存两份 sha，job 仍是一个 `requirement_set_compile`。草稿 `limits.max_turns=80`，与终稿 1200 分开。

## 15. 状态

不新建表。`Checkpoint` 增加（`deny_unknown_fields` 下必须带 default，旧终稿 checkpoint 仍能反序列化）：

```text
draft_stage: none | outline | fill | published
draft_plan: DraftPlanItem[]
draft_active_id: string | null
draft_compile_object_id: string | null   // objects/{sha256}
draft_docx_base64: string | null         // 有 filled 章时必有
outline_config_sha256 / fill_config_sha256: string | null
```

其余现有字段：

| 字段 | 草稿用法 |
| --- | --- |
| `role` | 始终 `Main`。不得进入 Reviewer |
| `dispatch` | 空。不用 source 种子 |
| `source_review` | `None` |
| `review` | 发布时写 `{draft:true, analysis_sha256, findings:[]}` |
| `main_work` | `source_scope = 当前 window`；由宿主写，模型无 `set_work_note` |
| `analysis.records` | 仅 Template（填章）+ 大纲不写 record |
| `analysis.dispositions` | 草稿不写。覆盖看 `draft_plan.status` |
| `pending_coverage` | **仅**本回合新 `read_*`。预装 window 在 prepare 时直接写入 `analysis.coverage`，不经 pending |
| `transcript` | 见 §16 |
| `main_progress` | 停表后走 §9 发布，不把 deadline 变成无产物 blocker |

状态机：

```
outline: 活动项=整份大纲
  put_outline_item / omit_outline_item
  宿主判定「至少 1 个非 omitted 章且模型本批未再新增」→ 切 fill
fill: 活动项=draft_active_id
  宿主按 order 取下一个 pending，计算 window，预装
  filled | omitted → 下一章
  无 pending 或墙钟/额度尽 → published
published: run() 返回 draft AnalysisResult，worker 挂上 docx
```

大纲何时结束：本批零 `put_outline_item` 且 plan 非空；或小文件已达 3 回合且 plan 非空；或模型对宿主预装的组成条款都给了 item/omit。禁止等「扫完全文 source_unit」。

切 fill 时：`draft_stage=fill`，`dispatch` 保持空。不在批末把 `journal.session` 置空（committed 边界还要用它，否则 `FROZEN_INPUT_DIGEST_MISMATCH: pending SDK session missing`）。下一 prepare 改用填章 tools；`Session::prepare` 在投影不复用时 rebase。coverage 保留已读 window。

## 16. Session 与 transcript

沿用 Journal v3 三边界和 `SESSION_PREFIX=2`。草稿只有 Main，无 Reviewer suffix 切换。

**一段合同之内（大纲或填章）：** 活动章/大纲未变则复用 SDK session。超字节从宿主 transcript 重建，不重置 `turn` / 费用 / coverage。

**换阶段（大纲合同 → 填章合同）：**

1. 大纲最后一批 **committed 之后** 才改 `draft_stage=fill`。
2. 下一 `prepare_request` 广告填章 tools（`put_chapter_template` / `put_chapter_omission`），system 用填章提示。
3. 不要在 execute/after_batch 里 `session=None`。`Session::prepare` 比较投影，`!reuse` 时 rebase 新 AgentRun。同一 prepared body 内 tools 仍不变。
4. 填章第一包带当前章 window 预装 + `draft_plan` 摘要。
5. `turn` / `tool_calls` / `read_bytes` 累计。

**填章换下一章：** 同填章合同，可复用 session；但用户包换成新 window 预装。上章原文可裁；上章 Template id 留在 `draft_plan` / records，不靠 transcript 全文。

**可见证据：** 当前判断只认本请求里的预装 + 已确认 coverage。裁掉的上章原文不算已读于本章。重装已读 window 不计「新进展」，但计入本次字节。

**容量：** 预装失败（window 序列化 > `max_tool_result_bytes`）→ 缩小 window（仍同一 `chapter_id`），不得截断后假装完整交付。

---

组成绑定与 blank 校验均不得把行业词表写进代码。`limits.draft_bind_terms` 可空；缺省 title 精确包含。blank 用「非空原文不得被全部抹掉」的结构规则。草稿 compile 复用现 `docx_composition::compile`，必须走 `validate_draft_basis`；presentation 由宿主填默认值。
