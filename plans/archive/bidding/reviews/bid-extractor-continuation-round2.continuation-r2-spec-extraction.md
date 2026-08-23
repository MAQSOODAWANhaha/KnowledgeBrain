# Bid Extractor Hardening — Continuation Round-2 Review

## Review

### Correct

- **Server-canonical text is implemented and documented.** Agent output keeps `text` required but replaces it with the verified quote; reconciliation again persists `text=quote`: `crates/bid/src/extraction/agent.rs:422-457`, `crates/bid/src/extraction/reconcile.rs:99-125`, `docs/bid-platform-domain.md:292-297`. Tests explicitly assert this contract at `crates/bid/src/extraction/agent.rs:793-825` and `crates/bid/src/extraction/reconcile.rs:251-277`.
- **Family arbitration now follows the approved server-owned order.** Heading prior is evaluated first, then policy body score, then server-owned extractor rank, with only a final tie producing technical plus conflict: `crates/bid/src/extraction/reconcile.rs:140-181`. Contradictory heading/body and rank tests exist at `crates/bid/src/extraction/reconcile.rs:227-277`.
- **Ambiguous must defaults false and table syntax alone no longer makes must true.** `resolve_must` ignores the proposal and returns true only for policy hard evidence after exclusions: `crates/bid/src/extraction/policy.rs:221-250`. Ambiguous prose and neutral table assertions are present at `crates/bid/src/extraction/policy.rs:264-273` and `crates/bid/src/extraction/reconcile.rs:316-346`.
- **The previously identified mutable outline term sets moved into typed policy.** Sentence anchors, enumeration terms, table predicates, heading suffixes, and numbered-requirement predicates are typed and validated: `crates/bid/src/extraction/policy.rs:10-48,87-114`, `crates/bid/config/cn-tender-v2.json:29-36`. A policy-mutation test exists at `crates/bid/src/extraction/outline.rs:694-704`.
- **Diagnostics and coverage arithmetic were corrected.** Agent and sweep rejection counts are accumulated through `merge_stats`, final reconciliation adds rather than overwrites, and covered spans are intersected with candidate IDs: `crates/bid/src/extraction/mod.rs:245-267,331-340`.
- **Compact latest_extract and startup/run version logging are present.** API projection contains only status, mode, compact coverage/fallback diagnostics, and error: `crates/api/src/routes.rs:3710-3753`. Worker startup logs mode/model/policy/prompt, and per-document logs include prompt version: `crates/worker/src/main.rs:16-25`, `crates/bid/src/lib.rs:861-876`.
- **No legacy upgrade shim was found.** The current schema directly defines the required extraction columns and constraints in `migrations/0007_bid.sql:31-125`.

### Blocker

- None found.

### High

1. **Table syntax still makes every non-skip table row a candidate span, violating requirement-like coverage and allowing neutral tables to fail the entire extraction.**
   - **Evidence:** `is_candidate_span` returns true solely when `body.contains('|')`: `crates/bid/src/extraction/outline.rs:119-139`. The heuristic subsequently requires policy family/trigger signals and emits nothing for a neutral row: `crates/bid/src/extraction/coverage.rs:89-117`. Fixed table-chrome labels also remain hardcoded outside Policy at `crates/bid/src/extraction/coverage.rs:119-130`.
   - **Failure path:** A table such as `| 序号 | 名称 | ... | 1 | 路由器 |` creates a candidate data-row span because it contains `|`. It has no configured family or coverage signal, so heuristic extraction emits no clause. The span remains uncovered and the engine returns `candidate_spans_uncovered`, discarding the document report: `crates/bid/src/extraction/mod.rs:236-296`.
   - **Narrow fix:** Do not use table syntax alone as candidate evidence. Put configurable table-header/chrome terms and any table candidate predicates in Policy, and require heading/body/policy evidence for a data row. Add heuristic and hybrid tests proving a neutral inventory table is skipped while actual key/value requirements remain candidates.

2. **A multi-document run can be marked done despite failed documents, and the compact UI hides that partial failure.**
   - **Evidence:** Any run with at least one successful document is assigned `status="done"` even when `failed_documents > 0`: `crates/bid/src/lib.rs:884-908`. The compact API drops `failed_documents`, although it retains `error_message`: `crates/api/src/routes.rs:3713-3752`. The UI reads `error_message` only when the whole run status is `failed`; a done run with no uncovered/fallback entries produces no notice: `web/src/bid/Workbench.tsx:198-205`.
   - **Failure path:** Document A succeeds; document B fails early from a strict-agent provider error and has no completed coverage counters. The run is persisted as done with a nonempty error. API returns done, zero uncovered spans, and no fallback; UI displays no warning, leaving the user unaware that B produced no drafts.
   - **Narrow fix:** Include a compact `failed_documents` count or `partial_failure` boolean and display it whenever nonzero. Alternatively display any nonempty extraction `error_message` even for done runs. Add an API/UI projection test for one-success/one-failure diagnostics.

### Medium

3. **“Exact oversized table handling” is incomplete beyond the tool-output limit, and the current test misses that boundary.**
   - **Evidence:** Dependent key/value rows are intentionally retained as one span with no upper bound: `crates/bid/src/extraction/outline.rs:155-176`. Every family-agent tool result, including `read_span`, is truncated to `max_tool_output_chars`: `crates/bid/src/extraction/agent.rs:170-174,295-303,370-380`. Policy permits 8,000-character ordinary spans but caps tool output at 16,000: `crates/bid/config/cn-tender-v2.json:44-48`. The oversized-row test uses only `max_span_chars + 100`, so it remains below the tool-output cap: `crates/bid/src/extraction/outline.rs:707-714`.
   - **Failure path:** A key/value row longer than roughly 16,000 characters is retained intact, but strict Agent mode can retrieve only a truncated JSON/tool response. A partial quote may pass initial substring validation but final table reconciliation requires the entire row; it is rejected, leaving the span uncovered.
   - **Narrow fix:** Define an explicit bounded oversized-row contract. Ensure every accepted exact-row span fits an untruncated `read_span` response, or fail it deterministically with a precise diagnostic. Add a test above `max_tool_output_chars` that exercises the complete Agent read/emit/reconcile path.

