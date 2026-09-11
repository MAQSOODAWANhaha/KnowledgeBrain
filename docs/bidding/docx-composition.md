# 分章 Agent 与 DOCX 实际落位

2026-09-10 运行层补充：编制与稿件复核已接共享进展策略，`set_composition_work` 取代 `remember`，表达当前来源、章节、动作及交接状态；`check_composition_execution` 分页查看执行阻塞。来源与章节身份由宿主校验，章节写入通过原业务校验后才能完成局部编制动作。重复读、改笔记和重启不刷新额度；执行阻塞不进入来源报告，也不能发布为已复核稿件。源码/稿件详情优先于导航保留，始终按完整协议组裁剪；各角色的阅读回执独立，既有版本摘要继续拒绝陈旧稿件。新预算字段省略时采用6/24/2默认值，并写入冻结合同；不得恢复旧提示词/工具摘要的检查点。工程验证和实测边界见[本轮记录](../../artifacts/bid-full-sample/loop-repair/verification.json)，真实整稿尚未通过。

**真实样稿当前不可直接编制：** 独立来源复核累计32项开放发现，早期格式批次还记录12个模板文本重叠（R08–R10已更正，见[格式复核](prescribed-format-acceptance.md)），已增加通用拒绝校验，原提取结果未人工改写。须先完成[真实稿复核清单](real-tender-acceptance-review.md)中的提取修正与逐段应答承载，再执行真实整稿验收；此前合成链路通过仍只证明对应工程边界。

业务目标沿用 [PRD](prd.md) 和 [F2/O1-S 接入计划](../../plans/bidding/onlyoffice-integration.md)：根据招标要求生成全部章节、固定文字、附表、填写说明和签署位置，投标方事实、报价、人员及证明材料内容后置。当前已有 `docx_composition` 核心、生产 Journal/队列/发布与 HTTP/前端接线，并完成合成生成至 Office 保存重开；真实完整稿尚未验收。运行层按 [Rig 方案](../../plans/bidding/agent-runtime-rig.md)推进：三边界 Journal 已实现并通过隔离恢复测试，Rig Chat 接缝已验证；共享宿主驱动已接提取/编制，生产 Rig AgentRun 已接入并通过有界会话及三边界恢复验收。具体范围见[验证记录](agent-runtime-recovery-results.md)。下文保留各批次证据，“本批次未接线”等只描述当时状态，最新真实运行见[样稿记录](full-sample-results.md)。

## 输入与运行边界

分章阶段消费一个冻结的 `FrozenInput` 和已独立复核的 `AnalysisResult`，校验输入摘要、分析摘要、阅读覆盖及复核覆盖。 独立复核存在未修正 findings、覆盖不完整或摘要不匹配时拒绝编制；已复核且明确记录的来源缺失/未知允许生成待确认稿。质量标签按冻结内容复算，不能把 needs_review 手改为 verified。来源待确认项保存在独立 manifest 的 source_open_items 中，并纳入编制复核阅读覆盖，不混入投标正文；此类稿件复核完成后标为 reviewed_template_with_open_items。不会再次解析上传文件、读取旧大纲或复制上一轮稿件。原页证据复用分析版本内冻结的图片；需要新原页或修订源模板时，应先创建新的来源分析版本。

`Draft` 绑定完整分析版本摘要。Agent 使用有界工具逐章设置层级、顺序、内容和依据，声明所需模板、响应项和证明项的位置。每次修改提供当前 draft 摘要，避免覆盖较新的结果；章节身份由工具分配，不由附件标题推导。章节数量、业务分类、字体、纸型和表格列名不由程序套固定模板。

运行参数由调用方显式提供 `agent::Config`：复用现有工具调用模型配置与 `provider_turn` 传输，预算包括 `max_turns`、`max_tool_calls`、`max_physical_calls`、`max_read_bytes`、`max_context_bytes`、`max_tool_result_bytes`、`max_review_rounds`、`max_docx_bytes`。模型、提示词、工具和预算构成冻结运行摘要。此批次没有添加隐式环境默认值，也没有部署或配置真实模型。

