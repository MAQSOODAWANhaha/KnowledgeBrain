# 招投标实施、删除矩阵与验收

本文记录最终方案的实现切片、删除矩阵和剩余验收。目标环境始终是 fresh redeploy，不安排兼容阶段。

## 0. 当前证据边界

| 层次 | 当前状态 |
| --- | --- |
| 最终合同 | 产品基线与 stable durable-dispatch 修订已批准并固化；PR8A transport seam 已实现并本地验收，PR8B～PR9 尚未完成 |
| implemented | 部分；PR1～PR7 产品主链与 PR8A stable transport 已实现，PR8B～PR8F durable owner 纵切及旧两跳 recovery 删除尚未完成 |
| locally verified | 部分；PR8A mandatory transport acceptance 的 14 个 contract ID、六模式 cleanup、资源零残留及 Standards/Spec 双轴复审已通过；PR8B～PR9、全量产品门禁与 fresh runtime 仍须实施或重跑 |
| committed | 部分；PR8A 已由本地提交 `c825040` 收口，PR8B～PR9 尚未实施或提交 |
| pushed | 部分；`ee8b492` 已在 `origin/main`，方案固化与 PR8A 提交仅保留本地，当前 `main` ahead 6、未 push |
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
- 每 target 一条 mutable current head、0..N immutable `bid_dispatch_intents` 与每 dispatch 一条受检 state；
- 每 dispatch 至多一次 delivery attempt、每 target 有界 ordinal business attempt/唯一 owner lease、bounded late observation、immutable disposition settlement、inbound outcome、repair obligation、rejected delivery、typed evidence 与跨generation governor；
- target kind/id/generation exact relation 与 frozen fence hash；
- dispatch semantics snapshots、ready/offer lease/delivery-start/business lease due partial index 与 shared governor；
- conversion/extraction/matching schedule+job/attachment preparation/submission render target adapters；
- minimal `BidDeliveryV1Job` 与平台 stable `WorkTransport prepare/offer` adapter；
- owner-only SQL `stage/replace_current_target/cancel_target` 由所属 SECURITY DEFINER domain mutation 在同一transaction直接调用；Rust不得直调internal entry，只在确有Rust mutation调用方时以`pub(crate)` adapter包住完整domain mutation，并为run/handle提供private store seam；
- 独立 `kb_runtime_bid_dispatcher` login role/`BID_DISPATCH_DATABASE_URL` 只拥有 `run` 背后的 bounded dispatch grants，`kb_runtime_worker` 只拥有 `handle` grants；
- 每次 offer identity 只调用一次 Oxana；任一未知结果只创建新 `dispatch_id` successor，不重用旧 ID；
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
| 旧 Bid convert/extract/matching/attachment/render wire DTO 与 task registrations | 删除 | 一个显式 `bid:delivery:v1` typed job + allowlisted lane mapping |
| `discover/claim/heartbeat/complete/release/fail` recovery 浅 interface | 删除 | `stage/replace/cancel/run/handle` 深 module，状态机留在 implementation |
| dirty-manifest/orphan-target/orphan-match 大 UNION 与泛化 housekeep | 删除 | due-intent index + target-local exact repair |
| Bid 对 `hostname+pid` 私有 Redis replay/live-recovery 的 correctness 依赖 | 删除 | DB delivery-start deadline + 新 dispatch ID；共享 replay 当前可能触碰 Bid membership，但新路径不调用/不依赖，按平台独立 cutover 删除 |
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
| PR0 | 产品基线与 stable durable-dispatch 修订已批准并固化 | 2026-08-26 `diff --check`、relative links、Markdown tables通过；DB/并发、transport、跨文档三轴复审均P0/P1/P2=0 |
| PR1 | 部分 | dispatch schema/ACL/checksum 落位后重跑 fresh-schema/catalog/seed/ACL |
| PR2～PR3 | 部分 | 产品逻辑已在；conversion/extraction dispatch 替换后重跑 publication/promotion |
| PR4 | 部分 | 产品逻辑已在；schedule/fanout/job dispatch 替换后重跑 matching/lease/pick |
| PR5 | 产品逻辑已在 | quote/HTTP/Web 最终门禁待重跑 |
| PR6 | 部分 | attachment/render dispatch 替换及 fresh runtime 正式输出待验收 |
| PR7 | 产品逻辑已在 | HTTP、Web lint/build、mocked 与 live Playwright 待最终重跑 |
| PR8A | 已实现并本地提交（`c825040`） | 2026-08-26：registry Oxana 2.1.3 verifier、pure 8/8、registry negative 1/1、runtime 44/44、live Redis 4/4、14 个 required contract ID、六模式 cleanup 与零残留通过；`cargo check`/Clippy 0 error，Standards/Spec 均 P0/P1/P2/Judgement=0；未切换业务 owner，未 push、未部署、未作 runtime accepted 声明 |
| PR8B | 未完成 | dormant async target/空 conversion target/head/0..N intent/state、delivery/business attempts、observations、settlement/inbound、repair obligation、rejected delivery、typed evidence、semantics/governor catalog、owner-only SQL + 完整domain mutation wrapper/run-handle store seam、独立dispatcher role/DSN与policy/ACL；不注册真实adapter、不启动dispatcher或业务owner |
| PR8C | 未完成 | conversion/extraction 纵切替换并删除该类旧 owner |
| PR8D | 未完成 | attachment preparation/render 纵切替换并删除该类旧 owner |
| PR8E | 未完成 | matching schedule/job/fanout 纵切替换并删除 dirty/orphan recovery |
| PR8F | 未完成 | Bid 剩余旧 recovery/housekeep 删除、Bid/WorkTransport private-key denylist、catalog/ACL/checksum/registry closure 与全量门禁 |
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

