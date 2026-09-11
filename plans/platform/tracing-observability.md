# Unify process logs on tracing

## Context

API and worker already initialize the shared tracing subscriber. This plan verifies and completes stage-level process visibility for knowledge and tender processing; it does not assume all current paths still use raw output or that a second logging stack is needed.

This change does two things:

1. **One logging stack** — `tracing` events + `tracing-subscriber` fmt/env-filter, initialized once in api/worker.
2. **Process logs you can follow** — convert, chunk, VLM and bidding Request stages emit start → progress → done/fail lines with ids, not bodies.

Queue depth is already a different surface: admin `GET /api/v1/ops/oxana` and UI `/api/v1/ops/oxana/web`. That panel stays and is the only queued/running/retrying/dead transport view；知识业务查询保留自己的状态；投标 Request 使用 `pending|succeeded|failed` 与 bounded last error，不再维护durable-dispatch backlog/oldest-due metrics。

`document_processing_spans` (`knowledge::obs`) stays the product timeline for the SPA. Tracing is for operators watching containers.

招投标日志只围绕当前 Request/轮次、SourceUnit、候选、保存文件版本和导出结果定位，不恢复旧匹配/组卷任务。ONLYOFFICE 主链已有部分实现与隔离验证；保存请求、落盘和转换应分开观测，业务成功以真实持久化结果为准。当前接缝见 [运行手册](../../docs/bidding/backend-runbook.md)，编辑与出件顺序见 [计划](../bidding/onlyoffice-integration.md)；Agent 请求、工具、检查点及用量指标按 [Rig 方案](../bidding/agent-runtime-rig.md)接入既有 tracing，不另建观测服务。

## Approach

Workspace already has `tracing` + `tracing-subscriber` (fmt, env-filter, std, ansi). `platform::init_tracing()` exists and api/worker `main` already call it. **Do not add a second subscriber.** Finish that path.

Rules:

- Production crates emit `tracing::{info,warn,error,debug}` only. No new `eprintln!`.
- Default `RUST_LOG=info` (compose `x-app-env` already sets this). Compact human fmt on **stdout** so `docker logs` works.
- `with_target(true)` identifies the actual emitting module, such as `bidding`, `worker`, `api` or `docparser`.
- Fields are typed: `document_id`, `file`, `engine`, `run_id`, `section_key`, `status`. Never log API keys, full markdown, clause quote/body, image bytes, or data URLs. Truncate object keys.
- Events first. Add `#[instrument(skip_all, fields(...))]` only on the long-running entrypoints listed below — not on every helper.
- Keep test skip diagnostics and existing helper/CLI output protocols; do not treat their stdout payloads as operator logs.
- No in-app log viewer. No JSON fmt this phase (can add `RUST_LOG_FORMAT=json` later without changing events).

Noise control:

| Level | When |
| --- | --- |
| `info` | Stage start/done, enqueue, per tender **section** result, per knowledge **image** OCR result, run summary |
| `warn` | thin / tables_flat, VLM missing, fallback, Oxana retry/dead/resurrect |
| `error` | Stage/run failure with bounded message |
| `debug` | Request/stage progress, SourceUnit processing, replay and result publication; correlate request/project identity and bounded status, never document bodies |

## Files to modify

| File | Change |
| --- | --- |
| `crates/platform/src/lib.rs` | Keep single `init_tracing()`; disable ansi in Docker if `NO_COLOR`/`TERM=dumb` (compose can set `NO_COLOR=1`) |
| `crates/api/src/main.rs` | Verify the existing tracing initialization and shutdown event; do not recreate removed stdout helpers |
| `crates/worker/src/main.rs` | Verify existing initialization/shutdown and remaining helper diagnostics without changing helper output protocols |
| `crates/worker/src/consume.rs` | Knowledge convert/chunk/embed/fanout/image/postprocess + bid workers |
| `crates/docparser/src/images.rs` | Remote rewrite cap/fail → `warn!` |
| `crates/docparser/src/lib.rs` | Convert start/fallback (`anydoc_fallback`) at `info`/`warn` |
| `crates/knowledge/src/enrichment/mod.rs` | `describe_image` fail/not-configured → `warn!`/`error!` (no image payload) |
| `crates/platform/src/jobs.rs` | Knowledge enqueue at `debug`；Bid transport 只记录 target kind/id/revision与Oxana job ID |
| `crates/knowledge/src/graph/neo4j.rs`、`crates/platform/src/s3.rs` | Inspect production paths only; the existing `skip: neo4j` / `skip: s3` test diagnostics stay unchanged |
| `deploy/.env.example` | Document `RUST_LOG=info` |
| `docs/research/repository-implementation-snapshot.md` §9 | Note tracing + oxana split |
| `Cargo.toml` / crate `Cargo.toml` | 核对现有 platform/api/worker/bidding/knowledge/docparser 日志依赖，不按旧 crate 清单重复添加 |

