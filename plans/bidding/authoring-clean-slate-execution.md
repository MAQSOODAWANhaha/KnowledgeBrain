# 招投标编制 Clean-Slate 执行计划

> 状态：**已批准，按顺序实施**。  
> 产品与领域契约：[`../../docs/bidding/authoring.md`](../../docs/bidding/authoring.md)。  
> HTTP、Schema、Job 与领域细节：[`tender-to-submission-v2.md`](tender-to-submission-v2.md)。  
> Web 交互：[`frontend-authoring.md`](frontend-authoring.md)。  
> 本文是唯一执行顺序；若旧计划中的 Phase、first-launch、兼容或 cutover 描述与本文冲突，以本文为准。

## 1. 不可变实施原则

1. 招投标旧实现直接删除；不兼容、不迁移、不双写、不保留 façade、alias、feature flag 或旧数据读取。
2. 只删除招投标域的 V1 API；平台 auth、knowledge 等其它领域版本不在本计划删除范围。
3. 不保留 first-launch、manifest checksum、catalog allowlist、intended-state 或启动 verifier；使用普通 schema bootstrap 与普通服务启动。
4. 旧业务语义删除前，只允许把目标系统仍需的通用原语移动到中立模块；移动不是兼容层。
5. 可复用 ObjectRegistry、DocReader/docparser、KnowledgeRetrieval 算法、Oxana transport、QuoteSnapshot、字体/图片/PDF/DOCX底层能力；不得复用 PartSet、Gate、route/part matching 或旧 Submission workflow。
6. 每个任务结束仓库必须编译；中间阶段允许招投标功能不完整，但不允许 mock 成功、死按钮或 V1 回退。
7. 业务风险只产生 Assessment；CAS、Schema、资产、事务和 renderer 技术错误 fail-closed。
8. 用户可随时改树、正文、表格、图片、附件和要求映射；AI 只生成 Candidate，且只能由用户触发。
9. Web 步导航始终只有 `files | authoring | export`；DocumentSet、checkpoint、matching、quote、requirements、settings 不成为强制向导步骤。
10. 一个项目恰好一个 project-wide Workspace 和一份投标输出主文档。

## 2. Phase 0 — 建立 V2-only 代码基线

### P0-A 保存规则与删除扫描

- 更新计划索引和旧计划状态，声明本文为唯一执行顺序。
- 新增删除扫描，禁止招投标生产路径出现：first-launch、`/api/v1/bids`、PartSet/part key、SubmissionGateV1、BidDeliveryV1、独立 EvidenceMatch Job、binding current pointer、兼容 façade、双写和自建 continuation/fan-in/fan-out。
- 扫描允许规范文档在“禁止/历史”语境提及旧名，但不允许生产调用。

### P0-B 提取目标系统仍需的通用原语

- `RequestIdentity` / `MutationContext` 移到 authoring/shared idempotency 模块。
- QuoteSnapshot 保留为唯一专用业务快照。
- 从旧 Part renderer 中移动 DOCX/PDF、字体、图片和附件准备底层能力；删除按 part key 编排的渲染。
- 保留 Tender upload validator、ObjectRegistry、DocReader、KnowledgeRetrieval 与 Oxana transport。
- V2 模块改为引用新位置后，立即删除旧容器中的对应代码。

### P0-C 删除 first-launch

- 删除 `deploy/first-launch/**`、verifier/evaluation/intended-state、catalog allowlist、handoff/checksum/marker。
- 删除 API、Worker、Retention 启动 gate。
- 从 shared schema、postgres-init、Compose、CI 与脚本删除 verifier role、verification table/function/trigger 和 first-launch service/env。
- 可保留普通 app owner、runtime roles 和普通 migration owner。
- 替换为直接、可重复的 schema bootstrap；不在运行时做 schema manifest 门禁。

### P0-D 删除招投标 V1

- 删除 V1 Web、parts/gate/matching/submission 路由和 client。
- 删除 `/api/v1/bids/...`。
- 删除 BidDeliveryV1 Queue/Job/Worker。
- 删除 PartSet、固定六部分、Markdown part、SubmissionGateV1、旧 profile/procedural API/表、route/part matching 和旧 submission 状态机。
- 删除 `migrations/bidding_v1_baseline.sql`、V1 测试和 acceptance 脚本。
- 旧数据库直接废弃，不提供升级路径。

### P0-E 建立最小 V2-only 运行骨架

