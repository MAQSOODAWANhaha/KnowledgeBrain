# 投标平台领域草案

| 项 | 值 |
|---|---|
| 状态 | **已按审查修订**（配套 `docs/system-design.md` 同步改；实现按本文 + 规格） |
| 日期 | 2026-08-19 |
| 仓库 | 同仓 `KnowledgeBrain`。部署沿用 Compose：`api` / `worker` / `docreader` + postgres/redis/minio。投标 HTTP 挂 `api`，作业挂 `worker` 新队列。不新开 Git 仓，第一期不新开业务容器 |
| 配套规格 | `docs/system-design.md`（Workspace / `/match` / 鉴权 / VLM 以本文为准） |

知识库回答：我方能卖什么、手册怎么写、公司有哪些证、服务能力和业绩案例。  
投标平台回答：这一标怎么拆、勾哪些产品组成解决方案、缺什么、预览/导出投标文件。

整套系统按**一家公司**理解：用户、产品线、公司资料、招标项目在同一个池子里，不再套 Org，也不再把 Workspace 当投标任务的父级。

---

## 0. 已拍板

| 决定 | 结论 |
|---|---|
| 我们是谁 | **应标（乙方）** |
| 一个项目 | 一份招标公告；多文件直接挂项目上；**无包件层** |
| 项目和产品线 | **无关**。产品线只是产品分类。一份标是跨产品线的**解决方案** |
| 部署 | 同仓、Compose 三进程。内网 |
| 鉴权 | **关掉注册**。登录走公司 **LDAP**（成功后发现有 JWT）。**不按 Workspace 成员做门闩** |
| 谁能看见 / 打标 | 任何登录用户（或已认证 API key）都能看见全部项目、匹配全部产品线、走完全部投标流程 |
| 多模态 | **必须开**（产品线 + 公司资料的 current）。真 VLM，禁止 chat URL 冒充 |
| 人 | 只要**负责人**（名字文本）。不存招标人/甲方 |
| 时间 | `expires_at` = 招标结束时间，用于跟踪 |
| 建项时不写 | `review_kind`、`allow_deviation`、`delivery_region`、`product_scope`、`version_scope`、完整状态机 |
| 项目里的文件 | **只放招标侧**。手册 / 证 / 业绩 / 服务 / 案例在知识库 |
| 后来又传文件 | 只追加；已确认不动；该文件 `index_ready` 后**自动**抽 draft |
| 条款 | 解析出初稿 → **人按段确认**。该段确认集变化后入队**该勾选段**的技术匹配；商务仍项目级。不在确认 HTTP 里同步打全库 |
| 技术 / 商务怎么判 | `TenderExtractionEngine` 内两个独立有界 Agent + Span 级覆盖、仲裁与可见 fallback。**按段扇出、增量落库**；覆盖不全标 partial，禁止假成功。标题只是 prior，不是门闩 |
| 推荐 | **按勾选段**排序，不宣布唯一最佳；人给该段勾选 1..N 个产品 |
| 勾选落下 | `(勾选段, product_id, version_id)` + 该段当时快照 |
| 解决方案 | 各勾选段已勾产品的并集。不另建方案表。不是全书一份排行榜 |
| 商务第一期 | 公司资料库**按条款找文档**。不做证过期/金额判断。**不参与产品打分** |
| 状态 | `open` / `ended` |
| 成稿目标 | **投标应答卷** ①～⑤ 过程中可编；导出渲当前稿。⑥ 以后。不是装订递交包 |
| 过程顺序 | **先商务、再技术段**。商务不做门闩。工作台上导航三步：文件 → 评估 → 成稿；侧栏跟步走本标树。可回头，不硬锁 |
| 缺了就补 | 都不卡死、不整表标红，给一条补的路径 |
| 公司资料 | **恰好一条** `kind=company` Workspace。分类夹是它下面的 `kind=library` Product（资质证照 / 体系认证 / 业绩案例 / 服务能力）。文档挂 ProductVersion。不用把 `workspace_id` 改成空 |
| `/match` | `scope=product_lines\|company`。带 `scope` 时不传、不推断 `workspace_id` |

---

## 1. 三个平级的根

```
（一家公司，LDAP 登录即可）
  ├── Workspace kind=product_line     产品线（型号手册、界面图）
  │     └── Product (kind=product)
  ├── Workspace kind=company          恰好一条
  │     └── Product (kind=library)    分类夹：资质证照 / 体系认证 / 业绩案例 / 服务能力
  │           └── Document            证、合同、案例扫描件（进向量，商务才能打到）
  └── BidProject
        ├── BidDocument[]
        ├── BidSection[]
        ├── BidExtractRun
        ├── BidClause[]               draft / confirmed / rejected / superseded
        ├── BidMatchJob               技术按勾选段；商务项目级
        ├── BidSectionPick[]          (project, 勾选段, product)
        ├── BidCommercialHit          按条款的文档命中，不是产品排行
        ├── BidShot[]
        └── BidBooklet                ① / 2:{段} / ③④⑤ 过程可编
```

**禁止**把招标文件写进任一 ProductVersion。  
**禁止**把某次投标人补的界面图写进产品手册。  
**禁止**再往产品线默认 library「公司资料」写入；存量迁到 company，商务不扫旧 library。

