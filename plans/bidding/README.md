# 招投标最终 V1 完整方案

| 项 | 值 |
| --- | --- |
| 状态 | **最终 V1 方案已批准并固化为实施基线；durable dispatch 替换待实施，尚未完成本地验收、实现提交、部署或 runtime accepted** |
| 日期 | 2026-08-26 |
| 业务 | 网络安全产品与服务应标（乙方） |
| 部署 | clean-slate fresh redeploy |
| 正式范围 | 招标拆解、事实、两路匹配、人工选择、人工报价、①～⑥组卷、DOCX/PDF |

本页是招投标实施方案的总入口。稳定领域定义在 [`../../docs/bidding/domain.md`](../../docs/bidding/domain.md)，专题细节只在以下文档定义：

| 专题 | 权威内容 |
| --- | --- |
| [`tender-publication.md`](tender-publication.md) | 招标转换源、SourceSpanV2、KindRouter、条款/事实 publication 与生命周期 |
| [`matching.md`](matching.md) | 两路检索、不可变证据、MatchingReportV1、选择集和匹配发布 |
| [`quote.md`](quote.md) | CNY Decimal、限价口径、QuoteSnapshotV1、finalize/reopen |
| [`submission-export.md`](submission-export.md) | ①～⑥、程序材料、stale、manifest、ObjectRegistry 使用、DOCX/PDF |
| [`durable-dispatch.md`](durable-dispatch.md) | target+intent 原子性、单跳投递、业务 lease 恢复、Oxana transport seam 与删除矩阵 |
| [`implementation-acceptance.md`](implementation-acceptance.md) | 最终 baseline schema、删除矩阵、PR0～PR9（含 PR8A～PR8F）、测试与运行验收 |

已被替代的旧方案和评审只保存在 [`../archive/README.md`](../archive/README.md)，不再留旧路径兼容副本。

---

## 当前实施状态

当前仓库已落位大部分 clean-slate V1 产品主链，但本轮拟议的 transactional durable dispatch 尚未实施；当前工作树仍含将被替换的两跳 live-recovery、commit 后 best-effort enqueue 和 Oxana 私有 processing-list replay。旧方案不再是兼容目标，替换时必须删除而不是双跑。

| 层次 | 当前证据 |
| --- | --- |
| implemented | 部分；产品主链已有实现，durable dispatch 深 module 与旧 recovery 删除尚未实施 |
| locally verified | 部分；已有定向证据不能覆盖新 dispatch 合同，完整 workspace/强制活库/fresh runtime 需重跑 |
| committed | 否；当前工作树存在未提交变更 |
| pushed | 否 |
| deployed | 否；未部署到 fresh 或生产环境 |
| runtime accepted | 否；当前 checkout 没有完整 fresh-runtime 证据包，`phase_1d_runtime_complete=false` |

历史运行记录只能作为历史证据，不能提升当前 checkout 的状态。“已实现”也不等于 locally verified、committed、pushed、deployed 或 runtime accepted；各层必须按 [`implementation-acceptance.md`](implementation-acceptance.md) 分别记录实际证据。

## 1. 方案结论

这是最终 V1，不是对现有 0010/0012 的向后兼容扩展。实现以空库、空对象卷、空 Redis 启动：

- 不保留旧 schema、旧 API、旧 client family、alias、兼容 façade；
- 不双写、不读旧格式、不迁移历史业务数据；
- 旧 migration 和 runtime repair helper 合并/删除，最终只留下能建立完整目标系统的 baseline；
- 允许直接删除已经废弃的历史逻辑、view、test 和存储接口；
- 不以“旧调用方还能跑”为验收条件，以最终领域契约和真实全链为准。

clean-slate 只取消兼容负担，不取消约束、并发控制、幂等、审计、权限、retention 和运行验收。

## 2. 产品边界

### 2.1 包含

1. 项目与招标文件；
2. 转换、拆段、事实建议、条款抽取和人工确认；
3. 产品资料与公司资料两路匹配；
4. supported 候选 1..N 人工选择；
5. CNY 人工报价、限价检查与定稿快照；
6. ①～⑥可编辑 parts、程序材料、DOCX 过程稿和正式 PDF；
7. fresh deploy、浏览器全链、失败恢复和运行验收。

### 2.2 不包含

