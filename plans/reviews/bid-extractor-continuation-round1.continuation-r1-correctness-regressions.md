# Current Worktree Round-1 Correctness / Regression / Security Review

## Review

### Blocker

- None found.

### High

1. **Match jobs have no claim or generation fence, so an older job can overwrite newer match results.**
   - **Evidence:** `run_match_job` starts work without atomically changing or validating job status at `crates/bid/src/lib.rs:932-1095`. Commercial jobs replace all project hits at `crates/bid/src/lib.rs:1016-1019,1084-1089`. `set_match_job` updates any row by ID without expected-status, project, or generation conditions and assigns `started_at` only when finishing at `crates/storage/src/bid.rs:1063-1087`. Redis uniqueness includes the debounce key, so jobs for different clause generations may run concurrently: `crates/runtime/src/jobs.rs:189-194`. The schema has no running/latest generation constraint for `bid_match_jobs`: `migrations/0007_bid.sql:133-151`.
   - **Failure path:** Job A starts for an old confirmed-clause set and blocks in search. A clause change creates job B with a different debounce key. B finishes and writes current `bid_commercial_hits`; A then finishes and replaces them with stale results. Because A receives the later `started_at`, `latest_match_job[_for_unit]` can also select A as latest (`crates/storage/src/bid.rs:1032-1060`).
   - **Narrow fix:** Atomically claim `pending → running`, setting `started_at` at claim; bind claims and terminal writes to `(job_id, project_id, token/generation)`. Before replacing project-wide hits, verify the job remains the newest generation for that project/unit. Supersede or fence older jobs. Add an old-slow/new-fast PostgreSQL race test and a duplicate-delivery test.

2. **Bid conversion is not fenced against project ending or document deletion.**
   - **Evidence:** Conversion reads the document, then ignores failure when marking it processing at `crates/bid/src/lib.rs:491-503`. Its completion update is only conditioned on document ID, and `set_document_status` does not inspect affected rows or project state at `crates/storage/src/bid.rs:226-249`. Document deletion only checks the extraction lock, not an active conversion, at `crates/storage/src/bid.rs:283-315`. `end_project` likewise fences extraction but not conversion at `crates/storage/src/bid.rs:144-167`. After conversion, the worker requires the document still to exist and requires insertion of a run into an open project at `crates/worker/src/consume.rs:1504-1518`.
   - **Failure paths:**
     - Delete a `processing` document while DocReader is active. The late completion UPDATE affects zero rows but reports success; the worker then returns `converted bid document missing`, causing retries/dead-letter behavior for a deliberately deleted document.
     - End the project during conversion. The converter can still mark its document completed after end, but `insert_extract_run` rejects the ended project, so the conversion job repeatedly fails after performing work. This violates the ended/read-only contract.
   - **Narrow fix:** Add a conversion claim token and heartbeat, acquired only for a pending document on an open project. Condition processing/completed/failed writes on document, project, and token, requiring one affected row. Delete/end must reject or explicitly cancel an active conversion; deleted/ended outcomes should be terminal rather than retryable. Add end-versus-convert and delete-versus-convert database tests.

3. **Clause edits can race full extraction and resurrect a superseded draft.**
   - **Evidence:** The PATCH route reads the current clause without checking the project extraction lease at `crates/api/src/routes.rs:3997-4016`. Full report persistence supersedes all current document drafts transactionally at `crates/storage/src/bid.rs:745-815`. The later API update is keyed only by clause ID and unconditionally writes the client-derived status at `crates/storage/src/bid.rs:957-981`.
   - **Failure path:** PATCH reads a draft; full extraction supersedes it and inserts replacement drafts; PATCH then updates the old row back to `draft` or `confirmed`. A stale confirmed clause is retained by future extraction and enters matching.
   - **Narrow fix:** Make clause mutation ownership- and version-conditioned, e.g. `(project_id, clause_id, expected_status/version)`, and return 409 on zero affected rows. Alternatively serialize edits with the project extraction lease. Add a race test where supersession commits between PATCH read and update.

4. **Confirmed-clause mutation and durable match scheduling are separate commits.**
   - **Evidence:** PATCH commits `update_clause` before invoking `enqueue_bid_match` at `crates/api/src/routes.rs:4052-4075`. Manual creation commits a confirmed clause before scheduling at `crates/api/src/routes.rs:4126-4149`. `schedule_match` performs later reads and inserts the pending job separately at `crates/bid/src/lib.rs:1185-1230`.
   - **Failure path:** A transient DB failure while scheduling occurs after the clause commit. The API returns 500, but no durable pending match exists for housekeeping. Retrying manual creation generates another UUID and duplicates the confirmed clause; not retrying leaves displayed match results stale.
   - **Narrow fix:** Persist a match-dirty/outbox record in the same transaction as the confirmed-set mutation, then let housekeeping create/enqueue match jobs idempotently. At minimum, provide transactional storage operations that update/insert the clause and durable pending match intent together.

### Medium

