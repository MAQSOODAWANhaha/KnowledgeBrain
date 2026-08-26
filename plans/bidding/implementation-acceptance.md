# 招投标实施、删除矩阵与验收

本文记录最终方案的实现切片、删除矩阵和剩余验收。目标环境始终是 fresh redeploy，不安排兼容阶段。

## 0. 当前证据边界

| 层次 | 当前状态 |
| --- | --- |
| 最终合同 | 已批准并固化为 transactional durable dispatch 实施基线 |
| implemented | 部分；PR1～PR7 产品主链已有实现，durable dispatch 与旧两跳 recovery 删除尚未实施 |
| locally verified | 部分；旧定向证据不能证明新 dispatch 合同，完整门禁与 fresh runtime 需重跑 |
| committed | 否；当前工作树存在未提交变更 |
| pushed | 否 |
| deployed | 否；未部署到 fresh 或生产环境 |
| runtime accepted | 否；当前 checkout 没有完整证据包，`phase_1d_runtime_complete=false` |

`6:quote` 的结构化 DOCX table / PDF grid renderer seam 已通过当前 checkout 的 Rust/SQL/Web 回归；fresh runtime 正式输出仍须另行验收。旧 checkout 或历史隔离运行证据不能提升当前状态。

## 1. 成功标准

实施完成时必须同时成立：

1. 产品/领域文档只承载最终①～⑥目标；
2. 空 PostgreSQL/Redis/object volume 可一次建立系统；
3. 最终 schema 没有旧投标表、旧 view、alias 或兼容列；
4. API/Web/worker 只调用最终接口；
5. 所有被替代的 persist/export/object deletion/best-effort enqueue/live-recovery 路径已删除；
6. 单元、DB、HTTP、浏览器、Compose 和真实运行验收分别通过；
7. 实际生成的正式 PDF 可由 manifest、artifact hash 和 audit 重放证明。

## 2. 最终 baseline 策略

### 2.1 不保留升级叙事

最终代码不再以“0010 publication + 0012 matching，再升级 0013～0018”组织目标。当前 baseline 直接从最终 ERD 建立 fresh schema，旧编号只留在 Git 历史和归档报告。

baseline manifest 的分片顺序、checksum、共享表和角色只由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 定义。本文只定义 `bidding_v1` 业务 slice 及其消费约束。

不可变验收条件是：只有一套可从空库建立最终 catalog 的 baseline manifest；招投标 slice 不先建旧表再 ALTER 到最终形态，不包含 runtime repair DDL 或历史回填，也不改变知识库业务语义。

### 2.2 招投标 repository wiring

当前 repository 按平台 manifest 注册最终 `bidding_v1` slice，并维护：

- bidding schema version/checksum 与 seed contract artifacts/current pointers；
- bidding catalog allowlist/denylist 与受检函数权限声明；
- CI fresh-database、Compose first-launch 和 health/readiness 的招投标断言；
- 旧 bid migrations、runtime repair 和 compatibility view denylist。

平台 runner、角色和启动期 schema identity 规则不在本文复制；招投标应用不得自动补列、建 view、改约束或修数据。

## 3. baseline 逻辑切片

### 3.1 shared platform

shared platform slice 由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 唯一定义，队列 transport 由 [`../platform/queue-runtime.md`](../platform/queue-runtime.md) 定义。招投标 baseline 保存自己的 durable intent 和业务 lease，不复制 actor、idempotency、audit、`ObjectRegistry`、retention 或 Oxana 内部表。

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
- knowledge-owned matching scope attestation ID/hash；
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
- attachment preparation jobs/claims/leases 与冻结 page artifacts；
- BidShot artifacts/current placements；
- template contract artifacts/current pointers；
- part content/dependencies/current pointers/stale；
- manifest/input/render relations/output artifacts/current pointer；
- GateIssue typed projections。

### 3.6 Durable dispatch

- immutable `bid_async_targets` 与六类 typed target exact-one extension；
- immutable `bid_dispatch_intents`、mutable current state、append-only offer/probe attempts 与 durable inbound settlement receipts；
- target kind/id/generation exact relation 与 frozen fence hash；
- dispatch policy snapshots、due partial index、offer claim/lease/receipt；
- conversion/extraction/matching schedule+job/attachment preparation/submission render target adapters；
- minimal `bid-delivery/v1` envelope 与平台 `WorkTransport` adapter；
- target-local business lease repair、terminal settlement 与 bounded retention。

