# DOCX 编辑配置与保存接入

本切片将已实测的 ONLYOFFICE 保存语义接入 KnowledgeBrain 后端：签名编辑配置、限定用途的打开文件地址、可关联的 forcesave 请求、回调文件校验和原子版本发布。**没有新增会话表或 migration 文件**，只修改尚未发布的 Bidding baseline；未操作现有数据库。

已有 DOCX 正式稿的编制前端已接线，初稿自动生成及前端发布新轮入口尚未完成。后端故障契约使用真实 PostgreSQL/Redis/对象文件和受控文档服务替身；[真实 Document Server 与产品路由联调](onlyoffice-product-results.md)提供保存、重开及历史文件验证，新增 `--web` 模式直接驱动 React 编制页。字体、生产许可与最终模板外观仍待落实，不能标完整 O1 已验收。

## 四个字段为何需要持久化

沿用 `bid_docx_current` 的 Workspace 主键与版本身份，只补以下实际被读写的字段：

| 字段 | 必要性与实际读写方 |
| --- | --- |
| `editor_key` | 打开配置时原子分配/复用；文件读取、请求保存与回调核对同一活动会话。API 重启不应改变 key。最终保存或新轮发布时清除。 |
| `editor_base_version_id` | 打开时冻结文件基线。同一会话内 forcesave 推进 current 后，Document Server 的源地址仍指向最初打开文件；以同项目/Workspace/轮次的外键限制基线。 |
| `pending_save_id` | 后端生成并作为命令 `userdata` 发送，回调匹配后才能发布 forcesave 文件。仅允许一个待完成请求，防止无可靠新旧关系的并行保存。 |
| `editor_error` | 回调 `3/7` 或明确命令拒绝的来源/错误码；由当前状态读取接口提供，不能因为旧文件仍存在就把失败显示成已保存。 |

key 与打开基线同时为空/非空；没有活动 key 时不得遗留 pending/error。无需独立 session ID、会话表或 session-current 表；历史 DOCX 继续使用 `bid_docx_version_artifacts`，请求回执使用既有 idempotency，审计使用既有 audit。源文件、业务风险、章节或材料类别不写成固定规则。

领域 SQL 只有限定动作集 `open/request_save/save/status`，按动作精确拒绝额外输入字段；它不是通用工作流。写入均通过 API role 所属函数，worker 无编辑函数权限，runtime 无直接写表权限。锁顺序保持 idempotency → project SHARE → Workspace UPDATE → DOCX current，网络请求不持有这些数据库锁。

## HTTP 与配置

实现：[API 子模块](../../crates/api/src/bid_v2_routes/docx/editor.rs)、[Bidding 调用](../../crates/bidding/src/docx_round.rs)。以下路径前缀为 `/api/v2/submission-workspaces/{workspace_id}/docx`：

| 方法/路径 | 输入和结果 |
| --- | --- |
| POST `/editor` | 用户 JWT + `Idempotency-Key`，body 为 `{version_id, docx_sha256, language?}`，语言可省略。校验实际基线文件与 CAS 后返回 `config`、`session`、`api_script_url`。同一活动会话复用 key；旧打开请求不能在新轮或终结后复活 key。 |
| POST `/editor/save` | 用户 JWT + `Idempotency-Key`，body 为 `{editor_key, expected:{version_id,docx_sha256}}`。返回保存请求 ID、是否本次派发及其预约时 pending 状态；这不是文件保存回执。 |
| GET `/editor/{editor_key}/saves/{save_id}` | 用户 JWT；项目所有者读取该次 forcesave 的不可变入稿回执，尚未成功发布返回 JSON `null`。返回 save/key/round/parent/version/digest/revision，不以 current 替代历史结果；不依赖 Document Server 配置或缓存。 |
| GET `/editor/{editor_key}/source?token=…` | 校验读取用途绑定、Workspace/key 和打开基线，还要求 Document Server 的有效 HS256 Authorization JWT 精确签署完整读取 URL；地址本身不授予读取权限。返回经摘要/长度/容器检查的实际 DOCX。 |
| POST `/editor/{editor_key}/callback?token=…` | 同时校验我方回调用途绑定和 Document Server 的 HS256 Authorization JWT、时效及完整签名载荷；客户端不能只凭编辑配置中的用途签名伪造保存。 |
| GET `/current` | 原有版本元数据增补 `editor:{key,base_version_id,pending_save_id,save_error}`，供调用方查询活动状态。历史版本元数据保持原结构。 |

