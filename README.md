# KnowledgeBrain

Product-document knowledge service. Documentation: [`docs/README.md`](docs/README.md). Knowledge-base domain: [`docs/knowledge-base/domain.md`](docs/knowledge-base/domain.md). Bidding target: [`docs/bidding/domain.md`](docs/bidding/domain.md).

HTTP (`/api/v1`) validates, persists, and enqueues only. Parse / chunk / vector / wiki / graph run in `worker`. Task progress: `GET /api/v1/documents/{id}/timeline`.

## Deploy

Deployment definitions live in [`deploy/`](deploy/README.md). Plain Compose is intentionally **infrastructure-only**: `docker compose up -d` never starts API, worker, migrations, or first-launch verification.

A new production installation must eventually use only the checked-in destructive orchestrator:

```bash
cp deploy/.env.example deploy/.env
KNOWLEDGEBRAIN_FIRST_LAUNCH_FRESH=required deploy/compose-first-launch.sh
```

The fixed fresh baseline is exactly `knowledge_base_baseline`, `shared_platform_baseline`, and `bidding_v1_baseline`; there is no incremental bidding chain. Production is still intentionally blocked: the orchestrator fails before Docker while reviewed runtime completion is `false`. API, worker, and the independent retention consumer only verify the exact manifest identity and never migrate. After verified runtime acceptance, use `deploy/compose-runtime-restart.sh` for restarts.

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
scripts/bidding_v1_deletion_scan.sh
```
