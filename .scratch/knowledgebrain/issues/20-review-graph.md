# 20 — Review：图谱对照 brain

**What to build:** 对照 brain 图谱命名空间与抽取入队，确认没有跨版本写边、没有拷回 Neo4j 门闩。

**Blocked by:** 19 — 图谱抽取

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] namespace 为 product_version + document
- [x] 两 chunk 同名实体一行两个 chunk_id（单测）
- [x] `attemptSuperseded` 不 FinalizeSubtask
- [x] 偏差已记明（可选 Neo4j 双写；无 Langfuse；attributes 不落盘）

## Comments

- 存储形状与 extract.go Handle 顺序已核对。门禁：fmt / clippy -D warnings / cargo test --workspace / ruff / pytest 98 pass 13 skip / compose / proto script / spec 副本。
- PG persist 已改增量 UNION（`persist_graph_unions_chunk_ids`）；superseded 不再 drain；override 走 `effective_version`。
