# ONLYOFFICE 与产品后端真实联调

## 2026-09-09 目录计算、保存书签与实际 PDF 页码

现有验收入口增加 `--update-toc`（须同时使用 `--web --pdf`），通过编辑器引用工具栏的正常“更新目录”按钮，在每次修改后、保存前刷新整个原生目录。未调用内部编辑方法，也未在生成器填写页码。读回持久化 DOCX 时按实际 OOXML 标题层级解析，允许 Office 重编号样式 ID；检查目录标题/顺序、数字页码及其书签指向实际正文标题。随后沿用同稿转换，检查 PDF 指定页含目标章节文字，并保留独立目视核对。

最终完整真实目标 **1 passed、0 failed、0 ignored、0 filtered、exit 0**，历时 98.88 秒，临时服务清理 exit 0；最终脚本与运行快照逐字节一致。两次 forcesave、最终保存及再次重开编辑后均保留计算结果。两页合成模板的目录显示点线与页码 **2**，第 2 页实际显示对应章节及两列表；PDF 两页均经共享 Python 服务渲染并实际查看。原始引擎输出另做正例及三项破坏检查：缺页码、错误书签、正文标题改写均被拒绝。证据见 [`artifacts/bid-full-sample/toc-pdf/`](../../artifacts/bid-full-sample/toc-pdf/)。

此前页面加载失败、验收器误用原样式 ID、末尾故障页未载入三轮错误均保留。验收工具只在确切 `ERR_NETWORK_CHANGED` 且未发出编辑器打开请求时允许一次新页面导航；不重试编辑或保存。独立的应用状态读取故障页检查移至停服务前，避免与停 Docker 的接口变更混在一起；真实签名回调仍在服务已停止后重投，所有原断言保留。

**边界：** 本轮只证明默认连续页码、单节、两页生成模板的目录更新和同稿持久化能力。PDF 指定页出现标题只是机器检查的一部分，不能取代原页和版式核对；多节页码重起、罗马页码、跨多页目录以及真实招标整稿尚未验收。产品 O2 导出工作流仍待实现，下节“目录无页码”指此前未执行更新的历史样稿。

## 2026-09-09 同一保存版本的 PDF 验收工具

现有 `scripts/onlyoffice_product_probe.py` 增加可选 `--pdf`。在实际 App 编辑、两次 forcesave、最终保存及新 key 重开后，直接用该重开会话所绑定的不可变 DOCX 调用 ONLYOFFICE Conversion API。转换使用独立随机 key；前后均核对受控来源字节、产品下载、对象摘要及当前版本不变。PDF 经统一 `services/docreader` 回读，保存的三个随机标记逐字符保持，仅忽略 PDF 解析产生的排版空白；原始回读 JSON 保留。后续编辑保存、历史版本及停服后的真实回调重投检查仍执行。

最终脚本在专属 PG/Redis/ONLYOFFICE 中完整 **1 passed、0 failed、0 ignored、0 filtered、exit 0**，所有临时服务清理 exit 0，脚本摘要与该轮运行快照一致。使用的是此前生成验收的本机合成模板，所得 PDF 为 2 页；两页已通过共享服务渲染并实际查看，三个保存标记、中文标题、章节与表格可见。此前两次 `ERR_NETWORK_CHANGED` 页面加载失败、一轮 PDF 标记空白比对失败及另一轮成功均保留，未跳过失败断言。Docker 网关从已启动容器的网络字段读取，诊断不输出整份容器环境。

证据：[`artifacts/bid-full-sample/same-version-pdf/`](../../artifacts/bid-full-sample/same-version-pdf/)。`live-final-retry/pdf-source.docx` 与 `same-version.pdf` 由 `pdf-result.json` 绑定摘要；`pdf-source-after.docx` 是转换后的再次取回。没有归档环境文件、来源票据、服务凭据或原始容器运行日志。

**限制：** 这是同稿转换的验收工具和实证，不是 O2 产品导出流程已实现，也不是实际招标整稿验收。该合成 PDF 的目录缓存尚无页码；实际 Office 目录域更新、保存后的页码核验及整本版式仍须完成。未新增解析器、业务章节规则或 migration。

