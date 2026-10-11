# 运行配置与数据库账号收敛方案

## 状态、范围与目标

本页是用户确认方向后的待实施方案，不代表代码已完成。核对基线为 PR #22 的 `3a8bafae77048d3e94f9ddd1c0aef26ce7a1424d`（2026-10-11）。本 PR 只提交方案和导航，不修改运行代码、不部署、不清库。

用户只需明确提供：**账号密码、JWT_SECRET、模型服务配置、最大上下文**。其余参数使用程序默认值或由标准部署派生，允许外部基础设施的显式覆盖。当前业务范围为招标解析、OCR、Discover → Organize → Check；知识检索与匹配填充不在本阶段。

**仅支持全新数据库部署，不设计旧库、旧角色、旧配置或旧检查点迁移。** 不编写 ALTER/backfill、角色转移或兼容迁移工具，不自动删除已有数据库或资料。遇到已有不匹配数据库，明确拒绝并要求提供新空库。

本方案取代 `runtime-foundation.md` 中独立 migrator/API/worker/retention 数据库账号、专用 owner 与角色 ACL 边界的目标；schema 正确性、业务校验、幂等、审计、对象引用保护仍保留。数据库权限收敛不等于合并 API/Worker/DocReader 进程或删除 retention 业务职责。

## 1. 当前问题与实施差距

| 问题 | 当前依据 | 目标 |
| --- | --- | --- |
| 用户要配置四套运行角色密码 | `deploy/postgres-init/010-runtime-identities.sh`、`deploy/docker-compose.yml` | 一套应用数据库账号，各组件使用相同连接 |
| SQL 与 catalog 仍硬编码多角色 | `migrations/*baseline.sql`、`crates/platform/src/catalog.rs`、`db.rs` | fresh baseline 与校验共同改为单账号，不只删 env |
| 上下文固定且另有 JSON 覆盖 | `crates/bidding/src/analysis/agent.rs` | 一个必填标量入口，131072 是当前选择而非永久上限 |
| 未知模型要求用户提供 tokenizer 校准 | `agent_runtime/chat/tokenization.rs` | 程序内部保守计数、自动分包；不要求用户理解编码或倍率 |
| 无 LDAP 的登录不校验密码 | `crates/platform/src/auth.rs`、`crates/api/src/routes.rs` | 默认本地账号密码登录，LDAP 为可选替代 |
| JWT 与存储等存在固定开发秘密 | `crates/api/src/lib.rs`、`deploy/docker-compose.yml` | 明确凭据，无固定或共享秘密回退 |
| 不用知识库仍被 Embedding 就绪门阻塞 | `crates/platform/src/probe.rs` | readiness 只检查当前启用能力 |
| Compose、宿主脚本、代码默认不一致 | `deploy/.env.example`、Compose、runtime helper | 同一解析规则与默认值，环境只决定网络拓扑 |

此前本地已验证的 `KB_TENDER_AGENT_LIMITS` 删除改动不属于本 PR；实施时复用其删除方向，不能把其中固定 131072 的临时行为当最终接口。原工作区修改保持原样，不借本 PR 提交。

## 2. 唯一用户配置入口

以下是**目标接口**；新增变量必须在后续实现、测试和 Compose 透传后才能使用。

