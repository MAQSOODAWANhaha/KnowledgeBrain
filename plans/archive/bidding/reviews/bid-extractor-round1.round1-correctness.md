# Tender Extractor Round 1 — Correctness Review

## Review scope

Fresh-context, read-only inspection of the extraction plan, project review instructions, engine modules, persistence, worker/runtime integration, API/UI surfaces, migration, policy, prompt, and golden fixtures.

There is **one blocker**. No product decision or approval is needed; it is an implementation-correctness issue.

## Blockers / decisions needing approval

### 1. Blocker — High: project-level extraction exclusivity is not concurrency-safe

**Evidence**

- `crates/storage/src/bid.rs:244-262` claims a run using `UPDATE ... AND NOT EXISTS (...)`. Two concurrent transactions can both observe no running row and update different pending rows.
- `migrations/0007_bid.sql:45-63` has no partial unique constraint preventing two `running` rows for one project.
- `crates/bid/src/lib.rs:1130-1205` performs section retry without participating in run claiming, so retry can overlap a full extraction.
- `crates/storage/src/bid.rs:265-283` reclaims stale work merely by changing the same row back to `pending`.
- `crates/storage/src/bid.rs:323-351` finishes by `id` alone, with no lease/attempt token. An old worker can therefore persist or finish after a reclaimed worker has taken ownership.

**Impact**

Concurrent reports can supersede each other’s drafts and overwrite section state and diagnostics. This violates the documented “禁止并行两套抽取” contract and makes transactional re-extraction nondeterministic.

**Concrete fix**

1. Claim inside a transaction that locks the project row (`SELECT ... FOR UPDATE`) before checking/updating run state.
2. Add a defensive partial unique index for one `running` run per project.
3. Model section retries through the same queued run/lease mechanism, or reject/queue retry while a project extraction is active.
4. Add a claim generation/lease token and require it in `persist_extraction_report` and `finish_extract_run`; heartbeat long-running extraction so a reclaimed stale worker is fenced out.
5. Add a two-connection PostgreSQL concurrency test covering simultaneous claims and stale-owner completion.

**Approval decisions:** none.

## Fixes worth doing now

### 2. High: quote validation does not enforce a continuous original-source quote

**Evidence**

- `crates/bid/src/extraction/reconcile.rs:8-26` converts Chinese punctuation, removes all whitespace, and then performs containment on normalized strings.
- `crates/bid/src/extraction/agent.rs:383-385` and `crates/bid/src/extraction/reconcile.rs:58-62` use that permissive predicate.
- The model-supplied quote is subsequently persisted as `raw_text` at `crates/bid/src/lib.rs:706-728`.
- The domain contract requires a continuous original quote (`docs/bid-platform-domain.md:237-240`).

For example, a model quote containing inserted whitespace or ASCII punctuation can pass against source text that does not contain that literal substring.

**Impact**

`raw_text` and `source_span.quote` can differ from the tender source, breaking the core quote-validity invariant and making evidence display unreliable.

**Concrete fix**

Use exact `span.body.contains(quote)` validation. If typography normalization is intentionally supported, build an index mapping normalized positions back to the source and persist the canonical source slice—not the model string. Keep normalized comparison only for deduplication. Add whitespace and Chinese/ASCII punctuation regression tests.

### 3. High: a report containing explicitly failed sections still supersedes old drafts

**Evidence**

- `crates/bid/src/extraction/mod.rs:256-275` marks sections with uncovered candidate spans as `failed`.
- `crates/bid/src/extraction/mod.rs:279-283` nevertheless returns `Ok(ExtractionReport)`.
- `crates/bid/src/lib.rs:686-742` persists every `Ok` report.
- `crates/storage/src/bid.rs:463-471` supersedes all existing drafts for that document before inserting the new clauses.
- Section retry follows the same pattern at `crates/bid/src/lib.rs:1155-1205` and `crates/storage/src/bid.rs:554-560`.

