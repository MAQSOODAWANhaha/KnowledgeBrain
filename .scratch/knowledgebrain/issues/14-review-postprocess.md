# 14 — Review：post_process 与 housekeeping 对照 brain

**What to build:** 确认状态机收口没有手写 finalizing、没有跳过计数器。

**Blocked by:** 13 — post_process 收口与 housekeeping

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 公式在 N=0 路径与 brain 一致（spec 表用例单测）
- [x] 未实现的真实 LLM 仍按开关计入 N（stub handler 会 drain）
- [x] housekeeping 以 oxana cron + DocumentProcessTimeout+10m 跑着
- [x] 偏差已记明：无 span 时用 `documents.updated_at`；`GET /api/v1/ops/queues` 看内存队列 + PG pending

## Comments

- review 2026-08-15f: SetFinalizing / FinalizeSubtask 与 brain 同语义。cron 每 5 分钟，心跳=span `finished_at/started_at` 否则 `updated_at`。
- 2026-08-17: PG 先原子 SetFinalizing 再入队；owned slot 入队失败释放；post_process 死信 fail 父文档。Wiki 不进 shortfall（brain 同款缺口）。
- `GET /api/v1/ops/queues` 返回 memory / pending_ops / dead_letters。post_process 会跑 summary/question 并回写 PG。