### PR8A — 发布版 Oxana transport seam

范围：

- 精确锁定 crates.io `oxana`、`oxana-macros`、`oxana-web` 为 2.1.3，禁止 vendor、path dependency 与 `[patch.crates-io]`；
- 按 [`../platform/queue-runtime.md`](../platform/queue-runtime.md) 落位 pure deterministic `prepare`、最薄 production/recording `offer` adapter 与 `returned|indeterminate|returned_job_id_mismatch(actual_job_id)` outcome；publisher 返回的不匹配 actual ID必须在 transport outcome 处bounded归档，领域按indeterminate observation结算并使readiness fail closed。consumer 的 `ObservedBidDeliveryV1.observed_job_id` 则必须原样复制公开 `JobContext.meta.id`，进入 PR8B rejected-delivery 持久化边界时再按该表的bounded合同归档，不能在worker seam提前截断；
- `BidDeliveryV1Job` 显式固定 `Job::name`、unique ID、`on_conflict=Skip`、`resurrect=true`；worker 固定 `max_retries=0`；
- 证明一次 `offer` adapter invocation 最多一次 `Storage::enqueue`：未取消且 deadline 有效的正常路径恰好一次，deadline 已过/首次 poll 前取消允许零次；任何路径都不在 adapter 内部 retry/probe/delete。跨 invocation 的 once-per-dispatch 留给 PR8B；
- Bid/WorkTransport 不读取 `oxanus:*`、不使用 `get_job/list/stats/delete_job` 作 correctness。共享 `replay_orphaned_local_jobs` 当前无领域过滤，可能触碰 Bid membership，但新路径不主动调用、不依赖其结果，也不把它作为 Bid owner 或验收证据；
- 本切片不启用 Bid 新 dispatcher，不改变任一业务 target owner。

新增 `scripts/verify_oxana_registry_source.sh`，fail-closed 解析 `cargo metadata --locked`、workspace manifest 与 `Cargo.lock`，并断言：三个 package 各恰有一个 2.1.3 registry node；workspace 中实际直接声明的 Oxana dependency constraints 精确为 `=2.1.3`；source 为 crates.io registry；checksum 分别为 `bf94eae5bcc69eb7d6950252afa3f316cfa7d769fecc184735a760861eeb01a1`、`4451fc018cae2fdd5fe86041b3807f0c80401ba87a3fa2e04335e28fa3f20cd1`、`e9b57c0781b889c6dcab3e3e47ad5aef395d5f95443295c3d3b5a2f7819bebda`；仓库无 Oxana path/vendor/`[patch.crates-io]`。任一缺包、重复、版本/source/checksum 漂移或 parser 失败都退出非零。

