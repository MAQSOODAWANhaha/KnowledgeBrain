# 招投标业务工作流、编制架构与运行时接口

本页统一定义招投标的**分析流程、发送边界、编制架构、实际模型调用表**以及**证据层/模型层运行时接口**。字段与枚举合同见 [字段字典](field-dictionary.md)，工作台与 Word/ONLYOFFICE 见 [工作台与 Word 编辑](workbench.md)，验收口径见 [验收与语义评测](acceptance.md)，当前动态进展见 [招投标入口 · 当前状态](README.md#当前状态交接)。

---

## 分析主流程与发送边界

**登记文档集（`register`）→ 解析冻结（`freeze`）→ 统一分析（`extract`）→ 独立核验（`verify`）→ 人工确认（`confirm`）→ `analysis-ready`**

| 节点 | 输入与产物 | 放行条件 |
| --- | --- | --- |
| `register` | 主件、附件、澄清/补遗身份、版本、角色与缺件问题 | 原件持久保存、身份可追溯；缺件不悄悄排除 |
| `freeze` | 规范正文、章节、表格/合并、图片 OCR、附件位置与解析缺口 | 可读范围可推进；必要来源缺口保留并阻断最终确认 |
| `extract` | 小批正文 + 必要上下文 → 统一事项、逐范围处置、关系与未决引用 | 宿主组织逐范围处置；局部回包缺陷保留有效结果与待核问题继续核验 |
| `verify` | 冻结来源 + 抽取事项，按预算分批独立 `review` | 区分硬阻断与人工待核；缺页或漏报原子不再触发额外模型轮次 |
| `confirm` | 展示来源、事项、评分/后果、关系与问题，逐条裁决语义疑点并确认摘要 | 无硬阻断、语义疑点均已驳回或保留、摘要与当前版本一致；硬阻断仅阻止确认，不阻断编制或导出 |

- LangGraph 固定图为上述五节点（`services/docreader/workflow/graph.py`），无报告生成节点，不按业务类别拆多 Agent；`locate`、`associate`、`joint` 已退役，核验只发 `review`。
- **职责分工**：模型负责语义抽取、证据选择与关系判断；宿主负责身份、原文定位、反向索引、分包、预算、存储与流程推进；正则/词表不能充当最终语义裁判。

### 当前发送边界

1. **分析调用数**：实际准入的抽取包数 + 准入的 `review` 请求数（OCR 另计）；分开统计计划条目、唯一逻辑请求与物理发送；每个逻辑请求最多合同纠正 2 次、传输重发 4 次，两类分开计数，物理发送上界 7 次（见 [字段字典 · 失败执行的显式恢复](field-dictionary.md#失败执行的显式恢复)），成功回执按完整摘要复用。
2. **准入预算**：在 `131072` 上下文、输出预留 `32768`、安全余量 `4096` 下，单次输入准入上限 `94208`（抽取按 UTF-8 字节估算，review 按校准 Token 估算）。
3. **抽取软目标与紧凑处置**：多范围抽取包输入软目标 ≤ `55,000` 字节且单包正文范围 ≤ `200`（单条不可拆大范围按 `94208` 准入）；`non_item` 用固定理由码逐范围声明。
4. **严格 Schema 与触发条件**：v2 文本抽取通过 `response_format.json_schema` 传同源 Schema（计入执行身份）；多个独立触发条件共用后果须列全各条件并保留原文引证。
5. **同源纠错零抽取**：CLI `correct` 继承基线冻结来源与抽取分包，发 0 次抽取，允许引用同文档跨包原文范围，保留未改包产物与既有宿主问题，重排并重验受影响 `review`。
6. **不誊写原则**：模型只回语义事实、短引用 ID 与理由码，不誊写宿主已知或可推导的原文、页码、表格或覆盖名单。

### 核验怎么发

- `verify` 只发 `review`：宿主对整份来源待核正文及同页/同节范围取并集，以 `32,768` 输入 Token 为软目标按文档顺序分批；每个查漏正文范围仅归属一个主请求，未同包覆盖的关系与显式人工作用域按预算补审。
- 缺页/残页记 `analysis_page_source_unavailable`，漏报原子补待核，不再开额外轮次；重复引用、未知编号、上下文环、正文与上下文重叠拒收。
- **同源范围延续（BUG-312）**：冻结来源、分包、作用域与查漏范围一致时，同源纠错优先沿用直接基线的主核验分组，只重发完整摘要变化的请求。
- **PDF 表格与逐字行互补**：同页表框内逐字行唯一匹配单元格（含勾选选项或相邻双格首行）时，查漏请求互带完整表格行与已有事项引证作为阅读上下文；歧义留人工。
- **跨章作用域附带（BUG-299）**：`review-scope` 提交的人工作用域结论除独立作用域补审外，凡查漏或关系补审请求读到该决定的控制条款或目标时一并附带。

### 状态、修订与恢复

- 运行状态：`pending` / `running` / `blocked`（硬阻断）/ `waiting-confirmation`（待人工裁决语义疑点）/ `failed` / `cancelled` / `analysis-ready`。
- 版本隔离：来源变更升 `source_version`；同源纠错升 `domain_revision` 并使旧确认失效；协议格式由 `schema_version` 隔离；问题决议按 `issue_id` 与版本绑定，不按 code 批量清除；必要缺件经 `revise` 的 `expected_document_ids` 声明。
- 恢复边界见 [字段字典 · 失败执行的显式恢复](field-dictionary.md#失败执行的显式恢复)；在途 `sent` 禁止并发重发，改提示/重分包不重置已有额度。

---

## 编制架构与缺资料默认处理

**核心不阻断原则**：任何资格、评分、产品、人员、价格、证明、模板或分析未确认问题，均**不阻止**生成、保存、预览或导出 DOCX——能写的写、未知的留空/待补，文件生成状态与内容齐备状态分开报告。

### 缺资料时的默认处理

| 情况 | 默认处理 |
| --- | --- |
| 名称、型号、人员、参数等必填信息未知 | 保留原字段/格位留空（不破坏原格式时可注“待补充”），不阻断生成 |
| 报价、单价、总价、折扣等最终值 | 保留原表与招标预算/限价，最终值留空交人工填写，不填 0 或自动算价 |
| 无匹配企业知识 | 企业应答留空，不自由编造或拿招标要求复述充当应答；有招标来源支持的背景可保留 |
| 缺证书、报告、授权或签章 | 保留附件/签署位并记待提供，不用生成文字冒充原件 |
| 规定格式来源暂缺或不可读 | 用当前已知结构保留位置与待补说明，不声称已保留未知原格式 |
| 未确认分析或冲突未决 | 固定当前可用快照继续生成，冲突值保留待核对，不静默选边 |
| 局部写作/复核耗尽重试仍失败 | 写作失败留空位；初审接口失败保留未被否定 `U0` 待核；已有明确否定时改稿失败或再核不可用回退安全稿 `S0` |

### 四个语义职责、大文件制作与回退

| 职责 | 责任范围与边界 |
| --- | --- |
| 组织响应位置 | 按招标规定结构与有效要求确定分册、章节、原式表与附件位；宿主管稳定引用，不一事项一章 |
| 准备响应依据 | 有限业务词检索，汇总有知识支撑的共同方案；去重 `supports`；产品型号可选，不因缺型号阻断功能方案 |
| 编写响应 | 依据允许的 `supports` 与原模板分任务编写正文与填格；仅选 `target_ids`/`support_ids`/`attachment_ids` |
| 复核响应 | 核对实际内容、附件及“有候选却判空”的语义空稿；`b/f/a` 精确定位，无资料/人工空位免复核，明确无据才移除 |

- **内容制作方式**：项目化正文由模型按完整业务单元分任务生成；固定格式与原式表由宿主装载（`manual_fill` 不调写作）；证书/报告原件按引用插入；目录/页码/`is_volume` 分册由宿主组装。
- **三对象隔离**：会话大纲（唯一接受显式补丁）、整树回包、人工 Word（分叉，新导出为候选不覆盖人工已保存稿）相互独立；覆盖统计与版本/SHA 不一致标待核。
- **知识用途与履约边界**：产品规格/证书用于事实证明，规程用于方法参考，历史方案仅借鉴结构（历史项目名/人员/型号/价格/承诺绝不迁移）；正式履约承诺须有业务授权。
- **一次改稿与回退（`U0 → S0 → U1`）**：初审明确否定内容剔除得 `S0`；仅 `can_revise=true` 且有足够依据的任务最多改稿 1 次得 `U1` 并再核；改稿失败/空回包/`U1` 再核不可用回退 `S0`；初审接口不可用保留未被否定 `U0` 待核；不自动循环追求“零问题”。
- **宿主边界**：Python 宿主负责身份、不可变引用、预算、缓存、重试、租约与组装导出，不引入通用自由调度 Agent；签章、授权、付款与平台递交属外部业务动作。

---

## 编制目标设计：一条要求链

实施路线见 [修复与改进路线 · W2 一条要求链](../../plans/improvement-roadmap.md#w2-一条要求链)。新的自动大纲默认走 `single_requirement_chain_v1`，条件是：有冻结来源，没有旧原件复核事实，部署未设 `KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT`，调用方未点名旧目录合同 `source_refs_compact_plan_v12`，也不是已保存执行的续跑。调用方未点名任一合同时，后继沿用前任的合同；点名旧目录合同的大纲留在全文目录路径，即使前任是一条要求链也不改写。显式传入 `single_requirement_chain_v1` 仍然有效。旧目录合同只能用于有冻结来源的自动大纲，用在正文或固定大纲上直接拒绝。前任是一条要求链时，追加旧原件复核事实须同时点名旧目录合同，否则 Web 返回 409。Web 新建的大纲任务都带默认标记；切换默认前已入队、没有前任也没有点名一条要求链的任务没有标记，Worker 按旧目录合同执行。分析事项台账是唯一覆盖分母，原式表、固定文字、签章与有据首张原封面由宿主从冻结网格原样复制，标注“不适用”的编号式样为复制边界。

### 新链格式补位与事项归属

流水线：`格式章节选择` → `格式补位` → `编译` → `事项归属` → `宿主结算`（字段见 [字段字典 · 新链事项归属](field-dictionary.md#新链事项归属)）：

1. **格式章节选择（`bid_format_source_outline_v1`）**：通常 1 次逻辑请求（宿主兜底 0 次），仅读格式章节短标题行；单独成行标题、子编号格式与可选提交方式各成一章，同行“或”择一成一章，“不适用”不选。标题行整体超出准入预算时按原文顺序分片，每片 1 次逻辑请求，切点优先落在附件编号或编号式样标题前；每片只回本片标题、不回 `body`，宿主合并后按整节规则校验。单行连同 Prompt 仍超限、或分片请求失败时，该片标题不进大纲并记 `outline_directory_plan_failed`；全部分片失败时本次大纲执行失败。每份登记文档（主件、附件、补遗、澄清）各找一个格式章节锚点，按登记顺序各发各的请求（仅主件有格式章节时与单文档请求相同），每节的隐含复制窗口和不适用范围都止于本节末尾，不跨文档。补遗或澄清格式章节里的每一章记 `outline_directory_format_source`，写明来源文档和页码；不同文档出现同名格式时各章都保留，记 `outline_directory_format_conflict` 交编制人员核定，宿主和模型都不择一。
2. **格式补位（宿主 0 次调用）**：`submission_requirement` 点名且不在格式章节内的必交文件，宿主在引证页及前页找同名独立标题行补成 `outside_format` 候选章，无事项落入则删除。补遗或澄清内联替换某张原式（无格式章节标题、证据页在补遗或澄清里）且格式章节已有同名章时，宿主在该补遗或澄清的引证页及前页找同名独立标题行补成一章，记 `outline_directory_format_conflict`；这一章不是 `outside_format` 候选，不因无事项落入而删除。
3. **事项归属（`item_placement`）**：按预算分批（每批 ≤ `240` 条），每批都带完整位置目录；单条事项连同位置目录仍超出准入预算时不发送，与未采纳批次一样记 `outline_item_placement_failed` 留待核，不中断执行。每批 1 次逻辑请求，模型只回位置短 ID、理由代码（`evaluation`/`performance`/`document_wide`/`not_applicable`）或 `null`（待核）；宿主落账为 `rule_refs`、`constraint_rule_refs`、`outline_rule_excluded` 或覆盖台账 `pending`。

### 新链方案正文与评标待核

- **方案知识应答位**：唯一手填原式、标题以“方案”结尾且原式为纯编制指引的方案章，宿主保留原式说明并在其后追加知识应答位承载归属规则。
- **评标待核裁决**：归属返回 `evaluation` 先留 `pending`，编制人员经工作台抽屉/大纲保存 `evaluation_decisions` 显式裁决去向。
- **方案原文上下文**：经 `source_additions` 绑定冻结主件背景段，仅在 Word 标注「原文上下文（非企业应答）」，不充当企业依据。
- **开标一览表项目名称**：宿主自动识别主件公告唯一项目名称并仅在《开标一览表》唯一空「项目名称」格回填；有补遗/不唯一/非空交人工手填。

---

## 编制实际模型调用

Web 端分大纲执行（`outline_only`）与正文执行（固定大纲）；保存 Word 后可单独复核。已保存版 [PDF 出件](workbench.md#pdf-与报告) 与常规组装新增模型调用为 0。下文均为逻辑请求数，每个逻辑请求各有合同纠正 2 次、传输重发 4 次的恢复额度；整次执行另受按阶段计划累计的运行级上限约束（见 [字段字典 · 调用、预算、恢复与修订](field-dictionary.md#调用预算恢复与修订) 第 7 条）。

### 大纲执行（旧目录编译路径，保留的先前实现）

不满足[一条要求链默认条件](#编制目标设计一条要求链)的自动大纲走下表：调用方点名 `source_refs_compact_plan_v12`，执行规格或前任执行记下该合同，带旧原件复核事实，部署设了 `KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT`，或是切换默认前已入队的无标记任务。已保存的旧稿仍按该快照打开、编辑和导出。删除旧链代码仍待真实对照之后。

这条路径不设 `KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT` 时，B、C 走旧路径，原文选择走 `directory_native_body`。A、M 与旧路径 C 把文档集内全部文档的全部行拼进一个请求，不分片；超出准入预算即拒发，不降级为分片，也不提高单次准入上限。

| 阶段 | 任务 / Prompt | 次数 | 随什么增长 | 单次输入与超限去向 | 代码入口 |
| --- | --- | --- | --- | --- | --- |
| A 目录规划 | `directory_plan` / `directory_ref_plan` + `_v2`（新执行默认 `_v2`） | 固定 2 | 不增长 | 全部文档全文；两次均超限时大纲无章节，M、B、C、P、F 均不执行，只记 `outline_directory_plan_failed` | `_request_directory_plans` |
| M 章内材料 | `directory_material` + 附加段 | 1 | 不增长 | 全部文档全文；超限记 `outline_directory_material_rejected`，其余继续 | `_request_directory_details` |
| B 要求归属 | 旧路径 `directory_requirement`；原生 `_native` | 旧路径按行数打包（每块约 600 行，整页不拆）；原生按预算分包 | 随来源规模线性 | 旧路径单块超限记 `outline_directory_requirement_rejected`；原生受预算约束 | `requirements()` / `_request_native_requirements` |
| C 归属分类 | 旧路径 `directory_classification`（可选 `_reviewed_facts`）；原生 `_native` | 旧路径固定 1；原生装得下 1 次，装不下 `G≤1 + S` | 旧路径不增长；原生随待核案例数 | 旧路径全部文档全文，超限整体记 `outline_directory_classification_rejected`；原生仍超限的案例留待核 | `_request_directory_details` / `_request_joint_classification` |
| P 长章展开 | `directory_parameter` | 每个 ≥25 行叙述章 1 次 | 随长叙述章数 | 单章，超限记 `outline_directory_parameter_rejected` | `_request_directory_details` |
| F 响应原式 | `directory_response_form` + `_v2`/`_v3` | M 选中的每个逐条响应原式 1 次 | 随选中原式数 | 单原式来源范围 | `_request_directory_details` |
| 原文选择 | `directory_parameter` / `directory_native_body` | 按规划分包，每包 1 次 | 随目标原文量 | 受预算约束 | `resolve_native_bodies` |

- B/C 原生合同与原文选择由 `KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT` 互斥切换。部署设了该变量时，新的自动大纲也留在这条路径并使用原生合同。
- 新链（`single_requirement_chain_v1`）的目录规划只发 `bid_format_source_outline_v1`，不发 M、B、C、P、F，共 `格式章节数与分片数（通常为 1）+ 归属批数` 次逻辑请求；单次输入边界与缺口见[新链格式补位与事项归属](#新链格式补位与事项归属)。

#### F 原式的条款号

原式依据支持逐条展开、重复区紧接「条款号」表头且该列在重复区内全空时，`sequence_column` 绑定该列；每行首个正文引用选入完整点分数字条款号，宿主复制条款号与所选正文，保留固定格、空行与签章表注。

### 正文执行（固定大纲）与保存后复核

| 阶段 | 任务 | 次数 |
| --- | --- | --- |
| Q 知识检索 | HTTP（非模型） | 每检索词 1 次 `search`；命中文档各 1 次 `document`；需原件各 1 次 `file`（各最多 4 次尝试） |
| 依据 | `authoring_basis` | 按预算分批，每批 1 次 |
| 结构复核 | `authoring_structure` | 每次正文执行 1 次（超限改紧凑投影，仍超限记告警） |
| W 写作 | `authoring_write` | 每个有允许依据的任务 1 次（大任务按子片拆）；全人工/无依据不写 |
| 正文复核 | `authoring_review` | 按预算分批，每批 1 次（含“有候选却判空”语义空稿） |
| 改稿 / 再核 | `authoring_write` / `authoring_review` | 仅 `can_revise=true` 任务至多 1 次改稿 + 1 轮受影响再核 |
| 保存后复核 | `review_saved_word` → `authoring_review` | 按预算分批，每批 1 次（`services/docreader/tender/saved_word_review.py`） |

### 执行身份范围

执行规格只计入本执行实际可调用任务的 Prompt 与输出契约哈希（[BUG-259](bug-fix.md)：改核验 Prompt 不影响编制身份，改目录 Prompt 不影响固定大纲正文身份），详见 [字段字典 · 调用、预算、恢复与修订](field-dictionary.md#调用预算恢复与修订)。

---

## 运行时接口合同（证据层与模型层）

- **证据层**：`register_manifest(manifest, artifacts)` 持久化原件；`freeze_sources(registration, artifacts, ocr)` 在线程/进程池异步产出冻结索引（每文档 raw、每 pack 存内容寻址文件，checkpoint 只存引用）；`EvidenceRepository(artifacts).iter_packs` / `load_pack` 复用冻结 refs。OCR 回调只用于真实图片且不覆盖原件，文本 PDF 禁止静默全页 fallback；附件无正文记 `attachment_content_not_extracted`。
- **解析与分包边界**：
  - Excel 正文取唯一 row 范围、raw 留全网格、drawing anchor 补图片偏移、全空行压缩视图但留 `body` 身份、`display_incomplete_reason` 升级为可见问题；Word SECTION 保留原生定位；PDF 补齐 image occurrence，重复页边与不可读图记 `partial`，原件损坏属技术失败。
  - `supporting` 只累计前文同节/祖先节条件与表头（混合小章包用 `scope_binding.applies_to_range_ids` 绑定）；无法切出完整条件句时写 `pack.pending_scope_context`（仅定位与 `scope_binding` 元数据，不含正文/表格/图像），模型空 findings 不解除阻断。
- **模型层**：`ModelConfig.from_env(mapping=None)` 单一配置源；`OpenAIModel(config, store, lease, prompts, concurrency=2, timeout_seconds=60).call(task, payload, images=())` 关闭 SDK 重试，仅接受本地图像字节/data URL。
  - 默认连续 60 秒无非空原始响应字节超时（收到流字节重计时，不限总墙钟）；`timeout`、`retry_policy=split_transport_contract_v1`、`outline_source_binding_contract=batch_excerpt_coverage_v1` 计入冻结执行身份。
  - 请求状态 `prepared → sent → succeeded / failed / uncertain`（真实 usage 缺失留 `null`，收部分流后超时记 `uncertain`）；错误分 `ModelInfrastructureError` / `ModelSemanticError` / `ModelConfigurationError`；429 按 30/60/120 秒退避并持久化 `not_before`；同一进程内某端点收到 429 后，该端点的全部并发槽位在冷却结束前都不发新请求（冷却取 `Retry-After`，至少 30 秒、至多 120 秒；不跨进程共享）；首原始字节/首解码块/首非空正文/最大 raw 静默/排队耗时独立计量且不进请求身份。
