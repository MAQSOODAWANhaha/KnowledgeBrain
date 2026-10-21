# 领域模块合同：目标分析流程与现行实现

字段名、类型、可选性及枚举以[唯一字段字典](field-dictionary.md)为准，本文说明接口、职责和
执行边界，不另定义同义结构。大型 JSON 仅在内容寻址文件中持久化，checkpoint 只存文件引用。
以下区分工作区v2主线、共享接口和历史独立工具。五节点及新协议已实现，本轮尚未更新
部署或发起付费调用，旧历史产物保持原义；测试和运行事实以[交接](handoff.md)为准。

## 最新一期主线：完整分析招标文件（工作区已实现）

目标主流程为 **register → freeze → extract → verify → confirm**，人工确认当前无阻断的
分析结果后到 `analysis-ready`。目标产物为来源清单、统一事项台账、来源覆盖与独立核验结果、
问题及有证据的决议。实际内容由招标原文决定，包括评分、否决、技术、服务、售后、人员及
其他要求；不按业务类别建立不同发现流程。详细顺序见[业务工作流](business-workflow.md)。

本期不处理企业响应数据，不生成响应计划、响应动作或准备待办，不决定评分机会是否采纳，
不计算企业实际得分。原文明示的填表、附件、签章和证明材料要求仍须分析，但不能据此生成
企业准备任务、声称材料已提供或判定企业已经满足要求。独立计划审阅不属于本期实施任务。

### 统一事项的最小合同

每项仅必填 `item_id`、`kind`、`text`、`evidence`。`kind` 使用四种类型：

| kind | 源文分析内容 |
| --- | --- |
| `obligation` | 资格、技术、服务、人员等义务或约束 |
| `scoring_rule` | 评分指标、分档、公式、加扣分及分值规则 |
| `consequence_rule` | 否决、无效、不得参与、履约责任等独立于计分的触发规则及后果 |
| `submission_requirement` | 原文明示的填报字段、正文内容、附件、证明、签章或提交要求 |

对象、范围、条件、例外、数值、业务分类及各类型的具体信息均按来源可选。没有信息则省略，
不强制填写空数组、空对象、`null` 或“无”。省略不证明原文不存在相关信息；无法判断的内容
进入外部问题或待解析引用，不得把缺省适用性解释为“不适用”。评分规则记录源文规则及其
所需证明，不推算投标人得分。各类型可以通过有证据的关系关联，不把同一证书或参数的
资格、评分及否决含义混成一个布尔值。

`work_unit_id` 由宿主提供，模型在该命名空间内生成 `item_id`，宿主校验格式与唯一性。
持久事项引用为 `ItemRef{artifact_ref,item_id}`，不凭同一 ID 假定跨修订内容相同。目标统一
台账根引用使用 `items_ref`；旧工具的`requirements_ref`保持原义，不增加兼容别名。
目标业务输出使用 `schema_version=2`，不因此伪造来源或领域内容变更。目标 checkpoint 去掉
`response_plan_ref`，分析结果由来源、事项、核验和确认回执投影，不另增分析状态机。

### 发现、补读与核验的职责

先统一全源发现，再对具体复杂问题按需处理；不通过技术、人员、评分等分类预路由原文。
当前包未提供某章正文不代表全文缺失。可用性目录只用于定位，补读后的完整原文才是判断
依据。verify 使用字段字典规定的 locate/associate/review 三种固定子操作，跨包引用最多一轮
目标定位和补读核对；目标缺失、歧义或仍未决时进入人工裁决，不递归追索或自动反复生成。
locate 还须按完整台账发现可能冲突、修改、条件依赖或改变含义且台账尚未体现的跨源候选；
宿主装载相关完整原文后关联。只提出关系、没有决议的候选不进 joint，关系标为未独立核验；
有决议的候选必须联合审阅，分开核验的 ID 覆盖并集不能代替。

associate 只提交有证据的 Decision 提案；独立 review 须逐条审阅并明确接受，才可解决提案
绑定的具体问题。遗漏、拒绝、过期或审阅失败均保留原阻断，空 findings 不代表解决。
提案、接受结果及原问题均在 verification_ref 下保留不可变回执，并进入确认摘要。
补遗决议按字典的 AmendmentEffect 明确受影响原项、修改范围、全部保留规则和替代事项；
纯删除不强造替代项。当前结果由原台账及有效决议投影，不增加可编辑的当前规则台账，
不由宿主做自然语言范围运算；漏抽或原项错误通过显式领域修订后重验。

