# Python 招标第一期交接

## 独立环境联合 B/C 和四源实跑

- 2026-11-18 00:04 PST（08:04 UTC）：定位并修复 **BUG-248**（`bind_native_targets` / `bind_source_body` 未复用 `EvidenceRepository` 与 `cache` 导致逐目标重复校验全书 PDF 字形，且 `outline_from_compiled` 与原生分类准备在主事件循环同步执行阻塞租约心跳触发 `StaleExecution`）。红绿测试与全量 **3,874** 项单测全部通过（证据 `bug248-heartbeat-cache/`）。
- **111 首轮候选提示单包实测（`111-b-prompt-single-ZPyr5w/send1/`）**：对原运行 4 次均因未认证正文引用被拒的第 7 包单次实发，未认证 `body_refs` 错误从 40 项降为 0、开放边界归属一次合规，但漏列第 14 页 `764..803` 共 40 个原子，触发 `Disposition coverage must preserve the complete supplied source denominator.` 宿主拒收；未获单次通过，按预案不安排整份首轮提示重跑。
- **222 后继（`web-outline-3334c1a3...`）已完成并出件**：在完成全部模型请求（B 25/29 通过、4 耗尽，C 3/5 作用域通过、2 耗尽）后应用 BUG-248 修复，以 **0 次新增模型发送**在 28.605 秒完成收尾并导出 `222-next-live-v1zWWWsY/completed-222/`。独立专项门禁 `remaining-delivery-audits-igwAXEvk/222.audit.log` 退出码 **`0`**（`passed: true`：8 张原表、主稿 3 处信誉查询截图独立空栏、ZIP 打包均通过；大纲中另有 387 条 `outline_directory_requirement_unassigned` 与 88 条 `outline_directory_native_source_decision` 留待人工核定，因此四源完整语义与归属验收仍记 **0/4**）。
- **111 CMap 后继（`web-outline-52b006f6...`）已完成并出件**：完成全部 43 个 B 包（27 通过、16 耗尽）及参数/响应表后应用 BUG-248 修复，以 **0 次新增模型发送**在 51.171 秒完成收尾并导出 `111-cmap-export-wPO0WPaJ/completed-111/`。专项审计 `anfu-111-delivery-audits-zktWvwBh/111.audit.log` 显示 CPU 原表、补充分卷与附件待核通过，但因所绑清单 `e26c83b3...` 的 3 条 `credential_slot` 事实 `entry` 为 `None`（而第 3.4 章投标保证金带目录入口 `page: 79, line: 10`），3 处保证金空位落入 `outline_directory_reviewed_fact_pending` 待核（`bank_guarantee_ok: false`）。
- **原候选环境（`kb-literal-candidate`）**：Bidding（`web-outline-c8cfdf5a...`，B 49/63 通过、14 耗尽）应用 BUG-248 修复后以 **0 次新增模型发送**完成大纲落库；导出时因 `Bidding.pdf` 第 21 页行末连字符的 PDFium 占位符 `\ufffe` 进入待核项触发 `lxml` XML 字符校验异常（已记 **BUG-249**），且因该环境使用 BUG-244 前镜像（全量 C 超限回退），2 条核定独立正文留在 `outline_directory_reviewed_fact_pending`。原 111（`5e242ac4`，B 28/43 通过）与安服（`03a20559`，B 58/71 通过）同样在 B 后同步收尾处停于 `StaleExecution`，已接受回执完整保留在库。

- BUG-246 `23afc97` 全量单测 3,869 项通过。111 正式回执对照的 43 个首轮摘要一致，已接受回执通过重验。改进后的 26 个修订请求均通过输入预算。BUG-247 `11b7a57` 按原件顺序物化待核和参考来源，72 项定向通过。含两项修复的完整字符映射候选镜像 `kb-docreader:joint-next-cmap-11b7a57` 已构建。现有执行仍使用各自固定镜像。证据 `bug246-GFAwNZW9/`、`bug247-order-obD6Ti/`、`111-order-fix-image-6XwoF5tH/`。
- 2026-11-17 22:04 PST 的 111 CMap 回放中，旧全量 C 估算输入 464,566，输出上界 982,314，分别超过 94,208 和 32,768。分包版准入 1 个文件级包和 10 个案例包，736 个案例进入自动裁定，602 个因相连组整体超限保留待核。全失败对照保留 2,896 条待核记录原 ID 和内容。另测首轮候选提示保留 43 包边界和所含 3,145 个来源范围，43 个请求摘要均变化。证据 `111-cmap-C-capacity-znRCQGsp/`、`111-prompt-preflight-stnt0KEH/`。
- 111 的保证金凭证主稿验收补齐三处具名空栏。原件要求一套投标文件和平台指定格式，补充分卷按实际内容核对。安服另逐格对照物理第 91、92 页报价表，并检查第 63 页签章和日期空栏。两份出件后由 `kb-tender-anfu-111-audit-monitor.service` 执行专项审计。证据 `111-guarantee-gate-dUwvBg/`、`anfu-targeted-gate-7ig4E8zt/`、`anfu-111-delivery-audits-zktWvwBh/`。

- 2026-11-17 21:46 PST，四份完整成品验收仍为 **0/4**。原 Bidding、安服、111 分别有 **36/63、28/71、17/43** 个正文包通过宿主验收，原 222 有 **23/29** 个通过且已出件。111 完整字符映射后继有 **4/43** 个包通过，222 后继已有 1 个正文包通过，另有 1 次正文请求在途。五个在途执行均持有有效租约，原四源监控和两个后继监控均持锁。
- 222 原件第 22–25、46–48 物理页规定一套投标文件及其组成。独立商务分卷不属于原件必交项。修正后的审计把主稿三项信誉凭证空栏单独设为通过条件。原成品重验仍为 0/3，八张原表与 ZIP 通过。新门禁及对照证据 `222-source-required-audit-nOq3st6c/`。222 后继运行、导出及监控目录 `222-next-live-v1zWWWsY/`，111 后继目录 `111-cmap-export-wPO0WPaJ/`。
- 111 附件性能表直接对照已补证。DOCX 段落 1277 与 PDF 第 63–64 页同属防病毒软件，段落 1489 与 PDF 第 68 页同属威胁行为防护软件。两对指标格去除版式空白后逐字一致。后者两侧均有 CPU 不高于 2% 和内存不高于 100 MB。旧报告跨产品比较的结论已加注更正，附件适用性仍待平台证据确认。证据 `111-embedded-table-identity-JY8HeO5j/table-match.private.json`。
- `kb-tender-source-audit-monitor.service` 等待 Bidding 与 222 后继完整交付包，再执行原件专项门禁，结果写入 `remaining-delivery-audits-igwAXEvk/`。111 后继及原安服已有导出监控。

- 2026-11-17 21:32 PST，222 首份实际成品已导出并独立检查。八张原表的文字和合并格均通过，ZIP 校验和主稿一致。三项信誉截图专用空栏均缺，包内无商务分卷，另有 680 条要求待归属。该次使用旧三事实清单，后继六事实清单已离线验证。证据 `original-live-audit-0Yii4Vh2/222.private.json` 和 `post-outline-export-Aytf4ruN/completed-222/`。
- 同一快照 Bidding、安服和原 111 仍在运行，分别接受 33/63、20/71、12/43 个正文包。导出监控覆盖原四项。四份完整成品验收仍 0/4。
- BUG244 最终离线回放对照见 `joint-c-partial-capacity-DcyYgXwF/final-xoyj82/comparison.json`。原运行中的任务使用固定镜像。222 的后继验收需使用新版本和已核后继清单，继续依正式结果核对原表、凭证和章节归属。

- `e7802a1` 本地已准备兼容和完整字符映射两份镜像。兼容镜像四源严格回放 206 个分包摘要、33 条已接受回执和 75 条拒绝记录一致。111 完整字符映射版的旧目录和材料回执命中 0/2、0/1。临时重绑目录得到的 43 个下游批次与旧批次摘要无交集。证据 `joint-next-e7802a1.7BEHOZ8W/REPORT.md`、`e7802a1-four-source-replay.67ayCifq/comparison.json`、`111-cmap-reuse-3GCZr3ZE/REPORT.md`。
- 2026-11-17 21:00 PST，Bidding／安服／111／222 的宿主已接受包为 18/63、7/71、7/43、15/29，合计 47/206。四份仍持有原租约，各有一个在途请求，均未发布正式大纲。完整成品验收 0/4。导出监控覆盖全部四项。
- 兼容镜像四源离线回放复现 206 个分包摘要及各项目进入 C 前的编译结果，核对 33/33 条已接受回执、47/47 条初始请求和 75/75 条拒绝记录。早期回放只读挂载阻止生成临时几何文件，改用可写的隔离内存目录后完成对照。零模型发送。证据 `fd344dc-replay-p2WVjM02/REPORT.md`。
- `50ef77a` 正文分包并发已通过四份真实回执的串并行字节对照。`fd344dc` 凭证入口、主体、重复空位和纠正反馈修复，全量 Python 单测 3865 项通过。候选镜像 `kb-docreader:joint-next-compat-fd344dc` 保持在途的 Poppler 投影，`kb-docreader:joint-next-cmap-fd344dc` 含中文字形映射修复，均已构建并验证固定源码，尚未部署。111 换映射后须重新核对投影和可复用请求，旧清单不跨版本直接沿用。证据 `bug239-instance/`、`joint-next-fd344dc.rDrMK8du/`。
- 111 与 222 的真实来源切片由回放目录进入正常的宿主编译和导出路径，同一份大纲生成主稿、商务分卷和 ZIP。每份各三个独立空位，原条款保留。图像处置的四份局部回放从八组待核精确缩减至四组。111 的原 DOCX 适用性及四处参考截图保留。证据 `bug239-instance/entry-replay-result.json`、`staged-image-review-dFMuieDy/candidate-facts-smoke-ljbrsu2s/summary.private.json`。
- 修正中文字符映射后，111 的独立冻结保持 3146 个范围，9 条事实通过重新绑定，其中含三条具名凭证。当前为仓库外的离线验收身份。PDF 第 68 页与附件同一产品表均包含空闲 CPU 不高于 2%，已更正前次跨产品表的误比。完整验收同时核对第 85–86 页的银行保函条款和原式。证据 `joint-next-fd344dc.rDrMK8du/rebind-111.log`。

- 真实项目已进入独立 `kb-literal-candidate`。四份上传回读 SHA、402 页和 253715 个原字符通过。按当前实际冻结输入迁移 Bidding 10、安服 51、111 6、222 3，共 70 条事实全部通过加载，68 个依赖工件已附加导入独立 CAS。证据 `kb-literal-candidate/web-freeze-ShzYt62H/` 和 `web-fact-migration-WDBCgT6T/`。
- 联合编制已接上核定事实，兼容 Bidding 两事实共享来源但分别保留独有续段；部分重叠整格在 Word 用短说明待核，选择器移到独立证据。B 分包失败保留其独占页面的普通正文。C 超预算或来源不可用时，具名事项、已核原文和人工空位保留在 Word。当前合并定向 110 通过，入口追加 11 通过，隔离 PostgreSQL 完成 3 项构建执行与恢复回归。全量 3855 通过在追加入口前启动，不算追加后的全量门禁。证据 `joint-BC-reviewed-94V7bKg4/`。
- 同一批归档目录和冻结源的分包复测覆盖 11299 范围，B 用 58/45/40/29 包，共 172 包，超限范围为 0。C 最小未决场景实测输入为 72273/86416/69037/46526，输入预算 94208；完整 C 仍要按 B 实际输出复验。证据 `joint-mixed-four-QVnjxg1h/`。
- 候选源码与镜像建构快照的 551 个文件逐一核对 SHA 一致，最终入口在内的全量 Python 单测 3856 项通过。111 的宿主投影与候选投影仅第 1 页有差异，按候选投影复核六条事实原字范围和坐标后已通过准入。证据 `unit-live-candidate-6X2MfBWK/`、`candidate-fact-refs-BbEnKSdp/`。四份已进入普通 Web 执行。安服在途使用 51 条清单，独立复核的 50 条清单排除含第 62 页未能唯一核定日期锚点的分组。采用前须复核这处分组与日期空位。
- 独立候选 DocReader、dispatcher、worker 使用 `kb-docreader:joint-entry-023ba0d0c3ef`，API 与 OnlyOffice HTTPS 健康检查均 200，实际 worker 为 `low` 和 `native_requirement_sources_v1`。候选实时任务回执 `outline-candidate-driver-gZ8hiXBe/`。成品验收仍 0/4，Word 待核区在主稿，分册待核归属仍需按实际职责核验。

## 首轮原字正文接入与四源容量验证（本段优先）

- 本阶段提交保存首轮原字接入、附件、图片待核和 Word 导出修复，尚未部署。四源完整模板验收仍为 **0/4**，本轮未发模型请求。覆盖原字、附件、冻结与分包的 110 项回归通过，证据 `unit-commit-wLAu3Bdu/pytest.log`。
- BUG234 已接通显式联合 B/C 的正文选择及 Word 来源绑定。四源 5,698 / 9,964 原字行唯一定位，校验约 0.98 秒。已承载行保持既有路径，新选原字使用同一投影坐标作排除检查。4,266 行仍留来源待核。两页合成 PDF 的跨页正文、原本存在的 `#`、尾句、相邻条款和 DOCX/ZIP 一致性通过。证据 `literal-layout-identity-FjIvOpXB/four-source-locations.json`、`literal-word-regression-EQEYHhXU/pytest.log`。
- 以四份真实冻结和 v36 已有目录做离线测量，11,299 个来源范围完整保留，Bidding/安服/111/222 分别需 64/53/46/32 个 B 包。195 包均通过输入及合法输出形状预算，个别包接近 32,768 预留。原逐行试包合计约 325.52 秒，缩减重复试包后约 73.45 秒。调用数量仍受每个正文组可选择整套章节的最坏输出边界影响。证据 `joint-integration-KH9gRQJM/budget-four.json`、`native-pack-search-eW5V9HYO/budget-optimized.json`。
- 正常入口还未启用联合 B/C。当前带核定事实的修订显式要求旧分类路径，且旧核定对象绑定旧冻结和完整输入。要先验证事实在新冻结中的来源关系，明确已核定正文的唯一承载，以及 C 对未决来源的预算与处置，再做正式四源 Word/PDF/ZIP 验收。候选预检 `isolated-candidate-plan-lMKqJbzN/PLAN.md`，配置仍按用户选择 `low`。

## 111 内嵌原件与审查出口已接入（本段优先）

- 当前仍为 **0/4** 最终成品验收。BUG230 本地修改尚未部署，独立候选栈未启动本轮模型任务。
- 四真实原件重新冻结并重载，共 402 页、253715 个原字符，四原件 SHA 均一致。耗时依次 5.748、16.283、16.002、3.641 秒，总计 41.674 秒，包含证据重载核对。外置证据 `2026-11-17-source-contract-realignment/attachment-four-audit-1zOuvow3/result.json`。父 PDF 共 196 处图像待处置。111 的单个内嵌载体有两个原名，子 DOCX 256844 字节、631 个逻辑范围、17 个原生表、2 处图像，保留父子适用性待核及子件自身问题。
- 真实 111 隔离导出探测中，Word 和 ZIP 的待核清单均只列一次该附件并保留填写位，附件未列为投标证明。证据同目录 `111-attachment-export-probe.json`。这是单项监督样稿。图像和附件仍按原件绑定核对，生产未导入评审处置。
- 111 的 30 个非精确匹配长段逐项对照后，29 项确认是版式差异，1 项对应 PDF 实体第 51 页公式及 DOCX `Equation.3`。独立读取 PDF、内嵌对象原生流及 WMF 后确认公式一致，子件整体适用关系仍待核。证据 `111-paragraph-compare-ONEulMTX/REPORT.md` 与 `FORMULA.md`。
- 196 位点有 8 个图像哈希，192 处是装饰候选，4 处是参考截图。其中 111 第 85 页的金额为保证金提交示意，222 第 68–69 页的三类信用截图须留真实凭证位置。隔离回放验证 192 处装饰候选可按原件和图片 SHA、位置及坐标生效，剩 4 处参考图待核。新大纲待核出口在原快照显示 8 组，在隔离回放显示 4 组。111 子件另有 2 处独立哈希图像。证据 `image-position-review-g7y9CkFp/REPORT.md`、`attachment-four-audit-1zOuvow3/image-review-outlet-audit.json`。176 项覆盖冻结、编制和导出回归通过，证据 `attachment-outlet-gnjcNhFw/image-green.log`。
- 正文首轮选择的根因和五问见 BUG234。B 即使收到原字引用，也会在 Schema 或 B/C 宿主格内限制处拒绝它。正在建立逐行位置证明，之后按可选正文重新计算容量，验证跨页续句和格内邻项。

## 新任务原字版本准入与旧正文兼容（本段优先）

- 本次BUG232/233修改尚未部署；生产CAS、运行数据库、`.env`与四份原件未写。真实成品验收仍为 **0/4**，本轮无新模型请求、无新Word。
- BUG232：新`pdf_literal`范围继续参与来源索引，退出旧Markdown/单行自动正文判断。缺显式来源的旧正文不获自动完整性；不影响已绑定正文或旧生成ATX路径。先红后绿，相关79项通过，外置`2026-11-17-source-contract-realignment/literal-legacy-regression-DHtH0qHP/pytest.log`。
- BUG233：无显式前稿的新任务复用PDF分析冻结时核原件身份与当前原字profile；已锁定的历史前稿仍沿用原冻结。隔离PG流程50项通过，随后新增前稿绑定断言，版本准入文件4项通过（范围有重叠，不相加）；外置`freeze-admission-green-St91ePyw/pytest.log`及`freeze-admission-pinned-6f8Fz1kc/pytest.log`。
- 四真实来源只读核：旧v36四份原件身份有效、均须为无前稿新任务刷新；新冻结四份通过原字profile准入，402页/253715原字符重载，111和222共5个原`#`未变。外置`legacy-four-source-admission-audit-nH1SLPIL/result.json`及`four-source-admission-audit-v7TvlYIf/result.json`。
- 下一处主断点：默认执行仍用旧layout生成目录与要求，再配置末端R；实验Joint B/C虽然提供了literal atoms，但目前只允许grid进入正文，并按grid估合法输出，不能仅删除`require_grid_body()`。后续应让首次选择在完整原字段与已证格内选取正文、上下文及连接，明确同页唯一认领、重复格字证据和超界待核；原字首行只能作定位。仍需覆盖196个图像位置、111同一DOCX的两别名及独立成品核对。

## 方案已获确认，正在实施原件来源纠偏（本段优先，2026-11-18 UTC）

- 用户后继要求“先提交当前所有改动的代码”，本轮先保存整个工作区阶段改动，随后继续原四源目标。Python最新稳定全量回归为3831 passed，530个源码/Prompt文件无漂移，证据`unit-gate-20261118T015611Z-c2a534b8/`。此前外置pytest启动脚本缺少multiprocessing主入口保护导致子进程重入的尝试已停止、保留且不算通过；最终通过用的是已加主入口保护的脚本。
- 提交前前端`npm run build`与80项测试通过。Rust版式响应deadline边界已按BUG231作最小修复，六项相关非环境用例通过、两项既有环境用例ignored；API整个lib回归为34 passed、11 ignored，环境用例未伪称执行。变更Rust文件格式及git diff检查通过；全仓cargo fmt仍指出HEAD中未修改的`tender_routes.rs:532`既有格式问题，不修改相邻代码掩盖。
- 用户已明确答复“确认方案，开始修改与四源验证”，解除此前只读范围。继续四份无企业资料模板的完整性、一致性、正确性、高效性目标；最终成品仍0/4。新执行沿用用户选定low与当前`deploy/.env`，源env SHA仍`d595fa7e3c90729431702383d88d81952d45e5152d87510267863751f37ba4b5`，没有修改endpoint、窗口、输出帽或网络。
- 外置基线为`/opt/workspace/kb-tender-evidence/2026-11-17-source-contract-realignment/baseline-20261118T004404.856235Z/`：695个代码/配置/文档before副本及SHA；共享脏树不回滚、不暂存、不提交。BUG228记录来源纠偏及五问，BUG229记录首次原生选择到Word的绑定缺口。
- `pdf_literal.py`在通用转换前保存原字、char_index、坐标及独立符号证据；新freeze/SourceIndex逐字、位置及完整覆盖重验，新profile包含五个实际来源模块指纹。bbox仅登记`native_grid_candidate`，不吞字符。新的24项、旧兼容33项、grid六例分别已收终态，证据在上述目录`literal-extract/`与`grid-green-20261118T010831Z-1ca94f3c/`；这些范围有重叠，不能相加冒充全量测试。
- 首次原生B/C的已有正文/context选择现在可直接物化BodyBinding，不再只写purpose和issue JSON；上下文分组的continuation保留，Word消费同一绑定。生产`outline_from_compiled`传入ArtifactStore，纯编译接口仍可不带存储。相关93项回归终态通过：`native-bind-green-v3.log`；不是四份完整成品验收。
- 两个被重启取消的任务已恢复并完成，无其pytest在途：literal_source_extract完成加载校验/profile四文件；review_literal_freeze完成helper网格候选修复与新六例。不得因重复重启通知再次启动相同任务。
- 四份真实新freeze已在外置独立CAS完成，非生产CAS：`four-source-freeze-20261118T011840Z-0c7f5c3e/result.json`。402页/253715原字符通过索引重载及全覆盖重验；原件SHA均未变。源冻结仍partial：Bidding98、安服93、111两、222三处图像为`ocr_provider_unavailable`；111另两内嵌名称未提取。这些图像/附件未决在旧v36同样存在，不能略过或当无内容。
- 已只读核111的两内嵌名称指向同一256844字节DOCX（SHA `b861ccbd313b2250f9c64bd0351d1726892043957ec3fe252c8b548c63dd55d1`）。需按同内容别名保来源、只解析一次；不能把DOCX逻辑位置假装PDF物理页，也不能未核适用关系就覆盖PDF。
- 已用有主入口保护的独立脚本读取该真实DOCX：311个section、17张原生表、2个图像、631个逻辑来源range，证据`embedded-docx-20261118T015919Z-a2fd3779/result.json`。其另一个OLE部件是XML明示的`Equation.3`内嵌公式，不能误当缺少一份独立招标文件；相关原式/来源关系仍待正式接线及核验。未加主入口保护时产生的多个外置探测目录不作为一次解析的有效证据。
- 尚未完成：新literal正文的首次语义选择及完整原件独立核验、原式/文表混排、真实特殊符号显示、局部待核的实际核定入口、全文件预算、新镜像部署与四份最终Word/PDF/分册验收。旧R只保兼容方向未完全接线，真实`#`及物理换行不能进入旧ATX/单行exact捷径；当前不得把新来源层局部绿部署成全流程已收敛。
- 重启后运行态改变：2026-11-18约01:25 UTC只读核，旧docreader退出143、Redis/MinIO退出0、旧worker/dispatcher不健康。root仅在确认Postgres已退出后启动独立`kb_isolated_postgres`，随后它与旧API健康；未启动新模型任务、未部署候选，旧Redis/MinIO/docreader尚未恢复。后继先查实际状态与队列/租约，不重复未知动作；不用旧v36健康记录当实时状态。

## 用户最新明确选择：本轮low

用户已明确答复“本轮使用 low（推荐）”，覆盖下文“仅root可撤回默认”的后继运行含义；旧v36状态/证据的user_selected_option=false是历史事实，不回写。新决策记录`performance-default-v35/user-selected-low-20261117T225203Z.json`，后继v37引用此记录并标user_selected_option=true。root于22:52:03 UTC只读查当前独立环境：effective reasoning_effort=low/context131072，四个v36任务均succeeded、无live lease，reserved/sent请求0；无需停止任何当前请求，也未重新发模型。`deploy/.env` SHA仍d595fa7e…，不改源env、endpoint/窗口/输出帽/网络或质量验收标准。继续原四源完整性/一致性/正确性/高效性目标，不能因用户答复替换为单独配置任务。

## 新事实已追加独立CAS：47 blob，禁止重复apply

root已完成最终70facts的最小追加导入。新计划`shared-native-body-authority/remaining-source-decorations/additive-import-root-v2/import-plan.json` SHA`bd400d86a55e5402a72056d2d8f9f6b4039e9b7fdf0cbc183dede8dcdb587906`；实际`apply-audit.json` passed：47新blob/264127字节、47已有逐字一致，94最终manifest可达依赖全重核，零覆盖。`apply-start.json`和逐节点fsync journal已存在，不重复apply；如恢复先读audit。目标卷已验证只属于kb-isolated docreader挂载的`kb-isolated_python-tender-artifacts`。最新manifest为Bidding10/安服51/111六/222三，引用见apply-audit.manifest_refs。

旧计划ready=false/旧阻断和所有旧事实/IDs/supersedes/p82审批原样保持；新计划以BUG219完整退役审计及新49回执/13退役联合证据关闭旧阻断，不把纯装饰当企业职责恢复。仅CAS追加，四份state和源env SHA前后相同；未写DB、未启动模型/新execution、未部署或生成正式Word，业务验收仍0/4。下文“CAS仍未导入/本轮无CAS写”均是此前历史，不作为当前状态。

## 后继修复进行中：BUG223/225/226/227，尚未冻结或部署

- v3原生文本范围扩域已实现并验证，原13个完整范围（含一项既有装饰投影）可直绑，300个唯一文本范围候选交同R选择；17个exact bindings实际内存bind/load成功，12条已核context保全。完整Prompt/Schema/2304纠正空间/输出双预算9包（2/2/4/1），不是业务通过。完整材料`shared-native-body-authority/bug223-native-prose-v1/`。
- 最新51/6装饰事实与BUG217/219/222编译/投影组合已固定零模型重放四源49接受/60lookup，公共候选授权差异与完整原行/原格/主体/13退役全审。规范入口`shared-native-body-authority/approved-facts-current-compiled-v1/SCOPE-INPUTS.json`，聚合`diagnostic-v2/four-source-current-compiled.private.json`约29.6MB；最终863target/7134原行/147grid/70facts。不要重跑已完成组合或读取旧摘要代替完整compiled。
- 新组合初始v3 pending121（45/44/18/14），原生R657、17exact；其中11个旧orphan审计与4个mixed footer问题已精确关闭。仅为修前来源域分母，后继BUG225已改变。
- BUG225独审发现完整文本范围扩大可重开其它target已核页眉。已修全局原件/页/字符hash守卫、唯一layout/PDF原字符映射及严格prefix/suffix裁片，原Index/原行不改；118相关测试通过、3真实Bidding prefix[0,30)新源码回放通过。5边缘clip/264disjoint/46无法证明（315候选），新pending165=64/65/21/15，R613=grid357+prose256；17exact和9包仍全部机械准入。报告`shared-native-body-authority/bug225-global-native-decorations-v1/REPORT.md`及`final-v3-snapshot/`，保留修前红；不把新增46说成业务字丢失。
- BUG227现已核46根因：DocReader在unit.text冻结了结构ATX `#`语法（source parser指纹一致），不是PDF字面原文。46个target/32range/17页，#仅2–6字符起始语法、标题与unit/contract/tree一致、原PDF各页#字形0；引用旧Index精确suffix全46可读且layout唯一，45也PDF流唯一。原Index/旧49请求无须重建。必须保suffix起点边界false、不回补到context、不全局删#。独审`bug223-native-prose-v1/native-text-authority-review/REPORT.md`；core将在BUG226后实现窄投影，尚未源修/在线验证。
- BUG226正在改v4：文本range可能跨下一独立事项/银行保函，不能自动把全部未选余文当context。prose新增显式context三元组最多2/岛，body/context不交叠，旧native anchor非空字符须被两角色覆盖；grid规则保持，未选新余文不自动入Word，原源仍完整。root已更新专用Prompt及新PG fixture的anchor/context字段；core持selection/scope/prose和专测，子代理只写test_native_body_v4_anchor.py。尚未混合或多range扩域；完整请求/JSON输出界须重测。v37门禁未冻结文件已对齐v4，不保留v3发布断言。
- 2项真实独立PG诊断通过（v3生产帧，合成scope、真实pipeline/flow/executor/SQL、mock HTTP）：lost-ack后重试复用R接受结果、原成功请求行不变，body receipt先于outline且幂等；source pending保存诊断而failed、无outline发布。2passed/0skip/无source drift、专属容器和临时凭据清理。新模块`tests/acceptance/test_native_body_execution.py`，最终PG要`--additional-pg-file`纳入。报告`bug220-native-body-persistence-v1/REPORT.md`；后续v4 fixture已变，不能拿v3结果当v4全PG通过。首tmpfs检查夹具0tests失败保留，v2按HostConfig.Tmpfs真实形态核验后通过。
- 121封存未决研究已完成，根目录`shared-native-body-authority/remaining-layout-domains-v1/`，权威`physical-order-v3/originalchars-physical-v3/native-absence-substudy`：59候选全域装饰可证、16确认交集、18未证、6跨页多义/不足、7同页多range不足、7短句多义、8mixed。93唯一range域仅定位候选，不自动正文；326nongrid ranges物理bbox全空，rawordinal观察同向不能当物理证明。8mixed的48行/14块原PDF锚唯一，14转换旧片段物理证据=9上方/1下方/4跨页，不能据此替扩展整range排序。将来扩多range/mixed先核真实范围/次序/尾句，不猜join。
- v37私有candidate已由root准备（不是部署）：`deploy-final/candidate-review.json`，SHA`55b2b3ed6a06d1834c5cd76b798abc47be740a2be8b1808c61003aa9be0cc48d`；仅4个Python image改为独立v37标签、13服务roundtrip/源env保持、user_selected_option=true。不要重复prepare_candidate。script-review/source-stop授权/selection/freeze尚未创建，未collect正式门禁/build/deploy；仍v36 image a59…运行。后继代码收口并停写后依`deployment-v37/PLAN.md`准备新完整freeze，含全部旧3611 unit/142 PG身份及新用例。临时证据容器会结束，部署前若保护基线有合法临时资源消失，须具名审查记录，不能盲重放或抹掉原拒绝。

当前无付费模型请求、无生产CAS/DB/state写；goal active、四份最终验收0/4。当前active工作为core的BUG226→BUG227实现；所有只读研究和联合回放已完成。root不要因用户low答复或单元/PG诊断通过结束原四源目标。

## 最新恢复进展：3740单元全过，R正式接线候选已写，v36运行/最终验收仍0/4

本段覆盖下方在途描述。未部署、未新发付费模型、未写生产CAS/DB/state，旧四个v36 execution均已终态，不重复启动。当前源码已继续变化，旧v36 freeze及本段3740测试快照都不能直接当新最终发布快照。

- 完整单元诊断：`recovery-full-unit-v37/`，3740 passed/0skip，327.29秒；517个docreader源码文件前后无drift。此为诊断不是正式release gate。之后root已补固定大纲预览不启R、native物理页优先读取physical_locator、结构提示不再发送宿主已知body_ref；64相关回归通过。后继BUG223尚在实现，须新冻结全量/真PG/镜像门禁。
- BUG217七行编译修复及BUG219纯装饰完整退役审计已完成。安服240原目标=238应答+2纯装饰退役，111285投影前=277应答+8既有退役；所有原对象/位置/完整来源/覆盖facts审计保全。安服12/11115接受回执完整零模型回放，公共候选/所有原请求字节保持。装饰51/6facts的47新/47已有CAS计划仍仅准备、未导入，旧ready=false历史不改。
- BUG218正文强绑定已接发布、写作、结构、Word消费者：`source_body_ref`绑定当前AuthoringInput/目标/精确来源/上下文/连接，缺源/改purpose/换版本均拒绝；共享projection每次消费只读一次。四源8个真实反例（Bidding2/1113/2222/安服1）、48篡改探针及8真实_load_task角色检查全部通过，四份诊断Word/ZIP；严格属于其冻结10模块快照，不是正式R或交付。报告`bug218-native-body-context/real-four-source-bindings-v1/REPORT.md`，后继两文件diff已具名封存。
- BUG220有限R核心/全purpose scope/runner/pipeline候选已写：新自动原件outline execution在spec固定directory_native_body_contract；R沿既有directory_parameter持久执行器、专用operation/Prompt/完整动态Schema，原P请求不变。scope枚举全部865个v36 final target；精确native/context分离，global receipt绑定最终outline_ref且先于on_outline保存，pending抛SourceBodyResolutionPending保原源、不能发布技术成功。仅完整native/existing显式来源选择可绑，不让模型抄原字。当前未实发R或部署。
- BUG221已修：R错误用受控detail和空feedback_fragments，不将整包坏回包复制入纠正，原回包仍完整持久保留；旧ModelContractError缺省None行为保持。分包额外核initial_input+2304纠正保守上界，不改窗口/预留/输出硬帽。63相关回归全过；独审实际_DocumentModel/_OutlineModel的12组离线wire均fit，原8192边界案例转具名manual pending且来源不裁。证据`shared-native-body-authority/r-flow-review-v1/bug221-recheck/REPORT.md`。
- BUG222三处真实标题fallback加已有generated标记（compile SHA`0b3c3c28…`），69个原表标题来源缺口精确关闭。四源immutable v36+唯一compile覆层回放13/12/15/9共49接受回执、所有lookup/digest/full body逐字对照，四公开候选仍同v36，模型0。新scope pending126/165/87/74，nativegrid R域51/59/182/65；该scope不是最终原生prose扩域。报告`shared-native-body-authority/native-body-scope-v1/bug222-generated-template-labels/implementation-v1/REPORT.md`。
- BUG223进行中：PDF line_units只登记grid，426个layout_pending不等于无native正文。独立nongrid审查最窄12个完整native range逐字相同可直绑（3/2/2/5）；复用已有批准装饰投影另222一例可成为第13，需逐对象证明。135个substring只有定位证据，其中61后面仍有原字，不可直接当完整正文；另仅空白差异167保原文。正扩同一R的native_prose类型（无伪表格几何），给完整唯一同页原生range让模型选句段/补尾句，跨页/混型/重复仍保pending，合同将升v3，包数必须用正式完整Prompt+2304纠正界重测。`implement_r_finite_core`当前只写scope/selection/prose helper及聚焦测试；root持有flow/pipeline/spec/Prompt，避免冲突。
- 111唯一legacy excerpt mismatch已定位：chapter-10:target-5是两格66+42字的明确续接；原body_bound事实`reviewed-fact-e89ed390610bd7ffe99f97e00cfc0da5c2276d4b4f946d243b9820bf79359958`已存source_join=continuation及exact refs/hash/context。scope将只在回执全部严格一致时复用，不按去空白相似猜连接。BUG218已补。
- Bidding p59L6签章续行whole-word唯一exact-cover已证35行/50word/676glyph一次恰覆盖。258点注册grid外几何均已核，但不等于prose或可删除；未注册续句和三个target身份仍保全。同页native/layout缺物理顺序证明时不授权续接，当前prose薄片不吞mixed域。
- `review_r_flow`另在准备仓库外`deployment-v37/`最终unit/真隔离PG/镜像审计/独立部署脚本：仅准备，尚未freeze/collect/build/deploy，root以后显式冻结才运行。不要沿用旧v36 runner的旧freeze。当前无shell门禁在途（30561/75888/9169等已收终态）；只有上述BUG223实现和v37准备agent仍在工作。

实际源env在`deploy/.env`，不是仓库根.env，SHA仍`d595fa7e3c90729431702383d88d81952d45e5152d87510267863751f37ba4b5`；不打印值、不改。low仍为root已有可撤回验收默认，不是用户显式选择；131072/32768/4096和60秒raw-idle保持。最终35项原件/正式Word/ZIP/分册/同版PDF及独立语义验收标准保持，当前仍0/4，goal active。

## 恢复后新增：BUG217/218/219/220局部修复中，仍运行v36，验收0/4

本段覆盖下方“所有源码保持冻结/两个准备任务在途”的旧状态。v36四任务已终态，不重复启动；本轮没有新模型发送、生产CAS/DB/state写入或部署。宿主源码已有后续增量，不能再沿用v36 freeze部署。

- BUG217七行编译修正已完成，71相关测试通过；真实安服零模型回放12/12接受回执及全部请求轨迹保持，仅两处purpose排除p71已核页脚，完整原行/16原表保持。外置`bug217-parameter-presentation/`及`online-four-projects-v36/anfu/page071-footer-fact-v3-bug217/`。
- BUG218公共正文/context承载第一片86相关测试通过；随后复现6项真实行为红：篡改purpose、破坏正文selector、换frozen来源时，Word/结构仍接受。现补`source_body_ref`不可变宿主组合绑定与消费前逐字重物化，支持native和未注册续表冻结layout来源，29项聚焦已通过，仍在审查/回归，未接正式R。
- 装饰packet准备委派`b084b15d-84ff-42d5-9aa3-ffe6cd849e52`已完成。安服51facts/111六facts完整admit、12/15接受回执零模型回放；47新/47已有CAS节点，仅准备未导入。报告`shared-native-body-authority/remaining-source-decorations/final-packets-v1/preparation-20261117T213313Z/REPORT.md`。原守卫因安服两个target退役保红；root复核二者仅页码/编号且无任何职责引用，正确出口为纯装饰退役并完整审计，不能恢复伪填写位。BUG219正做严格职责/双向关联保护和完整旧target/位置回执；旧红历史保持。
- 全四源追加native到原A/M路线已被真实容量反证。R只做共同最终来源字段的有限选择，调用公式`N_base+R(U)`，旧U子集51/61/182/65及六包实验不是全部范围或最终上界。BUG220独立有限选择/完整I-O双预算纯函数正落实，尚未接Prompt/生产；原流程、旧额度和完整原回包保持。
- Bidding p59L6“公章）”已补整页唯一exact-cover：35行/50whole words/676glyph恰覆盖一次，唯一对应C0，属于未注册续表的grid外续句，必须保留。旧未决证据不改；补证`shared-native-body-authority/unowned-source-geometry/p59-whole-word-exact-cover-20261117T220038Z/REPORT.md`。因此258点几何已全部核为registered grid外；不等于prose/可删除，也不证明职责覆盖闭合。
- 111原table0是采购技术续表，public kind=form来自parameter_parent适配，不能据此新添必交原格式；独立未知样本鉴定尾句遗漏及跨列仍真实红。131未完成整行绑定/395重复匹配装饰候选均保留，不批准删除。

下一步：完成当前小片独审/回归及冻结真实回放，闭合R全部来源域与可表达边界→正式有界选择/持久回执→新冻结门禁/独立部署→按四原件逐份正式Word/ZIP/分册/同版PDF验收。最终仍0/4，goal active，不调用create/complete/paused。

## 最新：v36四份真实候选均已终态，源审未过，最终验收0/4

本段覆盖下方所有在途记录。最核心根因已由四原件实证：A正文/clause、M材料、B要求、P子项共用layout阅读行字符串作为正文权威；行号/Schema/缓存匹配不证明正确列、完整格和完整独立条目。原生cell内容可得且正确，错误在选择和物化。已核CPU/96字等少数native facts修复不覆盖普通来源出口。未采用四份红候选、未生成本轮新Word，旧revision和旧Word实物保持。

- 当前独立环境仍v36：Python image `sha256:a59a39d8e7bffb1b0fe5eca9870d21cfaeb20ab01b51c746bb306a946cfe5b34`，API `sha256:54ed8dd17da94540fc045febc40892a23d8fe706cd14d96002bd38e77ff42f14`。source freeze `f0e2c7d3789ead7a5af144c433ad0357c809371aa915e160f9ecc78b00a0a8cd`：510文件、3611唯一unit/142真隔离PG全绿、零skip，旧3515/140身份保全，507镜像源码/依赖/Poppler22.12一致。真实部署attempt `deployment-v36/deploy-final/attempt-20261117T200314Z-612b0438/`及canonical summary保留，不重复deploy。
- 源`.env` SHA `d595fa7e3c90729431702383d88d81952d45e5152d87510267863751f37ba4b5`未改；low是root可撤回的验收默认，记录`performance-default-v35/operator-default-decision-v36.json`，不是用户答复或源env设置，后续用户显式偏好优先。60秒仍是连续无原始数据空闲超时，不是总时限，不加输出硬帽。
- 四份v36均已terminal succeeded，但全部technical_success_source_review_failed，无在途模型/lease，禁止重复start或换key刷新额度。Bidding `046accb4…`、安服 `ccf6695c…`、111 `e4892248…`、222 `efd7dfc4…`，完整ID见各state。8A实际精确复用v34；完整v35事实10/3/3/9及supersedes保全。所有原始请求/回包/失败费用/未知usage均保留。
- Bidding：620.2秒，11实发/496494已知tokens/未知0，复用2A+1P。3卷/27原表/财务双主体原表/3.5.1与3.5.5独立正文/报价顺序均在。硬红：继承P的地址转换正文夹“规格/配置/规格”、HTTP正文夹邻列“护”；不是本轮模型重写，回放延续旧source物化缺陷。
- 安服：695.3秒，11实发/482113已知tokens/1笔provider api_error用量未知，复用2A。2卷/16原表/5服务表/28服务及无关联/投标书13→14→15/资格脚注/团队条件均在。硬红：普通B团队要求夹入评分列“13/项目经理要求/1”；P子章及父方案还含原p71L58页脚71，bbox已证。当前页脚fact仅p17/p30，未覆盖p71，不能误称已核fact路由失效。5服务表重复表头仍须最终实物核。
- 111：850秒，11实发/466355已知/未知0，复用2A+3P。32原条/B05实际入表、CPU native/context、财务双独立空位与同时约束双方共同说明、基本表单份均具备；F另第33行是原件要求的价格评审程序，不能按33≠32假报。硬红：普通A/B/P跨列，且独立“未知样本代理鉴定”缺native格末句“件进行鉴定。”；原A/P包络截掉L4，B在别处出现不抵该独立响应完整性。q8仅form/attachment类型分歧，不是缺企业资料或两份材料，最终财务共同说明可合法共用，不机械复制463字。
- 222：690秒，10实发/299912已知/2笔未知，复用2A。双签单份、96字native在chapter-15:target-6、本次实际章名“集成建设方案”、8原表/15组成及售后均具备。硬红：普通chapter15/16/17正文夹评审标签与分值，已追溯到两A合并的clause_windows→obligations；本轮无P，不能只修P。
- 四份完整报告在`online-four-projects-v36/<key>/stage-final-source-review.json`及`TERMINAL-REPORT.md`或`HANDOFF.md`。共性新问题已记BUG216，脚注/财务诊断补记BUG206/205。全部模型/runner已终态，后继先核事实，不等待旧在途任务。

下一片先验证统一原生正文方案，再写代码：独立只读方案与五问在`shared-native-body-authority/independent-reviewed-entry-agent/proposal.md`，现有SourceIndex/excerpt已能准确物化明确选择的cell/span，但不能从旧页/行/unit自动猜正文列或一律扩整格。需要正文/context/部分格与续句的明确有界选择，保原表/merge/空位，不按关键词、最长列、纯数字删字，不建全文atom/ledger。真实最大A/M及完整纠正容量尚未验证，不能沿用单P样本估计。

目前仅两个外置准备任务：`root_cause_explanation_check`只读测四源native选择方案真实容量/输出上界（`shared-native-body-authority/capacity-feasibility/`）；`v36_runtime_preparation`仅准备安服p71已核页脚的新事实与零模型回放/additive计划，不导入、不改源码、不启执行。所有源码保持冻结；下一轮实现前先收方案反例/预算结果。goal仍active，0/4，不调用complete/paused/create_goal。

部署运行保护基线曾因一个无关临时容器消失而只读拒绝；原快照/拒绝及root刷新记录保留，后续“protected未变”指刷新后基线，见`deployment-v36/deploy-final/runtime-baseline-refresh-root.json`。

证据根：`/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/pending-closure-v1/`。最终35项独立清单/Word/ZIP/正式layout回执同版PDF验收口径继续保持，不用机械绿替业务绿。

## 当前恢复核查：v35最终门禁已启动，运行环境仍v34，最终验收0/4

用户追问根因后的独立复核确认：原方案缺少“原件应交付职责→合法且足量的承载→最终实物”闭合；旧F把原文入表与应答责任合成一维；完整阅读源转正式purpose时丢失呈现映射；链接/出件及局部测试不足以证明最终语义与版式。全atom/native替代路线的规模假设已被真实73/55包与合法超限输出反证，继续撤默认，不另建同类框架。

- 安服受限页眉审批后仍未决已定位并修复：旧obligations逐窗strip，私有rows却仅平铺。现在只在该既有路径保私有render_start/render_end/rendered；完整原行、坐标、窗口、模型文本不变，按真实切片校验purpose后排除已核片段。有效红回归1失败/3通过；修复后含条款/完整行/非法切片保护42/42通过。最初缺role的夹具红不是行为红证据，后续有效红另存。
- 新独立只读复核499文件前后全同；50完整请求摘要及原接受标志不变，47有精确成功回执，安服旧C/2F的3项仍未接受；私有切片未进wire，12旧业务事实ID不变。真实安服11行切片重建279字purpose，raw flat仍312字，差33全部为窗口边界空白。证据`source-hygiene-design/implementation/render-slices-recheck-20261117T170138Z/`。不将其当完整页眉投影或业务完成。
- 当前目录布局核心、Python HTTP/export/executor及Rust受限renderer均已接候选代码。最新Bidding89项目录真实固定点回放2次转换通过；Rust6纯用例及两项真隔离PG/HTTP（含真实OnlyOffice）通过，专用资源清理。root已补PDF实际显示/目的坐标区域、完整HTTP绝对期限、响应body配额/期限及layout回执宕机恢复，原独审红与修复证据保留。
- API最终镜像`kb-isolated:runtime-render-v35-final`已构建，独立审计17/17；image54ed8dd1…，167源码冻结匹配，原7层/其他二进制/UI保持、仅新增API二进制，Bookworm链接正常。未部署，禁止误用资源修复前旧候选`runtime-render-v35`。证据`deployment-v35/api-final-audit/`。
- Python最终门禁新冻结503文件，保全部旧3311单元身份；实收3515 unit、140真隔离PG（含layout提交后失租两分支）。当前已启动各一次，不能重复；门禁`deployment-v35/python-final-gates-v1/`，PG专属plan在其persistence-plan.json，freeze SHA`4fdd9d31427fbb46b12e276a436f33fe02fa59da227abaf02784124f686e4e64`。仍须收结果、按身份清理专属PG，然后build/image审计。
- 完整装饰审批包与overlay仍外置，入口`complete-source-decorations/approved-manifest-index.json`；四源当前新代码回放在`final-window-slices-v1/`执行，不复写旧130项红。58物理片段及25事实只是已审分母，不能冒充四原件全部语义完整。

恢复补充（以下覆盖上方在途描述）：

- v1已终态：unit3515中3508通过/7失败/0skip，503源码全稳；真PG140/140通过并按ID清理。7失败均是新增私有来源输出与旧整体相等断言冲突，独立归因后仅更新3指定测试，保全部旧状态深比较并精确核新增映射；33focused全过，11坏映射/旧状态负控均被拒。源码生产部分未改，原红完整保留，不能将v1称全绿。
- 新v2冻结503文件，3515测试身份与v1完全相同，仅3测试文件变更；freeze SHA`1553222c722320a583bb167ac74223a86f008902b037a0eddfd940b58a063f34`，`deployment-v35/python-final-gates-v2/`。已各启动一次unit52349、真PG35995；build44202已exit0，image`sha256:251adbe0e0e28000188829a79e0e2ec0cd32259893c26af72897446eed1fcdd0`；image审计91156已500/500逐字节通过、parser版本保持，不重跑已终态build/audit。
- 四源完整装饰/Word反事实在当前生产源码上198/198通过，58/58精确片段排除、64原表与全部团队条件/签章保全。743映射中737为逐字来源、6为已有宿主整体响应且rows为空。最终四manifest refs未变，见`complete-source-decorations/final-window-slices-v1/IMPORT-HANDOFF.md`。
- root已审实际隔离挂载并追加导入CAS：最终计划SHA`75f42cb10fb40fca55bfe43fcf8a0b5135069f2e4d05517174efba6d7f9588f7`，23新增blob/434855字节、24已有一致，零覆盖/DB/模型。audit`apply-audit-3d8a283f74bb4e9983c14d8f00d6ec08.json`在该目录的`additive-cas-plan-v1/`。此前旧计划SHA变更在首个只读SHA检查即拒、未发生写入；最终计划封稳后才成功导入，不能重复apply或沿用旧e4fcc88…SHA。
- `deployment-v35/deploy-final/`最新.env私有Compose/部署脚本已独立准备，16只读反例通过，尚未执行部署。仅接受最终v2全部门禁，保旧人工稿与受保护容器/隔离连接；按最新.env的effort空值，去除旧诊断low覆盖，不能承诺50旧请求全部可复用。部署summary在新attempt目录，root需成功后写规范根summary供v35 runner身份门禁。
- `online-four-projects-v35/{bidding,111,222,anfu}/state.json`已只读准备，新request_keys空，无新execution/revision/adopt_input；完整manifest输入/当前draft/本机及API原件/当前机器Word字节一致，企业资料空、分析未确认、无编辑/待保存。outline前驱用各最新v34（安服cancelled），document前驱前三v34/安服v33。新`run_four_projects_v35.py`会传真实reviewed_source_facts_ref，`select_v35_generated.py`按实际新execution显式选择。尚未发任何v35模型任务。

旧API build46657、context负例61572、BiddingTOC36181、v1unit81424/PG98467均已收终态，禁止重启。当前运行仍v34，业务验收0/4；下一步收v2两项门禁、清PG→显式check/deploy→规范部署summary→新四源真实流程→实际新Word/PDF/分册逐原件验收。

## 最新接线：v35候选源码已冻结，尚未部署，最终验收仍0/4

本轮继续完成BUG205/207/208的最小生产接线，当前运行环境仍为下方v34；未新发付费模型请求、未导入事实包到生产CAS、未改旧人工稿。所有v34任务仍已终态，不重复提交。

- M加入`source_carriers_closed_v1`候选原表差集处置；已核原表不能普通exclude，物理载体存在仍不代表主体或语义完整。
- F加入`source_bound_response_forms_v3`，拆开原文入表与criterion/reference/procedure责任，企业列继续空白；导出指南复验最终target/row绑定。
- 新`reviewed_source_facts_v1`固定完整输入、来源投影与审计证据，支持原表、主体、分册和独立正文事实；前驱继承及更正需保留或有新证据supersedes。只有独立正文事实启用新C；form章可承载同entry/subjects的独立narrative目标，原式不变。
- 独立审发现并已补两个接线缺口：最终事实应用记录曾只在内存，现单独持久化绑定实际outline的私有回执，保存失败不发布；同来源/主体/entry的重复正文事实可改label绕过，现以规范源覆盖与role/entry拒重复，保不同真实实例。最终窄独审无剩余必修项，不等于四源语义全部通过。
- root固定快照聚焦单元160/160；代理相关回归234/234与root用例有交集，不能相加。最终冻结源码在v34镜像只读、无网络回放84/84，含222的96字精确正文、Bidding两项资格附证职责及27原表保护、安服5表及分组反事实。完整M/C及代表纠正最大91073/94208输入；不是最坏输出证明或真实新回包。
- 首轮真隔离PG138中137通过，唯一失败是root新增worker测试的ModelConfig夹具前置错误，已按相邻worker测试补显式fixture配置、保全部断言；旧红证据和专属资源清理回执保留。最终稳定快照真隔离PG138/138通过、零skip/failure/error，492文件运行前后SHA一致，零真实模型调用、未用项目数据库。最终结果路径`root-integration/persistence-v35-46bea3714ea84163/run/result.json`，该轮已终态，不重复启动。

主要证据：`reviewed-source-facts-v1/REPORT.md`、`source-freeze-final.json`、`final-v5/full-probe-results.json`、`reviewed-facts-narrow-review/CANONICAL-FINAL-REVIEW.md`、`final-reviewed-fact-receipts/integrated-final.xml`。module SHA为`7750851163c955bfbb56da19789393b5989febdfe8e68a584249ff4fbcdfbbd9`；PG完整492文件冻结SHA为`0ba0c4a8f11d1bb0a6b28417d914ac1811a0ca6e81234e999847d6a5851c7634`。

事实包仍全部外置，见`source-closure-next-slice/reviewed-fact-packets/HANDOFF.md`：安服v3七项、222v3两项、111v2一项及Bidding v4两项草稿。Bidding新guard/实物反事实已过，但当前索引仍标draft，待root最终准入；111必须使用v34固定Poppler投影，不能重签宿主漂移投影。

后继仍需完成最终Word目录缓存正式转换接线、源页眉页码/普通章节native污染修复，随后完整unit/真PG/build/镜像与最新.env门禁、独立部署、受影响真实请求与最终Word/PDF/分册验收。TOC外置89项固定点证明不是产品接线；4份仍0/4，不能因本段任何局部绿项授予通过。

源页眉页码至少一条确定路径已查清：Bidding财务`chapter-2.5.4:target-3/-9`及报价`chapter-4.2:target-3`来自两A显式`body_windows`的非grid源片段；`directory_compile._body`显式分支全拷，绕过另一旧范围分支已有装饰过滤，后经manual source purpose原样写Word。不是这些原生表格内部损坏。尚未修；不能把`line_units=null`或纯数字直接等同页装饰，需物理源证据保护参数、分值、表单字段和签章。

该只读独审已完成停止，报告`source-decoration-root-cause/REPORT.md`及`source-to-word-proof.json`：三样本逐字链与原件bbox均已核，页码y45.15–51.30/页眉y772.36–780.82/页高841.95；三负控证明纯数字正文80、原表固定数量80、混合页眉与公章/签字/日期的section均不可整行按数字或整unit盲删。最终三个form target没有source refs/excerpts/template refs，混合段落的行坐标在purpose物化时丢失；既有整部件装饰排除出口不适用。后继需精确来源角色/片段核定及物化前保源应用，不把这一A窗口根因推广到全部20组或111/安服的其他污染。若新增角色写入documents会改变SourceRefs digest及事实包/请求，不得假称零重跑；独立位置排除记录且阅读源不变时才可精确复用旧模型回执。

## 当前：v34三份新Word仍红、安服无效分支已取消，最终模板验收0/4

### 最新实物结论（v34已收口，仍0/4，先看本段）

四份v34均已终态，**没有需要继续等待/重复提交的v34模型任务**：Bidding/111/222均已
adopt→document→显式select→下载及源审；安服因新M仍同错，仅支持cancel了本次outline，
未adopt/未document。不得按下方较早“在跑”文字重复动作。完整报告分别在
`online-four-projects-v34/<key>/two-delivery-review/`。

| 文件 | 当前新机器稿 | 确认绿 / 未通过 |
| --- | --- | --- |
| Bidding | version cd1aed91-b40e-4f0c-b7e4-be95d6a61d42，Word SHA81e652ba… | 3卷、27原格、分册/总稿target区一致、严格企业空位及12负控绿；同版85页PDF的89目录缓存仍待计算、20组源页眉/页码红；24旧target的长行差异待原件职责复核 |
| 111 | version a64ad987-0c83-483e-b112-1a24764949b9，Word SHA75e61b1d… | 强制原四列表已实际31行且31/32正文/上下文精确、企业空格/省略行绿；B05程序条款只在guidance未入表（BUG207）；普通章节旧native污染未关；财务分别主体空位回退，原句仍在；基本表单份绿 |
| 222 | version61bbef71-5b01-4fd7-b257-66bfeb4ec670，Word SHA076c13c6… | 8原表/严格企业空位及12负控绿；双签原函仍2份；已在v30正文的保密方案退尾部待核；售后承诺实在chapter-18:target-1，旧标题检查误报保留纠正 |
| 安服 | 仍v33 version6fba270e… / SHA2f22df78… | v34新M仍q11=current仅说明1行、assignments=[]、source_groups=[]；9次发送后支持cancel，已知332567tokens且1未知，旧稿/元数据不变；未作无效新Word |

本轮Bidding12次/549998tokens、11116次/663083、22211次/421853，各完整费用/继承/来源见
实际报告，不编币种金额。新Word DB/CAS/API/ZIP绑定均通过；Bidding仅view转换PDF未
执行Office保存，111/222尚未Office/PDF视觉，不能以出件给最终通过。

新方案评审已明确：全量native候选73/55包及C超限，稀疏168行外置原型14项机械保护
及CPU两B→明确C/Word同源绿，但其合法最大fanout实际49k/45k/73k输出仍超32768；错误
单列语义声明closed也可能机械通过。**不接默认、不调预算、不再堆同类全量框架**。
证据`bug190-bounded-alternative-review/sparse-prototype/REPORT.md`。

root已汇总方案根因到`root-cause-and-closure-review.md`：缺原件交付事实/候选载体差集闭合、
缺已核定事实修订保护、双A共漏、宿主错误位置代理及最终Word/PDF门禁不足。
source_closure_next_slice代理已完成并停止，报告`source-closure-next-slice/REPORT.md`、
`probe-results.json`最终36/36、`closure_core.py`100行仅外置；零模型/DB/repo写，未接生产。
最小下一片方案：同一M明确candidate carrier
差集处置、已核源事实复用到既有C，不强制保所有候选/不按标题猜组。特别222旧保密target
没有source_refs/excerpts，不能把历史available_targets直接当绿事实；独立源窗可从真实
SourceIndex精确绑定该行，再核既有C complete_basis出口。未有生产接线，后继必须先读
它的报告/反例再决定。注意首版主体并集被反例否证并撤回；物理table_id出现不等于财务
两主体实例完整。两种安服合法修复出口（既有alternative或assignments/source_owner）及
222用同源review fact进入既有C后恢复独立正文均仅反事实，不是真实模型成功回执。
本轮所有代理均已完成，无在途模型任务/后台控制进程需等，不要重启旧任务。
F角色BUG207、Word目录/源装饰BUG206均先记bug，尚待新实现；最小生产接线、明确五问/
新合同冻结/真正受影响请求复验尚待继续，绝不标goal complete。


以下是较早的v34启动记录，终态/新Word以顶部“最新实物结论”为准。

v34已全部门禁通过并部署，四份真实outline已启动时的状态：481源码文件，冻结SHA
`2bd56d86a0160d54dafb502e12af4f97395d567ed7013e95aff65b08f4c9f811`，收集3311唯一unit、
135唯一真PG均全过、零失败/错误/跳过，478镜像文件逐SHA一致。专用PG资源已按身份清理。
tag `kb-docreader:review-20261117-source-closure-v34`，image
`sha256:39c5373cf5d34ce7071b17e9f7b94565966916870b46d979ca51dccec21df0aa`；五角色healthy、
init 0，最新.env SHA仍d595fa7e…、受保护容器/旧人工Word字节不变。部署证据`deployment-v34/`。
四份state已准备最新实际rev/SHA/机器Word作前驱，新的request_keys，不覆盖旧执行/旧文件。
已启动任务：111 `web-outline-fa044617f2d66f88cf57a353924b9e3a4e91f6705382c8a853b46c7be860a266`；
222 `web-outline-8d8ce67dda7dc1ac0895e5682824667a6ddbbad78f6e360458841fa318d4a96c`；
Bidding `web-outline-4a345c485c752d224d357029b4ccc7acf4797e41a84bd16d7fa43669338e509e`；
安服 `web-outline-2a2418baddbb510304dbd8116478236bdbd13cbcfc28e6aee8e9b117dd3920aa`。
**不得重复outline_start**。使用`run_four_projects_v34.py`、`select_v34_generated.py`、
`audit_v34_live.py`；state在`online-four-projects-v34/<key>/state.json`。
代理v34_bidding_anfu_delivery拥有Bidding/安服的status→adopt→document_start→select→download；
v34_111_222_delivery拥有111/222同流程，root勿重复动作。初次报告Bidding/安服A各两槽接受、
M各一次sent无拒；111一A接受、另一在途，222首拒一次正在有限纠正，尚无新Word。
API当前authoring的succeeded仍可能是v33/v30前驱，必须按state当前execution判身份。
v34仅默认启用refs v12、新M来源分组/材料owner、F v2完整依据决议及精确来源分册修复。
native joint B/C候选已撤默认spec启用：完整真实容量Bidding73包/安服55包，C全未决容量亦
超94208/32768，不能当高效修复上线（BUG204）。候选/helper/124相关回归与原容量红保留，
bounded_native_design_review正在外置只读核更小方案；源码冻结期间不编辑repo。
BUG200决议完整mandate/真实target已进入全/compact/实际结构审核，独立68项过、P1关闭；
BUG201来源卷10新定点、新M组20新定点及235相关回归过，旧29合同不变。Bidding离线
反事实恢复3卷、230 target区与27原格保全，未代替线上新Word验收。BUG203已新M gate接线。


当前独立环境实际为`kb-docreader:review-20261117-native-forms-v33`，image
`sha256:a432c4b42f784d053dd569ab0b31dac1d1b6c404852cff80774d77dcac796c03`，465文件冻结SHA
`ab986955b38d1ce3f672277a62789cc41d2853054811aaffed24e6c6b7fb867c`。3170唯一unit、134唯一
真实隔离PG全部通过、零失败/错误/跳过，462镜像文件逐SHA一致；专用PG资源已清理。
最新deploy/.env仍SHA d595fa7e3c90729431702383d88d81952d45e5152d87510267863751f37ba4b5，
Grok/131072及既定私有low覆盖生效。五持续角色healthy，其他容器/原人工稿未动。

四份v33状态在外置`online-four-projects-v33/<key>/state.json`，不得重复提交已有任务：

| 文件 | 最新实际结果 | 尚未通过 |
| --- | --- | --- |
| 111 | outline c3d75983…，0个A接受、同错误6次后支持cancel，cancelled | BUG199协议修复后真实v2重跑；逐条原式扩行、BUG190/191等实物复验 |
| 222 | outline d66fac8c…，0个A接受、同错误5次后支持cancel，cancelled | BUG199协议修复后真实v2重跑；双签授权函重复等实物复验 |
| Bidding | outline b63aa991…、document b0effe4b…均succeeded；新version 3c6f7b46-3141-447e-a696-e5bbe898e5b7 / Word SHA 5544ecdda91b730f996d4ceeb5c67c4296854e85a92316b3c436df827f872d83 | 三组is_volume=false、ZIP零分册（BUG201）；27/27原格及总稿三部分/报价顺序已核绿 |
| 安服 | outline f9211338…、document 904c8e4f…均succeeded；新version 6fba270e-4eff-41d3-9945-1d6b5a8dc347 / Word SHA 2f22df780abadc5fe439ea58f62fa9c6ac2c977631ed03a506ad51867a0d3ed2 | 0分册；11/16原格、S10三片全缺；26/28独立服务位；F两表complete=false未落展开（BUG200/201/202） |

两份均经既有幂等runner采用、生成、显式选中新机器Word并下载，新旧版本DB/CAS/API
字节一致且无编辑/待保存，旧人工稿未动。冻结容器相关提取已全部完成，后继只读审计
不再依赖v33。两份完整身份/成本/证据索引分别见`two-delivery-review/delivery-summary.json`，
源与Word独审见同目录`source-word-review.json`。Bidding本轮17发送/3重试/873200tokens，
安服18发送/4重试/1005431tokens，本轮usage全已知；旧链费用/未知项另存，未编币种费用。
严格无企业资料审计分别230/208单元、165/176规范空位及12负控通过。安服原书13→14→15、
无关联承诺完整正文/签章绿；原服务检查器18/28保留，更正8项同源父标题前缀判据后26/28，
S07/S08同target仅一组空位仍红，6负控全拒。Office保存/PDF视觉/最终四源一致性尚未验。

当前源码已超过v33，**尚未冻结或部署v34**：BUG199新增v12阅读锚点operation/Schema/补充
Prompt及pipeline/spec接线，54项focused通过；旧v11 Schema保持，只有精确旧成功回执才
可无发送重验复用，不能升级旧错source_only回包。BUG190第一片native source ledger/helper
已40新+79旧回归通过，第二片B/C生产接线由native_requirement_finish_design继续，不能把
helper绿当线上已修。source_volume_restore正核BUG201来源分组物化；root负责BUG200职责
及BUG202实审跟进，勿并行编辑各自函数。所有问题先记录bug-fix，后继全门禁须新快照。

以下为较早轮次记录，版本/任务进展以本节及外置state为准。

## 较早：v31安服与v32候选记录

后继v32冻结/门禁已跑完但**未部署**：464文件冻结SHA
`70b2671a3293c9d0a738782a262b0f6b25607f26cb4dc6897368362cca6c4ca8`，
3161唯一unit为3159过/2红（均严格预算spec任务期望未加F，零错误/跳过），134唯一真PG
零失败/错误/跳过（含新增F持久化、恢复及前驱精确复用零重发），461镜像文件逐SHA通过。
专用PG容器/卷/密钥已按身份清理，source freeze各gate期间未漂移。v32镜像独立标签
`kb-docreader:review-20261117-native-forms-v32`、image `sha256:4c38966cb4ab155729d0cb36c719df71eedff72870fd67119ce367d15cf21b3f`，
不因镜像/PG通过而部署红unit。独立review另发现BUG197：native row join/context-body及
guidance join实际输出变而结构review payload同，需修。证据`deployment-v32/`（含
independent-review/review.md、source-join-structure-probe.json），BUG198为预算期望同步。
freeze已解除：root已只补预算显式F期望，34项focused全过，不削弱整体相等/计量保护；
assessment_response_design已通过followup_task重新启动，正在修BUG197，先红再绿，旧
非native视图字节不变。它完成并明确停止前不要新冻结或部署。v33 helpers已准备但
`prepare_v33.py`尚未执行；须等源码稳定后新unit/PG/build/SHA全gate，不覆盖v32原失败。

v32四项目state已准备在`online-four-projects-v32/<key>/state.json`，均**未启动任何新
outline/document**，仅固定此前真实rev/机器稿作为前驱；不要误把prepared当running。
v33尚无state。`run_four_projects_v33.py/select_v33_generated.py/audit_v33_live.py`已准备，
实际运行须v33 deployment-summary正确绑定。全部证据外置；原人工Word未动、未提交推送。

几何/P/F post-ID hook已由恢复代理独立复核收尾、停止编辑：compile SHA
`e699b7fd3c27c54090f07e8dacc7f8679445dc2046798603a32268f01e09fcde`（未纳BUG197修订），
187相关unit全过；新几何只新增6条null→owner，未撤旧合法owner。离线新Word SHA455bee4f…
父16表/28项局部应答保全，S10列碎片0、引用父target6/8/10，原三片无长文/应答夹入，仍有
真实源页码/编号，不能称已清全部装饰。新SourceRefs摘要变，真实两槽A必须新发。
BUG188完整M/F代码已168项相关回归、完整111 F输入15405/最坏结构输出14551/代表纠正
输入26804准入；都未有真实M/F成功回执，人工32行oracle不变产品规则。BUG197修订仍待。
BUG190 B-native设计已落`bug190-native-requirement-design-v1/recovery/design.md`，
新代理native_requirement_finish_design正在**仅外置**implementation-draft写下一片helper/测试，
尚未改仓库/接pipeline，不影响本轮冻结；root需在后继真实四源运行时继续此未完成项。

效率只读工具`audit_generation_efficiency.py --round vNN`已在v31实读核验，按实际请求记录
区分发送/重试/继承、区间求和/并集/生命周期及当前明确前驱链累计；usage未知保持未知，
没有币种价格就不编费用。性能结论仍须质量相同后再比较，不能拿当前未验收稿宣称高效通过。

安服v31实跑已完成：outline `web-outline-9496bb8597557fba86cd66aba621032ae10bd68a8bf6d8064558a009b7494d9b`，
新rev `03822587-3718-4b22-9f6a-de13a603caaa` / SHA `36e8522c87fd24d0bb7d3f843866ede84592c33bf6bc81ea4ea589d1f6d2c586`，
document `web-document-f2e2913eb5c395a8b829b55c0eb9d73dd2afa2031482da1bb6446224280420e3`，
version `31fb04f1-84b2-42e3-a937-eddf4fc2526c`，Word SHA
`7d063aac37c891827f04095d71e476138bc7ee00142395d08e0a3638e0fdb64c`。Word/ZIP已显式选中新
机器版本、DB/CAS/API绑定下载，旧机器稿/旧人工稿字节不变。10次新outline调用零重试
（A两槽精确继承，M1/B7/C1/P1真实发送）及1次文档结构核对；全部usage已知。
独立28服务分母以完整源窗/实际接受P/Word target书签核28/28局部应答，v29同审计0/28；
7类删除应答/证明/源字/重复应答/错误书签负控全拒，另4项确认其他全局应答仍在以防假绿。
独立复核同时确认16/16原格/空白/merge、两卷真实子树一致、固定投标书13→14→15及独立
合格性交付保留。严格无企业资料审计277单元/239空位及12类负控全通过。证据
`online-four-projects-v31/anfu/`与`source-layout-integrity-design-v1/v31-word-review/review-final.json`。
尚未通过：第三片续表仍后置、P S10重平铺原表碎片（BUG195）、两A共同漏无关联承诺使
B/C改为unassigned（BUG196）；纯源装饰target在v31为form且实际没有应答，不再沿用v29
“有应答”的旧事实。两卷初审尾比较false已保留，实为总稿4段pending附录，不是技术子树
不一致。局部28/28不能覆盖上述新缺陷或整体模板验收。

后继尚未部署源码：主体`subjects_alternative`已22新项/332相关回归及真实来源5/0，保完整
正文/原格/条件仅选择主体布局；原式native_layout扩行已24新项/156相关回归，真实111
人工oracle可生成一份四列表/32源条款且企业列空，但非模型选择/生产验收。M新variant及
F来源选择正在接入，合同`source_bound_response_forms_v1`；P原表精确引用合同
`source_bound_parameter_tables_v1`及保守bbox修复正在收尾。两合同已加入新的outline
execution spec及逐键review_options传参，旧v31 spec实查均无新字段未改；最终新真PG须
验执行恢复/CAS。A职责Prompt已通用修订，试验19项通过但另有clause/condition风险，窄化
生产版未真实重跑；后继A摘要确变须新发，不以新ID抹Bidding旧失败额度。未构建v32。

本轮两个原代理因sealed task服务错误中止后，已确认无其测试/探针进程、保留源码/证据，
新代理geometry_completion_review复审几何/P及F最终编号hook、native_requirement_finish_design
恢复BUG190只读设计；assessment_response_design仍接F。不要重复已终态两次诊断请求。

v31于2026-11-17 09:58:57Z部署到独立环境，tag `kb-docreader:review-20261117-source-response-v31`，
image `sha256:4a0a392f4e61a5c972e2fdb7d9a5f89cd05da046b26f1a3d0b8851e43e86ef15`。
454文件冻结SHA `023e1f42b1a743083b2a9d742e3abb7e3e43d56785d32b25de7d0b026b2a2f4c`；3055
唯一unit结果以“v30其余文件不变+本轮6个确切SHA+新收集3055身份完全相同”绑定复用，
没有伪造一次新的测试执行。新133真PG零失败/错误/跳过、451镜像文件逐SHA通过，专用PG
容器/卷/生成密钥已按身份清理，五持续角色healthy、init退出0。原.env、其他受保护容器及
旧人工Word不变。证据在`deployment-v31/`及其`persistence-plan.json`指定的真实PG目录。

安服v31 outline已启动，execution
`web-outline-9496bb8597557fba86cd66aba621032ae10bd68a8bf6d8064558a009b7494d9b`，
state为`online-four-projects-v31/anfu/state.json`；**不要重复POST**。已精确继承两个A成功
回执，M首请求完整输入估算91164<94208并已发送。其前驱是v29，v30实际修改了M/C提示，
所以M/C不满足精确复用条件，必须按真实摘要新跑，不能声称本轮只有一次P调用。B/P是否
发送及费用按真实回执核，不从公式假定。v31尚无新Word，旧v29机器稿仍保留；后继
`run_four_projects_v31.py --project anfu --action status|adopt|document_start|download`，选择
新Word用`select_v31_generated.py`（已准备并固定旧机器稿身份）。只读`audit_v31_live.py`
会导出各实际接受P的完整源视图/回包，供28项独立分母核验，不能拿P接受代替充分。

本轮已部署两项源码修复：BUG189按同源组锚点恢复连续分册顺序，并一次反馈本阶段
全部正文冲突对；BUG193不再因叙述章已有子章就跳过其自身长正文的逐项展开。前者9项
定点/431项目录回归通过、真实四次拒收回包复核10/0；后者先2红1保护、后与跨页/原式
回归21通过。新源码没有改变A/M/B/C阶段或额度，P仍每个既有长叙述章最多一次（不递归）；
已部署，但安服新Word及Bidding旧Word仍待实物复验。加入BUG193后的最终全unit3055唯一
用例全部通过，零失败/错误/跳过，耗时290.98秒，v31持久化/镜像门禁见顶部；没有沿用v30
旧镜像的3045/133结果。两项实现前的成本/风险/修订五问及当前验证状态均记入bug-fix.md，
证据分别`bug189-compiler-fix/`与`bug193-nonleaf/`，最终源码SHA/审查见后者`code-review.json`。
部署前实查独立环境无活动authoring lease，原.env SHA仍d595fa7e…；旧人工Word仍为
17f90/982b且无编辑或待保存。当前有3个并行任务：主体布局精确出口、评审原表扩行设计、
来源几何/原式完整性核定；v31镜像已独立，后继源码修改不会热改该运行Spec。未提交/暂存/推送。

2026-11-17 09:27Z续跑实查：111/222的v30 Word/ZIP均已真实生成、显式选中正确新机器版本并
下载；生成各1次结构调用、零重试。旧机器版本与新版本DB/CAS/API字节在选择前后不变，
无编辑会话/待保存，未操作旧人工项目。111 execution `web-document-e80df532aba4a40e02e472498c5f3a377ed30e226d898ec87c128595a7675957`，
version `986d6119-7504-4b9f-8ea8-042025b24a10`，SHA `19b36e9ffdf019ef9eee300cecee94ad64e8ecdf634a0e67c8817142efeba381`；
222 execution `web-document-97997fd715d4db54e6815a9693737daa36acde5a35ddd71b115725621182fa15`，
version `954aef20-31fd-44e1-8b77-cad6566bfdf6`，SHA `294425afc670ac356385e625ad576e445d74e3ff368b19020af8dcce61a84cfe`。
实物原件回归：111为5通过/1失败，基本表实际仅一份且财务两主体保留，但指定评审响应表
仍没有原文条款；222为30通过/1失败，保密方案已落现有建设方案章，双签授权函仍重复。
这些定点回归不覆盖BUG190/191等全部缺陷，不授予最终验收。两份严格计划绑定无企业资料
审计分别316/108单元、268/80固定空位通过，12类负控全部拒收；粗下载审计“有blocks即
编造”的误报仍原样保留。证据在`online-four-projects-v30/<key>/authoritative-audit/`、
`source-reproduction-v30.json`、`strict-no-enterprise-audit.json`及`explicit-word-selection.json`。

本轮已实际完成四份来源解析/材料阶段，四份新候选Word/ZIP均已下载并绑定版本；安服v29
精确复用两槽目录成功回执，9次新outline调用零重试，另1次结构复核后Word出件。111/222
现有v28实物仍有缺口，正在用v30修主体/方案归属。不能把出件或阶段成功计为最终通过。

| 文件 | 当前实际产物 | 已确认尚存问题 |
| --- | --- | --- |
| BiddingFile | v28新Word/ZIP，27表；文档执行bd21286a… | 原件分册/报价相对顺序仍反；第二A槽4次拒收，未成功 |
| 111 | v30新Word/ZIP，18表；文档执行e80df532… | 基本表重复已修；强制逐条响应表、版式污染与空栏目仍未关闭 |
| 222 | v30新Word/ZIP，8表；文档执行97997fd7… | 保密方案已落章；一份双盖章授权函仍被错误复制两份 |
| 安服 | v31新Word/ZIP，16表；文档执行f2e2913e… | 28项应答和原书14已修；第三片后置/P列碎片/无关联承诺回归仍未关闭 |

前三份绑定与原字节见`online-four-projects-v28/<key>/authoritative-audit/`及同目录
`template.docx/template.zip`。222新版本`402db1ff-aa65-4e75-8f48-5be5e4b0f613`已通过支持的
API显式选为该验收项目当前机器稿，SHA`c66656d32fa57ed98315e8160d2031668effbaf44c8d8bdd3ad1673cd24e3a17`；
旧v27生成稿仍逐字节可下载，旧人工项目17f90/982b未操作。111新Word SHA
`a8a7a24f4f09e5d20cc9cd3d565a50814aea27ebdc1e6d68052b8d91fa19e0b7`，Bidding新Word SHA
`169022769b0868db780f84a7c8f357b6c12960f0008baac068f139eb6db154b9`。

v29历史镜像为`kb-docreader:review-20261117-material-coordinates-v29`，image
`sha256:93b8650fb346b96366e48d6d45e6da0092f438322a1664c9097fe54c21332ad8`；冻结453文件，SHA
`2379be099b523732c6e9e12258712acd2d26f260b13c1caeb627facba1220545`。3045唯一unit、133唯一
隔离真PG全过、零失败/错误/跳过，450镜像文件逐SHA一致，4种新内存变异全检出，专用
PG容器/卷/生成密钥均按身份清理。最新deploy/.env不变（d595fa7e…）、Grok/131072与既定
私有low覆盖生效，源parser指纹仍7feba070…；独立服务健康，受保护容器及人工Word不变。

安服v28已知不完整候选在记录96014>94208后经支持的cancel API停止，成功回执/历史保留；
新v29已绑定execution
`web-outline-b933d0f709d9fd39a62ec2518bda36a81bd3904f7abd9bfde0bfdcb6349e96ef`，不要重复POST。
该outline现已succeeded，并已采用rev `a1a4b5f4-baa1-42a0-8877-585f35bd2404`；document
execution为`web-document-7ad5c73f09a028a71f01254efd09665366bc3e4cda4fdb4498769da47c1dc9c0`，
生成版本`e4e3062c-b5ba-497d-bba2-4204e12d6b3c`，Word SHA
`28b0369cfb3946ed3f2a9d6dc7cf02ef47e1f6c5fa8e3a626651be5b7ff58db0`；
`online-four-projects-v29/anfu/`有实际Word/ZIP及authoritative-audit。196单元/167固定空位
严格计划绑定无企业资料审计通过，12类负控全拒；不关闭后述原式连续性和应答粒度缺口。
实际M采用坐标数组输入、
对象Schema输出，完整输入估算90966，真实prompt88270/completion8404、零重试接受；此前
两份A只精确继承成功回执，没有重新付费发送。后续修订仍须按真实摘要继承，不热改Spec/重置额度。

新增实证在BUG186–189及外置原件核验：222原有两组成漏项和非报价10章整表错挂已修；
31项核验v27为21/10，v28为29/2，剩保密及授权重复。检查器已按真实报价根/子章和Word
书签排除误报，保留旧非报价10章反例。111独立源审已完成21原式、基本表重复及
逐条响应表红基线（3失败/3保护通过），目录`bug186-111-source-review/`，含完整评审分母；
不以自审意见直接授予通过。
Bidding独立原表审计确认固定文字/格/合并/空白完整，无规定原表缺失、无评标工作表当form；
旧基线不认识两个有源锚的业务短名产生volume=null假错。已另建只增加两名称映射的基线
变体，原基线保留；真实分册/报价顺序BUG189仍未关闭，不能借名称修正掩盖。

v30仅改M/C两个提示：核双签/附证主体与独立整表实例、让完整技术方案子要求按真实职责
落已有方案章，保财务明确双主体及完整保密义务。仍4+B+P，Schema/阶段/额度不增；只从
既有来源候选选择，不做全局去重或强挂其他资料。当前真实Word实效见顶部；111强制逐条响应表
BUG188还未设计/实现，不能声称这两个Prompt已解决它或全部质量缺口。

v30已冻结453文件，SHA`ef2d138db0475bfd366f2633f45590249eb52692c8884109671017c549449c62`，
检查日志/结果在`deployment-v30/`；
PG运行身份由该目录`persistence-plan.json`指向，检查已全部通过且本轮专用PG资源/密钥已清理；
3045唯一unit、133真PG零失败/错误/跳过，450镜像文件逐SHA一致，gate期间源码未变。
v30已在2026-11-17 08:23:48Z部署，当前tag `kb-docreader:review-20261117-subject-scope-v30`，
image `sha256:f2c59f4244e4d4e59be9737e59558317e21a32de4e887bc226962ea7e3578afc`，五个持续角色healthy，
原.env、受保护容器及旧人工Word不变。后继M/C须真实新跑，
A body没变时只精确继承成功回执；失败A槽不得靠新ID隐去失败费用或重置同逻辑额度。

仅111/222有v30新outline启动回执，现均succeeded，不要重复POST：

| 文件 | v30 outline execution |
| --- | --- |
| 111 | web-outline-13a71c9fe7d9a22e0851aa8b9bd5ebe51883c07204d258cc4fa7cd3c4617b0a1 |
| 222 | web-outline-0dacff4c91f64910fd31443371d9d0e8e365a0611ae31574efd67b330cb4328b |

两文件均已精确继承两槽A、M首轮接受；111为12次新调用/零重试，222为7次/零重试，用量
全部已知。v30新Word已生成/选择/下载，绑定与复核见顶部。新candidate已核：111基本表仅一份（财务主体位置仍两份），
222保密义务已落建设方案评价，但授权函仍为投标人/制造商两份。因此M/C提示修改有部分
实效，却没有消除222主体误拆，不得再把通用提示更长或accepted当最终修复。下一步需先
验证更聚焦的实例核定/明确源审决议出口，不能无依据继续重复全文件M或全局去重。
入口`run_four_projects_v30.py --project 111|222 --action status`；
对应state已记录v28前驱execution、document和当前draft rev/SHA，以CAS保旧稿。后续采用新
outline后再生成，显式选择属于新execution的机器Word（当前可能仍指旧v28），确认无编辑/待保存
且旧版本字节不变，再下载和重新源审，不能把旧current_docx当新结果。

只读`audit_v29_live.py`首次读取继承A时因当前execution没有该旧model-request回执KeyError，
不是业务失败。已保旧脚本/错误，沿同run/protocol/operation的明确前驱链只读查原请求体，
验证摘要和source_projection后导出；v29重跑通过，v30 helper也沿用此修正，无模型/付费记录修改。

新质量问题已记录BUG190–193：111兼容评分CPU词被源页脚/左栏残片打断；空“补充要求”
源栏目不应成为独立应答章；安服表3三片断序/中插应答、28项独立服务部分共用一个空位。
安服独立源审已完成34原式及28项工作，证据在`anfu-source-review/`；12项真实回归6失败/
6保护通过，固定输入摘要全程不变。新增BUG194为原投标书第14项被迁走（独立交付仍有），
不能混称材料漏交。16原表及商务/技术两卷与总稿同源一致、数值1/4/12小时保留、实际投标
无关联承诺保留、企业回答/报价/外部动作未确认，均属通过的保护边界。以源原文/实际Word
定位判断，不用P=0或表数计数直接判充分。源码的逐条响应表/原生逻辑续表/来源版式处理尚未
修复，先看BUG188/190/192/193及该源审报告，实施前补五问，不能用全局去数字/去重或隐藏未决。

BUG189外置核定已完成：`bug189-source-order/review.json`、排序RED 13通过/7失败、
`rejection-diagnostics-red.json` 9通过/1失败。拒收第三回包同时有三组非法重叠（2065/2088、
3120/3137、3120/3147），当次只反馈一组；最后额度才反馈此前已知技术重叠。完整原回包/拒收
在`rejected-slot2/attempt-1..4-*`，源compiler仍拒收全部、额度/epoch/digest未重置。下一版可
按同一authority且无priority_basis的连续有标题scope做机械源序（保cover/无标题屏障/子items），
报价内部仍须A正确entry_ref971/972，不能把任意clause当排序；同阶段可一次报告全部已知重叠，
保原拒收条件和合法独立实例。尚未修改源码或A Prompt，不得把外置可行性当实际修复。

下一步：先冻结/验证/部署本轮BUG189/193新候选，精确复用A/M/B/C成功回执，让安服真实长
父章进入一次P并核28项实际空位，生成新Word再独审；不能重跑新ID重置Bidding失败A额度。
111/222的v30新Word已生成/选择/下载并复验部分实效，保旧稿；针对仍错的主体用最小完整
源样本验证专职核定可行性，再按五问实施，不继续盲加M提示。并逐项关闭逐条响应表、
Bidding顺序/完整冲突反馈、原生续表/后补服务响应、版式污染/空栏目及原书完整性。生成
同版本新Word，完成Office保存/同SHA PDF/分册实物审查与重新生成一致性、调用/用量/耗时。
仍未完成最终验收，不提交/暂存/推送，不借其他项目材料或编造企业事实。

## v28部署及v29形成过程（历史）

重启后的实际状态已重新读取。v27四个outline均已结束且无在途请求，但仅222获得材料
接受回执：BiddingFile的材料四次纠正均被源校验拒收，111和安服的材料完整输入分别
98989/110605超94208，零材料发送。不能把四个succeeded当作四份目录语义通过。
222已采用大纲并生成Word，保留在`online-four-projects-v27/222/template.docx`及同目录ZIP；
它是该独立验收项目的新生成稿，未操作旧人工稿17f90/982b。

| 文件 | v27模型请求/重试 | 实际尚存差距 |
| --- | --- | --- |
| BiddingFile | outline 16/3 | 材料无接受回执，表头/来源取舍待修复 |
| 111 | outline 14/1 | 材料超限未发，尚无Word |
| 222 | outline 9/0，document 1/0 | Word已生成；两项真实组成漏独立响应，评分表错挂，未验收 |
| 安服 | outline 19/0 | 材料超限未发，尚无Word |

BUG185短ID已接入真实M发送入口，先重验精确旧成功回执，再尝试短ID回执；所有身份
按命名空间恢复后走原Schema/原式/选择校验，映射摘要绑定请求，新合同写入execution_spec。
12项新回归先红10项后全绿，132项相关回归全过；其中8项首次失败为旧mock仍用永久ID，
已仅调整wire fixture并保留全部原表/拒收/Word出口断言。四源实际完整投影输入分别
76808/83499/45521/92528，全部准入，逐原文/空白/grid及全payload往返完全相同。
这些是本地验证，还不能替代新真实回包。

BUG186独立核原件：222物理48页L42/L44的“建设方案评价”“产品源代码、知识转移方案”
均被两槽共同漏建，composition误用格式标题/全书目录；物理33页整张评分889字被复制到
10个非报价章。另一方面物理76页技术服务原式应保留，短业务名无需回退原名；不能照自审
建议删真原式。A及章节要求Prompt已作针对职责的补充，仍4+B+P，必须以新实际回包复核。

v28冻结453文件，freeze SHA
`00791da65675d12d77bd1cdedbaa83317c6174c8228cc7a6cbc6c7742dd4b46b`；3042唯一unit和
133唯一隔离真PG全部通过，零失败/错误/跳过，运行前后源码完全一致。本轮专用PG容器、
卷及生成密钥已核身份清理，证据在`deployment-v28/persistence-plan.json`指向的run目录。
候选镜像已构建
`kb-docreader:review-20261117-material-closure-v28`，image
`sha256:c023198b5755401da569ec0dfafbdbf916c363c72797159aa97c6f82269e02c0`；450包内文件与
冻结逐SHA一致、parser指纹仍7feba070…，不重复套用旧unit/PG门禁。镜像检查不是业务验收。
v28于2026-11-17 07:03:07Z部署完成，五个持续角色healthy、init退出0；受保护容器及旧人工
Word身份/字节不变。新配置由最新deploy/.env生成，原.env未改，保持既定私有low覆盖。
另用内存变异独立反证未知命名空间放行、映射绑定丢失、原表身份字段改写三种错误均被
新回归捕获，未修改冻结源码；`deployment-v28/code-review.json`记录本轮范围复审。

`online-four-projects-v28/*/state.json`已准备：复用同项目/同source绑定，新增请求幂等身份，
明确前驱v27 execution。222同时保留既有draft rev/SHA及document前驱；其他项目无人工大纲。
四个v28 outline均已有独立启动回执，真实状态为running；不要重复POST：

| 文件 | v28 outline execution |
| --- | --- |
| BiddingFile | web-outline-6ddf72e681a6bbcbf8a588ccfeba590aed71ad002bf554cdd97152f8fb5d3a1e |
| 111 | web-outline-b582b3e7e04690ba509c234557893aaf4e6c98ee4d200fe2bc0b84c10c48de56 |
| 222 | web-outline-6cba260e232af7a9bf6f9240e86c5469de9768c26ca610cc5b0ff678c2249cf5 |
| 安服 | web-outline-099a1f168032d7a873b2e3d20e367c9efde0897b14440aa02e4d86c350b9f4b1 |

首轮实际wire审计确认每份请求发送完整106/101/77/118页、3622/3623/2801/4003源行，
新A Prompt和low/Grok/131072配置均真实生效，材料身份合同已固定在各execution_spec。
安服第一槽已接受并开始第二槽，其余第一槽仍在途；这是某时点快照，须先用
`run_four_projects_v28.py --project ... --action status`及`audit_v28_live.py`核当前状态。
新material尚无接受证据，不能把离线容量通过当线上材料通过。222当前显示的候选及Word
仍可能属于v27，任何采用/下载须核execution等于对应v28绑定，不能沿用旧成品称新通过。

222旧稿的严格计划绑定无企业资料审计为164任务/164单元/137固定空位、12种负控全拒，
证明旧download脚本“有blocks即编造”的判据过粗；不关闭BUG186组成和错挂问题。
其独立原件/实际Word红基线为20通过/10失败，实际只对应两个缺陷在多层的断言；
`bug186-source-review/reproduction.py`可对新slots、workbench及DOCX复验，不强求长原名、
不把正常勾选选项当源冲突。222两份外部引用沿既有来源盘点的具名待核边界处理。

后续等待上述任务、核实际材料输入/回包/组成与评分归属，再逐项目采用/生成/保存与实物验收。
旧v27 state和所有历史
Word/ZIP不覆盖，旧人工项目和受保护容器不动。不提交、不暂存、不推送。

## v27部署与启动事实（历史）

v27已经完成新冻结/构建/部署：451源文件，freeze SHA
`456f95b2f15eda0dbd09fd7121661b44d9f9daffa8e3ed4f772b42f43afc9b40`；
3030唯一unit、133唯一真实PG全部通过，零失败/错误/跳过，运行前后源码完全一致。
专用PG容器/卷/生成密钥已按身份清理。镜像448文件逐SHA与冻结一致，Python3.11.18；
tag `kb-docreader:review-20261117-source-refs-glyphs-v27`，image
`sha256:dcccb54a06217eb8c8044bc81f1807712359353573ed872dfa403e29299eebc8`。
v26的18个PG失败仅由目录专用参数无条件透传None引起；v27将其限定在实际目录合同，
非目录路径不收新参数。没有删断言/跳过测试或把v26旧通过结果冒作v27。

独立5个持续角色healthy、init退出0，zhaobiao及其他受保护容器身份、Office/存储不变，
旧manual17f90/982b当前版本与字节不变。部署证据`deployment-v27/deployment-summary.json`。
实际使用最新deploy/.env（SHA d595fa7e…）的Grok4.7/131072/知识开关true，私有独立候选
对未显式配置的reasoning_effort补low；原.env未改，显式用户值始终优先。此覆盖已提前告知。
目录合同为实际接线的`source_refs_compact_plan_v11`，仍4+B+P；没有部署大型新分包框架。

为避免替换旧人工稿，新建独立v27验收项目，原四项目及旧取消执行均保留。每项目仅上传
其自身原PDF、SHA和物理页数均与原件核定一致，不借用其他项目或企业资料：

| 文件 | 新project_id | 当前outline execution |
| --- | --- | --- |
| BiddingFile.pdf | 606d94ac-406f-5144-ab6e-e01dc1bdee59 | web-outline-5242d7aef1b486f0deba1347bce3359adf9d9f06b0dc59ddc57d7b6bd9ffc09c |
| 111.pdf | d74deca4-8b03-5f40-97c3-95af3728f028 | web-outline-601ee5b943901e8043dec208c4478e516bfaf7de37a4fcfb80d412a4ea0f2325 |
| 222.pdf | 650f789d-9c46-5975-9b64-24a644a0898a | web-outline-fae6d666eee6ad813a71cc5025e63ec272ab23922447ee2c86b6db207abfa4c7 |
| 安服.pdf | 65331b39-2db3-5cfd-ab0a-3da57158c16b | web-outline-3c3aae31676c63dfc8620cdbe658c26bdf37201df7a1a95f065650e13cee7c70 |

启动请求已回执并写入`online-four-projects-v27/<key>/state.json`，不要重复POST；先用
`run_four_projects_v27.py --project bidding|111|222|anfu --action status`读取真实状态。
首轮只读状态已确认Bidding/222运行，222第一槽已接受、零重试；111/安服也已成功提交。
`audit_v27_live.py`是只读配置/新冻结/实际发送body摘要核对，首次查询含不存在的artifact
created_at排序导致审计未完成，已去掉无关排序重跑；该脚本故障不是业务执行失败。
后续若状态仍运行则等待既有任务，不能热改运行Spec或重置每逻辑四次额度。

四份原件当前解析器指纹7feba070…的两次native解析每轮402页完全一致、PUA未核0，
不等于目录、要求、Word/PDF及无企业资料模板的最终语义通过。当前真实新成品仍**0/4**，
尚未采用新大纲、生成新Word或打开Office保存会话。下一步等四个正式outline完成，核
actual body/条件/短业务名/组成顺序/参数/源要求，再逐项目采用→生成→保存Word/PDF/分册验收。
任何发现继续记bug-fix并修复，不以大量待核、空Word或结构通过授予验收。没有提交/暂存/推送。

## v25重启恢复与v26/v27形成过程（历史）

### 重启恢复后的执行边界（2026-11-17）

宿主04:00:42Z重启后，已检查原进程和持久化回执；独立`kb_isolated_*`服务已恢复且healthy，
线上仍v25，没有重建或操作zhaobiao环境。BiddingFile、222、安服三轮v25执行已取消，
无活动prepared/sent请求；111尚未启动正式模型流程。下文的“正在等待/尚未启动安服”是
重启前历史状态，不应据此重复启动。四份新模板成品验收仍**0/4**。

旧诊断4/5的脚本已落盘，但无preflight/结果目录；历史发送、远端受理/执行/计费均unknown。
已确认旧客户端不再存活，未重放它们。新诊断6有不同的轻锚协议，保持完整106页3622行，
只发现候选/组成/分册坐标与源块路由，发送前保存nonce、请求摘要、完整请求和send-intent，
20秒持久化进度；沿最新env、原Grok/131072，零自动重试、360秒绝对截止。它只是可行性
诊断，不能当正式目录或四源通过；结果见`live-model-diagnosis/anchor-only-probe6/`。

当前源码另含尚未部署的BUG177 reasoning选项透传、BUG178精确字体数学符号及全源编号行对。
恢复定向验证分别33项和23项通过；当前源码439文件固定快照的隔离真PG新62项全部通过，
零失败/错误/跳过，证据在`root-integration/persistence-reasoning-19567b72f103e58f3e08/`。
当前源码未全量unit冻结/构建/部署，
不得套用下文v25旧gate。真实env未设置reasoning_effort，不能称medium已配置或已解决大请求。
无损全源medium诊断3在360秒内仍0业务JSON，故仅压元数据未证明可用。

有界目录方案在仓库外`bounded-directory-design/proposal.md`和`design-summary.json`；先验证
全源轻锚，再决定局部明细或完整页块首次发现。材料包必须包含残余源块的遗漏发现、整批
原子校验和跨包修复出口；全局版本变更最坏仍全链失效。没有新增正式Word、采用或保存操作。

恢复后新的真实证据：默认强度全源轻锚6、全源短引用10均360秒仍0业务JSON；完整18页
局部轻锚7在213.83秒返回27候选，短引用单职责8在89.32秒返回23候选；同请求显式low
对照9为25.10秒，23个ref及顺序与8完全一致。全106页短引用low对照11为76.48秒，
68候选/19组成候选/6组标题候选，均只证明坐标有效，不是最终必交或目录。真实.env未改。
现有完整DirectoryPlan在low下12/13/14分别168.94/151.97/128.28秒返回完整JSON和usage，
依次被原Schema、组成覆盖、正文重复所有权拒收；没有手改回包授予通过。原逻辑首次加
三次额度由12–15共同遵守，15是最后一次原协议纠正；16是另一短引用Schema实验，单独
记账，不重置旧额度。当前目标仍先验证可用最小路径，不先部署大型分包重写。

BUG181已汇总一轮内全部未决组成名称，77项相关回归通过；BUG180新增parser_version及
实际source_spec绑定source-only身份，真实PG先红2项后3项全绿，专用容器/卷/密钥已清理，
证据`bug180-source-profile/`。当前PG仍非最终全量；新来源profile会生成新run，新一轮不能
携带旧source-only run的outline/document前驱，旧manual及历史大纲继续保留。

四源已按最新env分别做两次新原生解析，每轮402页，逐文件原文/原生单元/源合同摘要相同、
PDF字节不变。发现111的16个F06C黑圆点后，BUG179经独立字体/视觉证据和失败回归修复，
49项含真实原页/数学符号回归通过。后继`native-four-parse-v26/`中111不再有未核私用符号；
安服仍有52个F06C及1个F0D8、12个source unit未核，记BUG182。该字体无cmap，不能套111
程序或按PUA猜字形；正在核实际PDF Encoding/ToUnicode/CIDToGIDMap并建立反例。
上述解析通过均不代替原式/要求/标题及真实保存Word验收，当前新成品仍0/4、未部署。

### v26完整检查与后继修正（未部署）

当前完整短引用合同`source_refs_compact_plan_v11`已真正接入Schema/Prompt/两槽、旧计划
编译及恢复验证，仍4+B+P，无新分包层。原协议12–15四次合计约534秒才过源校验；
短引用16首次137.37秒即过，实际total69700 token，对照原四次305967。独立原件审查另证
附件7漏跨页签字/阶段混并（BUG183）和最终nest报价顺序颠倒（BUG184），故没有授予通过。
后继集成17在208.10秒通过，已分开承诺/银行原式并选回物理88页签字；条件仍只选孤立
勾选值，Prompt已补完整主体/时点/义务上下文约束，须在正式新双槽继续核验，不冒称已闭。

BUG182已按精确字体程序+PDF Encoding/CIDMap/ToUnicode SHA修复；同字体交换编码、
交换CID流及竞争资源均保未知，76项符号/来源绑定回归通过，独立source hash变异被测试
捕获且生产文件未改。`native-four-parse-v26-final/result.json`证明当前解析器指纹
`docreader-source-v2/7feba07058d2227c532598c2170dc4b9e1b8fe95745c7f00ef94e4db4dec2d13`，
四源每轮402页、两轮native文本/单元/合同摘要完全相同、未核私用字符均0，原PDF不变。
各文件单次约2.93/1.81/3.01/13.37秒；这是native解析证据，不是LLM或Word性能。

v26冻结451文件，SHA`466655008f710ec62b0f5213575a70d758667f5992ca3f0d6f85e39f05f8c20e`。
3030唯一unit全过、0跳过，镜像448文件逐SHA一致，Python3.11.18，镜像
`sha256:f297a41f394ed2ab2010b446720488bd86084a0d86364891b3b6dd65ee16ff81`。133项真PG中
18项失败：17项同一无关正文分支收到directory_contract=None的参数兼容问题，另1项由
该问题导致等待超时；115项通过、无错误/跳过，不能当最终通过。已按仅目录合同存在
才传该参数修正，需新冻结/完整unit及PG后继；v26专用PG资源已清理。

Ruff新增/相关18文件通过；pdf_parser.py原有reading_order_text未用导入在HEAD与当前
相同，已保存baseline对照，不删除无关旧代码，也不声称该旧诊断不存在。证据`deployment-v26/`。
最新env SHA仍d595fa7e…，私有候选基于它、额外显式low、用户若显式设置effort则优先；
原.env未改。当前运行仍v25、没有正式四源新Word或保存动作，后继须用新source profile，
不携带旧source-only run的执行前驱，但采用时保留旧大纲rev的CAS及人工稿。

### v25冻结及重启前记录

用户明确要求`testdata/bid`四个项目全部满足完整性、一致性、正确性和效率，不能只用
BiddingFile样本代表整体。四个PDF分别106/101/77/118页，总402页，项目身份和SHA已逐一
绑定；111内嵌两个附件名称实际指向同一DOCX原稿，已提取核读并继续逐段/逻辑表核对。
八个外部引用按投标/签约/后续执行职责分别处理，不以未定位引用自动阻断已提供正文解析。
尤其BiddingFile不要求实际担保、111要求合同额5%，不得跨项目套用裁定。

最终候选v25已冻结435文件，2876唯一unit、60隔离真PG全过，零失败/错误/跳过；
v24首次全量被中断且发现旧TOC无编号断言，后继仅更新该断言保留全部链接/目的页检查。
v25冻结SHA`a38354c900a7d4fdde80a423e6af96a63f3e34645fa003a6503ec90dd788d191`，
镜像`sha256:c9894b37337e0570a76363a637d58b63f9a002a885f51c0f258ae1d6055b33e5`；
432包内源码与冻结逐SHA一致，3份根SQL由PG gate锁定。专用PG资源与生成密钥已按身份清理。
独立审查发现的BUG173非法源窗先成功、BUG174深层编号/辅助目录/孤儿顺序、BUG175参考
范本不提交却仍在正文的组合矛盾均先红后绿；源要求裁定的case/合并/坐标/依据已进入交付追溯。

独立栈已部署`kb-docreader:review-20261116-directory-integration-v25`及最新`deploy/.env`，
Chat/VLM实际均`grok/grok-4.7@500k`，显式context为131072；知识库开关按文件真实值true。
env SHA`d595fa7e3c90729431702383d88d81952d45e5152d87510267863751f37ba4b5`；候选配置
清除宿主同名旧变量覆盖，保独立存储与Office配对设置。五个持续角色healthy，init退出0；
所有zhaobiao容器及独立Office/存储容器身份保持，旧manual17f90/982b及历史包字节不变。
env RERANK三字段当前仓库未透传，不据此宣称知识检索通过。

四项目已实际注册：BiddingFile沿用`2e41e3da-d217-5d5b-8690-9d08ab4b2506`，111为
`881eaa0f-434a-53b5-88ff-57120a83470e`，222为`c87888fc-0332-5749-b66f-32cd610b7174`，
安服为`4db4e009-98bf-5b36-a97c-2911041775e0`。当前BiddingFile与222的新outline已启动；
实际请求台账确认使用新Grok，首个请求仍在等待完整回包，usage未返回，尚无新Word。
111/安服尚待正式新流程；旧直接目录调用和离线回放不能作新v10+新模型完整验收。

四份新模板的真实成品验收为**0/4，持续执行中**。旧o200k预检峰值87365只是估算，
须用新provider实际usage、重试、整份耗时及新回包再核；不调高.env上下文，不以请求已发送
或基础设施healthy当业务成功。源目录、代码/gate/部署及真实运行证据均在仓库外
`pending-closure-v1/`，动态状态仅本交接维护；不提交、不暂存、不推送。

## v23 / v14验收继续：用户未接受最终通过（历史）

用户明确“没通过还需要继续持续修复”，整体目标重新打开。上轮结构、保存链、
Word/PDF/目录及交付包字节校验是已完成的技术证据，不能据此结束来源归属与适用性验收。
当前50项待核已重建：18项计划冲突、5项适用性、4项候选未并入、23项要求未归属；
此外还有未计入该50项的requirement_missing与authoring_input_missing，正在核定真伪。
已证新问题包括电子签章/扫描原件条款在两请求块边界断成两项、已有专用不适用依据的
条款仍未归属，以及无效子节点group差异形成的假冲突。真实企业未知继续留空，
不能通过隐藏警告、改计数或将未归属全部改成全局要求来宣称解决。

运行版本仍v23，实际保存稿17f90/982b及既有原始包、审查包保持字节不变；
没有重新部署或打开编辑会话。新证据置于仓库外`pending-closure-v1/`，逐项源审后
再记录修复方案、调用增长与实际验收。当前整体验收状态为**未通过、持续修复中**。

### 当前修复快照：标题、来源闭合与容量（尚未部署）

用户截图指出的长句/残句/裸附件编号标题记为BUG170；业务显示名已与原名/来源锚分离，
材料现有读取统一命名，参数现有请求补短业务名，原文/原表不改，来源名称对应写入指南。
正文使用原生Word层级编号，主件与分册共享宿主路径，可编辑标题不含宿主数字。
348项目录回归零失败；50项导出/删除原式/覆盖/版式邻接回归通过，彼此有重叠不加总。
新材料正式入口拒绝漏名、附件定位号及来源格式号；未知取舍和企业身份不伪称已满足。

编号合成实物已在一次性、无网络、字体缓存只读的隔离Office更新目录并输出PDF：
主件和第二分册均实际显示2/2.1/2.1.1，原可编辑标题、正文/书签/原表校验通过；
这只是编号运行证据，不能替代106页真实原件的新名称和最终Word验收。
证据在`pending-closure-v1/root-integration/native-numbering/numbering-runtime-audit.json`。

BUG167来源闭合已接material目录取舍和B之后全源classification要求裁定，不新增模型阶段；
跨块完整条款、外部待执行、本次不适用、企业条件待确认分别保留。公开指南保留完整
参考范本、所选来源、原文及裁定依据。BUG168关闭5项确定假冲突；BUG169删除原式仍保分母。
22个新目录case人工独审中20个有可验证来源选择、2个仍未决；这不是模型回包或正式关闭。
67项旧50清单之外的源审已完成，63条missing对应64叶章：55有源载体、8引用待绑定、
1空标题叶章；4条旧链input提示另列。6份第四章协议正文不在当前冻源及2类引用细节
未定位记BUG171，没有批量清警告或编造来源。

BUG172容量预检最初发现材料99504>94208，不能以其他请求继续运行冒充全量准入。
材料全源行表示改为无损[行号,完整原文]后，完整13请求容量预检峰值87272，
均在既有94208输入上限内，输出fixture峰值9071<32768；131072上下文/4096安全量未改。
预检使用真实冻源、旧坐标及40字合成标题上界，未调用模型、未采用这些合成标题，
不证明所有候选分支或后续实际请求；正式新材料取舍后必须按实际payload再量测。
证据`pending-closure-v1/root-integration/capacity-v10-v3/report.json`。

全量unit、真实隔离PG、独立代码审查和新实际模型/保存Word/PDF/三分册仍需完成。
独立环境运行仍v23；本轮新增模型发送0，未部署、不提交、不暂存、不推送，旧17f90保存稿
及既有包不改。新候选协议为`source_closure_business_titles_v10`，不能套用上轮测试通过结论。

## v23 / v14上轮交接记录（历史，已被当前用户验收结论取代）

本轮无企业资料模板及最终交付包独立验收全部通过：
`overall_final_template_acceptance=true`、`final_delivery_package_acceptance=true`；
`enterprise_business_acceptance=false`。50项待核（含2项重点unassigned）均保持未确认，
人工处理出口保留；企业资料匹配、企业应答及这些人工决定不计为已完成。

当前实际保存稿为manual `17f90e56-ff4c-4750-b99a-bb92384ee27a` rev9，115759字节，SHA
`982b2c830f35a772b902db0f91305cfd1570da11d3c40468c089382a0ca84c6e`，直接parent为
`ff54c637-bb71-4cd8-832a-e85f28c1ff9e`。同manual PDF90页，SHA
`be8ea862277ef10a76b803d81eb73a6cbc5f54a78583d2e2634554a85ebe3a9d`；三份新分册商务35/
技术36/报价11页，合计172页，179条目录目的页、27个原表实例及35参数均完成独立验收。
v23实际运行源码417份逐SHA一致，2746唯一unit、真实PG58全过且零跳过。

- [最终验收包](/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/reviewed-delivery-v14/无企业资料投标模板验收包.zip)：12193531字节，SHA
  `2f624471ebd2983ef2134e898cf8cf5beda72718f1ed038a592c954d2b949dcd`。
- [验收报告](/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/reviewed-delivery-v14/验收报告.md) ·
  [最终回执](/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/reviewed-delivery-v14/final-acceptance.json) ·
  [独立最终门](/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/independent-semantic-review/v14-Word-review/final-package-and-overall-template-acceptance.json)。

包内及外部manifest SHA均为 `b8a0f9453689da594f92c06b02cf1025644a60171e28b75d77689cadcd45aa02`；
独立最终门SHA为 `332c073f8e0088ef50897e55d020fe036e9c6ef0e5baecb8cf429367edc67e0b`。
原7331a ZIP、a636生成稿及旧manual全部保留，当前17f90无editor/pending/未关闭session。
v22冻结已作废，1056例中断不算全量通过，其build/PG仅保留历史证据且从未部署；最终通过
依据v23/v14新证据。不提交、暂存或推送。

## 历史过程快照

以下v11/v19至v23/v14中间保存点保留当时事实，其中“待终审”“未关闭”等均为历史状态；
当前结论与未确认边界以上方最终交接及最终回执为准。

独立栈当前Python已部署v23，镜像
`sha256:b2a24c0e71487278e3c68b4c98cecf60f47bbf89d16a7e01fc0b543e3bbefafb`。
2746唯一unit、真实PG58全部通过，零失败/错误/跳过；417冻结文件与包内及运行中源码逐SHA一致。
实际init退出0、三个持续Python角色healthy，Office/API及旧栈容器未变。最新只读实查
当前为v14真实Office更新整目录后应用保存的manual
`17f90e56-ff4c-4750-b99a-bb92384ee27a` rev9，SHA
`982b2c830f35a772b902db0f91305cfd1570da11d3c40468c089382a0ca84c6e`，115759字节；
直接parent为原generated ff54c637。真实callback/DB/UI与API下载一致，当前无editor、
pending或未关闭session。原gen a636/105455字节、原ZIP7331a及旧e03ac人工稿均保留；
正常同manual PDF90页，strict目录及raw→manual保全已过，视觉/分册/审定包尚待完成。

v11目录 `web-outline-b55c1f60bb6a0e135d8923801cc3e9e47b3d950a2bf9e1e60a68d18a5adde02a`
已成功，13逻辑回执均按实际摘要匹配后重验复用，新增模型发送0。候选SHA
`ed62015f837386d7f3862ff7dbc32ccf4d26957fc913fda1516802731620b65d` 与独立四次回放
严格相同，已采用修订 `63e0b729-7a1c-4034-81c4-4fe7243db864`。文档执行
`web-document-d07d1a251bd113ead315b1a68c4c26aab9c0584a48782d7743ed5963880d5216`
成功，1次structure、0retry/unknown，64742输入/1054输出/65796总Token。

- 原generated `fd5f6501-0100-43f5-84e3-3707e73a905f` rev6，SHA
  `b68c424b9fce18416546f9e0585487efbabd6369eac6066ff1f9548df5fb40b9`，107624字节；
  raw DOCX/原ZIP/authority已保全，ZIP SHA
  `a5d61d81c10c63913b7a9cfd5185a52d66dd6fdcd7a771641e7f77364350fa2c`。BUG-159原件
  锚定封面门及ZIPnotes保全已过，不再出现原评分要求及应答空位污染封面。
- v11经明确授权，真实UI更新全目录并应用保存；已保留manual
  `e03ac0cd-bfaf-4284-82b9-e5934e92ed94` rev7，SHA
  `01d70b010ec3ba3876ecab25caf07e4e014dd8bf2008ab6a45b632beebf752da`，118170字节，
  直接parent为fd5f。实际callback/DB/下载一致，原gen及ZIP、v10坏raw和旧e5f3人工稿
  字节不变。正常同保存版PDF95页；主TOC90条及三卷22+64+3条均按严格PAGEREF与
  完整Dest实际页逐条匹配，无需二次刷新。长标题多annotation不按同页号误合并。
- 完整来源/格文/合并/表头/kb锚点/media保全技术门已过，仍在实物终审发现三Required：
  **BUG-160** 商务8D标题、主体标签与首表分页保留链断开；**BUG-161** 履约义务的
  clause与目录entry同坐标机械重复；**BUG-162** 父级物理原文切片仍继承narrative应答
  职责，在完整跨页原句中插入应答空位。根因及修复边界已记问题记录；三项均未关闭，
  不以保存、目录页码或保全测试通过代替新实际Word及各分册验收。
- 原保存流程8条console error已无dirty重现：read/edit各缺两份插件静态翻译资源，
  各有SSL脚本获取及ServiceWorker注册错误；发生在载入而非关闭。真实保存/下载成功，
  但不称全UI干净；来源分类证据保留，不为消日志改Word、证书或运行配置。
- v19修复源码已按根任务明确指令冻结413文件，相对v18仅export、directory_compile及
  三个新增unit文件变化；Prompt/Schema/spec及依赖不变。**2718唯一unit、真实PG58全过，
  零失败/错误/跳过**，前后源码无漂移；候选镜像
  `sha256:30ece39fa70ce79d4d39b71cae123b3433d3d7da3deb00ced37e19ef29c33200`
  构建成功且413文件与冻结清单逐SHA一致。专用PG及凭据已按身份清理，全部验证进程结束。
  独立复核另发现**BUG-163**：商务1.10.2同一原句在“中标人应”与后页“当就分包项目”
  之间被响应空位截断。已确认requirement assignment单物理页窗口及Prompt同页约束
  无法表达完整跨页条款，不是参数父载体问题。**v19明确不部署**，技术绿色证据完整保留；
  根任务已明确解除源码freeze，仅继续BUG-163最小修复，后继须新冻结候选及实际Word/
  分册验证。v19快照未启动新模型或任务、未获部署放行，禁止部署的结论保持。
- v20已补BUG-163及**BUG-164**：已有来源node时，组成entry.name仅为命名点，不再
  自动追加为独立义务；显式clause、原式、元数据和无node出口保持。真实新稿尚未验证。
  已按根任务明确指令重新冻结415文件，目录合同升为
  `source_roles_requirement_continuations_v7`，候选Dockerfile检查RequirementAssignment
  的continuation_windows及v7。2736唯一unit及真实PG58均零失败/错误/跳过，源码前后
  无漂移；build及415文件包内逐SHA一致通过，专用PG与凭据已按身份清理。独立160–164
  源码评审无Required、完整技术门齐后，根任务明确授权并已精确部署四Python角色。
  init真实容器复查退出0，三持续角色healthy；只改四image字段，Office/API/旧栈身份
  不变。**这些是技术与部署通过，尚未授予新实际模板验收**。
  v12 state以成功v11目录为前驱，修订父63e0b729、Word前驱d07d1；13响应仅在实际摘要
  相同且重验有效时复用。v12目录
  `web-outline-8396ccd3fb70eba4e632ffaae5a1c84f715c8b8bba7c532f72aeedb46cf14bd8`
  已取得7个要求块新回包，实际物理sent8、retry1、unknown_usage1；未知用量不写成0，
  不冒充完整总用量。来源独立门发现4组认证/检测/荣誉要求误挂8H失信截图，**源门不通过**。
  根任务已hold，**未采用v12候选、不生成v12 Word**，不能以目录执行成功代替职责正确。
  BUG-165最小修复现已落实：仅requirements目录为form/attachment附完整source_body、
  为显式义务附source_obligations，material目录不变；这是payload上下文，不新增模型
  Schema字段，不截正文、不让模型另写职责摘要。合同升为
  `source_roles_requirement_duties_v8`，原4组误挂及其它目标仍须新实际回包逐源复核。
  根任务已明确冻结v21共416文件，相对v20仅pipeline、requirement Prompt、execution、
  acceptance断言及1个新unit文件变化；global Schema、要求response Schema、依赖及
  material Prompt逐SHA不变。**2738唯一unit、真实PG58全部通过，零失败/错误/跳过**，
  前后源码无漂移；候选镜像
  `sha256:adfc91ac688b784e8c0fae9f970a2eeb682572616dbdccbe0228da2707929afa`
  构建成功，416文件包内逐SHA一致，职责payload/Prompt/v8导入门实际通过；专用PG及
  凭据已按身份清理，全部验证进程结束。根任务已逐SHA核定独立160–165源码及容量门，
  无Required后明确放行，v21已实际精确部署四Python角色；init实际容器38400ae0退出0、
  三持续角色healthy、416文件运行中逐SHA一致，Office/API/旧栈容器身份后验不变。
  **实际新回包/归属/Word尚未验收，不以技术与部署通过授予模板通过**。
  v13 outline前驱为成功v12 8396，因v12未采用，修订父仍
  v11 63e0/ed620，Word前驱仍d07d1；预计7要求块真发，其余仅精确摘要匹配且重验后
  复用。根任务已通过API启动
  `web-outline-40878f2b18a03e14a1cef1838c62f636e02d131919650425e5d40a4036988334`，
  平台子线程不重复启动或打开编辑器；仅准备v13专用路径及UI保护flag，不写旧轮文件。
  v13新回包已正向修复原8H职责错挂，但源门新增**BUG-166**：跨阶段产生同源、同章的
  机械响应重复；其它全来源审查仍收尾。**v13继续hold、未采用、不生成Word，尚未验收**。
  BUG-166已先记录五问并完成宿主修复：绑定实际clause来源，逐章按来源subset去机械重复；
  部分重叠仍保完整原文及具名待核，新issue接入既有Word/ZIP处理栏，不用全局同文删重。
  根任务已明确冻结v22共417文件；相对v21仅directory_compile、authoring_coverage、
  document_execution、PG合同断言及1个新增6例unit文件变化。Prompt/Schema/payload/
  context及依赖保持，目录合同升为 `source_roles_same_chapter_ownership_v9`。
  v22独立评审确认职责边界过宽：仅form原文载体及未实际生成common narrative的
  obligation==body也登记为响应owner，可能吞掉合法独立应答。**v22 freeze已作废，
  绝不能部署**；根任务拟撤_adopt_clause collector，仅为实际common narrative登记
  ownerrefs，后继使用v23新冻结证据，不把旧候选绿色转记到新代码。
  已按明确指令停止本轮fullunit：SIGINT优雅退出、exit2，执行1056唯一用例但整套未完，
  **不算fullunit通过**。此前build完成，镜像
  `sha256:18fc9924ce8b0d2f0fad9f075c249e1154e5e1bd80a5eebc899d858c758bf713`
  及日志保留；真实PG58已通过，专用PG/凭据按完整身份核查后清理。全部验证进程结束，
  live v21容器ID/镜像不变；新源码仅在这些进程结束后由根任务修订，平台不另启验证。
  v23现已完成收紧：撤no-node collector，仅当obligations与body不同、确实生成common
  narrative时保存ownerrefs；新增两个真实反例先红后绿，ownership共8例。已按根任务
  明确指令重新冻结417文件，相对live v21仍5路径、相对作废v22仅compiler及ownership
  unit变化；Prompt/Schema/payload/budget及v9保持。**2746唯一unit、真实PG58全过，
  零失败/错误/跳过**，417文件前后/包内/运行中逐SHA一致，专用PG与凭据已清理。
  独审角色边界及实际处理出口通过，实际13回执两seed×两时序四重回放仅删1机械重复，
  新golden为 `ed86fd2ee15d9c91ad9cf0909b78090b7aaf2fe3bcf8200890fff5e73a021814`。
  根任务逐SHA核门并明确放行后，v23已精确升级四Python角色；init实际容器018d2262
  退出0、三持续角色healthy，Office/API/旧栈后验ID不变。v22作废证据原名保留不变。
  v14以前驱成功v13 40878启动outline20ca1331，实际候选严格匹配该golden并通过正式
  source gate，已采用 `c0b16446-652b-4ed1-82a3-a8c752f1afd9`/ed86，Word前驱仍d07d1。
  文档执行 `web-document-f71b4d084f0ef93adb4cbc6ab8e892dd86274fc736c7feae1cc0fc2a027e3e27`
  已成功，1structure、0retry/unknown，61680输入/1502输出/63182总Token；无writer/retrieval。
  平台只读解析唯一gen ff54c637，成功job固定修订/执行outline_ref、DB版、实际CAS与API
  下载逐SHA/字节一致后按既有API显式选择，receipt在v14目录；未等待人工誊UUID，未直接写SQL。
  e03ac旧人工稿、fd5父稿及旧e5f3实际前后下载字节不变。根任务确认raw DOCX/原ZIP/
  authority及cover/source/noenterprise必要门全部齐后，已明确放行真实原生UI；平台执行
  更新整个目录及应用保存，发布唯一manual17f90，实际signed callback/DB新child/UI和
  API下载逐SHA一致，另文件保存，ff54原稿与原execution result_ref不变。
  正常converter仅取该manual，输出90页PDF SHA
  `be8ea862277ef10a76b803d81eb73a6cbc5f54a78583d2e2634554a85ebe3a9d`；自身edit/view
  全闭，Office容器/image/WQY字体及三个cache逐SHA不变，不使用liveDocBuilder。
  独立strictv2核90 PAGEREF/92物理annotation→90 fullDest逻辑项，实际目的页0错，
  raw→manual正文/27表合并/表头/书签/media完整，无需二次保存。90页同版图像及5概览
  已齐，供独立关键图审；各分册实物独立验收及审定包仍待完成。
  本次UI8console error实际归类为langs.json/zh-CN.json各2次静态翻译404，加4条SSL/
  ServiceWorker载入错误，pageerror0，非关闭会话失败；不以保存成功声称全UI干净。

证据根为 `/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/`。
v20见 `deployment-v20/`（deployment/validation/init receipts）；v11保存链、源审计、PDF
和console分类见 `online-template-v11/`；
严格目录/原文独审见 `independent-semantic-review/v11-Word-review/`。v19技术证据见
`deployment-v19/validation-summary.json`；v12实际身份见 `online-template-v12/state.json`。
v21实际部署与后验见 `deployment-v21/validation-summary.json`、`init-receipt.json`及
`runtime-source-verification.json`；v13实际身份及平台阶段边界见
`online-template-v13/state.json` 与 `platform-phase-preparation.json`。
v22作废与安全停止见 `deployment-v22/freeze-invalidated-stop-receipt.json`；v23实际部署及
后验见 `deployment-v23/validation-summary.json`、`init-receipt.json`、`runtime-source-verification.json`。
v14见 `online-template-v14/state.json`、`explicit-word-selection.json`及`platform-phase-preparation.json`。
实际保存链见v14 `office-saved-{bindings,version,callback,chain}.json`，最终状态见
`platform-final-saved-state-v1.json`；同manual PDF、renderer/字体绑定及90页图像见
`office-saved-layout-v1/`。
整体无企业资料模板验收未关闭；不提交/暂存/推送，保留共享工作区其它改动。

## v17运行 / v18封面出口修复待部署（2026-11-16，历史快照）

独立栈当前Python为v17，镜像
`sha256:8beaf6ddadeb8226ba369c2f68752cb5882c3adac1fef81011cce6d8a54a8869`。
2693唯一unit/真实PG58均通过、零跳过，410源码与包内逐SHA一致；init0、持续三角色
healthy，Office/API/旧栈未动。实际v10目录零新模型请求并精确匹配六次真实回放golden
`4e889e7799314b998c99ac286ca2550caa0c631af05e5816244f4dc7f676cb03`，37源义门通过。

当前Word为已显式选中的generated v10 `8b03a257-c07c-4a4c-a2de-15e9984d8f51`，SHA
`661b907a1950f287f166acad1ef174065d1220da275a9cf6981e6ae4d27350a6`，107885字节。
raw DOCX/原ZIP、authoritative提取已完成且身份一致；**未编辑/更新目录/保存，仍hold**。
独立实际Word发现封面评分原文/响应空位污染（BUG-159），不能用之前源门通过覆盖；
旧manual e5f3字节不变。坏稿与原ZIP完整保留在 `online-template-v10/`。

- BUG-159已先记录五问，再按明确COVER角色转要求为现有全局编制说明；8C关联/全部
  ID校验/未知待核/普通同名section保持，原cover form不改，COVER不假报缺章内要求。
  有效v17基线3红/3绿，修复后128相关通过；含实际DOCX前目录纯原式、ZIP notes原文
  完整及混合企业章要求保全。最初缺snapshot字段的测试日志保留，不冒充业务红。
- v18已冻结并行构建/unit/PG58，Prompt d548、Schema/spec/模型payload未变。构建及
  PG58已通过，fullunit与source-bound COVER最终独立复验待齐；部署硬gate未打开。
  current门检预期8b03/661b907…且无editor/pending，只升级独立四Python角色。
- 新v11 state已准备，目录前驱为成功v10，预期修订父a94c25d5，Word前驱91a6，
  尚未启动新模型或任务。正式新候选及新Word/Office保存、同版PDF和三个分册仍待验收。

证据根不变。代码/测试 `bug159-cover-requirements/`，实物反例
`independent-semantic-review/v10-cover-review/`，候选 `deployment-v18/`。
整体无企业资料模板验收未关闭；不提交/暂存/推送，保留共享工作区其它改动。

## v15 / v9 整框表头语义后继（2026-11-16，历史快照）

独立栈已部署 `kb-docreader:review-20261116-directory-integration-v15`，镜像
`sha256:cbb32ccc071ccc23e94d9fb0312d9ce24659ab69ee152e26972b1b1ee9618dba`。
**2688个唯一unit通过，0失败/错误/跳过，281.26秒；真实PG58通过，0失败/错误/跳过，
57.49秒**。410源码前后与包内逐SHA一致，相对v14仅材料Prompt新增一段，SHA
`d548a9d409ecc3c3968fef0f40dd77363489a4ca4a11f0e8736de39b31d1cdb3`。
Python逻辑/Schema/spec均未变，init0及三个持续Python角色healthy；Office/API/旧栈保护。

- 新规则要求重复表头适用于整个grid；隔有数据/填写行的不同分块表头不能把首组当
  整框公共头，保留真正公共起始头、连续多级公共头及0/null出口。不加词面或行相似度
  启发式；Prompt独立复读无Required，但仍须看真实回包，不以写了规则授予通过。
- v8最终候选SHA `e7aea0edda1e8f7eff6dacd3a778683c4912654b9be56b4976cfcd338c243ab3`
  已完成37源门，除8E-1错误count1外无新Required；BUG-157修复正向确认，原式59/
  叙述169/附证3均manual，35参数完整且前向无重叠。该候选未采用、未生成Word。
- v9目录 `web-outline-5dc4f18e2cee1a23daa4b01cada4edc807fc9c24223227b7235be1787b57693a`
  已通过API启动，显式前驱为成功v8，源/来源版本不变，实际摘要一致且重验有效才复用。
  还没有采用、成稿或最终验收结论；当前Word仍为历史v6 manual e5f3版本，历史字节保留。
- v9后续succeeded：13逻辑仅1材料真发，12按精确摘要重验复用，0重试/未知；83050输入/
  453输出/83503总Token。23源表头回到正确0×5/1×15/2×3，27实例0×5/1×19/2×3，
  37源义门全部通过。候选SHA `bebb1938ff87b6424ff5fd945c999714775144be1433450658e86e9f178671fa`。
- 继续一致性对照发现BUG-158：五章25个要求槽位只重排、内容未增减，由并行one按完成
  时序立即apply造成。已记录后以事件先2例失败，改为并行收计划、按blocks顺序应用，
  分类也固定相位应用；保留sem2，中块/分类失败均有原块号出口。相关142通过，最终7
  integration/调度组合通过；真实13回执两返回顺序完整canonical相同。
- 跨hash seed又定位仅六条conflict顺序来自_choose的shared set，章节/目标/表格不变。
  已补一行sorted(text_diffs)，源键顺序回归在seed33先失败后修复。v16中间快照unit2692/
  PG58均通过且source-after无变，但不含该最后一行，保留不部署。新v17准备，部署前
  需真实13回执在3种hash seed及两返回时序完整canonical一致。v10 state已建，前驱为
  成功v9，尚未发新任务；目前没有采用新候选、没有生成新Word，整体验收尚未关闭。
- v17随后正式部署：2693唯一unit全过/0skip、PG58全过/0skip，410源码与包内逐SHA一致；
  独立6次真实13回执全canonical同SHA `4e889e7799314b998c99ac286ca2550caa0c631af05e5816244f4dc7f676cb03`。
  v10正式目录 `web-outline-9995fcb3f2abca6669762c6324ccd2f7eea8640d53ebf2cbd200995dd89d2d71`
  succeeded，0真实模型发送/13精确摘要重验复用，实际候选严格匹配该golden。
  固定修订 `a94c25d5-4a4c-44f4-b09a-6919fbc41cb9` 已采用。
- v10 Word执行 `web-document-91a6deaff5f46385eccd321c08830aa6e3b5813f713723090c5cf44e5520569f`
  succeeded，1结构请求/0retry/0unknown，64872输入/1367输出/66239总Token。新生成版
  `8b03a257-c07c-4a4c-a2de-15e9984d8f51` rev5已按API显式选中，SHA
  `661b907a1950f287f166acad1ef174065d1220da275a9cf6981e6ae4d27350a6`、107885bytes；
  原ZIP SHA `e59185c4772b15a7dafd26ea08dc2733db9c09965030962edc42dbc130063343`。
  raw下载/ZIP主稿/CAS/DB一致，1606段/27表/569空格；authoritative提取完成。旧manual
  e5f3实际下载及SHA保持；未打开编辑、更新目录或保存该新稿。
- 实际Word/ZIP审查确认BUG-159：源评分要求被分配cover+8C，真实封面/目录前出现评分
  原文及应答空位。37源门只查原封面存在，漏检额外污染，故未授予Word通过；平台hold。
  已先记录并按明确COVER角色把要求转全局编制依据、保8C与所有ID校验，原form不改。
  初测试fixture合同已纠正，真实v17基线用有效tests红=3失败3通过，当前128相关通过
  （含实际DOCX前目录无评分/应答、原封面字段及ZIPnotes原文）。新v18准备/复核中，
  新v11 state前驱为成功v10，预期修订父为a94c25d5、Word前驱91a6，尚未启动。

证据根不变，新轮 `online-template-v9/`，冻结/部署 `deployment-v15/`，独占PG
`bug149-tests/persistence-v15-platform-20261116/`；原件复核 `independent-semantic-review/v8-final-review/`。
整体无企业资料模板验收仍未关闭，不提交/暂存/推送。

## v14 / v8 条件保全后继验收（2026-11-16，历史快照）

独立栈Python已部署 `kb-docreader:review-20261116-directory-integration-v14`，镜像ID
`sha256:a3f1cbed4471016886d1d7b3a758e66a489a6e5951335a0b54608dd23f5219bc`。
**2688个唯一unit通过，0失败/错误/跳过，282.96秒；真实PG58通过，0失败/错误/跳过，
58.77秒**。410源码前后及包内逐SHA一致；init退出0、三个持续Python角色healthy，
只切独立Compose四image字段，Office/API及旧栈容器ID不变。测试PG/凭据已按身份清理。
证据为 `deployment-v14/`、`bug149-tests/persistence-v14-platform-20261116/`。

- BUG-157新增9反例、独立23相关回归及真实两计划离线回放通过；52章位置/正文/原表/
  主体均不变，只补原厂授权的明确源条件，附件10不同条件保先值且完整具名待核。
  Prompt/Schema/spec不变，不改旧回执。BUG-156源字体投影、683行几何owner保全及
  BUG-152的caption/8E新页规则仍在同一冻结候选。
- v7已成功但未采用、未出Word；最终API/CAS候选SHA
  `cfa6259a285148dfcd00b5b7b95a5f021a177c706ad53f8afbce8509d727fe6e`。
  独立37源范围审查除BUG-157外无新增Required；27原表、双财务角色、165叙述/3附证
  填写出口及35参数（8通用+27硬件）均完整，六评审表排除，跨页完整且同父无重叠。
- 新v8目录 `web-outline-316cf53f67c80e54216af5550b31614c8475276c9bf6feb054daba81cf35cade`
  已由API启动，显式前驱为成功v7；同一106页原件/source-only run、source_version与
  domain_revision均1。实际摘要相同且重验有效才复用，条件上下文变化的材料/要求真发。
- 当前Word仍为历史v6 manual `e5f3f789-5715-4c60-9284-06e57cb623f7`，SHA
  `561d1cc2bcd2d25eaea42377b86d4b92d63dcbe479971847a92f625b69402eff`。
  未用新候选覆盖该稿，所有历史生成包不变。v8的最终采用、真实成稿、Office保存/同版
  PDF及三个分册实物审计仍待完成，**整体模板验收尚未关闭**。
- v8后续已succeeded：13逻辑单元中8真实发送、5按精确摘要复用，0重试/未知，
  204322输入/10414输出/214736总Token。auth条件及附件10双来源具名待核已在新执行
  正向确认。但新材料回包把8E-1两组不同列名原框误判count1（原v7为0），源网格/
  模型可见输入完整、原始response本身错误；此候选不采用、不生成Word。
- 已先在BUG-152记录新反例与五问。下一候选v15仅补材料Prompt的整grid适用边界，
  区分隔有数据/填写行的不同分块标题与连续多级公共表头；0/null出口保留，不引入
  词面或行相似度启发式。v9 state准备，前驱为成功v8，尚未发模型；正式新发/复用
  仍按实际摘要与回执重验，不因预期只改1调用而降低准入。

证据根不变，新轮 `online-template-v8/`；独立源审查 `independent-semantic-review/v7-final-review/`，
条件回放 `bug157-review/`。不提交/暂存/推送，保持共享工作区修改。

## v13 与 v7 后继真实验收（2026-11-16，历史快照）

无企业资料模板目标不变，尚未授予整体通过。独立栈已部署
`kb-docreader:review-20261116-directory-integration-v13`，镜像
`sha256:fba3850f7ee2b594c6550163d85fad196daa5774ce152795400866311c91c8f2`。
410个冻结源码文件前后及包内逐SHA一致，**2679个唯一unit通过/0失败/错误/跳过**，
300.72秒；真实PG **58通过/0失败/错误/跳过**，60.12秒。init实际退出0、持续三Python
角色healthy；只改独立Compose四Python image字段，Office、API及旧栈容器身份不变。
证据 `deployment-v13/` 与 `bug149-tests/persistence-v13-platform-20261116/`。

- BUG-156最终先绑定原版式几何，再作有源字形投影：真实原件106页的行号/字符位置不变，
  16处F052→☑、1处F0A3→□，683条native表行owner全部保留，原PDF/raw/index不覆盖。
  未证实/冲突/重复/计数或类型异常均保原字，具名待核有Word人工处理出口。
- BUG-152原式说明/主体接到后继原表；8E-1两组列名不能重复统一首头，额外keepNext
  实验无效不入产品。本章起始纯源form文字→明确count0原表才给既有heading新页，
  独立4反例通过且真实v6仅8E命中，隔离实物无新增空白页。新项目实物仍待复验。
- v7新目录 `web-outline-be847cac72c4d7e714081a60cec306568f4756cc6ea73be145abe0a3e97e869f`
  已通过API启动，沿同一source-only run、106页原件、source_version/domain_revision1，
  显式目录前驱为v6；合同 `source_roles_continuations_headers_symbols_v6`。真实已发送
  第一slot的持久model-request/CAS已读核：payload私用字0，☑16、□18；不是仅从代码
  重建Prompt。摘要变化请求真实重发，相同且重验有效的后续才能复用。
- 当前主稿仍为v6的manual版本 `e5f3f789-5715-4c60-9284-06e57cb623f7`，SHA
  `561d1cc2bcd2d25eaea42377b86d4b92d63dcbe479971847a92f625b69402eff`，旧生成稿/ZIP
  不变且无编辑或待保存。v7尚未采用/成稿，新模型可见来源正确仍不等于模板最终验收。
- v7后续已succeeded：13逻辑单元/13真实发送/0重试/0未知，450108输入、33407输出、
  483515总Token。三分册/必需原表及评分表排除前置通过，但独立原件复核确认BUG-157：
  两槽都有原厂授权条件，祖先subtree先收子节点后seen去重丢了明确条件；候选不采用。
- BUG-157先记录后新增6反例（5失败/1通过），宿主仅补空条件、保先非空值并明列冲突，
  新clause校验后只待核保全不自动合义务；116相关通过，补齐无选择不清除/同值幂等/
  已承载clause不造冲突后15条件回归通过。新v14快照准备，尚未部署；新v8 state已建，
  前驱为成功v7，正式请求只按实际摘要复用，条件上下文改变的材料/要求需真实新发。

证据根仍为 `/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/`，
新轮 `online-template-v7/`。源字形/owner复核 `independent-semantic-review/bug156-*`，
分页实验 `credit-grid-isolated-pagination/`。没有提交、暂存或推送代码。

## v11 / v6 与真实 Office 保存版验收（2026-11-16，历史快照）

本轮仍只验收无企业资料模板，用户已授权持续验证修复。以下旧节保留对应历史快照，
不作为当前运行状态；最终整体结论需同时覆盖实际保存主稿及三个独立分册。

- 独立 Compose 项目 `kb-isolated` 的11个持续服务健康。Python四角色为
  `kb-docreader:review-20261116-directory-integration-v11`，镜像ID
  `sha256:317799b57f92f7b8628dec58dda99d83b28bf480adf0ebb9cc6d7d7a1a4109ea`；
  Rust/Web仍为 `kb-isolated:runtime-20261116`。没有操作属于zhaobiao的同名旧栈。
- v11完整unit **2652通过，0失败/错误/跳过**，411个冻结输入无漂移；独立真实PG
  **58通过，0失败/错误/跳过，54.06秒**，测试数据库/secret已按身份清理。证据分别在
  `deployment-v11/`、`bug149-tests/persistence-v11-platform-20261116/`。
- Office已持久切换 `kb-onlyoffice:review-20261116-cjk-v1`，镜像ID
  `sha256:fd7dd88841c1b79e5746b0b9783136d1ca9a61814563638f3b3d4e1ebb3e358c`。
  仓库 `deploy/Dockerfile.onlyoffice` 固定官方基础镜像及既有WQY字体指纹，统一注册和
  字体缓存；字体修复本身没有改变来源Word字节。副作用试验只在隔离容器执行。
- v6仍为同一106页 `BiddingFile.pdf`，源SHA
  `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`，
  source_version/domain_revision均为1，分析未自动确认。目录执行
  `web-outline-f930f18beb93212185f5621d84451df1a38963facc0a70ba43ddc9d4c1331cbd`
  succeeded，13逻辑单元只有材料1次真实发送，12次按摘要重验复用，0重试/未知用量；
  82978输入/453输出/83431总Token。23个唯一源表头判断全部显式，27实例分布
  `{0:5,1:19,2:3}`，原正文/要求/主体/条件/参数与v5一致，独立来源表头门检通过。
- 固定目录修订 `aae5d94b-dd9c-4c6e-8338-d158d8e513ff`，SHA
  `a7e02953c815c897199ccca4a9f2f21a3334950686b9c70e1b78b27ae9c12e81`。
  Word执行 `web-document-65c529f53cdb5fd4bf2d6a1d1d6110710b20e3353d4040756f213fce33fafa25`
  succeeded，仅1次authoring_structure，64852输入/1159输出/66011总Token；无企业检索、
  authoring_write、企业填值或实际附证。严格审计240任务/180宿主空白及12负对照通过。
- 原生成保存版本 `ab967837-84c5-435b-8426-18d25c2aeb61`，Word SHA
  `ec5d6cad4c5ea262a7944628f8e19227d26e6b0a9201ef10318592df11208ccb`，
  原ZIP SHA `555da112912a36cdf5c12340fd1ddac6dd24af2be0820bbcb345781708ba5c39`。
  原生成稿、ZIP、execution result及其不可变引用保留，不用手动保存版覆盖。
- 已通过真实网页“引用→更新整个目录”及应用“保存修改”，核验Office callback、
  `tender_draft_saves.saved`、下载字节及父子关系。当前manual保存版本
  `e5f3f789-5715-4c60-9284-06e57cb623f7`，revision4，115348字节，SHA
  `561d1cc2bcd2d25eaea42377b86d4b92d63dcbe479971847a92f625b69402eff`。
  自身编辑/查看会话已关闭，父版字节保持。独立bindings及保存链分别为
  `online-template-v6/office-saved-bindings.json`、`office-saved-chain.json`。
- 独立核验manual版240目标书签及非表来源正文、27表文本/空白/横纵合并/头标、
  176响应空白、4待附位置及具名条件未变；目录“待计算”为0。**同保存版PDF视觉、
  真实页码与三个独立卷目录正在验收，整体尚未关闭。** 原ZIP的分册缓存不能借主稿
  手动更新判通过；逐卷在隔离字体缓存容器刷新，另留来源及字节绑定，最终交付包另名。
- 原件只有整稿共用封面依据，没有明确逐卷复制封面的要求，不增设无来源阻断。
- 继续实物检查确认主稿2A的表名/表号/币种单位与首次表格完全分离，独立商务卷另有
  8D-4表名落前页。故没有给v6整体通过。已先补BUG-152并以主稿/分册及narrative/跨章
  反例建立回归（1失败/2通过），窄修同章原式文字→后继原表的keep-with-next，原表主体
  标签接续、数据行不整体粘连；相关80通过，追加主体断言后的3定向通过。Prompt/
  企业空白不改，新v7目录/成稿准备中，尚未启动新模型任务。混合注释与
  下一表名共处的8D/8E/8I源段须特别实看，不把宿主分页约束宣传为caption语义识别。
- BUG-156已确认目录版式文字绕过冻结的源字体证明；独立全原件清点17处来源私用符号
  均有有效嵌入字体证据。新增逐页等义投影、缺证据/混合含义/重复证据/计数与类型异常
  保原字及具名出口；真实联动审查又发现先投影会丢17条几何表绑定，已先记录并改为
  原版式完成owner绑定后投影，保持模型可见正确符号及原行坐标。新目录合同为
  `source_roles_continuations_headers_symbols_v6`，新请求只按实际摘要复用。
- 中间候选v12未部署：构建及410文件指纹通过，完整unit2675通过/零跳过；真实PG
  57通过/1失败（旧case仍断言v5协议），失败和已清理独占PG的证据保留。随后已精准
  更新该协议断言并修owner顺序，须在新v13快照重验，不能借v12绿测宣布后续通过。
  离线caption预检确认2A/8D-4已连接，但8E-1仍有列头留页；两种额外keepNext实验均
  无效，未放入产品。正在以外部副本验证明确分页边界，旧在线主稿/原ZIP保持不变。

证据根为 `/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/`。
当前实物 `online-template-v6/`，字体 `office-cjk-fix/`，独立审计
`independent-semantic-review/`。没有提交、暂存或推送，保留共享工作区既有修改。

## v10冻结、新目录v5与原件逐项验收（2026-11-16，历史快照）

目标仍是无企业资料模板，用户要求持续验证修复直到实际验收。下方历史v7/v8、未生成Word
等描述只保留对应快照含义。当前独立环境Python四角色已切不可变v10，Rust/Web保持本轮
runtime；init退出0，11个持续服务健康，没有操作既有zhaobiao栈或提交/暂存/推送代码。

- v4目录 `web-outline-16bc16212016c97c9a4770befff8ee0eed43c10bef199d788e0bb5056f91eba0`
  技术succeeded，13次发送、1重试、0未知用量，453901输入/38904输出/492805总Token。
  两槽、1材料、7要求、1分类、2参数；三分册/原网格/五技术表阶段检查通过。
  最终候选SHA `1a9cc94c2d6e5568b254a3eeb47558ad41d76c4098066de2296276196bc85b81`，
  未采用、未生成项目Word。独立审查的4个真实阻断为：附件7条件无具名待核、跨槽8H重复、
  原封面漏选、单页参数协议使跨页条款拆残句/交叠。不是只剩一个condition问题。
- BUG-151补齐条件/补充义务差异、合并后精确源重叠保护及最终采用章的条件issue；
  不裁正文，不按相似度删不同来源/实例，不把被挡父章后代挂到消失父章。
  条件片102相关通过、跨槽片110唯一相关通过；最后两处有效审查反例2失败→26相关通过，
  独立定版24通过。参数已校验项由宿主按注册来源位置排序，不为机械顺序新增模型纠正。
- BUG-153把已有cover职责接入目录，保全来源封面空白并置于目录前，2例失败→相关86通过；
  BUG-154增加同参数continuation_windows，跨页条件/证据一目标，成功回执前拒非法来源/
  逆序续窗/同父行重叠，11新回归及相关115通过。新执行合同为
  `source_roles_and_continuations_v4`，旧执行和Word不热改。
- BUG-152保留实际章节/书签的可点击目录缓存，不虚填页码；原PDF统一字号/行距/内边距
  只在原几何可核时恢复，表首行不孤页，不猜重复表头。相关82通过，离线27表逐格文字及
  空白不变。Office CLI能力实验中文方框负证据保留，不引入生产；最终视觉仍走保存同版
  converter，不把离线预览当项目保存版本。
- v9冻结回归2622通过、58真实PG通过后，独立审查又发现跨item逆序和第二槽condition
  issue丢失，故没有部署v9或冒称其已覆盖最后修复。v10最终**2624个唯一unit全部通过**，
  0失败/错误/跳过，297.03秒；408构建输入前后无漂移，Ruff/diff通过。v10独立真实PG
  **58通过 / 0失败/错误/跳过，58.62秒**；410文件前后无漂移，408部署清单匹配，测试
  schema清净，自建PG及secret文件已核身份删除，不触碰真实独立栈。
- 当前v10镜像 `kb-docreader:review-20261116-directory-integration-v10`，ID
  `sha256:02ae0312167b379070b98d8a60a2cc0af90c0a852274a928dc62987598331be9`。
  新真实目录 `web-outline-2f6509b054020fc7a5b08f56e9e6fe18a0b582459b1320b7c6e29a5cbda53dee`
  已完成，仍同一106页原件/run/source_version1/domain_revision1；15次发送/3纠正/0未知，
  实际626183输入/62457输出/688640总Token，13逻辑单元。候选经37项原件完整门检，
  固定修订 `77125d1c-3220-40f1-8eed-d42283dfc2a7`，SHA
  `a90cee037ef1c09d1ad8edbbf2056fd0dc88c76d5c87bbe8a4c033799c5a3913`。
- 真实Word执行 `web-document-f2f08ce8c0afbf2a9c2ded0a2fa084d8f3bc9aa7bf669b59a095cd7e94417364`
  succeeded，仅1次authoring_structure，64852输入/1452输出/66304总Token，0重试/未知；
  没有authoring_write、真实知识材料、cell_fills/row_fills或实际企业附件。保存版本
  `648d99e3-e167-4536-a542-2c73d7c4adf5`，Word SHA
  `67d5ac1e32415627e8a5278c75970f8c6f7d979770311393576b078fd816e1d7`，103708字节；
  ZIP SHA `77ccfe7a3a60a0d83da4556a22175b8be48d638470449384c484fd2c5af14e3b`。
  1644段/27表/569空格，240目标均manual；176叙述/4附件有实际宿主空白，90目录链接
  全部指向真实书签。新稿在确认旧稿SHA及无编辑/待保存后通过既有API显式切为current，
  旧v3 Word下载字节未变，回执`explicit-word-selection.json`。
- v5权威CAS/数据库/下载Word绑定一致；27表/23唯一源网格、104原文锚点、全部必需原式
  及双主体财务各四表审计通过，无无据重复。严格无企业资料审计240任务/240单元/180个
  逐字宿主占位通过，12负对照全拒绝；初版“非空blocks即企业生成”误报原报告保留，
  更正后报告为`download-audit-final-technical.json`。独立实际Word37项/240目标语义与
  出口审查通过；不据此授予最终视觉/业务通过。
- 同版converter已转76页PDF，自建view会话关闭。但真实PNG中文普遍为方框，两个独立
  审查者确认，**视觉验收失败，整体模板仍未关闭**。当前Office fc-list无可用CJK，PDF
  未嵌CJK；Word XML中文及字体声明完整，与历史配置一致，历史v3 PDF字体来源未定。
  BUG-155已先记录，正以独立不可变Office字体候选持久修复，不改Word字节/旧栈。修复后
  固定同Word版本重转，必须重验表头/标题分页、签章/粘贴留白和全稿可读；不能以字体安装
  或转换成功替代视觉通过。

证据根不变。新轮`online-template-v5/`；冻结构建/完整unit在`deployment-v10/`，PG在
`bug149-tests/persistence-v10-platform-20261116/`；独立缺口与代码复核在
`independent-semantic-review/`，修复反例分别在`bug151-condition-merge/`、
`bug151-cross-slot-ownership/`、`bug152-tests/`、`bug153-tests/`、
`bug154-parameter-continuations/`和`v9-review-regressions/`。实际模板业务验收尚未关闭。

## 真实Word语义查漏与BUG-151（2026-11-16，持续验收中）

用户要求持续验证并修复直至验收，当前目标与独立环境不变。下列为本轮实际身份和结果，
不把旧节“尚未生成Word”或“v7回归进行中”当作当前状态。

- v7完整unit **2568 passed / 0 failed / 0 skipped**。独立PG的58项属于此前v6快照，
  不能混称v7持久化全量。Python四角色为独立v7标签，Rust/Web保持本轮runtime标签。
- v7目录 `web-outline-3307772b8ea7990459aef047df886f9dafdc048f6db679ddb19f078b02d79685`
  succeeded，19次真实发送、1重试、0未知用量，输入481055/输出40311/总521366 Token。
  两计划、1材料、7要求、1分类、8参数。固定修订 `9eb10b99-09ec-4d53-b84f-68d956f0d587`，
  SHA `b6d499d8b3df92a62773a7bad2cc6aa42724406553720c1ef3f6e60cbaacb74d`。
- 真实成稿 `web-document-4fadd86cc797499c22f5cba9ce2143c0e15658b82b363790fdeec023b391b2e5`
  succeeded，仅1次authoring_structure，输入71937/输出1464/总73401 Token；无企业资料、
  无basis/write调用，分析未自动确认。实际保存版本 `9c35dbe5-db28-473d-bbb6-a3ed8d9d0390`。
- 下载Word SHA `97357ad5e934288cfadbfc24dcbed98c62484876bcee605f22824c5834505285`，
  ZIP SHA `bf5099e300b25ab4518d3c3dad36dc01cd03ba837d6c36e6e01b948fc51e8146`；1479段、27表、
  571空格。CAS result/outline/frozen、DB保存版本与下载字节一致，adopted-outline与最终
  权威outline逐对象相同。已选择表格保全、三分册及5技术原表归属阶段核对通过。
- 独立原件审查37项范围，形成9类开放语义缺口：附件7整体误排、专项授权空叶、限价
  仅在issues、财务双主体未分别承载、B不适用借A.2泄漏、技术通用/专用结构顺序、技术逐段
  应答/附证空位、原函与资审表同实例重复、来源未决未计待核。故实际模板**尚未验收通过**。
  质保函填写出口、身份证粘贴文字、报价80/13%固定及四类企业报价空白已正向确认。
- BUG-151已先记问题与五问方案，正在以失败回归修复：沿用现有目标kind及subject表达
  职责/主体，适用性依据进入现有分块请求，真实未决保留人工核定栏及计数。旧执行和旧Word
  不修改；后继online-template-v4已准备身份，尚未发送新模型任务。部署/真实复验待完成。

证据根仍为 `/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/`。
当前实物与权威绑定在 `online-template-v3/`，独立源文审查在 `independent-semantic-review/`，
修前源码在 `bug151-baseline/`、失败/成功回归在 `bug151-tests/`。下载审计初版把81个纯人工
占位块误作企业生成，外部审计器正在按真实计划/目标纠正；不据此否定或放行实际语义缺口。
同版ONLYOFFICE PDF排版验证进行中。没有提交、暂存或推送；共享已有修改继续保留。

### BUG-151候选v8及同版排版后续

- 新增职责/主体/真实Word出口18项回归先失败后通过，相关8文件110项通过；root待核出口
  相关59项通过，最终名称/引用失败/孤立章节三种未决分支另有10项定向通过及独立5项复核。
  首次完整unit2592通过/1失败：测试进程在最后两行修改前已载入旧helper，随后测试断言
  已更新，不能记全绿。新进程最终源码完整回归 **2593 passed / 0 failed / 0 skipped，306.02秒**；
  2593个唯一测试身份已核对，401源码/测试前后指纹及v8的402构建输入均无漂移；Ruff/diff通过。
  总结为`bug151-tests/summary-final.json`，不降低断言或删除用例。
- v8镜像 `kb-docreader:review-20261116-directory-integration-v8`，镜像ID
  `sha256:af8e0ba5bee501e2c8172e8a53e584418059bdf3fa3de8f6f5417de7be896e14`，
  402个构建输入指纹无漂移。只更新本轮独立栈四个Python角色，init退出0，三持续角色健康。
  切换前独立authoring_jobs在途为0；Rust/Web/数据/原旧栈不操作。
- v4显式后继目录任务
  `web-outline-16bc16212016c97c9a4770befff8ee0eed43c10bef199d788e0bb5056f91eba0`
  已真实提交，继续同一run/source_version1/domain_revision1，未修改旧固定修订或Word。
  目录合同为`explicit_response_instances_v3`，当前回执/语义与实物待验，不能称已完成。
- v3严格无企业资料审计用权威plan逐目标构造宿主占位，297任务/297单元/81空白块通过，
  12个反向对照全部拒绝。旧初版generated_units误报保留，不把纯宿主空位当企业事实。
  原件基准仅增加3个原文明示分组的精确标题行别名，104锚点通过；27所选表/22必需原网格
  保全通过，但实际附件7缺失仍使总审计失败，不因此授予业务通过。
- 同版Word转换PDF成功，77页，自建只读会话已关闭。视觉查明目录仅更新提示、报价/财务
  表头孤页及续页无表头，报价窄列二次折行；已先记录BUG-152，由独立排版修复片继续。
  Word只保全原文字形/网格不代表排版可用，历史v3实际稿仍未验收。

## 持续模板验收与BUG-149/150修复（2026-11-16）

用户已明确要求持续边验证边修复直至实际验收，本轮目标仍是同一106页BiddingFile的
无企业资料模板，不填企业事实/报价、不自动确认分析；独立环境保持运行，不操作旧栈。

- 原文独立基准已建立：3逻辑分册、21个待填/提交原式网格及1个引用型原式网格、1个
  可选网格，6张评审工作表不作投标原式、5张技术表的正确归属及实例、10组章节要求、
  价格空位/80数量/13%固定说明。基准分母来自原件，99处源锚点有页/行/引文SHA，
  不从模型已选结果倒推。A.1仅按原式框/固定参见保全判，不称需要独立重复填数据。
- BUG-149显式body_windows必填、空窗无正文、坐标/结构在成功回执前编译校验；分组及
  真实来源参与两计划比较，超集不能覆盖不同正文却声称保留第一份。新增失败回归先红
  后绿，2/3/4组及非固定名称的真实pipeline→固定修订→Word/ZIP纵切通过。
  v6完整unit **2565 passed / 0 failed / 0 skipped**；独立PG持久化4文件 **58 passed**，
  同轮临时PG/随机端口/tmpfs和fixture schema均清理。实际3.11在线运行另列，不能混为一套。
- v6新执行 `web-outline-9dde942f8e22d8135dc00cf3d1e44383409cf63bc9c2b09b00aa6a96ccc2fd73`
  真实结束succeeded：2计划、1材料、7要求、1复用分类、7参数。19次发送、2重试，
  已知输入390419/输出32043/总422462 Token，另2次未知用量不计零。
  原文明示分册、禁评审表、五技术表唯一归属阶段核对通过；A.1仍以clause纯文字承载，
  原框未保全，故未采纳该候选，没有发布为合格Word。
- BUG-150已登记：数据可“详见分项报价表”不代表删除原件引用型原式。通用Prompt已区分
  数据复用与原式保留；显式正文节点的clause按同源坐标求差集，不按文字删不同来源/实例。
  新回归1失败/2通过→3通过，和受影响目录/范围/纵切合跑85通过。v7全量回归进行中。

证据继续在 `/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/`，
`acceptance-baseline.json` 与 `acceptance_baseline_checks.py` 只用于独立验收，不硬编码进
生产。旧样稿negative审计证明“所选原表保真”与“满足完整模板要求”分别判定；旧报告及
基准修正前快照保留。当前未达业务验收，后续只对新真实Word继续核原框、要求、版本及版式。

Python v7候选从已验证依赖层只更新源码/Prompt，独立新tag，不热改v6原执行；计划用同一
原件/项目/冻结快照、显式后继execution重验。两槽目录因提示修订重跑，其余只复用实际
请求摘要完全一致的成功回包，不承诺全部复用。当前没有新的原件解析或企业资料调用。


## 目录固定修订、镜像兼容与独立本机环境（2026-11-16）

本轮用户先同意推进目录衔接、真实验证和部署，随后明确本轮只做**无企业资料模板验收**，
并在本机另建独立 KnowledgeBrain 环境。企业资料检索/应答验收不在本轮关闭，222 留出样本未动。

- BUG-145：两份目录计划使用固定槽位 1/2，真实请求和回执独立，同槽恢复仍精确复用；
  第二份增量章的局部 ID 及父引用隔离，避免合法计划合并后丢章或重复章。冻结来源的大纲
  执行 spec 增加 `independent_plan_slots_v1`，旧执行不能原地切换合同。
- 接线复核确认网页已经支持“目录候选→固定修订→正文/模板→Word”；没有直接移除
  `outline_only` 条件，保留自动组织与知识写作职责。3 项集成用例覆盖原文/原表、人工调序、
  显式 knowledge 写作目标及空目录待核出口，模型为替身。
- BUG-146：首次完整单元回归 2513 passed / 8 failed，改前快照精确复跑这 8 项同样失败。
  将旧自动组织用例移到现存完整编制入口，预算列表补齐五种目录任务；82 条业务断言 AST
  不变。修订后 3 文件 48 passed，随后完整 2521 passed。
- 实际 Python 3.11 候选暴露 BUG-147 字符串枚举包含判断和 BUG-148 系统工具缺失；分别
  最小修复角色值比较、正式 DocReader Dockerfile 的 Poppler/Tesseract 与中英文语言包。
  最终源码完整单元回归 **2536 passed / 0 failed / 0 skipped**；另有隔离 PostgreSQL 的
  持久回放、旧合同拒收、编制计划/单元恢复与知识操作测试 **58 passed**。后者在角色修复前
  完成，角色修复另有真实 3.11 的 14 场景及相关源码 49 项定向验证，不混称同一快照全量。
- 三份真实 PDF 用旧模型回包离线回放，共五稿；Python 3.11.18、Poppler 22.12.0、
  Tesseract 5.3.0 的 v3 镜像中逐章原文、已选原表网格/合并/字形检查通过。
  与宿主 Poppler 25.03.0 的五份 outline SHA 及五份 DOCX 全部解压条目 SHA 一致。
  选中表为 111 每稿 19、BiddingFile 21、安服 16；原生总表仍为 41/41/42，不能把
  “选中表保真”称为全部表格发现或全文件语义通过。BiddingFile 物理 37—42 页六表是
  评审/评分依据，不是投标人应填交原式；原计划 tech 范围过宽仍待核，要求/评分交接另验。
  初版审计器误丢带书签首段的失败证据保留，纠正后仍逐章比对，不放宽为整稿搜索。

证据根 `/opt/workspace/kb-tender-evidence/2026-11-16-directory-integration/`。本轮相对开始
快照的代码/测试基线、失败和成功报告均保留；没有提交、推送或混入已有工作区修改。

### 独立部署与本轮在线验证

当前原 `knowledgebrain-*` 栈属于 `/opt/workspace/zhaobiao/deploy`，不是本仓库 Python
招标运行栈；先前健康观察只能说明该栈可达。本轮不重启或复用其容器、数据和 local 标签。
新项目为 `kb-isolated`，容器前缀 `kb_isolated_`，独立 PG/Redis/MinIO/对象和 Python
产物卷，知识库关闭，Office 单独部署，环回入口 `https://127.0.0.1:28080/`，Office 为
`https://127.0.0.1:28081/`。配置、管理脚本及登录信息仅在仓库外受限部署目录维护。

Rust/Web 从当前仓库冷构建 `kb-isolated:runtime-20261116`，包含 API、worker、retention、
migrator 和静态网页；构建限制 jobs=4，303 个输入前后指纹一致。Python 四角色使用
`kb-docreader:review-20261116-directory-integration-v3`。实际镜像、依赖版本、启动/退出
及旧栈保全检查见证据 `deployment/`、`deployment-v3/`。前两份失败候选和报告保留。
新栈 11 个持续服务均已健康，migrate/init 正常退出；这只证明部署技术链可用。

登录脚本最初把 Compose 配置序列化的美元符号转义当作原口令，401 发生在建项目之前；
修正测试脚本读取方式后成功登录。账户和密码未重置，Compose 配置保留。

新项目 `2e41e3da-d217-5d5b-8690-9d08ab4b2506` 已真实登录并上传同一份 106 页
BiddingFile.pdf，SHA `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
实际部署源快照为 3622 行、41 个原生表、7 个要求块；两槽最终目录请求各估算输入 70669、
输出预留 32768、安全 4096，总 107533/131072，均准入。使用当前 `.env` 的
`gpt-6.1-terra`、`https://code.zleiwork.cn/v1`，不打印或复制密钥到仓库。

在线目录执行 `web-outline-ebf7d3a0e3adeab5f0f1f933e10617143558cc9a8d51a7333a2e2cdf60b5cc84`
已技术结束为 `succeeded`：2 目录、1 材料、7 要求、1 分类、7 参数，共 18 次物理发送，
0 重试、0 未知用量，实际输入 482789、输出 31912、合计 514701 Token。阶段评审发现
BUG-149 后请求取消，但提交取消时模型工作已全部结束，不能把技术终态改称 cancelled。
当前 0 在途；没有采用候选或发布项目 Word，原分析未自动确认。

最终候选共 215 章、10 个顶层、0 分册；从原组成说明扩出的技术章误带六张评审工作表，
另有五张技术表重复。为核对实际导出后果，使用同一最终候选和正式导出器零模型生成
`online-template/验收未通过-目录候选样稿.docx`，未发布至项目；实际 32 张 Word 表对应
27 个唯一来源，六张评审表和五项重复均仍存在。报告为 `final-diagnostic-review.json`。
**本轮模板业务验收未通过**，不能由 2536 项技术测试、18 次正常请求或正常导出替代。

用户随后明确最终目标是投标文件满足招标要求，要求分别保证稳定、正确、一致和高效。
本轮据此收敛验收原则：分母来自全部适用原件/补遗，逐项检查“原文及条件→适用性→
响应位置→同版本实际 Word/证明→明确结论”，所选表保真不证明没有漏选；人工填写/
无需内容复核不代表义务已满足。业务缺口仍可保存下载，影响的是验收结论。来源计划
正文边界的下一步设计仍在复核，未用新增循环或正则试错修改运行合同。

部署技术核对通过：11 持续服务健康、migrate/init 退出 0，网页/ready、实际 JS/CSS、
Office healthcheck 均 200，运行 Python 文件 SHA 匹配 v3。新旧网络/数据卷不相交。
旧栈保全观察必须分开报告：旧 API/worker/retention 在 12:16Z 的 ID、image 和启动时间
发生变化，仍属 zhaobiao 工作目录，原因未确认；其余九容器身份未变。本轮构建标签、
Compose project 与新容器镜像均不与旧三者交叉，不能因此宣称旧十二容器全部未变。
具体安全报告见 `deployment/deployed-verification.json` 及 `other-project-differences.json`。


问题台账统一见[问题与修复记录](bug-fix.md)：每次发现问题追加或更新根因、方案和验证状态；
本文保留运行时间线及证据位置，字段定义仍以[字段字典](field-dictionary.md)为准。

## 用户修改.env后重新验收（2026-11-04）

用户明确要求用修改后的.env重新运行以排查服务商影响。已直接读取deploy/.env，
Chat/VLM均为https://cpamc.818996.xyz/v1/chat/completions、gpt-6.1-terra；同端点同模型
的VLM沿配置逻辑继承Chat的131072窗口，仍保留并发2、60秒raw idle。无继承模型环境
覆盖；GET models返回200且列有新模型，生成调用0。服务商和模型同时变化，后续对比
是整组配置对比，不能只归因供应商。未打印/复制密钥，不改用户.env。

已向唯一旧bug140-successor runner发送SIGINT，实际停在F0尚未准备/发送的边界；
6次物理请求均已结束、0在途。首A0第1次完整流但JSON尾缺] }，第2次携带现有格式
反馈后通过；第二局部包首次HTTP成功但两处章节多余purpose字段拒收，随后3次均首
raw静默超时，局部额度耗尽后进入来源结果保全的F0门控。F0输入146858、fits=false，
尚未发送。执行因用户切换配置主动停止，不记为新增模型失败；原分析、前驱回执、
旧Word/ZIP、两份runtime保全检查全通过。详见旧目录provider-change-stop.json。

新证据根为/opt/workspace/kb-tender-evidence/2026-11-04-perf-f9-provider2/。复制同一份
已验证402文件代码，manifest SHA为72441120fd609b478e94962613f6b59a33987616e7459584706fe76a6ead3001，
不热改旧冻结代码。准备独立analysis_provider2 schema的新分析，成功后才从新分析真实
快照编制未确认稿；不自动确认、不将旧服务商回执混作新配置验收。此时仅连通检查，
新模型生成尚未启动，入口与新分析/编制状态在本节后续记录。

新分析已启动：run 5efa33e9-5e68-481b-84bd-503b095e0209，analysis_provider2初始化
11张分析/checkpoint表且起始0 run/0请求/0确认。新入口独立复核通过，run_analysis.py
SHA为6aa3dae1392c99fd0c21a9f9eba3fba8912dcb3b807dbdae6256f8b8349ac9c1；固定
dotenv、完整执行/解析spec、源文件SHA、loopback与隔离DB/role/schema，终态额外记录
在途请求计数，不自动retry新命令或确认。初始2个OCR均首发成功（3.931/3.481秒），
OCR后实际4个抽取包，当前首2包在途；未据OCR速度宣布整轮提速。阶段对照只读脚本
及截面证据在provider2/analysis-comparison，进行中raw计时没有落盘时保持未知。

新编制入口已准备并独立复核，尚未启动。必须等本轮分析真正waiting-confirmation、
failure_type=None、无在途/确认且全部spec/原件CAS身份一致；缺finished.json的实际
负例已确认不创建编制目录/UUID/CAS。之后还需在analysis_provider2初始化authoring
段表，再预检真实S调用；不借旧parent或回执宣称新配置完整链路。说明与输入要求见
provider2/authoring-entry.md、authoring-input-requirements.json及independent-entry-review.json。

本轮新分析已完整执行至终态`blocked`，1035.841秒、60次物理发送均正常返回且无重试，
0在途、0确认、failure_type为空。调用为O2/E4/R8/D1/L24/A16/J5；覆盖无缺失、无未完成
单元。抽取阶段548.727秒，旧配置同阶段881.022秒；新逐源核验8/8一次通过、82.988秒。
旧核验只有7/8且最终失败，不能拿两轮总墙钟宣称完整流程提速。新抽取263事项/27关系，
输出56791本地可见Token；旧230/23、65309Token，输出与语义不同，质量仍单独核对。

OCR两份回执为complete1/unresolved1，先前“2个首发成功”仅指请求返回。未决项是封面
公司标识，完整PNG确实随请求发送：离线重建请求digest与发送回执一致，模型正常返回
`image_not_supplied`。根因见BUG-142，不能认定宿主漏图或供应商丢图。最终核验有389条
blocking问题，其中388条属待人工核对，另1条受保护来源问题；包括抽取处置、引用、遗漏
等，不能把全部问题归因封面图片，也不能把模型finding全当作已确认缺陷。

用户随后明确“先跳过这个问题”，因此暂停OCR进一步诊断，无新增OCR调用，不修改旧
识别回执或确认状态。继续新配置编制：产品`read_authoring_input/_available_input`原生
允许带问题的固定快照，保留`authoring_analysis_issues`并生成未确认稿；外部验收runner
先前额外要求waiting-confirmation过严，正按这一既有边界最小修正。只接受本轮已结束、
无执行异常/在途、完整根产物及checklist身份一致的快照；不使用旧parent，不宣称分析
或业务验收通过。相关修正只在外部验收脚本，不热改冻结生产runtime。

编制准备期间再次检测到用户`.env`的Chat/VLM模型变为`gpt-6.1-sol`，端点与窗口不变。
首次离线预检在配置校验处停止，未创建编制state、未发送请求；独立schema的authoring节
两张空表及索引已初始化，实时分析与终态文件均未变。按最新配置继续编制，原terra分析
不重跑；外部入口分别固定分析依据的原完整spec与编制sol配置，不能把这两段称作同模型
对照。真实后续以该入口的最终preflight和execution回执为准。

sol编制离线预检36.838秒完成，S为3包、首次输入92130/92092/33549 Token，均在94208
预算内，累计217771；完整520个native unit按221/221/78进入来源包，旧回执复用0。
执行`perf-provider2-authoring-8905af54488a4eb4868b60ae2ca70423`已启动，首段3个摘要
放行、含原四次额度最多12次物理发送；后续动态请求仍在准备发送前逐段计量。输入固定
本轮terra分析的blocked快照，问题389、human388、未独立核验关系13、confirmation为空，
运行中不改冻结代码或模型配置。详见provider2/authoring-run与authoring-comparison。

本轮sol编制已在全局A0尚未发送、0在途的边界正常SIGINT结束；持久执行状态为failed/
CancelledError，原因是主动保存容量问题现场，不是新增模型失败。3个S、3个局部A0均已
校验采纳；8次物理发送中7次完整返回，另1次S流内上游`server_error/request_timeout`
在109.774秒结束，首raw/最大静默7.542秒，usage未知。首A0另有一次完整回包引用了本包
不存在的`s855`（实际仅s1–s359），被正确拒收；原额度纠正后通过，未手改回包。

S请求区间并集395.580秒；局部A0四次物理发送区间并集493.042秒；合计模型占用并集
888.622秒，请求耗时和1210.532秒。整轮墙钟1351.830秒，其中外部逐段审核/停止等待
404.869秒，不能将其当成引擎性能。7份已知provider用量输入563668、输出52616、其中
reasoning2031；另1次未知不补零。8次请求最长raw静默35.993秒，未触发本地60秒idle。

全局真实发送前视图与禁网重建一致：129137输入/94208有效输入预算/32768输出预留，
在131072总窗口内仅剩1935，fits=false。局部3包92947/93950/69480均fit；91章、103
候选完整不重，规则/模板/摘录闭包缺失0，37个未分配事项只在局部末包且在全局保留。
全局摘要未获放行、发送0；F0/A1/正文/Word未执行，完整业务验收继续未完成。容量仍属
BUG-140未完成部分，不能以更换服务商或机械增大上下文配置宣称解决。

保全检查确认新分析/空人工确认、旧快照/Word/ZIP和冻结runtime全部未变。新证据为
`authoring-run/capacity-boundary-stop.json`、`preservation.json`、`run-timing.json`及
`a0-capacity/plan.json`/`verification.json`。Word审计入口已准备并核对本轮58个来源锚点，
但没有新Word可审，不能把准备计为成稿验收。后续先解决全局输入及输出空间，再以新
execution和完整摘要校验确定可复用范围；不覆写本轮已成功或失败回执。

## F9后续真实验证准备（2026-11-04）

用户要求继续后续计划，证据根为`/opt/workspace/kb-tender-evidence/2026-11-04-perf-f9-followup/`。
已冻结当前runtime及完整SHA清单，保持原模型gpt-5.6-sol、131072窗口、并发2和新60秒
raw idle；先编制有限样本，再运行独立新分析，避免共享Key并行干扰。原模型端点授权
模型列表可达并列有原模型，仅是连通检查，不代表生成稳定或上游断连已解决。

本机当前knowledgebrain容器实际属于另一仓库，不能据其健康状态认定本仓库已部署。
原accept_real临时数据库端口不可达，未观察到原Python运行进程；不可把连不上库当作
持久状态中没有在途。改用本轮新建专用数据库、分别隔离分析/编制schema，从已保存的
真实快照核验输入，不修改旧业务数据库、旧回执、Word或人工确认。原confirmation为空
仍为空；新分析入口明确不自动确认、不自动串联编制。

编制预审：4个新S请求，最大/累计首次输入91956/279894 Token，旧精确复用0；含原
四次额度最多16次物理调用。authoring_f9已导入真实parent及成功复用所需predecessor
回执；旧审计仍保存在原外部快照，不将未导入的旧审计行当成旧用量0。动态A0/F/A1等
在发送准备前等待本轮逐摘要放行，原心跳及取消仍生效；等待时间单列，不能算常规墙钟
性能。完整清单与入口见authoring-run/preflight.json、authoring_preflight.py。

新分析预审：106页原件，2个唯一OCR请求，未OCR时4个临时抽取包；OCR后分包可能
变化。独立analysis_f9已初始化且0 run/0请求/0确认。抽取输入是字节保守估算，review
是本地o200k，两个数值不能直接混比。analysis-preflight记录完整边界与只分析入口。
两份入口已独立审查，尚未启动真实模型生成。

启动受阻：自动审批拒绝执行首段真实编制，明确理由为发送可能敏感的招标内容到
api.kunkun-gpt.com，而“继续计划”未明确授权具体内容和外部目的地。拒绝发生在进程
创建前，没有模型发送；已询问用户是否授权该106页原件相关正文/结构化结果/OCR图片
外发至既有端点用于本轮真实分析和编制。用户随后明确授权完整端点
`https://api.kunkun-gpt.com:8317/v1/chat/completions`及本轮相关内容和模型费用；据此已启动
authoring_preflight.py的首段真实执行。先核定4个S请求（含原重试最多16发送），后续
动态请求逐段核定，完整分析待编制结算后执行，不并行争用Key。授权问题已解决。

04:32 UTC首段观察：4个S已有3个正式采纳，累计6次发送，最后一个S第二次仍在途。
首个完整回包191.471秒，首raw/最大静默26.550秒；S2首次313.249秒后采纳。S1因
章节角色错放target而纠正一次后通过；S3首次258.601秒完整结束后JSON语法失败，
仍报告91750输入/8157输出，不能将无效结果的用量记零。独立原流回放确认S3有DONE、
stop和usage，根对象过早闭合导致Extra data；不是传输截断或客户端分块损坏。
兼容json_object模式与已批准BUG-099一致，不临时切strict或猜修角色/JSON。
细节见runtime-audit/live-source-diagnostics.json，实际总账以最终execution快照为准。

04:45 UTC：本轮已在A0第二包未放行、模型无在途的边界主动结束，专用执行以
CancelledError结算；这是发现BUG-140后中止基准，不记为模型失败或Word业务完成。
成功的4个S和首个A0回执保留，原分析/前序回执/旧Word/runtime的保全检查全通过。
共10次物理发送：S7次、A0首包3次。S完成请求区间并集1047.972秒，provider输入
554948、completion58479、reasoning6947，7/7有usage；A0首包先非法JSON、再60秒
首数据静默超时、第三次采纳，用量有1次未知。全轮wall1644.262秒，外部门控等待并集
204.493秒、完成请求并集1394.251秒、请求耗时总和1835.194秒，三者不互相冒充。
未进入F0/A1、未产出本轮Word，完整新分析亦未启动。

BUG-140新增证据：实际A0为3+1，局部输入86081/93958/104045，全局155287；
末包在试装后追加37项未分配规则，额外10211 Token；实际首、二包候选规则证据
分别有29/18个x-id未闭包进完整source_excerpts。规则quote仍在、宿主原文未改，
但不符合本请求Prompt的完整依据承诺。先修复依据闭包和最终装包，再测全局无损表示，
新执行只按完整摘要复用本轮成功回包；不热改已冻结runtime。入口、停机理由与测量见
authoring-run/boundary-stop.json、a0-offline-plan.json、a0-reference-closure.json。

输出表示试算另发现6份可解析S JSON可见本地Token43969→34099，仅移除字符串外
空白即可减少22.45%，逐值/类型/顺序不变；无效JSON未修复、未知usage不补零。
这只证明表示开销，不证明模型会遵循紧凑格式或实际同比提速，当前Prompt未改。
证据output-costs目录。

BUG-140本地补片：候选/未分配规则证据闭包先2失败后通过，相关100项通过。
实际固定原前两包缺失29/18→0，除新增所需source_excerpts外所有字段逐值不变。
最终装包的末章试装纳入未分配规则，另2项失败反例转通过，相关83项通过。真实4份S
禁网重验后，86章/125候选完整且唯一，37项未分配只在末包；1044份摘录完整保全。
最终局部4包输入87138/91794/93600/88901，全部≤94208；旧3包变4包，4个A0摘要
均变化，旧成功首A0不能直接复用。全局仍155287，故现计划仍4+1，最多20次物理发送。
证据bug140-closure与bug140-packing；这是容量和保全证据，不是新编制业务采纳。

新outline_assembly_planning_contract冻结标记先2失败/1通过，再21项document执行
验收测试通过；另2项明确验证同合同/旧合同前驱均可在新execution按精确请求摘要及
当前校验复用成功回包。测试用专用数据库随机schema和模型替身，不计真实发送。
证据bug140-contract。源码新合同不注入已停旧执行或正在运行的原F9冻结分析runtime。

全局无损表示试算：同值规则投影、模板描述、retained来源和原文行池逐值往返保全，
组合输入155287→124191，仍超预算29983；最后窄试验只再省7 Token，未落生产。
249条完整规则没有可删默认空字段，482个quote仅454种、仅1个与任意规则text完全相同，
不能继续靠删引文冒充无损。证据bug140-global-view，不声明全局容量已修复。

分析执行已在编制无在途的边界启动，使用原F9冻结runtime、独立analysis_f9和原授权
端点；先2个OCR成功，随后2个抽取并行。此为新run，不自动确认或触发编制；编制后继
等待分析结算，避免共享Key争用。动态analysis-preflight/run/progress.json和最终快照
为准。新发现BUG-141局部回退重复/串批已先登记。

BUG-141本地修复：局部接收器现绑定每次实际请求的t/a/u/x/p/e/s域，遗漏只保全本包；
同一回调生成器覆盖单包/局部/全局/A1，compact/retained保持完整身份域和安全继承。
新增14用例先13失败/1全局正例通过，再全部通过；相关218项回归通过、独立源码复核
无阻断。拼接另先2失败/1通过，再59项通过；按包优先回绑，唯一旧跨包引用保持兼容，
歧义保留目标并记录待核。合成实际DOCX/ZIP总稿分别参见1.1甲包、2.1乙包，两个分卷
只含各自参见。它证明导出引用正确，不替代106页原件真实Word验收。证据bug141-binding、
bug141-join；全量最终unit正在运行，源文件未提交/部署。

规划冻结标记更新为complete_evidence_scoped_batches_v1，覆盖闭包/最终计量/局部
接收域；先确认旧标记断言失败，再22项document执行隔离验收全通过，含旧前驱的新ID
精确复用。新的bug140-successor入口已准备且完成独立差异审查，尚未freeze/preflight/
创建数据库执行或发送；CAS回读已停F9及v8，旧runtime和快照不覆盖。

新分析抽取阶段：4次发送均成功并逐工作单元找到extract-pack采纳回执，另有extract
聚合；provider输入100345、completion68311、reasoning2978，完成区间并集881.022秒、
耗时和1519.353秒。四份可见内容共65309本地Token，原来已全紧凑，去空白收益为0。
前两次首内容后的输出阶段占95.25%/92.38%；不能套用S输出空白收益。最后242.201秒
仅一槽运行，固定总工作量和等速无损调平的理论回收上限约121.346秒（13.77%），不是
重分包实测改善。原2份cutoff与4份完整审计均保留在analysis-output-costs。

随后R核验实际持久计划为8个，已有成功及首数据静默失败混合；失败样本同一逻辑单元
连续3次在before_headers约60.04秒结束，0 raw/0 SSE/0 usage。确为本地首raw截止，
尚不能称供应商断开、JSON损坏或计费为0；仍按原四次额度，最终状态以运行快照为准。
只读report_analysis_progress.py经独立反例核对，逐指标报告已知小计/缺失覆盖，无usage
返回null，区分在途待结算与结束后未知；不把HTTP成功和业务采纳合并为一项。

新分析终态：1550.950秒后因ModelWorkUnitExhausted失败，20次物理发送全部结算、
0在途、0人工确认。R共14发送=7成功+7次首raw静默失败；review-6d11前三次失败后
第4次成功，review-150f连续4次失败耗尽；后者保持失败，不自动新建retry命令。
7次失败共占用420.310请求秒，usage全未知。R实际8个、采纳7个；新台账230事项/
23关系可离线构造D=1、输入63463、全26335事项对覆盖，但D并未运行/落计划，L/A/J
仍动态未知，不填0冒充已完成。详见analysis-runtime-audit与analysis-preflight/run/finished.json。

BUG-140/141最终技术验证：完整unit2455通过、0失败/错误/跳过（281.28秒），源码
运行前后SHA不变；另document执行22项、来源/规划/前驱/提交来源10项PG验收通过。
真实4S禁网回放配合合成空A0及global失败，4包各保全59/30/30/6候选，最终125个/
125次绑定、0跨包重复；280规则refs、201候选来源、165提交来源保全，37未安排规则
仍明确待核，旧runtime/Word/source未变。合成失败只验证宿主恢复，证据bug141-final。

新后继已冻结402文件，manifest SHA为
e8214f0b40568e47085ce2c1271b836cff8791f296ff6cc665c8bd8f1904764e，独立核对全部
文件集合/字节与当前源码一致。新runtime不覆盖原F9。禁网预检31.963秒，4/4已成功S
精确摘要及新校验通过，新增来源发送0。后继execution为
perf-bug140-successor-8611145896364d04b44b14336ceab2f0，前驱为已停F9执行；仍使用
authoring_f9、原分析快照、空confirmation和空企业资料，未将新失败分析冒充已完成输入。
新分析结算后才启动编制，仅放行已测4个局部A0摘要、含重试最多16次，全局和其他
动态请求仍在准备发送前门控。入口、清单与新快照见bug140-successor/authoring-run；
此时首个局部A0已发送，尚无新Word。

历史传输复算：两个原证据根31份快照去重561次请求，样本观测并发峰值2，跨command/
execution重叠0秒，没有样本内不同任务争用额度的证据；不能外推共享Key的全局负载。
280次transport_response_unknown均为before_headers，其中277次落在59—64秒，
中位61.023秒；v8的14份HTTP trace显示请求body发完后等待headers失败。缺供应商/网关
日志，关闭方与具体deadline根因仍待查。证据runtime-audit目录，未据此改网络或并发。

固定依据的合批容量试算：64个旧associate/joint批、200目标，目标上限1/2/4分别需要
200/109/64个请求，累计本地输入2593103/1429260/839504 Token；完整依据与成员逐值
保留、回答目标不重不漏，发现计划39060对完整。独立复核373行明细及6个最大输入请求
计量一致。该试算只拆回答targets，未全局重新装批；不证明模型质量、时延、推理或重试
改善，保持现有4目标上限。证据batching-capacity目录，新增模型调用0。

## F9分析与编制性能修复（2026-11-04）

用户同意完整方案后，已本地实现请求计时、持久退避、纯review请求缓存、S包校验隔离与
有界并发、抽取关系ID宿主派生。尚未部署/重启/提交/推送，新增真实模型调用0；完整
单测及隔离workflow/acceptance扫描与失败项复验已结束，不能称为真实任务提速或Word语义验收。
方案与源代码清单见tasks/plan.md、tasks/todo.md的F9；原F0—F8未完成项继续保留。

证据根为`/opt/workspace/kb-tender-evidence/2026-11-04-perf-f9/`：

- 请求计时分别保留first_raw、first_data（解码块）、first_content、max_raw_idle和内部
  semaphore排队；gzip/心跳/thinking/空delta/取消/尾段与慢close均有先失败反例。首次
  终止后冻结计时，不把JSON处理、CAS写入或close延迟算成网络静默。HTTP拒绝无法观察
  raw时保持缺失。`request-timing/green-final.xml`208通过；安全字段过滤45通过，两条
  PG保存路径48通过。`report-green.xml`52通过，历史编制24请求报告区间并集1534.488441秒，
  与原独立诊断一致，旧回执缺raw计时不补零。
- 普通/合批429统一现有30/60/120且封顶120秒；物理失败与语义拒收持久not_before，
  恢复等剩余时间。物理审计后journal前崩溃、预留后等待中崩溃、过期、取消/失租、
  成功兄弟复用及四次额度已定向覆盖。交叉复核另修复“新preflight失败误用上次429
  deadline”的问题，本次物理回放限定当前group_attempt，发送前仍检查相关未过期截止。
  218项定向通过后，曾因仅注入一个测试DSN跳过20项document测试；补齐环境后20/20通过，
  不把跳过计验收。最后扩展物理/语义崩溃回归`f91-recovery-complete.xml`72通过。
- 真实冻结回执禁网重放发现主热点是review_requests构建。原3轮44.348—45.412秒，
  中位44.750秒；缓存冷构建44.726秒，热恢复8.645/8.716/8.976秒，中位减少36.034秒。
  每轮完整verification SHA相同，0网络/0模型。10/10真实review请求摘要完全相同；
  直接CAS排序会改变10/10摘要，因此缓存JSON字符串保序，体积992393字节。依据见
  recovery-profile-before/after.json及review-cache-wire-audit.json。未落收益不足的
  单独operation_plan缓存，也未跳过locate/association副作用规划。此为恢复准备收益，
  不是98分钟续跑缩短80%的证据。
- S包先以实际提供的冻结正文范围校验，完整原文可覆盖其片段，同native unit其它片段
  不冒充已读。每包wire/回调固定，协调者按计划序归并；并发沿既有执行参数，默认2。
  定向70通过，PG来源恢复/单发去重3通过。固定延迟夹具来源段0.604→0.353秒，
  _organize总0.609→0.357秒，4个请求及完整产物摘要一致，证据f94/fixed-delay-report.json。
  这是调度实验，不是真模型提速；样本R仅一包，未额外拆包或并行R。
- 抽取只去掉模型必填关系ID，由宿主按原数组位置赋pack:lN；事项ID和全局Relation、
  人工纠错合同不变。173项定向通过。BUG-077原始回包在当前接收器只读回放保留112事项、
  27关系，仅第24个关系ID变化，关系文本/端点/证据逐值相同；233处置/23问题等是既有
  规范化结果，不计本次业务改善，证据relation-id-real-replay.json。

收尾验证：`unit-final-clock.xml`完整单测2433通过，0失败/错误/跳过；首轮两项精确
sleep断言受实时时钟影响，测试改用推进虚拟时钟，仍精确检查1/2/4秒，不放松产品策略。
`integration-final.xml`完整workflow/acceptance扫描372项，370通过、2失败、0错误/跳过；
两项均是流解码后审计失败恢复的旧metadata全等断言，不认识合法新增的等待截止字段。
仅调整测试，精确限定新增not_before和retry_deadline_origin，原metadata、请求身份、
发送时间、回包引用和未知用量仍全等，并校验截止为恢复行的finished_at加1秒。随后整份流恢复、
PG截止及普通语义/429恢复`stream-deadline-final.xml`64通过，覆盖原2失败项；没有把这次
专项复验描述为第二次372项全量运行。新增测试无跳过或删除，独立审查通过。
33份修改/新增Python文件语法编译、`git diff --check`及新增内容凭证模式检查通过；
本机未安装Ruff，未执行该工具，也未为本次验证安装依赖。
最终XML交叉核对及源码SHA见`final-verification.json`，两项原失败均在复验中逐ID通过。
本轮专用PG数据库/角色和Redis容器已清理，临时凭证已删除，业务数据库/服务未改；
测试报告和回放证据保留，清理记录为`cleanup.json`。复跑需重新建立隔离测试环境。

冻结边界：三个模型执行入口新增retry_policy；编制另记录来源校验合同，抽取Schema及
Prompt摘要随合同变化。旧spec不自动补字段，旧180秒执行仅指定原timeout也不足以匹配
新runtime，必须使用原冻结runtime；新后继执行仍按精确请求摘要重验成功回包。

未完成：上游约61秒断连缺供应商/网关日志，不能宣称关闭；当前S Prompt与v8不同，
禁网预览在首个新请求停止（91750输入Token、374407 wire字节），未借旧S回包冒充新版
A0/A1验证。进一步字段精简、缺集合补写及合批大小仍需可靠基线和质量/总成本证据。
现成原始短引用回包去关系ID仅省615字节/216个本地o200k Token（可见输出约0.73%）；
112个事项中仅2个text与某条quote完全相同，不能继续按原文冗余删除语义文本。准确的
缺集合候选未定位，补写还需携带原依据及已有事项，未证明净收益，暂不启用。保持当前
4目标合批及完整配对覆盖；依据见extraction-output-costs.json和f95-f96-conditional-decisions.json。
普通路径混合传输与语义失败后重启，后续语义短退避序号仍可能为2秒而非累计物理序号
对应的4秒；已记录截止不重置、物理四次额度不突破，此既有限制未扩为通用attempt日志。

## 连续60秒无流数据期限（2026-11-04）

用户提出先收紧连续无数据等待。源码中OpenAIModel、编制execute_document及保存稿复核
execute_saved_review三个默认timeout由180改为60秒，复用原始字节idle算法，无请求总时长
截止；心跳、thinking及非空原始字节均续期。部分流超时仍保留uncertain与原始回包，原有
首次加三次额度不变。没有部署、重启服务或新增模型调用，真实提速和误超时率未验证。

证据`/opt/workspace/kb-tender-evidence/2026-11-04-idle60-tbf1VY/`：red.xml记录旧默认下5项
失败；首次相关回归264通过/1失败，失败是tool活动测试累计虚拟时长旧断言未同步；修正后
green-final.xml共265通过，0失败/错误/跳过。覆盖首数据共享期限、60秒边界、中途静默、
每59秒来流且总时长472秒、心跳/推理/SSE/UTF-8/gzip活动、部分回包、配置漂移及恢复。
独立源码与XML复核通过，未提交或推送；原有并行方案文件保留。

timeout属于冻结执行身份，新默认不覆盖旧180秒快照；旧分析revision或编制execution若
直接改用新默认续跑会被身份校验拒绝。叠加F9合同修改后，须沿原冻结runtime恢复，
仅显式指定旧timeout已不足以匹配旧spec。新编制后继执行
可按原精确摘要与重验规则复用成功回包。既有约61秒响应头前断连通常只能提前约1秒结束；
成功编制首批解码数据曾到60.226秒，raw最大空闲不可由该字段还原，实际误超时风险待观察。

## Word内容修复与同文件编制恢复（2026-11-03）

本地零模型重导出已核对 BUG-138：法规名单不再粘成一段，技术参数表按原表 4 列画出且投标响应列为空。成品在 `host-recovery-v8/reexport-current/投标文件.docx`。未部署，未提交前的工作区改动不代表已发布。知识库比对逻辑仍在，本稿没有企业资料，没有新的检索或写作。不启动第 4 轮付费运行。

13:25 UTC：第3轮在13:21:55结束，303.061秒，4次A1均响应头前断连、用量未知，
16份成功回包精确继承、55单元恢复。outline、全部unit及coverage引用/字节与第2轮
完全相同；总Word及三分卷的全部解包成员相同，仅DOCX ZIP时间戳改变，不能把
二进制SHA不同当作内容改善。沿用第2轮业务未通过结论，当前无在途付费运行，
第4轮仅计划未启动。v8累计27次新增发送、报告669996 Token、14份用量未知。
仍须解决技术层级、第二表共同条件、剩余4处源页首、两个附件式标题，以及部分
已绑定narrative技术要求未完整进入正文；旧Word、原分析和回执均保留。

工作区另确认并最小修复BUG-137：未归属要求待核区只由总稿显式启用，独立分卷
不再无条件附加；单卷/双卷实际DOCX先2失败后通过，相关5文件66项通过，独立
增量复核通过。该3行生产增量及新测试未混入v8，未部署，不代替真实语义验收。
其他并行方案/代码改动保留，未提交或推送；当前仍不能宣布完整正式投标文件验收完成。

13:17 UTC：第2轮结束于13:14:22，400.863秒，5次新增发送（F0成功、A1四次
响应头前断连），报告108518 Token、4份用量未知。53章/57目标/32响应行未变，
模板ref由338降至332，采用15份前序回包及55单元；第2轮Word待独立审计。
第3轮已按同一冻结runtime启动，execution
bid-content-repair-eb2fc2e84cd242e884a2e2ae32519932，前序为第2轮，仅补未完成A1。
工作区此时有并行方案/代码修改，未混入v8冻结runtime，也不将本轮结果作为并行
新版代码的验收。另经精确审计：温湿度、三年现场质保、逐段应答和KKS等原文仍在
目标引用及底稿，但部分narrative正文仅待补，未完整显示对应要求；26核心锚点
通过不代表全部技术要求已进入Word。该承载缺口继续追踪，不用缺企业资料解释。

13:13 UTC：第2轮F0已首次完整返回并正式采纳，识别技术结构与原文层级不一致等
问题，提出6组独立源装饰排除及4处标题修改；进入既有一次A1修订。A1前两次均
响应头前断连，第3次在途，尚无A1拒收或新Word。第1轮实物审计已完成，报告
v8/audits/round-1-business-verdict.json：三卷、26/26技术锚点及完整跨页协同防御
已保留，无企业资料和报价填值、无JSON/purpose泄漏；第二表缺共同条件、10处
源页首与两个附件式标题仍未通过。F0动作是否在新稿保全及剩余问题是否修好待核。

13:09 UTC：host-v8第2轮独立恢复已启动，execution
bid-content-repair-c2110681defc43e9b07f56ab49f53319，previous指向第1轮；冻结代码与
配置预审通过。已精确继承四S及正式A0共5份回包，仅发送F0，未重复全文件抽取。
第1轮Word及逐文件SHA保存在round-1/resume-boundary.json；结构、标题及共同条件
仍待真实复核，不将第1轮A0成功误记为BUG-136的全失败保全分支已真实触发。

13:06 UTC：host-v8第1轮已在13:03:50正常结束，970.197秒（约16分10秒），18次
新增发送、6份用量未知；已报告561478 Token，不将未知用量当作未计费。A0第4次
正式通过校验并采纳，形成商务/技术/报价三卷；F0四次均在响应头前断连，无可采纳
结构结论，未触发A1。实际53章/57目标/32响应行/338模板ref，55单元，旧稿保留。
初步独立Word审计确认26技术锚点及237/118字符两段已有实际应答位；第二表共同条件、
源格式标题及重复/页首仍在核查，不能将导出或自动投影通过解释为完整业务验收。
下一步只恢复未完成结构复核，精确复用本轮已接受A0和来源/写作回包，保留第1轮Word；
不重跑分析、不伪造人工确认、不改模型、网络、并发或180秒策略。证据host-recovery-v8。

12:47 UTC：BUG-136最小修复已完成：A0耗尽保留来源局部树和精确候选映射，封面及
提交待办角色沿用；全局未归并时is_volume仍false，不自动发布局部分卷建议。
最终完整unit2297及隔离PG25项全部通过，无失败/错误/跳过。真实来源禁TCP/DNS投影
中四份S完整请求精确相同、原回包逐validate，保留86章/91目标/31响应行/367模板ref，
90计划单元、11个写作候选；未模拟真实F0排除，367与旧稿361不等于新增实质表单。
投影只是宿主与容量验证，未修好两缺失技术段、跨源重复、原页首或第二表共同条件。

host-v8已独立冻结159文件，仅pipeline更改，2046份CAS逐SHA独立复制，零网络预审
通过；原模型gpt-5.6-sol、地址、并发2及180秒策略不变。保持旧S冻结合同以精确复用，
不冒称新版S Prompt已付费验收。12:47 UTC启动execution
bid-content-repair-a55a74ba9cf444eea80f041c01cb4124，previous明确指向v7唯一执行；
不重跑原分析或伪造人工确认，不传旧降级大纲。当前尚无本轮Word，仍须实物验收，
旧稿/回执/执行保全，未部署、提交或推送。证据host-recovery-v8及
bug-136-source-projection-20261103T124000Z/fixed-final/。

11:05 UTC：host-v7第1轮正常结束，551.043秒（约9分11秒），新增8次正式组装发送
全部uncertain/usage未知；不把reported usage为0当作未计费。A0/A1各首次加三次均
未取得响应头，固定标签全部server_disconnected_without_response，耗时约61秒。
独立trace已把失败收窄至本地发送完成后的等待响应头阶段，具体远端关闭原因仍未知。
正式复用19份成功回包、恢复84单元；成品仍1章91目标、31响应行，业务验收未通过。
独立Word审计确认outline/unit/coverage引用及DOCX所有解包成员与v6第3轮完全相同；
无分卷、概括待补与原格式重复安排、237/118字符两段无完整应答位、第二表缺共同
条件及5处源页首均未改善。原分析、原执行、v6前序执行与文件、冻结runtime全部
保全检查通过；原分析仍waiting-confirmation，无企业资料不编造应答或报价。
当前无在途付费运行，v7第2轮仅计划未启动。证据v7/round-1、http-traces/、audits/；
不再无变化地重跑整轮，不将传输/诊断通过或保全稿导出解释为正式投标文件已完成。

10:55 UTC：host-v7真实恢复已启动，execution为
bid-content-repair-53fca238521c4e7bb5c202f7324de3c8，previous明确指向v6第3轮。
运行driver与159文件runtime独立冻结，2040份CAS逐SHA核验并独立复制；零TCP/DNS及
真实SDK hook离线预审10项通过，未改变实际请求、headers、transport、trust_env或超时。
仅证据侧记录真实请求阶段事件，数据库仍承担正式发送围栏与回执；不使用诊断回包，
不传固定降级大纲，成功来源按完整摘要复用，保留旧Word。启动时尚无新成品，
仍须按原有独立原文锚点核对全部技术响应位置及原格式归属，不能因导出成功判通过。

10:51 UTC：独立对照正常结束，仅发送2次，两组均首次取得完整有效固定JSON。
小/大实际HTTP正文81823/631171字节，首数据15.404/31.057秒；供应商报告输入
19794/150968 Token、输出各9 Token，用量已保存。大输入本次明确超过131072仍成功，
不能将该配置当作供应商静态硬上限；也不能由此证明真实A长输出或语义任务必然成功。
两次连接/发送均完成，runtime与回执独立保留，诊断回包没有进入业务CAS或Word。
接下来准备host-v7独立恢复与真实请求阶段记录，仍从v6第3轮精确继承成功回包，
不重跑原分析、不改模型或网络、不使用固定诊断输出替代正式大纲；尚无新成品。

10:49 UTC：先停止无变化的整轮恢复，启动同端点/模型的独立大小输入对照，
证据protocol-diagnostic-20261103T104416748291Z。实际输入估算20000/151174，
两组只要求固定微小JSON；每组最多4次、成功即停，总发送不超过8，不采纳为业务内容。
诊断runtime独立冻结159文件，仅新增HTTPX远端协议错误固定标签；284项定向回归
及独立隐私/行为审查通过，零TCP预审通过。模型、网络和180秒策略未变；
只记录连接/发送/等待响应头事件名及耗时，准备与发送围栏持久化。首个小请求已发送，
TCP/TLS及请求正文发送已完成，正等待响应头；尚无对照结论或新Word。
旧执行、回执及Word保持不变，原分析仍waiting-confirmation，业务验收仍未通过。

10:05 UTC最新状态：第3轮已正常结束，535.773秒，8次新增A请求全部未收到响应头，
多数约61秒断连（另有46.31/63.10秒），没有JSON或Schema拒收；用量均unknown，
reported usage为0不代表没有收费。正式复用19份成功回包并恢复84单元，结果仍为
1章91目标、31条响应行；outline_ref、全部unit_refs、coverage_ref与第2轮完全相同，
未获得新的语义组织，真实投标内容验收仍未通过。原分析、旧执行及冻结代码保持不变，
证据round-3/及run-round-3.log保留。当前没有在途付费运行，第4轮仅计划未启动；
不将技术succeeded或重复导出解释为内容修复成功，不更换模型/网络配置、清缓存或
修改已保存Word。成品一致性复核见v6/audits下round-3独立记录。

09:55 UTC续跑：用户要求继续，host-v6冻结159文件SHA全部一致，四个本轮宿主/Prompt
文件仍与工作区相同，禁TCP预审再次通过，原模型/地址/并发/timeout不变。第3轮已启动，
execution为bid-content-repair-b93f5663e76e4c708e5a4382fa0d3e9f，previous明确指向第2轮；
证据run-round-3.log及round-3目录。只恢复未采纳调用，不重跑分析或强制人工确认。
旧两轮Word及回执保留，未部署、提交或推送，当前尚无本轮新Word或业务验收结论。

08:37 UTC补充：host-v3第2轮已结束，实际65章、83目标、商务/技术/报价三分卷，
26表、31条响应行；保留真实函件和空企业应答，但两段协同防御/跨页要求尚未完整
进入应答行，另有第二响应表共同条件、重复填写实例、标题及源页首问题，内容验收未通过。
host-v4第1轮也已结束，四份来源按完整摘要继承，A0/A1共8次均未收响应头；保全稿
仅1章91目标，不能交付为正式投标文件。实际发送请求已包含BUG-132补齐的完整
原文引用，仍不能称模型选用或最终Word验证通过；旧运行、回执和Word均保留。

BUG-133/134已经实现同填写实例声明、实例范围内的装饰排除延续，最终120项定向
回归通过。BUG-135按已有F0/F1增加明确章节改名出口，8项先RED，独立49项通过；
最终完整unit2288及四文件隔离PG25项均通过，0失败/错误/跳过，148生产文件SHA稳定。
证据后缀same-instance-decoration-title-fix-20261103T083755970817Z，不含真实模型验收。
host-v5仅准备未发送；host-v6已独立冻结159文件，禁TCP预审通过，08:39:50 UTC启动，
execution为bid-word-final-82a5e78748784f6eb6e7bec8e7eb802e。v6保留旧S合同以精确复用四来源，更新A/F及宿主，
不把BUG-129新版来源Prompt算作已付费验收；模型、地址、并发及timeout不变。
当前真实Word业务验收仍未通过，原分析仍waiting-confirmation，不伪造人工确认。
本轮四S已精确继承；A0前三次分别HTTP502（26.79秒）、HTTP503（3.36秒）、响应头前
断连（57.47秒），尚未拿到可校验内容，第4次仍在途。模型错误不能归因Schema拒收，
也不能据此确认请求未被供应商执行；151414输入估算与既有超限告警保留，仍尝试发送，
没有新预算门禁或网络配置变更。后续实际结果继续追加，旧回执和Word不覆盖。

08:59 UTC补充：v6第1轮已正常结束，1019.309秒、26次新增发送、11份用量未知，
已报告433899 Token；A0与A1各四次均无可采纳回包，F0第4次有效并正式采纳，
12份W及2份R有效。F0明确改名根章、执行6组独立装饰排除，并指出重复表、带★完整
技术行遗漏及编排待核；这些语义意见不能在A1未完成时自动算解决。实际Word仍是
1章91目标、31条响应行、84单元全placeholder；无分卷、第二表缺共同条件，技术完整
范围与协同防御跨页检查不通过，未达业务验收。Word无内部purpose/JSON，企业答案
及报价空白；原分析、旧执行及运行源码SHA不变。证据v6/round-1、v6/audits保留。

已读完整供应商HTTP错误正文：502为server_is_overloaded，503为auth_unavailable
并注明上一上游错误过载；其余before_headers断连仍不能逐条归因。A0已经compact_v1，
701模板/862完整x/655行共享池；仅再池化quote可减约2.35% wire字节，不能宣称能治愈
断连，不为此临时更换压缩合同。08:59 UTC同runtime/配置启动第2轮，execution为
bid-content-repair-80379b86e1da458d900eda5e733fba07，明确previous指向第1轮。
只补未采纳请求，按完整摘要复用成功回执；未重跑98分钟分析或人工确认，仍无企业资料。

09:09 UTC最终补充：v6第2轮正常结束，552.519秒，新增8次A发送全部响应头前断连，
用量均unknown（reported usage为0不能解释为未计费）。正式复用19份成功回包并恢复
84单元，四S不重读；outline_ref、unit_refs及coverage_ref与第1轮完全相同，实际
word/document.xml SHA也相同，因此内容没有改善。两轮均未取得A0/A1新语义组织。
完整unit2288及隔离PG25通过不能替代业务验收；BUG-135仅确认根章改名实际落地，
BUG-133/134没有新A采纳结果，不能宣称已通过同文件最终验收。两个执行和旧Word、
原分析及冻结159文件均保留不变；当前没有在途付费运行，第3轮仅计划未启动。

实物审计确认：1章、无分卷、前部概括待补位与后部真实格式重复安排，协同防御237/118
字符两段都未进入Word，第二14行响应表缺共同条件，正文仍有5处独立源页首；这些是
当前未解决的成品问题。原函件、原网格与签署位置保留，无JSON/purpose/源目录污染，
身份/授权/业绩实际原表各一份；两份财务格式分别属于投标人和原厂，不能误报重复。
证据v6/audits/round-1-business-verdict.json和round-2独立审计保留。完整请求现为
compact_v1，再池化全部规则正文/引文仅减A0字节4.36%、输入估算4.43%，A1字节4.06%，
不能据此归因或宣称修复61秒断连，不为这点收益新增协议。过载与上游认证路由不可用
仅由两份明确HTTP正文证实，其余断连仍无供应商端执行状态，后续需先取得有效A回包
再验收实际成稿；不清缓存、不加预算门禁、不变更模型/代理/超时/并发，也不伪造确认。

最新补充（07:30 UTC）：host-v3第1轮新增8次发送均未知，仍沿保全树导出1章91目标、
25表31响应行；84个单元全placeholder、72pending，内部purpose泄漏0，未通过内容验收。
财务两处残留页首对应p255，F0实际排除的是另一片段p269；不能误报BUG-131重新引入
已排除部件。第2轮A0第4次取得有效回包并进入结构核对，尚无该轮最终Word。
只读追链另确认BUG-132：一段已在unassigned_rules中的技术要求没有任何可选x原文，
另一段跨页续文则已有x431；前者是宿主引用域缺口，不能都归因模型漏选。已先更新
字段/方案/问题记录，准备通用失败回归；当前冻结运行保持不变，新宿主修复另行验证。

07:44 UTC补充：host-v3第2轮A0、F0及A1均已正式采纳，非仅传输成功。A1一次完整
DONE/stop的JSON在EOF尚缺14层闭合（BUG-005复发），精确反馈后取得有效回包，未修补
原文；采用树已有商务/技术/报价三分卷及技术通用/专用层级，F1仍在进行，尚无该轮Word。
技术响应行和表单归属仍须实物核对，不能以章节数改善判为通过。
BUG-132最小宿主及A Prompt修改已完成：新6项先4失败/2通过，修复后含DOCX6项通过，
相关72项通过；最终完整unit2265及隔离PG25均0失败/错误/跳过，148生产文件SHA稳定。
真实A1只读重建补齐237/118字符的前后两段可选原文，旧666引用逐值不变，新增精确
证据196份/25894字符；没有模型、DB或CAS写入。新host-recovery-v4/冻结159文件，
仅更新pipeline及A Prompt，旧S Prompt/Schema保持以继承原来源回包，不冒称BUG-129
新版来源合同已付费验收。禁TCP预审通过，尚未启动；待旧运行安全结束后复用最新回执，
同模型/地址/并发/timeout继续真实测试，旧Word不替换，完整业务验收仍未通过。

用户指出导出的Word不是所需投标文件。已确认此前209条待安排要求没有正文位置，
技术响应表和原格式引用为0；不是“企业资料缺失所以要求可省略”。实际失败来源请求
已包含第五章技术内容、技术参数行及第三卷投标格式，不能再解释为用户未提供原文。
BUG-125的写作/复核及Schema合同已补用途错误：编制说明不能代替实际应答；模型明确
否定后沿既有机制剔除生成段落，固定原文、背景和无资料空位保留，不新增语义节点。
完整Python单元回归2232项通过（178.11秒）；新增PG隔离恢复回归通过，确认新执行
只补失败来源/归并、成功回包继承、旧Word不变，新Word保留真实函件及空白技术响应。

新证据根`2026-11-03-bid-document-repair/`，修复后runtime独立冻结159文件，旧runtime
及全部旧成品保持原样；1231份真实CAS复制验SHA。默认禁TCP预审通过，04:52:33 UTC
启动新execution `bid-content-repair-089ebcda7a3146b1be6fe15c5860176d`，previous指向
上次完整续跑执行，原authoring_input_ref不变，不传旧降级大纲或定向review_request。
模型gpt-5.6-sol、原地址、并发2、180秒超时不变；不重跑分析，不伪造人工确认，
没有企业知识资料。已实际继承首份成功来源回包，开始补做未成功来源。每个执行内
仍首次加三次；外部driver先观察最多三轮既有前序恢复，各轮结果单独保留，未知用量
不记零。后续必须核对实际原格式、技术要求及共同条件、正文用途与空报价；启动时没有
新Word，不能用单元回归或开始发送代替内容验收，未部署、提交或推送。
随后四个受影响持久化/恢复/提交来源验收文件全部通过：25 passed，0失败/错误/跳过，
29.35秒；使用独立PG schema及MockTransport，不触发额外真实模型调用。日志与JUnit
保存在本轮证据根，不把这些隔离结果当作新Word的语义验收。
首份来源按原摘要成功继承。第二份来源首次取得完整JSON（7540输出Token），但两处
目标kind误写为章节展示用途submission_note；宿主保存原回包和精确错误，沿既有
纠正请求重试，未擅自猜测转换或采纳。后续三次纠正均约61秒断连、用量未知，本轮
该来源未采纳，已继续读取第三份来源；不能把传输succeeded当document-model-response
接受。独立实物审计脚本已对旧坏稿得到红色基线，将分别检查原文保全及真实应答位置，
不能把只复制技术指标原表、没有企业应答格视为已形成逐项响应。
第三份来源首次完整回包也有kind层级混淆（response_table/submission_note误作kind），
沿已有精确反馈纠正后已正式提交document-model-response：32个局部章节、38个候选，
其中25个form及14条response_table行。第四份来源按原摘要继承；当前三份来源采纳、
仅第二份未采纳，开始A0归并。候选不等于正式大纲或Word已恢复，最终仍须独立审阅。

第1轮结束用时1833.379秒，25次新增发送、12份用量未知，已报告504757 Token。
新Word61431字节、实际24表/1371段落；1个保全章节、71个target、14条响应行、
367份模板引用（不是367份表）。68个采用单元均placeholder，无生成正文或企业填值，
此前BUG-125的规则复述已不再采纳。独立原文/实物审计确认函件、签署、身份授权、
廉洁、报价、偏差、商务财务及开标价格格式实际落入Word；14行要求和空白应答保全。
仍有6组技术参数/跨页检查不满足、pending132，不能把中间稿当完整投标文件。

实物审查新发现BUG-128：导出器把内部purpose直接作为23处Caption输出，另有无单元
待补文字同类出口。已先更新合同并做5项失败回归，再修复为仅显示实际subject和通用
占位；原模板固定说明不删除、不按关键词过滤。60项定向回归通过。用同一真实大纲、
采用单元及原requirements/coverage独立重新导出到`new-renderer-round-1/`：23处内部
Caption归零，24张表XML、14条要求/空白应答、书签、图片及覆盖结果不变；模型0、DB0。
renderer-provenance.json记录新渲染器实际模块哈希，comparison.json保留逐项比对。
原round-1和执行结果未改，正在运行的冻结runtime也未热改；不能宣称此刻实际执行
已经使用新渲染器。新重导出仍有同样的技术缺口，未通过完整业务验收。
最终渲染器源码的四个PG隔离验收文件再次25项全通过（25.88秒、无跳过/失败/错误），
日志为acceptance-tests-renderer-fix.*。完整unit首轮2236通过/1失败，失败为另一处旧
purpose标题合同断言；已改用显式subject并保留全部实例/规则/网格/空白/数据不变检查，
该文件40项通过。随后完整unit为2237 passed；BUG-129最终代码完整unit为2243 passed
（156.78秒），最终4个PG隔离验收文件为25 passed、0失败/错误/跳过（35.36秒）。
日志unit-tests-reference-fix.log及acceptance-tests-bug-129.*保留，历史失败日志不覆盖；
这些隔离测试使用模型替身，不能替代真实Word业务验收。

第2轮执行`bid-content-repair-5bc5c45f9fdb4fc9a4fd485127132cc7`继续补第二来源；
首次完整JSON仅有一处constraint_range_ids引用不属本包实际sources，原回包/错误保留，
三次纠正均断连耗尽。其余三份来源成功回包已继承。A0正式归并随后成功，结构F0也
取得有效回包，已形成商务、技术、报价等31个章节的候选正式树；A1四次断连未采纳，
继续使用实际A0树进入写作与正文复核。第二来源未成功的缺口仍在，不因目录改善或
覆盖路由数量减少就判定全技术原文已响应；第2轮Word和后续恢复仍待完成。

第2轮最终32个章节、71个target；A1失败和第二来源缺口仍在。其完整回包中的r64
已按BUG-129定位为本请求规则证据中可见、可绑定的真实冻结引用，顶层结构交接与
表格共同条件共用已读范围造成误拒。新版已分开两种引用域；72项定向回归及真实S2
原回包只读重验通过，A0可无损还原211字符交接原文，已读集合没有增加。此处不是
未知引用或需要模型猜别名。新版Prompt/Schema未在付费运行使用，旧runtime不热改。

第3轮`bid-content-repair-71cce900c84143478c1bf84d7ab3b1c1`于06:05:36 UTC结束，
用时1348.27秒，25次新增发送、10份用量未知、587796已报告Token。第二来源在
一次完整JSON语法失败后纠正成功，四份来源现均正式采纳；A0及A1各四次断连耗尽，
F0实际成功。采用结构仍为“待整理内容”1章、91个target、0分卷，不能作为正式目录。
84个采用单元全placeholder；两张技术响应表17+14共31行，投标人应答均留空。
独立审计确认NAT、IPS、WEBshell和弱口令续文已有真实应答位置；协同防御及跨页
定制开发的两处来源仍未安排，14行续表缺少同等以上配置共同条件的显式关联。
另有空位与真实格式重复、投标函夹带源页首标题，现有F0意见不能当修复已落地。
函件真实承诺/签署正文在Word中完整且无表格，不能再解释为函正文已丢失。
新版导出器独立重导出至new-renderer-round-3/，23处purpose归零，25张表XML、31行
要求/空白、书签、图片及coverage逐值不变；模型0、DB0、旧Word不变。实物详证见
audits/round-3-business-review.json；pending72不是已完成应答数，完整业务验收未通过。

原三轮观察结束后，06:09:18 UTC沿准备好的新身份
`bid-content-repair-3ed9236870e44e46b458c138e5dd6c15`继续第4轮；运行器会继续观察
第4–6轮，未将任意三轮结束当业务完成。四份成功来源已实际继承，不重读原文或
重跑分析；仅恢复未采纳的全局整理，模型、地址、网络及并发配置保持不变。

第4轮A0第二次取得完整回包但JSON逗号语法错误，第四次取得完整JSON后又因
constraint_rule_ids[0]=e17被拒收；e17实际来自本请求submission_rules，是来源已选、
宿主自动保留的编排约束。已定位BUG-130并补最小宿主规范化，不改Prompt、Schema、
请求摘要或正文依据域。12项失败回归先7失败/5通过，修复后58项定向通过；最终
完整unit2256 passed（282.69秒），PG25 passed（37.94秒），均无跳过/失败/错误。
同类人工非form占位泄漏purpose也以一行修复，先RED后83项定向通过；原表/空位保留。

旧driver已沿第4轮结束进入第5轮。确认上述宿主缺陷后，06:30:10 UTC在第5轮两次
请求之间使用官方cancel取消`bid-content-repair-3b35301c6d354b1aa29a4e24390968cf`，
取消时没有sent请求，四份来源及全部receipts逐值保留；不是中断在途调用。第6轮未启动。
新独立证据与运行置于host-recovery-v2/；保留159文件旧冻结基线，只更新authoring.py、
authoring_pipeline.py、authoring_export.py三个宿主模块为已验证源码，manifest及
runtime-provenance.json记录哈希。所有模型Prompt/Schema沿用此前付费合同，因此此轮
验证宿主BUG-128/129/130修复，不冒称BUG-129新版出站Prompt已付费验收，也不热改旧运行。
共享CAS只追加，旧Word及执行结果保留；禁TCP预审通过，06:35:26 UTC启动
`bid-host-repair-b4d9fc04109649cc85ed95ba2281fb99`，previous指向已取消第5轮。
四份来源已实际按完整摘要继承，新增请求从A0开始；当前无修复版最终Word，语义及
正式目录、跨包技术条件、原格式归属仍待真实产物核对，不能以2256项测试代替通过。

host-recovery-v2第1轮实际导出后直接审查（未另重导出）：25表、31条已选响应行
逐值保全，84采用单元全placeholder，无企业填值或附件；内部purpose泄漏0。
投标函原运行页首已不在正文，38份模板部件变37份，固定承诺/签署保留；两套财务
实例仍各夹一处源页首。协同防御两处来源无应答位置、续表共同条件未绑定、重复
空位及1章/0分卷问题仍在，pending72。新增21次发送，543826已报告Token及7份
未知用量；4S均按正式回执及原内容SHA核对。详证在该根audits/round-1-business-review.json。
第2轮沿用成功回包继续，A0/A1新增8次均断连，W/R均可按原摘要复用，未取得正式目录。

只读追链新确认BUG-131：A0失败后保全树91/91个candidate_target_ids为空，宿主已知
映射未交给A1；隔离用例实际证明F0已排除的投标函源页首会因A1省略模板而恢复。
已补有序候选键与宿主编号映射，不改业务语义或新建节点。新增3项先2失败/1通过，
修复后3通过，相关62项定向通过；最终完整unit2259 passed（283.55秒）及PG25
passed（39.69秒）均无跳过/失败/错误。真实A1原回包未采纳，不声称本轮已发生恢复。
07:06:04 UTC在无sent请求时官方取消host-v2第3轮
`bid-content-repair-c30295a896494a4899c90a8f1f77776a`，5份未知请求及全部回执保留。
独立host-recovery-v3/冻结159文件，只更新已验证pipeline并保留旧模型合同及其他
宿主修复；禁TCP预审通过。新执行`bid-host-final-2ac7142f19a244cab3273a8f5126bb51`
以完整host-v2第2轮为previous，以复用四S、F0及W/R；不重读原文或改.env，旧执行、
Word、runtime和全部回执保留。该最终修复版已开始真实恢复，Word业务验收仍待实际产物。

## 持续完整流程恢复（2026-11-03 01:25 UTC，已导出，内容验收未通过）

用户澄清目标为持续跑完整流程，而非每轮retry结束后停。按BUG-126调整本轮外部
验收编排，从最新62请求/37artifacts/6commands、8份成功review的备份完整恢复；
继续同一源版本和领域修订、原冻结runtime/模型/预算，已采纳包不重发。
单command+工作单元仍首次加三次，耗尽后保存回执并沿最新failed命令显式官方恢复，
不自动消除人工问题或把review10/10当最终核验完成。正常分析结束后再以新快照启动
编制，previous指向持有36份可复用成功回包的continuation执行；不复用旧输入或固定旧大纲。
证据根`2026-11-02-complete-acceptance/full-continuation-20261103T012528Z/`，旧六轮
文件、Word、回执和备份全部保留，无真实企业资料，企业应答与报价保持留空合同。
隔离库已实际完整恢复，60表行数与最新备份相同；307个真实CAS复制验SHA，冻结
spec与当前配置精确一致。持续driver的AST、禁TCP预审及纯内存两轮恢复状态推演通过，
01:39:17 UTC启动，PID3084700；失败结算写入recovery-history.json，用量未知单列，
每轮新键及最新失败身份写入recovery-state.json，沿官方入口继续，尚不等于业务通过。
恢复第1轮已实际结算：第6包第3次成功且scope+succeeded交叉确认，第10包4次未知耗尽，
review由8/10增至9/10；7次新增发送、36813已报告Token，另6次用量未知。随后自动创建
第2轮官方命令dcf3cd8c，只继续未完成第10包，原9份采纳不重发。第6包保存18疑点、
2条需补读，尚无生效修订；最终verify和后续编制仍待验证。
恢复第3轮第4次已使第10包成功（原摘要、首数据59.309秒，总94.852秒），基础review
10/10均scope+succeeded确认；随后实际进入locate计划24包，观察时已6包采纳。
不再以review完成终止任务，associate/joint及最终verify继续等待实际产物。
编制helper已AST与禁TCP预审通过，仅在分析正常结束且存在当前verify根后执行；
新execution_id为complete-authoring-full-continue-e7bbf51ff83b40aa9291924412a9a149，
读取新快照，未传固定大纲、无企业资料，不把人工问题自动判为通过。
locate24/24随后全部采纳，已进入associate计划40批/157逻辑单元；中途先保存batch，
整阶段收齐才汇总逐逻辑单元，不能把logical commit暂为0当无进展。已观察到2批采纳。
关联调用既有约61秒断连，也有1次完整DONE/stop的JSON语法失败（BUG-005复发），
已知用量18892 Token，分类保留且沿原额度纠正；补读完成不等于最终verify已完成。
02:43 UTC继续观察：官方恢复第8轮，associate已采纳27/40批、2批在途；
review10/10与locate24/24均按正式采纳及成功回执交叉核对，成功包未重发。
新增一次完整回包缺少findings的宿主拒收已在后续尝试恢复并采纳，不是当前阻塞；
外部约61秒before_headers断连仍发生，用户暂无法取得网关日志，明确要求继续现配置。
宿主编制监督器PID3160281实际存活，等待finished.json后启动continue_authoring.py；
有人工未决时保留原问题、跳过确认并走未确认草稿出口，不伪造人工裁决。
在线备份已在review9、review10、locate24及associate25时保全；备份文件名中的
associate20不代表实际快照计数，元数据明确为associate25，未执行在线恢复或停任务。
当前尚无joint采纳、verify根或新编制产物，BUG-124/125及完整内容验收仍未通过。
03:11 UTC阶段推进：恢复第12轮，associate40/40批全部采纳，157个逻辑单元
均已commit且与succeeded回执关联；joint已生成24请求/43逻辑单元的实际计划，
前2批在途。早前普通Key容量耗尽的429回执明确保存错误码及Retry-After解析值1秒；
与before_headers断连分别计数，未知用量不记零。模型、并发、Prompt和预算未改。
verify根与新编制仍待后续结算，关联提案不能提前当作已接受修订或事项修复。
03:17 UTC核验实际结算：joint24/24批及43逻辑单元完成；verify根已生成
（SHA256 `2e2f6a3b9a76decd734cee00ee56393791c6f9b1cf54385a49d6d0d63f64d6dc`），
coverage.missing为空、incomplete_work_unit_ids为空、hard_blocking=false。
43项决策中28项接受，47条问题消解；仍有590条human_issue记录，不能按记录数
直接认定590个独立业务缺陷。状态waiting-confirmation，未提交确认或伪造人工裁决。
本轮分析实际墙钟5895.858秒（98分15.9秒），新增168次发送：91succeeded、
62uncertain、15failed；成功传输仍须按正式产物核对，未知用量不记零。
编制监督器已自动进入新execution，读取当前verify和新快照、不固定旧大纲，
当前outline调用在途；人工待核保留，沿现有未确认草稿出口继续到Word/ZIP及内容审阅。
新编制已实际完成复核和导出，墙钟1612.994秒（26分53秒），新增24次发送，
6succeeded/18uncertain；已报告264906 Token，18条未知用量单列，未继承旧回包。
来源读取4份中2份成功、2份各4次断连耗尽；A0归并及一次A1调整也均4次耗尽，
采用保全候选结构继续。结构复核、2次写作和正文复核实际成功，不代表正式结构已通过。
最终32个采用单元为31placeholder/1generated；唯一生成段落仍复述“技术文件应按照
招标文件……编制”等规则，BUG-125再现。outline有209条未安排规则、18处模板缺失。
最终coverage347条：resolution为manual17/content96/constraint25/pending209；
review为not_required42/unreviewed284/reviewed21，判定为unsupported18/partial3，
其余326无判定。content96是路由类型，不等于96项应答已完成；无企业资料引用。
Word40182字节、ZIP262589字节，保全检查原执行/历史文件/冻结runtime均为true。
本轮已沿同一运行从剩余核验持续跑到编制导出，但人工确认未完成，内容验收未通过；
不能把技术succeeded或缺口仍在CAS/ZIP解释为投标正文完整。最终只读产物审计及
可实际还原的完整备份继续保存到本轮证据目录，未改变.env或生产代码。
最终完整备份complete-flow-final.dump为425033字节，SHA256
`feb419f74009069fe589273b75f70283116cd020daebdf180a224fa35cefa65f`；
已在本次新建无网络/无宿主端口的隔离PG实际还原，60表行数及逐表全行SHA全部相同。
最终分析230请求/1075artifacts/18commands，编制3executions/81请求，confirmations=0；
仅移除了本次精确创建的还原验证容器，原本轮PG/Redis及旧回执、Word均保留。
快速实物检查确认CAS/DOCX/ZIP完整，但采用大纲只有1章/33targets，response_table=0、
原生template引用=0，Word实际表格=0；无技术参数响应表与原格式承载。
280条requirements中234条无target（含25个constraint位置），其中209条应答待安排，
不能将无target总数直接解释为全部缺应答。缺口在宿主覆盖中可查，投标正文仍不完整。
final-artifact-audit.json已保存：381个可达CAS、下载文件与ZIP一致，原执行及历史文件
保全通过。来源范围重规划明确skipped/not_verified，仅按真实请求回执核对来源2/4成功、
A0/A1均未采纳；projection通过仅说明280条规则仍在台账，Word通过仅相对于当前
缩水大纲，不构成原件模板/技术响应表的完整性验收。最终overall未验证、业务未通过。
本轮问题补充BUG-011/124/125/126，并新增BUG-127记录来源/归并失败后的正文承载缺口；
未修复产品代码或改变冻结执行，git diff --check通过，未提交或推送。
final-business-audit.json/md进一步确认：209条pending均无target及采用内容，Word无
要求/覆盖保全附录，这些要求只在JSON/ZIP侧车保全，不能算正文已承载。原件第五章
正文的存在性尚待定位，仅找到引用，不沿用模型“用户未提供”的判断。

## 核验再重试（2026-11-03 01:02 UTC，已结束，review8/10）

用户要求继续重试，沿同一run/source_version/domain_revision，从上一轮已实际还原
验证的analysis-retry-real.dump恢复53条分析请求、34个artifact、5个command及7份
已采纳review；以5ba88d75失败命令为基线，仅创建一次稳定幂等官方retry。
先核对当前配置与冻结pin，再完整恢复隔离PG/Redis并逐表核对；成功7包不重发。
首次加三次、并发2、180秒超时及阶段停止策略不改，不启动编制或伪造人工确认。
新证据目录`2026-11-02-complete-acceptance/retry-20261103T010203Z/`，保留五轮旧文件、
Word及全部回执。review达到10/10也不等于完整核验完成，仍须跟踪后续补读/关联与verify根。
禁TCP预审通过，当前配置与冻结执行spec精确相同；完整恢复60表行数一致，302个真实
CAS对象复制并验SHA。01:04:17 UTC正式启动，唯一新command为a12867e4；实际墙钟
360.639秒（6分01秒，不含恢复准备）。新增9次发送：第9包首次成功，第6/10包各4次
uncertain耗尽。仅新增第9包采纳，已逐scope核对committed artifact与同command的
succeeded请求，review推进至8/10；原7包成功结果均未重发。8次未知仍在60.953–61.661秒
before_headers断连，异常链APIConnectionError→RemoteProtocolError，未取得可用正文，
外部具体关闭环节仍待查。第9包首数据59.286秒，已报告27097输入+1435输出=28532 Token；
另8次用量未知，不能记零费用。每个command+工作单元仍不超过4次。

最终analysis failed，无verify根、confirm或analysis-ready；未进入locate/associate/joint、
编制或导出，不自动追加retry。第9包保存3条疑点且均需补读，accepted_decision_ids为空；
没有生效事项修订，BUG-124/125继续开放，完整业务验收仍未通过。
旧53回执/34artifacts/5commands逐项存在且SHA未变，五轮旧文件及Word保全；
最终62条分析请求/37artifacts/6commands，编制仍2次执行/57请求。
完整备份analysis-retry-real.dump为248657字节，SHA256为
`95a415aefa166c3b0f96a4b1d22cecb293740c83e412ae76b602070286149c36`；
已实际还原新库、60表行数相等，仅按精确ID移除本轮两个tmpfs容器，其他容器保留。
详见新目录finished.json、analysis-observer-final.json、database-backup.json、cleanup.json
及verification-summary.md/json。未修改生产代码、.env、模型、网络、预算或超时，未提交/推送。

## 核验继续重试（2026-11-02 09:31 UTC，已结束，核验7/10）

用户要求再重试一轮，沿同一run/source_version/domain_revision，从上一轮实际还原
验证通过的analysis-retry-real.dump恢复45条分析请求、31个artifact、4个command和
5份已采纳review；以96d154f7失败命令为基线，仅创建一次稳定幂等官方retry。
继续原冻结runtime与原配置，首次加三次、并发2、180秒超时和阶段停止策略不改；
成功5包不重发，不启动编制或伪造确认，也不自动循环创建额外命令。
新证据目录`2026-11-02-complete-acceptance/retry-20261102T093102Z/`，旧四轮文件、
Word和全部回执保留。禁TCP预审通过，模型、端点、Prompt、预算及原pin精确一致；
隔离PG/Redis完整恢复、60表行数逐一相等，297个真实CAS对象复制并验SHA。
09:33:41 UTC正式启动，唯一新command为5ba88d75，实际墙钟328.037秒（5分28秒，
不含恢复准备）。新增8次发送：第6包4次uncertain、第7包第3次成功、第8包首次成功。
第7/8包均逐scope交叉核对committed artifact与同command的succeeded请求，采纳增至7/10；
原第1–5包不重发，第9/10包因阶段停止未发送。6次unknown仍在约61秒before_headers断连，
异常链APIConnectionError→RemoteProtocolError，无可用正文，远端具体关闭环节仍待查。
第7/8包首数据分别55.327/37.040秒，总耗时100.936/61.845秒；第7包原摘要重试成功。
已报告43402输入+6953输出=50355 Token，另6次用量未知；每command+工作单元最多4次。

最终analysis failed，无verify根、confirm或analysis-ready，未进入locate/associate/joint，
未启动编制、导出或自动追加retry。新增review分别15/9条疑点，共24条，其中1条needs_source；
accepted_decision_ids均为空，没有生效事项修订，BUG-124/125继续开放，业务验收仍未通过。
旧45请求/31artifacts/4commands逐项存在且SHA未变，四轮旧文件和Word保全；
最终53条分析请求/34artifacts/5commands，编制仍2次执行/57请求。
完整备份analysis-retry-real.dump为247291字节，SHA256为
`2c28531749c04f297429142a4b8072cb4445d015d56219429d61a28334f63e74`；
已实际还原到新库、60表行数相等，仅按精确ID移除本轮两个tmpfs容器，其他容器保留。
详见新目录finished.json、analysis-observer-final.json、database-backup.json、cleanup.json
和verification-summary.md/json。未修改生产代码、.env、模型、网络、预算或超时，未提交/推送。

## 继续核验重试（2026-11-02 08:42 UTC，已结束，核验5/10）

用户在核验推进至4/10后要求继续重试。本轮analysis-only沿原run/revision，
从最新已验证analysis-retry-real.dump完整恢复，60表行数一致，保留34条历史请求、
29个artifact、3个command及4份成功review；以e2b063d9失败命令为基线，
仅创建一次稳定幂等官方retry。原冻结runtime、模型、Prompt、预算、180秒超时、
并发2及首次加三次额度均不变，不重复编制或自动确认。
新证据目录`2026-11-02-complete-acceptance/retry-20261102T084200Z/`，
原三轮回执、产物及备份保留，隔离PG/Redis使用本机随机端口、原pgvector镜像及tmpfs。
唯一新command为96d154f7；实际核验08:45:23–08:53:55 UTC，墙钟512.024秒
（8分32秒，不含恢复准备）。新增11次发送：第2包第3次成功、第6/7包各4次
uncertain，最终采纳第1–5包共5/10；原第1/3/4/5包未重发，第8–10包因阶段
停止未发送。新增采纳已逐项交叉核对committed scope和同command的succeeded回执。
10次未知结果均在60.902–61.494秒before_headers断开，异常链仍为
APIConnectionError→RemoteProtocolError；未取得可用正文，未进入Schema或语义拒收。
第2包保持原request_digest，从历史多次失败转为成功，首数据48.876秒、总94.375秒；
进一步说明不是固定非法请求，外部具体断链环节仍待查。已报告24482输入+4079输出
=28561 Token，另10次用量未知，不能记零费用。每个command+工作单元不超过4次。

最终analysis failed，无verify根、confirm或analysis-ready，未进入locate/associate/joint，
未重跑编制、导出或自动追加retry command。新增第2包保存13条疑点（4条needs_source），
accepted_decision_ids为空；只读审计确认未直接触及BUG-124已知锚点，不能据此宣称
遗漏或正文已修复。BUG-124/125继续开放，真实完整业务验收仍未通过。
旧34请求/29artifacts/3commands逐项SHA未变，原三轮回执、Word及文件保全；
最终45条分析请求/31artifacts/4commands，编制仍2次执行/57请求。
本轮完整备份analysis-retry-real.dump为246035字节，SHA256为
`c05d0fba885245d081da129bff0e3008df0c36da19cb1a63bec6f86504f6f9e1`；
已在新库实际还原，60表行数相等，仅按精确ID移除本轮两个tmpfs容器，其他容器保留。
详见新目录finished.json、analysis-observer-final.json、database-backup.json、cleanup.json
及verification-summary.md/json。未修改生产代码、.env、模型、网络或预算，未提交/推送。

## 再次核验重试（2026-11-02 UTC，已结束，核验推进但未完成）

用户在根因复核后明确要求“再重试一次”。本轮仅重试核验，沿同一run/revision，
以最新失败command为基线创建一次稳定幂等的官方retry，不改变首次加三次额度。
保留原23条分析请求、25个artifact、2个command及全部编制结果；4抽取包和2份
成功核验继续复用。若核验再次耗尽，本轮即结束，不重复发送未变化的A0/A1。
模型、Prompt、预算、180秒超时、并发2及冻结runtime与原pin精确一致，未改.env。
新证据目录`2026-11-02-complete-acceptance/retry-20261102T081806Z/`。

隔离恢复使用上轮已验证continuation-real.dump，60表行数逐一相等。
准备时就绪检查曾提前，后改用目标库实际TCP连接；普通Postgres缺vector扩展，
已仅替换本轮新建空库容器为原pgvector/pgvector:0.8.6-pg16镜像，错误输出保留。
当前PG/Redis均本机随机端口及tmpfs，未触碰开发栈。预审及实际还原完成后才发送。

本轮真实核验墙钟478.628秒（7分58.6秒，不含恢复准备），新增11次发送：
第2包4次uncertain、第4包第2次成功、第5包首次成功、第6包4次uncertain。
新增采纳仅第4/5包，原第1/3包复用，最终4/10；第7–10包因阶段停止从未发送。
曾按总数误把第5包认作第2包成功，已向用户纠正；最终结论只认逐scope采纳回执。
9次uncertain均before_headers、APIConnectionError→RemoteProtocolError，
60.845–61.543秒，宿主未取得可用正文。第4包同摘要从失败转成功，首数据55.702秒；
不是固定非法请求，但外部具体断链环节仍待查。本轮未改模型、网络、预算或源码。
每个command+工作单元最多4次；第2包累计12次来自3个用户显式授权命令各4次。
已报告38287输入+4317输出=42604 Token，另9次用量未知，不能记零费用。

最终analysis failed、没有verify根或confirm；未进入locate/associate/joint或编制，
未重跑抽取、原已采纳核验、大纲、写作或Word导出。旧23回执/25artifacts/2commands
逐项SHA未变，原两轮Word和全部文件保留。最终数据库34条分析请求/29artifacts/3commands；
详见新目录finished.json、analysis-observer-final.json及verification-summary.md/json。
新增第4包提出14条疑点（其中3条needs_source），第5包无疑点；已明确识别BUG-124
中i96报价修正例外遗漏。但新增accepted_decision_ids为0，无事项修订或正式确认，
仅识别疑点不等同补齐要求；见independent-review-delta-audit.md/json。

本轮完整备份analysis-retry-real.dump为244729字节，SHA256为
`5330cf0e481cd78792225785bce088bc27fc6c7c07916ac610770b1e7ce43956`；
已在临时新库实际还原，60表行数相等。仅移除本轮两个临时容器，无附加卷需删除，
其他容器及历史备份保留，见database-backup.json/cleanup.json。未部署、提交或推送。

## 核验2/10根因复核（2026-11-02 UTC，未新增真实请求）

用户要求继续重试后，明确先定位核验为何停在2/10；尚未创建新retry command或模型请求。
逐包核对纠正表述：10包中2包成功并被复用、2包尝试耗尽、6包从未发送。
原轮实际发送第1/2/3包，续跑新发送第2/4包；第2包两个命令各4次，第4包仅续跑4次。
全部10包fits=true。不是“剩余8包都失败”，也不是预算门禁拦截。

两轮verify共15次发送，2次成功、13次uncertain；13次均在SDK未交回响应对象前以
APIConnectionError→RemoteProtocolError结束，耗时60.862–61.601秒，宿主未取得正文。
同一第1包的首次发送也这样失败，原摘要重试成功，首数据53.148秒/总79.808秒；
第3包首数据53.394秒/总140.689秒。失败第2包估算24688输入/97017字节，
小于成功第1包的32705输入/145977字节；缺乏以包过大或上下文超限解释断连的证据。
本地180秒IdleDeadline、SDK max_retries=0及httpx read=None与冻结runtime一致。
约60秒首响应断链是外部调用链待核假设，具体网关、供应商层或链路关闭原因仍未定位。
before_headers及正文0字节不等于网络抓包结论，未持久化provider请求ID不等于对端未提供。

直接导致2/10停止的宿主链条已确认：单元四次耗尽→_Analysis.execute设置阶段stopped→
排队包取得额度后直接返回→等待在途兄弟结算后整阶段抛错→runner结算failed。
verify_analysis未继续locate/associate/joint，也未返回verify根，故未走到confirm。
stopped来自BUG-032的失败控费设计；它使前部持续失败能够反复阻止后部独立包启动。
不能仅靠继续重试宣布覆盖补齐；调度策略和外部断连要分别评估，当前只定位未改源码。

既有失败停止/成功兄弟保全定向回归2项通过（1.96秒）；默认sandbox首跑等待异常后
已精确终止，受限时长隔离重跑通过，不能把该本地执行环境现象混作模型断连。
传输归因的纯MockTransport反证5项通过、0真实TCP，用模拟时钟区分61秒断连与
本地180秒deadline；这证明错误分类不同，不证明真实供应商内部原因。
现有传输诊断与首包超时定向回归16项通过（1.80秒，禁止TCP），不代替真实网关日志。
诊断证据继续保存在续跑目录的verify-transport-root-cause-audit.json及
verify-review-root-cause.md/json，真实运行、旧回执、Word及数据库备份不变。

## 同修订续跑（2026-11-02 UTC，已结束，完整业务验收仍未通过）

用户在完整验收未通过后要求“继续跑一下”。本次恢复上轮完整备份至新的独立PG/Redis，
保留原run/source_version/domain_revision、LangGraph检查点、15条分析及49条编制请求。
当前Chat/VLM模型与端点、预算、来源解析spec经离线比较均与原修订完全一致。
沿官方Store.retry创建一次稳定幂等的显式retry command，只给未完成工作单元新的
首次+3额度；旧命令回执不改，成功OCR/抽取和2份核验不重复付费。不是自动循环换键。
分析后按实际核验状态继续编制，有人工未决不伪造confirm；新编制执行显式previous，
不传旧fallback outline/plan，来源/写作仅在完整摘要匹配且重验通过时继承。

新证据目录为`2026-11-02-complete-acceptance/continuation-20261102T072705Z/`。
备份已实际完整还原（旧备份此前仅做目录验证），检查点及逐阶段回执已核对。
续跑已启动：原4包抽取和2包核验保留。两个未完成核验单元各4次均在响应头前
断开，本命令新增8条uncertain回执，仍仅2/10包采纳；没有verify根或确认。
沿既有未确认草稿出口继续编制，不再自动创建新的retry command。
旧成稿、旧汇总与原始回执保留，未修改生产代码、.env或网络。

本轮真实墙钟848.414秒（14分08.4秒）：分析291.490秒、编制556.794秒。
新增16次发送/4个所属执行内的逻辑单元/12次重试：核验两个单元各4次，
A0、A1各4次；全部在before_headers出现APIConnectionError→RemoteProtocolError，
用量全未知，不能视为零费用。未收到可采纳的新增语义结果，远端具体断开环节仍待查。
既有4包抽取/2包核验不重跑；编制36份成功回包经完整请求摘要一致及现行校验后继承，
分别为S3、结构复核1、初写25、改稿5、两轮正文复核各1，无新增正文写作/复核调用。

新Word/ZIP已导出，但A0/A1均未采纳，仍是1章/96目标的保全大纲。
新旧最终outline_ref及93份采用unit_refs完全相同，均86份placeholder/7份generated；
初写检查点87/6不能代替最终采用统计，既有w67改稿解释此差异。
Word全部解压成员及压缩payload相同，仅ZIP成员时间不同，本轮没有内容质量改善。
载体保全、42条响应行及4条共同条件检查通过；BUG-124细节遗漏与BUG-125规则复述/
编制说明进入正文仍开放，没有verify根、confirm或analysis-ready，不能作为可提交稿验收。
无企业资料，知识与报价继续留空，不代表真实知识命中或真实Office排版验收。

本轮执行成功结果重放1.974秒，模型构造/付费调用均0，请求8→8、回执134→134；
36份继承完整请求摘要逐条匹配。旧49请求/172回执及新旧产物摘要均不变。
此项只证明成功结果复用，不代表中途恢复或业务内容验收。独立审计与调用统计分别见
新证据目录的`independent-continuation-conclusion.md`、`continuation-call-statistics-final.json`；
汇总见`verification-summary.md/json`，全部原始回执和旧运行保留。

本轮数据库已完整备份为`continuation-real.dump`（242903字节），SHA256为
`42e728ee55a833e3d71ae7fafe4677b89c41cde975f9f7da956910f0b91a542b`；
在临时新库实际pg_restore成功，60张表行数逐一相等。本轮PG/Redis两容器已按ID精确
移除，其他容器保留，镜像声明的Redis匿名卷保留，未删除卷，见database-backup.json/
cleanup.json。未重新运行上轮已完成的全量隔离回归，未部署、提交或推送。

## 完整验收复跑（2026-11-02 UTC，已结束，完整业务验收未通过）

本轮从同106页原PDF新建独立分析，使用当前已授权api.kunkun-gpt.com:8317/v1与
Chat/VLM gpt-5.6-sol，未改.env、预算、并发、网络或生产源码。无真实企业资料，
仅验证留空及受控知识链路；未复用旧分析或固定大纲，不伪造人工确认。
证据根`/opt/workspace/kb-tender-evidence/2026-11-02-complete-acceptance/`，
汇总`verification-summary.json/md`，独立业务结论`independent-final-acceptance.md`。

当前完整源码unit/workflow/acceptance同轮2576项通过，0失败/错误/跳过，1062.76秒；
356份源码/测试/Prompt/迁移/依赖文件前后SHA256一致。前端80项、build/lint通过；
Chromium首轮132通过/2条旧断言失败，BUG-123仅修正测试后8项定向、134项完整复跑
通过，0跳过，失败证据保留。Rust API库28通过/9项既有环境依赖测试未执行。
生产SQLx migrator独立新库及重复执行验收1项通过，版本/checksum登记且业务数据保留。
受控知识定向114项及真实loopback HTTP纵切通过（持续503至多4次，成功复用0发送），
不代表真实企业资料命中；浏览器Office为fixture，不代表真实Office保存或排版验收。

真实解析520原生单元/796范围，98图片位置共2个不同SHA，均补读成功；抽取4包全部
采纳，共280事项/32关系/796处置，其中221范围仍未决。核验计划10包，仅2包采纳，
另一包连续4次before_headers后额度耗尽，停止新兄弟请求并保留成功结果；未进入
locate/associate/joint，没有verify根、confirm或analysis-ready，分析最终failed。
已有核验45条blocking findings及15条补读需求不能当作完整核验已完成。

沿现行缺口合同，未确认快照继续编制。3包S均采纳；A0四次、A1四次均before_headers，
无有效正式自动归并。F0成功（108344输入/2604输出）并保留12项结构意见，实际最终为
1章/96目标的待整理fallback，不是正式合格大纲。93个单元完成（86占位/7generated），
25次初写、5单元一次改稿和两轮正文复核有采纳回执；一次改稿JSON失败已同额纠正。
Word/ZIP已导出，分别在`real-full-python/投标文件.docx`与`投标文件.zip`，旧稿不替换。

只读实物审计通过已绑定载体保全：321文字、23网格、16图片、1空白对象共361次，
唯一来源分别296/18/1/1；42条响应行及4条共同条件逐字保留，企业应答与报价安全留空。
280初次规则保存在requirements及437覆盖记录中（75待安排/328未核），原件及6份旧回执/
Word/ZIP摘要未变，冻结runtime未变。不能由这些结构事实宣称语义完整：评分机构、
盈利依据、条件提交及标准适用细节仍有缺口（BUG-124），部分正文仍是规则复述与
编制说明（BUG-125）；全部技术义务、正式结构及可提交稿质量未通过。
字体/分页、真实企业匹配和真实Office保存未验收；具体证据见13组细节追踪及独立报告。

真实墙钟4011.726秒（66分51.7秒）：分析1659.243秒、编制2352.409秒。
64次发送/47逻辑/17重试，44成功/2明确失败/18未知；所有逻辑单元均<=4次。
供应商已报告784004输入+157774输出=941778 Token，18请求用量未知，不记零费用。
分析最大输出22013超过16384预留，但预留不是Wire硬限，本轮未调参数。
A1最大输入估算149921；F0超本地估算仍成功，不能以超估算或断连推定服务端上下文不足。
成功结果原生重放约1.94秒：请求49→49、回执172→172，模型构造0、文件/回执摘要不变；
此项不是中断恢复或语义验收。队列重投隔离测试通过，本次真实driver直接执行生产函数，
未验证真实队列传输或整套网页端到端。没有因失败重置额度、换execution或伪造确认。

独立accept_real已完整pg_dump，225566字节，SHA256为
`c3ac3a1471f21a50c510b96e879e52668368917c9e5a2d0827e08511ff1713a6`，
pg_restore --list验证通过，未另做整库还原演练。仅本轮PG/Redis两容器已精确清理，
没有挂载卷需删除，其他容器ID全部保留，见database-backup.json及cleanup.json。
本轮新增修正仅两份过期浏览器断言及问题/交接记录，无部署、提交或推送。

## F8范围修复实施与真实验证（2026-11-02 UTC，草稿已导出，大纲语义未通过）

候选内部范围选择已接入A：按实际选择重建规则、精确摘录及来源，未采用部分独立
保全；修订继承不能借改变主体、候选集合或载体重新展开整个候选。F已能读取逐目标及
响应行的完整规则语义。精确检索按实际范围与共同条件匹配，缺适用意图时仅用完整
精确原文补查。结构compact_v2按section和原支持身份无损分组，不改模型、预算、
输出预留、并发、网络或修订轮次。完整单元2231项通过（152.92秒）；隔离PG/Redis的
workflow及acceptance共345项通过（782.27秒），均0失败/错误/跳过。345项完成后新增
BUG-122的宿主提示清理修复，31项定向及上述最终2231单元通过；受影响的两个编制
入口另跑23项通过（24.95秒），独立复审无新增问题。没有重复把23项相加为新的用例总数。
当前真实运行使用此前冻结runtime，未热改；BUG-122只影响A1成功后的过期提示结算，
不改变模型请求、Schema、原回执或收费状态。修复前3失败/1通过与最终结果均保留。

只读预审确认相同文件当前6包S的完整摘要均匹配旧v11接受回执，且全部通过当前
validator，可复用；0新HTTP、0 CAS写。新A含规则语义后raw估算237051输入Token、
938087 Wire字节，现成compact_v1为180701 Token、711427字节，仍超当前有效输入
94208。旧冻结全树F含238条target_rules、1090个supports，compact_v2为137598
Token、533205字节，同样超估算；它不是新自动A产物，不能代表新树的最终F大小。
旧93669试算未含本轮新增规则，已不能当作新合同可单包的证据。仍沿现有授权发送/
待核草稿出口实测，不把估算超限当作远端已拒收，也不把压缩实现当整稿语义验收。

逐字段容量归因确认compact A的payload为173042 Token，candidate_rules占22.38%、
template_parts占16.84%、chapters占11.22%、unassigned_rules占11.16%。475次候选
引文中447次是共享原文的精确子串，但候选语义text仅9/234条是共享原文子串，不能
直接删除规则语义。把候选及未安排规则的quote纳入现有文字池，内存试算仅再省3449
输入Token；再共享语义text合计省3648，约2%，未实施，不能作为容量问题已解决的依据。
全量候选规则、模板、摘录和未安排语义仍随文件内容累积，现有精确共享不足以限制
完整请求规模。仅字节/Token统计保存在证据根的`assembly-capacity-attribution.json/md`。

本轮证据根为`/opt/workspace/kb-tender-evidence/2026-11-02-scope-fix/`，预审报告在
`real-auto-no-knowledge/preflight.json`，正式脚本为根目录`run_real_auto.py`。
真实执行`scopefix-real-46e49e9bff1c48498966ea9fd211a70d`固定原分析输入，不传旧
outline_ref；6包S按完整摘要复用并重验通过，不覆盖旧运行和Word。A0四次均在约
60.95～61.96秒于before_headers断开，没有形成采用结果，用量未知；不能据此确认
供应商是否执行或收费，也不能认定断流由本地容量超估算造成。

A0不可用后保全候选进入实际F，其compact_v2请求为144502输入Token、563669 Wire
字节，含234条target_rules、1154个supports及324条submission_sources；这是未归并
的待整理候选树，不是成功新A树。该F虽超本地配置估算，仍完整成功并通过宿主校验，
供应商报告prompt144296/completion4310/total148606。由此只能确认该实际请求的
传输及返回合同可用，不能断言服务器容量不足，也不能代替全树语义验收。

既有A1首次已收到done/stop完整流，供应商报告193765输入/11245输出/205010总Token，
但JSON在EOF缺容器关闭符，未采纳；其余三次before_headers断开，用量未知。A1也未
形成接受树，不能把待整理候选称为标题/职责范围已修好。仅内存补唯一关闭符后的
诊断对象仍把excluded_targets/constraint_rule_ids放入第三章，原Schema两处拒收；
未修改或采纳原回包，也不搬移字段过关。诊断对象只有56个目标、未用candidate_scope，
不能以其假称新的范围分配有效，实际保全树仍有122个目标/823摘录/234规则。

流程已于1729.573秒（28分50秒）完成无资料编制及Word/ZIP导出：109个单元、24次
初写及3次既有修订写作，3次正文复核完成，末次复核另一次断流后在同额度恢复。
共40次新发送（7次retry），8次用量未知；供应商已报告1063314输入/46632输出/
1109946总Token，未知8次不能按零费用结算。6包S全部复用，不计作本轮新发送。
原分析与6份旧文件摘要不变；不能将execution=succeeded
或Word已导出当作正式大纲/整稿合格。无真实企业资料，本轮仅验证留空路径，不能声明
真实知识库召回与企业应答语义通过。产物在`real-auto-no-knowledge/投标文件.docx`及
`投标文件.zip`，明确作为待整理待核草稿，旧稿不替换。

本轮隔离数据库已备份为`f8-completed.dump`，367046字节，SHA256为
`58142b26da80fcba90f430f8c1c5778be711782a99d159404ca983103b956790`，
pg_restore目录验证通过。仅本轮两个PG/Redis容器及其一个匿名卷已精确清理，回执在
`cleanup.json`；共享服务未动。运行冻结源码清单仍逐项相符，当前源码仅BUG-122的
宿主结算差异已记录`runtime-current-comparison.json`和diff；无部署、提交或推送。

最终Word/ZIP只读保全审计通过，自动结构与scope语义总体为not_verified：361次明确
模板出现均完整（326文字、24原生网格、10图片、1空白文字），唯一引用/原图分别为
303/19/1/1；不能把重复出现数当不同原图数。24条技术要求及3条共同条件内容/次序
完整，24行企业应答均空；无知识填写和报价未自动填值。122候选的234规则、823摘录
及全部源引用仍在，最终保全树与只读回放一致。该核对不表示所有理解上下文均须印入
Word，也不表示原字体、分页、样式或规定分册已经通过：当前无成功自动分册树。
审计报告在`real-auto-no-knowledge/word-scope-preservation-audit.json`，新增模型/DB/
CAS写入0；总验收回执为`verification-summary.json`。BUG-005/092回包质量及BUG-069/
062正式职责范围/标题验收仍开放，不因保全和导出成功勾选为已修好。

## 剩余修复设计核实（2026-11-02 UTC，实施前记录）

已按本轮用户“怎么修复”补齐tasks/plan.md的F8方案、tasks/todo.md及拟议字段合同。
本轮仅改方案/问题记录，未新增生产实现、模型调用、部署、提交或推送。

逐S/A回执确认：复合候选在S产生，A0仍承担复合范围；A1改为环境用途却继承全部
原文和规则，协议目前无法拆候选内部范围。方案增加单个纯叙述候选的可选scope，
同时重建三类引用、保留未安排内容，并修同unit精确检索串绑；不按标题拆文或增加A2。
旧固定纲/人工Word保护不变；标题须在新的自动A中验证，重导出旧纲不算修好。

完整F的内存容量试算使用当前真实RequestBudget.measure_request：原输入123260、
Wire494334字节；按section分组/原ID为键及省明确默认值后，含视图说明输入93669、
Wire357812字节。context131072、output_reserve32768、safety4096未改，总估算
130533，剩余539。此为旧冻结全树的只读试算，没有展开等价测试或真实发送，不能
当实现/语义验收；新增scope/最终Schema及说明后须重测。当前完整F已做文字行共享和
网格压缩，不是压缩未启用；本例19个grid均无非空text双载，不能归因于表格重复全文。
先实施这一种无损视图，暂不增加分包器，继续保留超估算可发送行为。

传输事实已纠正：原形全树前三次before_headers、第四次reading_body/ReadError，
后者53.806秒取得首数据。部分流没有形成可接受结果，审计CAS大小不代表原流字节数。
容量超估算已确认，远端断流具体原因仍未确认；两项不合并称为一个已解决根因。

## BUG-069/013续修（2026-11-02 UTC，本地验证）

最后自动结构复核F1已可返回显式`chapter_moves`，宿主原子校验并移动完整子树，保留
章节/响应/行身份、原文和模板。仅A1改树后既有F1启用，不增模型轮次；A1不变仍留原
观察待核。固定/人工大纲、已保存Word及旧成功运行不自动改动。传输回执新增有限异常
类型链、整数errno和取得响应头前/读正文阶段，不存异常自由文本或密钥，不改重试/网络。

新结构20例、接线到Word两例、诊断13例均先红后绿；结构合同80项、客户端289项相关
回归通过，均包含在当前完整unit的2181项中，不能重复相加。完整unit用时149.14秒，
0失败/错误/跳过；隔离PG另验authoring_store 31项、编制入口及来源20项通过。实现/新
测试Ruff及diff检查通过；schema.py的三个HEAD既有F401重导出未在本轮删改。交叉只读
复审未发现本轮新缺陷，明确A1无变化不走F1是既有限制，不冒称所有层级都自动修好。

真实范围是同106页文件冻结输入的结构复核，非重新跑五节点或完整编制。当前已授权
端点及gpt-5.6-sol保持不变，.env未修改。首次全树脚本补入模型默认空字段，出现无效
的旧CAS对象相等判定；原运行和回执保留。已改为读取生产原始JSON复验，全树后继
`hierarchy-validation-8f1e2f449ca8447d83edf1dab3f75c06`耗时264.856秒、4次均uncertain，
前三次before_headers发生APIConnectionError→RemoteProtocolError，第四次53.806秒
取得首数据后在reading_body发生ReadError；均未形成完整可接受结果，沿四次额度留
structure_review_unavailable。输入/候选为同一CAS，分析、旧大纲、旧Word及ZIP均不变。
只读模型目录HTTP 200，当前模型在目录中；仍不能断定是哪一段远端连接关闭，也不能
从本轮倒推上轮20秒断流的内部原因。不改代理、TLS、模型或180秒空闲策略。

独立真实技术子树核对`hierarchy-validation-26fd657205f74c119fa4770e409ad8fd`耗时
198.934秒，前两次uncertain、第三次成功；14934输入/3043输出/17977总Token为唯一
已报告用量。模型依据硬件规格实际正文选中唯一移动：chapter-2.3.2.5归入
chapter-2.3.2.4，置于chapter-2.3.2.4.1之后。独立逐原文核对确认正确，子树13章、
15目标、全部引用与原文保全，24技术行及3共同条件不变。三次验证合计11次新发送、
10次用量未知，不能按零收费；首次脚本及原形全树重验均不算语义成功。

已将该已接受的明确动作离线重放到完整原始大纲副本，生成独立Word/ZIP；不重写正文，
不覆盖旧稿，覆盖统计仍属于原采用正文。Word稳定书签确认该节为硬件章的下一级，
大纲72章/87目标/339模板选择/24行/3共同条件/3分册保全。进一步对主Word及所属分册逐项
核对678处模板（608文本、52网格、18图片字节）完整；技术行和共同条件在总稿及技术
分册均完整，企业应答和报价仍留空。旧标题含源编号未擅自改写，不能把父子修复说成
标题编号已清理。另核实环境目标混入硬件原文，后续追链根因及方案见本文顶部；混合证明来源
可作为汇编标题，索引冲突也不直接等于漏文。上述情况已更新BUG-069，整稿语义未验收。

本轮证据根为`/opt/workspace/kb-tender-evidence/2026-11-02-hierarchy-fix/`，原形全树
在`real-structure-v2/`，成功子树在`real-structure-scoped/`，离线Word/ZIP及实物审计
在`reexport-scoped-move-v2/`（`audit.json`、`word-preservation.json`）。首轮验证脚本
的默认值比较及第一次重导出编号断言失败均保留；后者误把不编号封面计入显示章号，
已改为按Word实际稳定书签验证父子级别，未为让验收通过修改生产渲染器。
本轮唯一隔离PG已按完整容器ID精确清理，其他服务未动；删除前完成备份并校验
pg_restore目录，`hierarchy-completed.dump`为342457字节，SHA256为
`d06927e500fe78927964c7a57d56468af7ab3a8563b65e1c5658ea7b9dcc6ed0`。
最终汇总见证据根`verification-summary.json`，清理记录为`cleanup.json`。
本轮无业务部署、提交或推送。

## BUG-115—121评审修复（2026-11-01本地，技术验证完成）

F0—F5实现及定向回归已接入，保留既有模型轮次、预算、并发和无资料留空出口。
统一有序资料短引用、document-plan/逐单元恢复、逐知识操作持久重试、逐要求有效
assessment结算，以及人工Word覆盖待核。字段合同和具体根因见字典及各BUG实施结果。
定向结果：编制执行19项、实际管线计划恢复4项、覆盖相关54项、知识74项单元及
31项隔离PG通过；Rust版本适用性2项、Chromium工作台14项、前端build/lint通过。
完整unit/workflow/acceptance重跑2489项通过（1022.66秒），0失败/错误/跳过。首轮
2478通过、5失败（1024.33秒），为三处
替身缺spec、一处HEAD既有role身份断言漂移和一处补充来源替身未接新参数。均按真实
接口及project/document/hash合同修正，不改生产策略；相应定向复验38/20/1项通过。

真实文件在独立隔离库、固定旧分析输入及大纲上完成编制，不能算重新跑完五节点分析。
执行`reviewfix-real-859b2d6824a54bd1bc4436cddfff4820`用时386.49秒，提交63个单元，
输出Word/ZIP及三个分册；旧分析和原Word保全检查均通过。使用最近已授权的地址及
gpt-5.6-sol，无企业资料或知识库，企业应答/报价留空，未修改.env。17次新发送为1次
写作成功、4次结构复核和12次正文复核uncertain；后两者在约20秒内未收到响应内容而
断开，四次额度耗尽后保留review_unavailable。已知usage为19950 tokens，16次未知
不能按零计；累计输入估算1319937不代表供应商实际计费。DNS/TLS/HTTP可达且写作
成功，具体断开原因仍待查，不归因于历史证书故障，不擅改网络或模型配置。

实物已核对24条技术要求、3个共同条件在主文档及技术分册保留、响应列留空，433条
规则覆盖台账可重算。审计额外发现并修复BUG-121：一个非form目标显式选择的四段
固定原文被导出器忽略。六项回归先红后绿，56项受影响测试及独立交叉复审通过；已用
同一批已采纳单元离线重导出，3.089秒，新增模型/数据库调用0，原执行结果不覆盖。
新Word73438字节、ZIP667013字节，三个分册齐全；B18实物检查20/20通过，678处显式
模板（608文本、52网格、18图片）在所属章节保全，图片核对关系及CAS字节。
546条覆盖记录与433条规则及24行映射重建一致；受审assessments为0，不能把空数组
当语义通过。旧成品及两份失败审计保留。模型复核没有有效结论，真实企业资料亦未提供，
因此不能宣称正文语义或真实知识匹配验收通过；既有固定大纲层级问题BUG-069未由
本轮改动解决。未部署到业务服务、未提交或推送。

B1另补W/R/C/R2全路径及真实CAS原件字节回归，25项受影响测试通过；故意令k2错绑
m2时新增断言失败。B17真实并发2使用事件精确控制两个W在途，一单元已提交、另一
成功原回包已存但单元提交失败，恢复零新W发送、零重fit，计划/单元摘要保持一致。
上述两项为绿色整套启动后新增的专项，计数不与原有测试重复相加。
最终按当前collection与三份JUnit逐用例核对：2491个唯一用例均有绿色证据，无漏项；
主回归2489项加两项新增用例，不将专项4项/25项与全套数量重复相加。Python改动的
Ruff仅剩HEAD原有三个F401（schema.py重导入状态符号），未新增忽略、删断言或降门槛；
这些既有告警未在本轮清理。`git diff --check`通过。

全部证据位于`/opt/workspace/kb-tender-evidence/2026-11-01-review-fixes/`：
`python-final.*`保留首轮集成结果，`python-final-green.*`为重跑结果，真实运行在
`real-no-knowledge/`，修复后Word/ZIP及20项实物报告在`reexport-bug121-v3/`；报告为
`acceptance/b18-originals-word-coverage.json`。各专项另存F3/F5报告及浏览器证据。
原数据库不可达，本轮只向专用real_review库导入保留备份中必需表，
不操作原库；验证结束后已按完整容器ID精确清理本轮专用PG/Redis，其他业务容器未动。
清理前已保存全部隔离真实执行及回执备份`reviewfix-real-completed.dump`，337549字节，
SHA256为`16f50b3272ab412587a28741ae92145db8ffe59db3728c2e97efa579172fbd42`。
最终汇总为`verification-summary.json`，包含JUnit去重、代码指纹、实物审计与验收限制；
`cleanup.json`记录仅两只本轮容器的清理。不存在新的业务部署、提交或推送。

## BUG-113/114编制检索与覆盖结算（源码已接入，真实验收未做）

按[编制完善方案](../../../tasks/plan.md)接入逐要求补查、可选正文assessments、覆盖结算、
查询尝试持久计数和逐单元回执。`services/docreader/tests/unit` 2098 passed。
同一编制执行的杀进程计数和失租拒绝已在隔离 PostgreSQL 上通过
`test_same_execution_survives_kill_and_rejects_the_expired_worker`。
浏览器对照、106页文件和真实企业资料验收未做；没有新的付费调用。

## BUG-100—112本轮修复与复测（2026-10-29—30本地）

最新续记BUG-109：v10结构F0收到成功回包，但5个排除动作涉及6个实质部件，
包含履约保证金条款、合同条件续文及联合体填写备注，没有独立页首装饰。精确引用
审计仅证明绑定合法，不能证明允许删文；原危险回包及独立语义风险报告保留。
已正式取消v10，最终状态cancelled，在途A1为uncertain；旧分析、确认、当前Word
前后不变，无v10新成品。接下来将宽template_exclusions合同收窄为
source_decoration_exclusions，仅独立页首/页尾/页码装饰可选；混合或实质部件
保留。没有新增节点、修订轮次、环境变量或语义正则。
BUG-109新合同已完成，51项定向及完整2073项单元通过（141.07s），完整日志/XML为
readiness的`bug109-final-unit.*`。已准备独立v11，前序仍为v9，模型环境离线核对
与已授权值一致；S/A/R不改，危险旧F回包不匹配新合同，不清缓存或改旧响应。
新版动作与最终Word的语义保全仍待实际验收，单测不作为真实修复完成的依据。

2026-10-30 07:17 UTC：独立v11已启动，执行
`precise-source-rerun-068f5e5c39494bc89079ce87f0f0495a`，目录
`2026-10-29-authoring-rerun-v11`，日志readiness的`real-run-v11.log`。冻结生产文件
与启动工作区一致；前序仍v9，明确不以v10危险排除回包作为新合同成功依据。
外部流诊断已排除正常GeneratorExit和取消，不更改生产传输、TLS或代理。
v11复验7份S/A0回执后只发送新F0；F0于07:20:19 UTC成功返回，6个装饰排除动作
对应8个实例部件。独立逐值payload/精确native绑定及语义核对确认8段均为独立固定
页首，无正文/条件/备注，v10六处实质部件6/6未被F0排除。报告在v11的
`acceptance/f0-source-decoration-binding-and-preservation.json`。此为F0阶段结果，
A1及最终Word尚未验收，不据此关闭109。F0另有5条结构意见进入既有一次A1。
A1于07:26:08 UTC完成，F1于07:27:38 UTC完成，随后进入正文。独立精确回放10份
accepted调用重建候选→实例关系，最终树与持久document-outline逐值一致；六处实质
内容在各自实例中6/6完整覆盖。F1唯一排除也精确绑定独立页首，两轮累计9个实例部件
均纯装饰、实质删文0。报告在v11的`acceptance/final-six-substantive-instance-lineage-and-coverage.json`
及`f1-source-decoration-semantic-classification.json`。24响应行/3共同条件仍在；A1未另加
原生技术表，尚不能用F的“缺模板”意见断言没有响应表，Word仍待实际检查。

随后独立复查确认BUG-110：业务响应表与原生模板在S/A已有分工，F却不知道宿主
会输出全部response_rows；form无template_refs的导出分支还额外提示原格式待补。
先复现2个失败，再仅修一个导出条件及F渲染说明；当前v11冻结不热改。新版本将
固定v11最终大纲恢复，重验成功正文，仅补未接受单元及实际变化复核，不重读来源、
重组目录或替换当前Word。真实表行、空应答和相邻条款仍须检查，不能仅以隐藏提示
当作表单完整性已通过。
BUG-110修复后完整单元2075项通过（144.29s），报告为readiness的
`bug110-final-unit.*`。受限进程中另一次定向测试因已有线程环境问题停滞后已终止，
不报通过；完整回归在正常隔离测试环境覆盖同组全部用例，0失败/跳过。

v11于07:51:31 UTC完成，Word72909字节、ZIP531286字节，分析/确认/当前Word未变。
页首实物为总稿及分卷全0；六处原实质内容和34个相邻选段在对应Word位置保全，
24技术行/3共同条件/10个★均在，总则下标准及一般规定关系在Word中通过。
报告在v11的`acceptance/final-source-decoration-and-response-table-semantic-conclusion.json`
及readiness的`fixed-source-header-audit-v2/v11-final-readonly.json`。原报告误称部分
选择为完整native unit已追加勘误，不覆盖旧证据；实际为独立页首选段，同行正文保全。
BUG-110实物复现：主稿/技术卷各已有完整响应表，仍各多一处原格式待补；当前代码
修复未进入v11冻结，不能称v11已包含110。63单元中1个技术失败（商务摘要表），
52次发送为41成功/9失败/2未知；8次429按30/60/120等待实际生效，但仍有一次耗尽。
其他正常占位与技术失败分别计数。复核又触发10次局部改稿，已登记BUG-111：仅纯
空form且无本任务知识候选的记录退出正文R，原格式继续由F/导出保全；5专项先2
失败后全通过，完整回归进行中。新固定纲恢复将补唯一失败并验110/111，不新增全源读取。
随后BUG-111完整单元2080项通过（140.66s），`bug111-final-unit.*`，0失败/跳过。
v11严格业务完整性16/16通过；首次验收命令漏传v6 CAS而出现来源审计失败，旧报告
保留，补齐原始只读CAS后按同一物理/逻辑摘要标准通过，未降低检查或改变成品。
最终报告为`acceptance/business-with-full-cache-provenance.json`。恢复前隔离库另备份
readiness的`v11-completed-before-recovery.dump`，710096字节，SHA前缀5b343fe0。

2026-10-30 08:00 UTC：独立v12固定v11大纲恢复，执行
`precise-source-rerun-9af27d62460f49ccbbca74a0cf1b7f74`，目录
`2026-10-29-authoring-rerun-v12`，日志readiness的`real-run-v12.log`。冻结163文件
与当前生产一致，包含110/111；不运行S/A、不更换大纲，先新F只读核对，然后重验
成功W并只补未接受正文，R按实际内容重批。模型环境与原授权一致，无新配置。
旧运行、全部回执及当前Word仍保留；没有v12成品前不称恢复验收完成。

v12于2026-10-30 08:08:42 UTC完成，本次固定大纲恢复约8分20秒，不能当作从头分析
到成稿的总耗时。Word74154字节、ZIP536413字节；63个写作单元为11个generated、
52个正常placeholder，error_type非空0。19次新发送为18成功、1次429失败后重试成功，
另15份旧成功回包经当前validator复验复用。分析、人工确认及当前人工Word均未变。
16项既有业务检查全部通过，报告为v12的
`acceptance/business-acceptance-v12-fixed-outline.json`；固定大纲的S来源归属明确继承
v11，不伪造v12做过来源调用。BUG-109六处实质内容在总稿/所属分卷12/12保全，
34个相邻选段68/68保全；独立页首审计通过，总稿及分卷没有残留选中的源页首。
BUG-110主稿/技术卷各仅一张25×2响应表，24行原要求、3段共同条件和10个★保留，
多余原格式待补各1处降为0。实物报告为
`acceptance/word-six-guards-and-parameter-table-audit.json`；页首报告为readiness的
`fixed-source-header-audit-v2/v12-final-readonly.json`。BUG-111的正文R0为2包、局部C
2次、R2为1包；这不能单独解释总时长变化，因为本次也固定大纲并复用旧调用。

验收边界：上述16项与`acceptance/final-technical-hierarchy.json`只覆盖既定完整性及
总则下标准/一般规定关系，不代表全局层级正确。独立逐原文核查仍确认BUG-069的
另一处：硬件`chapter-2.3.2.4`与“其他”`chapter-2.3.2.5`在成稿中并列，而后者
原文为硬件内部的第二子项。F已报告此问题，固定大纲模式只记录建议，不自动更改；
不能将这轮技术成功或16项通过表述为所有章节已符合正式投标要求。原目录与正文
编号本身冲突、商务父标题概括及条件性规则处置另按语义意见保留，不全当确定缺陷。
四条F意见的独立归类报告为v12的`acceptance/structure-review-semantic-audit.json`，
原文定位为`section:218:page:55:3`至`section:220:page:60:1`；实际总稿及技术卷均
通过稳定章节书签确认“其他”仍在硬件之外，不能用全稿文字存在代替正确归属。
没有真实企业资料，知识匹配仍未做实际企业资料验收，企业应答和最终价格留空。
BUG-011共享限流协调仍未关闭；BUG-112固定入口未补段落片段注册已定位，正在
独立复现/修复，v12冻结成品不会被热改或覆盖。生产部署尚未以该产物完成验收。

BUG-112随后完成真实只读复现：相同最终大纲及经AuthoringBasis恢复生产字段顺序的
依据下，63个任务中未准备片段时23个完整请求不同；固定入口补同一宿主准备后
63/63与自动入口相等。26个自动请求摘要命中v11实际accepted回执，27个未准备
请求摘要命中v12实际回执，说明差异不是只有JSON键顺序。报告为readiness的
`bug112-request-reproduction-corrected.json`；早期未恢复basis字段顺序的报告保留并
显式勘误。生产仅在固定大纲正文分支补一行异步片段准备，不改大纲、原文或缓存。
当前生产_DocumentExecutor.replay对这26份真实accepted回包全部重验通过，零网络、
零数据库和CAS写入；继承receipt只存本次审计内存，没有伪造持久回执，报告为
`bug112-real-accepted-replay.json`。失败回归先1失败/1通过，修复后2专项通过，完整
2082项单元通过（141.57s），0失败/跳过，readiness的`bug112-final-unit.log/xml`；
红绿及独立复核记录为`bug112-red-green-review.json`，`git diff --check`通过。
此补丁未进入v12冻结运行，不能把v12已完成成品称为含112的重新付费验证。
独立review随后发现非对象冻结JSON会被新准备路径抛AttributeError；先3失败/2通过，
再在prepare_template_parts现有不可读出口前加同类形状检查，5项专项全部通过。
文件缺失、数组、null和字符串都保留缺口继续导出，不加门禁；第二份红绿及复核
报告为readiness的`bug112-shape-red-green-review.json`。原2082项完整结果及原报告
保留，补充边界后的最终完整2085项单元通过（140.33s），0失败/跳过，日志/XML为
`bug112-shape-final-unit.*`；独立复核确认缺源降级回归已关闭，`git diff --check`通过。
本轮最终汇总为v12的`acceptance/final-summary-with-open-issues-v2.json`，前一版汇总
原样保留。BUG-069具体层级和BUG-011共享限流仍开放；不称正式投标完整业务验收通过。

最新续记：v8于2026-10-30 05:50:46 UTC完成，技术状态succeeded但业务验收失败。
Word68818字节、ZIP504173字节；源分析、确认及当前Word前后不变。82次实际发送，
57次HTTP429、2次连接前失败、2次transport uncertain，14次写作成功含后续修订。
最终64个写作单元中13个为ModelWorkUnitExhausted技术失败占位，另46个正常无错误
占位，不混为“无企业资料正确留空”。已知完整表单重复3→0、内部测算正文目标0；
业务完整性15/16，函件958字按源序全在但中间插入页首，非丢138字。主稿页首9处，
分卷1八处、分卷3一处；技术层级在主稿/技术卷仍错误，不能宣布全流程真实验收通过。
报告见v8的`acceptance/final-readonly-business-summary.json`及readiness的
`fixed-source-header-audit-v1/v8-final-readonly.json`。v8独立库备份为readiness的
`v8-completed-before-recovery.dump`，674657字节，SHA前缀b2ebeb00；元数据另存。

本轮继续补齐011与107：429携带并审计解析后的Retry-After，无有效提示30/60/120秒，
有提示的实际等待也至多120秒，仍首次加三次；并发、模型和预算不改。结构F专用
Schema可选template_exclusions，只执行明确目标及其已选模板support；默认只读，
仅自动组装F0/F1显式允许，固定人工大纲及当前Word不改。三个成功/回放出口统一
应用，validator先全量校验，不边验证边删。F独立Prompt移除块级正文噪声，S/A/R
Prompt摘要实际核对不变；069层级仍待新成品，不能用提示聚焦冒充已解决。
完整单元2057项通过（137.92s），随后仅追加2个保护用例，13个新排除专项连同既有
相关70项通过，无后续生产修改；429定向207项通过。完整报告为readiness的
`bug107-429-final-unit.*`，独立代码复核未发现新增阻断缺陷。

2026-10-30 06:16 UTC：冻结新版启动v9，执行
`precise-source-rerun-98fcdb2112de49c4ae7cd7d9d940db3f`；目录
`2026-10-29-authoring-rerun-v9`，日志readiness的`real-run-v9.log`。前序为v8，
按完整请求摘要重验成功回执；自动组装用于验证107/069，不使用固定人工大纲。
启动前当前7项模型环境与已授权v8一致；旧运行、全部回执、数据库及Word均保留。
尚无新成品，不能将前述离线通过或v8技术成功当作v9真实验收完成。

v9实际于06:24:31 UTC完成，业务未通过。F0同摘要4次均transport_response_unknown、
0字节，未取得任何可执行排除响应；走保留原纲的降级出口，因此实际主稿页首仍9处，
分卷8/0/1。此运行不能证明新排除动作被模型接受或执行后失效。双主体各4张财务原表
实物保全仍通过。最终61写作单元含13个技术失败占位、46个正常占位、2个generated；
旧分析/确认/当前Word不变。80次发送含55个429、16个uncertain，未知usage71次。
全部新429回执已保留retry_after_seconds=1.0，证实网关确给1秒提示；原补丁实际仍取
1/2/4秒，不能称退避已解决。继续先补短提示下本地30/60/120秒下限，再建独立新运行，
不篡改或重置v9。独占实物/边界报告见readiness的
`fixed-source-header-audit-v1/v9-{final-readonly,financial-grid-readonly,header-runtime-context-readonly}.json`。

2026-10-30 07:02 UTC：v10开始，执行
`precise-source-rerun-e93acb4366fa47378405e409b1e4ab7b`，目录
`2026-10-29-authoring-rerun-v10`、日志readiness的`real-run-v10.log`，前序v9。
当前冻结包含短提示也不缩短30/60/120秒退避及F精确行共享；完整单元2072项通过
（141.94s），报告`v10-final-unit.*`。F无损共享12专项通过，A字节兼容独立250组合
通过；同真实F离线净降估算输入约7058 tokens，仍超过原本地预留，不新增阻断。
压缩不作为零响应断流根因结论。外部runner仅增加脱敏异常类型链/耗时诊断，默认
transport/代理/TLS和请求不变；离线夹具确认流字节、close及异常传播不变且不记私密
异常文字。没有新环境变量。163份冻结文件与启动时工作区生产逐项匹配，模型环境
仍与授权值相同；S/A/R提示不改。已复验7份S/A0回执，尚无v10成品，不称验收通过。
v9独立库另有备份`v9-completed-before-recovery.dump`，691195字节，SHA前缀c1b54f02。

最新代码后续追加BUG-108，不能将较早冻结v7当成包含此修复：真实A0/A1在身份、
授权和财务处均明确选择唯一已知subject，宿主仍拆出null主体副本。财务原S的空模板
narrative还被A0错误拆成携26模板的副本，A1省略选择又继承该坏前版，因而残留页首。
已先复现1失败/3通过，再以两行条件接受明确matching subject；多已知主体及未知未决
保全不变。定向58项、最终全单元2022项通过（138.54s），0失败/错误/跳过，日志/XML
为readiness的`bug108-final-unit.*`。没有改Prompt/Schema/字段或请求摘要；相同模型回包
仍须经过当前binder验证，旧固定outline_ref不会自动重绑，也不重写旧Word。
实际链证据为v7的`acceptance/explicit-subject-split-readonly-audit.json`及
`financial-narrative-inheritance-readonly-audit.json`；独立变异检查也能抓到条件反转。
完整离线回放已逐值复现冻结v7，再仅用当前binder重验同一真实S/A0/A1回包：3对重复
消除、目标88→85、t45模板26→0且两条残留页首退出；433规则、795摘录、24响应行、
3共享context及直接来源引用全保留，两类财务主体各22模板/4grid逐值不变。只有坏
副本派生的两张图退出合法模板投影，不按图内容删文，原CAS及旧对照均保留。
六S及A0完整请求精确相同，A1因前版结构修正改变，不能称后续复核免费命中。证据为
`2026-10-29-bug108-v7-offline-binder/comparison.json`；该回放不是新F或新Word验收。

v7已完成，45次发送中44 succeeded、1 uncertain；Word68757字节、ZIP526075字节。
技术父子层级实际Word检查通过；16项既有完整性验收也通过，但3对重复仍在成品，
不能据此关闭108。来源校验起初因只查当前执行而无法回溯缓存请求，原失败报告保留，
仅按相同输入/摘要回溯v6原model-request后重验，未降低来源覆盖。最终报告为v7
`acceptance/business-with-cache-provenance.json`及`known-f1-structure-readonly-audit.json`。
独立数据库已完成备份：readiness的`v7-completed-correct-port-20261030.dump`、
`v7-completed-backup.json`，654717字节，SHA前缀a5e35ea6；端口沿实际35245。

2026-10-30 05:29 UTC：冻结包含108的当前源码启动独立v8，执行
`precise-source-rerun-9e7a0d9744a04434a42b928bfb8f515a`，日志readiness的`real-run-v8.log`。
启动前核对授权模型环境未变；不改.env、预算、窗口或旧运行/Word。当前进行来源
缓存重验与组织，尚无新版成品，需实际新F及Word/ZIP复核后才能关闭真实验证。

v8后续进入正文与内容复核，出现大量HTTP 429；先前按F1早期失败概括为连接故障
不准确，正文拒收已核实为BUG-011复发。56份原始拒收均为
`GATEWAY_NORMAL_KEY_CONCURRENCY_EXCEEDED`，每次发送时本执行另有一条最终成功
正文在途；成功调用11.1—56.2秒，失败单元却只等待1/2/4秒。初写21个请求组中
13组耗尽、8组成功，后续修订另记，不能把技术失败空稿算作无资料的正确留空。
回执不能确定该Key全局额度、其他项目占用量，亦未保存Retry-After，不能声称没有此头。
已先在BUG-011补齐最小恢复方案，等待提示/有限退避修复单独验证，不热改冻结v8。
v8若成功落成带失败空稿，原resume只返回旧result；须新执行显式继承v8、固定其大纲，
只重新发送未接受正文及摘要改变的复核。保留原运行、全部回执和当前Word。

外部验收工具也完成独立校准：缓存来源溯源增加完整API物理摘要与所属成功传输绑定，
13个正反守卫通过，v7以严格溯源重验仍16/16通过；原误报/失败报告保留。页首工具
补封面及独立单元格边界后23项通过，v6真实负例失败、v7主稿及3分卷通过。此处均不
替代v8成品和技术失败数量的最终核对。

最新状态（下方早期running/尚无成品是历史时间线）：v6已于2026-10-30 04:25:23 UTC
成功结束，未热改该冻结版本。主稿76592字节，30表/531段；ZIP540403字节，3分卷。
主稿SHA为`e95c17f5eb968c77e08d592ae9f6a5b70257b2ad34d75612d695750d07630ce8`。
共54次发送，52 succeeded、1 uncertain、1 failed，后两者后续重试成功，原回执保留；
已报告prompt2710731、completion169329、total2880060，另2次usage未知，不记零费用。
源分析/确认/当前Word前后快照一致，未自动替换Word。数据库完成备份及审计为
readiness的`v6-completed-20261030.dump`、`v6-completed-audit.json`。

v6真实成品核对为15项通过、1项失败，见`acceptance/business-after-platform-correction.json`。
原函958字及历史138字均在主稿和卷1中，逐行完整且有序；失败实因为跨页固定页首
文字被当作函正文，记BUG-107，不再称删文。原生连续性不等于同一业务格式。
摘要表设备标签在19×4首表，3×4续表的获奖等原格保留；原“设备必须在续表”是验收
误报。平台动作已由绑定原来源的10条约束完整进入提交待办，11个note章无targets是
合法结构；原“必须有note target”也是误报。外部验收已修正且9个正反守卫通过，
仍保留原函严格失败，不以调整判据宣称成稿合格。S/A/F实际请求及Word追链见v6
`acceptance/letter-stage-readonly-audit.json`、`letter-fixed-page-heading-readonly-audit.json`
及`f-structure-input-visibility-audit.json`；没有为复核重复补发已有原文。

BUG-107仅补A组装与F结构复核的选择职责，通过原有离散部件排除已确认的页首，
不按频次/标题/关键词删文，不改S合同、字段、节点、重试和预算。新增载体回归确认
可排除独立页首，同时保留相同文字但独立来源的两条实质条款及签署栏；该夹具只
验证宿主表达与导出，不冒充模型语义验收。定向54项及完整回归已通过，最终数量见下。
最终A输入估算139958，59个runs自身增加417；连同105/107合同变化较v6共增加1210。
仍按既有授权实际发送，不因本地估算添加阻断。6个S完整请求摘要精确不变，证据为
readiness的`bug107-final-capacity-audit.json`。完整2018项单元通过（138.80s），日志/XML
为`bug107-final-unit.*`；最后澄清空候选集合不自动继承后，受影响77项另行通过。
仅澄清既有身份边界，不扩展继承代码。公共复核Prompt的结构分支变化也会改变R的
完整请求摘要，所有回放仍逐请求核对，不能将“只改结构职责”误记为R必定命中缓存。

2026-10-30 04:50 UTC：冻结最新源码启动独立v7，执行
`precise-source-rerun-e4d82cc4afa04272bfc49c3996c5d68b`，日志为readiness的
`real-run-v7.log`。启动前实际核对当前7项模型环境与已授权v6完全一致，独立数据库
可达；未更改.env、预算、窗口或共享部署。当前running，尚无新成品，不能把v6成功
当作105/106/107最新版真实通过。六S按完整摘要复用，其余以实际变化请求为准。

v6的F1七条意见已独立回查，不能都归为缺资料：5类具体瑕疵分别为身份/财务的
多余空占位、技术参数双实表、通用技术部分父子层级错误、内部测算正文；另两项
为履约保证函适用性和当前无实物的可选附件，保留人工核对，不判必交卷缺失。
双参数表是A1明确实例化同一候选两次，未观察到参见/空行撤回被宿主保全抵消。
对应BUG-041/054/067/069继续保留，不新增关键词清理或模型轮次。独占证据为v6
`acceptance/f1-findings-readonly-classification.json`及`f1-assembly-selection-readonly-audit.json`。
技术层级另有跨run只读脚本`readiness/check_technical_hierarchy_readonly.py`，5个离线
守卫通过，v6确证失败；v7须对实际Word再验，单看已选要求完整不足以关闭这些问题。

- 后续独立复核追加BUG-105：同lineage唯一位置的A1省略字段会撤回A0新增规则、
  理解来源、人工标记或参见。已以局部选择记录区分省略与明确替换，29个新增专用
  回归和受影响110项通过；最终全单元2009项通过（140.10s），0失败/错误/跳过。
  最终日志/XML为readiness目录的`bug105-final-unit.*`；独立复核及红绿证据为
  `bug105-selection-revision-audit.json`及对应增量patch。该修复不热改下述v6，真实
  修订效果另验，不能混用两版结果。
- BUG-100字段级引用域说明、BUG-101完整格式范围/主体/提交待办、BUG-102技术表共享
  上下文已按统一字段接通。首次完整Python单元1961项通过（139.59s），该结果早于
  以下独立复核及压缩补丁，不充当最终回归。未安装Ruff，不声称该工具通过。
- 独立复核补出4个宿主边界：已知+未知主体合并归属、旧正文目标跨章转待办回流、
  A1省略时丢A0新增响应行，以及不同原表条件被提升为整表共同限定。已补失败回归
  并进行精准修复，原文/空应答/既有待核出口保留，不新增主节点或生成门禁。
- BUG-103基于同一v5六个已接受S回包（132候选）重建A：完整可选模板由302增至
  883，覆盖187/520原生unit；新增560段落与21网格。162新增段落与既有摘录区间
  不交叠，其输入属于合法原文增量，不能通过删掉可选续文来消除。
- 最终生产compact投影输入估算136174，payload473529字节、wire534897字节；此前
  未共享投影为159545，Prompt更新亦计入当前估算，两者不是供应商计费数字。新视图
  以原x为键、精确原文行共享和唯一章节共享减少重复，不改.env、窗口或预算算法。
  原883部件/884摘录身份与顺序、41份网格的文字/空白/合并跨度、完整原文逐字还原
  均通过；p791—795与p857三处遗漏仍可选，最长text_lines为29。
- 容量仍超本地131072窗口和94208输入预留估算，不新加阻断，将按已有明确授权
  实际尝试发送。离线容量不是模型接受或最终语义通过；无真实企业资料，验收应答
  留空与要求完整，不冒充真实知识匹配。源分析和人工确认、旧回执及当前Word保留。
- 证据：v5的`acceptance/pool-final-production-capacity-audit.json`及相关离线脚本。
  新v6运行器位于`2026-10-29-authoring-readiness/rerun_v6.py`，独立数据库端口35245。
  此处原零付费预审已完成；最终回归与真实状态如下续记。
- 最终定向69项通过；完整Python单元1980项通过（136.45s），0失败/错误/跳过，
  `git diff --check`通过。新压缩另有300组内存逐字恢复及索引/净收益抽样，错误strip
  变异被回归抓到，生产摘要不变。测试日志/XML在readiness目录的
  `bug100-103-final-unit.*`，此数量包含最后的四类宿主边界及压缩修复。
- 2026-10-30 02:44 UTC左右：独立v6启动，执行
  `precise-source-rerun-8a493cef44fe422f956cf9e5c7334604`。实际运行冻结Python源码和
  当前gpt-5.6-sol、api.kunkun-gpt.com:8317/v1配置，不热改运行；日志
  `2026-10-29-authoring-readiness/real-run-v6-20261030T0244Z.log`。当前running，来源请求
  规划中，尚无新模型回执；没有真实语义通过结论。
- 只读成品核验脚本`2026-10-29-authoring-readiness/accept_v6_business.py`已准备，
  旧v5反例运行准确检出函缺文、续表、财务主体、技术限定与平台第四卷五类失败；
  新版result未出现时仅返回pending，不创建伪验收。v6结束后核对实际Word/分卷及
  逐字来源，而非仅确认“所选部件已渲染”；人工版式/叙述审阅仍是独立验收范围。
- 环境中断（BUG-104）：最后DB导出为03:01:12 UTC，4个来源已接受、第五sent；
  进程随后因35245连接拒绝退出。发现当前Docker全部容器/卷均为空，原PG不存在，
  清理来源待查。先前把旧running快照解释为仍在等待，已向用户更正；不能把该
  等待时长都算作模型耗时。第五完整SSE及接受封装在CAS内，正在精确复验。
- 已从readiness的566965字节初始custom备份恢复独立PG，并恢复原先已验证的
  `--network host`及仅127.0.0.1:35245监听，同镜像、凭据和DSN；初始1个分析、
  3个历史编制执行已恢复，未改共享栈。.env、模型和生产源码未变化。桥接-p恢复
  尝试复现旧记录的宿主超时，未更改任何主机防火墙/代理；新建容器壳已替换，
  同一恢复卷保留。接下来按最新完整导出恢复缺失执行/请求，不清空付费围栏。
- 最新v2/v3/v5/v6导出已单事务补回4个执行、130条请求，逐项核对无冲突；原状态、
  sent_at、请求epoch及全部旧回执保留。报告为readiness目录的
  `v6-restore-exports-applied.json`，不是重新调用模型或重置尝试额度。
- 第五包完整SSE和已接受封装均找回，CAS摘要、完整请求、输入摘要与冻结源码一致；
  离线重建S1—S5全部通过原引用和上下文校验，未发送S6。证据在v6的
  `acceptance/s5-cas-replay-audit.json`。恢复脚本只读核验通过，准备使用原生Store
  租约/请求完成/回执接口补回该检查点；恢复时间将明确标注，不能冒充原提交时间。
- 03:34:36 UTC：第五包已用原生接口恢复成功；旧四请求与全部旧回执不变，无新付费
  行，原请求epoch/sent_at保持，恢复回执记录新租约epoch及证明。审计在v6的
  `acceptance/s5-checkpoint-recovery.json`。恢复后数据库custom备份617585字节，
  `readiness/v6-after-s5-recovery-20261030.dump`；03:35 UTC同一v6以`--resume`
  续跑，新的进程已确认存活，快照持续更新。最终Word与ZIP仍待实际生成和验收。
- 03:38 UTC：S6也已接受，6包来源均为首次成功，S6供应商输入18594、输出5282。
  A0已实际发送，输入估算138748、wire544979字节，仍沿原算法与配置且仅记录容量
  警告；不能将已发送等同于接口接受或语义通过。最终业务审计追加技术原要求基准
  到响应行及Word可见性的分层检查，避免只验“已选行渲染”遗漏上游未选要求。
- 03:44 UTC：A0完整回包，供应商输入138542、输出15262，说明本请求真实获接口
  接受；本地估算超窗未导致供应商拒绝。宿主发现5个target缺候选/规则或合法参见，
  按原合同返回具体path，现进行同逻辑单元的有限纠正，未接受该版大纲。不能将
  transport succeeded当成模型合同或成稿验收通过；未放宽为无依据目标。
- A0前三次回包依次发现5个无依据位置、两次同一p284—p311模板跨度可见性问题，
  原字段路径纠正沿首次加三次的同一额度执行，未清缓存或重置尝试。03:59:20 UTC
  第四次A0通过宿主合同校验，随后进入authoring_structure独立结构核对。最终成品
  尚未出现；模型能纠正引用不等于格式或技术要求已完整。
- 04:00:47 UTC前F0返回6条结构观察，涉及技术组成顺序、附件同级关系、重复表单、
  分项报价位置、封面字段及无固定格式的分类；当前A1已发送。此处是待原文/成品
  核实的模型意见，不将每条直接记为已证实缺陷；只有既有一次结构修订，不扩循环。
- A1及F1已完成，F1仍有7条结构意见待实物核实，不启动额外结构循环。04:14 UTC
  已完成24个authoring_write，进入authoring_review，尚未生成最终Word/ZIP。
- 模板重复纠正的根因已确认并记BUG-106：1715个prepared部件中A仅见850个，
  p284—p311的native区间30个部件有1个image未入A池，A完全没有连续性断点信息。
  它在S6也只有空文本别名，不能说模型看过图像；已有OCR complete且text空。
  root本地查看CAS确认为小图形标识，但修复不加入图像语义特例。证据在v6的
  `acceptance/span-rejection-readonly-audit.json`及`span-missing-image-metadata-audit.json`。
  BUG-106仅为A增加宿主计算的同文档连续runs，不扩大池或自动插图，当前按TDD实施；
  同样不热改v6冻结运行。
- BUG-106已实施并通过定向25项、独立guard对等复核；最新完整单元2017项通过
  （137.91s），0失败/错误/跳过。日志/XML在readiness的`bug106-final-unit.*`。
  真实v6输入重建生成59个runs，移除该字段后payload与原A0逐值一致；6个S完整
  请求摘要亦精确一致。runs本身仅增加417个输入token，连同BUG-105的Schema/Prompt
  最新A输入估算139765（原138748），仍无本地估算阻断。证据为readiness的
  `bug106-capacity-audit.json`；这是离线精确对照，不冒充新视图真实模型验收。
- 内容修订过程中一次transport_response_unknown、一次connection_before_send，
  后续均已沿同单元剩余额度重试成功；保留原uncertain/failed回执及未知用量，
  不将unknown usage记成零费用、不重置尝试。当前仍进行局部修订，尚无最终成品。

## 2026-10-29 内容完整性与引用修复（本轮）

- 按已批准方案接通精确SourceExcerptRef、实际response_table及row_fills：原文读取、
  来源/组装绑定、写作、逐行复核/拒收、改稿与Word导出保留完整引用；无资料行继续
  显示招标要求和空白企业应答，不恢复每章通用分析表。组装漏选的已知行保留并记录
  归属待核；模板片段与同unit其他片段不再混同，原生grid仍独立处理。
- 写作固定原始规则/明确来源/模板范围，只选一次关联；装载原文不再扩大语义起点。
  片段只关联真实父范围，不自动关联全部兄弟片段。单目标按完整规则/逻辑响应行
  有限分片；不可分原表保留容量缺口和原格式，不裁格或阻断其余内容。
- 自动大纲A0后先F0，现有依据足够时最多A1一次；改变结构才F1。不改人工固定
  大纲，结构未解决意见随草稿保留，outline_only自动规划也核对结构。最终结构确定
  后再发布outline_ref及写作任务。缓存复验先于真正发送准入，仍沿原尝试上限。
- 用户指出不应增加预算配置：本轮新增profile环境变量、配置API、部署接线及默认
  算法切换已全部撤回，deploy/.env、.env.example、Compose与llm默认算法相对HEAD
  均无差异。继续使用原o200k_base和既有预留；同端点模型历史用量仅作仓库外审计。
- 受影响回归593项通过，独立引用/行覆盖/结构元数据审查26项通过；最终全单元
  1825项通过（134.59秒）。首次全量出现的3个旧引用断言已修正为精确来源断言和
  完整selector；环境未安装Ruff，未以语法检查冒充该工具通过。
- 真实复测准备在`/opt/workspace/kb-tender-evidence/2026-10-29-authoring-readiness/`。
  准备期间用户环境配置已改为api.kunkun-gpt端点的gpt-5.6-sol，复测沿用启动时
  冻结的当前配置，不切回旧模型，不跨端点/模型复用付费请求。原v12分析快照保持
  未确认，复测另存产物，不代confirm、不改Word当前版本。
  只恢复原独立验收PG容器，未重启共享栈；启动前没有新增模型调用，不能将隔离回归或
  原人工修订预览当作本次模型自动理解与成稿验收完成。
- 补充组装反馈Prompt：复核v引用只定位上一版，候选t引用仍用于输出选择，不按
  finding.code机械删除内容；对应16项结构/引用/导出隔离回归通过，diff检查通过。
  当前地址的新真实发送被自动审批拒绝：此前可信用户授权未明确覆盖该具体目的地
  与招标原文载荷。已请求精确确认，未冻结新执行或发出模型调用，未绕过拒绝。
- 用户随后明确回复“允许发送”，授权同一106页招标原文至当前api.kunkun-gpt端点、
  gpt-5.6-sol的独立付费复测。沿既有默认算法启动，保留旧执行及全部回执，不代
  人工确认、不改Word当前版本；实际启动与进度以下续记。

08:24:59 UTC：唯一新执行`precise-source-rerun-6dc3d63346174fc79ba882ea820b2579`
启动，161个运行源码文件冻结。证据根为
`/opt/workspace/kb-tender-evidence/2026-10-29-authoring-rerun-v1/`，准备/运行日志在
`2026-10-29-authoring-readiness/real-run-v1.log`；沿用当前gpt-5.6-sol、上下文131072、
并发2和原默认预算，无profile、不改.env。复用同一冻结分析输入，不重抽取/核验，
从来源组织重新编制，无旧固定大纲或前驱执行；没有知识资料或人工确认，企业应答
及报价仍留空。初始来源规划进行中；执行和最终内容均尚未验收。

08:29 UTC：初始来源请求连续`connection_before_send`，没有任何接受回包。
同客户端`getaddrinfo`约8秒后报`socket.gaierror [Errno -3] Temporary failure in name resolution`，
已确认失败在DNS而非TCP/TLS、模型语义或预算。使用原取消入口停止本次不可达执行，
保留全部失败回执及尝试，不继续耗尽后续来源；旧分析与Word不动。供应商无用量回执，
不将unknown usage当作已确认0费用。正在做宿主/容器DNS对照，未改变模型、环境配置
或网络策略，真实内容尚不能验收。

连通性对照补充：原隔离PG的默认bridge使用8.8.8.8/8.8.4.4，解析失败；健康业务
容器的user-defined网络使用127.0.0.11并正常解析。一次性host-network客户端解析
及严格TLS正常，无密钥`/v1/models`返回401；其PG发布端口连接仍超时，不以改DNS
参数掩盖数据库路径问题。v1最终取消，19次物理尝试含18条连接前失败及1条
`uncertain(cancelled_after_send)`，没有接受回包或成稿，`preservation.json=true`。
仅为本任务创建隔离网络、给原隔离PG附加连接并验证客户端直连，不重建/重启PG、
不改共享栈。旧执行及所有额度保留；修复后验证另存v2，尚未开始新的付费调用。

隔离网络检查结果：Docker内嵌DNS恢复，但同网PG连接5秒超时、外部严格TLS连接
10秒超时，尚未满足复测前提。不能将本次网络尝试记为已修复；继续只读区分TCP
与PG握手、核对Docker转发/路由，不改宿主防火墙或共享服务，不开始v2付费调用。

进一步确认同网客户端的TCP连接本身超时；PG`listen_addresses='*'`且5432监听正常，
同命名空间loopback可用，因此不是密码/认证失败。宿主存在TUN策略路由及Docker
转发规则，当前证据不足以指认具体规则，不修改这些规则。采用仅本任务的
host-network隔离PG副本（空闲端口、仅127.0.0.1监听），通过pg_dump/pg_restore复制
原独立DB，原库/卷不动；核对冻结输入、旧结果、确认、Word heads和v1回执后才
进行无付费连通性验证。回收本任务新增bridge/attach，不动任何既有网络。

08:42 UTC：新隔离PG`kb_authoring_pg_host_20261029`仅监听127.0.0.1:35245，使用
原固定镜像和新独立卷；原库约13.2MiB，dump566965字节。8张关键状态表逐行比对
完全一致，覆盖run/checklist/根引用、旧编制结果、0确认、Word head及v1所有回执。
host-network客户端DNS约0.002秒、PG查询约0.006秒、严格TLS无密钥401约1.64秒，
模型仍为已授权gpt-5.6-sol，161个生产文件摘要与v1一致。失败测试bridge/附加连接
已回收，原PG/卷/原网络不动。证据`host-pg-clone-audit.json`、`clone-host-preflight.json`
在readiness目录；满足前提后启动唯一环境修复后v2复测，不清除或重置v1历史。

08:43:19 UTC：v2执行`precise-source-rerun-ee88feff592841a1a9db6429243d9316`启动，
证据根`/opt/workspace/kb-tender-evidence/2026-10-29-authoring-rerun-v2/`，日志仍在
readiness目录的`real-run-v2.log`。冻结gpt-5.6-sol、同端点、上下文131072，161个
生产文件与v1逐一一致，不改默认算法、环境变量、提示或模型。v1取消记录在原库及
副本均保留；当前从来源规划开始，新稿尚未生成，不以环境预检代替内容验收。

08:48 UTC：首个S请求完整成功并由当前binder接受，第二个S在途，暂无失败/重试。
首包耗时206.45秒，首字节70.789秒；估算输入92150，供应商prompt91944、completion8909
（reasoning2766）。这只支持当前请求的计量观察，不推广为全部模型已校准。开始
按真实回包核对短引用、持久精确来源与候选用途，不把传输成功当成成稿语义通过。

冻结S计划只读重建并与首个实际请求摘要精确一致：5个来源组织包、1715条原生组织
记录/部件，输入估算分别92150/92075/92058/92146/86628，均沿原32768输出预留。
这是编制来源规划，不是抽取包数，也不是5包已完成；证据`source-plan-audit.json`。
首包170个精确来源均能读取正确区间，20章/30局部候选尚待全局整理；本包不含
后续技术参数来源，不能据无response_table就判技术内容遗漏。

09:01 UTC：S已接受4/5，第5包在途，仍零失败/重试。第4包耗时361.20秒，实际
输入91940、输出15121（reasoning1773）；同一既有socket只读计数确认持续收流，
不把字节数当完成率。第4包生成3个response_table共49行（19/6/24），519个精确
片段与89个原格式绑定均可正确读取。跨页协同条款选择两个原生范围、355字全文
保留；独立SD-WAN与前项续文仍共处一个原生范围及派生行，尚不能宣称逐义务拆分。
原说明索引只进入结构约束，没有选作模板或另建目录章；投标函36个原格式片段/
7个原生单元与旧完整选择一致。此为来源候选审阅，49行是否进入正式总稿/分卷、
条件边界及无资料留空仍待全局组装与最终Word验收。

09:05 UTC：5/5来源包全部接受，但没有A/F模型调用；已发布大纲只有“待整理内容”
一个章、143个候选，不是正式大纲质量通过。正文按兜底继续生成，不新增阻断门禁。
离线F0重建原输入154436、compact135303，超过既有94208准入；compact中supports
59741、未安排规则20830、submission_sources17864、chapters19817等，证据
`structure-capacity-audit.json`。优先量化A0未准入及重复/整unit扩张根因，不改预算
算法或环境配置，不重读已接受的5份来源。写作已有两条无接收数据的unknown回执，
同时有成功调用；原回执和有限重试均保留，不把unknown用量计作0。

09:16:45 UTC：v2机器执行succeeded，result为`ecd24539…`，总进程32分35秒，49次
物理尝试（36成功、10失败、3未知）：5S/30W/14内容复核。已报告prompt1101976、
completion79088，13次缺完整usage继续单列。10失败为供应商503，上游明确报告
`server_is_overloaded`，不是本地缺Key的证据；有限重试后部分恢复。分析、确认、
Word heads前后相同。原Word/ZIP在v2根目录，A/F未发送导致结构验收不通过。

真实载体验收：26张表含19原生表、3响应表49行，原网格/合并/文本完整，49个企业
应答格全空；主稿包内字节一致。投标函36段/7原生节、958规范化字符连续保留，
没有前置分析表；说明索引未渲染，未发现JSON协议键对/HTML实体。当前只有一个
兜底章承载143候选、0分卷，不能说正式目录或分卷通过。定点来源确认27/27技术
原范围及跨页续文完整；详见acceptance下carrier/semantic/technical-source审计。

BUG-097实物确认22个单元格暴露Markdown标题前缀，先复现失败后按既有模板显示
处理修复，26项相关回归通过。对同一v2输入零模型重导出，只有22个单元格的标题
标记变化，其他文字/原表/行/空应答保持；新稿在
`v2/acceptance/display-reexport/fixed/投标文件.docx`，旧稿与Word head不动。

用户明确要求完整A/F超本地估算也尝试发送，已先更新字段字典和方案，接通五层
admit_request发送策略，严格require仍用于分包/compact选择；不改算法、窗口或.env。
超估算只记`fits=false`及`admission_warning=local_context_estimate_exceeded`，不进入
HTTP body、不改变摘要。A0/A1共用_ask中的原缓存优先/一次compact逻辑，F保持先
compact再发送。19项新发送反例及下层254项、上层54项回归通过（有重叠不合计）。
完整单元1848通过、1项旧“超估算禁止发送”断言失败；该断言已按新合同改为实际
尝试后保留供应商失败出口，随后39项定向回归全部通过，生产代码未再改变。
独立发送链审查未确认新缺陷，额外3项同步边界断言通过。
显式v2前驱复用预检已确认5个S完整请求逐字段/摘要不变，5份旧回包通过当前
Wire解码和binder；76份旧receipt CAS均可读，新CAS回退顺序为v3/v2/v12。
证据readiness/v3-preflight-audit.json；当前配置仍是精确授权端点/模型，待冻结启动v3。

09:36:39 UTC：v3执行`precise-source-rerun-419debc5f66f469590c23c84147ffc2f`启动，
161个运行文件已冻结；新证据根`2026-10-29-authoring-rerun-v3/`，显式前驱为v2。
同一gpt-5.6-sol、已授权端点、131072窗口及默认算法，旧稿/Word head/原v2不动。
当前规划与回放S，随后实际尝试A/F；模型是否接受超本地估算尚待真实回执。

09:38 UTC：5份S均实际继承v2成功回执，带reused_from_execution_id，没有S物理发送。
首个A完整compact请求已sent，input_tokens=126988、fits=false，metadata保留
`admission_warning=local_context_estimate_exceeded`；不再被本地估算拒发，等待供应商
实际返回。此为发送路径验证，尚非归并语义或完整成稿验收。

09:42 UTC：A0完整成功且进入F0真实发送，未重试。供应商prompt126782、completion
12141、total138923；实际总量高于本地131072配置但接口正常完成，证明本地估算
不能直接当作供应商硬上限。当前仍保留原算法/窗口和真实fits=false，不据此推断
供应商全部模型容量。开始审A的最终候选组织及F意见，不能以请求成功代替大纲质量。

09:43 UTC：F0也完整成功并被宿主接受，估算input124095、fits=false及warning保留，
供应商prompt123889、completion1614；A/F各首次成功。A0生成51章/64目标/4分卷，
3张业务响应表49行保留，5份S继续全部继承、没有来源重发。F0指出待整理目标归属
及“投标文件可编辑版本”被错误设为独立分卷，但供应商原始回包3条意见均显式
can_revise=false，未触发A1。已确认不是宿主抹掉true；BUG-095补记结构/正文标志
共用的衔接根因，字段字典及方案先改为F0有意见即交一次A1，仍由模型依据原文
决定修改或保持。当前v3冻结版本不热改，继续保存本轮实际结果，不宣称结构已验收。

09:49 UTC：v3正文已有21次成功及1条transport_response_unknown，内容复核已开始。
未知回执完整保留，不视作零用量；A/F无失败重试。仍无最终result，真实Word与
分卷、空应答和技术来源完整性待本轮实物验收，Word heads及分析确认状态不动。

09:54 UTC：结构意见交接最小修复完成：只将自动大纲的触发条件改为findings非空，
结构Prompt同步，正文can_revise/人工固定大纲边界不变；先红测3失败复现，随后
44项定向回归通过，最终全单元1852项通过（135.08秒），diff检查通过。.env、
.env.example及Compose无差异。v3仍沿原冻结版本：6份首轮正文review共14条发现，
其中6条can_revise=true涉及3任务，正在既有一次修订；其余为人工/资料待补。
唯一未知仍是首轮W断流。已批准在v3完成且原稿落盘后再跑v4，不并行新增付费；
先精确复验继承5S+A0，改变的F及其真实依赖重新执行，不重置或清除任何旧回执。

09:57:43 UTC：v3最终succeeded、容器退出0，DB执行1260.18秒（21分00秒）。result
`358003795b2e84d0ca89a3d45ea1ad10ac2f3a351b3dab9ee87544d8361ba6d4`，Word及ZIP
在v3根目录。35次实际发送：A0/F0各1、W24成功（含3修订）、正文review8成功、
另1条首轮W未知；5S全部继承。已报告prompt1366667、completion44363，1次usage
未知单列；analysis_and_word_unchanged=true。机器流程完成，F0三条结构意见及
部分正文未决仍在，不宣称正式目录和内容语义全部合格。

09:59 UTC：进一步核对发现宿主保留的5个未安排位置没有进入candidate_bindings，
其原t候选仍在但反馈只给空映射。先补BUG-095及字段/方案后，以红测复现，再用
3处最小变更登记已知候选到保留实例；相关74项回归通过，多实例歧义仍保留。
此变更不改S/A0请求、模型或预算；在v4冻结前完成，不热改v3。下一轮须先对
6份S/A0以新binder精确复验，随后实际验证F0/A1/F1及最终结构，不重付已完成来源。

10:07 UTC：v4预检通过，5S及A0请求逐字段/摘要一致，旧raw回包当前binder均接受；
5个保留目标的候选映射正确补齐。readiness/v4-preflight-audit.json、rerun_v4.py和
client-run-v4-command.json已准备，但启动被自动审批拒绝：审查认为此前“允许发送”
未明确覆盖api.kunkun-gpt.com目的地和招标载荷。没有创建v4执行或新模型调用，
不得记为已开始；拒绝原文及未执行状态在real-run-v4-approval-blocked.json。
已向用户说明自动审批理由并请求对具体地址/模型/载荷的明确授权，未间接绕过。

v3最终实物：49响应行、27/27技术原范围、投标函及19张原生表完整，主稿与4分卷
一致，企业应答与报价未填值，无JSON/HTML/Markdown泄漏。但最终w10资格资料
摘要仍有已知门槛遗漏和条件弱化，真实文字问题与合法空位分开，记BUG-098，
不将载体完整或机器succeeded说成整体语义合格。

用户询问Prompt返回是否应由Schema约束，已只读核对request/wire/Response类型及
官方Structured Outputs文档，具体合同漂移记BUG-099：json_object未发送生成Schema、
空children/blocks默认规则不一、可选null与实际校验矛盾、逐项容错list[Any]不能作为
完整生成合同。

用户同意后，BUG-099已落地：llm/contracts.py按显式task/operation/批次选择强类型
生成Schema，六处生产构建入口均接入，system仅附加一次紧凑Schema并继续json_object。
生成合同与逐项容错接收分离，可选null、空列表默认及blocking必填规则已统一；
Prompt只保留业务语义和局部引用示例。Schema计入完整输入、请求摘要和执行spec；
当前Schema静态801～5524字节，未新增模型调用节点、环境变量或预算算法。
全单元1903项通过（137.19秒），含MockTransport实际发送/计量/缓存/纠正回归；
数据库验收23项收集成功但未执行，专用测试库本轮不可连接且未配置测试DSN。
diff检查通过，未部署、未付费重跑，未改.env、Compose、旧冻结执行/回执或Word head。
供应商strict能力及真实输出效果尚未验证，BUG-098文字遗漏仍未解决。

此前v4预检中的“继承5S+A0”只适用于当时冻结合同；本次Schema/Prompt改变完整请求
摘要，该预检已过期，不可运行旧脚本并沿用旧复用承诺。旧回执全部保留，后续真实
验证须重做当前请求/成本预检，并取得具体目的地、模型、招标载荷的明确发送授权。
本轮“可以”仅用于Schema实施，没有新增真实模型调用。

BUG-098后续修复（用户要求继续）：真实证据区分了两条根因。w10父级重复索引没有
全部子表原文，不能据复核建议补写；业绩/财务实际输入则完整，最终b2仍被明确指出
错义。已先更新字段/业务流程/实施与五项成本约束，再新增reject_content语义动作。
宿主仅执行该标记或旧unsupported_content的定位剔除，code不作为其他语义判断；
原要求、原表及正确兄弟保留。动作经现有一次修订及再核传递，结构及保存Word的
只读复核强制false；S/A优先已有组织/参见关系，不按标题删真实摘要表。
最终全单元1914项通过（135.57秒），diff检查通过。.env、Compose、模型、窗口及
预算计量均未改，未部署、未新增付费发送，全部旧执行和Word head保留。

真实旧产物的离线采用/导出回放已通过，证据根为
`/opt/workspace/kb-tender-evidence/2026-10-29-bug098-adoption-replay/`，含replay.py、
audit.json、adoption-replay.docx及adoption-replay.zip。回放使用v3/v2/v12原CAS只读
联合读取；两个最终意见经独立核对后仅在回放中显式标记，不补写或冒充模型原回包。
只去掉w10/b2错误生成表，另外60单元不变，主稿其余23表及49行要求/响应逐格一致，
主稿+4分卷仍可导出，66个已读旧产物SHA未变。最初只挂v3导致来源不可读的回放
如实失败，补齐真实执行的来源根后通过；不是生产丢数据。
这份稿件是宿主采用路径的审计稿，不能视为新Prompt/Schema真实生成验收或正式
当前Word版本；新模型的错义判定与重复摘要预防效果仍须按当前合同真实复测。

2026-10-29 12:13 UTC：用户已明确同意向当前api.kunkun-gpt.com:8317/v1使用
gpt-5.6-sol发送同一106页标书的必要原文/表格，启动新冻结编制执行v5，执行ID
`precise-source-rerun-cba0b392ea0e4a1192cb16256dbf0b45`。沿用既有专用克隆验收库，
不重启共享栈、不改.env；证据根`2026-10-29-authoring-rerun-v5/`，脚本/授权/日志
位于`2026-10-29-authoring-readiness/`的rerun_v5.py及real-run-v5-*文件。
当前合同离线预审S=6（旧5），1991个来源片段及433条规则各发送一次；完整输入
估算合计463497、wire合计1782961字节，每包预留32768。旧5S/A0摘要均不兼容，
不沿用旧复用承诺。后续A/F/W/R数量依赖新回包，无KB检索；正常逻辑调用上界
为10+4W，物理调用上界40+16W（首次加三次额度），W仍未知。预留不是API输出
硬限，没有供应商单价或货币硬上限，不能据此承诺人民币或总Token最大值。
12:15 UTC只读核实：运行租约有效，首个S已sent，无错误；实际完整请求Schema仅
出现一次，输入89019、wire355269与预审及持久metadata完全一致，未继承旧回包。
冻结review含reject_content；其真实返回和最终采用尚待验证，不标记语义通过。
成本及初始发送审计在`2026-10-29-schema-cost-preflight-v1/`；原分析/Word最终
不变性、6S及全部后继调用、Word/ZIP载体验收仍待本次执行结束后核对。
首个S因将上下文事项i107误作可返回规则ID被拒，带实际错误的纠正回包已接受，
随后进入第二包；不将首次transport succeeded当宿主接受。没有补映射特例。
并行23项PG隔离回归初次22过1失败，已确认是恢复缓存测试仍期待写作后重做F；
当前F在写作前且输入不变，实际精确继承，W及包含恢复正文的R正常新发。仅修正
此过期测试，并加强实际W/R覆盖及F摘要、CAS、来源回执检查；正式23/23通过
（28.86秒），0跳过/失败/错误，所有新随机schema已清理，不删除旧schema，模型
调用0。最终证据`2026-10-29-bug098-099-pg-acceptance-fixed-b945165d/`，原失败及
诊断证据保留；生产代码和本次冻结runtime未变，未重跑无相关改动的全单元。
12:46 UTC：前三个S均经一次引用纠正后接受；第四包首次混用i100规则引用，首次
纠正调用在246.6秒后记transport_response_unknown，usage未知，既有同单元额度
继续重试。当前3/6已接受，不将未知调用计入成功或推断供应商计费。重复引用域
问题与候选漂移已单独记BUG-100，源码未热改；三例只读核对报告在成本预审目录
`v5-reference-domain-review.md`及`v5-reference-domain-audit.json`。
12:55 UTC：6/6来源包全部接受，来源阶段10次有usage的完整返回（含4次引用拒收）
及2次传输结果未知；第五包首次约8.7秒结果未知后重试接受，第六包首轮接受。
全局组装A0已发送，完整输入估算124477、wire495667；fits=false是相对于32768
输出预留后的建议输入量，沿既有发送策略继续，不删原文或扩大窗口。尚待A/F及
正文/成稿验收，来源阶段通过不代表目录和文字语义已通过。

13:18:17 UTC：v5流程结束，status=succeeded，启动至结束65分09.683秒；主稿
`投标文件.docx`及`投标文件.zip`已保存在v5根。原分析/确认/当前Word状态不变。
实际31个逻辑请求、41条宿主sent、10次额外尝试；单逻辑最多3次（上限4）。
其中5次合同纠正（4S引用域、1W多余blocks.title）、4次uncertain传输和1次F0
发送前连接失败。36份usage已知：prompt1877203、completion117373、total1994576；
5份usage仍未知，不按零或估算账单。没有旧回包继承；全部41请求Schema恰好
一次且摘要/计量/spec一致，162个冻结文件及59个已加载CAS（含10个旧CAS）
SHA核对通过，不把局部已加载核查说成遍历全部旧存储。

阶段墙钟：S41分37秒、A0/F0为4分10秒/1分49秒、A1/F1为4分35秒/2分19秒、
W0/R0为4分10秒/3分53秒、W1/R1为1分23秒/31秒。S约占64%，本轮完整重读
且含引用纠正，是主要耗时；旧v3约21分钟继承来源，不能直接横比成同口径加速。
实际review初轮3条reject_content=true，针对固定5日期限被泛化、无依据的整体
合规声明及履约承诺；沿两单元修订后最终再核不再否定，所检查的最终采用内容
集合及正确兄弟未发现误删。原资格父章额外自由摘要已消失；不宣称模型以后不
再出错，也不将本次修订成功等同真实测试已覆盖最终仍否定时的所有回退出口。

最终结论：**机器流程完成，整体业务验收未通过**。通用carrier通过：20表
（17原生+3响应）、48响应行，主稿/4分卷/ZIP一致，企业应答和报价空白；无已查
JSON字段、代码围栏或HTML实体泄漏，ZIP完整。专项原文覆盖未通过：原技术表
“同等及以上配置”列头限定全稿缺失（BUG-102），8A后页3×4续表未承载、四类
财务表均未分投标人/原厂实例（BUG-101）；平台上传附件仍进入目录并成为第四
分卷。完整投标函原7源单元仅选5，原第10条代理服务费承诺缺少两个连续源单元，
规范化原函958字符中缺138字符；另写w10承诺不替代原函，归BUG-101。通用
carrier仅证明选中32段均渲染，完整原函专项仍失败。不能把48vs49直接认定
参数条目丢失，已查设备参数正文尚无可证缺失，
但表头限定本身有义，不能当标题去掉。仍无企业资料，企业能力/最终报价留空
属于授权场景，不与这些来源和结构缺陷混淆。本轮未修改生产或冻结源码、未部署。

验收证据：v5/acceptance/{carrier-audit,review-adoption-audit,outline-stage-audit,
final-source-structure-audit}.json；adoption专项有3项来源并集差异，不能用
carrier的passed覆盖它。调用/预算/Schema及阶段耗时独立审计在
`2026-10-29-v5-execution-audit/`的calls-budget-schema-audit.json和同名review.md。
当前待修是BUG-100字段引用域、BUG-101实际实例/续表、BUG-102表头限定承载；
未把这些待修标记为已完成，后续按原文及实际成品验收，不增加下载门禁。

## 修改后完整复跑（2026-10-28，本地时间）

用户要求查看当前修改后的完整效果，已于10月28日18:10:37 America/Los_Angeles
（10月29日01:10:37 UTC）启动同一106页原件的新执行。run
`c189e80a-e1a6-4442-bf09-00c0f58ffb66`，project
`f677c09d-716d-5631-b152-3454b00e77f1`；证据根为
`/opt/workspace/kb-tender-evidence/2026-10-28-full-rerun-v12/`。
使用当前Python冻结源码、normalization12及已核定的API/migrator二进制，3份SQLx
迁移checksum与当前SQL逐一一致；独立数据库/API/gRPC/Redis DB15，不改共享栈或.env。
启动前模型和执行表均空，不继承旧分析、大纲或正文；旧运行及全部回执保留。
模型、上下文131072和并发2沿已授权配置；没有企业知识资料，企业应答及报价留空。
normalization12针对回归最终9/9通过；首次测试目录未建导致的fixture错误单独保留，
不是生产执行错误。完整运行刚启动，尚无新稿，不能用旧输出替代本轮验收。

01:19 UTC：106页解析、2次OCR已完成，冻结仍796范围/4抽取包。89个既有重点锚点
按完整来源位置与文字唯一匹配，另加B限定及诉讼检查；尚不代表语义抽取已验收。
抽取已1/4接受；一包收到供应商api_error中断，原流与未知usage保留，自动重试
在原额度内进行。Dramatiq监控展示子进程端口冲突已记BUG-090，业务Worker与租约
正常，不为修监控打断在途调用。原文检查及错误证据在新根acceptance/和
provider-stream-errors.json。

01:26 UTC：抽取仍1/4接受。一包第二次返回完整JSON，仅第24条关系身份无法绑定
（BUG-077再现），已按同一首次加三次额度反馈纠正。另两条中断回执同时缺完整结束
和usage，分类为transport_response_unknown；尚无底层异常类证据，不能与第一条明确
供应商api_error混为同一已知原因。目前两包第3次尝试在途，不重置耗尽额度、不重跑
成功包。细目及原回执留在新证据根。

01:33:18 UTC：4/4抽取包全部接受，约22分40秒。新台账433事项、796处置、120关系、
83issues；处置478 items/291 non_item/27 unresolved，796原范围逐一覆盖、零重复遗漏。
累计2 OCR、8抽取物理调用（5完整回包含1次合同拒收、3断流）；已知439311 Token，
3条unknown usage不按零计。随后独立review规划11包开始执行。
实际新事项/处置可读预览位于`acceptance/extraction-preview.md`与
`acceptance/extraction-all-items-and-dispositions.md`。30个技术入口均有新事项引用，
备件B限定与诉讼要求已抽取；目录附件3/5的部分改标点引文以问题/未决保留，后续
需核验。上述为抽取及原文覆盖结果，不代表核验或最终稿语义通过。

01:54:39 UTC：独立review11/11全部首轮完成，无新增传输或合同重试；随后定位补读
规划18包并开始发送。该数量来自本轮433事项/核验问题台账，不沿用旧运行13包，
发现/逐问题定位的拆分及后续关联数量仍须按实际计划审计。核验回包完成不等于
待核业务问题已经解决，分析尚未进入人工确认，大纲和正文也尚未生成。

02:24 UTC：locate18/18全部首轮完成，开始42批关联判断。定位实际为6 discovery+
12 reference；发现批数未增，reference89问题由47条needs_source=true、8条false及
34条未声明组成。另47问题走local（43条false、4条未声明）。8条false随5个仍有
真实来源依赖的组进入4请求，不能仅按false删除整批；18份重建payload摘要逐一匹配
真实计划。源证在`acceptance/review-followthrough.{md,json}`。本轮已知问题台账
不同于旧运行，不能把18/13或42/45差异直接归因于优化。关联及后续joint尚在进行。

03:01:24 UTC：关联21/42批完成，77/159逻辑组结果保存，均首轮接受；2批在途。
完整计划159组分别装入34×4、7×3、1×2共42批，每组恰一次。最紧请求估算
28387输入+98304输出预留+4096安全=130787，仅距配置131072余285；这是未完成
端点计量校准的准入估计，不是实际Token窗口保证，BUG-086未据此关闭。分析阶段
仍运行中，已知报告用量3028232 Token，原3条未知usage继续单列；尚无新大纲/正文。

03:27 UTC用户询问运行是否符合预期，完成只读中途审计：35/42批、131/159成员
精确对应计划，无成功同摘要重复发送、成员重复或串组；抽取各包尝试3/1/3/1，均
在首次加三次额度内。成员空shared字段截至该快照没有出现，不能声称真实触发v12
修复。按首发送至末回执，抽取22.21分、review19.89分、locate28.11分；关联仍
进行中，不能以批数下降宣布全流程明显提速。证据`execution-audit-latest.json`。
内容方面，独立核验能识别目录适用性遗漏和替代义务弱化，部分关联提案及关系已
补充正确边界，但尚未joint，不能算最终生效。关联按既有合同不直接改写错误事项；
无amendment不等于没有下游修正作用，也不是新程序缺陷。原台账措辞更正仍走显式
修订，最终大纲/正文须对照原文与已接受解释检查；当前只确认执行和恢复基本符合
设计，内容收敛及预算校准尚未达到完整验收。03:30:06已推进37/42批，无新增失败。

03:37:31 UTC：关联42/42真实发送均首轮完成，159成员精确覆盖一次、零重复遗漏。
实际耗时74.732分钟、1900323已知Token；旧轮78.59分钟/2097250 Token，但逻辑组
和语义台账不同，不能将全部差异归因于v12。42份原始回包均无成员shared_findings
字段，本轮没有触发精确空数组容错分支；该分支隔离回归通过与真实全流程无相关
错误分开陈述。随后joint17批/23成员开始执行，最紧请求估算余44025上下文。
证据`associate-completion-audit.json`，大纲仍未开始；分析完成后自动推进预览大纲，
不代用户提交人工确认。

03:51 UTC重新查询进度发现最新快照停在03:46:48（joint12/17、2在途），不是实时
状态。随后核实原验收PG/Redis于03:47:16退出0，测试容器03:48:59退出255，执行
已经中断；原因待查，记BUG-091。三个原容器及持久挂载仍在，CAS758文件保留。
当前仅准备恢复原三个验收容器、原数据库和同run，不重建或重复成功调用；在途
请求远端结果未知，须按原持久回执/额度恢复。大纲和正文尚未开始，不能报完整通过。

用户确认继续测试恢复后，03:56:26 UTC已重新启动原三个验收容器及同一冻结API/
gRPC/Worker/dispatcher/driver。原数据库、run与command不变；本轮private中的
PG/Redis端口随原容器重新分配调整为32768/32769，Office origin也对准恢复后的
实际容器/网关；配置备份与激活回执保留，模型和协议不变。
03:57:47核对租约重新有效、原command execution_count/delivery_count各2、失联
计1次；旧2条sent自动记executor_lost/uncertain，现90份完整回执+5条未知用量。
原成功包尚无重发，新模型调用尚未产生，正在回放原回执恢复计划；重试额度没有
重置。证据`recovery-activation.json`、`private.before-recovery.json`与恢复前快照
`recovery-run-before.json`；尚待验证剩余核验真正继续及最终输出。

03:59:37 UTC同任务恢复回执审计通过：原90份完整response_ref逐一不变，成功
digest重发0；新增2次joint的目标恰为中断两批4个成员，持久尝试数均2，没有重置。
旧2条发送保留为executor_lost/uncertain。04:00:27已实际接受14/17联合核验，
证明剩余任务恢复推进；5条完成但未知usage单列，另有2条在途不能当未知账单。
证据`recovery-receipt-audit.json`。这仅确认续跑和复用边界，尚无大纲/Word最终验收。

04:02:16 UTC：joint17/17完成，分析进入waiting-confirmation；433事项、356待核
问题，未提交人工确认。分析从01:10:37至04:02:16墙钟约171.65分钟，包含环境
退出/恢复等待，不能把该时间直接当纯模型执行耗时。总100物理调用（含6次追加
尝试）、4463816已知Token、5条完成但未知usage。原成功回执恢复复用检查通过，
大纲于04:02:17自动提交，新execution为
`web-outline-40e0015ab3a0eacecb76e246a4a429a139028baa176028c48627870bc7dcf1d9`。
分析输出与复核台账在新根output/；仍需核查最终语义去向，大纲、正文和Office尚未
通过本轮验收，不能以恢复成功代替完整文件通过。

04:22:56 UTC：本轮大纲source5/5全部首轮接受，实际601438 Token，开始assemble
归并。5包规划覆盖1991个唯一来源、零重复，输入估算86090～92126，保留原2048
纠正余量；这是准入及来源覆盖证据，不是内容已全部正确。当前尚无最终candidate，
已准备真实来源对照审阅，候选完成后及时放行草稿正文，不替用户确认事项。

04:26:56 UTC：assemble首回完整JSON因四个全局字段错放chapters[0]被拒收，
已在原额度内发起第二次纠正；五个source回执保持不变、不重跑。该非空错层不能
套用成员空shared冗余处理，记录BUG-092，原回包及四处真实反馈均保留。
当前最终candidate尚未产生，语义审阅和正文仍待完成。

04:31:48 UTC：第二次assemble已接受，五份source未重跑；大纲共7次物理调用、
867738已知Token，source至归并墙钟29.53分钟。新candidate为57章、121目标
（37 form/48 narrative/36 attachment），商务不含报价、技术、报价三卷；身份及
目标关联机械检查零错。基础内容审阅确认附件3/5不适用有显式排除，诉讼说明及
法律文书位置保留，★整段和强制定制承诺、独立SD-WAN证明在用途与来源中保留。
尚未据此宣布技术参数表逐行完整或最终正文语义通过。原候选及审阅证据在新根
`output/outline-candidate.json`和`acceptance/outline-candidate-*`。

04:35:14 UTC：根代理完成基础审阅后释放验收脚本停点，正式API保存原候选修订
并启动正文执行
`web-document-84b96439e0139286e279619b82fcad8962c9f1e649b7a7cb295cfb9fc4304243`。
未手工修饰候选，未提交业务人工确认，confirmation_ref仍为空；无企业知识资料，
应答及报价继续留空。接下来核查实际生成Word的原技术行、依据措辞、表格与留空，
不以大纲通过替代内容和Office验收。

04:40:17 UTC：13次写作物理调用已全部成功，无合同或传输重试，开始内容复核。
已抽查12份写作回包，使用185份招标来源支持、未使用企业资料，cell_fills和
attachment_ids均为空；没有代填姓名、签章、报价、“无诉讼”或“完全响应”。
这是回包抽查，最终Word中的技术原要求保留、依据措辞、原生表格和Office保存
仍待验收；证据`acceptance/authoring-response-review-latest.json`。

写作规划补核：121目标对应108初始任务，预算拆分后112单元；21个符合写作路径
的候选中仅13个可准入，另8个单目标完整输入超限，未发送模型并走预算占位。
涉及w5/w16/w20/w62/w65/w104/w105/w110，包含卷级说明、保证金、资格资料、
商务评审和有效期/完成期限/付款方式。其余为12原格式保留、39缺允许依据、
4人工填写、36缺证明原件；这些路径不能混为一类。实际13次写作全部成功不等于
112单元实质生成完整。准入重建在`writing-plan-audit.json`，最终units及输入极值
仍需对账；本轮作为BUG-040同类容量缺口记录，不擅改运行预算或截断原文。

用户随后确认本轮基础设施退出是其重启服务所致，BUG-091根因已据此更正，不再将
该中断当成未查明系统故障。继续定位上下文膨胀、关联耗时及已知摘要错误的下游
传递，真实结构复核与文档导出不为根因分析暂停。

04:52:18 UTC：本轮新Word及ZIP已下载，正文执行17.04分钟，实际27次调用全部成功：
13初轮写作、1次w4局部内容修订、11初轮内容复核、1次修订复核和1次真实结构复核，
1872728已知Token；结构调用244.47秒，保留10条待核finding，不将其全部当确认缺陷。
112单元实际为5 generated/107 placeholder，8个ModelBudgetError逐项与规划对账一致；
无企业资料的正常留空、保留原格式、人工填写和预算失败分列。全流程134次发送
（129完整成功、5未知用量），7204282已知Token；分析开始至下载墙钟221.68分钟，
含服务重启恢复和验收审阅等待。不可变result为`6e39f688…`，Word为`8b908684…`，
文件在新根`output/generated.docx`、`output/package.zip`，原分析仍waiting-confirmation。

最终机械验收：57章/121目标书签均保留；23原生表855单元格及合并与原件逐格一致，
562空白格未代填，总稿/三分卷/ZIP哈希核对通过。433原事项完整进入交付底稿，不等于
正文全部安排；正文对应263个不同事项，332次要求展示、718次引文均与当前记录一致。
56依据表的企业应答列全部空白，报价/企业证明未编造。实际语义审阅仍确认BUG-093
跨页原文展示缺口和BUG-095双目录；i15/i36错误摘要保持待确认，合法correct出口
已存在、本轮未触发，不能误报joint已接受更正被丢弃。审阅报告位于acceptance/。

深入根因与当前代码评估：8个超限请求按字段精确去重均删除0条；保留全部原文、
仅停止第二轮关联扩展的因果对照使输入从96312～161875降为31221～80687，均低于
94208准入。该对照不证明新增关联语义都可删除。关联阶段双槽利用率99.07%，
42请求全部首轮成功，completion中89.86%为供应商报告reasoning；不存在明显宿主
调度空等，仍需解决任务及上下文负荷。当前工作区与冻结版本对照后，预算/结构修改
主要统一authoring_structure任务和提示加载，未改变容量公式或写作扩展逻辑；相关
37项隔离回归通过，不据此声称8个容量缺口已修。证据为`stage-efficiency-causal-audit.json`
和`acceptance/writing-context-causality.*`、`current-budget-structure-review.md`。

真实Office验收：首轮保存因本轮隔离API缺本地NO_PROXY超时，原编辑已保全为recovered
版本，首次失败未冒称成功；仅修正该API进程局部环境后，第二轮实际打开/定位/输入/
显式保存/下载校验通过（1项浏览器验收），真实回执saved、178书签完整。标记版和恢复版
独立保留，最终选回原清洁生成版本、标记0，会话/key/pending/save_error均为空；原Word
字节及模型调用数未变化。通用启动配置未改，细目见BUG-094及`office-final-audit.json`。

当前未提交代码评估补充：恢复、客户端、布局与复核113项隔离测试通过，加前述预算/
结构37项共150项。默认沙箱测试两次在asyncio Runner清理阶段挂起，均中止且未算
通过；禁用自动插件、显式启用pytest_asyncio后完成113项，未据此确认业务代码缺陷。
指定持久化、旧入口移除与PAGE字段改动只读审查未发现新的已证实恢复/付费围栏
或跨版本错误；这些清理不等于业务语义和容量问题已解决。
最终10条结构意见逐项对照后，确证双目录角色混入、技术规定次序不符；报价卷混入
评标算法/修正规则为成稿质量问题。E表适用性、可选附件及身份证放置仍待核，A.2
双表、第三偏差表和第二组资信表头丢失未复现，不按10条确认缺陷统计或机械补表。
详见`acceptance/structure-findings-triage.md`，BUG-044/054/093/095同步保留准确边界。

用户随后指出正式Word各章被大量表格遮盖，新增BUG-096并纠正成稿验收口径：
81表中56张为宿主统一插入的要求分析表，23张为原生模板表，2张为模型生成的
报价修正规则表。56张分析表含62095可见字符，占全部表格文字96.8%；技术无损
不表示这些分析内容应进入正式投标文件。正式投标函正文前叠加目录、商务总章、
投标函本章3张分析表，共36条数据、6568字符；本章分析表15行含表头，商务总章
22行含表头。正文在Word body47～82，13条与署名空位均与原函完整对应；原PDF
为物理75～76页（冻结unit使用page:74/page:75零起始索引），不凭XML推定Word
实际页码。发现时生产导出逻辑尚未修复，不能把先前机械检查0错解释为正文内容正确。
证据`acceptance/document-table-placement-review.json`，正确边界是正式正文与
要求/证据侧栏及编制底稿分开，真实技术响应表等业务表单仍须完整保留。

BUG-096导出修复：字段字典先废除旧展示合同，再以反例确认旧实现失败，移除共享
章节渲染中的分析表前插。正式Word保留原格式、写作内容、真实表格和空位；要求
移至既有可读待办，按最终大纲展示章节/封面、提交约束、候选/规则排除和未安排
要求。候选排除不等于整条关联规则不适用，本轮2条规则同时关联被排除候选及正式
章节，其各自用途和理由均保留。没有新增协议、工作流节点、模型调用或生成门禁。
相关139项隔离回归通过（55正文/底稿/确认/布局、46生成链/表单/编制边界、38导出/
书签/编制v2）；含asyncio的默认沙箱执行挂起后已中止，正常授权环境禁用自动插件、
显式加载pytest_asyncio完成，不将挂起计作通过。

同一result、大纲、112写作单元和433条要求已用修复后源码独立重导出：
`reexport-bug096/generated.docx`、`package.zip`、`submission-notes.md`。
导出源码SHA`590feac299b9f947df4a05f84814b75c52b05f12a06944bf5d5acee033ec4af1`；
新Word SHA`e3085eb3d836e24528e9d4ba124e97de0a7a0364b5e04bf2adb4bdbf89bb6c2f`，
62476字节，81表降至25表，56张自动分析表为0；剩余25表XML逐一相同，178书签不变。
独立机械验收确认总稿57章/121目标、三分卷、23原生表855锚点单元格/562空白单元格
及合并保持。原函13条和签章空位仍保留；标题后原有空叙述任务占位仍存在，不宣称
标题后立即就是函文。该占位来自缺知识的独立声明任务，未按标题或表格外观删除。
要求底稿452184字节、SHA`9433504e6c3b48369e8e4414d2f8c22e7e310eef2b56465ac67a405f832f6574`
与原包逐字节一致；新notes589513字节。旧result、大纲、112单元、DOCX/ZIP均未变，
重导出期间禁止网络，模型0调用，没有DB/API写操作或Office当前版本替换，共享
Worker尚未部署新源码。证据`reexport-bug096/reexport-audit.json`、
`quick-comparison.json`及`acceptance/reexport-mechanical-review.json`。
独立可读底稿验收通过：433/433规则的全部非空语义值（正文、主体、范围、条件、
例外、量化、评分、后果及提交细则）及783/783引文，逐条核对65个章节/处理区域；
旧56张分析表332行的语义和引文均有可读出口。i15/i70的候选排除与实际章去向同时
保留，confirmation_ref仍为null，底稿保留待确认提示。原投标函958个规范化字符、
13条及签章空位完整。证据`reexport-bug096/independent-notes-acceptance.md`及同名JSON。
双目录、技术续文覆盖、原摘要缩限、章节排序及写作上下文预算问题仍按原记录保留，
本次去掉分析表不代表这些内容问题已经解决。

BUG-095跟进：用户指出“1 目录/附件1至10清单/本部分内容待补”仍不应进入正文。
原PDF物理74页为第六章投标文件格式的组成索引，source第4批已在purpose中说明
用于确定组成及顺序，却选成form及12个文字部件；assemble以t71/p4～p15继续采用，
最终为chapter-2/target-1。纯文字form无网格的短路产生空w2/placeholder，导出
既复制索引又追加待补；这不是企业资料缺失或模型写作失败。结构复核曾报
toc_volume_order_mismatch，只形成意见，不会自动回修大纲。旧source/assembly提示
没有告知宿主已生成自动TOC，实际用途与实例化的合同边界不足。

先更新问题记录及字段字典，再向来源、组装、结构复核三个提示各补一段职责说明；
说明性清单用constraint_range_ids/submission_refs保留，不无条件实例化为form，
误建候选沿既有excluded_targets处理。明确要求提交指定目录格式时仍保留真实实例，
不新增目录字段/节点、标题规则或门禁。48项相关隔离回归通过；新用例串联source、
assembly、structure、要求快照及导出，验证说明候选排除而真实表格/空值仍保留。
此为既有引用及出口回归，不是新提示已通过真实模型语义验收。

本次依用户已明确的用途裁定，复用apply_outline_edit形成独立修订预览：精确移出
chapter-2/target-1及仅绑定它的空w2，保留全部实际附件和原规则/来源；该规则及两条
来源原已在约束/提交来源中，仍保持75个constraint_rule_refs、93个submission_refs。
其余56章、120目标的身份/内容及111个unit/CAS不变；preserve_requirements重建
target_ids后433条(rule_ref,rule)逐一相同，底稿新摘要变化只来自关联位置更新。
最终文件在`reexport-bug095-user-revision-v2/generated.docx`（62251字节）、
`package.zip`、`submission-notes.md`。相对BUG-096稿仅移出14个索引/占位节点和
2个对应书签，自动TOC保留；25张剩余表XML、投标函及三分卷实质内容不变，55处
标题编号及4处宿主参见编号随实际位置更新。4条精确绑定旧位置的局部待补移入审计
retired_gaps，25条原结构观察保留并注明修订前、尚未重新复核，不宣布其他顺序/
覆盖问题已经解决。原版、BUG-096稿及首版预览均未覆盖，模型0调用，导出禁网络。
这是人工裁定后的独立文件预览，未保存正式API修订或替换Office当前稿；共享服务
未部署新提示，新提示的自动分类尚待真实模型验证。审计为新目录中的
`revision-preview-audit.json`、`revision-content-comparison.json`。
最终独立验收通过：相对BUG-096稿，仅移出14个索引/对应占位节点，剩余525节点在
排除55处标题号、4处宿主参见号及书签序号后XML完全相同；总稿自动TOC保留1处，
178至176书签恰为该章及目标两处移除，三分卷同规则核对通过。原函958个规范化
字符/13条完整，25张表XML完全相同；433规则语义、783引文在notes仍逐条可读，
确认状态仍为空。原章/模板/空w2及用户决定在审计中可追溯，旧文件SHA不变。
详见`reexport-bug095-user-revision-v2/independent-revision-acceptance.md`及同名JSON。

## 当前真实全流程（2026-10-28）

15:06:05 UTC已启动新版整份文件真实流程：run
`900c52eb-302f-42e1-8304-4b2a16ff5031`，项目沿新隔离库中已验证上传的
`9536e332-d533-50dc-8095-456ec262a94a`。使用独立runtime-full（154文件）、协议11、
最新verify条款范围提示，grok原模型/131072上下文/并发2不变。免费预审仍4抽取包，
范围数165/210/271/150、输入估算110478/109701/109320/64061，完整来源全部保留。
只切换该新任务的grpc/worker/dispatcher，旧运行及共享服务未动；实际进度以新证据根
`flow-state.json`、`monitor-latest.json`和logs为准。15:06:19 OCR一成功一在途，无重试。
外部验收脚本会在大纲产出后暂停供根代理检查，不增加产品门禁或让用户再次授权。

15:21 UTC：前两包直接通过，第3包完整JSON仅含schema_version/items，缺其余三个
必需数组，已沿原额度反馈重试；第4包仍在途。供应商DONE、stop、completed均正常，
不是流截断。该回包142事项暂不计入已接受台账，完成数仍2/4，实际合同重试1次。
这是BUG-077已知回包完整性问题再现，未热改快照或重复执行成功包。首包已在实际内容
核对中保留目录“不适用”信息；最终核验与成稿去向还需检查，不能提前关闭BUG-079。

15:27 UTC：抽取已3/4。剩余包第一次纠正收到供应商api_error，明确为供应商到Grok
的中途ECONNRESET；没有DONE/usage，保留为uncertain并单列未知计费。目前同包第3次
发送在途，仍在原首次加三次额度内，已接受3包未重跑。去敏传输审计在新证据根
`provider-stream-error-audit.json`，不归因于本机连接、Key额度或上下文超限。

15:31:25 UTC：4/4抽取全部接受，最后包第3次发送成功，推进独立核验。抽取含两次
追加尝试约25分钟；累计7个成功传输和1个uncertain，已知458999 Token，未知usage
仍单列。未复跑已接受包；此时核验计划及后续调用量尚未确定。

15:52 UTC独立review11/11完成，均首轮通过，进入13次locate（6 discovery+7 reference）；
15:57 UTC已完成2次locate。最终抽取421事项/109关系/53issues，796原范围各一次处置
（items520、non_item275、unresolved1）。定位计划对421事项的88410对组合无遗漏，
只说明计划覆盖，不能当语义已通过。累计已知1284288 Token，仍1笔旧unknown usage。
真实抽取及11份review内容核对见`content-acceptance/observations.md`；目录适用性与
30处技术原文重点已保留，条件边界、非源流程文字及局部评分依据不足仍单独追踪。

16:19:29 UTC：13次locate均首轮通过，实际进入45请求的associate合批计划，2个在途。
累计已知1925813 Token，仍1笔抽取阶段unknown usage；逻辑组/候选数及joint数量待
本轮计划与实际回包核对，不把旧台账的192组/55批直接套用到这个新运行。

实际关联为163逻辑组装入45请求（33×4、8×3、3×2、1×1），目标每组一次、无遗漏
或重复。计划输入855247，最紧请求含输出预留/安全量总130487≤131072；全部准入，
预留不等于wire输出硬限。17:37:38 UTC关联45/45批完成、163/163成员结果保存，实际
47次调用含2次仅失败成员纠正。16:19:03起墙钟78.59分钟；prompt1000050、
completion1097200（其中reasoning993906）、total2097250。旧关联阶段117.60分钟，
但新旧语义台账不同，不能把全部差异归因于合批。联合核验16/16批首轮通过，
17:37:47→17:49:41墙钟11.90分钟，prompt458910、completion187129（reasoning185423）、
total646039。17:49:47分析进入waiting-confirmation，整体163.71分钟、95次发送，
累计已知4669102 Token，仍1笔抽取阶段未知usage；421事项、351个人工问题待核对，
未模拟人工确认或宣称analysis-ready。当前已启动预览大纲，结束后按约定暂停审阅。
审计在`associate-plan-audit.json`；阶段收齐后才汇总共同反证和正式成员产物，中途
成员stage暂未全部落盘不表示回包丢失。

17:17首个成员回包把批次顶层专属的`shared_findings: []`放入成员，触发
`extra_forbidden`。成功兄弟先保留；纠正只target坏成员，完整公共依据及原子额度
身份不变。17:22:10该成员纠正已接受，坏成员共2次尝试、成功兄弟仍1次，没有整批
重跑。纠正耗时304.6秒，prompt58125、completion35136（reasoning31555）、total93261；
真实局部恢复证据在`member-retry-audit.json`。该恢复证明成功兄弟未重发，不代表机械
格式错误没有成本，也不能用请求压缩比例代替实际阶段用时。

第二次同类空数组错层在17:28:26完成仅坏成员纠正，另3个成功兄弟均保持1次；两次
恢复合并证据为`member-retry-audits.json`。协议12源码对该精确空字段的规范化已完成
离线回放，未混入本轮冻结协议11。阶段用时/usage以`phase-usage-comparison-latest.json`
为准，未结束阶段仅是截至观察时的部分数据。

47次关联预算已按冻结代码和实际model-wire逐一离线重建，请求摘要与原预算各项一致，
完整messages独立o200k计数+512也一致。实际prompt累计923834估算→1000050，
高8.25%，单次最高高17.498%；2次按实际prompt加原reserve/safety分别超配置窗口
1742/1342，但所有实际prompt+completion均未超窗。当前对非图像请求无条件启用
o200k的校准适用范围存在问题；供应商分词与中转附加内容仍不能仅凭回执区分。
证据在`budget-calibration-samples.json`，此次未加系数、修改运行或新增付费调用。

BUG-085共享写作单元书签修复已完成源码及隔离验证；分析runtime-full保持原样。
大纲暂停且本库两类模型请求在途均0后，根代理明确授权18:24:15切换独立
runtime-authoring：154文件中仅已验证的`authoring_export.py`变化，其余153逐字节
一致，normalization仍11。仅该任务grpc/worker/dispatcher重启，API/旧运行/共享服务
均未动；旧快照、配置备份及两份manifest保留。根代理完成业务/机械大纲审阅后，
18:26:30已单独放行正文；不能将旧稿重导出检查当作本轮成稿验收。

18:12:12 UTC大纲source已5/5接受，第5包首回被宿主拒收后仅该包纠正一次通过；
source共6次物理调用，已知720031 Token。冻结
`_organize.plan_sources`与本执行固定context离线重建为5包，输入估算
92158/91942/91421/92134/85466，421条规则各归属一包，1991个源range无重复；
各source包摘要和实际首轮wire一致。source请求本身不带operation字段，不能误认整份直接
生成。18:12:13 assemble已真实发送，输入88728、含输出/安全预留125592≤131072，
wire354527字节；18:19:07完整回包首次通过，未走单根兜底。18:19:11大纲完成并停在
`OUTLINE_REVIEW_READY`，共29.39分钟、7次物理调用、854227 Token，零在途。产物有
5根/55章节/118位置，商务、技术、报价3卷；仍99条outline_rule_unassigned、24条
outline_template_missing，内容验收待审阅，不能以树结构已生成代替业务通过。
证据为`outline-source-plan-audit.json`及`outline-execution-latest.json`。

第5包首回在16个findings中多交非空range_ids，OutlineFinding合同不接受该字段。
BUG-081叶级反馈只带这16处路径/短片段，反馈2687字节，实际wire332499→335637；
输入85466→86213（+747），在2048纠正余量内，18:07:37真实发送成功进入生成。
前4包各一次且accepted引用保留，未重跑；纠正实际274.79秒，prompt92232、
completion29712（reasoning19805）、total121944，最终已接受。完整审计在
`outline-correction-audit.json`，本轮未热改合同或删除非空来源信息。

归并耗时414.27秒，prompt86331、completion47865（reasoning33185）、total134196。
输出超过32768预留，实际总量比配置131072高3124；供应商完整成功，未证实供应商
自身窗口超限或截断。此前“实际总量未超配置窗口”只适用于关联与定向样本，不能外推
到整轮。完整请求独立重计/摘要核对在`outline-budget-audit.json`，阶段结果在
`outline-checkpoint.json`。外部approval记录说明35条核心技术要求已有响应位置，99条
未分配多为流程规则但仍保留待办；i45/i108等未决项要在成稿核对展示状态，不能当作
语义已解决。该记录只解除本次外部暂停，不构成人工业务确认，分析仍waiting-confirmation。
18:26:33正文执行`web-document-4337afe7256c70b35932687180eeab30393cf342da2aecd13bdeaf7eef65036b`
已提交，18:42:24完成生成及HTTP下载，正文15.83分钟、31次调用、2037812 Token，
无合同重试。18次write含w107“技术条款偏差表”一次核对驱动语义修订；13次review
含修订后复核，不能把这次修订误报为重试或只统计初始17次write。原稿DOCX107837
字节、ZIP537641字节，v1为`c2da4346-1966-465a-a640-bd8037b2ed96`，原实物保留。

原稿机械检查通过，但业务审查发现w103备品备件正文遗漏大纲已知的附件B不适用边界，
来源传递根因另查；待核标注不能替代语义修复，也不宣称内容验收通过。根代理授权后，
18:46:30仅本任务Python角色切换runtime-final-export，仍normalization11，154文件中
仅`authoring_export.py`从BUG-085版变为BUG-087待核展示版（SHA a91edee167912b5d16b6f16d40d87500fde3068ad88d86915fe47cbb8b5e8985）。
旧快照/原稿/API/共享栈/.env不动。

正式API使用同固定outline、空资料及previous_execution_id创建独立执行
`web-document-9c57b8a868642fc565eb3cafc183dba874a0161b6a74982faa367fc59e863603`，
18:47:56成功；实际SQL模型请求0行，31份accepted CAS全量继承（含追加write及再次review），
不是仅据代码推断0调用。18:50:53新稿下载到`final-export/`，DOCX108107字节、
ZIP538264字节，v2为`f11523f4-d620-47e7-9d50-f8f18e53adc1`。
新生成稿按产品设计不会自动替换当前选中稿，下载使用明确v2版本；验收脚本原先把
current当最新的断言已纠正。新稿机械检查及仅标注差异比较通过，分析仍
waiting-confirmation，未伪造人工确认。

18:53:52通过现有选择API将v2设为本次验收稿，随后真实Office浏览器用例通过
（1 passed，22.6秒）：打开、书签定位、实际编辑、显式保存、下载及保存后定位全部
完成，未伪造回调或清pending。v3为`d95fc5fa-f61f-4823-a59f-be5e80a44901`，
下载SHA 995f9003369ff631b4faf3880509885449cfa5e46b1499adc855e63c564a6a78，
XML确认测试标记恰一次、173书签保留；会话已关闭且无pending。带测试标记的保存稿
单独存于`real-office-final-export/`，清洁v2仍在`final-export/generated.docx`。
三阶段共133次物理发送（含2次OCR），已知7561141 Token、1笔未知usage；final-export
新增调用0。总证据在`full-flow-acceptance-status.json`。运行/编辑链路通过，w103等
内容缺陷尚未修复，不因此宣称内容验收通过。

BUG-088最小提示修复与同类前驱恢复经44项来源引用回归、19项隔离PG/Worker回归后，
根代理授权真实复测。19:16:25 UTC确认两类模型请求均零在途、无运行/排队任务后，
仅本任务Python角色切换runtime-scope-fix：从runtime-final-export的154文件冻结，
只覆盖`web.py`、`document_execution.py`和`outline_assemble.md`，其余151文件逐字节
一致，仍normalization11。19:17:19正式API提交新大纲
`web-outline-73cb45498ed977b66a4be973913604f353eed1684cf07ae498bbea511a9d751a`，
沿原模型/run、指定旧大纲前驱。19:25:53新candidate完成并暂停审阅：五份source
accepted（含原纠正后回包）原CAS全部匹配继承，实际新增仅1次assemble、零重试，
两类模型请求零在途。归并468.615秒，执行墙钟514.326秒；prompt86430、completion
55058（reasoning39798）、total141488，比配置131072多10416；供应商完整成功，不能
称预算校准通过。证据在`scope-fix/`及其`inheritance-and-usage-audit.json`，新candidate
已交业务/机械双审，未保存新大纲或启动正文；旧分析仍waiting-confirmation，当前
Word版本、旧执行全字段与回执均未变化，.env及共享栈未动。

随后业务审查确认上述candidate仍未修复BUG-088：备件target继续只选t79，未绑定
实际限定来源；正式写作未启动。来源直绑最小合同经62项回归、完整真实请求离线回放
及独立审查后，19:47:25 UTC根代理授权仅本任务Python角色切换runtime-source-binding。
基于runtime-scope-fix仅改schema、authoring_pipeline及outline_assemble三文件，其余
151文件一致，仍normalization11；切换前零在途、零running/queued。19:47:45正式提交
`web-outline-489183e56d8d1dd0b02ac1a804403e50ee9171e69f1703cfd92fd209871c1d2e`，
以scope-fix为前驱，沿同run、原模型。19:54:38新candidate成功并停止在双审：5份source
原CAS完整继承，实际仅1次新assemble、零重试、零在途。真实wire与完整离线回放逐
字段一致，input估算88998；归并363.254秒、整次412.452秒，prompt86591、completion
50307（reasoning36824）、total136898，仍超配置131072共5826。原始回包、accepted
回执、candidate和继承/usage审计均在`source-binding/`，两名审查代理已启动，尚未
保存新大纲或生成新正文；旧执行和当前Word保持不变，分析仍waiting-confirmation。

source-binding候选随后因遗漏必需诉讼说明等业务退步被拒绝，未采用其章节树。
根代理改为保留清洁v2全树，由明确agent语义审阅仅追加B原文；不是模型自动修复成功。
40项来源编辑、23项结构投影回归及真实完整请求回放通过后，20:13:03 UTC仅本任务
Python角色切换runtime-reviewed-fix，从runtime-source-binding只覆盖outline_edit、
authoring_structure、authoring_view三文件，其余151文件一致、normalization11不变；
切换前两类模型请求均零在途。正式API基于旧revision
`981ba5d9-2b08-4315-9865-4110708ae792`保存新revision
`0ba8084c-31c6-4287-aefa-638ca871d361`，逐字段核对仅
`chapter-3.16:target-1`增加`section:227:page:64`的完整SourceUnitRef，诉讼位置、
其他目标、树、规则和模板均不变。20:13:56以清洁v2执行为前驱提交
`web-document-09d27526185a3e5077113d9ab4fdafd35a89f7928a586cb3cdab4215583cc09a`，
证据在`reviewed-fix/document/`。离线已证29/31正文请求摘要可继承，基础新增为
w103写作、其内容复核批、首次可准入的结构复核；实际新输出/语义修订以真实回执为准。

20:19:47新稿成功并按明确版本下载：清洁新version
`3db2a089-3900-4c53-9e39-6364915e1de4`，DOCX108282字节，SHA
`c3773a84376741842f80ad037478bd7ee53517b1dbbd3f7be961c0195c6a1ac9`。
实际29份accepted原CAS继承，3次新调用均首轮成功、零额外语义修订：w103写作59.135秒/
45113 Token，内容复核44.374秒/79287 Token/零finding，结构复核228.249秒/115513
Token/8条finding。总351.243秒、239913 Token，零在途；结构完整执行不等于零业务
问题。新w103实际3段、9个supports，已包含B原文；独立审阅确认其余113个写作单元
原CAS不变、诉讼留空位置保留，30个技术入口与10个评分因素仍有依据，未编造企业
资料或报价。`execution-result.json`、`actual-units.json`及明确新版本DOCX为本轮依据；
此时`workbench.json`的context仍按当前选中旧Word指向旧稿，保留原响应，不冒充新稿
正文。机械审阅以实际新result重建上下文，55章118目标、3分卷、23原表855格562空格
均保留、无自动填空；主稿173书签及分卷书签顺序与清洁v2一致。结构真实请求与完整
离线请求唯一差异为`units[102].blocks_count: 2→3`，来自新w103实际正文；Prompt和
其他请求字段不变，详见`structure/request-diff-from-baseline.json`。

20:28:11 UTC新稿真实Office验收完成，1 passed（用例21.6秒、总24.3秒）：打开、
书签定位、实际输入、显式保存、下载SHA/XML核对及保存后定位均通过。测试标记版
`ad3bc721-4539-431a-b572-d77eee175964`单独保留，marker恰为1，173个书签；已通过
正式close接口关闭会话，当前选择恢复清洁版`3db2a089-3900-4c53-9e39-6364915e1de4`，
下载SHA仍为`c3773a84376741842f80ad037478bd7ee53517b1dbbd3f7be961c0195c6a1ac9`，
marker为0。editor.key、pending_save_id和save_error均为null；本轮Office无模型调用。
证据在`real-office-reviewed-fix/summary.json`、`browser/`及重新获取的
`workbench-clean-selected.json`。分析仍waiting-confirmation，8条结构发现及B范围
解释待核仍保留；Office通过和本次定点修订不代表自动归并或整稿语义已全部合格。

旧稿Office真实测试已打开、定位并输入；因隔离API此前未绕过本地代理，保存回调下载
失败，原缓存地址后来410。保留原pending/失败证据，没有伪造回调、清pending或声称
保存成功；后续使用新稿和已修正的隔离环境验证正常保存链。当前full使用原冻结API
二进制，最新Rust静态CORS修复仅已做路由回归，真实浏览器通过修复后的preview加载
插件；不能称最新API已发布。旧运行、共享服务与实际.env均保持原样。

### 真实定向对照与验收新增问题

版本11第二轮定向已于约14:58 UTC完成：同样4A+3J，12组完整结果、4个joint结果，
0拒收/重试，0在途；墙钟504.896秒（8分25秒），prompt315144、completion112679、
total427823，对应旧16次的653437减少约34.5%。run为
1c0a53f5-550c-4818-9f99-64a93d63a181。真实A/J卷章引用分别始终为r165/r168，短号
误读未再发生；语义逐组检查报告在`targeted-v2/`，完整新流程仍待开始。

大纲修复已完成源码及隔离验证：551项广泛用例通过，6个失败中5个是新参数/错误路径
的夹具更新，1个为冻结旧代码也存在的矛盾断言；针对性34项复验及新增重复坐标保护
通过。归并叶路径另11项先失败，修复后62项通过，批次有重叠不累加。真实旧归并输入
压缩后93441/94208；22个网格文本/空位/合并关系往返一致，11类叶级反馈均只增加56–79，
最大93520仍准入。极大新输出仍需实际准入，不把单例容量当成任意规模保证。

14:32 UTC完成首轮真实定向A+J：4个A批、3个J批，12逻辑组/13候选全部有正式回执，
没有重试或拒收，已确认在途请求0。总耗时543.6秒；对应旧16次调用变7次，prompt
457282→313992，completion196155→129343，总653437→443335，usage全部已知。
新run为269dec68-0a62-4cf0-acaa-35937a268569，证据在新根`targeted/summary.json`及
`comparison.json`。该run是定向局部验收，不伪装完整analysis-ready。

逐条语义复查发现BUG-082：A说明里的局部r号在J变号，产生一条假的证据不符反证；
同一提案另有真实范围疑点，所以原未接受结果仍保留，不能称该轮语义已全通过。
正在修复共同来源短号稳定性，下一定向使用独立冻结版本，旧回执不改写。

旧全流程基线于14:20:57 UTC完成生成及HTTP下载DOCX/ZIP。DOCX有28表、119书签，
但大纲归并在发送前超预算，最终只有一个“待整理内容”根章节；这是流程可继续、内容
验收未通过。BUG-081确认源纠正粗错误路径复制整树和全量assemble超限两处根因。
源纠正已完成51项针对性检查，反馈13860→241字节；同文件按2048纠正余量规划仍6包，
全部准入。全量归并紧凑视图源码已加入，完整网格/原文不截断，隔离回归及真实复测待完成。

旧Office实际打开后因插件公共静态文件缺CORS而无法定位，尚未保存；BUG-083记录
生产静态服务和preview的同一问题，修复范围仅导航插件路径。不得把DOCX可下载或
编辑器打开当作编辑保存验收通过。新完整运行尚未提交，等待这批修复冻结并完成定向。

### 关联合批源码与隔离验证

normalization_version=10首次接入真实TenderNodes，11补共享来源短号稳定性：associate共享输入、逐组接受，joint保留
全部实际依据，明确共同反证可影响此前成功兄弟组；初次加三次按成员在现有artifact台账
原子预留，未发送失败也累计，重启先恢复原物理回包。没有新增业务表、节点或Agent框架。
预算规划及发送同算最多4组、max(32768,24576*k)；CAS回读和目标缩小均不改变公共短引用。

源码正式离线预审仍为192组/195候选→55物理请求，输入估算1199901；同新Prompt的逐组
基线2653287，旧冻结基线2567798。最大总上下文130000；全部原文、条件、表格、别名、
上下文reason和候选保留，55份请求的CAS往返字节/摘要/预算一致。证据在旧证据根的
`batching-real/preflight/formal-summary.json`及相邻完整私有回放，不将输入降幅当真实提速。

隔离验证：主核验及并发86项通过；最终合批/wire/budget32项通过；恢复14项PG/模拟SSE与
网页进度/合批主流程合计43项通过。完整workflow+acceptance首轮320通过、15失败：
14个下游失败来自旧32k测试窗口与顶层单组协议，专用夹具校准后17项均已复验通过，
保留分包/超限及四次重试断言；1个Worker故障注入因测试初始broker与子进程Redis地址
不一致、Prometheus端口占用，统一测试地址/临时端口后7.93秒通过，未改生产网络。
旧非批/Store/Stream等199项另已通过；上述批次有重叠，不累加声称独立用例总量。

新运行证据根`/opt/workspace/kb-tender-evidence/2026-10-28-analysis-batching-real/`，含
`regression/`、`skeleton-retest/`、`final-progress/`、`root-unit.xml`及定向`targeted/`准备。
真实对照选择4批共12逻辑组/13候选，覆盖有决议、共同反证与长来源；原件没有补遗决议，
该类仅有隔离用例，不能宣称已有真实补遗验收。以下为定向发送前准备记录，当前真实
结果及新增问题以上节为准；新完整流程尚未完成。未修改.env。

新版非付费准备已完成：冻结155文件；专用PG库`kb_real_batching_20261028_008430`，
API36407、gRPC57619、Redis DB14，项目`9536e332-d533-50dc-8095-456ec262a94a`。
登录、原件上传/重放及下载SHA核对通过，模型请求仍为0。沿用基线API/migrator二进制，
本轮仅比较Python核验协议；不以此替代另批数据库重设计的验收。

旧冻结基线于13:34:04 UTC进入waiting-confirmation，分析约227.6分钟、283发送，
已知7941727 Token，1份未知usage；未人工确认。associate实际192次全部成功，约117.6
分钟、4546748 Token；joint实际26次。432事项保留，15决议接受、16问题解决，258新关系
仍未验证，hard_blocking=false。附件3“不适用”遗漏尚未得到正确表达；证明条件关系的
反证与未接受决议留在未决中，不能称业务语义全部通过。大纲和编制的后续真实结果及
容量失败见上节；原始回执及审阅在旧目录`analysis-final-audit.md`及output/。

### 此前基线与方案快照

12:47 UTC只读快照：OCR 2/2、Extract 4/4、review 35/35、locate 20/20；associate
已保存133/192组，另2请求在途。已知累计6254266 Token；此前1份uncertain未报告usage，
3次抽取结构纠正和1次review传输重试已恢复，无新增失败。租约正常；joint、最终Verify、
大纲、正文及Word闭环尚未执行完成，不能称全流程通过。原冻结运行继续，不热换协议。

用户明确DeerFlow只参考合理设计，不采用其框架。下一批
[效率方案](analysis-efficiency-plan.md)已统一待实施字段和验收，尚未改运行代码。
离线完整192组/195候选无损重建；最多4组、按组数预留输出的暂定合批为55请求，
输入估算2567798→1317863（-48.68%）。这是associate的假设发送视图，正式字段/Prompt
落地后须重测；joint、实际耗时和新协议语义质量未验，不能外推全流程收益。
证据在本次运行目录`associate-capacity/README.md`、`output-aware-results.json`及
`observed-single-group-output.json`。127份完整单组回执的completion P95=17918、
最大24610、reasoning占92.88%；24k/组仅为试验余量，并非输出保证。本轮只更新设计
文档及离线证据，未部署、未修改.env或在途运行。

遗留复核已更新BUG-074/076：已实现的网页恢复、历史大纲、人工复核、知识版本接线不再
标作待实现；Compose补worker桥接token并让内部API默认端口跟随API_PORT，11项配置
测试通过。此前隔离回归不等于当前联合业务验收。

为同一106页原件新建专用数据库`kb_real_full_flow_20261028_f884b7`，运行当前SQLx
migrator和Python检查点初始化。冻结当前Python代码及Rust二进制，独立API端口59463、
认证gRPC端口59873，dispatcher与Dramatiq worker已启动；不操作现有业务库或旧运行。
原件摘要仍为`4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
本地真实HTTP已验证登录、项目同键重放、认证gRPC上传同键重放、原件下载逐字节一致；
项目为`5f0d555e-5b95-5250-aa4a-d854ed914f8d`，证据见`local-http-checks.json`。
实际`.env`尚缺`GRPC_AUTH_TOKEN`，正式Compose启动前仍需配置；本次隔离API/Python
共用独立随机凭据，未写回`.env`。该配置项不阻塞已启动的隔离测试。

免费原生解析预审确认106页、796范围、98图片位置，4个抽取包，范围数165/210/274/147；
输入保守估算110209/109432/110458/62387，均低于110592准入上限，输出预留16384。
该结果不含真实OCR，不能当最终核验计划或真实Token用量。全分析调用量仍为O+P+R+L+A+J，
后续各阶段依本轮实际模型输出规划；不得用预审4包承诺全流程仅4次调用。

当前配置为`https://code.zleiwork.cn/v1`、`grok/grok-4.7-build-fast@500k`，显式上下文
仍为131072。自动审批首次因新目的地未被明确授权而拒绝；用户随后明确答复“同意，按当前
地址和模型执行”，本轮真实HTTP分析已获准提交，后续将继续大纲及无知识资料编制。
保留原要求，企业应答及报价留空；人工语义确认不会由脚本代填。当前尚未得到最终稿，
不能声称已跑完全流程，实时执行身份和阶段见证据根的`flow-state.json`与`logs/flow.log`。

真实分析run为`622f0570-f910-41b3-b9c0-8b20ea078491`，于09:46:30 UTC提交。
截至10:07 UTC，2次OCR完成、4包中2包入账（271事项、437处置）；另外两包在执行/纠正。
三个首次抽取回包被拒收：一例新关系身份无效、两例顶层必需数组缺失，均保留回执并反馈
实际错误；前两例第一次纠正已成功。详见BUG-077。已有回执累计442924 Token，未计在途
请求，不表示最终预算；已完成的回包均正常stop，不能归因为网络截断。

10:12 UTC前4包已全部完成：432事项、796处置；到10:42 UTC，35个首轮review全部
通过，开始20个locate请求。review期间1次供应商流式api_error未给usage，回执保留为
uncertain，第一次重试成功。已知累计1476483 Token，未知回执及在途用量不计入；尚未
到最终Verify或人工确认。BUG-079的“不适用”问题尚未在首轮review中识别，i44/i45
正文获核对，但T:l21的证明条件关系被识别为混淆，继续观察后续处理，不提前判通过。

11:15 UTC，20个locate全部完成且未重试；正式associate计划为192请求，已开始执行。
此时已知累计2591920 Token。BUG-080记录逐逻辑组调用及joint整包接受耦合带来的
预算/耗时遗留；不能以BUG-078的review合批已修复宣称全流程预算问题已解决。本轮继续
已授权的冻结基线，保留全部原文/候选/回执，不靠删候选减少调用；后续还需joint和编制。

真实基线执行期间定位并修复BUG-078关系补读未合批：新源码协议9离线同台账为14请求、
346910输入估算，原冻结8计划为35请求、426725输入。前12包摘要不变，全部844来源ID、
432事项、116关系和原声明保留。92项相关单测、3项独立PG/合成五节点验收及Ruff通过。
证据为`bug078-*.xml`、`bug078-offline-plan-review.json`。本次真实任务仍执行冻结8基线，
不混用在途回执；协议9的合批尚未做真实语义验收。独立原件与Extract审阅另见
`source-acceptance-review.md`、`extract-acceptance-review.md`，BUG-079暂待最终Verify核对。

证据根：`/opt/workspace/kb-tender-evidence/2026-10-28-real-full-flow/`，含
`runtime-snapshot.json`、`setup.json`、`preflight.json`及各进程日志。该目录私有配置
含本次隔离凭据，不提交或输出；实际`deploy/.env`未修改。

## 用户归属字段收敛（2026-10-28，BUG-075）

按用户最新要求删除`kb_actor_identity` DOMAIN及六处操作者字段，不再保存user/api_key
前缀身份。只有尚未绑定业务实体的上传暂存新增`owner_user_id uuid`用户外键；内部后台
产物可为空，业务发布/撤销/主动清理始终严格比较UUID，None不跳过归属校验。招标文件
通过project_id继承项目owner，对象引用通过业务实体继承；CAS对象不设置单一用户owner。
API Key使用所属用户，原件及Word上传直接传UUID，审计/维护/删除事件只去掉无消费的
操作者列。保留既有Word版本创建用户和编辑会话owner，不扩大到工作流协议调整。

两个专用新库验证17项通过：fresh迁移1、对象生命周期8、双用户/Key HTTP1、Word保存1、
retention6；`cargo check --offline --workspace --all-targets`及diff检查通过。fresh确认无
actor DOMAIN/列、uploads UUID/可空/FK、references无冗余用户字段，同时验证迁移重放、
失败恢复及校验和。首次Word测试因测试库名称不含其要求的tender_web前缀，在进入业务
逻辑前拒绝；改用符合命名约定的另一新库后通过，未修改测试保护或操作现有业务库。

证据根`/opt/workspace/kb-tender-evidence/2026-10-28-owner-user-id/`，结果见`results.json`、
`fresh.log`、`uploads.log`、`owner-http.log`、`word-save-v2.log`、`retention.log`及
`workspace-check.log`。未部署、未改真实.env或冻结runtime；正在进行的106页真实流程
不包含此次schema修改，不能以其结果替代新结构的真实联合验收。

## 标准迁移去掉额外空库检查（2026-10-28，BUG-075）

删除首份SQL中的DO递归扫描，SQLx继续管理版本、校验和、并发锁和每份迁移事务。
同名对象按PostgreSQL正常冲突失败；不冲突的无关对象允许共存，不自动接管旧业务库。
`fresh_database_postgres`在两个独立新库先失败后通过：原代码错误地返回55000；删除
门禁后返回预期42P07，原夹具行保留、前置DOMAIN回滚、没有成功迁移回执。夹具表改名
后不删除，后续三份迁移、并发恢复、重复执行与校验和拒绝均通过，最后核对该行仍存在。
证据根`/opt/workspace/kb-tender-evidence/2026-10-28-sqlx-migration/`，见red/green日志及
结果JSON。只修改未发布的新初始化源脚本，未对已运行库重放、清台账或更新校验和；
真实106页任务继续使用冻结基线。文档与diff检查通过，未部署。

## 网页能力闭环（2026-10-28，BUG-074/076，后续修复）

已接入分析失败同run续跑，保留来源、成功付费回执及原模型工作单元额度；未知提交冻结
body/key跨刷新恢复。早期失败沿run/command的修订身份，不把来源回执的revision=0当运行修订。
结构化事项纠正已补业务表单，覆盖类型/主体/领域/范围/条件/例外/数值/评分/后果/提交要求，
证据与完整包由服务端冻结恢复；继续用既有完整纠正及重新核验合同。

大纲可手动设置独立分册和封面角色，保存到不可变修订。章节定向复核选择真实响应及理由，
提交时固定前次成功稿的大纲、来源及目标身份；新结果保持候选稿，人工Word不被覆盖。
展开复核表单导致Word网格挤缩、工具栏被覆盖的浏览器反例已修复，工作区可纵向滚动。

保存Word后的只读语义复核为独立持久任务，输入为明确version_id/SHA对应的实际保存字节。
段落、表格、页眉/页脚及注释文字进入复核，硬换行和制表符保留；未知章节、缺失书签、
不可读图片/签章、跨批一致性与最终排版均保留明确人工核对出口，不声称全稿合格。
原始分析run/source/revision与来源是否更新分别展示；新补遗不改写旧报告的依据。
解析/来源准备及分包在工作线程中进行，避免阻塞租约心跳；已有HTTP同键回放不重新读S3，
完整请求仍由Python比较，新任务继续校验真实Word字节。未新增业务表或改变初始化SQL。

本轮前端67项单测、完整87项浏览器回归、build/lint通过；Python六组最终XML去重153个
唯一用例通过，无失败/错误/跳过。Rust API常规25项通过，外部夹具6项默认忽略；本轮新增
保存稿回放夹具已独立执行1项通过，验证真实JWT HTTP Router→认证gRPC→Python及当前
三份实际迁移，覆盖同键202、变更409、缺原件新请求503、跨用户404与跨项目隔离，模型请求0。
这些门禁使用合成文件、模型/Office替身；本轮未启动新的付费模型任务，未正式部署，
也不替代上方正在进行的106页真实业务流程验收。真实deploy/.env及并行BUG-075保持原样。

证据根：`/opt/workspace/kb-tender-evidence/2026-10-28-complete-web-fixes/`。
最终浏览器记录`frontend-final.log`；后端为`correction-final.xml`、`root-bridge.xml`、
`recovery-final-v2.xml`、`saved-word-bridge.xml`、`saved-source-final.xml`及
`saved-review-execution-final.xml`，含重叠故以测试身份去重；HTTP专项为
`http-review/authenticated-http-{replay.log,result.json}`；去重汇总为`verification-final.json`。
本轮独占PG容器/匿名卷、临时依赖容器、HTTP桥接夹具和新建测试库均已清理，32792/4184
连接拒绝；回执为`cleanup-final.json`与`http-review/cleanup.json`。正在运行的真实全流程
任务及共享服务未操作；代码未提交或推送。

## 网页遗留收尾（2026-10-28，BUG-074/076）

已修复大纲连续未知保存后刷新丢后续编辑、Word默认只读刷新残留pending导致无法切换。
重试保持原body/key，恢复存储单独保留最新工作树；Word只更新已固定阅读版本的真实
会话元数据，不切换稿件或伪造本地保存确认。

大纲新增历史列表、结构预览、另存恢复及响应重排；target_ids只能选择同生成执行、
同authoring_input的原始/祖先定义，完整规则/来源/原表由服务端继承。空历史可另存，
目录失败明确显示未知，历史读取与响应目录独立重试，提示固定当前来源而非最新候选。
未指定固定大纲的任务在提交时固化authoring_input_ref，避免排队期间分析修订使其失败。

项目资料可显式选择知识资产固定版本；worker通过60秒purpose/execution受限内部JWT
回读，任务不存用户凭据，API逐次重验项目/产品/版本/原件关系。检索失败保留待补。
同时修复解压后的响应被再次解码，以及资产停用后已有同键任务被重新准入阻断；已有
任务仍经Python完整请求比较，新任务和真实资料读取继续执行当前权限校验。

招标解读新增逐项人工判定、确认、事项文案纠正与重新核验；数字/条件/证据等结构保持
原值，页面明确提示完整修订出口。未知请求保持原键，409保留输入并可对照最新版本，
输入纳入离开保护且有显式放弃。确认和页面操作新增0模型调用，纠正沿现有R+L+A+J
请求包核验，来源/提取复用；本轮验证均用合成回包，没有调用真实模型。

本轮前端65项单测、65项完整浏览器回归及build/lint通过。Python域/合成DOCX74项、
桥接存储36项、追加catalog2项、人工分析12项、知识/原件46项及key golden2项通过；
各组有重叠，不相加。Rust常规API24项通过（外部夹具5项默认忽略），知识归属和同键
准入各自另跑真实PG专项通过；部署配置9项、本轮Python Ruff及diff检查通过。

证据根为`/opt/workspace/kb-tender-evidence/2026-10-28-remaining-fixes/`，含
`domain-final.xml`、`bridge-final.xml`、`history-final.xml`、`analysis-final.xml`、
`web-knowledge-final.xml`、`key-golden.xml`及`frontend-final-65/`。
真实HTTP/认证gRPC及当前SQLx迁移专项位于`backend-bridge/`；最终运行及清理回执在
该目录单独记录。本轮浏览器使用Office替身，未完成新大纲/知识/真实模型/真实Office
联合业务验收，也未正式部署；保留用户并行BUG-075及真实`.env`，未提交或推送。

跨层收尾：当前Rust HTTP Router→认证gRPC→Python测试1项通过；独立真实HTTP脚本7门
通过，覆盖当前三份实际SQLx迁移、历史/detail正向和跨项目/异用户隔离、分析确认/文案
纠正真实ledger及重放/过期冲突、知识版本准入、实际Python短期JWT经Rust HTTP回读
metadata及wrong job/version/login token/archive/cancel拒绝。18条分析模型回执全部
为synthetic且external_request=false，正文模型请求0；未测真实知识search/blob或真实模型。
证据为`backend-bridge/{verification.json,rust-authenticated-http.log,live-http.log}`。

本轮独占API18089、gRPC40489及独立数据库已清理；任务专用PG容器
`kb_remaining_test_20261028`及独占匿名卷已删除，4184/18089/40489/32794真实连接均拒绝。
原共享Office、Python runner及用户并行BUG-075/业务服务保持不变。根目录
`verification-final.json`、`cleanup-final.json`及`backend-bridge/cleanup.json`为最终回执。

## 数据库职责、统一命名与标准迁移（2026-10-28，BUG-075）

按用户确认只考虑全新初始化，旧 Rust 招标工作流及专用表/队列/脚本退出；Rust 保留
API、知识库与通用 worker。业务归属在应用层按创建人隔离，API Key 继承所属用户，
不保留成员角色与独立 scope。数据库保留事务、主外键、引用、outbox、租约和付费回执，
三领域无自定义业务函数或触发器。命名约定见[数据库命名与关联约定](../database-naming.md)。

初始化已从无版本 baseline 改为三份时间戳 SQL 与标准 SQLx migrator：
`20261028000001_shared_platform.sql`、`20261028000002_knowledge_base.sql`、
`20261028000003_tender.sql`。现有部署入口不变，新增版本自动编译嵌入；每文件和台账
同事务提交，失败版本回滚，已完成版本保留。旧未登记业务库明确拒绝，未来增量通过
新增版本升级，不改写已执行文件。并行大纲编辑表及所有业务改动均保留。

已实际验证：新隔离库56张业务表（10/26/20）及SQLx内部台账，0自定义函数/用户触发器；
迁移顺序/故障回滚/并发恢复/校验和/数据保留综合场景通过，实际migrator连续启动通过，
普通应用账号在预装扩展的空库连续启动也通过。知识库命名后278项通过；对象/retention
11项通过；项目及Word真实PG回归各1项通过。Python命名及迁移路径回归211项通过
（522.02秒，0skip），包含并行大纲逻辑，先前缺方法的4项失败已不再出现。

补查修复两个应用权限旁路：正文提及对象不能成为下载凭据；本地及LDAP配置都禁止
profile改写登录身份。已先复现失败，再在两种配置下执行双用户HTTP测试通过；认证
单元4项通过。未连接真实LDAP。部署离线配置9项、CI来源与diff检查通过。

用户随后要求将状态约束集中在业务代码；已按实体补齐Rust enum/Python StrEnum，
移除未知状态默认pending/active/done的宽松转换，不新增数据库状态限制。Python六类
宿主状态在持久化读写、工作流和调用边界共用定义；Rust平台/项目/编辑/保存五类状态及
知识库九类状态贯穿解码、业务分支与写入，JSON值不变。状态收敛后知识库281项实际通过，
含9类38个合法值及各类未知SQL text解码拒绝；平台2单元及6真实PG、API14单元及2真实
项目/Word回归通过。全workspace Clippy `-D warnings`、Rust workspace测试及fmt/diff
检查已通过；Rust常规组有28项显式忽略，不把它们计作实际PG验收，数据库结果以上述
独立专项为准。全量并发测试曾复现问题生成单测读取了另一个测试临时设置的模型环境，
仅给实际调用索引的3个测试补已有TEST_ENV_LOCK后通过，生产模型与网络配置未改。

Python枚举后完整回归为264通过/1夹具失败（575.83秒）：唯一失败的检查点围栏测试直接
调用graph却省略生产runner一直提供的status，已补RunStatus.RUNNING。随后相关20项
复测通过（2.16秒），含新增未知/缺失状态不得进入confirm的2项，未剩未解决失败；
合计覆盖267个不同用例，不表示267项在一次运行全部通过。上文211项和278项为状态
收敛前结果，不与新结果相加。Python StrEnum保留JSON字符串值；LangGraph内部msgpack
可使用枚举编码，不宣称检查点二进制字节不变，真实检查点/租约恢复由专项覆盖。

证据目录：`/opt/workspace/kb-tender-evidence/2026-10-28-database-redesign/`，
迁移反例为`migration-before-fix.log`，通过结果为`migration-sequence.log`、
`migration-cli-repeat.log`、`migration-application-account.log`及
`python-migration-authenticated.log`。状态验证见`knowledge-state-validation.md`、
`platform-state-enums.log`、`api-state-enums-postgres.log`、`python-status-regression.log`、
`python-status-focused.log`、`state-final-clippy.log`及`state-final-workspace-tests.log`。
专用PG/Redis测试夹具暂保留供并行验收使用，未清理共享Python runner。仅使用专用PG/Redis
与合成资料，未运行真实模型、
未部署、未修改现有业务库或`.env`，未提交或推送。

## 网页大纲编排与固定修订正文（2026-10-28，BUG-076）

本轮补齐“生成大纲→显式载入候选→网页编排→保存修订→按修订生成正文”。编制页提供
大纲编排/正文编辑两视图，章节支持新增、重命名、删除、排序、缩进提升及原生拖拽；
按钮与拖拽等价，循环拒绝。已有成功编制的大纲可直接作为候选，不必重新付费组织。
新章可填编写目标，留空则只生成标题；原章节的表单、附件、规则和来源引用由服务端继承。
删章保留原要求和移出位置的问题，不伪造已满足状态，不自动补回人工删除的章节。

新增`tender_outline_revisions`保存不可变大纲，项目行锁校验父修订；同键异内容经真实
认证gRPC明确返回409/REQUEST_CONFLICT。保存未知保持原payload/key，平台返回规范化
大纲作为已保存基准，保存后的继续编辑仍保持未保存。来源更新给出重生成出口。
正文任务固定revision/hash对应的完整大纲及原authoring_input，跳过组织阶段；模式及
outline_ref固定进执行规格，不能在同ID下更换。默认整稿路径保留，前序正文仍按完整
请求摘要复用，未指定前序不宣称跨执行复用。网页修改不直接重排当前Word，新正文仍
作为候选；两视图切换保留Office实例和修改状态，离开项目同时保护大纲和Word。

本轮验证分别为：新增域/投影/真实DOCX25项通过（domain-final.xml）；既有扩展域集
80项通过（与新增用例有重叠，不相加）；前端61项单测、46项浏览器回归、build及lint通过。
当前fresh schema的真实HTTP/认证gRPC/Word保存合同14项通过（0忽略），Python桥接和
健康探针44项、传输3项通过；实际migrator在专用空库连续两次，fresh专项1项通过，
20张业务表与3张官方checkpoint表的真实只读查询通过。并行BUG-075改名后健康探针
选列按实际baseline修正，没有恢复旧SQL、旧列或加入兼容建表。

真实合成DOCX核对了标题顺序/层级、删除、表单文字与合并及空位、稳定章节/响应书签，
并检查编写目标进入实际任务和正文、原始来源不变。此为固定模型替身与实际文件验证，
没有调用真实模型、没有重新验收真实招标语义或正式部署；本轮浏览器使用Office替身
验证两视图会话保护，未将前轮真实Office结果冒充新大纲全流程验收。

本轮主要证据根：`/opt/workspace/kb-tender-evidence/2026-10-28-outline-workbench/`，
含`domain-final.xml`、`outline-edit-export.xml`及`backend-bridge/`；实际合成文件为
`pytest-outline-edit-export/test_human_outline_projection_0/edited-outline.docx`，同目录有
`word-document.xml`及`export-summary.json`。浏览器最终46项及截图保存在
`/opt/workspace/kb-tender-evidence/2026-10-27-web-integration/outline-final/`。
最终已停止本轮gRPC夹具并确认进程不存在，删除`kb_outline_test_20261028`及独占匿名卷；
共享Office、原Python runner和BUG-075专属PG/Redis均仍运行。未启动本轮常驻API，
4184浏览器测试服务已退出；清理回执为`backend-bridge/cleanup.json`。测试证据和实物保留，
未改真实`.env`、提交或推送。

## 前端工作台及真实Word接入（2026-10-28，BUG-074）

本轮接通v3项目权限、资料上传/原件下载、Python持久任务与工作台、同版要求/依据/待补，
正式正文仅在ONLYOFFICE读取/编辑。新生成稿保留为候选，不覆盖当前人工稿；显式保存
以同会话/父版本/文件哈希相关的签名回调发布不可变版本。页面读取/导航/保存/下载不调用模型。
开发runtime纳入Python三个角色并共享DocReader的数据库、模型和artifact卷，认证桥接token
显式配置。Word映射最初通过增量接入，随后随用户并行BUG-075统一到tender_baseline；
保留用户初始化方案，不恢复旧SQL或旧增量。正式release已接八服务/五镜像及严格阶段，
18项合同回归通过，Python健康探针10项单元测试通过；未实际运行正式发布。

隔离验证：Python95个唯一用例通过、实际认证gRPC传输2项、Rust API13项、Word导航/导出46项、
部署离线配置9项；此前platform单元63项和migrator诊断1项通过，另2项明确忽略。
在BUG-075当前fresh-only结构上另建新专用空库，实际migrator连续执行两次，验证Python/Word
19张业务表齐全且保留测试用户，fresh专属integration 1项通过；最新cargo check -p api通过。
随后同一新库复跑Rust API全组13项全部通过，包含真实HTTP→认证gRPC→当前Python只读setup、
上传/资料原件/工作台及Word保存回执，不再仅依赖旧schema的同名测试结果。
前端最终完整30项浏览器回归（其中插件7项）及npm lint通过。
当前fresh schema另有关键Python25项通过（55.32秒，0skip/0error）：桥接/jobs21项、
大纲检查点1项、AuthoringStore恢复/取消/只读setup3项。标准CLI init及真实22表只读检查通过；
实测修复健康探针将checkpoint_writes.blob误写为value的问题，反例回归已保留。
这不代表已启动并验收worker/dispatcher消费者或完整生产健康。JUnit证据为
`current-schema-python-key-tests-final.xml`，仍位于本轮外部证据根。

真实ONLYOFFICE8.3.3合成文件验收已通过（1项，10.5秒）：实际view插件GoTo→edit键入marker
→顶部显式保存→签名status=6回调→不可变v2→下载sha256与版本元数据一致。独立检查Word XML
保留marker及两个chapter书签，保存后重新绑定GoTo成功。此前反例确认直接JWT带iat/exp导致
status=1回调403；已修复并保留签名/过期/业务内容校验。该验收只使用合成文件及任务专用库，
没有调用真实模型，没有读取真实招标资料，没有执行正式部署。最终高度布局再次真实验收
1项通过（8.5秒），保存到不可变v4，新增marker入稿、下载hash一致及保存后GoTo再次成功。
两轮真实Office均使用此前固定API二进制及专用旧schema，不能宣称已覆盖当前fresh schema
的Office完整端到端；当前fresh schema由上述13项真实HTTP/PG/gRPC专项覆盖。

证据根为`/opt/workspace/kb-tender-evidence/2026-10-27-web-integration/`；最终实物位于
`real-word-final-verified/`，首轮位于`real-word-verified/`，包含`saved-synthetic.docx`和
`word-after-save.png`。测试token不打印、不提交；新增当前schema连接配置不含密码，凭据仅经
测试进程环境传递。最终已撤销仅任务fresh库的临时CREATE权限，停止本轮API和三个gRPC夹具，
删除专用PG容器`kb_tender_web_test_20261027`、未使用的专属空Redis容器
`kb_tender_web_redis_test_20261028`及各自独占匿名卷，并只读确认已不存在。
既有`kb_authoring_test_20261026` Python runner和共享`knowledgebrain-onlyoffice`仍在运行，
业务服务与真实`.env`未改；DOCX、截图、JUnit及回放脚本保留在外部证据目录。
本轮不提交、不推送、不修改真实`.env`。

## 参数引用位置的规则混绑优化（2026-10-28，BUG-071）

进一步核对S/A原始回包：S未生成独立主要参数参见位置，A借内容候选t76建该位置，
宿主继承其报价填写及否决规则。旧A合同不允许仅以related_target_ids创建纯参见，
属于协议表达缺口。共享全部已安排规则会令输入从89400增至至少107646，超94208，
未采用重复全量发送、删条件或提高预算。

先补问题记录、字段字典及五项调用/容量方案；沿已有字段允许narrative纯参见，
不继承被参见位置的规则/模板/企业内容。来源验证仅在规则、来源、模板皆空时
记录未绑定，不再给真实来源支持的引用位置误报；未读、未知、多实例及未安排候选
继续保留诊断。已解析且无自身内容的纯参见不派空写作任务、不显示多余正文待补；
旧真实正文及诊断仍保留。未修改S提示、部署或.env，不涉及其他并行Rust/网页任务。

反例分别先复现4项来源误报、5项纯参见合同/关联失败及4项空任务/待补失败。
修复后来源相关35项、合同/保留79项、配套含书签94项通过（模块重叠，不相加计数），
本轮8个修改Python文件Ruff及diff通过。免费预审4S完整摘要不变并成功回放，实际A
89500/94208准入，未增加来源读取或模型阶段。

独立`document-reference-routing-20261027-v1`使用冻结runtime完成真实复测，沿用
专用测试库中前一成功执行以复用完整相同请求；旧运行和证据保留。仍基于旧363规则
快照编制，无知识/企业资料，非新五节点分析。证据目录为
`/opt/workspace/kb-tender-evidence/2026-10-27-remaining-fixes/reference-routing-v1/`；
耗时130.96秒，复用6份完整相同请求回包（4S及2写作），新增1A+4写作+2复核共7次
发送，供应商报告309232 Token，0失败/重试/未知用量；该耗时包含来源缓存重放，
不是重新阅读原文全流程。冻结代码指纹一致。同ID恢复同结果、0新增请求/回执，
0.160秒，旧成功运行及文件未覆盖。

独立实物审计通过：C仅显示“主要技术参数”及“参见2.2.2 专用技术要求”，没有
报价/否决规则、要求底表、独立写作单元或多余待补。19条核心参数完整保留；上轮
91条技术规则在新全稿全部可见，90条仍在技术卷，随机备件计价规则移到报价位置，
不是漏失。技术卷98个章节要求行全部正文及有序引文保留；报价i73在实际报价位置、
否决i98在专用技术风险要求保留，证明/评分/偏差条件均保留。363底稿逐字段不变，
24张原表文本及合并结构无增删；61单元全placeholder，0企业填值/证明，3处人工价格空白。

总稿、ZIP、三分卷及底稿/待办与manifest一致，技术分卷的要求表及C文字与总稿相同。
封面无要求底表，其要求及原引文在提交待办完整。交付目录为
`reference-routing-v1/document-reference-routing-20261027-v1/`，技术卷`volumes/002.docx`，
最终证据`independent-reference-routing-audit.json`。源码未部署；当前验收是已有规则
完整保留与引用位置正确展示，不代表全原文语义召回已完成。F关于目录层级、分项
报价引用、分册封面/可选附件归属的意见仍保留待核，未自动重排或宣称外部提交完成。

## 技术响应要求入表复核（2026-10-27，BUG-063/071）

用户指出技术响应不全。再次对照原件及最新display-revision-v1：旧稿技术卷81条
已绑定唯一规则中，仅71条进入依据表；其余部分由原表保留，部分实际遗漏。原文
物理59—60页七项★功能的证明及原厂盖章要求已抽为i29，只绑attachment，导出
按narrative过滤而漏掉；产品先进性证明及偏差表条件也有同类问题。核心技术段
i1—i48中47条正文已在分散表格，不能称核心参数全未抽取。20张依据表151行仅85条
唯一规则，不能再用总行数证明完整；主要技术参数位置只绑否决后果且无实际参见。

先更新BUG-063/071、字段字典与方案，再修宿主导出：每章汇集所有目标的完整有效
规则引用，精确去重后显示一份要求依据；原表及不同主体实例不合并，企业应答及报价
仍空白。红测5项失败，修复后全部7个导出相关模块98 passed，引用/结构42 passed
（存在模块重叠，不能相加作为唯一测试总数），Ruff/diff通过。来源/组装提示沿已有
constraint_range_ids传递实际跨位置引用依据，由模型选related_target_ids，不按标题
搬规则，不新增合同字段、来源轮或生成门禁。

独立`document-technical-coverage-20261027-v1`完成编制复测，仍用旧363规则冻结
快照，不是重跑五分析节点。无知识/企业资料、无人工补选submission_sources；新来源
提示使4S重新付费，不声明旧S复用。免费预审4包输入94120/93655/94203/85275，均
≤94208，输出预留32768，并发2。证据在`/opt/workspace/kb-tender-evidence/2026-10-27-remaining-fixes/technical-coverage-v1/`。
耗时293.26秒，4S+1A+6写作+2复核共13次真实发送，供应商报告703464 Token，0重试/
失败/未知用量；同ID恢复同结果、0新增请求/回执，0.121秒。64单元全placeholder，
0企业填值/证明，3个人工价格目标保持空白。

独立实物审计确认技术卷91条已绑定唯一规则对应108个章节要求行，逐章全部正文和
完整引文保留；核心参数19条各在专用技术要求出现一次，通用及主要参数位置明确
参见2.2.2，不再复制整套参数。i29七项功能证明及原厂章、产品先进性评分/证明及
9条偏差条件完整入表。363底稿逐字段不变，24张原表无增删，表文和合并结构一致。
这是已知规则成稿保留验证，不是全原文技术召回验收；C本地仍绑定报价填写/否决
说明，虽已有正确参见，其语义归类仍待改进。F关于目录层级、分项报价、分册封面及
财务主体的意见保留待核，不自动当事实或宣称全部解决。

导出修复的封面回归先由1个失败反例确认，再修为原封面不插依据表，相关要求及
引文进入可读提交待办。最终8个导出相关模块112 passed，Ruff/diff通过。独立
export-revision-v1零模型重导出保留成功执行与旧文件，最终审计通过：封面无依据表、
封面要求和引文进入待办，技术卷规则/空位及24原表不变，ZIP/总稿/3分卷与manifest
逐项一致。交付文件在该修订目录的`投标文件.docx`、`投标文件及附件.zip`，技术卷为
`volumes/002.docx`；最终证据final-technical-coverage-audit.json。源码未部署。
并行任务修改integrations/web.py，宽范围code_unchanged为false；编制相关源码运行时
未变，运行后封面导出修订单独记录，不把该宽标记或重导出当作新全流程验证。

## 剩余缺口统一修复与验证（2026-10-27）

用户授权继续全部修复。先补BUG-065—068及BUG-053历史状态说明，统一字段字典和
调用/容量/出口/恢复方案。实施拆为未安排规则及跨目标引用、知识证明原件、正式布局、
持久语义评测；沿既有S/A/写作/复核流程，不新增主节点或门禁。用户明确没有真实
企业资料，本轮继续验证正确留空，不再要求知识库输入。旧产物和回执保留，.env和
部署不改，不把人工GT或真实资料匹配验收自动标为完成。

完整unit及编制相关隔离回归1567 passed，357.49秒；最新衔接先补失败反例，再由
最终56项专项通过，8.31秒，Ruff/diff通过。测试使用既有隔离容器、专用PG库、
每例schema和空Redis DB15，不清理共享业务数据。

### 无资料编制与目录衔接复测（BUG-065/069/070）

独立v1 `document-completion-20261027-v1`完成119.85秒，继承4份来源回包；新增
1A+6写作+2正文核对+1F=10次发送，437601 Token，0重试/失败/未知用量。
同ID恢复同结果，0新请求/回执，0.147秒。363/363规则逐字段保留，24原表、903锚点、
21合并及598空白完整；66单元全placeholder，正文/填值/证明各0，3处人工价格保留。
原48条未安排规则变为1条绑定、3条交付约束、20条按原文条件明确排除、24条仍待核。
5个排除issue对应20条规则；gaps按code/detail去重后的1项不能当未安排规则数量。

完整A初测94601/94208超准入，根因是290部件重复25种完整标题路径；复用现有标题
共享投影后85223准入，省9378输入估算Token/33620 bytes，规则、原表、空位及所有
语义逐字段不变，未提高窗口。实物检查显示表齐全却将独立技术附表组成降为专用正文
子节；3.1.2及附表正文虽在A/F，真实技术目录未交接，解析标题路径还继承错误。

已追加BUG-069/070及字典和五项容量说明：来源S用原constraint_range_ids传递目录/
组成依据，原文高于定位路径；A/F核对父子层级而非只看先后。F实际投影未安排/排除
规则语义、引文及理由，共享支持标题，不新增核对轮/门禁。新S Prompt使来源摘要
改变，必须重新付费，不继续宣称4S可复用。独立v2不传任何人工补充submission_sources，
4S离线输入94144/94200/93947/83502均≤94208；首包488来源含真实技术目录及3.1.2。
v2已完成302.18秒，全部16调用重新付费、933019 Token，0重试/失败/未知用量；同ID
恢复同结果、0新请求/回执，0.151秒。实际S自主选择目录和3.1.2，A/F都收到，技术
通用/专用/附表同级恢复。57单元56空稿、1段来源要求说明，363规则/24原表保真。
另发现参数位置只留空标题、三份规定文件却全is_volume=false；追加BUG-071/072，
没有因F意见多就新增门禁或把每个来源疑点都自动变成章节。

v3只澄清A/F既有字段语义，S完全不变。完整A禁外发预审86788/94208，4份S摘要
一致且重放成功，无人工source注入。`document-completion-20261027-v3`完成138.45秒，
继承5份完整请求一致的回包（含4S），新增13次调用：1outline/7authoring_write/
5authoring_review，693069 Token；0重试/失败/未知用量。此时间包含缓存来源重放，
不能当重新阅读全部原文的耗时。同ID恢复同结果、0新请求/回执，0.181秒。

实际ZIP含总稿及商务/技术/报价三份独立Word，manifest/CAS/实际成员和分组一致；
参数位置已有真实narrative目标和待填空位，没有造原表。该处原文只有“详见技术
规范书”，未建立明确指向正文参数目标的参见，不能宣称引用映射已验收。全部363规则、
24原表/903锚点/21合并/598空白保留；20张底表151行引文精确，企业应答空白。
62单元中61空稿、1段有来源的要求说明，无企业参数/实际承诺/填值/证明或价格代填。
当前未安排27规则，多为其他参与方程序及待核范围，不能按数量自动塞正文或判不适用。

v3正常中文PDF总稿68页，商务23/技术43/报价4，抽查7张代表页未见裁切；总稿页数
增加与要求底表97→151行及目标安排变化有关，不代表增加了有依据的企业正文。
F四项保留待核：分文件封面/可选附件归属、分项报价引用、财务主体及签署、履约保函
适用范围。现有原文不足以自动复制封面、编造附件或删条件实例，仍沿待核草稿出口。
最新代码未部署，真实企业资料匹配和人工GT仍未完成。轻微重复展示标签另记BUG-073，
只作零模型重导出，不覆盖本次成功执行或重新付费阅读。

BUG-073已完成：actual DOCX反例先2 failed/3 passed，修复后四个导出相关模块
66 passed/0 skipped/0 failed，8.90秒，Ruff/diff通过。独立display-revision-v1在禁网
环境从同outline/62单元/review/requirements重导出，新调用/Token均0；旧成功执行及
Word/ZIP哈希不变。审计0问题：底稿和完整待补JSON逐字节相同，原表XML及三卷保留，
151行空响应及人工价格不变，仅总稿/技术卷各7处生成标签和待办17处同名标签显示修正。
最终可交付留空稿位于real-authoring-v3/display-revision-v1/的`投标文件.docx`及
`投标文件及附件.zip`，revision-audit.json/revision.json明确原v3累计用量与本修订0新增
调用的区别，不把显示重导出当新付费或覆盖旧运行。目录实际页码及上述原文待核仍保留。

本批合计214次真实发送（含32次合成评测），供应商报告3985109 Token；无失败/重试/
未知用量。真实分析、三轮编制及评测分别计量，不把缓存重放当新付费或把不同模型的
耗时变化当代码单因果收益。显示修订新增0；原回执和旧产物全部保留。

v1正常字体转换实际查看8个页面，封面、用途、跨页表头、列宽、空应答/价格可读；
Word有TOC域，但原x2t不更新目录页码。docbuilder私有更新副本中文缺字且分页错误，
已拒绝交付，不替换原Word，不将字段存在称为页码实物验收完成。字体工具曾意外
覆盖OnlyOffice FileConverter的三份既有缓存；已保留覆盖版本，并按同版本/同原字体集
私有重建恢复，生成Web字体表与未改原Web表逐字节相等，原Web哈希未变，无服务重启。
还原前后校验及备份见v1/visual-audit/font-cache-recovery/restoration-audit.json。
本节证据均在`/opt/workspace/kb-tender-evidence/2026-10-27-remaining-fixes/`的
real-authoring-v1/v2/v3目录，包含原回执、预算前后对照、独立数据/语义/视觉审计和恢复证明。

### 新完整分析实测（normalization8）

独立运行`6597eb6f-e638-466d-b556-30fb9d161989`从同106页原件重新登记、冻结、
2份唯一图实际OCR、抽取及独立核验。runtime复制冻结，原件及代码SHA核对通过。
总701.54秒（11分41.54），抽取207.90秒，核验478.84秒；实际最大并发2。
143次调用全部成功：2OCR+4抽取+11review+41locate+81associate+4joint，0重试，
供应商报告1839506 Token，未知用量0。OCR逐出现位置callback98次不能当98次付费。
review/locate/associate/joint实测分别57.79/130.17/215.22/16.66秒，核验总计另含规划/
持久化；本次是terra+n8，旧sol+n6核验56分33秒，不能把所有差异都归因代码。

4包187/250/335/24范围，共796原生范围/796处置精确覆盖一次；241事项，551事项
引文+82关系引文+20决定引文均可回溯，443 CAS及138份runtime文件SHA一致。
终态waiting-confirmation，0人工确认；589个人工问题、259条未决处置、25条未复核
关系和0条接受决议保留，未自动confirm或宣称analysis-ready/语义验收。旧363事项
不是人工GT，不能只按数量差异断言本次漏检或召回通过。源码运行与完整语义验收分开。
证据：同父目录full-analysis/的status/output及independent-full-analysis-audit.json/.md。

### 持久语义评测真实执行（2026-10-27，BUG-068）

新增caller-owned Python执行器，复用独立分析Store/Lease及现有模型付费围栏，
不新增CLI、表或主节点。冻结本轮Python/Prompt副本，使用专用PG库及独立schema
`semantic_evaluation_20261027_v1`，没有运行或更改真实招标分析。合同、部分失败、
预算、标注隔离、用量和恢复相关47项隔离测试通过，90.60秒；Ruff及diff检查通过。

当前gpt-6.1-terra、131072窗口、并发2及同已校准endpoint下，32份v2合成独立核验
输入全部准入，累计输入估算95472，单次2826—3181 Token，输出预留32768。真实
执行`synthetic-semantic-v2-20261027-v1`完成46.03秒，32/32回包获宿主合同接受，
32次发送，0重试/耗尽/预算超限；供应商报告79408输入+2506输出=81914 Token，
未知用量0。同一租约内再次执行同身份返回同报告，0新请求行、0新发送。每30秒
保存进度、原始回包及拒收/预算/用量引用；.env及部署未改。

逐案Agent阅读的31个整体判断方向与草案一致，1个`table-unit-consistent`存在标注
分歧：原文要求按表价填报，候选写成投标人的实际单价，模型认为改变义务性质。
该反馈有合理依据，保留原草案等待人工裁定，没有自动改标或计算正式准确率。
`human_review_completed=false`、`semantic_acceptance=false`保持；本轮不验证抽取
召回、五节点全流程或真实投标成稿。隔离Store以blocked释放评测租约，仅因既有
API无评测终态，metadata明确`evaluation_completed=true/workflow_executed=false`，
没有创建业务门禁。报告状态独立保留，不将blocked误称模型执行失败。

证据：`/opt/workspace/kb-tender-evidence/2026-10-27-remaining-fixes/semantic-evaluation/`
的result/report/resume-proof/owner-run/progress及agent-review；隔离回归证据为
`/opt/workspace/kb-tender-evidence/2026-10-27-semantic-evaluation/green.log/xml`。

## 正式业务标题与可读细则修复（2026-10-27，BUG-062/064）

用户指出仍有内部JSON及来源格式编号标题。已先更新问题记录、字段字典及调用/容量
方案。BUG-064仅改要求展示：按已有字段的业务标签显示嵌套细则，不再json.dumps进入
Word；隐藏kind/domains，0/false、成组量值及原文代码字符串保留，完整底稿不改。
actual DOCX反例先5 failed/1 passed，相关79项回归通过，Ruff通过；最终42模块完整
隔离回归494 passed，158.79秒，未以此代替真实语义验收。

BUG-062已实际修改outline_assemble Prompt，区分业务标题和来源格式标签，不默认
一表一章；合章不合并独立填写实例，保留角色/条件/人工价格边界。明确选取必要原格式，
不截表体/续表/备注/签署，不使用附件号或标题正则。来源Prompt和模型回包schema不变。

独立执行`document-business-outline-20261027-v1`已启动，同106页冻结分析、同
gpt-6.1-terra/131072、并发2、无知识/企业资料。离线完整A预审准入，4份来源请求摘要
均未变并已重放，109候选及19完整网格保留；A和实际变化下游需新付费，未把旧A当成
新标题验收。源码指纹固定，旧运行及.env不改，未部署。证据：
`/opt/workspace/kb-tender-evidence/2026-10-27-title-and-readable-output/`。
v1已完成113.96秒，继承4份来源回包，新增10次调用、431950 Token，重试/失败/未知
用量均0；同ID恢复同结果、0新增请求/回执，0.155秒。商务标题已按业务分组，原
“8A 商务部分摘要表”不再独立作为正式标题。但独立审计发现技术卷被改成5个主题章，
压平原规定层级并重新排序；最终F也报告technical_order，不能宣称结构已合格。
已先补BUG-062及字段/方案，在同一次A明确规定结构高于业务归并与标题简化，仅
纯标签可作为冗余省略，实际填报说明保留。独立v2执行
`document-business-outline-20261027-v2`已完成114.42秒，继承4份来源回包；新增
1A+6写作+2正文复核+1F=10次调用、432796 Token，重试/失败/未知用量均0。
四份来源摘要仍一致，完整A估算85551/94208输入Token准入，19完整网格及109候选
不减；没有新增节点、全文件源读取或标题规则引擎。同ID恢复同结果，0新请求/
回执，0.112秒。运行指纹不变，保留v1及全部旧结果、原文和回执，.env不改，未部署。

实际Word共29个正式标题，与最终树一致；原“8A 商务部分摘要表”已组织为
“2.5.1 商务摘要及基本情况”，另有“2.5.2 股权结构”“2.5.3 财务状况”等业务标题。
技术卷恢复“通用部分→专用部分→技术附表”；专用部分按原顺序安排总则/标准、
环境/硬件、技术支持、售后、预装调试、供货等位置，最终F未再报告technical_order。
股权表两段实质填写/截图证明说明在实际Word出现一次，保留原表，不只留在底稿。

独立实物审计确认363/363当前规则逐字段保留，24张原生表XML集合与v6一致，
903锚点/21合并/598空白未变；109行要求依据的规则文字、有序引文及企业空应答格
逐行对应。内部英文键/序列化JSON不再倾倒，中文细则如“最高分：20”可读；原文
真实代码字符串不清洗。主文与三卷、ZIP与CAS一致。65单元仍全为placeholder，
没有真实企业资料、填值、证明或最终报价，不能将结构修复当企业应答已完成。
独立结构审计另确认48条原未安排规则身份集合完全一致，不因改标题消失或被自动
判为已安排；四类财务表各两角色实例、10条用途说明及3个人工报价目标保留。

v2最终F的4条当前意见为偏差表待真实应答、平台上传可编辑压缩包、跨卷报价表
引用及报价承诺格式待核；意见不新增门禁，也不要求据此创建额外章节。当前确认
的是同一冻结106页分析上的编制语义及展示修复，不是从PDF重跑五节点，不代表
全部类型招标文件或完整布局已验收。知识检索与BUG-060版式边界仍待后续。
v2证据：`/opt/workspace/kb-tender-evidence/2026-10-27-title-and-readable-output-v2/`，
包括result/status/resume、独立readable-output审计及artifact-audit；最终Word与
ZIP在执行ID同名子目录。独立结构报告为independent-structure-audit-v2.md/json，
确认原规定层级、顺序及说明已进入实际Word。两个新版真实编制合计新增20次调用、
864746 Token。最终Ruff、diff及修改文档本地链接检查通过。

## 已知要求独立保留修复（2026-10-27，BUG-063）

用户明确要求无企业响应也不能丢失已知招标数据。已先更新字段字典及五项容量约束，
新增宿主只读AuthoringRequirements快照，写作前保存全部当前规则及target关联；
新结果以requirements_ref索引，ZIP附完整`招标要求底稿.json`。正文narrative位置
独立显示要求依据、范围/条件/例外、量化/评分/后果/提交细则及原引文，企业应答栏
保持空白，实际生成内容仍按旧单元单独渲染。form/attachment不盲目复制原文造表。
快照复制保留可读旧字段，不以二次合同校验删除内容；补遗仅用当前effective_rule。

专项失败反例先红后绿：无资料、组织/写作耗尽、应答整块被剔除、改稿耗尽、人工
报价、部分补遗、未安排和source-only均覆盖；来源背景不被冒充已确认要求。最终
42模块完整回归485 passed，154.46秒。随后补两个已处置身份反例，确保提交约束
及已明确排除候选不误报未安排，36项本轮专项全部通过，7.94秒；最终42模块完整
回归**487 passed / 0 skipped / 0 failed**，152.79秒，证据在v2目录。
七个实现及新测试文件Ruff、语法、diff和文档检查通过。所有测试使用既有隔离容器，
完整回归采用专用PG库/每例schema及空Redis DB15，不清理共享业务数据。

同106页冻结分析的独立v1`document-requirement-preservation-20261027-v1`已完成，
40.09秒，14份真实回包继承、新请求/Token均0，生产指纹未变；私有验收transport拒绝
任何未命中缓存的模型外发。同ID恢复0新请求/回执、同结果，0.108秒。此为本地编制
重放，不是新模型语义验收，也未从PDF重跑分析五节点。

独立实物审计确认363/363当前规则及字段完整，引用去重及目标关系一致；技术位置
原31条规则和71条有序引文全部进入Word。共13张要求依据表、122行空响应，旧24张
原模板表XML不变，总稿/分卷一致。底稿144条没有正文target，其中95条已有提交约束
安排、1条显式排除、48条确实未安排，不能将144都称漏响应。全部仍在底稿，不自动
塞入技术章或把未安排当适用结论。原64个单元均为placeholder，未增加企业正文/填值。

已处置诊断修复后的独立v2`document-requirement-preservation-20261027-v2`已完成，
40.68秒、14份回包继承、新请求/Token均0、代码指纹未变；同ID恢复0新请求/回执、
同结果，0.125秒。底稿requirements_ref与v1相同，DOCX解包后所有成员逐字节相同，
继承v1实物及PDF定向核对结果；保留v1及全部旧回执，不覆写旧成功结果。
v1实物审计见`independent-requirements-audit.md/json`，技术卷转为32页PDF，已实际
查看第18/22/23页，参数、联动条件和跨页原文清晰可见，应答栏为空；列宽和跨页表头
仍属BUG-060。此为数据保留/显示定向验收，不证明企业响应内容已齐备或全篇版式合格。
最终v2独立报告见`independent-v2-delta-audit.json`：三卷正文XML同样与v1一致，技术卷
共102行要求依据，其中技术规格位置31行；全部企业应答格为空，没有自动承诺。
证据：`/opt/workspace/kb-tender-evidence/2026-10-27-requirement-preservation/`及
`/opt/workspace/kb-tender-evidence/2026-10-27-requirement-preservation-v2/`。本轮没有
修改Prompt、模型/预算合同、.env或部署；BUG-062正式标题和BUG-060完整布局仍未修复。

## 新增内容完整性核查（2026-10-27，BUG-062/063）

用户指出正式标题夹带来源附件/表号及技术参数响应表缺失。已对照v6大纲、写作单元、
原PDF及冻结来源，确认chapter-2.5.1使用“8A 商务部分摘要表”，导出另加2.5.1章节号，
随后采用的模板又包含原表标识；编号来自模型接受的title，不是导出器额外生成。
原PDF物理91页本身就有外层格式标签和表内标题两行，当前成稿再加Heading3形成三处。
已核规定要求按第六章格式编写并编目录，但未要求正式章名必须前置附件号或表号；
原模板标签是否都应插入须由模型按部件边界选择，不能由代码按编号删除。
字段字典已明确任意层级章节均按业务组织，来源表号仅在原格式中保留，不按编号正则
删改，也不机械一表一章。本轮先核实根因及修订文档，尚未改Prompt或运行新模型调用。

技术卷实物4页，chapter-3.2.2主叙述目标已绑31条规则、16份来源引用，包含五个原生
参数grid；w49却为knowledge-only而全空，最终只显示待补。原PDF物理55页要求逐段
响应，物理57—61页的参数原表仍完整可读；参数附表C只引用技术规范书，G.1/G.2是
偏差表，不能替代全部参数应答。物理82页报价表也有要求/投标参数列，实际留空；
不能把报价卷存在的空表算成技术卷已逐项响应。根因是招标要求骨架和企业真实应答
混为一个可跳过任务，而不是解析器漏表或全部归因于未接知识库。

后续修复须保留来源要求/条件及可填写位置，企业参数、符合/偏差结论和证明按资料
匹配，没有资料留空。现有table块可表达底表，但混合来源和整块移除的限制尚须设计
核对，未宣称仅改Prompt即可保证逐行完整。旧v6及全部回包、原文件保留，未部署。
原文/目标/空置单元的只读证据见v6目录`technical-response-audit/review.md`及
`audit.json`。五个参数grid包含跨页续段，不能按单元格片段数冒充独立要求条数。

## 通用提交结构修复实施（2026-10-27，BUG-054/057/058/059/061）

按已批准后续方案实施。已对照原PDF、冻结原文、四份来源回包及A实际输入：两项
被误报缺失的格式实际有本次不适用限定，第二卷技术规范也已提供；限定在来源读取
时可见，却未被选作提交依据，来源适用性finding又未进入A。不能用固定附件名/页码
清单作生产判断，也不能把本批未读等同于原文件缺失。

源码已恢复A完整原生grid及locator标题路径，传入来源观察；增加模型明确排除候选
及理由的可选表达，未知/选排冲突沿原合同纠正，真正遗漏继续待整理。调用方可用
现有SourceUnitRef显式补充当前冻结提交原文，输入冻结spec后供A/F实际读取；不新增
主节点、主体体系、结构改稿循环或业务词表。明确排除候选的专属旧问题按原批次/目标
身份退役，原回包保留。BUG-059另在展示层解释解析标题装饰，不修改冻结原文或定位。
BUG-061在同章多个form复用相同模板时显示各自已有purpose，避免空白表单无法区分
填写用途；不推断主体、不改原表，不新增角色字段或模型调用。

定向回归包含真实PG入口、来源去重/冻结/恢复、完整网格、排除与实际DOCX合同。
首轮39模块完整相关回归447 passed/0 skipped/0 failed，149.64秒；随后补精确退役
反例及来源阶段标注后448项通过；再补实际多实例用途展示，最终39模块完整回归
**451 passed / 0 skipped / 0 failed**，149.00秒。使用专用PG测试库及空Redis DB15，
模型为确定性替身；七个实现/测试文件Ruff、语法、diff及文档链接检查通过。未以这些
结果代替真实语义验收。最终回归证据在v6目录。

独立v5已完成，保留v1—v4全部回执及文件。离线重放确认原四份来源摘要完全未变；
补充的82份原生引用只作本次复测输入，实际文件特定选择保存在仓库外，生产代码中
没有附件名、页码或范围ID。A完整请求估算85,041/94,208输入Token，含19完整网格及
37条来源观察及82份补充原文引用，输出预留32,768，实际发送并成功。同gpt-6.1-terra、
131072窗口、并发2、无知识/企业资料，运行代码指纹未变。

v5执行`document-submission-followup-20261027-v5`耗时117.30秒，继承四份来源回包，
新增1A+6写作+2正文复核+1F=10次调用，434,009 Token，0重试/失败/未知用量；各请求
输入估算比供应商报告保守多502。形成34章、65位置、64个placeholder单元，新增正文、
单元格填值及真实附件均为0，不能算已完成实质企业应答。备件候选明确排除，原件中
本次不适用的格式不再误报缺失；多角色财务表形成独立实例。F的5条当前待核意见包括
条件性签署/授权、参数附表对应、跨卷价格引用、平台上传和报价承诺格式，仍须核实；
52条来源阶段观察和797条累计缺口另计，不能当成797个独立当前缺陷。v5同ID恢复
0新请求、0新回执、同结果，0.162秒。

BUG-061修复后独立v6执行`document-submission-followup-20261027-v6`耗时44.57秒，
14份模型回包全部继承，**新增模型请求0、计入新调用Token为0**，运行代码指纹未变；
同ID恢复0新请求、0新回执、同结果，0.146秒。上述两个耗时均为已有冻结分析上的
编制/重新导出，不是从PDF开始重跑五节点。保留全部旧文件及回执，未修改.env或部署。
独立实物审计确认增加10条既有用途说明，各在对应实例前出现一次；除此之外段落
序列及全部原表内容与v5一致，24表、903锚点、21合并、598空白保持，总稿/三卷/ZIP
一致，字面Markdown标题残留为0。大纲、依据、64个单元及复核引用和797条累计缺口
均与v5严格相同；14份继承回执的输入摘要及产物逐条对应，未补造付费请求。
证据分别位于`/opt/workspace/kb-tender-evidence/2026-10-27-submission-followup-v5/`及
`/opt/workspace/kb-tender-evidence/2026-10-27-submission-followup-v6/`。
v6独立报告为`independent-replay-audit.md/json`、`artifact-audit.json`和
`native-grid-and-instance-audit.json`；最终文件保存在该目录下的执行ID同名子目录。
v6总稿及三卷经已有OnlyOffice转换为26/19/4/4页PDF，代表页截图实际核对各角色
用途说明位于对应表前，条件、空白及页脚正常；见`visual-audit-v6/review.md`。
此为BUG-061定向显示验收，不代替BUG-060全篇版式验收，转换新增模型调用0。

无模型重导出v4并经已有OnlyOffice转换为22页PDF：20处Markdown标题装饰被正确
解释，原19张表格文字、段落数及旧文件不变；视觉定向检查通过，未宣称所有版式
验收完成。真实知识库当前无可用真实资料：默认资料库为空，其他集合为模拟验收资产；
不能用它们冒充知识匹配验收。部署API认证探针401，搜索实际合同尚未核实，不因健康
探针正常宣称可检索。未改.env、网络配置或部署。

通用性约束已在修复方案明确：不预设固定三卷、角色或附件清单，案例特定选择只在
仓库外测试输入，生产代码按当前原文及显式引用工作。单文件结果不代表多类型招标已
全面验收。BUG-060的目录页码、封面顺序和跨页表头仍待通用布局合同，未宣称已修复。

## 提交结构与表单归属修复实施（2026-10-27，BUG-054）

用户要求完整方案，已新增[修复方案](submission-structure-plan.md)，同步字段字典的
唯一后续合同、实施入口和15项未勾选验收。方案覆盖正式目录、依据与模板分离、精确
部件/填写实例、整表人工报价、实际采用内容复核、紧凑结构核对及同树组卷；不新增
分析节点、Agent、审批门禁或多层路由。旧17回包缺少新模板语义，升级不能免费补认。
用户随后批准直接实施。已先复现模板/目录错误，再接通精确模板装载、一次全局归并、
最终结构核对和同树组卷。修复新增冻结根不可读出口及图片模板OCR语义投影，原图不重复
打印OCR；关联条款原文进入写作可选支持，最终引用仍须模型选择。相关32模块完整回归
**378 passed / 0 skipped / 0 failed**，141.81秒，使用专用PG测试库与独立Redis DB15；
模型为确定性替身。源码Ruff、语法及diff检查通过。

真实同文件新执行已启动：`document-submission-structure-20261027-v1`，专用schema
`submission_structure_20261027`，gpt-6.1-terra/131072、并发2、不提供knowledge/materials。
复用106页原件的冻结分析快照，不重跑五节点；不继承旧17份来源模型回包。新离线规划
21来源包全部准入，累计输入保守字节估算1,783,298（非付费Token），最大94,175/94,208，
输出预留32,768（非API硬限）。A/F各最多一次，真实完整请求与结构效果仍待观察。
自动审批首次要求明确具体文件及模型目的地，用户随后明确授权当前目的地付费复测；
被拒绝的操作没有发送请求。deploy/.env及全部旧产物保留，未部署、提交或推送。
本次证据：`/opt/workspace/kb-tender-evidence/2026-10-27-submission-structure/`。

### 当前验证结果：v4已导出草稿，正式语义验收仍有待办

最终相关36模块完整回归**426 passed / 0 skipped / 0 failed**，145秒。v4已成功导出
总稿及商务/技术/报价三卷：30章、58响应位置，ZIP与总稿/卷子树机械一致；实际模板
检查未发现上下文整段串入或精确部件错位，价格和未知企业资料留空。

实际编制逻辑为4来源+1A+6写作+2正文复核+1F=14调用。来源/A的5个付费调用在v3已
成功，v4继承这5份回执，仅新增9次、0重试；新增335,212 Token，无未知用量。v4实耗
72.43秒是**恢复后编制阶段**，不等于从PDF开始或14次调用全新的端到端耗时。
同v4执行再次读取结果实测0新请求、0新回执、同结果引用，0.148秒，未覆盖原耗时证据。
运行代码指纹未变；本次没有改.env、部署、提交或推送。

无知识/企业资料场景下58位置最终均为placeholder，0新增正文/填值/真实附件；原模板
仍按要求保留，这不等于完成有资料的投标响应或知识库匹配验收。F确已实际发送，指出
目录遗漏/附件3与报价承诺函、8D多主体实例、技术顺序及上传安排待核；G.2未填偏差
也不能认作符合。问题保留且不阻断下载，**正式目录和内容语义尚未全部通过**，不能将
三卷存在或succeeded当作可直接投标。实物及独立审阅报告在v4证据目录。

以下保留本轮失败、定位、恢复时间线；动态事实以上述当前结果为准。

上述v1真实执行已结束，521.22秒（8分41秒），39次发送全部成功、0重试；输入761,555、
输出39,743，合计801,298 Token，未知用量0。21来源、13次写作、5次正文审阅/改后审阅；
A实际输入字节估算229,025超过94,208未发送，F也因预算未完成。仅导出“待整理内容”
一章、173位置、0正式分卷，故**正式结构未通过**，不能将succeeded当作本次修复完成。
精确模板/价格留空等机械检查通过，正文仅复述条件，缺企业资料时不能算有效企业应答。

用户随后追问4包与21包的差异，已记录BUG-055：4为原分析抽取阶段，21为编制重新读源，
同时存在标题路径重复及保守字节计量扩大分包的问题。39份供应商用量与o200k计量逐条
核对，保留512固定开销后每份都高估502，无低估；A真实保守token计量62,985，可在
现有窗口和32,768输出预留下准入。已共享heading_path并显式启用已校准编制计量，
默认/分析合同不变。源视图7模块102项与预算2模块50项定向回归通过，完整相关回归在跑。
新版离线计量来源回到4包，全部准入：94,176/94,006/93,711/83,368 Token，累计365,261；
请求wire由1,900,628降至1,409,164字节，原文字符串、完整父路径及模板锚点恢复测试通过。
这些是离线计量，不等于新4包已经实际完成或输出已验收。

独立新执行`document-submission-structure-20261027-v2`已启动，schema
`submission_structure_20261027_v2`；同模型、同目的地、无知识场景，启动前核对目的地/
模型与39回执校准对象完全相同，配置变化即停止，不按别名猜tokenizer。
新视图改变来源请求及提示，需重新发送4包，不将旧21回包补认成新语义；v1证据完整保留。
本次新证据：`/opt/workspace/kb-tender-evidence/2026-10-27-submission-structure-v2/`。

v2现场未按离线4包执行：execute_document重建预算漏传新开关，实际仍按字节计量。
已精确取消该独立执行、防止继续错误付费；全部成功回执和在途状态保留，不复用该ID。
413项相关回归全部通过仍未覆盖这处入口接线，正在补执行器级反例后精准修复。
不能将v2说成实际4包已通过；下一次使用新ID并在首个实际请求核对method与冻结spec。

v2精确取消后共10发送：9成功、1在途取消记uncertain，已报告220,610 Token，1次用量
未知，不计为免费。接线反例2项先失败，补传tokenized_authoring后入口/恢复/预算四模块
88项通过；持久spec与调用方预算全等，真实测试请求method已核对。Ruff与diff检查通过。
另BUG-055完整相关回归34模块413项通过（入口新反例另计，不重复相加）。
独立v3离线再次确认4来源包全部准入并已启动；首个实际请求须核对计量方法和冻结spec，
不将预审替代真实执行。证据位于`/opt/workspace/kb-tender-evidence/2026-10-27-submission-structure-v3/`。

v3实际验证4来源均成功，累计输入363,253、输出16,349 Token；估算每份仍保守多502。
第五请求确为A，估算50,604准入并成功，实际形成商务/技术/报价三卷，价格表安排正确。
随后宿主ConnectionTimeout，尚无DOCX。首次288.23秒、同ID恢复92.12秒均保留5份成功
回执，没有新增源/A发送。恢复记录循环阻塞35.457/13.476/40.841秒，确认同步准备及
反复全源重建问题，记BUG-056；不将其当作模型或外部网络故障。

BUG-056已复用执行内EvidenceRepository并保持原生对象深拷贝，源/A/写作/改稿准备移到
线程；来源及请求语义不变。缓存专项160项不同测试、线程及改稿相关141项、F/导出25项
均通过（分组有重叠，不相加当总数）；最终全组回归在跑。改稿准入同步补authoring_write。
独立v4已启动，继承v3的5份成功模型回执，源码指纹重新固定；不重做付费来源/A。
证据：`/opt/workspace/kb-tender-evidence/2026-10-27-submission-structure-v4/`。

## 知识匹配修复（2026-10-27，BUG-047至BUG-052）

源码已切换[knowledge_required_v2](field-dictionary.md#编制最小修订knowledge_required_v2)，
分析五节点未改；实施边界与真实待验收项见[实施计划](implementation.md)和
[验收清单](acceptance.md)。未部署、未提交或推送，deploy/.env未修改；下述无知识真实验证已启动。
既有v1及历史真实文件仍保留，不能作为本次策略的真实验收证据。

在现有大纲调用内产生业务检索意图，宿主去重、检索并绑定来源；写作/复核通过一份
supports表选择依据。背景可由招标原文支持，企业应答须有适用知识/明确企业资料，
未取得资料及最终报价留空，继续生成文件。人工/无支持空位免正文复核，但有候选
却被模型完整判空的结果仍须复核，允许一次有依据补写。首次复核不可用保留未被否定
草稿；初审明确否定后，改稿失败或再核不可用回退到已剔除问题内容的安全稿，不能
恢复已否定内容。附件须有可判断证据，复核可精确移除单份附件。

共同依据每批仅处理当前支持，宿主合并已绑定安排，不逐批重读旧材料；可读OCR正文
只投影一次，空OCR状态不算支持。跨任务旧问题只有在某次成功复核完整看到全部关联
任务时才替换，部分上下文/失败包继续保留问题。显式重审使用新的execution继承前驱，
review_request的真实范围和理由冻结进spec及相关复核/必要修订请求，不改变初次写作，
不清缓存、不加盐或覆盖旧稿。知识回执按单query及提供器范围冻结并重绑当前目标；
无法证明知识同版时不跨执行复用检索。

必要失败用例先复现后修复，最终相关回归321项全部通过、无跳过，131.90秒。使用
已有隔离测试容器，专用PostgreSQL库kb_authoring_v2_test_20261027及每例独立schema，
Redis独立空DB15，不清理共享业务数据。模型均为确定性测试替身；覆盖短引用、大纲
两入口、来源背景/source-only、语义空稿补写、混合原表及报价空白、a1局部删除、
初审/改稿/再核故障回退、跨任务问题、实际DOCX/ZIP、同ID恢复和新执行重审。
首次组合回归242通过/25跳过是测试连接变量未注入；补齐TENDER_TEST_DSN后已全部
实际执行，不将跳过计为通过。最终证据为仓库外
`/opt/workspace/kb-tender-evidence/2026-10-27-authoring-v2/final.xml`；各模块红绿记录
同目录outline-*、knowledge/、revision/及root/保留。
相关源码与测试Ruff通过，7个实现模块语法检查及git diff --check通过。

新检索意图会改变原文大纲请求，旧17份来源回包不能直接复用为新合同；真实调用前
须重新计量实际调用数、完整输入及输出容量。输出预留不是API硬限，
中文关键词召回限制仍待真实知识库纵切验证，不将短查询修正等同于召回已合格。
当前入口仍由调用方传入knowledge提供器及检索集合。用户随后明确先测无可用知识资料
场景：同一106页原件，不传knowledge/materials，使用当前.env模型gpt-6.1-terra与
131072窗口，新执行document-no-knowledge-20261027-v2，不继承旧17份模型回包。
复用既有冻结分析快照，未重新运行五节点分析。离线规划17个原文大纲包全部准入，
累计输入估算1,418,773，wire为1,497,399字节；单包输入估算最大94,115/94,208，
输出预留32,768，非供应商硬上限；未沿用旧模型tokenizer。真实执行已完成，并发2，
知识检索和共同知识依据调用为0。
不把无知识留空场景当作真实知识库召回验收。证据目录为仓库外
`/opt/workspace/kb-tender-evidence/2026-10-27-authoring-no-knowledge/`。

本次17个来源包已全部返回，796个范围各发送一次，没有来源读取重复。归并请求实际
输入估算145,661，大于94,208准入（总上下文182,525/131,072），未付费发送；
触发outline_assembly_unavailable后保留原批次91个章节节点、63个顶级章节、196个
响应位置。这是已知BUG-040的容量出口，存在跨批重复风险，不能称已完成大纲归并。

执行于2026-10-27 03:58:02至04:09:18 PDT完成，状态succeeded，实耗676.29秒
（11分16秒）。80次新增调用全部成功：17次原文大纲、37次首写、13次首核、11次
定向修订、2次再核；分阶段已按实际payload字段核对。重试、失败、不确定及未知用量
均为0；输入1,216,306、
输出50,786，合计1,267,092 Token。按字节累计输入估算4,012,901不等于付费Token。
同ID恢复实测0新增请求、0新增回执、同一结果引用，原真实耗时记录未改写。

实物检查：181个写作单元中164个占位、17个generated；实际36个非空块、0格填值、
0真实附件，无知识materials/shared_arrangements。80份实际请求全部可读，无知识来源
supports、纯知识任务写作/复核、纯人工任务写作/复核或无原件附件任务写作/复核。
DOCX及ZIP可打开且CRC、成员CAS一致；35个原生表实例的638个固定锚点、117个合并
锚点及489个空白锚点均与原网格一致，未填写原空白格。
实际Word投标价格表的单价、总价、产品参数和品牌型号格均空，原数量保留；投标函
价格引用未填金额，开标价格表的大写/金额/数量填写线及签章主体仍空。未以原预算、
限价或历史参考价代填投标价格，具体Word坐标见semantic-review.json。

内容质量仍未完成验收。36块初审未发现新增产品能力、型号、人员、业绩或价格。
BUG-053的初审定性已纠正：w25的指定有效期及起算点确有第11页前附表原文，四阶段
均通过related_rules/relations收到，不能称为凭空生成或必然错误删除例外。确认问题
是最终块只绑定第22页通用条款，未绑定支持指定值的前附表；承诺授权是否需额外资料
另审。最初只核block.source_refs遗漏完整关联上下文，原审计结论已保留并修正。
来源可读、指定数值有原文与最终引用链完整须分别核对。固定模板中的第一人称声明仍是名称、签章
未填的规定格式，应保留，不能将这些原条款本身当作模型伪造企业事实；但原模板与
另一份重写/留空版同章重复，仍属BUG-040的模板/成文角色问题。

3个局部“模板缺失”位置在后批均已有真实原表：2张偏差表和1张获奖表。归并未执行
使早期待定位目标、后批原表目标及部分局部警告同时保留；不能误报为PDF缺表。
363条规则中325条安排了目标或约束，38条保留未安排诊断，数量不等于独立业务漏项。
实际form引用146次、不同来源115个，16组来源重复、额外31次引用；不能将这些全称为
重复表格，语义实例仍待整理。最终共1377条缺口，是累积诊断数量，非1377个独立问题。
ZIP待补清单与document.gaps经同一AuthoringGap默认值规范化后完全相等；最初检查器
将可选null字段省略误判为清单不一致，已修正检查器，生产数据没有丢失。

结果位于上述证据目录的document-no-knowledge-20261027-v2/：投标文件.docx、
投标文件及附件.zip、document.json和gaps.json。原运行与全部回执保留，未静默修改
不合格内容。结构检查为acceptance.json，来源/归并审计为source-audit.json，恢复检查
为resume-check.json；完整内容审阅为semantic-review.json，问题四阶段追溯为
bug053-trace.json。结构及零知识库合同通过，内容审阅为reviewed_with_findings，
不能据此宣称真实知识库召回、五节点分析或正式投标文件内容质量已经验收。

用户随后指出目录/附件表不合常理，新增BUG-054。真实追溯：“附件1 投标函及相关声明”
首次出现在第9来源包，最终为chapter-16一级标题；原文确有附件1格式清单，但该概括
标题不是强制目录名称。其唯一form目标引用了形式评审依据，实际导出为评审表及占位，
没有绑定真实投标函等原格式。只删“附件”字样无法修复。

最终提交结构亦未验收：原文3.1.1/3.1.2/3.1.3分别规定商务、技术、报价组成；最终
技术分卷只含早批chapter-4标题（5段、0表、无响应目标），商务分卷只含chapter-3及
3.1早批内容（11段、1表），报价分卷仅早批chapter-5内容，真正表2A在chapter-50、
开标价格表在chapter-62，没有进入报价分卷。需统一原文规定结构、表单实例与章节
归属。此前“结构检查通过”仅指文件/引用/网格完整性，不代表目录和交付结构通过。
标题追溯见attachment1-title-trace.json；分卷及目录复核见私有内容审阅记录。

## 知识匹配应答修正（2026-10-27，BUG-046，已实现基线）

以下保留BUG-046的v1实现及验证历史；随后发现的BUG-047至BUG-052已按上节修复。
下述224项属于v1隔离回归，不代表当时已修复后续问题，也不代表真实知识库匹配验收。

本次用户口径替代此前“无知识仍可生成拟议方法”的设计：新增投标应答必须匹配可用知识
资料，未匹配留空，技术/实施/服务方法同样适用；招标背景、要求、固定模板照常保留，
报价最终值继续人工填写，缺资料不阻断文件生成和导出。

文档已先统一knowledge_required_v1：一次write完成语义匹配和改写，各block/fill绑定
target_ids/material_ids；无候选目标跳过basis/write；共同依据不再自由生成全源方案；
复核content_ids定位无依据内容并移除，定位不精确则清空明确关联任务应答，复核不可用
也暂不采用该批生成应答。保留原稿/回执及模板/空位，策略进入真实请求和执行spec。

代码已按合同实现。额外边界包括：纯原件无可读文字不能支撑新增正文，仍可作为附件；
填值必须属于其目标原表；共同依据携候选关联实际要求；无依据发现按任务规范化，
分批再核可以替换旧意见，不再按旧块编号误删已修正稿。缺资料不触发额外匹配轮。

隔离容器完整编制回归224项通过（117.71秒），包括25项知识应答回归、实际PG持久化、
租约、缓存继承、改稿及DOCX/附件导出；模型为确定性测试替身。Ruff与diff检查通过。
证据位于仓库外`2026-10-26-authoring-implementation/bug046-final-regression.xml`；
首次失败及边界红绿记录保留在同目录`bug046-grounding/`及`bug046-*-red.xml`。

部署和真实知识库文件验证待完成。当前Python编制入口仍由调用方传入knowledge提供器
及检索集合范围；没有提供器或可用资料时输出空位，不静默使用模型常识补写。
本次没有付费调用、没有改写deploy/.env及旧v2文件；下方v2记录保留历史实际行为，不能用其
执行成功证明新知识匹配策略已验收。旧v2未连接真实知识服务、没有真实企业材料，既有
拟议方法不能直接作为本策略合格应答。

## 当前工作区状态（2026-10-26）

### 完整编制链与本轮真实验证（T3–T5）

**本次恢复执行已结束，状态succeeded，但产物仍是有明确缺口的待补草稿。**
`document-real-20261026-v2`使用原gpt-6.1-sol配置，于2026-10-26 22:18:47至23:32:10
PDT完成，DB记录总耗时4,402.76秒（73分23秒）。消费既有未确认分析快照，本次没有
重新执行五节点分析，也未验证真实知识库或企业材料匹配。

| 阶段 | 新增调用 | 实际阶段耗时 |
| --- | ---: | ---: |
| 大纲归并（17份来源回包复用） | 1 | 3分02秒 |
| 共同编制依据 | 30 | 24分07秒 |
| 首轮写作 | 61 | 21分58秒 |
| 首次正文复核 | 58 | 20分41秒 |
| 定向改稿 | 5 | 1分35秒 |
| 改稿后再核 | 4 | 1分24秒 |

共159次新增调用全部成功，失败、不确定、重试、合同拒收及未知用量均0；上述耗时是
该阶段首发至最后完成的跨度，含阶段间组织及最终导出的总耗时以DB记录为准。
末次模型完成至执行结束13.17秒。本次输入3,000,118、输出230,803，合计3,230,921
Token；历史17次441,769 Token另计，合计3,672,690，不将旧回包复用再次记为付费。

最终产物位于仓库外`2026-10-26-authoring-implementation/document-real-20261026-v2/`：
`投标文件.docx`（163,714字节）、`投标文件及附件.zip`（549,487字节）、`document.json`
及`gaps.json`。ZIP含主稿、商务/技术/报价三分卷、清单及待补事项；CRC及各文件哈希
通过，四份DOCX均可打开。主稿70标题、675段落、101表格。没有企业证明原件，包内
无真实企业附件。汇总为`real-v2-acceptance-summary.json`，版式及语义完整性另见下文。

独立入口为`tender.document_execution.execute_document`，内部连接大纲分批组织及目标级
语义归并、可选知识资料快照、共同编制依据、完整响应单元写作、实际正文复核、一次可修
问题定向改稿及受影响内容再核、主DOCX/明确分册/原件ZIP导出。未新增分析节点、CLI、
Rust、API或页面入口。宿主管引用与执行，模型负责语义；知识片段与实际原件分离，
给定事实与拟议安排分离，旧稿和初次复核均留CAS。原报价、单价、总价、折扣等最终值
由manual_fill位置保留空白，人工最后填写，不调用模型代填或用0补空；技术内容继续。

`previous_execution_id`只复用同一分析运行中完整请求摘要相同的已接受模型JSON，
经本次validator重新绑定宿主引用；知识搜索同执行固定回执。未实现独立通用依赖图或
Word人工修改自动合并。每请求初次加三次恢复，重复同摘要并发在进程内等待并复用；
不同摘要可并发。数据库/CAS故障、取消及失租保留真实错误，局部模型耗尽保留位置继续导出。

真实验证已获用户“运行发送”明确授权并通过自动审批，独立schema
`authoring_real_20261026`内的`document-real-20261026-v1`消费历史捕获的106页未确认快照
（363规则、796范围、42表格），不把旧快照说成当前业务库已确认。原CAS不回写，新产物
位于私有`2026-10-26-authoring-implementation/real-artifacts`。实际使用gpt-6.1-sol、窗口
131072、输出预留32768、并发上限2；知识服务未配置，不能宣称实际知识库召回验收通过。

v1的17个来源请求全部成功，真实报告输入388,546、输出53,223、合计441,769 Token；
没有模型重试或未知用量。后续宿主组装失败，不是整份真实编制完成：归并被UTF8字节
计量误拒，随后多片段大纲的model_dump注入EffectiveItemRef可选null，严格读取拒绝。
全部17份回包保留，v1自然failed且没有在途调用，未清缓存或篡改发送状态。

修复仅两处持久引用输出省略未提供字段；回归覆盖原项及retained index 0/1多次CAS往返。
归并规则改为完整语义共享表及短引用，并使用独立outline_assemble.md，保持原source
prompt和分包不变。15个已完成请求离线比较均为provider prompt=o200k正文+10；仅对
归并显式开启已校准o200k，保留512 framing、32768输出预留及4096安全余量。默认分析
及其他编制请求不变，不新增多层归并。共享输入的准确准入与真实输出仍分别验证。

恢复过程中发现`.env`改为gpt-6.1-terra。用户最新明确选择“可以用原来的配置继续验证，
先跑完一遍流程”；该文件未改写，继续冻结sol配置。新执行`document-real-20261026-v2`
以previous_execution_id指向v1，已复用17/17旧来源回包，没有重新支付原文读取。
归并首次成功：实测49,119输入及12,981输出Token，164章节/233候选位置归并为
70章节/118响应位置，原候选全部安排，保留7个人工填写位置。结构归并成功不代表
363条规则全部语义覆盖；仍有38条未分配规则、17个无规则绑定目标及2条来源读取问题待核对。
共同依据30/30批、全部准入写作及复核请求均已结束，5份改稿的旧版本保留在CAS。

实际DOCX核对确认：source_refs混合上下文与模板，44次原表渲染中包含评审表9次、
投标须知表4次，同一报价表在一个章节输出3次；两类合同及渲染缺陷均已实际发生，
详见BUG-040补充。44次渲染的919个非空固定锚点及139个合并锚点保持，源文字段段落
未丢失，但这不能抵消非交付表被带入或重复输出的问题。报价表三份副本的单价/总价
6格均空白，人工目标涉及的8个原空白锚点均未填入。只有1条export_cell_fill_unplaced：
叙述任务重复提交同章表单单元格填值，因不属于该任务form位置而未落位；相应form
任务已有同格填值，封面及报价未因此丢失。详细证据为`real-v2-final-export-review.json`
及早期`real-v2-template-export-review.json`。未进行分页视觉或逐项语义完整性验收，
未热改在途代码；真实流程结束后仍需修复模板角色/实例合同及预算误拒。

最终共同依据下只读重放规划得到113个写作单元：61个可准入模型请求、13个单目标
预算占位、32个缺本任务证明原件占位、7个人工填写占位。13个完整请求的UTF8估算为
96,970–218,100，高于94,208准入；o200k正文加512 framing仅31,197–71,789，全部可保留
32,768输出及4,096余量。明确为写作仍走byte计量的宿主误拒，不是模型调用失败；
其中关联规则/问题/关系合计占载荷字段体积约74%，压缩问题另计。已保存
`real-v2-writing-admission-plan.json`及`real-v2-writing-budget-diagnosis.json`，
本轮保持冻结执行配置完成，不以占位导出替代完整正文验收。

最终61个准入首轮写作请求全部成功，无重试；实际单元
为56 generated、57 placeholder，后者包括52个宿主预定占位及5个已接受的留空回包：
4项财务表缺真实财务数据，1项可选配置清单缺实际配置/分项报价事实，均error_type=null，
不能记为模型执行失败。初审实际79批：58批准入覆盖92单元，21个单单元
请求预算拒发。58次初审及4次再核均已完成，最终保留151条finding；不能说113单元
均完成独立内容复核。汇总gaps为1,665条，含继承的问题及多位置提示，不代表1,665个
独立故障；证据为`real-v2-acceptance-summary.json`及
`real-v2-review-admission-plan.json`。
这21批另行精确对照：UTF8估算95,493–217,067，o200k正文加512仅30,067–71,525；
保留输出及安全余量后66,931–108,389，全部低于131,072，亦属计量误拒。相关规则/
问题/关系累计1,957,531字节，原文376,847字节，正文content仅24,437字节；不能把
输入膨胀误归因于生成正文太长。本轮配置保持冻结，未热改预算或清除已接受回执。

隔离PG+合成SSE完整纵切已经验证：未确认分析、原表填值、真实PNG装配、某章4次失败
仍输出真实DOCX/ZIP、同ID零发送，以及新执行精确复用后仅重写失败章及复核。
`test_authoring_revision_integration`验证只读A/B不能消除仍涉及B/C的旧问题；实际Word
保留A新稿、C原稿、B占位及旧稿CAS。真实v1修复前的全部unit及本轮受影响持久验收同轮
**1241 passed（279.73秒）**，覆盖当时的输入、组织、报价留空、改稿、导出、独立执行
与共享恢复。此后引用序列化及归并计量修改使用定向回归，不能将1241项视为修改后的
完整复跑，也不累加此前分轮结果。
后续定向报告分别为引用往返152项、归并计量34项、执行及任务穿透37项、最新编制
边界54项通过；各轮有重叠，不累计为一次全量测试结果。实时任务中未再修改源码。
静态检查通过，122个本地文档链接路径均存在。

全部证据放在`/opt/workspace/kb-tender-evidence/2026-10-26-authoring-implementation/`；
v1日志为`real-document-live.log`，当前v2日志为`real-v2-document-live.log`，
只读监控为`real-v2-monitor-latest.json`，全量回归报告为`final-regression.xml`。
未部署共享服务，未提交或推送。

### 真实投标成品完整内容审阅（2026-10-27）

本轮按用户要求阅读主稿及商务/技术/报价三分册全部70标题对应的正文/表格和113个
最终写作单元，逐项核对大纲、共同依据、实际初审/改稿及相关原招标条款。技术部分
27标题、37单元单独逐章列出，商务及报价全部生成单元及实际章节亦已阅读；封面和
附件位置另核对。没有付费模型调用、生产代码修复或原产物改写，不以此声明分页视觉
验收、扫描原件逐像素核查或真实评标通过。

内容结论：当前稿件存在实质质量问题，仍是需要修正的编制中间稿，不能根据执行完成
和56个generated宣称完整实质响应。实际存在以下已确认问题：

- 专用附表明示本次不适用的商业运行备件，仍被写作和报价位置重新设为本项目义务；
  不能归因于缺企业材料。大纲、共同依据及任务原文遗漏了项目适用边界。
- 原件要求按原结构逐段应答，当前只有主题方案及待定稿说明，逐段主任务预算缺失；
  质量安全风险变更专章也未生成。售后母任务虽空，两个子章已有具体内容，不能把
  整个售后评分项误说成完全未写。
- 商务及技术多处把编制、取证、核验及评分教学当正文，内外职责边界不清；有的
  generated财务单元只有待补填值，非空结构指标不代表财务内容完成。
- 承诺模板和模型重写全文同时输出，出现双份签章/承诺；原生section跨附件，使
  相邻保函进入另一章。已发现的表格重复只是这一合同缺陷的一部分。
- 完整原件已有的资格条款未送到当前任务，正文却称资料缺失；部分真实业绩门槛
  又被局部复核误判为无来源。全局原文确实缺失与本任务未发送必须分清。
- 复核只见模型内容，没见宿主实际组装结果；既漏报成品重复，也以模型未画原表
  误报缺表，引发对宿主本会输出表单的改稿。初审148条含127条模型finding及21条
  未复核诊断，不能混作148条已完成的语义发现。
- 成品有19处局部来源短号（6个内容块），72段以解析标题标记开头；内部核对任务
  也成为公开章节。功能说明、预装流程重复，同一开发方案两套时间安排未明确衔接。

可保留的内容包括现场安装/培训、三年质保及硬件12小时到场、每批30天预装、到货
与预装节点区分、人员培训/资格审查/考试及监督整改等具体方法。未知价格、财务、
型号、资质及业绩事实继续留空合理，不新增资料确认门禁，也不要求模型虚构满足结论。

问题根因已追加BUG-041至BUG-045，并补充BUG-040模板缺陷。当前仅审阅和记录，
尚未实施所列修复。完整私有报告为`real-v2-content-review.md`（含70标题清单、评分
对照及修复边界）；详细技术报告为`real-v2-technical-content-review.md`，商务证据为
`real-v2-business-content-review.json`，均在本轮仓库外证据目录。原件对照文字为
`real-v2-review-original-layout.txt`；报告附成品SHA256，原文件保持不变。

### 可用编制输入与独立执行适配（BUG-040，T1/T2）

输入合同已改为固定实际可读根及成功抽取子包；缺确认、未完成分析、业务问题、不可读
资料均带具体诊断继续提供可用快照，不伪造analysis-ready或确认。核验投影可用时沿用
current_items的既有接受决议；否则保留未核验抽取，只有原文时装载来源。
发送视图rule_origin明确区分verified_projection/extraction/source_only；原始提取不被
标记成有效补遗投影，后续资料和来源问题进入大纲问题，不静默丢失。

execute_document与AuthoringStore优先恢复固定输入；后续分析修订不废弃旧版本成果，
结果仍绑定原快照。心跳与发布保留显式取消、独立epoch/租约、存储围栏，移除重复确认
门禁。Store支持outline、authoring_basis、authoring_write、authoring_review，sent与缓存
按task+logical_request_digest隔离，每个完整逻辑单元共用首次加三次发送；不同章节不再
争抢同一逻辑请求身份或相互消耗额度。稳定ID不热换快照，不增加CLI或Rust入口。

该阶段同轮编制相关回归 **174 passed（103.74秒）**：包含全部unit/test_authoring*.py、
unit/test_outline*.py、独立AuthoringStore及既有输入/大纲/执行三组acceptance，使用真实
隔离PG/Redis和合成模型/SSE。覆盖缺确认/部分抽取/零资料、损坏引用、保留补遗、分析
修订期间完成、同ID缓存恢复、取消/租约、多逻辑单元独立重试，以及8项正文链边界回归。
正文边界包括空输出占位、跨位置资料选择、非事项项目背景和复核载荷不含持久CAS身份。
新增文档持久执行入口由其独立回归报告覆盖，不包含在此174项结果内。

报告位于`/opt/workspace/kb-tender-evidence/2026-10-26-authoring-implementation/`的
`snapshot-final-regression.xml`；此前136项、84项和8项分轮报告仍保留，不与最终结果
相加。原分析workflow无改动，STAGES与默认Task及模型提示快照保持既有范围。
上述合成回归没有使用付费模型，不等于真实投标内容的语义验收。共享服务未部署；
本轮真实编制运行进度以上方完整编制记录为准。

上述174项之后，核验问题/关系投影复核新增2个边界回归，先复现**1 failed、9 passed
（4.07秒）**：只有source_refs的背景任务遗漏原文问题及人工意见。补齐来源范围路由及
共同依据中全局问题的人工意见后，整个边界文件**10 passed（4.17秒）**，Ruff通过。
同时验证无关长问题不进入其他局部包；报告为`analysis-context-boundaries.xml`与
`analysis-context-boundaries-fixed.xml`。这是后续定向回归，不与174项相加，也不表示
在新增投影后再次执行了全部174项。

按新增“投标报价留空、最终人工填写”要求，补manual_fill纵切回归：边界文件
**11 passed（3.47秒）**，Ruff通过。测试实际读取生成DOCX，原报价表头和200000元
招标限价保留，总价/单价格仍为空字符串，没有采用历史123456元/888元或填0；其他
技术正文存在，write只调用技术任务一次。复核回包即使要求自动改报价，也不会让人工
占位进入改稿。报告为`manual-quote-boundaries.xml`，使用合成来源及mock语义，不是
真实报价或真实模型验收；不与此前各轮结果累计。

### 多批大纲引用序列化修复（BUG-040）

针对真实v1暴露的EffectiveItemRef显式null问题，先新增实际预算分包、归并后同时包含
原项与两条保留规则的往返回归，**1 failed（2.85秒）**准确复现。仅在多批大纲返回与
WritingTask序列化两处使用exclude_unset=True；未放宽分析schema，也未修改Prompt或
Token估算设置。修复后同例**1 passed（2.89秒）**，保留规则索引0/1、明确允许null的
AuthoringInput字段，以及正文装载按引用查找规则均验证正确；outline和task分别经过
两轮CAS保存/重读/校验。随后全部authoring/outline单元同轮**152 passed（17.57秒）**，
Ruff与diff检查通过。报告为`effective-ref-roundtrip-before.xml`、
`effective-ref-roundtrip-after.xml`和`effective-ref-unit-regression.xml`。
该轮未重跑完整持久验收或真实编制；实际v1失败及后续恢复状态以顶部实时记录为准，
不与前面的1241项或其他分轮测试累计。

## 历史工作区状态（2026-10-23）

### Excel表外跨行合并修复（BUG-038）

本轮按BUG-038继续最小修复：先补字段字典与问题记录，再补原生解析及冻结/装载失败
用例。旧实现5 failed、21 passed（6.15秒）；仅修改excel_parser.py行定位的一处筛选，
按物理矩形相交保留完整原始合并坐标，定义表格仍按完整包含生成grid。修复后聚焦
26 passed（6.29秒），包含纵向、带空尾列的横向、二维合并、同行列不相交反例及
实际Excel解析→冻结→大纲CAS→来源装载往返，行region与表内稀疏grid均保持原义。

解析器源码摘要使parser_version变化，同步生成合成protobuf回放夹具；逐字段核对仅
metadata.source_contract.parser_version变化，不改Rust执行逻辑、通信字段或业务节点。
最终同轮相关回归**255 passed、2 skipped（69.40秒）**，无失败/错误，覆盖原生Excel、
结构化来源、完整请求/短引用、原生证据、分包/预算、来源配置、大纲组织及执行时序。
跳过项为缺少en_mergecell.xlsx及en_calcchain.xlsx；未计作通过，未重跑完整Python套件。
Ruff、git diff --check及文档本地链接/锚点检查通过，独立只读复核无新增发现。

证据位于`/opt/workspace/kb-tender-evidence/2026-10-23-xlsx-merge-fix/`，包含修复前后及
最终XML、跳过原因、源码指纹和合成夹具差异。使用断网容器及合成文件，真实模型调用0，
没有使用DB/Redis；未部署或修改环境，未进行真实招标文件语义验收。
本轮唯一临时测试容器及匿名卷已清理，未创建数据库、缓存或专用网络，未触碰共享开发栈。
纯空白表外布局及模型合并语义不在本批范围，旧冻结产物不会自动补齐；需要补齐旧文件
时沿原来源修订重新冻结和分析确认，不原位改写旧CAS或热换旧快照。

### 大纲来源完整装载修复（BUG-036/037）

只读复审已确认两处来源装载缺口：图像unit之外的冻结OCR/图片/锚点未返回；Excel行
绑定grid时遗漏表外单元格，装载器也未展开grid/row所依赖的共享单元格。先补字段字典
及问题记录，再以9 failed、5 passed（5.46秒）复现，最小修改tender/authoring.py：
保持document/unit原样，补齐冻结OCR/图片/锚点及所需单元格；按原生行坐标补表外行引用，
完整grid仍去重。字段值只在读取副本中恢复，不重新OCR、计算公式或改写CAS。

修复后聚焦14 passed（5.07秒）；补充实际Excel解析/冻结到大纲来源的合成文件往返。
新增测试夹具的ordinal和freeze返回引用使用错误已纠正；最终同轮相关回归
**163 passed（52.67秒）**，无失败或跳过，包含15项来源用例及大纲组织、条件锚点、
共享合同、原生证据、分包、预算、短引用、完整请求及执行时序。未重跑完整Python套件，
不沿用或累加上一批结果。本次无DB/Redis依赖，测试容器断开外网，真实模型调用0。
Ruff及git diff --check通过，独立review-agent只读复核无新增发现。

另以定义表B1:C2及表外合并A1:A2的合成文件确认BUG-038：原件保留，但冻结unit的
merged_ranges全部为空，根因在原生解析的单行包含判定；该批仅记录，当前修复见上节。本批只保证
已有冻结结构不被装载器丢弃，不能恢复冻结前已遗漏的合并跨度。

报告、源码指纹及合成复现位于
`/opt/workspace/kb-tender-evidence/2026-10-23-outline-source-fix/`。
本轮唯一临时测试容器及其匿名卷已清理，未创建专用数据库、缓存或网络。
本批不增加模型调用、业务节点、Prompt或持久字段，未部署、未修改环境或共享开发栈，
未使用真实招标文件验收；该批之后的BUG-038修复见上节，实际模型的大纲语义/容量仍待验收。

### Python大纲独立持久执行（T2）

先补[独立执行字段](field-dictionary.md#t2独立持久执行)与BUG-035，再实现
`tender/document_execution.py::execute_document`。调用方固定execution_id及父分析确认身份，
同身份重新调用即恢复；`integrations/authoring_store.py`用两张独立PG表管理epoch租约、
请求及少量CAS回执。分析run、五节点、默认Task和原确认均不变；没有新增CLI或队列。
本批未接dispatcher，进程退出后的继续执行依赖调用方再次启动，不表示无人值守接管。

共享OpenAIModel完整请求发送入口，保留SSE、超时、响应脱敏、用量及原始审计，不复制
HTTP栈。既有分析调用仍按原构造、短引用及预算委托；拒收恢复只增加编制所有者元数据。
同一编制执行仅一个outline逻辑请求，所有真实发送跨epoch共用四次；反馈变更不重置，
已sent或其他副本已成功时不能再发。传输成功后合同拒收保留成功缓存，再带真实错误
纠正；成功回包或拒收回执保存后中断，同ID重入复用既有响应和反馈。

执行快照固定实际outline提示、Chat地址/模型/窗口及预算，不含密钥或分析提示集合；
密钥轮换可复用，其他已固定配置变化报冲突。纠正完整请求再次准入，预算不符不会
新增HTTP发送，统一重试后仍返回明确预算错误。取消/失租停止工作，迟到响应只补审计；
发布事务同时检查父分析仍为同一已确认修订及独立租约，避免返回前快照检查的竞争窗口。
首次并发初始化的PG目录冲突也已先复现，再用schema级事务锁修复。后续只读review确认
原同时结束优先工作仍漏掉客户端关闭窗口：成功发布后，客户端尚未关闭即可能被心跳误报
失租。本次按BUG-035调整为关闭客户端、停止并等待心跳、原事务围栏发布、返回。

上一批验证使用隔离PG/Redis及合成HTTP/SSE，未调用真实模型。首轮相关回归387 passed、1 failed
（122.47秒），唯一失败为新加入的首次并发setup用例在修复前执行；最小锁修复后完整
Store回归25 passed（5.45秒）。最终执行入口及心跳竞争回归11 passed（85.83秒），包含
成功/拒收中断恢复、统一耗尽、取消、父修订、配置变更/密钥轮换及纠正预算出口。
逐项合并XML去重，该批399个相关用例分轮通过，无剩余失败或跳过，不写成一次完整
399 passed。Ruff、git diff --check及221个本地文档链接/锚点通过；报告和源码指纹位于
`/opt/workspace/kb-tender-evidence/2026-10-23-authoring-execution/`。
该批三个隔离测试容器及专用网络已清理，证据保留，未触碰共享开发栈。

本次发布时序修复用5项事件驱动测试先复现旧实现：4 failed、1 passed（4.03秒）；
最小修复后5 passed（3.51秒）。最终同轮回归40 passed（91.52秒），无失败或跳过，
包含上述5项、25项独立Store及10项执行验收，验证关闭期间续租、关闭失败、组织及关闭
阶段真实心跳错误、等待心跳停止时的调用方取消和原有恢复/发布围栏；未重跑完整Python套件。
测试使用隔离真实PG/Redis及合成HTTP/SSE，真实模型调用0；本次修改的Python文件Ruff
及git diff --check通过，独立只读review未发现新的确定缺陷。报告及源码指纹位于
`/opt/workspace/kb-tender-evidence/2026-10-23-outline-publication-fix/`。
本次三个隔离测试容器、匿名测试卷及专用网络已清理，证据保留，未触碰共享开发栈。

未部署或修改环境、Rust/API/页面，未确认上次真实文件；实际模型及真实招标文件的大纲
语义和容量验收仍待完成。下一步先验证技术条款/评分/表格样例的真实模型执行，再接T3
资料匹配、T4正文/填表、T5核对/导出和T6完整文件验收。

### Python大纲与响应位置（T2，内部组织）

先在[唯一字段字典](field-dictionary.md#python大纲与响应位置t2)定义EffectiveItemRef、
SourceUnitRef及大纲/响应位置合同，再于`tender/schema.py`实现；
`tender/authoring.py`提供load_outline_source和大纲校验；正式组织由`authoring_pipeline._organize`完成。
模型按有效要求、评分及规定格式组织章节；宿主产生章节/位置身份、还原短引用并保存CAS。
同一规则可进入正文、规定表格和证明附件，局部补遗多条保留规则分别引用，不复制旧评分。

输入复用T1当前已确认分析及current_items；相关原文按完整原生unit供给一次，其他来源
只给可用性目录。表格保留明文及空行压缩，片段保留无正文alias；XLSX行映射完整grid，
横向多表不丢引用，虚拟空白页不可当模板。未分配/未绑定/模板缺失/正文未读均保留具体
问题及原有人工出口，不增加业务节点、状态或语义规则。条件元数据锚点同样可回溯，
只有pending元数据不视为已阅读。原人工意见及未核验关系继续保持原义。

该批是接收执行端的Python领域函数：交付完整请求和同步校验器，调用前完整准入，返回前
重查确认版本。分析模型客户端和持久回执依赖活跃分析租约，不能借用来编制；本批没有
接通独立真实付费执行、成功缓存、首次加三次持久恢复或队列。完整请求超限发送0次，
不截断输入，也未实现超限全文件分批合成；显式输出预留需真实样例校准。

新增类型先在入口缺失时失败，关系短引用、虚拟空白页/XLSX来源、条件元数据锚点均先以
失败用例复现再修复。验证使用测试模型执行端和隔离真实PG/Redis，未调用真实模型、
未确认真实招标文件，未部署；不能将本批记为投标编制全流程完成。
最终相关回归333 passed（54.41秒），无失败或跳过，包含30项T2用例和T1/预算/短引用/
原分析兼容回归；此前聚焦27项通过，补充片段和条件锚点后合并复跑，不重复累加。
Ruff、git diff --check及本地文档链接/锚点检查通过。
报告及代码指纹位于`/opt/workspace/kb-tender-evidence/2026-10-23-authoring-outline/`。
本轮三个隔离测试容器及专用网络已清理，证据保留，未触碰共享开发栈。

该批之后的独立持久执行现见上节；技术样例真实调用及T3至T6仍须继续，按任务规划推进，
先定义字段，不新增CLI或Rust/API/页面接入。

### Python编制输入首批（T0/T1）

先在[唯一字段字典](field-dictionary.md#python编制输入t0t1)定义AuthoringInput，再于
`tender/schema.py`实现严格共享类型、`integrations/authoring.py`提供read_authoring_input。
输入只含现有分析身份、确认摘要和五份根产物引用；校验当前analysis-ready、CAS、引用及
持久确认一致性，兼容v2旧normalization。复用current_items和EvidenceRepository按需装载，
保留局部补遗、人工意见及未核验关系状态，不创建新事实台账、CLI、节点或状态。
本轮新增模型调用0，未修改Rust、公共API、页面、环境变量或部署。

新增用例先因入口缺失失败，修复后首轮49 passed；补充片段原文/定位后，相关回归
284 passed（100.17秒），共享合同补充61 passed（10.62秒），两轮去重321项全部通过，
无失败/跳过。其中新增入口用例47项单元、3项真实PG验收，覆盖确认前后、保留疑点及
领域修订后旧版本失效；PG/Redis均为独立容器，模型是测试替身。Ruff、git diff --check
及文档本地链接检查通过。报告位于
`/opt/workspace/kb-tender-evidence/2026-10-22-authoring-input/`。
本轮三个临时测试容器及专用网络已清理，未触碰共享开发栈。

尚未部署或用真实已确认招标文件验收本入口；上次真实运行仍待人工确认，未自动确认。
T0该批只固定输入合同，后续产物仍在各任务编码前细化；T2内部组织现见上节，独立真实
执行及T3至T6尚未完成，不能将输入读取完成记为投标文件生成已跑通。

### 既有分析优化

本轮继续完成[discovery已有关系供给](bug-fix.md#已有关系供给修复已实现隔离回归通过)，
normalization_version=8：本批两个端点均在场的完整Relation及其evidence进入discovery，
目录同时映射关系证据；候选和finding允许引用已提供的关系证据，其他来源仍拒收。
初始分块之后按实际完整并集准入，必要时确定性二分事项块并重建全部块对，保留全部
事项对及关系；不可分超限仍走原预算修复出口。共享verify Prompt同步引用边界，
全部verify摘要受影响，仅新执行使用；没有部署、付费调用、人工确认或提交推送。

核验职责与调度的[最小方案](bug-fix.md#核验职责与共读修复方案最小修改已实现)已实现，
此前normalization_version=7：joint仅覆盖dispositions=[]，其余原文及对象保留，该轮共享Prompt未改；
review/locate/associate/joint阶段内复用既有并发2，依赖阶段仍依次推进。相同宿主请求
身份先加局部锁再获取额度，保存后复用回执；按计划顺序归并。单元最终失败后不启动
排队工作单元，已启动者继续按原重试额度结算；恢复时复用成功者，执行失败和未启动者。
保留原重试、付费围栏、取消/失租及人工出口，没有增加节点、模型字段或配置。

上一轮定向测试先在旧实现复现，再修复：完整单元1020 passed（164.92秒），包含14项并发/恢复
及4项joint职责/反证用例；Ruff、本地链接/锚点和diff检查通过。首轮单元因临时容器
缺少仓库合成语料文件有9项环境失败，补齐原仓库fixture后完整重跑通过，没有修改语料。
临时PG/Redis上的workflow/acceptance共138 passed（349.36秒），不使用共享业务库、
模型密钥或外部模型。三个临时测试容器及专用内部网络已清理，未触碰业务栈；
该轮没有部署、付费调用、人工确认或提交推送。

首轮流程回归136 passed、2 failed：旧重试fixture按整个阶段共用四份坏回复，旧joint预算
预期仍继承处置。已按实际工作单元隔离回复、逐单元检查首次加三次及反馈/审计，并更新
joint预算预期；两项定向验收及最终完整回归均通过。失败费用复审另用失败测试复现
排队调用继续发送，加入阶段停止标记；完整单元和两项定向验收均在此最终源码上通过。

既有relations供给已按本节首段实现，整章补读精度仍仅为后续评估项；新增关系可能增加
discovery分包，没有可靠范围引用时整章读取可能必要。当前修改不证明104次关联
或全程耗时已减少，实际提速及语义行为待新版本真实对照。旧版本运行不热切换，
后续验证须新建运行并保留旧回执；下节为上次部署与真实运行记录，不代表本轮部署状态。

后续项的只读预算核查：同一原运行的1份discovery和4份extract-pack均校验CAS摘要；
原87关系的113处证据引用（73个唯一范围）都在当前460个事项引用/目录范围内，缺失0。
用现有容器、相同完整预算spec及verify Prompt哈希复现基线：输入估算86431、短引用
请求328930字节；仅补现有87关系后为93057、354625字节。输出预留32768、安全余量4096、
窗口131072保持不变，两者均fits；补充后输入距94208准入上限仅1151。此文件仍可一次
discovery，不据此承诺其他文件的块数或真实输出完整性；未发新模型请求、未改源码，
没有重复候选、后续associate/joint或耗时收益的验证。

本轮验证：新增用例在旧实现先复现5 failed / 2 passed，修复后定向125 passed，完整
单元1027 passed（184.80秒）。首轮定向的1项新fixture把未知语义提示误当作非阻断，
改为断言保留该人工疑点及无硬阻断，没有修改宿主问题分级策略。全部相关Python的
Ruff和108个本地文档链接/锚点通过。

临时PG/Redis上的workflow/acceptance首轮134 passed / 4 failed（514.12秒）：3项成功
fixture的刻意scope填充使旧事项对已达14332/14336字节估算，Prompt新增后14441，
生产准入正确拒发。成功fixture重复数12→11，重分包/覆盖/恢复断言保留；真实不可分
超限用例不变。另1项worker重投测试在导入broker前未设置TENDER_REDIS_URL，仅传
TENDER_TEST_REDIS_URL，导致连接默认端口；修正临时容器的测试启动环境，没有修改
broker、业务配置或生产预算。4项复验全部通过（36.91秒），逐项对照XML确认覆盖完整
138项，无遗漏或跳过；这是分轮通过，不是重新一次完整138 passed。报告及代码指纹
在本节证据目录的validation-summary.json、acceptance-initial.xml和acceptance-fixed.xml。
三个临时测试容器及专用内部网络已清理，未触碰业务栈；未部署或调用真实模型。

真实文件只读重规划先精确复现旧locate计划，CAS读取逐份校验SHA-256，再装载相同
source review回执；363事项、87关系、65703个事项对保持完整。采用本轮实际Prompt，
discovery仍1次，输入估算86431→93077，短引用请求328930→354735字节；输入准入
94208，剩余1131。reference仍36次，累计输入估算709692→710304（共享Prompt增加
612估算Token），两类请求全部fits；输出预留32768、安全余量4096不变。没有新模型
回包，不能据此宣称候选减少、下游G/K减少或实际提速。对照报告与修改前源码/Prompt
保存在`/opt/workspace/kb-tender-evidence/2026-10-22-discovery-relations/`，未修改旧回执。

## 上次部署与真实运行（2026-10-21 UTC）

normalization_version=6 已于2026-10-21 02:59 UTC部署。三个Python角色统一镜像
`knowledgebrain-docreader:20261021-retry-v6`，镜像ID
`sha256:578de027146e21279cf90a29e735574460617a6c8ff5295b3517e60a33878bbb`；
53份运行源码/Prompt指纹与工作区一致，init退出0，worker/dispatcher运行。
复用现有PG/Redis及`knowledgebrain_default`，健康预检通过，无新增宿主端口；
并发仍为客户端默认2，窗口131072，Chat/VLM沿用各自既有配置。

用户调整环境变量后，2026-10-21 03:01 UTC重新force-recreate三个Python角色。
Chat/VLM现均为各自显式配置的`gpt-6.1-sol`，窗口仍为131072；
标准`/models`探测均200且模型在列表中，PG/Redis正常，53份运行指纹仍一致。
没有关闭TLS校验或修改用户配置；此结果不证明旧端点已经续期。
新独立运行`e28605cb-468f-46ed-b2cf-e22530ce7025`已正式提交，
source_version/domain_revision均1，request_key=`real-v6-full-20261021-0302`。
自动分析已完成，停在`waiting-confirmation`；尚未人工确认，未到`analysis-ready`。

04:19:34 UTC自动执行结束：2 OCR、4 extract、16 review、37 locate、104 associate、
22 joint全部完成，共185次成功、实际188次发送；3次早期抽取中断通过工作单元重试恢复，
未知用量仍保留，无在途请求。command执行一次、基础设施失败0，租约已释放。
最终363事项、188关系、796原生范围全部覆盖，执行/来源覆盖缺口0，hard_blocking=false。
关系中117条进入独立核验，71条仍标记unverified；进入核验不等于语义已通过。
22条决议最终仅接受2条、解决2问题；累计639问题中637条仍active并进入人工出口，
其中70条原始明确未决仍保留。问题数不是去重后的独立业务缺口数，不能据此宣称语义验收。
尚无人工确认或事项修订，不自动代替用户裁决。已报告3193067输入、235165输出、
3428232总Token，另3次中断用量未知，不能据此计算完整费用。

最终阅读版及完整JSON/Markdown已从实际产物只读导出到私有证据目录
`/opt/workspace/kb-tender-evidence/2026-10-21-full-run/output/`：
`final-analysis.html`可按类别和关键词查看事项、原文引文及页码、关系、处置、问题及决议；
`final-analysis.json`保留实际字段，`final-analysis.md`为完整文本，`source.pdf`为同SHA原件。
导出逐项核对363事项及637待确认问题与终态一致，796处置无重漏，2008条展示记录完整；
报告见`export-validation.json`。导出未触发模型调用、语义合并或人工确认。

### 2026-10-22耗时与任务展开复审（原回执只读计算）

同一运行从命令开始到结束为4624.38秒（77分04秒），verify为3392.83秒（56分33秒）。
核验请求sent_at到finished_at区间峰值在途均为1，配置并发2未用于阶段内调度，见BUG-032。
review16次跨度311.63秒，discovery1次57.69秒，reference36次741.99秒，
associate104次1732.37秒，joint22次476.90秒；阶段规划、收据及间隙另计。
没有核验失败重试，不能把该耗时归因于重试；并发只能缓解等待，任务展开仍见BUG-026。

关联104包中103包仅1候选、1包5候选。原计划出现5500次range身份，来自697个唯一range
（含上下文），平均7.89次，同一range最多进入91包；2367次item身份来自305个唯一事项。
这些是请求来源重复计数，不等于所有原文重复都可以删除。108候选中88个选择完整章节，
24个关联请求携带受保护的明确未决问题；模型继续分析不等于它有权消除保护。
关联产生101关系、22提案、133新问题；没有提案的82包仍可能提供有效关系或缺口。
joint22次中模型认可21提案，18条因所在包另有blocking findings未接受，剩余3条中
1条又因保护问题未接受，最终接受2条、解决2问题。哪些疑点与决议无关尚需语义审查，
没有据此自动放宽接受或删除问题。核验实报输入3099934 Token，为抽取92598的33.48倍；
其中含必要独立复核、不同目标的联合阅读及提示开销，比例不能证明可裁减的原文或任务数量。
优先核实结果归属与重读任务是否重叠；本次只读复审及文档记录，新增模型调用0，
未改协议、源码、部署或确认状态。

### 重读合理性补充复审

全部188发送对应185个唯一(task, request_digest)，成功后同摘要再发送0次；额外3次是
两个抽取包的uncertain恢复。四个核验阶段计划各1份，无递归或外层重跑。去掉work_unit_id
后，183份非OCR模型payload仍无完全相同组。179份核验视图内，同一来源字符区间重叠0；
不能把累计range身份次数当成单包原文重复。关联最高频的context-80c8…实际是10字符卷标题，
进入91包，作为章节定位上下文合理；此前频次统计不能证明91次大段重读。

六组代表对照中，joint-21d40a82366a与joint-44357c88a243共享6个来源范围，但分别核对
续接条件和一般规则的局部例外，两提案均被接受，不能仅凭同源删掉一个任务。
associate-38515729f5bf与associate-95258a6534ae的source_views/items/relations/dispositions
完全相同，来源、事项及所指7个章节相同，均核对同一资格材料引用；问题绑定和输出不同，
属于可评估共读的重叠任务，尚不能直接删除任一输出。joint-0df88a0626fd继承48事项、
101范围及80处置，确认在核对一条提案时同时再次报告其他已知缺口；同一保证金金额来源
共15条问题记录（13来自joint），联系人来源9条。这些是记录数，不等于新业务缺口数，
也不证明相关原文都可裁剪。未完成104关联逐包的语义裁剪验证，未给出可省调用数或时间承诺。

报告与CAS引用位于`/opt/workspace/kb-tender-evidence/2026-10-22-reread-review/`的
`reread-review.md`与`reread-review.json`。本轮新增付费调用0；只更新问题记录及交接，
原始产物、协议、源码、部署和人工确认状态均未改变。

### 核验职责与共读设计（2026-10-22，评审记录）

只读统计104份associate原始发送投影，逐份核对CAS SHA-256；除work_unit_id、candidates、
issues外，其余公共字段逐字段比较，得到102种完整上下文、2个双任务组。因此仅合并完整
相同上下文的结构上限是少2次associate，尚未验证新输出容量或语义，不能作为主提速。
统计使用既有2026-10-21-full-run/live/latest.json及artifacts，无新增模型调用。

三轮复审后收缩[方案](bug-fix.md#核验职责与共读修复方案最小修改已实现)：
一，原共读草案按显式引文重建joint依赖可能漏掉模型未引述的上下文，已撤回该裁剪规则。
二，现有usage可直接作为输出校准起点；共享verify Prompt变更会影响全部核验请求摘要，
不能只算joint。三，通用groups/group_results收益不足以支持立即开发，降为离线证明
完整链路收益后再评审的备选。

已重新按成功回执核对：associate104次实际输入1488915、输出69260、单次输出中位525.5、
最大3318；joint22次输入497394、输出19593、中位984、最大1629。这是旧合同单文件
观察值，不是新包上限。缩减joint续报不会回头减少已完成的104次关联或36次引用补读。

当前建议先仅覆盖joint发送的dispositions=[]，保留原文、对象和旧问题；现有Prompt已有
相应职责边界，先不改Prompt，后续用真实模型新调用对照。032并发调度单独修复，不增加额度，
不把等待下降算作减少调用。源码、Prompt、环境变量、部署及确认状态均未变；本轮新增
付费调用0，未宣称提速或真实验收完成，原运行仍等待人工确认。
本轮仅更新设计、字段约束、验收和交接；105个本地链接/锚点及git diff --check通过。
未运行代码测试，未新增运行证据文件；上述统计只读复用既有真实回执。

### 前述运行过程快照

03:09 UTC快照：登记、冻结及2次去重OCR完成，796个原生范围进入4个抽取包，
全部请求通过现有输入准入。尚无抽取包接受，2个请求在途；已有3份uncertain回执，
两份在约108秒持续输出后传输中断，另一份在约44秒收到供应商
`server_error/request_timeout`。三份均无完整JSON/DONE/usage，不能统计实际用量。
两包分别已实际发送2次、3次，logical_request_digest与command保持不变，
execution_count=1、infrastructure_failures=0，工作单元重试未被外层失败预算抵消。
worker租约与心跳正常；不得将`/models`200或OCR成功宣称为完整文件验收。
已报告用量仅OCR的535输入、127输出、662总Token；不含在途或中断调用未知用量。
问题机制、根因边界与处理见BUG-031，原始流完整保存在新目录CAS，未提交人工确认。

03:23 UTC：抽取4/4包及总索引全部提交，363事项、796处置、202待核验问题，
正在准备独立核验。前两包分别在第2次、第3次实际发送后成功，后两包首次成功；
command始终execution_count=1、infrastructure_failures=0，无失租或外层重投。
累计已报告93133输入、102884输出、196017总Token（含2次OCR），3份中断用量仍未知。
首包只读审查确认187个正文范围对应187个唯一处置，95事项与15关系的197条引文均可定位；
模型声明47 non_item、24 unresolved，宿主按事项证据派生116个items处置，无恢复丢弃或诊断。
该包17个模型问题加24个明确未决范围问题=41；这是来源明确的宿主派生，不是模型重复输出。
首包实际26110输入/27548输出，输入估算110058约为实际4.22倍，输出超过16384预留。
完整回包正常接受；该结构与引用检查不证明全文件语义召回或五节点验收。

03:26 UTC：完整抽取只读审查确认4包正文796个唯一范围，796处置exactly1，
无缺失/额外/重复；363事项、87关系均与原模型数量一致，832条事项/关系引文可在发送内容定位。
事项类型为152 obligation、114 submission_requirement、80 consequence_rule、17 scoring_rule。
处置460 items、221 non_item、115 unresolved；未决中70由模型明确声明、45由宿主恢复缺失处置。
另1处non_item与已有事项证据矛盾，保留事项并记问题；46个extraction_disposition_unresolved
不代表丢弃46个事项。202问题=86模型+116宿主，3条诊断均为evidence_quote_restored。
抽取输出102757，比4包预留多56.8%，前三包超预留；容量参数尚未调整。
独立原文review计划16包，累计输入估算344108，最大32656，0超限；5/16接受、1个在途，
无新增失败，累计已报告214885输入/106833输出/321718总Token，3次中断用量仍未知。
locate/associate/joint尚未规划，不能把review16包当成全部核验调用量或最终语义验收。

03:43 UTC：原文review16/16、locate37/37均首次接受，无新增失败。
review真实输入336076、输出11674；review产生67个问题，与抽取合并为269个唯一问题。
locate计划1 discovery+36 reference；discovery发送全部363事项，输入估算86431≤94208，
本文件没有启用两两分块组合，65703事项对由同一发现请求覆盖。
补读累计输入估算709692，单包17562–24361；259问题进入补读，10问题进入本地关联，
依赖组保持完整，36批中12/9问题的两批未为了8条偏好拆开。
已生成associate104次，累计输入估算1541123、单包最大51384、0超限，第1次在途。
joint尚未规划；正常总调用量目前为2 OCR+4 extract+16 review+37 locate+104 associate+J，
另已有3次中断重发，J未知，不能记作0。累计已报告1206758输入、146312输出、1353070总Token。
运行仍同一command/epoch，未自动提交人工确认或宣称analysis-ready。

04:12 UTC：associate104/104全部首次接受；review16/16、locate37/37保持完整，
核验阶段尚无失败或纠正发送。joint已生成22次，首个在途；正常整文件计划调用量现为
2+4+16+37+104+22=185，另有3次早期中断重发，若余下请求均首次成功则实际188次。
累计已报告2695673输入、215572输出、2911245总Token；3次中断用量仍未知。
关联计划来源为locate101候选+7本地候选=108；5个共享issue依赖候选合为1组，故104请求。
未按共享range/item串为巨包，候选无遗漏；最大192范围/102事项/51384输入估算，0超限。
独立来源在多个候选间重复读取仍是主要累计开销，未因本轮监控热改分组或冻结配置。
locate新增104问题，连同前269合为373，保留未决、无递归重启定位；最终解决数等待joint。

本次最初预检的BUG-030已确认：`api.kunkun-gpt.com:8317`服务端叶证书于
2026-10-21 02:08:32 UTC过期，02:57 UTC新旧容器均在TLS握手报verify_code=10；
公开证书链同样在depth=0报过期，非新版代码回归。后续配置调整及恢复情况如上。
同一106页原件SHA已核对；旧运行、CAS和全部付费回执保留在原目录。
新部署覆盖、预检、指纹及`tls-diagnosis.log`位于
`/opt/workspace/kb-tender-evidence/2026-10-21-full-run/`，没有密钥落盘。
本次通过该目录compose.py及既有
`/opt/workspace/kb-tender-evidence/2026-10-17-full-run/input/manifest.json`
提交独立v6运行；不要把新协议热套旧v4运行，不自动提交人工确认。

工作单元的普通重发及合同纠正统一
首次加三次，以现有请求审计的command_id/logical_request_digest/sent_at持久限额；
按用户最新要求，已补齐所有失败统一重试，包括uncertain、重复拒收及缓存不可验证。
未知付费重发可能重复计费，原回执/状态/未知用量保留，活动sent仍禁止并发重发。
外层基础设施预算独立，完整JSON语法失败提供有界脱敏反馈。反馈首次发送即采用CAS
规范字段顺序，重启继续命中成功缓存；OCR结构校验也在接受前进入同一恢复入口，
有效partial/unresolved仍保留业务问题。相同合同错误继续按真实rejection_count纠正。
最新完整单元1002项通过（171.29秒），统一策略110项、OCR结构4项及异常分类3项包含其中，
不重复累加。此前999项为分类修复前基线（161.03秒）。
完整workflow86项通过（111.34秒），报告为
`test-results/recovery-contract/workflow-unified-final.xml`，包含隔离PG定向49项及增强工作单元16项。
完整acceptance新合同52项通过（419.54秒），报告为
`test-results/acceptance-final/all-failures-final.xml`；最终单元报告为`test-results/unit-complete-final.xml`。
acceptance进程在耗尽异常基类最后调整前启动；该调整的旧领域传播由完整unit及随后完整
workflow验证，未因这处分类修复重跑无关的acceptance用例。
实际OpenAIModel+模拟SSE+隔离PG已验证中断后第二次成功、落盘失败重启复用成功缓存并
在夹具确认后analysis-ready，以及连续四次中断耗尽，5项定向通过。均非真实模型调用。
耗尽异常已改为执行类RuntimeError，旧extract/verify/plan不再吞为领域问题；三项失败
回归及领域/恢复/冻结/队列306项关联回归通过，包含在上述最终整套unit中。
本轮测试环境曾缺/docs挂载，补齐后原985项基线通过；acceptance基线49通过/1项因
导入时Redis地址缺失失败，补齐容器级TENDER_REDIS_URL后该项单独通过，原失败报告保留。
上述离线测试不含模型付费调用；新真实运行已到人工确认入口，终态及待裁决边界见本文顶部。
真实失败SSE按整块、7/17/8191字节分块禁网回放一致，均报offset1939/column1940，
新版含rejection_count的反馈243字符（此前无计数为223）；未接受修补前缀或改写原回执。证据：
`/opt/workspace/kb-tender-evidence/2026-10-21-retry-fix/test-results/`。
新版回放报告为`real-json-replay-unified.json`。所有修改Python的Ruff及diff检查通过。
测试执行容器已退出，本轮专用`kb_tender_fix_pg`、`kb_tender_fix_redis`及`kb-tender-fix-test`
网络已清理；私有报告、真实回执及镜像保留。v6镜像及运行服务已更新，新真实任务已提交。

最近查询2026-10-21 01:45 UTC：run `979ab808-1db3-411e-932c-73ca4a10567c` 已于
2026-10-20 14:27 UTC结束为failed，无在途或uncertain请求。抽取4/4、原文review9/9、
locate25/25接受，associate67/103接受，joint尚未规划；人工确认和analysis-ready未发生。
301事项、45关系、796处置及所有成功回执保留。累计已报告prompt1586409、completion229065、
total1815474 Token，另有一份HTTP500的用量未知。

停止原因：抽取两次invalid_json_response之后，关联`associate-25829aa83789`收到HTTP500，
重试后该单元成功；随后`associate-e6fc49149a18`收到完整DONE/stop但JSON无效，
16914输入/2679输出Token。命令execution_count=4、infrastructure_failures=4，
首次加3次的跨阶段共享失败预算耗尽，回执原因为infrastructure_retries_exhausted。
原始SSE诊断进一步确认：最后两个content事件逐字相同，均为`]}`；完整JSON对象后
多出第二组闭合符，在第1940列报Extra data。独立重建与四种分块回放一致，宿主拼接
未制造错误；上游模型生成还是中转重复发送仍待查。该请求输入估算17120/预算94208，
输出2679/预留32768，无截断或预算不足证据。最后工作单元只失败一次便耗尽command
剩余额度；JSON解析错误也未进入contract_feedback。这两处是上述v6修复针对的恢复缺口。
诊断报告：`/opt/workspace/kb-tender-evidence/2026-10-20-recovery/full-run/live/associate-848d-diagnosis-20261021T015235Z.json`。
本轮完成原回包回放、源码修复及隔离回归，零模型调用，未接受或提交修补后的前缀。
尚未提交新的retry、校正或部署；旧运行冻结于v4，v5镜像仅用于挂载源码测试。
v6不能热套旧run，需以独立新版运行验证；若正式恢复原v4，沿用原合同并复用成功回执，
175条处置的旧版错误出口仍需按下文既定方式处理。

### 前次执行中快照

13:15 UTC：正式运行`979ab808-1db3-411e-932c-73ca4a10567c`仍在执行，原件SHA与下述106页文件
一致；来源/领域修订均为1，request_key=`real-v2-full-20261020-001`。登记、冻结和2次OCR已完成，
抽取4/4包接受：301事项（义务132/提交100/后果53/评分16）、45关系、796处置、214问题。
处置items337/non_item284/unresolved175；完整JSON接受不代表语义通过。失败包前两次返回
无效JSON，第三次正式执行成功，另外3包未重发；execution_count=3、infrastructure_failures=2。
已报告prompt207152、completion116910、total324062 Token（含两次失败），在途用量未知。
独立review计划9包、累计输入218731 o200k Token、最大32726、0超限；2/9接受，第3包在途。
discovery固定部分可完整容纳301事项及引文：1包、67709输入Token、0超限，无需两两组合；
reference、associate、joint依赖真实review回包，调用量仍未知。人工处理及analysis-ready未发生。

本次真实回包另暴露BUG-028处置恢复出口错误：175条宿主恢复问题被错误归为
correct硬阻断。最小修复已完成；保留unresolved及有效事项，后续独立决议/人工
处理，真实来源和执行缺口仍硬阻断。当前v4运行继续使用冻结镜像，不热改回执或切换协议。
若结束后因此blocked，可在原v4通过正式correct把缺省处置明确保留unresolved后重新核验；
不重新OCR/抽取，同摘要成功请求可复用，但变更的核验请求仍付费。此路径不能冒充新code实测。

源码normalization_version=5：96项定向及949项完整单元通过，Ruff/diff通过；完整单元首轮
940通过/9项缺语料文件，补齐一次性容器的只读docs挂载后949项通过，前后报告均保留。
真实PG上`test_analysis.py`整文件12项通过（合成模型），新增用例覆盖独立无事项正文的
处置漏报、完整review、human出口、缺处理拒绝确认及夹具处理后analysis-ready且零新增调用。
首轮11通过/1项失败是DOCX同章合并导致夹具未形成独立范围，补充分章后整文件通过。
本轮未重新跑v5全部workflow/acceptance，不与下述v4的118项合并宣称全套通过。
四份真实已付费回包禁网回放通过：301事项/45关系/796处置逐字段不变，214问题全部保留，
仅175条恢复问题code及宿主问题身份变化，完整台账中均不再protected；真实来源/预算仍hard。
回放0模型调用、0生产及旧产物写入，不当作v5真实调用或语义召回验收。
新候选`knowledgebrain-docreader:20261020-disposition`，镜像
`sha256:a844d96a75abad7f3aa8014f0eeb1ca21c0a19dd755bfa1ae09e0f7eef8d5399`；
53份源码/Prompt指纹与工作区一致、断网tokenizer通过，尚未替换在途v4容器。

BUG-028的可信问题来源修复已部署，normalization_version=4。单元941项通过，隔离PG/Redis的
workflow/acceptance共118个独立用例通过（117项首轮通过、1项修正测试Redis启动环境后通过），
Ruff及diff检查通过。三个Python角色统一镜像
`sha256:85d7d16ed2da9e1841ebdef4d76f7b72077200e268e1abc12cd85d138eb1b3a5`，
部署时53份运行源码/Prompt指纹一致，tokenizer断网检查通过；本轮v5源码修复尚未热替换。
`kb_python_tender_worker`、`kb_python_tender_dispatcher`运行，init正常退出0；未新增宿主端口。
复用`knowledgebrain_default`上的现有PG/Redis；仓库PG账号与共享栈不一致，本次执行在内存中
读取现有API的有效DATABASE_URL，未改共享账号或用户`.env`，私有覆盖文件不保存凭据。
窗口131072、并发2、Chat/VLM沿用已配置模型；worker容器`/models`探测200。
证据、部署覆盖、指纹和只读`poll.py`：
`/opt/workspace/kb-tender-evidence/2026-10-20-recovery/full-run/`；
测试证据在其上级`test-results/`。自动执行到确认前，保留全部付费回执，人工确认不自动提交。

### 恢复现场核对

恢复前现场已无Python招标容器，共享PostgreSQL也没有`tender_*`运行回执表，不能续跑旧run。
`2026-10-17-full-run/artifacts`仍保留：2次OCR成功、1个抽取包接受（67事项/187处置/14问题），
另有3份HTTP401回执；没有完整抽取台账和verify产物。旧运行不能视为仍在执行。
预检确认仓库`.env`与现有API容器Chat Key一致，供应商`/models`只读探测返回200，当时尚未发送生成请求。
BUG-028来源分级遗漏的失败基线为4失败/4通过，修复后状态见上文；隔离证据保存于
`/opt/workspace/kb-tender-evidence/2026-10-20-recovery/`。

## 上次提交状态（2026-10-17，已停止）

真实原件 `BiddingFile.pdf` SHA `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
运行 `bdcf6dac-73b2-4ef5-bb3a-a34019df82f4` 当时已提交：106页已解析，分包计划已写出，模型调用已返回。
首次执行在 freeze 因未定类型的空 metadata 更新失败，修复提交 `92a1bc1` 后用命令 `30de0a63-5048-4eb9-98bc-092a0e2e8921` 续跑。
产物在 `/opt/workspace/kb-tender-evidence/2026-10-17-full-run/artifacts`。窗口 131072，并发 2。
完整五节点尚未结束。

## 此前状态（2026-10-17）

用户已清理此前的Python招标运行栈、数据卷及`/tmp`证据；下文各run只剩文档记录，不能续跑。
当前修改方案及四项已确认决策见[全量复审](bug-fix.md#全量根因复审与统一优化方案)。
P0（`e85cd2f`）与P1（`76f09d3`）已在`pr-24`本地提交、未推送、未部署。
P2源码已实现：模型输入去掉版本号、工作单元按内容稳定、discovery只提冲突/修改/条件依赖候选、
同source_version成功回执跨修订复用；D2（无决议不进joint）原已落地。单元927项、临时PG/Redis上
workflow/acceptance 118项通过，Ruff通过。
P3源码已实现：抽取模型只声明 non_item/unresolved，有事项的范围由宿主按证据派生。
默认窗口 131072，显式纠错最多 262144，抽取输出预留仍为 16384。
G3在预算观察里逐请求标记估算低于实际 prompt、输出超过预留、首包等待达到 60 秒。
并发仍为 2。
未提交、未重建镜像或部署，也未重跑隔离 workflow/acceptance。G2 及完整文件运行需另行确认。
证据目录为`/opt/workspace/kb-tender-evidence/`。

## 发现请求去冗余（2026-10-16，源码完成，未部署）

BUG-025最小修复：多个容量块仅发送全部两块并集，不再另发已被覆盖的基础块；
零/单块保持原处理。只改调度，没有删引文、改预算或引入候选合批及新字段。
新增回归先复现3失败，修复后4个相关测试文件共98项通过，Ruff及文档检查通过。

复用下述真实run的383事项离线对照，旧计划重建一致：discovery 21→15，reference保持12，
定位合计33→27；累计输入估算2672459→2335222（约降12.6%，非实际Token节省）。
全部73153对事项覆盖，保留请求除work_unit_id外的内容、问题路由和本地候选不变。
禁网、产物只读，无新付费调用；报告`/tmp/kb-discovery-dedup/comparison.json`。
真实语义召回及完整流程仍待验证。运行镜像未替换，不把改变编号和摘要的新计划套入旧运行。

用户随后授权真实测试，已启动独立discovery对照run `d26f0b15-4a60-4e30-ac62-b149c75bb4a2`，
同一原件及上述383事项，计划15个真实请求，单并发；使用既有Chat配置、独立测试schema、
正式Store/模型客户端/租约/回执及三次重试机制。固定源码快照SHA（analysis.py）
`af3a9b9520f26e9c1120f5772602078ce99436804a5eca730988b539da4535a5`；原生产产物卷只读，
测试产物及状态在`/tmp/kb-discovery-real/`。这是发现阶段真实回归，不是另一次五节点完成。
启动前旧run快照08:34 UTC：locate33/33完成，associate8/259、6个关联计划包超预算，
有1个调用在途；旧运行未取消或切换源码。本测试与旧verify串行调用合计最多2个在途。

真实对照已完成：15/15首次成功，无纠正、拒收或uncertain，供应商报告输入412301、输出55850、
合计468151 Token，调用时间跨度约19.1分钟；旧21次对应502278/71250/573528 Token。
本阶段实际输入下降17.91%、总Token下降18.37%；候选163→124、findings97→59。
13个业务线索抽查仍存在，包括★否决与加分、付款与否决、财务主体/年份、质保、暂列数量，
并核对了服务费“一个月内/5日内”的原引文；这不是完整召回评测或人工确认。
测试run的blocked是组件测试结束标记，容器退出0；没有执行confirm或宣称analysis-ready。

下游离线对照复用新版15份发现回包及旧版12份补读回包（补读未重新付费）：旧关联计划精确
复现259次/9752104输入估算/6包超限；新版为220次/9461679输入估算/9包超限。
调用数下降15.1%，输入估算仅降约3%；联合来源范围仍大，超预算及逐候选调用尚未解决。
新旧模型候选划分存在波动，不能把超限增加全部归因于取消基础块；不能把候选减少当召回证明。
该220次仅为离线预测，没有付费执行或接入旧run。报告见`/tmp/kb-discovery-real/`
下的`report.json`、`semantic-observations.json`、`downstream-report.json`；旧运行回执未改动。
08:57 UTC旧完整任务仍运行，associate54/259，发生过1次HTTP500后自动重试，
仍未进入joint/confirm；累计已报告1751235 Token。发现阶段验证通过不等于五节点完成。

## A/B版真实文件完整运行（2026-10-16 07:25 UTC）

用户要求使用真实文件完整运行。已在无在途请求及有效租约时，将三个Python角色切到已验收
候选镜像`sha256:6646b63df0f48ee4f2331f8e04288aaecf27581536ffc40aa4f62a1369ee12fd`。
52个源码/Prompt指纹一致，normalization_version=3；独立Chat/VLM沿用原配置，模型及分包
并发均为2。共享PG/Redis就绪，原网络overlay保留，无新增端口；未重启其他服务。
本次Compose额外镜像选择文件：`/tmp/kb-ab-full-real/candidate.yml`，与原网络overlay一起使用。

正式Store.submit新建run `addaa817-3a73-4f92-8445-eea99a9384c7`，来源/领域修订均为1，
request_key=`real-v2-ab-full-20261016-0725`。同一106页原件SHA
`4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`；完整原件已重新
登记、解析、OCR、冻结及规划，未挪用旧抽取回执。执行快照与候选配置一致。
旧run状态、来源版本、修订、manifest、产物、请求、命令及历史保留检查通过。

07:44 UTC：登记/冻结及2次OCR完成，4/4抽取接受，383事项/796处置（义务174、
提交101、后果89、评分19），累计已报告194902 Token，命令首次执行、失败0。
原文review新规划12包，累计输入估算834780，全部准入，首包在途；后续定位/关联/联合
核验尚未规划，不能沿用旧387包或组件验证结果。事项及处置可读导出在`extraction-export/`。
本地导出脚本曾依赖旧运行dropped.json，已改为当前产物的issues和diagnostics；与模型
运行无关，没有重发请求或修改已保存台账。
07:57 UTC：全源review12/12完成，累计已报告451420 Token；locate新规划33次，
其中21 discovery+12 reference，累计输入估算2672459，全部准入。只读重建与已存计划
逐项一致：156条问题记录组织成42组reference、52组local、1组revision；续报只共享任务，
原记录保留。本地52候选和后续discovery候选尚需统一关联，33不是剩余总调用数。
路由证据：`routing-plan.json`、`review-routing-observation.json`。

阶段预算、真实用量、问题及最终结果持续记录在`/tmp/kb-ab-full-real/live/`，只读脚本
`/tmp/kb-ab-full-real/poll.py`。部署及提交证据也保存在该目录根部。

按用户随后要求，已清理空闲`kb_python_tender_preflight_unit`和两个已结束的
`kb_python_tender_ab_validation`、`kb_python_tender_ab_prompt_validation`容器。
116份测试log/XML/JSON及容器日志归档于`/tmp/kb-ab-full-real/history-cleanup/`。
没有删除镜像、卷或真实回执；当前worker/dispatcher、PG/Redis及其他正在运行的测试项目保留。

## A/B实施与验证（2026-10-16）

A/B已在源码实现：有效事项核心局部保留，异常可选字段原值送独立核验；以dispositions
界定漏项检查目标；Issue续报共享处理任务，needs_source决定一次补读或本地核对；
Decision只解决明确列出的记录。预算报告按review/discovery/reference/associate/joint
分别观测，并显示尚未规划的下游成本未知。没有新增主节点、运行状态、业务正则或环境变量。
normalization_version=3，短引用与budget builder为v2；未实施C的引文延后及独立候选合批。

**回归与预算。** 定向失败回归先复现丢事项、路由和报告缺项，再实现修复。第一轮986项
通过；首次全套13失败/1237通过/15跳过，定位为提示过长及两项过期的重复拒收断言。
压缩后全套1250通过/15跳过（524.32秒）。真实反例促使补充提示，11418字节又使8项
32K压力用例失败；最终压缩为10658字节，43项定向回归通过，未放宽预算或删减必要原文。
最后补充关联投影缺陷的回归，2项先失败；最终源码全套 **1253 passed / 15 skipped**
（530.82秒），跳过项均缺少样本。使用隔离测试数据库/Redis和固定模型，Ruff及diff通过。
文档118个本地链接/锚点、4段JSON和26项问题索引及根因覆盖检查通过。
新增4份review语义草案后共32份；合同/导出验证通过，人工标注及整套语义验收未完成。

**原件回放。** 同一106页原件4份已付费回包：原接受315条→新代码保留319条核心，
39条关联、796条处置；4份异常条件原值均进入独立核验输入，11个新review请求准入。
原包边界和原回包未改写，回放不是新抽取或完整语义验收。旧样本extract输出最大36865，
4份中3份超16384预留；不凭样本比值更换tokenizer或扩包。报告仍为observations_only。
证据：`/tmp/kb-plan-implementation/{replay-report,budget-observation}.json`。

**真实定向验证。** 使用独立候选镜像、现有Chat配置和测试数据库独立schema，复用正式
OpenAIModel、Store、call_validated及CAS审计，每次最多1个调用；没有直接HTTP绕过围栏。
首轮run `569a3828-d68e-495e-bdc5-e4cf04ce2a21` 的6份调用均成功返回，但合成上下文反例
出现漏项误报；该失败证据保留。第二轮增加明确双向任务说明并压缩提示，完成11份真实调用：
2份原件review、4份合成边界/路由案例、3份未使用提示示例文字的反例、2份真实关联输入。
全部11份传输及宿主合同接受，0拒收/0uncertain，报告prompt72835、completion10046、
total82881 Token，无未知usage。上下文不再误报漏项；真实目标遗漏、例外扩大、附件未提供及
上下文与事项矛盾均按语义被识别，评分条款及额外body反例也符合预期。两份关联分别返回
1条关系/0问题、5条关系/3问题，保留邮寄要求冲突等疑点；未运行对应joint，不能称业务通过。
第二轮run `ec8786f4-9e61-4940-b862-837345d1473a`，证据目录
`/tmp/kb-plan-ab-prompt-validation/`；原始请求、回包、用量和语义观察分别保留。
组件运行已保留为blocked并注明component_only；这不表示真实工作流已完成或业务阻断
已裁决，没有调用confirm，也不计作analysis-ready或人工验收。

**额外定位的真实根因。** 07:06 UTC只读快照，旧run
`bb5fb879-daa0-462f-82ef-79c45286db51` 已failed：315事项/796处置、review12/12、locate49/49，
associate51/387；计划输入估算15284782、6包超预算，joint未规划。累计已报告2194385 Token。
命令执行4次：1次HTTP500与3次finding身份拒收；associate-2、9的反馈纠正曾成功，后续包
再次拒收而耗尽共享重试。3份拒收都涉及同一联合体事项：输入含其disposition却没有item，
模型指出缺对象后返回该ID，宿主又判越界。修复associate/joint仅发送选入范围且随包items
闭合的disposition，额外原文仍保留；未扩大接收身份、不删除finding或全局处置。
两份原件输入对照原文与items逐项相等，处置由5→3及15→14，均去除1个悬空事项引用。
旧运行和全部回执保留，未提交retry、未修改旧协议或人工确认。

候选镜像`knowledgebrain-docreader:plan-ab`：
`sha256:6646b63df0f48ee4f2331f8e04288aaecf27581536ffc40aa4f62a1369ee12fd`。
尚未替换生产worker/dispatcher的local镜像。新版同文件完整五节点、人工语义核对、
预算容量校准及C阶段仍未完成；组件反例符合预期不代表这些工作完成。

## 历史：全量问题复审与修复规划（2026-10-16，设计阶段）

方案进一步收敛：续报复用不可变Issue记录，只共享处理任务，不新增Observation体系；
Decision仅解决明确列出的记录。非阻断提示无需额外调用，本地候选与定位候选统一组织后
再执行。预算增加校准实施顺序及未知下游计划的报告边界；A可独立交付，B整体接通后交付。
字段字典、工作流和验收已同步，仍仅为文档设计，未修改源码、部署或触发真实模型调用。

第二轮静态审查补齐四处实施边界：false跳过定位后的本地处理入口、续报阻断性及决议快照、
异常可选字段原值实际送审、输出余量校准前不扩包。详见问题台账“第二轮方案审查”；
字典与专项验收同步。本次只改方案，尚无新代码、部署或真实模型验证结果。

已逐条复审BUG-001至026，补齐索引、六类共同根因、五种处理边界和三个交付批次；
将历史测试/运行记录与当前状态分开。参考LangGraph官方的workflow/agent及持久执行说明、
Pydantic AI输出校验反馈机制，复用现有框架、回执与付费保护，不增加通用Agent运行时。

首批修复可选字段拖掉事项与局部核验误报；第二批在现有回包内表达问题续报和原文需求，
由宿主组织一次补读，并行校准预算；第三批验证后再决定引文延后和候选合批。
字段字典的下一修订章节新增两个可选字段设计，尚未改schema/Prompt或部署。
完整原文核验、跨组发现基线和显式修订/人工确认保留；不以缩减调用直接宣称语义不退化。

离线容量对照：315事项完整发送估算266919；延后引文、其他字段不变为184468；仅保留
完整正文和引用仍为151043，均超过110592。延后引文并保留跨组覆盖可从21发现包降至10包，
输入估算1787169→810143；只是实验，不是正式新合同或语义验收。前6包的37候选离线生成
37个associate请求，当前joint整包接受条件使直接合批存在相互阻断风险，故列为后续优化。
证据：`/tmp/kb-shortrefs-real/proposal-review.json`；本轮方案审查未新增模型调用。
文档检查：6份文件的114个本地链接/锚点、4段JSON示例、26个问题编号与六类根因覆盖检查
通过，git diff --check通过。本轮仅更新文档，没有运行或声称通过新的代码回归。

## 短引用修复版真实重跑（2026-10-16 05:08 UTC）

用户明确选择取消旧任务并立即测试新版。正式Store.cancel已将
`6f0a29a2-8f45-4725-bebb-70485d8a75ba`取消，已完成产物精确保留；当时在途请求
`aea3c80c589949519c72523f0a3226a3`记为uncertain，不自动重发。

无在途请求/有效租约检查通过后，修复版镜像
`sha256:98f3e76af8a8910481e0d141209f897c947acfe1acd23aa122f0404864b98856`
已部署：worker/dispatcher运行、init退出0，52个相关源码/Prompt指纹一致；
normalization_version=2、短引用budget builder、Chat/VLM原配置、并发2、网络overlay、
PG/Redis和产物卷均核对通过，无新增端口。

同一106页原件通过正式submit创建独立run
`bb5fb879-daa0-462f-82ef-79c45286db51`（source_version=1/domain_revision=1），
请求键`real-v2-shortrefs-recovery-20261016-0508`。原件SHA/manifest不变，重新付费，
未复用旧运行回执冒充新协议结果。05:08快照：登记及2次OCR完成，运行中。
本轮观察实际短引用映射/请求摘要、宿主接受、纠正请求、用量与执行路径；不代替人工确认。
06:02 UTC快照：抽取4/4包接受，315事项、796处置（items431/non_item343/unresolved22）；
事项为义务118、评分22、后果84、提交91。模型原始319条中4条使用合并键
conditions/exceptions，保留为未决问题及原声明，未静默删除（BUG-023）；恢复8处源换行。
独立原文核验已接受12/12包，定位20/49包；review累计输入估算846749、locate3972953，
全部计划输入准入。05:49 UTC discovery-13明确HTTP500后自动恢复，该包已接受，命令第2次
执行、失败计数1，未重做成功兄弟；无model-rejection/合同反馈纠正。供应商内部500原因未知。
已报告983207 Token；已完成抽取最大输出36865，超过16384预留但实际总上下文未超限。
review-2发现一例上下文事项被误报漏项（BUG-024），正在观察后续定位处理，未热改协议。
旧运行部署后11项保留检查通过，未新增旧请求。关联、联合核验、确认尚未开始。
用户要求查看的全量事项/处置/原文证据已导出到`/tmp/kb-shortrefs-real/extraction-export/`。
证据：`/tmp/kb-shortrefs-real/`；运行尚未完成。

## 合同拒收纠正、短引用及宿主核验覆盖（前轮源码验证记录）

本轮按用户认可方案先更新字段字典，再实现BUG-010/022；保持五节点和业务范围不变。
`llm/recovery.py`使用现有产物回执保存具体拒收路径与调用/响应引用，下一次执行发送
真实错误反馈及相关片段。传输succeeded不改写，一次执行最多调用一次，沿用首次加3次
重试；重复已拒收的纠正内容明确停止。原回包、成功兄弟和sent/uncertain围栏保持有效。

`llm/wire.py`统一请求内短引用，原文/引文/理由不替换，父定位及源偏移留在宿主；
保留alias片段区间、单元格、章节、角色和作用域。`model-wire`保存原payload和映射，
单个request_ref防止不同工作包被压缩为相同付费摘要。预算试算和实际发送共用转换器。
review模型只返回schema_version/accepted_decision_ids/findings；宿主从实际请求派生
reviewed_*接受回执。模型附带旧名单时忽略并记诊断，不据此拒收或证明语义完整。
语义样本导出同步使用短引用，将还原映射保存在review目录；不改变草案标注的验收状态。

协议以normalization_version=2及chat_json_object_stream_short_refs_v1冻结。
当前真实run继续用已部署旧协议，不热替换、不把旧产物冒充新协议验收。

验证：新增恢复测试先因缺实现失败；广覆盖493项单元回归通过，最终名单容错及停止策略
177项定向复验通过（与493项有重叠，不相加）；最终85项隔离PG/Redis工作流及分析验收
全部通过，包括完整SSE成功→宿主拒收→反馈纠正→重启缓存复用、共享4次执行上限和
五节点确认。Ruff/diff通过；未重跑全部Python套件，没有真实付费纠正验收。

真实106页原件、330事项的禁网核对：相同选定payload及当前Prompt比较，4包抽取Wire
405792→370828字节（8.62%）；13包新规划review为1046599→920310字节（12.07%）。
全文/引文/单元格/作用域/引用往返逐项一致，全部准入。这是字节测量，不是Token降幅，
13包是新发送协议离线规划，不能替代当前运行的19包回执。
证据：`/tmp/kb-protocol-check/report.json`、`/tmp/kb-protocol-tests-final2.log`、
`/tmp/kb-protocol-last.log`；隔离容器内`/tmp/kb-v2-test-results/contract-release.xml`。

2026-10-16 05:02 UTC只读快照：当前真实run `6f0a29a2-8f45-4725-bebb-70485d8a75ba`
抽取4/4（330事项、796处置）、原文review19/19接受；locate6/58接受，第7包在途。
真实locate计划累计输入估算4933763（非实际Token），全部单次准入；不是旧产物40包测算。
累计已报告749139 Token，命令首次执行、失败0。关联、联合核验、确认及analysis-ready未完成。

## 新版本真实文件重新运行与评测迁移（2026-10-16）

用户明确选择“用新版本重新跑同一文件，保留旧运行及全部回执（会重新付费）”。
已通过正式Store.submit创建独立run `6f0a29a2-8f45-4725-bebb-70485d8a75ba`，
source_version=1/domain_revision=1，请求键`real-v2-locate-compressed-20261016-0400`。
同一106页原件SHA `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`，
manifest与旧运行完全相同；旧run及状态/版本/来源/修订/产物/请求/命令/历史10项保留检查通过。
没有把旧成功产物标为新协议结果，没有重发或改写旧uncertain。

首次提交被自动审批要求核实具体外部端点授权；只读比较.env、运行容器、旧执行快照与
原件SHA一致后，原样重申请同一提交动作获准。当前Chat/VLM仍使用已配置的
`gpt-5.6-sol`及`https://api.kunkun-gpt.com:8317/v1/chat/completions`，并发2，
网络overlay与03:52部署的定位压缩镜像不变。新run冻结execution_spec与提交时核对值一致。
新运行的事项数量、定位包数将以新模型结果重新规划，不能照搬旧319事项/40包离线结果。
2026-10-16 04:29 UTC快照：登记、冻结、2次OCR及全部4包抽取已完成，
330事项、796处置（135义务、91提交要求、84后果规则、20评分规则），run为running。
当前命令首次执行、失败0，尚未生成核验阶段计划；累计已报告231945 Token。
当前仍未完成五节点及人工语义验收，事项计数不代表语义正确或完整。

同时完成BUG-020：28个合成Verify输入迁移v2（13组正反+2个跨文档），表格明文cells、
独立文档角色、补遗原事项及retained_rules决议均使用当前合同，预期保持agent草案。
禁网导出28份输入，累计输入估算352308、最大13301，全部准入；标注与模型输入隔离。
仍无付费语义评测执行器，未运行合成样本的真实评测或人工裁定，也未据此宣称抽取完整。

BUG-021新增源码status的budget_observation，只读区分计划与实际Token、输出max/p95、
超预留次数、实际总量与上下文容量及未知usage。旧运行4次成功抽取均超16384输出预留，
最大输出36660，但最大实际total69842、未超131072；66份核验usage输出max8417/p957399，
最大total40042，另1份uncertain用量未知。该报告没有调整预留或增加阶段硬门限。
报告CLI改动未热更新正在执行的新运行；离线使用同一源码报告，不影响固定协议。

新增失败基线10 failed/1 passed；最终107项单元/模型预算回归及1项隔离PG status验收通过，
Ruff和diff通过。未重跑全部Python套件，未将结构验证替代语义验收。
私有证据：`/tmp/kb-semantic-v2/`；`evaluation-final/`为最终离线导出，
`old-budget-observation.json`为旧运行只读报告，`live/`保留新运行快照。

## 定位输入压缩修复已部署（2026-10-16 03:53 UTC）

BUG-014代码及Prompt已实现：discovery目录只携带本批事项证据所在的文档/章节、角色、
完整heading_path和range_ids映射；全部TenderItem及evidence.quote不变。reference保留
完整目录、问题原ID/detail和原文，按完整输入预算合批且初始每批最多8题。块间容量
同时预留双方事项和目录；不增加业务正则、工作流节点/状态或环境变量，并发仍为2。
无新事项的删除补遗由全源review产生未决目标，再走reference完整目录定位，已有回归覆盖。

使用同一106页文件的319事项、147问题和17份review产物禁网复算：旧持久计划精确重建；
新计划 **21 discovery + 19 reference = 40包**，原为202包。累计输入估算
**15287999 → 3535319（减少76.88%）**，单包最大109812/110592，全部准入。
全部事项/quote逐项相等、147问题各一次、50721事项对覆盖及原区间/正文/cell内容均核对。
这是新生产Prompt及planner的输入测量；新批量实际输出、语义完整性及费用未验证。
新测试失败基线6 failed / 1 passed；修复后336项定向回归、83项真实隔离PG/Redis工作流
及分析验收通过，0跳过/失败/错误，Ruff及diff通过；未重跑整个Python套件。
证据：`/tmp/kb-locate-fix-576syj9o/`；测试XML保存在测试容器`/tmp/locate-regression.xml`
及`/tmp/kb-v2-test-results/locate-fix.xml`。

旧run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8` 已于03:39 UTC停止，03:48只读确认
status=failed。2次OCR、4包抽取（319事项/30关系/796处置）、17/17原文review及49/202
locate均保留；第50包请求`a892d57537f845338e974360da01ac01`等首数据180.175秒后
记为uncertain，原始字节/DONE/usage均未收到。BUG-019记录直接失败点与底层原因待查。
累计已报告prompt1884357 / completion465350 / total2349707 Token，超时调用用量未知。
当前无在途请求；完整定位、关联、联合核验、确认及人工语义验收仍未完成。

本次Prompt及计划摘要发生真实变化，旧修订冻结的execution_spec不被改写；不能给旧run
直接换新40包计划，也不能用correct/revise伪造业务变化。服务升级只供新的执行协议使用；
旧任务未重发、未清缓存、未新增来源/领域修订，旧成功回执保留。原协议恢复需先核实
uncertain的供应商执行结果，并使用匹配的旧镜像/Prompt；尚未实现跨执行协议迁移。
旧运行镜像：`sha256:78ef880f0672c6ad8ddbdc3844a748a688b911791fa57dbf9cff1316ecf0df32`。

03:52 UTC新镜像`sha256:4d3c59667e3ac306915fea7008ffd3e9b895ae41e906c459d78724c55e05df14`
部署核对通过：worker/dispatcher运行、init退出0、三角色同镜像，106源码/Prompt指纹一致；
PG/Redis连通、模型与抽取并发2、Chat/VLM配置保留、无新增宿主端口，原网络overlay保留。
03:53 UTC旧run的状态/版本、来源、修订、产物、请求、命令及历史10项精确保留检查通过，
没有新增模型请求，也未把旧49包回执标成新40包计划完成。真实输出验证与旧断点恢复未完成。

## 前次真实调用测试（2026-10-16 02:10 UTC）

02:10 UTC补充：locate 2/202 已接受，第3包在途；第2包为修复后新增的真实模型调用，
当前命令仍首次执行、失败0。累计已报告 prompt 489743 / completion 192437 /
total 682180 Token。以下02:08记录保留首包缓存恢复的检查事实。

同一 106 页文件、run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8` 继续使用 `.env` 已配置模型。
4 个抽取包保留 319 事项、30 关系和 796 处置；**17/17 原文 review 已全部接受**。
这表示核验回执完整，不表示语义通过：review 共返回 89 条 findings，包含重复及待跨包
核对的问题。例如防火墙参数的局部“漏抽”提示在完整台账已有另一条事项对应，不能据此
认定全文缺失；抽查已核对实际抽取请求摘要及原文 cells，尚未代替最终联合核验。

实际 locate 计划为 **202 包**：55 个 discovery、147 个逐问题 reference，输入估算合计
15287999，单包最大 110271，均准入。只读重建与实际计划精确一致：目录重复 10053540
字节，占 payload 合计 13186795 字节约 76.2%；319 事项重复出现 3385 次。已更新 BUG-014，
本轮尚未优化请求组织；估算不是实际计费 Token，后续 associate/joint 容量尚未知。

首个 locate 完整回包因候选 `range_ids=[A,B,A]` 被拒收，随后首次加 3 次执行重试均复用
同一缓存失败；新增 BUG-018。最小修复只在宿主整理完全相同的候选来源引用并记录诊断，
不改模型语义、原回包、Prompt、请求摘要或计划。失败基线 2 failed / 6 passed；修复后相关
协议、缓存和流式 **358 passed / 0 skipped / 0 failed / 0 errors**，Ruff、diff 通过。
禁网、产物卷只读的真实回放保留 13 候选和 2 条 findings，202 包计划及请求摘要不变。

02:05 UTC 部署镜像 `sha256:78ef880f0672c6ad8ddbdc3844a748a688b911791fa57dbf9cff1316ecf0df32`，
三角色同镜像、init 退出0、106 源码/Prompt指纹一致、并发2、PG/Redis连通，原网络overlay保留。
首次正式恢复被自动审批要求核实文件和具体外部服务授权；只读核对原文件 SHA、当前环境与
冻结执行配置一致后，原样重申请同一正式 retry 获准，未绕过拒绝。

恢复命令 `8842cf8f-61eb-4238-919a-f98271e9ee51` 首次执行、失败0。02:08 UTC 首个 locate
已直接复用缓存接受，第2包在途，run 为 running。13 候选、2 条 findings 及去重诊断均
实查通过；原产物、请求、命令、来源及领域修订的10项保留检查通过，两个历史run快照不变。
累计已报告 prompt 465694 / completion 186839 / total 652533 Token；在途用量未返回。
完整定位、关联、联合核验、确认及人工语义验收未完成。BUG-010 通用合同恢复仍未实现。
本轮证据：`/tmp/kb-real-call-wfdivvxl/`；未提交或推送。

## 前次恢复记录（两类审阅身份修复已部署）

2026-10-16 01:46 UTC补充：review 6/17有回执，第7包在途；当前命令仍首次执行、失败0。
累计已报告prompt325844 / completion149740 / total475584 Token。以下01:44记录保留恢复时的检查事实。

2026-10-16 01:44 UTC实查：同一run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8` 为running，
review 5/17有宿主回执，第6包在途，恢复命令 `075d3838-f090-4bda-9a26-c4e6892ff36e`
首次执行、失败0。BUG-009修复后第4包正常接受，第5包发现新的ID转写漏字：应收88个、
实回88个，1个缺失、1个未知、无重复；真实摘要及17包计划匹配，事项35/35、关系1/1匹配。
已新增BUG-017，宿主仅在单个未知/缺失身份可由全部已知来源唯一匹配单字符省略时恢复；
不改findings业务引用。未知但已在其他来源登记、歧义、替换和多字符差异仍拒收。

新增失败基线1失败/21通过；修复后相关306项全部通过，零跳过/失败/错误，Ruff及diff通过。
真实第5包禁网只读回放通过；部署后正式retry直接复用缓存，88范围、5条findings及1条
恢复诊断均实查通过，无第5包新增请求。原4包抽取、4包核验、请求审计及修订的10项保留
检查通过，两个历史run完整快照不变。截至该快照已报告prompt293259 / completion144121 /
total437380 Token；第6包用量待返回，完整流程及人工语义验收仍未完成。

01:42 UTC部署镜像 `sha256:ca52c23aeeff834aa0736b46eb9db933b2c8fb2b70f9cb3c2f8087b6f7fffc29`，
三角色同镜像、init退出0、106个源码/Prompt指纹一致、并发2、PG/Redis连通，网络overlay保留。
本轮生产变更仍仅在analysis.py身份处理，未改模型请求、Prompt、分包或生产预算。
证据：`/tmp/kb-review-id-omission-picsmw16/`。BUG-010已恢复两类可确定性修正的身份案例，
一般合同拒收恢复仍未完成；不将其泛化为任意失败可自动恢复。

### 父定位ID首次恢复（2026-10-16 01:29 UTC）

2026-10-16 01:29 UTC实查：同一真实run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8` 已恢复running，
review 3/17有宿主回执，第4包在途；恢复命令 `5000f6e7-e047-4c9a-bfc3-c6dbad135770`
首次执行、失败0。第3包直接复用原传输成功缓存，宿主将92个身份规范化为85个实际范围，
保留8条findings并记录7个冗余父ID诊断；未为第3包新增模型调用。10项保留检查通过，
原4个抽取包、2个核验回执、原始回包及用量保持不变，来源及领域修订仍为1。
截至该快照已报告prompt231752 / completion134870 / total366622 Token；在途用量待返回。

本轮仅修改 `tender/checks/analysis.py` 的父定位身份归一化；先经schema检查原列表无重复，
实际范围均已出现时才移除当前body/supporting携带的冗余父ID，未知/缺失/重复仍拒收。
未改Prompt、payload、分包计划或预算。补齐重复父ID和完整SSE成功后宿主拒收/缓存恢复回归；
定向330个独立用例通过，零跳过/失败/错误，Ruff及diff通过；未重跑完整Python套件。
真实缓存禁网只读回放核对request/17包plan摘要不变，模型、网络、DB及产物写入均0。

01:26 UTC已部署镜像 `sha256:1c55ddb645f25c1a4ea3b9ac4929c9bcb72be2be49f15d02b79e73b0ef0a2069`，
三角色同镜像、init退出0、106个源码/Prompt指纹一致、并发2、PG/Redis连通，原网络overlay保留。
恢复首次被自动审批要求核实数据与目的地授权；只读核对原件SHA、10条原请求和.env服务
与冻结执行快照一致后，同一正式retry命令获准执行，未换路绕过拒绝。
证据在 `/tmp/kb-review-id-fix-85w1gdzd/`。BUG-009已完成真实恢复验证；BUG-010的一般合同
拒收恢复尚未完成，不能把当前可确定性恢复案例泛化为任意错误已解决。真实五节点及人工
语义验收仍未完成。后续发现问题继续更新[问题记录](bug-fix.md)。

### 修复前的失败诊断（2026-10-16 00:51 UTC）

2026-10-16 00:51 UTC只读实查：真实run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8`
已于2026-10-15 10:35:36 UTC终态failed。4个抽取包保留319事项、30关系、796处置；
17个原文review仅2个有宿主回执。第3个verify收到完整JSON并记录传输succeeded，但其
reviewed_range_ids应为85个，实回92个（额外7个，缺失/重复均0），事项61/61、关系3/3、
决议0/0均匹配；未通过`Independent review did not cover exactly the supplied identities`。7个
额外ID已精确核对：全部来自已发送supporting条目的source_range_id父定位字段，
模型把父定位身份额外列作reviewed_range_ids；不是未知ID，也不是alias segment漏算。
宿主以实际range_id检查85项与回包92项不符而拒收，语义正确性仍须独立核验；
首次失败后的3次自动执行均复用同一已成功传输的缓存响应，重复合同错误而没有新模型调用。
当前命令execution_count=4、infrastructure_failures=4，预算耗尽后停止，无在途模型请求。
累计供应商prompt231752 / completion134870 / total366622 Token。尚未开始后续跨源定位、
联合核验或确认，不能称真实五节点通过。当前断点为核验回执/缓存重试衔接；后续恢复需
保留原回包和已成功抽取、核验回执，不通过清缓存、改摘要或删覆盖检查来伪造通过。
只读归属诊断还核对真实request_digest与完整17包plan完全一致，证据在
`/tmp/kb-review3-origin-9rmepgx_/report.json`。输入同含range_id和source_range_id而回包要求
模型完整枚举ID，将机械身份管理负担留给模型；传输成功缓存与业务接受的恢复策略未衔接，
造成3次执行重试原样重放无效回包。当前未改源码或重发模型。
本段替代下文10:29“正在运行”的历史状态，证据见
`/tmp/kb-lease-repair-_aszzk_8/latest-failure-summary.json`及`live/`快照。


用户最新确认本期只做**完整分析招标文件**：识别原文中的义务、评分规则、不利后果，
以及原文明示的填表、签章、附件与证明材料提交要求。技术、服务、售后、人员等是来源中的
业务内容，不按固定分类凭空补项。不采集或生成企业响应数据，不做响应计划、材料准备待办、
评分机会采纳或实际得分判断；企业是否具备材料、人员和资格也不在本期分析结论内。

当前工作区主线为 `register → freeze → extract → verify → confirm` 五节点，确认后终态为
`analysis-ready`。统一最小 `TenderItem` 合同为 `item_id/kind/text/evidence`，`kind` 仅有
`obligation`、`scoring_rule`、`consequence_rule`、`submission_requirement`；条件、例外、
分档/公式、否决触发范围等必须在条目文本及来源证据中完整保留。提交要求描述原文要求
提供什么，不填写企业实际信息，也不派生准备任务。条目类型和业务含义由 LLM 审阅，
宿主核对结构、来源引用、覆盖、身份版本、预算及执行围栏，不扩充正则承担语义裁决。

跨包引用只允许一轮有边界的目标补读；目录存在不等于正文已审阅，收到目标原文后才可
核验关系。仍然歧义、不可读或需要继续追索的内容转人工证据裁决，未解决前继续阻断。
不因“本包未提供”断言全文缺失，也不以人工确认覆盖来源或语义问题。

**五节点、新条目合同、抽取/核验提示、确认摘要及CLI已完成首次v2部署。** 新主线不再推进
旧Plan，不把旧产物转换成v2分析结果，也不改写旧运行记录。已部署的压缩版真实任务已结束，
抽取协议校验失败，尚未进入独立核验。

2026-10-15 10:24 UTC补充：同一run的失败抽取包已恢复，4包共319事项（117义务、18评分、
81后果、103提交要求）、30关系、796处置；原3包及全部成功付费回执保留。累计供应商用量
259898 Token，8个助手业务锚点均有事项匹配，不能代替独立核验或人工验收。

抽取完成后发现核验同步规划阻塞事件循环：真实离线规划耗时39.36秒，超过30秒租约，
导致旧命令8次执行而失败计数仍0。最小修复将三类规划、描述符预算测量、回包校验与读取
移到线程，预算/请求/回执身份不变；同一文件规划17个review请求覆盖796/796范围，最大
输入估算110490/110592，无超额；702次模拟心跳最大间隔0.149秒，断网且CAS只读。
失租由接管方计入原4次共享失败预算，兼容旧execution_count漏计，旧Worker继续受围栏保护。
旧恢复命令按8次失租终结，未增模型调用；再经显式retry建立同版本命令
`203a1ee6-2d6f-4af6-b454-d15223faf740`继续核验，13项保留检查通过。没有清DB状态、改来源
版本、改请求摘要或重抽成功包。

该修复10:23 UTC部署，镜像
`sha256:7552a5d995f576e6850bf4505b94cb8dd945b4641ad5d407c86e1d1122428858`；三角色同镜像、
init退出0、106个非测试源码/Prompt指纹一致，模型及分包并发2、PG/Redis连通、无新增端口
或环境变量，原网络overlay保留。定向143项通过；新增心跳4例和失租8例均先复现再修复，
完整Python回归1175 passed / 15 skipped / 0 failed / 0 errors，472.191秒；15项因缺少历史
或私有样本跳过。106个非测试源码/Prompt测试后指纹未变，Ruff及diff检查通过。10:29:48 UTC
真实核验1/17已接收，第2个请求在途；当前命令仍首次执行、失败0、心跳有效。累计供应商
用量292910 Token；部署前两个历史run全快照复核不变。证据在`/tmp/kb-lease-repair-_aszzk_8/`，
五节点终态及用户人工业务验收仍未完成。以下09:56/09:58记录为该轮历史状态。

用户随后同意补齐失败包恢复，并要求执行失败默认重试3次。该版于09:56 UTC部署：
镜像`sha256:cfdd20e86d1fad7b17fbc030d11f610c269f4c80667ccbea92835c1539a1addc`，
三角色同镜像、init退出0，106个非测试源码/Prompt与验证基线一致；并发2、PG/Redis连通，
保留网络overlay，无新端口或环境变量。明确执行失败采用首次加最多3次重试，命令发布与
执行共享4次持久失败预算，不叠加SDK重试；取消、版本/租约失效和sent/uncertain仍停止。
并发子操作优先传播不确定付费或失效围栏，防止被格式错误掩盖。

新增CLI retry在同一run/source_version/domain_revision新建幂等命令，保留原kind和业务
payload；重试审计不进入模型或确认产物。当前已以命令
`b1297d2b-c359-4a58-9d77-1bac5679df99`恢复下文run，09:58 UTC为running，仅新增失败包
1个extract在途，原3包221事项/518处置和OCR2次均复用；Verify尚未开始。
本轮定向回归分组最新197个独立用例全部通过，0跳过/失败/错误；包括默认3次、耗尽后显式
恢复、成功包复用、确认resume和uncertain优先停止。首组187通过/1旧预期失败，更新首次失败
即终止的旧断言后，补跑17项全部通过。14个修改Python文件Ruff及diff检查通过；未将该
定向结果称为完整Python回归。证据在`/tmp/kb-bounded-retry-vawj54vv/`，最终五节点与用户
人工业务验收仍未完成。下文09:32 failed及无恢复入口描述为本次修复前的验证事实。

用户于2026-10-15 UTC明确要求使用真实招标文件再次验证。本轮回包容错版已于09:14部署，
三角色镜像为`sha256:1344994e60b6091743e3542ab1c35af52b390a27f583125f3089fa438baa5fce`；
168个源码/Prompt与完整回归版本逐字节一致，init退出0，PG/Redis连通，模型及分包并发2，
保留原网络overlay，无新增宿主端口。使用.env既有Chat/VLM服务与`gpt-5.6-sol`模型，
目的地与上一真实run一致，未新配模型或改旧运行。106页原件SHA256仍为下文同一摘要。
新run为`eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8`，source_version=1、domain_revision=1，
以新请求正常重新登记/冻结及OCR，不将旧回包伪装为新请求。运行于09:17:01开始，
09:32:00 UTC终态failed，耗时899.62秒；OCR2次成功，extract3次成功、1次失败。
3个extract-pack保留221事项（56义务、16评分、74后果、75提交要求）、25关联、518条处置，
未接收的一包对应278条原文范围。独立Verify和确认均未开始，不能称真实五节点通过。

失败请求`f9408dfbe11c459fa2dc9344143e4e31`已收到完整SSE、DONE和`finish_reason=stop`，
有供应商usage；content的两个多余右花括号导致`invalid_json_response`，不是网络超时或
流式截断。仅在内存中移除两个符号的诊断可得到JSON，但其dispositions为空，仍不足以
证明完整覆盖；该诊断不写回、不作为有效抽取、不替代独立语义核验。当前局部字段容错
发生在JSON解码之后，无法覆盖这种整包语法失败；待补齐显式恢复能力，不能直接改DB
重开命令、伪造来源修订或把格式修复当成业务核验。

6次调用全部报告usage：prompt108140、completion83180、total191320 Token；单次extract
最大completion36660。没有在途或uncertain请求，未自动付费重试。实际发送前覆盖审计为
106页、796范围、4包173/214/278/131范围，原文缺漏/重复/区间错误均为0；请求Wire合计
401776字节，4包输入估算均不超110592，最大110482，不能将这些字节当作实际Token。
本轮168个源码/Prompt指纹未变，两个旧run快照深比较一致，旧uncertain围栏保持。
部署、前后旧运行快照、真实终态及用量证据在`/tmp/kb-tender-real-check-ef2lx0v4/`；
发送覆盖审计在`/tmp/kb-live-plan-audit-70htrgxw/report.json`，JSON根因和终态审计在
`/tmp/kb-tender-terminal-audit-ay9itnq6/{failed-json-report,report-terminal}.json`；
14个不同CAS产物的路径、SHA和size核对通过。接收包有34条源LF恢复诊断、15条
unresolved处置，以及34次阻断问题记录，尚未经独立Verify；这些数量不能当作最终业务结论。
目前没有公开的同run失败包恢复入口：底层虽可复用成功回执、允许明确failed的新请求，
终态command不能靠重新投递恢复，且本run缺完整extract根产物，不能走correct伪装恢复。
恢复缺口只读报告为`/tmp/kb-tender-real-check-ef2lx0v4/recovery-gap.md`。
8个业务锚点的助手抽样复核中，评分、否决、财务例外和合同证明6项内容保留；质保服务
1项内容保留，但同一提交事项承载义务和后果，类型与拆分是否恰当待Verify；技术证明1项属于失败包，尚无
有效产物。未执行用户人工验收，不以抽样代替独立核验。报告为
`/tmp/kb-business-anchors-RIG0X2/assisted-sample-review-terminal.json`。
下方尚未部署、调用暂停及六阶段记录均保留其对应轮次的历史含义。

2026-10-15 UTC压缩修订与复审：

此前压缩修订完整Python回归：**954 passed / 15 skipped / 0 failed / 0 errors**，共969项，
404.143秒，退出0；隔离PG/Redis及确定性模型，无真实模型调用。15项因缺少历史或私有
样本跳过。部署配置9项、44个Python文件Ruff、diff检查通过；164个源码/Prompt文件
测试前后指纹一致。完整证据在`/tmp/kb-v2-finaltests-0cs6i341/complete-results/`。

- 压缩版三角色镜像为`sha256:230c2822f8683fdb9ff79112976e06d3fe1b46fb7edb12aaac6cace97df0becb`，
  init退出0、并发2、PG/Redis连通、旧run未变；证据为
  `/tmp/kb-tender-v2-live-iaveude4/compressed-deployment-verification.json`。
- 抽取不发送全目录，保留当前role、原文和必要上下文；locate目录按文档嵌套章节，
  无全量range列表，用section_refs由宿主装载完整章节。目录114909→49753字节。
  同一106页文件离线4包/796正文范围全部准入，最大输入估算110254/110592，覆盖与运行投影一致。
  796条处置仅最小JSON结构就需74328字节，不能当作真实Token或完整输出预测。
- 上一版全套910 passed / 3 failed / 15 skipped（416.81秒）；3项合成压力用例的joint/review
  超过14336输入额度。修复仅校准测试填充，生产上下文、输出预留及超额阻断保持不变；
  增加“associate可准入、完整关系使joint超额且零发送”负例，最终回归已全部通过。
- 格式复审的源码修复已落实：views按登记文档、冻结范围和片段起点排序；cell直接发送
  已切好的原文，能完整表示时省略重复整行text；抽取和关联共用实际可见证据校验。未发送
  整行text时，不能用隐藏行全文或跨cell拼接充当证据；已发送的整行原文仍可引用。
  规范原文及宿主偏移保持不变。
- locate新增受冻结PDF页清单约束的page_refs，按0基物理页补读全页及必要上下文；
  缺页、partial、OCR问题、整页超额均阻断。印刷页码映射不明仍未决，不靠正则或模型猜测。
- 新格式离线预检：4包、796原始范围、796处置、76模板行；完整原文覆盖缺漏、重复及
  错误区间均为0。239个表格行共805cell，172行省略整行text、67行保留兜底，所有cell
  原文与规范区间一致。Wire407944→400055字节（减少1.934%）；这是请求字节，非计费Token。
  最大输入估算110428/110592，规划与运行请求测量全部相同，输入全部准入；输出量与
  独立核验容量仍需真实结果验证。网络、模型及数据库调用均为0，源码指纹前后一致，证据为
  `/tmp/kb-v2-preflight-celltext-9ghbrybb/report.json`。
- 新run `c70e61d5-cd13-4b2a-afe3-665825d7f0fa`已failed：OCR2次、extract4次均succeeded，
  无sent/uncertain或HTTP失败，但只有1个extract-pack通过宿主校验落库。4包disposition合计
  796条完整覆盖；原始回包共315个事项，不能当作已接受事项或独立核验结果。
- 全量供应商usage：prompt111924 / completion130269 / total242193 Token，全部6次调用均报告
  用量；单次extract最大completion43709。16384输出预留不是wire上限，也不是实际输出预测。
  终态证据为`/tmp/kb-tender-v2-live-iaveude4/real-run-terminal-2026-10-15T073257.json`，
  observed_at为2026-10-15T07:32:57.298218+00:00。
- 旧镜像离线回放诊断定位92处纯空白引文差异，均可在各自range内唯一定位；另有2处
  dispositions各多声明1个item_id，以及1个numbers条目缺quote。未修改原回包或弱化校验，
  未重试付费。诊断在`/tmp/kb-tender-v2-live-iaveude4/live-extraction-schema-diagnostic.json`。

首次部署及此前验证事实：

- 2026-10-15 UTC，init、worker、dispatcher已统一更新为镜像
  `sha256:7e676a42033cd8a0c9a48dd8f7d6e5442425e19c987193c1f8278659585485bd`；init退出0，
  双常驻角色运行，容器内核对协议v2五节点、模型/分包并发均为2、PG与Redis实际连通。
  未新增宿主端口；保留原模型TCP中转overlay。旧run全快照未变，仍含2条uncertain请求。
  核验摘要在`/tmp/kb-tender-v2-live-iaveude4/deployment-verification.json`。
- verify已实现locate/associate/review固定子操作、全台账跨源候选发现、候选联合原文审阅、
  具体问题决议及部分补遗投影。宿主核对必要回执、覆盖和预算；独立接受的Decision才可
  解决其绑定问题，不能豁免宿主来源或执行事实。新路径的SupportingIndex词表只选候选
  上下文，不自动绑定或产生语义阻断；人工scope_decisions保留，scope审议不再要求理由
  命中特定词表，CLI按当前核验问题的真实范围受理。parser及旧独立工具的历史启发式仍保留。
- 完整Python回归 **896 passed / 15 skipped**，耗时407.35秒，使用隔离PostgreSQL/Redis
  和确定性模型。覆盖五节点确认、阻断、部分成功恢复、确认提交故障、纠错/来源修订、
  旧版本保护、完整输入预算及跨文档联合审阅；15项均因缺少历史或私有样本跳过。
- 本轮末次复核修复公共来源/事项引发的候选过度合并：仅共享同一issue_id才联合分组，
  单候选仍完整装载；对应回归已执行。同一句原文分片时，支持引文回到规范冻结来源，
  测试同时核对正文片段完整重组。Ruff、部署配置9项离线检查和文档检查通过。
- 新协议真实模型执行、真实文件五节点完成和人工标注质量验收均未完成；原有13项
  作用域未决、图片人工复核及旧uncertain请求仍保留，不能靠清空问题或伪造修订跑通。

本轮在[字段字典](field-dictionary.md#回包容错与继续核验)先明确职责与继续核验规则：模型判断
语义，宿主管理身份、原文和引用。抽取和关联共享受限LF恢复；处置item_ids由宿主派生；
不完整或无法追溯的可选numbers整字段省略并记录诊断，保留text/evidence及原始回包。
局部坏声明保留有效兄弟结果、完整原文和具体问题送独立核验，不新增自动付费修复循环。
宿主补出的待核对处置不冒充模型已判定的来源未决；业务疑点仍须独立核验或显式修订。
工作流执行身份增加normalization_version=1，避免旧派生回执混用；历史产物不重写。
最终源码和Prompt已离线重放原始4包：全部通过接收校验，保留315事项、37关系、796处置；
恢复92处引文中的137个源LF，2个事项省略整个可选numbers，重建2处处置索引。第二个
numbers问题为其引文无法在该事项已引用范围的实际发送文本定位，并非要求两条摘引互相
嵌套。模型原有14个明确unresolved仍保留为来源问题，另2个处置矛盾待独立核对；宿主
没有补造额外的受保护来源未决。原始回包/来源深比较及文件哈希不变，模型、网络、DB调用0。
预算复查使用当前Prompt和发送视图，4包输入估算107588/109873/110530/47760，全部不超
110592；最紧包余量62，后续Prompt或输入变动须由现有规划重新测量，不能复用旧预算描述。
完整请求Wire407944→401401字节；省去处置item_ids使最小输出结构74328→63184字节，
均为字节测量而非计费Token或实际输出预测。报告为
`/tmp/kb-final-protocol-VvBnef/{raw-replay-report,budget-report}.json`。

本轮回包容错最终完整Python回归：**1052 passed / 15 skipped / 0 failed / 0 errors**，共1067项，
426.74秒，退出0；隔离PG/Redis及确定性模型，无真实模型调用。跳过项均缺历史或私有样本。
168个Python/Prompt文件测试前后指纹一致；本轮12个修改Python文件Ruff及diff检查通过。
包括两项异常可选字段/旧式索引下的五节点验收，确认后到analysis-ready且无额外模型修复请求。
首次完整回归1037 passed / 5 failed / 15 skipped；5项均为合成重复填充叠加Verify提示增长
442字节后，完整joint输入超限50–114。仅调整3处测试调用的填充量，生产预算不变，现有
joint超限零发送负例仍通过。完整报告及前后指纹在`/tmp/kb-analysis-recovery-tests/`。

真实315事项的Verify初始计划也已离线测量，使用与真实4包ID一致的原始CAS副本，完整
保留37关系、796处置和54个Issue。review为20包，输入保守估算合计1115191，最大
110584/110592；locate为109包，输入估算合计9236641，全部准入。locate由10个基础块、
45个两两并集和54个逐问题定位包组成，每个事项在discovery中恰出现10次；每包全目录
重复发送，目录贡献约565万Wire字节，是当前累计输入的主要开销。这个规划没有发送模型
请求，也不等于核验完成；associate/joint仍须等待真实候选才能测量。不以新增门禁替代
后续运行，联调时须明确报告该累计开销。报告为`/tmp/kb-real-verify-budget-5tqBXk/`。
上述离线修复轮次未部署或重试付费；随后新建真实运行的状态见本文开头。旧failed和uncertain
不换身份盲目重放。历史v1语义样本仍须
迁移并经人工复核；合同测试、实际模型执行与人工语义验收分别报告，不把原始模型成功
响应等同于五节点成功。

## 现有部署与真实联调（2026-10-15 UTC，六阶段实现）

- 本地独立部署及真实模型调用已由用户明确恢复授权；下方历史记录中的“未部署/真实调用0次/暂停”
  仅代表当时轮次，不能作为当前状态。Rust 接入、正文生成、最终导出仍不在本期。
- 正式镜像已构建，`kb_python_tender_worker`、`kb_python_tender_dispatcher` 使用同一镜像运行；
  migrate 和一次性 init 已成功退出。容器命令使用 `--no-sync`，避免运行时下载构建依赖。
- Chat/VLM 已按用户要求独立配置；Python 不再读取 `KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES`，
  不从 Chat 补齐 VLM 地址/密钥/模型，不使用图片能力模型白名单。该变更后模型单测79项通过。
- 真实 PDF（106页，原件摘要 `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`）
  已进入 run `96cf4063-56ea-4466-a80b-aa7ed1177b45`，source_version=1、domain_revision=1。
  登记、解析/冻结及2次OCR完成；4个抽取包仅1个完整返回，另有4次HTTP429拒绝、2次uncertain；
  当前终态为 **failed**，独立Verify、人工确认、Plan均未开始，不能宣称真实端到端通过。
- 完整包返回35条要求、165条处置、6项模型问题，耗时418.63秒，实际输出22795 token。
  宿主另发现5处处置与要求证据不一致；有成功回执不等于业务已通过。
- 两份中断流离线核对末尾错误帧分别为 `request_timeout`（904.94秒）和
  `server_is_overloaded`（363.47秒）；均无完整JSON/结束标记/usage，继续保留uncertain防重发。
  HTTP429原因为 `GATEWAY_NORMAL_KEY_CONCURRENCY_EXCEEDED`，不是密钥缺失或额度结论。
- 模型报告的“第五章供货要求缺失”已在第3包找到对应章节，暴露跨包可见范围问题，不能据此
  判原件缺失。全源语义核对尚未完成；原有13项作用域未决和图片人工复核也未被豁免。
- 用户要求默认并发从4降为2：分包调度和模型客户端已同时修改、重建并部署，容器内实例化
  核对两者均为2；客户端/并发回归 **82 passed**。4条消息线程及其他应用仍可能共享模型Key额度。
  并发属于不可变execution_spec，旧修订不会静默换配置；未重跑旧失败任务或删除支付围栏。
- 当前镜像身份 `sha256:3cc842ee83ba48c4b16a613e9427e913825cde715ba7fec825e4c757d1efe9e2`；
  工作区尚未提交。最新定向回归不能替代下方较早的完整Python测试。
- 容器直连新建TCP连接仍不稳定，旧项目新连接也曾复现；底层网络原因未查实。
  本轮模型请求依赖临时宿主TCP中转，仅绑定 `172.19.0.1:8317`，透传TLS，不改模型URL。
  worker 的私有Compose overlay在 `/tmp/kb-workflow-start-g8vpogcy/model-relay.yml`，
  中转脚本同目录 `model-relay.py`；后续重建角色须保留该overlay，不能称直接出口已修复。
- 私有响应报告及完整/中断文本在
  `/tmp/kb-tender-real-ef9nvq8i/output/response-review/`；本次构建和定向回归证据在
  `/tmp/kb-workflow-start-g8vpogcy/build-concurrency2.log`、`concurrency-green.xml`。
  原件、业务全文、完整wire、密钥均不提交Git。

前一轮用户要求先修复差距，并指出评分、否决、技术、服务、售后、人员等只是业务例子，需要
重新核对完整 workflow。已完成[业务流程修订](business-workflow.md)，从来源到要求/规则/关系、
独立核验、人工确认、响应矩阵及独立计划审阅列出产物与放行条件。先统一这条业务主线并完成
合成纵切，再据新合同校准输出分包、跨包补读和显式付费恢复；不再仅按零散分类字段补丁推进。
**新增业务协议、关联决议、响应矩阵及计划审阅仍未实现**。旧自然语言启发式仍可能绑定或阻断，
不能靠删规则、伪造来源/作用域变更或清空问题取得成功。真实任务未重跑。
该轮包含响应规划的方向已被本文顶部的新范围取代，独立计划审阅不再是当前任务。

本轮另修复流式供应商错误被掩盖为 missing_done 的问题：安全记录 type/code，显式错误保持
uncertain，附带 DONE、诊断载荷损坏或响应头错误也不授权自动重付费。相关模型/流式单测
**139 passed，0 skipped**，不计为业务语义验收；私有报告
`/tmp/kb-workflow-start-g8vpogcy/recovery-green.xml`。该代码尚未重建部署，不能把当前容器
当作已包含此修复。

## 较早的隔离真实服务测试（2026-10-14）

- 已用现有 `deploy/.env` 启动本仓库的 `kb_postgres`、`kb_redis`，均健康且未发布宿主端口；
  未修改 `/opt/workspace/zhaobiao` 的旧栈。主 Compose 的 Python PG*/Redis 配置已通过实际连接检查。
- `.env.example` 已与共享配置及新端口一致，无需另配 TENDER 连接；实际 `.env` 的
  `KB_ADMIN_PASSWORD` 已补单引号，确认含 `$` 的密码按原字面值读入，不记录密码内容。
- 首轮完整 Python 测试为 715 passed、1 failed、15 skipped；唯一失败是镜像缺少 Calc，
  无法生成旧版 XLS。`deploy/Dockerfile.docreader` 已补 `libreoffice-calc-nogui`。
- 补齐依赖后的单项回归通过；完整复验为 **716 passed、15 skipped，309.11秒**。
  其中 workflow/acceptance 73项全部通过、零跳过；另有来源检查2项、部署配置检查9项通过。
  15项跳过全部是缺少文件样本，不能当作这些样本的验收成功。
- 测试在同一 PostgreSQL 实例的临时库及 Redis 15号库执行；临时库、账号、测试容器及
  队列已清理，PG/Redis 保留运行。报告在 `/tmp/kb-python-tests-zu7cbv56/summary.json` 和 `pytest.xml`。
- 上述完整测试使用确定性模型，代表该次隔离测试，不计为真实模型六阶段验收；当前进度见上节。

## 较早职责边界复核与六阶段待办（历史）

以下保留当时的审查与接手记录；当前范围和下一次实现顺序以本文顶部为准。

基于 `2ad480b` 完成[LLM 语义职责与宿主规则审查](semantic-review.md)，并同步 README、
领域合同和验收待办。业务语义由 LLM 审阅及必要的人工裁决确定；宿主保留结构、来源、
覆盖、身份版本、预算和执行保障。自然语言正则不能作为最终语义判断依据，不以继续
扩充比较词、动作词或作用域词表作为修复路线。

随后已准备[合成语义样本草案](semantic-cases.md)：13组配对、26个候选抽取，另有2个
跨文档联合审阅草案。agent 复核补齐表格单位案例的报价说明，并已准备
[Verify 离线导出、记录与人工判定流程](semantic-evaluation.md)：26份隔离输入、提示和语料
摘要、未运行记录模板；2个跨文档草案不纳入调用输入。标准库引用/覆盖及导出隔离检查
通过；预期待人工复核，未运行 Pydantic 合同校验、完整 pytest 或真实模型评测，不能据此
报告模型质量。

语义审查阶段只改文档、样本及离线检查，语义运行代码、提示和协议尚未调整；现有语言启发式仍可能直接绑定或阻断。
不将新职责目标写成已实现，不移除现有阻断，不新增真实模型调用，也不将下方历史测试
结果视为本轮语义验收证据。

用户随后明确：下一阶段优先把当前 PR 的真实端到端流程跑通。合成样本评测作为诊断及
回归材料，不要求先完成26例人工标注或新增计划审阅协议才启动真实全流程联调。

下一阶段的主线及可验证结果：

1. 恢复 Python 运行依赖、共享 PostgreSQL/Redis、持久产物目录及模型配置，验证既有
   CLI、worker、dispatcher 能运行；宿主连接使用本机端口。本执行器的 Python 运行依赖
   待装配，`deploy/.env` 已有通用部署及模型配置。按用户要求，Python 角色现已并入主
   Compose 的 `python-tender` profile，复用同一 PostgreSQL、账号和 Redis，无需另配
   TENDER 连接；运行时支持 libpq PG* 变量及 DATABASE_URL。启动顺序固定为共享库
   `migrate`、Python 初始化、worker/dispatcher，避免首次建表竞态。
   初次沙箱端口探测不能代表宿主状态：随后宿主检查确认旧 `knowledgebrain`
   栈健康运行，归属 `/opt/workspace/zhaobiao`。本仓库部署使用独立 `kb` 项目及端口，
   不接管旧栈；下方历史测试结果不能当作本仓库 Python 全流程已就绪的证据。
2. 取得真实原件并校验摘要，执行登记、解析、覆盖及冻结，检查实际分包输入和预算。
   逐项审议真实作用域及图片问题；历史13项未决不按测试需要清零，也不假定新环境已有原件。
3. 满足下方真实调用恢复边界后，用既有模型客户端及工作流执行抽取和独立 Verify。
   按实际回包审查后续完整输入，沿运行结果修复具体阻塞，保留审计与修订后重新验证。
   合成/FixtureModel 的成功不能计为真实模型全流程成功。
4. 提供实际来源、抽取及验证清单供人工确认；确认后执行 Plan，核查实际计划逐条落实
   当前适用要求，最终状态达到 response-plan-ready。未解决阻断不能通过确认豁免。
5. 保存真实 run_id、来源及修订身份、各阶段产物、模型调用/usage、确认及最终计划证据，
   验证中断后复用已成功回执；将本次发现的具体缺陷补成针对性回归。

这里的完整流程终点仍为本期响应计划，不扩大到 Rust 接入、正文生成或最终导出。
真实付费调用继续按下方明确授权边界执行。人工标注和系统性语义质量验收仍待完成，
与“已跑通一次真实全流程”分别记录；当前未实现计划生成后的独立 LLM 审阅。

## 433a464 实现交接与暂停边界（2026-10-14，以下保留原验证事实）

当前两项代码review已修复，并完成本轮确定性workflow、恢复、全量Python和真实原件离线
SDK wire复验。按用户最新要求，提交本交接后暂停，供父任务独立核对；不启动真实模型或新范围。
[Draft PR #24](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24)保持Draft，未合并、部署或切流。

- 分支：`feat/python-tender-phase1`；本轮起点fetch核对为`f78462ff956d62893f0909bd897071787f21f5f3`，无遗漏后续远端改动。
- 最终运行代码：`433a46475a528f089a4b6942767405c44d375c34`，已推送。最终交接为此文件所在提交，精确HEAD另记在PR描述及任务结束报告。
- 文档先行：`cc6185e`、`1b2ccf7`；解压不确定发送修复：`e64e1a942f948654c27e72e1f00f036377e6d958`；完整下游依赖与回归：`433a464`。
- main基线仍为`e8b681a0997760ec3f567223fe7eb82b622f5ed7`；Rust源码、Cargo、默认链路无diff。未改生产接入或增加第二状态机。
- 剩余额度没有可读接口，已阶段性push；真实模型调用0次并继续暂停。没有待审批或不确定执行动作。

## 433a464 当时方案与合同（历史）

1. 普通输入上限131072 token，显式纠错最多262144 token；两者均受provider总窗口、输出
   和安全预留约束。当前`context_tokens=131072`是总窗口，未扩大；输出16384、安全4096，
   有效普通输入110592。常规verify/plan及新领域修订不自动借纠错额度。不新增wire max_tokens。
   未知tokenizer用标明的UTF8保守估算，不冒充服务真实token或usage。
2. extract/verify使用共享SourceView；后阶段按当前修订同文档**全部extract包**的完整输入
   规划。verify加入全部合法requirements/dispositions，plan加入全部有效要求/约束及显式
   来源引用所需source_context。小章节合并；只有完整请求超限才按原生结构/语义依赖边界
   重分。同处置、完整义务和明确跨包依赖不可分；仍超限则明确预算阻断，零发送，不裁证据。
3. 要求、处置统一为`RequirementRef(artifact_ref, requirement_id)`与
   `DispositionRef(artifact_ref, range_id)`；阶段根`requirements_ref`只指完整索引。
   原文只在冻结来源保存；阶段计划仅持selector和引用，checkpoint仍是六个固定阶段引用。
   旧support证据身份与body重合时，`source_aliases`以无text的精确区间保留原ID，不重复原文。
   显式range依赖按canonical区间相交连接；非重叠片段和仅共享条件不强制绑定。
4. 来源覆盖、要求/处置、条件/例外、模板、修订身份均核对；未知、跨文档、歧义及已失效
   义务依赖明确阻断。verify/plan子包回执绑定完整输入、预算、修订和plan_digest；部分成功
   回执在恢复时复用，修订使旧确认失效。沿用既有LangGraph/PG回执，不增加通用框架。
5. 所有LLM任务stream=true；180秒连续**无原始数据**才idle超时，无总180秒截止。部分
   SSE/gzip解压失败记uncertain；审计写盘/提交故障保留sent围栏，同digest恢复不再付费。
   完整成功响应后的业务提交故障从已校验response_ref继续，缺usage保持unknown/null。

字段和语义以[字段字典](field-dictionary.md)、[合同](contracts.md)及[最终请求审查](request-review.md)为准。

## 最新验证事实（只代表433a464）

| 检查 | 本轮结果 |
| --- | --- |
| 完整Python3.11.16，真实PostgreSQL/Redis | **716 passed / 15 skipped，176.64秒** |
| CI另行Python source-captions检查 | **2 passed，0.05秒** |
| 新增完整下游实际workflow文件 | **12 passed，48.60秒**；已包含在完整套件 |
| 独立范围/别名/失效/歧义依赖回归 | **10 passed，2.02秒**；已包含在完整套件 |
| 流式/请求回放相关套件 | **124 passed，6.64秒**；已包含在完整套件，其中新增8项真实PG恢复用例 |
| Ruff（相关运行模块及unit/workflow/acceptance新增检查）、diff检查 | 通过 |
| wheel | 构建成功，98个Python/类型/提示文件与当前源码逐字节一致 |
| 固定SHA实际SDK离线wire | `runtime_dirty=false`；全部完整输入预算、引用/来源覆盖检查通过 |
| 运行代码远端Python专项CI | **486 passed，172.24秒**，wheel成功；真实PG/Redis，0skip |
| 运行代码远端通用CI的Python job | **642 passed / 89 skipped，48.30秒**；source-captions另2 passed；该job未配置同等真实服务 |

15个skip均为原有缺失fixture：Excel2、Markdown1、PDF3、PPT6、旧私有招标源合同1、
旧testdata/bid2。新增PG/Redis、workflow及恢复测试无skip；本轮真实私有PDF另行明确执行了
下述离线复验。首轮全量为699 passed/5 failed/15 skipped；5个失败均由新增fixture越过
预期预算或触发重复文档校验造成，修正fixture且完成独立发现的实现缺陷后重跑完整套件，
最终结果为上表716/15，不把首轮或旧684/146混作最新通过结果。

实际workflow证明：extract能容纳、合法输出加入后verify超限，跨extract包重新规划后全部
审核；人工确认后plan按其自身完整输入重分。verify及plan的不可分预算分别验证零发送；
两阶段部分成功后的恢复不重做成功兄弟包；同源修订撤销旧确认而冻结引用不变；条件alias
完整重建引用且发送原文不重复。范围别名、失效引用和命名空间歧义由独立审查补测。

wheel SHA256：`e79ca5bcd102770fdbb3a503f627ebcbab3ba54d0f179a3cb87a97fb1f98adea`。
本轮未重建Docker镜像；历史镜像不能称为当前代码镜像。
[当前Python专项CI](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/actions/runs/38545943835)通过；
[通用CI](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/actions/runs/38545943820)的Python/web通过，
Rust及依赖job显示cancelled，未宣称整个CI全绿，也不在此任务扩展运行/修复Rust。
以上均绑定运行433a464；最终纯文档提交触发的后续流程应另核对，不能混为同一SHA结果。

## 真实原件与最终发送审查

当前Library支持方式重新materialize了`libfile_5be1d1c487b88191aed7fe91a0d6ec1f`，
`BiddingFile(1).pdf`，755602字节，SHA256：
`4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
本轮真实重解析得到106页、520单元、796范围、42表/318行；来源覆盖通过。98图片位置
绑定两份实际像素，重新作agent直接视觉审阅，`human_review_completed=false`；原件及审议
记录仅本地私有。13项`condition_scope_unresolved`仍在11个页面，未清零。

最终wire证据绑定运行433a464，实际AsyncOpenAI序列化、本地MockTransport捕获，网络/
模型调用0。622条明确synthetic要求及796处置用于完整输入压力测试，**不是实际抽取结果**。
原4个完整verify估算297565/320147/315232/142166，全部超过110592；新结果：

| 阶段 | 请求数 | wire字节 | 最大输入保守估算 |
| --- | ---: | ---: | ---: |
| extract | 4 | 402371 | 110413 |
| verify | 13 | 1186694 | 110517 |
| plan | 7 | 695530 | 110538 |

全部stream=true、无max_tokens；796处置保留，698交模型审核、98宿主单列。622要求各阶段
完整覆盖，条件/例外/跨包source_context保留，extract/verify正文均159186原始UTF8字节。
同请求body/support重复字符0；必要表结构、空模板身份、prompt/schema保留。压力样本
全文quote不可当最小生产引用建议；24请求也不能当真实执行次数预测。

- [新真实结构证据](evidence/real-pdf-downstream-structure.json)
- [最终完整下游wire证据](evidence/request-wire-downstream-synthetic.json)，文件SHA256：`256aa3cb9794a75230a6bbe5ca74d119f6f638b594bf4ac0ffc460a20e254482`
- [字段必要性与局限](request-review.md)；原件正文、完整wire及图片没有提交到GitHub。

## 433a464 当时剩余状态与执行顺序（历史）

当前两项代码缺陷和已安排离线验证完成；PR仍Draft，真实语义验收**未完成且仍暂停**。
后续只有再次收到授权后才执行：

1. fetch核对PR24最新HEAD，读取本交接、AGENTS及可用相关skills，保留任何后来修改。
2. 新执行器通过当前Library支持的materialize方式取得上述file ID，校验大小和SHA；不能
   假设本机私有路径在新执行器存在。若不可读，只做可复现合成回归，明确未作真实文件复验。
3. 对13项scope逐项准备带来源证据的决议并复验，不能按测试需要清零；人工复核图片判定，
   准备人工标注集及验收标准。零基页10、15、21、64、66、67、68、76、77、84、88存在待办。
4. 取得实际provider总窗口/能力证据，维持输入预算和输出/安全预留；不得以付费探测猜容量。
5. 仅在明确恢复真实模型调用的授权后开展真实语义验收。extract前审查最终wire；真实输出
   形成后，再审查实际verify/plan完整wire、必要重分、依赖与成本，逐阶段准入后才付费发送。
   13项未决、人工标注和真实模型结果不能由本轮synthetic通过替代。
6. Rust API/outbox/权限/事务/生产接入及切换另行授权和验收；本任务不继续这些范围。

## 环境、复现与私有产物

仓库AGENTS已读；`/workspace/.agents`为空，仓库无相关`.agents/skills`，不假装已读。
当前本地私有输入：`/workspace/private-tender-acceptance-current/BiddingFile(1).pdf`。
私有结构/新图片审议：同目录`downstream-preflight/`；最终完整wire：`downstream-wire/`。
新环境须通过Library `prepare_materialize`提供的授权方式自行获取，下载地址不写入文档。

本地隔离测试容器`kb-tender-budget-postgres`、`kb-tender-budget-redis`保留，端口15432/16379；
测试临时schema在结束后清理，无生产库改动。Python路径为`services/docreader/.venv311/bin/python`。
最终全量日志`/tmp/kb-downstream-full-final.log`，测试产物`/tmp/kb-downstream-full-final/`；
wheel在`/tmp/kb-downstream-final-wheel/`。复现（从services/docreader执行）：

```bash
export TENDER_TEST_DATABASE_URL=postgresql://tender_test:tender-test-local-only@127.0.0.1:15432/tender_phase1_test
export TENDER_TEST_DSN="$TENDER_TEST_DATABASE_URL"
export TENDER_TEST_REDIS_URL=redis://127.0.0.1:16379/1
export TENDER_REDIS_URL="$TENDER_TEST_REDIS_URL"
.venv311/bin/pytest tests -q -rs --tb=short --basetemp=/tmp/kb-downstream-reproduce
PYTHONPATH=.. .venv311/bin/pytest ../../scripts/tests/test_bidding_source_captions.py -q
UV_CACHE_DIR=/workspace/.cache/uv UV_TOOL_DIR=/workspace/.uv-tools uv tool run ruff check llm tender workflow tests/acceptance tests/workflow tests/unit/test_downstream_dependencies.py
UV_CACHE_DIR=/workspace/.cache/uv uv build --wheel --python .venv311/bin/python --out-dir /tmp/kb-downstream-wheel
.venv311/bin/python tests/acceptance/review_downstream_wire.py \
  --source '/workspace/private-tender-acceptance-current/BiddingFile(1).pdf' \
  --runtime /workspace/KnowledgeBrain \
  --private-root /workspace/private-tender-acceptance-current/downstream-preflight \
  --output-private /workspace/private-tender-acceptance-current/downstream-wire \
  --report /tmp/kb-downstream-wire-reproduce.json
```

wire脚本读取上述新真实解析产物；新执行器还需先按`tests/acceptance/prepare_real_pdf.py --help`
重建私有预检与图片来源绑定审议，不能把缺失私有文件当作可复现。保持原件私有属性与权限。
历史`f78462f`/运行`1480efc`只通过146项且两项review未完；其完整记录在Git历史，不代表当前
结果。以下5aa6b84/运行51d7ec3的684/15skip、454专项CI和镜像证据也仅保留历史原义。

## 以下为5aa6b84历史交接，保留原义（不代表本轮验收结果）

本文件是当前安全保存点；保持 [Draft PR #24](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24)，不合并、不部署、不切流，付费模型请求继续暂停。
环境未提供可读的剩余额度接口，无法监控10%阈值；采用阶段性提交，避免只保留本地修改。

## 身份与当前结果

- 分支：`feat/python-tender-phase1`。
- 最新只读fetch的main基线：`e8b681a0997760ec3f567223fe7eb82b622f5ed7`。
- 最终运行代码：`51d7ec322e7ac09cb6c05043ccb07e8ecf03d76c`。
- 最终镜像/实际wire证据：`41c772812afeef32f0553e952446d3808b69466d`。
- 当前文档保存点是此文件所在提交；代码未在上述实现之后改动。
- 独立Python阶段1–6及确定性验收完成，终点`response-plan-ready`；真实模型语义验收未完成。
- Rust源码/Cargo/默认执行链路diff为空，旧实现未删除；未来Rust API/outbox/事务合同不表示已集成。

## 已完成

真实Redis/PostgreSQL骨架先验收，再完成来源登记、原生解析与真实图OCR适配、覆盖/冻结、
结构分包、并行抽取、独立全来源验证、人工确认、响应计划和独立CLI。支持租约/围栏、
重复消息、claim token、失联补投、持久interrupt释放worker、心跳取消、源版本及域修订、
完成回执恢复、部分包隔离和恢复配置固定；使用共享严格schema，无未发布旧格式兼容层。

最终发送视图只投影必要语义：正文、来源ID/片段、共享章节、表结构/公式/部分空字段/实际图片、
范围条件及目标；模板逐原行处置，宿主审议和模型审阅分开。实际SDK wire审计由本地transport
截取。未知tokenizer采用decoded-message UTF8保守估算，HTTP转义单独计字节，不冒充usage。

全部模型任务已stream=true；180秒原始接收idle，无总180秒截止。request_digest绑定完整无鉴权
请求，真实PG发送围栏阻止sent/uncertain换ID重付费；成功response_ref校验后复用。收到部分流
保留单份审计、usage unknown，无最终业务成功则不能产生成功领域产物。

## 测试与证据状态

| 检查 | 结果及边界 |
| --- | --- |
| 完整Python3.11真实PG/Redis回归 | 684 passed、15 skipped，142.69秒；新增服务测试无skip，原有可选测试skip |
| 定向phase1/增长/共享schema | 46 passed，48.05秒；之后追加严格SourceView用例已纳入全套 |
| Ruff、git diff --check | 通过 |
| wheel | 构建成功，48运行模块/提示与源码逐字节相同 |
| 镜像 | 构建成功；Python3.11.17；48文件与51d7ec3一致；容器CLI真实PG初始化通过；默认gRPC CMD不变 |
| 真实106页PDF离线预检 | 520原生单元、796范围、4包；原文/结构/图片ref不变；13范围未决仍阻断 |
| 最终SDK wire | 抽取1,021,405→402,371字节，减少60.6%；正文159,186字节不变；不是provider token |
| 2倍/4倍增长 | 文件产物1.980/3.941倍；checkpoint约18.9KB、6阶段引用，非平方增长 |
| 真实LLM、人工标注召回 | 未运行/未完成；真实模型调用0，usage=null |
| Rust测试 | 本阶段未运行/未修；已知main历史500passed/2failed/24ignored不能归因本PR |
| GitHub Actions | 运行代码51d7ec3的Python tender standalone已通过：454 passed / 83.88秒，wheel成功；后续纯证据/交接提交的流程尚在运行，接手核对 |

[运行代码的远端验收](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/actions/runs/38512688636)已确认成功；与本地全套684项口径不同，远端只跑unit/workflow/acceptance。

首轮新增失败断言曾误把“队列执行回执completed且业务failed”写成command failed；已修正测试并全套通过。
镜像验证脚本曾把原parser导入日志与JSON合并解析而报JSONDecodeError；修正只读诊断解析后核验通过，
不是镜像运行失败。此前构建磁盘不足已清理本任务旧镜像/缓存并重建成功；无需清理共享服务或数据。
当前没有未修复的本地测试失败。原有15个skip、真实语义及远端未完成状态保持单列。

公开统计/哈希/逐包比例/边界/字段必要性/镜像和增长证据见
[request-review.md](request-review.md)、[acceptance.md](acceptance.md)、[evidence](evidence/)。
旧sample-*.json、旧镜像与旧21/13包证据保持历史原义，不替换成新模型输出。

## 私有产物与环境

以下仅是工作区位置；文件正文、图片、请求原文和密钥不得加入Git或发给新接收方。

- 原件：`/workspace/private-tender-acceptance/BiddingFile(1).pdf`，755602B/106页，SHA256
  `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
- 当前canonical与4包：`/workspace/private-tender-acceptance/python-phase1-wire-preflight/`；
  `frozen-index-ref.json`指向aggregate PackPlan，`artifacts/`为内容寻址产物。
- 修复前13包：`/workspace/private-tender-acceptance/python-phase1-final-preflight/`。
- 实际SDK发送JSON仅在：`/workspace/private-tender-acceptance/python-phase1-wire-before-requests/`
  和`python-phase1-wire-after-requests/`；有私有权限/原件属性，禁止上传。
- 图片审议输入：`/workspace/private-tender-acceptance/python-phase1-preflight/image-dispositions-reviewed.json`；
  98出现位置/2份内容经视觉审议，结论只对该原件hash有效，不能按重复/大小自动推广。
- 最终pytest文件产物：`/tmp/kb-phase1-wire-final/`；临时schema测试后已删除。
- Python：`services/docreader/.venv311/bin/python`；系统默认3.12不能替代3.11验收。
- 本地PG：127.0.0.1:15432；Redis：127.0.0.1:16379/1。使用独立测试库/临时schema。
- 本地镜像：`knowledgebrain-docreader:python-tender-phase1-review`，
  `sha256:8769713667a41d1810e351b47a29f49b074c1a8acf15e51102895bb7010d3032`。
- 构建/核验辅助脚本：`/tmp/kb-build-latest-image.py`、`/tmp/kb-verify-latest-image.py`；
  日志`/tmp/kb-tender-latest-image-build.log`。仅临时CA secret mount解决代理信任，TLS未关闭。
- 修复前运行代码隔离worktree：`/tmp/kb-before-view-runtime`，基于82678ac，仅用于重放成本审计。

## 复现命令

在仓库根核对当前状态及远端，不将密钥环境完整打印：

```sh
git status --short
git rev-parse HEAD origin/main
git diff e8b681a0997760ec3f567223fe7eb82b622f5ed7 --name-only -- '*.rs' Cargo.toml Cargo.lock
gh pr view 24 --json url,isDraft,headRefOid,statusCheckRollup
gh run list --branch feat/python-tender-phase1 --limit 6
```

在`services/docreader`，设置本地测试配置（这些是隔离测试服务固定凭据，不是模型/生产秘密）：

```sh
export TENDER_TEST_DSN='postgresql://tender_test:tender-test-local-only@127.0.0.1:15432/tender_phase1_test'
export TENDER_TEST_DATABASE_URL="$TENDER_TEST_DSN"
export TENDER_TEST_REDIS_URL='redis://127.0.0.1:16379/1'
export TENDER_REDIS_URL="$TENDER_TEST_REDIS_URL"
.venv311/bin/pytest tests -q --tb=short --basetemp=/tmp/kb-phase1-reproduce
uv build --wheel --python .venv311/bin/python --out-dir /tmp/kb-phase1-reproduce-wheel
```

离线预检（输出私有数据到新目录；不要覆盖旧证据）及wire重放：

```sh
.venv311/bin/python tests/acceptance/prepare_real_pdf.py \
  --source '/workspace/private-tender-acceptance/BiddingFile(1).pdf' \
  --private-root /workspace/private-tender-acceptance/reproduce \
  --report /tmp/kb-reproduce-preflight.json \
  --expected-sha256 4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc \
  --context-tokens 131072 \
  --image-dispositions /workspace/private-tender-acceptance/python-phase1-preflight/image-dispositions-reviewed.json
.venv311/bin/python tests/acceptance/review_wire.py \
  --source '/workspace/private-tender-acceptance/BiddingFile(1).pdf' \
  --runtime /workspace/KnowledgeBrain \
  --private-root /workspace/private-tender-acceptance/reproduce \
  --report /tmp/kb-reproduce-wire.json \
  --output-private /workspace/private-tender-acceptance/reproduce-wire --label after
```

## 剩余项、阻塞与下一步

1. 优先查看最新远端CI；若只有已知Rust基线问题，不越界修Rust。若Python失败，按真实日志修复并重新保存。
2. 私有原件仍有13项范围/引用/优先级绑定审议，不能一概叫文档歧义，也不能confirm清空。
   使用带原文证据的`review-scope`生成新domain_revision，重新分包/抽取/独立验证/确认。
3. 人工标注集及真实LLM语义验收未运行。付费调用是**明确暂停**，不是权限拒绝或已失败；
   先向父任务报告输入、endpoint/model和估计范围取得继续确认。已有配置指向
   `https://cpamc.818996.xyz/v1/chat/completions`，model `gpt-6-luna`，C=131072；禁止打印key。
   当前无重试/重分假设为4抽取+4验证+0..4计划（8..12次），实际用量未知。
4. verify的成本目前只是来源下界，加入真实requirements/dispositions后可能超限；必须重新准入。
   超限会明确阻断，不裁证据、不扩大服务C；必要有界重分需保持依赖/覆盖并记录设计判断。
   当前未实现通用自动后阶段重分框架，也没有把这个未知预算写成验收通过。
5. 达到独立真实语义验收后，未来Rust接入仍需另行授权/验收。knowledge材料检索、正文生成、
   成稿审查及最终导出不在本期，不增加空壳模块冒充完成。

没有待执行的破坏性操作，也没有不确定的Git推送结果；此前41c7728推送及PR正文更新已确认成功。

## 2026-10-28 UI交互收尾（独立验证，不修改真实全流程任务）

- 排队状态统一为pending/published/running，保留取消及同键未知请求确认，禁止另建重复任务。
- 首稿明确“正文已生成，查看Word”；大纲未保存调整保留，首次查看前不隐藏初始化Office；
  后台新稿继续由用户显式选择，不替换当前Word。
- Word附近提供保存稿内容复核和生成稿修订入口，业务版本名优先；保存新版本仍保留旧版
  报告与理由，入口明确要求进入后确认复核版本，不自动切换固定依据。
- 下载明确只包含已确认保存的版本；dirty/syncing/saving/unknown/pending均显示边界。
- 分析/大纲/正文/保存稿复核显示真实阶段及回执，当前来源/领域修订隔离；不估算百分比，
  不把收到模型返回当已接受单元，同逻辑请求重试按最新状态统计。新增模型调用0，新增DB
  查询及artifact读取0，不改模型协议/Prompt/预算。
- 最终前端build/lint、67项单测、107项浏览器回归通过，覆盖1440/390px、300条事项、
  80个长章节及合法书签实际定位；两类复核表单不覆盖Word工具栏。真实Office截图补查后
  编辑区保底420px，工作区允许纵向滚动；核对栏抽屉包含自身关闭入口，桌面缩窄后
  可实际关闭并点击下载。旧视口检查改为先确认项目操作可见，再滚动显示完整编辑区，
  保留1px浏览器舍入容差；未以隐藏编辑器或缩小正文换取测试通过。
- 后端单轮209个唯一用例全部通过，无失败、错误或跳过，相关25文件Ruff通过；独立后端
  回归数据库已删除。当前未部署、未提交或推送，也未运行付费模型或读取真实招标文件。
- UI证据：`/opt/workspace/kb-tender-evidence/2026-10-28-ui-interaction-fixes/`，
  `frontend-accepted.log`及`frontend-accepted/`含最终浏览器结果和桌面/窄屏截图；失败回归
  分别为`red-download.log`、`red-guidance.log`、`red-review-target.log`、`red-office-layout.log`。
- 后端证据沿本轮代理原目录保存：
  `/opt/workspace/kb-tender-evidence/2026-10-28-complete-web-fixes/backend-final-all.xml`、
  `backend-final-summary.json`、`backend-ruff-final.log`及`backend-final-cleanup.json`。
- 真实Office8.3.3独立验收通过（1项，18.4s）：实际载入/目录定位→输入唯一文字→未保存
  下载仍为旧版→显式保存/真实回调发布新版→实际DOCX含新文字且摘要变化→原版本不变→
  再次定位；390px正常关闭核对栏及目录后实际点击下载，所得字节与新版完全一致。
  宿主另行核对4份DOCX的摘要、实际XML文字及新旧版本不变性；数据库关联回执为saved，
  pending_save=null，两类模型请求均0。Office首次“知道了”介绍按真实路径关闭，无force。
- Office证据在UI目录`office-live/`：`playwright-layout-final.log`、`browser-layout-final/`、
  `acceptance.json`、`save-receipt-evidence.json`及`cleanup-evidence.json`。仅使用新建合成
  项目和新凭据；4个容器、8个匿名卷、独占网络/API已精确清理，32850～32854和4175释放。
  不影响另一个真实全流程任务的59463/59873、数据库或证据。本轮真实Office通过不代表
  真实招标/企业资料/付费模型的联合语义验收通过；该任务仍由原任务独立维护。
- 本轮最终汇总为UI证据目录`verification-final.json`，包含测试数量、Office/清理回执及
  已验证源码摘要；4184预览端口也已确认释放。没有待清理的本轮服务或待确认保存操作。


## 2026-10-28 UI异常边界修复验收（独立复核）

- 多入口读取按最新请求序号接收，在提交开始时失效旧读取，成功与错误都受围栏保护；
  同一分析/恢复身份的最新状态控制新提交，保留本地意见、理由和已冻结的未知请求。
- 已结束项目拒绝新编辑、切换当前稿及新可执行任务。Rust结束项目与Word编辑/保存按
  项目行串行，活跃编辑或待保存时提示先保存关闭；Python最终提交先锁项目，再以同
  连接写入run/项目映射、job或分析command。既有同key同body回执、来源只读确认、
  读取、下载、关闭和取消保留；分析二次准入发现结束时在旧版本业务校验前精确回放。
  不可变artifact/source_only准备资料仍可保留，不宣称项目结束后所有准备资料零写入。
- Word打开未知结果按稳定用户身份和本标签页保存原body、语言、模式与key，刷新后
  显式确认原请求；不关闭其他标签页会话。关闭/切模式先锁输入、失焦并排空已排队
  状态事件，回执未知时保持锁定，重试同editor_key。迟到修改保留iframe，通过原生
  “文件→下载为→DOCX”导出恢复稿后显式关闭，签名配置允许download，不虚报平台已保存。
  卸载不复活旧实例，预检期间卸载释放本组件清洁会话，已发送close不重复发送。
- 恢复任务pending/published覆盖旧execution.failed显示，保留同执行身份和已接受回执；
  恢复排队期间不放开重复生成按钮，既有5秒轮询继续刷新。
- 最终前端80项单测、130项完整浏览器回归、build及完整lint通过。新增19项状态回归中
  18项在旧dist失败，保存稿读取排序一项旧dist也通过；4项Word浏览器边界另行覆盖。
  预检卸载RED是明确标注的修复前条件隔离回放，完整GREEN运行当前真实代码。
- 最终Rust API单轮34/34通过，显式执行全部真实PostgreSQL/认证gRPC夹具用例，0忽略；
  含结束后上传回执、保存稿复核回执、Word生命周期和保存原子性。Python最终单轮
  213/213通过，317.457s，0失败/错误/跳过；XML身份核对原147全部覆盖、新20项、
  原生Store46项。Ruff/AST 7文件通过；旧209项3个测试替身失败的轮次不是最终结果。
- 真实Office8.3.3最终冻结源码复验1/1通过（34.0s，0跳过）：合法书签实际定位→输入→
  未保存下载仍为旧版→显式保存/真实回调发布→下载实际DOCX含新文字→历史版本不变→
  390px实际下载→返回阅读释放后台key→再次编辑→正常关闭、iframe销毁且session清空。
  独立核对4份DOCX摘要/XML标记和3个历史/当前版本HTTP下载；两次save均saved，
  pending/session均null，未关闭session0，模型回执0。首轮证据保留，最终只认冻结后复验。
- 本轮未部署、未提交或推送，新增模型调用0；全部使用新建隔离数据及合成Word。
  真实招标/企业资料/付费模型联合语义验收由原独立任务维护，本轮不替代其业务验收。
- 本轮证据根：`/opt/workspace/kb-tender-evidence/2026-10-28-ui-edge-fixes/`。
  总回执`verification-final.json`含数量、边界及源码摘要；前端`frontend-final.log`、
  `frontend-unit-final.log`，Rust`rust-api-final.log`，Python`python-project-end-complete.xml`
  及`python-project-end-final-summary.json`，Office`office-live-new/verification-final.json`
  与`office-live-new/logs/office-live-final.log`。失败证据与首轮结果保留，不以旧进程或局部补跑冒称最终。
- 清理完成：本轮6容器、10匿名卷、1独立网络及API/预览已精确释放；32850～32854、
  32793、历次bridge端口及4175/4184均无监听。回执`backend-cleanup-final.json`、
  `office-live-new/cleanup-final.json`；未触碰并行真实任务的进程、数据库或证据。
