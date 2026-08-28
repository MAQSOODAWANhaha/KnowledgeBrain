# 招投标领域（现有V1实现快照与Target V2导航）

| 项 | 值 |
| --- | --- |
| 状态 | **Target V2已替代本文的目标契约；下文仅用于识别待删除V1实现** |
| 目标契约 | [`../platform/tender-to-submission-authoring.md`](../platform/tender-to-submission-authoring.md) |
| 实施方案 | [`../../plans/bidding/tender-to-submission-v2.md`](../../plans/bidding/tender-to-submission-v2.md) |
| 部署 | Target V2 clean-slate fresh redeploy |

Target V2的动态大纲、统一Workspace、ContentBlock、Candidate、Assessment和DOCX/PDF规则只由上述两份文档定义。下文保留固定PartSet、SubmissionGateV1等名称，只是当前V1代码/删除矩阵的实现快照，不再构成产品要求，也不得被新代码引用为Target V2合同。

## Legacy V1实现快照（仅供删除与回归定位）

## 1. 产品目标

系统面向一家网络安全产品与服务供应商，完成：

1. 招标文件拆解与项目事实确认；
2. 技术条款到产品证据的匹配；
3. 商务条款到公司资料证据的匹配；
4. 人工选择产品、确认缺件与补充材料；
5. 人工报价、定稿与限价检查；
6. 生成可编辑过程稿和受门禁约束的正式 PDF。

正式 PDF 的含义是“内容已冻结、可打印、可线下签章”。V1 不提供 CA 电子签章、电子投标平台登录、加密上传或自动递交。

## 2. 固定边界

- 一家公司、无 Org、无多租户、无复杂角色系统。
- V1 以 `owner_user_id` 作为 project 访问边界：项目列表只返回当前用户拥有的项目，所有项目路径先校验 owner；当前没有 Bid membership 或 API-key-to-Bid scope relation，无法证明 scope 时必须 fail-closed。
- 负责人仍可作为业务跟踪字段，但不能替代 `owner_user_id` ACL。
- 一个 `BidProject` 表示一份招标事项，多份招标文件直接挂项目；V1 无包件层。
- 招标文件不进入知识库 `Document` 或产品索引。
- 只保留产品证据、公司证据两条知识检索路，不引入第三个通用 Agent。
- 价格由人录入并定稿；系统可提示，不替人确认正式价格。
- Word/DOCX 是过程稿；正式 PDF 必须通过 `SubmissionGateV1`。
- V1 金额只支持 CNY；存储和计算使用定点 Decimal，禁止 JSON float。
- 所有业务日期按 `Asia/Shanghai` 解释，持久化时转 UTC；API 必须携带明确 offset，禁止依赖服务器本地时区。

## 3. 明确不做

- 土建施工、工程量清单、成本引擎、评标引擎；
- 包件、复杂角色与职责分离门闩；
- 多币种和汇率；
- CA 电子签章、电子标书加密、平台自动递交；
- 历史生产数据迁移、旧 binary 共存、灰度、down migration；
- 旧 schema、旧 API、alias、兼容 façade、双写或旧格式读取。

## 4. 领域边界

### 4.1 五个招投标深模块

| 模块 | 拥有的状态与规则 | 对外结果 |
| --- | --- | --- |
| `TenderPublication` | 项目、招标文件、转换代次、抽取候选、事实建议、原子发布 | current project facts、current clauses、不可变来源 |
| `ClauseLifecycle` | kind/family、确认、人工编辑、KindRouter promotion、clause-set identity | 可参与后续流程的 confirmed clause sets |
| `MatchingPublication` | 冻结路由范围、证据、候选、决策、报告与人工 PickSet | current published matching decisions 与选择集 |
| `Quote` | 报价草稿、行计算、限价口径、finalize/reopen、快照 | current eligible `QuoteSnapshotV1` |
| `Submission` | profile、程序材料、parts、stale、manifest、DOCX/PDF | 过程稿或正式交付 artifact |

