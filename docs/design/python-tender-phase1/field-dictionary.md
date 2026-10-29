# Python 第一阶段唯一字段字典

## 业务协议v2（工作区已实现）

**本文件是第一阶段字段名称、类型、可选性和身份语义的唯一规范。**
数据库内部表名、存储列及外键规则另由 [数据库命名与关联约定](../database-naming.md) 定义；存储改名通过读写适配保持本文 HTTP/gRPC、LLM 和 CAS 字段合同，不把数据库列名直接暴露为新的业务字段。
[业务工作流](business-workflow.md)规定处理顺序，其他设计文档引用本字典，不另建同义结构。
工作区`tender/schema.py`、抽取/核验提示和五节点主线已实现v2；下文单列的v1合同仍供
历史独立工具使用，不自动转换旧回包。v2压缩、回包容错和默认重试已部署，两类审阅身份修复后真实独立核验已恢复，新协议语义验收未完成；
测试及部署事实以[交接](handoff.md)为准。
下文“问题续报与按需补读”已在源码实现；可信问题来源为normalization_version=4，处置恢复出口及覆盖分级修复为5，工作单元重试与JSON反馈升为6，joint职责与阶段内调度升为7，discovery已有关系及完整并集准入升为8。跨包关系核验按完整证据合批使用9；associate/joint专用合批、逐组恢复和共同反证归属使用10，完整共享来源短号稳定性为11；当前接收合同12仅规范化成员内精确空shared_findings数组。旧执行继续其冻结版本，新旧核验计划不混用。部署及实测状态见交接。

