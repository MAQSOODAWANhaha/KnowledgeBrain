# Continuation Round-3 Lifecycle Final Review

## Review

- **Correct:** Expected-generation binding, generation-fenced publication, project-first storage locking, bounded conversion retry, completed/multimodal constraints, end fencing, configured-S3 failure propagation, Redis connectivity probing, and duplicate DefaultQueue BID registration are present.
- **Fixed:** None; review was read-only.
- **Blocker:** Match-job uniqueness conflates the commercial job with the technical unsectioned unit.
- **Blocker:** Concurrent partial clause PATCH can overwrite confirmed match inputs without incrementing generation.
- **High:** Concurrent Section merges can create cycles.
- **High:** Section-retry project and durable-job leases are not finished atomically.
- **Medium:** Several BID mutations still ignore required booklet-staleness failures or expose raw storage errors.
- **Verdict:** **Not release-ready. Immediate final parent fixes are warranted.**

## Findings

### Blocker — Commercial and unsectioned technical jobs collide in the uniqueness index

**Evidence**

- Unsectioned technical clauses resolve to `Uuid::nil()`: `crates/bid/src/lib.rs:54-64`.
- Scheduler inserts technical units as `Some(unit)` and the commercial job as `None`: `crates/bid/src/lib.rs:1360-1388`.
- The unique index maps both SQL `NULL` and the nil UUID to the same key: `migrations/0007_bid.sql:184-190`.
- On conflict, `insert_match_job` subsequently searches using the original nullness of `unit_id`: `crates/storage/src/bid.rs:1503-1542`.
- Manual technical clauses explicitly permit no Section: `crates/api/src/routes.rs:4170-4188`.
- The existing uniqueness test exercises only commercial `None` versus commercial `None`, not `Some(Uuid::nil())` versus `None`: `crates/storage/src/persist.rs:4268-4347`.

**Reproduction**

1. Confirm an unsectioned technical clause and any commercial clause in one project generation.
2. Scheduler inserts the technical job with `unit_id = Some(00000000-...)`.
3. It then attempts the commercial job with `unit_id = NULL`.
4. The unique index rejects the insert because `COALESCE(NULL, nil) = nil`.
5. The follow-up lookup asks for `unit_id IS NULL`, cannot find the technical row, and returns `RowNotFound`.
6. `enqueue_one_match` turns that into `Ok(None)`; scheduling clears `match_dirty`.
7. No commercial job exists, so commercial matching is silently omitted.

**Narrow fix**

Add an explicit job kind (`technical`/`commercial`) and make uniqueness cover `(project_id, generation, job_kind, unit_id NULLS NOT DISTINCT)`. Do not use a UUID value that is also a valid technical unit as the commercial sentinel. Add a PostgreSQL regression containing both unsectioned technical and commercial clauses in one generation and assert two distinct jobs complete.

---

### Blocker — Partial concurrent PATCH can change confirmed match input without a generation bump

**Evidence**

- PATCH reads and materializes all clause fields before entering the storage transaction: `crates/api/src/routes.rs:4034-4051`.
- Missing request fields are filled from that pre-lock snapshot: `crates/api/src/routes.rs:4048-4051,4088-4097`.
- `set_changed` is also calculated against the pre-lock snapshot: `crates/api/src/routes.rs:4098-4115`.
- Storage locks the project first, but its optimistic predicate checks only clause status: `crates/storage/src/bid.rs:1429-1455`.
- Storage trusts the caller-provided `mark_match_dirty`: `crates/storage/src/bid.rs:1456-1477`.
- The current test performs only a sequential full-field update: `crates/storage/src/persist.rs:4228-4265`.

**Reproducible interleaving**

1. Confirmed clause has text `X`, generation 1.
2. Request A reads `X` and PATCHes text to `Y`.
3. Request B concurrently reads `X` and PATCHes only `assessment`.
4. A commits `Y`, increments generation to 2, schedules a generation-2 job, and a slow worker reads `Y`.
5. B acquires the project lock afterward. Because status remains `confirmed`, its update succeeds, writing its stale default text `X`.
6. B calculated `set_changed=false`, so generation remains 2 and no older job is fenced.
7. The slow worker publishes results for `Y` as generation 2 although the confirmed clause now contains `X`.

Even without the worker timing, B silently loses A’s edit.

**Narrow fix**

Make the storage transaction authoritative: project lock first, then lock the current clause row, apply only fields actually supplied by PATCH, and derive match-input change from the locked old/new values. Add a clause revision and reject stale revisions with `409 STALE_CLAUSE`, or otherwise ensure partial updates cannot write stale defaults. Add a two-connection assessment-only-versus-text race test and assert both the final clause and generation.

---

### High — Concurrent merge requests can commit a merge cycle

**Evidence**

- Cycle validation uses an API-side snapshot before the project-locked mutation: `crates/api/src/routes.rs:4444-4457`.
- `set_section_merge` locks the project but validates only ownership, not the current graph or cycle condition: `crates/storage/src/bid.rs:2078-2118`.
- `resolve_unit` merely terminates when it notices a cycle and returns whichever repeated node it encountered: `crates/bid/src/lib.rs:58-73`.

**Reproducible interleaving**

1. Sections A and B are both roots.
2. Requests R1 (`A → B`) and R2 (`B → A`) read the same acyclic merge map.
3. Both API validations pass.
4. R1 takes the project lock and commits `A → B`.
5. R2 then obtains the project lock but does not revalidate; it commits `B → A`.
6. A and B now resolve to different apparent units depending on traversal start, corrupting match partitioning.

**Narrow fix**