完整状态机、删除范围和故障验收只由 [`durable-dispatch.md`](durable-dispatch.md) 定义。

### 3.7 约束风格

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

### 3.8 招投标 mutation 对共享能力的使用

actor、幂等 identity、receipt、audit envelope 与 heartbeat/lease 豁免只由 [`../platform/runtime-foundation.md`](../platform/runtime-foundation.md) 定义。所有可重试的招投标 user/system mutation 使用这些平台能力，但 operation、payload、锁序、CAS 和 error code 由所属业务模块定义。

首次执行时，领域写、revision/digest、audit、stale/current pointer、durable dispatch intent 与 completed receipt 必须同事务；瞬时错误全回滚。Bootstrap 不能伪造人工 fact/quote/profile/procedural/pick/export 决定。

## 4. 最终删除矩阵

删除动作和新路径必须在同一 PR 完成，不能先保留双写。

| 旧内容 | 最终处理 | 替代 |
| --- | --- | --- |
| 旧 bid 增量 migrations、backfill、runtime repair DDL | 删除 | fresh baseline manifest |
| commit 后 `enqueue_bid_*_with_snapshots` / best-effort enqueue | 删除 | target 与 dispatch intent 同事务 stage |
| `system:live-recovery:v1` 两跳 envelope/handler/claim ledger | 删除 | [`durable-dispatch.md`](durable-dispatch.md) 单跳 final delivery |
| 旧 Bid convert/extract/matching/attachment/render wire DTO 与 task registrations | 删除 | 四条 `bid:delivery:*:v1` task→queue route + `bid-delivery/v1` payload |
| `discover/claim/heartbeat/complete/release/fail` recovery 浅 interface | 删除 | `stage/run` 深 module，状态机留在 implementation |
| dirty-manifest/orphan-target/orphan-match 大 UNION 与泛化 housekeep | 删除 | due-intent index + target-local exact repair |
| `hostname+pid` 私有 Redis processing-list replay | 删除 | 平台 boot instance identity + Oxana resurrection |
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

## 6. 实现切片与验收状态

| 切片 | 实现状态 | 验收状态 |
| --- | --- | --- |
| PR0 | 已批准并固化 | `git diff --check -- plans`、修改文档相对链接检查及 P0/P1 交叉 review 已通过 |
| PR1 | 部分 | dispatch schema/ACL/checksum 落位后重跑 fresh-schema/catalog/seed/ACL |
| PR2～PR3 | 部分 | 产品逻辑已在；conversion/extraction dispatch 替换后重跑 publication/promotion |
| PR4 | 部分 | 产品逻辑已在；schedule/fanout/job dispatch 替换后重跑 matching/lease/pick |
| PR5 | 产品逻辑已在 | quote/HTTP/Web 最终门禁待重跑 |
| PR6 | 部分 | attachment/render dispatch 替换及 fresh runtime 正式输出待验收 |
| PR7 | 产品逻辑已在 | HTTP、Web lint/build、mocked 与 live Playwright 待最终重跑 |
| PR8A | 未完成 | 平台 transport adapter、Oxana boot UUID patch 与 retry/resurrection 合同；不切换业务 owner |
| PR8B | 未完成 | async target/intent/state/attempt schema、dispatch 深 module、policy/ACL；不切换业务 owner |
| PR8C | 未完成 | conversion/extraction 纵切替换并删除该类旧 owner |
| PR8D | 未完成 | attachment preparation/render 纵切替换并删除该类旧 owner |
| PR8E | 未完成 | matching schedule/job/fanout 纵切替换并删除 dirty/orphan recovery |
| PR8F | 未完成 | 全局旧 recovery/housekeep/private-key replay 删除、catalog/ACL/checksum/registry closure 与全量门禁 |
| PR9 | 未完成 | 干净已 push 候选 SHA 的 clean-slate Compose、全量 fault matrix、live Playwright/PDF、retention、强制资源清理、证据绑定与受审计 cutover；`phase_1d_runtime_complete=false` |

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
- 完整 eligible version scope 与有限 hit 集解耦；
- frozen route/requirements/product memberships 与 knowledge scope attestation；
- EvidenceV1、RequirementDecisionV1、MatchingReportV1；
- adapter 内部 Open/Stage/Commit、heartbeat/reaper；
- current projections、RoutePickSet/ProjectPickSet；
- 删除旧 CommitRoute/live fallback/rebuild decision。