- 招投标只挂 `/api/v2/bid-projects` 与 `/api/v2/submission-workspaces`。
- 未形成真实闭环的按钮和请求暂时不展示，不返回 mock 成功。
- active queue 只注册已实现且有 handler 的 Job。
- 使用随阶段演进的单一 fresh bidding schema；不以当前完整 V2 SQL 中“表已存在”冒充功能完成。

### Phase 0 验收

- fresh schema 可创建；API/Worker/Retention 可启动。
- 仓库编译，删除扫描通过。
- 招投标生产路径无 V1、first-launch、Part/Gate、兼容与双写。
- 通用原语有目标语义测试，未随旧业务代码误删。

## 3. Phase 1 — Tender Source 与 RequirementSet

- 收口 PDF、DOCX、XLSX、PNG、JPEG、WebP 的 magic/container/结构校验。
- 使用封闭 `SourceUnitSpanV2`；验证 kind/locator、range、parent、ordinal、预算和确定排序。
- TenderDocumentProcess 冻结转换、OCR/VLM、ObjectRegistry 与 SourceUnit revision identity，只发布单 document 来源。
- 完成 document role、typed relation、DocumentSetRevision、DispositionSetRevision。
- 每个 SourceUnitRevision 在所选 DispositionSet 中恰好出现一次。
- 完成 RequirementSource、AtomicRequirement、局部 supersession 与唯一 WorkspaceRequirementProjection。
- DocumentSet/Disposition publication 在事务内发布唯一 RequirementSetCompile request，提交后使用 Oxana enqueue；唯一键为 project + DocumentSetRevision + DispositionSetRevision。
- 正式注册 TenderDocumentProcess 和 RequirementSetCompile Worker；不保存 continuation 状态。
- 文件页支持上传、状态、重试和可选 role/relation 修改；pending/failed/unresolved 只提示，生成时可静默冻结当前集合。

### Phase 1 验收

多文件上传 → 当前成功输入冻结 → SourceUnit/Disposition → RequirementSet/Projection 可查询；澄清只局部替代指定要求；owner、CAS、幂等、replay、stale delivery 和六格式 fixture 通过。

## 4. Phase 2 — Workspace 纯人工编制

- 实现唯一 project-wide Workspace、WorkspaceScopeRevision、WorkspaceRequirementProjectionRevision、默认 DocumentSettingsRevision、WorkspaceRevision/Head。
- 实现 node/block/binding lineage 与 revision、FulfillmentExprV1、SubmissionFulfillmentEvidenceRevision。
- WorkspaceRevision 冻结 scope + projection + settings + tree + blocks + binding occurrences + assets；无独立 binding current。
- 实现 Rust 封闭 ContentBlockV1：RichText、Table、Image、Attachment、StructuredForm、PageBreak、SignaturePlaceholder。
- 完成 ContentBlockV1 ↔ Tiptap adapter；正文禁止 heading，标题只来自树。
- 实现 workspace load、mutation、assets、binding 与默认 settings API；所有操作共用 WorkspaceHead `If-Match`。
- 接通现有 OutlineTree、DocumentCanvas、SectionEditor、draft queue；409 保留草稿，poll 不覆盖人工内容。
- 本阶段不做完整 Settings UI。

### Phase 2 验收

空树手建章节 → 增删改名/拖拽/拆合 → 编辑正文 → 插表格/图片/附件 → 保存刷新恢复；并发冲突不丢稿。完成无 AI 可用的第一条产品竖切。

## 5. Phase 3 — 动态 OutlineCompiler

- 实现 OutlineGenerate Worker，冻结 DocumentSet、Projection、Scope、prompt/template/model/agent contract。
- 实现 agent schema、bounded verifier 和最多一次 repair；未知 role、伪造 identity、超深树和注入输入拒绝。
- 按招标明确组成、表格/表单、资格/技术/商务/评分要求生成动态树，无固定标题或 part key。
- 实现 OutlineCandidate overlay：默认全选、取消节点、部分接受、Workspace CAS、obsolete 拒绝。
- OutlineCheckpoint 是意图快照，不是审批锁；生成中和确认后都允许人工编辑。

### Phase 3 验收

用户触发生成大纲 → pending 不锁画布 → 部分接受 → 继续改树和正文；后台不会自动创建 OutlineGenerate request。

## 6. Phase 4 — KnowledgeRetrieval V3 与 Evidence