后续共享配置已统一：新请求从 `.env` 读取 `KB_AUTHORING_MAX_OUTPUT_TOKENS`、`KB_AUTHORING_TIMEOUT_MS`，部署默认分别为8192和180000；可选 `KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT` 未配置时省略。主编制与稿件复核使用同一冻结参数，SQL 预约拒绝参数漂移。参见[运行参数说明](tender-analysis-review.md#运行参数与环境默认值)。

## 实现选择与验证边界

| 环节 | 当前选择及理由 | 验证边界 |
| --- | --- | --- |
| 章节组织 | Agent 分批创建/修改章节，以父节点和同级顺序组成树；程序检查身份、循环、重复顺序及空叶章节。一次生成整本 JSON 难以恢复，也无法逐章修复。 | 结构合法不证明章节语义完整；独立复核仍逐项核对招标要求。 |
| 指定模板 | 引用整条已审查模板，按来源区域展开段落和完整网格。表后说明、签章及固定文字保留原文。 | 交错且无法确定顺序的网格、缺失单元格策略或只有图片的文字不能伪造为可编辑模板。 |
| 表格与留白 | 所有实际网格锚点必须明确固定/待填/说明/签章策略。原列宽、跨行/跨列合并和重复表头保留。待填格默认整格清空；混合格可由提取记录的 `blank_ranges` 指定只清除原文中的部分 UTF-8 字节，其余文字保留。 | 同一锚点只接受一个策略；固定标签应与待填值分开标注，错误角色应修订分析，不能在编制阶段暗改。没有示例值的提示格不清空。文本换行和制表符写为 Word 原生元素。 |
| 逐段原文与响应 | `source_response` 绑定一个明确响应项；每段由冻结 UTF-8 字节区间或网格锚点单元格依次拼接，并另建空响应段落。`source_excerpts` 分别记录原文书签、来源片段及响应书签。 | 来源须属于该响应项和要求记录的证据；模板区域只能走模板复制策略，不能通过原文段落绕过。片段正确不证明条款完整，段落顺序、条件与缺漏仍须独立复核。 |
| 非指定表格 | 没有指定格式时，Agent 可提出响应表及列名、空行数，或保留明确的响应/证明空位。 | 不能用通用空白或拟制表替代选定的指定模板；拟制结构需复核。 |
| 多种条件/格式 | 每个模板、响应项和证明项都须放置或保留有来源依据的不输出决定。条件已明确且经独立复核的模板可完整放置，模板前显示复核后的适用条件；未选用的条件模板关系另行说明依据。 | 条件说明是有来源的解释，不冒充原文逐字引文；不预选投标身份，不把条件改成无条件适用。未知或不适用模板拒绝放置，不按关键词判断 OR。 |
| 实际落位 | 编译后回读生成包中的 Word XML，校验章节样式、目录、文字、表格、合并、留白、页面尺寸/方向及边距。manifest 将模板区域/单元格、响应/证明位置绑定到实际书签和表格坐标。网格区域的每个已选锚点各有一条位置，不以整表书签代替区域内具体格。 | 书签以 DOCX 摘要为作用域；仅有 plan 或记录存在不算入稿。此处校验生成包，不是业务侧新增上传文件解析器。 |
| 目录前内容 | `put_section.placement` 可显式选 `front_matter`，用于来源要求的封面等内容；默认 `body`。前置内容使用 Title 样式，在原生目录前输出，不进入正文目录。 | 必须位于全部正文章节前、无父级和子章节，且文档至少保留一节正文。仍需来源依据及原有模板/响应校验，没有默认封面或固定行业章节。 |
| 独立复核 | 使用独立上下文，读取完整冻结来源/分析、章节及不输出决定、实际渲染结果和位置映射。长表的实际单元格分批读取，按内容摘要记录检查；不能用整表元数据代替逐格核查。 | 请求/提交复核须单独一轮工具调用，防止同一批“读取后立即批准”时模型尚未收到结果。结构验证不能代替真实语义评测。 |
| 恢复与失效 | `Journal` 提供调用前保留边界和工具完成后 checkpoint 保存；恢复校验冻结身份，预算耗尽/取消保留未完成状态。修改章节使旧文件与检查失效。完成重放从 draft 重建确定性文件及映射核对。 | 本批次验证了序列化 checkpoint 和确认丢失后的重放；生产 PostgreSQL Journal 已在后续批次接入并验证，见文末；该批次队列 worker 与文件发布尚未闭环，后续接线及合成验证见文末。 |

原表格原语中的固定总宽度限制已移除，几何仍需有限、正值且与来源边界一致，最终由文档可打印宽度校验。横向测试稿采用源网格 220 mm 宽；换成容纳不了它的页面会失败，不自动挤压列宽。

本轮还修正了来源网格定位：Python 解析结果是完整网格，合并区域覆盖的位置也存在于 cells 中。判断可编辑单元格不能仅检查位置存在，必须排除被覆盖位置。对应关系和模板策略共用这一约束。

## 证据与待接线

条件适用修正后，本地投标模块单元测试 **75/75** 通过。新增回归覆盖条件要求/规则/模板通过提取复核后仍保留原条件、未知状态仍待复核、缺条件/范围/来源仍拒绝，以及生成 DOCX 中条件说明、换行、特殊字符和实际表格字段绑定。编制结果仍需独立内容复核；这些合成测试不等于真实招标文件语义验收。运行中的真实提取未被重启或人工修改检查点。

- 投标模块单元测试 **72/72**（分章 **9/9**）、定向 Clippy `-D warnings` 与 API/worker check 通过；worker 有此前旧路径未使用告警。分章用例覆盖两章实际文件、同名附表隔离、跨行/跨列合并、表后说明、签章、证明位置、待填值清空、条件格式的明确取舍、预算取消、复核权限、过大结果不计已读、检查点确认丢失恢复，以及已保存映射被替换后的拒绝。
- 模型为脚本，DOCX 字节为真实生成。测试文档 `/tmp/kb-composition-artifacts/chapter-template.docx` 与 manifest、Python service 回读结果和独立包读取记录保留在同一目录。它是合成协议样稿，不是 `testdata/bid/BiddingFile.pdf` 的模型生成成果。
- 回读使用现有 Python service；测试另用 Python DOCX 库核对两张表的合并、留白、原列宽和页面尺寸/方向。未完成 ONLYOFFICE 的实际排版、编辑保存重开或全行业样稿验收。
- **后续接线：** 冻结生成请求的分析版本、文档集、Workspace 和新轮基线；将 `Journal` 接入已有持久化运行/owner 边界；将已复核 DOCX 与 manifest 通过已有对象及新轮发布接口原子关联；接前端生成状态与结果查看。不能把回调式存储接口当作已接好的生产持久化。
- 本批次没有新表、列或 migration，也没有修改现有数据库。长文运行成本、每轮 checkpoint 的图片/文件重复缓存，以及修订影响范围仍需真实模型与长样稿验证。发布后的人工编辑会产生新文件版本，初稿映射不能直接声称对新版本仍有效。

## 真实整本基准样稿

2026-09-08 已用真实 106 页招标文件制作本地人工核对基准：68 个章节标题、52 张表格，固定正文和指定表格已展开，投标方实际填充仍后置。发现并修复共享 Python service 的短边框、空白表和文字错序问题。真实模型固定读取用户的 `deploy/.env`，用户已授权实际文件发送，提取运行已启动，独立复核及完整编制尚未完成。产物、验证边界和可恢复测试入口见 [完整样稿记录](full-sample-results.md)。


## 生产接线：编制依据与恢复合同

`docx_composition::postgres` 已接现有数据库的不可变分析及来源。`prepare` 只接受指定 Workspace 的当前文件集/要求集合和预期 DOCX 版本，先通过项目 owner 权限，再沿 V4 发布收据定位原提取请求，从其冻结身份读取原文、人工决定、网格和分析内原页。重新读取当前来源、扫描上传目录或把上一份投标稿当来源都不在此路径中。没有可追溯 V4 分析的初始空集合/旧编译结果不能当作编制输入。

`FrozenCompositionRequest` 绑定 Workspace、发起人、文件集/要求集合 ID 与摘要、原提取请求身份、完整来源/分析摘要、显式模型配置和编制契约摘要、预期 DOCX 版本。快照只保存身份与配置；106 页图片等大内容沿现有不可变分析读取，不在新请求中重复缓存。`restore` 校验已持久化的请求摘要，读取同一历史版本并再次检查独立复核及覆盖；后续文件冻结不改变输入，模型/提示词/工具/预算契约变化不能自动沿用旧检查点。该接口的摘要参数必须来自可信的持久化请求身份，不能用客户端临时重算的摘要替代。

编制期间用户可以继续编辑。预期 DOCX 身份仅用于后续发布 CAS，不把旧正文/报价/检查/页码传给 Agent，也不在准备时锁住文档。请求准备时来源或版本过期返回确定性的 `WORKSPACE_CAS_CONFLICT`，不能当暂时网络错误不断重试；最终发布仍必须重新执行已有来源和 DOCX CAS。

本批真实临时 PG16 验证 **3/3**（已有 Agent 发布/预算、诊断/权限和新增编制来源冻结/历史恢复）；新增用例通过真正的 `kb_runtime_api` 登录调用数据库函数，覆盖来源身份、越权/摘要错配、过期 DOCX、运行契约篡改、后续集合冻结、旧输入精确恢复，以及不创建 DOCX 的边界。脚本模型只用于建立已复核的合成分析，不是行业语义验收。证据在 `artifacts/bid-full-sample/retirement/composition-source-regression/`；同一用户跨项目混用与空集合拒绝的扩展回归见 `composition-source-regression-green/`。这些专属数据库容器均已清理。78 项模块、17 项 baseline 合同及库/数据库测试 Clippy 通过。

**上一批依据接缝范围：** 当时已接数据库来源解析与可序列化冻结合同；没有新增表、列或 migration，没有创建编制任务。完整请求入队、`Journal` 的调用保留/检查点/owner 接线、已复核文件及 manifest 的原子发布、前端生成状态仍待完成。编制是新的、绑定 Workspace 和预期稿件版本的工作，不能冒用已成功的提取请求或旧章节内容填充请求；下一步沿现有 Request/AgentRun/对象设施实现该专用身份及生命周期，不引入另一套通用任务存储。


## 持久化编制请求与 Journal

新增专用的 `bid_docx_composition_request_identities` 身份表，复用现有 `bid_async_request_snapshot_artifacts`、`bid_tender_agent_run_artifacts`、调用记录与检查点表。只有身份表是新增表；没有新增迁移文件、第二套任务状态机或对象存储。该表必要性在于：编制绑定 Workspace、分析版本和预期 DOCX；提取请求已完成且不绑定 Workspace，旧内容请求绑定旧章节/块及证据填充，二者均不能表达本期整本模板的身份。直接把快照塞进任意 checkpoint 也无法提供这些作用域外键。所有改动限于所属 fresh baseline，未操作现有业务数据库。

身份表以外键绑定原提取请求、文档/要求集合和预期 DOCX 版本，以约束逐项核对冻结 JSON 与类型列。配置、提示词和工具定义随请求保存一次，摘要与编制核心契约一致；不保存模型密钥值。创建采用已有幂等及 audit，重放返回原请求；请求字节与重新构造的 job payload 必须一致。编制期间仍可编辑旧稿，最后发布必须再次执行来源和稿件 CAS。

`PgJournal` 已使用独立的 composition main/review/checkpoint stage 接入原执行账本。调用前锁定当前 owner/attempt/token/lease，核对冻结模型、token 上限、提示词、工具及规范请求字节；同一未完成轮的重试不能换角色或请求体，最多复用原有三次调用边界，总物理预算按整个请求累计，重新 claim 不会清零。每次工具完成后追加检查点，确认丢失可重放相同字节，改变已提交检查点会拒绝。章节、阅读/复核状态及真实 DOCX/manifest 保存在检查点中；复核完成只进入 publishing，不能把尚未发布的新轮标成 succeeded。

真实隔离 PG16 回归包含 API/worker 两种实际角色：创建幂等、待入队 payload、完整脚本编制与独立复核、真实 DOCX 字节、检查点提交后确认丢失、第二个 attempt 恢复章节、旧执行者拒绝、完成检查点无模型重放、角色/请求体变更拒绝、单轮三次调用及跨 attempt 全局调用上限。新用例 **1/1** 通过，另外三项原提取/诊断/编制来源回归在同批 baseline 上通过；证据分别在 `retirement/composition-journal-regression-5/` 与 `composition-journal-shared-final/`（最终 baseline）。78 项模块测试、17 项 baseline 合同及库/数据库测试 Clippy 通过。最初函数定义顺序、测试角色和返回封装错误保留 red 日志，随后修正；没有为修复测试额外开放辅助函数或跨角色权限，也没有放宽预算。所有专属数据库容器已清理。

**该持久化批次结束时的待接线项（后续已接通，见文末）：** SQL 已可持久化 `docx_compose` 请求，Rust 的 `create_request`/`load_request` 和生产 Journal 已实现；本批没有 HTTP 创建/查询入口，也未接 Rust 队列 payload/注册、worker 的 heartbeat/失败处理、已复核 DOCX 与 manifest 的原子新轮发布、前端生成入口。因此请求在本批测试中保持 pending/publishing，不能把它当作已投递或已生成用户可用新轮。该批次真实模型样稿曾待外发授权；用户后续已明确允许发送，独立语义与整稿验收仍未完成，脚本模型结果不替代它。

## 已复核 DOCX 与清单原子发布

已接 `prepare_publication`、`publish_staged` 和只读 `replay_publication`。发布前从持久化请求和检查点恢复原来源，调用与 Agent 完成出口共用的 `reviewed_artifact`：核对冻结契约、完成状态、独立复核覆盖及 findings，并确定性重建 DOCX、清单和实际渲染位置。不能只凭 `reviewed_template` 标签发布。生成 bytes 通过既有 `InitialDocx` 验证；清单使用规范 JSON 字节，两个对象都须经现有 blob 接口回读校验长度和 SHA-256。

SQL 使用同一事务完成 DOCX staging 转移、既有新轮创建及来源/稿件 CAS、清单 staging 转移、不可变 `object_commit` 收据，以及 AgentRun/Request 的 succeeded。入口和提交前均检查现有 owner/attempt/lease；锁等待后过期会整体回滚。复用 `object_owner_references`，将清单作为初始 `bid_docx_version` 的 `composition_manifest` occurrence，没有新增表、列或 migration 文件。新轮内部使用随机幂等键，编制请求的稳定重放由原子收据承担，防止上传入口的自定义幂等键撞入新轮 replay 分支而跳过 CAS。

`replay_publication` 在任何重新 staging/写入之前使用，验证原请求身份、成功收据、两个可用对象及版本归属，再读取物理文件核验。确认丢失后不需要旧执行者仍有租约；后续来源或人工稿变化也不改变旧结果。缺失/损坏文件返回错误，不自动重写旧文件。该只读重放不消耗新 staging；`publish_staged` 成功才表示两个传入 staging 均被转移，错误时调用方须保留现有 cleanup guard。生产 worker 的 staging/write/cleanup 编排仍待接入。

`get_manifest_identity` 先验证 Workspace/项目 owner，再查询指定版本。人工保存产生的新版本没有原清单归属，返回无清单；不会继承旧书签/表格映射。原版及其清单继续可以按历史身份读取。

最终专属 PG16 **5/5**：完整脚本编制后双对象发布、第二对象失败回滚到无新轮且保留 staging/pending、旧 owner 拒绝、实际缺文件拒绝、确认丢失只读重放、两个文件分别损坏/缺失拒绝、跨 Workspace/用户读取拒绝、人工最终保存后旧生成 CAS 拒绝、来源新轮后生成 CAS 拒绝、原稿和原清单历史不变，以及原 Journal/来源/提取/诊断回归。使用实际 API/worker 数据库角色、现有 blob 函数和独立系统临时对象目录；S3 明确关闭，未使用外部模型，专属容器和对象目录已清理。78 项模块、17 项 baseline 合同及定向 Clippy `-D warnings` 通过。证据汇总：`artifacts/bid-full-sample/retirement/composition-publication-verification.json`。首轮测试错误地复用第一次编制文件来登记第二次产物，已改为读取各自检查点的真实字节；保留失败日志，没有放宽文件核验。

**当前未完成项：** HTTP 创建/查询、Rust 队列注册及 worker heartbeat/失败处理与对象写入清理编排、前端生成入口，以及真实招标模型全稿和同版本 PDF/ONLYOFFICE 语义排版验收。以上通过的是原子发布边界，不能标记完整 O1-S/F2 或整体计划完成。

## 编制队列与 worker 执行

已增加 `DocxComposeJobV2`、`bid:docx_compose:v2` 和真实 `DocxComposeV2Worker` 注册，继续使用 `bid-authoring-v2` 物理队列、Request ID/revision 唯一身份及原 Oxana 重试策略。专用 SQL job loader 仅允许读取编制类型；执行前逐项核对 payload 的项目、Workspace 和完整请求身份，防止错误投递消耗别人的 attempt 或将其标失败。模型和预算只从持久化的原请求读取，没有新增模型默认值、业务章节规则或外部文件解析路径。新增一个 worker 只读函数授权，无新表、列或 migration 文件。

`docx_composition::runtime` 先读取成功收据并核验两份实际文件，再 claim 原 authoring owner，恢复冻结来源/检查点并执行分章和独立复核。模型调用期间沿现有租约发送心跳；心跳失败、取消或截止会取消协作工作并等待其退出，然后持久化 retry_yielded/failed。已成功提交的发布优先用收据解释，不能因确认丢失反向写成失败。完整复核后的对象写入失败可以换 attempt 恢复，测试证明不再调用模型；来源或 DOCX CAS 冲突记录为确定性失败。

对象阶段在 staging SQL 之前登记现有 `StagedObjectCleanupTracker`，避免 staging 确认丢失漏清理。依次写入并回读核验 DOCX 和清单后，调用上节原子发布；仅成功返回后 disarm 两项。worker 在成功、失败、取消及超时出口将未提交 staging 交接原 RetentionQueue，交接失败不显示成功，不另建补偿框架。

生产 `ObjectIo` 使用既有 object-read helper，并增加对称的 object-write helper，调用原平台 blob 接口；其输入摘要/长度先核验，错误 bytes 不能覆盖对象。复用现有进程组终止、stdin/stdout 有界读取和子进程回收。心跳与正文执行采用协作取消并等待完成，不能在清理前直接丢弃仍运行的写入 future。子进程用于约束同步存储 I/O 的生命周期，不承担文件解析，也不访问旧大纲。

验证证据为 `artifacts/bid-full-sample/retirement/composition-worker-verification.json`：

- 专属 PG16/Redis **6/6**，含新运行时测试及五项发布/来源/Journal/提取回归。实际读取官方 enqueue 产生的 Redis envelope、重复投递只保留一项，随后直接调用生产 executor（脚本模型、测试 ObjectIo）；核对错误 scope 不消耗 attempt、live owner 不重复工作、第二对象写失败后重试/无新增模型调用、成功重放无写入、cleanup Redis 交接、旧 staging 释放不影响已发布对象、6 秒模型等待期间真实心跳和取消释放、确定性失败落库。追加取消后恢复遇到新稿的 CAS 失败/旧稿保护通过，见 `composition-worker-cas-final/`。
- 真实 worker 可执行文件的 object-write helper **2/2**：精确写入、错误长度/摘要拒绝、进程阻塞于未完整输入时终止回收且不写文件。共享 deadline 测试新增编制 worker，**1/1** 通过；payload 合同 **3/3**、队列注册 **4/4**、投标模块 **78/78**、baseline **17/17** 通过，定向 Clippy `-D warnings` 及 API/worker check 通过。专属 PG/Redis 容器及临时对象目录已清理。

**该 worker 批次的验收界限（后续合成整链见文末）：** 队列 envelope、生产 executor 和真实 helper 分别已有证据，但尚未完成“HTTP → 真实 Oxana consumer → 模型 HTTP → 前端”的完整产品回归；清理测试证明交接及精确 expiry 的归属保护，没有据此宣称 Retention consumer 已完成最终物理删除。HTTP 生成/状态入口、前端、真实招标 Agent 整本及同版本 PDF 仍待完成。真实编制外发授权状态保持不变。

## 生成 HTTP 与前端入口

已接以下 owner-scoped 路由，正文只接受当前招标分析依据与预期保存稿身份，不接受旧大纲、投标正文或客户端模型配置：

| 路由（均位于 `/api/v2/submission-workspaces/{workspace_id}`） | 用途 |
| --- | --- |
| `GET /docx-compositions/basis` | 返回可编制的当前文件/要求身份；没有 V4 已发布分析的初始化空集合返回 null。完整语义/摘要验证仍在提交及执行时完成。 |
| `POST /docx-compositions` | `{basis, expected}`＋`Idempotency-Key`，冻结请求后调用现有官方 enqueue，返回 202。 |
| `GET /docx-compositions/latest` | 恢复该 Workspace 最近一次编制任务，无记录返回 null。 |
| `GET /docx-compositions/{request_id}` | 读取指定请求状态、进度、失败码和原始结果身份；不存在或跨 Workspace 返回 404。 |

HTTP 幂等按用户的 Workspace/basis/expected 意图确定，服务端运行契约不参与客户端重放身份。首次提交解析环境配置并冻结实际来源；重试先通过已有 idempotency 表只读找到原收据，不重新解析环境或 current。专用 submit SQL 再锁定同一 intent，将冻结请求、原始 key 的 audit 和收据一起提交，解决并发重放。新配置或新招标来源不改变已提交任务；同 key 改输入返回 409。Redis 失败返回携原请求身份的 503，仍由同 key 重载原 payload 再投递。没有新表、列或 migration 文件。

新请求通过 `Config::from_environment` 读取现有模型配置及显式 `KB_DOCX_COMPOSITION_LIMITS`，缺预算或模型配置不创建请求。预算 JSON 的八个必填字段为 max_turns、max_tool_calls、max_physical_calls、max_read_bytes、max_context_bytes、max_tool_result_bytes、max_review_rounds、max_docx_bytes。另有默认进展参数（6轮无进展、24轮焦点、2次重规划），以及默认上下文参数 `max_context_tokens=131072`、`image_token_reserve=16384`、`token_safety_margin=4096`，均可在同一 JSON 中显式配置。编制和稿件复核复用提取侧的保守估算，同时检查工具/消息/图像、输出预留及字节上限；这不是供应商精确 tokenizer。Rust 和既有 baseline 拒绝零预算及无法容纳输出预留的配置。所有生效默认值写入冻结合同；新增字段改变编制合同摘要，旧编制检查点不能冒充新合同续跑，提取合同不变。没有默认模型，本次未修改实际 `.env` 或业务数据库。`deploy/.env.example` 和 Compose 已传递该项；此前本地 `deploy/.env` 的配置采用 `artifacts/bid-full-sample/limits.json` 的 composition 配置，旧文件全部字节原样保留，模型/地址/密钥未改。此配置写入没有启动模型调用或重启业务服务。公共运行时剩余的 `OUTLINE_AGENT_*` 常量名称已改为 `AUTHORING_AGENT_*`，数值和冻结契约未改变。

前端默认提供“按招标要求生成”，保留“上传已有 DOCX”。生成前读取已分析的依据；保存仍待确认时禁止开始。状态面板轮询指定请求，展示编制/复核/发布与恢复进度，成功后按结果中的具体版本下载，来源缺口继续显示待确认提示。投标人资料、实际响应、价格和证明材料仍待填，不注入“完全响应”等结论。进入当前稿件与下载本次生成版本分开，不能把较新 current 冒充本次结果。

提交前在 sessionStorage 保存 `{input, attempt}`；响应丢失或 queue 503 保留原依据及 key，并阻止普通离开/切换导入。强制刷新后仍可确认同一次请求。202 后清除未确认记录；重开页面通过 latest 恢复后台进度，不追加 POST。失败或完成后重新生成需要刷新依据并再次点击；读取进度失败保留原任务。已知服务配置未就绪显示可重试提示，不假称请求已在执行。这个提交恢复记录只有请求身份和版本依据，不含招标全文、模型配置或密钥。

证据汇总：`artifacts/bid-full-sample/retirement/composition-http-verification.json`。专属 PG/Redis 上真实 API router **1/1**，前置真实脚本分析/编制/双对象发布夹具 **1/1**；覆盖 owner/跨 Workspace、缺 key、无配置不写请求、实际 Redis 拒连后的同 key 重投、改变模型/清空预算后的原请求重放、修改请求体拒绝、真实 AgentRun 进度、空初始化集合不可编制、来源更新后原请求重放及新请求 CAS 拒绝。脚本夹具生成真实 DOCX，但不是招标行业语义样稿。前端 **29/29**，实际 Chromium 对最终构建的模拟 API 交互 **3/3**，覆盖响应丢失＋刷新恢复、后台任务重开、已有 DOCX 导入与未分析项目禁止生成。78 项模块、17 项 baseline 合同、API 定向 Clippy 和前端定向 ESLint/build 通过。最初 HTTP fixture 将仓库已有缺 key 的 400 误写为 428，保留 red 日志后只修正夹具。

**该 HTTP/界面批次的待验收项（后续合成产品链已通过，见文末）：** HTTP、Redis envelope/worker executor、实际 I/O helper 和浏览器已有各自证据，尚未合成完整真实 consumer＋模型 HTTP＋ONLYOFFICE 产品链路。该批次曾待真实招标外发授权，用户后续已明确允许发送；Agent 完整样稿及同版本 PDF、逐义务覆盖、逐页排版和最终物理回收验收未完成，不标整项完成。ONLYOFFICE 人工参考稿的回归脚本已对齐新入口并通过语法检查，本批没有重跑其完整 Office 验收。

## 实际 API、consumer 与 HTTP 模型联调

`scripts/bidding_composition_transport_acceptance.py` 在自建并标注 owner 的 PG16/Redis、临时对象目录和独立环境中，使用正式 migrator 创建空白 baseline/receipt，启动实际 API 与 worker 二进制并检查双方 `/ready`。复用原有分析/编制测试夹具导出的工具步骤，由 localhost HTTP provider 分片返回 SSE 工具参数；不读取部署 `.env`，不发送真实招标内容，不替换真实模型配置。

本轮正式启动发现并修复一个运行时边界错误：正常上传会向 `bid_authoring_contract_artifacts` 追加 converter/vision 合同，原 manifest 将整表当作 frozen seed，正常业务写入后即导致 schema mismatch。`deploy/platform-frozen-seed-tables-v2.json` 现在显式配置混合表的 baseline 主键，通用 builder 通过绑定参数选择这些初始记录，保留全部列的摘要校验；未配置主键子集的表仍校验全表。建库时额外验证所选身份覆盖全部初始记录，防止以后追加 baseline seed 却漏更新清单。没有在 Rust 中按合同类别猜测初始记录，也没有放宽 schema/ACL/owner/原始 seed 的完整性检查；无新表、列、migration 或 receipt 改写。

数据库反向测试证明：初始合同 payload（同步合法摘要）被修改、初始合同被删除均拒绝就绪，事务回滚后恢复；正常注册解析器合同并完成发布后，原收据在 admin/API/worker 角色下仍通过。所有破坏性模拟仅在自建测试库的回滚事务内进行。

实际 HTTP 连续生成两轮，每轮经 15 次真实 HTTP provider 调用、独立文档复核、worker 文件子进程和 DOCX/manifest 原子发布。验证完成与历史请求重放、投递去重、越权状态/下载拒绝、可下载文件字节与两对象摘要/长度一致；第一轮后重启双方，第二轮仍正常生成，旧版本继续可下载。最终只有夹具原轮加两个新轮，模型调用共 30 次，无额外调用。两轮 DOCX 和对应 manifest 保存在证据目录，均显式标为 synthetic。

证据：`artifacts/bid-full-sample/retirement/composition-transport-final/verification.json`。平台单测 62/62（catalog 13 项），相关定向 Clippy 和 API/worker/migrator 构建通过；扩大到全部 integration-test targets 的 Clippy 被继承的 `object_registry_concurrent.rs::add_owner` 八参数告警阻止，原始失败日志保留于 `composition-transport-checks/clippy.log`。本轮没有改该无关测试。

这一批完成实际 API/consumer/provider/文件发布链，尚未将浏览器生成入口和 ONLYOFFICE 编辑接在同次验收里，也不证明真实模型的行业语义质量。真实招标 Agent 整本 DOCX、同版本 PDF、逐义务覆盖和字体排版仍待验收；外部编制授权状态不变，不标 F2/O1-S 完成。

## 逐段原文与响应承载验证

已在现有增量编制工具中加入 `source_response`，复用既有 DOCX 原语与冻结来源，不新增解析服务、模型配置或 migration。一个段落可显式组合多个文本/单元格片段以承接跨页断句；不接受模型自带引文。Word 中原文段落与待填响应段落使用不同书签，只有后者计入响应落位。每条来源映射随 `inspect_composition` 分页返回，遗漏检查或映射变化会产生复核缺口，章节修改会清除旧稿和复核结果。指定模板关系仍执行原有共址校验。

模块测试 **83/83** 通过，另有显式运行的真实来源承载测试 **1/1**；定向库/样例 Clippy 通过。新增测试覆盖文本与网格跨页拼接、多段映射、空响应、外来来源/表格、UTF-8 边界、合并覆盖格、模板复制策略、指定附表约束及复核失效。

真实来源测试直接读取统一 Python 服务已冻结的第59–61页：弱口令跨页续句，以及协同防御“承＋诺”和 SD-WAN 独立证明文字。逐字比对生成 DOCX 中的原生段落，保留两个空响应段落。证据目录为 `artifacts/bid-full-sample/source-response-carrier/`，其中 `plan.json` 是人工选择的定向测试输入，`output/source-excerpt-carrier.docx` 是承载验证件。该文件不是 Agent 整本样稿；未修改此前被拒绝的提取结果，未消除 R14 的提取粒度和完整性问题。真实整稿、Office 及同版本 PDF 验收仍待完成。

精确网格引用补充：网格原文片段现在必须由要求记录和对应 response 的 `grid_cell` 依据共同授权，不能只用同页文本范围。86 项模块测试通过，新增真实来源引用验证见 `artifacts/bid-full-sample/grid-citations/`；这不改变上述承载验证件的非 Agent 整稿性质。详见[提取证据结构](tender-analysis-review.md#精确网格证据与逐段要求)。

## 招标指定封面与目录顺序验证

目录前内容沿用现有 Section、冻结来源及模板编译链。文档标题之后依次输出显式前置内容、分页、原生目录、分页和正文；正文标题参与目录，前置内容使用无大纲级别的 Title 样式。省略 placement 时维持原正文行为。生成后除逐块内容检查，还按实际顶层书签顺序核验位置，并检查原生目录的指令、域标记、缓存标题及分页，防止完整块被调序却仍通过验收。

模块 **89/89**、baseline **17/17**、真实来源定向承载 **1/1** 通过，库和样例 Clippy `-D warnings` 通过。新回归覆盖封面模板及响应映射保留、目录排除封面、非法嵌套/后置/全部前置拒绝，以及 OOXML 块调序与目录指令篡改拒绝。证据为 `artifacts/bid-full-sample/front-matter/`。

其中 `output/source-excerpt-carrier.docx` 使用物理第73页封面和第59–61页部分条款：封面九格逐字核对、三格空白保留，源文件和旧分析摘要未变。输入由人工选择，仅用于确定性原语验证；未运行真实 Agent，不是整本投标样稿，封面的间距、下划线和实际分页亦未通过 Office 外观验收。第73页提取缺口另记 R20，继续保持 open。

## 混合单元格的局部留白

提取记录的 `TemplateRegion.blank_ranges` 为可选数组，每项包含 `{row,column,start,end}`，偏移相对于该冻结单元格原始文字的 UTF-8 字节。仅 `bidder_blank` 网格区域可用；数组非空时，区域内每个格子都必须有明确范围，避免未指定的格子被意外整格清空。范围必须位于已读取的实际锚点内、非空、字节边界有效且不重叠。同一格不能同时被多个区域赋予策略；没有局部范围时沿用原来的整格待填策略，空字段不改变旧记录序列化。

`read_form.find_text` 可按精确子串返回位置对齐的 `matches`，每个命中直接给出原格行列及UTF-8起止位置；包括重叠出现，不做模糊匹配或自动选择。没有命中返回空数组，合并覆盖位置返回 null；查询超出既有工具输出预算或参数无效时不确认阅读。编制沿用同一读取工具，没有第二套来源定位器。Agent仍须判断哪些值是示例，不能自动删除全部同文命中。

编译器将已审范围传给既有表格原语，保留范围外全部来源字节，只生成空填写位置，不引入替换内容。部分清空与整格清空互斥，重复表头和合并覆盖位置不可清空。实际DOCX回读核对剩余文字与原几何，原有单元格/模板区域映射继续可用；改变范围会使旧分析摘要和复核失效。

94项模块测试通过，17项baseline与定向Clippy已有通过证据。新增回归覆盖中文精确位置、多处/重叠匹配、未读格、外来格、无效/重叠范围、缺少策略、表头保护、审核失效和整格清空冒充局部清空拒绝。`artifacts/bid-full-sample/partial-cell-blanks/` 保存两类明确区分的验证件：合成混合格移除4个示例值，保留标签和签章；真实第91–92页8A网格保留61个锚点原文和19格空值。真实格没有混合示例值，因此没有应用局部清空，不能把本批说成R16的真实Agent修复。8A完整表前后文字、原提取修复与完整Agent样稿仍待验收，无新migration、模型配置或上传解析器。

## 网格区域的精确落位

此前模板单元格引用保存了实际坐标，但 `template_region` 网格区域只保存整表书签。当 Agent 将响应或证明绑定到该区域时，位置记录无法区分区域内的输入格和旁边的标签格。该问题已先通过失败回归复现，随后修复。

编译器现在为区域内每个冻结锚点分别输出 `bookmark + row + column`，响应/证明的区域绑定沿用这些位置；单格区域和直接单元格引用得到同一位置，合并覆盖位置不单独生成字段。文本区域仍定位到实际段落，整条模板引用只表示整模板，不能当成某个具体字段对应的证据。区域归属及角色完全由已审提取记录提供，没有附件编号、标题或行业关键词规则。

工具说明明确这一契约，并进入既有 Agent 运行摘要；历史请求不能静默按新合同恢复。未改数据库、migration 或源文件解析流程。新增两项回归分别核对多格区域仅指向已选输入格、单个合并锚点的区域/单元格别名一致，并核对编译回读后的真实原生单元格。投标模块 **96 passed、0 failed、1 ignored**，Clippy `--all-targets --all-features -D warnings`、fmt 及10份 Schema 正反例检查通过。原始失败与成功记录见 `artifacts/bid-full-sample/region-placements/`。

这是字段定位能力修复，不证明 Agent 已选对语义关系。真实 `BiddingFile.pdf` 的原分析保持不变，32项独立发现（包括 R12 附表关系）仍待重新提取和整稿验证。

## 浏览器生成至实际 Office 保存的同次验收

现有 `bidding_composition_transport_acceptance.py` 增加成对传入的 `--browser` / `--onlyoffice-image`，复用 `onlyoffice_web_probe.product_web` 和既有表格校验器，将完整 App 的生成入口接入同一组真实 API、Oxana consumer、文件发布与 ONLYOFFICE。镜像由调用者显式传入并固定摘要；模型仅为 localhost 合成工具夹具，不读取 `deploy/.env`，未外发真实招标内容。没有新增产品接口、框架、配置字段或 migration。

同时修复验收脚本仍按旧 `OBJECT_DIR/<hash>` 读取文件的问题：Rust 夹具通过平台 `blob_path` 返回当前实际对象目录，Python 在专属临时根内校验路径后读取；不自行拼接 namespace。原 native driver 的文件与重投前后摘要核对同步使用同一定位方式，本轮对其完成编译检查，实际浏览器链由扩展后的 transport runner 执行。

最终运行 `live-final` 通过：前两轮 HTTP 编制保留既有重放、进程重启和越权检查；第三轮由完整 App 点击创建，45 次本机 SSE 工具调用全部经真实 worker 完成。浏览器下载生成稿后进入真实 Document Server，插入段落标记并保存，关闭重开后编辑生成表格中的单元格并再次保存。新 editor key、两次实际保存版本及下载摘要均核对；前一次段落标记仍在，指定单元格编辑出现且只出现一次，去掉测试标记后的整表文字、列宽、合并与表头属性与原件一致。人工编辑推进 current，编制任务的原始结果和历史生成文件保持不变。

首次运行因 Docker inspect 的旧 Gateway 字段失败，改为读取实际 Networks 映射后通过；失败日志、第一次整链通过记录及增加单元格操作后的最终记录均保留。正式 api/worker/migrator 构建、全工作区全目标/全特性 check 和 Clippy `-D warnings`、fmt、Python 语法检查与表格校验器 **2/2** 通过；每轮真实数据库夹具 **1/1**。最终运行与清理均成功，API/worker exit 0/0，本轮登记容器全部移除，未清理任何其他资源。执行期间被改文件摘要稳定。

证据位于 [`artifacts/bid-full-sample/product-composition/`](../../artifacts/bid-full-sample/product-composition/)，包括日志、源码差异及摘要、3 轮生成稿、清单和浏览器编辑后的文件/截图。此夹具只含一张合成表，用于验证产品接线及可编辑性；不是完整真实招标样稿，不证明逐页排版、字体、行业义务覆盖或同版本 PDF。O1-S/F2 的真实高质量验收仍未完成；下一步继续原文驱动的 Agent 补正和整稿验收，外发授权状态未变。

### 响应为空的要求

分析中的合规约束可以没有投标提交产物。编制端仅为实际存在的 `response`、`proofs` 和指定模板建立输出位置；空响应数组不产生默认表格或声明，仍有证明义务时必须保留证明位置。所有约束继续保留在完整分析中；空数组不代表投标人合规，也不允许跳过原文或上位条款明确要求的响应。
