# 招投标文档入口

分析走 `register → freeze → extract → verify → confirm`，确认后是 `analysis-ready`。编制是独立管道，先跑大纲执行，再跑正文执行，产出可编辑 DOCX 并经 ONLYOFFICE 编辑、保存与同版 PDF 出件。

| 内容 | 文档 |
| --- | --- |
| 分析节点、放行条件、各阶段发送次数 | [业务工作流](workflow.md) |
| 编制职责与缺资料处理 | [编制架构](authoring.md) |
| 字段、类型、枚举与编制合同 | [字段字典](field-dictionary.md) |
| 分析模块接口与执行边界 | [领域模块合同](domain-contract.md) |
| 工作台交互与稿件行为 | [投标工作台](workbench.md) |
| ONLYOFFICE 编辑、保存、版本与 PDF 出件 | [ONLYOFFICE](onlyoffice.md) |
| 来源格式进 Word 规则 | [Word 导出格式](word-export.md) |
| 验收口径与最低验收线 | [验收](acceptance.md) |
| 语义评测语料与方法 | [语义样本](semantic-cases.md)、[语义评测](semantic-evaluation.md) |
| 问题根因和修复 | [问题记录](bug-fix.md) |
| 部署状态与各样本当前基线 | [交接](handoff.md) |
| 本机启动与运维命令 | [运行手册](runbook.md) |

文档维护与命名规范见 [docs/README.md](../README.md) 与 [工程与验收标准](../code-standards.md)。