Do **not** touch test behavior under `crates/*/tests` or inline test modules, existing helper/CLI output protocols, or Cargo build instructions in `crates/docparser/build.rs`.

## Reuse

- `platform::init_tracing()` — `crates/platform/src/lib.rs`
- Compose `RUST_LOG` — `deploy/docker-compose.yml` `x-app-env`
- Oxana dashboard — `crates/api/src/routes.rs` nest `/api/v1/ops/oxana/web`, JSON `/api/v1/ops/oxana`
- Document timeline — `GET /api/v1/documents/{id}/timeline` + `knowledge::obs` spans (`SPAN_DOCREADER` … `SPAN_POSTPROCESS`)
- Engine choice: `knowledge::parser_engine_for` / `docparser::convert_to_markdown`
- VLM configuration: `platform::vlm_configured` / `platform::vlm_endpoint_ready`

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

### 招投标来源与文档接入

现有 `crates/bidding/src/tender_process.rs`、`content_runtime.rs` 与 `crates/api/src/bid_v2_routes.rs` 是事件定位接缝；记录 request/project/document identity、阶段、重放、失败码，不记录原文/证件或 secret。来源、候选和当前指针发布分别记录，不从 transport ACK 推断业务成功。

ONLYOFFICE 保存/转换日志随接入切片实现：区分会话、新稿轮次、forcesave 请求、持久化版本和出件版本；旧轮/乱序拒绝与存储/转换失败可定位，不能用一个 done 覆盖整个保存链。

### API / runtime

| Event | Level | Fields |
| --- | --- | --- |
| enqueue knowledge process / Bid delivery | info | `document_id`, `job` 或 `target_kind`,`target_id`,`target_revision`,`oxana_job_id` |
| `bid database unavailable` | error | `error` |
| `bid {op} failed` | error | `operation`, `error` |
| stale target revision noop | warn | `target_kind`,`target_id`,`target_revision` |
| Oxana retry/dead/resurrect | warn | 使用 Oxana 原生日志与 metrics，不在业务表镜像 phase |

## eprintln inventory

**Inspect remaining production diagnostics:** `crates/worker/src/main.rs`, `crates/worker/src/consume.rs`, `crates/api/src/routes.rs`, `crates/api/src/bid_v2_routes.rs`, `crates/docparser/src/images.rs`, `crates/knowledge/src/graph/neo4j.rs`, `crates/platform/src/s3.rs`. Existing tracing events need no conversion; first distinguish production diagnostics from inline tests and helper protocol output.

**Keep:** skip diagnostics under `#[cfg(test)]` or `mod tests`, existing helper/CLI output protocols, and Cargo `println!` in `crates/docparser/build.rs`.

## Steps

- [ ] Confirm subscriber is once-only (`try_init`); set compose `NO_COLOR=1` if ansi leaks into Dockge.
- [ ] 核对现有 tracing 依赖及调用，不按已合并的旧 crate 名重复加依赖。
- [ ] Convert api/worker leftover stdout helpers.
- [ ] Knowledge convert path: start/reuse/done/fail, chunk, embed, multimodal hold/enqueue, image, postprocess.
- [ ] API bid error helpers + idempotent target create/enqueue。
- [ ] Duplicate/stale target revision 与 poison `warn!`；retry/dead/resurrect 使用 Oxana 原生观测。
- [ ] Doc: `docs/research/repository-implementation-snapshot.md` §9 one paragraph; `.env.example` `RUST_LOG`.
- [ ] Rebuild **api + worker** images (runtime is in both). Do not wipe volumes.
- [ ] No `#[allow(clippy::…)]` for this work.

## Verification

- `cargo test -p bidding -p worker -p api --offline` (existing tests; logging must not change behavior).
- `docker logs -f knowledgebrain-worker` while uploading one knowledge docx: see convert start → parsed → chunk → (multimodal or hold) → postprocess.
- Upload one tender docx in an authorized environment: correlate Request, conversion/source publication and bounded failures; do not equate queue ACK with business success.
- With `RUST_LOG=info,bidding=debug,api=debug,worker=debug`, correlate Request/stage, SourceUnit and result-publication events by identity and bounded status; transport ACK is not publication success. Default info remains bounded and neither level exposes bodies or credentials.
- Admin opens `http://localhost:28080/api/v1/ops/oxana/web` — queues still load.
- Inspect remaining `eprintln!` in current API/worker/bidding/docparser/knowledge source paths: ordinary operator diagnostics use tracing; tests and documented helper/CLI protocol diagnostics remain explicit exceptions, without redirecting protocol payloads into logs.

## Out of scope

- MinerU/Paddle, new parse engine, SPA log console, Langfuse, Prometheus series, JSON log shipper.
- Changing extract/match product rules.
- Wiping compose volumes.
