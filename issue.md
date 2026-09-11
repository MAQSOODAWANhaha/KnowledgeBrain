# Migration 与向量存储架构评审报告

## 评审范围

本报告基于当前工作树进行只读评审，重点回答：

1. 当前 migration 设计是否合理；
2. 大量数据库表和存储过程是否都有必要；
3. 是否需要引入独立向量数据库；
4. 当前 V21 变更中有哪些合并前应处理的问题。

评审由一轮三个 fresh-context reviewer 并行完成，角度分别为：

- Migration 生命周期、部署与升级安全；
- 表、存储过程及 V21 持久化设计的必要性；
- pgvector、检索一致性与独立向量数据库需求。

评审结论：无 P0；存在多个 P1。三位 reviewer 的合并结论均为 `BLOCK`。

---

## 1. 总体结论

当前设计的主要问题不是“三个 baseline 太多”，而是同时保留了两套互相矛盾的数据库生命周期模型：

1. 正式文档声明只支持 clean-slate fresh bootstrap；
2. 仓库中仍保留若干没有统一编排、版本账本和 checksum 的 `*_live.sql` 升级脚本。

主 bootstrap 路径本身比较清晰：

```text
migrator
  → advisory lock
  → schema_slice_state 检查
  → 单事务依次执行：
      knowledge_base_baseline.sql
      shared_platform_baseline.sql
      bidding_v2_baseline.sql
```

合理之处包括：

- 三个 baseline 按固定依赖顺序执行；
- 三个 baseline 位于同一个事务中，失败整体回滚；
- advisory lock 防止并发 bootstrap；
- API、Worker、Retention 启动时不执行 DDL；
- runtime 数据库身份与 migrator 身份分离。

证据：

- `crates/platform/src/bin/migrator.rs:2-12`
- `crates/platform/src/db.rs:148-182`
- `deploy/docker-compose.yml:146-187`
- `deploy/README.md:7-11`
- `docs/bidding/backend-runbook.md:9-11`

因此：**bootstrap 机制基本合理，但 schema 版本识别和 live upgrade 生命周期不合理。**

---

## 2. 各 migration 文件的必要性

| 文件 | 当前作用 | 必要性判断 |
| --- | --- | --- |
| `migrations/knowledge_base_baseline.sql` | 创建 Knowledge catalog、检索、索引、知识图谱及相关函数 | Fresh 安装必要 |
| `migrations/shared_platform_baseline.sql` | 创建 actor、幂等、审计、队列、ObjectRegistry、retention 和运行权限 | Fresh 安装必要 |
| `migrations/bidding_v2_baseline.sql` | 创建当前 active bidding V2/V21 schema、存储过程和权限 | Fresh 安装必要 |
| `migrations/shared_platform_retention_live.sql` | 替换四个 retention 函数 | Clean-slate 模式下不必要；只对保留旧库升级有意义 |
| `migrations/bidding_v2_semantic_spine_live.sql` | 修改既有 V2 schema 并替换 outline 相关函数 | Clean-slate 模式下不必要；当前也不是完整 V21 升级 |
| `migrations/.gitkeep` | 目录占位 | 目录已有 SQL 后无实际作用，保留无害 |

三个 baseline 的执行顺序确实存在依赖：

- Knowledge 先创建核心 catalog；
- Shared 创建 Bidding 使用的 domain、ObjectRegistry 和共享函数；
- Bidding 依赖前两者，并直接修改 shared/knowledge 对象。

例如 `migrations/bidding_v2_baseline.sql:968-984` 会修改 `object_registry`、`knowledge_image_artifact_revisions` 和 `knowledge_matching_scope_attestations_v2`。

因此不建议为了减少文件数量而机械合并三个 baseline。它们是合理的领域分片，但应明确它们共同构成一个原子 fresh schema snapshot，而不是三条可独立升级的 migration。

另外，`scripts/bidding_v2_phase*_live.sql` 实际由 `scripts/fresh_schema_acceptance.sh:17-21` 用作数据库 acceptance fixture，不是生产 migration。建议改名为 `*_acceptance.sql`，避免造成支持线上升级的误解。

---

## 3. P1：旧持久卷可能被错误认定为当前 schema

`schema_slice_state()` 只抽样检查部分表、列、约束和函数签名：

