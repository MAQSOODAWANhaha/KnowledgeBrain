# KnowledgeBrain 规格

| 项 | 值 |
|---|---|
| 状态 | **定稿**（2026-08-19 按 `docs/bid-platform-domain.md` 修订 Workspace / `/match` / 鉴权 / VLM） |
| 日期 | 2026-08-19 |
| 仓库 | `/opt/workspace/code/KnowledgeBrain` |
| 管线对照 | `/opt/workspace/code/brain`（WeKnora）。任务类型、队列名、`parse_status`、分块算法、Wiki/图谱作用域与 brain **同语义** |
| 投标 | 同仓。HTTP 挂 `api`，作业挂 `worker` 新队列。抽取按段扇出、覆盖率可见。领域见 `docs/bid-platform-domain.md` |

本文只描述**已确定的结构**。实现按本文执行，不另开管线。Workspace / `/match` / 鉴权 / VLM 与投标草案冲突时以草案为准。

---

## 1. 系统结构

### 1.1 进程

| 进程 | 路径 | 端口 | 职责 |
|---|---|---|---|
| `api` | `crates/api` | `:8080` | HTTP：LDAP/JWT/API key、领域 CRUD、入队、检索、文件代理、投标 CRUD、静态前端（`KNOWLEDGEBRAIN_WEB_ROOT`）、`/system/parser-engines`、`/ops/oxana` |
| `worker` | `crates/worker` | 无 | 6 个 oxana Runtime + 投标队列（`bid:convert` / `bid:extract` / `bid:match`） |
| `docreader` | `services/docreader` | gRPC `:50051` | 文件/URL → Markdown + 图片字节 |

第一期**不**新开投标业务容器。投标与知识库共用 Postgres / Redis / 对象桶 `objects/{sha256}`。

数据面：Postgres（领域 + chunks + 向量 + wiki + 图 + spans + 死信）、Redis（oxana + 分布式锁）、对象存储（S3/MinIO/local，桶 `KNOWLEDGEBRAIN_S3_BUCKET`，键 `objects/{sha256}`）。可选 Neo4j（`KNOWLEDGEBRAIN_NEO4J_HTTP_URL`）：图谱双写投影，**不**门闩 extract。

```
Client ── JWT 或 API key ──► api ──► Postgres / Redis / Object
                                 │
                                 │ oxana enqueue
                                 ▼
                            worker ──► DocReader (gRPC ReadStream)
                                      ──► MinerU / Paddle / LLM
                                      ──► Postgres / Object
```

HTTP **只**做校验、落盘、建 `pending` 行、入队。解析、分块、向量、多模态、摘要、问题、Wiki、图谱全部在 worker 完成。

### 1.2 仓库

```
KnowledgeBrain/
  Cargo.toml
  crates/
    domain/        # ID、状态机、配置、payload
    auth/          # JWT、API key、workspace_members
    models/        # 模型目录
    runtime/       # oxana 注册、enqueue、死信回调
    storage/       # 对象存储
    api/           # 二进制 api
    worker/        # 二进制 worker
    docparser/     # Rust convert / simple / MinerU / Paddle / 图片 / DocReader 客户端
    chunker/       # 分块
    index/         # processChunks、pgvector、tsvector
    enrichment/    # multimodal / summary / question
    graph/         # 实体关系 upsert
    wiki/          # wiki ingest / finalize
    clone/         # version:clone
    search/        # 检索
    bid/           # BidProject / 抽取 / 匹配作业 / 预览（api 与 worker 链接）
    obs/           # spans、Prometheus
  services/
    docreader/     # Python 解析进程 + proto 真源（自 brain/docreader 复制，去掉 *.pb.go）
      proto/docreader.proto
      pyproject.toml
      ...
  migrations/
  testdata/        # 从 brain 拷贝 docreader / chunker / wiki_test fixtures
  deploy/          # compose、.env.example、镜像、部署说明
  docker-compose.yml  # 转发到 deploy/docker-compose.yml
```

依赖方向：

```
api     → auth, domain, runtime, search, bid, obs, storage, models, docparser
worker  → runtime, docparser, chunker, index, enrichment,
             graph, wiki, clone, obs, models, storage, domain, bid, search
runtime → domain, oxana
docparser → domain, storage, tonic
index   → domain, models
```

约束：`domain` 不依赖 oxana / axum / tonic；`chunker` 不依赖网络；Rust 不嵌入 Python。

### 1.3 依赖（workspace）

```toml
[workspace]
resolver = "2"
members = [
  "crates/domain", "crates/auth", "crates/models", "crates/runtime",
  "crates/api", "crates/docparser", "crates/chunker", "crates/index",
  "crates/enrichment", "crates/graph", "crates/wiki", "crates/clone",
  "crates/search", "crates/bid", "crates/obs", "crates/storage", "crates/worker",
]

[workspace.package]
edition = "2024"
rust-version = "1.97"

[workspace.dependencies]
tokio = { version = "1.43", features = ["rt-multi-thread", "macros", "time", "signal"] }
axum = "0.8"
tower-http = { version = "0.6", features = ["cors", "trace", "limit"] }
sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "postgres", "uuid", "chrono", "json"] }
oxana = { version = "2.1", features = ["registry"] }
oxana-web = "2.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
jsonwebtoken = "9"
sha2 = "0.10"
hex = "0.4"
tonic = "0.13"
prost = "0.13"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
object_store = { version = "0.12", features = ["aws"] }
pgvector = { version = "0.4", features = ["sqlx"] }
prometheus = "0.14"
```

`services/docreader` 使用 brain 的 `pyproject.toml` 依赖集（grpcio、pypdfium2、markitdown、openpyxl 等）。Rust `docparser` 用 tonic 从 `services/docreader/proto/docreader.proto` 生成客户端。

---

## 2. 领域

两条正交轴：

| 轴 | 实体 | 回答的问题 |
|---|---|---|
| 快照 | `ProductVersion` | 哪一套（产品发版 / 资料批次） |
| 分类 | `Tag` | 哪一类（手册 / 规格 / 营业执照 / ISO） |

Workspace 另有 `kind`，只表示**池子**，不再当 ACL 根：

```
Workspace kind=product_line       产品线（可多条）
  └── Product (kind=product)      型号
        └── ProductVersion        发版快照 ≡ KnowledgeBase
              └── Document
                    ├── tags[]
                    └── Chunk / 向量 / 抽取

Workspace kind=company            恰好一条（slug=company）
  └── Product (kind=library)      分类夹：资质证照 / 体系认证 / 业绩案例 / 服务能力
        └── ProductVersion        资料批次
              └── Document        必须进向量，商务 /match 才打得到

BidProject                        与 Workspace 平级；见 bid-platform-domain.md
```

文档只挂在某一个 `ProductVersion` 上，不跨版本共享同一 `document_id`。Workspace 不直接存知识。Wiki 一本、图谱一份，按 `product_version_id` 隔离。TAG 不改变归属、不改变 Wiki/图谱命名空间、不替代 Version。

