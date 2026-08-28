# 招标文件驱动的投标文件编制 V2 详细设计

> 状态：产品、领域、HTTP、Schema、Job 与 Web 详细合同已确认。唯一执行顺序见 [`authoring-clean-slate-execution.md`](authoring-clean-slate-execution.md)；其中“直接删除 Legacy V1、不兼容、不迁移、不双写、删除 first-launch”的原则和阶段验收覆盖本文旧有 Phase/cutover 描述。本文不得改变 [`../../docs/bidding/authoring.md`](../../docs/bidding/authoring.md) 的“用户主导、Assessment只提示、Word 式画布、clean-slate”目标契约。Web 详细设计见 [`frontend-authoring.md`](frontend-authoring.md)。

## Context

当前实现已经具备招标文件上传与转换、来源定位、条款/事实抽取、两路知识匹配、人工报价、固定 PartSet、manifest 和 DOCX/PDF 渲染基础，但最终组卷仍依赖固定 `1、2:*、3、4、5、6:*` part key，正文编辑仍是 Markdown Textarea，旧 `SubmissionGateV1` 仍具有业务阻断语义。该名称在本文只用于指认必须删除的旧实现，不属于 Target V2 设计。

面向用户的唯一主流程是：

```text
上传招标文件 → 解析招标内容 → 生成并编辑投标大纲 → 从知识库填充内容 → 导出投标文件
```

用户看见的步骤只有 **文件 → 编制 → 导出**。freeze / checkpoint / 台账是后台身份，不是「确认后才能改」的界面。

该主流程在当前领域设计中的内部映射是：

```text
同一 BidProject 的多份招标文件
→ 冻结 TenderDocumentSetRevision（生成大纲时如需要则静默）
→ SourceUnit 与原子要求台账
→ WorkspaceRequirementProjection
→ AI 建议的可编辑大纲树（Candidate overlay）
→ 用户在 Word 式连续画布改树、改字
→ 按节点匹配知识与生成 ContentCandidate（填充时如需要则静默 checkpoint）
→ advisory-only Assessment
→ RenderDocumentSnapshotV2
→ DOCX/PDF
```

## 实现优先级与简化约束

1. **P0：跑通主流程。** 优先交付上传、解析、大纲、知识库填充和导出五个可操作步骤；每个阶段都必须形成用户可验证的纵向切片。
2. **P1：补齐主流程必需能力。** 包括人工修改、来源追踪、失败重试、基本并发保护和可重复导出；只实现当前切片真实需要的边界。
3. **P2：体验与治理增强。** 高级Assessment、复杂资产治理、全面负例矩阵和切换治理在主流程稳定后继续，不得阻塞P0。

实施必须遵守：

- 不为未来可能需求预建第二套pipeline、通用调度框架、额外状态机或平行业务概念；优先复用现有上传、DocReader、知识检索和DOCX/PDF能力。
- artifact、CAS和冻结identity只保留保障来源可追踪、人工编辑不丢失、重试不重复以及导出可复现所必需的部分，不把合同复杂度本身作为交付目标。
- `migration-manifest.toml`、queue/cutover fixture和Phase 0合同维护不是Phase 1的主任务；仅在当前功能保持一致性确有需要或Phase 7切换时处理。
- 不得通过随意添加`#[allow(...)]`、关闭lint或弱化测试来绕过实现问题；应通过拆分函数、收敛类型或修正根因满足规范。
- 自动化工具超时本身不等于测试失败；以明确执行完成的测试、构建和验收结果为准。
- 若某项设计不能直接服务上述五步主流程，应延后、删除或降级为后续增强。

## 已确认产品原则

- 同一批上传文件肯定属于同一个招标项目；不做跨项目合并。
- 用户在任何阶段都能修改大纲和内容。
- AI 只生成候选，不直接覆盖人工内容。
- 缺项、偏离、评分损失和 stale 只提示，不阻断确认或导出。
- Target V2 不存在 `SubmissionGate`、`Gate PASS` 或 `Gate BLOCK` 业务概念；统一使用 advisory-only 的 `OutlineAssessmentSnapshot` 和 `SubmissionAssessmentSnapshot`。
- Schema 非法、CAS 冲突、资产丢失、渲染失败等技术错误必须 fail-closed；这是执行失败，不是业务 Gate。
- 不兼容旧固定 PartSet、旧 schema、旧 API 或历史数据；fresh baseline 一次性切换。
- 分阶段开发期间允许“未激活的V2源码/Schema/测试fixture”与当前V1实现暂时同库存在，以保证每阶段可编译验证；这不是运行时双模式：V2路由、queue registry和first-launch manifest在Phase 7前不得激活，Phase 7一次性切换并删除V1实现。
- V1招标输入必须支持 PDF、DOCX、XLSX 和图片；现有 API 只接受 PDF/DOCX，因此 XLSX 与图片需要新增受检 parser adapter。
- 每个BidProject恰好拥有一个project-wide SubmissionWorkspace，只生成一份投标文件。
- UI 采用 Word 式三栏：左侧独立大纲导航、中间按树前序展开的连续画布（聚焦章 Tiptap，其余静态）、右侧当前章证据/提示。不是 Word 桌面应用（无功能区、无用户模板、无邮件合并），也不是 Markdown `#` 大纲。
- 支持按单章节、子树或整份文档生成，但所有生成都由用户显式触发。
- 招标文件是输入源，只用于解析项目要求、表格结构、固定格式提示和大纲依据；不得把招标方提供的图片或附件自动当作投标方证明材料插入输出。
- 输出正文和图片只允许来自：知识库冻结证据、用户在当前Workspace人工插入的文字/资产、冻结QuoteSnapshot，以及基于上述来源生成的结构化内容。
- 用户为本次投标人工插入的证书、案例、图片和附件只属于当前 SubmissionWorkspace，不写入长期知识库，也不建立自动检索索引。

## Approach

按可运行的纵向切片推进；仅在主流程需要的位置使用深模块和不可变身份链，避免先横向铺开全部合同：

```text
TenderDocumentSetRevision
→ SourceUnitDispositionSetRevision
→ ProjectRequirementSetRevision
→ WorkspaceScopeRevision
→ WorkspaceRequirementProjectionRevision
→ OutlineCheckpoint
→ WorkspaceRevision（引用DocumentSettingsRevision）
→ SubmissionAssessmentSnapshot
→ RenderDocumentSnapshotV2
→ SubmissionManifestV2
```

每个模块通过小接口发布不可变 artifact；调用方不得读取内部候选表或根据标题、part key、live UI 状态重建另一份真相。

## 模块设计草案

### 1. UI：投标文件编制工作区

黄金路径只做三步，细节以契约 §2.4 和 [`frontend-authoring.md`](frontend-authoring.md) 为准：

- 文件：项目文件集合与解析状态；
- 编制：Word 式三栏。左独立大纲（点击跳转、拖拽调序/层级、增删改名拆合）；中 `DocumentCanvas` 按树前序叠章，聚焦章活 Tiptap，其余静态可点；右当前章证据/生成状态/提示；
- 导出：当前 `WorkspaceRevision` 的 DOCX/PDF；业务提示不禁用按钮。

另外：

- AI 大纲/内容候选只 overlay，默认全选、可取消或部分接受；
- 生成中、有候选、已 checkpoint 都不锁编辑器；
- 独立 HTML preview 若保留，只渲染冻结 WorkspaceRevision，不读 Tiptap 内存 JSON；
- 要求台账、报价、Document Settings 不是黄金路径必经步。

可复用现有实现：

- `web/src/bid/Workbench.tsx` 的项目数据加载；收成 files / authoring / export；
- `web/src/bid/FilesPane.tsx` 的多文件上传状态；
- `web/src/bid/authoring/OutlineTree.tsx`、`SectionEditor.tsx`、`adapter.ts`、`drafts.ts`；
- `web/src/bid/gfm.tsx` 仅作为旧内容参考，不作为 V2 编辑真源。

计划新增：`AuthoringShell`、`DocumentCanvas`、`StaticBlock`、`CandidateReview`。编辑器采用 Tiptap Adapter，前端编辑状态必须转换为后端拥有的 `ContentBlockV1`，不得把第三方编辑器 JSON 直接作为领域真源。

### 2. HTTP API 与 application interface

按资源拆分，不继续把全部逻辑堆在 `crates/api/src/bid_routes.rs`：

- `/projects/:id/tender-documents`：上传、角色、关系、转换状态；
- `/projects/:id/document-set-revisions`：冻结文件集合；
- `/projects/:id/requirements`：SourceUnit、disposition、要求、替代关系；
- `/projects/:id/workspace`：取得项目唯一输出工作区；项目创建时同事务建立默认 project-wide WorkspaceScope；
- `/workspaces/:id/outline`：大纲 proposal、mutation、checkpoint；
- `/workspaces/:id/content`：block mutation、资产 placement、stale 核对；
- `/workspaces/:id/candidates`：生成、查看、接受、拒绝；
- `/workspaces/:id/assessment`：大纲与提交提示；
- `/workspaces/:id/exports`：snapshot、manifest、render job、下载。

所有mutation使用authenticated actor、idempotency key和payload hash。CAS按聚合拆分：WorkspaceHead保护树、block、settings、OutlineFulfillmentBinding和Candidate decision；DocumentSet、DispositionSet、RequirementSet、supersession和projection分别携带自己的`expected_artifact_id/sha256`。创建资源只用幂等identity，不要求workspace CAS。

### 3. 后端领域与存储

计划在 `crates/bid` 内拆出深模块，在 `crates/storage` 中提供对应 repository：

