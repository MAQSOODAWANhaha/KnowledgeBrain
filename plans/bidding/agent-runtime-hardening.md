# Agent Runtime 硬化：取消生命周期、合同测试与无业务偏移

## Context

`crates/bidding/src/agent_runtime/` 是提取/编制共用的 **I/O 三边界运行层**，不是通用 Agent 框架。评审结论：

- 主干合同成立：预约后原字节发送、完整 `tool_calls` 才执行、Journal v3、AgentRun 窗口、Progress 原语。
- 真实提取/复核/整稿失败主要在领域策略，不在缺第二套 runtime。
- 运行层仍有合同缺口，与文档声称的行为不一致。

本方案只修这些缺口，并用测试把现有行为锁死。

## Decisions

用户 2026-09-12 确认按推荐收口，**先确认本方案再改代码**：

1. **仅 runtime 硬化**：不抽 Adapter、不改 Progress/空转/提示词/工具。
2. **编制取消粒度 A**：只在工具之间检查 cancel；单个同步工具跑完。
3. **重试与错误码不动**：仍是 3 次、同一正文；不拆 `AGENT_TURN_BUDGET_EXCEEDED`。

硬约束：

- 不改业务语义：提示词、工具 schema、覆盖账本、work_state、进展阈值、完成条件、物理预约上限、`tool_choice=required`、JCS 正文。
- 不升级 `CHECKPOINT_CONTRACT_VERSION`（3）/ `RUNTIME_ADAPTER_VERSION`（`rig-chat-0.42.0/3`）。
- 不把 `content_runtime` 并入本层。
- 不抽跨领域 `AgentHost`，不把 transcript 裁剪或工具分发上收。

## Approach

| 类 | 问题 | 本方案 |
|---|---|---|
| A. 合同回归 | `chat::send` spawn 的流任务在取消/drop 时不 abort；编制 `execute` 丢弃 cancel | 修到与现有文档一致 |
| B. 可观测/可测 | `drive` 缺少成功路径宿主测试；prefix/suffix 魔法数；`Session::bytes()` 尺子未写清 | 测试 + 命名常量 + 注释 |
| C. 明确不做 | 重试次数/可重试分类、拆错误码、Progress 策略、Adapter 去重、领域空转 | 会改预约消耗、SQL 失败码或提取行为 |

### A1. 取消时 abort 后台 HTTP

`chat::send` 为心跳不打断 SSE 而 `tokio::spawn(complete_provider_stream)`。超时分支已 `handle.abort()`；`drive` 在 `call_model` 上 `select` 取消时只 drop 外层 future。Tokio `JoinHandle` 默认 drop 不 abort，请求可在供应商侧继续，超时器一并被 drop。

实现：

- 在 `chat.rs` 增加私有 `AbortOnDrop<T>`，`Drop` 时 `handle.abort()`。
- `send` 用它包住 spawn。超时路径仍显式 abort + `AGENT_TURN_TIMEOUT`（与 drop 双 abort 无害）。
- **不**把 `CancellationToken` 传入 `send`：drive 已用 drop 表达取消。
- `OnceHttp` 一份预约一次发送、无 HTTP 重试、`[DONE]` 截流保持不变。
- 恢复仍是 **at-least-once**：已预约次数仍计；pending 无 response 时恢复会再 reserve。只停止幽灵 HTTP。

测试（本机 loopback，模式同 `headerless_connection_times_out`，**不** `#[ignore]`）：

- 服务端读完请求后挂起，不写响应。
- `pin` 住 `send`（长超时），等请求到达后 **drop** `send`。
- 断言服务端很快读到连接关闭（远小于超时），证明 spawn 已被 abort。
- 现有超时 / `[DONE]` / 单次发送断言继续通过。

### A2. 编制工具批在边界上响应取消

`docx_composition::agent::execute` 目前 `_cancel`。`execute_turn` 同步改内存 `Checkpoint`；`drive` 只在 `execute` 之后 `committed` + `save`。因此工具中途取消 = 丢弃内存、磁盘停在 **已保存 response、未 committed**（与崩溃同一恢复点）。恢复跳过模型、重放整批工具。不新增幂等层。

实现：

- 将 `driver::check_cancel` 改为 `pub(crate)`，由 `agent_runtime.rs` 再导出。错误码/文案不变：`INTERNAL` / `"Agent run cancelled"`。
- `execute_turn` 改为 `async`，增加 `&CancellationToken`。循环 `enumerate`：`index > 0` 时 `check_cancel`。
- 每完成一个工具后 `tokio::task::yield_now().await`，让 Worker 在批内有机会 `cancel()`。**不**打断当前正在执行的同步工具（`compile_docx` 等跑完本次调用）。
- 提取侧已把 cancel 传到 `read_source_view`；**不**在提取 CPU 工具循环加新取消点。

测试：

- **单元**：`pub(super) execute_turn` + 已取消 token + 两个 `inspect_composition`。第一工具执行（`tool_calls == 1`），第二工具不跑，`turn` 不递增，错误 `INTERNAL`。
- **磁盘合同**（`drive` 内存宿主，不必走编制 Postgres）：预先 `responded` 的 journal + 已取消 token → `execute`/`call_model`/`reserve` 均为 0，pending 仍带 response、未 committed。

### B1. `drive` 成功路径内存测试

在 `driver.rs` 增加不碰网络的 `ScriptedHost`（补现有 `FailedProvider`）：