- Org、多租户、包件、复杂角色系统；
- 评标、成本、工程量清单；
- 多币种、自动定价；
- CA 电子签章、投标平台自动递交；
- 历史数据迁移、旧 binary 共存与灰度。

正式 PDF 仅表示内容冻结且可打印签章，不表示已完成 CA 签章或电子平台递交。

## 3. 领域与平台边界

```text
Knowledge Base
  Workspace / Product / ProductVersion / Document / index / retrieval
             |
             | KnowledgeRetrievalPort
             v
Bidding
  TenderPublication -> ClauseLifecycle -> MatchingPublication
                                          |
                                          v
                    Quote -----------> Submission
             |
             v
Shared Platform
  auth / actor / idempotency / audit / queues / ObjectRegistry / observability
```

### 3.1 知识库端口

跨域接口和 DTO 只由 [`../../docs/knowledge-base/domain.md`](../../docs/knowledge-base/domain.md) 的 `KnowledgeRetrievalPort` 定义。招投标不得直接 join 知识库表；Matching 侧如何立即冻结返回值见 [`matching.md`](matching.md)，不在总方案复制端口字段。

### 3.2 共享平台

鉴权、actor identity、幂等/audit 基础表、运行时队列、维护门、对象注册与物理删除归共享平台。业务模块只使用平台接口；fresh baseline 编排与 `ObjectRegistry`/retention 内部协议见 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md)，Oxana/Redis transport interface 见 [`../platform/queue-runtime.md`](../platform/queue-runtime.md)。

Matching 的 Open/Stage/Commit 是大 artifact 的 adapter 内部协议。application service 只表达“执行并发布 route”，不能让 staging set、claim token 或 batch ordinal 泄漏到其它业务模块。

### 3.3 Durable dispatch

招投标拥有自己的 PostgreSQL durable intent 和 target-local recovery，完整合同只在 [`durable-dispatch.md`](durable-dispatch.md) 定义。业务 target 与 intent 同事务提交，dispatcher 通过平台 `WorkTransport` 单跳投递最终 delivery；API 不在 commit 后 best-effort enqueue，Oxana 不拥有业务 snapshot、generation 或 retry 状态。

该 runtime module 是五个业务深模块共享的内部实施能力，不增加第六个业务领域。正常投递和失败恢复使用同一 intent；不得保留独立 `system:live-recovery:v1` queue hop 或泛化 housekeep 扫描。

## 4. 五个深模块

### 4.1 TenderPublication

把一个已转换的招标 source generation 变成可审计的 current publication。它拥有 target/generation/claim fencing、section/span candidates、fact suggestions、clause candidates 和原子 publish。

输出：冻结来源、current sections、current fact suggestion projection、current draft clauses。

### 4.2 ClauseLifecycle

拥有 clause 状态、`kind -> family` 唯一派生、人工编辑、确认/取消确认、集合 identities、KindRouter contract promotion 和待重新确认 marker。

输出：current confirmed matching set、ServiceClauseSetV1、pricing/payment/delivery/evaluation/procedural sets。

### 4.3 MatchingPublication

冻结由完整 eligible version scope 产生的 route membership，并将有限 hit 集独立冻结；知识库拥有 schedule-time scope attestation，招投标不得直接 join live 知识表。随后构建 `RequirementDecisionV1` 和 `MatchingReportV1` 并原子发布 current report；eligible version 没有 hit 时保留 membership，并产生 `NO_EVIDENCE`。人工选择形成 `RoutePickSetV1`，项目聚合形成 `ProjectPickSetV1`。

### 4.4 Quote

拥有报价 draft/revision、行计算、`ceiling_basis`、eligibility、finalize/reopen 与 immutable `QuoteSnapshotV1`。系统不替人确认价格。

### 4.5 Submission

拥有 company/submission profile、程序材料分类与 resolution、附件 durable preparation、parts、dependency/stale、manifest、render assets 和导出门禁。PDF 附件上传只提交原对象和 durable job，worker 通过 lease/heartbeat/reaper 生成并原子发布冻结页面；preparation 未完成时 Gate 拒绝。Submission 只读其它模块发布的 identity/artifact。

## 5. 关键不变量

### 5.1 内容身份

正式输入都使用 `schema_version + canonical bytes + SHA-256`：

