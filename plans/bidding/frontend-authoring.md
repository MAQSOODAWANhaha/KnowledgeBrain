# 投标编制前端：Word 式大纲 + 正文画布

> 产品契约：[`../../docs/bidding/authoring.md`](../../docs/bidding/authoring.md) §2.4 / §11.4  
> 后端阶段：[`tender-to-submission-v2.md`](tender-to-submission-v2.md)  
> 本文只排 **如何落地** 已确认的 Web 编制面，不重新定义交互规则。

## 已确认选择

- 编制过程 **没有业务锁**。导出当前 `WorkspaceRevision`；改完再导出是新文件。
- 大纲第一期就做完整导航：点击跳转、拖拽调序/层级、增删改名拆合。
- AI 大纲候选只 overlay diff（默认全选、可取消节点），永不直接换树。
- 范围只盯黄金路径，不做报价/台账/设置精编。
- 编辑器：**Tiptap 能满足需求，采用 Tiptap**。章标题来自树；正文 Tiptap 关闭 heading。只给聚焦章挂活编辑器，其余章静态渲染。

## Context

最终目标只有这一条链：

```text
上传招标文件 → 解析 → 生成大纲 → 从知识库填充内容 → 导出投标文件
```

用户随时改大纲和正文，改完可以再导出；**没有业务锁**。导出 = 当前 `WorkspaceRevision` 的快照。Assessment 只提示。AI 只出 Candidate overlay，不直接改树或正文。

现有代码已经有壳，不是从零开始：`Workbench` 三栏、`OutlineTree` 按钮树、`SectionEditor` 单章 Tiptap、`DraftMap` autosave、`editorAdapter.ts`。缺的是 **整篇画布 + 左侧导航联动 + 拖拽大纲 + 把 Candidate 灌进 UI**。旧 `gfm.tsx` Markdown 预览 **不是** V2 编辑真源，不要往里面嵌大纲。

## Approach

### 不要把大纲嵌进 Markdown 源码

Word 导航栏 **不是** 从一篇 `# 标题` Markdown 里抽出来的。V2 同样：

```text
OutlineNode（独立树，有 lineage / 父子 / ordinal / semantic_role）
        ↕ 点击跳转、拖拽改树、滚动高亮
DocumentCanvas（按树前序把各章叠成一篇）
        └── 每章的 ContentBlockV1
                ↑ 唯一转换：editorAdapter.ts
            Tiptap JSON（仅内存，WYSIWYG，不是 Markdown 源）
```

| 错误做法 | 为什么不行 |
| --- | --- |
| 一整篇 Markdown，`#` / `##` 当大纲 | 没有稳定 `node_lineage_id`；生成「本章」、知识绑定、拆合、CAS 都会碎 |
| 一个巨型 Tiptap doc，heading 当树 | 和封闭 `ContentBlockV1`、按节点生成、表格/图片块冲突 |
| 继续用 Markdown textarea + `gfm.tsx` | 已废弃；V2 真源是块，不是 GFM 字符串 |

**现在就能做。** 依赖已在：`@tiptap/react`、Table/Link/Underline、`OutlineTree`、`Shell` 左槽。不缺编辑器库。缺的是把「单章 SectionEditor」换成「整篇画布 + 导航同步」。

### 布局（一开始就按完整 Word 导航做）

```text
┌─────────────┬──────────────────────────────┬─────────────┐
│ 大纲导航     │ 投标文件画布                   │ 知识 / 提示  │
│ OutlineTree │ DocumentCanvas                │ Inspector   │
│             │                               │             │
│ 点击 → 滚到该章│ 前序展开全部章节               │ 证据、匹配   │
│ 拖拽 → 调序/层级│ 章标题 = 树节点，不是 # heading│ 生成状态     │
│ 增删改名拆合  │ 点某一章 → 该章 Tiptap 可编     │             │
└─────────────┴──────────────────────────────┴─────────────┘
步导航：文件 / 编制 / 导出。编制画布工具条：生成大纲 · 填充（本章/全部空章）· 导出 DOCX/PDF
```

