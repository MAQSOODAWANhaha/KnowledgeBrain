# KnowledgeBrain 部署

进程：`api`（HTTP `:8080`）、`worker`（Oxana Runtime）、独立 `retention`
（probe `:8082`）、`docreader`（gRPC `:50051`）、本地 `onlyoffice`（HTTP host `:18081`）。数据面：PostgreSQL+pgvector、
Redis、MinIO；Neo4j 可选。

## 招投标编辑接入状态

[ONLYOFFICE/DOCX 技术方向](../docs/bidding/onlyoffice.md)已确认，普通实施与隔离开发验证已授权，任务见[实施台账](../plans/implementation-tasks.md)；ONLYOFFICE/DOCX 主链已有部分实现和隔离验证，真实整稿及生产部署仍待验收。编辑与出件顺序见 [接入计划](../plans/bidding/onlyoffice-integration.md)，Agent 运行、模型协议和配置改造见 [Rig 方案](../plans/bidding/agent-runtime-rig.md)。

本地 `--profile runtime` **包含** Community Document Server（`onlyoffice`，映射 `ONLYOFFICE_HOST_PORT`，默认 18081）。`deploy/.env.example` 必须填写 `KB_ONLYOFFICE_*`（浏览器 origin、容器内 command origin、API origin、两套独立 secret）；缺项时打开编辑器 fail-closed。Community 镜像只用于本地闭环，不代表生产嵌入许可、字体或并发已验收。同稿 PDF 仍属 O2。基础编辑/保存不依赖外部 Automation API。

### Agent 与模型配置改造（待实施）

