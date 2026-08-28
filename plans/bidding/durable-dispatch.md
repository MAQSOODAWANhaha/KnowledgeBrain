# 招投标异步任务投递方案

| 项 | 值 |
| --- | --- |
| 状态 | 已批准；按 Oxana 原生能力简化方案实施 |
| 所有者 | Bidding |
| 队列依赖 | [`../platform/queue-runtime.md`](../platform/queue-runtime.md) |

本文只定义招投标业务 target 如何调用 Oxana。Oxana 负责 enqueue、retry、retry delay、并发消费、worker crash resurrection 和 dead queue；招投标不复制这些能力。

## 1. 目标与取舍

- 六类现有业务 target 继续表达“要完成什么”和“最终结果是什么”；
- 所有 transport 生命周期完全交给 Oxana 2.1.3；
- duplicate、旧业务 revision 和重复执行不能重复发布结果；
- conversion -> extraction、matching schedule -> 0..N jobs 等 successor 仍由一个业务事务原子创建；
- 删除旧 `system:live-recovery:v1`、best-effort enqueue 和复杂 dispatch/recovery 草稿。

V1 明确不做：Redis 不可用时仍返回成功、Redis volume 删除后的自动 DB 重建、后台扫描 pending target、通用工作流引擎或 transport exactly-once。enqueue 失败返回可重试错误；Redis 恢复后重试原幂等 operation。

## 2. 最小业务模型

不新增 `dispatch head`、`dispatch intent`、`delivery generation`、`delivery attempt`、`settlement`、`successor` 或 `repair obligation` 表。现有六类 target 只保留业务字段：

```text
id
status                 pending | completed | failed | cancelled | superseded
business generation / watermark / revision
frozen input identity
result/artifact pointer nullable
last_error_code/detail nullable bounded text
created_at, completed_at nullable
```

约束：

- `pending` 表示业务意图尚未终结，不区分 queued、running 或 retrying；
- Oxana job phase、retry counter、dead state 不写入 PostgreSQL；
- target 不保存 `delivery_generation`、`next_enqueue_at`、`claim_token`、`claim_lease_ms` 或 `heartbeat_at`；
- final publish 必须校验 target 的 business generation/watermark/revision 和所属项目 current 状态；
- immutable artifact unique key、current pointer CAS 和 ObjectRegistry owner reference 保证重复执行安全；
- target、audit、幂等 receipt 和 successor targets 在所属业务事务内原子提交。

## 3. 唯一流程

```text
幂等业务事务创建或取得 pending target
        |
        | commit 后 enqueue 一次
        v
Oxana 2.1.3 bid:delivery:v1
        |
        | retry / delay / resurrection / dead queue
        v
handler 读取冻结业务 target
        |
        | 外部工作
        v
current/revision fenced transaction 原子 publish
        |
        | 若有 successor，commit 后逐个幂等 enqueue
        v
完成；enqueue 失败则由当前 Oxana job 重试重放
```

### 3.1 API 创建 target

API 使用调用方 idempotency key：

1. 同一 transaction 创建 target、业务 mutation、audit 和业务 receipt；receipt只返回稳定target identity，不声明queue accepted；
2. commit 后调用一次 `enqueue(target_kind,target_id,target_revision)`；
3. accepted 返回 endpoint 的既有成功状态与稳定 target ID；纯任务为 `202`，同步创建资源并附带异步 target 的 endpoint 可保持 `201`；
4. unavailable返回`503 QUEUE_UNAVAILABLE`、稳定target ID和`retry_same_idempotency_key=true`；客户端必须用同一key重试，命中receipt后仍执行第2步，不重复业务写。

不新增 outbox、due 字段或 reconciler 来掩盖 Redis 不可用。

### 3.2 Handler

payload 只有 `target_kind + target_id + target_revision`。handler 在任何外部 I/O 前读取 target：

| target kind | `target_revision` |
| --- | --- |
| `document_conversion` | `conversion_generation` |
| `extraction_target` | `extraction_generation` |
| `matching_schedule` | `mutation_watermark` |
| `matching_job` | 冻结 manifest generation |
| `attachment_preparation` | attachment revision |
| `submission_render` | immutable render target 的固定 revision `1` |

- target 不存在、已取消/取代或 revision 已旧：`Ok` noop；
- target 已完成且没有 successor 待投递：`Ok` noop；
- target 已完成且有确定性 successor：只重放 successor enqueue；
- target pending 且 current：执行冻结输入对应的业务工作；
- 确定性输入/业务错误：原子置 `failed` 后 `Ok`；
- 可重试外部错误或领域固定 timeout：记录 bounded error 后 `Err`，由 Oxana 决定 retry/dead。

handler 不 claim、不续 lease，也不在 PostgreSQL 计算 retry。Oxana unique job 和 process resurrection负责正常单 job 执行；极端重复 owner只能竞争最终 CAS，loser 不得覆盖结果。

### 3.3 Successor

conversion 完成并创建 extraction target、matching schedule 创建 manifest 与 0..N matching job、attachment pages 完成以及 render output 发布，都必须在各自成功 transaction 中原子完成。不能先终结父 target，再用第二次数据库调用补建 child target。

success transaction 返回确定性 child target 列表。commit 后逐个 enqueue：

