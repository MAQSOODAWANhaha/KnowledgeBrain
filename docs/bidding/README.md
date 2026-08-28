# 招投标

| 项 | 值 |
| --- | --- |
| 状态 | Target V2 目标契约与 Web 编制交互已确认，正在分阶段实现。V1 不是整份作废；变的是产品主路径 |
| 角色 | 网络安全产品与服务应标方（乙方） |
| 部署策略 | Target V2 clean-slate fresh redeploy，不保留旧数据与旧协议 |

Target V2 权威入口：

- 产品 / 领域 / Web 交互契约：[`authoring.md`](authoring.md)
- 仓库级产品与视觉：[`../../PRODUCT.md`](../../PRODUCT.md)、[`../../DESIGN.md`](../../DESIGN.md)
- 后端实施：[`../../plans/bidding/tender-to-submission-v2.md`](../../plans/bidding/tender-to-submission-v2.md)
- Web 编制面实施：[`../../plans/bidding/frontend-authoring.md`](../../plans/bidding/frontend-authoring.md)

因目标变更而必须换掉的是固定 PartSet、SubmissionGateV1、①～⑥ 向导和旧 profile/procedural 专用身份。转换、SourceSpan、知识检索、QuoteSnapshot、CAS 仍要接入 V2。

## 当前证据边界（2026-08-27）

| 层次 | 状态 |
| --- | --- |
| implemented | 部分；V1 主链仍在，V2 壳与契约已部分落位，Word 式整篇画布未完成 |
| locally verified | 部分；黄金路径 E2E、完整 workspace、强制活库与 fresh runtime 需重跑 |
| committed | 否；当前工作树存在未提交变更 |
| pushed | 否 |
| deployed | 否 |
| runtime accepted | 否；`phase_1d_runtime_complete=false` |

旧运行记录不能替代当前 checkout 的 acceptance。V2 完成以实施方案与编制契约验收为准，不以 V1 Gate 为准。

## 文档导航

- Target V2 领域目标、术语与 Web 编制面：[`authoring.md`](authoring.md)
- Target V2 后端实施方案：[`../../plans/bidding/tender-to-submission-v2.md`](../../plans/bidding/tender-to-submission-v2.md)
- Target V2 Web 编制面：[`../../plans/bidding/frontend-authoring.md`](../../plans/bidding/frontend-authoring.md)
- 方案总入口：[`../../plans/bidding/README.md`](../../plans/bidding/README.md)
- 现码对照（§1 是当前目标，其余是现码；权威契约在编制文档）：[`current-code.md`](current-code.md)
- 招标发布与条款生命周期（可复用转换 / 待换主路径）：[`../../plans/bidding/current-code/tender-publication.md`](../../plans/bidding/current-code/tender-publication.md)
- 两路匹配（检索仍要 / 向导与 part 待换）：[`../../plans/bidding/current-code/matching.md`](../../plans/bidding/current-code/matching.md)
- 人工报价（QuoteSnapshot 可复用，非黄金路径）：[`../../plans/bidding/current-code/quote.md`](../../plans/bidding/current-code/quote.md)
- 组卷与导出（目标改为 Workspace 快照；PartSet/Gate 待删）：[`../../plans/bidding/current-code/submission-export.md`](../../plans/bidding/current-code/submission-export.md)
- Durable dispatch：[`../../plans/bidding/current-code/durable-dispatch.md`](../../plans/bidding/current-code/durable-dispatch.md)
- V1 runtime 验收切片（不是产品完成）：[`../../plans/bidding/current-code/implementation-acceptance.md`](../../plans/bidding/current-code/implementation-acceptance.md)
