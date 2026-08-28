# 人工报价与不可变 QuoteSnapshotV1（V2 后端现状）

> 招投标 V2 黄金路径固定为 `files → authoring → export`。报价不是向导前置步骤，也没有单独 Web 步骤。权威总契约见 [`../tender-to-submission-v2.md`](../tender-to-submission-v2.md) 与 [`../../../docs/bidding/authoring.md`](../../../docs/bidding/authoring.md)。

**实施状态（2026-08-28）**：后端已切换为服务端计算、append-only 的 QuoteSnapshotV1；fresh baseline、ObjectRegistry、HTTP、ContentGenerate 冻结输入、Assessment、Preview、DOCX/PDF 和 Manifest 均使用同一不可变 ID/hash。旧 draft/reopen/eligibility/Part/Gate 路径不属于 Target V2，也不保留兼容实现。

## 1. 边界

V2 QuoteSnapshot 拥有：

- project-wide 报价 revision；
- CNY、含税/未税模式；
- 服务端逐行及总计计算；
- 人工逐行确认；
- 最高限价身份，或无最高限价时的完整人工复核；
- immutable canonical payload/hash；
- ObjectRegistry 对象身份、current pointer、audit 和 idempotency receipt。

V2 QuoteSnapshot 不拥有：

- mutable draft、reopen 或 eligibility 状态机；
- 自动定价、成本引擎、多币种或汇率；
- 招标事实抽取；
- 固定 Part、SubmissionGate 或 PDF 业务阻断；
- 独立 Web 向导。

## 2. 金额合同

- currency 固定 `CNY`，金额 scale 固定 2；
- quantity、unit_price、tax_rate 输入 scale 固定 6；
- Decimal 全部使用字符串，拒绝 JSON float、指数、正号、负数、多余 scale 和越界；
- 舍入固定为 midpoint-away-from-zero；
- 客户端不能提交或覆盖 basis/net/tax/gross/totals。

`unit_price`：

```text
quantity > 0
unit 非空
basis = round(quantity × unit_price, 2)
```

`lump_sum`：

```text
quantity = unit = unit_price = NULL
basis = entered_amount
```

税计算：

```text
tax_exclusive:
  net = basis
  tax = round(net × tax_rate, 2)
  gross = net + tax

tax_inclusive:
  gross = basis
  net = round(gross / (1 + tax_rate), 2)
  tax = gross - net
```

总计由服务端对已舍入行金额求和。空报价、空 description、未确认行、非法 tuple 或 overflow 均 fail-closed。

## 3. 最高限价与人工复核

`ceiling` 只能为 `null` 或冻结：

```text
amount, currency_code=CNY, basis=tax_inclusive|tax_exclusive,
ceiling_revision, ceiling_identity_sha256
```

- tax_inclusive 与 gross_total 比较；
- tax_exclusive 与 net_total 比较；
- 报价超过明确 ceiling 时拒绝发布；
- ceiling 缺失时必须提交 `no_ceiling_review`，其中包含 reviewed=true、bounded reason、当前 user actor 和 RFC3339 UTC 时间；
- ceiling 与 no_ceiling_review 互斥。

## 4. QuoteSnapshotV1 canonical payload

顶层闭合字段：

```text
schema_version, quote_id, project_id, revision,
currency_code, currency_scale, tax_mode, title, notes, lines,
net_total, tax_total, gross_total, ceiling, no_ceiling_review,
fact_revision, pricing_revision, pricing_set_sha256
```

每行闭合字段：

```text
id, ordinal, description, pricing_mode,
quantity, unit, unit_price, entered_amount, tax_rate,
basis_amount, net_amount, tax_amount, gross_amount, user_confirmed
```

canonical bytes 由 `crates/bidding/src/quote_snapshot.rs::build_quote_snapshot_v1` 唯一生成：UTF-8、closed schema、稳定字段顺序、UUID 小写连字符、CNY 金额 2 位、计算输入 6 位。`content_sha256` 是这些 exact bytes 的 SHA-256。

## 5. 数据与发布事务

唯一 baseline 使用：

```text
bid_quote_snapshot_artifacts
bid_quote_snapshot_object_identities
bid_quote_snapshot_current
```

发布顺序：

1. `kb_bid_v2_next_quote_snapshot_revision` 在 project row lock 下分配数据库 canonical `quote_id + revision`；
2. Rust 按该 identity 生成 canonical bytes/hash；
3. API 把 exact bytes 放入 ObjectRegistry staging；
4. `kb_bid_v2_publish_quote_snapshot` 在单事务中验证 actor、revision CAS、canonical schema、服务端金额、对象 identity；
5. commit 对象 owner reference，插入 append-only artifact/object identity，推进 current ID/hash/generation；
6. 写 audit 与首次 idempotency receipt。

SQL publication 不信任客户端 totals。普通 runtime role 不能直接 INSERT/UPDATE/DELETE/TRUNCATE QuoteSnapshot 表；PUBLIC 无 execute 权限。

相同 actor、operation、Idempotency-Key 和 request hash 只重放首次 receipt。API replay 创建的临时 staging 必须 abandon，不产生第二个 snapshot 或 owner reference。

## 6. HTTP

```text
POST /api/v2/bid-projects/{project_id}/quote-snapshots
GET  /api/v2/bid-projects/{project_id}/quote-snapshots
GET  /api/v2/bid-projects/{project_id}/quote-snapshots/{snapshot_id}
```

POST 只接受人工报价输入；DTO `deny_unknown_fields`。返回 immutable `quote_snapshot_id`、aggregate `quote_id`、revision、SHA、object_ref 和 byte_length。

不存在 V1 quote route、draft mutation、reopen、兼容读取、双写或 Feature Flag。

## 7. Authoring、Assessment 与 Export

- ContentGenerate request 创建时从 `bid_quote_snapshot_current` 冻结 ID/hash；Worker loader 只读取 typed request 中的该 immutable identity，不追查 live current；
- fulfillment binding 的 `target_kind=quote` 只接受同 project QuoteSnapshot；
- Preview 读取当前 immutable snapshot，并通过共享 LayoutDocument 渲染报价表；
- Assessment input hash 包含 QuoteSnapshot ID/hash；存在 pricing 要求但没有 snapshot 时产生 advisory `QUOTE_SNAPSHOT_MISSING`，不阻断导出；
- SubmissionExport request 冻结对应 Assessment；RenderSnapshot/Manifest 依赖同一 QuoteSnapshot ID/hash；
- DOCX/PDF 共用报价布局，不从 mutable draft 或客户端 totals 重算；
- 报价缺失、风险提示和 stale 都是业务 warning；Schema、CAS、对象、digest 或渲染错误才技术失败。

## 8. 验收

必须覆盖：

- exclusive/inclusive 税公式与逐行舍入后求和；
- unit-price/lump-sum closed tuple、负数、scale、unknown field 和 overflow 负例；
- ceiling gross/net 比较及 no-ceiling 人工复核；
- HTTP publish/replay 完全相等且无 staging 泄漏；
- append-only、ObjectRegistry owner、current ID/hash 和 runtime privilege；
- ContentGenerate loader 冻结同一 QuoteSnapshot；
- Preview、Assessment、assessment report、RenderSnapshot、Manifest、DOCX/PDF 使用同一 identity；
- fresh bootstrap 与 migrator re-entry；
- 最终源码中不存在旧 Part/Gate、mutable draft、reopen、客户端 totals 或兼容路径。
