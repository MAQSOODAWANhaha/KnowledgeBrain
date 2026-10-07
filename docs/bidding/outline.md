# 大纲运行时

产品行为见 [PRD](prd.md)。代码入口是 `crates/bidding/src/outline`。工具合同是 [`outline-tools-v1.schema.json`](../../crates/bidding/schemas/outline-tools-v1.schema.json)，共六个工具。

生产配置在 `Config::environment_limits` 和 `with_provider_for` 里把 `draft_path` 设为 `true`。请求因此走本页的职责和工具。`draft_path = false` 只留给抽取合同的单测，走 `analysis/outline_flow.rs`，不是产品路径。

大纲运行的输入是已经发布的冻结解析。`outline::parse` 里的图片 OCR 并发（`KB_TENDER_PARSE_CONCURRENCY`，默认 4，上限 16）只决定解析时有多少图片同时识别；发布顺序仍必须等于解析器顺序。它不决定发现包怎么领。

## 职责

一轮只有一个职责。模型只看见这个职责的工具（`schemas_for`）。其它名字由 `deny` 拒绝。

现行选择是 `outline::agent::current` / `select`，不看旧的扫描游标：

1. 发现包还没全部提交：发现。
2. 还没有章节，或还有未绑定的附件表：组织。
3. 还没提交过模板槽：模板。
4. 否则：收尾。
5. 阶段已经是填充或已发布：模板。填充阶段的系统提示用 `prompts/template.txt`，工具仍只有 `put_slots` 和 `read_outline`。

| 职责 | 工具 | 提示约束 |
| --- | --- | --- |
| 发现 | `submit_pack`，`read_outline` | 只读已领取的包。同一包失败后才把 `repair` 设为 true |
| 组织 | `put_chapters`，`bind_forms`，`read_outline` | 替换整棵章节树，并把每个附件表绑到唯一章节 |
| 模板 | `put_slots`，`read_outline` | 只抄招标文件已有文字。投标人槽和签字槽留空并带 `match_query` |
| 收尾 | `read_outline`，`finish_outline` | 核对后结束。有未绑定附件表时不能结束 |

`finish_outline` 还要求：章节树非空且无环、同级顺序不重复、已经调用过 `put_slots`、每个 `response` 章节至少有一个槽。成功后 `tool_draft.finished = true`，并把 `outline_run.phase` 和 `analysis.outline.phase` 标成 `complete`。

## 六个工具

| 工具 | 写入 |
| --- | --- |
| `submit_pack` | `pack_id`、`call_id`、`repair`、`requirements[]`（`description`、`source_id`、`start`、`end`） |
| `put_chapters` | 整棵树替换。字段是 `id`、`parent_id`、`order`、`title`、`purpose`（`group` 或 `response`） |
| `bind_forms` | `bindings[]` 的 `form_id`、`chapter_id`。整表替换。同一附件表出现两次会拒绝 |
| `put_slots` | 整表替换。`kind` 为 `fixed_text`、`tender_value`、`instruction`、`bidder_blank`、`signature`、`preserved` |
| `read_outline` | 无参数。返回当前章节、绑定、槽、`slots_submitted`、`finished` |
| `finish_outline` | 无参数。通过上面的门之后把草稿标成结束 |

`bidder_blank` 和 `signature` 的 `text` 必须为空，`match_query` 必须有内容，并且 `response_required` 为真。其它种类的 `match_query` 必须为空。分组章节不能带这两种槽。

换掉章节树时，指向已删除章节的绑定和槽会被丢掉。槽被丢掉后 `slots_submitted` 回到 false，必须再交一次模板。

附件表来自冻结 `structured_forms`：标题同时含「附」和「件」，或该表所在来源的 `heading_path` 含「附件」。

宿主包里的 `outline` 比 `read_outline` 多一个 `unmapped_forms` 列表。工具返回本身不含这个字段。

## 发现：阅读包与并发

`DiscoverWork::plan` 按冻结来源的章节块切包，计划摘要写入 `plan_sha256`。

- 相邻、同一文档、加在一起仍不超过预算的块合成一包。
- 一个章节块超过预算时，先按段落、条款和表格行切开。
- 续表把表头重复成上下文（`header_cells`）。这些表头单元格不在该切片的 `[start, end)` 里，也不再扫一遍。
- 只有一条条款仍然大于预算时，才在 UTF-8 字符边界上按字节切开。

预算是 `reading_budget(pack_max_chars)`。`pack_max_chars` 为 0 时用 8000 字节。

每一轮发现调用 `claim_turn`，最多把 `DEFAULT_PACK_CONCURRENCY`（4）个 `pending` 包标成 `running` 并放进请求 brief 的 `reading_packs`。已经在跑的包保持不动。

`submit_pack` 由宿主拆开：`repair` 为 false 时走 `submit_pack_scan`，为 true 时走 `repair_pack_scan`。修复只接受状态已经是 `failed` 的同一包。

