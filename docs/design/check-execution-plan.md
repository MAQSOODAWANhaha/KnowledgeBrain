# Check 执行闭环改造方案

状态：通用多文档、Word/图片混合及独立原图交付已有本地模拟闭环。当前并行整改来源遗漏修复、跨文件依赖失效、来源副本语义、真实视觉范围与大附件预算分页。下文“目标合同”在对应生产代码及验收通过前均不算已实现；完成状态以集成测试记录为准。FrozenInput schema为3，OutlineArtifact schema为3（旧1/2严格拒绝，无兼容），解析合同为source-v2；检查点契约为29，不迁移旧阅读信用。本地模拟不能替代真实模型验收。

## 本轮实施与验证边界

当前代码已修改，真实性能和服务验收分别记账，不能以计划或本地模拟替代通过：

- 冻结输入仍是唯一来源。分包选择直接使用有序章节、单元及范围，先合并允许关联的连续范围；仅拟定最终请求构造时展开并完整计数，超限才缩小章节或原子范围，单原子超限才拆分。删除逐项增长候选的完整序列化循环和候选缓存；一次规划复用不可变输入摘要，并一次建立正文/载体/表格/定位的轻量权重与前缀；首次实际请求测量给出容量比例，之后顺序贪心选择合法章节边界，仅对权重前缀二分。完整校验超限则按实测比例严格缩短，成功不填满。句柄不跨候选作用域复用。没有新增预算平台、预处理存储或固定字节阈值。
- 宿主保留完整身份、原文与表格；wire共享文档元数据并使用有作用域短引用。空白压缩保留结构与可填写位置。上下文上限为131072 token，完整输入估算已含协议/图片/安全余量，只加一份当前输出预留；删除第二份未来输出与固定反馈预留，累计任务预算独立记账。实际请求不加`max_tokens`；图像义务只按真实载体和实际图片交付判断，文本或缓存不算看图。
- 失败修复轮转版本后，诊断归属键绑定新身份；旧/伪造键仍拒绝。已结算失败提交正文保留宿主审计，当前repair仅投影对应失败反馈；未消费读取、未确认和混合调用不压缩，实际仍超限则停止。既有局部repair与回执保持，不将身份错误变成自动整包重发。
- Discover结构接受不代表语义覆盖。现有Check复核遗漏、否定、条件与适用性；全部在途响应结算后，终止失败范围不阻止独立已提交范围的来源复核。依赖失败范围的来源排除，所有必要范围和全局未完成阻断保留。正常提取/比较结果与文档问题可共存并持久化。

验证顺序为失败响应的脱敏离线回归 → 同一冻结输入的性能、覆盖和完整预算对照 → 经后续额度协调的真实Discover/Organize/Check → 代码与文档PR → 最终CI。修复后真实流程尚未通过；本地测试、性能结果及独立静态复核应分别记录。不会因本轮性能修复迁移OCR、接入knowledge业务、补造缺失外部公告或引入复杂平台。

## 1. 目标与审查结论

当前范围是解析 → Discover要求提取 → Organize章节模板 → Check/repair。交付目标是 **template-ready（模板就绪）**，不是投标人材料已经填齐。knowledge检索、材料匹配、填写和填充后材料验收属于下一阶段；现有knowledge通用HTTP/SSE依赖及其单元测试不代表业务联动已接通。历史通用修复保留独立范围。

`outline/mod.rs`明确投标人空位保持空；`outline/tools.rs`要求editable_blank的text为空、match_query非空，并允许它作为履行目标。Check应验证每项义务有具体、可填写且正确归属的字段、空位、规定格式或操作说明，保留完整来源、条件和例外。具体义务绑定的合法空位可通过模板验收；泛化“请提供材料”而没有字段、来源、条件或明确位置不能通过。不得为通过Check填写未经提供的投标人事实。

审查决定：

- 复用现有Draft、DiscoverWork、比较器、回执和journal；最小CheckWork只保存源范围处置与结果引用，不复制传输生命周期。
- 先证明串行垂直闭环，再完善多文件保真、固定文件集内repair和实测优化。Check并发后置；现有Discover并发保持。
- 文件集变化后重新全项目Check；跨集合复核信用复用暂缓。类型化现有来源清单，不维护两份manifest。
- 按实际修改提取模块，不预先大搬目录、不新增crate或调度服务；性能优化先测量。

## 2. 保留、重构与替换的决策依据

复用不是目标。判断标准是需求是否满足、职责是否单一、权威状态是否唯一、版本是否正确、能否独立测试及实测成本是否合理。现有实现若根本阻碍这些目标，应替换其边界和旧路径；不能因少改代码保留错误设计，也不能用“最佳方案”预设一套通用框架。以下决策可被实现证据推翻，变更时记录理由。

