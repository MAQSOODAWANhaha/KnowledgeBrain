# 知识库领域

| 项 | 值 |
| --- | --- |
| 状态 | 当前业务语义保持 |
| 服务对象 | 知识资产管理、问答、招投标证据检索 |

本文是知识库领域与跨域检索端口的唯一权威定义。实现现状快照见 [`../research/repository-implementation-snapshot.md`](../research/repository-implementation-snapshot.md)，不从快照反推新业务规则。

## 1. 领域模型

```text
Workspace
  -> Product
       -> ProductVersion
            -> Document
                 -> parsed Markdown / Chunk / Image derivations / indexes
```

### 1.1 Workspace

- `kind=product_line`：产品线空间，可有多个；
- `kind=company`：公司资料空间，当前产品边界恰好一个；
- Workspace 是知识资产边界，不是 BidProject 父级；
- authenticated-global 是当前访问边界，Workspace member 不作为招投标门闩。

### 1.2 Product 与 ProductVersion

- 产品线下 `kind=product` 表示可参与产品证据检索的型号；
- company 下 `kind=library` 表示资质证照、体系认证、业绩案例、服务能力等分类；
- `ProductVersion` 是文档、索引、Wiki/graph 的版本范围；current/active 语义由知识库拥有；
- 换手册、换证或换资料批次通过版本生命周期处理，不让招投标复制版本状态。

### 1.3 Document

- 知识资产文件归某一 ProductVersion；
- 解析、Markdown、chunk、图片派生内容、embedding、关键词与索引归知识库；
- `index_ready` 等状态只描述知识资产是否可检索；
- 招标文件、某次投标的人补截图和 submission artifact 不创建知识库 Document。

## 2. KnowledgeRetrievalPort

这是招投标可调用的唯一知识库业务端口：

```text
retrieve_product_evidence(ProductEvidenceRequestV1)
  -> KnowledgeEvidenceBatchV1

retrieve_company_evidence(CompanyEvidenceRequestV1)
  -> KnowledgeEvidenceBatchV1
```

两个 request 只表达知识库可理解的冻结检索范围、requirement text/identity、版本选择和检索 policy identity；不得包含 BidProject 表名、matching job、part 或 quote 模型。

batch 把完整 eligible scope 与有界命中集合分开：

```text
KnowledgeEvidenceBatchV1
  schema_version
  eligible_versions: EligibleEvidenceVersionV1[]
  hits: KnowledgeEvidenceHitV1[]

EligibleEvidenceVersionV1
  product_id, product_version_id
  workspace_kind = product_line|company
  frozen_display_name
```

`hits` 使用同一有界 `KnowledgeEvidenceHitV1` 结构：

```text
schema_version
document_id, source_chunk_id
product_id, product_version_id
workspace_kind = product_line|company
frozen_document_display_name
chunk_utf8, chunk_sha256, chunk_byte_length
quote_start_offset, quote_end_offset, offset_unit=utf8_byte
retrieval_rank, retrieval_raw_score
retrieval_contract_version
```

不变量：

- `eligible_versions` 是本次 request 对应 workspace kind 的完整 current eligible version 集合，不受 hit 数量配额截断；
- `product_line` 只包含 current eligible product version，`company` 只包含 current eligible library version；
- 每个 hit 必须属于同 batch 的 `eligible_versions`，但 eligible version 不要求产生 hit；
- `eligible_versions` 非空而 `hits=[]` 是合法结果，调用方据此为每个 requirement 生成 `NO_EVIDENCE`，不能把“无命中”解释成“无 eligible membership”；
- hit 数量、单 chunk bytes 和总 bytes 受检索 policy 限制，eligible version 数量使用独立边界；
- company hit 不构成产品排名；
- quote 是 chunk 的连续 UTF-8 byte slice，offset/digest/length 可验证；
- 文件显示名和 chunk bytes 是检索时快照，便于调用方立即冻结；
- 知识库不接收也不保存招投标 Candidate、EvidenceBundle、MatchingReport 或人工选择；编制侧如何采用检索结果见 [`../bidding/authoring.md`](../bidding/authoring.md)；
- 端口返回以后 live Document 的修改或删除，不改写调用方已冻结 artifact。

