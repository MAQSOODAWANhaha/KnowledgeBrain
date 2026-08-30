# OutlineGenerateV2：冻结全覆盖、可恢复 Map/Reduce 与闭合大纲生成

## Status

- 方案状态：已评审，作为下一轮实现基线。
- 当前实现已有：冻结输入、稳定 Map batch、Map artifact 缓存、受限冻结读取工具、增量 SSE/tool-call 解析、候选发布、阶段进度。
- 当前实现待替换：Map 与 synthesis 共用 10 分钟 wall clock、单体 `submit_outline`、非 attempt-aware trace、非原子 Agent 终态、全量 Agent 自主读取与路由。
- 本计划只定义代码与向前兼容的数据变更；不得在 live PostgreSQL 上重跑 baseline、执行 `down -v` 或让自动测试连接 `:15432`。
- 前端与部署变更在本地验证完成前保持 source-only；未经明确授权不重建/发布运行环境。

## 1. 目标

`OutlineGenerateV2` 必须在冻结 `DocumentSet` 上生成招标证据驱动的 proposed outline，并满足：

1. 每个冻结 `SourceUnit` 全覆盖，无全局 first-N、头中尾或整文截断。
2. 招标文件明确的组成、格式、目录、资格、评分、报价、技术、商务、表单及附件结构优先于通用知识。
3. 模型只负责结构判断、冲突消歧和低置信路由；Rust 负责覆盖、组装、闭合校验和发布。
4. 失败重试复用已完成 Map、Reduce、draft chunk 和 requirement route，不从 turn 1 全部重跑。
5. 达到采集预算只会从 collecting 转入 drafting，不因普通 wall-clock 直接丢失结果。
6. 输出中的 SourceUnit/requirement 身份必须来自冻结输入；最终通过 Rust 与 SQL 双重闭合校验。
7. 不允许 AI 失败后生成无来源的通用大纲 fallback。
8. 进度在刷新后恢复，attempt/turn/tool counters 单调，不重复 toast。

## 2. 现场规模与问题

以请求 `354ad19a-cfc4-4bfe-8443-94a5d8b53fde` 为校准样本：

- 冻结输入约 629 KiB；
- 136 个 SourceUnit；
- 101 条 requirement，约 260 KiB；
- 4 个 structured form；
- 10 个 Map batch；
- Map artifact 共约 54 KiB，113 个 chapter signal、4 个结构冲突；
- synthesis 曾持续搜索/读取但未调用 `submit_outline`；
- 第一次 attempt 因 Map 与 synthesis 共用 10 分钟而 `AGENT_DEADLINE_EXCEEDED`；
- 后续 attempt 出现 SSE body error，并重新从较小 turn 计数，造成 UI 看似倒退。

根因：

- `AGENT_WALL_CLOCK` 从整个 job 开始计时，Map 消耗 synthesis 时间；
- 模型只要持续调用读取工具，代码不会强制进入提交阶段；
- 单次 4096/8192 token 无法可靠承载可扩展的 nodes + bindings + notices；
- tool trace 以 `(request_artifact_id, ordinal)` 唯一，重试从 ordinal 1 开始后被冲突吞掉；
- run upsert 主要写 `running`，请求 terminal 与 Agent run terminal 不原子；
- 当前 Map retry 循环只覆盖 `finish_reason=length`/非法 JSON，provider 连接或 SSE 错误会提前返回。

## 3. 非协商约束

- 生产 API 保持 Postgres-only，不 hydrate 整个 workspace Store。
- 真源保持 `OutlineNode` + `ContentBlockV1`，不是 Markdown 或 Plannotator runtime。
- 不增加用户可见 freeze 按钮；生成内部冻结并等待编译。
- 不使用 Pi SDK 作为运行时依赖。
- 不开放 bash、文件系统、live DB 或任意网络工具给 Agent。
- 视觉只允许当前冻结招标图片，最多 4 张，绝不作为投标方证据。
- per-turn provider timeout 保持 120 秒；transient call 最多额外重试一次或受统一总 attempt 上限约束。
- obsolete/missing request 是成功 skip，info 日志，不修改成失败。
- 不生成 generic fallback；结构证据不足时返回明确错误或 high-severity notice。

## 4. 总体流水线

```text
Freeze
  → Map（全覆盖、缓存、可恢复）
  → Deterministic Reduce（全局结构索引）
  → Evidence Collect（只复核冲突/低置信项）
  → Draft Tree（分块）
  → Route Requirements（确定性优先，Agent 处理歧义）
  → Rust Closure（组装完整 OutputV1）
  → Verify（Rust + SQL）
  → Publish（原子终态）
```

