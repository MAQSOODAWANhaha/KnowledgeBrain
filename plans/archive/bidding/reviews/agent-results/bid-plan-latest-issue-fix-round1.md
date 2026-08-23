Implemented the user-approved current `issue.md` Issue 1–3 closure in `plans/bid-platform-complete-solution.md` without widening scope.

## Changed files

- `plans/bid-platform-complete-solution.md`: targeted plan-contract edits only; now 1,347 lines / 177,550 bytes; SHA-256 `d9bb9129d3e125d4ac2d385b2feb5a852a9cae0e92698a6c12eb905c8b896955`.
- `subagent-results/bid-plan-latest-issue-fix-round1.md`: required execution report.
- No edit to `issue.md`, code, `PRODUCT.md`, domain documentation, migration, deployment, or runtime-completion files.

## Issue mapping

### Issue 1 — consecutive KindRouter promotion

- The controlled promotion now processes both eligible confirmed extracted clauses and every pending draft with `confirmation_required_reason=KIND_ROUTER_PROMOTION_RECONFIRM` on every generation, under the existing maintenance → router current → project UUID → clause UUID → procedural identities lock order.
- The first eligible confirmed cross-kind transition still exits OLD membership exactly once before writing the new kind/family while draft.
- Later promotions keep pending rows draft and never repeat OLD exits or enter NEW membership.
- If the pending row remains unedited extracted with a current frozen SourceSpanV2 that passes scope verification, it is recomputed with the current Router and family derivation seam. If it is manual/manual_after_edit or no longer has a legal current frozen span, its current human-owned kind/family remains unchanged.
- Both pending branches refresh `confirmation_required_router_generation`, bump clause revision exactly once, retain the reason, and append audit data containing before/after revision, kind/family, old/new marker generation, promotion generation, system actor and `router_recomputed`.
- Normal durable user/api-key confirmation still requires expected clause revision and marker generation equal to the current promotion generation. It clears the marker and enters final NEW membership exactly once; CAS loss or repeated confirmation cannot double-bump.
- Gate/UI/PDF blocking remains until successful confirmation. Fixtures now require generation 2 → pending draft → generation 3 without human confirmation → successful confirmation for both recomputed extracted and preserved manual/manual_after_edit branches.

### Issue 2 — promotion provenance boundary

- Automatic KindRouter promotion is now explicitly limited to unedited `provenance=extracted` clauses with a valid current frozen SourceSpanV2 and scope proof.
- KindRouter input, §3.2, procedural hard invalidation, schema ownership, PR3 gates, mandatory tests and completion criteria use the same eligibility predicate.
- Confirmed manual/manual_after_edit clauses are skipped by automatic promotion. Their kind changes only through durable user/api-key PATCH plus normal confirmation.
- A pending marker row that later becomes manual/manual_after_edit still has its marker generation/revision/audit refreshed on every promotion, without automatic kind/family recomputation, preventing permanent confirmation failure.
- Removed the stale statement that promotion could automatically change a confirmed manual/manual_after_edit clause kind.

### Issue 3 — exact `2:unsectioned` PickSet subset

- Defined the part’s unique current unsectioned report artifact as `R`.
- Defined the relevant subset exactly as `S = PickSetV1.items WHERE source_report_artifact_id = R.id`.
- Only `S` must carry the lowercase nil UUID and match `R.route.unit_id`; ordinary-unit PickSet items in the same project are not subject to the nil constraint.
- The global current-visible verifier still proves all source projection/report identities current.
- PartDependency, 0013/0018 slices, PR1/PR6 gates, tests and completion criteria now require mixed ordinary-unit + unsectioned positive fixtures and reject wrong-report/nil mappings.

## Validation and commands

1. `python3` targeted whitespace/newline/required-marker/stale-marker/fence/table validator
   - Exit 0: 1,347 lines, 177,550 bytes, 10 required marker groups, 13 consistent Markdown tables, balanced fences, no tabs/trailing whitespace, final newline present.
