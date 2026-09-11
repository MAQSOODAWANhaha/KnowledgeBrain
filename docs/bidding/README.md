# 招投标

2026-09-11 UTC 当前进展：real-run-v14已使用当前已验证程序及.env的grok-4.6＋Chat＋low启动完整106页空候选提取与独立复核，来源、原图、预算均已核对，不导入旧检查点或人工答案。连续技术第55–63页此前已完成两轮局部复核（50条记录、0发现）；标签清空、未勾选条件负例均已闭环。复杂附表和跨范围关系的整体语义、完整32项及同版DOCX/PDF/报告仍待验收。未改配置、新增migration或修改生产逻辑；223项库测试、20项合同及fmt/Clippy/编译证据保留。 详见[统一方案](../../plans/bidding/agent-runtime-rig.md#55-本次修复的实施门槛)。

ONLYOFFICE/DOCX 主链已完成部分实现与隔离联调；提取 P0 与复核 P1 已部分实现，Journal 三边界恢复与 Rig Chat 接缝已通过隔离验证，共享宿主驱动已接提取/编制，Rig 已接生产 Chat 请求序列化、流解析和 AgentRun 多轮状态，有界会话恢复已通过隔离验收。前序 clean-review 独立复核曾完成72个局部比较，但第64轮因执行/交接额度耗尽停止，32 项语义发现仍开放，真实招标的 Agent 整稿及同版本 PDF 尚未通过完整验收。用户流程固定 **文件 / 编制 / 导出**。

| 职责 | 唯一入口 |
| --- | --- |
| 业务清单、招标指定目录、人员/报价/附件与检查 | [PRD](prd.md) |
| 来源、提取结果、证据、存储身份和领域所有权 | [领域契约](authoring.md) |
| 招标理解 Agent、统一解析服务、行业覆盖与验证边界 | [实现审查](tender-analysis-review.md) |
| 真实提取问题、指定格式和技术要求逐项验收 | [修订复核](real-tender-acceptance-review.md)、[格式清单](prescribed-format-acceptance.md)、[技术清单](technical-requirements-acceptance.md) |
| 正文主源、会话/保存、授权、AI 定点入稿与同稿出件 | [ONLYOFFICE 契约](onlyoffice.md) |
| ONLYOFFICE 编辑与出件顺序 O0 → O1 → O2 → O3 → O4 | [接入计划](../../plans/bidding/onlyoffice-integration.md) |
| Chat Completions、提取/复核修复与 Agent 运行时 Rig P0–P4 | [完整方案](../../plans/bidding/agent-runtime-rig.md) |
| 跨域进度、真实失败与样稿证据 | [执行台账](../../plans/implementation-tasks.md)、[性能诊断](extraction-performance.md)、[样稿记录](full-sample-results.md) |
| 当前源码接缝与运行诊断（不代表新链可用） | [运行手册](backend-runbook.md) |
| Web 外壳产品与视觉 | [PRODUCT](../../PRODUCT.md)、[DESIGN](../../DESIGN.md) |

未变且有结果的源文件复用，集合变动后新轮整稿；DOCX 是唯一正式正文，保存回调持久化后冻结出件，PDF 由同一 DOCX 转换。报价和附件在完整稿内，报告独立，单独提交由用户拆分复核。业务提示不锁编辑/导出，技术失败不伪成功。

旧编辑/组卷方案正文与迁移壳已归并移除。平台、知识库、来源、对象与历史版本继续复用。合成链路测试和手工参考稿的编辑验证，不能代替真实招标 Agent 整稿的内容验收。
