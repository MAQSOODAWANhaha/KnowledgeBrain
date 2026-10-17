# Python 第一阶段唯一字段字典

## 业务协议v2（工作区已实现）

**本文件是第一阶段字段名称、类型、可选性和身份语义的唯一规范。**
[业务工作流](business-workflow.md)规定处理顺序，其他设计文档引用本字典，不另建同义结构。
工作区`tender/schema.py`、抽取/核验提示和五节点主线已实现v2；下文单列的v1合同仍供
历史独立工具使用，不自动转换旧回包。v2压缩、回包容错和默认重试已部署，两类审阅身份修复后真实独立核验已恢复，新协议语义验收未完成；
测试及部署事实以[交接](handoff.md)为准。
下文“问题续报与按需补读”已在源码实现，normalization_version=3，部署及实测状态见交接；不改变旧执行的冻结协议。

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
| `work_unit_id` | 宿主固定、当前修订内唯一的子操作身份，复用现有 stage/scope 回执，不另建任务系统；抽取时由本包身份提供 |
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
| 抽取输出 | `{schema_version: 2, items: TenderItem[], dispositions: Disposition[], relations: Relation[], issues: Issue[]}`，五项均必填；模型返回的 Disposition 仅需 `{range_id,status,reason}` |
| `Disposition` | 持久化为 `{range_id, status, reason, item_ids}`；status 为 `items/non_item/unresolved`，reason 为有来源依据的非空说明 |
| `item_ids` | 宿主按事项的有效直接 evidence 派生反向索引，模型不再重复生成。规范产物中 items 处置非空，non_item/unresolved 时为 `[]`；每个 body 范围均有一条规范处置 |
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

抽取及 verify 子操作的输入均带 `schema_version: 2`、`work_unit_id`、`source_version`、
`domain_revision`；run 身份、输入引用和 ID 到不可变产物的映射由宿主回执固定。
实际协议、操作及版本绑定 input_digest/request_digest，不能只凭 task 相同复用。

| 输入字段/类型 | 唯一定义 |
| --- | --- |
| `source_views` | SourceView[]，实际装载的完整来源视图；抽取恰有一个，多文档核对分别保留每份视图的 document_id/document_version，不拼成假单文档；当前文档的登记 role 在视图中发送一次，保留主件/补充文件等理解依据。文档按登记顺序、body/supporting 各按冻结原文顺序及片段字符起点排列，不按哈希 ID 排序；排序由宿主完成，不额外发送排序元数据。同一文档按已有区间去重规则投影成一个请求视图，不跨来源按文本去重 |
| `SourceRangeView` 正文 | 普通段落保留 `text` 原文。表格行用 `cells: SourceCellView[]` 直接提供单元格原文；当各单元格及原生行分隔符完整表达本范围时省略重复的整行 `text`。不能完整表达的范围保留 `text`，不得丢掉未被单元格覆盖的原文；宿主的规范行全文、字符偏移与定位保持不变 |
| `SourceCellView` | `{column: 非负int, text: string, row?: 非负int, row_span?: 正int, col_span?: 正int, formula?: string, ...}`；text为当前范围实际可见的单元格原文，允许空字符串，保留换行、Unicode与合并/公式等原有语义字段。不发送cell的start/end让模型切片；片段由代码按规范字符区间切好。证据按本次实际可见的text或单元格原文逐条引用，不能引用未发送的单元格剩余内容 |
| `source_aliases` | SourceView内的既有引用身份及实际已发segments映射，不新增正文副本。引文须受原别名区间约束且由本次实际可见文本证明；普通段落的连续已发片段可还原引文，cells-only片段不能借规范整行全文或分隔符拼接成引文。若整行text实际已发，可引用其原文子串 |
| `catalog` | CatalogEntry[]，仅 locate 携带；discovery 为本批事项证据所在文档/章节的局部目录，reference 为完整章节定位目录。抽取不携带全目录，associate/review 使用已装载原文。目录不表示正文已经装载或审阅 |
| `CatalogEntry` | `{document_id: string, document_version: string, role: string, sections: CatalogSection[], page_count?: 正int}`；文档身份只发送一次，role 沿用登记角色。仅冻结PDF有完整、连续的0基物理页清单且页数一致时提供page_count；不发送全量页码定位表 |
| `CatalogSection` | `{section_id: string或null, heading_path: string, range_ids?: 非空string[]}`；discovery 的 range_ids 仅映射本批事项实际引用的来源，不附全章范围清单、页码、坐标或存储引用，也不扩大证据引用权限。宿主通过当前冻结索引的 `(document_id, section_id)` 装载全部章节原文；无章节为null，不得按标题文本猜测或去重 |
| `items`, `dispositions`, `relations`, `decisions` | 对应本字典的完整对象数组；由宿主按实际引用装载，不发送内容摘要冒充原对象 |
| 输入 `issues` | 已持久化 Issue[]，每项额外带宿主赋予的 issue_id；相关来源缺口也须显式提供 |
| 输入 `scope_decisions` | 可选的现有 ScopeDecision[]；当前修订有显式人工作用域审议时发送完整 decision、rationale、evidence 和 target_range_ids，包括 local_only。是需结合原文复核的解释/绑定依据，不自动解除独立核验或来源阻断；无审议时省略 |
| `Candidate` | `{candidate_id: string, issue_ids: string[], range_ids: string[], item_ids: string[], reason: 非空string, section_refs?: SectionRef[], page_refs?: PageRef[]}`；至少一个来源、事项、章节或物理页引用；前三个 ID 数组必填，无值的可选引用数组省略。range_ids 仅引用实际提供的来源或完整事项证据，section_refs 仅引用目录中已提供的章节，page_refs仅引用目录声明支持的物理页。未指向既有问题时 issue_ids 为 `[]`。共享来源或事项不自动合并独立候选；同一具体 issue_id 的多个候选须联合核对，单个候选不得拆分 |
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
Token，结构下限也不是完整输出预测；`fits` 不证明输出能完整生成。当前16384输出预留只用于
准入，未作为 wire 输出上限。真实联调需同时检查实际 usage、结束原因和完整 JSON/覆盖校验；
usage 缺失保留未知，sent/uncertain 不通过换身份重发。后续关联调用数随真实候选产生再核定。
已部署目录压缩版的首轮真实抽取出现单次43709 completion Token（详见交接），说明不得把
16384当作真实输出上界。更换发送格式后的请求必须重新测量，旧估算不能直接套用。

