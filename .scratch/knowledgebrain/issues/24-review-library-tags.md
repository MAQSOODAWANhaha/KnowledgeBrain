# 24 — Review：library 与 TAG

**What to build:** 确认资质没有做成第二套管线，也没有把 Version 改成 TAG。

**Blocked by:** 23 — library、TAG 与 include_library

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] library 仍是 Product+ProductVersion
- [x] Wiki/图谱仍按版本隔离
- [x] 偏差已记明（数据面仍是内存）

## Comments

- reality: 未把 Version 改成 TAG。
