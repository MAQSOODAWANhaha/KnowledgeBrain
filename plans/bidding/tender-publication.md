# 招标发布与条款生命周期

本文定义 `TenderPublication` 与 `ClauseLifecycle` 的最终 V1。所有表名均表示最终 baseline，不表示增量 migration 阶段。

> 实施状态（2026-08-26）：Rust bounded tender parser、SourceSpanV2、KindRouter 和 publication/lifecycle 产品路径已落位；conversion/extraction durable dispatch 替换尚未实施，完整门禁与 fresh runtime 需重跑，未部署。

## 1. 模块边界

### 1.1 TenderPublication 拥有

- `BidProject` 与 `BidDocument` 的招标侧生命周期；
- converted source/section 不可变 artifact；
- extraction target、generation、claim/attempt；
- document conversion/extraction target 的 dispatch stage 与 target-local repair adapter；
- span disposition、clause candidate、fact suggestion candidate；
- section publication、current projection 与 receipt；
- fact suggestion decision ledger 和项目事实 mutation。

### 1.2 ClauseLifecycle 拥有

- clause provenance/status/kind/family；
- manual create/PATCH/confirm/unconfirm/delete；
- matching mutation watermark；
- Service/pricing/payment/delivery/evaluation/procedural clause-set identity；
- KindRouter contract artifact/current pointer/promotion；
- 待重新确认 marker 与相关 audit/stale。

两个模块可共享一个 project transaction seam，但不得各写一份 clause 状态机。

## 2. 招标源

### 2.1 BidDocument

```text
BidDocument
  id, project_id
  file_name, media_type, byte_length
  original_object_ref, original_sha256
  conversion_generation
  current_conversion_target_id
  current_converted_source_artifact_id
  created_at
```

- 招标文件不写入知识库 `documents`，不进入产品索引。
- 可复用同一个 convert adapter，但状态、重试、publication 和对象 owner 均归招投标。
- current conversion target 的 `completed` 表示原件已转换且所需多模态写回完成，可以冻结 source artifact；不是条款抽取成功。
- 重试产生更高 `conversion_generation`、新的 stable conversion target 和新 source artifact，不覆盖历史。

### 2.2 DocumentConversionTarget

每个 generation 使用新的 stable async target：

```text
id, project_id, document_id, conversion_generation
conversion_snapshot_id, feature_snapshot_id
status = pending|running|completed|failed|superseded|cancelled
active_attempt, max_attempts
created_at, completed_at, terminal_code
```

`id` 同时是 `bid_async_targets` 的 PK/FK identity；`BidDocument.current_conversion_target_id` 只能指向同 document/current generation 的非 superseded target。claim/attempt/heartbeat 使用 target ID，不再用可复用的 document ID 表示多个 generation。

### 2.3 ConvertedSourceArtifactV1

artifact 至少冻结：

```text
id, project_id, document_id, conversion_target_id, conversion_generation
original_object_ref, original_sha256
canonical_markdown_utf8, markdown_sha256, byte_length
converter_contract_version, image_asset_set_sha256
created_at
```

source artifact immutable，复合唯一键覆盖 `(project_id,document_id,conversion_target_id,conversion_generation)`。抽取 target 必须引用确切 artifact，不允许只存 document current pointer。

### 2.4 SectionArtifactV1

Section 记录 `section_key`、heading path、在 source Markdown 中的 parent UTF-8 byte 半开区间和 section digest。section key 由大纲路径与稳定序号生成，不包含正文 hash；正文变化由 artifact generation/digest 区分。

OutlineParser 必须识别 ATX、章/节、数字层级和中文编号。普通要求句不能误判成标题。表格行保留可回源的原始 Markdown。

### 2.5 Conversion 与 extraction dispatch

文档上传或 conversion retry 必须在创建/推进 `conversion_generation` 的同一事务创建 base async target、`DocumentConversionTarget` 和 `document_conversion` dispatch intent；API 不在 commit 后 enqueue。conversion worker 成功时，在 fenced settlement 事务中同时冻结 converted source、创建 extraction base/typed target、stage extraction dispatch intent，并终结 conversion target。

