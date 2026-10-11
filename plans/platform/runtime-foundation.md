# 共享平台运行时基础方案

> 本文数据库身份与初始化方式已按[配置收敛方案](configuration-simplification.md)更新：fresh 部署使用一套非超级用户应用账号；不处理存量迁移。

| 项 | 值 |
| --- | --- |
| 状态 | 方案已确认；普通实施与隔离开发验证已授权，任务见[实施台账](../implementation-tasks.md) |
| 所有者 | Shared Platform |
| 消费方 | 知识库、招投标 |

本文是 fresh baseline、共享 actor/idempotency/audit、`ObjectRegistry` 和 retention 的唯一活动定义。队列能力由 [`queue-runtime.md`](queue-runtime.md) 定义；业务领域只定义自己的 target、业务引用和技术消费校验。招投标产品目标见 [PRD](../../docs/bidding/README.md) 和 [ONLYOFFICE](../../docs/bidding/onlyoffice.md)，来源/身份接缝见 [领域契约](../../docs/bidding/authoring.md)；不以业务检查阻断编辑/导出。旧 first-launch/intended-state/verifier 与 V1 slice 已删除；最终系统使用三份 clean baseline，不设置运行时数据库结构版本或摘要门禁。

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
3. 单一应用账号拥有 public application catalog，并执行 baseline；该账号没有超级用户、建库、建角色、复制或 BYPASSRLS 权限，数据库仍由引擎管理员拥有；
4. 三 baseline 在独立初始化进程的同一事务内执行，不生成结构 hash 或 schema snapshot；
5. 不建立 catalog manifest，不在运行时比较 schema 版本或结构摘要；
6. runtime readiness 检查数据库和所需运行依赖，不检查数据库结构版本或 hash，绝不 DDL/repair；
7. 仅支持新空库安装；不兼容数据库不自动迁移、修复或清理；
8. 初始化器、API、worker、retention 使用同一应用登录身份。

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

Production 发布入口由 [`../../deploy/README.md`](../../deploy/README.md) 的 `knowledgebrain-release` 定义。发布 descriptor 只负责整组镜像 identity 与实际 RepoDigest 验证，不携带数据库 schema revision，不向应用注入结构校验环境变量或挂载数据库 receipt。运行时依赖就绪与发布镜像验证分别执行。

## 5. ObjectRegistry

平台唯一对象标识为：

```text
object_ref = objects/<64-lowercase-hex>
digest, media_type, byte_length
state = available|deleting|deleted
owner references
immutable deletion artifact/tombstone
```

规则：

- object key 只接受 `objects/<digest>` 规范形式；绝对路径、`..` 和 alias 全部拒绝；
- 业务通过受检接口注册/移除 owner reference、读取 available 对象，不维护第二套 refcount；
- 写物理 bytes 前创建有时限的 staging owner reference，业务事务原子转移给最终 owner；
- crash 遗留 staging 由 retention expiry 回收；
- 任一 reference 存在时不得进入 `deleting`；
- 普通 API/worker 无物理删除权限；
- 同 digest 的 `deleted` 对象在 V1 拒绝复活；
- 旧 `content_objects.ref_count` 与公开 bump/release/delete/drop 旁路删除，不留 alias 或双写。

### 5.1 独立部署资源

每个安装使用独立的 PostgreSQL 数据库、Redis 数据库、对象存储 bucket 和 OBJECT_DIR；启用图谱时使用独立 Neo4j 数据库。业务 key 保留文档、版本与任务身份，不再额外派生部署身份。路径 containment、符号链接拒绝和 ObjectRegistry 引用保护继续有效。仅支持新空库安装，不提供旧布局兼容、迁移或跨后端 reset 编排。

## 6. Retention consumer

**优先复用已有队列能力，不另建清理调度或补偿框架。** 沿用 upload-expire（`staging_id`）与 retention（`deletion_id`）两类 typed job；这两类清理消息不设 transport unique_id，每次交接使用 Oxana 原生独立 JobId，重试、延迟和进程崩溃恢复交给 Oxana，边界见 [`queue-runtime.md`](queue-runtime.md)。数据库只保存对象、引用和完成凭据等业务事实，不增加清理任务状态表、outbox 或队列扫描器。

Handler cleanup 在 [队列 §9](queue-runtime.md#9-lifecycle-与资源) 已有绝对 deadline 内完成清理交接，不等待 blob 物理删除。enqueue 未确认时保留 staging/可恢复身份并显式失败，不能因 `Skip` 或结果未知就当作交接成功。真实 upload-expire consumer 幂等释放 staging；若对象仍有其他 reference，只完成 staging 清理，不进入物理删除。需要最终回收时沿用 ObjectRegistry 的冻结 deletion identity，由retention consumer（共享应用数据库账号） 执行：

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

Fresh 初始化保留三份 SQL 的顺序、事务和单一非超级用户身份。`schema-contract` CI job 验证新空库 DDL 与账号权限，不执行旧 catalog snapshot/版本/hash gate。API、Worker 和 Retention 启动只连接依赖，不运行 schema 修补或迁移。

验收继续覆盖业务 actor、幂等、ObjectRegistry 引用保护、Retention 实际消费、失败重试及资源清理。发布镜像 identity 与业务工件 hash 各有独立用途，不属于数据库结构启动门禁。

implemented、locally verified、committed、pushed、deployed 和 runtime accepted 必须分别报告。