- `tender_set`：同项目多文件角色、关系与冻结集合；
- `requirement_ledger`：SourceUnit disposition、AtomicRequirement、FulfillmentExpr、supersession；
- `workspace_scope`：固定project-wide范围与projection；
- `outline`：OutlineCompiler、树 mutation、lineage、checkpoint；
- `authoring`：WorkspaceRevision、ContentBlock、Asset placement、stale；
- `candidate`：OutlineCandidate/ContentCandidate 终态机；
- `assessment`：advisory-only Outline/Submission Assessment；
- `render_v2`：RenderDocumentSnapshotV2、manifest 和 DOCX/PDF adapter。

fresh cutover删除`migrations/bidding_v1_baseline.sql`并新增唯一`migrations/bidding_v2_baseline.sql`；旧part、template slot、`SubmissionGateV1`和固定key函数最终删除。Target V2后端只发布Assessment snapshot，不发布业务Gate状态。每个project以唯一约束限制为一个project-wide SubmissionWorkspace。

### 4. Agent：招标内容解析与要求提取

流水线草案：

```text
PDF/DOCX/XLSX/PNG/JPEG/WebP 招标输入
→ 按媒体类型选择受检 parser/OCR adapter
→ docparser/DocReader转换或结构化抽取
→ 冻结 source artifact
→ 结构解析（章节、段落、表格、表单区域）
→ typed SourceUnitRevision（section/table_row/form_region/attachment_region/image_ocr_region）
→ SourceSpanV2 locator
→ RequirementExtractionCandidate
→ bounded verifier
→ SourceUnitDispositionSetRevision
→ 人工修订/确认
→ ProjectRequirementSetRevision
```

Agent只输出bounded proposal：`source_unit_revision_id`、候选义务文本、requiredness/compliance建议、fulfillment建议、来源和置信度。`SourceSpanV2`仅作为SourceUnit定位器。服务端负责Schema、范围、去重、supersession、表达式不变量和publication，并验证每个选入DocumentSet的SourceUnit在DispositionSet中恰好出现一次。

可复用现有实现：

- `crates/bid/src/tender.rs::outline_and_route`；
- 现有 `SourceSpanV2` 和 publication fencing；
- `crates/docparser` 的 PDF/DOCX 转换能力；现有 tender upload 的 `tender_document_media_type` 只验证 PDF/DOCX，计划扩展为统一 TenderInputAdapter registry，并为 XLSX/图片增加结构化抽取与OCR实现；
- `crates/bid/tests/tender_publication.rs` 的中文 UTF-8 span、标题与表格测试。

### 5. 大纲生成模块

`OutlineCompiler` 输入冻结的 DocumentSet、WorkspaceRequirementProjection 和 WorkspaceScope，输出不可变 `OutlineCandidate`：

1. 定位投标文件组成、投标文件格式、分册和顺序；
2. 从招标文件解析要求的表格/表单结构和必需附件说明，生成结构化节点建议；不依赖或填充外部DOCX模板；
3. 将正文型 fulfillment need 映射到显式章节；
4. 为未承载的资格、技术、商务和评分项建议节点；
5. 输出逻辑 binding、来源、冲突和提示；
6. 不直接写 WorkspaceHead。

文件解析和要求投影完成后，用户显式点击“生成大纲”创建 OutlineCandidate；系统不得后台自动改写大纲。用户编辑后确认 `OutlineCheckpoint`；确认不是锁，后续修改形成新的 WorkspaceRevision。

### 6. Agent：按大纲生成内容

按节点 `semantic_role` 和 fulfillment 类型选择 generator：

- narrative generator：技术、实施、服务等叙述内容；所有业务事实句必须携带指向EvidenceBundle的`EvidenceCitation`，仅连接、总结和组织语言可以无证据生成；
- response-table generator：逐项响应和偏离表；
- structured-table/form generator：根据招标输入解析出的列、行、字段和填写要求生成可编辑ContentBlock表格/表单；
- evidence-index generator：证明材料目录和引用；
- quote adapter：只消费已选报价快照；
- deterministic generator：目录和附件索引。

每次生成冻结workspace、checkpoint、requirement projection、目标node/block、`EvidenceSelectionInput`、prompt/generator contract和系统RenderStyleContract版本。用户可以选择“生成本章”“生成当前子树”“生成全部空章节”，三种操作都必须显式触发；后台不自动生成。`fill_policy`支持`empty_only|append_candidate|missing_requirements_only`，光标插入冻结`InsertionAnchor(node_revision_id, block_revision_id?, utf8_offset?)`。结果只进入`ContentCandidate`，人工逐块接受。匹配到的知识库图片允许Agent直接生成带provenance的ImageBlock候选；用户在Candidate diff中接受、删除或调整位置。系统不读取用户DOCX模板。

### 7. 知识库内容匹配

继续通过[`../../docs/knowledge-base/domain.md`](../../docs/knowledge-base/domain.md)的唯一跨域`KnowledgeRetrievalPort`，禁止招投标直接join知识库表。当前V2 hit虽然支持`source_type=image_ocr`，但只返回OCR文本和chunk identity，没有可插入输出的图片资产身份。独立计划[`../knowledge-base/bidding-evidence-media-v3.md`](../knowledge-base/bidding-evidence-media-v3.md)负责在knowledge ingestion/baseline建立`image_ocr chunk -> immutable image artifact -> ObjectRegistry`映射，并把唯一端口升级为`KnowledgeRetrievalPortV3`；不改变现有排序/scope，也不新增第二个检索端口。

```text
KnowledgeEvidenceMediaV1
  image_artifact_revision_id
  object_ref, sha256, media_type
  width, height
  page_ordinal/bounding_region
  frozen_document_display_name
```

招投标接受端口结果后立即冻结自己的`EvidenceAssetArtifact`并取得ObjectRegistry引用；后续知识库文档或版本变化不得改写已生成Candidate或Manifest。

建议分为：

- 产品技术证据：产品版本、白皮书、手册、参数、技术截图与能力说明；
- 公司/商务证据：资质证书、案例、服务能力、公司材料及其图片；
- 无证据项：明确 `NO_EVIDENCE`，不生成虚假事实；
- 输出EvidenceBundle：同时冻结可引用文本片段和可插入图片资产身份；图片必须来自V3端口返回的受检media snapshot，不能仅根据`image_ocr`文本反查live知识库；
- 证据选择支持两种模式：用户先建立`EvidencePickSetArtifact`，或系统从MatchingReport生成`ProposedEvidenceSetArtifact`；默认使用系统建议并在ContentCandidate中与正文/图片一起呈现，只有用户接受Candidate后才冻结为用户采用的evidence selection。

招标输入和输出证据严格分域：Tender Source只驱动要求、标题、响应表“招标要求”列和大纲结构；Knowledge Evidence才可被Agent自动引用为投标方事实、正文或图片。WorkspaceAsset不进入长期知识库，也不参加自动语义匹配，只能由用户在编辑器中直接插入；插入后冻结为ContentBlock依赖。

Agent输出的RichText需要把事实性span绑定到`EvidenceCitation(evidence_bundle_id, evidence_item_id, quote_range)`；服务端拒绝引用bundle外证据，并由Assessment提示无引用或弱引用事实。通用连接、章节过渡、排版性标题和不引入新事实的总结可以没有citation。

### 8. 输入结构、输出资产与渲染

- PDF/DOCX/XLSX/图片都是Tender Source输入；XLSX只解析sheet、单元格、合并区域、表格和文本，不提供浏览器Excel编辑器，也不把原工作簿当作输出模板；
- 招标输入中的表格/表单被规范化为`TenderStructuredFormDefinition`，大纲/内容生成器据此创建普通可编辑Table/Form ContentBlock；
- 知识库匹配图片和用户人工插入图片/附件通过 ObjectRegistry 与`EvidenceAssetRevision/WorkspaceAssetRevision`进入输出；
- 用户人工插入的PDF embedded-pages使用冻结`AttachmentPreparationRevision`；
- 不存在用户DOCX模板、外部模板填充或下载填写再上传模板流程；
- ContentBlockV1使用封闭Schema；
- RenderDocumentSnapshotV2冻结节点/块occurrence、系统`RenderStyleContractV1`、字体、页面几何和renderer contract；
- `SubmissionManifestV2`只读取冻结快照，不读取live状态；
- 业务提示不阻断导出，独立生成检查报告。

可复用现有实现：

- `crates/bid/src/render.rs` 的 DOCX/PDF、CJK 字体、图片缩放和附件页能力；
- `crates/storage/src/object_registry.rs`；
- 当前 manifest-only render 和 asset digest 校验思想。

### 9. Rust模块边界与调用方向

目标代码按领域能力拆分，避免继续扩大当前约1,400行的`tender.rs`、约1,000行的`render.rs`和约2,600行的`bid_routes.rs`：

```text
crates/bid/src/authoring/
  mod.rs                  # 只导出application use cases
  input.rs                # DocumentSet与SourceUnit组装
  requirement.rs          # RequirementSet、supersession、FulfillmentExpr
  outline.rs              # OutlineCompiler、树校验、Checkpoint
  workspace.rs            # Node/Block mutation与CAS命令
  evidence.rs             # MatchingReport -> EvidenceBundle/Selection
  generation.rs           # Outline/Content agent合同与Candidate
  assessment.rs           # advisory-only评估
  render_snapshot.rs      # WorkspaceRevision -> RenderDocumentSnapshotV2
  schema.rs               # 封闭serde类型与canonical hash
```

```text
crates/bid/src/render/
  mod.rs                  # 从snapshot分派，不读live表
  layout.rs               # ContentBlock -> 共享LayoutDocument
  docx.rs                 # docx-rs后端
  pdf.rs                  # printpdf/lopdf后端
  html.rs                 # Web全文预览
  assets.rs               # 图片、PDF页、字体和digest校验
```

```text
crates/storage/src/
  bid_tender.rs           # 输入、转换、SourceUnit、RequirementSet
  bid_workspace.rs        # Workspace聚合、revision/head、candidate CAS
  bid_evidence.rs         # matching、selection、EvidenceAsset冻结
  bid_export.rs           # Assessment、snapshot、manifest、render job/output
```

