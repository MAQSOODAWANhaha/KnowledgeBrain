# 02 — Review：仓库骨架

**What to build:** 对照规格 §1，确认骨架没有另起技术栈或把解析塞进 API 进程。

**Blocked by:** 01 — 仓库骨架与 compose

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 三进程边界与规格一致：`api`、`worker`、`services/docreader`
- [x] 队列选型为 oxana（crate 2.1 已进 workspace；HTTP 入队走 `default`；6 Runtime 消费仍在后续票）
- [x] 无 Python 嵌入 Rust
- [x] 偏差已记明（6 个 Runtime 消费留给 09/13；本票只要求选型）

## Comments

- review 2026-08-15b: 选型已是 oxana 2.1。进程边界仍对。无 Python 嵌入 Rust。