2. `git diff --check -- plans/bid-platform-complete-solution.md`
   - Exit 0.
3. `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`
   - Native exit 1 because the plan is untracked and differs from `/dev/null`; diagnostics were empty, so normalized whitespace validation passed.
4. `git diff --no-index --stat /dev/null plans/bid-platform-complete-solution.md`
   - Native exit 1 as expected; one plan file and 1,347 inserted lines relative to `/dev/null`.
5. Targeted `rg` scan
   - Exit 0: required pending-draft and report-subset contracts found; stale automatic-manual-promotion and ambiguous `每个相关PickSet item` wording absent.
6. Scoped status and staged-file check
   - Exit 0: only pre-existing untracked `issue.md` and the plan appear in scoped status; staged count 0.
7. `sha256sum plans/bid-platform-complete-solution.md`
   - Exit 0: `d9bb9129d3e125d4ac2d385b2feb5a852a9cae0e92698a6c12eb905c8b896955`.

No code tests were run because this task changed only the implementation plan.

## Surprises

- A pending clause may become manual/manual_after_edit between Router generations. Refreshing its marker generation without recomputing the human-owned kind is necessary to satisfy both the approved provenance boundary and recovery from consecutive promotions.
- Marker refresh is a real row mutation; the plan now requires a clause revision bump so stale clients cannot confirm without observing the newest Router generation.

## Remaining / residual risks

- Fresh-context parent review remains required.
- SQL/Rust/API/Web/CI implementation and the listed cross-layer fixtures remain for PR0–PR7.
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
      "evidence": "Only plans/bid-platform-complete-solution.md was edited as product content. The approved extracted-only promotion boundary, every-generation pending-draft refresh/recovery, manual/manual_after_edit preservation, exact report-scoped unsectioned PickSet subset, and synchronized schema/PR/test/completion gates were implemented without adding product, migration, deployment, or object-layer scope."
    }
  ],
  "changedFiles": [
    "plans/bid-platform-complete-solution.md",
    "subagent-results/bid-plan-latest-issue-fix-round1.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "Python whitespace/newline/targeted contract/stale/fence/table validator",
      "result": "passed",
      "summary": "Validated 1347 lines, 177550 bytes, 10 required contract markers, zero targeted stale forms and 13 consistent Markdown tables."
    },
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
      "command": "targeted rg required/stale scan",
      "result": "passed",
      "summary": "Found the exact pending-draft and R/S subset contracts; obsolete automatic-manual-promotion and ambiguous PickSet wording were absent."
    },
    {
      "command": "scoped git status and git diff --cached --name-only",
      "result": "passed",
      "summary": "Only pre-existing untracked issue.md and the plan appeared; staged count was zero."
    }
  ],
  "validationOutput": [
    "validation_ok lines=1347 bytes=177550 required=10 tables=13",
    "noindex_native=1 diagnostics_bytes=0",
    "sha256=d9bb9129d3e125d4ac2d385b2feb5a852a9cae0e92698a6c12eb905c8b896955",
    "scoped status: ?? issue.md; ?? plans/bid-platform-complete-solution.md",
    "staged_count=0"
  ],
  "residualRisks": [
    "Fresh independent review remains required.",
    "SQL/Rust/API/Web/CI implementation and cross-layer fixtures remain for PR0-PR7.",
    "The plan and issue.md remain untracked and uncommitted."
  ],
  "noStagedFiles": true,
  "diffSummary": "Closed current issue.md Issues 1-3 with every-generation pending-draft recovery, extracted-only Router promotion/manual kind preservation, and exact report-scoped unsectioned PickSet subset semantics, synchronized through schema slices, PR gates, tests and completion criteria.",
  "reviewFindings": [
    "no blockers in worker targeted validation; fresh parent review remains required"
  ],
  "manualNotes": "No code tests were run because only the implementation plan was edited. No commit or PR was created."
}
```