| 概念 | 落点 |
|---|---|
| 型号手册、功能界面图 | 产品线下 `kind=product` + Document |
| 执照、体系证、业绩/案例、服务能力证明 | company 下 `kind=library` + Document（Tag 再细分） |
| 发版 / 换证 | ProductVersion / `version:clone` |
| 这一标 | BidProject |
| 招标文件 | BidDocument（抽出的 convert 库，不进产品索引） |
| 条款 | BidClause → 技术按**勾选段** `scope=product_lines`，商务 `scope=company` |
| 解决方案 | 各段 BidSectionPick 并集 |
| 技术标截图 | 产品库命中图 → BidShot；不足人补仍是 BidShot |

company 下 Product 只是**分类夹**，不是互相竞争的型号。人不强制种子数据，进系统后自建「资质证照 / 体系认证 / 业绩案例 / 服务能力」即可。

---

## 2. BidProject

```
BidProject
  id
  title                 项目名称
  owner_name            负责人（文本，非账号，不当权限）
  expires_at            招标结束时间（跟踪用）
  status                open | ended
  ended_at              可空；人手结束或过了 expires_at 后写入
```

- 列表对全体登录用户可见。负责人只用于跟踪/筛选，不是 ACL。
- `expires_at` 到了可以自动标 `ended`（worker 定时扫），人手也可提前结束。结束在项目行事务内 fencing/cancel pending/running convert、extract、Section retry 与 match intent；旧 token 的 heartbeat/发布均失败。结束**不**删条款、不改已锁版本。
- 没有 `workspace_id`、甲方、包件、评分办法、产品范围。

页面派生（不当状态枚举）：是否有文件、文件是否 `index_ready`、抽取是否在跑、是否有未确认 draft、匹配作业是否在跑、是否已勾选产品、是否有文件尚未纳入条款。

---

## 3. BidDocument

```
BidDocument
  id
  project_id
  file_name
  file_hash
  file_size
  object_key            原件，objects/{sha256}，失败重试还靠它
  parse_status          pending | processing | completed | failed
  markdown_ref          解析出的 Markdown（项目私有对象），completed 才有
  parsed_at
  error_message
```

- 不标 `kind`，不作废链。补遗 = 再传一个文件。
- **解析统一，入库分开。** 知识库文件与招标文件走同一套 convert：同一 `convert_to_markdown`、同一默认引擎表、同一 VLM 写回。**禁止**按「这是招标」另选引擎或另写 OCR。分叉只在落盘：
  - 公司资料：Document 管线（必须进 `chunk_embeddings`，否则商务打不到）。
  - 招标文件：同一套 markdown+图 只写入 `BidDocument`。不 `INSERT documents`，不进产品索引。两边 **不**共用知识库的 `parse_status` / `enable_status`。
- 招标 `parse_status=completed` **只**表示 convert +（无图或 multimodal 写回）完成，等价于该文件的 `index_ready`。禁止拿一堆 `![](images/…)` 去切条款。
- **失败就补**：该文件可重试（同一 `object_key` 再跑），可删掉重传。一份 `failed` **不**挡住其它已完成文件去抽条款。
- **按文件自动抽。** 该文件 `completed` 后入队切段+抽取，draft 追加。后传同样：解析完就抽，不和已确认条款打架。
- 不搞 `upload_batch_id`，不等「整批传完」。
- 「重新解析」：整项目按当前全部 `completed` 文件重抽 draft（已确认 / rejected 不动）。

案卷对象前缀：招标原件、人补图、手册抽图一律 `objects/{sha256}`，与产品共用去重。

---

## 4. 条款怎么来

### 4.1 BidClause

```
BidClause
  id
  project_id
  extract_run_id        哪一次抽取产出的；手补为空
  section_id            来自哪一段；手补为空
  source_document_id    可空
  source_span           span_id + heading_path + quote；手补可空
  family_conflict       bool；跨 family 无法确定时为 true
  extraction_meta       policy / prompt / extractor provenance
  raw_text              招标原文
  text                  自动抽取时固定为校验通过的连续原文 quote；人工编辑后用这个去匹配
  family                technical | commercial
  must                  bool
  status                draft | confirmed | rejected | superseded
  deviate               bool，默认 false；人标偏离后为 true，不再被自动改掉
  deviate_note          仅 deviate=true
  confirmed_at
  superseded_by_run_id  仅 superseded：被哪一次新抽取替代
```

- 抽取只产生 `draft`。`confirmed` 必须人点一次（可改 text / family / must）。
- `rejected`：人丢掉的初稿（目录行、重复、不是需求）。
- `superseded`：该文件新 report **抽取成功并进入事务提交时**，上一 run 仍为 `draft` 的条款才改为此状态，默认隐藏。抽取失败保留旧 draft；人仍可打开旧结果并确认。不覆盖旧 confirmed / rejected。
- **未确认不得进入匹配**（`draft` / `superseded` / `rejected` 都不进）。
- `family=technical`：打全部产品线的 `kind=product`。
- `family=commercial`：打 company 资料库；**不参与产品排序**。
- 第一期没有 `procedural`、没有 scored/weight、没有作废链。

