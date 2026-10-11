# 证据与审查运行时契约

本文记录当前实现的接口边界；反例矩阵与验证状态见[执行计划](check-execution-plan.md)。本轮实现已落盘，本地回归收尾状态见执行计划，不能据此宣称真实模型语义验收通过。离线协议测试、领域夹具和真实服务结果分别记录，不互相替代。

## 当前实现

- FrozenInput/OutlineArtifact schema 3、parser source-v2、checkpoint 31；Discover 并发 2、每请求 1 包。新检查点包含自动 user 页和保留审计的请求投影，不迁移旧状态。
- 必需支持由原生合并格、适用表头、既有续表链及精确有向关系选择；同节/祖先正文仍是可选候选，不全部成为提交屏障。主证据严格属于 owned 范围。
- 宿主自动附入尚未确认的必需支持页。重复失败比较使用归一化动作及错误类，不以固定六次拒绝判定停滞；不同自然语言改写不保证能被识别为同一业务失败。

工具参数唯一规范为 [outline-tools-v2.schema.json](../../crates/bidding/schemas/outline-tools-v2.schema.json)，duty 白名单与路由以 `outline::agent::registry()` 为准；不维护第二份手抄工具清单。输入细节见[冻结输入](../bidding/frozen-input.md)。

## 模型输入与宿主证据

模型输入只使用宿主提供的身份：

- Discover：`evidence_key` 选择当前包主证据，`atom_key` 选择所属原子或章节；`support_key` 单独关联已核验的外部适用条件。
- Organize：`source_key` 用于读取、固定原文复制和生成说明的依据。
- Check：`review_evidence_key` 只能引用当前 Check 已完整交付的范围；比较工具的 quote handle 只在其 review unit/version 内有效。
- 四类读取工具的续页只接受宿主 opaque cursor，不接受数字页码、手写字节范围或模型指定的页面字节预算。

`source_wire` 是模型参数校验与 canonical 解析边界。内部 `EvidenceRef` 仍保存冻结输入摘要、源单元/原生表格坐标和 UTF-8 区间，用于解析原文、复制、覆盖与回执校验。`apply_canonical` 处理已解析的宿主参数，不再次套用模型 key schema，也不构成另一个模型入口。

来源 key 绑定输入、工作修订和读取 epoch；包 key 另绑定包修订和 claim。不同用途不可互换。组织写入或重开使旧 Check 授权失效。模型参数不保留 rawref、atom/index 的兼容路径。

## 容量、续页与真实交付

`prepare_request` / `fit_batch` 对完整下一请求计量，包括系统提示、工具、历史、来源、图像与输出预留。自定义 tokenizer 需有可核实来源或实测校准；经验估算不代表长上下文能力证明。模型上下文与操作员调用/费用/时间限制独立。

`read_evidence`、`read_claim_evidence`、`read_requirements`、`read_outline` 由正式异步 runtime 决定容纳量和续页。完整段、原生行优先；单个载体过大时，宿主生成连续 UTF-8 子范围。超大元数据以稳定 record identity、命名字段和字段范围传递，宿主核对完整字段后恢复记录；模型不拼接 JSON 字节。缺页不能授予完整回执。

工具读取结果与宿主自动 user 页先成为 pending frame。最终装配器对实际 compact/project 后的消息、图片身份及完整请求封存摘要；只有匹配 journal 已持久化完成响应的请求才确认交付。预取、同响应中读后写、被回收的未发送结果、失败批次、旧修订或旧 Check epoch 不授予授权。

Discover 的业务拒绝与宿主交付错误分开处理。当前 Failed claim 可以继续补读；只有 durable Received、当前请求身份与有效 tool turn 均匹配，才确认实际携带页面。stop、无工具、length 或传输错误不授信；完整响应中的坏 JSON、旧 scope、未知或多个工具属于新动作错误，先确认先前交付，再独立拒绝动作。宿主确认错误终止对应 worker，不反复采购响应。

排队、目录和新 operation ID 不算进展。只有精确确认的新原文范围或有效业务产物推进状态；新 claim 重新交付旧原文恢复当前授权，但不能清掉跨 claim 的业务失败记忆。必需页面尚未完整时，提前提交只反馈缺页并由宿主续交；全部交付后重复同一业务失败仍终止。

