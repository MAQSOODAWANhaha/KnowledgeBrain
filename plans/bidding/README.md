# 招投标最终 V1 完整方案

| 项 | 值 |
| --- | --- |
| 状态 | **最终 V1 主链已实现；本地、DB、Web 与隔离 fresh Compose runtime 已验收** |
| 日期 | 2026-08-24 |
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
| [`implementation-acceptance.md`](implementation-acceptance.md) | 最终 baseline schema、删除矩阵、PR0～PR8、测试与运行验收 |

已被替代的旧方案和评审只保存在 [`../archive/README.md`](../archive/README.md)，不再留旧路径兼容副本。

---

## 当前实施状态

当前仓库已切换到 clean-slate V1：最终 baseline、Rust domain/storage/API/worker、Web 工作台和 manifest-only DOCX/PDF renderer 均已有实现路径，旧方案不再是兼容目标。

“已实现”只描述代码和合同已经落位，不等于全部验收完成。本地、DB、HTTP、浏览器、部署和 runtime acceptance 必须按 [`implementation-acceptance.md`](implementation-acceptance.md) 分别记录实际证据。

2026-08-24 已在空 PostgreSQL/Redis/对象卷上通过强制 Compose 全链：真实 PDF/DOCX 转换、抽取、两路匹配、人工报价、Gate 修复、DOCX/PDF、manifest replay、claim/reaper、staging retention 与结束态拒绝均产生运行证据，临时容器、卷和镜像随后清理。该结论只表示当前 checkout 的隔离 fresh runtime accepted；生产未部署，平台级 `runtime-completion.toml` 仍因 provider evaluation、签名 image lock 和 topology 发布闭包保持 `false`。

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

鉴权、actor identity、幂等/audit 基础表、运行时队列、维护门、对象注册与物理删除归共享平台。业务模块只使用平台接口；fresh baseline 编排与 `ObjectRegistry`/retention 内部协议见 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md)。

Matching 的 Open/Stage/Commit 是大 artifact 的 adapter 内部协议。application service 只表达“执行并发布 route”，不能让 staging set、claim token 或 batch ordinal 泄漏到其它业务模块。

## 4. 五个深模块

### 4.1 TenderPublication

把一个已转换的招标 source generation 变成可审计的 current publication。它拥有 target/generation/claim fencing、section/span candidates、fact suggestions、clause candidates 和原子 publish。

输出：冻结来源、current sections、current fact suggestion projection、current draft clauses。

### 4.2 ClauseLifecycle

拥有 clause 状态、`kind -> family` 唯一派生、人工编辑、确认/取消确认、集合 identities、KindRouter contract promotion 和待重新确认 marker。

输出：current confirmed matching set、ServiceClauseSetV1、pricing/payment/delivery/evaluation/procedural sets。

### 4.3 MatchingPublication

冻结 route membership，调用知识库端口，验证证据，构建 `RequirementDecisionV1` 和 `MatchingReportV1`，再原子发布 current report。人工选择形成 `RoutePickSetV1`，项目聚合形成 `ProjectPickSetV1`。

### 4.4 Quote

拥有报价 draft/revision、行计算、`ceiling_basis`、eligibility、finalize/reopen 与 immutable `QuoteSnapshotV1`。系统不替人确认价格。

### 4.5 Submission

拥有 company/submission profile、程序材料分类与 resolution、parts、dependency/stale、manifest、render assets 和导出门禁。只读其它模块发布的 identity/artifact。

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
- functions、triggers、views、ACL 和 seed contract artifacts。

### 7.2 权限

招投标 API/worker 只获得受检函数和必要读 view 权限；不能直接改不可变 artifact、current pointer、ObjectRegistry 或 outbox。平台角色与 retention 权限由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 定义，fresh-schema acceptance 联合验证 allow/deny 矩阵。

V1 的 project 访问边界是 `owner_user_id`：项目列表只返回当前用户拥有的项目，所有 `/api/v1/bids/{project_id}/...` 路径先校验 owner。当前模型没有 Bid 成员或 API-key-to-Bid scope relation，因此这两类访问必须 fail-closed；若未来需要协作成员，先增加显式 membership/scope artifact 与受检授权合同，不能把“已认证”当作“可访问任意 Bid”。

## 8. 实施与验收状态

| 阶段 | 当前实现状态 | 完成证据 |
| --- | --- | --- |
| PR0 | 已实施：文档、PRODUCT、领域词汇和最终 contract 已固化 | 文档链接与重复权威定义检查 |
| PR1 | 已实施：最终 baseline 与共享平台边界已有单一路径 | fresh DB、catalog、ACL、seed 已通过 |
| PR2 | 已实施：TenderPublication、SourceSpanV2 与 bounded parser 已落位 | publication 并发与失败原子性 DB fixture 已通过 |
| PR3 | 已实施：ClauseLifecycle、KindRouter 与 fact decision 已落位 | promotion generation 2/3、连续 marker 与人工边界 DB fixture 已通过 |
| PR4 | 已实施：MatchingPublication 与两路检索端口已落位 | report/evidence/staging/current 集成验收已通过 |
| PR5 | 已实施：Decimal、ceiling、finalize/reopen 与 QuoteSnapshotV1 已落位 | Rust/SQL exact bytes、DB 并发与 Web 验收已通过 |
| PR6 | 已实施：parts、冻结程序附件、manifest、durable render job、报价 table/grid 与 renderer 已落位 | DB/对象生命周期、Gate 矩阵与 fresh runtime PDF 已通过 |
| PR7 | 已实施：最终 API routes 与模块化 Web 工作台已落位 | HTTP contract、lint/build 与 mocked 浏览器验收已通过 |
| PR8 | 隔离验收已完成 | 空环境启动、全链 PDF、日志、失败恢复与资源清理已通过；生产发布闭包不在本方案内 |

每个实现切片都必须同步删除被替代的旧逻辑；验收时继续用删除矩阵检查，不把兼容层清理留给部署阶段。

## 9. 验收口径

“完整实现”需要同时满足：

1. 当前 PRODUCT 和领域文档明确①～⑥；
2. 只有一个空库 baseline，fresh schema/ACL/seed 可重复建立；
3. 招标发布、条款、事实、匹配、报价、组卷都没有旧写入旁路；
4. 所有正式 artifact 可按 canonical bytes/hash 重放；
5. DOCX warning 与 PDF hard gate 行为一致且可测试；
6. Web 能走完创建项目到下载正式 PDF；
7. 旧 API/client/schema/view/test 删除且 `rg` 无残留调用；
8. Compose 在真实空环境启动，健康检查、worker、对象存储、失败恢复通过；
9. 实际运行生成的 PDF 与 audit/manifest/fixture identity 可互证。

本地测试、commit、部署与真实运行验收分别报告，不相互代替。
