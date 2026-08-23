# 招投标实施、删除矩阵与验收

本文把最终方案转成可执行 PR0～PR8。它假设 fresh redeploy，不安排兼容阶段。

## 1. 成功标准

实施完成时必须同时成立：

1. 产品/领域文档只承载最终①～⑥目标；
2. 空 PostgreSQL/Redis/object volume 可一次建立系统；
3. 最终 schema 没有旧投标表、旧 view、alias 或兼容列；
4. API/Web/worker 只调用最终接口；
5. 所有被替代的 persist/export/object deletion 路径已删除；
6. 单元、DB、HTTP、浏览器、Compose 和真实运行验收分别通过；
7. 实际生成的正式 PDF 可由 manifest、artifact hash 和 audit 重放证明。

## 2. 最终 baseline 策略

### 2.1 不保留升级叙事

最终代码不得继续以“0010 publication + 0012 matching，再升级 0013～0018”组织目标。实施 PR1 从最终 ERD 重建 fresh schema，旧编号只留在 Git 历史/归档报告。

baseline manifest 的分片顺序、checksum、共享表和角色只由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 定义。本文只定义 `bidding_v1` 业务 slice 及其消费约束。

不可变验收条件是：只有一套可从空库建立最终 catalog 的 baseline manifest；招投标 slice 不先建旧表再 ALTER 到最终形态，不包含 runtime repair DDL 或历史回填，也不改变知识库业务语义。

### 2.2 招投标 repository wiring

PR1 按平台 manifest 注册最终 `bidding_v1` slice，并同步更新：

- bidding schema version/checksum 与 seed contract artifacts/current pointers；
- bidding catalog allowlist/denylist 与受检函数权限声明；
- CI fresh-database、Compose first-launch 和 health/readiness 的招投标断言；
- 旧 bid migrations、runtime repair 和 compatibility view denylist。

平台 runner、角色和启动期 schema identity 规则不在本文复制；招投标应用不得自动补列、建 view、改约束或修数据。

## 3. baseline 逻辑切片

### 3.1 shared platform

shared platform slice 由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 唯一定义。招投标 baseline 只保存受检平台引用和业务元数据，不复制 actor、idempotency、audit、`ObjectRegistry`、retention 或 queue 内部表。

### 3.2 TenderPublication / ClauseLifecycle

- bid projects/documents/source/section artifacts；
- extraction targets/runs/claims/candidates/dispositions/publication states；
- fact suggestion candidates、decision ledger、current pending view；
- project facts/FactIdentity；
- clauses、origin/current SourceSpan、kind/family/status；
- KindRouter artifacts/current/promotion markers；
- matching/service/pricing/payment/delivery/evaluation/procedural identities。

### 3.3 MatchingPublication

- manifests/routes/route memberships/requirements/product-version artifacts；
- jobs/attempts/claims/lease；
- adapter staging set + typed collections；
- immutable source/evidence/candidate/group/decision/report artifacts；
- current technical/commercial projections；
- RoutePickSet/ProjectPickSet revisions/artifacts/current pointers。

### 3.4 Quote

- quote root/revisions/lines；
- snapshots、canonical payload/hash/provenance；
- active/draft pointers；
- eligibility transition functions。

### 3.5 Submission

- company/submission profile artifacts/current pointers；
- procedural segments/classifications/decisions/attachments；
- BidShot artifacts/current placements；
- template contract artifacts/current pointers；
- part content/dependencies/current pointers/stale；
- manifest/input/render relations/output artifacts/current pointer；
- GateIssue typed projections。

### 3.6 约束风格

最终 baseline 必须用 CHECK、UNIQUE、composite FK、immutable trigger、deferred verifier 和受检 SECURITY DEFINER function 把以下规则放入可信边界：

- scope/project/generation 一致；
- kind/family 与 typed NULL matrix；
- revision/digest 成对；
- current pointer 所属与状态；
- immutable artifact；
- successor XOR terminal；
- report/decision/payload 集合等价；
- quote totals/ceiling/eligibility；
- RequiredPartSet/dependency/manifest/render relation 等价；
- 业务 object reference 与平台 [`ObjectRegistry` 契约](../platform/runtime-foundation.md) 等价。