### 2.1 knowledge-owned scope attestation

知识库拥有检索 scope 的持久化 `kb_knowledge_attest_matching_scope_v1` / `kb_knowledge_verify_matching_scope_v1` 合同。招投标 schedule 只能把端口返回的 `eligible_versions` 与 `hits` 快照交给该合同；只有知识库函数可以将快照与 live Workspace/Product/ProductVersion/Document/chunk 关系比较。

attestation scope 必须携带固定结构 `version_selections={"product_line":[],"company":[]}`。两个 key 都必须存在，每个值都是按 UUID 升序排列且无重复的 version ID 数组；空数组表示该 workspace kind 的全部 current eligible versions，非空数组表示必须精确匹配的冻结子集。未出现在 `workspace_kinds` 的 kind 必须使用空数组，禁止把 product 与 company selection 混在同一无类型列表中。`products` 必须与上述每个 kind 的 effective selection 双向精确一致。

attest 成功后产生 immutable attestation ID、canonical payload 与 SHA-256。招投标 manifest 冻结 attestation ID/hash，deferred verifier 只能以 ID/hash/同一 payload 调用 knowledge verify；不得在招投标函数中直接 join 知识库表。attestation 证明 schedule 时的完整 scope 和 hit 来源，后续 live 资料变化不改写已发布 manifest/report。

### 2.2 招投标图片证据V3评审边界

当前V2只冻结`image_ocr`文本chunk，不提供可插入投标文件的图片asset identity。为使未激活的bidding V2 baseline能够建立真实复合外键，Phase 0只提前冻结`knowledge_image_artifact_revisions`及`knowledge_image_ocr_chunk_artifact_mappings`存储identity：同一ProductVersion/Document的`image_ocr` chunk映射不可变图片revision。由于first-launch固定先knowledge后shared，knowledge baseline先冻结等价的closed text identity和immutable约束；shared加载ObjectRegistry后，inactive bidding V2 baseline再追加`object_ref + digest + media_type + available`复合FK。该表当前没有publication/query API，也不改变任何retrieval response。

Phase 4才实现图片ingestion publication、V3 hit/media schema、knowledge-owned verifier和唯一`KnowledgeRetrievalPort`的V3查询行为；不得新增第二个检索/media端口，也不得改变V2排序、quota、eligible scope或scope attestation语义。具体实现范围和验证见独立计划[`../../plans/knowledge-base/bidding-evidence-media-v3.md`](../../plans/knowledge-base/bidding-evidence-media-v3.md)。

## 3. 招投标边界

招投标：

- 不直接 join Workspace/Product/Document/chunk/index 表；
- 不把 BidDocument 写入知识库索引；
- 不复用知识库 Document 解析状态机；
- 把被采用的 port hit 转成招投标拥有的 frozen source/evidence artifact；
- 对候选、验证、decision、report、picks 和正式输出承担全部责任。

知识库：

- 不读取 BidProject、clause、quote 或 submission；
- 不因招投标 current/stale 改变自己的 Document 生命周期；
- 只按端口 request 的知识范围执行检索。

## 4. 共享平台边界

鉴权、队列运行时、ObjectRegistry、通用幂等/audit 和可观测性归 [`../platform/README.md`](../platform/README.md)。知识库使用这些能力，但不在本领域重新定义平台状态。

## 5. 本轮不变项

本次文档整理不改变 Workspace、Product、ProductVersion、Document、解析、索引、问答或检索算法的现有业务语义。任何后续知识库语义调整都必须先在 [`../../plans/knowledge-base/README.md`](../../plans/knowledge-base/README.md) 下独立评审。
