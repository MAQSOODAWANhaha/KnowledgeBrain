# DOCX 新轮持久化接缝

本切片实现 O1-S 的后端新轮、初始文件版本记录，以及真实 DOCX 上传/读取 HTTP 接口。输入是已经生成或用户明确选择的初始 DOCX；**编辑会话、签名配置和保存回调已有后端接线及局部验证；尚未实现初稿自动生成、编制前端与产品真实文档服务联调，不代表在线编制已可用**。

## 输入与所有权

[`docx_round.rs`](../../crates/bidding/src/docx_round.rs) 提供 `InitialDocx`、`DocxRoundBasis`、`DocxVersionIdentity`，以及创建、当前版本和历史版本读取接缝。

- `InitialDocx` 拥有经过校验的真实 bytes，计算并保存 SHA-256，复用现有 Office 容器、大小和压缩安全检查。格式校验不依赖伪造文件名，不证明模板内容或字体已验收。
- `DocxRoundBasis` 明确文件集合与要求集合的 ID/摘要。输入不包含旧 Workspace 正文、响应、绑定、报价、检查、页码或会话字段；反序列化拒绝未声明字段。初稿内容仍由后续生成器或显式模板选择负责，本接缝不会自动清除用户所选 DOCX 内已有文字。
- 创建前，调用者须完成身份验证，使用原 ObjectRegistry 流程注册 staging，再写入 `InitialDocx.bytes()`。HTTP 入口复用既有 `stage_upload`；写入、回读或发布失败时等待既有 cleanup 入队。任务中断遗留的 staging 仍由平台过期扫描处理；本切片不另建清理框架。

生产 SQL 只校验对象注册身份和转移所有权，不能代替物理文件写入/读取检查。HTTP 入口在发布前回读实际对象并核对摘要、长度和 DOCX 容器；下载也执行相同校验。

## 持久化与并发

所属 `bidding_v2_baseline.sql` 新增三张表：

| 表 | 职责 |
| --- | --- |
| `bid_docx_round_artifacts` | 每轮冻结完整文件集合、要求集合及摘要，按 Workspace 记录轮次 |
| `bid_docx_version_artifacts` | DOCX 对象、摘要、长度和所属轮次；本切片仅创建无 parent 的第一个版本 |
| `bid_docx_current` | 当前轮次和文件版本，使用版本 ID/摘要进行 CAS |

`kb_bid_v2_create_docx_round` 校验项目 owner、项目仍开放、当前 DOCX 版本、当前文件集合和要求集合，以及要求是否属于当前集合/disposition。集合变化但要求未就绪时拒绝过期组合；不依据业务风险或材料充分性阻断。

同一事务创建 round/version、将 staging 转为 `bid_docx_version` 的持久对象引用、推进 current、写 audit 和幂等 receipt。请求身份包含 Workspace、冻结输入、预期版本及初稿内容身份，排除临时 staging UUID：同 key/同输入重放可消费新的匹配 staging，并返回首次结果；换内容拒绝，重放旧轮不回退 current。

新轮不调用旧 `advance_workspace_projection`，不读取或复制旧节点、块、binding、quote、assessment 或页码映射。旧 Workspace 和旧 DOCX 版本均保留；仅版本历史关系并不代表正文内容已做业务检查。

权限只授予 API role 所属 DOCX 领域函数（新轮、编辑/保存、重放、当前与历史读取）；runtime 不直接写表，worker 没有文档读取授权。项目状态使用 SHARE 锁，避免与集合冻结的外键 KEY SHARE 锁构成锁循环；首次创建在 Workspace 上串行化，再检查版本和来源 current。

## 持久化必要性复核：编辑会话增量暂不落表

本轮按用户提出的“新增 migration 尤其确认是否必须、设计是否合理”复核。**结论：已有 round/version/current 的职责成立；跨请求稳定的会话关联需要保存，但不能由此推导出现在必须新增一张会话表。** 本轮试写的会话表、SQL writer 与仅分配 key 的 HTTP 入口已撤回；三份代码/SQL 文件与本轮开始时一致，上轮上传、读取和版本能力保留。