- 全部 accepted/Skip：父 handler 返回 `Ok`；
- 任一 unavailable：父 handler 返回 `Err`；
- 父 job 重试时检测父结果已经提交，只重放完全相同的 child enqueue；
- child unique ID 固定，因此 response lost 和部分成功不会产生重复 child job。

零 route 是成功的空 successor 集合，不创建占位 job。

## 4. Oxana 与业务事务如何配合

| 场景 | 收敛方式 |
| --- | --- |
| handler 返回可重试错误 | Oxana 原生 retry/delay |
| worker 进程崩溃 | Oxana process heartbeat 与 resurrection |
| duplicate enqueue/response lost | unique ID + `Skip`；final publish CAS |
| retry 耗尽 | Oxana dead queue；target保持`pending`和last error，修复外部原因后用oxana-web `revive_all_dead` |
| API enqueue 失败 | 返回 `503`；同一 idempotency key 重试相同 target |
| successor enqueue 部分失败 | 父 job retry 只重放相同 child enqueue |
| 旧业务 revision job 到达 | target/current revision 校验后 noop |
| Redis volume 被删除 | 从基础设施备份恢复，或人工重试原业务 operation；V1 不自动 DB 扫描重建 |

这套设计保证业务发布幂等，不声称 PostgreSQL 与 Redis 跨库原子或 transport exactly-once。

## 5. Matching 大结果

Matching 的 Open/Stage/Commit 只解决单次大 artifact 不能放入一个 JSON/transaction 的业务问题，不承担队列恢复：

- staging set 绑定 immutable matching target ID、route ID、Oxana job ID 和本次 report nonce；
- Oxana retry/resurrection 复用同一 unique job ID，但每次执行先原子abandon该job未消费staging，再用新report nonce从冻结输入重算；不恢复部分provider输出；
- batch ordinal、canonical hash、配额和 final report CAS 保持幂等；
- staging TTL 只回收未提交临时数据，不触发 enqueue、不改变 retry 次数；
- 不需要 claim token、lease heartbeat 或 reaper 接管 job。

## 6. 需要删除

| 旧内容 | 处理 |
| --- | --- |
| `system:live-recovery:v1` envelope、claim ledger、handler | 删除 |
| commit 后忽略错误的 `enqueue_bid_*` | 改为显式 accepted/`503`/`Err` 合同 |
| dirty-manifest/orphan-target/orphan-match recovery UNION | 删除 |
| delivery generation、next enqueue、due reserve/reconciler | 删除 |
| delivery claim、lease、heartbeat、attempt counter/reaper | 删除 |
| Bid 对 `oxanus:*`、hostname/PID replay 的 correctness 依赖 | 删除 |
| async base/extensions/head/intent/state 等复杂 dispatch 草稿 | 删除 |
| successor/gate/governor/activation hold/独立 dispatcher service | 删除 |

只删除被本方案替代的招投标路径；知识库现有 consumer 不在本轮顺手重构，但后续新增 consumer 必须遵守平台队列方案。

## 7. 实施顺序

1. 固化 [`../platform/queue-runtime.md`](../platform/queue-runtime.md) 的 Oxana 2.1.3 合同；
2. 将统一 job payload 收敛为 target kind/ID/business revision，删除 delivery/recovery 字段与接口；
3. API target 创建改为“幂等 transaction -> 单次 enqueue -> accepted/503”；
4. 逐类切换 conversion/extraction、matching、attachment/render；successor 使用“原子创建 -> 父 job retry 重放 enqueue”；
5. Matching staging 改为 target/job identity 幂等，不再依赖 claim/lease heartbeat；
6. 删除全部 Bid live-recovery、reconciler、private Redis correctness 和复杂 dispatch 草稿；
7. 从空库执行最终验收并清理所有测试容器资源。

## 8. 验收矩阵

1. Oxana retry、10 秒 delay、resurrection、Skip、dead queue 和 `revive_all_dead` 使用发布版原生实现；
2. API enqueue 失败返回 `503`，同一 idempotency key 重试不重复 target 或业务 mutation；
3. duplicate job 和旧 revision job不能重复/覆盖业务结果；
4. 任意 final publish 都有 current/revision CAS；
5. conversion -> extraction 与 matching -> 0..N jobs 在数据库中原子创建；
6. child enqueue response lost/部分失败后，父 job retry 只重放相同 child unique job；
7. matching retry复用同一target/job identity、先清旧staging再重算，TTL cleanup不enqueue；
8. 外部调用 timeout 进入同一 Oxana retry，handler退出时无遗留子进程；
9. `rg`、registry 和 catalog denylist 证明 live-recovery、delivery generation、due reconciler、claim/lease heartbeat、private Redis correctness 和复杂 dispatch 草稿已删除；
10. 所有活库/Compose 测试完成后，本轮 container、volume、network 和临时 image 为零。

额外静态门禁：后续任何异步 consumer 若新增 delivery attempt/generation、pending target 自动重投、queue membership、retry schedule、dead queue、resurrection、claim lease heartbeat，或读取 Oxana 私有 Redis key，直接判定为方案违规。若确有新的业务一致性需求，必须先证明它不是 Oxana transport 能力，再单独修改方案评审。

验收分别报告本地测试、已提交、已推送、已部署和 runtime accepted；不得用其中一个状态代替另一个。