验证：上述 verifier、`BidDeliveryV1/KBDL` golden、explicit name/job ID、payload/registry 负例、RecordingTransport 单 invocation 0/1 调用次数、真实adapter不可达Redis分类与硬deadline、live Redis `Storage::enqueue`/unique `Skip`/resurrection、真实注册worker的`max_retries=0`/单次失败/dead路径、offer returned ID mismatch携带bounded实际ID且readiness fail closed、legacy replay mixed processing-list 事实和 Bid/WorkTransport private-key/call-site/correctness-inspection denylist。`resurrection`运维指标固定为来自冻结job合同的`resurrection_enabled` gauge；实际复活由live行为回执证明，不能手工累加伪造发布版计数。required脚本必须验证对应测试结果行是`ok`，仅出现测试名、`ignored`、skip或零用例都失败。测试必须如实证明发布版行为，不能将 `Ok` 提升为 inserted/accepted receipt，也不能在 PR8A 冒充 DB 收敛证据。该切片即把统一cleanup harness、六种退出模式receipt、shell trap和CI `if: always()`残留断言接入`bid-durable-dispatch`；不是等PR9才补资源合同。

### PR8B — Durable dispatch core 与 baseline

范围：

- 建立 dormant base/六类空 typed extension、空的 document conversion domain target 表、dispatch head/immutable intents/state/一次性 delivery attempt/business attempt owner lease/bounded late observation/settlement/inbound/repair obligation/rejected delivery/typed evidence/semantics/governor schema、partial index 与 ACL；conversion producer/current pointer 仍不激活；
- owner-only SQL `stage/replace_current_target/cancel_target` 只能由所属 SECURITY DEFINER domain mutation 在其现有transaction中直接调用；`PUBLIC`、API、worker、dispatcher、retention role均无直接`EXECUTE`。Rust不得一一映射或直调internal entry；只有实际Rust mutation调用方可用接收现有transaction的`pub(crate)` adapter包住完整domain mutation，run/handle只使用private store seam；禁止commit后补stage；
- 新增独立login role`kb_runtime_bid_dispatcher`与`KNOWLEDGEBRAIN_BID_DISPATCHER_DB_PASSWORD`；`010-runtime-identities.sh`按既有风格创建`LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS` identity。该role加入脚本全部governed数组、password helper允许名/初始`EXECUTE` grantee、handoff/finalizer检查、Rust`GOVERNED_ROLES`/runtime reachability与first-launch catalog allowlist，finalized exact role count从13变14；dispatcher自身始终无membership/`MEMBER/SET`，handoff只保留verifier的两条临时SET edge且finalizer后governed membership为零；
- `kb_app_owner`只授予dispatcher非grantable database`CONNECT`、`public` schema`USAGE`和`run`背后的bounded函数；worker DSN只能`handle`，dispatcher独立`BID_DISPATCH_DATABASE_URL`只能`run`，双方不能直接DML、直调internal mutation或调用对方入口。`deploy/docker-compose.yml`向PostgreSQL bootstrap传password并生成dormant DSN；`.github/workflows/ci.yml`、fresh-schema/Compose first-launch脚本、catalog ACL/password posture/handoff/post-finalizer verifier常量与expected role count同步验证全链；PR8B不在worker main、`run_core`、API或domain mutation中启动dispatcher；
- 实现 `stage/replace_current_target/cancel_target/run/handle` 深 module、PostgreSQL store、one-shot offer claim/settle、delivery-start successor、内部 consumer begin/background heartbeat/publish、business lease repair 和 retention；worker只能调用`handle`，不能编排内部原语；
- synthetic fixture只在可丢弃独立数据库或最终显式rollback的完整transaction中创建真实`bid_document_conversion_targets` row，并经测试owner fixture调用同一owner-only mutation建立`document_conversion` aggregate；多连接commit/race只能使用可丢弃独立数据库。每例在trap/finally中销毁并断言aggregate/role/database零残留；不新增synthetic target kind/extension/task/registry。测试可注入RecordingTransport与测试私有typed adapter；PR8B生产composition root不注册六类真实adapter、不创建真实aggregate、不启动dispatcher；
- 旧 target 不补建、不双写，六种 target owner（conversion、extraction、matching schedule、matching job、attachment preparation、submission render）均不切换，旧 producer/consumer/live recovery继续保留为唯一真实 owner。

