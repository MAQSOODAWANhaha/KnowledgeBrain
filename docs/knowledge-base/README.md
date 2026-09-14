# 知识库

| 项 | 值 |
| --- | --- |
| 状态 | 领域语义稳定；crate 已按目录拆分；问答 embedding 身份已冻结为环境模型 |
| 服务对象 | 知识资产管理、问答，以及招投标的证据检索 |

## 知识库拥有

- `Workspace`：产品线与公司资料空间；
- `Product` / `ProductVersion`：产品、资料分类和版本；
- `Document`：知识资产文件及其解析生命周期；
- Markdown、chunk、图片派生内容、embedding、关键词与检索索引；
- 版本范围内的 Wiki 与图谱；
- 产品证据和公司证据的检索、排序、过滤及来源定位；
- 知识资产详情页、原件预览、解析正文和派生数据展示。

## 与招投标的边界

招投标不得直接读取知识库表、复用知识库 `Document` 状态机或把招标文件写入产品索引。唯一跨域契约及 DTO 由 [`domain.md`](domain.md) 的 `KnowledgeRetrievalPortV3` 定义；招投标负责冻结采用证据。招投标如何冻结证据用于初稿/候选并经确认进入 DOCX，见 [`../bidding/authoring.md`](../bidding/authoring.md)，不在知识库重写编制流程。

## 规范

- 领域与招标端口：[`domain.md`](domain.md)（§1.4 产品问答 vs 招标证据）
- crate 目录、ingest 真源、embedding 冻结：[`crate.md`](crate.md)

仓库实现快照（非规范）：[`../research/repository-implementation-snapshot.md`](../research/repository-implementation-snapshot.md)。

## 计划

知识库当前无进行中计划，见 [`../../plans/knowledge-base/README.md`](../../plans/knowledge-base/README.md)。已落地含 crate 拆分、embedding 身份冻结、DocParser convert 门面（cancel + 拆分）、招标冻结网格、详情页分类、图片证据 V3 接缝。后续语义调整仍须在计划目录独立评审。
