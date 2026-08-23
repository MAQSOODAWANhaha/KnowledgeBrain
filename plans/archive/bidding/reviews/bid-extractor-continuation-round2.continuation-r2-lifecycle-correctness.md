# Round-2 Concurrency / Reliability / Security Review

## Review

- **Correct:** Important Round-1 fixes are present: durable generation/dirty fields, claim tokens and heartbeats, conversion-generation extraction idempotency, optimistic clause status fencing, durable Section retry intent, and explicit multimodal status.
- **Fixed:** None; this review was read-only.
- **Blocker:** Match scheduling can assign stale clause snapshots to the current generation, defeating both technical and commercial old-slow/new-fast fencing.
- **Verdict:** **Not ready.** Core lifecycle gaps remain around scheduling generations, project end, Section retry recovery, multimodal retries, object persistence, BID error mapping, and worker readiness.

## Findings

### Blocker — Match scheduling can publish stale results under the current generation

**Evidence**

- `schedule_match` reads generation and confirmed technical/commercial clauses in separate autocommit statements, then performs further merge queries and inserts jobs later: `crates/bid/src/lib.rs:1320-1369`.
- `enqueue_one_match` does not pass the generation used to read clauses: `crates/bid/src/lib.rs:1293-1316`.
- `insert_match_job` rereads and assigns the *current* project generation when inserting: `crates/storage/src/bid.rs:1303-1339`.
- Both terminal technical candidates and commercial replacement accept any job whose stored generation equals the current project generation: `crates/storage/src/bid.rs:1490-1518,1552-1599`.
- The schema has no unique/current-generation constraint preventing two same-unit jobs with different debounce keys in one generation: `migrations/0007_bid.sql:170-195`.

**Interleaving**

1. Scheduler A reads generation 1 and clause set S1.
2. A confirmed-clause mutation commits S2 and increments the project to generation 2.
3. Scheduler B reads generation 2/S2 and inserts a generation-2 job.
4. Scheduler A resumes; `insert_match_job` rereads generation 2 and inserts its S1-derived job as generation 2.
5. Both jobs claim successfully. B finishes quickly with correct S2 results; A finishes later.
6. Since both rows say generation 2, A can overwrite project-wide commercial hits. Its later `started_at` can also make stale technical candidates the latest unit result.
7. B clears `match_dirty` for generation 2. A’s generation-1 clear affects zero rows, so no recovery remains scheduled.

This defeats the intended old-slow/new-fast fence for both result families.

**Narrow fix**

Take one project-locked transaction that reads `(generation, confirmed clauses, merge map)` and inserts all jobs using that exact generation, or pass an expected generation to `insert_match_job` and reject if it changed. Ensure one authoritative job per `(project, generation, unit)` and add a true two-connection PostgreSQL race test covering both `tech_candidates` and `bid_commercial_hits`.

The existing test only proves a plainly older stored generation cannot finish; it does not exercise this scheduling race: `crates/storage/src/persist.rs:4141-4234`.

---

### High — Confirmed-set mutation and dirty intent are transactionally adjacent but not completely atomic

**Evidence**

- `update_clause` updates the clause first, then updates the open project, but does not check the project update’s `rows_affected`: `crates/storage/src/bid.rs:1253-1285`.
- `insert_clause` has the same unchecked project update for a confirmed row: `crates/storage/src/bid.rs:1002-1038`.
- API `require_open_project` is a separate preflight query, not serialization with the mutation: `crates/api/src/routes.rs:3628-3644,4024-4029,4155-4174`.
- Section merge changes technical match partitioning but commits through `set_section_merge` before scheduling and never atomically increments generation/dirty intent: `crates/api/src/routes.rs:4424-4454`; `crates/storage/src/bid.rs:1873-1883`.
- Full extraction locks run/project before superseding draft clauses: `crates/storage/src/bid.rs:1050-1066,1095-1103`; PATCH locks the clause before attempting the project dirty update: `crates/storage/src/bid.rs:1253-1282`.

**Interleavings**

- Project end can commit after the API preflight. The later clause UPDATE/INSERT succeeds, while `UPDATE bid_projects ... status='open'` affects zero rows. The confirmed set changes in an ended project without a generation increment.
- PATCH and full extraction have opposite lock ordering. PATCH can hold a draft clause while waiting for the project; extraction can hold the project while waiting to supersede that clause, causing PostgreSQL deadlock detection and an avoidable 500 response.
- A section merge can commit and then encounter a DB error in `schedule_match`, leaving no durable dirty intent.

**Narrow fix**

