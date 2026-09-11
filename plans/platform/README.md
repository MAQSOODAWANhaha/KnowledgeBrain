# 共享平台计划

共享平台计划只覆盖鉴权、运行时、通用幂等/审计、对象注册表、队列、部署与可观测性。业务领域通过端口使用这些能力。

## 当前材料

- [`runtime-foundation.md`](runtime-foundation.md)：fresh baseline、actor/idempotency/audit、`ObjectRegistry` 与 retention 的平台唯一活动实施定义。
- [`queue-runtime.md`](queue-runtime.md)：Oxana/Redis transport interface、进程 identity、retry/resurrection 所有权与平台验收。
- [`tracing-observability.md`](tracing-observability.md)：可观测性计划。
- [`../../docs/research/repository-implementation-snapshot.md`](../../docs/research/repository-implementation-snapshot.md)：迁移前仓库实现快照，非规范。

队列 transport、handler retry、Worker process resurrection 与 dead queue 只由 `queue-runtime.md` 定义。显式 Agent-bound Request 可以保存 DB-generated AgentRun attempt/token/lease 与 attempt-independent physical-call ledger，仅用于 fence 外部业务副作用；PostgreSQL 不保存或恢复 queue transport work。
