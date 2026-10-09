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
| `check` | 收尾。草稿可结束时（章节、附件、槽都齐，应答章节都有槽，且没有被拒绝的 `finish_outline`）阶段写成 `check`。`finish_outline` 成功写成 `complete`；被拒绝则回到 `outline` |
| `complete` | 大纲已结束。`DraftStage` 仍是大纲时职责是收尾；`DraftStage` 已是 `Fill` 或已发布时职责是只写槽 |

| 职责 | 工具 | 提示约束 |
| --- | --- | --- |
| 发现 | `submit_pack`，`read_outline` | 应答义务是投标人必须提交、填写、声明、承诺、报价、列偏差、提供资格证明或按指定格式作答的事项。读完已领取的包再 `submit_pack`。整包没有应答义务时 `requirements` 用空数组。同一包失败后才把 `repair` 设为 true |
| 组织 | `put_chapters`，`bind_forms`，`put_slots`，`read_outline` | 替换整棵章节树，把每个附件表绑到唯一章节，并写入规定槽。有两张及以上互不续表的附件表时，树要有三层：根分组（可多个，即森林）、中间分组、叶子应答。中间层是 `group`，沿用招标文件自己的章节，不要为每一张附件表设一个应答章。同一条续表链只绑一个叶子；互不续表的附件表可以绑在同一个应答章。每个应答章节至少有一个槽。投标人槽和签字槽留空并带 `match_query`。分组章节不能带这两种槽 |
| 收尾 | `read_outline`，`finish_outline` | `readiness.ready` 为 true 时下一步是 `finish_outline`。核对结果在 `readiness` 里，不要反复 `read_outline`。不改章节，不改槽 |
| 填槽（`Fill` / 已发布） | `put_slots`，`read_outline` | 只抄招标文件已有文字。不能改章节，不能改绑定 |

`finish_outline` 还要求：章节树非空且无环、同级顺序不重复、已经调用过 `put_slots`、每个 `ChapterPurpose::Response` 章节至少有一个槽。附件链有两条及以上时，每个应答章节的父章节必须是 `group`，并且从根数起深度至少为 3（根分组（可多个，即森林）、中间分组、叶子应答，见 `RESPONSE_LEAF_DEPTH` 与 `DEPTH_CHAIN_MIN`）。同一条续表链只绑一个叶子；互不续表的附件可以绑在同一个应答章。链按表头延续判定，不读标题：换了标题路径，或两表之间有非空正文，就不是同一条链。一条链时仍可以是两层。未绑定的附件卡片带 `chain`，同一条链的表要绑到同一个叶子。一次成稿的 `put_slots` 在还有应答章节没有槽时拒绝，不把 `slots_submitted` 写成 true，职责留在组织。已经交过槽后又多出一个没有槽的应答章节，职责回到组织。`finish_outline` 被拒绝时检查点记下 `finish_rejected`，阶段回到 `outline`，下一轮职责回到组织，不再挂出 `finish_outline`。草稿能过工具门、却过不了发布校验时同样拒绝，不进入 `complete`。拒绝理由就是发布校验的那一句，例如章节 id `cover` 是保留字。之后组织成功写入章节、绑定或槽，这次拒绝撤销。结构齐全时阶段写成 `check`，职责进入收尾。成功后 `tool_draft.finished = true`，并把 `outline_run.phase` 和 `analysis.outline.phase` 标成 `complete`。进度里的 `outline_phase` 因此会经过 `check`，不会从 `outline` 直接跳到 `complete`。

## 六个工具

| 工具 | 写入 |
| --- | --- |
| `submit_pack` | `pack_id`、`call_id`、`repair`、`requirements[]`（`description`、`source_id`、`start`、`end`） |
| `put_chapters` | 整棵树替换。必填 `id`、`parent_id`、`order`、`title`、`purpose`（`group` 或 `response`）、`requirement_ids`（字符串数组，允许空数组）。未知 id、重复 id、未挂上的已提交 id，同一次调用全部报出 |
| `bind_forms` | `bindings[]` 的 `form_id`、`chapter_id`。整表替换。同一附件表出现两次会拒绝。分组章节不能接收附件表 |
| `put_slots` | 整表替换。`kind` 为 `fixed_text`、`tender_value`、`instruction`、`bidder_blank`、`signature`、`preserved` |
| `read_outline` | 无参数。返回当前章节、绑定、槽、`slots_submitted`、`finished`，以及 `readiness` |
| `finish_outline` | 无参数。通过上面的门之后把草稿标成结束 |

