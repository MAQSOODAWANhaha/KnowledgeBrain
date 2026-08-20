# Bid Extractor Round 2 — Correctness Review

## Review scope

Fresh-context, read-only inspection of:

- `plans/bid-extractor-hardening.md`
- all Round-1 reports under `plans/reviews/`
- `migrations/0007_bid.sql`
- extraction engine, outline, coverage, reconciliation, persistence, worker, API retry, evaluation, and tests
- current repository files directly, independent of the timed-out fix worker

No files were edited. Git status/diff and executable tests could not be run because this environment exposes file inspection tools but no shell/Git runner.

## Blockers / approval decisions

### Blocker — High: live extraction can be reclaimed solely because its original start time exceeds the housekeeping threshold

**Evidence**

- Full-run stale detection uses immutable `started_at`, not a heartbeat: `crates/storage/src/bid.rs:333-346`.
- Section retries similarly set `extract_lock_at` once at claim and reclaim solely by its age: `crates/storage/src/bid.rs:244-262,373-381`.
- The shared stale threshold is only 2h10m: `crates/runtime/src/lib.rs:41-46`.
- Housekeeping applies that threshold to extraction runs: `crates/worker/src/consume.rs:1451-1456`.
- A hybrid run may perform 80 span sweeps, two families, three 60-second attempts per request, in addition to initial family-agent rounds: `crates/bid/config/cn-tender-v2.json:36-51`; `crates/bid/src/extraction/mod.rs:173-217`. Multiple documents are processed sequentially at `crates/bid/src/lib.rs:693-734`.
- A repository-wide search found no bid-extraction heartbeat update.

**Impact**

A healthy but long-running project or section extraction can lose its lease. Token fencing prevents the old worker from persisting afterward, but the replacement starts over and can encounter the same timeout repeatedly. Large projects may therefore never finish, and model work is wasted.

**Concrete fix**

Add a token-conditioned heartbeat for both full and section-retry leases, updated periodically by the caller while the engine runs. Reclaim based on the latest heartbeat, not `started_at`. Keep `started_at` as historical metadata. Add an integration test where extraction exceeds a short stale threshold while heartbeats prevent reclaim, followed by a stopped-heartbeat case that verifies reclaim and stale-owner fencing.

**Approval decision:** none; this is an implementation-correctness blocker.

## Fixes worth doing now

### High: exact quote validation is still against a synthesized span, not necessarily the original document

**Evidence**

- Each table row span is synthesized by repeating the table header: `crates/bid/src/extraction/outline.rs:163-175`.
- For a later row, `header + "\n" + row` is not a continuous substring of the original Markdown because preceding data rows occur between them.
- Quote validation only checks `span.body.contains(quote)`: `crates/bid/src/extraction/reconcile.rs:24-25,49-55`.
- The accepted quote is persisted directly as `raw_text` and provenance: `crates/bid/src/lib.rs:806-835`.

**Impact**

A model can quote across a synthesized header/row boundary and pass validation even though that literal text never occurred continuously in the tender. This still violates the original-source quote invariant and the 100% quote-validity gate.

**Concrete fix**

Retain source offsets for quotable span text, or keep repeated table headers as non-quotable context separate from the row source. Validate final quotes against the original `ExtractionInput.markdown` and persist the canonical source slice. Add a regression test using a second table row and a quote spanning the repeated header and that row.

### High: numbered requirement lines remain lossy for common delimiters and non-hard wording

**Evidence**

- `numbered_requirement_line` recognizes all numeric/Chinese numbering, but coverage-trigger checks are restricted to `)`/`）` list style; other formats are preserved only if they contain a hard-must term: `crates/bid/src/extraction/outline.rs:262-289`.
- The heading parser then classifies short `1.`, `1、`, and Chinese `一、` lines without terminal punctuation as headings: `crates/bid/src/extraction/outline.rs:296-341`.
- Therefore lines such as `1、设备应支持 IPv6` or `一、设备应兼容 IPv6` can become headings and disappear from span bodies. `应` is a coverage trigger but not a hard term, and `1、` is not considered `list_style`.
- The current regression test covers `1）设备支持万兆接口` and a hard `不得` example, but not these variants: `crates/bid/src/extraction/outline.rs:403-424`.

**Impact**

Valid tender requirements can be unavailable to both agents and heuristic extraction, producing silent recall loss or an uncovered-document failure.

**Concrete fix**

Use requirement signals for every numbered delimiter, not only `)`/`）`, while preserving actual structural headings. For ambiguous lines, keep the text in an extractable source span even if it also establishes outline structure. Add cases for `1.`, `1、`, `一、`, `（一）`, and `1）` with `应`, optional wording, family signals, and actual headings.

### High: stale section-retry owners are not fully fenced, and crash cleanup leaves stale section state

**Evidence**

- The project retry lease contains no section identity: `migrations/0007_bid.sql:10-18`.
- Stale retry cleanup only clears the project lock and does not transition the affected `bid_sections.extract_status`: `crates/storage/src/bid.rs:373-381`.
- `finish_section_retry` returns success even if its token matched no row: `crates/storage/src/bid.rs:265-279`.
- Clause replacement is correctly token-fenced in `persist_section_retry`: `crates/storage/src/bid.rs:662-682`.
- However, after persistence failure or engine failure, the caller performs unconditional, unfenced section status writes: `crates/bid/src/lib.rs:1258-1284`.

**Impact**

