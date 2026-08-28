# 招投标方案导航（Target V2）

> 唯一产品/领域/Web 交互契约：[`../../docs/bidding/authoring.md`](../../docs/bidding/authoring.md)（含 §2.4 编制面）。  
> 唯一后端实施方案：[`tender-to-submission-v2.md`](tender-to-submission-v2.md)。  
> 唯一 Web 编制面实施方案：[`frontend-authoring.md`](frontend-authoring.md)。  
> 下文旧专题中的固定 PartSet、SubmissionGateV1、①～⑥ 和专用 profile/procedural 流程只描述待删除的 V1 实现，不得作为 Target V2 要求。

| 项 | 值 |
| --- | --- |
| 状态 | **Target V2 分阶段实现中；Legacy V1 只保留为删除与回归定位** |
| 日期 | 2026-08-27 |
| 业务 | 网络安全产品与服务应标（乙方） |
| 部署 | clean-slate fresh redeploy |
| 正式范围 | 上传招标文件 → 解析 → 生成大纲 → Word 式画布编制 → 知识库填充 → 导出 DOCX/PDF |

本页是招投标实施方案的总入口。仓库级产品与视觉见 [`../../PRODUCT.md`](../../PRODUCT.md)、[`../../DESIGN.md`](../../DESIGN.md)。V1 代码识别用 [`../../docs/bidding/current-code.md`](../../docs/bidding/current-code.md)，不是当前目标。

## 当前权威计划

| 专题 | 内容 |
| --- | --- |
| [`tender-to-submission-v2.md`](tender-to-submission-v2.md) | V2 schema、API、job、Workspace、OutlineCompiler、Assessment、render、删除矩阵 |
| [`frontend-authoring.md`](frontend-authoring.md) | 黄金三步、独立大纲树、Tiptap 连续画布、Candidate overlay、无业务锁 |

## Legacy V1 删除 / 复用地图

| 专题 | 用途 |
| --- | --- |
| [`../../docs/bidding/current-code.md`](../../docs/bidding/current-code.md) | V1 领域快照，供删除扫描 |
| [`current-code/tender-publication.md`](current-code/tender-publication.md) | 可复用 SourceSpan/转换 seam；条款/family 模型待替换 |
| [`current-code/matching.md`](current-code/matching.md) | 可复用检索冻结；route/part 模型待替换为节点级 EvidenceBundle |
| [`current-code/quote.md`](current-code/quote.md) | 可复用 `QuoteSnapshot`；不是 Web 黄金路径必经步 |
| [`current-code/submission-export.md`](current-code/submission-export.md) | ①～⑥ / Gate 删除定位；V2 导出走 RenderDocumentSnapshotV2 |
| [`current-code/durable-dispatch.md`](current-code/durable-dispatch.md) | 现有业务 target/revision、幂等 publish、Oxana retry |
| [`current-code/implementation-acceptance.md`](current-code/implementation-acceptance.md) | V1 fresh-runtime 历史验收；不得用来判定 V2 完成 |

---

## 当前实施状态

当前仓库仍有大量 V1 主链（固定 PartSet、Gate、Markdown 编辑）。Target V2 的 Workspace / 画布 / Assessment 正在按阶段替换。异步投递以 Oxana 为 transport，业务 target 只承担意图与幂等 publish，边界见平台 [`queue-runtime.md`](../platform/queue-runtime.md)。

| 层次 | 当前证据 |
| --- | --- |
| implemented | 部分；V1 主链仍在，V2 壳与契约已部分落位，Word 式整篇画布未完成 |
| locally verified | 部分；完整 workspace / 强制活库 / fresh runtime / 黄金路径 E2E 需重跑 |
| committed | 部分；当前精确状态以 Git 为准 |
| pushed | 部分；当前精确状态以 Git 为准 |
| deployed | 否 |
| runtime accepted | 否；`phase_1d_runtime_complete=false` |

“已实现”不等于 locally verified、committed、pushed、deployed 或 runtime accepted。V2 完成证据以 [`tender-to-submission-v2.md`](tender-to-submission-v2.md) 与 [`frontend-authoring.md`](frontend-authoring.md) 为准。

## 1. 方案结论

这是 Target V2，不是给 V1 ①～⑥ 做兼容扩展。实现以空库、空对象卷、空 Redis 启动：

- 不保留旧 schema、旧 API、旧 client family、alias、兼容 façade；
- 不双写、不读旧格式、不迁移历史业务数据；
- 最终只留下能建立完整目标系统的 V2 baseline；
- 不以“旧调用方还能跑”为验收条件。

clean-slate 只取消兼容负担，不取消约束、并发控制、幂等、审计、权限、retention 和运行验收。

