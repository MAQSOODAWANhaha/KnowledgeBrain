# KnowledgeBrain 部署

进程：`api`（HTTP `:8080`）、`worker`（oxana Runtime）、独立 `retention`（HTTP probe `:8082`）、`docreader`（gRPC `:50051`）。
数据面：Postgres+pgvector、Redis、MinIO；Neo4j 可选双写。

本目录是部署真源：compose、环境变量、镜像。仓库根的 `docker-compose.yml` 只做转发，方便 `docker compose` 仍从根目录调用。

## 启动边界

普通 `docker compose up -d`（从仓库根或本目录执行）**只启动基础设施**：Postgres、Redis、MinIO、Neo4j。它不会启动 API、worker、迁移或 verifier，也不是生产首次上线命令。

全新生产安装最终只能从仓库根运行 checked-in orchestrator：

```bash
cp deploy/.env.example deploy/.env
KNOWLEDGEBRAIN_FIRST_LAUNCH_FRESH=required deploy/compose-first-launch.sh
```

当前固定 manifest 只有 `knowledge_base_baseline`、`shared_platform_baseline`、`bidding_v1_baseline` 三个 fresh slices；bootstrap role/extension 脚本也由 manifest 校验 checksum。`bidding_v1_baseline` 是**现在还能 first-launch 的切片**，不是产品终态。招投标产品目标是 Target V2（[`../docs/bidding/authoring.md`](../docs/bidding/authoring.md)），Phase 7 用 `bidding_v2_baseline` 替换 v1 切片。生产仍明确不可部署，因为 runtime-completion artifact 保持 incomplete 且没有环境变量绕过。只有 final API/Web、真实对象卷、PDF、恢复、readiness、image 和 topology 证据闭合后，orchestrator 才会销毁专用 volumes、执行一次性 migrate/verifier handoff，并启动 runtime profile。禁止对已有安装运行。

```bash
deploy/compose-runtime-restart.sh
```

该命令只命名 runtime profile 的 `api`、`worker`、`retention`、`docreader`，不会运行 migrate/verifier。Worker 只在 Redis 可用并注册消费者后报告 ready；物理对象删除只由 retention login/consumer 在数据库 claim 后执行。对象写入容器卷 `/data/objects`，并按 `.env` 写入 MinIO。

生产部署必须把 `JWT_SECRET` 改为随机强密钥，并配置 `KNOWLEDGEBRAIN_LDAP_URL` 与 `KNOWLEDGEBRAIN_LDAP_BIND_DN`。LDAP URL 为空时是仅供本地/测试使用的开放登录行为，不能用于对外服务。

## 只起数据面（本机 cargo 跑进程）

```bash
cd deploy
docker compose up -d postgres redis minio neo4j
```

然后在仓库根：

```bash
set -a && source deploy/.env && set +a
# 本机连映射端口，不要用 compose 服务名
export DATABASE_URL=postgres://knowledgebrain:knowledgebrain@127.0.0.1:15432/knowledgebrain
export REDIS_URL=redis://127.0.0.1:16379
export DOCREADER_ADDR=127.0.0.1:15051   # 若本机也起了 docreader 容器
export KNOWLEDGEBRAIN_S3_ENDPOINT=http://127.0.0.1:19000
API_PORT=8080 cargo run -p api
cargo run -p worker
cargo run -p retention
```

DocReader 本机：`cd services/docreader && PYTHONPATH=.. uv run python main.py`。

## 端口（默认）

| 服务 | 容器 | 主机 |
| --- | --- | --- |
| api | 8080 | 18080 |
| retention probe | 8082 | internal |
| docreader | 50051 | 15051 |
| postgres | 5432 | 15432 |
| redis | 6379 | 16379 |
| minio | 9000 / 9001 | 19000 / 19001 |
| neo4j | 7474 / 7687 | 17474 / 17687 |

改 `deploy/.env` 里的 `*_HOST_PORT`。

## 镜像

| 文件 | 产物 |
| --- | --- |
| `Dockerfile.rust` | `api`、`worker`、`retention`（对应 `BIN`）；Node 构建阶段同时产出并内置 `/web` SPA |
| `Dockerfile.docreader` | Python gRPC DocReader |

```bash
docker build -f deploy/Dockerfile.rust --build-arg BIN=api -t knowledgebrain-api .
docker build -f deploy/Dockerfile.rust --build-arg BIN=worker -t knowledgebrain-worker .
docker build -f deploy/Dockerfile.rust --build-arg BIN=retention -t knowledgebrain-retention .
docker build -f deploy/Dockerfile.docreader -t knowledgebrain-docreader .
```

## 环境变量

见 `.env.example`。容器内必须用服务名（`postgres`、`redis`、`minio`、`docreader`），不要写 `127.0.0.1`。

可选模型端点未配时走 stub：embedding / chat / VLM / MinerU / Paddle。  
`KNOWLEDGEBRAIN_NEO4J_HTTP_URL` 为空则图谱只写 Postgres。

并发：`KNOWLEDGEBRAIN_{CORE,POSTPROCESS,ENRICHMENT,MAINTENANCE,SHARED,WIKI}_CONCURRENCY`。

## 进度与健康

- `GET /health` — api 存活
- `GET /api/v1/documents/{id}/timeline` — 五段任务进度（JWT / API key）
- `GET /api/v1/ops/queues` — 队列深度
- `GET /api/v1/ops/oxana` — 队列深度 + 在途任务摘要（admin）
- `GET /api/v1/ops/oxana/web` — oxana-web 看板（admin）

## 停止

```bash
docker compose -f deploy/docker-compose.yml down
# 连数据卷一起删：
docker compose -f deploy/docker-compose.yml down -v
```
