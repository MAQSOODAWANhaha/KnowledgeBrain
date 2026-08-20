# KnowledgeBrain 部署

进程：`api`（HTTP `:8080`）、`worker`（6 个 oxana Runtime）、`docreader`（gRPC `:50051`）。  
数据面：Postgres+pgvector、Redis、MinIO；Neo4j 可选双写。

本目录是部署真源：compose、环境变量、镜像。仓库根的 `docker-compose.yml` 只做转发，方便 `docker compose` 仍从根目录调用。

## 一键拉起

在仓库根目录：

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build
docker compose -f deploy/docker-compose.yml ps
curl -sS http://127.0.0.1:18080/health
```

或进入本目录：

```bash
cd deploy
cp .env.example .env
docker compose --env-file .env up -d --build
```

首次启动 `api` / `worker` 会在报告 ready / 开始消费队列前连接 Postgres 并执行 `0001`–`0008`；连接、迁移或公司工作区初始化失败时进程直接失败，不会以缺失 schema 的状态继续服务或确认作业。Worker 只在 Redis 可用并注册消费者后报告 ready，连接中断后会退避重连；Compose 也配置了自动重启。对象写入容器卷 `/data/objects`，并按 `.env` 双写 MinIO；配置了 MinIO 时远端写失败会令本次对象写失败，不再静默降级。

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
```

DocReader 本机：`cd services/docreader && PYTHONPATH=.. uv run python main.py`。

## 端口（默认）

| 服务 | 容器 | 主机 |
|---|---|---|
| api | 8080 | 18080 |
| docreader | 50051 | 15051 |
| postgres | 5432 | 15432 |
| redis | 6379 | 16379 |
| minio | 9000 / 9001 | 19000 / 19001 |
| neo4j | 7474 / 7687 | 17474 / 17687 |

改 `deploy/.env` 里的 `*_HOST_PORT`。

## 镜像

| 文件 | 产物 |
|---|---|
| `Dockerfile.rust` | `api` 与 `worker`（`BIN=api` / `BIN=worker`）；Node 构建阶段同时产出并内置 `/web` SPA |
| `Dockerfile.docreader` | Python gRPC DocReader |

```bash
docker build -f deploy/Dockerfile.rust --build-arg BIN=api -t knowledgebrain-api .
docker build -f deploy/Dockerfile.rust --build-arg BIN=worker -t knowledgebrain-worker .
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
