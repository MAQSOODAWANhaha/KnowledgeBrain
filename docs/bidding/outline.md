# 大纲运行时 V2

入口为 `crates/bidding/src/outline`。本合同替换旧六工具协议，不提供旧检查点或旧冻结输入兼容。完整动机、边界与 T01–T23 验收见[解析与 LLM 改造方案](parsing-llm-remediation-plan.md)。工具参数的唯一规范为 [outline-tools-v2.schema.json](../../crates/bidding/schemas/outline-tools-v2.schema.json)，职责与路由由 `outline::agent::registry()` 同源生成。

## 输入边界

`outline::frozen::build_frozen_input` 将解析结果、区域 OCR 状态回执、持久化图像引用与解析合同冻结为 `FrozenInput` schema 3。运行入口和持久化入口验证身份、版本、单元覆盖、顺序、页清单、网格完整性及必需 OCR。不能只把旧 JSON 的版本改为 3。

章节坐标和物理定位并存。文本区间使用 UTF-8 字节半开区间；网格引用必须使用真实锚点，不能引用合并覆盖的格子。所有证据绑定同一输入摘要。冻结后不得被知识库摘要或模型自由文字覆盖。

## 原图与视觉模型

`read_source_view` 从冻结对象加载并验证原图摘要，生成完整画面的有界 JPEG，通过现有配置模型的 `image_url` 发送，不换供应商。`limits.vision_enabled` 必须由操作者按实际模型能力明确配置；文本模型不能冒充看过图片。缓存、对象引用与 OCR 文字均不算原图交付，只有实际请求正文包含对应像素且收到完整响应后才记回执。Discover 和 Check 分别记账，Check 不能借用此前看图记录。非空白但 OCR 为空的图像保留视觉检查义务；截断或未覆盖的 OCR 不得宣称完整。

## Discover

保留“小章节合包、大章节拆包”。有序 `PackAtom` 保留正文、表格、图像混排顺序及章节归属。超大段落按安全字节边界切片，超大表格逐行、逐格、逐格内文本拆分；重复表头是上下文，不增加拥有的证据计数。

单次包提交含 claim、revision、call identity、已检查载体、要求与多条 `EvidenceRef`。空要求必须说明负结果且检查全部载体。引用不仅必须存在，还必须完全处于该包交付区间的无间隙联集内。

提交先检查身份和状态，再完整验证临时结果，最后整体替换。未知包、非法状态、失效 claim 或畸形信封不得改变状态。合法 Running 包业务校验失败留下 Failed 诊断；修复整包替换，不保留旧尾部要求。已 Committed 包仅允许同操作身份、同参数摘要的幂等回放。

分包失败显式停止，不得让空错误计划成为“全部完成”。发送前整次请求仍受完整上下文 token 预算约束。

唯一上下文上限为 `max_context_tokens=131072`，单位是 token。旧的 `pack_max_chars`、`max_context_bytes`、`max_history_bytes` 配置已移除，不保留别名；也不再按每格固定 512 字节切包。规划器把候选包放进真实请求信封，用显式绑定模型的 tokenizer 计算完整提示、工具、历史和源载体，并预留图片、输出与安全额度；小章节尽量合包，大章节只有实际 token 预算放不下才拆分。

`tokenizer` 必须声明 `model_id` 与 `encoding`。已核实模型/编码组合使用内置 o200k/cl100k 词表；未知模型必须提供基于实际 usage 的校准倍率及依据，不能根据模型名猜测。该估算包含完整 JSON 与额外协议预留，不伪称服务端实测 usage。图片 Base64 不作为正文 token 计数，按明确的视觉额度预留。内部 SDK 的字节分配上限由 token 词表最大编码长度和图像额度推导，不是另一个切包策略。

Discover 每次最多领取 4 包放进同一个模型请求，最终仍检查整个请求的 token 总量；这是执行调度上限，不用于决定包的大小。包数不等于调用数，修复、组织、独立检查另需轮次。

规划还在同一上下文额度内预留由模型输出额度推导的修复反馈空间，避免满窗源证据加上失败诊断后无法继续。完整错误保留在检查点；模型侧反馈投影明确报告未显示数量。只有与持久化提交回执完全匹配的历史才能去重，不能把未提交或错误身份的请求当作完成。

## Organize

组织阶段读取分页要求、分页目录和精确证据；工具返回不包含无限增长的全量草稿。章节树和要求目标分离：Group 仅组织目录，Response 叶节点负责要求，Fulfillment 指向真实槽、表格、可见人工任务等响应目标。

