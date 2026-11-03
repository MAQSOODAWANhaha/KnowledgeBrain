# 编制完善实施任务

方案见[plan.md](plan.md)。下方T1—T7保留初版工作记录；评审已指出11项实现缺口，旧勾选
不能代表当前合同正确完成。本轮以文末F0—F8当前清单为准；源码完成与完整隔离回归、
真实语义验收分别记账。F8代码已实现，最终技术回归待补齐、真实全树尚未验证，实际
运行状态见交接。

## T1：固定底账与字段合同

- [x] 在`schema.py`落实字典拟议合同；规则×显式位置、响应行及已有处置均入账，不生成笛卡尔积。
- [x] 正文R/R2增加可选assessments，结构F不承担正文返回合同；旧文档缺coverage_ref保持可读但不追认覆盖。
- [x] 先建立同target两规则、无rule行、多主体及旧产物回归反例。

验证：`test_authoring_v2_schema.py`、`test_authoring_requirements.py`及新增覆盖结算单测。
依赖：无。主要文件：字段字典、`schema.py`、`authoring_requirements.py`及对应单测。

## T2：补齐漏意图检索

- [x] `_knowledge_queries`改为按本条要求/精确响应行核对意图，缺失才按固定要求文本补查一次。
- [x] 固定范围及查询来源、共享原值去重、原文长度边界和明确无资料出口，不加入复核后二次检索。
- [x] A1—A5定向用例通过，真实空命中不会自动换问法。

验证：`test_authoring_knowledge.py`、`test_authoring_pipeline.py`、`test_document_execution.py`中的查询回执路径。
依赖：T1。主要文件：`authoring_pipeline.py`、`document_execution.py`、知识适配及相关测试。

## 检查点一

- [x] 复核实际查询公式、共享要求去重及长度边界；确认缺口均可继续导出。
- [x] 定向Schema/检索/要求保全回归通过，未更改环境、预算算法或人工价格政策。

## T3：知识HTTP跨恢复重试

- [x] 在现receipt中持久记录查询/原件操作尝试，未知尝试计数；只由一层控制首次加三次。
- [x] 复用成功查询/空结果，耗尽不因重启重置；真实未配置与失败类别分别保留。
- [x] 第二次发送后中断、同执行失租接管、完成后零重查：隔离 PostgreSQL 用例已通过。这不是另一个用户接管项目。

验证：隔离PG/Redis执行测试及知识HTTP可计数fixture，必须检验真实请求总数。
依赖：T2。主要文件：`document_execution.py`、`integrations/authoring_knowledge.py`、`web_knowledge.py`及执行测试。

## T4：逐项复核与覆盖结算

- [x] 沿既有R/R2收集有效assessments，绑定完整范围、受审unit_ref和最终采用版本，局部错误不整包拒收。
- [x] 宿主生成coverage_ref；一行/一主体/局部答案不能覆盖其他行/主体；partial不自动删正文。
- [x] A7—A11、A16的宿主结算与草稿文案已有单测。报价仍留空。真实Word抽查未做。

验证：`test_authoring_review_scope.py`、`test_authoring_semantic_adoption.py`、覆盖新单测及执行/导出验收。
依赖：T1、T2。主要文件：`authoring_pipeline.py`、`authoring_revision.py`、`authoring_review.md`、覆盖结算模块及测试；
内部再分“复核接收”和“最终结算”两个小提交，避免一个大改动。

## T5：逐单元与采用检查点

- [x] 沿现receipt保存逐unit；已保存unit恢复时不重写。查询尝试另见T3。
- [x] 模型回执仍走既有document-model-response重验；一次改稿额度沿现有请求账，不在恢复时清零。
- [ ] A12—A15里，已保存单元在失租后仍可被同一执行读到，过期工作者不能改写。整次编制进程中的采用/导出杀进程仍未单列跑完。

验证：`test_document_execution.py`及新增逐单元/采用恢复验收；记录恢复前后真实发送计数。
依赖：T3、T4。主要文件：`document_execution.py`、`authoring_pipeline.py`、`authoring_revision.py`及执行测试；
先落逐unit，再落review/adopt两个可验证小提交，不重构整个管线。

## T6：候选选材与消费展示

- [x] 保留query/rank/score，容量不足按要求公平选择完整片段；落选保留明确码，不用词重叠裁决。
- [x] 工作台读取coverage_ref，展示草稿、待补及待核；旧产物显示覆盖未知。
- [ ] A6的选材函数已测。浏览器对照最终Word未做。