PR8B 是一个合并门，内部实现固定拆为八个顺序 vertical slices：

| 内部 slice | 范围 | 进入下一 slice 前的验证 |
| --- | --- | --- |
| B1 | canonical KBDL/KBTF、dispatcher role/DSN、基础 ACL | golden/篡改、role catalog、API/worker/dispatcher交叉 deny |
| B2 | dormant base/extensions/conversion target/head/intent/state、owner-only SQL、完整domain mutation wrapper与run/handle store seam | commit/rollback、exact FK、immutability、NULL matrix、runtime/Rust direct-call deny |
| B3 | one-shot delivery claim/offer与delivery-attempt outcome | claim后crash/Ok/Err/timeout/response lost无二次offer、outcome矩阵 |
| B4 | late observation/settlement/inbound/repair/rejected/evidence | late publisher/consumer/reaper、XOR/exact FK、bounded uniqueness、canonical insert-or-read |
| B5 | business begin/lease/budget/governor/promotion | capacity、N边界、global/per-kind最后slot及promotion双序 |
| B6 | successor/replacement/cancel/absorbing cleanup | 五态、late delivery/publisher、race与残留拒绝 |
| B7 | `handle` scoped heartbeat lifecycle | normal/error/timeout/drop/panic/shutdown/fenced/DB failure收敛 |
| B8 | required job与dormant closure | fresh catalog/ACL/checksum；无生产target/adapter/dispatcher spawn；旧owner仍唯一 |

验证：base `PRIMARY KEY(id)` 与 typed/head/initial intent/state commit/rollback、六类 `KBTF` 与 `KBDL` fixed golden及逐字段篡改负例、`claim_lease_ms` 单真源及0/低于30s/超过30m负例、`max_attempts` 0/>10负例、delivery attempt exact intent/claim-token/phase/outcome composite FK及完整NULL matrix、cross-dispatch pointer、offering→settled、awaiting→inflight/consumer-started、returned job≠expected、第二次claim与settled outcome改写负例、bounded observation exact FK/每observer唯一、publisher/consumer/reaper/replacement/cancel outcome与NULL shape错配及adapter mismatch actual=expected负例、publisher-result与consumer-first/lease-expiry/replacement/cancel四组双序race及败方重读后的合法transition、publisher-first settled→consumer begin创建唯一business owner且delivery disposition不变、delivery-start deadline-vs-publisher与gate-rebase-vs-publisher双序race及old absorbing state/attempt matrix、business attempt dispatch/target/status FK与 ordinal/token unique、running缺lease、terminal缺code、ordinal 0/gap/>max、state/settlement/evidence attempt-status错配负例、contract-poison stored/recomputed、`UNIQUE(settlement_key)` 双事务 insert-or-read、head/predecessor/generation/attempt/settlement composite FK、replacement 指向 generation>0、cross-project/cross-kind/same-target 与 state/settlement kind 错配负例、第二 successor/第二 disposition拒绝、`advanced` 与 cross-target `superseded` replacement XOR/FK/并发/晚到 delivery、replacement 的 ready/offering/awaiting/running/absorbing 五态与 late publisher、replacement-vs-heartbeat/publish、正确 payload + 错误 `JobContext.meta.id` 且 external I/O=0、state NULL matrix、claim 后 crash/Ok/Err/timeout/response-lost 均不二次 offer、consumer-before-publisher、late publisher observation、deadline-vs-begin、duplicate owner CAS、owner-expired repair、attempt N-1/N、retryable-at-N handler settlement/evidence/inbound原子性、reaper-at-N无 inbound、resultless exhausted/result-artifact错配负例、非法 begin-at-max 后继只能 `DISPATCH_BUDGET_ORPHAN` poison、begin后连续 crash/reap在 max边界 terminal exhausted、gate rebase、hash-only/volume-loss successor、ACL 和空库 catalog。

`handle` lifecycle测试必须覆盖正常、error、timeout、cancel/future-drop、panic、worker shutdown、heartbeat fenced与heartbeat DB failure；每条都证明guard不残留续租task、adapter cancellation token被触发、旧owner不能publish，lease最终可由reaper按budget收敛。测试不得用detached `tokio::spawn`加sleep作为通过条件。

