# Bid Extractor Hardening — Continuation Round-1 Traceability Review

## Review

- **Correct:** The worktree contains the intended deep extraction module, async tool-chat seam, embedded policy/prompt, stable sections/spans, transactional persistence, UI conflict/fallback notices, fixtures, evaluator, and updated documentation.
- **Fixed:** None; this was read-only.
- **Blocker:** No release-blocking storage or data-loss defect was verified.
- **Verdict:** **Not faithful and complete.** Most infrastructure is present, but three approved behavioral requirements remain materially inconsistent: family arbitration order, ambiguous `must` handling, and policy centralization. Diagnostics and evaluation also have important gaps.

## Material findings

### High — Cross-family arbitration runs in the wrong order and omits extractor confidence

**Evidence**

- Approved plan: “`heading hint → policy family score → extractor confidence` 依次裁决” (`plans/bid-extractor-hardening.md:87`).
- `choose_family` scores quote signals first and returns immediately, consulting the heading only on a score tie (`crates/bid/src/extraction/reconcile.rs:140-155`).
- `CandidateClause` has an extractor string but no confidence/rank input (`crates/bid/src/extraction/types.rs:114-120`).
- The test explicitly codifies body-signal-over-heading behavior (`crates/bid/src/extraction/reconcile.rs:198-220`), contrary to the approved ordering.

**Impact**

A quote emitted by both agents can receive a different family than the plan specifies. Strong body signals override the heading prior rather than acting as the second discriminator, and extractor confidence is never considered. The resulting `family_conflict` flag can also be cleared when the plan’s ordered arbitration would have reached a different result.

**Narrow fix**

Implement arbitration as:

1. compare heading prior;
2. if unresolved, compare policy signal score;
3. if unresolved, compare a deterministic extractor confidence/rank;
4. only then choose technical with `family_conflict=true`.

Add tests for contradictory heading/body signals and extractor-confidence ties.

---

### High — Ambiguous `must` values can remain true, and every otherwise-ambiguous table row is forced true

**Evidence**

- Approved verification: “歧义默认 false” (`plans/bid-extractor-hardening.md:227`).
- `resolve_must` returns the model/heuristic proposal when no hard, exclusion, or optional term matches (`crates/bid/src/extraction/policy.rs:221-250`).
- Reconciliation treats every table-containing span as a positive must proposal (`crates/bid/src/extraction/reconcile.rs:102-103`).
- Heuristic extraction does the same (`crates/bid/src/extraction/coverage.rs:57`).

**Impact**

An ambiguous agent proposal can become `must=true`, and a key/value table row such as a neutral parameter row becomes mandatory solely because it contains `|`. This violates the approved rule and can alter downstream unmet-must decisions and bid responses.

**Narrow fix**

Make the no-rule/ambiguous branch resolve to `false`. Do not use table syntax itself as mandatory evidence. Preserve true only when the versioned hard/veto/lower-upper-bound rules establish it. Add explicit ambiguous prose and ambiguous table-row tests.

---

### High — Policy is not the unique mutable strategy source

**Evidence**

- Approved Step 11: “确保 Policy 是唯一可变策略来源” (`plans/bid-extractor-hardening.md:202`).
- `ExtractionPolicy` only models families, skip headings, must, coverage, and limits; it has no process-heading, sentence-anchor, table-predicate, or numbered-requirement strategy fields (`crates/bid/src/extraction/policy.rs:11-18`).
- Sentence-splitting terms remain hardcoded in `ANCHORS` (`crates/bid/src/extraction/outline.rs:300-327`).
- Table self-containment verbs remain hardcoded (`crates/bid/src/extraction/outline.rs:234-256`).
- Heading/requirement suffixes and predicate phrases remain hardcoded (`crates/bid/src/extraction/outline.rs:455-489`).
- Updated documentation nevertheless claims title, classification, must terms, and limits are all in the policy (`docs/bid-platform-domain.md:217`).

**Impact**

Changing `cn-tender-v2` does not fully version extraction behavior. Prompt/policy regression results cannot reliably identify the deployed strategy, and the documentation is inaccurate.

**Narrow fix**

Move mutable anchor, table-predicate, numbered-heading/requirement, and process-heading term sets into typed policy fields. Retain only true domain invariants in Rust. Add policy validation for nonempty required sets and tests proving policy changes affect these paths.

