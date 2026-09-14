# 共享平台运行时基础方案

| 项 | 值 |
| --- | --- |
| 状态 | 方案已确认；普通实施与隔离开发验证已授权，任务见[实施台账](../implementation-tasks.md) |
| 所有者 | Shared Platform |
| 消费方 | 知识库、招投标 |

本文是 fresh baseline、schema snapshot、共享 actor/idempotency/audit、`ObjectRegistry` 和 retention 的唯一活动定义。队列能力由 [`queue-runtime.md`](queue-runtime.md) 定义；业务领域只定义自己的 target、业务引用和技术消费校验。招投标产品目标见 [PRD](../../docs/bidding/prd.md) 和 [ONLYOFFICE](../../docs/bidding/onlyoffice.md)，来源/身份接缝见 [领域契约](../../docs/bidding/authoring.md)；不以业务检查阻断编辑/导出。旧 first-launch/intended-state/verifier 与 V1 slice 已删除；最终系统使用三份 clean baseline、单行 release receipt 与 runtime read-only catalog verification。

## 1. 所有权边界

共享平台拥有：

- fresh baseline、extension、普通 bootstrap 角色与最小权限 runtime identity；
- authenticated actor、共享幂等 intent/receipt 和 append-only audit envelope；
- `ObjectRegistry`、owner reference、immutable deletion artifact/tombstone 与 Oxana typed 物理删除 consumer；
- Oxana 版本、queue registry 和 worker runtime；
- maintenance gate、健康检查、日志、tracing、metrics 与部署验收基础。

业务领域拥有：

- 业务 target、status、generation/revision 和结果；
- artifact、current pointer、operation 和 payload；

平台和业务不得复制 Oxana 的 transport retry、resurrection、queue membership 或 dead-job 状态机。显式 Agent-bound Request 可以按 [`queue-runtime.md`](queue-runtime.md) 保存 DB-generated AgentRun attempt/token/lease 与 physical-call ledger，用于 fence 外部业务副作用；这些字段不得表示 Oxana transport phase。

## 2. Fresh baseline

**当前尚未发布，直接维护所属 baseline。** 未发布阶段的 schema 修正同步调整必要的 Rust 调用、共享 schema/错误码和测试，不新建历史 migration chain、升级账本、兼容 live SQL 或双写框架。三份 baseline 是同一个原子 fresh snapshot 的领域分片，不因文件或表数量多而机械合并。

首次正式发布前固定 baseline/revision、镜像、PostgreSQL 与扩展身份并留存验收证据；发布后如需保数据升级，再单独制定策略。当前允许修改 baseline，不表示永久支持破坏性升级，也不表示开发资料可自动删除。

从空 PostgreSQL 按固定顺序、一个 advisory lock 和一个 transaction 执行三份所有权切片：

```text
shared_platform_baseline.sql
knowledge_base_baseline.sql
bidding_v2_baseline.sql
```

必须满足：

1. Shared 先创建通用 ObjectRegistry/platform primitives；Bidding 不 ALTER Shared/Knowledge-owned object。现存 Shared→Knowledge relocation 的 exact symbol set 是 `kb_register_knowledge_image_object`、`kb_register_knowledge_document_object`、`kb_release_knowledge_document_object`、`kb_validate_knowledge_document_object_reference`、`kb_guard_knowledge_document_delete`、`documents_object_reference_contract`、`documents_object_reference_delete_guard` 及这些函数的 runtime grants；Knowledge baseline 创建它们，Shared baseline 对这些 identifiers 必须零匹配；
2. 不创建旧投标表，不 ALTER/backfill/repair 到目标形态；
3. `kb_app_owner` 拥有 application catalog；`kb_migrator` 通过显式 `SET LOCAL ROLE kb_app_owner` 执行 baseline，runtime roles 无 DDL；
4. 三 baseline 后、同一事务内写恰好一行 `platform_schema_snapshot`；row 含 release revision、exact compiled baseline digests、catalog manifest digest、server/extension identity、deployment namespace；
5. Manifest 是 sorted canonical app-owned catalog projection，覆盖 relation/column/default、constraint/index、function/view/type/trigger、owner/ACL、role membership、database/schema/default ACL 与 frozen seed digests；包含 receipt table structure、排除 receipt row values；
6. runtime readiness 比较 compiled identity、receipt 与 fresh manifest；缺失、重复、mismatch fail-closed，绝不 DDL/repair；
7. matching schema 的 migrator 重放只做验证；partial/stale schema 先停止，由用户确认数据保留要求与隔离环境重建范围，再按 namespace reset 合同处理，不自动 reset/repair；
8. API、worker、retention 使用独立最小权限角色。

