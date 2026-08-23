# 投标主路径修复

## Context

用户要的完整业务是：

1. 创建项目  
2. **马上**上传招标文件  
3. 解析招标文件，抽出商务 / 技术等条款  
4. 填充推荐（商务命中/缺件，技术段候选+覆盖率）  
5. 审核和修改（确认、勾选、人评、手补、去补证）  
6. 预览和导出（成稿 Word / 定稿 PDF）

当前断点（已用新建标 `b91e275a-…` 抓 DOM 核实）：

| 现象 | 原因 |
|---|---|
| 建完标看不到上传 | `go(/bids/:id)` 默认 `view=commercial`，页上只有手补 |
| 点上传没反应 | 上传只在 `?view=files`；空态大卡片不可点，人点不到 Dropzone |
| 解析/推荐接不上 | 文件页与评估脱节；抽取 LLM 常 `tool=stop` 出 0 条；UI 不说明下一步 |
| 流程不像投标 | 无阶段，无文件也能进评估/成稿 |

后端上传/解析本身可用：`POST /documents` 201，convert 完成后 worker 会 `enqueue_bid_extract`，抽完后 `coverage_sweep` 会补扫。**不改 `api.ts` 契约。**

工作区里已有未收束的前端半成品（建标跳文件、FilesPane 选文件按钮、空标弹回文件）。落地时按本方案收束或改齐，不另开一套。

## Approach

前端做成**有闸的一张工作台**：无文件必须停在上传；文件在解析/抽取时停在文件页看状态；有条款再进评估。抽取仍走现网 worker；Agent 出 0 条时保证 sweep 跑完，UI 允许「再抽一次 / 手补」，流程不卡死。

```
新建标
  → 招标文件页（唯一上传入口）
  → 上传 → 排队 / 解析中 / 已解析 / 失败
  → convert 完自动抽条款（已有）
  → 有 draft 后进入评估商务（默认）
  → 确认 → 匹配推荐 → 勾选 / 人评 / 补图
  → 成稿预览/编辑 → Word / 定稿 PDF
```

无文件点「评估」也回到文件页。有文件后商务 / 分段 / 成稿仍可来回切，不是锁死向导。

## Files to modify

- `web/src/App.tsx` — 创建成功进文件页  
- `web/src/bid/Workbench.tsx` — 阶段闸、上传态、抽取提示、抽完再进评估  
- `web/src/bid/FilesPane.tsx` — 可点的上传（Dropzone +「选择文件」+ 空态同一入口）  
- `web/src/bid/ClauseTable.tsx` — 空态按阶段给下一步，不再只写手补  
- `web/src/bid/Sidebar.tsx` — 文件角标用解析状态，不只是个数  
- `web/src/app.css` — Dropzone 可点样式、页头四步  
- `web/src/hash.ts` / `web/src/bid/helpers.ts` — 只复用，不改路由语义  
- `crates/bid/src/lib.rs` — **仅当**验收时 Agent+sweep 对简单中文标仍 0 条：Agent 出空后强制再跑 `coverage_sweep` / `run_extract_section`（已有函数）。不重写抽取智能体。

不改：`web/src/api.ts`、登录、资料/产品库、成稿编辑器内核。

## Reuse

- `bidHref` / `parseBidRoute` / `go` — `web/src/hash.ts`  
- `api.uploadDoc` / `docs` / `reextract` / `clauses` / `patchClause` / `addClause` / `picks` / `downloadExport` — `web/src/api.ts`  
- `fileLabel` / `liveClauses` / `unitIdForView` — `web/src/bid/helpers.ts`  
- Mantine `Dropzone` + `openRef` — 已在 FilesPane  
- 4s `load()` 轮询 — Workbench 已有  
- convert 完自动 extract — `crates/worker/src/consume.rs` `BidConvertWorker`  
- `coverage_sweep` / `run_extract_section` — `crates/bid/src/extract_agent.rs`、`lib.rs` 约 939–955 行已接  

## Steps

- [ ] **阶段**  
  由 `docs` + `derived` + `clauses` 算出：`upload`（无文件）→ `parse`（有文件未齐或抽取中）→ `eval`（有条款）→ `booklet`（人在成稿）。页头四步：上传 / 解析 / 评估 / 成稿。

- [ ] **建项即上传**  
  创建成功 `go(bidHref(id, "files"))`。打开标时若 `docs.length===0 && clauses.length===0` 且当前是默认商务，跳到文件页。

- [ ] **上传必须有反应**  
  Dropzone 整块可点；`openRef`「选择文件」；空态同一按钮。上传中：toast + 按钮禁用 + 列表先出现「上传中」行。`onDrop` 调现有 `api.uploadDoc`（字段名 `file`）。失败 toast 红字。

- [ ] **解析可见**  
  文件行四态：排队 / 解析中 / 已解析 / 失败。失败可重试/删。全部 completed 后文案改为「正在抽条款」。`files_ready && !extract_running && clauses===0` 时 **每标只踢一次** `api.reextract`（防 LLM 抽空死循环）。

- [ ] **抽完进评估**  
  仅「本次上传」后：`files_ready && !extract_running && clauses.length>0` 再跳商务并 toast。人主动点「文件」看补遗不踢走。抽完仍 0 条：留文件页，按钮「再抽一次」+「去评估手补」。

- [ ] **评估空态**  
  无文件 → 去上传；解析/抽取中 → 等；抽完 0 条 → 再抽 + 手补。有条款后：行内确认、缺件去补证、检查器勾选/覆盖率/人评/补图，长技术条款深挖。逻辑沿用现 Inspector / ClauseTable。

- [ ] **推荐**  
  确认后走现网 match（`derived.match_running` 横幅已有）。商务 hit/miss；技术段 `picks`+`candidates`。不新造满足度大分。

- [ ] **成稿导出**  
  成稿仍是预览/编辑 + Word / 定稿 PDF。无文件时成稿侧栏可点，但主列提示先上传。

- [ ] **抽取兜底（仅验收失败时动 Rust）**  
  对一份含「营业执照 / ISO9001 / IP65」的 `tender.txt`：若 convert 成功而 clauses 仍 0，检查 worker 是否跑了 sweep；Agent 空结果时确保 `coverage_sweep` 仍执行。不改工具协议、不加新表。

- [ ] **发布**  
  `npm -C web run build`（容器已挂 `web/dist` → `/web`）。仅当动了 `crates/bid` 才 `--build` worker。

## Verification

1. 浏览器打开 `http://127.0.0.1:28080/`，空账密进入。  
2. 新建标 → URL 含 `view=files`，标题「招标文件」，点「选择文件」弹出系统选框。  
3. 上传 `tender.txt`（资格+技术各两句）→ 立刻有行，状态从排队到已解析。  
4. 抽取结束后评估出现商务/技术 draft（或明确「0 条，再抽/手补」）。  
5. 确认一条商务；缺件可点去补证进资料。  
6. 技术未归段或分段：检查器有候选或「确认后才匹配」。  
7. 手补一条并确认。  
8. 成稿预览/编辑，导出 Word 有文件下载。  
9. 再进「文件」加补遗，不被立刻踢回评估。  
10. 回归：资料四夹上传、产品线建型号仍可用。

命令：`npm -C web run build`；`curl` 建标/上传/clauses/export；headless 抓新建标 DOM，断言默认页有 `type="file"` 和「选择文件」。

## Out of scope

- 重写抽取智能体 / 换模型 / 改 prompt 大段（见 `plans/bid-extract-agent-redesign.md`）  
- 改 `api.ts` 字段或新建后端资源  
- ⑥ 函报价、包件、Org  
- 要求点独立存库  
