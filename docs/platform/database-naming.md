# 数据库命名与关联约定

本文为已确认的全新初始化命名实施约定。改造前盘点为 56 张业务表，按原 baseline 分为平台 7 张、知识库 30 张、招标 19 张。删除未使用的 Wiki 日志表，并把用户、API Key、模型注册移入平台后，本轮统一为 55 张：平台 10 张、知识库 26 张、招标 19 张。

并行的大纲编辑任务另增 `tender_outline_revisions`，保留其独立设计，本轮不改其字段和关联；合并后的初始迁移因此为 56 张业务表（10/26/20）。SQLx 的 `_sqlx_migrations`、LangGraph 自行维护的 checkpoint 表及扩展对象不属于本业务表清单，使用各工具的标准名称。本文统一数据库内部名称，不更改 HTTP、gRPC、LLM、队列或已有 CAS 产物的字段合同；执行进度与验证结果记录在交接文档。迁移版本管理见[数据库初始化与业务归属](database-initialization.md)。

## 采用的约定与成本

采用 `platform_`、`knowledge_`、`tender_` 三个业务域前缀，使用小写 snake_case。保持 `public` schema，不增加数据库 schema、账号或权限层。用户、API Key、模型注册属于共享平台，移入平台 baseline；目标为 55 张表，按职责分为 10/26/19。

全域前缀需要同步较多 SQL，但这次明确只支持全新初始化，改一次比长期保留 `bid_`、裸表名、`knowledge_*_v2`、`tender_web_` 多套名称更直接。避免额外重构：不增加兼容视图、不双写、不为改名拆合业务表、不统一不同生命周期的状态枚举。对外字段可用 `SELECT new_column AS existing_field` 或现有读写适配器保留。

- 实体表用复数业务名。普通实体主键用 `id`；既有运行/命令/请求的稳定执行身份保留 `run_id`/`command_id`/`request_id`/`execution_id`，不为改名改动所有回执适配。已有 UUID/text 类型不转换。外键用 `<实体>_id`，例如 `project_id`、`run_id`、`execution_id`、`session_id`。
- 自然键、复合键和一对一扩展表保留业务键作主键：例如 `object_ref`、`revision_sha256`、`(run_id, source_version)`、以 `document_id` 为主键的来源元数据。不要为统一 `id` 再造一层身份。
- 表名不包含接口版本或调用端：移除 `_v2` 和 `_web_`。`schema_version`、协议版本、模型行为修订等有实际语义的列及取值保留。
- 字节数统一 `byte_length`；原始内容的 SHA-256 统一 `content_sha256`。`request_sha256`、`policy_sha256`、`artifact_sha256` 等不同内容的哈希保留对象限定词，不能混成同一个字段。
- `_at` 用于发生时间，`_until` 用于租约等有效截止时间，保留现有 `lease_until`/`claim_until`；`_ms` 用于毫秒时长；位置使用 `_offset`。`_id` 只用于身份，`_ref` 用于对象路径或结构化内容引用。`original_ref`/`artifact_ref` JSON 和 `object_ref` 字符串不合并。
- 工作流进度使用 `status`，对象可用性、发布/支持状态可保留现有 `state`/`support_state`。多个独立状态保留限定词；不机械统一列名或合法状态值。
- 资源归属使用 `owner_user_id uuid`，API Key 继承其所属用户。子资源通过项目/工作区继承归属，不重复保存 owner；稿件版本已有的 `created_by_user_id` 保留具体版本创建来源。删除操作者文本字段与 `user:<uuid>`/`api_key:<uuid>` 字符串身份。
- 普通索引 `<table>_<purpose>_idx`，唯一索引 `<table>_<purpose>_uidx`，组合外键 `<table>_<relation>_fk`；PostgreSQL 标识符不得超过 63 字节。长表名索引使用明确目的词，避免静默截断导致重名。

## 业务状态定义

状态合法性主要由业务代码负责，不新增 PostgreSQL ENUM 或状态 CHECK。现有主外键、唯一约束及事务一致性保持原职责。表字段采用 text 不等于应用可以任意传字符串。

