# Bid Extractor Round 3 — Contracts and Documentation Review

## Review

### Correct

- **Document route ownership is fixed.** Delete and retry use `(project_id, document_id)` and require exactly one affected row at `crates/storage/src/bid.rs:234-264`; the API returns 404 for missing or mismatched documents at `crates/api/src/routes.rs:3833-3878`. PostgreSQL coverage verifies cross-project mutations fail at `crates/storage/src/persist.rs:3998-4051`.

- **The reviewed extraction mutation routes now require durable state before success.**
  - Upload persists a pending document before enqueue at `crates/api/src/routes.rs:3822-3829`.
  - Document retry durably resets the owned row at `crates/api/src/routes.rs:3862-3878`.
  - Full re-extraction persists a pending run before enqueue at `crates/api/src/routes.rs:3914-3928`.
  - Ignoring the immediate Redis result is valid for these routes because housekeeping re-enqueues pending converts and runs at `crates/worker/src/consume.rs:1441-1479`.

- **Section retry ownership, fencing, heartbeat, and stale recovery are materially correct.**
  - The project lease is bound to `section_id` at `crates/storage/src/bid.rs:283-315`.
  - Status writes, report persistence, and release all verify the retry token at `crates/storage/src/bid.rs:317-338`, `:806-880`, and `:1354-1380`.
  - Heartbeats run during extraction at `crates/bid/src/lib.rs:661-692,1296-1300`.
  - Stale recovery marks the owned running section failed before clearing its lease at `crates/storage/src/bid.rs:495-535`.
  - The two-owner stale retry and late stale-owner mutation case is tested at `crates/storage/src/persist.rs:3900-3986`.

- **Retry HTTP error mapping is acceptable.** Missing/mismatched sections map to 404, lease conflict to 409, incomplete extraction to 422, and unexpected storage/configuration/persistence errors to a generic 500 at `crates/api/src/routes.rs:3881-3905`. A typed error enum would improve maintainability but is not required for correctness.

- **Full extraction lease behavior is correct.** Claims require an open project, match stored payload identity, and serialize through the project row at `crates/storage/src/bid.rs:342-391`; heartbeat and fenced finish/persistence are at `:394-427`, `:568-620`, and `:693-803`. Tests cover concurrent claims, live-heartbeat protection, stale-owner persistence/finish rejection, and ended-project claims at `crates/storage/src/persist.rs:3792-3995`.

- **Fresh-schema migration shape is idempotent and no upgrade migration is required under the accepted wipe policy.** `0007_bid.sql` uses `CREATE TABLE/INDEX IF NOT EXISTS` and a guarded FK addition at `migrations/0007_bid.sql:3-207`. The run-token and heartbeat invariants are defined at `:70-94`. The test setup drops the schema and applies all migrations at `crates/storage/src/persist.rs:3688-3709`, with lease-column and invalid-running-row assertions at `:3713-3784`. The lack of `ALTER ... ADD COLUMN` upgrade logic is not a blocker because old volumes may be wiped.

- **Direct migration SQL errors now propagate through `storage::connect`.** `crates/storage/src/persist.rs:19-22,35-44` uses `?` for the migration scripts instead of returning a usable pool after failure.

- **Deployment extraction variables are coherent.** Shared environment values provide `BID_EXTRACT_MODE` and model fallback to both API and worker at `deploy/docker-compose.yml:3-34,135-174`; defaults are documented at `deploy/.env.example:37-41`. Deployment documentation correctly states migrations `0001`–`0008` at `deploy/README.md:25`.

- **`latest_extract` remains API/UI compatible.** Storage selects the expected fields at `crates/storage/src/bid.rs:623-638`; the API serializes them at `crates/api/src/routes.rs:3706-3732`; and TypeScript consumes the nested document diagnostics safely at `web/src/api.ts:76-105,192` and `web/src/bid/Workbench.tsx:198-209`.

- **Documentation matches the current quote/context/heartbeat semantics.**
  - `quotable_text` is the only valid quote source and table context is explicitly non-quotable at `docs/bid-platform-domain.md:210-217,237-241,290-297`.
  - The implementation exposes separate `non_quotable_context` and `quotable_text` at `crates/bid/src/extraction/agent.rs:204-213,370-377`.
  - Literal continuous containment is enforced at `crates/bid/src/extraction/reconcile.rs:24-25,52-61`, with context-rejection coverage at `:218-255`.
  - Token and heartbeat ownership is documented at `docs/bid-platform-domain.md:219-235` and mirrored in both `docs/system-design.md:254-256` and `.scratch/knowledgebrain/spec.md:254-256`.

- **Evaluator README matches the CLI.** Positional arguments and output behavior align between `testdata/bid-extraction/README.md:10-29` and `crates/bid/src/bin/bid_extract_eval.rs:8-63`. One-to-one exact normalized-label assignment, aliases, false positives, family/must metrics, and threshold failures are implemented at `crates/bid/src/extraction/evaluation.rs:99-289`.