另以 synthetic aggregate 固定验证 `cancel_target` 的 ready/offering/awaiting/running/absorbing 五态与 late publisher/delivery；统一absorbing cleanup覆盖normal terminal、advanced、replacement、cancel、`DISPATCH_BUDGET_ORPHAN`与`DISPATCH_FENCE_ORPHAN`，并拒绝任何absorbing state残留inflight attempt；gate stale只能 same-target rebase，target fence stale必须 replacement或 `DISPATCH_FENCE_ORPHAN` poison，绝不创建复制 stale fence的 successor。

inbound shape负例必须证明 historical/terminal只能引用 absorbing settlement，current owner/gate/fence stale只能以相同reason exact composite FK引用 unresolved repair obligation，delivery mismatch只能以observed identity与mismatch kind exact composite FK引用rejected delivery；三类指针XOR、owner/gate/target reason错配、cross-dispatch repair resolution、rejected expected/observed ID/version/mismatch kind/reason错配均拒绝，repair resolution只绑定后续 settlement而不改写原 observation。owner-expired→gate-stale、gate-stale→target-stale及handler-vs-repair双序race必须证明任一absorbing transaction按dispatch解析全部unresolved obligations，且absorbing state不能残留unresolved row。

execution governor以两个并发begin争抢global/per-kind最后一个slot证明硬上限；terminal/reap/replacement/cancel精确释放，counter underflow/overflow、attempt config generation错配、promotion limit低于active count、跨新旧generation聚合超限及counter与全部running attempt数量不等都必须失败。capacity unavailable不得创建business attempt或清start deadline；promotion-vs-begin与promotion-vs-terminal两组双序并发必须按统一pointer→global→kind子序完成且无`40P01`。

### PR8C — Conversion/Extraction 纵切

范围：

- 切换 document conversion 和 extraction target adapter；
- conversion settlement 与 converted source + extraction target + intent 同事务；
- 同一改动删除这两类旧 enqueue/recovery/housekeep 分支。

验证：TenderPublication 强制活库、commit/offer/begin/publish fencing、长转换 heartbeat、后继原子性、conversion/extraction 人工 retry generic replacement/current-pointer CAS和该 target family 的删除扫描。

### PR8D — Attachment preparation/Render 纵切

范围：

- 切换 PDF attachment preparation 和 submission render target adapter；
- 保持 upload staging、owner transfer、manifest-only render 与 publish fencing；
- 同一改动删除这两类旧 enqueue/recovery/housekeep 分支。

验证：preparation/render claim/heartbeat/retry/reap/cancel、staging abandon、publish 原子性、preparation重新提交/render重新请求 generic replacement/current-pointer CAS、DOCX/PDF gate与该 target family 的删除扫描。

### PR8E — Matching schedule/job 纵切

范围：

- 切换 matching schedule 和 matching job target adapter；
- schedule settlement 原子产生 manifest、0..N jobs 与等量 intents；
- 同一改动删除 dirty-manifest、orphan-target/orphan-match 和旧 matching recovery owner。

验证：schedule/fanout 原子性、lease/staging/commit、65 eligible/0 hit、普通+unsectioned 混合、持久化 1..N picks；matching mutation 对旧 schedule replacement与旧 manifest下0/1/N nonterminal jobs cancel同事务，晚到旧 job外部 I/O=0；以及该 target family删除扫描。

### PR8F — 单 owner closure 与全量门禁

范围：

- 删除剩余 Bid 两跳 live recovery、best-effort enqueue、业务 housekeep 和旧 wire DTO/registry；复证 Bid/WorkTransport 对 private Redis key 与 inspection/delete correctness API 的 denylist为零；
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

当前 CI 已落位独立 job `bid-durable-dispatch`，使用独立 PostgreSQL 16 和 Redis 7；`bid-product-smoke` 继续承担产品活库门，不作为 durable-dispatch 的可替代绿色旁路。该 CI 变更尚未 push，branch protection/required-check 聚合也尚无远端证据，因此这里只记录 implemented/locally verified/committed，不提升 pushed 或 runtime accepted。job 固定：

```text
KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS=1
KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS=1
scripts/bid_durable_dispatch_acceptance.sh
```