向导收成三步，不再让用户理解冻结/台账/checkpoint：

1. **文件** — 上传，看解析状态  
2. **编制** — 生成大纲、改树、填充、一直改  
3. **导出** — 导出当前稿；改完再导一份

点「生成大纲」时前端顺序：文件未冻则先 `freezeDocumentSet`，再 `createOutlineCandidate`。checkpoint 若后端生成内容需要，在用户点「填充」时静默建，**不出现「确认后才能改」的界面。**

### 中间画布：怎么「嵌」进编辑器

不是嵌进 Markdown，是 **树驱动的一篇连续稿**：

```text
for node in outline.preorder():
  <section id={lineage_id} data-depth={depth}>
    <h{min(depth+1,3)}>{node.title}</h>     // 只展示树标题
    for block in node.blocks:
      if node == focused:  活 Tiptap / 表 / 图卡片
      else:                只读渲染，点击 → 聚焦并开始编
```

- 左侧点击：`scrollIntoView(section)` + 该章聚焦可编。  
- 画布滚动：`IntersectionObserver` 反标左侧当前章，并写 hash `#/bids/:id/authoring/:lineageId`。  
- 性能：投标文件章节多，**只给聚焦章（及可选相邻章）挂活 Tiptap**；其余章静态渲染。点哪编哪，整篇仍连续可滚，感觉像 Word。  
- 章标题禁止在正文 Tiptap 里再插 heading；改标题只改左侧树。

拖拽（第一期就做完整，不当二期）：

- 节点 `draggable`；放置指示：之前 / 之后 / 成为子节点。  
- drop 调已有 `session.moveNode(id, parent, ordinal)`（禁移到自己的子孙，单根）。  
- 上移下移升降级按钮保留作键盘/无障碍备份。  
- 不新增状态机；拖拽只是 `move_node` 的指针 UI。

### 黄金路径交互

```text
上传 PDF/DOCX/XLSX/图片
  → 轮询 parse_status（可边解析边等）
  → 「生成大纲」
       freeze（如需要）→ OutlineGenerate
       CandidateReview 树 diff，默认全选，可取消节点（已确认方案 A）
       接受后写入树；期间用户若已改树 → 候选过期，人改的留下
  → 用户可立刻改树、改字（生成中也不 disable）
  → 「填充全部空章」或「生成本章」
       ContentGenerate(system_proposed) 同一 job 内检索知识库并出 Candidate
       文本/表/图 diff，部分接受
  → 继续改
  → 「导出 DOCX/PDF」= 当前 WorkspaceRevision
  → 再改 → 再导出（新 export，不是改旧文件）
```

知识填充：默认系统建议证据，不单独做「匹配」向导步。右侧 Inspector 只给当前章证据预览和「匹配资料」补入口。

报价、Document Settings、结构化表单精细编辑、人工 PickSet、要求台账改分类：**本计划不做。** 空章可以先人工打字；表单块先当普通表格。

### 保存与导出（没有锁）

- 大纲：`move/insert/rename/...` 立即 CAS（lineage 是跳转和「生成本章」的身份）。  
- 正文：`DraftMap` + 800ms autosave；409 保留本地字，选保留或用服务器。  
- `refresh`/轮询必须进 `mutationTail`；有未保存 draft 时 **禁止** `applyWorkspace` 冲稿。  
- 导出不检查业务是否「完成」；技术错误（schema/资产缺失）才失败。  
- `project ended` 仅表示项目结束、不再 mutation；不是编制过程中的锁。

## Files to modify

新增：

- `web/src/bid/authoring/AuthoringShell.tsx` — 编制步三栏  
- `web/src/bid/authoring/DocumentCanvas.tsx` — 整篇前序画布 + 滚动同步  
- `web/src/bid/authoring/CandidateReview.tsx` — 大纲树 / 正文块 / 图片候选  
- `web/src/bid/authoring/StaticBlock.tsx` — 非聚焦章只读渲染  
- `web/src/bid/authoring/*.test.ts` — tree drop / adapter / drafts / session（加 vitest）

改：

