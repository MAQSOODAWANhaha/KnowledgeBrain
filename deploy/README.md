# KnowledgeBrain 部署

默认招标栈：API、Worker、Retention、DocReader、PostgreSQL、Redis、MinIO。
Neo4j（`knowledge` profile）与 OnlyOffice（`editing` profile）不随招标栈自动启动。
原图复核使用 Chat；OCR 可继承已确认支持图像的 Chat，或显式指定独立 VLM。

默认 Compose 项目名为 `kb`，容器统一命名为 `kb_<服务名>`，例如 `kb_api`、`kb_postgres`。
项目的网络和数据卷也使用 `kb` 命名空间。Python 招标工作流通过同一 Compose 的
`python-tender` profile 启用，复用 PostgreSQL、应用数据库账号和 Redis；不再单独部署
数据库、Redis 或配置另一组连接地址。

## 最少配置

复制 `.env.example` 后，仅明确填写：

- `POSTGRES_USER/PASSWORD`：唯一应用数据库账号，所有应用组件共用；不得使用 `postgres`。
- `KB_ADMIN_USERNAME/PASSWORD`：本地登录管理员（选择 LDAP 时不要求本地引导）。
- `JWT_SECRET`：独立 JWT 密钥，没有固定秘密回退。
- `MINIO_ROOT_USER/PASSWORD`：标准内置对象存储账号；外部 S3 使用自己的访问凭据。
- `KNOWLEDGEBRAIN_CHAT_BASE_URL/API_KEY/MODEL` 和整数 `KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS`。

上下文 `131072` 是用户当前选择，而非永久代码上限。其余预算、超时、并发使用默认；
无需 `KB_TENDER_AGENT_LIMITS` 或 tokenizer calibration JSON。Python 图片处理独立读取
`KNOWLEDGEBRAIN_VLM_BASE_URL/API_KEY/MODEL`，三项均需配置，不自动复用 Chat 配置或按模型名猜测能力。
招标模式 `KNOWLEDGEBRAIN_KNOWLEDGE_ENABLED=false`，不以空 Embedding 地址伪装就绪。

配置优先级为进程环境、显式 env 文件、程序默认。Compose `.env` 仅提供插值，支持的覆盖
在 Compose environment 中逐项声明。密码保留原字节，含 `$` 或 `#` 的值在 env 文件使用单引号。
不要将实际密码、密钥、带凭据 URI、渲染后的 Compose 配置贴入日志或提交仓库。

Compose 统一传递 `KNOWLEDGEBRAIN_CHAT_*`、`KNOWLEDGEBRAIN_VLM_*` 和
`KNOWLEDGEBRAIN_EMBEDDING_*`，不再向容器重复传递 `LLM_*`/`EMBEDDING_*` 别名。
API 接收登录、LDAP 和 OnlyOffice 配置；Rust/Python worker 共享模型配置；Retention、
Python 初始化和 dispatcher 仅接收各自需要的存储、队列或数据库配置。
已移除没有消费者的 `KNOWLEDGEBRAIN_SHARED_CONCURRENCY`，其余仍在使用的并发配置保留。

### 数据库与网络

标准初始化 wrapper 仅在新卷初始化数据库：引擎管理员 `postgres` 无密码且只允许容器本地
Unix socket 管理，TCP 使用 SCRAM；用户提供的应用账号无 superuser/建库/建角色权限，拥有应用
schema，所有应用进程共享此账号。`migrate` 是独立的一次性 fresh baseline 步骤，API/Worker
不执行 DDL。共享账号意味着不再有运行角色级权限隔离；业务授权、租约和引用保护仍保留。
已有不兼容数据库拒绝启动，不迁移、接管、清库或重置旧账号。

Rust 从 `POSTGRES_HOST/PORT/DB/USER/PASSWORD` 安全组装一个连接；无需用户 URL 编码密码。
完整 `DATABASE_URL` 属于外部/宿主高级覆盖，若同时设置 split 字段必须一致，否则拒绝。
标准 Compose 本身包含默认内部 split 拓扑，因此外部覆盖应同步这些字段，或使用专用 overlay
移除它们和内置数据库依赖。外部服务应由其管理员预建扩展及应用账号，不能自动夺取管理权限。

默认仅发布浏览器/API，数据库、Redis、MinIO 和 DocReader 不映射宿主端口。
容器连接使用服务名；宿主开发必须用明确的 loopback 端口映射 overlay 和对应连接参数，
不可把 `postgres`、`redis` 等 Compose DNS 直接交给宿主进程。
S3 仅在默认内部 `http://minio:9000` 且未提供访问凭据时派生 MinIO 账号；外部地址要求独立凭据。
跨主机 DocReader 的 TLS/证书策略需显式部署 overlay，禁止缺证书时降级；标准私有单机网络
可选 `GRPC_AUTH_TOKEN` 会同时注入客户端与服务端。

