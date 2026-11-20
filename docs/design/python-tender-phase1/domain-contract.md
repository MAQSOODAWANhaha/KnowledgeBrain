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
当前包未提供某章正文不代表全文缺失。verify 只发 review：宿主把本包正文和同一页、同一节的原文装进这一次请求。目标缺失、歧义或仍未决时进入人工，不递归补读。

抽取已经写下的关系随这次 review 一起看。模型没写进台账的跨源组合不再另开一轮。空 findings 不清除既有问题。回执留在 verification_ref，并进入确认摘要。
补遗决议按字典的 AmendmentEffect 明确受影响原项、修改范围、全部保留规则和替代事项；
纯删除不强造替代项。当前结果由原台账及有效决议投影，不增加可编辑的当前规则台账，
不由宿主做自然语言范围运算；漏抽或原项错误通过显式领域修订后重验。

自然语言义务、数字归属、比较关系、条件/例外、作用域、冲突及引用含义由带完整来源
上下文的 LLM 独立核验及必要的人工裁决确定。宿主校验结构、来源定位、覆盖、身份版本、
预算和执行状态；正则和词表不能作为最终语义裁判。模型空 findings 不能清除既有宿主问题。
正则和词表只提供线索，不能代替这次 review。

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
prompts, concurrency=2, timeout_seconds=60).call(task,payload,images=()) -> dict`。
images 是 bytes+MIME 或 data URL，只用于实际来源图像，不拉取任意远程 URL。
类实现 async close/context manager。task 对应版本化领域提示（extract/verify/plan/ocr）。
默认连续60秒未收到非空原始响应字节则超时；首响应头与首个body共用初始期限，收到
心跳、推理或其他流字节后重新计时，不限制请求总时长。编制及保存稿复核使用同一默认值。
timeout及`retry_policy=bounded_work_unit_deadline_v1`属于冻结执行身份；旧执行须沿匹配
完整身份的冻结runtime恢复。仅显式指定旧180秒不足以兼容新的退避合同，不自动迁移或
改写已有快照。编制另冻结`outline_source_binding_contract=batch_excerpt_coverage_v1`，
来源包按实际提供的正文范围校验，在现有并发上限内执行，最终按计划顺序归并。

每次请求唯一 request_id；prepared → sent（发送前持久）→ succeeded/failed/uncertain，
真实 usage 只取服务端返回。SDK 重试关闭。ModelInfrastructureError 表示可重试网络/429/5xx，
ModelSemanticError 表示 JSON/拒答/结构错误，ModelConfigurationError 表示配置错误。
已收到部分流后的超时或无法确认服务端执行结果记uncertain；首数据超时沿既有策略记failed，
两者没有供应商用量时均保留未知，不声称未计费。框架恢复不能承诺模型 exactly-once。
队列与 dispatcher 共用命令基础设施失败计数，最多 4 次后明确 failed；进程失联恢复与语义修复
不依靠该次数做业务判断。不写输出 token 上限，不将累计 usage 当单次 context 容量。

429沿30/60/120秒退避，Retry-After在现有120秒上限内参与计算；其它
失败仍沿原短退避。等待截止`not_before`随失败审计/纠正回执持久保存，恢复只等剩余
时间，发送仍检查租约、取消和四次额度；旧记录缺字段使用可用的实际失败/提交时间回退。
首原始字节、首解码块、首非空正文、最大raw静默和模型客户端semaphore排队分别计量；
未知值保持缺失。请求耗时总和、区间并集、首发至末完成跨度分列，后者包含内部准备/
退避空档，均不冒充完整任务墙钟；计时与观测不进入模型请求身份。

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


