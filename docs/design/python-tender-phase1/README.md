# Python 招标

分析走 `register → freeze → extract → verify → confirm`，确认后是 `analysis-ready`。编制是另一条管道，先跑大纲执行，再跑正文执行。

| 内容 | 文档 |
| --- | --- |
| 节点、放行条件、各阶段发送次数 | [业务工作流](business-workflow.md) |
| 编制职责与缺资料处理 | [编制架构](authoring-architecture.md) |
| 字段和合同 | [字段字典](field-dictionary.md) |
| 分析模块接口 | [领域模块合同](domain-contract.md) |
| 验收口径 | [验收](acceptance.md) |
| 语义评测 | [语义样本](semantic-cases.md)、[语义评测](semantic-evaluation.md) |
| 问题根因和修复 | [问题记录](bug-fix.md) |
| 跑到哪一步 | [交接](handoff.md) |
| 怎么启动 | [运行手册](runbook.md) |

文档维护规则见 [docs/README.md](../../README.md)。
