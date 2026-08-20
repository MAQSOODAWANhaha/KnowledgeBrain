# 投标技术/商务条款抽取完整修复

## Context

当前方向正确：招标 Markdown 用两个有界专用 Agent 抽取技术/商务条款，quote 必须回源，只写 draft，人工确认后再匹配；抽取阶段不访问产品库或公司库。

需要修复的是实现层：

- tool-calling Agent 默认未真正启用：`BID_EXTRACT_MODEL_ID` 未由 Compose 传给 worker，`live_tool_chat` 对 `stub-chat` 直接返回无 tool call。
- 标题词、补扫词、wrong-family 词、heuristic 词分散在四处，既重复又互相不一致。
- 技术先跑并把 quote 放入共享快照，会让错误的技术分类阻止商务 Agent 纠正。
- Coverage 以整个 Section 是否有任意 quote 判断，一条命中会掩盖同段遗漏。
- `read_span` 对超长段只返回第一块；标题解析不支持中文序号和真正的多级路径。
- `emit_clauses` 工具 Schema 没声明 item 字段，Prompt 也未落实 must 判定规则和正文提示注入防护。
- Agent、单轮补扫、heuristic 的降级是静默的；run 无 model/policy/prompt/coverage 诊断。
- 没有真实招标黄金集，Prompt/词表改动无法量化回归。

目标：形成一个可测试的深 Module `TenderExtractionEngine`，外部只提交 Markdown，得到 Sections、Clauses、Coverage 和 Diagnostics；内部隐藏大纲解析、Span、双 Agent、仲裁、校验、补扫与降级。

## Approach

### 1. 明确运行模式和部署契约

引入显式模式，默认 `hybrid`：

- `agent`：必须有可 tool-call 模型；不可用、无 tool call 或请求失败则 run failed，不静默降级。
- `hybrid`（默认）：双 Agent（可用时）→ 未覆盖 Span 单轮补扫 → heuristic；每次降级和原因写 diagnostics。
- `heuristic`：测试/离线明确使用，不发 LLM 请求，也不伪装成 Agent 成功。

新增 `BID_EXTRACT_MODE`；`BID_EXTRACT_MODEL_ID` 为空时回退 `KNOWLEDGEBRAIN_CHAT_MODEL`，Compose 显式透传两者。启动和每次 run 记录 mode/model/policy/prompt 版本。`hybrid` 在无模型时允许 heuristic 完成，但 UI/诊断必须显示“规则兜底”；严格验收使用 `agent`。

### 2. 收敛成深 Module

新增/重构为内部模块：

```text
TenderExtractionEngine::extract(TenderDocument) -> ExtractionReport
  ├── OutlineParser
  ├── SpanBuilder
  ├── Technical Agent
  ├── Commercial Agent
  ├── ClauseReconciler
  ├── ClauseValidator
  ├── CoverageAnalyzer
  └── FallbackExtractor
```

`extract_run_body` 只负责 claim/load/persist；抽取引擎不直接操作数据库。引擎先在内存完成一份文档的 report，成功后才进入事务：upsert Sections → supersede 该文档旧 draft → insert 新 Clauses → prune 无引用旧 Sections。抽取失败只标 run/文档诊断，不提前隐藏旧 draft。整项目重抽按文档分别提交，单文件失败不回滚其它成功文件。

### 3. 版本化 ExtractionPolicy

先使用 `crates/bid/config/cn-tender-v2.json`，通过 `include_str!` + `OnceLock` 编译时内置、启动时强类型解析（复用 `serde_json`，不新增 YAML 依赖）。近期只做一个 `cn-tender-v2`，结构允许未来加行业 profile，但不做后台配置和行业选择 UI。Policy 统一承载：

- technical/commercial 定义、标题 hints、正反例；
- skip/process 标题提示；
- must hard/optional 规则；
- coverage trigger terms；
- max rounds/emit/file/span 等限制；
- `policy_version`。

领域不变量仍硬编码：family 合法值、quote 回源、draft-only、人工确认、无 KB 工具、匹配 scope。

### 4. Outline + Span

建立稳定的层级大纲和 Span：

