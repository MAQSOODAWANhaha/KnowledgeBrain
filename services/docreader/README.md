# DocReader

KnowledgeBrain 解析进程。协议 **仅 gRPC**（`services/docreader/proto/docreader.proto`）。

- `ReadStream`：默认。第一帧 `meta`（markdown / metadata / error / image_count），之后每帧一张 `ImageRef`。
- `Read`：一元回退。
- `ListEngines`

本进程 **不分块、不 OCR、不写对象存储、不写业务库**。扫描页只出 JPEG；OCR/caption 在 Rust worker。

## 引擎

| engine | 类型 |
|---|---|
| `builtin` | docx→Docx2；OLE `D0CF11E0` 当 doc；pdf/md/xlsx/xls/epub/mhtml/图 |
| `markitdown` | MarkItDown |
| `opendataloader` | 仅 pdf |

引擎不支持该类型 → 回退 builtin。URL 固定 `WebParser`。空 content → `error` 非空。

## 环境

| 变量 | 含义 |
|---|---|
| `DOCREADER_PORT` | 默认 `50051` |
| `GRPC_AUTH_TOKEN` | 可选；客户端带 `Authorization: Bearer` |
| `GRPC_TLS_ENABLED` | 可选 TLS |
| `MAX_FILE_SIZE_MB` | 默认 50 |

Worker 通过 `DOCREADER_ADDR` 拨号。MinerU / Paddle 在 Rust `docparser`，不在本进程。
