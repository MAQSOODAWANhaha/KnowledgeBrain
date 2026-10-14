# 领域模块实施接口（骨架后启用）

本文先定义各目录共享的最小 JSON 合同，避免并行实现产生第二套状态机。大型 JSON 仅在
内容寻址文件中持久化，checkpoint 只存文件引用。所有数据版本为本次新版本，不做旧数据迁移。

冻结恢复细化：图节点按文档登记 `freeze-document` 回执、按实际来源图登记 `ocr` 回执，
之后合并小索引为全局 freeze 回执。部分文档/图片成功后其他范围基础设施失败，重投复用成功
回执，不重复全量解析/OCR。抽取与验证同样按 pack 独立回执。技术失败允许独立范围先完成，
全局只在所有必需范围具有成功产物或明确领域未决处置后推进。
响应计划按已验证要求的原包分组调用并保存 `plan-pack` 回执，再合并为逐要求计划；
避免把所有来源/要求重发到单个大上下文。全局检查仍覆盖全部适用要求，局部计划失败阻断终点。

## 证据

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

## 模型层

`ModelConfig.from_env(mapping=None)` 是单一配置解析源。`OpenAIModel(config, store, lease,
prompts, concurrency=4, timeout_seconds=120).call(task,payload,images=()) -> dict`。
images 是 bytes+MIME 或 data URL，只用于实际来源图像，不拉取任意远程 URL。
类实现 async close/context manager。task 对应版本化领域提示（extract/verify/plan/ocr）。

每次请求唯一 request_id；prepared → sent（发送前持久）→ succeeded/failed/uncertain，
真实 usage 只取服务端返回。SDK 重试关闭。ModelInfrastructureError 表示可重试网络/429/5xx，
ModelSemanticError 表示 JSON/拒答/结构错误，ModelConfigurationError 表示配置错误。
断连/超时或无法确认服务端执行结果必须 uncertain；框架恢复不能承诺模型 exactly-once。
队列与 dispatcher 共用命令基础设施失败计数，最多 4 次后明确 failed；进程失联恢复与语义修复
不依靠该次数做业务判断。不写输出 token 上限，不将累计 usage 当单次 context 容量。

## 抽取、验证与计划

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

`plan` 返回 `items:[{requirement_id,target_kind,target,action,material_needs:[]}]`。
target_kind 为 body/table_field/attachment/manual_action；target 是具体位置字符串，action
必须说明该要求的实际响应动作，material_needs 非空（所需材料/用户输入，可明确“无需外部材料”
及依据）。表格目标必须标识具体字段，正文目标须有章节定位及写作动作；仅标题/空槽位拒绝。
所有适用要求恰好覆盖，引用要求不存在或缺项均阻断；终点 `response-plan-ready`。

规则细化：数字以 Decimal 和原文单位反校验；只有来源明确 scoring.total 与 scoring.components
时检查声明合计，不推定 100。同主体/条件/例外且去数字后同文、数字不同的跨文档要求标记
潜在冲突待明确，不按日期覆盖。声明修改的补遗/澄清必须具有 replaces.document_id 及
range_id 或 scope；普通澄清不声明修改则保留但不覆盖其他来源。全空 body 行可规则生成
`non_requirement`（依据 empty_table_row），其他范围必须模型逐一处置。

## 已识别的设计细化

- 现有 Excel 同时有 sheet/table 网格与 row units：正文覆盖选择唯一 row 范围；raw 保留全网格。
- 现有 Word SECTION 可能聚合多段：保留原生定位，不捏造更细坐标。
- PDF parser 失败时可能退回全页图：新适配必须显式检测并报告，不能把文本 PDF 全页发 OCR。
- 原生 PDF 对小图及重复图有过滤；新增 evidence 适配调用正常原生路由，补齐实际
  image object occurrence 及坐标，绝不打开 output_inventory 的逐页渲染模式。原生失败返回
  文档问题；重复页边文字、无法读取图像等覆盖缺口显式记为 partial，不悄悄通过。
- 原生 Excel 图片只有 drawing 序号；evidence 从原件 drawing anchor 补齐单元格/偏移。
  同时使用 raw grid 补齐 row unit 中省略的空单元格。全空行只压缩发送视图，范围身份仍在
  body（empty=true、non_requirement_reason=empty_table_row）及 compacted_blank_ranges；
  抽取对其登记有依据的非要求处置。部分空行保留完整列位置，合并关系只读原网格。
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

## 追加正确性评审合同（2026-10-14，先于修复）

- 验证模型的 `blocking` 只能提高阻断级别。宿主对漏项、证据/数字/条件/适用性/冲突等
  正确性问题规定最低阻断等级，未知问题默认阻断；仅明列的纯信息类可非阻断。
  人工问题处理不是确认放行；问题未解决不得产生 response-plan-ready。
- 响应计划引用不可变要求约束快照及摘要（对象、义务原文、条件、例外、数值/单位、
  比较关系等），每项结构化绑定必须与经验证要求一致。裸数字相同不能证明义务保真。
  30工作日/30自然日/30天、至少/至多、适用对象/条件变更均应拒绝；保留人工语义复核边界。
- 分包需绑定章节/祖先中的累计前置适用条件及来源，不只取最近一条。“仅第一标段适用”
  与“联合体还需”必须同时出现在相关后续包的 supporting 中。结构能确定的范围按章节
  绑定，不能可靠确定条件作用域/替代关系则返回阻断问题，不能默默丢上下文或塞全篇。

派生 supporting ID 仅是有来源的引用身份，不增加正文范围计数。若多个片段来自同一正文
范围，它们分别保留准确连续原文及偏移；不能用省略号拼接不连续原文冒充一个证据范围。
真实 PDF 离线预检的原件、解析正文、图片和冻结包只写私有目录；Git 只收摘要 hash、计数、
页码、问题类别、覆盖断言与 token 估算方法。估算不是模型实际用量，OCR 未执行不能写成功。

最新结构分包与各层内容去重合同见 [packing-review](packing-review.md)。旧分包inline body/supporting持久格式与verify回显requirements均待按新合同替换，不作为最终实现。
