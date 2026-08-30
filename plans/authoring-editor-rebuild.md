# 编制画布彻底重构

## Context

编制页现在不像投标文件编辑器：多实例 Tiptap 拼卡片、自制插入按钮、原生 `<select>`/`<button>`、左栏不像大纲。已确认：

1. 中间是 **Word 连续白纸**，章标题来自左侧树。
2. 能力要对齐 **完整 Markdown 编辑器**（所见即所得 + Markdown 快捷键），**真源仍是 `OutlineNode` + `ContentBlockV1`**，不是 `.md` 文件，不装 `@plannotator/ui`。
3. **整篇随时可改**，不是点中一章才能打字。
4. 右栏保留，改干净。
5. 顶栏「生成大纲 / 生成本章 / 填充空章 / 保存」留下，**全部改成 Mantine**，禁止编制面原生控件。

当前块 schema（前后端一致）正文只有 `paragraph | bullet_list | ordered_list` + marks（B/I/U/S/code/link）。表格、图、分页、签章是**独立块**。导出 renderer 也只认这些。所以「完整 Markdown」必须 **扩 schema + adapter + renderer**，不能只加 Tiptap 扩展。

章标题继续只属于树：正文里的 `#` **不新建大纲节点**（否则和 Word 左栏大纲抢身份）。`#` 在正文里当强调样式或忽略，章节只通过树 / 画布章标题行改。

## Approach

**一张纸、一个 Tiptap。** `DocumentCanvas` 用**一个** `useEditor`，文档结构：

```text
doc
  chapter{ lineageId, depth, title }     ← 标题行 = OutlineNode，不可变成正文 heading
    paragraph | bulletList | orderedList | blockquote | codeBlock | table
    image | pageBreak | signature
  chapter …
```

- 点击左树：滚到对应 `chapter`，不卸载编辑器。
- 滚动：`chapter` 进视口则反标左树 / 右栏。
- 改章标题行 = `renameNode`；在章内打字 = 把该 chapter 的子节点 diff 回该章的 `ContentBlockV1[]`，走现有 `DraftMap` + CAS。
- 空行 `/` 或 FloatingMenu：段落、列表、引用、代码、表格、图片、分页、签章。
- 选区 BubbleMenu：B/I/U/S、链接、列表。不要 always-on 格式条，不要页面级「插入段落」。
- Markdown 输入：`**` `*` `` ` `` `- ` `1. ` `>` ` ``` ` 走 Tiptap input rules。

**本轮扩展 `RichNode`（前后端 + 导出）：** `blockquote`、`code_block`、`horizontal_rule`（`---`）。`heading` 仍不进正文 schema。

**壳：** 编制步 `find=false`；左栏标题树 + 拖拽 + 悬停 `⋯`（已起步，收齐样式）；右栏 Mantine Tabs；顶栏 `Select` + `Button`。灰底/卡片/选中描边去掉，文稿铺满 `bench-main`。

**不做：** Markdown 源、报价/台账、后端 `kind=content` accept、整页换编辑器栈。

## Files to modify

- `web/src/bid/authoring/DocumentCanvas.tsx` — 单编辑器连续稿
- `web/src/bid/authoring/SectionEditor.tsx` — 收成章内节点/Bubble，或删除若并入 Canvas
- `web/src/bid/authoring/adapter.ts` — 整篇 doc ↔ 树+块；新增 node
- `web/src/bid/authoring/contentBlock.ts` — `RichNode` 扩 quote/code/hr
- `crates/bidding/src/content_block.rs` + `render_v2.rs` — 同扩、DOCX/PDF 能渲
- SQL 校验块 JSON 的函数（`migrations/bidding_v2_baseline.sql` 里 `block_kind`/content check）— **只加新 baseline 片段，不重跑 migrate**
- `web/src/bid/authoring/OutlineTree.tsx` — 树样式收齐
- `web/src/bid/authoring/InspectorPanel.tsx` — Mantine Tabs
- `web/src/bid/Workbench.tsx` — Mantine 顶栏
- `web/src/app.css` / `web/src/theme.ts` — 编制面 token
- 新增 `web/src/bid/authoring/Chapter.ts`（Tiptap Node）+ `slashMenu.tsx`
- `web/src/bid/authoring/*.test.ts` — adapter 整篇往返

依赖（执行阶段才装）：`@tiptap/extension-placeholder`、`@tiptap/suggestion`、`@tiptap/extension-blockquote`（若未随 StarterKit 开）、code-block 已在 StarterKit。

## Reuse

- `session.ts`：`renameNode` / `moveNode` / `insert*` / `editRichText` / `editTable` / `uploadAsset` / `save` / `DraftMap`
- `tree.ts`：`dropMove`、`flattenPreorder`
- `blocks.ts`：`blocksForNode`
- 已有 Tiptap Table/Link/Underline/`EvidenceRef`
- Mantine `Button` `Select` `Menu` `Tabs` `ActionIcon` `Modal`
- `theme.ts` lake 主色

## Steps

- [ ] **Schema：** `RichNode` 增加 `blockquote` / `code_block` / `horizontal_rule`；Rust + TS 校验 + renderer 段落/等宽/分隔线；SQL check 与前端 `canonicalContentJson` 对齐。不重跑 migrate，对新库走 baseline，对现库 `CREATE OR REPLACE` 校验函数。
- [ ] **Chapter 节点 + 整篇 adapter：** `outline + blocks` → 一个 Tiptap doc；`onUpdate` 按 chapter 拆回块数组，未变块不写 draft。
- [ ] **DocumentCanvas：** 单 `useEditor`，白纸铺满；章标题行；Slash/Floating 插入；Bubble 格式；去掉自制插入条。
- [ ] **整篇可编：** 删除「非聚焦章 StaticBlock」路径；左树只滚动不高卸载。
- [ ] **大纲：** 标题树、拖拽、`⋯`、双击改名；无 ⌘K。
- [ ] **顶栏/右栏：** 全部 Mantine；生成/填充/保存留在 Stepper 旁，高度/圆角/字重与 `theme.ts` 一致。
- [ ] **测试：** adapter 往返（含 quote/code/hr/table）；打字 poll 不丢字；`npm --prefix web test && npm --prefix web run build`；产物拷进 `knowledgebrain-api:/web`。

## Verification

- 编制页：左树、中连续白纸、右检查器；无灰卡片、无原生 select/button。
- 不点章节也能在任意章打字；点树只滚动。
- `**`、列表、`>`、` ``` `、表格、插图、分页、签章可用；导出 DOCX 看得到引用/代码/分隔线。
- `#` 不会在正文里长出新大纲节点；改章名只改树标题。
- 生成大纲 overlay、生成本章、保存、刷新不丢字。
- Network 新 `index-*.js` 来自容器 `/web`。
