# 投标条款抽取：专用智能体

## Context

当前抽取是「规则切段 + 标题词表 + 每段一次 JSON」。标题没命中仍会抽（`unknown`），但有三处硬伤：

- `hint_family=skip`（投标人须知等）**不调模型**，实质性废标句会丢。
- 转出来没有 `#` / `第X章` / `3.2` 时整份变成一段「正文」，8000 字切片 + 每片 40 条，后半本丢掉。
- 抽完没有覆盖率，人不知道哪一章是空的。

WeKnora 的通用对话 Agent 不搬。要抄：**专用智能体 + 极小工具 + 有界循环 + grep/按段读原文**。标题只做目录。

人确认、`BidMatchJob`、两路 `/match`、招标文件不进 `documents` —— 都不动。

## Approach

仍是 **一个** `BidExtractRun`、一把项目锁、worker 队列 `bid:extract`。两个智能体顺序跑；同一项目禁止并行两套抽取。

```
convert 完成
  → 规则切 BidSection（大纲；hint 只提示，不再 skip 掉）
  → TechnicalClauseAgent     family 锁死 technical
  → 交出 CoverageSnapshot    已 emit 的 quote + 覆盖到的 heading
  → CommercialClauseAgent    同一套工具 + 快照（不是共用对话）
  → CoverageSweep            见下，确定性函数，不是第三个 Agent
  → quote 去重写入 draft
  → 人确认 → 原匹配
```

### 两个智能体怎么传上下文（简单传，不隔离）

**不**把技术 Agent 的 12 轮聊天记录交给商务——那才复杂。

只传一份内存里的 `CoverageSnapshot`：`quotes[]` + 被 quote 命中的 `heading_path[]`。商务的第一条 user 消息带这段摘要；`list_outline` 给已覆盖节点打标。两边各有自己的 messages。实现就是一个结构体往下传，比完全隔离多几十行。

技术看到注册资本：**不 emit**。商务看到吞吐量：不 emit。快照避免商务把同一句再抽成商务条款（去重也会挡）。

### 每个智能体

5 个工具，作用域 = **这一份招标 Markdown**，禁止打产品/公司库：

| 工具 | 作用 |
|---|---|
| `list_outline` | 标题、hint、是否已被 quote 覆盖 |
| `read_span` | 按 heading 读，单次 ≤ 8000 字（超长沿用 `split_long_body`） |
| `grep` | 本文件正则，回行 + heading，上限 40 |
| `emit_clauses` | 校验 `quote_in_body` 后写入本轮缓冲；family 由智能体身份写入；单次 ≤ 40 |
| `done` | 结束该智能体 |

每边最多 12 轮；单文件软顶 400 条。JSON 动作协议：模型只回 `{ "tool", "args" }`。多轮走 `enrichment::chat_messages`。

步骤 **只打日志**（工具名、heading、条数、错误），不落 `steps` 表、不加 migration。

「重试该段」= 对该 `BidSection` 再跑一次 `extract_one_piece`，不开 12 轮。

### CoverageSweep 是什么、在哪跑

**不是**第三个智能体，也不是新队列、不是人要点的按钮。

就是 `extract_run` 里、两个 Agent **都结束之后**、写库之前的一段普通 Rust：

1. 已有缓冲 = 技术 emit + 商务 emit。
2. 每个 `BidSection`：若任一 quote 是该 `body` 的子串 → 已覆盖。
3. **未覆盖** 且正文命中种子词（必须 / 须 / 应 / 否则废标 / 不少于 / ISO / 注册资本 / 业绩 / 类似项目 / 资质）→ 对该段调用现有 `extract_one_piece`（`hint_family` 当 prior）。**包括原来的须知/手续段。**
4. 未覆盖且无种子词 → `extract_status=skipped`，人可手补。
5. 补扫产出与已有 quote 去重（规范化后包含或重叠 → 丢新留旧）。

例：须知里有「必须具备 ISO 9001」——标题 hint 仍是 skip，但 Agent 没 grep 到时，补扫因「必须」「ISO」会再抽一次，应进商务 draft。

大纲偏移在 **本次作业内存** 里算，不改 `bid_sections` 表。覆盖用现成的 `quote_in_body(quote, section.body)`。

## Files to modify

- `docs/bid-platform-domain.md` — §0、§4.2、§7.1、§10
- `docs/system-design.md` + `.scratch/knowledgebrain/spec.md` — 抽取表述同步（CI `cmp`）
- `crates/enrichment/src/chat.rs` — `chat_messages(&[Message])`
- `crates/bid/src/extract_agent.rs`（新）— 工具循环、快照、补扫谓词
- `crates/bid/src/lib.rs` — `extract_run` / `run_extract_section` / `retry_section`；删 skip 短路径
- `crates/bid` 单测

不加 migration。不改 Compose、`/match`、队列名。

## Reuse

- `split_sections` / `hint_family` / `quote_in_body` / `parse_extract_json` / `split_long_body`
- `extract_one_piece` — 补扫 + 重试该段（先 `pub(crate)`）
- `extract_run` 的锁、supersede、读 `markdown_ref`
- `enrichment::chat_complete` / `models::chat_sse`
- `runtime::enqueue_bid_extract` + `BidExtractWorker`
- `storage::bid::{insert_section,insert_clause,set_section_status,set_extract_run}`

## Steps

- [ ] 改规格：标题非门闩；允许有界专用抽取；禁止通用闲聊 Agent / 抽取阶段打知识库
- [ ] `chat_messages` + stub 单测
- [ ] `extract_agent`：JSON 循环、family 锁死、quote 校验、CoverageSnapshot、tracing 日志
- [ ] `extract_run`：切大纲（skip 也插入）→ 技术 → 快照 → 商务 → CoverageSweep → 去重写入
- [ ] 删除 `if hint == "skip" { continue }`；`retry_section` 走 `extract_one_piece`
- [ ] 单测见 Verification

## Verification

- `cargo fmt --check`；`clippy --workspace --all-targets -- -D warnings`
- 无标题长文：grep/补扫仍能出条
- 须知「必须有 ISO」→ 商务 draft（Agent 或补扫）
- 技术表里的注册资本 → 不进技术 draft
- skip 段不再被直接丢掉
- 快照单测：技术已 emit 的 quote，商务再 emit 被去重
- upload 只入队，API 不 `tokio::spawn` 抽条款
- `cmp docs/system-design.md .scratch/knowledgebrain/spec.md`