For every match-input mutation, lock the project first, require `status='open'`, mutate the clause/merge second, increment generation and dirty in the same transaction, and require exactly one affected project row. Use consistent project→clause/section lock ordering.

---

### High — Project end does not fence conversion or pending/running match work

**Evidence**

- Manual and expiry end only require the extraction lock to be absent and only fail pending extraction runs: `crates/storage/src/bid.rs:158-180,1791-1812`.
- Pending/running match rows are not canceled or superseded.
- Match heartbeat checks only job status/token, not project status or generation: `crates/storage/src/bid.rs:1365-1378`.
- Match terminal writes and commercial replacement check generation but not `p.status='open'`: `crates/storage/src/bid.rs:1490-1518,1552-1599`.
- Conversion heartbeat/final completion checks only document token/status, not project state: `crates/storage/src/bid.rs:271-337`.
- `pending_converts` includes documents belonging to ended projects: `crates/storage/src/bid.rs:1768-1773`.

**Interleavings**

- End a project while conversion is running. Heartbeats continue and the worker can mark the document completed after end. Auto extraction is then suppressed, leaving an ended project mutated after its read-only boundary.
- End while a match job is running. The worker can continue heartbeating and publish candidates or commercial hits after end.
- Match jobs already pending remain permanently pending because claims and housekeeping require an open project, while `any_match_running` continues counting them.

**Narrow fix**

Under the project-row lock, end should either reject active conversion/match or atomically cancel/fence them. Terminal writes and heartbeats must require an open project and current generation. Cancel pending match and Section retry rows on end, and exclude ended projects from pending conversion recovery. Add end-vs-convert and end-vs-pending/running-match race tests.

---

### High — Multimodal status is durable, but failure is not automatically retryable

**Evidence**

- Durable multimodal states exist in `bid_documents`: `migrations/0007_bid.sql:28-52`.
- Normal conversion sets `running`, then `done` or `skipped`, before completing the document: `crates/bid/src/lib.rs:558-630`.
- On VLM failure it marks multimodal failed and returns conversion failure: `crates/bid/src/lib.rs:590-606`.
- `convert_document` then marks the document `parse_status='failed'`: `crates/bid/src/lib.rs:517-535`.
- `BidConvertWorker` returns a retryable `JobErr`, but redelivery calls `claim_document_conversion`, which only claims `parse_status='pending'`; the failed document therefore becomes a no-op and is acknowledged: `crates/storage/src/bid.rs:241-258`; `crates/worker/src/consume.rs:1502-1525`.

**Impact**

Automatic queue retry does not retry multimodal or conversion failures. Recovery requires an explicit user retry, despite the worker reporting an error to Oxana.

The schema also lacks an invariant that `parse_status='completed'` requires `multimodal_status IN ('done','skipped')`; the current worker path observes the order, but storage does not enforce it.

**Narrow fix**

Keep retryable failures pending with generation/token rotation until the queue retry budget is exhausted, then transition to failed; alternatively introduce a dedicated durable multimodal job with attempts. Condition conversion completion on multimodal done/skipped and add transient-VLM-failure-then-success coverage.

---

### High — Configured S3 and conversion blob writes can still falsely succeed

**Evidence**

- `write_blob` discards a configured S3 PUT failure and returns local success: `crates/storage/src/lib.rs:25-31`.
- Bucket creation failure is also discarded: `crates/storage/src/s3.rs:65-72`.
- Bid conversion discards even the local/S3 result when writing extracted images and final Markdown, then marks conversion completed with the resulting reference: `crates/bid/src/lib.rs:583-586,628-638`.

**Failure path**

A full/read-only object volume or failed MinIO PUT makes the Markdown write fail. The ignored result is followed by a successful `finish_document_conversion`, leaving `markdown_ref` pointing to data that may not exist. Extraction later fails with an unavailable Markdown blob. Upload API similarly reports success when local disk succeeds but configured S3 fails.

**Narrow fix**

Propagate local write failures before changing DB state. When S3 is configured as required durability, map PUT/bucket-creation failure into the write result. If S3 is intentionally only a projection, persist an explicit unsynced projection state/outbox rather than reporting it as successful. Add forced local-write and configured-S3-500 tests.

---

### High — Durable Section retry has two independent leases with incorrect contention/recovery behavior

**Evidence**

