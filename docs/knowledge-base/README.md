# 知识库

| 项 | 值 |
| --- | --- |
| 状态 | 现有业务语义保持不变；本轮只建立归属与引用 |
| 服务对象 | 知识资产管理、问答，以及招投标的证据检索 |

## 知识库拥有

- `Workspace`：产品线与公司资料空间；
- `Product` / `ProductVersion`：产品、资料分类和版本；
- `Document`：知识资产文件及其解析生命周期；
- Markdown、chunk、图片派生内容、embedding、关键词与检索索引；
- 产品证据和公司证据的检索、排序、过滤及来源定位；
- 知识资产详情页、原件预览、解析正文和派生数据展示。

## 与招投标的边界

招投标不得直接读取知识库表、复用知识库 `Document` 状态机或把招标文件写入产品索引。唯一跨域契约及 DTO 由 [`domain.md`](domain.md) 的 `KnowledgeRetrievalPort` 定义；招投标负责冻结采用证据。

## 当前文档

- 知识库领域真源：[`domain.md`](domain.md)
- 仓库实现快照（非规范）：[`../research/repository-implementation-snapshot.md`](../research/repository-implementation-snapshot.md)
- 文档详情计划：[`../../plans/knowledge-base/document-detail.md`](../../plans/knowledge-base/document-detail.md)
- 外部解析抽取调研基线：[`../research/weknora-parse-extract-baseline.md`](../research/weknora-parse-extract-baseline.md)

本页没有改动现有 Workspace、Document、索引或检索业务规则；后续知识库重构应在 [`../../plans/knowledge-base/README.md`](../../plans/knowledge-base/README.md) 下独立评审。