- FactIdentityV1；
- 各 ClauseSetV1；
- MatchingReportV1 与 EvidenceV1；
- RoutePickSetV1 / ProjectPickSetV1；
- QuoteSnapshotV1；
- CompanyProfileV1 / SubmissionProfileV1；
- Procedural classification/decision/attachment sets；
- PartDependencyV1 / SubmissionManifestInputV1。

revision 用于 CAS 和快速失效，digest 用于证明内容；不得相互替代。

### 5.2 原子性

每次领域 mutation 必须在一个事务中完成：

```text
领域行
+ revision/digest
+ current pointer 或 status
+ audit
+ stale/invalidation
+ completed idempotency receipt
```

任何 CAS、scope、hash 或 verifier 失败都零领域写。涉及物理对象时，平台允许先建立受检、可过期的 upload staging reference；领域事务失败必须 abandon staging 并由 retention 回收，禁止留下无 registry/reference 的孤儿 blob。

### 5.3 人工边界

- 自动抽取只产生 draft/suggestion；
- 人确认 clause 和 fact；
- 人从 supported 候选选 1..N；
- 人录入并 finalize 正式报价；
- 人解决程序材料，系统负责验证 gate；
- PDF gate 不得被 Bootstrap/system actor 假造人工确认绕过。

### 5.4 时间与金额

- 业务时区固定 `Asia/Shanghai`；持久化 UTC；
- V1 金额只允许 CNY、scale=2；
- Decimal 以字符串进入 canonical JSON；
- `ceiling_basis=tax_inclusive|tax_exclusive|unspecified`；
- 有效期天数与绝对日期同时存在时显式冲突。

## 6. 已确认决策的实施位置

总方案不复制领域规则：

- KindRouter eligible/manual/连续 promotion 状态机：[`../../docs/bidding/domain.md`](../../docs/bidding/domain.md) 定义业务不变量，[`tender-publication.md`](tender-publication.md) 定义事务与验收；
- unsectioned `R/S` 与普通 unit 共存：[`../../docs/bidding/domain.md`](../../docs/bidding/domain.md) 定义业务不变量，[`matching.md`](matching.md) 和 [`submission-export.md`](submission-export.md) 定义 verifier/part 映射；
- DOCX/PDF 语义与门禁：[`../../docs/bidding/domain.md`](../../docs/bidding/domain.md) 定义业务结果，[`submission-export.md`](submission-export.md) 定义实施协议。

## 7. 最终数据策略

### 7.1 一个 baseline

最终仓库只维护创建完整 V1 的 baseline schema 与校验清单。文档不再以“从 0010/0012 升到 0013～0018”描述生产路径；这些编号只可出现在历史评审中。

baseline 必须一次建立：

- 平台 runtime foundation（由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 唯一定义）；
- project/document/source/publication/clause/fact；
- matching job/artifacts/current projections/picks；
- quote draft/snapshot/current pointer；
- profiles/procedural/parts/manifest/render assets；
- Bid async target identity、durable dispatch intent/immutable offer/state/attempt/transport receipt/settlement/retirement 与 typed target exact relation；
- functions、triggers、views、ACL 和 seed contract artifacts。

### 7.2 权限

招投标 API/worker 只获得受检函数和必要读 view 权限；不能直接改不可变 artifact、current pointer、ObjectRegistry、retention outbox 或 Bid dispatch intent/state。平台角色与 retention 权限由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 定义，dispatch 权限由 [`durable-dispatch.md`](durable-dispatch.md) 定义，fresh-schema acceptance 联合验证 allow/deny 矩阵。

V1 的 project 访问边界是 `owner_user_id`：项目列表只返回当前用户拥有的项目，所有 `/api/v1/bids/{project_id}/...` 路径先校验 owner。当前模型没有 Bid 成员或 API-key-to-Bid scope relation，因此这两类访问必须 fail-closed；若未来需要协作成员，先增加显式 membership/scope artifact 与受检授权合同，不能把“已认证”当作“可访问任意 Bid”。

## 8. 实施与验收状态

