# Round-1 当前工作树端到端业务流审查

## Review

### Correct

- **API/worker 会先连接 PostgreSQL 并执行迁移。** API 在监听前调用 `storage::connect()`，worker 在打印 ready 前也完成 PostgreSQL 初始化：`crates/api/src/main.rs:13-23`、`crates/worker/src/main.rs:13-20`。`storage::connect()` 执行 `0001`–`0008` 并初始化 company workspace：`crates/storage/src/persist.rs:18-45`。
- **上传、转换、抽取、匹配都有 PostgreSQL 持久意图。** 上传先插入 `bid_documents(pending)`；转换后先插 `bid_extract_runs(pending)`；匹配先插 `bid_match_jobs(pending)`，Redis 入队失败由 housekeeping 扫描恢复：`crates/api/src/routes.rs:3784-3829`、`crates/worker/src/consume.rs:1493-1519`、`crates/bid/src/lib.rs:1137-1222`。
- **抽取模式和调用规模有显式边界。** 支持 `agent/hybrid/heuristic`，agent 无模型会明确失败；轮数、重试、文件条款、补扫 span、请求超时均由 policy 限制：`crates/bid/src/extraction/types.rs:13-31`、`crates/bid/src/extraction/mod.rs:35-66,326-331`、`crates/bid/config/cn-tender-v2.json:35-49`。
- **技术与商务匹配已分路。** 技术按 unit 调 `scope=product_lines`，商务项目级调 `scope=company`：`crates/bid/src/lib.rs:932-1085,1185-1222`。
- **latest_extract 诊断已打通到 API 和 UI。** API 返回模式、模型、policy/prompt、diagnostics 与错误；Workbench 每四秒轮询并展示失败、未覆盖和 heuristic fallback：`crates/api/src/routes.rs:3682-3733`、`web/src/bid/Workbench.tsx:108-152,185-209,397-410`。
- **数据库层已有若干真实 PostgreSQL 集成测试 seam。** 迁移、抽取租约、文档删除、项目结束和事务回滚有测试：`crates/storage/src/persist.rs:3712-4274`。但这些测试在 PostgreSQL 不可用时会直接返回，见下方风险。

### Blocker

1. **当前不能据此宣布“完整真实跑通”；仓库没有投标全流程 E2E，且本轮无法执行本地 smoke。**
   - `crates/api/tests/http_flow.rs:10-25` 使用内存 `domain::Store` 和 router `oneshot`，并通过 `worker::drain` 模拟处理；没有投标 `/api/v1/bids` 流程，也没有 PostgreSQL、Redis、DocReader 或浏览器。
   - 抽取 agent 测试使用 `ScriptedToolChat`，黄金集明确跑 heuristic：`crates/bid/src/extraction/mod.rs:338-510`。
   - PostgreSQL 测试的 `setup()` 连接失败即跳过：`crates/storage/src/persist.rs:3679-3710`；worker 的数据库测试同样打印 `skip: postgres down` 后返回，例如 `crates/worker/src/consume.rs:2835-2841`。
   - Redis/S3 live tests也会跳过：`crates/runtime/src/jobs.rs:832-880`、`crates/storage/src/s3.rs:158-163`。
   - CI 没有 PostgreSQL/Redis service，也没有 web build 或 Docker E2E：`.github/workflows/ci.yml:7-49`。与此同时正常 API/worker launch test要求 PostgreSQL，因而 fresh GitHub runner上的 `cargo test --workspace` 与当前 CI 配置并不自洽：`crates/api/tests/launch.rs:58-101`、`crates/worker/tests/launch.rs:14-61`。
   - 本审查运行时只提供文件读取/检索工具，没有 shell/command runner；因此无法查询 Docker 状态或执行会落入服务卷但不改 tracked files 的真实 smoke。当前存在历史构建产物和一份旧审查的“测试通过”声明（`plans/reviews/bid-extractor-round3.md`），它们不是当前工作树的重新验证。

