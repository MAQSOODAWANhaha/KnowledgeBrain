# 招标理解 Agent 实现审查

## 当前方案与验收入口

唯一有效实施方案为 [Agent 运行时与完整样稿方案](../../plans/bidding/agent-runtime-rig.md)；已合并方案评审修订，按 S0–S6 分阶段推进。保留完整最终目标，按分项能力逐步实施和验收；设计要求、已有实现与真实验收分别记录，新设计不等于已经实现或通过。

api-v4 已完成两轮完整独立复核，但尚未通过。最新进度、缺口与证据只维护在[主方案 §19](../../plans/bidding/agent-runtime-rig.md#19-当前能力真实验收与下一步)，本页不重复运行轮次或临时检查数字。

下列历史证据只说明对应版本与当时状态，不构成另一份现行方案。历史 Agent P0–P4 编号不再用作当前实施阶段；平台 P0/P1/P2 和 ONLYOFFICE O 阶段的既有任务编号不受影响。

## 历史运行与验证记录

2026-09-16 早期历史快照：有界证据预装、Main自动推进和真实PG三边界恢复已验证；同版DOCX/PDF/报告正式导出接线通过隔离HTTP→worker测试（转换器模拟，非真实Office验收）。统一DocReader的DOCX列宽单位错误已修复，20项解析回归及3项真实DOCX冻结回归通过；新source-v4仅6表widths_mm变化，28个来源的文本、单元格、合并和ID保持一致。旧v3因错误冻结输入停止，47次调用及终态保留。api-v4在14:06 UTC观察到turn124/Main、49条候选、396次工具调用，首轮独立复核已完成28个来源判断并提出14项finding，现交回Main修复；这不是语义通过。同一request已自动由attempt1续至2，未手工continue，checkpoint与累计计数保留。379项库回归、API20项、worker38项及严格Clippy通过；解析合计23项（20项解析＋3项真实DOCX）、验收脚本18项及4个子测试通过，见[验证汇总](../../artifacts/minimal-bid-fixture/implementation/current-verification.json)。完整分析准入、编制及同版三件套仍未验收；最新明细以[统一方案](../../plans/bidding/agent-runtime-rig.md)及其验收证据为准。

106页历史终态摘要：106页真实文档的历史终态仍为 turn1604，424条候选、177条关系、104项有效修复说明、2项待处理、3处执行阻塞、0轮完整独立复核，累计3826/4000次调用。尚无验收通过的完整DOCX、同版PDF和报告。

[调用审查](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/call-cost-audit-20260916.md)确认v19有1604个完整Main回合、Reviewer为0，六段运行约3.96小时；1104回合无业务写尝试，674/3475次工具失败。必要读取不能一概算浪费，但主要改进对象是串行导航、范围/参数错误和反复登记。旧终态、消费和配置保持不变；新试验单独冻结合同，不自动重启旧耗尽运行。

2026-09-12 UTC 最新终态：`real-run-v19-repair-scope-resume5` 已于 12:43:50 UTC 结束，运行退出码 1，本段耗时 4427.34 秒；[固定终态](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/resume5-terminal-verification.json)为 turn1604（SHA `5e69cacf…`），424 条候选、177 条关系、104 条保存修复说明，有效处置 104/106、待处理 2，保留 3 处执行阻塞，完整独立复核 0 轮。错误 `AGENT_TURN_BUDGET_EXCEEDED` 指局部执行及独立工作交接额度耗尽；全文累计调用 3826/4000、尚余 174 次，并非总调用帽耗尽或供应商超时。未重启。有效处置不等于独立语义批准；完整 106 页、32 项语义及同版 DOCX/PDF/报告验收仍未完成。正确性与稳定性优先，速度优化后置。

2026-09-12 主修复异议闭环记录：[宿主核查与默认 CI 回归](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/repair-dispute-closed-loop.md)确认生产已有 disputed→独立保留或撤回→完整来源复核及编制准入的闭环；当时仅补测试，覆盖受影响候选详情门槛、主角色不能自批和独立裁定分支。[验证记录](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/repair-dispute-closed-loop-verification.json)为最终断言加强前 217 项 tender_analysis 测试通过、36 项既有忽略，随后加强断言的新专项 1 项通过，严格 Clippy、全仓 fmt 与 diff 整合检查通过。这是合成脚本的宿主协议验证，不是 grok 的真实语义成功。[主方案](../../plans/bidding/agent-runtime-rig.md)已记录“Main 修复任务隔离：turn1604 后续实施边界”，T1–T3 任务账本、Main 派发和既有 Journal 合同已进入代码整合与离线回归，尚未完成整合验收或部署，未授予新尝试；旧 turn1604 终态、检查点、阻塞及累计调用账本保持不变。

编制版式：[行内布局修复](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/inline-template-layout-implementation.md)已完成局部验证：同一冻结来源中连续文本区域保留原始换行和独立字段书签，示例填写值在原位置清除；跨来源、间隔和网格保持边界。固定 turn1571 的[真实候选前后 DOCX 对照](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/inline-layout-before-after.json)确认四个身份字段恢复同行、20 个区域定位均可回读。[整合验证](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/inline-layout-integration-verification.json)为 40 项编制测试通过、1 项既有忽略，严格 Clippy、全仓 fmt 与 diff 检查通过；无新增 migration，未改提取 schema 或配置。该局部诊断不是整单样稿，也不代表原页像素、完整 DOCX/PDF 或 R03/R06 验收通过。

终态局部核查：[A/E 四条处置复核](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/ae-repair-semantic-check.json)绑定 turn1604，四条均为 `revised` 回执刷新，所引用记录和关系已在 turn1231 存在且未变；引用目标有原文依据，但字段语义及重复关系风险仍在，不能计作四个新语义修复或独立批准。 [最后两项原文核查](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/remaining-ae-source-semantics-1604.json)区分 A 的有据缺失关系与 E 尚无具体前附表目标支持的修复要求；后者需要有源异议或准确未决并由独立复核裁定，不能强行补边。[终止轨迹](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/repair-tail-terminal-check-1604.json)未出现“全部修复完成却无法复核”的状态。以上人工诊断不发送模型，检查点、阻塞及调用额度保持原样。

历史工程状态（恢复范围完成校验修复部署时，下方检查数字不含本轮行内布局修复）：Main 完成范围与既有阻塞目标交叠时，只要该目标仍有未有效处理的问题，就拒绝 complete，保留目标 watch 并给出既有问题导航；覆盖合并、部分交叠及合法恢复后直接完成的路径。此前局部 v2、历史召回与候选身份索引导航保持生效。[最终联合检查](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/current-checks.json)的 320 项库测试、17 项 baseline、严格 Clippy、fmt、diff 及构建全部通过，39 项库测试仍忽略；[实现独立复核](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/implementation-review.md)与[迁移脚本独立复核](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/transition-review.md)完成，[实际部署核验](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/deployment-verification.json)通过。未改 Progress 算法、schema、静态提示词、`.env`、预算或检查点字段，未新增 migration；历史已删除 watch 不凭空还原。

历史恢复：resume4 停止于 prepared 边界 turn1147 / calls1150，[原边界恢复验证](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/resume-verification.json)通过；resume5 的[首个请求](../../artifacts/bid-full-sample/real-run-v19-repair-scope-resume5/startup-verification.json)保留原 98,478 字节正文。[turn1156 启动快照](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/started-progress-snapshot.json)与[turn1514 中途快照](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/progress-snapshot-turn-1514.json)仅为历史记录，已由上方 turn1604 终态替代。

独立局部语义核查：[R06 修复报告](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/r06-repair-semantic-check.json)严格绑定 turn1472，确认弱口令跨页完整文本、续项继承适用性、父项证明和响应义务、续项关系已补齐，关系端点版本匹配；“制行方式”来自冻结原文，并非此次修复新增错字。父子项仍各自声明同一证明响应，存在下游重复编制风险，须检查实际整稿；证明对象本身并未重复，不能预先断言 DOCX 已重复或 R06 整项已通过。

[R03 修复报告](../../artifacts/bid-full-sample/loop-repair/repair-history/scope-completion/r03-repair-semantic-check.json)严格绑定 turn1499，只确认新增一条与原网格证据相符的 requires_template 关系；四条相关既有记录未变，unknown 适用性/强度及附表归属仍未解决。原标题、单位、表外注释及跨页续文、签署虽已在 requirement，仍未进入模板 region 或建立对应关联；表内备注实际保留，不能误报全部注释缺失。未核查该版实际 DOCX/PDF，R03 完整验收未通过。两份局部核查分别绑定各自检查点，不扩大为 turn1604 或完整样稿已通过；人工诊断不发送模型。

固定 turn939 的语义预检仍单独保留：[R03 原文与网格证据](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/r03-939-source-evidence.json)及[诊断](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/r03-939-source-review.md)表明报价表同页标题、表外注释及续页签署来源已存在，但当时网格适用性与模板关联尚未完成；表内备注已保留，不能误报全部注释缺失。[R04–R07 技术预检](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/technical-r04-r07-precheck-939.json)确认若干正文/证明保留子断言，同时 R06 弱口令跨页记录仍缺连接。两项诊断只绑定 turn939，后续候选变化须重新核验，不能当作后续检查点的最终状态或已通过验收；人工诊断不发送模型。

2026-09-12 UTC 历史状态（候选身份索引导航部署，10:28）：候选 ID 不存在或类别不匹配时，保留失败并给出既有 `inspect_analysis` 索引查询，由模型取回完整 ID 后再精确读取；不猜测或自动替换 ID，不自动执行查询或授予详情回执。此前主修复局部 v2、旧摘要兼容及历史只读导航保持生效。[最终联合检查](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/current-checks.json)的 315 项库测试、17 项 baseline、严格 Clippy、fmt、diff 及构建全部通过，39 项库测试仍忽略；本次 4 项普通专项测试、[固定 turn666 的实际 48,000 字节预算索引及精确详情投影](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/fixed666-index-projection.json)、[独立代码复核](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/identity-error-review.md)通过。[部署核验](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/deployment-verification.json)已完成；未改 schema、静态提示词、`.env`、预算或恢复许可。未知 source 错误不在本次修复范围内。

该次历史续跑记录：旧 `real-run-v19-repair-local-resume3` 已通过 SIGINT 停止于 prepared 边界 turn825 / calls827，原 5 处阻塞与 44 条修复说明完整保留，[真实边界无模型恢复验证](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/resume-verification.json)通过。新 `real-run-v19-repair-id-resume4` 已从原检查点启动（会话 65461 / PID 2278412）；[首个实际请求](../../artifacts/bid-full-sample/real-run-v19-repair-id-resume4/startup-verification.json)与离线恢复的 64,896 字节正文一致，`.env` 原 SHA、冻结来源及旧 journal 原档均未改变。

该次历史观测：运行进度引用[固定的 10:43:32 UTC 快照](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/progress-snapshot-2026-09-12T104332Z.json)：turn924 / calls927，全文累计 3146/4000；419 条候选、122 条关系、52 条保存修复说明，有效处理 44/106、待处理 62，保留 4 处执行阻塞，完整独立复核 0 轮。[E 范围只读诊断](../../artifacts/bid-full-sample/loop-repair/repair-history/id-navigation/blocker-e-source-navigation-diagnosis.json)表明查询反复停留在正文条款，尚未取回前附表目标及选择分支证据作完整比较；source ID 错误提示不能单独解决该问题，finding41 也未被判定错误或撤销。源码部署、保存说明及工程验证不代表语义通过；完整 106 页、32 项语义及同版 DOCX/PDF/报告验收仍未完成。正确性与稳定性优先，速度优化后置。

2026-09-12 UTC 历史状态（09:28）：历史召回、有界恢复与按范围/阻塞状态选择待修复问题的导航已完成[联合验证](../../artifacts/bid-full-sample/loop-repair/repair-history/navigation/verification.json)；295 项库测试、17 项 baseline、严格 Clippy/fmt/diff 通过，37 项库测试仍忽略。`real-run-v19-repair-history-resume2` 正在运行（会话 87609），09:27 快照 turn519 / calls521，全文累计 2740/4000；420 条候选、87 条关系、25 条保存修复说明、3 处执行阻塞，独立复核 0 轮。此次已有实际候选修正和新端点关系，但一条 Rule 修改触发全局版本失效，有效处理数曾 23→0，现重新核查后为 18；[只读诊断](../../artifacts/bid-full-sample/loop-repair/repair-history/navigation/global-rule-invalidation/assessment.md)确认主修复说明与独立复核共用全局规则依赖，执行 blocker 的局部依赖却未变化，尚未实施该职责拆分。首条修正核心有原文支持，但新缩写与字段引用精度仍待复核。原检查点、已消费额度及 `.env` 配置均保留；32 项语义与完整 DOCX/PDF/报告验收尚未通过。

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

2026-09-11 UTC 最新状态：局部导航、历史来源判断取回及固定任务清单复用已通过248项库、严格Clippy及全局fmt。同一检查点离线请求构造中位耗时约556→295毫秒、请求摘要不变；尚无新的模型完成率或全文性能验收。全文兼容接续后已在第236轮因HTTP500/503/503终止，当前没有真实模型进程运行；累计998次调用，原总额度剩202次，但该边界三次尝试已耗尽，未重置。保留33项来源判断及27项发现，另有2处执行阻塞。32项与完整DOCX/PDF/报告仍未完成；费用正文两组亦未完成复核，不能宣称性能可用。以[方案§14](../../plans/bidding/agent-runtime-rig.md#14-成本可观测性与全文容量)及所链接证据为准，下方为历史运行记录。

2026-09-11 UTC 性能修复真实对照通过：同一七页目录、原7项候选和同.env配置，baseline为33次/239.30秒/10次工具错误，完整原文出处规则的provenance为8次/63.55秒/0次工具错误，两组均verified且生产结构审计通过、候选值不变。该局部对照墙钟减少73.44%，不代表完整106页性能。已合并有界边界证据，并将冻结原文读取/引文与实际候选依赖分开；实际候选查询、记录/映射/关系ID及同页原图排版依赖保留。241项库、严格Clippy和编译通过；同次导航复用清单，离线输出摘要一致。旧全文v3在146轮无在途边界暂停，累计759次；v4已使用原1200额度剩余441次启动，首请求合同与成功对照一致，保留416条记录/8条关系，未导入旧独立回执。32项和同版DOCX/PDF仍未完成。证据：artifacts/bid-full-sample/loop-repair/review-boundary-evidence/provenance-final.json、real-run-v15-review-v4/startup-verification.json。

2026-09-11 UTC 当前执行状态以[统一方案](../../plans/bidding/agent-runtime-rig.md)为准：原文搜索的错误全局依赖已修复，239项库回归通过；全文v3保留主提取成果并在原1200次上限扣除613次后，以587次余额进行独立复核。完整语义及出件验收未完成。以下较早运行记录保留为历史证据。

**2026-09-10 当前修复进度：本地功能验证通过，独立复核已完成局部比较，未完成最终提交，运行已停止；完整验收尚未通过。** 已分离来源权限与局部焦点，自动维护成果/未解决引用，主提取、独立复核、编制和稿件复核共用进展与有界恢复策略。默认连续无进展6轮、焦点24轮、重规划2次；记录局部执行阻塞后允许转向独立范围，连续6轮仍未交接则在工具提交边界停止。重启、重复读取、笔记改写和任务改名不能刷新额度；有效局部写入可以完成当前动作。执行失败单独保存并阻止最终发布，不冒充来源缺项。候选当前版本参与窗口保留；各 reviewer 保留自己的冻结原文回执，修改后的候选/稿件仍按摘要核查。旧手抄引用输入及编制 `remember` 路径已删除，未新增 migration，既有 baseline 同步检查点与预算 JSON。

验证：最新176项库测试（11项忽略）、20项合同、1项真实检查点离线提交诊断、严格 Clippy、workspace fmt及样稿编译通过；既有6项隔离 PostgreSQL结果保留，本次未改SQL。首次3来源短测在21轮停止，11条记录、0关系，发生一次超时后重试成功；第二次 v2 在32轮停止，29条记录、12条关系、2个来源处置，四组重点引用由真实Agent产生，但独立复核未开始。v2请求42214–353819字节，含必要原页图片，无503或超时。轨迹还暴露无响应要求被迫指定渠道、同类型合规属性的不同条件被拒绝、工作引用格式说明不足；均已修复并验证。未新增 migration，未修改实际 `.env` 或旧检查点，未执行暂存操作。包含全部修复的 `response-contract-trial` 已以新身份、空候选重测第11、16、17页的3处来源，模型与预算仍来自 `deploy/.env`；启动合同已核对，结果待验。该试验在34轮保留32条记录、21条关系后，进一步定位到重复读取会触发无实际缺口的 pending_delivery 阻塞。已删除这条冗余判断，尚未交付的原文/候选仍按真实缺口阻止交接；172项库测试、20项合同及Clippy/格式/编译通过，SQL未因本项调整。`response-contract-resume1` 已在第101轮由无进展/交接保护停止：主提取完成，41条记录、21条关系、3项来源处置；独立复核收到65个候选当前版本后仍重复读取，两次重规划无效，0轮复核、1项执行阻塞。已保留终态和计数，未将空缺口等同于语义通过。现补充复核专用完成指引与按实际缺口生成的下一动作：有问题逐项保存、无问题完成原范围后提交空草稿；执行阻塞仍禁止提交。173项库测试、20项合同及Clippy/格式通过，无新工具/配置/migration；提示词已改变，`reviewer-completion-trial` 已以新身份、空候选复测；启动核验确认实际配置、工具和预算未变，旧终态未改。该试验在第85轮到达诊断时限并取消：主提取41条记录、28条关系、3项来源处置；reviewer完成一个局部范围、收到72个候选当前版本，仍未提交最终结论。除精确读取位置反馈、局部复核排除不相关待办外，现新增 complete_review_check，在既有进展账本中记录当前候选的无问题比较；重复版本/改写结论不能续额度，来源、当前版本、执行阻塞及最终全局门槛保持不变。176项库测试、20项合同、3项.env启动测试及Clippy/格式/编译通过，无新表、migration、检查点字段或环境变量。新工具/提示词采用新身份：clean-review-trial 在补充明确授权后实跑656.16秒，停于第25轮：独立收到72个候选当前版本，完成41个记录版本的局部比较，46次重复比较被去重；28条关系和3项来源处置仍未形成比较结论，0轮最终复核。定位到当前焦点已完成但执行反馈仍要求比较该焦点，已增加焦点剩余数和转向未完成引用的派生反馈；176项库测试、20项合同、Clippy/格式/编译通过。clean-review-resume1 保留原检查点、预约正文及计数兼容续跑，在第40轮完成全部72个局部比较，但此后反复读取，0次完成范围、0次提交复核，第58轮进入执行阻塞，第64轮耗尽交接额度后停止。只读第54轮检查点副本的正式工具调用均通过，证明当时接口可用；离线副本不作为真实复核结果。最终结束行为仍未解决，本轮后续复杂附表及完整样稿重测未启动。此前自动审批拒绝已由用户补充明确授权解除。旧运行、原始来源和计数未改。复杂附表、完整106页独立复核、32项语义发现及完整 DOCX/PDF仍未通过。证据：[验证记录](../../artifacts/bid-full-sample/loop-repair/verification.json)、[v2真实轨迹](../../artifacts/bid-full-sample/loop-repair/source-scope-trial-v2/result.json)、[上一实跑启动核验](../../artifacts/bid-full-sample/loop-repair/reviewer-completion-trial/startup-verification.json)。

当前进度（2026-09-10）：v12 已人工停止并保留第152轮检查点：耗时2975.47秒，54条记录、6条关系、20个来源处置，独立复核未开始；最后47个完成轮次没有新增记录，期间仍有读取和导航。导航裁剪改善了原文保留，但未解决完整持续产出。终态见 `artifacts/bid-full-sample/real-run-v12/terminal.json` 和 `diagnostic-checkpoint.json`。

v13 已获明确外发授权并启动，向 `https://ai.zleiwork.cn` 发送同一招标文件的解析文本、网格及必要原页图片，模型和预算只读 `deploy/.env`。归档程序包含候选详情回执及字段校验反馈；启动核对确认供应商、预算、二进制、冻结来源和新提示词/工具合同一致。真实持续提取、独立复核及完整 DOCX/PDF 尚未通过，32项发现保持开放。证据：`artifacts/bid-full-sample/real-run-v13/startup-verification.json`。第104–129轮状态保留在 `artifacts/bid-full-sample/real-run-v13-resume1/progress-snapshot.json`；第129轮后由兼容恢复目录 v13-resume2 续跑，新增15条记录后再次出现重复核查，现已停稳于第164轮，详见下方字节定位修复记录。

2026-09-10 字段校验反馈已补齐：复用原 `ok/error` 封装及语义校验器，以 `INVALID_FIELD <JSON Pointer>: <constraint>` 指明记录、引文、模板单元格及关系的失败位置和约束，失败写入保持原子性。反序列化错误定位到所属容器，不声称每个嵌套 Serde 错误都能定位叶子字段；原文未给出的单位等仍允许为空。153项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、严格 Clippy、workspace fmt 及样稿编译通过，无新增 migration。证据：`artifacts/bid-full-sample/validation-fields/verification.json`。

2026-09-10 原页历史淘汰修复：v13 第68→69轮确认超大旧图片组会先挤掉较早的完整网格，随后自身也被淘汰。现按冻结历史预算优先淘汰必然放不下的已交付图片组，保留较小原文组；离线回放从0张完整网格改为保留2张及相关正文，154项库测试、20项合同、5项隔离 PostgreSQL、Clippy/格式及样稿编译通过。最终修复未改变提示词、工具、来源或预算合同。v13 原实例停稳于第104轮后，以归档修复程序从同一检查点恢复，保留52条记录和105次累计调用；恢复记录见 `artifacts/bid-full-sample/real-run-v13-resume1/startup-verification.json`。证据：`artifacts/bid-full-sample/image-history/verification.json`。

2026-09-10 正文字节定位修复：`read_source` 在保留原文及起止范围的同时返回逐行 `line_spans`，中文与原换行均按真实 UTF-8 字节定位，完整结果按原工具预算分页；兼容恢复仅给已交付历史补充确定性位置，不改已预约请求和阅读回执。真实第120→121轮离线回放保留两页58行完整正文及附表，请求116767字节；157项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、Clippy/格式及样稿编译通过，无新增 migration。v13-resume1 停稳于第129轮，52条记录、0关系、20个来源处置、0轮独立复核，累计131次调用。原状态已逐字节复制到 v13-resume2，生产合同校验通过；自动审批首次拒绝后，用户针对具体目的地和载荷再次明确回复“继续,允许”，现已从第129轮恢复，原预约正文保持不变并累计第二次调用；启动核验见 `artifacts/bid-full-sample/real-run-v13-resume2/startup-verification.json`。第130轮实际请求已验证包含两页58个逐行位置；随后新增15条记录，达到67条。第136轮后连续28个完成轮次无新增记录、关系或来源处置，期间73次候选详情/目录查询、29次搜索，未尝试写入关系。原文持续保留，但候选详情在有界窗口内反复取回；该相关性尚不能单独证明模型循环的根因。恢复实例运行777.78秒后安全停于第164轮，保留67条记录、0关系、20个来源处置及167次累计调用；未进入独立复核。逐行位置已验证可用，整体持续提取仍未通过，不能继续以相同正文重试或放宽预算冒充修复。证据见 `artifacts/bid-full-sample/real-run-v13-resume2/repeated-inspection-observation.json`、`terminal.json` 和 `line-spans-request-verification.json`。证据：`artifacts/bid-full-sample/source-line-spans/verification.json`、`artifacts/bid-full-sample/real-run-v13-resume2/preflight-verification.json`。32项语义发现及完整 DOCX/PDF 验收仍开放。

2026-09-10 停滞进一步定位：四组简单引用的双方完整详情在7个真实请求中同时可见，生产关系工具的离线内存副本验证全部通过，单组详情仅1116–1288字节；不能再将零关系简单归因于窗口装不下。28轮内257份详情只有25个版本，工作笔记与17项缺口不变。当前缺口是局部任务推进和停滞恢复，候选历史保护不足会加重重复，但不是充分解释；模型内部选择原因仍不可由轨迹证明。详见[定位报告](agent-loop-diagnosis.md)。本次未修改生产逻辑或新增外发，修复方向尚待短范围真实验证。

**历史结论（v6/v7）：当时真实提取结果不通过，累计 32 项语义发现仍开放。** 早期格式批次的19项发现和12个模板文本区域重叠只是部分证据，完整范围见[真实稿复核](real-tender-acceptance-review.md)及[106页验收索引](../../artifacts/bid-full-sample/acceptance-index.json)。v6 因连续57轮无业务写入而人工停止；修复工作范围缺口反馈后的 v7 完成137轮后仍持续空转，已停止，尚未完成独立复核或完整 DOCX，见[性能诊断](extraction-performance.md)。

下文记录历次运行层及业务实现证据；[改造方案](../../plans/bidding/agent-runtime-rig.md)的 P0 已实现精确查询、读取交付确认、结构化范围交接、有界历史、环境参数读取及输入 token 估算，并完成本地和历史请求投影测试。Rig AgentRun 已接入提取/编制并通过隔离回归，新运行持续提取/独立复核仍待完成。生产持久化、发布、HTTP/前端及合成生成至 Office 保存已在后续批次接通，真实内容验收仍未通过。

业务依据沿用 [PRD §1.3、§1.4、§2.3](prd.md)。目标覆盖网络安全行业的设备、软件、服务和综合类招标；当前真实主样稿是防火墙采购，不能据此宣称已覆盖全行业。当前阶段提取招标侧要求，生成所需章节、固定文字、模板与填写位置；投标方事实、报价、证明材料和具体响应后置。

提取质量 `verified` 表示招标侧解释已通过独立复核，不表示某个投标人满足条件。有原文依据、明确条件与范围的 `conditional` 要求、规则和模板可通过提取复核，编制时必须保留条件，不擅自选定投标身份。`unknown`、未解决关系和来源失败仍使来源质量为 needs_review，独立复核问题也不得跳过。编制可消费已经独立复核、缺口明确记录的待确认分析并单独报告缺口；当前自动编制路径拒绝尚未修正的复核 findings；这是自动化结果校验现状，不是对已有 DOCX 编辑/导出新增业务闸门，也不表示所有未复核分析已经可编制。不得为生成样稿而把条件改成无条件适用。

## 解析与理解的边界

上传原件是事实依据，统一 Python `services/docreader` 是文件解析入口，冻结的解析结果是 Agent 的主要工作输入。实际生产调用链为 `DocReaderGrpcTenderSourceConverter → docparser::convert_tender_source → DocReaderServicer`；Rust 包装层负责调用、校验、冻结和任务生命周期，Agent 不再实现 PDF/DOCX 解析器。

同一文件版本与解析契约复用结果；解析配置变化应产生新版本，不能覆盖已经用于分析的来源。文本、表格、页码、图片与版面来源不能压成不可回溯的纯文本摘要。文档解析完成不等于语义提取完成，也不等于逐页解析质量可靠。

对扫描文字、星号/勾选标记、合并单元格、续表和版面歧义，Agent 可调用 `read_source_view`，通过统一 Python service 的 `SourceView` RPC 查看冻结 PDF 的物理原页或上传图片。页面获取复用现有 PDF 渲染器、全局锁、资源限制和 gRPC 认证；Rust 只作范围/摘要/图像尺寸校验，不新增原件解析器。重新解析仍应产生新的来源版本，不混入当前冻结版本。DOCX/XLSX 的物理页排版与局部放大尚未接通；不能伪造页码或以截图替代可编辑模板。人工测试中使用 `pdftotext` 核对原文不构成生产解析路径。

每张原页保存原件 SHA256、来源 ID、物理页序、图片 SHA256、尺寸、渲染器身份及实际图片字节。worker 复用已有可取消的原件读取器，数据库只允许读取当前 owner 所持请求的冻结文件集合。图片通过模型的图片消息传入，不能把 OCR 摘要称作“已看原件”。主 Agent 与复核 Agent 分别记录查看，复核必须查看主 Agent 使用过的图片；重试及复核复用快照内同一图片。视觉引用使用 `view_id` 和零文本偏移，不能为图片上的发现伪造 OCR 原文引文。图片不替代完整文本/网格阅读，回看失败或摘要/预算校验失败不能发布为 verified。

本轮复用现有 checkpoint 与分析版本保存有界图片，没有新建表或 migration 文件。按需查看可以控制规模，但完整 checkpoint 会重复保存缓存，大量页面的存储成本仍需实测，不能宣称已解决长文性能问题。

### 当前实现的运行配置（Rig 改造前）

`KB_TENDER_AGENT_LIMITS` 是请求创建时冻结的必填 JSON，包含 `max_turns`、`max_tool_calls`、`max_read_bytes`、`max_context_bytes`、`max_history_bytes`、`max_context_tokens`、`image_token_reserve`、`token_safety_margin`、`max_tool_result_bytes`、`max_review_rounds`、`max_source_view_bytes` 和 `max_source_view_edge`。前十项限制整个主 Agent/复核流程；最后两项分别限制单张图片编码前字节数和像素长边。数值由运行环境明确配置，不由样稿、业务类别或模型名称推导。

`max_history_bytes` 必须为正且小于 `max_context_bytes`，限制已经交付的历史协议组；图片按实际 Base64 消息计入。最新待交付工具组不计入历史额度，但仍受总上下文上限约束，不能静默裁掉。旧预算 JSON 缺此字段时拒绝创建新请求，不添加隐藏默认值；新提示词、工具及预算构成新冻结合同，旧运行不得混用。历史样稿的 `limits.json` 是旧运行证据，不覆盖为新配置。

按用户要求，`deploy/.env.example` 提供完整提取预算默认 JSON，沿用既有运行额度并加入 `max_history_bytes=65536`；本地 `.env` 只补缺项。这里是部署默认值，不是 Rust 静默补全旧合同。它限制历史字节量，不代表已验证模型 token 窗口。

`max_context_bytes` 必须容纳提示词、工具定义、对话及 Base64 图片；图片编码开销约为原字节数的 4/3，也计入读取预算。`max_source_view_edge` 不得超过 Python service 的 `DOCREADER_PDF_RENDER_MAX_EDGE`，图片预算也受 service 的 gRPC 文件上限约束。模型必须同时支持工具调用和图片输入；不支持时不能自动改为只看 OCR 并称作通过。Compose 已透传预算配置，此处没有为真实模型配置隐式额度，也没有部署服务。

## 逐步审查与取舍

| 环节 | 实现理由与替代方式 | 判定条件及当前边界 |
| --- | --- | --- |
| 冻结文件集合 | 汇总全部已选文件、关系与人工决定；关键词检索只帮助定位，不能筛掉输入。只读取向量检索命中片段会遗漏表后说明、否决条款和补遗。 | 全部成员有状态；未解析文件显式保留。不能靠“已处理文件数”证明语义完整。 |
| 完整阅读 | Agent 自主分段读取、增量保存，可在一轮返回多个工具调用；不把整本招标文件塞进一次请求。 | 提取主/复核角色的读取成功先保存为 pending，下一完整请求取得有效工具响应后才确认交付；同批写入不能使用未送达的新读取。索引、检索和失败读取不算覆盖。主动窗口及多工具总上下文预算仍待完成。 |
| 语义提取 | 项目事实、编制规则、要求、模板和待确认项分开；使用来源定义的指标名称和条件，避免维护一套写死的网络安全产品参数表。 | 指标保留对象、指标项、原比较符、数值、单位与条件；维保/许可/升级/服务范围不能并成摘要。AND/OR、按单项/按合计、包件与实施阶段需保留原条件，当前不执行任意自然语言公式。 |
| 多重属性 | 强制程度、适用性、评分与响应方式不能混为一项。真实样稿的星号要求同时涉及否决、正偏离加分和证明材料。 | 已将响应属性改为多项，各有条件和来源；证明义务保留主体、出具方、有效期及条件。旧单值投影遇到不同属性类型标为 unknown；同类属性的多个条件仍投影该类型，完整分析保留所有属性，后续消费者必须读取完整分析。 |
| 附表关联 | 附件是跨页的结构，可有固定段落、多个网格、子附件、说明和签章。记录多对多关系及依据，不按同名/同号合并。 | 已校验来源范围、模板父子循环和网格单元格身份；原文是否支持关系仍需语义复核。字段级关系已支持模板区域、冻结网格锚点及要求的响应/证明/指标项，绑定记录摘要；修改后必须重新建立并独立复核。实际 DOCX 字段落位校验已在后续编制批次接通，真实语义与编辑后映射仍待验收；记录一条关系不能替代此验收。 |
| 独立复核 | 复核使用独立上下文与阅读台账，从原文查遗漏，再逐项检查结果依据。自我声明“没有问题”不能作为完成门槛。 | 已新增结果及关系的实际读取记录，按内容摘要识别过期检查；完整读过原文但没检查结果也不能提交。相同模型的两个上下文仍可能同错，需要独立人工预期与真实模型评测。 |
| 修复循环与成本 | 复核问题分页读取，避免把整份复核台账塞回上下文；只读工具不再复制不断增长的语义图。 | 当前复核轮仍保守地完整重读来源。改成增量复核前，需证明修改影响传播到引用、适用性、总则、补遗及被删除记录；不能简单跳过“内容未改”的关联项。每轮完整 checkpoint 的存储成本仍需长文实测。 |
| 恢复与发布 | 精确保存请求和 checkpoint，调用预算跨重试累计；发布受 owner/lease 保护。 | 已验证隔离数据库发布、重投、活跃 owner、错误 token 和调用预算；取消时先销毁工作 future 再释放 owner。预算耗尽不会把未完成分析发布为已通过。 |
| DOCX 消费 | 基于冻结分析组织完整稿，由确定性工具检查实际章节、固定文字、网格及填写区。 | 旧单次 LLM 模板入口及专用 SQL/模型 schema 已删除，保留确定性渲染原语；已新增独立分章 Agent 核心，可生成并核验真实 DOCX 的章节、模板和字段位置；生产持久化、新轮发布、HTTP/界面及合成生成保存已在后续批次接通；分析成功不等于真实完整稿已验收。 |

行业通用性应通过不同类型样稿验证：设备指标及原厂证明、软件授权与容量/期限、测评服务的范围/标准/交付物、运营服务的人员/驻场/SLA、综合项目的包件和跨附件依赖。这些是验收维度，不是自动套用到每个项目的业务规则。招标文件未写的产品能力、法规证书或行业惯例不能自行加入投标义务。

## 早期实现批次的证据与未完成项

- 来源预期：[tender-analysis-golden-v1.json](../../crates/bidding/tests/fixtures/tender-analysis-golden-v1.json)，由真实原件建立，标注原件 SHA256 和物理页码，未使用旧 Agent 输出作标准答案。
- Python service 来源锚点测试：`test_tender_analysis_source_contract.py` 通过，覆盖星号多重效力、复合技术指标、原厂证明、G.1/G.2、不适用附件、附件8条件和2A/9价格一致性原文。此测试证明来源保留，不证明模型理解正确。
- Rust 核心测试覆盖读区间、超预算回滚、增量记录、复核遗漏后的修订、变更结果重新检查、来源越界、恢复与预算。
- 新建临时 PostgreSQL 应用三份 fresh baseline，并执行 `tender_analysis_postgres`，通过发布、重放、owner隔离和跨重试物理预算验证；资源按本次标签清理。验证发现并补齐了新发布系统身份的登记；没有新增 migration 文件，也未操作现有数据库。
- 本轮回归：投标模块单元测试 57/57、docparser 46/46；提示词收尾后 Agent 定向复跑 15/15。gRPC 代码使用仓库 `services/docreader/scripts/generate_proto.sh` 重新生成，保留既有 experimental 类型兼容包装，重新生成后 Python 原页与来源锚点 11/11；API/worker check 与 diff 空白检查通过。
- 原页增量：真实 Python gRPC 与来源锚点测试 11/11；Rust Agent 测试包含图片实际进入独立上下文、复核缺图拒绝、快照 ACK 丢失后图片复用、摘要/字节预算失败不计覆盖。临时 PG16＋真实认证 Python service 联调 2/2，通过冻结集合隔离、原件摘要校验、图片入版本和损坏原件降级待复核。真实 PDF 第70物理页已人工查看 G.1/G.2、表头、表后说明及页码。测试模型为脚本，图片渲染和 RPC/数据库为真实实现。联调同时修复并发首次上传在两个唯一索引之间的解析契约登记冲突，保留原有契约摘要一致性检查。
- 该早期批次没有配置模型调用参数和 `KB_TENDER_AGENT_LIMITS`，尚未运行真实模型全样稿提取；后续已实际运行，最新终态见本页顶部。该批次尚未完成全行业样稿、Office 原页/局部放大、字段/章节到 DOCX 的完整映射及界面展示。旧单次原型已在后续字段关系批次撤除。不能把这些标成已验收。原页联调的本机证据位于 `/tmp/kb-source-view.q2mqy8kk/`，本次临时容器与 Python 进程已清理；不是部署验收。

## 字段关系与旧原型撤除增量

原关系只有两端记录 ID，无法区分同一附表的多个金额字段、同一要求的多个响应位置，也禁止同一记录内字段关联。`RelationTarget` 现在区分整条记录、模板区域、冻结网格单元格锚点、响应项、证明项和指标项。相等/汇总关系必须定位具体字段或标为 unresolved；多单元格区域不能作为已解析的单值字段，区域与单元格两种定位指向同一字段时也不能伪造自关联。相同附件标题不是身份；网格必须属于该模板已标注的区域，合并单元格只允许原始锚点。

关系由工具绑定两端完整记录的摘要，模型不能提供或伪造摘要。记录修改后工具返回需要重新核查的关系 ID，结构缺口阻止沿用旧关系进入复核；重新建立关系改变其摘要，原复核记录不能继续充当已核查。这里保守地使整个记录相关的关系失效，因为修改适用性、条件或区域顺序都可能改变字段含义；只绑定单元格坐标不能处理这些变化。数值关系的公式与条件仍保留原文解释，本期没有把边自动解释为求和或开始填入报价。

已删除 `/docx-template/preview`、`prepare_template`、专用模型 schema 和 `kb_bid_v2_load_docx_template_input` 及其授权；全量原文单次请求不再是活跃模板生成路径。仅保留 `compile_template` 的确定性 DOCX 原语，尚未将它接成已完成的 Agent 分章生成器。本轮没有新表、列或 migration 文件，不修改已有数据库，也不引入网络安全产品参数字典。

验证：投标模块单元测试 **63/63**（Agent **21/21**）；新字段测试覆盖同号附件、非锚点/越界/外部网格、字段别名、记录修改失效、复核过期和序列化恢复。独立临时 PG16 三 fresh baseline 加真实认证 Python service **2/2**，包含新关系随分析版本发布、按版本分页读取保留精确端点、已完成请求重投不再调用模型；模型为脚本。API/worker check 与 diff 检查通过，worker 仍有此前旧路径未使用告警。本机证据 `/tmp/kb-field-relations-f_leqq21/`；本轮容器与 Python 进程已清理。真实模型语义准确率、前端关系展示、分章 DOCX 落位和真实编辑后映射保持仍未验收。

分章生成的后续核心实现、选择理由及边界见[分章 Agent 与 DOCX 实际落位](docx-composition.md)。此前“仅保留渲染原语、分章尚未实现”和待生产接线均为较早批次状态；后续合成产品链路已有验证，真实完整稿仍未验收。

## 精确网格证据与逐段要求

真实样稿复核暴露出要求/指标/证明只能引用文本或原页的缺口。现在 `Span` 增加可选 `grid_cell: {form_id, row, column}`；网格引用使用 `start=end=0`，与 `view_id` 互斥。`read_form` 按返回单元格顺序提供 `citations`，合并覆盖位置对应 `null`。校验复用冻结网格锚点规则，并核对来源归属、行列范围和该单元格的实际阅读记录。读取整页文本不授予网格引用权限；超出工具输出预算时，新增引用和阅读状态均不提交。

要求的 response、criterion、proof、applicability 等沿用同一证据结构。这样，投标阶段的逐项响应和交付阶段的证书条件可以分别引用实际来源，跨页条款可以保存多段精确依据。无文本但有可读网格的来源，在全部网格已读且存在非空锚点时可以记录有依据的判定；不能仅凭表格存在消除未知。

发布沿用现有完整分析 JSON 和格式身份关联。网格/图片引用不写入旧文本引文表，不制造零长度引文；网格身份单独加入已有格式引用集合。不增加数据库字段、迁移文件或解析流程。旧式文本引用的序列化保持不变；提示词与工具合同发生变化，真实重跑必须创建新运行身份，不能改旧 checkpoint 冒充恢复。

编制 `source_response` 的网格片段必须同时出现在要求记录和具体响应项的精确单元格依据中。整页依据不再授权任意单元格；已审模板的固定/待填策略仍单独执行。Agent 提示词要求按可独立核查的义务组织记录，保留复合条件、跨页续文及证明时点；独立复核逐行核对条件与落位。精确引用只证明指向正确位置，不能自动证明理解或提取完整。

模板混合格现可用可选 `blank_ranges` 表达局部待填值，复用 `read_form.find_text` 获取原格精确子串的UTF-8位置；每个命中需结合上下文审核，匹配不是自动清空授权。区域只能使用已读锚点、互不重叠的有效范围，同格只分配一次策略；未选部分原文完整保留。主Agent和独立复核都须核查固定标签、单位、说明及签章没有误入清空范围。详见[局部留白与验证边界](docx-composition.md#混合单元格的局部留白)。

验证：86 项模块测试、17 项 baseline 合同、定向 Clippy 和样例构建通过；真实第59–61页沿用冻结 Python 来源完成单元格引用与 DOCX 原文承载测试，证据在 `artifacts/bid-full-sample/grid-citations/`。这是定向来源/编制原语验证，未重新调用真实模型，未完成整本提取或样稿验收。

## P0 首批实现：精确查询与读取交付确认

2026-09-09，修改当前 `tender_analysis`，未接入 Rig，也未改变模型、协议、解析服务或数据库。`inspect_analysis` 新增可选 `ids` 与 `source_id`：先按真实身份选择，再序列化返回；筛选后分页，组合条件取交集，同名标题不作匹配依据。关系的来源查询包含依据及两端记录的声明来源；disposition 用来源 ID。未知/重复/错类别 ID、外部来源与过大结果明确拒绝，不能增加复核覆盖。工具 schema 和提示词摘要随之变化，旧运行不得混用新合同。

主 Agent 与 reviewer 各自的读取结果先存入 checkpoint 的 `pending_coverage`，完整结果进入下一请求并取得有效工具响应后才确认阅读/候选检查覆盖。同一批“读取→写记录”不能借用尚未送达的依据；503/中断不确认，恢复仍发送同一冻结正文。图片延期只撤回未送达的查看，不撤回另一角色或此前已经取得的证据。这里仍沿用既有每轮 checkpoint；不声称已经实现 P3 的三边界恢复。

本机证据 `/tmp/kb-agent-p0-2p0jh_w4/`：查询旧实现2项失败、交付旧实现2项失败的 red 日志保留；修复后投标库108 passed/0 failed/1 ignored，忽略项是需显式来源和输出目录的导出载体工具；bidding 全目标/全特性严格 Clippy 通过。该批恢复测试使用内存 Journal 和脚本模型，未跑真实数据库或外部模型。后续范围和窗口实现见下节，不能据此标记 P0、32项语义或真实整稿验收通过。

## P0 后续实现：工作范围、历史窗口和批量响应预算

主 Agent 和 reviewer 分别保存结构化 `set_work_note`：`source_scope`、`objective`、`output_refs`、`pending_refs`、`status`、`note`。阅读正文、网格和原页前须声明真实来源范围；跨引用可以扩展范围，切换前须完成交接。完成要求已交付范围正文和网格、来源处置及成果引用，未解决项必须保留。reviewer 另须独立检查当前成果摘要和主提取用过的范围原页。主 Agent 不能通过删除记录抹去任一角色交接中的未解决项。结构检查不证明语义正确。

完成交接后主动释放旧原文；请求按完整 assistant/tool 组限制历史，稳定前缀后放历史，末尾放当前工作和进度。一次模型返回多个工具时，执行前为全部响应保留序列化空间，执行后检查实际下一请求。读取结果装不下时给出缩小范围/减少调用的反馈，并撤回该次新增的读取覆盖和预算计费；最新待交付结果不能靠历史裁剪丢弃。写操作结果无法容纳则停止该批，不保存虚假成功。仍使用既有每轮 checkpoint，P3 三边界尚未实现。

投标库115项通过、0失败、1项按显式输入要求忽略，全目标/全特性严格 Clippy 和工作区 fmt 检查通过。隔离 PostgreSQL 已验证提取发布/重放/物理预算、诊断权限及编制持久化消费者，三个测试通过；现有 JSON 检查点兼容，无新增 migration、Agent 表或 baseline 修改。证据位于 `/tmp/kb-agent-context-lcbbk3hb/`，临时数据库容器已清理。真实长轨迹、输入 token 估算、环境参数一致性和32项语义复验仍待完成，没有重新调用真实模型，不能据此宣称 P0 或完整样稿通过。

## 运行参数与环境默认值

提取和编制共用 `AuthoringRuntimeContractV1`。`KB_AUTHORING_MAX_OUTPUT_TOKENS=8192`、`KB_AUTHORING_TIMEOUT_MS=180000` 是环境模板和 Compose 的部署默认值；本地 `.env` 已补齐缺失项，已有模型、地址和凭据不改。新请求读取并冻结正整数，运行中不跟随环境变化。`KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT` 默认空，未配置时省略；配置后按原值发送，不根据模型名称推断支持值。主配置与 `LLM_*` 别名冲突时拒绝，错误只显示变量名，不显示凭据。

真实样稿启动器会从选定 `.env` 重新加载这三项并清掉继承进程的旧值。提取及编制 SQL 预约检查推理参数与冻结配置一致；编制允许此可选字段，其余字段仍严格校验。旧 baseline 的隔离测试已复现“篡改推理参数仍获预约”，因此需调整现有 baseline 的字段校验；不新增 migration、表，也未向现有数据库应用 SQL。参数测试与证据在 `/tmp/kb-agent-config-uwhxt9d5/`，不能代替真实模型可用性或语义验收。

本批验证：投标库118项通过、2项需要显式输入而默认忽略；另显式执行历史请求投影测试并通过。严格 Clippy、fmt、API/Worker 编译检查、20项 schema/baseline 合同、3项启动器测试和3项隔离 PostgreSQL 测试通过。重新读取本地 `.env` 得到模型 `grok-4.6`、输出8192、超时180000、推理参数未设置、历史预算65536；这是启动器配置检查，没有重启服务或调用模型。

历史窗口测试用 v5 的31个已预约请求快照，对主提取与 reviewer 共做62次投影；原始最大请求784403字节，投影最大154698字节，最后主提取93614字节、reviewer72749字节。验证最新工具组原样保留、工具调用与结果配对及重复构建字节一致。该测试只把历史协议组交给新请求构建器，不执行旧工具、不迁移旧合同、不声称旧 v5 已恢复，也不证明局部成果写入或复核语义通过。明细见同证据目录 `recorded-window-replay.json`。

## 输入预算与实际用量

部署默认 `max_context_tokens=131072`、`image_token_reserve=16384`、`token_safety_margin=4096` 已写入环境模板，本地 `.env` 仅补缺字段。总量是应用设置的预算，不是根据模型名推测或验证过的供应商窗口；部署时须与服务能力一致。估算将完整请求 JSON 的 UTF-8 字节计为文本 token，包括提示词、工具、参数、消息和状态；每张图片移除 Base64 URL 的文本计数，改用独立图片预留，再加安全余量。它是保守估算，不是精确 tokenizer，图片额度也须结合实际 usage 复核。

每次请求须满足“估算输入＋冻结输出上限 ≤ 总 token 预算”，并同时满足总字节及历史字节上限。token 超限沿用完整协议组裁剪、图片延期及批量读取反馈；不裁掉未交付结果后冒充已读。无有效预算时在预约前停止。新请求包含 `stream_options.include_usage=true`，baseline 预约校验此固定字段；复用既有 SSE 解析器保存供应商用量，不新增 SDK 或第二套传输。缺失用量保持未知，多次 usage 事件按最终累计快照处理，缓存/推理量是输入/输出子集，不重复累加。日志记录估算、实际量及是否低估，不能把请求变小直接当作提速或语义通过。

本批121项投标库测试、7项 SSE 解析测试、严格 Clippy、3项隔离 PostgreSQL 测试及62组历史请求投影通过。token 约束下投影最大请求116546字节，最后主提取93654字节；历史原件、模型和凭据未改。证据 `/tmp/kb-agent-token-budget-pidcumoz/`。无读取方的 `BID_EXTRACT_MODE`、`BID_EXTRACT_MODEL_ID` 已从部署配置删除，提取只使用统一生成模型配置。


## P1 首片：独立复核问题持久化

本节记录早期 Agent P1 切片，不对应当前 S1。现行设计见[方案 §9](../../plans/bidding/agent-runtime-rig.md#9-分析复核修复与全局闭合)，最新实现与验收见 §19；以下为该历史切片结果：独立原文判断、依赖失效及宿主批次末汇总已落地，旧 `submit_review` 已删除；本轮工程验证通过，真实短测及完整语义验收进行中。以下旧工具描述只用于理解当时的持久化验收。

reviewer 用 `put_review_finding` 逐项保存有依据的问题，服务分配 ID，已知 ID 可修订；`delete_review_finding` 只能明确撤回已存问题。`inspect_review` 分页返回 reviewer 草稿及 ID，主 Agent 继续读取上一轮已提交发现。`submit_review {}` 校验完整独立覆盖后提交存储中的草稿；旧的 `findings` 整数组参数已删除，传空数组不能清除已有问题。

每条发现必须有 reviewer 已收到的来源依据或独立检查过当前摘要的成果。主 Agent 没有草稿写权限。草稿保存在现有 JSON 检查点，跨范围交接、历史裁剪以及检查点确认响应丢失均保留；最终 `Review.findings` 发布结构不变。新工具/提示词摘要与 v6 不同，不能用新代码恢复旧合同。没有新表、migration 或 baseline 改动；该切片不等于 P3 的三边界实现。

证据 `/tmp/kb-agent-review-draft-yeb8o8v4/`：10项复核相关测试、20项 schema/baseline 合同和4项隔离 PostgreSQL 测试通过，其中新增非空草稿保存后确认丢失、恢复查询与撤回测试。专用数据库容器已删除。全库测试及严格 Clippy 受另一 agent 正在修改的 DocReader/网格接口影响，未计为通过；按用户要求不修改该部分。具体字段错误结构、完整附表/跨范围关系和真实复核仍未完成。


v6 停滞诊断后的查询修复：`inspect_analysis` 按预算返回完整记录页，`next` 可继续，只有实际返回的记录进入待交付复核覆盖。新增回归逐页核对记录无丢失、顺序不变和无越界复核；复核草稿的大小校验包含查询响应外壳，防止保存后单条也无法取回。修复后提取模块53项通过、1项显式输入工具默认忽略，另按协作范围跳过正在改动的空网格锚点测试；合同20项、隔离 PostgreSQL 4项通过，bidding 格式检查通过。严格 Clippy 仍被并行修改的 DocReader/稀疏网格代码阻断，未记通过。范围交接反馈和新真实运行尚待继续，不能将本批测试视为停滞已解决。


## 当前范围交接缺口：复用 check_gaps

v6 停滞现场的活动范围有12个来源，均缺少来源处置；已有37条范围记录，但工作笔记仅保留1条输出引用和1条待办。全局268条缺口的首页50条全是范围外未读正文。这证实了诊断入口与当前任务错位，不能由此断言模型停滞只有这一种原因。

`check_gaps` 现在必须显式选择 `scope=work` 或 `scope=analysis`，已删除不声明范围的旧调用合同。前者只报告当前工作交接需要的实际缺口：未交付读取、未读原文/网格、缺失来源处置、应保留的成果/未解决引用，以及 reviewer 自己尚未检查的成果和原页；返回 `kind`、`field`、真实来源/表格/成果身份和具体说明。后者继续承担全局发布/复核前检查，不被局部完成替代。

当前范围诊断和 `set_work_note` 完成校验共用同一检查逻辑；切换范围失败时明确指向该诊断。模型须先修复当前范围，再以相同 `source_scope` 提交 `status=complete`，最后打开下一范围。查询只返回导航信息，不授予原文、网格或候选复核覆盖；同批新读结果仍须经下一次有效模型响应确认。跨范围关系和已携带的未解决引用继续保留，未更改语义判定或解析。

候选查询和两种缺口查询共用预算分页，返回完整条目与 `next/total`；单条也放不下时明确失败，不返回截断记录。无效范围、未声明工作、零页大小和错误偏移均拒绝。无需新表、migration、环境配置或模型覆写。

本批投标库128项通过、3项显式输入工具默认忽略；另外显式执行 v6 离线诊断回放通过，48个局部缺口可按2KB预算分5页完整取回。回放不调用模型、不更改或恢复旧检查点。20项 schema/baseline 合同和4项隔离 PostgreSQL 测试通过；严格 Clippy 与 bidding 格式检查通过，专用容器已清理。证据在 `/tmp/kb-agent-work-gaps-y42wdqci/`，实际启动的新 v7 使用冻结二进制与新身份，32项语义验收不因此关闭。

## 2026-09-10 候选目录与完整详情分离

2026-09-10 候选目录/详情修复：`inspect_analysis` 增加 `view=index/detail`。无 ID 查询默认返回目录（ID、引用、原候选标签及来源），有 ID 默认返回完整详情；目录不计阅读或复核覆盖。详情仍按当前候选摘要确认且完整分页，预算不变。v9 两批真实查询离线回放保留全部当前原文，目录中12/14条不同候选均可逐条完整取回。库147项、合同20项、隔离 PostgreSQL 5项及严格 Clippy/格式通过；新合同须新建运行，32项语义验收仍开放。证据：`artifacts/bid-full-sample/candidate-index/verification.json`。

## 2026-09-10 大范围的安全拆分

2026-09-10 已补齐安全拆分：工作笔记新增有界 `deferred_sources`，允许将活动范围缩小并释放旧窗口；移出的每个来源及已有成果引用必须保留。待处理来源只能通过重新纳入活动范围移出列表，列表未清空禁止主提取提交及复核提交；局部完成、全局阅读、来源处置和独立候选核查门槛不变。v10 离线投影将12个活动来源缩为1个、保留11个待处理来源，完整48格网格和对应原页可同时发送，业务成果和原检查点不变。库148项、合同20项、隔离 PostgreSQL 5项、Clippy/格式通过，真实持续产出仍待验证。证据：`artifacts/bid-full-sample/work-split/verification.json`。


## 2026-09-10 先释放已交付导航，保留原文证据

v11 的真实第33轮请求包含4张完整网格及28274字节旧来源索引；6次记录写入后，第34轮只保留2张网格，旧来源索引却全部保留。原有整组裁剪把大索引和同组唯一原文绑定，造成可避免的原文丢失和重读。

请求超限时，先把已交付的旧 `source_index`、`search_sources` 和 `inspect_analysis(view=index)` 成功结果替换成明确省略标记，再按原规则裁剪整组。完整来源、网格、原页、集合元数据、候选详情和错误结果保持原样，最新尚待交付的整个工具组不改。协议调用 ID、阅读账本、预算和业务校验不变；使用已有 Rig 会话重建路径，不增加模型总结、配置项、表或 migration。主提取与独立复核共享此策略，提示词说明省略标记不是证据，变更提示词摘要后须新建运行。

真实请求离线重放先红后绿：修复前仅保留2张网格、请求107508字节；修复后全部4张网格及当轮写入结果原样保留、请求95213字节，原检查点未改。库149项通过（9项需显式夹具/服务的测试默认忽略）、合同20项、5项隔离 PostgreSQL 恢复回归、严格 Clippy、workspace fmt 通过。证据见 [navigation-history/verification.json](../../artifacts/bid-full-sample/navigation-history/verification.json)。数据库回归验证既有三边界、重放和所有权合同，不代替真实导航轮次的语义验收。

v11 在第48轮检查点保存后停止以切换已验证修复，最终35条记录、12个来源处置、0关系、0复核。该轮仍有写入，且实际发生180秒超时重试；不能把停止描述为连续空转或宣称服务超时已解决。v12 使用原 `.env` 配置和新运行身份复验；32项语义发现、独立复核及完整 DOCX/PDF 仍开放。


2026-09-10 v12 实际请求已触发导航省略：第46→48轮新增11条记录期间保持3张当前网格；第51轮补读第4张网格后，第53→54轮又新增5条记录，4张网格仍全部保留。第55轮检查点为43条记录、6条关系、12个来源处置，独立复核未开始。证据见 [实际导航裁剪观察](../../artifacts/bid-full-sample/real-run-v12/navigation-observation.json) 和 [运行快照](../../artifacts/bid-full-sample/real-run-v12/progress-snapshot.json)。这补充了真实供应商下写入后原文保留的证据，仍不证明完整持续产出、语义正确或供应商延迟已解决；32项发现保持开放。


## 2026-09-10 候选详情的角色独立回执

v12 第54–89轮检查点的记录、关系、来源缺口及处置数量均不变，多轮重复取回已存详情后才继续写入。观察到188次完整详情对象返回，其中146次为此前已返回的相同版本；重复可能用于必要比较，不能将全部重复都归为无效调用。原主提取不保存候选详情回执，历史退出窗口后无法从目录判断已收到哪些版本。诊断见 [candidate-inspection-diagnostic.json](../../artifacts/bid-full-sample/real-run-v12/candidate-inspection-diagnostic.json)。

主提取和 reviewer 现在各自复用已有 `Coverage.candidate` 保存完整详情摘要。目录新增 `detail_received`，仅表示本角色在已完成模型轮次中收到过当前版本，不表示语义正确、比较已完成或原文已读。同一批工具中刚取回的详情仍标为未交付；记录修改后旧摘要不再匹配。目录查询自身不创建或刷新回执，另一角色不能继承回执。工作交接反馈明确：已有成果只需保留其引用，不要求为登记引用重新查询全文；原有完整阅读、成果引用和独立复核门槛保持。

两项新增回归覆盖角色隔离、修改失效、检查点往返、未交付与已交付边界；分页回归确认仅完整返回的对象获得回执。151项库测试、20项合同、5项隔离 PostgreSQL 恢复回归、严格 Clippy 和 workspace fmt 通过。使用现有 Journal v3 和 JSON 字段，无新增表或 migration。证据见 [candidate-receipts/verification.json](../../artifacts/bid-full-sample/candidate-receipts/verification.json)。

提示词和工具摘要已改变；v12 已停止，v13 使用新身份和新归档程序复验，不混入旧代码或重写旧检查点。本项尚未证明实际消除循环，32项语义发现、独立复核及完整 DOCX/PDF 验收仍开放。

### 没有提交产物的约束

真实短测发现，强制每条要求的 `response` 非空会诱发无依据的 `structured_form` 或声明渠道。现在允许该数组为空，但 `explicit_response` 合规属性仍要求有来源的响应项；其他合规属性、条件、判据和证明义务不因此消失。Agent及独立复核须检查关联总则，区分未要求、明确要求和尚未找到引用目标，不能用空数组静默省略真实义务。

既有 SQL 发布投影同步允许没有响应项的约束，用根节点 `all_of` 的空 `children` 表达零提交项，不生成虚构 need。嵌套空组和空 `any_of` 仍无效；这不表示投标人已经满足资格或履约条件。仅调整既有 baseline 函数，没有新表、列或 migration。

同类合规属性可以有多项：例如不同阶段或不同对象各有 `must_comply` 条件，应逐项保留条件与依据。只拒绝 policy、condition、grounds 完全相同的重复项；不能按 policy 去重后迫使模型合并条件。这一约束由真实短测中的连续写入拒绝定位，并有字段保留及持久化回归。

## 局部无问题比较与独立诊断

Reviewer 可用 complete_review_check 为活动焦点中的一个精确候选记录无问题比较，输入 reference、summary、sources。当前候选详情和原文引文都必须由该 reviewer 独立收到；存在该候选的已存问题时不能用无问题声明覆盖。说明和证据保存在既有 Journal 工具历史，候选引用＋版本摘要的去重回执复用 reviewer_progress；改写说明、变换引文或重启不重复计进展。修改候选后旧回执不代表新版本已核对。请求同时给出当前范围的比较进度和下一候选引用，避免用重新读取目录代替保存结果。该局部声明不是整份分析批准，原文到成果的遗漏核查、待办、最终覆盖和执行阻塞规则不变。

样稿诊断程序新增 review 模式，以 analysis-seed.json 中的归档 Agent 候选为待核查声明。种子摘要绑定运行合同，新的 reviewer 阅读覆盖从空开始；旧运行不恢复、不改写或重置计数。输出 review-diagnostic-result.json，与新的完整提取及 DOCX 验收分开。工具/提示词合同变化须用新运行身份。

原文检索与候选查询的依赖必须区分：`search_sources`只返回冻结正文/表格中的位置导航，有命中和无命中均不受候选或发现变更影响，因此不设置全局分析依赖。实际取回候选、跨来源阅读和判断引文仍登记相应依赖；全局候选查询和全局规则保留保守失效。已有检查点的global标志及回执不改写。见[红绿与真实重派发证据](../../artifacts/bid-full-sample/loop-repair/source-query-dependencies/verification.json)。
