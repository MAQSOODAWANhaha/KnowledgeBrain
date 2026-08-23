# Round-2 真实业务流 / 部署 / UI 复审

## Review

### Correct

- `scripts/bid_e2e_smoke.sh` 确实启动真实 `api` 和 `worker` 二进制，并连接真实 PostgreSQL、Redis，而不是 mock server（`scripts/bid_e2e_smoke.sh:22-23`）。
- API 和 worker 启动时都会连接 PostgreSQL并执行迁移；迁移使用 advisory lock 串行化两个进程的并发启动（`crates/api/src/main.rs:14-17`、`crates/worker/src/main.rs:14-17`、`crates/storage/src/persist.rs:18-40`）。
- 上传后的对象确实写入本地对象目录；随后 worker 又从该对象读取并转换，成功产出条款间接证明了本地 blob 往返（`crates/api/src/routes.rs:3838-3855`、`crates/bid/src/lib.rs:538-542`）。
- 自动抽取具备数据库级幂等保护：同一文档同一 conversion generation 的 auto run 有唯一索引，并使用 `ON CONFLICT DO NOTHING`（`crates/storage/src/bid.rs:855-901`、`migrations/0007_bid.sql:129-131`）。
- bid convert、extract、section retry、match 和 housekeeping workers 都已注册到运行时（`crates/worker/src/consume.rs:2722-2733`、`2771-2782`）。
- worker 有 Redis runtime 退出后的指数退避重连循环（`crates/worker/src/main.rs:34-52`）；数据库中也保留 pending intent，housekeeping 会重新投递 convert/extract/retry/match（`crates/worker/src/consume.rs:1438-1487`）。
- SPA 使用已提交的 `package-lock.json`，CI 和镜像构建均使用 `npm ci`，具备可重复依赖安装基础（`web/package.json:5-9`、`.github/workflows/ci.yml:88-90`、`deploy/Dockerfile.rust:3-7`）。
- rejected 条款没有被完全隐藏：客户端只过滤 `superseded`，因此 rejected 仍会显示（`web/src/bid/helpers.ts:50-57`）。
- 成稿编辑、预览、陈旧稿提示和 DOCX/PDF API caller 均存在（`web/src/bid/Workbench.tsx:222-252`、`296-327`、`web/src/bid/BookletPane.tsx:1-54`）。

## Blocker

### 1. 新 smoke 远未覆盖声明的完整业务流

真实 smoke 只执行：

1. 启动 API/worker；
2. `/health`；
3. 本地开放模式登录；
4. 创建 bid；
5. 上传一个纯 Markdown；
6. 等待任意 draft；
7. 确认一条；
8. 最后只断言 confirmed 数量至少为一。

证据见 `scripts/bid_e2e_smoke.sh:22-82`。它没有执行 edit、reject、技术和商务双匹配结果、知识资产建模/入库、pick、shot、booklet、preview、DOCX/PDF export、manual reextract、section retry、end、Redis 重启或 PostgreSQL 重启。

此外，match 等待循环结束后没有断言 `.derived.match_running == false`，循环超时仍会继续并通过 confirmed 计数（`scripts/bid_e2e_smoke.sh:73-82`）。因此即使 match 永远 pending，此 smoke 也可能打印 PASS。

**窄修复：**扩展该脚本或新增第二个完整 flow 脚本；每个异步阶段都必须有带超时的最终状态断言，并验证具体输出记录，而非只等待或计数一条 confirmed。

### 2. 部署镜像缺少 PDF 所需 CJK 字体，容器内 PDF 导出会失败

PDF 生成只接受以下字体文件：

- `/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc`
- `/usr/share/fonts/opentype/unifont/unifont.otf`

找不到即返回错误（`crates/bid/src/export.rs:132-143`）。Rust runtime 镜像只安装 `ca-certificates` 和 `curl`（`deploy/Dockerfile.rust:18-21`），没有安装任何上述字体。

**窄修复：**在 runtime stage 安装 `fonts-wqy-zenhei` 或 `fonts-unifont`，并在 CI 中实际构建镜像后从容器调用 PDF export，断言 `%PDF`。

### 3. Compose 部署无法启用 LDAP，默认实际是任意账号免密码登录

`deploy/.env.example:31-35` 声明 LDAP 配置，但 `deploy/docker-compose.yml:3-47` 的 `x-app-env` 没有把 `KNOWLEDGEBRAIN_LDAP_URL` 或 `KNOWLEDGEBRAIN_LDAP_BIND_DN` 传入 API 容器。没有 LDAP URL 时，认证明确进入 local-open 模式并忽略密码（`crates/auth/src/lib.rs:34-40`、`crates/api/src/routes.rs:523-539`）。

