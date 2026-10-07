# KnowledgeBrain

Product-document knowledge service. Product: [`PRODUCT.md`](PRODUCT.md). Design: [`DESIGN.md`](DESIGN.md). Documentation: [`docs/README.md`](docs/README.md). Knowledge-base domain: [`docs/knowledge-base/domain.md`](docs/knowledge-base/domain.md). Bidding: [business PRD](docs/bidding/prd.md), [outline runtime](docs/bidding/outline.md), and [remaining gaps](plans/bidding/outline-gaps.md). After tender parse, one outline run writes the chapter tree and prescribed template. Later knowledge-base fill reads only the frozen outline. Editor integration stays in [ONLYOFFICE](plans/bidding/onlyoffice-integration.md). Historical runs are in [docs/bidding/archive](docs/bidding/archive/README.md).

HTTP (`/api/v1`) validates, persists, and enqueues only. Parse / chunk / vector / wiki / graph run in `worker`. Task progress: `GET /api/v1/documents/{id}/timeline`.

## Deploy

Deployment definitions live in [`deploy/`](deploy/README.md). A fresh installation uses the
`migrate` bootstrap job followed by the runtime profile. The old launch verifier/intended-state system、compatibility migration and bidding V1 schema are absent; the migrator writes one minimal schema release receipt and every runtime performs read-only compiled-digest/catalog-manifest verification before readiness.

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/docker-compose.yml --env-file deploy/.env --profile runtime up -d --build
```

The bootstrap applies `shared_platform_baseline`, `knowledge_base_baseline`, and
`bidding_v2_baseline` to an empty database in one transaction. API, worker, and retention only connect and verify at startup; they never execute DDL.
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
scripts/fresh_schema_acceptance.sh
(cd services/docreader && uvx ruff check . && uv run --with pyright pyright . && PYTHONPATH=.. uv run --with pytest pytest tests/)
docker compose -f deploy/docker-compose.yml --env-file deploy/.env.example config -q
npm ci --prefix web
npm --prefix web run lint
npm --prefix web run build
npm --prefix web run test:e2e
scripts/bidding_v2_deletion_scan.sh
```
