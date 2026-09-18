# Agent 字段复核、检查点恢复与 Rig 接缝验证

> 历史验证记录：仅说明所记录版本的结果，不是现行设计或新流程验收。当前方案与任务见 [统一方案](../../plans/bidding/product-two-phase.md)。

本文保留各日期的历史验证结果，版本号和运行状态不代表当前实现。最新合同、能力缺口及真实样稿验收见[统一方案 §19](../../plans/bidding/product-two-phase.md)。

**2026-09-10 当前修复进度：本地功能验证通过，独立复核已完成局部比较，未完成最终提交，运行已停止；完整验收尚未通过。** 已分离来源权限与局部焦点，自动维护成果/未解决引用，主提取、独立复核、编制和稿件复核共用进展与有界恢复策略。默认连续无进展6轮、焦点24轮、重规划2次；记录局部执行阻塞后允许转向独立范围，连续6轮仍未交接则在工具提交边界停止。重启、重复读取、笔记改写和任务改名不能刷新额度；有效局部写入可以完成当前动作。执行失败单独保存并阻止最终发布，不冒充来源缺项。候选当前版本参与窗口保留；各 reviewer 保留自己的冻结原文回执，修改后的候选/稿件仍按摘要核查。旧手抄引用输入及编制 `remember` 路径已删除，未新增 migration，既有 baseline 同步检查点与预算 JSON。

验证：最新176项库测试（11项忽略）、20项合同、1项真实检查点离线提交诊断、严格 Clippy、workspace fmt及样稿编译通过；既有6项隔离 PostgreSQL结果保留，本次未改SQL。首次3来源短测在21轮停止，11条记录、0关系，发生一次超时后重试成功；第二次 v2 在32轮停止，29条记录、12条关系、2个来源处置，四组重点引用由真实Agent产生，但独立复核未开始。v2请求42214–353819字节，含必要原页图片，无503或超时。轨迹还暴露无响应要求被迫指定渠道、同类型合规属性的不同条件被拒绝、工作引用格式说明不足；均已修复并验证。未新增 migration，未修改实际 `.env` 或旧检查点，未执行暂存操作。包含全部修复的 `response-contract-trial` 已以新身份、空候选重测第11、16、17页的3处来源，模型与预算仍来自 `deploy/.env`；启动合同已核对，结果待验。该试验在34轮保留32条记录、21条关系后，进一步定位到重复读取会触发无实际缺口的 pending_delivery 阻塞。已删除这条冗余判断，尚未交付的原文/候选仍按真实缺口阻止交接；172项库测试、20项合同及Clippy/格式/编译通过，SQL未因本项调整。`response-contract-resume1` 已在第101轮由无进展/交接保护停止：主提取完成，41条记录、21条关系、3项来源处置；独立复核收到65个候选当前版本后仍重复读取，两次重规划无效，0轮复核、1项执行阻塞。已保留终态和计数，未将空缺口等同于语义通过。现补充复核专用完成指引与按实际缺口生成的下一动作：有问题逐项保存、无问题完成原范围后提交空草稿；执行阻塞仍禁止提交。173项库测试、20项合同及Clippy/格式通过，无新工具/配置/migration；提示词已改变，`reviewer-completion-trial` 已以新身份、空候选复测；启动核验确认实际配置、工具和预算未变，旧终态未改。该试验在第85轮到达诊断时限并取消：主提取41条记录、28条关系、3项来源处置；reviewer完成一个局部范围、收到72个候选当前版本，仍未提交最终结论。除精确读取位置反馈、局部复核排除不相关待办外，现新增 complete_review_check，在既有进展账本中记录当前候选的无问题比较；重复版本/改写结论不能续额度，来源、当前版本、执行阻塞及最终全局门槛保持不变。176项库测试、20项合同、3项.env启动测试及Clippy/格式/编译通过，无新表、migration、检查点字段或环境变量。新工具/提示词采用新身份：clean-review-trial 在补充明确授权后实跑656.16秒，停于第25轮：独立收到72个候选当前版本，完成41个记录版本的局部比较，46次重复比较被去重；28条关系和3项来源处置仍未形成比较结论，0轮最终复核。定位到当前焦点已完成但执行反馈仍要求比较该焦点，已增加焦点剩余数和转向未完成引用的派生反馈；176项库测试、20项合同、Clippy/格式/编译通过。clean-review-resume1 保留原检查点、预约正文及计数兼容续跑，在第40轮完成全部72个局部比较，但此后反复读取，0次完成范围、0次提交复核，第58轮进入执行阻塞，第64轮耗尽交接额度后停止。只读第54轮检查点副本的正式工具调用均通过，证明当时接口可用；离线副本不作为真实复核结果。最终结束行为仍未解决，本轮后续复杂附表及完整样稿重测未启动。此前自动审批拒绝已由用户补充明确授权解除。旧运行、原始来源和计数未改。复杂附表、完整106页独立复核、32项语义发现及完整 DOCX/PDF仍未通过。证据：[验证记录](../../artifacts/bid-full-sample/loop-repair/verification.json)、[v2真实轨迹](../../artifacts/bid-full-sample/loop-repair/source-scope-trial-v2/result.json)、[上一实跑启动核验](../../artifacts/bid-full-sample/loop-repair/reviewer-completion-trial/startup-verification.json)。

