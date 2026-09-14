# DocParser 转换门面：cancel 贯穿 + 模块边界

## Context

`crates/docparser` 是知识库 ingest 和招标冻结共用的转换门面，不是解析器本体。审查缺口是 **停机 token 没贯穿**、**入口注释撒谎**、以及 **`lib.rs` 把 DTO / 路由 / simple 转换堆在一起**，后续改引擎或合同容易再踩。

### 知识库要不要和招标「一样」？

**不要同一套产出合同。** 招标原件对 Agent 必须是「全文 + 可定位结构」（`structured_source_units` → `read_form` / 冻结证据）。知识库问答的真源是切块后的 Markdown/图，没有冻结网格、没有 `read_form`。

| | 招标 | 知识库 |
| --- | --- | --- |
| 要什么 | 全文 + 结构（units 为空即失败） | 可切块正文 + 图（忽略 units） |
| 默认引擎 | **写死 builtin**（DocReader） | Office **anydoc**（无 DocReader 也能 ingest）；PDF builtin |
| 结构 | 冻结进 `structured_forms` | 表最多是 GFM，不进招标那种 locator |

两边共用 `docparser` 门面是对的；把知识库 Office 也改成 builtin，只会让 ingest **依赖 DocReader**，仍不会自动得到招标那种结构索引。若以后要「知识库也能按格引用」，另开 ingest/chunk 计划，不塞进本次。

本计划**不改**引擎表、不让 anydoc 产 units、不把 MinerU/Paddle 设为默认、不拆 `grpc.rs` 金测试、不改 DocReader Python / Agent / 切块。

要修：

1. **anydoc → builtin 回退丢掉 cancel。** `fallback_builtin` 使用 `CancellationToken::new()`。扫描 PDF / Office 失败回退时，知识库传入的停机 token 进不了 `grpc::read`，SIGTERM 仍可能等到 30min。
2. **招标外层 `select!` 不够。** `tender_process` 对 `convert_tender_source` 做 `cancel.cancelled()`，内部却是 `convert()` → 空 token。drop future 不会停 DocReader 流。
3. **HTTP 引擎不看 cancel**（默认不用，但版本规则一旦配 mineru/paddle，停机同样漏）。
4. **`convert_tender_source` 注释写「引擎来自产品默认表 office→anydoc」**，代码一律 builtin。计划文档已经写对。
5. **`lib.rs` ~1131 行**：DTO、引擎路由、csv/json simple、anydoc 回退、测试夹具混在一个文件。打基础 = 按职责切开，短名 `pub use` 保持不变。

## Approach

**先切开门面文件，再把所有网络转换接到同一个 `cancel`。引擎表不动。**

目标目录：

```text
crates/docparser/src/
  lib.rs          薄门面：mod + 显式 pub use（对外短名不变）
  types.rs        ReadResult、Structured*、ImageRef、ConvertError、ConvertInput、NOT_CONFIGURED
  convert.rs      resolve_engine、convert*、convert_anydoc、fallback_builtin
  simple.rs       convert_simple、json/csv → markdown
  grpc.rs         不动（客户端 + units 校验测试）
  anydoc.rs / asr.rs / engines.rs / images.rs / http_engine.rs / table_grid.rs
```

`table_grid.rs` 继续做 proto ↔ `TableGrid` 映射，不把 DTO 再搬一次。

Cancel 贯穿：

```text
convert_with_cancel(input, cancel)
  simple      → 本地
  anydoc      → 本地；失败 fallback_builtin(..., cancel)
  docreader   → grpc::read(..., cancel)      // 已有
  http-engine → convert_http(..., cancel)    // select! cancel vs reqwest

convert_tender_source(file_name, bytes, cancel)
  → convert_with_cancel(engine="builtin", cancel)

tender_process：把已有 cancel 传入，不再只包一层 select!
```

`convert` / `convert_with` / `convert_to_markdown` 仍是无 token 包装（内部 `CancellationToken::new()`），给测试和非停机调用。生产：知识库继续 `convert_with_cancel`；招标改带 token 的 `convert_tender_source`。

