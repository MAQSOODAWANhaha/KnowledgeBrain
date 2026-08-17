# 12 — Review：分块与向量对照 brain

**What to build:** 对照 brain chunker 与 `processChunks`，确认索引文本、父子块、enable_status 无偏差。

**Blocked by:** 11 — 分块 + 向量，版本内可搜

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] EmbeddingContent / title 前缀与 brain 一致
- [x] parent_text 不进向量；text 子块进
- [x] `enable_status=enabled` 发生在索引写完
- [x] 空串 strategy = 仅 legacy（不是 auto）；auto 链末尾永远 legacy
- [x] ValidateChunks 失败换下一 tier；legacy 兜底即使校验失败
- [x] 偏差已记明：0008 `vector(1024)`；`index::embed` 可走 HTTP，未配置回 hashed stub。parent-child 在 chunker 里有。

## Comments

- review 2026-08-15d: chunker 已按 brain 分层重做（profiler / 保护区 / heading breadcrumb 在 ContextHeader 且标题行留在 Content / heuristic 边界 / mergeUnits 7500 / ValidateChunks 换档）。
- 2026-08-17: 父子 overlap 已按 `buildParentChildConfigs` 修正（父用 base overlap，子用 child_size/5）；chunking_config 完整映射。未做 brain `/chunker/preview` / `SplitWithDiagnostics`（debug API，v1 规格未要求）。
- 2026-08-17b: §5.5 PG 终态对齐 finalizeIndexedKnowledgeState；embed 失败不再静默 stub。未做 `pre_chunk_id`/`next_chunk_id`/`image_info` 列（出题仍按 start/end 扫前后块）。
- 2026-08-15 收口：pgvector 表在用；检索/匹配在内存空时走 `hybrid_search_pg`。向量仍是 stub，不是生产模型。
- 门禁：见 `.scratch/knowledgebrain/review.md`。