| 对象 | 当前选择及依据 | 重构/替换触发条件与代价 |
| --- | --- | --- |
| 领域工作调度 | 重构聊天驱动的下一工作选择为源范围处置+结果ref派生；现有结果仍唯一权威。避免任务状态与真实比较/复核分叉，可直接测试重载一致性 | 若Draft无法提供明确状态所有权，允许整理或替换领域结果容器；不得补一层账本隐藏矛盾。删除被替代扫描/恢复路径 |
| 传输协调和journal | 保留已测试的发送/接收/重放、取消与未知预算语义，因为它们符合传输职责及恢复要求，不是只因代码已有 | 若现有Host接口迫使共享领域可变状态或阻碍Check隔离，重构或替换协调边界，迁移故障测试后删除旧实现；不留两套传输真相 |
| 证据身份和版本 | 保留精确来源身份、全digest和epoch校验，它们可拒绝过期信用。第一版文件集变化全Check以正确性换取重算成本 | 全项目重算可能昂贵，不宣称永久最佳。只有显式依赖闭包和跨版本证明经测试后再替换为细粒度版本；不能提前放宽旧信用 |
| 执行分包 | 重构现有同文档选择和单document包边界，因为它不能满足跨文件关联；保留逐原子身份及完整请求预算约束 | 若当前ParsePack结构无法表达多源而产生隐式主文档，应替换包合同及对应wire，不堆例外；删除旧单文档假设并拒绝旧合同 |
| 模板完成gate | 重构为唯一template-ready gate，结构、源处置、具体字段覆盖和fresh复核共同决定；区分未来filled验收 | 当前散落done路径若无法统一必须替换，保留验证器仅因其仍正确。删除并行完成权威，以独立正反例验证 |
| 请求fit | 保留完整请求准入、无损续页及未知预算；先profile再重构request-local数据流 | 若clone/业务投影耦合导致不可测或明显重复成本，替换构建边界而非叠加缓存补丁。性能收益需等价工作负载证明，无收益的抽象不保留 |

实现不保留兼容双轨。需求合同及正确性测试应先定义，再决定保留哪些内部实现；迁移成本是权衡因素，不能凌驾于目标正确性。

## 3. 当前能力与缺口

| 代码依据 | 已有能力 | 需要解决的问题 |
| --- | --- | --- |
| `analysis/agent.rs::Checkpoint`、`analysis/outline_flow.rs::OutlineRun` | 持久检查点、TurnJournal、业务草稿与发现状态 | 从权威结果稳定选择下一领域工作，支持历史裁剪和重载 |
| `outline/discover.rs::DiscoverWork, RequirementRecord` | 来源包、要求、条件支持、版本和幂等提交 | 已提取要求不能代表全源没有漏提 |
| `outline/read_receipts.rs::queue, seal, confirm` | exact-wire交付、发现版本与Check epoch隔离 | 读过不能证明语义正确或复核完成 |
| `outline/tools.rs::Draft, CheckReads`、`claim_review.rs::build, validate` | 要求比较、来源观察、目标与复核结果 | 从源范围独立挑战遗漏；模板目标必须覆盖具体义务 |
| `outline/agent.rs::host_packet, submit_review` | 当前工作投影、pack-only来源复核及要求复核 | 扩展现有提交协议表达源处置和模板覆盖，不按任务种类增加工具 |
| `outline/agent.rs::reopen_discovery_pack` | 受控重开、repair签名、fresh Check | 局部repair已在开发；需验证跨依赖闭包与fresh Check，不以保存草稿等同保存信用 |
| `analysis/agent/discover_coordinator.rs::Pending`及journal | Prepared/Sending/Received/Committed、重放、取消、未知发送记账 | Check接入并发前先证明串行收敛；不另建租约和传输状态机 |
| `outline/frozen.rs::FrozenBuildInput` | document_set_id、documents、document_relations、全输入digest | 类型化现有成员/关系；区分明确引用、推断候选、缺失与冲突 |
| `outline/discover_planner.rs::plan_packs_with_budget`、`discover.rs::ParsePack` | 章节合拆、完整请求预算；已支持多文档原子与显式关联 | 跨文件关联不能仅新增类型：规划器须实际选择关联atom或依赖读取，并保留逐原子来源 |
| `analysis/agent.rs::read_projection_in_context` | full_read_projection已在候选fit循环外构建 | profile实际checkpoint clone、host投影、序列化与测量成本，不重复列已完成优化 |

### 当前解析调用链（已实现）

此处三个层次不能笼统称为同一个“解析模块”：

1. **bidding冻结准备入口**：`outline/frozen.rs::prepare_tender_input` → `prepare_tender_input_with` → `ConfiguredTenderSourceParser::parse`。该适配器调用`docparser::convert_tender_source`，不重新实现PDF/Office格式解析。
2. **Rust docparser路由、传输和契约层**：`convert.rs::convert_tender_source`为招标来源固定选择builtin，`convert_with_cancel`路由到`grpc::read`/`read_stream`；接收并组装元数据和图片、传递取消、校验表格/来源合同。它不只是DTO。通用非招标转换还有simple、anydoc与HTTP引擎入口，不能把这些通用路径当成招标冻结路径。
3. **Python DocReader原生格式层**：`services/docreader/main.py::_parse_request`调用`parser.parse_file`，由registry选择PDF、Excel、Word、图片等实现；生成原生结构、定位和相应格式/OCR结果。Excel显示/单位/类型/公式保真首先在此层及返回合同补齐。
4. **返回bidding冻结与业务处理**：检查source contract和完整性，`build_frozen_input`建立稳定证据；`outline/parse.rs`提供发布次序/完整性辅助；`discover_planner`选择阅读范围，Discover解释招标义务。

