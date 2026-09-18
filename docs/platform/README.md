# 共享平台

| 项 | 值 |
| --- | --- |
| 状态 | 领域归属已确认；运行时基础已进入独立实施方案 |
| 服务对象 | 知识库与招投标 |

共享平台只提供两类业务都需要的基础能力，不拥有知识库或招投标业务状态机。

投标 [Rig 运行层](../../plans/bidding/product-two-phase.md)归属 `crates/bidding`，复用本域 Worker、调用预约、执行权、检查点和对象能力，不新增平台 Agent 服务或第二套队列。Rig 的 SDK `AgentRun` 与领域执行 token/lease 不同，前者不能授权业务副作用或发布。

## 共享平台拥有

- LDAP/本地登录、JWT、API key 与 authenticated-global 访问边界；
- API/worker/docreader 进程拓扑、队列注册；Oxana 负责 transport retry/resurrection/dead queue；显式 Agent-bound Request 的业务副作用由领域 AgentRun token/lease 与 logical-call ledger fence，不能镜像 Oxana phase；
- 通用 actor identity、幂等结果、append-only audit 基础设施；
- `ObjectRegistry`、不可变 deletion identity、Oxana typed retention job 与 owner-fenced 完成 tombstone；
- 维护门、健康检查、日志、tracing、metrics 与部署验收基础；
- PostgreSQL/Redis/对象存储的启动、权限与运行安全基线；三 baseline release receipt、catalog manifest readiness 与 deployment namespace reset。

## 共享平台不拥有

- Workspace、Product、Document、chunk、index 与 retrieval 语义；
- BidProject、SubmissionWorkspace、DOCX 编制版本、初稿/候选、Assessment、报价快照与导出；
- 任何“为了某个调用方方便”而复制出的知识库或招投标状态。

## 使用规则

- 业务聚合通过自己的 repository/application service 使用共享能力。
- `ObjectRegistry` 是对象可用性与引用的唯一平台真源；业务表只保存受检引用及业务元数据。
- Open/Stage/Commit 若用于大结果提交，只是具体 adapter 的内部传输协议，不升级为全平台业务接口。
- 运行时成功、代码测试通过、部署完成和真实业务验收必须分别报告。

## 当前参考

- 招标文件驱动的投标文件编制工作区：[目标契约](../bidding/authoring.md)（来源、候选、证据与身份边界；[正文/保存目标](../bidding/onlyoffice.md)仍归招投标领域）
- 仓库实现快照（非规范）：[`../research/repository-implementation-snapshot.md`](../research/repository-implementation-snapshot.md)
- 共享机制计划：[`../../plans/platform/README.md`](../../plans/platform/README.md)
- fresh baseline、actor/idempotency/audit、ObjectRegistry 与 retention：[`../../plans/platform/runtime-foundation.md`](../../plans/platform/runtime-foundation.md)
- 可观测性计划：[`../../plans/platform/tracing-observability.md`](../../plans/platform/tracing-observability.md)
