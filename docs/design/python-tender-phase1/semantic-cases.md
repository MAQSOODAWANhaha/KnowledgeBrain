# v2合成语义核验样本

[semantic-cases.json](semantic-cases.json)已迁移为当前v2合同：共32个可单独导出的
`operation=review`输入，含13组正反配对和2个跨文档样本，另加4个漏项目标、异常例外及补读路由样本。所有来源为合成数据，预期是
agent编写、待人工复核的草案；human_review_completed、model_evaluation_completed和
semantic_acceptance均为false，不能据此宣称模型准确率或完整流程通过。

## 覆盖范围

| 场景 | 主要核对内容 |
| --- | --- |
| unit-assignment | 30天交付与60万元报价的数字/单位归属 |
| subject-assignment | 项目经理与实施工程师人数归属 |
| comparison-direction | 至少与至多颠倒 |
| comparison-omission | 有下限但遗漏上限，需读取完整来源 |
| comparison-equivalence | 不大于与不超过等价；删除否定不等价 |
| condition-scope | 第一标段条件是否被扩大为全部标段 |
| exception-preservation | 不可抗力例外是否被删除 |
| table-unit | 单元格数值继承同列表头单位 |
| within-range-omission | range已处置仍可能遗漏其内部维护义务 |
| background-only | 背景事实不能改成投标义务 |
| partial-amendment | 相同原事项下，替换决议是否保留未修改的维护规则 |
| section-identity | 同名标题在不同标段的身份与上下文 |
| partially-blank-table | 空填报字段仍可承载明示提交要求 |
| joint-conflict | 两份文档约束同一标段，期限上下界冲突 |
| joint-compatible | 两份文档分别约束不同标段，不应误判冲突 |
| context-is-not-omission-target / real-omission-in-review-target | 上下文未带事项不等于全局漏项；真正目标范围内漏抽仍需发现 |
| optional-exception-needs-local-correction | 异常字段中的例外须结合原文核对，续报既有问题、无需额外原文 |
| unseen-attachment-needs-source | 未提供附件正文时请求补读，不能推断整个文档集缺少附件 |

## 与v1的差异

- 输入为完整v2公共信封及source_views/items/relations/decisions/dispositions/issues/candidates。
  复用当前字段定义，不另造评测专用业务合同。
- 表格cells提供literal text；跨单元格的旧整行引文拆为各可见单元格，保留表头、说明和空格。
- 部分补遗不再用applicable=false表达整条失效。两例使用相同原事项/替换事项，差别在
  amendment.affected_scope和retained_rules，预期观察决议接受与保留义务。
- 跨文档样本已可按当前联合review输入导出；各文档身份、角色与来源独立，不假造单文档。
- source_corpus_sha256记录迁移前语料摘要；此次旧快照及迁移脚本在私有验证目录
  `/tmp/kb-semantic-v2/`。预期文字做了合同适配，但没有改成人工ground truth。

condition-scope的明确绑定只是合成输入前提，不表示真实文件已完成范围裁决。
本批测试独立review，尚未覆盖locate/associate的整链生成，也不能代替Extract逐项完整性、
评分规则/否决边界的完整业务评测或真实文件人工验收。

校验/导出和人工判定见[评测流程](semantic-evaluation.md)。导出时case_id、category、
provenance和expected只留在本地review目录，不进入模型输入；不使用关键词匹配自动判断语义。
