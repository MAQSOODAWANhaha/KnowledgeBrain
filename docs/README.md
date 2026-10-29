# KnowledgeBrain 文档导航

本目录保存已经确认的产品与领域定义。招投标当前实施计划在 [Python 实施计划](design/python-tender-phase1/implementation.md)，其余见[计划导航](../plans/README.md)。

招标分析与编制统一使用 [Python 路径](design/python-tender-phase1/README.md)，Rust 提供 API、知识库和基础服务。旧 Rust 招标流程、专用表和运行入口已退出；相关历史文档不再定义当前执行合同。当前初始化与创建人权限见 [数据库设计](design/database-initialization.md)。隔离回归和历史真实验收不能代替当前版本部署验收。

仓库级产品与界面规范仍保留在标准入口：[`../PRODUCT.md`](../PRODUCT.md) 与 [`../DESIGN.md`](../DESIGN.md)。

## 权威性规则

1. 每条规范只能有一个权威定义位置；其它文档只链接，不复制另一份规则。
2. `docs/` 描述已确认的当前目标与领域边界；它不等于“代码已经实现”。实现状态必须由代码、测试和运行验收分别证明。
3. 实施计划描述如何到达目标；目录位置不授予实现状态，领域入口只链接一个当前计划，不能并存重复方案。
4. 调研材料不是当前规范；过期实施草案直接删除。冲突时以本页链接的领域文档为准。
5. 跨领域能力由所属领域定义，通过端口使用；禁止在调用方重新定义被调用领域的数据模型。

## 领域

| 领域 | 当前入口 | 拥有的能力 |
| --- | --- | --- |
| 共享平台 | [`platform/README.md`](platform/README.md) | 鉴权、运行时、队列、幂等与审计基础设施、对象注册表、可观测性 |
| 知识库 | [`knowledge-base/README.md`](knowledge-base/README.md) | Workspace、Product、ProductVersion、Document、解析、索引、检索 |
| 招投标 | [`design/python-tender-phase1/README.md`](design/python-tender-phase1/README.md) | 文件分析、要求核验、编制依据、大纲与章节、Word 编辑及出件；状态以交接文档为准 |
| 调研材料 | [`research/README.md`](research/README.md) | 外部对标、实验和非规范性分析 |

## 现有综合规格

当前领域规则只从各领域入口进入。迁移前的综合代码说明保存在 [`research/repository-implementation-snapshot.md`](research/repository-implementation-snapshot.md)，仅用于识别实现差距，不是任何领域的第二真源。