另有已实现的补充图像识别路径：`outline/frozen.rs::ConfiguredTenderImageProcessor`调用`knowledge::enrichment::literal_ocr_regions_async`，检查识别完整性，经`TenderImageStore`持久化图片并构建`FrozenImageResult`。因此当前并非所有OCR都在Python。此共享识别调用是内容准备基础设施，不等于knowledge材料召回或投标人事实填写业务已接通。

### 目标职责及接口决策（待实现评估）

纯文字OCR、原图保真及区域定位属于**内容解析职责**；招标义务、适用条件、章节模板和充分性判断属于bidding业务职责。职责归属不直接等同部署进程归属：不能据此立即将现有补充OCR搬入Python，或只为目录一致迁移已验证路径。

第一步保留`TenderSourceParser`/`TenderImageProcessor`/`TenderImageStore`边界，准确区分内容识别输出与业务判断。解析/识别接口应返回原始图像身份、文档/单元/区域定位、literal text、解析或识别版本、完整性及终止/失败状态，并传递取消；不得返回“符合投标要求”等业务结论。bidding消费这些结果冻结证据并独立复核，不把识别成功当业务通过。持久化仍遵循对象owner/staging lease，不能产生无归属裸写入。

再评估现有`ConfiguredTenderImageProcessor`是否只做取消、完整性校验、存储和来源映射的薄适配；若符合，则可保留编排位置，将内容识别实现约束在解析能力接口后。若识别逻辑与招标状态实质耦合、存在重复OCR或无法独立测试，应重构或替换该边界。是否统一到Python服务，须比较原生定位保真、模型配置与预算、取消/重试、图像所有权、故障恢复、重复处理和实测延迟后决定，不提前宣称搬迁更优。

验证至少覆盖同图输入的原文/区域与图片身份不变、识别不完整显式失败、取消停止、存储归属、重载以及不产生业务结论。接口或实现替换后删除被替代路径，不维持双套权威识别结果；在此之前文档明确当前行为与目标职责。本次只记录决策和接口要求，未迁移OCR、改解析实现或调用真实模型。

## 4. 第0阶段：已实现修复

`analysis/agent/context.rs`保留one_shot_progress_marker作为业务完成标记，新增delivered_read_progress_marker及merged_read_ranges。当前作用域内确认交付的新来源、结构和目标区间进入读取进展，不进入业务完成集合。来源身份含input digest与载体；结构回执含pack前缀；区间规范合并。重复、重叠、A/B/A、pending frame以及epoch/nonce自身不产生新覆盖。Discover继续按真实包提交推进。

`outline/agent.rs::host_packet`选择当前未复核要求，显示实际target_refs、阶段与next_unread_slot。比较结束但目标未读/未复核时不跳过要求。投影不签发回执或完成状态。

现有回归位于`analysis/agent/context/tests.rs`：

- `delivered_check_pages_advance_reading_without_completing_semantic_work`
- `check_read_progress_requires_delivered_new_ranges_not_overlap_or_pending`
- `completed_wire_read_progress_rejects_stale_and_tampered_frames`
- `organize_new_delivered_evidence_is_reading_not_completion`
- `check_task_retains_target_body_until_read_and_reviewed_after_reload`

这些测试证明读取/状态合同，不证明真实模型完整召回或Check收敛。真实运行截止后的修复尚未做真实复验。

## 5. 最小串行领域工作

拟议`CheckWork`只承载当前输入作用域下的**源范围处置和领域结果引用**。范围处置包括待复核、存在义务、无响应义务、未解决，并关联已交付证据、理由及当前比较/复核结果引用。准确字段与序列化契约随串行实现评审，不预先落八类任务节点或完整依赖DAG。

`RequirementRecord`仍是唯一可修改的当前要求。拟议`SourceObligation`是独立review观察，用来指出遗漏、误提、条件缺失或模板对应问题；它引用来源并挑战当前要求，不成为第二份可编辑要求列表。经受控repair修改RequirementRecord，再fresh复核观察是否解决。

下一工作由当前源范围处置、Draft比较/复核、目标覆盖、回执与阻塞派生。可重建的next提示不另存权威状态。业务结果只存在于现有领域结果容器；传输准备、发送、接收、重放及未知发送继续由协调器/journal保存。串行阶段不新增Leased/Received/Committed业务状态或自建租约。复核授权仍来自Duty、工具注册、用户运行范围及当前输入/版本/epoch校验。

默认按有界语义来源范围推进：读取 → 独立源观察 → 对照要求及具体模板目标 → 提交比较/复核 → 处理问题。定位粒度可以是原生格，调度粒度不必是一格；不为每个cell固定生成一组模型任务，不先遍历完整目录。跨页续片、条件、例外和替代关系是必要关联读取，未闭合范围不能判无义务。

优先扩展现有`submit_review`的pack-only来源范围及现有claim comparison协议。实现前说明新增字段如何复用当前验证器；目前不预定新增工具。若现有协议确实无法表达某操作，必须给出具体缺口、最小增量和对完整请求预算的影响。

