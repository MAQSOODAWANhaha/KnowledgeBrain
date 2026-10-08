# 大纲运行时

产品行为见 [PRD](prd.md)。代码入口是 `crates/bidding/src/outline`。工具合同是 [`outline-tools-v1.schema.json`](../../crates/bidding/schemas/outline-tools-v1.schema.json)，共六个工具。

产品请求走本页的职责和六个工具。一次成稿是唯一的大纲路径。`analysis/outline_flow.rs` 里的扫描、检查工具只留给不经过配置的抽取单测，不是产品请求。

大纲运行的输入是已经发布的冻结解析。`outline::parse` 里的图片 OCR 并发（`KB_TENDER_PARSE_CONCURRENCY`，默认 4，上限 16）只决定解析时有多少图片同时识别；发布顺序仍必须等于解析器顺序。它不决定发现包怎么领。

## 职责

一轮只有一个职责。模型只看见这个职责的工具（`schemas_for`）。其它名字由 `deny` 拒绝。

现行选择是 `outline::agent::current` / `select`。`select` 先看 `DraftStage`，不看旧的扫描游标，也不要调用 `outline::agent::duty`。`duty` 把 phase `outline` 固定成组织；槽已经提交、尚未 `finish_outline` 时会把收尾选成组织。

1. `DraftStage::Fill` 或已发布：只写槽。这不是 `response` 的知识库填充，也不是一次成稿里的组织职责。该阶段的系统提示用 `prompts/template.txt`，工具只有 `put_slots` 和 `read_outline`。不能 `put_chapters`，不能 `bind_forms`。
2. 发现包还没全部提交：发现。
3. 还没有章节，或还有未绑定的附件表，或还没提交过槽，或有应答章节没有槽：组织。同一职责看见 `put_chapters`、`bind_forms`、`put_slots`、`read_outline`。
4. 否则：收尾。工具只有 `read_outline` 和 `finish_outline`。

一次成稿因此是发现 → 组织（章节、绑定、槽）→ 收尾。`phase` 是检查点上的阶段。职责是这一轮给模型的工具集，由 `current` / `select` 决定。

| `phase` | 这一阶段的职责 |
| --- | --- |
| `discover` | 发现。阅读包还没全部 `committed` |
| `outline` | 先组织，再收尾。章节未齐、附件未绑完、槽还没交，或应答章节没有槽，是组织；这些都齐是收尾 |
| `check` | 收尾。旧路径会把 phase 写成 `check`。一次成稿的 `finish_outline` 直接写成 `complete` |
| `complete` | 大纲已结束。`DraftStage` 仍是大纲时职责是收尾；`DraftStage` 已是 `Fill` 或已发布时职责是只写槽 |

| 职责 | 工具 | 提示约束 |
| --- | --- | --- |
| 发现 | `submit_pack`，`read_outline` | 只读已领取的包。同一包失败后才把 `repair` 设为 true |
| 组织 | `put_chapters`，`bind_forms`，`put_slots`，`read_outline` | 替换整棵章节树，把每个附件表绑到唯一章节，并写入规定槽。每个应答章节至少有一个槽。投标人槽和签字槽留空并带 `match_query`。分组章节不能带这两种槽 |
| 收尾 | `read_outline`，`finish_outline` | 核对后结束。有未绑定附件表时不能结束。不改章节，不改槽 |
| 填槽（`Fill` / 已发布） | `put_slots`，`read_outline` | 只抄招标文件已有文字。不能改章节，不能改绑定 |

`finish_outline` 还要求：章节树非空且无环、同级顺序不重复、已经调用过 `put_slots`、每个 `ChapterPurpose::Response` 章节至少有一个槽。一次成稿的 `put_slots` 在还有应答章节没有槽时拒绝，不把 `slots_submitted` 写成 true，职责留在组织。已经交过槽后又多出一个没有槽的应答章节，职责回到组织。`finish_outline` 被拒绝时检查点记下 `finish_rejected`，下一轮职责回到组织，不再挂出 `finish_outline`。之后组织成功写入章节、绑定或槽，这次拒绝撤销，结构齐全时可以再进入收尾。成功后 `tool_draft.finished = true`，并把 `outline_run.phase` 和 `analysis.outline.phase` 标成 `complete`。