验证：65 个 eligible version/0 hit 仍 schedule、保留 65 个 membership 并生成 `NO_EVIDENCE`；报告 exact bytes/aggregation、attestation mismatch、lease/staging/commit、1..N supported picks、普通+unsectioned混合集。

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
- PDF attachment durable preparation：claim/heartbeat/retry/reaper/cancel、page staging 与 atomic publish；
- RequiredPartSet、parts/dependency/stale；
- BidShot/Markdown render assets；
- SubmissionGate/manifest/DOCX/PDF；
- ObjectRegistry consumer cutover，复用平台 retention；
- 删除旧 export/regenerate/refcount/direct delete。

验证：程序 lifecycle、R/S、gate matrix、manifest race、assets/owner-reference 平台集成、DOCX placeholder/PDF reject；PDF preparation incomplete gate、过期 claim/reaper/cancel fencing、连续 page set、publish 失败零 page owner/page row；retention 内部状态机使用平台验收证据。

### PR7 — API/Web 最终切换

范围：

- 最终 DTO/error codes/HTTP routes；
- Web 工作台：文件 -> 事实/条款 -> 匹配/选择 -> 报价 -> ①～⑥ -> 导出；
- 删除 endpoint alias、旧 client family、fallback UI；
- 固定 Playwright runner/fixture。

验证：Rust tests、HTTP contract、Web lint/build、键盘主路径、中文多字节、失败也上传浏览器证据。

### PR8A — 平台 transport 与 Oxana 进程恢复

范围：

- 按 [`../platform/queue-runtime.md`](../platform/queue-runtime.md) 落位 `WorkTransport` production/recording adapter 与 typed outcome；
- vendor 并锁定 Oxana 2.1.x，落位 deterministic transport ID、atomic receipt/membership、fingerprint/phase probe、active-receipt cleanup 与 boot UUID；
- 证明 `max_retries=0` 不阻断 `resurrect=true`，且业务代码不读取 `oxanus:*` 私有 key；
- 本切片不启用 Bid 新 dispatcher，不改变任一业务 target owner。

验证：Oxana upstream 测试、atomic create/dequeue/resurrection/terminal cleanup 故障注入、duplicate-equivalent/identity-conflict、phase probe、超过七天的 active receipt、相同 hostname/PID 不同 boot UUID 恢复和私有 key denylist。

### PR8B — Durable dispatch core 与 baseline

范围：

- 建立 base async target、typed exact-one extension、intent/state/attempt/policy schema、partial index 与 ACL；
- 实现 `stage/run`、PostgreSQL store、offer/probe、consumer begin、target-local repair registry 和 retention；
- 接入四条 task→queue closure，但不切换六类业务 target owner。

验证：base/typed/intent commit/rollback、六类 fence canonical golden、identity conflict、offer/probe/lease/gate/ACL 和空库 catalog。

### PR8C — Conversion/Extraction 纵切

范围：

- 切换 document conversion 和 extraction target adapter；
- conversion settlement 与 converted source + extraction target + intent 同事务；
- 同一改动删除这两类旧 enqueue/recovery/housekeep 分支。

验证：TenderPublication 强制活库、commit/offer/begin/publish fencing、长转换 heartbeat、后继原子性和该 target family 的删除扫描。

### PR8D — Attachment preparation/Render 纵切

范围：

- 切换 PDF attachment preparation 和 submission render target adapter；
- 保持 upload staging、owner transfer、manifest-only render 与 publish fencing；
- 同一改动删除这两类旧 enqueue/recovery/housekeep 分支。

验证：preparation/render claim/heartbeat/retry/reap/cancel、staging abandon、publish 原子性、DOCX/PDF gate 与该 target family 的删除扫描。

### PR8E — Matching schedule/job 纵切

范围：

- 切换 matching schedule 和 matching job target adapter；
- schedule settlement 原子产生 manifest、0..N jobs 与等量 intents；
- 同一改动删除 dirty-manifest、orphan-target/orphan-match 和旧 matching recovery owner。

验证：schedule/fanout 原子性、lease/staging/commit、65 eligible/0 hit、普通+unsectioned 混合、持久化 1..N picks 和该 target family 删除扫描。