---

### Medium — Quote-rejection diagnostics lose span-sweep rejection counts

**Evidence**

- The plan requires diagnostics to retain quote-validation rejection counts (`plans/bid-extractor-hardening.md:126`).
- The engine snapshots `tool_rejected_quotes` before span sweeps (`crates/bid/src/extraction/mod.rs:180`).
- Sweep `merge_stats` can increment `diagnostics.rejected_invalid_quotes`, but later assignments reset the counter to the pre-sweep snapshot plus reconciliation counts (`crates/bid/src/extraction/mod.rs:234-252`).

**Impact**

Invalid quotes rejected during hybrid span sweeps are absent from persisted diagnostics, understating model validation failures.

**Narrow fix**

Keep separate cumulative tool-rejection and reconciliation-rejection counters, or increment rather than resetting. Add a scripted hybrid test where a sweep emits an invalid quote and assert the final count.

---

### Medium — `covered_spans` can count non-candidate spans

**Evidence**

- Coverage is required to report candidate/covered/uncovered requirement-like spans (`plans/bid-extractor-hardening.md:126`, `:228`).
- The engine builds `covered` from every reconciled clause span, without intersecting it with `candidate_spans` (`crates/bid/src/extraction/mod.rs:254-264`).
- Agents receive all spans, not only candidate spans (`crates/bid/src/extraction/agent.rs:309-346`, `:580-603`).

**Impact**

If an agent emits a valid clause from a non-candidate span, `covered_spans` can exceed `candidate_spans`, yielding misleading run/API/UI diagnostics.

**Narrow fix**

Calculate `covered_spans` as candidate span IDs intersected with emitted-clause span IDs. Keep a separate optional count for clauses emitted outside the candidate set.

---

### Medium — Golden/scripted regression coverage is incomplete, and the evaluator cannot report failed extraction diagnostics

**Evidence**

- Approved fixture cases include “无标题长文、长表格” (`plans/bid-extractor-hardening.md:138`).
- Both golden Markdown files begin with headings; neither contains unheaded long prose (`testdata/bid-extraction/cn-tender-golden-01.md:1-13`, `cn-tender-golden-02.md:1-35`).
- The only golden table has two rows, not a long/cross-page table (`testdata/bid-extraction/cn-tender-golden-02.md:17-20`).
- Step 14 requires a “scripted 流程测试” (`plans/bid-extractor-hardening.md:208`), but the golden test runs `ExtractionMode::Heuristic` with an empty `ScriptedToolChat` (`crates/bid/src/extraction/mod.rs:520-540`); it does not exercise the scripted agent workflow.
- The evaluator immediately converts `ExtractionFailure` into an error before writing JSON/Markdown (`crates/bid/src/bin/bid_extract_eval.rs:17-22`), so uncovered-span diagnostics are not emitted even though the plan calls for evaluator output including uncovered spans (`plans/bid-extractor-hardening.md:142`).

**Impact**

Regression claims do not cover all approved golden cases or the complete agent workflow. A real-model miss can terminate without producing the promised diagnostic report.

**Narrow fix**

Add fixtures for unheaded long prose and long/cross-page tables. Run at least one full golden set in Agent mode with scripted tool replies. Make the evaluator serialize failure diagnostics and a failed quality gate before returning nonzero.

---

### Medium — Startup configuration/version logging is absent

**Evidence**

- Approved requirement: “启动和每次 run 记录 mode/model/policy/prompt 版本” (`plans/bid-extractor-hardening.md:30`).
- The worker startup only logs `worker ready` (`crates/worker/src/main.rs:11-22`).
- Successful per-document runs log mode/model/policy but omit prompt version (`crates/bid/src/lib.rs:773-787`).
- Persisted run rows do contain all four values (`crates/bid/src/lib.rs:827-832`).

**Impact**

Persisted diagnostics are adequate after a run, but deployment startup cannot attest the effective extraction contract, and operational run logs do not include the prompt version.

**Narrow fix**

Construct/validate extraction configuration at worker startup and log mode, model, policy, and prompt versions. Include prompt version in per-run logging.

---

### Medium — Tool `text` contract was intentionally changed without approval

**Evidence**