## 六个工具

| 工具 | 写入 |
| --- | --- |
| `submit_pack` | `pack_id`、`call_id`、`repair`、`requirements[]`（`description`、`source_id`、`start`、`end`） |
| `put_chapters` | 整棵树替换。必填 `id`、`parent_id`、`order`、`title`、`purpose`（`group` 或 `response`）、`requirement_ids`（字符串数组，允许空数组）。未知 id 拒绝；已提交的 id 必须各出现在恰好一个章节上 |
| `bind_forms` | `bindings[]` 的 `form_id`、`chapter_id`。整表替换。同一附件表出现两次会拒绝 |
| `put_slots` | 整表替换。`kind` 为 `fixed_text`、`tender_value`、`instruction`、`bidder_blank`、`signature`、`preserved` |
| `read_outline` | 无参数。返回当前章节、绑定、槽、`slots_submitted`、`finished` |
| `finish_outline` | 无参数。通过上面的门之后把草稿标成结束 |

`bidder_blank` 和 `signature` 的 `text` 必须为空，`match_query` 必须有内容，并且 `response_required` 为真。其它种类的 `match_query` 必须为空。分组章节不能带这两种槽。

换掉章节树时，指向已删除章节的绑定和槽会被丢掉。槽被丢掉后 `slots_submitted` 回到 false，同一组织职责必须再交一次槽。

附件表来自冻结 `structured_forms`：标题同时含「附」和「件」，或该表所在来源的 `heading_path` 含「附件」。

宿主包里的 `outline` 比 `read_outline` 多一个 `unmapped_forms` 列表。工具返回本身不含这个字段。

## 发现：阅读包与并发

`DiscoverWork::plan` 按冻结来源的章节块切包，计划摘要写入 `plan_sha256`。

- 相邻、同一文档、加在一起仍不超过预算的块合成一包。
- 一个章节块超过预算时，先按段落、条款和表格行切开。
- 续表把表头重复成上下文（`header_cells`）。这些表头单元格不在该切片的 `[start, end)` 里，也不再扫一遍。
- 只有一条条款仍然大于预算时，才在 UTF-8 字符边界上按字节切开。

预算是 `reading_budget(pack_max_chars)`，直接使用 `Limits.pack_max_chars`。部署缺省写在 `deploy/.env.example` 的 `KB_TENDER_AGENT_LIMITS.pack_max_chars`（8000 字节）。代码里没有另一份 8000 回退。省略该键时 serde 缺省是 0，产品环境拒绝小于 1200 的值。

每一轮 brief 里同时处于 `running` 或待修 `failed` 的包不超过 `DEFAULT_PACK_CONCURRENCY`（4）。`claim` 只补满这个名额里的空位，按包的 `order` 领取，不按包 id 的字符串序。请求 brief 的 `reading_packs` 就是这些在途包。已 `committed` 的包不再出现。试装（`fit_batch`、检查）调用的 `prepare_request` 不留下额外领取。

`submit_pack` 由宿主拆开：`repair` 为 false 时走 `submit_pack_scan`，为 true 时走 `repair_pack_scan`。修复只接受状态已经是 `failed` 的同一包。

校验失败不写入要求。返回 `{ok: false, feedback}`，包变为 `failed`。`PackFeedback` 含 `pack_id`、`call_id`、参数摘要 `arguments_sha256`、`errors[]`（`path`、`code`、`message`）、`total`、`truncated`。当前构造函数把 `truncated` 设为 false，并返回全部字段错误。参数缺 `pack_id` / `call_id`，或对未失败的包要求修复，是工具错误，不是这条 feedback。

