# 已确认方案实施任务台账

2026-09-12 UTC 最新终态：`real-run-v19-repair-scope-resume5` 已于 12:43:50 UTC 结束，运行退出码 1，本段耗时 4427.34 秒；[固定终态](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/resume5-terminal-verification.json)为 turn1604（SHA `5e69cacf…`），424 条候选、177 条关系、104 条保存修复说明，有效处置 104/106、待处理 2，保留 3 处执行阻塞，完整独立复核 0 轮。错误 `AGENT_TURN_BUDGET_EXCEEDED` 指局部执行及独立工作交接额度耗尽；全文累计调用 3826/4000、尚余 174 次，并非总调用帽耗尽或供应商超时。未重启。有效处置不等于独立语义批准；完整 106 页、32 项语义及同版 DOCX/PDF/报告验收仍未完成。正确性与稳定性优先，速度优化后置。

主修复异议闭环：[宿主核查与默认 CI 回归](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/repair-dispute-closed-loop.md)确认生产已有 disputed→独立保留或撤回→完整来源复核及编制准入的闭环；本次仅补测试，覆盖受影响候选详情门槛、主角色不能自批和独立裁定分支。[验证记录](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/repair-dispute-closed-loop-verification.json)为最终断言加强前 217 项 tender_analysis 测试通过、36 项既有忽略，随后加强断言的新专项 1 项通过，严格 Clippy、全仓 fmt 与 diff 整合检查通过。这是合成脚本的宿主协议验证，不是 grok 的真实语义成功。[主方案](bidding/agent-runtime-rig.md)已记录“Main 修复任务隔离：turn1604 后续实施边界”，T1–T3 任务账本、Main 派发和既有 Journal 合同已进入代码整合与离线回归，尚未完成整合验收或部署，未授予新尝试；旧 turn1604 终态、检查点、阻塞及累计调用账本保持不变。

编制版式：[行内布局修复](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/inline-template-layout-implementation.md)已完成局部验证：同一冻结来源中连续文本区域保留原始换行和独立字段书签，示例填写值在原位置清除；跨来源、间隔和网格保持边界。固定 turn1571 的[真实候选前后 DOCX 对照](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/inline-layout-before-after.json)确认四个身份字段恢复同行、20 个区域定位均可回读。[整合验证](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/inline-layout-integration-verification.json)为 40 项编制测试通过、1 项既有忽略，严格 Clippy、全仓 fmt 与 diff 检查通过；无新增 migration，未改提取 schema 或配置。该局部诊断不是整单样稿，也不代表原页像素、完整 DOCX/PDF 或 R03/R06 验收通过。

终态局部核查：[A/E 四条处置复核](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/ae-repair-semantic-check.json)绑定 turn1604，四条均为 `revised` 回执刷新，所引用记录和关系已在 turn1231 存在且未变；引用目标有原文依据，但字段语义及重复关系风险仍在，不能计作四个新语义修复或独立批准。 [最后两项原文核查](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/remaining-ae-source-semantics-1604.json)区分 A 的有据缺失关系与 E 尚无具体前附表目标支持的修复要求；后者需要有源异议或准确未决并由独立复核裁定，不能强行补边。[终止轨迹](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/repair-tail-terminal-check-1604.json)未出现“全部修复完成却无法复核”的状态。以上人工诊断不发送模型，检查点、阻塞及调用额度保持原样。

历史工程状态（恢复范围完成校验修复部署时，下方检查数字不含本轮行内布局修复）：Main 完成范围与既有阻塞目标交叠时，只要该目标仍有未有效处理的问题，就拒绝 complete，保留目标 watch 并给出既有问题导航；覆盖合并、部分交叠及合法恢复后直接完成的路径。此前局部 v2、历史召回与候选身份索引导航保持生效。[最终联合检查](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/current-checks.json)的 320 项库测试、17 项 baseline、严格 Clippy、fmt、diff 及构建全部通过，39 项库测试仍忽略；[实现独立复核](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/implementation-review.md)与[迁移脚本独立复核](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/transition-review.md)完成，[实际部署核验](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/deployment-verification.json)通过。未改 Progress 算法、schema、静态提示词、`.env`、预算或检查点字段，未新增 migration；历史已删除 watch 不凭空还原。

历史恢复：resume4 停止于 prepared 边界 turn1147 / calls1150，[原边界恢复验证](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/resume-verification.json)通过；resume5 的[首个请求](../artifacts/bid-full-sample/real-run-v19-repair-scope-resume5/startup-verification.json)保留原 98,478 字节正文。[turn1156 启动快照](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/started-progress-snapshot.json)与[turn1514 中途快照](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/progress-snapshot-turn-1514.json)仅为历史记录，已由上方 turn1604 终态替代。

独立局部语义核查：[R06 修复报告](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/r06-repair-semantic-check.json)严格绑定 turn1472，确认弱口令跨页完整文本、续项继承适用性、父项证明和响应义务、续项关系已补齐，关系端点版本匹配；“制行方式”来自冻结原文，并非此次修复新增错字。父子项仍各自声明同一证明响应，存在下游重复编制风险，须检查实际整稿；证明对象本身并未重复，不能预先断言 DOCX 已重复或 R06 整项已通过。

[R03 修复报告](../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/r03-repair-semantic-check.json)严格绑定 turn1499，只确认新增一条与原网格证据相符的 requires_template 关系；四条相关既有记录未变，unknown 适用性/强度及附表归属仍未解决。原标题、单位、表外注释及跨页续文、签署虽已在 requirement，仍未进入模板 region 或建立对应关联；表内备注实际保留，不能误报全部注释缺失。未核查该版实际 DOCX/PDF，R03 完整验收未通过。两份局部核查分别绑定各自检查点，不扩大为 turn1604 或完整样稿已通过；人工诊断不发送模型。

固定 turn939 的语义预检仍单独保留：[R03 原文与网格证据](../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/r03-939-source-evidence.json)及[诊断](../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/r03-939-source-review.md)表明报价表同页标题、表外注释及续页签署来源已存在，但当时网格适用性与模板关联尚未完成；表内备注已保留，不能误报全部注释缺失。[R04–R07 技术预检](../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/technical-r04-r07-precheck-939.json)确认若干正文/证明保留子断言，同时 R06 弱口令跨页记录仍缺连接。两项诊断只绑定 turn939，后续候选变化须重新核验，不能当作后续检查点的最终状态或已通过验收；人工诊断不发送模型。

2026-09-12 UTC 历史状态（候选身份索引导航部署，10:28）：候选 ID 不存在或类别不匹配时，保留失败并给出既有 `inspect_analysis` 索引查询，由模型取回完整 ID 后再精确读取；不猜测或自动替换 ID，不自动执行查询或授予详情回执。此前主修复局部 v2、旧摘要兼容及历史只读导航保持生效。[最终联合检查](../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/current-checks.json)的 315 项库测试、17 项 baseline、严格 Clippy、fmt、diff 及构建全部通过，39 项库测试仍忽略；本次 4 项普通专项测试、[固定 turn666 的实际 48,000 字节预算索引及精确详情投影](../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/fixed666-index-projection.json)、[独立代码复核](../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/identity-error-review.md)通过。[部署核验](../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/deployment-verification.json)已完成；未改 schema、静态提示词、`.env`、预算或恢复许可。未知 source 错误不在本次修复范围内。

该次历史续跑记录：旧 `real-run-v19-repair-local-resume3` 已通过 SIGINT 停止于 prepared 边界 turn825 / calls827，原 5 处阻塞与 44 条修复说明完整保留，[真实边界无模型恢复验证](../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/resume-verification.json)通过。新 `real-run-v19-repair-id-resume4` 已从原检查点启动（会话 65461 / PID 2278412）；[首个实际请求](../artifacts/bid-full-sample/real-run-v19-repair-id-resume4/startup-verification.json)与离线恢复的 64,896 字节正文一致，`.env` 原 SHA、冻结来源及旧 journal 原档均未改变。

该次历史观测：运行进度引用[固定的 10:43:32 UTC 快照](../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/progress-snapshot-2026-09-12T104332Z.json)：turn924 / calls927，全文累计 3146/4000；419 条候选、122 条关系、52 条保存修复说明，有效处理 44/106、待处理 62，保留 4 处执行阻塞，完整独立复核 0 轮。[E 范围只读诊断](../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/blocker-e-source-navigation-diagnosis.json)表明查询反复停留在正文条款，尚未取回前附表目标及选择分支证据作完整比较；source ID 错误提示不能单独解决该问题，finding41 也未被判定错误或撤销。源码部署、保存说明及工程验证不代表语义通过；完整 106 页、32 项语义及同版 DOCX/PDF/报告验收仍未完成。正确性与稳定性优先，速度优化后置。

2026-09-12 UTC 历史状态（09:28）：历史召回、有界恢复与按范围/阻塞状态选择待修复问题的导航已完成[联合验证](../artifacts/bid-full-sample/loop-repair/repair-history/navigation/verification.json)；295 项库测试、17 项 baseline、严格 Clippy/fmt/diff 通过，37 项库测试仍忽略。`real-run-v19-repair-history-resume2` 正在运行（会话 87609），09:27 快照 turn519 / calls521，全文累计 2740/4000；420 条候选、87 条关系、25 条保存修复说明、3 处执行阻塞，独立复核 0 轮。此次已有实际候选修正和新端点关系，但一条 Rule 修改触发全局版本失效，有效处理数曾 23→0，现重新核查后为 18；[只读诊断](../artifacts/bid-full-sample/loop-repair/repair-history/navigation/global-rule-invalidation/assessment.md)确认主修复说明与独立复核共用全局规则依赖，执行 blocker 的局部依赖却未变化，尚未实施该职责拆分。首条修正核心有原文支持，但新缩写与字段引用精度仍待复核。原检查点、已消费额度及 `.env` 配置均保留；32 项语义与完整 DOCX/PDF/报告验收尚未通过。

2026-09-12 UTC 并行结构审计：已迁出source_review/context/repair测试，context生产类型不再排在测试之后；source_review已独立为`tender_analysis/source_review/{mod.rs,tests.rs}`，旧agent路径和公开导出已删除。MAIN/REVIEWER迁到`crates/bidding/prompts/`，原提示词字节和SHA不变；图片读取迁到`agent/view_io.rs`，原函数体不变且无转发包装。FrozenInput入口复用现成docparser网格校验器，42张真实表离线通过，未重造解析/几何算法、未新增migration。[合并工作区验证](../artifacts/bid-full-sample/loop-repair/repair-disposition-handoff/parallel-verification.json)为bidding273项、docparser48项、authoring schema3项通过，Clippy、全仓fmt与diffcheck通过；34项依赖特定归档/环境的ignored测试不计入通过。独立模块迁移另经56项source_review、1项runner及1项冻结配置恢复测试通过，14项模块归档回放与2项runner归档测试保持ignored。模块仍依赖Agent Checkpoint/context，仅必要访问扩大到tender_analysis内部，不代表业务完全解耦；[主tests.rs拆分证明](../artifacts/bid-full-sample/loop-repair/repair-disposition-handoff/tests-physical-split/verification.json)：父文件已收至583行，105项测试迁入8个协议模块，原input/repair模块保留；父测试树117项及其中3项ignored清单不变，整个tender_analysis测试176项通过、31项ignored。此次P1物理拆分完成，其他生产职责与完整结果获取接口仍待整理。v19冻结二进制未更新，以上源代码整理不替换运行程序；截至07:47第104轮/105次本次调用，累计2324/4000，已保存13条主处理记录，候选419/关系40，0轮完整独立复核。另对第51轮最早5条主处理记录的只读抽查发现一条引用仅部分覆盖、一条修正引入新错字，人工诊断保存在source之外且不发送网关，必须在当前候选及后续独立验收中复查。主处理数量、并行工程测试均不代表106页/32项/DOCX/PDF已通过。

2026-09-12 UTC 当前实施状态（07:45 UTC，覆盖下方旧状态）：逐条修复约束及独立异议复核已通过[269项库、17项当前基线合同、7项隔离PostgreSQL、3项runner/归档来源测试及Clippy/fmt/编译](../artifacts/bid-full-sample/loop-repair/repair-disposition-handoff/verification.json)。除“读完不等于处理完”外，已修复新异议可能复用旧来源判断直接结束的问题：主处理说明只使相关来源判断失效，复核详情携带该说明，原独立报告不被主Agent改写。v19于07:34:36启动，[首个真实请求](../artifacts/bid-full-sample/real-run-v19-repair-dispositions/startup-verification.json)核对.env、grok-4.6、Chat、low及新工具/提示词摘要；此前2219次全文调用保留，本次1781次，全文总额4000及补充73/160不变。v18最新418候选/39关系与原已验证反馈依据分开保留，106历史模型意见重新核对；v18撤回的一条意见有独立归档，本次不继承旧通过回执。截至07:43为第66轮、67次本次调用、8条主处理记录、3条候选变化、0轮全量独立复核。主处理记录不是正确性证明；106页、32项语义和完整同版DOCX/PDF/报告仍未验收。用户授权后已并行迁出source_review/context及repair测试、逐字节提取提示词、迁移图片I/O；输入入口复用docparser现成网格校验器的最终回归进行中。运行使用冻结程序，不被这些并行源代码整理替换；速度仍后置。

2026-09-12 UTC 当前状态更正（07:17 UTC，覆盖下方旧运行状态）：v18已于06:47:44主动停止，第58轮、59次实际调用；累计全文2219/4000次，剩1781次，另73/160次局部对照不变。保存418候选/39关系、11来源判断、105条模型问题、0轮完整独立复核。106条旧反馈全部交付后，主Agent仍在第34轮只处理少量问题就提前交接；原表单标题、固定提示被填空覆盖及转录错误有整条候选未变的明确证据，不能把“已读”当“已处理”。[诊断及回归目录](../artifacts/bid-full-sample/loop-repair/repair-disposition-handoff/diagnosis.json)记录逐条处理队列修复：主Agent必须保存实际修正或有原文依据的异议，修改结果须完成详情交付；无关改动不算当前修正，相关版本变化会使处理记录失效，独立复核仍决定正确性。现有检查点增加repair状态并升级baseline保存校验，不新增表或独立migration；新工具/提示词合同不可续用旧运行。266项库测试和4项隔离PostgreSQL检查已通过，另补充删除/同批读取/恢复回归通过；最终全套检查及新合同交接仍在进行，尚未冻结新程序或恢复模型调用。已完成的v18真实修正和旧反馈依据须分别保留，不能伪造新独立回执。全106页、32项语义及同版完整DOCX/PDF/报告未验收，正确性与稳定性优先，速度后置。

2026-09-12 UTC 新修复合同与完整问题交付（06:37 UTC）：旧v17于第537轮优雅停止，累计全文调用2160次，保留415候选/29关系、133来源判断、106模型问题、2处执行阻塞和未完成后段；没有完成独立复核。第96页另出现把8D-4当前网格引用用于8D-3问题的反复失败，现仅在原错误反馈补一处已保存问题的紧凑原文位置，明确不是阅读回执或该问题适用的证明；不自动修改参数，不放宽校验，不新增schema/提示词/字段/配置/migration。[真实轨迹与离线反馈投影](../artifacts/bid-full-sample/loop-repair/mapping-source-feedback/feedback-replay.json)、[263库/20合同、Clippy/编译与模块fmt](../artifacts/bid-full-sample/loop-repair/mapping-source-feedback/verification.json)通过；同次全仓fmt保留共享worker/src/knowledge.rs换行差异，本任务未覆盖其逻辑，不能宣称全仓全绿。已核对旧进程身份、记录两处已诊断阻塞和验证后的替换程序后停止，替代上一条等待所有独立来源结束的过渡安排；未完成来源没有视为已完成。v18导入经生产原文/候选回执校验的原模型问题和候选，重新检查完整106页/148来源/42网格，106原图文件摘要一致；独立回执为空，不继承通过结论。保留总额4000和此前2160次，剩1840次；[实际首请求](../artifacts/bid-full-sample/real-run-v18-repair-feedback/startup-verification.json)仍为.env的grok-4.6＋Chat＋low，无临时覆盖。[真实完整交付](../artifacts/bid-full-sample/real-run-v18-repair-feedback/feedback-delivery-observation.json)确认106条问题从0接收变为106条已提交回执，第13轮已有4条原候选变化；仅关闭本次完整反馈交付，不证明106项已修复。32项语义、字段忠实性、固定日期标签/附表交错顺序及同版完整DOCX/PDF/报告仍待验，速度后置。

2026-09-12 UTC 跨页附表问题归属冲突修复（06:19 UTC）：真实第87页的缺失关联 finding 只有前页原文、affected为空；嵌套映射校验接受后，外层仅按当前来源或affected ID筛选又将它拒绝，模型删掉它则再次触发缺失映射错误。现只在现有映射/关系证据校验成功后纳入其明确引用的问题ID，不扩大相邻页来源、候选或问题清单。无新增schema、提示词、检查点字段、表或migration。[原错误与诊断](../artifacts/bid-full-sample/loop-repair/cross-source-finding-admission/diagnosis.json)、[263项库/20项合同及Clippy、fmt、编译验证](../artifacts/bid-full-sample/loop-repair/cross-source-finding-admission/verification.json)通过；无关内外层问题、未读原文和过期候选继续拒绝。第433轮原模型参数在第480轮候选快照上的[内存投影](../artifacts/bid-full-sample/loop-repair/cross-source-finding-admission/archived-replay.json)可提交并保留findings状态；只投影活动任务/依赖版本，不增加阅读或比较回执，不改原检查点，不是真实模型修订或全量通过。新归档程序同时包含此前反馈完整交付、字段忠实性、准确来源缺口与关系幂等修复，尚未替换冻结v17。旧程序继续可执行来源；若剩余任务仅有已保存阻塞，在核对进程身份后优雅停止并归档未完成状态，再以新合同导入经生产原文/候选回执校验的模型问题和候选，独立复核从空回执开始；旧失败/调用计数不清零。本条替代前文“必须等首轮完整报告后切换”的执行前提，不能用已证实会阻止首轮完成的旧校验无限等待。全106页、32项语义与同版DOCX/PDF/报告仍待验，速度后置。

