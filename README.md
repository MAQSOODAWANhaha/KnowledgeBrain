# KnowledgeBrain

Product-document knowledge service and tender authoring. Product: [`PRODUCT.md`](PRODUCT.md). Design: [`DESIGN.md`](DESIGN.md). Documentation: [`docs/README.md`](docs/README.md). Knowledge-base domain: [`docs/knowledge-base/domain.md`](docs/knowledge-base/domain.md). The current tender workflow is implemented in Python: [business workflow](docs/design/python-tender-phase1/business-workflow.md), [field dictionary](docs/design/python-tender-phase1/field-dictionary.md), and [handoff](docs/design/python-tender-phase1/handoff.md). Rust provides the HTTP adapter, knowledge service and shared infrastructure; the former Rust tender workflow has been removed. Historical private evidence is not published.

HTTP (`/api/v1` knowledge, `/api/v3/tender-projects` tender) validates, persists, and enqueues. Knowledge parse / chunk / vector / wiki / graph run in `worker`; tender analysis and authoring run in Python workers. Resources are visible and writable only by their creator. API keys act as their owning user without separate scope permissions.

## Deploy

Deployment definitions live in [`deploy/`](deploy/README.md). A fresh installation uses the
`migrate` bootstrap job followed by the runtime profile. The bootstrap creates the application tables; deployment namespaces, schema version receipts and runtime catalog hash verification are not required.

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/docker-compose.yml --env-file deploy/.env --profile runtime up -d --build
```

The migrator applies timestamped SQL files from `migrations/` using SQLx. Initial versions create the shared platform, knowledge base, and tender tables; later versions upgrade them incrementally. Each file and its version/checksum record commit together. These contain tables, indexes and data constraints, with no custom functions, triggers or business database roles. API, worker, and retention check connectivity; they never execute business DDL. See [database design](docs/design/database-initialization.md).
Use `./deploy/deploy.sh up` for an ordinary restart.

## Local rust (infra only)

```bash
docker compose -f deploy/docker-compose.yml up -d postgres redis minio neo4j
API_PORT=8080 cargo run -p api
cargo run -p worker
cargo run -p retention
```

Host ports: API 18080, DocReader 15051, Postgres 15432, Redis 16379, MinIO 19000 / console 19001.

## Development checks / target release gates

The commands below are development checks. They become release gates only when the named required CI jobs in [`deploy/README.md`](deploy/README.md) run them fail-closed with real PostgreSQL/pgvector、Redis、MinIO and zero skipped suites. Current workflow status must not be described as release accepted merely because these commands are documented.

Review evidence: `.scratch/knowledgebrain/review.md`.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd services/docreader && uvx ruff check . && uv run --with pyright pyright . && PYTHONPATH=.. uv run --with pytest pytest tests/)
docker compose -f deploy/docker-compose.yml --env-file deploy/.env.example config -q
npm ci --prefix web
npm --prefix web run lint
npm --prefix web run build
npm --prefix web run test:e2e
```
