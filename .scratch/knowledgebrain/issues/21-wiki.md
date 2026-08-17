# 21 — Wiki 防抖批处理

**What to build:** 一个版本下多篇文档上传后，合并成批生成 Wiki；页面只属于该版本；ingest 结束才让文档 completed。

**Blocked by:** 14 — Review：post_process 与 housekeeping 对照 brain

**Status:** done

- [x] ScopeID = product_version_id；常量 30s/20s/5/32768/15s 写在 crate 里
- [x] 两 lane 不混读；ingest 防抖 30s / finalize 20s 真正调度
- [x] slug 锁、tombstone、stale claim
- [x] published 页写成 `wiki_page` chunk
- [x] FinalizeSubtask 在 ingest 终态调用

## Comments

- `EnqueueWikiIngest`：ingest-lane 行 + 30s 触发（`task_id=wiki-ingest-{version}` 合并）。`EnqueueWikiRetract`：tombstone + 5s。
- `ProcessWikiIngest` 只 claim ingest lane（批 5）；`ProcessWikiFinalize` 只 claim finalize lane（slug/change）。
- slug 锁 `wiki:slug:{version}:{slug}`；tombstone `wiki:deleted:{version}:{document}`；stale claim 90m 可再领。
- `!wiki_enabled` 可重试；死信 `fail_open_pending` 以免父文档卡在 finalizing。retract 不减计数。
- 合成：map 写 `summary` + `entity`/`concept`（chat JSON，失败用图谱节点）；chunk cite 进 `source_refs`；同 slug union refs。chat 用 `wiki_config.synthesis_model_id` 否则 `summary_model_id`。
- finalize：`slug`/`change`/`folder_prune`；ingest 未排空则 prune 行回队列。写 index/log/synthesis/comparison 并 linkify。
- taxonomy：目录 ≤60 全量给规划器；更多则保留一级目录，深层用 embedding 余弦取每条 top-3（常量对齐 brain 60/150/3）。无命中再启发式。
- oxana：`enqueue_wiki_ingest` / `finalize` 走 `enqueue_in` 30s / 20s；`unique_id` Skip 合并窗口内重复触发。ingest 批后剩余行 follow-up 5s。`WikiIngestWorker::retry_delay` 固定 15s（brain `asynqRetryDelayFunc`）。
- 偏差：内存 drain 不 sleep 30s。PG ingest 写 `wiki_pages` + folders + `wiki_page` chunk。
