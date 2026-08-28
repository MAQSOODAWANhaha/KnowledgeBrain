# 招投标

| 项 | 值 |
| --- | --- |
| 状态 | Target V2目标契约已确认，正在分阶段实现；旧V1文档只作删除定位 |
| 角色 | 网络安全产品与服务应标方（乙方） |
| 部署策略 | Target V2 clean-slate fresh redeploy，不保留旧数据与旧协议 |

Target V2权威入口：

- 产品/领域契约：[`../platform/tender-to-submission-authoring.md`](../platform/tender-to-submission-authoring.md)
- 分阶段实施：[`../../plans/bidding/tender-to-submission-v2.md`](../../plans/bidding/tender-to-submission-v2.md)

固定PartSet、SubmissionGateV1、旧profile/procedural专用流程和旧API只属于当前V1实现快照，不是Target V2要求。

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

## 文档导航

- Target V2领域目标与术语：[`../platform/tender-to-submission-authoring.md`](../platform/tender-to-submission-authoring.md)
- Target V2实施方案：[`../../plans/bidding/tender-to-submission-v2.md`](../../plans/bidding/tender-to-submission-v2.md)
- V1实现/删除定位：[`domain.md`](domain.md)
- V1专题导航：[`../../plans/bidding/README.md`](../../plans/bidding/README.md)
- 招标发布与条款生命周期：[`../../plans/bidding/tender-publication.md`](../../plans/bidding/tender-publication.md)
- 两路匹配：[`../../plans/bidding/matching.md`](../../plans/bidding/matching.md)
- 人工报价：[`../../plans/bidding/quote.md`](../../plans/bidding/quote.md)
- 组卷与导出：[`../../plans/bidding/submission-export.md`](../../plans/bidding/submission-export.md)
- Durable dispatch 与失败恢复：[`../../plans/bidding/durable-dispatch.md`](../../plans/bidding/durable-dispatch.md)
- 实施与验收：[`../../plans/bidding/implementation-acceptance.md`](../../plans/bidding/implementation-acceptance.md)

已被替代的领域草案和实施评审统一收在 [`../../plans/archive/README.md`](../../plans/archive/README.md)，不再留旧路径兼容副本。
