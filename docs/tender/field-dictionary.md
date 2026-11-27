# 招投标字段与协议字典

本页是招标分析与编制（`services/docreader`）字段名称、类型、可选性、枚举与身份语义的唯一规范。处理顺序与调用公式见 [业务工作流](workflow.md#当前发送边界) 与 [编制实际模型调用](workflow.md#编制实际模型调用)，数据库内部表名与外键规则见 [共享平台与数据库](../platform/README.md#数据库命名状态与约束约定)，动态部署与验证事实见 [招投标入口 · 当前状态](README.md#当前状态交接)。

## Web 项目来源设置

`GET /api/v3/tender-projects/{project}/sources` 返回 `source_settings`，包含保存令牌 `version`、已声明的 `expected_document_ids` 和逐文件的 `document_id`、`file_name`、`role`、`replaces`、`modification_scope`。同路径 PUT 接收 `expected_version`、`expected_document_ids` 与完整 `documents`，逐文件提交 ID、角色、替代文件与适用范围。角色为 `main`、`attachment`、`clarification`、`amendment`。替代文件可空，非空时指向同项目的其他已上传原件。适用范围作为登记元数据随原件冻结，效力由文件实际条款确定。

- 单次输入不超过 94208 token（`o200k_base` 校准边界，详见下文）；输出只含判断、问题或填空，不回写宿主已索引原文范围。
- 硬阻断只阻止把该版本确认成 `analysis-ready`，不拒绝未确认快照的读取、保存、预览或导出。

## 分析业务协议（v2）

工作区 `tender/schema.py`、抽取/核验提示与五节点主线使用 v2（旧回包不自动转换；核验只发 `review`）。分析目标为 `register → freeze → extract → verify → confirm`，确认后到 `analysis-ready`。输入为招标文件、附件、澄清和补遗，不引入企业响应数据、准备待办或实际得分；原文明示的填表、证明、附件与签章要求属于来源事项。

### 事项 TenderItem

| 字段 | 类型 | 必填 | 统一含义 |
| --- | --- | --- | --- |
| `item_id` | 非空 string | 是 | 当前台账内的事项身份，不编码业务含义 |
| `kind` | ItemKind | 是 | 事项的业务类型，与业务领域不同 |
| `text` | 非空 string | 是 | 忠于来源的完整事项，保留条件、例外、数值归属和规则，不填写企业实际内容 |
| `evidence` | 非空 Evidence[] | 是 | 每项为 `{range_id, quote}`；quote 是能回到冻结原文的非空引文，至少一处来自本包 body |
| `domains` | 非空 string[] | 否 | 人员、技术、服务等可交叉的查漏维度，不作为固定路由或完整业务枚举 |
| `subject` | 非空 string | 否 | 原文明确的受约束主体或对象 |
| `scope` | 非空 string | 否 | 原文明确的标段、对象或阶段范围；不判断企业已经符合 |
| `conditions` | 非空 string[] | 否 | 来源中的适用条件；缺省不表示不存在条件 |
| `exceptions` | 非空 string[] | 否 | 来源中的例外；独立于conditions，缺省不表示不存在例外 |
| `numbers` | 非空 Number[] | 否 | `{value: string或number, unit: string, quote: 非空string}`；value 不接受bool或空字符串，unit 为来源单位（无量纲可为空），quote 为对应原文 |
| `scoring` | 非空 ScoringDetails | 否 | 仅供 scoring_rule；已明示的计分方法、分值或上限 |
| `consequence` | 非空 ConsequenceDetails | 否 | 仅供 consequence_rule；已明示的后果及触发条件 |
| `submission` | 非空 SubmissionDetails | 否 | 仅供 submission_requirement；原文要求填写的字段名或提供的文件名称 |

`ItemKind` 仅有以下四个值，不使用旧 `veto_risk` 布尔值代替后果规则：

| 值 | 含义 |
| --- | --- |
| `obligation` | 资格、技术、商务、人员、服务等义务或约束 |
| `scoring_rule` | 评分指标及其分档、公式、加扣分、上限和计分条件 |
| `consequence_rule` | 否决、无效、不得参与、履约责任等独立于计分的后果规则 |
| `submission_requirement` | 原文明示的文件内容、表格字段、附件、证明、签章、格式及提交要求 |

可选细节只保留首版需要的少量字段；所有子字段均按来源可选，出现对象时至少有一个有效字段：

- `ScoringDetails`：`method: string`、`points: number`、`max_points: number`。
  points 仅表示本事项一次条件成立时明示的固定得分或加扣分，加分为正、扣分为负、不得分为0；
  不是企业实际得分，不用它任选某一分档或替代公式结果。max_points 仅表示来源明确规定的
  本指标可得分上限，为非负数；不是最大扣分幅度，也不从子项相加推定父项上限。
  分档、公式、封顶和重复计分限制完整保留在 text/method，无法无歧义使用标量时省略；
  不建立公式执行器，没有明示合计不推定总分100。
- `ConsequenceDetails`：`effect: string`、`trigger: string`。记录原文后果和触发含义，不能把扣分、不得分标成投标否决。
- `SubmissionDetails`：`fields: string[]`、`documents: string[]`。只记录源文要求的字段或文件名称，不出现实际填写值、文件齐备状态或准备动作。

例如“每个业绩2分，最高6分”可给 points=2、max_points=6；“每缺一项扣2分，最多扣6分”
可给 points=-2，扣分上限完整写入 method，不能写成 max_points=6。“优秀5分、一般2分”
保留完整分档，不给单个 points；父子指标的共同上限及层级在 text/method 和来源关系中保留，
不得把不同层级或不同条件的分值直接相加。

### 可选性与完整性

- 事项层面仅四字段必填。其余字段有源文依据才提供，不输出 `null`、空容器或“无/未知”占位；合法零分、零数量必须保留。
- 省略只表示未作结构化声明，不等于原文没有、不适用或已经核验。歧义、缺必要原文和未解析引用进入 Issue。
- 可选细节不替代完整 text/evidence；不要求每类信息都重复结构化，但原文明示的重要规则不得丢失。
- 一段可以生成多个事项并共享证据。评分规则可含完整分档，不为增加条数而拆散共同条件或封顶规则。
- 加分、扣分、不得分统一记为 scoring_rule；同一计分事实不再重复建后果事项。有独立的否决或其他后果时，另建 consequence_rule 并关联。
- 原始事项均保留，来源的适用范围和补遗修改通过 scope、关系及问题决议说明，不以缺省 applicable 过滤台账，也不自动沿用 v1 的整条失效推断。
- text、理由和业务细节使用源文语言；ID、枚举、单位及原文引文保持其协议或原文形式。
- 必填协议集合允许为空，例如 `issues: []`、无事项时 `items: []`，与事项可选字段规则不同。

### 身份、关联与问题

| 字段/类型 | 目标定义 |
| --- | --- |
| `work_unit_id` | 宿主固定的子操作身份，按请求内容稳定编号（前缀+内容摘要），复用现有 stage/scope 回执，不另建任务系统；抽取时由本包身份提供 |
| `item_id` | 模型使用 `<work_unit_id>:i1` 等格式，正整数后缀不要求连续；宿主检查当前台账内唯一 |
| `ItemRef` | `{artifact_ref, item_id}`，持久引用不可变事项；同名 ID 不代表跨修订语义相同 |
| `Relation` | `{relation_id, from_item_id, to_item_id, relation, evidence}`；relation 为非空的来源关系说明，evidence 为非空 Evidence[] |
| `relation_id` | `<work_unit_id>:l1` 等台账内唯一身份；抽取由宿主按原关系数组位置派生，模型只输出端点、relation和evidence；后续关联/联合协议不变。端点必须是实际提供的事项 ID，不能从标题编造 |
| `Issue` | 模型公共问题结构 `{code: 非空string, detail: 非空string, blocking: bool}`，按实际定位可附 `range_id: 非空string`、非空 `item_ids: string[]`；新协议可附 `related_issue_ids`、`needs_source`，语义见下文；抽取 issues 与核验 findings 使用同一类型 |
| `issue_id` | 宿主为已持久化问题赋予的具体身份；解决问题须绑定它及当前版本，不能仅按 code 清除 |
| 未解析引用 | 使用 Issue，必须以 range_id 定位引用处，detail 保留原文引用及未决原因；目标未知时不填虚假 item_id，不新增引用状态机 |
| `Decision` | `{decision_id, issue_ids: string[], reason: 非空string, evidence: 非空Evidence[]}`，可附非空 `relation_ids: string[]` 和一个 `amendment: AmendmentEffect`；是待独立核验的决议提案，不是模型自行解除阻断的指令 |
| `decision_id` | `<work_unit_id>:d1` 等当前台账内唯一身份；issue_ids 只引用输入中宿主已赋号且本提案拟完整解决的问题，只有不解除既有问题的补遗决议才允许为 `[]` |
| 问题决议 | 通过 Decision 绑定具体 issue_id、当前来源/领域版本、证据和理由，使用现有不可变修订/审议回执；独立核验明确接受后才生效。仍未解决的语义疑点只能在确认时由人工逐条处理（见[问题分级与人工处理](#问题分级与人工处理)），硬阻断不能由确认处理 |

关联只表达源文事实，例如评分作用对象、证明文件对应义务、否决触发条件或补遗修改对象。
抽取先记录本包已知关联；后续补读形成独立不可变的关联决议并接受核验，不改写已保存回包。
抽取接收不按文本去重关系，也不推断重复事项身份或未知端点。宿主编号保留原数组位置，
无效关系的待核出口不变，原始模型回包保持只读。抽取专用无ID输出Schema与Prompt一起
进入冻结身份；人工纠错的持久Relation仍提交完整relation_id，不热替换旧运行合同。
跨包持久选择通过 ItemRef；核验请求中的 item_id/relation_id 均须从实际装载的当前台账取得。
Decision 中的事项选择同样由宿主持久绑定 ItemRef；问题身份绑定其原始产物，不按 code、
相同文字或同名 ID 猜测目标。最低阻断策略沿用宿主规则，模型不得把正确性或未知问题降为提示。
Decision.reason 须说明所列每个问题如何被证据完整解决；仍未决的问题不列入拟解决列表，
原记录继续阻断，不能通过接受“保持未决是合理的”提案将其清除。

### 补遗修改与当前有效规则

`AmendmentEffect` 的字段全部必填，携带 amendment 的每条 Decision 只处理一个原事项：

| 字段 | 类型 | 含义 |
| --- | --- | --- |
| `target_item_id` | 非空 string | 实际提供的受影响原事项；原事项与证据保持不变 |
| `effect` | `replace/delete` | 来源明确替换或删除，不依据发布日期、上传顺序自动推断 |
| `affected_scope` | 非空 string | 修改的标段、对象、条件及具体条款部分；整条修改也须明确说明 |
| `retained_rules` | EffectiveRule[] | 修改后仍有效的全部原事项内容，逐条明确保留范围；`[]` 明确表示没有保留部分 |
| `replacement_item_ids` | string[] | replace 时非空，引用已抽取的替代事项；delete 时为 `[]`，不虚构替代事项 |

`EffectiveRule = {text: 非空string, scope: 非空string, evidence: 非空Evidence[]}`，只用于决议中
明确保留的原规则，不产生新的 item_id。Decision.evidence 须包含目标原条款、补遗修改及
作用范围所需依据；替代事项及其完整来源也须提供给独立核验。保留内容和范围由 LLM 或
有证据的人工裁决明确给出，宿主不能做自然语言 scope 相减、字符串替换或整条失效推断。

例如原事项“第一、二标段均在30天内交付”，补遗“仅第二标段改20天，其他不变”：
affected_scope 为第二标段交付期限，retained_rules 明确保留第一标段30天，replacement_item_ids
指第二标段20天的事项。纯“取消该条”用 delete、空替代列表表示，不强造义务或 Relation 端点。
若同一原事项还含未改保修内容，也须写入 retained_rules，不能随交付期限一起丢失。

当前视图对未受修改事项显示原项；对受修改事项显示已接受决议的保留规则与限定范围内的
替代事项，并保留回到原项、补遗及决议的引用。决议不是另一份可编辑台账，不增加 analysis_ref。
保留片段按 EffectiveRule 自身内容展示，不自动继承原项 numbers/scoring/consequence/submission
等可选字段为当前值；原完整字段仍可在历史原项查看。例如补遗取消评分上限后，不能让原项的
scoring.max_points 在保留片段中重新生效，也不为此把 EffectiveRule 扩成另一套 TenderItem。
替代事项按实际身份只计一次；同一目标的连续修改须形成涵盖相关补遗的完整当前决议，
不能同时接受作用范围重叠且矛盾的提案，也不能由宿主按时间戳挑选“最新一条”。
作用范围不明、保留部分不完整、替代事项漏抽或多份补遗的先后效力未决时登记阻断 Issue；
漏抽或内容错误须显式领域修订后重验，不在决议中夹带新事项或改写原抽取产物。

### 抽取、核验与阶段引用

| 字段/协议 | 目标定义 |
| --- | --- |
| 抽取输出 | `{schema_version: 2, items: TenderItem[], dispositions: Disposition[], relations: Relation[], issues: Issue[]}`，五项均必填。被事项证据引用的范围不声明，由宿主派生 `items` 处置；其余每个正文范围必须恰好声明一次，`non_item` 或 `unresolved` |
| 抽取处置声明 | 生成合同要求模型返回 `{range_id, status, reason_code?, reason?}`。`status=non_item` 时要求给 `reason_code`，取值 `page_number`（页码、页眉页脚）、`blank`（空白或空行）、`heading`（标题、目录、导航）、`form_field`（表头或填写栏，要求已由所属表头或说明表达）、`repeated`（重复的版式或说明）、`informational`（纯项目背景、概况、基本信息标签或名词解释，严禁用于一般合同条款或评标程序）、`other`；`other` 必须另给 `reason` 文字。`status=unresolved` 必须给 `reason` 文字且不得给 `reason_code`。宽容接收边界允许省略 `reason_code` 但给出非空 `reason` 的 `non_item`（记低级诊断，不丢弃有效处置）；持久化时若模型给了具体 `reason` 则保留，否则展开为代码对应的固定中文说明，持久化格式不变 |
| `Disposition` | 持久化为 `{range_id, status, reason, item_ids}`；status 为 `items/non_item/unresolved`，reason 为有来源依据的非空说明。规范产物仍覆盖每个 body 范围 |
| `item_ids` | 宿主按事项的有效直接 evidence 派生反向索引，模型不再生成。规范产物中 items 处置非空，non_item/unresolved 时为 `[]`。既未被引用也未声明的范围是合同缺陷，不是允许的省略：宿主保留已抽取结果，对该范围记 `extraction_disposition_unresolved` 并继续，不整包拒收重发（BUG-250），也不新增第二轮补处置发送，数量进入执行报表 |
| 核验模型输出 | `{schema_version: 2, accepted_decision_ids: string[], findings: Issue[]}`，三字段必填；接受集合只能选择本次输入决议 |
| 核验接受回执 | 宿主添加`reviewed_range_ids/reviewed_item_ids/reviewed_relation_ids/reviewed_decision_ids`，来自实际发送的输入；表示执行覆盖，不是模型声明或语义完整性证明 |
| 核验覆盖 | 宿主按成功接受的请求记录已提供的来源、事项、关系和决议；全局核对必要覆盖并集及候选联合核验回执，共享上下文可重复送审，不计为新增正文归属；ID 覆盖不证明语义正确 |
| `task` | 目标一期使用 `ocr/extract/verify`；verify 内使用下文固定 operation，提示、操作、版本及实际输入进入请求摘要 |
| `items_ref` | 替代现行 requirements_ref，指完整抽取事项/关联/处置的产物索引，不复用为某个单包引用 |
| `verification_ref` | 指各子操作产物、核验结果、候选联合审阅覆盖、关系/决议及具体问题；核验不得静默修复错误事项 |
| 目标 checkpoint | run_id、source_version、domain_revision、status、registered_ref、frozen_ref、pack_plan_ref、items_ref、verification_ref、checklist_digest |
| `checklist_digest` | 绑定当前来源/领域版本、冻结来源、分包、完整台账、关联/问题决议和核验产物；任一相关修订使旧确认失效 |
| `schema_version` | 本次事项抽取/核验输出为2；来源合同沿用现行版本；与 source_version/domain_revision 分开，不自动转换旧回包或改写历史记录 |

同一 body 范围同时含确定事项和未决内容时，处置为 items、保留已抽取 item_ids，并另记阻断 Issue。
unresolved 加空 item_ids 只用于该范围尚无可确定事项；不能因局部歧义丢掉已有结果。
覆盖沿用 SourceView 的来源身份：压缩模板的每个原始行 ID 仍需处置和独立核验；supporting、
别名及其已提供片段进入核验清单，但不重复计入正文归属。已证实空白页/显式审议装饰图仍由宿主单列。

填报要求归属于有明确文字依据的表头或说明所在 body 包。后续纯空行结合这些上下文审阅后，
若没有新增独立要求，可逐行处置为 non_item，说明义务来源和无新增内容；不需要为每行生成
事项或伪造非空引文。例如“人员表填写姓名、证书编号”在表头包生成提交事项，后续空行记录
“字段填写要求已由该表头表达，本行无新增独立要求”。结构含义不明则保留 Issue，宿主不能
因空白模板标记直接代替模型处置，也不泛化允许仅凭 supporting 新抽事项。

### 回包容错与继续核验

模型负责语义：识别事项、评分、条件、适用范围，选择相关证据，解释证明、否决和补遗修改
等业务关系。代码负责组织：固定来源和事项身份、装载/分包原文、定位引文、派生反向索引、
管理引用及不可变产物、预算和流程推进。代码不能依据关键词、数字集合或位置邻近推断
业务关系；反向 item_ids 仅表示直接引用，不表示资格满足、条件适用或规则替换。

中间节点尽量保留可核验结果，局部格式问题不得使整包有效事项丢失；业务正确性由独立
Verify 和人工确认收口。不新增流程节点、业务正则或改写内容的模型修复循环。原始模型回包和冻结
原文保持不变，宿主只生成派生结果；模型请求完成不等于业务已核验。

- 引文先按实际发送文本和冻结范围精确匹配。仅因删除源 U+000A 换行而不匹配时，可在该
  引用范围实际可见文本内唯一定位并恢复精确原文，记录恢复路径及原文位置；不改变事项
  text，不折叠其他空白、标点或 Unicode。不能跨单元格拼接、引用隐藏片段或越过 alias 边界。
  同一规则用于抽取及后续关联证据。定位不明的引文不作为有效证据，登记具体问题后继续
  核验其对应完整原文及其余有效事项。
- `numbers` 是可选增强。模型提供的整个声明不完整、结构不合法或引文无法追溯到该事项
  有效 evidence 所引用范围的实际发送文本时，在派生事项中省略该字段，保留 text/evidence、
  其他字段和原始回包。数字引文与事项引文可互补，不强制两条摘引逐字嵌套。
  共享 Number 的 value/unit/quote 仍必填，不猜补、不用正则判断数字关系，不以省略表示原文
  没有数值。Verify 继续从完整原文核对数值归属、单位、比较方向及遗漏。
- 模型只需逐范围说明 status/reason；反向 item_ids 由宿主生成。收到旧式冗余声明时不据此
  改写证据或认定新关系，差异保留诊断供核对。缺失、重复或与直接证据矛盾的处置保留已知
  事项，并记录未完成核对的问题；不得据此将未读范围判为 non_item，也不让整包执行失败。
  宿主为此补出的 unresolved 仅表示处置待核对，不另冒充“模型已确认来源无法理解”的
  unresolved_source_range；已有明确来源阻断保持不变。完整原文下可由模型提出并独立核验
  接受具体问题决议；若确实需要新增或修改事项，仍须显式修订，不能在决议中补造事项。
- 对不能保留为规范事项或关系的局部声明，记录可定位的问题及待核对内容，保留其他有效
  结果。Verify 必须看到相关完整原文和问题，不能静默删去异常后宣称全部通过。
- review不再要求模型誊写身份覆盖名单；如仍附带四个旧reviewed字段，宿主忽略并记录
  `model_review_scope_ignored`（含fields列表），不信任、不补齐其声明，也不因此拒收。旧协议的父ID冗余和单字符恢复仅保留于历史
  BUG-009/017记录；当前宿主按本次请求派生回执范围，模型仍须报告缺文和语义未决。
- 宿主派生产物可附 `diagnostics` 数组，记录 `{code,path,range_id?,range_ids?,declared_id?,start?,end?}`；它不是模型
  输出字段，不重复复制原文，不自动作为 blocking Issue 或发送给模型。可恢复排版、冗余索引
  和可选细节省略属于诊断；真实来源缺口、无法追溯的事项及业务未决仍使用现有 Issue。
- 核验不再发 locate。同一冻结页或同一节的原文由宿主装进同一次 review，不让模型再点名要补的范围。
- 必填顶层结构无法解析、身份无法安全绑定等执行失败仍报错；原文追溯、上下文容量和付费
  防重放继续生效。历史产物不原地重写；请求协议/提示及宿主处理版本绑定执行摘要，不能
  用升级后的发送视图冒充旧请求的实际 payload。JSON Schema 增强不作为本轮跑通前提。

### 语义评测输入与预算观察（离线合同）

评测语料使用schema_version=2，单例`{case_id, category, provenance, payload, expected}`。
payload直接使用当前extract或verify操作合同；case_id、category、expected和人工记录
只存于本地审阅目录，不进入模型请求。source_views必须通过当前SourceView合同，表格
单元格提供text；items、relations、decisions及处置使用本文件同名定义。跨文档样本
保留各自文档身份和角色；补遗保留原事项及明确的amendment决议，不能沿用applicable。
expected为待人工复核草案，含verdict、rationale、source_evidence和affected_item_ids，
不得以结构校验、模型判断或自动关键词匹配填写人工语义验收。Verify样本不能代表抽取
全量质量；真实文件完整性另以原件逐项核对。

预算观察区分计划请求数/输入估算、实际供应商Token和未知用量；按task及实际可追溯的
子操作记录输出分布、超过output_reserve的次数，并单列实际总Token与配置上下文容量对照，
不把缺失usage记为0或将输出超过预留等同于实际总上下文超限。首次校准先给出
观测报告，不凭单次最大值自动调高预留或新增阶段硬门限，不改已冻结执行快照。

`status.budget_observation`是只读运维输出，不进入LLM或工作流状态：

| 字段 | 定义 |
| --- | --- |
| `calibration_status` | 固定`observations_only`，不表示容量已经校准 |
| `requests` | 实际持久请求条数，含失败及uncertain；不含缓存复用造成的虚拟调用 |
| `tasks` | 按请求task分组；各组含requests、statuses计数、reported_tokens及missing_token_fields |
| `operations` | 按宿主请求元数据区分extract/review/ocr；历史运行里还会出现discovery/reference/associate/joint；旧verify缺元数据时归verify_unattributed，不猜归属 |
| `reported_tokens` | 各有报告的prompt_tokens/completion_tokens/total_tokens之和；完全未知的字段省略 |
| `missing_token_fields` | 每个Token字段未报告的请求数，部分usage分别计算 |
| `completion` | observations、max、p95（最近秩）、reserve_comparisons、exceeds_reserve；无观测时max/p95为null |
| `successful_completion` | 仅传输succeeded的输出样本数、max、p95；排除失败/uncertain，但不代表语义已通过，不单独作为容量保证 |
| `context` | observations、max_reported_total、capacity_comparisons、exceeds_context；只比较实际报告total与该请求记录的容量，无观测时最大值为null |
| `plans` | 已生成的 extract 与 review 计划及各阶段的 planned_requests、input_estimate、max_input_estimate、output_reserve_total、over_budget_requests。这是计划统计，不能与实际 Token 相加。locate、associate、joint 不再生成发送计划。 |
| `planning` | unplanned_phases列出尚未生成的阶段；known_planned_requests只计已知计划；有未知阶段时total_planned_requests为null。此计数不含OCR、重试和纠正，不能充当全程付费调用上限 |
| `deviations` | 逐请求标记，不含原文。`estimate_below_prompt`：实际 prompt_tokens 大于记录的输入估算；`completion_above_reserve`：completion 大于该请求 output_reserve；`slow_first_data`：首个原始字节等待达到 60 秒。字节估算偏高、首包更快、缺字段都不标记。标记不改变 131072 默认窗口、262144 纠错上限、预留或重试 |
| `failures` | 失败聚合表，每行一个「阶段 × 类别 × code」：`stage`（口径同 `operations`）、`task`、`kind`、`code`、`count`、`reported_tokens`（供应商回报的 total_tokens 之和）、`unknown_usage`（无回报用量的条数）。`kind=send` 来自状态为 failed 或 uncertain 的请求行，code 取 `error.type`；`kind=contract` 来自 `model-rejection` 回执，每条回执计一次，code 取首条错误的 code，Token 按 `model-wire` 记录的原始 payload 对应到 `scope`（`request_digest`）再匹配同 task 且状态为 succeeded 的请求行，匹配不到或无 wire 记录（如 OCR）记入 `unknown_usage`。contract 行的 count 之和等于回执条数。按 `reported_tokens`、`count` 降序排列 |
| `limits` | 估算、未知用量及小样本分位数的解释限制 |

`authoring-report` 是编制执行的只读阶段报表（CLI `authoring-report <execution_id>`；网页 `GET /api/v3/tender-projects/{project}/executions/{execution}/report` 返回 `{"report": …}`，执行尚未创建时为 `null`），由执行的请求行和结果产物现场计算，不写入任何产物、不改变执行身份、不发模型请求，旧执行也可以查看：

| 字段 | 定义 |
| --- | --- |
| `execution_id`、`status`、`kind` | 执行身份、状态；`kind` 为 `outline`（大纲执行）或 `document`（正文执行） |
| `stages` | 按请求 task 分组，`stage` 为[编制实际模型调用](workflow.md#编制实际模型调用)里的阶段名。各组含 `logical_requests`（按逻辑请求摘要去重）、`requests`（实际持久请求条数，含重试）、`statuses`、`reported_tokens`、`completion`、`context`，后四项与 `budget_observation` 同义 |
| `requests` | 本执行全部持久请求条数 |
| `targets` | 大纲响应位置总数 `total`、知识应答位置 `knowledge_answers`、带分析规则引用的位置 `with_rule_refs` |
| `outline_issues` | 大纲 issues 按 code 计数，含预算拒收、待核和未安排的分析规则（`outline_rule_unassigned`） |
| `document_gaps` | 仅正文执行：缺口按 code 计数 |
| `pending_queue` | 仅正文执行：待核人工队列汇总，含 `total`、`pending`、`handled`、`by_status`（`pending`/`resolved`/`need_material`/`accepted_gap`）及 `handling_duration_ms`（`observations`、`avg`、`max`、`p95`，无已处理样本时统计值为 `null`） |
| `failures` | 失败聚合表，每行一个「阶段 × 类别 × code」：`stage`、`task`、`kind`、`code`、`count`、`reported_tokens`（供应商回报的 total_tokens 之和）、`unknown_usage`（无回报用量的条数）。`kind=send` 来自状态为 failed 或 uncertain 的请求行，code 取 `error.type`；`kind=contract` 来自 `model-rejection` 回执，每条回执计一次，code 取首条错误的 code，Token 取产生该回包的请求行（按回执里的请求重建实际发送的请求摘要匹配），匹配不到记入 `unknown_usage`。contract 行的 count 之和等于回执条数。按 reported_tokens、count 降序排列，回答「哪类失败最多」 |

#### 编制待核人工队列（`pending_queue`）

- 枚举 `AuthoringGapStatus`（`services/docreader/tender/status.py`）：`pending`（未处理）、`resolved`（已处理）、`need_material`（补资料）、`accepted_gap`（接受空缺）。
- 每条缺口由宿主按 `(chapter_id, target_id, code, detail, occurrence_index)` 派生稳定 24 位十六进制 `gap_id`（不回写编制产物，不改变执行身份），在工作台投影 `pending_queue` 中按大纲响应位置（章节顺序、目标顺序、原顺序）稳定排序，每项含 `gap_id`、`code`、`detail`、`chapter_id`、`chapter_title`、`target_id`、`target_purpose`、`bookmark`、`status`、`note`、`created_at`、`handled_at`、`handling_duration_ms`。
- 处置接口 `PUT /api/v3/tender-projects/{project}/executions/{execution}/pending/{gap_id}` 接收 `{ status, note? }`，持久化至 `tender_authoring_gap_dispositions` 并返回 `{ item, summary }`；重置为 `pending` 时清空 `first_handled_at` 与 `handled_at`。处理时长按编制执行入队列基准时间至 `handled_at` 的毫秒数计算。

`acceptance-eval` 是最低验收线离线评测脚本（`python -m docreader.tender.acceptance_eval`，见[验收与语义评测](acceptance.md#离线最低验收线评测脚本)）的只读打分输出：

| 字段 | 定义 |
| --- | --- |
| `schema_version`、`passed` | 固定 `1`；`passed` 仅在四项最低验收线全部为 `pass` 时为 `true` |
| `summary` | `score_passed` / `score_total`（`4`）、`machine_passed`（机器检查通过的项数）以及各状态计数（`pass`、`fail`、`needs_human_check`、`needs_onlyoffice_check`、`unknown`） |
| `criteria.requirements_coverage` | 第 1 项（要求不漏）：以分析事项为分母统计已落位、已结案（约束/排除）、显式待补和静默漏项，`missing_items` 列出每条漏项的 `rule_ref`、`item_id`、位置与原因；缺底稿时为 `unknown`，缺人工抽查回执且机器无漏项时为 `needs_human_check` |
| `criteria.original_tables` | 第 2 项（原式表正确）：核对大纲原式表在 Word 中的表格结构（列数、行数、表头、合并单元格）与固定文字/签章行保留，输出 `mismatches` 精确到 `chapter_id`、`chapter_title`、`target_id`、`table_index`、`kind`、`expected` 与 `actual`；无原件网格且无模板错误时保留 `unknown` |
| `criteria.no_fabrication` | 第 3 项（缺口不被编造）：核对未剔除的 `unsupported_content`、人工位自动填值（`manual_target_has_generated_content`）、无资料无来源正文写入（`content_without_material_or_source`）、未复核正文（`generated_content_unreviewed`）与导出编造/越界缺口，`violations` 列出具体违规项；已被 `_adopt_reviewed` 剔除的历史 `unsupported_content` 计入 `counts.removed_by_review`；覆盖结算中仅表示响应不足/待补资料的 `assessment == "unsupported"`（`coverage_unsupported`）单独记入 `counts.unmet_by_review` 与 `unmet_items`，不误判为编造违规 |
| `criteria.file_usability` | 第 4 项（文件可用）：校验 DOCX 压缩包与 XML 完整性、大纲章节/目标书签覆盖，并结合 `版式核对.json` 或 `--onlyoffice-receipt` 核对同摘要 `docx_sha256` 的 ONLYOFFICE 保存与 PDF 导出；缺实测回执时为 `needs_onlyoffice_check` |
| `execution_report`、`limits` | 透传的 `authoring-report` 摘要（含 `failures`、`outline_issues`、`document_gaps`）及离线评测局限说明 |

工作台任务进度（`progress`）的 `accepted_by_phase` 每行为 `phase`、`count`（已接受工作单元）和 `planned`（宿主登记的计划单元数，`null` 表示未知）。`planned` 只来自已登记的计划：编制取各 task 的 `execution-plan` 回执 `planned_requests` 之和，分析的 `extract` 取当前修订提取范围规划的包数，其余阶段为 `null`。有计划但尚未接受单元的阶段也列出（`count` 为 0）。整体总量不给出：后续阶段的计划在前序阶段完成后才登记。

### 运行状态

目标 RunStatus 为 `pending/running/blocked/waiting-confirmation/analysis-ready/failed/cancelled`。
blocked 表示存在硬阻断（分析本身不完整）；failed 表示执行/协议失败；waiting-confirmation 表示
独立核验完成、无硬阻断，剩余语义疑点待人工在确认时逐条处理；analysis-ready 表示当前版本已核验、
语义疑点均有人工处理结论且确认有效。
不增加按业务类别的状态；模型请求 succeeded、子操作有回执或命令 completed 均不证明分析完成。

### 问题分级与人工处理

宿主产生的每个问题code在`tender/schema.py`的`HOST_ISSUE_EXITS`登记出口，单元测试保证新流程
源码中出现的宿主code均已登记。模型finding的code不登记，阻断时一律为语义疑点。

| 出口 | 含义 | 例子 |
| --- | --- | --- |
| `revise` | 补充或替换来源，新建source_version | 缺件、不可读、章节/物理页来源不可用 |
| `review-scope` | 显式作用域审议 | scope_context_pending、condition_scope_unresolved |
| `correct` | 显式纠错修订 | 严格校正处置覆盖不完整、正文范围重复、整包抽取失败 |
| `host-fix` | 宿主组包或执行缺陷，修复后重跑 | 各类预算超限、核验覆盖不完整 |
| `human` | 语义疑点，确认时人工处理 | 事项/关联/问题/局部处置声明待核对、决议未被接受 |

前四类为硬阻断：当前修订存在阻断的此类记录，或必要覆盖/工作单元不完整时，运行为blocked，
人工不能驳回。verification产物在原`blocking`（存在任何阻断问题）之外增加`hard_blocking: bool`
和`human_issue_ids: string[]`（仍阻断、需人工处理的问题身份，升序）。

抽取CAS产物另含宿主生成的`host_issue_ids: string[]`（升序），记录真实来源及宿主检查所产生的
问题身份；模型输出、模型输入及人工校正合同不增加此字段。核验按宿主可信问题集合
（抽取`host_issue_ids`及核验自身检查）应用宿主code的硬阻断出口，模型自报同名code仍是
语义疑点。来源阶段问题继续全部硬阻断。
旧产物缺此字段时保守保留原分级；同协议可正式校正重建，旧协议运行只读并新建运行，
不改写旧产物及回执。

模型回包局部恢复生成的`extraction_disposition_unresolved`为human类阻断问题，覆盖处置缺失、
重复、格式错误及与有效证据矛盾；有效事项及完整原文保留，无事项证据的范围保持unresolved。
完整review覆盖不会自动消除疑点，须有独立接受的明确决议或人工处理；严格校正仍要求完整处置。
宿主最终发现的`analysis_review_incomplete`为hard类，不能进入human_issue_ids。

确认时提交`IssueResolutions`：`{schema_version: 1, resolutions: [{issue_id, verdict, reason}]}`。
verdict为`dismissed`（误报）或`acknowledged`（属实，作为已知问题保留展示）；reason为非空string；
issue_id集合必须恰好等于`human_issue_ids`，没有待处理问题时省略；CLI为`confirm --resolutions`。
进入waiting-confirmation时，命令回执metadata记录`human_issue_ids`；Store在写入确认前据此校验，
不完整的处理不会落库，避免幂等确认记录阻挡后续正确提交。通过后按issue_id排序的处理结论写入
确认记录的`issue_resolutions`及resume命令，confirm节点按verification产物重新核对并写入确认回执。
处理不修改事项、不删除Issue，需改事项仍走`correct`。同一处理文件可在新修订复用，但只对身份
不变的问题有效。


### 短引用与拒收纠正协议

主节点、领域 `schema_version=2` 及持久事项身份保持稳定；执行快照和预算 builder 显式标识发送协议。

- 请求内引用采用按类型编号的短ID。宿主以`model-wire`产物保存原始payload及双向映射；正文、引文、理由等
  自由文本不做字符串替换。所有结构化引用使用同一映射，未知引用不能猜测恢复。
  单个`request_ref`绑定原始payload，避免不同工作包压缩成相同短引用后误用付费缓存。
- source_views中的父来源定位与字符偏移保留在宿主，模型只见可选择的范围身份。
  alias仍保留实际发送片段及区间，表格单元格、标题、文档角色、作用域语义不裁剪。
- 模型review输出为`{schema_version:2,accepted_decision_ids:[],findings:[]}`。
  `reviewed_*_ids`只在宿主接受回执中由本次实际输入派生，不再作为模型输出字段。
  accepted_decision_ids仍须属于本次输入决议；覆盖记录证明任务执行，不证明语义完整。
- `model-rejection`产物绑定原始任务输入、具体调用输入及收到的响应，保存errors的
  `path/code/detail`。正文不进入普通异常日志。传输succeeded及原回包审计保持不变。
- 下次执行从拒收记录构造`contract_feedback`，包含真实错误及相关回包片段；原任务所需
  输入完整保留。纠正请求按实际完整消息预算准入并使用真实内容摘要，不加随机盐。
  模型恢复入口统一执行工作包的纠正与重发额度（见[失败执行的显式恢复](#失败执行的显式恢复)），
  不叠加SDK或队列模型重试。反馈记录真实的rejection_count；相同合同错误再次出现也
  使用剩余纠正额度，不能反复读取同一成功坏缓存。完整DONE/stop后的JSON语法错误也进入反馈，错误位置及附近脱敏文本
  合计不超过512字符；原回包保持不变，不以截前缀或删除符号作为接受依据。
- 响应纠正只处理尚未接受的工作包合同，不修改已接受事项或自动解除业务问题。
  uncertain允许在同一额度内重发，原回执及未知用量保留，可能重复计费；仍在途的sent
  阻止并发发送，租约接管先按既有规则结算为uncertain。不增加状态、环境变量或计数表。

### 失败执行的显式恢复

核验分包、完整请求预算测量等同步规划在工作线程执行，事件循环持续处理租约心跳；
线程调度不改变请求内容、摘要或回执身份，也不扩大上下文预算。

模型工作单元的失败分两类分别计数，由恢复入口统一调度（BUG-260）：

| 类别 | 包括 | 处理 | 每个逻辑请求上限 |
| --- | --- | --- | --- |
| 合同纠正 | JSON 语法错误、宿主校验拒收、缓存不可验证 | 写拒收回执，带 `contract_feedback` 重发 | 纠正 2 次（含初发共 3 份不同完整请求） |
| 传输与其他调用失败 | 连接、429/5xx、服务拒绝、配置、响应结果不确定、校验代码异常 | 退避后原样重发 | 4 次，跨纠正累计 |

单个逻辑请求的物理发送上界为 3 + 4 = 7 次。现有请求台账以`command_id`和`logical_request_digest`
持久计次。逻辑摘要移除顶层contract_feedback，保留其余完整请求身份；完整request_digest仍用于
缓存和付费围栏。Store在转sent的同一事务按完整request_digest核对已发送意图：不同完整摘要
不超过3个，同一完整摘要的重发合计不超过4次；缓存复用不计次，重启不重置。发送前错误也
受同一上限约束。模型工作单元耗尽明确 failed 并记录 `metadata.reason = "model_work_unit_retries_exhausted"`，队列不再叠加模型重试；成功子产物不重做。在模型阶段遇到执行配置冲突（`execution configuration changed`、`request budget changed`、`execution protocol changed` 或 `artifact inputs changed`）时立即以 `failed` 结束并记录 `metadata.reason = "execution_config_changed"`，不消耗 4 次基础设施重试额度；从 `confirm` 中断点恢复时仅校验 `workflow_protocol` 兼容性，不因提示词、解析器或请求预算变化阻断已完成的分析确认。发布、数据库、普通代码异常和执行器丢失保留同命令四次外层兜底，耗尽时记录 `metadata.reason = "infrastructure_retries_exhausted"`；历史 SQL 字段 `infrastructure_failures` 仅用于该执行计数，不新增状态、配置项或计数表。过期执行由接管方结算一次失租失败；重复投递不重复计数。
旧Worker取消/版本或租约失效后停止执行；并发兄弟中取消或失租优先传播，其次是单元
额度耗尽，不能被另一个可重试错误掩盖。业务blocked不是执行异常，继续走独立核验与现有裁决入口。

`retry` 只恢复当前来源/领域修订的执行失败，不做业务纠错，不增加节点或状态。操作绑定
`run_id/source_version/domain_revision/failed_command_id/request_key/reason`；首次申请须指向
当前修订最新的失败命令，运行已 failed、无活动租约且未取消。同 request_key 和相同参数
返回同一恢复命令，不因重复提交增加模型调用；参数不同或基线过期拒绝。

恢复新建同修订命令，继承原命令的 start/resume 和业务参数，在 payload.retry 中记录
`failed_command_id/request_key/reason`。该审计元数据不进入确认业务内容，也不发给模型。
原失败命令、回包及用量不改写；沿用同一执行快照和 checkpoint，复用成功产物及成功模型
响应，失败及uncertain的新版逻辑请求可重新付费。正式retry在无活动租约的事务中把
新版遗留sent结算为uncertain，不改原回包或用量；旧无逻辑摘要的冻结请求保留原围栏，
不以换版本、改摘要或清状态绕过额度。语义未决的 blocked 继续走现有证据裁决或显式纠错，
恢复命令不代表独立核验或人工确认完成；新command给予失败工作单元一份新额度（纠正2次、
传输重发4次）。所有反馈摘要计入该command逻辑单元额度，
不能通过修改反馈摘要绕过；未知付费尝试仍完整计入请求台账。

### 最小事项与复合条款示例

下例不需要 subject、numbers 或任意业务细节对象即可表达一个普通要求：

```json
{"item_id":"p1:i1","kind":"obligation","text":"保修期为三年。","evidence":[{"range_id":"r1","quote":"保修期为三年"}]}
```

复合来源 r2 为“负责人须持A证；持B证加2分；未附A证复印件则否决投标。”，同段四类事项共享来源。
下例为宿主规范化后的结果，含宿主派生的relation_id；抽取模型输出不包含该字段：

```json
{
  "schema_version": 2,
  "items": [
    {"item_id":"p2:i1","kind":"obligation","text":"负责人须持A证。","evidence":[{"range_id":"r2","quote":"负责人须持A证"}]},
    {"item_id":"p2:i2","kind":"scoring_rule","text":"负责人持B证加2分。","evidence":[{"range_id":"r2","quote":"负责人须持A证；持B证加2分"}],"scoring":{"points":2}},
    {"item_id":"p2:i3","kind":"submission_requirement","text":"须附负责人A证复印件。","evidence":[{"range_id":"r2","quote":"负责人须持A证；持B证加2分；未附A证复印件则否决投标"}],"submission":{"documents":["A证复印件"]}},
    {"item_id":"p2:i4","kind":"consequence_rule","text":"未附负责人A证复印件则否决投标。","evidence":[{"range_id":"r2","quote":"负责人须持A证；持B证加2分；未附A证复印件则否决投标"}],"consequence":{"effect":"否决投标","trigger":"未附负责人A证复印件"}}
  ],
  "dispositions": [{"range_id":"r2","status":"items","reason":"该段包含负责人资格、加分及证明文件的否决条件。","item_ids":["p2:i1","p2:i2","p2:i3","p2:i4"]}],
  "relations": [
    {"relation_id":"p2:l1","from_item_id":"p2:i3","to_item_id":"p2:i1","relation":"A证复印件是负责人A证资格的证明文件。","evidence":[{"range_id":"r2","quote":"负责人须持A证；持B证加2分；未附A证复印件则否决投标"}]},
    {"relation_id":"p2:l2","from_item_id":"p2:i4","to_item_id":"p2:i3","relation":"未附所要求的A证复印件触发否决。","evidence":[{"range_id":"r2","quote":"未附A证复印件则否决投标"}]}
  ],
  "issues": []
}
```

该例不产生企业姓名、持证情况、响应动作或是否争取加分的决定。跨包“详见第五章”若未读取
目标正文，须在 issues 中保留引用和原因；不能把缺 scoring 字段解释为没有评分规则。

### 决议与独立接受示例

下例假定当前输入实际提供原事项 p1:i1“第一、二标段均在30天内交付”、补遗事项 p2:i1
“第二标段改为20天内交付”，以及待解决问题 q1；原文范围分别为 r1、r2：

```json
{
  "decision_id": "a1:d1",
  "issue_ids": ["q1"],
  "reason": "补遗仅修改第二标段期限，第一标段原期限保留。",
  "evidence": [{"range_id":"r1","quote":"第一、二标段均在30天内交付"},{"range_id":"r2","quote":"仅第二标段改20天，其他不变"}],
  "amendment": {
    "target_item_id": "p1:i1",
    "effect": "replace",
    "affected_scope": "第二标段交付期限",
    "retained_rules": [{"text":"第一标段在30天内交付。","scope":"第一标段","evidence":[{"range_id":"r1","quote":"第一、二标段均在30天内交付"},{"range_id":"r2","quote":"仅第二标段改20天，其他不变"}]}],
    "replacement_item_ids": ["p2:i1"]
  }
}
```

此对象只是 associate 回包中的待验 Decision。后续独立 review 须实际装载两份原文和两项事项，
在 accepted_decision_ids 中明确接受本次实际提供的 a1:d1；只返回
findings=[] 不能解决 q1。原文/版本变化或任一必要核验未接受时，旧提案不授权当前结果。

## 编制依据与交付约束（T3至T5业务语义）

本节说明业务语义，正式结构统一见下节Python编制执行合同；职责与预算见
[编制架构](workflow.md)。字段先在本字典定义，再同步schema、提示和执行链；
skill只引用合同，不另建同义结构。真实服务与文档验收状态只在交接记录。

统一原则：所有业务缺口均是可交付内容的一部分，不是生成/导出的拒绝条件。原文必填
而未知的值可为空，必要说明在不破坏原格式的位置或缺口清单中展示；缺价格不填0，
缺材料不伪造引用，缺复核不声称已经通过。留空不需要额外审批。

| 概念 | 最小含义与保存边界 |
| --- | --- |
| 编制输入快照 | AuthoringInput固定实际可读来源、已有事项/核验/人工意见及真实缺失状态；确认和全部根产物不再是生成前提。无效引用不作为有效资料，旧结果保留实际版本，不假装已确认或最新 |
| 编制依据 | 与当前AuthoringInput关联的功能应答方案、可用资料与共同安排；产品信息/采用记录可选。宿主管理版本和引用，不另建招标事实台账或产品确认门禁 |
| 项目背景 | 来源中的现状、规模、现有系统及目标等说明性事实，可以是non_item；按引用装载，不为进入生成流程强造事项，也不从历史方案补造本项目现状 |
| 整体方案 | 编制依据内跨章共用且有适用资料支撑的功能实现、技术路线、部署/集成、实施阶段、角色、验收和服务安排；缺具体配置不阻止有依据部分生成，无依据部分留空，不新增独立状态机 |
| 产品/方案事实 | 功能能力、实现方式、服务范围及其依据；品牌、型号、版本和配置均为可选补充，缺省不阻止写作。原文明示某处必填时保留局部待补项，拟议实现不冒充已有产品事实 |
| 采用决定 | 可选的已有产品/方案选择、适用范围及授权依据；不作为编制依据或正文生成的必填条件，也不新增确认步骤，候选建议不自动构成业务授权 |
| 履约承诺 | 有依据且获授权的交期、人员、定制、质保、费用等；拟议承诺保持未决，不能从产品介绍或招标要求直接推出 |
| 资料引用 | 可重放的企业/产品/证明原件及定位、适用型号/主体/版本；沿用ArtifactRef管理不可变内容，实际知识服务定位适配在T3定义，不混同招标SourceUnitRef |
| 资料用途 | 事实/证明依据与方法/写作参考分别判断；同一文档不同片段可能用途不同，历史方案不能直接证明本项目能力。usage只记录资料用途，是否适用仍由语义复核判断，命中不等于证明 |
| 具体缺口 | 未满足、证据不足或待判定的内容及其影响位置、处理出口；默认留空/待补继续生成与导出，不以空资料生成满足结论，不要求问题关闭才能下载 |
| 内容响应与编制/交付约束 | 是既有规则的使用方式，不新增ItemKind；前者进入响应位置，后者约束相应文件/表单/交付行为，如格式、签章、上传期限 |
| 写作任务 | 可独立生成和保存的完整业务单元，绑定响应位置、相关要求、编制依据、资料及模板；可以是完整小节、相关条款组或表格，不以页数或事项数量固定划分。正式字段见下节WritingTask/WritingResponse/WritingUnit |
| 交付核对视图 | 从既有规则、响应位置、采用证据及实际产物派生要求/评分、具体响应、证明、文件位置与剩余事项；不另建事实台账，也不以视图覆盖替代内容复核 |

outline_rule_unassigned以响应位置及constraint_rule_refs共同计算未分配规则。不能为消除
未分配提示强造正文章节，也不能把已引用但未落实的交付义务标成完成。具体字段覆盖
分册与填表，不增加通用任务状态机。

技术响应、方案、配置/偏离表和报价相关内容须引用同一编制依据；各章节不复制维护各自
一套产品事实。编制依据修订仅影响其依赖的编制内容及必要复核，招标原文未变不重抽分析。
正文、填充值、采用证据与实际导出合同见下节；purpose仅为写作要求，source_refs仅指
招标来源，不能冒充已生成内容或已匹配企业材料。

### 模型传输诊断

以下可选诊断同时写入`ModelRequest.error`及流式审计CAS顶层，不改变请求状态、
既有恢复额度、超时或用量结算。字段缺失表示旧记录或非传输错误，不能推测原因。

| 字段 | 类型与含义 |
| --- | --- |
| transport_error_types | `str[]`；外层至内层的异常类型名，最多四层，按对象防环。不记录异常自由文本、URL、认证头、请求正文或密钥 |
| transport_protocol_reason | 可选固定诊断标签；仅异常链中出现HTTPX `RemoteProtocolError`时记录。库标准消息精确匹配为`server_disconnected_without_response`，其他为`other_remote_protocol_error`；不保存异常原文，不由标签推断具体网关、超时参数或供应商内部根因。原有phase、状态、用量及重试额度不变 |
| transport_errno | 可选整数；异常链中首个真实整数errno，布尔值和字符串不接受 |
| transport_phase | `before_headers/reading_body`；按客户端是否取得可用HTTP响应对象判定。before_headers不能证明远端从未发送过头；零正文也不能证明未收头。取消、供应商错误及语义拒收不伪造该字段 |

## Python编制执行合同（T3至T5）

当前编制策略见 [编制交互策略（knowledge_required_v2）](#编制交互策略knowledge_required_v2)；实际运行与验收状态只在 [招投标入口 · 当前状态](README.md#当前状态交接) 维护。所有模型回包中的 ID 仅为所需短引用选择，宿主产生执行、章节、任务、资料及文件身份。

| 结构 | 字段与含义 |
| --- | --- |
| OutlineAssemblyTarget | `kind:narrative/form/attachment`、`purpose:Text`、`target_ids:Identity[]`且至少一项；选择本请求宿主提供的短候选位置引用，将语义重复的跨批响应合并为一个实际位置。模型给出合并后的用途和模式，不回抄原规则/来源/CAS或持久身份 |
| OutlineAssemblyChapter / OutlineAssemblyResponse | 节点含`title:Text`、`targets:OutlineAssemblyTarget[]=[]`、`is_volume:bool=false`、`children:OutlineAssemblyChapter[]=[]`；响应仅为`{chapters:OutlineAssemblyChapter[]}`。宿主对每个合并目标联合去重原rule_refs/source_refs，保留原模板依赖，并统一生成正式章节/位置ID；任一候选manual_fill=true则合并目标继承true，不能被模型改为自动填价 |
| OutlineTargetResponse.manual_fill / ResponseTarget.manual_fill | `StrictBool=false`；大纲模型按实际语义将最终报价、投标单价、总价、折扣等人工填写位置设为true，宿主原样绑定。保留格式和待填位置，同章其他目标继续；无原表的人工正文仅显示通用填写空位，不将内部purpose拼入正文。不新增金额结构、审批状态或下载门禁，不扩展为所有技术参数都人工填写 |
| KnowledgeMaterial | `material_id:Identity`、`title:Text`、`text:str`、`content_ref:ArtifactRef或null`、`original_ref:ArtifactRef或null`、`locator:dict`、`usage:fact/reference/proof`、`attachment:bool=false`；locator保留适用target及必要产品/版本/原文件定位；宿主按命中的 document_id 读取知识库文档记录（每份文档一次 `document` 操作，与检索共用恢复额度），能核实时写入 `source_sha256`（原件字节 sha256）和 `source_document_id`，不能核实时写 `source_identity:"unconfirmed"`，不阻断该命中。content_ref只指冻结文字片段，original_ref只指实际原件；片段不能冒充证明原件；搜索命中默认reference，不自动证明事实或资格 |
| AuthoringArrangement | `topic:Text`、`text:str`、`nature:provided/proposed`、`material_ids:Identity[]=[]`；任何nature都须关联适用资料，provided只描述资料所载，不代表系统已核实或用户确认。proposed兼容旧产物，不再作为无依据生成的兜底 |
| AuthoringBasisResponse | `project_background:str`、`approach:str`、`shared_arrangements:AuthoringArrangement[]`、`gaps:str[]`；前两字段兼容保留，本策略置空，不作为无引用依据进入写作。共同安排仅汇总适用资料及其关联要求支撑的内容，不因全源原文另起技术路线 |
| AuthoringBasis | `schema_version:2`、`authoring_input:AuthoringInput`、`response:AuthoringBasisResponse`、`materials:KnowledgeMaterial[]`；同版编制共用一份依据 |
| AuthoringBlock | `kind:paragraph/table`，`text:str=''`、`headers:str[]=[]`、`rows:str[][]=[]`、`target_ids:Identity[]=[]`、`material_ids:Identity[]=[]`；本块应答绑定适用目标及实际发送的非空知识文字；只有原件、没有文字的资料不能支撑新增正文。text可作表格前说明，paragraph同时带表格时按原顺序保留，记录block_kind_mismatch；无依据或非法引用的生成内容不采用，原位置留空 |
| CellFillResponse | `source_id:Identity`、`row:NonnegativeInt`、`column:NonnegativeInt`、`text:str`、`target_ids:Identity[]=[]`、`material_ids:Identity[]=[]`；source_id为本任务原生模板短引用，坐标零起始；填值须关联form目标、该目标真实source_refs内原表及实际发送的非空支撑资料文字，不能借用同任务其他位置的模板 |
| CellFill | `source_ref:SourceUnitRef`、`row`、`column`、`text`、`target_ids:Identity[]=[]`、`material_ids:Identity[]=[]`；宿主绑定原表及正式目标身份，固定非空格不被覆盖 |
| AuthoringAttachment | `material_id`、`title`、`artifact_ref:ArtifactRef`；只能绑定实际可读原件，不能由模型制造CAS引用 |
| WritingResponse | `blocks:AuthoringBlock[]`、`cell_fills:CellFillResponse[]`、`material_ids:Identity[]`、`gaps:str[]`；同次调用完成语义匹配及有依据改写，各block/fill分别关联目标和资料；顶层material_ids仅用于附件选择，不能替代内容依据。不回抄任务/章节身份 |
| WritingTask | `task_id`、`chapter_id`、`title_path:str[]`、`targets:ResponseTarget[]`；同章连续narrative目标可合成一个任务；每个form、attachment或manual_fill目标各自单任务。manual_fill任务只保留格式占位；纯attachment缺可读原件、普通目标缺候选知识时均跳过写作，保留空位与gaps。部分匹配只写有依据部分，同章其他任务继续。单个表格实例填值只属于该target，不能因模板相同跨target合并。过大叙述任务按已有目标边界拆分，不按页数或每条要求拆分 |
| WritingUnit | `schema_version:2`、`task_id`、`chapter_id`、`target_ids:Identity[]`、`status:generated/placeholder`、`blocks`、`cell_fills:CellFill[]`、`attachments:AuthoringAttachment[]`、`gaps:str[]`、`error_type:str或null`；失败占位不冒充成功正文 |
| AuthoringReviewFinding / AuthoringReviewResponse | `findings:[{code:Text,detail:Text,task_ids:Identity[]=[],content_ids:Identity[]=[],suggestion:str='',can_revise:bool=false,reject_content:bool=false}]`；content_ids选宿主提供的具体block/fill短引用。reject_content由模型判断当前已生成内容确实失真、与依据矛盾、增删条件改变原义，或虽有来源但用途错误（如把编制说明作为实际应答）时置true；原格式固定文字与必要项目背景按实际用途保留，纯遗漏、待补及复核不可用保持false。宿主对true或旧unsupported_content按任务拆分并执行原定位剔除，不按其他自由code猜语义。无精确内容但关联任务明确时，保守清空该任务生成应答并保留原稿；固定原表和要求行不删。can_revise独立表示现有资料是否足以作一次修订，缺证书、未知价格或授权保持false。自动大纲的结构F0只要有findings就交A1复核一次，不以can_revise拦截结构意见；A1依据原文可修改或保持，树改变才F1，人工固定大纲不自动修订。结构/保存Word只读复核不执行正文剔除，reject_content保持false。缺项/冲突不设置下载门禁 |
| AuthoringStructureResponse | 结构F专用，继承`findings`，增加`source_decoration_exclusions:[{target_id:Identity,support_ids:Identity[],reason:Text}]=[]`。仅排除确定为独立源页首、页尾或页码装饰的部件，只选当次目标实际`template_support_ids`。含实质正文、条件、备注、表内标题、填写说明或签署内容的部件及混合部件不得排除；本项目不适用不能作为删除固定原格式条款的依据。不确定时保留内容并记finding，表单归属仍交既有A1。仅上下文/要求行引用不可排除。宿主按精确部件身份在指定实例内执行，不按code或文字推断，不删除原要求、原CAS、相邻部件或其他实例。仅自动组装F0/F1设置`source_decoration_edits_allowed=true`后执行；固定人工大纲及只读复核只保留建议，不改纲。全部选择合法后才统一应用；命中成功回执也执行相同规则，旧`template_exclusions`不属于新合同；非法选择沿原有限反馈出口，不加轮次或门禁 |
| StructureChapterMove / AuthoringStructureResponse.chapter_moves | 可选`chapter_moves:[{chapter_id:Identity,parent_chapter_id:Identity\|None,after_chapter_id:Identity\|None,reason:Text}]=[]`；选择本次chapters实际章节，parent为null移到根，after为null置于该父章子项首位，否则放在所选同级章之后。只改完整子树归属与顺序，保留标题、身份、targets、模板、响应行及所有引用，不重写原文。仅自动组织最后F1的输入`hierarchy_edits_allowed=true`时执行；A1未改变树时沿既有边界不增F1，保留原问题待核。F0、固定/人工大纲及已保存Word只保留建议。宿主验证引用唯一、最终父子无环及相对位置一致，非法/冲突整批不应用并保留待核；不按标题或自由finding文本搬章。原findings保留供核对，执行记录不等于独立语义验收通过，不新增模型轮次 |
| 结构F compact文本共享 | 可选`text_pool:str[]`及support/submission_source的`text_lines:(str或非负整数)[]`。整数仅为本次共享池从0起的下标，按顺序换行连接逐字恢复原text；不合并support/source身份、条件、所属目标或原格。只在有净收益的发送副本采用，不影响原CAS或输出合同，不回抄共享池 |
| 结构修订反馈 previous_structure.targets | `review_target_id`为F中的v短引用，`candidate_target_ids`为同请求原候选t引用。宿主为模型已选及兜底保留的全部位置登记映射；保留遗漏候选或A0整体失败时，不能把已知候选引用置空。按相同候选集合/主体继承前版选择时保留F0后的实际模板，不恢复被排除装饰。同一候选多实例继续分别登记、参见不明确时保留歧义，不让模型推测持久身份 |
| AuthoringGap | `code:Text`、`detail:Text`、`task_id/chapter_id/target_id:Identity或null`；宿主统一缺口位置，模型局部字符串缺口在边界补位置 |
| AuthoringUsage | `sent_requests`、`retry_requests`、`estimated_input_tokens`、`prompt_tokens`、`completion_tokens`、`total_tokens`、`unknown_usage_requests`均非负整数；计量范围为本执行的实际发送；继承回执不伪造新发送或用量。Token总数只含已报告值，未知调用另计，不能视为免费；原执行费用按其回执单独追溯 |
| AuthoringDocument | `schema_version:2`、`authoring_input`、`outline_ref`、`basis_ref`、`unit_refs:ArtifactRef[]`、`review_ref`、`initial_review_ref:ArtifactRef或null=null`、`superseded_unit_refs:ArtifactRef[]=[]`、`docx_ref`、`package_ref`、`gaps:AuthoringGap[]`、`usage:AuthoringUsage`；unit_refs指实际采用内容，初次复核和被替换的原稿分别保留，review_ref指最后复核结果。全部实际落盘后返回，费用未知保持未知 |

归并输入采用单次`rules`共享表：按EffectiveItemRef完整身份分配短rule_id，保留该规则的
完整text及已有scope/conditions/exceptions/subject/kind/scoring，不按相同文字合并不同范围。
候选target仅携rule_ids及短source_ids；source_ids标识原生模板身份，用于区分同模板的
复用与不同来源。模型选择target_ids，宿主仍以原引用联合规则/模板，绝不根据文本相似
自行合并。source组织使用原outline.md；归并使用独立outline_assemble.md，改变归并协议
不改变已完成来源请求摘要。当前样例归并可在显式校准预算内一次完成，不增加层层调用。

T2追加`OutlineResponse.constraint_rule_ids:Identity[]=[]`及
`AuthoringOutline.constraint_rule_refs:EffectiveItemRef[]=[]`，只用于文件/交付约束；它们
不强造正文章节，也不表示已签章或已提交。章节可选`is_volume:bool=false`，仅在原文
明确分册时设置；导出主DOCX保留全章节顺序，并对明确分册输出独立文件。普通顶级
章节不自动当分册。未覆盖规则仍保留具体问题。

归并输入按原章节列出候选target_id短引用、kind、purpose、manual_fill及必要rule_texts，
供模型判断哪些是同一响应任务。它不是要求模型誊写宿主清单；未选候选仍由宿主保留为
待核对位置，不能被静默删除。不同标段、适用条件、原格式实例或确有要求的正文/证明
重复交付须分别保留；报价人工位置不应吞并仍需自动编写的技术/商务说明。

投标人的最终报价、单价、总价、折扣等人工把控值保持空白，不用0、知识库/历史项目价格
或自动计算值替代；已有价格资料也不自动变成本次最终报价。招标预算、最高限价、评分
数值及其他原文要求仍原样保留，并供人工填写时参考。manual_fill只作用于对应响应位置，
不暂停整份生成，也不把其他有依据的技术参数、实施方法及服务内容强制转为人工填写。

`source_id`只在一次写作请求内有效，由宿主映射到SourceUnitRef；行列是该原生grid的零起始
锚点坐标，不是Word导出后的编号。同一target内多任务填同一格且内容冲突时保留原空白和
具体问题；不同target即使引用同一SourceUnitRef，也输出各自的表格实例。固定非空文字
不因模型回包被覆盖；来源不属于本任务模板的填值不写入其他位置，保留定位缺口。

资料准备时宿主将实际发送的`text`冻结到`content_ref`，不能将该片段引用放入`original_ref`。
`attachment=true`同时要求实际可读原件；缺原件时只保留文字参考和缺口。当前知识适配的
搜索片段默认`usage=reference`、`attachment=false`，不把检索命中升格为证明或授权。
资料明确记载的功能及方法可在产品、版本和主体适用范围内据实应答或改写，不能只因
reference标签忽略有效功能资料；事实/证明/授权声明仍须对应资料用途，命中不自动
升格为proof。无有效资料关联的共同安排不保留，不再降为`proposed`继续生成。
来源身份与范围由宿主维护，是否支持本项目响应由模型判断，不能用关键词或正则代替。

### 调用、预算、恢复与修订

1. 分析核验只发 review，见业务工作流。编制按实际目录、依据、写作和复核任务发请求，次数随任务和装不下的包增长，不再用 `L=S+A0+F0+…` 当上限。每个请求的恢复额度见[失败执行的显式恢复](#失败执行的显式恢复)。
   不做任务两两组合、不固定逐要求调用。整棵归并树放得下时A0为1。放不下时按完整章节分组各发一次，再发一次去掉已持有正文、但仍包含全部候选身份的归并；该次放不下才由宿主拼接，不再重发整棵原文树。
   单章仍放不下时沿既有超预算发送出口，不新增阻断。任一分组失败则保留来源局部章节与待核顺序，不假称已具备递归归并。共同依据按完整记录
   顺序分批并携带受限previous摘要，不反复回传全量原文。知识检索另计实际次数。
   W不计manual_fill、无候选资料及缺原件的纯附件任务；其格式占位新增调用为0，不参与
   无资料改稿。语义匹配包含在W内，不增加独立匹配轮次。知识查询Q次最多按既有首次
   加三次产生4Q次HTTP尝试；无命中/不适用是有效空白结果，不重试至命中。
2. 请求按实际消息测量，输入不超过现有准入限额及总窗口扣输出/安全预留的较小值；
   编制明确配置正输出预留，输出预留不是供应商硬上限。共同依据输出限12000 UTF-8字节；
   写作提示通常控制6000汉字，不能将提示长度当成已验证输出容量。正文输出按逻辑单元控制，
   原表不可拆时容量不足保留待补并记录，不能静默裁原文。记录累计估算输入、实际usage、
   未知usage调用与重试次数，不将单次准入冒充累计预算。
3. 缺知识/价格/型号/模板/确认有空白及位置缺口出口；模型恢复耗尽只使该任务占位。
   复核失败记未核对，该批生成应答暂不采用，保留原稿与回执，模板/空位及其他任务
   继续导出。基础设施/取消/写围栏保留真实失败，不能伪报导出成功。
4. 同执行恢复复用已完成的内容摘要回执，不覆盖旧结果和手工文件。初次复核后，只对
   status=generated且有can_revise=true发现的明确任务发起一次改稿；加入当前稿和真实
   复核意见，仍沿用既有execute的恢复额度，不另套重试、不补检索或无限续写。
   空结果或改稿失败可保留未被否定的旧成功正文；明确无依据内容先移除，不能回退采用。
   成功替换时旧稿进入superseded_unit_refs。
   改过的任务再核一次，保留initial_review_ref；复核仍有问题继续导出，不开启下一轮自动改稿。
   改稿输入沿用该任务原写作payload，追加previous_content中的blocks、cell_fills、attachments
   和gaps，以及review_findings中的code/detail/suggestion/can_revise/reject_content。旧填值用本次宿主
   sources映射回source_id，附件只发material_id/title；不发送CAS、任务身份或按出现顺序猜绑定。
   无法绑定原稿所用资料时保留原稿和具体问题，不让改稿静默移除原内容。
   新执行按依赖选择受影响任务及跨执行复用属于后续修订能力，不能由previous_document_ref
   参数存在推定已实现；原文未变不重抽，无法确定依赖时应明确重跑范围。
5. 模型不生成CAS、execution/task/chapter ID，不回抄输入/覆盖清单或计算最终页码。
   只选择来源/资料/所需响应位置的短引用；宿主负责映射、表格坐标、真实附件、文件装配。
6. 执行身份只含本执行会调用的任务（BUG-259）。分析执行的规格和预算只含 `extract`、`verify`、`ocr`
   的 Prompt 及这三个任务的输出契约。`single_requirement_chain_v1` 大纲执行只登记
   `bid_format_outline`、`item_placement` 和 `proposal_outline`，分别绑定 `BidFormatOutline`、`ItemPlacementResponse` 与 `ProposalOutlineResponse`/`ProposalTreeOnlyResponse`。
   非冻结大纲执行登记 `outline`、`outline_assemble`；未固定大纲的整稿执行登记 `outline`、`outline_assemble` 及四个正文任务；固定 `outline_ref` 的正文执行只含
   `authoring_basis`、`authoring_write`、`authoring_review`、`authoring_structure`。输出契约按同一任务集合计入。
   执行调用了身份外的 Prompt 时宿主报配置错误，不从文件补读。改某任务的 Prompt 或契约，只改变
   包含该任务的执行身份；身份变了的在途执行仍按原规则拒绝续跑，需新建执行。
7. 运行级上限（BUG-259）只作用于大纲执行和正文执行，分析执行与保存后复核不设。执行规格写入
   `run_limit_policy=planned_task_budget_v1` 与倍数 `run_limit_multiplier=2`，计入执行身份。
   - **计划登记**：每个阶段在发出本阶段请求之前，按任务登记本阶段计划的逻辑请求数，宿主存为
     `execution-plan` 回执；同一阶段同一范围重放时登记值必须一致。
   - **调用上限**：Store 在准入新逻辑请求的同一事务里核对，某任务已准入的不同逻辑请求（含已发送
     和已准备未发送）不超过该任务累计计划数的 2 倍。没有登记计划的任务计划数为 0，首次发送即拒。
   - **Token 上限**：每个逻辑请求的计划量为首发输入估算加输出预留。本执行全部已发送请求的用量
     （供应商未回报时，失败的发送取该次输入估算，结果不确定的发送取输入估算加输出预留）加上本次
     发送的输入估算与输出预留，不超过已准入逻辑请求计划量之和的 2 倍；它约束重试、改稿和超长输出
     的累计开销。
   - **触顶出口**：缓存复用不计入。触顶的请求以 `ExecutionBudgetExhausted` 拒发，不发 HTTP，该任务
     按既有本地失败留占位或问题（写作单元缺口写明已达运行上限）；整份结果追加一条
     `execution_budget_exhausted`（大纲为 issue，正文为 gap），已完成内容照常导出。后续由人工补写，
     或新建执行重跑。

### 归并操作的显式Token计量

来源组织装包在实际输入准入内预留 2048 token 纠正空间，实际发送上限不变；未知引用错误精确到叶字段，不复制整棵章节树。全量归并仅在原发送视图超限时使用 `assembly_view="compact_v1"`，仍保持同一输出合同与宿主引用映射：

- `template_parts`、`source_excerpts`、`submission_sources`、`submission_rules`、`unassigned_rules` 由数组改为以原 `part_id`/`range_id`/`source_id`/`rule_id` 为键的对象，值保留该条除重复 ID 外的全部字段。
- 纯原生网格可用 `rows` 完整二维明文矩阵表达，行列仍从 0 开始；`null` 表示该坐标没有独立原生单元格，`""` 表示确实存在的空白格。`merged_cells` 每项为 `[row,column,row_span,col_span]`，只列非 1×1 合并；`row_count`/`column_count` 保留。原件列宽属于宿主排版数据，仍在冻结原件内，不参与模型语义判断。带额外单元格属性的网格保持完整原结构，不丢公式或其他未知字段。
- 候选省略 `is_volume=false`、`manual_fill=false`、`support_origins=["knowledge"]`、`template_part_ids=[]` 时只应用这些明确默认值，不表示缺失语义；其他字段不裁剪。
- 以上仅是输入投影，不改变持久结构、最终 DOCX 或输出短引用，不要求模型重抄压缩表；若投影后仍超限，保留既有容量缺口及人工编辑/重新编制出口。

compact 视图可在顶层携带 `text_pool: string[]`，仅共享完全相同的原文行。`template_parts`、`source_excerpts` 及 `submission_sources` 中可用 `text_lines: (string | integer)[]` 代替 `text`（字符串为原文行，整数为从 0 开始的 `text_pool` 索引，按顺序以原始 `\n` 连接即逐字恢复原文）。空行、首尾换行、Unicode、空白与标点均不规整，仅在完整 JSON 实际变小时采用；相同文字共享不合并来源身份、部件、适用条件或实例。仅当 `source_sections` 只有一个章节且所有 `source_excerpts` 确实引用该章节时，compact 摘录可省略共同 `section_id`。

`RequestBudget` 接受宿主构造参数 `tokenized_outline_assembly: bool=false`，开启时须同时显式配置 `tokenizer="o200k_base"`（不按模型名猜测，不在运行中修改），仅对 `measure_request/require_request(request, task="outline")` 且发送 payload 顶层 `operation="assemble"` 生效；其余操作沿用既有字节估算或 `verify` tokenizer 分支。开启时按实际消息 `o200k_base` token 计量，扣除 512 framing 余量、安全余量与 `min(32768, context_tokens/4)` 输出预留（131072 窗口对应输入上限 94208），并在 `spec.tokenized.operations` 写入 `{"outline":["assemble"]}` 固定执行快照。未校准或未显式开启时保持字节估算，配置无效在发送前报错。

实际知识服务接口是可注入的窄provider：按任务语义查询，返回冻结文字/原件及定位；
没有配置、无命中或语义不适用返回可读诊断，相关应答留空、固定模板保留，继续导出。
接口和合成回归不代表完成真实知识库
验收，原件下载和适用性核实也不能由搜索片段代替。导出DOCX按冻结文字/网格重建；PDF原件作为真实附件按页嵌入或
随包提供，不能把重建表格说成无损复刻PDF版式。

本轮复核按实际任务内容和共同依据分批；跨批依赖组全局检查、单个巨型
表格的分组填写、输入修订后的依赖选择、最终PDF视觉核对及目录实际页码/图表交叉编号
仍须分别实现和验收。保留这些完整交付目标，不把当前分批复核和DOCX产物冒充全部完成。
金额自动计算不在当前实现目标内；如未来明确需要，再单独讨论输入、人工采用和计算合同。

### 新链事项归属

流程和五问见[业务工作流](workflow.md#新链格式补位与事项归属)。

| 结构 | 字段与含义 |
| --- | --- |
| 大纲合同 | `POST /api/v3/tender-projects/{project}/outline/generate` 可选传 `requirement_chain_contract: "single_requirement_chain_v1"`，只用于有冻结来源的自动大纲；有冻结来源的自动大纲统一按 `single_requirement_chain_v1` 执行。同一请求键绑定同一选项；恢复使用已登记的执行合同。 |
| 方案上下文 | 在大纲保存 `chapters[].source_additions[]` 指定本章已有或选入的 `target_id` 及 `project_context_excerpts`，随响应位置保存。精确引用列表替换背景，空列表清除，省略沿用原选择。位置须为非手填、仅允许 `knowledge` 的叙述位置，且无原式表或原文正文绑定。宿主从固定快照读取非空、不重叠的范围，不接受客户端传入文字。现有原文正文的 `source_context_excerpts` 仍须由原文正文绑定校验。两种上下文在模型输入使用 `source_context_support_ids`，企业应答的依据仍由 `support_ids` 和位置权限决定。 |
| 格式选择 `BidFormatChapter` | `{heading_refs:int[], name, label?, parent_heading_ref?:int|null, body?:[int,int]|null}`。`label` 可选，须为所引 `heading_refs` 原文连续子串且不以 `附件`/`附录`/`附表` 开头，用于保留原件序号与大小写（支持首行序号与次行文件名拆行）；`parent_heading_ref` 指向本节先出现的上级章 `heading_refs[0]`（根章为 `null`）。分片超限时输入可附只读 `parent_context:[[ref,text]]` 供跨片父引用 |
| 归属请求 `item_placement` | `{operation:"place_items_v1", positions:[{id,title_path:str[],kind,opening,origin?}], requirements:[{id,kind,subject?,scope?,text,conditions?,exceptions?,submission?,consequence?,scoring?}]}`。`id` 为本批短 ID（位置 `t*`，事项 `r*`），`opening` 为位置原文开头，最长 120 字；`origin:"outside_format"` 标出格式章节外的候选原式 |
| `ItemPlacementResponse` | `{placements:[[r,value]]}`，每个本批事项恰好一行。`value` 为本批位置 ID、理由代码或 null（待核；字面 `"null"` 归一为 `null`） |
| 方案子大纲 `proposal_outline` | `{operation:"plan_proposal_outline_v1"|"plan_proposal_tree_v1", chapter_title, instruction, source_headings:[str], requirements:[{id,kind,text,scoring?,section_path?:str[]}]}`。输出 `ProposalOutlineResponse`（`{sections:[{id,parent_id:str|null,title}], assignments:[[r,s|null]]}`）或超窗两步模式的 `ProposalTreeOnlyResponse`（`{sections:[...]}` 后接分批 `place_items_v1`）。失败或拒收留原单目标并记 `outline_proposal_structure_pending` |
| 不需响应理由代码 | `evaluation`：评标程序或评委动作；`performance`：签订合同、付款、验收、合同变更、违约处理这类签约后才发生、投标文件不需说明的手续，采购需求的技术、服务、人员和交付要求不属此类；`document_wide`：整份投标文件的格式、份数、递交等约束；`not_applicable`：原文明示本项目不适用 |
| 宿主落账 | 位置 ID → 目标 `rule_refs`；`document_wide` → `constraint_rule_refs`；`evaluation`、null、未采纳批次和连同位置目录超出预算未发送的事项 → 覆盖台账 `pending`；其余代码 → `outline_rule_excluded` 问题，`detail` 写明理由 |
| 评标裁决 | 大纲保存可选 `evaluation_decisions: [{rule_ref, resolution, target_id?}]`。仅接受当前基线大纲的评标建议待核事项及完整正式 `EffectiveItemRef`。`resolution` 取 `evaluation`（纯评审流程）、`reference`（原文索引）、`document_wide`（整份约束）、`target`（本次保存后仍在大纲中的位置，须提供 `target_id`）或 `pending`（继续待核）。按现有修订并发及幂等规则保存。 |
| 开标项目名称 | `ResponseTarget.opening_project_name_excerpt` 为新链宿主记录的同快照公告原文精确引用。只在已绑定完整原生《开标一览表》的手填位置保存。模型和编辑请求均不指定文字、行列或填值，该值不计为企业应答。 |

## 编制交互策略（knowledge_required_v2）

`schema_version` 仍为 2，`knowledge_required_v2` 标识当前编制交互策略并进入执行 spec 与实际模型请求：不增加分析节点、Agent、产品确认门禁、环境变量或通用状态机。

### 依据与位置

招标来源支持项目背景、建设现状、要求解释及规定格式；知识/明确企业资料支持产品
功能、实现方法、企业事实和获授权承诺。来源事实不能被改写成投标人已经满足要求。
功能资料可以支持相应功能，无须先确定品牌型号；历史价格仍不得填成本次报价。

| 结构或字段 | 本轮最小修订 |
| --- | --- |
| OutlineTargetResponse / ResponseTarget.support_origins | `list[Literal["tender","knowledge"]]`，1至2项、去重，默认`["knowledge"]`；由已有大纲调用按位置内容选择。背景为`["tender"]`，企业应答为`["knowledge"]`，混合表单可含两者。只决定调度和允许的引用来源，不证明语义正确、不表示资料已匹配 |
| 知识应答位置 | 大纲生成后，宿主给每个 `manual_fill=true`、`support_origins=["tender"]` 且需要企业应答的招标原文位置追加一个相邻位置：`target_id=<原位置>:answer`、`kind=narrative`、`purpose` 同原位置、`manual_fill=false`、`support_origins=["knowledge"]`、`related_target_ids=[原位置]`。原位置保持只读，不改写招标原文；只有这个应答位置参与知识检索。没有可用依据时，应答位置写空段落占位，不生成正文（`attach_knowledge_answer_slots`） |
| 大纲归并 | 宿主联合已选目标的support_origins及已有检索意图；模型不重抄这些字段。同一混合表单保持一个实例，不为不同依据类型复制表格。kind仍为narrative/form/attachment |
| 模型请求supports | 本请求局部共享表：`support_id`、`origin:tender/knowledge`、`target_ids`、实际必要`text`；原表按需携原生grid，知识按需携usage及必要主体/产品/版本/当前版本标记。由宿主生成引用和元数据，必要文字只发送一份，替代重复的originals/materials正文副本 |
| AuthoringBlockResponse / CellFillResponse.support_ids | `Identity[]`，仅选择当前supports短引用。每个采用块/填值仍选择target_ids；源文说明可选择s1，企业应答须有适用的k1等知识支持。模型判断支持含义，宿主检查存在性、目标范围和允许origin，不用词表判断承诺 |
| CellFillResponse.source_id | 继续标识原表，选择origin=tender且有实际grid的support_id；行列仍是宿主原生坐标。填值本身可由同一原文或知识支持，固定内容不覆盖，最终价格仍由人工填写 |
| 持久AuthoringBlock / CellFill | 保留现有target_ids/material_ids，新增`source_refs:SourceUnitRef[]=[]`记录源文支持。宿主将support_ids拆解为正式资料和来源引用；CellFill.source_ref仍单独标识被填原表。短support_ids不写成跨执行身份，不为此迁移全部旧稿 |
| WritingResponse.attachment_ids | 取代模型顶层material_ids，仅选择有真实原件及可判断证据的knowledge支持短引用。持久attachments继续使用现有AuthoringAttachment，不让模型生成CAS |
| AuthoringArrangement模型视图 | 同样选择support_ids；共享企业安排仅由knowledge支持，宿主继续落盘material_ids。各批只处理本批支持，宿主合并已绑定安排，不要求模型重写旧批安排或重读旧材料；写作/复核装载本任务必要安排。无引用的approach不恢复为方案来源；项目背景在相应位置用招标来源支持 |
| AuthoringReviewRequest / execute_document.review_request | 可选`{target_ids:Identity[至少1项],reason:Text}`。新执行继承前驱时冻结进spec，宿主将选定目标转换成本请求短引用，将真实理由送入相关review和必要修订；初次write不添加此反馈，无关请求按完整摘要复用 |
| 复核content_ids | 沿用现有字段；b1/f1分别标识受审稿的正文/填值，补a1标识附件。别名只属于本次受审稿，宿主保存稿件引用与别名映射；旧稿序号不能用于删除新稿 |

示例：`s1:tender`记载项目位于甲园区，`k1:knowledge`记载支持角色权限。项目位置说明
选择s1，权限应答选择k1；没有容灾依据时该响应留空。仅有s1不能生成“我方已建成
容灾中心”。一个表格中项目名称可由s1填写，企业人员由适用k1填写，价格格保持空白。

### 有限检索计划与真实接口

检索意图在已有原文大纲调用内产生，不增加独立查询规划或语义匹配轮。
`OutlineResponse.search_intents:SearchIntentResponse[]=[]`，其中
`SearchIntentResponse{query:Text[1..256字符],for_ids:Identity[至少1项]}`；for_ids仅选择
该包已有规则/来源短引用。同一主题可关联多个引用，多个独立功能分别查询；不要求
模型回抄全量规则。纯来源说明和人工报价不产生知识查询。256只约束查询文字，不
裁剪原要求、条件和例外。

`AuthoringOutline.search_intents:SearchIntent[]=[]`；宿主保存
`SearchIntent{query,rule_refs:EffectiveItemRef[],source_refs:SourceUnitRef[],range_ids:Identity[]}`，
将for_ids绑定为真实引用及原范围。目标通过其规则/来源引用
关联查询，不另让模型誊写target-query关系。source-only位置直接依据实际原文或表格
产生意图，不能只从标题/purpose猜测。缺少必要意图记retrieval_plan_missing，不伪装
成已检索无命中；其他任务、明确提供的资料和源文说明继续。

保留现有assembly接口，每次query只发送业务查询文字。完整要求、条件、例外和范围
仍由write/review用于判断候选适用性，不拼入一个要求全部词同现的JSON查询。
宿主按`query原值＋检索范围`精确去重，合并关联目标；不按语义猜测去重，不在无命中
后递归扩写同义词，也不硬设每事项两查询从而丢掉复合要求或宽表的独立功能。

Q为全文件计划去重后的实际查询数，须在发送前报告。Q随独立检索主题及输入内容
增长，不等于章节数；O分包同时预留检索意图的输出容量，不能用输入准入掩盖查询
输出超载。超过容量按原文/规则边界缩包；不可拆时保留局部待规划缺口，不静默裁掉
查询或假称已经完整规划。查询条数的容量以实际输出预算验证，不凭空承诺固定值。

每次HTTP仍最多8个命中，但服务没有限制单个命中文字大小，响应体不能宣称已有
固定字节上界。原始命中先冻结，写作按任务装载必要文字；容量不足保留原片段和具体
待处理位置，不截掉支持条件后采用。仅原件无可读证据不能作为已匹配证明选入。

同一执行按单query与范围保存既有receipt，不仅按整个query数组缓存。回执保存实际
命中/空结果/诊断及来源，目标关系由本次检索计划重新绑定，不复用旧target_ids。
跨执行只有可证明知识快照同版时才复用查询；无法证明时重新检索，不能因query相同
跳过对更新知识的读取。已知检索版本/范围固定，现有API无法提供统一时点快照的限制
如实记录，不另建知识快照平台。保留必要产品版本及is_current等信息交模型判断，
不用强制型号确认解决语义适用性问题。

现有关键词索引simple不保证中文子词或同义召回；短查询只修复JSON污染及跨条款AND，
不承诺解决该限制。没有可用语义召回时记录检索能力/召回不足，结果只能表述“本次
未取得可用资料”，不能断言“知识库不存在资料”。不隐藏Embedding配置或Rust改造。

### 写作、复核和采用

| 情况 | 处理 |
| --- | --- |
| 全人工，或缺少本位置允许的任何支持 | 不写作、不正文复核；保留原格式/空位和真实原因 |
| 允许tender且相关来源可读，无企业知识 | 可写来源说明/源文可确定填值；企业应答部分仍留空，不用来源推出企业能力 |
| 有可用候选，模型完整返回但判断不匹配 | 保留空稿及理由，仍进入独立复核，检查是否漏匹配；不是传输失败，不无变化重试 |
| 有正文、填值或待采用且有可读证据的附件 | 复核实际内容、支持范围及当前资料足以支持的遗漏；只有片段不能冒充证明原件 |
| 写作传输/预算失败而形成纯空稿 | 沿既有恢复额度处理，耗尽后记运行原因；不为不存在的语义结果再发空稿复核 |
| 现有支持足够、非人工且can_revise=true | 无论generated或语义placeholder，都允许一次有真实反馈的定向修订；无资料、未知报价及外部授权缺口继续留空 |
| 已生成内容被模型明确否定 | reject_content=true或旧unsupported_content按当前稿的task/content引用剔除，包含虽有来源支持、但把编制说明或规则复述当作实际应答的用途错误；原表固定文字、要求行及正确兄弟保留；纯遗漏、待补、技术失败不据此剔除 |

以上从现有输入、manual_fill、内容、error_type及完整回包推导，不增加needs_review
状态。混合单元同时核对有据内容及漏匹配；“没有正文就不复核”不作为实现规则。
已有OCR正文投影到supports.text一次，OCR状态字典不作为可读正文。跨任务旧问题仅在
一次成功复核完整看到其全部关联任务时替换，分包未覆盖完整上下文时继续保留待核；
previous_findings.original_content_ids只表达历史位置，partial_context标明本包上下文不全。

版本采用规则固定如下：初稿U0经初审，明确无依据内容剔除后得到S0。修订仍可读取
U0及其真实意见，以便模型定位问题；S0是宿主保留的回退稿，不把已删块重新编号后
冒充原稿。修订稿U1按自身实际内容重新分配b/f/a别名并复核，旧content_ids只用于
原稿定位，作为历史意见送入新复核时不充当U1的删除指令。

- 初审只是接口/预算不可用：保留尚未被否定的U0草稿，明确待复核，不写成缺少资料。
- 初审已明确否定内容，改稿失败、空回包或U1再核不可用：采用S0，保存U1及回执，
  不回退含已否定内容的U0，也不直接采用未经再核的替换稿。
- U1复核完成：仅按U1的reject_content=true或旧unsupported_content意见移除被否定的
  当前内容，其他内容采用；纯缺项不扩大为正文否定，仍有问题停止自动改稿。
- 精确否定a1只移除该附件。不能精确定位时只将明确关联任务的生成应答留空，保留
  固定模板、缺口及可追溯原稿；不扩展为全文删除，不固定增加定位调用或阻断导出。

generated/placeholder继续只表示实际内容是否存在，核验完成与否由现有review/gaps
表达；质量、未核对、待补和价格问题不阻断保存/导出。首批优先消费已有可读证明文字，
缺少证据时保留附件位置；不隐含新增OCR/VLM付费调用。若后续需读取原页或新OCR，
另计其实际调用和输出，不凭图片标题判定符合。

### 调用、容量、恢复及验收口径

调用次数、预算分批、恢复额度与执行身份边界统一见 [业务工作流 · 编制实际模型调用](workflow.md#编制实际模型调用) 与 [失败执行的显式恢复](#失败执行的显式恢复)；业务验收口径见 [验收](acceptance.md)，实际运行结果与预算测量只维护在 [交接](README.md#当前状态交接)。

### Word 编辑器与 PDF 出件协议（v3）

Rust API（`crates/api/src/tender_editor.rs`、`crates/api/src/tender_render.rs`、`crates/api/src/tender_state.rs`）与 SQL 迁移（`migrations/20261028000003_tender.sql`）共同定义 v3 Word 编辑、保存回调与同版 PDF 出件的字段级合同。前端交互与部署约束见 [工作台与 Word 编辑](workbench.md)。

#### 持久表结构

| 表 | 主键 / 唯一约束 | 关键字段与不变式 |
| --- | --- | --- |
| `tender_draft_versions` | `id`；`UNIQUE(project_id, revision)`；同 `(project_id, execution_id)` 仅一个 `generated` | `project_id`、`execution_id`、`round_id`、`revision`（>0）、`parent_version_id`、`source`（`generated`/`manual`/`recovered`）、`object_ref`（`objects/<content_sha256>`）、`content_sha256`、`byte_length`（>0）、`media_type`（DOCX MIME）、`object_state`（`available`）、`outline`（jsonb 对象）、`created_by_user_id`、`created_at` |
| `tender_editor_sessions` | `id`（对外即 `editor_key`/`key`）；`UNIQUE(project_id, request_key)` | `project_id`、`base_version_id`、`owner_user_id`、`mode`（`view`/`edit`）、`request_key`、`request_sha256`、`created_at`、`closed_at` |
| `tender_draft_state` | `project_id` | `version_id`（当前选中版本）、`session_id`（活动编辑会话）、`pending_save_id`（待确认保存，延迟外键）、`editor_error`（`{"kind":"command"|"callback","code":<i64>}`） |
| `tender_draft_saves` | `id`（对外即 `save_id`）；`UNIQUE(project_id, request_key)` | `project_id`、`session_id`、`parent_version_id`、`parent_sha256`、`request_key`、`request_sha256`、`state`（`pending`/`saved`/`failed`）、`result_version_id`（与 `state='saved'` 互为充要）、`callback_sha256`、`error_code`、`created_at` |
| `tender_draft_recoveries` | `session_id` | `callback_sha256`、`version_id`（记录 `status=2` 最终关闭回调产出的 `recovered` 版本，防重放内容漂移） |

#### 公开版本对象

`GET .../docx/current`、`POST .../docx/current`、`GET .../docx/versions/{version}` 及工作台版本列表返回同一版本 JSON 结构：

`id`、`version_id`（同 `id`）、`sha256`、`docx_sha256`（同 `content_sha256`）、`project_id`、`workspace_id`（同 `project_id`）、`round_id`、`execution_id`、`revision`、`source`、`created_at`、`outline`、`byte_length`、`object_ref`、`editor`（`{key, base_version_id, pending_save_id, save_error}`；仅在当前选中版本等于该版本时返回活动 `key`、`pending_save_id` 与 `save_error`）。

#### HTTP 路由、请求/响应字段与错误码

所有 `/api/v3/tender-projects/{project}/...` 用户路由均要求登录且校验项目创建人归属；所有写操作请求体启用 `deny_unknown_fields`。

| 路由 | 必需头 / 请求体 | 响应与行为 | 关键错误码 |
| --- | --- | --- | --- |
| `GET /api/v3/tender-projects/{project}/docx/current` | — | 当前选中版本对象或 `null` | — |
| `POST /api/v3/tender-projects/{project}/docx/current` | `{version_id, docx_sha256}` | 项目须为 `open` 且无活动 `session_id`/`pending_save_id`；切换 `tender_draft_state.version_id` 并清空 `editor_error`，返回版本对象 | `409 DOCX_VERSION_CONFLICT` |
| `GET /api/v3/tender-projects/{project}/docx/versions/{version}` | — | 指定版本对象 | `404` |
| `GET /api/v3/tender-projects/{project}/docx/versions/{version}/download` | — | DOCX 字节流（`cache-control: private, no-store`、`x-content-type-options: nosniff`），读出后校验 `docx_sha256` 与 `byte_length` | `404`、`503 ONLYOFFICE_UNAVAILABLE` |
| `GET /api/v3/tender-projects/{project}/docx/versions/{version}/review` | — | 经 `TenderBridge` 返回 `docx.review.status` | `404` |
| `POST /api/v3/tender-projects/{project}/docx/versions/{version}/review` | `Idempotency-Key`（1–200 字节）；`{docx_sha256, reason}`（`reason` 非空且 ≤4000 字符） | 校验摘要后经 `docx.review.start` 启动保存稿复核，返回 `202 Accepted` | `409 DOCX_VERSION_CONFLICT`、`422` |
| `POST /api/v3/tender-projects/{project}/docx/editor` | `Idempotency-Key`；`{version_id, docx_sha256, mode: "view"|"edit", language?}`（`language` ≤40 字节 ASCII 字母数字/`-`） | `edit` 要求项目 `open`、目标为当前选中版本且无活动 `session_id`/`pending_save_id`；同 `request_key` 且 `request_sha256` 一致且未关闭可幂等复用。返回 `{config, api_script_url, session: {editor_key, round_id, version_id}}` | `409 PROJECT_ENDED`、`409 DOCX_VERSION_CONFLICT`、`503 ONLYOFFICE_UNAVAILABLE` |
| `POST /api/v3/tender-projects/{project}/docx/editor/save` | `Idempotency-Key`；`{editor_key, expected: {version_id, docx_sha256}}` | 校验基线一致且无其他待保存，写 `tender_draft_saves(pending)` 并向 `KB_ONLYOFFICE_COMMAND_ORIGIN/coauthoring/CommandService.ashx` 发 HS256 签名 `{c:"forcesave", key, userdata: save_id}`。返回 `{save_id, pending, dispatch}` | `409 DOCX_VERSION_CONFLICT`、`503 ONLYOFFICE_COMMAND_UNCERTAIN`、`503 ONLYOFFICE_COMMAND_REJECTED` |
| `GET /api/v3/tender-projects/{project}/docx/editor/{key}/saves/{save}` | — | `state='saved'` 时返回 `{save_id, editor_key, parent_version_id, round_id, version_id, docx_sha256, revision}`，否则 `null` | — |
| `POST /api/v3/tender-projects/{project}/docx/editor/{key}/close` | — | 无 `pending_save_id` 时清空 `tender_draft_state.session_id`/`editor_error` 并写 `closed_at`，返回 `{closed: true}` | `409 DOCX_VERSION_CONFLICT` |
| `GET /api/v3/tender-projects/{project}/docx/editor/{key}/source?token=<jwt>` | 能力 JWT（`aud="tender-word-source"`） | 实时核验 `tender_editor_sessions.closed_at IS NULL` 与项目归属后返回 `base_version_id` DOCX 字节 | `401`、`403` |
| `POST /api/v3/tender-projects/{project}/docx/editor/{key}/callback?token=<jwt>` | 能力 JWT（`aud="tender-word-callback"` 且 `mode="edit"`）+ ONLYOFFICE JWT（`body.token` 或 `Authorization: Bearer`） | 成功返 `{"error":0}`，失败返 `{"error":1,"code":...}`；语义见下节 | `401`、`403`、`409`、`422`、`503` |
| `POST /api/v3/tender-projects/{project}/docx/versions/{version}/pdf` | `{docx_sha256}` | 校验版本、摘要与体积上限 `min(max_file_bytes, 50 MiB)`，4 并发槽位与 `min(KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS, 180s)` 限时，经一次性内部取源通道 `/api/internal/tender-saved-pdf/{project}/{version}/{nonce}/source?token=<jwt>` 调 ONLYOFFICE `converter` 返回 `application/pdf` 及 `Content-Disposition: attachment; filename="tender-v<revision>.pdf"`、`X-KB-Render-Source-Version`、`X-KB-Render-Source-Sha256`、`X-KB-Render-Pdf-Sha256` | `409 DOCX_VERSION_CONFLICT`、`413 SAVED_PDF_FILE_TOO_LARGE`、`503 SAVED_PDF_UNAVAILABLE` |
| `POST /api/internal/tender-render/{execution}` | Worker Bearer JWT（`GRPC_AUTH_TOKEN`，`purpose="tender_docx_render_v1"`，含 `execution_id`/`docx_sha256`/`epoch`/`exp`），请求体为 DOCX 字节 | 供编制版式核对使用的内部转换入口，配 `/api/internal/tender-render/{execution}/{nonce}/source?token=<jwt>`（`aud="tender-docx-render-source"`），返回 `application/pdf` 及 `X-KB-Render-Engine: onlyoffice-converter-v1`、`X-KB-Render-Service-Sha256`、`X-KB-Render-Source-Sha256`、`X-KB-Render-Pdf-Sha256` | `401`、`403`、`422`、`503` |

#### 签名、能力令牌与回调状态机

- **环境变量**：`KB_ONLYOFFICE_SERVER_ORIGIN`、`KB_ONLYOFFICE_COMMAND_ORIGIN`、`KB_ONLYOFFICE_API_ORIGIN`、`KB_ONLYOFFICE_JWT_SECRET`、`KB_ONLYOFFICE_CAPABILITY_SECRET`、`KB_ONLYOFFICE_TOKEN_TTL_SECONDS`（默认 `1200`）、`KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS`（默认 `120`）。
- **配置签名**：`config` 使用 `KB_ONLYOFFICE_JWT_SECRET`（HS256）签发，带 `exp = now + TTL`；`view` 模式下 `document.permissions` 的 `edit`/`review`/`fillForms`/`comment` 均为 `false` 且省略 `callbackUrl`，`download` 为 `true`；合法浏览器 `Origin`/`Referer` 会附加导航插件 `onlyoffice-navigation/config.json`。
- **会话能力 JWT**：使用 `KB_ONLYOFFICE_CAPABILITY_SECRET`（HS256）签发 `{aud, project, key, version, owner, mode}`，不依赖 `exp` 过期，每次取源与回调均由数据库 `closed_at IS NULL` 与项目创建人实时校验撤销。
- **回调验证与状态处理**：校验能力 JWT 与 ONLYOFFICE 服务 JWT（去除 `iat`/`exp` 后须与回调业务体逐字段一致，且 `body.key == key`）；回调下载地址须同源于 `KB_ONLYOFFICE_COMMAND_ORIGIN` 或 `KB_ONLYOFFICE_SERVER_ORIGIN`，拉取时统一改写走 `KB_ONLYOFFICE_COMMAND_ORIGIN` 且禁重定向、限 `max_file_bytes`：
  - `status=1`（编辑中）：仅校验会话有效；
  - `status=4`（无修改关闭）：无 `pending_save_id` 时释放 `session_id` 并写 `closed_at`；
  - `status=2`（关闭定稿）：按 `callback_sha256` 幂等，经 `docx.outline` 重算目录写入 `source='recovered'` 版本与 `tender_draft_recoveries`，将未关联的 `pending` 保存标 `failed(error_code=2)`，释放会话但**不替换**用户当前选中版本；
  - `status=6` 且 `forcesavetype=0`（显式关联保存）：按 `userdata=save_id` 与 `callback_sha256` 校验幂等，核对 `tender_draft_state` 的父版本、摘要、会话与 `pending_save_id` 一致后下载文件并重算目录，原子写入 `source='manual'` 新版本、推进 `tender_draft_state.version_id`、清空 `pending_save_id`/`editor_error` 并置 `tender_draft_saves.state='saved'`；
  - `status=7` 且 `forcesavetype=0`（保存失败）：置 `state='failed'`、`error_code=7` 与 `editor_error={"kind":"callback","code":7}`，清空 `pending_save_id`；其余状态或非 `forcesavetype=0` 拒绝。

