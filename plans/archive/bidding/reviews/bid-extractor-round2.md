# Bid Extractor Round 2 — Synthesis

Three fresh-context, read-only reviewers inspected concurrency/atomicity, extraction quality/evaluation, and storage/API/documentation contracts. Their complete reports remain in:

- `bid-extractor-round2.round2-correctness.md`
- `bid-extractor-round2.round2-security-quality.md`
- `bid-extractor-round2.round2-contracts.md`

## Accepted and fixed

- Replaced start-time stale detection with periodically renewed heartbeat leases for full runs and Section retries.
- Added section-specific lock identity and token-conditioned status, persistence, finish, and release operations.
- Fenced reclaimed owners from late writes and cleaned stale retry state.
- Required open projects during claims and hardened project-end behavior.
- Added fresh-schema lease constraints/FKs and propagated migration SQL failures from storage.
- Separated non-quotable table context from quotable source text.
- Made quote validation literal and server-canonicalized persisted text to the verified quote.
- Split numbered/coordinated requirement units without dropping numbered source requirements.
- Tightened tool arguments and termination diagnostics.
- Reworked evaluator assignment to exact normalized labels or explicit aliases with one-to-one matching; unlabeled runs are `NOT_EVALUATED`.
- Expanded must/negation/prohibition policy terms and tests.
- Scoped document retry/delete by both project and document IDs.
- Propagated failures from the reviewed document/extraction API mutations.

## Rejected or deferred

- Rejected an old-volume upgrade migration because the approved deployment policy allows clearing volumes and recreating the consolidated schema.
- Deferred global LLM budgets, expanded retry UI, and other non-blocking UX/type improvements.

## Outcome

Focused checks, workspace tests, Clippy, web build, and fresh PostgreSQL migration smoke passed. Material edge cases found by the final Round-3 review were subsequently fixed and are recorded in `bid-extractor-round3.md`.
