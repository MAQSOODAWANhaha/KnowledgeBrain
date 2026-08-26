# 共享平台计划

共享平台计划只覆盖鉴权、运行时、通用幂等/审计、对象注册表、队列、部署与可观测性。业务领域通过端口使用这些能力。

## 当前材料

- [`runtime-foundation.md`](runtime-foundation.md)：fresh baseline、actor/idempotency/audit、`ObjectRegistry` 与 retention 的平台唯一活动实施定义。
- [`queue-runtime.md`](queue-runtime.md)：Oxana/Redis transport interface、进程 identity、retry/resurrection 所有权与平台验收。
- [`tracing-observability.md`](tracing-observability.md)：可观测性计划。
- [`../../docs/research/repository-implementation-snapshot.md`](../../docs/research/repository-implementation-snapshot.md)：迁移前仓库实现快照，非规范。
- [`../archive/platform/README.md`](../archive/platform/README.md)：旧综合稳定性与 schema 加固方案，仅供追溯。

队列 transport 只由 `queue-runtime.md` 定义；领域 durable intent、target recovery 和业务 retry 由所属领域定义并链接平台 interface，不在平台文档复制业务表。
