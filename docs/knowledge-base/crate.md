# 知识库 crate 边界

| 项 | 值 |
| --- | --- |
| 状态 | 已落地（含问答 embedding 身份冻结） |
| 服务对象 | `crates/knowledge` 目录职责与 ingest 真源 |

领域语义与招标端口仍以 [`domain.md`](domain.md) 为准。本文只约束 crate 内部怎么切、job 怎么读写。不改 Workspace / Product / Document 业务规则，不改 `KnowledgeRetrievalPortV3`。

实施记录见 [`../../plans/knowledge-base/crate-module-boundaries.md`](../../plans/knowledge-base/crate-module-boundaries.md)。

## 1. 真源

- 业务行在 **PostgreSQL**。
- 任务协调在 **Oxana 队列**。
- 没有进程级内存库。禁止 `Store` HashMap、`hydrate_document` / `hydrate_version`、job 结束后靠 `write_back` 猜要写哪些表。
- `DocJob` / `WikiJob` 只是**这一次 job** 的工作行集：SQL 读入 → 纯函数处理 → catalog/index SQL 写回 → 必要时再 enqueue。

```text
Oxana job
  → catalog / wiki/sql / graph/sql 读当前文档或版本所需行
  → DocJob 或 WikiJob（仅本任务）
  → chunker / enrichment / wiki / graph
  → SQL 事务写回
  → 必要时 enqueue 下一队列
```

Wiki ingest 需要版本内多页时，用 SQL 查出所需 page/chunk，不要把整版本无关索引灌进进程。

## 2. 目录（`crates/knowledge/src/`）

```text
lib.rs                 薄门面：显式 pub use，禁止 glob，不转发 platform
catalog/               知识资产 SQL（workspace / product / version / document / chunks / spans / tags）
identity/              用户、成员、API Key SQL（仍在本 crate，不迁 platform）
ingest.rs              convert / fanout / kb delete / reparse
pipeline.rs            解析后队列 job（summary / questions / image / post_process / extract / wiki / datatable）
job.rs                 DocJob、WikiJob
process.rs             解析引擎与切块覆盖
obs.rs                 span 只写 PG
chunker/               切块
enrichment/            OCR / 摘要 / 问答
index/                 关键词与向量写入
search/                产品问答（hybrid_search_pg / matching_pg / assembly_pg）
knowledge_retrieval.rs 投标证据端口 DTO/trait
knowledge_retrieval_pg/ Postgres 适配
wiki/                  Wiki 算法；SQL 在 wiki/sql.rs
graph/                 图谱算法；SQL 在 graph/sql.rs
clone/                 版本克隆
store.rs               领域 DTO（无 HashMap 库）
status.rs              ParseStatus 等枚举
persistence_api.rs     列表/slug 等只读 API SQL
formula.rs             子任务计数
knowledge_index_v2.rs  语义索引合同
models/                LLM HTTP
```

新 SQL 写进对应目录，禁止再恢复 `persist.rs`。

`lib.rs` 用显式 `pub use catalog::load_document` 等保持旧短名，供 api/worker/bidding 使用。队列常量、hash、文件类型判断从 `platform::` 引，不经 knowledge 转发。

产品问答与投标证据是两套检索，入口分开；不在本文合并算法。共用一个环境 embedding 模型：`embed_http` / `embed_index` 不接收模型参数；写 `chunk_embeddings` 前 `freeze_version_embedding_model` 把 `embedding_model_id` 冻成 `platform::embedding_model()`。

## 3. 禁止

- 第二个 knowledge crate。
- 把整版本 hydrate 换成 Redis 文档 Map 或其它全局缓存。
- 改 DocReader / 招标解析状态机。
- 改 `KnowledgeRetrievalPortV3` 语义或 attestation SQL 合同。
- 把用户表迁出本 crate（只放 `identity/`）。
- 新增独立向量库；投标 V3 不得为了速度改成 ANN / 过采样召回。
- 用空值或 `stub-emb` 冒充真实 embedding 模型。问答写入前把 `product_versions.embedding_model_id` 冻成 `KNOWLEDGEBRAIN_EMBEDDING_MODEL`；HTTP 只使用该环境模型，不另传模型参数。
- API/worker 在 embedding URL 或 model 缺失时对外 ready。

## 4. 验证口径

- `rg 'Store::default|hydrate_version|hydrate_document|write_back' crates/knowledge crates/worker` 应为空。
- API 文档列表/详情/问答走 catalog + search；问答查询先 `query_embedding_model_id`。
- 招标证据只经 `retrieve_evidence_v3`。
- `rg freeze_version_embedding_model crates/knowledge` 命中 ingest / pipeline；`embed_index(` 无模型参数。
- 网络转换必须带调用方 `CancellationToken`：`convert_with_cancel`、anydoc→builtin 回退、`convert_tender_source`、HTTP 引擎；空 token 只留在无停机包装 `convert` / `convert_with`。
