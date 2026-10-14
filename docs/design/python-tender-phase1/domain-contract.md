# 领域模块实施接口（骨架后启用）

本文先定义各目录共享的最小 JSON 合同，避免并行实现产生第二套状态机。大型 JSON 仅在
内容寻址文件中持久化，checkpoint 只存文件引用。所有数据版本为本次新版本，不做旧数据迁移。

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
- 分包保留同节及祖先节的导语/条件与表格表头来源；supporting 复用源 range_id，正文身份
  在所有包之间互斥。导语范围必须有来源，字符偏移基于 native unit；不得虚构 Word 段落坐标。