调用方向固定为`api/worker -> bid application -> storage/domain ports`。`bid`不得直接查询知识库表；`render`不得查询live workspace、matching或asset current表。现有`quote.rs`和`bid_quote.rs`保留为独立能力，只通过冻结QuoteSnapshot接入workspace。

### 10. Fresh baseline持久化设计

#### 10.1 输入与要求

保留并升级`bid_projects`、`bid_documents`、`bid_converted_source_artifacts`、`bid_section_artifacts`和`bid_source_span_artifacts`。media type封闭为PDF、DOCX、XLSX、PNG、JPEG、WebP。

新增：

- `bid_document_role_revision_artifacts/current`：系统建议、用户确认/修改的文件角色；
- `bid_document_relation_revision_artifacts/current`：`complements|clarifies|partially_amends|replaces|withdraws`及applicability；
- `bid_document_set_artifacts/items/current`：用户显式冻结的文件、role/relation、conversion/source身份和失败列表；
- `bid_source_unit_revision_artifacts`：`section|table_row|form_region|attachment_region|image_ocr_region`及稳定lineage/revision；
- `bid_source_unit_disposition_set_artifacts/items/current`：每个选入DocumentSet的SourceUnitRevision恰好一项；
- `bid_tender_structured_form_definition_artifacts`：sheet/行列/字段/合并区域及SourceUnitRevision来源；
- `bid_requirement_set_artifacts/items/current`：项目级要求台账；
- `bid_requirement_source_revision_artifacts`：RequirementRevision到SourceUnitRevision的多对多来源；SourceSpanV2只保存在SourceUnit中作为locator；
- `bid_requirement_supersession_revision_artifacts/edges`：只记录显式局部替代，不实现全局latest-wins；
- `bid_workspace_requirement_projection_artifacts/items/current`：V1唯一workspace的要求投影。

role/relation修改、DocumentSet冻结、DispositionSet、RequirementSet、supersession和projection publication各自校验自己的`expected_artifact_id/sha256`，不占用WorkspaceHead CAS。创建project/document/workspace只使用幂等identity。任何新publication写不可变artifact并原子移动本聚合current pointer；旧artifact不可更新。

#### 10.2 Workspace树与内容

新增：

- `bid_submission_workspaces`：`project_id UNIQUE`；
- `bid_workspace_scope_artifacts/current`：V1固定project-wide；
- `bid_document_settings_revision_artifacts`：受`RenderStyleContractV1`约束的A4、页边距、中西文字体、正文字号、行距、标题编号、页眉页脚和页码设置；
- `bid_outline_node_lineages`、`bid_outline_node_revision_artifacts`；
- `bid_content_block_lineages`、`bid_content_block_revision_artifacts`；
- `bid_workspace_revision_artifacts`；
- `bid_workspace_node_occurrences`、`bid_workspace_block_occurrences`、`bid_workspace_binding_occurrences`：冻结树、块和OutlineFulfillmentBinding revisions；
- `bid_outline_lineage_edges`：split/merge来源；
- `bid_workspace_heads`：唯一current pointer；
- `bid_outline_checkpoint_artifacts/current`。

创建项目时，`kb_bid_create_project_v2`在同一事务创建project、唯一workspace、scope、空WorkspaceRevision和head。所有人工树/块编辑只调用`kb_bid_commit_workspace_mutation_v2`：

1. 校验owner、`Idempotency-Key`和封闭operation schema；
2. `SELECT ... FOR UPDATE`并比较expected revision+sha256；
3. 写新的node/block/binding revision和occurrence集合；
4. 写新WorkspaceRevision并原子移动head；
5. 把受影响citation/dependency标记stale；
6. 写audit与首次receipt。

允许的operation闭集：`insert_node`、`rename_node`、`move_node`、`split_node`、`merge_nodes`、`delete_node`、`insert_block`、`update_block`、`move_block`、`delete_block`、`insert_asset_block`、`update_document_settings`、`bind_fulfillment`、`remap_fulfillment`、`unbind_fulfillment`、`acknowledge_stale`。不提供按标题、part key或整份Markdown更新的入口。

新增`bid_outline_fulfillment_binding_lineages`、`bid_outline_fulfillment_binding_revision_artifacts`和`bid_submission_fulfillment_evidence_revision_artifacts`。binding没有独立current pointer，由`bid_workspace_binding_occurrences`纳入WorkspaceRevision；`bind_fulfillment|remap_fulfillment|unbind_fulfillment`与树/block/settings mutation共用WorkspaceHead CAS。binding绑定Need与逻辑node/table/structured-form/quote目标；evidence绑定实际WorkspaceRevision、block/table-row/structured-value、asset或QuoteSnapshot revision。目标、projection或依赖变化只使evidence stale，不删除逻辑binding或人工内容。

#### 10.3 Candidate、证据和评估

新增：

- `bid_async_request_snapshot_artifacts/results`：只保存五类粗粒度业务request的冻结输入、用户可见`pending|succeeded|failed|obsolete`和结果identity；不保存delivery attempt、lease、retry/backoff或fan-out状态；
- `bid_outline_candidate_artifacts`、`bid_content_candidate_artifacts`、`bid_candidate_operations`；
- `bid_candidate_decision_receipts`：终态与幂等结果；
- `bid_evidence_match_reports/current`：从旧route/part语义改为requirement/node目标；
- `bid_evidence_bundle_artifacts/items`：文本quote和media snapshot；
- `bid_evidence_selection_artifacts/items`：`user_pick_set|system_proposed|accepted`；
- `bid_evidence_asset_artifacts`：知识库图片在招投标域的冻结引用；
- `bid_workspace_asset_artifacts`：仅人工上传；
- `bid_outline_assessment_snapshot_artifacts/current`；
- `bid_submission_assessment_snapshot_artifacts/current`。

Assessment使用`assessment_input_sha256`去重，hash包含WorkspaceRevision、RequirementProjection、WorkspaceScope、该WorkspaceRevision引用的DocumentSettingsRevision、冻结asset和QuoteSnapshot identities；不能只按workspace revision coalesce。

`kb_bid_accept_candidate_v2`必须在一个事务中验证candidate仍为`proposed`、base head一致、选择的operation index合法、EvidenceBundle完整，然后应用选中操作、创建新WorkspaceRevision、冻结正式evidence selection并把candidate置为`accepted`。head冲突时原子转`obsolete`并返回409；重复接受返回首次receipt。

#### 10.4 Render与输出

新增或clean-slate替换：

- `bid_render_style_contract_artifacts/current`：系统拥有的默认值、允许范围、标题层级、表格和页码渲染规则；用户只修改受控`DocumentSettingsRevision`，不能上传模板或任意样式代码；
- `bid_attachment_preparation_revision_artifacts`：冻结PDF页资产、顺序、geometry和digest；
- `bid_render_document_snapshot_artifacts/nodes/blocks/assets`：冻结`preview|review_draft|submission`及mode options；
- `bid_submission_manifest_artifacts/dependencies`：只为`review_draft|submission`发布；
- `bid_submission_output_artifacts/current`。

`SubmissionExport`业务job顺序固定为：计算SubmissionAssessment → 准备全部附件 → 验证preparation ready/digest → 创建RenderDocumentSnapshot → 创建Manifest → 渲染DOCX/PDF。Manifest创建只读取指定WorkspaceRevision引用的DocumentSettingsRevision、对应Assessment、已ready的AttachmentPreparationRevision和冻结asset/QuoteSnapshot revisions。review draft允许受控水印；submission验证无水印、风险提示和知识来源。`kb_bid_create_export_request_v2`不检查业务PASS/BLOCK，只在schema非法、identity链不闭合、资产/preparation缺失或digest错误时失败。

### 11. Durable job图

只使用一个Oxana queue。queue registry声明queue/task identity、handler、request snapshot要求和能力；Oxana Queue/Worker配置拥有concurrency、unique job、delivery attempt、retry和backoff。业务数据库不实现第二套scheduler、lease、retry counter、fan-out/fan-in或continuation state machine。fresh deploy只注册五类粗粒度业务job：

```text
TenderDocumentProcess
RequirementSetCompile
OutlineGenerate
ContentGenerate(operation = match_only|generate)
SubmissionExport
```

- `TenderDocumentProcess`：对一个冻结document request顺序完成conversion、extraction和该document的SourceUnit publication，不发布项目RequirementSet，也不负责猜测或自动冻结项目DocumentSet；
- `RequirementSetCompile`：只从用户显式冻结的`DocumentSetRevision + SourceUnitDispositionSetRevision`确定性编译RequirementSet/Projection。成功冻结DocumentSet时必须同时发布覆盖其全部SourceUnit的初始DispositionSet并创建/enqueue compile request；同一DocumentSet后来发布新DispositionSet时也必须创建/enqueue对应request。这是确定性输入处理，不是AI生成；
- RequirementSet发布事务锁定项目current并比较request冻结的`document_set_sequence + disposition_set_sequence`与current RequirementSet所引用的输入：更旧输入no-op，完全相同输入重放首次receipt，更高DocumentSet sequence或同DocumentSet下更高DispositionSet sequence可以原子推进current。不得用创建request时的`expected_requirement_set_current`硬拒绝较新输入；
- `OutlineGenerate`：只由用户触发，发布OutlineCandidate；
- `ContentGenerate(operation=match_only)`：用户点击“匹配资料”时检索并冻结EvidenceBundle/MatchingReport，不生成正文；
- `ContentGenerate(operation=generate)`：只由用户触发；system-proposed模式在同一job内检索/冻结证据并生成Candidate，manual PickSet模式消费已冻结pick；
- `SubmissionExport`：顺序执行Assessment、附件准备及验证、RenderSnapshot、Manifest和DOCX/PDF，不拆分附件或render子job。

