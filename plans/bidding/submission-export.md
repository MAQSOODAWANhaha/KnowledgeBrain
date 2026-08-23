# ①～⑥组卷、程序材料与正式导出

本文定义 `Submission` 深模块。它只消费其它模块已经发布的 identity/artifact，不读取临时 candidate、live quote draft 或知识库 live rows。

## 1. 输出语义

- DOCX/Word：过程稿，可带 warning、缺失清单和固定 placeholder，可供人工修订。
- 正式 PDF：内容与依赖冻结、通过 `SubmissionGateV1`，可打印和线下签章。
- V1 不包含 CA 电子签章、电子投标平台登录、加密上传或自动递交。
- DOCX 与 PDF 必须从同一类 manifest/render pipeline 生成，不能各自拼一套 live 数据。

## 2. RequiredPartSet

### 2.1 part keys

```text
1
2:{lowercase-unit-uuid}      # 每个 current 普通 technical unit 一份
2:unsectioned                # 存在 unsectioned technical route 时
3
4
5
6:letter
6:authorization
6:quote
6:implementation_plan
6:procedural
```

实际 part key 映射固定 template slot：

```text
2:{uuid} -> 2:unit
2:unsectioned -> 2:unsectioned
其它 key -> 同名 slot
```

未知 key 拒绝；模板不能动态创建任意业务 part。

### 2.2 内容与依赖

| part | 内容 | 唯一主要依赖 |
| --- | --- | --- |
| `1` | 项目概况、预算、限价、时间等 | FactIdentityV1、project/profile identity |
| `2:{unit}` | 某技术单元逐条应答 | 对应 MatchingReportV1 + RoutePickSetV1 |
| `2:unsectioned` | 未归段技术应答 | 唯一报告 `R` + 精确 pick 子集 `S` |
| `3` | 总体产品与解决方案 | ProjectPickSetV1 + BidShotSetV1 |
| `4` | 公司资质/服务证据 | current commercial matching reports/decisions |
| `5` | 偏离、未解决、缺件 | current matching/procedural missing identities |
| `6:letter` | 投标函 | facts + profiles + active quote identity/NULL placeholder |
| `6:authorization` | 授权材料 | profile + authorization classifications/decisions/attachments |
| `6:quote` | 正式报价表 | active eligible QuoteSnapshotV1/NULL placeholder |
| `6:implementation_plan` | 实施与交付计划 | ServiceClauseSetV1 + ProjectPickSetV1 + delivery set |
| `6:procedural` | 程序材料检查 | procedural segment/classification/decision/attachment sets |

`6:implementation_plan` 不读取 commercial matching report 作为服务条款真源，也不读取所有 technical reports；它只读明确列出的三类 identity。

## 3. Profiles

### 3.1 CompanyProfileV1

固定必填：

```text
legal_name
unified_social_credit_code
registered_address
legal_representative
contact_name
contact_phone
contact_email
```

### 3.2 SubmissionProfileV1

固定必填：

```text
buyer_name
project_code
authorized_representative
submission_date
submission_place
seal_confirmed
signature_confirmed
```

日期按 `Asia/Shanghai` 解释并在 canonical object 中使用明确日历日期；空白等同缺失。两个 profile 是独立 standalone canonical object，不使用含义模糊的通用 map 或 `schema_version,items` set wrapper。

mutation 使用 durable actor、expected revision、idempotency；更新 profile identity 并 stale 所有真实消费者。

## 4. 程序条款

### 4.1 stable segments

每个 current confirmed `procedural` clause 先生成 `ProceduralSegmentSetV1`。

- 未编辑 extracted clause 复用合法 current SourceSpanV2 routed segment key；
- manual/manual_after_edit 按 `。；！？\n` 和有界编号列表边界切分；
- offsets 是相对 current clause text 的 UTF-8 byte 半开区间；
- trim 只移除两端 Unicode whitespace 并同步收缩 offsets；
- 空 segment 丢弃，最多 1024 段；
- stable key 由 clause ID、segmentation version、start/end 和 segment bytes digest 组成，不包含 ProceduralRouter version。

manual_after_edit 永久保留 extracted origin，但 current segment provenance 必须标记人工编辑后内容。

### 4.2 ProceduralRequirementRouterV1

每个 segment 独立分类；同一 clause 可以有多类要求。