### 4.2 技术 / 商务怎么判

**不要**开通用闲聊 Agent 对着整份招标书或知识库一直聊。
**不要**在抽取阶段打 `/match`、产品库、公司库。

允许：`TenderExtractionEngine` 内的**两个有界专用抽取智能体**（技术、商务）+ Span 级确定性覆盖检查。标题只提供 prior，不决定抽不抽。所有可调策略来自编译进制品的 `crates/bid/config/cn-tender-v2.json`；系统提示来自 `crates/bid/prompts/clause-extractor-v2.md`。family 枚举、quote 回源、只写 draft、人确认门闩和知识库隔离仍是 Rust / DB 硬约束。

知识库 `tender_text` 先 chat 再按行兜底、**没有 family**，不能当投标拆条。投标平台自己抽，只把已确认条款的 `text` 交给 `/match`。

```
每个 BidDocument：completed（convert + multimodal）
  → OutlineParser：层级 BidSection + stable section_key
  → SpanBuilder：要求级段落 / 列表 / 表格行，stable span_id
  → TechnicalClauseAgent（family 锁死 technical，独立候选）
  → CommercialClauseAgent（family 锁死 commercial，独立候选）
  → ClauseValidator（quote 必须回到指定 Span）
  → ClauseReconciler（同 family 去重；跨 family 仲裁 / conflict）
  → SpanCoverage（候选 Span 覆盖检查）
  → hybrid 时未覆盖 Span 单轮模型补扫，再逐 Span heuristic
  → 成功后事务：upsert Section → supersede 旧 draft → insert 新 draft
  → 人改 / 确认 / 丢弃
  → 已确认集合变化 → 入队 BidMatchJob（debounce）
```

**① 标题、Section 与 Span（大纲，不是门闩）**

```
BidSection
  id
  project_id
  document_id
  section_key           层级路径 + 同名出现序号的稳定键；不含正文 hash
  heading_path          如「第三章 / 技术要求 / 性能指标」
  hint_family           technical | commercial | skip | unknown   // 提示，不是门
  body                  该段 Markdown（含表格）
  extract_status        pending | running | done | failed | skipped
  error_message

Span（引擎内）
  span_id               section_key + span ordinal
  heading_path / body / char_count / candidate
  context               只辅助理解的非引用上下文（如表头）
```

**不要**复用 `crates/chunker` 的 ATX heading 切块。OutlineParser 同时识别 ATX、第 X 章 / 节、`3.2`、`一、`、`（一）` 等编号并维护真层级路径。普通 `1. 投标人须……。` 要求句不能误判成标题。

标题、分类信号、must 词和限制全在 `cn-tender-v2` Policy。正文按句号 / 分号和“并应 / 且须 / 并提供”等并列约束锚点拆成要求级 Span。表格中自带主体与约束的单元格独立成 Span；依赖兄弟单元格的键值参数保留原始 Markdown 整行，并要求 quote 等于整行。表头只作为 `non_quotable_context`，无数据行的空表不会生成可引用 Span。超长正文仍按字符上限完整分片，`read_span(span_id)` 返回独立的 `quotable_text` 与非引用上下文。

**② 两个专用智能体、仲裁与 Span Coverage**

```
BidExtractRun
  id / project_id / document_id
  status                pending | running | done | failed
  triggered_by          auto | manual
  section_total / section_done
  extractor_mode        agent | hybrid | heuristic
  model_id / policy_version / prompt_version
  diagnostics           rounds / retries / tool calls / fallback / coverage / failed spans
  claim_token / heartbeat_at   // 服务端租约；不暴露给模型
  error_message
  started_at / finished_at
```

**禁止并行两套抽取。** 项目行锁 + running partial unique index 串行 claim；每次 claim 生成新 token，并在执行期周期性刷新 heartbeat。report 持久化和 finish 都必须同时持有 run / project token；回收后的旧 worker 不能写入。Section retry 的 HTTP 只持久化 `bid_section_retry_jobs` 意图并返回；worker claim job 后再取得绑定 section_id 的项目租约，在租约内读取最新 Section。job / Section 的 heartbeat、状态写和 draft 替换均要求 token；durable job 终态与项目/Section 租约释放在同一 token-conditioned 事务完成，Redis 短暂不可用由 housekeeping 重投。删除文件会级联删除其 pending 文档级 run，项目存在活动抽取租约时拒绝删除。任一 `BidExtractRun` 为 `running` 时，其它自动抽 / 整项目重抽排队。两个智能体可以顺序调用，但各自面对完整候选 Span，**不共享已覆盖 quote 来抑制另一 family**，也不共享聊天记录。

每个智能体 5 个工具，作用域 = **这一份招标 Markdown**：`list_outline` / `read_span(span_id)` / `grep` / `emit_clauses` / `done`。传输是原生 async OpenAI tool calling；不嵌套 runtime、不自写 JSON 动作协议、不引入通用 Agent 运行时。每边最多 12 轮，单次 emit ≤40，单文件软顶 400。

