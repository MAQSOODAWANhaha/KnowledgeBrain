# 真实招标文件完整模板样稿验证

2026-09-11 UTC 当前进展：real-run-v14已使用当前已验证程序及.env的grok-4.6＋Chat＋low启动完整106页空候选提取与独立复核，来源、原图、预算均已核对，不导入旧检查点或人工答案。连续技术第55–63页此前已完成两轮局部复核（50条记录、0发现）；标签清空、未勾选条件负例均已闭环。复杂附表和跨范围关系的整体语义、完整32项及同版DOCX/PDF/报告仍待验收。未改配置、新增migration或修改生产逻辑；223项库测试、20项合同及fmt/Clippy/编译证据保留。 详见[统一方案](../../plans/bidding/agent-runtime-rig.md#55-本次修复的实施门槛)。

**2026-09-10 前序修复记录：本地功能验证通过，独立复核已完成局部比较，未完成最终提交，运行已停止；完整验收尚未通过。** 已分离来源权限与局部焦点，自动维护成果/未解决引用，主提取、独立复核、编制和稿件复核共用进展与有界恢复策略。默认连续无进展6轮、焦点24轮、重规划2次；记录局部执行阻塞后允许转向独立范围，连续6轮仍未交接则在工具提交边界停止。重启、重复读取、笔记改写和任务改名不能刷新额度；有效局部写入可以完成当前动作。执行失败单独保存并阻止最终发布，不冒充来源缺项。候选当前版本参与窗口保留；各 reviewer 保留自己的冻结原文回执，修改后的候选/稿件仍按摘要核查。旧手抄引用输入及编制 `remember` 路径已删除，未新增 migration，既有 baseline 同步检查点与预算 JSON。

前序验证：176项库测试（11项忽略）、20项合同、1项真实检查点离线提交诊断、严格 Clippy、workspace fmt及样稿编译通过；既有6项隔离 PostgreSQL结果保留，本次未改SQL。首次3来源短测在21轮停止，11条记录、0关系，发生一次超时后重试成功；第二次 v2 在32轮停止，29条记录、12条关系、2个来源处置，四组重点引用由真实Agent产生，但独立复核未开始。v2请求42214–353819字节，含必要原页图片，无503或超时。轨迹还暴露无响应要求被迫指定渠道、同类型合规属性的不同条件被拒绝、工作引用格式说明不足；均已修复并验证。未新增 migration，未修改实际 `.env` 或旧检查点，未执行暂存操作。包含全部修复的 `response-contract-trial` 已以新身份、空候选重测第11、16、17页的3处来源，模型与预算仍来自 `deploy/.env`；启动合同已核对，结果待验。该试验在34轮保留32条记录、21条关系后，进一步定位到重复读取会触发无实际缺口的 pending_delivery 阻塞。已删除这条冗余判断，尚未交付的原文/候选仍按真实缺口阻止交接；172项库测试、20项合同及Clippy/格式/编译通过，SQL未因本项调整。`response-contract-resume1` 已在第101轮由无进展/交接保护停止：主提取完成，41条记录、21条关系、3项来源处置；独立复核收到65个候选当前版本后仍重复读取，两次重规划无效，0轮复核、1项执行阻塞。已保留终态和计数，未将空缺口等同于语义通过。现补充复核专用完成指引与按实际缺口生成的下一动作：有问题逐项保存、无问题完成原范围后提交空草稿；执行阻塞仍禁止提交。173项库测试、20项合同及Clippy/格式通过，无新工具/配置/migration；提示词已改变，`reviewer-completion-trial` 已以新身份、空候选复测；启动核验确认实际配置、工具和预算未变，旧终态未改。该试验在第85轮到达诊断时限并取消：主提取41条记录、28条关系、3项来源处置；reviewer完成一个局部范围、收到72个候选当前版本，仍未提交最终结论。除精确读取位置反馈、局部复核排除不相关待办外，现新增 complete_review_check，在既有进展账本中记录当前候选的无问题比较；重复版本/改写结论不能续额度，来源、当前版本、执行阻塞及最终全局门槛保持不变。176项库测试、20项合同、3项.env启动测试及Clippy/格式/编译通过，无新表、migration、检查点字段或环境变量。新工具/提示词采用新身份：clean-review-trial 在补充明确授权后实跑656.16秒，停于第25轮：独立收到72个候选当前版本，完成41个记录版本的局部比较，46次重复比较被去重；28条关系和3项来源处置仍未形成比较结论，0轮最终复核。定位到当前焦点已完成但执行反馈仍要求比较该焦点，已增加焦点剩余数和转向未完成引用的派生反馈；176项库测试、20项合同、Clippy/格式/编译通过。clean-review-resume1 保留原检查点、预约正文及计数兼容续跑，在第40轮完成全部72个局部比较，但此后反复读取，0次完成范围、0次提交复核，第58轮进入执行阻塞，第64轮耗尽交接额度后停止。只读第54轮检查点副本的正式工具调用均通过，证明当时接口可用；离线副本不作为真实复核结果。最终结束行为仍未解决，本轮后续复杂附表及完整样稿重测未启动。此前自动审批拒绝已由用户补充明确授权解除。旧运行、原始来源和计数未改。复杂附表、完整106页独立复核、32项语义发现及完整 DOCX/PDF仍未通过。证据：[验证记录](../../artifacts/bid-full-sample/loop-repair/verification.json)、[v2真实轨迹](../../artifacts/bid-full-sample/loop-repair/source-scope-trial-v2/result.json)、[上一实跑启动核验](../../artifacts/bid-full-sample/loop-repair/reviewer-completion-trial/startup-verification.json)。

2026-09-09 v8 历史记录：v8 已停止；离线回放确认候选查询与当前原文相互挤出上下文。已实现按当前范围查询、保留来源证据的分页，以及提取/编制共享 I/O 驱动；Rig 已接生产 Chat 请求序列化、流解析及 AgentRun 多轮步进，有界会话和三边界恢复已通过隔离验收；32项语义验收及真实整稿仍待完成。见[最新验证](agent-runtime-recovery-results.md#agentrun-多轮步进与有界检查点)。

v9 已在第45轮检查点停止：运行1043.91秒，34条记录、10个来源处置、0关系，独立复核未开始；第25–44轮仅查询已有候选，连续20轮没有新增成果或来源覆盖。后半段未观察到503，重复候选全文查询多次因无法与当前原文共存而失败。已保留全部请求及检查点，目录/详情取回修复已通过离线及隔离验收；不能视为真实语义或整稿验收通过。用户对 `.env` 目的地的具体外发授权持续有效。证据：`artifacts/bid-full-sample/real-run-v9/terminal.json`、`stagnation-report.json`。

v10 已停止：运行387.94秒，第24轮检查点保存7条记录、8个来源处置、7张已交付原页，0关系；第5轮后没有新增成果，反复读取12个活动来源。目录查询未再复现 v9 的候选共存错误，但完整持续产出仍失败。原文工具返回完整单元格；活动范围只能扩大不能拆小，未充分实现方案的分次处理要求，显式待处理来源与安全拆分已通过离线和隔离验收，真实持续产出尚待复验。证据：`artifacts/bid-full-sample/real-run-v10/terminal.json`、`diagnostic-checkpoint.json`。本轮未生成可验收的提取结果或整稿。

v11 已保存第48轮检查点后停止，耗时1421.51秒，35条记录、12个来源处置、0关系、0轮独立复核。期间仍有写入，不能归类为连续空转；停止原因是已复现旧索引挤掉原文网格，需以新合同验证修复。第33→34轮离线重放由仅保留2张网格改为保留全部4张，请求107508→95213字节，最新工具结果保持原样。库149项、合同20项、隔离 PostgreSQL 5项及 Clippy/格式通过，未调整预算或新增 migration。证据：`artifacts/bid-full-sample/real-run-v11/terminal.json`、`artifacts/bid-full-sample/navigation-history/verification.json`。

v12 已人工停止并保留第152轮检查点：耗时2975.47秒，54条记录、6条关系、20个来源处置，独立复核未开始；最后47个完成轮次没有新增记录，期间仍有读取和导航。导航裁剪改善了原文保留，但未解决完整持续产出。终态见 `artifacts/bid-full-sample/real-run-v12/terminal.json` 和 `diagnostic-checkpoint.json`。

v13 已获明确外发授权并启动，向 `https://ai.zleiwork.cn` 发送同一招标文件的解析文本、网格及必要原页图片，模型和预算只读 `deploy/.env`。归档程序包含候选详情回执及字段校验反馈；启动核对确认供应商、预算、二进制、冻结来源和新提示词/工具合同一致。真实持续提取、独立复核及完整 DOCX/PDF 尚未通过，32项发现保持开放。证据：`artifacts/bid-full-sample/real-run-v13/startup-verification.json`。第104–129轮状态保留在 `artifacts/bid-full-sample/real-run-v13-resume1/progress-snapshot.json`；第129轮后由兼容恢复目录 v13-resume2 续跑，新增15条记录后再次出现重复核查，现已停稳于第164轮，详见下方字节定位修复记录。

2026-09-10 补充：主提取及 reviewer 各自保存完整候选详情的已交付摘要，目录 `detail_received` 显示本角色是否收到当前版本；它不是语义通过标志，修改后失效，同批尚待交付及另一角色的回执不能冒充已收到。交接反馈明确保留已有引用不需重新取回全文。151项库测试、20项合同、5项隔离 PostgreSQL 测试及 Clippy/格式通过，无新增 migration；v12 已停止，新提示词/工具合同由 v13 真实复验。证据：`artifacts/bid-full-sample/candidate-receipts/verification.json`。

2026-09-10 字段校验反馈已补齐：复用原 `ok/error` 封装及语义校验器，以 `INVALID_FIELD <JSON Pointer>: <constraint>` 指明记录、引文、模板单元格及关系的失败位置和约束，失败写入保持原子性。反序列化错误定位到所属容器，不声称每个嵌套 Serde 错误都能定位叶子字段；原文未给出的单位等仍允许为空。153项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、严格 Clippy、workspace fmt 及样稿编译通过，无新增 migration。证据：`artifacts/bid-full-sample/validation-fields/verification.json`。

2026-09-10 原页历史淘汰修复：v13 第68→69轮确认超大旧图片组会先挤掉较早的完整网格，随后自身也被淘汰。现按冻结历史预算优先淘汰必然放不下的已交付图片组，保留较小原文组；离线回放从0张完整网格改为保留2张及相关正文，154项库测试、20项合同、5项隔离 PostgreSQL、Clippy/格式及样稿编译通过。最终修复未改变提示词、工具、来源或预算合同。v13 原实例停稳于第104轮后，以归档修复程序从同一检查点恢复，保留52条记录和105次累计调用；恢复记录见 `artifacts/bid-full-sample/real-run-v13-resume1/startup-verification.json`。证据：`artifacts/bid-full-sample/image-history/verification.json`。

2026-09-10 正文字节定位修复：`read_source` 在保留原文及起止范围的同时返回逐行 `line_spans`，中文与原换行均按真实 UTF-8 字节定位，完整结果按原工具预算分页；兼容恢复仅给已交付历史补充确定性位置，不改已预约请求和阅读回执。真实第120→121轮离线回放保留两页58行完整正文及附表，请求116767字节；157项库测试（9项默认忽略）、20项合同、5项隔离 PostgreSQL、Clippy/格式及样稿编译通过，无新增 migration。v13-resume1 停稳于第129轮，52条记录、0关系、20个来源处置、0轮独立复核，累计131次调用。原状态已逐字节复制到 v13-resume2，生产合同校验通过；自动审批首次拒绝后，用户针对具体目的地和载荷再次明确回复“继续,允许”，现已从第129轮恢复，原预约正文保持不变并累计第二次调用；启动核验见 `artifacts/bid-full-sample/real-run-v13-resume2/startup-verification.json`。第130轮实际请求已验证包含两页58个逐行位置；随后新增15条记录，达到67条。第136轮后连续28个完成轮次无新增记录、关系或来源处置，期间73次候选详情/目录查询、29次搜索，未尝试写入关系。原文持续保留，但候选详情在有界窗口内反复取回；该相关性尚不能单独证明模型循环的根因。恢复实例运行777.78秒后安全停于第164轮，保留67条记录、0关系、20个来源处置及167次累计调用；未进入独立复核。逐行位置已验证可用，整体持续提取仍未通过，不能继续以相同正文重试或放宽预算冒充修复。证据见 `artifacts/bid-full-sample/real-run-v13-resume2/repeated-inspection-observation.json`、`terminal.json` 和 `line-spans-request-verification.json`。证据：`artifacts/bid-full-sample/source-line-spans/verification.json`、`artifacts/bid-full-sample/real-run-v13-resume2/preflight-verification.json`。32项语义发现及完整 DOCX/PDF 验收仍开放。

2026-09-10 停滞进一步定位：四组简单引用的双方完整详情在7个真实请求中同时可见，生产关系工具的离线内存副本验证全部通过，单组详情仅1116–1288字节；不能再将零关系简单归因于窗口装不下。28轮内257份详情只有25个版本，工作笔记与17项缺口不变。当前缺口是局部任务推进和停滞恢复，候选历史保护不足会加重重复，但不是充分解释；模型内部选择原因仍不可由轨迹证明。详见[定位报告](agent-loop-diagnosis.md)。本次未修改生产逻辑或新增外发，修复方向尚待短范围真实验证。

## 历史结论（2026-09-09，v7）

当时的 v7 采用显式工作范围诊断和复核草稿工具，完成137轮后仍连续46轮无成果/覆盖进展，已人工停止；沿用 `.env` 和同一冻结解析来源，[终态与性能证据](../../artifacts/bid-full-sample/real-run-v7/performance.json)已保存。只有34条记录、8条处置、0关系和0轮独立复核，整稿未验收。

前轮 v6 请求已缩小至最大118281字节，但86轮后仍为39条未复核记录、0关系和0复核；连续57轮无业务写入，已人工停止并保存证据，见[本轮诊断](extraction-performance.md#v6-有界请求后的实际停滞)。完整 Agent 样稿仍未通过验收。

历史 v5 已终止：22 分 23 秒、30 轮、200 次工具调用、33 次物理调用，27 条未复核记录，0 条关系、0 轮复核；最终为 `AGENT_PROVIDER_UNAVAILABLE` / HTTP 503。请求从 32,048 增至 784,403 字节，性能与终态证据见[真实 v5 诊断](extraction-performance.md)。32 项独立语义发现仍开放。

[Rig 完整方案](../../plans/bidding/agent-runtime-rig.md)已批准并保存，提取/复核已部分实现，Journal 三边界与 Rig Chat 接缝已通过隔离验证；共享宿主驱动已接提取/编制，生产 Rig AgentRun 已接入并通过隔离验收，详见[验证记录](agent-runtime-recovery-results.md)。后续按该方案处理有界上下文、逐范围提取、附表关系与独立复核，再生成按招标要求组织的完整模板及同版本 DOCX/PDF。模型及凭据只读 `deploy/.env`，用户明确的外发授权持续有效。

现有合成来源的 API/consumer/Agent 生成至 ONLYOFFICE 保存重开已验证；人工参考稿和合成稿均不能作为真实 Agent 整稿交付。以下记录按批次保留，旧模型、测试计数、运行启动及当时待接线/待授权项不覆盖本节状态。

## 历史运行与诊断

后续整稿已具备无探针标记的办公整理入口：正常界面更新目录→实际保存→关闭重开→同版本 PDF，并比较目录之外的正文字符。两页合成验证及原完整编辑回归通过，详情见[办公结果](onlyoffice-product-results.md#2026-09-09-只更新目录的-docxpdf-出件验证)；仍须对真实 Agent 整稿执行，不能将合成文件作为交付稿。

**2026-09-09 v5 启动记录（现已终止）。** 修复完整 SSE 响应后仍等待 HTTP EOF 的缺陷后，在 `/tmp/kb-real-tender-v5-g7k96c_x/` 启动新提取，复用统一 Python service 的冻结输入及 106 张原页，预算与 v4 相同。未导入 v4 分析或重置其已耗尽边界。最终 HTTP 503 及性能统计见上方当前结论；不能断言此前 SSE 缺陷就是 v4 超时原因。

异步传输现在按完整 SSE `[DONE]` 事件结束读取，跨字节分块和 LF/CRLF 均通过本地真实服务回归；正文中的同名文字、缺少事件终止空行的片段不会提前完成。保留原请求字节、180 秒时限、错误状态及已有工具解析；不增加重试或改变模型。质量证据见 [`SSE 边界修复`](../../artifacts/bid-full-sample/provider-diagnostics/sse-terminal/)。

**v4 之后的小型 Rust 诊断记录。** 重新读取 `deploy/.env` 后，复用正式 `provider_turn`、相同模型/凭据解析及 180 秒时限进行无招标内容的诊断：`auto` 控制约 4.7 秒返回成功响应但没有工具；指定一个合成函数约 4.9 秒返回正确工具及参数。生产提取使用 `tool_choice=required`，这两次小请求不等同于真实提取验收，也不能据此重置第 19 轮预算。此前 Python 的 403 不能作为当前 Rust 接口不可用的结论；真实 423,721 字节请求超时原因仍未确定。保留失败的本地临时程序链接/运行证据，改由临时 Cargo 项目统一依赖后完成诊断，未修改产品传输。见 [`Rust 诊断证据`](../../artifacts/bid-full-sample/provider-diagnostics/rust-readiness/)。

**2026-09-09 v4 终止记录。** 启动时 `.env` 的 `grok-4.6` / `ai.zleiwork.cn` 在第 19 轮耗尽三次尝试并返回 `AGENT_TURN_TIMEOUT`（每次 180 秒）。本轮累计 23 次预约调用、59 条未复核记录，仍有 93 个来源阅读区间未完成，没有复核或 DOCX 输出。已保留原检查点和计数，不通过重置尝试次数绕过预算。随后独立 Python 诊断的模型清单及一次无招标内容的最小工具调用均返回 HTTP 403；这些请求使用当前 `.env`，但不是 Rust 提取传输，不能据此断言超时原因是凭据、额度或模型权限。已请用户核对服务可用性/配置，外发授权仍有效，无须再次授权。

本轮只读证据及明确标记的未完成分析见 [`real-run-v4`](../../artifacts/bid-full-sample/real-run-v4/)。原有 32 项验收发现未关闭。以下“正在运行”说明保留为此前启动记录，不能覆盖该轮终止状态；最新运行结论以本页顶部运行状态为准。

**v3/v4 启动与授权记录。** 用户回复“允许发送”后先以 `gpt-5.6-sol` 启动 `/tmp/kb-real-tender-v3-oxmowusm/`，推进至第 20 轮、10 条事实记录后遇到 HTTP 524。修复本地验收 Journal 的单轮重试计数后，恢复入口重新读取发现 `.env` 模型/服务已变为 `grok-4.6` / `ai.zleiwork.cn`；冻结配置校验正确拒绝混用旧检查点，未发起该次模型请求。现使用全新目录 `/tmp/kb-real-tender-v4-oigx_mvd/` 按当前配置运行。未修改 `.env`、临时覆盖模型或复用旧分析结论。

两轮均复用统一 Python 服务的冻结输入和 106 张原页；输入 SHA-256 为 `a54ee4096524923483805eb398ea319c7c6e71ad1b1d1c40303508fa0aeb5962`，预算沿用既有 `limits.json`。旧结果累计 32 项 open 发现、结构检查 16 条无效记录；须以新结果逐项核验后才能关闭。下文外发待授权、模型名称及旧计数均为历史记录，不能覆盖本次授权或作为新一轮通过证据。

本轮诊断与重试修复的日志见 [`provider-diagnostics`](../../artifacts/bid-full-sample/provider-diagnostics/)。共享 HTTP 传输在非成功响应时只保留状态码，不读取错误正文，避免错误正文被截断后丢失 HTTP 状态，也不输出可能回显的凭据/原文。400/413/429/503 的截断正文回归通过；完整默认 Rust 测试日志统计 601 passed、0 failed、15 ignored，fmt 和严格 Clippy 通过。本地 Journal 分别持久化累计调用数和每轮/角色尝试次数，恢复后仍受每边界三次及原总预算限制；专属恢复/请求一致性/预算测试 1/1、fmt、入口 Clippy 和构建通过。旧 23 次调用仅按完整保留日志核对并转换预约计数，未改写提取记录、关系、复核或检查点。

**早期格式复核记录：现有真实提取结果不通过。** 该批次记录19项发现及12个模板文本区域重叠；R08–R10已更正，部分加分及报价公式已存在，问题包括来源绑定不足及其他分项遗漏。指定格式第74–106页已作文本/非空网格核对，详见[格式验收清单](prescribed-format-acceptance.md)。原结果不改写，需修正后再编制。通用重叠校验和离线 audit 已接，80 项投标模块测试、定向 Clippy/构建通过。详见[真实稿验收复核](real-tender-acceptance-review.md)。下文历史执行成功或空 findings 不能作为语义验收通过。

2026-09-08。本轮使用 `testdata/bid/BiddingFile.pdf`，106 个物理页，SHA-256 为 `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。同目录另一份合同 DOCX 未自动关联。

## 可查看的本地基准

- `artifacts/bid-full-sample/reference/reference-template.docx`：68 个章节标题、52 张表格，含 27 次原表落位、25 张明确拟制的待填表。
- `artifacts/bid-full-sample/reference/preview.html`：目录导航及正文、表格内容预览，不代表 Word 实际分页效果。
- `artifacts/bid-full-sample/reference/verification.json`、`rendered.json`、`docreader-result.json`：校验结果及生成文档回读。
- `artifacts/bid-full-sample/reference-plan.json`、`reference-placements.json`、`build_reference.py`：针对这份真实文件逐页核对的显式测试夹具及来源落位；业务章节和本样稿的排版选择只存在于测试数据中，不进入通用生成规则。

**这是本地人工核对基准，尚不是产品 Agent 的独立复核通过版本。** 包含目录、附件 1A–1D、补充廉洁合规承诺、2A 及完整跨页注释、附件 3 的拟制表、商务技术偏差和 G.1/G.2、保证金和履约函、8A–8I 及资格补充材料、开标价格表、技术规范原文与逐段响应位置、技术附表 A/C、技术支持和售后/调试/质量风险方案位置。8D 分别保留投标人和原厂财务资料位置。

**该人工稿也有已确认的适用性错误，不能作为完整正确模板交付。** 后续原页复核确认第 8 页将附件 3 列为本次不适用，第 15 页 7.6.1 不要求履约保证金；不能因后部格式目录存在就拟制必交附件 3 或输出必交履约保证函。下文保留人工稿的历史构造说明；新 Agent 及最终 DOCX 必须以[逐格式来源清单](prescribed-format-acceptance.md)和实际原文核对，不能照搬该稿。

附件 5 标为本次不适用；技术附表 B/D/E/F 的不适用表格不复制为待提交附表。1B/1C 保留原文适用条件，待实际投标身份确定。附件 3 在所提供原件中有目录要求而未附对应网格，基准明确标为拟制，不能冒充招标原表。原件中的 `XXX 公司` 示例没有作为投标人复制。

固定声明和填表说明已展开，投标方身份、报价、实际参数、承诺确认、人员、方案细节和真实证明仍待填写。没有预填“无偏差”或“完全满足”。目录含原生 TOC 域及完整标题缓存，页码等待办公引擎实际排版，未虚构页码。

## 共享 Python 解析修复

上传文件继续只走 `services/docreader`。实际原页确认后修复了以下通用问题：

1. PDF 导出器会逐单元格画短边框；原来的 24 点长度过滤漏掉空白模板边线，现按线段几何识别。
2. 有边框的待填表可能只有表头有文字，不再要求存在两行填充内容；支持 8D-4 这样的两列表。
3. 独立边框区域分别提取，避免把页眉、标题及同页多张表拼成一张假表。
4. 合并区域从整个矩形内的原始字符顺序读取，避免按虚拟列拼接造成跨行文字错序。
5. 对齐检测以字母/数字/汉字的字高分行，避免标点、网址或目录引导点变成假表格行；8G 声明第 6 条完整保留。

新冻结输入有 148 个来源单元、42 个网格。旧 outline 导出的网格不再充当该部分回归的语义真值，相关断言改为对原页确认的表形、空白区、表头及具体单元格。

## 真实模型测试入口

`scripts/bidding_sample_source.py` 通过现有 Python service 冻结解析内容及原页图像。`crates/bidding/examples/tender_sample.rs` 直接调用现有提取和编制 Agent，提供本地调用前保留、检查点和恢复能力，不连接数据库，不发布工作区新轮。它不是生产 Journal/owner/lease 适配。

`scripts/bidding_sample_run.py` 要求显式 `--env-file`，每次从该文件重新读取模型、地址和凭据，并清除继承进程中另一套 chat/LLM 设置；没有临时模型覆盖选项。本样稿以 **`deploy/.env` 的当前配置**为唯一来源，不能混用根目录 `.env` 或临时指定模型；配置变化后须新建运行目录。

用户已明确授权向 `deploy/.env` 的模型接口发送本文件的解析正文、表格及必要原页图像，随后实际运行获准并已启动。早期运行的检查点曾记录全部 148 个来源单元的文本和网格读取，当时独立复核与编制尚未完成；这不是 v5 的当前进度；不能把阅读覆盖或本地人工基准冒充模型验收结果。

运行输入位于 `artifacts/bid-full-sample/live-source-v2/`，显式预算位于 `artifacts/bid-full-sample/limits.json`。执行入口：

```sh
services/docreader/.venv/bin/python scripts/bidding_sample_run.py \
  --env-file deploy/.env \
  --binary /opt/github/.knowledgebrain-target/debug/examples/tender_sample \
  --source-directory artifacts/bid-full-sample/live-source-v2 \
  --limits artifacts/bid-full-sample/limits.json \
  --run-directory artifacts/bid-full-sample/extraction \
  --mode extract
```

提取完成后将审查结果绑定到相同输入，再使用 `--mode compose` 的独立运行目录编制。按照原件对投标文件的组成、格式和完整提交义务逐项验收；本地人工基准仅供比对，不限定 Agent 的章节结构。不得预设“商务、技术、报价”框架，也不得消费旧 outline 生成结果。预算、输入或运行配置发生变化时须新建运行目录；不能把未完成检查点当成功结果。

## 已验证及限制

- 解析定向回归 23 项通过，含真实文件逐页内容覆盖和独立原表形状/空白区验证。
- 配置文件隔离与重新读取回归 2 项通过。
- 撤除旧 live-map 后投标模块单元测试 72 项通过；新增 examples 的定向 Clippy 通过。
- 真实基准 DOCX 经 Rust 生成 OOXML 回读校验、Python service 回读及独立 DOCX 表格/空白验证；内容及网格检查通过。
- 未进行这份新样稿的 ONLYOFFICE 打开、实际分页、目录页码更新、编辑保存重开和同稿 PDF 导出；此前缓存缺失；本轮已重新获取与历史实测相同摘要的官方镜像，见后文。
- 该批次尚未完成真实模型语义验收及生产 worker/Journal/结果发布接线；后续合成产品接线见文末，真实整稿仍未验收。没有新增 migration、表或列，也没有修改数据库、提交或推送代码。

`artifacts/` 和真实文件都受 Git 忽略，以上是本机文件位置。后续需要长期共享时应使用现有制品存储归档，不能只依赖本机测试目录。

## 按当前设计撤旧

编制依据为本项目招标文件对投标文件的要求；指定投标目录、组成顺序及格式优先，结合须知、评审、规范和补遗补全提交义务。招标文件自身目录不能直接成为投标目录，业务类别不能成为默认章节。本约束已补入 PRD §2.3 和编制 Agent 提示。

已删除固定章节/附件映射 `live_map`、自行构造复核通过的配套测试、两个旧 outline 生成的 JSON 夹具、旧 Tiptap/大纲会话及专用客户端、17 个旧大纲 Schema 和对应验证。新链使用的几何渲染原语改名为 `template_grid`，保留真实列宽、合并及空白验证；没有重写上传文件解析器。前端保留 DOCX 编辑、新轮及来源文件操作。

旧大纲创建、加载、生成、发布、预览及阶段执行 SQL 已撤除，队列枚举、序列化、enqueue 和部署注册表同步移除旧任务。删除 25 个旧专用 SQL 函数、4 张旧专用表；原执行、检查点和调用保留能力更名为 `bid_tender_agent_*`，沿用现有 baseline，没有新增 migration 文件或增加表数。新提取的诊断路径沿用共享 UTF-8 字节前缀，保留 NULL 拒绝、错误 owner 拒绝和失败原子落库。

旧 PostgreSQL 测试族及失效辅助夹具已删除，诊断和发布围栏回归迁至 `tender_analysis_postgres` 并接入现有 CI。当前内容、证据及导出仍消费的节点/块/绑定数据结构没有被当作旧生成流程一起删除；后续 DOCX 内容填充接线仍需按消费者逐项收敛。旧块渲染器不再根据“空根节点”推断隐藏章节，是否隐藏由显式 render_role 确定。

本地验证使用专属 PostgreSQL 16/pgvector 容器和空数据库，三个 baseline 整体应用成功；新 Agent 发布/恢复/预算及诊断/错误 owner 两项真实数据库测试通过。平台队列合同 3 项、注册表 4 项、API 9 项和 Worker deadline 1 项通过；Bidding/API/Worker 测试编译通过。没有连接或修改现有业务数据库，没有提交、推送或部署。

真实模型仍读取同一冻结输入，已完成首轮独立复核。复核发现多数指定模板遗漏，退回主 Agent 补全，尚未完成提取语义验收或 DOCX 编制。后续编制将使用当前编制提示；运行中的提取检查点和契约未被人工修改。

撤旧验证日志及 baseline 摘要已保存在 `artifacts/bid-full-sample/retirement/verification.json`。共享编译目录末轮曾因 Rust 1.88/1.97 产物混用失败，保留错误日志；改用独立目录和固定 Rust 1.97 后，72 项投标模块、3 项 Schema、17 项 baseline 合同全部通过。专属数据库容器已删除。

后续编制可使用本轮已编译的 `/tmp/kb-retire-target/debug/examples/tender_sample`，启动仍必须经 `scripts/bidding_sample_run.py --env-file deploy/.env` 重新读取配置。运行中的提取继续使用其原冻结契约，不因重编译重启或替换检查点。

## 本轮补充核对

以下页码均为物理页。第 22–23 页 §3.1 规定投标文件组成，第 13 页前附表 §3.7.1 规定平台上传分区；第 74 页是第六章内的格式附件目录，不能自动替代整本投标文件的组成要求。编制应建立组成条款与附件格式的对应关系，按第 22 页 §2.5 的来源解释顺序处理冲突。本地样稿仍输出一份完整可编辑 DOCX，不能据此声称已经完成平台分区提交。

第 25 页 §3.5.5 还要求近三年败诉的设备买卖合同诉讼及仲裁情况、判决/裁决文书位置。此项不在第 74 页附件标题中，必须结合须知补全，不能以附件齐全推断提交义务齐全。第 13 页前附表 §3.5 未另设资格审查特殊要求；投标人的真实诉讼情况仍待填写，不能预填“无”。

当前提取记录将缺失招标公告列为待确认项：第 1 页招标编号、第 11 页资格条件和第 14 页截止时间均引用公告；第 12 页保证金金额另指向电子商务平台。这些来源缺口与身份相关的明确适用条件不同，不能为了生成样稿伪造来源或改为确定值。

已修正条件适用的通用处理：有来源、明确范围和条件的要求/规则/模板通过独立复核后不再仅因 `conditional` 被降为待复核，原条件仍保留。编制条件模板时，在完整模板前显示经复核的适用条件；未知或不适用模板仍拒绝放置。DOCX 回读校验包含条件文字、换行和特殊字符，新增说明不破坏表格字段绑定。

本轮 75 项投标模块单元测试及库/样稿入口 Clippy 通过，日志为 `retirement/conditional-tests.log`、`retirement/conditional-clippy.log`。条件模板合成 DOCX 另经共享 Python service 正常解析路径回读，证据在 `conditional-regression/`；首次沙箱限制导致多进程 IPC 失败并触发简化解析的日志也保留，随后正常路径复验通过。此合成稿不是实际招标文件样稿。

真实提取已完成首轮复核后的补正并进入第二轮独立复核，目前候选含 41 条记录、10 条关系，尚无复核通过的整本 Agent DOCX。未修改运行中的检查点、来源冻结文件、模型配置或数据库。

### 真实运行恢复与来源缺口处理

第二轮复核在第 111 轮前因一次读取 19 张原页而超过 2 MB 冻结上下文预算，实际进程 exit 1。已保留 `extraction-before-image-batch-recovery/` 失败现场。提取和编制请求共用图片批次限流：最后一组未送达的图片超限时，保留原件缓存及已消耗预算，向模型明确返回延后查看的工具错误，并撤回该角色未送达图片的查看覆盖。不会缩放或替换冻结原页，不把移除图片当作已看图。

从同一检查点和 `deploy/.env` 恢复后，第 111 轮实际请求为 1,969,922 字节，送达 11 张图、明确延后 8 张；模型随后补看完，物理调用从原 111 次继续，没有清零预算。第二轮独立复核指出履约保证函与项目专用规定存在适用性冲突，主 Agent 补正后已进入第三轮复核。原子重放、图片覆盖撤销及编制端同类场景回归通过。

另按计划“缺失来源明示但不阻止编制”修正入口：复核 findings 非空、覆盖不完整、摘要不符或来源质量标签失真仍拒绝；独立复核已完成且缺口明确记录时，可以生成待确认模板。`manifest.json` 单独保留 `source_quality` 和 `source_open_items`，包括缺失/失败文档、未解决来源/记录/关系和原页失败；它们不注入正文、不生成未知值，并须由编制复核逐项读取。此类稿件最终标记 `reviewed_template_with_open_items`，不能冒充完整来源已验证的稿件。恢复时删除报告项会被拒绝。

本批投标模块单元测试 **78/78**、库及样稿入口 Clippy、运行器编译通过，证据为 `retirement/open-item-tests.log`、`retirement/open-item-clippy.log`、`retirement/open-item-build.log`。未新增 migration、表或业务字典；仍未完成真实 DOCX 语义及办公引擎整稿验收。


### 当前交接状态

真实提取已于第 161 轮完成，第三轮独立复核 findings 为空；结果为 `extraction/analysis-result.json`，文件 SHA-256 `1df40cc3e64ecb06ff312a47a1c4cc8c1fadafce4878d32f0883b96c32adb686`。保留外部公告等来源缺口，quality 为 needs_review。已将同一原始结果绑定到 `live-source-v2/analysis-result.json`，未手工改写质量或分析。

编制调用尚未执行：自动审批两次拒绝同接口的编制外发，要求用户明确授权此编制动作。`composition-authorization-audit.json` 证明配置地址/模型相同，全部 41 条记录和 10 条关系逐对象已出现在此前发往该接口的复核请求中；复审仍不接受此证据替代明确授权。已向用户提出授权问题，不能通过其他执行路径绕过。

本地已恢复官方 `onlyoffice/documentserver@sha256:e3da62a847b9a5d51a11f73cfea1d9c13c3be3809614490d4edddcf01dcf919b`。以人工基准尝试产品编辑回归时，`document_collection_acceptance.sql` 在编译输入完整集合断言失败，尚未进入编辑器；失败证据在 `office-reference-product/`，专属容器/网络均已清理且 cleanup 无错误。该次失败来自夹具调用已过时的 V3 loader/publisher，而新冻结集合先将来源记为 unresolved、由新 Agent 读取全集。后续已迁移至 V4 并取得完整数据库回归，见下节。


### V4 发布修复及本地 Office 复验

集合验收已迁到 V4，删除无调用方的 V3 加载/发布函数和授权。真实回归还检出了迟到结果占用判定版本、导致下一轮 CAS 失败的问题；已修复为只有当前输入推进判定，旧结果保留历史。五轮集合、同集合判定替换、迟到结果先完成、两轮 DOCX、重放及非空旧稿保护均通过。详细边界及证据见[来源集合回归](source-collection-regression.md#v4-集合与迟到结果回归)。无新 migration、表或业务硬编码。

Office 产品测试必须使用系统临时目录，已在 Python 启动前校验，避免创建服务后才被 Rust 的运行目录保护拒绝。实际浏览器已编辑并首次保存人工基准，但最初保真断言将 `<w:vMerge/>` 和显式 `continue` 当成不同：逐项差异仅有 48 个等价纵向合并值，52 张表无文字、列宽或结构差异。已按 OOXML 默认值修正比较，并用回归确认合并重启、删除合并、修改其他单元格文字和列宽仍会被检出；两项校验器测试通过。失败原件、保存稿和差异清单保留在 `/tmp/kb-full-sample-office/run-68df1a9a6d8e488ab5635702a9cf81f7/`，后续整链复验另行记录。


后续浏览器回归已完成两次强制保存、关闭最终保存及重开后再编辑保存，52 张表、单元格修改、插入图片及尺寸保持；随后在首稿新轮步骤等待旧入口按钮超时。当前 Workbench 对无 DOCX 的新项目自动显示首稿面板，已将测试对齐该流程，保留真实新轮上传/响应丢失重试/新 editor key 检查。一次复验另遇 Chromium 加载本地脚本的 `ERR_NETWORK_CHANGED`，失败日志保留于 `/tmp/kb-full-sample-office/run-96bb7f9c078c4b84932962de604d03c0/`，没有据此放宽检查或标记通过。


最终本地产品复验 **1/1 通过，exit 0**，证据归档于 `artifacts/bid-full-sample/office-reference-product/accepted-run-4ad5b9e5f28b45a3919a9a50fc968ed9/`。人工基准的 52 张表在两次 forcesave、关闭最终保存及重开后再次编辑保存后保持文字、网格、列宽和合并；单元格修改、图片字节/绘图关联和尺寸保持。真实前端首稿/新轮创建、提交成功但响应丢失后的同请求重试、新 editor key、历史版本字节及停掉本次 Document Server 后四次真实签名回调重放均通过。`cleanup.json` errors 为空，专属容器/网络已清理。归档不含登录票据、回调签名或临时服务密钥。

这次通过的是人工参考稿的产品编辑链路，没有调用编制模型，没有产出 Agent 的完整测试样稿或其同版本 PDF；真实招标义务覆盖、最终稿逐页排版及来源缺口仍需后续验收。编制请求继续读取 `deploy/.env`，外发授权未被本地通过结果替代。


### 编制请求和检查点持久化

已在所属 fresh baseline 增加一张编制请求身份表，复用已有 Request/AgentRun/调用/检查点记录，未新增 migration 文件，未操作业务数据库。真实临时 PG 验证脚本模型的完整分章编制与独立复核、实际生成 DOCX 字节、提交检查点后确认丢失、不同 attempt 恢复、旧 owner 拒绝、同一轮请求字节固定以及全局/单轮调用预算。连同来源冻结、原提取/诊断回归 **4 项通过**；78 项模块和 17 项合同、定向 Clippy 通过。证据清单为 `retirement/composition-journal-verification.json`，专属资源已清理。

这批仍未启动真实招标文件的编制模型，未完成 HTTP/队列/worker/初稿与清单原子发布及前端生成入口。测试中“独立复核完成”只推进到 publishing，Request 保持 pending，防止没有用户可用的新轮却显示成功。请求身份表必要性、冻结字段及持久化合同见[编制实现](docx-composition.md#持久化编制请求与-journal)。

### 已复核文件和清单的原子发布边界

已实现重新校验持久化复核结果、实际 DOCX/规范清单 bytes，复用新轮 CAS 和对象归属，在同一事务发布两对象、成功收据与完成状态。确认丢失使用只读重放，损坏或缺文件不报成功；生成期间人工保存或更新来源都会拒绝覆盖，人工新版本不继承初始清单。无新表、列或 migration 文件。完整 scripted-model 临时数据库回归及四项共享回归 **5/5**，78 项模块、17 项合同、定向 Clippy 通过；证据为 `retirement/composition-publication-verification.json`，临时容器/对象目录已清理。

这仍不是 `BiddingFile.pdf` 的 Agent 样稿：未运行外部编制模型，未生成其完整 DOCX 或同版本 PDF。HTTP、队列/worker 的完整编排和前端生成入口仍待接线。具体字节、对象归属、重放和验证边界见[原子发布实现](docx-composition.md#已复核-docx-与清单原子发布)。

### 编制队列和 worker 运行接线

新增专用编制 typed job/worker，复用原 Oxana 队列、运行 owner/心跳、检查点、原子发布和 Retention cleanup。已验证对象写入失败后恢复完整复核检查点、不新增模型调用，重放不重写文件，取消后恢复遇到新稿会终止为 CAS 冲突。写对象复用平台 blob API，在既有 worker 子进程生命周期边界内执行；没有新增解析器、模型默认值、表或 migration 文件。

本地 PG/Redis 六项及追加 CAS 恢复、worker 实际文件 helper 两项、deadline 一项、平台合同三项/注册四项、78 项模块和 17 项 baseline 通过；定向 Clippy 和 API/worker check 通过。证据汇总：`retirement/composition-worker-verification.json`。这批读取真实 Redis envelope 后直接测试生产 executor，使用脚本模型/测试 I/O；helper 另经真实二进制验证，不等同于完整 consumer/provider 产品链路。没有运行真实招标编制，没有产出 Agent 整本或其 PDF；HTTP 和前端生成入口仍待接入。

### 生成接口与前端入口

已接可编制依据、生成 POST、最新/指定请求状态 API，以及默认“按招标要求生成”的前端。空初始化要求集合不能代替 V4 招标分析；生成仍使用招标要求决定目录、完整内容及附表，实际投标方填充后置。原 DOCX 导入保留。HTTP 重放按原始意图查收据，模型/预算和 current 后续变化不能改写旧任务；页面保存未确认请求的依据和 key，刷新继续确认，202 后通过指定任务轮询。

真实 API 路由/PG/Redis **1/1**（含脚本真实 DOCX 前置夹具 **1/1**）、前端 **29/29**、最终构建实际 Chromium 模拟 API **3/3**、78 项模块及 17 项合同、API Clippy/前端 ESLint/build 通过；专属数据库/Redis、对象目录和浏览器预览服务已清理。证据为 `retirement/composition-http-verification.json`。显式编制预算已从既有 limits.json 同步到 `deploy/.env` 的新字段，其余配置字节不变，没有外部模型调用。该批仍未生成真实招标 Agent 整稿或 PDF，完整 consumer/provider/Office 端到端和语义高质量验收尚未完成。

## 实际 API/consumer/provider 发布链增量

正式 API/worker 二进制与真实 Oxana、localhost SSE provider、文件 helper 已在专属 PG/Redis 环境联通，连续发布两轮合成 DOCX/manifest，共 30 次工具调用；中间重启、重复提交、历史下载、越权和双对象摘要核对通过，资源已清理。证据：`retirement/composition-transport-final/verification.json`。

联调修复正常上传追加解析器合同导致 schema readiness 错判的问题：显式 frozen-seed 配置只锁定 baseline 初始身份，运行期追加合同仍走原 append-only 业务校验；初始记录修改或删除仍拒绝，建库时检查初始身份清单完整，无新 migration。平台单测 62/62、定向 Clippy/二进制构建通过；全 integration-target Clippy 尚有继承测试的八参数告警。详见[联调记录](docx-composition.md#实际-apiconsumer-与-http-模型联调)。

两轮输出使用合成来源和脚本模型，不能当作本页真实招标 Agent 样稿。真实招标整稿/PDF、真实内容覆盖和 Office 编辑的联合验收仍待完成，既有真实提取证据和外发授权状态保持不变。

### 模板校验与空格策略补充

本轮聚焦 O1 完整模板，不扩展基础设施。删除 `repair_grid_policies.rs` 自动修补程序：它把未分配的空格归为投标人填写区、添加“无内容填 /”的指令，并机械重绑关系及复核摘要。这些语义判断不能由固定规则替 Agent 完成。

在现有 Template 校验中补齐两项已存在于 DOCX 编译器的约束：同一张表的 regions 必须连续；非网格正文需要可编辑的解析文字。现在错误在 `put_record` 时返回，分析与读取覆盖不变，Agent 可以依据原文修正或保留 unresolved。程序不自动调整来源顺序，不把截图当作可编辑正文，不从空格推导填写规则。主 Agent 和独立复核指引同步明确这些边界；提示合同摘要随之变化，旧检查点不可冒充当前合同的新复核。

两项缺口均先以测试复现错误接受，再修复通过；另补空白 anchor 必须显式分配角色且可保持 FixedText 的回归。投标模块 **100 passed、0 failed、1 ignored**；fmt、全工作区全目标/全特性 check 与 Clippy `-D warnings` 通过。完整默认 Rust 测试首次因沙箱禁止本地监听失败，获准相同范围重跑后 **598 passed、0 failed、15 ignored**；源码、Cargo 清单及锁文件在验证期间未变。未增加表、migration 或运行配置，未修改 Python/前端。

使用现有 `tender_sample audit` 对原始 `live-source-v2` 执行离线只读校验，得到 **16 条结构无效记录**，主要是正文区间重叠与网格 anchor 角色缺失，编制依据仍被拒绝。该检查不进行语义验收，也不替代既有 32 项 open 发现。真实 PDF、冻结输入、原始分析的文件 SHA-256 均与原记录一致；未改写分析、关系或复核结果。完整 Agent DOCX 和同版本 PDF 仍未生成。

日志及摘要见 [`template-policy/`](../../artifacts/bid-full-sample/template-policy/)，原始失败日志和被删除程序备份保留于 `/tmp/kb-o1-template-policy-bxly0wze/`。本轮未调用外部模型；已只读确认 `deploy/.env` 仍为 `gpt-5.6-sol`、接口主机 `cpamc.818996.xyz`。该批次曾等待自动审批拒绝所要求的明确外发授权；用户随后已回复“允许发送”，见上方授权记录。

### 产品生成与 Office 同次闭环补充

已用既有合成夹具连通完整 App 点击生成、实际 API/consumer/SSE 工具调用、DOCX+manifest 发布、浏览器下载和 ONLYOFFICE 两次保存/关闭重开；第二次保存包含实际生成表格的单元格编辑。共 3 轮编制、45 次本机工具调用，原生成版本及历史摘要在编辑后仍保持不变，表格未被编辑的内容与结构一致。最终运行及临时资源清理通过，记录见[同次产品验收](docx-composition.md#浏览器生成至实际-office-保存的同次验收)。这是合成来源的产品接线证据，真实招标提取的 32 项发现、完整 Agent DOCX 和同版本 PDF 仍待完成；原始招标、冻结输入、分析及验收索引未修改。