| 优先级 | effective kind | guard | veto/review |
| --- | --- | --- | --- |
| 1 | `bid_bond` | 保证金、保函、缴纳凭证/回执且要求提交证明 | 只有金额无履行动作 -> review |
| 2 | `authorization_support` | 授权委托书、法定代表人/代理人证明附件 | 只有口头确认无材料对象 -> review |
| 3 | `seal_sample` | 印章样本、签章样张、盖章截图/图样附件 | 普通投标函盖章、骑缝章 -> confirmation |
| 4 | `procedural_support` | 明确命名的其它程序附件、上传/递交/加密回执 | 裸“支持材料”、资格证书 veto |
| 5 | `confirmation` | 只需人工确认的签字盖章、密封、线下递交动作 | 明确附件对象优先 |

不可再拆的同一 segment 同时要求不同附件/确认方式时，输出 `review + MULTIPLE_PROCEDURAL_REQUIREMENTS`，kind 为 NULL，不默认放行。

固定 golden：

```text
提交授权委托书原件 -> authorization_support
投标函签字并盖章 -> confirmation
密封袋加盖骑缝章 -> confirmation
上传保证金缴纳回执 -> bid_bond
提交平台上传成功回执 -> procedural_support
上传 ISO 证书 -> 顶层 qualification，不进入本 Router
按要求提供支持材料 -> review/missing
```

### 4.3 classification

classification 分开保存：

```text
router_result_status = classified|review
router_requirement_kind NULLable
review_reason NULLable
effective_requirement_kind NULLable
override_from/to/actor/reason/time NULLable
lifecycle_status = current|superseded
revision
successor_id XOR terminal_reason/time/actor
```

不变量：

- classified：router kind 非 NULL、review reason NULL；未 override 时 effective=router kind；
- review：router kind NULL、review reason 非空；未 override 时 effective NULL；
- override 保留原始 Router 结果，只设置 effective kind 和完整 actor/reason；
- current 没有 successor/terminal；superseded 必须是 same-key higher revision successor 或无 successor 的 terminal，二者互斥；
- same-key reclass/override 插入 successor，不原地改语义；
- current set 中每个 segment 至多一条 current classification；缺失允许持久化但 Gate 必须阻断。

### 4.4 lifecycle terminal

stable segment 消失时不伪造 successor，terminal reason 按固定优先级：

```text
clause_deleted
clause_unconfirmed
left_procedural
text_changed
resegmented
segment_removed
```

KindRouter promotion 接入 ClauseLifecycle 已有事务 seam：

- eligible extracted clause kind 不变但 segment key 因 Router target 改变：旧 classification/decision 以 `segment_removed` terminal，新 key 重新分类并等待人工处理；
- kind 改变：ClauseLifecycle 先执行 confirmed -> draft，旧程序记录以 `clause_unconfirmed` terminal；
- manual/manual_after_edit 不自动改 kind；待确认 marker 的连续 promotion 仍按 ClauseLifecycle 规则刷新；
- Submission 只扩展程序记录、stale 与 Gate，不复制 clause 状态机。

### 4.5 decision

resolution 只引用 current classification ID/revision，不允许客户端再次提交 requirement kind：

```text
confirmed_by_user
satisfied_by_attachment
not_applicable
```

- `confirmed_by_user` 只适用于 effective kind=`confirmation`；
- `satisfied_by_attachment` 必须引用同项目、current、valid、confirmed、kind 匹配的 attachment；
- `not_applicable` 需要非空 reason 和 durable actor，attachment 必须 NULL；
- effective kind NULL 时不能创建 resolution；
- decision 修改插入 higher revision successor；classification superseded 或 segment terminal 时 current decision 同步 terminal。

## 5. Attachments

attachment 生命周期至少包含：

```text
kind
object_artifact_id
validation_status = pending|valid|invalid
status = draft|confirmed|rejected|superseded
revision
actor/timestamps
```

upload/replace/delete/validate/confirm/reject 是独立 typed operation，使用 expected revision + idempotency。validated object artifact 与 attachment 通过 project composite FK 和 ObjectRegistry reference 互证。

只有 `validation_status=valid AND status=confirmed` 才能满足 Gate。附件集合按 kind 维护 revision/digest，任何变化在同一事务 stale 对应 part。

## 6. PickSet 与技术 parts

### 6.1 普通 unit

每个 current technical unit report 对应 `2:{unit}`。part dependency 必须引用：