### 2.1 SchemaReceiptV1 与 CatalogManifestV1

`platform_schema_snapshot` 恰好一行，fixed columns 为：

```text
singleton boolean PRIMARY KEY CHECK(singleton)
schema_revision text
shared_baseline_sha256 char(64)
knowledge_baseline_sha256 char(64)
bidding_baseline_sha256 char(64)
manifest_contract_version integer
catalog_manifest_sha256 char(64)
postgres_server_version_num integer
extensions jsonb
release_descriptor_sha256 char(64)
deployment_namespace_id uuid
created_at timestamptz
```

除 `created_at` 外全部参与 runtime identity comparison；`extensions` 是按 name/version/schema 排序的 exact array。`manifest_contract_version=1`。Rust migrator/runtime 共享唯一 manifest builder；SQL `jsonb::text`、`pg_dump` 文本或 locale-dependent output 不得作为 preimage。

对象选择规则：

- relation/view/sequence/type/function/schema 自身 owner=`kb_app_owner` 时 include；column/default/constraint/index/trigger/policy 通过 owning relation 继承 owner 并 include，不要求 child catalog row 有独立 owner；
- include allowlisted runtime/migrator/app-owner roles、memberships、database/schema/default ACL；
- exclude `pg_catalog`、`information_schema`、temporary/toast、extension-owned definitions、snapshot row values、statistics/physical OID；
- extension 只在 receipt 的 ordered `{name,version,schema}` tuple 中冻结；
- seed tables 来自 target artifact `deploy/platform-frozen-seed-tables-v2.json`，artifact 是 closed `{schema_version:2,tables:[{schema,table,primary_key,primary_key_values?}]}`；tables 按 UTF-8 `(schema,table)` 排序，`primary_key` 保持 declared PK ordinal，并按 schema/table/primary-key tuple 读取完整业务列；混有运行期合同的表通过 `primary_key_values` 明确列出 baseline 初始身份，新增业务合同不参与 schema 指纹，指定初始身份缺失或被修改仍拒绝就绪；
- `pg_get_*` 一律使用 pinned PostgreSQL major 的 non-pretty output、LF，不 trim、不 rewrite expression。

每个 record 都是 closed object `{kind,schema,name,identity,owner,definition,acl,dependencies}`；字段 required，无值 JSON null。Common nested types：

```text
AclEntry={grantee,grantor,privilege,is_grantable}
DependencyRef={kind,schema,name,identity}
acl sort=(grantee,grantor,privilege,is_grantable)
dependencies sort=(kind,schema,name,identity)
```

字符串均按 UTF-8 byte order。Per-kind extraction/identity 固定为：

