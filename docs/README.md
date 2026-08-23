# KnowledgeBrain 文档导航

本目录保存已经确认的产品与领域定义。实现方案、拆分顺序和验收矩阵放在 [`../plans/README.md`](../plans/README.md)。

仓库级产品与界面规范仍保留在标准入口：[`../PRODUCT.md`](../PRODUCT.md) 与 [`../DESIGN.md`](../DESIGN.md)。

## 权威性规则

1. 每条规范只能有一个权威定义位置；其它文档只链接，不复制另一份规则。
2. `docs/` 描述已确认的当前目标与领域边界；它不等于“代码已经实现”。实现状态必须由代码、测试和运行验收分别证明。
3. `plans/` 描述如何到达目标。计划完成后，稳定结论回写到对应的 `docs/<domain>/`。
4. 历史评审、调查报告和旧实施草案不是当前规范；冲突时以本页链接的领域文档为准。
5. 跨领域能力由所属领域定义，通过端口使用；禁止在调用方重新定义被调用领域的数据模型。

## 领域

| 领域 | 当前入口 | 拥有的能力 |
| --- | --- | --- |
| 共享平台 | [`platform/README.md`](platform/README.md) | 鉴权、运行时、队列、幂等与审计基础设施、对象注册表、可观测性 |
| 知识库 | [`knowledge-base/README.md`](knowledge-base/README.md) | Workspace、Product、ProductVersion、Document、解析、索引、检索 |
| 招投标 | [`bidding/README.md`](bidding/README.md) | BidProject、招标文件、条款、匹配决策、报价、组卷与正式导出 |
| 调研材料 | [`research/README.md`](research/README.md) | 外部对标、实验和非规范性分析 |

## 现有综合规格

当前领域规则只从各领域入口进入。迁移前的综合代码说明保存在 [`research/repository-implementation-snapshot.md`](research/repository-implementation-snapshot.md)，仅用于识别实现差距，不是任何领域的第二真源。
