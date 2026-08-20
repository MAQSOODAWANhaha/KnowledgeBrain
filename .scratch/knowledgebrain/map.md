# KnowledgeBrain

规格：`docs/system-design.md`  
副本：`.scratch/knowledgebrain/spec.md`（必须与正文逐字一致，改规格先改正文再拷副本）

对照：`/opt/workspace/code/brain`。HTTP 只入队；解析之后全在 worker。

## Status 规则（必须）

工单文件是进度真源，和代码必须同时改：

| Status | 含义 |
|---|---|
| `done` | 本票勾选的 `[x]` 在仓库里都为真 |
| `partial` | 有实现，但仍有 `[ ]` |
| `ready-for-agent` | 下一张可开工的实现票 |
| `blocked` | 前置未完成 |

禁止：代码没做完把 Status 写成 `done`；勾了 `[x]` 但仓库没有对应行为；只改代码不改工单 / 只改工单不改代码。  
Review 票只能把「已核对为真」的项打钩；偏差未修回就记在 Comments，对应项保持 `[ ]`。  
`map.md` Frontier 只指向当前该做的下一张实现票。

**门禁（必须）：** 每张实现票做完、每张 review 票标 `done` 前，按 `.scratch/knowledgebrain/review.md` 跑通 **Rust + Python + 本票触及的其它栈**（fmt / lint / test；compose / SQL / proto / 脚本）。禁止只跑 `cargo test`，禁止 `#[allow(dead_code)]` 糊警告。CI 与该文件命令一致。

## Wave-1 review（2026-08-15，已收口）

实现票 **01–34 均为 `done`**。下面那段「02/04–22/30 未完成」「09 不消费队列」是收口前的快照，**不再代表仓库**。

当时当场修掉、现已为真：09 进程消费 oxana；11 硬顶切开；25 深拷版本配置 + keep 拷 0004；27 graph 阈值；30 `/answer` 锁 current。

## Frontier

**下一张实现票：无。** 01–34 勾选项在仓库里为真。

票后收口（不是新票号）：

