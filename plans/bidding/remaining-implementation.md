# 未完成项实施任务（草稿主路径 + 终稿）

日期：2026-09-17。本文**不是**新设计，只把两份已定方案里尚未闭合的项拆成可执行任务。设计仍以 [outline-then-template.md](outline-then-template.md) 与 [agent-runtime-rig.md](agent-runtime-rig.md) 为准；完成标准不得降低。草稿 DOCX 不能销 32 项或同版三件套。

**执行顺序：先闭合产品草稿主路径（D0–D5），再做终稿库存化（D6 / T6），终稿调度与样稿验收（F）并行但不挡 T5。**

状态约定：`open` 未做；`partial` 有代码/测试但产品或真实验收未过；`blocked` 依赖外部端口或真实模型跑。

---

## 0. 已有 vs 未有（对照基线）

| 路径 | 已有 | 未有 |
| --- | --- | --- |
| 草稿 T1 | `validate_draft_basis`、SQL `review.draft` 跳过 Rechecker/disposition、`reserve` 按 `draft_path` 跳过 owner | 产品真跑仍待 D5 |
| 草稿 T2/T3 | 大纲/填章双冻结 hash；SQL 允许 fill hash；内存脚本测 | 产品真跑仍待 D5 |
| 草稿 T4 | 分析 job `stage`+`write_blob`；`publish_v4` 写 current；UI 去编制入口；编制/导出 SQL 拒 `review.draft` | PG 联合未跑；D5 真跑 |
| 草稿 T5 | 脚本 `turn≤8`、`tools.length≤6` | 无相对 extract-6 的真实模型数字门 |
| 草稿 T6 | — | 终稿仍按 source_unit 抽取 |
| 终稿宿主合同 | Rule.items、AnalysisResult v2、Draft.plan、装包、EnterReview、export_review 工具 | 完整 §3–§6 调度、增量 pending、S4-B 闭环、S5-A/B、S6 |

工作区大量相关文件仍是未提交改动；落地以测试+SQL+产品入口为准，不以「有 Rust 文件」为准。

---

## 轨道 D：日常草稿（产品主路径）

对应 [outline-then-template.md](outline-then-template.md) T1–T5。T6 单列但不挡 T5。

### D0 草稿 PG 预约合同（挡 T2/T3/T4 真跑）

**问题：** `prepare_request` 在 `draft_path` 下把 `main_dispatch` 写成 `null`、填章阶段广告 `fill_schemas()`。`kb_bid_v2_tender_agent_reserve` 仍要求 Main 包带 owner，且 `tools_sha`/`prompt_sha` 只能等于开跑时的大纲合同。产品 PG 会在第一轮或切填章时 `FROZEN_INPUT_DIGEST_MISMATCH`。内存 Journal 测不到。

**做：**

1. `Config` 开跑同时冻结 `tools_sha256`（大纲）与 `fill_tools_sha256` / `fill_prompt_sha256`（填章）。同一 job 两段预约，禁止同一 prepared body 热换。
2. SQL `reserve`：`limits.draft_path=true` 时
   - 跳过 `main_dispatch` owner / 首源 ordinary owner
   - Main 的 tools/prompt 允许大纲 **或** 填章冻结 hash
   - `dispatch.entries` 保持 `{}` 合法
3. 终稿 `draft_path=false` 行为一字不放宽。

**验证：** 字符串合同测 SQL 含 draft 分支；PG：空 dispatch 能 reserve 大纲包；第二轮 fill tools hash 不等于大纲 hash 仍能 reserve；`draft_path=false` 缺 owner 仍拒绝。

**落点：** `crates/bidding/src/tender_analysis/agent.rs`、`migrations/bidding_v2_baseline.sql` `kb_bid_v2_tender_agent_reserve`、`crates/bidding/src/tender_analysis/tests/draft.rs`。

### D1 分析 job 写入真实对象（T4 对象存储）

**问题：** `draft_compile_object_id=objects/{sha}` 未 `stage`/`commit`，也未 `write_blob`。禁止只存 hash。

**做：** `finish_draft_path` 编译后保留 `draft_docx_base64`；`postgres::execute` 在 `publish_requirement_set_v4` 成功后，对有 filled 章的草稿：`stage_object_upload` + `write_blob` + 请求级 object commit。`object_ref` 必须等于 `objects/{sha256}`。无 filled 章不强制 DOCX 对象（仅大纲骨架按方案允许）。

**验证：** PG：registry 能按 sha 读回与 checkpoint 相同字节；失败回放不重复上传不同 sha。

### D2 草稿成为工作区当前稿（T4 可打开）

**问题：** `DocxEditor` 只认 `docxApi.current`。`kb_bid_v2_create_docx_round` 要求 `user:` actor，分析 worker 是 `system:requirement-set-compile-v4`，不能直接复用用户建 round。

**做：** 新增 `kb_bid_v2_publish_draft_docx`（系统 actor + 现有 owner fencing）：

- 前置：本请求已 published 的 requirement set 为 current；checkpoint 有 DOCX 字节且 sha 与 staging 一致
- 写入 `bid_docx_round` / `bid_docx_version` / `bid_docx_current`（CAS 当前 version）
- manifest `status=needs_review`、`source_quality=needs_review`；文件身份带 draft
- **不**走 `kb_bid_v2_publish_docx_composition`（那是编制 Agent 终稿门）
- 正式 `submission_export` / 编制 `validate_basis` 仍拒绝 `review.draft`

**验证：** 分析 succeeded 后 `get_current_docx` 有 version；编制 start 对 draft analysis 仍 4xx；导出拒绝 draft。

### D3 UI：草稿 succeeded 打开编辑器，禁止编制 Agent

