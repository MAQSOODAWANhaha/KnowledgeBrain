# 招投标知识证据图片身份 V3

> 状态：当前源码有 V3 存储、publication/retrieval 接缝；本轮仅只读核查，未重跑验收。本文只补齐招投标插入知识库图片所需的冻结media identity，不改变检索排序、eligible scope、Workspace/Product/Document生命周期或招投标业务模型。

## Context

历史 V2 仅返回 OCR 文字，缺乏可冻结图片身份；V3 保留为当前共享能力。招投标通过它取得初稿/候选证据，不直接查 live 知识库，也不把 media 返回成功算作已经插入 DOCX。正文与采用边界见 [编制领域契约](../../docs/bidding/authoring.md)。

目标是建立一条知识库拥有的不可变链：

```text
image_ocr source chunk
→ KnowledgeImageArtifactRevision
→ ObjectRegistry object_ref/sha256
→ KnowledgeEvidenceMediaV1 snapshot
→ KnowledgeEvidenceHitV3
```

## 所有权与实施边界

Fresh bootstrap 固定 Shared → Knowledge → Bidding：Shared 创建通用 ObjectRegistry；Knowledge 创建 image artifact/mapping、复合引用、validators/triggers 与 grants；Bidding 只消费唯一 V3 端口，不 ALTER Knowledge/ObjectRegistry-owned 对象。没有 V2/V3 运行时双模式。

图片证据能力不随旧编辑方案删除；ONLYOFFICE 接入也不要求重做检索排序、eligible scope 或知识生命周期。下面是该能力的实现接缝与验证清单，不能以旧阶段勾选声明当前运行已验收。

## Approach

### 1. 知识库图片artifact

在knowledge ingestion publication中，为可检索图片发布`KnowledgeImageArtifactRevision`：

```text
image_artifact_revision_id
source document/version identities
object_ref, sha256, media_type
width, height
page_ordinal/bounding_region
source_image_key
artifact_sha256
```

`image_ocr`chunk必须通过不可变mapping引用一个同Document/ProductVersion的image artifact revision。`text`和`parent_text`hit不携带media。图片对象使用现有ObjectRegistry；端口不返回临时URL、base64或live storage lookup key。

### 2. 唯一KnowledgeRetrievalPort V3

只升级现有唯一跨域端口，不增加第二个media/retrieval port：

```text
KnowledgeEvidenceHitV3
  # 保留V2全部检索字段
  source_type = text|parent_text|image_ocr
  media: KnowledgeEvidenceMediaV1?

KnowledgeEvidenceMediaV1
  image_artifact_revision_id
  object_ref, sha256, media_type
  width, height
  page_ordinal/bounding_region
  frozen_document_display_name
```

不变量：

- `source_type=image_ocr`必须有media，其他source type必须没有media；
- chunk、image artifact、Document和eligible ProductVersion必须属于同一冻结来源链；
- ObjectRegistry identity、digest、media type和尺寸必须匹配；
- V3继续使用V2 exact/semantic排序、quota、rerank、eligible scope和scope attestation语义；
- live Document、chunk或图片后来变化不能改写已返回的V3 snapshot；
- 招投标收到V3 hit后冻结自己的EvidenceAssetArtifact，后续不直接读取知识库表。

### 3. Publication与验证

knowledge baseline负责：

1. 在图片解析/多模态处理时提交ObjectRegistry对象；
2. 原子发布image artifact revision与`image_ocr chunk -> image artifact`mapping；
3. 禁止没有mapping的`image_ocr`进入V3 trusted retrieval；
4. V3 verifier同时验证V2 hit字段和media来源链；
5. 删除/替换live资料只影响未来current检索，不改写历史artifact。

## 当前实现接缝与复用

- `migrations/knowledge_base_baseline.sql`：knowledge-owned image artifact/mapping/attestation 与 grants。
- `crates/knowledge/src/knowledge_retrieval.rs`、`knowledge_retrieval_pg/`：V3 DTO/port、exact/semantic 排序与 scope。
- `crates/knowledge/src/ingest.rs`、`pipeline.rs`、`catalog/`：图片 ingestion 原子发布（无 `persist.rs`）。
- `crates/platform/src/object_registry.rs`：对象摘要、引用与平台所有权；对象命名空间按平台合同，不由图片专题另定义路径。
- `crates/bidding/src/content_runtime.rs`、`bid_authoring_v2.rs`：消费受检证据并冻结本标引用；不代表 DOCX 入稿已实现。
- `crates/knowledge/tests/knowledge_image_ingestion_v3.rs`、`crates/bidding/tests/knowledge_retrieval_attestation_v2.rs`：现存测试位置，不是本轮执行结果。

## Verification

- schema/golden：V3 JSON稳定，未知media字段、错误source type组合和缺失identity fail closed；
- SQL：image_ocr mapping同Document/ProductVersion、ObjectRegistry digest和不可变约束；
- retrieval：相同fixture下V2/V3文本hit顺序、score、quota和eligible scope完全一致；
- negative：无mapping OCR chunk、跨Document图片、digest/media type/尺寸不匹配被拒绝；
- integration：技术截图、资质证书和案例图片通过V3进入招投标EvidenceAsset，live知识文档删除后仍可重放既有Candidate/Manifest。
