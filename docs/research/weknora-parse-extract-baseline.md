# WeKnora 解析 / 抽取对照基线

| 项 | 值 |
|---|---|
| 状态 | **调研快照**（供后续对齐跟踪，不是本仓规格） |
| 快照日期 | 2026-08-20 |
| 对照对象 | [Tencent/WeKnora](https://github.com/Tencent/WeKnora) |
| 默认分支 | `main` |
| **main HEAD（快照时）** | `ba03be775f52ee2179c4a6ec6c5356bec28662cc`（2026-08-20T11:51:10Z，`feat(system-admin): add endpoint for creating users (#2722)`） |
| **发布标签** | `v0.7.2`（2026-08-07） |
| **v0.7.2 commit** | `3d5d8bfcdfeeea266b292b71cea616847af28d0f`（lightweight tag，2026-08-07T03:37:05Z） |
| 上一标签 | `v0.7.1`（2026-07-24）、`v0.7.0`（2026-07-17） |
| 本仓规格 | `docs/knowledge-base/domain.md`；实现对照见 `docs/research/repository-implementation-snapshot.md` |
| 本仓投标 | `docs/bidding/domain.md` |
| 本仓历史改进方案 | `plans/archive/platform/parse-extract-weknora-parity-legacy.md`（已执行一轮；因混合多个领域现已归档） |

本文只记录 **WeKnora 公开实现在快照日的机制**，以及当时对本仓「抽不全 / 内容不对」的对照结论。  
**不是** KnowledgeBrain 的运行规格；领域规格以 `docs/knowledge-base/domain.md` 为准。

## 0. 如何续跟

```bash
# 当前 main
curl -sS https://api.github.com/repos/Tencent/WeKnora/commits/main | jq -r '.sha,.commit.committer.date,.commit.message'

# 最新 release
curl -sS https://api.github.com/repos/Tencent/WeKnora/releases/latest | jq -r '.tag_name,.published_at'

# 相对本快照的提交
#   main:    ba03be775f52ee2179c4a6ec6c5356bec28662cc
#   v0.7.2:  3d5d8bfcdfeeea266b292b71cea616847af28d0f
```

续跟时在文末「变更日志」加一行：日期、新 SHA/tag、改了哪条机制、对本仓是否仍适用。  
本仓 fork 对照路径：`/opt/workspace/code/brain`（若仍在）。

证据范围（快照日）：WeKnora 公开仓库与文档。**没有**把 KnowledgeBrain 源码送进那次抓取；本仓差距以当时阅读 + 后续历史计划 `plans/archive/platform/parse-extract-weknora-parity-legacy.md` 为准。

---

## 1. WeKnora 是什么

企业级 LLM 知识框架：原始文档 → 可查询 RAG、可编排 Agent、可自维护 Wiki。

- 仓库：https://github.com/Tencent/WeKnora  
- 产品：https://weknora.weixin.qq.com  
- 文档站：仓库内 `website-docs/`（v0.7.2 起正式 VitePress）

三条能力：RAG 问答、ReAct Agent（检索 / MCP / 联网）、Wiki Mode（文档蒸馏成互链 Markdown + 图谱，可编辑、有修订、可回滚）。

核心三进程：`app`（Go :8080）、`frontend`（Nginx + Vue3）、`docreader`（Python gRPC :50051），外加 Postgres / Redis。检索默认 ParadeDB（BM25 + pgvector）。Neo4j 可选。摄入走 Asynq（Redis）异步流水线。

---

## 2. 症状对应哪一层

「提取不全、内容不对」在 WeKnora **不是同一阶段**：

| 阶段 | 产出 | 失败时的典型症状 |
|---|---|---|
| 文档转换（Python docreader） | Markdown + 图片引用 | 缺页、乱序、表格丢格、扫描页空白 |
| Go 侧 OCR / VLM / ASR | 图/扫描页文字与 caption | 图里字没了，但文本页看起来「成功」 |
| 切块 | parent/child chunks | 段落切断、检索碎片、上下文错位 |
| 后处理提取 | 摘要、问题、图谱、Wiki | 实体/关系不全，但原文 chunk 其实在 |

**docreader 是轻量 sidecar**：只把文件/URL 转成 Markdown + 原始图片引用；**不做 OCR、VLM caption、切块、对象存储上传**。扫描 PDF 先渲 JPEG，再交给 Go 做 OCR/VLM。

若把「解析」和「提取」揉在一个函数里：转换丢了内容，后面的 LLM 只能在残缺文本上工作，看起来像「提取不全」。

---

## 3. 架构：分阶段状态机

任务类型：`document:process`、`knowledge:post_process`、`chunk:extract`。

解析状态：`pending / processing / finalizing / completed / failed / deleting / cancelled`。

- `processing` = DocReader / 切块 / embedding  
- `finalizing` = 主解析完成但摘要/问题/图谱仍在飞，**此时文档已可查询**  
- `completed` 要等全部富化子任务结束  
- `cancelled` 保留已写 chunks/index，方便重解析  

UI 时间线五阶段 DAG：`docreader → chunking → (embedding ∥ multimodal) → postprocess`。切块失败级联取消后续。span 在 `knowledge_processing_spans`。

对照含义：没有分阶段状态、没有 `finalizing`、没有「失败可见但仍可查」，就很难区分「解析丢了」还是「提取没跑完」，也容易把部分失败显示成整体成功。

---

## 4. 八条机制（快照清单）

后续跟踪时逐条打勾：WeKnora 是否改了、本仓是否已对齐。

### 4.1 转换与理解拆开

Python 只出 Markdown + 图；OCR / VLM / 切块 / 抽取全在 Go。混在一起时，扫描页空白会被当成「提取失败」。

本仓：知识库 `document:process` 已拆；投标 `bid:convert` 仍把 VLM 内联进转换。

### 4.2 PDF 逐页数字页 vs 扫描页 + XY-cut

builtin PDF **逐页**路由（主信号图像面积占比，默认 `SCAN_IMAGE_AREA_RATIO=0.5`）。混合 PDF：数字页出原生文本，扫描页出 JPEG 且 `image_source_type=scanned_pdf`，由 Go OCR/VLM；docreader **从不跑 OCR**。默认开版面感知 XY-cut（`DOCREADER_PDF_LAYOUT_ORDERING`）。pdfium 全局锁串行。

本仓：builtin 路径已有（`services/docreader/parser/pdf_parser.py`）。版面模型（MinerU / Paddle）**本期不部署**。

### 4.3 可插拔版面引擎

Go 注册：`builtin`、`simple`、`anydoc`、`weknoracloud`、`mineru`、`mineru_cloud`、`paddleocr_vl`、`paddleocr_vl_cloud`。Python 注册：`builtin`、`markitdown`、`opendataloader`。未知引擎名打到 docreader。

MinerU 自托管超时约 1000s，`return_md=true, return_images=true`。PaddleOCR-VL：`POST {endpoint}/layout-parsing`。

本仓：`crates/docparser` 已有 HTTP 客户端与 catalog；运行时 `KNOWLEDGEBRAIN_MINERU_ENDPOINT` / `PADDLE` 为空。**有意暂缓。**

### 4.4 解析器链式回退

- `.docx` 若字节以 OLE 魔数 `D0 CF 11 E0` 开头，改走 DOC parser  
- `Docx2Parser`：先 MarkItDown，失败再 DocxParser  
- MarkItDown：PPT 先 normalize；DOCX 先填纵向合并单元格；`keep_data_uris=True` 失败再关 URI 重试  

风险：FirstParser 把「非空字符串」当成功，典型中文 `.docx` 往往走不到 DocxParser 的 HTML 表格。

### 4.5 失败可见性（#2537）

图 OCR/caption 失败必须有独立错误（`ocr_error` / `caption_error`），不能 `parse_status=completed` 且 0 图块。公开 issue **#2537**：GPT-5 / o-series 误发 `max_tokens`，图全失败仍显示成功，`chunks_created=0`。Closed，关联 #2614。

本仓：投标文件行已有偏瘦/需复核；知识库最后一次 VLM 失败写 `finalizing`，但仍可能被 postprocess 打成 `completed`；无 VLM 时 enrichment 仍 stub OCR 文案。

### 4.6 切块是独立产品质量

生产切块在 Go `internal/infrastructure/chunker`（WeKnora）/ 本仓 `crates/chunker`。策略 `auto / heading / heuristic / recursive|legacy`。默认 `chunk_size=512`、`overlap=80`（~15%）。父子切块：小 child 入向量，大 parent 回给 LLM。有 `POST /api/v1/chunker/preview`（本仓本期不加）。v0.7.2 可编辑 chunk + 修订历史 + 自动重建索引。

投标抽取走 `build_sections`（ATX `#`），**不走**知识库 chunker。无标题文档会合成一段「正文」，又回到整篇 Agent。

### 4.7 提取按 chunk 扇出 + 重试

`knowledge:post_process` 把 `parse_status` 打成 `finalizing` 并写 `pending_subtasks_count`，再扇出图/摘要/问题/图谱/Wiki。图谱每文本 chunk 入队 `chunk:extract`（MaxRetry 3，超时 30 分钟）。Wiki Map-Reduce 的 Finalize **无 LLM**。

本仓投标：已改为 **按标题段** 扇出并增量 persist（不是 WeKnora 的 graph `chunk:extract`）。项目级「同时只能一条 running」的 unique index 曾与并行抽取冲突，属实现债。

### 4.8 表格 / 合并单元格

公开 bug（快照时已 closed，仍是高发区）：

| Issue | 现象 | 关联 |
|---|---|---|
| [#2634](https://github.com/Tencent/WeKnora/issues/2634) | Word 合并单元格表只解析出第一行有内容 | #2657 |
| [#2648](https://github.com/Tencent/WeKnora/issues/2648) | 含图 xlsx 在 `ExcelParser` / `fill_merged_cells_xlsx` 报 `'Typed' object has no attribute 'to_tree'` | #2663 |
| [#2552](https://github.com/Tencent/WeKnora/issues/2552) | PDF `PDFium: Data format error`，38ms 失败 | — |
| [#2537](https://github.com/Tencent/WeKnora/issues/2537) | VLM 失败静默，parse 仍成功 | #2614 |

xlsx 有 `fill_merged_cells_xlsx`。DocxParser 的 colspan 是相邻相同文本，**无 `vMerge`**。builtin PDF 无表格重建。

---

## 5. 对本仓的含义（快照结论）

知识库管线骨架已与 WeKnora 同构（任务类型、`parse_status`、分块、Wiki/图谱）。  
**短板在投标 convert→extract，以及知识库图失败可见性。**

超过 WeKnora 的点不靠新版面引擎，而靠：招标条款 span 覆盖、quote 必须是原文连续子串、抽取增量可见、禁止假成功、lease 可复活。

本期有意不做：部署 MinerU / Paddle；难 PDF 的版面模型还原。

快照日仍开放的准确性质检（实现债，不是新调研）：

1. 转换质检未做「表格被拍扁」；无 `#` 且 ≥800 字不当 thin  
2. 知识库无 VLM 时 stub OCR/caption 仍可 completed  
3. Word MarkItDown 非空即成功，DocxParser 表格 HTML 走不到  
4. 投标扫描页 `describe_image` 未带 `scanned_pdf` prompt  
5. 黄金集 03 是合成短文，不是中煤五份去标识节选  

---

## 6. 建议的验证顺序（续跟时仍适用）

1. 先看转换产物（Markdown + 图列表），不要先看 LLM 抽取。  
2. 混合 PDF：数字/扫描是否逐页分流，扫描页是否真进 OCR（对照 #2537）。  
3. Word/Excel：合并单元格、嵌入图表（对照 #2634 / #2648）。  
4. 切块单独看：约 512 字、约 15% overlap；需要整表/整章再开父子块。  
5. 抽取按段/按 chunk，带 pending 计数和重试；总状态不要在图失败时显示成功。  
6. 难 PDF 第二引擎（MinerU / Paddle）是下一期能力，不是这一期门闩。

一句话：WeKnora 的差距不在「会不会抽」，而在 **把读文件、读版面、切块、读图、抽知识当成五条可观测、可回退、可重跑的流水线**。

---

## 变更日志

| 日期 | WeKnora 锚点 | 本仓动作 |
|---|---|---|
| 2026-08-20 | main `ba03be77…`；release `v0.7.2` = `3d5d8bfc…` | 首次写入本文；历史方案现存于 `plans/archive/platform/parse-extract-weknora-parity-legacy.md` |
| 2026-08-21 | 同上 | 方案二期：解析统一。anydoc 已集成，office 默认 anydoc（有意不同于 WeKnora 空引擎=MarkItDown）；PDF 仍 builtin。VLM 必配禁止 stub。 |
