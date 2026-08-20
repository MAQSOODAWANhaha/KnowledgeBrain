# Round-3 真实业务流 / 部署 / UI 最终复审

## Review

### Correct

- Round-3 smoke 已不再只是“上传后确认一条”：它启动真实 API/worker，连接 PostgreSQL、Redis，执行上传、抽取、拒绝/确认、Section retry、成稿保存、预览 API、DOCX/PDF、手动重抽和结束后写保护（`scripts/bid_e2e_smoke.sh:27-146`）。
- 自动抽取成功和最小 diagnostics 已有明确断言：`latest_extract.status == done`、`candidate_spans >= 3`（`scripts/bid_e2e_smoke.sh:63-71`）。
- Section retry 不再只验证入队，而是等待最新 retry job 到 `done`，并显式拒绝 `failed`（`scripts/bid_e2e_smoke.sh:99-111`）。
- PDF 字体和 Docker ARG 问题已经修正：全局 `ARG RUST_IMAGE` 可用于后续 `FROM`，runtime 安装 `fonts-wqy-zenhei`（`deploy/Dockerfile.rust:3-4`、`18-25`）。CI host smoke 也安装同一字体并实际导出 PDF（`.github/workflows/ci.yml:52-58`）。
- CI 现在有 PostgreSQL/Redis service、强制 storage persistence tests、真实 API+worker smoke、web build 和 Rust runtime image build（`.github/workflows/ci.yml:17-58`、`76-96`）。
- rejected UI 已与 draft 区分，并可从 Inspector/Detail 恢复草稿（`web/src/bid/ClauseTable.tsx:40-50`、`139-177`，`web/src/bid/Inspector.tsx:60-103`，`web/src/bid/ClauseDetail.tsx:44-56`、`163-168`）。
- draft family/must 控件和 Section retry pending/running/failed UI 已存在，运行中会禁用重复 retry（`web/src/bid/Inspector.tsx:75-98`，`web/src/bid/Workbench.tsx:31-33`、`187-193`、`526-541`，`web/src/bid/ClauseTable.tsx:61-69`）。
- 部署文档已经诚实注明：无模型端点会走 stub，LDAP URL 为空属于本地/测试开放登录（`deploy/README.md:22-25`、`80-81`）。

---

## Blocker

### 1. 当前 LDAP 实现不支持安全的 LDAPS，却会静默把 `ldaps://` 当普通 TCP

`ldap_bind` 同时剥掉 `ldap://` 和 `ldaps://`，随后直接使用 `TcpStream` 发送包含密码的 simple-bind BER，没有 TLS、证书校验或 StartTLS；未写端口时甚至统一回落到 `389`（`crates/auth/src/lib.rs:43-75`）。实现还手工用单字节长度编码 DN/password（`crates/auth/src/lib.rs:77-89`），不是完整 LDAP BER 客户端。

与此同时，领域要求正式登录走公司 LDAP（`docs/bid-platform-domain.md:23-27`），部署文档要求生产配置 LDAP（`deploy/README.md:22-25`）。因此“严格生产 LDAP 已可用”不能成立；对外生产部署是安全阻塞项。

**窄修复：**

- 改用成熟 LDAP 客户端，明确支持 LDAPS/StartTLS、证书验证、DN escaping 和完整 BER。
- 对 `ldaps://` 禁止明文降级；默认端口应为 636。
- 增加真实 LDAP/LDAPS integration test，至少断言正确密码成功、错误密码失败、证书错误 fail-closed。
- 增加显式 `AUTH_MODE=local-open|ldap`；ldap 模式配置缺失时启动失败。

---

## High

### 2. Match smoke 仍可在技术或商务匹配失败时误报 PASS

Smoke 只等待 `.derived.match_running == false`（`scripts/bid_e2e_smoke.sh:91-97`）。该字段只统计当前 generation 的 `pending|running`；`failed` 同样返回 false（`crates/storage/src/bid.rs:1651-1663`）。

后端实际分别创建技术 unit job 和项目级商务 job（`crates/bid/src/lib.rs:1340-1389`），并维护 `status/tech_status/commercial_status/error_message`（`crates/storage/src/bid.rs:1665-1694`），但 smoke 没有断言：

- 技术 job `status=done && tech_status=done`；
- 商务 job `status=done && commercial_status=done`；
- 技术 candidates 的预期结果；
- 商务条款的 `hit_outcome=miss|hit`。

当前样本又没有产品/公司资产，故技术结果应为空、商务应是 deterministic miss；脚本连这些确定结果也未检查。