模块之间只传版本化 identity 或不可变 artifact，不跨模块读取对方的临时表并重建另一份真相。

### 4.2 外部领域

| 所属领域 | 招投标如何使用 |
| --- | --- |
| 知识库 | 只调用 `KnowledgeRetrievalPort`，随后在匹配报告中冻结采用的证据 |
| 共享平台 | 使用 authenticated actor、幂等/audit 基础、队列、ObjectRegistry、运行时与可观测性 |

`ObjectRegistry` 属共享平台，不是 Submission 私有对象表。Open/Stage/Commit 只允许作为 `MatchingPublication` adapter 内部的大结果提交协议，不得扩散为业务 service interface。

## 5. 核心聚合与术语

### 5.1 BidProject

`BidProject` 是招投标根聚合，至少拥有：

- 标题、负责人、招标结束时间、`open|ended`；
- 已接受或人工设置的预算、最高限价、开标时间、投标有效期；
- 各事实与条款集合的 revision + canonical digest；
- current quote、current parts 和 current submission manifest 的指针；
- project mutation watermark，用于匹配调度失效。

`ended` 后禁止新的抽取发布、匹配发布、报价定稿和正式导出；历史不可变 artifact 仍可审计读取。

### 5.2 BidDocument

`BidDocument` 是招标侧文件：原件、转换结果、conversion generation 和状态归招投标所有。它可以复用共享的 convert 能力，但不复用知识库 `Document` 状态机，也不进入知识索引。

### 5.3 SourceSpanV2

任何 extracted clause 或 fact suggestion 必须定位到被冻结的转换源和 section 边界，并用 UTF-8 byte 半开区间引用连续原文。人工编辑后，历史 origin 永久保留，但 current provenance 变为 `manual_after_edit`，不得继续声称 current text 是原文逐字引用。

### 5.4 Clause kind 与 family

`kind` 表达招标内容语义；`family` 只表示是否进入两路匹配，由服务端唯一派生。

| kind | family | 主要消费者 |
| --- | --- | --- |
| `technical` | `technical` | 产品证据匹配、技术响应 |
| `qualification` | `commercial` | 公司资质证据匹配 |
| `service` | `commercial` | 公司能力证据、实施计划 |
| `pricing` | `NULL` | 报价结构 |
| `schedule_delivery` | `NULL` | 实施/交付计划 |
| `schedule_payment` | `NULL` | 投标函与商务条款 |
| `evaluation` | `NULL` | 评标备忘 |
| `procedural` | `NULL` | 程序材料与正式导出门禁 |

客户端创建或修改条款时只提交 `kind`，不得提交 `family`。匹配成员恒为：

```text
status = confirmed AND family IS NOT NULL
```

禁止 `else -> commercial`。

### 5.5 ProjectPickSetV1 与 RoutePickSetV1

- `RoutePickSetV1` 是用户对某一 current technical route/report 的 1..N 个产品选择，绑定 report、requirement、candidate 与 product version identity。
- `ProjectPickSetV1` 是所有 current route picks 的项目级规范并集，供封面、总体方案、实施计划和 manifest 使用。
- 两者不可用同一个含糊的 `PickSet` 名称替代，也不得从 live UI 状态临时推导正式内容。

## 6. 端到端业务流

```text
创建 BidProject
  -> 上传 BidDocument
  -> convert 并冻结 SourceArtifact
  -> 抽取 span / fact / clause candidates
  -> 原子 publication
  -> 人工接受事实、确认或编辑 clause
  -> confirmed family 路由触发两路 matching
  -> 发布不可变 MatchingReportV1
  -> 人工建立 RoutePickSetV1，聚合 ProjectPickSetV1
  -> 人工编辑并 finalize QuoteSnapshotV1
  -> 生成/修订 ①～⑥ parts
  -> SubmissionGateV1
  -> DOCX 过程稿或 PDF 正式定稿
```