- Approved plan calls `span_id + quote + text + must` “模型真正负责的字段” (`plans/bid-extractor-hardening.md:94-97`).
- The server validates that submitted `text` is nonempty but discards it, storing `item.quote` as candidate text (`crates/bid/src/extraction/agent.rs:422-449`).
- Reconciliation again writes the quote as clause text (`crates/bid/src/extraction/reconcile.rs:115-118`).
- Documentation was changed to codify this new behavior (`docs/bid-platform-domain.md:292-295`).

**Impact**

Model-provided normalization can never reach the draft even though the approved tool contract assigns that field to the model. This is an unauthorized contract change, not merely missing implementation.

**Narrow fix**

Preserve submitted `text` after validating it does not introduce unsupported conditions/numbers, or obtain explicit plan approval before changing this contract. Update tests and docs to match the approved behavior.

---

### Note — `latest_extract` is not the approved compact projection

The plan requests a compact `latest_extract` containing status/mode/coverage/fallback/error (`plans/bid-extractor-hardening.md:128`). The API returns IDs, model/policy/prompt, timestamps, section counts, and the entire diagnostics JSON (`crates/api/src/routes.rs:3711-3725`). No raw spans are presently stored there, so this is not a direct data leak, but it is API scope drift. Return a dedicated compact view rather than the storage row projection.

## Completion matrix — Steps 1–15

| Step | Status | Traceability |
|---:|---|---|
| 1 | **Partial** | Mode/model env and Compose forwarding exist (`deploy/docker-compose.yml:28-30`); strict/hybrid/heuristic behavior is tested. Startup version logging and direct worker-env integration coverage are absent. |
| 2 | **Implemented** | Typed input/report/diagnostics and engine exist (`extraction/types.rs`, `extraction/mod.rs`); policy/prompt use `include_str!` and `OnceLock` (`policy.rs:7-8`, `:91-104`). |
| 3 | **Implemented** | Async `ToolChat`, OpenAI adapter, and scripted test adapter exist (`agent.rs:35-97`, `:714-763`); no nested runtime exists in bid extraction. |
| 4 | **Implemented** | ATX, chapter/section, decimal, Chinese and bold headings plus stable IDs and bounded spans exist (`outline.rs:1-577`). Cross-page behavior has unit-level mechanisms but lacks the required golden case. |
| 5 | **Partial** | Five scoped tools, strict schemas and injection-resistant prompt exist (`agent.rs:307-570`, prompt file). Model `text` is discarded and oversized emit batches are silently truncated at `agent.rs:428`. |
| 6 | **Partial** | Agents are independent and quote validation/dedup exist, but arbitration order and extractor confidence violate the plan. |
| 7 | **Partial** | Per-span candidate coverage, sweep, and heuristic fallback exist. Coverage count can include non-candidates, and rejection diagnostics are lost after sweeps. |
| 8 | **Implemented** | Migration has stable section key, conflict/meta, run diagnostics (`migrations/0007_bid.sql:31-105`); transactional report persistence writes `source_span` (`storage/src/bid.rs:745-862`). |
| 9 | **Implemented** | Extraction completes in memory before per-document transaction; failures retain drafts; section retry uses the same engine (`bid/src/lib.rs:838-930`, `:1200-1400`). |
| 10 | **Implemented** | `ClauseView`, API persistence, conflict clearing, and UI family editing are present (`bid/src/lib.rs:20-42`; `storage/src/bid.rs:950-970`; `web/src/bid/Inspector.tsx:72-85`). |
| 11 | **Not complete** | Multiple mutable extraction term sets remain hardcoded in `outline.rs`; policy is not the unique strategy source. |
| 12 | **Partial** | Typed finish/latest storage and run columns exist, but span-sweep rejection diagnostics are undercounted. |
| 13 | **Partial** | API and UI fallback/failure notices exist (`Workbench.tsx:198-208`, `:397-405`), but API projection is not compact. |
| 14 | **Partial** | Fixtures, metrics and manual evaluator exist, but approved fixture cases and full scripted golden flow are missing; evaluator does not write failure diagnostics. |
| 15 | **Partial** | Env and principal docs were updated, and system design mirrors the scratch spec structurally. Documentation now codifies the unapproved `text=quote` behavior and falsely claims all relevant terms are policy-based. |

