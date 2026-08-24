# 人工报价与定稿快照

本文定义 `Quote` 深模块的最终 V1。报价由人录入和确认；系统只负责结构、计算、限价校验、快照和正式输出一致性。

> 实施状态（2026-08-24）：Rust Decimal 计算与 QuoteSnapshotV1 canonical seam，以及最终 baseline、storage/API 和 Web 报价路径均按本文合同落位；数据库、HTTP、mocked 浏览器和隔离 fresh runtime 的人工金额/finalize/reopen/正式输出验收均已通过，生产未部署。

## 1. 边界

Quote 拥有：

- 一个项目的一条 quote 聚合；
- draft revision、line、edit version；
- 行/总计计算与 CNY/税模式校验；
- immutable `QuoteSnapshotV1`；
- active finalized pointer 与 eligibility；
- finalize/reopen/audit/idempotency。

Quote 不拥有：

- 招标事实抽取或 pricing clause；
- 产品成本和自动报价引擎；
- 多币种/汇率；
- submission part 内容编辑器。

## 2. 数据模型

```text
Quote
  id, project_id UNIQUE
  current_draft_revision_id NULLable
  active_finalized_snapshot_id NULLable
  next_revision

QuoteRevision
  id, quote_id, project_id, revision
  status = draft|finalized|reopened
  edit_version
  currency_code = CNY
  currency_scale = 2
  tax_mode = tax_inclusive|tax_exclusive
  title
  notes NULLable
  based_on_snapshot_id NULLable
  actor/timestamps

QuoteLine
  id, quote_revision_id, ordinal
  description
  pricing_mode = unit_price|lump_sum
  complete
  quantity, unit, unit_price
  entered_amount
  tax_rate
  basis_amount, net_amount, tax_amount, gross_amount
  user_confirmed

QuoteSnapshot
  id, quote_id, revision_id, project_id
  schema_version = 1
  canonical_payload, content_sha256
  title, notes, tax_mode, totals
  ceiling provenance
  fact/pricing provenance
  eligibility
  finalized actor/time
```

约束：

- 创建首个 draft 后 `current_draft_revision_id` 与 `active_finalized_snapshot_id` 恰有一个非 NULL；
- draft pointer 只能引用同 project/quote 且 status=draft 的 revision；
- active pointer 只能引用同 project/quote 的 finalized snapshot；
- snapshot payload/hash/totals/provenance immutable；历史 snapshot 不删除；
- 普通 DB role 不能 UPDATE/DELETE/TRUNCATE snapshot。

## 3. 金额与税

### 3.1 通用表示

- currency 固定 `CNY`；金额 scale=2；
- quantity、unit_price、tax_rate 可用 scale=6；
- API/canonical JSON 的 Decimal 一律为字符串，禁止 float、指数、`+`、`-0`；
- 所有负数拒绝；折扣用非负调整行和 notes 表达；
- round 固定 half-away-from-zero。

### 3.2 unit price

complete tuple：

```text
quantity > 0
unit 非空
0 <= unit_price <= 10^12
basis = round(quantity * unit_price, 2)
```

quantity 上限 `10^9`；乘法中间值必须可放入 `numeric(30,6)`，结果必须可放入 `numeric(20,2)`。

### 3.3 lump sum

complete tuple：

```text
quantity = unit = unit_price = NULL
0 <= entered_amount <= 10^18 - 0.01
basis = entered_amount
```

### 3.4 税计算

`tax_rate` 范围 `[0,1]`。

```text
tax_exclusive:
  net = basis
  tax = round(net * tax_rate, 2)
  gross = net + tax

tax_inclusive:
  gross = basis
  net = round(gross / (1 + tax_rate), 2)
  tax = gross - net
```

totals 按 ordinal 对每条已经舍入至 2 位的 net/tax/gross 分别求和，不重新汇总未舍入中间值。Rust 与 DB 使用同一输入向量重算；任一金额或 total 越界返回 `QUOTE_AMOUNT_OVERFLOW`。