- 严格工具 schema：模型每项只提交 `span_id / quote / text / must`，四项都 required 且禁止额外字段；family 和 provenance 由服务端补。
- Prompt 明确技术 / 商务边界、must 正反例，并把招标正文视为不可信数据；正文中的“忽略指令 / 访问知识库”不构成系统指令。
- quote 必须逐字包含于指定 Span 的 `quotable_text`；不折叠空白、不统一标点，也不能把表头 context 与数据行拼接。键值表整行 Span 只能提交完整整行 quote，不能用字段名或数值片段覆盖它。无 span、空 text 或回源失败立即拒绝并计 diagnostics。
- 同 family 重叠 quote 去重只发生在同 Span。相同 quote 被两个 family 命中时：先看 heading prior，再按 Policy 信号评分，再按服务端 extractor rank（Agent > Span sweep > heuristic）裁决；最终仍相等才建议 `technical` 并写 `family_conflict=true`，不引入 `unknown` family。人修改 family 后确认会清冲突标记。

Coverage 以候选 **要求级 Span** 为单位，不以 Section 为单位：一个 Span 有合法 clause 不代表兄弟 Span 已覆盖；中性清单/库存表不能仅因含 `|` 或位于技术/商务标题下成为候选。键值表整行必须带有 Policy 正文要求/分类信号；标题 prior 只在候选成立后裁决 family。超过 `max_span_chars` 的整行不进入 Agent 候选；`read_span` 只返回完整合法 JSON，完整序列化结果超限时返回结构化 size error，绝不截断 JSON。“并应 / 且须 / 且不得 / 并提供”等并列约束，以及带强制前缀的 `、…和/及…` 清单会拆成兄弟 Span，后项携带只读的前项语义上下文。

- `agent`：必须有可用 tool-capable 模型；模型不可用、请求失败或返回零 tool call 时 run 明确失败；不走规则兜底。
- `hybrid`（默认）：模型可用时先跑双 Agent；未覆盖候选 Span 再按两个 family 各做一次单轮补扫；仍未覆盖则逐 Span heuristic。模型不可用 / 失败 / 零 tool call 均写 diagnostics 和 UI 提示，不能静默冒充 Agent 成功。
- `heuristic`：完全不请求模型，逐候选 Span 按同一 Policy 抽取；用于离线和应急，UI 明示规则模式。

`BID_EXTRACT_MODEL_ID` 为空时回退主 Chat model；`BID_EXTRACT_MODE` 默认 `hybrid`。`must` 由 Policy 强制 / 可选词校正：强制上下限和否决语义优先为 true，纯建议 / 加分为 false，拿不准为 false。

一个 Span 未覆盖会进入 diagnostics，Section 为 `failed`；没有候选 Span才是 `skipped`。一份文件抽取失败不取消其它文件。引擎先构造内存 report；只有成功 report 才原子替换该文件 draft，因此模型故障不会隐藏上一次草稿。重试 Section 走同一引擎的 Section scope，不走旧 Prompt。

**③ 人确认**

- 列表按 `family` 分组；`draft` / `confirmed` 分开；`superseded` 默认折叠。
- 人可改 text、family、must，可 `rejected`；partial PATCH 只更新请求中实际出现的字段，服务端在 project-first 锁内计算确认集是否变化，禁止旧快照覆盖省略字段。
- 允许人**手工加一条** confirmed。`extract_run_id` / `section_id` / `source_document_id` 皆可空。
- 一段抽取 `failed`：人可重试该段，或手补。
- 未确认不进匹配。
- 确认成功后入队：**该勾选段**技术匹配（仅本段确认集变时），以及商务确认集变时的项目级商务路。HTTP 立即返回。

### 4.3 后传文件与重新解析

- 后传：**自动**解析并抽取，draft 追加。已确认 / rejected **一行不动**。
- 「重新解析」：新的整项目 run。每份文件独立形成内存 report，成功后分别事务提交；单文件失败不回滚其它文件。
- Section 用 `(document_id, section_key)` 稳定 upsert；正文变化不重复新增侧栏。已确认 / rejected 继续指向同一稳定 Section；消失但仍被人工结果引用的 Section 保留。
- 上一 run 未确认的 draft 只在新 report 提交时 → `superseded`（见 4.1）。
- 界面标明：有文件还在解析/抽取/匹配，以及规则兜底、未覆盖 Span、Agent 失败；多文件 run 只要存在失败文件，即使其它文件成功，也显示 partial failure 和失败文件数。
- 技术覆盖**只按 §5.3 现算**。
- 商务不走 `need_rematch`。确认后入队 `scope=company`；尚无本轮检索结果 → 「待检索」，不进 ⑤。

### 4.4 抽取输出契约

模型不能输出 `family`、`raw_text` 或 provenance。它只能通过严格 `emit_clauses` 工具提交：

```json
{
  "clauses": [
    {
      "span_id": "sec-a91d...-1:span-0002",
      "quote": "投标人须具备有效的 ISO9001 质量管理体系认证",
      "text": "投标人须具备有效的 ISO9001 质量管理体系认证",
      "must": true
    }
  ]
}
```

| 字段 | 规则 |
|---|---|
| `span_id` | 必须是当前文件 `list_outline` 暴露的 Span |
| `quote` | 必须逐字包含于该 Span 的 `quotable_text`；任何空白 / 标点变化或 context 拼接都拒绝；键值表整行必须完整引用 |
| `text` | 工具层仍 required；Prompt version `clause-extractor-v3` 要求其逐字等于 `quote`，服务端仍规范为已验证 `quote` |
| `must` | bool；服务端再按版本化 Policy 的强制 / 可选语义校正 |

