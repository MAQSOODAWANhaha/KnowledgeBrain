# 知识库领域与 Crate 边界

| 项 | 值 |
| --- | --- |
| 状态 | 领域语义与 crate 结构稳定；问答 embedding 身份冻结为环境模型 |
| 服务对象 | 知识资产管理、产品问答、招投标证据检索（`crates/knowledge`） |

本文是知识库领域语义、跨领域检索端口（`KnowledgeRetrievalPortV3`）与 `crates/knowledge` 目录边界的唯一权威定义。平台基础设施与数据库迁移/命名约定见 [共享平台与数据库](../platform/README.md)。

## 1. 领域模型与归属

```text
Workspace (owner_user_id)
  -> Product
       -> ProductVersion
            -> Document -> parsed Markdown / Chunk / Image derivations / indexes
            -> Wiki pages & folders（版本范围）
            -> Graph nodes & relations（版本范围）
```

- **创建人隔离**：`knowledge_workspaces.owner_user_id` 是归属根，产品、版本、资料、标签、检索与派生产物继承该归属；API Key 沿用所属用户身份，没有成员角色或独立读写分级。数据库不执行用户授权或业务函数，索引发布、任务投递与解析代次由 Rust 服务事务维护。
- **Workspace**：`kind=product_line` 为产品线空间（可多个）；`kind=company` 为公司资料空间（当前边界恰好一个）。Workspace 是知识资产边界，不是投标项目父级。
- **Product / ProductVersion**：产品线下 `kind=product` 表示可参与产品证据检索的型号；公司资料下 `kind=library` 表示资质证照、体系认证、业绩案例、服务能力等分类。`ProductVersion` 是文档、索引与 Wiki/图谱的版本范围，换手册/换证走版本生命周期。
- **Document**：解析、Markdown、chunk、图片派生内容、embedding、关键词与索引归知识库；`index_ready` 描述知识资产是否可检索。解析与后处理由 Oxana 队列驱动（`ingest.rs` 负责 convert/fanout/delete/reparse，`pipeline.rs` 负责 summary/questions/image/post_process/extract/wiki/datatable）。

## 2. 与招投标及平台的边界