校验失败不写入要求。返回 `{ok: false, feedback}`，包变为 `failed`。`PackFeedback` 含 `pack_id`、`call_id`、参数摘要 `arguments_sha256`、`errors[]`（`path`、`code`、`message`）、`total`、`truncated`。当前构造函数把 `truncated` 设为 false，并返回全部字段错误。参数缺 `pack_id` / `call_id`，或对未失败的包要求修复，是工具错误，不是这条 feedback。

字段错误包括：包不在冻结计划里（`unknown_pack`）、包不是本轮领取或失败待修（`pack_not_running`）、引用了包外来源（`outside_pack`）、偏移不在该标题切片内（`outside_slice`）。

成功则包变为 `committed`，要求描述以 `{pack_id}:{index}` 存入 `DiscoverWork.requirements`。全部包都是 `committed` 时发现完成；空计划也算完成。

## 阅读包载荷：现行与目标

`reading_packs` 是 brief 里的一组会话对象。身份、状态和引用范围以检查点上的 `DiscoverWork` 为准。

**现行：只发偏移。** `DiscoverWork::session` 把 `ParsePack` 放进 `pack`。模型看不到切片文字和单元格。

```json
{
  "duty": "discover",
  "pack": {
    "id": "pack-1",
    "document_id": "doc-1",
    "order": 1,
    "context_heading": "投标文件格式",
    "heading": "附件一 报价表",
    "text": [{ "source_id": "src-12", "start": 400, "end": 880 }],
    "forms": [{ "form_id": "form-3", "start": 6, "end": 12, "header_cells": 3 }]
  },
  "status": "running",
  "feedback": null
}
```

**目标：缺口 1 修好之后的载荷。** 同一对象补上物化文本。`submit_pack` 的要求仍然只提交包内的 `source_id`、`start`、`end`，不把正文写进引用。

| 字段 | 缺口 1 之后必须带上 |
| --- | --- |
| `pack.text[]` | 保留 `source_id`、`start`、`end`，并增加 `text`：该来源 `[start, end)` 的 UTF-8 切片 |
| `pack.forms[]` | 保留 `form_id`、`start`、`end`、`header_cells`，并增加 `cells`：下标落在 `[start, end)` 的单元格文本 |
| `pack.forms[].header` | `header_cells > 0` 时带上被重复的表头单元格文本。这些单元格不在该切片的 `[start, end)` 内 |
| `id`、`document_id`、`order`、`context_heading`、`heading`、`status`、`feedback` | 与现行相同。`context_heading` 是父标题 |

切片本身已经含表头时 `header_cells` 为 0，不另附 `header`。包外文字不出现。`outside_pack` 和 `outside_slice` 继续只检查偏移是否落在这些切片里。

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
| `phase` | `discover`、`outline`、`check`、`complete`。`finish_outline` 把它标成 `complete`。发现包全部提交，或旧扫描被判定完成时，`draft::after_batch` 会从 `discover` 拨到 `outline`。旧扫描完成不是这条产品路径的完成条件 |

`put_chapters` 不接收要求 id，写入的 `requirement_ids` 为空。发现要求还没有进入组织轮的宿主包。这是缺口，见计划，不是本页已经完成的交接。

## 一轮大纲

记忆只有检查点。`TurnJournal` 是 `Checkpoint.journal`（检查点合同版本 14，运行适配 `rig-chat-0.42.0/4`）。`reading_packs`、`tool_draft` 和 `phase` 在同一检查点的 `outline_run` 上。`load` 恢复这一个对象；输入摘要或运行合同变了就拒绝恢复。`save` 写回这一个对象。

1. **选职责。** `outline::agent::current`。本轮只挂该职责的工具。
2. **领包。** 仅当职责是发现且包未完成：`discover::claim_turn` 在还没有计划时建一次计划，把最多 4 个 `pending` 包标成 `running`，会话对象放进 brief 的 `reading_packs`。
3. **准备会话并预约。** `prepare_request` 拼出系统提示、brief、检查点对话，以及宿主包。随后 `TurnJournal::prepare_session` 复用或重建 SDK 会话，`prepare` 把精确 UTF-8 请求体记成待完成轮。`reserve` 在调用模型之前冻结这同一份字节，最多三次。已经保存的响应不再预约，也不再调用模型。
4. **模型。** 返回工具调用。`responded` 先把响应写入检查点。
5. **工具。** `outline::agent::apply` 在 `deny` 下执行。结果追加到检查点对话，`session.finish` 记到当前 SDK 会话。
6. **提交。** 运行结束、角色改变，或序列化后的 SDK 状态超过 `max_context_bytes` 时，丢掉 SDK 会话。大纲草稿和对话留在检查点。`committed` 清掉待完成轮，`save` 再写同一检查点。
7. **职责推进或结束。** 下一轮重新选择职责。发现包全部 `committed` 后阶段可从 `discover` 到 `outline`。`finish_outline` 把 `tool_draft.finished` 和 phase 标成结束。`draft::after_batch` 用旧扫描完成去结束大纲，不属于这条顺序。

