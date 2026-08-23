# 解析 / 抽取：对齐并超过 WeKnora（历史混合计划）

> 归档说明：本文同时包含知识库解析、招投标抽取和共享 worker/lease，保留用于追溯，不再作为任一领域的活动计划。知识库当前入口见 [`../../knowledge-base/README.md`](../../knowledge-base/README.md)，招投标当前入口见 [`../../bidding/README.md`](../../bidding/README.md)。

## Context

知识库管线已与 WeKnora/brain **同构**（迁移前实现快照 `docs/research/repository-implementation-snapshot.md`：oxana 任务、`parse_status`、`crates/chunker` auto/heading/heuristic/legacy + parent-child、`crates/docparser` ReadStream / MinerU·Paddle 客户端）。

**短板在投标，以及知识库的失败可见性。** 中煤 8×docx：转换全 `completed`，抽取一份 20 分钟、109 round、50 fallback、covered_spans=50 仍大量 `fallback_reasons`。整篇 Agent + 项目级串行 + lease 重启死锁，表现为抽不全、抽得慢、界面假忙。

已拍板：

- 解析一条、落盘两条（知识库索引 / 招标抽条款）
- **不部署** MinerU / Paddle（这一期）
- 验收以 **完整性 / 准确性** 为硬门（黄金集召回）；性能尽力优化，慢不单独否决
- **VLM 必须配置**（真多模态模型，可与 chat 不是同一个）；未配或失败 → 可见错误，禁止 stub 假字 + `completed`
- **解析只有一条逻辑。** 不按「知识库 / 招标文件」分叉。bytes → markdown+图 → VLM 写回，是同一个 `convert_to_markdown` + 同一张默认引擎表 + 同一套 multimodal。分叉只允许在 **落盘之后**（Document 索引 vs BidDocument 抽条款）。

超过 WeKnora 的点不靠新版面引擎，而靠：**招标条款级 span 覆盖 + quote 必须是原文连续子串**（WeKnora RAG 没有）、抽取增量可见、假成功禁止、lease 可复活。

## Approach

一期改抽取调度 / lease（已落地）。二期把 **解析收成一条**：默认引擎表、office 择优、VLM 写回都在 `docparser`/`enrichment`，`document:process` 与 `bid:convert` 只当调用方。禁止再写投标专用 convert。条款语义不动。

### 1. 投标抽取：按段扇出，禁止整篇 100+ 轮

现状：`extract_one_document` 把整份 markdown 交给 `TenderExtractionEngine::extract` → 商务/技术两个全家 Agent（各 `max_rounds=12`）再对最多 80 个 uncovered span 各扫两遍。中煤第六章 109 round 由此而来。

改为：

- `build_sections` 仍按标题切段（已有）
- **每个 section 单独跑** 有界 Agent（沿用 `run_family_agent` / `run_span_sweep`，limit 仍受 `cn-tender-v2.json`）
- **每段 persist 一次**（已有 `persist_extraction_report` 按 `document_id`），评估表边抽边出条款
- 段失败记 `bid_sections.extract_status=failed` + 可见 error，不让整份 `done` 假装全覆盖
- `uncovered_spans` 写入 diagnostics；`partial_failure` 当覆盖率低于门槛（与黄金集 `technical_recall`/`commercial_recall` 同口径）
- heuristic fallback **可以补洞，但必须打标**（已有 `fallback_reasons`）；UI 与 run 状态不得把高 fallback 显示成纯成功

性能（次要）：同一项目 **多文档可并行抽取**（persist 已按 `document_id` 隔离）。项目 `extract_lock_kind=full` 改为「每文档一把锁」或允许多 `running` run（`claim_extract_run` 的 `NOT EXISTS running` 去掉，改为 per-document）。准确性不依赖串行。

### 2. 转换质检（一期已做薄质检；二期补表拍扁 + 换默认引擎）

convert 完成后写质检，不达标则 **不自动 extract**：

- markdown 过短 / 几乎无标题 / 表格疑似被拍扁 → `parse_status=completed` 仍可看原文，但 `extract` 入队带 `conversion_quality=thin`，工作台提示「转换偏瘦，抽取可能漏」
- `.doc` OLE 魔数已有知识库分流；投标 convert 复用 `crates/docparser` 同一路由（禁止再对 `.doc` 空转 pending）

不接 MinerU。难 PDF 这一期只保证失败可见，不保证版面模型级还原。

### 3. 知识库：禁止图失败假成功