After locking the project, load/lock the current Section merge graph and validate ownership, self-merge, and reachability inside `set_section_merge` before updating. Add a two-connection opposing-merge test asserting exactly one request conflicts and the graph remains acyclic.

---

### High — Section-retry finish is not paired with project-lease release

**Evidence**

- Claim atomically assigns one token to both project and durable job: `crates/storage/src/bid.rs:562-615`.
- Reclaim handles both rows in one transaction: `crates/storage/src/bid.rs:680-727`.
- `retry_section_claimed` releases the project lease before returning: `crates/bid/src/lib.rs:1568-1577`.
- Only afterward does the worker finish or reset the durable job in a separate transaction: `crates/worker/src/consume.rs:1597-1618`.
- `finish_section_retry_job` updates only the job; `finish_section_retry` updates only the project: `crates/storage/src/bid.rs:641-665,758-779`.
- Existing coverage tests paired claim/reclaim, but not the successful persist/release/job-finish crash window: `crates/storage/src/persist.rs:4073-4158`.

**Reproducible failure**

1. Retry successfully persists clauses and marks the Section done.
2. It releases the project lease.
3. Worker crashes before finishing the durable job.
4. A user can now confirm one of the new drafts because the project is free.
5. Housekeeping reclaims the still-running job and executes it again.
6. The replay preserves the confirmed clause but inserts another complete draft set, including a duplicate of the confirmed requirement.

**Narrow fix**

Provide one token-conditioned transaction that finishes/resets the durable job and releases the matching project lease together. On success, do not expose the project as free while the job remains running. Add crash-injection coverage immediately before terminal finish, including confirmation attempts and stale reclaim.

---

### Medium — BID ancillary DB failures still produce false success or raw 500 responses

**Evidence**

- Manual clause creation ignores `mark_booklet_stale`: `crates/api/src/routes.rs:4208-4211`.
- Section merge does the same: `crates/api/src/routes.rs:4463-4471`.
- Pick creation/deletion also discard booklet-staleness errors: `crates/api/src/routes.rs:4355-4360,4409-4414`.
- Match completion ignores the commercial booklet-staleness update: `crates/bid/src/lib.rs:1254-1262`.
- Clause insert/PATCH map unexpected SQLx errors, including project-end races, directly into response text rather than the generic BID mapping: `crates/api/src/routes.rs:4117-4119,4205-4207`.

A stale-marker failure can leave booklet preview/export presenting old content after the primary mutation was reported successful.

**Narrow fix**

Persist required stale intent atomically with the primary mutation or through a durable outbox. Do not merely propagate after committing the primary write, since that would return a retryable-looking 500 after success. Map expected closed/stale conflicts to 409 and all unexpected DB failures through `bid_query_failed` without returning raw SQLx text.

## Verified Fixed Seams

- Scheduler insertion binds the generation used by the snapshot and rejects a changed project generation: `crates/storage/src/bid.rs:1486-1502`.
- Match claim, heartbeat, commercial publication, and terminal technical publication require an open project and current generation: `crates/storage/src/bid.rs:1547-1594,1669-1770`.
- Clause insert/update and Section merge acquire the project lock before their row mutation and check critical affected-row results: `crates/storage/src/bid.rs:1171-1212,1429-1479,2078-2118`.
- End and expiry cancel conversion, extraction, match, and Section-retry work transactionally: `crates/storage/src/bid.rs:158-226,2029-2045`.
- Conversion retries return failed attempts to pending, final exhaustion marks them failed, and completed documents require multimodal `done|skipped`: `crates/bid/src/lib.rs:505-552`; `crates/worker/src/consume.rs:1507-1536`; `migrations/0007_bid.sql:43-52`.
- Local image/Markdown writes and configured S3 bucket/PUT failures propagate: `crates/bid/src/lib.rs:598-645`; `crates/storage/src/lib.rs:25-39`; `crates/storage/src/s3.rs:38-42,66-73`.
- Extraction supersedes only draft clauses transactionally and preserves confirmed/rejected clauses: `crates/storage/src/bid.rs:1214-1324`.
- Redis readiness performs a real operation; BID workers appear only in the core DefaultQueue topology, and reconnect signal tasks are aborted: `crates/runtime/src/jobs.rs:263-275`; `crates/worker/src/main.rs:29-53`; `crates/worker/src/consume.rs:2710-2818`.

## Required Internal Regression Tests

After the fixes, add and run mandatory PostgreSQL tests for:

1. Commercial `NULL` and technical unsectioned nil UUID jobs in the same generation.
2. Assessment-only PATCH racing a confirmed text/family/must PATCH.
3. Opposing concurrent Section merges.
4. Crash between successful Section persistence and paired terminal finish.
5. Technical and commercial old-slow/new-fast publication with a timing-controlled provider.

These must not silently skip when PostgreSQL is unavailable.

## Residual External Tests

- Redis stop/start during active consumption, readiness loss/recovery, retry backoff, and runtime-task cleanup.
- Forced local filesystem failure and configured S3 bucket/PUT 500.
- Real VLM transient failure followed by success and final exhaustion.
- Strict Agent, non-Markdown DocReader, LDAP, real embedding/search, and model-provider tests.
- Full Docker image build and service restart testing.
- Product/company asset pick, shot upload/read/delete, and real candidate matching.

## Release Readiness

**Not ready for release.** The uniqueness collision silently omits commercial matching, while the stale partial-PATCH race permits same-generation publication against obsolete confirmed text. Both affect core correctness and require an immediate final parent fix. The merge-cycle and Section-finish gaps should be repaired in the same pass before release.
