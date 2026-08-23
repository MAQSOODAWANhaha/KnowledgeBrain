Implemented the bounded `issue.md` contract closure in `plans/bid-platform-complete-solution.md`.

## Changed files

- `plans/bid-platform-complete-solution.md`: targeted plan-only edits; now 1,334 lines / 162,262 bytes; SHA-256 `f73063f6af031e0c6208cc043706bf39cad15ea26dc7fb56520e851b33434104`.
- `subagent-results/bid-plan-issue-md-fix.md`: this required execution report.
- No code, `issue.md`, `PRODUCT.md`, domain documentation, migration, deployment, or runtime-completion file was edited.

## Issue-by-issue fix mapping

### Issue 1 — implementation plan single source of truth

- Added project-level `service_revision + service_set_sha256`.
- Defined exact `ServiceClauseSetV1`: current confirmed `kind=service` members only, fixed top-level/item keys, UUID-byte sorting, uniqueness, bounds, empty-domain hash, storage builder and DB verifier.
- Service enter/leave/semantic changes now atomically bump both matching watermark and service set identity and stale `6:implementation_plan`.
- `6:implementation_plan` now depends only on `ServiceClauseSetV1 + PickSetV1 + schedule_delivery_set`.
- PartDependency now has `service_set`; commercial decisions remain only for ④⑤.
- Removed every `MatchingReportSetV1`, `matching_report_set`, `implementation_matching_set`, and `service matching identity` occurrence, including schema/PR/test/completion references.

### Issue 2 — all supported technical candidates remain selectable

- Added checked `bid_match_current_technical_supported_candidates` projection/API contract restricted to current technical route/report/generation/watermark.
- It exposes every supported candidate in comparator order and marks exactly the RequirementDecision selected row as `is_system_recommended=true`.
- Technical UI supports 1..N picks per requirement; pick mutation must bind current report/requirement/candidate identity and cannot submit arbitrary products.
- Extended PickSetV1 source identity with requirement and candidate artifact IDs, with duplicate and current projection verification.
- Kept ④⑤ commercial candidate view selected-only and explicitly separated it from the technical projection.

### Issue 3 — user-selected option B

- `6:letter` and `6:quote` accept `quote_snapshot=null` only when no current eligible finalized quote exists.
- In that branch, DOCX freezes `QUOTE_NOT_FINALIZED`, `active_quote=null`, and the fixed “报价尚未定稿” placeholder; it never reads the live draft.
- If an eligible snapshot exists, DOCX must freeze it and cannot deliberately choose null.
- Finalize/reopen/eligibility transitions stale and rebuild both parts across null/non-null identity.
- PDF requires identical non-null quote snapshot identities for both parts plus current eligible active quote; any null is rejected.

### Issue 4 — fact validity conflict gate

- Added `BID_VALIDITY_CONFLICT`, `entity_kind=fact`, all UUID locators null, `field=bid_validity`, `reason=null`.
- It is server-derived whenever both validity fields are non-null, frozen as DOCX warning, and hard-rejected for PDF.
- Explicitly prohibited substituting `NO_CEILING_REVIEW`.

### Issue 5 — bounded Agent output authority

- Defined `RequirementSpanAgentV1` exact top-level/item schema for checked span, likelihood and rationale only.
- Defined `FactSuggestionAgentV1` exact schema for allowlisted fact field, typed value, checked span, confidence and rationale only.
- Both forbid extra keys and kind/family/technical/commercial classification output and do not split by matching route.
- Invalid/conflicting output yields bounded failure/review; KindRouter is the sole clause-kind authority, storage solely derives family, and publisher accepts only Router output.
- No second publication state machine was introduced.

### Issue 7 — deterministic report-level matching aggregate

- Defined coverage directly from RequirementDecisionV1 rows: total=eligible=decision count; four support counts sum to total; unresolved_count equals coverage.unresolved.
- Defined block > review > pass aggregation and `degraded == quality_status != pass`.
- Defined report reasons as sorted unique `FROZEN_SCOPE` plus all decision reasons, plus the applicable empty-route reason.
- Expanded the 0013 reason allowlist contract for decision reasons.
- Commit verifier must recompute these fields from staged decisions and reject worker header/reason-set mismatch.
- Added mixed select/review/reject, empty, coverage and reason-union fixtures to PR/test gates.

### Issue 8 — pricing/evaluation tie-break

- Split their priorities.
- An inseparable dual match deterministically chooses `pricing` and records bounded `PRICING_EVALUATION_CONFLICT`; separable clauses still route independently.
- Added an exact golden fixture and shifted service/fallback priorities.

### Issues 6 and 9

- Did not change their approved architecture.
- Existing KindRouter promotion terminal/stale semantics and worker scheduling/job shell language remain intact.

## Validation

1. `git diff --check -- plans/bid-platform-complete-solution.md`
   - Exit 0.