- Python 持久化工作流状态采用按实体命名的 `StrEnum`；Rust 使用按领域的 enum，读取、业务分支及写入参数引用该类型。现有有效字符串和对外 JSON 保持不变。
- 分析运行、调度命令、模型请求、编制执行、项目和稿件保存分别定义，不能因同样包含 `pending` 就合并生命周期。实体共用完全相同的模型调用回执合同才复用同一枚举。
- 已有 Pydantic `Literal` 对模型输出有限取值的校验继续保留；本轮聚焦宿主状态管理，不为统一形式改动 LLM 输入输出合同。
- 数据库读入或接口接收的未知状态返回明确的状态解码/参数错误，不默认成 `pending`、`done` 或成功。错误继续由现有任务重试、接口错误及运行诊断处理，不引入通用状态机框架或新的业务门禁。
- SQL 中固定的状态条件不是另一份类型定义。优先让服务读写参数与业务判断使用枚举；保留必要的静态 SQL 条件、部分索引及已有事务转换逻辑，不增加 SQL 生成器。
- 测试覆盖每个实际状态的序列化/反序列化、未知值拒绝，以及删除/失败/恢复等关键分支。两种语言共享的项目/回执状态须保持相同 wire 值，不能各自发明别名。

此次类型收敛不改变工作流节点、合法转换、重试上限、调用预算或模型协议；不新增模型调用。

## 表名完整映射

下表未单独标明的列保持现有名称；字段变更以后一节为准。

### 共享平台（10 张）

| 当前表 | 目标表 | 职责 |
| --- | --- | --- |
| `users` | `platform_users` | 登录用户，业务归属根 |
| `api_keys` | `platform_api_keys` | 绑定用户的访问凭据 |
| `models` | `platform_models` | 共享模型注册配置 |
| `audit_events` | `platform_audit_events` | 操作审计 |
| `application_maintenance_gate` | `platform_maintenance_state` | 单例维护状态 |
| `object_registry` | `platform_objects` | 内容对象身份与生命周期 |
| `object_owner_references` | `platform_object_references` | 业务对象引用 |
| `object_upload_staging` | `platform_object_uploads` | 上传暂存与到期回收 |
| `object_deletion_artifacts` | `platform_object_deletion_requests` | 删除请求及其不可变对象身份 |
| `object_retention_tombstones` | `platform_object_deletion_receipts` | 已删除回执 |

`available_object_registry` 视图改为 `platform_available_objects`。`kb_sha256`、`kb_object_ref` 保留内容身份约束；删除 `kb_actor_identity` DOMAIN及所有操作者字段。上传暂存尚未绑定业务实体，使用可空的 `owner_user_id uuid REFERENCES platform_users(id)`；用户上传从认证上下文取值，内部后台产物可为空。空值不是越权通行证，发布/主动清理仍要求与记录归属一致。数据库只维持用户关联，授权由业务层决定。

### 知识库（26 张）

| 当前表 | 目标表 |
| --- | --- |
| `workspaces` | `knowledge_workspaces` |
| `products` | `knowledge_products` |
| `product_versions` | `knowledge_product_versions` |
| `documents` | `knowledge_documents` |
| `tags` | `knowledge_tags` |
| `document_tags` | `knowledge_document_tags` |
| `document_processing_spans` | `knowledge_document_processing_spans` |
| `chunks` | `knowledge_chunks` |
| `knowledge_image_artifact_revisions` | `knowledge_image_artifact_revisions` |
| `knowledge_image_ocr_chunk_artifact_mappings` | `knowledge_image_chunk_links` |
| `chunk_embeddings` | `knowledge_chunk_embeddings` |
| `embedding_revisions_v2` | `knowledge_embedding_revisions` |
| `rerank_revisions_v2` | `knowledge_rerank_revisions` |
| `knowledge_retrieval_policies_v2` | `knowledge_retrieval_policies` |
| `product_version_embedding_bindings_v2` | `knowledge_product_version_embedding_bindings` |
| `chunk_keyword_indexes_v2` | `knowledge_chunk_keyword_indexes` |
| `product_version_vector_index_generations_v2` | `knowledge_product_version_vector_generations` |
| `product_version_keyword_index_generations_v2` | `knowledge_product_version_keyword_generations` |
| `knowledge_semantic_index_intents_v2` | `knowledge_semantic_index_intents` |
| `chunk_vector_indexes_v2` | `knowledge_chunk_vector_indexes` |
| `graph_nodes` | `knowledge_graph_nodes` |
| `graph_relations` | `knowledge_graph_relations` |
| `wiki_pages` | `knowledge_wiki_pages` |
| `wiki_folders` | `knowledge_wiki_folders` |
| `wiki_log_entries` | 删除 | 无当前业务读写，停止为未使用日志保留表 |
| `knowledge_job_outbox` | `knowledge_job_outbox` |
| `knowledge_job_receipts` | `knowledge_job_receipts` |

