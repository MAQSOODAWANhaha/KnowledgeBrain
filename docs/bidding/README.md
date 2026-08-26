# 招投标

| 项 | 值 |
| --- | --- |
| 状态 | 最终 V1 领域目标已定义；产品主链部分已落位，durable dispatch 替换和完整验收未完成 |
| 角色 | 网络安全产品与服务应标方（乙方） |
| 部署策略 | clean-slate fresh redeploy，不保留旧数据与旧协议 |

## 当前证据边界（2026-08-26）

| 层次 | 状态 |
| --- | --- |
| implemented | 部分；产品主链已有实现，durable dispatch 深 module 与旧两跳 recovery 删除尚未实施 |
| locally verified | 部分；已有定向证据不能证明新 dispatch 合同，完整 workspace、强制活库与 fresh runtime 需重跑 |
| committed | 否；当前工作树存在未提交变更 |
| pushed | 否 |
| deployed | 否 |
| runtime accepted | 否；`phase_1d_runtime_complete=false` |

旧运行记录不能替代当前 checkout 的 acceptance。最终状态以 [`../../plans/bidding/implementation-acceptance.md`](../../plans/bidding/implementation-acceptance.md) 的分层证据为准。

## 权威文档

- 领域目标与术语：[`domain.md`](domain.md)
- 完整实施方案入口：[`../../plans/bidding/README.md`](../../plans/bidding/README.md)
- 招标发布与条款生命周期：[`../../plans/bidding/tender-publication.md`](../../plans/bidding/tender-publication.md)
- 两路匹配：[`../../plans/bidding/matching.md`](../../plans/bidding/matching.md)
- 人工报价：[`../../plans/bidding/quote.md`](../../plans/bidding/quote.md)
- 组卷与导出：[`../../plans/bidding/submission-export.md`](../../plans/bidding/submission-export.md)
- Durable dispatch 与失败恢复：[`../../plans/bidding/durable-dispatch.md`](../../plans/bidding/durable-dispatch.md)
- 实施与验收：[`../../plans/bidding/implementation-acceptance.md`](../../plans/bidding/implementation-acceptance.md)

已被替代的领域草案和实施评审统一收在 [`../../plans/archive/README.md`](../../plans/archive/README.md)，不再留旧路径兼容副本。