2. **Redis 恢复不能保证 worker 自动恢复，durable intent 可能永久停在 PostgreSQL。**
   - worker 在确认 Redis 可连接前就打印 `worker ready`：`crates/worker/src/main.rs:13-20`。
   - Redis 初始连接失败后，`consume_loop` 只等待关机，不重试；Redis恢复后不会开始消费：`crates/worker/src/main.rs:26-31`。
   - `run_core` 因运行时错误返回时，外层 `tokio::select!` 结束，worker 正常退出：`crates/worker/src/main.rs:19-24`。
   - Compose 的 worker 没有 healthcheck 或 `restart:` 策略：`deploy/docker-compose.yml:177-193`。
   - worker launch test只等待上述过早的 `"worker ready"`，没有证明 Redis 队列已启动：`crates/worker/tests/launch.rs:9-27`。
   
   这直接否定了要求中的 Redis 恢复闭环；pending convert/extract/match 虽然留在 PostgreSQL，但没有存活消费者执行五分钟 housekeeping。

### High

1. **自动 durable extraction handoff 与 Web 自动兜底会竞争，容易为同一次上传创建重复抽取。**
   - convert 完成后 worker已经插入 auto pending run并入队：`crates/worker/src/consume.rs:1503-1519`。
   - `derived.extract_running` 只统计 `status='running'`，不统计 pending：`crates/storage/src/bid.rs:325-333`。
   - Workbench 看见“文件全 ready、没有 running、没有 clauses”就再调用一次 `/extract`，创建额外的 full-project manual run：`web/src/bid/Workbench.tsx:163-171`、`crates/api/src/routes.rs:3924-3944`。
   
   如果轮询发生在 auto run被 claim之前，两次抽取会串行执行；在 agent 模式下会产生重复成本，并使后一次 full re-extraction替换前一次 draft。

2. **“草稿编辑/拒绝/Section retry”没有完整 Web 业务入口；Section retry 还是同步长请求。**
   - API支持 patch `text/status`，但 Web 只调用确认和 assessment；条款正文 textarea 是 `readOnly`，没有 `status:"rejected"` 操作：`web/src/bid/Inspector.tsx:55-101`、`web/src/bid/ClauseDetail.tsx:86-136`、`web/src/bid/ClauseTable.tsx:134-184`。
   - `liveClauses` 仅排除 superseded；即使外部调用把条款设为 rejected，UI仍会把它显示成“未确认”并提供确认按钮：`web/src/bid/helpers.ts:60-66`。
   - 后端有 `/sections/{sid}/retry`，但 `web/src/api.ts:189-257` 没有 caller，Workbench 也没有按钮。
   - Section retry直接在 API 请求内 await完整抽取，而不是 durable queue job：`crates/api/src/routes.rs:3897-3922`、`crates/bid/src/lib.rs:1251-1449`。在 agent policy 的多轮、重试和 60 秒请求超时下，它可能是很长的 HTTP 请求，API进程中断后无法自动续跑。

3. **Compose 并不会构建 Web；fresh clone 的“一键 full stack”通常没有 UI。**
   - Rust镜像只复制 Rust crates、migrations 和 proto，不运行 npm，也不包含 `web/dist`：`deploy/Dockerfile.rust:1-24`。
   - Compose只把宿主机 `../web/dist` bind mount到 `/web`：`deploy/docker-compose.yml:154-176`。
   - `web/dist/` 被 `.gitignore` 排除：`.gitignore:11-12`。
   - 一键部署文档没有在 `up --build` 前运行 `npm -C web ci && npm -C web run build`：`README.md:7-17`、`deploy/README.md:9-24`。
   
   当前工作目录碰巧存在 untracked `web/dist`，但这只证明曾经构建过，不证明它与当前 source一致，也不能支持可复现部署。