每个job只携带不可变request identity。通用queue unique ID为`kind:request_artifact_id:revision`；`RequirementSetCompile`明确使用`requirement_set_compile:project_id:document_set_revision_id:disposition_set_revision_id`，同一冻结输入只发布一次，而不同Disposition revision可产生不同RequirementSet。数据库只保存request snapshot、结果identity和用户可见业务状态。Assessment是确定性函数，在workspace响应需要时同步计算，导出时在SubmissionExport内部重新计算，不单独排队。

粗粒度job内部每个阶段使用`request_artifact_id + stage_kind + frozen_input_sha256`派生确定性identity，并保存首次artifact/receipt。queue重投时，已发布阶段校验同一identity后no-op并重放首次receipt，只执行尚未发布阶段；不增加transport attempt、lease或retry状态表。

API首次POST在事务中创建不可变request artifact和幂等receipt，再尝试enqueue。enqueue不确定时返回503和稳定request identity；客户端重放原POST及相同`Idempotency-Key`时，服务端加载同一request/首次receipt，并在业务状态仍为`pending`时再次尝试enqueue同一个Oxana unique job，不能因为receipt已存在而跳过enqueue。终态request只重放首次业务receipt。不提供retry endpoint、outbox、reconciler、scheduler或lease。

Outline允许基于当前冻结DocumentSet生成；pending/failed/unresolved输入进入Assessment，后来完成的输入使旧candidate/checkpoint stale，但不删除用户内容。OutlineGenerate、ContentGenerate的两种operation和SubmissionExport都必须由用户显式触发；RequirementSetCompile只是确定性输入编译，由成功的DocumentSet freeze或后续DispositionSet publication创建并enqueue。

### 12. HTTP资源与错误合同

clean-slate只暴露`/api/v2`，删除旧`/api/v1/bids/.../parts`和gate接口。`crates/api/src/bid_routes.rs`只组合以下子router：

#### 12.1 Project与Tender Source

```text
GET|POST /api/v2/bid-projects
GET       /api/v2/bid-projects/:project_id
POST      /api/v2/bid-projects/:project_id/end
GET|POST  /api/v2/bid-projects/:project_id/tender-documents
PATCH     /api/v2/bid-projects/:project_id/tender-documents/:document_id/role
POST      /api/v2/bid-projects/:project_id/tender-documents/:document_id/retry
GET|POST  /api/v2/bid-projects/:project_id/tender-document-relations
PATCH     /api/v2/bid-projects/:project_id/tender-document-relations/:relation_id
GET|POST  /api/v2/bid-projects/:project_id/document-set-revisions
GET       /api/v2/bid-projects/:project_id/document-set-revisions/:revision_id
GET       /api/v2/bid-projects/:project_id/source-units
POST      /api/v2/bid-projects/:project_id/source-unit-disposition-sets
GET       /api/v2/bid-projects/:project_id/requirements
PATCH     /api/v2/bid-projects/:project_id/requirements/:requirement_id
POST      /api/v2/bid-projects/:project_id/requirement-supersessions
GET       /api/v2/bid-projects/:project_id/workspace
```

Upload按magic bytes和容器结构验证，不信任扩展名/浏览器MIME：PDF尾标记、DOCX`word/document.xml`、XLSX`xl/workbook.xml`、图片由`image` crate完整decode。其它格式返回415。成功`POST document-set-revisions`必须在同一业务事务发布覆盖该文件集全部SourceUnit的初始DispositionSet和对应RequirementSetCompile request，提交后enqueue其唯一Oxana job；成功`POST source-unit-disposition-sets`同样发布并enqueue以新Disposition revision为输入的compile request。

#### 12.2 Workspace、Candidate与Evidence

```text
GET  /api/v2/submission-workspaces/:workspace_id
POST /api/v2/submission-workspaces/:workspace_id/mutations
POST /api/v2/submission-workspaces/:workspace_id/outline-candidates
POST /api/v2/submission-workspaces/:workspace_id/content-candidates
GET  /api/v2/submission-workspaces/:workspace_id/candidates/:candidate_id
POST /api/v2/submission-workspaces/:workspace_id/candidates/:candidate_id/accept
POST /api/v2/submission-workspaces/:workspace_id/candidates/:candidate_id/reject
POST /api/v2/submission-workspaces/:workspace_id/outline-checkpoints
POST /api/v2/submission-workspaces/:workspace_id/fulfillment-bindings
PATCH /api/v2/submission-workspaces/:workspace_id/fulfillment-bindings/:binding_lineage_id
DELETE /api/v2/submission-workspaces/:workspace_id/fulfillment-bindings/:binding_lineage_id
POST /api/v2/submission-workspaces/:workspace_id/nodes/:node_lineage_id/evidence-matches
GET  /api/v2/submission-workspaces/:workspace_id/nodes/:node_lineage_id/evidence
PUT  /api/v2/submission-workspaces/:workspace_id/nodes/:node_lineage_id/evidence-pick-set
GET|POST /api/v2/submission-workspaces/:workspace_id/assets
DELETE   /api/v2/submission-workspaces/:workspace_id/assets/:asset_revision_id
GET|PATCH /api/v2/submission-workspaces/:workspace_id/document-settings
GET|PATCH /api/v2/submission-workspaces/:workspace_id/requirement-projection
GET       /api/v2/submission-workspaces/:workspace_id/evidence-overview
GET       /api/v2/submission-workspaces/:workspace_id/assessments/current
```

`content-candidates` request只允许：

```text
target = node|subtree|workspace
node_lineage_id?               # node/subtree必需
fill_policy = empty_only|append_candidate|missing_requirements_only
insertion_anchor?              # node/block revision + optional utf8 offset
selection_mode = system_proposed|user_pick_set
pick_set_artifact_id?          # user_pick_set必需
expected_workspace_revision_id
```

fulfillment binding三个endpoint只是Workspace mutation的资源化入口，必须携带Workspace`If-Match`并在同一事务发布包含binding occurrence的新WorkspaceRevision，不能维护第二个binding current pointer。`evidence-matches`创建`ContentGenerate(operation=match_only)`request。

服务端使用用户明确选择的DocumentSetRevision，并以ID+SHA冻结对应requirement projection、matching policy、prompt、template、model和agent contract identities；不从live documents重建输入，也不接受调用方指定candidate/job/artifact ID、ObjectRegistry路径或知识库document/chunk身份。

#### 12.3 Preview与Export

```text
GET  /api/v2/submission-workspaces/:workspace_id/preview?mode=preview
POST /api/v2/submission-workspaces/:workspace_id/exports  # mode=review_draft|submission
GET  /api/v2/submission-workspaces/:workspace_id/exports
GET  /api/v2/submission-workspaces/:workspace_id/exports/:export_id
GET  /api/v2/submission-workspaces/:workspace_id/exports/:export_id/download
GET  /api/v2/submission-workspaces/:workspace_id/exports/:export_id/assessment-report
```

Export request冻结`mode=review_draft|submission`、所选WorkspaceRevision、format及mode options；只有review_draft允许watermark配置，submission带watermark或风险/知识来源渲染选项时返回422。preview不发布Manifest或正式output。

GET workspace/preview返回`ETag=workspace_sha256`。Workspace树/block/settings/binding和candidate decision要求Workspace `If-Match`；DocumentSet、DispositionSet、RequirementSet、supersession和projection请求携带各自`expected_artifact_id/sha256`；创建请求要求`Idempotency-Key`。enqueue不确定的503响应携带首次request identity；重放原POST及相同key时复用request/receipt，并在状态仍为`pending`时重试Oxana enqueue。统一错误：400 malformed request，401/403身份，404资源隔离，409对应聚合CAS/candidate obsolete，415输入格式，422 schema/asset引用非法，503 request artifact已提交但Oxana enqueue不确定。Assessment warning始终随200/202返回，不映射为4xx。

### 13. Agent输入输出合同

新增`AuthoringModel`测试seam，生产adapter复用现有model/enrichment HTTP能力，测试adapter使用冻结fixture。Agent只能读取job snapshot，不得在推理期间追查live表。

#### 13.1 OutlineGenerateV1

输入包含DocumentSet、SourceUnit dispositions、RequirementProjection、TenderStructuredFormDefinition和WorkspaceScope的canonical payload/hash。模型只返回client-local node refs、标题、`semantic_role`、`render_role`、父子顺序和requirement binding建议；UUID、revision、hash和状态全部由服务端分配。

服务端验证：树单根、无环、父节点存在、ordinal连续、角色为闭集、所有requirement identity属于输入projection、无固定part key/title假设、节点/深度/文本大小在policy上限内。无效输出允许一次受限schema-repair调用，仍无效则以`AGENT_OUTPUT_INVALID`终止job且不发布Candidate。

#### 13.2 ContentGenerateV1

输入按目标node/subtree/workspace裁剪，只带必要要求、已选EvidenceBundle、冻结QuoteSnapshot、当前blocks和表格/表单definition。输出是封闭`ContentBlockV1`和insert/append操作；光标插入只能使用request中冻结的InsertionAnchor，`missing_requirements_only`只能引用输入projection中尚未覆盖的Need：

- 文本事实span只能引用输入中的evidence item；
- image block只能引用输入中的EvidenceAsset或明确的人工WorkspaceAsset；
- 模型不得返回URL、object_ref、base64、任意SQL/HTML或未知Tiptap节点；
- `NO_EVIDENCE`要求只能生成占位/提示，不能编造事实；
- tender/knowledge文本按untrusted data封装，防止其内容被当作system instruction；
- table/form执行网格、rowspan/colspan、宽度和字段约束验证；
- candidate发布前重新比较冻结input hash，输入变化则标记obsolete。

`evidence_ref`作为自定义不可见RichText mark。Agent事实性span必须携带；人类编辑可以移除或使引用stale，但Assessment只提示、不阻断。

### 14. Web状态、编辑器与交互

