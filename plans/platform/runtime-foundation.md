# 共享平台运行时基础方案

| 项 | 值 |
| --- | --- |
| 状态 | 招投标 clean-slate V1 平台依赖方案已批准并固化，待最终验收 |
| 所有者 | Shared Platform |
| 消费方 | 知识库、招投标 |

本文是 fresh baseline、共享 actor/idempotency/audit、`ObjectRegistry` 和 retention 的唯一活动定义。队列能力由 [`queue-runtime.md`](queue-runtime.md) 定义；业务领域只定义自己的 target、业务引用和消费门禁。

## 1. 所有权边界

共享平台拥有：

- baseline manifest、checksum ledger、extension、角色与启动期 schema identity 校验；
- authenticated actor、共享幂等 intent/receipt 和 append-only audit envelope；
- `ObjectRegistry`、owner reference、retention outbox/tombstone 与物理删除 consumer；
- Oxana 版本、queue registry 和 worker runtime；
- maintenance gate、健康检查、日志、tracing、metrics 与部署验收基础。

业务领域拥有：

- 业务 target、status、generation、claim、lease 和结果；
- artifact、current pointer、operation 和 payload；
- Redis 完全丢失时从 durable target 发起的兜底投递。

平台和业务都不得复制 Oxana 的 retry、resurrection、queue membership 或 dead-job 状态机。

## 2. Fresh baseline manifest

最终系统只有一套能从空 PostgreSQL 建立完整 catalog 的 baseline manifest。逻辑上按所有权组织并由固定 checksum ledger 排序：

```text
knowledge_base_baseline.sql   # 现有知识库语义，重排不改业务
shared_platform_baseline.sql  # 本文拥有的运行时基础
bidding_v1_baseline.sql       # 最终招投标 V1
```

必须满足：

1. 不先创建旧投标表再通过 ALTER、backfill 或 runtime repair 到目标形态；
2. migration runner 只执行固定 manifest，应用启动只验证 schema identity；
3. extension、seed contract、current pointer、role/grant/revoke 进入 manifest 与 checksum；
4. CI 和 Compose first launch 都从空库验证 catalog allowlist/denylist；
5. API、worker、retention 和 migration 使用现有独立最小权限角色。

fresh migration 在一个 main transaction 中使用 `SET LOCAL ROLE kb_launch_owner`。commit 后、handoff 前必须重新证明：

```text
session_user = kb_migrator
current_user = kb_migrator
current_setting('role') = none
```

active role、catalog checksum 不匹配或旧 schema 残留时 fail closed。V1 不为首次启动另建复杂 activation 状态机、候选 release 协议或专用 dispatcher role。

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
- worker 的显式 shutdown、SIGINT 和 SIGTERM 汇聚到一个 cancellation token；
- handler 子进程与 heartbeat 使用同一 scope，退出时 cancel，最多等待 5 秒，随后 kill process group、wait/reap 并 join；
- 不增加独立 `bid-dispatcher` service、DSN、credential 或 activation hold；轻量 due reconciler运行在现有 worker 中。

## 5. ObjectRegistry

平台唯一对象标识为：

```text
object_ref = objects/<64-lowercase-hex>
digest, media_type, byte_length
state = available|deleting|deleted
owner references
retention outbox/tombstone
```

规则：

- object key 只接受 canonical 形式；绝对路径、`..` 和 alias 全部拒绝；
- 业务通过受检接口注册/移除 owner reference、读取 available 对象，不维护第二套 refcount；
- 写物理 bytes 前创建有时限的 staging owner reference，业务事务原子转移给最终 owner；
- crash 遗留 staging 由 retention expiry 回收；
- 任一 reference 存在时不得进入 `deleting`；
- 普通 API/worker 无物理删除权限；
- 同 digest 的 `deleted` 对象在 V1 拒绝复活；
- 旧 `content_objects.ref_count` 与公开 bump/release/delete/drop 旁路删除，不留 alias 或双写。

## 6. Retention consumer

物理删除由独立 retention role/consumer 执行，投递、失败重试和进程崩溃恢复复用 [`queue-runtime.md`](queue-runtime.md)：

1. claim 时再次确认对象为 `deleting` 且不存在 reference；
2. 删除 blob 成功后原子写 tombstone/receipt 并转为 `deleted`；
3. PostgreSQL outbox row是durable删除intent，claim token/lease只防止旧owner发布错误结果；
4. Oxana负责handler retry/resurrection，Redis全丢时由outbox due scan重新enqueue；
5. duplicate或lease lost不得重复释放业务reference，也不得删除仍有引用的对象；
6. 日志、audit 和 receipt 不记录对象内容或 secret。

`object_upload_staging` expiry 每五分钟处理固定上限 100 条；超出部分由后续 tick 按 `expires_at,id` 继续。它只负责发现到期业务intent，不实现第二套retry或queue membership状态机。

## 7. 实施与验收

PR1 建立 baseline manifest、actor/idempotency/audit、`ObjectRegistry`、通用角色/ACL 和 seed；业务 PR 随后接入。招投标异步任务复用现有 worker role和服务，具体合同见 [`../bidding/durable-dispatch.md`](../bidding/durable-dispatch.md)。

平台完成证据至少包括：

- 空库重复建立、checksum、extension、seed 与 catalog allow/deny；
- API/worker/retention/migration role allow/deny；
- 应用重复启动只验证 schema，不执行 DDL；
- object key、digest、MIME/bytes、owner scope 和 reference 一致性；
- 有引用拒绝删除、释放后删除、并发 add-reference/delete 竞态；
- retention crash、lease reclaim、响应丢失、重试与幂等 receipt；
- [`queue-runtime.md`](queue-runtime.md) 定义的版本、retry、resurrection 和 shutdown 验收；
- `rg` 与 catalog denylist 证明旧 refcount、公开删除函数和 runtime repair 已不存在；
- Compose 空对象卷下的上传、读取、引用保护和最终回收实测；
- 每次测试结束立即清理本轮 container、volume、network 和临时 image并断言零残留。

implemented、locally verified、committed、pushed、deployed 和 runtime accepted 必须分别报告。
