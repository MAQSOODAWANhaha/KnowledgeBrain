# 招投标知识证据图片身份 V3

> 状态：Phase 0存储identity已冻结；Phase 4 publication/retrieval仍待实现和独立评审。本文只补齐招投标插入知识库图片所需的冻结media identity，不改变检索排序、eligible scope、Workspace/Product/Document生命周期或招投标业务模型。

## Context

当前`KnowledgeRetrievalPortV2`把`image_ocr`作为可信source type返回，但`KnowledgeEvidenceHitV2`只有OCR chunk identity和文本，没有可冻结的图片artifact/ObjectRegistry identity。招投标因此无法从一次检索响应安全地把技术截图、资质证书或案例图片放入ContentCandidate，也不得通过OCR文本回查live知识库表。

目标是建立一条知识库拥有的不可变链：

```text
image_ocr source chunk
→ KnowledgeImageArtifactRevision
→ ObjectRegistry object_ref/sha256
→ KnowledgeEvidenceMediaV1 snapshot
→ KnowledgeEvidenceHitV3
```

## 实施阶段约束

Phase 0只创建不可变`KnowledgeImageArtifactRevision`、`image_ocr chunk -> artifact`mapping以及ObjectRegistry复合identity约束，供inactive bidding V2 baseline建立真实FK。first-launch固定按knowledge→shared→bidding加载，因此knowledge baseline先创建closed text identity和knowledge-owned immutable trigger，bidding V2 baseline在ObjectRegistry存在后追加复合FK；不复制ObjectRegistry、不改变迁移顺序。Phase 0不新增ingestion发布路径、不修改KnowledgeRetrievalPort、不返回V3 media，也不注册任何运行时行为。

Phase 4负责原子publication、V3 schema/port/storage adapter、knowledge-owned media verifier和consumer接入。该分段是构建期依赖管理，不是V2/V3运行时双模式。

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

## Files to modify

- `docs/knowledge-base/domain.md`
- `migrations/knowledge_base_baseline.sql`
- `crates/domain/src/knowledge_retrieval.rs`
- `crates/storage/src/knowledge_retrieval.rs`
- `crates/storage/src/knowledge_retrieval/semantic_v2.rs`
- `crates/storage/src/persist.rs`
- knowledge ingestion/worker中当前发布`image_ocr`chunk的路径
- `crates/bid/src/matching/*`与`crates/storage/src/bid_matching.rs`仅作为V3 consumer

## Reuse

- `crates/domain/src/knowledge_retrieval.rs`的`KnowledgeSourceTypeV2::ImageOcr`、V2 batch验证和golden serialization；
- `crates/storage/src/knowledge_retrieval.rs`及`semantic_v2.rs`的exact/semantic选择、quota和scope attestation；
- `migrations/knowledge_base_baseline.sql`的knowledge-owned verifier；
- `crates/storage/src/object_registry.rs`的对象identity、digest与引用管理；
- `crates/storage/src/persist.rs::document_image_object_refs`现有图片引用查询只作为定位实现seam，不作为V3 live回查合同。

## Steps

- [x] Phase 0在knowledge baseline添加不可变image artifact/mapping存储identity、同Document/ProductVersion约束和ObjectRegistry复合引用；不提供发布/查询行为。
- [ ] Phase 4冻结`KnowledgeEvidenceMediaV1`和`KnowledgeEvidenceHitV3`canonical schema/golden hash，并补齐publication payload schema。
- [ ] 修改图片ingestion publication，使image object、artifact和OCR chunk mapping原子成功或整体失败。
- [ ] 扩展唯一KnowledgeRetrievalPort及storage adapter返回V3 media snapshot；保持V2排序与scope选择代码路径。
- [ ] 扩展knowledge-owned attest/verify验证media来源链和digest。
- [ ] 修改招投标consumer把选中media冻结为EvidenceAssetArtifact，不直接join知识库表。
- [ ] 增加deletion/change测试，证明live资料变化不改写历史V3响应或已冻结投标EvidenceAsset。

## Verification

- schema/golden：V3 JSON稳定，未知media字段、错误source type组合和缺失identity fail closed；
- SQL：image_ocr mapping同Document/ProductVersion、ObjectRegistry digest和不可变约束；
- retrieval：相同fixture下V2/V3文本hit顺序、score、quota和eligible scope完全一致；
- negative：无mapping OCR chunk、跨Document图片、digest/media type/尺寸不匹配被拒绝；
- integration：技术截图、资质证书和案例图片通过V3进入招投标EvidenceAsset，live知识文档删除后仍可重放既有Candidate/Manifest。