### PR8F — 单 owner closure 与全量门禁

范围：

- 删除剩余两跳 live recovery、best-effort enqueue、业务 housekeep、旧 wire DTO/registry 和私有 Redis replay；
- 重生 baseline checksum、catalog/ACL allowlist/denylist 和 queue/producer/handler registry closure；
- 从 compiler/link、`rg`、catalog 和 runtime registry 四个角度证明任一 target 只有新 dispatcher owner。

验证：完成 [`durable-dispatch.md`](durable-dispatch.md) 第 11.1～11.4 节、平台 queue-runtime 本地合同、第 7.2 节固定强制活库 job 和删除扫描。PR8A/PR8B 不得启用第二 owner；PR8C～PR8E 不得让同一 target family 新旧双跑。

### PR9 — clean-slate 部署与运行验收

范围：

- 在已 commit、已 push、`git_dirty=false` 的候选 SHA 上从空 volumes 执行 Compose first launch；
- health/readiness、queue/worker/docreader、对象存储；
- 通过 live Playwright 使用真实 PDF/DOCX 样本完成①～⑥、DOCX warning、修复 gate、PDF 成功；
- 完成第 8.2 节六类 target × commit/offer/begin/publish 的故障矩阵，另行覆盖 Redis volume loss、worker process crash、alive-but-stuck lease 和 retention retry；
- 记录并机器校验候选 SHA、image/source revision digest、schema manifest digest、fixture/report/artifact hashes、no-skip 和资源清理结果；
- 证据通过 review 后由受审计 cutover 改动绑定 runtime-completion hashes 并显式翻转 `phase_1d_runtime_complete=true`；验收脚本自身不得修改库内 completion 真源。

验证：完成 [`durable-dispatch.md`](durable-dispatch.md) 第 11.5 节、平台 queue-runtime fresh 验收和第 7.3 节固定 required job；不以 mock、dirty checkout、`playwright=not-used`、本地单测或旧环境升级代替真实 fresh runtime。任一证据、矩阵 case 或 cleanup receipt 缺失均使 required job 失败。

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

仓库已提供以下专项验收入口；是否通过只以本次实际命令输出和证据包为准：

```text
scripts/fresh_schema_acceptance.sh
scripts/bid_e2e_smoke.sh
scripts/bidding_v1_deletion_scan.sh
npm --prefix web run test:e2e
scripts/compose_first_launch_acceptance.sh
```

上述入口是合同名，不得用另一个绿色 job、mock 或手工截图代替。直接 `cargo test --workspace` 在无库环境的结果只属于常规 Rust 门禁，不能代替强制活库验收。

### 7.2 PR8 required job：`bid-durable-dispatch`

当前 CI 的强制活库 job 名为 `bid-smoke`；`bid-durable-dispatch` 是本方案的目标 required check，不是当前已落位事实。PR8A 必须新增该 job，或把现有 `bid-smoke` 显式改名并同步更新 branch protection/required-check 聚合；旧 job 不得继续作为可替代的绿色旁路。目标 job 使用独立 PostgreSQL 16 和 Redis 7，固定：

```text
KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS=1
KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS=1
scripts/bid_durable_dispatch_acceptance.sh
```

PR8A 新增 `scripts/bid_durable_dispatch_acceptance.sh` 并纳入 Oxana/runtime transport 命令；PR8B 在启用任一 Bid owner 前纳入 dispatch SQL/worker 命令；PR8C～PR8E 在各自纵切中纳入对应领域命令；PR8F 使用下列完整固定清单。未来切片的 test target 不必在更早 PR 用占位测试伪造通过，但启用某 target owner 的 PR 必须先将它的真实合同加入 required job。该入口顺序执行且任一失败立即失败：

```text
cargo test --manifest-path vendor/oxana/Cargo.toml --all-targets
cargo test -p runtime jobs::tests -- --nocapture
cargo test -p bid --test durable_dispatch_sql -- --nocapture --test-threads=1
cargo test -p worker --test durable_dispatch_worker -- --nocapture --test-threads=1
cargo test -p bid --test tender_publication -- --nocapture --test-threads=1
cargo test -p bid --test knowledge_retrieval_selection -- --nocapture --test-threads=1
cargo test -p bid --test matching_publication -- --nocapture --test-threads=1
cargo test -p bid --test submission_sql -- --nocapture --test-threads=1
cargo test -p api --test submission_contract -- --nocapture --test-threads=1
scripts/fresh_schema_acceptance.sh
scripts/bidding_v1_deletion_scan.sh
```

