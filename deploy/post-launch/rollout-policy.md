# Post-launch rollout policy

This is the reviewed post-exposure policy for KnowledgeBrain. It applies only after
`production_launch_state` has a non-NULL traffic-exposure marker. First-launch
cutover, `kb_migrator`, and prelaunch reset remain permanently closed.

This file is documentation only. It does not implement SQL, grant a login, or
flip `phase_1d_runtime_complete`.

## Binding rules

- New migration identity is `kb_additive_migrator`. It is a distinct login from
  `kb_migrator` and never inherits first-launch SET/`kb_launch_owner` authority.
- NEVER re-enable `kb_migrator`. After first-launch handoff the role stays
  `NOLOGIN NOINHERIT PASSWORD NULL` with no object/default/database/schema
  privileges and no governed memberships.
- NEVER re-enable first-launch reset. Prelaunch
  `suspend_open_unrouted` / `reopen_unrouted` / `authorize_pre_exposure_reset`
  stay rejected for the life of any exposure, revocation, or first-request
  marker.
- Releases are non-destructive: binary/config/topology change and ledgered
  additive forward migration only. No drop/truncate/recreate of production
  state, no ledger rewind, no launch-marker clear, no reset-authority restore.
- The only out-of-band control migration permitted after first launch is
  `migrations/0013_postlaunch_migration_control.sql`. That name is required.
  The name MUST NOT be `0013_bid_backfill`. This slice does not add that file.

## Roles

| Role | Login | Purpose after exposure | Allowed | Forbidden |
| --- | --- | --- | --- | --- |
| `kb_app_owner` | no | Durable owner of application objects after handoff | Own relations/routines assigned at first launch | Login, `INHERIT`, password, first-launch reset, SET to `kb_launch_owner` |
| `kb_launch_owner` | no | Owner of launch/marker/gate objects | Remain the owner of launch-control objects | Login, direct DML from operators, restoring `kb_migrator` |
| `kb_migrator` | no | Retired first-launch migrator | Remain disabled evidence of handoff | ANY re-LOGIN, password, membership, privilege, SET ROLE, object ownership |
| `kb_first_launch_verifier` | no | Retired one-shot catalog verifier | Remain disabled after marker insert | Replay, login, membership restore |
| `kb_runtime_api` | yes | Production API | Runtime DML/procedures granted at first launch | DDL, ledger writes, launch-state DML, grant/revoke, reset |
| `kb_runtime_worker` | yes | Production worker | Runtime DML/procedures granted at first launch | DDL, ledger writes, launch-state DML, grant/revoke, reset |
| `kb_additive_migrator` | yes, only inside a dual-control grant window | Post-launch additive migrator | Apply the already-reviewed next additive ledger file after 0013 control exists | First-launch procedures, SET `kb_launch_owner`, drop/truncate, rewind ledger, clear markers, enable `kb_migrator` |

`kb_additive_migrator` is created only by `0013_postlaunch_migration_control.sql`.
Until that control migration exists, no post-launch DDL role is authorized.

## Control migration: `0013_postlaunch_migration_control.sql`

`0013_postlaunch_migration_control.sql` is the only out-of-band control
migration. It is not a Bid backfill and MUST NOT be named
`0013_bid_backfill`. It is not a domain/data migration.

When later implemented it may only:

1. Create `kb_additive_migrator` as a dedicated identity (`NOINHERIT`, no
   password until a dual-control grant window, no membership in `kb_migrator`
   or `kb_launch_owner`).
2. Create the dual-control grant/restore-disable helpers used below.
3. Record that first-launch `kb_migrator` remains `NOLOGIN` and that prelaunch
   reset procedures remain non-callable.

It MUST NOT create `schema_flags`, MUST NOT insert compatibility markers, and
MUST NOT appear in the first-launch manifest `1–8, 10, 11, 12`. First-launch
repeat-apply continues to expect that exact head. 0013 is applied later, once,
under this policy, after exposure.

## Dual-control artifact

Every grant window and every additive apply requires one dual-control artifact
on disk before any SQL session is opened:

```text
deploy/post-launch/dual-control/<release_id>.toml
```

Required fields:

| Field | Constraint |
| --- | --- |
| `release_id` | Non-empty literal; matches the file name |
| `ticket` | Change ticket |
| `actor_a` | Authorized person A |
| `actor_b` | Authorized person B; MUST differ from `actor_a` |
| `signed_at` | RFC3339 UTC timestamp of the later signature |
| `control_migration` | Exact `0013_postlaunch_migration_control.sql` when creating the role; otherwise the already-applied control head |
| `target_migration` | Next additive ledger file name, or empty when the change is grant/restore only |
| `target_checksum_sha256` | Raw UTF-8 SHA-256 of that file, or empty when `target_migration` is empty |
| `expected_schema_manifest_sha256` | Exact checked-in schema manifest SHA-256 that must already be verified |
| `image_lock_hash` | Current reviewed image-lock digest |
| `registry_hash` | Current reviewed `deploy/queue-registry.toml` digest |
| `intended_state_hash` | Current intended feature-state digest |
| `topology_hash` | Current rendered topology digest |
| `grant_window_seconds` | Positive bound; session longer than this fails closed |
| `reason` | Bounded operator reason |

Both actors sign the same bytes. A single signature, reused `release_id`, or
any field mismatch against live ledger/image/registry/intended-state/topology
facts fails closed.

## Grant SQL

