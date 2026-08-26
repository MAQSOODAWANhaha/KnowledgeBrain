# 招投标

| 项 | 值 |
| --- | --- |
| 状态 | 最终 V1 主链已实现；当前 checkout 的完整回归、提交、部署与 runtime acceptance 未完成 |
| 角色 | 网络安全产品与服务应标方（乙方） |
| 部署策略 | clean-slate fresh redeploy，不保留旧数据与旧协议 |

## 当前证据边界（2026-08-26）

| 层次 | 状态 |
| --- | --- |
| implemented | 是；当前工作树包含完整 V1 主链，以及 eligible/hit 解耦、knowledge scope attestation、PDF 附件 durable preparation |
| locally verified | 是；Rust workspace/Clippy、强制活库 SQL/HTTP、Web lint/build/mocked e2e、fresh-schema/ACL 与删除扫描已通过 |
| committed | 是；本轮增量已收拢为当前交付提交 |
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
- 实施与验收：[`../../plans/bidding/implementation-acceptance.md`](../../plans/bidding/implementation-acceptance.md)

已被替代的领域草案和实施评审统一收在 [`../../plans/archive/README.md`](../../plans/archive/README.md)，不再留旧路径兼容副本。
