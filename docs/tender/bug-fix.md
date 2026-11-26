# 招投标活跃问题与修复记录

本页只保留**当前未关闭、待真实验证或仍被活跃路线图引用的缺陷**（下一编号：**BUG-343**）。
已闭环的历史缺陷记录保存在 Git 历史，不在此长期堆积；跨模块高频防错规则沉淀在 [LESSONS.md](../../LESSONS.md)，动态运行状态只维护在 [交接](README.md#当前状态交接)。

## 维护约定

- 发现新问题按编号顺序追加（现象、影响、根因、改法、验证）；根因未查实明确标待查。
- 彻底修复、通过验证且不再被活跃计划引用后，从本页移除（Git 历史完整留存）。
- 离线单测、隔离环境回归、真实招标文件验收分别写明，严禁互相替代；私有证据放 `/opt/workspace/kb-tender-evidence/`，禁止放 `/tmp` 或提交原文/密钥。

```markdown
### BUG-XXX：问题名称

- 状态：待定位 / 已修待实测。
- 现象与影响：……
- 根因：已确认事实；尚未证实部分单列。
- 改法：已采取或计划采取的修改与约束。
- 验证：复现、回归、部署和真实文件结果；未完成的检查。
```

## 根因复审核心决策（长期有效）

| 决策 | 内容 |
| --- | --- |
| D1 问题分级 | 宿主可确认的来源缺失、不可读、作用域、结构、覆盖、预算问题是硬阻断，只能补源或修订。模型 finding 是语义疑点，人工逐条处理，不为此重跑核验。 |
| D4 抽取处置 | 模型只声明 `non_item` 和 `unresolved`。有事项的范围由宿主按证据派生；既未引用也未声明的范围由宿主生成问题交给 review。 |

## 活跃问题

## BUG-249 Bidding 原生字形软连字符占位符 `\ufffe` 触发 Word XML 导出失败

- 状态 / 发现阶段：导出写入已过滤 XML 1.0 非法码位；冻结台账未改。真实 Bidding 导出尚未重跑。
- 现象与影响：Bidding 大纲在 `kb-literal-candidate` 恢复成功后，执行 `post-outline-export-Aytf4ruN/export.py` 在 `authoring_export.py` 第 1462 行 `document.add_paragraph(issue["detail"])` 抛出 `ValueError: All strings must be XML compatible: Unicode or ASCII, no NULL bytes or control characters`，导致 `completed-bidding` 未能生成 `template.docx`。
- 根因：`Bidding.pdf` 物理第 21 页（`page_ordinal: 20, char_index: 626`）行末连字符在 PDFium 中返回 `pdfium_unicode: 2`、`text: '\ufffe'`（`U+FFFE`），进入冻结单元 `literal:page:20:d5cdfc3ddfdc9e065f63`（`“业务系统-电子招投标\ufffe参与的项目-招标业务管理-标前质疑”`）及大纲未归属待核项 `issues[699]['detail']`。`U+FFFE` 属于 XML 1.0 非法字符，`python-docx` / `lxml` 写入 `<w:t>` 时直接拒绝。
- 解决方案：Word 导出写入段落和单元格前去掉 `U+FFFE` 等 XML 1.0 非法码位。冻结源与 JSON 台账保持原字节。
- 验证：`test_export_xml_text.py`；真实 `BiddingFile.pdf` 离线宿主目录计划整稿导出与 `acceptance_eval` 通过（原式表全匹配且无 XML 非法字符报错）。含模型分析台账的完整 `check_bidding.py` 复跑尚未执行。

## BUG-250：原文原子未列全时整包拒收并再发模型

- 状态 / 发现阶段：已修宿主接受路径；未部署，未用真实文件重跑。
- 现象与影响：原生正文包只要漏一个来源原子，就以 `Disposition coverage must preserve the complete supplied source denominator.` 拒收。111 第 7 包因此在纠正后仍整包失败，并占用下一次模型发送。
- 根因：覆盖分母是宿主已知的冻结原子。模型漏报被当成合同错误，而不是待核。
- 解决方案：宿主把漏掉的原子补成 `unresolved`，已核定正文补成 `already_carried`，部分核定补成 `reference`。已标为要求但没有归属的原子按来源组件记成待核案例。需要另一次 C 判断才能定章节的赋值改为 `unassigned` 并留下案例。联合 C 和响应表单同样把漏掉的案例或原子补成待核。R 选择里漏掉的单元或岛补成 `unresolved`；声称已选中但没盖住冻结原字的，清空该正文并记 `native_body_anchor_uncovered`，不再整包拒收。重复引用、未知编号、未使用的依据组、上下文环和正文与上下文重叠仍拒收。
- 验证：原生来源、联合分类、响应表单和 R 选择相关单测通过。真实 111 包尚未重放。

## BUG-259：改一个 Prompt 会让无关执行的身份失效，且没有运行级费用上限

- 状态 / 发现阶段：已修（身份范围与运行级上限），离线回归通过。架构评审。
- 现象与影响：分析和编制的执行规格都带 `budget.spec()`。只改一个任务的 Prompt 或输出契约，用不到它的执行身份也会变，在途执行无法用新镜像续跑，只能新建。另外只有单请求预算和「每个请求最多 4 次」，没有按运行或按文件的总调用数、总 Token 上限。
- 根因：原条目写的「默认载入所有任务的 Prompt」不准确：分析只带 `extract`、`verify`、`ocr`，编制只带编制 Prompt。实际耦合有三处：`output_contract_spec()` 不分任务，把分析和编制的全部输出契约（含已退役的 `LocateResult`、`AssociationResult`、`Plan`）都算进身份；大纲执行和固定大纲的正文执行共用全部 23 个编制 Prompt；`budget.prompts.get(...) or 读文件` 的回退可以调用未计入身份的 Prompt。预算只设计了单请求和单逻辑请求两层。
- 改法：执行身份只含本执行会调用的任务，规则见[字段字典](field-dictionary.md#调用预算恢复与修订)第 6 条，五问见[业务工作流](workflow.md#执行身份范围)。输出契约按 Prompt 登记，未登记的 Prompt 不能进入身份；编制按大纲、正文、整稿取任务集合；回退改为统一入口，带编制 Prompt 的执行缺某个 Prompt 时报配置错误。运行级上限按阶段计划累计：各阶段按任务登记计划逻辑请求数，调用与 Token 各不超过计划的 2 倍，规则见[字段字典](field-dictionary.md#调用预算恢复与修订)第 7 条。
- 验证：`test_execution_identity_scope.py` 覆盖：分析身份只含分析契约；编制身份不含分析契约；正文执行身份不含大纲 Prompt 和契约；改 verify Prompt 时编制身份不变；身份外 Prompt 报错。上线会让现有在途执行身份一次性改变。续核：`bid_format_outline.md` 原未登记，一直经回退读文件、不在身份内；加检查后该路径会报配置错误。已登记为大纲 Prompt，并加用例要求每个打包 Prompt 都属于某个身份。运行级上限：`test_authoring_store.py`（PostgreSQL）覆盖未登记任务首次即拒、调用超过计划 2 倍被拒且缓存复用不受影响、Token 超过计划 2 倍时拒发重试、未启用策略的执行不受限；`test_authoring_pipeline.py` 覆盖写作在登记计划后才发送、被拒单元留具名占位且文档照常导出；`test_document_execution.py` 覆盖真实 Store 拒发未登记任务、不发 HTTP、结果带 `execution_budget_exhausted`。去掉 Store 核对与写作计划登记后这些用例失败。
- 遗漏：W3.1 修复时没有同步隔离 PostgreSQL 验收 `test_explicit_authoring_tokenizer_survives_execution_budget_reconstruction`，它仍按全部 Prompt 比对已存预算，4 个参数组合失败。期望改为按 `execution_prompt_names` 取本执行的 Prompt 子集。回退期望会重现 `output_contracts`、`prompts` 两项不一致。

## BUG-260：传输故障与合同纠正共用重试额度，429 时并发槽位互相抢占

- 状态 / 发现阶段：已修，离线回归通过；架构评审（外部分析复核）。
- 现象与影响：`llm/recovery.py` 的 `call_validated` 对网关 429、5xx 和断连（`ModelInfrastructureError`）、JSON 语法错误（`ModelJSONError`）以及宿主合同拒收共用同一个 4 次额度。连续几次 429 就能耗尽本该留给语义纠正的次数。客户端层面没有共享的限流冷却：BUG-011 的记录显示，一个工作单元连续 4 次 429 耗尽时，另一个并发槽位正好在发新请求，抢占了同一个 Key 的并发额度。
- 根因：恢复层只按逻辑请求计数，没有区分故障域；退避只作用于单个逻辑请求，不作用于共享同一个 Key 的全部请求。
- 改法：合同纠正与传输重发分开计数（纠正 2 次、传输 4 次），规则见[字段字典 · 失败执行的显式恢复](field-dictionary.md#失败执行的显式恢复)；分析与编制两个 Store 的发送围栏按完整请求摘要区分两类，重启后从请求台账恢复。同一进程内某端点收到 429 后，该端点全部槽位等冷却结束再发，见[业务工作流 · 运行时接口合同](workflow.md#运行时接口合同证据层与模型层)。`retry_policy` 改为 `split_transport_contract_v1`，在途执行身份一次性改变。
- 验证：`test_contract_recovery.py` 覆盖 3 次 429 后仍有完整 2 次纠正、传输重发跨纠正累计；`test_rate_limit_recovery.py` 覆盖一个槽位收到 429 后另一模型客户端等满冷却才发；`test_work_unit_attempts.py`（PostgreSQL）覆盖同摘要 5 次发送加 2 次纠正后第 6 次重发被拒、第 3 次纠正被拒。回退恢复层、请求模块与两个 Store 后上述用例失败。

## BUG-298：原式折行标题在 Word 目录中被截断

- 状态：已修待真实文件复验（模型选择路径与宿主目录计划路径均已修，离线回归通过）。
- 现象与影响：监狱企业条件式样等跨行标题在原件上折行，其中首行括号已闭合或无括号的标题（如「属于监狱\n企业的证明文件」或「其他商务\n及技术文件」）在目录和正文标题中被截断在首行，余下的尾行落入正文，读者无法从目录辨认完整文件名称。
- 根因：目录编译 `_continued_heading` 仅在标题括号尚未闭合时拼入下一行。模型选择路径（`bid_format_source_outline_v1`）最初未下传 `heading_lines`；宿主自编译路径（`bid_format_plan`）的 `_join_wrapped_heading` 对已匹配 `_single_form_title` 但未结束的长标签过早返回，且 `_single_form_title` 长度上限 30 字截断了折行完整名称，也未向节点、目录条目与列表式样窗口透传 `heading_lines`。
- 改法：模型选择的多行标题把行数随节点和目录条目传给编译器（`heading_lines`，1 至 4 行且在同页），编译器按所选行拼接标题。宿主 `bid_format_plan` 路径对未完标签按同页连续行继续拼接（最多 4 行，防正文序号句误入），将完整结尾称号的字符上限放宽至 45 字，并在节点、目录条目与列表项窗口透传 `heading_lines`；未给 `heading_lines` 时编译层仍沿用括号规则。
- 验证：`test_phase1_model_keeps_each_listed_file_and_each_wrapped_title`、安服第六章真实回包重放与 `test_bug298_host_plan_*` 回归覆盖多行折行标题（含闭括号、无括号、尾随编号式样、跨页护栏与 Word 导出正文去重）。待完整真实文件复验后关闭。

## BUG-313：表格序号混入事项的数量字段

- 状态：已修上游抽取与核验 Prompt，待全量真实文件重跑复验。
- 现象与影响：物理第 36、37 页的应急保障和安全检测工具事项分别把序号 6、10 写入数量字段；物理第 72、73 页的软件授权采购清单也把序号 4、6、8 写入数量字段。消费数量字段时可能把行号当成分值或服务数量。
- 根因：`extract.md` 对 `numbers` 未声明排除表格序号/编号列，导致表格序号与业务数值处于同一引用范围时被模型一并投影为数量。
- 改法：在 `services/docreader/tender/prompts/extract.md` 与 `verify.md` 明确规定表格行序号/编号列（如 `序号`/`编号`/`No.`）不得写入 `numbers`，仅记录真实业务量值、阈值、周期、比例或分值；核验阶段对误写序号报缺。
- 验证：离线 Prompt 契约回归与真实安服受影响分包实跑验证；待全量重跑闭环。

## BUG-314：未经允许转包的合同后果错绑到另一解除条件

- 状态：已修上游抽取与核验 Prompt，待全量真实文件重跑复验。
- 现象与影响：物理第 48 页 §15.4 规定未经允许转包或分包时的赔偿、违约金、可解除和退还。第 49 页 §17.6 另规定严重违约且协商无效时可以书面通知解除。抽取关系把前者挂成后者的具体情形，导致独立后果被错误附加条件。
- 根因：`extract.md` 对 `relations` 未明确禁止仅因后果相似而关联独立条款。
- 改法：在 `extract.md` 与 `verify.md` 明确仅当原文显式交叉引用或从属限定时才建立 `relations`，禁止把后果相似的独立条款（如不同合同解除情形）错绑为从属关系。
- 验证：离线 Prompt 契约回归；待全量真实文件重跑核对 §15.4 与 §17.6 关系。

## BUG-316：授权清单的技术量值因字形断开而保留错误空白

- 状态：已修 PDF 解析器合成空格根因及抽取/核验 Prompt 折行数值规则，待全量真实文件重跑复验。
- 现象与影响：物理第 73 页漏洞扫描系统的最大 IP 资源数在窄数量格内上下排为 `1000\n0`，台账存为带换行的数值字符串；SQL 审计速率在原件连写为 `100000SQL/S。`，但解析后的单元格与台账出现错误空格 `1 00000SQL/S。`（同页及邻页多处中文单元格也混入错位空格，如 `9个月的使用授 权`、`内 网`）。
- 根因：两处叠加——（1）`services/docreader/parser/pdf_parser.py` 的 `_page_chars` 保留了 PDFium 合成空格（`FPDFText_IsGenerated == 1`），这类合成空格在 `FPDFText_GetCharBox` 中坐标错位且为零面积点框，会落入相邻列字形之间（如落到 `1` 与 `0` 之间）；（2）`extract.md` 未说明窄单元格折行数字需在语义值还原、仅在引文中保留原格换行。
- 改法：（1）`_page_chars` 过滤 `ch.isspace()` 且 `FPDFText_IsGenerated == 1` 的 PDFium 合成占位空格（保留原件真实空格字形与 `_join_line_glyphs` 间距推算）；（2）`extract.md` 与 `verify.md` 明确窄格折行数字须在 `text` 和 `numbers[].value` 还原真实数值，同时在 `evidence[].quote` 和 `numbers[].quote` 保留原格字面子串。
- 验证：离线 `_page_chars` Red→Green 单测与真实安服第 72–73 页解析及实跑验证；待全量重跑闭环。

## BUG-318：履约交付的应急预案归入投标提交文件

- 状态：已修上游抽取与核验 Prompt，待全量真实文件重跑复验。
- 现象与影响：物理第 75–80 页（含第 77–78 页《网络安全事件应急预案》《应急响应报告》《应急演练方案》《应急演练总结》）属于服务履约/项目验收阶段交付成果，抽取将其归为 `submission_requirement` 并写入 `submission.documents`，导致投标待交清单混入履约阶段成果。
- 根因：`extract.md` 未明确区分投标/开标阶段提交件与中标后履约/实施/验收阶段交付成果的类别边界。
- 改法：在 `extract.md` 与 `verify.md` 明确规定仅投标/开标阶段需随投标文件提交的表单、证明或样例使用 `submission_requirement`（及 `submission.documents`）；履约/实施/验收阶段交付成果归为 `obligation`，除非招标文件明确要求在投标文件中附送样例或初稿。
- 验证：离线 Prompt 契约回归与真实安服受影响分包实跑验证；待全量重跑闭环。

## BUG-319：小微企业价格扣除事项缺少本条政策依据

- 状态：已修上游抽取与核验 Prompt，待全量真实文件重跑复验。
- 现象与影响：物理第 33 页第十五条明确用两份政策文件引出 10% 扣除。该处事项保留比例和优惠对象，但 `evidence` 遗漏了前置政策依据行。
- 根因：`extract.md` 未提醒跨行前置政策/法律依据须纳入同事项 `evidence`。
- 改法：在 `extract.md` 与 `verify.md` 明确当规则依据的政策文件、法律条款或适用前提位于同条款前置行/范围时，必须将该前置范围一并纳入 `evidence`。
- 验证：离线 Prompt 契约回归；待全量真实文件重跑核对第 33 页第十五条引证完整性。

## BUG-320：第三方盖章文件的举例被列为必交材料

- 状态：已修上游抽取与核验 Prompt，待全量真实文件重跑复验。
- 现象与影响：物理第 24 页 §16.2 要求在招标文件另行要求第三方出具盖章件原件时使用其扫描件，并举联合协议、分包意向协议、制造商授权书为例。抽取将三个括号举例写入无条件 `submission.documents`，在投标待交清单中引入不适用材料。
- 根因：`extract.md` 未约束总则条款中条件性/举例性文件名称（`如…`/`例如…`/`等`）不得写入无条件 `submission.documents`。
- 改法：在 `extract.md` 与 `verify.md` 明确规定签署/盖章/扫描总则中的举例性或条件性示例不得写入无条件 `submission.documents`，须保留触发条件于 `text`/`conditions`，仅将本条直接规定必交的文件写入 `submission.documents`。
- 验证：离线 Prompt 契约回归与真实安服第 24 页受影响分包实跑验证；待全量重跑闭环。

## BUG-331：新链格式章节选择超限时不分片，整次执行失败

- 状态：已修待实测；离线单测通过，未在真实文件上触发。
- 现象与影响：W2 五问写的是结构步骤「装不下按章拆」，`select_bid_format_plan`（`services/docreader/tender/bid_format_outline.py`）却只组装一个请求；格式章节标题行超出准入预算时 `ModelBudgetError` 不被捕获，整次大纲执行失败，不产出大纲也不留待核。格式章节很长或附件编号很多时触发。
- 根因：调用处没有捕获准入失败，也没有分片逻辑。回退测试已复现 `ModelBudgetError` 抛出。
- 改法：整份标题行装得下时请求不变；装不下时按原文顺序二分出能装下的最长前缀，切点退回到最后一个附件编号或编号式样标题前，每片 1 次逻辑请求，只回本片标题且不回 `body`，宿主合并后按整节规则统一校验。失败分片的标题不进大纲并记 `outline_directory_plan_failed`；全部分片失败时仍按逻辑请求耗尽失败。规则写在[业务工作流 · 新链格式补位与事项归属](workflow.md#新链格式补位与事项归属)。
- 验证：`test_bug331_*` 两条 Red→Green：回退改动时抛 `ModelBudgetError`；修复后 60 份附件按多片发送、按原序合并成全部章节，某片失败时其余章节保留并记待核。未做：真实大型格式章节实发。

## BUG-332：新链格式章节只读主件，补遗与澄清的格式表不进结构

- 状态：已修待实测；离线单测通过。本地没有真实补遗或澄清样本，三种形态都用合成文档验证。
- 现象与影响：`_format_lines`（`services/docreader/tender/directory_compile.py`）跳过 `role` 不是主件的文档，`_format_anchor` 只选一个锚点。补遗或澄清新增、替换的投标文件格式不进大纲，格式只在附件文件里时大纲为空。编译层只记一条笼统的 `outline_directory_revision_unreviewed`，不指出是哪张格式表。这与 W2 试点场景 3「未确认分析下的跨文档补遗隔离」冲突。
- 根因：格式章节识别只为单一主件设计。另外，编译层要求一份目录计划的组成清单只在一份文档内，两份计划之间是择一表决而不是合并，所以多文档的格式章节无法放进同一份计划编译。
- 改法：规则写在[业务工作流 · 新链格式补位与事项归属](workflow.md#新链格式补位与事项归属)。覆盖三种形态：（1）补遗或澄清自带格式章节；（2）格式章节只在附件文件里；（3）补遗或澄清不带格式章节、内联替换某张已有原式，经分析台账里证据在补遗或澄清的提交类事项发现。每份文档各找格式章节，各自发请求，复制窗口不跨文档；补遗和澄清的章节记来源，不同文档的同名格式各章都保留并记冲突。合并后的计划在编译前按文档拆开，各自编译后按登记顺序合并，重复的宿主问题只留一条，已被引用的补遗或澄清不再报未审。
- 验证：`test_bug332_*` 共 4 条 Red→Green（形态 1、形态 2、形态 3 的补位单测和端到端新链各一条）：回退改动时补遗格式不进大纲、附件格式不发请求、内联替换不补章；修复后全部出现，冲突和来源问题可见。只有主件时，请求与修复前逐字相同。未做：真实补遗或澄清文件（L3）。

## BUG-333：新链事项归属每批都带完整位置目录，目录过大时整次执行失败

- 状态：已修待实测；离线单测通过，未在真实文件上触发。
- 现象与影响：`placement_payload`（`services/docreader/tender/item_placement.py`）每批都带全部位置。`plan_batches` 遇到首条事项连同完整位置目录就超预算时抛 `ModelBudgetError`，它在 `place_items` 的批次异常捕获之外，整次大纲执行失败。位置目录本身超限时所有事项都触发。
- 根因：分批只按事项切，单条装不下时直接抛错而不是留待核。回退测试已复现。
- 改法：单条事项连同位置目录仍装不下时不发送，与未采纳批次合并记 `outline_item_placement_failed` 留待核，其余事项照常分批。未拆分位置目录：事项的证据页在招标要求章节而不在格式章节，按页把事项路由到目录分片没有依据；目录拆分需另定规则，在真实文件出现目录超限前不做。
- 验证：`test_bug333_*` Red→Green：回退改动时抛 `ModelBudgetError`；修复后超大事项留待核且其余事项照常归属，位置目录整体超限时 0 次发送、全部事项留待核。未做：真实大型大纲实发。

## BUG-335：全角括号后的未引用签署人姓名仍被写入投标稿

- 状态：已修。离线回归通过。当前空位稿没有写作段落填入这两行姓名。
- 现象与影响：写作段落里的 `法定代表人（签字或签章）：王五`、`单位负责人或其授权代表（签字）：李四` 会原样进入 DOCX。没有企业资料时，签署人姓名不应落稿。
- 根因：`_SIGNATORY_NAME` 只把半角 `)` 当作签署标签和冒号之间的可选括号，全角 `）` 对不上，名字留在句中。
- 改法：签署标签后的可选右括号同时接受半角和全角，未引用材料时仍删掉后面的姓名，标签本身保留。
- 验证：`test_missing_enterprise_deposit_and_signatory_stay_blank`。回退正则时断言失败，稿内仍有 `（签字）：李四`；修复后半角与全角标签都保留，`李四` 和 `王五` 不出现。

## BUG-336：投标文件一级目录被改成阿拉伯数字，并截短招标目录原文

- 状态：已修（并随 BUG-337/338 升级为完整多级目录与标题保真）。离线回归与当前空位稿重导通过。
- 现象与影响：导出目录写成 `1 投标书`、`7 投标保证金证明`、`31 …其他技术文件`。招标文件第六章目录是中文序号，且五为「投标保证金证明及退款申请函」、十三为「其他商务及技术文件」。序号样式和条目原文都不符。
- 根因：章名只用了格式正文的文件名，目录再按扁平章节从 1 编号。招标文件正文前的格式目录没有被当作一级目录。
- 改法：冻结来源里「投标文件格式」的连续目录条目存在时，一级目录和对应章标题照抄这些条目的中文序号与原文；子编号式样挂为二级/三级子章并在多级目录按树深度缩进显示，不升成一级；没有这份目录时仍按原章节序号。
- 验证：`test_first_level_toc_copies_the_tender_catalog_numerals_and_wording` 与多级目录回归用例。页码域仍是「待计算」，要等版式计算后才换成页码。

## BUG-337：新链大纲完全扁平无分级，子编号式样与方案需求子节均未展开为二级/三级章

- 状态：已修待真实全流程复验。
- 现象与影响：安服空位稿 `outline-exported.json` 全部 32 章均在 `depth=0`（`children=[]`），Word 全部输出为平级 `Heading1`；`5-1`~`5-3`、`8-1`~`8-5`、`9-1`/`9-3`、`（一）`/`（二）` 与附属文件被升为一级章，且 `十一、项目服务方案` 将 144 条招标要求全部压在一个平铺目标里、没有任何按评标办法和采购需求展开的二级/三级子节。
- 根因：`bid_format_outline.py` 的 `BidFormatChapter` 缺少父子引用字段并硬编码 `'parent_id': None`；方案知识应答位挂载后缺少有界的方案子大纲生成阶段。
- 改法：不靠正则硬猜，改为通用「模型按招标原文定树 + 宿主强校验」：1. `BidFormatChapter` 新增 `parent_heading_ref`（支持 BUG-331 分片 `parent_context` 跨片父引用，宿主校验同文档、先序、存在且无环）与可选 `label`（须为所引原行连续子串且不含附件映射号）；2. 新增有界任务 `proposal_outline`，对规则数 ≥ 6 的开放方案章结合评标办法与采购需求生成二级/三级子章节树并完成短 ID 规则归属（超半窗自动降级为「1 次定树 + 半窗分批归属」，规则 100% 守恒，失败留单目标并记 `outline_proposal_structure_pending`）。规则见 [业务工作流](workflow.md#新链格式补位与事项归属)。
- 验证：`test_phase1_bid_format.py`、`test_proposal_outline.py`、`test_execution_identity_scope.py` Red→Green 回归覆盖。

## BUG-338：原式序号与大小写未在章节标题中保真，Word 正文出现双标题连写

- 状态：已修待真实全流程复验。
- 现象与影响：大纲章节标题丢失 `一、`、`5-1`、`8-1`、`（一）` 等原件序号；Word 导出先输出 `Heading1` 标题，紧接着下一段又原样抄录一遍带原编号的标题行，全文 31 处双标题连写且目录刷新后与真实层级不一致。
- 根因：格式选择未保留原行序号 `label`，目录编译覆写剥离前缀；导出层仅对一级目录做静态文本替换而未按树深度写 `Heading1`/`Heading2`/`Heading3`、未在自带原序号时关闭阿拉伯自动编号、也未去重已作标题输出的原式首行。
- 改法：`BidFormatChapter.label` 经宿主原行子串校验后进入章名，目录编译不剥离已验证原序号；Word 导出按树深度写 `Heading1`/`Heading2`/`Heading3`，目录按树遍历输出全部层级，自带原序号时禁用 `decimal` 自动编号，并跳过与已输出章标题等价的正文首行（含折行标题）且不对纯标题容器章写假「（原格式待补）」。
- 验证：`test_toc_pagination.py` 与 `test_phase1_bid_format.py` Red→Green 回归覆盖。

## BUG-339：格式章节外补位的提交件按物理页码插队到封面和第一章之前

- 状态：已修待真实全流程复验。
- 现象与影响：安服第 61 页合同附件 `无关联关系承诺函` 因页码小于第六章（第 85 页），被 `add_submission_forms` 插到大纲最前面，Word 正文第 1 节变成 `无关联关系承诺函`，把 `一、投标书` 挤到第 2 位。
- 根因：`item_placement.py` 的 `add_submission_forms` 直接按 `(document_id, page, line)` 比较位置插入候选章。
- 改法：格式章节外的候选提交件统一追加在主格式章节末尾（封面与格式章节之后），保持各自相对顺序；内联替换冲突章同样不插队到封面之前。
- 验证：`test_item_placement.py` 回归覆盖封面首位与格式外提交件不抢在第一章之前。

## BUG-340：事项归属返回字符串 `"null"` 拒收整批，且非格式页重复行产生大量 `outline_directory_literal_pending` 噪声

- 状态：已修待真实全流程复验。
- 现象与影响：安服实跑中模型对 7 条事项返回字符串 `"null"` 而非 JSON `null`，导致整批 90 条被 `decode_placements` 拒收、引发 57 条 `outline_item_placement_failed`；同时大纲 `issues` 混入 143 条 `outline_directory_literal_pending`，其中 138 条来自第 34–38、56、72–73 页等非格式页面。
- 根因：`decode_placements` 未将字面字符串 `"null"` 归一为 `None`；`compile_plans` 直接把整份文档的 `ocr_issues`（含全书版式行对齐待核）全部拼入大纲 `issues`，未按大纲实际复制窗口过滤。
- 改法：`decode_placements` 将字面 `"null"` 归一为 `None`（其余非法值仍严格拒收）；`compile_plans` 仅保留落入大纲实际复制窗口页码内的 `outline_directory_literal_pending`。
- 验证：`test_item_placement.py` 与 `directory_compile` Red→Green 单测覆盖。

## BUG-341：纯标题去重章节的首个目标丢失 Word 书签，导致定位和覆盖评测报缺

- 状态：已修，离线回归与安服多级大纲重导通过；待真实工作台全链路复验。
- 现象与影响：父章节首个 `form` 目标的 `purpose` 与章节标题相同时，导出器抑制重复正文及占位行，未在该章节生成额外段落，致使该 `target_id` 的 Word 书签未被写入；工作台位置跳转无法定位这些目标，`acceptance_eval` 报告 `docx_missing_targets` 并连带把已归属到这些目标的事项判为 `target_missing_in_docx`。
- 根因：`services/docreader/tender/authoring_export.py` 仅在目标循环新增 body 节点或渲染写作单元时写入 `target` 书签，`heading_only` 路径两者皆无且未回落挂载到章节标题段落 `heading_paragraph`。
- 改法：在 `_Assembler.chapter` 记录当前章节的标题段落 `heading_paragraph`，当 `heading_only` 目标未在正文新增节点且未挂接写作单元书签时，将 `target` 书签锚定到 `heading_paragraph`。
- 验证：`services/docreader/tests/unit/test_toc_pagination.py` 新增 Red→Green 用例验证 `read_docx_outline` 书签绑定与 `acceptance_eval` 零缺失；安服多级大纲样本离线重导验证 `missing_target_ids` 归零、已放置规则零缺失且原式表全匹配。待隔离环境重建并由真实工作台与 ONLYOFFICE 全链路复验。

## BUG-342：v2 文本抽取省略系统提示词 Schema 导致兼容网关下大量合同违例与丢项

- 状态：已修，待受影响分包对比实测与全量重跑复验。
- 现象与影响：实跑安服 5 个分包（`openai/gpt-5.6-luna`）共产生 36 条合同类阻断问题：3 条核心事项因非法 `kind` 整条丢弃（含项目经理与技术负责人资格要求）、12 条评分/义务事项误带 `submission`/`consequence` 或非法 `issues` 字段触发 `extraction_optional_fields_unresolved`、3 处已引用范围重复声明 `non_item` 触发 `extraction_disposition_unresolved`。
- 根因：`services/docreader/llm/request.py` 对 v2 `extract` 仅在 `response_format.json_schema` 发送非全必填 `strict: True` Schema 而跳过 `schema_prompt()`；当 OpenAI 兼容网关忽略或降级 `response_format.json_schema` 时，模型完全看不到 JSON Schema；同时 `TenderItem` 字段描述与 `extract.md` 未把 `kind` 枚举边界、字段互斥（如评分项附带证明材料应拆项或省略 `submission`）和已引范围禁写 `disposition` 写透。
- 改法：`build_request` 取消 v2 `extract` 独有的非严格 `response_format.json_schema` 特例，与其他任务统一为 `response_format={"type": "json_object"}` + 系统提示词内嵌单份同源 Schema（`extract_response_format` 记为 `json_object`，避免双份 Schema 挤占输入预算）；在 `TenderItem` Schema 字段描述与 `extract.md` 明确 `kind` 四值边界、`scoring`/`consequence`/`submission` 严格互斥及已引范围禁发 `disposition`。
- 验证：`test_generated_output_requests.py`、`test_analysis_packing.py`、`test_analysis_extraction.py` 回归通过；安服受影响 3 个分包前后实测对比中合同阻断项由 33 条降至 13 条（`pack-961a9cbe` 7→0、`pack-446dc741` 13→2 且项目经理/技术负责人不再丢项、`pack-96443e84` 评分项 `submission` 违例 11→0）。
