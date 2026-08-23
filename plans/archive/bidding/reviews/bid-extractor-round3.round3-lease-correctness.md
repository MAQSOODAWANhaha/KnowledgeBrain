# Bid Extractor Round-3 Lease Correctness Review

## Review

### Blocker

- None found.

### High

1. **Section retry reads its extraction input before acquiring the project lease.**
   - **Evidence:** `retry_section` loads and copies the section’s document, key, heading, family, and body at `crates/bid/src/lib.rs:1255-1267`, but does not claim the section-retry lease until `crates/bid/src/lib.rs:1269-1272`.
   - **Impact:** A full extraction can claim the project after the initial read, replace that section, and finish. The retry can then acquire the now-free lease and persist extraction based on the pre-full-run body, superseding newer drafts. Full and retry execution are mutually excluded while leased, but their inputs are not serialized.
   - **Concrete fix:** Acquire `claim_section_retry` using the expected project and section IDs before reading section content, then reread the section while holding the lease. Release the lease on lookup failure. Add a race test where a full run updates the section between retry request entry and retry claim, and assert the retry uses the post-full-run body.

2. **Deleting a document can turn its pending document-scoped run into a full-project run.**
   - **Evidence:** `bid_extract_runs.document_id` uses `ON DELETE SET NULL` at `migrations/0007_bid.sql:69-72`. `delete_document_for_project` deletes the document directly at `crates/storage/src/bid.rs:251-260`. A null `document_id` is interpreted as a full-project run by `next_pending_extract` at `crates/storage/src/bid.rs:1206-1224` and then enqueued by housekeeping or the extract worker at `crates/worker/src/consume.rs:1458-1468,1550-1557`.
   - **Impact:** Deleting a document with a pending scoped extraction changes that run’s meaning. Recovery can later claim it as a full run and re-extract/supersede drafts for every remaining completed document.
   - **Concrete fix:** In the fresh schema, do not use `SET NULL` where null is the full-run discriminator. Use `ON DELETE CASCADE` for document-scoped runs, and make document deletion transactionally reject an active project extraction lease so a running run cannot be cascaded while leaving its project lock behind. Add pending-delete and running-delete integration tests.

### Medium

3. **Ending a project is not serialized with creation of new documents or pending runs.**
   - **Evidence:** `end_project` ends the project and fails only pending runs visible inside its transaction at `crates/storage/src/bid.rs:144-166`. `insert_document` and `insert_extract_run` insert without checking or locking project status at `crates/storage/src/bid.rs:170-183,547-562`. In particular, after conversion completes, `BidConvertWorker` inserts and enqueues a run unconditionally at `crates/worker/src/consume.rs:1504-1515`. API open checks are separate reads at `crates/api/src/routes.rs:3619-3637`, preceding document/run writes at `crates/api/src/routes.rs:3822-3826,3917-3928`.
   - **Impact:** A conversion or request racing with project ending can leave a new document or `pending` run on an ended project. Claims correctly reject ended projects, so the run remains stranded and can be shown as the latest pending extraction indefinitely.
   - **Concrete fix:** Make project-open validation and document/run insertion one transaction that locks the project row. Return whether insertion occurred and enqueue only after a successful open-project insert. Apply the same transactional open-state rule to document retry/delete if ended projects must be strictly immutable. Add end-versus-convert and end-versus-reextract tests.

## Correctness verified without a finding

- Full claims serialize on the project row and install matching run/project tokens: `crates/storage/src/bid.rs:342-392`.
- Full and section-retry heartbeats renew their respective leases every 30 seconds: `crates/storage/src/bid.rs:394-446`; `crates/bid/src/lib.rs:627-690`.
- Stale full runs are reset to pending and stale retry state is cleaned up with token/section fencing: `crates/storage/src/bid.rs:449-528`.
- Full/retry claim exclusion and stale-owner status/release fencing are exercised at `crates/storage/src/persist.rs:3790-3994`.
- Full-report and section-retry persistence are transactional and lease-conditioned: `crates/storage/src/bid.rs:693-887`. Rollback preservation is tested at `crates/storage/src/persist.rs:4054-4159`.
- Manual project ending and full-run claiming serialize correctly through the project row; a queued run cannot claim after status becomes ended: `crates/storage/src/bid.rs:144-167,349-392`.
- Document retry/delete operations now condition mutation on both project and document IDs: `crates/storage/src/bid.rs:234-260`, tested at `crates/storage/src/persist.rs:3998-4051`.
- For a fresh database, migration lock-state checks, running-run token/heartbeat checks, one-running-run partial uniqueness, heartbeat lookup, and clause lookup indexes are present at `migrations/0007_bid.sql:3-24,69-131`. No legacy `ALTER` migration is requested or required.

## Optional / deferred

- The PostgreSQL tests return early when the database cannot be reached (`crates/storage/src/persist.rs:3688-3710`), so CI must provision PostgreSQL to attest these races.
- Periodic heartbeat task behavior itself is not directly exercised; current integration coverage calls storage heartbeat functions explicitly.
- Git state and executable validation were unavailable in this read-only tool environment.

## Conclusion

**No blocker remains. Two worthwhile immediate high-severity correctness fixes and one medium-severity ending/mutation race remain.** The implementation should not yet be considered fully Round-3 lease-correct until those three cases are resolved and covered by PostgreSQL race tests.
