# 投标大纲语义骨架重建方案

> 状态：完整方案，待 Plannotator 审阅。已确认一级使用中文编号、二级起使用阿拉伯数字、四类总体骨架、证据驱动的小章节优化，以及重新生成成功后旧候选自动废弃。

## Context

当前 `OutlineGenerateV2` 已能完成真实文件解析、Map/Reduce、持久化、发布和页面展示，但生成结果只满足“单根、来源闭包、需求闭包”等结构契约，不满足投标目录的业务语义。

真实候选 `091f5ff0-0563-493a-b96c-e7816f0a8215` 的根下直接混合了：

- `第六章 投标文件格式`（模板来源目录）；
- `3.1.1 商务文件`、`3.1.2 技术文件`、`3.1.3 报价文件`（投标人须知条款编号）；
- `投标人须知前附表响应`、`第三章 评标办法响应`（要求与评分依据）。

其中商务、报价是空叶子，42/101 条需求仅通过 `UNMAPPED_REQUIREMENT` notice 闭包。对冻结输入进一步审计发现：101 条 requirement 中有 40 条 mandatory、61 条 informational，但现有 SQL compiler 把每个 requirement 的 fulfillment channel 全部写成 `narrative_content`；价格表、偏差表、资格证明、结构化表单和证据附件没有进入 `quotation`、`deviation_statement`、`structured_form`、`response_table` 或 `evidence_attachment`。同时，`kb_bid_v2_compile_requirement_set` 以“一个 SourceUnit = 一条 requirement”并用正则首命中分类，导致长合同/环保表格被误判为 qualification，表格单元格被拆成大量伪 requirement。

问题根因已定位：

1. Requirement compiler 没有原子化义务、正确 channel、applicability 和表单归并，输入给大纲的“必须覆盖什么”本身失真；
2. `reduce_outline_evidence` 保留了 373 个不同来源命名空间的结构片段，但没有形成唯一的“投标文件组成骨架”和小章节完备矩阵；
3. `synthesis_messages` 只要求提交 tender-derived nodes，没有规定先形成商务—技术—报价—其他附录的交付物结构；
4. `close_tree_shape` 会把模型输出的多个根机械挂到 cover 下，令树形 gate 通过但掩盖语义冲突；
5. `close_outline` 的路由降级会把 requirement 绑定到第一个同角色节点，缺少骨架路径约束；
6. `outline_validation` 校验来源和拓扑，不校验顶层角色、来源条款编号泄漏、空的组成章节、模板归并或 mandatory 小章节覆盖。

`1.png`（技术文件示例）与 `2.png`（商务文件示例）仅用于说明最终 Word 的目录观感和分册习惯，不进入运行时输入、prompt、artifact 或 provenance，也不提供可复制的小章节模板。运行时采用两层结构：第一层根据当前招标文件的投标组成语义归一为商务、技术、报价、其他附录（若招标文件明确采用不同分册/包件则按证据调整）；第二层完全依据当前冻结招标文件的组成条款、格式附件、mandatory requirements、技术规范和结构化表单生成并优化。资格通常归商务，格式附件归入证据指定的业务分支，须知/评标/合同只形成 requirement/binding，不直接复制成章节。

## Design invariants

- 商务、技术、报价、其他附录只是常见语义归一顺序，不是无条件硬编码的四节点模板；当前招标文件明确的投标组成、分册、包件和上传文件要求是唯一结构权威。
- 内部小章节必须由当前 frozen input 中的组成条款、格式附件、requirement、技术规范或结构化表单支持；参考图片、历史项目和通用经验不得充当 source evidence，也不得补造当前招标未要求的章节。
- 来源条款编号（如 `3.1.1`、`第六章`）保留在结构证据中，不直接作为投标文件显示标题。
- `OutlineNode.title` 保存纯语义标题；页面和 Word 根据树深度/ordinal 动态显示中文层级编号，重排后自动更新，避免把编号硬编码进标题。
- `第六章 投标文件格式` 是模板容器，不是输出一级章节；附件按 semantic role 归入商务、技术、报价或其他附件。
- 投标人须知、评标办法、合同条款默认是 requirement/evaluation context，不是输出章节。
- Rust 决定根和一级骨架；模型只能在已授权的骨架节点下补充子树和 route，不能再自由产生多个逻辑根。
- 任何自动 repair 都不得伪造 source IDs、need IDs 或 client refs。

## Evidence-derived acceptance example for current `testdata/bid`