同时 Compose 默认 `JWT_SECRET=change-me`（`deploy/docker-compose.yml:7`）。

这意味着按部署文档启动的暴露服务不是严格真实认证。

**窄修复：**把 LDAP 变量加入 `x-app-env`；生产 profile 应要求非默认 JWT secret，并明确 fail-closed，只有显式 local/test 开关才能免密码。

### 4. Dockerfile 的 Rust image ARG 作用域不正确，镜像构建未被 CI 验证

`ARG RUST_IMAGE` 位于首个 `FROM` 所建立的 web-builder stage 内，随后用于一个无继承关系的 `FROM ${RUST_IMAGE}`（`deploy/Dockerfile.rust:2-10`）。按 Docker ARG scope 规则，该变量应在第一个 `FROM` 前声明，否则后续 `FROM` 中未定义。

CI 只运行 Compose 配置解析，不实际构建 Dockerfile（`.github/workflows/ci.yml:84-90`），因此不会捕获此问题。

**窄修复：**把 `ARG RUST_IMAGE=rust:1.97-bookworm` 移到文件首个 `FROM` 之前，并增加 `docker build -f deploy/Dockerfile.rust --build-arg BIN=api ...` gate。

## High

### 5. 现有条款缺少完整 family / must 编辑控件

API 支持更新 `family` 和 `must`（`crates/api/src/routes.rs:3992-4008`、`4038-4048`），但 UI：

- family 只在 `draft && family_conflict` 时出现（`web/src/bid/Inspector.tsx:72-85`）；
- 现有条款没有 must 开关；
- must 仅在“手补新条款”表单中可选（`web/src/bid/ClauseTable.tsx:190-204`）。

这无法完整执行“草稿编辑后确认”的业务要求。

**窄修复：**在 Inspector/ClauseDetail 为所有未结束项目的 draft 提供 family 和 must 控件，调用现有 `patchClause`；确认前显示当前值。

### 6. Section retry 没有状态 UI，也没有状态 API 类型

UI 发起 retry 后只弹出“已排队重抽本段”，然后 reload（`web/src/bid/Workbench.tsx:553-561`）。`MatchUnit` 类型不包含 section `extract_status/error_message`（`web/src/api.ts:82-89`），界面也不显示 pending/running/done/failed。

后端确实维护 section 和 retry job 状态（`migrations/0007_bid.sql:50-100`、`crates/storage/src/bid.rs:1898-1925`），但没有被当前 web flow 暴露。

**窄修复：**在 units 或独立 section-status 响应中返回 `extract_status`、`error_message` 和 active retry job status；侧栏及 retry 按钮显示排队/运行/失败并在运行时禁用重复操作。

### 7. PostgreSQL/Redis 恢复只有静态机制，没有故障注入验证，部分中途故障可能长时间挂起

- Redis runtime 有重连循环，但没有测试重启 Redis 后继续消费（`crates/worker/src/main.rs:34-52`）。
- durable intent 依赖每五分钟 housekeeping 重投递（`crates/runtime/src/lib.rs:42-43`、`crates/worker/src/consume.rs:1416-1487`）。
- 已 claim 的 bid 工作以 2 小时 10 分钟为 stale threshold（`crates/runtime/src/lib.rs:41-46`）。若 PostgreSQL 在“业务已 claim、失败状态尚未写回”期间中断，清理写可能失败，任务可能等待 stale reclaim。
- 当前 smoke 完全不重启任何依赖。

现有 storage 测试验证了 fencing/reclaim 函数，但 rust CI job没有 PostgreSQL service，这些测试在数据库不可用时会 skip；bid-smoke job只 build，不执行这些测试。

**窄修复：**新增故障注入 E2E：分别在 convert、extract、match 中途重启 Redis/PostgreSQL，断言 worker 重连、intent 被重新消费且旧 lease 被 fencing。对 bid job 使用较短且阶段专用的 lease/recovery threshold。

### 8. 严格 agent、VLM 和真实 embedding/matching 均未验证

Smoke 强制 `BID_EXTRACT_MODE=heuristic`（`scripts/bid_e2e_smoke.sh:21`），上传文档也没有图像，因此：

