# Continuation Round-2 fix worker handoff

## Completed

- Match scheduling now carries the clause-snapshot `expected_generation` into the insert transaction. A stale scheduler cannot relabel an old snapshot as current. Fresh schema permits one authoritative job per `(project, generation, unit)`; duplicate insertion returns the existing job and failed same-generation jobs can be retried.
- Confirmed clause insert/patch and Section merge use project-first locking and atomically update `match_generation/match_dirty`; older pending/running jobs are immediately fenced.
- Project end atomically cancels/fences pending/running conversion, extraction, match, and Section-retry state. Conversion/match heartbeats and publication require an open project/current generation.
- Conversion/VLM failures remain pending for bounded Oxana retry, become visibly failed at retry exhaustion, and can be claimed again. Completed documents require multimodal `done|skipped`.
- Durable Section retry job and project/Section lease are now acquired together with one token. Busy projects leave the intent pending without spending retries; paired stale reclaim clears both leases; token-conditioned finish must succeed.
- Local blob/image/Markdown write failures propagate. Configured S3 is required write-through durability and bucket/PUT errors are no longer ignored.
- Neutral inventory tables no longer become candidates merely because they contain `|`; table chrome moved to typed Policy. Oversized exact rows are retained for provenance but are not offered as candidates, and Policy validates that accepted spans fit a complete `read_span` result.
- Compact latest-extract output and UI expose partial multi-document failure.
- `bid_extract_eval` writes a FAIL diagnostic JSON/Markdown artifact before returning nonzero on extraction failure.
- Worker readiness performs a real Redis operation, reconnect backoff resets, detached signal tasks are aborted, and BID/default workers are no longer registered in both core and shared topology.
- BID preview/booklet/export use explicit DB error propagation; invalid export format returns 400. Clause decoration and required booklet-stale errors are propagated.
- Draft UI now edits family/must, rejected is distinct from draft and can be restored, Section retry status/error is exposed and duplicate actions are disabled, and remount no longer automatically creates a manual extraction run.
- Docker ARG scope, CJK PDF font, LDAP Compose forwarding, production JWT/LDAP guidance, required PostgreSQL CI tests, image build gate, and service smoke gate were corrected.
- The deterministic service-backed smoke now verifies real PostgreSQL migrations/state, Redis queue, API+worker, local blob roundtrip, heuristic conversion/extraction diagnostics, edit/reject/confirm, match terminal state, durable Section retry, booklet/preview, DOCX/PDF signatures, manual re-extraction invariants, and ended-project read-only behavior. It prints external/stub boundaries honestly.
- Domain/system/deployment docs were updated; `docs/system-design.md` remains byte-identical to `.scratch/knowledgebrain/spec.md`.

## Main changed files in this worker pass

- `.github/workflows/ci.yml`
- `.scratch/knowledgebrain/spec.md`
- `crates/api/src/routes.rs`
- `crates/bid/config/cn-tender-v2.json`
- `crates/bid/src/bin/bid_extract_eval.rs`
- `crates/bid/src/extraction/coverage.rs`
- `crates/bid/src/extraction/outline.rs`
- `crates/bid/src/extraction/policy.rs`
- `crates/bid/src/lib.rs`
- `crates/runtime/src/jobs.rs`
- `crates/storage/src/bid.rs`
- `crates/storage/src/lib.rs`
- `crates/storage/src/persist.rs`
- `crates/storage/src/s3.rs`
- `crates/worker/src/consume.rs`
- `crates/worker/src/main.rs`
- `deploy/Dockerfile.rust`
- `deploy/README.md`
- `deploy/docker-compose.yml`
- `docs/bid-platform-domain.md`
- `docs/system-design.md`
- `migrations/0007_bid.sql`
- `scripts/bid_e2e_smoke.sh`
- `web/src/api.ts`
- `web/src/bid/ClauseDetail.tsx`
- `web/src/bid/ClauseTable.tsx`
- `web/src/bid/Inspector.tsx`
- `web/src/bid/Sidebar.tsx`
- `web/src/bid/Workbench.tsx`

## Tests added or extended

- PostgreSQL stale scheduler generation rejection and authoritative duplicate-job identity.
- Match generation/old-owner fencing.
- Active conversion/match fencing on project end.
- Transient conversion failure followed by successful reclaim/completion.
- Paired Section-retry job/project token and stale reclaim.
- Neutral inventory table skip and exact technical key/value row extraction.
- Oversized exact-row candidate bound.
- Expanded real service-backed BID smoke.

## Validation evidence

- `cargo check --workspace --all-targets` — exit 0.
- `cargo test -p bid --lib` — exit 0, 55 passed.
- `cargo test -p api --lib` — exit 0.
- `cargo test -p storage -- --nocapture` against a fresh temporary PostgreSQL database — exit 0 (27 passed at that checkpoint).
- `cargo test --workspace` with fresh PostgreSQL and Redis — exit 0; final storage set 28 passed and all workspace suites passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — exit 0.
- `cargo fmt --all -- --check` — exit 0.
- `npm -C web run build` — exit 0.
- `docker compose -f deploy/docker-compose.yml --env-file deploy/.env.example config -q` — exit 0.
- `scripts/bid_e2e_smoke.sh` with a fresh PostgreSQL DB and disposable Redis — exit 0; printed `bid service-backed smoke: PASS`.
- `bash -n scripts/bid_e2e_smoke.sh` — exit 0.
- `cmp -s docs/system-design.md .scratch/knowledgebrain/spec.md` — exit 0.
- `git diff --check` — exit 0.
- No staged files (`git diff --cached --quiet` exit 0).

## Remaining work / risks