**问题：** `Workbench` 在分析 succeeded 且无 current 时挂 `DocxStart`，会再开 45 分钟编制 job。

**做：**

- 分析 succeeded 且有 current → `DocxEditor`
- 分析 succeeded 且无 current → 草稿占位（等待 D2 或下载），**禁止**「生成模板」
- 进度文案：大纲/填章/已完成草稿；去掉「正在复核 / 已提取 N 条」
- 下载文件名 `投标草稿-{revision}.docx`；大纲预览继续标明非编制依据
- `onCreateRound` 不得对 draft 分析再拉编制 Agent

**验证：** 前端测：succeeded 不出现 `docx-composition` 的 start；下载名含「草稿」。

### D4 两套门产品入口（T4 终稿拒绝）

**已有：** `validate_basis` / `Draft::new` 草稿分支。

**还要：** composition prepare SQL、`submit_docx_composition_request`、`submission_export` 创建对 `review.draft=true` 明确拒绝；UI 导出页标明「终稿需另一次独立复核请求」。

**验证：** API 测草稿项目不能创建 composition / submission_export。

### D5 时间回归（T5）

**前置：** D0–D3。禁止加 `max_turns`/`draft_channel` 结案。

**做：** 新目录（禁止续 extract-6）：

1. 最小合成稿：必须有草稿 DOCX；回合相对 416 低一个数量级（大纲 ≤5 + 章×1～3）；墙钟 ≪ 45×4 分钟；`tools.length≤6` 且无 `put_record` / `inspect_analysis` / `put_source_review`
2. `BiddingFile.pdf`：允许 omitted；若仍数小时 = D0/T2/T3 失败

记录回合、墙钟、已填章、omitted 原因。`.env` 不临时覆盖模型。

**验证：** 产物目录 + 数字门 JSON。此门未过不得宣称产品草稿可用。

### D6 终稿按章复核（T6，不挡 T5）

另一次用户请求。库存 = 已发布 `draft_plan` + Template + 草稿 DOCX。Rechecker **按章**对照原文/DOCX，禁止再按 FrozenInput 每个 source_unit 抽取。官方 `validate_basis` 不放宽。现网 23/21 工具留给本切片按方案 §14.4 收，不在 D0–D5 改终稿合同。

---

## 轨道 F：终稿（独立复核 + 正式编制 + 三件套）

对应 [agent-runtime-rig.md](agent-runtime-rig.md) §15 / §19。S5-A 最低合同不能省略。

### F1 分析调度协议收口（§3–§6 / RT01–RT17）

**缺口：** 编制局部换项有回归；分析 Main 根 owner、写范围、跨 blocked/repair、增量 `pending_delivery` 未等同完整协议。现 `pending_coverage` 不能当同轮增量合并。

**做：** 按 rig §4.1–§4.4 / §6 补 `confirm_work`、局部完成、`continuation()` 与 SQL 单调消耗。不引入第二套任务框架。

**验证：** RT01/RT06/RT09/RT10/RT16 定向 + PG 联合。extract-4 形状必须 EnterReview。

### F2 Rechecker 装包硬顶（§4.5.2 / extract-7）

代码已有 `pack_batches`。必须 **新目录** 验证：预装后禁止只 inspect；满 3 批无 `put_source_review` 则 block 本包；零独立复核不写 AnalysisResult。extract-6 只对照。

### F3 §9.2 语义反例（B15–B19）

宿主投影已有。用新合同真实模型对照 api-v4 五类失败；人工预期不进模型。未发现则记失败，不改阈值。

### F4 S4-B `docx_layout` 闭环

端口预检：只读定位可；双会话同旧值两次写均成功，公开 callCommand 不能当 CAS。

**做：** 显示页码、跨会话隔离写入、API/worker/callback、有界稳定循环、ready 原子创建 export。未证明则能力阻塞，不猜页码。

**验证：** O03–O07 / O12–O13。无人值守与需打开编辑器分开记账。

### F5 S4-C / S5-A 最小正确模板 + 同版三件套

S5-A 必须：独立来源/候选复核、AnalysisResult v2 global_checks、Rule.items、Draft.plan 同一义务库存、§11 稿件复核、实际 DOCX/PDF 独立检查。延期的是自动页码和产品终检自动化，不是内容完整性。api-v4/v5/v6 均未关闭此门。

### F6 S5-B 产品自动全链

同一产品入口重复最小验收，使用 F4 适用能力与 F5 产物。

### F7 S6 复杂场景 + 106 页 / 32 项

全文启动前恢复 `acceptance-index.json`。分析/编制/保真/同版出件/成本分别报告。任何一项未测写未测。

---

## 明确不做（本账本）

- 用 `draft_channel` 当产品默认
- 用加总预算、续旧目录、本地 collector 报告销终稿
- 热改正在运行的旧合同
- T5 前宣称 106 页 / 32 项
- 新建表/队列/Agent 服务/通用规则引擎

---

## 切片顺序与依赖

```text
D0 SQL/冻结合同
  → D1 对象字节
    → D2 工作区 round
      → D3 UI 去编制 Agent
        → D4 导出门
          → D5 真实时间门
D6 不挡 D5
F1/F2 可与 D 并行（终稿合同）
F3 → F5 → F6 → F7
F4 不挡 F5-A，挡 F6 中页码能力项
```

当前进度：D0–D4 代码已落地。D5 最小合成稿过门。v8 数字门过但投标函 1A–1D 捆成一条、技术文件无 L2。已改：列举结构拒一条多标题、每个活分册都要有子女。v9 已启动：`artifacts/bidding/draft-d5-biddingfile-v9/`。
