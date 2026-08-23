# Bid Extractor Round 1 — Contracts & Maintainability Review

## Review scope

Fresh-context, read-only inspection of the named plan, schema, storage, extraction engine, API, web UI, deployment configuration, documentation mirrors, fixtures, evaluator, and tests.

No files were edited and no commands were run. The current Git diff/status could not be enumerated because this review environment provided file-reading tools but no Git command facility; findings below concern the current working-tree contents.

## Blockers and decisions needing approval

### Blocker — High: project-level extract serialization is race-prone

`claim_extract_run` uses an `UPDATE ... AND NOT EXISTS` predicate to prevent another run from becoming `running` (`crates/storage/src/bid.rs:244-262`). Two concurrent transactions can both observe no running row and update different pending rows. The migration has no partial unique constraint or other database serialization mechanism for one running run per project (`migrations/0007_bid.sql:45-63`).

This can allow concurrent runs to race through document persistence and supersede each other’s drafts.

**Required before proceeding:** serialize claims with a project-scoped transaction/advisory lock or an equivalent database-enforced mechanism. A partial unique index alone would also require treating the expected uniqueness conflict as “not claimed,” rather than a worker error.

### Decision — High: partial reports currently replace drafts from failed sections

The engine marks a section `failed` when candidate spans remain uncovered, but still returns `Ok(ExtractionReport)` (`crates/bid/src/extraction/mod.rs:264-283`). The caller consequently records the document as done and persists it (`crates/bid/src/lib.rs:693-760`). Persistence supersedes every existing draft for the document before inserting the partial result (`crates/storage/src/bid.rs:463-500`).

Therefore an uncovered/failed section can hide a useful draft from the prior run, despite the stated “failed extraction preserves old draft” behavior.

Approval is needed for one of these semantics:

1. Treat any failed section as document extraction failure and preserve all old document drafts; or
2. Commit successful sections only and preserve old drafts belonging to failed sections.

Until this is decided, partial extraction has a user-visible data-retention ambiguity.

**Blockers present: yes — one concurrency blocker, plus the partial-report policy decision above.**

## Fixes worth doing now

### Medium: conflict confirmation can bypass the requested classification review

The Inspector tells users to choose the correct family before confirming (`web/src/bid/Inspector.tsx:72-87`), but all confirmation handlers submit only `{status: "confirmed"}` (`web/src/bid/Workbench.tsx:360`, `:526`, `:564`). The table’s quick-confirm button exposes that path directly. Storage then clears `family_conflict` on any confirmation (`crates/storage/src/bid.rs:606-612`).

A user can therefore confirm the suggested family without making the explicit classification choice implied by the UX. Either require an explicit family selection for conflicting drafts or change the wording to make “confirm suggested classification” an intentional option.

### Medium: persistence and API enums remain stringly typed

Core storage inputs use `&str` for family, status, assessment, section hint, and extraction mode (`crates/storage/src/bid.rs:6-41`). The PATCH route accepts arbitrary strings without request validation (`crates/api/src/routes.rs:3896-3912`, `:3938-3947`); invalid values reach database checks and become internal errors rather than a validation response. TypeScript likewise models family/status/mode as unrestricted strings (`web/src/api.ts:59-65`, `:87-96`).

Introduce shared Rust enums or explicit boundary validation and TypeScript string unions. Typed JSON structs for `source_span`, `extraction_meta`, diagnostics, and `latest_extract` would also reduce manual contract drift.

### Medium: evaluator and golden checks do not measure false-positive precision

The golden test defines “unsupported” solely as clauses whose quotes do not occur in their span (`crates/bid/src/extraction/mod.rs:469-485`). Recall and accuracy are calculated only by searching expected clauses (`crates/bid/src/extraction/mod.rs:486-519`). Extra source-grounded but semantically incorrect clauses pass unless they match the small explicit `absent_quotes` list (`crates/bid/src/extraction/mod.rs:528-536`).

The real-model evaluator only emits the raw report and basic counts; it does not load expected labels or report precision, recall, family/must accuracy, duplicate rate, or threshold pass/fail (`crates/bid/src/bin/bid_extract_eval.rs:8-46`). This falls short of the planned regression evaluator.

Add one-to-one expected/actual matching, semantic precision, and threshold results to the evaluator and offline check.

### Medium: bid persistence query paths lack supporting indexes

`migrations/0007_bid.sql` contains the stable section uniqueness constraint but no explicit indexes. Frequent paths include:

- claim/running and latest-run lookups by project/status/order (`crates/storage/src/bid.rs:232-262`, `:356-370`);
- document draft supersession by source document/status (`crates/storage/src/bid.rs:463-470`);
- section cleanup by `section_id/status` (`crates/storage/src/bid.rs:505-516`);
- pending-file existence checks by source document (`crates/storage/src/bid.rs:1074-1083`).

Add targeted indexes for these access patterns, especially `bid_extract_runs(project_id, status)`, latest-run ordering, and clause source/section status lookups.

### Low: invalid-quote diagnostics are counted repeatedly

The same accumulated candidate set is reconciled three times, and each reconciliation’s rejected count is added (`crates/bid/src/extraction/mod.rs:168-170`, `:219-220`, `:234-235`). A candidate invalid from the beginning can therefore be counted more than once, in addition to dispatch-time rejection.

Report per-stage counters or count only newly rejected candidates/final unique rejections.