以下目录只表示当前两份 `testdata/bid` 文件经过证据归并后应达到的验收结果，**不是写进代码或 prompt 的固定模板**。一级来自本招标文件 3.1.1～3.1.4 的明确组成；二级及以下只有在本次 frozen input 支持时生成，允许模型优化命名与归并，但不得遗漏 mandatory obligation：

```text
投标文件（cover root，不参与正文编号）
├── 目录（自动生成，不参与正文编号）
├── 一、商务文件
│   ├── 1. 投标函及承诺文件
│   │   ├── 投标函（1A）
│   │   ├── 法定代表人身份证明（1B）
│   │   ├── 授权委托书（1C）
│   │   ├── 廉洁自律承诺书（1D）
│   │   └── 廉洁合规承诺书
│   ├── 2. 商务响应与偏差
│   │   ├── 商务条款响应
│   │   ├── 商务偏差表
│   │   ├── 投标保证金承诺/格式（附件6）
│   │   └── 履约保证函（附件7）
│   ├── 3. 资格审查资料（附件8）
│   │   ├── 商务部分摘要表（8A）
│   │   ├── 股权结构及证明（8B）
│   │   ├── 投标人/工厂简介（8C）
│   │   ├── 财务状况及财务报表（8D）
│   │   ├── 资信证明及商业信誉（8E）
│   │   ├── 原厂资质等级证书（8F）
│   │   ├── 企业状况声明函（8G）
│   │   ├── 失信名单查询截图（8H）
│   │   └── 业绩情况表（8I）
│   ├── 4. 合同、质量、安全及环保承诺（仅纳入明确要求签署/响应者）
│   └── 5. 其他商务材料
├── 二、技术文件
│   ├── 1. 技术响应总说明与逐条响应
│   ├── 2. 通用技术要求
│   │   ├── 总则与一般规定
│   │   └── 标准和规范
│   ├── 3. 专用技术方案
│   │   ├── 环境条件
│   │   ├── 产品/设备方案
│   │   ├── 硬件技术规格
│   │   └── 主要技术参数表
│   ├── 4. 供货范围与备品备件
│   ├── 5. 项目实施与交付
│   │   ├── 供货进度与交货要求
│   │   ├── 包装与运输
│   │   ├── 安装、预装、调试与测试
│   │   └── 验收方案
│   ├── 6. 质量保证、技术支持与售后服务
│   ├── 7. 技术偏差表
│   ├── 8. 环保及合规技术响应
│   └── 9. 技术附表与附图
├── 三、报价文件
│   ├── 1. 开标价格表（附件9）
│   ├── 2. 投标价格表（附件2）
│   └── 3. 分项价格、税费及价格说明（仅在招标证据要求时）
└── 四、其他附录
    ├── 1. 投标人需要说明的其他内容（附件10）
    └── 2. 其他有证据支持且无法归入前三类的适用附件
```

明确标为“本次不适用”的附件3、附件5以及技术附表 B/D/E/F 不进入正常目录，只写入审计 notice。上述示例节点必须逐项由 source IDs、form IDs、need IDs 或 composition signals 证明；参考图片不作为 prior、不进入模型上下文，也不允许覆盖或补充 frozen tender evidence。

## Approach

### 0. RequirementCompileV3：先修正大纲的义务输入

将 `kb_bid_v2_compile_requirement_set` 中的 SQL 正则分类替换为 worker 中的 bounded、full-coverage requirement compiler；SQL 继续负责 immutable artifacts、CAS 和 publication：

- 将长 SourceUnit 拆成原子 requirement，而不是整页/整表一条 requirement；
- 同一结构化表单的标题、字段和单元格归并为一个 form obligation，不把每个表格单元拆成伪 requirement；
- 分类 `qualification/technical/commercial/pricing/delivery/evaluation/format/attachment/other`，分类依据包含 heading path、form identity 和上下文，不使用正则首命中；
- 生成正确 channel：正文 `narrative_content`、逐条响应 `response_table`、偏差 `deviation_statement`、表单 `structured_form`、证明材料 `evidence_attachment`、报价 `quotation`；
- 从“必须/应/不得/如有/本次不适用”等上下文生成 requiredness 和 applicability；
- 持久化 compiler Map/Reduce artifacts 与 contract hashes，支持 retry/replay；
- projection 只发布通过 schema、source closure、原子义务去重和 channel 校验的 RequirementSet。

### 1. Map：区分“来源目录”与“投标输出结构”

扩展 Map 结构片段契约，为每个 fragment 明确：