2026-09-12 UTC 真实来源缺口与提取错误区分（05:53 UTC）：已定位边界校验把所有unresolved边界强制保留为finding，与编制允许准确来源缺口的既有设计冲突；即使主Agent已保存正确未决记录，局部也反复修订。现仅在当前Unresolved记录已独立取回并比较、边界原文对应且relationship_checks明确source_limited时允许checked；边界仍unresolved，来源报告仍保留缺口，缺证/过期候选/缺判断继续拒绝，可用续文仍须独立检查。无新增字段、表或migration。[红绿及262项投标库、20项合同、Clippy/fmt/编译验证](../artifacts/bid-full-sample/loop-repair/source-boundary-open-item/verification.json)通过。同原始两来源与19条已修订模型记录/7条关系的新独立复核实际7次调用、1轮、零finding正常结束，候选不变，quality=needs_review且未决项保留；[实际结果](../artifacts/bid-full-sample/loop-repair/source-boundary-open-item/real-result.json)与[生产编制准入审计](../artifacts/bid-full-sample/loop-repair/source-boundary-open-item/structural-audit.json)通过。该结果只证明局部来源缺口可准确报告，不代表106页完整或已生成DOCX/PDF。三组对照合计73次，仍在原160次补充额度内。新工具/提示词合同尚未部署到v17；全量首轮报告完成后按新合同修复，旧检查点/调用计数保留。

2026-09-12 UTC 修复反馈完整交付检查（05:29 UTC）：真实v17主阶段反复读取第0/20项起的页，99条原始反馈仅40条出现在58个主请求中，后59条未见交付就进入独立复核；结构/来源阅读检查原先不检查修复反馈是否读全。现复用main_progress.seen保存精确问题内容的已交付摘要，仅在完整模型响应后提交；请求中提供未读数量和下一查询，request_review拒绝未读完整清单的交接。内容改变须重读，查询不代表修复、不清除发现、不授予独立阅读/通过、不重置进度额度。无新表、检查点字段或migration；主工具说明合同变化，新检查尚未部署到冻结v17。[诊断](../artifacts/bid-full-sample/loop-repair/repair-feedback-coverage/diagnosis.json)、[真实归档离线投影](../artifacts/bid-full-sample/loop-repair/repair-feedback-coverage/archived-replay.json)、[261项库/20项合同及Clippy、fmt、编译验证](../artifacts/bid-full-sample/loop-repair/repair-feedback-coverage/verification.json)。离线补齐59条只证明分页和回执规则，不是模型已修复59项。当前继续全量独立复核，后续新修复合同同时接入已验证的字段忠实性规则与关系幂等；32项、完整DOCX/PDF/报告仍待验。

2026-09-12 UTC 正确性核查更新（05:13 UTC）：v17 全量独立复核仍在运行，尚未完成第一轮；主Agent只修订了部分候选，不能将99条初始发现视为已全部修复。已确认原文与错误候选同时交付后仍发生语义漏检：关系说明及要求条件把原文企业名称替换为另一企业，至少该关系被错误判为无问题。通用原文忠实性指令的局部对照已实际检出：同原文/原候选/预算，runtime仅两角色提示词摘要不同；旧指令完成第一轮仍漏掉两处，新指令第5轮自主保存两处字段错误及原文依据。主Agent已通过真实工具修正两处字段，修订后完整独立复核确认了相同候选版本且无这两处发现。对照归档时新组共3轮复核/40次调用，旧组1轮/26次；局部仍有截断来源的续页缺失发现，未将局部整体标为通过，也不能将目标字段闭环推广为全量正确性保证。人工预期留在source目录外，新指令未部署到全量冻结程序。[对照证据](../artifacts/bid-full-sample/loop-repair/candidate-prose-fidelity/detection-comparison.json)、[额外160次对照预算](../artifacts/bid-full-sample/loop-repair/candidate-prose-fidelity/budget-amendment.json)。完全相同关系重复新建的幂等返回也已实现，保留不同说明/作用域/证据的独立主张；未清理旧关系或把数量增长算作修复。新编译、严格Clippy、全仓fmt和diff检查通过；库测试259项在沙箱通过，另1项因本机bind权限失败后在允许本机端口的环境通过，30项忽略。此前并行knowledge接口不匹配编译失败保留为历史。[工程验证](../artifacts/bid-full-sample/loop-repair/candidate-prose-fidelity/verification.json)。全106页、32项语义及新增字段忠实性检查、完整同版DOCX/PDF/报告仍待验；正确性与稳定性优先，速度后置。下方早期“当前”记录仅代表各自时间的历史状态。

2026-09-12 UTC 用户调整优先级：先完成全链路功能、正确性和稳定性验收，再处理速度优化。当前继续全106页独立复核与发现修复、32项语义复验、完整同版DOCX/PDF/报告及恢复一致性验证；保留耗时观测，但不以性能不达标中断正确性验收，不放宽来源覆盖、附表对应关系或质量门槛。

2026-09-12 UTC 新合同修复运行（04:23 UTC）：v16-resume1已优雅取消并保留终态：621轮、累计1622次调用、126项来源判断、99条模型发现、3处执行阻塞，0轮完整复核。旧检查点/预约/计数均未改写。v17-repair以新合同先让主Agent核查并修复既有候选；仅从原检查点导入通过原来源/候选阅读证据校验的模型发现，不导入独立阅读回执、来源通过判断或已完成复核。旧发现不是人工答案或已通过结论，主Agent不能删除它们，后续仍需全106页/148来源/42网格的独立复核。按用户允许提高预算，将连续全文验收调用上限提高到4000，旧1622次继续计入，新运行上限2378次；.env仍为grok-4.6＋Chat＋low。已实际启动修复并产生记录/关系修改，不能据此关闭32项或发布DOCX。[启动核验](../artifacts/bid-full-sample/real-run-v17-repair/startup-verification.json)、[预算与来源审计](../artifacts/bid-full-sample/real-run-v17-repair/preflight.json)、[动态进度](../artifacts/bid-full-sample/real-run-v17-repair/progress-observation.json)、[工程验证](../artifacts/bid-full-sample/loop-repair/repair-bootstrap/verification.json)。独立复核、32项及同版DOCX/PDF/报告仍待验，速度优化后置。

2026-09-12 UTC 正确性阻塞修复（04:06 UTC）：全文复核在物理第75页投标函映射判断上发生证据不匹配；恢复时已知问题只能分页查找，多次整页超出工具字节限制，且误把问题ID传给候选查询，最终记录执行阻塞。第84页文字/表格范围随后也阻塞，除重复问题清单失败外，还出现候选详情与当前原文的上下文共存拒绝，后者已补问题查询的上下文保留检查，真实恢复仍待验证。已实现 reviewer 按ID精确取回草稿问题、按字节预算分页返回完整问题、映射错误提供精确查询，并复用现有上下文检查缩小问题页，保留当前比较原文；不截断问题证据、不放宽映射校验、不清零阻塞或计数。[诊断](../artifacts/bid-full-sample/loop-repair/full-review-blocker/diagnosis.json)、[验证](../artifacts/bid-full-sample/loop-repair/full-review-blocker/verification.json)：258项库测试、20项合同、样稿编译及修改文件fmt通过；全局fmt末次遇到共享 worker/src/bidding.rs 编辑中的语法错误，严格Clippy被其他会话新增的 submission_export.rs 八参数函数阻止，本任务未改这两个文件。归档第515轮请求窗口在第539轮状态副本上的离线投影，两次查询均成功、所需证据未丢失；这不是第515轮原检查点恢复或真实模型验收。新工具/提示词合同尚未部署到真实运行，旧冻结程序继续其他独立来源；后续须用新运行身份验证，不能把本地回归当作阻塞已解除。32项语义、完整独立复核和同版DOCX/PDF/报告仍待验，速度优化继续后置。

2026-09-12 UTC 预算调整与续跑：用户明确允许提高预算，全文累计物理调用上限由1200提高到2400；此前998次及v16已用109次继续计费，调整时剩余1293次。v16安全停于第107轮（本段1226.85秒），续跑副本保留31项来源判断、11项草稿发现、原请求字节和全部预约/重试计数；仅修改物理预算、逻辑轮次及对应SDK轮次上限和配置摘要，未重启独立复核。v16-resume1已通过生产配置/检查点校验，并以相同正文续接第107轮，启动核验时累计1108/2400次；模型仍为deploy/.env的grok-4.6＋Chat＋low，上下文和输出预算不变。[预算变更审计](../artifacts/bid-full-sample/real-run-v16-review-resume1/budget-amendment.json)、[启动核验](../artifacts/bid-full-sample/real-run-v16-review-resume1/startup-verification.json)。尚无完整复核轮次，32项语义及完整同版DOCX/PDF/报告仍待验；提高预算不代表速度或质量问题已解决。以下旧预算和进度记录保留为历史。

2026-09-12 UTC 当前验证（03:35 UTC）：共享工作区补验253项投标库测试（30项忽略）、20项合同、严格Clippy与全仓fmt均通过；知识库测试目标编译通过。先前并行取消令牌改造造成的两次编译失败与Clippy失败均保留；本轮仅补工作区依赖、修正测试辅助函数构建范围及必要导入/注释/格式，[工程记录](../artifacts/bid-full-sample/real-run-v16-review-resume1/current-workspace-verification.json)明确区分失败与最终通过。冻结程序的全106页/148来源/42网格复核继续，模型仍为deploy/.env的grok-4.6＋Chat＋low；第454轮、累计1455/2400次调用、103项已保存来源判断、45项待复核任务、72项草稿发现、0轮完整复核，[运行记录](../artifacts/bid-full-sample/real-run-v16-review-resume1/progress-observation.json)持续更新。32项语义及完整同版DOCX/PDF/报告仍待验，先完成正确性与稳定性，速度优化后置。下方为预算调整与历史记录。

2026-09-12 UTC 前次实测记录：查询依赖与来源任务职责分离已实现，[工程验证](../artifacts/bid-full-sample/loop-repair/source-task-ownership/verification.json)通过252项库测试（30项忽略）、20项合同、严格Clippy、全局fmt及编译。[局部复核终态](../artifacts/bid-full-sample/loop-repair/source-task-ownership/fee-corrected-review-resume1/final-observation.json)为第68轮、累计69/80次调用、5轮复核、23记录/10关系，quality=needs_review；7项来源判断已完成，但末段组成要求缺少续页的1项发现仍未解决，生产编制校验正确拒绝。续跑619.19秒，加修复前243.58秒累计862.77秒（不含暂停），性能仍不合格。已观察到无业务写入的主Agent查询后再次复核往返，需验证完整分析摘要包含阅读回执是否干扰重复终止判断。当前无模型进程；旧费用80/80及全文第236轮998/1200保持终态。106页、32项语义与完整同版DOCX/PDF/报告仍待验。下方为历史记录。