`knowledge_chunk_embeddings` 与 `knowledge_chunk_vector_indexes` 当前均有实际调用，改名不等于合并索引实现；模型修订和 tokenizer 的版本列继续保留。

`knowledge_wiki_pages.parent_slug` 同为未使用字段，本次删除；不新增父 slug 关系表或替代字段。

### 招标（19 张）

| 当前表 | 目标表 | 职责 |
| --- | --- | --- |
| `bid_projects` | `tender_projects` | 项目与用户归属 |
| `bid_documents` | `tender_documents` | 上传文件与原件身份 |
| `tender_runs` | `tender_analysis_runs` | 分析执行与租约 |
| `tender_versions` | `tender_analysis_source_versions` | 原文件集合版本 |
| `tender_revisions` | `tender_analysis_revisions` | 同源分析修订 |
| `tender_commands` | `tender_analysis_commands` | 分析调度命令 |
| `tender_artifacts` | `tender_analysis_artifacts` | 分析产物引用 |
| `tender_confirmations` | `tender_analysis_confirmations` | 人工确认回执 |
| `tender_model_requests` | `tender_analysis_model_requests` | 分析模型调用回执 |
| `tender_authoring_executions` | `tender_authoring_executions` | 编制执行与租约 |
| `tender_authoring_model_requests` | `tender_authoring_model_requests` | 编制模型调用回执 |
| `tender_web_documents` | `tender_document_sources` | 项目文件的分析原件引用 |
| `tender_web_runs` | `tender_project_analyses` | 项目关联分析及原件快照运行 |
| `tender_web_authoring_jobs` | `tender_authoring_jobs` | 编制入队与投递状态 |
| `tender_web_docx_versions` | `tender_draft_versions` | 生成、编辑及恢复的 Word 稿件版本 |
| `tender_web_docx_sessions` | `tender_editor_sessions` | 编辑会话 |
| `tender_web_docx_current` | `tender_draft_state` | 项目当前稿件、会话与待保存指针 |
| `tender_web_docx_saves` | `tender_draft_saves` | 保存请求及结果回执 |
| `tender_web_docx_recoveries` | `tender_draft_recoveries` | 会话的异常回调恢复结果 |

“分析运行”与“编制执行”是已有业务职责，不合并两套租约/模型回执；`tender_draft_*` 仍保存当前 Word 格式，名称调整不扩展支持格式。

## 必要字段映射

此表使用目标表名。对外旧字段通过适配保留；不重写存储于 JSON/CAS 内的同名键。

| 表 | 旧字段 → 新字段 | 说明 |
| --- | --- | --- |
| `platform_objects`、`platform_object_deletion_requests`、`platform_object_deletion_receipts` | `digest` → `content_sha256` | 已有 64 位 SHA-256 约束，确认为对象字节摘要 |
| `knowledge_documents` | `file_size` → `byte_length`；`file_hash` → `content_sha256`；`type` → `document_kind` | 文件名继续为 `file_name` |
| `tender_draft_versions` | `docx_sha256` → `content_sha256`；`parent_id` → `parent_version_id`；`created_by` → `created_by_user_id` | 内容摘要与父稿件身份 |
| `platform_object_uploads` | 操作者字段改为 `owner_user_id uuid` | 未绑定实体的上传归属；业务代码比较 UUID，Key 使用所属用户 |
| `platform_object_references`、`tender_documents` | 删除操作者字段 | 引用通过业务实体、文件通过项目继承归属，不再复制用户字段 |
| `platform_audit_events`、`platform_maintenance_state`、`platform_object_deletion_receipts` | 删除操作者字段 | 不把更新/删除者冒充所有者，保留事件、时间、删除回执与状态 |
| `tender_analysis_revisions` | `corrected_requirements_ref` → `corrected_items_ref` | 当前业务已统一为事项，去掉历史 SQL 列别名转换 |
| `tender_draft_state`、`tender_draft_saves`、`tender_draft_recoveries` | `editor_key` → `session_id` | FK 指向会话；OnlyOffice 协议中的 key 保持不变 |
| `knowledge_chunks` | `start_at` → `start_offset`；`end_at` → `end_offset` | 非时间字段。此次不改变现有偏移计算或单位 |
| `knowledge_graph_relations` | `node1` → `source_node_name`；`node2` → `target_node_name`；`rel_type` → `relation_type` | 现存值是节点名称，不冒充 node_id |
| `knowledge_wiki_folders` | `parent_id` → `parent_folder_id` | 避免父身份类型不明确 |