状态机：

```text
analyzing.loading
  → mapping
  → reviewing.reducing
  → reviewing.collecting
  → generating.drafting
  → generating.routing
  → generating.verifying
  → succeeded | failed | cancelled
```

为减少数据库枚举迁移，首版可继续使用现有四个 `progress_stage`，细阶段写入 `progress_detail.phase`。

## 5. Map V2：全覆盖结构与路由信号

### 5.1 分批

- 按冻结 DocumentSet、文档顺序、SourceUnit ordinal 稳定分批。
- 每个完整 SourceUnit 恰好进入一个 batch。
- 单个超大 unit 仅按稳定 `(source_unit_revision_id, offset, length, shard_index, shard_count)` 分片。
- 分片重组 digest 必须等于冻结 `text_sha256`。
- 关联 requirements/forms 跟随其 SourceUnit batch；视觉只保存需求，不自动读取。

### 5.2 `OutlineEvidenceBatchV2`

在 V1 基础上将 `chapter_signals` 升级为可合并的结构片段：

```json
{
  "signal_ref": "stable-digest",
  "title": "资格证明文件",
  "semantic_role": "qualification",
  "signal_kind": "explicit_toc",
  "path_segments": ["投标文件", "资格证明文件"],
  "heading_level": 2,
  "numbering": "第一章",
  "source_order": 123,
  "source_unit_revision_ids": ["uuid"],
  "confidence": "high"
}
```

`signal_kind`：

- `explicit_toc`
- `explicit_composition_clause`
- `explicit_format_clause`
- `heading`
- `form`
- `evaluation_clause`
- `inferred`

增加 requirement route hint：

```json
{
  "need_occurrence_id": "uuid",
  "suggested_semantic_role": "quotation",
  "target_path_hint": ["报价文件"],
  "channel": "quotation",
  "source_unit_revision_ids": ["uuid"],
  "confidence": "high"
}
```

所有 ID 必须在当前 batch/frozen requirement scope；Rust 对越界或缺失 ID 做规范化，不让单个脏字段报废整个 batch。

### 5.3 Map 执行策略

- 初始受控并发为 2；按 provider 稳定性配置，不随 workspace 大小无界增长。
- 每次 provider call 最长 120 秒。
- 每批最多 3 次总尝试；transport、429/5xx、length、非法 JSON 共用此上限，防止乘法重试。
- `json_schema` 不支持时回退 `json_object`，仍由 Rust closed parser 验证。
- `finish_reason=length` 应缩小当前 batch 或请求更紧凑输出，不盲目重复同一请求。
- artifact key 至少包含 `frozen_input_sha256 + batch_ordinal + model_contract_sha256 + agent_contract_sha256 + evidence_schema_sha256`。
- V2 contract/schema digest 变化后不得复用 V1 cache。
- 不设共享 10 分钟业务 deadline；有限 batch、有限 attempt 和 per-call timeout 已给出确定上界。

## 6. Deterministic Reduce

Map 完成后由 Rust 生成不可变 `OutlineReducePlanV1`：

```json
{
  "coverage": {
    "source_units_total": 136,
    "source_units_mapped": 136,
    "requirements_total": 101,
    "requirements_routed": 0
  },
  "structure_fragments": [],
  "priority_reads": [],
  "requirement_routes": [],
  "unresolved_conflicts": [],
  "vision_requests": [],
  "notices": []
}
```

Reduce 必须：

1. 按文档顺序、SourceUnit ordinal、shard offset 稳定排序；
2. 合并同一结构路径的重复信号；
3. 不按全局标题盲目去重，保留不同附件/分册内同名章节；
4. 优先 `explicit_toc/composition/format`，其次 heading/form/evaluation，最后 inferred；
5. 收集全部 format/evaluation/qualification requirements；
6. 计算每个 frozen SourceUnit/need occurrence 的 disposition；
7. 只把冲突、低置信、视觉、未路由要求加入 `priority_reads`；
8. 产生内容 digest，并持久化供 attempt 恢复。

Reduce 不调用模型，不截断身份集合。

## 7. Evidence Collect：覆盖驱动的受限复核

Agent 初始上下文只包含 manifest、ReducePlan、当前 outline 和输出合同。完整冻结输入不进入 prompt。

允许工具：

- `get_manifest`
- `search_frozen_units`
- `read_source_units`
- `read_requirements`
- `read_structured_forms`
- `read_tender_images`
- draft/routing/finalize 工具