5. **Automatic convert-to-extract handoff is durable but not idempotent across worker redelivery.**
   - **Evidence:** Every successful `BidConvertWorker` execution generates a fresh run UUID and inserts a new auto run at `crates/worker/src/consume.rs:1512-1518`. Redis deduplicates the active conversion job by document ID (`crates/runtime/src/jobs.rs:173-177`), but `bid_extract_runs` has no conversion-generation/idempotency key (`migrations/0007_bid.sql:68-105`).
   - **Failure path:** The worker persists and enqueues the extraction run, then crashes before acknowledging conversion. Redelivery reconverts the completed document and inserts a second run. Both runs eventually execute serially; the second needlessly supersedes the first run’s drafts and can create fresh drafts after a user confirmed results from the first run.
   - **Narrow fix:** Make automatic run creation an `ensure` operation keyed by document plus immutable converted-Markdown generation/reference, returning the existing run on redelivery.

6. **BID read routes still turn database failures into empty or not-found responses.**
   - **Evidence:** Database connection failures become empty project, document, clause, pick, unit, or shot collections at `crates/api/src/routes.rs:3643-3648,3763-3768,3967-3973,4194-4199,4366-4369,4418-4423`. `get_bid` maps connection/query failure to 404 and derived-query failures to zero values at `crates/api/src/routes.rs:3690-3705`. PATCH itself converts a clause-list query failure into a false 404 via `unwrap_or_default` at `crates/api/src/routes.rs:4004-4013`.
   - **Failure path:** During a PostgreSQL/query failure, the UI reports that bids or clauses do not exist rather than indicating service failure. A PATCH can incorrectly return clause-not-found even though the durable row exists.
   - **Narrow fix:** Use `require_bid_pool` and propagate each primary query failure as 503/500. Reserve empty arrays and 404 for successful queries with genuinely empty results.

## Optional / Deferred

- PostgreSQL integration tests silently skip when the configured database cannot be reached at `crates/storage/src/persist.rs:3684-3694`. CI must provision PostgreSQL and treat absence as a failed required gate; otherwise the lease/race tests provide false confidence.
- API clause enums remain stringly typed. Invalid `family`, `status`, or `assessment` values reach DB CHECK constraints and generally become 500 responses rather than 400 validation errors (`crates/api/src/routes.rs:3997-4066,4105-4145`). Strong request validation remains worthwhile but is secondary to the races above.
- Very long table rows are divided by `split_chars` after being designated a row/span at `crates/bid/src/extraction/outline.rs:155-196,387-398`. A row longer than `max_span_chars` therefore cannot retain the “exact whole Markdown row” invariant. Add an explicit oversized-row policy or adversarial test before accepting unusually large generated tables.
- No legacy upgrade shim is requested. The approved wipe-and-rebuild deployment constraint is respected.

## Verified-Correct Areas

- Full extraction claims, project locks, token-conditioned heartbeats, stale reclaim, report persistence, and finish fencing are coherent at `crates/storage/src/bid.rs:389-575,620-675,745-887`.
- Section retry acquires its section-bound lease before rereading source input and fences status, persistence, and release with the retry token at `crates/bid/src/lib.rs:1251-1400` and `crates/storage/src/bid.rs:336-386,824-912,1420-1445`.
- Per-document report persistence is transactional: sections, draft supersession, clause insertion, and obsolete-section cleanup commit together. Failed extraction does not hide prior drafts.
- `bid_extract_runs.document_id` now uses `ON DELETE CASCADE`, avoiding null reinterpretation as a full-project run: `migrations/0007_bid.sql:68-72`.
- Tool schemas are strict and server-side deserialization denies unknown fields; emitted family is server-locked and quotes must come from the addressed span at `crates/bid/src/extraction/agent.rs:320-470,492-568`.
- Extraction tools expose only the current tender document. The prompt explicitly treats tender text as untrusted and prohibits product/company KB or external access at `crates/bid/prompts/clause-extractor-v2.md:8-17`.
- Table headers are non-quotable; key/value rows retain exact Markdown-row provenance; coordinated requirements and mandatory lists receive separate coverage units at `crates/bid/src/extraction/outline.rs:155-196,254-380`.
- Reconciliation revalidates literal provenance and exact table-row quotes at `crates/bid/src/extraction/reconcile.rs:35-61`. Automatic extraction persists only `draft`; matching reads only `confirmed` clauses at `crates/storage/src/bid.rs:976-990`.
- Evaluator assignment is exact/explicit-alias and one-to-one, and unlabeled CLI runs report `NOT_EVALUATED`, not PASS, at `crates/bid/src/extraction/evaluation.rs:99-324` and `crates/bid/src/bin/bid_extract_eval.rs:26-39`.
- Global visibility is intentional: authenticated users/API keys may access all projects under the approved single-company model (`docs/bid-platform-domain.md:13,25-28,94-100`).

## Validation Limitations and Residual Risks

No shell or Git command facility was available. I inspected the current files and repository plans/reviews directly, but could not attest the exact Git diff, staged state, or exhaustive untracked-file list. No tests were executed. The supervisor should run:

- `git status --short --untracked-files=all`
- `git diff --check`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` with a required real PostgreSQL instance
- `npm -C web run build`

The most important missing tests are old-slow/new-fast match completion, duplicate match delivery, convert-versus-end/delete, extract-supersede-versus-clause-PATCH, auto-run redelivery idempotency, and failure between clause commit and durable match intent.

## Decision

**Another immediate focused fix worker is warranted.** It should prioritize match-job claim/generation fencing, conversion lifecycle fencing, optimistic clause-update fencing, and transactional match intent. The auto-extraction idempotency and read-error propagation fixes should follow in the same bounded pass if practical.