- `outline_usage`：`composition_spine | output_child | form_template | requirement_context | reference_only`；
- `applicability`：`required | optional | conditional | not_applicable`；
- `source_numbering`：保留 `3.1.1` 等来源编号；
- `composition_parent_role`：商务、技术、报价、附件、资格等归属角色；
- 继续保留 `path_segments`、`signal_kind`、source IDs、confidence。

Map 指令必须识别 `explicit_composition_clause`，例如将 3.1.1～3.1.4 识别为同一组成条款的成员，而不是四个可直接显示的标题。Map 只能读取 frozen tender SourceUnits/forms/requirements；禁止读取参考图、历史候选或通用模板来生成结构证据。

### 2. Reduce：生成 `CompositionSpineV1` 与 `SectionObligationMatrixV1`

在确定性 Reduce 中新增 `composition_spine` 和小章节完备矩阵：

- 从 `explicit_composition_clause`、分册/包件要求、上传文件要求中选择主骨架；
- 用 `explicit_format_clause` 和 form definitions 补齐各角色的必要子项；
- 用通用角色顺序排列已被招标证据支持的一级分区；
- 合并不同来源路径中的同角色节点，保留所有 source IDs；
- 将须知、评标、合同片段降级为 requirement context；
- 为每个一级分区产出 `required_children`、`conditional_children`、`excluded_children`、form identities、mandatory need IDs 和 source IDs；
- 每个 child obligation 使用由 contract + role + source identities 派生的稳定 `obligation_id`；
- 每个显式格式附件、结构化表单和 mandatory need 必须归入矩阵恰好一次；
- 产生冲突时保留 conflict artifact 并进入 repair，不把冲突路径并列挂到根下。

总体骨架角色顺序固定为 `commercial → technical → quotation → attachment`。qualification 默认是 commercial 的子体系；只有招标文件明确要求独立资格册/上传包件时改变成册边界，但页面仍保持四类业务导航。

### 3. Rust 预组装根与一级节点

新增确定性骨架装配器：

- 创建/选择唯一 cover root；
- 可选创建 TOC render node；
- 根据 `CompositionSpineV1` 创建稳定一级节点和 client refs；
- 节点标题保存语义标题（如“商务文件”），来源编号只留在 artifact；UI/Word 显示时渲染为“一、商务文件”；
- 以 composition member/source digest 派生稳定 refs，支持 checkpoint/retry；
- 禁止当前 `close_tree_shape` 将任意多根无条件挂到 cover；不符合骨架的根必须进入 repairing 或失败。

### 4. Synthesis：模型只补充骨架子树

在 `SynthesisPacketV1` 中加入冻结的 composition spine、允许的父节点、模板清单和 applicability：

- 模型不得创建/删除/改名一级骨架；
- 模型必须覆盖 `SectionObligationMatrixV1.required_children`，可以合并同义小章节但必须保留 obligation/source 映射；
- generation output 新增 `section_obligation_bindings`（`obligation_id → target_client_node_ref`）；一个节点可承载多个相容 obligations，但每个 required obligation 必须恰好绑定一次；
- 商务附件 1/4/6/7/8 等归入商务；附件 2/9 归入报价；技术附表归入技术；
- `not_applicable` 项不作为正常可选章节输出，按产品决策记录 notice；
- 技术章节依据第二卷技术规范和 DOCX 技术材料综合展开，而不是复制“第二卷/第五章”等来源容器；
- requirement routes 必须指向具体骨架路径，取消“找到第一个同 semantic role 节点”的宽松降级；
- mandatory needs 必须全部绑定；optional/informational 无法可靠放置时才允许 notice。

### 5. 语义校验与 repair gate

在 Rust validation 中新增：

- 一级节点必须与冻结 composition spine 一致且顺序一致；
- 一级标题不得泄漏来源条款编号（除非招标明确要求沿用该编号）；
- 有 required composition members 的商务/技术/报价节点不得是空叶子；
- `section_obligation_bindings` 必须只引用冻结 matrix IDs 和候选 node refs；`required_children`、mandatory needs、适用 format/forms 必须 100% 覆盖且不得重复绑定；
- format/template 节点必须归入对应角色，不得出现独立“第六章投标文件格式”一级分支；
- requirement-context 片段不得直接变成一级输出章节；
- 同一 composition role 不得出现未解释的多个一级节点；
- mandatory requirement 不允许用 `UNMAPPED_REQUIREMENT` 关闭；先 repair，仍未映射则本次生成失败；
- optional/informational requirement 可用有来源的 notice 关闭；
- repair 超预算时显式失败，不发布“结构合法但业务错误”的候选。