## 2. 产品边界

### 2.1 包含

1. 同一项目多份招标文件上传与解析（PDF / DOCX / XLSX / 图片）；
2. 动态大纲编译与 Candidate overlay（默认全选、可取消节点）；
3. Word 式独立大纲树 + Tiptap 连续画布；用户随时改树、改字、插表插图；
4. 从知识库填充内容（系统建议证据；匹配不是强制向导步）；
5. Assessment 只提示；导出当前 `WorkspaceRevision` 的 DOCX/PDF；改完再导是新文件；
6. `QuoteSnapshot` 可作为冻结输入，但报价精编、台账、文档设置不是黄金路径必经步；
7. fresh deploy、浏览器黄金路径、失败恢复和运行验收。

### 2.2 不包含

- Org、多租户、包件、复杂角色系统；
- 评标、成本、工程量清单；
- 多币种、自动定价；
- CA 电子签章、投标平台自动递交；
- 历史数据迁移、旧 binary 共存与灰度；
- ①～⑥ 固定 PartSet、SubmissionGateV1、Markdown 编辑真源、业务阻断 Gate。

正式 PDF 仅表示内容冻结且可打印签章，不表示已完成 CA 签章或电子平台递交。有业务提示仍允许导出；只有技术失败才停止 render。

## 3. 领域与平台边界

```text
Knowledge Base
  Workspace / Product / ProductVersion / Document / index / retrieval
             |
             | KnowledgeRetrievalPort
             v
Bidding
  TenderDocumentSet → RequirementSet → SubmissionWorkspace
       Outline / ContentBlock / Candidate / Assessment / Render
       QuoteSnapshot（可选冻结输入）
             |
             v
Shared Platform
  auth / actor / idempotency / audit / queues / ObjectRegistry / observability
```

### 3.1 知识库端口

跨域接口只由 [`../../docs/knowledge-base/domain.md`](../../docs/knowledge-base/domain.md) 的 `KnowledgeRetrievalPort` 定义。招投标不得直接 join 知识库表。填充阶段把命中冻结为 EvidenceBundle，不把匹配向导做成用户主路径。

### 3.2 共享平台

鉴权、actor、幂等/audit、队列、维护门、对象注册归共享平台。fresh baseline 见 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md)，Oxana transport 见 [`../platform/queue-runtime.md`](../platform/queue-runtime.md)。

### 3.3 Durable dispatch

业务 target 只保存意图和结果，合同见 [`current-code/durable-dispatch.md`](current-code/durable-dispatch.md)。Oxana 负责 enqueue / retry / resurrection / dead queue。API 在幂等事务提交后单次 enqueue，成功才返回 `202`，失败返回可重试 `503`。不建立第二套队列状态机。

## 4. 可复用与必须删除

可复用：SourceSpanV2、不可变 artifact、ObjectRegistry、CAS、幂等、`QuoteSnapshot`、知识检索端口、Oxana retry、manifest-only render 思想。必须通过 V2 新 interface 接入。

必须删除：`1、2:*、3、4、5、6:*` RequiredPartSet、`part_key`、SubmissionGateV1、旧 profile/procedural 专用 API、Markdown part 更新/重生成、OutlineFulfillmentBinding 独立 current pointer。

V1 五个深模块（TenderPublication / ClauseLifecycle / MatchingPublication / Quote / Submission）只描述当前代码归属，不是 V2 用户流程。V2 用户流程是文件 → 编制 → 导出。

## 5. 关键不变量

- 正式输入使用 `schema_version + canonical bytes + SHA-256`。
- 领域 mutation 同一事务：行 + revision/digest + current pointer + audit + stale + 幂等 receipt。
- 人工边界：AI 只出 Candidate；人改树和正文；人决定是否忽略提示并导出。
- 业务时区 `Asia/Shanghai`；V1/V2 金额仅 CNY Decimal。
- 编制过程没有业务锁；`project ended` 才停止 mutation。

## 6. 验收口径

“完整实现”需要同时满足：

1. `PRODUCT.md`、`DESIGN.md` 与编制契约描述同一条黄金路径，而不是 ①～⑥ / Gate；
2. Web 能不进入报价/台账/设置页走完上传 → 生成大纲 → 接受候选 → 打字 → 导出；
3. 大纲是独立树，正文是 Tiptap 画布，不是 Markdown `#`；
4. 只有一个空库 V2 baseline；
5. Assessment 不阻断导出；技术失败 fail-closed；
6. 旧 PartSet / Gate / part API 删除且检索无残留调用；
7. Compose 在真实空环境启动，dispatch 故障矩阵与浏览器黄金路径通过。

本地测试、commit、部署与真实运行验收分别报告。