API/UI 也隐藏了失败状态：picks API 只返回 picks/candidates，不返回 job status/error（`crates/api/src/routes.rs:4246-4282`）；空候选 UI 会显示“确认本段条款后会出现候选”，即使条款已经确认且匹配失败（`web/src/bid/Inspector.tsx:131-138`）。

**窄修复：**

1. 在 unit/picks 响应增加 `match_status`、`tech_status`、`error_message`；商务项目状态也要可读。
2. Smoke 分别断言技术、商务 job 成功，并断言商务 `COMM_ID.hit_outcome == miss`。
3. UI 区分“运行中 / 成功但无候选 / 失败”，不要把失败显示成待确认。
4. 另加一个种子资产 flow，才能验证真实 candidate、pick 和 commercial hit。

### 3. Smoke 对 edit/family/must 的声明高于实际断言

技术 PATCH 同时提交新文本、`family=technical`、`must=true` 和 confirmed（`scripts/bid_e2e_smoke.sh:78-80`），但后续只断言 `[status,family,must]`，不检查文本（`scripts/bid_e2e_smoke.sh:85-87`）。

而 `TECH_ID` 本来就是 technical，原句包含“必须”，很可能原始 `must` 已经为 true。因此：

- family 未发生真实变化；
- must 未证明发生 toggle；
- 文本虽调用了 API，但没有证明 DB 读回；
- 也没有走“先保存 draft 编辑，再确认”的用户顺序。

**窄修复：**先记录旧值，分别执行并读回 text edit、family 改变、must toggle，再单独确认；每步 GET 断言确实发生状态变化。

### 4. Manual reextract 允许失败后继续 PASS

等待条件接受 `latest_extract.status` 为 `done` **或 `failed`**（`scripts/bid_e2e_smoke.sh:125-132`）。后续 confirmed/rejected 保留和 superseded draft 断言有价值（`134-140`），但不能代替“本次重抽成功”。

**窄修复：**要求最新 manual run 为 `done`，并断言 extractor mode、run/generation 已变化、diagnostics 存在及新 draft 数量符合预期；失败必须打印 `error_message` 后退出。

### 5. Compose readiness 会在对象存储和完整业务链不可用时显示 healthy

Compose 默认给 API/worker 配置 MinIO endpoint/bucket（`deploy/docker-compose.yml:10-17`），对象写现在强制执行本地写及 S3 PUT（`crates/storage/src/lib.rs:23-39`）。但 API/worker 的 `depends_on` 都没有 MinIO；API 只依赖 PostgreSQL/Redis，worker 还依赖 DocReader（`deploy/docker-compose.yml:126-166`）。

与此同时：

- `/health` 只返回静态 JSON，不检查 PostgreSQL、Redis、MinIO 或 worker（`crates/api/src/routes.rs:276-281`）。
- Compose API healthcheck 使用该静态 endpoint（`deploy/docker-compose.yml:145-150`）。
- worker 没有 Compose healthcheck。
- 部署文档在 `up -d --build` 后只建议调用 `/health`（`deploy/README.md:8-16`）。

因此 MinIO 尚未 ready或已宕机时，API 仍为 healthy，而第一个上传会失败；worker/完整业务消费也不可由 Compose readiness 判断。

**窄修复：**

- 增加 `/ready`，检查 PostgreSQL、Redis、配置后的 S3 bucket操作以及 worker heartbeat。
- API/worker 对 MinIO 使用 `condition: service_healthy`，或明确将 S3设为可选且不默认配置。
- 为 worker 增加 healthcheck。
- Compose 和 smoke 等待 `/ready`，保留 `/health` 仅作 liveness。

### 6. 默认“一键部署”仍是开放登录和固定 JWT secret

Compose 默认 `JWT_SECRET=change-me` 且 LDAP URL 为空（`deploy/docker-compose.yml:7-10`）；API 在 LDAP URL 为空时忽略密码并 find-or-create 用户（`crates/auth/src/lib.rs:33-40`，`crates/api/src/routes.rs:523-539`）。虽然文档已经警告，但默认仍将 API 发布到主机端口。

**窄修复：**提供显式 dev profile；非 dev/production profile 在 `change-me`、空 LDAP 或空显式本地模式开关时启动失败。不能仅依赖文档警告。

---

## Medium

### 7. Booklet/preview 的 smoke 断言偏弱，Web preview 与 preview API 是两套未对齐合同