当前修订有 scope_decisions 时，抽取与核验同时携带该列表，并将原文、实际目标及关联事项
共同送入独立 review。不能仅凭 domain_revision 变化假定人工解释已送达或已通过。

| operation | 输入（除公共版本身份外） | 输出 |
| --- | --- | --- |
| `locate` | catalog、issues、items；引用定位须带原引用的 source_views，跨源候选发现须带计划覆盖的完整事项及来源身份 | `{schema_version: 2, candidates: Candidate[], findings: Issue[]}`；只能提出来源定位或跨源候选，不能解决问题或判定全局无冲突 |
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
- 事项间 discovery 的 catalog 仅包含本批事项证据所在的文档和章节，保留 document_version、
  role、heading_path 及已声明的 PDF page_count。各章节可附 `range_ids: string[]`，仅列出
  本批事项实际引用的来源，用于建立证据身份到文档/章节的映射，不扩充可引用来源。
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
| `correct` | 显式纠错修订 | 处置覆盖不完整、正文范围重复、整包抽取失败 |
| `host-fix` | 宿主组包或执行缺陷，修复后重跑 | 各类预算超限、核验覆盖不完整 |
| `human` | 语义疑点，确认时人工处理 | 事项/关联/问题声明待核对、决议未被接受 |

前四类为硬阻断：当前修订存在阻断的此类记录，或必要覆盖/工作单元不完整时，运行为blocked，
人工不能驳回。verification产物在原`blocking`（存在任何阻断问题）之外增加`hard_blocking: bool`
和`human_issue_ids: string[]`（仍阻断、需人工处理的问题身份，升序）。

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
  一次执行对一个工作包最多调用一次，不在节点内重试；失败由现有命令层统一计入首次加
  三次重试。已拒收的同一纠正内容再次出现时停止，不能反复读取同一缓存耗尽次数。
- 响应纠正只处理尚未接受的工作包合同，不修改已接受事项或自动解除业务问题。
  sent/uncertain继续阻止自动重付费；不增加状态、环境变量或计数表。

### 失败执行的显式恢复

核验分包、完整请求预算测量等同步规划在工作线程执行，事件循环持续处理租约心跳；
线程调度不改变请求内容、摘要或回执身份，也不扩大上下文预算。

明确的执行失败默认首次加最多3次重试，包括连接、服务拒绝、JSON/宿主合同和普通代码
异常；同命令共享4次失败预算，成功子产物不重做，不叠加SDK或节点内部重试。历史SQL字段
`infrastructure_failures`继续保存该共享执行计数，不新增状态、配置项或计数表。预算耗尽
才到failed。过期执行由接管方结算一次失租失败，也计入同一预算；重复投递不重复计数。
旧Worker取消/版本或租约失效后停止写入；并发兄弟中出现sent/uncertain相关异常时优先停止，
不能被另一个格式错误掩盖。业务blocked不是执行异常，继续走独立核验与现有裁决入口。

`retry` 只恢复当前来源/领域修订的执行失败，不做业务纠错，不增加节点或状态。操作绑定
`run_id/source_version/domain_revision/failed_command_id/request_key/reason`；首次申请须指向
当前修订最新的失败命令，运行已 failed、无活动租约且未取消。同 request_key 和相同参数
返回同一恢复命令，不因重复提交增加模型调用；参数不同或基线过期拒绝。

恢复新建同修订命令，继承原命令的 start/resume 和业务参数，在 payload.retry 中记录
`failed_command_id/request_key/reason`。该审计元数据不进入确认业务内容，也不发给模型。
原失败命令、回包及用量不改写；沿用同一执行快照和 checkpoint，复用成功产物及成功模型
响应，只允许明确 failed 的请求重新付费。当前修订存在 sent/uncertain 时仍禁止该恢复，
不以换版本、改摘要或清状态绕过围栏。语义未决的 blocked 继续走现有证据裁决或显式纠错，
恢复命令不代表独立核验或人工确认完成；合同纠正只在上述共享重试上限内执行。

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
| `request_digest` | SHA256，完整无鉴权模型请求的摘要 | 模型审计与发送重放身份；包括图片、消息、模型和协议参数，不是仅payload文本hash；同运行修订/task下sent或uncertain阻止自动重付费，完整成功响应可复用 |
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
| `status` | 各共享实体schema内明确枚举 | run、command、OCR、scope、disposition是不同实体状态，不相互冒用 |

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