- Knowledge：约 7/33 张表；
- Shared：约 6/14 张表；
- Bidding：约 27/95 张表；
- Bidding functions：约 37/159 个函数签名。

证据：

- `crates/platform/src/db.rs:45-143`
- `crates/platform/src/db.rs:155-158`

它没有完整检查：

- 函数 body；
- 大多数表、列和索引；
- trigger；
- grant 和 owner；
- seed 内容；
- 当前 baseline digest。

当前 V21 变更新增：

- `bid_outline_v21_stage_artifacts`；
- `kb_bid_v21_outline_stage_get`；
- `kb_bid_v21_outline_stage_put`。

对应位置：

- `migrations/bidding_v2_baseline.sql:8809-8869`

Rust 新路径会调用这些函数：

- `crates/bidding/src/bid_authoring_v2.rs:1596-1638`

但这些对象没有进入 `schema_slice_state()`。因此，保留上一版本 PostgreSQL volume 并部署新镜像时，migrator 可能错误地返回成功；直到第一次 V21 outline request 执行时，Worker 才因数据库函数不存在而失败。

### 建议

如果坚持 clean-slate，不应继续扩充手写 catalog fingerprint。更简单可靠的做法是：

- fresh bootstrap 成功时写入一个精确的 `schema_revision` 或 baseline digest；
- binary 声明其期望 revision；
- revision 不一致时在 runtime 启动前明确拒绝，并要求 reset。

一行精确 marker 比不断增长的不完整 fingerprint 更简单，也更安全。

---

## 4. P1：semantic live 已经与 V21 baseline 漂移

`migrations/bidding_v2_semantic_spine_live.sql` 由三部分共同维护：

1. 从 baseline 中字符串抽取函数；
2. Python 中手写 ALTER、seed 和 grant；
3. checked-in 的生成 SQL 文件。

生成器位置：

- `scripts/generate_bidding_v2_semantic_spine_live.py:6-17`
- `scripts/generate_bidding_v2_semantic_spine_live.py:20-143`

当前工作树的只读生成比较结果为：

```text
semantic live generated file in sync: false
```

至少以下函数已不一致：

- `kb_bid_v2_publish_outline_generation`
- `kb_bid_v2_outline_run_upsert`

Fresh baseline 中的 V21 publisher 会验证新的 `template_operations` 和 V21 closure helper：

- `migrations/bidding_v2_baseline.sql:6124-6255`

但 live SQL 仍会安装旧 V20 publisher：

- `migrations/bidding_v2_semantic_spine_live.sql:727-767`

Baseline 中新的 run-upsert 支持当前终态行为：

- `migrations/bidding_v2_baseline.sql:8981-9029`

Live SQL 中仍是旧实现：

- `migrations/bidding_v2_semantic_spine_live.sql:778-826`

更严重的是，生成器没有包含新的 V21 stage 表和 helper。即使重新运行生成器，也不能把旧 V2 数据库完整升级到当前 V21 schema。

该 live SQL 目前并非完全休眠：

- `scripts/fresh_schema_acceptance.sh:124-129` 会先建立 fresh schema，然后再次执行这个 stale live SQL。

这可能主动把 fresh V21 函数覆盖回旧 V20 实现。

### 建议

必须选择以下方案之一：

- 严格 clean-slate：删除 semantic live、生成器及 stale-schema repair acceptance；
- 支持旧库升级：将其改为完整、不可变、编号的 forward migration，并验证 previous release → current 与 fresh bootstrap schema 等价。

不建议继续维护一个可变的“latest live SQL”。

---

## 5. P1：CI 没有执行真实 migration

README 将 fresh schema acceptance 列为 gate：

- `README.md:33-43`

但 GitHub CI：

- 没有启动 PostgreSQL/pgvector service；
- 没有运行 `scripts/fresh_schema_acceptance.sh`；
- 没有检查生成 SQL 是否与 baseline 同步；
- 没有验证上一版本 schema 被升级或明确拒绝。

证据：

- `.github/workflows/ci.yml:14-60`

部分 PostgreSQL contract tests 在没有显式数据库 URL 时会跳过：

- `crates/bidding/tests/support/mod.rs:9-17`

因此，SQL 能被 `include_str!` 编译进 binary，并不能证明它可以在真实 PostgreSQL 上执行。

### 建议

CI 至少应覆盖：

