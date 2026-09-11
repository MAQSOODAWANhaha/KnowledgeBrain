# 招投标实施导航

2026-09-11 UTC 当前进展：real-run-v14已使用当前已验证程序及.env的grok-4.6＋Chat＋low启动完整106页空候选提取与独立复核，来源、原图、预算均已核对，不导入旧检查点或人工答案。连续技术第55–63页此前已完成两轮局部复核（50条记录、0发现）；标签清空、未勾选条件负例均已闭环。复杂附表和跨范围关系的整体语义、完整32项及同版DOCX/PDF/报告仍待验收。未改配置、新增migration或修改生产逻辑；223项库测试、20项合同及fmt/Clippy/编译证据保留。 详见[统一方案](agent-runtime-rig.md#55-本次修复的实施门槛)。

招投标有两个职责明确的实施入口，统一进展见[执行台账](../implementation-tasks.md)。编辑、保存与出件按 [ONLYOFFICE 接入计划](onlyoffice-integration.md)：

1. O0：许可、版本、真实样稿与保真验证；
2. O1：编辑配置、权限、回调落盘与版本闭环；
3. O2：DOCX 主源、同稿 PDF 与独立报告；
4. O3：初稿/AI 定点入稿、报价附件、证据定位与页码；
5. O4：真实回归后切换、撤旧入口。

提取与 Agent 改造按 [Rig 完整实施方案](agent-runtime-rig.md)的 **Rig P0–P4**：现有驱动的精确查询/有界主复核窗口 → 附表关系与复核修复 → 真实提取验收 → 独立 Journal 升级 → Rig 单协议接入与撤旧。协议只使用 Chat Completions，模型仍由 `deploy/.env` 决定。该编号不替代平台 P0/P1/P2；通过的分析交给既有 O1-S 编制链，O2 负责同稿出件，Embedding 一致性留在知识库独立事项。

招标冻结网格按 [DocReader 结构增强](../knowledge-base/docreader-structured-parse.md)（builtin `convert_tender_source`）。知识库 Office 默认仍是 anydoc。Word/Excel（含 `.doc`/`.xls` OLE 与 XLSM）已可冻结为 `read_form` 网格（无 Excel Table 的工作表走 used range；无 `widths_mm` 时编制 fail-closed）。

**普通实施已授权。** ONLYOFFICE/DOCX 主链已有部分实现及隔离验证；提取 P0 与复核 P1 已部分实现，Journal 三边界恢复和 Rig Chat 接缝已验证，共享宿主驱动已接提取/编制，Rig 已接生产 Chat 请求序列化、流解析和 AgentRun 多轮状态，有界会话恢复已通过隔离验收。前序 clean-review 独立复核曾完成72个局部比较，但第64轮因执行/交接额度耗尽停止，32 项语义发现仍开放，完整真实 Agent DOCX/PDF 未验收。旧大纲专用链已按授权删除；生产部署、采购及现有数据操作仍需对应授权。局部或合成验证不能代替完整真实验收。

## 契约与复用

- [业务 PRD](../../docs/bidding/prd.md)：三步流程、文件复用/新轮整稿、招标文件指定目录与完整业务清单。
- [领域边界](../../docs/bidding/authoring.md)：来源/要求、初稿候选、证据/事实/报价、存储身份及所有权；替代旧长合同中的仍需约束。
- [ONLYOFFICE 技术契约](../../docs/bidding/onlyoffice.md)：真实 DOCX 编辑/保存、插件与外部 Connector 许可边界、同稿出件和页码安全出口。
- [知识媒体证据](../knowledge-base/bidding-evidence-media-v3.md)：保留 knowledge-owned media/attestation，不通过 OCR 文本反查 live 图片。
- [平台基础](../platform/runtime-foundation.md) 与 [队列](../platform/queue-runtime.md)：保留 schema/对象所有权与执行围栏，不把旧固定树编译作为接入前置。
- [运行诊断](../../docs/bidding/backend-runbook.md)：源码定位与旧测试的适用范围，不是新链完成声明。

不维护第二套 Tiptap、旧大纲生成或固定分册/Gate 计划。仍被新链使用的结构与表格原语按消费者保留，不能从它们重建用户已编辑 DOCX。模型统一读取 `deploy/.env`，解析复用 Python service；提取与运行时改造在 Rig 方案定义，Embedding 独立事项见[知识库计划](../knowledge-base/README.md#模型与索引一致性独立事项)。
