# 招投标验收与语义评测

分析流程与发送边界见 [业务工作流](workflow.md)，字段见 [字段字典](field-dictionary.md)，当前进度只看 [招投标入口 · 当前状态](README.md#当前状态交接)。离线回归、确定性模型和真实文件验收不能互相代替。

## 最终投标文件验收

验收对象是同一版本的投标文件是否盖住当前原件和补遗里适用的要求。分析通过、目录通过或成功出件，都不能单独算整份通过。分母须从全部原件和补遗建立，不能从模型已选大纲反推；`manual` 或 `not_required` 不代表义务已完成；没有企业资料时只能验收模板、原式与空位，结论不能写成通过。

| 维度 | 需要证明 |
| --- | --- |
| 稳定 | 固定来源、代码、解析器和模型配置；重新生成和中断恢复能解释关键组成、原式范围与未决项变化 |
| 正确 | 回到原件核对组成、资格与否决、评分依据、参数、原式、条件和例外；Schema 合法不等于语义完整 |
| 一致 | 来源到成品、成品到来源都能对上最终 Word 版本；章节、表、附件与应答不串位 |
| 高效 | 质量和完整性相同后再比调用、重试、Token 与耗时；修订只重跑受影响任务 |

## 最低验收线

第一份真实文件先过以下四条最低线才算 `1/4`；过线前不新增协议或阶段，只修缺陷和推进 [W2 一条要求链](../../plans/improvement-roadmap.md#w2-一条要求链)：

1. **要求不漏**：以分析事项为分母，每条适用事项都有响应位置或明确列在缺口清单；人工抽查原件 3 处以上评分表和资格条款，无台账和缺口清单双漏项。
2. **原式表正确**：招标指定格式表在 Word 里是表，列数、表头、合并单元格、固定文字与签章位置与原件一致。
3. **缺口不被编造**：无企业资料的位置留空或写待补，不出现虚构证书、业绩、人员、参数或价格。
4. **文件可用**：同一份 DOCX 能在 ONLYOFFICE 打开、保存、重开并导出 PDF。

逐字保真只要求原式表格、固定文字与签章位置；其余正文解析造成的空白/换行差异记为解析器缺陷，不判定验收失败。解析不完整、附件未展开、Excel 显示不完整、OCR 读不出保留为可追溯问题，不能靠确认清除；未知 VLM 容量时不挪用对话模型容量。

## 合成语义核验语料与评测

合成评测语料位于 [semantic-cases.json](semantic-cases.json)（v2 合同，32 个独立 `operation=review` 输入：13 组正反配对、2 个跨文档样本、4 个漏项目标/异常例外/补读路由样本）。原文为合成数据，`expected` 为 agent 草案，`human_review_completed` / `model_evaluation_completed` / `semantic_acceptance` 均为 `false`。

- **覆盖场景**：`unit-assignment`、`subject-assignment`、`comparison-{direction,omission,equivalence}`、`condition-scope`、`exception-preservation`、`table-unit`、`partially-blank-table`、`within-range-omission`、`background-only`、`partial-amendment`、`section-identity`、`joint-{conflict,compatible}`、`context-is-not-omission-target` / `real-omission-in-review-target`、`optional-exception-needs-local-correction`、`unseen-attachment-needs-source`。
- **离线校验与导出**（不读模型凭证、不联网；输出目录须不存在）：

```bash
PYTHONPATH=services uv run --project services/docreader python \
  services/docreader/tests/acceptance/check_semantic_cases.py \
  --prepare /opt/workspace/kb-tender-evidence/tender-verify-offline-v2
```

导出目录包含 `model-inputs/v001.json`–`v032.json`（仅 review payload，不含答案或元数据）、`verify.md` 快照及 `review/{corpus,manifest,records}.json`。离线默认预算 `131072 / 16384 / 4096`，实测须传入目标模型 `RequestBudget`。

- **人工裁定口径**：先由人工冻结原文与草案标注再观察模型回答（agent 不能代填人工完成）；失败、`uncertain`、合同错误与未运行样本均留在分母中，未知 usage 不记 0 费用；禁止用正则/关键词自动判语义；分别报告错误命中/漏检、正确误报、宿主误阻断、未裁定与失败数。本诊断集只覆盖 `review`，不能代替抽取完整性或真实标书验收。
