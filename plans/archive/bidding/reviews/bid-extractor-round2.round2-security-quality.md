# Bid Extractor Round 2 — Security and Quality Review

## Review decision

**Not approved for production or quality-gate rollout. Three high-severity quality blockers remain.**

No confidentiality, external-tool, or arbitrary-execution security blocker was found. A medium availability/cost risk remains because resource caps are applied per agent invocation rather than across the extraction run.

## Blockers / decisions

### Blocker [HIGH] — Some common numbered requirements are still discarded as headings

`build_sections` removes lines classified as headings from span bodies (`crates/bid/src/extraction/outline.rs:32-45`). The attempted safeguard recognizes every numbered style, but applies general coverage triggers only to `)`/`）` list styles; numeric-dot and Chinese `、` lines are preserved only when they contain a configured hard-must phrase (`crates/bid/src/extraction/outline.rs:262-289`).

Consequently, lines such as:

- `1. 设备应支持万兆接口`
- `一、设备应支持万兆接口`
- `一、设备支持万兆接口`

are parsed as headings by `parse_heading` (`crates/bid/src/extraction/outline.rs:311-323`) but fail `numbered_requirement_line`, so their requirement text never reaches an extractable span.

The existing test covers only `1）设备支持万兆接口` and a hard-token example (`crates/bid/src/extraction/outline.rs:403-424`), which do not exercise this failure.

**Fix:** apply requirement/body-signal detection consistently to every numbered syntax, not only parenthesized list styles. Preserve ambiguous numbered lines as standalone extractable spans rather than deleting their text. Add engine and golden tests for `1.`, `一、`, `（一）`, and `1）`, with `应`, family signals, and no terminal punctuation.

---

### Blocker [HIGH] — Coverage is sentence/row-sized, not reliably requirement-sized

Table handling is substantially improved: each data row becomes a separate span (`crates/bid/src/extraction/outline.rs:149-179`). Prose is also split at `。`, `；`, semicolon, and newline (`crates/bid/src/extraction/outline.rs:207-233`).

However, comma- or conjunction-separated requirements remain in one span. Coverage still marks that entire span covered when any clause references its ID (`crates/bid/src/extraction/coverage.rs:17-28`). For example:

> `设备必须支持万兆接口，并应提供双电源热插拔。`

If an agent emits only the interface requirement, the span is considered covered and neither sweep nor heuristic recovery checks the omitted power requirement. The same masking can occur when a table row contains multiple independent requirements.

Reconciliation compounds the atomicity problem by grouping containment-overlapping candidates and selecting the longest quote (`crates/bid/src/extraction/reconcile.rs:65-84`), potentially preferring a broad multi-requirement quote over an atomic candidate.

**Fix:** detect requirement anchors within each sentence/row and track coverage per anchor, or split independent coordinated constraints into separate units. Validate that emitted clauses are atomic, and do not mark a unit complete until all detected anchors are covered or explicitly classified as non-requirements. Add a scripted test where the model emits only one of two comma/conjunction-separated requirements.

---

### Blocker [HIGH] — The quality gate remains gameable using short source fragments

One-to-one maximum matching is now present, which fixes the Round-1 reuse problem. But the matching edge remains any normalized containment overlap (`crates/bid/src/extraction/reconcile.rs:28-31`; `crates/bid/src/extraction/evaluation.rs:118-133`).

Therefore, an exact-source but semantically incomplete fragment such as `营业执照`, `万兆接口`, or `响应时间` can match a full expected requirement. Supplying one distinctive fragment per expected clause can produce perfect assignment, precision, recall, family accuracy, and must accuracy because metrics count assignments directly (`crates/bid/src/extraction/evaluation.rs:159-190`). Neither evaluator nor reconciliation enforces minimum semantic completeness or atomicity.

This means the golden gate still does not attest that extracted clauses express the labeled requirements.

**Fix:** use exact normalized quote matching by default, with explicit accepted aliases where necessary. If fuzzy matching is retained, require strong bidirectional coverage and compatible boundaries rather than one-way containment. Add adversarial evaluator tests proving short fragments and broad multi-requirement quotes fail assignment.

## Fixes worth doing now

### [MEDIUM] Unlabelled evaluator runs are reported as “PASS”

When no expected file is supplied, `passed` defaults to true (`crates/bid/src/bin/bid_extract_eval.rs:23-30`), and Markdown output prints `Quality gate: PASS` (`crates/bid/src/bin/bid_extract_eval.rs:39-49`) even though `metrics` is null.

**Fix:** report `NOT_EVALUATED` when labels are absent. Require an expected file for quality-gate mode and reserve PASS/FAIL for evaluated runs.

### [MEDIUM] `must` arbitration still uses context-free substring matching

`resolve_must` removes only three hard exclusions and otherwise tests raw substring presence, with hard phrases taking precedence over optional phrases (`crates/bid/src/extraction/policy.rs:222-252`). The optional policy includes `可以` (`crates/bid/config/cn-tender-v2.json:23-27`).

Examples still mishandled include:

- `不要求必须提供原件` → hard `必须` wins despite negation.
- `设备不可以支持弱算法` → `可以` can force `must=false` despite a prohibition.
- Mixed scoring/mandatory language lacks clause-level context.

The tests cover `优先` and `无需`, but not negated hard phrases or lexical collisions (`crates/bid/src/extraction/coverage.rs:153-166`).

**Fix:** add phrase-aware negation and prohibition handling, avoid matching optional phrases inside negated constructions, and test negated hard, `不可以`, scoring, and mixed hard/optional cases.

### [MEDIUM] Tool/model budgets are not global to one extraction run