工具：[隔离运行器及浏览器驱动](../../scripts/onlyoffice_product_probe.py)、[Rust TCP 测试目标](../../crates/api/tests/docx_native.rs)。此测试将真实 Document Server 接到 `api::router_with` 的 HTTP 路由，使用真实 PostgreSQL、Redis 和本地对象文件；没有增加测试 HTTP 端点，也没有改写编辑配置的签名字段。

## 运行边界

显式开启 `docx-native-tests`，缺少前置条件时失败，不忽略测试。运行器要求本机缓存的 Document Server digest 镜像、PostgreSQL/Redis 镜像、Rust 工具链与依赖、安装 Playwright 的 Python 及浏览器路径。所有样稿、部署路径、镜像和端口均由参数或本轮环境提供；身份、密钥和资源标签随机生成，没有新增产品配置默认值或 migration。

```sh
"$PLAYWRIGHT_PYTHON" scripts/onlyoffice_product_probe.py \
  --image "$ONLYOFFICE_TEST_IMAGE" \
  --postgres-image "$POSTGRES_TEST_IMAGE" \
  --redis-image "$REDIS_TEST_IMAGE" \
  --sample "$ONLYOFFICE_TEST_DOCX" \
  --browser "$PLAYWRIGHT_TEST_BROWSER" \
  --cargo "$TEST_CARGO" \
  --evidence-dir "$ONLYOFFICE_TEST_EVIDENCE"
```

运行目录须位于系统临时目录下。工具创建独立 Docker bridge 和带本轮 ownership label 的服务，PostgreSQL、Redis、Document Server 仅向 host loopback 发布随机端口。产品测试 listener 只绑定该 bridge 的动态 gateway，便于 Document Server 回调；不监听宿主全部接口。`ALLOW_PRIVATE_IP_ADDRESS=true` 仅设置在该隔离 Document Server，允许它读取测试 API 源地址。

三 baseline 和提交后的集合 fixture 仅安装到新建的 `knowledgebrain_test_*` 数据库。管理连接只用于 fixture 查询和验收，HTTP 请求使用实际 `kb_runtime_api` 连接。浏览器 JWT 经本轮私有文件传递；服务密钥只进入临时权限为 600 的 env 文件和进程环境。运行结束删除这些文件及对象工作目录，按 ownership 核对并删除本轮容器、匿名卷和网络；保留编辑过的样稿副本、结果与清理收据。

## 断言范围

1. 将真实样稿副本经 multipart 产品路由发布成新轮，使用产品签名配置打开编辑器。
2. 在同一 key 下两次插入随机标记并经产品 `/editor/save` 触发 forcesave。每次等待 current 推进、pending 清除，核对下载文件、对象文件及 SHA256，打开基线始终保持原稿。
3. 再次编辑后关闭浏览器页，等待真实最终回调发布并清除活动 key、基线与 pending；最终 DOCX 包含最后修改，前一次 forcesave 文件不包含它。
4. 以新 key 从持久化最终文件重开，再编辑并关闭保存；所得 DOCX 必须保留所有此前标记及新标记。另核对两份历史 forcesave 下载 bytes 没有变化，成功保存未遗留 staging 行。
5. 完成编辑后停掉本轮 ownership 核对通过的 Document Server，确认其 healthcheck 已不可达，再主动重投其实际签名的两条 forcesave 和两条最终保存回调。四条都应确认成功，完整 current 以及对象 bytes/修改时间保持不变。
6. 可选 `--config-ttl` 测试参数：打开后等配置到期再编辑。源地址无服务签名应拒绝，使用已捕获的有效服务读取签名仍能读取同一基线，后续保存仍成功；不更换活动编辑器的配置或回调地址。

