# 招标冻结网格：DOCX / XLSX / XLSM / PDF builtin

更新日期：2026-09-09。本文是**招标 `convert_tender_source`（固定 builtin）** 的解析/冻结增强。S0–S2 与 XLSM/用 used range **已按本文落地**；S0b 图 Y 轴/MIME 走现有解码路径，未另开政策。不替代 [WeKnora 对照基线](../../docs/research/weknora-parse-extract-baseline.md)，不切换知识库 Office 默认引擎，不改 Agent 循环。

v3 **不考虑旧冻结迁移**。S0 改生产者、发布缝、**以及 `table_block_from_grid` 本体**（不是「合成边后仍调用旧 dense 校验」）。

## 1. 消费者与引擎（不要写成一份结果）

| 调用 | 默认引擎 | 权威产出 | 本期是否改变 |
| --- | --- | --- | --- |
| 招标 `convert_tender_source` | **写死 builtin** | units → `structured_forms` / Agent | **是**：Word/Excel 可 `read_form` |
| 知识库 `parser_engine_for` + `convert_with_cancel` | Office → **anydoc**（成功不调 DocReader）；PDF → builtin | markdown + 图；**忽略 units** | **否** |

知识库 PDF 已有 leftover+GFM。Office 默认仍走 anydoc。不把「Word 知识库解析变好」列为完成条件。

招投标编辑/出件仍按 [ONLYOFFICE O0–O4](../bidding/onlyoffice-integration.md)；提取仍按 [Rig](../bidding/agent-runtime-rig.md)。`search_sources` 格命中是 **Agent 切片**（§6），不是本方案实现项。

### 1.1 本期做 / 不做

**做（招标 builtin）：**

- PDF 文字层：保持 leftover `SECTION` + 表网格；v3 格子只在 `unit.grid` / form。
- DOCX：OOXML 稀疏锚点（含空格、合并 span）；表内图挂网格 column，不依赖 `TABLE_ROW`。
- XLSX/XLSM：每个 ListObject 出一份 form；无 Table 的非空 sheet 按 used range 出 form，并保留行文本。
- 转换合同升 v3；form 从 `p_units.grid` 写入。
- `search_sources` 格命中；空 units 禁止 markdown 回退；空 OCR 分图 unresolved；招标白名单含 `.doc`/`.xls`（OLE，不执行宏）。

**不做：** 替换 DocReader；默认 MinerU/Docling；改 PDF 表检测；markdown 当引文；重写 `DocxParser` 版面通道；Office `SourceView`；重建旧冻结；Excel 字符列宽换算成 mm。

## 2. v3 字段与接线

Agent `read_form` / 编制以 **`structured_forms.definition`** 为准。locator 不再带格子。

### 2.1 Locator（身份）

| 来源 | locator | 字段 |
| --- | --- | --- |
| PDF 表 | `page_table` | **仅** `page_ordinal`, `table_ordinal`, bbox（y-up） |
| DOCX 表 | `document` | `section_ordinal`, `table_ordinal`, `heading_path` |
| XLSX/XLSM ListObject | `spreadsheet` | `sheet_ordinal`, `sheet_name`, 1-based Table 区域起止, 可选 `table_name`。无 cells/address 数组 |
| XLSX/XLSM used range | `spreadsheet` | `sheet_ordinal`, `sheet_name`, 1-based used 区域起止, **无** `table_name`（key `sheet:{n}:used`） |

不把 Word 写成 `page_table`。不把 180mm 帽写进源 locator。

### 2.2 `TableGrid`（gRPC + form schema 3）

**Proto**

- 新 `TableGrid`：`row_count`, `column_count`, 稀疏 `cells`（现有 `PdfTableCell`），可选 `widths_mm`。
- `StructuredSourceUnit.grid = 11`。无 grid 的 `TABLE_REGION`：**仍可冻结为来源单元**，只是不写 `structured_forms`（S0 Word 曾无 grid；S1 起 Word 表带 grid）。Decode **不得**因缺 grid 失败。
- `PageTableLocator` 字段 7–12 **先保留、不 `reserved`**：身份解码仅当 proto 默认空（`uint32`=0、repeated 空）时通过；任一非空则报错。Python 模型与 `main.py` **只填 1–6**，不把格子写回 locator。
- `TableCellImageParent.cell_ordinal`：**语义改为 0-based 网格 column**，不新增字段。

