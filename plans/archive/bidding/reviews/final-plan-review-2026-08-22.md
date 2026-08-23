# 投标完整方案评审

| 项 | 值 |
| --- | --- |
| 对象 | `plans/bid-platform-complete-solution.md` |
| 口径 | **只评方案自身是否正确**。现网实现与方案不一致，一律视为后续全面整改，不拿来否决方案。 |
| 日期 | 2026-08-22 |

上一轮 1–5 都已收口。按同一口径只评方案内部。

**结论：目的地可以当真源。还剩一处会把条款卡死的状态机漏洞，修完即可按方案整改实现。**

---

## 上一轮对照

| 原 Issue | 本次 |
| --- | --- |
| 1 promotion 改已确认 kind | **已修。** 先 `confirmed→draft` 退出 OLD 集合，再写新 kind；`KIND_ROUTER_PROMOTION_RECONFIRM` + GateIssue；人工 confirm 后才进 NEW |
| 2 §5.4 漏 service 列 | **已修。** `service_revision/service_set_sha256` 在 5.4，ownership 钉在 0015 |
| 3 未归段 unit_id | **已修。** report/projection/PickSet 用 nil UUID 字符串；part key 只能是 `2:unsectioned` |
| 4 0013 漏 `FROZEN_SCOPE` | **已修。** allowlist 显式含 `FROZEN_SCOPE` 并沿用 `EMPTY_ROUTE\|SKIP_UNIT` |
| 5 §5.1 vs §5.2 | **已修。** FactSuggestionAgent 指向 §5.2 |

---

## 仍未闭合

### Issue 1 — Severity: bug

- **File:** `plans/bid-platform-complete-solution.md:193-195`
- **Description:** 跨 kind 的 promotion 协议只处理 `status='confirmed'`。confirm 又要求 `confirmation_required_router_generation` **仍等于 current promotion generation**。因此：
  1. v2 把已确认 `service` 降成 draft，marker=`KIND_ROUTER_PROMOTION_RECONFIRM`、generation=2；
  2. 人还没确认又做 v3：行已是 draft，协议跳过，kind 与 marker.generation 仍停在 2；
  3. current generation 已是 3，confirm 稳定失败。

  这条条款既进不了 NEW 集合，也 confirm 不了，PDF 会永久 `KIND_ROUTER_RECONFIRMATION_REQUIRED`。
- **Suggestion:** 每次 promotion 对两类行都重算 kind，并把 marker.generation 写成 **本次** current：① 仍为 confirmed 且 kind 变化 → 现有先降 draft 协议；② 已是 draft 且带 `KIND_ROUTER_PROMOTION_RECONFIRM`（或仍带 frozen extracted span）→ 保持 draft，按新 Router 更新 kind/family，刷新 generation。fixture 覆盖「两次 promotion、中间无人确认，第三次 confirm 必须成功」。

---

## 建议（不挡把方案当真源，但应写死）

### Issue 2 — Severity: suggestion

- **File:** `:193` / `:206`
- **Description:** KindRouter 消费的是 frozen SourceSpanV2 + proposal。Promotion「对任一 confirmed clause 算出新 kind」没说 **manual / `manual_after_edit`** 怎么算：这类条款没有 converted artifact，也不能走 extracted 的 span verifier。若对全部 confirmed 跑 Router，输入契约不够；若只处理 extracted，应写明人工条款 kind 除非 PATCH 否则不变。
- **Suggestion:** 写死只对有 frozen SourceSpanV2 的 extracted 条款重算 kind；manual 条款不因 promotion 改 kind。

### Issue 3 — Severity: nit

- **File:** `:1069`
- **Description:** `2:unsectioned` 要求「每个**相关** PickSet item 的 unit_id 均为 nil UUID」。项目级 PickSet 里还有普通 unit 的 picks。「相关」未定义的话，可能被理解成整个 pick_set 都必须是 nil。
- **Suggestion:** 写成 subset：`unit_id = nil UUID` 且 `source_report_artifact_id` 等于该未归段 report。

---

## 总评

| 维度 | 判断 |
| --- | --- |
| 产品目的地 | 正确。①–⑥、kind/family、两路 matching、事实/报价、Word/PDF 门禁已收成一套。 |
| 方案内部 | 只剩 Issue 1：连续 promotion 会把待确认 draft 卡死。Issue 2–3 是补全，不是第二套真源。 |
| 现网 | 仍不作为否决。 |

**建议：** 只改方案 Issue 1（最好连同 Issue 2）。收完后可以把本文当施工真源，按方案全面整改实现。