PR8A 新增 `scripts/bid_durable_dispatch_acceptance.sh`，只纳入 registry verifier、Oxana/runtime pure prepare、adapter 单 invocation 0/1 调用次数、stable enqueue/Skip/resurrection、legacy mixed-list命令与统一六模式cleanup harness；CI从该切片起以独立`if: always()`步骤再次执行幂等cleanup+残留断言并校验receipt。PR8B 在启用任一 Bid owner 前纳入 dormant dispatch SQL/worker、claim 后 one-shot、successor 和 replay-independent synthetic 命令；PR8C～PR8E 在各自纵切中纳入对应领域命令；PR8F 使用下列完整固定清单。未来切片的 test target 不必在更早 PR 用占位测试伪造通过，但启用某 target owner 的 PR 必须先将它的真实合同加入 required job。该入口顺序执行且任一失败立即失败：

```text
scripts/verify_oxana_registry_source.sh
cargo test -p runtime jobs::tests -- --nocapture
cargo test -p runtime --test work_transport_live -- --nocapture --test-threads=1
cargo test -p bid --test durable_dispatch_sql -- --nocapture --test-threads=1
cargo test -p bid --lib dispatch::tests -- --nocapture
cargo test -p worker --test durable_dispatch_worker -- --nocapture --test-threads=1
cargo test -p bid --test tender_publication -- --nocapture --test-threads=1
cargo test -p bid --test knowledge_retrieval_selection -- --nocapture --test-threads=1
cargo test -p bid --test matching_publication -- --nocapture --test-threads=1
cargo test -p bid --test submission_sql -- --nocapture --test-threads=1
cargo test -p api --test submission_contract -- --nocapture --test-threads=1
scripts/fresh_schema_acceptance.sh
scripts/bidding_v1_deletion_scan.sh
```

三个PR8B入口责任固定：`durable_dispatch_sql`以DB owner fixture验证internal SQL mutation、schema/FK/CHECK/deferred verifier、ACL、并发与真实`document_conversion` typed synthetic aggregate，不经过Rust直调；`cargo test -p bid --lib dispatch::tests`以crate unit tests验证完整Rust domain mutation wrapper（若存在）、run/handle store seam、private runner、注入clock、RecordingTransport及once-per-dispatch，并证明Rust无internal SQL entry直调路径，不导出测试façade；`durable_dispatch_worker`只经worker可见`handle` seam验证ObservedDelivery、测试私有typed adapter、structured heartbeat与lifecycle，不能直接编排内部原语。三者从PR8B起都是固定合同入口；对应文件/测试不存在或无匹配用例时required job必须保持红色。PR8A的job不得用尚不存在的DB dispatcher测试冒充one-shot证据；PR8B合并前claim/crash/timeout/successor合同必须进入上述固定入口。所有活库测试必须在连接、schema或Redis不可用时fail closed；任一`SKIP`、`SKIPPED`、`skipped live`、零用例匹配或缺少预期contract ID均使job失败。无`continue-on-error`、无optional service、无本地环境自动降级。

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
- 固定 `success|failure|timeout|cancel|SIGINT|SIGTERM` 六种退出模式；每种模式使用独立 project，均执行 `down --volumes --remove-orphans`、移除本轮临时 image，并在返回前证明无残留 container/volume/network；cleanup 失败不得被原始状态覆盖。
- shell trap 负责进程内 cleanup，CI 独立 `if: always()` 步骤再次执行幂等 cleanup+残留验证；两层任一失败都使 required job 失败。

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
10. 额外独立 case 覆盖 claim 后/Redis 前 crash、enqueue error、timeout/response-lost、hash-only、membership-only、duplicate membership、Redis flush、Oxana 七天 cleanup、整个 Redis volume 丢失、同容器 hostname/PID restart、worker process crash、process-alive/task-stuck、DB 连接丢失、render failure 与 retention retry。其中 Redis volume loss 使用独立 Compose project，不污染其余 case。

第 10 项的 fixed recovery scenario IDs 是：