| kind | identity | owner | exact `definition` |
| --- | --- | --- | --- |
| `database` | database name | database owner | `{encoding,collate,ctype,locale_provider,is_template,allow_connections,connection_limit}` |
| `schema` | schema name | schema owner | `{}` |
| `relation` | `schema.name` | relation owner | `{relkind,persistence,row_security,force_row_security,replica_identity,partition_key:null|string}` |
| `view` | `schema.name` | relation owner | `{materialized,check_option,security_barrier,definition}` |
| `sequence` | `schema.name` | sequence owner | `{data_type,start,min,max,increment,cycle,cache}`; integral values decimal strings |
| `type` | `schema.name` | type owner | `{type_kind,category,preferred,delimiter,element_type:null|string,base_type:null|string,not_null,default:null|string,enum_labels:[string]}` |
| `column` | `schema.relation.column` | parent relation owner | `{ordinal,type,not_null,collation:null|string,identity_kind,generated_kind}` |
| `default` | `schema.relation.column` | parent relation owner | `{expression:pg_get_expr(adbin,adrelid,false)}` |
| `constraint` | `schema.relation.constraint` | parent relation owner | `{definition:pg_get_constraintdef(oid,false)}` |
| `index` | `schema.index` | indexed relation owner | `{definition:pg_get_indexdef(oid,0,false)}` |
| `trigger` | `schema.relation.trigger` | parent relation owner | `{definition:pg_get_triggerdef(oid,false)}` |
| `policy` | `schema.relation.policy` | parent relation owner | `{command,permissive,roles,using_expr:null|string,check_expr:null|string}`; roles sorted |
| `function` | `schema.name(identity_arguments)` | function owner | `{kind,return_type,language,volatility,security_definer,strict,parallel,definition}` |
| `role` | role name | null | `{superuser,inherit,create_role,create_db,can_login,replication,bypass_rls,connection_limit,valid_until:null|string}`; no password/verifier bytes |
| `membership` | `role/member/grantor` | null | `{role,member,grantor,admin_option}` |
| `default_acl` | `owner/schema/object_type` | owner role | `{entries:AclEntry[]}` |
| `seed_row` | `schema.table/JCS(primary_key)` | table owner | `{primary_key,values}` using typed JSON encoder below |

`acl`/`dependencies` 永远是 arrays（无项为 `[]`，不是 null）。ACL 只展开 explicit catalog ACL：catalog ACL 为 `NULL` 时固定 `[]`，非空时直接来自 `aclexplode(catalog_acl)`（不得与 `acldefault` 合并）；PUBLIC grantee 固定 literal `PUBLIC`；dependencies 来自 `pg_depend`，只保留 manifest 内对象 ref、排除 internal/extension edges。`identity_arguments` 使用 `pg_get_function_identity_arguments`；view/function/expression definitions 使用 pinned non-pretty `pg_get_*`。Seed typed encoder：boolean/string/null 直接 JSON；integer/numeric 在进入 JSON decoder 前投影为 exact text，再编码为无 exponent canonical decimal string；UUID lowercase string；timestamp 转 UTC RFC3339 microseconds；bytea lowercase hex；json/jsonb 解析后保留 RFC 8785 finite IEEE-754 number domain（含 fractional/exponent values）并由标准 JCS serializer 输出；array 保持 SQL ordinal/全部 dimensions 并按 scalar type 递归编码。Enum labels 按 enumsortorder；其它 definition arrays 按表中语义 key 排序，绝不按 OID/catalog return order。Records 按 `(kind,schema,name,identity)` 排序并序列化为 RFC 8785 JCS UTF-8：

```text
catalog_manifest_sha256 =
SHA256("KB:PlatformCatalogManifest:v1\0" || JCS(records))
```

Target artifact `deploy/catalog-manifest-v1.schema.json` freezes these closed shapes; independent golden fixtures must include ACL order, dependency order, inherited-owner children, overloads, seed rows and extension exclusion. Receipt table structure is included; receipt row values are excluded. PostgreSQL major or manifest version changes require a new schema revision.

## 3. Actor、幂等与审计

平台 actor identity 固定为：

```text
user:<lowercase-uuid>
api_key:<lowercase-uuid>
system:<allowlisted-bounded-name>
```

`user` 与 `api_key` 即使 UUID 相同也不是同一 actor。Bootstrap 只允许启动管理，不能伪造需要人工确认的业务决定。

共享幂等 identity 固定为：

```text
scope = actor_identity + operation + idempotency_key
request_identity = schema_version + exact operation payload + payload_sha256
```

同 key 同 hash 返回首次 completed receipt；同 key 不同 hash 返回 `IDEMPOTENCY_PAYLOAD_MISMATCH`；瞬时失败回滚且不写伪 completed receipt。领域 mutation 必须把领域写、revision/digest、current/stale、audit 与 receipt 放在同一 transaction。

audit envelope 至少冻结 operation、actor、request/response identity、before/after revision+digest、entity locator 和 UTC 时间；append-only 历史不得 UPDATE/DELETE。

## 4. 发布依赖与进程生命周期

