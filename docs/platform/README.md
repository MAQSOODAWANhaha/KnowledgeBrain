# 共享平台与数据库

| 项 | 值 |
| --- | --- |
| 状态 | 领域归属与数据库迁移基线已实施，以代码和测试为准 |
| 服务对象 | 知识库与招投标 |

共享平台只提供两类业务都需要的基础能力，不拥有知识库或招投标业务状态机。招标分析与编制运行在 `services/docreader`，Rust 提供 API 桥接、知识服务和通用对象/队列能力；领域执行状态、租约与模型回执由 Python 工作流维护。

## 平台职责边界

### 拥有

- LDAP/本地登录、JWT 与 API Key 身份认证；业务服务按创建人校验资源归属，API Key 沿用所属用户身份。
- API / worker / retention / docreader 进程拓扑与队列注册；Oxana 负责 transport retry / resurrection / dead queue；招标模型调用租约与回执由 Python 维护，不镜像 Oxana 状态。
- 通用审计事件（`platform_audit_events`）；实际命令的幂等结果归各业务持久化模块。
- `ObjectRegistry`（`platform_objects`）、不可变 deletion identity、Oxana typed retention job 与 owner-fenced 删除回执。
- 维护门（`platform_maintenance_state`）、健康检查、tracing 日志、metrics 与部署验收基础；API/worker 用 `platform::init_tracing()` 初始化 `tracing`（部分生产路径仍有少量 `eprintln!` 待统一）。
- PostgreSQL / Redis / 对象存储的启动与依赖就绪检查。

### 不拥有

- Workspace、Product、Document、chunk、index 与 retrieval 语义；
- TenderProject、分析运行、编制执行、DOCX 稿件版本、初稿/候选、报价快照与导出；
- 任何为调用方方便而复制出的知识库或招投标状态。

### 使用规则

- 业务聚合通过自己的 repository/application service 使用共享能力。
- `ObjectRegistry` 是对象可用性与引用的唯一平台真源；业务表只保存受检引用及业务元数据。
- Open/Stage/Commit 若用于大结果提交，只是具体 adapter 的内部传输协议，不升级为全平台业务接口。
- 配置与本机开发栈端口见 [deploy/README.md](../../deploy/README.md)；招投标工作流与保存见 [docs/tender/README.md](../tender/README.md) 与 [工作台](../tender/workbench.md)。

## 数据库迁移与业务归属

### SQLx 版本迁移

`migrations/` 是业务 DDL 的唯一来源，使用 UTC 时间戳加业务说明按版本递增执行。当前覆盖 57 张业务表（平台 10、知识库 26、招标 21）：

```text
migrations/20261028000001_shared_platform.sql         # 10 张表 + platform_available_objects 视图
migrations/20261028000002_knowledge_base.sql          # 26 张表
migrations/20261028000003_tender.sql                  # 20 张表（含 tender_outline_revisions）
migrations/20261125000001_tender_gap_dispositions.sql # 1 张表（tender_authoring_gap_dispositions）
```

SQLx 的 `_sqlx_migrations`、LangGraph 自维护的 checkpoint 表及 PostgreSQL/pgvector 扩展对象不属于业务表清单。

- `migrator` 二进制（`cargo run -p platform --bin migrator`）与 Compose `migrate` 服务运行 SQLx 标准迁移器，编译时嵌入 SQL，每份 SQL 与台账在同一事务提交并通过数据库锁串行执行。
- 已应用的迁移文件不可修改、重命名或删除，结构变化只能新增时间戳 SQL（`sqlx migrate add <说明>`）；默认前向迁移，不自动生成可能丢数据的 down 脚本。单份失败回滚该文件并保留此前成功版本，修正后继续。
- 发布顺序固定为：数据库就绪 → `migrator` 成功 → Python checkpoint 初始化 → API / worker / retention / dispatcher。运行进程不执行业务 DDL 或在线补表。
- 数据库只保留 PK/FK、唯一约束、简单 CHECK、索引和事务锁；**不定义业务角色、RLS、SECURITY DEFINER 授权入口、自定义业务函数或触发器**。对象引用/删除、知识库 outbox/代次隔离及派生失效全部由 Rust 事务显式维护；Python 租约、幂等、付费回执与 Word 保存并发均在服务层完整保留。

### 业务权限与归属

- 统一规则为**“谁创建、谁可见、谁可操作”**：知识库以 `knowledge_workspaces.owner_user_id` 为归属根，投标项目以 `tender_projects.owner_user_id` 为归属根，子资源继承归属并在业务层校验，禁止跨用户关联或读取。
- API Key 绑定 `owner_user_id`，作为所属用户的认证凭据，无 workspace/product scope 或 read/write 分级，无 Owner/Admin/Contributor/Viewer 成员模型。Bootstrap 管理员凭据只负责全局运维，不绕过资源归属。
- 未绑定业务实体的上传暂存（`platform_object_uploads`）保存可空 `owner_user_id uuid`（用户上传取认证上下文，内部后台产物可为空；空值不可越权操作用户上传）。审计、维护与删除回执不伪造所有者列。

## 数据库命名、状态与约束约定

