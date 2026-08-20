# Bid Extractor Hardening — Continuation Round-3 Final Review

## Review

### Correct

- No blocker was found.
- User-approved overrides are present:
  - Automatic `text` is canonicalized to the verified quote: `crates/bid/src/extraction/agent.rs:436-457`, `crates/bid/src/extraction/reconcile.rs:99-125`.
  - Arbitration follows heading prior → Policy score → server extractor rank → technical/conflict tie: `crates/bid/src/extraction/reconcile.rs:140-181`.
  - Ambiguous `must` defaults false, and table syntax alone does not make it mandatory: `crates/bid/src/extraction/policy.rs:221-250`.
  - Mutable outline/table terms are typed Policy fields: `crates/bid/src/extraction/policy.rs:10-53`, `crates/bid/config/cn-tender-v2.json:29-42`.
  - Quote rejection counts from successful Agent/sweep outcomes accumulate rather than being overwritten: `crates/bid/src/extraction/mod.rs:210-213,245-267,331-340`.
  - Partial multi-document failures are retained and projected compactly: `crates/bid/src/lib.rs:899-933`, `crates/api/src/routes.rs:3710-3760`, `web/src/bid/Workbench.tsx:193-203`.
  - Evaluator extraction failures now write FAIL artifacts before returning nonzero: `crates/bid/src/bin/bid_extract_eval.rs:17-42`.
  - Exact source provenance is revalidated and persisted as `span_id/heading_path/quote`: `crates/bid/src/extraction/reconcile.rs:40-61`, `crates/bid/src/lib.rs:990-1014`.
  - Automatic clauses remain draft-only; matching queries only confirmed clauses: `crates/storage/src/bid.rs:1250-1304,1471-1483`.
  - Confirmation clears `family_conflict`: `crates/storage/src/bid.rs:1429-1468`.

### Blocker

- None.

### High — Neutral table rows under family-hinted headings still become clauses or fatal coverage gaps

**Evidence**

- Any non-chrome table row under a technical/commercial heading is marked candidate without requirement evidence: `crates/bid/src/extraction/outline.rs:119-145`.
- Heuristic extraction likewise accepts table syntax plus a family heading as sufficient: `crates/bid/src/extraction/coverage.rs:82-118`.
- The only neutral-table test uses the unknown heading `设备清单`, so it does not cover the family-heading branch: `crates/bid/src/extraction/coverage.rs:160-170`.

A table such as:

```markdown
# 技术参数
| 序号 | 名称 |
|---|---|
| 1 | 路由器 |
```

produces the draft `| 1 | 路由器 |` in heuristic mode. A strict Agent correctly emitting nothing instead leaves a candidate uncovered and fails the document.

**Narrow fix:** Require a body-level Policy requirement/family predicate for table data rows; heading prior may choose family but must not independently establish that a row is a requirement. Add neutral rows beneath both technical and commercial headings.

### High — Heuristic splitting breaks exact key/value rows containing semicolons

**Evidence**

- Dependent key/value rows are intentionally retained as exact Markdown rows: `crates/bid/src/extraction/outline.rs:212-224`.
- Heuristic extraction unconditionally splits every span at Chinese/ASCII semicolons: `crates/bid/src/extraction/coverage.rs:27-29,121-126`.
- Final reconciliation requires a table quote to equal the entire row: `crates/bid/src/extraction/reconcile.rs:46-55`.

For example, `| 最大响应时间 | 2秒；峰值3秒 |` is a candidate exact-row span, but heuristic candidates are partial row fragments. Reconciliation rejects them, leaving the span uncovered and failing heuristic/default-hybrid extraction.

**Narrow fix:** When a span body is an exact Markdown table row, return the whole body as its sole heuristic requirement unit. Add a heuristic end-to-end test with `；` and `;` inside an exact row.

### Medium — Tool limits are not enforced as exact server-side bounds

**Evidence**