## Accepted decisions traceability

| Accepted decision | Status |
|---|---|
| No `unknown`; cross-family quote gets suggested family and conflict handling | **Partial:** enum and conflict metadata exist, but arbitration order/confidence are inconsistent. |
| Default `hybrid`, strict `agent`, visible fallbacks | **Implemented:** `ExtractionMode`, runtime contract, diagnostics, and UI notice are present. |
| Two delivery batches, plan covers both | **Partial:** Phase 2 evaluation/observability gaps remain. |
| Only `cn-tender-v2`; no industry admin/profile UI | **Implemented:** one embedded policy and no industry configuration UI were found. |

## Hard invariants

| Invariant | Result | Evidence |
|---|---|---|
| Family only technical/commercial | **Satisfied** | Rust enum (`types.rs:35-57`) and DB check (`migrations/0007_bid.sql:87`). |
| Quote must return to source | **Satisfied** | Exact span check and table-row validation (`agent.rs:432-438`; `reconcile.rs:42-70`). |
| Automatic extraction writes draft only | **Satisfied** | Transaction insert hardcodes `'draft'` (`storage/src/bid.rs:819-838`). |
| Human confirmation gates matching | **Satisfied** | Matching queries only `status='confirmed'` (`storage/src/bid.rs:973-986`). |
| Extraction has no KB/company/product tool | **Satisfied** | Agent tool set is only outline/span/grep/emit/done (`agent.rs:501-570`). |
| Matching scope/semantics remain outside engine | **Satisfied** | Engine returns an in-memory report; product/company search occurs only later in `run_match_job` (`bid/src/lib.rs:932+`). |
| Full chat/raw spans not persisted in diagnostics | **Satisfied** | Diagnostics contain counters, categories and span IDs, not messages or bodies (`types.rs:142-157`). |

## Verification traceability

### Automatic checks

Not executed: this review runtime exposed repository read/search tools but no shell or Git command runner. Therefore the following require supervisor execution:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p bid -p storage -p worker -p api`
- `npm -C web run build`
- `cmp docs/system-design.md .scratch/knowledgebrain/spec.md`

### Scenario coverage

- **Configuration:** unit coverage exists for strict mode, hybrid fallback, and heuristic no-chat. Startup logging and a direct Compose-to-worker test are missing.
- **Outline/Span:** strong unit coverage exists for heading formats, hierarchy, long spans, tables, parallel clauses and enumerations.
- **Tools:** missing/extra fields and invalid quotes are tested. Oversized batches and whole-file cap behavior are not directly tested.
- **Classification:** opposite-heading body-signal behavior, basic tie conflict and confirmation UI exist, but the implemented ordering contradicts the plan.
- **Must:** hard/exclusion/optional examples exist; ambiguous false is neither implemented nor tested.
- **Coverage:** sibling spans and coordinated clauses are tested; cross-page/long-table scripted sweep is not.
- **Security:** the prompt and golden injection text are present; tool scope mechanically exposes no KB operation.
- **Re-extraction:** storage tests cover lease fencing and transaction rollback (`crates/storage/src/persist.rs:3780-3979`, `:4130-4280`).
- **Golden gates:** evaluator computes quote validity, precision/recall, family/must accuracy, duplicates and false positives. Fixture breadth and scripted-flow coverage remain incomplete.
- **E2E:** no automated hybrid/strict upload-to-confirm-to-reextract E2E evidence was found.

## Scope and architecture drift

1. Model `text` is discarded and documentation was changed to legitimize it, contrary to the approved tool contract.
2. `latest_extract` exposes a broad storage-shaped response rather than the approved compact projection.
3. Mutable extraction strategy remains split between policy JSON and Rust constants despite documentation claiming policy centralization.

No old-volume migration compatibility is requested; the approved wipe/rebuild policy is respected.

## Residual risks

- Tests and builds were not executable in this review environment.
- Git status/diff/staged/untracked inventory could not be independently obtained because no Git command tool was available; inspection was against the current files exposed in `/opt/workspace/code/KnowledgeBrain`.
- Real-model behavior and the stated quality thresholds remain unverified.
- Ambiguous `must` and reversed arbitration can affect production draft semantics even if compilation and existing tests pass.