Discover 目录用 `original_read_complete` 标明当前 claim 已确认的完整原文覆盖。worker 的 `next_required_read` 优先指向正文续页，否则指向下一未覆盖条件；该导航可预期当前请求附带页面被消费后的下一动作，但不提前授予交付信用或语义适用性。已读首项仍可追溯，不应在尚有未读条件时成为默认重复读取目标。

Discover worker 保留原始消息、观察和失败回执作为审计，请求侧单独退休已交付正文和旧 tool 组，保留独特助手观察、合并连续逐字相同观察及完全重复失败反馈。claim 轮换清除旧授权、游标和句柄，不清审计；旧模型参数不能通过幂等入口复活。

初始规划计入首发支持页及图片，和 worker 共用支持页构造与 UTF-8 缩页规则。规划使用完整规划请求模板，不声称它与 worker 的全部协议消息逐字相同；最终 worker 请求重新计量全部实际内容。最小来源页或不可分图片仍超窗时，明确终止对应 worker，保留未完成范围；独立包及既有 partial-source Check 可继续，不启局部 Organize 或全局 ready。未知发送、输入完整性错误和人为暂停仍全局阻断。分页探测使用与实际候选相同的续页游标，不抬高窗口掩盖容量问题。

阶段交接共用 `handoff_history`：保留当前尚未交付、身份仍有效的工具组，移除较早历史。保留历史本身不授予读取回执。repair 轮换 claim 后，历史回收可按原提交身份核验已有持久回执和摘要；新模型提交仍按 live claim 拒绝旧 key。

## 显式主张与修复

模型显式声明可独立判断的义务、适用条件、原文覆盖片段、主证据和支持证据，并分别作出判断。宿主检查身份、完整覆盖和一对一判定，不按标点拆分冒充语义原子性，也不以关键词判断业务含义。

Check 重新读取完整原段、原生行、跨行合并格与适用表头；仅原要求主证据可触发一跳必要关系，语义候选不会继续扩张跨文件关系。确定支持至少需要一项与原要求范围相交且明确 relevant 的 primary；辅助引用不能冒充 primary，显式 relevant 条件不能从全部判定中消失。合法“无需提交”可以保留，unclear 保持未解决。跨包条件不能增加当前包覆盖或改变所属章节。缺失公告不得补造；固定原文由 canonical 引用复制。

`contradicts + manual_review` 是合法未解决状态。正式 `execute_turn` 完成工具批次后，可按修复轮次、输入签名和幂等记录执行有界重开：Check → Discover 完整重提 → Organize → fresh Check。重开使旧组织草稿和 Check 授权失效；失败或耗尽保持未解决。

## 验证与清理边界

领域测试使用显式投递 seam，不把递增 turn 当作交付。正式闭环测试必须实际经过 request、SDK session、journal response/tools/commit 和 `execute_turn`；脚本判断只证明协议。

新 schema、提示和配置有新的摘要；不伪改旧检查点摘要来续跑。需要新真实 run 时保留旧失败现场与 usage 账本。通过结构检查不代表召回、主张正确性或响应充分性通过。

本页记录当前接口合同；执行计划分别记录已实现、已测与未测范围，历史日志只证明对应版本和执行范围。最新修复须附实际测试记录；真实模型验收单独记录调用、usage 与语义结论，不从离线测试推断已通过。

历史离线测试只覆盖其记录版本和用例，不追溯证明新实现。合并表格、比较一致性、修复交叉生命周期、自动支持及停滞反例的实际测试结果见执行计划；合成可达不证明真实服务语义正确。真实原始材料、服务结果和费用账本私有保存，不进入仓库。CI 以具体提交为准。

## 集合写入模式

`put_chapters`、`bind_forms`、`put_slots` 使用必填 `mode`：`replace` 全量替换，`upsert` 按 ID 局部更新并保留其余条目。不提供默认模式或旧 append 别名。两种模式均保留各自的目标、续表链、重复 ID、证据和事务校验。具体工具集合以 registry 与当前 duty 的正式路由为准。