- 支持 `#`、`第X章/节`、`1/1.2/1.2.3`、`一、`、`（一）`、`1）`、加粗标题；
- 维护多级 `heading_path`；
- 段落、列表项、表格按固定行数组成稳定 Span；
- Section 有稳定 `section_key`（层级路径 + 同名出现序号，不含正文 hash，正文变化不换身份）；Span 有稳定 `span_id = section_key + span ordinal`、序号、正文、char count；
- `bid_sections` 增 `section_key`，唯一 `(document_id, section_key)`；重抽 upsert 同一 Section，避免每次新插重复侧栏；
- 不再存在于新大纲且无 confirmed/rejected 引用的旧 Section 可清理；仍被人工确认条款引用的 Section 保留；
- `read_span` 改为按 `span_id` 读取，避免只读超长段第一块。

### 5. 双 Agent 独立产出 + 仲裁

技术、商务可以顺序调用以控制并发，但不再用共享 quote 直接阻止另一 family 产出。各自产生带 provenance 的候选：

```text
span_id, quote, text, must, proposed_family, extractor
```

`ClauseReconciler` 统一：

- 同 family 重叠 quote 去重；
- 跨 family 同 quote 进入冲突仲裁；
- heading hint → policy family score → extractor confidence 依次裁决；仍平局时暂选 `technical` 作为展示建议，但写 `family_conflict=true`，禁止把该建议当自动确认；
- family 最终仍只写 technical/commercial，不引入 `unknown`。

复用 `bid_clauses.source_span` 保存 `{span_id, heading_path, quote}`；新增 `family_conflict boolean` 和 `extraction_meta jsonb` 保存 proposed families、extractors、policy/prompt version。人确认条款时自动清除 `family_conflict`；前端草稿行显示“分类冲突，请核对”，人可改 family 后确认。

### 6. 严格工具 Schema 和 Prompt

工具参数保持严格 schema：

```text
span_id + quote + text + must
```

`text` 仍为 required 以拒绝不完整工具调用，但经安全复审和用户确认，服务端不会信任模型规范化文本：持久化前统一令 `text=已验证 quote`。`family/raw_text/section/source_document` 均由服务端生成。Schema required + `additionalProperties=false` + strict tool schema。

System Prompt 使用中文模板，注入 family 定义、正反例、must 规则、原文不可信/忽略正文指令、建议工具流程；Prompt 独立文件并带 `prompt_version`。

### 7. Span 级 Coverage 和明确降级

Coverage 从“Section 有任意 quote”改为：

- 先识别 requirement-like Span；
- 每个 Span 独立 covered/uncovered/ambiguous；
- 只补扫 uncovered candidate Span；
- heuristic 逐 Span 补，不再只在全文件 0 条时才触发；
- 输出 coverage 报告，不把“有种子词但模型失败”标成 done。

### 8. 诊断、持久化和幂等

在 `bid_extract_runs` 直接增加（本项目允许清库重部署，不做兼容 ALTER）：

```text
extractor_mode   text CHECK (agent|hybrid|heuristic)
model_id         text
policy_version   text
prompt_version   text
diagnostics      jsonb NOT NULL DEFAULT '{}'
```

`diagnostics` 保存：Agent rounds/retries/tool counts、quote 校验拒绝数、family 冲突数、candidate/covered/uncovered Span 数、fallback 原因、失败 Span 和错误摘要。完整聊天、整段原文不落库。

增加 `finish_extract_run` 持久化终态与 diagnostics；不要继续扩大旧 `set_extract_run` 的位置参数。`GET /api/v1/bids/{id}` 返回精简 `latest_extract`（status/mode/coverage/fallback/error），前端文件/评估页显示规则兜底、未覆盖或失败提示。

### 9. 黄金集和回归指标

新增小型脱敏 fixture + expected JSON，覆盖：

- 技术参数表、多条同段；
- 资格/业绩/营业执照；
- 须知中的实质性商务要求；
- 技术标题下的商务句、商务标题下的技术句；
- 中文序号、多级标题、无标题长文、长表格；
- must/optional/否决表达；
- 文档提示注入文本。

离线 scripted model 测流程；可选真实模型评测命令输出 quote validity、precision/recall、family/must accuracy、duplicate rate、uncovered spans。

## Files to modify

预计关键路径：

