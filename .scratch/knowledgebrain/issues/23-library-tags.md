# 23 — library、TAG 与 include_library

**What to build:** 公司资料与型号用同一套管线入库；打 TAG 能过滤；搜产品时打开 include_library 能带上资质类文档。

**Blocked by:** 04 — Review：目录与登录；12 — Review：分块与向量对照 brain

**Status:** done

- [x] library 产品上传路径与型号相同
- [x] TAG 属 Workspace，改 TAG 不重解析
- [x] assembly + include_library 命中 library 的 current，hit 带 product_kind
- [x] 不能用多个 TAG 让一篇文档属于两个 Version

## Comments

- reality: HTTP `create_tag` / ingest / `put_tags` 写 PG。`DELETE /workspaces/{id}/tags/{tag_id}` 只删 `document_tags` + tag 行，不删文档。
