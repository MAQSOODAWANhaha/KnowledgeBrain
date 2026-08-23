Implemented the final targeted round-2 contract revision for `plans/bid-platform-complete-solution.md`.

## Changed file

- `plans/bid-platform-complete-solution.md`: now 1,273 lines / 138,582 bytes, SHA-256 `d56f4af8b7d34b66d963b3a3f399c48cc0104d89382475c57dda0c6909aa7673`.
- No code, `PRODUCT.md`, domain documentation, migration, runtime-completion, or deployment file was edited.

## Contract fixes

1. Added persistent procedural segment-member UUIDs and an exact seven-key `GateIssueV1` locator/NULL matrix, including classification-missing, stale, review, and decision-missing cases.
2. Normalized `ProceduralSegmentSetV1` into immutable header/member authority with member uniqueness, deterministic payload reconstruction, hash verification, and classification-to-member composite FK.
3. Replaced ambiguous extracted origin identity with source and section artifact IDs plus project/document/generation/boundary/digest scalars, exact 0015 composite FKs, provenance NULL rules, and a deferred scope verifier. Unedited extracted spans must equal origin spans exactly.
4. Defined `BEFORE UPDATE` immutability trigger allowlists for classification and decision history; semantic changes insert higher-revision successors.
5. Fixed matching `product_ordinal` provenance to `bid_match_route_product_versions.ordinal` and required route-membership verification.
6. Completed `OpenStagingSetV1`, all six `StageRouteBatchV1` collection schemas, bounded staged `chunk_utf8`, exact stage receipts/errors, and replay-before-mutable-state validation.
7. Completed compact `CommitRouteV2`, exact commit receipt/error behavior, typed idempotency framing, totals/hash verification, and successful-response-loss replay.
8. Added exact `matching_reaper_v1` and `staging_cleanup_v1` actor/request/receipt/no-op/mismatch semantics while preserving heartbeat as the only receipt-free CAS mutation.
9. Made `bid_object_registry + bid_object_references` the sole object authority, explicitly dropping `content_objects` after full-path cutover. Added digest/ref grammar, normalized references, deletion fencing, durable outbox claim/heartbeat/reclaim/fail/retry, and stable rejection of retired digests.
10. Added manifest-owned generic Markdown image assets alongside BidShot assets, with canonical Markdown AST parsing, normalized manifest relations, registry references, bounded media validation, and no render-time live blob scan.
11. Added independent retention role/login, credentials, runtime consumer, private physical-delete seam, legacy ACL revocations, and fresh allow/deny matrices.
12. Added 0015 KindRouter and 0018 ProceduralRouter/template immutable contract artifacts, unique current pointers, seed ownership, maintenance promotion transaction, stale propagation, and old-version export rejection.
13. Resolved profile canonical identity: Company/Submission profiles are standalone objects; `CanonicalSetV1` only wraps real sets.
14. Assigned canonical operation names to exact migration slices; 0016 now owns fact operations only and 0018 owns profile/submission/retention operations.
15. Required PR6 to remove legacy GET export, `regenerate_stale`, and save/regenerate calls without expected revision/hash CAS; replacement manifest endpoints and old-route rejection are explicit.
16. Fixed the fresh runner to the repository CI image `pgvector/pgvector:0.8.6-pg16@sha256:ccc6e83d6e35e931dc7c5def2022729d5a6c370318d099181995567ff1fb4d6b`, including vector-extension verification and cleanup traps.
17. Synchronized schema slice ownership, ACL requirements, PR0/PR1/PR4/PR6 outputs and removed seams, stale dependency rows, mandatory tests, and completion criteria.
18. Preserved the approved clean-slate boundary: no history migration, old-binary compatibility, runtime topology completion, Org, multi-tenancy, automated quote, currency conversion, or extra matching route.

## Validation and exit codes

1. `git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md`
   - Native exit 1 because the plan is untracked and differs from `/dev/null`; no diagnostics. Normalized whitespace-check exit 0.
2. Python direct whitespace/newline checker
   - Exit 0: 1,273 lines, 138,582 bytes, no trailing whitespace or tabs, final newline present.
3. Initial naive Markdown table checker
   - Exit 1 because it split the escaped `technical\|commercial` code span as a table delimiter. This was a checker defect; no plan change resulted from that failure.
4. Corrected fence/backtick/escape-aware Markdown table checker
   - Exit 0: 13 tables, consistent column counts, balanced fenced blocks.