招标文件与人补截图**禁止**写入任一 ProductVersion。

对照：

| brain | 本系统 |
|---|---|
| Tenant | Workspace |
| KnowledgeBase | ProductVersion |
| Knowledge | Document |
| KnowledgeTag（KB 内标签） | Tag（Workspace 内标签） |

### 2.1 表

**workspaces**：`id, name, slug unique, kind ∈ {product_line, company}, retrieval_config jsonb, created_at, updated_at`
无配额列。无 TOKEN / 计费字段。`kind=company` 部分唯一（恰好一条）。存量回填 `kind=product_line`。bootstrap：`slug=company`、`kind=company`。`POST /workspaces` 默认 `kind=product_line`，禁止再建第二条 company。

**users**：`id, email unique, password_hash null, ldap_dn null, created_at, updated_at`
登录走 LDAP bind，首次成功则插入。`POST /auth/register` **关闭**。

**workspace_members**：`(workspace_id, user_id)` PK，`role ∈ {owner, admin, contributor, viewer}`
第一期**留表、不当门闩**。登录用户或已认证 API key 可读写全部 Workspace / 产品 / 文件 / 投标。

**products**：`id, workspace_id FK, kind ∈ {product, library}, name, slug unique(workspace_id, slug), current_version_id`（无 FK）

- `kind=product`：型号文档。只出现在 `kind=product_line` Workspace。
- `kind=library`：分类夹。company 下用来挂资质 / 体系 / 业绩案例 / 服务能力。产品线 Workspace **不再**自动插默认 library。
- 存量产品线里 `slug=library` 的默认行：**冻结写入**（上传/clone 进该产品 409），文档迁到 company 对应分类夹。删除该默认行仍不允许（可改名）。company **不**插默认 library。
- `current_version_id` 对两种 kind 同义：产品的当前发版，或资料的当前批次。

**product_versions**：`id, product_id, label, status ∈ {cloning, active, archived, failed}, cloned_from_version_id, chunking_config, indexing_strategy, image_processing_config, embedding_model_id, summary_model_id, vlm_model_id, asr_model_id, vlm_config, asr_config, extract_config, wiki_config, question_generation_config, created_at, updated_at, deleted_at`  
活行唯一 `(product_id, label) WHERE deleted_at IS NULL`（软删后可再用同 label）。新建默认 `indexing_strategy` 四开关全 `true`。`enable_multimodel` 默认 **true**（产品线与 company 的 current 都开）。`vlm_configured()` **只**认 `KNOWLEDGEBRAIN_VLM_BASE_URL`（及对应 KEY/MODEL），禁止回落到 `LLM_BASE_URL` / chat URL。存量 current 回填打开。

**documents**：对齐 brain `knowledges`。关键列：`id, product_version_id, type, title, parse_status, pending_subtasks_count, summary_status, enable_status, index_ready, file_name, file_size, file_hash, object_key, process_overrides, error_message, processed_at, deleted_at`
去重唯一：`(product_version_id, file_name, file_size, file_hash)`。`file_hash` = sha256 hex。  
`enable_status`：入库 `disabled`；`processChunks` 写完 chunk/索引后改为 `enabled`（此时即可检索，`parse_status` 仍可能是 `processing`）。reparse 开始先改回 `disabled`。
`index_ready`：convert 完成 ∧（无图 ∨ multimodal 成功写回）。有图但 VLM 未配/失败：原文可看，`parse_status=finalizing` 且露出 `ocr_error`，**`index_ready=false`，不自动商务匹配**。商务自动 `/match` 与招标切条只看 `index_ready`，**不**等 wiki/graph 的 `parse_status=completed`，也**不**在仅 `enable_status=enabled` 时写商务 miss。

**tags**：`id, workspace_id FK, name, slug unique(workspace_id, slug), created_at`

**document_tags**：`(document_id, tag_id)` PK。TAG 只作过滤与导航，不参与解析、Wiki Scope、图谱 namespace。

**content_objects**：`hash PK, size, refcount`。`refcount=0` 删 blob。

**chunks**：对齐 brain `chunks`，外键 `product_version_id` / `document_id`。`start_at`/`end_at` 为 rune。`chunk_type` 取值与 brain 相同：`text` / `parent_text` / `image_ocr` / `image_caption` / `summary` / `entity` / `relationship` / `table_summary` / `wiki_page`。`parent_chunk_id`、`pre_chunk_id`、`next_chunk_id`、`context_header`（或等价列/metadata）、`image_info` 保留。

**chunk_embeddings**：`chunk_id PK, product_version_id, document_id, embedding vector(D), tsv tsvector, content`。`D` = 1024（`models.stub-emb` 与 HTTP embedding 同宽；列见 `0004`）。配置 `KNOWLEDGEBRAIN_EMBEDDING_BASE_URL` 时走 OpenAI 兼容 `/v1/embeddings`（请求带 `dimensions=1024`）；未配置或维度不符回 hashed stub。HNSW `vector_cosine_ops` + GIN(tsv)。换 embedding 模型必须对该版本全部文档 `list_reparse`，禁止只改 `embedding_model_id`。同一 Workspace 内参与 matching 的产品必须同一 `embedding_model_id`（建 Product / 改 current 时校验）。

**graph_nodes**：`UNIQUE (product_version_id, document_id, name)`，`chunk_ids uuid[]`。upsert：同名则 `chunk_ids` union。

**graph_relations**：`UNIQUE (product_version_id, document_id, node1, node2, rel_type)`，upsert。

**wiki_pages**：`id, product_version_id, slug unique(product_version_id, slug), title, page_type, status ∈ {draft, published, archived}, content, summary, aliases jsonb, parent_slug, folder_id, category_path jsonb, source_refs jsonb, chunk_refs jsonb, created_at, updated_at, deleted_at`

**wiki_folders**：`id, product_version_id, parent_id, name, path, depth, sort_order, created_at, updated_at, deleted_at`

**wiki_log_entries**：`id, product_version_id, document_id, level, message, created_at`

**document_processing_spans**：`(document_id, attempt, name)` upsert。列：`span_id, parent_span_id, kind, status, input/output/metadata jsonb, error_code, error_message, started_at, finished_at, duration_ms`

**task_pending_ops**：`id, task_type, scope, scope_id, op, dedup_key, payload jsonb, fail_count, enqueued_at, claimed_at`。wiki 的 `scope=product_version`，`scope_id=product_version_id`。无 tenant 列。

**task_dead_letters**：`id, task_type, scope, scope_id, related_id, payload jsonb, last_error (8KB cap), fail_count, failed_at`。无 TTL。

**retrieval_config**（挂 `workspaces.retrieval_config jsonb`，缺省如下）：