固定原文使用 `TemplateBody::SourceCopy`，主机按冻结引用复制。投标人填写处使用 `EditableBlank`；说明性生成文字必须和原文分开并保留来源；表格使用结构化复制。自由 `fixed_text` 不能充当权威源文字。

`put_chapters`、绑定、槽和履行关系的修改都先验证依赖再整体提交。角色变更或删除不得静默丢掉既有目标。每条续表链单独检查同一合法 Response、顺序和全覆盖，包括项目仅有一条链的情形。

## Check

结构 ready 不等于语义完整。Check 必须分页枚举所有要求和源包、调用 `read_evidence`，通过 `submit_review` 报告遗漏、条件/否定/数字误读、响应不足等问题。负结果包也必须复核。问题必须绑定已读取的证据，不能凭无来源结论改写要求。

`finish_outline` 要求结构、实际履行目标、包复核和要求复核全部完成。未决问题或人工任务保留 `needs_review`。真实模型召回率需要业务标注集评测；通过 mock 合同测试不代表没有语义遗漏。

## 工具

- Discover：`submit_pack`、`read_outline`、`read_source_view`
- Organize：`put_chapters`（必填 `mode=replace|upsert`）、`bind_forms`（必填 `mode=replace|upsert`）、`put_slots`（必填 `mode=replace|upsert`）、`put_fulfillments`、`read_outline`、`read_requirements`、`read_evidence`、`read_source_view`
- Check：`read_outline`、`read_requirements`、`read_evidence`、`read_source_view`、`submit_review`、`finish_outline`
- Template：`put_slots`（必填 `mode=replace|upsert`）、`read_outline`、`read_requirements`、`read_evidence`

准确 duty 白名单以注册表为准；广告、参数验证和 dispatcher 必须一致。追加工具与全量替换工具采用相同验证门。

## 有界执行与恢复

检查点合同版本 16。每个阶段都受 `max_turns`、`max_tool_calls`、`max_read_bytes` 限制，不再豁免一次成稿路径。`run_budget` 还限制物理调用次数、累计预留输入/输出 token 与墙钟时限；传输重试也消耗额度。未报告实际 usage 不能被理解为零成本。

完整请求及物理调用预留先在当前 lease 下持久化，再发送模型请求。完整响应先持久化，再执行工具；保存响应的恢复不能再次调用模型。每个 pending 请求最多三次传输尝试，跨恢复保留计数。

预算耗尽保存 `paused_budget` 与原因，不假装 Published，也不自动重复排队。保持已有检查点供检查；操作员可使用 `journal_db::resume_budget`，传入预期暂停检查点摘要、原 Config 和只增加额度的新 Config；事务锁定后核对旧配置摘要并保留全部已消费计数，再恢复待领取状态。输入、模型、提示、工具与上下文合同不得随预算授权变更。旧版本要求重解析并创建新运行。

## 发布

`project_draft` 和 `validate_publication` 都按最终输入、章节、槽、表格绑定与履行关系重新校验。最终检查点先保存，发布作为最后一次事务操作；崩溃重放同一 artifact 身份幂等。

数据库事务验证项目、输入、lease token、单调 epoch 和当前到期时间。旧 worker 发布零写入。对象必须 available 且获得持久域 owner 后才能推进 source/DOCX/export head；staging 过期或 GC 不能移除仍有持久 owner 的对象。

## 范围

本次不引入补充公告覆盖语义，不处理前后端 HTTP 交互。图片/解析、冻结、队列、数据库和对象生命周期的后端内部接线属于本合同。

### 请求级表格投影

FrozenInput 与内部 PackCarrier 保存完整网格及空白锚点。模型请求把每张表的结构归并到唯一 `table_structures` 字典，内容以 `table_id` 和原坐标引用。真正空字符串的普通 1×1 正文格可用半开坐标区间表示；表头、合并格、空白字符与控件等不按空字符串压缩。区间数量仅描述原文可填写位置，不表示要求数量上限或下限。预算规划与实际传输使用同一请求投影。Check 的逐锚点 receipt 始终来自 canonical session，不从紧凑模型视图推导。

## 工具参数与更新合同

`put_slots` 的每一项都必须包含 `slot_id`、`chapter_id` 和 `content`。其余字段由 `content.type` 决定，不能用空字符串代替应省略的字段。