当前进度（2026-09-10）：v12 已人工停止并保留第152轮检查点：耗时2975.47秒，54条记录、6条关系、20个来源处置，独立复核未开始；最后47个完成轮次没有新增记录，期间仍有读取和导航。导航裁剪改善了原文保留，但未解决完整持续产出。终态见 `artifacts/bid-full-sample/real-run-v12/terminal.json` 和 `diagnostic-checkpoint.json`。

v13 已获明确外发授权并启动，向 `https://ai.zleiwork.cn` 发送同一招标文件的解析文本、网格及必要原页图片，模型和预算只读 `deploy/.env`。归档程序包含候选详情回执及字段校验反馈；启动核对确认供应商、预算、二进制、冻结来源和新提示词/工具合同一致。真实持续提取、独立复核及完整 DOCX/PDF 尚未通过，32项发现保持开放。证据：`artifacts/bid-full-sample/real-run-v13/startup-verification.json`。第104–129轮状态保留在 `artifacts/bid-full-sample/real-run-v13-resume1/progress-snapshot.json`；第129轮后由兼容恢复目录 v13-resume2 续跑，新增15条记录后再次出现重复核查，现已停稳于第164轮，详见下方字节定位修复记录。

2026-09-10 字段校验反馈已补齐：复用原 `ok/error` 封装及语义校验器，以 `INVALID_FIELD <JSON Pointer>: <constraint>` 指明记录、引文、模板单元格及关系的失败位置和约束，失败写入保持原子性。反序列化错误定位到所属容器，不声称每个嵌套 Serde 错误都能定位叶子字段；原文未给出的单位等仍允许为空。153项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、严格 Clippy、workspace fmt 及样稿编译通过，无新增 migration。证据：`artifacts/bid-full-sample/validation-fields/verification.json`。

2026-09-10 原页历史淘汰修复：v13 第68→69轮确认超大旧图片组会先挤掉较早的完整网格，随后自身也被淘汰。现按冻结历史预算优先淘汰必然放不下的已交付图片组，保留较小原文组；离线回放从0张完整网格改为保留2张及相关正文，154项库测试、20项合同、5项隔离 PostgreSQL、Clippy/格式及样稿编译通过。最终修复未改变提示词、工具、来源或预算合同。v13 原实例停稳于第104轮后，以归档修复程序从同一检查点恢复，保留52条记录和105次累计调用；恢复记录见 `artifacts/bid-full-sample/real-run-v13-resume1/startup-verification.json`。证据：`artifacts/bid-full-sample/image-history/verification.json`。

2026-09-10 正文字节定位修复：`read_source` 在保留原文及起止范围的同时返回逐行 `line_spans`，中文与原换行均按真实 UTF-8 字节定位，完整结果按原工具预算分页；兼容恢复仅给已交付历史补充确定性位置，不改已预约请求和阅读回执。真实第120→121轮离线回放保留两页58行完整正文及附表，请求116767字节；157项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、Clippy/格式及样稿编译通过，无新增 migration。v13-resume1 停稳于第129轮，52条记录、0关系、20个来源处置、0轮独立复核，累计131次调用。原状态已逐字节复制到 v13-resume2，生产合同校验通过；自动审批首次拒绝后，用户针对具体目的地和载荷再次明确回复“继续,允许”，现已从第129轮恢复，原预约正文保持不变并累计第二次调用；启动核验见 `artifacts/bid-full-sample/real-run-v13-resume2/startup-verification.json`。第130轮实际请求已验证包含两页58个逐行位置；随后新增15条记录，达到67条。第136轮后连续28个完成轮次无新增记录、关系或来源处置，期间73次候选详情/目录查询、29次搜索，未尝试写入关系。原文持续保留，但候选详情在有界窗口内反复取回；该相关性尚不能单独证明模型循环的根因。恢复实例运行777.78秒后安全停于第164轮，保留67条记录、0关系、20个来源处置及167次累计调用；未进入独立复核。逐行位置已验证可用，整体持续提取仍未通过，不能继续以相同正文重试或放宽预算冒充修复。证据见 `artifacts/bid-full-sample/real-run-v13-resume2/repeated-inspection-observation.json`、`terminal.json` 和 `line-spans-request-verification.json`。证据：`artifacts/bid-full-sample/source-line-spans/verification.json`、`artifacts/bid-full-sample/real-run-v13-resume2/preflight-verification.json`。32项语义发现及完整 DOCX/PDF 验收仍开放。

2026-09-10 停滞进一步定位：四组简单引用的双方完整详情在7个真实请求中同时可见，生产关系工具的离线内存副本验证全部通过，单组详情仅1116–1288字节；不能再将零关系简单归因于窗口装不下。28轮内257份详情只有25个版本，工作笔记与17项缺口不变。当前缺口是局部任务推进和停滞恢复，候选历史保护不足会加重重复，但不是充分解释；模型内部选择原因仍不可由轨迹证明。详见[定位报告](agent-loop-diagnosis.md)。本次未修改生产逻辑或新增外发，修复方向尚待短范围真实验证。

2026-09-09 历史批次。本轮完成当前提取/编制驱动的三边界 Journal 实现及隔离恢复测试，并验证 Rig 0.42.0 Chat Completions 的发送接缝。该批次尚未替换生产循环；后续已接入 Rig AgentRun 与 Journal v3，真实招标提取与完整 DOCX/PDF 仍未验收。 阶段定义见[实施方案](../../plans/bidding/product-two-phase.md)，整体进度见[执行台账](../../plans/implementation-tasks.md)。