| 项 | 默认 | 含义 |
|---|---:|---|
| `vector_threshold` | 0.15 | 向量命中下限（0–1） |
| `keyword_threshold` | 0.3 | 关键词命中下限（0–1） |
| `embedding_top_k` | 50 | 单目标向量召回上限 |

无 rerank（v1 不做）。matching 条款 `hit=true` 当且仅当该条款最高分 ≥ 对应通道阈值。

**models**：`id, kind ∈ {embedding, chat, vlm, asr}, endpoint, api_key_enc, dimension, extra jsonb`

**api_keys**：`scope_type ∈ {workspace, product}, scope_id, scopes[] ∈ {ingest, search, admin}`

迁移分批：`0001` 领域（含 `workspaces.kind`、`documents.attempt` / `description` / `source_passages` / `index_ready`、tags）+成员+spans+pending+DL；`0002` models；`0003` api_keys；`0004` embeddings `vector(1024)`；`0005` graph；`0006` wiki（含 `chunk_refs`）；`0007` Bid*（含勾选段 / 人评 / 成稿分册）；`0008` 存量回填。API / worker 必须在 ready / 消费队列前完成连接、迁移和公司工作区初始化，任一步失败即启动失败。

### 2.2 配置合并

挂在 ProductVersion。单次上传 `process_overrides` 合并规则与 brain `ResolveProcessConfig` 相同：指针 0/空保留 base；`EnableParentChild` 整值覆盖；`ParserEngineRules` 非空整表替换；`GraphEnabled` 还要 `AND extract_config.enabled`。

### 2.3 library 与 TAG

- 解析 / 分块 / Wiki / 图谱 / clone 对 `kind=product` 与 `kind=library` **同一套**。
- **投标商务**只扫 `kind=company` Workspace 下 library 的 current，**不**把 library hit 记到产品线产品的分数上。旧的请求级 `include_library` / 条款 `use_library` 仅保留给不带 `scope` 的单仓旧调用。
- 换证、年审、换案例批次 = 对 company 下对应 library 做 `version:clone`。旧批次 `archived` 后默认不再命中。
- TAG 可在上传时带、也可事后改。改 TAG 不触发 reparse。Tag 按 Workspace 隔离（`界面` 不是硬门闩）。
- 一篇文档多个 TAG；一个 TAG 多篇文档。文档不能靠打多个「版本 TAG」属于两个 ProductVersion。
- 删除 TAG：只删 `document_tags` 行，不删文档。

### 2.4 投标表

见 `docs/bid-platform-domain.md`。本库 Postgres 增 BidProject / BidDocument / BidSection / BidExtractRun / BidClause / BidMatchJob / BidSectionPick / BidCommercialHit / BidShot / BidBookletPart。不另建库。条款抽取统一经 async `TenderExtractionEngine`：内含独立技术/商务 ToolChat、版本化 `cn-tender-v2` Policy/Prompt、要求级稳定 Outline/Span、逐字 quote 回源、跨 family 仲裁和 Span 级 coverage；默认 `hybrid`，所有 fallback 与未覆盖 Span 落 `BidExtractRun.diagnostics`。项目 run 使用 token + heartbeat 租约；Section retry 先持久化独立 job，再由 worker 以同一 token 成对 claim job 与项目/Section，job 终态和项目租约释放同事务完成，回收后的旧 owner 不能写状态、report 或 finish。招标转换同样以 conversion generation/token fencing，配置 VLM 时图像处理失败会保持可见失败并可重试；每代转换只建立一个自动抽取 run。成功 report 才按文件事务替换 draft，抽取阶段禁止访问任何产品/公司知识库。技术匹配与勾选按勾选段（可 merge_into）；商务仍项目级。确认集/Section merge 变化按 project-first 锁序与 `match_generation/match_dirty` 在同一事务持久化，partial clause PATCH 只应用显式字段并在锁内推导 match 变化；调度器只能为读取快照时的 expected generation 建立 `(project,generation,job_kind,unit)` 唯一 job，明确区分商务与未归段技术，match worker 只有持有当前 generation/token 才能发布候选或商务命中，Redis 中断由 dirty/pending/stale recovery 补投。成稿 MD 过程可编；`GET /api/v1/bids/{id}/export` 渲当前稿为 Word，`?format=pdf` 定稿。不回写 docx。

---

## 3. 运行时管线

```
HTTP: 校验 → 落盘 → INSERT pending → enqueue
        │
        ▼
document:process          queue default
  passages：跳过 convert，每段一个 text chunk
  否则 convert(DocReader/simple/mineru/paddle)
  ASR（音频，写回 Markdown）
  图片落库、改写 Markdown
  chunker（或父子 SplitParentChild）
  processChunks（清旧 → 写 chunk → embed + tsv）
  EnableMultimodel 且有图 → image:multimodal
  否则 → knowledge:post_process
  无 text chunk 且无多模态：processChunks 内直接 completed
        │
        ▼
knowledge:post_process    queue postprocess
  SetFinalizing(N)
  fan-out:
    summary:generation
    question:generation × batches
    chunk:extract × text-like chunks
    wiki:ingest（计 1）
  FinalizeSubtask → 0 则 completed
```

### 3.1 队列（oxana 2.1）

6 个 oxana Runtime，队列 key 与 brain 相同：

| Runtime | 并发 | Queue key | Task type |
|---|---:|---|---|
| core | 8 | `default` | `document:process`, `manual:process` |
| postprocess | 2 | `postprocess` | `knowledge:post_process` |
| enrichment | 12 | `summary`, `multimodal`, `graph`, `question` | `summary:generation`, `datatable:summary`, `image:multimodal`, `chunk:extract`, `question:generation` |
| maintenance | 4 | `sync`, `low` | `version:clone`, `kb:delete`, `knowledge:list_delete`, `knowledge:list_reparse`, `index:delete` |
| shared | 6 | `summary`, `multimodal`, `graph`, `question`（**同一 key**） | 与 dedicated 竞争，一条任务只跑一次；`default`/BID 只由 core 注册，避免重复消费拓扑 |
| wiki | 8 | `wiki` | `wiki:ingest`, `wiki:finalize` |

shared 不注册 `postprocess` / `low` / `sync` / `wiki`。maintenance 物理队列名仍为 `low`。

仍存在的 brain task type **不改名**。仅新增 `version:clone`。v1 不实现：`temporary_document:process`、`faq:import`、`datasource:sync`、`knowledge:move`、`kb:clone`。

并发环境变量：`KNOWLEDGEBRAIN_{CORE,POSTPROCESS,ENRICHMENT,MAINTENANCE,SHARED,WIKI}_CONCURRENCY`。

oxana 能力使用：独立 concurrency、`max_retries` / `retry_delay`、`unique_id` + `on_conflict=Skip`、scheduled job、dead letter、`oxana-web` nest 到 `/ops/oxana`。