Hash route 黄金路径只有三步，编制步用稳定 node lineage：

```text
#/bids/:projectId/files
#/bids/:projectId/authoring/:nodeLineageId?
#/bids/:projectId/export
```

`requirements` / `quote` / `preview` 若仍存在，只是次入口，不得做成向导前置锁。布局与交互不得偏离契约 §2.4：

- 左：可拖拽 OutlineTree，点击只负责跳转；节点增删改名、移动、split/merge；
- 中：`DocumentCanvas` 按树前序把各章叠成一篇；聚焦章 Tiptap，其余静态渲染、点击后聚焦；章标题来自树，正文关闭 heading；
- 右：当前章证据、生成状态、Assessment 提示；不设置强制独立匹配步骤；
- 顶部：生成大纲、填充（本章 / 全部空章）、导出 DOCX/PDF；
- 可选独立 Preview：后端基于冻结 WorkspaceRevision 生成 HTML，不直接渲染 Tiptap 内部 JSON。

Tiptap packages：React、StarterKit、Underline、Link、Table/Row/Header/Cell、Image 和自定义 EvidenceRef。`adapter.ts` 是唯一的 Tiptap JSON ↔ `ContentBlockV1` 转换点；领域 API 和数据库从不保存第三方 editor schema。

树 mutation 立即 CAS。正文短防抖 autosave；本地 draft 只有收到新 WorkspaceRevision receipt 后才清除。刷新与轮询不得冲掉未保存草稿。409 时保留 draft，不做 last-write-wins。CandidateReview 显示大纲树 diff 或文本/表格/图片 diff，允许部分接受；过期候选不覆盖人改。

### 15. Render实现

`render_snapshot.rs`先从所选WorkspaceRevision解析其`DocumentSettingsRevision`，再把树、blocks、资产和系统RenderStyleContract规范化为共享`LayoutDocumentV1`；禁止读取live settings pointer。HTML preview、DOCX和PDF都遍历同一LayoutDocument，避免三个后端各自解析业务状态。

- 标题级别来自node depth/render_role，不解析标题字符串；
- 目录使用冻结node occurrence及bookmark identity；
- table/structured form共享列宽、合并单元格和重复表头规则；
- Knowledge image与manual image使用同一缩放/crop路径，但manifest保留不同provenance；
- `evidence_ref`只作内部provenance，不在最终DOCX/PDF显示知识库文件名或脚注；证据来源只在Web Evidence面板、审计链和独立Assessment报告中展示；
- `SubmissionExport`先完成并验证全部AttachmentPreparationRevision，再发布RenderSnapshot与Manifest；
- RenderSnapshot冻结`preview|review_draft|submission`及mode options；review draft可带受控水印，submission必须无水印、风险提示和知识来源；
- render阶段只接收manifest ID/hash，沿用manifest-only no-live-read测试；
- DOCX与PDF验收比较标题、段落、表格文本、图片digest和附件页语义，不要求二进制相同。

## Files to modify

### 规范与计划

- `PRODUCT.md`、`DESIGN.md`
- `docs/bidding/authoring.md`
- `plans/bidding/frontend-authoring.md`
- `docs/bidding/current-code.md`
- `docs/knowledge-base/domain.md`（链接独立评审的V3 media合同）
- `plans/knowledge-base/bidding-evidence-media-v3.md`
- `plans/knowledge-base/README.md`
- `docs/bidding/README.md`
- `plans/bidding/README.md`
- `plans/bidding/current-code/tender-publication.md`
- `plans/bidding/current-code/matching.md`
- `plans/bidding/current-code/submission-export.md`
- `plans/bidding/current-code/implementation-acceptance.md`

### Parser、Rust与数据库

- 删除`migrations/bidding_v1_baseline.sql`，新增`migrations/bidding_v2_baseline.sql`；
- `deploy/first-launch/migration-manifest.toml`、`crates/storage/src/first_launch.rs`、`persist.rs`、`scripts/fresh_schema_acceptance.sh`切换唯一V2 baseline；
- `services/docreader/proto/docreader.proto`、`models/document.py`、`parser/excel_parser.py`、`parser/image_parser.py`及测试：返回表格/单元格/图片OCR structured units；
- `crates/docparser/src/lib.rs`、`anydoc.rs`：保留office快速路径，图片强制走builtin OCR，透传structured units；
- `crates/bid/src/tender.rs`拆分到新增`crates/bid/src/authoring/*`；
- 删除`crates/bid/src/submission.rs`中的固定PartSet实现；
- `crates/bid/src/render.rs`拆分到新增`crates/bid/src/render/*`；
- `crates/bid/src/matching/*`、`crates/bid/src/lib.rs`；
- `crates/domain/src/knowledge_retrieval.rs`；
- `crates/storage/src/knowledge_retrieval.rs`及`knowledge_retrieval/*`；
- `crates/storage/src/bidding.rs`、`bid_matching.rs`、`bid_submission.rs`拆分到新增storage模块；
- `crates/runtime/src/work_transport.rs`、runtime tests；
- `crates/api/src/bid_routes.rs`拆分到新增`bid_*_routes.rs`；
- `crates/worker/src/consume.rs`；
- `deploy/queue-registry.toml`。

### Web

Web 文件级步骤以 [`frontend-authoring.md`](frontend-authoring.md) 为准，不在本文重写交互。至少包括：

- `web/package.json`（Tiptap 已在 `web/`，不要在仓库根另起一份）；
- `web/src/bid/Workbench.tsx` 收成 files / authoring / export；
- 新增 `DocumentCanvas`、`StaticBlock`、`CandidateReview`；
- `OutlineTree` 拖拽、`SectionEditor` 收成聚焦章编辑器；
- 删除 `part` 路由与 Markdown 编辑真源；更新 E2E 为黄金路径。

## Reuse

- 来源结构与回源：`crates/bid/src/tender.rs`、现有 SourceSpanV2；
- 文档转换：`crates/docparser`及现有DocReader`ExcelParser`/`ImageParser`；当前XLSX和图片parser可复用，但Tender upload validator、图片engine选择和structured-unit返回合同需扩展；
- 知识检索端口：`crates/domain/src/knowledge_retrieval.rs`、`crates/storage/src/knowledge_retrieval.rs`；复用其eligible scope、V2 exact/semantic检索、`image_ocr`和scope attestation，升级V3响应以携带冻结图片media identity；
- 匹配出版：`crates/bid/src/matching/*`；
- 报价快照：`crates/bid/src/quote.rs`、`crates/storage/src/bid_quote.rs`；
- 对象和资产：`crates/storage/src/object_registry.rs`、S3 adapter；
- durable jobs：`crates/runtime/src/work_transport.rs`单queue和`crates/worker/src/consume.rs`dispatch模式；registry只声明queue/task、handler、snapshot与能力，Oxana Queue/Worker配置复用unique job/retry/backoff/concurrency；Target V2只使用五类粗粒度job，不复制transport状态；
- DOCX/PDF基础：`crates/bid/src/render.rs`；
- Web项目壳与现有业务步骤：`web/src/bid/Workbench.tsx`、`Sidebar.tsx`。

## 实施顺序与阶段验收

### Phase 0：冻结合同和fresh cutover边界

- [x] 更新`docs/bidding/current-code.md`、knowledge retrieval V3合同和各旧bidding计划，使其引用本文而不继续定义fixed Part/Gate；
- [x] 将ContentBlockV1、workspace operation、agent input/output、EvidenceBundle、Assessment和RenderSnapshot JSON Schema放入`crates/bid/schemas/`并加golden hash测试；
- [x] 定义尚未注册到active runtime的`TenderDocumentProcess|RequirementSetCompile|OutlineGenerate|ContentGenerate(match_only|generate)|SubmissionExport`五类V2 job payload、稳定request identity和错误码；新增独立V2 queue-registry fixture验证queue/task、handler、snapshot与能力，并新增不注册到active worker的Oxana Queue/Job/Worker policy contract来冻结concurrency/unique/retry/backoff所有权；
- [x] 新建`bidding_v2_baseline.sql`和独立first-launch V2 manifest fixture，增加空库V2 baseline测试；不得在本阶段替换active manifest/queue registry或删除V1，以保持现有构建与测试可运行。

验收：V2 fixture证明空库只加载knowledge/shared/V2 bidding三个baseline；schema golden、五类queue kind mapping和V2 first-launch manifest测试通过；active runtime仍使用单一V1路径且现有测试保持通过，没有V1/V2双注册、双写或请求分流。

Phase 0第十二轮P1修复后实现证据（等待独立review verdict）：详见`plans/bidding/phase0-acceptance.md`。request/candidate initial INSERT分别强制`pending`与`proposed + decided_at NULL`；SubmissionExport request mode options采用exact-key/type检查，submission固定NULL watermark和两个false flags，review_draft冻结受控watermark/flags。完整Draft、focused Rust、clean V2 live、active V1 fresh-schema/checksum/isolation、cargo check/fmt和diff check均通过；Phase 0按实现证据重新勾选，Phase 1仍须等待独立review PASS。

#### Phase 0 review缺陷记录（修复后方可进入Phase 1）

第一轮独立review判定Phase 0 `BLOCK`，暴露出合同冻结实现不能只依赖schema字节hash和SQL字符串搜索：