Agent 只读取：

- 明确目录、组成、格式来源；
- Reduce 标记的结构冲突；
- 低置信结构；
- 必要表单；
- 无法确定路由的 requirement；
- 最多 4 张确有结构识别需要的冻结招标图片。

正常转入 drafting 的覆盖条件：

```text
priority structure evidence reviewed
AND mandatory format/evaluation/qualification needs disposed
AND structural conflicts disposed
AND required forms/vision requests disposed
```

安全上限（AgentContract 版本化）：

- collecting 最多 8 turns；
- 最多 20 个读取工具；
- 最多 192 KiB 精确原文；
- 最多 4 张图片；
- collecting soft wall 8 分钟。

命中任一上限只触发 `collecting → drafting`，不直接失败。

## 8. 分块 Draft 与 Requirement Routing

### 8.1 不再单次提交完整输出

废止将完整 `OutlineGenerationOutputV1` 作为单个 `submit_outline` tool arguments 的主要路径。单次大 JSON 会因 token 上限截断，且无法从中间 checkpoint 恢复。

### 8.2 `submit_outline_nodes`

```json
{
  "chunk_ref": "qualification",
  "nodes": []
}
```

- 每个 chunk 最多 100–200 个节点；
- chunk append-only，以 `(request, attempt, chunk_ref, digest)` 标识；
- 节点 client ref 在整个 draft 唯一；
- 每个顶层/推断章节必须携带冻结来源或明确 notice；
- provider length 时缩小 chunk，不重发整个大纲。

### 8.3 `route_requirements`

```json
{
  "routes": [
    {
      "need_occurrence_id": "uuid",
      "target_client_node_ref": "qualification-1",
      "channel": "response_table"
    }
  ]
}
```

- high-confidence Map routes 由 Rust 直接应用；
- Agent 只处理 unresolved/ambiguous routes；
- route 分批提交并持久化；
- 未能路由的 occurrence 必须形成 `UNMAPPED_REQUIREMENT` notice。

### 8.4 `finalize_outline`

```json
{
  "draft_digest": "sha256"
}
```

该工具不携带完整输出。Rust：

1. 合并已接受 node chunks；
2. 合并确定性与 Agent routes；
3. 生成 notices；
4. 组装完整 `OutlineGenerationOutputV1`；
5. 执行闭合验证；
6. 成功才允许 publish。

若失败，返回结构化修复项（如 `UNREACHABLE_NODE`、`MISSING_NEED_DISPOSITION`），只允许一次定向 repair；不得重新读取全部资料。

## 9. `SynthesisPacketV1` 与 Checkpoint

finalizer 不携带全部历史消息。持久化 bounded `SynthesisPacketV1`：

- 全量身份 coverage manifest；
- Reduce 后结构片段；
- 选中的 source unit ID/range/result digest；
- compact requirement identities 与 route dispositions；
- forms/conflicts/notices；
- artifact digests。

原文仍留在冻结存储，通过 identity/range 重放；不保存模型隐藏推理。

不可变 `OutlineSynthesisCheckpointV1`：

```json
{
  "attempt": 2,
  "phase": "drafting",
  "reduce_plan_sha256": "sha256",
  "selected_evidence": [],
  "accepted_node_chunks": [],
  "accepted_routes": [],
  "unresolved_need_occurrence_ids": [],
  "total_turns": 10,
  "total_tool_calls": 22,
  "text_bytes_read": 98304,
  "images_read": 0
}
```

worker/provider 中断后从 checkpoint 继续，不依赖重放 chain-of-thought。

## 10. Retry、Deadline 与错误语义

### 10.1 统一 provider retry

Map、collecting、drafting、routing 共用 retry classifier：

- connect timeout、SSE body error、429、明确可重试 5xx：当前调用额外重试一次；
- format unsupported：同次调用回退 `json_object`；
- length/非法 JSON：缩小 batch/chunk 或追加格式修正后重试；
- frozen identity、schema invariant、request obsolete：不可重试。

Job-level retry 只用于 provider 持续不可用或进程崩溃，并从 artifact/checkpoint 恢复。

### 10.2 Deadline

- 删除 Map 与 synthesis 共用的 `AGENT_WALL_CLOCK=10min`。
- synthesis 从 Reduce/collecting 开始独立计时；soft deadline 只切阶段。
- 保留约 60 分钟进程级 watchdog 处理真正卡死，不作为正常质量决策。
- 用户可见 terminal 错误应是实际原因：
  - `STRUCTURE_EVIDENCE_INSUFFICIENT`
  - `AGENT_PROVIDER_UNAVAILABLE`
  - `AGENT_OUTPUT_INVALID`
  - frozen/request errors
