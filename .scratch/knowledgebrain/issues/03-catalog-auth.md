# 03 — 工作空间目录与登录

**What to build:** 用户能注册登录，创建 Workspace（自动带默认「公司资料」library）、Product、ProductVersion，成员角色生效。

**Blocked by:** 02 — Review：仓库骨架

**Status:** done

- [x] `POST /auth/register`、`/auth/login` 签发 JWT（24h）
- [x] 创建 Workspace 时插入不可删的默认 library（`slug=library`）
- [x] Product `kind` 为 `product` 或 `library`；Version 可创建，`current_version_id` 可设置
- [x] viewer 不能创建产品；contributor 可以
- [x] 无配额列、无 tenant/知识空间概念

## Comments

- reality: HTTP 目录走内存 Store。Postgres 种 library 在 33，尚未接到这些路由。
