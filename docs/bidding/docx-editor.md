# DOCX 编辑配置与保存接入

生成合同见 [大纲运行时](../design/evidence-runtime-contract.md)。本页维护已有 DOCX 的编辑配置、签名、保存关联和版本发布协议，不作为当前部署或真实服务验收通过的证明。

代码已有初始 DOCX 校验与新轮发布 API、冻结导出来源读取及报告下载路径，不能把当前候选验收未完成写成这些能力未实现。字体和最终模板外观、生产许可及完整故障验收仍需核验。知识库填充的当前主线接入与验收属于后续阶段，不属于本轮完成范围。

## 四个字段为何需要持久化

沿用 `bid_docx_current` 的 Workspace 主键与版本身份，使用以下字段：

| 字段 | 必要性与实际读写方 |
| --- | --- |
| `editor_key` | 打开配置时原子分配/复用；文件读取、请求保存与回调核对同一活动会话。API 重启不应改变 key。最终保存或新轮发布时清除。 |
| `editor_base_version_id` | 打开时冻结文件基线。同一会话内 forcesave 推进 current 后，Document Server 的源地址仍指向最初打开文件；以同项目/Workspace/轮次的外键限制基线。 |
| `pending_save_id` | 后端生成并作为命令 `userdata` 发送，回调匹配后才能发布 forcesave 文件。仅允许一个待完成请求，防止无可靠新旧关系的并行保存。 |
| `editor_error` | 回调 `3/7` 或明确命令拒绝的来源/错误码；由当前状态读取接口提供，不能因为旧文件仍存在就把失败显示成已保存。 |

key 与打开基线同时为空/非空；没有活动 key 时不得遗留 pending/error。无需独立 session ID、会话表或 session-current 表；历史 DOCX 继续使用 `bid_docx_version_artifacts`，请求回执使用既有 idempotency，审计使用既有 audit。源文件、业务风险、章节或材料类别不写成固定规则。

领域 SQL 只有限定动作集 `open/request_save/save/status`，按动作精确拒绝额外输入字段；它不是通用工作流。写入均通过 API role 所属函数，worker 无编辑函数权限，runtime 无直接写表权限。锁顺序保持 idempotency → project SHARE → Workspace UPDATE → DOCX current，网络请求不持有这些数据库锁。

## HTTP 与配置

实现：[编辑器模块](../../crates/api/src/bid_v2_routes/docx/editor.rs)。以下路径前缀为 `/api/v2/submission-workspaces/{workspace_id}/docx`：

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
| `KB_ONLYOFFICE_SERVER_ORIGIN` | 浏览器加载 `api.js` 的 Document Server HTTP(S) origin，必须是根 origin，无路径前缀、用户密码、query 或 fragment。浏览器必须能通过该 origin 加载编辑器；部署时核对代理后的 Host 与 HTTPS 配置。 |
| `KB_ONLYOFFICE_COMMAND_ORIGIN` | 可选。API 调用 CommandService 以及把回调 cache URL 改写后下载所用的 DS origin。空则等于 `SERVER_ORIGIN`。按部署网络显式配置。 |
| `KB_ONLYOFFICE_API_ORIGIN` | Document Server 可达的 API 根 origin，生成受控 source/callback 地址。按部署网络显式配置。 |
| `KB_ONLYOFFICE_JWT_SECRET` | 与 Document Server inbox/outbox 配置相符的 HS256 密钥，仅后端使用。回调采用标准 `Authorization: Bearer …`。 |
| `KB_ONLYOFFICE_CAPABILITY_SECRET` | 我方 source/callback 用途绑定的独立签名密钥，必须与前者不同。 |
| `KB_ONLYOFFICE_TOKEN_TTL_SECONDS` | 打开编辑器的签名配置有效期，正整数秒；不作为活动会话的编辑时限。 |
| `KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS` | 文档服务命令/下载的请求时间预算，正整数秒。 |

`config` 提供编辑器签名配置，`api_script_url` 用于加载官方 API。签名 token 可以交给浏览器，签名 secret 不进入返回值或日志。重新进入编辑页时通过已有 `/editor` 取得当前配置；活动会话的读取和保存由服务逐次签名及数据库中的活动关联控制，不需要前端定时续期或更新 callback 地址。当前不支持带路径前缀的服务部署或非标准 JWT header，不静默猜测这些配置。

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

## 验证要求

路由回归与真实文档服务验收分开记录。`docx-http-contract-tests` 使用文档服务替身，只验证协议，不能替代浏览器、真实 Document Server、存储及部署验收。配置缺失必须显式失败，不静默跳过。

- 并发打开复用 key；forcesave 后打开基线不变；重复保存命令只派发一次，pending 时拒绝另一次保存。
- 令牌用途、签名、期限、URL 与载荷作用域均核验；拒绝外部下载 URL、重定向、损坏文件及越权访问。
- 重放不重写 bytes、不推进 current；同关联换内容冲突；写盘失败及不确定命令保留可见失败或 pending。
- 最终保存清除活动状态；迟到回调、新轮切换及并发保存不能回退 current；不可变版本回执与完整 current 快照分别核验。
- 缓存过期后，已完成回调可核对本地回执；本地文件缺失或损坏仍失败，不能以远端补写掩盖问题。
- 真实服务还须验证打开、编辑、保存、关闭重开、表格与图片编辑、指定版本下载及故障恢复；同版 PDF 须在当前候选的完整导出链路中单独验收。

## 已完成回调的缓存失效恢复

