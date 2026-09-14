# 知识库文档详情：分类展示（已完成）

| 项 | 值 |
| --- | --- |
| 状态 | **已落地** |
| 代码 | `web/src/assets/DocumentDetail.tsx`、`FilePreview` |

点开知识库文件后不再把全部 `chunks` 摊成一条列表。一层页签按对用户的作用分：

| 页签 | 数据 | 作用 |
|---|---|---|
| **原件** | `object_key` + `FilePreview` | 上传文件本身 |
| **解析** | `markdown` 全文 | convert 结果，切块前 |
| **正文** | `text` / `parent` / `child`，按 `start_at` | 检索切块，不含问句 |
| **图像** | `image_ocr` + `image_caption` 按 `context_header` 成对 | 一图一组 |
| **问句** | `question` | 后处理；空则隐藏 |
| **摘要** | `summary` | 后处理；空则隐藏 |
| **Wiki** | `wiki_page` | 蒸馏词条；空则隐藏 |

默认 **原件**。API 仍是 `GET /api/v1/documents/{id}/content` 回全部 chunks，前端过滤。不改知识库检索语义。

部署时需把当前 SPA 拷进 api 镜像（`KNOWLEDGEBRAIN_WEB_ROOT`）；这是发布步骤，不是未完成的产品缺口。
