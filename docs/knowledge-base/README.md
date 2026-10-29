# 知识库

| 项 | 值 |
| --- | --- |
| 状态 | 领域语义稳定；crate 已按目录拆分；问答 embedding 身份已冻结为环境模型 |
| 服务对象 | 知识资产管理、问答，以及招投标的证据检索 |

## 知识库拥有

Workspace 按创建人隔离，产品、资料与派生成果继承其归属。API Key 是用户凭据，没有独立读写分级。数据库不执行用户授权或业务过程，索引发布、任务投递及解析代次由 Rust 服务事务维护；统一规则见 [数据库设计](../design/database-initialization.md)。

- `Workspace`：产品线与公司资料空间；
- `Product` / `ProductVersion`：产品、资料分类和版本；
- `Document`：知识资产文件及其解析生命周期；
- Markdown、chunk、图片派生内容、embedding、关键词与检索索引；
- 版本范围内的 Wiki 与图谱；
- 产品证据和公司证据的检索、排序、过滤及来源定位；
- 知识资产详情页、原件预览、解析正文和派生数据展示。

## 与招投标的边界

招投标不得复用知识库 `Document` 状态机或把招标文件写入产品索引。招投标负责冻结采用证据，当前编制及知识匹配入口见 [Python 编制设计](../design/python-tender-phase1/authoring-architecture.md)。知识库保持来源定位和资料归属，不在本域重写编制流程。

## 规范

- 领域与招标端口：[`domain.md`](domain.md)（§1.4 产品问答 vs 招标证据）
- crate 目录、ingest 真源、embedding 冻结：[`crate.md`](crate.md)

仓库实现快照（非规范）：[`../research/repository-implementation-snapshot.md`](../research/repository-implementation-snapshot.md)。

## 计划

知识库可靠性工作由[本域计划](../../plans/knowledge-base/README.md)独立管理。招投标的匹配与填充验收状态由 [Python 交接文档](../design/python-tender-phase1/handoff.md) 记录；知识库模块测试不代替完整编制验收。
