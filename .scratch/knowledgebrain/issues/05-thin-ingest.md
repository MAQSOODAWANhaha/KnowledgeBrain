# 05 — 薄上传入队

**What to build:** 上传文件后立刻得到 `document_id`，状态 `pending`，队列里有 `document:process`。HTTP **只**做校验、落盘、建行、入队。

**Blocked by:** 02 — Review：仓库骨架；04 — Review：目录与登录

**Status:** done

- [x] `POST .../documents/file` 校验白名单、50MB、sha256 去重四元组，视频拒绝
- [x] 对象写入内存 `objects/{sha256}`，行 `parse_status=pending`、`enable_status=disabled`（未写 MinIO）
- [x] oxana `default` 队列出现一条 `document:process`（`DOCUMENT_PROCESS_MAX_RETRY=3`；worker 消费在 09）
- [x] HTTP 处理路径上无 DocReader、无分块、无 embedding、无 LLM
- [x] 重复文件 409 并返回已有 `document_id`
- [x] 入队失败仍 200 且行标 failed
- [x] 类型/大小/SSRF URL/图片无 VLM / 音频无 ASR → 400

## Comments

- reality: HTTP 仍先写内存队列；Redis 可达时再 `runtime::enqueue_document_process` 到 oxana `default`。对象仍在内存。
- review 2026-08-15: 校验通过；当时无 oxana。
- review 2026-08-15b: oxana 入队测过 compose Redis 16379；task_type 仍是 `document:process`。MaxRetry 常量为 3，未在 Worker derive 上挂（09）。HTTP 不解析仍成立。