第一阶段完整分析招标文件，目标为 `register → freeze → extract → verify → confirm`，
确认后到 `analysis-ready`。输入为招标文件、附件、澄清和补遗；不引入企业响应数据、响应计划、
准备待办、评分采纳或实际得分。原文明示的填表、证明、附件和签章要求属于来源事项。

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
| `relation_id` | `<work_unit_id>:l1` 等台账内唯一身份；端点必须是实际提供的事项 ID，不能从标题编造 |
| `Issue` | 模型公共问题结构 `{code: 非空string, detail: 非空string, blocking: bool}`，按实际定位可附 `range_id: 非空string`、非空 `item_ids: string[]`；新协议可附 `related_issue_ids`、`needs_source`，语义见下文；抽取 issues 与核验 findings 使用同一类型 |
| `issue_id` | 宿主为已持久化问题赋予的具体身份；解决问题须绑定它及当前版本，不能仅按 code 清除 |
| 未解析引用 | 使用 Issue，必须以 range_id 定位引用处，detail 保留原文引用及未决原因；目标未知时不填虚假 item_id，不新增引用状态机 |
| `Decision` | `{decision_id, issue_ids: string[], reason: 非空string, evidence: 非空Evidence[]}`，可附非空 `relation_ids: string[]` 和一个 `amendment: AmendmentEffect`；是待独立核验的决议提案，不是模型自行解除阻断的指令 |
| `decision_id` | `<work_unit_id>:d1` 等当前台账内唯一身份；issue_ids 只引用输入中宿主已赋号且本提案拟完整解决的问题，只有不解除既有问题的补遗决议才允许为 `[]` |
| 问题决议 | 通过 Decision 绑定具体 issue_id、当前来源/领域版本、证据和理由，使用现有不可变修订/审议回执；独立核验明确接受后才生效。仍未解决的语义疑点只能在确认时由人工逐条处理（见[问题分级与人工处理](#问题分级与人工处理)），硬阻断不能由确认处理 |

关联只表达源文事实，例如评分作用对象、证明文件对应义务、否决触发条件或补遗修改对象。
抽取先记录本包已知关联；后续补读形成独立不可变的关联决议并接受核验，不改写已保存回包。
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
| 抽取输出 | `{schema_version: 2, items: TenderItem[], dispositions: Disposition[], relations: Relation[], issues: Issue[]}`，五项均必填。模型只为 `non_item` 和 `unresolved` 返回 `{range_id,status,reason}`；被事项证据引用的范围省略，由宿主派生 `items` 处置 |
| `Disposition` | 持久化为 `{range_id, status, reason, item_ids}`；status 为 `items/non_item/unresolved`，reason 为有来源依据的非空说明。规范产物仍覆盖每个 body 范围 |
| `item_ids` | 宿主按事项的有效直接 evidence 派生反向索引，模型不再生成。规范产物中 items 处置非空，non_item/unresolved 时为 `[]`。既未被引用也未声明的范围由宿主记问题并交 review |
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
- locate 候选的 `range_ids` 是来源选择集合。宿主可按首次出现顺序移除完全相同的重复 ID，
  记录 `candidate_range_ids_deduplicated`、字段 `path` 和重复的 `range_ids`；保留原始回包。
  不猜测、更换或删除未知来源，不合并候选，不修改 reason/findings，其他字段及来源边界仍校验。
- 必填顶层结构无法解析、身份无法安全绑定等执行失败仍报错；原文追溯、上下文容量和付费
  防重放继续生效。历史产物不原地重写；请求协议/提示及宿主处理版本绑定执行摘要，不能
  用升级后的发送视图冒充旧请求的实际 payload。JSON Schema 增强不作为本轮跑通前提。

### 模型输入与 verify 内子操作

抽取及 verify 子操作的输入均带 `schema_version: 2`、`work_unit_id`；`source_version` 与
`domain_revision` 只留在宿主回执和执行快照，不进入模型 payload。run 身份、输入引用和
ID 到不可变产物的映射由宿主回执固定。实际协议、操作及内容绑定 input_digest/request_digest，
不能只凭 task 相同复用。

| 输入字段/类型 | 唯一定义 |
| --- | --- |
| `source_views` | SourceView[]，实际装载的完整来源视图；抽取恰有一个，多文档核对分别保留每份视图的 document_id/document_version，不拼成假单文档；当前文档的登记 role 在视图中发送一次，保留主件/补充文件等理解依据。文档按登记顺序、body/supporting 各按冻结原文顺序及片段字符起点排列，不按哈希 ID 排序；排序由宿主完成，不额外发送排序元数据。同一文档按已有区间去重规则投影成一个请求视图，不跨来源按文本去重 |
| `SourceRangeView` 正文 | 普通段落保留 `text` 原文。表格行用 `cells: SourceCellView[]` 直接提供单元格原文；当各单元格及原生行分隔符完整表达本范围时省略重复的整行 `text`。不能完整表达的范围保留 `text`，不得丢掉未被单元格覆盖的原文；宿主的规范行全文、字符偏移与定位保持不变 |
| `SourceCellView` | `{column: 非负int, text: string, row?: 非负int, row_span?: 正int, col_span?: 正int, formula?: string, ...}`；text为当前范围实际可见的单元格原文，允许空字符串，保留换行、Unicode与合并/公式等原有语义字段。不发送cell的start/end让模型切片；片段由代码按规范字符区间切好。证据按本次实际可见的text或单元格原文逐条引用，不能引用未发送的单元格剩余内容 |
| `source_aliases` | SourceView内的既有引用身份及实际已发segments映射，不新增正文副本。引文须受原别名区间约束且由本次实际可见文本证明；普通段落的连续已发片段可还原引文，cells-only片段不能借规范整行全文或分隔符拼接成引文。若整行text实际已发，可引用其原文子串 |
| `catalog` | CatalogEntry[]，仅 locate 携带；discovery 为本批事项及已提供关系的证据所在文档/章节的局部目录，reference 为完整章节定位目录。抽取不携带全目录，associate/review 使用已装载原文。目录不表示正文已经装载或审阅 |
| `CatalogEntry` | `{document_id: string, document_version: string, role: string, sections: CatalogSection[], page_count?: 正int}`；文档身份只发送一次，role 沿用登记角色。仅冻结PDF有完整、连续的0基物理页清单且页数一致时提供page_count；不发送全量页码定位表 |
| `CatalogSection` | `{section_id: string或null, heading_path: string, range_ids?: 非空string[]}`；discovery 的 range_ids 仅映射本批事项及已提供关系实际引用的来源，不附全章范围清单、页码、坐标或存储引用，也不扩大证据引用权限。宿主通过当前冻结索引的 `(document_id, section_id)` 装载全部章节原文；无章节为null，不得按标题文本猜测或去重 |
| `items`, `dispositions`, `relations`, `decisions` | 对应本字典的完整对象数组；由宿主按实际引用装载，不发送内容摘要冒充原对象 |
| 输入 `issues` | 已持久化 Issue[]，每项额外带宿主赋予的 issue_id；相关来源缺口也须显式提供 |
| 输入 `scope_decisions` | 可选的现有 ScopeDecision[]；当前修订有显式人工作用域审议时发送完整 decision、rationale、evidence 和 target_range_ids，包括 local_only。是需结合原文复核的解释/绑定依据，不自动解除独立核验或来源阻断；无审议时省略 |
| `Candidate` | `{candidate_id: string, issue_ids: string[], range_ids: string[], item_ids: string[], reason: 非空string, section_refs?: SectionRef[], page_refs?: PageRef[]}`；至少一个来源、事项、章节或物理页引用；前三个 ID 数组必填，无值的可选引用数组省略。range_ids 仅引用实际提供的来源或完整事项/关系证据，section_refs 仅引用目录中已提供的章节，page_refs仅引用目录声明支持的物理页。未指向既有问题时 issue_ids 为 `[]`。共享来源或事项不自动合并独立候选；同一具体 issue_id 的多个候选须联合核对，单个候选不得拆分 |
| `SectionRef` | `{document_id: string, section_id: string或null}`；同一候选内不得重复。宿主解析引用并加载该章节全部原文及必要表头/条件后才能 associate 和联合 review；目录或引用本身不能作为原文证据，装载后超预算须阻断，不能静默截断章节 |
| `PageRef` | `{document_id: string, page_ordinal: 非负int}`；page_ordinal为PDF文件0基物理页序号，须小于该文档catalog.page_count，同一候选内不得重复。宿主用冻结页清单/原生物理定位装载该页全部原文、图片OCR及必要条件；缺失、部分解析、OCR未完成或预算不足保持阻断。印刷页码与PDF物理页序号不能默认等同；仅写“见第N页”且映射不明时保留未决，不用正则或模型猜测映射 |
| `candidate_id` | `<work_unit_id>:c1` 等身份；只表示待联合核对的候选组，不证明存在冲突或关系 |
| `operation` | `locate/associate/review`，仅用于 task=verify 的固定子操作，不增加主节点或 RunStatus |

抽取输入为公共版本身份、source_views、issues，不发送全局 catalog。标题、表头、条件、脚注及
影响含义的表格结构随实际原文提供；页码、坐标、内容哈希和存储位置由宿主按来源引用回查，
不要求模型还原这些 metadata。压缩不删改原文，不省略正文处置或全源核验覆盖。

例如下列发送行直接给出两列原文；模型用 `range_id` 和某一单元格的原文子串引用，
宿主按该引用回查冻结全文及位置。列之间的分隔符不另作可引用原文，不能自行拼接引文：

```json
{"range_id":"row-1","kind":"table_region","column_count":2,
 "cells":[{"column":0,"text":"交付期限"},{"column":1,"text":"合同签订后30天内"}]}
```

verify 各操作的必需输入和回包如下，
所有输出带 schema_version=2，列出的数组均必填且允许为空：

容量验收必须使用实际发送视图，逐包核对规划与运行时测量一致，并记录正文/上下文/其余开销、
处置条数及输出 JSON 结构下限、各阶段已知调用数。UTF-8 字节估算和输出预留不是真实计费
Token，结构下限也不是完整输出预测；`fits` 不证明输出能完整生成。131072 窗口保持不变。
字节估算的输出预留仍是 16384，只用于准入，不是 wire 输出上限；verify 在 token 计数下用
32768。真实联调需同时检查实际 usage、结束原因和完整 JSON/覆盖校验；usage 缺失保留未知，
同一逻辑单元的重试不通过换身份重置额度；uncertain保留原审计并计入四次上限，仍在途的
sent不可并发重发。已记录的抽取 completion 曾到 43709，不能把 16384 当成
真实输出上界，也不据此改默认窗口。更换发送格式后的请求必须重新测量，旧估算不能直接套用。

当前修订有 scope_decisions 时，抽取与核验同时携带该列表，并将原文、实际目标及关联事项
共同送入独立 review。不能仅凭 domain_revision 变化假定人工解释已送达或已通过。

| operation | 输入（除公共版本身份外） | 输出 |
| --- | --- | --- |
| `locate` | catalog、issues、items；引用定位须带原引用的 source_views，跨源候选发现须带计划覆盖的完整事项、两端事项均在场的已有完整relations及来源身份 | `{schema_version: 2, candidates: Candidate[], findings: Issue[]}`；只能提出来源定位或跨源候选，不能解决问题或判定全局无冲突 |
| `associate` | candidates、source_views、items、relations、issues；宿主装载引用处、候选目标、条件及表头的全部必要原文 | `{schema_version: 2, relations: Relation[], decisions: Decision[], findings: Issue[]}`；只生成待验关系/决议，不得修改已保存事项或解除问题 |
| `review` | source_views、items、dispositions、relations、decisions、issues，以及本次联合核验的 candidates | 上文核验输出；独立读取原文核对事项、关系及每条决议，不沿用 associate 的结论当作证据 |

所有必要来源仍须进入 review，包括 non_item/unresolved 范围；关联核对不能代替全源查漏。
review 明确列入 accepted_decision_ids 的决议，经宿主校验当前版本、具体问题及完整审阅后
才生效。未审阅、未接受或审阅失败的决议保持无效；拒绝须给出有依据的 finding，原问题保留。
review 是独立请求，不共享 associate 的对话历史；同一决议的必要审阅存在失败、未决或反对时，
不能挑选其中一个接受回包放行。
findings=[]、只审阅了某个 ID、人工确认或 associate 请求成功均不能解决旧阻断。
技术/结构/来源完整性及付费围栏不接受语义决议豁免，原事项错误须走修订而非决议覆盖。
若解决问题需要改 scope_binding 或抽取内容，须复用现有显式审议/纠错修订并重新核验，
不能仅以 Decision.reason 或接受记录改变来源绑定、修复原项或替代修订回执。

宿主在 verification_ref 下持久保存候选、关系/决议提案、独立审阅及接受结果，绑定当前版本、
work_unit_id、input_digest 和原始产物引用。只对被有效接受的具体 issue_id 记录解决结果，
不删除历史 Issue；来源或领域内容变化沿用修订和重验，旧确认失效。

### 关联与联合核验的专用合批合同

本节对应[效率修复方案](analysis-efficiency-plan.md)，适用于normalization_version=12新冻结执行的
associate/joint合批；不转换旧执行回包。既有共同问题/续报形成的work_unit
仍是不可拆语义依赖组，物理批次不是新的业务实体。其他抽取、review和locate合同不变。

| 字段/位置 | 唯一定义 |
| --- | --- |
| 合批输入公共池 | schema_version、operation及完整source_views/items/relations/candidates/issues，存在时带scope_decisions；associate保留原dispositions，joint沿用dispositions=[]并另带decisions。复用现有字段含义，仅同来源身份且内容一致的副本去重，保留完整必要条件/表头/例外；宿主保持成员的原输入映射，模型无需回传 |
| 输入 `work_units` | 原冻结批全部成员的非空数组；每项`{work_unit_id, candidate_ids, issue_ids}`，joint另有`decision_ids`。身份指向宿主保留的逻辑组，指定各组处理目标，全部公共池是模型可见依据；局部纠正/拆目标时仍保留成员及引用，不另造group_id或业务分类字段 |
| 输入 `target_work_unit_ids` | 本次须回答的成员子集，初次associate为全部成员，joint为需核验决议的成员，局部纠正只列仍需回答者。其余成员可被共同反证明确指向，但无需重新输出结果；不是模型要回抄的覆盖清单 |
| `work_unit_id` | 合批内不再用一个顶层值代表全部目标，每个成员有稳定的原工作单元身份；发送时映射为短引用，输出只回其必要路由号。物理request_digest及批次回执留宿主，不能借换批次重置成员尝试 |
| associate输出 `work_units` | 每项`{work_unit_id, relations, decisions, findings}`；数组沿用当前合同且允许为空，新relation/decision身份在该成员命名空间内唯一；决议不因associate完整返回而生效 |
| joint输出 `work_units` | 每项`{work_unit_id, accepted_decision_ids, findings}`；accepted_decision_ids是语义接受选择，保留；不新增模型誊写的reviewed名单，宿主从有效成员结果及实际发送回执派生执行覆盖，不据此宣称语义完整 |
| 输出 `shared_findings` | 必填数组，无值为`[]`；每项`{work_unit_ids: 非空string[], finding: Issue}`。用于跨组意见或对本次目标外兄弟组的意见，明确引用输入work_units中的成员，可包括此前已成功且此次不待回答的组；Issue自身结构不扩充，仅针对当前一个目标的意见放该组findings |

两个输出外层均带schema_version，成员结果只须覆盖target_work_unit_ids指定的目标。
接收端仅可在成员视图中省略精确的`shared_findings=[]`冗余，不修改原始物理回包。
非空或错误类型仍以具体字段拒收，反馈要求保留内容并在外层明确`work_unit_ids`；
不得自动搬移、猜测共同意见的归属或忽略其他额外字段。外层真实共同意见及兄弟组结果
保持原合同。版本12不改变发送载荷、Prompt或重试额度，旧版本11冻结执行不热替换。
先取得正常完成的完整JSON，再分别校验公共归属与
成员结果；不把所有成员绑成一个不可分的schema接受结果，也不从半截JSON提取成功组。
局部finding保留原组内整体接受逻辑；共同finding只作用于模型明确列出的组，宿主不得
从共享item/range猜测传播范围。阶段最终汇总须应用全部明确归属的共同意见，后续反证
指向此前成功组时按既有规则排除其受影响决议接受并保留未决，原成功回执不改写；不为
该语义意见自动启动新的修复循环。无归属或未知组的共同意见不能静默丢弃或先放行潜在
受影响组；保留原回包和具体错误，在同一剩余额度内纠正，耗尽后留未决及原恢复出口。

joint及局部纠正保留associate实际可见的完整公共依据和必要提案/反证，不按最终引用
缩回旧私有来源。joint目标可在预算不足时拆批，语义组不拆；完整单组仍超限走既有
修订出口。首版不支持组内逐决议部分接受，也不引入新的模型调度或递归补读。

宿主沿现有CAS/产物表保存公共依据、逐组结果和对应原始模型回执引用。结果摘要绑定
实际任务与完整共享依据、Prompt/模型/协议，整份计划只作导航；尝试按稳定逻辑目标
累计，批次、反馈、缺组重试和拆批均不能重置首次加三次额度。发送前原事务核对全部
成员额度和在途身份；重启先重放成功物理回包，补齐未保存的成员，随后才能规划新调用。
传输succeeded不等于业务接受，不清缓存或更改旧传输状态来获得新答案。

核验结果的历史统计counts.model_calls仅表示完成的规划请求数，含复用且不展开纠正；
operation_refs仍按逻辑组保存，并可附batch_ref追溯实际共同依据。真实付费调用、重试、
未知usage和耗时必须从model_requests的发送记录及供应商usage统计，不用这两个计数替代。

本节不新增主节点、RunStatus或数据库任务表。共享提示变动按完整摘要计算失效范围，
升级使用新冻结运行，不在当前运行中混用新外层和旧回包。容量及重跑公式见方案；实际
部署/真实验收只记交接，旧“共读暂列备选”描述属于此前已实现的小修订范围。

### 跨源候选发现与联合审阅

locate 除处理已知引用外，还须对当前完整事项台账发现跨包/跨文件的同对象规则、条件及
潜在修改或冲突候选，不能只检查抽取已声明的 relations，不能按 domains、关键词或没有
显式引用而提前排除事项。容量允许时一次提供完整台账；超限则固定事项块及块内、块间的
候选发现批次，使每个事项及块组合都进入发现清单。计划与完成回执进入 verification_ref，
未完成的必要发现批次阻断确认。遍历完成只证明本次范围已审阅，不保证模型发现全部关系。

候选发现只决定哪些来源需一起读，不以目录、事项摘要或局部通过判定不存在冲突。
宿主为每个 Candidate 装载完整相关来源、事项和必要条件，进入独立 review；涉及新关系或
问题解决时先 associate 再独立 review。必须联合审阅的候选须有实际共同装载原文的回执，
不能用不同请求的 reviewed_* 并集代替。候选缺原文、歧义或完整依赖超预算时保留阻断问题，
不截断或自动递归扩展。引用补读仅一次 locate、一次 associate，新引用链及仍未决内容转人工。

不新增 analysis_ref，结果及评分/后果视图从现有不可变产物和确认回执投影；不维护另一份事实。
目标不含 requirement_id、requirement_ref、requirements、veto_risk、response_plan_ref、
响应 target/action/material_needs/constraint_digest，也不包含评分采纳字段。

### locate 输入压缩与问题合批（BUG-014）

以下为已实现并通过回归的修复合同；部署及现有运行切换状态单独记入交接文档：

- 首轮复用完整 TenderItem，所有字段及 evidence.quote 保持原值，暂不引入 LocateItemView。
  将 quote 延后装载的投影虽有进一步节省，但事项 text 不保证与引文语义完全等价，
  需后续语义对照验证后再决定。associate/review 继续装载完整事项、证据与必要来源上下文。
- 事项间 discovery 提供本批事项两端均在场的完整已有relations及evidence，供模型判断台账
  是否已表达某组合，不由宿主按关系存在跳过候选。catalog仅包含本批事项及这些关系证据
  所在的文档和章节，保留 document_version、
  role、heading_path 及已声明的 PDF page_count。各章节可附 `range_ids: string[]`，仅列出
  本批事项及已提供关系实际引用的来源，用于建立证据身份到文档/章节的映射，不扩充可引用来源。
  复用现有目录结构，不新增定位节点；完整 heading_path 保留章节及标段上下文，不根据
  标题正则推断适用性。reference 合批保留全目录和原引用 source_views，允许选章节/页。
  两者仍属既有 locate 子操作，不增加工作流主节点或状态。
- 多个 reference 问题按完整请求预算合批，初始每批最多8个问题，不增加环境变量；
  这是控制输出及注意力负荷的执行批量，不是输出 Token 上限或语义保证。同一来源原文及上下文在批内共用；各问题的
  issue_id/detail 保持不变，不按文本相似度合并问题，也不将共批误作存在业务关系。
- 保留全部事项、问题与当前块内/块间发现覆盖。已有引用须可回查，关键文字、条件及
  单元格内容完整保留；不得用删去跨包比较或裁剪事项文本来换取请求数降低。
- 首轮沿用现有目录结构。正式实现需同步输入校验、候选引用边界和 Prompt，并以真实
  输出容量及用量复核批大小。早期原型为23包/16包；生产实现保留局部语义目录并限制8题，
  同一文件离线为40包。均不是固定请求上限或语义验收；不能只按输入上限塞满问题，
  也不能直接外推为多文档容量结果。块间容量试算同时为两侧事项和局部目录预留空间。
- 初始块试算不能界定跨块关系体积，最终须按含完整关系及目录的真实块并集重新准入。
  超限则确定性二分其中最大的可拆事项块，重建全部块对；不可分事项对仍超限走原预算
  修复出口，不删关系或事项对。全量事项、关系和目录不因规划修改而变动。
- 修改后的请求和计划使用实际新摘要；已有成功产物及原回包保留，不原地改写旧执行快照。
  当前固定协议的运行如何切换需在实施时明确，不能直接把旧计划的回执视为新计划完成。
现有 ArtifactRef、来源定位、输入/请求摘要、预算及发送围栏继续使用下文同名合同，
plan_digest/pack_plan_ref 仅表示分包执行计划，不能理解成投标响应计划。

### 问题续报与按需补读

本节实现BUG-023至025的首批修复；026及引文延后仍为后续实验。现行合同及运行回执不自动迁移。五主节点、RunStatus、
TenderItem的业务含义、Decision的生效方式保持不变。不增加分诊Agent、业务分类路由或新数据库表。

**先明确核验范围。** 原文review以本请求dispositions列出的范围检查“应抽未抽”，
宿主保证这些目标范围的已知直接证据事项一并发送。所有必要范围最终都须作为该目标进入review。
额外body、supporting及alias仍用于核对事项含义、条件和作用范围，但其事项台账可能不完整，
不能仅凭本包没带对应事项宣称全局漏抽。发现真实疑点仍保留，不能由宿主按范围删除finding。
联合review另以实际候选、事项、关系和决议为目标，所有共同依赖仍须读到。
associate及其joint仅携带明确选入范围且item_ids全部在本包items中的disposition；
额外上下文或证据闭包中的不完整台账不作为漏项检查目标，原文照常提供。
这只约束发送投影，不修改全局处置，也不免除原文review对全部必要范围的覆盖。

**局部格式错误保留可用核心。** 正确的item_id/kind/text及可追溯evidence可独立成立时，
可选字段异常不应直接删除整个事项；保留核心和有效字段，将不能接受的字段原值、错误路径
及原回包引用保留为诊断，可能影响含义的部分送独立核验。numbers沿用现有容错；其他字段
不能仅因可选就认定不影响业务。不能把conditions/exceptions猜成其中一个字段，不能由代码
把“至少”改为“至多”。核心字段/正文证据也不成立时，仍保留原声明与位置为待核对问题。
这表示可展示、可继续核验，不表示该事项业务通过；真正漏项/错项仍走显式领域修订重验。
送审不能只有本地诊断或CAS引用：宿主在既有Issue.detail中带入待核对字段名、原值、错误路径，
绑定可用item_id/range_id并装载对应原文；完整原回包仍只保存一份。只发送本次相关异常，
不把整包JSON重复到每个review。异常内容仍是不可信数据，纳入请求预算；不能通过截掉
条件/例外后将其当作已核验。确实放不下时保留明确未完成项，不静默丢弃。

**仅为Issue增加两个可选的模型语义字段：**

| 字段 | 类型 | 语义 |
| --- | --- | --- |
| `related_issue_ids` | 非空string[]，可省略 | 本条finding续报的输入中既有问题身份；只能引用实际提供的同修订问题。用于共同处理，不代表原记录合并或自动解决 |
| `needs_source` | bool，可省略 | true表示判断或处理本问题还需本请求外的原文；false表示现有原文足够，后续应核对/修正已有事项或作明确裁决；缺省为未判断，不能当false |

detail继续说明具体缺什么、为什么需要或不需要补读；不另增reason、处理类型或问题状态。

**续报只合并任务，保留现有Issue记录。** 每条finding仍由现有assign_issues机制赋予稳定
身份，保留原回执、位置和原值；不另建Observation类型、身份体系或版本表。related_issue_ids
只把合法续报和输入问题组织为共同待审任务，不能按同一范围或相似文字合并。同一finding
只陈述所关联问题的续报，另一个独立疑点另报；模型未提及旧问题不表示已解决。
有效关联可连接多个原问题，任务加载这些记录的全部有效来源/事项引用。未知或越界关联
保留诊断，finding本身仍保留；路由按未判断处理，不用错误关联隐藏疑点或拒收其他有效内容。

Decision仍只解决issue_ids明确列出的记录，不顺着related_issue_ids传播。后续associate/joint
带齐该共同任务的相关记录及完整原文，使用已有payload摘要和回执绑定；新增或遗漏的记录
不会随旧决议自动清除。joint中的新阻断意见沿用现有拒绝逻辑，其他任务的后续finding仍
保留为未决，不自动重开循环。需要一次解决原问题及续报时，模型在Decision列出实际解决的
各issue_id；不能要求它无条件照抄整组。相同回执重放保持同样记录、顺序、计划及摘要。

**调度只做一次，按以下顺序执行。** 原文review结束后冻结问题记录集合，由宿主沿合法
续报引用形成共同任务；每组保留所有记录，不推断它们是同一个业务事实。

| 该任务的输入情况 | 下一步 | 保留结果 |
| --- | --- | --- |
| 沿用现有策略已是非阻断提示，且没有明确needs_source=true | 展示提示，不为关闭提示额外运行reference或associate/joint | 非阻断原记录继续保留，不标作已解决 |
| 有明确needs_source=true | 一次reference定位 | 读取候选完整原文后进入associate/joint，找不到则保留具体未决 |
| 有明确false、没有true，且存在阻断问题 | 用已绑定的原文组织本地associate/joint，不调用reference | 模型提出解释决议或指出需显式修订；不自动改变原事项 |
| 均未判断，或关联声明无法绑定，且不是上述提示 | 一次reference兜底 | 一轮后未决仍保留，不递归定位 |

只有整组均为现有策略允许的非阻断提示才走提示分支，不放宽blocking规则或增加code白名单。
true优先是保守调度，不是业务裁判。false只表示本请求原文足够，不能仅凭外部公告/附件
未出现在本包就宣称全文缺失；未核对相关完整来源时应保留补读需求。宿主已有证据确认
必须修订的来源完整性/范围问题保留原修订入口，不能借由本地决议解除既有保护。

false任务复用现有候选容器和装载器，绑定Issue及续报明确引用的来源/事项与必要上下文，
不复制整份历史请求，不扩大为整个文档；没有可用定位时展示具体未决原因。模型提出语义
决议，独立joint作接受判断；须改事项/补源的仍走既有修订入口，不增加自动修正循环。
本地候选和locate候选都收齐后统一分组：只因共同问题记录及合法续报依赖而共同核对，
不因共享章节合并。所有必要候选仍加载完整原文，不能以某一候选已足够为由裁掉其他候选。
locate、associate或joint后产生的finding保留；规划后新关联不能追溯改写已冻结计划或启动
第二轮补读。全台账discovery独立保留，不因issues为空或上述路由而跳过跨组关系发现。

宿主核验产物`problem_routing`记录每组`{issue_ids,route,reason}`，用于解释实际调度，
不进入模型公共Issue或RunStatus。route为reference/local/note/revision/unresolved之一；
不代表业务结论。reference批次通常最多8条记录，共同问题组不为凑数而截断，仍需通过
完整请求准入。关联声明无效时保留原值诊断、移除该声明及路由判断，保留finding本身兜底。

**压缩与合批分步实施。** 首批保留完整事项、重点原文和现有跨组发现范围，先消除误报及
同一问题重复调度。预算校准与此并行；供应商模型/tokenizer未核实时沿用保守估算，不能
固定除以样本比例。输入、输出预留和实际总上下文共同核对；不增加累计费用硬门限。
按不同操作记录输出分布、样本量和缺失usage；截断或失败输出不能作为完整输出容量样本。
扩大输入包前必须先校准相应输出余量并重做完整请求准入；保守输入估算暂时抵消输出低估
不构成安全依据。没有足够输出证据时保持现有包规模，不把历史最大值当作上限保证。
校准先复用已有真实回执，按同一协议/模型配置列出样本数、输入/输出分布、最大总上下文和
失败/未知用量；未经核实的模型名不强套tokenizer。候选预留值及分包方案先离线重规划，
再将候选配置冻结到独立验证执行，用真实样本核对完整输出与语义质量，通过后用于后续执行。
失败表明容量不合适时缩小
新执行的工作包，不截掉已收到输出，也不改在途摘要。此项不增加模型字段或环境变量。

累计预算分为“当前已确定计划”“尚未生成的下游计划”和“已发生用量”，不得把未知的
associate/joint记为零或把locate总数称为全程上限。候选生成后在发送前更新下游计划；
成本比较包含本地处理、纠正/失败调用及未知用量，不只计算省掉的reference次数。

discovery延后evidence.quote仅为后续对照实验：其余事项字段、文档角色、章节及证据范围
不变，associate/review继续装载完整原文。315事项离线从21包变10包不证明语义召回，
投影的正式字段与新Prompt需在实验通过后再定，当前不将其当作TenderItem的可选evidence。
不裁剪text、不以模型摘要替换重要条件，也不直接取消跨组检查或承诺一次全局发现。

同一问题的共同依赖与一次调用的合批边界分开理解，但首批不改associate/joint的接受粒度。
当前任一阻断finding会阻止该joint包全部决议接受，不能直接把独立问题合批；后续只有在
每组有独立、可追溯的接受结果且共享依赖的反对意见均被处理后，才评估该优化。
同一来源可以共用，不等于业务问题相同；不可分候选的完整证据仍不能截断。

**执行与验证。** 新字段及提示必须同步schema、短引用往返、拒收反馈、预算测量、语义导出
和冻结协议标识；旧运行维持原语义。无新增的自动语义修正循环；结构错误仍用原失败预算
和真实反馈纠正，业务修订使旧确认失效。统计全链路review/locate/associate/joint及重试用量，
不只比较locate包数。具体验收见[验收标准](acceptance.md#ab专项验收合同已验证真实语义待验)。

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
| `operations` | 按宿主请求元数据区分extract/review/discovery/reference/associate/joint/ocr；旧verify缺元数据时归verify_unattributed，不猜归属 |
| `reported_tokens` | 各有报告的prompt_tokens/completion_tokens/total_tokens之和；完全未知的字段省略 |
| `missing_token_fields` | 每个Token字段未报告的请求数，部分usage分别计算 |
| `completion` | observations、max、p95（最近秩）、reserve_comparisons、exceeds_reserve；无观测时max/p95为null |
| `successful_completion` | 仅传输succeeded的输出样本数、max、p95；排除失败/uncertain，但不代表语义已通过，不单独作为容量保证 |
| `context` | observations、max_reported_total、capacity_comparisons、exceeds_context；只比较实际报告total与该请求记录的容量，无观测时最大值为null |
| `plans` | 已生成的extract/review/locate/associate/joint计划；各phase的planned_requests、input_estimate、max_input_estimate、output_reserve_total、over_budget_requests；全为计划统计，不能与实际Token相加 |
| `planning` | unplanned_phases列出尚未生成的阶段；known_planned_requests只计已知计划；有未知阶段时total_planned_requests为null。此计数不含OCR、重试和纠正，不能充当全程付费调用上限 |
| `deviations` | 逐请求标记，不含原文。`estimate_below_prompt`：实际 prompt_tokens 大于记录的输入估算；`completion_above_reserve`：completion 大于该请求 output_reserve；`slow_first_data`：首个原始字节等待达到 60 秒。字节估算偏高、首包更快、缺字段都不标记。标记不改变 131072 默认窗口、262144 纠错上限、预留或重试 |
| `limits` | 估算、未知用量及小样本分位数的解释限制 |

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


### 模型发送协议修订：短引用与拒收纠正

本修订先在源码及隔离测试实现，部署前不改变在途运行的冻结协议。
主节点、领域schema_version=2及持久事项身份不变；执行快照和预算builder标识发送协议。

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
  模型恢复入口统一执行工作包的首次加三次重试；普通重发与反馈纠正共用同一持久额度，
  不叠加SDK或队列模型重试。反馈记录真实的rejection_count；相同合同错误再次出现也
  使用剩余额度纠正，不能反复读取同一成功坏缓存。完整DONE/stop后的JSON语法错误也进入反馈，错误位置及附近脱敏文本
  合计不超过512字符；原回包保持不变，不以截前缀或删除符号作为接受依据。
- 响应纠正只处理尚未接受的工作包合同，不修改已接受事项或自动解除业务问题。
  uncertain允许在同一额度内重发，原回执及未知用量保留，可能重复计费；仍在途的sent
  阻止并发发送，租约接管先按既有规则结算为uncertain。不增加状态、环境变量或计数表。

### 失败执行的显式恢复

核验分包、完整请求预算测量等同步规划在工作线程执行，事件循环持续处理租约心跳；
线程调度不改变请求内容、摘要或回执身份，也不扩大上下文预算。

模型工作单元的所有失败默认首次加最多3次重试，包括连接、服务拒绝、配置、JSON、
宿主校验、缓存不可验证及响应结果不确定；恢复入口
统一调度，现有请求台账以`command_id`和`logical_request_digest`持久计次。逻辑摘要移除
顶层contract_feedback，保留其余完整请求身份；完整request_digest仍用于缓存和付费围栏。
Store在转sent的同一事务检查已发送意图次数，最多4次；缓存复用不计次，重启不重置，
普通重发及合同纠正共用额度。发送前错误也受入口四次上限约束。单元耗尽明确failed，
队列不再叠加模型重试；成功子产物不重做。发布、数据库、普通代码异常和执行器丢失保留
同命令四次外层兜底，历史SQL字段`infrastructure_failures`仅用于该执行计数，不新增状态、
配置项或计数表。过期执行由接管方结算一次失租失败；重复投递不重复计数。
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
恢复命令不代表独立核验或人工确认完成；新command给予失败工作单元新的四次额度，
合同纠正仍与该单元普通重发共享上限。所有反馈摘要共用该command逻辑单元额度，
不能通过修改反馈摘要绕过；未知付费尝试仍完整计入请求台账。

### 最小事项与复合条款示例

下例不需要 subject、numbers 或任意业务细节对象即可表达一个普通要求：

```json
{"item_id":"p1:i1","kind":"obligation","text":"保修期为三年。","evidence":[{"range_id":"r1","quote":"保修期为三年"}]}
```

复合来源 r2 为“负责人须持A证；持B证加2分；未附A证复印件则否决投标。”，同段四类事项共享来源：

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

## 核验职责与共读（最小修改已实现）

三轮评审后的[当前方案](bug-fix.md#核验职责与共读修复方案最小修改已实现)沿用现有字段，
不新增groups/group_results、Issue/Decision字段或第二套任务身份。此前的分组包装设想
暂缓，不是待直接实施的协议；当前v2回包结构保持不变。职责与调度修改引入于冻结执行
版本7，当前连同discovery关系供给使用版本8，
旧版本运行保持原合同，不热切换；部署和真实模型验证状态以交接为准。

| 现有字段或合同 | 最小方案 |
| --- | --- |
| source review的dispositions | 不变，仍按目标范围检查全源漏项 |
| joint的dispositions | 仅在发送投影覆盖为[]，停止再次发起通用查漏，不修改全局台账处置 |
| joint的source_views/items/relations/candidates/decisions/issues及scope_decisions | 完整保留原单组输入及关联产物，不裁原文、条件、例外、对象或旧问题 |
| 核验输出 | 继续schema_version、accepted_decision_ids、findings；不新增结果包装或覆盖名单 |
| 接受与人工出口 | 保留原整组blocking、protected、补遗冲突及确认规则，不增加运行门禁 |

现有verify Prompt已区分“原文支持下的语义核验”和“dispositions目标查漏”；review
职责指令未改，locate已补充已有关系和证据引用说明，因此共享Prompt摘要已改变。
joint空列表是否足够仍待定向真实调用。即使无通用查漏目标，仍须发现影响决议的条件、
例外、数值归属、证明边界和补遗保留规则错误。新观察正常保留，不能靠压制finding通过。
旧问题不因未被再次提及而解决；续报仍用现有related_issue_ids。protected仅限制自动
决议，不把原有human类问题改成硬阻断。同组多决议尚无精细部分接受，明确保留该局限。

共读仅是待证明收益的备选：未显式引用的上下文也可能影响结论，不能让associate先读
共享并集、joint再仅按引文裁回旧组来源。首批试验须保留associate实际可见公共证据及
必要对象；若因此超预算或抵消收益，不采用该合批。后续选择性缩小依赖需独立论证，
不能从缺少quote推定无依赖，也不能提前给备选字段赋予正式协议含义。
调用公式、预算、修订及重试范围见方案；实际状态和观察值只在交接维护。

## Python编制输入（T0/T1）

后续编制先在Python独立执行，业务范围及任务顺序见[编制任务规划](implementation.md#python投标编制闭环任务规划独立于一期)。
本轮先固化编制输入；大纲、模板、响应位置、知识资料及正文的新增字段在对应任务编码前
继续在本字典定义，不提前为尚未验证的用例建立完整字段体系。原五节点、模型回包和确认
协议保持原义，不复用历史v1响应计划。下列输入由宿主产生，不发送给LLM要求其回填。

以下为本次实施合同。编制固定当前可读资料，不要求分析已确认；分析五节点的
核验、确认与补遗接受语义不变。入口、上下文、恢复、心跳和发布一起遵守此边界。

### AuthoringInput

| 字段 | 类型 | 含义 |
| --- | --- | --- |
| `schema_version` | 整数2 | 统一合同版本 |
| `run_id`、`source_version`、`domain_revision` | 沿用RunIdentity | 本次消费的分析版本 |
| `checklist_digest` | Digest或null | 实际存在的分析摘要，未形成时为空 |
| `frozen_ref`、`pack_plan_ref`、`items_ref`、`verification_ref`、`confirmation_ref` | ArtifactRef或null | 当前可读的来源、计划、抽取索引、核验和确认产物；缺失/不可读不伪造 |
| `extract_refs` | ArtifactRef[]，默认空 | 抽取根未形成时已完成子包的固定引用 |
| `issues` | AuthoringInputIssue[]，默认空 | `{code:Text,detail:Text}`，当前资料可用性说明，不增加blocking字段 |

`read_authoring_input`开始时核对请求的来源/领域版本；checklist_digest可省略。
对当前修订可读根及部分抽取回执建立不可变输入。原文有而分析缺少时仍可组织/写作，
来源全部缺少时保留待补位置，不虚构招标要求。损坏引用不能充当有效来源。
稳定execution_id恢复时沿用已固定输入，不重新抓取或静默替换；后续分析修订不使旧输入
作废，旧产物继续标记真实版本。执行期间保留取消、独立租约及存储围栏检查；不重复
以确认、当前清单或业务缺口作为发布门禁。新资料用新执行版本生成，保留旧产物。

### 按需装载与事实保留

- 核验产物及其依赖可读时，有效事项复用`current_items`。其既有条目为`{item_ref,item,decision_ids?}`或
  `{item_ref,decision_id,effective_rule}`；二者均须保留。effective_rule只携带已接受决议明确
  保留的text/scope/evidence，不继承原项的numbers/scoring等可选值，不重造item_id。
  核验投影不可读时使用已有抽取事项，并明确标记未核验及补遗待核；不能把原抽取当作有效投影。
- 关系从verification的relations_ref读取，并保留unverified_relation_ids；决议从decisions_ref
  读取，并保留accepted_decision_ids。只有既有接受规则决定补遗投影，读取和人工确认不改变核验状态。
- 问题保留issues/all_issues及确认中的issue_resolutions；acknowledged表示知悉并保留，
  dismissed为人工驳回，二者都不改写原问题或变成模型已解决。
- 原文和表格复用EvidenceRepository。evidence.range_id可能是片段身份，须沿pack_plan的
  body/supporting selector装载原区间及定位；不能直接用片段ID索引原生范围，也不能只把
  原文引文当成完整项目背景。此输入不复制正文、事项、运维命令或模型费用流水。

本轮读取新增模型调用为0、模型输入输出为0；调用次数不随文件规模增长。宿主只读工作
随现有索引及所需产物规模增加。来源或领域修订沿既有重验/确认规则处理，读取本身可重做
且不消耗付费重试；后续生成的重跑量按实际依赖确定。模型无需誊写任何本输入字段。

## Python大纲与响应位置（T2）

可注入执行端的Python内部领域函数`organize_outline`读取T1可用快照或消费执行根已固定的输入，
装载已有事项、可读有效投影及必要原文，向执行端交付完整请求和回包校验函数，保存CAS大纲产物。
不接旧v1 Plan，不修改分析Task、提示快照或五节点；领域函数不持有付费执行及恢复上下文，
由下节独立执行入口负责，不能借用已完成分析的租约。执行端负责完整请求发送及首次加三次的统一
失败/纠正重试；本函数不再套一层重试。测试执行端不能替代真实持久执行验收。

### 持久引用与产物

| 结构 | 字段 | 含义 |
| --- | --- | --- |
| EffectiveItemRef | item_ref: ItemRef；decision_id?: Identity；rule_index?: NonnegativeInt | 原项直接引用；局部保留分支同时提供decision_id和retained_rules的零起始rule_index，不复制规则正文；同一决议可保留多条规则 |
| SourceUnitRef | source_index_ref: ArtifactRef；unit_id: Identity | 指向冻结原生unit，含完整固定文字、原表格grid及定位；document_id、原件和raw_ref由索引读取，不重复保存 |
| ResponseTarget | target_id: Identity；kind: narrative/form/attachment；purpose: Text；rule_refs: EffectiveItemRef[]；source_refs: SourceUnitRef[]；manual_fill: bool=false | 正文、规定格式/表格或证明附件位置。manual_fill为true时保留原表/字段由人工填写最终值，宿主不对该位置请求正文；不是审批门禁。同一规则可对应多个位置，评分规则也用同一引用 |
| OutlineChapter | chapter_id: Identity；title: Text；targets: ResponseTarget[]；children: OutlineChapter[] | 有序章节树。章节和位置身份由宿主按路径产生，不要求模型生成或回抄；分组标题可没有targets |
| OutlineIssue | code: Text；detail: Text；rule_refs: EffectiveItemRef[]；target_id?: Identity | 具体待处理事项；不新增blocking状态或再次确认流程，语义疑点、未分配规则及模板定位缺口都不能因保存大纲而视为解决 |
| AuthoringOutline | schema_version: 整数2；authoring_input: AuthoringInput；chapters: OutlineChapter[]；issues: OutlineIssue[] | 绑定本次固定可用快照，未确认状态不因生成大纲而改变。原分析问题、人工意见、关系核验状态继续从输入引用读取，不复制或改写台账 |

上述字段除`?`或已注明默认值外均必填；数组可以为空。空rule_refs的位置可由真实
source_refs/template_refs或已解析的纯参见支持；确实没有依据时保留具体未绑定问题，不伪造要求。
局部保留引用不继承旧项scoring等字段。EffectiveItemRef未提供的decision_id/rule_index在
持久化时必须省略，不能序列化成null；已提供的局部保留索引0必须保留。大纲、写作任务
及CAS产物须可反复validate/dump/load往返，不以放宽下游null校验掩盖生产端序列化错误。章节/位置身份只在本产物内有效，不能跨大纲修订按
同一路径假定同一内容；持久消费须绑定产物CAS。源表格按原unit装载，不由模型重写后保存。
空白页等没有原生unit的虚拟范围不作为可选模板来源。XLSX的行范围按table_id/table_ids
绑定真实表格unit；同一物理行横跨多表时保留全部原表引用，不用table_row载体冒充完整grid。
若该原生行还有选中表格区域之外的单元格，同时保留该行引用，避免丢失同行说明或公式；
完全由原表覆盖的行不额外保存行引用，重复选择仍按原生unit去重。

`load_outline_source`是宿主只读装载函数，返回`document`及原样`unit`，按需补齐下列冻结依赖；
这些是读取结果，不新增SourceUnitRef或大纲产物字段，不发给模型誊写：

| 返回字段 | 含义 |
| --- | --- |
| ocr? | 该图像unit的完整冻结OCR记录，包含text/status/issues及已有disposition；不以空unit.text替代OCR，不重新识别 |
| image_ref? | unit.locator.image_key对应的冻结图片ArtifactRef，不复制图片内容 |
| image_anchor? | 该unit已有的冻结图片锚点，保持原坐标及偏移 |
| spreadsheet_cells? | unit.locator.cell_refs所指单元格的映射；在副本中按grid_cell_ref恢复grid_value_fields，保留公式、缓存、显示及类型元数据，不重新计算公式。原生locator.cells未压缩时仍随unit原样返回 |

grid与row使用相同的单元格依赖装载；原grid、合并范围、原生unit及CAS均不修改。
原生Excel行的locator.merged_ranges保留与该行物理矩形相交的完整原始范围；跨行或
超出非空单元格边界时仍保留完整坐标，不裁剪、不扩张行region、不复制主单元格文本。
定义表格grid仍只接收完整包含的合并范围，保持原有稀疏铺满约束。冻结压缩及装载均保留
上述坐标；无原生正文行的纯空白表外合并仍以原件为准，本批不承诺完整Excel布局重建。
此处恢复宿主结构，不新增模型merged_ranges字段，也不宣称模型已读到表外合并关系。
不存在的OCR/图片/锚点保持缺省，不伪造已完成；原分析问题继续沿原确认和后续核对出口处理。
底层装载器以ArtifactIntegrityError报告损坏；编制上下文记录不可读来源及待补位置，其他可用资料继续。
不得把损坏引用当有效来源，也不要求模型修复存储引用。
本次装载修复新增模型调用为0，T2仍为正常1次、同执行最多4次；请求语义视图、输入输出
结构、预算及超限出口沿用原合同。目录可能补充含表外格的行锚点，最坏仍为O(S)目录，
原文不重复发送；新增row引用由宿主保存，不增加模型回抄。索引读取沿用既有全文件
完整性核对；返回结果只补齐所选unit的OCR、
图片引用和单元格，不复制全文件raw，依赖展开随相关单元格及原grid线性增长。
修订依旧重做T1及T2，纯重新装载不重跑付费调用。

BUG-038的原生解析修复新增模型调用同样为0：全文件调用数C_new=C_existing，随文件规模
增长的模型分包公式、单次最坏输入/输出及预算出口均不改变，不要求模型回抄定位数据。
每行仍扫描既有合并集合，最坏O(原生行数×合并数)，仅冻结中的相交定位元数据增加。
原生parser_version按源码摘要变化，新解析生成新CAS；旧冻结及大纲可读但不会自动补齐。
已有运行继续读取原产物；如需补齐旧文件，须沿既有来源修订重新冻结、分包和分析确认，
模型调用量为该修订原有分析调用数及后续T2最多4次，不热换旧快照或重置失败额度。
不新增问题类型或状态，结构超限/解析失败继续走原技术重试及来源修订出口。

### 模型发送与回包

发送`schema_version=2、rules、rule_origin、relations、unverified_relation_ids、issues、issue_resolutions、scope_decisions、source_views、source_catalog`。
`rule_origin`由宿主填`verified_projection/extraction/source_only`，分别表示使用核验投影、未核验抽取或仅来源；
只描述本次发送资料的来源，不新增运行状态、不要求模型回抄，也不表示响应已满足招标要求。
关系的既有未核验身份随输入保留，不把候选关系当成已确认语义事实。
每条rules使用宿主rule_id，加`item`或`effective_rule`以及原item_id关联关系；只发送语义值，
不发送CAS、确认摘要、版本、原件路径及费用流水。规则来源及相关关系/问题所需原文按
完整原生unit装载，同一unit只发送一次，保留既有表格明文、空行压缩和必要supporting；
不重新抽取全文件。其他来源仅发送可用性目录，目录不是已阅读原文。

大纲使用独立outline提示，短引用继续由WirePayload映射，unverified_relation_ids与关系
身份使用相同映射；新增rule_id/rule_ids和关系中的
from_rule_ids/to_rule_ids；分析原有映射及默认Task提示集合保持不变。只有模型需要选择的
语义引用才回传，不要求覆盖名单、宿主身份或评分数值回抄。

回包`OutlineResponse{schema_version:2, chapters, findings}`：chapters递归使用
`{title,targets,children}`；targets为`{kind,purpose,rule_ids,range_ids,manual_fill?}`；manual_fill
默认false；findings为`{code,detail,rule_ids}`。其余字段必填，列表允许为空。range_ids可以选择已读原文或
目录中的锚点，宿主映射至完整SourceUnitRef并去重。目录来源尚未读入模型时保留具体
`outline_source_not_read`问题，后续生成前须读取核对，不宣称模板含义已验证。
`scope_bindings`及`pending_scope_context`内的可见锚点同样经原selector映射；正文去重
不能丢掉引用身份。仅有pending元数据仍不算已阅读，选择时沿用同一未读问题出口。

未知引用或无效结构通过ModelContractError交执行端纠正；完全重复的选择由宿主去重。
未分配规则派生`outline_rule_unassigned`，无规则绑定的位置派生`outline_target_unbound`，
form没有源模板派生`outline_template_missing`；这些问题随产物保留，其他位置继续，
由用户或后续编制核对补齐。模型语义findings原样保留，不以标题或映射存在宣称已满足。
人工保留的原问题及未核验关系仍保持原状态，来源错误沿现有分析修订出口处理。

### 本批容量及修订边界

设有效规则R、原生范围S、实际响应位置T。全文件T2在完整输入能准入时为1次组织请求，
领域函数不按事项/候选发请求，也不做两两组合；执行端最多首次加3次，共不超过4次。
输入规模为O(R+相关关系/问题+S目录+被装载原文)，输出为O(章节+T+映射数+findings)，
一规则多位置使映射数可能大于R，不承诺输出恒定或默认16384足够。

预算由调用方显式传入；按独立outline提示、完整短引用请求及显式正数output_reserve准入，
不默认套用核验tokenizer或输出预留，也不设供应商输出硬上限。窗口及输入上限沿RequestBudget；
实际输出仍需样例校准。不可分完整请求超限时发送0次，返回既有ModelBudgetError，
调用方须使用经核实的容量重新规划，不能截断要求、目录或原文来通过，也不声称已支持超限
全文件分批合成。纠正请求新增反馈后的预算须由执行端再次检查。

同修订同完整请求的成功复用由下节独立执行上下文负责，领域函数的CAS产物不等于付费缓存。
分析修订后重做T1、组织请求及其受影响下游，组织最多4次；纯样式修改不触发分析。
运行中分析推进或修订不替换已固定输入；返回产物明确绑定旧快照，只检查取消及独立执行围栏。
需要新资料时建立新执行，不把旧产物伪称最新；领域函数本身不持有长期租约。

### T2独立持久执行

Python内部入口`execute_outline(analysis_store, artifacts, config, budget, *, execution_id,
run_id, source_version, domain_revision, checklist_digest=None)`调用上述组织函数；execution_id由
宿主显式固定，相同身份恢复同一执行，不自动生成新身份绕开失败额度。不新增CLI或队列节点。
本批恢复由调用方以同execution_id再次调用；尚未接dispatcher，不代表进程退出后无人值守续跑。
使用独立AuthoringStore和AuthoringLease，不领取或更改父分析运行的租约、状态及确认。

| 结构 | 字段与含义 |
| --- | --- |
| AuthoringLease | execution_id、epoch；只授权当前编制执行，epoch单调递增，失效后不可发送或发布 |
| 编制执行根 | execution_id；authoring_input: AuthoringInput；execution_spec: 非秘密配置；status: pending/running/succeeded/failed/cancelled；epoch、lease_until、cancel_requested；result_ref?: ArtifactRef；receipts: 小型宿主回执列表；error?: 错误类型；created_at、updated_at |
| 编制请求回执 | request_id、execution_id、epoch、model、task=outline/authoring_basis/authoring_write/authoring_review；status沿prepared/sent/succeeded/failed/uncertain；metadata含request_digest、logical_request_digest及预算测量；usage?、response_ref?、error?、created_at、sent_at?、finished_at? |
| 编制宿主回执 | stage、scope、input_digest、artifact_ref、epoch、metadata（宿主非秘密诊断）；只保存完整请求及最多四次拒收等少量CAS引用，不复制请求正文 |

字段沿用已有含义。执行状态只描述内部执行生命周期，不增加业务审批状态。PG只新增
`tender_authoring_executions`和`tender_authoring_model_requests`两表，完整请求、响应、
反馈及结果仍存CAS。执行配置固定独立协议、规范Chat地址/模型/窗口、实际outline提示摘要、
实际准入预算及超时策略，不包含密钥、密钥摘要或默认分析提示集合。相同execution_id的
指定不同来源/领域身份或非秘密配置须显式报冲突；恢复优先取已有固定输入，
不会因父分析新增产物替换输入或清空回执。轮换密钥不改变请求身份。
首次建表使用当前schema内的PG事务级advisory lock串行初始化，避免并发启动时
CREATE TABLE IF NOT EXISTS仍出现系统目录唯一冲突；不锁父分析或其他schema的执行。

共享客户端新增完整请求发送入口，原分析call继续原有构造/短引用/预算再委托；编制只用
Chat及显式预算，不扩分析Task，不继承VLM或verify tokenizer。拒收恢复复用既有
call_validated，编制拒收元数据写execution_id/epoch，不伪造分析command_id。
contract_feedback放入用户消息JSON，并对纠正后的完整请求重新准入；逻辑摘要固定为原始
完整请求，具体发送摘要包括反馈。短引用只在组织边界解码一次，模型不回抄执行身份。
同execution_id可容纳上述四种语义任务及多个完整逻辑单元；在途sent、成功缓存和四次额度
按task+logical_request_digest隔离。反馈不改变逻辑摘要，不能借改反馈重置额度。
同逻辑单元已prepared的并行副本在另一份成功后也不能再次发送，独立单元可同时推进。

正常全文件仍1次组织调用，失败共用首次加3次，上限为同execution_id整个逻辑单元4次
真实发送，跨进程恢复不重置；prepared、缓存读取及预算拒发不计付费发送。最坏输入为
完整原请求加具体反馈，并须再次符合窗口；输出沿上节显式预留，尚未实测的容量不假定。
失败回包保留真实反馈，成功传输缓存不等于领域接受；完整成功响应从校验CAS恢复后重新
核对领域合同，不能删除缓存重付费。最终产物成功时直接复用同一结果引用。

发送前原子检查独立租约、取消、全部逻辑尝试、在途sent及四次额度；变更反馈不能绕过
在途围栏。过期接管将旧sent记uncertain、旧prepared记failed，保留实际或未知用量；
uncertain按用户已接受的重复计费风险在剩余额度内重试，不承诺远端exactly-once。
所有调用失败共用额度；取消、失租立即退出，数据库不可用由调用方恢复，不用新付费调用
修数据库。预算/配置不满足明确返回，保留成功产物；耗尽后保留全部回执和具体错误，
用户可修输入或配置后显式发起新执行，不自动重新跑分析。消费新分析修订时建立新快照及
受影响组织请求；在途旧执行继续绑定原快照。样式不变更父分析；发布检查取消及独立租约。
工作只负责组织和关闭模型客户端，关闭期间保持心跳；工作完成后须停止并等待心跳，
再由原事务核对父分析/编制的取消标记及独立租约并发布成功，发布后直接返回，不留下心跳竞争窗口。
心跳实际错误、客户端关闭失败及调用方取消不发布成功；等待子任务取消不能吞掉调用方取消。
成功模型回执仍保留，同ID恢复复用；此顺序不改变调用次数、请求预算或修订重跑范围。

## 编制依据与交付约束（T3至T5业务语义）

本节说明业务语义，正式结构统一见下节Python编制执行合同；职责与预算见
[编制架构草案](authoring-architecture.md)。字段先在本字典定义，再同步schema、提示和执行链；
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

## Python编制执行合同（T3至T5）

以下保留knowledge_required_v1历史基线；BUG-047之后的当前统一合同定义在
[编制最小修订](#编制最小修订knowledge_required_v2)及后续提交结构字段中。
源码和真实验收状态见交接，不能将下面v1的删稿策略或旧字段限制当作v2要求。

本节先定义本次实施所用的最小字段；实际运行和验收状态仅在交接维护。
不增加分析节点，不复制招标事实台账。所有模型回包中的ID仅为所需的短引用选择，
宿主产生执行、章节、任务、资料及文件身份。

| 结构 | 字段与含义 |
| --- | --- |
| OutlineAssemblyTarget | `kind:narrative/form/attachment`、`purpose:Text`、`target_ids:Identity[]`且至少一项；选择本请求宿主提供的短候选位置引用，将语义重复的跨批响应合并为一个实际位置。模型给出合并后的用途和模式，不回抄原规则/来源/CAS或持久身份 |
| OutlineAssemblyChapter / OutlineAssemblyResponse | 节点含`title:Text`、`targets:OutlineAssemblyTarget[]=[]`、`is_volume:bool=false`、`children:OutlineAssemblyChapter[]=[]`；响应仅为`{chapters:OutlineAssemblyChapter[]}`。宿主对每个合并目标联合去重原rule_refs/source_refs，保留原模板依赖，并统一生成正式章节/位置ID；任一候选manual_fill=true则合并目标继承true，不能被模型改为自动填价 |
| OutlineTargetResponse.manual_fill / ResponseTarget.manual_fill | `StrictBool=false`；大纲模型按实际语义将最终报价、投标单价、总价、折扣等人工填写位置设为true，宿主原样绑定。保留格式和待填位置，同章其他目标继续；不新增金额结构、审批状态或下载门禁，不扩展为所有技术参数都人工填写 |
| KnowledgeMaterial | `material_id:Identity`、`title:Text`、`text:str`、`content_ref:ArtifactRef或null`、`original_ref:ArtifactRef或null`、`locator:dict`、`usage:fact/reference/proof`、`attachment:bool=false`；locator保留适用target及必要产品/版本/原文件定位。content_ref只指冻结文字片段，original_ref只指实际原件；片段不能冒充证明原件；搜索命中默认reference，不自动证明事实或资格 |
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
| AuthoringReviewFinding / AuthoringReviewResponse | `findings:[{code:Text,detail:Text,task_ids:Identity[]=[],content_ids:Identity[]=[],suggestion:str='',can_revise:bool=false}]`；content_ids选宿主提供的具体block/fill短引用。宿主把跨任务unsupported_content按任务拆分，便于分批再核替换旧发现。明确无依据内容从采用稿移除；无精确内容但关联任务明确时，保守清空相应任务生成应答并保留原稿。仅现有资料足以修正时can_revise为true，缺证书、未知价格或授权保持false；缺项/冲突不设置下载门禁 |
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

### 知识匹配后编写（knowledge_required_v1）

本策略统一进入basis/write/review实际payload及执行spec。招标来源提供背景、要求、
条件和固定格式，不能单独证明投标人已有能力；所有新增应答，包括技术、实施、人员及
服务方法，均须匹配可用知识资料。未匹配留空并记现有gaps，结构和导出照常保留。
用户直接提供并冻结的有效企业资料同样属于可用资料，不要求特定知识服务才可采用。

检索查询携本任务实际要求、适用条件及范围，不只使用章节标题、路径或purpose。
候选视图保留必要文字、可适用target短引用及产品/版本/原文件定位；不发送无关元数据，
也不能省去区分不同型号、版本或主体所需的信息。有功能依据即可写对应功能，型号和
配置可选；不能借用另一目标、产品或版本的材料。无候选目标跳过basis/write，不能
以招标要求或共同依据中的无来源approach补出方案。

共同依据批次只发送候选资料、其关联实际targets及去重的rules，保留判断适用性所需的
要求、条件和范围，不恢复全源反复编方案。只有原件但没有可发送文字的资料不进入
共同依据，也不能被block/fill挂名作为支持；可读真实原件仍可按附件用途选择和导出。

不另建匹配节点或ResponseMatch。write一次完成语义适用性判断及改写，block/fill只
选择本请求的target短引用（如t1）及material引用，宿主绑定正式身份并检查可用范围。
review的content_ids按任务选择宿主提供的b1/f1等正文块/填值别名；跨任务无依据发现
由宿主拆成单任务发现，原始回包保留，不要求模型增加誊写。字段空数组
仅兼容旧产物；新策略下不能凭默认值放行无依据新正文。部分命中只保留有依据内容。
复核使用实际内容短引用；无依据内容移除后留空，不因改稿失败再采用已否定内容。
复核不可用时，该批生成应答暂不采用，保留原稿及回执供恢复，原模板和空位继续导出。
旧策略产物仅作历史证据，不能通过同摘要恢复静默成为本策略的采用稿。

宿主导出包包含主DOCX、明确分册DOCX、真实附件、manifest及独立缺口清单。manifest中
原件按摘要去重，`occurrences:[{material_id,title,chapter_id}]`保留同一原件的不同使用位置；
模型不生成路径或附件摘要。图片/PDF按真实像素或原页插入；其他格式原件随包附送并记录
未嵌入。按已知文字和网格重建不等于保留原字体、分页或完成最终版式验收。

### 调用、预算、恢复与修订

1. 全文件新增调用为`O + B + W + R + C + R改`：O是原文/规则按预算组织及必要归并次数，
   B为有候选资料的共享依据批次，W为有候选资料的完整写作任务数，R为初次实际正文复核批次，C为本轮允许的一次
   定向改稿任务数，R改为改稿后的复核批次。首次加三次重试上界`4*(O+B+W+R+C+R改)`；
   不做任务两两组合、不固定逐要求调用。N个组织包只在N>1时另发一次候选响应位置归并；
   归并超限或失败保留原批次章节与待核顺序，不假称已具备递归归并。共同依据按完整记录
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
   复核意见，仍由既有execute共用首次加三次额度，不另套重试、不补检索或无限续写。
   空结果或改稿失败可保留未被否定的旧成功正文；明确无依据内容先移除，不能回退采用。
   成功替换时旧稿进入superseded_unit_refs。
   改过的任务再核一次，保留initial_review_ref；复核仍有问题继续导出，不开启下一轮自动改稿。
   改稿输入沿用该任务原写作payload，追加previous_content中的blocks、cell_fills、attachments
   和gaps，以及review_findings中的code/detail/suggestion/can_revise。旧填值用本次宿主
   sources映射回source_id，附件只发material_id/title；不发送CAS、任务身份或按出现顺序猜绑定。
   无法绑定原稿所用资料时保留原稿和具体问题，不让改稿静默移除原内容。
   新执行按依赖选择受影响任务及跨执行复用属于后续修订能力，不能由previous_document_ref
   参数存在推定已实现；原文未变不重抽，无法确定依赖时应明确重跑范围。
5. 模型不生成CAS、execution/task/chapter ID，不回抄输入/覆盖清单或计算最终页码。
   只选择来源/资料/所需响应位置的短引用；宿主负责映射、表格坐标、真实附件、文件装配。

### 归并操作的显式Token计量

BUG-081补充：来源组织装包在实际输入准入内预留2048 token纠正空间，实际发送上限
不变；未知引用错误精确到叶字段，不复制整棵章节树。全量归并仅在原发送视图超限时
使用`assembly_view="compact_v1"`，仍保持同一输出合同与宿主引用映射：

- `template_parts`、`submission_sources`、`submission_rules`、`unassigned_rules`由数组
  改为以原`part_id`/`source_id`/`rule_id`为键的对象，值保留该条除重复ID外的全部字段。
- 纯原生网格可用`rows`完整二维明文矩阵表达，行列仍从0开始；`null`表示该坐标没有
  独立原生单元格，`""`表示确实存在的空白格。`merged_cells`每项为
  `[row,column,row_span,col_span]`，只列非1×1合并；`row_count`/`column_count`保留。
  原件列宽属于宿主排版数据，仍在冻结原件内，不参与模型语义判断。带额外单元格属性
  的网格保持完整原结构，不丢公式或其他未知字段。
- 候选省略`is_volume=false`、`manual_fill=false`、`support_origins=["knowledge"]`、
  `template_part_ids=[]`时只应用这些明确默认值，不表示缺失语义。其他字段不裁剪。
- 以上仅是输入投影，不改变持久结构、最终DOCX或输出短引用，不要求模型重抄压缩表。
  若投影后仍超限，保留既有容量缺口及人工编辑/重新编制出口，不能声称归并成功。

`RequestBudget`增加宿主构造参数`tokenized_outline_assembly: bool=false`。开启必须同时显式
配置`tokenizer="o200k_base"`；不能根据模型名称猜测，也不能在正在执行的请求中改配置。
仅`measure_request/require_request(request, task="outline")`且发送payload顶层
`operation="assemble"`时启用该计量。原文组织、依据、写作、改稿及复核仍按既有字节估算；
既有verify tokenizer分支不变。prepared请求的task允许显式字符串，但不扩充分析Task枚举。

开启时按实际消息的o200k token计量，保留512 framing余量、现有安全余量和
`min(32768, context_tokens/4)`输出预留；131072窗口对应输入上限94208。输出预留不是
供应商硬上限或已验证输出能力。关闭时默认spec及计量行为不变；开启时只在
`spec.tokenized.operations`追加`{"outline":["assemble"]}`，`tokenized.tasks`仍为`["verify"]`，
使执行快照能够区分本次操作准入配置。

校准依据保留在私有证据目录`/opt/workspace/kb-tender-evidence/2026-10-26-authoring-implementation/`：
`offline-budget-calibration-20261027T045351Z.json`对15个已完成、无图片的真实请求，逐个用
保存的请求CAS对照供应商prompt usage；样本中的供应商prompt等于o200k正文计数加10，
加512余量后的估算均高502。`real-assembly-o200k-preflight.json`显示完整归并在相同计量
规则下可准入。该证据只支持本次端点/模型的窄操作显式启用，不证明所有模型tokenizer
身份、图片费用或归并输出容量，也不将离线预审写成真实归并完成。

本调整的五项边界：

1. 新增模型调用为0；原文N包不变，N>1仍最多增加一次完整候选归并，不先新增树形多轮调用。
2. 单请求按完整Prompt、候选、共享规则及全部必要语义实际计量；输出和安全预留照常扣除，
   不以删除条件、模板或引用换取准入。
3. 未校准/未显式开启继续字节估算；配置无效在发送前明确报错，实际超限仍保留已组织片段
   和容量缺口，不改为无限重试或静默部分归并。
4. 预算配置和Prompt共同固定在执行spec；需调整时使用明确的新执行，保留原调用及回执，
   实际未变请求依既有摘要复用；不得对在途运行临时放宽准入。
5. 不新增模型输出字段，不要求模型誊写计量、候选清单或宿主身份；模型仍只选择目标语义引用。

实际知识服务接口是可注入的窄provider：按任务语义查询，返回冻结文字/原件及定位；
没有配置、无命中或语义不适用返回可读诊断，相关应答留空、固定模板保留，继续导出。
接口和合成回归不代表完成真实知识库
验收，原件下载和适用性核实也不能由搜索片段代替。导出DOCX按冻结文字/网格重建；PDF原件作为真实附件按页嵌入或
随包提供，不能把重建表格说成无损复刻PDF版式。

本轮复核按实际任务内容和共同依据分批；跨批依赖组全局检查、单个巨型
表格的分组填写、输入修订后的依赖选择、最终PDF视觉核对及目录实际页码/图表交叉编号
仍须分别实现和验收。保留这些完整交付目标，不把当前分批复核和DOCX产物冒充全部完成。
金额自动计算不在当前实现目标内；如未来明确需要，再单独讨论输入、人工采用和计算合同。

## 编制最小修订（knowledge_required_v2）

本节是BUG-047至BUG-052的统一后续合同，取代v1中所有内容必须有material_ids、
复核不可用清空应答、只允许generated改稿以及所有空位均复核的设计。
不增加分析节点、Agent、产品确认门禁、环境变量或通用状态机。schema_version仍为2，
knowledge_required_v2仅标识编制交互策略，须进入执行spec和实际模型请求。

### 依据与位置

招标来源支持项目背景、建设现状、要求解释及规定格式；知识/明确企业资料支持产品
功能、实现方法、企业事实和获授权承诺。来源事实不能被改写成投标人已经满足要求。
功能资料可以支持相应功能，无须先确定品牌型号；历史价格仍不得填成本次报价。

| 结构或字段 | 本轮最小修订 |
| --- | --- |
| OutlineTargetResponse / ResponseTarget.support_origins | `list[Literal["tender","knowledge"]]`，1至2项、去重，默认`["knowledge"]`；由已有大纲调用按位置内容选择。背景为`["tender"]`，企业应答为`["knowledge"]`，混合表单可含两者。只决定调度和允许的引用来源，不证明语义正确、不表示资料已匹配 |
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
- U1复核完成：仅按U1意见移除其无依据内容，其他内容采用；仍有问题停止自动改稿。
- 精确否定a1只移除该附件。不能精确定位时只将明确关联任务的生成应答留空，保留
  固定模板、缺口及可追溯原稿；不扩展为全文删除，不固定增加定位调用或阻断导出。

generated/placeholder继续只表示实际内容是否存在，核验完成与否由现有review/gaps
表达；质量、未核对、待补和价格问题不阻断保存/导出。首批优先消费已有可读证明文字，
缺少证据时保留附件位置；不隐含新增OCR/VLM付费调用。若后续需读取原页或新OCR，
另计其实际调用和输出，不凭图片标题判定符合。

### 调用、容量、恢复及实施验收

1. 编制逻辑调用L=`O+B+W+R+C+R2`。O包含现有组织/归并及新增意图输出，B为有知识
   候选的共同依据批次，W为有允许支持的写作单元数，R包含有候选的语义空稿复核，
   C为最多一次定向修订，R2为受影响再核。`R,C,R2≤W`，故`L≤O+B+4W`，付费发送
   沿既有首次加三次上界`4L`。全人工/全无允许支持时`W=R=C=R2=0`，有源文说明
   时仍可能W>0。有候选但全部语义判空时R仍须覆盖，不能借空白省掉核验。检索HTTP
   尝试另计，最多4Q；无命中不重试至命中。后端Embedding/重排可能另有费用，未知
   用量不记免费。不做全章节或全事项两两匹配，必要的独立复核仍读取实际证据。
2. 单请求计完整Prompt、相关要求/条件、去重supports、原表、真实反馈和输出引用。
   短ID不代替必要原文。查询最长256字符，模型输出意图总量与目录/正文一起计量；
   单个请求仍受实际输入准入和预留约束。输出预留不是API硬上限；当前HTTP没有输出
   硬限，不能宣称最坏completion被32768封顶。单命中响应体及巨型单元同样无已验证
   固定上界，按真实样例校准、测量与记录，不将此方案当作容量已验收。
3. 缺资料/候选不适用留空，来源不明或缺检索意图有局部缺口，复核暂不可用保留草稿，
   已否定内容不恢复；不可拆容量不足保留已有稿/空位。取消、失租、存储或实际导出
   失败保留真实技术状态，不伪报文件成功。没有只有阻断没有出口的业务条件。
4. 用户明确重审用新execution继承前驱，选定范围和真实重审理由进入实际review及
   必要修订请求，并固定spec摘要；原写作可复用，空语义回包缓存也不跳过独立复核。
   同ID已完成结果不覆盖，不清缓存、不加盐、不自动换ID重置额度。资料变更重跑
   受影响查询/依据/写作/复核，无法确认知识同版时查询可能扩大；共享依据变化也可
   扩大正文范围，不承诺每次只一章。一次策略升级会改变outline Prompt，旧106页
   样例17份来源请求不能直接作为新意图回包复用；新O数量及费用须重新预审，不能
   宣称新增轮数为0就没有重跑成本。后续未变请求仍按完整摘要复用。
5. 模型只提供语义检索词、需要选择的for_ids/support_ids及正文/局部意见；origin、
   正式身份、关联目标、CAS、版本、覆盖及调用成本由宿主管理。模板结构不由模型
   重建，不重抄全量输入清单、元数据或覆盖名单。

实施与验证状态见交接。真实验收需同时包含真实中文检索、分散片段支持、source-only、
原文背景、混合表单、候选空稿可纠正、初审/再核失败的不同回退、附件a1局部删除、
全无资料零正文复核、单query恢复，以及相同请求复用和知识更新失效；真实知识库
单组纵切通过后再跑完整106页。结果与实际预算仅在交接维护，不提前勾选。

## 提交结构与模板实例（BUG-054）

本节是BUG-054的唯一字段合同。用户已批准实施，字段与装载路径已接通，实际回归、
部署和真实文件验收状态见[交接](handoff.md)，不能仅凭schema通过声明业务结构已验收。
实施后仅覆盖前述T2、T3至T5及knowledge_required_v2中关于局部章节、模板来源、
实例归并和混合报价表的对应定义；knowledge_required_v2的知识支持、复核及恢复合同
继续适用。不得同时沿用“form的全部source_refs都是模板”或“归并时自动联合全部模板”
的旧语义。运行、部署及真实文件验证状态仍只在交接维护。

### 上下文、模板部件与请求短引用

BUG-055补充：编制来源请求的`sections:[{section_id,heading_path}]`按完整路径共享，
每个`sources`条目以`section_id`关联，不再在每个段落重复heading_path；不要求回包
重抄该共享表。宿主原始来源和持久引用不变，实际文字与上下文不按容量裁切。
`RequestBudget.tokenized_authoring`为显式布尔选项，默认false；true必须同时显式
提供已由该模型/目的地实际用量核对的o200k_base，适用任务仅outline、authoring_basis、
authoring_write、authoring_review。计量方法、任务及提示摘要固定进spec。该选项不按
名称自动开启，也不改变分析抽取/核验合同；首次启用及视图变化须重新做输入输出预审。

| 结构或字段 | 类型、必填性及唯一含义 |
| --- | --- |
| SourceUnitRef | 沿用`{source_index_ref:ArtifactRef,unit_id:Identity}`，两字段必填，指向一个冻结原生unit；不因新增模板合同改变其他来源引用 |
| TemplatePartRef | `{source_ref:SourceUnitRef,selectors:RangeSelector[]}`，两字段均必填。一个部件只能属于source_ref指定的冻结原生unit；selectors按选定片段顺序排列。`selectors=[]`仅表示已明确选择整个unit，不表示边界未知、未提供或装载失败 |
| RangeSelector | 复用现有冻结来源选择器，必填`range_id/source_range_id/start/end`，其余已有谱系及作用域字段保持原定义；start/end为原规范范围中的半开区间，不由模型填写、计算或改写，不为本节另建字符偏移协议 |
| 模型请求template_parts | 宿主提供的局部共享表`{part_id:Identity,range_ids:Identity[至少1项]}[]`；part_id使用p1等短引用，range_ids指向本请求实际提供的有序段落/网格锚点及其文字、结构。宿主私有映射保存每个part_id对应的TemplatePartRef，CAS、selector及原生身份不要求模型誊写；原文和grid在请求共享来源中只发送一份 |
| OutlineTargetResponse.range_ids / ResponseTarget.source_refs | 前者仍选择本请求上下文来源，宿主绑定后者`SourceUnitRef[]`。它们用于说明、要求或相关背景，不再因kind=form而获得模板插入权限；选择上下文不能代替选择实际模板 |
| OutlineTargetResponse.rule_ids / ResponseTarget.rule_refs | 选择本位置直接应答或填写的完整有效规则，不能仅因某段引文或主题相同绑定整条异用途规则。有真实来源或模板的引用/展示位置可为空，即使同批已有其他rules；这不表示其他规则已被覆盖或不存在。仅规则、来源及模板均空时报outline_target_unbound，来源未读继续独立报告 |
| OutlineTargetResponse.template_part_ids | 新增`Identity[]=[]`，仅选择本次实际提供并已读的template_parts.part_id，顺序即本候选实际模板的部件顺序。正文、证明附件或无已定位模板时可为空，不要求回抄候选全集 |
| ResponseTarget.template_refs | 新增`TemplatePartRef[]=[]`，由宿主绑定模型选定部件；顺序即该实例实际插入的顺序。不存在时保持空，不从source_refs、标题、kind或临近位置猜补 |
| OutlineAssemblyTarget.template_part_ids | 新增`Identity[]=[]`，选择归并请求重新提供的共享template_parts短引用，明确最终采用部件及顺序；与原target_ids的语义归并选择分开，不按所选候选无条件联合全部template_refs |
| OutlineAssemblyResponse.excluded_targets | 可选`[{target_ids:Identity[至少1项],reason:Text}]`，默认`[]`。模型对原文确定不属于本次交付的候选作明确处置，不要求抄写所有已安排身份；target_ids仅选本次短引用，不得与正式树已选候选重叠。宿主在现有OutlineIssue中保存outline_target_excluded、原用途、理由及关联rule_refs，不渲染被明确排除的目标；未选择且未明确排除的目标继续保留待整理 |
| 归并请求findings | 来源批次的`{code,detail}[]`观察，过滤本来就由宿主全局重算的outline_rule_unassigned；不重新要求模型誊写回包或认为局部观察必然成立。条件、目录差异及本批未找到模板等须结合真实原文核对，不将观察作为执行指令 |
| execute_document.submission_sources | 可选`SourceUnitRef[]`，默认空。调用方显式补充当前固定快照内的只读提交/适用性原文，按完整引用去重后冻结到execution_spec；未知来源记具体缺口并继续。宿主装载这些原文供A/F读取，并加入AuthoringOutline.submission_refs，不能因此猜补模板、声明要求适用或改变分析结果。generate_document及_organize传递同一输入，不作为正文review_request或自动结构修订指令 |
| OutlineResponse.constraint_range_ids | 新增`Identity[]=[]`，选择本批实际提供的来源范围，记录无事项台账时也能识别的提交组成、分册及格式约束；有事项时可与constraint_rule_ids互补，不要求回抄全部来源 |
| AuthoringOutline.submission_refs | 新增`SourceUnitRef[]=[]`，宿主将constraint_range_ids绑定并精确去重为提交约束的只读依据，供assembly及结构核对实际装载。不是模板插入授权，不因名称含submission就创建新实体或正式章节 |

part_id、range_ids及target_ids的短引用只在各自请求内有效；source与assembly各自建立
映射，不将同名p1视为同一个持久部件。持久目标仅保存template_refs及现有正式引用，
不增加持久template_part_id、模板注册表或同义来源字段。归并可以将精确相同部件在
同一实例中的机械重复记录一次，但不能据此合并不同填写实例或替模型选择补遗版本。

宿主绑定TemplatePartRef时须核对source_ref的索引、unit确实存在，并通过同一冻结索引
验证各selector的规范source_range_id、原始start/end边界、range_id及其可回放谱系。
materialize后的unit_id必须与source_ref.unit_id相同，不能因其他unit具有相同文字、
相邻位置或有效selector就跨unit接受。多个selector不得越出其原范围，部件之间的
实际顺序与跨unit组合由模型选择、宿主按映射保存；不重新搜索引文猜出另一处位置。

文字部件通过宿主已有的冻结段落锚点映射精确selectors，允许同一unit内的多份声明
分别选取；一个unit不是一个模板实例。表格部件本轮保留完整原生grid及原坐标、
合并关系，行锚点用于回绑已确认的真实grid，不能将任一被引用的行升级为整个模板。
不能把字符片段当作可裁剪表格结构；同一grid含多份独立格式且尚不能精确保留边界时，
保留对应待补位置及边界缺口，不把整grid复制到各target。无法精确定位的文字也不能
退回复制整个unit；`selectors=[]`只能由明确的整unit选择产生。

### 局部候选、正式章节与填写实例

本轮剩余修复沿用既有实体，新增字段均有默认值，旧冻结产物继续可读：

| 字段 | 最小合同与职责 |
| --- | --- |
| OutlineAssemblyTarget.rule_ids | `Identity[]=[]`，仅选择A的unassigned_rules短引用，补充已有候选或建立有实际规则依据的位置；与target_ids至少一者非空，不要求回抄全部规则 |
| OutlineAssemblyTarget.source_ids | 可选`Identity[]=[]`，只选本次已发送submission_sources中可绑定的短引用，补充本目标实际使用的限定/上下文依据。宿主映射到完整SourceUnitRef并并入现有source_refs，不改变rule_refs归属、规则排除、template_refs或企业应答；不能仅凭source_ids创建无候选/规则/合法参见依据的新目标 |
| submission_sources.bindable | 仅无确定冻结来源映射时显式`false`，原文仍供阅读，不得选入target.source_ids；正常可绑定来源省略true。实际可选集合由宿主映射决定，不根据文本相似度猜来源；未知或不可绑定选择报具体source_ids元素路径，沿既有最多4次请求额度纠正 |
| OutlineAssemblyTarget.target_ids | 默认`[]`，只选已有候选；rule_ids非空，或narrative有非空related_target_ids用于纯参见时，允许不选择原内容候选；不能凭空生成无依据目标 |
| OutlineAssemblyTarget.manual_fill / support_origins | 新规则位置可明确人工填写及允许来源，默认false/knowledge；已有候选的人工边界及来源许可由宿主保留，不能借归并扩大 |
| OutlineAssemblyResponse.constraint_rule_ids | `Identity[]=[]`，只选未安排规则，补充实际提交约束，不因此创建正文章 |
| OutlineAssemblyResponse.excluded_rules | `[{rule_ids:Identity[至少1],reason:Text}]=[]`，仅实际原文足以确认不适用时明确排除；不与已安排规则冲突，宿主保留原规则和理由 |
| OutlineAssemblyResponse.search_intents | 沿用`SearchIntentResponse[]=[]`，仅为新增规则位置补充检索意图，for_ids选择unassigned_rules短引用；宿主绑定规则及实际证据，旧候选的意图不重抄 |
| OutlineAssemblyTarget.related_target_ids | `Identity[]=[]`，选择实际有引用关系的候选；宿主映射到最终唯一实例，多实例歧义保留待核，不能按标题猜测。kind=narrative且本字段非空时可作为纯参见目标唯一的候选依据，target_ids/rule_ids可空；不继承被引用候选的规则、模板或企业内容，不免除原候选的安排/处置 |
| ResponseTarget.related_target_ids | `Identity[]=[]`，保存本大纲内正式target引用，导出据实际章节显示参见位置；不证明应答内容一致 |

纯参见指narrative目标有可解析related_target_ids且无自身rule_refs/source_refs/
template_refs，也非manual_fill。它没有独立写作任务，显示实际参见即可，不误报缺少
企业正文。带自身要求、模板或原文的相关位置仍需正常处理；未知、多实例或自引用
未能解析时保留具体诊断和待补位置，不能因形状相似宣称内容已覆盖。
| OutlineAssemblyChapter.presentation_role / OutlineChapter.presentation_role | `body/cover`，默认body；只有模型依据原格式明确封面才为cover。原来源候选不增加此字段，不按首章或名称推断 |
| OutlineAssemblyResponse.table_headers | `[{part_id:Identity,header_row_count:NonnegativeInt}]=[]`，模型从完整原表判断连续起始表头，仅选择本A提供的完整网格；未知不猜，零表示明确不重复 |
| TemplatePartRef.header_row_count | `NonnegativeInt或null=null`，仅展示信息；显式值需在实际行数内、不能切断跨行合并。无值可用原生header_cells，否则保留未知，不改冻结原表或selector |

unassigned_rules只发送当前未被候选或提交约束安排的完整规则语义和实际证据一次；
短引用与context.rules建立宿主映射。未选择不等于排除，仍保存未安排诊断与全量底稿。
新位置沿实际规则证据绑定source_refs，禁止从标题/关键词决定技术、评分或后果归属。
完整请求过大沿原预算出口保留待整理稿，不截断规则，不新增逐条模型核对。
A的template_parts沿已有来源投影使用section_id，完整heading_path在template_sections
共享一次；模型据共享表理解所属标题和范围，不要求回抄section_id，宿主原部件及定位
保持不变。仅去重重复元数据，不删表格空位、跨行合并、原文字段或规则语义。
heading_path是解析器提供的定位上下文，可能继承错误，不是规定的投标目录；真实目录、
组成段落及适用条款优先。来源S沿constraint_range_ids同时选择结构条款及被引用的
实际目录/组成原文，不能只送“按招标文件要求”一句，不能靠当前文件ID人工补齐生产逻辑。
F的rule_dispositions仅投影outline_rule_unassigned和outline_rule_excluded所关联的
实际规则语义及原引文、code和reason，不发送永久规则身份、CAS或原文位置身份；
已安排规则不重复加进此表。F独立判断是否应安排响应/提交待办或有明确不适用依据，
仍只返回既有findings，未决和无企业资料不阻止导出。
F的supports同样复用来源标题投影：section_id引用本请求sections，完整heading_path
仅发送一次；支持文字、表格及原表引用不变，不要求模型回抄sections。
受约束的响应/填报/引用位置不能只以空title表示；沿已有target保留应答空位，或通过
related_target_ids参见实际对应内容。合并主题不等于可以移动规定位置；没有独立原表
不能造表，纯组织容器仍可只有children，不由宿主把所有空叶章判为失败。
is_volume=true沿现有字段表示该内容组实际导出独立文件，不局限原文的“卷/册”词汇。
真实要求不同文件分别交付/提交时保留对应分组，只有业务部分划分不足以推断独立文件。
全树均为false时只有总稿；外部上传/签章/维护仍为人工待办，导出不代表平台已接收。
要求细则的展示标签由宿主按既有字段产生；结构化首子字段与父字段的展示标签完全
相同时只显示一次，原文字符串、业务值及次序不清洗，完整底稿保持不变。

知识原件沿KnowledgeMaterial已有original_ref、attachment、locator表达，不增加实体：
检索query可带宿主派生的include_original，仅实际证明目标启用；同query合并需要原件的
范围。提供器范围spec包含原件合同版本和认证方式，不含密钥；知识快照身份包含原件
模式，不复用旧片段回执冒充原件已取。真实文档/版本元数据及文件SHA核对成功后才
有original_ref，attachment=true只代表可供选择，不代表证明适用或企业满足。元数据/
下载各首次加三次，失败保留片段及定位缺口；下载大小有限、同源且不跟随重定向。
检索自动取得的原件只在证明目标中采用，避免共享片段使普通正文顺带附送原件；调用方
明确提供的真实证明仍可在相应表单等位置使用，适用性由模型判断，不增加表单禁附门禁。

正式目录使用可更新TOC域、页脚保留PAGE，文档请求更新字段；实际页码由排版器更新，
不按字数估算。封面只在所属输出范围的目录前显示，不加自动章号，不挪动其他卷封面。
表头根据显式展示信息或原生标记重复，列宽按实际可用页宽设置；没有可确认表头时
仍可导出。提交约束和结构待办另交付可读清单，不把平台上传动作变成正文一级章。

语义评测结果单独保留语料/Prompt/模型/输入摘要、请求和原响应引用、用量及未知项、
宿主合同结果；每例结果回执支持恢复。执行完成不等于人工标注或semantic_acceptance，
人工GT、模型观察及人工裁定不混写，不把agent意见标为人工已完成。

source回包继续沿用现有chapters/targets容器，但其章节标题、层级和目标只是本批
局部候选；单批也不能绕过正式组织直接成为最终投标章节。source不生成正式章节/
目标身份。assembly读取实际提交/分册约束、候选父路径、用途、适用范围及判断模板
所必需的真实文字；constraint_rule_refs所指的提交条款须实际进入该输入，短ID不
代替语义。source中的附件标签、表号或模板内标题不自动成为任意层级的编号章节；
一张表也不必对应独立章节。chapter.title表达成稿业务内容，来源格式标识保留在
实际采用的模板内部，不机械前置到正式标题。仅原文明确规定投标目录须使用某名称
时按该要求组织；宿主不按附件字样或编号规则统一删除。最终标题和分册归属以
assembly形成的正式章节树为准。

业务命名不能改变原文明确约束的投标文件组成、层级及相对顺序。assembly先判断
实际提交约束，保留受约束结构，再在允许范围内整理title；仅自由编排部分可按业务
归并。不能以主题覆盖代替规定结构，也不能反向把整个招标书目录当作投标目录。

旧独立`organize_outline/execute_outline`仅为来源候选预览，沿用AuthoringOutline
载体及路径ID不表示正式目录。其结果没有生产写作/导出消费者；完整文档入口
不复用该预览，必须由自身assembly形成正式目录。独立预览保持一次来源请求的
既有执行合同，不将它的成功回执当作正式结构验收。

| 结构或字段 | 本节确定的身份与归属 |
| --- | --- |
| OutlineAssemblyTarget.target_ids | 选择已有候选短引用；与rule_ids至少一者非空，或按上文narrative纯参见合同只选择related_target_ids。用于绑定候选规则、上下文和既有检索关联，所选候选的template_refs不随之自动全部插入 |
| ResponseTarget.target_id | 由宿主产生的本大纲内正式位置身份。对kind=form，一个target就是一次实际填写并提交的完整实例；一个实例可有多个有序template_refs，多份独立填写表单不共用一个target |
| OutlineChapter | 沿用chapter_id、title、targets、children和is_volume；正式树的位置表达提交归属，仅原文明示独立分册时is_volume=true。不另加instance_id、owner_id或volume_id，也不把source标签当提交归属 |
| CellFillResponse.source_id / CellFill.source_ref | 仍使用现有请求短引用与SourceUnitRef回绑。原表必须属于对应target.template_refs中的实际grid；仅属于source_refs、其他target或同任务上下文的表不得填入。坐标沿用该原生grid，不因目录重排改变 |
| OutlineTargetResponse.manual_fill / ResponseTarget.manual_fill | 沿用`StrictBool=false`，作用于整个target。含最终报价、单价、总价、折扣等且没有精确价格字段策略的form整体为true；不另建一个引用同表的价格target。该实例保留原格式、固定数量和技术条件，非价格企业字段也可最终由人工补齐，其他正文目标照常生成 |

不同人员、标段、对象或原文明示分别填写的多份副本，应分别建立target，即使
template_refs相同也保留独立实例。确属同一实例的跨批候选由模型归并，不按来源SHA、
相同标题或相同表格全局去重。一个最终实例只在正式树中实际插入一次；需要提及它
的其他位置不因此再次创建相同填写实例。模型判断是否同一实例、应采用哪个模板及
提交归属，宿主负责身份、selector回绑、实例装载及按正式树导出。

同一章节内多个form实例复用同一模板时，导出须显示各自既有purpose作为用途说明，
以便区分填写对象和适用条件。说明不是新字段或填写事实，不写入原表固定单元格；
宿主只按模板引用检测复用，不从表名猜角色、不生成主体名称。单一实例沿原格式显示。

归并请求的共享template_parts必须包含模型判断实例及适用范围所需的完整内容：文字
部件沿用text，网格部件使用完整原生grid（含所有单元格、合并和原坐标），不以首行
或形状代替后续条件。heading_path取原生unit的实际路径，优先根字段，缺失时读取
locator.heading_path；相同部件只发送一次。该模型输入投影不增加持久模板或主体字段，
也不授权模型选择请求外的部件。完整请求须重新计量，真实预算和修订成本见修复方案。

本轮不新增manual_cells、金额模型或价格词表。manual_fill禁止自动填入最终价格，
不授权宿主按数字/关键词擦除原表。若原件价格格已有示例值，原格式保留不等于本次
报价已经留空，须明确记录人工清理缺口；不得把示例、历史价格或预算当本次最终报价。
模板缺失、边界未知、归属或适用性未决均记录具体位置和原因，相关位置留空待补，
其余内容继续保存导出，不以猜测模板或额外业务门禁替代处理出口。

明确排除仅是模型对本次交付候选的语义结论，不删除原始事项、来源或回执；排除理由
通过既有issues保留。其关联规则计入本次已有安排/处置，不再机械报同一规则未安排；
其他未安排规则照常保留。一个候选含多项仍适用内容时应拆分或保留条件，不可同时把
同一短引用安排和排除；身份冲突按既有合同纠正/有限重试出口处理，不按理由关键词裁决。
明确排除候选的专属局部旧问题按原批次/目标身份退役，其他来源观察保持原code和
原文，并在最终detail前标注其来源读取阶段，避免把历史局部观察冒充最终缺失结论。
最终模型排除理由和宿主重算的全局未安排问题不加该来源阶段标记，不据文字猜测已解决。

### 已知要求与企业应答分离（BUG-063）

已知招标要求属于独立的只读数据，不以企业资料、写作成功或复核接受为存在条件。
新编制结果先保存要求底稿，再执行既有写作及复核；删除不受支持的应答，只影响
WritingUnit中的企业内容，不删除底稿。以下字段由宿主从已冻结分析及实际绑定关系
投影，不新增模型字段，也不要求模型重写要求或抄回覆盖名单。

| 字段 | 唯一合同 |
| --- | --- |
| AuthoringDocument.requirements_ref | 可选`ArtifactRef`，旧结果默认null；新生成结果指向独立AuthoringRequirements只读快照，不嵌入WritingUnit.blocks |
| AuthoringRequirements | `schema_version:2`、`authoring_input:AuthoringInput`、`rule_origin:extraction/verified_projection/source_only`、`rules:AuthoringRequirement[]`、`issues:AuthoringInputIssue[]=[]`。包括当前上下文全部已知有效规则，不只保存已安排位置的规则 |
| AuthoringRequirement.rule_ref | 沿用完整EffectiveItemRef，包含适用时的decision_id和rule_index；宿主按此身份去重，不按文字、数值或标题判断同一要求 |
| AuthoringRequirement.rule | `dict或null`，只读原样保存现有TenderItem或EffectiveRule的完整字段，不在复制时重复校验、补默认值或丢弃可读旧字段；仅无法读取该引用时为null并报告具体待核。有效投影只采用当前effective_rule，不读旧item补回被删除的条件/评分 |
| AuthoringRequirement.target_ids | `Identity[]=[]`，宿主由最终target.rule_refs派生关联，保留多位置关系；空列表表示未关联实际响应位置，不能删除该规则或自行宣称不适用 |

全量底稿以`招标要求底稿.json`随交付包提供。每章汇集narrative/form/attachment
全部目标的已有rule_refs，按完整有效规则身份去重，展示一份“招标要求依据”及空白
逐项响应位置，保留text、scope、conditions、exceptions和
原文依据；这些是招标方要求，不是企业承诺。企业已有内容仍按原写作单元独立显示，
不凭同章关系自动认定逐行满足。独立表单和证明实例仍按各自目标输出，不为每张
原表再各抄一份要求；没有证明文件也保留证明要求。去重仅限同章的要求依据，不合并
不同主体/标段的填写实例或跨章语义位置。未安排规则同样保留，并有待安排提示。不把评分、废标及
提交要求统一改名为技术参数，不将source_refs中的评审表/背景自动升级为模板。
presentation_role=cover的封面保持原格式，其已绑定填写要求及原文依据进入现有
`提交要求与待办.md`中的封面填写要求，不在封面前插入依据表，也不只保存在JSON。

展示确认状态沿用`AuthoringRequirements.authoring_input.confirmation_ref`，不新增字段。
该引用缺失或为null时，依据表第一列表头为“分析整理（待确认）”，表前及
`提交要求与待办.md`开头明确说明“摘要及适用范围仍需结合原文和有效补遗核对”。
企业应答继续留空；原摘要、条件、引文和有效补遗逐字保留，提示不代表语义错误已修正。
确认引用存在时沿用现有展示；`rule_origin=verified_projection`本身不等于人工确认。
该变化只影响确定性导出，不改变底稿、模型请求、分包或恢复身份；已有稿件保留，
重导出形成独立版本，不为补展示状态重跑模型。

仅有原文而没有结构化规则时，保留既有来源和可用模板、注明尚未取得逐项规则；
不从任意上下文复制整表猜参数。规则文字可读但原文不可读时保留已知内容及引用、
报告定位缺口，继续导出。快照不重复加入写作或F请求，本轮新增模型调用为0。

正文展示与存储编码分开：要求细则按既有字段含义显示可读标签及原值，列表分行，
不得直接将dict/list的JSON序列化结果写入Word。量化事实、评分、后果及提交细则
不能为消除JSON而删除；原始结构继续完整保存在要求底稿。未知旧形状保留可读内容
和具体待核，不能用JSON倾倒作为展示回退。原文已有的代码/JSON字符串仍按原文保留，
不作全篇标点或花括号清理；这些规则不改变模型协议或授权宿主推断招标语义。
kind、domains等内部分类只保留在底稿，不作为正文要求事实打印。数值、单位及引文
按原记录成组显示，0及明确布尔值不能按空值丢弃，不额外推导比较方向或计分含义。

### 新旧回包与修订边界

归并阶段的`OutlineAssemblyTarget.purpose`只是写作目的，不能替代来源证据。
该目的采用另一已提供候选或未安排规则的条件、例外、适用性或约束时，模型须通过
现有`target_ids`或`rule_ids`选择实际提供依据的条目；仅补充已发送提交原文或规则
引文的限定上下文时，也可用`source_ids`独立选依据，避免把上下文规则同时归属为
本位置的应答事项。宿主通过提交原文的直接引用或引文range_id的已有冻结映射，
按完整SourceUnitRef去重并入现有`source_refs`，后续加载完整来源。只选择实际需要的短引用，不回抄
全集、不按文本相似度推断关联，不因存在全局`submission_refs`就给每个任务装全目录。
宿主持久引用仍沿用source_refs；新增返回字段source_ids可省略，预算和原4次请求额度
不变。未知范围保留待核，不将purpose文字当成原文依据。

历史大纲/写作产物仍可读取；缺少template_refs时默认`[]`仅表示没有按新合同绑定
模板，不能恢复为“source_refs全部作为template_refs”。旧source回包的range_ids
未区分模板角色和精确边界，也未证明是否同一填写实例；不能根据旧kind、标题或unit
身份免费补出这些语义判断，不能仅因schema仍可解析就当作新合同成功回执。

完整业务顺序、一次结构核对及调用/修订预算见
[提交结构与表单归属修复方案](submission-structure-plan.md)；结构核对复用现有review
问题合同，不新增工作流状态，实际语义操作、Prompt及输入须固定进执行与请求摘要。

最终`operation=structure`请求先保留原视图；完整请求超准入时，才以既有网格投影
重试准入。投影视图标记`structure_view=compact_v1`，只转换`supports.grid`及
`submission_sources`内以JSON文本传送的原生网格。`rows`按原坐标保留全部明文和
空字符串，`null`仅表示没有独立原生单元格；`merged_cells`保持非1×1合并坐标和
跨度，物理列宽仍留在宿主。未知网格/单元格属性及稀疏展开更大的网格保留原对象。
所有目标、章节树、原文、处置、引用、顺序及其他字段保持不变。
仅该次structure系统提示追加格式说明并进入请求摘要，不修改共用`authoring_review`
提示，既有正文复核回执继续按完整摘要复用。投影后仍超限时沿原
`structure_review_unavailable`缺口继续生成；最多一次结构复核及原有限纠正，不新增
模型阶段、输出预留或身份誊写要求，压缩准入通过不代表语义或输出容量已验证。

新合同的schema/Prompt及实际视图须进入请求摘要。旧106页样例17份来源回包不能
直接复用为新模板合同的判断，需按新合同重跑受影响source批次，再重新组织正式
结构；实际包数与预算重新预审，不预先声称必定仍为17次。后续实际未变的完整请求
仍按摘要复用；目录重组导致target身份、title_path或依赖变化时可能使相关写作和
复核请求失效，不能承诺只增加一次assembly便复用全部正文。完整调用公式、容量
及业务处理顺序由实施方案引用本节，不在字段字典另设一套流程。

## 历史v1字段、共享来源合同与实现依据

以下表格及PR记录保留共享来源合同、旧独立接口和恢复事实；涉及Requirement/Plan的内容
不属于v2业务定义。schema仍是唯一运行校验源，新主线confirm后进入analysis-ready，
不执行旧Plan；不增加v1/v2兼容别名，也不迁移历史checkpoint。

本字典先于本轮命名统一实现提交。实现的唯一共享定义为 `tender/schema.py`；原生解析器
`StructuredSourceUnit.key` 等外部协议由新证据适配层一次映射，原 parser、gRPC 与 Rust 不变。
本路径尚未发布，不保留旧 `version`/`ref` 等内部别名，不增加历史 checkpoint 迁移层。

| 字段 | 类型与含义 | 身份与生命周期 |
| --- | --- | --- |
| `run_id` | string，独立工作流身份 | 提交后稳定；全部阶段/命令/产物统一 |
| `document_id` | string，文档集内一份主文件/附件/补遗身份 | 不用文件名或出现次序代替 |
| `document_version` | string，单文件的提交版本标签 | 登记、冻结、范围、引用始终同名；不叫`version` |
| `source_version` | positive int，run的整个文档集版本 | 原文件/清单变化才增加；DB、Lease、队列、checkpoint、确认统一 |
| `domain_revision` | positive int，同来源的要求/作用域审议修订 | 纠错增加、撤销旧确认；共享规范来源回执使用0，不能机械并入source_version |
| `schema_version` | positive int，产物格式版本 | 与业务版本不同，无旧格式兼容框架 |
| `unit_id` | string，规范原生载体身份 | 原parser的key只在适配边界映射一次；后续不改名 |
| `section_id`, `parent_section_id` | string或null，可靠章节身份/父身份 | 同名标题不等于同一章节；无标题允许null，不以heading_path当身份 |
| `heading_path` | string，展示用标题路径 | 不做唯一键；原生单位/共享章节索引持有 |
| `range_id` | string，某个可引用连续来源区间的身份 | 原始范围、合法fragment和support excerpt各有确定身份；证据/处置/验证一致引用 |
| `source_range_id` | string，区间所属未拆分规范范围 | 与range_id不同概念；重分包不改变它 |
| `parent_range_id` | string或缺省，派生区间的直接来源 | 与source_range_id/fragment lineage区别记录，不另造别名 |
| `start`, `end` | int，来源文本Unicode字符半开区间 | 与row_start/row_end的原生行区间不同；不当token位置 |
| `artifact_ref` | ArtifactRef，sha256/relative_path/size/media_type | 所有通用产物索引项统一此名，不再叫ref |
| `original_ref`, `raw_ref`, `source_index_ref` | ArtifactRef，原件、规范原生证据、定位索引 | 三种不同内容角色；全阶段保留同名，不复制正文 |
| `image_key` | string，原生图片载体在图片索引中的标识 | 原parser locator.original_ref字符串只在适配边界映射；不能与原件ArtifactRef同名混用 |
| `registered_ref`, `frozen_ref`, `pack_plan_ref`, `requirements_ref`, `verification_ref`, `response_plan_ref` | ArtifactRef，固定图阶段输出 | checkpoint仅保存这些固定引用，不逐包复制映射 |
| `pack_id` | string，计划内正文选择身份 | 不用它单独代表预算/输入相同；必须结合plan_digest |
| `task` | extract/verify/plan/ocr | 请求/计划所服务的具体领域动作 |
| `input_digest` | sha256，直接上游引用及相关操作配置的摘要 | 相同输入重试幂等；改变输入不得复用旧回执 |
| `budget_digest` | sha256，实际请求构造与容量规范摘要 | 包含model/C/prompt/reserves；不含秘密、不等于usage |
| `context_tokens` | positive int或null，配置的provider总上下文上限（输入+输出+安全余量） | 与输入预算分开；旧131072配置不自动升级为更大服务窗口 |
| `input_budget_tokens`, `correction_input_budget_tokens` | positive int，普通输入131072、显式纠错最多262144 token | 共享预算合同；普通verify/plan和新domain_revision不自动使用纠错额度 |
| `correction` | bool，明确标记一次修复请求可使用纠错输入额度 | 不改变provider总窗口或wire输出参数；未知总窗口不授权双倍 |
| `effective_input_budget_tokens` | int或null，当前输入预算与总窗口扣预留后的较小值 | planner/client使用同一测量；UTF8保守估算不是provider实测token |
| `request_digest` | SHA256，完整无鉴权模型请求的摘要 | 模型审计与发送重放身份；包括图片、消息、模型和协议参数，不是仅payload文本hash；同修订task下仍在途的sent阻止并发发送，uncertain按逻辑单元额度恢复；同run同source_version下摘要相同的成功回执可跨领域修订复用 |
| `logical_request_digest` | SHA256，移除顶层contract_feedback后的完整请求摘要 | 仅宿主请求审计使用，普通重发及合同纠正共用每command最多四次发送，uncertain仍计次；改变反馈不能重置额度，缺少此字段的旧冻结请求保留原防重放合同 |
| `metadata.command_id` | string，当前命令身份 | Store从lease写入模型请求审计，与logical_request_digest一起限定四次发送；不发送给模型，不由调用方或模型指定 |
| `plan_digest` | sha256，task+input_digest+budget_digest+selectors/决议的规范摘要 | 新预算或作用域绑定生成新计划，规范来源身份不变 |
| `requirement_id` | string，当前要求清单内义务身份 | 验证决议、计划项引用同一身份；内容修正产生新domain_revision |
| `evidence` | Evidence[]，range_id+原文quote | 不更名为另一种引用；quote必须能在规范范围重放 |
| `requirement_ref` | artifact_ref+requirement_id | 响应项引用已有不可变要求，完整约束只在内存校验 |
| `requirement_refs` | RequirementRef[]，artifact_ref+requirement_id | 后阶段从一个或多个原抽取产物选择完整要求；与响应项requirement_ref同一schema |
| `disposition_refs` | DispositionRef[]，artifact_ref+range_id | 后阶段完整逐范围处置引用；汇总核对完整覆盖，不复制处置正文 |
| `conditions`, `exceptions` | string[]，要求的适用条件/例外语义 | 与来源范围关系scope_binding不同，不机械合并 |
| `scope_binding` | 来源条件到具体range_id集合的关系 | 明确bound/unresolved；祖先正文不自动成为条件 |
| `scope_bindings` | `ScopeBindingRef[]`，每包完整条件的来源范围、字符区间及`scope_binding` | 无text；条件已在body中时仍传递明确目标，正文与supporting区间并集只发送一次 |
| `sections` | 请求内`{section_id,parent_section_id?,heading_path}[]` | 章节语义共享一次；每来源只引用section_id，宿主保留完整物理坐标 |
| `source_aliases` | SourceAlias[]，range_id+segments[{range_id,start,end}] | 跨旧包合并后原证据ID映射至本请求已发送的精确Unicode区间；无text，不更改原要求 |
| `source_context` | SourceView，响应计划显式范围依赖的必要来源视图 | 与extract/verify共享来源schema，不另建同义字段 |
| `blank_table_templates` | 请求内空白填写结构摘要数组 | 每项table_id/column_count、rows区间及原range_ids，可含merges/table_ids；不代表无要求，仍需逐原range处置和独立验证 |
| `column_count` | positive int，表格原矩形宽度 | 保护空白字段及模板列数，不用非空列数替代 |
| `host_reviewed_range_ids` | 仅宿主已证实空白页/显式审议装饰图的range_id[] | 与模型调用接受回执的`reviewed_range_ids`分开，模板空行不进入此列表；不伪称模型读过 |
| `scope_decisions` | 人工bind/local_only决议记录列表 | 每项带range_id、证据、理由、明确目标；进入新领域修订后重分包/重验 |
| `applicability_decisions` | requirement_id键控小型适用性决议 | 验证产物保存；计划读取原要求+当前决议有效视图，不复制完整要求 |
| `checklist_digest` | sha256，source_version/domain_revision/来源/计划/要求/验证摘要 | 确认精确绑定，任何相关修订使旧确认失效 |
| `status` | 各共享实体schema内明确枚举 | run、command、模型回执、编制执行、调度任务、项目分别采用宿主业务枚举，读取/写入/分支共用定义，未知值明确报错；OCR、scope、disposition继续由模型schema中的Literal约束，不相互冒用 |

## PR评论6160192506的三个边界

1. **PackPlan**：规范冻结来源在同source_version内复用，分包计划属于domain_revision，含task、
   input_digest、budget_digest及plan_digest。重试复用同计划；作用域/输入/预算变化须新修订/新计划，
   不碰原件、不复用旧extract/verify回执。verify/plan按完整实际输入生成阶段计划，必要时按语义
   和依赖重分；仅不可分单元超限才预算阻断，预算拒绝与模型语义失败分开。
2. **有效要求视图**：`applicability_decisions`按requirement_id保存具体决议；计划只处理原要求与
   当前决议合成的有效视图。实际文本、数字、条件纠正依旧通过领域修订，不能伪装成验证结论。
3. **有限作用域审议**：新增独立`review-scope`入口；检查当前来源和计划摘要、具体问题range_id、
   有效原文证据与目标范围。人工可确认作用域或证明是局部条款/误报，但不能仅删除问题。
   决议持久在新领域修订，来源保持不变；图重建相关支持和计划、重新抽取/独立验证，再允许确认。
   重复提交幂等、过期/无证据/不存在目标拒绝，旧确认和旧执行者失效。它不是通用运行时。

测试包括跨阶段JSON往返、禁止内部旧别名、三类版本区别、稳定unit/range身份、不同预算/决议
新plan_digest、幂等重试、适用性覆盖真正影响计划，以及scope审议后重启重验和过期拒绝。
真实13项原件范围问题在有依据决议并重验之前仍未完成；不把schema统一宣称真实模型验收。

PackPlan为当前领域修订的总计划（引用多个PackDescriptor）；PackDescriptor保存单包正文/
支持selector和planning_details（planner_version、boundary_reason、section_ids、budget）。
`submitted_source_version`/`submitted_domain_revision`仅表示幂等提交回执所指的历史提交，
可能不同于run当前身份，不是当前身份的别名。`review-scope`本期仅能绑定同文档规范目标；
跨文档条件引用保持明确未决，不能凭相邻上传顺序自动合包。补遗适用性是另外的验证决议。

后阶段使用task=verify/plan阶段PackPlan；verify使用PackDescriptor，plan使用
继承同一来源/引用合同的ResponsePackDescriptor，仅保存来源、完整义务选择及预算。对应verify/plan结果中的pack_plan_ref
指向后阶段计划，图原pack_plan_ref仍指extract计划，不扩大checkpoint字段数。
完整输入、跨extract包依赖、修订/部分成功恢复及不可分预算已通过确定性workflow回归；
真实语义验收剩余项见[当前交接](handoff.md)。

已去掉WIP子包的requirements_ref/requirement_ids/disposition_range_ids组合，统一上表
RequirementRef/DispositionRef；requirements_ref固定指阶段根的完整要求索引。内部合同未发布，不增加旧格式迁移。

## 网页大纲修订与固定大纲正文（BUG-076）

正式大纲继续使用 `AuthoringOutline`，不引入另一套模型响应格式。网页编辑投影由
`tender/outline_edit.py::OutlineEditChapter` 定义：稳定章节标识、标题、子章节、可选卷册/
封面标记及用户编写目标。原章节的响应位置、规则、来源和模板引用从服务端父大纲继承，
网页请求不能覆盖这些引用；来源仅可按下述明确修订追加。新增章使用客户端创建的独立 `manual-UUID`；有编写目标时
宿主建立对应narrative位置，无目标则仅标题，不推断招标要求或伪造企业依据。

生成候选通过所属项目的成功执行标识读取；保存传明确父修订及来源修订/来源执行二选一，
以项目行锁和父修订比较追加不可变记录。`revision_id`与`sha256`分别表示已保存修订身份
及其完整大纲文件，候选不替代已保存head，保存未知仍重试同一键和同一请求内容。

正文提交的`outline_revision_id`、`outline_sha256`只能引用本项目保存的大纲。宿主解析其
`outline_ref`并固定原`authoring_input`，实际执行用 `outline_ref` 跳过组织；独立大纲执行
用 `outline_only=True`，成功返回大纲文件而非`AuthoringDocument`。非默认选项固定到
执行规格，同ID不能更换模式或大纲；默认整稿规格保持原义。旧人工Word、已保存大纲和
新编制候选分别保留，不能把网页改纲称为已同步修改当前Word。

大纲与正文请求可使用既有可选`previous_execution_id`指定一个同类前驱：大纲只继承
大纲，正文只继承正文，且不得指向自身或另一项目。大纲前驱须属于当前选定的同一
分析run、来源仍当前；正文固定大纲继续绑定其原run，未固定大纲时沿用现有前驱run。
worker再次校验同run、同执行协议与同`outline_only`模式。大纲仍不接受知识选择、
`review_request`或固定大纲选项，不增加模式、字段或节点。
继承单位是完整请求摘要相同且重新校验通过的`document-model-response`；带反馈纠正
后接受的回包同样可继承。新执行仅提交`reused_from_execution_id`引用，不复制付费请求
行或usage。只改`outline_assemble`提示时，source请求满足以上条件才可复用，归并及
任何实际改变的请求另计调用；旧运行与各版产物保持不可变。

网页投影新增可选`target_ids`，仅选择同一来源执行及相同`authoring_input`的服务端响应。
未提交时沿原章继承；显式空数组移出本章响应，不能重写其规则、来源、表单或用途。
重复安排或未知身份拒绝。历史列表由`outline.history`读取，`outline.detail`返回本项目
修订与`available_targets`；恢复使用历史修订为来源、最新head为父修订，保存追加新记录。
历史目录不能跨生成执行混用相同的目标ID，人工响应定义从同执行的祖先修订恢复。

章节编辑可选 `source_additions:[{target_id,source_refs:[SourceUnitRef]}]`，由审阅者明确
选择遗漏的依据；目标须是本章保留或经 `target_ids` 选入的既有服务端响应。来源只允许
父大纲 `submission_refs`、父大纲响应或上述合法历史目录已持有的完整引用；宿主按完整
`SourceUnitRef` 核对及去重追加，不能仅凭 `unit_id` 接受另一份原文。不得删除原来源、
修改规则/模板/用途/企业依据权限，也不能借此创建新响应。未传或空数组保留原行为。
追加随现有保存请求摘要和不可变修订留痕；后续固定此修订编制，沿原摘要复用未变的
写作回执，仅重算实际受影响的写作与复核。它是审阅后的明确引用修订，不代表原模型
自动理解正确，不新增API、表、主节点或模型调用。

网页未指定固定大纲的编制任务在提交时保存`authoring_input_ref`；worker只从持久job
读取此引用，浏览器不能指定。执行规格固定该引用并检查完整快照与执行身份，恢复仍
沿同一快照；分析后来修订不替换已排队输入，取消检查仍独立生效。

正文请求可带`knowledge_selections:[{product_id,version_id}]`，最多20个固定UUID版本。
选择同时固定在request和job中，空选择不产生知识依赖；不得传current、凭据或服务URL。
知识worker在内存生成60秒、purpose/execution受限的内部令牌，API从job读取项目归属与
已选版本，逐次核对所有权、版本状态和原件实际关系。检索失败保留待补，结果在写作前
冻结；项目上传材料继续可用。选定资料不代表模型或宿主已确认其业务适用性。

分析网页的`analysis.review`提供精确run/source/domain/checklist及事项基准摘要。
`analysis.confirm`提交全部待人工问题的`acknowledged/dismissed`及理由，沿既有确认
合同绑定原版本。`analysis.correct`仅收`pack_id/item_id/text`、基准摘要和人工理由；
完整packs、证据、处置、关系和其他结构字段由服务端恢复，沿原领域修订重新核验。
此入口只纠正文案，数字、条件、范围等结构修订仍使用既有完整纠正流程。未知提交保持
原请求/key，过期基准409保留输入并提供最新分析对照，不自动套用到新版本。
