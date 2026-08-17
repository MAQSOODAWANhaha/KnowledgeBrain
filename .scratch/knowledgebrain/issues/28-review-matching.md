# 28 — Review：Matching

**What to build:** 确认荐品是多条款聚合，不是单 query 扫 current；没有定标字段。

**Blocked by:** 27 — Matching 按条款荐产品

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 未做工作空间无差别全库搜
- [x] library 证据挂在条款下，不单独成候选
- [x] 偏差已记明（stub 检索）

## Comments

- reality: 无 best_product_id。