环境配置全部显式提供，未设置时编辑入口失败关闭；原 DOCX 上传/历史下载入口不依赖这些配置。没有默认部署地址或开发 secret：

| 变量 | 含义 |
| --- | --- |
| `KB_ONLYOFFICE_SERVER_ORIGIN` | 浏览器加载 `api.js` 的 Document Server HTTP(S) origin，必须是根 origin，无路径前缀、用户密码、query 或 fragment。本地 Compose（Caddy TLS）为 `https://127.0.0.1:18081/`；用局域网 IP 打开 SPA 时，`open` 会把该 origin 的主机改成请求 Host，避免混内容。 |
| `KB_ONLYOFFICE_COMMAND_ORIGIN` | 可选。API 调用 CommandService 以及把回调 cache URL 改写后下载所用的 DS origin。空则等于 `SERVER_ORIGIN`。Compose 内为 `http://onlyoffice/`。 |
| `KB_ONLYOFFICE_API_ORIGIN` | Document Server 可达的 API 根 origin，生成受控 source/callback 地址。Compose 内为 `http://api:8080/`。 |
| `KB_ONLYOFFICE_JWT_SECRET` | 与 Document Server inbox/outbox 配置相符的 HS256 密钥，仅后端使用。回调采用标准 `Authorization: Bearer …`。 |
| `KB_ONLYOFFICE_CAPABILITY_SECRET` | 我方 source/callback 用途绑定的独立签名密钥，必须与前者不同。 |
| `KB_ONLYOFFICE_TOKEN_TTL_SECONDS` | 打开编辑器的签名配置有效期，正整数秒；不作为活动会话的编辑时限。 |
| `KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS` | 文档服务命令/下载的请求时间预算，正整数秒。 |

`config` 提供编辑器签名配置，`api_script_url` 用于加载官方 API。签名 token 可以交给浏览器，签名 secret 不进入返回值或日志。重新进入编辑页时通过已有 `/editor` 取得当前配置；活动会话的读取和保存由服务逐次签名及数据库中的活动关联控制，不需要前端定时续期或更新 callback 地址。此切片也没有支持带路径前缀的服务部署或非标准 JWT header，不静默猜测这些配置。

打开请求在原版本身份之外可附带 `language`。前端取页面 `html.lang`，缺省时取浏览器语言；后端校验语言子标签语法，写入 `editorConfig.lang` 后再签名，不维护另一套服务翻译目录。省略该字段的旧调用方继续使用 Document Server 的默认语言。`editorConfig.user.id` 保持既有登录 actor，`user.name` 由后端按认证用户读取现有账户邮箱；不接受浏览器提供 `user` 或姓名。用户与项目访问核验保持原有链路，账户不存在时不能打开。语言、姓名属于打开配置，不改变 DOCX 版本、活动 key 或原幂等请求的存储结构。

配置中的 `customization.forcesave=false` 用于区分后端关联命令与编辑器按钮自动 forcesave；编辑器正常同步修改并未关闭。当前只接受带匹配 `userdata` 的 `forcesavetype=0`，不把来自其他 forcesave 来源的文件按到达时间发布。

## 保存与失败行为