空 description、空报价、incomplete line、未 `user_confirmed` line 都禁止 finalize。

## 4. 最高限价口径

BidProject 的最高限价身份必须包含：

```text
ceiling_price NULLable
ceiling_currency NULLable (CNY)
ceiling_basis = tax_inclusive|tax_exclusive|unspecified
ceiling_revision
ceiling_identity_sha256
```

规则：

- ceiling 为空时 currency 为空，basis 固定 `unspecified`；
- ceiling 非空时 currency=CNY，basis 可由抽取/人工设置；无法从招标原文确定时保留 `unspecified`，不得猜测；
- `tax_inclusive` 与 quote `gross_total` 比较；
- `tax_exclusive` 与 quote `net_total` 比较；
- `unspecified` + 非空 ceiling 禁止 finalize，返回 `CEILING_BASIS_UNSPECIFIED`；用户必须先明确修改项目事实；
- 比较值大于 ceiling 拒绝，等于允许；
- ceiling value/currency/basis/有无状态任一实际变化都 bump `ceiling_revision` 和 digest。

这样不会把含税/未税限价静默混用，也不需要在 quote 内再造第二份限价口径。

## 5. QuoteSnapshotV1

### 5.1 canonical payload

唯一 storage seam `build_quote_snapshot_v1` 生成 canonical bytes/hash。顶层固定键序：

```text
schema_version,quote_id,project_id,revision,currency_code,currency_scale,
tax_mode,title,notes,lines,net_total,tax_total,gross_total,ceiling,
no_ceiling_review,fact_revision,pricing_revision,pricing_set_sha256
```

每行固定键序：

```text
id,ordinal,description,pricing_mode,quantity,unit,unit_price,entered_amount,
tax_rate,basis_amount,net_amount,tax_amount,gross_amount,user_confirmed
```

`ceiling` 只能为 `null` 或：

```json
{
  "amount": "1000000.00",
  "currency_code": "CNY",
  "basis": "tax_inclusive",
  "ceiling_revision": 3,
  "ceiling_identity_sha256": "<64 lowercase hex>"
}
```

`no_ceiling_review` 只能为 `null` 或：

```json
{
  "reviewed": true,
  "reason": "招标文件未设置最高限价，已人工复核",
  "actor_kind": "user",
  "actor_id": "<uuid>",
  "at": "<RFC3339 UTC microseconds>"
}
```

两者互斥：有 ceiling 就必须有明确 basis 且 no-ceiling review 为 null；无 ceiling 就必须冻结完整人工 review。

### 5.2 字节规则

- UTF-8、无 BOM、无额外空白/额外键；
- title trim 后 1..256 bytes；notes <=4096 bytes，空白规范为 NULL，但 canonical 键仍显式 `null`；
- strings 不做 NFC/NFD 转换；quote/backslash/control chars 按固定小写 JSON escape；
- UUID 小写连字符；time 为 UTC 微秒；digest 为 64 位小写 hex；
- quantity/unit_price/tax_rate 固定 6 位 string，CNY 金额固定 2 位 string；
- lines 按 ordinal，hash 为 canonical UTF-8 bytes 的 SHA-256；
- JSONB 只是解析存储，不能用数据库任意 JSON 输出重新计算 hash。

跨 Rust/SQL exact fixture 至少覆盖中文、escape、notes NULL/非 NULL、有 ceiling、无 ceiling review 和 pricing digest。

## 6. draft 编辑

所有 draft mutation 需要：

```text
durable actor
idempotency_key
expected_edit_version
canonical payload hash
```

成功后：

- edit version +1；
- 写 revision/line；
- 重算预览 totals；
- 写 audit/receipt；
- stale quote preview/相关 draft part；
- 同事务提交。

建议价格只能由用户显式“应用”成一次普通 draft edit；模型/worker 不能后台写正式 line 或设置 `user_confirmed=true`。

## 7. finalize

请求必须携带：