- Oxana 精确锁定为 crates.io 2.1.3，完整 source/checksum 只由 `Cargo.lock` 固化；
- 构建与 CI 使用 `cargo --locked`，禁止 fork、vendor、Git/path/patch/source replacement；
- workspace image 从干净、已提交候选构建，部署证据记录 candidate SHA、`Cargo.lock` SHA 和实际 image digest；
- worker 的 shutdown、SIGINT、SIGTERM 和进程 heartbeat/resurrection 直接使用 Oxana runtime；
- handler 自己启动的外部子进程在退出时 cancel，最多等待 5 秒，随后 kill process group、wait/reap；
- 不增加独立 `bid-dispatcher`、outbox、PostgreSQL transport scanner、DSN、credential 或 activation hold；用户显式 HTTP replay 直接复用 Oxana queue-native uniqueness。

### 4.1 ReleaseDescriptorV1 binding

Production 唯一 release 入口由 [`../../deploy/README.md`](../../deploy/README.md) 的 `knowledgebrain-release` 定义。`release_revision` 与 `platform_schema_revision` 都固定为 ASCII `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`，不 trim/normalize。Migrator/API/Worker/Retention 必须挂载同一 read-only descriptor，由 exact `KB_RELEASE_DESCRIPTOR_PATH` 加载，重算 domain-separated JCS hash，并比较 `KB_RELEASE_DESCRIPTOR_SHA256` 与 canonical-lowercase `KB_DEPLOYMENT_NAMESPACE_ID`；`KB_COMPONENT_KIND` 选择 descriptor 中的 full digest-only image ref，`KB_COMPONENT_IMAGE_DIGEST` 则必须是从该 ref 提取并 byte-for-byte 比较的 exact lowercase `sha256:<64hex>` suffix。Descriptor hash 绑定 full ref；只有外部 release tool inspect actual full RepoDigest，runtime 不作 container-self-inspect 声明。Migrator 将 hash 写入 SchemaReceiptV1；runtime readiness 要求 mounted hash=env hash=receipt hash。外部 release tool 通过 container engine inspect 证明实际 container image digest=descriptor mapping；runtime 不伪称可从容器内部自行证明 registry digest。任一 target artifact/field/mount/inspect 缺失均 not-ready，禁止使用 tag fallback。

## 5. ObjectRegistry

平台唯一对象标识为：

```text
object_ref = <deployment_namespace_id>/objects/<64-lowercase-hex>
digest, media_type, byte_length
state = available|deleting|deleted
owner references
immutable deletion artifact/tombstone
```

规则：

- deployment namespace 使用 path-safe closed grammar；object key 只接受 namespace-bound canonical 形式；绝对路径、`..` 和 alias 全部拒绝；
- 业务通过受检接口注册/移除 owner reference、读取 available 对象，不维护第二套 refcount；
- 写物理 bytes 前创建有时限的 staging owner reference，业务事务原子转移给最终 owner；
- crash 遗留 staging 由 retention expiry 回收；
- 任一 reference 存在时不得进入 `deleting`；
- 普通 API/worker 无物理删除权限；
- 同 digest 的 `deleted` 对象在 V1 拒绝复活；
- 旧 `content_objects.ref_count` 与公开 bump/release/delete/drop 旁路删除，不留 alias 或双写。

### 5.1 DeploymentNamespaceV1 与 reset

实施进展见[namespace 隔离记录](../../docs/platform/namespace-isolation.md)：canonical UUID 类型、Neo4j 属性隔离、OBJECT_DIR/MinIO layout 和 Redis/Oxana 派生前缀已落实到源码并经真实专属服务验证。Redis 多模态计数器也使用同一部署前缀。PostgreSQL 派生库名、reset 确认校验和及其实际连接/receipt/catalog 的只读 gate 已通过专属服务验证；本地对象根/namespace/objects 的实际设备号与 inode、逐层无符号链接只读校验也已补齐；Redis 独立连接的实际数据库、standalone primary 服务 run ID 及无凭据地址观察通过专属只读 ACL 实测；全后端 mapping、runtime 排空、checkpoint 和 reset 执行仍待完成。新代码未部署，不据此认领、搬迁或删除现有数据。

