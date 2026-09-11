# 部署 namespace 隔离与 R2 前置修复

R2 reset 必须先能确定各后端的数据归属。此前 PostgreSQL receipt 和 Oxana 已验证 canonical deployment UUID，但 Neo4j 的节点、关系、查询及删除仅按文档/版本身份筛选，没有部署条件。专属真实 Neo4j 测试复现了问题：部署 A 写入后，部署 B 使用同一文档/版本 ID 能读到 A 的数据。

## 已实现

`platform::DeploymentNamespaceV1` 统一校验 canonical lowercase UUID，字符串解析及 JSON 反序列化都拒绝大小写、空白、无连字符等别名。来源仍为 `KB_DEPLOYMENT_NAMESPACE_ID`；配置缺失不生成默认部署。现有 release identity 与 Oxana 校验复用该类型，receipt 的 UUID 类型保持原值。最初 Neo4j 切片保留原队列 namespace；后续 Redis 切片已落实方案中的派生前缀，见下文。所有切片均未部署或改写既有资料。

Neo4j 沿用现有单数据库 HTTP/Cypher 封装，采用[已确认方案](../../plans/platform/runtime-foundation.md#51-deploymentnamespacev1-与-reset)允许的属性隔离方式：

- 写节点的 MERGE 身份与 entity key 包含部署 namespace。
- 写边时两端节点均限定相同 namespace，边自身也写入该属性。
- 查询及文档删除都将 namespace 作为参数，与版本/文档条件共同使用。
- 公共操作从环境解析一次 typed namespace 后传给内部操作，不接受任意字符串前缀，也不从文件名或正文猜测部署。

旧无 namespace 的图资料不会自动被认领、读取或删除；它们的处理需要明确的独立数据操作。本轮没有操作现有业务图数据库，没有新 migration、依赖库、后端或业务规则。尚未将新代码构建为发布镜像或部署；此前 R1 镜像及运行证据绑定旧源码，不能充当本次改动的镜像验收。

## 验证与证据

原始执行目录 `/tmp/kb-r2-graph-75tefr87/`，归档副本见 [`artifacts/namespace-isolation/`](../../artifacts/namespace-isolation/)。测试只使用唯一标签 Neo4j 5.26 容器、随机回环端口和生成的图数据。

| 验证 | 结果及范围 |
| --- | --- |
| 原缺口复现 | 新隔离测试在修复前 exit 101，另一部署读取结果不为空；`red.log` |
| 真实图隔离 | 修复后两个必跑 live 用例通过，无 ignored/skip：既有写入/查询/删除，以及两个 namespace 使用相同文档 ID 时的查询、重写、节点/关系归属、删除保护和旧无 namespace 数据保留；`green.log` |
| 类型及兼容性 | 新类型 JSON/字符串别名拒绝、既有 release 身份合同、既有 Oxana namespace 分区校验各 1/1；`unit-exits.json` |
| Clippy | platform/knowledge 全目标、全特性 `-D warnings` 通过；`clippy.log` |
| 完整 Rust 工作区 | `cargo test --locked --workspace --no-fail-fast` exit 0，589 passed、0 failed、10 ignored；fmt 通过。未配置基础设施时提前返回的既有测试不作为集成验收；`workspace-tests.log` |
| 清理 | 核验唯一容器 ID、标签和两个卷的独占使用后清理，owned runtime 残留 0；`cleanup-verification.json` |

首次清理保护检查因为 Docker 返回相同挂载的数组顺序不同而停止，未执行删除。修正为比较完整挂载身份集合后清理成功；保留首次失败记录，不放宽身份或独占条件。

新的图隔离用例是显式环境测试，默认 ignored；必跑时必须提供专属服务配置和 `KNOWLEDGEBRAIN_REQUIRE_NEO4J_TESTS=1`，使用 `--include-ignored`。缺服务或 require flag 时失败，不能计作通过。本轮实际必跑日志是 `2 passed; 0 failed; 0 ignored`；完整默认工作区测试不替代这项真实服务证据。

## R2 剩余实施

当前仍没有可用的 `deploy/reset-namespace.sh`。需要继续落实后端实际 mapping 核对、runtime drained 与 receipt gate、确认 token、mode-0600 原子且 fsynced checkpoint，再按既定顺序实现五个后端的受保护删除及逐步故障恢复。

对象隔离及 Redis 派生前缀是当前源码行为，尚未部署，不能凭 receipt UUID 假定既有安装的数据已处于新布局。PostgreSQL 的派生库名与 reset 只读核对已实现，见下文；全部后端实际 mapping 与 runtime 排空仍须组合落实。旧无 namespace 或旧布局资料不进入自动 reset；缺 checkpoint、配置/身份不符或 PostgreSQL 已删但没有合法恢复凭据时必须拒绝执行。后续须用专属环境验证每个 crash 边界，不能把归属前置修复标为 R2 完成。

## OBJECT_DIR 与 MinIO 隔离补充

本地路径为 `<OBJECT_DIR>/kb-<namespace UUID 无连字符小写值>/objects/<文件名>`，MinIO key 为同一 namespace 下的 `kb-<值>/objects/<文件名>`；逻辑 `objects/<摘要>` 引用保持不变。解析 Markdown 缓存的 `.md` 后缀沿用既有约定，处于相同部署目录。路径前缀来自已确认的基础设施协议，不是招标章节、行业分类或样稿特判。

生产读写必须显式配置非空 `OBJECT_DIR` 与合法 `KB_DEPLOYMENT_NAMESPACE_ID`，不生成默认部署，不回退读取旧根目录或旧 MinIO key。`LocalObjectStore` 在 Unix/Linux 上使用逐级 `openat`、`O_NOFOLLOW` 和目录描述符访问；拒绝路径穿越、符号链接、非普通文件及多硬链接文件。写入使用同目录随机临时文件、文件 fsync、原子 rename 和目录 fsync。配置根目录本身及中间层也不能是符号链接；这不是对拥有目录重命名权限或管理员挂载能力的隔离承诺。

MinIO 沿用已有 S3 签名和传输封装，以同一个 typed namespace 构造物理 key；凭据必须来自环境配置，删除 Rust 中 `minioadmin` 默认凭据。平台直接复用工作区已锁定的 libc，Cargo.lock 只增加 platform 对既有包的依赖边，没有新包版本或 migration。

图片读取已改为统一 `platform::read_blob`，不会再直接拼接 `OBJECT_DIR`。只有本地 NotFound 才尝试所属 namespace 的 MinIO 并恢复安全缓存；路径或权限异常不会通过远端读取绕过。Retention 同样限定本地及远端对象归属。上传使用异步存储入口，只有写入成功后才返回身份并继续持久化文档记录、入队；删除无人调用且会吞写入错误的旧内存/磁盘双写入口。

本轮原始目录 `/tmp/kb-r2-objects-i1qmd7z4/`，证据见 [`artifacts/namespace-isolation/objects/`](../../artifacts/namespace-isolation/objects/)。3 项本地用例覆盖双 namespace 的源文件/Markdown 隔离、读写删除、路径穿越、逐层符号链接和硬链接保护。知识库子进程回归验证存储成功、缺目录配置、缺 namespace 和目录不可用四种情况；worker 子进程测试验证真实 helper 的新路径、错误输入与停止后不再写入。

唯一标记临时 MinIO 上 3 项必跑用例全部通过，0 failed、0 ignored：既有 roundtrip、两个 namespace 使用相同 key 的读写删除与旧对象保护、实际缓存回源及 Retention 删除。用例要求 `KNOWLEDGEBRAIN_REQUIRE_S3_TESTS=1`，缺前置直接失败。测试只使用生成数据和凭据，核验 ID、标签与卷独占归属后清理，owned runtime 残留 0。没有运行真实模型、修改 `.env` 或构建/部署新发布镜像。

对象隔离补充后的完整质量门禁：fmt、全工作区全目标/全特性 check 和 Clippy `-D warnings` 均 exit 0；完整默认 Rust 测试最终 594 passed、0 failed、12 ignored，exit 0。默认结果仍不代替基础设施集成验收。第一次完整测试的 4 个失败 target 均在本地监听处遇到沙箱 `Operation not permitted`，获准回环监听后通过。验收期间另一处共享源码修改产生 fmt 差异；统一格式后重新执行完整测试/Clippy，并核验其间源码摘要无变化。两类首次失败日志与最终通过日志均保留；详见归档 `quality-exits.json`、`manifest.json` 与 `verification-source-before.json`。

## Redis/Oxana 与多模态计数器隔离补充

Oxana 2.1.3 的官方 StorageBuilder 接收 `kb:<32 lowercase hex>`，由其官方实现追加分隔符，物理键前缀为 `kb:<32hex>:`。`DeploymentNamespaceV1` 统一导出 namespace/prefix；生产连接及原生故障测试复用同一 `oxana_storage_in_namespace` 入口。没有 fork/vendor、依赖版本变化、队列调度封装或 migration。原有唯一身份、重试与 crash resurrection 仍由 Oxana 负责。

原 Redis 地址默认值已删除，生产连接必须提供非空 `REDIS_URL` 与合法 UUID。测试也不再探测固定本机 Redis 端口。知识库的多模态计数器此前使用无部署前缀的 `multimodal:pending:<document>`，现同样通过 typed namespace 构造键；配置缺失不会连接共享 Redis 或写无归属键。计数器既有内存回退/Redis 出错处理语义未在本轮重设计，此次隔离测试不证明这些故障路径的业务完成正确性。

原始执行目录 `/tmp/kb-r2-redis-mtq5yffw/`，归档见 [`artifacts/namespace-isolation/redis/`](../../artifacts/namespace-isolation/redis/)。修复前 unit 红色证据显示实际 Storage namespace 仍为 raw UUID，与合同中的 `kb:<32hex>` 不符；修复后通过。首次编译发现新计数器测试的 Redis DEL 参数需借用数组，修正后继续运行；保留该失败日志。

两项显式 ignored 隔离用例必须在独占测试服务上设置 `KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS=1` 并 `--include-ignored` 执行，缺前置直接失败：

- 官方 Storage 实测相同 unique job identity 在两个部署的写入、去重、读取和删除互不影响；旧 raw UUID namespace 的测试任务不被新部署认领。只枚举物理键名核对归属前缀，任务状态及删除都调用官方 API，不解析或操作私有队列存储格式。
- 计数器实测通过独立子进程加载配置，同一 document ID 的 SET/GET/DECR/DEL 互不影响；旧无前缀测试键保持原值，缺 namespace 时不能改写它。

另以真实 Redis 执行 4 项既有 Oxana native faults（unique Skip、crash resurrection、housekeep 不复活、dead revive）及 14 项 typed producer/队列契约。首轮已通过并清理；删除默认地址回退后，在第二个全新独占 Redis 中复验最终代码，结果与资源清理见 `live-final/`。新源码尚未构建为运行镜像或部署，既有 raw UUID 队列不自动迁移/消费；正式切换不能混用两种布局。

最终四个必跑套件分别 1/1、1/1、4/4、14/14 通过，均 exit 0、0 failed、0 ignored；两轮服务各自核对容器 ID/标签及卷独占使用后清理，owned runtime 残留均为 0。最终 fmt、全工作区全目标/全特性 check、Clippy `-D warnings` 和完整默认 Rust 测试通过，594 passed、0 failed、14 ignored。新增两项 ignored 用例的真实执行由上述专属日志单独证明；默认 workspace 结果不替代 PostgreSQL/DocReader 等集成验收。最后一轮源码摘要保持不变，见 `verification-source-final.json` 与 `quality-exits.json`。

## PostgreSQL reset 只读前置校验

`NamespaceResetRequest` 只接受精确 `kb-<32 lowercase hex>` 标签、现有 release 合同允许的 revision 和匹配的 uppercase SHA-256 确认校验和。复用既有 revision 校验，不另定义一套语法。`canonical_namespace` 在 reset 确认公式中明确指 CLI 的规范 `kb-<32hex>` 标签；数据库名由同一 `DeploymentNamespaceV1` 派生为 `kb_<32hex>`，不是调用方传入的任意字符串。测试向量核对域分隔、两个 NUL 分隔符和全部字段；大小写/空白/标签别名、另一个 namespace 或 revision 都被拒绝。该校验和不是身份认证，也不表示已经授权删除。

`verify_namespace_reset_postgres` 使用专属连接和 `REPEATABLE READ READ ONLY` 事务：

- 先核对 request 与完整 release identity 的 namespace、revision、descriptor 摘要。
- 从实际连接读取 `current_database()`、数据库 OID/owner；必须精确匹配派生名称且不是 template 数据库。
- 开始完整 catalog 查询前先拒绝其他目标数据库连接，避免在仍运行的业务事务锁上等待；结束前再次清除统计快照并核对连接数。
- 直接复用现有 `verify_runtime_schema_on_connection`，验证 receipt、三个 baseline 摘要、完整 catalog、扩展及 PostgreSQL 版本；没有第二套 receipt 解析或弱化校验。
- 从 PostgreSQL `pg_control_system()` 读取实际集群 system identifier，与数据库 OID/owner、完整 receipt 一起形成只读观察结果；权限不足或查询失败停止，不猜集群身份。

连接数为零只是某时刻的数据库观察。它不能证明 API/worker/retention 已停止，也不能阻止未来重连；调用方仍须在全后端 reset 协调中验证并维持 runtime 排空、复核映射和写入 checkpoint。这一结果类型没有删除方法。本轮没有 `DROP DATABASE`、checkpoint 写入或 `deploy/reset-namespace.sh` 可执行入口；也没有自动修改 `.env`/Compose 的库名或迁移既有安装。旧库名继续被本 preflight 拒绝，不能把此项验证当成已经切换现有运行环境。

原始目录 `/tmp/kb-r2-pg-preflight-l8zff3r5/`，证据见 [`artifacts/namespace-isolation/postgres-preflight/`](../../artifacts/namespace-isolation/postgres-preflight/)。独占 PostgreSQL 16+pgvector、随机回环端口和新 UUID 派生库名，角色初始化及三个 baseline 均复用现有入口。必跑 `namespace_reset_postgres` 1/1 通过，0 failed、0 ignored，实际覆盖：

| 条件 | 断言 |
| --- | --- |
| 合法 receipt、实际映射、无其他连接 | 只读检查成功，cluster/database/OID 等于直接查询值 |
| 重复检查 | PostgreSQL session 强制只读时仍成功，观察结果及 receipt 完全不变 |
| 连到其他数据库 | DatabaseMapping 拒绝 |
| request/release namespace 或 revision 不同 | Identity 拒绝 |
| 另一个 namespace 与自身 release 相符但实际库不同 | DatabaseMapping 拒绝 |
| 目标数据库仍有另一个连接 | DatabaseSessions 拒绝；关闭该连接后才继续 |
| receipt namespace/revision 被修改或 receipt 行缺失 | 复用 schema gate 拒绝 |
| 增加索引导致 catalog 漂移 | 复用 schema gate 拒绝；仅恢复本测试索引后再次检查通过 |

所有故障只注入本轮刚创建的测试库，结束时 receipt 恢复并核对一致。容器 ID、标签及匿名卷独占性核验后清理，owned runtime 残留 0。首次启动请求因审批服务 HTTP 429 拒绝且脚本未执行；核对并记录唯一资源/随机端口/无既有资料操作范围后，重试获准。首次测试编译发现 SQLx 0.9 要求动态测试索引 DDL 显式确认，已核对名称仅由固定测试前缀和生成 UUID hex 组成；保存首次编译失败和最终必跑成功日志，没有通过关闭检查规避。

最终 fmt、全工作区全目标/全特性 check、Clippy `-D warnings` 和完整 Rust 默认测试均通过：595 passed、0 failed、15 ignored。新增 PostgreSQL 用例的必跑日志另证 1 passed、0 failed、0 ignored；默认结果不替代服务验收。完整验证期间源码摘要一致，见 `verification-source.json`、`quality-exits.json` 和归档 `manifest.json`。


## 本地对象 reset 只读前置校验

`verify_namespace_reset_objects` 接收已校验的 `NamespaceResetRequest` 和显式绝对对象根目录，复用 `LocalObjectStore` 的逐层目录描述符打开方式。根目录、派生 namespace 目录和 `objects` 目录均通过 `O_NOFOLLOW | O_DIRECTORY` 打开；不会以字符串拼接后的普通读取替代 containment 检查。返回实际设备号和 inode，记录配置根路径及 typed namespace，后续可据此发现同一路径下的目录被替换。

配置根目录必须存在，文件系统根、相对路径、非 UTF-8 路径和 `..` 被拒绝。namespace 或 `objects` 子目录尚不存在时，返回明确的 `None`，不创建目录；普通文件、符号链接或其他打开错误不能冒充“不存在”。这是只读观察，文件描述符在返回前后不作为长期锁持有，数字 inode 也不是删除能力。完整 reset 协调仍须确认 runtime 排空、全后端 mapping、receipt 和持久 checkpoint，并在使用路径时重新安全打开和核对；本轮未增加任何删除操作或可执行 reset 入口。

三个实际文件系统回归覆盖：目录 inode 与直接 stat 相等、目录替换后身份变化、其他 namespace 文件不变；缺失根/子目录不创建与非目录拒绝；根、namespace、objects 三层符号链接均拒绝且外部哨兵文件不变。与原确认校验和测试一起定向 **4/4** 通过。全工作区 fmt、全目标/全特性 check、严格 Clippy 通过，默认 Rust 测试日志合计 **604 passed、0 failed、15 ignored**；其中包含既有对象读写/删除隔离测试，ignored 不作为服务验收通过。

证据：[`artifacts/namespace-reset-objects/`](../../artifacts/namespace-reset-objects/)，原始目录 `/tmp/kb-r2-object-preflight-qo6lbleu/`。仅修改两份平台 Rust 文件，复用了已有对象访问原语，没有新 migration、环境配置更改、业务数据操作或部署。R2 的其余 backend mapping、排空、checkpoint、执行入口和 crash 恢复仍未完成。


## Redis reset 只读前置校验

`verify_namespace_reset_redis` 复用调用方已有的 Redis client 配置与 `DeploymentNamespaceV1::redis_prefix()`，在独立连接中读取 `INFO server`、`INFO replication` 和 `CLIENT INFO`。实际连接的 `db=` 必须等于配置数据库，服务必须报告有效 run ID、standalone 模式和 primary 角色；不支持的拓扑、身份缺失、权限不足或调用方指定时限耗尽均拒绝，不回退到猜测值。

观察结果只序列化 TCP/TLS/Unix 连接位置、实际数据库、部署前缀和服务 run ID，不包含用户名/密码；拒绝不校验证书的 TLS 和非法 Unix 路径。服务重启后的 run ID 改变需要后续协调器重新核对，不能静默认作原服务。这些结果不反序列化为删除凭据，也不能证明 runtime 已排空；不扫描 Oxana 私有键，不写入或删除业务键。

在新建、唯一标签、随机回环端口、tmpfs 数据目录的 Redis 7 实例中显式运行服务用例，**1 passed、0 failed、0 ignored、0 filtered**：使用非默认 DB 2，另核对 DB 3；只读 ACL 用户允许 INFO/CLIENT INFO/PING/SELECT 且 SET 被拒绝时仍能预检；重复观察与直接读取的服务身份相等；不同数据库及 namespace 保持区分；序列化不含测试凭据；撤销 CLIENT INFO 权限后拒绝；目标、其他部署及旧布局哨兵全部不变。测试清理精确键和 ACL 用户，驱动再验证容器归属并销毁本轮实例，清理错误为零。

证据见 [`artifacts/namespace-reset-redis/`](../../artifacts/namespace-reset-redis/)，原始目录 `/tmp/kb-r2-redis-preflight-v1i9zwjf/`。保留首次编译借用错误及最终通过日志；fmt、全目标/全特性 check、严格 Clippy 全过，默认 Rust 测试 **604 passed、0 failed、16 ignored**。默认 ignored 计数不替代上述实际服务验收。MinIO/Neo4j mapping、持续排空 gate、持久 checkpoint、reset 入口和逐步故障恢复仍待实现，R2 未完成。
