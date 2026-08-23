Implemented the user-approved B-state-machine closure and the four remaining bounded `issue.md` corrections in `plans/bid-platform-complete-solution.md`.

## Changed files

- `plans/bid-platform-complete-solution.md`: targeted plan-contract edits only; now 1,343 lines / 172,889 bytes; SHA-256 `836f5959c4e52ee394645667e8702d3f589adfe2500be710d36b1f9514d28781`.
- `subagent-results/bid-plan-current-issue-fix-round1.md`: required execution report.
- No edit to `issue.md`, code, `PRODUCT.md`, domain documentation, migration, deployment, or runtime-completion files.

## Issue mapping

### Issue 1 — approved B promotion state machine

- Fixed the only KindRouter promotion path that changes a confirmed clause kind: maintenance gate → KindRouter current contract → projects by UUID → clauses by UUID → procedural identities.
- The controlled function must first transition `confirmed→draft`, calculate all OLD membership exits, update matching watermark and applicable service/nonmatching/procedural revisions, digests and stale identities, then write the new Router kind plus uniquely derived family while the clause remains draft.
- Draft rows cannot enter NEW matching/clause sets, create current procedural classification/decision, satisfy parts, or pass formal PDF. Durable user/api-key confirmation with expected clause revision and current promotion generation clears the marker and enters NEW membership exactly once.
- Added minimal metadata on the existing draft/confirmed lifecycle rather than a second state machine: `confirmation_required_reason=KIND_ROUTER_PROMOTION_RECONFIRM` plus `confirmation_required_router_generation`.
- Fixed system actor/audit (`system:kind-router-promotion:v1`), transition reason, before/after identity, UI text and a canonical `KIND_ROUTER_RECONFIRMATION_REQUIRED` GateIssue (`entity_kind=clause`, `entity_id=clause UUID`, `field=kind`). DOCX freezes a warning; PDF hard-rejects.
- Reconciled the existing extracted hard invalidation: unchanged procedural kind keeps the clause confirmed and redoes the new-key procedural decision; changed kind takes the approved draft/reconfirm path. Added explicit `service→technical` and `procedural→qualification` fixtures and negative seams.

### Issue 2 — ServiceClauseSet project columns

- Added to §5.4:
  - `service_revision bigint NOT NULL DEFAULT 0`
  - `service_set_sha256 bytea(32) NOT NULL`
- Fixed the revision-0 value to the `kb:service-clause-set:v1:empty` domain-tagged V1 empty hash.
- Kept ownership in `0015`/PR3 and explicitly made 0018/PR6 a consumer; synchronized schema slice, PR matrix, DB constraints, clause-set tests and completion definition.

### Issue 3 — unsectioned identity

- Fixed the unique unsectioned route identity as `kind=technical` with lowercase nil UUID JSON string `00000000-0000-0000-0000-000000000000`, never JSON/SQL NULL and never commercial.
- Required the technical supported projection and every related PickSetV1 item to carry the same nil UUID.
- Fixed the formal part key to `2:unsectioned` and prohibited `2:<nil-uuid>`.
- Synchronized route schema, projection, PickSet verifier, RequiredPartSet, PartDependency current verifier, 0013/0018 slices, PR gates and positive/negative fixtures.

### Issue 4 — matching reason allowlist

- Made the 0013 SQL/Rust/DTO/fixture allowlist explicit: `FROZEN_SCOPE|SUPPORTED|UNRESOLVED|INSUFFICIENT|CONTRADICTED|NO_EVIDENCE`.
- Stated that `EMPTY_ROUTE|SKIP_UNIT` are retained verbatim from 0012 in the same allowlist, with no aliases.
- Synchronized PR1 and mandatory matching tests.

### Issue 5 — cross-reference

- Corrected FactSuggestionAgentV1 field reference from §5.1 to §5.2 only.

## Validation and commands

1. `git diff --check -- plans/bid-platform-complete-solution.md`
   - Exit 0.
2. `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`
   - Native exit 1 because the plan is untracked and differs from `/dev/null`; diagnostics were empty, normalized whitespace validation passed.
3. Initial combined required-marker/stale/table checker
   - Exit 1 because the stale regex intentionally searched for `confirmed clause.*静默保持confirmed` and matched the newly added negative statement “不能静默保持confirmed”. This was a checker false positive, not a plan defect.
