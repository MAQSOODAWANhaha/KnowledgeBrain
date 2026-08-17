# 01 — 仓库骨架与 compose

**What to build:** 空仓能一键拉起 Postgres、Redis、MinIO，以及空的 `api` / `worker` 进程；CI 能跑通空测试。尚无业务。

**Blocked by:** None — can start immediately

**Status:** done

- [x] `docker compose` 拉起 Postgres、Redis、MinIO，健康检查通过
- [x] Cargo workspace 含规格中的 crate，`api` 与 `worker` 能启动并退出
- [x] CI 对空测绿
- [x] HTTP 进程不调用任何解析/分块/向量逻辑

## Comments

- reality: compose 健康（host 15432/16379/19000）。edition 2024 / rust-version 1.97，crate 全部 workspace 继承。