## 已实现范围

- 复核问题的 `affected` 改为 `{id, path}`，`path` 使用 JSON Pointer 定位当前成果的真实字段；空路径表示整个对象。`correction` 要求具体修正说明。对象、字段、重复引用及独立查阅的当前摘要均校验；旧的纯 ID 数组不再兼容。
- 提取和编制复用 `agent_runtime::TurnJournal`，保存独立 `sequence` 与可选待处理轮次。待处理轮次含角色、模型轮次、精确 JCS 请求正文和可选完整模型响应。
- PostgreSQL 预约与“请求已准备”检查点在一个事务提交；响应先持久化，再确认模型已收到证据、执行工具；业务成果、工具结果和新状态共同保存后才进入下一轮。
- 检查点表的 `batch_ordinal` 表示检查点序号；调用尝试表的同名列继续表示模型轮次。代码不按 `turn * 3` 生成序号。既有 baseline 校验三个状态转换、原始预约正文、不可变身份、owner、单调预算、同序号幂等及分歧拒绝。发布必须没有待处理轮次。
- 冻结检查点合同升级为版本 2，不新增表或 migration。历史失败检查点保留原格式；不将它们改写后冒充新合同恢复。

本地样稿工具按文件同步写入并原子替换。预约账本先保存，检查点保存成功才发送；跨文件失败可以消耗一次预约但不会发送。它不宣称具备 PostgreSQL 的跨文件事务原子性。

## 恢复验收

新增内存故障测试覆盖提取与编制两侧：预约失败零模型调用；准备状态已保存但 ACK 丢失后复用同一正文；完整响应已保存但 ACK 丢失后不重调模型；恢复时取消不推进工具或覆盖；已有工具提交 ACK 丢失测试继续通过。共享状态拒绝非法顺序、不完整响应、重复工具调用 ID 和损坏的非 JCS 请求。

五项显式隔离 PostgreSQL 测试通过，全部使用自建临时数据库，结束后删除所属容器：

1. `analysis_three_boundaries_are_atomic_and_resume_received_responses`：检查点拒绝使预约事务回滚、准备/响应边界恢复、拒绝绕过响应直接提交、已收响应禁止重约模型、owner 切换、独立检查点序号。
2. `review_draft_survives_committed_checkpoint_ack_loss`：复核草稿持久化、工具提交后的 ACK 丢失与无重复执行。
3. `analysis_publication_replay_fencing_and_physical_budget`：提取发布、重放、执行权及累计物理预算。
4. `analysis_diagnostics_preserve_utf8_and_reject_foreign_owners`：诊断和 owner 隔离。
5. `composition_durable_journal_resumes_review_and_enforces_owner_and_call_budgets`：编制三边界、真实事务回滚、完整响应复用、工具提交恢复、冻结正文和累计预算。

本轮 Bidding 库测试 **137 passed / 3 ignored**，Schema/baseline 合同测试 **20 passed**；Bidding 全目标严格 Clippy 通过。ignored 的外部依赖测试不计入通过；当前范围的数据库和 HTTP 测试均另行显式运行。Knowledge SSE 既有7项测试及本轮修改包的 fmt 检查通过。全仓 `cargo fmt --all -- --check` 仍报告其他 agent 正在修改的 `crates/docparser/src/grpc.rs` 格式差异，按用户要求未改该文件。该检查范围不等于全仓、生产运行或真实语义验收。

## Rig Chat Completions 接缝

初次接缝验证时 `rig-core = "=0.42.0"` 仅加入 Bidding 测试依赖；后续生产传输切片见下文。此接缝门禁仍可重复执行：

```sh
cargo test -p bidding --test rig_chat_seam -- --ignored --nocapture
```

测试只访问自有 loopback HTTP 服务，使用合成参数，不读取 `.env` 凭据或发送招标资料。7 个场景覆盖正常响应、预约拒绝、HTTP 503、提前 EOF、缺少 finish reason 的 DONE、缺少 usage、流中取消。每个场景都实际捕获服务器收到的正文并与预约字节逐字节比较；拒绝预约为零发送，其他场景恰好一次发送，无 SDK 内部重试。

SDK 负责请求序列化和工具参数流聚合。公开 `HttpClientExt::send_streaming` 接收最终正文，JCS 规范化后预约并直接发送同一字节；没有 fork SDK、复制供应商序列化或预约后重建请求。测试同时检查工具调用/结果 ID 配对与图片 URL/detail 保真。

接入时必须保留两项约束：

- SDK 提前 EOF 可能正常结束流，甚至已拼出工具参数，但没有完整终态；必须核对终态和全部工具调用后才能保存可执行响应。不能凭流结束或工具参数出现就执行工具。
- SDK 用零值表示缺失 usage；应保留“未知”，不能把这个哨兵当成实际零消耗。实际报告用量与 token 预算估算分开。

自定义 HTTP 客户端用于 SDK 的 `CompletionModel` 时需实现 `Default`；测试采用拒绝预约的默认实现，真正的发送必须使用显式装配的客户端。生产接入仍需验证这个门禁与 PostgreSQL 预约/三边界状态的组合，以及 `AgentRun` 交接、各角色工具权限、业务回归并删除被替换循环。**7 个接缝场景通过不等于 P4 已完成。**

