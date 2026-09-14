# 招投标运行诊断与接入验收边界

**2026-09-10 当前修复进度：本地功能验证通过，独立复核已完成局部比较，未完成最终提交，运行已停止；完整验收尚未通过。** 已分离来源权限与局部焦点，自动维护成果/未解决引用，主提取、独立复核、编制和稿件复核共用进展与有界恢复策略。默认连续无进展6轮、焦点24轮、重规划2次；记录局部执行阻塞后允许转向独立范围，连续6轮仍未交接则在工具提交边界停止。重启、重复读取、笔记改写和任务改名不能刷新额度；有效局部写入可以完成当前动作。执行失败单独保存并阻止最终发布，不冒充来源缺项。候选当前版本参与窗口保留；各 reviewer 保留自己的冻结原文回执，修改后的候选/稿件仍按摘要核查。旧手抄引用输入及编制 `remember` 路径已删除，未新增 migration，既有 baseline 同步检查点与预算 JSON。

验证：最新176项库测试（11项忽略）、20项合同、1项真实检查点离线提交诊断、严格 Clippy、workspace fmt及样稿编译通过；既有6项隔离 PostgreSQL结果保留，本次未改SQL。首次3来源短测在21轮停止，11条记录、0关系，发生一次超时后重试成功；第二次 v2 在32轮停止，29条记录、12条关系、2个来源处置，四组重点引用由真实Agent产生，但独立复核未开始。v2请求42214–353819字节，含必要原页图片，无503或超时。轨迹还暴露无响应要求被迫指定渠道、同类型合规属性的不同条件被拒绝、工作引用格式说明不足；均已修复并验证。未新增 migration，未修改实际 `.env` 或旧检查点，未执行暂存操作。包含全部修复的 `response-contract-trial` 已以新身份、空候选重测第11、16、17页的3处来源，模型与预算仍来自 `deploy/.env`；启动合同已核对，结果待验。该试验在34轮保留32条记录、21条关系后，进一步定位到重复读取会触发无实际缺口的 pending_delivery 阻塞。已删除这条冗余判断，尚未交付的原文/候选仍按真实缺口阻止交接；172项库测试、20项合同及Clippy/格式/编译通过，SQL未因本项调整。`response-contract-resume1` 已在第101轮由无进展/交接保护停止：主提取完成，41条记录、21条关系、3项来源处置；独立复核收到65个候选当前版本后仍重复读取，两次重规划无效，0轮复核、1项执行阻塞。已保留终态和计数，未将空缺口等同于语义通过。现补充复核专用完成指引与按实际缺口生成的下一动作：有问题逐项保存、无问题完成原范围后提交空草稿；执行阻塞仍禁止提交。173项库测试、20项合同及Clippy/格式通过，无新工具/配置/migration；提示词已改变，`reviewer-completion-trial` 已以新身份、空候选复测；启动核验确认实际配置、工具和预算未变，旧终态未改。该试验在第85轮到达诊断时限并取消：主提取41条记录、28条关系、3项来源处置；reviewer完成一个局部范围、收到72个候选当前版本，仍未提交最终结论。除精确读取位置反馈、局部复核排除不相关待办外，现新增 complete_review_check，在既有进展账本中记录当前候选的无问题比较；重复版本/改写结论不能续额度，来源、当前版本、执行阻塞及最终全局门槛保持不变。176项库测试、20项合同、3项.env启动测试及Clippy/格式/编译通过，无新表、migration、检查点字段或环境变量。新工具/提示词采用新身份：clean-review-trial 在补充明确授权后实跑656.16秒，停于第25轮：独立收到72个候选当前版本，完成41个记录版本的局部比较，46次重复比较被去重；28条关系和3项来源处置仍未形成比较结论，0轮最终复核。定位到当前焦点已完成但执行反馈仍要求比较该焦点，已增加焦点剩余数和转向未完成引用的派生反馈；176项库测试、20项合同、Clippy/格式/编译通过。clean-review-resume1 保留原检查点、预约正文及计数兼容续跑，在第40轮完成全部72个局部比较，但此后反复读取，0次完成范围、0次提交复核，第58轮进入执行阻塞，第64轮耗尽交接额度后停止。只读第54轮检查点副本的正式工具调用均通过，证明当时接口可用；离线副本不作为真实复核结果。最终结束行为仍未解决，本轮后续复杂附表及完整样稿重测未启动。此前自动审批拒绝已由用户补充明确授权解除。旧运行、原始来源和计数未改。复杂附表、完整106页独立复核、32项语义发现及完整 DOCX/PDF仍未通过。证据：[验证记录](../../artifacts/bid-full-sample/loop-repair/verification.json)、[v2真实轨迹](../../artifacts/bid-full-sample/loop-repair/source-scope-trial-v2/result.json)、[上一实跑启动核验](../../artifacts/bid-full-sample/loop-repair/reviewer-completion-trial/startup-verification.json)。

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