禁止先把 document 标记为 completed，再通过第二次 DB 调用或 Redis enqueue 创建 extraction target。Redis unavailable、duplicate、worker crash、alive-but-stuck lease 和旧 generation delivery 的行为只由 [`durable-dispatch.md`](durable-dispatch.md) 定义；本模块只实现 target-local begin/heartbeat/publish/repair。

## 3. SourceSpanV2 与抽取契约

### 3.1 SourceSpanV2

每个可引用 routed segment 固定包含并验证：

```text
schema_version
source_artifact_id
section_artifact_id
project_id
document_id
conversion_generation
section_key
parent_start_offset
parent_end_offset
start_offset
end_offset
offset_unit = utf8_byte
quote
quote_sha256
heading_path
```

要求：

- offset 为 UTF-8 byte 半开区间，全部落在 section parent 边界内；
- `quote` 逐字等于 source bytes 的切片，digest 匹配；
- project/document/generation/section identity 由复合 FK 与 verifier 互证；
- 不允许省略键、额外键或以 live Markdown 回源。

### 3.2 有界 Agent 输出

`RequirementSpanAgentV1` 与 `FactSuggestionAgentV1` 只输出 proposal：

- Requirement Agent：segment bounds、proposal text、must signal、bounded reason；
- Fact Agent：segment bounds、field、typed value、raw quote、confidence；
- 两者都不得输出 `kind`、`family`、`technical` 或 `commercial`；
- `KindRouterV1` 是 clause kind 唯一自动权威，family 由服务端根据 kind 唯一派生；
- 额外键、越界、重叠冲突、非连续 quote、数量/字节超限全部拒绝或形成 bounded unresolved disposition。

### 3.3 segment 与 disposition

每个 routed segment 恰有一条 disposition：

| 结果 | disposition |
| --- | --- |
| 产生 clause | `clause` |
| 只产生 fact | `non_requirement/FACT_ONLY` |
| 确定不是要求 | `non_requirement/DETERMINISTIC_NON_REQUIREMENT` |
| 无法有界分类 | `unresolved/AMBIGUOUS` |

同一 segment 可以有 0..N 个 fact suggestions 和最多 1 个 clause。fact 与 clause 可以共存；skip 标题不是 publication 门闩。

## 4. KindRouterV1

### 4.1 kind/family

```text
technical             -> technical
qualification         -> commercial
service               -> commercial
pricing               -> NULL
schedule_delivery     -> NULL
schedule_payment      -> NULL
evaluation            -> NULL
procedural            -> NULL
```

DB CHECK 必须显式拒绝 technical/qualification/service + NULL、错误 family 和 non-matching kind + 非 NULL family。客户端永远不写 family。

### 4.2 路由优先级

能可靠拆句时逐 segment 路由；不可拆时使用下表固定优先级和 veto：

| 优先级 | kind | guard | veto/处理 |
| --- | --- | --- | --- |
| 1 | `technical` | 设备/系统/接口/协议 + 性能/能力/参数主谓结构 | delivery 仅作条件从句时仍 technical |
| 2 | `qualification` | 许可证、ISO、等保资质、软著、业绩/合同佐证及提交义务 | 证书/佐证优先于泛化“提交/盖章” |
| 3 | `procedural` | 保证金、递交、密封、投标函/授权材料格式、签章样式 | 资格佐证 veto；不得仅凭“提交”命中 |
| 4 | `schedule_payment` | 付款/结算动作 + 主体、比例/金额、节点或账期 | 支付接口/网关/密码/API 等技术语境 veto |
| 5 | `schedule_delivery` | 到货、交货、供货、工期、实施周期或地点 | 仅作付款节点/技术条件时 veto |
| 6 | `pricing` | 分项报价结构、计价口径或必须单列价格项 | 裸金额不命中；与 evaluation 不可拆时选 pricing 并 review |
| 7 | `evaluation` | 评分项、权重或得分计算 | 可拆时独立 segment |
| 8 | `service` | 质保、驻场、培训、应急、7x24、SLA | 系统响应时间属于 technical |
| 9 | `technical` | 其它有界技术要求 | — |

固定 golden 至少覆盖：支付接口是 technical；验收后付款是 payment；到货期是 delivery；合同复印件加盖公章是 qualification；投标函盖章是 procedural；驻场 7x24 是 service；不可拆的报价+评分落 pricing 并记录 `PRICING_EVALUATION_CONFLICT`。

## 5. 单一 publication