4. Corrected stale scan plus escape/backtick-aware Markdown table checker
   - Exit 0: zero real stale markers; 13 consistent tables; balanced fences; no tabs/trailing whitespace; final newline present.
5. Five-group targeted contract checker
   - Exit 0: Issue 1 B promotion 7 markers, Issue 2 service columns 3, Issue 3 unsectioned 4, Issue 4 reason allowlist 2, Issue 5 cross-reference 1; forbidden stale markers 0.
6. Final direct validation after the last wording correction
   - Exit 0: 1,343 lines, 172,889 bytes, 13 tables, 10 core required markers, staged count 0.
7. `git diff --no-index --stat /dev/null plans/bid-platform-complete-solution.md`
   - Native exit 1 as expected for an untracked file; one plan file and 1,343 inserted lines relative to `/dev/null`.
8. Scoped status and staged check
   - Exit 0: only pre-existing untracked `issue.md` and the plan appear in scoped status; no staged files.

No code tests were run because this task changed only the implementation plan.

## Surprises

- A naive stale regex matched the desired prohibition sentence. The checker was narrowed; no document change was made because of that false positive.
- The approved B behavior needs a persistent, derivable gate locator so a draft requirement cannot disappear from formal readiness. The minimal clause confirmation-reason/generation metadata and GateIssue were added on the existing lifecycle without introducing a parallel state machine.

## Remaining

- Fresh-context parent review remains required.
- SQL/Rust/API/Web/CI implementation and all listed fixtures remain for PR0–PR7.
- The plan and pre-existing `issue.md` remain untracked and uncommitted.

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
      "evidence": "Only plans/bid-platform-complete-solution.md was edited as product content. The approved KindRouter B transition, ServiceClauseSet columns, unsectioned nil identity, explicit FROZEN_SCOPE allowlist and §5.2 cross-reference were closed and synchronized with existing slices, gates and tests without widening Playwright, object, migration or deployment scope."
    }
  ],
  "changedFiles": [
    "plans/bid-platform-complete-solution.md",
    "subagent-results/bid-plan-current-issue-fix-round1.md"
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
      "command": "initial combined required-marker/stale/table checker",
      "result": "failed",
      "summary": "An over-broad stale regex matched the intended negative phrase; this was a checker false positive."
    },
    {
      "command": "corrected stale and escape/backtick-aware Markdown table checker",
      "result": "passed",
      "summary": "Zero stale markers, 13 consistent tables, balanced fences and clean whitespace."
    },
    {
      "command": "five-group targeted issue closure checker",
      "result": "passed",
      "summary": "All current issue.md Issue 1-5 closure markers were present and stale markers absent."
    },
    {
      "command": "final direct validation, SHA and scoped status/staged check",
      "result": "passed",
      "summary": "1343 lines, 172889 bytes, SHA-256 836f5959c4e52ee394645667e8702d3f589adfe2500be710d36b1f9514d28781, staged count 0."
    }
  ],
  "validationOutput": [
    "final_validation_ok lines=1343 bytes=172889 tables=13 required=10",
    "issue1_b_promotion=ok",
    "issue2_service_columns=ok",
    "issue3_unsectioned=ok",
    "issue4_reasons=ok",
    "issue5_crossref=ok",
    "forbidden_stale=0",
    "sha256=836f5959c4e52ee394645667e8702d3f589adfe2500be710d36b1f9514d28781",
    "staged_count=0"
  ],
  "residualRisks": [
    "Fresh independent review remains required.",
    "SQL/Rust/API/Web/CI implementation and cross-layer fixtures remain for PR0-PR7.",
    "The plan and issue.md remain untracked and uncommitted."
  ],
  "noStagedFiles": true,
  "diffSummary": "Targeted closure of current issue.md Issues 1-5: approved B draft/reconfirm promotion fencing and GateIssue, explicit 0015 ServiceClauseSet columns/empty hash, exact unsectioned nil identities, explicit FROZEN_SCOPE reason allowlist, and corrected Agent fact-section reference.",
  "reviewFindings": [
    "no blocker found in worker targeted validation; fresh-context parent review remains required"
  ],
  "manualNotes": "No code tests were run because only the implementation plan was edited. No commit or PR was created."
}
```
