# KnowledgeBrain 部署

进程：`api`（HTTP `:8080`）、`worker`（Oxana Runtime）、独立 `retention`
（probe `:8082`）、`docreader`（gRPC `:50051`）。数据面：PostgreSQL+pgvector、
Redis、MinIO；Neo4j 可选。

## Fresh 启动

本系统只支持 clean-slate fresh deploy。普通 `migrate` job 直接建立
`knowledge_base_baseline`、`shared_platform_baseline`、`bidding_v2_baseline`；没有
first-launch verifier、manifest checksum、catalog allowlist、兼容迁移或双运行模式。

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/docker-compose.yml --env-file deploy/.env \
  --profile runtime up -d --build
```

`api`、`worker`、`retention` 都等待 `migrate` 成功。应用进程启动时只连接依赖，不执行
DDL。重启使用：

```bash
deploy/compose-runtime-restart.sh
```

Fresh reset 会永久删除数据：

```bash
docker compose -f deploy/docker-compose.yml down -v
```

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

| 服务 | 容器 | 主机 |
| --- | --- | --- |
| api | 8080 | 18080 |
| retention probe | 8082 | internal |
| docreader | 50051 | 15051 |
| postgres | 5432 | 15432 |
| redis | 6379 | 16379 |
| minio | 9000 / 9001 | 19000 / 19001 |
| neo4j | 7474 / 7687 | 17474 / 17687 |

生产部署必须替换 `JWT_SECRET` 和所有数据库密码，并配置实际 LDAP、对象存储及模型
端点。容器内连接使用 compose 服务名，不使用 `127.0.0.1`。

## 招投标 V2 Worker

`bid-authoring-v2` 物理队列只注册以下五类粗粒度任务，且都必须有 active handler：

- `bid:tender_document_process:v2`
- `bid:requirement_set_compile:v2`
- `bid:outline_generate:v2`
- `bid:content_generate:v2`
- `bid:submission_export:v2`

不得恢复 EvidenceMatch continuation、旧 Part/Gate Job 或 default queue fallback。队列声明以
`deploy/queue-registry.toml` 为唯一运行注册表，启动时会校验 handler、payload 和唯一身份公式。

## 故障恢复

- TenderDocument 技术解析失败：修复 DocReader/OCR/VLM/ObjectRegistry 后，由用户调用 V2 retry；不要改表或复用旧请求身份。
- Requirement/Outline/Content/Export delivery 可按原 frozen request 重投；stage receipt 保证同一输入幂等。过期 Requirement/Candidate 终态为 `obsolete`，不得推进 Workspace。
- Workspace CAS 返回 409 时，客户端重新 GET 当前 ETag 后显式重放用户意图；服务端不得覆盖并发编辑。
- `NO_EVIDENCE`、未覆盖要求等业务 Assessment 只产生 warning；Schema、CAS、资产 digest、事务和 renderer 错误必须 fail-closed。
- 失败上传由 retention 回收 `object_upload_staging`；已进入 Artifact/Manifest 的对象通过 ObjectRegistry owner reference 保留，禁止直接删除对象或业务行。
- partial/stale catalog 不做在线修复。确认无保留要求后执行 fresh `down -v`，再由 migrator 重建三份 baseline。

更详细的招投标运行与验收见 [`../docs/bidding/backend-runbook.md`](../docs/bidding/backend-runbook.md)。

## 健康与验收

- `GET /live`：进程存活；
- `GET /ready`：PostgreSQL、普通维护门和 queue registry 可用；
- `GET /api/v1/ops/queues`：共享平台队列（不是招投标 V1 API）；
- `scripts/fresh_schema_acceptance.sh`：空库三 baseline、重复 migrator 与 runtime ACL；
- `scripts/bidding_v2_phase{0,1,3,6}_live.sql`：分阶段 SQL 活库证据；
- `scripts/bidding_v2_phase2_api_e2e.py`：纯人工 Workspace API；
- `scripts/bidding_v2_evidence_api_worker_e2e.py`：Evidence/PickSet/Candidate API→Redis→Worker；
- `scripts/bidding_v2_export_api_worker_e2e.py`：DOCX/PDF 两阶段导出、下载和报告；
- `scripts/bidding_v2_deletion_scan.sh`：Legacy 招投标生产源码零匹配扫描。
