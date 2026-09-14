# Embedding 身份冻结

| 项 | 值 |
| --- | --- |
| 状态 | **已落地** |
| 规范 | [`docs/knowledge-base/crate.md`](../../docs/knowledge-base/crate.md)、[`domain.md` §1.4](../../docs/knowledge-base/domain.md) |
| 代码 | `index/mod.rs`、`catalog/version.rs`、`ingest.rs`、`pipeline.rs`、`search/mod.rs`、`platform/probe.rs`、`api` `patch_version` |

## Context

crate 拆分之后，产品问答索引仍会把环境里的真模型向量写入 `chunk_embeddings`，却把版本列留成 `stub-emb`。本记录是那次冻结的完成说明。

### 问题：向量写进库了，但没记下是哪个模型算的

知识库问答用 `chunk_embeddings` 里的 1024 维向量做相似度。每个产品版本有一列 `embedding_model_id`，本来应该记下「这批向量是用哪个模型算的」。

实际发生的是：

1. 新建版本时这列固定写成占位符 `stub-emb`（测试/没配 embedding 服务时用本地假向量）。
2. 生产一旦配了 embedding HTTP，写索引的函数发现是 `stub-emb`，就**偷偷改用环境变量里的真模型**去算向量。
3. 算完只把数字写入 `chunk_embeddings`，**不把真模型名写回** `embedding_model_id`，版本行仍显示 `stub-emb`。
4. 以后换环境模型、或另一台机器 env 不同，会再往同一张表写入另一套向量。两边都是 1024 维，数据库看不出差别，问答分数会 silently 错。

部署里**只有一个** embedding 模型（`KNOWLEDGEBRAIN_EMBEDDING_MODEL`）。问答表和投标 V3 表可以仍是两张（检索合同不同），但写入时必须用**同一个模型名**。禁止问答偷偷用 env、列上还写 `stub-emb`，也禁止问答冻成 A、V3 revision 却是 B。

不合并两张向量表、不改 V3 排序/attestation、不改 DocReader、不重算旧向量。

文档收口已做：`worker-runtime.md` 标成已落地；向量 SLO 不列入当前计划。

## Approach

**前提：进程不配 embedding 就不能对外服务。** `KNOWLEDGEBRAIN_EMBEDDING_BASE_URL` 与 `KNOWLEDGEBRAIN_EMBEDDING_MODEL` 都非空，否则 API/worker `/ready` 为 NotReady（`platform::check_readiness`）。全库只有这一个模型名 = `platform::embedding_model()`（live）。

知识库 crate 的 **unit 测试**仍可不设这两项 env，走本地 stub 向量；那不是可启动的服务。

live 必须等于该 version 上已有 V3 binding 的 `provider_model_identifier`（若已绑定）。问答 `embedding_model_id` 冻成这同一个字符串。

**打 embedding HTTP 不再传入模型参数**，固定用 `platform::embedding_model()`。版本列只做记账和冲突检查，避免再出现「参数是 stub、请求却是 env」。

```text
live = platform::embedding_model()   // ready 已保证非空

insert_version：直接写入 live（不再默认 stub-emb）

写 chunk_embeddings 前 freeze(version):
  V3 binding 若存在且模型名 ≠ live → 拒绝
  embedding_model_id 未绑定（空/stub-emb，仅历史行）：
    已有向量 → 拒绝（不准猜成 live）
    否则 UPDATE 成 live
  已冻结 ≠ live → 拒绝（配置漂移）
  已冻结 = live → 用该 id embed

问答查询未绑定 → 失败
PATCH embedding_model_id 只能是 live；已有向量且改名 → 400
```

两张向量表保留（问答 LIMIT 近邻 vs 投标按 revision 全量打分）。不合并表、不改 V3 合同。

向量性能不列入当前计划。`worker-runtime.md` 是完成记录，不是待办。

### 向量检索（结论：不单独成计划）

不是缺功能，是护栏。不列入当前待办。

现状：

| 路径 | 代码 | 索引 | 语义 |
| --- | --- | --- | --- |
| 产品问答 | `search/hybrid.rs`：`ORDER BY embedding <=> … LIMIT top_k` | `chunk_embeddings_hnsw` 已建 | 近似最近邻可用，慢了再 `EXPLAIN` |
| 投标证据 V2 | `knowledge_retrieval_pg/semantic_v2.rs`：eligible 集合上算全量 cosine，再量化/RRF/配额 | `chunk_vector_indexes_v2` 也有 HNSW | **必须确定性**；改成 ANN 会改命中集合，不是调优 |

