# 大纲缺口与旧路径退役

现行合同是 [PRD](../../docs/bidding/prd.md) 和 [运行时](../../docs/bidding/outline.md)。缺口 1–3 已接到一次成稿产品路径。本文留下的「现状」是接通前的记录；下面的验证和一次成稿验收是回归合同。模板已并入组织：一次成稿是发现 → 组织（章节、绑定、槽）→ 收尾。

本文件不授权另做一套产品设计。

## 1. 阅读包正文要进模型

现状：`DiscoverWork::session` 把 `ParsePack` 放进请求 brief 的 `reading_packs`。`text` 只有 `source_id`、`start`、`end`，`forms` 只有 `form_id`、`start`、`end`、`header_cells`。冻结来源的文字和单元格不在包里。`claim_turn` 只返回本轮新领取的包，已经是 `running` 的包不会再次出现。

完成：发现轮发给模型的包符合 [运行时](../../docs/bidding/outline.md) 里的目标载荷：`text[]` 带切片正文，`forms[].cells` 是 `[start, end)` 的行优先一维文本，续表的 `header` 按列顺序且不在引用范围内。本轮仍要处理的 `running` 和待修 `failed` 包每次重放。模型提交的 `start` / `end` 仍必须落在这个切片里。超预算丢掉发现对话时，只丢掉已经 `committed` 的包所在轮次，依据是 `DiscoverWork` 的包状态，不读 `analysis.outline.scanned`。

验证：用一份含标题、超长条款和续表的冻结输入组包，断言 brief 里的包文本等于来源切片，包外文字不出现，续表切片带按列排列的 `header`，且这些表头单元格的下标不在 `[start, end)` 内。同一 `running` 包在下一轮请求里仍然带正文。已 `committed` 的发现轮在超预算时从窗口消失；仍是 `running` 或 `failed` 的轮次留下。判定不读 `analysis.outline.scanned`。

## 2. 发现要求要进入组织

现状：提交成功后只把描述存进 `DiscoverWork.requirements`，键是 `{pack_id}:{index}`，值只有描述字符串，`source_id`、`start`、`end` 没有留下。`put_chapters` 的 schema 不接收 `requirement_ids`（章节项只要求 `id`、`parent_id`、`order`、`title`、`purpose`），宿主把 `ChapterOutline.requirement_ids` 写成空数组。空数组是现行行为，不是目标。组织轮宿主包没有这份要求。

目标参数（改 `outline-tools-v1.schema.json` 里 `put_chapters` 的章节项，以及 `outline/tools.rs` 的写入）：

- 检查点上的要求记录必须留下 `source_id`、`start`、`end`，不能只留描述。`requirement_ids` 只把这些 id 挂到章节；组织轮还要能读到对应切片。
- 每个章节增加必填 `requirement_ids`（字符串数组，允许空数组）。`id`、`parent_id`、`order`、`title`、`purpose` 仍必填。不新增其它章节字段。
- id 就是发现时写入的键 `{pack_id}:{index}`。
- 未知 id 拒绝。全部已提交 id 都要出现，每个 id 只出现在一个章节上。缺 `requirement_ids` 拒绝。
- 发现职责仍然不能调用 `put_chapters`。
- 组织轮宿主包能读到已提交要求的身份、描述和来源切片。

完成：上面的参数生效，组织轮能把要求挂到章节。

验证：两包提交之后，组织轮可见这些要求的描述和 `source_id`、`start`、`end`。只存描述、缺 `requirement_ids`、未知 id、已提交 id 没有全部挂上、或同一 id 出现在两个章节时拒绝。发现轮调用 `put_chapters` 仍被拒绝。不带 `requirement_ids` 的现行调用不能再通过。

## 3. 从 tool_draft 投影并发布，进度改读这条状态

现状：`outline::template::project` 从 `AnalysisResult` 的 `draft_plan`、records 和 outline issues 投影 `OutlineArtifact`，没有生产调用方。`outline::store::publish` 能把 artifact 写入 `kb_bid_v2_publish_outline`，收尾函数不调用它。生产驱动在收尾之前仍要求 `outline_flow::checked`：phase 为 `complete` 还不够，`analysis.outline.checks` 必须非空且全部通过。新路径只把 `tool_draft.finished` 设为 true 时，驱动返回 `outline completeness check has not passed`，运行也不会因此结束。`Checkpoint::progress` 仍报告 `analysis.outline` 和 `draft_plan`（`outline_requirements`、`outline_chapters`、`outline_scan_repair` 等）。前端 `AnalysisProgress` 也读这些字段。

完成：出场条件改为 `tool_draft.finished` 且投影通过 `validate_artifact`。不再读 `outline_flow::checked` 或 `analysis.outline.checks`。通过后发布 `OutlineArtifact`（章节、槽、附件绑定）。进度来自 `DiscoverWork` 的包计数和 `tool_draft` 的章节、未绑定附件、槽是否已交、是否结束。扫描游标不再代表发现进度。

