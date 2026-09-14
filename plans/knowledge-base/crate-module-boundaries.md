# 知识库 crate 模块边界（已完成）

| 项 | 值 |
| --- | --- |
| 状态 | **已落地** |
| 规范 | [`docs/knowledge-base/crate.md`](../../docs/knowledge-base/crate.md) |

把 `persist.rs` 按目录拆开，并删掉进程级 `Store` HashMap。生产真源一直是 Postgres + Oxana；`Store` 只是 job 里把 PG 灌进 Map 的影子模型。招标端口语义未改。

当前目录、job 形态与禁止项以 **crate 规范**为准，不要按本文旧推荐树继续拆 `domain/` 或把 `pipeline.rs` 再搬一次。

## 已做

- [x] S0 冻结对外短名；新 SQL 不再进 `persist.rs`
- [x] S1 `persist.rs` → `catalog/` `identity/` `search/hybrid.rs` `wiki/sql.rs` `graph/sql.rs`；`persist.rs` 已删
- [x] S2 去掉 glob 与 `platform` 转发；`lib.rs` 显式 `pub use` 保持 `knowledge::load_document`
- [x] S3 pipeline / wiki job 改为 `DocJob` / `WikiJob::from_pool` + SQL
- [x] S4 删除 `Store`、`hydrate_*`、`write_back`
- [x] S5 规范写入 [`docs/knowledge-base/crate.md`](../../docs/knowledge-base/crate.md) 与 [`docs/knowledge-base/domain.md`](../../docs/knowledge-base/domain.md)

## 刻意未做（另开任务）

- DTO 仍在 `store.rs` / `status.rs`，没有再拆 `domain/*.rs`
- `pipeline.rs` 与 `ingest.rs` 仍在 crate 根，没有并成 `ingest/` 目录
- 用户表仍在 `identity/`，不迁 `platform`
- 不合并产品问答与投标 V3 检索

## 验证（落地时）

`cargo check -p knowledge -p api -p worker`；`cargo test -p knowledge --lib` 224 passed（跳过需 PG 的 `matching_pg` 一条）。
