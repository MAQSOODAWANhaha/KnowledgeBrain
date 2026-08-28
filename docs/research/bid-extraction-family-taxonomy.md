# 招标抽取：技术 / 商务分类是否过宽

| 项 | 值 |
| --- | --- |
| 状态 | **调研结论（对照仓库 primary sources；不是改规格）** |
| 问题 | 技术和商务是否过宽；条件 / 资质 / 报价 / 时间去哪了 |
| 依据 | 仓库源码与领域草案（primary）；法规篇章仅作对照 |

> 本文是调研当时的分类学笔记，不是当前产品契约。当时对照的是 V1 ①～⑥ / 两路匹配成稿。当前 Target V2 以 [`../bidding/authoring.md`](../bidding/authoring.md) 为准：动态大纲、Word 式编制画布、知识填充、Assessment 只提示。下文写「当前目标已扩展为①～⑥」只记录调研时状态。

## 结论

**对「这一产品」而言，技术 / 商务两分不是分类学过粗，而是匹配与成稿的路由契约。** 条款落库只能是 `technical | commercial`；匹配只有两路：技术按勾选段打产品线，商务按条款打公司资料库。领域草案把商务写成资格 / 证照 / 业绩 / 合同材料，并明确第一期不做金额判断、不参与产品打分、⑥（函 / 授权 / **报价** / 保证金 / 实施计划）以后再做。

条件、资质、报价、时间**不是**四个未入库的 family。它们在现实现里分别落到：

| 日常说法 | 实际落点 |
| --- | --- |
| 资质 / 资格 / 证照 / 业绩 | **商务**（有 heading + signal；打 company 资料） |
| 技术规格 / 性能 / 接口 / **系统**响应时间 | **技术**（打产品线） |
| 「条件」 | 大纲标题后缀，不是 family |
| 报价、保证金、递交、密封、投标函 | 标题 **skip** 或 prompt「纯流程不抽」；**报价本身不在 skip 列表里**，可能漏进候选 |
| 招标截止时间 | 项目字段 `expires_at`，不从条款抽 |
| 交货期 / 工期 / 付款 | Policy **没有**专属信号；带「须 / 应 / 不超过」时可能当候选，无 family 信号则启发式双写，仲裁默认技术并打 `family_conflict` |

相对《政府采购货物和服务招标投标管理办法》第二十条那种完整招标文件目录，两分当然窄。那是法律文件结构，不是本产品的匹配合同。**没有** primary source 表明必须拆第三个 `bid_clauses.family` / `job_kind` 才能跑通 拆条款 → 人确认 → 技术按段匹配产品 / 商务按条款找公司资料 → 成稿 ①～⑤。真会伤匹配的是：报价 / 交货 / 付款被抽出来并确认后，被送进错误的 `/match` scope。那首先是 **skip / 候选启发式 / 无信号句** 的覆盖问题，不是缺一个法律枚举。

---

## 1. 当前分类实际做什么

### 1.1 条款 family：只有两个，且是路由键

`ClauseFamily` 只有 `Technical` / `Commercial`，`ALL` 长度 2。

```37:59:crates/bid/src/extraction/types.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClauseFamily {
    Technical,
    Commercial,
}

impl ClauseFamily {
    pub const ALL: [Self; 2] = [Self::Technical, Self::Commercial];
    // ...
    pub fn other(self) -> Self {
        match self {
            Self::Technical => Self::Commercial,
            Self::Commercial => Self::Technical,
        }
    }
}
```

数据库同样锁死：`bid_clauses.family` 只能是这两值；`bid_sections.hint_family` 才有 `skip | unknown`。条款行**不能**写成 skip / unknown / procedural。

```61:63:migrations/0007_bid.sql
    hint_family text NOT NULL DEFAULT 'unknown'
        CHECK (hint_family IN ('technical', 'commercial', 'skip', 'unknown')),
```

```141:152:migrations/0007_bid.sql
CREATE TABLE IF NOT EXISTS bid_clauses (
    ...
    family text NOT NULL CHECK (family IN ('technical', 'commercial')),
```

匹配工件复述同一合同：`bid_match_jobs.job_kind IN ('technical', 'commercial')`（`migrations/0007_bid.sql`）；`bid_match_requirement_artifacts.family` 与 `route_kind` 仍只有这两类，且 `technical` 必须带 `unit_id`、`commercial` 必须 `unit_id IS NULL`（`migrations/0012_bid_match_contract.sql`）。

