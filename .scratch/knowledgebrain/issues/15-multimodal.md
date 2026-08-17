# 15 — 多模态图任务

**What to build:** 带图文档先跑完每张图的 OCR/Caption，再进入 post_process；一张图挂了不会整篇 failed。

**Blocked by:** 12 — Review：分块与向量对照 brain

**Status:** done

- [x] 仅 `EnableMultimodel && 有图` 入队 `image:multimodal`（oxana convert 与内存 drain）
- [x] Redis `multimodal:pending:{document_id}` SET N EX 24h + DECR；Redis 不可达时回落内存 HashMap
- [x] 子 chunk 类型为 `image_ocr` / `image_caption`，挂父文本块
- [x] 单图死信不 fail 父文档，最后一次仍 DECR
- [x] OCR/Caption 使用 brain prompt；`image_source_type=scanned_pdf` 走扫描 PDF 专用 OCR prompt；`sanitize_ocr_text`

## Comments

- reality: 无外部 VLM 时用 stub 文本，但 prompt 选择与 sanitizer 与 brain 一致。PDF 文档入队时带 `scanned_pdf`。oxana `multimodal` 队列已注册。
- PG convert 入队前 `SET multimodal:pending:{id}=N`。VLM URL 配置时失败可重试、中间不 DECR；最后一次 DECR 入队 post_process，单图死信不 fail 父文档。cancelled/deleting 仍计数。父块 = 正文里含该图 key 的 text chunk。
