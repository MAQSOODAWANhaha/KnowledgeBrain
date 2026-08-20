# Bid Extractor Round 1 — Security and Extraction-Quality Review

## Review

### Blockers / approval decision

**Approval decision: not approved for quality-gate or production rollout. Three high-severity blockers remain. Security isolation is substantially sound, but extraction completeness and evaluation validity are not yet attestable.**

1. **Blocker [HIGH] — The golden test does not measure precision and can pass with arbitrarily many source-grounded false positives.**  
   `unsupported` is defined only as a quote that fails source containment, not as an extracted clause absent from the labels (`crates/bid/src/extraction/mod.rs:475-485`). Family and must accuracy use only matched expected items as their denominator, ignoring unmatched output (`crates/bid/src/extraction/mod.rs:486-519`). Matching uses containment overlap and does not enforce one-to-one assignments, so one broad output can satisfy multiple expected clauses (`crates/bid/src/extraction/mod.rs:487-494`). The only false-positive checks are a short hand-written `absent_quotes` list (`crates/bid/src/extraction/mod.rs:528-536`; `testdata/bid-extraction/cn-tender-golden-02.expected.json:1-26`).  
   Consequently, the advertised precision threshold is not implemented, and the test may pass even when precision is poor. The manual evaluator likewise emits the raw report but calculates no expected-set metrics (`crates/bid/src/bin/bid_extract_eval.rs:16-45`).  
   **Required:** implement one-to-one expected/actual matching, precision, recall, F1, false-positive counts, atomicity checks, and family/must accuracy over assigned pairs. Add negative narrative, procedural, table-header, and near-keyword cases.

2. **Blocker [HIGH] — Common numbered requirement lines can be classified as headings and silently removed from extraction.**  
   The outline loop treats every parsed heading as structural metadata and never adds its text to a span body (`crates/bid/src/extraction/outline.rs:21-39`). Short `1 …`, `1）…`, Chinese `一、…`, and parenthesized Chinese lines are recognized as headings (`crates/bid/src/extraction/outline.rs:260-291`). Thus a line such as `1）设备必须支持万兆接口` without terminal punctuation becomes a heading; if no following body exists, the requirement is absent from every span and cannot be recovered by either agent or heuristic. This is a likely tender/list format, not an edge-only syntax.  
   **Required:** distinguish numbered headings from numbered requirements using context, typography, length and requirement signals, or preserve ambiguous heading text as an extractable span. Add tests for numbered list requirements with and without punctuation.

3. **Blocker [HIGH] — “Span covered” still permits many missed requirements to be marked done.**  
   Coverage considers a span covered as soon as any clause references its `span_id` (`crates/bid/src/extraction/coverage.rs:17-28`; `crates/bid/src/extraction/mod.rs:237-274`). Paragraphs accumulate until blank lines or 8,000 characters, and table spans contain up to 20 rows (`crates/bid/src/extraction/outline.rs:147-205`, especially `:172-180`). One extracted sentence or table row therefore prevents sweep and heuristic recovery for all other requirements in that span. This recreates the original masking problem at a smaller granularity and makes `extract_status="done"` unreliable for dense paragraphs and tables.  
   **Required:** create requirement-sized coverage units—at minimum sentence/list-item/table-row units—or track detected requirement anchors within each span. Add a test where only one of several table rows or same-paragraph requirements is emitted and verify the rest remain uncovered.

### Fixes worth doing now

1. **[HIGH] Cross-family reconciliation lets heading priors override strong body evidence and hides the disagreement.**  
   When both agents emit the same quote, `choose_family` immediately returns the heading family and sets `family_conflict=false` (`crates/bid/src/extraction/reconcile.rs:136-147`). Body scoring only runs for unknown headings (`:148-154`). A stray technical-agent extraction of `注册资本不低于…` under a technical heading therefore defeats the correct commercial-agent extraction and is not flagged for review. This undermines the independent-agent correction objective.  
   Reconcile using body evidence separately from heading priors, retain visible conflict when agents disagree materially, and add scripted cross-heading tests. `CandidateClause` also carries no confidence despite the planned confidence tie-break (`crates/bid/src/extraction/types.rs:87-94`).

