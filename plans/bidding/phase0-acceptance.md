# Tender-to-Submission V2 Phase 0 Acceptance

Date: 2026-08-27

Scope: inactive Phase 0 contracts and fixtures only. The active first-launch manifest, queue registry, worker registration, and V1 API path were not switched.

## Executed gates

| Command | Exit | Evidence |
| --- | ---: | --- |
| `python3 scripts/bid_authoring_schema_validation.py` | 0 | Draft 2020-12 validated all 10 frozen schemas; RenderDocumentSnapshotV2 additionally exercised workspace scope, form/preparation occurrences, `ready` status, schema/operation and DOCX/PDF renderer identities, closed asset objects, accepted `quote_snapshot` provenance, and rejected the former `render_font` asset provenance. |
| `scripts/fresh_schema_v2_acceptance.sh` | 0 | Static migration/manifest digests, inactive queue matrix, schema inventory, V2 SQL inventory, monotonic RequirementSet signature, typed-binding trigger, and clean-render constraint passed. |
| `cargo test -p bid --test authoring_schema_contracts --test bidding_v2_baseline_contract` | 0 | 7 passed: 3 schema golden/reference/invariant tests and 4 V2 baseline/fixture tests. |
| `cargo test -p runtime bid_authoring_contract --lib` | 0 | 3 focused contract tests passed: five closed job variants, both content operations, exactly two SubmissionExport modes, stable uniqueness/error codes, and inactive Oxana policy. |
| `cargo test -p domain queue_registry` | 0 | 11 passed, including exact inactive/active registry separation. |
| `cargo test -p storage fixed_manifest_is_exact_and_checksummed` | 0 | 1 passed; active V1 manifest and actual migration digests agree. |
| `RUN_LIVE_V2_SCHEMA=1 scripts/fresh_schema_v2_acceptance.sh` | 0 | Final Phase 0 clean-container run applied knowledge/shared/V2 baselines. It additionally published a coherent text quote and rejected a fresh bundle whose recomputed root hash contained a false `quote_sha256`. All five typed requests, lifecycle guards, export options, and prior Requirement/Render/Evidence/Manifest negatives passed. |
| `python3 -m py_compile scripts/bid_authoring_schema_validation.py` and `sh -n scripts/fresh_schema_v2_acceptance.sh` | 0 | Python and POSIX shell syntax passed. |
| `cargo check -p bid -p runtime` | 0 | Focused production crates compiled after the render contract changes. |
| `cargo fmt --all -- --check` | 0 | Rust formatting passed. |
| `git diff --check` | 0 | No whitespace errors. |
| V1-only checksum/grep scan in the implementation session | 0 | Active manifest ends in `bidding_v1_baseline`; active registry and worker contain no V2 authoring registration. Active and V2 manifests agree on knowledge/shared digests; inactive V2 bidding digest is `75f2f0623823e17af2f2abccfd559e9fd62ef31380f79ef121d6dce9ad8208e7`; fixtures are trackable and no files are staged. |
| `git check-ignore` negation checks for `deploy/authoring-v2/*.toml` | 0 | Both V2 fixtures are trackable and not ignored. |
| `scripts/fresh_schema_acceptance.sh` | 0 | The unmodified active-V1 gate directly applied knowledge/shared/V1 baselines, verified catalog/seed/actor/idempotency/object/ACL contracts, and completed first-launch handoff. |

The mandatory `bid-authoring-v2-phase0` CI job repeats static schema/fixture acceptance, all focused Rust gates, and the live clean-container V2 run with Python 3.13 and `jsonschema==4.25.1`.

## Live negative coverage

The checked-in `scripts/bidding_v2_phase0_live.sql` proves:

- DocumentSet, SourceUnitDispositionSet, RequirementSet, RequirementSupersession, and WorkspaceRequirementProjection each exercise publication advance, deterministic replay, stale/older handling, current-pointer coherence, composite/cross-scope identity rejection, and append-only rejection.
- RequirementSet publishes artifact revision 7 first, replays it, treats revision 3 with the older frozen input tuple as obsolete, then publishes revision 11 with the newer tuple. Its current generation counts publications rather than requiring consecutive artifact revisions.
- OutlineFulfillmentBinding accepts valid same-scope outline-node, response-table lineage with a real table revision, structured-form definition, and QuoteSnapshot targets. It rejects one invalid or cross-project case for every target kind.
- Preview and submission reject non-null watermarks; review draft accepts a watermark. Preview/submission reject omitted mode-option keys through two-valued JSON containment/key checks.
- Two valid WorkspaceRevision tuples with different scope/projection/settings identities prove Outline/Submission Assessment must reference the exact owning tuple; RenderSnapshot must reference both that exact workspace tuple and the matching composite SubmissionAssessment identity. Mixed-but-individually-valid dependencies are rejected while both coherent chains are accepted.
- WorkspaceRevision additionally freezes its RequirementProjection ID+SHA as a composite identity. OutlineCheckpoint references that exact owning tuple: the coherent WorkspaceRevision 2 + projection 2 checkpoint passes, while WorkspaceRevision 2 + another legitimate projection 1 in the same workspace is rejected specifically by the composite foreign key.
- Every generic request must publish exactly one matching typed projection in the same transaction: TenderDocumentProcess freezes document/role/converter; RequirementSetCompile freezes DocumentSet+DispositionSet; OutlineGenerate freezes base workspace, DocumentSet/DispositionSet/RequirementSet/Projection/scope and agent contracts; ContentGenerate freezes base/projection/checkpoint/scope/settings/style, selection, quote and agent contracts; SubmissionExport freezes workspace/checkpoint/projection/scope/settings/style and output options. Missing, wrong and multiple projection attempts fail.
- Generic requests can only be inserted as `pending`; candidates can only be inserted as `proposed` with `decided_at IS NULL`. Direct insertion of every request/candidate terminal state and a pre-decided proposed candidate fails with the exact initial-state `23514` message before any deferred projection check can mask it.
- ContentGenerate manual and system paths freeze the outer selection digest. Manual PickSet binds the exact same-workspace MatchingReport; system mode binds a typed matching-policy contract. Prompt/template/model/agent IDs+SHAs are mandatory, target/fill/anchor predicates are two-valued, workspace target revision is required, and ContentCandidate requires the exact `generate` typed request/base tuple. NULL, mixed-mode, wrong-policy/report, invalid fill/target/anchor and match-only candidate cases fail.
- Form-definition occurrences accept only the frozen revision ID plus matching canonical SHA. Attachment-preparation occurrences additionally require the status-qualified `ready` identity: matching `pending` and `failed` revisions are rejected. DOCX/PDF renderer IDs and digests must reference separately approved contracts.
- SubmissionExport request mode options use exactly `watermark|include_assessment_notices|include_knowledge_sources`, enforce JSON null/string/boolean types and a 1–128 byte draft watermark, and require submission watermark null plus both flags false. Valid review-draft and submission requests pass; extra/missing keys, each wrong type, empty draft watermark and either submission true flag fail.
- Manifest creation rejects preview snapshots and any mode, format, or mode-options mismatch; output creation rejects a format different from its Manifest.
- PostgreSQL readiness waits for the completed-init entrypoint marker, the final postmaster, and all five bootstrap roles rather than the early API-role sentinel.
- WorkspaceRevision parent, WorkspaceHead, and Candidate base references are composite artifact ID+content SHA identities; wrong parent/head/base digest DML is rejected.
- SubmissionExport serialization accepts exactly `review_draft|submission`; a `preview` export payload is rejected while preview remains valid only for the render snapshot/API preview contract.
- MatchingReport freezes workspace+requirement and a real knowledge-owned attestation. EvidenceBundleV1 enforces RFC3339 lexical date-time plus exact row instant, rejecting date-only, `infinity`, invalid offsets and invalid dates with recomputed hashes. Text evidence requires `quote_sha256` to equal SHA-256 of the exact UTF-8 `quote_utf8` bytes; coherent publication and a fresh wrong-digest rejection both run live. Fresh bundle/item/asset fixtures prove MIME, width, height, page and bounds must exactly equal the immutable KnowledgeImageArtifactRevision and ObjectRegistry tuple; no uniqueness error is accepted as a substitute.
- Phase 0 creates only the inactive knowledge media storage identity and same-Document/ProductVersion `image_ocr` chunk mapping. The V2 baseline adds the ObjectRegistry composite FK after shared has loaded. Valid media mappings pass; unknown revision, wrong digest/MIME and non-OCR source mappings fail. No V3 retrieval/publication behavior or second port exists.
- Every frozen media identity uses shared ObjectRegistry object_ref+digest+media_type+available state. Render assets additionally require the owning `knowledge_evidence|manual_workspace|prepared_attachment|quote_snapshot` revision; prepared pages and Quote canonical bytes use immutable owner mappings rather than a second registry.
- RenderSnapshot stores closed canonical JSON with hash over `payload - snapshot_sha256`; correlates workspace/checkpoint/projection/settings/assessment digests; and exactly matches ordered node/block/font/asset/form/preparation projections at transaction end. Unknown fields/UUID/provenance, duplicate occurrences, out-of-range geometry, wrong hash/MIME/revision, missing/extra/reordered projections, and update/delete/truncate all fail. The persisted payload passes Draft 2020-12.
- AttachmentPreparation binds a same-workspace manual source asset, hashes closed canonical JSON over source/status/ordered page objects and geometry, and requires an exact deferred relational projection. Unknown source, missing/extra/reordered pages, wrong digest/geometry/hash and update/delete/truncate fail.
- SubmissionOutput requires an available ObjectRegistry object_ref+digest+MIME+length tuple and an exact `bid_submission_output` owner occurrence keyed by output/project/workspace/manifest. Unknown, wrong digest/media/length, unavailable and missing-owner fixtures fail.
- Manifest dependency kinds are closed and relationally checked against the frozen snapshot. The acceptance oracle is a literal independent 19-tuple inventory containing exact ordinals, kinds, IDs and SHAs; it never invokes the production expected-set helper. Zero/missing/unknown/wrong-SHA sets fail, and the wrong-digest case uses a fresh Manifest with only `check_violation` accepted.

## Review status

Implementation gates and the final independent quote-digest review pass. Phase 0 is closed; Phase 1 implementation may proceed.