Smoke 的 booklet 主断言是手工 PUT 一段 Markdown 后 GET 回来（`scripts/bid_e2e_smoke.sh:113-117`）。PUT 在 part 不存在时会间接先生成 part（`crates/bid/src/booklet.rs:432-447`），export 也会确保其它 part，但脚本没有断言自动生成的 ①～⑤ 内容、must anchor、商务 miss 或技术段内容。

`/preview` smoke 只断言 `project_id`（`scripts/bid_e2e_smoke.sh:118-119`），任何缺 clauses/picks/commercial 内容的退化响应仍能通过。

Web 的“预览”实际上直接渲染 booklet Markdown（`web/src/bid/BookletPane.tsx:33-43`），没有调用 `/api/v1/bids/{id}/preview`。更明显的是旧 `#/bids/{id}/preview` 路由会被 Workbench 重定向到 `pane:"draft"`，即编辑页，而不是预览页（`web/src/hash.ts:42-57`，`web/src/bid/Workbench.tsx:104-108`）。

**窄修复：**

- Smoke 调用 regenerate，断言自动生成 part key、技术确认文本、must anchor和商务缺件内容。
- Preview 断言 clauses/commercial/coverage 等关键字段。
- 明确一个预览合同：若 UI 预览 booklet，则删除/重命名 JSON preview API；若保留 API，则 UI 应消费它。
- legacy `/preview` 应重定向到 `pane:"table"` 而不是 draft。

### 8. Redis/PostgreSQL 恢复仍只有静态机制，没有故障注入证据

worker Redis 连接中断后会指数退避重连（`crates/worker/src/main.rs:30-53`）。PostgreSQL中的 pending intent 会由 housekeeping 重投 convert/extract/section retry/match（`crates/worker/src/consume.rs:1439-1488`）。

但 housekeeping 每五分钟运行，stale threshold 为通用文档超时加十分钟，即约 2 小时 10 分钟（`crates/runtime/src/lib.rs:41-47`）。Smoke/CI 未重启 Redis 或 PostgreSQL；处理中断、旧 lease fencing、恢复耗时均未被端到端证明。

**窄修复：**增加短阈值测试配置和 fault-injection job，分别在 convert/extract/match/Section retry 中断 Redis、PostgreSQL，断言重连、重投、旧 owner fencing 和最终成功。

### 9. CI 仍没有验证完整 Compose 和 DocReader 镜像

CI 实际 build Rust runtime image，但只解析 Compose 配置，没有 `compose up`；DocReader Dockerfile也没有单独 build gate（`.github/workflows/ci.yml:76-96`）。Python单测不能证明 `deploy/Dockerfile.docreader:1-20`、其 apt/uv frozen install 和 Compose healthcheck能在部署环境启动。

**窄修复：**增加 DocReader image build；增加最小 Compose profile 启动并检查 API `/ready`、worker、DocReader gRPC和 MinIO roundtrip。

### 10. UI 只有 TypeScript/Vite build，没有浏览器用户流验证

`web/package.json` 只有 dev/build/preview，无 Vitest、Playwright或 Cypress业务测试（`web/package.json:4-10`）。CI 的 web gate只是 `npm ci && build`（`.github/workflows/ci.yml:88-90`）。因此 rejected恢复、retry状态、end只读、pick/shot、preview/export点击都只是静态代码存在，未证明浏览器中可操作。

**窄修复：**增加一个 Playwright deterministic flow，覆盖上传、编辑、驳回/恢复、确认、retry、成稿编辑/预览、结束后禁写和下载。

---

## 逐阶段最终分类

