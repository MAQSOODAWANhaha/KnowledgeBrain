# 投标提取与 Agent 运行时实施方案：Chat Completions、Rig 与有界复核

2026-09-12 UTC 最新终态：`real-run-v19-repair-scope-resume5` 已于 12:43:50 UTC 结束，运行退出码 1，本段耗时 4427.34 秒；[固定终态](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/resume5-terminal-verification.json)为 turn1604（SHA `5e69cacf…`），424 条候选、177 条关系、104 条保存修复说明，有效处置 104/106、待处理 2，保留 3 处执行阻塞，完整独立复核 0 轮。错误 `AGENT_TURN_BUDGET_EXCEEDED` 指局部执行及独立工作交接额度耗尽；全文累计调用 3826/4000、尚余 174 次，并非总调用帽耗尽或供应商超时。未重启。有效处置不等于独立语义批准；完整 106 页、32 项语义及同版 DOCX/PDF/报告验收仍未完成。正确性与稳定性优先，速度优化后置。

主修复异议闭环：[宿主核查与默认 CI 回归](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/repair-dispute-closed-loop.md)确认生产已有 disputed→独立保留或撤回→完整来源复核及编制准入的闭环；本次仅补测试，覆盖受影响候选详情门槛、主角色不能自批和独立裁定分支。[验证记录](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/repair-dispute-closed-loop-verification.json)为最终断言加强前 217 项 tender_analysis 测试通过、36 项既有忽略，随后加强断言的新专项 1 项通过，严格 Clippy、全仓 fmt 与 diff 整合检查通过。这是合成脚本的宿主协议验证，不是 grok 的真实语义成功。[主方案](agent-runtime-rig.md)已记录“Main 修复任务隔离：turn1604 后续实施边界”，T1–T3 任务账本、Main 派发和既有 Journal 合同已进入代码整合与离线回归，尚未完成整合验收或部署，未授予新尝试；旧 turn1604 终态、检查点、阻塞及累计调用账本保持不变。

编制版式：[行内布局修复](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/inline-template-layout-implementation.md)已完成局部验证：同一冻结来源中连续文本区域保留原始换行和独立字段书签，示例填写值在原位置清除；跨来源、间隔和网格保持边界。固定 turn1571 的[真实候选前后 DOCX 对照](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/inline-layout-before-after.json)确认四个身份字段恢复同行、20 个区域定位均可回读。[整合验证](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/inline-layout-integration-verification.json)为 40 项编制测试通过、1 项既有忽略，严格 Clippy、全仓 fmt 与 diff 检查通过；无新增 migration，未改提取 schema 或配置。该局部诊断不是整单样稿，也不代表原页像素、完整 DOCX/PDF 或 R03/R06 验收通过。

终态局部核查：[A/E 四条处置复核](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/ae-repair-semantic-check.json)绑定 turn1604，四条均为 `revised` 回执刷新，所引用记录和关系已在 turn1231 存在且未变；引用目标有原文依据，但字段语义及重复关系风险仍在，不能计作四个新语义修复或独立批准。 [最后两项原文核查](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/remaining-ae-source-semantics-1604.json)区分 A 的有据缺失关系与 E 尚无具体前附表目标支持的修复要求；后者需要有源异议或准确未决并由独立复核裁定，不能强行补边。[终止轨迹](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/repair-tail-terminal-check-1604.json)未出现“全部修复完成却无法复核”的状态。以上人工诊断不发送模型，检查点、阻塞及调用额度保持原样。

历史工程状态（恢复范围完成校验修复部署时，下方检查数字不含本轮行内布局修复）：Main 完成范围与既有阻塞目标交叠时，只要该目标仍有未有效处理的问题，就拒绝 complete，保留目标 watch 并给出既有问题导航；覆盖合并、部分交叠及合法恢复后直接完成的路径。此前局部 v2、历史召回与候选身份索引导航保持生效。[最终联合检查](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/current-checks.json)的 320 项库测试、17 项 baseline、严格 Clippy、fmt、diff 及构建全部通过，39 项库测试仍忽略；[实现独立复核](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/implementation-review.md)与[迁移脚本独立复核](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/transition-review.md)完成，[实际部署核验](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/deployment-verification.json)通过。未改 Progress 算法、schema、静态提示词、`.env`、预算或检查点字段，未新增 migration；历史已删除 watch 不凭空还原。

历史恢复：resume4 停止于 prepared 边界 turn1147 / calls1150，[原边界恢复验证](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/resume-verification.json)通过；resume5 的[首个请求](../../artifacts/bid-full-sample/real-run-v19-repair-scope-resume5/startup-verification.json)保留原 98,478 字节正文。[turn1156 启动快照](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/started-progress-snapshot.json)与[turn1514 中途快照](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/progress-snapshot-turn-1514.json)仅为历史记录，已由上方 turn1604 终态替代。

独立局部语义核查：[R06 修复报告](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/r06-repair-semantic-check.json)严格绑定 turn1472，确认弱口令跨页完整文本、续项继承适用性、父项证明和响应义务、续项关系已补齐，关系端点版本匹配；“制行方式”来自冻结原文，并非此次修复新增错字。父子项仍各自声明同一证明响应，存在下游重复编制风险，须检查实际整稿；证明对象本身并未重复，不能预先断言 DOCX 已重复或 R06 整项已通过。

[R03 修复报告](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/r03-repair-semantic-check.json)严格绑定 turn1499，只确认新增一条与原网格证据相符的 requires_template 关系；四条相关既有记录未变，unknown 适用性/强度及附表归属仍未解决。原标题、单位、表外注释及跨页续文、签署虽已在 requirement，仍未进入模板 region 或建立对应关联；表内备注实际保留，不能误报全部注释缺失。未核查该版实际 DOCX/PDF，R03 完整验收未通过。两份局部核查分别绑定各自检查点，不扩大为 turn1604 或完整样稿已通过；人工诊断不发送模型。

固定 turn939 的语义预检仍单独保留：[R03 原文与网格证据](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/r03-939-source-evidence.json)及[诊断](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/r03-939-source-review.md)表明报价表同页标题、表外注释及续页签署来源已存在，但当时网格适用性与模板关联尚未完成；表内备注已保留，不能误报全部注释缺失。[R04–R07 技术预检](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/technical-r04-r07-precheck-939.json)确认若干正文/证明保留子断言，同时 R06 弱口令跨页记录仍缺连接。两项诊断只绑定 turn939，后续候选变化须重新核验，不能当作后续检查点的最终状态或已通过验收；人工诊断不发送模型。

2026-09-12 恢复范围完成计账方案（已实现、验证并部署）：[真实合并完成审计](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/history-expanded-completion-e-accounting-assessment.json)确认，E 原业务依赖未变且没有有效新修复处置，却随其他来源的完成丢失已消费 watch。该缺口不会自动批准 finding，但会错误释放局部执行额度。[独立方案复核](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/recovered-target-completion-design-review.md)选择在 Main 的既有完成校验中复用修复版本/来源判断：只要完成范围与既有阻塞目标交叠，且该目标仍有未有效处理的问题，就拒绝完成并给出原 finding 导航；正常无目标的局部工作、Reviewer 和允许拆分/defer 的路径保持原语义。保护覆盖合并、部分交叠以及先前 Blocked 但已有合法恢复条件时直接 complete 的路径，诊断与实际完成共用同一校验。此方案已随 resume5 部署；部署时暂停边界的 A/E blockers、watch 与 known/consumed 保留，历史已删除 watch 不追溯重建。工程验证与语义验收分别判断。

2026-09-12 resume4 历史恢复链：[B/D/E 完整历史交付与消费](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/history-recovery-transition-1000-1004.json)发生在 response1000 取回、response1001 完整响应确认后显式进入；[旧完成路径](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/history-recovery-transition-through1022.json)于 request1014 把合并范围内三处 blocker 一起移除，当时 E 没有新的处置。D 随后自行补全已删除对象的原 ref 并登记成功；history 是删除文字说明及原 ref，不是每个引用都有结构化 deleted 状态。[E 后续实际重入](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/e-post-completion-reentry-through1082.json)先证实旧 watch 丢失，response1027 又真实登记 offset37 并完成 E 局部工作；该局部完成不代表 finding41 已修复。request1046 E 再次阻塞，摘要是原业务 hash 加已消费恢复标记，不能说业务变化已解锁；后续独立 B 工作才读取前附表原表。新补丁保留该真实再阻塞状态，不能把后续全部重规划都归因于旧缺口，也不将范围完成当语义批准。

2026-09-12 UTC 历史状态（候选身份索引导航部署，10:28）：候选 ID 不存在或类别不匹配时，保留失败并给出既有 `inspect_analysis` 索引查询，由模型取回完整 ID 后再精确读取；不猜测或自动替换 ID，不自动执行查询或授予详情回执。此前主修复局部 v2、旧摘要兼容及历史只读导航保持生效。[最终联合检查](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/current-checks.json)的 315 项库测试、17 项 baseline、严格 Clippy、fmt、diff 及构建全部通过，39 项库测试仍忽略；本次 4 项普通专项测试、[固定 turn666 的实际 48,000 字节预算索引及精确详情投影](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/fixed666-index-projection.json)、[独立代码复核](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/identity-error-review.md)通过。[部署核验](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/deployment-verification.json)已完成；未改 schema、静态提示词、`.env`、预算或恢复许可。未知 source 错误不在本次修复范围内。

该次历史续跑记录：旧 `real-run-v19-repair-local-resume3` 已通过 SIGINT 停止于 prepared 边界 turn825 / calls827，原 5 处阻塞与 44 条修复说明完整保留，[真实边界无模型恢复验证](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/resume-verification.json)通过。新 `real-run-v19-repair-id-resume4` 已从原检查点启动（会话 65461 / PID 2278412）；[首个实际请求](../../artifacts/bid-full-sample/real-run-v19-repair-id-resume4/startup-verification.json)与离线恢复的 64,896 字节正文一致，`.env` 原 SHA、冻结来源及旧 journal 原档均未改变。

该次历史观测：运行进度引用[固定的 10:43:32 UTC 快照](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/progress-snapshot-2026-09-12T104332Z.json)：turn924 / calls927，全文累计 3146/4000；419 条候选、122 条关系、52 条保存修复说明，有效处理 44/106、待处理 62，保留 4 处执行阻塞，完整独立复核 0 轮。[E 范围只读诊断](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/blocker-e-source-navigation-diagnosis.json)表明查询反复停留在正文条款，尚未取回前附表目标及选择分支证据作完整比较；source ID 错误提示不能单独解决该问题，finding41 也未被判定错误或撤销。源码部署、保存说明及工程验证不代表语义通过；完整 106 页、32 项语义及同版 DOCX/PDF/报告验收仍未完成。正确性与稳定性优先，速度优化后置。

2026-09-12 并行核查补充：[C 范围解除证据](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/resume4-c-release-and-history-through924.json)确认，真实跨页关系刷新端点版本后才允许重入，随后提交异议并完成局部工作，request901 首次移除阻塞；这不是独立语义批准，B/D/E 的历史截至 request924 仍未完整交付。[E 独立复核](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/blocker-e-independent-review.md)确认实际请求携带现行提示与所需工具，尚未重现通用宿主缺陷。[固定 turn939 模板预检](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/template-precheck-939.json)及[独立核对](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/template-precheck-939-independent-review.md)确认：8G 续页日期仍被标为整段空白，若通过准入后保持此策略编译会删除固定日期标签；8H 仅第102页归属子断言成立；报价网格仍为未知适用性、不能落位，但其表内备注实际存在，不应误报所有注释均缺失。当前无完整独立批准及实际整稿，人工诊断不写回候选或发送给模型。

2026-09-12 E 原工具离线验证：[固定 turn939 的生产工具测试](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/e939-tool-reachability-verification.json)通过，零供应商请求。按实际 source_index 页码/表格导航取回 64 个完整网格单元格、跨页续文、选择项及 22 个当前候选详情；单次最大实际成功响应为 6220/48000 字节。[验证边界](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/e939-tool-reachability-review.md)限定为原工具调用及字节上限，未执行 Agent 上下文装配，也不证明模型自主发现或语义通过。临时测试已移除，未改生产代码、模型输入或运行状态；当前没有据此增加检索补丁的依据。

2026-09-12 候选身份导航真实使用补充（10:34 UTC）：`real-run-v19-repair-id-resume4` 第850轮请求已包含新错误导航，捏造的候选 ID 仍被拒绝；随后两轮成功读取索引，第853轮请求中出现成功的显式来源索引和精确详情读取。[实际调用证据](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/first-live-error-navigation.json)只证明已执行新错误分支及后续读取成功，不等于所有目标定位正确、修复因果或独立批准。另一个读取仍曾因 UTF-8 字节边界失败，来源身份与来源适用性问题继续分别定位。

2026-09-12 UTC 历史状态（09:28）：历史召回、有界恢复与按范围/阻塞状态选择待修复问题的导航已完成[联合验证](../../artifacts/bid-full-sample/loop-repair/repair-history/navigation/verification.json)；295 项库测试、17 项 baseline、严格 Clippy/fmt/diff 通过，37 项库测试仍忽略。`real-run-v19-repair-history-resume2` 正在运行（会话 87609），09:27 快照 turn519 / calls521，全文累计 2740/4000；420 条候选、87 条关系、25 条保存修复说明、3 处执行阻塞，独立复核 0 轮。此次已有实际候选修正和新端点关系，但一条 Rule 修改触发全局版本失效，有效处理数曾 23→0，现重新核查后为 18；[只读诊断](../../artifacts/bid-full-sample/loop-repair/repair-history/navigation/global-rule-invalidation/assessment.md)确认主修复说明与独立复核共用全局规则依赖，执行 blocker 的局部依赖却未变化，尚未实施该职责拆分。首条修正核心有原文支持，但新缩写与字段引用精度仍待复核。原检查点、已消费额度及 `.env` 配置均保留；32 项语义与完整 DOCX/PDF/报告验收尚未通过。

2026-09-12 本次依赖方案状态：已选择主修复局部 v2、独立复核保留全局规则的职责拆分。新摘要以固定域标记、冻结输入摘要及现有候选局部依赖值生成；`valid` 与 `main_history` 共用旧版/新版匹配规则，只能由模型正常提交 `put_repair_result` 写入 v2。候选本身、显式关联 Rule、关系端点、增删边及模板父级变化仍使相关说明失效；无关 Rule 不再使新局部说明整体失效。真实 456 检查点的[旧摘要核对](../../artifacts/bid-full-sample/loop-repair/repair-history/local-dependencies/legacy-review-hashes.json)保留 22 条匹配、1 条原先失效；[单条真实 Rule 替换投影](../../artifacts/bid-full-sample/loop-repair/repair-history/local-dependencies/actual-rule-local-projection.json)中，旧摘要有效数 22→0，假设正常新写 v2 的 22 条无关说明保持有效，原先失效说明不升级。该投影不修改原检查点，也不代表模型已重新登记或语义已通过。本轮已完成联合验证、真实边界无模型恢复核对及兼容部署；原请求、调用计数、blocker、watch、known/consumed 及删除标记按原合同保留，不新增字段、SQL、schema、静态提示词或配置。

2026-09-12 本轮身份导航方案：只给类型化候选身份错误附加现有索引入口，清除失败查询中的 `ids`，保留有效 limit 和显式 source_id；未指定 source_id 时沿用当前活动范围，无活动范围才使用全局索引。类别、source、limit 等其他参数错误不套用该导航，序列化错误响应须满足原工具结果上限。索引分页后仍须按完整 ID 取详情，不增加阅读、进展或恢复额度。[实现与验证](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/implementation-verification.json)明确未知 source 错误另行定位；本次没有新增工具、状态或配置，也不保证模型会遵循导航或判断正确。

本轮[有效处理数骤降的只读核查](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/handled-drop-assessment.md)确认：response723 修改一条 Rule 后，request724 的有效数 37→5 是 32 条 legacy 说明依照旧合同失效，之前正常新写的 5 条局部 v2 保持有效；[生产匹配器核对固定 turn739](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/production-receipt-matchers-666-739.json)为 9 条 v2 current、35 条继承的 legacy stale，后续回升来自模型正常提交，没有自动升级旧说明。第 5 处阻塞在 request703 已形成，早于此次 Rule 修改，不能归因为局部 v2 失效；相关未知 source 错误也未被本次候选索引补丁覆盖。

本轮[附表 7 角色语义复核](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/annex7-role-semantics-review.md)纠正此前“整段 fixed_text 不可编辑”的判断前提：非网格 `fixed_text` / `instruction` 保留原文，经 `quote` 输出普通可编辑 DOCX 段落；没有把每处现有空白独立切成 `bidder_blank` 本身不构成缺陷，后者会把整段引用替换为空段落，并非行内填写控件。原文完整性、示例值清空、填写空间与版式、跨页签字归属和实际 DOCX 仍待验；该人工诊断不修改模型问题或修复回执，不构成附表验收通过。

本轮[E 范围停滞的只读诊断](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/blocker-e-source-navigation-diagnosis.json)确认，完整 finding41 已反复交付，搜索返回的完整 source ID 也曾正确使用；两次编造 source ID 只是部分错误，主体轨迹是成功但重复的正文索引和字面搜索。实际前附表编制/签章及递交选择候选已经存在，却未在该时段取回其原表、跨页续文与精确详情。原问题引用的 3.7.3–3.7.5 位于“线下招标”分支，前附表则选择电子招标且不要求纸质文件；仍须逐条验证对应关系及适用条件，不能给相似电子条款强接关系，也不能据此宣布 finding41 错误。现有网格字面搜索、source_index/read_form 与精确候选读取可用于收集依据，`put_repair_result conclusion=disputed` 可记录有原文依据的主 Agent 异议，再由独立复核裁定；这些能力不授予已阻塞范围新机会，不替代读取回执或允许重置预算。source 导航提示只能改善错误恢复，不能作为 E 根因已解决的证明；本诊断未改运行合同或模型输入。

2026-09-12 09:28 历史依赖诊断（已由上述局部 v2 方案替代）：主修复说明目前直接复用独立复核的候选版本，后者包含全部 Rule；response459 的一条真实 Rule 修改因此使原修复说明全量失效，实体修改没有丢失。优先评估让主修复说明跟踪其具体纠错对象/关系/证据依赖，独立复核继续保留全局 Rule 失效；另一选择是主修复也要求全局重新论证，但必须配套真实新规则证据的有界执行恢复。旧 blocker 未保存 Rule 基线，不能通过安装新摘要公式、清除 blocker、重置 watch 或人工转换 receipt 就授予机会/通过。两种方案的旧回执兼容仍须单独验证，当前仅诊断，运行仍有重新登记进展，未据此暂停或实施新策略。

2026-09-12 修复导航纠错：优先当前 Active 范围内可执行的未处理问题；没有当前项时，给出独立范围的显式 `set_work_note` 导航。原 Finding 证据决定范围，原证据为空才依次使用旧修复证据/受影响候选定位，不把聚合候选的所有来源强加给单个字段问题。不可执行的问题另列但保留在 pending 总数中；无可执行项不等于修复完成。`inspect_review` 地址只用于缺失或已裁剪信息的精确取回，完整原文/候选/历史已在上下文时应直接判断并提交修复说明。这是动态导航修正，不增加任何恢复许可、不改静态提示词、工具、Config 或数据库合同。

2026-09-12 恢复策略修正：`inspect_review` 保留原 Finding，分页附带当前/失效的旧修复说明与精确候选导航；保存 finding/repair 前校验完整可取回预算。完整响应确认相关历史实际交付后，以既有 `Progress.seen` 保存首次已知、可用、已消费标记；同一问题/来源的旧历史不因说明改写或候选变化再变成新信息。显式 `set_work_note` 才能消费绑定角色、精确范围与业务依赖的一次机会，已消耗的 replans/focus 不因重启或切换范围重置，扩展范围失败也更新每个原目标的依赖摘要。这是明确的运行时纠错，不是单纯文件拆分；未修改 Config、提示词、工具 schema、SQL 或检查点形状，未给候选与 finding 自动通过。离线/真实恢复仅证明宿主边界，最终语义仍由独立复核及原文验收判定。

2026-09-12 UTC 并行结构审计：已迁出source_review/context/repair测试，context生产类型不再排在测试之后；source_review已独立为`tender_analysis/source_review/{mod.rs,tests.rs}`，旧agent路径和公开导出已删除。MAIN/REVIEWER迁到`crates/bidding/prompts/`，原提示词字节和SHA不变；图片读取迁到`agent/view_io.rs`，原函数体不变且无转发包装。FrozenInput入口复用现成docparser网格校验器，42张真实表离线通过，未重造解析/几何算法、未新增migration。[合并工作区验证](../../artifacts/bid-full-sample/loop-repair/repair-disposition-handoff/parallel-verification.json)为bidding273项、docparser48项、authoring schema3项通过，Clippy、全仓fmt与diffcheck通过；34项依赖特定归档/环境的ignored测试不计入通过。独立模块迁移另经56项source_review、1项runner及1项冻结配置恢复测试通过，14项模块归档回放与2项runner归档测试保持ignored。模块仍依赖Agent Checkpoint/context，仅必要访问扩大到tender_analysis内部，不代表业务完全解耦；[主tests.rs拆分证明](../../artifacts/bid-full-sample/loop-repair/repair-disposition-handoff/tests-physical-split/verification.json)：父文件已收至583行，105项测试迁入8个协议模块，原input/repair模块保留；父测试树117项及其中3项ignored清单不变，整个tender_analysis测试176项通过、31项ignored。此次P1物理拆分完成，其他生产职责仍待整理；结果获取接口的后续实施见下段。v19冻结二进制未更新，以上源代码整理不替换运行程序；截至07:47第104轮/105次本次调用，累计2324/4000，已保存13条主处理记录，候选419/关系40，0轮完整独立复核。另对第51轮最早5条主处理记录的只读抽查发现一条引用仅部分覆盖、一条修正引入新错字，人工诊断保存在source之外且不发送网关，必须在当前候选及后续独立验收中复查。主处理数量、并行工程测试均不代表106页/32项/DOCX/PDF已通过。

同次[发布与API审计](../../artifacts/bid-full-sample/loop-repair/repair-disposition-handoff/publication-api-audit.md)指出的两个获取入口已补齐：`GET /api/v2/bid-projects/{project_id}/requirement-sets/{requirement_set_id}/analysis?kind=all&offset=0&limit=100`按冻结要求集分页读取完整record类别；relation、disposition、finding须分别查询，`all`不代表整张分析图。`GET /api/v2/submission-workspaces/{workspace_id}/docx/versions/{version_id}/composition-report`下载该生成版本持久化的JSON报告，生成页已提供入口；后续手动上传版本没有编制报告时返回404，不回退到旧版本。旧`/requirements`及`/requirement-projection`仍为有损投影，生产DOCX编制继续读取完整`analysis_result`；quality须结合findings和来源open-items分别理解，不能单凭needs_review认定提取错误。[API整合验证](../../artifacts/bid-full-sample/loop-repair/analysis-report-api/verification.json)包含真实隔离数据库/HTTP的两个target、17项API单测及Clippy/fmt；[前端验证](../../artifacts/bid-full-sample/loop-repair/analysis-report-api/frontend/verification.json)为34项单测、10项浏览器fixture、build/lint/typecheck通过。这些工程验证不代替106页/整稿验收。Checkpoint全量clone和容量探测仍按正确性优先后置，未新增migration或修改v19冻结运行。

2026-09-12 UTC 当前实施状态（07:45 UTC，覆盖下方旧状态）：逐条修复约束及独立异议复核已通过[269项库、17项当前基线合同、7项隔离PostgreSQL、3项runner/归档来源测试及Clippy/fmt/编译](../../artifacts/bid-full-sample/loop-repair/repair-disposition-handoff/verification.json)。除“读完不等于处理完”外，已修复新异议可能复用旧来源判断直接结束的问题：主处理说明只使相关来源判断失效，复核详情携带该说明，原独立报告不被主Agent改写。v19于07:34:36启动，[首个真实请求](../../artifacts/bid-full-sample/real-run-v19-repair-dispositions/startup-verification.json)核对.env、grok-4.6、Chat、low及新工具/提示词摘要；此前2219次全文调用保留，本次1781次，全文总额4000及补充73/160不变。v18最新418候选/39关系与原已验证反馈依据分开保留，106历史模型意见重新核对；v18撤回的一条意见有独立归档，本次不继承旧通过回执。截至07:43为第66轮、67次本次调用、8条主处理记录、3条候选变化、0轮全量独立复核。主处理记录不是正确性证明；106页、32项语义和完整同版DOCX/PDF/报告仍未验收。用户授权后已并行迁出source_review/context及repair测试、逐字节提取提示词、迁移图片I/O；输入入口复用docparser现成网格校验器的最终回归进行中。运行使用冻结程序，不被这些并行源代码整理替换；速度仍后置。

2026-09-12 UTC 当前状态更正（07:17 UTC，覆盖下方旧运行状态）：v18已于06:47:44主动停止，第58轮、59次实际调用；累计全文2219/4000次，剩1781次，另73/160次局部对照不变。保存418候选/39关系、11来源判断、105条模型问题、0轮完整独立复核。106条旧反馈全部交付后，主Agent仍在第34轮只处理少量问题就提前交接；原表单标题、固定提示被填空覆盖及转录错误有整条候选未变的明确证据，不能把“已读”当“已处理”。[诊断及回归目录](../../artifacts/bid-full-sample/loop-repair/repair-disposition-handoff/diagnosis.json)记录逐条处理队列修复：主Agent必须保存实际修正或有原文依据的异议，修改结果须完成详情交付；无关改动不算当前修正，相关版本变化会使处理记录失效，独立复核仍决定正确性。现有检查点增加repair状态并升级baseline保存校验，不新增表或独立migration；新工具/提示词合同不可续用旧运行。266项库测试和4项隔离PostgreSQL检查已通过，另补充删除/同批读取/恢复回归通过；最终全套检查及新合同交接仍在进行，尚未冻结新程序或恢复模型调用。已完成的v18真实修正和旧反馈依据须分别保留，不能伪造新独立回执。全106页、32项语义及同版完整DOCX/PDF/报告未验收，正确性与稳定性优先，速度后置。

2026-09-12 UTC 新修复合同与完整问题交付（06:37 UTC）：旧v17于第537轮优雅停止，累计全文调用2160次，保留415候选/29关系、133来源判断、106模型问题、2处执行阻塞和未完成后段；没有完成独立复核。第96页另出现把8D-4当前网格引用用于8D-3问题的反复失败，现仅在原错误反馈补一处已保存问题的紧凑原文位置，明确不是阅读回执或该问题适用的证明；不自动修改参数，不放宽校验，不新增schema/提示词/字段/配置/migration。[真实轨迹与离线反馈投影](../../artifacts/bid-full-sample/loop-repair/mapping-source-feedback/feedback-replay.json)、[263库/20合同、Clippy/编译与模块fmt](../../artifacts/bid-full-sample/loop-repair/mapping-source-feedback/verification.json)通过；同次全仓fmt保留共享worker/src/knowledge.rs换行差异，本任务未覆盖其逻辑，不能宣称全仓全绿。已核对旧进程身份、记录两处已诊断阻塞和验证后的替换程序后停止，替代上一条等待所有独立来源结束的过渡安排；未完成来源没有视为已完成。v18导入经生产原文/候选回执校验的原模型问题和候选，重新检查完整106页/148来源/42网格，106原图文件摘要一致；独立回执为空，不继承通过结论。保留总额4000和此前2160次，剩1840次；[实际首请求](../../artifacts/bid-full-sample/real-run-v18-repair-feedback/startup-verification.json)仍为.env的grok-4.6＋Chat＋low，无临时覆盖。[真实完整交付](../../artifacts/bid-full-sample/real-run-v18-repair-feedback/feedback-delivery-observation.json)确认106条问题从0接收变为106条已提交回执，第13轮已有4条原候选变化；仅关闭本次完整反馈交付，不证明106项已修复。32项语义、字段忠实性、固定日期标签/附表交错顺序及同版完整DOCX/PDF/报告仍待验，速度后置。

2026-09-12 UTC 跨页附表问题归属冲突修复（06:19 UTC）：真实第87页的缺失关联 finding 只有前页原文、affected为空；嵌套映射校验接受后，外层仅按当前来源或affected ID筛选又将它拒绝，模型删掉它则再次触发缺失映射错误。现只在现有映射/关系证据校验成功后纳入其明确引用的问题ID，不扩大相邻页来源、候选或问题清单。无新增schema、提示词、检查点字段、表或migration。[原错误与诊断](../../artifacts/bid-full-sample/loop-repair/cross-source-finding-admission/diagnosis.json)、[263项库/20项合同及Clippy、fmt、编译验证](../../artifacts/bid-full-sample/loop-repair/cross-source-finding-admission/verification.json)通过；无关内外层问题、未读原文和过期候选继续拒绝。第433轮原模型参数在第480轮候选快照上的[内存投影](../../artifacts/bid-full-sample/loop-repair/cross-source-finding-admission/archived-replay.json)可提交并保留findings状态；只投影活动任务/依赖版本，不增加阅读或比较回执，不改原检查点，不是真实模型修订或全量通过。新归档程序同时包含此前反馈完整交付、字段忠实性、准确来源缺口与关系幂等修复，尚未替换冻结v17。旧程序继续可执行来源；若剩余任务仅有已保存阻塞，在核对进程身份后优雅停止并归档未完成状态，再以新合同导入经生产原文/候选回执校验的模型问题和候选，独立复核从空回执开始；旧失败/调用计数不清零。本条替代前文“必须等首轮完整报告后切换”的执行前提，不能用已证实会阻止首轮完成的旧校验无限等待。全106页、32项语义与同版DOCX/PDF/报告仍待验，速度后置。