- 后端先持久保存关联，再发送命令。相同请求重试返回 `dispatch=false`，不再次发送可能生成另一份内容的同关联命令。明确命令拒绝记错误并释放该 pending；传输失败/无效应答等不确定结果保留 pending，不能靠等待超时自动放行下一次请求。此时须等待对应回调或最终保存确认，尚无额外的命令恢复 UI。
- `status=6` 要求活动 key 与 pending ID 匹配。下载成功后校验真实 DOCX，复用 staging，写入后回读，再由一个事务新增同轮子版本、推进 current、清除 pending/error、转移 ObjectRegistry 所有权并写入幂等/audit；活动 key 与打开基线保持。
- `status=2` 不要求 forcesave 的 `userdata`。同样先校验/写入实际文件，再在同一事务发布最终子版本并清除活动 key、打开基线、pending 和错误。下次打开生成新 key。未完成的迟到 forcesave 拒绝；已完成的相同保存重放原回执，不回退 current。
- `status=1/4` 仅确认通知，不生成文件、不更换 key、不清除保存错误。`status=3/7` 记录失败；7 必须匹配 pending。重复的旧失败回执不会清除更新的 pending。
- 回调下载仅允许配置的 Document Server origin 和 `/cache/files/` 路径；拒绝其他协议/主机/端口、用户密码、fragment、重定向，并使用显式超时和既有平台文件大小上限。私有地址仅通过明确配置的服务 origin 使用，不增加全网抓取入口。
- 下载失败、损坏、摘要或存储失败不能推进版本。应用层处理成功才返回 ONLYOFFICE 数字 `error:0`；处理失败返回非 2xx 与数字 `error:1`/错误码。鉴权前的参数提取错误仍由既有 API extractor 返回错误状态。
- 已完成保存的重放先校验我方用途绑定、服务签名和下载 origin，再以已验证回调内容及打开基线的摘要查询只读回执。相同通知直接核验已有存储文件后确认，不访问服务缓存、不重写 bytes、不推进 current；缓存过期或文档服务停机不再阻止这一确认。服务请求 JWT 过期、签名错误、通知内容变化及本地文件损坏/缺失仍失败。
- 新轮创建在原事务内清空编辑关联；旧 token 或回调不会隐式打开新轮、复制旧响应或覆盖新稿。业务缺料/风险不参与这些技术保存判断。

## 验证范围

显式 `docx-http-contract-tests` 目标中的路由回归使用真实 `kb_runtime_api` 登录、三 baseline、集合 fixture、两份真实 DOCX 副本及本地对象存储，文档服务命令/下载由专属 HTTP 替身提供。新增断言包括：

- 并发打开相同 key、无修改通知保持 key、forcesave 后打开基线不变；命令重复请求仅物理派发一次，pending 时拒绝其他请求。
- 两方向令牌用途、期限、路径作用域、错误签名及载荷篡改拒绝；外部 URL、服务内非 cache 路径、重定向和损坏文件拒绝。
- 同关联换内容冲突、重放不重写 bytes；失败记录、旧失败不影响新 pending、不确定应答不重复派发；真实回调写盘失败返回数字 error 1，版本保持不变。
- 最终保存清除状态、旧 source/open 回执失效、下次生成新 key；已完成重放和未完成迟到回调分流，新轮 current 不回退；真实并发 HTTP forcesave/最终保存后最终文件保持为 current；worker 编辑函数 deny。

集合 fixture 分别保存“不可变版本元数据”和“包含 editor 状态的完整 current 快照”，继续完整比较来源发布前后 current，未改成只比较版本 ID。该 fixture 的 SQL 文件使用合成对象元数据；真实 bytes 由 HTTP 目标验证。

