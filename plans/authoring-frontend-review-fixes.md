# 编制前端：审查缺口修改方案

> 对照 [`frontend-authoring.md`](bidding/frontend-authoring.md) F0–F3。  
> 已拍板：内容候选要看见块本身；后端 content accept **本轮不做**；用 mock 数据把效果跑出来。

## Context

黄金路径按钮已接上，没有 P0。审查后仍要兑现：

| 问题 | 本轮 |
| --- | --- |
| 滚动画布不反标左树、不写 hash | 修 |
| 有草稿时「接受所选」静默失败 | 修 |
| 内容候选只有一行摘要 | 改成块预览（复用 `StaticBlock`） |
| 后端 `kind=content` accept 仍 400 | **等后端**；前端 toast 错误，e2e 用 mock 假装成功 |
| Inspector「人工先选」会炸 | 去掉选项 |
| 解析完成文案提「要求台账」 | 改文案 |
| 看不到效果 | e2e mock 注入完整 content candidate（正文+表+图） |

## Approach

### 1. 滚动反标

编制步有 Inspector 时，滚动容器是 `Shell` 的 `.bench-main`（`overflow: auto`）。`DocumentCanvas` 把 `IntersectionObserver` 绑在 `.ed-doc` 上；父级不是限高 flex 时 `.ed-doc` 跟着内容长高，observer 看不到视口。

改法：`root` 取 `scrollRef.current?.closest(".bench-main")`，没有再退回 `null`（viewport）。点树 `scrollIntoView` 不动。

### 2. 接受候选 + 草稿

`acceptCandidate` 在 `try/fail()` **之前**抛 `SAVE_DRAFTS_FIRST`，点击是 `void session.acceptCandidate()`，toast 只看 `state.error` → 无提示。

改法：

- `CandidateReview`：`hasDrafts` 则先 `await session.save()`，再 `await session.acceptCandidate()`（两次独立 `enqueue`）。**禁止**在 `acceptCandidate` 里 `await save()`（`mutationTail` 会死锁）。
- `SAVE_DRAFTS_FIRST` 改到 `fail()` 里，草稿仍脏会 toast。
- 不关编辑器。
- `exportDocument` 同样先 `save()`。
- 真后端 content accept 仍 400 时走现有 `fail()` toast，不伪装成功。

### 3. 内容候选要看见块（不是一行字）

不是像素级左右 diff，是 **拟写入的块按正文样子渲染**，旁边勾选：

- 每条 operation：勾选 + `op.kind` 短标签 + `StaticBlock` 渲染 `op.block`。
- `rich_text`：段落/列表；`table`：表格；`image`：caption/alt（本轮没有对象 URL，不接真实图字节）。
- 默认全选（`hydrateCandidate` 已如此）。
- 大纲候选仍是带层级的标题勾选树。

### 4. Mock 看到效果

不改后端。加宽 `web/e2e/bid-v2-flow.spec.ts` 的 `page.route`：

- `GET /candidates/:id`：outline 与 content 按 id 分（`cand-outline` / `cand-content`）。
- content mock 至少三条 operation：一段中文 `rich_text`、一张小表、一条 `image`（caption）。
- `GET /requests/:id` 按 request id 返回对应 kind。
- 点「填充全部空章」后断言 `candidate-review` 里能看到那段正文和表格，不只 `insert_block · …`。
- 「接受所选」走 mock `POST /candidates/` 成功（不打真后端）。
- 再点导出按钮（不必真下载文件）。

单测：`session.test.ts` hydrate content candidate 后 `selectedOperationIndexes` 为全选。

### 5. 小伤

- Inspector 只留「系统建议」，删 `user_pick_set`。
- `fileStage` 已解析：`可以生成大纲或进入编制。`

## Files to modify

- `web/src/bid/authoring/DocumentCanvas.tsx`
- `web/src/bid/authoring/CandidateReview.tsx`
- `web/src/bid/authoring/session.ts`
- `web/src/bid/authoring/InspectorPanel.tsx`
- `web/src/bid/helpers.ts`
- `web/src/bid/authoring/session.test.ts`
- `web/e2e/bid-v2-flow.spec.ts`

## Reuse

- `StaticBlock`（`StaticBlock.tsx`）渲染候选块，不要新开 Markdown/GFM
- `hasDrafts` / `save` / `enqueue` / `fail`（`drafts.ts`、`session.ts`）
- `hydrateCandidate` 已默认全选 operation indexes
- `ContentBlockV1`（`contentBlock.ts`）直接作为 mock payload
- e2e 现有 `mockApi` / `workspace()` / `login`

## Steps

- [x] Observer `root` → `.bench-main`
- [x] `CandidateReview`：内容操作用 `StaticBlock` 预览 + 勾选
- [x] 接受：先 `save()` 再 `acceptCandidate()`；`SAVE_DRAFTS_FIRST` 走 `fail()`
- [x] `exportDocument` 前 `save()`
- [x] 去掉 `user_pick_set`；改 `fileStage` 文案
- [x] `session.test.ts`：content hydrate 全选
- [x] e2e mock：填充后看见正文/表；接受（mock）；点导出

## Verification

- 滚动画布：左树高亮跟着变，hash 为 `#/bids/:id/authoring/:lineageId`
- 聚焦章打字后立刻「接受所选」：先保存，有 toast 或成功，编辑器不关
- 点「填充全部空章」（e2e mock）：候选区出现整段文字和表格，不是一行摘要
- 真后端 content accept 仍失败时有 toast（不假装写入）
- Inspector 没有「人工先选」
- 解析完成芯片不再出现「要求台账」
- `npm --prefix web test`
- `npm --prefix web run test:e2e`（浏览器可用时）

## 刻意不做

- 后端 `kind=content` accept 真正 apply（等后端）
- 像素级新旧对照 diff、真实图片字节
- Quote 向导、台账、Document Settings
- 删除未引用的 `QuotePane` / `RequirementsPane` / `PreviewPane`
- 把测试跑手改成 vitest CLI