- agent tool calls 完全不执行；
- VLM 完全跳过；
- DocReader gRPC 不执行，Markdown 使用 in-process simple reader；
- bid-smoke CI 只启动 PostgreSQL 和 Redis（`.github/workflows/ci.yml:20-47`）；
- 未配置 embedding endpoint 时使用 deterministic hashed stub（`crates/index/src/lib.rs:12-27`）。

**窄修复：**保留 deterministic smoke，同时新增有凭证的非默认 strict integration job，使用 PDF/Office 和含图样本，要求 agent tool-call diagnostics、VLM OCR/caption 和真实 embedding model metadata。

## Medium

### 9. Rejected 虽可见，但被错误当成“待确认草稿”

`liveClauses` 会保留 rejected（`web/src/bid/helpers.ts:50-57`），但 UI 使用 `status !== "confirmed"` 判断 draft：

- rejected 行显示“未确认 · 未进匹配”并继续提供“驳回/确认”按钮（`web/src/bid/ClauseTable.tsx:139-178`）；
- ClauseDetail 将 rejected 显示为“待确认”（`web/src/bid/ClauseDetail.tsx:58-65`、`151-159`）；
- Sidebar 将 rejected 计入 pending（`web/src/bid/Sidebar.tsx:64-67`），而后端 derived 只统计真正 draft（`crates/storage/src/bid.rs:1943-1948`）。

**窄修复：**明确区分 `draft`、`rejected`、`confirmed`；增加 rejected filter/标签和“恢复为草稿”动作，不要把 rejected 混入待确认计数。

### 10. Workbench auto-extract guard 仅覆盖当前组件生命周期

`extractTried` 是内存 ref（`web/src/bid/Workbench.tsx:91`），切换项目时清空（`143-147`），自动调用 reextract 后只在当前 mount 阻止重复（`165-173`）。重新进入同一项目且仍无条款时会再创建 manual extract run；manual run 没有 auto generation 唯一约束。

数据库对 convert 触发的 auto run 幂等是正确的，但 Workbench 的补救式 manual reextract 不是持久化幂等。

**窄修复：**后端 expose 最近 run 状态/触发原因；UI 只有在最新 run 终态失败或明确“完成且零条款”时提示用户重抽，不应在每次 mount 自动 POST manual run。

### 11. API readiness 只表示 HTTP 存活

`/health` 返回静态 `ok`，不检查 PostgreSQL、Redis、worker 或 DocReader。Smoke 也只等待该 endpoint（`scripts/bid_e2e_smoke.sh:25-35`）。Compose API health 因而不能表示业务链已 ready（`deploy/docker-compose.yml:145-150`），worker 本身也没有 healthcheck。

**窄修复：**保留 liveness，增加 readiness endpoint，至少检查 PostgreSQL、Redis enqueue/read 和 worker heartbeat；Compose 与 smoke 使用 readiness。

### 12. MinIO “双写”失败被静默吞掉

本地对象写成功后，S3 PUT 错误被直接忽略（`crates/storage/src/lib.rs:25-31`）。API/worker Compose 也没有依赖 MinIO healthy。当前 smoke 使用临时本地 `OBJECT_DIR`，不启动或读取 MinIO。

因此 blob 的本地路径是真实的，但部署文档所称 MinIO 双写可靠性没有验证。

**窄修复：**记录并暴露 S3 projection failure；增加 MinIO roundtrip smoke，或明确文档说明 MinIO 仅 best-effort projection。

---

## Stage-by-stage classification