当前已接入 DOCX 编辑与显式新轮；完整 Agent 模板生成和最终出件仍待整链验收。 本文定位当前可复用代码与存量运行诊断，不保留旧阶段施工/完成声明。目标见 [领域契约](authoring.md) 与 [ONLYOFFICE 契约](onlyoffice.md)，编辑与出件顺序见 [ONLYOFFICE 计划](../../plans/bidding/onlyoffice-integration.md)，Agent 改造见 [Rig 方案](../../plans/bidding/agent-runtime-rig.md)。

Agent 检查点当前合同版本3包含活动 SDK 会话，已区分准备、完整响应和工具提交；检查点序号与模型轮次分别计数。恢复/SDK接缝的实测范围及旧检查点处理见[验收记录](agent-runtime-recovery-results.md)。不要将旧运行换成新二进制后直接恢复，也不要以局部恢复测试代替语义验收。

## 1. 当前源码证据（只读核对）

| 接缝 | 路径 | 可证明什么 / 不能证明什么 |
| --- | --- | --- |
| 文件处理 | `crates/bidding/src/tender_upload.rs`、`tender_process.rs`、`tender_analysis/` | 单文件验证、SourceUnit/冻结来源与要求编译基础；不证明完整文件复用/新轮整稿已验收 |
| Workspace/权限 | `crates/bidding/src/workspace.rs`、`bid_authoring_v2.rs`、`crates/api/src/bid_v2_routes.rs` | project/workspace 与 CAS/幂等接缝；不证明 DOCX 保存协议 |
| 报价 | `crates/bidding/src/quote_snapshot.rs` | Decimal、人工确认和不可变报价；不证明实际正文含报价表 |
| 媒体证据 | `crates/knowledge/src/knowledge_retrieval.rs`、`knowledge_retrieval_pg/` | V3 media/scope 与冻结引用；不证明图片已插入 DOCX |
| 对象/队列 | `crates/platform/src/object_registry.rs`、`jobs.rs`、`crates/worker/src/{runtime,helpers,knowledge,bidding}.rs` | worker 只含 adapter + helper；queue ACK 不等于业务成功；housekeep env 需重启；retention 不在本进程 |
| 编辑与新轮 | `web/src/bid/authoring/DocxEditor.tsx`、`DocxRound.tsx`、`crates/bidding/src/docx_round.rs` | DOCX 编辑和显式发布；合成来源的 Agent 生成至 Office 保存已验证，真实整稿另行验收，旧块渲染不能作回退路径 |

路径中简写文件与同格首个完整路径同目录。当前实现可只读定位，不意味着应继续扩建旧正文模型。已有 schema/compiler/fixture 按其真实行为诊断，不作为新产品标准。

### 完整分析与生成版本报告查询

