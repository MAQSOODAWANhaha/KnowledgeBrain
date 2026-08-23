# Bid Extractor Round 2 — Schema and Contract Review

## Review scope

Fresh-context, read-only inspection of current migration, storage, worker, API, UI, deployment, documentation, evaluator, tests, and all Round-1 reports.

No files were edited. Shell/Git execution was unavailable, so SQL application, Cargo tests, web build, compose validation, and staged-file state could not be independently executed.

## Blockers / approval decisions

### Blocker — High: existing databases do not receive the new lease columns

**Evidence**

- The new project lease columns are present only inside `CREATE TABLE IF NOT EXISTS bid_projects` (`migrations/0007_bid.sql:3-19`).
- `bid_extract_runs.claim_token` is likewise present only in its original table declaration (`migrations/0007_bid.sql:52-71`).
- Startup replays `0007_bid.sql` instead of applying a new upgrade migration (`crates/storage/src/persist.rs:25-44`). `CREATE TABLE IF NOT EXISTS` does not add columns to an existing table.
- Worse, `connect()` discards any migration error and returns a usable pool (`crates/storage/src/persist.rs:19-22`). A worker can therefore start against an old schema and fail later in `claim_extract_run`.
- Persistent Postgres volumes are part of the normal deployment (`deploy/docker-compose.yml:47-57`).

**Impact**

Any database created before these fixes lacks `extract_lock_token`, `extract_lock_kind`, `extract_lock_at`, and `claim_token`. Extraction claims then fail at runtime, while APIs may still return success because enqueue/storage errors are ignored.

**Required fix**

Add a new idempotent upgrade migration using `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`, normalize existing running rows, add the lease checks/indexes after normalization, and propagate migration failure from `connect()`. Do not rely on editing the historical `CREATE TABLE IF NOT EXISTS`.

### Blocker — High: stale section-retry recovery is not fenced and can corrupt newer state

**Evidence**

- Section retry records only a project token/kind/timestamp; it does not associate the lease with a section or heartbeat (`crates/storage/src/bid.rs:244-263`).
- Housekeeping clears every old `section_retry` project lock based solely on its original `extract_lock_at` (`crates/storage/src/bid.rs:373-380`). It does not update the corresponding section’s `running` state.
- No heartbeat updates either `extract_lock_at` or run `started_at`; those fields are written only when initially claimed (`crates/storage/src/bid.rs:299-325`).
- After lease loss, the old retry still calls unfenced `set_section_status(..., "failed", ...)` on engine or persistence failure (`crates/bid/src/lib.rs:1268-1284`; `crates/storage/src/bid.rs:1191-1203`). A newer retry may already own the project or have completed.
- `finish_section_retry` does not check `rows_affected`, so lease loss can be reported as success (`crates/storage/src/bid.rs:265-280`).

**Impact**

A long-running retry may be declared stale, a newer retry may claim the project, and the old worker may subsequently mark the same section failed. Stale recovery also leaves orphaned `bid_sections.extract_status='running'` rows.

**Required fix**

Fence section status updates with a section-level lease token, associate the project lease with the section, add heartbeat/lease renewal, and make stale recovery transition the owned section to a recoverable terminal/pending state. Require one affected row when releasing or mutating under a lease. Add a two-owner PostgreSQL test covering stale retry, new retry, and late old-owner failure.

**Approval decision:** no product decision is needed. The current state is not approved until these implementation blockers are fixed.

## Fixes worth doing now

### High: retry/delete document routes do not validate route ownership

**Evidence**

- `retry_bid_doc` validates that path project `id` is open, but updates `did` without checking `bid_documents.project_id=id` (`crates/api/src/routes.rs:3817-3830`; `crates/storage/src/bid.rs:190-217`).
- `delete_bid_doc` has the same issue (`crates/api/src/routes.rs:3801-3814`; `crates/storage/src/bid.rs:1051-1057`).
- Unknown document IDs also return success because affected-row counts are ignored.

**Impact**

`POST /bids/A/documents/B/retry` or the delete equivalent can mutate a document belonging to another project, and callers cannot distinguish missing/mismatched documents from success.