验证：候选组织单测、API契约测试、真实浏览器查看及Word实物对照。
依赖：T4；可与T5并行。两个独立小任务先后提交，各限制在约3—5个实现/测试文件。

## 检查点二

- [x] `tests/unit` 2098 passed。浏览器与隔离杀进程未跑。
- [x] 调用预算仍是既有四次额度；assessments只增加正文复核输出。真实文件与企业资料仍未验收。

## T7：真实文件验收

- [ ] 同106页文件无企业资料跑通，逐项结算且原要求/条件/模板保全，企业应答和报价不编造。
- [ ] 真实索引/检索/模型纵切通过，再全文件；受控资料与真实企业资料的验收状态分开。
- [ ] A1—A16实测证据、Word/ZIP、用量/耗时及恢复对照保存在仓库外，交接记录实际状态。

验证：按plan.md验收矩阵与三层验收逐项人工核对；真实企业资料未提供时该项明确未验收。
依赖：T5、T6。不增加产品型号确认、资料齐备或业务缺口下载门禁。

## 本轮评审修复待办

详细根因和B1—B25验收见[评审修复方案](plan.md#评审修复方案)，问题记录BUG-115至120。
F0—F5已实现并完成定向验证；以下勾选区分源码/定向结果与F6完整验收。
历史2098项unit或初版34项定向结果不能替代本轮反例、真实文件及企业资料语义验收。

### F0 显式验收入口（BUG-120）

- [x] generate替身显式接收saved_units/on_unit/saved_plan/on_plan，验证传值并调用回调，不用**kwargs掩盖漂移。
- [x] 原acceptance入口和最低持久化/恢复/取消烟测可运行。
- [x] 各F任务分别建立自己的失败反例，不在F0集中前置，不把简化fixture当实际并发恢复通过。

验证：`tests/acceptance/test_document_execution.py`定向19项通过；真实管线计划恢复另有4项通过，归F1。
依赖：无；约1—3个测试文件。该项修测试接口，不修改生产策略。

### F1 资料映射与旧单元恢复（BUG-115）

- [x] _task_materials唯一选材/顺序；W/replay/R/C/R2及附件使用相同映射。
- [x] 落saved_plan/on_plan及AuthoringDocumentPlan合同；全部fit后、首个W/on_unit前持久提交document-plan，保留检索合并资料、分片/选材及规划gaps。
- [x] 恢复从实际冻结依赖重算plan/unit input_digest，再采用原计划；旧task_id或自带摘要不能绕过校验。旧绑定合同升级沿显式后继执行，不覆盖原不可变回执。
- [x] B1/B2/B17/B25定向通过：原成功JSON可按同请求重绑，计划提交故障窗口不误发W，旧错绑单元不直通，旧CAS/人工Word保留。

定向验证：核心22项通过，实际管线/隔离PG计划恢复4项通过，包含并行写半途恢复；
资料裁剪专项观察W/R/C/R2、附件及ZIP原件，故意错绑可使断言失败。恢复与绑定的
技术证据已具备，不表示真实企业资料相关性或供应商模型语义验收通过。
依赖：F0；主要authoring_pipeline.py、document_execution.py、schema.py及对应测试，分“映射”和“检查点”两小步。

### F2 完整查询身份（BUG-116）

- [x] 文内content与外部submission按已有用途区分，移除kind快捷分类及固化它的错误测试。
- [x] 共同条件和行原文一起装载/查询；spec()与真实冻结范围统一。
- [x] B3—B5对应定向场景通过，现有报价/无资料/空命中出口保持。

定向验证：coverage查询单测及实际_FrozenKnowledge→pipeline组合测试；F2/F4合计54项通过。
依赖：F0；主要authoring_coverage.py、authoring_pipeline.py、document_execution.py及对应测试。

### F3 单操作持久重试（BUG-118）

- [x] 落窄run_operation回调，Web仅转传，provider和冻结器删除聚合/内层重复重试。
- [x] 单scope search、metadata、file各自累计四次；start/outcome独立不可变回执，成功结果先提交再返回，聚合快照前可恢复。
- [x] 同operation局部锁内重读结果/额度；取消、失租、持久存储异常退出，未提交结果按unknown扣额度。
- [x] B6—B8及B20—B24对应定向场景通过，已提交成功/空结果零重发；实际HTTP发送≤已登记start≤4，无中断时二者相等，outcome不计额度；不承诺exactly-once。

定向验证：真WebKnowledgeProvider+MockTransport等74项通过，隔离PG的31项通过。
旧聚合回执不追认为逐操作额度；跨合同升级保留原记录，用显式后继执行。
依赖：F2；分“适配器单次操作”和“持久runner接线”两小步，每步约3—5文件，不迁移数据库。

### F4 覆盖映射与精确结算（BUG-117）

- [x] 补target共同规则差集的独立row=None底账，不重复行要求；cN.refs来自真实投影，相关范围按实际分片，空判定绑定所有受审版本。
- [x] 四类内容统一识别，最终只从有效判定选择回绑资料；R0/R2按有效范围/版本采用。BoundRequirementAssessment及AuthoringReviewRecord保留宿主绑定元数据。
- [x] B9—B15及B19对应定向场景通过，共同规则不掉target/范围，错引用局部诊断、正文保留、不拼局部为完整、不增加模型轮。

定向验证：coverage/review scope/semantic adoption及实际管线组合；与F2合计54项通过，非真实模型语义验收。
依赖：F1、F2；分“cN/受审映射”和“内容索引/结算”两小步，各约3—5实现/测试文件。

### F5 Word版本覆盖适用性（BUG-119）

- [x] 后端摘要绑定生成version_id+sha，人工/恢复/未知稿当前覆盖待核。
- [x] 前端核对current/pinned；多版本下载统计标清所指版本，下载继续可用。
- [x] B16 API和Chromium工作台定向验证通过，人工Word未被覆盖、无自动付费重审。

定向验证：Rust API 2项、Chromium工作台14项及前端build/lint通过；真实Word实物内容仍归F6。
依赖：F4；按后端契约、前端展示两步，复用已有版本字段，不新建权限或状态系统。

### F6 完整回归与真实文件（全部）

- [x] 受影响unit、workflow、acceptance和PG/Redis/CAS中断测试通过，B17验证真实并行管线；当前逐用例绿色证据已核对，数量见交接。
- [x] B1—B17及B19—B25技术场景有证据；新缺陷先复现后修复，既有保护保留回归，B20—B25明确区分持久提交前后的故障窗口。B18真实语义另列，不以技术证据代替。
- [x] B18无知识106页草稿保全检查通过：复用冻结分析/大纲，核对原要求、共同条件、模板、分册与覆盖底账；发现的BUG-121经失败回归修复并离线重导出，旧成品保留。
- [ ] B18真实检索纵切→全文件，人工核对资料/应答/证明/Word版本；企业资料未提供，有效模型语义复核未取得，不能由保全检查代替。
- [x] 报告物理发送、Token、耗时、重绑复用与未决项，证据仓库外保存、动态状态只记交接；未知用量及未取得的语义结论明确保留。

依赖：F1—F5。资料缺失不阻断生成；受控资料与真实企业语义验收状态分开报告。

### F7 既有结构修复出口与传输诊断（BUG-069、BUG-013）

- [x] 核实真实原文、自动A1和最后F1回包，区分已读原文后的错误关系与固定纲只读保护。
- [x] 先统一chapter_moves及安全传输诊断字段；不按标题、编号、路径或finding文本推导移动。
- [x] 仅既有自动F1接收显式移动，整批校验并保全子树及稳定身份；固定纲/人工Word不变。
- [x] 首部前与正文阶段的异常类型链/errno入持久回执，不改重试、网络、模型或超时。
- [x] 新增失败回归、结构Schema及客户端定向回归通过，接线到真实Word渲染已验证。
- [x] 当前代码完整unit与同文件真实子树纵切；核对原文、所有模板/技术行和Word层级。全树语义复核未取得有效回包，保持待核。
- [x] 保存真实回执、用量及旧稿保全证据，补交接并完整备份、精确清理本轮隔离库。

真实纵切只验证独立自动候选的末次结构复核，不算从头五节点或全编制语义验收。
供应商中断若重现，按原四次额度留存新增诊断及待核出口，不用技术测试声称外部故障已解决。

### F8.1 候选内部范围与保全（BUG-069）

- [x] 建立scope缺陷失败反例；普通候选rule_ids/candidate_rules共享映射与candidate_scope按字典实现，顶层追加选择语义保持不变。
- [x] 按scope重建精确/粗来源及完整规则；未安排子范围保留待整理，主体/人工边界与模板/响应表不拆改。
- [x] 保存前版scope，拆后省略不猜全量；耗尽保留安全稿，歧义参见明确待核，保留普通无scope合并分支。
- [x] F提供每目标及response_rows各自所选rule_ids与共享target_rules；精确证据逐值一致才复用supports，缺失记structure_target_rule_unavailable。
- [ ] 补齐C1—C4/C6对应的受影响完整隔离回归及实际自动组织接线验收，不能仅由定向绿色勾选。

验证：test_assembly_candidate_scope.py、test_assembly_scope_regressions.py、test_structure_target_rules.py
及既有hierarchy/grounding/output-contract回归；分别核对逐目标关系与保全并集。定向测试
已有证据，完整隔离技术回归待最终补齐，实际数量和执行状态只在交接维护。
依赖：字典及plan.md合同已记录；分A候选映射、scope绑定/保全/修订、F逐目标规则映射三小步，各约2—4文件。不依赖F8.2的表示实现，但最终计量须同时包含两者。

### F8.2 完整F无损视图（BUG-069容量补充）

- [x] 建立C7展开等价反例并实现compact_v2：support_sections按既有section分组，未分组supports及submission_sources按原ID字典化，仅省可还原默认值。
- [x] 沿authoring_structure更新视图说明及新表示接线；保留非默认/未知字段、原件及持久树，不压修订回包。
- [x] 使用最终RequestBudget.measure_request重新计全部Schema/Prompt/Wire；保持超估算可发送，不预建分包器。
- [x] 补齐完整隔离回归，并在包含新规则映射的实际完整候选树F请求中验证表示及返回合同；本项不表示自动归并或全树语义已通过。

验证：test_structure_section_view.py、test_structure_compact_view.py、test_structure_shared_text.py
及完整请求回归；同文异源、网格、空行及全部引用逐值还原。真实全树测量仅为容量证据。
依赖：字典及plan.md合同已记录，可与F8.1并行。主要authoring_view.py、authoring_structure.py及新测试。

### F8.3 精确范围检索（BUG-069检索衔接）

- [x] 建立同unit两段意图串绑反例；精确意图为本位置范围子集且并集覆盖完整原文/共同条件，仅双方均无精确范围时保留粗匹配。
- [x] _excerpt_texts装载目标自身及表/行条件；明确knowledge内容位置缺适用意图时，仅用全部成功装载的精确原文补一次，残缺/超长记缺口。
- [x] 沿现有用途边界排除人工/纯导航/tender位置，带参见正文处理自身要求；按原文、固定scope/版本和原件模式去重，保留既有回放，不新增复核后检索。
- [ ] 补齐C5/C6恢复及受影响完整隔离回归，真实知识/全树语义另列，不能将固定范围查询身份测试当成真实命中验收。

验证：test_authoring_precise_retrieval.py、知识计划/coverage及_FrozenKnowledge接线测试；
定向用例已有证据，完整技术回归待最终补齐，单操作物理重试仍沿既有首次加三次额度。
依赖：F8.1。主要authoring_coverage.py、必要pipeline装载接线及对应测试。

### 检查点三：新自动候选与旧稿保护

- [ ] C1—C8及受影响完整回归通过，最终新请求容量已重新计量，旧纲/Word/回执未改。
- [ ] 标题通过既有A0/F0/A1检验业务职责和来源定位分离；不新增编号正则、F1改名器或A2。
- [ ] 不把union、schema、fits或Word导出成功当成语义验收；保留未解决范围的明确出口。

### F8.4 标题保护与同文件真实验收（BUG-069/062/013）

- [ ] 先离线预审完整调用及复用边界，再用同106页文件的新自动候选完成C9/C10；不以旧固定纲重导出替代。
- [ ] 取得整树F有效回包并逐原文核对；验证原要求/模板/技术行/共同条件/图片/分册保全、空企业应答及人工价格。
- [ ] 核对Word业务标题、目录域与分页，保存全部回执/新旧稿及用量；断流仍发生则区分阶段、明确未验收，不冒称容量修复了远端故障。

验证：真实模型/Word/ZIP人工对照；受控知识资料验收与真实企业资料验收分别记录。
依赖：F8.1—F8.3及检查点三。证据仓库外保存，动态状态只在handoff.md更新。