以下两个 GET 已接入现有 owner 权限与持久化查询，不新增 migration。接口验证见 [API整合记录](../../artifacts/bid-full-sample/loop-repair/analysis-report-api/verification.json)：两个隔离 PostgreSQL/HTTP target 各1项、17项API单测及Clippy/fmt通过；前端报告入口的[34项单测、10项合成浏览器回归与构建检查](../../artifacts/bid-full-sample/loop-repair/analysis-report-api/frontend/verification.json)通过。合成fixtures及实际HTTP校验均不等于真实106页提取或完整DOCX/PDF验收。

| GET | 用途与边界 |
| --- | --- |
| `/api/v2/bid-projects/{project_id}/requirement-sets/{requirement_set_id}/analysis?kind=all&offset=0&limit=100` | 按精确冻结要求集分页读取完整记录及原始多分类、政策、visual/grid引用；`kind=all`仅包含所有record类别。关系、来源处置、复核意见分别使用`kind=relation`、`disposition`、`finding`；也可按`fact`、`rule`、`requirement`、`template`、`unresolved`筛选。显式提供非负offset和1–100的limit，沿各集合total读完，不能把一页all当整张图。 |
| `/api/v2/submission-workspaces/{workspace_id}/docx/versions/{version_id}/composition-report` | 按workspace及精确生成版本下载持久化manifest原始JSON，含来源质量及source_open_items；有无待确认项都可下载。校验存储摘要、长度及DOCX身份后返回`application/json`附件。后续手动上传版本无报告返回404 `DOCX_COMPOSITION_REPORT_NOT_FOUND`，不回退到历史生成报告。 |

旧项目`/requirements`和workspace`/requirement-projection`仍是有损兼容投影，不应用来重建完整章节、政策及附表关系。分析页的quality不是单独的错误结论：复核finding和原文来源open-items须分别解释；编制报告只反映其绑定生成版本的来源状态。不存在的冻结要求集返回404；旧要求集没有完整分析时返回`available:false`。越权查询返回403，未认证返回401；报告对象不可读返回503，摘要/长度/绑定身份校验失败返回422，均不自动取另一版本。生成页报告下载失败不丢失原DOCX下载及重新生成功能。

## 2. 现有部署与平台恢复

运行进程、网络、bootstrap、角色、release receipt/catalog 校验和隔离测试准备统一见 [部署说明](../../deploy/README.md)、[平台基础](../../plans/platform/runtime-foundation.md) 与 [队列合同](../../plans/platform/queue-runtime.md)。不在本页复制另一套清库/重建步骤。

当前 Compose 命令启动的是已有系统，不安装 ONLYOFFICE。接入另需确认浏览器/API/文档服务/对象存储互通、JWT 服务端密钥、受控下载、资源预算、版本/字体与产品嵌入许可。没有采购或部署决定，不默认插件免费商用，也不把外部 Automation 当基础编辑必买前置。