HTTP / 手补同一约束：`family must be technical or commercial`（`crates/api/src/routes.rs` patch / add）。UI 分组只有商务表 vs 技术段表；检查器两个按钮是「技术条款 / 商务条款」，冲突时也只能改这两类（`web/src/bid/Inspector.tsx`、`web/src/bid/helpers.ts` `liveClauses`、`web/src/bid/ClauseTable.tsx`）。

当时领域草案写明第一期**没有** `procedural`，没有 scored/weight，没有作废链；其后 V1 曾把目标扩到 ①～⑥。那不是现在的产品目标。

### 1.2 Policy 两个 family 的语义（不是「一切商务条款」）

`crates/bid/config/cn-tender-v2.json`：

| family | 名称 | 定义（摘） | heading_hints | 正文 signals |
| --- | --- | --- | --- | --- |
| `technical` | 技术条款 | 产品 / 设备 / 软件 / 服务**交付本身**的功能、性能、容量、接口、兼容、实施和技术服务 | 技术规格、技术要求、采购需求、功能要求、性能指标、服务要求、技术参数 | 吞吐量、转发性能、接口、端口、容量、并发、兼容、支持、IP等级、防护等级、**响应时间** |
| `commercial` | 商务/**资格**条款 | 投标**主体**资格、法定证照、财务信用、资质认证、业绩案例、合同和商务响应**材料** | 资格审查、资质要求、商务要求、商务条款、业绩、注册资本、投标人资格、资格条件、类似项目、服务能力 | 注册资本、投标人资格、营业执照、类似项目、业绩、资质、认证、ISO、财务、信用、合同、许可证 |

正例 / 反例把边界钉在「执照 / 业绩」vs「设备接口 / 系统响应时间」，**没有**报价表、交货期、付款节点、评标办法的例子。

标题 hint 只看路径**最深一段**；skip 优先于 family：

```192:218:crates/bid/src/extraction/policy.rs
pub fn hint_family(policy: &ExtractionPolicy, path: &str) -> &'static str {
    let deepest = path.rsplit('/').next().unwrap_or(path);
    let folded = fold_text(deepest);
    if policy.skip_heading_hints.iter().any(|term| folded.contains(&fold_text(term))) {
        return "skip";
    }
    // technical / commercial heading_hints；两边都中或都不中 → unknown
}
```

`skip_heading_hints`：**投标函、授权委托、保证金、递交、密封、包装、目录、前附表、投标人须知**。没有「报价」「付款」「交货」「工期」「评标 / 评分办法」。

测例：`hint_family(..., "投標人資格條件") == "commercial"`（`policy.rs` tests）。

### 1.3 Prompt：family 锁死 + 不确定不交 + 不抽纯流程

模板 `crates/bid/prompts/clause-extractor-v2.md`（Policy 里 `prompt_version` 记为 `clause-extractor-v3`，文件名仍是 v2）：每个 Agent 只抽当前 family；「不确定是否属于当前 family 时不要提交」；「只抽取要求投标人应答、证明、承诺或满足的条款」；「不抽取目录、章节标题、背景描述和没有实质要求的纯流程说明」。模型**不能**输出 family（服务端锁定）。

编排对 `ClauseFamily::ALL` 各跑一遍独立 Agent，再 span sweep，再 heuristic（`crates/bid/src/extraction/mod.rs`）。两边不共享「已覆盖」来互斥。

### 1.4 skip / unknown / 非需求：三层不同东西

| 层 | 取值 | 作用 |
| --- | --- | --- |
| Section `hint_family` | technical / commercial / **skip** / **unknown** | 标题 prior，**不是**抽不抽的门闩（领域草案 §4.2） |
| Span `candidate` | bool | 覆盖检查与 heuristic / sweep 的工作集 |
| Clause `family` | technical / commercial | 确认后进哪条匹配路 |

无候选 span 的 Section → `extract_status = skipped`；有候选但未覆盖 → `failed`；不是把条款标成 skip（`extraction/mod.rs`）。

`is_candidate_span`：

- 过短 / 超 `max_span_chars` → 非候选。
- **`hint == skip` 时不看 `须/应` 等 trigger**，只在正文 **family_score > 0 或 veto**（否则废标 / 实质性要求 / 无效投标 / 否决投标）时重开。
- 非 skip：`coverage.trigger_terms` 或任一侧 family signal。
- 纯表头 chrome（指标 / 要求 / 项目 / 内容 / 参数 / 序号 / 名称 / 备注）→ 非候选。

因此「投标文件须在截止时间前递交」挂在「投标人须知」下：有「须」，但无资格 / 技术信号、无 veto → **不是候选**。Golden-02 把它放进 `absent_quotes`；Golden-01 期望集也不含这条。同章「投标人须具有有效安全生产许可证，否则废标」因 **许可证** signal + **否则废标** veto 成为候选，期望 `commercial`（`testdata/bid-extraction/cn-tender-golden-02.*`）。

Agent 的 `list_outline` / 首轮 Outline **仍列出 skip 段**（含 `candidate=false`）。`emit_clauses` **不要求** `candidate=true`，模型仍可能从非候选 span 交条；heuristic / 覆盖补扫只扫未覆盖**候选** span。

`unknown`：最深标题两边 hint 都不中或都中。跨 family 都命中同一 quote 时，仲裁顺序是 **heading prior → 正文 signal → extractor 等级（agent > span_sweep > heuristic）→ 仍平则建议 technical 且 `family_conflict=true`**，**不**引入 unknown family（`extraction/reconcile.rs` `choose_family`；领域草案 §4.2）。人改 family 后确认清冲突（草案）。

### 1.5 大纲：什么成为 span / 候选

`outline.rs` 用 ATX / 第X章节 / `3.2` / `一、` / `（一）` 建 `BidSection`。编号行若像要求句（硬词 / veto，或长度 ≥ 8 且命中 `numbered_requirement_predicates`）则**不当标题**。`numbered_heading_suffixes` 含 **条件、资格、资质、业绩、要求、指标…**：「1. 投标人资格要求」「三、类似项目业绩要求」保持为大纲节点（测例 `signal_bearing_numbered_headings_remain_outline_nodes`）。

正文再按句号 / 分号 / 「并应、且须、并提供…」锚点以及「须提供 A、B 和 C」类列举拆成要求级 span。表格：自带主体+约束的单元格独立；键值行保留整行 Markdown，表头只进 `non_quotable_context`。

### 1.6 匹配与成稿：family 决定打哪、进哪一册

| 路 | 条款 | 作业 | `/match` | 成稿 |
| --- | --- | --- | --- | --- |
| 技术 | 已确认 `family=technical` | `job_kind=technical` + 勾选段 `unit_id` | `scope=product_lines`，`include_library=false` | ② 点对点、③ 偏离；must 锚只计技术 |
| 商务 | 已确认 `family=commercial` | `job_kind=commercial`，`unit_id` 必须空 | `scope=company`，按条款展平 library 文档 | ④ 命中文件名、⑤ must miss |

实现：`crates/bid/src/lib.rs` `run_claimed_match_job`、`coverage_for`（**只**对已确认技术条款现算 cover/unmet）；`crates/bid/src/booklet.rs` part `4` / `5`；`crates/search/src/lib.rs` `matching_pg` 遇 `scope=company` 走 `matching_company_pg`。商务**不**进产品 `score`，**不**进技术勾选段（当时领域草案 §5.1–5.3）。公司资料夹是资质证照 / 体系认证 / 业绩案例 / 服务能力（当前边界见 `docs/knowledge-base/README.md`；另见 `DESIGN.md`、`web/src/assets/Assets.tsx`）。

当时产品目的（旧 `PRODUCT.md`）：拆条款 → 人确认 → 先商务再技术段 → 成稿 ①～⑤。⑥ 函/报价模板当时不做。现目标见编制契约，不再按分册。

---

## 2. 条件 / 资质 / 报价 / 时间（及相关）落点

对照《政府采购货物和服务招标投标管理办法》（财政部令第 87 号）第二十条，招标文件**应当**包括投标邀请、投标人须知、资格资信证明、投标报价与保证金、技术规格、合同文本、**货物服务提供的时间地点方式**、**支付方式时间条件**、评标方法与无效情形、投标有效期、投标截止与开标时间等（[中国政府网](https://www.gov.cn/zhengce/2017-07/11/content_5727444.htm)）。《招标投标法》第十九条同样把技术要求、资格审查标准、**投标报价要求**、评标标准并列为招标文件内容。本产品**没有**按这条目录建 family。

### 2.1 资质 / 资格 — 商务，且匹配合同成立

Policy 商务定义、heading（资格审查 / 资质要求 / 投标人资格 / 资格条件）、signals（营业执照 / 资质 / ISO / 许可证 / 业绩…）与 Golden-01/02/03 期望一致：执照、类似项目业绩、注册资本、ISO9001、安全生产许可证 → `commercial` + `must`。

确认后走 company 文档命中。领域草案：**商务第一期不做证过期 / 金额判断、不参与产品打分**；结构化资质表「不在第一期」。把资质放进商务**不会**把产品排序弄错；缺件进 ⑤，补证后等 `index_ready` 再检索。

「资格条件」同时是 heading_hint 和大纲 suffix：编号「资格条件」当标题，其下执照 / 业绩句按 span 抽商务。

### 2.2 「条件」— 不是 family

Policy 没有 `condition` 类型。「条件」出现在：

1. `numbered_heading_suffixes`：编号标题以「条件」结尾则保持为节，不当要求句。
2. 商务 hint「资格条件」。
3. 正文若只有「投标人应满足以下条件」、无资格 / 技术信号：`应` 使 span 成为候选；heuristic 两侧分都为 0 时**两个 family 各 emit 一条**，仲裁无 heading prior 则 `technical` + `family_conflict`（`coverage.rs` 分数相等分支；`reconcile.rs` 平局）。

所以「条件去哪了」：多数是**资格小节的标题**，不是漏掉的第三类。

### 2.3 报价 — 故意不进 ①～⑤；skip 列表却漏了「报价」

领域 / 产品：

- 成稿 ⑥ = 投标函 / 授权 / **报价** / 保证金 / 实施计划，在当时草案中延期；后来写进 V1 `domain.md` 现码对照，不是现在的产品目标。
- 「报价、成本、折扣：若以后做，只活在投标平台，不进知识库检索。」
- skip 标题含**保证金、投标函、授权委托**，**不含报价**。

因此：

- 「投标函 / 保证金 / 递交」下的纯手续句：skip，无信号则不进候选（Golden 把「截止前递交」标为应缺席）。
- 标题「投标报价 / 报价要求」→ `unknown`。正文「投标报价应包含… / 不得超过预算」命中 trigger `应` / `不得超过` → **是候选**。无商务 / 技术 signal → heuristic 双写 → 默认技术 + 冲突。
- 若人确认成商务：company 库（证 / 案例 / 服务）几乎打不中报价表 → 假 ⑤ 缺件。
- 若确认成技术：报价句进入产品排序，污染 `score` / `unmet_must`。

这是**漏抽 / 误路由风险**，不是「报价类型尚未建表」。第一期匹配合同本来就不检索报价。

### 2.4 时间 — 三种完全不同的「时间」

| 种类 | 落点 |
| --- | --- |
| 项目跟踪用招标结束日 | `BidProject.expires_at`，建项人手填；① 扉页「截止日」；**不从招标正文抽**（领域草案 §0、§2；`booklet.rs` part `1`） |
| 投标截止 / 递交 / 开标 | 通常在须知 / 递交 / 前附表 → **skip**；Golden 要求不要抽「截止时间前递交」 |
| **系统**响应时间 | 技术 signal「响应时间」；Golden-02「系统响应时间不得超过2秒」→ technical must。键值表 `\| 最大响应时间 \| 2秒 \|` 整行候选，无「不得 / 不超过」则 `must=false`（`reconcile.rs` / `coverage.rs` 测例） |
| 交货期 / 工期 / 服务期 | **无** heading、无 signal。有「不超过 / 须」则成候选，分类同报价：易冲突或默认技术 |

87 号把「货物、服务提供的时间」和「投标截止时间」分成两项。本抽取把前者未建模、后者当手续 skip，把「响应时间」当性能指标。看起来像「时间丢了」，实际是**三种时间从未共用一个类型**。

### 2.5 交货期、付款、废标、保证金、评分办法

| 主题 | 分类 | 证据 |
| --- | --- | --- |
| 交货 / 工期 | 无类型；候选则易 `family_conflict` / 默认技术 | Policy signals 无「交货」「工期」 |
| 付款 / 支付 | 无类型。商务 signal 有「财务」「合同」，不等于付款节点。确认成商务会拿证照库去打付款条款 | 87 号第二十条（十）单独列支付；本 Policy 无对应 hint |
| 废标 / 否决 / 无效 | 不是 family；`must.hard` / `veto`。skip 段因 veto **重开** | Golden-02 须知里「否则废标」的许可证条款被抽为商务 must |
| 保证金 | **skip 标题**。无信号则不抽；有「许可证 / 业绩」等才会当候选 | `skip_heading_hints`；⑥ 以后 |
| 评分 / 加分 | `must.optional` 含「加分」「评分项」→ 抽出则 `must=false`。无 skip hint。BidProject **没有**评分办法字段（草案 §2） | 抽出的评分说明若被确认，同样误进技术或商务检索 |

### 2.6 「商务 = 公司库命中、技术 = 产品排序」在资质 / 报价 / 时间搅在一起时还成立吗？

**对资质 / 证照 / 业绩：成立。** 商务定义、company 四夹、④⑤ 都按「有没有这份材料」设计。

**对报价 / 交货 / 付款：一旦被抽成条款并确认，映射就不成立。** 公司库没有报价单可命中；产品手册也不该为「交货 30 日」排序。领域已经把这类放进未做的 ⑥ 和「报价不进知识库」。映射破裂的前提是抽取 + 人确认把它们放进了两 family 之一——UI 又没有第三类可改。

---

## 3. 对「这一产品」技术 / 商务是否过宽

当时产品链路：`拆条款 → 人按段确认 → 技术按勾选段匹配产品 / 商务按条款找公司资料 → 成稿 ①～⑤`；其后 V1 目标曾扩到 ①～⑥。**现在的产品目标**已改为动态大纲 + Word 式编制画布，见 [`../bidding/authoring.md`](../bidding/authoring.md)，不再以固定分册为目标。

要分清两件事：

1. **匹配 / 路由分类**（family → job_kind → `/match` scope → booklet 2/3 vs 4/5）
2. **完整招标法律篇章**（87 号第二条十几项；施工标准文件里的须知 / 评标办法 / 合同 / 工程量清单 / 投标文件格式）

本产品只要（1）。（2）里大部分第一期明确不做：包件、procedural、⑥、结构化金额 / 证过期、评分办法字段。

| 缺失「类型」 | 会不会弄坏匹配 / 成稿 / 人确认 | 只是 UI 标签？ |
| --- | --- | --- |
| 资质 / 业绩 / 证照（已在商务内） | 不会；这就是商务路 | 显示「资格 / 体系 / 业绩」不改 enum 也能做 |
| 技术指标（已在技术内） | 不会 | 显示「性能 / 接口」同理 |
| 报价 / 限价 | **若抽出并确认** → 错 scope；**若 skip / 人驳回** → 不进 ①～⑤，符合第一期 | 不需要新 family 才能「不匹配」 |
| 投标截止 | 项目 `expires_at` + skip；抽进条款无对应检索 | 标签无助于截止日跟踪 |
| 交货 / 付款 | 同报价：确认后错路；不抽则本期无成稿位置（⑥ 以后） | 可显示，但改 enum 仍无独立 job_kind / part |
| 评标办法 | 抽出则噪音；不抽则符合「项目不写评分办法」 | 标签即可 |
| 「条件」 | 不抽不影响路由 | 不是独立需求类型 |
| 废标句 | 只抬 `must` / 重开 skip | 不要新 family |

**结论：匹配契约上两分不够宽到必须拆 schema。** 法律篇章上当然宽。人确认时只有两个按钮，误抽的报价 / 交货看起来像「商务太宽」或「技术太宽」，实质是**把不该进匹配的句子塞进了路由键**。

跨 family 冲突可见：`family_conflict` 在表和检查器提示「分类冲突，请核对」（`ClauseTable.tsx`、`Inspector.tsx`）。这是人闸，不是新类型。

标题 prior 可能压过正文：测例 `# 技术要求` 下「注册资本不得低于人民币500万元」，**若两 Agent 都交**，heading 判 technical（`reconcile.rs` `heading_prior_precedes_contradictory_body_signal`）。Golden-02 同句期望却是 **commercial**（启发式看正文「注册资本」只交商务）。这是仲裁 / 双 Agent 问题，不是「商务定义太宽」。

---

## 4. 看起来像分类问题、其实是抽取缺口

1. **Skip 不看「须 / 应」。** 须知里的递交截止被正确丢掉；须知里夹着的资格废标句靠 veto / 许可证信号捞回。没有信号的交货 / 付款若写在须知里会静默丢失；写在「商务条款」章则变成候选。
2. **报价不在 skip_heading_hints。** 与「⑥ 不做报价」的产品决定不一致，易从 `unknown` 标题漏进候选。
3. **Trigger 过宽、signal 过窄。** `须 / 应 / 不得 / 不超过` 就能当候选；报价 / 交货 / 付款没有 family signal → heuristic 双 emit。Prompt 要求不确定不交，但 hybrid 默认最后会 heuristic 补洞（`extraction/mod.rs`）。
4. **中性表 / 键值表。** 无约束词的库存表不是候选（`neutral_inventory_table_is_not_a_requirement_under_family_headings`）。`| 投标有效期 | 不得少于90日 |` 因「不得」成为候选（`testdata/bid-extraction/office-table.md` 在商务标题下还有无 trigger 的「双路热插拔」，按现行规则**不是**候选——技术句写在商务表且无「应 / 支持」会漏）。键值行必须整行 quote，只引用「最大响应时间」会被拒（`reconcile.rs`）。
5. **编号「条件」当标题。** 「1. 资格条件」不拆成条款；真正要求在节体内。若节体是「应满足以下条件：」一句大口袋，atomic 规则（一条一个要求）依赖后续列举拆分；没有 `、/和/及` 或句号时可能整段一条。
6. **Agent 可对非候选 skip span emit**；覆盖率不管这些多余条。Golden `absent_quotes` 只约束评测夹具，线上靠人驳回。
7. **「响应时间」是性能 signal。** 交货期不会因为含「时间」变成技术；用户问「时间去哪了」时，系统响应时间其实在技术里，截止时间在 skip / `expires_at`。

这些都是 coverage / skip / heuristic 行为，改 family 枚举解决不了。

---

## 5. 建议（保守，不改匹配合同）

**保持两 family 匹配合同。** `bid_clauses.family`、`job_kind`、`/match` 的 `product_lines | company`、成稿 ②③ vs ④⑤ 已经对齐产品目的。Primary sources **没有**显示「必须拆出报价 family / 时间 family 才能路由」；相反，草案把报价放进未做的 ⑥，并把商务限制为材料检索。

可选、且不必迁 schema 的方向（此处只记录，不实施）：

- **显示用 subtype / 标签**（资格 / 业绩 / 体系；或「疑似手续 / 报价」）挂在 `extraction_meta` 或 UI，**不**进 `family` CHECK，**不**改 `job_kind`。
- **把抽取缺口当抽取缺口**：skip 与 ⑥ 对齐（报价是否按须知一样不当候选）、trigger 与「纯流程」对齐、无信号句不要 heuristic 双写进技术。这调整 Policy / heuristic，不是第三枚举。
- **人确认**已能把误类条款改到另一 family 或驳回；冲突标记已经在催核对。

**不要**为条件 / 报价 / 时间增加 `bid_clauses.family` 值，除非以后真做：独立检索语料（报价表、合同付款、工期承诺）或独立分册。那一天需要的是新的 scope / part，而不是先把现在的两路改宽。

---

## Sources

仓库（primary）：

- `crates/bid/config/cn-tender-v2.json`
- `crates/bid/prompts/clause-extractor-v2.md`
- `crates/bid/src/extraction/{types.rs,policy.rs,outline.rs,coverage.rs,agent.rs,reconcile.rs,mod.rs,evaluation.rs}`
- `crates/bid/src/lib.rs`（抽取落库、`run_claimed_match_job`、`coverage_for`、`list_match_units`）
- `crates/bid/src/booklet.rs`（①～⑤，尤其 4 / 5）
- `crates/api/src/routes.rs`（family 校验、手补）
- `crates/storage/src/bid.rs` `validate_match_job_route`；`crates/storage/src/bid_matching.rs` 冻结 technical/commercial
- `crates/search/src/lib.rs` `scope=company` / `product_lines`
- `migrations/0007_bid.sql`；`migrations/0012_bid_match_contract.sql`
- 当时投标领域草案；`docs/research/repository-implementation-snapshot.md`（迁移前投标抽取段）；`PRODUCT.md`；`DESIGN.md`
- `web/src/bid/{ClauseTable.tsx,Inspector.tsx,ClauseDetail.tsx,Sidebar.tsx,helpers.ts,Workbench.tsx}`；`web/src/api.ts`
- `testdata/bid-extraction/cn-tender-golden-0{1,2,3}.*`；`office-table.md`；`README.md`

法规（仅对照篇章，不是本仓规格）：

- 财政部令第 87 号《政府采购货物和服务招标投标管理办法》第二十条，[中国政府网](https://www.gov.cn/zhengce/2017-07/11/content_5727444.htm)
- 《中华人民共和国招标投标法》第十九条（技术要求、资格审查标准、投标报价要求、评标标准）