2026-09-11 UTC 最新状态：局部导航、历史来源判断取回及固定任务清单复用已通过248项库、严格Clippy及全局fmt。同一检查点离线请求构造中位耗时约556→295毫秒、请求摘要不变；尚无新的模型完成率或全文性能验收。全文兼容接续后已在第236轮因HTTP500/503/503终止，当前没有真实模型进程运行；累计998次调用，原总额度剩202次，但该边界三次尝试已耗尽，未重置。保留33项来源判断及27项发现，另有2处执行阻塞。32项与完整DOCX/PDF/报告仍未完成；费用正文两组亦未完成复核，不能宣称性能可用。以[方案§5.4](bidding/agent-runtime-rig.md#54-性能指标和结论)及所链接证据为准，下方为历史运行记录。

2026-09-11 UTC 性能修复真实对照通过：同一七页目录、原7项候选和同.env配置，baseline为33次/239.30秒/10次工具错误，完整原文出处规则的provenance为8次/63.55秒/0次工具错误，两组均verified且生产结构审计通过、候选值不变。该局部对照墙钟减少73.44%，不代表完整106页性能。已合并有界边界证据，并将冻结原文读取/引文与实际候选依赖分开；实际候选查询、记录/映射/关系ID及同页原图排版依赖保留。241项库、严格Clippy和编译通过；同次导航复用清单，离线输出摘要一致。旧全文v3在146轮无在途边界暂停，累计759次；v4已使用原1200额度剩余441次启动，首请求合同与成功对照一致，保留416条记录/8条关系，未导入旧独立回执。32项和同版DOCX/PDF仍未完成。证据：artifacts/bid-full-sample/loop-repair/review-boundary-evidence/provenance-final.json、real-run-v15-review-v4/startup-verification.json。

2026-09-11 UTC 全文性能修复接入：已确认search_sources仅搜索冻结原文，却误记全局分析/发现依赖，导致同一第5页目录在第13–21、46–52、87–93轮重复派发。已删除该错误依赖；真正的候选查询、相关来源/关系/规则及输入摘要校验保持，239项库、严格Clippy和编译通过。旧real-run-v15-review-v2在第140轮无在途请求边界主动暂停，累计613次调用及416条记录/8条关系保留；real-run-v15-review-v3已启动，原1200次上限余额587次，沿用.env的grok-4.6＋Chat＋low及原上下文/输出预算，不导入旧独立回执。新首请求已核对相邻导航及同批交接合同。32项语义和同版DOCX/PDF/报告仍未完成，全文提速尚待验。证据：artifacts/bid-full-sample/loop-repair/source-query-dependencies/verification.json、artifacts/bid-full-sample/real-run-v15-review-v3/startup-verification.json。

2026-09-11 UTC 当前进展：read_review_task已合并当前原文和完整候选的读取，保留原预算及交付边界。同一页/同一11条初始候选对照中，原工具71次/652.93秒/5轮复核，合并读取14次/139.90秒/1轮复核；两组均needs_review且关系判断不同，不能宣称等质量提速。现已补同页多个模板的文字/网格原图检查（真实第84、95、96页暴露旧漏检），并从主写入schema派生复核可见的关系类型及说明，明确同页条件依赖仍须判断。236项库、20项合同及严格Clippy通过，关系合同同输入复验已完成34次/399.59秒/2轮复核：原11条记录不变，正确新增references关系并再次独立确认；仍有compliance=unknown待审项，不算完整验收。旧全文主提取完成416条记录/8条关系/148项来源处置，已在第473轮无在途请求边界暂停旧合同复核；新合同全文复核已启动，保留原成果并从原1200次上限扣除已用473次，余额727次，不复用旧独立回执。32项语义及同版DOCX/PDF/报告仍未完成。详见 artifacts/bid-full-sample/loop-repair/relationship-vocabulary/verification.json 和 artifacts/bid-full-sample/real-run-v15-review-v2/startup-verification.json。

2026-09-11 UTC 性能对照终态补充：同一第11页原格式27次调用/503.66秒，简短引用21次/410.91秒，墙钟缩短18.42%；新版主提取6次/模型162.54秒，独立复核15次/模型242.04秒。两组均完成一轮复核且生产结构审计通过，但新版quality=needs_review，原格式为verified，候选分类不同，不能宣称等质量提速或性能可用。新版仍有8次工具错误及重复读取/复核提交往返；下一步应缩减有界提取与复核的串行调用，保留独立证据和语义门槛。证据：artifacts/bid-full-sample/loop-repair/compact-evidence-references/final-comparison.json。全文旧合同运行继续，完整32项及DOCX/PDF验收仍未完成；本条取代下文“对照进行中/待终态”的状态。

2026-09-11 UTC 历史进展（以上方状态为准）：简短证据引用已实现并共用于提取/独立复核和编制/稿件复核，领域Span、持久化成果和阅读校验保持原合同语义；232项库、20项合同和严格Clippy通过，离线5轮参数缩减约25%–55%且展开结果逐值等于原参数。相同第11页来源、同.env、同预算的原格式/简短格式顺序对照已启动，真实提速及语义质量尚未证明；当前workspace格式检查仍有其他并行修改的排版差异。全文real-run-v15-resume2仍使用其归档程序及旧合同继续，第196轮接入正文/表格导航修复时保留241条记录、2条关系和196次累计调用；新工具/提示词合同不套回旧检查点。全文独立复核、32项语义、完整同版DOCX/PDF/报告尚未完成，部分模板整段待填可能删除固定文字仍须复核修正。沿用deploy/.env的grok-4.6＋Chat＋low，无模型配置调整或新migration，人工答案不发送。 详见[统一方案](bidding/agent-runtime-rig.md#55-本次修复的实施门槛)。

**2026-09-10 前序修复记录（当前状态见顶部）：本地功能验证通过，独立复核已完成局部比较，未完成最终提交，运行已停止；完整验收尚未通过。** 已分离来源权限与局部焦点，自动维护成果/未解决引用，主提取、独立复核、编制和稿件复核共用进展与有界恢复策略。默认连续无进展6轮、焦点24轮、重规划2次；记录局部执行阻塞后允许转向独立范围，连续6轮仍未交接则在工具提交边界停止。重启、重复读取、笔记改写和任务改名不能刷新额度；有效局部写入可以完成当前动作。执行失败单独保存并阻止最终发布，不冒充来源缺项。候选当前版本参与窗口保留；各 reviewer 保留自己的冻结原文回执，修改后的候选/稿件仍按摘要核查。旧手抄引用输入及编制 `remember` 路径已删除，未新增 migration，既有 baseline 同步检查点与预算 JSON。

验证：最新176项库测试（11项忽略）、20项合同、1项真实检查点离线提交诊断、严格 Clippy、workspace fmt及样稿编译通过；既有6项隔离 PostgreSQL结果保留，本次未改SQL。首次3来源短测在21轮停止，11条记录、0关系，发生一次超时后重试成功；第二次 v2 在32轮停止，29条记录、12条关系、2个来源处置，四组重点引用由真实Agent产生，但独立复核未开始。v2请求42214–353819字节，含必要原页图片，无503或超时。轨迹还暴露无响应要求被迫指定渠道、同类型合规属性的不同条件被拒绝、工作引用格式说明不足；均已修复并验证。未新增 migration，未修改实际 `.env` 或旧检查点，未执行暂存操作。包含全部修复的 `response-contract-trial` 已以新身份、空候选重测第11、16、17页的3处来源，模型与预算仍来自 `deploy/.env`；启动合同已核对，结果待验。该试验在34轮保留32条记录、21条关系后，进一步定位到重复读取会触发无实际缺口的 pending_delivery 阻塞。已删除这条冗余判断，尚未交付的原文/候选仍按真实缺口阻止交接；172项库测试、20项合同及Clippy/格式/编译通过，SQL未因本项调整。`response-contract-resume1` 已在第101轮由无进展/交接保护停止：主提取完成，41条记录、21条关系、3项来源处置；独立复核收到65个候选当前版本后仍重复读取，两次重规划无效，0轮复核、1项执行阻塞。已保留终态和计数，未将空缺口等同于语义通过。现补充复核专用完成指引与按实际缺口生成的下一动作：有问题逐项保存、无问题完成原范围后提交空草稿；执行阻塞仍禁止提交。173项库测试、20项合同及Clippy/格式通过，无新工具/配置/migration；提示词已改变，`reviewer-completion-trial` 已以新身份、空候选复测；启动核验确认实际配置、工具和预算未变，旧终态未改。该试验在第85轮到达诊断时限并取消：主提取41条记录、28条关系、3项来源处置；reviewer完成一个局部范围、收到72个候选当前版本，仍未提交最终结论。除精确读取位置反馈、局部复核排除不相关待办外，现新增 complete_review_check，在既有进展账本中记录当前候选的无问题比较；重复版本/改写结论不能续额度，来源、当前版本、执行阻塞及最终全局门槛保持不变。176项库测试、20项合同、3项.env启动测试及Clippy/格式/编译通过，无新表、migration、检查点字段或环境变量。新工具/提示词采用新身份：clean-review-trial 在补充明确授权后实跑656.16秒，停于第25轮：独立收到72个候选当前版本，完成41个记录版本的局部比较，46次重复比较被去重；28条关系和3项来源处置仍未形成比较结论，0轮最终复核。定位到当前焦点已完成但执行反馈仍要求比较该焦点，已增加焦点剩余数和转向未完成引用的派生反馈；176项库测试、20项合同、Clippy/格式/编译通过。clean-review-resume1 保留原检查点、预约正文及计数兼容续跑，在第40轮完成全部72个局部比较，但此后反复读取，0次完成范围、0次提交复核，第58轮进入执行阻塞，第64轮耗尽交接额度后停止。只读第54轮检查点副本的正式工具调用均通过，证明当时接口可用；离线副本不作为真实复核结果。最终结束行为仍未解决，本轮后续复杂附表及完整样稿重测未启动。此前自动审批拒绝已由用户补充明确授权解除。旧运行、原始来源和计数未改。复杂附表、完整106页独立复核、32项语义发现及完整 DOCX/PDF仍未通过。证据：[验证记录](../artifacts/bid-full-sample/loop-repair/verification.json)、[v2真实轨迹](../artifacts/bid-full-sample/loop-repair/source-scope-trial-v2/result.json)、[上一实跑启动核验](../artifacts/bid-full-sample/loop-repair/reviewer-completion-trial/startup-verification.json)。

2026-09-09 v8 历史记录：v8 已停止；离线回放确认候选查询与当前原文相互挤出上下文。已实现按当前范围查询、保留来源证据的分页，以及提取/编制共享 I/O 驱动；Rig 已接生产 Chat 请求序列化、流解析及 AgentRun 多轮步进，有界会话和三边界恢复已通过隔离验收；32项语义验收及真实整稿仍待完成。见[最新验证](../docs/bidding/agent-runtime-recovery-results.md#agentrun-多轮步进与有界检查点)。

v9 已在第45轮检查点停止：运行1043.91秒，34条记录、10个来源处置、0关系，独立复核未开始；第25–44轮仅查询已有候选，连续20轮没有新增成果或来源覆盖。后半段未观察到503，重复候选全文查询多次因无法与当前原文共存而失败。已保留全部请求及检查点，目录/详情取回修复已通过离线及隔离验收；不能视为真实语义或整稿验收通过。用户对 `.env` 目的地的具体外发授权持续有效。证据：`artifacts/bid-full-sample/real-run-v9/terminal.json`、`stagnation-report.json`。

v10 已停止：运行387.94秒，第24轮检查点保存7条记录、8个来源处置、7张已交付原页，0关系；第5轮后没有新增成果，反复读取12个活动来源。目录查询未再复现 v9 的候选共存错误，但完整持续产出仍失败。原文工具返回完整单元格；活动范围只能扩大不能拆小，未充分实现方案的分次处理要求，显式待处理来源与安全拆分已通过离线和隔离验收，真实持续产出尚待复验。证据：`artifacts/bid-full-sample/real-run-v10/terminal.json`、`diagnostic-checkpoint.json`。本轮未生成可验收的提取结果或整稿。

v11 已保存第48轮检查点后停止，耗时1421.51秒，35条记录、12个来源处置、0关系、0轮独立复核。期间仍有写入，不能归类为连续空转；停止原因是已复现旧索引挤掉原文网格，需以新合同验证修复。第33→34轮离线重放由仅保留2张网格改为保留全部4张，请求107508→95213字节，最新工具结果保持原样。库149项、合同20项、隔离 PostgreSQL 5项及 Clippy/格式通过，未调整预算或新增 migration。证据：`artifacts/bid-full-sample/real-run-v11/terminal.json`、`artifacts/bid-full-sample/navigation-history/verification.json`。

v12 已人工停止并保留第152轮检查点：耗时2975.47秒，54条记录、6条关系、20个来源处置，独立复核未开始；最后47个完成轮次没有新增记录，期间仍有读取和导航。导航裁剪改善了原文保留，但未解决完整持续产出。终态见 `artifacts/bid-full-sample/real-run-v12/terminal.json` 和 `diagnostic-checkpoint.json`。

v13 已获明确外发授权并启动，向 `https://ai.zleiwork.cn` 发送同一招标文件的解析文本、网格及必要原页图片，模型和预算只读 `deploy/.env`。归档程序包含候选详情回执及字段校验反馈；启动核对确认供应商、预算、二进制、冻结来源和新提示词/工具合同一致。真实持续提取、独立复核及完整 DOCX/PDF 尚未通过，32项发现保持开放。证据：`artifacts/bid-full-sample/real-run-v13/startup-verification.json`。第104–129轮状态保留在 `artifacts/bid-full-sample/real-run-v13-resume1/progress-snapshot.json`；第129轮后由兼容恢复目录 v13-resume2 续跑，新增15条记录后再次出现重复核查，现已停稳于第164轮，详见下方字节定位修复记录。

2026-09-10 补充：主提取及 reviewer 各自保存完整候选详情的已交付摘要，目录 `detail_received` 显示本角色是否收到当前版本；它不是语义通过标志，修改后失效，同批尚待交付及另一角色的回执不能冒充已收到。交接反馈明确保留已有引用不需重新取回全文。151项库测试、20项合同、5项隔离 PostgreSQL 测试及 Clippy/格式通过，无新增 migration；v12 已停止，新提示词/工具合同由 v13 真实复验。证据：`artifacts/bid-full-sample/candidate-receipts/verification.json`。

2026-09-10 字段校验反馈已补齐：复用原 `ok/error` 封装及语义校验器，以 `INVALID_FIELD <JSON Pointer>: <constraint>` 指明记录、引文、模板单元格及关系的失败位置和约束，失败写入保持原子性。反序列化错误定位到所属容器，不声称每个嵌套 Serde 错误都能定位叶子字段；原文未给出的单位等仍允许为空。153项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、严格 Clippy、workspace fmt 及样稿编译通过，无新增 migration。证据：`artifacts/bid-full-sample/validation-fields/verification.json`。

2026-09-10 原页历史淘汰修复：v13 第68→69轮确认超大旧图片组会先挤掉较早的完整网格，随后自身也被淘汰。现按冻结历史预算优先淘汰必然放不下的已交付图片组，保留较小原文组；离线回放从0张完整网格改为保留2张及相关正文，154项库测试、20项合同、5项隔离 PostgreSQL、Clippy/格式及样稿编译通过。最终修复未改变提示词、工具、来源或预算合同。v13 原实例停稳于第104轮后，以归档修复程序从同一检查点恢复，保留52条记录和105次累计调用；恢复记录见 `artifacts/bid-full-sample/real-run-v13-resume1/startup-verification.json`。证据：`artifacts/bid-full-sample/image-history/verification.json`。

2026-09-10 正文字节定位修复：`read_source` 在保留原文及起止范围的同时返回逐行 `line_spans`，中文与原换行均按真实 UTF-8 字节定位，完整结果按原工具预算分页；兼容恢复仅给已交付历史补充确定性位置，不改已预约请求和阅读回执。真实第120→121轮离线回放保留两页58行完整正文及附表，请求116767字节；157项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、Clippy/格式及样稿编译通过，无新增 migration。v13-resume1 停稳于第129轮，52条记录、0关系、20个来源处置、0轮独立复核，累计131次调用。原状态已逐字节复制到 v13-resume2，生产合同校验通过；自动审批首次拒绝后，用户针对具体目的地和载荷再次明确回复“继续,允许”，现已从第129轮恢复，原预约正文保持不变并累计第二次调用；启动核验见 `artifacts/bid-full-sample/real-run-v13-resume2/startup-verification.json`。第130轮实际请求已验证包含两页58个逐行位置；随后新增15条记录，达到67条。第136轮后连续28个完成轮次无新增记录、关系或来源处置，期间73次候选详情/目录查询、29次搜索，未尝试写入关系。原文持续保留，但候选详情在有界窗口内反复取回；该相关性尚不能单独证明模型循环的根因。恢复实例运行777.78秒后安全停于第164轮，保留67条记录、0关系、20个来源处置及167次累计调用；未进入独立复核。逐行位置已验证可用，整体持续提取仍未通过，不能继续以相同正文重试或放宽预算冒充修复。证据见 `artifacts/bid-full-sample/real-run-v13-resume2/repeated-inspection-observation.json`、`terminal.json` 和 `line-spans-request-verification.json`。证据：`artifacts/bid-full-sample/source-line-spans/verification.json`、`artifacts/bid-full-sample/real-run-v13-resume2/preflight-verification.json`。32项语义发现及完整 DOCX/PDF 验收仍开放。

2026-09-10 停滞进一步定位：四组简单引用的双方完整详情在7个真实请求中同时可见，生产关系工具的离线内存副本验证全部通过，单组详情仅1116–1288字节；不能再将零关系简单归因于窗口装不下。28轮内257份详情只有25个版本，工作笔记与17项缺口不变。当前缺口是局部任务推进和停滞恢复，候选历史保护不足会加重重复，但不是充分解释；模型内部选择原因仍不可由轨迹证明。详见[定位报告](../docs/bidding/agent-loop-diagnosis.md)。本次未修改生产逻辑或新增外发，修复方向尚待短范围真实验证。

## 授权与使用方式

用户已明确：「方案已经确认了,现在拆分任务来实现吧」。**方案已确认，普通实施与必要的隔离开发验证已授权**，不再把普通代码开发标成等待方案批准。采购/许可证承诺、生产部署、现有数据库/Compose/对象资料操作、清库、提交/push/PR 不在此次授权内；外部能力仍须满足真实前置。

本台账只管理跨域交付与证据，不替代技术合同，不新增产品范围或技术顺序：

- 产品范围：[PRD](../docs/bidding/prd.md)，文件 / 编制 / 导出，DOCX 唯一正式正文。共享解析（PDF/Word/Excel/OCR）见[DocReader 结构增强](knowledge-base/docreader-structured-parse.md)，不另建招标解析器。
- 编辑与出件：[ONLYOFFICE 契约](../docs/bidding/onlyoffice.md)、[领域接缝](../docs/bidding/authoring.md)；[接入计划](bidding/onlyoffice-integration.md)中的 O0 → O1 → O2 → O3 → O4 管编辑与出件，下列 O 阶段后缀只是阶段内切片。
- Agent 改造：[Rig 完整方案](bidding/agent-runtime-rig.md)，Rig P0–P4 管当前 Chat Completions 路径的提取/复核修复及运行/恢复，不替代下表平台 P0/P1/P2。
- 平台：[运行时基础](platform/runtime-foundation.md)、[队列](platform/queue-runtime.md)；清理唯一合同是[平台 §6](platform/runtime-foundation.md#6-retention-consumer)，不另建清理/调度/补偿框架、outbox、Request 或 artifact scanner。
- 首发：[部署说明](../deploy/README.md)；性能：不列入当前计划，禁止项见 [crate.md](../docs/knowledge-base/crate.md)。两条工作独立于 O0，不要求先重做平台；知识库其他产品计划不并入。

**当前执行补充：** 用户已授权继续后续任务并直接在 main 修改，且要求严格避免硬编码。P1 接受及 P2 分批进展见各节；P2 由已有 agent 继续；F1/F2 需求拆分及集合回归后，当前追加 Bidding 所属 DOCX 新轮后端、所属 baseline，以及 API 的 DOCX 子路由/显式 HTTP 测试和对应文档，不改 P2 的平台/队列文件与测试入口。O0 本地技术 PoC 已取得真实保存重开证据，字体/生产许可与模板外观仍待落实。阶段 A/B/C 的范围及审查记录保留为历史证据，不再把“本阶段只列出后续任务”作为普通后续开发阻断；共享工作区按文件划分，保留继承改动和 index。

候选文件面用于下一切片定位，不表示新 API、Schema 或文件已经存在，也不是批量写入许可。除下文 P1 精确范围外，下一切片启动前由父会话给出最小文件范围；未发布 Schema 可按所属 baseline 直接维护，但不能操作现有资料。需要新产品/架构决定时暂停裁决，不能隐式扩面。

状态区分：**代码已有 / 历史局部验证 / 待本轮复核 / 待实施 / 外部前置未齐 / 条件未触发 / 待独立审查与父验收**。worker 完成不等于父接受；implemented、locally verified、committed、pushed、deployed、runtime accepted 分别报告。下列验收均为应取得的证据，不能读成已通过。

## 当前优先任务：Rig 方案与真实整稿

2026-09-09：用户已批准完整方案，要求先保存并统一全局文档；[方案文档](bidding/agent-runtime-rig.md)已保存，提取/复核已部分实现、Journal 恢复已验证、Rig 接缝已验证，共享宿主驱动已接提取/编制，生产 Rig AgentRun 已接入并通过隔离验收。本节说明当前优先级，以下阶段 A/B/C、P1 首任务及旧运行启动记录保留为各批次历史，不覆盖最新安排。

| 顺序 | 当前工作 | 状态与衔接 |
| --- | --- | --- |
| Rig P0 | 精确查询、局部提取、有界窗口、待办导航 | 已实现有界窗口和进展保护；最新72个局部比较后仍未收尾，派生原文待办与版本状态已实现，新空候选短测进行中。 |
| Rig P1 | 附表/关系、字段反馈、双向独立复核及确定性收尾 | 原文结果、依赖失效、完整批次末汇总与空提交撤旧已实现，工程验证通过；空候选短测进行中，按 Rig §5.5 完成真实验收。 |
| Rig P2 | 当前协议真实提取及32项逐项复验 | 待真实验收；通过后交 O1-S 既有编制链。 |
| Rig P3 | 独立 Journal 的多边界检查点、预算与恢复 | 提取/编制三边界已实现；五项隔离 PostgreSQL 测试通过，预约/准备原子提交、完整响应恢复零额外模型调用、工具提交重放和预算/owner 校验通过，无新增 migration/表。 |
| Rig P4 | Rig 0.42.0 Chat Completions 最小接缝、共享驱动切换与撤旧 | Rig Chat 接缝已验证；共享宿主驱动及 Rig 生产 Chat 流解析已接提取/编制，两处重复外层循环已删除；18场景及五项隔离数据库回归通过；SDK 请求序列化已接入并通过恢复验证；AgentRun 已接入；145项库测试（含提取/编制真实宿主会话复用断言）、20项合同、18场景传输及五项隔离数据库恢复通过，完整真实语义与样稿仍待验收。 |

前轮 v6 在86轮、468次工具调用后人工停止；39条记录、0关系、0复核，连续57轮没有记录/关系/来源处置写入。请求有界仍出现重复查询和范围交接失败，32项验收不关闭。诊断与后续修复见[性能记录](../docs/bidding/extraction-performance.md#v6-有界请求后的实际停滞)。用户已确认解析模块完成并授权修复交接问题；本轮补齐 v3 网格搜索、样稿冻结脚本和每轮当前范围清单，验证记录见[解析交接](../docs/bidding/agent-runtime-recovery-results.md#解析-v3-交接与当前范围清单)。仍在变化的共享文件须按实际并行情况协调，不覆盖其他修改。

本轮实现与验证见[Journal / Rig 接缝记录](../docs/bidding/agent-runtime-recovery-results.md)：Bidding 库141 passed/5 ignored，20项 Schema/baseline 合同、五项隔离 PostgreSQL 与三项真实窗口离线回放通过，Bidding/DocParser 全目标严格 Clippy 和全仓 fmt 通过；这些结果不关闭32项真实语义发现，也不等于全仓验收。

本次风险复核已重排原阶段编号，删除双协议任务。Embedding 一致性归知识库独立事项；完整 DOCX/PDF 的项目目标继续由 O1-S/O2 验收，不能因运行时改造而重建现有编辑链。

历史真实 v5 已终止：22 分 23 秒、30 轮、200 次工具、33 次物理调用，27 条未复核记录、0 条关系、0 轮复核，最终 `AGENT_PROVIDER_UNAVAILABLE` / HTTP 503。不是仍在运行，也不证明供应商已恢复。32 项独立发现仍开放，完整真实 Agent DOCX/PDF 未验收；证据见[性能诊断](../docs/bidding/extraction-performance.md)及[样稿记录](../docs/bidding/full-sample-results.md)。外发已获用户明确授权，新运行仍只能读取 `deploy/.env`，不改旧检查点或重置已消耗预算。

S3 官方 SDK 替换已由 `s3_sdk` 完成，当前代码使用 `aws-sdk-s3 1.146.0`。2026-09-12 UTC 核对时，旧临时证据目录 `/tmp/kb-s3-sdk-52g29iye/` 已不存在，因此在当前工作区使用项目锁定 MinIO 镜像及独立临时对象目录重跑：4项真实 MinIO 测试各1 passed/0 ignored，平台库72 passed/3 ignored，平台全目标严格Clippy通过；专用容器已清理，源码摘要与测试时一致。[持久归档证据](../artifacts/s3-sdk-verification/verification.json)覆盖读写、命名空间隔离、只读预检、缓存回退及保留期删除。未重做SDK替换或触碰应用对象；该局部结果不等于全仓或Rig语义验收。

## 既有增量与证据基线

- 当前工作区：`main@34b118914d6e4a128b5738afe768893f45927242`；阶段 A 开始有 449 个 Git-visible 文件、228 项既有 staged 变动，全部是用户状态，禁止 stage/unstage/commit/reset/checkout/clean。以独立 baseline/before 比较本轮，不用 `git diff HEAD` 冒充本轮增量。阶段 A 仅更新七份已有文档的状态/链接并新增本文件，预期 **449 → 450，其他 442 文件的内容/hash/type/mode 不变，index 原样保留**。
- 共享诊断及执行保护继续保留；旧大纲专用部分已按最新授权删除，其诊断边界回归迁至新提取 Agent。涉及文件：`migrations/bidding_v2_baseline.sql`，`crates/bidding/tests/tender_analysis_postgres.rs`，`crates/bidding/tests/content_agent_run_postgres.rs`，`crates/bidding/tests/support/diagnostic_messages.rs`，`.github/workflows/ci.yml`，`scripts/fresh_schema_acceptance.sh`，`deploy/README.md`，`plans/platform/runtime-foundation.md`，`plans/platform/queue-runtime.md`。其中五条诊断 writer 已修 UTF-8 字节边界；不重做、不回退、不放宽既有 NULL 拒绝、错误码或 owner/attempt/token/lease/deadline 围栏。
- `crates/platform/src/db.rs` 已有单事务三 baseline、receipt/catalog 与 runtime 只读校验；旧 live 链、V20 六 stage 表已移除，V21 原七码、owner/stage fence 已修，不另列重建项目。fresh/catalog、Content/Outline 有此前隔离 PG16 的局部真实回归；`schema-contract` 已接线。CI skip guard 后续修正有 **41/41 完整 shell 控制流探针**，这是 mock cargo 的控制流证据，不是 41 个数据库或 hosted CI 测试。
- 历史证据保留于 `/tmp/knowledgebrain-diagnostic-ci.HIYtpX/`，以 `change-manifest.json`、`parent-final/verification.json`、`parent-final/green.log` 及原始 red/green、suite 日志区分批次；最终 CI hash 为 `f7b2fc69c021b788f56d2741089d069cbb890b6b9b5c46e156cb16c181de54ba`。先前失败探针仍保留，不把早期未修状态当作当前通过依据。最近文档审阅在 `/tmp/knowledgebrain-plan-clarity.wlFEsQ/`。这些本机证据不是永久发布资产；交付时由父会话归档，不删除或覆盖。
- 当前 `request_delivery_postgres.rs::non_agent_handler_timeout_codes_terminalize_requests_atomically` 清理块仅登记 `timeout cleanup` 字节，没有真实写 blob；`cleanup_pending()` 后立即断言 staging 不存在。既有失败缺 Oxana Redis，同时断言把交接误当同步删除。两者必须分开证明，不能只加 Redis 或改成相反断言宣称闭环。源码定位不等于本轮已复现失败。
- `retention/src/main.rs` 已有真实 `ObjectUploadExpireWorker` / `ObjectRetentionWorker`；upload-expire consumer 释放 staging 后可调用物理删除处理。已有 `api_style_dispatch_reaches_typed_oxana_retention_worker` 等测试可复用，不复制假 consumer。五分钟、每次上限 100 条的 staging expiry 业务处理沿既有合同，不扩为 transport scanner。
- 当前编制入口使用 `DocxEditor.tsx` / `DocxRound.tsx`。旧 Tiptap 编辑界面、大纲会话及其专用客户端已删除；固定 live-map 章节和附件映射已删除。解析、集合冻结、对象、Job 与新链使用的表格原语继续复用；旧大纲创建、阶段 SQL、队列注册和专用测试已撤除；当前仍被内容、证据及导出使用的块模型另行跟踪，不能据此声称全链清理或自动样稿验收完成。

本阶段证据目录为 `/tmp/knowledgebrain-confirmed-execution.zvtgzdwc/`：既有 `baseline.json`、`before/`、`index-before.txt`、`staged-before.diff` 不覆盖；新增 `plan-current.diff` 和文档验证器/逐项结果供阶段 B 复核。临时证据与交接不写仓库根。

## 当前工作区质量检查

针对 fmt/check/Clippy/test 报错的修复及原始日志见[质量检查记录](../docs/bidding/workspace-quality-results.md)：首批 Rust 1.97 的 fmt、全工作区/全目标/全特性 check 与 Clippy `-D warnings` 通过，默认 Rust 测试 586 passed/0 failed/9 ignored；Python 150 passed/13 skipped，前端 lint/build/test 通过（33 tests），10 份 Schema 正反例通过。后续本地对象 reset 只读前置后的完整 Rust 回归日志统计为 **604 passed/0 failed/15 ignored**，fmt、全工作区全目标/全特性 check 和严格 Clippy 通过，见[最新质量记录](../docs/bidding/workspace-quality-results.md)。生成至 Office 的同次合成产品验收以及 R3 queue-faults 原样 CI 命令的本地 4/4 验证也已完成；二者不替代真实招标语义或 hosted 验收。CI 显式启动已有 Python DocReader 执行必跑 gRPC 重放，本机合成和真实 DOCX/PDF 两组均 1/1。基础设施 guard 提前返回及 skipped/ignored 的部分不算完整集成验收。

已删除固定签名关键词/机械区间切分的提取修补脚本，保留原始真实 Agent 结果及独立拒绝结论；招标结构和附表对应仍须从源文件语义提取并复核。模型继续读取 `deploy/.env`，无新增 migration。本次基础检查不等于 O1 完整样稿验收；R1 后续真实启动及对象生命周期已通过，干净候选与正式验收仍待落实，见 R1。

## 任务总览与依赖

以下 18 项是既有跨域交付切片，Agent 改造的当前优先顺序见上节 Rig P0–P4；无工期、日期或虚构接口承诺。

| ID | 可交付切片 | 必要依赖 | 当前状态 |
| --- | --- | --- | --- |
| P0 | 保留增量与证据复核 | 现有 baseline、源码及历史证据 | 覆盖映射已独立审查并父接受；未重跑 diagnostic/hosted CI |
| P1 | 清理交接、真实 consumer、最终回收闭环（首代码任务） | P0、阶段 B ready、专用隔离资源 | 清理交接/真实 consumer 已父接受；readiness 见 P2 首片 6/6；hosted CI 改挂 R3 |
| P2 | 完整数据库/队列正确性 | P1 | 可执行半片已有限接受；当前 CI DB suite 本地关门已父接受（Content 7/7、Request 5/5、analysis 2）；hosted CI/`queue-faults` 改挂 R3 |
| F1 | 文件级复用与完整集合实证 | P0、隔离解析依赖及样本 | BiddingFile.pdf 当前 parser 无挂载 gRPC 已接受（kb-docreader:f1-parser）；compose 仍指向旧 local 镜像 |
| F2 | 来源/规范及新轮边界实证 | F1 | source_views 真 Python RPC 与 106 页核对索引已有证据；旧分析有 16 条结构无效记录、32 项 open 语义发现；外发已授权。v6 在86轮后人工停止；显式范围诊断修复后的 v7 在137轮后因持续空转停止，独立复核和整稿仍未验收；按 Rig P0–P2 修复并验收，真实整稿待验收 |
| O0 | 许可、版本、字体、真实样稿 PoC | 外部真实前置 | 真实编辑/保存/重开/同稿 PDF 已验证；字体/生产许可/外观待落实 |
| O1-S | 新轮 DOCX 初稿、存储版本与编辑会话 | O0、F2 | 显式新轮已接线；整本模板按分章 Agent+compiler（非 live_map）；合成来源的生成至 Office 保存重开同次验收通过；真实模型整稿待完成 |
| O1-C | 受控读取、签名回调与持久化安全 | O1-S | 后端契约/真实读写、缓存失效恢复及配置过期后的活动保存通过；无需独立续期机制 |
| O1-W | 编制前端真实编辑/保存/重开 | O1-C | 已接入已有 DOCX，真实 App 编辑/保存/下载/重开、账户/语言及登录态恢复通过；已补真实表格/图片修改回归，完整故障/版式验收待完成 |
| O2-S | 保存关联与冻结出件版本 | O1-W（O1 验收） | 精确保存回执前置修复已验证；冻结出件待实施 |
| O2-E | 同稿 DOCX/PDF 与独立报告 | O2-S | 产品导出待实施；现有隔离工具已验证同一保存 DOCX 的实际 PDF 转换/摘要不变，已补两页合成稿原生目录更新/保存/书签与实际 PDF 页核对，真实整稿仍待验收 |
| O3-A | 官方插件候选入稿与人工保护 | O2-E（O2 验收）、插件能力/许可实证 | 待实施，插件能力待验证 |
| O3-M | 报价附件实际入稿与页码安全出口 | O3-A | 待实施 |
| O4 | 端到端真实回归、切换与撤旧 | F1、F2、O3-M（O3 验收）、P2 | 旧大纲专用链已按授权删除；新链完整端到端验收待完成 |
| R1 | release descriptor、RepoDigest 与启动验证 | P2、干净候选和可核验镜像身份 | 入口、实际镜像构建及真实隔离启动/只读重放/对象生命周期通过；干净候选与正式验收待完成 |
| R2 | 受保护 namespace reset | 平台 namespace 合同、专用可销毁资源 | namespace 归属及对象/Redis 隔离、PostgreSQL 和本地目录身份的只读前置已验证；完整 mapping、drained、checkpoint 和 reset 入口待实施 |
| R3 | 首发 named required jobs 闭环 | P2、O4、R1、R2、发布所需外部前置 | schema-contract/queue-faults 已接线并有本地证据；其余岗位及 hosted 验证待完成 |
| V1 | 条件式 pgvector 分域基准 | 明确性能需求、代表性数据与 SLO | **不列入当前计划**；有 EXPLAIN/SLO 证据再开 |

P1 是后续第一个代码任务；P0 是证据复核，不重做已有实现。F1/F2 是复用面的实证任务而非预设重构。R1/R2/R3 和 V1 **不是 O0 或普通开发前置**；O0 不能因而免除版本/样稿/许可要求。跨域依赖只用于安全验收，不能构造第二套 ONLYOFFICE 顺序。O0 缺外部条件时可推进已授权平台切片和样稿准备，不擅启文档服务。

## 任务明细

### P0 — 保留增量与证据复核

- **目标 / 不做：** 形成现状、历史局部通过、已知失败和未运行项的覆盖映射；不重写五 writer、fresh/catalog 或已有 CI guard，不把复核变成全仓清理。
- **归属 / 候选文件面：** Shared Platform 与 Bidding；只读上节九文件、`crates/platform/src/db.rs`、已有日志及工作区基线。台账仅记事实。
- **输入：** 已确认三份平台/部署合同、历史 red/green 与最终 guard 证据、本轮独立 baseline。
- **依赖：** 无新代码依赖；沿用既有证据，不要求运行服务。
- **产出：** 带批次/文件/hash 的保留清单、P1 旧清理断言定位与未覆盖项，交独立计划审查。
- **可证实验收：** 九文件逐项对应历史证据；五 writer、NULL/非字符串 JSON 边界及原终态围栏未被台账改义；41 探针明确为 shell 控制流而非全 CI；本轮状态不冒充 hosted、提交或部署。
- **当前状态：** 阶段 A 只读定位后，关闭包已完成独立审查与父验收。证据 `/tmp/knowledgebrain-p0-closure.p0c1/`：`coverage-mapping.md`、`nine-files.json`；两路 fresh review 均为 `OK / No issues found`（contracts `15c4f8a0-d77d-4287-b8ff-04c30aefcf24`，evidence `d2937eff-1b0a-479f-81b3-61f710d589f3`）。父核继承 index 5/5 PASS、228 staged、HEAD 未变。诊断九文件历史绿绑定当时字节（绿 CI `f7b2fc69…`）；今日树仅 3 文件仍与诊断 after 相同。`outline_agent_run_postgres.rs` 已删、台账换成 `tender_analysis_postgres.rs`（集合替换）。41 探针是历史 shell 控制流记录，本轮未重跑。没有 hosted/提交/部署。父验收见该目录 `parent-acceptance/decision.md`。

### P1 — 对象清理交接与真实消费回归闭环

- **目标 / 不做：** 按[平台 §6](platform/runtime-foundation.md#6-retention-consumer)纠正交接与异步完成的错误验收，补齐真实消费结果；handler 在原绝对 deadline 内只确认交接，不同步删 blob、不等待物理回收。不新增 retry budget、扫描器、通用 wrapper、清理任务表或第二状态机。
- **归属 / 精确候选文件面：** `crates/platform/src/object_registry.rs`、`crates/platform/src/jobs.rs`、`crates/platform/src/queue_registry.rs`、`crates/retention/src/main.rs`、`crates/worker/src/runtime.rs`、`crates/bidding/tests/request_delivery_postgres.rs`、`.github/workflows/ci.yml`（仅 P1 真实依赖及必跑入口，保留旧 guard 和其他 job）；另可更新本台账的 P1 事实/证据。不是要求七个代码候选全改。
- **输入：** P0 定位、`StagedObjectCleanupTracker::cleanup_pending`、`schedule_object_upload_cleanup`、`dispatch_object_deletion`、`ObjectUploadExpireJob`、两个真实 retention worker 与现有回归；冻结 Request 错误码/终态及 owner-revision-digest 围栏。
- **依赖：** 阶段 B 独立计划复核 ready；新建 phase-C 快照保护阶段 A 与既有 index；本轮专用 PG16+pgvector、Oxana Redis、临时身份和实际本地 `OBJECT_DIR`，不连接现有 Compose 或资料。
- **产出：** 最小修复、真实 consumer 回归、原 Request 用例逐项覆盖映射、红/绿退出码与数据库/blob/receipt 证据、P1 必跑入口及隔离资源清理收据。
- **可证实验收：**
  1. 先用最窄可用 fixture 留下旧失败的真实退出码，区分缺 Redis 与错误同步删除语义；原 fixture 改用真实写入的测试 blob，不只登记字节。
  2. enqueue 未确认/失败/未知结果时保留 staging 与可恢复 identity，显式失败；仅官方 enqueue 的确认才 disarm tracker，`Skip` 不冒充 delivery receipt。确认交接不宣称 staging 或 blob 已删除。
  3. 真实 typed consumer 执行后证明 staging 释放；无引用应回收对象在有界期限内观测到 blob 消失、`deleted`、tombstone/receipt；有引用拒绝物理删除，重复消费幂等。沿 Oxana 原生 retry/resurrection 验证失败保留与恢复，不复制测试 consumer、不用直接 SQL 函数调用替代队列消费。
  4. 原 Request 各错误码、失败终态原子性、owner/revision/digest 围栏全部保留。必须调整归属时提交逐项覆盖映射与真实结果；不能只反转/删除 cleanup 断言、固定 sleep、skip 或削弱最终回收条件。
  5. `runtime.rs` / `bidding.rs` adapter 仍共享既定 absolute cleanup deadline，terminal write/tracker/取消与 join 不获新预算；失败/取消不遗留超期等待或 late write。
  6. CI 实际执行相关 consumer；缺 DSN/Redis/对象资源/role、ignored/skipped/filtered/零用例或 cleanup 失败必须失败。保留 `schema-contract`、Content/Outline/Request 三 suite guard、`--locked`，不趁机实现整套发布流水线。
  7. 仅使用已存在本地镜像创建本轮唯一 label/ID 的专用资源，不 pull/升级依赖，不读真实 `.env`/secret；创建前记录既有容器/卷/网络/镜像身份及端口（25433 须先空闲），绑定 127.0.0.1、`knowledgebrain_test_*`；退出仅清本轮登记且 label 匹配资源，零残留且既有资源身份不变。缺安全隔离条件即报告阻断。
- **当前状态：** P1 本切片已实施、完成独立审查并由父接受；完整服务启动与队列故障矩阵仍归 P2，不声明整体 CI 或应用通过。阶段 C 独立快照保护 450 文件，现为 451；继承 228 项 staged 未改变，未 stage/unstage/commit。父按源码批准最小扩面：新增 `crates/retention/src/lib.rs` 等价导出两个真实 consumer，`crates/bidding/Cargo.toml` 仅加 retention dev-dependency，`Cargo.lock` 仅增加这一依赖边；main 与 Request 测试共用同实现，未升级外部依赖。另父核对 pinned Oxana 2.1.3 的 Skip 返回既有 JobId，批准仅两个清理 job 不设 transport unique_id、使用原生独立消息 ID；业务身份、原生 retry/resurrection 与数据库幂等/引用围栏不变，两平台合同只同步该例外。没有新增调度/状态框架或 SQL 修改。
- **本轮证据：** `/tmp/knowledgebrain-confirmed-execution.zvtgzdwc/phase-C/` 保存 baseline、完整命令/真实退出码与日志、`coverage-mapping.md`、父逐文件 LSP、CI 控制流和资源清理收据。原 fixture 在配置缺失/Redis 前置失败时 exit 101；正确隔离 Redis 下原 `!staging_exists` 断言 exit 101，分开保留。修后完整 Request suite 4/4：真实 blob、绝对 deadline 内交接、生产同一 typed consumer 的 staging/最终 deleted/tombstone、两 pending 身份及顺序保留/取消后同身份重交接、持久业务 owner 引用保护、两类 native 重复消息幂等与真实文件系统失败后同一 job 原生 retry 恢复。初次 retry 观察 10s 与 Oxana 默认 dequeue 10s 竞争的失败仍保留；30s 有界观察后得到真实 retries=1 成功，不改原生预算。经父批准并在替代覆盖通过后，仅移除 worker 中一个基于同步 DB cleanup 假设的旧 tracker 测试，其余 supervisor/deadline/cancel/join 代码及原 Request 错误码/终态/fence 均保留。
- **P1 验收时的验证层次与残留（后续处理见 P2）：** 实际运行修改后的 CI delivery 完整 shell body，Content 7/7、Outline 4/4、Request 4/4 与必跑 guard 均通过；10/10 完整 shell 控制流探针仅证明 missing/zero/ignored/filtered/skip/consumer 证据/cleanup 失败非绿，不是额外业务测试。platform tracker 单测 1 个、worker library 的 5 个精确 deadline/cancel/terminal-reserve 测试及受影响四 crate `clippy --locked --all-targets -D warnings` 通过；最初误用 worker binary 的五次 0 tests 不计通过。完整 retention binary suite 为 **5 通过 / 1 失败（exit 101）**：raw baseline 测试库 receipt 表 0 行且未提供 runtime release identity，未改 readiness 断言，父裁决留待 P2 完整 migrator/receipt 环境复验，不称整个 retention 或服务启动已绿。没有完整进程崩溃/丢失 enqueue 响应故障矩阵、hosted CI、全应用、ONLYOFFICE 或部署验收。本轮两 owned 容器/对象目录已清理，原容器/卷/网络身份不变、零 owned 残留，cleanup exit 0；报告不等于父最终接受。

- **审查修复与父接受：** 两路首审指出共享 namespace 测试缺锁、fault server 失败路径缺少 stop/join；已在单一 Request 测试文件内修复，child 分支不取父锁，新增真实启动失败/child 101→父 unwind→join/socket 重绑回归。独立证据 `/tmp/knowledgebrain-p1-review-fix.rcef3388/`：最终默认并行与原串行入口均 **5/5、exit 0**，focused clippy/format 与父 LSP 通过；两路 fresh 复审均 `RESOLVED / No issues found / OK`。父复核本轮 451 文件仅一文件变化、450 路径保护及 228 staged 原样；累计阶段 C 为 12 个授权路径、439 继承文件不变。原生报告交付曾 `Request was aborted`，仅恢复交付成功，未覆盖原失败或重做测试。两轮 owned 资源均已清理。父接受限 P1 清理交接/真实消费者/回收及此次测试修复；验收时的完整 readiness、未归因 API 自动 1/15、完整 crash/持久 enqueue 丢 ACK 等残留移交 P2，后续进展见下节，不重写当时结果。父验收记录见该证据目录 `parent-acceptance/decision.md`。

### P2 — 完整数据库/队列正确性

- **目标 / 不做：** 在 P1 闭环后复跑完整数据库相关 suite 与平台/队列故障矩阵，区分完整 suite 和聚焦用例证据；不重做已有诊断修复，不以跳过用例获得绿色。hosted CI 与 named job `queue-faults` **不是本任务重点**，改挂 R3，不在 P2 剩余清单里追接线。
- **归属 / 候选文件面：** `crates/platform/tests/catalog_manifest_postgres.rs`、`crates/bidding/tests/` 既有 AgentRun/Request/检索 suite、`crates/worker/src/runtime.rs`、`crates/retention/src/main.rs`、`scripts/fresh_schema_acceptance.sh`、`.github/workflows/ci.yml`；发现缺陷才按父限定范围修改。
- **输入：** P1 覆盖映射、[平台 §7.1/§7.2](platform/runtime-foundation.md#71-聚焦数据库-gate)、[queue §10](platform/queue-runtime.md#10-验收)、现有 role/bootstrap 与专用 DSN/require flags。
- **依赖：** P1；所需外部依赖仅在已授权专用隔离环境配置，不借用现有运行库。
- **产出：** 逐 suite 真实执行/退出码、数据库和队列故障覆盖表、必要的最小修复。本地分轮证据即可关闭本切片；不要求 hosted workflow 或新增 `queue-faults` job。
- **可证实验收：** fresh→matching 只读重放→receipt/catalog/server/extensions/seed/ACL drift 拒绝，角色 allow/deny 及真实登录；五 writer 字节/NULL/错误码、AgentRun 锁后时钟、调用预算和原子终态；ObjectRegistry 引用竞态、queue 原生 retry/delay/reconnect/resurrection/dead revive、六 handler deadline、SIGINT/SIGTERM/fatal 与 helper cancel/kill/reap。完整 suite 不得 ignored/skipped/filtered/零执行；缺依赖与清理失败非绿。golden 每 kind/child owner/ACL/dependency 与生产应用启动分别记证据，不能用 shared verifier 冒充全应用启动。
- **当前状态：** P2 可执行半片已有限接受（readiness、API、批次1–3、Oxana native 故障、SIGINT/ObjectRegistry、catalog 负例、owned S3/Neo4j live）。队列只依赖 Oxana 原生 Skip/retry/resurrect/dead revive，不另做传输层。用户明确：CI 不是本任务重点；`queue-faults` named job 与 hosted 跑通改挂 R3，不挡 P2 关闭口径。Outline 条件 HTTP、真实模型/DOCX 仍属后续 O 线，不单开为 P2 剩余 CI 项。
- **首片证据与接受边界：** `/tmp/knowledgebrain-p2-readiness.tok1pF/` 保存精确授权、独立451文件 before、适配 runner、原始结果与两路审查；父验收见 `parent-acceptance/decision.md`。真实 fresh/matching、三 runtime 登录、九项 SQLSTATE42501 权限拒绝通过；receipt revision 与函数 volatility 漂移令 migrator 真实101且未 repair，精确恢复后整 receipt/catalog 不变。完整默认并行 retention 两次 **6/6、exit0**；五种身份错误和 opt-in 缺 DSN 均实际101，不算完整 suite。两路 fresh review 均 `No issues found / OK with notes`；父核一文件变更、其余450路径保护和228 staged原样。完整 runtime 对应前格式 hash；最终只对新增 assert 折行，父有逐字节等价证明及明确例外，最终 LSP/2024fmt/locked no-run 通过，未宣称最终 hash 重跑 runtime。
- **资源与剩余项：** 各片 owned PG/Redis/e2e/MinIO/Neo4j 容器已清；9 个 compose `knowledgebrain-*` 未作为本片资料。原 readiness `cleanup.exit=1` 与 API-full inventory exit1 仍保留，不单开任务。本任务不再把 hosted CI/`queue-faults` 列为剩余项。并行 writer 仍在共享 checkout。
- **API 精确诊断小片已接受：** `/tmp/knowledgebrain-p2-api-check.Cqr9uK/parent-acceptance/decision.md`。新产物真实清单14例；完整 `bid_v2_routes::tests::` 模块 **5 passed / 0 failed / 9 filtered、exit0**，只代表模块而非完整 API。fresh review 无问题，父核451文件零变化、228 staged/index原样及owned目录/进程组清理。历史自动1/15缺确切命令/失败名，仍未复原、未归因，不称已修；不为其单独无限扩测。首次rustup代理预备命令124且实际意外下载，原证据保留、owned下载已清理；父批准直调已安装工具链后offline no-run成功，不声称零网络或全局副作用已审计。
- **API 完整 lib 功能片已接受（附限制）：** `/tmp/knowledgebrain-p2-api-full.yKVyRT/parent-acceptance/decision.md`。真实默认并行 **14/0/0/0、exit0**；新库真实 baseline 函数生成 receipt，上传竞态与 Redis 拒连后同身份重放、两 document 各六 spans、原生队列计数有实证。fresh review 无问题；父复算归档/产物/18项证据并核451文件、228 staged保持，owned容器/目录/进程组回收。既有3容器22个端点/IP/MAC/Sandbox字段变化，原严格 inventory **exit1仍保留**；其它字段相等，但有界events为空，不能确定变化原因。父只接受功能验证与owned清理，不将其改称宿主全绿。admin与synthetic descriptor只用于fixture，不证明最小权限API启动、真实镜像身份、生产解析或后台回收；不是完整package/P2/hosted CI。
- **API extras 与剩余映射已接受：** `/tmp/knowledgebrain-p2-api-extras.SHVKEU/parent-acceptance/decision.md`；`no_pipeline` **1/1**、`probes` **2/2**，各完整目标无过滤、exit0；probes只help/bad-arg，不是HTTP健康验收。fresh review无问题，父复算源归档fullmode/hash、两产物hash及451/index保护与owned清理。`remaining-P2.md` 提出三批后续计划：①schema/SQL/AgentRun/检索，②完整平台/worker/knowledge runtime，③缺失故障与CI入口收敛。API各目标是分轮证据，不宣称一次完整package执行。
- **批次1（schema/SQL/AgentRun/检索）已有限接受：** `/tmp/knowledgebrain-p2-db-batch1.FsAUMx/parent-final-acceptance/decision.md`。两路 fresh review `OK with notes`、无 P0/P1。exact 真实 PNG+mapping 三 source/3-hit 与 typed PolicyRevoked/DigestMismatch；attestation pin 版本软删后真实 blocker/SCOPE。actual index 228 未变。fresh 薄 wrapper SIGKILL 为 valid non-blocker，禁止直接复用。Outline 条件 HTTP、真实模型/hosted CI/完整 P2 不在本接受内。
- **批次2（platform/worker/knowledge runtime）已有限接受：** `/tmp/knowledgebrain-p2-db-batch2.k7Qm2A/parent-acceptance/decision.md`。两路 fresh review `OK with notes`、无 P0/P1。分轮 platform 61（S3 live NOT RUN）、worker-lib 49、knowledge 233（原 227/6 保留）、render 1、launch 3（原 2/1 保留）。neo4j live 内部 skip；launch 超时 reap 未修。不是完整 P2/hosted CI。
- **批次3（CI cleanup + scripted-content e2e）已有限接受：** `/tmp/knowledgebrain-p2-db-batch3.p3k1/parent-acceptance/decision.md`。两路 fresh review `OK with notes`。cleanup `content-e2e-cleanup-test-ok`；包装 e2e `bidding-v2-content-stack-e2e-ok` exit0，`--pull=never`/offline，compose 9 ID 未动。scripted-content/synthetic descriptor，不是真实模型/DOCX。snapshot clone/search/launch 用 index 字节而非批次2 工作区哈希，不为此重跑。`queue-faults` named job 改挂 R3，不作为 P2 剩余。
- **剩余故障最小片已有限接受：** `/tmp/knowledgebrain-p2-faults.n4q1/parent-acceptance/decision.md`。两路 fresh review `OK with notes`。新 `oxana_native_faults` 原 3/1 红保留，修观察后 4/0/0/0。Oxana 公开 API only。用户后续明确：不扩队列补偿/ACK 传输层，enqueue 回放以 Skip+resurrection 为准。约束见 `parent-queue-native-constraint.md`。
- **SIGINT/超时 reap + ObjectRegistry 并发已有限接受：** `/tmp/knowledgebrain-p2-signals-registry.s2r1/parent-acceptance/decision.md`。两路 review `OK with notes`；P1（probe 口、并发首次 INSERT）测试内修复后重跑 launch 5/5、registry 1/1。不是完整 P2/queue-faults CI。
- **catalog server/extensions 负例已有限接受：** `/tmp/knowledgebrain-p2-schema-negatives.sn1/parent-acceptance/decision.md`。两路 review `OK with notes`。收据 `postgres_server_version_num+1` 与 `extensions=[]` 必须 verify 失败后 rollback；catalog 1/1。生产 db.rs 未改。
- **owned S3/Neo4j live 已有限接受：** `/tmp/knowledgebrain-p2-s3-neo4j-live.sn2/parent-acceptance/decision.md`。两路 review `OK with notes`/`OK`。REQUIRE=1 下 `s3::tests` 2/2、`graph::neo4j::tests` 2/2，live 非 skip；未借用 compose MinIO/Neo4j。不是 hosted CI。
- **当前 CI DB suite 本地关门已父接受：** `/tmp/knowledgebrain-p2-local-gate.p2lg1/parent-acceptance/decision.md`。两路 fresh review `OK_WITH_NOTES`。隔离 1.97 target：Content **7/7**、Request **5/5**、`analysis_publication`/`analysis_diagnostics` 各 1（另 2 ignore 过滤，含未跑 `source_views`）。phase1 fixture 在 freeze 后显式 `publish_disposition_set` 再编译；生产 freeze 仍为 unresolved。Request 创建者 assert 7→6 对齐已删 Outline。attempt1–3 红保留。owned 资源已清。不是完整 P2/hosted CI。

### F1 — 文件级复用与完整集合实证

- **目标 / 不做：** 证明只处理需处理文件、汇总全部纳入文件，真实缺口才最小补齐；不推倒 DocReader/解析、不引入旧稿局部重编或新解析产品。
- **归属 / 候选文件面：** Bidding；`crates/bidding/src/tender_upload.rs`、`tender_process.rs`、`bid_authoring_v2.rs`、`crates/api/src/bid_v2_routes.rs` 及既有文件/解析测试。
- **输入：** [PRD §1.2](../docs/bidding/prd.md#12-输入)、可追溯样本 A/B/补遗、已有 `freeze_document_set_v2` 与文件级转换/解析身份。
- **依赖：** P0、专用隔离解析服务/样本前置；不得假定全部外部服务已可用。
- **产出：** 场景调用次数/冻结身份/集合成员对照、失败可见性证据与必要的最小修补。
- **可证实验收：** A → A+B → A+B+补遗记录真实处理身份及转换/OCR/抽取次数；未变且有可用结果的 A 不重跑，B/补遗才处理；变更或尚无可用结果可处理/重试。项目汇总含当前完整集合而非仅差量；pending/failed/unresolved 明示，不抹掉成功结果，不阻止编制。仅改投标正文不触发招标解析。
- **当前状态：** 独立 SQL fixture 五轮通过：A → A+B → A+B+补遗，随后加入 pending 成员并转 failed；旧来源 ID/摘要复用、完整集合、warning、冻结编译输入与幂等重放均有数据库实证。**convert 前短路已有限接受：** `/tmp/knowledgebrain-f1-skip-reconvert.f1a1/parent-acceptance/decision.md`。成功 frozen 结果不再重入 converter/vision；`tender_document_process_v2` 5/5。**真实 gRPC 计数已有限接受：** `/tmp/knowledgebrain-f1-real-parse.f1b1/parent-acceptance/decision.md`。B=`testdata/bid/BiddingFile.pdf`；旧镜像 101 保留（page_table column edges）；当前 parser 28 表合法，挂载源码后 A 二次不增量、B convert_count=2。未放宽解码器；生产 `knowledgebrain-docreader:local` 未重建。Vision 仍 mock。**BiddingFile.pdf 全量覆盖已有限接受：** `/tmp/knowledgebrain-f1-full-coverage.f1c1/parent-acceptance/decision.md`。pytest 13/13：106 页 SECTION、无 table_extraction_error、命名表含 tech_spec_cont、每页 4-gram≥0.95 全局≥0.99；超限按表跳过。超时 4 红保留。生产镜像未重建。**命名表 TABLE_REGION 已有限接受：** `/tmp/knowledgebrain-f1-table-canonical.f1d1/parent-acceptance/decision.md`。pytest 14/14；价格表/偏差表/tech_spec_cont 等为独立表格。**canonical P2 已接受：** `/tmp/knowledgebrain-f1-table-canonical.f1d1/parent-acceptance/p2-fix.md`。pytest 20/20：阅读序不写回 cells、per-cell 坐标、markdown leftover+GFM、无整页 Exception 吞表。生产镜像未重建。**无挂载 gRPC 已接受：** `/tmp/knowledgebrain-f1-parse-complete.f1e1/parent-acceptance/decision.md`。`kb-docreader:f1-parser` 无 bind-mount，BiddingFile.pdf 计次 1/1，pytest 20/20。compose/`knowledgebrain-docreader:local` 仍为旧 parser。真实 OCR 模型未验收。可重跑入口及边界见[回归记录](../docs/bidding/source-collection-regression.md)。

### F2 — 来源、要求/规范及新轮边界实证

- **目标 / 不做：** 核对现有来源、disposition、要求/表格规范与新轮冻结能支撑整稿；不把需求清单改为固定五章节、不做旧稿影响分析/自动合并。
- **归属 / 候选文件面：** `crates/bidding/src/requirement_compile.rs`、`bid_authoring_v2.rs`、`workspace.rs`、既有来源/要求测试及所属 baseline（仅后续确证缺口且获最小范围后）。DOCX 初稿交 O1-S，不在此实现第二编辑链。
- **输入：** F1 全集样本、[PRD §1.3–§1.6](../docs/bidding/prd.md#13-需要提取的招标内容)、[领域来源合同](../docs/bidding/authoring.md#2-来源文件集合与要求)。已有 `publish_disposition_set_v2`、`compile_requirement_input_v3` 是检查入口，不声称已覆盖所有产品类型。
- **依赖：** F1。
- **产出：** PRD 字段/业务类别到当前持久化结果的覆盖矩阵、缺口实证/最小补齐、供 O1-S 使用的新轮冻结输入。
- **可证实验收：** SourceUnit revision 在 disposition 恰好一次，无遗漏/重复/孤立/越界；要求及规范带来源文件/章节/页码。资格/商务/技术/价格/人员、公共数据、目录/签章/暗标/提交规则、固定表行列/合并/证明名称与页码规则逐项有结果或明确待确认。补遗适用范围、冲突/撤回保留依据，不按时间“最后胜出”。集合变化开启新轮，不自动继承旧响应、检查或证明页码；旧稿可追溯、基础材料可重新匹配；招标图片不当作投标方证明。
- **当前状态：** 已确证并修复四处“少于 8 字丢弃”造成的短要求遗漏，保留所有非空片段及原容量约束，不新增业务关键词或固定章节。完整 requirement_compile 测试模块修前 3/4、修后 7/0（通过/失败）；SQL 验证 ready 来源恰好覆盖、旧冻结输入保持，以及遗漏/重复/外来来源拒绝。PRD 类别/页码及 Workspace 新轮隔离仍待实证，不称 F2 完整完成。见[回归记录](../docs/bidding/source-collection-regression.md)。
- **source_views 真 Python RPC 已父接受：** `/tmp/knowledgebrain-f2-source-views.sv1/parent-acceptance/decision.md`。两路 review `OK`。owned `kb-docreader:f1-parser` 无挂载，`source_views_cross_real_python_rpc_and_publish_frozen_pixels` **1 passed / 0 failed / 2 filtered**。现网 compose 未动。真实提取 `quality=needs_review`、41 记录/10 关系/148 disposition、0 findings；编制仍待显式授权，不得把空 findings 当成 verified。
- **后续增量：** 混合输入的空来源曾被静默记为已覆盖，已复现并改为含来源 ID 的技术错误，完整模块现为 **8/8、exit 0**。SQL 新增最新轮先发布、四旧轮迟到与重放，current 不回退、历史结果保留、非空人工 Workspace 不改写，最终 exit 0 且专属资源清理 exit 0；首次 PG 初始化连接失败仍保留。真实 DOCX 结构提取有 30 单元、486 非空正文段落文本/顺序及表/行计数实证。现有显式 projection apply 会复制旧节点/块/binding/quote，不能充当新轮隔离；新轮 DOCX 与不继承旧响应/检查/页码仍交 F2/O1-S 接缝实现。证据及限制见上述回归记录后续切片。

### O0 — 许可、版本、字体与真实样稿 PoC

- **目标 / 不做：** 按[接入计划 O0](bidding/onlyoffice-integration.md#3-o0--授权版本与真实样稿-poc)取得可保存的真实 Word 实证；不以 iframe/截图代替，不默认免费无限制，不作采购决定。
- **归属 / 候选文件面：** Bidding/部署接入；现有 `deploy/` 配置入口与真实样稿证据；仅在具体测试环境、版本和部署权限明确后落最小配置。
- **输入：** 真实可用 DOCX、ONLYOFFICE 版本、本体嵌入/并发/API 许可依据、固定中文字体及浏览器/API/文档服务/对象读取网络条件。
- **依赖：** 外部前置有记录；不依赖 R1/R2/R3、V1 或 P2 全绿。不具备启动前置时只准备样稿/验收步骤。
- **产出：** 服务版本/字体/样稿摘要、人工编辑后保存重开 DOCX 与同一文件 PDF、格式差异和许可核实记录。
- **可证实验收：** 合并单元格、非等宽列、重复表头、页眉页脚、中文字体、图片附件页、指定模板/固定表实际保存重开及 PDF 对照；关键不可保真项明确而不直接切换。普通编辑/回调/初稿/转换不以额外 Automation API 采购为前置，实际本体许可与版本能力仍须核实。
- **当前状态：** 用户提供 `testdata/bid` 真实稿；官方 Community 9.4.0-129 本地 PoC 已验证实际编辑、保存重开和同稿 PDF，真实稿全表结构保持，补充稿长表/图片通过。专属服务资源已清理；原字体缺失/回退、拟采用产品的嵌入并发许可及最终模板外观尚未验收，因此完整 O0 未完成；该批次尚未接入 O1，后续接线进展见 O1-S/C/W。见[O0 结果与证据](../docs/bidding/onlyoffice-o0-results.md)。

- **F2 条件适用及真实样稿复核增量：** 来源明确且经独立复核的条件要求/规则/模板不再被等同于未知；DOCX 中保留条件说明和完整模板，未知、不适用和缺来源仍受校验。投标模块 **75/75**、分章 **10/10** 与定向 Clippy 通过，无新 migration。真实文件首轮复核指出附表遗漏，主 Agent 已补正并进入第二轮独立复核；整本 Agent 样稿仍未验收。第 22–23 页组成条款、第 74 页格式目录和第 13 页平台分区要求分别核对，不能相互替代；第 25 页诉讼仲裁材料纳入验收范围。详见[样稿记录](../docs/bidding/full-sample-results.md)。

**F2/V4 本轮补充：** 集合 SQL 验收已迁到实际 V4 claim/reserve/checkpoint/publish，删除无调用方的 V3 loader/publisher 和授权。真实回归发现并修复迟到分析占用判定版本、导致下一轮连续 CAS 失败：迟到结果只保留原冻结身份和分析历史，只有当前输入推进判定。五轮集合、同集合两次判定替换、旧结果先完成、两轮 DOCX、重放/权限/非空旧稿保护通过；脚本模型数据库回归 2/2、模块 78/78、baseline 合同 17/17。无新 migration、表或业务硬编码。此项不代表真实模型完整样稿已验收；编制外发仍待明确授权，Office 人工基准本地复验 1/1 通过：52 张表、单元格/图片编辑、保存重开、首稿/新轮创建、同请求重试及四次离线回调重放，资源清理通过；不替代 Agent 全稿与同版本 PDF 验收。见[样稿记录](../docs/bidding/full-sample-results.md)。

### O1-S — 新轮 DOCX 初稿、存储版本与编辑会话

- **F2 核心方案已确认，实施中：** 招标内容提取与附表关系按 PRD 落实主 Agent＋独立复核；完整来源读取、五类产物、跨条款/附件/表格多对多关系、适用性依据、独立核查及真实 DOCX 承载分别验收。具体合同和实施顺序见接入计划的「F2 核心」。当前单次请求原型与关键词/默认强制判断不作为已完成成果；真实模型和最终文档验收取得证据前不得标记完成。

- **F2 Agent 核心审查增量：** 已接统一 Python docreader 的冻结文本/网格，补齐可并存的响应属性、带依据的指标与证明条件、多个响应位置及逐项结果复核，取消新发布路径默认正文响应。投标模块单元测试 **53/53**（其中 Agent 核心 **11/11**）；独立临时 PG16 三 baseline 与 Agent 发布/重投/owner隔离/跨重试预算 **1/1**；真实 PDF 经 Python service 的关键来源锚点 **1/1**；API/worker check 通过，worker 有旧路径未使用告警。没有新增 migration 文件；Shared baseline 仅补新系统发布身份登记。尚未配置真实模型验收参数；原页回看、更多行业样稿、字段/章节到 DOCX 的实际映射、界面和旧单次原型撤除仍待完成。详见[逐步审查与证据](../docs/bidding/tender-analysis-review.md)，不代表完整 F2/O1-S 已验收。

- **F2 原页证据增量：** 已通过统一 Python docreader 接入冻结 PDF 物理页和上传图片回看，主 Agent 与独立复核分别查看同一份图片，视觉引用与原件摘要随分析版本保存；越权、摘要不符、回看失败或预算不足不能判为已验证。复用已有原件读取、认证、渲染器与 checkpoint，无新表或 migration 文件。Rust 投标模块 **57/57**、docparser **46/46**、Python 原页与来源锚点 **11/11**、独立临时 PG16＋真实认证 Python RPC **2/2** 通过；真实样稿第70物理页已人工核对表格及表后说明。上述模型测试使用脚本，真实模型验收尚未运行；Office 物理页、局部放大、完整章节/附表到 DOCX 落位、界面和旧单次原型撤除仍待完成。此前“原页回看待完成”为较早切片状态，详见[审查记录](../docs/bidding/tender-analysis-review.md)。

- **F2 字段关系增量：** 已补模板区域/单元格、响应/证明/指标项的精确关系端点；相同附件名不作身份，多单元格区域不能冒充单值字段，修改端点记录使旧关系及独立复核失效。投标模块 **63/63**（Agent **21/21**），临时 PG16＋认证 Python RPC **2/2** 通过，包含关系发布、分页读取及重投；API/worker check 通过。已直接删除旧单次 LLM 模板路由、调用、专用 schema 和 SQL 函数/授权，保留确定性 DOCX 原语；无新表/列/migration。之前“旧单次原型撤除待完成”为历史状态。当前仍缺真实模型验收、关系界面、持久化分章生成及整本 DOCX 实际落位，不代表 F2/O1-S 已完成。详见[实现审查](../docs/bidding/tender-analysis-review.md)。

- **F2 分章 DOCX 核心增量：** 已实现独立分章 Agent＋独立文档复核核心，消费冻结分析版本，按章增量保存、编译完整模板/拟制响应表及留白，校验实际 DOCX 章节、原文、网格、合并、表后说明、签章和字段位置。长表单元格分页核查；修改使旧文件/复核失效，确认丢失可恢复，预算/取消不伪成功；条件模板的未选用须明确保存原文依据，不能把关系自动全当 AND。原网格宽度由页面设置校验，待填示例值不复制。投标模块 **72/72**（分章 **9/9**）、定向 Clippy `-D warnings`、API/worker check 通过；合成两章两表 DOCX 经统一 Python service 回读及独立包核对通过。无新表/列/migration；`Journal` 仍待接生产 PostgreSQL owner/lease，worker 任务、新轮发布、前端及真实模型/ONLYOFFICE 全稿验收未完成。测试文件与边界见[分章实现记录](../docs/bidding/docx-composition.md)。本项不代表完整 F2/O1-S 已验收。

- **分章生产依据接缝：** 已从已发布 V4 分析/收据接回原冻结来源，新增编制请求的可序列化身份合同及摘要校验；绑定 Workspace、当前文件/分析版本、预期 DOCX 和显式模型契约，不重复缓存原页或读取旧投标正文。runtime API 权限下验证越权、身份错配、过期版本、契约篡改以及新文件轮次后的精确历史恢复，临时 PG16 合计 3/3。未新增表/列/migration；完整编制请求入队、生产 Journal、文件/manifest 原子发布和前端生成入口仍未完成，不能据此标记 O1-S 完成。见[编制接线](../docs/bidding/docx-composition.md#生产接线编制依据与恢复合同)。

- **分章持久化接缝：** 已增加一张专用编制请求身份表，绑定原提取请求、文件/分析及预期 DOCX；复用已有 Request、AgentRun、调用记录和检查点，无新 migration 文件。生产 PgJournal 已验证完整脚本编制/独立复核、确认丢失恢复、旧 owner 拒绝、相同 checkpoint 重放及跨 attempt 调用预算。真实 PG 新用例 1/1，其他三项来源/提取/诊断回归通过。HTTP、Rust 队列注册、worker heartbeat/失败处理、DOCX+manifest 原子发布及前端仍待接线；请求持久化不等于已入队，不标 O1-S 完成。必要性与边界见[持久化编制](../docs/bidding/docx-composition.md#持久化编制请求与-journal)。

- **分章文件发布接缝：** 已复用新轮 CAS、对象归属及收据，将已独立复核 DOCX 与规范清单、初始版本和 Request/AgentRun 完成状态原子发布；无新增表/列/migration。提交前重建核对实际文件，重试只读验证两对象；第二对象失败完整回滚、生成期间人工保存/来源更新拒绝覆盖，清单只绑定原版本。真实临时 PG 5/5（含四项共享回归）、78 项模块、17 项合同及定向 Clippy 通过，资源已清理。HTTP/队列/worker 写入清理编排、前端和真实模型整本验收仍未完成，不标 O1-S/F2 完成；见[原子发布](../docs/bidding/docx-composition.md#已复核-docx-与清单原子发布)。

- **分章队列/worker 接线：** 已增加专用 typed job 和核心 worker 注册，复用原 Oxana 唯一身份/重试、执行心跳、检查点与原子发布；每次执行验证完整投递 scope。对象写入使用原平台 blob 接口及现有子进程取消/回收边界，失败 staging 由原 tracker 交接 RetentionQueue；无新表/列/migration。PG/Redis 6/6 与追加取消后恢复 CAS 用例通过；实际 worker helper 2/2、deadline 1/1、payload 3/3、registry 4/4、78 项模块、17 项合同及定向 Clippy 通过，资源已清理。真实 Redis envelope 后直接调用脚本模型 executor，尚未完成 HTTP/真实 consumer/provider/前端整链或最终物理回收验收；不标 F2/O1-S 完成。见[执行接线](../docs/bidding/docx-composition.md#编制队列与-worker-执行)。

- **分章 HTTP/前端接线：** 已接编制依据、提交、latest/指定进度 API 与“按招标要求生成”入口，保留已有 DOCX 导入。HTTP 重放先查原意图收据，不重取新配置/current；前端以原依据/key 恢复未知提交，后台任务重开不再 POST，空初始化要求集合不可生成。专属 PG/Redis 真实 API 1/1（脚本真实 DOCX 夹具 1/1）、前端 29/29、实际浏览器模拟 API 3/3、78 项模块/17 项合同及定向 Clippy/ESLint/build 通过，资源已清理。无新表/列/migration；编制预算沿用真实样稿既有 limits 写入 deploy/.env 新字段，模型配置未改。完整 consumer/provider/Office 整链与真实高质量验收仍未完成，不能标 F2/O1-S 完成；见[生成入口](../docs/bidding/docx-composition.md#生成-http-与前端入口)。

- **实际 API/consumer/provider 接缝：** 正式 migrator 空白建库与 API/worker 就绪、真实 HTTP 提交、Oxana 消费、30 次本地 SSE 工具调用、文件 helper、两轮 DOCX/manifest 发布、中途重启、重放/历史/越权及对象摘要核对通过，资源清理通过。联调发现正常上传追加合同使整表 frozen seed 指纹变化，最小修复为通用显式 baseline 主键选择及建库完整性检查；初始合同修改/缺失仍拒绝，无新表/列/migration，不改维护门或队列调度。平台 62/62、定向 Clippy/构建通过；全部集成目标 Clippy 有继承测试八参数告警。本批是合成来源/脚本模型的实际传输联调，真实招标 Agent 整稿、PDF 与浏览器/Office 联合验收仍待完成，不标 F2/O1-S 完成。见[执行证据](../docs/bidding/docx-composition.md#实际-apiconsumer-与-http-模型联调)。

- **浏览器生成至 Office 同次验收：** 复用已有产品浏览器/表格校验工具，完成 3 轮真实 API/consumer 编制（45 次本机合成 SSE 工具调用）；第三轮由完整 App 发起，下载后实际编辑、保存、关闭重开，再改单元格保存。新 editor key、保存版本/摘要、原生成结果不变与其余表格内容/结构通过核对。修复验收读取旧对象路径，改用平台返回的定位，不复制 namespace 规则；无产品业务改动或 migration。构建、fmt/check/全目标全特性严格 Clippy、数据库夹具 1/1 与表格校验器 2/2 通过，最终运行/清理通过。详见[同次产品验收](../docs/bidding/docx-composition.md#浏览器生成至实际-office-保存的同次验收)。这只完成合成来源的产品接线证据，真实招标完整 Agent 稿/同版本 PDF 与高质量语义验收仍缺，不标 O1-S/F2 完成。

- **真实稿独立复核红色证据：** 对统一 Python 冻结来源作重点逐条复核，首轮列出14项发现与12个模板文本区域重叠（R08–R10已在后续全量要求交叉检查中更正，见下方指定格式复核）；8G/8H原文含义、2A跨页注释、技术阈值及评分条件均有遗漏/错配，原161轮提取的空 findings 不足以通过验收。新增通用文本区间互斥校验，历史已审结果不能跳过；离线 audit 无模型调用，当前真实结果预期退出1并列出12个 invalid_record。80项模块及定向 Clippy/build通过，无新 migration。下一步优先修提取粒度、完整模板/关系与逐段应答承载，再在授权环境用 deploy/.env 重跑真实模型，保留旧结果为失败证据。完整106页、Agent整稿、Office和同版本PDF仍未验收。见[逐项复核](../docs/bidding/real-tender-acceptance-review.md)。

- **逐段原文＋待填响应承载：** 已新增来源片段内容块，原文仅从冻结文本字节区间/网格锚点读取，可显式拼接跨页条款；响应留白独立落位，不能绕过指定附表，来源映射必须逐项复核。83项模块测试、定向 Clippy，以及真实第59–61页跨页原文的 DOCX 原语验证通过。验证件与精确文本证据在 `artifacts/bid-full-sample/source-response-carrier/`；无新 migration、模型配置或上传解析实现。R14 的结构承载已落地，但汇总提取仍需修正，旧真实结果继续拒绝，未标真实整稿/Office/PDF验收完成。

- **要求/指标/证明的精确网格引用：** `read_form` 已返回实际锚点引用，要求证据可直接定位冻结表格中的单元格；核对归属、行列与独立阅读，拒绝混用图片/文本坐标。发布保留完整引用，网格依据不伪造文本引文，逐段编制须同时匹配要求与 response 的具体单元格。86项模块、17项 baseline 合同、定向 Clippy/build及真实第59–61页引用/原文承载验证通过；无新 migration、配置或解析器。记录粒度与完整性仍需通过真实模型重新提取和逐项复核证明，原结果保持拒绝。见[证据结构](../docs/bidding/tender-analysis-review.md#精确网格证据与逐段要求)。

- **真实指定格式逐页复核：** 第74–106页47个来源单元、14个网格/187个非空格已核对；另查看8张原图，形成24项格式预期。新增附件6日期归属、8A固定字段提示、8D/8E标题与表体顺序、签署区示例值及附件8续页问题；同时纠正首轮R08–R10误报，已有技术/业绩加分和报价公式予以保留，修复重点包括对应评分来源。修订共19项发现，原提取与旧审计留存。通用提示词补充多表顺序、字段提示和跨记录复核，无编号规则、解析器或migration。仍须真实重新提取及整稿/Office/PDF验收。见[完整格式清单](../docs/bidding/prescribed-format-acceptance.md)。

- **目录前内容及真实封面：** 编制支持显式、来源驱动的前置内容，封面不进入正文目录；校验实际书签顺序及原生目录域。真实第73页封面九格/三处空白与目录位置通过定向原语验证，89项模块、17项baseline及Clippy通过。旧提取封面缺口另记R20；实际Agent整稿和Office封面版式仍待验收。见[封面验证](../docs/bidding/docx-composition.md#招标指定封面与目录顺序验证)。

- **专用技术部分逐项复核：** 第54–70页31来源单元、14网格/140非空格及6张原图已核对，形成65项来源预期和R21–R25五项open发现；75处引用、10处原记录观测及原图摘要验证通过。已提取的硬件/服务关键数值保留，补查网络与编排复合条件、环境和服务细项、期限起算依据。Agent及独立复核提示补充相邻标记格/合并表头、实际阅读顺序及不得补造期限起点，无行业字典或migration。89项模块通过；真实重新提取、通用技术部分及全部106页/整稿/Office/PDF验收仍未完成。见[技术清单](../docs/bidding/technical-requirements-acceptance.md)。

- **混合单元格局部留白：** 提取区域可显式指定原格UTF-8范围，既有read_form支持精确子串定位，不由模型手算中文偏移或用行业关键词识别。编制只删除已审范围，保留固定提示/签章；同格重复策略、未读/越界/重叠范围、表头和合并覆盖格误删拒绝。94项模块及17项baseline通过；合成两格四个示例值清除、真实8A两表61锚点/19空值保留均有OOXML证据。真实8A不含混合示例值，原提取R16仍需修复，不据此关闭整稿验收；无新migration。见[实现及证据边界](../docs/bidding/docx-composition.md#混合单元格的局部留白)。

- **模板区域的字段落位修复：** 修正网格区域绑定只有整表书签、没有单元格坐标的问题。现按已审区域逐个记录实际锚点，单格区域与单元格别名定位一致，多格区域不混入邻格或合并覆盖格。编制工具说明纳入冻结契约摘要，旧契约不可静默恢复。模块 96 passed/1 ignored、定向全目标/全特性 Clippy、fmt 和10份 Schema 检查通过；红色回归保留。未增加业务规则、数据库/migration或解析器；原真实提取及32项拒绝发现不变，R12仍需真实关系修复和整稿验收。见[精确落位](../docs/bidding/docx-composition.md#网格区域的精确落位)。

- **目标 / 不做：** 用当前完整冻结输入单向生成包含全部章节和章内待填写表格的整本 DOCX 模板，关联项目/Workspace/新轮/文件版本与编辑会话；复用对象、幂等、CAS，不建设通用版本平台或 DOCX↔块双向同步。具体内容自动填充后置，不作为本期模板生成的验收前置。
- **归属 / 候选文件面：** `crates/bidding/src/workspace.rs`、`bid_authoring_v2.rs`、`render_v2.rs` 及既有持久化边界；API 配置入口在 `crates/api/src/bid_v2_routes.rs`。需要 Schema 时只按已确认所属 baseline 原则和后续限定范围处理。
- **输入：** O0 可用版本/key 生命周期、F2 新轮输入、已有不可变对象与 current pointer、PRD 固定目录/表格优先规则。
- **依赖：** O0、F2。
- **产出：** 可核验初稿对象/摘要/版本与会话绑定、必要持久化与配置接口实现、对应回归；具体 API/schema 在实施读码后落位，不在台账伪造。
- **可证实验收：** 固定目录优先、表格列/合并及模板可编辑，不强制通用骨架或人员专章；缺材料可生成空章/待确认稿而不造事实。`document.key` 与每次保存版本分离，关闭/重开/forcesave/新轮符合官方生命周期；新轮不复用旧会话或旧响应/页码。并发 CAS、重复请求、对象 staging→owner 与历史身份可核验；已编辑正式稿不从旧树重建。
- **用户明确的本期模板范围：** 交付一份可直接打开继续填写的整本 DOCX，包含本项目投标文件全部章节、层级、顺序和目录，以及各章应填写的表格；表格放在所属章节，落实表头、固定文字、列宽和合并关系，投标方待填写区域保留可编辑空位。指定格式中的声明、说明和签署位置属于模板结构，不因推迟内容填充而一并删除。章节与表格从当前完整招标文件集及其明确要求确定，指定目录/格式优先，不套固定五章/六节点，不按业务关键词硬编码章节。没有明确指定格式时，依据本项目要求组织可确认的完整结构；来源不足或不确定之处标记待确认，不虚构规定。
- **分期验收与后置内容：** 本期核对完整章节清单和章内表格清单到生成文件的逐项对应，真实 DOCX 可编辑、保存重开后层级/表格结构保持，空位可人工填写并再次保存；只有空目录、若干孤立表格或上传原文件不算生成完成。企业资料、人员、业绩、技术响应、报价数值等具体内容的自动填充及证明材料自动嵌入可放下一期，分别沿用 O3-A/O3-M 的候选确认和实际入稿约束；缺内容不阻止生成模板，也不代表已响应或可直接投标。此分期不降低保存、权限、对象完整性及新轮隔离要求，不将尚未生成模板标为完成。
- **当前状态：** 已实现后端新轮持久化接缝：独立 round/version/current，冻结当前完整文件/要求身份，校验 owner、当前基线与版本 CAS；原子 ObjectRegistry 转移、幂等/audit、当前和历史读取。新轮不读取/复制旧 Workspace 响应、绑定、报价、检查或页码。DOCX 字节模块 3/3、上传模块 5/5，SQL 两轮/权限/历史及真实双连接 CAS、冻结并发通过，owned 资源已清理。输入为已经生成或明确选择的 DOCX；已补 multipart 新轮创建、当前/历史元数据及真实 DOCX 下载接口，发布前回读核对 bytes，完成请求只读重放不重写文件。API 模块 12/12、真实 runtime API 登录的 PG/Redis/本地文件路由级 HTTP 契约 1/1、baseline 契约 21/21 与 API/HTTP 目标 clippy 通过，专属资源已清理；覆盖越权、重试/历史/CAS、真实写入失败与 cleanup 入队、损坏/缺失下载。该后端接缝批次未包含初稿自动生成、编制前端或编辑会话；后续接线见 O1-C/W，不代表完整 O1-S/O1、API 可执行文件启动或 S3 已验收。详见[新轮接缝与验证](../docs/bidding/docx-rounds.md)。旧 Outline/块/renderer 仅可复用初稿原语，活跃旧入口保留至 O4。


- **会话持久化必要性复核：** 用户要求新增 migration 先确认必要性。已有 DOCX round/version/current 保留；本轮仅支持打开/替代的会话表及分配 API 试写已撤回，生产代码/SQL 回到本轮开始状态，未操作数据库。稳定 key 需要跨请求关联，但不等于必须另建表；先验证 forcesave/最终保存/无修改重连/乱序的生命周期，优先评估复用 current/version/idempotency/audit，再按实际读写需求确定最小结构。禁止以预留字段/接口替代真实接入，详见[持久化复核](../docs/bidding/docx-rounds.md#持久化必要性复核编辑会话增量暂不落表)。本记录不增加普通开发的人工批准前置。


- **生命周期协议实测：** 新增显式隔离工具 `scripts/onlyoffice_lifecycle_probe.py`；真实样稿两次 forcesave 的不同 `userdata`/DOCX、同 key 继续编辑后的最终保存、新 key 打开最终文件、旧签名回调主动重投和无修改 `4 → 1` 重开均有文件/事件证据，run/cleanup exit 0。签名有效的旧回调仍指向旧 bytes，不能当成“最新”。本轮未增改 schema、未接产品 callback；不是自然乱序、断网恢复、产品并发分配或完整 O1 验收。具体约束见[生命周期实测](../docs/bidding/onlyoffice-lifecycle-results.md)，下一实现先评估现有 current 的最小活动关联与既有版本/幂等/audit 的原子联动。


- **编辑/保存后端接线：** 实测后复用现有 `bid_docx_current`，仅补活动 key、打开基线、pending 保存 ID 和错误状态；未新增会话表或 migration 文件。签名编辑配置、用途受限 source、后端关联 forcesave、双重校验 callback 和原子版本发布已有实际调用方；最终保存/新轮清除活动关联，旧回执不复活 key。配置均显式提供，未操作现有数据库。最终路由级 HTTP 1/1、API 模块 12/12、baseline 契约 21/21、API/HTTP clippy 通过，回调写盘失败及最终保存并发有实证，专属资源已清理。字段必要性、API、验证和技术限制见[编辑后端记录](../docs/bidding/docx-editor.md)。原“会话未接线”记录为较早切片，不代表此次增量的最终状态；完整 O1-S/C/W 仍未验收。

- **显式 DOCX 新轮入口：** 已接编制页首次发布和安全关闭编辑器后的新轮入口，沿用 multipart、不可变对象、幂等和 CAS；补当前冻结依据的只读 API，空要求集可读取身份，依据不匹配不可发布。200/null 才判定尚无稿件，查询错误不切换写入器；不确定响应保留原文件/依据/版本/key 重试，409 刷新后须重新确认。无修改关闭可以保留重连 key，入口不将 key 存在误判为仍在编辑，只阻止待确认保存；发布使旧轮会话失效。仅在所属 baseline 新增只读函数及 API 授权，无新表/列/migration 文件。前端 **51/51**、build/lint、HTTP **1/1**、baseline **21/21**、API 模块 **12/12** 和定向 Clippy 通过。完整 App/真实文档服务 native **1/1、65.66 秒**，首次上传丢失成功响应后的原请求确认、关闭后第二轮/新 key、既有保存历史/停机回调及 403/404/503 保护通过，所有专属资源已清理。详见[新轮前端与验证](../docs/bidding/docx-rounds.md#前端选择-docx-创建新轮)。此增量是明确选择的文件发布；完整冻结输入驱动的自动初稿生成、固定目录/模板优先和 F2 整链仍待实现，完整 O1-S 未完成。

### O1-C — 受控读取、签名回调与持久化安全

- **目标 / 不做：** 实现保存回调的权限与持久化闭环，不把任意通知/下载成功当保存，不接受任意 URL 抓取。
- **归属 / 候选文件面：** `crates/api/src/bid_v2_routes.rs`、`crates/bidding/src/bid_authoring_v2.rs`、现有对象/配置/安全测试接缝。
- **输入：** O1-S 会话/版本关联、O0 回调协议与网络信任范围、[ONLYOFFICE §4/§5](../docs/bidding/onlyoffice.md#4-最小接入职责)。
- **依赖：** O1-S。
- **产出：** 受控读取与签名配置、回调处理及真实服务回归，保存关联/幂等/乱序策略的明确实现与证据。
- **可证实验收：** 用户/项目/新轮/文件/用途越权及伪造签名拒绝；secret 不入前端/日志。状态 2/6 按保存语义处理，3/7 明确失败，其他通知不伪保存；重复不重复发布，乱序、旧会话、旧轮不回滚当前稿，不能按到达时间猜最新。主机/协议/重定向/范围、超时/大小预算有效；损坏 DOCX、摘要不符或存储失败不推进 ready、不返回伪成功。
- **当前状态：** 后端已接线，权限、签名、下载范围、幂等/乱序及写盘失败等已有独立 HTTP 契约证据；真实 Document Server 经产品 TCP 路由完成两次 forcesave、最终保存、新 key 重开再编辑和历史 bytes 核验。后续已补[已完成回调的缓存失效恢复](../docs/bidding/docx-editor.md#已完成回调的缓存失效恢复)：复用原幂等回执保存已验证通知摘要，重复通知直接核验已有文件；仅调整现有两个 baseline 函数，无新表/列/函数签名/migration 文件。再次复核后取消“令牌续期”独立任务：打开配置 TTL 与活动地址用途绑定分开，source/callback 均要求服务逐次有效签名，状态由既有 key/owner/基线和完成回执约束，无新增数据库修改。HTTP 1/1、API 模块 12/12、baseline 21/21、clippy 通过；真实 native 目标 1/1，打开配置过期后继续编辑保存及停机后四条真实签名回调主动重投均成功，所有 run/cleanup exit 0。详见[真实产品联调](../docs/bidding/onlyoffice-product-results.md)。首次保存缓存不可用、不确定命令 pending 与完整故障验收仍有边界；编制前端的后续进展见 O1-W，初稿生成仍待完成，不标完整 O1 完成。

### O1-W — 编制前端真实编辑、保存与重开

- **目标 / 不做：** 接入真实编辑配置与保存反馈，让用户仍按三步操作；不只是嵌入 iframe，也不把 DOCX 回转成 Tiptap 主存。
- **归属 / 候选文件面：** `web/src/bid/Workbench.tsx`、`api/docx.ts`、`authoring/DocxEditor.tsx`、`docxSession.ts`、`web/src/hash.ts`、`Shell.tsx` 及前端/真实浏览器测试。
- **输入：** O1-C 已验读写回调链、O0 样稿。
- **依赖：** O1-C。
- **产出：** 可打开/编辑/保存/关闭重开的编制入口、版本/保存中/失败反馈及真实浏览器回归。
- **可证实验收：** 修改段落/表格/图片后重开恢复真实 bytes 中的内容；保存中与我方持久化成功明确区分，刷新/失败/迟到通知不报假 ready。密钥仅后端签发，不能跨项目换稿；业务缺料允许编辑。旧入口在切换前受控保留，不形成同一正式稿双主写入。
- **当前状态：** 已有正式 DOCX 的项目进入独立编制页，使用真实配置、保存回执、关闭/重开和指定保存版本下载，保留三步导航；明确不存在 DOCX 才使用旧入口，权限/服务查询失败不切换写入器。同步完成不报已保存，不确定发送复用原请求身份，后续编辑和旧轮/迟到响应不误确认；未保存时拦截步骤、链接、退出、hash 跳转，离开浏览器有提示。前端测试 **42/42**、build 与定向 ESLint 通过；真实产品组件在 StrictMode 下完成两次按钮保存/下载、关闭最终保存、新 key 重开再编辑、干净关闭/重开、历史 bytes 和停机四条回调重投，native **1/1**、run/cleanup exit 0；另有 403/503 查询故障注入验证无旧编辑器回退。实测先暴露并修复直接 hash 跳转先卸载编辑器的问题，失败证据保留。详见[产品前端实测](../docs/bidding/onlyoffice-product-results.md#产品编制前端实测)。无本轮数据库/baseline/migration 变更或令牌续期功能。后续已接真实账户邮箱及应用语言，配置由后端签名；移除测试启动入口，改由完整 App 调用真实 me 恢复登录态。新增非法参数/伪造账户拒绝和签名覆盖契约，HTTP **1/1**、API 模块 **12/12**、Clippy、前端 **42/42**/build/lint 通过；完整 App 真实保存链 native **1/1**、run/cleanup exit 0，中文菜单及姓名提示消失已截图核对，详见[账户与语言实测](../docs/bidding/onlyoffice-product-results.md#账户语言与完整-app-登录态恢复)。无新表列或 migration。后续已补[表格/图片实测](../docs/bidding/onlyoffice-product-results.md#表格与图片的真实编辑回归)：真实页面改单元格、文件选择插图及高级设置改尺寸，两次保存、最终保存和新 key 重开再编辑后，四张表的被检结构/文字、原图片、新图片引用/摘要与尺寸均保持预期；native **1/1**，run/cleanup exit 0。此次仅变更测试工具与文档，原稿、baseline 和 index 不变。下一实施回到 O1-S 初稿生成及前端新轮入口；账号密码登录、完整故障与最终版式验收仍待完成，完成并复核 O1-S/C/W 后才进入 O2。

O1真实稿验收补充：已核对物理第43–53页通用技术及评标程序，新增47项人工预期与R26–R28（标准版本/优先范围、条件性材料、引用目标）；53处引文、原41条记录/10条关系与4张图片摘要通过完整性核对。已补充Agent通用指引，无业务字典、新表列或migration。详见[通用技术验收补充](../docs/bidding/general-requirements-acceptance.md)。原真实提取仍不通过，完整Agent DOCX与同稿PDF仍待完成。

O1来源核对现已形成[106页索引](../artifacts/bid-full-sample/acceptance-index.json)：前42页新增134项预期及R29–R32，55来源/13网格/308非空格与5张前附表原图已核对；引用完整性检查通过。累计32项发现保持open，原提取仍rejected，尚不能进入整稿已验收状态。详见[投标须知及评标核对](../docs/bidding/instructions-requirements-acceptance.md)。

O1 模板校验补充：删除自动给空格分配 bidder_blank、添加“无内容填 /”并重绑复核摘要的修补程序；在现有提取校验中提前拒绝同表 regions 不连续及仅有截图的非网格正文，交由 Agent 依据原文处理。三项回归、fmt/check/严格 Clippy 通过，完整 Rust 测试 598 passed、0 failed、15 ignored。原始真实分析只读检查仍有 16 条结构无效记录，32 项语义发现未关闭；原文件摘要未变，未生成或验收完整 Agent DOCX/PDF。继续聚焦原文补正→独立复核→整稿生成；不扩展新框架或 migration。详见[模板校验记录](../docs/bidding/full-sample-results.md#模板校验与空格策略补充)。

O1 真实运行恢复补充：共享异步传输已修复“完整 SSE `[DONE]` 到达后仍等待 HTTP EOF”的可复现误报，11 项传输/解析回归及 fmt/check/严格 Clippy/完整默认 Rust 测试通过（606/0/16）。按 `deploy/.env`、同一 Python 冻结输入和原预算启动新 v5 完整提取；不重置 v4 已耗尽的边界或导入旧分析。旧超时原因尚未证实，32 项独立发现仍开放，真实完整 DOCX/PDF 尚未验收。见[样稿当前记录](../docs/bidding/full-sample-results.md)。

O1 整稿验收工具补充：现有办公探针增加只更新目录、保存重开及同版本 PDF 的无标记整理模式，正文文字保留检查及合成稿实际服务 1/1 通过；原完整编辑/历史/停服回调回归 1/1 通过，临时服务清理零错误。该模式用于之后输出可审阅文件，不代替真实招标语义、版式或 O2 产品导出验收；详情见[办公结果](../docs/bidding/onlyoffice-product-results.md#2026-09-09-只更新目录的-docxpdf-出件验证)。

### O2-S — 保存关联与冻结出件版本

- **目标 / 不做：** 当前稿导出等待正确保存并冻结具体版本；不把 forcesave 请求接受当作保存完成，不隐式回退历史稿。
- **归属 / 候选文件面：** `crates/api/src/bid_v2_routes.rs`、`crates/bidding/src/bid_authoring_v2.rs`、`crates/worker/src/bidding.rs`、`web/src/bid/authoring/ExportPane.tsx`、`session.ts`。
- **输入：** O1 会话/持久化关联、既有 frozen Request/导出历史/幂等与 CAS。
- **依赖：** O1-W（O1 验收）。
- **产出：** 保存→正确回调→冻结出件 identity 的实现及并发/失败回归。
- **可证实验收：** 有未保存编辑时等待或明确失败；正确保存关联后才冻结。保存失败/冲突不输出“最新稿”；持续编辑不改变已冻结输出，界面说明版本边界。历史下载仅可显式选择，同 key 重放保留同一冻结 identity，不能读 live 输入改写历史。
- **当前状态：** 冻结出件待实施；已完成保存关联前置修复。前端改为核对本次 `editor_key + save_id` 的不可变入稿回执，其他保存产生的新版本不能误报本次已保存。复用既有幂等回执/版本产物，仅增加 fresh baseline 只读函数，无新表列或 migration 文件。前端 **33/33**、build/定向 ESLint、真实本地 PG + HTTP 替身契约 **1/1**、baseline **17/17** 通过，专属环境清理成功；详见[精确保存确认](../docs/bidding/docx-editor.md#本次保存的精确确认)。这不是 O1 完整 Agent 样稿验收，也未完成冻结请求或 PDF 转换。

### O2-E — 同稿 DOCX/PDF 与独立报告

- **目标 / 不做：** DOCX 直接取冻结文件、PDF 从同一文件经 ONLYOFFICE 转换，独立报告绑定输出；不从 ContentBlock 独立重排正式 PDF、不自动拆分文件。
- **归属 / 候选文件面：** `crates/bidding/src/render_v2.rs`、`bid_authoring_v2.rs`、`crates/worker/src/bidding.rs`、`crates/worker/src/helpers.rs`、`crates/api/src/bid_v2_routes.rs`、`web/src/bid/authoring/ExportPane.tsx` 及既有导出/报告测试。
- **输入：** O2-S 冻结版本、O0 Conversion 能力、既有 JSON 报告与对象 owner 引用。
- **依赖：** O2-S。
- **产出：** 同稿两格式及独立历史报告获取入口、技术失败和版本一致性证据；基础整本能力在本项核验，O3-M 继续补自动入稿/页码细节。
- **可证实验收：** 实际 DOCX/PDF 内容与同一保存摘要/版本关联，转换失败不回退旧自研正式链；历史输出/报告不随当前编辑改变。报价、价格及附件属于同一完整稿，缺料在报告明示仍可出件；报告不混入正文，保留单独提交要求及用户拆分/拆后自行复核提示，不宣称整本已满足分册提交。
- **当前状态：** 待实施；全部 O2 证据通过后进入 O3。

### O3-A — 官方插件候选入稿与人工保护

- **目标 / 不做：** 优先验证编辑器内官方插件展示候选、人工确认后定点插入并保存；不后台覆盖全文，不绕过跨域 DOM/私有协议，不默认插件等于 Connector。
- **归属 / 候选文件面：** `crates/bidding/src/content_runtime.rs`、`bid_authoring_v2.rs`、`web/src/bid/authoring/CandidateReview.tsx`、`session.ts` 及后续按真实插件能力限定的新接缝；复用知识检索/候选/AgentRun。
- **输入：** O2 实证、官方插件 API 与所用版本/许可、冻结证据/生成基线/目标/用户决定。
- **依赖：** O2-E；插件能力和产品嵌入许可有证据。仅选用外部 Connector 时才另核 Docs Developer + Automation 附加授权，未选择不作全部 AI 前置。
- **产出：** 公开插件候选流程、目标/版本/人工冲突保护与入稿后持久化证明，能力不满足则明确缺口供父裁决。
- **可证实验收：** `executeMethod` / `PasteHtml` 等公开能力真实验证，不凭简单插入宣称复杂表保真；过期版本、缺失/重复锚点及未保存人工修改不误插/覆盖。部分接受/拒绝、重复决定不重复插入，保存回调后才称入稿。事实需投标侧证据、拟议响应可待确认，模型不签发权限或路径；只改正文不重跑招标解析。
- **当前状态：** 待实施，具体插件能力与许可未实证。

### O3-M — 报价附件实际入稿与页码安全出口

- **目标 / 不做：** 将报价/证明真实内容写入整本，并对实际排版验证页码、有限轮回填与最终检查；不以快照/对象引用/Conversion 成功冒充已入稿或页码正确。
- **归属 / 候选文件面：** `crates/bidding/src/quote_snapshot.rs`、`render_v2.rs`、`content_runtime.rs`、`bid_authoring_v2.rs`、`crates/worker/src/bidding.rs`、`web/src/bid/authoring/ExportPane.tsx` 及实际文档回归。
- **输入：** O3-A 获准写入能力、O2 保存/冻结/转换/报告链、固定表规范、QuoteSnapshot、真实证据资产与锚点。
- **依赖：** O3-A。
- **产出：** 确定性报价表、实际附件页、证明定位/回填、最终文件检查与安全降级证据。
- **可证实验收：** Decimal/定点金额逐行舍入再求和、明细/税额/合计与实际 DOCX/PDF 核对；更新报价不后台覆盖人工稿。证据内容实际嵌入，招标附件不冒充我方证明，损坏的已选附件为技术失败。长表换行、多页证明、锚点丢失/重复、不收敛场景经过真实页验证；回填后保存/转换/有限轮重检，最终 DOCX/PDF/页码/报告同版本。不稳定数字须清除或降级且验证输出无伪确定引用，不能安全出件则失败。最终检查区分未满足、待确认与技术失败，业务风险不变 Gate。
- **当前状态：** 待实施，自动页码映射能力未获证；必须有安全出口，不能用文档选型代替结果。

### O4 — 真实端到端回归、切换与撤旧

- **目标 / 不做：** 按当前授权直接删除废弃的大纲设计及其专用实现、界面和契约，正式入口统一使用 DOCX；保留新链实际使用的共享原语，端到端验收单独跟踪。
- **归属 / 候选文件面：** 接入计划 §2 所列 Bidding/API/Worker/Web 面、运行注册/权限/Schema/真实调用方、`docs/bidding/backend-runbook.md` 与相关操作文档；逐项核对当前消费者，删除旧业务专用路径，保留共享能力。
- **输入：** F1/F2、O0–O3 样稿与回归、P2 平台正确性结果、旧入口/调用方清单。
- **依赖：** O3-M（O3 验收）、F1/F2、P2；不把 R1/R2 或 V1 变为新开发前置。
- **产出：** 可复现的新链端到端证据、旧路径替代映射、最小切换/删除 diff、更新后的操作手册。
- **可证实验收：** 同一项目走上传→要求/规范→DOCX 初稿→人工编辑→AI 候选确认→保存→整本 DOCX/PDF+独立报告；复测 A→A+B→A+B+补遗与新轮，未变文件复用、旧稿可追溯。缺料/高风险可编制出件，保存/转换/损坏/不可信页码不假成功。源码与运行调用证明旧 Tiptap 主写入、自研正式 PDF 及隐式块出件路径已撤；有真实初稿/候选消费者的中间原语保留，未替换的解析/权限/身份回归通过。
- **当前状态：** 旧大纲前后端生成、阶段 SQL/队列注册、专用 Schema/测试已撤除；新提取复用的执行/检查点/调用表按当前职责命名，诊断回归迁至新 Agent。专属空库三 baseline 应用及新 Agent 数据库回归通过，详见[样稿与撤旧记录](../docs/bidding/full-sample-results.md)。完整自动样稿、DOCX 内容填充及出件整链仍未验收；当前共享块模型的实际消费者后续收敛。未提交、修改业务数据或部署。

### R1 — release descriptor、RepoDigest 与启动验证

- **目标 / 不做：** 落实已确认首发 identity 与启动 gate，不另建发布框架，不以 development synthetic digest 或 `latest` 作生产身份。
- **归属 / 候选文件面：** `deploy/` release/Compose 入口、`crates/platform/src/db.rs` 及现有 descriptor/runtime 配置边界；`deploy/release-descriptor-v1.schema.json` 已存在并复用；宿主 `knowledgebrain-release` 已落位，隔离运行证据已取得，干净候选验收待完成。
- **输入：** [部署说明](../deploy/README.md)、[平台 §4.1](platform/runtime-foundation.md#41-releasedescriptorv1-binding)、干净已提交候选、Cargo.lock 与实际 digest-only 镜像身份。
- **依赖：** P2；候选提交/镜像获取及任何生产动作需其对应授权，本任务可先做普通实现与隔离验证。
- **产出：** 复用现有 checked schema 的唯一 release 工具、read-only descriptor mount 与启动顺序/实际 inspect 验证、拒绝矩阵及发布证据模板。
- **可证实验收：** exact descriptor/JCS hash、mount/env/receipt、component kind/digest suffix 与实际 full RepoDigest inspect 一致；缺文件/非法字段/任一 mismatch fail-closed、无 tag fallback；migrator 先成功再启动 API/Worker/Retention，重复启动只读 readiness，DocReader 用 post-start inspect gate。空对象卷真实上传/读取/引用保护/最终回收，记录完整应用外部依赖启动而非仅 shared verifier。所有验收仅专用隔离环境。
- **当前状态：** release 入口已实现，15 项脚本 Engine/输入故障测试、6 项身份合同通过，生产 Dockerfile 已构建 runtime/DocReader 并取得本机 registry 的实际 RepoDigest。修复无显式 environment 的依赖校验后，专属环境完整启动、migrator 先于 runtime、真实 receipt/readiness、容器及 receipt 不变的重复执行均通过；空对象卷真实 API 上传/读取、两业务引用的部分释放保护和最后引用释放后的 Worker/Retention 回收、精确删除凭据核对通过。最终四阶段 exit 0，9 服务容器、5 卷、专属网络及 registry 清理后零 runtime 残留。保留初次产品启动失败及第一次重跑的验收脚本失败，详见[真实隔离验收](../docs/bidding/release-live-results.md)和[入口记录](../docs/bidding/release-entry.md)。旧 tag 发布 overlay 已删除，无新增 migration。干净候选及对应构建身份验收仍缺，**R1 未正式完成**；hosted required gate 见 R3，不授权生产发布/提交。

### R2 — 受保护 namespace reset

- **目标 / 不做：** 落实平台现有 namespace reset 合同并故障注入；不是清理现有开发资料的任务，不用 `down -v`/全局 prune 或共享卷删除代替。
- **归属 / 候选文件面：** `deploy/` 现有 namespace/config 入口、platform typed namespace、后端归属边界及相关测试接缝；目标 `deploy/reset-namespace.sh` 尚未实现。
- **输入：** [平台 §5.1](platform/runtime-foundation.md#51-deploymentnamespacev1-与-reset) typed namespace、backend mapping、confirm token 与 fsynced checkpoint 合同。
- **依赖：** 专用可销毁测试资源和明确身份，隔离注入所需 receipt；不依赖 O0，正式发布前完成。
- **产出：** 最小 reset 工具、checkpoint/逐 backend receipt、每 crash 边界恢复与拒绝证据。
- **可证实验收：** drained/receipt/revision/mapping/containment 先验证；mode-0600 checkpoint atomic rename + file/directory fsync；按 OBJECT_DIR→MinIO→Neo4j→Redis→PostgreSQL 执行，每步 started/delete/done 注入失败。尤其 PostgreSQL 已 drop 仅凭合法 checkpoint 收敛；缺失/篡改/hash/token/mapping 漂移拒绝 resume。既有/非目标 namespace、共享卷、symlink escape 不受影响，失败也保留收据；清理仅本轮登记资源且零残留。
- **当前状态：** 已完成必要的归属前置修复：统一 canonical `DeploymentNamespaceV1` 并复用到 release/Oxana 校验，Neo4j 写入/查询/文档删除增加 typed namespace，节点/边均有归属。真实专属 Neo4j 红色复现及修复后 2/2 通过，三项类型/兼容性检查和 platform/knowledge 全目标/全特性 Clippy 通过，清理零残留。最初图隔离切片未改队列命名；后续源码已落实 Redis/Oxana 派生前缀，未操作既有资料或新增 migration；详见[namespace 隔离记录](../docs/platform/namespace-isolation.md)。OBJECT_DIR/MinIO layout 已补充 typed namespace 隔离、本地描述符 containment、缓存回源及 Retention 删除保护，3 项本地用例和 3 项专属 MinIO 必跑用例通过并清理零残留；上传不再吞存储错误，旧布局不自动认领。Redis/Oxana 与多模态计数器也已落实统一派生前缀，删除默认 Redis 地址回退；两项隔离实测、4 项原生故障测试及 14 项 producer 契约通过。PostgreSQL 派生库名、reset 确认校验和及专属连接下的实际库/OID/集群身份/完整 receipt/catalog 只读 gate 已实现；真实新建 PG 正反例 1/1 通过并清理零残留，不修改现有库名。本地对象 root/namespace/objects 已复用描述符路径检查并观察实际设备号/inode，三项文件系统正反例通过；Redis 实际数据库/standalone primary run ID/无凭据地址只读观察通过专属只读 ACL 正反例 1/1，临时实例清理零错误；MinIO/Neo4j mapping、runtime drained gate、持久 checkpoint、reset入口及五后端 crash恢复仍待实现，**R2 未完成**；任何现有数据 reset 仍需单独授权，未发布不构成许可。

### R3 — 首发 named required jobs 闭环

- **目标 / 不做：** 完成既定首发必跑接线并真实执行，不把 development checks 或历史 guard 探针称 release accepted，不在阶段 A/P1 顺手实施。
- **归属 / 候选文件面：** `.github/workflows/ci.yml`、现有 `scripts/`/`deploy/` 验收入口，按各领域已交付用例接线。
- **输入：** P2/O4/R1/R2 结果、固定 candidate SHA/Cargo.lock SHA/PG 与扩展/image 身份、真实所需样稿/字体/许可与 CI 环境。
- **依赖：** P2、O4、R1、R2；hosted 运行及发布操作需满足各自权限，不自动提交/push 触发。
- **产出：** `schema-contract`、`queue-faults`、`namespace-reset`、`outline-scripted-e2e`、`web-export-e2e`、`release-descriptor` 六个 named required jobs 的完整接线与同候选运行证据。
- **可证实验收：** 六项均真实执行其业务/故障验收，新 DOCX 链不得用旧块 E2E 冒充；已有 rust/三个 suite guard、`--locked` 与 schema job 不丢失。依赖缺失、零用例、ignored/skipped 或 cleanup 失败使 job 失败；保留各命令退出码与资源零残留。无 hosted 结果时只能报告本地验证，正式发布仍不得放行。
- **当前状态：** `schema-contract` 与 development checks 已接线；新增独立 `queue-faults` 岗位，强制真实 Redis、拒绝跳过/零用例/ignored/filtered、始终归档日志，并作为镜像构建依赖。复用现有 Oxana 原生测试，不新编队列产品。9 项实际 shell 控制流检查及该 CI 命令的专属 Redis 4/4 验证通过，清理零残留，见[岗位证据](../docs/bidding/workspace-quality-results.md#r3-queue-faults-ci-接线)。未提交或触发 hosted 工作流；其余 named jobs、分支 required 配置和同候选 hosted 跑通仍待完成，**R3 未完成**。

### V1 — 条件式 pgvector 分域基准

- **目标 / 不做：** 只有明确性能需求才测量，按实际计划决定是否调优；保留 PostgreSQL+pgvector，不新增向量库/同步/双写，不透明替换 exact 或冻结 policy。
- **归属 / 候选文件面：** Knowledge；`crates/knowledge/src/knowledge_retrieval_pg/semantic_v2.rs`、`crates/knowledge/src/search/hybrid.rs` 与所属索引/检索测试；本任务不纳入知识库其他产品计划。
- **输入：** 明确查询场景/SLO、可用于隔离验证的代表性数据、eligible scope、policy/revision 及普通搜索样本。
- **依赖：** 数据与性能条件触发；与 O0、平台正确性及首发安全独立。
- **产出：** V2 exact 证据检索与普通搜索分别记录 `EXPLAIN ANALYZE`、latency/P95、QPS、recall、CPU/I/O；证据支持的最小 SQL/HNSW 改动或不改结论。
- **可证实验收：** 使用代表性 eligible 集合复现瓶颈并核对优化前后结果与资源，不以任意向量数立门槛；exact/确定性排序、冻结 policy/revision、OCR 映射、eligible scope 与事务发布不变。ANN 候选/召回语义变化须另经证据和语义确认；只有调优后仍不满足 SLO 或独立故障域/扩缩容需求有实证才另评外库，不在本项擅换。
- **当前状态：** 条件未触发；无真实查询计划、代表性规模或延迟瓶颈证据，本阶段不优化。

## 外部前置与下一交接

| 前置缺口 | 影响任务 | 处理边界 |
| --- | --- | --- |
| P1 专用 PG16+pgvector/Redis、本地对象目录、临时 role/DSN 与安全资源登记 | P1，随后 P2 | 下一 worker 创建前先核实本地镜像与空闲端口；阶段 A 不启动服务、不读真实环境 |
| ONLYOFFICE 确切版本、本体许可/并发/嵌入权利、字体、真实样稿及可达测试环境 | O0 起 | 按真实条件准备/验证；未获环境权限不启动，不默认免费，不影响已授权平台开发 |
| 官方插件的目标定位、复杂表格及人工保护能力与许可 | O3-A/M | 先验证公开插件；只有选择外部 Connector 才涉及额外 Automation 授权，不作预先采购承诺 |
| 真实页码反馈与有限轮安全降级证据 | O3-M | 不因 Conversion 成功标完成；不能保真安全输出即记录技术失败 |
| 干净候选、实际镜像 RepoDigest、完整依赖和 hosted required jobs 结果 | R1/R3 | 首发前完成；提交/push/生产部署仍须对应授权，不阻塞 O0 或普通开发 |
| 专用多 backend 可销毁资源及 namespace/receipt 身份 | R2 | 只做已授权隔离故障注入，不操作现有资料或共享卷 |
| 明确性能需求/SLO与代表性数据 | V1 | 条件未触发则不优化，保留当前库与检索语义 |

**阶段 A 停止点：** 交付八份文档及相对独立 baseline 的 diff、范围/链接/文本验证器和逐项结果；由阶段 B 只读审查实际文件与 P1 范围。ready 是父预设工程门槛，不是再次询问普通开发许可。计划真实不一致则返回 blocked 等父裁决；本 worker 不续做 P1、不启动其他 worker、不运行仓库测试。后续切片完成后由独立审查和父验收更新本台账，任何未实际运行项均保留为待验证。