招投标现有 `/api/v2` 与共享 `/api/v1/ops/*` 分域。`bid-authoring-v2` 队列现注册五类任务，新增的 `docx_compose` 绑定冻结分析、Workspace 与预期 DOCX，用于整本模板编制，不能冒用已完成的提取或旧内容填充请求。运行合同以 `deploy/queue-registry.toml` 为准，不另建 transport 状态；HTTP/前端已接，完整 consumer/provider 验收进度见[编制记录](docx-composition.md#编制队列与-worker-执行)。

## 3. 可复用故障诊断

- **源文件失败**：查单文件状态、原件摘要、DocReader/OCR/VLM、source publication 与对象状态；修复后由用户重试。保留已成功文件结果，不为一份失败清理整个项目。源集合变动后新轮整稿，不能按旧运行快照要求局部修补旧稿。
- **队列不可用**：已提交 Request 不代表送达；503 携同一 Request identity，同幂等键显式 replay 精确重载冻结 payload 并调用官方 enqueue。不得扫描 PostgreSQL pending Request 建第二套队列。transport ACK、Request terminal 与发布结果分别核对。
- **过期结果/CAS**：409 保留人工修改；重新加载后由用户核对意图。后台旧请求不能推进新 current。新 DOCX 会话的乱序保护不能仅复用 Workspace ETag 就宣称完成。
- **缺证/图片失败**：NO_EVIDENCE 是提示；检查 knowledge-owned mapping/attestation，禁止通过 OCR 文本回查 live 图片或手改 EvidenceBundle。选中资产损坏则技术失败。
- **输出/资源失败**：核对请求、冻结来源、摘要、对象 owner reference 与实际文件；不得直接删除 blob 或业务历史。未提交 staging 的有界清理由平台处理。存量自研 renderer 的成功/失败只说明旧路径；新转换失败不能隐式回退该路径。
- **保存**：区分 session key、forcesave 命令、2/6 回调落盘与冻结出件版本；3/7 错误、重复、乱序、旧轮和存储失败须可定位，不以到达时间猜最新、不静默下载旧稿。

## 4. 验证如何计数

以下是后续实施可选的现有聚焦验证位置，不是本轮运行记录，也不要求先重做平台：

| 测试/脚本 | 证明范围 |
| --- | --- |
| `crates/bidding/tests/tender_document_process_v2.rs` | 来源、OCR/图片 identity、确定性重放与失败清理 |
| `crates/knowledge/tests/knowledge_image_ingestion_v3.rs` | media publication 原子、幂等和保留 |
| `crates/bidding/tests/request_delivery_postgres.rs` | 请求/执行围栏，不是编辑保存 |
| `scripts/bidding_composition_transport_acceptance.py` | 正式 migrator/API/worker、真实 Oxana 与本地 SSE provider、两轮 DOCX/manifest 发布和重启；仅合成来源，非真实招标语义或 Office 验收 |
| `crates/bidding/tests/tender_analysis_postgres.rs`、`content_agent_run_postgres.rs` | Agent 副作用与调用预算，不是新文档定位或入稿完成 |
| `scripts/bidding_v2_phase2_api_e2e.py` | 存量块 Workspace API |
| `scripts/bidding_v2_evidence_api_worker_e2e.py` | 现有证据 API/Worker |
| `scripts/bidding_v2_export_api_worker_e2e.py` | 存量自研双格式导出，不计为 ONLYOFFICE 验收 |

必须在授权隔离环境按实际切片运行并记录命令、原始退出码、fixture/样稿、输出摘要、依赖与跳过项；SQL/服务不可用、零测试或 mock 成功不能计为真实通过。文档检查不执行构建、全测试、服务启动或部署。

新链验收必须实际打开/改写/保存/重开 DOCX，验证回调错误与乱序、未保存导出、同源 PDF、独立版本报告、定点 AI 不覆盖人工、固定表保真、真实报价附件和页码安全出口。最终检查不能只看数据库快照存在。整本出件保留独立提交要求，用户自行拆分与复核，系统不验拆分稿。

实施、局部测试、提交、部署与真实运行验收分别报告；旧日志或 fixture 名称不决定完成状态。

## 归档候选的独立复核诊断

`scripts/bidding_sample_run.py --mode review` 使用同一 `--env-file deploy/.env` 配置入口。来源目录同时包含冻结原文、原页图片和 `analysis-seed.json`；程序先校验主提取结构及阅读缺口，将种子摘要加入冻结合同，以新的空 reviewer 阅读账本开始独立复核。修订种子不能复用原诊断合同，已有检查点不会被初始化分支覆盖。结果单独写入 `review-diagnostic-result.json`，不冒充完整主提取或整稿验收。真实候选仍是需要核查的声明，不是预设正确答案。

2026-09-10 的 `clean-review-trial` 程序、来源、种子及摘要已准备；自动审批拒绝派生候选外发，尚未启动。所需补充授权及外发范围见[诊断审批记录](../../artifacts/bid-full-sample/loop-repair/clean-review-trial/approval.json)。