- V3 返回稳定 text quote 与可选 media identity，保留检索排序和 scope 语义；招投标不直接 join knowledge 表。
- 匹配目标改为 Requirement/OutlineNode。
- 实现 EvidenceBundle、EvidencePickSet、ProposedEvidenceSet、NO_EVIDENCE、EvidenceAssetArtifact 与 ObjectRegistry 持有。
- `POST .../evidence-matches` 只创建 `ContentGenerate(operation=match_only)` request，不建立 EvidenceMatch Job/Worker/状态机。
- 知识图片在 retrieval 时返回并由招投标立即冻结，不通过 OCR 文本反查 live 图片。
- 右侧 Evidence 面板支持 quote/图片、人工改选、章节刷新和全文覆盖概览；匹配不是强制步骤。

### Phase 4 验收

证据可回到真实 knowledge document/chunk/version/media；Tender Source 不能伪装投标方证据；NO_EVIDENCE 只提示，不阻断编辑或导出。

## 7. Phase 5 — ContentGenerate 与 Candidate Review

- 实现 ContentGenerate Worker，支持 `match_only|generate`。
- 生成只由用户触发，范围为 node/subtree/workspace，策略为 empty_only/append_candidate/missing_requirements_only，并支持冻结 InsertionAnchor。
- system-proposed 模式在同一 Job 内检索并冻结证据；user-pick 模式消费指定 PickSet；不建立 matching continuation。
- 实现 narrative、response table、structured form、evidence index、quote 和 deterministic generators。
- 校验业务事实 evidence_ref、bundle membership、图片 identity、表格网格、prompt injection 和封闭 Schema。
- CandidateReview 展示文字/表格/图片操作并允许部分接受；接受与人工 mutation 共用 Workspace CAS。
- 普通人工保存保留 dependency identity 和 stale；只有接受当前候选、显式核对或确定性重生成可绑定当前输入。

### Phase 5 验收

生成本章/子树/全部空章 → 查看证据与 diff → 部分接受 → 继续人工修改；过期候选不覆盖人工内容，后台不自动生成。

## 8. Phase 6 — Assessment、Preview、Render 与 Export

### P6-A Assessment 与 Preview

- 实现确定性 OutlineAssessment/SubmissionAssessment；以 workspace/projection/scope/settings/assets/QuoteSnapshot 复合 hash 复用，不创建 Assessment Job。
- 完成 Settings UI 与共享 LayoutDocument；HTML preview 使用冻结 WorkspaceRevision 和 DocumentSettingsRevision。

### P6-B V2 Renderer

- renderer 只遍历 node/block occurrences，不解析标题或旧 part key。
- 复用 DOCX/PDF、CJK字体、图片和PDF附件底层能力；实现复杂表格、结构化表单、目录、页眉页脚和页码。
- preview/review_draft/submission 共用冻结 settings 与 RenderStyleContract。

### P6-C SubmissionExport

- 实现单个 SubmissionExport Worker：SubmissionAssessment → AttachmentPreparation → verify → RenderDocumentSnapshotV2 → SubmissionManifestV2 → DOCX/PDF。
- 输出独立 Assessment report；知识来源只出现在 Web/审计/报告，不进入 submission 正文或脚注。
- review_draft 可带受控水印；submission 不含水印、风险提示或知识来源。

### Phase 6 验收

有业务 warning 仍可导出；技术错误稳定失败；修改后再次导出产生新文件；同一 RenderSnapshot 可重放，HTML/DOCX/PDF 使用同一 settings identity。

## 9. Phase 7 — 工程化收口

- 只收口已实现能力，不把未完成功能推迟到本阶段。
- 冻结最终 clean-install bidding schema/bootstrap；不引入 first-launch 或兼容迁移。
- active queue 只包含五类粗粒度 Job：TenderDocumentProcess、RequirementSetCompile、OutlineGenerate、ContentGenerate、SubmissionExport。
- 运行全仓删除扫描、Rust unit/SQL live/API/Worker/Agent fixture/Renderer golden/Web unit/Browser E2E。
- 更新部署、运行、故障恢复和审计文档。

## 10. 完成定义

只有同时满足以下条件才算完成：

1. 六格式文件可驱动 RequirementSet、动态大纲和 Workspace；
2. 无 AI 也能完成整篇人工编制；
3. AI 大纲、Evidence 和 ContentCandidate 均由用户触发且不覆盖人工内容；
4. Assessment 不阻断业务，技术错误 fail-closed；
5. DOCX/PDF/preview/报告按冻结快照可重放；
6. 招投标域无 V1、Part/Gate、first-launch、兼容、双写和旧 API/Job/SQL/Web；
7. fresh 环境、全量测试和浏览器黄金路径通过；
8. 实现文件、运行注册、Schema、API、Web 与本文阶段及契约一致。