读取进展、合法业务结果提交和最终完成分别统计。模型自述、目录覆盖率、调用次数或预算耗尽都不能设置完成。旧scope结果可作审计和usage结算，不能作为当前复核信用。

## 6. 模板覆盖与唯一完成门槛

源正向复核检查资格、材料清单、格式、评分、技术、商务等相关范围，包含无已提取要求的包。每个范围必须有带证据的处置；`no_response_obligation`是待独立核验的语义判断，不是自动免责。scope accounting能证明有记录，不能证明语义零遗漏。

反向`ResponseSupport`验证义务到具体目标的对应：

- source_copy逐字保持规定原文、归属正确，但复制说明不自动等于具备可填写响应位置。
- editable_blank保持空，具体字段/空位说明、来源、条件及填写目的足以承担义务；不要求填入实际证明材料。
- 表格需指向字段、range、facet或提供完整覆盖证据；whole form id只能证明表存在，不能替代每项义务覆盖。两项字段只覆盖一项仍失败。
- generated_explanation必须有依据，不能添加不存在的资格、承诺或产品事实；可见人工操作说明同样需要具体义务与位置。

金额、比例、单位、期限、上下限、主体、否定、例外和替代条件保留原文锚点。宿主校验来源与字段、发现明显缺失和不一致；数值相同或正则命中不能直接证明语义等价。

唯一template-ready gate合取：必需来源可用且各范围有合法处置；相关源义务对应当前要求及充分具体的模板位置；固定原文与生成解释依据有效；当前目标内容/字段说明已读取；当前scope比较与独立复核有效；repair后fresh复核完成；无未决阻塞。复用validate_final_outline作为结构子校验，不让多个路径各自设置done。

缺失被招标文件引用的必需附件阻塞全项目验收；投标人未来需填写的材料本身不作为缺失来源。缺失外部公告全文时明确未验证范围，不编造时间或条款。可显示部分草稿及阻塞原因，但不能标整项目已就绪。填充后材料是否齐全由下一阶段单独验收。

## 7. 多文件保真与关联规划

三种概念分开：逻辑项目文件集决定业务范围；PDF块/表、Excel格、Word段落/表及图片区域是原生解析单元；LLM执行包按关联及完整请求预算选取原子。不能把所有文件拼成无身份的大文本再切包，也不能把一个文件强制视为一个请求。

直接类型化`FrozenBuildInput.documents/document_relations`，保留一个清单和现有document_set_id。成员表达document身份、内容/解析版本、角色和可用性；关系保留两端定位、明确引用或推断候选、确认依据与未决状态。集合身份及关系变化纳入现有冻结输入digest，不能增加一个可能漂移的平行版本真相。文件名只作线索，不默认主文件或附件优先；重复义务保留多源证据，冲突显式待核。

实际改动必须包括`discover_planner.rs::plan_packs_with_budget`现有同document筛选及`ParsePack`单document边界。规划器选择关联原子并保留document/sheet/cell身份；可在预算内同包展示，无法容纳时发起显式关联读取。模型wire映射、pack身份和逐源覆盖均需验证。复用现有规划器，不建planner服务；跨文档合并不能借用其他来源的完成回执。

| 格式与代码 | 当前能力/缺口 | 计划要求 |
| --- | --- | --- |
| PDFParser、docparser来源契约 | 页、块、表和续片路径存在；真实验收只覆盖PDF且Check未通过 | 原文/原图、续表和条件来源可追溯，解析失败显式 |
| `services/docreader/parser/excel_parser.py` | _cell_model当前text=str(value)；load_workbook使用data_only=False，未保留number_format/type等完整语义 | 保留相关display/unit/type、公式表达式/ref及可用cached value；例如0.6的百分比显示不能丢成普通数值。无缓存或无法可靠显示时明确不完整，不编造计算值、不自建计算引擎 |
| Docx2Parser/DocxParser及fallback，旧doc转换 | 段落/表路径存在，fallback不保证稳定定位等价 | 分路径验证定位和规定格式，保真不足显式阻塞 |
| ImageParser/视觉来源 | 原图及像素身份存在；注册格式不代表OCR/视觉成功 | 保留原图/区域、识别完整性及跨文件引用，缺失不能丢弃 |

合成多文件对象使用自行生成的主PDF、报价Excel、规定Word及图片附件，此为测试样例而非产品格式或文件数限制。主文件角色与格式独立，也可使用 Word 主文件、PDF 附件、多个同格式附件及不同文件数；仅接受实际解析器已支持且原生合同完整的输入，未知格式或信息不完整应显式阻断。格式注册列表不等于语义验收通过；此矩阵尚待实现与验证。

## 8. 版本与修复：第一版保守边界

`EvidenceRef`绑定整个FrozenInput digest，读取回执绑定Discover revision/Check epoch。因此第一版只在**固定文件集**内做领域局部repair；新增、替换或移除来源后重新冻结并fresh全项目Check。可复用内容一致的解析缓存，但不复用旧集合的读取/比较/复核信用。跨集合增量复核以后单独设计，不承诺本轮实现。