验证：完成的工具草稿投影后，artifact 的章节 id、槽和绑定与草稿一致，未完成草稿不能发布。`tool_draft.finished` 且 `validate_artifact` 通过时，空的 `analysis.outline.checks` 也不再挡住出场。只有旧 checks 通过、工具草稿未结束时不能发布。只有 `tool_draft` 里有章节时，进度里的章数等于这份草稿，而不是 `draft_plan` 的长度。

## 旧路径退役

已从产品路径删除。配置里没有第二条大纲开关。下列内容不是产品合同：

- `plans/bidding/product-two-phase.md` 保持删除。文档索引不把它写成入口。
- 产品请求只挂 `outline::agent::schemas_for` 的六个工具。`submit_outline_scan`、`put_outline_items`、`submit_outline_check` 不会出现在这份请求里。产品配置调用 `draft::apply` 会拒绝这三个名字，以及经 `outline_flow` 的 `finish_outline`。
- `analysis/outline_flow.rs` 仍只给不经过 `Config` 的抽取单测。产品在 `tool_draft.finished` 时调用 `Journal::publish_outline`。
- 产品收尾是 `outline::agent::apply` 的 `finish_outline`。`draft::after_batch` 在阅读包全部提交后把阶段从 `discover` 拨到 `outline`，并在 `project_draft` 通过时结束。它不再对旧 `outline.checks` 调用 `outline_flow::apply(..., "finish_outline")`。空的 `analysis.outline.checks` 不挡住出场，出场不读 `outline_flow::checked`。
- 双正式编制 / draft-fill、强制终稿复核，以及 T0–T9、A01–A20 那套任务表。
- `phase0_acceptance.sql`、`phase1_acceptance.sql`、`phase1_supersession_acceptance.sql`、`phase3_acceptance.sql`、`phase6_acceptance.sql`。它们调用的文档集 CAS、大纲检查点、报价快照和预览 HTML 已不在 `bidding_v2_baseline.sql`。CI 的 SQL 门是 `scripts/bidding_v2_phase_fixture_acceptance.sh`，只应用 `outline_response_acceptance.sql`。

## 职责分叉

已决定并入组织。一次成稿是发现 → 组织（`put_chapters`、`bind_forms`、`put_slots`、`read_outline`）→ 收尾（`read_outline`、`finish_outline`）。`DraftStage::Fill` 和已发布仍只给 `put_slots` 和 `read_outline`，不能改章节或绑定。六个工具的 schema 形状不动。槽规则不变。见 [运行时](../../docs/bidding/outline.md) 的「职责分叉」。

## 一次成稿验收

下面替换已删除的 A01–A20。对象是一条真实招标的一次成稿大纲运行。缺口 1–3 和旧路径退役已接到代码上。这条清单仍是那次真实运行的验收目标，不是已经跑过的记录。

通过：

1. 冻结解析按解析器顺序发布。大纲运行读的是这份冻结输入。
2. 计划中的每个阅读包变为 `committed`。发现轮 brief 里的包带上切片正文，以及 `[start, end)` 的行优先 `cells`。每条要求的 `source_id`、`start`、`end` 落在该包切片内。续表的 `header` 按列排列，且这些表头不在引用范围内。同一 `running` 或待修 `failed` 包在下一轮 brief 里仍然带正文。超预算时只丢掉已经 `committed` 的发现轮，不读 `analysis.outline.scanned`。
3. 章节树非空、无环、同级顺序不重复。每个附件表恰好绑定一个章节。缺 `requirement_ids` 字段不能通过。有已提交 id 时，每个 id 恰好出现在一个章节上，不能靠各章全空数组过关。没有任何已提交 id 时，各章可以是空数组。组织轮读的是检查点上的要求记录。phase 拨到 `outline` 且 `analysis.outline.checks` 为空时，发现对话已被 `transcript.clear()`，不能再从对话里读要求。
4. 已经 `put_slots`。`bidder_blank` 和 `signature` 的文本为空、`match_query` 非空、`response_required` 为真。其它种类的 `match_query` 为空。每个 `ChapterPurpose::Response` 章节至少有一个槽。
5. `finish_outline` 使 `tool_draft.finished` 为真，phase 为 `complete`。
6. 投影出的 `OutlineArtifact` 通过 `validate_artifact`：版本和身份正确，章节或模板非空，id 不重复，分组章节不带知识库回答槽，回答槽文本为空且带 `match_query`。
7. `outline::store::publish` 写入该 artifact。进度里的包计数、章数、未绑定附件、槽是否已交和是否结束来自 `DiscoverWork` 与 `tool_draft`。空的 `analysis.outline.checks` 不得挡住出场，出场不读 `outline_flow::checked`。

拒绝：

| 情况 | 结果 |
| --- | --- |
| 引用了别的包，或偏移不在本包切片内 | `outside_pack` 或 `outside_slice`，包变为 `failed`，该条要求不入库 |
| 还有未绑定的附件表就 `finish_outline` | 拒绝结束，`finished` 保持 false |
| 分组章节带 `bidder_blank` 或 `signature` | `put_slots` 拒绝 |

## 不做

- 不建长期语义记忆。检查点是记忆，SDK 会话只复用当前窗口。
- 大纲包不读企业知识库。知识库匹配留在 `response`，输入只能是已冻结的 `OutlineArtifact`。
