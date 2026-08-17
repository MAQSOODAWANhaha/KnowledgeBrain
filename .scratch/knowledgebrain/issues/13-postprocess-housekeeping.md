# 13 — post_process 收口与 housekeeping

**What to build:** 无需摘要/问题/Wiki/图谱时，文档从 processing 走到 completed；卡住的任务会被定时扫成 failed。

**Blocked by:** 12 — Review：分块与向量对照 brain

**Status:** done

- [x] `expectedSubtasks==0` 时直接 completed，且只在 `processing` 上 `SetFinalizing`
- [x] `FinalizeSubtask` 减到 0 才 completed
- [x] oxana cron `0 */5 * * * *` 扫 `pending|processing|finalizing`，阈值 `DocumentProcessTimeout+10m`，有 span 心跳则跳过
- [x] `runtime::housekeep`（内存）+ `storage::housekeep_documents`（PG）

## Comments

- reality: worker 在 `low` 上注册 `HousekeepJob`。无 PG 时 cron 空转。`KNOWLEDGEBRAIN_HOUSEKEEPING_ENABLED=false` 可关。未接 queue inspector（brain 的「队列里还有任务则不杀」）。
- PG `SetFinalizing` 在入队前原子 `WHERE parse_status='processing'`。入队失败释放 owned slot（summary/question/extract）；Redis 不可达仍 inline。扇出 summary 时 `summary_status=pending`。`knowledge:post_process` 最后一次重试 fail 父文档。Wiki 仍不进 shortfall（与 brain 已知缺口一致）；入队 `Err` 会 `FinalizeSubtask`，无 Redis 则 inline ingest。
- review: 见 14。
- 5.12 `datatable:summary`：csv 抽样、xlsx/xls 只读 DocReader Markdown（不在 Rust 解析表格）。brain 表/列 prompt + `table_metadata_instructions`。重试替换旧 `table_summary`/`table_column`。PG worker 删类型后 append，不 `replace_document_chunks`。ingest 与 reparse 入队。不计入 `pending_subtasks_count`。不 fail 父 `parse_status`。无 DuckDB。