- exact report ID/hash/generation/watermark；
- exact RoutePickSetV1 revision/hash；
- requirement/candidate membership verifier；
- template slot/version；
- part content revision/hash。

### 6.2 unsectioned

定义：

```text
R = 唯一 current unsectioned technical MatchingReportV1
S = ProjectPickSetV1.items
    WHERE source_report_artifact_id = R.id
```

只对 `S` 要求：

- 每项 unit ID 为 lowercase nil UUID；
- candidate 属于 `R`；
- 只进入 `2:unsectioned`。

普通 unit items 可以和 `S` 共存在同一 ProjectPickSetV1。不得把整个选择集强制 nil，也不得用 `unit_id=nil` 代替对 report ID 的精确筛选。

## 7. BidShot 与可编辑内容

### 7.1 BidShotSetV1

BidShot 是某次投标使用的图片 artifact，不回写知识库产品手册。current placement 使用 project 内唯一 ordinal；替换或位置变化总是新增 immutable shot artifact 再切 current pointer，不复用历史 artifact。

每次变化重建 BidShotSetV1 并 stale part `3`。正式 renderer 不读取 live `bid_shots.object_key`。

### 7.2 part content

每个 part 有 current editable content revision 与 immutable canonical Markdown artifact。编辑只改变该 part 的 content identity；生成/重生成必须携带 expected dependency/current content CAS，不能覆盖并发人工修改。

Markdown image grammar 固定且只允许 `objects/<64-lowercase-hex>`。manifest 创建时用固定 parser 解析 occurrence，不能在 render 时用临时 regex 重新发现资源。

## 8. PartDependencyV1 与 stale

### 8.1 dependency

每个 dependency 都固定：

```text
schema_version
project_id, part_key, template_slot, template_version
input identities[]
part_content_revision, part_content_sha256
generated_at
```

input identity 必须为 typed union，例如 fact、clause set、matching report、route/project pick、quote snapshot、profile、procedural sets、attachment set、shot set。禁止通用字符串键值 map。

### 8.2 stale 图

| 输入变化 | stale parts |
| --- | --- |
| 项目事实 | `1` 及实际读取字段的 `6:letter` |
| ordinary technical report/pick | 对应 `2:{unit}`、`3`、可能的 implementation plan |
| unsectioned report/pick 子集 | `2:unsectioned`、`3`、可能的 implementation plan |
| commercial report | `4`、`5` |
| service set | `6:implementation_plan` |
| pricing set/ceiling | quote eligibility、`6:quote`、`6:letter` |
| delivery set | `6:implementation_plan` |
| procedural set/classification/decision/attachment | `6:authorization`、`6:procedural`、`5` |
| profiles | 所有真实读取对应字段的 part |
| BidShotSet | `3` |
| template contract promotion | 使用该 slot 的所有 parts |
| KindRouter 待重新确认 | 相关 parts + PDF Gate |

领域 mutation 必须在同一事务更新 identity 与 stale；不得由异步扫描最终一致地补 stale。

## 9. SubmissionGateV1

### 9.1 GateIssue

issue 至少包含稳定 code、part key、entity locator、current/expected identity 和用户可执行的修复路径。固定重要 code：

```text
PROFILE_FIELD_MISSING
SIGNATURE_OR_SEAL_NOT_CONFIRMED
PROCEDURAL_CLASSIFICATION_MISSING
PROCEDURAL_CLASSIFICATION_REVIEW
PROCEDURAL_DECISION_MISSING
ATTACHMENT_NOT_VALID
PART_MISSING
PART_STALE
QUOTE_NOT_FINALIZED
BID_VALIDITY_CONFLICT
KIND_ROUTER_RECONFIRMATION_REQUIRED
DEPENDENCY_NOT_CURRENT
```

### 9.2 Word/PDF 矩阵

| 输入 | DOCX | 正式 PDF |
| --- | --- | --- |
| profile 缺失或 seal/signature=false | warning | 拒绝 |
| classification 缺失/过期/review 未 override | warning | 拒绝 |
| current procedural segment 无 decision | warning | 拒绝 |
| valid confirmed compatible attachment | 允许 | 允许 |
| not applicable + reason + actor | warning 显示原因 | 允许并冻结 |
| attachment pending/rejected/wrong kind | warning | 拒绝 |
| part/dependency stale | warning | 拒绝 |
| 无 active eligible quote | 固定 placeholder + warning | 拒绝 |
| bid_valid_days 与 bid_valid_until 冲突 | warning | 拒绝 |
| KindRouter 待重新确认 | warning + clause locator | 拒绝 |

