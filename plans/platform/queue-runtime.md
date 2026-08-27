# 共享队列运行时方案

| 项 | 值 |
| --- | --- |
| 状态 | Oxana 2.1.3 简化方案已批准并固化，待实施与验收 |
| 所有者 | Shared Platform |
| 发布依赖 | crates.io `oxana = "=2.1.3"`、`oxana-web = "=2.1.3"` |
| 消费方 | 招投标异步任务、retention；知识库现有任务 |

本文只定义队列运行时。业务 target、业务状态、claim/lease 和最终发布由所属领域定义。不得在 PostgreSQL 中复制 Oxana 的队列状态机。

## 1. 决策

V1 直接使用 crates.io 发布的 Oxana 2.1.3：

- `Cargo.toml` 精确锁定 `=2.1.3`；
- `Cargo.lock` 是版本、source 和 checksum 的唯一真源；
- 构建、测试和 CI 使用 `cargo --locked`；
- 不 fork、不 vendor、不使用 Git/path dependency、`[patch.crates-io]` 或 Cargo source replacement；
- 升级 Oxana 时单独评审并重跑本文件验收，不为依赖版本再造 promotion 协议。

## 2. 职责边界

| 能力 | 唯一所有者 |
| --- | --- |
| enqueue、队列存储、并发消费 | Oxana |
| handler 失败重试与 retry delay | Oxana |
| worker 进程失联后的 in-flight resurrection | Oxana |
| unique job 冲突处理 | Oxana |
| 业务任务是否仍有效、是否已完成 | PostgreSQL 业务 target |
| worker claim、长任务 heartbeat、lease fencing | PostgreSQL 业务 target |
| 最终 artifact/current pointer 的原子发布 | PostgreSQL 业务事务 |
| Redis 数据完全丢失后的兜底重新投递 | 业务 target reconciler |

明确禁止：

- 自建 delivery attempt、queue membership、dead queue、retry schedule 或 resurrection 状态机；
- 读取或修改 `oxanus:*` 私有 Redis key；
- 用 `get_job`、queue list、stats 或 Redis membership 证明业务完成；
- 在数据库中镜像 queued/processing/retrying/dead 等 Oxana phase；
- 在 transport adapter 内部隐藏 retry、probe、delete 或 repair。

该规则适用于后续新增的所有异步 consumer。确需 PostgreSQL target 的理由只能是业务原子性、claim fencing、幂等 publish 或 Redis 全丢恢复，不能是重新实现队列已有能力。

## 3. 发布合同

招投标只注册一个稳定 typed job：

```text
name       = bid:delivery:v1
payload    = { target_id, delivery_generation }
unique_id  = <target_id>:<delivery_generation>
on_conflict = Skip
resurrect   = true
```

payload 只负责定位 PostgreSQL target，不携带业务 snapshot、对象内容、claim token 或执行结果。worker 必须从 PostgreSQL 重新读取当前业务事实。

平台 adapter 只暴露一个小 interface：

```text
enqueue(target_id, delivery_generation)
  -> accepted(job_id)
   | indeterminate(error)
```

一次调用最多执行一次 `Storage::enqueue`。`accepted` 只表示 Oxana 接受调用结果，不等于业务成功；`indeterminate` 也不能证明 Redis 没有写入。两种结果都由业务 target 的下一次到期扫描自然收敛。

## 4. Worker 配置

发布版固定：

```text
max_retries = 3
retry_delay = 10 seconds
resurrect = true
on_conflict = Skip
```

这里的 `max_retries=3` 表示初次执行失败后最多由 Oxana 再重试三次。业务 handler 按结果返回：

| handler 结果 | PostgreSQL 动作 | 返回 Oxana |
| --- | --- | --- |
| 成功 | fenced transaction 原子发布并终结 target | `Ok` |
| stale generation、duplicate、已 terminal | 不修改业务结果 | `Ok` |
| 确定性业务失败 | 原子记录 terminal failure | `Ok` |
| 可重试外部错误 | 释放本次 claim并记录 bounded error | `Err` |
| lease 已丢失 | 禁止发布 | `Ok` |

Oxana 负责同一 delivery generation 的短期重试。只有任务长期未终结、Redis volume 被清空或 Oxana 重试已耗尽时，PostgreSQL reconciler 才创建新的 `delivery_generation` 再次 enqueue；它不查询 Oxana 私有状态。

## 5. 生命周期与资源

- worker shutdown 使用一个 cancellation token 汇聚显式 shutdown、SIGINT 和 SIGTERM；
- handler 启动的 heartbeat 和外部子进程必须归属同一 task scope；
- 退出顺序固定为 cancel，等待最多 5 秒，仍未退出则 kill process group，随后 wait/reap 并 join background task；
- Redis/PostgreSQL/Compose 测试结束后立即清理本轮 container、volume、network 和临时 image，并断言零残留。

## 6. 验收

只保留能证明上述接口的关键测试：

1. `cargo metadata --locked` 证明 Oxana 2.1.3 来自 crates.io 且无覆盖；
2. 活 Redis 证明失败 handler 实际执行初次加三次 retry，间隔使用 10 秒策略；
3. kill worker 进程后，`resurrect=true` 的 in-flight job 被发布版 runtime 重新消费；
4. 同一 unique ID 重复 enqueue 使用 `Skip`，业务结果仍只发布一次；
5. transport 不读取私有 Redis key，也不实现第二套 retry/resurrection；
6. success、failure、timeout、SIGINT 和 SIGTERM 路径均完成资源清理并零残留。

队列测试只证明队列行为；业务幂等、generation fencing、lease lost 和 Redis 全丢恢复由 [`../bidding/durable-dispatch.md`](../bidding/durable-dispatch.md) 验收。