在 PostgreSQL 发布 gate 中镜像关键语义约束，避免绕过 Rust 发布错误候选；fresh baseline 和 live `CREATE OR REPLACE FUNCTION` 必须原子更新。

### 6. Word/UI 中文层级编号

复用 `LayoutSettingsV2.heading_numbering`、`numbered_section_titles` 和 `chinese_ordinal`，但收紧为适合正式投标文件的层级格式：

- 将 `semantic_role`/`render_role` 带入 `LayoutSectionV2` 和 render snapshot，使 cover/TOC/front matter 不参与正文标题计数；
- 新建投标项目的 document settings 默认 `heading_numbering=chinese`；若用户显式修改设置则尊重用户选择；
- 一级正文：`一、商务文件`、`二、技术文件`、`三、报价文件`、`四、其他附件`；
- 一级正文使用中文序号：`一、`、`二、`、`三、`、`四、`；
- 二级使用阿拉伯数字：`1.`、`2.`、`3.`，并在每个一级章节下重新从 1 开始；
- 三级使用层级数字：`1.1`、`1.2`，四级继续使用 `1.1.1`；
- 编号由 depth + ordinal 计算，不写入 `OutlineNode.title`，确保拖拽、增删和重新排序后 Word/页面编号一致；
- 候选审阅页面显示与 Word 相同的计算编号，API 中仍保留纯标题和结构 ordinal。

### 7. 契约版本、缓存与候选生命周期

- 更新 evidence/reduce/packet schema 与 contract hash；
- bump outline agent contract（建议 `outline-agent-v5`），使旧 V4 Map/Reduce 缓存不会污染新语义；
- 保留已解析 SourceUnits、RequirementSet 和冻结输入，重新运行 Map/Reduce/Synthesis；
- 保留同一 idempotency key 的网络重试回放；同 frozen input 已有 pending request 时复用 pending，避免双击并发；
- **用户在前一次生成结束后再次点击“生成大纲”时，即使 frozen input 未变化，也创建新的 generation request，不再返回历史 succeeded request**；
- 新请求成功发布 candidate 的同一事务内，将该 workspace 中更早的 `proposed` outline candidates 标记为 `obsolete`；accepted/rejected 历史记录保持不变；
- 新请求失败时不废弃旧 proposed candidate，用户仍能看到上一次成功结果；
- 前端默认 hydrate 最新 succeeded request 的非 obsolete candidate。

### 8. 前端审阅与可解释性

候选树继续使用现有 `CandidateReview`，补充：

- 一级骨架/来源依据摘要；
- mandatory unmapped、结构冲突、条件附件数量；
- 对 `not_applicable` 材料的清晰说明；
- 存在阻断质量问题时禁用“接受所选”，避免错误候选进入 workspace。

## Files to modify

关键文件：

- `crates/bidding/src/outline_agent.rs`
- `crates/bidding/src/outline_validation.rs`
- `crates/bidding/src/bid_authoring_contract.rs`
- `crates/bidding/src/tender_process.rs`（复用 SourceUnit/form provenance，必要时补充 heading context）
- 新增 `crates/bidding/schemas/requirement-compilation-output-v3.schema.json`
- 新增 `crates/bidding/schemas/composition-spine-v1.schema.json`
- 新增 `crates/bidding/schemas/section-obligation-matrix-v1.schema.json`
- `crates/bidding/schemas/outline-evidence-batch-v2.schema.json`（升为 V3）
- `crates/bidding/schemas/outline-reduce-plan-v1.schema.json`（升为 V2）
- `crates/bidding/schemas/outline-synthesis-packet-v1.schema.json`（或升为 V2）
- `crates/bidding/schemas/outline-generation-output-v1.schema.json`（升为 V2，加入 `section_obligation_bindings`）
- `crates/bidding/schemas/render-document-snapshot-v2.schema.json`（携带 section semantic/render role）
- `crates/bidding/tests/authoring_schema_contracts.rs`
- `crates/bidding/src/bid_authoring_v2.rs`
- `crates/bidding/src/render_v2.rs`
- `crates/bidding/src/workspace.rs`（仅在编号设置契约需扩展时）
- `migrations/bidding_v2_baseline.sql`
- `crates/worker/src/consume.rs`（契约版本/重试分类如需）
- `web/src/bid/api/types.ts`
- `web/src/bid/authoring/CandidateReview.tsx`
- `web/src/bid/authoring/session.ts` 及对应测试
- `web/e2e/bid-v2-flow.spec.ts`（真实候选审阅断言）