`max_tool_calls=48` (`crates/bid/config/cn-tender-v2.json:33-47`) is enforced independently in each family agent and each span sweep (`crates/bid/src/extraction/agent.rs:138-195,251-275`). Hybrid orchestration may run both families over as many as 80 uncovered spans (`crates/bid/src/extraction/mod.rs:186-216`), while accumulated diagnostics are never checked against an engine-wide budget.

The run is bounded, but its theoretical tool/model work is far above the apparent 48-call policy and may monopolize a worker.

**Fix:** maintain an engine-wide request/tool-call/deadline budget shared by both agents, retries, and sweeps. Stop further calls when exhausted and return a visible cap termination.

## Optional / deferred

- Validate `ExpectedClause.family` as a typed technical/commercial value. It is currently an unrestricted string (`crates/bid/src/extraction/evaluation.rs:17-22`), while only family-specific recall thresholds are enforced.
- Add a scripted prompt-injection boundary test that attempts unknown tools, extra fields, wrong-span quotes, and forged completion arguments. The implementation exposes no dangerous tool, but current injection fixture evaluation remains heuristic-only.
- Add golden fixtures for comma-dense prose, multi-requirement table rows, numbered body requirements, must collisions, near-keyword narratives, and intentionally partial outputs.
- Add direct tests for round, clause, and tool-call cap termination paths; current strict behavior is evident in code but lacks focused tests.

## Verified fixed

- **Correct — Exact continuous source quotes:** `quote_in_body` uses direct non-empty `body.contains(quote)` (`crates/bid/src/extraction/reconcile.rs:24-25`). Dispatch rejects wrong-span/non-continuous quotes (`crates/bid/src/extraction/agent.rs:434-450`), reconciliation validates again, and tests distinguish exact source punctuation/spacing from normalized overlap (`crates/bid/src/extraction/reconcile.rs:219-223`).
- **Correct — Server-side typed validation for every tool:** `EmptyArgs`, `SpanArgs`, `GrepArgs`, `EmitBatch`, and `EmitItem` all use `deny_unknown_fields` (`crates/bid/src/extraction/agent.rs:478-506`). Malformed raw JSON becomes null and is rejected by these typed decoders. Extra/malformed argument tests cover non-emit tools (`crates/bid/src/extraction/agent.rs:923-952`), while missing emit fields are tested at `crates/bid/src/extraction/agent.rs:842-876`.
- **Correct — Strict-agent termination:** round, clause, tool-call, and no-tool termination reasons are recorded (`crates/bid/src/extraction/agent.rs:138-195`). Strict agent mode rejects every termination other than `done` (`crates/bid/src/extraction/mod.rs:136-154`), and remaining candidate spans fail extraction rather than being reported done (`crates/bid/src/extraction/mod.rs:273-300`).
- **Correct — Body evidence precedes heading arbitration:** cross-family reconciliation scores the quote body before consulting the heading (`crates/bid/src/extraction/reconcile.rs:135-151`). Strong body evidence resolves contradictory headings; unresolved/tie cases retain `family_conflict=true`. Proposed families remain in extraction metadata (`crates/bid/src/extraction/reconcile.rs:88-121`).
- **Correct — Basic dense prose and table coverage improved:** sentence splitting and one-row table spans are implemented, and tested at `crates/bid/src/extraction/outline.rs:425-445`. The blocker above concerns multiple independent requirements within one sentence or row.
- **Correct — Hallucinated normalized text is removed:** dispatch replaces model-proposed `text` with the verified quote (`crates/bid/src/extraction/agent.rs:458-462`), and reconciliation also persists the representative quote as text (`crates/bid/src/extraction/reconcile.rs:107-115`).
- **Correct — Provider errors are sanitized before persistence:** engine diagnostics use stable provider categories rather than raw errors (`crates/bid/src/extraction/mod.rs:158-170,218-223,311-323`). Production persistence receives only the categorized `ExtractionFailure.message` (`crates/bid/src/lib.rs:793-796`).
- **Correct — One-to-one assignment and actual false-positive metrics exist:** the evaluator uses augmenting-path assignment (`crates/bid/src/extraction/evaluation.rs:118-158,299-317`), computes unmatched outputs as false positives and real precision/recall/F1 (`crates/bid/src/extraction/evaluation.rs:159-169`), and applies thresholds to PASS/FAIL (`crates/bid/src/extraction/evaluation.rs:204-295`).
- **Correct — Evaluator failure affects process status:** labelled threshold failure returns an error/non-zero exit (`crates/bid/src/bin/bid_extract_eval.rs:59-63`).
- **Correct — Prompt and tool isolation remain sound:** the prompt treats document instructions as untrusted, prohibits product/company/KB/external access, and requires continuous source quotes (`crates/bid/prompts/clause-extractor-v2.md:9-26`). Available tools remain limited to local outline/span/grep/emit/done operations.

## Validation limitations

No files were edited. The available review environment exposed no shell/test runner, so Cargo tests and Git status were not executed. The supervisor should run:

```text
cargo test -p bid extraction::
cargo test -p bid golden_fixture
```

## Residual risks

- Numbered `1.` and `一、` requirements without hard-must tokens can disappear before extraction.
- A partial clause can mask sibling requirements within the same sentence or table row.
- Short source fragments can still achieve false true-positive assignments in evaluation.
- Unlabelled evaluator runs display a misleading PASS.
- Must classification remains vulnerable to negation and lexical-context collisions.
- Hybrid extraction lacks a single global request/tool/deadline budget.
- Golden fixtures remain too narrow to detect these regressions.

**Production/quality blockers remain: yes. Security boundary blockers found: no.**