## 真实验收仍未通过

v7 沿用冻结 `.env` 和 Python 来源，完成137轮、721次工具调用；共有138次物理预约、137次用量响应，请求35507–118315字节。第91轮后连续46轮没有成果、关系、处置数量或阅读缺口变化，人工停止，终态为 SIGTERM / exit -15。只有34条记录、8条处置、0关系、0轮独立复核。

v7 终态的离线诊断还能列出36项当前范围缺口（含12项未处置来源），4页、每页最多2048字节；全局前50项仍无当前范围条目，说明不能只看全局首页。诊断只投影内存对象，不改写或恢复旧检查点。这说明有界请求与局部缺口工具尚未保证主流程持续产出。不能把此次恢复改造或 SDK 接缝证明成提取停滞已经解决；P1 的附表/关系完整性、P2 的32项真实语义复验、O1-S/O2 整稿和同版本出件继续开放。v7 使用旧的冻结二进制，不包含本轮字段反馈和三边界合同，也不能直接用新二进制恢复。

证据：[验证清单](../../artifacts/bid-full-sample/agent-runtime-recovery/verification.json)、[Rig 七场景结果](../../artifacts/bid-full-sample/agent-runtime-recovery/rig-chat-seam.json)、[v7 性能与终态](../../artifacts/bid-full-sample/real-run-v7/performance.json)。本机完整日志为 `/tmp/kb-agent-runtime-implementation-606644sg/`。HEAD 与原 index 摘要保持不变；没有操作现有运行数据库、部署或提交 Git。

## 解析 v3 交接与当前范围清单

用户随后确认解析模块已经完成，允许修复交接问题。本轮复用 Python DocReader，补齐以下缺口：

- 每轮请求直接携带从当前范围、持久成果及本次交付推导的有界 `work_state`。主提取和 reviewer 分别使用自己的覆盖账本；清单不提交证据，响应完整后才确认本次阅读。v7 离线投影显示12项未处置来源和23项未保留成果引用，旧检查点字节未改写。
- `search_sources` 同时检索正文和独立稀疏网格，返回真实来源、表单/行列、`read_form` 使用的密铺偏移及格内 UTF-8 命中位置。合并覆盖位不会重复命中；完整结果按字节预算分页；搜索不增加阅读覆盖。
- 样稿冻结脚本删除 `locator.cells` 旧分支，从 Python `unit.grid` 保存 schema 3 表单、原始宽度及来源身份，显式使用招标链相同的 builtin engine。否则新版解析会使该脚本漏掉全部表单。已有冻结文件禁止覆盖。
- 修复 DocParser 格式差异及样稿示例中仍使用旧 Journal 参数的测试。没有新增生产解析器、数据库表、migration 或模型配置。

验证结果：

| 检查 | 结果 |
| --- | --- |
| Bidding library | 139 passed / 4 ignored；两个本轮外部来源诊断另行显式执行 |
| DocParser library | 46 passed；使用临时 loopback gRPC 服务 |
| Python 结构、表格、来源视图回归 | 41 passed |
| 真实文件覆盖和冻结合同 | 12 passed，含106页 BiddingFile及 testdata/bid 文件 |
| 样稿脚本配置与 Word/Excel 真实解析冻结 | 5 passed |
| 样稿本地预约账本重启测试 | 1 passed |
| 真实 v3 来源到 Rust Agent 工具 | 1 passed：148来源、42表单，641非空锚点全部可检索，1352锚点全部可读并可引用 |
| v7 当前清单离线投影 | 1 passed；请求包限制2048字节，原文件未改写 |

Python 首次来源视图测试因沙箱禁止监听端口失败；使用自有 loopback 测试服务后通过，未访问现有服务。共享文件曾出现 `tender_process.rs` 的 `unused_mut` 和 `tender_upload.rs` 格式差异；这些历史失败已消除，当前 Bidding/DocParser 全目标严格 Clippy 及全仓 fmt 通过，最新证据见下节。

v8 已用新目录 `/tmp/kb-real-tender-v8-948dzgen/` 启动，输入重新通过统一服务冻结，使用归档二进制、Journal v2 和 `.env` 中的 grok-4.6 / Chat Completions。该运行随后因持续停滞人工停止（SIGINT，exit 1），见下节终态；最终提取、32项语义复验及 DOCX/PDF 仍待完成。运行只使用归档二进制及其工具合同；不能用后续源码直接恢复。

证据：[本轮验证](../../artifacts/bid-full-sample/agent-work-context/verification.json)、[v7 离线清单](../../artifacts/bid-full-sample/agent-work-context/v7-work-state.json)、[v8 启动](../../artifacts/bid-full-sample/real-run-v8/start.json)。完整本机日志位于 `/tmp/kb-agent-work-context-wf4alwtb/`。

## 共享驱动与候选来源共存修复

提取/编制已共同调用 `agent_runtime::drive`，两处重复的准备、预约、重试、保存外层循环已删除。领域适配器继续负责工具、覆盖、角色切换及发布校验；共享驱动只控制 I/O 三边界和取消。预约完成后、完整响应保存后再次检查取消，避免进入后续模型或工具调用。新增测试证明这两个边界取消后能够恢复，已收响应不会重调模型。没有新增持久化结构、表、migration 或配置。

