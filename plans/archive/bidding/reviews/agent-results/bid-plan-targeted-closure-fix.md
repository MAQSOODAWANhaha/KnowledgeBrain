Implemented the bounded round-3 contract closure in `plans/bid-platform-complete-solution.md` without widening product scope.

## Changed files

- `plans/bid-platform-complete-solution.md`: targeted contract edits only; now 1,307 lines / 151,206 bytes; SHA-256 `615f38dbdcb1caa53c4a8b1240f3f3caed126315eb6241ff4461ac61adb2c90d`.
- No code, `PRODUCT.md`, domain documentation, migration, deployment, or runtime-completion file was edited.

## Fix mapping

### A. Procedural / dependency

1. Added exact classification and decision terminal fields to DDL and canonical items: `terminal_reason`, `terminal_at`, and `terminal_actor_identity`. Fixed successor-XOR-terminal NULL matrices, canonical actor/time encoding, same-key higher-revision successor rules, and deterministic no-successor reason precedence for text changes, resegmentation, removed segments, clause unconfirm/delete, leaving procedural, and decision `classification_superseded`.
2. Added minimal `MatchingReportSetV1` only for `6:implementation_plan`: exact item fields, route/unit/report ordering and uniqueness, project revision/hash, current report verifier, empty hash, promotion/retirement stale propagation, and PartDependency `matching_report` versus `matching_report_set` NULL matrix. No route kind was added; the set only aggregates existing technical routes.
3. Added `schema_version` and `canonical_payload` to the ProceduralSegmentSet header DDL. Member UPDATE/DELETE/TRUNCATE is rejected; header UPDATE only permits `current→superseded` with all other fields unchanged, and DELETE/TRUNCATE is rejected. ACL and negative tests were synchronized.
4. Completed GateIssue locators: `NOT_APPLICABLE` uses the current segment/current classification and current decision reason; STALE deterministically selects the highest historical classification revision for the same project/clause/stable key and is verifier-checked.

### B. Matching staging

1. Unified Open/Stage/Commit replay semantics: a completed same-hash call returns the first immutable receipt bytes, without a replay result. Success results are only `opened`, `accepted`, and `committed`. Open now fixes schema version, actor, canonical JSON, hash coverage and deterministic error replay. Stage `collection_bytes` is exactly the canonical UTF-8 byte length of `items`.
2. Prevented reaper no-op poisoning: fresh active leases and not-yet-expired sets return non-persistent `not_due` observations and do not write completed idempotency rows; stale transitions and genuinely terminal observations have distinct durable receipts.
3. Defined terminal staging cleanup: every consumed/failed/expired transition purges all six typed staging child collections and releases project active-set/row/chunk/evidence reservations exactly once in the same transaction. Only bounded headers/totals/receipts remain; receipt replay cannot release counters again.

### C. Assets / render

1. Removed the render-artifact natural uniqueness contract. Every BidShot mutation and Markdown occurrence gets a new artifact identity, including A→B→A and repeated manifests; the object registry alone deduplicates content.
2. Split `source_placement_ordinal` from `manifest_ordinal`. Manifest ordinals are contiguous across all occurrences in RequiredPartSet order; part 3 explicitly emits BidShots in source placement/shot order before its Markdown image occurrences. Payload, relation PK/unique constraint, renderer and verifier use manifest ordinal consistently.
3. Added an exact object-reference owner matrix. `kb_document`, `bid_document`, converted source, shot, attachment, render artifact and manifest relation each have a precise owner/project rule and composite FK or deferred verifier. Manifest asset references use independent relation UUIDs per occurrence, so repeated references to one object remain representable.
4. Fixed the minimal deterministic Markdown contract: fixed parser artifact/version/options, exact `objects/<sha>` destination grammar, alt-to-caption and width default/token rules, AST occurrence index and node path, frozen locator fields in artifact/relation/manifest, and renderer reparse-and-replace behavior with no live or regex fallback.

### D. Export / template / idempotency / delivery

 1. Added `output_format=docx|pdf` to the formal-export exact request, canonical manifest input, immutable manifest row and first receipt. DOCX warning semantics, PDF hard gate, MIME, extension, frozen filename, retry and download behavior are now format-bound. No new format was introduced.
 2. Replaced per-runtime-part template current rows with fixed slots (`2:unit`, `2:unsectioned`, and fixed part slots), exact actual-part-to-slot mapping, composite artifact/current FK, slot-based promotion stale propagation, and customer-data-free seed rules.
 3. Replaced the partial idempotency list with one complete shared-operation registry grouped by 0013/0015/0016/0017/0018. Exact extraction/matching claim names are fixed. Attachment replace/delete/reject are distinct from upload/validate/confirm. Formal-export format is hashed. Retention claim/reclaim/succeed/fail now have exact request, receipt, error and first-receipt replay contracts; heartbeat remains receipt-free CAS. The private unreferenced-object scheduler explicitly shares its enclosing domain receipt and is not a separate wire operation.
 4. Unified fresh-runner identity timing: PR1 creates only migrator/verifier/API/worker identities; PR6 adds retention login, password environment, bootstrap wiring, consumer and allow/deny ACL verification.

