# 招投标实施与验收（Legacy V1验收快照）

> Target V2分阶段实施和最终验收只以[`tender-to-submission-v2.md`](tender-to-submission-v2.md)为准，目标契约见[`../../docs/platform/tender-to-submission-authoring.md`](../../docs/platform/tender-to-submission-authoring.md)。本文保留V1命令、表名和门禁仅供删除扫描与历史证据定位，不得用于判定Target V2完成。

本文记录V1的实施顺序、删除范围和验收快照。

## 0. 当前边界

| 层次 | 当前状态 |
| --- | --- |
| 方案 | 产品基线与 Oxana 原生能力简化方案均已批准；当前代码尚未按简化方案完成收敛 |
| implemented | 部分；产品主链和旧 transport seam已有实现，Oxana原生能力简化方案尚未完成 |
| locally verified | 部分；历史局部测试不能代替本轮简化方案和最终全量门禁 |
| committed/pushed | 当前分支和 dirty 状态以 Git 实际输出为准；本轮方案未提交、未 push |
| deployed | 否 |
| runtime accepted | 否；`phase_1d_runtime_complete=false` |

后续实现必须以本方案为准；修改职责边界前必须重新评审。实施后仍分别报告 `implemented`、`locally verified`、`committed`、`pushed`、`deployed` 和 `runtime accepted`。

## 1. 成功标准

最终完成时必须同时成立：

1. 空 PostgreSQL、Redis 和 object volume 可一次建立完整系统；
2. 最终 schema 没有旧投标表、兼容 view、alias、runtime repair 或历史回填；
3. API、Web 和 worker 只使用最终接口；
4. 六类异步 target 由 Oxana 负责 retry/resurrection/dead queue，由 PostgreSQL 只负责业务revision与幂等publish；
5. 所有旧 best-effort enqueue、`system:live-recovery:v1` 和 Bid private Redis correctness 路径已删除；
6. 单元、活库、HTTP、Web、Compose 和 runtime 验收均通过且无静默 skip；
7. 正式 DOCX/PDF 可由 manifest、artifact hash、ObjectRegistry owner 和 audit 证明来源；
8. 每次容器化测试结束立即清理本轮 container、volume、network 和临时 image，残留数为零。

## 2. Baseline 策略

最终只维护一套 fresh baseline manifest：

```text
knowledge_base_baseline.sql
shared_platform_baseline.sql
bidding_v1_baseline.sql
```

必须满足：

- 不先创建旧结构再 ALTER/backfill 到目标结构；
- 应用启动只校验 schema identity，不执行 DDL；
- extension、seed contract、current pointer、role/grant/revoke 纳入 checksum；
- fresh database 测试验证 catalog allowlist和旧结构 denylist；
- runtime code 不包含 repair DDL 或兼容写入。

共享平台合同见 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md)，队列合同见 [`../platform/queue-runtime.md`](../platform/queue-runtime.md)。

## 3. 业务切片

### 3.1 TenderPublication / ClauseLifecycle

- document、converted source、section 和 SourceSpanV2；
- conversion/extraction target、business generation/revision 和原子 publication；
- fact suggestion/decision/current fact；
- clause kind/family/status、origin/current span；
- KindRouter artifact/current/promotion marker；
- template/procedural router contract promotion。

关键验收：中文 byte span、多级数字标题、普通正文不误判为标题、金额字段分别解析且支持千分位；程序 splitter 必须满足 `1. 提交`会切、`10.00 万元`不切；promotion generation 2/3、管理员权限和 maintenance gate均有测试。

### 3.2 MatchingPublication

- frozen manifest/routes/eligible scope/requirements；
- knowledge scope attestation、EvidenceV1、decision/report canonical artifact；
- matching job target/job identity、staging 和原子 publish；
- current technical/commercial projections；
- RoutePickSet/ProjectPickSet revision与 current pointer。

关键验收：eligible scope 与 hit quota 解耦、普通和 unsectioned 混合、staging failure/TTL cleanup/Oxana retry、两个不同 pick 持久化、连续 pick 使用最新 revision或串行 mutation，不因共享旧 revision丢第二次选择。