1. empty database fresh bootstrap；
2. migrator 对同版本 schema 重入；
3. previous schema revision 被升级或明确拒绝；
4. runtime ACL；
5. generated SQL freshness，或直接删除 generated live 路径。

---

## 6. 表很多是否等于过度设计

当前三个 baseline 大约包含：

- 142 张表；
- 214 个数据库函数；
- 其中 Bidding 约 95 张表、159 个函数；
- Bidding 中约 48 张 `*_artifacts` 表。

表和函数数量确实较大，但不能仅按数量判断为冗余。

### 大部分表承担不同生命周期职责

典型模式包括：

```text
artifact   = 不可变历史记录
current    = 当前 CAS 指针
items      = 冻结集合的精确成员
lineage    = 跨 revision 的稳定业务身份
revision   = 某次不可变内容版本
occurrence = 某个 WorkspaceRevision 中的树、顺序或绑定
receipt    = 幂等或终态结果
identity   = typed request/object identity 与复合外键边界
```

Reviewer 没有证明以下 cluster 可以安全合并：

- Project/document/source artifact、item 和 current；
- Requirement set/revision/source/supersession/projection；
- Workspace node/block/binding lineage、revision、occurrence 和 head；
- Generic async request 与五种 typed request projection；
- Candidate、operation 和 decision receipt；
- Evidence、bundle、asset 和 assessment；
- Quote snapshot、object identity 和 current；
- Render snapshot、manifest、dependency 和 output；
- Shared ObjectRegistry、retention 和 queue；
- Knowledge ingestion/index/retrieval 表。

这些拆分用于保证：

- append-only；
- 复合外键；
- 防止跨 workspace 或跨 digest identity splicing；
- 精确 replay；
- CAS；
- 来源追踪；
- runtime 最小数据库权限。

将它们合并为万能 JSONB 表虽然会减少表数量，但会把关系约束和原子性转移到应用代码中，并不一定降低系统总复杂度。

粗略静态分析也没有发现大规模无调用函数：约 210/214 个数据库函数可从 runtime、trigger 或其它数据库函数路径到达。因此不能按存储过程数量进行批量删除。

---

## 7. 已证明可简化的 V20 outline persistence cluster

当前 V21 已经改用通用：

```text
bid_outline_v21_stage_artifacts
kb_bid_v21_outline_stage_get
kb_bid_v21_outline_stage_put
```

新表提供：

- closed stage kind；
- deterministic replay key；
- canonical payload digest；
- divergent replay 检测；
- 通用 get/put seam。

证据：

- `migrations/bidding_v2_baseline.sql:8809-8869`

与此同时，旧 V20 stage-specific subsystem 仍然存在：

- `bid_outline_evidence_batch_artifacts`
- `bid_outline_requirement_grouping_batch_artifacts`
- `bid_outline_reduce_plan_artifacts`
- `bid_outline_synthesis_packet_artifacts`
- `bid_outline_agent_tool_traces`
- `bid_outline_agent_checkpoint_artifacts`

位置：

- `migrations/bidding_v2_baseline.sql:8871-8980`

以及对应的：

- map/group/reduce/synthesis/trace/checkpoint/tool procedures：`:9027-9428`；
- Worker EXECUTE grants：`:9461-9476`；
- Rust wrappers；
- V20-only JSON schemas 和静态合同测试。

当前生产入口已切换至：

- `crates/worker/src/consume.rs:1329-1334`

它调用 V21 generic wrappers：

- `crates/bidding/src/bid_authoring_v2.rs:1596-1637`

父级静态检查发现旧 wrappers 没有生产调用，剩余消费者主要是历史 acceptance/live 机制。

### 建议

如果确认 V21 clean-slate 完全取代 V20，可以删除：

1. 六张旧 V20 stage 表；
2. 对应 SQL procedures；
3. 对应 Rust wrappers；
4. 对应 grants；
5. V20-only JSON schemas；
6. 只验证旧合同存在的静态测试。

保留：

- `bid_outline_v21_stage_artifacts`；
- V21 get/put；
- `bid_outline_agent_run_artifacts`；
- publication 与 terminal state 更新。

这是一项有调用证据支持的简化，不是按表名猜测删除。

---

## 8. P1：V21 terminal error 无法可靠落库