`bidder_blank` 和 `signature` 的 `text` 必须为空，`match_query` 必须有内容，并且 `response_required` 为真。其它种类的 `match_query` 必须为空。分组章节不能带这两种槽。

换掉章节树时，指向已删除章节的绑定和槽会被丢掉。槽被丢掉后 `slots_submitted` 回到 false，同一组织职责必须再交一次槽。

附件表来自冻结 `structured_forms` 的表格结构和它在来源顺序里的位置。检测不读标题，也不匹配标题或章节里的用词。

满足下面任一条件即是附件表：

1. 网格至少有两个锚点、其中一个有内容，并且出现至少两处行内占位（连续三个 `_`、`.`、`-`、`~`、`…`、`□` 一类笔画，空白不打断），或者至少一半锚点是空白或占位，或者首行有内容且其后至少一半锚点是空白或占位。
2. 表头行下有空白列。表头单元格有内容，且该列在表头之下的空格多于有内容的格子。有两列及以上这样的列时，其余格子带数字也不排除。只有一列时，表体里有内容的格子都不超过 12 个字符，并且表体里带数字的格子不到三分之一。第一行若是一个跨满列的标题格，表头改看下一行。
3. 该表是某一非空标题下的唯一表，该标题下正文不超过 80 个字符，表体至少三分之一是空白或占位，表体里有内容的格子都不超过 12 个字符，并且表体里带数字的格子不到三分之一。

表头之下，单独一个锚点并且 `col_span` 盖住全部列的行是表尾，不参与空白列和表体填充的计数。它不把每一列都涂成有内容，也不多算一个已填格子。表体里超过 12 个字符的格子只有一个时，这一格是行旁注记，不拿它的长度去否决空白列或续表；有两格及以上长文时，最长的那格仍然否决。

续表跟表头表走。同一文档里、中间没有别的表、列数相同，并且后一张表重复前一张的表头，或后一张表的表头整行是空的，它们是同一条链。链上只要有一张表自己满足上面的条件，链上其余的表也是附件表。编号参数表和长要求行不从邻居继承：按上面的规则，阻断用的表体长度超过 12 个字符，或表体里带数字的格子达到三分之一。

