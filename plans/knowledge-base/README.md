# 知识库计划

知识库与招投标分开评审、分开实施。当前阶段 A 不改变 Workspace、Product、Document、解析、索引和检索语义；已确认跨域方案的任务及授权边界见[实施台账](../implementation-tasks.md)，不并入知识库其他产品计划。

## 当前计划

- [`document-detail.md`](document-detail.md)：知识资产详情页分类展示。
- [`bidding-evidence-media-v3.md`](bidding-evidence-media-v3.md)：为招投标冻结`image_ocr`对应图片资产身份；保持现有排序与scope语义。
- [`docreader-structured-parse.md`](docreader-structured-parse.md)：招标 builtin 冻结网格（DOCX/XLSX/XLSM/PDF）S0–S2 已落地；知识库 Office 默认 anydoc 不切换。
- [模型与索引一致性](#模型与索引一致性独立事项)：独立待实施事项，不进入投标提取修复关键路径。

解析对标的调研基线保存在 [`../../docs/research/weknora-parse-extract-baseline.md`](../../docs/research/weknora-parse-extract-baseline.md)。共享解析合同以 [DocReader 结构增强](docreader-structured-parse.md) 为准；知识库切块/索引/失败可见性仍只在本目录演进，招投标抽取与 worker/lease 仍回其所属领域。

## 向量检索优化边界

继续使用 PostgreSQL+pgvector，不新增独立向量库、同步链或双写。未发布阶段可按 [平台 baseline 策略](../platform/runtime-foundation.md#2-fresh-baseline)直接调整所属 migration，相关方案已确认；性能任务按[实施台账](../implementation-tasks.md)中的条件独立推进，当前阶段 A 不实施索引或检索改动。

当前已有1024维向量及 GIN/HNSW；V2 证据检索 `crates/knowledge/src/knowledge_retrieval_pg/semantic_v2.rs` 对 eligible 集合计算距离、量化并稳定排序，未必走 ANN 路径。普通搜索 `crates/knowledge/src/persist.rs` 另有距离 `ORDER BY/LIMIT`，不能据此认定全项目 HNSW 无效。尚无真实查询计划、规模或延迟瓶颈证据。

1. 有性能需求时，在授权隔离环境对代表性 eligible scope 做 `EXPLAIN ANALYZE` 与 latency/P95、QPS、recall、CPU/I/O 基准，分别记录 V2 证据检索和普通搜索；不以历史样例或任意向量数量作硬门槛。
2. 默认保留 exact/确定性、冻结 policy/revision、eligible scope、OCR 映射和事务发布语义。根据实际计划决定 SQL/HNSW 是否调整，不先全局删除索引；ANN 候选过采样须另经证据和语义确认，不能透明替换当前召回/排序。
3. 仅调优后仍不满足明确 SLO，或有独立扩缩容/故障域需求的实证时，再评估独立向量库及跨库一致性成本。

这条性能工作独立于错误落库与数据库 CI 正确性修复，不作为 baseline 修正或 ONLYOFFICE O0 的前置。

## 招投标依赖

知识库只需稳定提供 `KnowledgeRetrievalPort` 的产品证据和公司证据检索。招投标的 BidProject、DOCX 正文/文件版本、初稿/候选、Assessment 与导出不进入知识库计划。

## 模型与索引一致性独立事项

模型配置核对发现的 Embedding 身份一致性归知识库，独立于[投标提取与运行时方案](../bidding/agent-runtime-rig.md)。复用现有向量化接口、ProductVersion 的具体模型字段及1024维结构，不引入 Rig 知识库 Agent、任意维度系统或新的检索排序。

新索引写入前冻结环境解析出的具体模型身份，不长期以空值或 `stub-emb` 代表真实模型；已有索引与配置冲突须明确报告，不能混写。历史身份未知时不能用当前环境反推；同维度的不同模型也不能混用。核查及实现另行记录，当前未完成；不自动重建、清理或迁移既有索引，不阻塞投标提取与真实模板验收。
