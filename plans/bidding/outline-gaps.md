# 大纲缺口与旧路径退役

现行合同是 [PRD](../../docs/bidding/prd.md) 和 [运行时](../../docs/bidding/outline.md)。本文只记录还没接到这条链上的工作，以及必须退出产品合同的遗留。缺口不是当前设计。

本文件不授权改业务代码。下面的「完成」指后续实现要达到的可检查结果。

## 1. 阅读包正文要进模型

现状：`DiscoverWork::session` 把 `ParsePack` 放进请求 brief 的 `reading_packs`。`text` 只有 `source_id`、`start`、`end`，`forms` 只有 `form_id`、`start`、`end`、`header_cells`。冻结来源的文字和单元格不在包里。`claim_turn` 只返回本轮新领取的包，已经是 `running` 的包不会再次出现。

完成：发现轮发给模型的包符合 [运行时](../../docs/bidding/outline.md) 里的目标载荷：`text[]` 带切片正文，`forms[]` 带 `[start, end)` 的单元格，续表另带 `header` 且这些表头不在引用范围内。本轮仍要处理的 `running` 和待修 `failed` 包每次重放。模型提交的 `start` / `end` 仍必须落在这个切片里。

验证：用一份含标题、超长条款和续表的冻结输入组包，断言 brief 里的包文本等于来源切片，包外文字不出现，续表切片带 `header`，且这些表头单元格的下标不在 `[start, end)` 内。同一 `running` 包在下一轮请求里仍然带正文。

## 2. 发现要求要进入组织

现状：提交成功后只把描述存进 `DiscoverWork.requirements`，键是 `{pack_id}:{index}`。`put_chapters` 不接收要求 id，写入的 `requirement_ids` 为空。组织轮宿主包没有这份要求。

完成：组织轮能读到已提交要求（身份、描述、来源切片），并把要求挂到章节。发现轮仍然不能写章节。

验证：两包提交之后，组织轮可见这些要求。`put_chapters` 写出的 `requirement_ids` 覆盖已提交要求；未知 id 被拒绝；发现轮调用 `put_chapters` 仍被拒绝。

## 3. 从 tool_draft 投影并发布，进度改读这条状态

现状：`outline::template::project` 从 `AnalysisResult` 的 `draft_plan`、records 和 outline issues 投影 `OutlineArtifact`，没有生产调用方。`outline::store::publish` 能把 artifact 写入 `kb_bid_v2_publish_outline`，`finish_draft_path` 不调用它。`Checkpoint::progress` 仍报告 `analysis.outline` 和 `draft_plan`（`outline_requirements`、`outline_chapters`、`outline_scan_repair` 等）。前端 `AnalysisProgress` 也读这些字段。

完成：`tool_draft.finished` 且 phase 为 `complete` 时，投影出经 `validate_artifact` 的 `OutlineArtifact`（章节、槽、附件绑定）再发布。进度来自 `DiscoverWork` 的包计数和 `tool_draft` 的章节、未绑定附件、槽是否已交、是否结束。扫描游标不再代表发现进度。

验证：完成的工具草稿投影后，artifact 的章节 id、槽和绑定与草稿一致，未完成草稿不能发布。只有 `tool_draft` 里有章节时，进度里的章数等于这份草稿，而不是 `draft_plan` 的长度。

## 旧路径退役

生产强制 `draft_path = true`。下列内容不是产品合同：

- `plans/bidding/product-two-phase.md` 已删除。不要再把它写成唯一入口。
- `analysis/outline_flow.rs` 以及工具名 `submit_outline_scan`、`put_outline_items`、`submit_outline_check`。`draft_path = false` 只覆盖抽取单测。
- `draft::after_batch` 在旧 `outline.checks` 上调用 `outline_flow::apply(..., "finish_outline")`。新路径的结束只走 `outline::agent::apply` 的 `finish_outline`。
- 双正式编制 / draft-fill、强制终稿复核，以及 T0–T9、A01–A20 那套任务表。

退役完成时：生产请求构造不出这些旧工具；`draft_path = false` 不能发布 `OutlineArtifact`；文档索引不再把两阶段方案写成入口。

## 职责分叉

模板维持独立职责，还是并入组织，见 [运行时](../../docs/bidding/outline.md) 的「职责分叉」。建议等缺口 1–3 落地再决定。合并会改 `outline/agent.rs` 的职责可见性、提示和对应测试；六个工具的 schema 形状不动。本文件的缺口不包含这次合并。

## 一次成稿验收

下面替换已删除的 A01–A20。对象是一条真实招标、`draft_path = true` 的大纲运行。缺口 1–3 未接通时，这条清单是目标，不是当前已经通过的记录。

通过：

1. 冻结解析按解析器顺序发布。大纲运行读的是这份冻结输入。
2. 计划中的每个阅读包变为 `committed`。每条要求的 `source_id`、`start`、`end` 落在该包切片内。续表的表头只作为上下文。
3. 章节树非空、无环、同级顺序不重复。每个附件表恰好绑定一个章节。
4. 已经 `put_slots`。`bidder_blank` 和 `signature` 的文本为空、`match_query` 非空、`response_required` 为真。其它种类的 `match_query` 为空。每个 `response` 章节至少有一个槽。
5. `finish_outline` 使 `tool_draft.finished` 为真，phase 为 `complete`。
6. 投影出的 `OutlineArtifact` 通过 `validate_artifact`：版本和身份正确，章节或模板非空，id 不重复，分组章节不带知识库回答槽，回答槽文本为空且带 `match_query`。
7. `outline::store::publish` 写入该 artifact。进度里的包计数、章数、未绑定附件、槽是否已交和是否结束来自 `DiscoverWork` 与 `tool_draft`。

拒绝：

| 情况 | 结果 |
| --- | --- |
| 引用了别的包，或偏移不在本包切片内 | `outside_pack` 或 `outside_slice`，包变为 `failed`，该条要求不入库 |
| 还有未绑定的附件表就 `finish_outline` | 拒绝结束，`finished` 保持 false |
| 分组章节带 `bidder_blank` 或 `signature` | `put_slots` 拒绝 |

## 不做

- 不建长期语义记忆。检查点是记忆，SDK 会话只复用当前窗口。
- 大纲包不读企业知识库。知识库匹配留在 `response`，输入只能是已冻结的 `OutlineArtifact`。