来源定位器自己的 `heading_path` 为空时，标题取同一文档里顺序在前、最近一条非空 `heading_path`。页表因此跟在它前面的章节后面，不必把标题写进页表定位器。合成标题（例如 `source_unit:` 加来源 id）不参与判断。样本冻结仍会把结构标题写进定义。优先用第一行跨满列、且不是页眉页脚也不是单元注的格子。否则在前一条来源里，从靠近表格的一行往前找。页眉页脚是同一行或标题路径里的同一段出现在至少两页上，不限页首页尾；同一页里重复不算。不超过短标签上限的页首或页尾行，如果只多一个一至三位的页码，去掉页码后与另一页相同，也算同一条页眉；页中的编号标题不去页码。页码取定位器的 `page_ordinal`；章节正文没有这个字段时，取来源 key 里的 `page:<n>`。比较时去掉行首的 `#`。索引行即使跨页重复也保留，不当事页眉。单元注是整行包在括号里，或者只有一个冒号、冒号两侧都不超过 12 个字符、且整行没有数字（与上面的短标签上限相同）。带数字的行是索引，不是单元注。编号行即使带冒号也保留。跳过句中已经出现句号、问号或叹号的残句，括号不成对的行，括号外出现逗号、顿号或分号的分句，也跳过以闭括号或逗号一类收束符开头的行，以及超过 80 字或以句末标点结束的行。索引行是以数字或「一二三…」加顿号、点、括号开头，或含有一至三位数字、后面最多一个字母的记号（不含四位年份，不含小数）。候选窗口是上一张表之后的全部正文，不只最后一条来源。有索引行时用最靠近表格的那一行；短索引（不超过短标签上限的一半）若窗口里有更长的索引行，则用那条更长的，否则让给窗口里更长的一行。短索引单独出现时保留。跨满列的短索引同样让位。不超过短标签上限、且不是索引的标题路径段，窗口里还有别的可用行时不当标题。上一张表之后的正文如果全是页眉或单元注，而这张表与前一张列数相同、表头相同或表头整行为空，标题沿用前一张。那只是冻结记录，检测不读它。解析器已经判过的页眉页脚写在 `metadata.repeating_lines`（短边行、至少四页正文、出现在不少于 60% 的正文页上）。冻结标题把这份集合并进页眉。整行带页码时（末尾一至三位数字，或「第」加数字再加可选的「页」）每一页都不同，进不了那份 60% 集合；短行去掉这个页码后再比较，出现在至少两页上才是页眉，不限页首。不超过短标签上限、且不是索引的行，去掉空格后相同即算同一条。短行若只是表格里一个非跨列字段（长度不超过短索引上限），不当标题：窗口里还有别的可用行就用那一行，否则标题为空。短索引让给更长的续写：标题路径、单元格，或同一行若干格拼起来的文字；去掉空格后以前缀开头，或共用一个带字母的索引记号（单独一位数字不算）。不超过短标签上限、且不是索引的行，若比活动标题路径里的某一段短，并且是该段去掉行首 `#` 之后的连续子串，则与短标题横幅一样让位。没有跨满列的首行标题格、让位之后仍只剩这个子串时，标题为空；跨满列的标题格即使落在标题段里也保留。短索引续写时，仅因末尾一个句末标点而不可用的行，去掉这个标点后再套前缀和带字母记号；这两条都对不上时，才看短索引紧后面的第一条非页眉行，该行必须是去掉句末标点后变可用并且更长，中间的页眉跳过，再往后不看。禁止硬编码：这些规则只用短标签上限、页眉重复、页码形状、标题路径和句末标点，不写文档专用词。`bb056cd` 再冻 106 页 PDF 时 468 仍是 `投标文件`、545 仍是 `附件 8F`，`repeating_lines` 仍为空：页码折叠和字段格的前提不成立，短行是活动标题段的子串，短索引下一行以句号结束所以进不了续写池。上面的子串让位和句末续写是针对这次失配补的，还没有用那份 PDF 再冻一次，不能写成已经干净。

宿主包里的 `outline` 比工具返回多一个 `unmapped_forms` 页。每一项是一张卡片：`form_id`、`title`、`header`（表头行）、`source_id`、`ordinal`、`page`、`heading`（最近的前一标题）。页按来源顺序排，预算是宿主包 `max_bytes` 的一半，装不下的留在 `next`。来源索引那一页不必翻到这些序号。`readiness` 在宿主包和每次工具返回里都有：`ready`、`missing`，齐了才有 `next: finish_outline`。

## 发现：阅读包与并发

`DiscoverWork::plan` 按冻结来源的章节块切包，计划摘要写入 `plan_sha256`。

- 相邻、同一文档、加在一起仍不超过预算的块合成一包。
- 一个章节块超过预算时，先按段落、条款和表格行切开。
- 续表把表头重复成上下文（`header_cells`）。这些表头单元格不在该切片的 `[start, end)` 里，也不再扫一遍。
- 只有一条条款仍然大于预算时，才在 UTF-8 字符边界上按字节切开。

预算是 `reading_budget(pack_max_chars)`，直接使用 `Limits.pack_max_chars`。部署缺省写在 `deploy/.env.example` 的 `KB_TENDER_AGENT_LIMITS.pack_max_chars`（8000 字节）。代码里没有另一份 8000 回退。省略该键时 serde 缺省是 0，产品环境拒绝小于 1200 的值。

每一轮 brief 里同时处于 `running` 或待修 `failed` 的包不超过 `DEFAULT_PACK_CONCURRENCY`（4）。`claim` 只补满这个名额里的空位，按包的 `order` 领取，不按包 id 的字符串序。请求 brief 的 `reading_packs` 就是这些在途包。已 `committed` 的包不再出现。试装（`fit_batch`、检查）调用的 `prepare_request` 不留下额外领取。