字段错误包括：包不在冻结计划里（`unknown_pack`）、包不是本轮领取或失败待修（`pack_not_running`）、引用了包外来源（`outside_pack`）、偏移不在该标题切片内（`outside_slice`）。

成功则包变为 `committed`。要求以 `{pack_id}:{index}` 写入 `DiscoverWork.requirements`，记录留下描述以及 `source_id`、`start`、`end`。全部包都是 `committed` 时发现完成；空计划也算完成。

## 阅读包载荷

`reading_packs` 是 brief 里的一组会话对象。身份、状态和引用范围以检查点上的 `DiscoverWork` 为准。`DiscoverWork::session` 在发给模型时物化切片正文；检查点仍只存偏移。`submit_pack` 的要求仍然只提交包内的 `source_id`、`start`、`end`，不把正文写进引用。

| 字段 | 内容 |
| --- | --- |
| `pack.text[]` | `source_id`、`start`、`end`，以及 `text`：该来源 `[start, end)` 的 UTF-8 切片 |
| `pack.forms[]` | `form_id`、`start`、`end`、`header_cells`，以及 `cells`：`[start, end)` 的单元格文本，按行优先排成一维数组。长度等于 `end - start` |
| `pack.forms[].header` | `header_cells > 0` 时带上被重复的表头文本，按列顺序，长度等于 `header_cells`。这些单元格不在该切片的 `[start, end)` 内。`header_cells` 为 0 时不带 `header` |
| `id`、`document_id`、`order`、`context_heading`、`heading`、`status`、`feedback` | 包身份和本轮状态。`context_heading` 是父标题 |

`cells` 不使用 `{row, column, text}`。有 `header` 时列数等于它的长度；下面的例子是 3 列、2 行，`["1", "人工", ""]` 是第一行。切片本身已经含表头时 `header_cells` 为 0，不另附 `header`，表头行就在 `cells` 里。包外文字不出现。`outside_pack` 和 `outside_slice` 继续只检查偏移是否落在这些切片里。

```json
{
  "duty": "discover",
  "pack": {
    "id": "pack-1",
    "document_id": "doc-1",
    "order": 1,
    "context_heading": "投标文件格式",
    "heading": "附件一 报价表",
    "text": [
      { "source_id": "src-12", "start": 400, "end": 880, "text": "……该切片正文……" }
    ],
    "forms": [
      {
        "form_id": "form-3",
        "start": 6,
        "end": 12,
        "header_cells": 3,
        "header": ["序号", "项目", "报价"],
        "cells": ["1", "人工", "", "2", "材料", ""]
      }
    ]
  },
  "status": "running",
  "feedback": null
}
```

## 状态

检查点上的 `OutlineRun` 保存这条运行：

| 字段 | 内容 |
| --- | --- |
| `reading_packs` | `DiscoverWork`。第一次发现轮才建立。包状态是 `pending`、`running`、`failed`、`committed` |
| `tool_draft` | `Draft`：`chapters`、`bindings`、`slots`、`slots_submitted`、`finished` |
| `phase` | `discover`、`outline`、`check`、`complete`。`finish_outline` 把它标成 `complete`。阅读包全部 `committed` 之后，`draft::after_batch` 从 `discover` 拨到 `outline`。此时若 `analysis.outline.checks` 为空，会 `transcript.clear()`。组织必须读检查点上的要求记录，不能指望发现对话还在 |

每个章节的 `requirement_ids` 必填。组织轮宿主包带上已提交要求的身份、描述、`source_id`、`start`、`end`。发现职责不能调用 `put_chapters`。

## 一轮大纲

记忆只有检查点。`TurnJournal` 是 `Checkpoint.journal`（检查点合同版本 14，运行适配 `rig-chat-0.42.0/4`）。`reading_packs`、`tool_draft` 和 `phase` 在同一检查点的 `outline_run` 上。`load` 恢复这一个对象；输入摘要或运行合同变了就拒绝恢复。`save` 写回这一个对象。