服务端锁定 Agent family，写 `raw_text=quote`，并把 `span_id / heading_path / quote` 放入 `source_span`，把 extractor、两边 proposed family、Policy / Prompt version 放入 `extraction_meta`。所有自动输出一律 `status=draft`；只有人确认后才可进入匹配。

---

## 5. 匹配与勾选

### 5.1 两路，不要混成一次「既打产品又打证」

今天 `/match` 的 `use_library` 是「产品分之外再把 library hit 记到该产品上」。投标里商务条款**不是给产品加分的**，是公司有没有这份材料。

某勾选段的已确认技术集合变化之后，条款/Section merge 变更事务以 project→clause/section 的固定锁序原子递增项目 `match_generation` 并写 `match_dirty`，再入队 **该段** 技术匹配（debounce：该段确认集哈希）。调度器把读取确认集时的 expected generation 带到插入事务；generation 已变化就拒绝旧快照，每个 `(project,generation,job_kind,unit)` 只有一个权威 job，`job_kind=technical|commercial` 明确区分未归段技术 nil UUID 与商务 NULL。商务确认集变化仍入队项目级商务路。worker claim token 并刷新 heartbeat；候选和商务命中只允许当前 generation/token 发布，housekeeping 会恢复 dirty、pending 或 stale-running 意图。作业不在 HTTP 里打全库。技术 `/match` 的 `requirements` **只含该段**已确认技术条款，禁止把全书技术条款打成一个 `candidates[]`。

```
BidMatchJob
  id
  project_id
  status                pending | running | done | failed
  tech_status           pending | skipped | running | done | failed
  commercial_status     pending | skipped | running | done | failed
  debounce_key          当时确认集的哈希
  error_message
  started_at / finished_at
```

- 该路确认集为空 → **不调** `/match`（0 条 requirement 会 400），该路标 `skipped`。
- 一路失败不影响另一路：技术 400 不回滚已写的 `BidCommercialHit`，商务失败不丢掉技术候选。
- 技术结果进**该段**候选，等人勾选才写 BidSectionPick；商务结果直接刷新 `BidCommercialHit`。
- 「重新匹配 / 重新检索商务」也入同一类作业，不在 HTTP 里打全库。

| 路 | 条款 | 打哪 | 用来干什么 |
|---|---|---|---|
| 技术 | `family=technical` | `/match scope=product_lines`，`use_library=false` | 产品排序、覆盖率、`unmet_must`、证据、界面图 |
| 商务 | `family=commercial` | `/match scope=company`：**按条款找文档**，不把分类夹当产品排名 | 预览 ④⑤。**不**进入产品线产品的 `score` |

不要用现有 `tender_text` 让知识库再拆一遍。不走 assembly `/search`。

`/match` 增加 `scope`（一个入口，两种响应）：

| `scope` | 评谁 | 响应 | 谁调 |
|---|---|---|---|
| `product_lines` | 全部 `kind=product_line` 下的 `kind=product`，**排除** company | 今日的 `candidates[]`（产品排序） | 登录用户或已认证 API key |
| `company` | 仅那一条 company Workspace 里全部 `kind=library` 的 current | **按条款展平**，见下 | 同上 |
| 不带 `scope`、仍带 `workspace_id` | 今日单线 `kind=product` | 今日 `candidates[]` | 旧调用；投标不用 |

带 `scope` 时不要传、也不推断 `workspace_id`。`scope` 只走 PG，不走单仓内存 matching。

投标技术路：

```json
{
  "mode": "matching",
  "scope": "product_lines",
  "requirements": [{ "id": "条款id", "text": "...", "must": true, "weight": 1.0 }],
  "version_scope": "current",
  "include_library": false,
  "expand_wiki": false,
  "expand_graph": false,
  "match_count": 10
}
```

投标商务路：同一入口，`scope=company`，`expand_wiki=false`，`expand_graph=false`。漏传 expand 时，带 `scope` 的请求默认两者为 **false**（与旧接口默认 true 不同）。

商务响应（不是产品排行榜）：

```json
{
  "clauses": [
    {
      "id": "条款id",
      "outcome": "hit",
      "document_id": "...",
      "version_id": "...",
      "file_name": "ISO9001.pdf",
      "score": 0.81,
      "product_id": "...",
      "hits": [ /* 7.3 Hit，仅该公司资料文档；禁止 wiki/graph 节点 */ ],
      "alts": [
        { "document_id": "...", "file_name": "...", "score": 0.70 }
      ]
    }
  ],
  "warnings": []
}
```

同一条款多份材料：`outcome=hit` 取**分数最高**的一条写入 `BidCommercialHit`；其余进可选 `alts[]`（预览 ④ 先展示最佳文件名）。全部低于召回阈 → `outcome=miss`，无 `document_id`。

知识库现行 `/match` 每请求 **1–30** 条。技术超过 30：作业按 30 条一批顺序调，按 `product_id` 合并：