### 5.1 target 与 candidate 图

每个 extraction target 冻结：

```text
project_id, document_id
source_artifact_id, conversion_generation
extraction_generation
router_contract_version
policy/prompt/schema versions
```

candidate 图包含 section、span、disposition、clause 和 fact suggestion。所有 full/document/section retry 都走同一个 `ExtractionPublicationStore`；禁止 production 直写 current domain rows。

### 5.2 publish_section

单事务依次验证：

1. project open；
2. target/project/document/source identity；
3. claim token、attempt、heartbeat/lease；
4. expected conversion/extraction generation；
5. section candidate terminal 且每个 segment 恰一 disposition；
6. clause/fact candidate bounds、cardinality、schema；
7. current section publication CAS 仍属于本 target。

验证后原子提交：

- current section；
- draft clauses；
- current pending fact suggestions；
- 同 section 旧 pending suggestions superseded；
- publication state/current pointer；
- audit 与首次 receipt。

任何失败都不得产生部分可见行。

### 5.3 publisher 锁序

```text
project
-> document extraction head
-> section publication state（section_key 排序）
-> suggestion decisions（candidate_id 排序）
-> clauses（clause UUID 排序）
```

fact accept/reject 和 clause mutation 在交叠资源上使用相同顺序。

## 6. FactSuggestionV1

### 6.1 候选

支持字段：

```text
budget_amount
ceiling_price
expires_at
bid_open_at
bid_valid_until
bid_valid_days
```

typed value 恰好匹配 field：金额为 `numeric(20,2)+CNY`，时间为 timestamptz，天数为有界 integer。候选绑定 target/section/span/source generation，published 后不可修改或删除。

### 6.2 current 与历史

- `current pending view` 只显示仍属于 current section publication 的 pending candidate；
- durable decision ledger 保存 pending/accepted/rejected/superseded 历史；
- 来源后来被替换不回滚已接受项目事实；只 supersede 旧 pending；
- 同字段多个 current 建议同时展示，不自动择优。

### 6.3 FactIdentityV1

canonical payload 固定包括：

```text
schema_version, project_id, revision,
budget_amount, budget_currency,
ceiling_price, ceiling_currency,
expires_at, bid_open_at, bid_valid_until, bid_valid_days
```

project create 就按真实初值计算 revision=0 digest；若请求含 `expires_at`，digest 必须包含它。每次成功 mutation 在同一事务 `fact_revision += 1` 并重算完整 SHA-256。

### 6.4 accept/reject/set/clear

- accept：要求 current pending、expected fact revision、幂等 key；覆盖不同现值要求显式 override reason。
- reject：要求 current pending 和 bounded reason，不改 fact revision。
- set/clear：人工修改同样使用 expected revision、typed validation、audit 和幂等。
- accept 后同字段其它 current pending 在同事务 superseded。
- ceiling 实际变化时 bump 独立 ceiling revision/identity，并失效 active quote eligibility。
- `bid_valid_days` 与 `bid_valid_until` 并存只记录冲突，不自动换算。

正式 parts 只读已接受/人工设置的 project facts，绝不读 pending suggestions。

## 7. ClauseLifecycle

### 7.1 clause 状态

```text
provenance = extracted|manual|manual_after_edit
status = draft|confirmed|rejected|superseded
kind
family (server-derived)
must
current_source_span_v2 NULLable
extracted_origin_source_span_v2 NULLable immutable
revision
confirmation_required_reason NULLable
confirmation_required_router_generation NULLable
```

- extracted draft 初始保留 current/origin span；
- 人工编辑 text 后改 `manual_after_edit`，current span 清空，origin 永久保留；
- manual 从来没有 source span；
- rejected/superseded 不进入 current sets；
- confirmed membership 变化只经唯一 lifecycle seam。

### 7.2 clause-set identities

项目至少保存：

```text
matching_mutation_watermark
service_revision + service_set_sha256
pricing_revision + pricing_set_sha256
schedule_payment_revision + schedule_payment_set_sha256
schedule_delivery_revision + schedule_delivery_set_sha256
evaluation_revision + evaluation_set_sha256
procedural_revision + procedural_set_sha256
```

每个集合的成员是 `status=confirmed AND kind=<kind>`，按 clause UUID bytes 排序构建 canonical set。空集合使用各自固定 domain-tagged V1 hash，不用 NULL/零字节/其它集合 hash。