1. **选职责。** `outline::agent::current`。本轮只挂该职责的工具。
2. **领包。** 仅当包未完成，在还没有计划时建一次计划。`claim_turn` 使本轮在途包（`running` 加待修 `failed`）不超过 `DEFAULT_PACK_CONCURRENCY`，按 `order` 补满空位，并把这些包放进 brief。
3. **准备会话并预约。** `prepare_request` 拼出系统提示、brief、检查点对话，以及宿主包。随后 `TurnJournal::prepare_session` 复用或重建 SDK 会话，`prepare` 把精确 UTF-8 请求体记成待完成轮。`reserve` 在调用模型之前冻结这同一份字节，最多三次。已经保存的响应不再预约，也不再调用模型。
4. **模型。** 返回工具调用。`responded` 先把响应写入检查点。
5. **工具。** `outline::agent::apply` 在 `deny` 下执行。结果追加到检查点对话，`session.finish` 记到当前 SDK 会话。
6. **提交。** 运行结束、角色改变，或序列化后的 SDK 状态超过 `max_context_bytes` 时，丢掉 SDK 会话。大纲草稿和对话留在检查点。`committed` 清掉待完成轮，`save` 再写同一检查点。
7. **职责推进或结束。** 下一轮重新选择职责。发现包全部 `committed` 后阶段从 `discover` 到 `outline`。`finish_outline` 把 `tool_draft.finished` 和 phase 标成 `complete`。出场条件是 `tool_draft.finished` 且 `project_draft` 通过 `validate_artifact`。空的 `analysis.outline.checks` 不挡住出场，出场不读 `outline_flow::checked`。通过后 `Journal::publish_outline` 发布投影出的 `OutlineArtifact` 和附件绑定。

`draft_stage` 为 `None` 或 `Outline` 时（含发现轮），宿主包含 `progress`、`work`、来源索引和 `tool_draft` 的模型视图（比 `read_outline` 多 `unmapped_forms`）。组织轮还要带上检查点里的要求记录：身份、描述、`source_id`、`start`、`end`。这不按 phase 名字开关。前缀长度是 2（系统提示 + brief），后缀长度是 1（宿主包）。前缀、后缀不变且投影历史等于当前窗口时复用 `AgentRun`；否则按这个窗口重建。

## 上下文窗口

预算是 `max_context_bytes`（整次请求，以及序列化后的 SDK 状态）和 `max_history_bytes`（对话窗口，必须大于 0 且小于 `max_context_bytes`）。没有长期语义记忆，也不为超限另写一份摘要。

每一轮从检查点重放：

- 宿主包每次带上 `progress` 和 `work`。`draft_stage` 为 `None` 或 `Outline` 时同时带上来源索引和大纲草稿，发现轮也包括在内。
- 发现轮要把这一轮仍要处理的包放进 brief：刚领取的，以及尚未 `committed` 的 `running` 和待修 `failed`。SDK 会话被丢掉之后，切片正文仍来自这一次请求。

超预算时按下面的顺序缩小窗口：

1. 已交付的旧导航（`source_index`、`search_sources`、`inspect_analysis`）收成一条 `history_omitted` 说明。
2. 丢掉证据已在其它组里的已交付对话组。
3. 裁掉可选的候选回忆。
4. 发现轮装不下预装证据时，丢掉已经 `committed` 的包所在的发现轮，再把预装包减半。依据是 `DiscoverWork` 的包状态，不读 `analysis.outline.scanned`。
5. 仍放不下则推迟图片。
6. 仍放不下且在途阅读包多于一个时，把 `order` 最大的 `running` 或 `failed` 包放回 `pending`（清掉 feedback，attempt 减一），再装一次。只剩一个在途包仍放不下才失败 `AGENT_TURN_BUDGET_EXCEEDED`，检查点保留。试装通过后恢复调用前的包状态，不把这次放回写进检查点。

`prepare_session` 若发现序列化后的 SDK 状态超过 `max_context_bytes`，先按当前窗口重建一次。重建后仍超限，在预约和网络 IO 之前失败，错误同样是 `AGENT_TURN_BUDGET_EXCEEDED`。