- The smoke does not seed a real product/company asset, so pick/unpick and shot upload/read/delete remain outside this service smoke; matching reaches a real durable terminal job but uses the deterministic embedding boundary with no candidate asset.
- No Redis/PostgreSQL restart fault-injection test was added. Redis availability is genuinely probed and static recovery is covered, but stop/start consumption still needs a dedicated integration test.
- Strict Agent, VLM, non-Markdown DocReader, LDAP, and real embedding/model paths require external services and credentials and remain manual.
- The requested additional unheaded-long-prose/long-table golden files and full Agent-mode ScriptedToolChat golden with invalid sweep rejection were not completed. Evaluator failure artifacts are fixed.
- Forced local-write/S3-500 tests were not added. The code now propagates both local and configured-S3 write failures; ordinary local mode remains independent of S3.
- The new tests prove stale scheduler rejection, authoritative generation identity, and stale owner fencing, but do not implement a timing-controlled old-slow/new-fast search provider test that publishes both technical candidates and commercial hits.
- Paired Section retry ownership/reclaim is tested, but a dedicated full-extraction-contention timing test is still desirable.
- Dockerfile parsing is covered by Compose and CI now has an actual image-build gate, but this worker did not run the full Docker image build locally. Host PDF export was exercised by the service smoke.
- BID error propagation was improved on reviewed core routes, but a future audit should verify every ancillary shot/stale mutation rather than infer completeness.
- The active worktree contains broad pre-existing uncommitted product work. This worker made no commit and staged nothing.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Implemented the synthesized Round-2 correctness, lifecycle, extraction, deployment, UI, and deterministic service-smoke fixes without adding a new product mode; full workspace tests, Clippy, web build, Compose validation, and the expanded smoke pass. Explicit lower-priority/external gaps are listed rather than half-claimed."
    }
  ],
  "changedFiles": [
    ".github/workflows/ci.yml",
    "crates/api/src/routes.rs",
    "crates/bid/config/cn-tender-v2.json",
    "crates/bid/src/bin/bid_extract_eval.rs",
    "crates/bid/src/extraction/coverage.rs",
    "crates/bid/src/extraction/outline.rs",
    "crates/bid/src/extraction/policy.rs",
    "crates/bid/src/lib.rs",
    "crates/runtime/src/jobs.rs",
    "crates/storage/src/bid.rs",
    "crates/storage/src/lib.rs",
    "crates/storage/src/persist.rs",
    "crates/storage/src/s3.rs",
    "crates/worker/src/consume.rs",
    "crates/worker/src/main.rs",
    "deploy/Dockerfile.rust",
    "deploy/README.md",
    "deploy/docker-compose.yml",
    "docs/bid-platform-domain.md",
    "docs/system-design.md",
    "migrations/0007_bid.sql",
    "scripts/bid_e2e_smoke.sh",
    "web/src/api.ts",
    "web/src/bid/ClauseDetail.tsx",
    "web/src/bid/ClauseTable.tsx",
    "web/src/bid/Inspector.tsx",
    "web/src/bid/Sidebar.tsx",
    "web/src/bid/Workbench.tsx"
  ],
  "testsAddedOrUpdated": [
    "crates/storage/src/persist.rs lifecycle/generation/retry tests",
    "crates/bid/src/extraction/coverage.rs neutral/exact table tests",
    "crates/bid/src/extraction/outline.rs oversized-row test",
    "scripts/bid_e2e_smoke.sh expanded service flow"
  ],
  "commandsRun": [
    {"command":"cargo check --workspace --all-targets","result":"passed","summary":"workspace and all targets compile"},
    {"command":"cargo test --workspace (fresh PostgreSQL + Redis)","result":"passed","summary":"all workspace tests passed; storage 28/28"},
    {"command":"cargo clippy --workspace --all-targets -- -D warnings","result":"passed","summary":"no Clippy warnings"},
    {"command":"cargo fmt --all -- --check","result":"passed","summary":"format clean"},
    {"command":"npm -C web run build","result":"passed","summary":"TypeScript and Vite production build passed"},
    {"command":"scripts/bid_e2e_smoke.sh (fresh PostgreSQL + disposable Redis)","result":"passed","summary":"expanded deterministic service-backed BID flow passed"},
    {"command":"docker compose -f deploy/docker-compose.yml --env-file deploy/.env.example config -q","result":"passed","summary":"Compose contract valid"},
    {"command":"git diff --check && cmp system-design/spec","result":"passed","summary":"diff whitespace clean and design mirror identical"},
    {"command":"docker build -f deploy/Dockerfile.rust ...","result":"not-run","summary":"added as CI gate; not run locally due checkpoint instruction"}
  ],
  "validationOutput": [
    "workspace tests passed with mandatory fresh PostgreSQL and Redis",
    "bid service-backed smoke: PASS",
    "Clippy -D warnings passed",
    "web production build passed",
    "no staged files"
  ],
  "residualRisks": [
    "pick/shot asset flow and dependency restart injection are not in the deterministic smoke",
    "strict Agent/VLM/DocReader/LDAP/real embedding need external services",
    "expanded Agent-mode goldens and forced S3 failure tests remain",
    "full Docker build was delegated to the new CI gate"
  ],
  "noStagedFiles": true,
  "diffSummary": "Fenced generation/lifecycle races, paired durable retries, propagated object failures, closed table/partial-failure contracts, corrected worker/deploy/UI behavior, and expanded mandatory service validation.",
  "reviewFindings": [
    "no known compile/test blocker after this pass",
    "remaining gaps are explicitly listed for Round 3 classification"
  ],
  "manualNotes": "No commit was created. The worktree already contained broad uncommitted changes before this worker pass."
}
```
