# 知识库计划

知识库与招投标分开评审、分开实施。本轮不改变 Workspace、Product、Document、解析、索引和检索语义。

## 当前计划

- [`document-detail.md`](document-detail.md)：知识资产详情页分类展示。
- [`retrieval-ranking.md`](retrieval-ranking.md)：招投标证据 v2 的精确源文前缀、语义尾部融合与专用重排。
- [`bidding-evidence-media-v3.md`](bidding-evidence-media-v3.md)：为招投标冻结`image_ocr`对应图片资产身份；保持现有排序与scope语义。

解析对标的调研基线保存在 [`../../docs/research/weknora-parse-extract-baseline.md`](../../docs/research/weknora-parse-extract-baseline.md)。若继续做知识库解析重构，必须先在本目录重建只属于知识库解析、索引和失败可见性的计划；招投标抽取与共享 worker/lease 分别回到其所属领域。

## 招投标依赖

知识库只需稳定提供 `KnowledgeRetrievalPort` 的产品证据和公司证据检索。招投标的 BidProject、大纲、ContentBlock、Candidate、Assessment 与导出不进入知识库计划。