- 某产品的 `score` / `coverage` = 各批加权后再按**全部技术条款**重算（Σ 条款分 / Σ weight；weight 第一期全 1）。
- `unmet_must` = 各批并集。
- 证据按条款 id 保存，禁止跨批、跨版本拼一条 hit。

商务超过 30 条：同样 30 条一批，按 `clause_id` 写下 `BidCommercialHit`（并集，不把各批分数加在一起）。

**产品超过 50：`scope=product_lines` 一次评完全部产品线，禁止静默截断。** 不要按产品线分批合并来绕过 embedding 不一致。跨线 `embedding_model_id` 不同，或各产品线 `retrieval_config`（vector/keyword 阈值）不一致 → **400**，运维先统一再 `list_reparse`。评完可以慢，所以必须走 `BidMatchJob`，禁止确认按钮同步等待。

```
BidCommercialHit
  project_id
  clause_id             UNIQUE (project_id, clause_id)
  outcome               hit | miss
  document_id           仅 hit
  version_id            仅 hit：检索当时该分类夹 Product 的 current
  file_name             仅 hit
  score                 仅 hit
  product_id            仅 hit：分类夹 id，不当候选排序
  matched_at
```

每次商务检索对**当时全部已确认商务条款**各写一行 `hit` 或 `miss`。先删已不在确认集里的旧行，与写入同一事务；检索失败则**整轮回滚、不改表**，预览当「待检索」，不当缺件。半批成功半批失败视为整轮失败。

**补证后再检索的触发：** 公司资料文档 `index_ready=true`（convert 完成 ∧（无图 ∨ multimodal 完成））之后才自动入队商务匹配。`index_ready=false` 期间不写 `miss`，已有行保持，没有行则继续「待检索」。不要在上传 POST 或仅 `enable_status=enabled` 时就检索。手点「重新检索商务」同样必须等相关文档 `index_ready`，否则当失败→待检索。

`index_ready` 与知识库 `parse_status=completed` **不是**同一时刻：后者还要等 wiki/graph/摘要。商务和招标切条都不借用 `completed`。

### 5.2 勾选 = 按勾选段

勾选段（匹配单元）默认等于一个 `BidSection`。相邻节可 `merge_into` 锚段，共用候选与勾选。多文件同名章默认两段，人可并。`family=commercial` 不进任何技术勾选段。

```
BidSectionPick
  project_id
  unit_id               锚 BidSection id（并入后用锚）
  product_id
  version_id            勾选当时该产品 current（须 active）
  score / coverage      相对本段参数
  picked_at
  clauses[]             勾选当时**本段**已确认技术条款每条一行（含 hit=false）
    clause_id, text, must, hit, hits[]
```

- 系统只按本段排序，不宣布唯一最佳。人给该段勾 1..N 个产品。
- 唯一键 `(project_id, unit_id, product_id)`。同一型号可出现在两段。
- 解决方案 = 各段 Pick 的产品并集。不另建方案表。
- 匹配只刷该段候选，不改已勾、不清 `need_rematch`。要更新快照必须按新候选重勾该段。

### 5.3 覆盖状态（只存偏离，其余现算）

库里存人评 `assessment`：`unset | meet | partial | deviate | fail`（旧 `deviate`/`deviate_note` 迁入）。  
建议 `pending` / `cover` / `unmet` / `need_rematch` / `uncovered` **只对已确认技术条款**，只看 **该条款所属勾选段** 的 SectionPick，不写回条款行。本段没有 Pick 时禁止把 must 标成 unmet。商务不进这张表。

「在快照里」= 本段某个 Pick.`clauses[]` 有这条 id。  
「过期」= 本段含该 id 的 Pick 其快照 `text`/`must` 与当前不同。

| 建议 | 条件 | 人怎么补 |
|---|---|---|
| `pending` | 本段没有 Pick | 在本段勾选 |
| `need_rematch` | 本段有 Pick，但不含该 id 或已过期 | **按新候选重勾本段** |
| `cover` | 本段至少一个 Pick 该条 hit | 可补图；人评默认可 meet |
| `unmet` | 本段 Pick 均未 hit 且 must | 换本段产品 / 补手册后重勾 / 人评 partial·deviate·fail。**禁止**对人评 meet |
| `uncovered` | 均未 hit 且非 must | 同上 |

人评写入成稿默认句与③，不改建议列。

商务现算（只看 `BidCommercialHit`，不看 SectionPick）：

| 算出 | 条件 | 预览 |
|---|---|---|
| 待检索 | 已确认商务，本轮检索还没有该 `clause_id` 的行 | ④⑤ 都不进 |
| 有材料 | `outcome=hit` | ④ |
| 缺件 | `outcome=miss` 且 `must=true` | ⑤ |
| 未覆盖 | `outcome=miss` 且 `must=false` | ④⑤ 都不进，商务列表可标「未覆盖」 |

---

## 6. BidShot（针对招标参数的功能界面图）

每一条**技术**条款应对应该参数的产品功能界面截图，不是装饰图。

**独立上传的 png 认，手册/白皮书里抽出来的图也认。** 两条路都要多模态。