- `SourceUnitDisposition`曾错误采用`accepted|ignored|reclassified|unresolved`，Requirement曾被压缩为`mandatory boolean`；必须改回权威合同的`requirement|non_requirement|unresolved`以及独立的`requiredness`、`compliance_policy`和lifecycle；
- generation input、binding、Assessment和RenderSnapshot缺少冻结identity或交叉约束；必须补齐workspace checkpoint、scope/style/projection identity、typed binding target/state、Assessment status/dependencies、required mode options和`html iff preview`；
- DocumentSet、DispositionSet、RequirementSet、Supersession、Projection的current pointer/CAS及跨document/workspace复合外键不完整；必须用同一复合identity关闭不可变链并增加负例DML验证；
- runtime payload version与registry fixture不一致，且尚未冻结inactive Oxana transport policy；Oxana `Job::name()`是关联静态函数，一个tagged enum不能同时暴露五个task name，因此Phase 0只冻结单queue的concurrency、unique conflict、resurrection、retry/backoff常量及测试，不构造或注册Worker；Phase 7 active cutover时再以五个thin Job/Worker adapter消费该policy。必须建立单一registry envelope version来源，不能把业务schema的`/v2`误当作`payload_version=2`；
- active/V2 manifest对同一knowledge/shared baseline必须校验为相同实际digest；V2 acceptance同时验证active checksum，不能只检查migration名称；现有active fresh-schema脚本曾假设`migrations/`目录只能含active manifest文件，这与Phase 0未激活V2 baseline fixture同库存在的阶段约束矛盾，必须改为显式允许该单一inactive V2文件，同时继续证明active migrator只读取V1 manifest；
- golden hash测试继续保留，但必须增加JSON Schema正反例和live PostgreSQL CAS/FK/append-only负例。上述缺陷已修复并在本地focused/live验收后重新勾选；Phase 0仍等待第二轮独立review verdict，不得据此提前进入Phase 1。

第二轮独立review中runtime合同通过，但schema/SQL验收仍为`BLOCK`，必须继续修复：

- `RequirementSet`发布仍错误接收request-time expected current；改为只锁定current并比较冻结input tuple，旧输入no-op、同输入重放、更高输入推进，允许queue乱序到达；
- binding的tagged union不能只在JSON中有类型，SQL必须验证每类`target_id`真实存在且属于同一workspace/QuoteSnapshot；
- 只有`review_draft`允许watermark，因此preview和submission都必须强制`watermark=null`；
- Draft 2020-12正反例必须覆盖十个authoring schema而非六个；live PostgreSQL必须实际覆盖DocumentSet、DispositionSet、RequirementSet、Supersession、Projection五个aggregate的advance/replay/stale、复合identity/current coherence和append-only；
- Phase 0验收命令必须由CI或checked-in acceptance记录证明真实执行，不能只在计划中写“通过”；V2 live harness readiness必须等待PostgreSQL最终ready而不是早期role sentinel。上述项目已修复、记录并重新勾选，仍须通过第三轮独立review后才能进入Phase 1。

第三轮独立review确认runtime、十schema执行、五aggregate live coverage、binding target和单调发布均已修复，但仍判定`BLOCK`，必须补齐：

- PostgreSQL `CHECK`的NULL语义会让缺失`mode_options`键通过；SQL必须用two-valued predicate或JSON containment要求preview/submission显式冻结全部键和值，并增加缺键负例；
- Manifest必须用复合identity绑定同一RenderSnapshot的`submission|review_draft` mode和format，preview不能产Manifest；Output format必须与Manifest format相同；
- RenderDocumentSnapshotV2必须冻结form definition和attachment preparation的ID+canonical SHA occurrence、显式schema/operation version，以及分别审批的DOCX/PDF renderer contract identity，不能只保存一个generic renderer identity；
- 对上述identity、缺键、preview→manifest和format mismatch增加Draft/live正反例并刷新golden/manifest digest。上述项目已实现并完成最终focused/live验收，仍须通过下一轮独立review后才能进入Phase 1。

第四轮独立review确认此前记录项均已关闭，但发现两条新的冻结链P1，必须先修复：

- WorkspaceRevision拥有的scope/projection/settings tuple必须作为一个复合identity被Assessment引用；RenderSnapshot必须复合引用与该workspace revision/settings一致的Assessment，禁止把同一workspace不同revision的合法依赖拼成从未存在的快照；
- RenderSnapshot attachment preparation occurrence只能引用`ready` revision，`pending|failed`不得通过仅ID+SHA的外键进入snapshot；必须增加status-qualified identity或严格insert trigger，并加入两个状态负例和mixed-revision负例。上述两项已通过复合外键、status-qualified identity和clean-container live负例修复；Phase 1仍须等待下一轮独立review PASS。

第五轮独立review确认历史缺陷均已关闭，但进一步发现Phase 0 frozen contract仍有P1，必须先修复：

- WorkspaceRevision parent/head以及Candidate base的artifact ID+SHA必须使用复合外键，SHA不能只是未关联文本；
- `SubmissionExport` payload只能接受`review_draft|submission`，`preview`只属于独立HTML preview/render snapshot；
- SQL必须持久化并验证完整RenderSnapshotV2 canonical payload，至少复合绑定workspace digest、checkpoint、ordered node/block occurrence、page geometry、typed font/ObjectRegistry identity，live positive不得使用任意bytes冒充schema-valid snapshot；
- EvidenceBundle必须复合绑定同一workspace/requirement的MatchingReport，EvidenceAsset必须绑定准确bundle item及可用ObjectRegistry ID+digest，禁止跨requirement或伪造media provenance；
- 增加wrong parent/head/base digest、invalid render payload/missing font/checkpoint、cross-requirement/unknown item/object digest正反例。上述第五轮缺陷已通过复合外键、共享ObjectRegistry status-qualified identity、canonical JSON trigger/relational occurrence投影以及clean-container负例修复；Phase 1仍须等待下一轮独立review PASS。

第六轮独立review继续发现Phase 0 canonical publication和provenance P1，必须修复：

- MatchingReport的knowledge attestation ID+SHA必须复合绑定`knowledge_matching_scope_attestations_v2`真实记录；ObjectRegistry identity还必须包含media type；
- RenderSnapshot `asset_revision_id`必须按provenance kind绑定真实workspace/knowledge/Quote资产revision和同一ObjectRegistry tuple，不能只有object digest；
- RenderSnapshot和EvidenceBundle canonical JSON必须执行完整closed-schema验证、定义不含self-hash字段的canonical hashing规则，并与全部projection严格等价；所有node/block/font/asset/form/preparation projection必须transaction-end完整、顺序一致且append-only；
- Manifest dependency必须按kind绑定真实artifact ID+SHA并强制完整集合，不能允许零依赖；
- 移除live fixture中的`\\if false`禁用块，确保所有历史负例真实执行；增加unknown field、坏UUID/provenance、重复occurrence、越界geometry、wrong canonical hash、cross-attestation/MIME、missing/extra/reordered projection和manifest dependency负例。上述第六轮项目已实现并通过focused、Draft 2020-12和clean-container live验收，Phase 0重新勾选；仍须通过下一轮独立review后才能进入Phase 1。

第六轮修复采用以下冻结规则后再实施：数据库以PostgreSQL `jsonb::text`作为唯一canonical serialization；RenderSnapshot的`content_sha256`计算`canonical_payload - 'snapshot_sha256'`，EvidenceBundle计算`canonical_payload - 'bundle_sha256'`，再将结果写回self-hash字段并要求二者一致。发布trigger执行closed schema的exact-key/type/format/range/union验证；deferred constraint trigger在事务末比较canonical ordered arrays与全部projection行。runtime role仍无直接表写权限且后续API只能调用发布函数，但Phase 0 live owner-path也必须证明绕过函数的malformed INSERT会被trigger拒绝，不能把权限边界冒充schema验证。Render asset的closed provenance为`knowledge_evidence|manual_workspace|prepared_attachment|quote_snapshot`：font只存在于独立font occurrence；prepared attachment通过其不可变page-asset projection绑定ObjectRegistry；QuoteSnapshot canonical bytes通过共享ObjectRegistry owner identity绑定，不建立第二套registry。

第七轮独立review仍判定Phase 0 `BLOCK`，必须补齐以下真实provenance和schema-equivalence：

- Evidence item row的`id/item_kind`必须与canonical payload中的`evidence_item_id/kind`相等，EvidenceAsset必须复合绑定Bundle所属workspace；`page_ordinal`只能是非负整数；所有required UUID/enum/null判断必须two-valued；
- live fixture必须先发布supported retrieval policy并调用knowledge-owned attestation procedure，不能owner直插伪造attestation；Phase 0提前建立仅用于冻结FK的KnowledgeImageArtifactRevision/ObjectRegistry identity和chunk mapping存储合同，Phase 4再实现检索/发布行为，EvidenceAsset `knowledge_evidence`必须引用该真实V3 media identity；
- Render/Evidence strict validator继续补齐null、additional field、范围、duplicate和canonical hash负例；projection exactness和append-only负例必须实际执行；
- Manifest完整依赖必须包含OutlineCheckpoint，并用独立显式oracle断言required kind/identity，wrong digest测试不得被duplicate unique violation掩盖；
- Manifest、render asset和evidence media所有ID+SHA/type都必须落到真实artifact复合identity，不能保留caller-supplied UUID终点。上述缺陷已修复并通过Draft、focused Rust、clean-container live、checksum和active-V1 isolation验收；Phase 0重新勾选，但Phase 1仍须等待下一轮独立review PASS。

第七轮实现中补充记录baseline顺序约束：first-launch固定先加载knowledge、再加载shared，因此knowledge baseline创建storage identity表时不能引用稍后才存在的`kb_object_ref`/`kb_sha256` domain、ObjectRegistry表或shared append-only函数。knowledge baseline必须先用等价的closed text digest/object-ref约束和knowledge-owned immutable trigger建表；inactive bidding V2 baseline在shared已加载后再追加ObjectRegistry复合FK。Phase 7保持该三baseline顺序，不允许为解决循环依赖而复制ObjectRegistry或改变active迁移图。

第八轮独立review发现以下剩余P1，必须修复：