固定文件集内复用`DiscoverWork::reopen_committed`、幂等operation_id及repair签名。明确改动要求/目标/条件关系的受影响闭包；未证明独立时保守扩大领域结果失效。领域草稿保留与读取信用是不同问题：即使保留无关目标，凡全局revision/epoch已使回执失效，必须重新交付和复核，不绕过现行验证器。

repair提交检查当前版本；旧工作者晚到结果不应用。修改后重新加载当前状态，以新鲜证据独立复核，不能以修复者自述通过。禁止静默删义务、缩小required集合或把全部问题改成人工任务来降低门槛。误提/不适用只能经有证据的修订解决，并保留审计。稳定问题签名用于避免无限重开。

## 9. 性能、预算与运行边界

`analysis/agent.rs`已经把full_read_projection置于候选循环外；不再把此项列作待实现。先profile循环中的checkpoint clone、host构建、序列化、fit/token测量及保存成本，再优先尝试request-local不可变数据复用。当前不预建四类全局cache，也不承诺无基线的50%收益。

固定fixture/source及历史hash，记录输入字节/消息数、图片、候选页/最终页、fit次数和各阶段耗时；网络等待与本地CPU分别记录。可比工作负载重复测量，报告中位数和范围，并校验页内容、游标及续页重构hash。改动不得改变语义或丢来源。只有实测收益支持时再扩大缓存范围和模块抽取；缓存命中从不授予阅读信用。

最终准入仍测量完整系统指令、工具schema、历史、host、视觉载荷与输出预留，配置窗口131072。自定义模型tokenizer须有可核实资料或实测依据，不冒用其他模型配置；经验余量不当作长窗口能力证明。不恢复固定字节分包、业务操作次数硬限或provider max_tokens。取消、有限传输重试、未知发送占额、真实无进展保护保留。

私有验收累计预算与截止授权属于测试运行控制，不硬编码产品。新的真实服务测试必须获得新的明确窗口；本次文档修订不授权调用。测试范围扩大也不能自动续测。

## 10. 实施顺序、文件与责任

以下全部是待实现计划，不以文件拆分数量作为交付指标。

| 顺序 | 必要改动与位置 | 验收出口 |
| --- | --- | --- |
| 1 串行垂直闭环 | `outline/agent.rs`选择工作/host，`tools.rs`和`claim_review.rs`扩展模板覆盖，现有submit_review扩源处置；最小CheckWork按需放outline内模块，OutlineRun仍作容器 | 源范围→要求→具体空位/表字段→比较→复核→单一gate；重载/裁剪历史不丢工作；语义正反例 |
| 2 多文件保真 | `excel_parser.py`及docparser冻结契约保真；类型化现有frozen清单/关系；`discover_planner.rs`、ParsePack及wire准确改动 | 四格式来源身份、百分比/公式、关联选择、缺失/冲突及文件集变化全Check |
| 3 局部repair | `outline/agent.rs::reopen_discovery_pack`及现有DiscoverWork受控重开，必要时提取repair模块 | 固定集内正确失效、全局回执边界不绕过、旧版本拒绝、fresh复核 |
| 4 实测优化 | 实际热点所在`analysis/agent.rs`请求构建/fit，先request-local复用 | 等价性与可比性能证据，不先规定全局缓存架构 |
| 5 后置Check并发 | 串行收敛后评审现有coordinator/Host适配，复用journal及预算 | 单提交者、不可变请求、乱序/重放/取消/CAS；Discover现有并发不退回串行 |

解析层拥有原始结构、定位和不完整状态，不判义务。bidding拥有唯一要求、模板、源观察、复核及gate。Check工作选择只读这些权威结果；host视图不修改状态。通用运行时拥有传输、预算、token测量、重放，不导入Draft或判断业务义务。领域来源投影属于bidding；公共LLM层只处理请求层数据。knowledge业务接入下一阶段。

文件过大本身不是重构理由。只在具体变化需要时将host/review/repair或请求fit提取到相邻模块，并同步测试；不预先移动全部目录、不新增crate/工作流服务。替代逻辑在当前契约切换后删除，不保留两套Check调度；来源身份、exact-wire和版本校验继续保留。

## 11. 验收矩阵