超时：`document:process` 2h；`knowledge:post_process` 30min；单次 DocReader 30min。Wiki lock 冲突固定 15s 再试。

取消信号只有 `parse_status ∈ {cancelled, deleting}`。不靠看板删任务。

死信回写：`document:process` / `knowledge:post_process` / `manual:process` → 父文档 `failed`。`image:multimodal` 不 fail 父文档。`knowledge:list_delete` 只更新仍为 `deleting` 的行。

### 3.2 parse_status

```
pending → processing → finalizing → completed
              ↘ failed | cancelled | deleting
```

| 状态 | 含义 |
|---|---|
| pending | 已入库，等 worker |
| processing | 解析 / 分块 / 向量进行中 |
| finalizing | 主索引完成，enrichment 未完，仍可检索 |
| completed | 应跑的子任务全部结束 |
| failed | 不可恢复或最后一次重试失败 |
| cancelled | 用户取消；保留已写 chunks，可 reparse |
| deleting | 删除中；在途任务短路 |

合法转移与 brain 相同。规则：

1. 入口：`deleting`/`cancelled` 退出；`completed` 幂等跳过；`failed` 允许重试。
2. 写成 `processing` 前再读一次 abort。
3. `SetFinalizing(id, n)`：`WHERE parse_status='processing'` 原子写入 `finalizing` + `pending_subtasks_count=n`。
4. `FinalizeSubtask`：计数减 1，再无条件尝试 `finalizing AND count=0` → `completed`。
5. 入队失败：行已在，标 failed，HTTP 仍 200 返回该行。

`summary_status`：`none | pending | processing | completed | failed`。

### 3.3 attempt 与 span

首次 parse `attempt=1`；用户 reparse `max+1`；oxana 重试同一 attempt。

Span DAG：

```
docreader → chunking → embedding
                  ↘ multimodal
                        ↘ postprocess   // join(embedding, multimodal)
```

### 3.4 Payload

序列化只用新字段。反序列化按类型 alias：`knowledge_id`→`document_id`，`knowledge_base_id`→`product_version_id`。`tenant_id` 丢弃，不写入 `workspace_id`。

| 类型 | 必有字段 |
|---|---|
| `DocumentProcessPayload` | `document_id`, `product_version_id`, 文件/URL/passages, `attempt` |
| `KnowledgePostProcessPayload` | 同上 + `clone_keep: bool` 默认 false |
| `ExtractChunkPayload` | `chunk_id`, `document_id`, `attempt`；`product_version_id` 可从 chunk 行读 |
| `WikiIngestPayload`（trigger） | `product_version_id`；无 `document_id` |
| `VersionClonePayload` | `task_id`, `source_version_id`, `target_version_id`, `diffs[]` |

---

## 4. HTTP

前缀 `/api/v1`。鉴权：`Authorization: Bearer <jwt>` 或 `X-API-Key`。

| 方法 | 路径 |
|---|---|
| POST | `/auth/login`（LDAP；`/auth/register` **关闭**，410） |
| GET/PATCH | `/me` |
| GET/POST | `/workspaces` |
| GET/PATCH/DELETE | `/workspaces/{id}` |
| GET/POST/PATCH/DELETE | `/workspaces/{id}/members` |
| GET/PATCH | `/workspaces/{id}/retrieval-config` |
| GET/POST | `/workspaces/{id}/products`（`?kind=product\|library`） |
| GET/PATCH/DELETE | `/products/{id}` |
| GET/POST | `/products/{id}/versions`（POST 可 `clone_from` + diffs） |
| GET/PATCH | `/products/{id}/versions/{version_id}` |
| POST | `/products/{id}/current-version` |
| DELETE | `/products/{id}/versions/{version_id}` → 入队 `kb:delete` |
| GET | `/products/{id}/versions/{version_id}/documents`（parse_status / tag / keyword） |
| POST | `.../documents/file`、`url`、`manual`、`passage` |
| GET | `/documents/{id}` |
| POST | `/documents/{id}/reparse`、`/cancel` |
| DELETE | `/documents/{id}` |
| GET | `/documents/{id}/timeline`（span 树） |
| GET/POST | `/workspaces/{id}/tags` |
| PUT | `/documents/{id}/tags` |
| GET | `/products/{id}/versions/{vid}/wiki/pages`、`/wiki/pages/{slug}`、`/wiki/folders` |
| GET | `/products/{id}/versions/{vid}/files?key=` |
| GET | `/files?key=`（登录即可；招标原件 / 人补图 / 已引用对象） |
| GET/POST/PATCH | `/models`（admin） |
| POST | `/search`（`mode=assembly\|matching`） |
| POST | `/match`（可选 `scope=product_lines\|company`） |
| POST | `/answer` |
| GET/POST | `/bids` …（项目、文件、条款、勾选段、成稿 ①–⑤；见领域草案） |
| GET | `/ops/oxana/*`、`/ops/dead-letters`（admin） |

级联：删 Workspace → 入队删其下全部 ProductVersion（`kb:delete`）；删 Product 同理。有 `parse_status ∈ {pending, processing, finalizing}` 时先 cancel 再删。产品线存量默认 library 不可删、不可再写入。`GET /documents/{id}` 回 `object_key` / `file_name`。

`version_id=current` 解析为该 Product（含 library）的 `current_version_id`；空则 400。写入仅允许 `status=active` 的版本。TAG 的 CRUD 不影响解析状态。

### 4.1 上传校验

1. 调用方已认证（登录或 API key）。不再要求 Workspace 成员。写入冻结的产品线默认 library → 409。
2. 版本 `active`。
3. 扩展名白名单：`pdf,txt,docx,doc,epub,mhtml,md,markdown,png,jpg,jpeg,gif,csv,xlsx,xls,pptx,ppt,json,mp3,wav,m4a,flac,ogg`。视频拒绝。
4. `MAX_FILE_SIZE_MB` 默认 50。
5. 去重 `(version, filename, size, sha256)` → 409 + 已有 `document_id`。
6. URL：http(s) + SSRF 校验。
7. 图片无 VLM / 音频无 ASR → 400。

落盘：sha256 同时作 `file_hash` 与 object key；`refcount++`；INSERT `pending`；可选 `tag_ids`（必须属于同一 Workspace）；`OpenAttempt`；enqueue `document:process`（MaxRetry=3）。csv/xlsx/xls 另入 `datatable:summary`。

`kind=library` 的上传路径与 product 相同，只是 `product_id` 指向 library。

### 4.2 鉴权

内网一家公司。`POST /auth/login`：配了 `KNOWLEDGEBRAIN_LDAP_URL` 才 LDAP bind；空则测试模式，不验账号密码，缺省 `dev@local`，upsert `users` 后发 JWT。`/auth/register` 关闭。JWT：HS256，`JWT_SECRET`，TTL 24h。claims：`sub=user_id`, `exp`。

