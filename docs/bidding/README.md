# 招投标文档入口

当前招标分析与编制统一使用 [Python 工作流](../design/python-tender-phase1/business-workflow.md)，字段以 [字段字典](../design/python-tender-phase1/field-dictionary.md) 为准，实施与实际验证见 [交接文档](../design/python-tender-phase1/handoff.md)。

旧 Rust 招标工作流、专用 API、worker、表和存储过程已删除，不保留兼容运行入口。Rust 的 API 桥接、知识库、对象存储和通用后台服务继续使用。全新初始化及创建人权限见 [数据库设计](../design/database-initialization.md)。

本目录只保留 Word 编辑、保存和出件。分析次数、字段和运行状态分别看业务工作流、字段字典和交接，不在这里再写一套。发布工具边界见 [发布入口](../../deploy/release-entry.md)。文档怎么维护见 [文档导航](../README.md)。

## 来源格式怎么进 Word

投标文件只产出 Word。PDF、DOCX、Excel 都冻结成同一批来源对象，格式定义只在来源存一份。Word 还原的是来源里已经记下的格式，不按原 PDF 逐页排版。

- **给模型的：** 表格的格子明文 `grid.rows`（空字符串是原生空格，`null` 是没有独立单元格）、非 1×1 的 `merged_cells`、正文的阅读文本，以及 `part_id`、`range_id` 这些身份。模型只回引用和有依据的应答，不回表格、列宽或换行。
- **只留在来源的：** 列宽、字体、页边距、页坐标，以及 `paragraph_breaks` 的字符偏移。
- **导出（`authoring_export.py`）按引用回来源取格式：**

| 来源里的格式 | Word |
| --- | --- |
| 格子 | 画表。只选中部分行时带上表头，空应答留空 |
| 落在 `paragraph_breaks` 上的切点 | 新段落 |
| 没有对应切点的换行 | 并回原句，字中间断开的也并 |
| 上一行以 `。！？；：` 结尾，或下一行以 `13.`、`（1）`、`一、` 开头 | 新段落 |
| 冻结解析标记为重复页边文字的行 | 仅省略处于当前物理页首或页尾的原文单元；同样文字出现在表前后或其他正文位置时保留 |
| 图片 | 嵌入原件 |

同一条要求既在表里又在正文里时，两处都保留。`text_boundaries` 只说明片段是否顶到原范围两端，不能单独决定合并，切点要回来源的 `paragraph_breaks` 核对。
