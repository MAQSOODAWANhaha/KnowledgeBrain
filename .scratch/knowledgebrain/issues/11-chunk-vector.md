# 11 — 分块 + 向量，版本内可搜

**What to build:** 一篇文档从上传到 worker 分块、写向量/关键词后，assembly 按该版本能搜到。索引写完即可检索（`enable_status=enabled`），不必等 Wiki。

**Blocked by:** 10 — Review：convert 对照 brain

**Status:** done

- [x] chunker 策略链：`auto` → heading/heuristic → **legacy 兜底**；`heading`/`heuristic` 后接 legacy；`legacy`/`recursive`/`""` 仅 legacy
- [x] legacy：rune 偏移；`end-start == rune_count`
- [x] 7500 硬顶切开超长片（单 unit >7500 先切片）
- [x] 索引文本 = `title+\n` + EmbeddingContent()；ContextHeader 不进 Content
- [x] 关向量也写 chunk；parent_text 不进向量；NeedsEmbedding = vector || keyword
- [x] 无图时入队 `knowledge:post_process`
- [x] `POST /search` assembly + product_id + version_id 能命中（stub 向量）

## Comments

- reality: 可检索主路径仍是 stub embed。chunker 已按 brain：保护区 + 策略链 + heading/heuristic/legacy，不是硬拆。
- 父子配置对齐 brain `buildParentChildConfigs`：父 overlap = 版本 overlap（不是 chunk_size）；子 overlap = child_size/5。`parent_chunk_size`/`child_chunk_size`/`separators`/`token_limit`/`languages` 进 `chunking_config`，hydrate / PATCH / clone / `process_overrides` 合并。
- processChunks：空 content 不写；无 text 且无多模态直接 `completed` 不入队 post_process；`summary_status=none`。配置了 embedding URL 则失败可重试，最后一次 fail 文档；请求用版本 `embedding_model_id`。写库前后再查 abort，deleting 清刚写的索引。
- review: 见 12。
- 门禁：见 `.scratch/knowledgebrain/review.md`。