| 场景 | 必须验证 |
| --- | --- |
| 合法空白模板 | 具体义务→字段/空位/规定格式、来源及条件完整可template-ready，保持空text，不要求实际投标人材料 |
| 泛化说明与表格粒度 | “请提供材料”无具体位置失败；两字段只覆盖一字段失败；whole form id不能代替覆盖 |
| 源遗漏与误提 | 有义务范围模型判no_obligation计语义漏检；无义务却添加资格/承诺计误提；独立预期不由被测模型生成 |
| 条件与关键内容 | 金额/比例/期限/否定/主体、跨页例外/替代关系及各业务类别有独立正反例 |
| Excel保真 | 数值0.6+百分比格式、公式/引用/可用缓存、缺缓存；展示/单位不丢、未知显式、不自行算出值 |
| 多文件关系 | 同名表/跨sheet不混账，主文到Excel/Word/图像保留原子来源；推断未确认不自动优先 |
| 来源缺失 | 真实缺附件/公告记录带证据的待处理issue及影响范围，阻塞相关模板范围和clean-ready；独立范围继续，剩余明确文档问题保存报告后受控停止，clean-ready仍失败；投标人待填材料不是招标来源缺失 |
| 文件集变化 | 新digest后旧结果拒绝，全项目fresh Check；解析缓存不转移复核信用 |
| 固定集repair | 共享条件影响多个目标、无关领域结果保留有证据；全局epoch过期回执不可复用；修复者不自批 |
| 重载/历史裁剪 | 序列化、重载、清空transcript后next工作和当前结果一致，不从聊天声明恢复 |
| 交付与进展 | pending/字节篡改/旧scope/重复/重叠不获信用，新读仅推进读取，不自动完成 |
| 完整请求预算 | Unicode、图片、工具schema、历史和输出预留全计入；不截断来源，未知发送保留占额 |
| 后置并发 | 乱序、重复Received、取消、旧结果和冲突CAS；仅一个领域提交者 |

先用确定性合同测试与独立标注语义夹具证明对应目标，再经授权实测原文件及页图。scope记录齐全不证明语义零遗漏，本地模拟不证明真实服务收敛。报告必须区分通过/失败/未测、模型调用数、usage、时间与版本。

真实历史结果按模板阶段重解释：未填投标人事实或没有实际证明材料不单独构成本阶段失败；模板位置泛化、义务/条件遗漏、字段不充分仍是真实问题。历史原始调用、输出和usage不可改写，后续解释追加到独立验收记录。

## 12. 文档与验证记录

本文件是当前方案及实施顺序的唯一主线。`docs/bidding/frozen-input.md`、`docs/design/evidence-runtime-contract.md`、`docs/bidding/outline.md`只记录已实现合同；旧总体方案仅作导航。具体已实现范围见本文开头状态及 frozen-input.md；局部 repair 和真实多文件模板验收尚未完成。

本地常用验证入口：

```sh
python3 scripts/run_local_tests.py -- cargo test -p bidding --test authoring_schema_contracts
python3 scripts/bid_authoring_schema_validation.py
python3 scripts/run_local_tests.py -- cargo test -p bidding -p docparser -p knowledge --lib
python3 scripts/run_local_tests.py -- cargo clippy -p bidding --all-targets --all-features -- -D warnings
```

私有原文件、文件名、抽取正文、截图、模型输出及原始运行记录只保存在私有证据包，不进入公开仓库。公开测试仅使用自造材料，记录实际验证边界，不把模型模拟或单测数量写成端到端通过。

### 发布复核证据的信任边界

`OutlineArtifact.review_proof` 保存复核版本和读取回执供同一 template-ready
门槛重新校验，不是签名或授权凭证。模型只能提交复核判断；读取回执由宿主在
已完成的实际请求交付后记录。版本绑定冻结来源、要求、主张比较及响应目标内容。
`validate_publication` 是内容一致性校验，不能证明任意调用者提交的 JSON 来自可信宿主。
生产写入仍经过已有 journal 和数据库边界：最终宿主 checkpoint 先持久化
publication receipt，`kb_bid_v2_publish_outline` 在行锁和 worker token/epoch
校验后比对 checkpoint 中的产物 SHA256 与 bindings，再写入。仅伪造 artifact
的 pass、proof 或 receipt 不授予 checkpoint 写权限。能修改宿主或持有数据库
worker 写权限的调用者属于现有可信执行边界；本阶段不增加认证框架。

### 第1阶段当前实现边界

- `source_review` 的处置是复核结果，只引用唯一 `RequirementRecord`；来源范围由
  `DiscoverWork` 派生，不维护第二份义务正文。零提取范围也必须显式处置，来源实际
  读取、理由、版本和范围身份均校验；未解决判断阻塞完成。
- `template_review` 为每个显式主张保存版本绑定的具体槽位、表格范围或操作说明判断。
  空白投标人字段可以通过；主张漏审、越界、跨目标、过期和缺回执拒绝。
- finish、投影及直接发布内容校验调用同一门槛。持久化字段带入发布产物的复核证据，
  最终授权仍由下面记述的 journal/数据库边界承担。
- 独立本地 mock 通过生产 request/execute_turn/持久化入口运行；mock 只消费传入的
  请求。修复后的 fresh Check 和清空历史重载有独立断言。结果仅说明协议和状态流转，
  不证明语义召回、实际模型表现或私有投标文件验收通过。
- 多文件保真已有模拟闭环，局部repair和目录分页已编辑但本轮整体回归尚未完成；性能仅有测试计时，Check并发后置。当前修复复用受控重开流程，不能从历史聊天恢复信用。


## 当前并行整改目标合同（未通过集成验收前不宣称已落地）

### 模型显式返回原文问题，结果与问题并存

