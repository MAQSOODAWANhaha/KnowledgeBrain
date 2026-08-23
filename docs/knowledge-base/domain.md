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
  -> ProductEvidenceHitV1[]

retrieve_company_evidence(CompanyEvidenceRequestV1)
  -> CompanyEvidenceHitV1[]
```

两个 request 只表达知识库可理解的冻结检索范围、requirement text/identity、版本选择和检索 policy identity；不得包含 BidProject 表名、matching job、part 或 quote 模型。

两个 hit 使用同一有界 `KnowledgeEvidenceHitV1` 公共结构，并分别增加产品或公司资料 scope identity：

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

- `ProductEvidenceHitV1` 只能来自 product_line 下 current eligible product version；
- `CompanyEvidenceHitV1` 只能来自 company 下 current eligible library version，不构成产品排名；
- quote 是 chunk 的连续 UTF-8 byte slice，offset/digest/length 可验证；
- 文件显示名和 chunk bytes 是检索时快照，便于调用方立即冻结；
- 返回数量、单 chunk bytes、总 bytes 有 policy 上限；
- 知识库不接收也不保存招投标 candidate/decision/report/pick；
- 端口返回以后 live Document 的修改或删除，不改写调用方已冻结 artifact。

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