- `AGENT_DEADLINE_EXCEEDED` 仅保留内部 watchdog 兼容，不应成为常规结果。

## 11. Progress、Attempt 与 Immutable Trace

进度 detail 使用单调累计值：

```json
{
  "attempt": 2,
  "phase": "routing",
  "mapped_batches": 10,
  "total_batches": 10,
  "structure_signals": 113,
  "mandatory_requirements_done": 40,
  "mandatory_requirements_total": 40,
  "requirements_done": 78,
  "requirements_total": 101,
  "turn_in_attempt": 4,
  "tool_calls_in_attempt": 8,
  "total_turns": 10,
  "total_tool_calls": 22,
  "retry_count": 1
}
```

Tool trace 唯一身份改为：

```text
(request_artifact_id, attempt, ordinal_in_attempt)
```

trace 保留工具名、参数/result digest、读取身份/范围、字节数、耗时、状态；不复制大段原文。总计数由数据库或 checkpoint 单调维护，不因重试归零。

用户进度：

```text
分析文件 10/10
汇总结构 113 项
复核条款 78/101
生成候选 · 正在校验
```

瞬时重试显示：

```text
自动重试 2/3 · 正在恢复生成
```

只有显式点击显示一次 toast；transient retry 不 toast；terminal failure 只通知一次，刷新后 banner 可恢复。

## 12. SQL 原子状态转换

新增 CAS 风格函数（名称可按迁移规范调整）：

```text
kb_bid_v2_outline_run_transition(...)
kb_bid_v2_outline_checkpoint_append(...)
kb_bid_v2_outline_draft_chunk_append(...)
kb_bid_v2_outline_route_chunk_append(...)
```

要求：

- 强制 request artifact/revision/frozen SHA；
- 校验合法 phase/status transition；
- terminal `succeeded/failed/cancelled` 不可被普通 upsert 恢复为 running；
- `kb_bid_v2_publish_outline_generation` 同一事务写 request 与 run succeeded；
- `kb_bid_v2_mark_outline_generation_failed` 同一事务写 request 与 run failed；
- obsolete/cancelled 同步写 run cancelled；
- append-only artifact 带 canonical payload digest；
- 函数保持 `SECURITY DEFINER SET search_path=pg_catalog,public` 和最小 grant。

live DB 只能应用经授权的向前变更，不能重跑 baseline 或 down migration。

## 13. 闭合验证

### 13.1 大纲树

Rust 在 publish 前验证：

- 根对象仅含 schema 声明字段；
- 恰好一个 root；
- client ref 唯一且符合 pattern；
- 所有 parent 存在；
- 无环；
- 所有节点都从 root 可达；
- 每组 sibling ordinal 恰为 `0..n-1`；
- title 长度、semantic/render role 枚举合法；
- source unit IDs 唯一且全部属于冻结输入；
- 每个顶层章节至少有关联来源；推断项必须有 notice。

SQL 重复关键身份、树和 frozen scope invariant，作为最终权威门。

### 13.2 Requirement closure

每个适用 `need_occurrence_id` 必须满足且只满足一种 disposition：

1. 有合法 binding；或
2. 有对应 `UNMAPPED_REQUIREMENT` notice。

禁止：

- 未知 need ID；
- 未知 node ref；
- 相同 need/channel 的冲突 target；
- 同一 need 同时 bound 和 unmapped；
- 缺失 mandatory need disposition。

闭合等式：

```text
bound needs ∪ explicitly unmapped needs = applicable frozen needs
bound needs ∩ explicitly unmapped needs = ∅
```

### 13.3 No generic fallback

- 候选顶层结构必须来自明确 Map/Reduce evidence；
- 无结构证据时返回 `STRUCTURE_EVIDENCE_INSUFFICIENT`；
- 冲突可选择最强显式证据并发布 `CONFLICTING_STRUCTURE` high notice；
- 不得用固定“商务/技术/报价/附件”模板掩盖 AI/provider 失败。

## 14. Files to modify