### Blocker — High: migration failure still does not reach process startup

Although `storage::connect()` propagates migration errors, its deploy-facing callers discard them:

- API starts serving and only logs a background migration/connect failure at `crates/api/src/main.rs:12-26`. Its `/health` check can therefore report healthy while schema initialization failed.
- Worker converts the result to `Option<PgPool>` at `crates/worker/src/main.rs:22-28`.
- Bid convert, extract, and match workers then return `Ok(())` when that pool is absent at `crates/worker/src/consume.rs:1493-1504,1540-1552,1576-1588`, acknowledging jobs without executing them.
- Housekeeping also silently succeeds without a pool at `crates/worker/src/consume.rs:1428-1434`.
- `apply_0001` additionally ignores `ensure_company_workspace` failure at `crates/storage/src/persist.rs:42-44`, allowing the one-shot backfill to proceed without its required company workspace.

This contradicts the deployment promise that API/worker startup applies migrations and can lose queued work following a schema or connection failure.

**Required fix:** await and propagate storage initialization before API readiness and worker consumption; do not construct a production worker context with `pool=None`. Change `ensure_company_workspace(pool).await` to propagate failure before applying the backfill.

### Blocker — High: the broader bid mutation API still contains false-success paths

The Round-2 document/extraction routes are fixed, but the global statement that bid mutations no longer report success without durable state is false:

- Clause PATCH returns 204 when both connection attempts fail at `crates/api/src/routes.rs:3987-3993`.
- Manual clause creation returns 201 even when no database connection was obtained at `crates/api/src/routes.rs:4092-4143`.
- Manual matching always returns 202 while its helper suppresses database and scheduling failures at `crates/api/src/routes.rs:4145-4161`.
- Pick creation and section merge return 204 on database unavailability at `crates/api/src/routes.rs:4220-4226,4362-4371`.
- Pick deletion suppresses connection and delete failures before returning 204 at `crates/api/src/routes.rs:4307-4326`.
- Shot upload ignores blob-write, database-connect, and insert failures before returning 201 at `crates/api/src/routes.rs:4427-4498`.
- Shot deletion suppresses connection/delete failures before returning 204 at `crates/api/src/routes.rs:4500-4514`.

These are current correctness blockers, not optional type or UX improvements.

**Required fix:** use the existing `require_bid_pool()` once per mutation, propagate the primary storage operation, and only return success after its durable row change. Durable pending job insertion may still justify accepting an enqueue failure.

### Blocker — High: automatic convert-to-extract handoff can be lost

After successful conversion, the worker ignores `insert_extract_run` failure, enqueues regardless, and returns success at `crates/worker/src/consume.rs:1507-1523`. If insertion fails transiently, the document remains `completed`, no pending run exists for housekeeping to discover, and the auto-extraction job has no claimable durable record.

**Required fix:** propagate run insertion failure, or transactionally/idempotently ensure a pending extraction run exists before acknowledging conversion. Add a recovery test for failure between completed-document persistence and extraction-run creation.

### Note — Medium: ownership hardening remains incomplete outside document routes

- Shot deletion is keyed only by shot ID at `crates/storage/src/bid.rs:1186-1191`; the path project ID is not part of the delete.
- Shot upload accepts arbitrary clause/product/version IDs and inserts them with the path project at `crates/api/src/routes.rs:4427-4498`; the schema has independent project and clause foreign keys at `migrations/0007_bid.sql:163-173`, so it does not enforce same-project ownership.
- Manual clause creation accepts a `section_id` without checking it belongs to the path project at `crates/api/src/routes.rs:4092-4143`.

Use ownership-conditioned storage operations and require one affected row, following the corrected document pattern.

### Note — Tests and non-blocking follow-ups

- No API tests exercise database-unavailable mutation responses, retry status mapping, shot ownership, or `latest_extract` JSON compatibility.
- Fresh setup is tested, but `apply_0001` is not explicitly invoked twice in the same test. The SQL is structurally idempotent; a repeat-application assertion would make that contract executable.
- PostgreSQL tests skip successfully when the database is unavailable at `crates/storage/src/persist.rs:3688-3709`; CI must provide a required PostgreSQL gate.
- Section retry remains absent from the TypeScript client and UI: `web/src/api.ts:182-240` exposes document/full retries but no section retry, and `MatchUnit` lacks extraction state at `web/src/api.ts:113-119`. This is a real UX/spec gap, but not a storage-contract blocker.
- Clause family/status/assessment remain stringly typed at `web/src/api.ts:49-66` and `crates/api/src/routes.rs:3963-3985`. Enum validation would improve error quality but is optional relative to the blockers above.

## Conclusion

**Another immediate focused fix is justified.** It should address startup migration propagation, all remaining bid mutation false-success paths, durable convert-to-extract handoff, and shot/manual-clause ownership. The completed lease, document ownership, migration schema, evaluator, latest-extract, and documentation work does not require redesign.

No files were edited. Executable tests, compose validation, Git status, and staged-file state were not available through the review tools.