# 32 — Review：生命周期对照 brain

**What to build:** 对照 brain 取消/重解析/删除/死信，确认没有靠看板删任务、没有改 task type 字符串。

**Blocked by:** 31 — reparse / cancel / delete 与死信

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 取消信号仅为 parse_status
- [x] `kb:delete`、`knowledge:list_delete` 未改名
- [x] 偏差已记明

## Comments

- reality: 未靠看板删任务。
- HTTP 薄入队：reparse / delete 不再在请求线程里做 worker 清理。refcount 与 housekeep 状态集已按规格收口。