A crashed retry can leave a section permanently `running`. More seriously, after housekeeping reclaims the retry and a new retry or full extraction succeeds, the stale worker can resume, lose its persistence lease, and then overwrite the fresh section status to `failed`.

**Concrete fix**

Associate the retry lease with its section ID, require the token for every section-status mutation, and make stale reclaim transition the leased section to a terminal/retryable state. Treat zero-row release as lease loss. Prefer representing section retry as a queued leased run if that avoids a second lease protocol. Add a race test where an old retry resumes after reclaim and cannot modify status or drafts owned by the new worker.

## Optional / deferred

### Low: focused run-level integration coverage remains incomplete

The storage suite now tests simultaneous project claims, stale-owner fencing, retry/full exclusion, and transactional rollback at `crates/storage/src/persist.rs:3760-3959`. No focused tests were found for:

- periodic heartbeat versus live stale reclaim;
- stale section retry resuming after another owner;
- crash after one document commit but before run finalization;
- project runs mixing successful and document-local failures.

These tests should accompany the fixes above. The existing database tests also return early when PostgreSQL setup is unavailable, so a green unit-test invocation alone does not attest that they executed.

## Verified-fixed items

- **Concurrency-safe project claim:** the claim transaction locks `bid_projects` with `FOR UPDATE`, validates stored run/document identity, and installs the project token at `crates/storage/src/bid.rs:283-330`. A partial unique running-run index provides defense in depth at `migrations/0007_bid.sql:65-67`. The concurrent claim test is at `crates/storage/src/persist.rs:3760-3801`.
- **Normal project lock lifecycle:** successful claim installs the full-project lock, while `finish_extract_run` conditionally updates the running row and clears the matching project token in one transaction at `crates/storage/src/bid.rs:426-474`.
- **Full-run lease fencing:** report persistence requires matching run and project tokens at `crates/storage/src/bid.rs:549-574`; finishing requires the claim token at `crates/storage/src/bid.rs:430-473`. The stale-owner test checks both persist and finish rejection at `crates/storage/src/persist.rs:3803-3846`.
- **Full/retry mutual exclusion:** section retry atomically claims the project lock and rejects an active full run at `crates/storage/src/bid.rs:244-262`; full claims reject any existing project extraction lock. The API validates open-project ownership at `crates/api/src/routes.rs:3833-3849`.
- **Document-atomic failure preservation:** uncovered candidate spans now return `ExtractionFailure` rather than a persistable partial report at `crates/bid/src/extraction/mod.rs:276-305`. Persistence is entered only after successful extraction at `crates/bid/src/lib.rs:794-850`, and all draft supersession/insertion occurs transactionally. Rollback preservation is tested at `crates/storage/src/persist.rs:3862-3959`.
- **Per-document error isolation:** lookup, eligibility, blob loading, UTF-8 decoding, extraction, and persistence are isolated in `extract_one_document` at `crates/bid/src/lib.rs:767-853`; the outer loop records document diagnostics and continues at `crates/bid/src/lib.rs:693-734`. Lease loss remains correctly fatal.
- **Empty/ineligible requested runs:** a project run with no completed documents fails with `no_completed_documents` at `crates/bid/src/lib.rs:664-690`. A requested missing, mismatched, or non-completed document fails locally at `crates/bid/src/lib.rs:775-790`.
- **Exact literal validation within a span:** whitespace and punctuation normalization is no longer used for provenance acceptance; `quote_in_body` uses exact containment at `crates/bid/src/extraction/reconcile.rs:24-25`, with regression coverage at `:219-224`. The synthesized-table exception remains above.
- **Standard numbered headings:** trailing-period forms such as `1.` and `1.2.` are parsed through terminal-period stripping at `crates/bid/src/extraction/outline.rs:347-370`, with tests at `:403-424`. Requirement discrimination is not yet complete.
- **Requirement-sized coverage:** prose is split on sentence/newline boundaries, list items become individual spans, and tables become one span per row at `crates/bid/src/extraction/outline.rs:145-234`. Dense prose/table behavior is tested at `:427-445`.
- **Coverage failure semantics:** coverage remains keyed by span ID, and any uncovered candidate now fails the document at `crates/bid/src/extraction/mod.rs:259-305`.
- **Crash/replay idempotency:** database transactions prevent partial document replacement, stable section keys preserve section identity, run tokens fence reclaimed owners, and queue jobs use `bid:extract:{run_id}` uniqueness with conflict skipping at `crates/runtime/src/jobs.rs:180-187`. A crash after document commit may cause a later replay and superseded historical drafts, but only the current lease can write.
- **Evaluation precision:** one-to-one assignment, false positives, precision/F1, per-family precision/recall, and threshold enforcement are now implemented at `crates/bid/src/extraction/evaluation.rs:88-259`.

## Overall decision

**A blocker remains, and fixes worth doing now remain.**

- Blockers: **1**
- Additional fixes worth doing now: **3**
- Product/API approval decisions: **none**

The project should not be considered Round-2 correctness-complete until heartbeat-based reclaim, original-document provenance, numbered requirement preservation, and fully fenced section-retry cleanup are corrected and exercised with PostgreSQL integration tests.

## Validation limitations and residual risks

- Git working-tree, staged, and untracked state could not be established without a Git command tool.
- Formatting, clippy, Cargo tests, web build, and PostgreSQL integration tests were not executable in this runtime.
- Existing database tests can silently skip when PostgreSQL is unavailable.
- Dense requirements separated only by commas or other unsupported punctuation can still share a coverage unit.
- Repeated crash/replay can accumulate superseded draft history, though visible current drafts remain lease-protected.