当前 `bid_async_request_snapshot_artifacts.error_code` 的 CHECK 仍只允许旧错误集合：

- `migrations/bidding_v2_baseline.sql:882-895`

V21 新代码会产生额外错误码。父级静态验证发现有 7 个 emitted code 不在数据库允许集合中：

```text
OUTLINE_OBSERVATION_PARTITION_INVALID
OUTLINE_REQUIREMENT_CLOSURE_INVALID
OUTLINE_SECTION_APPENDIX_INVALID
OUTLINE_SECTION_COUNT_INVALID
OUTLINE_SECTION_EMPTY
OUTLINE_SECTION_IDENTITY_INVALID
OUTLINE_SECTION_ORDINAL_INVALID
```

V21 错误来源包括：

- `crates/bidding/src/outline_v21_plan.rs:623-644`
- `crates/bidding/src/outline_v21_plan.rs:748-750`
- `crates/bidding/src/outline_v21_plan.rs:868-896`

失败函数保留并写入这些错误码：

- `migrations/bidding_v2_baseline.sql:6310-6330`

因此失败 UPDATE 会违反 CHECK 并回滚，请求可能一直停留在 `pending`。

### 建议

- 同步数据库 closed error-code constraint 与 V21 错误集合；
- 为每个错误 family 增加 procedure-level 测试；
- 验证失败写入与 AgentRun/request terminalization 位于同一原子路径。

---

## 9. P2：V21 stage writer 的并发边界仍可加强

原评审时的计划要求每个 stage boundary 重新检查 request pending/current attempt：

- 历史出处：`plans/bidding/outline-v21-simplified.md:278-282`（该计划已删除；此处仅保留原评审依据，不是有效链接或当前实施依据）。

但 `kb_bid_v21_outline_stage_put` 当前只检查 request ID/digest：

- `migrations/bidding_v2_baseline.sql:8844-8854`

它没有验证：

- `request_kind='outline_generate'`；
- `status='pending'`；
- 当前 AgentRun attempt；
- run 是否已被 supersede/terminalize。

Rust 会在模型调用前检查 pending，但模型返回到 stage insert 之间仍存在竞争窗口：

- `crates/bidding/src/outline_agent_v21.rs:187-205`

### 建议

在写入前锁定并重新验证 request 和当前 AgentRun attempt。`attempt` 不一定需要成为 artifact identity，但应成为写入合法性条件。

---

## 10. 是否需要引入其它向量数据库

### 结论：当前没有证据需要

当前检索并不是单纯的“向量 + 任意 metadata”，而是与 PostgreSQL 关系数据紧密耦合：

- ProductVersion；
- Document 状态；
- source type；
- tags；
- embedding revision；
- workspace kind；
- retrieval policy；
- immutable generation marker。

证据：

- `crates/knowledge/src/persist.rs:3598-3614`
- `crates/knowledge/src/knowledge_retrieval_pg/semantic_v2.rs:896-907`

当前实现还结合：

- PostgreSQL GIN keyword recall；
- pgvector recall；
- deterministic RRF/reranking；
- chunk 与 vector generation 的事务性发布；
- provider/reconcile 失败后保留上一代可用索引。

证据：

- `migrations/knowledge_base_baseline.sql:1431-1448`
- `migrations/knowledge_base_baseline.sql:1575-1597`
- `crates/knowledge/src/knowledge_index_v2.rs:442-456`
- `crates/knowledge/src/knowledge_index_v2.rs:522-539`

当前规模证据也不足以证明 pgvector 已成为瓶颈：

- 一个文档样例约 288 chunks：`plans/knowledge-base/document-detail.md:5`；
- 检索 assembly 最多 20 个 target versions：`crates/knowledge/src/search/mod.rs:381-384`；
- 单 ProductVersion V2 indexing 上限为 16,384 inputs：`crates/knowledge/src/knowledge_index_v2.rs:9-14`。

引入独立向量数据库会新增：

- PostgreSQL → vector DB 双写或 CDC；
- metadata 与权限复制；
- 跨库 publication identity；
- backup/restore 两套系统；
- rebuild、replication lag 和部分失败处理；
- 更多凭据、监控、容量与部署资源。

在没有性能证据的情况下，这会显著增加而不是减少复杂度。

---

## 11. 应先优化当前 pgvector 查询