### 3.3 Quote

- draft、line 和 Decimal CNY 计算；
- ceiling value/basis；
- QuoteSnapshotV1 finalize/eligibility/reopen；
- API/Web 可录入描述、数量、单价、税率和金额。

关键验收：numeric(20,2) 全范围、含税/未税/无上限三种 ceiling 口径、canonical bytes、finalize/reopen CAS；出现 ceiling 后 UI 清除 `noCeiling`，请求只在确实无上限时发送 `no_ceiling_reviewed=true`；live E2E 使用非零真实报价。

### 3.4 Submission

- company/submission profile artifact和 current pointer；
- procedural segment/classification/decision/attachment；
- attachment preparation、page staging 和原子 publish；
- RequiredPartSet、part dependency/current/stale；
- BidShot、manifest render assets、DOCX/PDF output；
- template和 procedural router register/promote。

关键验收：附件上传可选择 authorization、bid bond、seal sample 等实际 kind；程序卡片显示正文；未编辑 extracted clause 复用 SourceSpanV2 key，manual/manual_after_edit 按编号列表切分；正式报价表包含完整 snapshot lines；已确认程序附件进入 manifest assets和正式文件；Markdown 图片语法被消费而不显示内部 key；PDF 文本换行、图片同时受 A4 宽高约束；PDF字体文件与许可证冻结digest一致；Gate 拒绝时 DOCX/PDF 都不能发布。

### 3.5 Durable dispatch

六类 target：

```text
document_conversion
extraction_target
matching_schedule
matching_job
attachment_preparation
submission_render
```

统一遵循 [`durable-dispatch.md`](durable-dispatch.md)：job只携带target kind/ID/business revision；Oxana原生处理retry/resurrection/dead queue；PostgreSQL只做current/revision fenced publish；不建立reconciler、claim/lease或第二套队列状态机。

## 4. 删除矩阵

删除和新路径必须在同一切片完成，不保留双写。

| 旧内容 | 最终处理 |
| --- | --- |
| 旧 bid 增量 migration、backfill、runtime repair DDL | 删除，使用 fresh baseline |
| commit 后忽略错误的 `enqueue_bid_*` | 改为accepted后返回`202`、unavailable返回可重试`503`/`Err` |
| `system:live-recovery:v1` envelope/claim/handler | 删除 |
| dirty-manifest/orphan-target/orphan-match 泛化 recovery | 删除，不替换为另一套扫描恢复 |
| Bid 对 `oxanus:*`、hostname/PID replay 的 correctness 依赖 | 删除 |
| async base/extensions/head/dispatch intent/state 草稿 | 删除或不实施 |
| delivery attempt/observation/settlement/successor/governor 草稿 | 删除或不实施 |
| delivery generation/next enqueue/claim/lease/heartbeat/reaper | 删除或不实施 |
| 独立 bid-dispatcher service/role/DSN/activation hold 草稿 | 删除或不实施 |
| direct extraction persist façade | 删除，使用 TenderPublication publisher |
| live knowledge/document fallback | 删除，使用 frozen evidence/source artifact |
| 旧 PickSet、quote JSON/float、Markdown export placeholder | 删除，使用最终 typed artifact |
| `content_objects.ref_count` 和 public delete旁路 | 删除，使用 ObjectRegistry/retention |
| endpoint alias、旧 DTO、旧 Web fallback | 删除，使用最终 `/api/v1/bids` contract |
| 只证明旧行为的 test/fixture | 删除或重写为最终合同测试 |

验收使用 `rg` denylist、compiler/link、HTTP contract inventory和 fresh catalog denylist共同证明删除，不能只证明旧代码无人调用。

## 5. 实施顺序

### PR0 — 方案确认

- 固化四份方案文档；
- 明确 Oxana/DB 职责和禁止重复造轮子的规则；
- Markdown、link、重复权威定义检查。

PR0 只修改文档。用户确认前不开始后续代码。

### PR1 — Fresh baseline 与共享平台

- 建立最终 baseline manifest、checksum、roles/ACL；
- 接入 actor/idempotency/audit/ObjectRegistry/retention；
- 删除旧 bid migration、repair DDL和 compatibility view。

