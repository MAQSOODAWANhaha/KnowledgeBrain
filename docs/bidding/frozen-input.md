# Preparing a complete frozen tender

The internal producer is `prepare-tender-input`. It reads original source files, invokes the configured DocReader tender parser, completes required literal OCR, restores parser order, validates the SourceContract V2, and publishes one owned frozen snapshot. Its output is the canonical input digest accepted by the existing tender-processing job. HTTP upload and frontend wiring are outside this change.

```sh
cargo run -p bidding --bin prepare-tender-input -- tender-manifest.json
```

Configure the existing database, blob storage, DocReader and VLM settings for this process. Do not put credentials in the manifest. The project must already exist. Source paths are relative to the manifest file.

```json
{
  "project_id": "00000000-0000-0000-0000-000000000001",
  "document_set_id": "tender-2026-01",
  "parser_contract_version": "source-v2",
  "documents": [
    {"document_id": "main-tender", "file_path": "sources/tender.pdf", "role": "primary"}
  ],
  "document_relations": [],
  "decisions": []
}
```

The default combined raw-file limit is 256 MiB. `KB_TENDER_PREPARE_MAX_BYTES` changes that explicit preparation budget. The command stages source and image objects before publication; the final database transaction acquires durable ownership of the complete snapshot and registers its digest. Failure abandons temporary staging references and never registers a partial frozen input. Physical cleanup follows the existing retention rules.

`outline::frozen::prepare_tender_input` is the equivalent internal service. `prepare_tender_input_with` exposes typed parser and image-processing ports for deterministic integration tests. Neither treats a caption as OCR, guesses missing image order, nor accepts a missing parser contract. Token-limited OCR triggers bounded spatial subdivision with stable region coordinates and exact overlap merging. Incomplete termination or ambiguous regional overlap blocks freezing. Original region text and physical-call counts remain in provenance. Textless nonblank images retain `ocr_complete=false` and require original-image vision. Original pixels are read with `read_source_view`, transported as bounded full-frame image content to the same configured authoring model, and only credited after a completed response confirms that exact request. Discover and Check have separate image-reading receipts. Configure `limits.vision_enabled` only when that same authoring model supports image input. A text-only model produces an explicit capability error rather than silently treating the image as a negative scan.

The frozen schema is version 3 and the supported parser contract is exactly `source-v2`. Source text and grids are verified against the parser's digests; OCR has separate provenance and a literal-text digest. Every required unit, image and physical page must be accounted for. Known partial inventory carriers block freezing rather than becoming successful empty evidence. New source bytes, parser version or OCR results produce a different canonical input digest.

Discovery uses ordered atoms, carrier-specific evidence and claimed pack revisions. Empty-text grids remain visible. Oversized cells are split at UTF-8 boundaries while preserving anchor identity. Repeated table headers are context with their original evidence identity. Stored image locators alone do not grant visual-reading evidence; actual transported image pixels require a completed-response receipt before an `ImageRegion` citation or whole-pack negative scan is accepted.

Native DOCX/XLSX contract fixtures contain the original OOXML bytes and the real producer responses. Regenerate them with `python services/docreader/scripts/generate_native_office_fixtures.py crates/docparser/tests/fixtures/python-native-office-v2.json`. This isolated local harness loads the unchanged native parser modules without importing optional MarkItDown/ONNX engines. Archive clocks are fixed so consecutive runs produce identical fixtures. The existing mixed-PDF fixture separately covers text, scanned and blank pages.

### Excel 原生单元格保真

SpreadsheetCell 从 Python 原生解析经 protobuf 和 Rust 保留 `raw_value`、
`value_type`、`number_format`、`display_text`、`display_complete`、
`display_incomplete_reason`，以及公式表达式、引用和可用缓存值/缓存类型。
Frozen 的原生 locator 保留这些字段；表格正文采用可靠显示值，显示未知时保留原值。

显示器只处理明确支持的数字小数位、分组、百分比、引号单位和 `yyyy-mm-dd` 日期。
未知自定义格式、未实现的 General 数值显示、公式缓存缺失或缓存错误均明确标记，
不计算公式，不把公式文本当计算结果。单元格显示不完整不等同于整个来源丢失；
后续业务解释必须结合原值、格式与不完整原因。原生元数据计入解析载荷上限。

`generate_excel_wire_fixture.py` 生成无用户资料的 XLSX，并调用实际 ExcelParser 与
生产 protobuf 编码器。Rust 测试解码该 wire fixture，复用生产 unary 响应转换，先核对
protobuf 单元格与 source contract 的物理 locator 一致，再构建并验证 Frozen。
这证明跨语言元数据传递，不能替代真实模型语义验收或证明所有 Excel 格式均受支持。

### PDF＋Excel 关联读取

Frozen schema 3 使用唯一的类型化 documents/document_relations 清单，文档角色必须显式提供；解析合同仍为 source-v2。已确认关系必须是显式引用，具有存在的来源、目标、原文引用依据和匹配的原生 locator。主机校验原文存在性，不替模型证明引用语义。必需关系未确认时不能完成。

Discover 包可包含相邻且有已确认关系的多个文档，各原子保留自己的文档、版本与原生来源身份。跨包依赖通过既有 read_evidence 读取，不能借用目标包的完成状态。关联支持使用关系来源身份，不创建虚假的需求记录。

关联读取按真实下一次完整请求预算分页，保留 UTF-8 边界。游标绑定运行、包、修订和输入摘要；只有该工作者持久化的完整模型响应确认了准确请求，才授予阅读信用。已确认交付的原文工具消息可退出历史；模型留下的观察仍是模型陈述，原始来源可重新读取。条件支持保留可恢复的证据引用，Check 必须重新取得自己的阅读回执。来源集合变化后必须重新执行整个 Check。

本地合成 PDF＋Excel 纵向测试覆盖解析、冻结、关联分页、条件支持、Organize 和新的 Check；不代表真实服务或语义验收。Word/图片跨文档关联扩展、局部 repair 和后续性能工作仍未完成。单个不可再拆的原生元数据若超过完整请求预算，会明确失败，尚未实现元数据分片。

真实验收 runner 必须从原始文件重新解析并生成 schema 3 的新冻结输入；不得改写旧快照版本、迁移旧阅读回执或复用旧来源集合的 Check 信用。

关联目标指定空正文的原生网格单元时，主机从该单元拥有的表格签发 GridCell 引用。并发工作者的 source key 绑定完整输入摘要和自身运行、包修订及 claim；其他包提交不使在途读取失效，自身 reopen 或依赖来源／关系变化仍拒绝旧信用。检查点契约 23 拒绝旧作用域合同；SQL 发布入口只接受 Frozen schema 3。

已删除绕过冻结校验并生成 schema 1 的旧 Python sample writer。保留的 `scripts/bidding_source_captions.py` 仅提供标题辅助函数；CI 运行对应 caption 测试。新的本地验收必须调用正常 parse／freeze 管线，不能复用旧 writer 输出。