### 3.7 招投标 mutation 对共享能力的使用

actor、幂等 identity、receipt、audit envelope 与 heartbeat/lease 豁免只由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 定义。所有可重试的招投标 user/system mutation 使用这些平台能力，但 operation、payload、锁序、CAS 和 error code 由所属业务模块定义。

首次执行时，领域写、revision/digest、audit、stale/current pointer 与 completed receipt 必须同事务；瞬时错误全回滚。Bootstrap 不能伪造人工 fact/quote/profile/procedural/pick/export 决定。

## 4. 最终删除矩阵

删除动作和新路径必须在同一 PR 完成，不能先保留双写。

| 旧内容 | 最终处理 | 替代 |
| --- | --- | --- |
| 旧 bid 增量 migrations、backfill、runtime repair DDL | 删除 | fresh baseline manifest |
| `persist_extraction_report` / `persist_section_retry` 生产直写 | 删除/私有测试 helper 也不保留同名 public seam | `TenderPublication` 单一 publisher |
| heuristic 同时写 candidate/current domain | 删除 | candidate -> verified atomic publication |
| 旧 `family` client 写入与 family-only clause DTO | 删除 | client 提交 kind，服务端派生 family |
| confirmed clause 原地跨 kind | 删除 | 先 unconfirm OLD，再 draft/reconfirm NEW |
| promotion 对 manual/manual_after_edit 自动改 kind | 删除 | durable actor 显式 PATCH |
| revision-only/含义不明 hash | 删除 | typed canonical identity + SHA-256 |
| 单 JSON `CommitRoute` / `CommitRouteV1` | 删除 | adapter 内部 Open/Stage/Commit V2 |
| storage 从 candidate `.find()` 重建 decision | 删除 | worker typed decisions 原样冻结 + verifier |
| live document/chunk/evidence fallback | 删除 | frozen source chunk artifact + EvidenceV1 |
| technical view 只返回 selected/recommended | 删除 | 全部 supported + recommended 标记 |
| 含糊单一 `PickSet` | 删除 | RoutePickSetV1 + ProjectPickSetV1 |
| 把整个 unsectioned PickSet 强制 nil | 删除 | `R` 对应精确子集 `S` verifier |
| 旧报价 JSON/float/自动正式价格 | 删除 | CNY Decimal draft + QuoteSnapshotV1 |
| ceiling 默认当含税或未税 | 删除 | required `ceiling_basis`，unspecified 阻断 finalize |
| live quote draft 进入 letter/quote | 删除 | 同一 active eligible snapshot/NULL DOCX placeholder |
| 只生成①～⑤或⑥ Markdown 占位 | 删除 | RequiredPartSet ①～⑥完整 parts |
| 旧 booklet part key/client family | 删除 | typed part keys + template slots |
| `export` / `regenerate_stale` 无 CAS API | 删除 | manifest + dependency/current CAS |
| renderer 查询 live bid shots/Markdown/knowledge data | 删除 | manifest-owned render relations |
| `content_objects.ref_count` | 删除，不留 view/alias | shared ObjectRegistry + references |
| public `bump/release/delete_object/drop_blob` | 删除或完全私有 adapter | 受检 owner references + retention outbox |
| 普通 API/worker 直接物理删对象 | 删除 | 独立 retention role/consumer |
| 旧 current views/compatibility views | 删除 | 最终 typed current projections |
| 旧 endpoint alias、旧 request/response DTO | 删除 | 最终 `/api/v1/bids` contract family |
| 旧 Web API client、旧页面状态与 fallback | 删除 | 新模块化 client/workbench |
| 只证明旧行为的 tests/fixtures | 删除或重写 | 最终 contract/golden/E2E fixtures |

验收以 `rg` denylist + compiler/link + API contract inventory + catalog denylist 四种证据共同证明，不以“新路径没人调用旧代码”代替删除。

## 5. 安全前置

