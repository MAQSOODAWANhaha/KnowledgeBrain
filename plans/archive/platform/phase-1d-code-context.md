# Code Context

Read-only Phase 1D / first-launch inventory. `phase_1d_runtime_complete` was not flipped.

## Files Retrieved

1. `deploy/first-launch/runtime-completion.toml` (lines 1-9) - empty hashes + `phase_1d_runtime_complete = false`
2. `crates/storage/src/bid_extract_publication.rs` (lines 872-956) - hardcoded `matching_relevant = false`
3. `crates/storage/src/bid_matching.rs` (lines 870-920) - test-only verifier/score seed (`test-v1` / `verify-v1`, `typed-v1-compatible`/`1`)
4. `crates/bid/src/matching/workflow.rs` (lines 140-237) - `FrozenVersionAdapter` lexical retrieve + token-overlap verify
5. `crates/bid/src/matching/handler.rs` (lines 16-39) - production path uses adapter unless `KNOWLEDGEBRAIN_MATCH_FAKES`
6. `crates/bid/src/bin/bid_extract_eval.rs` (lines 1-90) - eval binary exists; no deploy artifacts
7. `deploy/docker-compose.yml` (lines 1-236) - profiles `runtime` + `first-launch` only; API health = `/health`
8. `docker-compose.yml` (lines 1-3) - include delegator to deploy compose
9. `crates/api/src/main.rs` (lines 1-98) - no `/live` or `/ready` routes
10. `crates/domain/src/lib.rs` - no `queue_registry` module
11. `migrations/0012_bid_match_contract.sql` - verifier table exists; no first-launch policy seed file in deploy/
12. Plan refs: `plans/agent-stability-complete-solution.md` (~495-826, 908-920) - expected 1D artifacts

## Key Code

```toml
# deploy/first-launch/runtime-completion.toml
phase_1d_runtime_complete = false
registry_sha256 = ""
evaluation_sha256 = ""
readiness_sha256 = ""
images_sha256 = ""
topology_sha256 = ""
```

```rust
// bid_extract_publication.rs
let matching_relevant = false;
// ...
if matching_relevant {
    crate::bid_matching::mark_project_matching_mutation(tx, target.project_id).await?;
}
```

```rust
// bid_matching.rs test seed (not first-launch catalog seed)
VALUES($1,'test-v1','verify-v1',...)
// score: typed-v1-compatible / 1 / authoritative
```

## Architecture

Current first-launch surface is migrations + `catalog-row-allowlist.toml` + compose `first-launch`/`runtime` profiles. Queue names live as `domain::QUEUE_*` constants (re-exported via `crates/runtime`), not a TOML registry. Matching production handler already wires `FrozenVersionAdapter` (lexical retrieve + local lexical verify). Probe/image-lock/queue-registry/post-launch policy files from the stability plan are not in-tree.

## Checklist (EXISTING vs MISSING)

| # | Item | Exists? | Key symbols | 1-line gap | Sev |
| --- | ------ | --------- | ------------- | ------------ | ----- |
| 1 | `deploy/queue-registry.toml` + `crates/domain/src/queue_registry.rs` | **MISSING** both paths | Queues only as `QUEUE_*` in domain/runtime | No launch-mode registry / exact-closure source | blocker |
| 2 | `deploy/images.lock.json` + image lock tests | **MISSING** lock file; no `*image*lock*` tests | Schema mentions `image_lock_hash` in catalog/tests | No target-aware digest lock or lock-unit tests | blocker |
| 3 | API/worker `/ready` `/live` + `deploy/health/mode-aware-probe.sh` | **MISSING** routes + `deploy/health/` | Compose API healthcheck: `curl .../health`; worker has no HTTP probe | Plan requires mode-aware startup/live vs ready; not implemented | blocker |
| 4 | Matching verifier policy seed + FrozenVersionAdapter in prod | **PARTIAL** | Seed in tests: `policy_name=test-v1`, `policy_version=verify-v1`; score authority `typed-v1-compatible`/`1`. Prod handler uses `FrozenVersionAdapter` unless `KNOWLEDGEBRAIN_MATCH_FAKES`. Verify impl is in-process lexical (`quote_supports_requirement`), not DB policy-driven. | No first-launch/catalog seed of an authoritative verifier row; name/version not frozen as launch artifact | high |
| 5 | publish `matching_relevant=false` | **EXISTING** | `bid_extract_publication.rs:872` always false; mutation mark skipped | Behavior present; leftover `let _ = matching_relevant` (no family-based flag) | low |
| 6 | evaluation artifacts under `deploy/` or `bid_extract_eval.rs` | **PARTIAL** | Binary `crates/bid/src/bin/bid_extract_eval.rs` exists | No signed/eval artifacts under `deploy/`; `evaluation_sha256` empty | high |
| 7 | post-launch additive migration docs | **MISSING** | Plan path `deploy/post-launch/rollout-policy.md` absent; `docs/` has no post-launch/additive-migration runbook | Only plan text + comments that 0001-0009 freeze | medium |
| 8 | `runtime-completion.toml` hashes empty | **EXISTING (empty by design)** | all five `*_sha256 = ""`; `phase_1d_runtime_complete = false` | Hashes remain empty until 1D closures exist — do not flip | info |
| 9 | Phase 2: `queue_registry.rs`, `deploy/docker-compose.yml`, production profile | **PARTIAL** | `deploy/docker-compose.yml` **exists**; profiles `runtime` + `first-launch` only; root compose is include-only | No `queue_registry.rs`; **no `production` profile**; MinIO uses `:latest` (not lock) | high |

## Start Here

Next implementer: `plans/agent-stability-complete-solution.md` §queue registry / probes / image lock (checklist ~908-920), then add the missing files listed above. Do not set `phase_1d_runtime_complete = true`.

## Residual / open

- Authoritative verifier identity for launch is **not** documented as a single seed row; only test insert `test-v1`/`verify-v1` vs score `typed-v1-compatible`/`1`.
- Production matching verify is lexical adapter code, not a versioned verifier-policy snapshot consumer.
- Compose still uses `/health` as service healthcheck (plan forbids bootstrap `/ready` as Compose healthcheck, but `/live` is also absent).