- clone keep 同 embedding → 拷 chunk/向量 + `clone_keep` post_process；不同模型仍 reparse
- `/search` `/match` 内存无命中走 PG；hydrate + `merge_catalog` 带 tags/graph/wiki/chunks
- oxana `postprocess` 消费 `knowledge:post_process`（clone_keep 会真正跑）
- worker convert 写 chunk/embedding 并 `enable_status=enabled`；`GET /documents/{id}` 会按文档 hydrate 工作区
- worker `post_process` 会跑 summary/question 并回写 PG chunk
- `GET /api/v1/ops/queues` 看内存队列、PG pending、死信数量（含 PG `task_dead_letters`）
- VLM：配置了 `KNOWLEDGEBRAIN_VLM_BASE_URL` / `CHAT_BASE_URL` 走 HTTP OCR/caption，否则 stub
- multimodal worker hydrate 后 `persist_index` 写 OCR/caption chunk，并入队 `post_process`
- ingest / `create_tag` / `put_tags` 写 `tags` + `document_tags`；`fail_now` 写 `task_dead_letters`；`GET /ops/dead-letters` 合并 PG
- embedding：列宽 `vector(1024)`（`0008`）；`index::embed` 走 HTTP `/v1/embeddings`，未配置回 hashed stub
- 图谱：PG 为真源；`KNOWLEDGEBRAIN_NEO4J_HTTP_URL` 双写 Neo4j；`expand_graph` 走 `graph_hits_pg`
- oxana convert：`enable_multimodel && 有图` 入队 `image:multimodal`，否则 `post_process`
- PG wiki ingest 写 `wiki_pages` + `wiki_page` chunk
- cancel/reparse/delete 写 PG 并入队 oxana `low`；6 Runtime 并行消费
- `/answer` hydrate + current `summary_model_id`；`/models` `/ops/oxana` `/metrics` `/wiki/folders` `/files`
- 对象：本地 `var/objects` 必写；配了 `KNOWLEDGEBRAIN_S3_BUCKET` + `KNOWLEDGEBRAIN_S3_ENDPOINT` 再双写 MinIO/S3（compose `:19000` 已测通）
- matching：`tender_text` 抽条款；`group_by=none|version|product`
- 检索：过检 `min(match_count*5*N,500)` floor 50；PG assembly 每批 4 并行；全目标失败 502；无 `version_id` 搜全部 active；阈读 `retrieval_config`
- DocReader：仅 gRPC；删未用 Python splitter；Rust 客户端 TLS + `GRPC_AUTH_TOKEN`
- convert：版本 `parser_engine`；MinerU/Paddle HTTP；`url:` blob → WebParser；`passages` 不走 convert/Split；图片改写 `objects/{sha}` + SSRF；`GET .../files` 仅本版本引用的 key
- 任务进度：brain 同款五段 DAG（`docreader → chunking → embedding∥multimodal → join postprocess`）。失败级联 `cancelled`；无图则 multimodal=`skipped` 才放行 postprocess。`GET /documents/{id}/timeline` 回 `current_stage` + 五段树（缺行补 pending）
- `index:delete` 入 `low`；删文档 / reparse 会入队（purge 仍同步执行）
- 并发：`KNOWLEDGEBRAIN_{CORE,POSTPROCESS,ENRICHMENT,MAINTENANCE,SHARED,WIKI}_CONCURRENCY`，队列 `Dynamic` 默认与规格表一致
- 父子 chunk：`chunking_config.enable_parent_child` → 父 4096 / 子 384；PG convert 走 `index::index_chunks`（尊重 vector/keyword）
- post_process 扇出 oxana `summary`/`question`/`extract`（无 Redis 才 inline）；wiki map 合成 + reduce 回写 `wiki_page`
- 部署：`deploy/`（compose + `.env.example` + Dockerfile + README）；根 `docker-compose.yml` 转发；`connect()` 会 apply `0001`–`0008`
- 队列看板：`oxana-web` 嵌在 `/api/v1/ops/oxana/web`（admin）；`GET /ops/oxana` 回队列深度 + 在途任务；Redis 不可达则不下挂看板
- `process_overrides`：上传 / URL / passage / reparse 可带 `process_config`；合并规则对齐 brain（0/空保留 base、`EnableParentChild` 整值覆盖、`ParserEngineRules` 非空整表替换、`GraphEnabled AND extract.enabled`）；convert / chunk / post_process 走 `effective_version`
- 文档列表：`GET .../documents?parse_status=&tag=&keyword=`
- 级联删除：删 Workspace / Product 先 cancel 在途文档，再对全部版本入队 oxana `kb:delete`；PATCH 工作区/产品/成员/版本写 PG
- `DELETE /workspaces/{id}/tags/{tag_id}` 只删 tag + `document_tags`，不删文档
- `datatable:summary` 对照 brain `extract.go`：csv 用 simple 文本抽样；xlsx/xls **不在 Rust 解析**，只读 DocReader convert 写出的 Markdown（`col: val` 行或表）。convert 未完成则重试。写 `table_summary` + `table_column` 并索引
- question payload 带 `prev_ids`/`next_ids`；生成时拼前后块上下文
- Wiki 合成：map 出 summary + entity/concept（LLM JSON，失败回落图谱节点）；cite `source_refs`；同 slug 多文档 union refs。合成模型 `wiki_config.synthesis_model_id` 否则 `summary_model_id`。finalize 写 index/log/synthesis/comparison，入队 `folder_prune`（ingest 未排空则等）。taxonomy 选文件夹对齐 brain：≤60 全量，否则 L1 + 向量 top-3
- `PATCH /products/{id}/versions/{version_id}` 可改 chunking / indexing / 模型 / 多模态 / ASR / wiki/graph/extract；GET 版本回这些字段
- payload 认 `knowledge_id` / `knowledge_base_id`，丢弃 `tenant_id`；ingest 调 `OpenAttempt`；`PATCH /me` 写 PG；删 Workspace 入队 `kb:delete` 后 retire（清成员、改 slug）
- `question_generation_config`：默认 3 / max 10；hydrate + PATCH + clone + `process_overrides`；出题用 `version.question_count()`，`custom_instructions` 按 brain 标签追加
- wiki oxana 防抖：ingest `enqueue_in` 30s、finalize 20s、批后 follow-up 5s；`unique_id` Skip 合并；`WikiIngestWorker` 锁冲突重试 15s
- chunker 父子配置对齐 brain：父 4096 / overlap=版本 overlap，子 384 / overlap=child_size/5；`parent_chunk_size` `child_chunk_size` `separators` `token_limit` `languages` 走 `chunking_config`
- processChunks：空 content 跳过；无 text 且无多模态直接 completed；embedding HTTP 失败 retryable，最后一次 fail 文档；请求带版本 `embedding_model_id`；写库前后查 abort
- image:multimodal：PG 入队前 SET `multimodal:pending`；VLM 失败中间不 DECR、最后一次仍计数；父块挂含图的 text chunk；单图死信不 fail 父文档
- post_process：PG `SetFinalizing` 先于入队；owned slot 入队失败 `FinalizeSubtask`；summary 扇出写 `pending`；post_process 死信 fail 父文档
- summary：`start_at` 拼接；实文不足（去图标记后 <8）failed 仍 drain；superseded 不 FinalizeSubtask；chat 失败可重试，最后一次回落首块 500 rune
- question：payload `prev_ids`/`next_ids` 进 prompt `<surrounding_context>`；空/非 text 跳过；chat HTTP 失败跳过该块；superseded 不 FinalizeSubtask；PG `QuestionWorker` 传 neighbors，最后一次失败仍 drain
- graph：`ExtractOutcome::Superseded` 不 drain；抽图 `effective_version`；persist 增量 UNION `chunk_ids`（并行 extract 不互盖）；`ExtractWorker` 最后一次失败仍 drain
- datatable：brain `extract.go` 表/列 prompt；`table_metadata_instructions` 走 chunking_config / PATCH / hydrate / `process_overrides`（标签 `table_metadata`）；xlsx/xls 只读 DocReader Markdown；重试先删旧表块；PG 只 append 表块不 replace 全文；ingest+reparse 入队；不计入 pending；chat HTTP 失败可重试
- 检索：查询向量 `embed_index` + 版本 `embedding_model_id`；PG assembly 校验 embedding 一致 / current 空 400；graph 分不再写死 0.2；chunk_id 融合取高分；PG 关键词用 `ts_rank_cd`；Hit 带 `tag_slugs`；matching 硬顶 50 产品
- 生命周期：HTTP reparse 只排队 `list_reparse`；HTTP delete 只写 `deleting`；PG `refcount--` 到 0 删 blob；housekeep 只扫 processing/finalizing；reparse 校验版本 active
- 残留收口：`index_one` 走 `embed_index` + 版本模型（HTTP 失败不再回 stub）；wiki 入队 `Err` 仍 `FinalizeSubtask`（owned slot），`Ok(None)` 无 Redis 则 inline `process_wiki_ingest`。编排器仍不对 wiki **shortfall**（与规格 / brain TODO 一致）
- 写路径 hydrate + 去重：ingest/reparse/delete/clone/`current` 先 `ensure_product`/`ensure_document`；`version_id=current` 且无 current → 400；写仅 `active`；PG 有版本才 `insert_document`，唯一约束 409；图要 VLM、音频要 ASR；超限读 `MAX_FILE_SIZE_MB`
- convert：引擎读版本 `chunking_config.parser_engine_rules`（覆盖 `process_overrides`）；内存 convert 传配置引擎不再 `""`；DocReader/ASR 未配置立刻 fail；其它 convert/ASR 错误可重试，最后一次 `fail_now`
- multimodal：`delete_image_chunks` + `insert_document_chunks` 追加，不再 `replace_document_chunks` 盖并发图块；DECR 在 persist 成功之后；`scanned_pdf` 仅当 PDF 实文 < `IMAGE_DOMINATED_RUNES`；passage 索引用版本 vector/keyword；空向量 pad 到 1024
- `/manual` 走 `manual:process`（跳过 convert、走 Split）；`/passage` 仍跳过 convert+Split；`/answer` 不默认 `current`，目标解析与 assembly 相同（省略 `version_id` = 全部 active）；reparse PG `OpenAttempt`；拉图禁止自动跳转并复检 SSRF；matching PG 过检 `per_target_limit`
- summary/question 增量 persist（`persist_summary_chunks` / `persist_question_updates`），不再 `replace_document_chunks`；`documents.type` + `source_passages`；reparse 按 file/passage/manual 入队；软删后部分唯一索引可再传同一文件；hydrate 读 `attempt`/`description`/`summary_status`；multimodal 用 `job.attempt`，过期 attempt 不写不 DECR；`merge_catalog` PG 覆盖；删产品只入队 `kb:delete`，空产品先删已归档版本；退役 workspace 吊销 API key；image 入队失败 DECR，inline post_process 调 `finalize_subtask`
- `DefaultBodyLimit` 读 `max_file_bytes()`；HTTP reparse 只改 pending + 入队 `list_reparse`；`set_current` 要求 active；PG convert `parser_engine_rules` 整表替换；`/search` current 空 400；PG 检索错误不再吞成 200；`embedding_top_k` 限制向量召回；matching 把 library 纳入 embedding 校验；graph expand 过 `tag_ids` 且用 workspace 关键词阈；keep-clone 拷 `description`/`summary_status`；PATCH workspace/member/tag/retrieval 先 hydrate
- PG hybrid 尊重版本 `vector`/`keyword` 开关；convert 可重试错误先 fail span 并 cascade，最后一次才 `fail_now`；housekeep 同时把 running/pending span 标 failed；`release_object_ref` 到 0 删 blob；内存 `persist_index_snapshot` 仅在 PG 尚无 chunk 时 insert，不再整表 replace
- 写成 `processing` 用 `WHERE NOT IN (cancelled,deleting,completed)`；`list_delete` 只动仍为 `deleting` 的行；`kb:delete` / `DELETE /versions` 先 cancel 再标 deleting；HTTP 入队失败回写 PG `failed`；reparse 内存+oxana 都入队；wiki ingest 可重试不 FinalizeSubtask，`wiki:finalize` 真跑 slug/change/folder_prune；出题向量 `chunk_type=question`；PG graph hit 带 tag；`version_exists` 查询失败 500；`GET .../files` 先 hydrate