### Low: rejection-term policy remains duplicated in Rust

The plan and domain documentation state that adjustable classification and must terms live in `cn-tender-v2`, but `has_veto_term` hard-codes four terms (`crates/bid/src/extraction/outline.rs:113-118`) which overlap `must.hard` in `crates/bid/config/cn-tender-v2.json:23-26`.

Derive this check from the policy so policy changes do not silently diverge from candidate-span reopening behavior.

## Optional improvements and residual risks

### Coverage can still mask omissions inside a multi-requirement span

Coverage marks a span covered after any clause references it (`crates/bid/src/extraction/coverage.rs:17-28`). A table span may contain up to 20 rows (`crates/bid/config/cn-tender-v2.json:37`; `crates/bid/src/extraction/outline.rs:82-97`). One extracted row therefore prevents sweep/fallback for other missed rows in the same span.

The golden table fixture helps detect deterministic heuristic regressions, but this remains a live-model blind spot. Consider requirement-unit or row-level coverage, or smaller table spans.

### `covered_spans` can exceed `candidate_spans`

The final covered set includes every span referenced by a clause, while `candidate_spans` includes only spans marked as candidates (`crates/bid/src/extraction/mod.rs:237-252`). Agents can read and emit from non-candidate spans, so the diagnostic ratio is not guaranteed to be bounded. Intersect the covered set with candidate span IDs for reporting.

### Database behavior lacks focused integration tests

Engine tests cover tool schema, fallback modes, conflicts, quote validity, span partitioning, and golden fixtures, but no storage tests were found for:

- concurrent run claims;
- transaction rollback;
- partial-failure preservation;
- section cleanup with confirmed/rejected references;
- latest-extract API serialization;
- replay/idempotency after a crash between report commit and run finalization.

These are the highest-risk persistence seams and merit PostgreSQL integration tests.

## Verified-good areas

- **Migration constraints:** Stable section identity is enforced with `UNIQUE(document_id, section_key)` and section/run/clause enums have appropriate checks (`migrations/0007_bid.sql:29-85`).
- **Foreign-key cleanup:** Sections and documents cascade appropriately; clause evidence references use `SET NULL`, while cleanup preserves sections referenced by confirmed or rejected clauses (`migrations/0007_bid.sql:29-85`; `crates/storage/src/bid.rs:505-516`).
- **Per-document transaction:** Section upsert, old-draft supersession, clause insertion, and obsolete-section cleanup occur in one transaction (`crates/storage/src/bid.rs:426-520`).
- **Stable persistence:** Existing sections retain their ID through conflict upsert, so confirmed/rejected references remain stable (`crates/storage/src/bid.rs:437-461`).
- **Evidence JSON:** `source_span` contains `span_id`, `heading_path`, and quote, while extraction metadata is persisted with conflict/provenance fields (`crates/bid/src/lib.rs:706-730`; `crates/bid/src/extraction/reconcile.rs:105-131`).
- **Quote validation and tool schema:** Tool output is server-family-locked, required fields are strict, unknown fields are denied, and quotes must resolve within the selected span (`crates/bid/src/extraction/agent.rs:329-385`, `:456-490`).
- **Independent agents and fallback visibility:** Technical/commercial agents produce independent candidates; strict agent failures and hybrid/heuristic fallback reasons are represented in diagnostics (`crates/bid/src/extraction/mod.rs:119-166`, `:172-231`).
- **Latest-extract contract:** API fields consumed by TypeScript align, including nested per-document diagnostics (`crates/api/src/routes.rs:3690-3716`; `web/src/api.ts:76-105`). The API additionally sends timestamps and trigger information, which TypeScript safely ignores.
- **UI diagnostics:** Failed documents, uncovered spans, and heuristic/fallback operation produce visible banners (`web/src/bid/Workbench.tsx:198-212`, `:398-406`).
- **Deployment:** Compose passes both extraction variables to worker through shared environment configuration, defaults to hybrid, and falls back to the main chat model (`deploy/docker-compose.yml:24-31`; `crates/bid/src/extraction/agent.rs:99-109`). The example environment accurately documents this (`deploy/.env.example:37-40`).
- **Documentation:** The inspected system-design/spec mirror seam is consistent, and the domain document accurately describes the current engine, diagnostics, span coverage, persistence, and fallback contracts (`docs/system-design.md:254-256`; `.scratch/knowledgebrain/spec.md:254-256`; `docs/bid-platform-domain.md:176-295`).
- **Legacy cleanup:** No old `extract_agent.rs`, alternate prompt, or parallel extraction implementation remains under `crates/bid/src`; the extraction engine is consolidated under `crates/bid/src/extraction/`.
- **Fixture thresholds:** The two fixtures enforce the requested quote, recall, family, must, and duplicate thresholds; with their small label counts the 0.90/0.95 thresholds effectively require all expected clauses (`testdata/bid-extraction/cn-tender-golden-01.expected.json:8-15`; `cn-tender-golden-02.expected.json:18-25`).

## Validation status

No executable validation was run because shell execution was unavailable and the task prohibited file edits. The supervisor should run:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p bid -p storage -p worker -p api`
- `npm -C web run build`
- `cmp docs/system-design.md .scratch/knowledgebrain/spec.md`
- a PostgreSQL concurrency test for two simultaneous `claim_extract_run` calls