- `crates/bidding/schemas/outline-evidence-batch-v2.schema.json`
- `crates/bidding/schemas/outline-reduce-plan-v1.schema.json`
- `crates/bidding/schemas/outline-synthesis-checkpoint-v1.schema.json`
- `crates/bidding/schemas/outline-generation-output-v1.schema.json`（语义保持，补共享验证）
- `crates/bidding/src/outline_agent.rs`（Map V2、Reduce、状态机、draft/route/finalize、验证）
- `crates/bidding/src/bid_authoring_v2.rs`（artifact/checkpoint/transition ports）
- `crates/knowledge/src/enrichment/chat.rs`（统一 retry/format fallback 接口）
- `crates/worker/src/consume.rs`（attempt-aware retry/terminal）
- `migrations/bidding_v2_baseline.sql`（append-only artifact、trace identity、atomic state）
- `crates/api/src/bid_v2_routes.rs`（progress payload）
- `web/src/bid/api/types.ts`
- `web/src/bid/authoring/session.ts`
- `web/src/bid/authoring/useBidV2Session.ts`
- `web/src/bid/authoring/AuthoringShell.tsx`

不新增 sidecar，不开放 live 工具，不改变生产 API 的 PG-only 边界。

## 15. 实施顺序

### P0：恢复黄金路径

- [x] 删除 Map/synthesis 共享 10 分钟 deadline，soft deadline 只切阶段
- [x] 修复 Map/provider 统一调用级 retry
- [x] collecting 覆盖/预算达到后强制进入 drafting
- [x] attempt-aware progress 与 immutable trace
- [x] publish/fail/cancel 原子更新 Agent run terminal
- [x] Rust 完整树、来源、requirement closure 验证
- [x] 增加 timeout/retry/finalizing/terminal regression tests

### P1：可扩展闭合

- [x] `OutlineEvidenceBatchV2` 层级与 route hints
- [x] deterministic `OutlineReducePlanV1`
- [x] `SynthesisPacketV1` 与 checkpoint
- [x] 分块 `submit_outline_nodes`
- [x] 分批 `route_requirements`
- [x] Rust `finalize_outline` 与一次定向 repair
- [x] 受控 Map concurrency

### P2：体验与运维

- [x] 刷新后恢复 attempt/phase/累计进度
- [x] transient retry 不 toast，terminal failure 单次通知
- [x] stale-run watchdog 与 retry-wait 进度
- [x] 指标：阶段耗时、provider retry、读取字节、闭合错误
- [x] 同冻结 DocumentSet/source identity 的后端 durable dedup

## 16. Verification

### Unit / property

- 任意 SourceUnit 集合均恰好覆盖一次；超大分片可 digest 重组。
- Map V2 越界 ID 被拒绝/规范化，缓存键随 schema contract 变化。
- Reduce 排序稳定；同名不同路径不误合并。
- provider transport、length、JSON malformed 共用总 attempt 上限。
- collecting 达到 turn/tool/text/time 上限后进入 drafting，不返回 deadline error。
- draft chunk/route chunk 重放幂等，checkpoint 可恢复。
- 树 cycle/unreachable/duplicate/bad ordinal 被 Rust 拒绝。
- 每个 applicable need 必须 bound 或 explicit unmapped。
- generic/no-source outline 被拒绝。

### Persistence

- attempt 2 的 ordinal 1 trace 不覆盖 attempt 1。
- total turns/tool calls 单调。
- succeeded/failed/cancelled run 不被 upsert 恢复为 running。
- request 与 run terminal 在同一 SQL 事务更新。
- obsolete/missing job info skip 且不触发 provider。

### Integration / E2E

- 使用隔离 PostgreSQL（明确拒绝 live `:15432`）运行 contract tests。
- 1 个短 DOCX、10 个混合文件、长表、扫描图片均生成 proposed outline。
- Map 总耗时超过旧 10 分钟仍能进入 synthesis。
- turn 4 SSE 断流只重试当前调用/恢复 checkpoint。
- worker 中途终止后不重跑已完成 Map/draft chunks。
- 页面刷新显示当前 attempt/phase，计数不倒退。
- 用户接受候选后 `OUTLINE_EMPTY` 消失。

## 17. 明确不做

- 简单把 10 分钟改成 30 分钟并保留原控制流；
- 单次向模型提交/索取完整超大 OutputV1；
- 把 629 KiB 冻结输入或全部工具历史塞入 finalizer；
- 全局 first-N、头中尾抽样后直接生成大纲；
- 重试时重跑已完成 Map/Reduce/draft；
- 通用大纲 fallback；
- Agent 访问 bash、live 文件、live 数据库或任意网络；
- 招标图片作为投标方证据；
- 自动测试连接 live PostgreSQL `:15432`。
