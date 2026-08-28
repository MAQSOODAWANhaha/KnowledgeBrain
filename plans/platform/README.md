# 共享平台计划

共享平台计划只覆盖鉴权、运行时、通用幂等/审计、对象注册表、队列、部署与可观测性。业务领域通过端口使用这些能力。

## 当前材料

- [`runtime-foundation.md`](runtime-foundation.md)：fresh baseline、actor/idempotency/audit、`ObjectRegistry` 与 retention 的平台唯一活动实施定义。
- [`queue-runtime.md`](queue-runtime.md)：Oxana/Redis transport interface、进程 identity、retry/resurrection 所有权与平台验收。
- [`tracing-observability.md`](tracing-observability.md)：可观测性计划。
- [`../../docs/research/repository-implementation-snapshot.md`](../../docs/research/repository-implementation-snapshot.md)：迁移前仓库实现快照，非规范。

队列 transport、handler retry、worker crash resurrection和dead queue只由`queue-runtime.md`定义。领域只保存business target/revision与幂等publish，不保存claim/lease，不扫描pending target重建队列。