The following is the only grant shape later code may emit. It is not executed
in this slice. Substitute the dual-control `release_id` and the operator
session that will apply the reviewed file.

```sql
-- Requires deploy/post-launch/dual-control/<release_id>.toml
-- and an already-applied 0013_postlaunch_migration_control.sql.
BEGIN;
SELECT pg_catalog.kb_postlaunch_grant_additive_migrator(
  release_id := :'release_id',
  actor_a := :'actor_a',
  actor_b := :'actor_b',
  expected_schema_manifest_sha256 := :'expected_schema_manifest_sha256',
  grant_window_seconds := :'grant_window_seconds'
);
COMMIT;
```

The helper MUST:

- reject unless `actor_a` and `actor_b` are distinct and match the artifact
- reject unless exposure is committed and first-launch reset remains revoked
- reject unless `kb_migrator` is still `NOLOGIN NOINHERIT PASSWORD NULL`
- grant `kb_additive_migrator` login only for `grant_window_seconds`
- grant that role only the privilege to apply the named next additive file
- refuse `SET ROLE kb_launch_owner` and refuse any grant to `kb_migrator`

## Apply rules

1. Confirm production route policy, backups, and a forward/rollback-compatible
   rehearsal exist for this `release_id`. Rollback means a newer additive
   compensating migration or a binary/config revert, never a ledger rewind.
2. Load the dual-control artifact. Verify both signatures and exact live hashes.
3. Apply `0013_postlaunch_migration_control.sql` once if and only if it is the
   next reviewed head and the artifact `target_migration` names that file.
   After 0013 exists, later additive files are ordinary ledgered migrations.
4. Open the grant window with the grant SQL above.
5. Apply exactly one reviewed additive file whose name and raw-bytes SHA-256
   match the artifact. Checksum is SHA-256 of raw embedded UTF-8 bytes with no
   newline normalization. The runner holds the same advisory lock used by
   first launch and writes `{version,name,checksum,applied_at}` only after that
   file succeeds.
6. Compatibility rule: ship readers/writers that accept the old and new shape
   before switching traffic. Do not drop, truncate, or recreate production
   tables to “make room” for the new shape.
7. Immediately run restore-disable SQL. A successful apply with a still-open
   grant window is a failed release.
8. Record release evidence (release/evidence ID, ticket, both actors, new
   ledger head, image/registry/topology/intended-state diffs, readiness and
   canary results). Post-launch drain/maintenance/reopen uses independent
   non-destructive procedures and release/gate epochs. It MUST NOT call or
   extend first-launch procedures.

## Restore-disable SQL

```sql
BEGIN;
SELECT pg_catalog.kb_postlaunch_restore_disable_additive_migrator(
  release_id := :'release_id'
);
-- Fail closed unless the following are all true in the same transaction:
--   kb_additive_migrator: NOLOGIN NOINHERIT PASSWORD NULL
--   kb_migrator:          still NOLOGIN NOINHERIT PASSWORD NULL
--   no session holds kb_additive_migrator
--   no extra database/schema/object/default privileges remain
COMMIT;
```

If the helper is unavailable, the operator still ends the window with the
same end state:

```sql
ALTER ROLE kb_additive_migrator NOLOGIN NOINHERIT PASSWORD NULL;
REVOKE ALL ON DATABASE current_database() FROM kb_additive_migrator;
REVOKE ALL ON SCHEMA public FROM kb_additive_migrator;
```

Restore-disable is mandatory after success, failure, or abandoned apply.
Crash/retry may repeat restore-disable; it MUST NOT reopen `kb_migrator`.

## Fail-closed checks

Stop before any SQL or traffic change when any of the following hold:

- dual-control artifact missing, unsigned, single-signed, or stale
- `actor_a` equals `actor_b`
- live `expected_schema_manifest_sha256` / image / registry / intended-state /
  topology hash differs from the artifact
- target file name or raw-bytes SHA-256 differs
- proposed file is named `0013_bid_backfill` or any other backfill alias
- `kb_migrator` is LOGIN, has a password, holds memberships, or owns objects
- a first-launch reset/suspend/reopen procedure is callable or requested
- exposure/revocation/first-request markers are NULL (still prelaunch)
- grant window expired or restore-disable did not leave both migrator roles
  `NOLOGIN NOINHERIT PASSWORD NULL`
- catalog scan finds `schema_flags`, `0013_bid_backfill`, or a compatibility
  marker table/row
- apply would drop/truncate/recreate production state or rewind the ledger

## MUST NOT

- Re-enable `kb_migrator` (LOGIN, password, membership, privilege, ownership,
  or `SET ROLE`).
- Re-enable first-launch reset, or call
  `suspend_open_unrouted` / `reopen_unrouted` / `authorize_pre_exposure_reset`
  after exposure.
- Name the control migration `0013_bid_backfill` or reintroduce
  `schema_flags` / `0008_backfill.sql` / frozen-0009 aliases.
- Implement `0013_postlaunch_migration_control.sql` in this documentation
  slice, or add any 0013 file to the first-launch manifest.
- Use `kb_runtime_api` / `kb_runtime_worker` / `kb_first_launch_verifier` as
  a migration identity.
- Drop, truncate, recreate, or restore production volumes/tables to apply an
  additive change.
- Rewind or rewrite ledger rows, clear launch markers, or restore reset
  authority.
- Ship a destructive data migration under this policy. That requires a
  separately approved data/backup/recovery policy.
- Flip `phase_1d_runtime_complete` or fill runtime-completion hashes from this
  file.
