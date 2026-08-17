# 30 — Review：问答门面

**What to build:** 确认没有做成 Agent/多轮，也没有在 HTTP 里同步解析。

**Blocked by:** 29 — 问答门面

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 生成只用 current 的 `summary_model_id`（请求带了 `version_id` 也仍用 current）
- [x] 跨版本事实未融合（省略 `version_id` 时锁定 current，禁止跨版本拼接）
- [x] 无 hits 不编造；引用只来自本次 hits
- [x] 偏差已记明

## Comments

- `search::answer` 对照 brain `default_kb` system prompt + assembly hits。`stub-chat` / 无 `KNOWLEDGEBRAIN_CHAT_BASE_URL` 走抽取式 stub；有 URL 则 `/v1/chat/completions`。
- 可选 `context[]` 仅作会话上文，不当成知识。
- 偏差：无多轮 Agent / 无会话表 / 无 Langfuse；HTTP 检索仍是内存 Store。
