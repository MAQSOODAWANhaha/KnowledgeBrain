# BID Extractor Continuation — Final Fix Handoff

## Implemented

- Added explicit `bid_match_jobs.job_kind` (`technical|commercial`) and authoritative `(project_id,generation,job_kind,unit_id) NULLS NOT DISTINCT` identity. Commercial `NULL` and unsectioned technical nil UUID now coexist; stale expected-generation insertion and duplicate delivery remain fenced/idempotent.
- Reworked clause PATCH persistence so the project is locked first, the clause is read `FOR UPDATE`, only request-supplied fields are applied, and match-input changes are derived from locked old/new values. Assessment-only concurrent PATCH cannot restore stale text/family/must/status.
- Moved Section ownership/self/cycle validation into the project-locked merge transaction. Opposing concurrent merges cannot commit a cycle and match generation/dirty remains atomic.
- Made durable Section-retry terminal/reset state and project/Section lease release one token-conditioned transaction. The worker no longer releases the project before terminally finishing the durable job.
- Removed family-heading-only table candidacy. Neutral inventory rows are skipped under technical and commercial headings. Exact Markdown table rows remain one heuristic unit even when values contain Chinese or ASCII semicolons.
- Bumped logical Prompt version to `clause-extractor-v3`; model `text` is required to exactly equal quote and persisted text remains server-canonicalized.
- Replaced mid-JSON tool truncation with a deterministic structured size error, rejected `max_emit + 1` batches in full, and added escape-heavy/long-heading coverage.
- Terminal provider errors now carry `AgentStats`; strict and hybrid diagnostics include attempted rounds/retries while exposing only bounded provider-error categories.
- Exposed compact current match jobs and extraction run ID to the service smoke. The smoke now proves meaningful text/must/family edits before separate confirmation, distinct successful technical/commercial jobs, deterministic empty technical candidates and commercial miss, successful new manual re-extraction with diagnostics, and existing retry/export/end invariants.
- Fixed configured-S3 calls so the blocking client is created and dropped on a dedicated OS thread; configured S3 no longer panics when invoked from async API tests.
- Updated the approved plan, domain/system specifications, and final three-round synthesis. `docs/system-design.md` is byte-identical to `.scratch/knowledgebrain/spec.md`.

## Changed files

- `.scratch/knowledgebrain/spec.md`
- `crates/api/src/routes.rs`
- `crates/bid/config/cn-tender-v2.json`
- `crates/bid/prompts/clause-extractor-v2.md`
- `crates/bid/src/extraction/agent.rs`
- `crates/bid/src/extraction/coverage.rs`
- `crates/bid/src/extraction/mod.rs`
- `crates/bid/src/extraction/outline.rs`
- `crates/bid/src/lib.rs`
- `crates/storage/src/bid.rs`
- `crates/storage/src/persist.rs`
- `crates/storage/src/s3.rs`
- `crates/worker/src/consume.rs`
- `docs/bid-platform-domain.md`
- `docs/system-design.md`
- `migrations/0007_bid.sql`
- `plans/bid-extractor-hardening.md`
- `plans/reviews/bid-extractor-continuation-final.md`
- `plans/reviews/bid-extractor-continuation-final-fix-handoff.md`
- `scripts/bid_e2e_smoke.sh`

## Tests added or extended

- PostgreSQL match-kind identity alongside stale-snapshot and duplicate-delivery coverage.
- Concurrent partial text/must and assessment-only clause PATCH regression.
- Concurrent opposing Section merge cycle regression.
- Token-conditioned paired Section-retry terminal/release and duplicate-finish regression.
- Neutral tables under technical and commercial headings.
- Exact table rows containing both `；` and `;`.
- Structured oversized tool output and `max_emit + 1` rejection.
- Strict/hybrid terminal provider attempt diagnostics and message redaction.
- Expanded service-backed BID smoke assertions.

## Validation evidence