进入、离开或集合内 text/must/kind 语义变化才 bump 对应 revision/digest。`service` 同时 bump matching watermark 和 service identity。kind 跨两个 non-matching sets 时 old/new 两边都 bump。

### 7.3 mutation transaction

manual create、publisher materialization、confirm/unconfirm、PATCH/delete 统一：

1. 锁 project -> clause；
2. 计算 old/new matching 与所有 clause-set membership；
3. 写 clause；
4. 更新适用 revision/digest/watermark；
5. 失效 matching/quote/submission consumers；
6. 写 audit 与 idempotent receipt。

draft 内变化且 old/new 都不是某集合成员时不 bump 该集合。

## 8. KindRouter contract promotion

### 8.1 artifact 与 current

```text
KindRouterContractArtifact(version, canonical_payload, content_sha256)
KindRouterCurrent(singleton, version, promotion_generation)
```

promotion 只在 maintenance gate 下执行。调用者提交 expected current version/generation；事务冻结唯一：

```text
target_router_version
target_promotion_generation = current.promotion_generation + 1
```

所有 Router 输入、marker 和最终 current pointer 必须逐值使用这两个 target。

### 8.2 eligible predicate

自动重算只允许同时满足：

- `provenance=extracted`；
- 从未人工编辑；
- current frozen SourceSpanV2 存在；
- origin/current/scope verifier 通过。

confirmed manual/manual_after_edit 完全跳过自动 kind 重算。

### 8.3 confirmed extracted

- 新 kind 不变：不因 contract version 变化取消确认。
- 新 kind 变化：先按 OLD confirmed membership 执行 `confirmed -> draft`、watermark/set/stale；再写 target kind/family，保持 draft，不进入 NEW membership。
- 写 `KIND_ROUTER_PROMOTION_RECONFIRM` 与 target generation marker。
- 禁止 confirmed row 原地跨 kind。

### 8.4 连续 promotion

每代都处理所有带 marker 的 draft：

- 仍 eligible extracted：以新 target Router 重算 kind/family；
- 已是 manual/manual_after_edit 或 current span 失效：保留人工 kind/family；
- 两支都刷新 marker generation，clause revision 恰好 +1，写含 before/after、target version/generation 和 `router_recomputed` 的 system audit；
- 不重复退出 OLD 或进入 NEW set。

这样 generation 2 降 draft、无人确认又发生 generation 3 时，不会被旧 generation marker 卡死。

### 8.5 commit 与重新确认

锁序固定：

```text
maintenance gate
-> KindRouterCurrent
-> open projects（UUID）
-> clauses（UUID）
```

全部 clause/set/audit 成功后，才以最初 expected CAS 原子切 current pointer/generation；失败全回滚。并发 promotion 后取得锁的一方 expected 不匹配就稳定失败并按新 current 重试。

maintenance 期间拒绝 confirm。恢复后 confirm 携带 expected clause revision，验证 marker generation 等于 current promotion generation，清 marker，并按最终 kind 恰好一次进入 NEW membership。manual PATCH 不能清 marker或绕过 confirm。

## 9. API 边界

Application API contract：

```text
create_project
upload_bid_document / retry_conversion
schedule_extraction / retry_section
list_current_fact_suggestions / list_fact_history
accept_fact / reject_fact / set_project_fact / clear_project_fact
create_clause / patch_clause_kind_or_text
confirm_clause / unconfirm_clause / reject_clause / delete_clause
promote_kind_router_contract (maintenance only)
```

API 不暴露 family 写入、candidate 表写入、current pointer 更新、raw SQL status 切换或旧 persist façade。

## 10. 专题验收

- SourceSpanV2 中文多字节 offsets、错误 section/generation/digest 全拒；
- clause/fact/disposition 原子 publication，CAS loss 零写；
- fact accept 与 publisher 竞态可线性化；
- kind/family CHECK 与 manual DTO 负例；
- service mutation 同时 bump matching/service/stale；
- promotion generation 2/3 连续 marker、manual 人工边界、失败回滚、并发 CAS；
- confirmed 跨 kind 先降 draft，reconfirm 后只进入 NEW membership 一次；
- 所有旧 direct persistence 和旧 client family 在最终实现中不存在。