首次保存须真实下载、DOCX 校验、staging 写入与回读，最后在一个事务内发布版本并完成幂等回执。保存动作的内部输入 `callback_sha256`：由 API 对已验证回调内容、round ID 和打开基线 ID 作 JCS 规范化后计算 SHA256；只有摘要进入原 idempotency 请求，缓存 URL、JWT 和密钥不持久化。

回调 body 的 transport `token` 以及外层 JWT 的签名/期限不参与通知身份；我方用途绑定始终校验签名和作用域，Authorization JWT 始终校验签名和期限，body `token` 不作为鉴权凭据。相同 JSON 内容的属性顺序不影响摘要。服务若更换 URL 或其他已签名内容，即使关联 ID 相同也按冲突拒绝，不猜测是同一次保存。HTTP 路径与请求 DTO 保持不变。

回执响应关联首次发布的版本、DOCX 摘要与字节长度。若两个请求都在回执产生前完成下载，后到的 SQL 提交仍须与胜出回执的摘要和长度一致，不能把不同 bytes 绑定到旧版本。未完成的过期会话或不匹配 pending 在下载前拒绝；提交事务仍重复执行锁内状态检查，防止下载期间发生最终保存或新轮切换。

缓存失效恢复仅确认已成功持久化且通知身份相同的保存，不解决首次保存时缓存已消失、不确定命令长期 pending 或自然重试策略。命令恢复 UI 仍待完善，不以超时自动释放未知保存。

## 生命周期鉴权

打开配置与固定文件地址采用不同的授权约束：打开配置保留显式 TTL；source/callback URL 中的签名仅绑定 audience、Workspace、round、editor key、打开基线与 actor，不再设置独立倒计时。两条路由均要求 Document Server 当次 Authorization JWT，缺失、签名错误、缺少 `exp` 或过期都拒绝。读取 JWT 必须精确包含 `payload:{url:完整源地址}`，回调 JWT 必须精确匹配 body 去掉 transport token 后的内容。用途签名单独不能读取文件或提交保存。

数据库继续逐次校验当前 owner、活动 key 与打开基线；最终保存或新轮发布后旧读取地址失效。历史完成回调只能在服务签名仍有效且内容与原回执完全一致时作只读确认，不能借此重开或写入旧会话。没有新增续期 API、定时器、配置变量、表或 migration，也没有延长默认过期时间、关闭服务 JWT 时效校验或修改业务规则。

## 编制前端接入

`Workbench` 先读取项目和当前 DOCX。编制步骤的编辑器是 ONLYOFFICE：`DocxEditor` 使用后端返回的官方脚本地址和签名配置。还没有当前 DOCX 时停留在骨架等待（`DraftReady`），稿件读取失败显示重试，两种情况都继续只挂这一套编辑器。文件 / 编制 / 导出三步在左栏当前标下保留（hash 链接，testid `wizard-*`）。版本是编辑历史，不是草稿／终稿等级，见 [docx-rounds.md](docx-rounds.md)。

`docxSession` 区分编辑器同步、命令受理和后端版本发布：SDK 同步完成不会显示“已保存”。保存请求保留预期版本和幂等身份；响应不确定时，“确认保存结果”复用同一请求。轮次/key 未变、新版本发布且 pending/error 清除后，才确认本次保存；请求后继续输入仍标记未保存。状态查询失败立即撤销已保存提示，恢复查询可清理连接错误。新轮或 key 变化会暂停旧会话保存，要求显式重开。

未确认修改时，关闭、步骤跳转、导航链接和退出登录受保护；地址栏 hash 切换在 `useHash` 更新页面前检查编辑页守卫，浏览器离开使用 `beforeunload` 提示。下载按钮明确下载指定的已保存版本。页面关闭/重开会销毁和重建 SDK 实例，迟到的打开请求和实例事件不更新新会话，兼容 React StrictMode 的清理与重启。

现有 [新轮 API](../../crates/api/src/bid_v2_routes/docx.rs) 接收上传文件并通过 `InitialDocx::new` 校验后发布；前端 `docxApi.publish` 已提供调用接口。该路径不同于自动从大纲生成首份 DOCX，不能只凭它认定自动生成入口已打通，也不能再称新轮发布 API 不存在。当前候选仍须核验具体产品入口、字体版式与完整故障行为；本页不根据旧进度记录判断登录功能是否缺失。

## 本次保存的精确确认

不能只以“current 版本改变且 pending/error 清空”确认本次保存，因为同一会话的另一次保存也能满足这个条件。先读取 current，再按本次 `editor_key + save_id` 查询已发布回执，核对轮次、父版本、当前 version/digest 和 pending/error 后才能显示“已保存”。回执失败、身份不匹配、其他保存或新轮均不能清除未保存修改；轮询两次读取之间提交的正常回调由下一次轮询确认。发送期间新编辑仍由本地修改序号保留为未保存。

复用既有不可删除的 `idempotency_requests` 完成记录和不可变 `bid_docx_version_artifacts`。只读领域函数先验证所有者，再核对请求范围和发布产物；worker 不获此查询权限。历史回执不依赖活动会话，不能证明对象此刻仍可下载，实际下载继续校验对象 bytes。

[冻结导出来源路由](../../crates/api/src/bid_v2_routes/export_source.rs) 已调用 `load_submission_export_source_v2`，按请求、版本、DOCX 摘要和长度核验源对象；[composition report 路由](../../crates/api/src/bid_v2_routes/docx/composition.rs) 已提供指定版本报告下载。前端已有 `ExportPane` 和导出/报告客户端。它们证明代码路径存在，不证明当前候选已完成真实同版 PDF、报告内容及端到端验收；报告下载也不等于独立语义审核已通过。
