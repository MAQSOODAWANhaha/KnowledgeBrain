# 17 — 摘要与问题生成

**What to build:** 有正文的文档会生成摘要和分批问题；计数归零后 completed；生成问题可被检索。

**Blocked by:** 14 — Review：post_process 与 housekeeping 对照 brain

**Status:** done

- [x] `expectedSubtasks` 含 summary=1、question 按 text 块每 20 一批
- [x] 摘要 24k 截断、短文 summary_status=failed 仍 drain
- [x] 问题只对 `text` chunk；payload 带 chunk_ids
- [x] 不 fail 父 `parse_status`
- [x] 对照 brain prompt 模板 / 真实 chat 模型

## Comments

- `generate_summary.yaml` / `generate_questions.yaml` 已作为 system prompt 落地（`enrichment::SUMMARY_PROMPT` / `QUESTIONS_PROMPT`）。
- `sampleLongContent`：24k rune，头 60% / 中 20% / 尾 20%，插入 `[...content omitted...]`。
- 正文 <200 rune 时折入 `<image_ocr>` / `<image_caption>`；<8 rune 则 `summary_status=failed` 仍 `FinalizeSubtask`。
- `attemptSuperseded`（`documents.attempt > job.attempt` 且 job.attempt>0）直接 return，不 `FinalizeSubtask`。
- `stub-chat` 或未设 `KNOWLEDGEBRAIN_CHAT_BASE_URL`：抽取式 280 rune stub；有 URL 则 OpenAI `/v1/chat/completions`。
- 偏差：无 Langfuse；无独立 LatestAttempt 表（对照 `document.attempt` + payload `attempt`）。
- question payload 带 `chunk_ids` + `prev_ids`/`next_ids`；生成时拼前后块上下文。
- `question_generation_config`：默认 3、max 10、0 当默认；hydrate / PATCH / clone / `process_overrides` 合并。`generate_questions` 用 `version.question_count()`；`custom_instructions` 按 brain 标签追加到 prompt。
- summary：按 `start_at` 拼接；不足文本用去图标记后的实文（阈值 8）；PG `attemptSuperseded` 不再 FinalizeSubtask。chat URL 失败可重试，最后一次回落首块 500 rune。summary chunk 挂第一块 text；重试先删旧 summary。不 fail 父 `parse_status`。
- question（5.9）：`generate_questions_with` 吃 payload `prev_ids`/`next_ids`，prompt 拼 `<surrounding_context>`；空/非 text 跳过；chat HTTP 失败跳过该块不 fail 父文档；重试先删旧 question 子块。`QuestionOutcome::Superseded` 不 `FinalizeSubtask`（内存 + PG）。`QuestionWorker` 传 neighbors；最后一次失败仍 drain。解析题行 `len>5` 对齐 brain。