- `cargo check --workspace --all-targets` — exit 0.
- `cargo test -p bid --lib` — exit 0; 58 passed.
- `cargo test -p storage -- --nocapture` against a fresh PostgreSQL DB — exit 0; 31 passed, including all new races.
- `cargo test --workspace -- --nocapture` against a fresh PostgreSQL DB and disposable Redis — exit 0; all workspace suites passed. Output contained only optional `s3 not configured` and `neo4j not configured` skips; no PostgreSQL or Redis skip occurred.
- A first full-workspace run with S3 configured exposed a reqwest blocking-client drop panic in async API tests. After the dedicated-thread fix:
  - configured-S3 `cargo test -p api --test http_flow` — exit 0; 10 passed;
  - configured-S3 `cargo test -p storage s3::tests::live_put_get_roundtrip -- --nocapture` — exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` — exit 0.
- `cargo fmt --all -- --check` — exit 0.
- `npm -C web run build` — exit 0.
- `docker compose -f deploy/docker-compose.yml --env-file deploy/.env.example config -q` — exit 0.
- `scripts/bid_e2e_smoke.sh` with a fresh PostgreSQL DB and disposable Redis — exit 0; printed `bid service-backed smoke: PASS`.
- Fresh migration application of every `migrations/*.sql` — exit 0; verified `job_kind` and the `NULLS NOT DISTINCT` unique index.
- `bash -n scripts/bid_e2e_smoke.sh` — exit 0.
- `cmp -s docs/system-design.md .scratch/knowledgebrain/spec.md` — exit 0.
- `git diff --check` — exit 0.
- `git diff --cached --quiet` — exit 0; no staged files.

## Deferred by explicit scope decision

- LDAP/LDAPS and explicit production auth mode (user selected option A).
- Production readiness/worker-heartbeat architecture and Redis/PostgreSQL fault injection.
- Seeded product/company assets, pick/unpick, and shot flows.
- Strict external Agent, VLM, non-Markdown DocReader, and real embedding/provider validation.
- Browser automation and full production Compose recovery.
- Additional long/unheaded Agent-mode goldens and timing-controlled old-slow/new-fast provider publication.
- Ancillary booklet-stale intent atomicity for manual clause/merge/pick mutations; this pass did not add a new outbox architecture.

No commit was created and no files were staged. No fourth review round was run because the three-round cap was reached.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Implemented only the final capped-review correctness fixes, Prompt/tool contract, smoke closure, and directly discovered configured-S3 async safety issue; LDAP/readiness/assets/fault injection remained explicitly deferred."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "Fresh PostgreSQL race tests, full workspace tests with PostgreSQL+Redis, configured-S3 API/storage tests, Clippy, build, migration smoke, and expanded service smoke all have recorded exit-0 evidence."
    }
  ],
  "changedFiles": [
    ".scratch/knowledgebrain/spec.md",
    "crates/api/src/routes.rs",
    "crates/bid/config/cn-tender-v2.json",
    "crates/bid/prompts/clause-extractor-v2.md",
    "crates/bid/src/extraction/agent.rs",
    "crates/bid/src/extraction/coverage.rs",
    "crates/bid/src/extraction/mod.rs",
    "crates/bid/src/extraction/outline.rs",
    "crates/bid/src/lib.rs",
    "crates/storage/src/bid.rs",
    "crates/storage/src/persist.rs",
    "crates/storage/src/s3.rs",
    "crates/worker/src/consume.rs",
    "docs/bid-platform-domain.md",
    "docs/system-design.md",
    "migrations/0007_bid.sql",
    "plans/bid-extractor-hardening.md",
    "plans/reviews/bid-extractor-continuation-final.md",
    "scripts/bid_e2e_smoke.sh"
  ],
  "testsAddedOrUpdated": [
    "storage PostgreSQL match-kind, partial-PATCH, merge-cycle, and paired-retry tests",
    "bid neutral/semicolon table, tool-bound, emit-bound, and failed-agent-stat tests",
    "scripts/bid_e2e_smoke.sh full deterministic assertions"
  ],
  "commandsRun": [
    {"command":"cargo test -p bid --lib","result":"passed","summary":"58/58 BID tests passed"},
    {"command":"cargo test -p storage -- --nocapture (fresh PostgreSQL)","result":"passed","summary":"31/31 storage tests passed"},
    {"command":"cargo test --workspace -- --nocapture (fresh PostgreSQL + Redis)","result":"passed","summary":"all workspace suites passed; no PostgreSQL/Redis skips"},
    {"command":"configured-S3 cargo test -p api --test http_flow","result":"passed","summary":"10/10 API flow tests passed after async drop fix"},
    {"command":"configured-S3 cargo test -p storage s3::tests::live_put_get_roundtrip","result":"passed","summary":"MinIO write/read roundtrip passed"},
    {"command":"cargo clippy --workspace --all-targets -- -D warnings","result":"passed","summary":"no Clippy warnings"},
    {"command":"cargo fmt --all -- --check && cargo check --workspace --all-targets","result":"passed","summary":"format and compile clean"},
    {"command":"npm -C web run build","result":"passed","summary":"TypeScript/Vite production build passed"},
    {"command":"scripts/bid_e2e_smoke.sh (fresh PostgreSQL + Redis)","result":"passed","summary":"expanded service-backed BID smoke passed"},
    {"command":"fresh migrations/*.sql application","result":"passed","summary":"all migrations applied; match kind/index verified"},
    {"command":"Compose config, docs/spec cmp, git diff --check, staged check","result":"passed","summary":"deployment/static contracts clean; no staged files"}
  ],
  "validationOutput": [
    "BID tests: 58 passed",
    "storage PostgreSQL tests: 31 passed",
    "workspace: all tests passed with fresh PostgreSQL and Redis",
    "bid service-backed smoke: PASS",
    "configured MinIO S3 roundtrip: passed",
    "Clippy -D warnings, web build, migration smoke, Compose and diff checks: passed"
  ],
  "residualRisks": [
    "LDAP/LDAPS, readiness and dependency fault injection are explicitly deferred",
    "pick/shot and external Agent/VLM/DocReader/real embedding flows need external systems/assets",
    "ancillary booklet-stale atomic outbox and additional Agent goldens remain deferred",
    "workspace run without optional services reported only expected S3/Neo4j skips; S3 was separately configured and passed"
  ],
  "noStagedFiles": true,
  "diffSummary": "Closed final match identity, concurrent PATCH/merge, paired retry, table extraction, Prompt/tool bounds, diagnostics, smoke false-pass, and configured-S3 async safety defects.",
  "reviewFindings": [
    "no known in-scope blocker after the final fix pass and mandatory validation",
    "no Round 4 was launched because the review cap was reached"
  ],
  "manualNotes": "No commit was created. The active worktree contained broad pre-existing uncommitted product changes."
}
```
