# 知识库计划

知识库与招投标分开评审、分开实施。领域语义以 [`docs/knowledge-base/domain.md`](../../docs/knowledge-base/domain.md) 为准；crate 目录与 ingest 真源以 [`docs/knowledge-base/crate.md`](../../docs/knowledge-base/crate.md) 为准。已确认跨域方案的任务及授权边界见[实施台账](../implementation-tasks.md)。

## 当前计划

无。有 `EXPLAIN ANALYZE` / SLO 或 ingest 运维问题再开短计划。

## 已完成

| 记录 | 规范 / 代码 |
| --- | --- |
| DocParser 转换门面（cancel + 模块拆分） | [`docparser-convert-hygiene.md`](docparser-convert-hygiene.md)；`crates/docparser/src/convert.rs` |
| Embedding 身份冻结 | [`embedding-identity.md`](embedding-identity.md)；`docs/knowledge-base/crate.md` |
| crate 拆分与去掉内存 Store | [`docs/knowledge-base/crate.md`](../../docs/knowledge-base/crate.md)；记录 [`crate-module-boundaries.md`](crate-module-boundaries.md) |
| 知识资产详情页分类展示 | [`document-detail.md`](document-detail.md)；`web/src/assets/DocumentDetail.tsx` |
| 招标 builtin 冻结网格 DOCX/XLSX/XLSM/PDF | [`docreader-structured-parse.md`](docreader-structured-parse.md)；知识库 Office 默认仍 anydoc |
| 招投标图片证据 V3 接缝 | [`bidding-evidence-media-v3.md`](bidding-evidence-media-v3.md)；端口语义在 [`docs/knowledge-base/domain.md`](../../docs/knowledge-base/domain.md) §2.2 |

worker 只保留 Oxana adapter：convert 在 `knowledge::ingest`，见 [worker 运行时边界](../platform/worker-runtime.md)。

解析对标的调研基线：[`../../docs/research/weknora-parse-extract-baseline.md`](../../docs/research/weknora-parse-extract-baseline.md)。共享解析合同以 [DocReader 结构增强](docreader-structured-parse.md) 为准；知识库切块/索引/失败可见性仍只在本目录演进。

## 向量检索

禁止项在 [`docs/knowledge-base/crate.md`](../../docs/knowledge-base/crate.md)。不列入当前计划。有 `EXPLAIN ANALYZE` / SLO 证据再单开短计划。

## 招投标依赖

知识库只稳定提供 `KnowledgeRetrievalPortV3` 的产品证据和公司证据检索。招投标的 BidProject、DOCX 正文/文件版本、初稿/候选、Assessment 与导出不进入知识库计划。