```text
expected_edit_version
expected_fact_revision
expected_ceiling_revision
expected_ceiling_identity_sha256
expected_pricing_revision
expected_pricing_set_sha256
idempotency_key
```

锁序：

```text
project -> quote -> current draft revision -> lines(ordinal)
```

单事务：

1. project open、pointer/current revision/expected CAS；
2. draft 非空，全部 line complete + user_confirmed；
3. DB/Rust 重算每行和 totals；
4. pricing revision/digest current；
5. ceiling current：
   - 有 ceiling：basis 必须明确，按 basis 比较；
   - 无 ceiling：请求必须含 `no_ceiling_reviewed=true + bounded reason`；
6. 构建 QuoteSnapshotV1，验证 canonical bytes/hash；
7. revision -> finalized；
8. `current_draft=NULL`，active pointer -> 新 eligible snapshot；
9. 写 audit、stale 和首次 receipt。

`expected_fact_revision` 只证明 finalize 时读取的是一致项目事实，不作为持续 eligibility 的全局比较字段。

## 8. eligibility

snapshot 状态：

```text
eligible
ineligible_ceiling_changed
ineligible_pricing_changed
ineligible_multiple_inputs_changed
superseded
```

只允许受控单向变化：eligible -> 单原因/multiple/superseded；单原因 -> multiple/superseded；multiple -> superseded。不得回到 eligible，不得用另一单原因覆盖旧原因。

- ceiling identity 变化：标记 ceiling changed；
- pricing revision/digest 变化：标记 pricing changed；
- 两者都发生：multiple；
- 普通 budget、开标、截止或有效期变化只 stale 实际消费者，不使 quote snapshot ineligible；
- active pointer 不因 ineligible 自动清空，保留可审计 identity；
- PDF 只接受 active + eligible，且 current ceiling/pricing identities 与 snapshot 一致。

## 9. reopen

```text
reopen(
  expected_snapshot_id,
  expected_fact_revision,
  expected_pricing_revision,
  idempotency_key
)
```

验证 expected snapshot 等于 active（eligible 或 ineligible）后：

- 旧 revision -> reopened；
- 旧 snapshot -> superseded；
- 从 snapshot 冻结的 title/notes/lines 逐字段复制到 `next_revision` draft；
- `based_on_snapshot_id` 指向旧 snapshot；
- active pointer 清空，current draft 指向新 revision；
- 不读取被后续 live UI 改动的内容回填。

重新 finalize 前禁止正式 PDF。

## 10. Submission 读取

- 有 active eligible snapshot 时，`6:letter` 与 `6:quote` 必须引用同一个 snapshot ID/hash；
- 无 eligible snapshot 时，DOCX 可冻结 NULL quote dependency 并使用固定 placeholder/warning；
- PDF 对 NULL、ineligible、identity 不 current 均拒绝；
- renderer 只读 manifest 中的 snapshot artifact，不读 live draft。

## 11. API

```text
get_quote
create_quote_draft
patch_quote_header
upsert_quote_line / delete_quote_line / reorder_quote_lines
preview_quote_totals
finalize_quote
reopen_quote
get_quote_snapshot
```

所有 write DTO 禁止客户端提交计算后 totals、snapshot hash、eligibility 或 active pointer。

## 12. 专题验收

- unit/lump complete tuple、负数、边界、overflow；
- inclusive/exclusive 税公式与逐行舍入后求和；
- ceiling inclusive 比 gross、exclusive 比 net、unspecified 拒绝；
- no-ceiling 完整 review 与 actor；
- title/notes 中文/escape canonical exact bytes；
- finalize CAS、并发、pointer 恰一、snapshot immutable；
- ceiling/pricing 单原因与 multiple eligibility 状态图；
- reopen 逐字段复制旧 snapshot，不读 live draft；
- DOCX placeholder 与 PDF hard gate；
- 最终实现中不存在 JSON float、自动正式定价或从旧报价格式读取的兼容路径。
