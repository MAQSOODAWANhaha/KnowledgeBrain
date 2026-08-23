Implemented the two requested, architecture-preserving clarifications in `plans/bid-platform-complete-solution.md`.

## Changed files

- `plans/bid-platform-complete-solution.md`: targeted Issue 6/9 wording and existing acceptance-gate synchronization only; remains 1,334 lines, now 164,824 bytes; SHA-256 `b105d6bf8e230404ea81ec756d0e88d00a73b2f1de72099d6add90bc8bc7b99c`.
- `subagent-results/bid-plan-issue6-9-clarification.md`: this required execution report.
- No code, `issue.md`, `PRODUCT.md`, domain documentation, migration, deployment, or runtime-completion file was edited.

## Clarification mapping

### Issue 6 — explicit KindRouter hard invalidation

- Preserved `SourceSpanV2` and the existing extracted `stable_span_key` formula including `router_version`.
- Added an explicit statement at the key formula that KindRouter contract promotion is intentional **hard semantic invalidation**, not same-key reclassification: every unedited extracted routed segment receives a new key because the version frame changes.
- Closed procedural lifecycle behavior:
  - if the promoted clause remains procedural, the old extracted classification and current decision terminate without successors using the existing `segment_removed` reason;
  - if it leaves procedural, the existing higher-priority `left_procedural` reason applies;
  - a still/newly procedural new key must be classified again and the user must review/confirm again;
  - UI text is fixed to `KindRouter版本升级，程序决定需重新确认` and formal PDF is rejected until reconfirmation;
  - no same-key successor may disguise this invalidation.
- Preserved manual/manual_after_edit key behavior: the key does not change merely because KindRouter version changes; actual clause-kind changes still follow existing reroute/terminal rules.
- Synchronized the stale graph, PR3 exit gate and mandatory Router fixture with the exact hard-invalidation path.

### Issue 9 — single worker/runtime source of truth

- Removed the ambiguous `不重做 worker` wording.
- Baseline and §6 now state one contract consistently:
  - retain existing `schedule_dirty_project`, v1 job table/claim scheduling and worker process shell;
  - do not create a second scheduler/job/worker;
  - necessarily refactor the same worker's route execution/commit runtime to `OpenStagingSetV1 + StageRouteBatchV1 + heartbeat + CommitRouteV2 + cleanup/reaper`;
  - delete the old single-JSON `CommitRoute` execution path.
- Synchronized the PR1 matrix outputs, removed seams and exit gate to require the same worker protocol and repository-wide absence of old single-JSON commit calls.

## Validation

1. `git diff --check -- plans/bid-platform-complete-solution.md`
   - Exit 0.
2. `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`
   - Native exit 1 because the plan is untracked and differs from `/dev/null`; diagnostics were 0 bytes, so the normalized whitespace check passed.
3. Direct Python whitespace/newline/targeted-contract/table validator
   - Exit 0: 1,334 lines, 164,824 bytes, 13 consistent Markdown tables, balanced fences, 9 required clarification markers, 0 stale ambiguous worker phrases.
4. Scoped status and staged-file check
   - Exit 0: only the pre-existing untracked `issue.md` and plan appear in scoped status; staged count is 0.
5. `sha256sum plans/bid-platform-complete-solution.md`
   - Exit 0: `b105d6bf8e230404ea81ec756d0e88d00a73b2f1de72099d6add90bc8bc7b99c`.

No code tests were run because this task changed only plan wording and its existing acceptance gates.

## Residual risks

- SQL/Rust/API/Web/CI implementation remains for PR0–PR7 and must prove the fixtures now required by the plan.
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
      "evidence": "Only plans/bid-platform-complete-solution.md was changed as product content. Issue 6 now explicitly preserves router_version in extracted keys while defining promotion as hard invalidation with exact segment_removed/left_procedural, reconfirmation, UI and PDF semantics. Issue 9 now consistently preserves only the scheduler/job/worker process shell while requiring the same worker to adopt Open/Stage/heartbeat/CommitV2/cleanup/reaper and delete old single-JSON CommitRoute. No other architecture or issue was changed."
    }
  ],
  "changedFiles": [
    "plans/bid-platform-complete-solution.md",
    "subagent-results/bid-plan-issue6-9-clarification.md"
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
      "command": "Python whitespace/newline/targeted-marker/fence/table validator",
      "result": "passed",
      "summary": "Validated 1334 lines, 164824 bytes, 13 tables, 9 required markers and zero stale ambiguous worker phrases."
    },
    {
      "command": "git status --short -- scoped paths && git diff --cached --name-only",
      "result": "passed",
      "summary": "Only pre-existing untracked issue.md and the plan appeared; staged count was zero."
    },
    {
      "command": "sha256sum plans/bid-platform-complete-solution.md",
      "result": "passed",
      "summary": "SHA-256 b105d6bf8e230404ea81ec756d0e88d00a73b2f1de72099d6add90bc8bc7b99c."
    }
  ],
  "validationOutput": [
    "validation_ok lines=1334 bytes=164824 tables=13 required=9 stale=0",
    "ordinary_exit=0 noindex_native=1 noindex_diagnostics_bytes=0",
    "sha256=b105d6bf8e230404ea81ec756d0e88d00a73b2f1de72099d6add90bc8bc7b99c",
    "scoped status: ?? issue.md; ?? plans/bid-platform-complete-solution.md",
    "staged_count=0"
  ],
  "residualRisks": [
    "SQL/Rust/API/Web/CI implementation and the specified promotion/runtime fixtures remain for PR0-PR7.",
    "The plan and issue.md remain untracked and uncommitted."
  ],
  "noStagedFiles": true,
  "diffSummary": "Two narrow clarifications: explicit KindRouter-promotion hard invalidation for extracted procedural identities, and an unambiguous PR1 contract that preserves the existing scheduler/job/worker shell while replacing the same worker's old single-JSON route commit runtime.",
  "reviewFindings": [
    "no blockers in targeted validation"
  ],
  "manualNotes": "No code tests were run because only the implementation plan was edited. No commit or PR was created."
}
```