| 变量或组 | 必填条件 | 规则 |
| --- | --- | --- |
| `POSTGRES_USER`、`POSTGRES_PASSWORD` | 标准部署必填 | 唯一应用数据库登录身份；不再有四种运行密码 |
| `KB_ADMIN_USERNAME`、`KB_ADMIN_PASSWORD`（新增） | 默认本地认证必填 | 首次空库初始化创建一个管理员；不等同数据库用户。运行时缺失不应无意重建或覆盖已有用户，标准部署保留 Secret 输入 |
| `JWT_SECRET` | 必填 | 非空强随机秘密；无固定回退，不与其他密码复用 |
| `KNOWLEDGEBRAIN_CHAT_BASE_URL/API_KEY/MODEL` | 招标流程必填 | 地址由用户指定；模型须支持工具调用，完整原图流程还须支持图像；不默认供应商 |
| `KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS`（新增） | 必填 | 正整数；当前配置 131072；写入现有 `limits.max_context_tokens` 并冻结到运行 |
| `MINIO_ROOT_USER/PASSWORD` | 默认自带 MinIO 必填 | 标准开发部署可由其派生应用连接凭据；不是数据库或应用登录密码 |
| `KNOWLEDGEBRAIN_S3_ACCESS_KEY/SECRET_KEY` | 独立外部 S3 时必填 | 外部存储使用其授权凭据，不继承 MinIO root 或其他服务密码 |
| `KNOWLEDGEBRAIN_VLM_BASE_URL/API_KEY/MODEL` | 使用独立 OCR 服务时条件必填 | 详见视觉继承规则；不支持鉴权的本地服务需要显式能力约定，不能任意猜造 Key |
| `KNOWLEDGEBRAIN_EMBEDDING_*` | 本阶段不要求 | 后续开启知识能力时要求；当前 readiness 必须先修改，不能用假端点填空 |

生产可通过 Secret 注入上述值，无需明文 `.env`。密码校验只报键名及要求，不输出值。新管理员使用成熟、带盐的密码 KDF 存储；初始化幂等，不因重启重新设置密码；不开放公共注册。默认登录必须校验密码，删除未配 LDAP 时开放登录的回退。LDAP 的 URL、DN 模板为高级可选覆盖；若选择 LDAP，账号密码来自身份系统，不再要求本地管理员引导，不能把 `ldaps://` 简单降为明文 TCP。

数据库管理账号、对象存储账号、用户登录账号是不同身份，不复用密码；“一套数据库账号”仅指应用各进程访问同一数据库。

## 3. 单数据库账号与 fresh baseline

### 标准部署

1. 所有应用组件（baseline 执行入口、API、Worker、retention）读取同一组数据库身份，部署派生同一 `DATABASE_URL`。删除 `KNOWLEDGEBRAIN_MIGRATOR_PASSWORD`、`KNOWLEDGEBRAIN_API_DB_PASSWORD`、`KNOWLEDGEBRAIN_WORKER_DB_PASSWORD`、`KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD`。
2. 删除 `kb_migrator`、`kb_runtime_api`、`kb_runtime_worker`、`kb_runtime_retention`、`kb_app_owner` 的创建和依赖；不保留同密码多角色的表面简化。应用账号直接拥有专用应用 schema 与其对象。
3. PostgreSQL 服务初始化所需的引擎管理身份由容器内部管理，不作为第二套应用账号暴露。不能直接把应用 `POSTGRES_USER` 交给官方镜像初始化后让 API 继承 superuser。实现部署初始化适配：本地受控初始化准备必要扩展和应用库，创建用户提供的应用账号为 NOSUPERUSER/NOCREATEROLE/NOREPLICATION/NOBYPASSRLS；应用阶段只使用该账号。
4. 外部 PostgreSQL 由其管理员预备数据库和扩展，用户提供一套应用凭据；应用不得尝试夺取集群管理权限。地址、端口、库名可覆盖，不能把外部凭据硬编码。
5. 三份 fresh baseline 仍按现有领域顺序、锁和事务执行；移除 `SET LOCAL ROLE kb_app_owner`、多角色 GRANT/REVOKE/角色断言，改为当前应用 owner。保留与匿名 PUBLIC 访问、函数执行及跨库边界相关的合理限制。
6. API/Worker 仍只执行只读 schema readiness，不自行创建或修补 schema。baseline 初始化是独立启动步骤，即使凭据相同也不能在普通请求路径执行 DDL。

### 必须同步调整的安全合同

共享账号后，数据库不再隔离 API/Worker/retention 的 DDL 或删除权限，这是明确接受的取舍，不能继续宣称角色级最小权限。业务授权、事务、lease/token fence、幂等和删除引用保护必须继续在实际调用路径验证。单账号拥有对象意味着有权绕过部分 DB 保护，应如实记录其防护边界。