- `read_span` serializes heading, context and body, then generic truncation can cut the JSON into invalid text: `crates/bid/src/extraction/agent.rs:295-302,364-380`.
- Policy validation only compares `max_span_chars + 2048` with output size; it does not bound heading paths, JSON escaping or total serialized payload: `crates/bid/src/extraction/policy.rs:117-119`.
- Oversized `emit_clauses` batches are silently truncated with `.take(max_emit)` rather than rejected: `crates/bid/src/extraction/agent.rs:423-433`.
- Oversized exact rows are deliberately retained but made non-candidates: `crates/bid/src/extraction/outline.rs:731-739`. That documented override is implemented, but it does not close the serialized-payload cases above.

**Narrow fix:** Bound the complete serialized `read_span` payload during Span construction or return a structured size error without malformed JSON. Reject an emit batch exceeding `max_emit` in full. Test long headings, escape-heavy text, and `max_emit + 1`.

### Medium — Failed provider attempts disappear from cumulative diagnostics

**Evidence**

- Retry attempts mutate local `AgentStats`, but terminal provider failure returns only a string: `crates/bid/src/extraction/agent.rs:265-292`.
- The engine merges statistics only from successful `AgentOutcome`; error branches record a fallback category without rounds/retries: `crates/bid/src/extraction/mod.rs:116-169,201-220`.

After three failed attempts, persisted diagnostics can report zero retries and zero attempted rounds, contrary to the documented rounds/retries diagnostics contract.

**Narrow fix:** Return an error carrying `AgentStats`, and merge it in both family-Agent and span-sweep error branches. Add strict and hybrid terminal-failure assertions.

### Medium — The active Prompt contradicts the approved canonical-text contract

**Evidence**

- The Prompt tells the model that `text` may be normalized: `crates/bid/prompts/clause-extractor-v2.md:19-23`.
- Runtime discards that normalization and persists `text=quote`: `crates/bid/src/extraction/agent.rs:436-457`, `crates/bid/src/extraction/reconcile.rs:115-118`.
- Domain documentation correctly describes the approved canonical behavior: `docs/bid-platform-domain.md:290-297`.

**Narrow fix:** Change the Prompt to require `text` but instruct that it equal the exact verified quote. Bump `prompt_version` because the embedded system instruction changes.

## Steps 1–15 traceability

| Step | Verdict |
|---:|---|
| 1 | Complete |
| 2 | Complete |
| 3 | Complete |
| 4 | Complete under the documented oversized-row override |
| 5 | Partial — serialized tool bounds and oversized emit rejection are not exact |
| 6 | Complete |
| 7 | Partial — neutral family-heading tables and semicolon exact rows are incorrect |
| 8 | Complete |
| 9 | Complete |
| 10 | Complete |
| 11 | Complete |
| 12 | Partial — terminal provider attempts lose statistics |
| 13 | Complete |
| 14 | Partial — evaluator artifacts are fixed, but fixture/scripted breadth remains incomplete |
| 15 | Partial — domain docs largely align, but the embedded Prompt contradicts canonical text and the complete-tool-output claim is too broad |

## Accepted external/manual gaps and optional breadth

- **Accepted external/manual:** Strict real Agent, VLM, non-Markdown DocReader, LDAP and real embedding validation require external services and credentials.
- **Optional release breadth:** Redis/PostgreSQL restart injection, real product/company assets, pick/shot flow and timing-controlled old-slow/new-fast provider tests are outside extraction correctness.
- **Still partial against Step 14, but not itself a release blocker:** Goldens still lack unheaded long prose and a genuinely long/cross-page table; the golden gate remains heuristic rather than a full Agent-mode scripted golden: `testdata/bid-extraction/cn-tender-golden-01.md:1-13`, `testdata/bid-extraction/cn-tender-golden-02.md:1-35`, `crates/bid/src/extraction/mod.rs:518-545`.

## Verdict

The implementation now matches most of the approved architecture and overrides, but **does not completely match the approved plan**. Table extraction still has two production correctness defects, while tool-bound diagnostics and Prompt alignment remain partial.

**An immediate narrow code fix is worth doing before release:** fix neutral family-heading tables and exact rows containing semicolons first. The Prompt correction is also low-risk and worthwhile. No broad rewrite is needed.

No commands or tests were run because this review session had read/search tools only. Git/staged state and the handoff’s prior command results were not independently re-attested.