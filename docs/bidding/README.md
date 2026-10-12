# 招投标文档入口

当前生成合同只有一份：[证据与运行时合同](../design/evidence-runtime-contract.md)。实施状态与反例门槛只有一份：[实施计划](../design/check-execution-plan.md)。两者区分当前实现、离线回归及真实服务验收；checkpoint 34 的自动支持交付已实现，本地回归收尾状态见实施计划，真实完整语义验收尚未通过。

用户在解析完成后主动启动一次大纲运行：Discover 提取要求，Organize 建立章节、规定模板与具体响应目标，Check 独立复核来源、主张和模板。结构就绪不等于复核通过；没有旧的草稿／正式编制双流程。产物是冻结 OutlineArtifact，投标人事实和签字空位保持空白，不为通过验收编造材料。

knowledge 匹配填充属于下一阶段，只消费冻结大纲，不重新解析招标、不改规定文字。本轮没有接通该业务。正文编辑、保存、下载使用已保存 DOCX 与 ONLYOFFICE；不另设强制终稿等级或复核门阻止用户下载，转换/保存失败仍明确报告。

| 文档 | 唯一职责 |
|---|---|
| [运行时合同](../design/evidence-runtime-contract.md) | 当前执行、证据、模型配置、目标和发布边界及已知限制 |
| [实施计划](../design/check-execution-plan.md) | 实施状态、反例矩阵和验收门槛 |
| [冻结输入](frozen-input.md) | 解析、来源身份与冻结不变量 |
| [模块归属](authoring.md) | analysis / outline / response 的代码责任 |
| [运行手册](backend-runbook.md) | 当前故障定位与安全恢复 |
| [ONLYOFFICE](onlyoffice.md)、[保存协议](docx-editor.md)、[版本](docx-rounds.md) | 编辑、保存及同版本导出，不另定义生成合同 |
| [发布入口](release-entry.md) | 发布工具操作边界与待验证项，不作为语义验收 |

公开回归使用合成输入。私有原文、文件名、模型结果与历史费用账本不进入仓库；旧运行保留私有审计，不迁移为新合同进度。删除的重复历史占位页不再作为通过证据，历史实现可从 Git 查询。
