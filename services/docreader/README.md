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

## Source contract V2

The public `Parser` / `BaseParser.parse` path publishes `metadata.source_contract`
(JSON) alongside typed source units. The metadata is transported unchanged by
both gRPC response modes. `parse_into_text` is the internal extraction stage.

- `unit_id` is the existing deterministic unit key scoped to `document_revision`
  (the original bytes' SHA-256); `ordinal` is extraction/publication order.
- Section ownership, heading hierarchy and physical coordinates are independent.
  PDF prose retains its `DocumentLocator`; its identity record also carries a
  physical page locator. DOCX includes structural XML paths; spreadsheets retain
  sheet/row/column locations. Rendering a heading never changes the source text.
- `rendered_spans` are exact half-open UTF-8 byte ranges recorded while rendering,
  including native grids whose `text` is empty. Repeated strings never use a
  global substring search. XLSX grids map to the rows already rendered.
- `text_sha256` binds exact unit text. `grid_sha256` uses the explicit
  `docreader-grid-v2\0` framing implemented in Python `grid_digest` and Rust
  `table_grid_digest`: big-endian u32 dimensions/counts/anchor coordinates/spans,
  u64 UTF-8 lengths, bytes, and IEEE-754 big-endian widths. Merge-covered slots
  are absent; header annotations reference real anchors only.
- `page_manifest` inventories every PDF page, including blank and scanned pages,
  separately from sections. Output inventory additionally requires every page's
  image payload and immutable image hash. It never fabricates empty Page sections.
- Completeness describes extraction. Extracted image bytes do not imply completed
  OCR. Unsupported drawings/attachments stay explicit partial carriers; package
  thumbnails and unused image relationships are excluded from OCR obligations.
- The Rust `parse_source_contract` validates a present receipt and returns `None`
  for an absent receipt. Strict frozen-input callers must reject absence. General
  imports may mark missing mappings unresolved, never guess exact locations.
- `rewrite_images_with_contract` fails on unresolved image payloads, remaps byte
  ranges after host-owned replacements, and returns the updated rendering digest.
  Blob publication must succeed before the caller publishes that receipt.

`parser_version` fingerprints the serializer implementations. A source change,
parser change or later OCR result requires a newly frozen input rather than an
in-place mutation of evidence. The recorded cross-language fixture is generated
by `scripts/generate_source_contract_fixtures.py` and validated by Rust tests.

## HTTP engine configuration

The Rust catalog and converter share `resolve_effective_engine_config`:

| Engine | Endpoint override | Optional bearer override |
| --- | --- | --- |
| `mineru` | `mineru_endpoint` | `mineru_api_key` |
| `paddleocr_vl` | `paddleocr_vl_endpoint` | `paddleocr_vl_token` |

Overrides take precedence over the corresponding process configuration, including
an explicitly empty override which disables fallback. Endpoints require HTTP(S)
without embedded credentials, queries or fragments. Authentication is never part
of a source receipt. Cloud engine names are explicitly unsupported until their
provider-specific protocols are implemented; cloud keys cannot enable or reroute
the self-hosted protocol.

## PDF chapter and symbol fidelity

TOC rows ending in dotted/ellipsis leaders plus a page number remain ordinary
source text at both structural promotion and final sectionization. Leader length
is not a title-length limit. Explicit volume and chapter labels determine stable
ownership even when fonts assign inconsistent heading sizes across pages.

Private-use characters are never decoded by Unicode value alone. For the exact
Wingdings 2 family, embedded-font cmap names `boxcheck` (U+F052) and `box1`
(U+F0A3) permit checked/empty-box decoding. Font parsing is local, bounded to
16 MiB per embedded font, and cached only for the lifetime of one page. The typed
`glyph_normalizations` source receipt preserves raw/decoded symbols, font byte
hash, font/glyph names, character index and page geometry through frozen input.
Unknown, malformed, non-embedded or page-ambiguous private glyphs retain their
raw text and explicitly require visual review rather than claiming completeness.
