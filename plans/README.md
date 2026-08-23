# KnowledgeBrain 计划导航

`plans/` 只描述目标实现、切换步骤与验收方法。稳定的已确认定义在 [`../docs/README.md`](../docs/README.md)。

## 当前计划

| 领域 | 入口 | 状态 |
| --- | --- | --- |
| 共享平台 | [`platform/README.md`](platform/README.md) | 归属已建立，待按主题继续拆分 |
| 知识库 | [`knowledge-base/README.md`](knowledge-base/README.md) | 保留现有语义，独立演进 |
| 招投标 | [`bidding/README.md`](bidding/README.md) | clean-slate 最终 V1 方案 |
| 历史计划 | [`archive/README.md`](archive/README.md) | 仅供追溯，不是当前真源 |

## 规则

1. 新计划必须进入所属领域目录。
2. 旧 `bid-*`、评审输出和混合计划已归入 `archive/`；当前计划只从上表领域入口进入。
3. 跨领域事项拆成“拥有方规范 + 使用方端口”，不得复制数据库模型。
4. 计划中的“完成”至少区分：已实现、本地验证、已提交、已部署、真实运行验收。
5. 招投标 clean-slate 不建立兼容阶段；旧 schema、API、alias、双写和旧格式读取直接从最终实现中删除。
