# 知识库文档详情：分类展示

## Context

点开知识库文件后，`GET /api/v1/documents/{id}/content` 把所有 `chunks` 摊成一条列表。白皮书实测约 288 条：正文 47、问句 141、配图 30、OCR 29、Wiki 40、摘要 1。问句/配图还曾是英文。用户无法核对「解析后的正文是否完整」。

目标：详情页能 **预览原件**、看 **完整解析 Markdown**、核对 **仅正文分片**。派生数据（问句、摘要、Wiki、图）分类另放，不混进正文。

## Approach

一层页签，按 **对用户的作用** 分，不按数据库 `chunk_type` 平铺。

| 页签 | 数据 | 作用 |
|---|---|---|
| **原件** | `object_key` + 现有 `FilePreview` | 上传文件本身（pdf/docx/pptx/图/表） |
| **解析** | `markdown` 全文 | **完整连续正文**（convert 结果，切块前） |
| **正文** | 仅 `text` / `parent` / `child`，按 `start_at` | **检索用切块**，条数=正文块数，不含问句 |
| **图像** | `image_ocr` + `image_caption` 按 `context_header` 成对 | 一图一组：OCR / 配图说明 |
| **问句** | `question` | 后处理检索问句，标明「不是原文」 |
| **摘要** | `summary` | 后处理，空则隐藏 |
| **Wiki** | `wiki_page` | 蒸馏词条，空则隐藏 |

- 默认进 **原件**。要通读全文点「解析」，要核对切块点「正文」。问句 / Wiki 保留为独立页签（空则隐藏）。
- 问句 / 摘要 / Wiki **无数据则不出现页签**。原件、解析、正文始终在。
- 「正文」页脚/说明：完整连续文本看「解析」；这里只是切块后的检索单元。
- 不在正文块下再挂 `generated_questions`。
- 语言：正文/解析跟 convert；问句/caption 跟 enrichment 语种检测（已改，旧文档需重跑后处理）。

布局沿用知识资产 `Shell` + `card` / `chip` / `chunk-row`，不新开设计系统。

```
[返回列表]  文件名
原件   解析   正文 47   图像 30   问句 141   摘要 1   Wiki 40
────────────────────────────────────────
主区：当前页签内容（可滚）
```

## Files to modify

- `web/src/assets/DocumentDetail.tsx` — 分类页签与各列表（工作区已有草稿，按上表收口）
- `web/src/app.css` — 图像组 OCR/配图说明层级
- 不改 API：`document_content` 已回全部 chunks，前端过滤即可

## Reuse

- `FilePreview` — `web/src/assets/FilePreview.tsx`
- `GfmPreview` — `web/src/bid/gfm.tsx`
- `api.documentContent` — `web/src/api.ts`
- 路由 `#/products/.../{versionId}/{docId}` — `web/src/hash.ts`

## Steps

- [ ] 正文过滤：`text|parent|child`，按 `start_at` 排序，展示 `i/N` 与字区间
- [ ] 图像按 `context_header` 成对：OCR 一块、caption 一块
- [ ] 问句 / 摘要 / Wiki 独立页签，空则隐藏
- [ ] 去掉「一个列表混全部 chunk_type」
- [ ] 默认页签 `原件`；说明文案区分解析 vs 正文 vs 派生
- [ ] 重建 api 镜像里的 SPA（`KNOWLEDGEBRAIN_WEB_ROOT`）

## Verification

- 白皮书 `d1da77d7-…`：「正文」只有约 47 条中文，无 How does…
- 「解析」可通读全文 Markdown
- 「图像」30 组，不出现在「正文」
- 「问句」单独一栏；无问句的 txt 不显示该页签
- 原件 docx/pptx/pdf 仍可预览
