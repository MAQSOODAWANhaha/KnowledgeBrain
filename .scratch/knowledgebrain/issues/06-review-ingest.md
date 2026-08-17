# 06 — Review：薄上传对照 brain

**What to build:** 确认上传路径与 brain「HTTP 只入队」一致，没有在请求线程里解析。

**Blocked by:** 05 — 薄上传入队

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] HTTP 不在请求线程解析
- [x] task type 字符串为 `document:process`，未改名
- [x] `api` src（lib.rs/main.rs）不含 DocReader/chunker/embedding 调用
- [x] 入队失败仍返回行（200 + parse_status=failed）
- [x] oxana `default` 上能看到入队（Redis 可达时）
- [x] 偏差已记明（对象仍内存；MaxRetry 常量 3，worker 消费在 09）

## Comments

- review 2026-08-15b: 对照 brain「HTTP 只入队」成立。`runtime::enqueue_document_process` 把 `document:process` 推到 oxana queue key `default`。`no_pipeline` 仍绿。未在请求线程 convert。