## 运行参数默认值与边界

应用层 crates/bidding/config/request-defaults.json 提供请求输出预算 16384 token、单请求传输超时 180000 ms；环境变量仅作显式覆盖，非法正数仍拒绝。此默认值是保守请求预算，不冒称未知供应商的原生最大输出能力。完整请求继续计入输出预留并受上下文预算约束。隔离启动器无需复制这两个非秘密参数才能启动。环境覆盖名为 `KB_AUTHORING_OUTPUT_TOKEN_RESERVE`，它只参与本地上下文核算，不增加 provider 的 max_tokens/max_completion_tokens。Compose、示例与隔离脚本使用当前名称，空值统一回落共享默认。此前旧名 `KB_AUTHORING_MAX_OUTPUT_TOKENS` 的 8192 配置实际被运行时忽略，并非有效覆盖；该旧说明已纠正，不提供旧名兼容别名。

旧 source-review、finding repair、Reviewer/Fill 协议及其操作限额已经删除；实际生产只使用 Discover/Organize/Check，内容生成经 published outline → match_queries → retrieval → response::respond。旧检查点不兼容。分包根据包含提示、工具、历史、图片和输出预留的完整请求 token 判断。

Rust/API/worker 与 Python 验收入口共享 `crates/bidding/config/request-defaults.json` 的非秘密请求默认值。Python 比较 effective 值与启动快照，不再要求原始环境必须包含默认参数。私有验收 adapter 独立实施累计费控，生产 journal 仅累计计数；同一请求最多三次的边界重试保护仍保留。旧 Main source package 包装已删除。

生产配置已删除累计操作预算及其暂停/授予入口，不以 Option 或超大数保留。累计调用和 token 仅作诊断计数，算术溢出仍拒绝。私有真实验收 adapter 在发送前实施操作员明确授权的累计费控，不属于生产 Run 配置；具体额度和历史消耗只保存在对应运行账本，不作为通用默认值。每请求上下文、取消、超时、传输恢复和重复无进展保护保留。删除后的运行时、响应生成和配置回归必须全部通过。

## 环境默认配置

生产从所选 provider 解析模型身份。已核实的 tokenizer 映射不需要额外 Limits JSON；未知模型必须提供 `KB_AUTHORING_TOKENIZER_PROFILE` 的实测 encoding/calibration，不接受重复的 model_id。有效配置自动绑定 provider.model_id。`KB_TENDER_AGENT_LIMITS` 可省略，也可仅声明 `vision_enabled` 等单项覆盖；已删除的操作配额和工具字节上限字段会被拒绝。

默认 `max_context_tokens=131072` 是应用层单请求预算，包含提示、历史、工具、图片估算和输出预留，不代表自动探测到供应商上下文能力。图片能力必须由同一模型的实测或资料确认。宿主预览明确给出省略数和正式分页入口，不构成读取回执；所有 canonical 要求保持不变，正式工具使用完整请求 token 适配和 opaque cursor 续读。

当前profile的131072为硬上界，局部覆盖只能降低预算；更大窗口必须引入经过验证的新profile契约。分页续页仅传`{cursor}`，首次筛选保存在宿主cursor中，不再重复mode或筛选字段。

## 未发布阶段的变更原则

KnowledgeBrain 尚未发布，架构和协议改造直接采用新设计，不提供旧数据、旧检查点或旧协议迁移，也不保留兼容层。旧验收原始文件和记录仅作为审计证据保留，不作为新合同的运行状态或验收进度。不同合同从新状态执行；同一合同内的持久化恢复、取消、已收到响应的重放去重和未发送暂停幂等继续保留。

模型侧 claim 与 operation 使用宿主签发的短 opaque handle，持久绑定 run、pack、revision、generation 和 canonical 标识。未知、跨域、过期和碰撞严格拒绝，不猜补损坏标识。负提交缺失说明或真实完整检查记录时不得通过；业务拒绝以顶层失败反馈给模型与运行日志。领取工作不算完成进展。

## 已有组织、目标与发布边界