在任何共享环境开放前，代码内必须闭合：

- object key 与物理删除安全由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 验收；
- 上传 MIME/魔数、大小、像素、压缩炸弹、解析超时；
- LDAP TLS/fail-closed、JWT/API key 校验；
- Bootstrap actor 不能执行 fact/quote/profile/procedural/export mutation；
- API/worker 最小 DB grants；retention 独立 login；
- secrets 不进日志/audit/receipt；
- 正式输出文件名/header/content disposition 注入防护；
- parser/renderer 只读 manifest allowlisted assets。

运维 DPA、数据驻留、备份、凭据轮换等由部署 checklist 签署，和代码自动测试分别报告。

## 6. PR0～PR8

### PR0 — 文档与目标冻结

范围：

- PRODUCT 正式承诺①～⑥；
- 分域 docs/plans、术语和端口；
- 五模块、clean-slate、时区、ceiling basis、PickSet 拆分；
- 删除矩阵与验收口径。

验证：Markdown/link/重复权威定义检查；无代码改动。

### PR1 — fresh baseline 与共享平台边界

范围：

- 按平台方案注册最终 `bidding_v1` baseline slice；
- 接入 actor/idempotency/audit/ObjectRegistry/maintenance contracts；
- 声明招投标 roles/ACL、seed artifacts/current pointers；
- 删除旧 bid migrations/runtime repairs/compatibility views。

验证：与 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 联合完成纯空库、checksum ledger、catalog allow/deny、role allow/deny；招投标应用重复启动只验证不改 schema。

### PR2 — TenderPublication

范围：

- BidDocument/converted source/section/SourceSpanV2；
- bounded agents/candidate graph/dispositions；
- target/claim/generation fencing 与 atomic section publication；
- 删除 direct persist façade。

验证：中文 byte span、scope FK、publication CAS、并发/retry/ended、部分失败零可见。

### PR3 — ClauseLifecycle 与 facts

范围：

- kind/family、manual API、clause sets；
- KindRouter artifact/promotion/marker/reconfirm；
- fact suggestion current/history、accept/reject/set/clear；
- FactIdentity、ceiling identity/basis；
- matching/quote/submission stale seams。

验证：promotion generation 2/3、eligible-only、manual 人工边界、OLD/NEW membership、fact/publisher 竞态、幂等。

### PR4 — MatchingPublication

范围：

- KnowledgeRetrievalPort 两路 adapter；
- frozen route/requirements/product memberships；
- EvidenceV1、RequirementDecisionV1、MatchingReportV1；
- adapter 内部 Open/Stage/Commit、heartbeat/reaper；
- current projections、RoutePickSet/ProjectPickSet；
- 删除旧 CommitRoute/live fallback/rebuild decision。

验证：报告 exact bytes/aggregation、lease/staging/commit、1..N supported picks、普通+unsectioned混合集。

### PR5 — Quote

范围：

- draft/lines/Decimal calculations；
- ceiling basis；
- QuoteSnapshotV1/finalize/eligibility/reopen；
- quote UI/API；
- 删除旧报价格式与 live draft 正式读取。

验证：formula/bounds/overflow、canonical bytes、ceiling matrix、pointer/concurrency、reopen。

### PR6 — Submission

范围：

- profiles、procedural segment/router/classification/decision/attachment；
- RequiredPartSet、parts/dependency/stale；
- BidShot/Markdown render assets；
- SubmissionGate/manifest/DOCX/PDF；
- ObjectRegistry consumer cutover，复用平台 retention；
- 删除旧 export/regenerate/refcount/direct delete。

验证：程序 lifecycle、R/S、gate matrix、manifest race、assets/owner-reference 平台集成、DOCX placeholder/PDF reject；retention 内部状态机使用平台验收证据。

### PR7 — API/Web 最终切换

范围：

- 最终 DTO/error codes/HTTP routes；
- Web 工作台：文件 -> 事实/条款 -> 匹配/选择 -> 报价 -> ①～⑥ -> 导出；
- 删除 endpoint alias、旧 client family、fallback UI；
- 固定 Playwright runner/fixture。