`source_version`、`domain_revision`、`epoch`、`attempt`、`generation` 各有独立并发或版本含义，不为一致性合并。`request_key` 是幂等标识，`request_id` 是调用身份；二者不能互换。`upstream_digest`/`input_digest` 等字段允许空值标识或宿主逻辑签名，暂不声称全部都是 SHA-256，也不改 JSON 内的 `request_digest`。

## 关联约束：只补实际可成立的关系

数据库 FK 负责记录间一致性，业务服务仍负责用户归属校验；以下不是权限角色或新的业务审批。

| 现有关系缺口 | 建议 |
| --- | --- |
| 文件来源表独立引用 document/project，可把两者拼错 | `tender_documents` 增加 `UNIQUE(id, project_id)`；`tender_document_sources(document_id, project_id)` 使用组合 FK |
| 编制任务的 project/run 可独立拼错 | `tender_project_analyses` 增加 `UNIQUE(run_id, project_id)`；`tender_authoring_jobs(run_id, project_id)` 组合引用该映射 |
| Word 版本、会话、当前选择、保存结果各自合法但可跨项目 | 给稿件版本和会话增加 `UNIQUE(id, project_id)`，利用各表已有 `project_id` 为 parent/base/current/session/result 关联建立组合 FK；不增加全局权限函数 |
| 当前稿件 `pending_save_id` 没有 FK | 保存表增加 `UNIQUE(id, project_id)`，在两表创建完后加组合 FK；若测试或事务先写指针再写回执，则调整同事务顺序或使用 `DEFERRABLE INITIALLY DEFERRED`，不要求多次事务 |
| 对象删除回执与请求的对象身份可能拼错 | `platform_object_deletion_receipts(deletion_id, object_ref, content_sha256, byte_length)` 组合引用删除请求的现有四列唯一键，不新增字段或索引 |
| 产品 `current_version_id`、版本 `cloned_from_version_id` 无 FK | 本次已确认克隆限定同一产品：版本增加 `UNIQUE(id, product_id)`；current 与 cloned_from 分别组合引用所属产品版本。产品/版本循环插入使用延迟 FK，不拆开原子创建事务 |
| chunks、graph、outbox 分别关联 document/version，可能指向不同源 | 已有 documents 的 `UNIQUE(id, product_version_id)`；优先将这几处改成 `(document_id, product_version_id)` 组合 FK |
| embedding 行含 chunk/version/document，但只校验 chunk | 使用已有 chunks 三列唯一键建组合 FK，保留原删除级联 |
| wiki page.folder / folder.parent 未建立关联 | 使用 `(folder_id, product_version_id)`、`(parent_folder_id, product_version_id)` 组合 FK，先核实目录同步写入顺序；不要求旧目录先于同事务其他行存在 |

以下保留服务层管理，不机械补 FK：