| 阶段 | 当前实现状态 | 完成证据 |
| --- | --- | --- |
| PR0 | 已批准并固化 | `git diff --check -- plans`、修改文档相对链接检查及 P0/P1 交叉 review 已通过 |
| PR1 | 原 baseline 主体已落位；dispatch schema/ACL/checksum 尚待替换 | fresh-schema/catalog/seed/ACL 必须在替换后重跑 |
| PR2 | TenderPublication 产品逻辑已落位；conversion/extraction dispatch 尚待替换 | publication/conversion/extraction 强制活库套件待重跑 |
| PR3 | ClauseLifecycle、KindRouter 与 fact decision 已落位 | promotion/concurrency 强制活库套件待最终重跑 |
| PR4 | MatchingPublication 产品逻辑已落位；schedule/fanout/job dispatch 尚待替换 | matching/lease/fanout 强制活库套件待重跑 |
| PR5 | Quote 产品逻辑已落位 | quote/HTTP/Web 最终门禁待重跑 |
| PR6 | Submission 产品逻辑已落位；attachment/render dispatch 尚待替换 | submission/renderer/worker/fresh PDF 待重跑 |
| PR7 | API routes 与模块化 Web 工作台已落位 | HTTP、lint/build、mocked 与 live Playwright 待最终重跑 |
| PR8A | 未完成 | 平台 pure prepare、Oxana atomic offer/probe/retire+tombstone、boot UUID/resurrection、删除 private replay并回归 legacy enqueue；不切换业务 owner |
| PR8B | 未完成 | dormant async target/空 conversion target/intent/offer/state/attempt/receipt/settlement/retirement catalog、dispatch 深 module、policy/ACL；不激活 producer或业务 owner |
| PR8C | 未完成 | conversion/extraction 纵切替换，同步删除该类旧 enqueue/recovery owner |
| PR8D | 未完成 | attachment preparation/render 纵切替换，同步删除该类旧 owner |
| PR8E | 未完成 | matching schedule/job/fanout 纵切替换，同步删除 dirty/orphan recovery |
| PR8F | 未完成 | 删除剩余旧 owner、复证 private replay denylist、baseline checksum、catalog/ACL、queue registry closure 与全量本地/强制活库门禁 |
| PR9 | 未完成 | 干净已 push 候选 SHA 的 clean-slate Compose、全量故障矩阵、真实浏览器/PDF、retention、强制资源清理、证据绑定与受审计 cutover；`phase_1d_runtime_complete=false` |

PR8A 删除 private-key replay并由 Oxana public resurrection 接管 transport recovery；PR8F 只复证该 denylist，不承担首次删除。PR8A/PR8B 只建立平台和 dormant 深 module 能力，不补建旧 target aggregate、不双写也不启用第二个业务 owner。PR8C～PR8E 每个纵切才原子创建该 family 的 base/typed/intent/offer 0/state，并在同一改动启用新 adapter、删除旧 enqueue/recovery 分支；PR8F 证明全局只剩一个 owner。

Redis terminal tombstone 只证明 transport 已结束，`ack_settled` 还必须匹配 PostgreSQL durable settlement；`handler_unsettled|killed|invalid_envelope` 与领域主动 `retired` 不得混同。六张 `bid_async_*` sidecar 只保存 dispatch identity 与真实 domain target 的 exact relation，不复制业务状态、generation、claim或结果。

## 9. 验收口径

“完整实现”需要同时满足：

1. 当前 PRODUCT 和领域文档明确①～⑥；
2. 只有一个空库 baseline，fresh schema/ACL/seed 可重复建立；
3. 招标发布、条款、事实、匹配、报价、组卷都没有旧写入旁路；
4. 所有正式 artifact 可按 canonical bytes/hash 重放；
5. DOCX warning 与 PDF hard gate 行为一致且可测试；
6. Web 能走完创建项目到下载正式 PDF；
7. 旧 API/client/schema/view/test 删除且 `rg` 无残留调用；
8. Compose 在真实空环境启动，健康检查、worker、对象存储及 [`durable-dispatch.md`](durable-dispatch.md) 故障矩阵通过；
9. 实际运行生成的 PDF 与 audit/manifest/fixture identity 可互证；
10. `bid-durable-dispatch` 与 `bid-v1-fresh-runtime` 对同一可追溯候选系列成功，fresh evidence 证明 `git_dirty=false`、`playwright=live-ui`、无 skip、全量 fault case 通过且验收容器/volumes/networks 已清理，受审计 cutover 已绑定 hashes 并翻转 `phase_1d_runtime_complete=true`。

本地测试、commit、部署与真实运行验收分别报告，不相互代替。
