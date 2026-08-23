# Bid Extractor Round 3 — Extraction Quality Review

## Review

### Blocker [HIGH] — Cell-level table splitting destroys key/value requirements and can make headers quotable

**Evidence**

- Each table row is split into independent cells, and each cell becomes its own quotable span: `crates/bid/src/extraction/outline.rs:176-189`.
- This loses requirements whose meaning is distributed across cells. For example, `| 最大响应时间 | 2秒 |` becomes `最大响应时间` and `2秒`. The first is a candidate due to the `响应时间` signal, while the value is too short to be a candidate (`crates/bid/src/extraction/outline.rs:121-143`).
- Hybrid heuristics can consequently emit the incomplete quote `最大响应时间` and mark the candidate span covered (`crates/bid/src/extraction/coverage.rs:21-28,31-61,86-109`). Neither the agent nor reconciliation can emit the complete row because quotes must come from one cell’s `span.body`.
- When a table has a header and separator but no data rows, the fallback explicitly turns the header into quotable spans: `crates/bid/src/extraction/outline.rs:192-195`. This contradicts the non-quotable-header invariant and can create uncovered header candidates or false clauses.
- Existing tests use self-contained requirement cells and therefore miss this regression: `crates/bid/src/extraction/outline.rs:493-511`; `testdata/bid-extraction/cn-tender-golden-02.md:14-17`.

**Impact**

Common parameter tables can silently lose numeric values or produce label-only clauses. Empty table templates can also fail extraction or permit header extraction. This is a production extraction-quality blocker.

**Required fix**

Retain the exact original row as the quotable source unit for key/value rows, with only table headers as non-quotable context. If cell-level units are retained for self-contained requirement cells, detect that case explicitly; do not split rows whose constraint depends on sibling cells. Remove the header-only quotable fallback. Add tests for:

- `| 最大响应时间 | 2秒 |`;
- `| 端口数量 | 不少于24个 |`;
- self-contained requirement cells;
- a header-only table;
- exact row-source quote validation.

---

### High — Numbered requirement preservation now misclassifies real signal-bearing headings as body text

**Evidence**

- A parsed heading is accepted only when `numbered_requirement_line` returns false: `crates/bid/src/extraction/outline.rs:32-36`.
- Any numbered line of at least eight characters containing a coverage or family signal is classified as a requirement: `crates/bid/src/extraction/outline.rs:321-353`.
- Thus real headings such as `1. 投标人资格要求`, `1. 设备接口要求`, or `一、类似项目业绩要求` are not installed in the outline because they contain configured family signals.
- Heading parsing otherwise supports these numeric and Chinese forms: `crates/bid/src/extraction/outline.rs:359-408`.
- The regression test covers neutral headings such as `1. 总则` and `1.2 技术要求`, but not longer headings containing family signals: `crates/bid/src/extraction/outline.rs:462-490`.

**Impact**

The numbered requirement variants are preserved, but real headings can be destroyed. Their descendants receive the wrong heading path and family prior, and the title itself may be treated as a clause candidate.

**Fix**

Separate structural heading recognition from preservation of ambiguous source lines. Use clause grammar—modal/prohibition predicates, limits, or complete requirement constructions—rather than family nouns alone to decide that a numbered line is a requirement. Where ambiguity remains, retain the source line without preventing it from establishing outline structure. Add heading and body cases for every supported delimiter.

---

### High — Coordinated requirement splitting remains limited to twelve exact modal conjunctions

**Evidence**

- `sentence_slices` splits only on twelve literal anchors such as `并应`, `且须`, and `并不得`: `crates/bid/src/extraction/outline.rs:233-289`.
- The focused test exercises only this recognized pattern: `crates/bid/src/extraction/outline.rs:514-524`.
- Common forms such as `设备必须支持万兆接口，并提供双电源热插拔`, `须提供营业执照、ISO认证和业绩证明`, or `同时提供原件和电子版` remain one span.
- Coverage is still binary by `span_id`; one emitted clause covers the entire span: `crates/bid/src/extraction/coverage.rs:17-28`.
- Reconciliation groups containment-overlapping candidates and chooses the longest quote, which can preserve a broad combined clause instead of atomic requirements: `crates/bid/src/extraction/reconcile.rs:65-84`.