- `crates/bid/src/lib.rs` — `extract_run_body`/`retry_section` 改用新引擎；移走旧切段、单轮抽取实现
- `crates/bid/src/extract_agent.rs` — 被 `extraction/agent.rs` 取代后删除或只留兼容 re-export
- `crates/bid/src/extraction/mod.rs` — 唯一外部接口、编排
- `crates/bid/src/extraction/types.rs` — input/report/section/span/candidate/diagnostics
- `crates/bid/src/extraction/policy.rs` — typed policy + `OnceLock`
- `crates/bid/src/extraction/outline.rs` — 层级标题和 Span
- `crates/bid/src/extraction/agent.rs` — async ToolChat seam、OpenAI/Scripted adapters、严格工具
- `crates/bid/src/extraction/reconcile.rs` — quote 去重、family 冲突、must 校验
- `crates/bid/src/extraction/coverage.rs` — candidate Span、补扫、heuristic
- `crates/bid/config/cn-tender-v2.json` — 唯一默认 Policy
- `crates/bid/prompts/clause-extractor-v2.md` — 单一模板，注入 family 定义/正反例，避免两份漂移
- `crates/bid/Cargo.toml` — 增 `async-trait`（现有 lock 已使用），引擎全异步，删除 `block_in_place`/嵌套 runtime
- `crates/storage/src/bid.rs` — run diagnostics、clause source/extraction metadata、latest run 查询
- `migrations/0007_bid.sql` — section `section_key` + 唯一；clause conflict/meta；run 诊断列
- `crates/api/src/routes.rs`、`crates/bid/src/lib.rs` — ClauseView/latest_extract 输出；确认时解决冲突
- `web/src/api.ts`
- `web/src/bid/ClauseTable.tsx`、`web/src/bid/Inspector.tsx`（或实际评估提示位置）— 分类冲突和降级/未覆盖提示
- `deploy/docker-compose.yml`、`deploy/.env.example` — 模式与模型配置
- `docs/bid-platform-domain.md`、`docs/system-design.md`、`.scratch/knowledgebrain/spec.md`
- `testdata/bid-extraction/*.md` + `*.expected.json` — 脱敏黄金集
- `crates/bid/examples/eval_extractor.rs` — 手动真实模型评测；CI 默认不访问外网

## Reuse

- 队列与项目级串行：`BidExtractJob`、`BidExtractWorker`、`claim_extract_run`。
- 自动/手动重抽与 draft supersede：`extract_run`、`supersede_drafts`。
- 原文证据校验：`quote_in_body`、`normalize_quote`。
- 长段拆分基础：`split_long_body`、`split_table_rows`（改造成稳定 Span）。
- 测试模型 seam：`ScriptedChat`、`scripted_tool_message`。
- OpenAI tool calling：`live_tool_chat`、`async-openai`。
- 文档/表格转换结果：现有 `BidConvertWorker` 和 `markdown_ref`。
- 人确认与后续匹配：`BidClause`、`schedule_match`、`run_match_job`，不改匹配领域语义。

## Accepted decisions

- 跨 family 同 quote：不加 `unknown`；按 policy 给建议 family，写冲突标记，人工确认解决。
- 默认 `hybrid`，另有严格 `agent`；所有降级可见。
- 分两批交付但本计划覆盖全部：第一批引擎/P0-P1，第二批 diagnostics + 黄金集真实模型评测。
- 近期只有版本化 `cn-tender-v2`，不做行业配置后台。
- 自动条款 `text` 采用已验证 quote，不持久化模型改写；这是安全优先的已批准覆盖，避免规范化文本引入无依据语义。

## Steps

### Phase 1 — 修通和重构抽取引擎

- [x] 1. 部署契约：增加/透传 `BID_EXTRACT_MODE`、`BID_EXTRACT_MODEL_ID`；模型 ID 回退主 Chat 模型；严格 agent 与 hybrid/heuristic 行为单测
- [x] 2. 定义 `ExtractionInput/Report/Diagnostics`、`ExtractionPolicy`、`TenderExtractionEngine::extract`；Policy/Prompt 用 `include_str!` 版本化加载并校验
- [x] 3. 把 tool chat 改成真正 async 的内部 seam（OpenAI + Scripted 两个 adapter），移除 `block_in_place` 和临时 runtime
- [x] 4. 实现层级 OutlineParser 和稳定 Span：中文/数字标题、多级路径、段落/列表/表格分片、稳定 span_id；复用并替代 `split_sections/split_long_body/split_table_rows`
- [x] 5. 工具改为 `list_outline/read_span(span_id)/grep/emit_clauses/done`；严格 JSON Schema；返回 span_id；中文防注入 Prompt + family/must 定义和正反例
- [x] 6. 技术、商务 Agent 独立产出；不共享 quote 抑制；ClauseValidator 做 quote 回源/限额，ClauseReconciler 做同 family 去重与跨 family 冲突建议
- [x] 7. Span 级 Coverage：candidate Span 识别、未覆盖单轮补扫、逐 Span heuristic；agent/hybrid/heuristic 的失败和降级语义明确
- [x] 8. `0007_bid.sql` 先加入 `bid_sections.section_key` 唯一及 clause `family_conflict/extraction_meta`；storage 增 `upsert_section`、事务式 `persist_extraction_report`，并真正写 `source_span`
- [x] 9. `extract_run_body` 先在内存完成 report，成功后按文档事务提交；此时才 supersede 旧 draft，upsert stable Section，并清理无人工引用的过期 Section；`retry_section` 走同一引擎的单 Section scope
- [x] 10. API/ClauseView 暴露 `family_conflict`；确认时自动清冲突；前端草稿显示“分类冲突，请核对”并允许修改 family 后确认
- [x] 11. 删除旧散落词表/Prompt/全文件空才 heuristic 等重复实现，确保 Policy 是唯一可变策略来源