**Fix**

Use ownership-conditioned storage operations (`WHERE id=$did AND project_id=$id`), require one affected row, and return 404 for missing/mismatched documents.

### High: API routes return success when no operation was persisted or queued

**Evidence**

- Section retry returns `202 Accepted` when database connection fails because all work is inside `if let Ok(pool)` (`crates/api/src/routes.rs:3833-3851`).
- Full re-extraction ignores both `insert_extract_run` and enqueue failures, then returns 202 (`crates/api/src/routes.rs:3854-3868`).
- Document retry similarly ignores status-update and enqueue errors (`crates/api/src/routes.rs:3817-3830`).
- Runtime enqueue itself returns `Ok(None)` when queue storage cannot connect (`crates/runtime/src/jobs.rs:717-738`).

**Impact**

The UI can display “已重新抽取/已重试” although no durable work exists. A pending run can also be inserted without a queue job.

**Fix**

Require database connection, propagate insert failures, and treat absent/failed enqueue as a server/service-unavailable error. Prefer an outbox or housekeeping-backed durable contract and return a run identifier.

### Medium: section retry maps internal failures to HTTP 400

**Evidence**

`retry_bid_section` maps only `project_extraction_running` to 409 and `section_project_mismatch` to 404; all other errors become validation failures (`crates/api/src/routes.rs:3833-3850`). Those other errors include:

- `"section missing"` from `crates/bid/src/lib.rs:1182-1186`;
- extraction configuration failures at `crates/bid/src/lib.rs:1195`;
- SQL failures during lookup/claim;
- `"section_retry_persist_failed"` at `crates/bid/src/lib.rs:1267-1279`.

**Impact**

Missing resources and server/database/model configuration failures are reported as client mistakes, defeating retry logic and observability.

**Fix**

Introduce a typed retry error enum and map missing to 404, lock conflict to 409, invalid request to 400, and storage/configuration/persistence failures to 500 or 503.

### Medium: queued extraction can start after a project has ended

**Evidence**

- Mutating API routes call `require_open_project`, which is correct at request time (`crates/api/src/routes.rs:3608-3620`, `:3840`, `:3861`).
- The actual worker claim locks the project but does not require `status='open'` (`crates/storage/src/bid.rs:283-317`).
- Housekeeping also re-enqueues pending/stale runs without checking project status (`crates/worker/src/consume.rs:1451-1468`).

**Impact**

A run queued before manual/automatic project ending can later supersede drafts in an ended project, bypassing the route-level read-only gate.

**Fix**

Require an open project during claim, and decide whether ending a project cancels pending runs or marks them failed/cancelled. Add an end-versus-claim concurrency test.

### Medium: claim-token lifecycle is application-enforced only

**Evidence**

- `claim_token` is nullable with no status-coupled check (`migrations/0007_bid.sql:52-71`).
- Application transitions are coherent: pending insert has no token (`crates/storage/src/bid.rs:408-423`), claim writes token (`:299-317`), stale reclaim clears it (`:352-359`), and finish clears it (`:426-456`).
- The database nevertheless permits `running` with a null token or terminal/pending rows with a non-null token.

**Impact**

Manual SQL, interrupted upgrades, or future callers can create an unfenceable running row that holds the partial unique index and cannot be completed normally.

**Fix**

After legacy normalization, add a check coupling `status='running'` to a non-null token and all other statuses to a null token. Add DB tests for rejected invalid combinations.

### Medium: retry UX does not expose the section-retry contract

**Evidence**

- The backend route exists at `crates/api/src/routes.rs:189-192`.
- `web/src/api.ts:181-240` has document retry and full re-extraction methods, but no section-retry method.
- Match-unit responses omit section extraction status (`crates/bid/src/lib.rs:134-142,159-209`), and TypeScript has no `extract_status` field.
- The UI notice tells users to retry after failed or uncovered extraction (`web/src/bid/Workbench.tsx:198-207`), but a retry button appears only when the current clause table is empty (`web/src/bid/ClauseTable.tsx:82-115`) or the entire project has no clauses (`web/src/bid/Workbench.tsx:455-475`).