验证：空库建立、重复启动只读校验、catalog allow/deny、role allow/deny。

### PR2 — Tender 与 clauses

- 完成 conversion/extraction publication；
- 完成 facts、clauses、KindRouter和 promotion；
- 修复标题、金额、程序编号 splitter等 parser边界。

验证：`tender_publication` 进入 mandatory 活库测试，包含慢转换时Oxana进程heartbeat/resurrection、retryable error交给Oxana以及conversion→extraction原子创建/幂等enqueue。

### PR3 — Matching

- 完成 scope/attestation/evidence/report；
- 完成 target identity/staging/atomic publish和 picks；
- 删除旧 fallback和 rebuild decision。

验证：0 hit/大 scope、混合 route、失败恢复、两个不同 pick 持久化和并发 CAS。

### PR4 — Quote

- 完成 Decimal line editor、snapshot、ceiling和 reopen；
- Web 支持真实金额录入并正确处理 no-ceiling状态。

验证：Rust/SQL/HTTP/Web及非零金额 live flow。

### PR5 — Submission

- 完成 profiles、procedural attachments、parts、manifest、DOCX/PDF；
- 完成 template/procedural router promotion；
- 完成 renderer wrapping、image grammar和正式附件/报价明细。

验证：Gate matrix、attachment preparation fencing、manifest assets、正式 DOCX/PDF content。

### PR6 — API/Web 最终切换

- 只保留最终 DTO/routes/client/workbench；
- 资料编辑期间暂停自动刷新或只在无本地 dirty state时合并服务器更新；
- pick mutation串行或每次使用响应返回的新 revision；
- keyboard E2E 使用真实键盘操作，不以 `.click()` 冒充 keyboard walk。

验证：HTTP contract、Web lint/build、mocked E2E和 live Playwright。

### PR7 — Oxana 稳定运行时

- 锁定 crates.io Oxana 2.1.3 和 `Cargo.lock`；
- worker 固定 `max_retries=3,retry_delay=10s,resurrect=true`；
- 注册一个 `bid:delivery:v1` typed job；
- 使用Oxana dead queue与oxana-web `revive_all_dead`；
- 不读取私有 Redis key，不包装第二套 retry/recovery。

验证：活 Redis retry、delay、Skip、resurrection、dead/`revive_all_dead`和 shutdown cleanup。

### PR8 — 最小业务投递与旧逻辑删除

- 六类 target只保留business generation/revision、result与bounded error；
- 实现幂等target transaction、单次enqueue、accepted/结构化503（稳定target ID+同key重试）和current/revision fenced publish；
- conversion/extraction、matching、attachment/render逐类切换；
- successor transaction后enqueue失败由父Oxana job重试重放；
- 每切一类就在同一改动删除该类旧 enqueue/recovery owner；
- 最后删除全部 Bid live-recovery、due reconciler、claim/lease heartbeat、private Redis correctness和未采用的复杂 dispatch草稿。

验证：[`durable-dispatch.md`](durable-dispatch.md) 第 8 节十项行为全部通过。

### PR9 — Fresh runtime 验收

- 只在干净、已提交且已 push 的候选 SHA 上运行；
- 从空 PostgreSQL/Redis/object volumes启动；
- 执行完整业务流程、关键故障场景、live Playwright和正式 PDF下载；
- 清理并断言所有本轮容器资源零残留；
- 受审后再更新 `phase_1d_runtime_complete=true`。

## 6. 自动验证

### 6.1 常规门禁

至少包含：

```text
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
web lint
web build
web unit/component tests
```

默认 workspace test 可保留无外部服务的测试，但不能把活库测试 skip当作验收通过。下面两个 required job必须显式启动依赖并 fail closed。

### 6.2 `bid-durable-dispatch`

固定启用 PostgreSQL和 Redis：

```text
KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS=1
KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS=1
scripts/bid_durable_dispatch_acceptance.sh
```

入口至少覆盖：