2. **[MEDIUM] Non-emit tool schemas are strict only at the provider boundary, not server-enforced.**  
   Tool definitions correctly use `strict=true` and `additionalProperties=false` (`crates/bid/src/extraction/agent.rs:436-499`), but dispatch accepts `done` and `list_outline` without inspecting arguments and reads only selected fields for `read_span` and `grep` (`crates/bid/src/extraction/agent.rs:291-339`). Extra arguments are therefore accepted if a provider does not honor strict schemas; malformed JSON becomes `{}`, which is accepted by `done` and `list_outline` (`crates/bid/src/extraction/agent.rs:162-175`). Only `emit_clauses` has `deny_unknown_fields` server-side validation.  
   Add typed, `deny_unknown_fields` deserialization for every tool, including empty-object tools, and runtime tests for extra and malformed arguments.

3. **[MEDIUM] Strict agent termination and failed-coverage status are misleading.**  
   Reaching `max_rounds` without `done` returns a successful `AgentOutcome` with no termination diagnostic (`crates/bid/src/extraction/agent.rs:137-182`). The engine reports `Ok` even when all candidate spans remain uncovered (`crates/bid/src/extraction/mod.rs:234-278`). Such a document is counted as successful and the run may be finalized as `done` (`crates/bid/src/lib.rs:756-785`). The UI does show uncovered spans, which is good, but the terminal status still misrepresents strict-agent completion.  
   Record termination reason (`done`, round cap, clause cap, no tool call), fail strict-agent runs on cap exhaustion, and consider a partial/failed terminal state when candidate spans remain uncovered.

4. **[MEDIUM] Resource limits do not cover total tool calls, input/span count, regex length, tool-output bytes, or request timeout.**  
   Existing limits cover rounds, attempts, per-emit items, final clauses, span size and grep hit count (`crates/bid/src/extraction/policy.rs:49-59`; `crates/bid/src/extraction/agent.rs:137-150,339-363,375-377`). However:
   - Multiple tool calls may be emitted each round with no total call cap.
   - All section/span metadata is placed into the initial outline (`crates/bid/src/extraction/agent.rs:502-530`).
   - Regex patterns have no explicit length/complexity bound.
   - Up to 40 full matching lines can be returned, potentially hundreds of kilobytes.
   - No extraction-specific request timeout is configured in the client construction (`crates/bid/src/extraction/agent.rs:67-88`).

   Rust’s `regex` implementation avoids catastrophic backtracking, but compile/search and model-context costs still need explicit byte/call caps. The API has a global request-body limit (`crates/api/src/routes.rs:232-234`), so this is bounded externally, but converted Markdown expansion and repeated model calls remain concerns.

5. **[MEDIUM] `must` correction uses raw substring matches and mishandles lexical occurrences and mixed semantics.**  
   `resolve_must` tests whether any configured token occurs anywhere and always gives hard tokens precedence (`crates/bid/src/extraction/policy.rs:215-233`). The optional policy includes the single character `可` (`crates/bid/config/cn-tender-v2.json:31-34`), so `设备应支持可视化` or `可靠性` can force a model-proposed mandatory requirement to false. Conversely, negated wording such as `不要求必须…` and scoring text containing `必须` can become true.  
   Use phrase/context rules for negation and scoring clauses, avoid single-character substring matching inside words, and add mixed hard/optional and lexical-collision tests.

6. **[MEDIUM] A valid quote can be paired with hallucinated normalized `text`.**  
   Dispatch validates only that `text` is non-empty and that `quote` belongs to the selected span (`crates/bid/src/extraction/agent.rs:375-405`). Reconciliation repeats those checks but does not validate conditions, numbers, or scope in `text` (`crates/bid/src/extraction/reconcile.rs:47-65`). The prompt forbids additions, but the server does not enforce them. Since downstream matching uses normalized clause text, a valid short quote can legitimize a materially altered requirement.  
   Prefer server-derived text, or validate that numeric values, units, negation and constraint terms in `text` are supported by the quote.

7. **[MEDIUM] Golden security coverage does not exercise the LLM/tool boundary.**  
   The injection fixture is evaluated exclusively in `ExtractionMode::Heuristic` (`crates/bid/src/extraction/mod.rs:454-459`). It proves the heuristic does not emit that sentence, but not that an injected document cannot influence an agent’s tool flow. Add a scripted chat test that attempts forbidden/unknown tools, extra server-owned fields, fake `done` arguments, and a quote from the wrong span.

8. **[LOW] Raw provider errors are persisted and returned through the project API.**  
   Model errors are interpolated directly into fallback/failure strings (`crates/bid/src/extraction/mod.rs:143-164,200-208`), persisted in per-document diagnostics (`crates/bid/src/lib.rs:763-788`), and returned wholesale as `latest_extract.diagnostics` (`crates/api/src/routes.rs:3695-3708`). Some provider error payloads may include endpoint details or request excerpts.  
   Persist stable error categories plus bounded, sanitized summaries; keep detailed provider errors in restricted logs if needed.