## 职责分叉：模板并入组织

已决定并入组织。一次成稿在发现完成之后只有一个职责，看见 `put_chapters`、`bind_forms`、`put_slots`、`read_outline`。收尾仍只有 `read_outline` 和 `finish_outline`。章节、绑定和槽在同一次职责里提交。同一职责可以先换章节树再写槽；换树会丢掉被删章节上的绑定和槽，并清掉 `slots_submitted`。

`DraftStage::Fill` 和已发布不走这次合并。工具仍只有 `put_slots` 和 `read_outline`，不能 `put_chapters` 或 `bind_forms`。系统提示仍用 `prompts/template.txt`。

槽规则不变。投标人槽和签字槽留空并带 `match_query`；其它种类的 `match_query` 为空；分组章节不能带这两种槽。`outline-tools-v1.schema.json` 的六个工具形状不动。

## 必须遵守

- 一轮一个职责，模型只看见该职责的工具。
- 投标人槽和签字槽留空，并带 `match_query`。大纲运行不查企业知识库。
- 每个附件表只绑定一个章节。未绑完不能 `finish_outline`。
- `response` 只读冻结的 `OutlineArtifact`。工具是 `read_outline` 和 `put_responses`（[`response-tools-v1.schema.json`](../../crates/bidding/schemas/response-tools-v1.schema.json)）。
- 检查点是记忆。可恢复的是 `DiscoverWork`、`tool_draft` 和 `phase`。对话不是那份可恢复记忆。会话只是当前窗口的 SDK 对话。

## 进度

产品路径的进度来自 `DiscoverWork` 和 `tool_draft`：包的总数、待领、在跑、失败、已提交，章节数，未绑定附件数，槽是否已交，草稿是否结束。有阅读包时，发现阶段的停滞观察读包计数和要求条数，不读 `analysis.outline.scanned`。组织、收尾和完成读 `tool_draft`。有阅读包或工具草稿时，章数是 `tool_draft.chapters` 的长度，不是 `draft_plan` 的长度。扫描游标不再代表发现进度。

一次成稿没有回合上限，也没有阶段墙钟，也不用累计的 `max_tool_calls` 或 `max_read_bytes` 截停。`Limits::at_least_for` 不改写 `max_turns`，不按 `max_turns` 放大工具调用或读字节，只把 `reviewer_reserve` 清成 0。已经有阅读包、且 `draft_stage` 仍是大纲时，这三个累计额度都不结束循环，SDK 会话也不再按剩余回合数封顶。单次请求仍受 `max_context_bytes`、`max_context_tokens` 和 `max_tool_result_bytes` 约束，装不下就缩小这一次请求。没有阅读包的旧扫描路径仍受调用方写明的 `max_turns`、`max_tool_calls` 和 `max_read_bytes` 约束；那条路径只留给不经过配置的抽取单测。

卡住时只看 `Limits` 里的 `max_no_progress_turns`、`max_focus_turns`、`max_focus_replans`。观察进入 `Blocked` 后运行结束。错误码仍是 `AGENT_TURN_BUDGET_EXCEEDED`，正文是 `outline stalled in phase {phase}`，`phase` 取检查点上的 `discover`、`outline`、`check` 或 `complete`。这是逻辑停滞，不是回合预算。

编制页按 `outline_phase` 显示发现、组织（章节和模板槽）、核对、完成。`outline` 且章节、附件绑定或槽还没齐时是组织；三者都齐，以及 `check`，是核对；`complete` 是完成。页面文案只写这四个阶段。应答章节缺槽时职责仍是组织，即使 `slots_submitted` 已经为真。

## 尚未决定

职责分叉已决定为并入组织，见上一节。阅读包、要求进组织、`tool_draft` 投影发布，以及这次合并，都已经接通。回归合同仍是 [实施计划](../../plans/bidding/outline-gaps.md) 的验证和一次成稿验收。