验证：Rust tests、HTTP contract、Web lint/build、键盘主路径、中文多字节、失败也上传浏览器证据。

### PR8 — clean-slate 部署与运行验收

范围：

- 空 volumes Compose first launch；
- health/readiness、queue/worker/docreader、对象存储；
- 真实样本完成①～⑥、DOCX warning、修复 gate、PDF 成功；
- 进程重启、job reclaim、render failure、retention retry；
- 记录实际 checkout SHA、image digest、schema manifest digest、fixture/report/artifact hashes。

验证：不以 mock、本地单测或旧环境升级代替真实 fresh runtime。

## 7. 自动验证矩阵

### 7.1 常规门禁

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm ci --prefix web
npm --prefix web run lint
npm --prefix web run build
```

实际命令以仓库脚本为准；计划实施时补齐：

```text
scripts/fresh_schema_acceptance.sh
scripts/bid_e2e_smoke.sh
npm --prefix web run test:e2e
scripts/compose_first_launch_acceptance.sh
```

### 7.2 schema/ACL

- 空库 manifest/checksum/extension/seed；
- catalog allowlist 与旧 table/view/column denylist；
- API/worker/retention/migration role allow-deny；
- immutable triggers、current pointer、deferred verifiers；
- 运行时启动不执行 DDL。

### 7.3 领域 fixtures

- Tender：span、router golden、publication/fact/clause promotion；
- Matching：decision/report/evidence canonical、stage/commit、picks/R/S；
- Quote：tax/rounding/overflow/ceiling/snapshot/reopen；
- Submission：profiles/procedural/parts/stale/gate/assets/owner-reference 集成；
- Cross-cutting：actor/idempotency/audit/ended/maintenance/race。

### 7.4 HTTP/Web

固定浏览器流程至少完成：

```text
登录
-> 建项/上传
-> 接受并人工修订 fact
-> clause create/PATCH/confirm
-> 两路匹配三态与 technical 1..N pick
-> 无 quote 的 DOCX placeholder/warning
-> quote draft/finalize/reopen/refinalize
-> profile/procedural resolution/attachment
-> 有效期冲突与 KindRouter marker 的 PDF 拒绝
-> 修复后生成并下载正式 PDF
```

覆盖键盘主路径、中文、多个 candidates、stale、并发冲突。Playwright report、fixture digest、screenshots 以及失败时 video/trace 都必须保存；缺证据即 job 失败。

## 8. fresh runtime 验收

### 8.1 环境

- 删除并重新创建专用验收 volumes；
- 使用固定 image digests；
- 空 PostgreSQL/Redis/object store；
- 不挂旧 migration state 或旧对象；
- 记录 Compose 配置 digest 与全部环境开关（不记录 secret value）。

### 8.2 场景

1. first launch 建 schema、seed、roles，全部服务 ready；
2. 真实 PDF/DOCX 招标文件 convert/extract/publish；
3. 两路真实知识检索并冻结 evidence；
4. 人工 picks、quote、profiles、程序附件；
5. 先证明 Gate 拒绝，再修复并出 PDF；
6. 重启 worker，证明 active claim/staging 可恢复；
7. 删除 live knowledge document 后历史 report/PDF 可审计重放；
8. attachment/manifest reference 存在时对象不能删，释放后 retention 可恢复删除；
9. project ended 后新 publication/export 稳定拒绝。

### 8.3 证据包

保存：

- git SHA、镜像 digest、schema manifest digest；
- fresh schema/ACL machine-readable report；
- E2E fixture digest 与浏览器报告；
- job/attempt/report/quote/manifest/output artifact IDs/hashes；
- PDF hash 与下载响应元数据；
- 关键 logs/traces（脱敏）；
- 失败恢复步骤和最终状态。

## 9. 完成声明

分别报告：

```text
implemented
locally verified
committed
deployed to fresh environment
runtime accepted
```

只有五项全部有证据且删除矩阵闭合，才声明“招投标最终 V1 完成”。PR0 文档完成或本地测试全绿都不能提前使用该结论。
