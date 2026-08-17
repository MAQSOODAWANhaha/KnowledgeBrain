# 09 — Worker 解析成 Markdown

**What to build:** 上传后的任务由 worker 跑 convert，文档进入 `processing`，能看到解析 span；仍不可依赖向量检索。

**Blocked by:** 06 — Review：薄上传对照 brain；08 — Review：DocReader 对照 brain

**Status:** done

- [x] worker **进程**消费 oxana `default` / `document:process`（`run_core`；无 Redis 则仍等 SIGTERM）
- [x] 引擎路由：`builtin` 禁止 simple 兜底；精确字符串匹配
- [x] passages 跳过 convert（内存 `drain` 路径）
- [x] 非 simple：有 `DOCREADER_ADDR` 走 tonic ReadStream；无 addr 立即 failed、不重试
- [x] 音频：convert 标 `IsAudio` 并保留字节；无 ASR 配置立即 failed；`stub-asr` / OpenAI-compatible `/v1/audio/transcriptions` 写回 Markdown；转写失败可重试
- [x] span `docreader` 有起止（写 `document_processing_spans`）
- [x] HTTP 仍不调用解析

## Comments

- reality: simple / tonic convert 后走 ASR。`asr_config.enabled` + `asr_model_id`。无配置文案与 brain 一致。HTTP 仍不解析。
- review 2026-08-15c/e: 消费 + tonic。
- review 2026-08-15f: ASR 写回 + 任务 2h timeout 在 `process()`。