这一批共享宿主驱动切片尚未使用生产 SDK；后续 Chat 流解析接入见下节。该批次尚未接 SDK 请求序列化和 AgentRun；请求序列化的后续接入见下文，P4 仍未完成。

v8 运行1180.38秒后人工停止，完成67轮、366次工具、68次物理预约；31条记录、8个来源处置、0关系、0轮复核。最后成果数量变化在第18轮，最后阅读/数量进展在第32轮，分别连续49轮、35轮没有变化。请求36259–117758字节。10来源/4表单的当前范围仍缺10项来源处置。停止为 SIGINT 后 exit 1，锁文件已清理，不再作为运行中任务。

离线逐请求分析发现：第33、36、56轮携带完整当前来源，但没有候选明细；后续大页候选查询会挤出原文，重读再挤出候选。这解释了一个实现层循环条件，不能证明它是模型停滞的唯一原因。

本轮修复：

- 活动范围内 `inspect_analysis` 默认只取该范围的记录、相关关系及处置；显式 ID/来源仍可查询范围外对应物，schema 说明同步更新。旧工具摘要的检查点不能以新合同恢复。
- 删除历史时仍按完整 assistant/tool/image 协议组处理，优先移除不含当前独有来源的组。预算上限不放宽，最新未确认组保持完整。
- 候选页先经真实请求构造器试装，必要时减小完整条目数并返回精确 `next`，保证当前已在窗口内的来源继续可见。查询/试装不提前提交读取覆盖，只有实际交付的条目进入后续确认。
- 三个真实快照均逐页取完当前20条候选，无重复或来源挤出。第36轮旧查询返回31条全局候选后只剩62/105个来源锚点；新查询分两页、每页10条，保留105/105个锚点及原有正文。没有按样稿条目数或来源名称设置生产逻辑。

最终验证：Bidding 库 **141 passed / 5 ignored**；20项 Schema/baseline 合同、五项隔离 PostgreSQL 测试再次全部通过且所属容器已删除；三个上述离线回放显式通过；Bidding/DocParser 全目标严格 Clippy、全仓 fmt 通过。解析 v3 的先前真实逐格测试保持其原证据，本次未重复耗时解析。离线回放没有模型调用，也没有改写旧检查点；真实持续产出、独立复核、32项语义发现与完整 DOCX/PDF 仍开放。

证据：[共享驱动与窗口验证](../../artifacts/bid-full-sample/agent-shared-driver/verification.json)、[第36轮分页回放](../../artifacts/bid-full-sample/agent-shared-driver/v8-scope-replay-36.json)、[v8 终态](../../artifacts/bid-full-sample/real-run-v8/terminal.json)、[上下文诊断](../../artifacts/bid-full-sample/real-run-v8/context-forensics.json)。本机完整日志 `/tmp/kb-agent-shared-driver-wov2bamm/`。


## Rig 生产 Chat 流解析接入

提取与编制的 `ConfiguredModel` 现在共同调用 `agent_runtime::chat`，使用 Rig 0.42.0 公开的 `send_compatible_streaming_request`。SDK 接收已经预约的原始请求字节，负责 SSE 解码和工具参数聚合，发送阶段不再序列化正文。旧的 `tender_analysis::agent::provider_turn` 包装删除；知识库其他调用仍使用的公共传输保留。

HTTP 适配只保留现有合同需要的控制：关闭重定向/HTTP 重试，一份预约最多允许一次物理发送，非成功响应先保留状态码、不读取或记录可能截断的错误正文。超时覆盖完整请求/流，取消通过丢弃同一个 future 停止 I/O，没有额外后台任务。无效终态、参数、重复或缺失供应商调用 ID 都不能保存为可执行响应；零值 usage 哨兵记为未知，缺失的可选用量字段不补零。SDK 会把空详情对象的 cache/reasoning 成员补为0，适配器将这类0详情保守记为未知（包括无法与缺失区分的显式0），避免伪造已报告用量。

首次真实 loopback 测试发现 SDK 会将带工具调用的原始 `stop` 归一化为 `ToolCalls`。接入已同时检查 SDK 保留的 `raw.finish_reason`，要求供应商明确返回 `tool_calls`，没有放宽原合同。损坏 JSON 帧和流中错误同样拒绝，不把已经聚合出的部分工具当成成功。

SDK/适配版本以 `runtime_adapter = rig-chat-0.42.0/1` 纳入冻结提取配置和编制合同，Rust 与所属 baseline 均校验；Journal 格式仍为 v2，因为三边界结构没有改变。旧运行缺少新适配身份时不能静默恢复。没有新增 migration 文件、表、模型配置或解析器；历史轨迹离线投影只读取旧模型/预算，在内存中使用当前构造器，不冒充旧合同恢复。

本轮验收：

- 生产传输入口的自建 loopback 服务 **18场景通过**：正常工具、usage 缺失/部分缺失、中文和交错工具参数片段、提前 EOF、缺少结束原因、缺少/重复调用 ID、损坏参数、length/stop、流中错误、损坏 JSON、400/413/429/503 的未完整错误正文、超时。每一场景服务器收到正文都与输入字节逐字节一致，且只有一次发送。
- Bidding library **141 passed / 6 ignored**，其中本轮网络测试已另行显式执行；旧适配身份/缺失身份的拒绝已扩展到恢复回归，拒绝不产生额外模型调用。
- Schema/baseline **20 passed**；五项现有隔离 PostgreSQL 恢复回归再次通过，所属容器已删除。Bidding/DocParser 全目标严格 Clippy、API/worker 全目标 check 及全仓 fmt 通过。第一次临时库探针误认初始化阶段的 Unix socket 为最终就绪，已改为探测 TCP 后复跑通过，保留原失败日志。