A strict-agent run where both family agents call `done` has valid tool calls, but requirement-like spans remain uncovered. The section is marked failed while the old drafts are still hidden.

**Impact**

An incomplete or zero-clause retry can replace useful prior drafts despite the extraction state explicitly being `failed`, contrary to failure-preservation expectations.

**Concrete fix**

Define report commit eligibility explicitly. The safest document-atomic behavior is to return `ExtractionFailure` whenever any candidate section remains failed, preserving all old document drafts. If partial document commits are required, supersede drafts only for successful sections and retain drafts belonging to failed sections. At minimum, section retry must not call `persist_section_retry` when its returned section status is `failed`. Add full-run and retry regression tests asserting old drafts remain visible.

### 4. High: document-local I/O or persistence errors abort the remaining project run

**Evidence**

- `crates/bid/src/lib.rs:667-681` reads and decodes each document with `?` outside the per-document extraction failure branch.
- `crates/bid/src/lib.rs:733-742` similarly propagates persistence errors out of the document loop.
- The outer handler at `crates/bid/src/lib.rs:579-600` then records only a fatal failure with zero counters.

**Impact**

A missing Markdown blob, invalid UTF-8 file, or document-local persistence failure prevents later completed documents from being attempted. Earlier documents may already have committed, while final diagnostics incorrectly report zero totals and omit their success.

**Concrete fix**

Move load, decode, engine execution, and report persistence into a per-document helper. Record its error in that document’s diagnostics and continue with later documents. Preserve accumulated counters and successful-document diagnostics when finishing the run; reserve whole-run fatal errors for genuinely global failures.

### 5. Medium: section retry can remain permanently `running` and does not validate route ownership

**Evidence**

- `crates/bid/src/lib.rs:1141-1144` marks the section running before `TenderExtractionEngine::from_env()?`; configuration failure exits without marking it failed.
- `crates/bid/src/lib.rs:1197-1205` propagates persistence failure without restoring a terminal section status.
- Only the engine-error branch at `crates/bid/src/lib.rs:1207-1211` marks failure.
- `crates/api/src/routes.rs:3833-3842` discards the project path parameter as `_id`, omits `require_open_project`, and retries solely by section UUID.

**Impact**

Configuration or SQL errors leave stale section state. A caller can also retry a section from a different or ended project by combining its UUID with another route project ID.

**Concrete fix**

Initialize the engine before marking `running`, and wrap all subsequent paths so failures best-effort set `failed` with the error. Validate that `section.project_id == path id`, require the project to be open, and coordinate retry through the project extraction lock described in blocker 1.

### 6. Medium: common `1. Title` headings are not recognized

**Evidence**

- `crates/bid/src/extraction/outline.rs:299-306` consumes `1.` as the numeric prefix and rejects every prefix ending in a period.
- Thus standard headings such as `1. 技术要求` and `1.2. 技术要求` remain body text rather than outline nodes.
- Existing outline tests at `crates/bid/src/extraction/outline.rs:323-339` cover Markdown and Chinese headings but not this common format.

**Impact**

Heading paths, family hints, section identity, span grouping, and reconciliation priors become incorrect for widely used tender numbering.

**Concrete fix**

Treat one terminal period as a delimiter while preserving internal periods for hierarchy depth. Add tests for `1.`, `1.2`, `1.2.`, and numbered list items that should remain body content.

### 7. Medium: missing or ineligible requested documents produce a successful empty run

**Evidence**

- `crates/bid/src/lib.rs:647-658` can produce an empty document list.
- Missing and non-completed requested documents are silently skipped at `crates/bid/src/lib.rs:667-676`.
- With zero successes and zero failures, the final status calculation returns `done`.

**Impact**

A manual re-extraction with no completed documents, or a stale/mismatched document job, is displayed as successful despite processing nothing.

**Concrete fix**