产品线、公司资料里用于投标/商务检索的 current **都必须** `enable_multimodel=true`，且 `vlm_model_id` 指向真正的视觉模型（只认 `KNOWLEDGEBRAIN_VLM_*` / 该版本的 VLM 记录）。**禁止**用 chat URL 冒充 VLM。`ProductVersion::new` 默认打开。存量 current 要回填。

算匹配来的界面图，当且仅当 Hit 的 `chunk_type` ∈ {`image_ocr`, `image_caption`}。来源两种：

1. **独立图片文档**：png/jpg/gif 当成产品文档上传。原文件 = 该文档 `object_key`。建议 Tag `界面`（按 Workspace 隔离，第一期不是硬门闩）。
2. **手册里的图**：多模态子 chunk。Hit.`image_object_key` = `chunks.context_header`。**不要**新加 `image_info` 列。独立 png 若 header 为空，回退该文档 `object_key`。单靠 `document_id` 只能拿到整本手册。

不要把手册正文里的 `![](images/…)` 文本 chunk 当截图。人可删掉匹配错的图。

**人补**：本标条款上人传。不写回产品库。不挡预览。一条条款可以多张。读取走登录即可的全局 `GET /files?key=`（或等价），不挂 ProductVersion。

```
BidShot
  id
  project_id
  clause_id
  product_id
  version_id
  source                matched | uploaded
  object_key            图文件
  kb_document_id        仅 matched
  kb_image_ref          仅 matched：手册抽图时的图引用；独立图片文档可空
```

---

## 7. 预览（第一期 ①～⑤）

| 分册 | 内容 | 第一期 |
|---|---|---|
| ① 项目扉页 | 标题、负责人、结束时间、已选产品 | 要 |
| ② 技术点对点 | 条款 \| 应答 \| 产品 \| 证据 + 功能界面图 | 要 |
| ③ 技术偏离表 | 人标偏离 + 算出的 must unmet | 要 |
| ④ 资格/商务目录 | 公司资料命中的文件名 | 要 |
| ⑤ 商务偏离/缺件 | 已检索且 must miss | 要（弱） |
| ⑥ 投标函 / 授权 / 报价 / 保证金 / 实施计划 | 程序性模板 | **以后** |
| 导出 | 渲 **当前成稿 MD**。缺分册先生成。定稿 `?format=pdf`。图随文档嵌入 | **要** |

```
BidBookletPart
  key            "1" | "2:{unit_id}" | "3" | "4" | "5"
  markdown       过程中可编（GFM 子集）
  generated_at / edited_at / stale
```

成稿不是纯现算投影。结构化数据（勾选、人评、命中）是生成器；人改的 MD 是交出去的正文。数据变只标 `stale`，不自动覆盖。`ended` 后稿只读，仍可下载。

② **按勾选段一篇**，段内产品共享。每条已确认技术参数生成时带 `<!-- clause:{id} -->`。导出校验：已确认技术 must 的锚都在，缺则拦。不是一条参数对应一个产品行。

③ 人评 `partial`/`deviate`/`fail` + 建议 `unmet`。`pending` / `need_rematch` / `uncovered` 不进③。

④⑤ 生成自 `BidCommercialHit`（hit→④，miss 且 must→⑤；待检索不进⑤）。人可在对应 part 润色。

导出缺 part 先按当时数据生成再渲。有 stale 警告。不回写 docx。

### 7.1 缺了就补（总表）

| 缺什么 | 怎么补 | 不做什么 |
|---|---|---|
| 招标文件解析失败 | 重试该文件，或删了重传 | 不因一份失败锁死整个项目 |
| 模型漏抽 | 看 diagnostics 的未覆盖 Span；hybrid 会先单轮补扫再逐 Span heuristic；仍空才手补 | 不另开通用闲聊 Agent 扫全书 |
| 一段抽取失败 | 重试该段，或手补 | 不取消其它段 |
| 产品库没有该参数的界面图 | 本标条款上人补 BidShot；或先把 png 传到对应产品再重新匹配 | 不生成假图；不挡预览 |
| 技术 must 未覆盖 | 换产品 / 补手册后**重勾** / 标偏离 | 未勾选时不标 unmet |
| 技术条款不在快照或 text/must 与快照不一致 | 按新候选**重勾** | 只点「重新匹配」不够；不当 unmet |
| 商务 must 待检索 / miss | 证/案例传到 company，等 `index_ready` 后再自动检索 | 没搜过、证还在 processing 不进 ⑤ |
| 商务命中过期/换证 | 分类夹 `version:clone` 后重新检索商务 | 不改已锁的 SectionPick |

---

## 8. 要改的知识库契约