- The worker first claims the durable job: `crates/storage/src/bid.rs:473-491`.
- `retry_section` separately tries to claim the project extraction lease: `crates/bid/src/lib.rs:1395-1420`.
- A busy project is returned as an error rather than a durable deferred state.
- Worker retries can mark the durable job failed after three quick collisions: `crates/worker/src/consume.rs:1585-1601`.
- Job and project leases are reclaimed independently and in separate housekeeping calls: `crates/worker/src/consume.rs:1451-1457,1471-1476`.
- On the success path, the boolean result of `finish_section_retry_job` is ignored; `Ok(false)` still produces worker success: `crates/worker/src/consume.rs:1588-1593`.
- Existing coverage tests only the durable job token in isolation: `crates/storage/src/persist.rs:4080-4137`.

**Interleavings**

- A full extraction owns the project. A queued retry claims its job, fails the project claim, and consumes Oxana retries. It can become permanently `failed` before the legitimate extraction releases.
- If only the job heartbeat is judged stale, housekeeping resets the job to pending while the original worker can still hold and heartbeat the project lease. A replacement claims the durable job and collides with the old project lease. The original may successfully persist and release the project, then get `Ok(false)` finishing its reclaimed durable job; the worker nevertheless reports success while the durable row remains pending and will execute again.

**Narrow fix**

Acquire job and project leases in one transaction and one lock order, or treat project contention as deferred success without spending retries. Reclaim the paired leases atomically. Require `finish_section_retry_job == true` on both success and retry paths. Add paired-lease stale/active permutations and full-extract-contention tests.

---

### Medium — BID database error mapping remains incomplete

**Evidence**

- Clause list ignores `decorate_clauses` failures, which include pick and commercial-hit queries: `crates/api/src/routes.rs:3989-3997`; `crates/bid/src/lib.rs:217-262`.
- PATCH assessment validation also ignores decoration failure, allowing a failed suggestion lookup to appear empty and potentially accept `assessment='meet'`: `crates/api/src/routes.rs:4063-4081`.
- Preview maps connection/query failure to 404 or empty datasets: `crates/api/src/routes.rs:4596-4635`.
- Booklet and export endpoints still map connection failure to 404: `crates/api/src/routes.rs:4668-4683,4690-4704,4713-4728,4741-4755`.
- Several successful BID mutations ignore DB errors when marking booklet parts stale or inserting matched shots: `crates/api/src/routes.rs:4117-4128,4194-4197,4341-4369,4399-4400,4452-4454`.
- Invalid clause `family`, `status`, and `assessment` now correctly return validation errors: `crates/api/src/routes.rs:4041-4061`; manual clause family is also validated at `crates/api/src/routes.rs:4157-4163`.
- Invalid export formats silently become DOCX instead of a 400: `crates/api/src/routes.rs:4733-4763`.

**Narrow fix**

Use `require_bid_pool` and `bid_query_failed` consistently for all BID endpoints. Propagate decoration and preview component failures. Do not use empty data as an error fallback. Return 400 for unsupported export formats. Decide which stale/booklet/shot mutations are required and either propagate them or make them durable outbox work.

---

### Medium — Redis reconnect loop exists, but readiness, duplicate worker registration, and reconnect cleanup are not complete

**Evidence**

- `runtime::connect` is explicitly lazy and does not prove Redis connectivity: `crates/runtime/src/jobs.rs:263-268`.
- `consume_loop` logs `worker ready` immediately after this lazy constructor and before `run_core` establishes consumers: `crates/worker/src/main.rs:33-45`.
- Every `run_core` attempt spawns a detached signal-listener task; failed reconnect attempts do not cancel it: `crates/worker/src/consume.rs:2710-2719`.
- `run_core` concurrently registers the same DefaultQueue BID workers in both the `core` and `shared` runtimes: `crates/worker/src/consume.rs:2722-2730,2785-2810`. This creates two worker sets and makes effective concurrency the sum of two settings.
- The reconnect backoff is never reset after a successful connection.
- The launch test trusts the premature log and does not stop/start Redis or prove consumption: `crates/worker/tests/launch.rs:7-18`.

**Assessment**

The Round-1 “never reconnects” defect is improved: a returned runtime error now enters a retry loop. However, readiness remains false-positive, reconnect attempts accumulate detached signal tasks, and the code cannot support an attestation of “no duplicate workers.”

**Narrow fix**

Perform an actual Redis probe before readiness, construct one selected worker topology, own all runtime tasks beneath a cancellation token, await their shutdown before reconnecting, and reset backoff after a healthy interval. Add Redis stop/start coverage that verifies one processing attempt, task cleanup, and graceful SIGTERM during backoff.

---

### Medium — Stale match workers keep heartbeating after losing generation

**Evidence**