`catalog.rs` 的 owner 过滤、角色白名单、ACL canonicalization；`db.rs` 的 owner inventory、baseline 执行、receipt 校验；相关 SQL 函数、RLS/SECURITY DEFINER、测试及 fresh-schema 验收必须一起修改。定义应用 schema/object 边界，排除系统与扩展对象，不能以“所有当前用户对象”无界收集。可配置用户名必须按 PostgreSQL identifier 规则安全引用，不拼接密码或未转义标识符。

catalog/receipt 合同版本、baseline hash 与 release descriptor 同步重新生成；不能关闭校验绕过差异。发布身份由构建与部署生成并验证，生产不能沿用虚构的开发镜像摘要。部署命名空间首次创建后持久保存，重启不重新随机生成。

## 4. 上下文与内部预算

唯一最大上下文来源为 `KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS`，无值、非整数、溢出、非正数即配置错误。删除 `KB_TENDER_AGENT_LIMITS` 的读取、覆盖、Compose 和文档入口；不再通过 JSON 重复配置上下文或视觉开关。内部 `Limits` 仍是冻结执行配置，不删除预算保护本身。

删除固定 `PROFILE_CONTEXT_CEILING=131072` 对用户标量的第二重否决；131072 不代表自动发现的服务能力，用户负责提供所选模型可用窗口。实现通用数值/内存边界及供应商能力验证，不以放开上限为由允许无限分配。

默认输出预留上界 16384、安全余量 4096、图片估算预留 16384、原图最大 1500000 bytes/长边 1800。输出预留不等于发给供应商的 `max_tokens`。为支持小上下文，默认预留必须由唯一上下文派生：安全余量取 `min(4096, C/16)`、输出预留取 `min(16384, C/4)`，各至少 1；不足以容纳固定请求框架时在本地明确拒绝。显式高级覆盖不自动夹紧，非法关系报错。图片按有效模型策略计数；一张图片不能放入窗口时，受控缩放/合法裁剪后重计数，仍不适配就报告无法处理，不捏造已读回执。

未知模型不得要求普通用户配置 `KB_AUTHORING_TOKENIZER_PROFILE`。已验证模型使用内部映射；未知模型使用明确标记为估计的保守完整请求计数，包含工具、消息、图片、框架、安全与输出预留，不声称对任意供应商有精确保证。内部实测校准可由维护者更新，不作为用户必填。

发送前检查完整请求；超限由既有 planner 缩包、拆文本/网格并保持来源、表头、引用和顺序。供应商确认上下文超限后最多 3 次按较小容量重新规划，仅处理未完成范围；认证、余额和普通网络错误不伪装为上下文问题。每次物理调用照常记账，保持已完成回执，无法合法拆分明确失败。不得截断证据、无限重试或静默换模型。

## 5. 原图、OCR 与 readiness

招标原图复核始终使用配置的 Chat 模型，完整流程要求该模型有图像与工具能力；去掉用户 `vision_enabled` JSON 开关。能力由供应商资料/已维护配置确认，未知时明确指出待确认，不以模型名字猜测，不自动发送付费探测请求。

VLM 未配置时，仅当 Chat 已确认支持图像，才整组继承 Chat 地址、密钥、模型。显式 VLM 模型可在同一已确认端点复用其凭据；只要显式换端点，就要求独立凭据策略，绝不向新地址自动发送 Chat Key。独立 OCR 不能使不支持图像的 Chat 自动获得原图复核能力；能力不足返回清楚错误。

当前阶段标准部署固定为招标能力集合，无需用户额外填写能力 JSON。API/Worker readiness 检查 DB、schema/发布身份及其承担功能需要的 Redis、存储、DocReader配置/就绪；移除招标模式下无条件 Embedding 门槛。不得用空模型、stub 或不可用占位端点制造“已就绪”；readiness 不发付费模型请求。Neo4j、知识索引及 Embedding 仅在后续知识业务启用时加入检查，不删除其现存领域代码。