门禁：`.scratch/knowledgebrain/review.md`；CI 接 fmt / clippy / ruff / pyright / pytest / compose / spec 副本。

## 与仓库一致的进度（2026-08-15）

**done：** 01–34  
**partial：** 无

| 票 | Status | 和仓库不一致时曾被错标 done 的点 |
|---|---|---|
| 01 | done | 骨架、compose、健康检查 |
| 02 | done | oxana 2.1 已选型并入队 `default` |
| 03 | done | JWT + 内存目录 |
| 04 | done | JWT + `X-API-Key`；key 写入 PG，鉴权可回源 |
| 05 | done | oxana `default` + 入队失败 200；对象仍内存 |
| 06 | done | review 通过 |
| 07 | done | Python 进程 + tonic ReadStream 客户端 |
| 08 | done | OLE/路由测试通过 |
| 09 | done | 消费 + tonic + ASR 写回 |
| 10 | done | 30min + 2h 已接线 |
| 11 | done | 策略链 + 7500 硬顶；向量宽 1024，HTTP 未配时 hashed stub |
| 12 | done | 0004 HNSW + 0008 `vector(1024)`；`/search` 内存无命中走 `assembly_pg` |
| 13 | done | oxana cron 5min + PG/内存扫超时 |
| 14 | done | review 通过；无 queue inspector |
| 15 | done | Redis pending + scanned_pdf prompt；无外部 VLM |
| 16 | done | review 通过 |
| 17 | done | yaml prompt + 24k sample + stub/HTTP chat |
| 18 | done | attemptSuperseded 不 FinalizeSubtask |
| 19 | done | extract.go Extractor + Formater；PG 真源 + 可选 Neo4j 双写 |
| 20 | done | review 通过 |
| 21 | done | 双 lane + 防抖调度 + slug 锁 / tombstone / stale claim |
| 22 | done | oxana `WikiQueue` + PG pending claim |
| 23–24，27–29 | done | library/TAG/matching/answer；`/match` 内存无命中走 `matching_pg` |
| 25 | done | PG `run_clone` + oxana `low`；keep 同 embedding 拷 0004 行 + `clone_keep` |
| 26 | done | review 通过；worker 已消费 `low` 上的 `version:clone` |
| 30 | done | current `summary_model_id` + default_kb prompt |
| 31–32 | done | 生命周期类型名与 cancel/reparse/delete |
| 33 | done | `0001_domain.sql` + persist 测过 compose Postgres |
| 34 | done | hydrate 含 tags/graph/wiki/chunks；检索/匹配内存空则走 PG |

## Decisions-so-far

- Version 是快照容器；TAG 是分类。
- library 承载公司资质。
- 队列目标是 oxana；DocReader 仅 gRPC ReadStream。
- Matching 是多条款荐产品，无 best_product_id。
- 不做 TOKEN 成本。
- edition **2024** / rust-version **1.97**，crate 全部 `edition.workspace = true`。
- 01–32 主路径曾用内存 Store 验证语义；生产数据面是 Postgres。
- **克隆：crate 留在 `crates/clone`；实现必须是 PG + oxana `low`，不再扩内存 `run_clone`。**
