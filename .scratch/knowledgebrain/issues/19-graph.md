# 19 — 图谱抽取

**What to build:** 每个 text-like chunk 抽实体关系，写进该版本+文档的命名空间；重解析会先清图再抽。

**Blocked by:** 14 — Review：post_process 与 housekeeping 对照 brain

**Status:** done

- [x] `graph_enabled` 即入队 N 条 `chunk:extract`，无 `NEO4J_ENABLE`
- [x] upsert 节点 `(version, document, name)` union chunk_ids；边同样 upsert
- [x] reparse / processChunks 先 DelGraph
- [x] 不 fail 父 parse_status
- [x] 对照 brain `extract.go` 的 LLM 抽实体

## Comments

- Runtime 对齐 `extract.go`：Extractor few-shot（`extract_graph` description + Romeo 例）+ Formater 解析 fenced JSON（`entity` / `entity1` / `entity2` / `relation`）。
- `graph_extraction.yaml` 的 entity/relationship 模板已作为常量落地（两段式不是 runtime，runtime 走 Extractor）。
- `attemptSuperseded` 不 `FinalizeSubtask`；abort / `extract_enabled=false` / chunk 消失仍 finalize。
- `stub-chat` 或无 `KNOWLEDGEBRAIN_CHAT_BASE_URL`：本地实体 JSON 再走同一 parser；有 URL 则 chat。解析失败对真实模型可重试，死信仍 finalize、不 fail 父。
- 偏差：无 Langfuse；节点 attributes 不落盘。配置了 `KNOWLEDGEBRAIN_NEO4J_HTTP_URL` 时双写 Neo4j。
- 5.10：`ExtractOutcome::Superseded` 内存+PG 都不 FinalizeSubtask。抽图走 `effective_version`（`process_overrides.extract_config`）。`persist_graph_for_document` 增量 upsert，`chunk_ids` SQL UNION，不再先删整份文档图。`ExtractWorker` 最后一次失败仍 drain。
