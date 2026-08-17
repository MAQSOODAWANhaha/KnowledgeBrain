# 04 — Review：目录与登录

**What to build:** 对照规格 §2、§4.2，确认领域树和鉴权没有滑回 WeKnora「空间 / 知识库」。

**Blocked by:** 03 — 工作空间目录与登录

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 实体仅为 Workspace → Product → ProductVersion → Document，无 Tenant 一等列
- [x] library 与 product 共用 Version 容器，TAG 不是 Version
- [x] JWT + API key 作用域挂 Workspace/Product（`X-API-Key`；`0003_api_keys.sql`；workspace|product + ingest/search/admin）
- [x] 偏差已记明（HTTP 仍双写内存；PG 为 key/登录回源）

## Comments

- JWT 24h HS256。API key 写入 `0003` 表；鉴权先查内存，未命中再 `find_api_key_by_hash`。登录同样可从 `users` 回源。
