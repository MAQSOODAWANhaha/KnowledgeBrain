# Fresh-launch catalog and seed verification

`catalog-row-allowlist.toml` is the reviewed PostgreSQL 16 catalog, database/schema/relation ACL, role-topology, and seed-row contract produced by the fixed three-slice fresh baseline. The verifier compares it bidirectionally and records a durable one-shot marker.

## Mandatory Compose command

For a **new installation only**, the final release will run exactly:

```sh
KNOWLEDGEBRAIN_FIRST_LAUNCH_FRESH=required deploy/compose-first-launch.sh
```

Production remains deliberately nondeployable until runtime acceptance. Before any Docker action, the production script strictly parses the checked-in `runtime-completion.toml`, rejects missing, duplicate, malformed, or unknown fields, recomputes the reviewed registry/evaluation/readiness/image/topology hashes, and requires `phase_1d_runtime_complete=true`. It separately requires exactly `knowledge_base_baseline`, `shared_platform_baseline`, and `bidding_v1_baseline` with checked-in checksums. There is no environment bypass. This slice intentionally keeps completion `false`.

After that final gate exists, the noninteractive command deletes this Compose project's volumes, starts base infrastructure, runs `migrate` once, performs the irreversible migrator-to-verifier handoff, requires the marker still to be empty, runs the verifier once, and only then starts the `runtime` profile (`api`, `worker`, independent `retention`, and `docreader`). It does not print database passwords. Do not use it against an existing installation.

A plain `docker compose up -d` starts only base infrastructure. Both one-shot services are in the `first-launch` profile and all traffic/runtime services are in the `runtime` profile. Runtime services have no dependency on `migrate`.

For a normal restart after successful first launch, run exactly:

```sh
deploy/compose-runtime-restart.sh
```

That command names only runtime services, never migration or verification. API and worker read the durable marker and finalized topology and fail closed if either differs.

## Trust handoff

Bootstrap creates fixed `kb_app_owner` (`NOLOGIN NOINHERIT`, password null, nonprivileged), `kb_migrator`, separate `kb_first_launch_verifier`, API, worker, and retention logins. Its bytes are checksummed by the same manifest as the three SQL slices. During migration, the migrator temporarily can `SET ROLE kb_launch_owner`; therefore a marker created in that window is not trusted. After the exact manifest is applied, the migrate-only binary performs two committed phases.

Phase 1 invokes the PostgreSQL/bootstrap-owned, `SECURITY DEFINER`, fixed-search-path `pg_catalog.kb_handoff_first_launch_to_verifier()`. Under the migration advisory lock and an exclusive marker lock, it requires the exact three-slice ledger identity and closed maintenance/preflight state, temporarily disables only the marker's user triggers, deletes every possible forged marker, and restores the triggers. Under bootstrap authority it commits migrator `NOLOGIN NOINHERIT PASSWORD NULL`, `REASSIGN OWNED`s every migrator-owned database object to `kb_app_owner`, keeps marker/launch objects with `kb_launch_owner`, explicitly assigns the database and `public` schema to `kb_app_owner`, and drops/revokes all migrator object/default/database/schema privileges and governed memberships. It deliberately does not terminate another backend in that transaction.

The migrate-only process then closes and drops its entire migrator pool. Phase 2 opens the separately configured `KNOWLEDGEBRAIN_BOOTSTRAP_ADMIN_DATABASE_URL` and invokes `pg_catalog.kb_terminate_residual_migrator_backends()`. That fixed-search-path helper is executable only by its bootstrap owner, never by migrator. It treats a PID disappearing after enumeration as benign and performs bounded rescans until the exact migrator backend count is zero. Committed `NOLOGIN` prevents replacement authentication. Only then may the verifier acquire its catalog lock; it independently requires exact-zero migrator backends and the completed handoff topology before marker insertion. The verifier receives temporary `SET`/lock/read authority only to `kb_app_owner` and `kb_launch_owner`, never to migrator.

The marker INSERT guard independently requires `session_user=kb_first_launch_verifier`, handoff-complete app/launch ownership and exact membership topology, exact hash field shapes, and an empty marker. The verifier reads and locks both owner domains, checks the manifest, catalog, ACLs, recursive memberships, table counts, and canonical seed rows before inserting. In the same transaction the bootstrap-owned finalizer removes verifier login/password/memberships/privileges. A verifier replay therefore fails.

Runtime startup compares all four stored digests with deterministic values from the checked-in allow-list and migration manifest. It also rechecks finalized relation/routine/database/schema ownership, all governed login/elevated-role attributes, the absence of direct or recursive governed memberships, and exact database/schema ACLs. A marker without finalization is insufficient.

The verifier and migration runner parse `migration-manifest.toml`; bootstrap bytes and every slice version, name, filename, and checksum must match exactly. Raw embedded UTF-8 bytes are SHA-256 hashed without newline normalization. Catalog deparsing is PostgreSQL-major-sensitive, so production and mandatory CI use the pinned PostgreSQL 16/pgvector image. API, worker, and retention startup only verify the exact manifest identity; only explicit migrate-only mode can execute DDL.
