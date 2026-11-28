# 招投标验收与语义评测

分析流程与发送边界见 [业务工作流](workflow.md)，字段见 [字段字典](field-dictionary.md)，当前进度只看 [招投标入口 · 当前状态](README.md#当前状态交接)。离线回归、确定性模型和真实文件验收不能互相代替。

## W2 编制能力验收

W2 验收系统在本次实际输入下能否产出可追溯、可继续编制的投标稿。每份样本固定已收到的招标原件、补遗、企业资料和分析修订，包括缺必要附件、无企业资料或部分材料可用的场景。按已收原件逐项核对已知要求、原式、签章和文件结构；缺少依据的内容保留具名缺口和补办位置。缺资料场景满足四项最低线即计入 W2。对外递交时另行核定材料、商务选择和签署。

| 维度 | 需要证明 |
| --- | --- |
| 稳定 | 固定来源、代码、解析器和模型配置；重新生成和中断恢复能解释关键组成、原式范围与未决项变化 |
| 正确 | 回到原件核对组成、资格与否决、评分依据、参数、原式、条件和例外；Schema 合法不等于语义完整 |
| 一致 | 来源到成品、成品到来源都能对上最终 Word 版本；章节、表、附件与应答不串位 |
| 高效 | 质量和完整性相同后再比调用、重试、Token 与耗时；修订只重跑受影响任务 |

## 最低验收线

第一份真实样本在其实际输入下通过以下四条最低线即记 `1/4`；过线前不新增协议或阶段，只修缺陷和推进 [W2 一条要求链](../../plans/improvement-roadmap.md#w2-一条要求链)：

1. **要求不漏**：以已收原件独立抽核分析事项，每条适用事项都有响应位置或明确列在缺口清单；分析输入中已登记的缺件逐件保留到交付待补清单，缺件影响的条款留待核；人工抽查原件 3 处以上评分表和资格条款，无台账和缺口清单双漏项。
2. **原式表正确**：从本次已收原件及适用补遗独立列出已知必交表，逐张核对 Word 里的对应表，列数、表头、合并单元格、固定文字与签章位置与原件一致。
3. **缺口不被编造**：无企业资料的位置留空或写待补，不出现虚构证书、业绩、人员、参数或价格。
4. **文件可用**：同一份 DOCX 能在 ONLYOFFICE 打开、保存、重开并导出 PDF。

逐字保真只要求原式表格、固定文字与签章位置；其余正文解析造成的空白/换行差异记为解析器缺陷，不判定验收失败。解析不完整、附件未展开、Excel 显示不完整、OCR 读不出保留为可追溯问题，不能靠确认清除；未知 VLM 容量时不挪用对话模型容量。

### 离线最低验收线评测脚本

使用 `services/docreader/tender/acceptance_eval.py` 对一次编制执行的大纲、Word 稿件、交付包与阶段报表离线打分（不发模型请求、不写库、不联网）：

```bash
cd services/docreader && uv run --frozen python -m docreader.tender.acceptance_eval \
  --outline <outline.json> --docx <投标文件.docx> \
  --requirements <招标要求底稿.json> --coverage <覆盖结算.json> \
  --gaps <待补事项.json> --report <authoring-report.json> \
  --artifact-root <artifact-root> --required-tables <独立原式清单.json>
```

原式清单随冻结来源复用。清单完整度表示对本次冻结的已收原件是否盘点完整，每张表引用该来源的原生网格；单列缺失文书及有依据的适用性待核。已收原件只抽查部分表时，原式总体保留待核。清单格式见[字段字典](field-dictionary.md)。

也可直接传 `--package <交付包.zip>`（自动读取包内 DOCX、底稿、覆盖结算、待补事项与版式核对）或 `--result <document-result.json> --artifact-root <artifact-root>`。脚本按最低验收线四项输出每项状态与具体漏项/错表位置（字段见[字段字典](field-dictionary.md)的 `acceptance-eval`）：离线脚本只能给出机器核对结果，不能替代人工抽查原件（第 1 项）、人工真实性核验（第 3 项）或 ONLYOFFICE 实测回执（第 4 项）；未提供对应实测/人工回执时状态保留 `needs_human_check` / `needs_onlyoffice_check` 或 `unknown`，不会静默判通过。

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
