# 31 — reparse / cancel / delete 与死信

**What to build:** 用户能取消解析、重解析、删除文档或版本；队列耗尽重试后文档变 failed，而不是一直转圈。

**Blocked by:** 06 — Review：薄上传对照 brain；12 — Review：分块与向量对照 brain

**Status:** done

- [x] cancel 只改 `parse_status=cancelled`，在途 worker 入口短路
- [x] reparse 新 attempt，先 disabled，清 chunk/向量/图
- [x] 删除走 `deleting` + `kb:delete` / `knowledge:list_delete`
- [x] 死信回写：process / post_process / manual → failed；image 不 fail 父文档

## Comments

- reality: cancel 写 PG `cancelled`；reparse 清索引 + `bump_document_attempt` + oxana `document:process`；delete 入队 `knowledge:list_delete` / `kb:delete`（oxana `low`）。删 Workspace / Product 先 cancel 在途文档，再对全部版本入队 `kb:delete`。删 Workspace 另外清成员并把 slug 改成 `__deleted_{id}`。
- 8 生命周期收口：HTTP reparse 只 `mark_reparse_queued` + 入队 `knowledge:list_reparse`（不再 HTTP 里 purge/bump/再入队 process，避免 attempt +2）。HTTP delete 只写 `deleting`，worker 才 soft_delete。ingest `bump_object_ref`，delete `release_object_ref` 到 0 删 blob。housekeep 只扫 `processing`/`finalizing`（不杀排队中的 `pending`）。reparse 要求版本 `active`（`VERSION_NOT_ACTIVE`）。