Schema 创建了 cosine HNSW index：

- `migrations/knowledge_base_baseline.sql:1442-1447`

但当前 V2 query 会先计算全部 eligible rows 的距离，再按量化分数和关系字段进行确定性排序：

- `crates/knowledge/src/knowledge_retrieval_pg/semantic_v2.rs:871-907`

这种查询不一定能利用标准 ANN 路径：

```sql
ORDER BY embedding <=> $query
LIMIT k
```

因此 HNSW 可能只有写入和存储成本，却没有明显减少查询计算。

### 建议顺序

1. 对代表性数据执行 `EXPLAIN ANALYZE`；
2. 建立真实 latency、QPS、recall 和 scope-size benchmark；
3. 明确必须 exact deterministic ranking，还是允许 ANN candidate oversampling；
4. 如果必须 exact scan，考虑删除没有收益的 HNSW index；
5. 如果允许 ANN，先在 PostgreSQL 内进行 oversampled candidate recall，再执行确定性 quantization/reranking；
6. 完成上述步骤后再考虑独立向量数据库。

### 建议的重新评估门槛

只有出现以下证据时，才应重新评估独立向量存储：

1. 在两倍预测峰值 QPS 和两倍十二个月预测向量量下，调优后的 pgvector 无法达到明确的 p95/recall SLO；
2. vector workload 导致 PostgreSQL OLTP p95 明显恶化，或 CPU/I/O 长期处于高水位；
3. 单个 eligible retrieval scope 接近或超过一百万 vectors，exact scan 无法满足延迟要求；
4. full rebuild 无法满足正式定义的 RTO/RPO；
5. vector workload 必须独立扩缩容或隔离故障域。

上述是建议的决策门槛；当前仓库没有达到这些门槛的生产数据。

---

## 12. 向量检索中的两个 P1，但不是换数据库的理由

### 12.1 永久错误被当作 availability failure 重试

领域类型区分：

- `QuotaExceeded`；
- `Unavailable`。

证据：

- `crates/knowledge/src/knowledge_retrieval.rs:1204-1213`

但 Worker 会把 retrieval 错误统一转换为 `EVIDENCE_UNAVAILABLE`，然后走通用 retry：

- `crates/worker/src/consume.rs:2052-2063`
- `crates/worker/src/consume.rs:2251-2284`

这与 quota failure 不应重试的合同矛盾：

- `plans/knowledge-base/retrieval-ranking.md:90-94`

建议保留 typed error：

- 仅 `Unavailable` 自动重试；
- quota、invalid policy、revoked policy 立即终态失败；
- 增加调用次数测试，证明永久错误不会触发第二次 retrieval。

### 12.2 Retry 没有冻结 retrieval policy identity

每次 attempt 当前都会读取最新 supported policy：

- `crates/worker/src/consume.rs:2002-2008`
- `crates/knowledge/src/knowledge_retrieval_pg/mod.rs:846-856`

但 content request 的冻结 payload 没有携带完整 retrieval policy identity：

- `migrations/bidding_v2_baseline.sql:7024-7041`

合同要求 retry 在 policy promotion 后仍使用原始冻结 policy digest：

- `plans/knowledge-base/retrieval-ranking.md:186-190`

因此，同一个 durable request 可能在不同 attempt 使用不同向量、quota 和 ranking 行为。

建议冻结完整 `RetrievalPolicyIdentityV1`，并要求策略变化时创建新的 request。这是 PostgreSQL/idempotency 问题，不是引入另一个向量数据库的理由。

---

## 13. 其它 P2 事项

### Catalog owner 声明与实际 owner 不一致

`platform_role_contracts` 声称：

```text
kb_app_owner owns the application catalog
```

位置：

- `migrations/shared_platform_baseline.sql:58`

但实际 DDL 由拥有 schema CREATE 权限的 `kb_migrator` 执行：

- `deploy/postgres-init/010-runtime-identities.sh:19-35`

没有发现 baseline 使用 `SET ROLE kb_app_owner` 或转移对象 ownership。

建议：

- clean-slate 简化模式下删除未使用的 owner 抽象或修正文档；
- durable migration 模式下，让 migrator 受控 `SET ROLE kb_app_owner`，由稳定 no-login role 持有 catalog。

### 架构文档对 retrieval ownership 存在矛盾