| content.type | 必填内容 | 禁止的槽外层字段 |
| --- | --- | --- |
| `source_copy` | `content.refs` 恰好一个已读取、已交付的原文范围；主机复制原文 | `text`、`match_query`、`blank_kind` |
| `editable_blank` | `blank_kind=bidder_blank|signature`、非空 `match_query`；正文保持空白 | `text` |
| `generated_explanation` | 非空 `text`、非空且已读取的 `content.supporting_refs` | `match_query`、`blank_kind` |

来源读取和写入不能在同一模型响应中取得交付信用。未读取或未交付的引用、跨来源范围、任意补写的固定原文均被拒绝。分组章节不能承载模板槽。

| 工具 | `upsert` 语义 | 完整替换与顺序 |
| --- | --- | --- |
| `put_chapters` | 按已有章节 ID 更新；未提交章节保留，新 ID 新增 | 兄弟章节的 `order` 必须唯一，根章节同样如此。重命名复用原 ID；整体重写使用 `replace` 并保持现有目标引用有效。 |
| `bind_forms` | 已有表单原位更新归属；未提交项保留，新表单追加 | 不删除或重排。续表必须同叶且符合来源顺序；顺序修复先读取完整绑定，再以 `replace` 提交全部有效绑定。 |
| `put_slots` | 已有槽原位更新；未提交项保留，新槽追加 | 不删除或重排。调整顺序时先读取完整槽列表，再以 `replace` 提交完整有效列表。 |

`put_fulfillments` 不接受 `mode`。每项的 `target_refs` 必须非空，并引用与 `primary_response_chapter_id` 相同应答叶子下的已有槽或表单绑定，或声明可见人工任务。仅引用生成说明不能履行要求；人工任务会保持需要复核状态。目标修改和拒绝均为原子操作，失败不能部分更新已有结果。

分页首次调用不传 `cursor`。仅当实际工具结果包含非空 `next_cursor` 时，续页传该值作为 `cursor`，并同时提供**当前请求**的 `wire_scope`；不再传 `mode`、筛选字段或 `version`。宿主先验证并移除作用域，再检查内部续页参数。导航说明不会签发占位 cursor；历史作用域、猜测 cursor 和篡改过滤条件均被拒绝。

结构上达到 Check-ready 仅表示可以开始独立复核。Check 必须重新读取整包来源、未提取义务、结构回执与实际响应正文；Organize 的读取或离线合同测试不能代替语义验收。


Check 的宿主工作包保留尚未比较的要求 ID 与未完成计数。有效来源续页游标按输入、发现版本、Check 职责与读取 epoch 校验，并在历史裁剪后继续可见；成功续读会移除已消费游标。工作指引和游标都不授予阅读回执或语义通过，来源包中的未提取义务仍需独立复核。

完整请求超限时，适配器一次省略可重读的要求预览，并合并执行既有的安全历史整理，再重新测量。最新未消费工具结果不被截断；来源分页继续按有界页长缩小，剩余原文由宿主游标保留。发送前仍校验完整模型请求加输出预留不超过配置的上下文窗口。

分页预算探测另有只拒绝、不批准的快速路径：当最新未消费读取组（连同工具协议）的测量值本身已超出窗口时，直接要求缩小候选页，不再尝试无效移除旧历史。候选页最终仍由完整请求 token 测量批准；失败探测不得改写 Unicode 原文、历史或读取回执。来源行由当前页和宿主续页完整覆盖，消费成功后才移除旧游标。


读取进展和业务完成分开统计。Organize 与 Check 仅使用已由完整请求确认交付的来源、目录和目标正文回执；新覆盖范围可重置无进展计数，但不标记要求或来源包复核完成。相同页、重排或重叠区间不产生新读取进展；待交付、过期发现版本/Check 轮次、变更工具返回内容和游标变化均不产生交付信用。范围按来源载体、原生单元格与包标识保留身份，检查点重载不重复计功。Discover 继续以实际包提交推进，不把扫描声明或共享目录读取当作包完成。

Check 持续任务保持当前未复核要求：完成主张比较后，继续显示尚未读完的具体目标槽位、所属章节、字节长度和读取参数，直到正文完整读取并提交复核，才推进下一条。目录完整不代替原文、表格、人工任务或响应充分性判断；发现目标只有说明而缺少实际响应时应提交问题。任务提示本身不签发回执，也不自动标记通过。
