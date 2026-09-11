# KnowledgeBrain 计划导航

2026-09-11 UTC 当前进展：real-run-v14已使用当前已验证程序及.env的grok-4.6＋Chat＋low启动完整106页空候选提取与独立复核，来源、原图、预算均已核对，不导入旧检查点或人工答案。连续技术第55–63页此前已完成两轮局部复核（50条记录、0发现）；标签清空、未勾选条件负例均已闭环。复杂附表和跨范围关系的整体语义、完整32项及同版DOCX/PDF/报告仍待验收。未改配置、新增migration或修改生产逻辑；223项库测试、20项合同及fmt/Clippy/编译证据保留。 详见[统一方案](bidding/agent-runtime-rig.md#55-本次修复的实施门槛)。

`plans/` 只描述目标实现、切换步骤与验收方法。稳定的已确认定义在 [`../docs/README.md`](../docs/README.md)。

## 当前计划

| 领域 | 入口 | 状态 |
| --- | --- | --- |
| 共享平台 | [`platform/README.md`](platform/README.md) | 归属已建立，待按主题继续拆分 |
| 知识库 | [`knowledge-base/README.md`](knowledge-base/README.md) | 保留现有语义；招标冻结网格 S0–S2/XLSM 已落地（不切换 Office anydoc） |
| 招投标 | [`bidding/README.md`](bidding/README.md) | ONLYOFFICE/DOCX 部分实现与隔离验证；提取 P0/复核 P1 部分实现，Journal 恢复与 Rig Chat 接缝已验证；前序局部复核在72个比较后仍未收尾，双向复核与宿主汇总已实现、真实短测进行中；真实整稿未验收；网格见 [冻结方案](knowledge-base/docreader-structured-parse.md) |

## 规则

1. 新计划必须进入所属领域目录。
2. 当前计划只从上表领域入口进入；过期草案直接删除，不再另建 archive。
3. 跨领域事项拆成“拥有方规范 + 使用方端口”，不得复制数据库模型。
4. 计划中的“完成”至少区分：已实现、本地验证、已提交、已部署、真实运行验收。
5. 招投标编辑/出件按 ONLYOFFICE O0–O4，Agent 改造按 Rig P0–P4；两者在真实整稿验收汇合，不形成两套正文主源。旧大纲专用链已按用户授权撤除，完整新链验收仍待完成；历史用户资料、清库和生产部署不因文档更新获得授权。
