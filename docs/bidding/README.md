# 招投标

| 项 | 值 |
| --- | --- |
| 状态 | Target V2 Phase 0–7 后端 clean-slate 实现；Web 编制面按独立计划交付 |
| 角色 | 网络安全产品与服务应标方（乙方） |
| 部署策略 | Target V2 clean-slate fresh redeploy，不保留旧数据与旧协议 |

Target V2 权威入口：

- 产品 / 领域 / Web 交互契约：[`authoring.md`](authoring.md)
- 仓库级产品与视觉：[`../../PRODUCT.md`](../../PRODUCT.md)、[`../../DESIGN.md`](../../DESIGN.md)
- 后端实施：[`../../plans/bidding/tender-to-submission-v2.md`](../../plans/bidding/tender-to-submission-v2.md)
- Web 编制面实施：[`../../plans/bidding/frontend-authoring.md`](../../plans/bidding/frontend-authoring.md)

后端已直接替换固定 PartSet、SubmissionGateV1、①～⑥ 向导和旧 profile/procedural 专用身份；转换、`SourceUnitSpanV2`、KnowledgeRetrieval V3、QuoteSnapshot、CAS 和 ObjectRegistry 只通过 V2 身份链接入。

## 当前交付边界

| 层次 | 状态 |
| --- | --- |
| backend | Phase 0–7：Tender Source、Requirement、Workspace、Outline、Evidence、Candidate、Assessment、Preview、DOCX/PDF Export |
| runtime | fresh baseline；五类 `bid-authoring-v2` 粗粒度 Job；API 只暴露招投标 `/api/v2` |
| compatibility | 不迁移、不双写、不保留 V1 façade、first-launch、Part/Gate 或 runtime 双模式 |
| web | 独立交付；本轮后端不得用 mock success 代替未实现的 Web 能力 |
| acceptance | 以当前 checkout 的 fresh SQL、Rust、真实 API→Redis→Worker 和删除扫描结果为准；历史日志不替代重跑 |

部署、运行、故障恢复和验收命令见 [`backend-runbook.md`](backend-runbook.md)。

## 文档导航

- Target V2 领域目标、术语与 Web 编制面：[`authoring.md`](authoring.md)
- Target V2 后端实施方案：[`../../plans/bidding/tender-to-submission-v2.md`](../../plans/bidding/tender-to-submission-v2.md)
- Target V2 Web 编制面：[`../../plans/bidding/frontend-authoring.md`](../../plans/bidding/frontend-authoring.md)
- 方案总入口：[`../../plans/bidding/README.md`](../../plans/bidding/README.md)
- 现码对照（§1 是当前目标，其余是现码；权威契约在编制文档）：[`current-code.md`](current-code.md)
- 招标发布与条款生命周期（可复用转换 / 待换主路径）：[`../../plans/bidding/current-code/tender-publication.md`](../../plans/bidding/current-code/tender-publication.md)
- 两路匹配（检索仍要 / 向导与 part 待换）：[`../../plans/bidding/current-code/matching.md`](../../plans/bidding/current-code/matching.md)
- 人工报价（QuoteSnapshot 可复用，非黄金路径）：[`../../plans/bidding/current-code/quote.md`](../../plans/bidding/current-code/quote.md)
- 后端部署、运行、故障恢复与验收：[`backend-runbook.md`](backend-runbook.md)
- 组卷与导出现码历史对照：[`../../plans/bidding/current-code/submission-export.md`](../../plans/bidding/current-code/submission-export.md)
- Durable dispatch 历史对照：[`../../plans/bidding/current-code/durable-dispatch.md`](../../plans/bidding/current-code/durable-dispatch.md)
- V1 runtime 历史验收切片（不是 Target V2 产品完成）：[`../../plans/bidding/current-code/implementation-acceptance.md`](../../plans/bidding/current-code/implementation-acceptance.md)