**不**按 `workspace_members` 挡列仓、传手册/证、读原件、`/match`、投标。角色表仍在，第一期登录用户视为可写全库。`POST /auth/register` 关闭。

API key：`scope_type=workspace|product` 仍可建；第一期**不按 key scope 挡**投标或带 `scope` 的 `/match`。只要能认证即可。bootstrap：`KNOWLEDGEBRAIN_BOOTSTRAP_KEY`。未认证全部 401。

SSRF：拦 loopback / 链路本地 / 私网 / `169.254.169.254` / DNS rebinding。DocReader 侧 `utils/ssrf.py` 为第二层。

---

## 5. 处理阶段

算法逐步对照 `brain/docs/knowledge-parse-and-chunk-spec.md`。下列为必须落地的契约；未列出的正则/伪代码按该文档与对应 Go 实现移植。

### 5.1 DocReader（Python）

路径：`services/docreader`。协议 **仅 gRPC** `DocReader`：

- `ReadStream`：默认。第一帧 `meta`（markdown / metadata / error / image_count），之后每帧一张 `ImageRef`。
- `Read`：一元回退（`Unimplemented` 时）。
- `ListEngines`

不分块、不 OCR、不写存储/DB。扫描页只出 JPEG，OCR/Caption 在 5.6。

引擎：anydoc（office 默认）、builtin（pdf / 扫描页回退；docx→Docx2，OLE magic `D0CF11E0` 当 doc）、markitdown、opendataloader（仅 pdf）。引擎不支持该类型 → 回退 builtin。URL 固定 `WebParser`。空 content → `error` 非空。文本去 UTF-8 surrogate。

### 5.2 convert（Rust）

对照 `knowledge_process.go::convert` / `resolveDocReader`。

抽出 **`convert_to_markdown(bytes, file_name) -> (markdown, images[])`**，不依赖 Document / ProductVersion。**解析只有这一条**：引擎由扩展名 + 产品默认 `parser_engine_rules` 决定，再 VLM 写回。`document:process` 与 `bid:convert` 都是调用方，不得各写一套。落盘才分叉：知识库进 Document/索引；招标只更新 `BidDocument`，不 `INSERT documents`。

**与 WeKnora 的差别（有意）：** 上游空引擎 = DocReader/MarkItDown；本仓 anydoc 已进程内集成。anydoc 明显更好的类型（docx/doc/xlsx/xls/pptx/ppt 的表与结构）**默认 anydoc**，不跟上游用 MarkItDown 当默认。PDF / 扫描页仍 builtin（版面 + 栅格化 OCR），anydoc 无文本层时回退 builtin。版本 `parser_engine_rules` 可覆盖。

| engine | 实现 |
|---|---|
| `simple` | Rust SimpleFormatReader |
| `anydoc` | 进程内 anydoc 0.1.9（docx/doc/pptx/ppt/xlsx/xls/odf/rtf/epub/csv/pdf）。不经 DocReader。纯 URL 拒绝。无文字层 PDF 回退 builtin，并打 `anydoc_fallback=scanned_pdf` / `image_source_type=scanned_pdf`。成功结果带 `parser` / `anydoc_version` / `source_format`。抽图开关：`parser_engine_overrides.anydoc_extract_images`。开启抽图时走文档模型：把 `ImageSource::Asset` 改写成 `images/image-N.ext` 后再序列化，图片留在原段落/表格/列表位置；`to_document` 失败则回退纯文本并记 `anydoc_assets_error`。 |
| `mineru` / `mineru_cloud` | Rust HTTP |
| `paddleocr_vl` / `paddleocr_vl_cloud` | Rust HTTP |
| `builtin` | **强制** gRPC DocReader，禁止 simple 兜底 |
| `""` 或其他 | `!is_url && is_simple_format` → Simple，否则 DocReader |

`ResolveParserEngine`：**精确字符串相等**。`is_simple_format`（小写去点）：`md, markdown, txt, text, csv, json, jpg, jpeg, png, gif, bmp, tiff, webp, mp3, wav, m4a, flac, ogg`。

Simple：md/txt 原样；csv→Markdown 表；json 展开；图→`![](images/…)` + ImageRef；音频→`IsAudio` + 原始字节，随后 ASR 写回 Markdown，无 ASR 配置则 failed。

DocReader 子超时 30min。`ReadResult.Error` 或 transport：非最后一次 retry；最后一次 `failed`。无 reader：「Document parsing service is not configured」立即 failed、不重试。

`GET /api/v1/system/parser-engines`：对照 brain `ListParserEngines`。合并本进程 convert 引擎（builtin / simple / anydoc / mineru / mineru_cloud / paddleocr_vl / paddleocr_vl_cloud）与 DocReader `ListEngines`。同名以远端 `file_types` / `description` 为准；远端独有引擎追加。anydoc 始终 available。

`payload.passages`：不调 convert/chunker.Split，每段一个 `text` chunk，直接 5.5。

### 5.3 图片落库

对照 `image_resolver.go`。内联 / 远端 http(s) → `objects/{sha256}`，改写 Markdown。远端图 ≤ `MAX_FILE_SIZE_MB`，SSRF 校验。部分失败只 warn。Serving：`GET .../files?key=`，仅本版本引用过的 key。

### 5.4 Chunker（Rust）

整包移植 `brain/internal/infrastructure/chunker`。配置映射 `buildSplitterConfigFromChunking`：默认 size=512、overlap=80、分隔符 `\n\n` / `\n` / `。`。父子：父 4096 overlap=base，子 384 overlap=size/5。

| Strategy | 链 |
|---|---|
| `auto` | Profiler → heading → heuristic → **legacy 永远兜底** |
| `heading` / `heuristic` | 该 tier → legacy |
| `legacy` / `recursive` / `""` | 仅 legacy（空串不是 auto） |

不变量（spec §13）：

- `end - start == rune_count(content)`
- `ContextHeader` 不进 Content
- 保护区（公式 / 图 / 链 / 表 / 代码）不可横切
- 表跨块补表头
- 单块硬顶 7500 rune；`ValidateChunks` 失败换下一 tier
- 父块 `chunk_type=parent_text` 只写 DB；子/扁平块 `text` 进向量

新建 ProductVersion 显式写 `strategy=auto`。

### 5.5 processChunks / 向量 / 关键词

对照 `processChunks` + `finalizeIndexedKnowledgeState`。

1. abort 检查。
2. `NeedsEmbedding = vector_enabled || keyword_enabled`。为真则加载 embedding 模型；加载失败 retryable，最后一次 fail 文档且不入队 post_process。
3. 删该 document 旧 chunks、旧 `chunk_embeddings`、`DelGraph({product_version_id, document_id})`。
4. 写全部 chunk 行（**即使关向量也写**，Wiki/图谱/摘要依赖文本）。
5. 仅对 **text 子块/扁平块** 建索引，**不含** `parent_text`。
6. 索引文本（与 brain 相同）：

