# 33 — 领域表 0001 与默认 library

**What to build:** 规格 §2.1 / §10「01 领域」：Postgres 落表，创建 Workspace 时插入不可删的默认 library。状态机已在 `domain`，本票只持久化。

**Blocked by:** 32 — Review：生命周期对照 brain

**Status:** done

- [x] `migrations/0001_domain.sql`：workspaces / users / members / products(kind) / product_versions / documents / tags / document_tags / content_objects / spans / pending / DL
- [x] 无配额列、无 tenant、无 TOKEN
- [x] `retrieval_config` 缺省 0.15 / 0.3 / 50
- [x] `storage::create_workspace_with_library` 写入 `slug=library` `name=公司资料`；`delete_product` 拒删默认 library
- [x] 对 compose Postgres（15432）跑迁移并断言表存在（`storage` persist 测试）

## Comments

- HTTP 创建 Workspace / Product / Version / Document / API key 在 PG 可达时双写 persist。
- 0002 models、0003 api_keys、0004 chunks+vector(32)、0005 graph、0006 wiki 已随 `apply_0001` 落表。
