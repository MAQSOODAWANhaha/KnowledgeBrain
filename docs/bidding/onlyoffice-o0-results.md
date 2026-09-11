# ONLYOFFICE O0 本地验证记录

2026-09-07：真实 DOCX 已完成打开、键盘编辑、保存、关闭重开及同稿 PDF 导出。字体基线、生产嵌入/并发许可及最终模板外观仍未验收，**完整 O0 未完成，O1 产品接入尚未实施**。

## 环境与样稿

- 本次试验：官方 Community 9.4.0-129、Chromium 151.0.7922.34；不是生产版本选择或配置默认值。
- 镜像：`onlyoffice/documentserver@sha256:e3da62a847b9a5d51a11f73cfea1d9c13c3be3809614490d4edddcf01dcf919b`。
- 用户提供的 `testdata/bid/60adc38f81464a0a93898eb435b506d6.docx`，只编辑独立副本。原件 SHA-256：`4a7c68d8c20688a12f7e978e97abbbd5b2cec116728ad3f3f639558c8091b47d`。
- 同目录 `BiddingFile.pdf` 是另一份 106 页招标文件，不作为 DOCX 逐页对照基线。
- 真实稿缺少的长表与图片场景另用明确标注的合成补充稿，不计作真实业务样稿。

## 结果

| 验证项 | 证据结论 |
| --- | --- |
| 真实保存重开 | 正文、原表格单元格各一处编辑；实际持久化 DOCX 中两标记各一次，新会话可找到 |
| 内容与结构 | 除预期编辑，全文与所有表格内容保持；4 张非等宽表格的列宽/横纵合并、3 个节保持 |
| 页眉页脚 | 显示文本、PAGE 域及 tab 数量保持；XML 序列化空白发生变化 |
| 同稿 PDF | 编辑前 37 页、编辑后 38 页；导出前后已存 DOCX 字节一致；表格标记在第 35 页跨三行完整可见 |
| 补充长表 | 80 行各一次且顺序一致，跨越的 6 页都有重复表头 |
| 补充图片 | 第 7 页实际可见；DOCX 中图片摘要保持 |
| 原件与清理 | 原始 DOCX/PDF 摘要保持；本轮容器、网络与浏览器已回收，缓存及证据保留 |

真实稿使用原字体声明及引擎回退。容器可用 Times New Roman、Arial、Courier New，未检出宋体、黑体、仿宋_GB2312、Calibri、DFKai-SB、PMingLiU、Cambria 的精确 family。补充稿使用记录了摘要和许可的文泉驿正黑。这不表示原模板字体齐备或已接受替代字体。

保存了官方 Community AGPLv3 LICENSE，尚无拟采用产品的生产嵌入及并发授权结论。视觉检查仅为选定页抽查；全文和全部表格另有结构比较，不宣称逐页人工保真通过。

## 本机证据与后续

本轮证据保留在 `/tmp/knowledgebrain-o0-preflight.oWWSSD1k/`，包含 `REPORT.md`、`runtime-results.json`、`FAILURES-AND-LIMITS.md`、实际保存 DOCX/PDF、页图、结构比较及官方依据。它是本机临时交接位置，在其他 checkout 不保证存在；真实业务文件没有复制进文档目录。

交接包 `o0-handoff.tar.gz` 为 13,969,442 字节，SHA-256：`4cdc38998b090e1bd1f7434c9a6c95e90f5451a1b778c30372a6d9f287097200`。需长期保留时应归档此包及清单，不将 `/tmp` 当永久资产库。

下一步落实字体及产品许可依据、确认所选模板外观，并完成 F2 新轮输入边界。KnowledgeBrain 的会话授权、JWT 回调、持久化版本/CAS 和迟到保存保护属于 O1；官方示例的成功不能替代这些验收。ONLYOFFICE 编辑与出件顺序见 [接入计划](../../plans/bidding/onlyoffice-integration.md)。