Treat a requested missing/non-completed document as a document failure. For a project run with no eligible documents, either reject enqueueing or finish failed with a clear `no_completed_documents` diagnostic. Also load and validate the run’s stored project/document identity rather than trusting only the job payload.

## Optional improvements

### 8. Low: the file clause cap is applied independently per family and then silently truncates reconciled output

- `crates/bid/src/extraction/agent.rs:135-139` permits each family agent up to the full file cap.
- `crates/bid/src/extraction/mod.rs:181-186` can then skip all sweeps once the combined raw candidate count reaches the cap.
- `crates/bid/src/extraction/reconcile.rs:125-127` sorts and truncates without a truncation diagnostic.

**Impact:** large tenders can lose later-span clauses based on lexical span order, with no direct explanation.

**Fix:** enforce a deliberate shared post-reconciliation budget, reserve sweep/heuristic capacity, and record emitted/dropped counts and affected spans.

### 9. Low: golden regression checks do not calculate extraction precision

- `crates/bid/src/extraction/mod.rs:475-485` defines “unsupported” only as quote-source invalidity.
- `crates/bid/src/extraction/mod.rs:486-519` computes recall and accuracy only over expected matches.
- Extra source-backed false positives pass unless manually listed in `absent_quotes`.

**Impact:** policy changes can increase false positives while all current quality gates remain green.

**Fix:** match actual clauses back to expected clauses, calculate precision per family and overall, and fail on unmatched actual clauses above an explicit threshold.

## Verified-good areas

- **Async ToolChat:** `crates/bid/src/extraction/agent.rs:41-72,252-271` is genuinely async, bounded, and includes retry accounting; no nested runtime is used.
- **Mode semantics:** `crates/bid/src/extraction/mod.rs:124-166` distinguishes strict agent failure, visible hybrid fallback, and model-free heuristic mode.
- **Independent family agents:** both families receive the same complete section set and separate conversations at `crates/bid/src/extraction/mod.rs:124-161`; candidates are reconciled only afterward.
- **Strict tools:** `crates/bid/src/extraction/agent.rs:457-498` requires exactly `span_id/quote/text/must`, rejects extra fields, and locks family server-side.
- **Span coverage:** coverage is keyed by `span_id`, not section, at `crates/bid/src/extraction/coverage.rs:9-28`; sibling spans remain independently uncovered.
- **Stable persisted identity:** section keys exclude body hashes (`crates/bid/src/extraction/outline.rs:53-62`), span IDs derive from section key and ordinal (`outline.rs:87-100`), and SQL enforces `(document_id, section_key)` uniqueness (`migrations/0007_bid.sql:29-43`).
- **Conflict reconciliation:** overlapping candidates are grouped by span, proposed families are retained, and policy arbitration/conflict metadata is persisted at `crates/bid/src/extraction/reconcile.rs:65-123`.
- **Transactional persistence:** section upsert, draft supersession, new clause insertion, and obsolete-section pruning are in one transaction at `crates/storage/src/bid.rs:426-519`; confirmed/rejected clauses are not superseded and preserve obsolete sections.
- **Engine-level failure preservation:** when `TenderExtractionEngine::extract` returns `Err`, document persistence is not entered (`crates/bid/src/lib.rs:682-777`), so old drafts remain intact.
- **Diagnostics/UI visibility:** fallback, failed-document, uncovered-span, and heuristic notices are rendered at `web/src/bid/Workbench.tsx:198-209`; family conflicts are exposed in API/UI.
- **Deployment contract:** both extraction mode and model ID are passed to worker services in `deploy/docker-compose.yml`, with documented defaults in `deploy/.env.example`.

## Validation limitations and residual risks

The available runtime exposed file-reading/search tools but no shell or Git command tool. Therefore `git status`, `git diff`, Rust tests, SQL concurrency tests, formatting, clippy, web build, and migration application could not be executed. Staged-file state is unverified. The concurrency and transactional findings require PostgreSQL integration tests after correction.
