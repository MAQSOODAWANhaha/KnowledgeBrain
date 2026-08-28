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

## 健康与验收

- `GET /live`：进程存活；
- `GET /ready`：PostgreSQL、普通维护门和 queue registry 可用；
- `GET /api/v1/ops/queues`：共享平台队列；
- `scripts/fresh_schema_acceptance.sh`：空库 V2 baseline；
- `scripts/bidding_v2_deletion_scan.sh`：Legacy 招投标删除扫描。
