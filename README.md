# KnowledgeBrain

Product-document knowledge service. Spec: `docs/system-design.md`.

HTTP (`/api/v1`) validates, persists, and enqueues only. Parse / chunk / vector / wiki / graph run in `worker`. Task progress: `GET /api/v1/documents/{id}/timeline`.

## Deploy

Full stack (api + worker + DocReader + Postgres + Redis + MinIO + Neo4j) lives in [`deploy/`](deploy/README.md):

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build
curl -sS http://127.0.0.1:18080/health
```

`docker compose up` from the repo root includes the same file.

## Local rust (infra only)

```bash
docker compose -f deploy/docker-compose.yml up -d postgres redis minio neo4j
API_PORT=8080 cargo run -p api
cargo run -p worker
```

Host ports: API 18080, DocReader 15051, Postgres 15432, Redis 16379, MinIO 19000 / console 19001.

## Test / CI

Gates: `.scratch/knowledgebrain/review.md`.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd services/docreader && uvx ruff check . && uv run --with pyright pyright . && PYTHONPATH=.. uv run --with pytest pytest tests/)
docker compose -f deploy/docker-compose.yml --env-file deploy/.env.example config -q
```