**Impact**

A failed section cannot be retried through the UI, and a partial project failure with retained old clauses displays retry advice without an available retry action. This conflicts with `docs/bid-platform-domain.md:250,257,527-529`.

**Fix**

Expose section status/error in the units API, add a typed `retrySection` client method and action, and provide a global re-extract action on failed-run notices even when old clauses remain.

### Medium: invalid clause enums still become internal errors

**Evidence**

- PATCH accepts unrestricted strings for family, status, and assessment (`crates/api/src/routes.rs:3912-3930`).
- Storage accepts the same fields as `&str` (`crates/storage/src/bid.rs:30-41`) and relies on database checks.
- Database violations are mapped to 500 (`crates/api/src/routes.rs:3993-4010`).
- The TypeScript types remain unrestricted strings (`web/src/api.ts:49-66`).

**Impact**

Invalid client values are reported as server failures rather than 400 validation errors; stringly contracts remain prone to drift.

**Fix**

Validate or deserialize into enums at the API boundary and use TypeScript string unions.

## Optional / deferred

### Low: one extraction policy setting is dead

`table_rows_per_span` remains in the policy type and JSON (`crates/bid/src/extraction/policy.rs:49-66`; `crates/bid/config/cn-tender-v2.json:36-47`) but has no runtime caller. Tables are currently split one data row per span (`crates/bid/src/extraction/outline.rs:139-170`).

Remove the setting or intentionally use it. The current dead field conflicts with the claim that all limits in the policy are adjustable behavior.

### Low: deployment documentation names a nonexistent migration

`deploy/README.md:25-28` says startup applies `0001`–`0009`; the repository and storage constants stop at `0008` (`crates/storage/src/persist.rs:4-11`).

### Low: evaluator still has a narrow semantic/atomicity blind spot

The evaluator now uses one-to-one assignment and correctly measures precision, recall, false positives, duplicates, family/must accuracy, and threshold exit status (`crates/bid/src/extraction/evaluation.rs:99-300`; `crates/bid/src/bin/bid_extract_eval.rs:8-63`). Matching still uses normalized quote containment (`crates/bid/src/extraction/evaluation.rs:120-137`), so a broad source-grounded quote containing one expected requirement plus unrelated text can be assigned without a separate atomicity penalty. This is suitable as a deferred quality improvement, not a current storage/API blocker.

### Low: conflict confirmation wording and behavior remain inconsistent

The Inspector says users must choose the correct classification before confirming (`web/src/bid/Inspector.tsx:72-88`), but confirmation handlers still submit only `{status:"confirmed"}` (`web/src/bid/Workbench.tsx:360,526,564`). Storage clears `family_conflict` on any confirmation (`crates/storage/src/bid.rs:754-778`). Either enforce an explicit classification action or clarify that confirming the suggested family is allowed.

## Integration-test gaps

Existing DB-focused coverage is materially improved:

- Fresh setup drops all tables and reapplies migrations (`crates/storage/src/persist.rs:3688-3710`).
- Simultaneous full-run claims, stale full-owner persistence/finish fencing, and full-vs-section-retry exclusion are tested (`crates/storage/src/persist.rs:3760-3858`).
- Document transaction rollback is tested (`crates/storage/src/persist.rs:3860-3960`).

Remaining gaps:

1. Upgrade from a pre-lock `0007` schema.
2. Assertions for all three project lock columns, `claim_token`, lifecycle checks, partial unique index, and supporting indexes.
3. Stale section retry followed by a new owner and late old-owner status mutation.
4. Heartbeat/long-running full extraction and section retry.
5. Project ending concurrently with claim.
6. Retry-document ownership and affected-row semantics.
7. API status/error mapping for missing section, lock conflict, DB/config failure, enqueue failure, and ended project.
8. `latest_extract` API serialization compatibility.
9. Queue failure after run insertion and duplicate pending-run recovery.

The DB tests call `setup()` which returns `None` when PostgreSQL is unavailable, causing tests to return successfully rather than fail (`crates/storage/src/persist.rs:3688-3710`). CI must explicitly provision PostgreSQL or add a required DB-test gate.