真实文档缺附件、冲突、语义模糊或不可辨认，须记录来源证据、分类、影响范围及待处理问题；已完成独立可执行范围后，以保留问题的受控停止返回报告；当前不新增`completed_with_issues`状态，不表示问题已解决或全部模板可用。相关范围保持blocked，独立范围继续；clean-ready必须失败。系统漏提、错误绑定、丢失状态、未交付证据或解析器失败不能改称文档issue逃避修复。问题记录复用现有领域结果，不创建第二份RequirementRecord或通用工作流平台。优先扩展现有`submit_review.issues`结构化响应，让模型在正常`source_dispositions`/`template_reviews`旁返回原文问题；宿主验证来源和影响范围并持久化，由问题报告投影呈现；阻塞情形不发布模板artifact。当前已编辑schema保留`id/code/description/requirement_ids/evidence`，`requested_information`必填，`evidence`至少一项；用于表达待补充或确认事项，尚待集成测试。document_missing/conflict/ambiguous/unreadable与system类问题分开，缺附件必须引用实际提及处，不能伪造缺失正文证据。当前不新增完成状态机或`completed_with_issues`枚举。

### 三类 finding 的出口与失效

| finding | 修复出口 | 失效与验收 |
| --- | --- | --- |
| 独立来源观察发现漏提，包括零要求范围 | 受控重开准确发现范围，修改唯一RequirementRecord；不是只能对已有claim比较 | 按来源/要求/条件支持/响应目标/章节依赖闭包撤销结果；fresh读取、重新提取及Check后关闭finding |
| 已有要求的误提、条件或跨文件支持错误 | 允许修改、删除、重提取及重新判定支持；不得永久锁住旧关联 | 删除或修订传播到消费者、目标及章节；保留独立草稿，旧Check信用不得保留；重载清历史仍正确 |
| 目标、source-copy或模板覆盖错误 | Organize局部修正绑定、槽位和规定文字 | 撤销受影响比较/模板复核，重新读取来源与目标后Check；修复者不能自批 |

真实文档问题走上述独立issue结果；系统finding未修复仍阻止执行成功，不通过更换标签关闭。来源集合变化必须重新解析冻结并全新Check，不跨集合迁移回执。

### 来源副本与响应角色

招标人的instruction是解释义务的来源，不自动复制成投标人承诺；明确要求原文填报的mandated response才形成固定source-copy；投标人事实进入editable blank并保持空白。同一段包含指令和规定响应时按有证据的范围分别处理，不能整段复制或凭标题猜角色。B已修改`SourceCopyPurpose={instruction, mandated_response}`作为source-copy必填目的；`editable_blank`沿用独立槽位类型，不再添加第三种copy目的。该代码尚待统一编译及验收，不能据编辑完成宣称测试通过。

### 视觉范围、身份和读取进展

普通文字/表格PDF不要求全页提前渲染或逐页视觉复核。扫描页、真实图像及实际不可辨认的证据保留原图需求和准确source/region身份；OCR文字、缓存文件存在、别的来源同像素均不授予读取信用。新读取进展是当前来源身份下已确认交付区间的union增加，重叠/重复/过期/pending不推进。视觉分类必须来自解析合同及明确证据，不用格式名推定所有PDF为图像。

### 大附件读取与配置边界

目录用于定位，正文/网格/图像按完整实际请求预算分页，游标绑定输入、来源和当前修订；不能把目录视为正文已读。附件格式、数量、主附件角色独立，目标endpoint按已配置支持能力选择，禁止静默换供应商。source-set、支持关系或来源内容变化后清除过期工作和游标；显式缺失形成文档issue，传输/解析失败保持系统失败。复用现有manifest、关系和阅读工具，不新建泛化平台。

以上整改需包含正反例：零要求漏提重开；支持删除传播；instruction不冒充响应；新读区间union；普通PDF无全页图前置；大附件预算分页；真实文档issue允许执行完成但clean-ready失败；系统缺陷不能被issue状态吞掉。最终统一本地回归和CI，真实验收等单runner窗口，不提前调用provider。


当前B编辑将图像缓存键改为完整`view.id`身份，像素确认检查准确`view.message`，worker delivered-related-evidence的原生区间按实际原文carrier身份合并union，不按support origin分割同一原文范围。当前schema已编辑`read_evidence`的`dependency_id`/`dependency_cursor`四择一选择及`read_outline`的`manual_task`（requirement_id/task_id）；尚待集成测试。检查点常量已更新为26。

Discover `submit_pack`与独立Check `submit_review`均支持下述问题合同；新增Discover入口尚待最新编译验证。阻塞问题已有下述代码出口，四类问题的定向runtime测试已通过，整体集成回归仍待完成；不得提前宣称`completed_with_issues`或完整产品UI已实现。


### 当前问题报告出口（定向测试通过，整体回归待完成）

`Checkpoint::review_report(input)`纯投影`results`与`issues`：results包含requirements、chapters、bindings、slots、fulfillments、已复核requirement/pack IDs、template_reviews和source_dispositions；issues包含问题正文、按same-carrier原文区间实际overlap计算的`affected_pack_ids`及`resolved:false`。报告附输入摘要、discovery revision、review epoch、`template_ready`与`document_issues_reported`，不新增结果或问题store。数据库checkpoint仍保存权威状态。`progress.outline_review_report`仅保留元数据与issues，移除results，避免每tick复制完整结果；完整API accessor及私有报告包含results与issues。