证据：[生产 Rig 传输验证](../../artifacts/bid-full-sample/rig-provider/verification.json)。本轮没有真实模型重跑，也不证明32项语义发现关闭。该批次仍缺 SDK 请求序列化与 AgentRun；请求序列化的后续接入见下节。完整来源提取、独立复核和 DOCX/PDF 仍待真实验收。


## Rig 请求序列化与最终正文预算

提取与编制的请求准备现在共用 `agent_runtime::chat::prepare`。领域层继续提供角色指令、受限工具、当前证据/候选及进度；Rig 通过公开的兼容供应商构造器完成最终 Chat 正文序列化。两处手写 model/stream/max_tokens/tools 请求对象已删除，没有复制 SDK provider 序列化。

准备专用 HTTP 客户端没有网络后端，也不读取凭据；它通过有界通道交回 SDK 已生成的 JCS 正文，随后整个暂停的流随准备结束丢弃，没有后台任务。共享宿主先按该正文校验预算，再保存检查点并预约；发送继续使用公开的原始请求入口，传同一字节，不在预约后重建 SDK 请求。纯准备作用域关闭 SDK 的正文 TRACE 日志，宿主保留原有字节数/耗时元数据。

兼容供应商扩展采用 SDK 默认的 `max_tokens` 行为，遵循冻结 Chat 合同；不使用 OpenAI provider 的模型名称推断，不临时更换模型或新增供应商字典。三个任意模型名称的离线准备测试覆盖名称原样保留、输出预算字段、reasoning 配置、工具 schema、工具调用/结果 ID、中文、图片 URL/detail、JCS 与重复构造字节一致；不访问示例域名或外部服务。

SDK 将系统指令序列化为内容数组，冻结提示摘要改为 SDK 实际内容结构，编制合同及所属 baseline 同步使用该结构，并显式冻结 SDK 加入的 `stream_options.include_usage=true`。适配身份升级为 `rig-chat-0.42.0/2`，Journal 结构仍是 v2。主提取/复核的历史字节、完整正文大小与 token 估算均在 SDK 序列化后计算；候选分页及来源保留的试装也共用该构造器。没有增加模型调用、数据库表或 migration 文件。

最终库回归 **142 passed / 6 ignored**；其中本机 HTTP 测试已显式执行，**18场景通过**，其输入现已全部由生产 SDK 准备函数构造，服务器仍只收到一次、且收到字节与准备结果完全相同。五项隔离 PostgreSQL 恢复测试再次通过并清理所属容器，Schema/baseline 合同 **20 passed**。Bidding/DocParser 全目标严格 Clippy、API/worker 全目标 check 及全仓 fmt 通过。修改过程中两个旧测试仍把系统内容当字符串、一个测试跨 await 持有同步锁，已按 SDK 实际结构及缩小锁作用域修正，没有改用宽松业务断言。

证据：[SDK 请求与恢复验证](../../artifacts/bid-full-sample/rig-request/verification.json)。**该批次完成 Chat 协议的生产接入，当时尚未接入 AgentRun；后续进度见下节。真实语义及完整 DOCX/PDF 验收继续开放。**


## AgentRun 多轮步进与有界检查点

生产依赖现使用官方 `rig = "=0.42.0"` facade，启用 `agent/reqwest/rustls`。提取和编制均通过共享驱动调用 `AgentRun::next_step → model_response → next_step(CallTools) → tool_results`，同一活动窗口复用多轮 SDK 会话。业务层仍决定来源范围、工具语义、证据交付、独立复核与完成条件。

每次 `CallModel` 给出的系统指令及来源/工具历史，均通过 SDK 正向转换后与最终请求中的对应消息核对；当前进度及稿件摘要只进入本轮请求，不积累到 SDK 历史。实际裁剪、图片交付或角色交接导致历史变化时，使用公开构造器重建会话。活动 SDK JSON 受现有 `max_context_bytes` 限制，工具提交后超过上限则释放活动状态，下一轮按保留的证据窗口重建；全局轮次、工具、阅读和物理调用预算继续累计，不增加模型摘要请求。

新增测试暴露并修复了通用 Chat 反向转换将 system 降为 user 的行为，宿主保留系统角色。空 assistant 文本按相同语义规范化比较，避免编制工具轮每次误判为窗口变化。未知或当前角色未开放的工具走 SDK Skip；SDK 同时抑制该批其他调用，宿主逐个返回其预解析结果，全部被抑制调用均不执行业务工具，也不增加阅读凭证。工具 ID 保持供应商原值，错误参数仍由既有工具返回字段反馈，不自动修成业务输入。

检查点升级至 **v3**，适配身份为 **`rig-chat-0.42.0/3`**：既有 Journal 增加 `session`，准备边界保存 AwaitingModel 状态；完整响应仍先独立保存，然后才喂给 SDK 并执行工具。恢复收到响应的检查点不会再请求模型。所属 baseline 校验活动状态的包装结构、消息布局、字节限制以及响应边界不可修改会话，未新增表或 migration 文件，未操作现有数据库。SDK 内部状态不做字段改写；冻结 SDK/适配版本后，旧版本检查点不得直接恢复。