自然语言义务、数字归属、比较关系、条件/例外、作用域、冲突及引用含义由带完整来源
上下文的 LLM 独立核验及必要的人工裁决确定。宿主校验结构、来源定位、覆盖、身份版本、
预算和执行状态；正则和词表不能作为最终语义裁判。模型空 findings 不能清除既有宿主问题。
当前语言启发式及其局限见[职责边界与规则审查](semantic-review.md)。

### 状态、确认与回执

目标沿用 `pending/running/blocked/waiting-confirmation/failed/cancelled`，成功终态改为
`analysis-ready`。`failed` 表示执行或协议失败，`blocked` 表示必要来源缺口、作用域未审议、
覆盖或预算不完整等硬阻断；`waiting-confirmation` 表示独立核验完成且无硬阻断，等待人工处理语义疑点并正式确认。
新主线成功终态为`analysis-ready`；旧`response-plan-ready`记录及历史类型保持原义。

确认绑定当前 `source_version`、`domain_revision` 和包含来源、台账、关系/决议及核验结果的
`checklist_digest`。确认不能处理硬阻断；剩余语义疑点须在确认时逐条给出驳回或保留的人工
结论（见[问题分级与人工处理](field-dictionary.md#问题分级与人工处理)），不修改事项或删除问题。
内容修订使旧确认失效，必须重验。目标确认后即结束分析，不再进入计划生成。

主阶段、问题处理子操作和模型请求分别保留回执，成功请求不等于语义通过，命令完成也不
等于分析就绪。子操作进度、事项适用性及运行状态各自表达，不增加业务分类状态机。成功
产物、旧版本和付费审计保留，`uncertain`按[恢复合同](field-dictionary.md#失败执行的显式恢复)
共用单元首次加三次，可能重复计费；在途发送和租约围栏保留，不能换分包或协议版本重置额度。

## 历史六阶段接口与恢复事实

旧主线为register → freeze → extract → verify → confirm → plan，使用Requirement/Plan合同
并到response-plan-ready；现有运行容器仍是该版本。以下保留共享接口、历史独立工具及
设计依据；新工作区主线不调用旧Plan，涉及计划的部分不纳入新一期目标。

冻结恢复细化：图节点按文档登记 `freeze-document` 回执、按实际来源图登记 `ocr` 回执，
之后合并小索引为全局 freeze 回执。部分文档/图片成功后其他范围基础设施失败，重投复用成功
回执，不重复全量解析/OCR。抽取与验证同样按 pack 独立回执。技术失败允许独立范围先完成，
全局只在所有必需范围具有成功产物或明确领域未决处置后推进。
验证和响应计划按当前修订同文档全部抽取结果的完整输入独立规划，加入各阶段所需要求、
处置或来源上下文后重新准入，不沿用抽取原包作为固定请求边界。响应计划保存 `plan-pack`
回执，再合并为逐要求计划；全局检查覆盖全部适用要求，局部计划失败阻断终点。

## 证据（现行接口）

`register_manifest(manifest, artifacts)` 返回登记清单（documents/issues）；本地原件在提交时
复制并持久化，运行重放不重新读取用户原路径。`freeze_sources(registration, artifacts, ocr)`
异步返回冻结索引 ref；阻塞解析放线程/进程。每文档 raw 结构、每 pack 独立文件。

冻结索引含 `documents`、`packs`（refs）、`issues`、`range_count`。pack 为
`{pack_id, body: [range], supporting: [range], issues: []}`。range 为
`{range_id, document_id, unit_id, text, locator, kind, start, end}`（字符偏移相对原生 unit）。
导语/适用条件/表头作为 supporting 保留相同来源身份，不计入正文范围覆盖。raw 结构保留
parser 全部单位/网格/合并与坐标。native parser 不能覆盖时明确 partial/unsupported。

OCR callback `async(image_ref, source) -> {text,status,issues}`，由 Python 模型层调用实际图像，
来源图片原文/结构不可覆盖。文本 PDF 禁止强制 scanned 及无声全页 fallback；无法完成的
原生范围返回文档问题。附件有位置没有内容时返回 attachment_content_not_extracted。
`EvidenceRepository(artifacts).iter_packs(index_ref)` / `load_pack(pack_ref)` 复用冻结 refs。

## 模型层（现行接口）

`ModelConfig.from_env(mapping=None)` 是单一配置解析源。`OpenAIModel(config, store, lease,
prompts, concurrency=2, timeout_seconds=180).call(task,payload,images=()) -> dict`。
images 是 bytes+MIME 或 data URL，只用于实际来源图像，不拉取任意远程 URL。
类实现 async close/context manager。task 对应版本化领域提示（extract/verify/plan/ocr）。

每次请求唯一 request_id；prepared → sent（发送前持久）→ succeeded/failed/uncertain，
真实 usage 只取服务端返回。SDK 重试关闭。ModelInfrastructureError 表示可重试网络/429/5xx，
ModelSemanticError 表示 JSON/拒答/结构错误，ModelConfigurationError 表示配置错误。
断连/超时或无法确认服务端执行结果必须 uncertain；框架恢复不能承诺模型 exactly-once。
队列与 dispatcher 共用命令基础设施失败计数，最多 4 次后明确 failed；进程失联恢复与语义修复
不依靠该次数做业务判断。不写输出 token 上限，不将累计 usage 当单次 context 容量。

## 抽取、验证与计划（现行旧格式）

本节保留历史`Requirement/Plan`格式及六阶段行为，不作为新一期的事项字段合同或任务清单。

要求 JSON：`requirement_id,text,subject,evidence:[{range_id,quote}],conditions:[],exceptions:[],
numbers:[{value,unit,quote}],scoring:{},veto_risk,cross_references:[],applicable,applicability_basis`。
每包结果为 `requirements,dispositions,issues`；disposition 为
`{range_id,status:requirements|non_requirement|unresolved,reason,requirement_ids:[]}`。
不允许来源缺席；所有 body ranges 必须恰好处置一次。

验证使用独立调用 `verify`，逐包回看全部 body 与 supporting，并审阅提取结果，返回
`{reviewed_range_ids:[],findings:[{code,range_id,detail,blocking}]}`。模型漏审、证据原文不匹配、
数字/单位不被原文支持、跨文档冲突或无明确修改目标的补遗均成为问题。规则不能假设评分
总计 100。局部失败包保留成功包；依赖失败范围不放行。核对结果摘要包含全部来源版本、
要求和问题。未决阻断时图结束 blocked；人工确认不能覆盖阻断。

`plan` 返回 `items:[{requirement_id,target_kind,target,action,material_needs:[],constraint_digest}]`。
target_kind 为 body/table_field/attachment/manual_action；target 是具体位置字符串，action
必须说明该要求的实际响应动作，material_needs 非空（所需材料/用户输入，可明确“无需外部材料”
及依据）。表格目标必须标识具体字段，正文目标须有章节定位及写作动作；仅标题/空槽位拒绝。
所有适用要求恰好覆盖，引用要求不存在或缺项均阻断；终点 `response-plan-ready`。

当前规则：数字以 Decimal 和原文单位做局部反查；只有 scoring.total 与 scoring.components
同时声明时检查算术合计，不推定 100。合计关系是否由原文明示仍需模型核对。同主体/条件/
例外且去数字后同文、数字不同的跨文档要求会被标记为潜在冲突；它是待审线索，不是通用
语义证明。声明修改的补遗/澄清必须具有 replaces.document_id 及 range_id 或 scope；普通
澄清不声明修改则保留但不覆盖其他来源。

全空表格行只压缩发送视图，仍须模型结合表头和条件逐行给出有依据的处置并独立审阅，
不能仅依据 empty_table_row 自动生成 `non_requirement`。已证明空白页或有显式审议的
装饰图片由宿主单列，二者不能扩展为通用空表格处置规则。

## 已识别的设计细化（现行证据实现与历史合同）

- 现有 Excel 同时有 sheet/table 网格与 row units：正文覆盖选择唯一 row 范围；raw 保留全网格。
- 现有 Word SECTION 可能聚合多段：保留原生定位，不捏造更细坐标。
- PDF parser 失败时可能退回全页图：新适配必须显式检测并报告，不能把文本 PDF 全页发 OCR。
- 原生 PDF 对小图及重复图有过滤；新增 evidence 适配调用正常原生路由，补齐实际
  image object occurrence 及坐标，绝不打开 output_inventory 的逐页渲染模式。原生失败返回
  文档问题；重复页边文字、无法读取图像等覆盖缺口显式记为 partial，不悄悄通过。
- 原生 Excel 图片只有 drawing 序号；evidence 从原件 drawing anchor 补齐单元格/偏移。
  同时使用 raw grid 补齐 row unit 中省略的空单元格。全空行只压缩发送视图，范围身份仍在
  body（empty=true、non_requirement_reason=empty_table_row）及 compacted_blank_ranges；
  抽取模型对其作有依据的处置，不能预设为非要求。部分空行保留完整列位置，合并关系只读原网格。
- 分包保留同节及祖先节的导语/条件与表格表头来源；supporting 保留原 range_id 或绑定
  parent_range_id 的派生引用身份，正文身份
  在所有包之间互斥。导语范围必须有来源，字符偏移基于 native unit；不得虚构 Word 段落坐标。
- 表格原生 unit.text 可能为空；其发送文字明确标记 text_basis=grid_row/spreadsheet_row，
  start/end 相对该行发送文字，row_start/row_end 与 locator/raw_ref 定位原网格。
  OCR 文字另标 text_basis=literal_ocr，永不覆盖图片原件。空白 PDF 页有明确 blank_page 处置。
- supporting 累计携带已在前文出现的本节/祖先节适用条件与所属表头；不将后续条件提前应用，
  不把整章文字重复发送。条件以原文完整连续句/段保存，派生 range_id 绑定 parent_range_id、
  原 unit 的 start/end、section_id/heading_path 与 scope_binding。正文范围身份与覆盖不变。
  混合小章节包中的 scope_binding.applies_to_range_ids 明确列出绑定的当前正文范围，避免把
  第一章的条件错误施加到同一发送包中的第二章。
  结构明确的章节前导/明确本章范围可以按章节继承；局部条件、跨段续句、替代关系或缺少
  可靠章节关系等不确定情况保留来源片段并产生阻断问题，不能自动推断已解决。
  必要上下文超出发送预算时保留来源并返回阻断问题，不能静默裁剪。
- 无法找到完整条件句的范围不作为长篇 supporting 重发。pack.pending_scope_context 仅含
  range_id/parent_range_id/document_id/unit_id/start/end、原 locator/physical_locator/physical_path、
  section_id/heading_path 与 scope_binding（状态 unresolved、候选 applies_to_range_ids）、
  reason=incomplete_condition_sentence；不含 text/cells/图像 payload。完整原文仍在 body/raw。
  pending 的 locator 只保留定位字段，嵌套 cells 内容也必须移除。
  extract/verify 透传 pending 元数据作为待补读问题，不能将它当已读原文或新正文覆盖。
  condition_scope_unresolved 与宿主 scope_context_pending 均阻断；模型低阻断或空 findings
  不能解除它们。此修正不实现自动范围审议，也不要求改动原件或伪造来源新版本。
- Excel 的 display_incomplete_reason（如公式无缓存或格式不能准确还原）必须升级为可见问题；
  PDF 内嵌附件登记独立 carrier 与未提取问题。原件存储损坏是技术失败，不伪装成文档问题。

## 历史正确性评审合同（2026-10-14，保留依据）

以下保留此前针对六阶段实现的约束目标及依据；计划相关目标不纳入新一期。字面及词表
检查的局限及分析阶段已实现的独立审阅路径以上文及[规则审查](semantic-review.md)为准；
来源解析与旧独立工具的历史启发式仍保留，新流程真实模型语义验收未完成。

- 验证模型的 `blocking` 只能提高阻断级别。宿主对漏项、证据/数字/条件/适用性/冲突等
  正确性问题规定最低阻断等级，未知问题默认阻断；仅明列的纯信息类可非阻断。
  人工问题处理不是确认放行；问题未解决不得产生 response-plan-ready。
- 响应计划引用不可变要求约束快照及摘要（对象、义务原文、条件、例外、数值/单位、
  比较关系等），每项结构化绑定必须与经验证要求一致。裸数字相同不能证明义务保真。
  30工作日/30自然日/30天、至少/至多、适用对象/条件变更均需核对。摘要一致只能证明绑定，
  现有字面/词表检查不能证明自然语言保真；等价改写应由语义审阅判断，保留人工复核边界。
- 分包需绑定章节/祖先中的累计前置适用条件及来源，不只取最近一条。“仅第一标段适用”
  与“联合体还需”必须同时出现在相关后续包的 supporting 中。结构能确定的范围按章节
  绑定，不能可靠确定条件作用域/替代关系则返回阻断问题，不能默默丢上下文或塞全篇。

派生 supporting ID 仅是有来源的引用身份，不增加正文范围计数。若多个片段来自同一正文
范围，它们分别保留准确连续原文及偏移；不能用省略号拼接不连续原文冒充一个证据范围。
真实 PDF 离线预检的原件、解析正文、图片和冻结包只写私有目录；Git 只收摘要 hash、计数、
页码、问题类别、覆盖断言与 token 估算方法。估算不是模型实际用量，OCR 未执行不能写成功。

最新结构分包与各层内容去重合同见 [packing-review](packing-review.md)。旧分包inline body/supporting持久格式与verify回显requirements均待按新合同替换，不作为最终实现。