宿主包在大纲阶段含 `progress`、`work`、来源索引和 `tool_draft` 的模型视图（比 `read_outline` 多 `unmapped_forms`）。前缀长度是 2（系统提示 + brief），后缀长度是 1（宿主包）。前缀、后缀不变且投影历史等于当前窗口时复用 `AgentRun`；否则按这个窗口重建。

## 上下文窗口

预算是 `max_context_bytes`（整次请求，以及序列化后的 SDK 状态）和 `max_history_bytes`（对话窗口，必须大于 0 且小于 `max_context_bytes`）。没有长期语义记忆，也不为超限另写一份摘要。

每一轮从检查点重放：

- 宿主包每次带上 `progress` 和 `work`。大纲阶段同时带上来源索引和大纲草稿。
- 发现轮要把这一轮仍要处理的包放进 brief：刚领取的，以及尚未 `committed` 的 `running` 和待修 `failed`。SDK 会话被丢掉之后，切片正文仍来自这一次请求。
- 现行 `claim_turn` 只返回本轮从 `pending` 变成 `running` 的包，已经在跑的包不会再次出现。补上仍在处理中的包是缺口 1 的交付要求，存在 `DiscoverWork` 里，不新增存储。

超预算时按下面的顺序缩小窗口：

1. 已交付的旧导航（`source_index`、`search_sources`、`inspect_analysis`）收成一条 `history_omitted` 说明。
2. 丢掉证据已在其它组里的已交付对话组。
3. 裁掉可选的候选回忆。
4. 发现轮装不下预装证据时，丢掉已有持久结论的已完成发现对话，再把预装包减半。现行 `evict_completed_discovery_history` 仍对照 `analysis.outline.scanned`。要求改成：包已经 `committed`、要求已经在检查点上时丢掉对应发现对话。
5. 仍放不下则推迟图片，或失败 `AGENT_TURN_BUDGET_EXCEEDED`，检查点保留。

`prepare_session` 若发现序列化后的 SDK 状态超过 `max_context_bytes`，先按当前窗口重建一次。重建后仍超限，在预约和网络 IO 之前失败，错误同样是 `AGENT_TURN_BUDGET_EXCEEDED`。

## 职责分叉：模板是否并入组织

这是尚未改代码的产品选择。建议在缺口 1–3 落地之前维持四个职责，然后再决定要不要合并。合并不作为修阅读包或发布投影的一部分。

**维持现状。** 模板是独立职责，工具只有 `put_slots` 和 `read_outline`。组织不能写槽。填充和已发布阶段强制回到模板。

**并入组织。** 发现完成之后，同一职责看见 `put_chapters`、`bind_forms`、`put_slots`、`read_outline`。收尾仍只有 `read_outline` 和 `finish_outline`。章节、绑定和槽在同一次职责里提交。

| | 维持四个职责 | 并入组织 |
| --- | --- | --- |
| 隔离 | 写槽时不能换章节树。换树会丢掉被删章节上的绑定和槽，并清掉 `slots_submitted` | 同一职责可以先换树再写槽 |
| 轮次 | 章节和附件都齐之后才进入模板 | 少一次职责切换 |
| 填充阶段 | 已经强制为模板，不能改章节 | 必须单独保住这条限制 |
| 槽规则 | 投标人槽和签字槽留空；分组章节不能带这两种槽 | 规则不变，调用方从模板职责变成组织职责 |

若合并，改的是谁看得见工具，不是工具形状：

- `outline/agent.rs` 的 `ORGANIZE`、`TEMPLATE`、`select`、`current`、`deny`、`schemas_for`。填充阶段仍只给槽。
- `prompts/outline.txt` 写上现在在 `prompts/template.txt` 里的槽规则。填充阶段的提示可以留下。
- `outline-tools-v1.schema.json` 的六个工具形状不动。
- 断言组织轮不能 `put_slots` 的测试。
- 本页的职责表。

## 必须遵守

- 一轮一个职责，模型只看见该职责的工具。
- 投标人槽和签字槽留空，并带 `match_query`。大纲运行不查企业知识库。
- 每个附件表只绑定一个章节。未绑完不能 `finish_outline`。
- `response` 只读冻结的 `OutlineArtifact`。工具是 `read_outline` 和 `put_responses`（[`response-tools-v1.schema.json`](../../crates/bidding/schemas/response-tools-v1.schema.json)）。
- 检查点是记忆。会话只是当前窗口的 SDK 对话。

## 还不是现行行为

下面三项代码里还没有接通。不要把它们写成发现、组织或发布已经做到的事。任务和验证在 [实施计划](../../plans/bidding/outline-gaps.md)。

- 阅读包发给模型的是偏移。上文「目标」载荷和「仍在处理中的包每次重放」都还没接通。
- 已提交的要求没有进入组织。
- `tool_draft` 还没有投影成 `OutlineArtifact` 再发布。`Checkpoint::progress` 仍在报分析侧的旧计数字段。
