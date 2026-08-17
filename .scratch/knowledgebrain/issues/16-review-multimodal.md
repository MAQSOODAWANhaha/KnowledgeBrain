# 16 — Review：多模态对照 brain

**What to build:** 对照 brain 多模态入队与 finalize-on-last-image，确认父文档失败语义未被改掉。

**Blocked by:** 15 — 多模态图任务

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 死信：image 不 fail 父文档
- [x] cancelled/deleting 仍 DECR 计数
- [x] 扫描 PDF 专用 prompt 分支（`ocr_prompt("scanned_pdf")` ≠ 默认）
- [x] 偏差已记明（有 `KNOWLEDGEBRAIN_VLM_*` 走 HTTP，否则 stub 仍走 scanned_pdf prompt）

## Comments

- review 2026-08-15f: 死信不 fail 父文档；abort 仍 DECR；Redis pending + scanned_pdf prompt 已核对。
- 2026-08-17: PG 入队补上 SET pending；VLM 配置时失败可重试且中间不 DECR（对齐 brain defer）。未做 `image_info` 列 / VLM custom_instructions。