Gate 逐 current procedural segment 评估；同 clause 其它 segment 已满足不能代替当前 segment。

## 10. SubmissionManifestInputV1

manifest 创建必须在 project transaction 中冻结：

```text
schema_version
manifest_id, project_id
format = docx|pdf
required_part_keys
part_artifacts + PartDependencyV1
fact/profile/report/pick/quote/procedural/attachment/shot identities
template contracts
render asset occurrences
gate result/issues
created_by, created_at
```

规则：

- `required_part_keys` 精确等于从 current routes 派生的 RequiredPartSet，不能由调用者删项；
- PDF 创建时 Gate 必须 pass；DOCX 可冻结 warnings；
- 有 eligible quote 时 letter/quote 两 part 使用同一 snapshot；无 eligible 只允许 DOCX 使用 NULL dependency/placeholder；
- manifest payload 与规范化 relation rows 由 deferred verifier 互证；
- 创建前锁 current pointers，创建后得到 immutable manifest；
- render 完成/发布前重新比较 project/end state 与全部 current identities；变化则失败，不把过时文件发布为 current。

## 11. Render assets 与平台对象服务

### 11.1 消费边界

`ObjectRegistry`、对象状态、owner reference 与 retention 内部协议只由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 定义。Submission 通过受检平台接口创建/移除业务 owner reference、读取 manifest asset；不维护第二套 refcount，不直接删除 blob，也不拥有 retention claim/lease/retry。

最终 cutover 删除 Submission 旧 `content_objects.ref_count`、公开对象删除旁路和直接 blob 访问；平台侧删除与恢复语义不在本文复制。

### 11.2 manifest render relation

每个 BidShot/Markdown occurrence 转为 manifest-owned immutable render artifact/relation，冻结：

```text
source_kind = bid_shot|markdown_object
source artifact/part/occurrence locator
object_ref/digest/media type/byte length
placement/ordinal
manifest owner reference
```

manifest 创建时验证 available、ownership、MIME/魔数、bytes、pixels 和全局配额。重复同一 object occurrence 合法，但每个 occurrence 有独立 locator/reference。

renderer 只能调用 `read_manifest_render_asset`，禁止查询 live shot/part/object table 或直接读任意 object key。

## 12. 模板 contract promotion

每个固定 slot 保存 immutable template contract artifact 与 singleton current pointer/generation。promotion 只在 maintenance gate 下：

1. 锁 template current rows；
2. 按 project UUID 锁 open projects；
3. stale 使用旧 version 的 parts；
4. 原子切 target pointer/generation；
5. 失败全回滚。

maintenance 中拒绝 schedule/export；恢复后 PDF 对旧 template dependency 稳定拒绝，DOCX 可提示重建。

## 13. API

```text
get/update_company_profile
get/update_submission_profile
list_procedural_requirements
override_procedural_classification
resolve_procedural_requirement
upload/validate/confirm/reject/delete_attachment
get/update/regenerate_part
list_gate_issues
create_submission_manifest
render_docx
render_pdf
download_submission_artifact
promote_procedural_router/template_contract (maintenance only)
```

必须删除旧 `export` / `regenerate_stale` 无 CAS 旁路和旧 part/client family；新 API 不接收 caller-defined RequiredPartSet、family、computed Gate result 或任意 object key。

## 14. 专题验收

- RequiredPartSet 动态 unit、unsectioned 与缺 part 负例；
- 普通 unit + unsectioned 混合 ProjectPickSet，`R/S` 精确筛选；
- implementation plan 只读 ServiceClauseSet+ProjectPickSet+delivery；
- manual/manual_after_edit 中文 segment offsets 和 golden classifier；
- classification/decision successor XOR terminal、KindRouter promotion terminal/rebuild；
- attachment kind/validation/current identity；
- GateIssue exact locator 与 DOCX/PDF 矩阵；
- quote NULL placeholder 与 active eligible snapshot 同一性；
- part edit/regenerate CAS 和 complete stale graph；
- manifest create/end race、dependency current race；
- Markdown/BidShot occurrence、MIME/size/pixels/owner reference，以及平台对象生命周期集成；
- renderer 读取 manifest-only，ObjectRegistry 没有第二套 refcount；
- 旧 export/regenerate/object-delete 旁路和旧 client 全部不存在。