配置缺失时启用的 HTTP 目标失败，不静默跳过。复跑在原[HTTP 测试前置](docx-rounds.md#http-增量验证)基础上还需本页的 ONLYOFFICE 配置和专属测试替身；本机完整准备脚本/源码副本、日志及清理收据保存在 `/tmp/knowledgebrain-docx-editor.51jzk7c2/`。这些替身测试不是整个 workspace、API 可执行文件启动、S3 或浏览器产品验收；真实服务的后续证据独立记录在[产品联调结果](onlyoffice-product-results.md)。

最终检查：路由级 HTTP 目标 **1/1、0 ignored、0 filtered、exit 0**；API `bid_v2_routes::` 模块 **12/12**（其他 3 项被模块过滤）；Bidding baseline 契约 **21/21**；API library 与该 HTTP 目标 clippy `-D warnings` 通过。仅最后对错误提示的 match 分支应用 rustfmt，不改变执行逻辑；格式检查通过，未为纯格式变化重复启动服务。

证据工作包中的 `attempt5/` 为最终 HTTP 执行，`attempt4/` 已包含回调写盘失败和保存并发，二者 run/cleanup exit 0。`attempt2/` 因旧 fixture 将完整 current 与历史元数据等同而失败，原日志保留且清理 exit 0；修正为分别留存完整快照后通过，未忽略 editor 状态。`final-source/` 保存执行时的源码，最终核验记录保存格式化差异、SQL 副本一致性及 index/原稿保护结果。临时目录不是永久发布资产。

## 已完成回调的缓存失效恢复

首次保存仍需真实下载、DOCX 校验、staging 写入与回读，最后在一个事务内发布版本并完成原幂等回执。只调整现有两个编辑 SQL 函数，没有新增表、列、函数签名或 migration 文件。未发布 baseline 中保存动作的内部输入新增 `callback_sha256`：由 API 对已验证回调内容、round ID 和打开基线 ID 作 JCS 规范化后计算 SHA256；只有摘要进入原 idempotency 请求，缓存 URL、JWT 和密钥不持久化。

回调 body 的 transport `token` 以及外层 JWT 的签名/期限不参与通知身份；我方用途绑定始终校验签名和作用域，Authorization JWT 始终校验签名和期限，body `token` 不作为鉴权凭据。相同 JSON 内容的属性顺序不影响摘要。服务若更换 URL 或其他已签名内容，即使关联 ID 相同也按冲突拒绝，不猜测是同一次保存。HTTP 路径与请求 DTO 保持不变。

回执响应关联首次发布的版本、DOCX 摘要与字节长度。若两个请求都在回执产生前完成下载，后到的 SQL 提交仍须与胜出回执的摘要和长度一致，不能把不同 bytes 绑定到旧版本。未完成的过期会话或不匹配 pending 在下载前拒绝；提交事务仍重复执行锁内状态检查，防止下载期间发生最终保存或新轮切换。

HTTP 契约新增真实 GET 计数：替身以 `POST /test/mode {cache_available:false}` 模拟所有缓存 URL 返回 410，`GET /test/downloads` 返回实际缓存请求记录。重复 forcesave、最终保存和新轮后的旧完成回调均不增加 GET、版本或回执；本地文件损坏返回 422、缺失返回 503，仍不依赖远端补写。还验证过期或错误服务签名拒绝、相同关联改变通知内容拒绝，以及 SQL 提交时摘要/长度不一致拒绝。

结果：最终 HTTP 目标 1/1，API 模块 12/12（其他 3 项过滤），baseline 契约 21/21，API library 与两个 HTTP 目标 clippy `-D warnings` 通过。[真实服务联调](onlyoffice-product-results.md#已完成回调恢复实测)还完成停机后四条实际签名回调的主动重投。准备脚本与日志保存在 `/tmp/knowledgebrain-docx-recovery.v4m6dl89/`，`attempt2/` 为最终 HTTP 执行；两轮 HTTP 与真实服务 run/cleanup 均 exit 0。

这一恢复仅确认已成功持久化且通知身份相同的保存，不解决首次保存时缓存已消失、不确定命令长期 pending 或自然重试策略。后续生命周期复核已纠正把固定地址绑定和打开配置共用有效期的设计，见下节；不引入活动编辑器的地址续期机制。

## 生命周期鉴权复核：无需前端续期

先前把打开配置的 TTL 同时用于固定 source/callback 地址，导致编辑时间一长，服务仍在正常发送保存请求，我方地址令牌却先过期。这是接入设计造成的缺口，不是业务需求或 ONLYOFFICE 要求。官方[请求头签名文档](https://api.onlyoffice.com/docs/docs-api/additional-api/signature/request/token-in-header/)明确支持 Document Server 对每次文件读取的完整 URL、每次回调载荷分别签名。

因此分开两个用途：打开配置保留显式 TTL；source/callback URL 中的签名仅绑定 audience、Workspace、round、editor key、打开基线与 actor，不再设置独立倒计时。两条路由均要求 Document Server 当次 Authorization JWT，缺失、签名错误、缺少 `exp` 或过期都拒绝。读取 JWT 必须精确包含 `payload:{url:完整源地址}`，回调 JWT 必须精确匹配 body 去掉 transport token 后的内容。用途签名单独不能读取文件或提交保存。

数据库继续逐次校验当前 owner、活动 key 与打开基线；最终保存或新轮发布后旧读取地址失效。历史完成回调只能在服务签名仍有效且内容与原回执完全一致时作只读确认，不能借此重开或写入旧会话。没有新增续期 API、定时器、配置变量、表或 migration，也没有延长默认过期时间、关闭服务 JWT 时效校验或修改业务规则。

验证：HTTP 契约 1/1，包括源地址无 Authorization、错误 URL 签名、错误签名、源请求与回调 JWT 过期/缺少 `exp` 的拒绝，以及原有保存、旧会话和回执恢复回归；真实服务以测试参数 `--config-ttl 20` 等待打开配置过期后继续读取、编辑和保存，native 目标 1/1。API 模块 12/12、baseline 21/21、两个 HTTP 目标及 API library clippy 通过。证据位于 `/tmp/knowledgebrain-docx-auth-lifecycle.1l3jw79q/`，run/cleanup 均 exit 0。测试中的 20 秒只用于缩短等待，不是部署默认值。

## 编制前端接入

`Workbench` 先读取项目和当前 DOCX。已有正式稿时，编制步骤使用独立 `DocxEditor`，只使用后端返回的官方脚本地址和签名配置；文件 / 编制 / 导出三步在左栏当前标下保留（hash 链接，testid `wizard-*`），当前稿不再挂载旧块编辑器、候选写入或旧 renderer 出件入口。只有后端明确返回 `404/NOT_FOUND` 才允许尚无 DOCX 的项目进入旧流程；权限、网络或服务错误显示重试入口。

`docxSession` 区分编辑器同步、命令受理和后端版本发布：SDK 同步完成不会显示“已保存”。保存请求保留预期版本和幂等身份；响应不确定时，“确认保存结果”复用同一请求。轮次/key 未变、新版本发布且 pending/error 清除后，才确认本次保存；请求后继续输入仍标记未保存。状态查询失败立即撤销已保存提示，恢复查询可清理连接错误。新轮或 key 变化会暂停旧会话保存，要求显式重开。

未确认修改时，关闭、步骤跳转、导航链接和退出登录受保护；地址栏 hash 切换在 `useHash` 更新页面前检查编辑页守卫，浏览器离开使用 `beforeunload` 提示。下载按钮明确下载指定的已保存版本。页面关闭/重开会销毁和重建 SDK 实例，迟到的打开请求和实例事件不更新新会话，兼容 React StrictMode 的清理与重启。

本切片未新增依赖、部署地址默认值、数据库表列或 migration，也没有前端令牌续期流程。后续已完成[账户与应用语言实测](onlyoffice-product-results.md#账户语言与完整-app-登录态恢复)：复用现有邮箱，无需新增姓名字段；浏览器通过产品 `main.tsx`/`App` 调用真实 `/api/v1/me` 恢复登录态。真实样稿的[表格与图片编辑回归](onlyoffice-product-results.md#表格与图片的真实编辑回归)已补齐：单元格修改、图片插入及尺寸修改经实际页面保存和重开，并核对 DOCX 表结构、图片引用/摘要和尺寸。新轮首次生成/发布入口、账号密码登录、最终字体版式和完整故障验收仍需后续完成；出件版本冻结/PDF 属于 O2。

## 本次保存的精确确认

前端曾以“current 版本改变且 pending/error 清空”推断本次保存完成。同一会话的另一次保存也能满足这个条件，可能误清除未保存标记。现在先读取 current，再按本次 `editor_key + save_id` 查询已发布回执，核对轮次、父版本、当前 version/digest 和 pending/error 后才能显示“已保存”。回执失败、身份不匹配、其他保存或新轮均不能清除未保存修改；轮询两次读取之间提交的正常回调由下一次轮询确认。发送期间新编辑仍由本地修改序号保留为未保存。

复用既有不可删除的 `idempotency_requests` 完成记录和不可变 `bid_docx_version_artifacts`；仅在 fresh baseline 增加只读领域函数及 API grant，无新增表、列或 migration 文件。函数先验证所有者，再核对请求范围和发布产物；worker 不获此查询权限。历史回执不依赖活动会话，不能证明对象此刻仍可下载，实际下载继续校验对象 bytes。

验证与源码摘要见 [save-receipt/verification.json](../../artifacts/bid-full-sample/save-receipt/verification.json)。新增前端反例先失败、修复后整组 **33/33**，build/定向 ESLint 通过；真实本地 PG/Redis、生产路由和签名 HTTP 替身契约 **1/1**，覆盖 pending/写盘失败无回执、成功回执身份、随机 ID、非所有者、同所有者跨项目、worker 权限、新轮后历史回执不变及无额外下载/幂等写入。baseline **17/17**。首次测试编译因断言插入作用域错误失败，修正后重跑通过；两次专属环境均清理成功。服务替身不构成真实 ONLYOFFICE 或整本 Agent 样稿验收。

本项是 O1 保存反馈修复及 O2-S 的前置条件；尚未实现冻结导出请求、同 DOCX 转 PDF 或独立整稿报告。