- EvidenceBundle `created_at`在SQL中必须严格使用RFC3339 date-time lexical contract，禁止PostgreSQL宽松接受date-only或`infinity`；
- KnowledgeImageArtifactRevision的width/height以及可选page/bounds必须被EvidenceAsset复合绑定，不能由bundle caller伪造；
- AttachmentPreparation必须复合绑定同workspace source asset，并以canonical hash冻结ordered page asset/object digest/geometry，deferred projection必须严格等价；
- SubmissionOutput必须复合引用`available` ObjectRegistry的digest/media type/length及owner，正例不得引用未注册object；
- MIME负例必须使用fresh bundle/item并只接受目标FK/check错误；Manifest live oracle必须显式列出每个required dependency kind/ID/SHA/ordinal，不能仅复用production expected函数。修复并通过下一轮独立review后才能重新勾选Phase 0。

第八轮修复冻结以下实现规则后再改SQL：EvidenceBundle的`created_at`只接受RFC3339的`YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)`词法形式并要求round-trip到行timestamp；KnowledgeImage identity的复合键包含width/height/page/bounds；AttachmentPreparation canonical JSON以`payload - preparation_sha256`的PostgreSQL `jsonb::text`计算digest，且其source workspace asset与ordered page projection在事务末完全相等；SubmissionOutput以`output:<project_id>:<workspace_id>:<manifest_id>`作为ObjectRegistry owner identity并冻结object ref/digest/MIME/length/available；Manifest验收oracle在测试fixture中写死expected tuples，不调用production helper。所有负例使用fresh identity并只捕获预期SQLSTATE，禁止让unique violation掩盖目标约束。上述项目已实现并通过Draft、focused Rust、clean V2 live、active V1 catalog/checksum/isolation验收；Phase 1仍等待下一轮独立review PASS。

第九轮独立review中media/preparation和manifest专项均PASS，但综合review发现一条P1：OutlineCheckpoint必须通过复合外键绑定所选WorkspaceRevision实际拥有的RequirementProjection，不能把同workspace另一合法projection拼入checkpoint。该缺陷已通过WorkspaceRevision/Checkpoint的project+workspace+revision+projection ID+SHA复合identity、coherent positive和same-workspace cross-projection live负例修复；Phase 1仍须等待最终独立review PASS。

第十轮独立review确认checkpoint修复正确，但发现一条相邻P1：ContentGeneration request不能只保存opaque bytes；必须持久化并复合绑定同一project/workspace的base WorkspaceRevision、该revision拥有的RequirementProjection、对应OutlineCheckpoint、scope/settings/style/evidence selection和typed target identity。ContentCandidate必须引用同一content request/workspace/base tuple，禁止跨workspace base或引用非content request。该缺陷已通过`bid_content_generation_request_identities`、ordered EvidenceBundle projection、closed target/anchor约束和ContentCandidate复合FK修复；coherent与same-workspace cross-projection/cross-workspace/non-content-request/mismatched-candidate clean live覆盖均通过。Phase 1仍须等待最终独立review PASS。

第十一轮独立review发现request contract仍不完整，必须修复：

- ContentCandidate的`request_operation`必须非NULL且恒为`generate`，workspace target identity必须非NULL；ContentGeneration request须补齐outer selection digest、system matching policy、quote snapshot、prompt/model identity，并使manual PickSet复合绑定同workspace MatchingReport；补齐manual/system两条正路径和NULL/selection/policy/fill/anchor负例；
- 五类generic request每条都必须恰好拥有一个matching kind-specific typed identity projection，所有ID+SHA输入使用复合FK；OutlineCandidate必须复合引用同一OutlineGenerate request/base tuple；
- request frozen字段和typed projection不可update/delete/truncate，status只允许`pending→terminal`一次；candidate frozen字段不可变，decision只允许`proposed→accepted|rejected|obsolete`一次，禁止reopen或terminal切换；
- 为五类request和两类candidate增加coherent、kind mismatch、cross-input、missing projection、payload mutation、delete/truncate和terminal-reopen live负例。上述第十一轮缺陷已通过typed projection、复合FK、deferred exactly-one verifier、append-only/transition trigger及clean live负例修复；Phase 0重新勾选，Phase 1仍须等待独立review PASS。

第十一轮修复冻结以下实现规则：generic request只允许`pending→succeeded|failed|obsolete`一次；Candidate只允许`proposed→accepted|rejected|obsolete`一次，部分接受继续使用独立decision receipt/operation occurrence。Agent schema中原有SHA-only的prompt/model字段补为ID+SHA，并显式新增template/agent ID+SHA；system selection新增matching policy ID+SHA。所有合同指向单一`bid_authoring_contract_artifacts` closed kind，未引入handler或Phase 1行为。

第十二轮独立review中typed request专项PASS，但仍有两条P1：

- Request和Candidate的initial INSERT必须分别强制`pending`和`proposed + decided_at NULL`，不能直接插入terminal状态绕过transition trigger；增加direct-terminal insert负例；
- SubmissionExport request的mode options必须与RenderSnapshot同样closed：exact keys、布尔类型、非draft watermark为NULL，且submission两项include flags必须为false；增加extra key、wrong type、submission true负例。上述两项已用稳定`23514` initial-state guard、closed mode-options CHECK、两条正路径和targeted clean-live负例修复并通过完整验收；Phase 1仍等待独立review PASS。

第十三轮专项review PASS；综合review仅发现EvidenceBundle text_quote的`quote_sha256`未与`quote_utf8`实际SHA-256绑定。该digest已加入SQL发布校验，真实text-quote正例和fresh wrong-digest负例通过clean-container live验收，最终独立review确认实现正确；Phase 0关闭并立即进入Phase 1，不再扩展Phase 0设计范围。

### Phase 1：四类Tender Source输入与RequirementSet

- [x] 扩展upload magic/container validator；
- [x] 扩展DocReader proto/ReadResult返回`StructuredSourceUnit`；DocReader只拥有图片/表格的结构来源元数据（XLSX sheet/cell/merge identity、图片原始引用/region），现有`ImageParser`不承担OCR内容生成；
- [x] 拆分`tender.rs`，复用`outline_and_route`；`TenderDocumentProcess`对独立图片强制使用受检builtin parser路径而非simple byte passthrough，并由Rust侧复用现有enrichment OCR/VLM路径，冻结ObjectRegistry/source identities，发布带revision身份的section/table_row/form_region/attachment_region/image_ocr_region SourceUnit，SourceSpanV2仅作locator；
- [ ] 实现用户确认/修改document role、typed relation和显式DocumentSetRevision freeze；
- [ ] 实现每SourceUnitRevision恰好一Disposition、RequirementSourceRevision、RequirementSet、局部supersession和唯一WorkspaceRequirementProjection；
- [x] `TenderDocumentProcess`只发布单document SourceUnit，不创建或enqueue RequirementSetCompile；
- [ ] 成功冻结DocumentSet时发布初始DispositionSet并enqueue以project+DocumentSetRevision+DispositionSetRevision唯一的`RequirementSetCompile`，后续DispositionSet publication同样enqueue；RequirementSet使用单调输入栅栏确定性发布，不持久化continuation状态。

当前实施顺序固定为：先完成role/relation确认与DocumentSet冻结，再提供生成大纲所需的最小RequirementProjection；随后立即进入大纲生成、知识库填充和导出纵向流程。全面合同扩展和治理验收不得插队。

验收：PDF、DOCX、含多sheet/合并单元格的XLSX、PNG/JPEG/WebP fixture都可上传；role/relation可确认；用户可冻结文件集；来源能回到文件/page/sheet/cell/attachment region，独立或扫描图片OCR发布`image_ocr_region`；每个SourceUnit恰好一个disposition；一份amendment只替代指定Requirement。

#### Phase 1 review缺陷记录（修复后继续实现）

Task 1A独立review发现四条P1：upload validator不能只依赖PDF/OOXML marker，必须结构解析可打开的PDF以及OOXML content-type/root relationship/main-part；DOCX单位必须按body原始顺序维护heading栈归属；DOCX drawing及PDF embedded/vector figure必须发布带typed document/page bounds的image region；Rust边界必须拒绝kind/locator不兼容、坏range、重复key和非连续ordinal，并保证merged range排序确定。上述约束已按记录完成实现；DocReader只提供结构元数据、Rust后续拥有OCR/VLM发布的既定所有权不变。

Task 1A复审补充两项结构合同修正后再实现：复合文档中的图片locator必须冻结封闭的typed parent identity，明确属于paragraph、table cell或form occurrence，并拒绝无关ordinal组合；XLSX结构提取必须按materialized cell稀疏遍历，为逻辑行列、materialized cell、merge/table范围和总unit/text/cell payload设置硬预算，禁止按`max_row × max_column`展开矩形。两项合同修正及DOCX顺序/merge containment/API fixture等实现缺陷均已修复，六格式validator、DocReader全量、Rust边界、API contract、proto/Ruff、cargo check/fmt与diff验收通过，Task 1A复审PASS。

Task 1B实现前发现两项合同缺口：V1 `SourceSpanV2`只有Markdown section/UTF-8 offset形状，不能无损承载V2 page/sheet/cell/table/form/attachment/image typed locator，且Phase 7前不能破坏active V1序列化，因此新增inactive closed `SourceUnitSpanV2`作为V2 SourceUnit locator（仍非artifact identity），Phase 7删除V1类型；EvidenceBundle text_quote原校验只检查UUID格式、未绑定真实knowledge document/chunk/ProductVersion，可能让Tender UUID伪装成投标方证据，必须增加同一knowledge三元组存在性校验。Tender图片原件与OCR文本分别冻结ObjectRegistry owner identity，SourceUnit canonical payload固定`source_purpose=tender_requirements_and_structure_only`，不得进入EvidenceBundle union。

### Phase 2：Workspace核心与无AI编辑竖切