## 6. 其他参数默认与部署派生

| 参数组 | 默认/派生目标 | 外部覆盖 |
| --- | --- | --- |
| `POSTGRES_DB` | `knowledgebrain` | 可选 |
| `DATABASE_URL` | 从唯一账号、库名和部署拓扑安全组装 | 完整覆盖与拆分配置冲突时拒绝，密码正确 URL 编码 |
| `REDIS_URL` | 内部 Redis 服务地址，标准网络不公开端口 | 外部 Redis 可选 URI，含凭据时按秘密处理 |
| `DOCREADER_ADDR` | 容器内服务 DNS/端口；宿主用部署映射地址 | 可选，不混用两种网络 |
| `OBJECT_DIR`、S3 endpoint/bucket/region | 持久卷；内部 MinIO；`knowledgebrain`；`us-east-1` | 外部存储可选，凭据显式 |
| API/DocReader 内部端口 | 8080/50051 | 可选，同步健康检查与端口映射 |
| Worker probe | 当前默认 loopback:8081，容器按实际探测拓扑派生 | 可选 |
| 超时 | authoring 180000 ms | `KB_AUTHORING_TIMEOUT_MS` 可选 |
| 解析并发/招标队列并发 | 4/4 | 可选正整数及资源上限 |
| OCR 输出/区域深度/区域调用 | 8192/2/8 | 可选，保留完整性拒绝 |
| Core/Postprocess/Enrichment/Maintenance/Wiki/Multimodal 并发 | 8/2/12/4/8/12；只启动本阶段需要的队列 | 可选，不引入额外用户配额 JSON |
| DocReader worker/文件上限 | 4/50 MiB | 可选 |
| PDF 渲染并行/DPI/JPEG/长边 | CPU 派生 1–4/200/85/2000 | 可选 |
| PDF/XLSX 保护、临时目录、日志 | 保留已有解析安全默认；临时目录 `/tmp/docreader` | 高级可选 |
| ODL/外部解析/代理/SSRF 白名单 | 关闭/空 | 仅显式启用；不自动发外部请求或放宽白名单 |
| gRPC TLS/认证 | 私有单机网络由标准拓扑限定；跨网络按部署策略提供证书与内部随机 token | 不能开启 TLS 后因缺证书静默降级；不新增普通用户必填调参 |
| release descriptor/hash、component identity、namespace、queue registry | 构建/部署派生并挂载验证 | 非普通手填；外部发布来源须可信 |

固定密码、JWT、外部模型端点均不属于允许的默认。标准部署应只发布必要应用入口，数据库与存储管理端口默认内部可见。OnlyOffice 等不属于当前招标读取/大纲路径的服务使用独立功能配置，不列为本阶段必填；不能以清理 env 为由删除其业务代码。

## 7. 最小配置与可选覆盖示例

以下是**实施后的目标示例**，新增变量当前尚不可用；所有凭据均为占位符。

```dotenv
# 唯一应用数据库账号
POSTGRES_USER=<数据库用户名>
POSTGRES_PASSWORD=<数据库密码>
# 默认本地应用登录
KB_ADMIN_USERNAME=<管理员登录名>
KB_ADMIN_PASSWORD=<管理员密码>
JWT_SECRET=<独立JWT密钥>
# 标准自带对象存储
MINIO_ROOT_USER=<对象存储用户名>
MINIO_ROOT_PASSWORD=<对象存储密码>
# 用户选择的模型及其实际上下文
KNOWLEDGEBRAIN_CHAT_BASE_URL=<OpenAI兼容基础地址>
KNOWLEDGEBRAIN_CHAT_API_KEY=<模型密钥>
KNOWLEDGEBRAIN_CHAT_MODEL=gpt-6-luna
KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS=131072
```

该例只有在所选 Chat 模型确认支持完整视觉流程时才能继承为 OCR。不同服务时可追加：