任何阶段失败都不能发布半份 current state。重试只能生成新 attempt/artifact 或幂等返回首次 receipt。

## 7. 条款生命周期

- 自动抽取只产生 `draft`；只有 durable user/API actor 可以确认。
- `confirmed` 才能进入 matching 或非 matching clause set。
- 人工修改 extracted text 后 provenance 变为 `manual_after_edit`。
- 删除、取消确认、跨 kind 修改和集合内语义修改必须在同一 project transaction 更新相关 revision/digest、matching watermark 与 stale parts。
- `service` 同时属于 commercial matching 和 `ServiceClauseSetV1`，因此一次语义变化需要同时失效两类消费者。

### 7.1 KindRouter promotion

KindRouter contract promotion 只自动重算满足全部条件的 clause：未被人工编辑、`provenance=extracted`、仍有 current frozen `SourceSpanV2` 且 scope 校验通过。

- confirmed `manual` / `manual_after_edit` 不自动改变 kind/family；只能由 durable actor 显式 PATCH。
- eligible confirmed extracted clause 若新 kind 不同，必须先取消确认、退出旧集合，再写新 kind 并保持 draft，等待人工重新确认。
- 连续 promotion 必须在每一代刷新所有待确认 marker；manual/manual_after_edit 只刷新 marker，不改人工 kind/family。
- Router 输入、marker 和最终 current pointer 必须绑定同一个 target version/generation；失败全事务回滚。
- 重新确认使用 current generation + clause revision CAS，清 marker 并且只进入最终新集合一次。

## 8. 事实

系统可从招标原文提出 `budget_amount`、`ceiling_price`、`expires_at`、`bid_open_at`、`bid_valid_until`、`bid_valid_days` 建议。建议与 clause 共用一次 fenced publication。

- pending suggestion 不是项目事实；只有 accepted 或人工 set/clear 后才进入 `BidProject`。
- 接受一条建议会 supersede 同字段其它 current pending suggestions，但不删除历史 decision ledger。
- `FactIdentityV1` 使用完整 canonical content + revision 计算 SHA-256，禁止只用 revision 充当内容身份。
- `bid_valid_days` 与 `bid_valid_until` 同时存在时显示冲突并阻断正式 PDF，不自动择一。
- 事实 mutation、audit、幂等 receipt、相关 quote eligibility 与 part stale 必须同事务。

## 9. 匹配与选择

- technical route 按确认后的技术单元执行，另允许唯一 unsectioned technical route；commercial route 项目级执行。
- route membership 来自知识库端口返回的完整 eligible version scope；有限 hit 集不能截断 membership，eligible 但无 hit 仍必须保留并生成 `NO_EVIDENCE`。
- schedule 只消费 knowledge-owned scope attestation，不允许招投标 verifier 直接 join live Workspace/ProductVersion/Document/chunk 表。
- 每个 eligible requirement 恰有一个 `RequirementDecisionV1`。
- support 聚合优先级固定为 `supported > unresolved > insufficient > contradicted > no-evidence`。
- 有 supported 候选时系统按冻结 comparator 给出 recommended，但保留全部 supported 候选；用户可选择 1..N 个，不宣称唯一最佳。
- `MatchingReportV1`、证据片段、文件显示名、候选、decision 和 route membership 全部不可变并可按 hash 重放。
- current projection 只指向完整发布且 generation/watermark 仍 current 的报告。

unsectioned 报告约束：先确定唯一 current unsectioned technical report `R`，再定义 `S = ProjectPickSetV1.items WHERE source_report_artifact_id = R.id`，并验证它逐项等于 `R` 对应的 current `RoutePickSetV1`。只要求 `S` 使用 nil unit identity 并映射 `2:unsectioned`；同一个 `ProjectPickSetV1` 可以同时包含普通 unit items，不能把整个项目选择集强制为 nil。

