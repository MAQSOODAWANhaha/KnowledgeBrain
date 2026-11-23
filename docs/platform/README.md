# 共享平台

| 项 | 值 |
| --- | --- |
| 状态 | 领域归属已确认；运行时基础已实施，以代码和测试为准 |
| 服务对象 | 知识库与招投标 |

共享平台只提供两类业务都需要的基础能力，不拥有知识库或招投标业务状态机。

招标分析与编制运行在 `services/docreader`，Rust 提供 API 桥接、知识服务和通用对象/队列能力。旧 `crates/bidding` 及其专用运行入口已删除。领域执行状态、租约与模型回执由 Python 工作流维护。

## 共享平台拥有

- LDAP/本地登录、JWT 与 API Key 身份认证；业务服务按创建人校验资源归属，Key 沿用其所属用户身份；
- API/worker/docreader 进程拓扑、队列注册；Oxana 负责 transport retry/resurrection/dead queue；招标模型调用的租约与回执由 Python 工作流维护，不镜像 Oxana 状态；
- 通用 actor identity 与审计；实际命令的幂等结果归各业务持久化模块；
- `ObjectRegistry`、不可变 deletion identity、Oxana typed retention job 与 owner-fenced 完成 tombstone；
- 维护门、健康检查、日志、tracing、metrics 与部署验收基础；
- PostgreSQL/Redis/对象存储的启动与依赖就绪检查；三份全新初始化基线只定义表、索引、约束，不定义业务角色、自定义函数或触发器。对象生命周期在 Rust 事务中完成。

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

- 配置：只支持全新部署，一套应用数据库账号。用户必须提供的配置见 [AGENTS.md](../../AGENTS.md)「本机开发栈」和 [deploy/README.md](../../deploy/README.md)。
- 招标分析与编制：[Python 招标](../tender/README.md)。正文保存见 [ONLYOFFICE](../tender/onlyoffice.md)。
- 数据库结构：[数据库设计](database-initialization.md)、[命名约定](database-naming.md)
- 日志：API 和 worker 用 `platform::init_tracing()` 初始化 `tracing`。`api/routes.rs`、`worker/main.rs`、`platform/jobs.rs`、`platform/s3.rs` 等生产路径仍有少量 `eprintln!`，尚未统一。