| 阶段 | 当前分类 | Round-3 证据与边界 |
|---|---|---|
| upload | **真实 service-backed，已间接断言** | 真实 multipart API、DB intent、Redis worker；下游草稿证明上传可消费。 |
| blob | **真实本地存储，已间接断言** | 每次使用全新 `TMP/objects`；worker能转换说明真实写读。MinIO未运行/未断言。 |
| convert | **真实 service-backed，确定性 simple reader** | Markdown走 in-process simple converter（`crates/docparser/src/lib.rs:153-192`），不是 DocReader。 |
| multimodal | **确定性 skipped，未直接断言** | 无图片且无 VLM，转换设置 `skipped`（`crates/bid/src/lib.rs:575-585`）；没有OCR/caption。 |
| auto extract | **真实 service-backed，已断言** | worker + heuristic engine + PostgreSQL，要求至少3条 draft及 run done。 |
| diagnostics | **真实持久化，部分断言** | 只断言 candidate span数；未检查 fallback/uncovered/逐文档内容。 |
| text edit | **真实 API 被调用，持久化结果未断言** | PATCH成功，但GET未比较新文本。 |
| family | **未证明发生编辑** | technical写回technical，可能是 no-op。 |
| must | **未证明发生 toggle** | “必须”原句很可能原本即true。 |
| reject | **真实 service-backed，已断言** | rejected读回，重抽后保持；恢复草稿只在UI/API代码存在。 |
| confirm | **真实 service-backed，基本已断言** | 技术确认读回；商务PATCH成功但未单独读回完整字段。 |
| technical match | **真实作业被调度，仅终止被断言** | `failed`也会通过；无候选输出断言。使用hashed embedding边界。 |
| commercial match | **真实作业被调度，结果未断言** | 无公司资产时预期miss，但脚本未检查。 |
| pick/unpick | **未证明** | API/UI存在；无种子candidate，smoke不调用。 |
| shot upload/read/delete | **未证明** | API/UI存在；依赖pick，smoke不调用。 |
| booklet generation | **真实 service-backed、间接执行，内容未证明** | PUT/export会触发 ensure part；只验证人工稿回读和文件签名。 |
| booklet edit | **真实 API已断言** | PUT/GET人工Markdown roundtrip。浏览器编辑未验证。 |
| preview API | **真实 API被调用，弱断言** | 只检查project_id。 |
| UI preview | **静态实现，未浏览器验证** | 本地GFM渲染，不消费preview API；legacy路由语义错误。 |
| DOCX export | **真实 service-backed，已做签名断言** | 断言ZIP `PK`；未检查正文/图片。 |
| PDF export | **真实 service-backed，已做签名断言** | host装CJK字体并断言`%PDF`；容器build gate存在但未容器内调用。 |
| manual reextract | **真实作业和不变量已断言，成功未证明** | 允许最终failed后继续。 |
| Section retry | **真实 service-backed，done已断言** | durable retry job经worker到done；重抽内容变化未检查。 |
| end | **真实 service-backed，部分断言** | end成功并验证一个PATCH返回409；未穷举其它mutation。 |
| Redis recovery | **有静态机制，未故障注入** | worker重连/intent重投存在；未stop/start验证。 |
| PostgreSQL recovery | **有pool/lease机制，未故障注入** | 处理中断恢复和耗时未验证。 |
| strict Agent | **未证明，外部集成** | smoke强制heuristic；agent模式要求tool-capable模型（`crates/bid/src/extraction/mod.rs:323-328`）。 |
| VLM | **未证明，外部集成** | 无图片、无VLM endpoint。 |
| DocReader | **未证明，外部/部署集成** | smoke只传Markdown；PDF/Office gRPC未执行。 |
| LDAP | **未证明且当前LDAPS实现阻塞生产** | smoke走local-open；无真实目录测试。 |
| real embeddings | **确定性 stub** | URL为空时使用hashed bag-of-tokens（`crates/index/src/lib.rs:8-38`）；真实模型未调用。 |

---

## 对“完整真实跑通业务流程了吗？”的中文结论

**没有完整真实跑通。**

更准确的说法是：

> 已经真实跑通了一条明显扩展后的、确定性后端核心流：  
> local-open登录 → 建标 → Markdown上传/本地blob → Redis worker转换 → heuristic自动抽取及基础diagnostics → 条款PATCH/驳回/确认 → 匹配作业进入终态 → Section重抽done → 人工成稿保存 → preview API调用 → DOCX/PDF生成 → 手动重抽不变量 → 结束后局部只读。

但不能称为“完整真实业务流程”，因为：

1. 技术/商务匹配只证明“不再运行”，没有证明成功和结果；
2. family/must/text编辑断言存在明显空洞；
3. manual reextract允许失败后PASS；
4. pick、shot、真实生成成稿内容和浏览器流没有跑；
5. Redis/PostgreSQL恢复没有故障注入；
6. strict Agent、真VLM、非Markdown DocReader、真embedding和生产LDAP都未运行；
7. LDAP/LDAPS实现及默认开放认证仍阻塞安全生产部署；
8. Compose readiness不能代表MinIO、worker和完整业务链可用。

因此可对外表述为：

> **“确定性核心后端流程已大部分 service-backed 跑通；外部模型、文档解析、目录认证、真实知识资产匹配和生产恢复链仍未完整验证，不能宣称生产级完整真实跑通。”**