**稀疏规则：** `(row,column)` 唯一；span 矩形不重叠；覆盖位不出现在 `cells`；空白锚点 `text=""` 合法；无 `widths_mm` 合法；有则长度 = `column_count` 且全 >0。**铺满：** 每个 `0..row_count × 0..column_count` 格恰好是一个锚点，或被恰好一个锚点 span 覆盖。有洞则 decode/form 插入失败（`read_form` 的 `null` 只表示覆盖，不表示缺锚点）。

**Form schema_version = 3**（SQL `CHECK (IN (1, 2, 3))`）。definition 与 `TableGrid` 同形，外加 `kind=grid` 与两个 id。不存 `merged_ranges`、`column_edges`、`geometry_source`。

**密铺下标（禁止 `cells.position` / `cells.len()`）**

`index = row * column_count + column`，`total = row_count * column_count`。

| 调用 | 行为 |
| --- | --- |
| `read_form` | offset/limit/next/`total_cells` = `total`；返回密铺窗口；**覆盖位** JSON `null`；缺锚点不得当成覆盖（铺满校验已拒绝） |
| coverage / `validate_span` | 读区间落在 `[0, total)` |
| `unread_grid` / `set_disposition` / `validate_record` 的 **coverage/cite** | 密铺 `[0, total)`，cite = `row*column_count+column` |
| `validate_record` 的 **正文 / `blank_cell_text` / 完整格锚点集合** | 按 `(row,column)` 查稀疏 `cells`，**禁止** `cells[dense_index]` |
| `grid_cell_is_anchor` | 稀疏 `cells` 中存在该 `(row,column)` 且不被其他锚点 span 覆盖；**不读** `merged_ranges` |

**列宽**

- 有 `widths_mm` 即实测。PDF 写入现有 `pdf_tables.widths_mm`（已按可打印宽度缩放），避免 `BiddingFile` 编制触 cap。
- Word 仅 `tblGrid` 正宽度；Excel 仅非默认自定义列宽。禁止均分写入 form。
- **编制必须改 `table_block_from_grid`（`compiler.rs` 与 `docx_template.rs` 共用）：** 接受 schema 3 稀疏锚点；由 `row_span`/`col_span` 推导覆盖与合并，**不要求** `merged_ranges`、**不要求** `cells.len()==rows*cols`、**不把 definition.cells 密铺回存**；有 `widths_mm` 时在函数内合成 edges 仅作局部校验；无 widths 仍 fail-closed。禁止「只合成边然后调用未改的旧 helper」。

**发布缝（已落地）**

`tender_process.rs` unit JSON 带 `"grid": {…}`，与 `source_span_v2` 并列。locator 已是身份。

SQL：`unit_kind='form_region' AND jsonb_typeof(unit_value->'grid')='object'` 时从 **`unit_value->'grid'`** 组 schema 3（`IS NOT NULL` 对 JSON `null` 为真，不可用）。禁止 `locator.cells`。

converted_source 的 parser units 可保留 `grid`（解析快照）。Agent FrozenInput locator 无 cells。

`TENDER_CONVERTER_OPERATION` = `docreader-grpc-structured-source-v3`（SQL 上传合同 payload 同串；Rust 按 SHA-256 digest fail-closed）。

### 2.3 解码与表内图

- 身份 `page_table`：**新解码器**只认 bbox + 两个 ordinal。`from_proto_page_table` 删除或改为调用它（不得再要 cells/edges/180mm）。Python `validate_grid` 满格/180mm 规则删除。
- `unit.grid` 独立校验稀疏锚点。
- pairing：`TABLE_REGION` × 身份 `page_table` / `document`(有 table_ordinal) / `spreadsheet` 区域。
- 表内图 parent：S0 曾双接受 (a) Document `TABLE_ROW` `(section,table,row)` 或 (b) 网格锚点。**S1 已落地**：解码只认 (b) `grid_cell_is_anchor`，`cell_ordinal`=grid column；禁止空 Document `TABLE_ROW`。Excel spreadsheet `TABLE_ROW` 仍是行文本，不是图 parent。
- Excel：`row = excel_row - start_row`，`column = excel_column - start_column`；span 在平移后计算。

## 3. 按格式