| 项 | 现在 | 改成 |
|---|---|---|
| Workspace | 只有一种，既是鉴权根又是容器 | `kind`：`product_line` \| `company`。products / documents 列集不变 |
| 公司资料 | 每个 Workspace 一份默认 library | 恰好一条 `kind=company`。分类夹是它下面的 `kind=library`。**不要** `workspace_id` 为空 |
| 产品线默认 library | 建仓必插、商务可能误用 | **冻结写入**，存量迁到 company；`scope=company` 不扫产品线 library |
| `/match` | 必须落到一个 `workspace_id`；只评 `kind=product`；>50 静默截断 | 投标走 `scope`。`product_lines` 评完全部产品线或 400。`company` 按条款展平 library 文档。带 `scope` 时 expand 默认 false |
| 商务条款 | `use_library` 挂到产品分数上 | `scope=company` → `BidCommercialHit` |
| 图片 / 多模态 | 新版本默认关；chat URL 可冒充 VLM | **必须开**真 VLM。Hit.`image_object_key` = image chunk 的 `context_header` |
| 检索门闩 | `enable_status=enabled` 即可搜 | 商务自动重搜 / 招标切条看 `index_ready`，不看单独的 `enable_status`，也不等 wiki/graph 的 `completed` |
| 读原文件 | `GET .../files?key=` 要 `require_ws` | 登录即可。另提供不挂版本的 `GET /files?key=` 给人补图 / 招标原件。`GET document` 带回 `object_key`/`file_name` |
| 鉴权 | 开放 register；每请求查 `workspace_members` | 关 register；LDAP 登录；登录或 API key 即可读写全库与投标。`workspace_members` 留表、不当门闩 |
| `tender_text` | 知识库代拆、无 family | 投标平台自己拆 |
| 输出 | 不给 `best_product_id` | 不变 |

bootstrap：恰好一条 `slug=company`、`kind=company`（部分唯一）。存量 Workspace 回填 `kind=product_line`。`POST /workspaces` 默认 `kind=product_line`，禁止再建第二条 company。company **不**插默认 library。

换证：company 下对应分类夹做 `version:clone`。

其它纪律：禁止跨版本拼证据；勾选锁 `version_id`；跨线 embedding 与检索阈值须一致，否则 400。

**谁能干什么：**

| 动作 | 谁 |
|---|---|
| LDAP 登录；首次 bind 成功则插入 users | 公司目录里的人 |
| 建/改 Workspace、建 Product、传手册/界面图/证/案例、换证 | 任何登录用户或已认证 API key |
| 列 Workspace（回 `kind`）、列产品、读文档和 files | 同上 |
| 投标全流程；`/match` 两种 `scope` | 同上 |
| `POST /auth/register` | **410 / 404，关闭** |
| 未登录 | 全部 401 |

API key 第一期可继续用；不按 key 的 workspace/product scope 挡投标或 `scope` 检索。仍要认证，不是匿名。

Qualification / Performance 结构化表、过期与金额：**不在第一期**。以后仍挂 company 的分类夹上。

LDAP 具体协议（ldap/ldaps、Bind DN、组过滤）只进 `deploy/.env.example`，不进领域实体。

---

## 9. 五步操作（对到实体）

| 人做什么 | 系统做什么 | 落什么 |
|---|---|---|
| 1. 建项目（标题、负责人、结束时间） | 写 BidProject `open` | 项目 |
| 2. 上传一份或多份招标文件 | 每份自动 convert + OCR，`completed` 即抽条款 | BidDocument / BidSection / BidClause(draft) |
| 3. **先商务**：确认资格/商务条款；看 hit/缺件；补证或标偏离；编④⑤ | 入队项目级商务匹配 | BidCommercialHit；Booklet ④⑤ |
| 4. **再技术**：按勾选段确认参数、匹配、勾型号、人评、补图、编该段② | 该段 MatchJob + BidSectionPick | 本段候选 / Pick / ② |
| 5. 编①③；过程下 Word，定稿下 PDF | 渲当前 Booklet；缺 part 先生成 | BidBookletPart |

进标默认商务。商务不做门闩，随时可进技术段。人必须点的：商务确认、各技术段确认与勾选。可选：并段、人评、编成稿、补图、补证、重生成过期分册、结束项目。

---

## 10. 明确不做

已从 BidProject 拿掉：`workspace_id`、`buyer_name`、`package_no`、`review_kind`、`allow_deviation`、`delivery_region`、`product_scope`、`version_scope`、长状态机、`locked_at`。

第一期不做：包件、补遗作废链、`procedural`、方案实体、Org、结构化资质/业绩判断、⑥ 分册、把招标书或人补截图送进产品索引、用通用闲聊 Agent / 把招标书打进知识库再 RAG 来拆条、用 `tender_text` 让知识库代拆、模型生成假界面图、单独的投标业务容器。允许 §4.2 的有界专用抽取（tool calling，不进产品库）。

报价、成本、折扣：若以后做，只活在投标平台，不进知识库检索。

---

## 11. 落地顺序

1. 知识库：Workspace `kind` + 一条 company；存量回填 `product_line`；冻结并迁移默认 library；`/match` 加 `scope` 且登录即可；`index_ready`；真 VLM；Hit 出图；`GET document`/`files` 登录可读。
2. LDAP 登录、关闭 register；API key 可打标。
3. 抽出 `convert_to_markdown`。BidProject + 多文件上传 + 招标四态只表示 convert+multimodal。
4. BidExtractRun（async tool calling；`BID_EXTRACT_MODE` + `BID_EXTRACT_MODEL_ID`）+ diagnostics + 人确认条款。
5. 勾选段 MatchJob → BidSectionPick → 人评 / BidShot → BidBooklet 可编 → 导出渲当前稿。
6. 以后：⑥ 分册（投标函/授权/报价等程序性模板）；资质/业绩结构化。

Tag `界面` 建议打，第一期不是硬门闩。