### Optional / deferred ideas

- Split long prose on sentence boundaries before falling back to arbitrary character chunks; current `split_chars` can divide a requirement across spans (`crates/bid/src/extraction/outline.rs:219-226`).
- Reconciliation groups overlapping quotes and selects the longest representative (`crates/bid/src/extraction/reconcile.rs:68-91`). A broad multi-requirement quote can swallow multiple atomic clauses. Validate atomicity or split broad quotes before grouping.
- Expand fixtures beyond the current two short documents to include no-heading long prose, 20+ row tables, multiple requirements in one paragraph, numbered requirements, lexical `must` collisions, repeated requirements across spans, and high-density false-positive narratives.
- The evaluator README says quotes must be continuous substrings (`testdata/bid-extraction/README.md:24-27`), while validation intentionally uses normalized containment. Document explicitly which whitespace and punctuation changes are accepted.

### Verified-good areas

- **Correct — Prompt-injection isolation is explicit.** The system prompt labels tender text untrusted and says body instructions cannot alter role or trigger external/KB access (`crates/bid/prompts/clause-extractor-v2.md:9-14`).
- **Correct — Extraction tools expose no product, company, search-service, network, filesystem, or generic execution capability.** The available functions are only local outline/span/regex read operations, clause emission, and completion (`crates/bid/src/extraction/agent.rs:430-488`). Product/company matching occurs later in the separate matching path, not inside `TenderExtractionEngine`.
- **Correct — Family and provenance fields are server-owned.** The model emits only `span_id`, `quote`, `text`, and `must`; family and extractor are assigned by dispatch (`crates/bid/src/extraction/agent.rs:367-405,457-480`).
- **Correct — `emit_clauses` has strict schema and server-side unknown-field rejection.** Both batch and item structs use `deny_unknown_fields`, required fields are declared, and per-call `maxItems` is enforced (`crates/bid/src/extraction/agent.rs:367-375,414-424,457-480`). Missing-field behavior is tested (`crates/bid/src/extraction/agent.rs:762-791`).
- **Correct — Quote provenance is checked against the referenced span twice.** Dispatch rejects unknown span IDs and normalized non-containment, and reconciliation repeats validation (`crates/bid/src/extraction/agent.rs:375-386`; `crates/bid/src/extraction/reconcile.rs:8-25,47-65`). Persisted `source_span` includes `span_id`, heading path, and quote (`crates/bid/src/lib.rs:707-728`).
- **Correct — Technical and commercial agents run independently before reconciliation.** Neither family sees or suppresses the other family’s candidate set during its run (`crates/bid/src/extraction/mod.rs:112-166`).
- **Correct — Retries and hybrid fallback are counted and visible.** Retries are bounded and counted (`crates/bid/src/extraction/agent.rs:252-271`); fallback reasons, uncovered spans and conflicts are persisted and surfaced in UI notices (`crates/bid/src/extraction/mod.rs:143-230,243-253`; `web/src/bid/Workbench.tsx:198-208`).
- **Correct — Bullet list items and later long-section spans are independently readable.** Bullet/numbered body items that are not mistaken for headings become separate spans (`crates/bid/src/extraction/outline.rs:187-205,229-242`), and a test exercises reading/emitting from a later long-section span (`crates/bid/src/extraction/agent.rs:794-829`).
- **Correct — Diagnostics structures do not intentionally persist conversations or full tool transcripts.** They retain counters, span IDs, versions and fallback/error summaries (`crates/bid/src/extraction/types.rs:118-132`), subject to the raw-error caveat above.
- **Correct — Successful document persistence is transactional and quote provenance is retained.** Sections are upserted, old drafts superseded, clauses inserted, and obsolete sections pruned in one transaction (`crates/storage/src/bid.rs:426-515`).

## Validation and limitations

- No files were edited.
- No shell/test runner was available, so focused Cargo tests and Git diff/status commands were not executed.
- Review evidence comes from direct inspection of the current worktree. Exact staged/unstaged diff boundaries could not be generated with the available read-only tools.

## Residual risks

- LLM prompt isolation remains defense-in-depth rather than a proof against model misbehavior; the strong boundary is the restricted tool set.
- Dense paragraphs/tables can report complete coverage despite omitted requirements.
- Golden thresholds currently attest recall on a narrow heuristic fixture, not production precision or real-model security behavior.
- External provider errors may expose more detail than intended through persisted diagnostics.
- Model/tool request resource consumption lacks complete call, byte, and timeout limits.