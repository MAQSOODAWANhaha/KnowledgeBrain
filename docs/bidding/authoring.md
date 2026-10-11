# 模块归属

业务入口见[招投标文档](README.md)，执行合同仅见[运行时合同](../design/evidence-runtime-contract.md)。

- `analysis`：冻结来源、运行协调、检查点/journal 及阶段接线；`analysis/outline_flow.rs` 是当前阶段状态的一部分，不是旧扫描路径的代名词。
- `outline`：Discover 要求及支持、Organize 章节与模板、独立 Check/修复、产物验证与发布。不能把它描述为“不评审要求”。
- `response`：后续消费已发布冻结大纲，按槽检索与填充；knowledge 业务接入不在本轮验收范围，不能因模块存在就宣称联通。

章节、表格、来源、槽和要求均用稳定身份关联，不以标题代替身份。检查点保存权威领域状态；对话投影不成为第二份语义真源。用户保存的 DOCX 是正文主源，OutlineArtifact 不随编辑回写；响应绑定具体 outline 摘要。