- **招投标不侵入知识库**：招投标不直接 join `knowledge_*` 表，不把招标文件、投标人补图或 submission artifact 写入知识库 `knowledge_documents` 或产品索引，也不复用知识库解析状态机；招投标只消费检索端口返回并冻结自己的证据产物（见 [招投标工作流](../tender/workflow.md)）。
- **知识库不感知投标状态**：知识库不读取 `tender_*` 表，不接收或保存招投标 Candidate / EvidenceBundle / MatchingReport / 人工选择，也不因招投标状态改变自身 Document 生命周期。知识库单测不替代完整编制验收，编制验收状态见 [招投标入口 · 当前状态](../tender/README.md#当前状态交接)。
- **共享平台边界**：鉴权、队列运行时、ObjectRegistry、审计与可观测性归 [共享平台](../platform/README.md)。

## 3. 产品问答与招标证据检索

产品问答（`search/`：hybrid / matching / assembly）与招标证据检索（`retrieve_evidence_v3`）是两套检索，不共用命中合同，但共用唯一环境 embedding 模型（`KNOWLEDGEBRAIN_EMBEDDING_MODEL`）。启用知识库时，API/worker 未同时配置 embedding URL 和 model 不得对外 ready。

- **产品问答向量**：存于 `knowledge_chunk_embeddings`，版本身份为 `knowledge_product_versions.embedding_model_id`，写入前由 `freeze_version_embedding_model` 冻结为 `platform::embedding_model()`；空值和历史 `stub-emb` 不是模型名，`embed_http` / `embed_index` 不接收外部模型参数。
- **招标 V3 向量**：存于 `knowledge_chunk_vector_indexes`，身份为不可变 `knowledge_embedding_revisions`；同一版本上已有 `knowledge_product_version_embedding_bindings` 时，其 `provider_model_identifier` 必须等于环境模型。
- **不自动重建历史索引**：未绑定模型却已有问答向量时，拒绝继续写入、拒绝用环境模型查询。

## 4. KnowledgeRetrievalPortV3

`retrieve_evidence_v3(KnowledgeEvidenceScopeV2) -> KnowledgeEvidenceBatchV3` 是招投标调用知识库的唯一业务端口。请求只表达冻结检索范围、requirement 文本/身份、版本选择和检索 policy 身份，不得包含投标项目表名、matching job 或报价模型。

```text
KnowledgeEvidenceBatchV3
  schema_version = 3
  eligible_versions: EligibleEvidenceVersionV1[]
    (product_id, product_version_id, workspace_kind=product_line|company, frozen_display_name)
  hits: KnowledgeEvidenceHitV3[]
    (schema_version=3, document_id, source_chunk_id, product_id, product_version_id,
     workspace_kind, frozen_document_display_name, chunk_utf8, chunk_sha256, chunk_byte_length,
     quote_start_offset, quote_end_offset, offset_unit=utf8_byte, retrieval_rank,
     retrieval_raw_score, retrieval_contract_version=knowledge-evidence-v2,
     media: KnowledgeEvidenceMediaV1?)
```

### 端口与图片证据不变量

- `eligible_versions` 返回本次请求对应 `workspace_kind` 的完整 current eligible version 集合，不受 hit 配额截断；`product_line` 仅含产品版本，`company` 仅含资料库版本。
- 每个 hit 必须属于同 batch 的 `eligible_versions`；`eligible_versions` 非空且 `hits=[]` 为合法结果（调用方记为 `NO_EVIDENCE`，不得误判为无 eligible 范围）；`company` hit 不构成产品排名。
- `quote` 是 chunk 的连续 UTF-8 byte slice，offset/digest/length 可验证；端口返回后 live Document 的修改或删除不影响调用方已冻结产物。
- `source_type=image_ocr` 必须携带不可变 `media`（`knowledge_image_artifact_revisions`、ObjectRegistry ref/digest/media type、尺寸、页码/区域与冻结文件名）；`text|parent_text` 必须 `media: null`。未建立可信 `knowledge_image_chunk_links` 映射的 OCR chunk 在所有检索路径中被排除。
- 图片摄取在同一事务发布 chunk、embedding、`knowledge_image_artifact_revisions`、`knowledge_image_chunk_links` 与 `knowledge_image_artifact` 对象引用；输入 OCR 图片身份固定为 `objects/{sha256}`，digest、格式、尺寸、ObjectRegistry `byte_length` 与幂等冲突全部 fail-closed。live 资料退休时图片对象引用继续保留不可变对象；media 存在不代表正文 DOCX 已包含图片。

## 5. `crates/knowledge` 真源与目录边界

真源是 **PostgreSQL**，任务协调是 **Oxana 队列**，没有进程级内存库。`DocJob` / `WikiJob` 仅代表单次 job 的工作行集：SQL 读入 → 纯函数处理 → SQL 事务写回 → 按需 enqueue 下一队列。

```text
crates/knowledge/src/
  lib.rs                    薄门面：显式 pub use，禁止 glob，不转发 platform
  catalog/                  知识资产 SQL（workspace / product / version / document / chunks / spans / tags）
  identity/                 platform_users / platform_api_keys 读写 SQL 辅助
  ingest.rs                 convert / fanout / kb delete / reparse
  pipeline.rs               解析后队列 job（summary / questions / image / post_process / extract / wiki / datatable）
  job.rs                    DocJob、WikiJob
  process.rs                解析引擎与切块覆盖
  obs.rs                    span 只写 PG
  chunker/                  切块
  enrichment/               OCR / 摘要 / 问答
  index/                    关键词与向量写入
  search/                   产品问答（hybrid_search_pg / matching_pg / assembly_pg）
  knowledge_retrieval.rs    投标证据端口 DTO/trait
  knowledge_retrieval_pg/   Postgres 适配
  knowledge_index_v2/       语义索引子模块（入口 knowledge_index_v2.rs）
  wiki/                     Wiki 算法；SQL 在 wiki/sql.rs
  graph/                    图谱算法；SQL 在 graph/sql.rs
  clone/                    版本克隆
  models/                   LLM HTTP
  workflow/                 知识库异步工作流（入口 workflow.rs）
  store.rs                  领域 DTO（无 HashMap 内存库）
  status.rs                 ParseStatus 等枚举
  persistence_api.rs        列表/slug 等只读 API SQL
  formula.rs                子任务计数
```

### 禁止项与验证口径

- 禁止新建第二个 knowledge crate、恢复 `persist.rs`、引入 `Store` HashMap / `hydrate_document` / `hydrate_version` / `write_back` 或 Redis 文档缓存。
- 禁止为速度把投标 V3 改成 ANN / 过采样召回或新增独立向量库；禁止用空值或 `stub-emb` 冒充真实 embedding 模型。
- 网络转换必须携带调用方 `CancellationToken`（`convert_with_cancel`、anydoc→builtin 回退、`convert_tender_source`、HTTP 引擎）。
- 回归检查：`rg 'Store::default|hydrate_version|hydrate_document|write_back' crates/knowledge crates/worker` 为空；`rg freeze_version_embedding_model crates/knowledge` 命中 `ingest.rs` 与 `pipeline.rs`；`embed_index(` 不带模型参数；招标证据仅经 `retrieve_evidence_v3`。