Python 使用同一组 `.env` 的 `POSTGRES_*` 值，由 Compose 映射为 libpq 标准
`PGHOST/PGPORT/PGDATABASE/PGUSER/PGPASSWORD`，避免在 Compose 中拼接带密码的 URL；
显式 `DATABASE_URL` 也可复用。Redis 使用相同 `REDIS_URL`，Dramatiq 的
`python-tender` namespace 与 Rust 的 `kb:` 前缀区分。Python 只初始化自己的 `tender_*`
运行表及 LangGraph checkpoint 表，产物由三个 Python 角色共享 `python-tender-artifacts` 卷。

## 招投标编辑接入状态

OnlyOffice 不属于招标读取／Discover／Organize／Check 的前置条件。需要编辑时显式启用
`editing` profile，提供其 JWT/能力密钥及浏览器/API origin；没有固定秘密回退。
其现有业务与验收边界见 [接入计划](../plans/bidding/onlyoffice-integration.md)。

## Fresh 启动

当前尚未发布，按 [未发布 baseline 策略](../plans/platform/runtime-foundation.md#2-fresh-baseline)直接修正三份 migration；不建设历史升级链。当前运行方式只支持 clean-slate fresh deploy，不承诺发布后的保数据升级。修改 baseline 不授权对运行库执行迁移、清库或部署。

`migrate` job 在一个事务中按
`shared_platform_baseline` → `knowledge_base_baseline` → `bidding_v2_baseline` 建立应用表和函数。初始化不生成 schema snapshot、catalog manifest 或结构摘要；API、Worker、Retention 不比较 schema 版本/hash，也不执行 DDL 或在线修复。

```bash
cp deploy/.env.example deploy/.env
# Fill required values; use fresh isolated data only.
./deploy/deploy.sh create
```

启用 Python 工作流（复用同一项目内的 PostgreSQL/Redis，缺少时启动，并先完成共享数据库初始化）：

```bash
docker compose --env-file deploy/.env \
  -f deploy/docker-compose.yml -f deploy/docker-compose.local.yml \
  --profile python-tender up -d --build python-tender-worker python-tender-dispatcher
```

先由 `migrate` 完成共享数据库 baseline，再由 `python-tender-init` 创建 Python 表及
checkpoint，最后启动 worker/dispatcher，避免 Python 表先建立而触发空库初始化拒绝。
三者没有新增宿主端口。该 profile 不自动切换 Rust 链路，也不会自行提交文件或调用模型。
原件提交、状态核对和人工确认见[工作流运行手册](../docs/design/python-tender-phase1/runbook.md)。

生产发布使用 `deploy/release-descriptor-v1.json`：

```json
{
  "schema_version": 1,
  "release_revision": "...",
  "git_sha": "40-lowercase-hex",
  "images": {
    "migrator": "registry/name@sha256:<64hex>",
    "api": "registry/name@sha256:<64hex>",
    "worker": "registry/name@sha256:<64hex>",
    "retention": "registry/name@sha256:<64hex>",
    "docreader": "registry/name@sha256:<64hex>"
  }
}
```

字段 exact、`additionalProperties=false`，images key set 固定，JSON 键顺序由 JCS 规范化。`release_revision` 使用 exact ASCII grammar `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`（1～128 bytes，不 trim/normalize）。`release_descriptor_sha256=SHA256("KB:ReleaseDescriptor:v1\0" || RFC8785_JCS(document))` 是整组组件 identity，不把多个 image 误称为“一个 image digest”。入口复用平台 typed validator/JCS，并核对 Compose 同目录的 checked schema 与编译内嵌合同一致；缺文件、重复字段或合同漂移均拒绝。

宿主发布入口为 `knowledgebrain-release`，边界和当前候选门槛见[发布入口](../docs/bidding/release-entry.md)。旧本机结果不等于候选或生产放行。从审核后的候选构建并将 binary 安装到 PATH：

```bash
cargo build --locked --release -p platform --bin knowledgebrain-release
```

入口只接受显式 descriptor、env file、Compose project；也可用 `KB_RELEASE_TOOL` 指向已构建的宿主 binary。以下是验收完成后的唯一发布命令：

```bash
cp deploy/.env.example deploy/.env
echo "$GITHUB_TOKEN" | docker login ghcr.io -u USER --password-stdin
knowledgebrain-release --descriptor deploy/release-descriptor-v1.json \
  --env-file deploy/.env --project-name <explicit-project>
```

添加 `--check-only` 只验证本地输入并计算 hash，不调用 Docker、不验证镜像或数据库。`--compose-file` 默认 `deploy/docker-compose.yml`，`--timeout-seconds` 默认 300，为每条 Docker 命令设置上限。

实际执行先解析 Compose、把所有容器名限定到显式 project，检查已有组件/依赖的身份和健康状态，再 pull 并 inspect 全部 digest 引用。mode-0700 临时目录内的 mode-0600 override 写入五组件 exact image，四个 Rust 组件额外写入 `BIN`。先等待 Compose 数据面及 DocReader，再运行 migrator，确认其 image/config、环境和 exit 0 后才启动 API/Worker/Retention。最后逐组件 inspect，并实际执行配置中已核对一致的健康探针。镜像 manifest RepoDigest 与本地 image config ID 分别校验，不能混用。

发布工具在宿主侧验证实际镜像及健康探针，不向数据库写入 release receipt；应用不要求 descriptor 环境变量或挂载。已有部署不一致时拒绝自动替换；全部服务符合时只检查，不重新初始化或启动。失败只停止本次标签且 project 一致的容器，保留既有容器与数据卷。

checked-in `release-descriptor-v1.development.json` 仅用于离线工具测试；其 synthetic digest 不证明本地 image 的实际 RepoDigest，标准 Compose 不读取它。旧 `docker-compose.pro.yml` 和 `.env.example` 中的 tag 发布配置已删除。验证范围、可复现命令及剩余验收见[发布入口验证](../docs/bidding/release-entry.md)。

`api`、`worker`、`retention` 都等待 `migrate` 成功。应用进程启动时只连接依赖，不执行
DDL。重启使用：

```bash
./deploy/deploy.sh up
```

对象存储使用显式 `OBJECT_DIR`，本地布局为 `<OBJECT_DIR>/objects/<文件名>`，S3 key 为 `objects/<文件名>`。不同安装之间使用独立的数据库、Redis 数据库、S3 bucket 和对象目录；同一安装的 Rust/Python 角色共用 PostgreSQL 和 Redis。启用图谱时使用独立 Neo4j 数据库。路径穿越、符号链接与 ObjectRegistry 引用保护继续校验。仅支持全新安装，不提供旧布局兼容、迁移或跨后端 reset 编排。

Redis 队列入口要求显式 `REDIS_URL`，不回退固定本机地址；Oxana 使用固定应用前缀 `kb:`，生产者和消费者必须连接同一安装的独立 Redis 数据库。

### 本地全量清理、创建、重启与日志

本地联调统一使用 `deploy/deploy.sh`：

```bash
# 永久删除本 Compose 项目的容器、网络及 PostgreSQL/Redis/MinIO/Neo4j 数据卷。
./deploy/deploy.sh delete

# 使用 Docker/BuildKit 缓存构建；未变化的层不会重新编译，再创建或更新有变化的服务。
./deploy/deploy.sh create

# 不构建、不清数据，按当前 compose 配置 up。
./deploy/deploy.sh up

# 修改 deploy/.env 后使用：不构建、不清数据，强制重建容器以重新加载环境变量。
./deploy/deploy.sh restart

# 实时查看本 Compose 项目全部服务日志；按 Ctrl+C 停止。
./deploy/deploy.sh logs
```

`delete` 不会删除 Git 工作区、Docker 全局镜像或其他 Compose 项目的数据卷，但它会删除当前本地 Compose 项目的全部数据面；只允许用于明确的本地 clean-slate 验收，不是生产清理命令。

> Oxana 的 `Job finished success=true` 表示消息已被 Worker 确认处理，不能证明业务生成成功。招标分析应核对请求终态、独立复核结果和已发布的分析版本。执行诊断位于 `bid_tender_agent_run_artifacts`；诊断沿用 SQL 的 UTF-8 安全字节前缀，不承诺保留完整原始消息。旧大纲生成任务已从注册表和 API/SQL 创建链删除。


## 本地入口与可选网络覆盖

`deploy.sh` 叠加 `docker-compose.local.yml`：Caddy 提供本地自签 HTTPS，默认浏览器入口
为 `https://localhost:28080`（`API_HOST_PORT` 可覆盖）；证书由现有本地部署步骤准备。
基础 Compose 直接映射 HTTP API `API_HOST_PORT` 默认 28080；内部 `API_PORT` 默认 8080，
端口映射、Caddy upstream 和健康检查同步使用该值。DocReader 内部端口默认 50051，
`DOCREADER_GRPC_PORT` 同步客户端默认地址与探针。数据服务默认没有主机映射。

OnlyOffice 可选 profile 的浏览器入口为 28081（`ONLYOFFICE_HOST_PORT`）；Neo4j 仅在明确启用
knowledge profile 时启动，HTTP/Bolt 宿主端口默认为 27474/27687。启动前核对宿主监听及
其他 Compose 项目的端口映射；如需覆盖端口，在现有 `.env` 中设置对应变量，并同步
OnlyOffice 的浏览器 origin。
本仓库的开发 release descriptor 不能当作生产身份。
生产走上文说明的 release 工具并提供真实 descriptor；禁止以伪造摘要绕过校验。

## 招投标 V2 Worker

`bid-authoring-v2` 物理队列注册以下五类粗粒度任务，且都必须有 active handler：

- `bid:tender_document_process:v2`
- `bid:requirement_set_compile:v2`
- `bid:docx_compose:v2`（用户触发填充当前已保存 DOCX，复用冻结大纲运行合同；整链验收状态见统一方案）
- `bid:content_generate:v2`
- `bid:submission_export:v2`

不得恢复 EvidenceMatch continuation、旧 Part/Gate Job 或 default queue fallback。队列声明以
`deploy/queue-registry.toml` 为唯一运行注册表，启动时会校验 handler、payload 和唯一身份公式。

## 故障恢复

- TenderDocument 技术解析失败：修复 DocReader/OCR/VLM/ObjectRegistry 后，由用户调用 V2 retry；不要改表或复用旧请求身份。
- Requirement/Composition/Content/Export delivery 使用原 frozen Request。首次 POST 或用户显式同 idempotency-key replay 均 exact 重载同一 payload 并调用 pinned Oxana 2.1.3 official `enqueue`；Redis unavailable/unknown 返回可重试 503。PostgreSQL 不保存 delivery reservation/budget 或扫描 Request 恢复 transport；Stage receipt 与 AgentRun owner fence 保证同一输入幂等。
- Workspace CAS 返回 409 时，客户端重新 GET 当前 ETag 后显式重放用户意图；服务端不得覆盖并发编辑。
- `NO_EVIDENCE`、未覆盖要求等业务 Assessment 只产生 warning；Schema、CAS、资产 digest、事务和 renderer 错误必须 fail-closed。
- 失败上传由 retention 回收 `object_upload_staging`；已进入 Artifact/Manifest 的对象通过 ObjectRegistry owner reference 保留，禁止直接删除对象或业务行。
- 不兼容数据库不做在线修复、迁移或自动清理。新实例使用独立新空库；初始化失败先核对错误和资源范围，不能清除已有业务数据。

更详细的招投标运行与验收见 [`../docs/bidding/backend-runbook.md`](../docs/bidding/backend-runbook.md)。

## 健康与验收

以下仅记录已存在、获准保留的实现增量与历史本地验证，不代表本轮重跑或整体运行验收。方案已确认；普通实施与隔离开发验证的任务及授权边界见[实施台账](../plans/implementation-tasks.md)，不含采购、生产部署、现有数据操作或提交。

`schema-contract` 验证 fresh DDL 与单账号权限，旧 catalog/receipt 漂移测试已删除。业务清理验收继续按 [平台 §6](../plans/platform/runtime-foundation.md#6-retention-consumer) 区分交接、实际消费和最终回收；不得削弱引用保护、幂等和失败恢复断言。

- `GET /live`：进程存活；
- `GET /ready`：PostgreSQL、维护门和 queue registry 可用；
- `GET /api/v1/ops/queues`：共享平台队列（不是招投标 V1 API）；
- `scripts/fresh_schema_acceptance.sh`：仅面向显式授权的隔离 PostgreSQL 空库，验证单一非超级用户应用账号、fresh DDL 和数据库权限边界。运行前核对脚本限制，不连接实际业务数据库。
- `scripts/bidding_v2_deletion_scan.sh`：Legacy 招投标生产源码零匹配扫描。

Release 必须由 named required jobs `schema-contract`, `queue-faults`, `outline-scripted-e2e`, `web-export-e2e`, `release-descriptor` 执行。依赖缺失、测试为零、ignored/skipped suite 或 cleanup 失败均使 job 失败；目前仅已接入上述 `schema-contract`，其余 required jobs 尚未齐备；只能称 development checks，不得称 release accepted。