`durable_dispatch_sql` 和 `durable_dispatch_worker` 是 PR8B 起固定的新合同 target；PR8B 及后续切片中对应文件不存在时 required job 必须保持红色。所有活库测试必须在连接、schema 或 Redis 不可用时 fail closed；任一 `SKIP`、`SKIPPED`、`skipped live`、零用例匹配或缺少预期 contract ID 均使 job 失败。无 `continue-on-error`、无 optional service、无本地环境自动降级。

### 7.3 PR9 required job：`bid-v1-fresh-runtime`

`bid-v1-fresh-runtime` 只在已 push 的干净候选 SHA 上运行，固定入口为：

```text
KNOWLEDGEBRAIN_REQUIRE_DOCKER_ACCEPTANCE=1 \
KNOWLEDGEBRAIN_REQUIRE_BROWSER_ACCEPTANCE=1 \
KNOWLEDGEBRAIN_REQUIRE_CLEAN_ACCEPTANCE=1 \
scripts/compose_first_launch_acceptance.sh
```

job 必须从空 PostgreSQL/Redis/object volumes 启动，执行 live Playwright 和第 8.2 节 fault matrix，不接受 `playwright=not-used`、dirty diff、未追踪源文件、可选 Docker 或任一 skip path。证据上传步骤使用 `if: always()`，但 `if-no-files-found: error`；验收脚本失败与证据上传状态分开保留，不得因成功上传失败报告而把 required job 变绿。

### 7.4 schema/ACL

- 空库 manifest/checksum/extension/seed；
- catalog allowlist 与旧 table/view/column denylist；
- API/worker/retention/migration role allow-deny；
- immutable triggers、current pointer、deferred verifiers；
- 运行时启动不执行 DDL。

### 7.5 领域 fixtures

- Tender：span、router golden、publication/fact/clause promotion；
- Matching：eligible scope/hit quota 解耦、knowledge attestation、decision/report/evidence canonical、stage/commit、picks/R/S；
- Quote：tax/rounding/overflow/ceiling/snapshot/reopen；
- Submission：profiles/procedural/attachment preparation/parts/stale/gate/assets/owner-reference 集成；
- Cross-cutting：actor/idempotency/audit/ended/maintenance/race、target+intent 原子性、offer/lease/gate fencing、Redis 丢失恢复与 retry 所有权。

### 7.6 HTTP/Web

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
- 每次运行使用唯一、有界 `COMPOSE_PROJECT_NAME`，不复用开发或上一轮验收容器、network 或 volume；
- 使用固定 image digests；
- 空 PostgreSQL/Redis/object store；
- 不挂旧 migration state 或旧对象；
- 记录 Compose 配置 digest 与全部环境开关（不记录 secret value）；
- 成功、失败、取消和 signal 路径均执行 `down --volumes --remove-orphans`，移除本轮临时构建 image，并在返回前证明该 project 无残留 container/volume/network；cleanup 失败不得被原始成功状态覆盖。

### 8.2 场景

1. first launch 建 schema、seed、roles，全部服务 ready；
2. 真实 PDF/DOCX 招标文件 convert/extract/publish；
3. 两路真实知识检索并冻结 evidence；
4. 人工 picks、quote、profiles、程序附件，并等待 PDF 附件 durable preparation 完成；
5. 先证明 Gate 拒绝，再修复并出 PDF；
6. 按 [`durable-dispatch.md`](durable-dispatch.md) 完成六类 target × `commit|offer|begin|publish` 的 24 个必选故障 case，每个 case 证明最终收敛、最多一次有效发布、无孤儿 target/intent/artifact/object owner；
7. 删除 live knowledge document 后历史 report/PDF 可审计重放；
8. attachment 业务引用释放后，已冻结的 manifest owner 仍保持对象可重放；无 owner 的 staging 引用释放后由 retention 可恢复删除；
9. project ended 后新 publication/export 稳定拒绝；
10. 额外独立 case 覆盖整个 Redis volume 丢失、worker process crash、process-alive/task-stuck、DB 连接丢失、render failure 与 retention retry。其中 Redis volume loss 使用独立 Compose project，不污染其余 case。