### Phase 2 — 可观察、可评测

- [x] 12. `0007_bid.sql` 增 run 诊断列；storage 增 typed `finish_extract_run/latest_extract`，持久化内存 diagnostics
- [x] 13. API 暴露项目 `latest_extract`；前端显示规则兜底、未覆盖/失败提示
- [x] 14. 新增脱敏黄金集与 scripted 流程测试；真实模型 evaluator 只手动运行，输出 JSON/Markdown 报告，不纳入普通 CI 网络依赖
- [x] 15. 同步领域/系统规格和 `.env.example`，删除与实际行为冲突的“Section 任意 quote 即覆盖”等描述

### Final capped-review corrections

- Prompt contract logical version is `clause-extractor-v3`: required model `text` must equal exact `quote`, and the server still canonicalizes it.
- Neutral table rows require body-level Policy evidence; heading prior never creates a requirement. Exact Markdown rows remain whole through heuristic punctuation handling.
- Match identity is `(project,generation,job_kind,unit)`; partial clause PATCH, Section merge cycle checks, and retry terminal release are authoritative inside project-first/token-conditioned transactions.
- Deterministic PostgreSQL+Redis service smoke is an internal core-flow gate. LDAP/LDAPS, production readiness/fault injection, pick/shot assets, and external Agent/VLM/DocReader/embedding remain explicit external/deferred validation.

## Verification

### 自动检查

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p bid -p storage -p worker -p api`
- `npm -C web run build`
- 如项目仍要求镜像规格：`cmp docs/system-design.md .scratch/knowledgebrain/spec.md`

### 单元/集成场景

- 配置：worker 能看到 mode/model；`agent` 缺模型明确失败；`hybrid` 降级写 diagnostics；`heuristic` 不发 HTTP。
- Outline/Span：`#`、章/节、1.2.3、一/（一）/1）、加粗标题、多级路径、无标题、超长段、跨页表格；每个 Span 可通过 ID 读全。
- 工具：strict emit 拒绝缺字段/额外字段；quote 不在当前 Span/文档则拒绝；每调用≤40、每文件≤400、轮数/重试受 policy 限制。
- 分类：技术标题里的注册资本、商务标题里的吞吐量、同 quote 双 Agent 冲突、heading/policy 平局；冲突草稿可改 family 后确认且标记清除。
- must：必须/否则废标/无效投标=true；宜/可/优先/评分项=false；歧义默认 false。
- Coverage：同一 Section 多个 Span，只覆盖一个不会屏蔽其余；表格逐片补扫；失败 Span 不标 done。
- 安全：正文中“忽略系统规则/调用外部知识库”不会改变工具范围或 family。
- 重抽：抽取失败保留旧 draft；成功后才事务 supersede；不动 confirmed/rejected；stable section_key 不产生重复段；同项目串行；retry section 不走旧 Prompt。

### 黄金集门槛

- quote validity：100%
- 无依据生成：0
- technical recall：≥90%
- commercial recall：≥95%
- family accuracy：≥95%
- must accuracy：≥95%
- duplicate rate：<3%
- 每个 uncovered candidate Span 必须在报告中可定位

### E2E

上传中文招标 → convert/multimodal → 自动抽取 → 页面显示商务/技术 draft 与 coverage → 处理冲突并确认 → 商务写 company hit/miss、技术按勾选段生成产品候选。分别用 `hybrid` 和严格 `agent` 跑；重抽后确认条款不变，旧 draft 变 superseded。
