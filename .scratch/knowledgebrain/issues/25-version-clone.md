# 25 — 版本克隆

**What to build:** 从上一版克隆出新版本：未改文件复用对象和索引，改过的重新解析；源版本内容不变。

**Blocked by:** 12 — Review：分块与向量对照 brain；14 — Review：post_process 与 housekeeping 对照 brain；34 — Review：0001

**Status:** done

规格 §6（2026-08-15）：crate 仍是 `crates/clone`；生产路径是 Postgres + oxana `low`。

- [x] HTTP `clone_from`：只 INSERT `product_versions`（`cloning` + 深拷配置列）并入队 oxana `low` / `version:clone`；请求线程不拷文档
- [x] `clone::run_clone` 走 sqlx；keep 新 `document_id`、`content_objects.refcount++`、拷 `document_tags`；源行不动
- [x] add/replace 入队 `document:process`；delete 只影响目标
- [x] 0004 已落地且源/目标 `embedding_model_id` 相同：keep 拷 chunk / embedding（含 tsv、`generated_questions`），`parse_status=processing`，follow-up 为 `knowledge:post_process` 且 `clone_keep=true`
- [x] 0004 未落地或 embedding 不同：keep 不拷索引，入队 `document:process`
- [x] 不拷 wiki/graph；不自动改 current（除非 `make_current`）
- [x] 禁止再把内存 HashMap 克隆当作生产路径（旧 `run_clone(&mut Store)` 已删）
- [x] worker `low` 消费 `version:clone`，按 follow-up 入队 process / post_process

## Comments

- reality: `clone::run_clone` 测过 compose Postgres（keep 拷行 + mismatch 仍 reparse）。`runtime::enqueue_version_clone` 测过 Redis `low`。worker `process_version_clone` 调 `run_clone` 后按 `clone_keep` 入队。
- oxana `postprocess` 有 `PostProcessWorker`：hydrate 后跑 `post_process`，写回 parse_status / pending，clone_keep 入 wiki pending 并 inline graph extract。
- HTTP 目录仍双写内存；GET / list_tags / wiki 在缺实体时 hydrate；目录已在仍会 merge tags/chunks。