## Verified fixed since Round 1

- **Correct — Full-run serialization:** project-row `FOR UPDATE`, project lease, and defensive partial unique index prevent concurrent full claims (`crates/storage/src/bid.rs:283-330`; `migrations/0007_bid.sql:73-78`).
- **Correct — Full-run fencing:** report persistence and finish require matching run/project claim tokens and lock both rows (`crates/storage/src/bid.rs:426-477,549-575`).
- **Correct — Supporting indexes:** latest/status run and clause source/section/project indexes now exist (`migrations/0007_bid.sql:73-78,104-109`).
- **Correct — Storage signatures and callers:** all production persistence/finish calls pass non-optional claim/retry tokens (`crates/bid/src/lib.rs:572-603,643-760,838-850,1258-1268`). Exhaustive symbol searches found no stale production callers.
- **Correct — Failed coverage preserves old drafts:** uncovered candidate spans now return `ExtractionFailure` before persistence (`crates/bid/src/extraction/mod.rs:273-305`); document persistence is entered only for an `Ok` report (`crates/bid/src/lib.rs:774-850`).
- **Correct — Per-document failures continue:** load, decode, extraction, and persistence are isolated in `extract_one_document`, and later documents continue (`crates/bid/src/lib.rs:693-742,767-855`).
- **Correct — Requested missing/ineligible documents fail:** missing, mismatched, and non-completed documents produce explicit categories, while an empty project run fails with `no_completed_documents` (`crates/bid/src/lib.rs:681-691,775-791`).
- **Correct — Exact quote contract:** source validation now uses literal continuous containment; normalization is limited to overlap/deduplication (`crates/bid/src/extraction/reconcile.rs:8-32,48-63,217-224`).
- **Correct — Numbered requirements and dense coverage:** numbered headings/requirements are distinguished, prose is sentence-sized, and tables are row-sized (`crates/bid/src/extraction/outline.rs:112-230,251-368`; tests at `:376-452`).
- **Correct — Reconciliation:** body signals precede heading priors, and unresolved cross-family disagreements remain visible (`crates/bid/src/extraction/reconcile.rs:132-149,160-215`).
- **Correct — Tool hardening:** termination reasons, call/output/pattern/timeout limits, and typed strict arguments for every tool are implemented (`crates/bid/src/extraction/agent.rs:130-290,311-420`).
- **Correct — Policy consolidation:** veto terms now derive from `policy.must.veto`; no duplicate production veto list was found (`crates/bid/src/extraction/outline.rs:110-118`).
- **Correct — Latest-extract compatibility:** API fields align with the TypeScript consumer, including nested document diagnostics; timestamps and `triggered_by` are harmless extra JSON fields (`crates/api/src/routes.rs:3690-3717`; `web/src/api.ts:76-105`).
- **Correct — Evaluator and README:** real-model CLI loads expected labels, emits JSON/Markdown, exits nonzero on failed thresholds, and README invocation matches positional arguments (`crates/bid/src/bin/bid_extract_eval.rs:8-63`; `testdata/bid-extraction/README.md:1-32`).
- **Correct — Deploy variables:** mode/model variables are shared by API and worker, with documented hybrid defaults (`deploy/docker-compose.yml:24-31`; `deploy/.env.example:37-40`).
- **Correct — System/spec mirror seam:** the inspected extraction seam is identical at `docs/system-design.md:254-256` and `.scratch/knowledgebrain/spec.md:254-256`.

## Validation status

No executable validation was available in this review environment. The supervisor should run:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p bid -p storage -p worker -p api`
- `npm -C web run build`
- `docker compose -f deploy/docker-compose.yml --env-file deploy/.env.example config -q`
- `cmp docs/system-design.md .scratch/knowledgebrain/spec.md`
- a required PostgreSQL fresh-schema and pre-lock-schema upgrade test

## Conclusion

**Another focused fix worker is justified.** It should address the upgrade migration/error propagation blocker, section-retry heartbeat/fencing and stale cleanup, ownership/error mapping, and the missing retry UI/DB tests. A follow-up contract review should run against both a fresh database and a simulated pre-fix schema.