Schema slice ownership, DB/ACL constraints, PR0/PR1/PR6 delivery rows, forced tests and completion criteria were updated only where needed by these fifteen fixes. The approved single-company, clean-slate, CNY manual quote, two-format and existing-route boundaries remain unchanged.

## Validation

1. `git diff --check -- plans/bid-platform-complete-solution.md`
   - Exit 0.
2. `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`
   - Native exit 1 because the plan is untracked and differs from `/dev/null`; diagnostics were 0 bytes, so normalized whitespace validation passed.
3. Direct Python whitespace, newline, targeted contract/stale marker, fenced-block and escape/backtick-aware Markdown table checker
   - Exit 0: 19 targeted contract groups, 13 consistent tables, 1,307 lines, 151,206 bytes.
4. Targeted stale scan
   - Exit 0: no replay-result spelling, old render natural unique, old part-key template current, PR1 retention-role list, ambiguous origin artifact or old implementation-plan single-report dependency.
5. `git diff --no-index --stat /dev/null plans/bid-platform-complete-solution.md`
   - Native exit 1 as expected for an untracked file; stat reports one plan file and 1,307 inserted lines relative to `/dev/null`.
6. Scoped status and staged-file check
   - Exit 0: `?? plans/bid-platform-complete-solution.md`; `PRODUCT.md` and domain docs absent from scoped status; staged count 0.

## Residual risks

- This task changed only the plan. SQL, Rust, API, Web and CI implementation remains for PR0–PR7.
- Exact SQL/Rust canonical-byte, trigger, receipt, lease, asset-locator and ACL behavior must still be proven by the implementation tests now listed in the plan.
- The plan remains untracked and uncommitted.

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
      "evidence": "Only plans/bid-platform-complete-solution.md was edited. The fifteen closed round-3 findings were addressed across procedural terminal history, multi-report dependency, immutable segments, GateIssue locators, exact staging replay/reaper/cleanup, occurrence assets, deterministic Markdown, format-bound exports, fixed template slots, complete shared operation ownership and PR6 retention timing without adding routes, formats, tenancy, automated pricing, migration compatibility or deployment scope."
    }
  ],
  "changedFiles": [
    "plans/bid-platform-complete-solution.md"
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
      "command": "Python whitespace/newline/targeted contract/stale/table checker",
      "result": "passed",
      "summary": "Validated 19 contract groups, 13 Markdown tables, balanced fences, 1307 lines and 151206 bytes."
    },
    {
      "command": "targeted grep stale scan",
      "result": "passed",
      "summary": "No obsolete replay result, natural artifact unique, dynamic part-key template current, PR1 retention role, ambiguous origin, or single-report implementation dependency marker."
    },
    {
      "command": "git status --short -- scoped paths && git diff --cached --name-only",
      "result": "passed",
      "summary": "Only the untracked plan appears in scoped status; staged count is zero."
    }
  ],
  "validationOutput": [
    "ordinary=0",
    "noindex_native=1 noindex_diagnostics=0",
    "contract_groups=19 tables=13 lines=1307 bytes=151206",
    "sha256=615f38dbdcb1caa53c4a8b1240f3f3caed126315eb6241ff4461ac61adb2c90d",
    "scoped status: ?? plans/bid-platform-complete-solution.md",
    "staged_count=0"
  ],
  "residualRisks": [
    "SQL/Rust/API/Web/CI implementation and cross-layer tests remain for PR0-PR7.",
    "The plan remains untracked and uncommitted."
  ],
  "noStagedFiles": true,
  "diffSummary": "Targeted closure of the fifteen round-3 issues: exact terminal history and report sets, immutable segment authority and GateIssue locators, immutable first receipts and safe reaper/resource cleanup, occurrence-based asset/reference/Markdown rendering, output-format identity, fixed template slots, complete shared operation ownership and PR6-only retention identity wiring.",
  "reviewFindings": [
    "no blocker found in targeted worker validation; independent bounded review remains the parent gate"
  ],
  "manualNotes": "No code tests were run because only the implementation plan was edited. No commit or PR was created."
}
```