2. `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`
   - Native exit 1 because the plan is untracked and differs from `/dev/null`; diagnostics were 0 bytes, normalized whitespace result passed.
3. Direct Python whitespace/newline/contract/table validator
   - Exit 0: 23 required contract markers, 0 forbidden stale markers, 13 consistent Markdown tables, balanced fences, 1,334 lines, 162,262 bytes.
4. First composite stale-scan command
   - Checker command had a shell backtick quoting defect (`/: Is a directory`); the plan validators before it passed. No plan change was based on that failed scan.
5. Corrected single-quoted stale scan
   - Exit 0: no `MatchingReportSetV1|matching_report_set|implementation_matching_set|service matching identity|pricing\` / \`evaluation` matches.
6. Targeted eight-group issue closure checker
   - Exit 0: Issue 1/2/3B/4/5/7/8 markers present and Issue 6/9 semantics preserved.
7. Scoped status/staged check
   - Exit 0: plan and pre-existing `issue.md` are untracked in scoped status; staged count is 0. No commit or PR was created.

No code tests were run because the task changed only the implementation plan.

## Residual risks

- SQL/Rust/API/Web/CI implementation and the listed cross-layer fixtures remain for PR0–PR7.
- The plan and `issue.md` remain untracked; this worker did not stage or commit them.
- The DOCX-null/PDF-non-null quote choice is now a plan contract but still requires implementation validation of null↔non-null stale transitions.

## Commit / PR state

- No staged files.
- No commit created.
- No PR created.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Only the requested plan was edited as product content. The bounded issue.md findings were closed with ServiceClauseSetV1 as the sole implementation-plan clause source, all-supported technical selection, user-approved DOCX-null quote behavior, exact GateIssue additions, exact Agent authority, deterministic report aggregation and pricing/evaluation tie-break; Issue 6/9 architecture and all out-of-scope files were left unchanged."
    }
  ],
  "changedFiles": [
    "plans/bid-platform-complete-solution.md",
    "subagent-results/bid-plan-issue-md-fix.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "git diff --check -- plans/bid-platform-complete-solution.md",
      "result": "passed",
      "summary": "Exited 0."
    },
    {
      "command": "git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md",
      "result": "passed",
      "summary": "Native exit 1 was expected for untracked content; diagnostics were empty."
    },
    {
      "command": "Python whitespace/newline/contract/fence/table validator",
      "result": "passed",
      "summary": "Validated 23 required contract markers, zero forbidden stale markers, 13 tables, balanced fences, 1334 lines and 162262 bytes."
    },
    {
      "command": "initial composite stale scan",
      "result": "failed",
      "summary": "The checker command used unsafe shell backticks and emitted '/: Is a directory'; this was a checker quoting defect, not a document failure."
    },
    {
      "command": "corrected single-quoted targeted stale scan",
      "result": "passed",
      "summary": "No obsolete MatchingReportSet, service-matching placeholder, or combined pricing/evaluation marker remained."
    },
    {
      "command": "Python eight-group issue closure checker",
      "result": "passed",
      "summary": "Issue 1, 2, 3B, 4, 5, 7 and 8 closure markers passed; Issue 6/9 preservation markers passed."
    },
    {
      "command": "scoped git status and git diff --cached --name-only",
      "result": "passed",
      "summary": "No staged files; no commit or PR."
    }
  ],
  "validationOutput": [
    "ordinary_diff_check=0",
    "noindex_native=1 diagnostics_bytes=0",
    "contracts=23 stale=0 tables=13 lines=1334 bytes=162262",
    "issue1_service_single_truth=true",
    "issue2_all_supported=true",
    "issue3_choice_b=true",
    "issue4_validity=true",
    "issue5_agents=true",
    "issue7_report_aggregate=true",
    "issue8_tiebreak=true",
    "issue6_9_preserved=true",
    "sha256=f73063f6af031e0c6208cc043706bf39cad15ea26dc7fb56520e851b33434104",
    "staged_count=0"
  ],
  "residualRisks": [
    "SQL/Rust/API/Web/CI implementation and cross-layer tests remain for PR0-PR7.",
    "The plan remains untracked and uncommitted."
  ],
  "noStagedFiles": true,
  "diffSummary": "Targeted closure of issue.md Issues 1, 2, 3(B), 4, 5, 7 and 8: service-set implementation authority, technical multi-supported picks, DOCX quote placeholder/PDF hard gate, validity GateIssue, exact Agent boundaries, deterministic report aggregation and routing tie-break; removed all MatchingReportSet terminology.",
  "reviewFindings": [
    "no blocker found in worker targeted validation; independent bounded review remains the parent gate"
  ],
  "manualNotes": "No code tests were run because only the plan was edited. The initial stale-scan checker quoting defect was corrected and the final scan passed."
}
```