```
titlePrefix = title 非空 ? title + "\n" : ""
index_content = titlePrefix + EmbeddingContent()
EmbeddingContent() = ContextHeader=="" ? trim(Content) : ContextHeader + "\n\n" + trim(Content)
```

7. `vector_enabled`：写入 `embedding`（模型维度，HNSW cosine）。
8. `keyword_enabled`：同一行写 `tsv`（GIN）。两开关都关则跳过 BatchIndex。
9. `enable_status=enabled`，写 `processed_at`。若还有多模态或 text chunk：`parse_status` 保持 `processing`。若既无 text 也无多模态：直接 `completed`，并置 `index_ready=true`。
10. `EnableMultimodel && StoredImages>0`：设 Redis `multimodal:pending:{document_id}=N`，入队 N 条 `image:multimodal`。此时 **`index_ready` 仍为 false**。否则立刻置 `index_ready=true` 并 enqueue `knowledge:post_process`。

检索过滤 `enable_status=enabled`。`parse_status=finalizing` 的文档已可搜。投标商务自动重搜与招标切条另看 `index_ready`。

### 5.6 image:multimodal

对照 `image_multimodal.go`。每图 OCR + Caption（`ImageProcessingConfig` / VLM 开关）。扫描 PDF 用 `image_source_type=scanned_pdf` 专用 prompt。子 chunk `image_ocr` / `image_caption`，`parent_chunk_id`=文本块，并按 5.5 规则索引。

defer：成功或最后一次重试才 DECR pending；≤0 或 DECR 失败 fallback → 置 `index_ready=true` 并 enqueue `knowledge:post_process`。document 已 cancelled/deleting：drop 但仍计数。单图死信不 fail 父文档，也不把 `index_ready` 提前打真。

### 5.7 post_process

对照 `knowledge_post_process.go::Handle`。`needs_embedding` = `vector_enabled || keyword_enabled`。

```
textLike = type ∈ {text, image_ocr, image_caption}
willSpawnSummary  = len(textLike) > 0
willSpawnQuestion = willSpawnSummary && needs_embedding && question_enabled
willSpawnWiki     = wiki_enabled && len(textLike) > 0
questionChunks    = type==text only，按 start_at 排序
questionBatchCount = ceil(len(questionChunks) / 20)
graphChunkCount   = len(textLike) if (graph_enabled && extract.enabled) else 0
expected = (summary?1:0) + questionBatchCount + (wiki?1:0) + graphChunkCount
```

`clone_keep=true`：summary/question=0，只计 wiki+graph。行必须是 `processing`，本 handler 调 `SetFinalizing`。`expected==0` → 直接 `completed`。`graph_enabled` 即入队 `chunk:extract`，**无 `NEO4J_ENABLE` 门闩**。Wiki 计 1，编排器不对 wiki shortfall。owned slot 入队失败则 detached `FinalizeSubtask`。入口 abort → 不 fan-out。

| text | ocr | q | embed | wiki | graph | N |
|---:|---:|---|---|---|---|---:|
| 0 | 0 | * | * | * | * | 0 |
| 25 | 0 | T | T | T | T | 29 |
| 25 | 3 | T | T | T | T | 32 |
| 10 | 0 | F | T | T | F | 2 |

### 5.8 summary

对照 `ProcessSummaryGeneration` + `getSummary` + `config/prompt_templates/generate_summary.yaml`。

仅拼 `ChunkTypeText`，上限 `24*1024` 字符。正文 rune &lt; 200 时并入 OCR/Caption。不足文本：`description=""`，`summary_status=failed`，不重试，仍 FinalizeSubtask。成功：`description` + `chunk_type=summary`，向量文本 `title+\n+summary`。不 fail 父 `parse_status`。`attemptSuperseded` 不 FinalizeSubtask。

### 5.9 question

对照 `enqueueQuestionGenerationTasks` / `processQuestionGenerationForChunks` + `generate_questions.yaml`。

每批最多 20 个 **text** chunk；payload 只带 `chunk_ids` + prev/next id。每 chunk 默认 3 题（max 10），写入 `metadata.generated_questions`，每题一条向量（`source_id=chunk_id`）。OCR/Caption 不命题。每批一次 FinalizeSubtask。

### 5.10 graph

对照 `extract.go` + `graph_extraction.yaml`。对每个 text-like chunk 一条 `chunk:extract`。namespace `{product_version_id, document_id}`。节点 upsert：`(version, document, name)`，`chunk_ids` union。边 upsert：`(version, document, node1, node2, rel_type)`。reparse 时 5.5 已 `DelGraph`。不 fail 父 `parse_status`。不移植 `NEO4J_ENABLE`。配置了 `KNOWLEDGEBRAIN_NEO4J_HTTP_URL` 时额外双写 Neo4j `:KbEntity` / `:KB_REL`；失败不 fail 父文档。`/search` `/match` 的 `expand_graph` 读 Postgres（内存无命中时 `graph_hits_pg`）。

### 5.11 wiki

对照 `wiki_ingest.go` / `wiki_ingest_batch.go`。ScopeID = `product_version_id`。

| 常量 | 值 |
|---|---|
| ingest 防抖 | 30s |
| retract 防抖 | 5s |
| finalize 防抖 | 20s |
| 每批文档 | 5 |
| 拼文上限 | 32768 rune |
| stale claim | 90m |
| lock 冲突重试 | 15s |
| slug 锁 | `wiki:slug:{version_id}:{slug}` |
| inflight | `wiki:inflight:{version_id}` |
| tombstone | `wiki:deleted:{version_id}:{document_id}` |
| finalize TaskID | `wiki-finalize-{version_id}` |

两 lane：`wiki:ingest`（ingest / retract）与 `wiki:finalize`（slug / change / folder_prune）。Peek/Claim 不混读。

`FinalizeSubtask` **仅** ingest 终态（成功、skip、fail_count&gt;5 DL）。retract 不减计数。map 失败不打垮整批。`!WikiEnabled` retryable。合成模型：`wiki_config.synthesis_model_id` 否则 `summary_model_id`。

页面类型：`summary/entity/concept/index/log/synthesis/comparison`。published 页写成 `chunk_type=wiki_page` 进向量/tsv。

入口：`EnqueueWikiIngest`、`ProcessWikiIngest`、`mapOneDocument`、`reduceSlugUpdates`、`ProcessWikiFinalize`。

### 5.12 datatable:summary

csv/xlsx/xls 在 ingest/reparse 额外入队 `datatable:summary`（queue `summary`）。对照 `extract.go` 表格摘要：写 `table_summary` chunk 并索引。不计入 `pending_subtasks_count`（与 brain 一致，独立任务）。

---

## 6. 版本克隆

crate：`crates/clone`。只被 `worker` 依赖；`api` 不 import `clone`。

正式数据面是 Postgres（0001 领域表）。正式队列是 oxana，task `version:clone`，队列 key **`low`**。禁止再把内存 `Store` 上的 `run_clone` 当作生产路径。