5. Python targeted contract/stale assertion checker
   - Exit 0: 17 required round-2 markers present; three stale forms absent (`extracted_origin_artifact_id`, `match_stage_route_batch_v1`, `match_commit_route_v2`).
6. Scoped status and staged-file check
   - Exit 0: plan remains `??`; staged-file count is 0.
7. `sha256sum` / `wc`
   - Exit 0: digest and size reported above.

No code tests were run because this task changed only the implementation plan.

## Residual risks

- SQL/Rust/API/Web/CI work remains unimplemented and must be delivered through PR0–PR7.
- `PRODUCT.md` and domain documentation remain intentionally unchanged until PR0.
- The plan is still untracked and uncommitted.
- A fresh-context final round-3 review is still required; this worker did not substitute self-review for that gate.

## Commit / PR state

- No commit created.
- No PR created.
- No staged files.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "All round-2 procedural, staging/commit, object-retention, Markdown asset, router/template, profile, operation ownership, legacy API and fresh-runner contract findings were incorporated into plans/bid-platform-complete-solution.md."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "Only the requested plan was edited as product content; no code, PRODUCT.md, domain documentation, migration, runtime-completion or deployment file was changed."
    },
    {
      "id": "criterion-3",
      "status": "satisfied",
      "evidence": "Whitespace, corrected Markdown table consistency, targeted required/stale contract assertions, scoped status and no-staged checks all passed."
    }
  ],
  "changedFiles": [
    "plans/bid-platform-complete-solution.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "git diff --no-index --check /dev/null plans/bid-platform-complete-solution.md",
      "result": "passed",
      "summary": "Native exit 1 was expected for an untracked file differing from /dev/null; diagnostics were empty and the normalized whitespace result was 0."
    },
    {
      "command": "Python whitespace/newline checker",
      "result": "passed",
      "summary": "Exit 0; 1273 lines, 138582 bytes, no trailing whitespace/tabs, final newline present."
    },
    {
      "command": "Initial naive Markdown table checker",
      "result": "failed",
      "summary": "Exit 1 due to incorrectly treating an escaped pipe inside a code span as a table delimiter; this was a checker defect."
    },
    {
      "command": "Corrected escape/backtick-aware Markdown table checker",
      "result": "passed",
      "summary": "Exit 0; 13 tables had consistent column counts and fenced blocks were balanced."
    },
    {
      "command": "Python targeted contract and stale assertions",
      "result": "passed",
      "summary": "Exit 0; 17 required contract markers were present and 3 stale spellings were absent."
    },
    {
      "command": "Scoped git status and git diff --cached --name-only",
      "result": "passed",
      "summary": "Exit 0; scoped plan status is untracked and staged-file count is zero."
    },
    {
      "command": "sha256sum and wc",
      "result": "passed",
      "summary": "Exit 0; SHA-256 d56f4af8b7d34b66d963b3a3f399c48cc0104d89382475c57dda0c6909aa7673, 1273 lines, 138582 bytes."
    }
  ],
  "validationOutput": [
    "noindex_native_exit=1 diagnostics_bytes=0 normalized_exit=0",
    "whitespace_ok lines=1273 bytes=138582",
    "tables_ok count=13",
    "contract_assertions_ok required=17 forbidden=3",
    "sha256=d56f4af8b7d34b66d963b3a3f399c48cc0104d89382475c57dda0c6909aa7673",
    "scoped status: ?? plans/bid-platform-complete-solution.md",
    "staged_count=0"
  ],
  "residualRisks": [
    "SQL, Rust, API, Web and CI implementation remains for PR0-PR7.",
    "PRODUCT.md and domain documentation still require PR0 alignment.",
    "The plan remains untracked and uncommitted.",
    "Fresh-context round-3 review remains required."
  ],
  "noStagedFiles": true,
  "diffSummary": "Expanded the plan to 1273 lines to close persistent procedural locators and origin FKs, normalized segments, exact staging/commit/reaper wires, single object authority and retention runtime, Markdown manifest assets, current router/template contracts, canonical operation ownership, legacy export removal, and fixed pgvector fresh-schema validation.",
  "reviewFindings": [
    "No blocker found in worker self-check; independent final review remains required."
  ],
  "manualNotes": "No code tests were run because only the plan was edited. No commit or PR was created."
}
```
