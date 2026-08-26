# 共享平台运行时基础方案

| 项 | 值 |
| --- | --- |
| 状态 | 招投标 clean-slate V1 的当前平台依赖方案 |
| 所有者 | Shared Platform |
| 消费方 | 知识库、招投标 |

本文是 fresh baseline 编排、共享 actor/idempotency/audit、`ObjectRegistry` 和 retention 内部协议的唯一活动实施定义。队列 transport 另由 [`queue-runtime.md`](queue-runtime.md) 定义；业务领域只定义自己的聚合、durable intent、业务引用和消费门禁，不复制平台表、队列内部实现或对象生命周期。

## 1. 所有权边界

共享平台拥有：

- baseline manifest、checksum ledger、extension、角色与启动期 schema identity 校验；
- authenticated actor identity、共享幂等 intent/receipt 和 append-only audit envelope；
- `ObjectRegistry`、owner reference、retention outbox/tombstone 与物理删除 consumer；
- queue registry 与 [`WorkTransport`](queue-runtime.md)；业务 claim/lease/heartbeat/reaper 归所属领域；
- maintenance gate、健康检查、日志、tracing、metrics 与部署验收基础。

知识库和招投标分别拥有自己的表、artifact、current pointer、operation 名称和 payload。Open/Stage/Commit 若被某个 adapter 使用，仍是该 adapter 的内部提交协议，不属于本平台接口。

## 2. Fresh baseline manifest

最终系统只有一套能从空 PostgreSQL 建立完整 catalog 的 baseline manifest。逻辑上按所有权组织并由固定 checksum ledger 排序：

```text
knowledge_base_baseline.sql   # 现有知识库语义，重排不改业务
shared_platform_baseline.sql  # 本文拥有的运行时基础
bidding_v1_baseline.sql       # 最终招投标 V1
```

物理文件是否合并不是领域决策，但必须满足：

1. 不先创建旧投标表再通过 ALTER、backfill 或 runtime repair 到目标形态；
2. migration/schema runner 只执行固定 manifest，应用启动只验证 schema identity；
3. extension、seed contract、current pointer、role/grant/revoke 全部进入 manifest 与 checksum；
4. CI 和 Compose first launch 都从空库验证 catalog allowlist/denylist；
5. API、worker、retention、migration 使用独立最小权限角色。

业务 baseline slice 由所属领域定义；平台只拥有 manifest 编排、共享表和权限边界。

## 3. Actor、幂等与审计

平台 actor identity 固定为：

```text
user:<lowercase-uuid>
api_key:<lowercase-uuid>
system:<allowlisted-bounded-name>
```

`user` 与 `api_key` 即使 UUID 相同也不是同一 actor。Bootstrap 只允许启动管理，不能伪造任何需要人工确认的业务决定。

共享幂等 identity 固定为：

```text
scope = actor_identity + operation + idempotency_key
request_identity = schema_version + exact operation payload + payload_sha256
```

平台接口保证：同 key 同 hash 逐字返回首次 completed receipt；同 key 不同 hash 返回 `IDEMPOTENCY_PAYLOAD_MISMATCH`；瞬时 DB/网络失败回滚且不写伪 completed receipt。领域 mutation 必须把领域写、revision/digest、current/stale、audit 与 completed receipt 放在同一事务。heartbeat/lease renew 只可按明确的 claim-token CAS 协议豁免 receipt，不得扩大到普通 mutation。

audit envelope 至少冻结 operation、actor、request/response identity、before/after revision+digest、entity locator 和 UTC 时间；append-only 历史不得 UPDATE/DELETE。

## 4. ObjectRegistry

平台唯一对象标识为：

```text
object_ref = objects/<64-lowercase-hex>
digest, media_type, byte_length
state = available|deleting|deleted
owner references
retention outbox/tombstone
```

规则：

- object key 只接受上述 canonical 形式；绝对路径、`..` 和 alias 全部拒绝；
- 业务通过受检接口注册/移除 owner reference、读取 available 对象，不维护第二套 refcount；
- API/worker 写物理 bytes 前必须先创建有时限的 `object_upload_staging` owner reference；业务事务通过平台内部接口把该 reference 原子转移给最终 owner，CAS/校验失败则显式 abandon 并进入 retention，进程崩溃遗留 staging 由独立 retention-owned expiry 回收；
- 对象仍被任何 reference 引用时不得进入 `deleting`；
- 普通 API/worker 无物理删除权限，不能直接修改 registry、outbox 或 tombstone；
- 同 digest 的 `deleted` 对象在 V1 稳定拒绝复活；
- `content_objects.ref_count` 和公开 `bump/release/delete/drop` 旁路在 cutover 时删除，不保留 view、alias 或双写。

业务表可以保存受检 `object_ref` 与自己的 placement、occurrence、attachment 等元数据，但对象状态与引用真源只在平台。

## 5. Retention consumer

物理删除由独立 retention role/consumer 执行。平台定义 claim token、heartbeat、lease、reclaim、retry/backoff 和幂等 receipt：

1. claim 时再次确认对象为 `deleting` 且不存在 reference；
2. 删除 blob 成功后原子写 tombstone/receipt 并转为 `deleted`；
3. consumer crash、响应丢失或超时后可由 lease reclaim；
4. 重试不得重复释放业务 reference，也不得把仍有引用的对象删除；
5. 日志、audit 和 receipt 不记录对象内容或 secret。

`object_upload_staging` expiry 同样只由独立 retention service 执行；每个五分钟 tick 只运行一次固定上限 100 条的数据库批次。超过一批的 backlog 保持为 expired staging，由后续 tick 按 `expires_at,id` 顺序继续回收。API、worker 和 maintenance lane 均不得获得该 expiry 权限。

业务消费验收只证明自己的 reference 创建、替换、释放和读取行为；claim/lease/retry 状态机由平台测试证明。

## 6. 实施与验收

PR1 建立 baseline manifest、actor/idempotency/audit、`ObjectRegistry`、角色/ACL 和 seed；各业务 PR 在此后接入。招投标 PR6 只负责 Submission consumer cutover 和删除旧 refcount/direct-delete 路径，不重新实现 retention。

平台完成证据至少包括：

- 空库重复建立、checksum、extension、seed 与 catalog allow/deny；
- API/worker/retention/migration role allow/deny；
- 应用重复启动只验证 schema，不执行 DDL；
- object key、digest、MIME/bytes、owner scope 和 reference 一致性；
- 有引用拒绝删除、释放后删除、并发 add-reference/delete 竞态；
- consumer crash、lease reclaim、响应丢失、重试与幂等 receipt；
- 队列 registration、transport outcome、boot instance identity 与 in-flight resurrection 使用 [`queue-runtime.md`](queue-runtime.md) 的验收证据；
- `rg` 与 catalog denylist 证明旧 refcount、公开删除函数和 runtime repair 已不存在；
- Compose 空对象卷下的上传、读取、引用保护和最终回收实测。

本地测试、提交、部署和真实运行验收必须分别报告。
