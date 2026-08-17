# 18 — Review：摘要与问题对照 brain

**What to build:** 对照 brain `expectedSubtasks`、getSummary、question 分批，确认计数与失败语义无偏差。

**Blocked by:** 17 — 摘要与问题生成

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 公式用例：25 text + wiki+graph → 29；25+3 ocr → question 按 25 批
- [x] `attemptSuperseded` 不 FinalizeSubtask
- [x] 偏差已记明

## Comments

- 公式对。prompt / sampleLongContent / attempt 抢占 / stub-or-HTTP chat 已核对。
- 单测 `superseded_summary_does_not_finalize`：attempt=2 的文档跑 job_attempt=1，计数不减、description 空。
- question prev/next + `QuestionOutcome::Superseded` 不 drain 已落地（`superseded_question_does_not_finalize`）。
- 偏差：无 Langfuse；无独立 LatestAttempt 表（对照 `document.attempt` + payload `attempt`）。question 向量挂独立 text 子块（`chunk_embeddings.chunk_id` PK 不能一源多向量），`parent_chunk_id` 指源块。