### 3.1 PDF

检测算法与 leftover 不变。写入 `unit.grid` 前 **稀疏化**：只保留合并锚点（带 span）与未覆盖的 1×1 格；丢掉覆盖位上的 `(1,1)` 空壳。禁止把 dense `merged_ranges`/`column_edges` 拷进 `grid`。bbox 留 locator。form `widths_mm` = 现有 `pdf_tables.widths_mm`。

### 3.2 DOCX

只改 `_docx_structured_units`：`tblGrid`/`gridSpan`/`vMerge`；禁止 `row.cells` 重复合并。`TABLE_REGION` + `document` 身份 + `grid`；`text=""`。有正列宽才写 `widths_mm`。已删 Document `TABLE_ROW`；表内图挂网格 column。附件 `text=part_name`。`w:sdt` 无 grid，不进 `read_form`。

### 3.3 XLSX / XLSM

sheet `SECTION` 只留标题+身份。有 ListObject：每个 Table 一份 `TABLE_REGION` + `grid` + form，`text=""`。无 ListObject 的非空 sheet：used range 一份 `TABLE_REGION` + `grid` + form（key `sheet:{n}:used`），并保留行文本 `TABLE_ROW`。不把整张 sheet 当一张表。列宽只写非默认 customWidth；无则省略 `widths_mm`，编制 fail-closed。知识库 Excel markdown 不改。`.xls` 走 OLE 白名单，不执行 VBA；无 LibreOffice 转 xlsx 则无 grid，冻结 fail-closed。

### 3.4 旁路正确性

| 项 | 归属 |
| --- | --- |
| 嵌入图 Y 轴 / MIME 以解码为准 | S0b |
| 整单 `EmptyOcr` | 空 OCR 分图：源文本空，须 unresolved |
| `search_sources` | 格命中 `{form_id,row,column}` + `form_offset` |

## 4. 阶段

| 阶段 | 内容 | 完成条件 |
| --- | --- | --- |
| S0 | 身份 locator（7–12 非空则拒绝）；PDF 发出稀疏 `unit.grid`；`p_units.grid`；SQL CHECK `(1,2,3)` 从 grid 插入；**改写** `table_block_from_grid`；密铺下标含 `validate_record`；图 parent 双接受（无 grid 仍认 ROW）；转换 v3 | **合并表 BiddingFile 编制仍通过**（稀疏 form，无 `merged_ranges`）；`read_form` 覆盖位 null；`put_record` 用密铺下标；FrozenInput locator 无 cells |
| S0b | 图 Y 轴 / MIME | 带图 PDF 不因 MIME/倒立坐标失败 |
| S1 | DOCX OOXML grid；parent 只挂表；删 TABLE_ROW；附件非空 | 合成 DOCX 合并/空格/`read_form`；无 TABLE_ROW 时表内图 decode 通过 |
| S2 | ListObject → form；无 Table 的 sheet 用 used range；1-based→0-based 行**和列** | 有 Table 或 used range 可 `read_form` |
| S5 | PDF 4-gram/空表不回退；上传含 XLSM | 随 S0–S2 |

S1 依赖 S0。S2 可与 S1 并行。S0b 不挡 S1。

**本期不做：** MinerU HTML→units。招标 `.doc`/`.xls` 已进白名单（OLE，不执行宏）。

## 5. 测试

- PDF：full coverage / pdf_tables；schema 3 经编制与 `read_form`；合并表 `validate_span` 在 `read_form` 返回的格上通过；locator 无 cells。
- DOCX：合并、空锚点、表内图无 ROW、embeddings 非空 text。
- XLSX/XLSM：ListObject form；无 Table 的非空 sheet 出 used-range form；列平移；上传含 XLSM MIME。
- 编制：无 widths 不得均分；PDF 有 widths 的表仍能编。
- 知识库：默认 Office markdown 仍非空。

## 6. 后续

已落地：`search_sources` 格命中；无 units 禁止 markdown body 回退；空 OCR 分图（源文本空，OCR 对象为 `unresolved-empty-ocr-v1`）；`.doc`/`.xls` 与 upload/SQL/`convert_tender_source` 同批（OLE，不执行宏）。

仍后置：Office 原页 SourceView；Docling/MinerU HTML→units；Excel 字符列宽不换算成 mm。