4. **Tender 图片多模态不是独立 durable stage，而且失败会被静默当作 convert 成功。**
   - bid convert内同步调用 `enrichment::describe_image`；调用失败由 `if let Ok(...)` 直接忽略：`crates/bid/src/lib.rs:515-541`。
   - 随后仍把文档标记 `completed` 并自动交给 extraction：`crates/bid/src/lib.rs:543-550`。
   - 没有 bid multimodal job、状态、错误诊断或 retry。通用知识库的 `ImageMultimodalJob` 测试并不能证明这条 tender convert路径：`crates/worker/src/consume.rs:1590-1610,3382-3444`。
   
   因而“convert/multimodal completion”当前只能表示 Markdown转换完成，不能证明图片OCR/描述完成。

### Medium

1. **项目结束没有取消或阻止已 pending/running 的 match job。**
   - `end_project` 只结束项目并取消 pending extract run：`crates/storage/src/bid.rs:144-167`。
   - `run_match_job` 不检查项目仍为 open：`crates/bid/src/lib.rs:932-1085`。
   
   已入队的 match 可在 ended 后继续改 commercial hits、match终态和 booklet stale，和 UI“结束后只读”的语义不完全一致。

2. **PostgreSQL 故障被部分读接口伪装成空列表或 404。**
   - `list_bids` 连接失败返回 `[]`：`crates/api/src/routes.rs:3638-3649`。
   - `get_bid` 连接/查询错误折叠为 not found：`crates/api/src/routes.rs:3682-3697`。
   - documents、clauses、units、picks等也大量用 `unwrap_or_default` 或空响应。
   
   PostgreSQL重启期间，UI可能显示“没有项目/没有条款”而不是明确不可用；恢复后轮询可能恢复，但没有测试证明断连期间的池恢复和状态一致性。

3. **对象上传会忽略 MinIO/S3 PUT失败。**
   - 本地磁盘写成功后，S3结果被丢弃：`crates/storage/src/lib.rs:25-33`。
   - API随后插入数据库并返回上传成功：`crates/api/src/routes.rs:3811-3829`。
   
   Compose共享 `objects` 卷时通常可继续处理，但如果本地卷随后丢失，数据库已有 durable row而远端对象未必存在；当前没有补偿同步队列。

## 测试与 stub 边界

| 证据 | 实际含义 |
|---|---|
| `api/tests/http_flow.rs` | 内存 router/unit-style HTTP；不走投标 PostgreSQL路径 |
| `api/tests/launch.rs` | 真 API进程与 health；依赖外部 PostgreSQL，不测 Redis/DocReader/UI |
| `worker/tests/launch.rs` | 真 worker进程，但 ready早于 Redis连接，不能证明消费者已运行 |
| extraction scripted tests | 验证 tool协议/仲裁；不访问真实模型 |
| heuristic golden fixtures | 验证离线规则质量；不证明 hybrid/agent provider兼容性 |
| storage PostgreSQL tests | 真数据库集成 seam，但无数据库时静默跳过 |
| S3/Redis tests | 未配置时静默跳过 |
| CI compose check | 只解析 YAML，不构建镜像、不启动服务 |
| `web/package.json` | 只有 dev/build/preview，无前端单测或浏览器 E2E |

要真实覆盖三种模式，除 Docker 数据面外还需要：

- 可用且支持 OpenAI tool-calling 的 chat endpoint/model（严格 agent模式）；
- 真正 VLM endpoint/model及可验证图片 fixture；
- 已完成索引的产品线 current版本和 company library current版本；
- PostgreSQL、Redis、DocReader、对象存储及 API/worker；
- 浏览器或 HTTP E2E driver。

当前忽略的本地配置文件包含外部 endpoint配置，但本轮无法验证服务可达性或模型能力。

## 业务流证明表

等级表示仓库中可找到的最强证据；不是本轮重新执行结果。