Receipt 中存 canonical UUID；派生字符串为 `kb-` + UUID 去连字符后的 32 个 lowercase hex。Backend layout：

```text
PostgreSQL database = kb_<32hex>
Redis/Oxana prefix = kb:<32hex>:
MinIO prefix = kb-<32hex>/
Neo4j database = kb<32hex>；不支持多 database 时每个 node/edge 强制 deployment_namespace_id predicate
OBJECT_DIR = <configured-root>/kb-<32hex>/objects/<sha256>
```

所有 join/path/key builder 接受 typed namespace，禁止调用方传任意 prefix；路径必须通过 containment check，拒绝 symlink escape、absolute path 和 `..`。

生产 reset 的目标命令固定为：

```text
deploy/reset-namespace.sh \
  --namespace kb-<32hex> \
  --expected-schema-revision <revision> \
  --confirm <token>
```

```text
token = uppercase hex SHA256(
  "KB:NamespaceReset:v1\0" + canonical_namespace + "\0"
  + expected_schema_revision + "\0" + postgres_database
)
```

这里的 `canonical_namespace` 是 `--namespace` 接受的精确 `kb-<32 lowercase hex>` 标签，receipt 内仍为 canonical UUID。确认校验和绑定目标标签、revision 与派生库名，不替代 runtime/backend/checkpoint 检查或删除权限。

首次调用先验证 runtimes drained、receipt namespace/revision、各 backend mapping 与 containment，然后在任何删除前以 atomic rename + file/directory fsync 写 mode-0600 checkpoint `reset-state/<namespace>.json`。Checkpoint 的 closed preflight 包含 namespace、revision、PostgreSQL database、全部 backend mapping、token digest、ordered step states 与：

```text
preflight_sha256 = SHA256(
  "KB:NamespaceResetPreflight:v1\0" || JCS(preflight_without_sha256)
)
```

步骤顺序固定为 OBJECT_DIR→MinIO→Neo4j→Redis→PostgreSQL。每一步在执行前先持久化 `started`，成功/not-found 后持久化 `done`；PostgreSQL 仍最后删除。Resume 时：若 PostgreSQL 存在则重验 receipt；若已不存在，仅当同参数/token 验证通过、checkpoint preflight hash 有效、所有先前步骤为 `done` 且 PostgreSQL 为 `started|done` 时，才把 not-found PostgreSQL 收敛为 `done`。缺 checkpoint、mapping 漂移或 hash/token mismatch 一律停止，不从残留 backend 猜 namespace。完成/失败都保留每 backend receipt。禁止删除共享 host volume、无 namespace 的 Redis DB/MinIO bucket/OBJECT_DIR root；旧 unnamespaced state 只能进入独立人工 full-clean 流程。

## 6. Retention consumer

**优先复用已有队列能力，不另建清理调度或补偿框架。** 沿用 upload-expire（`staging_id`）与 retention（`deletion_id`）两类 typed job；这两类清理消息不设 transport unique_id，每次交接使用 Oxana 原生独立 JobId，重试、延迟和进程崩溃恢复交给 Oxana，边界见 [`queue-runtime.md`](queue-runtime.md)。数据库只保存对象、引用和完成凭据等业务事实，不增加清理任务状态表、outbox 或队列扫描器。

