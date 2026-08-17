# 22 — Review：Wiki 对照 brain

**What to build:** 对照 brain wiki 防抖、计数、锁，确认没有跨版本合并页面、没有改常量。

**Blocked by:** 21 — Wiki 防抖批处理

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 常量数值未改
- [x] wiki 独立 oxana 池（`WikiQueue` key=`wiki` 并发 8；worker `run_core` 消费 `wiki:ingest` / `wiki:finalize`）
- [x] 两 lane Peek/Claim 不混读
- [x] 偏差已记明

## Comments

- oxana `WikiQueue` + `WikiIngestJob` / `WikiFinalizeJob`（`unique_id` 按 version 合并）。PG `task_pending_ops` claim 只按 `task_type` 分 lane。
- ingest 终态 `FinalizeSubtask`；`!wiki_enabled` 可重试。
- 偏差：合成仍无 taxonomy/cite LLM；内存 drain 仍用于 HTTP 测；无独立 wiki 页表（0006）。