24 个核心 case 的 `target_kind` 必须精确覆盖：

```text
document_conversion
extraction_target
matching_schedule
matching_job
attachment_preparation
submission_render
```

每个 case 使用独立 project/target/dispatch identity，执行前证明没有同 identity 的历史 target、intent 或 transport receipt。每行保存以下固定 schema：

```text
case_id
target_kind
fault_point = commit|offer|begin|publish
fixture_sha256
project_id, target_id, dispatch_id, generation, offer
injection_trigger, injection_observed_at
expected_intermediate_target_status
expected_intermediate_dispatch_status
convergence_deadline_ms
final_target_status
final_dispatch_status
effective_publish_count
orphan_target_count
orphan_intent_count
orphan_artifact_count
orphan_object_owner_count
attempt_terminal_codes[]
evidence_refs[]
result = passed|failed
```

`commit` case 要求结果只能是全回滚或 target+typed extension+intent 全部可见；`offer` 固定注入 Redis accepted 但 DB receipt 未 settle；`begin` 固定注入 delivery 已取得但 business claim 未完成；`publish` 固定注入外部/staging 产物已形成但 fenced terminal transaction 未确认。不允许 `skipped`、`inconclusive` 或人工填写 `passed`；缺少任一 target/fault-point 组合、超过收敛时限或 count 不为预期值时 job 失败。

### 8.3 证据包

required job 必须产生唯一 machine-readable `evidence.json`，至少使用以下固定 schema：

```text
schema_version
candidate.git_sha
candidate.git_dirty = false
candidate.git_diff_sha256
candidate.pushed_ref
required_check.name = bid-v1-fresh-runtime
required_check.run_url
runtime.compose_config_sha256
runtime.schema_manifest_sha256
runtime.image_digests[]
runtime.image_source_revisions[]
execution.runtime = compose-live-api-worker-retention-docreader
execution.docreader = real-grpc
execution.playwright = live-ui
execution.skip_count = 0
fresh_schema_acl_report_sha256
fixture_sha256
browser_report_sha256
fault_matrix.core_case_count = 24
fault_matrix.core_passed_count = 24
fault_matrix.core_failed_count = 0
fault_matrix.sha256
recovery_scenarios.required_count = 6
recovery_scenarios.passed_count = 6
recovery_scenarios.failed_count = 0
recovery_scenarios.sha256
artifacts.job_attempt_ids[]
artifacts.report_quote_manifest_output_ids[]
artifacts.report_quote_manifest_output_sha256[]
artifacts.pdf_sha256
artifacts.pdf_download_metadata_sha256
logs_traces_sha256
cleanup.project_name
cleanup.containers_remaining = 0
cleanup.volumes_remaining = 0
cleanup.networks_remaining = 0
cleanup.temporary_images_remaining = 0
cleanup.result = passed
completion_eligible = true
```

所有 image source revision 必须是已提交、已 push 且可追溯到 candidate SHA 的固定身份；`git_dirty=true`、`playwright=not-used`、任一 skip、证据文件缺失或 artifact 上传 `warn` 均不能产生 `completion_eligible=true`。敏感字段只记录脱敏 digest、ID 或 bounded code，不保存 secret value、文档全文或未脱敏 trace。

cleanup 必须在 success/failure/cancel 路径运行，证据上传用 `if: always()` 保留失败现场；上传不取代 cleanup，cleanup 失败必须使 required job 失败。

`evidence.json` 是 runtime acceptance 原始证据，不得由脚本直接翻转库内 completion 真源。证据受审后，单独的 cutover receipt 必须记录 evidence digest、reviewer/approval identity、绑定的 registry/evaluation/readiness/image/topology hashes 和 `phase_1d_runtime_complete=true` 的受审计改动。只有原始证据与 cutover receipt 可双向校验时，才能把 runtime accepted 置为是。

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

只有六项全部有证据、删除矩阵闭合、`bid-durable-dispatch` 与 `bid-v1-fresh-runtime` 两个 required check 对同一可追溯候选系列通过，且受审计 cutover receipt 证明 `phase_1d_runtime_complete=true`，才声明“招投标最终 V1 完成”。PR0 文档完成、可跳过的测试绿色、dirty checkout 证据或本地测试全绿都不能提前使用该结论。