## 10. 报价

- 报价草稿可编辑，正式消费者只读 immutable `QuoteSnapshotV1`。
- 行金额、税额和总计按固定 Decimal 规则计算，先逐行舍入再求和；DB 与 Rust 双重验证。
- `ceiling_basis` 必须显式为 `tax_inclusive|tax_exclusive|unspecified`。
- 非空最高限价的 `ceiling_basis=unspecified` 只能进入人工 review，禁止 quote finalize；用户必须先把项目事实改为明确含税或未税，系统不能静默假定。
- finalize 绑定 current pricing set、ceiling identity 和全部正式标题/notes；变更后 snapshot 变 ineligible，但保留历史。
- reopen 从旧 snapshot 复制到新 draft，不修改旧 snapshot，不复活旧 revision。

## 11. ①～⑥组卷

`RequiredPartSet` 必须精确包含：

| part | 内容 | 主要冻结依赖 |
| --- | --- | --- |
| `1` | 项目概况与招标事实 | FactIdentityV1、项目 profile |
| `2:{unit}` | 各技术单元响应 | RoutePickSetV1、technical report/evidence |
| `2:unsectioned` | 未归段技术响应 | 唯一报告 `R` 及精确子集 `S` |
| `3` | 总体产品/解决方案 | ProjectPickSetV1、BidShotSetV1 |
| `4` | 公司资质与证明 | commercial matching decisions |
| `5` | 偏离与缺件 | current unresolved/reject/missing identity |
| `6:letter` | 投标函 | facts、company/submission profile、quote identity |
| `6:authorization` | 授权材料 | profile、程序分类/决定、附件集合 |
| `6:quote` | 报价表 | current eligible QuoteSnapshotV1 |
| `6:implementation_plan` | 实施计划 | ServiceClauseSetV1、ProjectPickSetV1、delivery set |
| `6:procedural` | 程序材料检查 | procedural segment/classification/decision/attachment sets |

每个 part 由唯一 dependency identity 生成。任何依赖变化都必须使受影响 part stale；manifest 创建和导出结束前各做一次 current identity CAS 检查。

## 12. SubmissionGateV1

DOCX 可以带 warning 和明确 placeholder，便于过程协作；正式 PDF 必须同时满足：

- 固定公司/投标 profile 字段完整，签字和盖章确认有效；
- 所有 current procedural segment 有 current classification 和 resolution；
- 所需附件已完成必要的 durable preparation、已验证、已确认且种类匹配，或 durable actor 明确标记 not applicable 并给出原因；
- 不存在待 KindRouter 重新确认条款；
- 存在 current eligible finalized quote；
- 不存在投标有效期冲突；
- RequiredPartSet 完整，所有 part 与 manifest dependency current；
- renderer 只读取 manifest 冻结资产，不读取 live project/知识库数据。

## 13. 正确性与审计

- user/system mutation 使用 canonical actor identity、operation、idempotency key、payload hash 和首次 response receipt。
- 同 key 同 payload 重放首次结果；同 key 不同 payload 稳定冲突。
- 领域写、revision/digest、audit、stale、current pointer 和幂等 receipt 在同一事务提交。
- canonical JSON 固定 schema version、键、排序、NULL、UUID、Decimal、时间和 digest 格式；禁止额外键。
- 不可变 artifact 只能追加，current pointer 原子切换；历史不可覆盖。
- 对象生命周期与引用由共享平台 `ObjectRegistry` 真源负责，业务代码不得直接物理删除 blob；内部协议见 [`../../plans/platform/runtime-foundation.md`](../../plans/platform/runtime-foundation.md)。

## 14. 完成语义

“目标契约已确认”只表示本文决策已批准；不表示已经实现。只有实现计划定义的 schema/API/Web、删除矩阵、测试、fresh deploy、浏览器全链和真实运行验收全部完成，才可以称“招投标最终 V1 已完成”。