数据库内部统一 `platform_`、`knowledge_`、`tender_` 三个小写 `snake_case` 前缀，保持 `public` schema，不建兼容视图或双写；外部 HTTP / gRPC / LLM / 队列 / CAS 字段合同通过读写适配稳定保留。

### 业务表分布（57 张）

- **共享平台（10）**：`platform_users`、`platform_api_keys`、`platform_models`、`platform_audit_events`、`platform_maintenance_state`、`platform_objects`、`platform_object_references`、`platform_object_uploads`、`platform_object_deletion_requests`、`platform_object_deletion_receipts`（加视图 `platform_available_objects` 及内容 DOMAIN `kb_sha256`、`kb_object_ref`）。
- **知识库（26）**：`knowledge_workspaces`、`knowledge_products`、`knowledge_product_versions`、`knowledge_documents`、`knowledge_tags`、`knowledge_document_tags`、`knowledge_document_processing_spans`、`knowledge_chunks`、`knowledge_image_artifact_revisions`、`knowledge_image_chunk_links`、`knowledge_chunk_embeddings`、`knowledge_embedding_revisions`、`knowledge_rerank_revisions`、`knowledge_retrieval_policies`、`knowledge_product_version_embedding_bindings`、`knowledge_chunk_keyword_indexes`、`knowledge_product_version_vector_generations`、`knowledge_product_version_keyword_generations`、`knowledge_semantic_index_intents`、`knowledge_chunk_vector_indexes`、`knowledge_graph_nodes`、`knowledge_graph_relations`、`knowledge_wiki_folders`、`knowledge_wiki_pages`、`knowledge_job_outbox`、`knowledge_job_receipts`。
- **招标（21）**：`tender_projects`、`tender_documents`、`tender_document_sources`、`tender_project_analyses`、`tender_analysis_runs`、`tender_analysis_source_versions`、`tender_analysis_revisions`、`tender_analysis_commands`、`tender_analysis_artifacts`、`tender_analysis_confirmations`、`tender_analysis_model_requests`、`tender_authoring_jobs`、`tender_authoring_executions`、`tender_authoring_model_requests`、`tender_outline_revisions`、`tender_draft_versions`、`tender_editor_sessions`、`tender_draft_state`、`tender_draft_saves`、`tender_draft_recoveries`、`tender_authoring_gap_dispositions`。

### 命名与类型规则

- **表与键**：实体表用复数业务名，不含接口版本（`_v2`）或调用端（`_web_`）；普通实体主键为 `id`，稳定执行身份保留 `run_id`/`command_id`/`request_id`/`execution_id`，自然键/复合键/一对一扩展表保留业务键（如 `object_ref`、`revision_sha256`、`(run_id, source_version)`）。外键统一 `<实体>_id`。
- **统一后缀**：字节数用 `byte_length`；原始内容哈希用 `content_sha256`（不同对象哈希保留限定词，如 `request_sha256`、`policy_sha256`）；发生时间用 `_at`，租约截止用 `_until`（`lease_until`/`claim_until`），毫秒时长用 `_ms`，偏移用 `_offset`，身份用 `_id`，对象/结构化引用用 `_ref`。
- **并发与版本词不混用**：`source_version`、`domain_revision`、`epoch`、`attempt`、`generation`、`request_key`（幂等键）与 `request_id`（调用身份）各司其职，不得互换或合并。
- **索引与约束名**：普通索引 `<table>_<purpose>_idx`，唯一索引 `<table>_<purpose>_uidx`，组合外键 `<table>_<relation>_fk`；所有 PostgreSQL 标识符不得超过 63 字节。组合 FK 引入的唯一键已覆盖左前缀时不重复建普通索引。

### 业务状态枚举（应用层负责）

- 数据库状态列采用 `text`，不新增 PostgreSQL ENUM 或复杂状态 CHECK；状态合法性由应用层枚举负责（Python 按实体定义 `StrEnum`，Rust 按领域定义 enum），跨语言共享状态保持相同 wire 值。
- 不同生命周期的实体（分析运行、调度命令、模型请求、编制执行、项目、稿件保存）分别定义枚举，不因同含 `pending` 合并。
- 数据库读入或接口接收的未知状态必须显式返回解码/参数错误（fail-closed），绝不默认成 `pending`、`done` 或成功。

### 关联约束边界

- 能成立的同项目/同版本/同对象跨表关系使用组合唯一键与组合 FK（如 `(document_id, project_id)`、`(run_id, project_id)`、`(id, project_id)`、`platform_object_deletion_receipts` 四列组合 FK、`(id, product_id)`、`(document_id, product_version_id)`、`(folder_id, product_version_id)` 等；循环插入使用延迟 FK）。
- 以下关系刻意保留在服务层管理，不机械补 FK：
  - `tender_analysis_artifacts.domain_revision = 0`（跨修订复用的来源产物）；
  - `tender_authoring_jobs.execution_id`（在执行行创建前入队预分配）；
  - `platform_object_references(owner_kind, owner_id)`（跨领域多态引用）；
  - `platform_object_uploads.owner_user_id` 与 CAS 对象的多用户共享语义；
  - CAS/JSON 内部引用与图谱节点名称/Wiki 来源 slug 关系。