对齐 WeKnora #2537：VLM OCR/caption 失败时 **不得** `parse_status=completed` 且 0 图块。

- 图任务失败写入 chunk 或 document 级 `ocr_error` / `caption_error`（字段已有 multimodal 路径则复用，补齐 API/UI）
- 主文本可检索用已有 `finalizing`；全部图失败则文档标 `partial` / 保持 `finalizing` + 可见错误，不静默成功
- 切块继续用现成 `crates/chunker`；这一期不加 preview HTTP，除非改动很小

### 4. Lease / 复活（上次事故）

- worker `main` 不要 `select!` 掉 consume_loop；抽取 Drop 把 run 打回 `pending`
- `claim_extract_run` 允许认领 **本 run 的过期 running**
- 抽取 stale 跟 30s 心跳对齐（约 90s），不要用知识库 2h10m
- 启动时把本进程 `oxanus:processing:{host}-{pid}` 里的 job 放回队列（Docker 重启 hostname+pid=1 撞车）
- unique Skip 不得挡住已 reclaim 的同一 `run_id`

### 5. 黄金集与回归

现有 `testdata/bid-extraction/cn-tender-golden-0{1,2}` + `bid_extract_eval`（quote 必须连续子串）。

- CI：`BID_EXTRACT_MODE=heuristic cargo test -p bid golden_fixture` 必过
- 增加 **中煤去标识节选** 黄金（封面/询价函/须知/评审/技术规范 各一截），阈值不低于 golden-01（技术召回 0.9、商务 0.95、precision 1、unsupported 0）
- 有模型时手跑 `bid_extract_eval`；召回不达标禁止把 hybrid 当默认成功
- 知识库：选一份带图 + 一份扫描感 PDF fixture，断言 OCR 失败时 API 露出错误而不是 completed

## Files to modify

- `crates/bid/src/lib.rs` — `extract_one_document` 按段循环 + 增量 persist；convert 质检
- `crates/bid/src/extraction/mod.rs` — `extract_section` / 覆盖率门槛；高 fallback → partial
- `crates/bid/config/cn-tender-v2.json` — 仅当段级调度需要收紧 `max_sweep_spans`（先不放宽 round）
- `crates/storage/src/bid.rs` — `claim_extract_run` 重领 running；per-document lock；`reclaim_stale_extracts` 90s
- `crates/runtime/src/lib.rs` + `jobs.rs` — 抽取 stale 常量；可选 processing 回放
- `crates/worker/src/main.rs` + `consume.rs` — shutdown 不 abort；Housekeep 用抽取 stale；知识库图失败可见
- `crates/enrichment/src` / `crates/docparser` — OCR/caption 错误上送，不假 completed
- `web/src/bid/FilesPane.tsx` + `helpers.ts` — 转换偏瘦 / 抽取 partial / 失败文案
- `testdata/bid-extraction/` — 新黄金
- `docs/knowledge-base/domain.md` — 只定义知识库领域和证据端口；招投标抽取规则由 `plans/bidding/tender-publication.md` 独立定义

## Reuse

- `TenderExtractionEngine`、`build_sections`、`candidate_spans` / `uncovered_spans`、`reconcile_candidates`、`quote_in_body`
- `persist_extraction_report`、`bid_sections.extract_status`
- `fileStage`、`derived.extract_running`、`latest_extract.diagnostics`
- `crates/chunker::{split, split_parent_child, validator}`
- `docparser` ReadStream、`.doc` OLE 分流
- `bid_extract_eval` + golden thresholds

## Steps

- [x] Lease：claim 重领、90s reclaim、shutdown Drop、processing 回放
- [x] 抽取按 section 扇出 + 每段 persist；run 级 `partial_failure` / coverage
- [x] `claim_extract_run` 改为文档级，多文件可并行
- [x] convert 质检字段 + 过瘦不静默当高质量抽取（表拍扁仍缺）
- [x] 知识库图 OCR/caption 失败可见，禁止假 completed
- [x] 黄金 01–03 + heuristic CI；中煤去标识节选仍缺
- [x] 工作台文件行展示转换/抽取/partial，不把高 fallback 画成纯完成
- [x] 回归：黄金 fixture；worker 重启中途抽取能续上；评估表在第一段 persist 后就能看见条款

## Verification（一期）

- `cargo test -p bid golden_fixture` 以及 chunker/docparser 现有测
- 手工：上传多 docx，第一段完成后评估已有条款；杀 worker 再起，无幽灵 `running`
- 不要求单份 <N 分钟