`submit_pack` 由宿主拆开：`repair` 为 false 时走 `submit_pack_scan`，为 true 时走 `repair_pack_scan`。修复只接受状态已经是 `failed` 的同一包。包 id 不在计划里时，错误是 `unknown_pack`，并列出当前失败包的 id。已知但还没失败的包仍是「requires a failed reading pack」。

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
- 检查点是记忆。可恢复的是 `DiscoverWork`、`tool_draft` 和 `phase`。对话不是那份可恢复记忆。会话只是当前窗口的 SDK 对话。供应商用量累加在 `journal.usage`。会话在运行结束时丢掉，用量还在。`analysis-result.json` 带同一份累计用量，并在 `finish_outline` 成功后带上已发布的大纲（章节、槽、附件绑定）。用量全空或大纲尚未结束时这两个字段不写出。检查点合同版本仍是 14。

## 进度

产品路径的进度来自 `DiscoverWork` 和 `tool_draft`：包的总数、待领、在跑、失败、已提交，章节数，未绑定附件数，槽是否已交，草稿是否结束。有阅读包时，发现阶段的停滞观察读包计数和要求条数，不读 `analysis.outline.scanned`。组织、收尾和完成读 `tool_draft`。有阅读包或工具草稿时，章数是 `tool_draft.chapters` 的长度，不是 `draft_plan` 的长度。扫描游标不再代表发现进度。

一次成稿没有回合上限，也没有阶段墙钟，也不用累计的 `max_tool_calls` 或 `max_read_bytes` 截停。`Limits::at_least_for` 不改写 `max_turns`，不按 `max_turns` 放大工具调用或读字节，只把 `reviewer_reserve` 清成 0。`max_turns` 为 0 表示没有回合上限，校验接受它；一次成稿本来就不读这个数。修复任务的回合上限在这时只看焦点预算（`max_focus_turns` 乘 `max_focus_replans + 1`），不把 0 当成上限。已经有阅读包、且 `draft_stage` 仍是大纲时，这三个累计额度都不结束循环，SDK 会话也不再按剩余回合数封顶。单次请求仍受 `max_context_bytes`、`max_context_tokens` 和 `max_tool_result_bytes` 约束，装不下就缩小这一次请求。没有阅读包的旧扫描路径仍受调用方写明的正数 `max_turns`、`max_tool_calls` 和 `max_read_bytes` 约束；`max_turns` 为 0 在那条路径上同样不是上限。那条路径只留给不经过配置的抽取单测。配置校验失败时，错误写明是哪一条不成立。摘要对不上才用 `FROZEN_INPUT_DIGEST_MISMATCH`。

卡住时只看 `Limits` 里的 `max_no_progress_turns`、`max_focus_turns`、`max_focus_replans`。观察进入 `Blocked` 后运行结束。错误码仍是 `AGENT_TURN_BUDGET_EXCEEDED`，正文是 `outline stalled in phase {phase}`，`phase` 取检查点上的 `discover`、`outline`、`check` 或 `complete`。这是逻辑停滞，不是回合预算。

编制页按 `outline_phase` 显示发现、组织（章节和模板槽）、核对、完成。`outline` 且章节、附件绑定或槽还没齐时是组织；三者都齐，以及 `check`，是核对；`complete` 是完成。页面文案只写这四个阶段。应答章节缺槽时职责仍是组织，即使 `slots_submitted` 已经为真。

## 已接通

一次成稿在产品路径上是发现 → 组织（章节、绑定、槽）→ 核对 → 完成。阅读包、要求进组织、`tool_draft` 投影发布，以及模板并入组织，都已经接通。106 页真实招标 PDF 在 [PR #13](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/13) 的 `da804a0`（main `c45001d`）上发布过。记录和还开着的项在 [大纲计划](../../plans/bidding/outline-gaps.md)：冻结标题残留（未重跑 106 页 PDF）、`response` 知识库填充、ONLYOFFICE 产品回路，以及 CI 里的真实 PDF 门。两条及以上附件链时，组织必须写出三层章节树。