| 阶段 | 分类 | 当前证据与真实程度 |
|---|---|---|
| API/worker startup | **real service-backed** | 真实二进制启动；仅 API liveness 被断言。 |
| migrations | **DB integration** | 真实 PostgreSQL迁移，由 API/worker 启动触发；未核验 schema/version 内容。 |
| login | **real service-backed** | 真实 HTTP/JWT/user persistence，但属于 local-open 免密码模式，不是 LDAP。 |
| create bid | **DB integration** | 真实 API insert。 |
| upload | **real service-backed** | 真实 multipart API、DB row、Redis enqueue。 |
| blob | **real service-backed** | 本地临时目录真实写读；S3/MinIO 未运行。 |
| convert | **real service-backed** | 真实 worker + in-process Markdown simple reader；DocReader未执行。 |
| multimodal | **unproven** | 无图、无 VLM，实际为 skipped；没有断言 status。 |
| auto extract | **real service-backed** | 真实 Redis worker、heuristic engine、PostgreSQL sections/clauses。 |
| extraction diagnostics | **DB integration** | 后端会存 diagnostics，API/UI只展示有限摘要；smoke 未断言。 |
| draft text edit | **unproven** | API/UI实现存在，smoke 未调用。 |
| reject | **unproven** | API/UI实现存在，smoke 未调用；UI状态语义不正确。 |
| confirm | **real service-backed** | 真实 PATCH + DB update，被 smoke 断言。 |
| technical match | **unproven** | confirm 会同步创建真实 job；smoke 没有断言 job终态或 candidates，等待循环可超时后通过。 |
| commercial match | **unproven** | 商务 draft 没有被明确确认，结果完全未检查。 |
| strict match semantics | **external-stubbed** | 无真实 embedding endpoint时使用 hashed embedding。 |
| pick/unpick | **unproven** | caller和DB实现存在，未执行。 |
| shot upload/read/delete | **unproven** | caller和对象实现存在，未执行。 |
| booklet generation | **DB integration** | 后端生成/保存实现存在且有 unit tests；服务 smoke 未执行。 |
| booklet edit | **unproven** | web caller存在，未执行。 |
| preview | **unit/scripted** | web预览渲染 booklet Markdown；`/preview` API未被当前 Workbench调用，未做浏览器验证。 |
| DOCX export | **unit/scripted** | 字节生成有 unit test；服务和容器未验证。 |
| PDF export | **unit/scripted** | host unit test存在；部署容器缺字体，实际阻塞。 |
| manual reextract | **unproven** | route和caller存在，smoke只走 auto extract。 |
| section retry | **DB integration** | lease/reclaim persistence tests存在；服务/UI状态 flow 未验证。 |
| end bid | **unproven** | route和UI存在，smoke 未调用。 |
| Redis restart/recovery | **unproven** | 静态重连和 durable intent 机制存在，无重启测试。 |
| PostgreSQL restart/recovery | **unproven** | sqlx pool/lease fencing机制存在，无重启测试。 |
| SPA build | **unit/scripted** | CI执行 `npm ci && build`，无浏览器用户流测试。 |
| Compose deployment | **unproven** | CI只做 `docker compose config`; 不构建或运行镜像。 |

## External credentials/services required for strict validation

严格 agent/VLM 验证至少需要：

1. **Tool-capable chat/agent provider**
   - `BID_EXTRACT_MODE=agent`
   - `KNOWLEDGEBRAIN_CHAT_BASE_URL`
   - `KNOWLEDGEBRAIN_CHAT_API_KEY`
   - `KNOWLEDGEBRAIN_CHAT_MODEL`
   - 可选显式 `BID_EXTRACT_MODEL_ID`
   - 也可使用对应 `LLM_*` alias。

2. **VLM provider**
   - `KNOWLEDGEBRAIN_VLM_BASE_URL`
   - `KNOWLEDGEBRAIN_VLM_API_KEY`
   - `KNOWLEDGEBRAIN_VLM_MODEL`
   - 必须使用带真实图片/扫描页的 tender。

3. **真实匹配语义**
   - `KNOWLEDGEBRAIN_EMBEDDING_BASE_URL`
   - `KNOWLEDGEBRAIN_EMBEDDING_API_KEY`
   - `KNOWLEDGEBRAIN_EMBEDDING_MODEL`
   - 产品手册和公司资质需先真实入库并达到 `index_ready`。

4. **非 Markdown 文档转换**
   - 正常运行的 DocReader gRPC 服务；
   - 若验证特定解析器，还需要 MinerU/Paddle endpoint 或 cloud token。

5. **严格生产登录**
   - 可达 LDAP 服务；
   - `KNOWLEDGEBRAIN_LDAP_URL`、`KNOWLEDGEBRAIN_LDAP_BIND_DN`；
   - Compose 修复变量透传；
   - 非默认 `JWT_SECRET`。

## Candid verdict

**对“完整真实跑通业务流程了吗？”的回答：没有。**

当前只真实跑通了一个很窄的后端 happy path：

> 本地免密码登录 → 建标 → 上传纯 Markdown 到本地 blob → Redis worker 转换 → heuristic 自动抽取 → 取得任意 draft → 确认一条。

它没有跑通完整用户业务流，没有浏览器级验证，没有真实 agent/VLM/embedding，没有验证双类别匹配结果、选型、补图、成稿、预览、导出、重抽、段重试、结束或数据库/队列恢复；部署镜像的 PDF、认证和 Docker ARG 还存在阻塞项。
