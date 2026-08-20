# Bid Extractor Round 3 — Final Synthesis

## Scope and stopping rule

Round 3 was the third and final fresh-context, read-only review round. Three reviewers independently inspected lease correctness, extraction quality, and API/storage/deployment contracts. The parent session evaluated each finding, implemented the accepted fixes as the sole writer, and reran the complete validation suite. No fourth review round is permitted by the agreed cap.

The original reports remain beside this synthesis:

- `bid-extractor-round3.round3-lease-correctness.md`
- `bid-extractor-round3.round3-extraction-quality.md`
- `bid-extractor-round3.round3-contracts-docs.md`

## Accepted findings and completed fixes

### Lease and persistence correctness

- Section retry now acquires its section-bound project lease before reading the Section input, then rereads while holding the lease. Lookup failures release the owned lease.
- `bid_extract_runs.document_id` now uses `ON DELETE CASCADE`; deleting a document cannot reinterpret a document-scoped run as a full-project run.
- Document deletion/retry lock the project, require it to remain open, and reject an active extraction lease.
- Project-open validation and document/run insertion are serialized with project ending through a project-row lock. Ended projects cannot receive late documents or extraction runs.
- PostgreSQL coverage now verifies pending-run cascade, active-lease deletion fencing, and rejection of late inserts after project end.

### Extraction quality and provenance

- Table parsing distinguishes self-contained requirement cells from key/value rows.
- Key/value rows retain the exact original Markdown row as `quotable_text`; a partial field/value quote is rejected, while headers remain non-quotable context.
- Header-only tables produce no quotable Span.
- Numbered structural headings containing family signals remain headings; numbered requirement lines are preserved using requirement grammar rather than family nouns alone.
- Coordinated modal/predicate clauses and mandatory `、…和/及…` lists become sibling coverage units. Later units carry non-quotable preceding-requirement context so inherited family/must semantics remain available without fabricating quote text.
- Regression tests cover key/value tables, exact-row quote enforcement, signal-bearing headings, inherited coordinated clauses, and enumerated requirements.

### API, worker, and deployment contracts

- API and worker now complete PostgreSQL connection, migrations, and company-workspace initialization before readiness or queue consumption. Initialization failure terminates startup.
- Bid workers no longer acknowledge jobs when their PostgreSQL pool is absent.
- Convert-to-extract handoff requires a durable pending extraction run before conversion work is acknowledged; transient enqueue failure remains recoverable by housekeeping.
- Remaining BID mutation routes now require a database pool and propagate primary storage failures instead of returning false success.
- Manual clause creation validates Section ownership.
- Shot insertion validates clause/project and product/version relationships; shot deletion is project-scoped.
- Pick deletion is ownership-conditioned and requires an affected row.
- Match scheduling persists the job first and treats Redis enqueue as recoverable through housekeeping.
- Startup-failure launch tests were added for API and worker.

## Rejected or deferred

- No legacy `ALTER` upgrade migration was added. The approved deployment policy explicitly permits wiping old volumes and rebuilding from the consolidated fresh schema.
- A global cross-document LLM token/cost budget remains optional product work; existing per-agent/file/tool limits remain enforced.
- A dedicated Section-retry UI and stronger TypeScript/Rust enum typing remain non-blocking UX/maintainability work.
- Broader natural-language coordination beyond the bounded modal/predicate and mandatory-list grammar remains future extraction-policy work and must be evaluated through new golden labels before expansion.
- PostgreSQL tests still skip when no database is available; CI must provision PostgreSQL for the database gate. This final validation used a real temporary PostgreSQL database.

## Final validation evidence

All checks passed after the Round-3 fixes:

- `cargo test --workspace`
- BID library tests: 50 passed
- Storage tests against temporary PostgreSQL: 22 passed
- API/worker startup-failure and normal launch tests passed
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- fresh PostgreSQL migration smoke for `0001`–`0008`, including CASCADE document-run FK and running-run unique index
- `cd web && npm run build`
- `docker compose -f deploy/docker-compose.yml --env-file deploy/.env.example config -q`
- `cmp docs/system-design.md .scratch/knowledgebrain/spec.md`
- `git diff --check`

## Final decision

No known blocker remains within the approved BID extraction-hardening scope. The review loop stops because the three-round cap has been reached, all accepted blocker/high findings were repaired, and the full validation suite passes. Deferred items are optional follow-up work rather than reasons to extend this hardening cycle.