4. **Golden/scripted evaluation and failure artifacts remain incomplete.**
   - **Evidence:** Extraction failure is converted to an error before payload rendering or output writing, so requested JSON/Markdown artifacts are never produced: `crates/bid/src/bin/bid_extract_eval.rs:17-42`. Both goldens begin with headings and the only golden table has two data rows: `testdata/bid-extraction/cn-tender-golden-01.md:1-13`, `cn-tender-golden-02.md:1-35`. The golden test still runs Heuristic mode with an empty `ScriptedToolChat`, rather than a scripted agent flow: `crates/bid/src/extraction/mod.rs:518-545`. No scripted cumulative span-sweep quote-rejection assertion was found.
   - **Failure path:** A real-model evaluation fails with uncovered spans; the command exits before writing the diagnostics artifact needed to locate them. Separately, scripted tool workflow, unheaded long prose, long/cross-page tables, and the prior cumulative-diagnostics regression are not protected by the golden gate.
   - **Narrow fix:** Serialize `ExtractionFailure.diagnostics` with `quality_gate="FAIL"` and write the requested artifact before returning nonzero. Add the missing long/unheaded fixture and at least one full Agent-mode scripted golden, including an invalid sweep quote whose cumulative rejection count is asserted.

5. **Documentation still omits the approved extractor-rank stage.**
   - **Evidence:** Documentation says heading prior, then policy signals, then directly technical/conflict on a tie: `docs/bid-platform-domain.md:239-242`. Implementation includes extractor rank between score and final conflict: `crates/bid/src/extraction/reconcile.rs:140-181`.
   - **Failure path:** Operators and future maintainers following the documented contract can change or test arbitration under the false assumption that equal policy scores always create a conflict.
   - **Narrow fix:** Amend the domain document to state `heading prior → policy family score → server-owned extractor rank → technical/conflict on final tie`, and mirror it anywhere the system specification summarizes arbitration.

## Accepted-decision status

| Approved decision | Status |
|---|---|
| Server-canonical `text=verified quote`; amend plan/docs | **Complete** |
| Heading → Policy score → server-owned extractor rank | **Implementation complete; docs partial** |
| Ambiguous must=false; no table-syntax must | **Complete for must** |
| Policy as unique mutable extraction strategy | **Partial:** table candidate/chrome strategy remains hardcoded |
| Correct cumulative diagnostics/coverage | **Implementation complete; targeted sweep regression test absent** |
| Better goldens/scripted evaluator failure artifacts | **Not complete** |
| Compact latest_extract | **Complete projection; partial-document UI visibility incomplete** |
| Startup version logs | **Complete** |
| Exact oversized table handling | **Partial beyond tool-output limit** |

## Steps 1–15 completion matrix

| Step | Current status | Evidence |
|---:|---|---|
| 1 | **Complete** | Modes/model fallback, Compose forwarding, startup validation/logging present. |
| 2 | **Complete** | Typed engine/report/policy and embedded versioned policy/prompt present. |
| 3 | **Complete** | Async OpenAI and scripted seams; no nested runtime found. |
| 4 | **Partial** | Outline and stable spans are implemented, but accepted oversized rows can exceed readable tool output. |
| 5 | **Partial** | Strict tools and prompt are correct; `read_span` is not complete for accepted rows beyond the tool-output cap. |
| 6 | **Complete** | Independent agents, quote validation, ordered arbitration, rank, dedup, and conflicts implemented. |
| 7 | **Partial** | Span coverage and fallback exist, but table syntax alone creates false candidate spans. |
| 8 | **Complete** | Stable sections, metadata, diagnostics schema, and transactional persistence present. |
| 9 | **Complete** | In-memory extraction before per-document transaction and shared section retry engine present. |
| 10 | **Complete** | Conflict API/UI handling and confirmation clearing present. |
| 11 | **Partial** | Most term sets moved to Policy; table candidate/chrome behavior remains hardcoded. |
| 12 | **Complete** | Typed finish/latest persistence and cumulative diagnostics implementation present. |
| 13 | **Partial** | Compact API exists, but partial document failure can remain invisible in UI. |
| 14 | **Partial** | Evaluator metrics exist; failure artifact and required fixture/scripted breadth remain missing. |
| 15 | **Partial** | Principal docs/env are updated, but arbitration documentation omits extractor rank. |

## Current completion verdict

**Not complete: 8 of 15 steps are fully complete; 7 remain partial.**

There is no release blocker in the reviewed extraction invariants, but an **immediate focused fix worker is warranted**. It should prioritize neutral-table candidate classification and partial-document failure visibility, then close evaluator artifacts, oversized-row/tool bounds, and the arbitration documentation mismatch.

## Validation and residual risks

- This review was read-only; no files were edited.
- The available runtime exposed file read/search tools but no shell or Git command runner. Exact Git diff, staged state, and exhaustive untracked inventory could not be attested.
- No cargo, npm, smoke, or database tests were executed. The supervisor should run:
  - `git status --short --untracked-files=all`
  - `git diff --check`
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
  - `npm -C web run build`
  - `scripts/bid_e2e_smoke.sh` with PostgreSQL and Redis supplied.