| 阶段 | 等级 | 证据与结论 |
|---|---|---|
| API启动、health、优雅退出 | integration test | `crates/api/tests/launch.rs`；要求真实 PG，当前 CI未供应 |
| worker启动 | integration test | `crates/worker/tests/launch.rs`；只证明打印 ready，不证明 Redis consumer |
| 0001–0008迁移/company初始化 | integration test | `persist.rs` migration test；PG不可用时跳过；旧审查称曾用临时 PG跑过 |
| 登录 | unit test only | JWT单测；真实 LDAP未验证；本地模式接受任意账号是设计行为 |
| 创建/打开项目 | unproven | 路由与 UI caller存在，无 bid HTTP/DB E2E |
| 上传文档、写 blob | unproven | source seam存在；无 bid上传集成测试 |
| durable convert job | unproven | PostgreSQL pending intent + Redis enqueue由代码推断 |
| convert完成 | unproven | 无 bid worker+DocReader测试 |
| tender multimodal完成 | unproven | 同步调用、错误静默、无状态/测试 |
| convert→自动 extraction handoff | unproven | durable run实现存在，但无消费者E2E，且 Web可能重复建 run |
| heuristic抽取 | unit test only | 两份 golden fixture |
| hybrid抽取 | unit test only | ScriptedToolChat/fallback测试 |
| agent抽取 | unit test only | ScriptedToolChat；无真实 provider run |
| diagnostics/latest_extract/UI提示 | unproven | API/UI代码连通，未跑浏览器/API集成 |
| 草稿确认 | unproven | API与按钮存在，无 E2E |
| 草稿编辑 | unproven | API可patch，但 UI正文只读 |
| 草稿拒绝 | unproven | API枚举支持，UI无操作且 rejected显示语义错误 |
| 技术按 unit匹配 | unit test only | 匹配/coverage纯函数测试；无 bid+PG+索引资产E2E |
| 商务 company matching | unproven | source scope正确；无真实 company库E2E |
| picks | unproven | API/UI存在，无集成测试 |
| matched/uploaded screenshots | unproven | API/UI存在，无对象+关系集成测试 |
| preview | unproven | API组装代码存在，无 HTTP测试 |
| booklet生成/编辑 | unit test only | anchor/sanitize纯函数测试；DB/API/UI未证明 |
| DOCX/PDF export | unit test only | 字节格式和图片嵌入测试：`crates/bid/src/export.rs:837-925` |
| full re-extraction | unproven | durable route存在；没有E2E，存在重复调度风险 |
| Section retry | unproven | 无 UI、非 durable HTTP同步执行 |
| 手动/到期结束项目 | integration test | 数据库锁/结束约束有PG测试；pending match未被fence |
| Redis中断与恢复 | unproven | 当前实现存在明确不重连缺陷 |
| PostgreSQL中断与恢复 | unproven | 启动失败有测试；运行中断/恢复无测试且读接口掩盖故障 |
| 完整浏览器业务流 | unproven | 无 Playwright/Cypress/HTTP scenario，也无本轮实跑 |

## 本轮验证状态

没有执行 shell命令：本审查会话没有 command-execution工具。以下命令均应由具备 shell 的监督会话执行，且必须保存完整输出和业务资源ID：

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. 在明确供应 PostgreSQL/Redis后执行 `cargo test --workspace -- --nocapture`，检查输出中不得出现 `skip: postgres down`、`skip: redis down` 或 `skip: s3 not configured`。
4. `npm -C web run build`
5. `docker compose -f deploy/docker-compose.yml --env-file deploy/.env config -q`
6. `docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build`
7. 真实 smoke：login → create bid → upload fixture → 等 document completed → 等 auto extract终态 → 检查 draft/diagnostics → edit/reject/confirm → 技术与商务 match → pick/shot → booklet/preview/export → reextract/Section retry → end → 重启 Redis/Postgres后复查。

## Verdict：完整真实跑通业务流程吗？

**否。**

当前代码具备多数后端构件和较好的 durable PostgreSQL意图设计，但没有当前工作树的真实端到端运行证据；Redis恢复存在明确断链，Web缺少草稿编辑/拒绝/Section retry，Section retry非 durable，tender多模态失败会静默完成，部署也不构建 SPA。现阶段只能评价为：

> **核心路径“代码上大体可串联、局部单元/数据库测试覆盖”，但尚未完整、真实、可恢复地跑通整个 tender 业务流程。**