对外 crate 路径不改：`docparser::ReadResult`、`convert_with_cancel`、`convert_tender_source` 等仍从 crate 根导出。

生产调用：

| 调用方 | 现状 | 本计划后 |
| --- | --- | --- |
| `knowledge::ingest` | 已 `convert_with_cancel` | 回退 builtin 也用同一 token |
| `bidding::tender_process`（`cancel` 已有） | `select!` + `convert_tender_source` 内部空 token | `convert_tender_source(..., cancel)` 进 `grpc::read` |
| `convert_to_markdown` / `convert` / `convert_with` | 无 token 包装 | 保持；仅测试/非停机 |

`convert_to_markdown` 在 crate 内几乎无生产调用（知识库走 `parser_engine_for` + `convert_with_cancel`）。保留包装，不删。

## Files to modify

- `crates/docparser/src/types.rs`（新）：从 `lib.rs` 挪 DTO
- `crates/docparser/src/simple.rs`（新）：simple/csv/json
- `crates/docparser/src/convert.rs`（新）：路由 + anydoc 回退（带 cancel）
- `crates/docparser/src/lib.rs`：只留 mod / pub use / proto include
- `crates/docparser/src/http_engine.rs`：`convert_http(..., cancel)`
- `crates/bidding/src/tender_process.rs`：传入 cancel
- `crates/docparser` 测试：回退路径 cancel；simple 测试随 `simple.rs`
- `crates/bidding/src/tender_process.rs`：`DocReaderGrpcTenderSourceConverter` 把已有 `cancel` 传入
- `docs/knowledge-base/crate.md`：一句「网络转换必须带调用方 cancel」

## Reuse

- `grpc::read(..., cancel)`、`cancelled_read_returns_before_connect`
- 知识库已走 `convert_with_cancel`（`crates/knowledge/src/ingest.rs`）
- 招标已有 `cancel`（`tender_process.rs`）
- 引擎合同：[`docreader-structured-parse.md`](docreader-structured-parse.md)
- `pub use` 短名模式同 `crates/knowledge/src/lib.rs`

## Steps

- [x] 1. 抽出 `types.rs`：结构体/枚举/`ConvertError`/`NOT_CONFIGURED`；`lib.rs` 显式 `pub use`
- [x] 2. 抽出 `simple.rs`：`convert_simple` + json/csv；现有 simple 单测跟着走
- [x] 3. 抽出 `convert.rs`：`resolve_engine`、`convert*`、anydoc 回退；`fallback_builtin` / `convert_anydoc` 接 `&CancellationToken`
- [x] 4. `convert_http` 接 token；未配置 endpoint 行为不变
- [x] 5. `convert_tender_source(file_name, bytes, cancel)` 走 `convert_with_cancel`；注释改为写死 builtin
- [x] 6. 招标 `tender_process` 把 cancel 传入；去掉「只 select、内部空 token」
- [x] 7. 测试：已 cancel 的 anydoc→builtin 回退在 FRAME_IDLE 前返回；未 cancel 的 office 回退仍成功
- [x] 8. `cargo test -p docparser`；`cargo check -p knowledge -p bidding -p worker -p api`
- [x] 9. crate.md 补 cancel 口径；不改 `docreader-structured-parse.md` 引擎表

## Verification

- 已 cancel 的 anydoc→builtin 回退不会把 `grpc::read` 跑满 30min / 120s idle
- 招标转换 cancel 能进 `grpc::read`
- `convert_tender_source` 仍只接受现有扩展名、仍强制 builtin
- 知识库默认引擎表不变（office anydoc / pdf builtin）
- MinerU/Paddle 仍非默认
- `docparser::ReadResult` / `convert_with_cancel` 等对外短名仍能从 crate 根引用
- `cargo test -p docparser`
- `rg 'CancellationToken::new\\(\\)' crates/docparser/src/convert.rs crates/docparser/src/http_engine.rs`：生产回退/HTTP 路径不得用空 token（无 token 包装只留在 `convert` / `convert_with`）