```text
offer_claimed_publisher_crash_before_redis
enqueue_error
enqueue_timeout
enqueue_response_lost
redis_hash_only
redis_membership_only
redis_duplicate_membership
redis_flush
oxana_seven_day_cleanup
redis_volume_loss
same_container_hostname_pid_restart
worker_process_crash_after_begin
worker_task_alive_but_stuck
database_connection_loss
submission_render_failure
retention_retry
```

验收程序从以上数组派生 `required_count`，逐个验证 ID 唯一且集合完全相等；禁止在代码或 evidence 中另写一个可漂移的手工常量。新增/删除 scenario 必须在同一改动更新该权威数组、执行器和 schema golden。

24 个核心 case 的 `target_kind` 必须精确覆盖：

```text
document_conversion
extraction_target
matching_schedule
matching_job
attachment_preparation
submission_render
```

每个 case 使用独立 project/target/dispatch identity，执行前证明没有同 identity 的历史 target、head 或 intent。每行保存以下固定 schema：

```text
case_id
target_kind
fault_point = commit|offer|begin|publish
fixture_sha256
project_id, target_id, initial_dispatch_id, final_dispatch_id, business_generation, dispatch_generation
injection_trigger, injection_observed_at
expected_intermediate_target_status
expected_intermediate_dispatch_status
convergence_deadline_ms
final_target_status
final_dispatch_status
effective_publish_count
orphan_domain_target_count
orphan_async_base_target_count
orphan_extension_count
orphan_head_count
orphan_intent_count
orphan_state_count
orphan_artifact_count
orphan_object_owner_count
historical_noop_count
delivery_attempt_outcomes[]
business_attempt_terminal_codes[]
evidence_refs[]
result = passed|failed
```

`commit` case 要求结果只能是全回滚或 domain target+async base target+typed extension+head+initial intent+ready state 全部可见；六类核心 `offer` case 固定注入 enqueue returned 但 publisher DB settle 未完成；`begin` 固定注入 delivery 已取得但 business claim 未完成；`publish` 固定注入外部/staging 产物已形成但 fenced terminal transaction 未确认。第 10 项 transport cases 再分别证明其它 ambiguous enqueue/Redis 形态，不把 24 个核心 case 的单行结果冒充四种故障都已执行。不允许 `skipped`、`inconclusive` 或人工填写 `passed`；缺少任一 target/fault-point 组合、超过收敛时限，或 domain target/async base/extension/head/intent/state/artifact/object owner 任一方向有孤儿时 job 失败。出现 successor 时必须证明旧 dispatch 只有 durable historical noop 且 `effective_publish_count=1`。

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
recovery_scenarios.required_ids[]
recovery_scenarios.required_count = len(required_ids)
recovery_scenarios.executed_ids[]
recovery_scenarios.passed_count = len(required_ids)
recovery_scenarios.failed_count = 0
recovery_scenarios.sha256
artifacts.job_attempt_ids[]
artifacts.report_quote_manifest_output_ids[]
artifacts.report_quote_manifest_output_sha256[]
artifacts.pdf_sha256
artifacts.pdf_download_metadata_sha256
logs_traces_sha256
cleanup.required_modes[] = success|failure|timeout|cancel|SIGINT|SIGTERM
cleanup.receipts[].mode
cleanup.receipts[].project_name
cleanup.receipts[].containers_remaining = 0
cleanup.receipts[].volumes_remaining = 0
cleanup.receipts[].networks_remaining = 0
cleanup.receipts[].temporary_images_remaining = 0
cleanup.receipts[].trap_result = passed
cleanup.receipts[].ci_always_result = passed
cleanup.result = passed
completion_eligible = true
```

所有 image source revision 必须是已提交、已 push 且可追溯到 candidate SHA 的固定身份；`git_dirty=true`、`playwright=not-used`、任一 skip、证据文件缺失或 artifact 上传 `warn` 均不能产生 `completion_eligible=true`。敏感字段只记录脱敏 digest、ID 或 bounded code，不保存 secret value、文档全文或未脱敏 trace。

cleanup harness 必须实际触发并验证 `success|failure|timeout|cancel|SIGINT|SIGTERM` 六种模式，每种模式使用独立 project 并产生独立 receipt；缺少、重复或伪造模式均失败。证据上传用 `if: always()` 保留失败现场；上传不取代 shell trap 或 CI cleanup，任何 cleanup/残留验证失败都使 required job 失败。

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