- `tender_analysis_artifacts.domain_revision = 0` 用于来源产物跨修订复用；不能强行 FK 到不存在的第 0 修订。继续引用 `(run_id, source_version)`，非来源修订由现有宿主围栏保证。
- `tender_authoring_jobs.execution_id` 在编制执行行创建前即被分配并入队。它是未来执行的关联标识，不能直接加指向 `tender_authoring_executions(execution_id)` 的即时 FK；不因改名改变调度顺序。
- `platform_object_references(owner_kind, owner_id)` 是跨领域引用，不能给 `owner_id` 加只指向某一业务表的 FK，也不应改名为 `owner_user_id`。
- `platform_object_uploads.owner_user_id` 引用用户表，当前用户和API Key都取认证结果中的用户 UUID；上传归属比较保留在业务层，不比较Key ID。平台内容对象可由多个用户的实体共享，不为CAS对象加单一用户owner。
- CAS/JSON 中的分析修订、原件、引用范围和模型回执身份仍由既有适配器验证。不要为补 FK 复制每个 JSON 字段成冗余关系列。
- graph 的节点名称、wiki 的来源 slug、编辑恢复跨表完整关系，若建立约束需要新增冗余域列或改变合法写入顺序，先保留现有业务校验；本轮不扩成全面图关系重构。

## 索引清单

PostgreSQL 不自动为引用端 FK 建索引，但也不能为每个 FK 无条件加索引。复合主键/唯一索引左前缀已经覆盖的查询不重复建。保留原有队列状态、调用摘要、向量、全文及当前稿件索引，改名时同步字段。

当前需要补齐或确认的实际路径：

| 表与列 | 用途与现状 |
| --- | --- |
| `tender_projects(owner_user_id, created_at DESC)` | 已加，项目列表 |
| `tender_documents(project_id, created_at)` | 已加，项目文件列表 |
| `tender_project_analyses(project_id, created_at)` | 已加，最近分析运行 |
| `tender_authoring_jobs(project_id, created_at)` | 编制任务列表与最近任务；现有 PK/dispatch 索引不覆盖 |
| `tender_analysis_commands(run_id, source_version, domain_revision, created_at)` | 运行详情/恢复命令读取；dispatch(status,created_at) 不覆盖 |
| `tender_analysis_model_requests(run_id, created_at)` | 运行调用明细，原摘要索引可过滤 run 但不覆盖该排序；是否新增以 EXPLAIN 确认 |
| `platform_object_uploads(object_ref)` | 检查有效上传对对象删除的保护；现有 expiry 索引不覆盖 |
| `knowledge_documents(product_version_id)` | 现有 live-file 部分唯一索引仅覆盖未删除文件；清理/克隆按版本读全部时需完整前缀索引 |
| `knowledge_document_tags(tag_id)` | 删除标签时 FK 检查及反向筛选；PK(document_id,tag_id) 不覆盖 |
| `knowledge_graph_nodes(document_id)`、`knowledge_graph_relations(document_id)` | 按文档失效及清理，原 PK 以 product_version_id 开头 |
| `knowledge_wiki_folders(product_version_id)` | 目录按版本查询 |

组合 FK 为目标增加的唯一键有一致性用途；不能再为相同左前缀重复加普通索引。列顺序采用真实查询过滤顺序；最终索引名须通过 63 字节与重名检查。

## 实施顺序和验收

1. 先冻结本文 55 张目标表和必要字段映射，按平台、知识、招标三个职责分工；只改三份 baseline 和对应 SQL/行读取适配、fixture。用户/Key/model 建表迁至平台 baseline 前部，保证依赖顺序。
2. 独立提交式步骤完成名称替换，再增加上表能成立的组合 FK/必要索引；数据库名与外部 DTO 的适配显式保留。初始化检查、探针、部署脚本、测试基线均指向新表。
3. 在全新隔离库核对精确表集合、列类型、PK/FK/索引，测试同项目关联成功、跨项目拼接失败，以及循环关联同事务创建成功。
4. 重跑实际数据库回归：两用户/Key 隔离、克隆与列表防越权、对象上传/删除并发、知识 outbox/代次、Python 租约/重试/付费回执、Word 保存/恢复。确认 HTTP/gRPC、队列身份、LLM 输入输出与 CAS 引用不变。
5. 扫描运行源码及 CI 中旧表名；历史设计记录不批量替换，以免篡改旧运行说明。记录哪些验证真实执行、哪些仅编译；不连接或修改现有业务数据库。

此次命名与关联调整不改变 LLM 调用数、请求预算、分包或语义职责，不需要付费模型重跑。若关联校验失败，事务回滚并进入已有任务重试/修复出口，不清除旧回执或内容产物。