```dotenv
KNOWLEDGEBRAIN_VLM_BASE_URL=<独立OCR服务地址>
KNOWLEDGEBRAIN_VLM_API_KEY=<独立OCR密钥>
KNOWLEDGEBRAIN_VLM_MODEL=<视觉模型>
# 可选运行覆盖，无需填写 limits JSON
KB_AUTHORING_TIMEOUT_MS=240000
KB_TENDER_PARSE_CONCURRENCY=2
# 外部基础设施按需覆盖，不能复制占位符当可用配置
DATABASE_URL=<外部数据库连接串>
REDIS_URL=<外部Redis连接串>
DOCREADER_ADDR=<该进程可访问的DocReader地址>
```

`DATABASE_URL` 完整覆盖不能与示例中的不同拆分凭据同时生效；应用按所选部署模式使用一个明确来源。外部 S3 时以其 endpoint/bucket/凭据替代内部 MinIO 配置。

## 8. 解析优先级与错误行为

进程注入值优先于显式指定 env 文件，最后是内部默认/部署派生；容器 `.env` 仅用于插值，不自动透传所有键，Compose 必须逐项列出支持的覆盖。宿主 helper 与 Compose 使用同一字段语义和网络适配，不能各自写一套默认供应商。

普通字符串 trim，空白视为未配置；秘密保留精确字节，不擅自 trim，拒绝空或全空白并避免写日志。必填缺失集中列出键名；可选缺失取默认；显式非法整数/布尔/URL/预算关系报错，不静默回退。删除旧字段和解析路径，不新建兼容层；配置文件中识别到已退役字段应提示移除，不再赋予其覆盖能力。兼容 `LLM_*` 别名如保留，应与主字段冲突时报错且不打印值；不作为新文档推荐接口。

模型与上下文等影响行为的有效值冻结到新运行；同一运行恢复不读取新环境改变语义。配置摘要仅展示非敏感值与来源，密钥、密码、带凭据 URI 只报告是否设置。

## 9. 实施顺序与验收

| 步骤 | 修改范围 | 必须通过的验收 |
| --- | --- | --- |
| 1. 收敛配置解析 | bidding runtime/agent、platform 配置、Compose/env 文档 | 删除 limits 入口；单一上下文冻结；未知模型不要求用户校准；空值/冲突/非法值单测 |
| 2. Fresh 单账号 | postgres-init、三 baseline、catalog/db、fresh 验收脚本 | 新空库初始化；只有一套应用登录身份；非 superuser；各组件使用同一 DSN；catalog/receipt 仍检查真实 schema 漂移 |
| 3. 默认登录与秘密 | API/platform auth、账号初始化、Secret 注入 | 正确密码登录、错误密码拒绝；重启不重置密码；JWT 缺失拒绝；不再有无密码开发回退 |
| 4. 模型能力与运行默认 | 原图/OCR、tokenization/planner、readiness、DocReader 注入 | 无 Embedding 的招标部署就绪；OCR 按能力整组继承；跨端点不泄露密钥；无付费健康探测 |
| 5. 全新部署集成 | 部署生成、示例与文档、CI | 只填最小配置可启动；容器/宿主地址各自可达；真实解析链集成使用受控 fixture；未授权不调用付费模型 |

预算测试覆盖多个上下文值、预留恰好等于/超过窗口、溢出、中文与 Unicode、长表格及表头支持、原图、供应商上下文拒绝后的有界重分包；不可分输入不截断或伪造完成。权限测试改为单账号语义，保留未认证业务访问拒绝、对象有引用时不能删除、业务 fence/审计不可绕过的调用路径测试。不能仅把旧角色测试删掉后宣称安全等价。

本 PR 的完成条件是设计可审阅、导航唯一、示例不含真实秘密；后续分别记录代码实现、聚焦测试、新空库集成、部署、真实业务验收。已存在的本地旧分支测试不能作为新配置设计的通过证据。

不提供存量升级/回滚脚本。不兼容部署只能选择独立新空库，停止失败的新实例不会触碰旧实例数据。任何清库或删除卷都不属于本方案授权。