- [ ] 创建唯一workspace、node/block/binding lineage与revision、WorkspaceRevision/head、DocumentSettingsRevision和SubmissionFulfillmentEvidenceRevision；binding occurrence纳入WorkspaceRevision，bind/remap/unbind与树/block/settings共用WorkspaceHead CAS且没有独立current pointer；
- [ ] 实现`ContentBlockV1`封闭serde/schema与Tiptap adapter；
- [ ] 实现`/api/v2` project/tender/workspace/mutation/assets API；
- [ ] Web 按 [`frontend-authoring.md`](frontend-authoring.md) 落地 `AuthoringShell`、`OutlineTree`、`DocumentCanvas`（聚焦章 Tiptap，其余静态）；先支持用户从空树人工编制、拖拽调序、插表格/图片并保存。Document Settings 面板不在黄金路径竖切。
- [ ] 实现409 conflict保留draft；相同Idempotency-Key重放首次request/receipt，若业务状态仍pending则再次尝试Oxana enqueue。

验收：两个并发Workspace编辑者不能互相覆盖；DocumentSet/DispositionSet/RequirementSet并发只冲突各自聚合；rename/move/split/merge/delete保持lineage；bind/remap/unbind保留历史并正确使evidence stale；页面刷新后树、blocks和人工资产一致。

### Phase 3：动态OutlineCompiler

- [ ] 实现OutlineGenerate request snapshot、agent schema、bounded verifier和一次repair；
- [ ] 按招标明确组成、表格/表单结构、资格/技术/商务/评分要求编译动态树；
- [ ] 实现 OutlineCandidate overlay（默认全选、可取消节点）、部分接受、CAS；checkpoint 若生成内容需要则在「填充」时静默建立，界面不得出现「确认后才能改」。
- [ ] 删除任何固定标题、part key或默认六部分假设。

验收：至少四类golden tender产生不同树形；用户可删除强制节点并仍确认；新文件发布后旧candidate变obsolete、已接受人工树只变stale。

### Phase 4：知识检索V3、证据选择与图片冻结

- [ ] 在唯一KnowledgeRetrievalPort V3响应中加入可选media identity并保持V2检索排序/scope attestation语义；
- [ ] matching从route/part目标改为Requirement/OutlineNode目标；显式“匹配资料”调用`ContentGenerate(operation=match_only)`，不定义独立EvidenceMatch job；
- [ ] 实现EvidenceBundle、NO_EVIDENCE、EvidenceAssetArtifact和ObjectRegistry持有；
- [ ] 实现人工PickSet与system-proposed selection；
- [ ] Web右侧Evidence面板支持预览quote/图片、人工改选、单独“匹配资料”和整份文档覆盖概览，不新增强制匹配步骤。

验收：技术截图、资质证书和案例图片可由OCR命中并冻结真实图片；删除/替换live知识文档不改变既有candidate；招标输入图片不会混入EvidenceBundle。

### Phase 5：ContentGenerate与Candidate review

- [ ] 实现node/subtree/workspace三种用户触发范围、`empty_only|append_candidate|missing_requirements_only`和InsertionAnchor；验证后台不会自动创建生成request；
- [ ] `ContentGenerate(operation=generate)`的system-proposed模式在同一job内检索并冻结证据；人工模式消费指定PickSet，不建立matching continuation；
- [ ] 实现narrative、response table、structured form、evidence index、quote和deterministic generators；
- [ ] 校验事实`evidence_ref`、图片identity、表格网格和prompt injection边界；
- [ ] CandidateReview显示文本/表格操作和图片，允许部分接受。

验收：无证据时不生成投标方事实；匹配图片可直接进入candidate；业务事实有冻结引用，连接语言允许无引用；接受candidate不会覆盖接受后发生的人工编辑。

### Phase 6：Assessment、preview、DOCX/PDF

- [ ] 实现OutlineAssessment与SubmissionAssessment确定性编译，以workspace/projection/scope/settings/asset/QuoteSnapshot组合hash复用结果；不创建Assessment queue job；
- [ ] 实现WorkspaceRevision引用的DocumentSettingsRevision、共享LayoutDocument、系统RenderStyleContract和HTML全文预览；
- [ ] 将现有renderer改为遍历node/block occurrence，复用DOCX/PDF图片、CJK字体和PDF附件准备能力；
- [ ] 在单个SubmissionExport job内实现Assessment → AttachmentPreparation → preparation verify → RenderSnapshot → Manifest → render；冻结输出mode/options；
- [ ] 输出独立assessment report。

验收：有高风险业务提示仍返回202并完成DOCX/PDF；非法ContentBlock、资产缺失、digest错误和renderer错误稳定技术失败；同一manifest重放语义一致。

### Phase 7：删除旧实现并fresh上线

- [ ] 在同一fresh cutover中用已验证的V2 first-launch manifest和queue registry替换active配置，并删除V1 baseline、`required_part_keys`、`technical_part_key`、`template_slot_for_part_key`、Markdown part更新/重生成、SubmissionGateV1及旧SQL表/函数；
- [ ] 删除`/api/v1/bids/.../parts`、`gate-issues`和所有旧submission endpoints；
- [ ] 删除PartsPane、`partTitle`、`BidStep=parts`及旧route/query参数；
- [ ] 只把现有QuoteSnapshot作为专用业务快照接入；删除旧company-profile、submission-profile和procedural专用API/表，不保留双写或compat façade；公司证据使用KnowledgeRetrievalPort/人工资产，流程要求使用通用Requirement/FulfillmentBinding/structured content；
- [ ] 新增`scripts/bidding_v2_deletion_scan.sh`并更新fresh schema/runtime部署说明；扫描任何非`project_wide` WorkspaceScope、`project_fact|submission_procedure`、独立`EvidenceMatch`/旧细粒度job名、binding current pointer以及自建scheduler/lease/retry/fan-out/fan-in/continuation状态。

验收：fresh E2E是唯一上线通路；仓库扫描只允许旧名称出现在删除矩阵/历史说明中，生产源码、SQL、API和Web bundle零命中。

## Verification

### 自动化测试矩阵

- Rust unit：media sniffing、`section|table_row|form_region|attachment_region|image_ocr_region` canonicalization与一Disposition约束、SourceSpanV2、supersession、FulfillmentExpr、树/binding验证、ContentBlock、evidence_ref、candidate状态机、LayoutDocument；
- SQL/live：唯一project-wide workspace、artifact不可变、各聚合CAS、binding随WorkspaceRevision且无独立current、binding/evidence revision与stale、幂等receipt、确定性stage identity、candidate terminal state、ObjectRegistry引用、assessment composite identity、historical settings resolution、manifest no-live-read；
- API：owner隔离、四类upload、role/relation/DocumentSet freeze、调用方伪造identity拒绝、binding使用Workspace CAS、聚合级409、415/422/503合同；同key POST在pending enqueue不确定时重新enqueue且不新建request；业务warning不阻断、下载范围隔离；
- Worker：五类粗粒度job映射、RequirementSetCompile的project+DocumentSet+DispositionSet唯一键、DocumentSet/Disposition publication enqueue owner及单调输入publication fence、Oxana unique/retry/backoff复用、确定性stage identity/首次receipt重放、重复或stale delivery no-op，且无自建scheduler/lease/fan-out/continuation状态；
- Agent fixture：中文多级编号、表格/扫描表单、局部澄清、prompt injection、超深树、未知字段、伪造asset、无证据hallucination；
- Renderer golden：动态标题、富文本、复杂表格、知识图片、人工图片、PDF附件、目录、CJK字体；比较HTML/DOCX/PDF的同一settings identity与语义；验证review draft水印及干净submission；
- Browser E2E：多文件上传→确认role/relation→冻结DocumentSet→要求台账→分别触发本章/子树/全部生成→missing requirements/光标插入→系统建议证据→含文字/图片Candidate→人工表格/图片→并发冲突→有提示导出；断言后台不自动生成、证据只出现在Web/报告。

### 必跑命令

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm --prefix web run lint
npm --prefix web run build
npm --prefix web run test:e2e
scripts/fresh_schema_acceptance.sh
scripts/bidding_v2_deletion_scan.sh
```

需要真实依赖的live套件额外运行PostgreSQL、Redis/Oxana、Object storage、DocReader/OCR和配置的chat/VLM provider；未配置时测试必须明确skip，不能伪装通过。

## 已确认产品选择

- 输入格式：PDF、DOCX、XLSX、PNG、JPEG、WebP；图片执行OCR和要求提取；
- 输入/输出边界：招标文件只驱动要求、大纲和结构；投标正文/图片来自知识库匹配、人工输入/插入及冻结QuoteSnapshot；
- UI：Word 式左树 / 中连续画布 / 右证据提示；Tiptap WYSIWYG；不是 Word 桌面应用，也不是 Markdown 大纲；
- 生成：单章节、子树和整份均支持，全部由用户触发；
- XLSX：只解析内容和表格结构，不实现Excel编辑，不作为输出模板；
- 输出样式：不使用用户模板；用户可修改受控全局DocumentSettings，系统`RenderStyleContractV1`据此把结构化树和ContentBlock生成DOCX/PDF；
- 临时材料：只保留在当前Workspace，由用户直接插入，不写长期知识库、不自动匹配；
- 范围：每个项目恰好一个project-wide Workspace、一份投标文件；
- 图片候选：知识库匹配图片可由Agent直接放入ContentCandidate，用户在diff中确认；
- 文本有据性：业务事实必须回指知识证据，连接和组织语言可以生成；
- 证据选择：支持人工先选与系统建议两种模式，默认系统提出、用户确认；
- 证据展示：知识来源仅在Web、审计链和独立Assessment报告中展示，不写入最终投标正文；
- 匹配入口：集成于章节右侧Evidence面板，并提供整份文档概览，不作为强制独立步骤；
- 文档设置：提供A4、页边距、中西文字体、字号、行距、标题编号、页眉页脚和页码的受控全局设置；
- 部分输入：允许基于成功解析的当前DocumentSet生成；pending/failed/unresolved文件只提示，后续恢复会使旧candidate/checkpoint stale。
