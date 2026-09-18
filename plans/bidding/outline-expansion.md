# 投标组成大纲展开

## Context

截图里的树几乎是须知组成条款的**字面第一层**（商务下列 1A–1D/4/6/7、「附件 8」整包、报价 9 与 2；技术分册空）。106 页原文里，资格包后文还有子件目录，技术分册要求结构与技术规范部分一致。漏的不是边角，是**该拆的节点没拆完就停了**。

不能绑条款号（不是「写完 3.1」），不能绑词表（不是「附件 8」），也不能让模型对每个标题投「算不算组成必要信息」——那会把招标 TOC、须知条款和技术正文搅进来。

目标：组成树按父子对应写全；**叶子才填章**；转填章由宿主判定「展开完成」，回合上限只是硬顶。

## Approach

**模型只做一件语义活：** 找到「投标必须提交什么」的组成/格式条款（标题各标不同），按列举写树。不要对全书每一行标题投票。

**宿主不发明章节，只验收已写入的节点：**

| 级 | 谁写 | 宿主 |
| --- | --- | --- |
| 一级 | 组成条款点名的分册，`parent=null` | 每个未省略一级必须有未省略二级。一级的孩子**不**从全书 `heading_path` 长出（避免抄招标目录） |
| 二级 | 点名它的分册下列的每一项 | 并列多项必须是兄弟（已有 `concatenated_composition_title`） |
| 三级+ | 该上级在后文仍当标题并列出的子项 | 见下「目录节点」 |

**必须全部展开的节点（填章前）：**

1. 每个未省略**一级**：≥1 个未省略二级。原文「如有」且无列举 → 允许 `omit` + grounds；否则继续写。
2. 每个未省略的**非根节点**：若冻结来源 `heading_path` 在匹配该标题核心的那一截后面还有下一级标题，这些下一级**全部**必须成为活孩子（`folded_contains` / `title_core`，不要求字面相等）。
3. 组成条款写「结构/顺序与某部分一致」：模型把**那一部分自己的目录**挂到该分册下。宿主用 (1) 卡住空分册，用 (2) 卡住「写了某章却不写章内子目」。
4. 父子：已有 `outline_child_fits_parent`（编号子孙或标题核心包含）。禁止页码邻近、数字碰巧相同。

**不要展开：** 招标总目录根（公告/须知章节号）不得成为一级的孩子。一级孩子只来自组成条款列举。

**转填章（大文件）：** `outline_tree_ready` **且** 上述目录节点已展开；再加上现有空闲回合或大纲回合帽。帽不是「齐了」。小文件仍可预装后短大纲。不提高 `DRAFT_OUTLINE_MAX_TURNS` 来混过门。

**填章：** 只填没有活孩子的叶子（现有 `assign_next_chapter` 已跳过有孩子的节点）。有孩子的分册/章编译成空 heading。

宿主在大纲阶段的 work/progress 里带上**未展开目录缺口**（节点标题 + 源里见到的下一级标题），模型按缺口补孩子，不靠「再读一遍 3.1」。

## Files to modify

- `crates/bidding/src/tender_analysis/draft.rs` — `source_subheadings`、`catalog_nodes_expanded`、`outline_expansion_ready`；`after_batch` 大文件改用 expansion 门；大纲 work 带缺口
- `crates/bidding/prompts/tender-draft-outline-v1.txt` — 组成条款 + 后文同标题下的列举必须写全；禁止条款号/词表/「是否必要」投票
- `crates/bidding/src/tender_analysis/tests/draft.rs` — 空一级仍挡填章；非根节点 heading 有下一级则必须有覆盖孩子
- `plans/bidding/outline-expansion.md` — 本方案（已作为实施说明）

展示侧（`get_tender_outline` / `outline.rs` / `AnalysisProgress.tsx`）已改为 `draft_plan` 树，本方案不重做，验收时确认页面「投标文件组成」读的是这棵树。

## Reuse

- `outline_tree_ready` / `has_plan_children` / `assign_next_chapter`（叶子优先）— `draft.rs`
- `folded_contains` / `title_core` / `outline_child_fits_parent` / `concatenated_composition_title` — `draft.rs`
- `Source.locator.heading_path`（` > ` 分段）— 已有冻结字段，不新词表
- `DRAFT_OUTLINE_MAX_TURNS` — 仍只当硬顶，不当齐套定义
- `plan_tree` — `tender_analysis/outline.rs`（预览）

## Steps

- [x] `source_subheadings(input, title)`：对每个 source 的 `heading_path` 分段，标题核心互相包含则收集**下一段**为应收孩子；只用于**已有 parent 的节点**
- [x] `catalog_nodes_expanded`：每个这类节点，应收孩子都必须被某个活孩子标题覆盖
- [x] `outline_expansion_ready` = `outline_tree_ready` ∧ `catalog_nodes_expanded`
- [x] 大文件 `after_batch` 用 `outline_expansion_ready` 替换单纯 `outline_tree_ready`
- [x] 大纲 `main_work.note` 或 progress 列出未覆盖的子标题，便于模型补写
- [x] 提示词按 Approach 改，零条款号、零行业词
- [x] 测试：空一级不转填；heading 有下一级的二级缺孩子不转填；孩子覆盖后可转填；`8.` 下挂无关标题仍拒

## Verification

- `cargo test -p bidding --lib tender_analysis::tests::draft -- --test-threads=1`
- `cargo clippy -p bidding --lib --tests -- -D warnings`
- 合成：组成清单 + 后文 `heading_path` 带子标题 → 未写孩子时保持 Outline
- 不把 BiddingFile 106 页/32 项当作本方案通过条件；本方案只保证宿主不再把「组成条款第一层」当成齐套