## Reuse

- `partition_source_units` / `verify_partition_coverage`：保持全量 Map 覆盖。
- `normalize_structure_fragments`：扩展而不是另建不兼容清洗路径。
- `priority_structure_unit_ids`：继续优先读取 composition/TOC/format evidence。
- `DraftAccumulator`、checkpoint、packet、trace：继续保证可恢复生成。
- `outline_validation::validate_outline_output`：作为统一 Rust 发布前入口。
- RequirementSet/Projection immutable artifact、CAS、stage receipt：保留 publication 机制，只替换粗糙的 SQL 正则编译内核。
- `kb_bid_v2_outline_tree_valid`、sources/requirement closure gates：保留并叠加 semantic spine/obligation matrix gate。
- `LayoutSettingsV2.heading_numbering`、`numbered_section_titles`、`chinese_ordinal`：复用现有动态编号能力并改为层级感知。
- `CandidateReview` 现有树展示与接受/拒绝流程：复用，不重做编辑器。

## Steps

- [ ] 定义并冻结 RequirementCompileV3、`CompositionSpineV1`、`SectionObligationMatrixV1` 与 fragment 分类契约。
- [ ] 将 RequirementSet 编译从 SQL 正则升级为原子义务、正确 channel、applicability 和 form 归并。
- [ ] 更新 Map schema/prompt/normalization，正确标记组成条款、模板和 requirement context。
- [ ] 在 Reduce 中实现证据优先级、角色合并、applicability、composition spine 与 obligation matrix 产出。
- [ ] 用 Rust 确定性创建 root/TOC/一级骨架，替换机械多根闭合。
- [ ] 约束 synthesis 只能在骨架下生成子节点、requirement routes 与 section obligation bindings。
- [ ] 收紧 route/obligation 匹配，要求 mandatory needs 与 required children 100% 覆盖。
- [ ] 扩展 Rust 与 SQL semantic validation。
- [ ] 更新 schema hashes、agent contract、缓存失效和同 frozen input 主动重生成语义。
- [ ] 实现中文层级编号的 UI/Word 同源渲染，排除 cover/TOC 计数。
- [ ] 更新候选审阅 UI 的质量摘要和接受阻断。
- [ ] 增加单元、schema、SQL baseline、API 与浏览器回归。
- [ ] 使用 `/opt/github/KnowledgeBrain/testdata/bid` 两份真实文件重新生成新候选。
- [ ] 在 `http://192.168.92.42:28080/` 验证一级顺序、全部子树、bindings/notices、刷新恢复和截图。

## Verification

### Automated

- Requirement compiler fixture：长 SourceUnit 原子化；报价/偏差/表单/证明分别进入正确 channel；表格单元不生成伪 requirement。
- Map/Reduce fixture：3.1.1～3.1.4 必须形成一个 composition spine，不能成为来源路径的孤立顶层。
- Obligation matrix fixture：每个 applicable form、mandatory need 和格式附件生成稳定 obligation ID，并通过 `section_obligation_bindings` 恰好绑定一个候选节点。
- Template merge fixture：附件 1/4/6/7/8 → 商务，附件 2/9 → 报价，技术附表 → 技术。
- Context exclusion fixture：须知、评标、合同不得成为一级输出章节。
- Anti-template fixtures：使用组成方式不同的资格独立分册、服务类招标、多包件/多文件上传样例，证明不会机械输出 `testdata/bid` 的小章节或无证据的商务—技术—报价四节点。
- Provenance fixture：所有 emitted node 和 obligation binding 必须能追溯到当前 frozen input；参考图和历史 candidate identity 不得出现在 packet/output provenance。
- Applicability fixture：`本次不适用` 按确认策略处理。
- Numbering fixture：cover/TOC 不计数；一级依次渲染 `一、二、三、四`；二级在各一级章节下从 `1.` 重新计数；三级/四级使用 `1.1`/`1.1.1`；重排后编号自动更新且 title 不含硬编码编号。
- Multi-root fixture：任意多根不得被静默挂到 cover。
- Validation fixture：空商务/报价、顶层 `3.1.1`、独立“第六章格式”、required child 缺失、任何 mandatory unmapped 均失败。
- `cargo test -p bidding --lib`
- authoring schema contracts、knowledge chat/SSE focused tests、`cargo check -p worker -p api`、`cargo fmt --all -- --check`
- disposable PostgreSQL baseline 与 semantic/tree/source/requirement gates；自动测试禁止访问 live `:15432`。
- web tests、TypeScript build、Playwright candidate-review 回归。
- Word render fixture：导出的 DOCX/XML 中 cover/TOC 无正文编号，一级为 `一、二、三、四`，二级在每个一级下从 `1.` 重置。