这覆盖真实服务与产品路由的读写及版本发布。浏览器入口是测试驱动生成的最小页面；编制产品前端、API 可执行文件完整启动/发布验证、S3、字体及最终版式、生产许可、网络断线与自然回调重试不在本测试范围内。权限、故障和乱序继续由[后端 HTTP 契约](docx-editor.md#验证范围)提供独立证据，不能声称所有故障都已在真实 Document Server 重现。

## 本机实跑结果

使用 Community **9.4.0-129**、Chromium **151.0.7922.34**，Document Server 完整镜像身份为 `onlyoffice/documentserver@sha256:e3da62a847b9a5d51a11f73cfea1d9c13c3be3809614490d4edddcf01dcf919b`。样稿为 `testdata/bid/60adc38f81464a0a93898eb435b506d6.docx` 的副本，原稿 SHA256 `4a7c68d8c20688a12f7e978e97abbbd5b2cec116728ad3f3f639558c8091b47d`，运行后原稿保持不变。这些是实跑记录，不是工具默认配置。

最终目标 **1 passed、0 failed、0 ignored、0 filtered、exit 0**，实测 41.26 秒；该测试目标 clippy `-D warnings` 通过，运行与资源清理均 exit 0。两次 forcesave 后同轮 revision 为 2/3，最终保存为 4，新 key 重开再编辑后的最终保存为 5。四次发布均核对实际 DOCX 的标记、摘要、对象 bytes 和下载 bytes；历史 forcesave 文件随后仍保持原 bytes。产品 source/callback 请求的脱敏 HTTP 记录均为成功响应，没有放宽签名、下载 origin 或数据库写入约束。

证据目录 `/tmp/knowledgebrain-docx-native.cxmyhric/` 中，`run-2b6cbe8cc9b5401abf0065a82f65e2e0/` 保存通过时的源码副本、三 baseline、fixture、`result.json`、`http-events.jsonl`、`native-test.log`、四份保存 DOCX、`reopened.png` 及清理收据。`run-d498866fcf954bf48e7f983c85b3a53a/` 和 `run-fe3ce60908564b18b7fbb49d69e742ec/` 为打开阶段失败记录，两轮同样清理 exit 0。临时证据不是永久发布资产。

前两轮暴露测试页面的网络来源问题：不安全的 bridge-origin 页面加载 loopback Document Server 脚本，被 Chromium 的地址空间策略拒绝。只读对照确认相同脚本从 loopback 页面正常加载；最终将浏览器本地测试页面也放在 loopback origin，不关闭浏览器安全校验，也不代理或改写产品签名地址。编制前端实际部署仍应验证其自身 origin/HTTPS 网络条件。

浏览器日志还保留部分 Document Server 图标资源加载失败；重开截图可见协作显示名提示，当前后端只提供用户 id。它们未阻止本次正文编辑与持久化断言，但不能据此宣称 UI 无错误或前端体验已验收。用户显示名应在 O1-W 使用真实账户资料接线，不能写死测试用户名。

## 已完成回调恢复实测

修复缓存失效恢复后，同一镜像、浏览器与原稿副本完整重跑原编辑链，再执行上述停机重投：**1 passed、0 failed、0 ignored、0 filtered、exit 0**，实测 55.20 秒，run/cleanup exit 0。`result.json` 记录 `authentic_callback_redeliveries_with_server_stopped:4`；四条都经原产品双重鉴权与已存文件核验返回成功。停机后没有代理缓存或补写文件，current、对象摘要和修改时间均保持不变。

测试 middleware 仅将原产品已经成功处理的状态 2/6 通知保存在本轮 mode 600 的 `callback-tickets.jsonl`，供驱动原样重投。这个文件包含临时用途令牌与服务签名，既不进入公开 HTTP 日志，也不作为持久证据保留；Rust 目标及外层 finally 均负责删除。公开日志仍只记录不含 query 的路径、方法和响应状态。测试没有增加 HTTP 端点。

证据目录为 `/tmp/knowledgebrain-docx-recovery.v4m6dl89/run-1c69213710a14279bb032da1ff29c577/`，保存源码/SQL 副本、脱敏 HTTP 记录、四份保存 DOCX、结果与清理收据。这里只证明**已完成的相同通知**在服务不可达时仍可确认；它是主动重投，不是自然重试、首次保存缓存失效或网络断线编辑恢复的验收。

## 活动会话无需地址续期的实测

[生命周期鉴权复核](docx-editor.md#生命周期鉴权复核无需前端续期)后，采用同一镜像、浏览器、样稿和 `--config-ttl 20` 重跑。先验证不带 Authorization 的源读取返回 401，再由真实 Document Server 发起带有效 JWT 的源读取并成功打开。等待配置的 `exp` 已经过期后，原活动会话完成两次 forcesave 和最终保存，新 key 重开再编辑保存及停机后的四条真实签名回调重投也通过。

结果 **1 passed、0 failed、0 ignored、0 filtered、exit 0**，66.15 秒，run/cleanup exit 0。`result.json` 中 `saved_after_opening_config_expired:true`、`unsigned_source_rejected:true`、`authentic_callback_redeliveries_with_server_stopped:4`；测试没有重新签发活动配置，也没有关闭服务 JWT 的期限检查。20 秒为显式测试参数，不写入产品默认值；这仍不是长时间断网或完整编制前端验收。

源请求的已验证签名只暂存于 mode 600 的 `source-tickets.jsonl`，供驱动重投实际请求以核对 bytes，不由驱动生成签名；与 `callback-tickets.jsonl` 一样由内外两层清理删除，公开 HTTP 记录不含 Authorization 或 query。证据目录 `/tmp/knowledgebrain-docx-auth-lifecycle.1l3jw79q/run-e8758951de114e548bafb680eae9ab9b/` 保留脱敏请求状态、源码与样稿副本、结果和清理收据。

## 产品编制前端实测

在上述命令加 `--web`，会启动本轮独立 Vite 服务，绑定 loopback 随机端口并显式将 `/api` 代理到本轮产品 router。`web/e2e/docx-product.html` 和 `docx-product-entry.tsx` 仅提供测试启动入口，实际挂载 React StrictMode、产品 `Workbench`、`DocxEditor` 和路由组件。登录 token 仅注入该 Web origin；不会注入 Document Server iframe。页面由真实 HTTP 服务返回，编辑脚本、源读取、保存和回调均走真实网络，未改写签名配置或放宽浏览器安全检查。服务退出时停止 Vite 进程，外层超时也清理整个专属进程组。

本次验证使用前述同一镜像、浏览器及真实样稿副本：在产品页面插入正文标记，点击“保存”，等待界面显示“已保存”，核对保存版本号和“下载已保存稿”的文件摘要；连续两次均成功。未保存时，关闭按钮不可用，步骤跳转、知识资产链接及直接修改 hash 均留在当前编辑页。随后编辑并关闭页面，最终保存发布 revision 4；新 key 重开后，用产品按钮干净关闭/重新打开，再编辑并关闭，发布 revision 5。四份 DOCX 包含各自应有标记，历史文件保持原 bytes，停掉本轮 Document Server 后四条真实已完成回调重投也通过。

保存链结束后，另以 Playwright 对 current 查询注入 403 和 503：实际项目读取仍走产品 API，页面显示查询错误，既没有加载 ONLYOFFICE，也没有回退旧块编辑器。这是独立故障注入，不能当作真实 Document Server 故障恢复证据。

最终 native 目标 **1 passed、0 failed、0 ignored、0 filtered、exit 0**，62.60 秒；前端单元测试 **42 passed、0 failed**，生产构建和本轮文件的定向 ESLint 通过。构建仍有已有大包警告。证据位于 `/tmp/knowledgebrain-docx-web.kzof363w/run-2581486e80134380b2e5b7b7003a0e18/`，包含源码副本、请求日志、四份 DOCX、重开截图、`result.json` 和清理收据；run/cleanup 均 exit 0。原样稿、Git index 及本轮开始时的 bidding baseline 保持不变，没有新增 migration。

该目录的前三次失败记录也保留且清理成功：首次由浏览器对合成测试页面的地址空间判定阻止脚本加载，改为真实服务页面；随后两次发现直接 hash 切换可能先卸载编辑器，单独调整 DOM 监听顺序仍不通过。最终在 `useHash` 路由订阅更新状态前统一调用编辑页守卫，隔离浏览器和完整真实保存回归均通过。

此轮验证的是**已存在正式 DOCX 的产品编制入口**。测试启动入口直接提供本轮账户身份，未经过全 App 登录流程；初稿生成及前端发布新轮、表格/图片实际编辑、断网后持续协作、最终字体版式及生产部署仍未验收。截图仍可见 Document Server 的英文界面、协作显示名提示及引导浮层，浏览器日志保留部分图标加载错误，故不标完整 O1-W/O1 完成。正式用户信息及界面语言应使用实际账户/应用语言资料，不能填固定测试姓名或部署值。

## 账户、语言与完整 App 登录态恢复

后续 `--web` 模式已改用真实 `web/index.html`、`main.tsx` 和 `App`，移除上一批次的测试 HTML/TSX 启动入口。浏览器仍只在本轮 Web origin 注入测试服务器签发的用户 token，随后由产品 App 实际请求 `/api/v1/me`，显示数据库中的账户邮箱，再打开编制页。该模式验证登录态恢复，不包括输入账号密码登录；`result.json` 分别记录 `app_auth_bootstrap_tested:true`、`credential_login_tested:false`，API 可执行文件启动仍未验收。

编辑配置中的 `user.name` 由后端按认证用户读取现有邮箱，`user.id` 保持既有 actor；不从浏览器取协作身份。界面语言来自应用页面，未声明时使用浏览器语言，经后端语法校验后写入签名配置。当前应用 `zh-CN` 在此次 Document Server 实测中显示中文菜单，重开截图中原协作姓名提示已消失；文档校对语言仍来自 DOCX，没有通过 UI 语言选项改写文档内容。没有新增账户字段、语言配置默认值、依赖或 migration，也没有续期流程。

HTTP 契约增加：非法语言和伪造 `user` 字段返回 400，不改变当前会话；配置姓名等于数据库邮箱，语言精确进入签名载荷；不同显示语言的并行打开复用同一活动 key，省略语言的调用方继续使用服务默认值。完整 HTTP 目标 **1/1**，API 模块 **12/12**，API library 与 HTTP/native 目标 Clippy `-D warnings` 通过；前端测试 **42/42**、build 与定向 ESLint 通过，构建仍有已有大包警告。

使用前述镜像、浏览器和真实样稿副本，经完整 App 完成两次按钮保存与下载、最终保存、新 key 重开、干净关闭/重开、历史 bytes、四条停机回调重投，以及 403/503 查询故障分支：native **1 passed、0 failed、0 ignored、0 filtered、exit 0**，59.63 秒。样稿保持原 SHA256，HTTP 和 native 的临时服务均已清理。

证据目录 `/tmp/knowledgebrain-docx-profile.6cm9tr8n/`：`http2/` 为通过的 HTTP 契约，`run-9cd32effd2ef4b999ad7728f87b3397e/` 为真实 App/Document Server 联调，均有源码副本、日志和 run/cleanup exit 0；`http/` 保留首次编译失败及清理收据。API 模块、Clippy、前端日志和最终文件摘要同目录保存。这仍不是初稿生成、表格/图片实际修改、最终字体版式、断网协作或完整 O1 验收。

## 表格与图片的真实编辑回归

在真实产品命令中同时加入 `--web --rich-edit`，通过完整 App 和实际 ONLYOFFICE 控件完成本轮表格、图片操作。新增驱动为 `scripts/onlyoffice_rich_probe.py`；该模式要求提供含非空首表单元格的 DOCX，并使用当前中文应用界面的控件定位。表格定位文字从样稿读取，测试标记与 PNG 随机生成；使用现有 python-docx/Pillow 测试环境，无产品配置默认值或新增依赖。

驱动通过编辑器查找目标单元格，追加唯一测试文字；随后从“插入 → 图片 → 来自文件的图片”选入本轮 PNG，再通过图片高级设置将当前宽度放大 1.5 倍。所有修改来自浏览器键鼠和文件选择操作，没有调用编辑器内部对象修改文档，也没有使用 Automation API 或预先重写上传的 DOCX。实际图片宽度从 2.54 厘米改为 3.81 厘米，保存绘图尺寸为 `1371600 × 914400 EMU`，这些数值只是本轮测试夹具的结果。

两次产品按钮保存、关闭最终保存、新 key 重开后再次编辑和保存均核对实际下载文件：四张原表的列网格、单元格宽度、横纵合并关系、重复表头属性和文字保持预期；目标单元格保留测试修改，其他单元格不受影响。原有图片及新增 PNG 的 SHA256 保持，正文绘图关系实际引用新增图片，尺寸在后续各版本中一致。绘图检查覆盖 `wp:inline`/`wp:anchor` 及其外层 `mc:AlternateContent`，避免仅依赖 python-docx 的 `inline_shapes` 漏掉实际存在的兼容绘图。重开截图可见新增图片；历史文件 bytes、四条停机后真实回调重投和 403/503 查询故障分支继续通过。

最终 native **1 passed、0 failed、0 ignored、0 filtered、exit 0**，65.70 秒；run/cleanup exit 0。证据目录 `/tmp/knowledgebrain-docx-rich.y2vykrvj/run-1405cd12dfa74a869da455abc5e67bbb/` 包含通过版本源码、样稿与 PNG 副本、四份下载 DOCX、`rich-result.json`、`rich-inspected.json`、`result.json`、插图/重开截图及清理收据。原样稿仍为此前 SHA256，数据库 baseline 和 Git index 未变。

同级目录保留七次未通过的探索记录，均已清理：涉及查找后焦点尚未恢复、图片菜单/尺寸控件定位、以截图精确颜色估算缩放的不可靠断言，以及 `inline_shapes` 对兼容包装的遗漏。最终尺寸操作采用已核对的官方高级设置控件，验收以保存文件的绘图尺寸和媒体引用为准。另有只读提取 SDK 界面源码的未启动临时容器，已删除；没有修改 SDK 或关闭浏览器安全检查。

本切片仅增改验收工具和文档，未修改产品前后端、数据库、依赖或 migration。它完成此真实样稿的段落/单元格/图片插入与尺寸修改保存回归；没有验收所有表格编辑组合、图文环绕、分页与最终模板外观，也不代表 O1-S 初稿生成、前端新轮入口或完整 O1 已完成，O2 仍按计划等待。


## 2026-09-09 只更新目录的 DOCX/PDF 出件验证

完整编辑探针会插入 `KB-NATIVE-…` 等标记，所生成的测试文件不能直接交付。现有 `scripts/onlyoffice_product_probe.py` 增加 `--finalize-only`，要求同时提供 `--web --pdf --update-toc`，并拒绝与富文本编辑或开稿配置过期注入混用。该模式只通过正常界面更新原生目录、请求保存并等待实际入稿，然后关闭重开，从同一保存版本转换 PDF；不插入标记、不填投标事实、不改源文件。复用既有隔离服务、产品路由、保存/对象验证、ONLYOFFICE 转换及统一 Python DocReader 回读，没有另写解析器或实现 O2 产品导出。

DOCX 的目录标题、计算页码和书签仍按既有方法核对，并检查标题出现在实际 PDF 目标页。新增正文比较排除 TOC 结果和布局空白，核对其余文字字符不变；它不证明表格几何、图片或整稿语义正确。对实际保存文件修改表格文字的负向检查能发现变化。

两页合成模板的实际结果：无标记整理用例 **1 passed、0 failed、0 ignored、0 filtered**（21.90 秒）；随后完整编辑/历史/PDF/四次停服回调重放回归 **1/0/0/0**（64.38 秒）。两轮执行脚本与当前源码摘要一致，所有临时服务清理零错误。通过共享 source-view 渲染并目视检查两页 PDF：第一页目录正确指向第二页，第二页对应标题及表格完整可见，没有探针标记。此文件仅用于工具验收，不能作为真实招标样稿。

首次实测已完成保存及 PDF，但测试误等“无修改关闭后清除 key”，因此超时；失败日志与零错误清理证据保留。产品已有 `status=4` 只确认通知、保留可重开 key 的语义，见[保存说明](docx-editor.md#保存与失败行为)。修正的是验收脚本：无修改重开关闭后验证同一保存 identity 和原字节，不伪造新版本，也未修改产品会话逻辑。

证据见 [`artifacts/bid-full-sample/clean-finalization/`](../../artifacts/bid-full-sample/clean-finalization/)。真实提取通过独立复核并完成整稿编制后，应对其产物运行该模式，继续逐项审查实际完整 DOCX/PDF；32 项招标语义发现及 O1/O2 完整验收仍未关闭。
