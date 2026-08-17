# 10 — Review：convert 对照 brain

**What to build:** 对照 brain `ProcessDocument` / `convert` / `resolveDocReader`，确认引擎表、超时、失败重试无偏差。

**Blocked by:** 09 — Worker 解析成 Markdown

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 引擎表与 `is_simple_format` 集合一致
- [x] DocReader 子超时 30min（`DOCREADER_TIMEOUT`）
- [x] 任务超时 2h（`DOCUMENT_PROCESS_TIMEOUT_SECS` 包住 convert）
- [x] 非 simple / 无 reader：立即 failed、不重试
- [x] 偏差已记明

## Comments

- reality: 路由 / ReadStream 30min / 任务 2h / 无 reader 立即 failed / ASR 写回均已对照 brain。mineru/paddle HTTP 仍未做（无引擎则 failed）。
- review 2026-08-15f: 2h 与 ASR 已核对。