Handler cleanup 在 [队列 §9](queue-runtime.md#9-lifecycle-与资源) 已有绝对 deadline 内完成清理交接，不等待 blob 物理删除。enqueue 未确认时保留 staging/可恢复身份并显式失败，不能因 `Skip` 或结果未知就当作交接成功。真实 upload-expire consumer 幂等释放 staging；若对象仍有其他 reference，只完成 staging 清理，不进入物理删除。需要最终回收时沿用 ObjectRegistry 的冻结 deletion identity，由独立 retention role/consumer 执行：

1. 执行前再次确认对象为 `deleting` 且不存在 reference；
2. 删除 blob 成功后原子写 tombstone/receipt 并转为 `deleted`；
3. PostgreSQL immutable deletion artifact 只表达 durable business identity，不保存队列 phase、retry、claim 或 transport backlog；
4. 发起方用该冻结 identity enqueue retention typed job；显式同一业务操作重放再次交接，由业务 identity/receipt 幂等收敛，不运行 artifact scanner；
5. Oxana 负责 handler retry/resurrection/dead queue，duplicate 通过对象状态和 receipt 幂等收敛；
6. 日志、audit 和 receipt 不记录对象内容或 secret。

`object_upload_staging` expiry 沿用 Oxana cron 每五分钟触发的到期业务处理，固定上限 100 条；超出部分由后续 occurrence 按 `expires_at,id` 继续。不新增常驻扫描进程，不扫描 pending Request 或 deletion artifact 重建队列；这不是第二套 retry 或 queue membership 状态机。

验收区分三个事实，不新增三套状态机：

- **交接已确认**：官方 enqueue 返回确认；失败/超时保留 staging 与恢复身份并报告失败，不把排队成功等同于已删除。
- **staging 已释放**：以真实 typed consumer 执行后的数据库结果证明，验证重复消费和有引用对象不被误删。
- **最终回收已完成**：对无引用、应回收的对象，在有界期限内观察真实消费结果，核对 blob 删除、`deleted` 与 tombstone/receipt。重试和崩溃恢复使用 Oxana 原生机制；不得用固定 sleep、仅看 job 成功、删掉最终回收断言或业务 handler 直接删 blob 来替代。

Oxana 2.1.3 的 unique `Skip` 返回既有 JobId，不能区分新入队与跳过；因此仅上述两类清理任务不启用 unique 过滤。官方 enqueue `Ok` 只确认本次消息入队，重复 envelope 是允许的 at-least-once 交付，不改变业务身份或增加交接计数器/状态层。其它任务的 unique/Skip 不变。

这些是同一回收链的验收层次。队列/deploy 文档引用本节，不另定义清理完成语义；隔离测试依赖缺失、消费未完成或测试资源清理失败均不能算通过。

## 7. 实施与验收

以下实现与本地验证是已存在、获准保留的增量记录，不代表整体运行验收。用户已确认方案并授权普通实施与隔离开发验证；切片状态及证据见[实施台账](../implementation-tasks.md)。当前阶段 A 只落盘任务与授权状态，不运行代码验证；采购、生产部署、现有数据操作及提交不在此次授权内。

当前 `crates/platform/src/db.rs` 已实现单事务 baseline、receipt/catalog 校验与 runtime 只读验证；旧 live 链和 V20 六张 stage 表已移除，V21 原七码、owner 与 stage fence 已修，不再列为重建任务。此前保留的实现增量已修复五条诊断 writer，并曾在新建隔离 PostgreSQL 16 完成 Content/Outline AgentRun 与 fresh/catalog 本地回归；`schema-contract` 已接线。Host CI 未触发、未提交或部署，首次发布闭环仍待实施。既有 `request_delivery_postgres` 的清理用例因未配置 Oxana Redis 失败；后续应按 [§6](#6-retention-consumer) 分层验证清理交接、真实消费后的 staging 释放和最终回收，不能仅补 Redis、立即断言删除或削弱最终回收断言来宣称解决。整体 CI/全部回归尚未绿，不删测试或弱化断言。

| 次序 | 切片状态 | 完成证据 |
| --- | --- | --- |
| 1 | 已直接修 Bidding baseline 的 outline/Content AgentRun 诊断字节边界，合同见 [queue §8.1](queue-runtime.md#81-agentrun-诊断字节边界) | 原 baseline 中文 CHECK 失败与修后真实 procedure 回归；保留既有 NULL 拒绝、错误码和执行围栏 |
| 2 | 已接入独立 `schema-contract` 与现有 AgentRun 必跑检查；正确性验收仍须补齐 §6 的清理交接/消费/最终回收证据，见下节 | fresh、只读重放、漂移拒绝、角色边界及字节回归已有局部证据；清理链须有真实 consumer、有引用保护及失败恢复证据，不把局部验证称为 hosted CI 已绿 |
| 3 | 待补齐首次发布的可执行检查 | 按 [部署说明](../../deploy/README.md) 落位 release/reset 工具，验证 descriptor、实际 RepoDigest、migrator 先于 runtime 与 readiness；不另建发布框架 |

pgvector 性能基准不列入当前计划（禁止项见 [`docs/knowledge-base/crate.md`](../../docs/knowledge-base/crate.md)），不增加外部向量库，也不是修 baseline 或 ONLYOFFICE O0 的前置。完整 release 自动化同样不是这些开发切片的前置，但首次正式发布仍须满足下列全部安全验收。招投标复用现有 worker 与[领域接缝](../../docs/bidding/authoring.md)，保持 [ONLYOFFICE O0 → O4](../bidding/onlyoffice-integration.md)，不先重做平台。

### 7.1 聚焦数据库 gate

当前 `.github/workflows/ci.yml` 已用 PostgreSQL+pgvector 执行 baseline，`scripts/bidding_v2_content_stack_e2e.sh` 也使用 Rust migrator；新增独立 `schema-contract` job 复用现有 fresh/catalog gate，使用单独 PG 服务与专用数据库，并作为 images job 的依赖。原 rust job 的 AgentRun/Request delivery 步骤保留；其独立既有失败仍会阻止 CI 放行。

- 已接入现有 `scripts/fresh_schema_acceptance.sh`，遵守 [部署说明的 bootstrap/角色前置](../../deploy/README.md#健康与验收)。用真实 Rust migrator 验证空库三 baseline、同身份重跑无 DDL/receipt 改写、receipt/catalog 漂移拒绝、角色 allow/deny 与 API/Worker/Retention 实际登录的共享 `schema-verifier` 只读校验；这不等于完整应用及其外部依赖的启动验收，不只运行静态 SQL 字符串断言。
- `crates/platform/tests/catalog_manifest_postgres.rs` 必须显式设置 `KNOWLEDGEBRAIN_CATALOG_TEST_DATABASE_URL` 与已有 `KNOWLEDGEBRAIN_REQUIRE_CATALOG_POSTGRES_TEST=1`，执行 `cargo test --locked -p platform --test catalog_manifest_postgres`。该测试族只允许 `127.0.0.1:25433/knowledgebrain_test_*` 的隔离 PostgreSQL 16；不能只设置其它测试族的 require flag。Bidding 字节回归接入现有 `tender_analysis_postgres` 与 `content_agent_run_postgres` 测试族，构建/测试继续要求 `--locked`。
- 所有上述验证只在授权、本轮创建的隔离环境执行；缺 DSN/服务、ignored/skipped suite、零实际执行或清理失败均使 gate 失败。记录真实退出码、执行用例与依赖；清理仅限本轮创建资源，不触碰开发既有数据面或共享卷。

### 7.2 平台完成证据

至少包括：

- 空库按 Shared→Knowledge→Bidding 建立、matching receipt 重放、server/extension/seed/catalog digest；CatalogManifestV1 golden 覆盖每 kind、child owner inheritance、nested ACL/dependency order；
- API/worker/retention/migration/app-owner role allow/deny，owner/ACL/membership drift fail-closed；
- 应用重复启动只读验证 schema snapshot，不执行 DDL；
- object key、digest、MIME/bytes、owner scope 和 reference 一致性；
- 有引用拒绝删除、释放后删除、并发 add-reference/delete 竞态；
- retention worker crash由Oxana resurrection恢复、响应丢失、retry/dead revive与幂等 receipt；
- [`queue-runtime.md`](queue-runtime.md) 定义的版本、retry、resurrection 和 shutdown 验收；
- ReleaseDescriptor schema/hash、mount/env/receipt、每组件实际 RepoDigest inspect mismatch 均 fail-closed；
- namespace reset 在每个 `started`/delete/`done` 边界注入 crash；尤其 PostgreSQL drop 后凭 fsynced preflight checkpoint 收敛，缺失/篡改 checkpoint 拒绝 resume；
- `rg` 与 catalog denylist 证明旧 refcount、公开删除函数和 runtime repair 已不存在；
- Compose 空对象卷下的上传、读取、引用保护和最终回收实测；
- 每次测试结束立即清理本轮 container、volume、network 和临时 image并断言零残留。

implemented、locally verified、committed、pushed、deployed 和 runtime accepted 必须分别报告。
