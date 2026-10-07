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

## 状态

检查点上的 `OutlineRun` 保存这条运行：

| 字段 | 内容 |
| --- | --- |
| `reading_packs` | `DiscoverWork`。第一次发现轮才建立。包状态是 `pending`、`running`、`failed`、`committed` |
| `tool_draft` | `Draft`：`chapters`、`bindings`、`slots`、`slots_submitted`、`finished` |
| `phase` | `discover`、`outline`、`check`、`complete`。`finish_outline` 把它标成 `complete`。发现包全部提交，或旧扫描被判定完成时，`draft::after_batch` 会从 `discover` 拨到 `outline`。旧扫描完成不是这条产品路径的完成条件 |

`put_chapters` 不接收要求 id，写入的 `requirement_ids` 为空。发现要求还没有进入组织轮的宿主包。这是缺口，见计划，不是本页已经完成的交接。

## 会话与检查点

没有长期语义记忆。记忆是 Journal 里的检查点。

- `load` 恢复检查点。输入摘要或运行合同变了就拒绝恢复。
- `reserve` 在发出 HTTP 之前冻结这一轮的请求字节。
- `save` 在工具提交之后写回检查点。

`prepare_session` 在前缀、后缀不变，且投影出的历史等于 SDK `AgentRun` 的 `full_history` 时复用会话；否则按当前窗口重建。进度、工作笔记、来源索引、大纲草稿和本轮领取的阅读包都在当次请求里重放，不另存一份业务记忆。

## 必须遵守

- 一轮一个职责，模型只看见该职责的工具。
- 投标人槽和签字槽留空，并带 `match_query`。大纲运行不查企业知识库。
- 每个附件表只绑定一个章节。未绑完不能 `finish_outline`。
- `response` 只读冻结的 `OutlineArtifact`。工具是 `read_outline` 和 `put_responses`（[`response-tools-v1.schema.json`](../../crates/bidding/schemas/response-tools-v1.schema.json)）。
- 检查点是记忆。会话只是当前窗口的 SDK 对话。

## 还不是现行行为

下面三项代码里还没有接通。不要把它们写成发现、组织或发布已经做到的事。任务和验证在 [实施计划](../../plans/bidding/outline-gaps.md)。

- 阅读包发给模型的是偏移，不是切片正文。
- 已提交的要求没有进入组织。
- `tool_draft` 还没有投影成 `OutlineArtifact` 再发布。`Checkpoint::progress` 仍在报分析侧的旧计数字段。