- Match heartbeat is conditioned only on job status/token: `crates/storage/src/bid.rs:1365-1378`.
- Generation loss is discovered only at commercial publication or terminal write: `crates/storage/src/bid.rs:1490-1518,1552-1575`.
- A failed terminal generation check leaves the row running; the worker stops its heartbeat and returns an error: `crates/bid/src/lib.rs:975-1000,1222-1244`.
- Housekeeping can only reclaim it after the stale timeout: `crates/storage/src/bid.rs:1381-1406`.

**Impact**

An obsolete slow search keeps extending its lease and consuming resources until search completion. It then remains `running` until housekeeping, keeping `match_running=true` and causing retry deliveries to no-op.

**Narrow fix**

Heartbeat must require current generation and open project. Generation bump/end should atomically mark pending and running older jobs superseded, or cancellation should transition them terminal immediately.

## Verified-Correct Areas

- **Duplicate delivery is idempotent at claim boundaries.** Conversion claims only pending documents with a new token (`crates/storage/src/bid.rs:241-269`); match claims only pending/current rows (`:1343-1362`); extraction and Section retry jobs also use pending→running token claims. Duplicate delivery cannot acquire the same active row twice.
- **Convert→extract handoff is generation-idempotent.** The fresh schema uniquely indexes `(document_id, conversion_generation)` for auto runs (`migrations/0007_bid.sql:122-131`), and `ensure_auto_extract_run` returns the existing row on conflict (`crates/storage/src/bid.rs:850-906`). The integration test calls it twice and receives the same run (`crates/storage/src/persist.rs:4237-4321`).
- **Retry and stale conversion writes are token-fenced.** Reset increments `conversion_generation` and clears the old token; late completion requires the old processing token and affects zero rows: `crates/storage/src/bid.rs:340-371,307-337`. Existing coverage proves this stale-write case.
- **Document deletion also prevents a late DB write.** Deletion removes the row, so token-conditioned heartbeat/finish affects zero rows. The remaining defect is lifecycle treatment and unnecessary worker retry, not stale DB resurrection.
- **Normal extraction does not begin before multimodal done/skipped.** The current converter sets multimodal terminal before marking parse completed, and auto-run creation requires parse completed: `crates/bid/src/lib.rs:558-638`; `crates/storage/src/bid.rs:850-906`.
- **Clause PATCH cannot resurrect a row already superseded.** PATCH passes the observed status, and the UPDATE includes `WHERE ... status = expected_status`: `crates/api/src/routes.rs:4087-4115`; `crates/storage/src/bid.rs:1253-1272`. If extraction supersedes first, PATCH returns `STALE_CLAUSE`.
- **Commercial replacement is transactional once a correctly generated job reaches publication.** Job/project are locked and the delete/insert set commits together: `crates/storage/src/bid.rs:1552-1599`.
- **Claim-token rows consistently inspect affected rows** for conversion heartbeat/finish, extraction finish, Section project lease release, match finish, and commercial publication. The notable exceptions are the project dirty update and the worker’s ignored Section job-finish boolean.
- **Fresh-schema policy is respected.** The lifecycle fields and constraints are defined directly in `0007_bid.sql`; no legacy-volume compatibility issue is raised by this review.

## Test and Race-Coverage Gaps

The current PostgreSQL tests cover generation mismatch, basic dirty increments, conversion token retry, auto-run idempotency, extraction leases, and isolated Section job reclaim. They do not cover:

- scheduler snapshot vs generation reassignment;
- same-generation old-slow/new-fast technical and commercial publication;
- duplicate match delivery during an active claim;
- match/conversion versus manual or expiry end;
- active conversion versus delete/end;
- automatic transient multimodal retry;
- paired Section job/project lease recovery and contention exhaustion;
- clause PATCH vs supersession using two connections, including deadlock ordering;
- Redis outage/recovery/readiness/task cleanup;
- forced local/S3 write failure;
- BID preview/decoration database failures.

PostgreSQL, Redis, and S3 tests also skip when services are unavailable, so CI must make those services mandatory for attestation.

## Residual Risks

- No shell or Git command facility was available. Tests, formatting, compilation, staged state, and the exact worktree diff were not executed or independently attested.
- Oxana’s internal runtime-drop behavior was not inspectable here; the source-level detached signal task and duplicate registrations remain proven, while any additional internal task leakage is unverified.
- Real DocReader/VLM/search-provider timing and cancellation behavior remain untested.
- Global BID visibility remains consistent with the approved single-company access model; no new tenant-isolation finding was introduced.

## Required Supervisor Validation

Run with mandatory PostgreSQL, Redis, and configured S3/MinIO; fail if any integration test prints a service-skip message:

- `git status --short --untracked-files=all`
- `git diff --check`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace -- --nocapture`
- `npm -C web run build`