验收：**145项 Bidding 库测试、20项 schema/baseline 合同测试、18场景生产 Chat 回环测试、5项隔离 PostgreSQL 恢复测试通过**；严格 Bidding/Docparser 全目标 Clippy、API/worker 全目标编译及 workspace fmt 通过。提取和编制现有宿主测试均断言 SDK 活动轮次实际大于1；新测试覆盖序列化恢复、多轮复用、动态进度不积累、角色/窗口交接、剩余预算及完整抑制反馈。隔离容器已清理。Docparser 的46项库测试亦通过：首次受限沙箱中8项回环 gRPC 测试无法绑定端口，允许本地回环后全部通过，未据此修改解析代码。

证据：[AgentRun 验证清单](../../artifacts/bid-full-sample/rig-agent/verification.json)。本批没有真实模型调用。v8 保持已停止状态；32项语义发现、主提取完成、实际独立复核、按招标要求生成的完整可编辑模板及同版本 PDF/report，均需新运行验收。

后续 v9 已用当前统一解析服务重新冻结148个来源单元、42个表格并归档新二进制；配置重新读取 `deploy/.env`（grok-4.6 / https://ai.zleiwork.cn）。首次启动被自动审批拒绝，随后用户明确回复“允许”。已重新核对文件、二进制及环境摘要并启动 v9，初始调用 HTTP 200。先前“未发送”仅描述等待授权时的状态。见 [准备记录](../../artifacts/bid-full-sample/real-run-v9/start.json) 和 [审批拒绝记录](../../artifacts/bid-full-sample/real-run-v9/approval-rejection.json)。


### v9 新冻结与独立来源验收引用核对

真实外发等待明确目的地授权期间，离线验证了新冻结输入：统一解析服务的主招标 PDF 全页/附表、三份 DOCX 及 XLSM 结构回归 **12项通过**，大型产品手册1项未纳入本次范围。另将5份历史独立来源报告与新冻结逐项比对：**201条 UTF-8 原文引用、321条网格引用、16处已复核原页摘要、106页来源归属及106张实际原页图像摘要/尺寸全部匹配**。历史冻结与新冻结 JSON 的摘要不同，不能直接声明文件一致；此次核对证实验收索引的具体引用仍可用于新来源，没有重写旧报告或据此关闭任何语义发现。

证据：[v9 离线来源验证](../../artifacts/bid-full-sample/real-run-v9/offline-source-verification.json)。解析/来源缺陷在此范围内未复现，未修改解析代码；32项提取发现仍开放；该离线验收批次的真实外发为0，后续已授权的 v9 真实运行见下文。

用户后续已明确授权本次具体文件和目的地，v9 已于2026-09-10 UTC启动真实提取，见 [授权记录](../../artifacts/bid-full-sample/real-run-v9/approval.json)；离线来源验收保持独立范围，不作为真实语义通过结论。

### v9 停止与候选查询复现

v9 已在第45轮检查点停止：运行1043.91秒，34条记录、10个来源处置、0关系，独立复核未开始；第25–44轮仅查询已有候选，连续20轮没有新增成果或来源覆盖。后半段未观察到503，重复候选全文查询多次因无法与当前原文共存而失败。已保留全部请求及检查点，正在离线复现并修复目录/详情取回；不能视为真实语义或整稿验收通过。用户对 `.env` 目的地的具体外发授权持续有效。证据：`artifacts/bid-full-sample/real-run-v9/terminal.json`、`stagnation-report.json`。

## 2026-09-10 候选目录与完整详情分离

2026-09-10 候选目录/详情修复：`inspect_analysis` 增加 `view=index/detail`。无 ID 查询默认返回目录（ID、引用、原候选标签及来源），有 ID 默认返回完整详情；目录不计阅读或复核覆盖。详情仍按当前候选摘要确认且完整分页，预算不变。v9 两批真实查询离线回放保留全部当前原文，目录中12/14条不同候选均可逐条完整取回。库147项、合同20项、隔离 PostgreSQL 5项及严格 Clippy/格式通过；新合同须新建运行，32项语义验收仍开放。证据：`artifacts/bid-full-sample/candidate-index/verification.json`。

## 2026-09-10 大范围的安全拆分

2026-09-10 已补齐安全拆分：工作笔记新增有界 `deferred_sources`，允许将活动范围缩小并释放旧窗口；移出的每个来源及已有成果引用必须保留。待处理来源只能通过重新纳入活动范围移出列表，列表未清空禁止主提取提交及复核提交；局部完成、全局阅读、来源处置和独立候选核查门槛不变。v10 离线投影将12个活动来源缩为1个、保留11个待处理来源，完整48格网格和对应原页可同时发送，业务成果和原检查点不变。库148项、合同20项、隔离 PostgreSQL 5项、Clippy/格式通过，真实持续产出仍待验证。证据：`artifacts/bid-full-sample/work-split/verification.json`。


## 2026-09-10 先释放已交付导航，保留原文证据

v11 的真实第33轮请求包含4张完整网格及28274字节旧来源索引；6次记录写入后，第34轮只保留2张网格，旧来源索引却全部保留。原有整组裁剪把大索引和同组唯一原文绑定，造成可避免的原文丢失和重读。

