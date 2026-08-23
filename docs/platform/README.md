# 共享平台

| 项 | 值 |
| --- | --- |
| 状态 | 领域归属已确认；运行时基础已进入独立实施方案 |
| 服务对象 | 知识库与招投标 |

共享平台只提供两类业务都需要的基础能力，不拥有知识库或招投标业务状态机。

## 共享平台拥有

- LDAP/本地登录、JWT、API key 与 authenticated-global 访问边界；
- API/worker/docreader 进程拓扑、队列注册、claim/lease/heartbeat/reaper；
- 通用 actor identity、幂等结果、append-only audit 基础设施；
- `ObjectRegistry`、对象引用、retention outbox 与受控物理删除；
- 维护门、健康检查、日志、tracing、metrics 与部署验收基础；
- PostgreSQL/Redis/对象存储的启动、权限与运行安全基线。

## 共享平台不拥有

- Workspace、Product、Document、chunk、index 与 retrieval 语义；
- BidProject、条款、匹配报告、报价、组卷、SubmissionGate；
- 任何“为了某个调用方方便”而复制出的知识库或招投标状态。

## 使用规则

- 业务聚合通过自己的 repository/application service 使用共享能力。
- `ObjectRegistry` 是对象可用性与引用的唯一平台真源；业务表只保存受检引用及业务元数据。
- Open/Stage/Commit 若用于大结果提交，只是具体 adapter 的内部传输协议，不升级为全平台业务接口。
- 运行时成功、代码测试通过、部署完成和真实业务验收必须分别报告。

## 当前参考

- 仓库实现快照（非规范）：[`../research/repository-implementation-snapshot.md`](../research/repository-implementation-snapshot.md)
- 共享机制计划：[`../../plans/platform/README.md`](../../plans/platform/README.md)
- fresh baseline、actor/idempotency/audit、ObjectRegistry 与 retention：[`../../plans/platform/runtime-foundation.md`](../../plans/platform/runtime-foundation.md)
- 可观测性计划：[`../../plans/platform/tracing-observability.md`](../../plans/platform/tracing-observability.md)