```text
scripts/verify_oxana_registry_source.sh
cargo test --locked -p runtime jobs::tests -- --nocapture
cargo test --locked -p runtime --test work_transport_live -- --nocapture --test-threads=1
cargo test --locked -p bid --test durable_delivery_sql -- --nocapture --test-threads=1
cargo test --locked -p api --test bid_queue_contract -- --nocapture --test-threads=1
cargo test --locked -p worker bid_delivery -- --nocapture
cargo test --locked -p bid --test tender_publication -- --nocapture --test-threads=1
cargo test --locked -p bid --test matching_publication -- --nocapture --test-threads=1
cargo test --locked -p bid --test submission_sql -- --nocapture --test-threads=1
scripts/fresh_schema_acceptance.sh
scripts/bidding_v1_deletion_scan.sh
```

任一数据库/Redis不可用、测试文件不存在、零用例、`SKIP/SKIPPED` 或缺少预期结果都使 job失败。
`bid_queue_contract` 必须覆盖 API enqueue失败后的 `503 QUEUE_UNAVAILABLE`，并证明同一
idempotency key在业务 watermark变化后仍返回首次冻结的 target identity；新 key才可绑定新 target。

### 6.3 `bid-v1-fresh-runtime`

固定入口：

```text
KNOWLEDGEBRAIN_REQUIRE_DOCKER_ACCEPTANCE=1 \
KNOWLEDGEBRAIN_REQUIRE_BROWSER_ACCEPTANCE=1 \
KNOWLEDGEBRAIN_REQUIRE_CLEAN_ACCEPTANCE=1 \
scripts/compose_first_launch_acceptance.sh
```

不接受 dirty checkout、`playwright=not-used`、可选 Docker 或任一 skip。失败现场可以上传，但不能把 required job变绿。

## 7. Fresh runtime 场景

产品流程：

```text
登录
-> 建项并上传真实 PDF/DOCX
-> convert/extract并接受或修订 fact
-> clause确认与两路matching
-> technical 1..N picks
-> 录入非零报价并 finalize/reopen/refinalize
-> 编辑资料、处理程序分段并上传不同 kind附件
-> 先证明 SubmissionGate拒绝
-> 修复后生成并下载正式 DOCX/PDF
```

关键故障只保留七类：

1. retryable handler error由 Oxana 重试；
2. worker process crash后 resurrection；
3. duplicate delivery幂等；
4. stale business revision noop；
5. API enqueue失败返回503，同幂等key重试相同target；
6. successor enqueue response lost/部分失败后只重放相同child job；
7. retry耗尽进入Oxana dead queue并可由oxana-web `revive_all_dead`，且保留原retry count。

每个场景只保存必要证据：输入 target ID/revision、注入点、最终 target状态、有效发布次数、关键 artifact hash和 cleanup结果。不为内部队列 phase建立复杂证据 schema。

## 8. 证据与清理

成功 evidence只需包含：

```text
candidate_git_sha
git_dirty = false
cargo_lock_sha256
schema_manifest_sha256
image_digests[]
required_checks[]
product_flow_result = passed
queue_fault_cases_passed = 7
formal_pdf_sha256
cleanup.containers_remaining = 0
cleanup.volumes_remaining = 0
cleanup.networks_remaining = 0
cleanup.temporary_images_remaining = 0
completion_eligible = true
```

规则：

- evidence字段由实际测试输出产生，不能手工翻转；
- 正式 PDF必须能关联 current manifest、QuoteSnapshotV1、procedural attachments、render assets、ObjectRegistry owner和 audit；
- 测试脚本用 trap cleanup，CI再以 `if: always()`执行一次幂等 cleanup和零残留断言；
- cleanup失败时保留失败证据，但 `completion_eligible=false`；
- 只有证据受审并与候选 SHA双向一致后，才能更新 runtime completion状态。

## 9. 完成声明

分别报告：

```text
implemented
locally verified
committed
pushed
deployed to fresh environment
runtime accepted
```

只有六项都有证据、删除矩阵闭合、两个 required job 对同一候选通过，且 `phase_1d_runtime_complete=true`，才可以声明“招投标最终 V1 完成”。本地 dirty测试、历史 evidence或单个绿色 job都不能提前使用该结论。
