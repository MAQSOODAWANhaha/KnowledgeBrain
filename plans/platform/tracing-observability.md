# Unify process logs on tracing

## Context

Uploading a knowledge or tender file currently looks idle: Docker logs stay empty until a job finishes or dies. The pipeline still talks through `eprintln!` / raw `stdout.write`, so there is no level filter, no structured fields, and no shared format between api and worker.

This change does two things:

1. **One logging stack** — `tracing` events + `tracing-subscriber` fmt/env-filter, initialized once in api/worker.
2. **Process logs you can follow** — convert, chunk, VLM, extract/eval sections emit start → progress → done/fail lines with ids, not bodies.

Queue depth is already a different surface: admin `GET /api/v1/ops/oxana` and UI `/api/v1/ops/oxana/web`. That panel stays and is the only queued/running/retrying/dead transport view；业务查询只展示target的`pending|completed|failed|cancelled`与bounded last error，不再维护durable-dispatch backlog/oldest-due metrics。

`document_processing_spans` (obs crate) stays the product timeline for the SPA. Tracing is for operators watching containers.

## Approach

Workspace already has `tracing` + `tracing-subscriber` (fmt, env-filter, std, ansi). `runtime::init_tracing()` exists and api/worker `main` already call it. **Do not add a second subscriber.** Finish that path.

Rules:

- Production crates emit `tracing::{info,warn,error,debug}` only. No new `eprintln!`.
- Default `RUST_LOG=info` (compose `x-app-env` already sets this). Compact human fmt on **stdout** so `docker logs` works.
- `with_target(true)` so lines show `bid` vs `worker` vs `docparser`.
- Fields are typed: `document_id`, `file`, `engine`, `run_id`, `section_key`, `status`. Never log API keys, full markdown, clause quote/body, image bytes, or data URLs. Truncate object keys.
- Events first. Add `#[instrument(skip_all, fields(...))]` only on the long-running entrypoints listed below — not on every helper.
- Tests and CLI keep `println!` / skip `eprintln!` (`skip: postgres down`, `bid_extract_eval`).
- No in-app log viewer. No JSON fmt this phase (can add `RUST_LOG_FORMAT=json` later without changing events).

Noise control:

| Level | When |
| --- | --- |
| `info` | Stage start/done, enqueue, per tender **section** result, per knowledge **image** OCR result, run summary |
| `warn` | thin / tables_flat, VLM missing, fallback, Oxana retry/dead/resurrect |
| `error` | Stage/run failure with bounded message |
| `debug` | Per-family agent round, tool-call count, span sweep — `RUST_LOG=bid=debug` |

## Files to modify

| File | Change |
| --- | --- |
| `crates/runtime/src/lib.rs` | Keep single `init_tracing()`; disable ansi in Docker if `NO_COLOR`/`TERM=dumb` (compose can set `NO_COLOR=1`) |
| `crates/api/src/main.rs` | Drop unused `log_ready`; `api exiting` → `tracing::info!` |
| `crates/worker/src/main.rs` | Drop unused `log_line`; `worker exiting` → `tracing::info!` |
| `crates/bid/src/lib.rs` | Replace all production `eprintln!`; add convert/extract/section events |
| `crates/bid/src/extraction/mod.rs` | `debug` family-agent / sweep; `warn` hybrid fallback |
| `crates/worker/src/consume.rs` | Knowledge convert/chunk/embed/fanout/image/postprocess + bid workers |
| `crates/docparser/src/images.rs` | Remote rewrite cap/fail → `warn!` |
| `crates/docparser/src/lib.rs` | Convert start/fallback (`anydoc_fallback`) at `info`/`warn` |
| `crates/enrichment/src/lib.rs` | `describe_image` fail/not-configured → `warn!`/`error!` (no image payload) |
| `crates/api/src/bid_routes.rs` | Bid DB/query failures → `error!`; target创建与enqueue accepted/unavailable at `info`/`warn` |
| `crates/runtime/src/jobs.rs` | Knowledge enqueue at `debug`；Bid transport 只记录 target kind/id/revision与Oxana job ID |
| `crates/graph/src/neo4j.rs` | `skip: neo4j` → `debug!` |
| `crates/storage/src/s3.rs` | `skip: s3` → `debug!` |
| `deploy/.env.example` | Document `RUST_LOG=info` |
| `docs/research/repository-implementation-snapshot.md` §9 | Note tracing + oxana split |
| `Cargo.toml` / crate `Cargo.toml` | Already wired for runtime/api/worker/bid; add `tracing` to **docparser**, **enrichment**, **graph**, **storage** (storage only if s3 log moves) |

Do **not** touch: `crates/*/tests`, `storage/src/persist.rs` skip lines, `bid/src/bin/bid_extract_eval.rs`, `runtime/examples/`, `docparser/build.rs` cargo instructions.

## Reuse

- `runtime::init_tracing()` — `crates/runtime/src/lib.rs`
- Compose `RUST_LOG` — `deploy/docker-compose.yml` `x-app-env`
- Oxana dashboard — `crates/api/src/routes.rs` nest `/api/v1/ops/oxana/web`, JSON `/api/v1/ops/oxana`
- Document timeline — `GET /api/v1/documents/{id}/timeline` + `obs` spans (`SPAN_DOCREADER` … `SPAN_POSTPROCESS`)
- Quality gates already in bid: `conversion_is_thin`, `conversion_tables_flat`, `conversion_quality_note`
- Bounded errors: `bounded_error` in `crates/bid/src/lib.rs`
- Engine choice: `domain::parser_engine_for` / `docparser::convert_to_markdown`
- VLM gate: `domain::vlm_configured` / `vlm_endpoint_ready`