2026-09-12 UTC 真实来源缺口与提取错误区分（05:53 UTC）：已定位边界校验把所有unresolved边界强制保留为finding，与编制允许准确来源缺口的既有设计冲突；即使主Agent已保存正确未决记录，局部也反复修订。现仅在当前Unresolved记录已独立取回并比较、边界原文对应且relationship_checks明确source_limited时允许checked；边界仍unresolved，来源报告仍保留缺口，缺证/过期候选/缺判断继续拒绝，可用续文仍须独立检查。无新增字段、表或migration。[红绿及262项投标库、20项合同、Clippy/fmt/编译验证](../../artifacts/bid-full-sample/loop-repair/source-boundary-open-item/verification.json)通过。同原始两来源与19条已修订模型记录/7条关系的新独立复核实际7次调用、1轮、零finding正常结束，候选不变，quality=needs_review且未决项保留；[实际结果](../../artifacts/bid-full-sample/loop-repair/source-boundary-open-item/real-result.json)与[生产编制准入审计](../../artifacts/bid-full-sample/loop-repair/source-boundary-open-item/structural-audit.json)通过。该结果只证明局部来源缺口可准确报告，不代表106页完整或已生成DOCX/PDF。三组对照合计73次，仍在原160次补充额度内。新工具/提示词合同尚未部署到v17；全量首轮报告完成后按新合同修复，旧检查点/调用计数保留。

2026-09-12 UTC 修复反馈完整交付检查（05:29 UTC）：真实v17主阶段反复读取第0/20项起的页，99条原始反馈仅40条出现在58个主请求中，后59条未见交付就进入独立复核；结构/来源阅读检查原先不检查修复反馈是否读全。现复用main_progress.seen保存精确问题内容的已交付摘要，仅在完整模型响应后提交；请求中提供未读数量和下一查询，request_review拒绝未读完整清单的交接。内容改变须重读，查询不代表修复、不清除发现、不授予独立阅读/通过、不重置进度额度。无新表、检查点字段或migration；主工具说明合同变化，新检查尚未部署到冻结v17。[诊断](../../artifacts/bid-full-sample/loop-repair/repair-feedback-coverage/diagnosis.json)、[真实归档离线投影](../../artifacts/bid-full-sample/loop-repair/repair-feedback-coverage/archived-replay.json)、[261项库/20项合同及Clippy、fmt、编译验证](../../artifacts/bid-full-sample/loop-repair/repair-feedback-coverage/verification.json)。离线补齐59条只证明分页和回执规则，不是模型已修复59项。当前继续全量独立复核，后续新修复合同同时接入已验证的字段忠实性规则与关系幂等；32项、完整DOCX/PDF/报告仍待验。

2026-09-12 UTC 正确性核查更新（05:13 UTC）：v17 全量独立复核仍在运行，尚未完成第一轮；主Agent只修订了部分候选，不能将99条初始发现视为已全部修复。已确认原文与错误候选同时交付后仍发生语义漏检：关系说明及要求条件把原文企业名称替换为另一企业，至少该关系被错误判为无问题。通用原文忠实性指令的局部对照已实际检出：同原文/原候选/预算，runtime仅两角色提示词摘要不同；旧指令完成第一轮仍漏掉两处，新指令第5轮自主保存两处字段错误及原文依据。主Agent已通过真实工具修正两处字段，修订后完整独立复核确认了相同候选版本且无这两处发现。对照归档时新组共3轮复核/40次调用，旧组1轮/26次；局部仍有截断来源的续页缺失发现，未将局部整体标为通过，也不能将目标字段闭环推广为全量正确性保证。人工预期留在source目录外，新指令未部署到全量冻结程序。[对照证据](../../artifacts/bid-full-sample/loop-repair/candidate-prose-fidelity/detection-comparison.json)、[额外160次对照预算](../../artifacts/bid-full-sample/loop-repair/candidate-prose-fidelity/budget-amendment.json)。完全相同关系重复新建的幂等返回也已实现，保留不同说明/作用域/证据的独立主张；未清理旧关系或把数量增长算作修复。新编译、严格Clippy、全仓fmt和diff检查通过；库测试259项在沙箱通过，另1项因本机bind权限失败后在允许本机端口的环境通过，30项忽略。此前并行knowledge接口不匹配编译失败保留为历史。[工程验证](../../artifacts/bid-full-sample/loop-repair/candidate-prose-fidelity/verification.json)。全106页、32项语义及新增字段忠实性检查、完整同版DOCX/PDF/报告仍待验；正确性与稳定性优先，速度后置。下方早期“当前”记录仅代表各自时间的历史状态。

2026-09-12 UTC 用户调整优先级：先完成全链路功能、正确性和稳定性验收，再处理速度优化。当前继续全106页独立复核与发现修复、32项语义复验、完整同版DOCX/PDF/报告及恢复一致性验证；保留耗时观测，但不以性能不达标中断正确性验收，不放宽来源覆盖、附表对应关系或质量门槛。

2026-09-12 UTC 新合同修复运行（04:23 UTC）：v16-resume1已优雅取消并保留终态：621轮、累计1622次调用、126项来源判断、99条模型发现、3处执行阻塞，0轮完整复核。旧检查点/预约/计数均未改写。v17-repair以新合同先让主Agent核查并修复既有候选；仅从原检查点导入通过原来源/候选阅读证据校验的模型发现，不导入独立阅读回执、来源通过判断或已完成复核。旧发现不是人工答案或已通过结论，主Agent不能删除它们，后续仍需全106页/148来源/42网格的独立复核。按用户允许提高预算，将连续全文验收调用上限提高到4000，旧1622次继续计入，新运行上限2378次；.env仍为grok-4.6＋Chat＋low。已实际启动修复并产生记录/关系修改，不能据此关闭32项或发布DOCX。[启动核验](../../artifacts/bid-full-sample/real-run-v17-repair/startup-verification.json)、[预算与来源审计](../../artifacts/bid-full-sample/real-run-v17-repair/preflight.json)、[动态进度](../../artifacts/bid-full-sample/real-run-v17-repair/progress-observation.json)、[工程验证](../../artifacts/bid-full-sample/loop-repair/repair-bootstrap/verification.json)。独立复核、32项及同版DOCX/PDF/报告仍待验，速度优化后置。

2026-09-12 UTC 正确性阻塞修复（04:06 UTC）：全文复核在物理第75页投标函映射判断上发生证据不匹配；恢复时已知问题只能分页查找，多次整页超出工具字节限制，且误把问题ID传给候选查询，最终记录执行阻塞。第84页文字/表格范围随后也阻塞，除重复问题清单失败外，还出现候选详情与当前原文的上下文共存拒绝，后者已补问题查询的上下文保留检查，真实恢复仍待验证。已实现 reviewer 按ID精确取回草稿问题、按字节预算分页返回完整问题、映射错误提供精确查询，并复用现有上下文检查缩小问题页，保留当前比较原文；不截断问题证据、不放宽映射校验、不清零阻塞或计数。[诊断](../../artifacts/bid-full-sample/loop-repair/full-review-blocker/diagnosis.json)、[验证](../../artifacts/bid-full-sample/loop-repair/full-review-blocker/verification.json)：258项库测试、20项合同、样稿编译及修改文件fmt通过；全局fmt末次遇到共享 worker/src/bidding.rs 编辑中的语法错误，严格Clippy被其他会话新增的 submission_export.rs 八参数函数阻止，本任务未改这两个文件。归档第515轮请求窗口在第539轮状态副本上的离线投影，两次查询均成功、所需证据未丢失；这不是第515轮原检查点恢复或真实模型验收。新工具/提示词合同尚未部署到真实运行，旧冻结程序继续其他独立来源；后续须用新运行身份验证，不能把本地回归当作阻塞已解除。32项语义、完整独立复核和同版DOCX/PDF/报告仍待验，速度优化继续后置。

2026-09-12 UTC 预算调整与续跑：用户明确允许提高预算，全文累计物理调用上限由1200提高到2400；此前998次及v16已用109次继续计费，调整时剩余1293次。v16安全停于第107轮（本段1226.85秒），续跑副本保留31项来源判断、11项草稿发现、原请求字节和全部预约/重试计数；仅修改物理预算、逻辑轮次及对应SDK轮次上限和配置摘要，未重启独立复核。v16-resume1已通过生产配置/检查点校验，并以相同正文续接第107轮，启动核验时累计1108/2400次；模型仍为deploy/.env的grok-4.6＋Chat＋low，上下文和输出预算不变。[预算变更审计](../../artifacts/bid-full-sample/real-run-v16-review-resume1/budget-amendment.json)、[启动核验](../../artifacts/bid-full-sample/real-run-v16-review-resume1/startup-verification.json)。尚无完整复核轮次，32项语义及完整同版DOCX/PDF/报告仍待验；提高预算不代表速度或质量问题已解决。以下旧预算和进度记录保留为历史。

2026-09-12 UTC 当前验证（03:35 UTC）：共享工作区补验253项投标库测试（30项忽略）、20项合同、严格Clippy与全仓fmt均通过；知识库测试目标编译通过。先前并行取消令牌改造造成的两次编译失败与Clippy失败均保留；本轮仅补工作区依赖、修正测试辅助函数构建范围及必要导入/注释/格式，[工程记录](../../artifacts/bid-full-sample/real-run-v16-review-resume1/current-workspace-verification.json)明确区分失败与最终通过。冻结程序的全106页/148来源/42网格复核继续，模型仍为deploy/.env的grok-4.6＋Chat＋low；第454轮、累计1455/2400次调用、103项已保存来源判断、45项待复核任务、72项草稿发现、0轮完整复核，[运行记录](../../artifacts/bid-full-sample/real-run-v16-review-resume1/progress-observation.json)持续更新。32项语义及完整同版DOCX/PDF/报告仍待验，先完成正确性与稳定性，速度优化后置。下方为预算调整与历史记录。

2026-09-12 UTC 前次实测记录：查询依赖与来源任务职责分离已实现，[工程验证](../../artifacts/bid-full-sample/loop-repair/source-task-ownership/verification.json)通过252项库测试（30项忽略）、20项合同、严格Clippy、全局fmt及编译。[局部复核终态](../../artifacts/bid-full-sample/loop-repair/source-task-ownership/fee-corrected-review-resume1/final-observation.json)为第68轮、累计69/80次调用、5轮复核、23记录/10关系，quality=needs_review；7项来源判断已完成，但末段组成要求缺少续页的1项发现仍未解决，生产编制校验正确拒绝。续跑619.19秒，加修复前243.58秒累计862.77秒（不含暂停），性能仍不合格。已观察到无业务写入的主Agent查询后再次复核往返，需验证完整分析摘要包含阅读回执是否干扰重复终止判断。当前无模型进程；旧费用80/80及全文第236轮998/1200保持终态。106页、32项语义与完整同版DOCX/PDF/报告仍待验。下方为历史记录。

2026-09-11 UTC 最新结论：全文性能仍不可用，当前没有真实模型进程，累计998次调用、原额度剩202次，第236轮三次供应商尝试已耗尽。离线定位并修复了局部导航误用全局阻塞的问题：第221轮当前20项候选比较全部完成、无局部缺口，却因另一范围的阻塞被引导继续找未完成候选；活动范围现复用执行准入的范围交集及依赖版本判断，全局验收阻塞保留。另一个范围在第179–185轮收到正确收尾指令后仍只执行13次查询，未尝试提交来源判断，这项修复不能解释或关闭该停滞。248项库测试、严格Clippy及全局fmt通过，模型提速及32项语义、完整DOCX/PDF/报告仍待验。后续优先验证有限轮次内提交有证据的来源判断或明确缺口的能力，并减少重复本地处理；不增加预算、不用完整重跑代替局部验证。详见[轨迹与分项计时](../../artifacts/bid-full-sample/loop-repair/scope-local-review-guidance/trace-analysis.json)。下方保留历史记录。

2026-09-11 UTC 性能修复真实对照通过：同一七页目录、原7项候选和同.env配置，baseline为33次/239.30秒/10次工具错误，完整原文出处规则的provenance为8次/63.55秒/0次工具错误，两组均verified且生产结构审计通过、候选值不变。该局部对照墙钟减少73.44%，不代表完整106页性能。已合并有界边界证据，并将冻结原文读取/引文与实际候选依赖分开；实际候选查询、记录/映射/关系ID及同页原图排版依赖保留。241项库、严格Clippy和编译通过；同次导航复用清单，离线输出摘要一致。旧全文v3在146轮无在途边界暂停，累计759次；v4已使用原1200额度剩余441次启动，首请求合同与成功对照一致，保留416条记录/8条关系，未导入旧独立回执。32项和同版DOCX/PDF仍未完成。证据：artifacts/bid-full-sample/loop-repair/review-boundary-evidence/provenance-final.json、real-run-v15-review-v4/startup-verification.json。

2026-09-11 UTC 全文性能修复接入：已确认search_sources仅搜索冻结原文，却误记全局分析/发现依赖，导致同一第5页目录在第13–21、46–52、87–93轮重复派发。已删除该错误依赖；真正的候选查询、相关来源/关系/规则及输入摘要校验保持，239项库、严格Clippy和编译通过。旧real-run-v15-review-v2在第140轮无在途请求边界主动暂停，累计613次调用及416条记录/8条关系保留；real-run-v15-review-v3已启动，原1200次上限余额587次，沿用.env的grok-4.6＋Chat＋low及原上下文/输出预算，不导入旧独立回执。新首请求已核对相邻导航及同批交接合同。32项语义和同版DOCX/PDF/报告仍未完成，全文提速尚待验。证据：artifacts/bid-full-sample/loop-repair/source-query-dependencies/verification.json、artifacts/bid-full-sample/real-run-v15-review-v3/startup-verification.json。

2026-09-11 UTC 当前进展：read_review_task已合并当前原文和完整候选的读取，保留原预算及交付边界。同一页/同一11条初始候选对照中，原工具71次/652.93秒/5轮复核，合并读取14次/139.90秒/1轮复核；两组均needs_review且关系判断不同，不能宣称等质量提速。现已补同页多个模板的文字/网格原图检查（真实第84、95、96页暴露旧漏检），并从主写入schema派生复核可见的关系类型及说明，明确同页条件依赖仍须判断。236项库、20项合同及严格Clippy通过，关系合同同输入复验已完成34次/399.59秒/2轮复核：原11条记录不变，正确新增references关系并再次独立确认；仍有compliance=unknown待审项，不算完整验收。旧全文主提取完成416条记录/8条关系/148项来源处置，已在第473轮无在途请求边界暂停旧合同复核；新合同全文复核已启动，保留原成果并从原1200次上限扣除已用473次，余额727次，不复用旧独立回执。32项语义及同版DOCX/PDF/报告仍未完成。详见 artifacts/bid-full-sample/loop-repair/relationship-vocabulary/verification.json 和 artifacts/bid-full-sample/real-run-v15-review-v2/startup-verification.json。

2026-09-11 UTC 性能对照终态补充：同一第11页原格式27次调用/503.66秒，简短引用21次/410.91秒，墙钟缩短18.42%；新版主提取6次/模型162.54秒，独立复核15次/模型242.04秒。两组均完成一轮复核且生产结构审计通过，但新版quality=needs_review，原格式为verified，候选分类不同，不能宣称等质量提速或性能可用。新版仍有8次工具错误及重复读取/复核提交往返；下一步应缩减有界提取与复核的串行调用，保留独立证据和语义门槛。证据：artifacts/bid-full-sample/loop-repair/compact-evidence-references/final-comparison.json。全文旧合同运行继续，完整32项及DOCX/PDF验收仍未完成；本条取代下文“对照进行中/待终态”的状态。