现有章节深度门：至少两条互不续表的附件链时，Response 叶节点须位于根 Group→中间 Group→Response 的第三层；单条链允许较浅树。根可为森林，同级 order 唯一；每个 Response 至少一个槽，每个附件表恰好绑定一个 Response，同一续表链同叶且保持来源顺序。不能以每表独立章节绕过原生章节组织。

组织阶段读取分页要求、分页目录和精确证据；工具返回不包含无限增长的全量草稿。章节树和要求目标分离：Group 仅组织目录，Response 叶节点负责要求，Fulfillment 指向真实槽、表格、可见人工任务等响应目标。

固定原文使用 `TemplateBody::SourceCopy`，主机按冻结引用复制，`purpose` 明确区分 instruction 与 mandated_response，不把招标说明冒充投标人承诺。投标人填写处使用 `EditableBlank`；说明性生成文字必须和原文分开并保留来源；表格使用结构化复制。自由 `fixed_text` 不能充当权威源文字。

`put_chapters`、绑定、槽和履行关系的修改都先验证依赖再整体提交。角色变更或删除不得静默丢掉既有目标。每条续表链单独检查同一合法 Response、顺序和全覆盖，包括项目仅有一条链的情形。


Check 在全部在途处理后可对独立已提交范围执行 partial-source 复核；不是局部 Organize 或完整模板批准。正常完成需要全部必要包、来源、要求、具体目标及 fresh Check 通过。已知合并格/表头及显式比较一致性缺口见执行计划，不能将结构通过当语义正确。

`project_draft` 和 `validate_publication` 都按最终输入、章节、槽、表格绑定与履行关系重新校验。最终检查点先保存，发布作为最后一次事务操作；崩溃重放同一 artifact 身份幂等。

数据库事务验证项目、输入、lease token、单调 epoch 和当前到期时间。旧 worker 发布零写入。对象必须 available 且获得持久域 owner 后才能推进 source/DOCX/export head；staging 过期或 GC 不能移除仍有持久 owner 的对象。


## 请求级表格与工具更新

FrozenInput 与内部 PackCarrier 保存完整网格及空白锚点。模型请求把每张表的结构归并到唯一 `table_structures` 字典，内容以 `table_id` 和原坐标引用。真正空字符串的普通 1×1 正文格可用半开坐标区间表示；表头、合并格、空白字符与控件等不按空字符串压缩。区间数量仅描述原文可填写位置，不表示要求数量上限或下限。预算规划与实际传输使用同一请求投影。Check 的逐锚点 receipt 始终来自 canonical session，不从紧凑模型视图推导。


`put_slots` 的每一项都必须包含 `slot_id`、`chapter_id` 和 `content`。其余字段由 `content.type` 决定，不能用空字符串代替应省略的字段。

| content.type | 必填内容 | 禁止的槽外层字段 |
| --- | --- | --- |
| `source_copy` | `content.refs` 恰好一个已读取、已交付的原文范围及 `purpose`；主机复制原文 | `text`、`match_query`、`blank_kind` |
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

## 正常结果与文档问题

Discover `submit_pack.issues` 必填，可与正常 requirements 同次提交。问题只接受 `document_missing/document_conflict/document_ambiguous/document_unreadable`，引用当前包 owned 的有效证据，`requested_information` 非空，Discover 的 `requirement_ids` 为空。空 requirements 仍须完整 inspected：真正无义务时 issues 为空并给具体 no_requirement_reason；存在有效文档问题时 reason 必须为 null，不能把不确定当无义务。后续 Check 可在 fresh 读取后关联要求；问题不授予阅读或模板完成信用。

问题保存在 canonical 包记录并投影至现有 Draft，不新增第二 store。报告使用 `Checkpoint::review_report` 输出 results 与 issues，按原文实际 overlap 给 affected_pack_ids、待补信息及未解决状态，并附输入摘要、发现版本、review epoch 与 template_ready。只有全部独立范围处理完成、仅明确文档问题剩余时，现有 `DOCUMENT_ISSUES_REPORTED` 出口保存报告并受控停止，不 publish、不置 done。失败 runner 仍须读取持久报告；传输/宿主错误不能被改名为文档问题。