- `plans/architecture.md:150-159` 允许 Bidding 直接读取并取消 mandatory retrieval port/attestation；
- `docs/knowledge-base/domain.md:8,41-48,98-102` 将 retrieval port 定义为唯一跨域 seam，并禁止直接 join。

当前代码遵循后者。考虑任何向量数据库拆分前，应先指定唯一权威合同并标记旧文档已失效。

---

## 14. 推荐方案

### 方案 A：严格 clean-slate

该方案与当前产品文档最一致。

保留：

- 三个 baseline；
- 固定执行顺序；
- 单事务；
- advisory lock；
- migrator/runtime 身份隔离；
- runtime 不执行 DDL。

删除或调整：

1. 删除两个 `*_live.sql`；
2. 删除 semantic live generator；
3. 删除 stale-schema repair acceptance；
4. 将 `scripts/bidding_v2_phase*_live.sql` 重命名为 acceptance fixture；
5. 删除旧 V20 stage-specific persistence subsystem；
6. 用一个精确 schema revision/digest 替代手写 fingerprint；
7. 生产镜像使用不可变 release tag/digest，而不是裸 `latest`；
8. 新版本遇到旧 schema 时在 runtime 启动前明确要求 reset；
9. CI 运行真实 PostgreSQL fresh acceptance。

优点：实现简单，与当前 clean-slate 声明一致。

代价：版本升级不能保留旧数据库数据。

### 方案 B：支持 durable-data upgrade

如果生产数据必须跨版本保留，则应采用正式 migration chain：

```text
schema_migrations(
  version,
  name,
  checksum,
  applied_at
)
```

要求：

1. migration 不可变、编号、forward-only；
2. 所有 migration 使用同一个 Rust runner 和 advisory lock；
3. `*_live.sql` 转换为正式历史 migration；
4. V21 migration 必须包含表、helper、函数 body、grant 和 owner；
5. CI 验证 previous release → current；
6. upgraded schema 与 fresh bootstrap schema 做等价性验证；
7. baseline 仅作为新安装 snapshot；
8. rollback 使用 backup/restore 或 forward-fix，而不是假设存在可靠 down SQL。

优点：支持持久数据和可审计升级。

代价：需要长期承担 migration 测试、兼容窗口和发布管理成本。

---

## 15. 最终裁决

### P0

无。

### 合并前应处理的 P1

1. 修复 V21 error-code CHECK，确保所有 terminal error 可落库；
2. 处理 stale semantic live，禁止它覆盖 V21；
3. 让旧持久卷被精确升级或明确拒绝；
4. 用 schema revision/checksum 替代不完整 fingerprint；
5. CI 执行真实 fresh migration；
6. 冻结 retrieval policy identity；
7. 保留 typed retrieval error，避免永久错误被重试。

### 建议清理的 P2

1. 删除无生产调用的旧 V20 stage persistence cluster；
2. 删除相应旧 SQL procedures、Rust wrappers、grants 和 schemas；
3. 加强 V21 stage writer 的 pending/current-attempt 检查；
4. benchmark HNSW 是否服务当前查询；
5. 统一 retrieval ownership 架构文档；
6. 修正 `kb_app_owner` 声明与实际 ownership。

### 明确延后

- 引入独立向量数据库：当前无必要；
- 全面合并 artifact/current/item 表：没有安全性证据；
- 仅为减少文件数量而合并三个 baseline：收益有限且会损失领域边界。

---

## 16. 待确认的架构决策

在开始修改前，需要明确：

> 已部署 PostgreSQL 数据在版本升级时，是否必须保留？

- 如果不需要保留，选择方案 A，彻底执行 strict clean-slate；
- 如果需要保留，选择方案 B，建立正式 migration chain。

当前同时保留 clean-slate 和零散 live upgrade 的混合状态，是整个设计中最复杂、也最危险的部分。

---

## 17. 验证记录

- Review rounds：1；
- Fresh-context reviewers：3；
- `git diff --check`：通过；
- V21 error code 与数据库 constraint 静态对比：确认 7 个缺失；
- semantic live 生成一致性：确认不一致；
- 旧 V20 wrapper 静态生产调用检查：未发现有效 runtime 调用；
- 未运行真实 PostgreSQL acceptance；
- 未修改既有项目文件；本报告仅新增 `issue.md`。