没有查询计划、P95、规模数据。HNSW 已经在 baseline 里。再写一份「先测再调」的长计划，只会让人以为还欠一笔性能工作。

值得留下的只有两句（放进 `crate.md` 禁止项即可）：不新增第二个向量库；投标 V3 不得为了速度改成 ANN / 过采样召回。真正变慢时再写短计划，带 `EXPLAIN ANALYZE` 证据。

## Files to modify

- `crates/platform/src/probe.rs`：readiness 要求 embedding URL + model 非空
- `crates/knowledge/src/index/mod.rs`：`embed_http` / `embed_index` 不再接收 model 参数，HTTP 固定 `platform::embedding_model()`；无 env 的 unit 测试仍 stub
- `crates/knowledge/src/catalog/version.rs`：`insert_version` 写 live；`freeze_version_embedding_model`（含 V3 binding 同名校验）
- `crates/knowledge/src/lib.rs`：显式 `pub use` 上述函数
- `crates/knowledge/src/ingest.rs`：`persist_document_embeddings` 先 freeze 再 `index_chunks`
- `crates/knowledge/src/pipeline.rs`：`run_datatable` / `run_summary` / `run_questions` / `run_image` 在 index 前 freeze，写回 `job.version.embedding_model_id`
- `crates/knowledge/src/pipeline.rs` wiki ingest/finalize locked：`from_pool` 前 freeze `version_id`
- `crates/knowledge/src/search/mod.rs`：query 用 `query_embedding_model_id`
- `crates/api/src/routes.rs`：`patch_version` 改 embedding 且已有向量时拒绝
- `docs/knowledge-base/crate.md`：补一句写入冻结规则
- `docs/knowledge-base/README.md` / `plans/knowledge-base/README.md`：当前计划只留 embedding 身份（做完后移到已完成）
- `plans/platform/README.md`、`plans/platform/worker-runtime.md`：改成已完成（删掉「待实施 / consume.rs 9934 行」过期叙述）
- `plans/implementation-tasks.md` V1：标注不列入当前计划

## Reuse

- `platform::embedding_base_url` / `embedding_model`（`crates/platform`）
- `workspace_embedding_conflict`（`catalog/workspace.rs`）— workspace 内 current 产品必须同模型；本项再加 **version 已冻结 vs 环境** 与 **已有向量不可改 id**
- clone 已按 src/dst `embedding_model_id` 是否相同决定是否 `copy_document_index`（`clone/mod.rs`）
- V3：只读 `product_version_embedding_bindings_v2` + `embedding_revisions_v2.provider_model_identifier`，核对等于 live；不改 revision 写入、不改 attestation

## Steps

- [x] 1. readiness：API/worker 缺 embedding URL 或 model → NotReady
- [x] 2. `embed_http`/`embed_index` 去掉 model 参数，只用 live；unit 测试不设 env 仍 stub
- [x] 3. `insert_version` 写 live；`freeze_version_embedding_model`（V3 同名、未绑定无向量才绑定、其余拒绝）
- [x] 4. ingest convert 写 embeddings 前 freeze
- [x] 5. pipeline 各写向量 job 与 wiki ingest/finalize 前 freeze
- [x] 6. search PG 查询路径 `query_embedding_model_id`
- [x] 7. API patch 改模型：只能 live；已有向量且改名 → 400
- [x] 8. 单测：无 env 仍 stub；有 URL+model 时未绑定无向量可绑定；未绑定已有向量失败；冻成其它名失败；ready 缺配置 NotReady
- [x] 9. 文档：crate 写入冻结 + 启动必配 embedding；拿掉向量 SLO 待办；worker-runtime 改成已落地

## Verification

- `cargo test -p knowledge --lib`（含新身份测试）
- `cargo check -p api -p worker`
- 无 HTTP：现有 stub 测试仍过
- 有 HTTP fixture 时：空/`stub-emb` 且无向量 → version 行变成 env model 再写向量；已有向量的 stub 版本再 ingest 失败且不写新向量
- `rg 'Store::default|hydrate_version|hydrate_document|write_back' crates/knowledge crates/worker` 仍为空