请求超限时，先把已交付的旧 `source_index`、`search_sources` 和 `inspect_analysis(view=index)` 成功结果替换成明确省略标记，再按原规则裁剪整组。完整来源、网格、原页、集合元数据、候选详情和错误结果保持原样，最新尚待交付的整个工具组不改。协议调用 ID、阅读账本、预算和业务校验不变；使用已有 Rig 会话重建路径，不增加模型总结、配置项、表或 migration。主提取与独立复核共享此策略，提示词说明省略标记不是证据，变更提示词摘要后须新建运行。

真实请求离线重放先红后绿：修复前仅保留2张网格、请求107508字节；修复后全部4张网格及当轮写入结果原样保留、请求95213字节，原检查点未改。库149项通过（9项需显式夹具/服务的测试默认忽略）、合同20项、5项隔离 PostgreSQL 恢复回归、严格 Clippy、workspace fmt 通过。证据见 [navigation-history/verification.json](../../artifacts/bid-full-sample/navigation-history/verification.json)。数据库回归验证既有三边界、重放和所有权合同，不代替真实导航轮次的语义验收。

v11 在第48轮检查点保存后停止以切换已验证修复，最终35条记录、12个来源处置、0关系、0复核。该轮仍有写入，且实际发生180秒超时重试；不能把停止描述为连续空转或宣称服务超时已解决。v12 使用原 `.env` 配置和新运行身份复验；32项语义发现、独立复核及完整 DOCX/PDF 仍开放。


## 2026-09-10 候选详情的角色独立回执

v12 第54–89轮检查点的记录、关系、来源缺口及处置数量均不变，多轮重复取回已存详情后才继续写入。观察到188次完整详情对象返回，其中146次为此前已返回的相同版本；重复可能用于必要比较，不能将全部重复都归为无效调用。原主提取不保存候选详情回执，历史退出窗口后无法从目录判断已收到哪些版本。诊断见 [candidate-inspection-diagnostic.json](../../artifacts/bid-full-sample/real-run-v12/candidate-inspection-diagnostic.json)。

主提取和 reviewer 现在各自复用已有 `Coverage.candidate` 保存完整详情摘要。目录新增 `detail_received`，仅表示本角色在已完成模型轮次中收到过当前版本，不表示语义正确、比较已完成或原文已读。同一批工具中刚取回的详情仍标为未交付；记录修改后旧摘要不再匹配。目录查询自身不创建或刷新回执，另一角色不能继承回执。工作交接反馈明确：已有成果只需保留其引用，不要求为登记引用重新查询全文；原有完整阅读、成果引用和独立复核门槛保持。

两项新增回归覆盖角色隔离、修改失效、检查点往返、未交付与已交付边界；分页回归确认仅完整返回的对象获得回执。151项库测试、20项合同、5项隔离 PostgreSQL 恢复回归、严格 Clippy 和 workspace fmt 通过。使用现有 Journal v3 和 JSON 字段，无新增表或 migration。证据见 [candidate-receipts/verification.json](../../artifacts/bid-full-sample/candidate-receipts/verification.json)。

提示词和工具摘要已改变；v12 已停止，v13 使用新身份和新归档程序复验，不混入旧代码或重写旧检查点。本项尚未证明实际消除循环，32项语义发现、独立复核及完整 DOCX/PDF 验收仍开放。


## 2026-09-10 超大旧图片组不再挤掉完整网格

v13 第68轮交付两张原页并保存6条记录后，第69轮原逻辑既丢掉了两张较早的网格，也丢掉了图片组，请求只剩48618字节。图片 Base64 本身已大于65536字节的冻结历史额度；先删除较早网格无法使该图片组留下。该问题不同于已经修复的旧导航载荷。

保留既有完整协议组淘汰方式：先选证据冗余的已交付组；没有冗余组时，优先移除实际图片载荷总长已超过历史预算的旧组，再回退到原顺序。只按已有配置和缓存的真实载荷比较，不猜模型窗口、不增加配置项或 migration。最新待交付组、所有已提交阅读回执、原页缓存及业务成果保持；超大图片本身不能常驻这个历史窗口，需要像素时仍须重新读取。

真实第68→69轮离线回放先红后绿：2张完整网格和62字节相关正文恢复保留，请求71136字节，满足原预算，当轮6条写入结果保持原样，原检查点未变。普通回归另覆盖多图合计超限、较大配置下原顺序及最新图片组不被淘汰；v11 导航回放也通过。154项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、严格 Clippy、workspace fmt 和样稿编译通过，临时数据库容器已删除。证据见 [image-history/verification.json](../../artifacts/bid-full-sample/image-history/verification.json)。

最终修复保持主提取和 reviewer 的提示词、工具、预算及 Journal 合同不变。生产校验器已确认 v13 原配置摘要、来源、SDK 会话和已预约正文兼容。原实例停稳后，检查点及全部调用记录逐字节复制到独立恢复目录，用归档修复程序从第104轮继续，52条记录和105次调用不重置；原未完成请求沿用相同正文并累计第二次预约。见 [恢复验证](../../artifacts/bid-full-sample/real-run-v13-resume1/startup-verification.json)。改变提示词、工具、来源或预算的运行仍须新身份；这次兼容的内部缺陷修复不需要从头提取。独立复核、32项语义问题及完整 DOCX/PDF 验收仍开放。