HTTP `POST /products/{id}/versions` 带 `clone_from` **只**：

1. 在 `product_versions` INSERT 目标行，`status=cloning`，`cloned_from_version_id` 指向源；配置列从源行整行深拷（`chunking_config`、`indexing_strategy`、`image_processing_config`、各 `*_model_id`、各 `*_config`）。
2. enqueue oxana `low`：payload `source_version_id`、`target_version_id`、`diffs[]`、`make_current`、`task_type=version:clone`。
3. 返回该版本行。请求线程不拷文档、不改源版本。

Worker 消费后调用 `clone::run_clone`（sqlx）：

1. 对源版本文档集 apply `diffs`（空 = 全部 keep）：
   - **keep**（同 `file_name+file_size+file_hash`）：新 `document_id`，`content_objects.refcount++`，拷 **document_tags**。0004（chunks / `chunk_embeddings`）落地后：再拷 chunk/向量/tsv/摘要/问题，`parse_status=processing`，enqueue `knowledge:post_process` 且 `clone_keep=true`。0004 未落地：与「源目标 embedding 不同」相同，改为 enqueue `document:process`（队列 `default`）。
   - **add/replace**：目标 INSERT pending 行，enqueue `document:process`。
   - **delete**：目标无行；源不动。
2. 目标 `status=active`。不拷 wiki/graph。不自动改 `current_version_id`（除非 `make_current=true`）。
3. 源目标 `embedding_model_id` 不同则 keep 改 reparse。

---

## 7. 检索

对外两个 mode，底层都是「一组 ProductVersion → 每版本 HybridSearch → 合并 hits」。不另建索引。

```
POST /api/v1/search
{
  "mode": "assembly | matching",
  "query": "...",
  "product_id": "assembly 必填；matching 禁止",
  "version_id": "optional | \"current\"",
  "include_library": false,
  "tag_ids": [],
  "group_by": "none | version | product",
  "match_count": 10,
  "expand_wiki": true,
  "expand_graph": true
}
```

### 7.1 assembly（默认）

产品闭集。问答机器人指定型号、投标组稿锁版本，走这里。

| 请求 | 目标 |
|---|---|
| `product_id` + `version_id` | 该一个版本 |
| `product_id` 无 version | 该 Product 全部 `active` 版本 |
| `product_id`（kind=product）且 `include_library=true` | 上式 ∪ 本 Workspace 内所有 `kind=library` 的 `current_version`（仅 `active`） |
| `product_id` 指向 library | 只搜该资料库 |

- 无 `product_id` → 400。
- `version_id=current`：解析该 Product 的 `current_version_id`；空则 400。
- `include_library=true` 且目标已是 library → 忽略，不重复加。

### 7.2 matching（按招标需求推荐产品 / 公司资料按条款找文档）

输入是**一组需求条款**。`scope` 决定评谁、回什么。不是单条 query 扫一遍 current。

```
POST /api/v1/search   // mode=matching
或 POST /api/v1/match  // 同语义别名

{
  "mode": "matching",
  "scope": "product_lines | company",   // 投标必带；不带则走旧单仓行为
  "requirements": [
    { "id": "r1", "text": "吞吐不低于 40Gbps", "weight": 1.0, "must": true,  "tag_ids": [], "use_library": false },
    { "id": "r2", "text": "具备 ISO9001",       "weight": 1.0, "must": false, "tag_ids": [], "use_library": false }
  ],
  "version_scope": "current | all_active",
  "include_library": false,
  "product_ids": [],
  "expand_wiki": false,
  "expand_graph": false,
  "match_count": 5
}
```

- 带 `scope`：不传、不推断 `workspace_id`。登录或 API key 即可。只走 PG。`expand_wiki` / `expand_graph` 默认 **false**。
- 不带 `scope`：旧行为，必须落到一个 `workspace_id`（兼容；投标不用）。`expand_*` 默认仍 true。带 `product_id` → 400。
- `requirements` 必填，1–30 条。每条 `text` 非空。只传顶层 `query`、无 `requirements` 时，视为一条 `id=q0` 的需求（兼容）。
- 可选 `tender_text`：仅旧单仓路径可用。投标平台自己抽条款，只传 `requirements`。
- `version_scope=current`（默认）：每个目标只用 `current_version`（须 `active`）。无 current 的跳过。
- `version_scope=all_active`：每个产品的全部 `active` 版本都搜；该产品的 `matched_version_id` 取条款综合分最高的版本。

**`scope=product_lines`：** 评全部 `kind=product_line` 下的 `kind=product`，排除 company。`include_library` / `use_library` 必须视为 false。跨线 `embedding_model_id` 不同，或各产品线 `retrieval_config` 的 vector/keyword 阈值不一致 → 400。**禁止静默截断产品数**；一次评完。投标侧条款超过 30 条由 `BidMatchJob` 分批再按产品重算。

**`scope=company`：** 只扫那一条 company Workspace 下全部 `kind=library` 的 current。响应**按条款展平**，不是产品排行榜：

```
{
  "clauses": [
    {
      "id": "r2",
      "outcome": "hit | miss",
      "document_id": "...",          // 仅 hit：分数最高的那份文档
      "version_id": "...",
      "file_name": "ISO9001.pdf",
      "score": 0.81,
      "product_id": "...",           // 分类夹，不当候选
      "hits": [ /* 7.3 Hit */ ],
      "alts": [ { "document_id", "file_name", "score" } ]
    }
  ],
  "warnings": []
}
```

命中必须是公司资料文档，不能是 wiki/graph 节点。同一条款多份材料取最高分写入主字段，其余进 `alts`。

**旧单仓（无 `scope`）：** `product_ids` 非空只评这些产品；空 = 该 Workspace 全部 `kind=product`。硬顶 50（仅旧路径）。条款 `use_library=true` 或请求级 `include_library=true`：该条款额外在本 Workspace 各 library 的 `current_version` 上检索，library hit 记在对应产品的该条款下，不单独成候选。

**打分（每个产品；仅 `scope=product_lines` 与旧单仓）：**

1. 对每条 requirement、每个目标版本做 HybridSearch（规则同 7.3，`match_count` 为每条款每版本条数）。
2. 条款分 = 该产品范围内（含其 library 补充）最高 hit.score；无超过召回阈的 hit → 0，`hit=false`。
3. `score = Σ (weight_i × 条款分_i) / Σ weight_i`
4. `coverage = Σ (hit 的 weight) / Σ weight`
5. `must=true` 且 `hit=false` → 列入 `unmet_must[]`。仍返回该产品，排在所有 `unmet_must` 为空的候选之后。
6. 按 `unmet_must` 空优先，再按 `score` 降序。不输出 `best_product_id` / 「唯一推荐」。投标平台取 top-K 自己定标。