投标 Agent 按 [统一 Agent 方案](../plans/bidding/agent-runtime-rig.md)仅使用 Chat Completions；不新增 Responses 或协议切换配置，模型与凭据统一来自 `deploy/.env`。Rig/Chat 已接入，新增语义修复、编制和终检按统一方案分阶段验证，真实全链尚未验收；能力与证据见[主方案 §19](../plans/bidding/agent-runtime-rig.md#19-当前能力真实验收与下一步)。Embedding 继续走知识库现有独立接口，索引身份一致性是[知识库独立事项](../plans/knowledge-base/README.md#模型与索引一致性独立事项)，不作为提取修复前置。

## Fresh 启动

当前尚未发布，按 [未发布 baseline 策略](../plans/platform/runtime-foundation.md#2-fresh-baseline)直接修正三份 migration；不建设历史升级链。当前运行方式只支持 clean-slate fresh deploy，不承诺发布后的保数据升级。修改 baseline 不授权对运行库执行迁移、清库或部署。

`migrate` job 在一个事务中按
`shared_platform_baseline` → `knowledge_base_baseline` → `bidding_v2_baseline` 建立 catalog，随后写唯一 `platform_schema_snapshot` release receipt。混有运行期追加合同的 seed 表由 `platform-frozen-seed-tables-v2.json` 显式列出 baseline 主键，初始记录全部字段继续校验，建库时检查初始清单完整；正常业务新增合同不改变 schema 指纹。旧 first-launch/intended-state/verifier、兼容迁移和双运行模式已删除；API、Worker、Retention readiness 只读验证 compiled baseline digests、server/extensions 与 canonical catalog manifest，绝不执行 DDL 或在线修复。

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/docker-compose.yml --env-file deploy/.env \
  --profile runtime up -d --build
```

生产发布使用 `deploy/release-descriptor-v1.json`：

```json
{
  "schema_version": 1,
  "release_revision": "...",
  "git_sha": "40-lowercase-hex",
  "platform_schema_revision": "...",
  "images": {
    "migrator": "registry/name@sha256:<64hex>",
    "api": "registry/name@sha256:<64hex>",
    "worker": "registry/name@sha256:<64hex>",
    "retention": "registry/name@sha256:<64hex>",
    "docreader": "registry/name@sha256:<64hex>"
  }
}
```

字段 exact、`additionalProperties=false`，images key set 固定，JSON 键顺序由 JCS 规范化。`release_revision` 与 `platform_schema_revision` 都使用 exact ASCII grammar `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`（1～128 bytes，不 trim/normalize）。`release_descriptor_sha256=SHA256("KB:ReleaseDescriptor:v1\0" || RFC8785_JCS(document))` 是整组组件 identity，不把多个 image 误称为“一个 image digest”。入口复用平台 typed validator/JCS，并核对 Compose 同目录的 checked schema 与编译内嵌合同一致；缺文件、重复字段或合同漂移均拒绝。

宿主 Rust binary `knowledgebrain-release` 已实现，完成脚本 Engine 故障测试、真实 Compose 配置合并及生产 Dockerfile 镜像构建。修复无显式环境的依赖校验问题后，真实隔离启动、receipt/readiness、只读重放及对象上传/引用保护/最终回收已通过，专属资源清理后零残留，见[真实隔离验收](../docs/bidding/release-live-results.md)；**干净发布候选及 R1 完整验收尚未完成，不能据此放行生产**。从审核后的候选构建并将 binary 安装到 PATH：

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

实际执行先解析 Compose、把所有容器名限定到显式 project，检查已有组件/依赖的身份和健康状态，再 pull 并 inspect 全部 digest 引用。mode-0700 临时目录内的 mode-0600 override 写入五组件 exact image，四个 Rust 组件额外写入 `BIN`、exact release env 和原 descriptor 的只读挂载。先等待 Compose 数据面及 DocReader，再运行 migrator，确认其 image/config、环境、挂载和 exit 0 后才启动 API/Worker/Retention。最后逐组件 inspect，并实际执行配置中已核对一致的健康探针。镜像 manifest RepoDigest 与本地 image config ID 分别校验，不能混用。

Migrator 把 descriptor SHA 写入 schema receipt；runtime 的 `/ready` 继续通过现有平台逻辑核对 mounted hash、env、receipt、component kind/digest。DocReader 由外部 post-start image/健康检查验证。已有部署不一致时拒绝自动替换；全部服务符合时只检查，不重新迁移或启动。失败只停止本次标签且 project 一致的容器，保留既有容器与数据卷；输入漂移、探针失败或临时文件清理失败均不报成功。失败留下的已停止容器须核实后单独处理，工具不执行删除或升级。

checked-in `release-descriptor-v1.development.json` 及 Compose 默认 mount 仅用于本地开发；其 synthetic digest 不证明本地 image 的实际 RepoDigest。旧 `docker-compose.pro.yml` 和 `.env.example` 中的 tag 发布配置已删除。验证范围、可复现命令及剩余验收见[发布入口验证](../docs/bidding/release-entry.md)。

`api`、`worker`、`retention` 都等待 `migrate` 成功。应用进程启动时只连接依赖，不执行
DDL。重启使用：

```bash
./deploy/deploy.sh up
```

当前对象存储源码要求显式配置 `OBJECT_DIR` 和 `KB_DEPLOYMENT_NAMESPACE_ID`，本地物理布局为 `<OBJECT_DIR>/kb-<32-lowercase-hex>/objects/<文件名>`，MinIO 使用同一部署前缀。旧目录/key 不会被自动认领或读取；配置路径及其中间层不能是符号链接。已有安装不能仅替换二进制就视为已完成对象迁移；源码验证和现有发布镜像的区别见[namespace 隔离记录](../docs/platform/namespace-isolation.md)。

Redis 队列入口同样要求显式 `REDIS_URL`，不回退固定本机地址；Oxana 与多模态计数器共用 `kb:<32-lowercase-hex>:` 物理键前缀。旧 raw UUID 队列与无前缀计数器不会被新代码自动接管，producer/consumer 不得混用新旧布局。此项也尚未部署，不能用旧镜像的运行结果证明新源码已验收。

Fresh reset 会永久删除当前 deployment namespace。正式发布前必须提供 [`../plans/platform/runtime-foundation.md`](../plans/platform/runtime-foundation.md) 定义的命令：

```bash
deploy/reset-namespace.sh \
  --namespace kb-<32-lowercase-hex> \
  --expected-schema-revision <revision> \
  --confirm <64-uppercase-hex-token>
```

命令验证 runtimes drained、receipt identity 与 backend mapping，按 OBJECT_DIR→MinIO→Neo4j→Redis→PostgreSQL 写 checkpoint 并幂等执行。脚本尚未实现或任一检查失败时不得执行 production reset，也不得以 `down -v` 代替。旧 unnamespaced 安装属于 unsupported stale state，只能在明确停机和确认无保留要求后进入独立人工 full-clean 流程。

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

`delete` 不会删除 Git 工作区、Docker 全局镜像或其他 Compose 项目的数据卷，但它会删除当前本地 Compose 项目的全部数据面；只允许用于明确的本地 clean-slate 验收，不是生产 namespace reset 命令。

> Oxana 的 `Job finished success=true` 表示消息已被 Worker 确认处理，不能证明业务生成成功。招标分析应核对请求终态、独立复核结果和已发布的分析版本。执行诊断位于 `bid_tender_agent_run_artifacts`；诊断沿用 SQL 的 UTF-8 安全字节前缀，不承诺保留完整原始消息。旧大纲生成任务已从注册表和 API/SQL 创建链删除。


## 本机 cargo + 容器数据面

```bash
docker compose -f deploy/docker-compose.yml up -d postgres redis minio neo4j
export DATABASE_URL=postgres://knowledgebrain:knowledgebrain@127.0.0.1:15432/knowledgebrain
export REDIS_URL=redis://127.0.0.1:16379
cargo run -p platform --bin migrator
API_PORT=8080 cargo run -p api
cargo run -p worker
cargo run -p retention
```

## 端口

本地 `deploy.sh` 叠加 `docker-compose.local.yml`：主机入口是 **Caddy 自签 HTTPS**（监听所有网卡，`127.0.0.1` 与局域网 IP 均可）。证书由 `deploy/certs/generate.sh` 生成，SAN 含 `localhost`、`127.0.0.1` 和本机 IPv4。浏览器需信任该证书。IP 变更后删除 `deploy/certs/localhost.crt` 与 `.key` 再 `./deploy/deploy.sh up`。

| 服务 | 容器 | 主机（`.env` / `.env.example` 变量） |
| --- | --- | --- |
| 浏览器 SPA/API（本地） | Caddy 28080 TLS → `api:8080` | `API_HOST_PORT`（本机常用 28080）→ `https://<主机>:${API_HOST_PORT}/` |
| api | 8080 HTTP | 本地 overlay **不映射**到主机；Docker 内健康检查与 ONLYOFFICE 回调仍用 `http://api:8080/` |
| 浏览器 Document Server（本地） | Caddy 18081 TLS → `onlyoffice:80` | `ONLYOFFICE_HOST_PORT`（示例 18081）→ `https://<同一主机>:${ONLYOFFICE_HOST_PORT}/` |
| onlyoffice | 80 HTTP | 本地 overlay **不映射**；API 用 `KB_ONLYOFFICE_COMMAND_ORIGIN=http://onlyoffice/` |
| retention probe | 8082 | 不映射 |
| docreader | 50051 | `DOCREADER_HOST_PORT`（示例 15051） |
| postgres | 5432 | `POSTGRES_HOST_PORT`（示例 15432） |
| redis | 6379 | `REDIS_HOST_PORT`（示例 16379） |
| minio | 9000 / 9001 | `MINIO_HOST_PORT` / `MINIO_CONSOLE_PORT` |
| neo4j | 7474 / 7687 | `NEO4J_HTTP_PORT` / `NEO4J_BOLT_PORT` |

生产部署必须替换 `JWT_SECRET`、`KB_ONLYOFFICE_JWT_SECRET`、`KB_ONLYOFFICE_CAPABILITY_SECRET` 和所有数据库密码，并配置实际 LDAP、对象存储及模型端点。容器内连接使用 compose 服务名：Document Server 用 `KB_ONLYOFFICE_COMMAND_ORIGIN=http://onlyoffice/`、`KB_ONLYOFFICE_API_ORIGIN=http://api:8080/`，不要让 API 容器去访问 `127.0.0.1:18081`。

## 招投标 V2 Worker

`bid-authoring-v2` 物理队列注册以下五类粗粒度任务，且都必须有 active handler：

- `bid:tender_document_process:v2`
- `bid:requirement_set_compile:v2`
- `bid:docx_compose:v2`（冻结分析到完整模板，需显式 KB_DOCX_COMPOSITION_LIMITS；整链验收状态见编制实施记录）
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
- partial/stale catalog 或 receipt/manifest mismatch 不做在线修复。Runtime readiness 返回稳定 schema mismatch；先停止并核对数据保留要求，只有用户确认可重建的隔离环境才按 deployment namespace reset contract 处理。未发布不代表开发资料可丢，不能自动 reset 或清共享卷。当前 receipt 还绑定完整 release descriptor 和精确 PG 版本，镜像/PG 补丁变化也可能拒启；不得伪改 receipt 绕过，首次发布前固定并验收这些身份。

更详细的招投标运行与验收见 [`../docs/bidding/backend-runbook.md`](../docs/bidding/backend-runbook.md)。

## 健康与验收

以下仅记录已存在、获准保留的实现增量与历史本地验证，不代表本轮重跑或整体运行验收。方案已确认；普通实施与隔离开发验证的任务及授权边界见[实施台账](../plans/implementation-tasks.md)，不含采购、生产部署、现有数据操作或提交。

五 writer 诊断与隔离 PG16 的 Content/Outline、fresh/catalog 本地回归已通过；CI 新增独立 `schema-contract`（使用 catalog 专用 DSN/require flag），但 hosted CI 未触发，整体回归仍被既有 `request_delivery_postgres` 清理用例阻断：当前缺 Oxana Redis，后续验收统一按 [平台 §6](../plans/platform/runtime-foundation.md#6-retention-consumer) 区分交接确认、真实消费后的 staging 释放及对象最终回收，不能仅加服务或排队成功即算解决，也不能削弱最终回收断言。仅复用已有 Oxana typed jobs 与原生恢复机制，不另建清理调度/补偿框架。任务已按 [平台 §7](../plans/platform/runtime-foundation.md#7-实施与验收) 的正确性与首发边界列入[实施台账](../plans/implementation-tasks.md)。release/reset 工具、synthetic digest 与 `latest` 的缺口不能以文档命令充数；它们不阻塞隔离开发修复或获准的 ONLYOFFICE O0，但不减免下列首发 required jobs。pgvector 基准不作发布正确性修复的前置。

- `GET /live`：进程存活；
- `GET /ready`：PostgreSQL、schema receipt/catalog manifest、server/extensions、deployment namespace、维护门和 queue registry 可用；
- `GET /api/v1/ops/queues`：共享平台队列（不是招投标 V1 API）；
- `scripts/fresh_schema_acceptance.sh`：严格限定 `127.0.0.1:25433/knowledgebrain_test_*` 的 PostgreSQL 16 空库；调用前必须先对映射该端口的临时 PostgreSQL 容器执行 `deploy/postgres-init/010-runtime-identities.sh`。Bootstrap 必须显式提供 `POSTGRES_USER`、`POSTGRES_DB`、`KNOWLEDGEBRAIN_MIGRATOR_PASSWORD`、`KNOWLEDGEBRAIN_API_DB_PASSWORD`、`KNOWLEDGEBRAIN_WORKER_DB_PASSWORD`、`KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD`；在授权隔离环境按 `deploy/postgres-init/010-runtime-identities.sh` 和脚本前置条件提供变量；不要对运行库执行。CI 通过明确的 `PGHOST/PGPORT/PGUSER/PGPASSWORD` 在 runner 调用该初始化脚本。随后验收脚本显式消费 `KNOWLEDGEBRAIN_TEST_DATABASE_URL` 及 Migrator/API/Worker/Retention 四个测试密码。Admin URL 必须使用与 bootstrap `POSTGRES_USER` 相同的 privileged identity（membership grantor），可读 `pg_authid` 并可在隔离测试中注入/恢复 receipt drift，不得使用 runtime role。脚本验证 exact roles/membership、`public` owner、Migrator 无 direct CREATE、runtime 无 TEMP/DDL，注入 test ReleaseDescriptor identity，执行三 baseline receipt insert、matching replay receipt/catalog object counts不变、API/Worker/Retention 各自实际登录的 shared runtime verifier、TEMP/DDL/SET ROLE owner 权限拒绝，以及 receipt/catalog 漂移的 `SCHEMA_REVISION_MISMATCH` fail-closed 行为（receipt mismatch 还核对 `reset required`）；这些断言直接集成在脚本中，不引用任何 obsolete `scripts/*_live.sql`；
- `scripts/bidding_v2_phase2_api_e2e.py`：存量块 Workspace API，不验证 DOCX 编辑保存；
- `scripts/bidding_v2_evidence_api_worker_e2e.py`：Evidence/PickSet/Candidate API→Redis→Worker；
- `scripts/bidding_v2_export_api_worker_e2e.py`：存量自研 DOCX/PDF 渲染、下载及报告，不作为 ONLYOFFICE 出件验收；
- `scripts/bidding_v2_deletion_scan.sh`：Legacy 招投标生产源码零匹配扫描。

Release 必须由 named required jobs `schema-contract`, `queue-faults`, `namespace-reset`, `outline-scripted-e2e`, `web-export-e2e`, `release-descriptor` 执行。依赖缺失、测试为零、ignored/skipped suite 或 cleanup 失败均使 job 失败；目前仅已接入上述 `schema-contract`，其余 required jobs 尚未齐备，整体 CI 仍有前述独立阻断；只能称 development checks，不得称 release accepted。
