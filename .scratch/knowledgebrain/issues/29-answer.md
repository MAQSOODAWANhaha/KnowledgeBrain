# 29 — 问答门面

**What to build:** 指定产品后问一句，得到带引用的答案；无会话。模型用该产品 current 版本。

**Blocked by:** 12 — Review：分块与向量对照 brain

**Status:** done

- [x] `/answer` 只调 assembly search（先 hydrate；PG 回落用 current `summary_model_id`）
- [x] 无 current → 400；无 hits → 空答案不编造
- [x] 引用全部来自本次 hits
- [x] 不写会话表、不调 matching

## Comments

- HTTP 只鉴权 + 调 `search::answer`。检索走 assembly；生成走 current 的 `summary_model_id`（见 30）。