独立scope完成且仅明确文档问题剩余时，`prepare_request`先保存checkpoint/progress，再返回`DOCUMENT_ISSUES_REPORTED`受控停止；这一出口不调用provider、不占新reserve，不publish、不置done，也不把问题改称clean pass。私有真实runner必须在run返回Err时仍读取已保存state，调用`review_report(input)`输出私有报告，不能丢掉问题。现有Web旧progress路由尚未接成新的报告文件下载；不承诺完整UI闭环。

### 附件目录的实际边界（定向验证通过，最终集成待完成）

目录每页一个relation及小段首原文preview；`dependency_id`/`dependency_cursor`与`refs`/`cursor`互斥。目录frame的exact-wire确认只推进目录读取，不产生来源正文信用。手工任务读取键绑定input/epoch/revision/requirement/fulfillment，实际信用仍须exact-wire交付。重开清除旧支持。`condition_support_options`首页仅一个身份及原生ref预览，`next_support_cursor`通过同一`read_evidence dependency_cursor`遍历持久grant；正文可用`dependency_id`选择`cs_` support_key完整范围，再用既有cursor分页。`ReviewedRequirement`不再内嵌excerpts。这些新改动尚待最新编译测试；单个不可分身份、locator或图像超过完整请求预算仍明确阻断，不能承诺任意输入无界运行。


最新定向验证：outline 150 passed、Discover 25 passed、手工任务分页1 passed、文档问题报告1 passed（一个runtime测试循环四种code）。问题报告验证无provider/reserve、保存报告且不批准模板；只有完成全部独立scope后才报告明确文档问题。Check复核与新增Discover问题提交分别遵循下述信用边界。`synchronize_discovery`在prepare前、Discover结束后和read_projection前重建唯一canonical投影。以上是本地定向结果，不代表最终全回归、CI或真实服务验收通过。

完整synthetic mixed测试已通过，包含results+issues报告断言、独立scope继续及reload。该结果只验证本地合成合同；当前整改完整回归尚待完成，无新增真实服务调用。

公开提交`4b3b9410d8efc7caff5c1792f643cb8f9efbb7ff`的CI run `38094511856`已completed/success：Python、Rust、Web、schema-contract、content-e2e、Postgres acceptance、DocReader replay及queue-faults通过，images跳过。它取代旧`ceef0d6`的Python失败/待重跑状态，但只覆盖该公开提交，不代表当前尚未推送的整改代码已通过CI。


### Discover同次返回要求与问题（新增代码待最新编译验证）

`submit_pack.issues`必填，复用`ReviewIssue`；证据ev_key必须属于当前包owned范围，仅接受四种document code（missing/conflict/ambiguous/unreadable），`requested_information`非空，Discover阶段`requirement_ids`为空数组。允许正常要求与问题同次返回，也允许空要求加合法问题；后者必须完整inspected且`no_requirement_reason=null`，不能把未读或系统漏提伪装成文档问题。

canonical问题保存在所属已提交`PackRecord`，身份为`discover:pack:revision:modelid`，经现有draft投影展示，不新增问题store。独立范围继续处理，reopen使旧revision问题失效。Check可用同id在fresh读取后补充requirement_ids并持久化重载；Discover问题本身不授予Check阅读或复核信用。该新增合同尚待重新编译和定向验证，之前测试结果不覆盖它。


### 最新本地验证状态（真实运行前冻结）

新增Discover问题合同重新编译后，15项discovery定向测试、正常/空要求问题跨Organize与Check重载测试、Organize保留问题但撤销旧信用测试、failed worker修订重开/unknown-send保护测试均通过。它们取代上文相应“待编译”描述。全workspace/all-targets/all-features Clippy（-D warnings）及最终fmt检查通过。全workspace测试两次在链接无关测试目标时耗尽磁盘，未完成，不能记为通过；后置全量测试，真实runner先执行已验证的相关合同。公开CI结论仍只覆盖上述公开提交。真实PDF语义结论另以实际调用和独立原文核对报告为准。

### 分包算法成本与反例

轻量权重来自冻结内容和结构的字节量，不引入固定字节包上限，也不冒充token。原始单元/表格权重一次构建；必要分片后按拥有范围及表头范围轻量调整并重建章节前缀，不反复展开全表。顺序及原有关系约束不变，不做打乱顺序的bin packing。容量提示只在本次规划存活，不缓存模型句柄或准入结果。

边界查询对单调权重前缀为O(log n)；来源、表格计量与字节量相关。关系合法范围仍需扫描，不能笼统声称整个规划为O(n)。真正超限后的调整每次严格减少章节或原子数；在非单调预算反例下最坏可能逐项下降，不能声称BPE二分的对数保证。单原子仍失败时复用既有UTF-8/表格锚点拆分；不可拆则明确失败。

回归覆盖不均匀大小章节（按章数对半会仍超限）、后续包复用实测容量而不重测全部剩余范围、非单调真实预算、UTF-8与表格分片、关联端点、原子一次有序覆盖，以及近窗失败修复审计与未交付读取保护。最终通过状态以本轮实际测试日志为准。