## Event catalog

### Knowledge parse (`worker::consume::convert_document`)

| Event | Level | Fields |
| --- | --- | --- |
| `parse convert start` | info | `document_id`, `file`, `engine`, `attempt` |
| `parse convert reuse` | info | `document_id`, `md_bytes` |
| `parse convert done` | info | `document_id`, `parser`, `md_bytes`, `images`, `anydoc_fallback` |
| `parse convert fail` | error | `document_id`, `stage=docreader`, `error` (bounded) |
| `parse chunking done` / `reuse` | info | `document_id`, `chunks` |
| `parse embedding done` | info | `document_id`, `chunks` |
| `parse multimodal hold` | warn | `document_id`, `reason=vlm not configured`, `images` |
| `parse multimodal enqueue` | info | `document_id`, `images` |
| `parse image done` / `fail` | info / warn | `document_id`, `image_key` (truncated), `ocr`, `caption` |
| `parse postprocess start/done` | info | `document_id`, `clone_keep` |
| `parse completed` / `finalizing` | info | `document_id`, `index_ready` |

Instrument: `convert_document`, `process_image_pg`, `process_post_process`.

### Tender convert + extract (eval)

| Event | Level | Fields |
| --- | --- | --- |
| `bid_convert start` | info | `document_id`, `file` |
| `bid_convert parsed` | info | `document_id`, `file`, `parser`, `images`, `anydoc_fallback` |
| `bid_convert quality` | warn | `document_id`, `note=thin\|tables_flat` |
| `bid_convert done` / `fail` | info / error | `document_id`, `retryable` on fail |
| `bid extraction target created` | info | `document_id`, `target_id`, `project_id`, `target_revision` |
| `bid_extract start` | info | `run_id`, `document_id`, `file`, `sections`, `mode`, `model` |
| `bid_extract section done` | info | `run_id`, `document_id`, `section_key`, `clauses`, `rounds`, `fallbacks` |
| `bid_extract section fail` | warn | `run_id`, `document_id`, `section_key`, `error` (bounded) |
| `bid_extract document done` | info | coverage counts, `partial_failure` |
| `bid_extract run fail` | error | `run_id`, `document_id`, `category` |
| `bid_extract family` | debug | `section_key`, `family`, `termination`, `rounds` |
| section retry persist fail | error | existing site in `retry_section` |

Instrument: `convert_document`, `extract_run`, `extract_one_document`.

### API / runtime

| Event | Level | Fields |
| --- | --- | --- |
| enqueue knowledge process / Bid delivery | info | `document_id`, `job` 或 `target_kind`,`target_id`,`target_revision`,`oxana_job_id` |
| `bid database unavailable` | error | `error` |
| `bid {op} failed` | error | `operation`, `error` |
| stale target revision noop | warn | `target_kind`,`target_id`,`target_revision` |
| Oxana retry/dead/resurrect | warn | 使用 Oxana 原生日志与 metrics，不在业务表镜像 phase |

## eprintln inventory

**Replace (production):** all `eprintln!` in `bid/src/lib.rs`, `worker/src/consume.rs` (non-test), `api/src/routes.rs` (three bid helpers), `docparser/src/images.rs`, `graph/src/neo4j.rs`, `storage/src/s3.rs`.

**Keep:** every `skip: postgres down` / `skip: redis` / `skip persist test` under `#[cfg(test)]` or `mod tests`; CLI `bid_extract_eval`; cargo `println!` in build.rs.

## Steps

- [ ] Confirm subscriber is once-only (`try_init`); set compose `NO_COLOR=1` if ansi leaks into Dockge.
- [ ] Add `tracing` deps to docparser / enrichment / graph (and storage if s3 log moves).
- [ ] Convert api/worker leftover stdout helpers.
- [ ] Knowledge convert path: start/reuse/done/fail, chunk, embed, multimodal hold/enqueue, image, postprocess.
- [ ] Bid convert + extract: start/parsed/quality/done/fail, per-section, run summary; family at debug.
- [ ] API bid error helpers + idempotent target create/enqueue。
- [ ] Duplicate/stale target revision 与 poison `warn!`；retry/dead/resurrect 使用 Oxana 原生观测。
- [ ] Doc: `docs/research/repository-implementation-snapshot.md` §9 one paragraph; `.env.example` `RUST_LOG`.
- [ ] Rebuild **api + worker** images (runtime is in both). Do not wipe volumes.
- [ ] No `#[allow(clippy::…)]` for this work.

## Verification

- `cargo test -p bid -p worker -p api --offline` (existing tests; logging must not change behavior).
- `docker logs -f knowledgebrain-worker` while uploading one knowledge docx: see convert start → parsed → chunk → (multimodal or hold) → postprocess.
- Upload one tender docx: convert start → parsed → queued extract → extract start → **one line per section** → document done.
- `RUST_LOG=bid=debug` shows family agents; default info does not flood.
- Admin opens `http://localhost:28080/api/v1/ops/oxana/web` — queues still load.
- Grep production `eprintln!` in `crates/{api,worker,bid,docparser,enrichment}/src` is empty except tests.

## Out of scope

- MinerU/Paddle, new parse engine, SPA log console, Langfuse, Prometheus series, JSON log shipper.
- Changing extract/match product rules.
- Wiping compose volumes.