---

## 二期（已收紧，待落地）

**原则：解析不看文件是知识库还是招标。** 扩展名 + 字节决定引擎；知识库版本 `parser_engine_rules` 只是同一路由上的覆盖，不是第二条管线。

**与 WeKnora 有意不同：** 上游默认 MarkItDown（anydoc 可选编译）。本仓 anydoc 已集成；**anydoc 更强的地方必须用 anydoc**（office 表/结构），不要为「对齐上游默认」退回 MarkItDown。该抄的是注册表、扫描 PDF→builtin+OCR、失败可见。

### 引擎：一张默认表，一个函数

`domain`/`docparser` 产品默认 `parser_engine_rules`。`parser_engine_for` 唯一入口：`document:process` 与 `bid:convert` 都调它。版本/overrides 非空则覆盖默认。

| 类型 | 默认引擎 | 理由 |
|---|---|---|
| docx / doc / docm / xlsx / xls / xlsm / pptx / ppt | **anydoc** | Table/Cell → GFM 表；避开 MarkItDown「有字即成功」 |
| pdf | **builtin** | 逐页数字/扫描 + XY-cut |
| md/txt/csv/json/图/音频 | **simple**（已有） | 不经 DocReader |
| 扫描 PDF | anydoc 误选时 → **已有** builtin 回退 | 与 WeKnora `AnydocReader.fallback` 相同 |

Office 回退（写进 `docparser`，唯一入口）：

1. **成功的 anydoc 结果必须留下。** 禁止再跑一遍 MarkItDown 来「比表多谁赢」——那会违反「anydoc 更强处必须用 anydoc」，也可能被更长的拍扁散文选中。
2. 仅当 anydoc **抛错 / 空 md** → 再试 builtin，并打 `anydoc_fallback=office_error`（与扫描 PDF 回退同思想）。
3. A/B（中煤 8×docx + 知识库样例）是 **验收 anydoc 质量**，不是投票要不要默认 anydoc。变差先修渲染，不改回 MarkItDown 默认。

**不**静默重跑已 `completed` 的文档；新上传 / 用户点重解析才走新默认。

### 转换与 VLM：同一函数，不是同一条 oxana 任务

知识库 `image:multimodal` 写的是 Document chunk，招标没有 Document，**不能**把 `bid:convert` 改去入队那条任务。统一的是：

- convert 只出 markdown + ImageRef（`image_source_type` 原样留下，扫描页=`scanned_pdf`）
- 共用 `enrichment::describe_image(key, source_type)` 写回 OCR/caption
- 无图：不挡 completed
- 有图：必须已配真 VLM；未配或失败 → 可见错误，**不得** stub 假字，也不得 `completed` 后去抽条款（招标）/ 假 completed（知识库可 `finalizing` 但 API 必须露出 `ocr_error`）

Chat 可 grok；VLM 必须是视觉模型。扫描页禁止再传空 `image_source_type`（现状 `convert_document_inner` 就是空串）。

### 质检补漏

- `conversion_is_thin`：无 ATX `#` 且无 GFM 表，即使 ≥800 字也标 thin（「几乎无标题」）
- 有管道表则不算拍扁；正文很长但零 `\|` 表、源文件是 docx/xlsx → `conversion_quality=tables_flat`，不自动抽
- 知识库 Docx2（MarkItDown-first）**保留为 builtin 回退链**，不再当 office 默认。不要去改 FirstParser「非空即成功」——默认不走它更干净

### 二期步骤

- [x] `parser_engine_for` 在 `domain`；产品默认 office→anydoc、pdf→builtin；空 rules 走默认。`bid:convert` 与 `document:process` 都调它
- [x] office：anydoc 成功即用；仅报错/空才 builtin + `anydoc_fallback=office_error`
- [x] 合成 docx 表 fixture（`docx_table_renders_as_gfm`）；中煤 8 文件 A/B 仍是上线后手验，失败修渲染不改默认
- [x] 共用 `describe_image(..., image_source_type)` 写回；招标不入队知识库 `image:multimodal`
- [x] 有图未配/失败 VLM：禁止 stub 与假 completed
- [x] thin / tables_flat；工作台文案
- [x] 有表的合成 docx fixture

对照基线：`docs/research/weknora-parse-extract-baseline.md`（main `ba03be77` / v0.7.2 `3d5d8bfc`）。