- `OutlineTree.tsx` — HTML5 拖拽放置指示；点击只负责跳转  
- `SectionEditor.tsx` — 收成「聚焦章的块编辑器」，由 Canvas 调用  
- `session.ts` / `useBidV2Session.ts` — 轮询不冲稿；hydrate candidate；生成大纲自动 freeze  
- `Workbench.tsx` — 黄金路径三步：files / authoring / export  
- `InspectorPanel.tsx` — 当前章证据；去掉当主流程的台账  
- `api/client.ts` — 用 request result 拉 `getCandidate`  
- `e2e/bid-v2-flow.spec.ts` — 上传→生成大纲→接受→打字→导出；生成中仍可改；再导出

复用不动：`mutations.ts`、`tree.ts`、`adapter.ts`、`contentBlock.ts`、`drafts.ts`、`generation.ts`、`EvidenceRef.ts`、`PreviewPane.tsx`（独立预览仍走后端 HTML，不读 Tiptap）。

## Reuse

- 树不变量与 `moveNode`：`web/src/bid/authoring/tree.ts`、`session.ts`  
- Tiptap ↔ 块：`adapter.ts`（未知 node 失败）  
- 草稿 ack：`drafts.ts` `clearAckedDrafts`  
- 左槽布局：`Shell.tsx` `tree` / `inspector`  
- 上传：`FilesPane.tsx`  
- 导出按钮：`ExportPane.tsx`（去掉 Gate 文案依赖即可）

## Steps

### F0 — 会话不冲稿、导出当前稿

- [x] 轮询与 mutation 同一队列；有 draft 时只拉 job/candidate  
- [x] 生成中 / 有 candidate / 已 checkpoint：**都不** `setEditable(false)`  
- [x] 导出只带当前 `expected_workspace_revision_id`，警告不 disable 按钮  
- [x] 单测：打字中 poll 不丢字；树拖拽与 autosave 串行

### F1 — 整篇画布 + 完整大纲导航（无 AI 也能写）

- [x] `DocumentCanvas` 按树前序渲染全部章节；聚焦章活编辑，其余只读可点  
- [x] 左树点击跳转、滚动反标、hash 同步  
- [x] 拖拽：之前 / 之后 / 降为子节点；非法 drop 拒绝  
- [x] 增删改名、拆合对话框（去掉 `prompt`）  
- [x] 空树可手建根章并打字、插表、插图（上传资产 → `insert_asset_block`）  
- [x] 刷新后树和正文一致

### F2 — 生成大纲 overlay（方案 A）

- [x] 「生成大纲」= 需要时 freeze + POST outline-candidates  
- [x] pending 横幅不挡住画布  
- [x] `CandidateReview` 建议树，默认全选、可取消节点；拒绝/过期不改用户树  
- [x] 接受后画布出现各章标题，可立刻改名、拖拽、打字

### F3 — 知识库填充 + 导出

- [x] 「填充全部空章」「生成本章」→ ContentCandidate；系统建议证据  
- [x] 文本/表格 diff、图片缩略图、部分接受；有 draft 则先保存再接受（提示，不关编辑器）  
- [x] 接受后可继续改，再点导出；第二次导出走新 export id  
- [x] E2E mock 用例已写黄金路径（无 `parts` / 无 Gate）；本机 Playwright 浏览器下载失败，未实跑

## Verification

- 画布：点左侧第三章，中间滚到第三章且可打字；拖到第二章下成为子节点，画布顺序跟着变  
- 不是 Markdown：仓库检索编制路径不再把 `gfm`/textarea 当编辑真源  
- 生成中改名不丢；candidate 过期不覆盖人改  
- 导出当前稿，改一字再导出成功  
- `npm --prefix web run lint && npm --prefix web run build`；新 vitest；`npm --prefix web run test:e2e`

## 刻意不做（本计划）

要求台账精编、报价向导、Document Settings 面板、人工选证、结构化表单专用编辑器、独立全文预览打磨、fulfillment binding UI。后端仍会 freeze/compile/checkpoint；前端不做成用户必须理解的步骤。
