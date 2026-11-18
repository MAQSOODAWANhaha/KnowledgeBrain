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

## F9：分析与编制性能修复清单（BUG-139）

合同见[完整方案](plan.md#f9分析与编制性能修复方案bug-139)。这是原招标修复工作的增量，
不替换F0—F8未完成项。60秒连续raw idle已有源码和隔离验证，部署/真实效果按交接记录；
下面其他能力均不因规划完成而勾选。每个实现切片先失败反例、最小修改、定向验证、增量审查。

### F9.0a 请求耗时观测

- [x] 在现有activity/回执补first_raw、first_content、max_raw_idle和排队时间，保留旧first_data含义。
- [x] 覆盖gzip缓冲、纯心跳/thinking、空块、末尾静默、取消；日志不含原文/headers/密钥，实际请求与摘要不变。
- [x] 定向streaming/client及两条PG保存链路测试通过，观测自身新增模型调用为0。

依赖：60秒基线。范围：`llm/streaming.py`、`client.py`及对应2份测试，约4文件。
验证：`test_tender_streaming.py`、`test_tender_llm.py`；全部时间来自虚拟时钟或本地SSE，不以真实模型替代边界测试。

### F9.0b 阶段准备与输出成本基线

- [x] 禁网真实回放分别计_Analysis、review/locate/association、描述计量、装批、CAS和回执恢复；重复测量并报告波动。
- [x] 列输出字段/visible/reasoning成本，传输成功与正式采纳、墙钟/请求并集/请求总和、未知用量分开。
- [x] 给出F9.2热点、F9.5字段成本及F9.6启用条件缺口的证据；不将历史612秒全部认定为可消除成本。

依赖：F9.0a指标口径；离线profile可并行准备。范围：窄诊断脚本、`llm/usage.py`或必要阶段计时及对应测试，拆成各约3文件。
验证：`test_budget_observation.py`、`test_analysis_responsiveness.py`和真实冻结回包回放；证据仓库外保存。

### F9.1a 普通与合批429策略一致

- [x] 保留安全失败类别及Retry-After数值，沿用既有30/60/120且封顶120的策略，非429不套用长退避。
- [x] 相同429失败序列下两条路径语义一致；成功兄弟保留、uncertain未知用量、每逻辑单元首次加三次均不改变。
- [x] 无效/缺失/超上限Retry-After与最终耗尽反例通过，不把延长等待等同于已经提速。

依赖：F9.0a；可与F9.2离线工作并行。范围：`llm/recovery.py`、必要`client.py`及rate-limit/failure测试，约3—4文件。
验证：扩展`test_rate_limit_recovery.py`、`test_failure_retry_table.py`、`test_contract_recovery.py`。

### F9.1b 等待期限持久化及崩溃恢复

- [x] 普通与合批在既有回执中保存失败身份及not_before；新发送和unfinished恢复均只等剩余时间。
- [x] 失败保存后、预留后等待中、截止过后重启；等待时取消/失租；部分成功叶恢复均不重复发送或增加额度。
- [x] 明确旧回执缺字段/无时间信息的回退；sleep不持事务锁，旧worker恢复发送仍受epoch围栏约束。

依赖：F9.1a。范围：`llm/recovery.py`、必要现有store回执接口及unit/隔离PG恢复测试，约4—5文件。
验证：`workflow/test_group_recovery.py`、`test_request_replay.py`、`test_work_unit_attempts.py`、`test_expired_retry.py`；普通路径同样覆盖崩溃点。

### F9.1c 外部断连与实际并发边界

- [ ] 用request_id/时间对照已有HTTP阶段与上游排队、首字节、deadline、关闭方；缺日志则明确待查。
- [ ] 计算多个execution/worker的真实重叠峰值；真实对照先单任务运行，不把单实例2当成Key全局2。
- [ ] 若证实共享额度争用，先评估既有接单配置；若没有对应证据，不改网络、代理、模型或并发来猜修。

依赖：F9.0a；与本地实现并行。范围：现有诊断/报告与服务侧证据，不预设生产代码修改。
验收：有上游证据和匹配的修复后结果才能关闭断连；60秒本地超时、一次成功或更快失败均不算解决。

### 检查点F9-A：可靠性

- [x] F9.0/1的确定性与隔离恢复测试完成，旧断言修正复验无失败/错误/必需项跳过，完整请求/既有回包身份未受观测影响。
- [x] 429策略修复与待查61秒断连分开；退避/重启不增加尝试次数，隔离取消测试确认无遗留后台调用；真实服务状态不据此推定。

### F9.2a 纯规划描述缓存收益评估

- [x] 重复剖析operation_plan/Token计量与各阶段请求构建，比较潜在收益与波动。
- [x] 描述缓存潜在收益未超过整体测量波动，保留原实现，进入已证实热点的F9.2b；实际数值与证据见交接。
- [ ] 若后续样本使该片成立，再验证完整依赖失效、冷/热请求一致及发送围栏；当前不声称已实现描述缓存。

依赖：F9.0b热点结果。候选范围：`checks/analysis.py`、必要`analysis_batch_execution.py`及计划复用测试；无净收益不落生产代码。
验证：新计划复用测试、`test_analysis_batch_execution.py`、`test_analysis_concurrency.py`、`test_analysis_responsiveness.py`。

### F9.2b 完整阶段请求复用（按profile决定）

- [x] 仅构造确为热点时缓存纯review请求；复用既有CAS/回执，保留JSON键顺序，不增加通用缓存框架。
- [x] 命中不重做对应构造，未命中重建、损坏失败、依赖变化失效；不缓存租约/凭证/attempt或pickle活对象。
- [x] 真实冻结回包冷/热重放的完整核验产物、实际review请求摘要相同，新增模型调用0；实测在交接维护。
- [ ] 仅未来另有热点证据才缓存locate/association，并显式恢复其候选、路由、问题、保护和未完成集合；本片保留原实现。

依赖：F9.2a测量。范围：一个阶段的analysis/必要接线及plan-reuse/隔离replay测试，各片≤5文件。
完成出口：有净收益则保留；无需实现时记录profile依据并保留原实现，不伪称已缓存全阶段。

### F9.3 当前A0/A1请求与内容验证

- [ ] 零网络构造当前A0各包/全局归并/F/A1，报告实际总调用、最大/累计输入、全部保留正文及未分配要求。
- [ ] 精确身份相同的S才复用；原表/技术行/共同条件逐值核对，不能用旧151k/162k请求当新版测量。
- [ ] 有限真实对照同时核对采纳、Token、重试、耗时及Word；现有实现无缺陷时不重复写compact或精简器。

依赖：F9.0；真实对照采用检查点F9-A配置。范围：离线脚本/测试，发现缺陷先入BUG再限域修改。
验证：`test_revision_context.py`、`test_session_outline_contract.py`、`test_assembly_candidate_scope.py`、`test_assembly_condition_handoff.py`、`test_outline_revision_selections.py`。

禁网预审已确认当前S Prompt与旧v8不同，首个请求即无精确回包；不得移植旧S假冒新版
A0/A1实测。实际输入体量与停止位置见交接，以上完整真实对照仍未完成。

真实执行暴露BUG-140，修改前五问见plan.md的F9.3补充：

- [x] 先失败复现并补齐每个A0子包的候选/未分配规则证据闭包，完整原文逐值保全。
- [x] 以含全部依赖及未分配规则的最终请求装包，复现并修复末包后追加超估；保留不可分单元的明确出口。
- [x] 全局表示先做无损去重试算；实测仍超预算，未增加生产协议，不盲发已知超估global。容量出口仍待解决。
- [ ] 完成新冻结合同及受影响回归后，以新执行精确复用已成功S/A0，再推进真实F/A1/Word和新分析。

真实全局出口检查另发现BUG-141：

- [x] 以多包反例锁住局部候选/引用域，遗漏保全不再重复其他包候选。
- [x] 拼接按包身份重映射关联，覆盖相同局部编号与候选绑定的保全。
- [x] 完成新接收合同、回归与冻结证据；全局容量未解决时不称完整编制通过。

### F9.4a S包校验隔离

- [x] 每包固定wire、别名和可读范围；移出共享read_units完成时序依赖，由协调者按计划序归并。
- [x] 逆序返回、前包纠正/耗尽、同unit不同片段、跨包仅有引用身份反例通过；语义变化显式记录冻结合同。
- [x] 既有来源缓存/引用绑定和安全稿保全定向回归通过。

依赖：F9.3离线来源基线。范围：`authoring.py`、`authoring_pipeline.py`及来源绑定/新增阶段并发测试，约4文件。
验证：`test_authoring_source_cache.py`、`test_outline_source_binding.py`、新`test_authoring_stage_concurrency.py`。

### F9.4b S受限并发，F9.4c R多包并发

- [x] S利用同client既有并发参数，默认2；真实样本R仅单包，保留原实现，不为并行额外拆包。
- [x] 固定回包下请求内容/数量/最终顺序相同；重复摘要只发送一次，局部错误不串兄弟身份。
- [x] 普通包耗尽保留gap和兄弟；取消/失租/存储错误停止新发送并结算在途；恢复成功包零重发。
- [ ] 仅后续真实多包R具有净收益证据时，单独实施顺序归并的R并发切片。

依赖：S依赖F9.4a，R依赖固定复核输入及多包测量。范围：每片pipeline、新并发测试及现有execution测试，约3—4文件。
验证：新增并发反例、`test_authoring_review_scope.py`、`test_authoring_coverage_review_fixes.py`；PG运行`acceptance/test_document_source_recovery.py`、`test_document_plan_recovery.py`、`test_document_execution.py`。

### F9.5a 新关系ID由宿主派生

- [x] 完整抽取回包按冻结关系顺序派生正式ID；关系文本/端点/证据/数量不变，不顺手改全局持久Relation类型。
- [x] 原BUG-077回包离线恢复；非法端点和重复事项ID仍保留纠正/问题，不能猜测映射或去重业务关系。
- [x] 输出合同、Prompt及冻结版本一起更新；旧执行不热替换，独立核验仍完整执行。

依赖：F9.0b；先于缺集合纠正。范围：`llm/contracts.py`、`requirements/extract.py`、extract prompt及2份测试，约5文件。
验证：`test_output_contracts.py`、`test_analysis_response_recovery.py`、`test_analysis_extraction.py`、`test_wire_references.py`。

### F9.5b 输出成本精简，F9.5c 补缺集合纠正

- [x] 现成回包字段成本及可行性评估：关系ID省量有限，其余事项text多数不等于quote；缺少准确缺集合候选的成本/质量对照，暂不启用进一步精简或补集合协议，依据见交接。
- [ ] 先按字段证明可精确恢复的重复；不把语义text当原文quote，不删条件/例外/必要证据，不重做已有短引用。
- [ ] 仅完整JSON且items/引用可验证时冻结候选，补明确缺失集合，完整合并校验后原子采纳；截断/歧义/缺items走原出口。
- [ ] 输入超容量或补充失败仍消耗原逻辑单元剩余额度；不提前暴露候选、不重置预算；已采纳及完整成功回包恢复零重发。
- [ ] sent/uncertain沿既有围栏及授权重试，未知用量不计零；不承诺供应商只执行或计费一次。

依赖：F9.5a及真实样本离线可行性。先合同/纯合并约5文件，再recovery/nodes/extract及恢复测试约5文件；不同时实现通用Patch。
验证：已有response/contract/output测试；拟新增`unit/test_extraction_partial_recovery.py`、`workflow/test_extraction_partial_recovery.py`，并回归work-unit/replay围栏。
完成出口：只有总调用/输入/输出/真实质量综合有收益才启用；不适用或超容量明确保留原有限纠正。

### F9.6 分包与合批校准（条件试验）

- [x] 核对启用条件：尚无新的可靠传输基线及1/2/4同依据净收益对照，保留当前分包和4目标上限；不把规划完成当成试验通过。
- [x] 完成旧真实台账固定完整依据的1/2/4目标拆分容量试算，核对全部目标、成员及发现配对覆盖；仅有输入/输出预留与请求数对照，未据此更改生产合批，真实质量/延时仍待验。
- [ ] 同一依据分别比较1/2/4目标关联及joint完整成本；另一次试验比较抽取输出负载分包，禁止同时改变两项。
- [ ] 每个候选列完整调用公式、最大/累计输入输出及修订失效范围；完整来源/表头/共同条件/配对覆盖不减少。
- [ ] 只有超过测量波动且质量不退化的方案才落地；无净收益保留原实现并记录结论，不扩大到8目标或取消独立核验。

依赖：F9.0、可靠性基线及相应F9.5成本数据。先1—2个试算文件；胜出后packing或analysis_batches/budget与测试单独成片。
验证：`test_analysis_packing.py`、`test_analysis_locate_packing.py`、`test_analysis_relation_batching.py`、`test_analysis_batches.py`、`test_batch_budget.py`、`test_batch_wire.py`。

### 检查点F9-B：技术与性能对照

- [x] 已实施切片的冷/恢复、固定延迟并发及完整产物对照保存；定向与完整unit通过，隔离workflow/acceptance全量扫描的旧断言失败项已精准修正复验，详情见交接。
- [x] 纯调度/缓存内容等价，协议修改列清语义差异及证据；失败更快、Token更少或单次更快均不能单独判真实验收完成。
- [x] 隔离测试与禁网回放核对旧身份/回执保全，旧Word未改；必要integration未以缺环境skip计通过，候选优化条件不足时保留原实现。
- [ ] 当前新版完整A0/A1及同文件真实性能/Word对照仍按F9.3/F9.7执行，不由上述技术检查代替。

### F9.7 同文件真实验收与分批发布

- [ ] 同原件、同模型端点、同60秒、同并发，从头分析与失败恢复分别验证；编制阶段另列，旧98分钟续跑不冒充新运行基准。
- [ ] 完整原文、事项、条件/例外、技术行、共同条件、narrative正文、分卷/标题/原表/图片/分页审计满足F8 C1—C10；人工未决和空应答/报价保持。
- [ ] 发布前核对新旧冻结runtime和接单边界，准备停止新版本接单及保留其执行恢复的回滚；实际部署/真实结果只更新交接。

依赖：检查点F9-A/B和F8相关内容修复。范围：现有验收/发布流程及记录；不新增业务节点。
最终完成标准：速度/失败成本的有效改善、完整业务验收和可恢复交付分别有证据；外部故障或已知正文缺口未关闭就如实保留未完成。

### 验证命令约定

在`services/docreader`执行。本机已存在`.venv/bin/python`；CI继续使用仓库冻结依赖环境。
新测试名是拟新增文件，落地后才加入命令。例：

```sh
KB_PERF_EVIDENCE="$(mktemp -d /opt/workspace/kb-tender-evidence/perf-repair-XXXXXX)"
.venv/bin/python -m pytest tests/unit -q --basetemp="$KB_PERF_EVIDENCE/unit-tmp" --junitxml="$KB_PERF_EVIDENCE/unit.xml"
```

PG/Redis测试须先按runbook提供独立测试库的`TENDER_TEST_DATABASE_URL`、`TENDER_TEST_DSN`
及隔离Redis DB，不能使用共享业务库/清理已有任务。配置完成后执行：

```sh
.venv/bin/python -m pytest tests/workflow tests/acceptance -q --basetemp="$KB_PERF_EVIDENCE/integration-tmp" --junitxml="$KB_PERF_EVIDENCE/integration.xml"
```

每片先运行其定向文件，检查点再运行完整受影响套件；无代码变化不重复刷测。真实模型及
独立Word/ZIP审计另跑并独立记账，所有运行证据留在上述仓库外目录。勾选只对应明确
完成的源码/离线能力，不代表部署或付费真实流程；实际执行状态与报告见handoff.md。
