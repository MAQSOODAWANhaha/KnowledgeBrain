# KnowledgeBrain

Product-document knowledge service. Product: [`PRODUCT.md`](PRODUCT.md). Design: [`DESIGN.md`](DESIGN.md). Documentation: [`docs/README.md`](docs/README.md). Knowledge-base domain: [`docs/knowledge-base/domain.md`](docs/knowledge-base/domain.md). Bidding target: [`docs/bidding/authoring.md`](docs/bidding/authoring.md) (current-code contrast, not wholesale deletion: [`docs/bidding/current-code.md`](docs/bidding/current-code.md)).

HTTP (`/api/v1`) validates, persists, and enqueues only. Parse / chunk / vector / wiki / graph run in `worker`. Task progress: `GET /api/v1/documents/{id}/timeline`.

## Deploy

Deployment definitions live in [`deploy/`](deploy/README.md). A fresh installation uses the
ordinary `migrate` bootstrap job followed by the runtime profile; there is no launch verifier,
manifest checksum gate, compatibility migration, or bidding V1 schema.

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/docker-compose.yml --env-file deploy/.env --profile runtime up -d --build
```

The bootstrap applies `knowledge_base_baseline`, `shared_platform_baseline`, and
`bidding_v2_baseline` to an empty database. API, worker, and retention only connect at startup.
Use `deploy/compose-runtime-restart.sh` for an ordinary restart.

## Local rust (infra only)

```bash
docker compose -f deploy/docker-compose.yml up -d postgres redis minio neo4j
API_PORT=8080 cargo run -p api
cargo run -p worker
cargo run -p retention
```

Host ports: API 18080, DocReader 15051, Postgres 15432, Redis 16379, MinIO 19000 / console 19001.

## Test / CI

Gates: `.scratch/knowledgebrain/review.md`.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/fresh_schema_acceptance.sh
(cd services/docreader && uvx ruff check . && uv run --with pyright pyright . && PYTHONPATH=.. uv run --with pytest pytest tests/)
docker compose -f deploy/docker-compose.yml --env-file deploy/.env.example config -q
npm ci --prefix web
npm --prefix web run lint
npm --prefix web run build
npm --prefix web run test:e2e
scripts/bidding_v2_deletion_scan.sh
```