### Real end-to-end acceptance

使用与 `testdata/bid` SHA-256 一致的 DOCX/PDF，通过 owner JWT、CAS、idempotency 和公共 API 重新触发：

- 请求成功且 Map 覆盖所有冻结 SourceUnits；
- 一级业务顺序为商务 → 技术 → 报价 → 其他附件（若招标证据要求）；
- 页面不显示孤立 `3.1.1` 或独立“第六章投标文件格式”；
- 页面与导出 Word 的一级正文编号一致显示为 `一、商务文件`、`二、技术文件`、`三、报价文件`、`四、其他附件`；
- 商务、技术、报价均有证据驱动的完整子章节；
- 须知/评标要求体现在 bindings/检查项，而不是错误一级章节；
- 40 条现有 mandatory requirement 在重新编译后全部由正确 channel 绑定，不能以 unmapped notice 关闭；
- 刷新后同一 candidate 可见，API/console 无错误；
- 保存完整页面截图及候选 JSON/SQL gate 证据。

## Confirmed decisions

1. **证据驱动总体骨架**：商务、技术、报价、其他附录是常见归一结果；实际一级和小章节必须匹配当前冻结招标文件。参考图片只说明成品形态，不进入生成逻辑或证据。
2. **章节动态编号**：一级正文使用中文 `一、二、三、四`；二级使用阿拉伯数字 `1.`、`2.`、`3.` 并在每个一级章节下重新计数；三级/四级使用 `1.1`/`1.1.1`。底层保留纯语义 title，由 UI/Word 动态编号。
3. **不适用材料**：明确标记“本次不适用”的附件不生成正常章节，只保留审计 notice。
4. **完整性门**：mandatory requirement 不能用 unmapped notice 关闭；自动 repair 后仍无法覆盖则本次生成失败，不展示缺项候选。
5. **主动重新生成**：前一次生成结束后再次点击“生成大纲”必须创建新 request；新候选成功后旧 `proposed` candidate 自动 `obsolete`。新生成失败则保留旧候选。

## V8 closure addendum (approved after real V7 repair-churn evidence)

真实 V7 请求显示 append-only repair 会在 mandatory IDs 已齐全后继续累积 duplicate、wrong-section 和 source-disjoint bindings。V8 因此冻结以下附加约束：

- `OutlineEvidenceBatchV4` 只负责结构、表单、context 与 applicability；删除 Rust 对“不适用/如有/偏差/响应表”等标题文本的语义 fallback。
- Mandatory need 的语义分组不进入 Source Map 大输出，而由独立 `RequirementGroupingBatchV1` 处理；每批最多 64 个稳定 home need occurrences，适用 mandatory need 必须恰好分类一次。
- `FulfillmentGroupV1` 是模型语义分组事实；Rust 只按完全相同的 group key、section、title、materialization 合并，禁止模糊归并。
- `SectionObligationMatrixV2` 只引用 required/conditional/excluded group refs；`required_children` 不再复制所有 atomic mandatory requirements。
- `OutlineReducePlanV3` 包含 `CompositionSpineV1`、groups 与 Matrix V2，不再包含要求模型机械复制的 requirement routes。
- 模型节点使用 `coverage_group_refs` 显式声明语义覆盖；source overlap 只是合法性门，不能由 Rust 单独用来选择 target。
- `OutlineDraftPatchV1` 使用 draft SHA CAS，原子 add/replace/delete；非法 patch 不写 checkpoint。
- Rust 从唯一合法 group assignment 同源派生最终 `section_obligation_bindings` 与 requirement routes；最终公开输出继续使用 `OutlineGenerationOutputV2`。
- `SynthesisPacketV3` / `CheckpointV3` 只保存 nodes、patch receipts、closure facts 与冻结事实，不保存 reasoning，也不保存独立 route/binding chunks。
- Repair 只接受能严格缩小 unresolved identity 集的原子 patch；重复 fingerprint 或无改善立即失败。
- V5/V6/V7 artifacts 保持不可变；V8 使用新 agent contract identity `00000000-0000-5000-8000-000000000108`。