2026-09-11 UTC 历史进展（以上方状态为准）：简短证据引用已实现并共用于提取/独立复核和编制/稿件复核，领域Span、持久化成果和阅读校验保持原合同语义；232项库、20项合同和严格Clippy通过，离线5轮参数缩减约25%–55%且展开结果逐值等于原参数。相同第11页来源、同.env、同预算的原格式/简短格式顺序对照已启动，真实提速及语义质量尚未证明；当前workspace格式检查仍有其他并行修改的排版差异。全文real-run-v15-resume2仍使用其归档程序及旧合同继续，第196轮接入正文/表格导航修复时保留241条记录、2条关系和196次累计调用；新工具/提示词合同不套回旧检查点。全文独立复核、32项语义、完整同版DOCX/PDF/报告尚未完成，部分模板整段待填可能删除固定文字仍须复核修正。沿用deploy/.env的grok-4.6＋Chat＋low，无模型配置调整或新migration，人工答案不发送。 详见[实施门槛](#55-本次修复的实施门槛)。

本次恢复验证及失败边界见[验证记录](../../artifacts/bid-full-sample/loop-repair/replan-context/verification.json)。旧17种查询回归继续作为前一缓存修复的证据；本次针对真实恢复状态回放，不将离线成功等同于模型语义正确。

前序原图验证：195项库测试（11项忽略）、20项合同、7项隔离 PostgreSQL恢复、bidding 全目标严格 Clippy、workspace fmt与样稿编译通过。真实请求离线投影保留当前10份焦点候选、全部解析来源范围及1张当前原图，请求365383字节；12份非焦点候选按预算淘汰，因此该投影的“全部候选均保留”断言失败记录仍保留，不以较弱结论替换它。原混合批次回归的全部解析及候选证据保留断言继续通过。证据：[当前原图与导航验证](../../artifacts/bid-full-sample/loop-repair/active-review-image/verification.json)、[续接/类别验证](../../artifacts/bid-full-sample/loop-repair/boundary-target/verification.json)。保留原预约字节、检查点和计数，模型始终重读 `deploy/.env`，显式预算保持原值。未新增表、migration、配置项或操作业务数据库。

补充跨来源导航回归：原文任务要求复核关系另一端，但工作导航只数当前来源候选，曾错误显示剩余0项。现让待交付候选、剩余比较和宿主指派引用同时纳入源任务依赖；明确扩展原文访问范围的约束保留。回归同时验证端点内容改变后，原来源判断及本地候选比较均失效。188项库测试、严格 Clippy、fmt、编译及7项隔离 PostgreSQL通过；未改工具/提示词、共享合同或SQL。证据：[跨来源修复](../../artifacts/bid-full-sample/loop-repair/cross-source-navigation/verification.json)、[兼容续跑核验](../../artifacts/bid-full-sample/loop-repair/source-review-resume1/startup-verification.json)。

混合图片批次裁剪补充：第60→61轮原请求显示14个焦点候选随旧大图批次被一起淘汰，随后重复取回。现对已交付、图片载荷自身超出历史预算的批次，仅将独立图片消息换为明确的历史省略标记，保留同批工具调用/结果和候选；最新待交付批次、原图缓存、阅读回执和预算不变。188项库测试、严格 Clippy、fmt和编译通过；真实请求离线回放保留15个当前候选版本及全部当前解析证据，请求92956字节，最新工具批次原样保留。证据：[裁剪修复及回放](../../artifacts/bid-full-sample/loop-repair/mixed-image-retention/verification.json)。两组恢复均核对了原预约正文与累计计数；这只证明证据保留，不能替代模型的比较和漏项判断。

下列早期运行记录保留用于追溯，不覆盖上面的当前状态。

**此前修复记录：本地功能验证通过，独立复核已完成局部比较，未完成最终提交，运行已停止；完整验收尚未通过。** 已分离来源权限与局部焦点，自动维护成果/未解决引用，主提取、独立复核、编制和稿件复核共用进展与有界恢复策略。默认连续无进展6轮、焦点24轮、重规划2次；记录局部执行阻塞后允许转向独立范围，连续6轮仍未交接则在工具提交边界停止。重启、重复读取、笔记改写和任务改名不能刷新额度；有效局部写入可以完成当前动作。执行失败单独保存并阻止最终发布，不冒充来源缺项。候选当前版本参与窗口保留；各 reviewer 保留自己的冻结原文回执，修改后的候选/稿件仍按摘要核查。旧手抄引用输入及编制 `remember` 路径已删除，未新增 migration，既有 baseline 同步检查点与预算 JSON。

验证：最新176项库测试（11项忽略）、20项合同、1项真实检查点离线提交诊断、严格 Clippy、workspace fmt及样稿编译通过；既有6项隔离 PostgreSQL结果保留，本次未改SQL。首次3来源短测在21轮停止，11条记录、0关系，发生一次超时后重试成功；第二次 v2 在32轮停止，29条记录、12条关系、2个来源处置，四组重点引用由真实Agent产生，但独立复核未开始。v2请求42214–353819字节，含必要原页图片，无503或超时。轨迹还暴露无响应要求被迫指定渠道、同类型合规属性的不同条件被拒绝、工作引用格式说明不足；均已修复并验证。未新增 migration，未修改实际 `.env` 或旧检查点，未执行暂存操作。包含全部修复的 `response-contract-trial` 已以新身份、空候选重测第11、16、17页的3处来源，模型与预算仍来自 `deploy/.env`；启动合同已核对，结果待验。该试验在34轮保留32条记录、21条关系后，进一步定位到重复读取会触发无实际缺口的 pending_delivery 阻塞。已删除这条冗余判断，尚未交付的原文/候选仍按真实缺口阻止交接；172项库测试、20项合同及Clippy/格式/编译通过，SQL未因本项调整。`response-contract-resume1` 已在第101轮由无进展/交接保护停止：主提取完成，41条记录、21条关系、3项来源处置；独立复核收到65个候选当前版本后仍重复读取，两次重规划无效，0轮复核、1项执行阻塞。已保留终态和计数，未将空缺口等同于语义通过。现补充复核专用完成指引与按实际缺口生成的下一动作：有问题逐项保存、无问题完成原范围后提交空草稿；执行阻塞仍禁止提交。173项库测试、20项合同及Clippy/格式通过，无新工具/配置/migration；提示词已改变，`reviewer-completion-trial` 已以新身份、空候选复测；启动核验确认实际配置、工具和预算未变，旧终态未改。该试验在第85轮到达诊断时限并取消：主提取41条记录、28条关系、3项来源处置；reviewer完成一个局部范围、收到72个候选当前版本，仍未提交最终结论。除精确读取位置反馈、局部复核排除不相关待办外，现新增 complete_review_check，在既有进展账本中记录当前候选的无问题比较；重复版本/改写结论不能续额度，来源、当前版本、执行阻塞及最终全局门槛保持不变。176项库测试、20项合同、3项.env启动测试及Clippy/格式/编译通过，无新表、migration、检查点字段或环境变量。新工具/提示词采用新身份：clean-review-trial 在补充明确授权后实跑656.16秒，停于第25轮：独立收到72个候选当前版本，完成41个记录版本的局部比较，46次重复比较被去重；28条关系和3项来源处置仍未形成比较结论，0轮最终复核。定位到当前焦点已完成但执行反馈仍要求比较该焦点，已增加焦点剩余数和转向未完成引用的派生反馈；176项库测试、20项合同、Clippy/格式/编译通过。clean-review-resume1 保留原检查点、预约正文及计数兼容续跑，在第40轮完成全部72个局部比较，但此后反复读取，0次完成范围、0次提交复核，第58轮进入执行阻塞，第64轮耗尽交接额度后停止。只读第54轮检查点副本的正式工具调用均通过，证明当时接口可用；离线副本不作为真实复核结果。最终结束行为仍未解决，本轮后续复杂附表及完整样稿重测未启动。此前自动审批拒绝已由用户补充明确授权解除。旧运行、原始来源和计数未改。复杂附表、完整106页独立复核、32项语义发现及完整 DOCX/PDF仍未通过。证据：[验证记录](../../artifacts/bid-full-sample/loop-repair/verification.json)、[v2真实轨迹](../../artifacts/bid-full-sample/loop-repair/source-scope-trial-v2/result.json)、[上一实跑启动核验](../../artifacts/bid-full-sample/loop-repair/reviewer-completion-trial/startup-verification.json)。

更新日期：2026-09-10。用户已批准本方案并授权实施；方案与文档已统一，当前推进实施与分项验收；本次按用户复核收敛为提取/复核修复先行、Journal 独立升级、Rig 单协议接入。明确仅使用 Chat Completions，删除 Responses 实施范围。P0/P1 已部分实现，P3 当前驱动恢复验收通过，P4 接缝已验证、共享宿主 I/O 驱动及 Rig Chat 请求序列化/流解析已接提取/编制，AgentRun 多轮步进和有界会话已接入并通过隔离验收；完整真实验收未通过。阶段编号仅属于本方案，不替代平台任务或 ONLYOFFICE 的 O0–O4。

v9 已在第45轮检查点停止：运行1043.91秒，34条记录、10个来源处置、0关系，独立复核未开始；第25–44轮仅查询已有候选，连续20轮没有新增成果或来源覆盖。后半段未观察到503，重复候选全文查询多次因无法与当前原文共存而失败。已保留全部请求及检查点，目录/详情取回修复已通过离线及隔离验收；不能视为真实语义或整稿验收通过。用户对 `.env` 目的地的具体外发授权持续有效。证据：`artifacts/bid-full-sample/real-run-v9/terminal.json`、`stagnation-report.json`。

v10 已停止：运行387.94秒，第24轮检查点保存7条记录、8个来源处置、7张已交付原页，0关系；第5轮后没有新增成果，反复读取12个活动来源。目录查询未再复现 v9 的候选共存错误，但完整持续产出仍失败。原文工具返回完整单元格；活动范围只能扩大不能拆小，未充分实现方案的分次处理要求，显式待处理来源与安全拆分已通过离线和隔离验收，真实持续产出尚待复验。证据：`artifacts/bid-full-sample/real-run-v10/terminal.json`、`diagnostic-checkpoint.json`。本轮未生成可验收的提取结果或整稿。

v11 已保存第48轮检查点后停止，耗时1421.51秒，35条记录、12个来源处置、0关系、0轮独立复核。期间仍有写入，不能归类为连续空转；停止原因是已复现旧索引挤掉原文网格，需以新合同验证修复。第33→34轮离线重放由仅保留2张网格改为保留全部4张，请求107508→95213字节，最新工具结果保持原样。库149项、合同20项、隔离 PostgreSQL 5项及 Clippy/格式通过，未调整预算或新增 migration。证据：`artifacts/bid-full-sample/real-run-v11/terminal.json`、`artifacts/bid-full-sample/navigation-history/verification.json`。

v12 已人工停止并保留第152轮检查点：耗时2975.47秒，54条记录、6条关系、20个来源处置，独立复核未开始；最后47个完成轮次没有新增记录，期间仍有读取和导航。导航裁剪改善了原文保留，但未解决完整持续产出。终态见 `artifacts/bid-full-sample/real-run-v12/terminal.json` 和 `diagnostic-checkpoint.json`。

v13 已获明确外发授权并启动，向 `https://ai.zleiwork.cn` 发送同一招标文件的解析文本、网格及必要原页图片，模型和预算只读 `deploy/.env`。归档程序包含候选详情回执及字段校验反馈；启动核对确认供应商、预算、二进制、冻结来源和新提示词/工具合同一致。真实持续提取、独立复核及完整 DOCX/PDF 尚未通过，32项发现保持开放。证据：`artifacts/bid-full-sample/real-run-v13/startup-verification.json`。第104–129轮状态保留在 `artifacts/bid-full-sample/real-run-v13-resume1/progress-snapshot.json`；第129轮后由兼容恢复目录 v13-resume2 续跑，新增15条记录后再次出现重复核查，现已停稳于第164轮，详见下方字节定位修复记录。

2026-09-10 补充：主提取及 reviewer 各自保存完整候选详情的已交付摘要，目录 `detail_received` 显示本角色是否收到当前版本；它不是语义通过标志，修改后失效，同批尚待交付及另一角色的回执不能冒充已收到。交接反馈明确保留已有引用不需重新取回全文。151项库测试、20项合同、5项隔离 PostgreSQL 测试及 Clippy/格式通过，无新增 migration；v12 已停止，新提示词/工具合同由 v13 真实复验。证据：`artifacts/bid-full-sample/candidate-receipts/verification.json`。

2026-09-10 字段校验反馈已补齐：复用原 `ok/error` 封装及语义校验器，以 `INVALID_FIELD <JSON Pointer>: <constraint>` 指明记录、引文、模板单元格及关系的失败位置和约束，失败写入保持原子性。反序列化错误定位到所属容器，不声称每个嵌套 Serde 错误都能定位叶子字段；原文未给出的单位等仍允许为空。153项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、严格 Clippy、workspace fmt 及样稿编译通过，无新增 migration。证据：`artifacts/bid-full-sample/validation-fields/verification.json`。

2026-09-10 原页历史淘汰修复：v13 第68→69轮确认超大旧图片组会先挤掉较早的完整网格，随后自身也被淘汰。现按冻结历史预算优先淘汰必然放不下的已交付图片组，保留较小原文组；离线回放从0张完整网格改为保留2张及相关正文，154项库测试、20项合同、5项隔离 PostgreSQL、Clippy/格式及样稿编译通过。最终修复未改变提示词、工具、来源或预算合同。v13 原实例停稳于第104轮后，以归档修复程序从同一检查点恢复，保留52条记录和105次累计调用；恢复记录见 `artifacts/bid-full-sample/real-run-v13-resume1/startup-verification.json`。证据：`artifacts/bid-full-sample/image-history/verification.json`。

2026-09-10 正文字节定位修复：`read_source` 在保留原文及起止范围的同时返回逐行 `line_spans`，中文与原换行均按真实 UTF-8 字节定位，完整结果按原工具预算分页；兼容恢复仅给已交付历史补充确定性位置，不改已预约请求和阅读回执。真实第120→121轮离线回放保留两页58行完整正文及附表，请求116767字节；157项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、Clippy/格式及样稿编译通过，无新增 migration。v13-resume1 停稳于第129轮，52条记录、0关系、20个来源处置、0轮独立复核，累计131次调用。原状态已逐字节复制到 v13-resume2，生产合同校验通过；自动审批首次拒绝后，用户针对具体目的地和载荷再次明确回复“继续,允许”，现已从第129轮恢复，原预约正文保持不变并累计第二次调用；启动核验见 `artifacts/bid-full-sample/real-run-v13-resume2/startup-verification.json`。第130轮实际请求已验证包含两页58个逐行位置；随后新增15条记录，达到67条。第136轮后连续28个完成轮次无新增记录、关系或来源处置，期间73次候选详情/目录查询、29次搜索，未尝试写入关系。原文持续保留，但候选详情在有界窗口内反复取回；该相关性尚不能单独证明模型循环的根因。恢复实例运行777.78秒后安全停于第164轮，保留67条记录、0关系、20个来源处置及167次累计调用；未进入独立复核。逐行位置已验证可用，整体持续提取仍未通过，不能继续以相同正文重试或放宽预算冒充修复。证据见 `artifacts/bid-full-sample/real-run-v13-resume2/repeated-inspection-observation.json`、`terminal.json` 和 `line-spans-request-verification.json`。证据：`artifacts/bid-full-sample/source-line-spans/verification.json`、`artifacts/bid-full-sample/real-run-v13-resume2/preflight-verification.json`。32项语义发现及完整 DOCX/PDF 验收仍开放。

2026-09-10 停滞进一步定位：四组简单引用的双方完整详情在7个真实请求中同时可见，生产关系工具的离线内存副本验证全部通过，单组详情仅1116–1288字节；不能再将零关系简单归因于窗口装不下。28轮内257份详情只有25个版本，工作笔记与17项缺口不变。当前缺口是局部任务推进和停滞恢复，候选历史保护不足会加重重复，但不是充分解释；模型内部选择原因仍不可由轨迹证明。详见[定位报告](../../docs/bidding/agent-loop-diagnosis.md)。本次未修改生产逻辑或新增外发，修复方向尚待短范围真实验证。

产品目标以 [PRD](../../docs/bidding/prd.md) 为准；领域身份以[领域契约](../../docs/bidding/authoring.md)为准；编辑与出件以 [ONLYOFFICE 契约](../../docs/bidding/onlyoffice.md)及其[接入顺序](onlyoffice-integration.md)为准。本文件是本次投标 Agent 改造的统一实施入口，进展记录在[执行台账](../implementation-tasks.md)。历史实现、失败和局部验证不能当作本方案已通过。

## 1. 目标、选型和本期范围

### 2026-09-10 已批准的停滞修复实施补充

用户已批准完整修复方案并要求实施。此次先以已归档的28轮重复查询及四组可写关系作为回归，再完成下列工作；工程保护通过不等于真实提取或整稿验收通过。

1. 扩展现有工作状态，分别表达允许读取的来源范围和当前定位/提取/关联/复核/交接焦点，使用冻结来源范围及真实成果引用；由 Agent 选择内容，不按样稿或行业关键词分配答案。 `locate` 可先声明空焦点或有效的待读文本位置，不产生阅读回执；网格/图片引文及正式记录仍须本角色收到证据。`complete` 以来源覆盖、成果和独立检查为准，不额外要求一个新的活动焦点。
2. 运行层自动维护已有成果与未解决成果引用，删除要求模型反复抄写全部 `output_refs/pending_refs` 的工具输入。完成仍要求真实成果、来源处置及独立复核，不以自动记账代替接受。
3. 按新增已交付覆盖、成果/关系版本、来源处置、复核发现、已独立核查的无问题候选版本和有效交接判断进展。重复读字节、调用次数、笔记改写及任务改名不算进展。统一配置新增默认值：`max_no_progress_turns=6`、`max_focus_turns=24`、`max_focus_replans=2`；冻结展开后的有效配置，不修改实际模型或提高原有预算。
4. 停滞时在已提交边界保存状态并生成精简恢复上下文，有限次重新规划；额度耗尽保存局部执行阻塞，继续其他独立范围。用户明确选择此策略。来源真实缺项与模型执行失败分别保存；后者不能冒充来源问题或通过结果。重启、换焦点及重复查询不能清零消耗。
5. 上下文清点完整候选版本并保护当前焦点所需原文与候选；按完整协议组删除旧导航、重复版本和已完成内容。待交付结果及阅读回执不变，必要内容超预算时拆分或分页。
6. 主提取、独立复核、编制及稿件复核应用同一进展判断机制，各角色保持自己的工作状态与覆盖。保留复合条款、完整附表、混合单元格、跨页连续性和要求到模板位置的验收要求。
7. 保持 Rig 0.42.0、Chat 和三阶段 Journal；工作状态只在工具提交边界更新。同步现有 baseline 的检查点字段/初值校验及隔离测试，不新增表或独立 migration，不操作现有数据库。新工具/提示词/有效配置按新摘要、新运行验收，不修改旧检查点冒充兼容恢复。
8. 顺序：离线故障回放 → 局部任务/恢复 → 当前证据保留/独立复核 → 四组引用短范围真跑 → 复杂附表真跑 → 完整106页提取/32项复验 → 全部章节模板和同版 DOCX/PDF/报告。投标方事实继续待填，来源缺口按原质量规则明确报告，执行阻塞禁止通过发布。

借鉴来源：Pi `prepareNextTurn`/`transformContext` 与完整工具组裁剪；OpenCode 重复工具检测（本项目扩展为业务状态检测，不增加人工权限询问）；LangGraph 按状态条件路由；Rig 发布版 `AgentRun` 手动步进。保留原运行层，不增加另一套 SDK、进程或任务队列。定位证据及边界见[停滞报告](../../docs/bidding/agent-loop-diagnosis.md)。

当前实施状态：功能修复通过本地及隔离数据库验证；`reviewer-completion-trial` 在第85轮到诊断时限，独立复核未完成。新增局部无问题比较后，`clean-review-trial` 已完成41个记录的局部比较，但第25轮仍未完成关系复核；完成焦点反馈修复后，`clean-review-resume1` 已完成72个局部结果，但第58轮进入执行阻塞，最终提交仍未完成。短范围关系产出已有证据，完整106页提取、独立复核、32项语义发现及完整 DOCX/PDF 验收仍开放。

项目最终目标：**依据真实招标文件规定的投标要求，生成包含目录、全部应有章节、指定格式、附表及固定内容的完整可编辑投标模板，并输出同一版本的 DOCX、PDF和检查报告。**

本期填入招标侧已经明确的项目已知信息、固定声明、填写说明、表格结构、原条款和签署提示。投标方公司资料、人员、产品选型、实际响应、价格和证明材料保留待填位置，后续完成。不得恢复通用“商务、技术、报价”结构，不得把招标文件自身目录直接当作投标目录。

本文件本次交付是可独立验收的招标提取/复核修复及运行时替换。完整模板由现有 `docx_composition` 消费修正分析，编辑与出件继续归 O1-S/O2；不能把上面的项目目标扩成运行时全链重写。

### 1.1 技术选择

- 锁定 `rig = "=0.42.0"`，关联 Rig 包在锁文件中固定到已验证发布版本。
- 发布包职责已核对：`rig-core` 提供模型/协议合同，`rig-agent` 提供 `AgentRun`；正式共享驱动应使用同版本官方 `rig` facade 的 `agent` feature。生产已使用 `rig = "=0.42.0"` facade 的 `agent/reqwest/rustls` features，Chat 请求序列化、流解析和 AgentRun 多轮步进均已接入提取与编制；本地恢复验收通过，实际供应商下的完整语义及整稿验收仍开放。
- 投标提取、独立复核、模板编制和稿件复核最终复用同一运行层；协议只使用 Chat Completions，不引入双协议配置、Responses 客户端或加密推理状态适配。先在现有驱动修复提取和复核，再验证 Rig 替换，不把 SDK 作为语义修复前置。
- Rust 业务模块继续负责来源、要求、附表关系、业务校验和发布；文件解析继续使用统一 Python `docreader` service。
- 复用现有 Worker、执行权、数据库检查点、对象注册和 ONLYOFFICE，不引入 Node runner、第二套队列、Agent 会话数据库或 Pi/Rig 双运行模式。
- Embedding 配置与索引一致性归知识库独立任务，本次仅说明模型职责，不改索引或向量化。
- 已完成的官方 S3 SDK 替换保留，补齐证据归档；不重复实施。

选择 Rig 是因为它提供 Rust 多轮状态机及模型协议封装，有望减少自维护代码；接入既有预算和恢复边界仍须通过小范围验证，不能把 SDK 能力等同于集成已成立。Pi 保留为研究备选，本次不维护双实现。SDK 选型不证明模型供应商可用，也不保证语义正确或速度倍数。

### 1.2 官方案例采用方式

案例及源码按 0.42.0 发布对应提交 `d5a34986a1ad57f1e9c5984b82f8d7438ffc717e` 核对，不直接采用 `main` 中发布版尚未包含的接口。

| 官方案例或实现 | 本项目采用方式 |
| --- | --- |
| [agent_run_stepping](https://github.com/0xPlaygrounds/rig/tree/d5a34986a1ad57f1e9c5984b82f8d7438ffc717e/examples/agent_run_stepping) | 使用 `AgentRun` 状态机，在模型和工具 I/O 边界接入持久化。 |
| [request_hook](https://github.com/0xPlaygrounds/rig/tree/d5a34986a1ad57f1e9c5984b82f8d7438ffc717e/examples/request_hook) | 借鉴逐轮准备请求机制；手动步进时由驱动层明确执行，不假定高层 hooks 自动生效。 |
| [agent_with_memory](https://github.com/0xPlaygrounds/rig/tree/d5a34986a1ad57f1e9c5984b82f8d7438ffc717e/examples/agent_with_memory) | 复用窗口策略，业务恢复不依赖进程内会话记忆。 |
| [agent_evaluator_optimizer](https://github.com/0xPlaygrounds/rig/tree/d5a34986a1ad57f1e9c5984b82f8d7438ffc717e/examples/agent_evaluator_optimizer) | 独立复核、反馈修订、统一预算上限；复核者自己读取原文。 |
| [agent_orchestrator](https://github.com/0xPlaygrounds/rig/tree/d5a34986a1ad57f1e9c5984b82f8d7438ffc717e/examples/agent_orchestrator) | 按真实来源组织工作单元，不照搬固定任务数量和任务类型。 |
| [openai_streaming_per_call_usage](https://github.com/0xPlaygrounds/rig/tree/d5a34986a1ad57f1e9c5984b82f8d7438ffc717e/examples/openai_streaming_per_call_usage) | 区分单次请求用量和整个运行的累计用量。 |
| [tool_result_outcomes](https://github.com/0xPlaygrounds/rig/tree/d5a34986a1ad57f1e9c5984b82f8d7438ffc717e/examples/tool_result_outcomes) | 工具返回错误事实，运行层决定停止或反馈，不能让模型绕过持久化失败。 |

`AgentRun` 本身仍会累积历史，序列化格式没有跨版本稳定保证。`CompactingMemory` 的水位和摘要是进程内状态，摘要处于保留窗口预算之外；默认 `TemplateCompactor` 不是语义总结模型。因此本方案单独控制发送窗口和活动 SDK 状态，默认不增加 LLM 总结调用。

OpenAI 官方文档页面在此次核查中返回 HTTP 403；上述能力依据 Rig 发布版源码，Rig 与实际供应商的接缝兼容性须在 P4 验证，P0 复用当前 Chat Completions 路径。

### 1.3 实施风险复核与范围收敛

2026-09-09 再次核对工作区及已读取的 Rig 发布版源码后，确认原顺序把提取修复与 SDK/双协议改造绑得过紧。以下约束替代初稿中的双协议前置顺序；用户已明确只使用 Chat Completions。不因保存了方案便声称风险已解决。

| 风险 | 已核对事实 | 修订决定与验证边界 |
| --- | --- | --- |
| 供应商与协议 | `authoring_runtime.rs` 只接受 `openai_chat_completions_sse`；提取及编制 SQL 预约都校验 `messages`、首条 system、`max_tokens` 与 required tools。现有代理已实际走过 Chat Completions，但最新完整运行因503终止。 | 只接入原 Chat Completions，不新增协议选项；不能为适配 SDK 放宽冻结校验，也不由503推断具体供应商故障原因。 |
| HTTP 预约接缝 | 当前已 JCS 规范化、预约并原样发送，传输层不另重试。Rig 0.42.0 的 `HttpClientExt::send_streaming(Request<T>)` 接受 `T: Into<Bytes>`，`CompletionsClient<H>` 支持注入该客户端。 | SDK 构造完整请求后只做通用 JSON 的 JCS 规范化，不另写 provider 字段序列化器。取得执行权后预约同一字节再委托 HTTP；必须用捕获服务验证字节一致、预约失败零发送、失败/取消与重复恢复。接口存在不等于该接缝已通过。 |
| 双协议矩阵 | 四种业务角色共同依赖模型/工具传输；原计划把两协议、图片、推理状态全部放在首批。 | 只验 Chat Completions。协议契约测共享适配一次，各角色测各自工具权限和业务效果，不机械相乘；删除双协议和加密推理适配任务。 |
| Journal 与 SDK 绑定 | 旧合同的 `checkpoint_put` 将 `state.turn` 写为 `batch_ordinal`，要求每次前进一轮；提取、编制和数据库回归依赖此约定。 | P3 单独升级既有 Journal，同轮多边界恢复必须有独立红/绿证据；不以引入 Rig 为改 baseline 的充分理由，不新增 Agent 表或 migration 文件。 |
| Tokenizer 与本次瓶颈 | 仓库尚未接 Rig `TokenCounter`；发布版虽提供接口与启发式计数，也不能提供经证实的 grok 精确 tokenizer。v5 最大约784 KB，没有触及2 MB上限。 | P0 先改上下文选取和主动交接，不能只降低上限或加入 token 计数。首期按显式配置做保守估算、余量及字节保护，标明估算，不引入 tokenizer 选型工程。 |
| 工具合同 | 旧版 `inspect_analysis` 的 schema/实现只接受 `kind/offset/limit`，不能按记录 ID 精确取回；这会增加扫描和重复读取。 | P0 明确修改现有工具的查询能力、schema、提示词与测试，更新 `tools_sha256` 和冻结合同，使用新运行身份。复用工具框架不等于冻结工具功能不准改。 |

Rig 的价值还包括共享多轮驱动与减少自维护协议代码，但不自动解决上下文、附表关系、独立复核或供应商503。若 P4 接缝需要 fork SDK、复制 provider 序列化或绕过预约，则停止该接入切片并报告，不拖住已验证的提取修复与真实样稿工作；现有运行层在切换前继续承担执行，切换后删除旧循环，不长期保留两套模式。

## 2. 模型、配置、通信和目录

### 2.1 模型职责

以下是保存方案时读取 `deploy/.env` 的配置快照，不是代码常量，也不证明运行进程已重新加载。

| 能力 | 当前配置 | 本期用途 |
| --- | --- | --- |
| 生成及工具调用 | `grok-4.6` | 提取、关系核对、独立复核、模板编制和稿件复核。 |
| Embedding | `text-embedding-3-small` | 知识库向量索引与语义检索，独立 `/embeddings` 接口。 |
| 知识库图片理解 | `grok-4.6` | 现有知识库图片处理。 |
| 招标原页核查 | 投标 Agent 当前配置的模型 | 原页工具提供图片，Agent 结合正文和网格核查。 |

不同 Agent 角色共用生成模型，独立性来自上下文、阅读覆盖和工具权限。不新增隐藏复核、摘要、重排或备用模型。招标全文提取不以向量 Top-K 召回代替完整核查。

### 2.2 配置及冻结

- 所有新运行的模型名、地址、凭据和可调参数来自 `deploy/.env` 对应配置；禁止命令行临时换模型。
- 保留 `KNOWLEDGEBRAIN_*` 和现有 `LLM_*`、`EMBEDDING_*` 别名；同一配置的别名同时存在但值冲突时明确报错。
- 协议沿用并校验 `openai_chat_completions_sse`，这是明确的接口合同，不是按模型名称猜测；只使用这一种协议，不新增 `KB_AUTHORING_PROTOCOL` 或 `KNOWLEDGEBRAIN_CHAT_PROTOCOL` 配置开关。
- 输出上限与超时已改为 `KB_AUTHORING_MAX_OUTPUT_TOKENS`、`KB_AUTHORING_TIMEOUT_MS`，按用户要求在环境模板与 Compose 提供8192 tokens、180000 ms默认值，本地 `.env` 仅补缺失项。Rust 读取并冻结正整数，不保留另一套常量。提取预算新增总 token 上限、每图预留和余量，默认分别为131072、16384、4096；估算按完整请求 UTF-8 文本字节与图片预留计算，同时保留字节上限。这是应用预算，不声称已验证模型的实际窗口。
- 已读取并冻结 `KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT`，提取/复核和编制/稿件复核请求实际发送配置值；未显式配置时省略。SQL 预约校验该参数与冻结配置一致。这里只扩展现有 baseline 的请求字段校验，无新增 migration 或表；未操作现有数据库。
- 记录脱敏有效配置与合同摘要，检查最终序列化请求是否实际采用。配置文件变化不等于进程加载，新运行须验证有效摘要。
- 旧运行按冻结合同恢复；模型、协议、提示词或工具合同变化时不得修改旧检查点冒充同一次运行。

### 2.3 Chat Completions 接入

显式使用 Rig `CompletionsClient`；不可误用 SDK 默认的另一协议客户端。继续 HTTP SSE、自定义工具、`tool_choice=required` 及现有图片输入。冻结模型名、输出预算、消息、工具与参数；必要协议字段由明确合同规定，可调参数从环境读取。

仅启用现有业务工具，不开放任意搜索、文件解析或代码执行。完整流、结束原因及工具参数校验通过后才能提交该轮工具；异常 EOF、长度截断、不完整参数或失败响应不能触发业务写入。共享传输测试覆盖工具往返、图片、usage 缺失、错误、取消及预约/发送字节一致；各角色只重复其业务权限与成果测试。

### 2.4 Embedding 职责边界

Embedding 继续使用知识库的既有独立 `/embeddings` 封装，当前1024维约束不变；招标全文提取不消费向量 Top-K。配置快照仅说明系统有哪些模型，不把索引模型一致性、重新索引或检索改造加入本次 P0–P4。该独立事项见[知识库计划](../knowledge-base/README.md#模型与索引一致性独立事项)。

### 2.5 通信与目录

```mermaid
flowchart TD
    W[现有 Worker：任务与执行权] --> R[共享 Rig 运行层]
    R --> A[提取与独立复核业务工具]
    R --> C[编制与稿件复核业务工具]
    R --> H[Rig 模型适配与受控 HTTP]
    H --> M[环境配置的模型服务]
    A --> P[已有解析结果与 Python docreader]
    R --> J[现有调用预约与检查点]
    C --> D[现有 DOCX 编译器]
    D --> O[对象存储与 ONLYOFFICE]
```

Rust 模块进程内异步调用；Python 使用现有 service 接口，不新增 RPC 或队列。当前目录如下。`agent_runtime/driver.rs` 已替换提取/编制两处外层 I/O 循环，`session.rs` 持有活动 AgentRun；业务范围、证据与预算投影继续留在领域模块。

```text
crates/bidding/src/
├── agent_runtime.rs   # 共享 Journal 与版本合同
├── agent_runtime/
│   ├── progress.rs     # 四角色共用的版本进展、局部额度、执行阻塞
│   ├── driver.rs       # 预约、响应、工具提交三个 I/O 边界
│   ├── session.rs      # AgentRun 多轮状态、工具权限和有界会话
│   ├── chat.rs         # Rig 原字节发送、流解析、取消与传输限制
│   └── chat/request.rs # Rig 请求序列化与无网络正文捕获
├── authoring_runtime.rs
├── tender_analysis/    # 来源、提取、关系、独立复核
└── docx_composition/   # 编制、编译、稿件复核、发布
```

复用现有工具 schema 的管理方式、参数校验和执行函数；不为套 SDK 重写全部业务工具，但提取修复必须扩展 `inspect_analysis` 的记录 ID/来源查询、同步 schema、提示词、`tools_sha256` 和测试。该变化只用于新合同/新运行，不能恢复旧检查点。两处旧 Agent 循环删除，其文件收敛为角色配置与业务适配；知识库其他功能仍使用的共享代码不能机械删除。

2026-09-10 候选目录/详情修复：`inspect_analysis` 增加 `view=index/detail`。无 ID 查询默认返回目录（ID、引用、原候选标签及来源），有 ID 默认返回完整详情；目录不计阅读或复核覆盖。详情仍按当前候选摘要确认且完整分页，预算不变。v9 两批真实查询离线回放保留全部当前原文，目录中12/14条不同候选均可逐条完整取回。库147项、合同20项、隔离 PostgreSQL 5项及严格 Clippy/格式通过；新合同须新建运行，32项语义验收仍开放。证据：`artifacts/bid-full-sample/candidate-index/verification.json`。

## 3. 招标提取、上下文管理与模板生成

### 3.1 已定位问题与修复对应

[真实 v5 性能诊断](../../docs/bidding/extraction-performance.md)：22 分 23 秒、30 轮、200 次工具调用、33 次物理调用，请求约 32 KB → 784 KB，27 条未复核记录，0 条关系和 0 轮复核，最终 HTTP 503 终止。轮次耗时包含工具和落盘，不能全部归因于模型计算；字节数不是 token 数。32 项独立验收问题仍开放。

| 问题 | 实施措施 |
| --- | --- |
| 长原文和历史反复发送 | 有界工作上下文、完整协议组裁剪、完成后通过成果引用交接。 |
| 先大段阅读再集中提取 | 当前来源范围内持续保存记录、局部关系和待办。 |
| 反复扫描记录和查引用 | 扩展现有查询，按来源与记录 ID 精确取回。 |
| UTF-8 范围、空语义值错误 | 使用工具准确引用；字段级错误反馈，不自动补造内容。 |
| 附表只有标题或粗略概括 | 保存完整格式，并建立到响应项、证明项和实际单元格的关系。 |
| 阅读覆盖被误认为完成 | 分开记录交付、处置、成果、关系和独立复核。 |
| 503、断流与重试边界 | 单一重试归口、完整响应检查、分阶段恢复。 |
| 耗时无法归因 | 逐请求上下文、用量、HTTP、工具和落盘指标。 |
| 配置未实际采用 | 冻结有效配置并检查最终请求。 |

### 3.2 来源准备与工作范围

修复以真实失败类型建立验收，不以更换运行时或延长提示词作为完成条件。下表编号仅是本样稿测试定位，不能进入生产分支、附件识别规则或固定章节映射。

| 真实问题 | 应改变的执行与反馈 | 可核查结果 |
| --- | --- | --- |
| R01/R02：8G/8H 语义与边界错判 | 在同一工作范围读取标题、完整正文、续页及签署区；对字段解释与来源的冲突给出记录 ID、字段和反证。 | 按真实声明/查询要求保存固定内容与签署归属，不套相邻附件含义。 |
| R04–R07：复合技术条款被摘要吞并 | 按可独立响应的条款提取，保留并列/替代条件、指标、证明和提交时点；局部读完即核对与写入。 | 每个独立义务有准确依据和响应/证明位置，不能只有汇总摘要。 |
| R12：附表关系几乎为空 | 局部记录后立即关联已知两端；未知端显式登记待办，按 ID/来源回查并逐项处置。 | 关系落到响应/证明项和模板区域/单元格，数量不作固定门槛，须核查语义完整性。 |
| R16/R17：混合格与交错顺序错误 | 对模板区域记录原文顺序及固定/待填子区间；返回具体锚点与冲突范围，复核实际单元格和续页。 | 固定标签未清空，标题/单位/表格/说明按来源交错；在 O1-S 用既有编译器验证实际 DOCX。 |

其余发现仍按完整验收索引逐项核对。模型可能持续误判；结构校验只拒绝可确定的范围/身份错误，语义正确性须靠独立来源预期与真实复验，不通过硬编码本样稿答案关闭发现。

复用 Python service 已解析正文、网格、定位、原件和页面视图；未变化文件不重新解析。解析变更产生新来源版本，不混入已冻结输入。Agent 不实现另一套上传文件解析器。

Agent 从索引、真实目录和正文定位组成要求、编制说明及指定格式，并完整核查其他来源中的追加要求。位置只决定阅读顺序，不是固定关键词规则。

工作范围按实际章节、条款、表格、表注和续页组织，再受上下文预算限制。超大章节可分次处理，保留父范围和未完成状态，不固定每批页数。

有界范围必须落实到宿主和工具行为，不只写进提示词：准备请求时按当前范围和必要跨范围引用选取内容，成功交接后主动移除已完成范围的长原文；工具校验真实来源、精确区间、累计返回预算及待交付状态。跨范围查询带明确引用目标，未确认两端进入待办；不能用一次换范围或简短笔记清掉未处置项。范围切换与提交沿用现有工具和 checkpoint 扩展，不新增任务队列；校验能证明结构完整性，不能代替语义验收。

现有工作笔记扩展为结构化工作状态：`source_scope`、`deferred_sources`、`focus`（动作、已交付来源区间与真实候选引用）、`objective`、`status`、有界 `note`。`output_refs/pending_refs` 仅由宿主维护并持久化，不是模型工具输入。它只是当前检查点内容，不形成独立任务队列。每轮只发送有界工作输入和成果数量，未解决引用通过 `check_gaps scope=pending` 分页，执行阻塞通过 `scope=execution` 分页；引用须指向真实成果，模型声明不能单独证明完成。

后续修复已将当前范围的有界 `work_state` 清单直接放进每轮请求，从持久成果及本次实际交付的读取结果推导缺口，避免只有模型主动调用 `check_gaps` 才看到局部阻塞项。清单本身不增加阅读覆盖；主提取与复核分别使用自己的账本。v7 离线投影能直接呈现12项未处置来源及23项未保留成果引用，但真实持续产出仍须新运行证明。

Python v3 解析已交接，Agent `search_sources` 已补齐独立网格搜索：返回真实表单/行列、密铺读取偏移和格内 UTF-8 命中位置，按字节预算分页，引用前仍须 `read_form`。本地样稿冻结脚本同步从 `unit.grid` 构造 schema 3 表单，删除 `locator.cells` 旧提取分支；新版来源必须新建冻结输入，不能拼入旧检查点。检查与证据见[后续验证](../../docs/bidding/agent-runtime-recovery-results.md#解析-v3-交接与当前范围清单)。

2026-09-10 已补齐安全拆分：工作笔记新增有界 `deferred_sources`，允许将活动范围缩小并释放旧窗口；移出的每个来源及已有成果引用必须保留。待处理来源只能通过重新纳入活动范围移出列表，列表未清空禁止主提取提交及复核提交；局部完成、全局阅读、来源处置和独立候选核查门槛不变。v10 离线投影将12个活动来源缩为1个、保留11个待处理来源，完整48格网格和对应原页可同时发送，业务成果和原检查点不变。库148项、合同20项、隔离 PostgreSQL 5项、Clippy/格式通过，真实持续产出仍待验证。证据：`artifacts/bid-full-sample/work-split/verification.json`。

### 3.3 局部提取循环

按“读取 → 提取 → 建立局部关系 → 登记待办 → 检查 → 保存交接”循环处理。

核查范围包括项目事实，文件组成/目录/顺序/提交，格式/签章/资格/否决/评分，商务/技术/报价/人员/交付，指定格式/附表/说明，以及适用条件/例外/替代选项/时点/未解决事项。业务分类不产生固定章目。

粒度以能否独立核查为准。复合条款保留对象、比较符号、阈值、单位、选项关系、证明要求和提交时点，不压成“满足相关技术要求”。网络安全行业的产品、服务、指标、证书、标准和人员条件均从来源产生；样稿名称、参数和附件编号只用于来源和测试，不进入生产识别规则。

引用使用现有搜索和读取工具返回的 UTF-8 范围、网格 citation 和原页身份。搜索仅定位，不计完整阅读。校验失败返回错误码、字段位置和约束，由 Agent 修正；不补默认值或改写原义使数据通过。

### 3.4 附表与对应关系

模板必须保留父子层级、顺序、正文/子标题/表格/表注/签署区的交错、跨页连续性、表头/合并/标签/单位，以及固定文字、招标已知值、投标方待填区和示例值的区别。单元格局部留白必须保留周围固定文字。

关系须具体落到“要求中的响应项或证明项 → 指定模板 → 区域或实际单元格 → DOCX 实际位置”。跨表字段一致性、汇总、替代和适用范围均保留来源依据。相似标题、相同编号或空白外观不能证明关系。

局部无法确认的关系进入待办，后续读取两端和控制条款再判定，不通过标题相似度补齐。原页可以形成视觉证据；生成可编辑模板仍须有可用文本，不能以截图替代。

### 3.5 逐轮上下文

2026-09-10 补齐：`docx_composition` 两个角色此前只检查字节预算，现复用 `agent_runtime::chat` 的保守估算；未填写的 `max_context_tokens`、`image_token_reserve`、`token_safety_margin` 分别采用131072、16384、4096，均可显式配置并写入冻结合同。仅更新现有 baseline 的预算键集合、正值及输出预留约束；旧编制合同不能伪装新配置续跑，提取合同不变，实际 `.env` 和业务库未改。验证及兼容边界见[编制 token 预算](../../artifacts/bid-full-sample/loop-repair/composition-token-budget/verification.json)。

请求包含稳定角色指令和工具定义、冻结身份与必要项目约束引用、当前范围原文/表格/成果、最近必要工具轮次及工作状态。动态进度放在后部；来源放在证据或工具消息中，不提升为系统指令。

每轮同时检查输入 token 预算、输出预留和字节上限，计入工具定义、消息、图像及协议状态。首期使用显式配置的保守估算与余量，保留字节上限并与实际 usage 核对，既不声称 grok 存在已验证的精确 tokenizer，也不将“字节数除以四”视为中文精确计数。Rig `TokenCounter` 只是后续可用接口，不是 P0 的依赖；主动选择当前工作范围才是本次上下文修复的重点。

- 已完成范围的完整原文退出活动窗口，业务成果持续保存；需要复查按 ID 或来源取回。
- 请求超限时先省略已交付的旧导航结果，保留原文证据和完整候选详情；省略标记不是证据，最新待交付工具组保持原样。仍超限时按完整协议组裁剪，不留下孤立工具结果。淘汰优先选择可由其他组覆盖的内容；对实际图片载荷已单独超过历史额度的已交付旧批次，先省略其独立图片消息并留下原图引用标记，保留同批候选、文本和完整工具协议。仍超限才淘汰无焦点证据的完整旧组或按原顺序裁剪；不得因为大图无法留下而无条件连带丢弃同批候选。历史省略标记不产生新阅读回执或语义结果。
- 尚未交付模型的读取结果不得裁剪后仍记为已读。
- 一轮多个工具的总结果同样受剩余输入预算限制；过大读取原子拒绝并返回分页反馈。
- 安全边界创建新的活动 `AgentRun`，保留必要完整末轮；全局预算和业务状态继续累计。
- 不修改 SDK 私有序列化字段清空历史；活动 SDK 状态和发送窗口都须有界。
- 默认不额外调用 LLM 总结，交接依赖已保存成果、待办和短工作笔记；笔记不替代证据。

- 当前角色已读且摘要匹配的焦点候选可从成果状态恢复到有界工作消息；reviewer 还召回当前来源任务指派的 `work_references`，焦点优先。回执、进展和复核结果不因此变化；未读、待交付、其他角色或过期候选不能召回。
- 只有证明在召回预算内可恢复的候选旧详情才可压缩，保留同批原文和工具协议；不能召回的焦点候选仍受保护。工作消息省略冗余逐项摘要字段，宿主仍核验内容摘要，并兼容验证旧消息中的摘要。
- 原图缓存仅保留当前 reviewer 指派来源的已读图片，以及当前角色 `focus.source_spans` 显式指定的已读图片；候选引文不自动固定所有历史图片。访问范围和本角色已读回执仍是前提。
- 最新工具结果与总预算竞争时，先清理已交付的冗余协议组和可退出历史的旧大图，再按实际字节/token超额量逐项移出非焦点候选缓存；保留仍能放下的其他候选，不再仅对原文读取特殊退让，也不一次退为仅焦点缓存。当前焦点不参与此缓存裁减，最新工具组原样保留；必要内容仍放不下时使用既有分页/拒绝边界。排除集合仅存在于该次请求构造，下一请求重新尝试完整召回；不改阅读回执、检查点或累计进展。

### 3.6 已有独立复核的有界化

当前已经有独立 reviewer，不再新建一套 evaluator-optimizer 流程。v5 主提取未结束，复核启动次数为0；首先解决主流程持续产生完整成果和结束判定，不能靠增加提示词要求进入复核。

P0 的请求窗口与交付账本必须同时适用于主 Agent 和 reviewer，P1 再完善语义核查。复核按来源范围逐段读取原文、网格、局部成果和关系，检查后退出活动窗口，按 ID 取回跨范围两端及控制条款；不将106页和全部分析长期留在会话。所有调用继续共用该请求的总预算，复核也必须受单请求窗口、工具结果总量和交接约束。

独立性来自 reviewer 自己取得证据并检查成果，不继承主 Agent 的阅读覆盖或“已完成”结论。首次复核覆盖完整冻结来源，再核对所有成果/关系/不输出决定；工作分段不减少必须核查的范围。独立复读会增加成本，应单独报告其调用、时间和用量，不能承诺免费或精确翻倍，也不能断言复读必然导致503。

修改记录使对应复核及关系摘要失效。首次复核仍覆盖全部冻结来源；后续按 §3.8 的来源关联集合及显式依赖重新核查，无法证明影响局部化时保守失效，不另建依赖图服务。轮数/调用预算耗尽则明确未完成。来源到成果和成果回到来源双向检查，发现记录需含对象 ID（尚无成果的漏项允许仅有来源）、具体字段、证据范围、问题类型与修订后核对依据；该结构化反馈替代笼统“请补全”，不让模型用空 findings 自签成功。

新合同下的归档修复验收使用 `tender_sample repair`：输入为同一冻结来源及原模型检查点，先用生产 `validate_finding` 检查历史来源/候选回执和字段定位，再以新的主Agent运行核查已有模型发现。`review=None`、完整复核轮次0、独立阅读/来源判断为空；保留草稿只提供修复线索，最终完成条件不变。新运行按原检查点摘要冻结来源，旧失败边界不重试或改写，调用消耗在跨运行验收台账累计。此入口用于验收归档修复，不新增产品数据库状态或migration，不允许把人工验收答案注入模型输入。

P1 的发现继续保存在既有 JSON 检查点 `review_draft`，以 `put_review_finding` 增量保存、按 ID 修订/撤回、`inspect_review` 按字节预算分页取回完整条目，reviewer 可用已知草稿问题 `ids` 精确召回；主 Agent 仍分页读取完整复核报告。问题 ID 与候选 ID 分离，未知 ID 明确拒绝，读取不产生语义通过或修改草稿。字段定位为 `affected: [{id, path}]` 与必填 `correction`；证据须由 reviewer 实际收到，涉及成果须独立检查当前摘要。现按 §3.8 同时保存候选局部比较和独立原文判断，由宿主在完整工具批次末核验、汇总及切换角色；已删除 `submit_review` 的工具/schema/提示词和旧离线调用测试，保留历史诊断证据。

原 `submit_review` 后端并不强制工作笔记完成，早期问题是模型没有发出最终调用，且缺少显式原文漏项结果。新机制不依赖重复笔记收尾，不将阅读回执或空 findings 解释为完整语义通过。新增领域状态保存于现有提取 checkpoint，所属 baseline 校验同步；没有新增表或独立 migration，真实附表及关系验收继续开放。

### 3.7 目录、模板与出件

编制按 PRD 顺序：固定投标目录 → 规定组成及顺序 → 适用固定格式 → 要求单独呈现的评分/响应/证明 → 无规定部分依据项目义务提出结构并记录理由。

Agent 按章节增量编制，复用 DOCX 编译工具，填入有来源的固定文字和招标已知信息，生成完整附表/签署区，按要求放入原条款及独立待填响应，绑定要求/模板/实际位置，保留条件性内容及有依据的省略决定。不能一次模型回答生成整本文件，不能填造投标方事实。

稿件复核检查实际 DOCX、位置映射及渲染结果，不只检查 JSON。ONLYOFFICE 保存、排版和转换后，DOCX/PDF/报告绑定同一最终版本。仍输出整本稿，单独提交或分册要求保留提示，由用户导出后拆分。

问题可随草稿展示，不新增禁止编辑或导出的业务闸门；存在未解决验收问题时不能标记完整通过。现有自动编制路径的复核前置和技术发布校验属于代码现状，不得误称所有未复核分析已经可编制。

### 3.8 双向复核与确定性收尾

#### 3.8.1 已知故障与修复边界

`clean-review-resume1` 第40轮完成41条记录、28条关系、3项来源处置的72个局部比较，随后继续读取；第58轮进入执行阻塞，第64轮耗尽交接额度停止，最终复核轮数为0。第54轮检查点副本中正式工具可完成提交，仅证明接口可调用，不是实际语义通过。两段运行累计65次物理调用、请求25144–396023字节、未出现503；不能把本次未收尾继续归因于请求过大或提交 API 不可用。证据见 `artifacts/bid-full-sample/loop-repair/clean-review-resume1/` 下的 `finalization-observation.json`、`finalization-probe/report.json`、`terminal.json`。

故障时工具能表达“这个候选版本比较过了”，却没有独立的“这段原文还有没有漏提”结果；全局收尾又依赖模型继续主动发出空提交调用。仅补提示、调大轮数或看到72个比较就自动通过，均不满足目标。源码与轨迹只能定位这些协议/状态缺口，不能证明模型内部为何继续重读。

| 当前问题 | 本次措施 | 能证明什么 |
| --- | --- | --- |
| 比较完成后仍反复检查、没有最终提交 | 宿主派生待办，在完整工具批次末自动核验并汇总 | 必要判断已完成时无需模型再决定是否收尾；未判断时有界退出 |
| 候选都正确仍可能整段漏提 | 独立原文任务及显式漏项判断，与候选比较分账 | 必须出现来源到成果的判断；准确性另以负例验收 |
| 8G/8H边界、复合技术条件、关系、混合格与交错排版 | 小范围语义核查，完整表格/续页证据，具体字段修订 | 以对应真实问题逐项复验，不用运行时测试关闭32项发现 |
| 关系文本出现字面 Unicode 转义链 | 查清原始响应、参数解析、对象和落盘边界后定点修复 | 正文可读且原文与合法反斜杠不被破坏 |
| 503与速度 | 保留现有有界请求及预约恢复，减少重复调用并分项计时 | 不承诺消除供应商503，不以换 SDK 或更高预算作为修复 |

职责保持不变：Python DocReader 提供统一解析与原页；业务 Agent 解释招标要求、边界和适用性；Rust 宿主负责证据/版本校验、派生待办和状态推进；Rig 0.42.0 负责 Chat 多轮协议；既有 Journal 保存同一执行状态。不开第二个 Agent 进程，不新增模型、Embedding 依赖、调度队列或 Agent 表。本切片先用于招标提取 reviewer，编制/稿件复核继续复用已有运行保护，不顺带重写其业务合同。

#### 3.8.2 有限任务清单与语义边界

复核包含两个都必须完成的集合：

1. **候选比较：** 所有当前记录、关系、主提取来源处置的版本。沿用 `complete_review_check` 和有证据的 finding；阅读回执只证明本角色收到内容。现有来源处置的比较不升级为漏项核查。
2. **原文漏项核查：** 宿主从完整冻结输入派生必核查来源片段，不能由模型挑选“重要页”构造全集。身份为冻结输入摘要、来源 ID、片段范围与任务划分版本的摘要；不写死本样稿页数、附件编号或预期答案。

片段复用现有文本 UTF-8 区间、网格实际 anchor 及原页引用：普通来源可整体处理；超预算来源按冻结预算和已有分页能力确定性拆分，文本不截断字符，合并单元格不拆 anchor。网格每个实际单元格（含空格）都有任务归属；标题、表头、单位、说明和必要原页作为上下文依赖，不因分页消失。单个必要证据仍装不下时明确执行阻塞，不截断后签收。不再引入另一套解析器或版面切分算法。

**片段是覆盖与调度单位，不是语义边界。** reviewer 核查完整条款的条件/例外、模板标题到签署/说明的归属、表格间正文及跨页续接。任务结果须解释片段两端是否完整；续页或控制条款通过精确来源引用登记依赖。仅当前页出现标题、读取了一页、主提取标为不输出，都不足以完成。跨片段语义单元可共享证据，但各片段仍须有自己的有效判断；不以合并任务删除待办。边界不清进入补证，真实材料不足则形成带来源的发现及既有 unresolved 处置。 `continuation` 至少引用一个本片段之外且 reviewer 已读取的目标：同源文本/同表格按前后方向核对字节或单元格范围，跨来源引用须通过冻结来源与阅读回执校验；本片段末行或同源整页图片本身不证明续接。跨来源引用仅证明存在独立目标，语义归属仍由 reviewer 判断。恢复时派生完成状态执行同一校验，不合格旧回执重开待办并在 `prior_boundary_error` 说明原因，不改写旧回执或刷新预算。

首次完整独立复核必然需要检查全部来源，成本不能省略；请求中只放当前片段、相关候选、必要跨来源依据和有界历史，不把106页再次拼进单轮。

当前原页保留补充：`category-feedback-review-trial` 第8轮请求已无图片，随后 reviewer 虚构了原页不存在的前附表优先句。缺图与误报都已确认，但因果强度不能由轨迹证明。现从 reviewer 自己的已提交阅读回执和不可变图片缓存，为当前原文任务补入同一原图；它属于工作证据而非旧历史，仍计入总请求字节和 token 限额，不增加阅读回执或进展。其他角色的回执不能授予独立阅读；超预算不得悄悄丢弃当前原图后继续声称已看图。保留图片不等于语义正确，误报须在真实闭环中复验。

候选查询与原图共存补充：原图移出旧历史后仍在最终请求中，但 `inspect_in_context` 只检查历史库存，导致单候选也被误拒绝。现对缺失的 `view:<id>` 精确核对最终序列化消息中的原图身份与像素；文本、网格和焦点候选的原有保留校验不变，仅有缓存不算可见。试算成功前不授予阅读，错误分支恢复临时状态。红/绿回归、195项库测试、20项合同、7项隔离 PostgreSQL、Clippy/fmt/编译及两项离线回放通过；证据：[候选与原图共存校验](../../artifacts/bid-full-sample/loop-repair/wire-view-inspection/verification.json)。这不等于真实语义验收通过。

原文判断收尾补充：当前任务清单提前列出 `completion.required_finding_ids` 和 `status_if_evidence_complete`，复用提交校验的同一依赖计算，包含跨来源及此前已指派但候选被删除的问题。该提示不形成判断，不代替缺证处理；新增引用仍可能扩展必需集合。补齐先看清单再提交的红绿回归，195项库测试、20项合同、Clippy/fmt/编译与两项上下文回放通过；[证据](../../artifacts/bid-full-sample/loop-repair/source-completion-packet/verification.json)。该收尾清单已随 resume5/resume3 的兼容分段应用，旧归档二进制未覆盖。

旧混合候选批次补充：真实第91轮投影中，8个精确单候选查询有4个被拒绝。旧协议组同时含焦点证据和已离开焦点的详情，整组淘汰会损失必要证据。现仅在无冗余/无焦点整组可先删时，标记省略已交付的非焦点详情，保留原文、当前焦点完整值及最新未交付组；历史分页数字原样保留并说明其历史含义。原预算下8个查询全部通过、请求396300字节；旧的较强混合批次断言继续通过。[证据](../../artifacts/bid-full-sample/loop-repair/candidate-batch-retention/verification.json)：195项库测试（12项忽略）、20项合同、7项隔离 PostgreSQL、Clippy/fmt/编译及3项回放通过，无新schema、表或配置项。`appendix-declarations-resume5` 从第204轮/211次调用、`category-feedback-review-resume3` 从第97轮/99次调用恢复；原二进制和日志未覆盖，当前仍不算语义通过。

单元格提示澄清：后续需要填写不等于原格已有值需要清除。仅含标签、提示或输入空位时保留原文，按实际用途选择 fixed_text/instruction/signature；不要为了建立空白位置虚构 blank_ranges。原始空格仍须有来源支持其 bidder_blank 用途，实际示例/输入值需要去除时才使用局部范围。未新增校验例外或招标词典；[工程验证](../../artifacts/bid-full-sample/loop-repair/prompt-cell-policy/verification.json)通过，新提示词采用 `appendix-layout-policy-trial` 空候选身份，不续接旧提示词检查点。该通用提示是否解决模型的布局判断必须由真跑证明。

候选待办清单补充：任务包旧 `current.candidate_refs` 每轮重复全部关联候选，完成比较后仍占据待办位置。现改为 `comparison_total` 与分页 `pending_candidate_refs`，由当前有效比较结果派生，仅列尚未完成的引用；候选修改或跨来源端点失效会重开待办。这里调整的是展示字段，`put_source_review` 输入仍使用 `candidate_refs`，提交时仍校验全部关联候选和原文判断。清单为空不能签收或批准，模型须继续判断漏项与边界。[工程验证与恢复记录](../../artifacts/bid-full-sample/loop-repair/pending-comparisons/verification.json)已保存；真实恢复第一轮保留原预约正文，后续轮才使用新清单，仍待验证是否减少重复读取并完成判断。

附表分组闭环证据：`appendix-declarations-resume6` 完成第231轮，最终三项原文判断均有效、41个当前候选比较完成，第三轮复核无未解决发现；[结构审计](../../artifacts/bid-full-sample/loop-repair/appendix-declarations-resume6/acceptance-audit/structural-report.json)及[独立分组核对](../../artifacts/bid-full-sample/loop-repair/appendix-declarations-resume6/acceptance-audit/semantic-report.json)通过。8G十项固定声明、最近三年条件和跨页签章保留，8H保留两类信用查询截图证明且未挪用8G签章。这只关闭本次分组提取核对，R01/R02的全稿与DOCX验收仍开放。累计239次调用与前序重试/修复均计入；单独最后恢复段不能充当全流程性能。另从该模型成果删除一项要求及其关联边，启动 `negative-missing-requirement-trial`，人工缺陷答案置于来源目录外；结果待验。

缺证状态反馈补充：3来源隔离复核反复在无外部目标的 `continuation` 与 `unresolved + checked` 间切换。现把缺少 reason 与状态冲突拆为精确字段错误，并明确补证中的 `needs_evidence + evidence_requests`，以及冻结集合确实缺源时先保存 source-only finding、完成比较后提交 findings 的合法路径；不允许用改名 complete 来替代证据。[红绿回归与196项库/20项合同检查](../../artifacts/bid-full-sample/loop-repair/boundary-feedback/verification.json)通过，未修改通过条件或运行中旧二进制。该反馈不保证模型边界解释正确。

工作规划与证据校验分离：真实负例多次先用 review 范围声明有效未读文本位置，宿主拒绝“未读引用”，紧接着读取同源又因尚未扩展范围被拒。工作笔记是计划，因此所有 action 现可声明合法未读文本位置；source_scope、UTF-8几何和实际成果/发现/比较/完成所需的独立阅读校验保留，grid/view引用仍须已交付。[红绿回归、196项库测试、20项合同、7项隔离PG及工程检查](../../artifacts/bid-full-sample/loop-repair/planned-focus/verification.json)通过。描述/提示词摘要已变，`negative-missing-requirement-planning-trial` 用同来源/同候选种子/同.env预算的新身份对照；旧负例第39轮因执行和交接额度耗尽而失败，保留正确漏项finding但不算完整复核通过。

#### 3.8.3 最小工具与持久化结果合同

保留读取、候选比较及增量 finding 工具，只新增 reviewer 专用 `put_source_review`；扩展现有导航返回原文任务及当前状态。新工具输入字段如下，全部使用真实引用：

| 字段 | 约束 |
| --- | --- |
| `task_id`, `expected_version` | 宿主发出的片段身份与当前依赖版本；过期或外来任务拒绝，模型不自定完成范围 |
| `status` | 仅 `checked`、`findings`、`needs_evidence` 三种语义结果；执行阻塞由宿主维护 |
| `summary`, `sources`, `candidate_refs` | 具体判断、独立收到的精确证据、比对的当前候选；候选为空仍须解释不输出或漏项依据 |
| `boundaries` | `before/after` 各含 `state=complete/continuation/unresolved`、`reason`、`sources`；续接须精确证据，未解决不得签为 `checked` |
| `finding_ids` | 引用已经保存的有效发现；`findings` 必须非空；漏项可以只有来源，无需先虚构被遗漏记录的 ID；不匹配时返回有界的必需ID列表与期望状态 |
| `evidence_requests` | `needs_evidence` 必填具体问题与来源目标或有界搜索范围；其他状态为空，不接受“再检查全部” |

`checked` 表示本片段已明确判断没有尚未报告的遗漏/错误，要求无待补证、无未解决边界、无影响该结论的活跃 finding。`findings` 表示本片段核查完成且问题已完整保存，允许结束本轮并反馈修订，**不表示通过**；仅找到一个问题、尚未检查完其余原文时仍未完成。两种完成结果都要求本片段覆盖、全部相关候选版本及声明依赖满足交付校验。`needs_evidence` 只保留问题，不计完成；收到证据后由 reviewer 提交新的判断，宿主不能把读取成功自动转换成 `checked`。

主 Agent 显式完成局部工作且全局结构缺口、deferred 来源及执行阻塞均为空时，若候选已相对上一复核变化，下一请求明确提示 `request_review`；旧复核报告在 reviewer 重新核验前仍保留，不要求主 Agent 自行清除。未变化的候选不标为“已修订”，导航不代替模型的显式复核请求或独立通过。

原文真实缺失或已由冻结输入/既有证据确认不可用，继续按现有 open items 与有依据的 unresolved 规则记录；此类处置必须显式进入独立核查，不能伪造阅读覆盖。临时读取失败、模型没有找完或执行额度不足不能转换成“招标未提供”。若现有覆盖合同仍不允许结束，保持未完成并报告，不能为收尾放宽它。

同一任务/依赖版本/等价结果幂等；改写 summary、重复找同一证据不能刷新进展预算。首次有效结果或真实新增证据/修订沿用已有进展政策。连续补证无新增依据进入现有重规划/阻塞流程，不允许把旧无限重读换名成无限 `needs_evidence`。

在现有提取 checkpoint 增加一个有类型的 `source_review` 域，保存域版本、任务清单摘要、各任务结果/补证请求及其依赖。任务清单由冻结输入与配置派生，不另存一套可编辑队列；已完成结果保留审计，但只有当前依赖匹配者计数。`Progress.seen` 继续承担既有去重，不把仅有哈希的集合当作原文语义结果。结果摘要、发现与引用在检查点中可审计，长原文仍按引用取回；不增加环境变量。

#### 3.8.4 版本失效与修订回合

不能只比较旧候选自身的摘要：新增一个遗漏条款、删除一张附表、改变跨页关联，都会影响原文结论。宿主计算并校验依赖，模型声明只能增加依据，不能缩小宿主的必要集合。

- 对每个来源维护派生关联集合：引用该来源的记录/处置，加上这些记录参与的关系、关系两端及显式依赖；集合摘要包含种类、ID和内容版本。变更按旧、新引用的并集失效，因此移走引用也不能留下旧完成结果。
- 原文结果绑定本片段、实际候选查询范围、实际比对候选及引用finding的版本。正文/网格读取和所有sources引文只证明冻结原文出处，保留于判断本身并受输入摘要及独立阅读校验约束，不自动要求比较每个被引用页面的全部候选。实际跨源候选比较通过候选查询、candidate_refs、映射或关系ID登记依赖；同页模板原图复核仍保留整页排版关联集合。结果取回或状态派生时重新计算，不依赖模型主动说“需要重审”。候选比较的去重摘要除自身版本外，绑定宿主可从其来源引用、相关关系/两端及规则集合重算的依赖；额外跨来源判断记入对应原文任务。关系两端变化不能保留旧无问题结论。
- 全局控制规则的适用范围变更按保守策略使原文结论失效；无法确定局部影响的编辑同样保守失效。首期直接使用现有 rule 集合摘要作为全局依赖，不实现语义影响推理服务。普通不相关局部记录编辑不要求全文件重读。
- “没有对应候选”属于候选集合判断：本来源范围内查询候选无结果绑定该来源集合；全局查询候选后的否定结论绑定整个分析摘要。宿主记录候选查询范围和取回依赖；无法确定候选查询影响范围时绑定全局摘要。这样新增/删除以及原候选改成匹配内容都会使否定结论失效，不能只保存搜索字符串或命中 ID。`search_sources`只检索冻结正文和表格，与候选及复核发现无关，命中和未命中均只依赖原输入摘要；不能因为原文搜索而绑定全局分析。后续候选查询和显式比较ID增加相应候选依赖；正文/网格读取及原文引文本身不查询可变候选集合。原图排版复核仍按同页模板集合登记依赖。
- 新增、修订、撤回 finding 会使关联结果重新待核查；撤回不能自动变成无问题。修订轮开始时保留上一轮发现供主 Agent 使用，并将仍有效发现纳入新草稿；失效的发现保留旧审计、要求重新确认，不能无条件清空草稿后复用原 `findings` 结果。

冻结原文未变时，reviewer 自己的真实阅读回执可复用；语义结果失效不等于必须重新传全部原图，当前判断所需证据仍须进入有界活动上下文。首次覆盖和每次失效的重审工作分别统计。仅当已登记依赖支持局部判断时精确失效；宁可明确扩大重审，也不能承诺永不全量失效。

#### 3.8.5 宿主自动汇总与原子边界

宿主在**完整响应中全部工具按序执行完后**，从最终状态运行统一 reducer；不能在同批第一个完成工具后提前发布，后续 finding/撤回/失败结果仍须处理。模型负责语义判断，宿主只做可验证的条件路由：

```text
主提取 request_review
  → 派生候选/原文待办
  → reviewer 局部读取、判断、保存结果
  → 批次末校验
      有未完成/过期/补证任务 → 选择下一待办
      有执行阻塞/额度耗尽 → 保存未完成终态
      必要任务齐备 → 汇总 Review
          有发现且可修订 → 主 Agent 按字段修订 → 重核受影响任务
          无需/不能继续修订 → 按既有质量规则结束
```

汇总同时要求：主提取结构有效；两个集合的当前版本全部有有效判断；完整独立覆盖；无待交付的必要证据、延期来源或补证；无执行阻塞；所有 finding 均有效且保留；分析摘要和源任务清单身份一致。缺任一项返回具体待办，不以计数相等代替集合成员/版本相等，不以空 findings 代替原文核查。失败的语义写入不能完成相应任务；后续合法结果只能解除其实际解决的缺口。

将现有 `submit_review` 中的发现验证、轮次计算、`Review` 保存和角色切换收敛到此宿主函数；同批业务结果、工具响应、原文结果、最终 Review/角色/done 作为第三边界的同一 checkpoint 事务提交。提交失败不发下一请求；ACK 丢失恢复读取已提交状态，不再次增加 review_rounds 或重复发布。响应已保存但工具未提交时重放原工具批次；同批已完成不代表可以跳过其中后续工具。取消与 owner 失效沿用既有围栏，不能离线补写最终状态。

删除 reviewer 工具列表/schema/提示词中的 `submit_review` 及“先完成笔记再空提交”指令，不保留废弃别名。`set_work_note` 仍可用于主提取和真正的局部范围规划；reviewer 收尾不依赖模型维护一个重复的完成标志。最后一个必要语义结果满足门槛后，额外收尾模型调用应为0；下一角色若确实有修订任务，其调用正常计数。

**完成与通过分开。** 现有 `AnalysisResult::expected_quality` 只有 `verified` / `needs_review`：findings 与 source open items 均空才是前者。`done=true` 只是结束循环；发现未解决或材料真实不足继续按既有 `needs_review` 及消费/发布校验处理，不新增 `quality=rejected` 枚举。重复未修正的分析、复核轮数耗尽可以结束诊断但不能伪装通过；执行阻塞则不得产出可接受分析。外部验收的 rejected/未通过不得与领域质量字段混写，也不新增禁止用户编辑/导出的产品规则。

#### 3.8.6 导航与窗口推进

既有动态工作包和查询目录增加当前任务/版本、阅读状态、语义状态、待补证、失效原因及剩余任务；默认显示未完成项，已完成详情仍可按 ID 查。待交付候选、剩余比较与任务完成校验均须包含源任务拥有的主体及直接关系端点；实际查询依赖继续参与失效，但不单独扩大必做清单，具体规则见§3.9。宿主在当前任务完成后选择稳定顺序中的下一个可执行任务，已完成任务不继续作为默认焦点；Agent 可为明确的续页或控制条款切换到其他必要依据。跨范围取证不篡改任务身份、不刷新预算，发现新问题仍可写 finding 并使旧结论失效。

`inspect_analysis kind=all` 已依据真实错误修正为记录、关系、来源处置的统一查询；`kind=record` 明确只查全部记录类型，各具体类型查询保留。混合ID按实际类别解析，未知或多类别同ID报错；按完整引用稳定分页，只对实际返回详情记录阅读回执。同步工具摘要并使用新合同身份，删除“all只查记录”的旧语义，不新增检索入口。

完成后的长证据退出默认窗口，必要跨引用可重新取回；读取已完成证据不是一律禁止，只是不计新进展。新任务的原文和关联候选优先于旧导航。所有角色继续共用冻结总预算，预算过小只能报告不能完成，不能静默增配。

#### 3.8.7 字面转义问题的定位及修复

对已发现的关系 `01e987a8-ae53-4b03-abe1-cfd828b4f4f3` 的 `scope/explanation`，依次比较归档 SSE 还原的工具参数字符串、Rig 返回参数、一次标准 JSON 解析后的 Value、写入前对象与 checkpoint。报告首个出现错误的边界及证据摘要；JSON 文件正常显示的 `\uXXXX` 和字符串实际包含的反斜杠字母序列必须区分。

若是协议转换多编码，在该边界修正并测试；若模型生成的字段本身就是转义链，返回字段级可读性反馈，要求 Agent 根据已读来源改写，并使旧比较失效。不能未经定位假定 SDK 有错。不得对全部字段、原文或历史数据运行第二次 Unicode 解码；路径、正则、代码、字面转义示例及合法反斜杠必须原样保留。检测不能仅凭出现 `\u` 判坏，只有定位确认的自然语言字段缺陷才进入定点修复；这也要进入独立复核的文本可用性检查。

#### 3.8.8 最小改动面与合同升级

| 所属位置 | 必要改动与限制 |
| --- | --- |
| `crates/bidding/src/tender_analysis/agent.rs` 及其 `context.rs` | 工具合同、typed source_review、依赖版本、待办导航、批次末汇总；优先保留在现有模块，规模确实需要才拆同目录文件 |
| 同目录读取/校验模块与 `tests.rs` | 复用片段分页/覆盖；记录查询依赖；增强当前候选判定，增加语义结果和失效回归 |
| `migrations/bidding_v2_baseline.sql` 及现有 PostgreSQL/合同测试 | 更新提取 checkpoint 初值（source_review 初始为空）、合法阶段变更及发布终态约束；不增加表和独立 migration 文件 |
| 既有样稿/诊断测试入口 | 保存新任务/状态指标，移除生产空提交工具依赖；历史证据只读，不注入验收答案 |
| 本方案、总台账和相关导航 | 只同步进展/入口，工具与失效规则在本节定义一次 |

新增 checkpoint 字段属于必要的领域结果记账；现有 Serde 严格字段和 SQL 初值不能忽略。SQL 继续验证输入/配置、单调计数、合法 Journal 边界及最终结果一致，并补齐 source_review 初始化/终态必备字段、未完成结果不能发布的合同测试；语义依赖/覆盖算法在 Rust 校验，不复制一份 SQL 语义推理。发布前 Rust 必须从冻结输入重新派生清单和依赖，不能相信模型或检查点中的 completed 总数。

工具/提示词摘要变化必须新建运行身份，旧 checkpoint 不能默认补空字段后宣称兼容，旧72项比较尤其不能当作已做过原文核查。source_review 使用自身域版本；本次不改变共享 Journal 三边界语义；隔离末轮恢复发现的参数键顺序问题只修复会话比较投影，不改已预约正文或持久化格式。不因领域字段增加而机械升级 `checkpoint_contract_version=3` / `rig-chat-0.42.0/3` 或编制 checkpoint。若实现证明共享边界确实需改，须单列必要性与双方回归后再调整本决定。只维护未发布 baseline 和隔离验证，不操作现有业务数据库。

借鉴方式沿用 §1.2 的官方案例和前述 Pi/OpenCode/LangGraph 机制：有界任务上下文、工具后的状态路由、持久化局部结果、按有效状态检测停滞。这里采用可验证的机制，不引入另一套框架，也不声称开源 Agent 能保证招标语义准确。宿主可以保证门槛具备时确定性收尾和失败有界；配置模型能否正确完成全部判断仍须真实验收。

### 3.9 已实现修正：查询依赖与来源任务职责分离

2026-09-12 修复前的修订候选独立复核未通过：相同第一个来源任务在16轮内没有完成来源判断，但必做关系判断随跨页查询从2项增长到6项，新增4项均不引用该任务所属来源。另有13次候选比较引文错用，不能把所有模型错误都归因于任务扩张。见[真实轨迹与修复入口](../../artifacts/bid-full-sample/loop-repair/relationship-subject-originals/query-obligation-diagnosis.json)。原文补回已实装且真实返回过；以下职责分离现已实现。离线同一第16轮必做关系主体由6项变为1项、必做比较由26项变为9项，查询依赖版本及原检查点保持不变，详见[回放](../../artifacts/bid-full-sample/loop-repair/source-task-ownership/archived-roster.json)。工程验证及实际续跑观察以上方当前记录为准，未据此关闭完整验收。

修正必须区分两件事：查询得到的对象可以影响当前结论，需要进入失效依赖；它自身的完整来源、关系和附表判断，不应仅因被查阅就全部变成当前任务的必做工作。

1. 从当前任务拥有的原文及直接相关关系推导必做比较、关系主体和模板映射主体。外页目标作为支持证据，继续检查其当前值、原文和实际端点；它自身的完整判断由其所属来源任务负责。
2. 查询范围、查无结果、实际引用端点及原页布局依赖继续参与版本计算。候选、关系、条件或相关发现变化，仍使依赖它的结论失效；不得通过缩小必做清单丢弃失效依赖。
3. 同步修改导航、工具返回、工作缺口、提交验证和历史判断有效性计算，禁止导航已完成而提交仍要求整份查询清单。共享原页的文字/网格模板继续接受整页图片与交错排版检查。
4. 复用冻结输入、既有任务和Journal，不新增任务表、人工业务归属或修订账本。新增字段若只是可推导的职责，不写入检查点。旧运行的正文、回执和调用计数保持原样；已耗尽边界不得重置。

验收先用同一状态证明：跨页查询改变依赖版本，却不增加当前来源的必做职责；相关边和目标变更仍触发重审；目标所属来源未完成时，全局复核不能通过；缺失要求、模板映射和跨页续接仍能被检出。随后在保留累计预算的合法运行边界做短范围真实验证，只有真实来源判断持续完成后，才推进完整106页、32项及DOCX/PDF验收。

后续真实闭环已完成5轮但未通过：同一缺续页发现持续保留，主Agent已写入有来源的unresolved。第48–51轮没有业务写入，仅查询和再次请求复核，仍产生第3轮并再次交回主Agent。当前`finish_review_batch`以包含主角色阅读回执的整个Analysis摘要判定重复；回归已确认候选读取改变整个摘要，但业务记录/关系/处置和独立判断保持有效。见[下一处定位证据](../../artifacts/bid-full-sample/loop-repair/source-task-ownership/repeat-review-diagnosis.json)。现已在主Agent成功请求复核的工具批次末复用review_complete：已有独立复核且全部判断仍有效时，直接汇总当前结果并保留未解决发现，不再追加供应商调用；真正的修订、缺失判断或无效边界仍进入复核。完整摘要和最终拒绝合同保留，不新增语义快照。隔离红绿及共享工作区253项库/20项合同/严格Clippy、编译及7项隔离PG已通过，全局fmt仍有knowledge并行拆分差异，详见[修复证据](../../artifacts/bid-full-sample/loop-repair/unchanged-review-handoff/verification.json)。此局部冻结集合确实缺续页，终态不能靠继续重跑、改写说明或追加来源冒充通过；全文验收仍须使用完整冻结来源。

## 4. 运行合同、预算、持久化与故障

### 4.1 接口与请求合同

P0/P1 保留现有驱动修复提取/复核，P2 取得真实语义证据；P3 升级独立 Journal；P4 验证切换后由 Rig 管理多轮状态和 Chat Completions 协议，共享运行层管理 I/O 边界，业务模块管理来源语义和成果。

| 内部合同 | 核心内容 |
| --- | --- |
| 运行合同 | SDK/适配版本、协议、模型配置、能力、预算、提示词和工具摘要。 |
| 待发送调用 | 调用序号、角色、准备完成的输入、最终正文和摘要。 |
| 检查点 | 独立检查点序号、活动 Rig 状态、工作/业务状态、待交付证据和累计计数。 |
| 用量记录 | 单次实际用量、估算值及供应商是否报告。 |
| 进度信息 | 当前范围、成果、关系待办、复核问题和调用状态。 |

保留既有 API 路由及下载方式，在既有进度响应中扩展字段，不增加一级产品流程。

| 校验对象 | Chat Completions 合同 |
| --- | --- |
| 输入及指令 | `messages`，首条 system 与冻结提示词一致。 |
| 输出预算 | `max_tokens` 与冻结配置一致。 |
| 工具往返 | tool calls / tool messages，调用 ID 完整配对。 |
| 终止状态 | 完整 SSE 与可接受结束原因；截断或错误不执行工具。 |

P4 的接缝路径为：Rig 完成序列化 → `HttpClientExt::send_streaming` 接收 `Request<T>` 并转换正文为字节 → 对该 JSON 做 JCS 规范化 → 验证新版本 Chat Completions 合同并预约 → 将同一份字节交回成熟 HTTP 客户端发送。不复制 SDK 的字段组装逻辑，不在预约后再次调用 SDK 重建请求。本机七场景已编译运行，实际捕获的预约/发送字节相同、预约拒绝零发送、503 无嵌套重试，工具配对/图片及完整终态检查通过；后续已通过公开的原始请求发送接口接入生产 Chat 流解析及既有 Journal；发送使用已预约正文原字节，请求序列化现已由兼容供应商构造器完成，并通过准备专用 HTTP 接缝交回最终 JCS 正文；AgentRun 多轮状态及有界交接现已接入并通过隔离恢复测试。详见[接缝与恢复记录](../../docs/bidding/agent-runtime-recovery-results.md)。

SSE 结束边界补充：本地真实 HTTP 测试确认，完整工具结果、`finish_reason=tool_calls`、usage 和 `[DONE]` 已交付后，如果连接不关闭，Rig 0.42仍等待 EOF 才输出汇总，导致本地超时。现复用 Rig 已依赖的 `eventsource-stream 0.2.3`，在完整 `[DONE]` 事件后结束内部响应流；不自行解析工具 JSON，不改预约正文、模型、超时或重试次数。195项库测试、20项合同、2项本地 HTTP 接缝测试、Clippy/fmt/编译通过；[红绿证据](../../artifacts/bid-full-sample/loop-repair/sse-done-boundary/verification.json)。新增仅含字节数、chunk数、SDK事件数和耗时的日志，不含正文或凭据。历史真实超时未保存完整流，不能据此认定当时收到了 `[DONE]`；以新真实轨迹继续区分供应商未完成与本地终止问题。

### 4.2 三个持久化边界

1. **请求已准备并预约：** 原子保存待发送状态和调用尝试，校验执行权/预算后发送。
2. **完整响应已保存：** 终止状态检查通过后保存模型结果、待执行工具、用量和恢复状态。
3. **工具结果已提交：** 业务成果、工具结果、覆盖状态及新 Agent 状态一起保存，才允许下一次请求。

活动 SDK 状态与历史审计分开，避免每轮重复保存全部原始响应。大对象和原图优先保存引用与摘要；日志只记录脱敏指标，不输出正文、参数或凭据。

读取工具先登记待交付。完整内容进入请求并取得有效响应后才确认交付给该角色；复核不能继承提取阅读覆盖。写工具默认顺序执行，保留一轮多工具，本期不引入嵌套 Agent 和并行语义写入。

### 4.3 预算与结束条件

- 同一业务请求的所有角色、分段、纠错和物理尝试合并计数；提取和编制沿用各自冻结请求预算。
- SDK 局部轮数只使用全局剩余额度，切分段不重置；不另建整链调度账本。
- 保留每个请求边界最多三次物理尝试，关闭其他隐式重试。
- 阶段结束必须经过业务校验；提取 reviewer 按 §3.8 改为完整工具批次末由宿主汇总，其他阶段沿用现有提交合同。自然语言“已完成”或 SDK `Done` 不触发业务发布。
- 提交成功直接交接下一阶段，不追加一轮总结调用。

| 情况 | 行为 |
| --- | --- |
| 429、503、超时等临时错误 | 同一预约策略内重试相同正文，使用明确等待策略和既有次数上限。 |
| 不支持的协议、参数或配置 | 明确失败并保留现场，不自动换协议/模型或删字段。 |
| 上下文超限 | 报告预算或能力配置问题，不让供应商自动截断来源。 |
| 工具参数或业务校验错误 | 返回结构化反馈，后续修正调用继续计入预算。 |
| incomplete、failed、异常 EOF | 保存诊断和可获得用量，不执行该轮工具。 |
| 检查点失败 | 停止后续调用，恢复至最近提交边界。 |
| 执行权失效或取消 | 停止外部调用与写入，沿用 Worker 生命周期。 |
| 预算耗尽 | 保留成果与检查点，明确未完成，不提高额度。 |

恢复等待模型时重放冻结请求；恢复待执行工具时使用已保存模型结果。业务成果和回执共同提交；外部对象操作复用既有摘要、注册和发布回执。供应商已处理而本地尚未保存响应时可能再次调用，所有尝试都计数，不宣称模型计费恰好一次。

**503 成因与恢复分开验收。** v5 的最后预约正文约784 KB，同边界三次503已耗尽；检查点升级既不能证明其成因是上下文大小，也不能使相同请求必然成功。不得在同一预约内裁剪正文重试或重置计数。P0 在新调用准备前主动缩小工作范围，改变提示词/工具/窗口合同后用新运行身份验证；旧 v5 保留失败终态。P3 的恢复测试只证明响应/工具状态不丢、预算不重置，供应商503单独记录，不能宣称被 Journal 或 Rig 修复。

### 4.4 数据库最小改动

旧检查点合同每次必须前进一轮，不能表达同轮内三个边界。P3 切片已将检查点序号与模型调用序号分离：检查点表按独立 sequence 排序，调用尝试表仍按 turn 计数，当时冻结合同升级为2，与是否采用 Rig 无关。随后 P4 将活动 SDK 会话纳入同一 Journal，当前合同为3，适配身份为 `rig-chat-0.42.0/3`。P0 的工具查询与请求窗口改动不以此 SQL 改动为前置。

复用现有检查点表和调用尝试表，调整预约、检查点读写和发布校验，扩展运行/检查点合同版本，保留输入身份、执行权、正文摘要、单调计数和重复提交约束。不新增 Agent 表，不新增历史 migration；按[未发布 baseline 策略](../platform/runtime-foundation.md#2-fresh-baseline)维护所属 baseline 并同步权限、schema 和合同测试。

旧失败运行保留，不修改为新格式冒充恢复。代码实施和隔离验证不包含对现有运行库执行迁移、清库、重置、部署或 Git 提交。

## 5. 实施顺序、测试和交付

### 5.1 阶段台账

| 阶段 | 工作内容 | 完成条件 | 当前状态 |
| --- | --- | --- | --- |
| Rig P0 | 当前驱动下精确查询、局部提取、主/复核有界窗口和交付确认 | 轨迹回放显示请求主动有界、持续写成果；新工具合同与旧检查点隔离。 | 精确查询、交付账本、有界历史、局部工作、自动成果引用、候选依赖、来源复核导航及停滞恢复已实现。最新248项库、严格Clippy及全局fmt通过，既有20项合同及隔离PG证据保留。边界证据合并版对照调用减少但未提速；原文出处与候选依赖错位已统一修复；同原文对照33→8次、239.30→63.55秒，两组verified。同批交接局部对照47→43次、344.42→298.90秒，两组均verified；仅代表该局部对照。冻结原文搜索错误全局依赖已删除。旧real-run-v15-resume2、review-v2及review-v3均已停止；v3在146轮无在途边界暂停，保留416条记录、8条关系及发现，原1200次上限已用759次；v4在225轮兼容接续后，于236轮三次HTTP500/503/503终止；累计998次，剩202次总额度，保留33项来源判断和27项发现，另有2处执行阻塞；跨页比较及收尾缺失比较派发已修复。跨范围成果、整体复核及性能门槛未关闭，见§5.5。 |
| Rig P1 | 完整附表、字段错误反馈、跨范围关系与已有独立复核 | 主流程能结束，reviewer 真正完成范围核查与修订反馈；无遗失条件/待办。 | 字段级错误、完整附表承载、精确关系、持久化发现与修订、原文任务及自动汇总已实现。签署错接、旧合同复合条件、整条要求漏提负例已完成独立检出和修复再复核；连续技术局部复核、条件误选和标签清空负例也已通过；复杂附表、跨范围关系及完整106页仍待验收，不能以局部通过关闭该阶段。最新证据见§5.5。 |
| Rig P2 | 当前协议真实提取与32项逐项复验 | 提取可供编制消费，32项关闭或有可审计更正；失败如实记录。 | 主提取已有416条记录、8条关系，旧版本146轮暂停，局部性能修复验证通过；v4兼容接续后已在236轮因三次HTTP失败终止，33项来源判断/27项发现保留，32项人工基线已整理但未关闭；32项及同版DOCX/PDF仍未通过。不以 Office 或 Embedding 改造为前置。 |
| Rig P3 | 既有 Journal 的多边界检查点、预算与恢复 | 独立数据库故障恢复通过，不绕过预约或重复提交。 | 当前提取/编制驱动已实现，五项隔离 PostgreSQL 恢复测试通过；修改现有 baseline，无新增 migration/表。见[验收记录](../../docs/bidding/agent-runtime-recovery-results.md)。 |
| Rig P4 | Rig 0.42.0 最小 Chat Completions 接缝、共享驱动切换与撤旧 | 预约字节等于发送字节、预约失败零发送、无嵌套重试、取消恢复及既有语义/编制回归通过。 | 最小 Chat 接缝七场景通过；共享宿主驱动已接提取/编制，两处重复外层循环已删除；生产 Chat 流解析已改用 Rig，18场景及五项隔离数据库回归通过。SDK 请求序列化已接入，142项库测试、18场景完整传输和五项数据库恢复通过；AgentRun 已接入；145项库测试（含提取/编制真实宿主会话复用断言）、20项合同、18场景传输及五项隔离数据库恢复通过，完整真实语义与样稿仍待验收。 |

本修订重排原 P0–P6，编号只属于此方案。先形成 P0→P1→P2 的提取证据，再推进独立 P3 与 P4 集成；提取侧不等 SDK 验证。用户后续要求先推进完整 Agent 功能，因此在真实验收未通过时已独立完成 P3 和 P4 最小接缝；这不改变 P0/P1/P2 的验收要求，也不得阻塞提取修复。共享驱动替换须接上既有编制消费者及其回归，不重写编制算法、DOCX 编译器或编辑器。

完整模板内容验收归 O1-S，同稿出件归 O2，继续记录在[总台账](../implementation-tasks.md)；Embedding 一致性归[知识库计划](../knowledge-base/README.md#模型与索引一致性独立事项)。P4 清理确认无人读取的旧提取配置、退役循环和旧结构生成分支；知识库仍使用的公共能力保留。单一 Chat Completions 协议按共享接缝验工具/图片/usage/结束状态，各角色分别验工具权限与业务结果，不构造双协议矩阵。

### 5.2 必测范围

- 配置：别名冲突、缺项、进程有效配置、冻结漂移与已配置参数实际生效。
- 协议：Chat Completions 的工具往返、图片、用量缺失、终止状态和预约/实际正文一致。
- 上下文：中文计数、完整协议组、超大结果、多工具总预算、未交付结果、分段恢复和角色隔离。
- 恢复：预约后、响应保存后、工具提交后崩溃，数据库失败、执行权失效、503和异常 EOF。
- 语义：重复引文、跨页条款、条件替代、证明时点、相似附件编号、局部待填单元格及精确关系。
- 稿件：目录依据、完整格式、固定文字、待填区域、来源到实际位置及同版本导出。
- 工程：`cargo fmt --all -- --check`、严格 Clippy、相关单元/数据库合同、既有解析/编制回归。跳过、缺依赖或清理失败不记通过。

### 5.3 提取验收与后续整稿交接

主样稿继续使用 `testdata/bid/BiddingFile.pdf` 对应冻结输入：106 页、148 个来源单元、42 个网格。[独立验收索引](../../artifacts/bid-full-sample/acceptance-index.json)仍有32项开放问题。

本次提取验收必须完成：主 Agent 与独立复核的所需覆盖；32项在提取层面的断言逐项通过或有可核查的审计更正；要求/适用模板/关系完整；分析与冻结来源绑定且可由既有编制入口消费。涉及实际DOCX位置、固定单元格或PDF分页的断言，须在对应产物生成后验证，不能在出件前标为关闭。[本轮逐项记录](../../artifacts/bid-full-sample/real-run-v16-review-resume1/independent-acceptance.json)与[分阶段交接](../../artifacts/bid-full-sample/real-run-v16-review-resume1/composition-handoff.json)均置于模型来源目录之外。

2026-09-12 当前候选的[本地附表核查](../../artifacts/bid-full-sample/real-run-v16-review-resume1/pre-review-template-audit.json)已核对R01/R02/R16/R17：8G/8H命名及p101归属、8A固定提示已有正确候选依据，不能重复套用旧缺陷；但8G日期整段留白会删除固定日期标签，8D-2/3/4标题与单位仍合并为不可交错的单个文本region。保持独立模型复核与修订，不把人工答案发送给Agent，不提前关闭任何案例；最终仍以真实DOCX/PDF为准。

2026-09-12 [R28实际复核抽查](../../artifacts/bid-full-sample/real-run-v16-review-resume1/reference-scope-audit.json)：第53页已保存的独立判断以“本页无具体章节编号”保留source_limited、未列目标候选或finding；但同一冻结集合第54–63页含专用技术、交付及测试内容。该理由不足以证明全集缺少目标。不得据此关闭R28，也不反向断言七处都必有唯一目标；须逐处核对可用目标与真实剩余缺口。此人工审计不发送给模型，当前全量复核继续。

交接 O1-S 后再验真实 Agent 生成的整本模板，核对目录、章节、附表、固定文字和签署位置，确保未编造投标事实；O2 验 DOCX/PDF/独立报告同一最终版本。它们仍是用户最终样稿目标的必要步骤，但不是本次 SDK 改造任务，也不因提取通过自动完成。

人工拼装稿、合成协议样例或局部表格测试不能替代真实 Agent 整稿。单一设备采购样稿通过也不等于已验证全部网络安全招标类型；软件、服务和综合类另以具备真实样本的验收记录说明覆盖边界。

### 5.4 性能指标和结论

2026-09-11 UTC 离线性能增量：`read_review_task` 在完整当前原文、候选及边界证据装入后，如剩余预算足够，附带本任务完整历史判断并明确要求重新核查、使用当前版本提交；旧结论不授予证据或审批，装不下则整体不附带，不裁剪旧判断或挤掉当前证据。真实第180轮历史判断4413字节，加入原工具包后19716字节，低于原48000字节预算。固定任务清单另以输入完整摘要和工具预算校验后在内存复用，候选/发现版本与完成状态仍实时计算；缓存不序列化，恢复后重建，不新增数据库或检查点合同。相同第236轮检查点三次debug离线样本，请求构造中位555.620→294.505毫秒，导航343.537→191.271毫秒，任务证据190.043→43.100毫秒；各计时包含重叠内部工作，不能相加。请求、任务、待办、导航和证据摘要均一致。248项库、严格Clippy和全局fmt通过。没有发起新模型请求，不证明模型收尾行为改善，原失败边界与32项/全稿验收仍开放。证据：[历史判断投影](../../artifacts/bid-full-sample/loop-repair/review-resubmission-context/prior-judgment-projection.json)、[本地前后对照](../../artifacts/bid-full-sample/loop-repair/review-resubmission-context/local-cost-comparison.json)。

2026-09-11 12:45 UTC 最新终态：全文`real-run-v15-review-v4-resume2`已在第236轮连续HTTP500/503/503后终止，三次各约0.5秒且未收到SSE事件；该预约正文111874字节，不能仅据状态码归因于正文大小或模型。该边界三次额度耗尽，未额外重试或重置；全链累计998次，原1200总上限剩202次。保留33项来源判断、27项发现、416条记录/8条关系，0轮完整复核，另外2处本地执行阻塞仍须排查。当前没有真实模型进程运行，不能把此前启动证明当成活跃状态。供应商终止与本地复核停滞分开处理；32项语义及完整DOCX/PDF/报告尚未通过。证据：[终态与冻结正文核验](../../artifacts/bid-full-sample/real-run-v15-review-v4-resume2/provider-terminal-observation.json)。

2026-09-11 12:38 UTC 跨页比较与收尾派发修复：真实第126轮已明确选择跨页候选并独立收到原表，complete_review_check仍按当前页面范围拒绝其引用；现只按任务/焦点限制候选，并按独立阅读回执验证原文引用。显式越界读取、未读原文和未选择的邻页候选仍拒绝。费用正文旧对照在所有7个来源已有判断后出现remaining=0、current=null，但仍缺当前候选比较，宿主不能汇总；新增尾部派发，在来源判断均有效时把缺失比较对应的任务重新列出，补验后由既有宿主汇总，不自动补签或增加收尾模型调用。245项库、严格Clippy、全局fmt及编译通过；实际终态检查点离线回放给出具体待补候选，原判断、原文、候选值和预算均不改。证据：[修复验证](../../artifacts/bid-full-sample/loop-repair/review-comparison-provenance/verification.json)。

费用正文两组均未完成独立复核：baseline80次/1162.16秒/50次工具错误；feedback75次/1098.40秒/25次工具错误，第72轮三次无工具输出后终止，第一流finish_reason=length。最后请求仍tool_choice=required，却没有活动来源任务；不能据此断言供应商每次输出的原因，也不能把未完成、不同发现的两组称为等质量提速。两组都不包含后来依赖、跨页比较及收尾派发修复，终态不重置。全文现由`real-run-v15-review-v4-resume2`在第225轮兼容接续，保留416条记录、8条关系、31项来源判断及22项发现；启动时总额度已用984次、剩216次，后续仍按759+v4累计calls计算。首个续接请求与旧runtime/env/预算一致；32项与真实DOCX/PDF/报告仍未完成。证据：[对照终态](../../artifacts/bid-full-sample/loop-repair/review-finding-feedback/final-comparison.json)、[续接核验](../../artifacts/bid-full-sample/real-run-v15-review-v4-resume2/startup-verification.json)。

2026-09-11 UTC 复核重复失效修复：真实费用表标题/计算示例被归为Rule，旧candidate_version把任何Rule的问题修订号纳入所有候选比较。对同一第105轮检查点的4项问题分别模拟撤回，每次均改变572个候选版本；候选正文未变。现在问题修订只影响实际候选依赖，四次回放各影响3个版本；真正的规则正文新增、修改、删除仍全局失效，关联问题撤回也不能恢复旧的无问题比较。新增问题状态导航及拒绝反馈，直接给出候选已有问题ID，保持拒绝“有问题却提交无问题”。这只证明依赖修复，不是全量提速或语义通过。243项库测试、20项合同、严格Clippy及编译通过；另仅修正3个共享文件的4处rustfmt排版差异，工作区cargo fmt --all -- --check现已通过。证据：[修复验证](../../artifacts/bid-full-sample/loop-repair/rule-finding-dependencies/verification.json)、[版本回放](../../artifacts/bid-full-sample/loop-repair/rule-finding-dependencies/comparison.json)。

全文v4已在第125轮、无在途Journal边界暂停归档，并以完全相同运行合同兼容接续至`real-run-v15-review-v4-resume1`。检查点与调用文件逐字节保留，416条记录、8条关系、28项来源复核及11项发现未重置；原1200次上限已用884次，剩316次（后续计数仍为759+v4累计calls）。首个续接请求已验证新导航、旧provider/prompt/tools/预算身份和保留成果；不是新合同从头复核。独立人工基线已逐项整理全部32项，全部仍待最终运行复验，人工答案不进模型。完整DOCX/PDF/报告尚未生成。另有原始费用正文7来源/2网格/21原候选的反馈旧新诊断在独立80次上限内执行，尚无终态结论。证据：[续接核验](../../artifacts/bid-full-sample/real-run-v15-review-v4-resume1/startup-verification.json)、[32项基线](../../artifacts/bid-full-sample/real-run-v15-review-v4/primary-template-audit.json)。

2026-09-11 UTC 可用性复查：review-v3前67个完成调用的供应商等待合计616.79秒，平均9.21秒；同期211次工具操作合计164.90秒，检查点保存仅2.86秒。这是已有解析结果之后的独立复核，仍未完成第一轮，当前性能不可用。后续观察与统计口径见[运行快照](../../artifacts/bid-full-sample/real-run-v15-review-v3/performance-usability-observation.json)。实际轨迹仍反复因未读取相邻证据而拒绝边界判断，证明增加邻接ID和同批交接尚不足以消除往返。当前原文、候选及有界边界证据合并交付已实现，语义判断仍由模型完成。真实对照进一步发现原文引文被误扩成相邻页全部候选比较；统一分离出处与候选依赖后，同七页目录33→8次、239.30→63.55秒、工具错误10→0，两组verified且原候选不变。每次导航重复重建冻结任务清单也已消除，三个离线样本的导航中位数440.23→265.60毫秒，任务/待办/导航/证据摘要一致。后者不是release或全流程吞吐，两项优化均不能替代正在执行的v4全文及32项验收。详见 `loop-repair/review-boundary-evidence/provenance-final.json` 和 `local-cost-replay/comparison.json`。

逐调用记录请求准备、预约、响应头、首正文块、首内容事件、整轮完成、工具和检查点耗时。首正文块不称首 token。

记录当前请求字节数、估算与实际输入/输出量、缓存/推理用量、调用/重试/重复读取/错误次数、当前范围/成果/关系/复核状态。缓存及推理量按子集统计，不与输入/输出重复相加；供应商未报告指标标为未知。

先用已有轨迹回放比较重复上下文和工程开销，再执行真实全链。模型、协议或供应商变化须分开报告，不把旧 v5 与新完整流程直接计算成提速倍数。

最终分别报告运行可靠性、语义完整性、整本样稿和性能瓶颈。**换用 Rig、请求变小或读完原文，不能单独代表任务完成。**

2026-09-10 新供应商证据：`negative-missing-relation-trial` 第7轮第一次尝试在180秒内仅收到86个推理事件，工具参数/完整工具/结束事件均为0，最后推理在176711ms。该次是模型未完成工具输出，不是完整结果后的本地EOF等待。另在技术组多个成功回合中，原预约请求 `max_tokens=8192`，供应商报告输出为9510–12818等超限值；这里只记录报告差异，不声称已独立精确计量token或证明供应商所有模型均忽略参数。[原请求摘要、事件计数及调用耗时](../../artifacts/bid-full-sample/loop-repair/provider-output-budget/verification.json)已归档。不得据此临时调大预算、覆盖模型或复活耗尽尝试的旧运行；可配置的 reasoning_effort 若调整必须写入 `.env`，新身份对照，且实际支持与效果另验。

逐模板映射判断（2026-09-10）：[唯一映射负例](../../artifacts/bid-full-sample/loop-repair/negative-query-template-link-trial/final-observation.json)和[清单修复后的配对负例](../../artifacts/bid-full-sample/loop-repair/negative-query-template-link-roster-trial/final-observation.json)均被错误批准。实际请求已包含缺失映射清单，最终判断只说明模板和要求分别存在，未解释二者关系；这是语义误通过，不能归因于提示未送达或拿工程检查替代验收。

本次针对提交合同修复：`put_source_review.template_mappings` 对当前适用、条件适用或未知适用模板逐项给出 `template_id / requirement_ids / relation_ids / finding_ids / reason / sources`。模型根据原文明确哪些现有要求使用该模板，截图附件和仅含说明的模板同样可能承接输出要求；宿主核对引用存在、当前版本已独立取回、关系类型和两端正确、父模板路径，以及无映射时已保存原文支持的发现。无需单独关联的模板允许空要求列表和原文理由，不强制生成固定数量关系，不用标题、行业词或样稿答案推断。`needs_evidence` 保留未完成状态，不能借此批准。

[工程证据](../../artifacts/bid-full-sample/loop-repair/template-mapping-judgment/verification.json)：197项库、20项合同、7项隔离PG恢复、严格Clippy、fmt与编译通过。改动复用现有源任务判断和Journal，未增加表、migration、队列或配置。工具与提示词摘要改变，旧运行不能改Config恢复；`negative-query-template-link-judgment-trial` 与 `appendix-query-link-judgment-control` 使用新身份、同原文/候选种子/预算/供应商做配对。必须验证负例检出并拒绝原缺陷、修复后复核完成、正例无误报；仅填写显式判断仍可能语义错误，实测完成前不得标为解决。此切片不关闭复合技术条件、签署错接、单元格策略或完整样稿验收。 实跑补充：[负例首轮拒绝](../../artifacts/bid-full-sample/loop-repair/negative-query-template-link-judgment-trial/first-rejection.json)准确指出无可填表格仍须映射截图附件；主Agent已补边，[负例终态](../../artifacts/bid-full-sample/loop-repair/negative-query-template-link-judgment-trial/final-observation.json)在21轮/约302秒完成第二轮独立复核，无最终finding，生产结构审计通过。[精确端点比较失败记录](../../artifacts/bid-full-sample/loop-repair/negative-query-template-link-judgment-trial/exact-edge-comparison.json)保留：修复使用整条requirement而非原response[0]端点；本例单项响应及两类证明均为同一附件的截图，整条映射有原文依据，不据此允许其他复合要求粗化端点。[正例](../../artifacts/bid-full-sample/loop-repair/appendix-query-link-judgment-control/final-observation.json)4轮/约111秒完成，无finding，生产离线结构审计通过。

候选类别与依赖焦点补充（2026-09-10）：旧签署/复合负例在目标比较前大量重复读取，真实请求用 `kind=all` 查询关系或处置被拒；[混合查询回归](../../artifacts/bid-full-sample/loop-repair/mixed-inspection/verification.json)已覆盖混合ID、分页、仅交付详情回执、未知/歧义ID原子拒绝及typed查询依赖。新合同实跑又复现：source-review待办包含跨来源关系端点，工作焦点却仅按原文scope计算允许候选，拒绝了整份扩展计划。[焦点回归](../../artifacts/bid-full-sample/loop-repair/dependency-focus/verification.json)现复用同一宿主依赖集合；允许规划已指派端点的比较，仍拒绝无关候选，原文访问、独立证据及完成门槛不变。

两项均无SQL、migration、表或配置变更。198项库、20项合同、严格Clippy/fmt/编译通过；混合查询合同另经7项隔离PG验证并清理容器。混合查询改变工具摘要，首次用新身份；焦点宿主修复未再改冻结合同，因此`negative-signature-dependency-focus-resume1`与`negative-compound-dependency-focus-resume1`保留原第6轮检查点、预约正文和累计调用兼容续跑，不将重启当预算续期。`source-scope-current-contract-trial`从空候选重跑第11/16/17页主提取及复核。三组实测尚未完成，旧失败和32项验收状态不清零。[签署错接发现](../../artifacts/bid-full-sample/loop-repair/negative-signature-dependency-focus-resume1/finding-observation.json)已在第12轮确认目标边错误连接8H，并给出改回8G的字段级修正，尚未完成整轮拒绝与修复复核。复合子项未检出；新的空候选组已保存8条记录，尚未交接复核。

焦点候选工作区召回（2026-09-10）：[第19轮检查点与真实查询离线回放](../../artifacts/bid-full-sample/loop-repair/candidate-recall/verification.json)发现，17种查询有16种在缩为单项后仍被拒；丢失的是旧工具批次中的5份焦点候选，不是原文或原图。候选总量本身能放入上下文，但协议历史接近冻结64KB上限，继续查询使包含独有候选的整组被淘汰。宿主现从当前角色的既有候选回执恢复完全相同的当前版本，放入有界工作消息；它不依赖保留旧工具协议组，仍受原总字节/token预算约束。未读、尚待交付、他角色、旧版本或已删除内容不可召回；该阶段只召回活动焦点；下述“来源任务候选召回与原文交付优先”补充已扩展为复核当前来源任务的指派候选。已在正文中出现或已完成工作的内容不重复带入。召回不更新回执、阅读量、执行进展或语义结果。

198项库（13项忽略，新增离线回放另行运行）、20项合同、7项隔离PG、严格Clippy、fmt和编译通过。17种查询最终发送正文均保留原文及焦点证据，399607–405222字节；这是接缝和证据共存验收，不替代模型语义。无工具/提示词/配置/SQL变更，`negative-signature-candidate-recall-resume2`保留第30轮，`source-scope-candidate-recall-resume1`保留第23轮、原预约及全部调用继续实测。复合子项组第32轮执行/交接额度耗尽，[终态](../../artifacts/bid-full-sample/loop-repair/negative-compound-dependency-focus-resume1/final-observation.json)保留了模型提出但未提交的遗漏发现，不能当作通过或清零恢复。

映射证据范围误拒修复（2026-09-10）：签署组第37轮的 `put_source_review` 已引用保存的错误端点发现，发现使用签字句 `721ac200:70–116`，映射判断使用包含该句的 `70–137`。宿主用整个 `Span` 完全相等判断“共有来源”，因此拒绝有效关联；模型随后为同一错误另存“缺失映射”发现，导致已完成来源判断按依赖失效。现仅允许同一来源、非空文本范围相互包含；相邻、仅部分重叠或不同来源均不关联，网格/原图仍要求相同引用。模型必须显式关联已保存 finding，真实阅读、当前候选版本、缺失关系不得批准等门槛保留，不自动合并发现或修改已存结果。

[可复现证据](../../artifacts/bid-full-sample/loop-repair/mapping-evidence-containment/verification.json)：原失败调用的映射校验离线回放通过，检查点未变。库测试199项通过、14项忽略，其中新增真实回放另行显式执行；20项合同、7项隔离PG恢复、严格Clippy、workspace fmt及样稿编译通过。首次回放相对路径调用因测试工作目录不同而失败，保留日志，改用绝对路径后通过。文本引用/工作计划的UTF-8错误同时补充由原文字节计算的相邻合法边界；仍由模型明确修正，不移动引文、不授予阅读回执。无工具/提示词/Config/SQL/migration变更。

该轮终态：`negative-signature-mapping-evidence-resume3` 在第66轮耗尽执行/交接额度，保存2项发现、0轮完整复核，已终止且不得清零恢复。正确错接发现已有记录，但不能用它或离线映射校验代替整轮拒绝、修复与再次独立复核。[空候选组终态](../../artifacts/bid-full-sample/loop-repair/source-scope-candidate-recall-resume1/final-observation.json)在第44轮耗尽执行/交接额度，主提取有25条记录、8条关系和3项来源处置，但独立复核没有完整结果；不得清零恢复。其后续重复读取已超出最初网格计划错误：阅读完成与候选可取回不代表模型愿意提交比较，需要继续依据最终发送正文核查焦点原文与候选是否同时可见、任务范围是否过大，以及全部来源被阻塞后是否仍浪费交接调用。当前未据此放松网格阅读门槛、增加额度或宣称模型语义问题已解决。


**来源任务候选召回与原文交付优先（2026-09-10）**

真实正文逐项核对显示：旧空候选组第30–37轮的网格与跨页原文交替丢失，同时缺1–7份候选；旧签署组第54–59轮缺11–28份候选。此证据证明可见性缺口，不能独自证明模型重复读取的内部原因。修复按 §3.5 保留当前工作证据，不扩大冻结预算、不重写提示词或工具合同。

[最终工程证据](../../artifacts/bid-full-sample/loop-repair/assigned-candidate-retention/verification.json)记录200项库测试（14项忽略）、20项合同、7项隔离PG、严格Clippy/fmt及编译。17种真实查询每种保留41份候选和所需原文，请求399737–399986字节，耗时445.36秒；该批回放早于最后“新原文优先”分支，候选查询分支未改，最终新分支有原文交付预算错误的红/绿及完整库/PG回归，不冒充再次跑过17查询。固定全部访问范围图片、递归固定候选引用图片的中间回放失败仍归档，最终实现仅保留当前显式视觉比较所需图片。

后续真实对照暴露原文重读边界：只按覆盖增量判断“新原文”会让已读、已退出窗口的原图重新取回时仍受可选缓存挤占。现按当前工具组成功的原文读取判断交付优先级，覆盖与计数不因此更新，候选/导航查询不能使用此退让。[原文重读验证](../../artifacts/bid-full-sample/loop-repair/original-reread-priority/verification.json)包含预算误拒红/绿、200项库测试（15项忽略）、20项合同、7项隔离PG及Clippy/fmt/编译；第18轮真实检查点逐张取回3张已读原图，请求399721/400971/450055字节，图像像素原样可见，检查点及阅读回执未改。此回放不是模型语义验收，也不替代上一批17查询的原验证边界。

当前对照：`source-scope-original-reread-resume1` 从第30轮、32次累计调用续跑，主提取36条记录/8条关系/3项处置后进入reviewer；`negative-signature-original-reread-resume1` 从第25轮、26次调用续跑，已有1项错接发现，尚无完整来源判断或复核轮次。前身 `*-assigned-retention-resume1` 为本次兼容修复主动暂停。原预约正文、检查点、runtime及累计调用逐字节核对保留，使用 `deploy/.env`，人工答案未发送。更早第44/66轮的耗尽运行不恢复、不清零。复合条件、勾选条件、混合单元格、连续技术页、106页全量及32项/整稿验收继续开放。

后续导航与查询诊断：当前焦点尚未比较时，`comparison_progress.next_reference` 原会指向焦点外的首项；现优先指向未完成焦点，焦点完成后再选其他待办。[导航回归](../../artifacts/bid-full-sample/loop-repair/focus-next-navigation/verification.json)包含红/绿、201项库测试（15项忽略）、Clippy/fmt/编译；它不产生阅读或语义判断，也不能单独解释反复读取。此版本尚未外发实跑。

第45轮检查点的两种精确查询仍在缩到单项后淘汰两段跨来源原文，[复现及待验证因果](../../artifacts/bid-full-sample/loop-repair/optional-cache-pressure/diagnosis.json)保持失败。先对同一请求只改变可选候选缓存，区分总预算与历史预算影响；若确认额外缓存造成竞争，则按实际剩余空间逐项退让，优先保护当前焦点、所需原文与最新工具结果，不把全体缓存永久关闭或放宽预算作为修复。保留既有17查询/41候选回归，不因新夹具更大而暗中改弱原验收。

上述两组续跑现均主动暂停于第48轮，进程已退出，原预约与累计调用保留。空候选组36条记录/8条关系/3项处置，0轮完整复核；签署组21条记录/17条关系/3项处置及1项错接发现，已有局部比较但进入执行阻塞，不能抹除阻塞或恢复额度来获得通过。继续完成上下文故障修复后再决定兼容续跑与新受控对照，全部语义和整稿门槛保持开放。

**可选缓存竞争的单变量定位与最小修复（2026-09-10）**

对同一第45轮检查点，仅禁用额外指派候选召回，两种精确查询立即保留原文并成功，但分别少了21/14份候选，证明缓存参与竞争；关闭整个缓存不是最终方案。现按 §3.5 的优先级逐项退让，在新压力夹具中保留39份候选中的34份、全部焦点及原文，5份非焦点候选暂时退出。压力回归单独验证必要证据和部分可选候选共存；原“全部39份共存”失败报告保留，不冒充通过或改弱旧17查询的41份共存断言。

初版退让发生在冗余历史/旧大图清理之前，使旧17查询虽都成功、必要证据无缺失，却不再保留全部41份候选，失败记录见 [过早裁剪](../../artifacts/bid-full-sample/loop-repair/optional-cache-pressure/premature-trim-17-query-report.json)。已修正顺序：先释放冗余完整工具组及过大的旧图消息，再缩减可选缓存；清理阶段禁止丢弃独有证据。最终17种旧查询均恢复41份候选及所需证据共存，请求399737–399986字节，离线耗时414.13秒；201项库测试（16项忽略）、两种压力查询、3张原图重读、严格Clippy/fmt/编译、20项合同及7项隔离PG均通过。[最终验证](../../artifacts/bid-full-sample/loop-repair/optional-cache-pressure/verification.json)分别记录缓存压力门槛、旧完整缓存门槛及中间失败，不据此宣称模型性能或语义已通过。

回归通过后已启动：`source-scope-adaptive-recall-resume1` 逐字节保留第48轮检查点、51次累计调用及原预约；`negative-signature-adaptive-recall-trial` 使用原模型候选及同一单项错接作为新受控对照，不导入旧复核回执、发现或计数。其前身已有执行阻塞，原失败不恢复、不清零，也不把新对照称为续跑。两组均继续使用 `deploy/.env`，真实比较、来源判断及拒绝/修复/再复核闭环尚待验收。

### 5.5 本次修复的实施门槛

原文搜索错误扩大依赖（2026-09-11 UTC）：真实v2轨迹中，第5页目录先在13–21轮完成，又在其他来源新增发现后于46–52、87–93轮被重新派发；这些阶段主分析仍为416条记录/8条关系，该任务却因原文搜索带有global=true。`search_sources`实际仅遍历FrozenInput的正文和网格，不读取Analysis或review_draft，因此无关候选、发现变化不会改变查询结果。红绿回归同时覆盖有命中、无命中和相关候选确实修改的情况；另保留真正全局候选查询的失效测试，纠正旧测试将原文搜索当作候选查询的错误假设。

修复仅删除该错误全局标记分支，不改写已有global标记或回执版本，不减少已登记的实际候选、关系、规则或原文依赖。239项库（25项默认忽略）、严格Clippy、样稿编译通过。v2已在140轮、无pending边界主动暂停，终态exit1来自取消，非供应商失败；473＋140＝613次累计物理调用及全部原检查点/请求保留。v3使用未改的416条主记录/8条关系建立新独立复核身份，不复用旧阅读、比较或原文判断；剩余物理/轮次587，工具10095，读取397436722字节，复核5轮，原上下文、输出和.env保持。新首请求的供应商及提示词/工具合同、相邻导航、剩余额度和原成果已核对。新旧独立复核初始化不能被描述为兼容检查点恢复。最终32项和整稿验收仍开放。

相邻片段导航与交接对照终态（2026-09-11 UTC）：同响应交接的旧/新对照已完成，47→43次调用、344.42→298.90秒，原目录处置逐值不变且两组独立复核均完成；新组6次明确从已完成任务读取到不同的下一任务，实际生效。但失败工具由11增至13，跨页边界仅引用当前页、补读前未扩展范围及重复取候选仍产生往返，局部改善不足以达到全文可用性能。对照包含同时运行的全文任务和模型采样差异，不能外推吞吐或全体32项语义。

后续已在 `source_review.current.neighbors` 派生前后任务的来源ID及精确文本/网格范围。导航使用完整冻结任务清单，不因前一任务完成而遗漏；限制在同一原文档，不把另一文档当自然续页。它既不证明语义续接，也不读取原文或修改工作范围，跨文档引用仍需调查。缺少前后任务仅表示冻结文档内无相邻任务，不代表外部证据不存在。模型据需要读取并逐项作出原有判断，所有来源、候选、布局、关系及版本门槛保持。238项库和严格Clippy通过；该修改未包含在上述两组归档程序或当前全文程序中，真实效果待验，不能将其计入13.22%的观察值。

任务交接性能修复（2026-09-11 UTC）：真实全文复核前23个完成回合记录模型等待253.75秒、本地工具21.30秒、请求构造10.48秒、保存1.35秒；这些是分事件计时，不是完整墙钟分摊。旧执行器只在整批工具结束后选择下一来源，模型即使将判断和下一次合并读取放在同一响应，读取仍返回旧片段。回归已实际复现该问题。

现在只有 `put_source_review` 成功且结果通过上下文准入、下一工具恰为 `read_review_task` 时，执行器提前复用 `select_next`。读写顺序和原批次提交不变：判断失败或 `needs_evidence` 保持未完成任务；新来源和候选仍暂存于 `pending_coverage`，同批后续判断不得使用；最终聚合仍在整批结束，跨源、关系、图像和遗漏校验不省略。提示词要求多个已读候选比较与原文判断合批，且存在后续任务才附加读取。未新增工具、参数、表、migration或配置。提示词/工具描述摘要已变，更换合同不得写回现有全文检查点。

验收分两层：现有回归证明同一模型响应可以完成旧片段并交付下一片段，且拒绝新证据即时使用、过期判断和未补证据跳过；真实对照采用物理第2–8页完整目录、7条原模型处置及同一 `.env`，每组诊断上限收紧为80次、其余上下文/输出预算不变，顺序执行旧版和新版。只有同范围、同质量终态才比较速度；目录只衡量任务交接成本，不能代替复杂附表、技术要求、32项语义和106页完整 DOCX/PDF/报告验收。详见 `artifacts/bid-full-sample/loop-repair/review-task-pipeline/`。

同页拆分模板与关系语义（2026-09-11 UTC）：原图门槛按同一文档同一页的适用模板共同包含文字/网格来判断，不能通过把标题、单位、说明和表格拆成不同记录而绕过。其他文档、其他页和不适用模板不触发该条件；旧无原图回执会重新开放，但宿主不据几何自动判定具体排版正确。真实主提取第84、95、96页及红绿回归见 `split-template-layout/`。复核导航从实际 `put_relation.kind` schema读取合法枚举及其说明，保持主写入和复核同一含义；同页/同表中的控制条件不因没有“详见”字样而自动免于关系判断。源条款依赖使用现有references语义；responds_to保留响应位置对要求的对应含义。没有新增关系枚举、表或migration；schema摘要已变化，复验使用新身份。

旧全文验收已在 `real-run-v15-resume2` 第473轮无pending边界暂停，退出码1来自主动取消，不是供应商终态。`real-run-v15-review-v2` 已以独立复核新身份启动，416条主记录、8条关系及原冻结输入逐值保留；不导入旧独立阅读或比较结论。其物理调用额度为1200−473=727，轮次、工具与读取额度同样扣除旧链消耗，保留原上限及旧全部请求/调用文件；这不是清零续跑或提高预算。局部关系语义复验已完成，主角色仅新增正确references关系并经再次独立确认；全文启动核验见其startup-verification.json，完整语义验收和出件门槛不降低。

复核合并读取（2026-09-11 UTC）：`read_review_task` 没有业务参数，读取宿主已派发的任务；正文/网格仍由既有 `read_source`/`read_form` 取得，候选沿用当前领域对象及摘要。单次总结果不超过既有 `max_tool_result_bytes`，原文使用任务既有的半额分片；超大的候选完整省略，仍留在 `pending_candidate_refs`，可由模型精确查询。优先给当前焦点中属于该任务的未比较候选，已比较候选不重复放入工作包；这是取数合并，不是批准候选或跳过关系、附表、边界和独立原图检查。

该工具与其他读取工具共用同批未交付保护、逐工具结果容纳检查、失败回滚及 Journal。只有包含读取结果的下一次有效模型响应才确认阅读；同批提前比较仍被拒绝。窗口检查识别实际工作包中的原文和完整候选，沿用当前版本校验及历史淘汰。主角色不开放此工具。未增加环境变量、表或 migration；工具和复核提示词摘要变化，必须新建运行，不套回旧全文检查点。测试及真实对照证据在 `artifacts/bid-full-sample/loop-repair/assigned-review-evidence/`；旧工具与新工具使用相同来源和原模型生成候选，人工验收答案不进入输入。对照终态前不承诺提速或关闭语义验收。

简短证据引用修复（2026-09-11 UTC，性能对照待完成）：[真实输出成本](../../artifacts/bid-full-sample/real-run-v15-resume2/reference-output-overhead-diagnosis.json)确认慢轮次包含大量重复来源/表格ID与坐标。现在工具引用字段接受`{"ref":"t:<来源数组位置>:<start>:<end>"}`或`{"ref":"g:<表格数组位置>:<row>:<column>"}`，位置只属于当前冻结输入，沿用既有输入摘要/运行合同约束，不能跨输入复用。模型复制`read_source.citation_ref`、`line_spans[].citation_ref`或`read_form.citation_refs[]`，网格覆盖位置仍为null；普通正文字符串不会解析成引用。原完整Span和视觉引用仍可使用。服务在工具边界展开为原有完整Span，随后仍验证几何、UTF-8、单元格锚点、各角色阅读回执和来源范围；规划引用不授予读取，同批新读仍不能立即用于写入。

主提取/独立复核和编制/稿件复核共用展开逻辑；持久化成果、领域Span、数据库/迁移和文档编译语义未变。工具参数、源文本及模型消息中继续保留所需的语义字段、来源和对应关系，不通过删掉要求来省输出。文本分页已将新增引用注释计入原工具结果预算，整段及行引用的生成不会使已截好的页面越界或漏掉UTF-8片段。232项库、20项合同及严格Clippy通过；当前workspace格式检查仍报告其他并行修改文件的排版差异，不将其记录为通过。

[生产函数离线回放](../../artifacts/bid-full-sample/loop-repair/compact-evidence-references/archived-replay.json)覆盖第202/205/207/212/216轮，展开后的完整参数逐值等于原请求；参数字节分别从7484/10278/15533/9871/13215降至5324/7713/10808/5146/5925，约减少25%–55%。这不是tokenizer或延迟测量，也不能据此预告同等比例提速。提示词及工具schema已变化，必须用新合同做同来源、同.env、同预算、空候选的受控对照；不得把新引用格式套回旧检查点或清零旧计数。当前完整v15-resume2仍使用其归档程序及旧合同继续验收，完整32项与同版出件门槛不变。

正文/表格导航顺序修复（2026-09-11 UTC）：[红绿及整体检查](../../artifacts/bid-full-sample/loop-repair/interleaved-reading-gaps/verification.json)复现`reading_gaps`先收集全部正文、后收集全部网格，主范围完成提示又直接取首项。现只对派生缺口稳定排序：元数据在前，正文与网格遵循冻结来源集合顺序；保留部分读取区间和全部缺口，不写入阅读回执、来源处置或关系，不把来源顺序当作视觉版式证据。[真实第188轮离线回放](../../artifacts/bid-full-sample/loop-repair/interleaved-reading-gaps/archived-replay.json)使用生产函数，从第84页正文切回第11页未读网格，runtime/Journal仍有效且原检查点未变。230项库、20项合同、严格Clippy、fmt和编译通过；首次库测试仅受沙箱本机TCP限制，授权后同套通过；另仅格式化现有PostgreSQL检查点查询链，SQL与绑定值未改。

[兼容接入](../../artifacts/bid-full-sample/real-run-v15-resume2/preflight-verification.json)在第196轮已提交、无pending边界停止前进程，逐字复制全部原文、历史请求、runtime、run-contract、checkpoint和calls；保留241条记录、2条关系和196次累计调用。[启动核验](../../artifacts/bid-full-sample/real-run-v15-resume2/startup-verification.json)确认同一.env合同，第196轮新请求已含第11页网格提示；实际表格成果和全文复核仍待验。此前0关系是中间观察，后续已出现关系，不据旧计数判最终漏提。

[整段待填候选观察](../../artifacts/bid-full-sample/real-run-v15-resume1/paragraph-blank-policy-observation.json)：部分非网格模板区域将固定授权/承诺措辞与待填字段一起标为`bidder_blank`。当前编译器会对该区域输出空白，不会根据instruction自动保留其中固定措辞；此类候选若原样获批会丢失模板原文。该观察未写回候选、未发送模型，必须在真实独立复核/主修订/再复核中核查，不擅自改编译器复制待填样例，也不将局部表格标签负例通过扩大为所有段落策略通过。

后续整稿环境预检（2026-09-11 UTC）：历史验证的固定ONLYOFFICE镜像缓存缺失，现已恢复同一摘要并核对镜像ID；PostgreSQL/Redis镜像、当前DocReader回读两页合成PDF及Chromium空白页启动均通过。[出件交接](../../artifacts/bid-full-sample/output-readiness-v15/finalization-handoff.json)复用既有`--web --pdf --update-toc --finalize-only`流程，须待真实提取、独立复核、32项及编制产物验收后执行，不使用合成稿替代真实样稿。

早期导航观察（2026-09-11 UTC）：[相同前34完成轮次](../../artifacts/bid-full-sample/real-run-v15-resume1/early-navigation-comparison.json)中，纯导航/规划轮次由v14的23轮降为v15的4轮，该类供应商等待由193.952秒降为33.352秒；同期全部已完成调用等待由319.485秒升为592.539秒，新组发生31次成功写入操作。两组处理范围与内容不同，不是等量语义工作的受控提速实验，也不能将19轮差额全当可消除成本；全文总耗时及质量仍待验收。

请求退避实现（2026-09-11 UTC）：[红绿与整体验证](../../artifacts/bid-full-sample/loop-repair/provider-retry-wait/verification.json)证明旧循环会无间隔耗尽三次额度；现由共享Driver在原有三次额度内等待1秒/2秒，取消发生在等待期间不预约下一次请求，已预约两次的恢复仅剩一次机会，发送正文逐字不变，失败或部分响应不执行工具。229项库、20项合同、严格Clippy/fmt/编译通过，无配置/schema/prompt/SQL变更。v15已在第33轮提交边界安全暂停；[续接预检](../../artifacts/bid-full-sample/real-run-v15-resume1/preflight-verification.json)逐字保留29条记录、33次累计调用、全部旧请求/runtime/合同；新程序同合同续接正在运行，未取得真实故障恢复成功证据，不声称解决502/530。

新合同全文验收v15已启动（2026-09-11 UTC）：[工程验证](../../artifacts/bid-full-sample/loop-repair/post-completion-guidance/verification.json)记录227项库、20项合同、4项本机流传输测试及严格Clippy/fmt/编译通过。[启动验证](../../artifacts/bid-full-sample/real-run-v15/startup-verification.json)确认供应商与预算逐值等于v14，实际首请求为.env的grok-4.6＋Chat＋low，并含更新后的工具说明；106张原图逐一核对摘要，空候选启动，不导入旧耗尽检查点。[早期合批实证](../../artifacts/bid-full-sample/real-run-v15/initial-batching-observation.json)确认真实第3轮将2次写入、2次处置和显式完成合批，全部成功，下一请求显示后续范围提示；随后规划和读取合批。模型仍在该批查询索引，不能宣称重复查询全部消失，也不能由单次成功预告全文提速。[真实合批失败保护](../../artifacts/bid-full-sample/real-run-v15/batch-failure-guard-observation.json)进一步确认第32轮引用UTF-8边界错误后，同批完成被阻止，其他成功写入保留；[实际模型修复](../../artifacts/bid-full-sample/real-run-v15-resume1/batch-error-repair-observation.json)确认续接第33轮模型将无效UTF-8边界改到原文合法边界，写入和原范围完成均成功；没有人工补候选。该证据仅关闭本次字段错误反馈/修复过程，不是独立语义批准。当前完整提取/独立复核、32项和同版整稿均未完成。

诊断更正与新修复（2026-09-11 UTC）：[统计更正](../../artifacts/bid-full-sample/real-run-v14-resume1/stream-statistics-correction.json)核对原始日志，第97轮在3.25秒已出现`llm_first_sdk_event`；旧`send()`在错误和超时分支改用空统计，使终态显示零。现共享当前请求的内存统计，断流、坏帧、缺终态及超时均保留已观察事件；统计不授予工具执行权限，也不把部分响应作为成功。新增响应类型固定分类及HTTP正文/SSE/SDK失败阶段日志，不打印任意头字段、模型正文或凭据。旧日志和原终态归档保持原样，不能把该修复当作502/530成因已解决。

导航/合批实现已完成：completed主范围从既有全局缺口/deferred/执行阻塞构造有界提示，元数据缺口也指向既有collection_index；导航不修改检查点、证据回执或语义状态。活动请求已有缺口时不再强制额外check_gaps。前轮已交付证据的写入、处置和显式完成允许合批；新读仍须后续模型接收，同批任何前序工具失败（含预算回滚）均阻止完成，成功的其他工具保留。红灯已复现原写入失败仍能完成的问题，定向绿灯覆盖合法合批、未交付证据拒绝及失败不能被完成掩盖。工具说明同步修正未读grid规划规则，提示词/工具摘要变化必须新身份验收；尚未取得新合同全文提速或语义通过证据。

全文供应商终态（2026-09-11 UTC）：[v14续接第97轮](../../artifacts/bid-full-sample/real-run-v14-resume1/provider-terminal-observation.json)保留50条记录、1条关系、22项来源处置，0轮独立复核，100次累计调用。当前89799字节正文第一次HTTP200收到7153字节；原日志在3.25秒已有SDK事件，约3.62秒流失败，未形成可接受的完整工具响应。旧终态统计被错误归零，无法据其断言事件类别或数量，随后两次分别约0.46秒502、0.20秒530，同边界三次额度耗尽。不重置或续跑该边界。真实导航修复已生效不等于供应商问题解决；现有日志没有响应类型或错误正文，不能凭530断定具体CDN/DNS故障。[公开网关单次GET](../../artifacts/bid-full-sample/real-run-v14-resume1/public-gateway-probe.json)没有发送凭据、招标数据或completion请求，返回403/JSON、Cloudflare标识和错误1010；它只说明这条公开访问受到拒绝，不证明带认证Chat请求的530成因，也不追加原模型轮次或绕过访问限制。全文及整稿验收保持未完成。

全文同合同续接（2026-09-11 UTC）：原v14在第90轮已提交、无pending预约时通过SIGINT安全暂停，47条记录、90次调用、0轮独立复核，终态及原因分别见`real-run-v14/terminal.json`和`compatible-pause.json`，并非供应商错误终止。[续接预检](../../artifacts/bid-full-sample/real-run-v14-resume1/preflight-verification.json)逐字节核对原检查点、全部请求、调用计数、runtime、run-contract、冻结输入和预算，当前生产Config及Journal校验通过。[启动核验](../../artifacts/bid-full-sample/real-run-v14-resume1/startup-verification.json)确认相同run_id，从第90轮继续原计数，当前宿主修复程序已接入，原47条记录保留，未重置旧预算或外发人工答案。更换程序没有额外取消/消耗在途调用。[真实范围切换](../../artifacts/bid-full-sample/real-run-v14-resume1/navigation-transition-observation.json)确认第93轮查询的source_index及文档metadata，在第94轮打开范围、第95轮读取时逐字保留；模型打开后直接read_source，未在该响应里重复索引。此处仅证明一次实际切换修复生效，不能据不同范围计算因果提速或关闭全文最终语义。

重复交接清理已做最小修复（2026-09-11 UTC）：[验证](../../artifacts/bid-full-sample/loop-repair/completed-scope-navigation/verification.json)从真实导航丢失得到红灯，现仅修改handoff判断，旧范围已complete后打开新范围不再次清空刚取得的导航。主/复核两角色均验证：旧范围完成仍释放原上下文；新导航保留不增加阅读回执；未读完成、未声明来源读取、无deferred的未完成范围替换仍被拒绝；活动范围拆分仍清理旧上下文且保留deferred。[实际第14→15轮回放](../../artifacts/bid-full-sample/loop-repair/completed-scope-navigation/archived-replay.json)进入最终SDK请求时保留导航，60500字节，原分析/coverage与归档检查点不变、原预算满足。224项库、20项合同、严格Clippy/fmt/编译通过。无新增状态、缓存、schema/prompt、配置或migration。初次测试夹具的作用层级/路径/派生work反序列化错误与修正日志保留，不将它们当生产缺陷。v14原运行保留修复前归档程序；同合同续接第93→95轮已验证一次真实导航保留，但尚未证明整体提速。后续提示、缺口查询和合批修改现已实现，整体回归及新合同真实对照待完成。

全文导航往返的原因与修复（2026-09-11 UTC）：[真实请求诊断](../../artifacts/bid-full-sample/real-run-v14/navigation-cause-diagnosis.json)核对v14前34个完成轮次，23轮只有导航/规划/状态或候选查询，模型等待193.95秒，占同期319.49秒的一部分；这些操作含必要工作，不能把23/34当作可消除比例或预告提速倍数。

已证实的机制：`set_work_note`在完成旧范围以及从completed打开新范围时都清除历史。实际第14轮请求里刚取得的source_index、候选目录和文档metadata，在第15轮打开新范围后全部消失；此处没有预算溢出。旧范围完成但全集尚有缺口时，`work_state`又返回null，缺少下一步提示。与此同时，主提示词和工具说明强制完成前再查`check_gaps`，但活动请求已携带相同缺口，完成校验本身也复用该检查。第8轮已给出下一来源和准确未读区间，模型仍再次查索引，说明既有状态利用也不充分，不能全部归因于缺少字节位置。工具说明仍称grid规划须已交付，与当前已修复的“有效未读grid可规划”实现不一致。

修复顺序（此段是设计，不代表已实现）：

1. 首先消除重复交接清理：旧范围已经complete后的新范围打开，不再次无条件删除刚查询的导航；保留最新完整工具组，继续受原历史/token/字节预算限制。活动范围拆分、角色交接和跨范围读取约束保留。用第14→15轮轨迹回放及合成跨范围场景验证，不能通过保留整份历史解决。
2. 在现有`work_state`中提供有界后续工作提示：由已有未读范围、缺处置及deferred状态产生可选择来源引用或缺口续页；它是导航，不是已读、已提取或语义完成。不给下一页硬编码优先级，不新增Agent表、持久化清单或调度框架，模型仍可按原文跨引用选择其他范围。
3. 删除主提示词/工具说明中“每次完成前必须重新调用check_gaps”的重复要求：完整动态缺口已提供时直接据此行动，仅分页未显示完整、出现具体错误或需要精确追查时再调用。允许在已有已交付证据下，将put_record/关系、set_disposition和显式complete合批；可将已知下一范围的规划与读取合批。新读原文仍必须在随后成功模型响应中确认，不能同批读取后未经模型看到便写成果。继续由模型显式作出完成决定，不因结构缺口为0自动宣称语义完整。同步纠正grid规划说明。
4. 验证：实际导航跨新范围保留且不挤掉必要原文/候选；欠读、跨源未声明、未提交或过期候选仍拒绝完成；同批失败不吞掉未完成义务；累计物理调用/恢复计数不重置。再以同来源/模型/预算对比导航往返与主提取产出，并完成106页、32项和整稿验收。提示词/工具摘要一旦改变，使用新运行身份，不能改写v14合同或复活旧耗尽边界。v14及其续接均已终止，原检查点和三次耗尽记录保留。

此修复针对应用层往返和信息丢失。旧500/503和不完整SSE是独立供应商证据；有限退避或Retry-After只能作为另行验证的恢复时序改进，不能宣称解决成因，更不能追加旧同轮三次额度。共享驱动现已实现两次重试前分别1秒/2秒的可取消等待，等待在下一次预约之前，三次上限及正文不变；这只是请求间隔策略，不是供应商故障修复。Retry-After解析及跨进程持久化冷却尚未实现。

完整106页新验收（2026-09-11 UTC）：`real-run-v14`从空候选启动，使用当前已验证网格规划修复程序；[预检](../../artifacts/bid-full-sample/real-run-v14/preflight-verification.json)逐一核对106张统一DocReader原图及原PDF摘要、148个来源/42个网格，冻结输入与前一完整运行相同，提取/编制预算逐值等于.env配置。[启动核验](../../artifacts/bid-full-sample/real-run-v14/startup-verification.json)确认实际请求为grok-4.6＋Chat＋low、runtime等于已通过的局部对照；不导入旧候选、旧检查点或人工修复，不重置旧耗尽边界。[32项独立清单](../../artifacts/bid-full-sample/real-run-v14/independent-acceptance.json)位于source目录之外，仍全部pending，原人工报告和全局验收索引不改成通过。运行需取得完整提取/独立复核结果并经生产审计、32项实际语义核对后，才能交给既有compose入口生成完整同版DOCX/PDF/报告。

交接纠正：连续技术第55–63页已在`technical-continuous-reasoning-low-resume2`完成217轮/218次调用、2轮独立复核、50条记录、0finding，终态和结构审计见本节已有归档。它不是当前未完成的单独技术运行；旧技术失败终态继续保留，全文32项、跨范围关系和出件仍须由后续完整新合同验收关闭。

未勾选条件对照（2026-09-11 UTC）：[预检](../../artifacts/bid-full-sample/loop-repair/unchecked-condition-pair/preflight-verification.json)保留物理第11页完整解析表格、统一DocReader原图及11条真实模型记录；正例不改候选，负例只将联合体条款的选择结论由“不接受”颠倒为未勾选的“接受”。来源/原图/程序/.env/预算相同，生产结构预检通过；独立人工预期不在source目录。此局部范围没有内部关系，跨页记录及其关联在两组中等量排除并列于预检，原归档不变；不将该测试视为关系组退役或完整范围通过。[正例](../../artifacts/bid-full-sample/loop-repair/unchecked-condition-control/final-observation.json)4次调用/73.02秒、1轮完整复核，候选逐值不变、0finding、verified；[负例](../../artifacts/bid-full-sample/loop-repair/negative-unchecked-condition-trial/final-observation.json)20次调用/289.07秒，真实wrong_selection发现、首轮完整拒绝、主Agent修订和第二轮独立复核均完成，0最终finding、verified，生产结构审计通过。[配对验收](../../artifacts/bid-full-sample/loop-repair/unchecked-condition-pair/paired-acceptance.json)关闭该孤立负例。要求data逐值恢复到正对照，但目标记录新增真实行标题引用，因此完整记录逐值相等断言失败，差异与依据保留于实际修订证据，不冒充完全相同。复核先两次写错字段路径、后一次使用过期来源版本，均由宿主拒绝后模型自行改正，未放宽校验。

标签配对[分阶段耗时](../../artifacts/bid-full-sample/loop-repair/blank-label-pair/performance-observation.json)：正例30次调用、526.13秒；负例首轮复核30次、主修订8次、再复核25次，合计63次/1263.31秒，其中供应商响应1215.93秒、本地工具21.47秒。终态完成后未再请求模型。该结果证明局部修复闭环，但调用数和总体性能仍待改善；不同流程不能当提速对照，也不代表release吞吐或完整106页性能。

标签清空对照的真实进展（2026-09-11 UTC）：[正对照终态](../../artifacts/bid-full-sample/loop-repair/appendix-blank-label-control/final-observation.json)为30轮/30次请求自然结束，1次完整独立复核、0finding、候选逐值不变，[生产审计](../../artifacts/bid-full-sample/loop-repair/appendix-blank-label-control/final-structural-audit.json)无结构或复核缺口。quality仍为needs_review，源于获奖条款原有strength=unknown；此处只确认保留提示的局部对照未被误报，不将该强度未决当作完整来源已通过。[负例首轮拒绝](../../artifacts/bid-full-sample/loop-repair/negative-blank-label-trial/first-review-rejection.json)和[实际主修订](../../artifacts/bid-full-sample/loop-repair/negative-blank-label-trial/primary-repair-observation.json)已取得：一个含人数提示格恢复fixed_text，无虚构blank_ranges；整个模板与正对照逐值相同，两条关联重新绑定当前版本，未人工改候选。[负例终态](../../artifacts/bid-full-sample/loop-repair/negative-blank-label-trial/final-observation.json)为63轮/63次请求、1263.31秒自然结束，2轮完整独立复核、0最终finding，当前来源判断摘要匹配最终review；全部记录和关系逐值等于正对照，[配对验收](../../artifacts/bid-full-sample/loop-repair/blank-label-pair/paired-acceptance.json)及生产结构审计通过。关闭该孤立标签清空负例，quality仍为needs_review；不关闭整体R16/R17或完整文件验收。

标签清空负例与附表终态（2026-09-11 UTC）：附表[第69轮供应商终态](../../artifacts/bid-full-sample/loop-repair/appendix-layout-editable-cell-review/provider-terminal-observation.json)为500/503/503、72次物理请求、2项草稿finding，主修订保留但再复核未完成；不重置同轮三次尝试。按原计划执行尚未跑过的“清空含标签待填格”正负对照：[预检](../../artifacts/bid-full-sample/loop-repair/blank-label-pair/preflight-verification.json)使用评审表及续表4来源/2网格、实际模型7条记录/4条关系；负例只将一个含人数标签的单元格从fixed_text移到无blank_ranges的bidder_blank，并刷新受影响端点摘要，原文/其他候选/预算均相同。两组结构有效，但保留的旧拒绝review不能授权新输入，预检不伪造通过。初次夹具摘要因空可选字段未按typed序列化省略而失败，已在外发前修正并保留失败报告。[正对照启动](../../artifacts/bid-full-sample/loop-repair/appendix-blank-label-control/startup-verification.json)与[负例启动](../../artifacts/bid-full-sample/loop-repair/negative-blank-label-trial/startup-verification.json)核对runtime与前附表完全相同，使用已验证网格规划修复binary，不导入旧检查点/独立复核回执或人工答案。须检出标签丢失、完整拒绝、实际修复并完成再次复核；正例同样独立验证，不称已验收金标准。这是不同负例任务，不是重放已耗尽的第69轮，也不替代R16/R17及完整文件验收。[第9轮真实检出](../../artifacts/bid-full-sample/loop-repair/negative-blank-label-trial/initial-finding-observation.json)已准确指出整格留白删除人数/职称人数固定提示并引用该单元格。建议中“已在region0”的表述仍需纠正（负例已将该格移出），不能复制出重复策略或编造空白范围；当时尚未完成完整拒绝、实际恢复及再复核；后续闭环已完成，见上方配对终态。

网格工作规划修复（2026-09-11 UTC）：[真实两次拒绝](../../artifacts/bid-full-sample/loop-repair/grid-focus-planning/observed-rejections.json)显示set_work_note合法网格焦点被“先读单元格”挡住，第二处阻止跨来源展开并引发read_form范围拒绝。正文规划原已只校验坐标，网格却误用交付回执校验。现抽取并复用既有网格位置检查：规划验证来源/表格归属、真实anchor、边界、零文本偏移及无混合视图；实际put_record和put_review_finding仍验证本角色已读网格，规划不增加coverage。主/复核两角色及locate/extract/review动作、未交付写入/发现拒绝、非法/覆盖格和伪造视图均回归。[红绿、223项库/20项合同及Clippy/fmt/编译](../../artifacts/bid-full-sample/loop-repair/grid-focus-planning/verification.json)通过，没有schema、提示词、配置或migration变化；该附表组使用原归档binary，未为此重启或清零；其后第69轮供应商终态见本节开头。

附表[首轮完整拒绝](../../artifacts/bid-full-sample/loop-repair/appendix-layout-editable-cell-review/first-review-rejection.json)已完成并交给主角色，包含备注缺表格关联、财务附表交错顺序和8E-1表后说明顺序共3项。此前fixed_text锁编辑、提示格必须清空和备注未提取的误报没有进入这份完整review；[真实主修订](../../artifacts/bid-full-sample/loop-repair/appendix-layout-editable-cell-review/primary-repair-observation.json)已重排两份模板、新增两条备注到表格的关联，并更新三条原关系（含财务承诺所在位置的解释）；逐格角色/留白策略和逐字节文字覆盖及角色保持一致，未删记录。仍须独立再次复核，不能将局部差异核对当作最终R16/R17或DOCX验收。

附表编辑语义说明（2026-09-11 UTC）：此前真实复核把fixed_text推断为禁止后续填写，要求为只有提示文字的格子造blank_ranges；编制器实际生成普通可编辑单元格，ranges只删已有文字。现只补充inspect_analysis说明与主提示词，明确区域角色控制初始文字保留，不控制编辑权限，也不创建输入控件或额外输入空间；不清空提示、自动撤回finding或新增样稿分支。[131项提取回归、20项合同及格式/编译](../../artifacts/bid-full-sample/loop-repair/editable-cell-semantics/verification.json)通过。[附表新合同启动](../../artifacts/bid-full-sample/loop-repair/appendix-layout-editable-cell-review/startup-verification.json)保留11来源/7网格和真实13条记录/6关系的值，provider/limits不变；不导入旧145轮的复核回执或13项finding。该组同时使用此前已实现的来源范围、关系判断和错误反馈修复，因此是当前完整合同验证，不宣称只有一句提示词带来的性能对照。旧错误结论与未完成结果保留，真实纠错、主修订及再次复核仍待完成。[首项真实发现](../../artifacts/bid-full-sample/loop-repair/appendix-layout-editable-cell-review/initial-finding-observation.json)已识别同页备注记录确实存在，指出它尚未关联评审表及续表；本地核对该备注没有关联边，与旧“备注未提取”的误报不同。修订须保留“仅不适用或确无内容时填/”的条件，不能把所有空格预填/；目前仅证明发现与缺边核对，尚无实际主修订或完整复核。第28轮又以独立原页图像检出财务附表标题/三张表/末尾承诺的交错顺序错误，[当前候选与发现](../../artifacts/bid-full-sample/loop-repair/appendix-layout-editable-cell-review/r17-detection-observation.json)确认候选尚未修订，R17仍只到检出阶段；不得凭工程回归或无fixed_text误报就关闭。

过时未决项修复（2026-09-11 UTC）：`context::check_delete`保留未审查pending删除保护；主角色修订时，已完成独立复核的finding明确affected该记录才允许退役。删除不会清除保存的review/finding，也不能完成验收；原有关系和子项引用校验继续生效。[真实归档只读核对](../../artifacts/bid-full-sample/loop-repair/reviewed-unresolved-retirement/archived-delete-eligibility.json)确认过时续页项符合主修订删除条件，而缺少公告/第五章证据的另一未决项不符合；这不是工具执行或模型通过证据。主/复核提示词和delete_record说明明确Unresolved只代表真实剩余疑问，改写为“已解决”不能代替退役，不通过行业词典或样稿ID判断。新增红绿回归、222项库及20项合同、Clippy/fmt/样稿编译[验证通过](../../artifacts/bid-full-sample/loop-repair/reviewed-unresolved-retirement/verification.json)。[新合同独立复核](../../artifacts/bid-full-sample/loop-repair/source-scope-unresolved-retirement-review/startup-verification.json)只导入实际修订的23条记录/15条关系，不导入旧复核回执或人工答案，provider/limits不变；该新组[终态](../../artifacts/bid-full-sample/loop-repair/source-scope-unresolved-retirement-review/provider-terminal-observation.json)为第15轮、18次物理请求，HTTP 500/503/503，0完整复核且候选未变；原失败不清零。随后[真实归档工具回放](../../artifacts/bid-full-sample/loop-repair/reviewed-unresolved-retirement/archived-delete-replay.json)在内存中切换主修订角色并实际执行生产apply/delete_record：过时项及两角色pending引用移除、真实缺证项保留、review/draft/原检查点文件不变且done=false；不是模型修订或Journal恢复。仍须验证模型实际检出、主删除和完整再次复核。供应商[时序定位](../../artifacts/bid-full-sample/loop-repair/reviewed-unresolved-retirement/provider-timing-diagnosis.json)显示先44.37秒500、再0.711/0.659秒503；此前395912字节成功而393307字节失败，不支持固定字节帽解释，也不足以证明加退避能解决，暂不调整预算或追加尝试。

关系组已完成[首轮完整拒绝](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-scope-resume3/first-review-rejection.json)，5项发现交给主角色。[实际主修订](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-scope-resume3/primary-repair-observation.json)已产生15条references关系，资格选择和联合体适用性已写入候选，并修正正文的续页表述；第二轮独立复核在第58轮供应商HTTP 500/503/503后终止，累计64次物理请求，3次同轮尝试耗尽，不能继续重置；0项草稿finding不代表再次复核完成。[终态证据](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-scope-resume3/provider-terminal-observation.json)保留395082字节原预约。主角色删除过时Unresolved被pending_refs保护拒绝，记录仍以unresolved保存“已读已提取”的说明。

第5项[真实检出](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-scope-resume2/additional-relationship-detection.json)为正文“其他禁止情形”尚未对应前附表实际选定内容。它使旧来源结论失效；[版本回放](../../artifacts/bid-full-sample/loop-repair/relationship-judgment-reuse/archived-receipt-versions.json)推翻了直接复用旧8项判断的假设，因此未实施过期结论复用。当前修复仅让关系判断错误精确到finding_ids数组位置，报告主体ID、错用finding ID及一个实际已存的相关主体finding ID，仍须独立核对，不自动替换、生成或撤回发现。[31项来源回归及Clippy/fmt/编译](../../artifacts/bid-full-sample/loop-repair/relationship-finding-feedback/verification.json)通过；[第39轮同合同续接](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-scope-resume3/startup-verification.json)启动时保留42次模型请求、5项发现与原预约；后续修订与第58轮终态见上方记录。

跨页finding绑定的[第二处过严拒绝](../../artifacts/bid-full-sample/loop-repair/relationship-finding-evidence/diagnosis.json)已修复：发现已明确绑定续页候选的适用性字段，生效条件证据来自其他原页，不必强制两边引用相同文字。复用既有validate_finding检查当前字段和独立交付证据；有明确受影响主体即可对应，否则仍须共同原文；把finding实际来源加入依赖，不借用无关问题或删掉发现。[最终220项库、30项来源回归及Clippy/fmt/编译](../../artifacts/bid-full-sample/loop-repair/relationship-finding-evidence/verification.json)通过，前序20项合同保持通过。[第28轮同合同续接](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-scope-resume2/startup-verification.json)保留30次模型请求、101次工具调用、4项发现及原预约；目前尚未完成主修订与再次复核。

未决记录关联校验的[过严拒绝](../../artifacts/bid-full-sample/loop-repair/unresolved-review-evidence/diagnosis.json)已修复：既有Unresolved.affected允许为空，不能要求复核者先修改主候选才能引用它与主体共有的精确原文。现允许自指、明确affected ID或共同/包含的原文证据，仍要求当前候选独立取回，仍拒绝借用无关未决项。[29项来源回归、Clippy/fmt/编译](../../artifacts/bid-full-sample/loop-repair/unresolved-review-evidence/verification.json)通过；[同合同续跑](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-scope-resume1/startup-verification.json)保留第18轮、19次模型请求、84次工具调用及4项发现，runtime、run-contract和原预约正文逐字节一致。主修订及再次复核尚待验收。

复核范围指令的[真实检出观察](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-scope-review/relationship-detection-observation.json)：Agent实际检索并取回前附表/续页对应候选后，保存了总则事实引用缺失、资格后审选择未落实、不接受联合体未落实到续页条件、已读已提取续页仍称未解决的4类发现。不是人工finding注入，仍待主修订和独立再复核。部分建议用了非现有关系枚举名称，主角色应遵守实际工具schema，不因此新增关系类型。最新工程验证为218库、20合同及严格Clippy/fmt/编译；图片批次测试改为按实际正文计算部分接纳边界，生产预算不变。

显式字段后的[真实语义失败](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-contract-review/semantic-rejection-observation.json)：16轮/245.06秒、1轮复核、0finding，22条候选逐值未变。模型填写了relationship_checks，但以“本页只引用前附表/另一页”为由签not_required，已读续页仍签source_limited；字段必填没有修正语义。请求回放确认通用条款窗口只保留正文与续页，先前已读的选择表不在窗口，Agent没有重新检索目标。现将读取窗口与条款作用范围的区别、实际引用目标检索及来源选定条件核对置于复核提示词开头；不添加词典、人工答案或全量原文重发。[新范围对照](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-scope-review/startup-verification.json)只有review_prompt_sha256变化，候选、模型、工具和预算相同。该假设仍待真实检出/修订/再复核，不能宣布关系问题解决。

附表[诊断终态](../../artifacts/bid-full-sample/loop-repair/appendix-layout-original-view-contract-resume1/segment-terminal-observation.json)为145轮、13finding、0完整复核，保留原预约与计数。第110轮后有20次来源提交漏掉必需集合中的同一项；[诊断](../../artifacts/bid-full-sample/loop-repair/finding-id-feedback/diagnosis.json)表明旧反馈只给完整清单，让模型比较两串ID。现在输出有界missing_finding_ids/unexpected_finding_ids，拒绝集合与显式撤回规则不变；[红绿及28项来源回归](../../artifacts/bid-full-sample/loop-repair/finding-id-feedback/verification.json)通过，未自动删除任何发现，也未将新工具/提示词合同强套旧附表检查点。

整页候选范围修复及真实R17检出（2026-09-11）：[原请求诊断](../../artifacts/bid-full-sample/loop-repair/whole-page-review-scope/diagnosis.json)确认第15–18轮已有整页图像，但候选清单只覆盖网格来源，同页文字中的已提取备注未显示，被误报漏提。成功读图现在记录同一文档页的全部来源依赖；恢复时也按已交付图片校正来源任务依赖，过窄旧判断重新待核。该扩展不增加任何文本、网格或候选阅读回执，也不把另一页/另一文档带入。[红绿及工程验证](../../artifacts/bid-full-sample/loop-repair/whole-page-review-scope/verification.json)为212项库、严格Clippy、fmt及编译通过，工具/提示词/schema/配置保持兼容；[续接核验](../../artifacts/bid-full-sample/loop-repair/appendix-layout-original-view-contract-resume1/startup-verification.json)保留第56轮原预约、57次调用与所有finding。

[R17真实检出](../../artifacts/bid-full-sample/loop-repair/appendix-layout-original-view-contract-resume1/r17-detection-observation.json)已引用原图指出96页8D财务表与98页8E-1说明的实际顺序错误。仅证明检出，不代表主修订、再次复核或出件通过。另有“fixed_text意味着不能填”的[可疑finding](../../artifacts/bid-full-sample/loop-repair/whole-page-review-scope/initial-text-semantics-observation.json)：当前编制器只按区域策略保留/去除初始文字，生成普通表格单元格，并不因fixed_text锁定编辑；不能为通过复核而编造零长度留白范围或删除字段提示，须继续核对并由Agent处理。

前附表选择/关系的[真实误通过](../../artifacts/bid-full-sample/loop-repair/source-scope-reasoning-low-current-trial/semantic-rejection-observation.json)：从空候选41轮完成，22条记录、0关系、0finding，结构审计通过，但四处总则引用被压成一个规则、未形成前附表端点关系；资格后审/不接受联合体的选择没有落实到通用条件，已提取的续页仍被保留为未解决。该结果不计分组验收通过，也不靠清零重跑碰运气。

关系与适用性复核已落到现有 `put_source_review.relationship_checks`：派生当前范围规则、未解决项和条件/未知/不适用要求或模板清单，允许补核其他记录；支持有真实关系的resolved、有原文依据的not_required、有明确剩余歧义的source_limited、引用已保存问题的findings。主体、端点、关系和未解决对象必须独立取回当前版本，引用必须实际交付；冻结集合已有的续页候选须先读并引用，不能把未读当成不存在。新增/删除关系和端点选择变化纳入现有依赖版本，旧clean回执不能跳过新判断。没有词典、固定条款或人工答案驱动的自动连边，也不另建表或复核工具。

[红绿及工程验证](../../artifacts/bid-full-sample/loop-repair/relationship-review/verification.json)通过217项库、20项合同、严格Clippy、fmt与编译；[原22候选离线回放](../../artifacts/bid-full-sample/loop-repair/relationship-review/archived-replay.json)重开两处来源，各有8条必需判断，旧检查点字节不变。校验显式结论与证据不能保证模型语义正确，故仍必须实测。[新合同独立复核](../../artifacts/bid-full-sample/loop-repair/source-scope-relationship-contract-review/startup-verification.json)使用同一3来源及原22条模型候选，只有review_tools_sha256变化，不导入人工预期、旧复核回执或finding；原误通过结论保留为失败。该初版实际复核已误通过，详见本节开头；不能勾选关系组或完整样稿完成。


混合版式原页证据门槛（2026-09-11）：来源任务关联模板在同一文档页同时引用文字与网格时，导航明确要求检查原页；`put_source_review` 的完成判断必须包含该角色已交付的同页视觉引用。主角色阅读、像素缓存、另一文档或另一页不能满足条件；缺证保持 `needs_evidence`，恢复的旧判断也重新检查证据。请求保留当前页的一张独立已读图片，允许同页不同解析来源复用，避免逐格重发。继续使用统一Python DocReader原图与现有coverage，不增加解析器、表、migration或生产预算。

[本地验证](../../artifacts/bid-full-sample/loop-repair/mixed-layout-view/verification.json)为211项库测试、20项合同、严格Clippy、fmt及编译通过；[真实旧判断回放](../../artifacts/bid-full-sample/loop-repair/mixed-layout-view/archived-replay.json)确认7项没有原图证据的版式判断重新待核。工具说明及review_tools_sha256已变，[新合同附表复核](../../artifacts/bid-full-sample/loop-repair/appendix-layout-original-view-contract-review/startup-verification.json)只导入真实主Agent候选，独立阅读/复核状态从新身份开始；旧4轮[拒绝结果](../../artifacts/bid-full-sample/loop-repair/appendix-layout-readonly-contract-resume2/final-observation.json)原样保留。模型是否检出并修正R17，以及实际DOCX顺序，均待真实验收。

连续技术组[终态与内容核对](../../artifacts/bid-full-sample/loop-repair/technical-continuous-reasoning-low-resume2/final-observation.json)：217轮/218次调用，2轮完整独立复核，最终50条记录、0finding，[结构审计](../../artifacts/bid-full-sample/loop-repair/technical-continuous-reasoning-low-resume2/structural-audit.json)通过。各段累计执行3839.27秒，包含诊断中断和本地故障，不能写成连续运行耗时或稳定提速。技术内容核对不替代完整32项、跨范围关系和最终出件验收。前附表选择/适用性/关系组另用当前low从空候选做[新对照](../../artifacts/bid-full-sample/loop-repair/source-scope-reasoning-low-current-trial/startup-verification.json)，原第169轮blocked不清零恢复。


已删除候选的复核导航修复（2026-09-11）：[实跑诊断](../../artifacts/bid-full-sample/loop-repair/deleted-review-reference/diagnosis.json)确认技术组第179轮为本地确定性错误，供应商已正常返回。主Agent合并并删除旧条款后，历史 `dependencies.references` 仍保留旧ID；版本计算以null记录缺失是正确行为，但当前导航再次要求该ID的 `candidate_version`，导致 `unknown work reference`。现在保留完整历史依赖用于版本失效，只将仍存在的候选送入当前比较清单；旧finding必须由独立复核修订或撤回，删除条款不等于解决问题。

[红绿与真实检查点回放](../../artifacts/bid-full-sample/loop-repair/deleted-review-reference/verification.json)覆盖恢复后的删除依赖、未解决finding保留、缺失ID仍影响版本、明确撤回后才能完成来源判断。工具、提示词、版本摘要形状及表结构均未改变。技术组使用隔离编译的原工具合同binary从第179轮/180次调用续接，[启动核验](../../artifacts/bid-full-sample/loop-repair/technical-continuous-reasoning-low-resume2/startup-verification.json)确认runtime与run-contract逐字节相等；不把当前工作区的新只读模板合同强套旧检查点。

附表仍有独立语义缺口：[真实主修订](../../artifacts/bid-full-sample/loop-repair/appendix-layout-readonly-contract-resume1/primary-repair-observation.json)已保留3条模板的前后候选。物理第96页的原图证明标题、网格交错且承诺在末尾；解析非表文字集中在一个来源，单靠text/grid来源顺序不能验证真实版式。当前候选及首轮复核仍漏掉这一差异。现已实施下述原页证据门槛；仍须真实验证模型对实际编制顺序的比较，门槛本身不能证明R17关闭，未新增替代编制器。


只读模板合同修复（2026-09-11 UTC）：[第57轮请求诊断](../../artifacts/bid-full-sample/loop-repair/readonly-template-contract/diagnosis.json)确认原文人员提示与bidder_blank候选同时可见，局部来源仍被签为checked；复核角色不接收put_record schema，而此前只读工具没有解释整格清空与实际编制顺序。现只在inspect_analysis说明中补齐既有执行语义：regions决定出件顺序，同一网格连续区域合成一表；instruction不改写/重排原文；无blank_ranges的bidder_blank清空整格，有范围时只去掉对应字节，其余原文保留。没有新工具、字段、预算或验收例外，工具摘要已变。[新合同启动](../../artifacts/bid-full-sample/loop-repair/appendix-layout-readonly-contract-review/startup-verification.json)沿用原模型生成的13条记录、4条关系和全部来源，空复核状态且不发送人工答案；[真实检出](../../artifacts/bid-full-sample/loop-repair/appendix-layout-readonly-contract-review/front-page-detection-observation.json)已指出原对照漏掉的人员/设备提示清空。随后[首轮完整拒绝](../../artifacts/bid-full-sample/loop-repair/appendix-layout-readonly-contract-resume1/first-review-rejection.json)保存11项来源判断和6项发现，已交给主角色修订；其中还保留旧反馈诱发的“映射已存在”非问题，须由真实复核撤回，不人工删掉。该记录不证明修订、R17或全稿完成。

映射错误反馈修复：[红绿及工程验证](../../artifacts/bid-full-sample/loop-repair/mapping-finding-feedback/verification.json)覆盖正确映射与独立内容finding并存。错误精确到/template_mappings/{index}/finding_ids/{index}，区分外层未包含该finding与没有共同映射证据；明确映射正确时该嵌套列表可以为空，内容问题留在来源finding_ids，不要求编造映射错误。真实缺失映射、引用交付及证据相交检查不变；210项库、严格Clippy、fmt和编译通过，无新SQL或配置。错误文本变化不改变工具/提示词/检查点合同；[兼容续接](../../artifacts/bid-full-sample/loop-repair/appendix-layout-readonly-contract-resume1/startup-verification.json)逐字节保留第58轮原预约、累计59次调用及全部发现。原始对照的[终态](../../artifacts/bid-full-sample/loop-repair/appendix-layout-reasoning-low-trial/failure-observation.json)保留第142轮/143次调用、10项发现及1项执行阻塞，0轮完整复核，不清零恢复。

连续技术low组[第一段终态](../../artifacts/bid-full-sample/loop-repair/technical-continuous-reasoning-low-trial/segment-terminal-observation.json)由30分钟诊断时限取消，已进入独立复核，52条记录、5个已存来源判断，0次供应商超时、无执行阻塞；取消的第89轮物理调用仍计入90次累计。已按[同合同续接核验](../../artifacts/bid-full-sample/loop-repair/technical-continuous-reasoning-low-resume1/startup-verification.json)保留原binary、原预约、计数和provider配置，未混入只读工具新摘要。该段进入复核不等于完整语义通过。

low首个局部对照（2026-09-11 UTC）：[正例比较](../../artifacts/bid-full-sample/loop-repair/reasoning-low/positive-comparison.json)保留默认与low两份原始结果。同一第102页模型种子，两次均3次调用、1轮独立复核、0finding、分析逐值不变及结构审计通过；墙钟106.03→41.01秒，供应商报告推理token合计3236→47。模型实际工具选择、缓存及运行时间不同，该单次观察不能证明因果提速、供应商内部实现或持续稳定性。low整条漏提负例已用15次调用/148.04秒完成拒绝/修订/再复核，默认基线为16次调用/309.07秒；[配对验收](../../artifacts/bid-full-sample/loop-repair/negative-entire-requirement-reasoning-low-trial/paired-acceptance.json)确认两类截图义务、第二类渠道的或关系及模板映射保留，未新增投标事实或否决规则。连续技术与复杂附表组仍在进行，未关闭其余语义或整稿门槛。

low复杂组中途观察：[性能快照](../../artifacts/bid-full-sample/loop-repair/reasoning-low/progress-observation.json)记录技术第18轮完整返回8个工具调用、12537字节工具参数、约105.8秒；供应商报告输出8671 token，高于请求8192，且该轮未报告推理token，不能按0补值。降低推理强度尚未消除用量报告异常；两组仍须完成提取及独立复核后才作语义判断。实际运行中的[工具错误核查](../../artifacts/bid-full-sample/loop-repair/reasoning-low/tool-error-audit.json)确认空焦点、越界读取、空条件及自指模板边被拒绝后可以继续，不据此放宽合同。

low附表候选的[复核前快照](../../artifacts/bid-full-sample/loop-repair/appendix-layout-reasoning-low-trial/pre-review-layout-observation.json)仍发现R16人员/设备/获奖提示被整格留白策略覆盖，以及R17多个标题集中在财务网格前、承诺在网格前的顺序问题。当前不是最终复核结论；须观察独立检出、主修订及再次复核，人工观察不发送模型。该组原来源page_ordinal为90/91/95/97，实际物理页为91/92/96/98；只更正状态文字，不改变输入、预算或运行身份。

全部来源已阻塞时的停止条件（2026-09-10）：新请求预约前，按现有阻塞的来源集合及当前依赖摘要判断是否仍有独立来源。若当前角色处于blocked、且未变化的阻塞已覆盖整个冻结来源集合，直接保留失败，不再要求模型选择不存在的独立来源。只检查新边界，不截断已预约/已保存响应的原处理流程；来源依赖变化或仍有独立来源时沿用既有交接路径。没有新提示词、工具摘要、配置、表或migration。该改动不会修复语义停滞，也不会删除旧阻塞或恢复额度。

[红绿回归及持久化验证](../../artifacts/bid-full-sample/loop-repair/all-sources-blocked/verification.json)：单来源停滞由24次调用降为阻塞当轮的18次，少掉6次无效交接；确认不新增预约，已保存响应仍提交一次，reviewer同样受限，依赖变化和独立来源重启仍可继续。209项库、20项合同、7项隔离PG、严格Clippy、workspace fmt及样稿编译通过，隔离容器已清理。旧运行保留自身binary与实际终态，不把本地回归改写成旧运行已恢复。

最新验收（2026-09-10）：[签署错接负例最终报告](../../artifacts/bid-full-sample/loop-repair/negative-signature-mixed-write-resume3/final-observation.json)确认自然结束，`done=true`、`quality=verified`、2轮完整独立复核、0项最终finding，3项来源判断均为当前版本checked。签署关系从错误8H端点改回8G，与原正例的端点、类型、范围、字段目标及原文依据一致，生产结构审计通过；仅explanation合理改写。该证据关闭孤立错接负例，不代表全部R01/R02、32项或完整提取已通过。[性能统计](../../artifacts/bid-full-sample/loop-repair/negative-signature-mixed-write-resume3/performance-observation.json)中，186次完整流合计约5071秒，本地工具处理约1101秒，检查点保存约8秒，累计报告输入约568万token。局部正确性通过不关闭性能门槛；该链包含兼容宿主修复且使用debug程序，不冒充release吞吐或受控提速对照。后续优化须分别核查重复模型查询和本地上下文容纳耗时，不将慢笼统归因于持久化。

新候选依赖合同的[复合条件主修订观察](../../artifacts/bid-full-sample/loop-repair/negative-compound-small-image-resume1/primary-repair-observation.json)记录首轮完整拒绝后补回漏项，进入第二轮时36项有效比较保留、5项重开；这是实际运行结果，与前述离线投影分开。子条件未重复时间时须结合父记录完整原文判断，不仅按criteria数量验收。[本次终态](../../artifacts/bid-full-sample/loop-repair/negative-compound-small-image-resume1/blocked-source-judgment-observation.json)在第128轮/130次累计调用触及诊断时限，41项比较已全部完成，但第二轮3项来源判断均未完成，且原检查点已有执行阻塞。依赖修复有效不等于解决来源判断停滞；不得清零阻塞或预算继续刷通过。

整条要求漏提补充对照：`appendix-missing-requirement-current-control`与`negative-entire-requirement-current-trial`使用相同第102页原文、当前工具合同、.env模型和预算。正例保留已验证的4条模型记录/1条关系；负例只删除整条截图要求及其关联关系，以避免悬空端点，其余候选和主角色阅读回执相同。生产离线审计确认负例无结构缺口，但旧复核结论不能沿用；两组均从空的独立复核状态运行。预期放在各自`independent-expectation.json`，不位于source目录、不发送模型。须取得真实遗漏发现、整轮拒绝、主修订及再次独立复核，并检查正例无误报；该对照不替代第9项漏提、复合条件或完整32项验收。

该对照[负例终态](../../artifacts/bid-full-sample/loop-repair/negative-entire-requirement-current-trial/final-observation.json)在第19轮完成：reviewer以`affected=[]`保存来源遗漏发现，主角色重建两类名单查询、第二类查询渠道的或关系、两类截图证明及正确模板映射，独立再次复核与结构审计通过，不填写投标人的查询结果。[首个正例](../../artifacts/bid-full-sample/loop-repair/appendix-missing-requirement-current-control/final-observation.json)第16轮完成，但先指出孤立原页不支持模板order=9，主角色将其改为null；不能计为“无finding正例”，也不能在缺少完整排序上下文时武断判为模型误报。`appendix-missing-requirement-order-control`与`negative-entire-requirement-order-trial`因此使用同一份模型已修订且复核通过的order=null候选做新配对，仍只在负例删除该要求及关联边；来源、binary、合同、模型及预算均与前一配对相同，不发送人工答案，旧结果保留。

修订后的[配对验收](../../artifacts/bid-full-sample/loop-repair/negative-entire-requirement-order-trial/paired-acceptance.json)通过：正例3轮/3次调用、1轮独立复核，无finding且analysis逐值不变；负例16轮/16次调用，真实来源遗漏发现、主角色修复、2轮独立复核及结构审计均完成。补回两类截图义务与替代查询渠道，正确连接现存模板，不虚构投标人查询结果或本页未规定的否决规则。两组使用相同冻结原文、provider、预算和归档binary，仅要求及其关联边构成初始差异。该结果关闭孤立整条要求漏提配对，仍不替代其他来源/条件/版式缺陷或整稿验收。

复合条件[旧合同最终验收](../../artifacts/bid-full-sample/loop-repair/negative-compound-mixed-write-resume4/final-observation.json)已在第165轮/168次累计调用自然结束，3轮完整独立复核、3项当前来源判断checked、0项最终finding，结构审计通过。两条criteria明确保留最近三年、骗取中标或严重违约，以及经鉴定部门认定的因产品引起的重大质量事故；首次漏项和后续病句均经模型独立检出、主修订、再次复核。该局部负例通过不替代连续技术正文或完整32项验收。

新增[流诊断验证](../../artifacts/bid-full-sample/loop-repair/stream-diagnostics/verification.json)：只记录已知SSE结束标签及文本/推理/工具参数delta字节数，不记录正文，不改变请求、重试、超时或冻结合同。[旧技术第30轮正文单次诊断](../../artifacts/bid-full-sample/loop-repair/stream-diagnostics/technical-turn30-probe/verification.json)按同一.env、完全相同预约字节调用生产传输一次，约42.229秒返回3个完整工具调用，观察到`tool_calls`结束原因及其后约52ms的`[DONE]`。未执行返回工具，也未导入旧检查点；它证明该输入/协议可以成功，不能证明间歇停滞已解决。旧终态和累计调用不变。[新技术组启动核验](../../artifacts/bid-full-sample/loop-repair/technical-continuous-diagnostics-trial/startup-verification.json)确认相同原文和预算、新身份及空候选，使用当前工具合同与诊断binary；没有导入单次探针响应或人工答案。

[新技术组终态](../../artifacts/bid-full-sample/loop-repair/technical-continuous-diagnostics-trial/failure-observation.json)：第33轮/37次累计调用，15条记录，0轮独立复核。该边界前两次各180秒超时，收到12254/13013字节工具参数delta，但无finish标签、完整工具或最终事件；第三次已预约调用由30分钟诊断时限取消，不能称为第三次完整供应商超时。该边界三次物理调用已消耗，不清零续跑。新观测排除了“这两次已经收到完整结束标签但SDK仍等待EOF”的解释，仍不能仅凭日志区分上游生成停顿与网关截流；单次探针成功也不能解封它。

[输出规模与工作范围核查](../../artifacts/bid-full-sample/loop-repair/technical-continuous-diagnostics-trial/output-scope-diagnosis.json)：第32轮成功响应只有一个put_record，参数11200字节，第二次尝试约168秒完成；供应商报告输出12776 token，扣除报告推理量3842后仍为8934，高于请求max_tokens=8192。第14轮和第3轮也有报告超限，不把这些报告冒充独立tokenizer测量。第33轮实际预约正文100319字节，并未达到2MB字节上限。单个复合条款也会产生较大响应，因此“一轮工具过多”不能单独解释失败，不能用强制单工具或更高预算替代诊断；当前没有据此改变SDK、模型、输出上限或接受不完整JSON。

供应商及指引核查（2026-09-10）：[来源终态的18份完成候选请求审计](../../artifacts/bid-full-sample/loop-repair/source-scope-mixed-write-resume2/completion-guidance-audit.json)确认工作状态及执行指引都要求进入原文判断，未发现反向强制继续比较的冲突；不能凭该假设继续叠加完成提示。技术组同一第29轮请求曾先180秒超时、下一次约22秒成功，而第30轮三次均未完整结束；不能把失败全部归因于输入规模。模型清单[只读查询](../../artifacts/bid-full-sample/loop-repair/provider-output-budget/model-metadata.json)返回403，可用型号未由该接口确认。

用户最新选择（2026-09-11 UTC）：保留grok-4.6，不执行此前模型切换提案；已按明确授权在deploy/.env写入KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT=low，[配置证据](../../artifacts/bid-full-sample/loop-repair/reasoning-low/config-change.json)确认除新增该行外逐字节不变。第102页[正例对照](../../artifacts/bid-full-sample/loop-repair/appendix-reasoning-low-control/startup-verification.json)与第55–63页[连续技术对照](../../artifacts/bid-full-sample/loop-repair/technical-continuous-reasoning-low-trial/startup-verification.json)已启动：同一归档binary、相同原文/种子/预算、仅推理配置变化，新身份及空检查点/独立复核状态。已核对实际预约正文包含low；HTTP接受和局部完整工具输出不证明供应商实际执行了low，也不证明语义验收通过。旧失败、原合同和计数保留，人工验收答案不发送；完整106页、32项和整稿门槛不变。

小图历史裁剪补充（2026-09-10）：旧逻辑仅在一组图片自身超过历史上限时释放像素；约41KB的旧图虽低于64KB上限，叠加候选查询历史后仍挤掉当前原文。现保留原有无损裁剪顺序，在准备丢弃独有证据的回退步骤先释放旧图的独立历史消息，混合批次里的候选/网格和完整工具协议保留；真正需要的当前原图仍由请求组装按已交付身份从像素缓存发送，最新未交付图片不提前裁剪。不改预算、回执、业务内容、工具摘要、提示词或持久化合同。

[红绿回归与四查询投影](../../artifacts/bid-full-sample/loop-repair/small-image-history/verification.json)确认从3项成功/1项拒绝且原文丢失，变为4项成功、原文与焦点候选完整，实际请求407215字节。该投影重建真实第8轮请求与工具批次，使用第11轮回执/缓存快照，不冒充原检查点精确恢复或语义验收；检查按最终发送正文计入候选召回内容，保留原失败。209项库、20项合同、7项隔离PG及Clippy/fmt/编译通过。只有与当前复核工具摘要相同的运行可兼容续接；更早工具合同的旧组仍须使用自身归档binary。

当前终态：[来源组](../../artifacts/bid-full-sample/loop-repair/source-scope-mixed-write-resume2/failure-observation.json)第169轮仍无原文判断，不能以47项比较完成代替整轮复核；[连续技术组](../../artifacts/bid-full-sample/loop-repair/technical-continuous-current-runtime-trial/failure-observation.json)与[版式组](../../artifacts/bid-full-sample/loop-repair/appendix-layout-current-runtime-trial/failure-observation.json)均在同一边界三次180秒后失败。收到部分SSE/SDK事件不等于完整工具响应，现有聚合日志不能区分供应商未完成输出与协议收尾缺失，不将其归咎于上下文大小或宣称已解决。各组不清零重启。复合条件[第二轮拒绝及主修订](../../artifacts/bid-full-sample/loop-repair/negative-compound-mixed-write-resume2/second-review-rejection-and-repair.json)已消除病句，第三轮独立复核现已通过，见本节最终验收记录。

候选比较依赖精度补充（2026-09-10）：此前单候选摘要把引文来源交给整段集合依赖，修正同页一项便让无关比较重新待办；另有模板父项及依赖端点finding撤回未正确失效的问题。现单条record依赖自身、直接相连关系及其端点，relation依赖自身及端点，两者包含引用模板的明确parent、全局Rule集合和相关candidate finding版本；disposition继续依赖整段来源集合。来源任务的新增/删除、跨来源关系、查询查无结果和finding版本依赖保持原有宽范围，不用候选比较取代遗漏判断。新增关系、删除/改接关系、端点变更、父项删除、全局规则变更及撤回finding均有回归；不递归遍历整个关系图，也不新增依赖存储或配置。

[工程验证与真实修订离线对照](../../artifacts/bid-full-sample/loop-repair/candidate-dependency/verification.json)：签署修订影响7项、保留34项，复合条件修订影响5项、保留36项；这是同一新算法下的分析对象变更投影，finding版本保持相同，不把旧算法回执迁移为新回执，不宣称端到端提速倍数。complete_review_check描述同步更新以改变冻结工具摘要；旧运行只可用前驱自身归档binary及全部累计状态继续，新合同以新身份实跑。无需新migration或共享checkpoint字段，也不维护旧摘要算法双路径。`negative-compound-candidate-dependency-trial`使用相同原文、变异候选和.env预算检验真实拒绝/修订/再复核，尚未通过。

复合条件文字质量的[独立模型检出记录](../../artifacts/bid-full-sample/loop-repair/negative-compound-mixed-write-resume2/wording-detection.json)：第二轮reviewer自行定位到criteria[1].condition病句，人工验收观察未发送；当前只证明检出，不能代替最终修复验收。

新合同查询诊断：[第8轮批次拒绝与第11轮单次试算](../../artifacts/bid-full-sample/loop-repair/candidate-dependency/query-rejection/diagnosis.json)保留原四次查询批次末项的原文共存拒绝。后续检查点单独调用相同查询可发送432653字节，必需证据完整、37份可召回候选保留22份；“全部缓存均保留”的既有断言失败，独立的部分缓存/必需证据门槛通过。该试算不是原批次重放，不能据此宣称原失败已修复，也不以新身份或更高预算消除失败记录。

本轮补充：重新规划及尚未完成动作的恢复状态主动移除冗余历史，正常状态不提前裁剪；独有原文、焦点候选、最新工具组及累计计数保留。编制沿用同一原则，带 key 的检查值无论对象还是字符串均属于证据。另针对真实第13轮“3次原图读取＋15项比较”的窗口溢出，保留每项写入前的状态；仅当该项输出无法容纳时原子回滚该项，返回未提交反馈，计入工具尝试但不增加比较/成果进展，其余已容纳结果继续提交。持久化失败、失去执行权及其他致命错误仍停止，不伪造成功、不跳过工具响应，不增加模型调用或配置。原失败、10项保留/5项拒绝的回放、普通回归和隔离恢复见[批次写入验证](../../artifacts/bid-full-sample/loop-repair/mixed-write-pressure/verification.json)；负例发现见[检出证据](../../artifacts/bid-full-sample/loop-repair/replan-context/negative-detections.json)。这些仅证明局部工程行为及检出，修订、完整复核及整稿门槛不变。

以下子任务归属现有 P0/P1/P2，不新增平台阶段。1–5已实现并取得本轮工程验证；6的空候选短测已交接到独立复核但第111轮保留执行阻塞后停止，另开候选种子的隔离复核诊断；7的8G/8H附表短测保留原计数续跑，复合技术条件组在第20轮三次超时后失败，混合格/交错排版组在局部无进展后又于第33轮耗尽三次超时，连续技术正文另组在提取；6–8均尚未通过真实验收。各行右侧仍是接受条件，工程测试不覆盖真实模型的语义准确性。

| 顺序 | 实施内容 | 必须取得的证据 |
| --- | --- | --- |
| 1：先复现 | 用归档轨迹构造“候选全部比较、原文未判断”和“原文也完成、缺空提交”两个不同状态；追踪转义首错边界 | 前者必须留待办，后者应无需新模型调用结束；现状失败与修复通过成对保存；不改原诊断文件 |
| 2：任务和结果 | 派生完整原文片段清单、实现 put_source_review/typed checkpoint、复用字段反馈 | 未选页/空网格不漏任务；跨页/跨表依赖明确；单项漏提允许 source-only finding；主角色冒充 reviewer、未收到证据或旧版本提交被拒绝 |
| 3：失效与导航 | 候选/来源关联集合、查询依赖、finding 版本、pending 导航及预算保护 | 新增/删除/改引用/改关系端点/全局查无结果后新增命中/撤回发现均重开正确任务；无关局部编辑保留有效结果；重复签收与改名不能刷新预算 |
| 4：汇总与撤旧 | 完整批次末汇总、角色切换、删除空提交工具/提示与孤儿代码，同步 baseline | 最后结果后额外收尾模型调用为0；同批最后一个 finding 或失效操作仍生效；有 findings/open items/阻塞不会错标 verified；重复修订和预算耗尽保留真实终态 |
| 5：隔离持久化 | 新域初值、合法边界、发布约束、幂等与恢复测试 | 预约后、响应保存后、工具提交前/后故障，事务失败/ACK 丢失、取消/owner 失效；恢复不多一次模型调用、不重复轮数/发布，不绕过未完成任务；提取与既有编制回归通过 |
| 6：3来源真跑 | 保留诊断种子回放，并另以新身份、空候选实跑第11/16/17页 | 主提取到独立双向复核自然结束，所有必需集合与版本可核验，阅读和语义统计分开；不能只用预填41条记录证明提取成功 |
| 7：复杂附表与技术条件 | 使用已准备的三组真实夹具、原页和网格；修复转义后复验 | R01/R02签署边界、R04–R07复合条件、R16/R17混合格/交错排版及跨来源关系逐条对照；冻结来源未改，无样稿专用分支 |
| 8：完整提取与整稿 | 新运行完成106页提取/独立复核及32项复验，再交既有 O1-S/O2 | 32项逐项关闭或有证据更正；招标要求决定目录与全部章节，附表和固定内容完整，投标事实待填；真实生成同版 DOCX/PDF/报告 |

**负例不可省略。** 在隔离测试中故意删除一项原文要求、丢失复合条件中的子项、错误接续签署区、选中原文未选条件、清空含标签的待填单元格、删掉关系或附表说明。仅阅读回执齐全、72个旧比较齐全、finding 数为0均不得自动通过。确定性测试验证这些状态不能绕过合同；真实 reviewer 能否发现每类缺陷须另跑有对照的语义验收，不用预写 mock finding 代替模型能力证据。测试答案与32项人工预期不发送给 Agent。

负例执行进度（2026-09-10）：以已完成的第100–102页组作为正例依据，`negative-missing-requirement-trial` 已由真实 reviewer 检出删除的第9项要求并引用原文1495–1631字节，整轮复核未结束，暂不标通过；另启动 `negative-compound-criterion-trial` 与 `negative-missing-relation-trial`。后两者经生产离线审计确认结构有效、旧正例复核因摘要变化被拒绝；模型收到原文与变异候选，预期答案放在各运行的 `independent-expectation.json`，不在 source 输入目录。缺陷检出与最终复核拒绝分开记录。缺失要求旧组第39轮执行失败后，以新规划合同另组对照；关系删除组第19轮无finding结束，但被删除边可能与保留的响应到模板关系冗余，负例设计不足，不计通过也不据此断言模型漏检。`negative-signature-target-trial` 检验明确的错误模板端点，第47轮耗尽执行/交接额度，未完成目标比较或检出错接；未勾选条件和标签清空仍待验。

补充负例终态与对照：复合子项旧组第45轮耗尽执行/交接额度，未完成目标比较，不能算检出或通过。为排除平行边冗余，“唯一模板关系”对照保留物理第102页全部原文和4条模型记录，正例有1条要求到模板关系、负例仅删除该唯一关系，其余值逐字节相同。`appendix-query-link-control` 已在4轮/4次调用、约118秒自然完成并verified；`negative-query-template-link-trial` 第3轮无finding且0关系结束，确认误通过。两者只用于局部语义判别，不替代完整提取验收。`technical-continuous-planning-trial` 以新规划合同从空候选复测同一连续技术正文，未清零旧第38轮失败。

**转义回归：** 普通中文、正常 JSON Unicode 编码、错误的双重编码、合法路径/正则/字面转义文本分别验，修复后实际持久化字段可读；不得用“全局二次解码通过中文样例”作为成功证据。

**工程门槛：** 新行为先有必要的红/绿回归，运行相关库测试、合同测试、隔离 PostgreSQL、严格 Clippy、workspace fmt和样稿入口编译；仅文档完善不重跑工程测试，也不把旧日志刷新日期冒充新通过。SQL变化必须补隔离数据库测试，不能只检查 SQL 字符串。

**真实运行纪律：** 启动时重新读取 `deploy/.env` 并归档脱敏有效配置，模型/协议/供应商/预算和工具摘要与预约正文核对；不临时覆盖模型或抬高额度。诊断种子与空候选全链分别报告。使用现有已准备的 fixture/观察入口，不启动另一套样稿 runner；新工具合同从新身份运行，不续接旧72项结果冒充更强验收。

**性能验收：** 分别报告主提取、候选比较、原文核查、修订的调用数、用量、重复任务、失效重审数和耗时；分离供应商响应、工具/数据库与额外收尾成本。离线脚本模型可稳定断言收尾额外调用为0、重复操作不增加完成数；真实语义复核允许必要重读，但不得出现任务全部有效完成后还继续请求模型。复核未完成便在原冻结额度内终止并列出阻塞任务，不将“有界失败”算成完整修复。没有同配置同范围对照时不宣称提速倍数。

最终接受必须同时满足：运行能够结束、来源和候选双向判断完整、32项真实语义问题完成复验、下游整稿可用。若仅宿主收尾通过但模型继续错误理解附件或遗漏条件，保持相应任务开放，按具体证据修复，不再次换运行时或增加空提交提示掩盖问题。网络安全设备、软件、服务和综合项目使用同一来源驱动机制；当前单一设备样稿不能证明其他类型已验收，有真实样本后补各类型记录。

## Main 修复任务隔离：turn1604 后续实施边界

状态：T1–T3 代码已接入，正在完成普通 CI、隔离 PostgreSQL 与整合验证；尚未部署或启动新的真实验收。既有有源异议闭环已有实现，本轮补默认 CI 回归；不能把脚本模型的协议测试写成 grok 已经完成误报裁定。

### 问题与证据

[终态恢复合同核查](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/terminal1604-recovery-contract-audit.md)确认，resume5 的 `journal.pending=null`，局部 watch 已达到交接上限；driver 在准备下一请求前停止。总调用 3826/4000，剩余 174 次不代表还有局部进入权限。第三来源虽然有尚未消费的历史恢复机会，也无法越过该启动门槛。A/E 原依赖和已消费标记仍未变化，不能靠重新启动、提高总帽或换 objective 恢复。

[最后两项原文核查](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/remaining-ae-source-semantics-1604.json)证明剩余工作有不同性质：一项有据缺失跨条款关系，另一项要求的具体参数目标没有得到冻结前附表支持。后者需要主 Agent 有源异议与 Reviewer 独立裁定。主 Agent 已有 `put_repair_result(conclusion=disputed)`，不能为这个用例再创建第二套异议工具，也不能把 Reviewer 的 correction 当成必须无条件执行的答案。

当前 `repair::packet.next_finding` 仅提供导航。真正的阻塞和完成仍按 `source_scope` 管理，同一来源上多个问题共用执行 watch；完成后的下一任务导航也继续计入旧来源。这是下一项需要改变的执行粒度。[独立设计审阅](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/fixed-finding-task-contract-review.md)同时指出：无有效 receipt 只表示未登记或已失效，绝不表示没有尝试过。

### 固定身份与账本

1. 仅在 Main 修复阶段增加领域内的任务状态。复用现有 finding、候选局部版本、`valid`、`relevant_change`、Progress 原语和三边界 Journal；不建新 SQL 表，不抽跨领域 AgentHost，不更换 Rig、模型或 Chat 协议。
2. 任务沿用 host 分配的 `review_draft` finding ID，另存当前 finding 内容 SHA 用于原反馈与 receipt 匹配。原 ID 下改写 correction、切换来源或改变候选版本，均不能获得新的任务额度。现有完全相同 SHA 的多个 draft ID 必须共享执行账本，保留别名对应；不引入模糊语义去重。
3. 任务集合从完整独立报告，或现有显式冻结的归档未完成反馈初始化。后者保留原 draft ID 与“未完成复核”身份，不能伪装已完成报告。Main 期间不能新增、删除、重编号任务。缺少可靠 ID 对应的旧 `Review.findings` 数组不得按位置推断新身份。
4. 每个任务保存累计执行消耗和已用 watch；已完成任务因相关变更重新待验，也沿用原账本。读取、换焦点、登记不同措辞、依赖 A→B→A、无关反馈变化都不重置消耗。任务累计上限和修复轮总上限必须显式有界；依赖版本只决定是否需要重验，不能独自创造新尝试额度。
5. turn、tool_calls、read_bytes、物理调用及旧失败审计继续保留。任务额度是既有总预算内的分配，不能替代总帽。新方案的任务执行上限优先由已冻结的进展限制推导，避免为这项修复增加一组任意环境变量；实现固定为 `min(max_turns, max_focus_turns × (max_focus_replans + 1))`，使用 checked 算术拒绝溢出；默认进展限制为每任务 72 次完整工具批次，仍受原总预算约束，不增加环境变量。无进展可提前耗尽；新依赖仅在同一累计上限内允许一次有界重入。

### 派发与完成

- 宿主确定性选择一个可执行的待修复任务，提供原 finding、已有处置、相关候选索引与原文定位。原文、详情、跨来源端点继续通过现有有界工具读取；不自动附上人工判断或预填关系答案。
- `source_scope` 继续控制当前阅读范围，任务身份控制执行计账。模型为同一问题查找跨页目标、调整 focus 或拆分阅读，仍属于同一任务；一个任务耗尽不能消耗同页另一个任务的独立额度。
- 任务内允许有据 `revised` 与 `disputed` 两种处置。已有修复符合当前局部版本时可以重新确认，不要求再次制造候选变更；主异议不删除原 finding、不生成独立阅读回执、不自动获得来源通过。
- 完整工具批次提交后，宿主才用现有 `valid` 检查当前任务的 receipt 并推进。一个批次不能在执行到一半时无提示地切换任务；同批后续变更导致 receipt 失效时仍保留该任务。角色交接继续要求全部处置有效、来源门槛满足及独立复核完成。
- 所有剩余任务耗尽时，保存实际未处理任务和消耗后确定性停止。任务列表为空与“尚有任务但都不可执行”分别返回，不能靠模型寻找不存在的独立工作。

### 合同与旧终态

当前实现将 `repair_task_policy=main-repair-tasks-v1` 纳入必需的冻结 Config 字段，任务账本保存在 `repair.tasks`；旧配置缺少策略字段直接拒绝。真实 prepare 和完整工具批次末允许派发，低层请求构造和 sizing 不派发、不计费；已保存的 prepared/received 保持原请求与任务。

新增持久任务状态会影响冻结合同。`migrations/bidding_v2_baseline.sql` 当前对 `repair` 对象使用精确字段校验；必须同步更新 Rust、baseline 验证与隔离 PostgreSQL 测试，不能只在 Rust 加默认字段便宣称兼容，也没有新增 Agent 表的必要。任务状态与策略版本必须进入冻结配置/检查点校验；旧合同仍使用原归档程序与原失败记录。

**不自动迁移并重新授权 turn1604。** 旧来源 watch 不能可靠拆成逐 finding 的实际消耗；A/E 两项事实上已多次尝试，不能初始化为 fresh。迁移必须保守保留旧耗尽和 consumed 状态，104 条有效说明也不因来源交叠而抹掉。若后续决定授予额外有限尝试，须作为明确的新合同变更记录来源、范围、上限及累计调用衔接；不是原合同的无损续跑。当前尚未作该授予、调整配置或启动新验收。

### 实施顺序与验收

| 步骤 | 实施范围 | 可验证结果 |
| --- | --- | --- |
| T0 异议闭环回归 | 独立 `tests/repair_dispute.rs`，复用真实工具与 Journal | 受影响候选必须读详情；主异议不能自批；Reviewer 真正收到原 finding 和异议；保留 finding 不能发布；独立撤回后源码判断完成且零虚构边 |
| T1 任务身份与消耗 | 修复领域内的任务状态及纯选择/计账函数 | 同页两个问题隔离；原 ID 改措辞、相同 SHA 别名、重新待验不刷新额度；缺身份旧输入拒绝 |
| T2 Main 派发接入 | `agent/repair`、工作范围/请求装配的必要入口 | 当前任务与有界证据明确；跨页阅读不换任务；批次末推进；耗尽任务保持失败；不得往 agent.rs 堆一套新状态机 |
| T3 持久化合同 | 冻结配置、既有 baseline 与 PG 合同测试 | prepared/response/committed 恢复不重复派发或扣漏额度；旧合同拒绝隐式导入；事务失败保留原账本 |
| T4 离线及真实对照 | 同页独立问题、跨页缺边、无对应目标的异议场景 | 脚本测试证明宿主边界；真实模型另证能找对关系、提出有源异议并完成独立复核，人工答案不发送 |
| T5 全文与整稿 | 原 106 页及完整图、全部 32 项、既有编制/Office 链 | 按原完成门槛取得接受结果、招标驱动全部章节和模板、同版 DOCX/PDF/报告；局部任务成功不能代替整单验收 |

任务隔离解决消耗与阻塞归属，不保证模型语义自动正确。T4/T5 的实际结果仍是最终依据；若模型继续遗漏附表内容或误解条件，保留相应失败并修复具体原因，不通过放宽门槛结单。