**Impact**

An agent can emit one coordinated item and mask omitted siblings. Hybrid heuristics may instead emit the entire combined sentence as one non-atomic clause. The strict uncovered-span check cannot detect either case because coverage operates on the unsplit span.

**Fix**

Model requirement anchors independently of exact conjunction spelling, including inherited modals across comma/`、`/`和`/`及`/`并` lists. Track coverage per derived requirement unit rather than only per coarse sentence. Add adversarial tests where only one item is emitted from two- and three-item coordinated requirements.

## Correct / verified

- **Standard table context and exact quotes:** Headers/current-row context are exposed as `non_quotable_context`, while cells are exposed as `quotable_text` (`crates/bid/src/extraction/agent.rs:372-377`). Tool dispatch and reconciliation both require literal containment in the quotable body (`crates/bid/src/extraction/agent.rs:436`; `crates/bid/src/extraction/reconcile.rs:57`), and persisted text is replaced with that exact quote (`crates/bid/src/extraction/reconcile.rs:111-112`). The prompt states the same contract at `crates/bid/prompts/clause-extractor-v2.md:20`. The header-only exception above remains.
- **Strict/hybrid failure preservation:** After all agent, sweep, and heuristic stages, any uncovered candidate causes `ExtractionFailure("candidate_spans_uncovered")`: `crates/bid/src/extraction/mod.rs:296-301`. Strict behavior is tested at `crates/bid/src/extraction/mod.rs:393-416`. This protection does not catch requirements masked within one coarse span.
- **Evaluator matching:** Expected clauses connect only to exact normalized quotes or explicit aliases (`crates/bid/src/extraction/evaluation.rs:118-134,291-301`), followed by one-to-one augmenting-path assignment (`crates/bid/src/extraction/evaluation.rs:305-324`). Short fragments therefore fail, as tested at `crates/bid/src/extraction/mod.rs:560-565`; broad quotes also cannot match unless explicitly labeled as aliases.
- **No-label evaluation:** Missing expected labels produce `quality_gate: "NOT_EVALUATED"`, not PASS: `crates/bid/src/bin/bid_extract_eval.rs:28-38`.
- **Must negation/prohibition:** Hard exclusions are removed before hard-term detection, while genuine hard prohibitions take precedence over optional substrings (`crates/bid/src/extraction/policy.rs:221-251`). Tests cover `不要求必须提供原件 → false` and `设备不可以支持弱算法 → true`: `crates/bid/src/extraction/policy.rs:269-270`.
- **Fixture documentation:** The README accurately documents exact/alias matching and `NOT_EVALUATED`: `testdata/bid-extraction/README.md:24-32`.

## Optional / deferred

- Add an explicit broad-quote evaluator adversarial test; the exact matcher already rejects it, but only the short-fragment case is currently exercised.
- Add a direct hybrid-mode test proving an unrecoverable candidate returns failure. The unconditional final check establishes the behavior in code, but focused regression coverage is absent.
- Extend golden fixtures with key/value tables, signal-bearing numbered headings, coordinated lists, header-only tables, and must negation/prohibition cases.

## Decision

**Production/quality blockers remain: yes.**

The evaluator, no-label state, exact-quote validation, and must arbitration fixes are coherent. However, cell-level table splitting is a production-quality blocker, while heading discrimination and coordinated requirement coverage remain high-severity recall/atomicity defects. Round 3 should not be approved for production extraction-quality rollout until these are corrected.

## Validation limitations

This was a read-only review. No files were edited, and no shell, Git, or test runner was available. The supervisor should run:

- `cargo test -p bid extraction::`
- `BID_EXTRACT_MODE=heuristic cargo test -p bid golden_fixture`
- `cargo fmt --all -- --check`
- `cargo clippy -p bid --all-targets -- -D warnings`
