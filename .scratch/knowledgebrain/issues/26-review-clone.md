# 26 — Review：克隆对照规格 §6

**What to build:** 确认克隆是快照差量，不是 brain 的整库 clone 改名，也没有串改源版本；生产路径是 PG + oxana `low`。

**Blocked by:** 25 — 版本克隆

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] `api` 不依赖 `clone` crate
- [x] 入队队列 key 为 `low`，task 名为 `version:clone`
- [x] 源版本文档数 / wiki / 图不变（PG 测：源 count 仍 1，不写 wiki/graph 表）
- [x] 0004 未到时 keep 不假装拷了 chunk/向量（FollowUp 全是 `document:process`）
- [x] 偏差已记明：HTTP 目录仍双写内存

## Comments

- review 2026-08-15: 与新 §6 一致。不是 `kb:clone`。内存 `Store` 克隆路径已移除。
- worker 已消费 oxana `low`：`VersionCloneWorker` 调 `clone::run_clone`，follow-up 入队 `document:process`。