1. `prepare_session` + `prepare` → `reserve` → 合法 `tool_calls` → `execute` → `session.finish` → `committed` → `save`，两轮后 `Session::turn() == 2`。
2. 预先写入 `responded`：`call_model`/`reserve` 次数为 0，仍 `execute` + `committed`。
3. 已保存 response + 取消：不执行工具、不调模型。

为让第二轮窗口复用可构造，在 `session.rs` 增加 crate 内测试辅助（不进生产 API）：

- `Session::projected_history()`（`#[cfg(test)]`）
- `test_window_body` / `test_system_evidence`（`#[cfg(test)]`），复用现有 `projected` + prefix/suffix 包装

### B2. 窗口参数与双尺子

数值不变，只命名：

```text
SESSION_PREFIX = 2                          // system + 每轮元数据
ANALYSIS_SESSION_SUFFIX = 1                 // 提取/复核的 progress 包
COMPOSITION_SESSION_SUFFIX = 0              // 编制把进度放进元数据
```

替换生产调用：

- `tender_analysis/agent.rs`：`(2, 1)`
- `docx_composition/agent.rs`：`(2, 0)`
- `tender_analysis/tests/work.rs` 与 `session.rs` 测试中的生产布局 `(2, 1)`

**不改** `agent_runtime.rs` Journal 单测里的 `(1, 0)`：那是只有 system 的最小夹具，不是生产窗口。

`Session::bytes()` / `drive` 释放窗口处加注释：比较的是 **SDK AgentRun JSON**，不是 Chat wire body；发送预算仍由适配器用 `chat::prepare` 后的 request 字节执行。不改比较对象。

## Files to modify

- `crates/bidding/src/agent_runtime.rs` — 导出 `check_cancel` 与三个窗口常量
- `crates/bidding/src/agent_runtime/chat.rs` — `AbortOnDrop`；drop 中止测试
- `crates/bidding/src/agent_runtime/driver.rs` — `check_cancel` 可见性；成功/恢复/取消测试；session 释放注释
- `crates/bidding/src/agent_runtime/session.rs` — `bytes()` 注释；`#[cfg(test)]` 窗口夹具
- `crates/bidding/src/tender_analysis/agent.rs` — `(2, 1)` → 命名常量
- `crates/bidding/src/tender_analysis/tests/work.rs` — 同上
- `crates/bidding/src/docx_composition/agent.rs` — `(2, 0)` → 常量；`execute_turn` 异步 + 批内 `check_cancel` + `yield_now`
- `crates/bidding/src/docx_composition/tests.rs` — 两工具批、已取消 token 的 `execute_turn` 测试

不改：`progress.rs`、`authoring_runtime.rs`、`content_runtime.rs`、提示词、工具 schema、Postgres SQL、baseline 合同字段、合同版本常量。

## Reuse

- `drive` 已有 `check_cancel` 与 `INTERNAL` / `"Agent run cancelled"`
- `chat.rs` 已有 loopback `exchange` / `headerless_connection_times_out`
- `session.rs` 已有多轮复用 / Skip / 角色交接夹具，driver 成功测试复用其 body 布局
- 编制 `composition_budget_and_cancellation_keep_unfinished_state` 覆盖 **run 开始前**取消；新测试覆盖 **工具批内**
- 提取 `Journal.source_view` 已与 cancel `select`，不改
- Postgres 失败码白名单含 `AGENT_TURN_BUDGET_EXCEEDED` / `AGENT_OUTPUT_INVALID` — **不拆码**（`tender_analysis/postgres.rs`）

## Out of scope

- 抽取共享 `AgentHost` / 合并两套 `trait Journal`/`Model`
- 按 HTTP 状态或 invalid terminal 改变 3 次重试
- 把 `execution_blocked` 与 `budget_exhausted` 拆成不同 `AgentError.code`
- Progress 策略、work_state、空转、提示词、工具
- 并行工具、向 UI 推 SSE、tokenizer、content Agent
- 编制单个工具内部抢占（同步 `compile_docx`）
- 提取侧批内 CPU 工具取消点

## Steps

- [ ] **S1** `chat::send`：`AbortOnDrop` 包住 `JoinHandle`；超时仍 abort + `AGENT_TURN_TIMEOUT`
- [ ] **S2** loopback 单测：drop `send` 后服务端连接关闭；`headerless_connection_times_out` 仍过
- [ ] **S3** driver 内存宿主：两轮成功且 SDK turn>1；`responded` 恢复零模型调用；response 后取消不执行工具
- [ ] **S4** 编制 `execute_turn`：`index > 0` 时 `check_cancel`；工具后 `yield_now`；错误码与 drive 一致
- [ ] **S5** 编制测试：已取消 token + 两 `inspect_composition` → 只跑第一工具、`turn` 不变、`INTERNAL`
- [ ] **S6** `SESSION_PREFIX` / `ANALYSIS_SESSION_SUFFIX` / `COMPOSITION_SESSION_SUFFIX`；`bytes()` 与 session 释放注释
- [ ] **S7** 回归：`cargo test -p bidding --lib agent_runtime` 及编制/提取相关取消测试；不改 SQL、不跑真实供应商

## Verification

- runtime / 编制新测试全绿；`headerless_connection_times_out` 与 `[DONE]` 截流仍成立
- `provider_retry_waits_between_exact_reserved_requests_and_keeps_exhaustion` 仍为 3 次、1s/2s、同一 body
- 现有取消测试错误码仍为 `INTERNAL` 或既有预算码，不新增失败码
- `git diff` 不含 `MAIN`/`REVIEWER`、工具 schema、`progress.rs` 默认值、合同版本常量
- 不修改 `CHECKPOINT_CONTRACT_VERSION` / `RUNTIME_ADAPTER_VERSION`