仓库目前尚未发布，按[平台 fresh baseline 规则](../../plans/platform/runtime-foundation.md#2-fresh-baseline)维护所属 baseline，不另建历史 migration chain。修改 baseline 和对现有数据库执行变更是两件事：本次没有执行数据库变更。后续 schema 增量先说明现有结构为何不够、必要字段及实际读写方，再用隔离数据库验证；这不增加普通开发的逐项人工批准流程。

### 现有结构与复用判断

| 结构/方案 | 已承担的职责或取舍 |
| --- | --- |
| `bid_docx_round_artifacts` | 冻结每轮完整文件/要求输入；同一轮后续多个保存版本共用，避免把冻结输入重复写进每个版本。 |
| `bid_docx_version_artifacts` | 保存每份不可变 DOCX 的对象、摘要和历史父版本；已有上传/下载和 ObjectRegistry 实际调用方。 |
| `bid_docx_current` | 表达当前选中的轮次/版本并作为 CAS 的对象；当前选择不由迟到请求或简单时间排序推断。未来可评估在这里保存最小活动会话字段，无须预设独立 session-current 表。 |
| 旧 Workspace / Workspace revision | 前者是一项目一个的身份，后者绑定旧节点/块/报价等编制快照。复用其 ID/权限；将 Office 会话塞进旧正文快照会混淆两个生命周期。 |
| 既有 idempotency / audit | 继续承担请求重放和审计。单个请求回执不能独立保证不同请求、不同 API 进程加入同一活动会话；不能为此再建一套通用请求/审计框架。 |
| 每请求随机 key、版本摘要当 key、进程内缓存 | 分别可能拆分活动会话、在 forcesave 后错误换 key 或在重启/多实例时丢失关联；不能直接采用。 |
| 独立会话表 | 只有在活动关联之外确需独立查询/约束会话历史，且既有 current/version/audit 无法清晰承担时再落地。表数少不自动合理，职责独立也不自动证明必须加表。 |

### 本轮试写为何撤回

- 只有 `active/superseded`，未覆盖最终保存确认后的关闭重开；提前暴露分配接口会留下不能正确结束的会话。浏览器关闭/刷新本身不能替代服务端保存确认。
- 只绑定打开版本，未确定 forcesave、最终保存及乱序回调的因果关联。CAS 能防止同时更新同一旧版本，不能独自判断哪个文档快照更新，不能按回调到达顺序发布。
- 为 session ID 和 `document.key` 分别生成两个 UUID 没有独立需求依据。后续可由一个持久会话身份生成符合协议的 key，不预先保留重复标识字段。
- 分配 key 的接口尚无可用的编辑 config、受控文档读取和签名回调消费者；只返回标识不构成编辑接入。

### 下次实现须闭合的生命周期

| 事件 | 实现必须证明的行为 |
| --- | --- |
| 并发打开/页面刷新/API 重启 | 同一活动会话复用同一个 key；打开基线与项目/Workspace/轮次关联保持。 |
| `status=6` forcesave | 校验签名、会话归属和保存关联，实际文件发布成功才确认；继续编辑时 key 不随该文件版本变化。 |
| `status=2` 最终保存 | 实际文件与最终保存关联验证后再确认完成；后续重新打开按已完成会话的生命周期生成新 key，旧回调不能覆盖。 |
| `status=4` 无修改关闭 | 不创建伪保存版本；官方文档允许断网后出现 `4 → 1` 重连，因此不能机械地把每个 4 都视为永久关闭并立即换 key。具体处理需在选定服务版本实测。 |
| `status=3/7` 保存失败 | 不推进成功版本，不返回伪成功，保留可恢复的失败关联。 |
| 新轮/重复/乱序回调 | 新轮不复用旧会话；同一已处理事件幂等。保存先后关系不能确认时不推进当前版本；普通 open 请求回执不能复活旧会话。 |

官方 forcesave 命令支持 `userdata` 关联请求，但这不是所有保存事件的可靠递增版本号；是否采用由后端发起、可关联的保存请求及如何处理其余保存来源，应先以实际协议回归确定，再选择 current 字段或最小专用表。**未完成该验证前，不新增预留表、字段或接口，也不把会话标为已接入。**

依据：[document.key](https://api.onlyoffice.com/docs/docs-api/usage-api/config/document/)、[保存流程](https://api.onlyoffice.com/docs/docs-api/get-started/how-it-works/saving-file/)、[回调状态及断网重连](https://api.onlyoffice.com/docs/docs-api/usage-api/callback-handler/)、[forcesave 请求关联](https://api.onlyoffice.com/docs/docs-api/additional-api/command-service/forcesave/)。本轮读取官方仓库对应文档，源码快照、撤回的试写和文件摘要复核保存在 `/tmp/knowledgebrain-docx-session.sahagmd3/`；未声称这些生命周期已经通过真实编辑服务回归。

## 生命周期实测增量

已完成独立 ONLYOFFICE 两次带关联标识的 forcesave、继续编辑后的最终保存、旧签名回调显式重投、新 key 打开最终文件及无修改同 key 的 `4 → 1` 重开。真实文件内容与摘要有对应证据；证明签名和到达顺序不足以决定最新版，且 `status=4` 不能机械地终结 key。本轮只增加可复跑协议试验工具和结果记录，没有增加表、字段或分配接口。下一实现继续优先评估复用现有 current/version/idempotency/audit，见[实测结果与最小实现约束](onlyoffice-lifecycle-results.md)。这不是生产保存、断网恢复或完整 O1 验收。

## 编辑后端接线增量

根据上述实测，只在既有 `bid_docx_current` 增加活动 key、打开基线、pending 保存关联和错误状态四个必要字段，配合现有 version/idempotency/audit，实现完整的后端打开与终结转换；未增加会话表或独立 migration 文件。此前的撤回记录保留为历史复核，此增量已有实际配置/读取/命令/回调调用方。字段必要性、配置、事务和剩余限制见 [DOCX 编辑接入](docx-editor.md)。

## HTTP 接口

实现位于 [`bid_v2_routes/docx.rs`](../../crates/api/src/bid_v2_routes/docx.rs)，复用现有 JWT、项目 owner、对象存储与文件预算配置。

| 方法 | 路径（前缀 `/api/v2/submission-workspaces/{workspace_id}`） | 行为 |
| --- | --- | --- |
| POST | `/docx-rounds` | 上传明确选择的初始 DOCX，创建新轮；返回 201 和持久化回执 |
| GET | `/docx/current` | 返回当前文件版本及冻结输入，无当前版本时返回 null |
| GET | `/docx/versions/{version_id}` | 返回授权范围内历史版本元数据 |
| GET | `/docx/versions/{version_id}/download` | 校验并下载该版本真实 DOCX |

POST 必须带 `Idempotency-Key`，multipart 仅接受各一个 `metadata` 和 `file`，字段顺序不限；缺失、重复、额外字段及损坏 DOCX 均拒绝。metadata 结构如下，ID/摘要全部来自当前接口结果；首次 `expected` 为 null，后续须携带当前版本 ID/摘要：

```text
{
  "basis": {
    "document_set_id": "<当前文件集合 UUID>",
    "document_set_sha256": "<文件集合 SHA-256>",
    "requirement_set_id": "<当前要求集合 UUID>",
    "requirement_set_sha256": "<要求集合 SHA-256>"
  },
  "expected": {"version_id": "<当前 DOCX 版本 UUID>", "docx_sha256": "<DOCX SHA-256>"}
}
```

项目 owner 校验先于 multipart 内容读取及对象注册。完成过的同输入请求通过只读 SQL 重放接缝返回原回执，同时验证原文件仍可读取，不重写其 bytes。版本或冻结输入变化返回 409；旧请求重放不推进 current。文件损坏返回 422 `DOCX_OBJECT_INTEGRITY_FAILED`，不可读取返回 503 `DOCX_OBJECT_UNAVAILABLE`。下载返回 DOCX MIME、版本 UUID 文件名、摘要 ETag 和 `private, no-store`。

此历史下载接口面向已登录用户。Document Server 的签名文件地址、编辑配置、保存关联及回调已另接入 [DOCX 编辑后端](docx-editor.md)，两类读取授权用途分开。

## 验证与剩余项

- DOCX 模块 3/3：真实 DOCX bytes/摘要保持，损坏和非 Office 输入拒绝，完整来源与版本输入以及旧字段拒绝。
- 复用的上传模块 5/5，既有六类文件及 Office 安全拒绝行为保持。
- 最终 Bidding baseline 契约目标 21/21，library clippy（`-D warnings`）通过；不代表整个 workspace 或 hosted CI 通过。
- SQL fixture 覆盖两轮创建、完整来源变化、要求未就绪、版本 CAS、对象尺寸、owner、重复/换内容重放、当前/历史读取、旧稿不变，以及 API allow/direct-write deny 和 worker deny。SQL 使用合成对象元数据，角色通过 `SET ROLE` 验证，未声称真实登录或 HTTP 文件保存。
- 独立双连接回归：相同基线两创建只有一个成功，另一条真实 `DOCX_VERSION_CAS_MISMATCH`；集合冻结与创建并发时冻结成功，新轮以 `DOCX_ROUND_BASIS_CHANGED` 拒绝，无死锁。

本机证据 `/tmp/knowledgebrain-docx-round.zubljb4b/` 保存 before、Rust 日志、三批精确 SQL/基线副本及清理收据。`database-attempt2/` 事务 fixture 回滚通过；最终锁修复与双连接测试在 `database-attempt3/`。后者只在独立无网络/无映射端口容器内提交测试资料用于跨连接观察，退出删除专属容器和临时卷。第一次 SQL fixture 的列名歧义和初次 Rust 公共导出路径编译错误均保留，未覆盖原失败日志。临时目录不是永久发布资产。

### HTTP 增量验证

- API `bid_v2_routes::` 单元测试 **12/12**，含 multipart 缺失/重复/额外字段检查；这是模块过滤执行，不代表整个 API 包通过。
- 独立 PostgreSQL + Redis + 本地对象目录，使用真实 `kb_runtime_api` 登录和 `testdata/bid` 原稿副本，路由级 HTTP 契约 **1/1、0 ignored、0 filtered**。覆盖身份/owner、损坏输入、真实字节上传下载、两次新轮、同 key 换内容、重试不重写、旧轮重放不回退、过期版本 CAS、物理写入失败及 cleanup 入队、历史文件损坏/缺失。
- 三个所属 baseline 与集合 SQL fixture 在专属数据库执行通过；最终 Bidding baseline 契约 **21/21**；API library 与显式 HTTP 测试目标 clippy（`-D warnings`）通过。HTTP 测试通过 `router_with` / Tower 执行真实提取器、鉴权、SQL 和对象读写，未启动 API 可执行文件，不能代替启动 receipt/readiness、浏览器或 S3 验收。cleanup 本轮只验证失败任务入队，未运行 retention 消费者。

HTTP 目标显式启用 `docx-http-contract-tests` feature。复跑须提供专属已准备数据库的管理员 `KNOWLEDGEBRAIN_TEST_DATABASE_URL`、同库 `kb_runtime_api` 的 `DATABASE_URL`、专属 `REDIS_URL`、UUID `KB_DEPLOYMENT_NAMESPACE_ID`、位于临时目录的专属 `OBJECT_DIR`、两个不同且有效 DOCX 的 `KNOWLEDGEBRAIN_TEST_DOCX_PATH` / `KNOWLEDGEBRAIN_TEST_SECOND_DOCX_PATH`；禁用 S3。数据库应装入三个 baseline 和 `document_collection_acceptance.sql` fixture，仅在一次性测试库把 fixture 最终 ROLLBACK 改为 COMMIT。测试会故意损坏专属对象目录/文件，禁止指向已有业务资料。启用 feature 后配置缺失直接失败：

```sh
cargo test --locked --offline -p api --features docx-http-contract-tests --test docx_round_http -- --nocapture
```

本机证据 `/tmp/knowledgebrain-docx-http.2sq_i99v/` 保留源码前态、精确 SQL 副本和检查日志；`attempt2/` 首次通过；`attempt3/` 在 clippy 条件分支简化与格式化后完成最终复测，两次 `cleanup.exit=0`。首次 Docker internal network 未公布端口，尚未执行 SQL/HTTP 即失败，原日志和清理收据保留；第二次用专属 bridge、仅回环地址的随机端口运行，退出删除专属容器、卷、网络及对象目录。最终测试源码与精确 SQL 副本摘要匹配，index 与本切片开始一致，真实原稿未修改。生产代码没有写入样稿路径、部署地址或业务分类规则。

后续编辑前端与真实文档服务联调记录见[产品联调](onlyoffice-product-results.md)。O0 字体/生产许可及模板外观仍待确认，完整 O1-S、F2 和 ONLYOFFICE 接入均未验收；旧运行链在 O4 前保留。

## 前端选择 DOCX 创建新轮

编制页尚无 DOCX 时，可从「从 DOCX 创建新轮」选择准备好的初稿或指定模板；旧编辑页有未保存草稿或正在执行操作时不能切换。已有 DOCX 时，先保存并关闭本页编辑器，再进入新轮入口；保存待确认时要求等待后刷新依据。无修改关闭产生的 status 4 可以保留可重连 key，不能据此断言编辑仍活跃。页面明确新轮会使旧轮编辑会话失效；其他窗口在确认后保存导致版本变化，仍由既有 CAS 拒绝新轮覆盖。

新增只读 `GET /api/v2/submission-workspaces/{workspace_id}/docx-rounds/basis` 返回当前文件集和要求集的四字段身份。查询使用同一数据库快照中的 current 指针，要求集必须关联当前文件集及 disposition；尚未匹配时返回 HTTP 200 / JSON null。初始化的空要求集也有有效身份，不依赖要求列表第一行、列表时间顺序或旧 Workspace projection。`/docx/current` 无稿件时同样返回 200 / null，404 和权限／服务故障不激活旧写入器。

此次持久化评估结论：复用现有 current、不可变集合及 Workspace 归属信息即可，不需要新增表、列或 migration 文件。所属 baseline 仅新增 `STABLE SECURITY DEFINER` 只读函数和 API 角色的执行授权；owner 校验仍在函数内，未放宽底表访问权限。实际发布继续在既有事务内校验冻结身份及版本 CAS，读取依据不会预约版本或锁住后续用户操作。

用户明确确认后，前端发送所选文件和当时的冻结依据；首次 `expected:null`，下一轮携带已保存版本身份。网络／服务错误及无效成功回执均保留原文件、依据、expected 和幂等键，通过「确认发布结果」重试；待确认期间禁换文件、取消、导航及退出。明确的 409 不自动换依据再次发布，须刷新并重新确认。成功回执展示发布轮次，进入稿件时重新读取 current。浏览器刷新／关闭仍可能丢失内存中的重试状态，因此未确认发布时使用浏览器离开提示，不宣称跨浏览器恢复。

此入口是**显式选择 DOCX**，该批次尚未实施自动初稿生成（后续合成生成链见[编制记录](docx-composition.md)），也不把招标文件自动当作投标证明或已完成投标稿。自动生成仍须落实 F2 的完整冻结输入、招标指定目录／模板优先及可编辑表格生成；禁止将人工编辑后的旧 Workspace 内容重建为正式稿。

验证证据位于本机 `/tmp/knowledgebrain-docx-round-ui.2kx1g72k/`：前端 **51/51**、build、定向 ESLint，baseline **21/21**、API 模块 **12/12**、API/HTTP/native 目标 Clippy 通过；`http2/` 真实 runtime 登录的 HTTP 契约 **1/1**，覆盖空稿首次发布、空要求集身份、同请求重放、版本冲突、越权、disposition 前进后依据不可用及旧依据发布拒绝。首批夹具删除受保护 current 被拒绝的日志保留；修正为通过已有函数正常前进 current，未绕过保护。

完整 App + 真实 Document Server 最终批次 `native/run-253f9543646f456ba5be0307627817f5/`：native **1/1、65.66 秒、run.exit=0、cleanup.errors=[]**。新建独占项目，实际文件选择上传后在浏览器丢弃已成功响应；重试使用相同文件摘要、metadata 与 key，仍为第 1 轮。待确认时禁取消／换文件并拦截 hash 跳转；真实编辑器无修改关闭后可发布第 2 轮，并获取新 key。`round-ui.json` 记录两轮真实 current，`round-ui.png` 记录最终页面。原有两次 forcesave、最终保存、新 key 重开后再编辑、历史字节和停机后四条真实签名回调重投保持通过，另检查 403/404/503 查询失败不激活旧写入器。

三批未完成的 native 运行也保留证据并完成资源清理：首次 Playwright `route.fetch` 转发 multipart 得到 400，故障注入改为原生上传完成后在 fetch 返回前丢弃响应；第二批暴露无修改关闭仍保留 key 的错误等待条件；第三批新轮场景已通过，但模块更名后本批 Vite 缓存不再对应最终路径，终止并重新运行最终源码。控制器最终命名 `docxRoundSession.ts`，避免与 `DocxRound.tsx` 在不区分大小写的文件系统上产生解析歧义。原失败日志未覆盖，四批专属容器／卷／网络均清理，真实样稿和 index 保持原摘要。