```
{
  "candidates": [
    {
      "product_id": "...",
      "product_title": "...",
      "matched_version_id": "...",
      "matched_version_label": "v3.2",
      "score": 0.81,
      "coverage": 0.67,
      "unmet_must": [],
      "requirements": [
        { "id": "r1", "hit": true,  "score": 0.91, "hits": [ /* 7.3 Hit */ ] },
        { "id": "r2", "hit": true,  "score": 0.70, "hits": [ ... ] }
      ]
    }
  ],
  "warnings": []
}
```

每条 hit 必须带 `product_id` + `version_id`。禁止跨产品、跨版本把证据并成一条。

### 7.3 共同规则

- 召回阈：向量分 &lt; `vector_threshold`（默认 0.15）丢弃；关键词分 &lt; `keyword_threshold`（默认 0.3）丢弃。matching 条款 `hit=true` 当且仅当该条款最高分过对应通道阈值。
- `tag_ids` 非空：每个目标内只留带其中任一 TAG 的 document（OR）。在版本过滤之后做。
- 默认排除 archived / cloning / failed；assembly 显式 `version_id` 可搜 archived。
- assembly 目标版本数超过 20 → 400。
- 并行度 4。单版本失败写入 `warnings[]`；全部失败 502。
- wiki/graph **按版本各自 expand 再合并**。禁止跨版本、跨产品连边。
- 向量：HNSW cosine。关键词：`tsv @@ plainto_tsquery` + `ts_rank_cd`。fusion 按 chunk_id 去重取高分。
- assembly 里产品自己的 current score × 1.15。library 拼入不乘该加成。wiki hit × 1.3。
- 过检：`min(match_count*5*num_targets, 500)`，floor 50。
- 同一请求里所有目标的 `embedding_model_id` 必须一致，否则 400。

Hit：`id, content, score, match_type, chunk_type, document_id, document_title, product_id, product_kind, version_id, version_label, is_current, tag_ids, tag_slugs, start_at, end_at, image_object_key`。
`image_object_key`：仅当 `chunk_type ∈ {image_ocr, image_caption}` 时取 `chunks.context_header`；独立 png 且 header 为空则回退该文档 `object_key`。文本 chunk 不要填。

---

## 7.4 问答门面

机器人 / 会话服务调本库，不在本库实现多轮 Agent。

```
POST /api/v1/answer
{
  "query": "...",
  "product_id": "...",
  "version_id": "optional | \"current\"",
  "include_library": false,
  "tag_ids": []
}
```

1. 内部只调 assembly `/search`（同一鉴权、同一目标解析）。
2. 生成模型取 **该 Product 的 `current_version.summary_model_id`**（请求带了 `version_id` 也仍用 current 的 chat 模型，避免无 current 时多版本歧义；无 current → 400）。无 hits → 不编造，返回空答案 + hits=[]。
3. 引用必须来自本次 hits（`document_id` + `version_id` + 偏移）。禁止跨版本拼事实。
4. 不写会话表、不做工具调用。会话历史由机器人服务持有；若要把上文送进来，用可选 `context[]` 字符串，本库不当成知识。

投标组稿走 assembly `/search`（锁已勾选版本）。按招标条款荐产品走 `scope=product_lines`；公司资料按条款找文档走 `scope=company`。`/answer` 只给已指定产品的问答机器人。

---

## 8. 生命周期

**reparse**：`OpenAttempt` → max+1；清 wiki pending；清 chunks/向量/图；`pending=0`；enqueue `document:process`。

**cancel**：`parse_status=cancelled`；在途任务入口短路；已写数据保留。

**delete document**：`deleting`；删向量 / tsv / chunks / graph / wiki 贡献；`refcount--`。

**delete version**：task type **`kb:delete`**，payload `product_version_id`（alias `knowledge_base_id`）。先删其下全部 document，再软删版本。

批量：`knowledge:list_delete` / `knowledge:list_reparse`（保留字符串）。

Housekeeping：oxana cron 每 5min。`processing`/`finalizing` 超过 `DocumentProcessTimeout+10m` 且无 span heartbeat → `failed`。

错误码（HTTP JSON `{"error":{"code","message"}}`）：`UNAUTHORIZED`、`FORBIDDEN`、`NOT_FOUND`、`CONFLICT`（含 DuplicateFile）、`VALIDATION`、`VERSION_NOT_ACTIVE`、`EMBEDDING_MISMATCH`、`TOO_MANY_TARGETS`、`PARSE_FAILED`。

---

## 9. 可观测

- 文档进度：`document_processing_spans` 五段树。
- 队列：`oxana-web` `/ops/oxana`。
- 指标：Prometheus。
- 无 Langfuse。

---

## 10. 实现顺序

| # | 交付 | 产出 |
|---|---|---|
| 00 | 骨架 | workspace、proto、CI、compose |
| 01 | 领域 | 表 0001（kind、tags、wiki/pending/DL 列、retrieval_config）、状态机；种默认 library |
| 01b | 模型目录 | models、ModelService |
| 02 | oxana 运行时 | 6 Runtime、死信、单次执行测试 |
| 03 | DocReader | 复制 + 原测试（可与 04 并行） |
| 04 | HTTP + JWT/API key | 薄 ingest，禁止同步解析 |
| 05 | convert | 引擎路由 |
| 06 | 图片落库 | Markdown 改写 |
| 07 | chunker | spec §13 对照（可与 05/06 并行） |
| 08 | processChunks | 向量 + tsv，主路径可检索 |
| 09 | post_process N=0 | completed + housekeeping |
| 10 | multimodal | |
| 11–12 | summary / question | |
| 13 | graph | upsert，无 NEO4J 门闩 |
| 14a / 14b | wiki 锁 → 算法 | |
| 15 | version:clone | `clone_keep` |
| 16 | search | assembly + matching（多条款荐产品）+ include_library + tag_ids |
| 16b | answer | 薄 RAG 门面，只调 assembly |
| 17 | reparse / cancel / delete | |
| 18 | spans + oxana-web | |
| 19 | key 管理、SSRF 全量测试 | |
| 20 | Workspace.kind + company + index_ready + 真 VLM + `/match` scope | |
| 21 | LDAP；关 register；files 登录可读 | |
| 22 | `convert_to_markdown`；Bid*；抽取；BidMatchJob；预览 ①～⑤ | |

```
00 → 01 → 01b → 02 → 04 ─┐
         03 ─────────────┼→ 05 → 06 ─┐
                         └→ 07 ──────┼→ 08 → 09 ┬→ 11 / 12
                                     │          ├→ 13
                                     │          └→ 14a → 14b → 15 / 16
                                     └→ 10（可与 09 并行）
```

对照实现：解析/分块/向量/Wiki/图谱与 brain 语义不一致时，以 brain 源文件 + `knowledge-parse-and-chunk-spec.md` 为准；领域名、library/TAG、oxana、Workspace 以本文为准。
