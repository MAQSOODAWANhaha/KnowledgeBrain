# 招标文件驱动的投标文件编制工作区

| 项 | 值 |
| --- | --- |
| 状态 | **已确认目标契约，尚不代表代码已实现** |
| 版本 | Target V2 |
| 部署 | clean-slate fresh redeploy，不兼容固定 PartSet、旧 schema 或旧 API |
| 核心原则 | 用户拥有最终编辑与导出决定权；系统只生成建议、提示风险并保存可追踪证据 |

本文是“同一招标项目多文件上传 → 动态投标大纲 → 人工编辑与 AI 辅助填充 → DOCX/PDF”的权威产品与领域定义。实现顺序、migration、API、测试和删除矩阵后续写入 `plans/bidding/`，不得在计划文档中重新定义本文规则。

## 1. 最终目标

系统接受同一个招标项目的一份或多份 PDF、DOCX、XLSX、PNG、JPEG 或 WebP 文件，包括主招标文件、技术或商务附件、报价清单、合同、投标文件格式、澄清、修改和补遗文件，并完成：

1. 独立解析每份文件，保留文件、页码、章节、表格行、单元格、图片OCR和表单区域来源；
2. 建立带版本、适用范围和局部替代关系的要求台账；
3. 根据招标文件明确组成、表格/表单结构、资格条件、技术商务要求和评分因素编译投标文件树形大纲；
4. 允许用户在任何阶段新增、删除、改名、移动、拆分和合并大纲节点；
5. 允许用户编辑文字、表格、结构化表单、图片、附件、分页和签章占位；
6. AI 只生成大纲或内容候选，不直接覆盖人工内容；
7. 系统持续提示遗漏、偏离、缺件、低置信度、评分损失和过期内容，但不替用户作最终决定；
8. 用户可以在存在提示时继续确认、编辑和导出；
9. 最终 DOCX/PDF 从不可变工作区快照渲染，并可回溯到招标文件、要求、大纲、内容和资产版本。

输入和输出严格分域：Tender Source只用于提取要求、生成大纲、构造响应表中的“招标要求”内容和结构；投标方正文、事实与图片只能来自知识库冻结证据、用户在当前Workspace人工输入/插入的资产、冻结`QuoteSnapshot`，以及基于这些来源生成的结构化内容。招标方文件中的图片或附件不得被Agent自动当作投标方证据插入输出。

系统不读取用户DOCX模板，也不把XLSX当作输出模板或浏览器编辑对象。Renderer使用系统拥有的版本化`RenderStyleContract`，并允许用户通过受控`DocumentSettingsRevision`设置A4页边距、中西文字体、正文字号、行距、标题编号、页眉页脚和页码；根据大纲树、ContentBlock和这些全局设置生成DOCX/PDF。

系统是辅助编制和风险提示工具，不是招标合规审批人，也不声称生成结果必然满足法律、评审或中标条件。

## 2. 最高产品约束：用户主导

### 2.1 永久允许的人工操作

任何业务阶段都允许用户：

- 修改、删除或增加大纲节点；
- 调整章节层级与顺序；
- 拆分或合并章节；
- 修改要求分类、适用范围和章节映射；
- 修改 AI 生成的任何文字、表格或表单字段；
- 插入、移动、替换或删除图片与附件；
- 忽略系统提示并继续确认或导出。

系统不得因为资格缺失、强制要求未覆盖、评分材料不足、内容过期或固定格式偏离而禁止用户继续业务操作。

### 2.2 Assessment，不使用业务阻断 Gate

大纲与提交检查统一使用 Assessment：

```text
ready
has_warnings
has_critical_warnings
```

不使用会阻止业务操作的 `PASS/BLOCK` 语义。

- `OutlineAssessmentSnapshot` 描述大纲覆盖、来源、顺序、结构化表格/表单和未解决要求；
- `SubmissionAssessmentSnapshot` 描述正文、表格、附件、报价、评分、偏离、stale 和格式问题；
- 用户可以在任意 Assessment 状态下确认大纲或导出文件；
- “用户选择继续”不得把未满足要求改写为 `covered` 或把 Assessment 改写为 `ready`。

### 2.3 技术失败仍然 fail-closed

用户主导不等于允许系统生成损坏或不可重放的文件。以下属于技术失败，必须停止对应 mutation 或 render：

- revision/CAS 冲突可能覆盖其他人工修改；
- ContentBlock 不符合冻结 Schema；
- 表格网格或合并单元格结构非法；
- 图片、附件、字体或RenderStyle资产不存在或 digest 不匹配；
- PDF 附件页面准备不完整；
- renderer 失败或输出文件结构无效；
- 数据库事务、对象提交或不可变快照发布失败。

业务风险只提示；无法正确执行的技术错误必须明确失败。

## 3. 范围与聚合

### 3.1 同一项目的多份文件

所有输入文件都属于当前 `BidProject`，不做跨项目识别、匹配或合并：

```text
BidProject
└── TenderDocumentSet
    ├── primary_tender
    ├── bid_format
    ├── technical_specification
    ├── commercial_requirement
    ├── bill_of_quantities
    ├── contract
    ├── drawing
    ├── clarification
    ├── amendment
    └── other_attachment
```

每份文件独立转换和解析。禁止把多份 Markdown 简单拼接后丢失来源身份。

### 3.2 一个项目一个 Workspace、一份输出

V1每个`BidProject`在创建时同时建立唯一的project-wide `SubmissionWorkspace`，只生成一份投标文件。数据库以唯一约束保证一个project不能出现第二个Workspace。

Workspace拥有：

- 项目级要求projection；
- 冻结`QuoteSnapshot`引用；
- 大纲与内容；
- 知识库证据和用户人工资产placement；
- Assessment；
- render snapshot与输出。

每个`BidProject`恰好拥有一个`SubmissionWorkspace`并生成一份投标文件；Workspace scope固定为`project_wide`。

## 4. 不可变身份链

一份输出必须能够回溯以下身份：

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
→ DOCX/PDF
```

每一层使用版本化canonical schema和SHA-256；历史不可覆盖，current pointer只能原子切换。CAS按聚合边界执行：WorkspaceHead保护树、ContentBlock、DocumentSettings、OutlineFulfillmentBinding和Candidate decision；DocumentSet、SourceUnitDispositionSet、RequirementSet、Supersession与WorkspaceRequirementProjection各自校验自己的`expected_artifact_id/sha256`。创建project/document/workspace只要求幂等identity，不伪造`expected_workspace_head`。

## 5. 招标文件集合

### 5.1 TenderDocument

```text
TenderDocument
  id, project_id
  file_name, media_type, byte_length
  document_role
  original_object_ref, original_sha256
  conversion_generation, parse_status
  effective_at, uploaded_at
```

`document_role` 由系统建议、用户确认或修改。文件名不能成为优先级或覆盖关系的唯一依据。

### 5.2 文件关系

```text
TenderDocumentRelation
  source_document_id
  target_document_id
  relation = complements|clarifies|partially_amends|replaces|withdraws
  applicability
  confirmed_by, confirmed_at
```

文件关系用于解释上下文；要求级 supersession 才是局部覆盖的权威。

### 5.3 TenderDocumentSetRevision

用户可随时冻结当前文件集合，也可以在文件尚未完全解析时继续，系统只产生提示。快照至少冻结：

```text
project_id
included document identities and roles
document relations
conversion/source artifact identities
unparsed or failed document list
created_by, created_at
source_set_sha256
```

后续新增、替换或重试文件形成新 revision，不修改旧 revision。

## 6. SourceUnit 与抽取完整性

### 6.1 SourceUnit

每份冻结招标源被分解为有界、可回放且有独立revision身份的SourceUnit：

```text
SourceUnitRevision
  source_unit_lineage_id, source_unit_revision_id
  unit_kind = section|table_row|form_region|attachment_region|image_ocr_region
  frozen source artifact/document identities
  source_span_v2                 # 仅定位器
  structural locator and digest
```

Requirement和Disposition必须引用`source_unit_revision_id`；`SourceSpanV2`只负责section/page/offset、sheet/cell或区域定位，不是要求来源的业务身份。

### 6.2 SourceUnitDispositionSetRevision

每个选入DocumentSetRevision的`SourceUnitRevision`在同一个`SourceUnitDispositionSetRevision`中恰有一条disposition：

```text
requirement
non_requirement
unresolved
```

Verifier检查：

- 文件集中的每个`SourceUnitRevision`恰好出现一次；
- 每个requirement至少引用一个`SourceUnitRevision`；
- 不存在孤立requirement、未知revision或仅以SourceSpan充当来源的记录；
- unresolved 被保留并进入 Assessment，不因用户继续操作而消失。

未解决 SourceUnit 不阻止用户冻结 RequirementSet、确认大纲或导出，但必须持续显示为风险提示。

## 7. 原子要求与局部替代

### 7.1 AtomicRequirement

```text
AtomicRequirement
  logical_requirement_id
  requirement_revision_id
  project_requirement_set_revision_id
  obligation_text
  requiredness = mandatory|optional|informational
  compliance_policy = must_comply|explicit_response|deviation_allowed|scored
  applicability
  lifecycle = current|superseded|withdrawn|unresolved
  requirement_sha256
```

`requiredness` 和 `compliance_policy` 只影响提示等级和满足度计算，不赋予系统阻止用户操作的权力。

### 7.2 RequirementSource

一个要求可以引用多个`SourceUnitRevision`；同一`SourceUnitRevision`也可以支持多个经过人工确认的原子要求。`RequirementSourceRevision`必须冻结requirement revision、source unit revision、relation、actor和digest；不得直接把`SourceSpanV2`作为RequirementSource。

### 7.3 RequirementSupersessionEdge

```text
RequirementSupersessionEdge
  old_requirement_revision
  new_requirement_revision
  old_source_unit_refs[]
  new_source_unit_refs[]
  amendment_document_relation
  applicability
  actor, reason, created_at
```

不变量：

- old/new 属于同一项目和兼容的文件集 lineage；
- supersession 图无环；
- ProjectRequirementSet 保存带适用范围的历史；
- Workspace projection 按自己的 scope 重放 DAG；
- 对某个 applicability fragment，old 与 successor 的 effective membership 不得同时含糊生效。

用户可以修改、撤销或重新建立 supersession 决定；新决定形成新 revision，不覆盖历史。

## 8. 履约表达式与覆盖

### 8.1 FulfillmentNeed

支持渠道：

```text
narrative_content
response_table
deviation_statement
structured_form
evidence_attachment
quotation
```

### 8.2 FulfillmentExprV1

```text
Need(need_occurrence_id, channel)
AllOf(non_empty children)
AnyOf(non_empty children)
AtLeast(min_count, children)
```

不变量：

- mandatory requirement 的表达式不得为空；
- `AllOf/AnyOf.children` 非空；
- `1 <= AtLeast.min_count <= children.length`；
- 同一表达式中的 `need_occurrence_id` 唯一；
- 同一 evidence occurrence 在一次求值中最多消费一次；
- 未知节点、字段、枚举或额外键拒绝。

表达式求值用于生成提示和覆盖状态，不用于阻止用户确认或导出。人工事实和流程响应使用普通`narrative_content`、`response_table`或`structured_form` block；只有`QuoteSnapshot`保留专用业务快照身份。

### 8.3 两层覆盖身份

`OutlineFulfillmentBindingRevision`把Need绑定到逻辑目标：

```text
binding_lineage_id, binding_revision_id
need_occurrence_id
workspace requirement projection revision
outline node lineage | response table | structured form | quote
state = bound|unbound|superseded
actor, reason, binding_sha256
```

`SubmissionFulfillmentEvidenceRevision`绑定实际输出身份：

```text
evidence_lineage_id, evidence_revision_id
binding_revision_id
workspace revision
target node/block/table-row or structured block/value revision
asset revision | QuoteSnapshot revision | assessment decision revision
state = current|stale|withdrawn
evidence_sha256
```

`OutlineFulfillmentBindingRevision`是WorkspaceRevision的一部分，没有独立current pointer。用户`bind`、`remap`或`unbind`时，必须在同一Workspace事务中校验`expected_workspace_head`、写binding revision和binding occurrence、创建新WorkspaceRevision并原子移动WorkspaceHead；每次操作保留历史。目标revision、RequirementProjection或依赖变化时SubmissionFulfillmentEvidence进入stale；逻辑binding可保留。Assessment只针对即将导出的WorkspaceRevision所引用的binding/evidence revisions重新求值。

## 9. Workspace scope 与要求 projection

`WorkspaceScopeRevision`固定冻结项目级范围：

```text
workspace_id
scope_kind = project_wide
applicable source regions
shared requirement rules
explicit requirement assignments
actor, created_at, scope_sha256
```

`WorkspaceRequirementProjectionRevision` 从 ProjectRequirementSet 选择当前 Workspace 的有效要求。未分配、多义或冲突要求进入 Assessment；用户可以继续并人工修改 assignment。

所有quote、coverage、Assessment、snapshot和manifest都携带唯一workspace composite identity，禁止脱离该项目Workspace读取或发布。

## 10. OutlineCompiler

OutlineCompiler 只生成候选，不直接修改 WorkspaceHead：

```text
compile_outline(
  document_set_revision,
  workspace_requirement_projection,
  workspace_scope
) -> OutlineCandidate
```

编译顺序：

1. 识别招标文件明确规定的投标文件组成、分册和顺序；
2. 将招标输入中投标函、授权书、报价表、偏离表等格式要求解析为结构化表格/表单节点建议；不读取或填充用户DOCX/XLSX模板；
3. 绑定资格、技术、商务、评分和证明材料要求；
4. 为尚无承载位置的文档型履约 Need 建议节点；
5. 生成 OutlineFulfillmentBinding 建议；
6. 输出冲突、未映射、低置信度和来源提示。

禁止根据固定通用大纲无条件生成与本次招标无关的章节。

pending、failed或unresolved输入不阻止用户基于当前成功解析的DocumentSetRevision生成大纲；Candidate和Assessment必须明确冻结并展示未参与输入。文件后来恢复或新增时，旧Candidate/Checkpoint进入stale，但不删除已接受的人工树和正文。

## 11. 树与正文的统一工作区

### 11.1 WorkspaceRevision

```text
WorkspaceRevision
  workspace_revision_id
  parent_revision_id, parent_sha256
  outline checkpoint/base identity
  document_settings_revision identity
  ordered NodeOccurrence[]
  ordered OutlineFulfillmentBindingOccurrence[]
  created_by, created_at
  workspace_sha256
```

所有树、块、binding和全局文档设置mutation在同一事务中：

1. 校验`expected_workspace_head`；
2. 写不可变node/block/binding revisions；
3. 写新WorkspaceRevision及其node/block/binding occurrences；
4. 原子移动WorkspaceHead；
5. 写audit和幂等receipt。

CAS冲突不得自动覆盖或丢弃人工编辑。

### 11.2 OutlineNode

```text
OutlineNode
  lineage_id
  node_revision_id
  title
  node_kind
  semantic_role
  render_role
  origin
  tombstone
```

标题可以任意修改，但结构化表单、报价、目录和渲染行为只能读取 `semantic_role/render_role`，不得解析标题或旧 part key。

### 11.3 身份规则

- rename/move/reorder：保留 lineage，创建新 node revision；
- split：创建多个新 lineage 和一对多 lineage edge；
- merge：创建新 lineage 和多对一 lineage edge；
- delete：写 tombstone，不物理删除历史内容；
- 未迁移的 requirement binding 进入 unresolved 提示。

## 12. ContentBlockV1

### 12.1 Node 与 Block placement

WorkspaceRevision中的节点 occurrence 冻结：

```text
NodeOccurrence
  node_revision_id
  parent occurrence
  ordinal, depth
  BlockOccurrence[]
```

```text
BlockOccurrence
  block_revision_id
  ordinal
```

每个 BlockOccurrence 在一个 WorkspaceRevision 中唯一属于一个节点。

### 12.2 RichText

允许 block/inline节点：

```text
paragraph
bullet_list
ordered_list
list_item
hard_break
text
```

允许 mark：

```text
bold
italic
underline
strike
code
link
evidence_ref(evidence_bundle_id,evidence_item_id,quote_range)
```

`evidence_ref`是不可见的事实来源标记，不改变DOCX/PDF视觉样式。Agent生成的事实性span必须携带该标记；用户编辑被标记文本时保留人工内容但将对应引用置为stale，由Assessment提示重新核对。

正式章节标题只来自 OutlineNode；正文 heading 不进入 ContentBlockV1。

### 12.3 Table

```text
rows
cells(content,rowspan,colspan)
widths_mm
repeat_header_rows
```

不变量：

- rowspan/colspan为正整数且不越界；
- 逻辑网格每个坐标恰由一个cell覆盖；
- 合并单元格不得重叠；
- widths数量等于逻辑列数；
- 总宽不超过可打印宽度；
- repeat_header_rows为连续前缀；
- cell内容只使用允许的RichText结构。

### 12.4 Image

```text
asset_revision_id
width_mm
alignment = left|center|right
crop = normalized left/top/right/bottom
caption
alt
```

不变量：宽度为正且不超过可打印宽度；crop坐标在0..1内，左右和、上下和均小于1；默认保持宽高比。

### 12.5 其它块

```text
attachment_ref(asset/preparation revision, render_mode, start_new_page)
structured_form(form_definition_revision, field_values)
page_break
signature_placeholder(kind,width_mm,height_mm,label)
```

每种 block 使用封闭、版本化 JSON Schema；未知kind、字段、enum、rich-text节点或mark全部拒绝。

### 12.6 DocumentSettingsRevision

```text
DocumentSettingsRevision
  page_size = A4
  margins_mm(top,right,bottom,left)
  cjk_font, latin_font
  body_font_pt, line_spacing
  heading_numbering = decimal|chinese|none
  header, footer
  page_number = none|footer_center|footer_outside
  settings_sha256
```

字段和值域由RenderStyleContract封闭约束。用户可以修改这些全局设置，但不能上传DOCX模板、注入任意样式代码或使用逐字符自由字号。设置修改创建新的DocumentSettingsRevision，并由同一事务产生的新WorkspaceRevision引用；历史preview/export只能沿所选WorkspaceRevision解析该settings revision，禁止读取live settings pointer。

## 13. 用户编辑与 AI Candidate

用户可以直接编辑任何非技术损坏状态的树或内容。AI只创建不可变 Candidate：

```text
OutlineCandidate
ContentCandidate
  base_workspace_revision/head_sha256
  target node/block revisions
  requirement/dependency identities
  proposed operations
  state = proposed|accepted|rejected|obsolete
```

状态机只允许：

```text
proposed -> accepted|rejected|obsolete
```

终态不可改写。接受操作整体CAS WorkspaceHead；head不匹配时只能在 `state=proposed` 条件下转 obsolete。已accepted的重复请求返回首次receipt，不得改写为obsolete。

支持逐块接受、部分接受、填充空章节和仅补充缺失要求。光标插入必须冻结`InsertionAnchor(node_revision_id, block_revision_id?, utf8_offset?)`；仅补充缺失要求使用`fill_policy=missing_requirements_only`。不存在“AI直接替换当前整章”的写入接口。

### 13.1 证据选择、事实引用与图片候选

内容生成的`EvidenceSelectionInput`支持两种形式：

```text
EvidenceSelectionInput
  = EvidencePickSetArtifact       # 用户先选择
  | ProposedEvidenceSetArtifact   # 系统根据MatchingReport提出
```

默认流程由系统提出证据集合并生成ContentCandidate，用户在同一review中确认；用户也可以先人工选择证据再触发生成。无论哪种方式，只有用户接受Candidate后，选择才成为正式采用的冻结evidence selection。

Agent匹配到知识库图片时，可以直接把带知识来源和稳定资产身份的ImageBlock放入ContentCandidate。用户可以接受、删除、移动或调整图片。不得只依据`image_ocr`文本反查live知识库图片；知识检索端口必须返回可冻结的图片media identity，招投标立即冻结自己的EvidenceAssetArtifact和ObjectRegistry引用。

Agent生成的业务事实必须使用`evidence_ref`回指本次EvidenceBundle；服务端拒绝引用bundle外identity。连接、章节过渡、组织语言和不引入新业务事实的总结可以无引用生成。用户人工输入的内容始终允许保留，缺少证据只产生Assessment提示，不阻止确认或导出。

## 14. Stale 与人工核对

普通人工保存：

```text
content revision +1
保留原 dependency identity
保留 stale 状态
```

只有以下操作才能绑定当前 requirement/dependency identity：

- 接受基于当前输入生成的候选；
- 用户显式执行“已根据当前要求核对”；
- 受检的确定性表单或响应表重生成。

用户可以忽略 stale 提示并继续导出，但系统不得静默把旧内容标记为已核对。

## 15. 输出Asset、PDF附件与招标表单结构

### 15.1 WorkspaceAsset

```text
WorkspaceAsset
  asset_revision_id
  workspace_id
  object_ref, sha256
  media_type, dimensions, page_count
  source, validation_status
```

内容块引用稳定asset revision，不存临时URL或内联大对象。输出WorkspaceAsset的来源只能是知识库EvidenceAssetArtifact或用户人工上传；Tender Source图片/附件不得被Agent自动登记为输出资产。

### 15.2 AttachmentPreparationRevision

所有 `embedded_pages` PDF附件必须冻结：

```text
source_asset_revision
ordered page asset revisions/digests
page geometry
status
preparation_sha256
```

Renderer不得运行时重新转换PDF。页面不完整属于技术失败；用户可以改用 `file_reference` 或替换附件。

### 15.3 TenderStructuredFormDefinition

招标输入中的DOCX/XLSX表格或扫描表单只被解析为结构定义：

```text
TenderStructuredFormDefinition
  source document/unit identities
  title and instruction text
  ordered columns/rows/fields
  merged-cell and required-field constraints
  definition_sha256
```

内容生成器根据definition创建普通可编辑`table`或`structured_form` ContentBlock。用户可以修改其结构和值；系统只提示与招标输入结构的差异。不存在下载原模板填写、上传完成模板或运行时合并DOCX/XLSX模板的流程。

## 16. 大纲确认与 Assessment

系统生成大纲后，用户修改并点击确认，创建：

```text
OutlineCheckpoint
OutlineAssessmentSnapshot
  workspace_revision_id
  workspace_requirement_projection_revision_id
  workspace_scope_revision_id
  document_settings_revision_id
  frozen asset/QuoteSnapshot identities
  assessment_input_sha256
```

同一个`assessment_input_sha256`可以复用同一结果；只按workspace revision去重不成立。

Assessment至少包含：

- 未映射或部分映射要求；
- unresolved SourceUnit；
- 招标要求的结构化表格/表单缺失或结构偏离；
- 招标指定顺序差异；
- 评分材料缺失；
- 用户忽略的高风险项。

存在任何提示都允许确认。确认是用户意图检查点，不是审批锁。

确认后仍可继续编辑；树变化形成新WorkspaceRevision和新的待确认状态。用户可以继续人工编辑内容，并可选择用当前草稿重新确认后生成候选。

## 17. 提交 Assessment 与导出

### 17.1 SubmissionAssessmentSnapshot

导出时针对明确选择的WorkspaceRevision及其冻结依赖重新计算：

```text
SubmissionAssessmentSnapshot
  workspace revision
  requirement projection and scope revisions
  DocumentSettingsRevision
  frozen asset and QuoteSnapshot identities
  assessment_input_sha256
  requirement fulfillment status
  unresolved and ignored requirements
  missing/invalid/stale content
  structured table/form completeness and deviation
  quote and scoring gaps
  asset/preparation status
  layout and rendering warnings
```

Assessment只提示，不阻止业务导出。

### 17.2 输出模式

```text
preview
review_draft
submission
```

- preview：在线预览，不发布可下载正式文件；
- review_draft：可下载，允许受控水印并配套独立问题报告；
- submission：干净的正式输出，不写入警告、水印、风险声明或知识来源。

模式和选项必须冻结在RenderDocumentSnapshot与Manifest中。三种模式都允许存在业务提示；只有技术上无法正确渲染时失败。

### 17.3 独立检查报告

系统可以生成：

```text
投标文件.docx / 投标文件.pdf
投标文件检查报告.pdf / json
```

检查报告不自动嵌入submission正文，由用户决定是否下载、保存或分享。知识库文件名、quote和`evidence_ref`只在Web证据面板、审计链和检查报告中展示，不写入最终DOCX/PDF正文或脚注。

导出技术顺序固定为：

```text
SubmissionAssessmentSnapshot
→ AttachmentPreparationRevision（如有）
→ 验证全部preparation ready且digest完整
→ RenderDocumentSnapshotV2
→ SubmissionManifestV2
→ DOCX/PDF render
```

不得在附件准备成功前发布RenderDocumentSnapshot或Manifest。

## 18. RenderDocumentSnapshotV2

```text
RenderDocumentSnapshotV2
  mode = preview|review_draft|submission
  mode options and watermark identity
  workspace revision/checkpoint identities
  document settings revision identity
  ordered node and block occurrences
  asset/form/preparation occurrences
  content_block_schema_version
  render_operation_contract_version
  docx_renderer_contract_id
  pdf_renderer_contract_id
  style_contract_id
  page_geometry
  font artifact identities
  numbering and TOC policy
  snapshot_sha256
```

Renderer按树的前序遍历输出标题和内容块。每个block kind有明确DOCX/PDF operation；不得再以标题、`part_key`或附件kind推导位置。

Schedule与publish都复核snapshot、renderer、style、font和asset identities。同一snapshot不因后续renderer或字体升级改变语义。

## 19. SubmissionManifestV2

```text
SubmissionManifestV2
  format = docx|pdf
  mode = review_draft|submission
  frozen mode options and watermark identity
  project/workspace identities
  document-set and requirement projection identities
  outline checkpoint, workspace revision and document settings revision
  submission assessment snapshot
  render document snapshot
  renderer/style/font identities
  manifest_sha256
```

Manifest只读取冻结快照，不读取live树、正文、报价、要求、settings或资产。历史导出从选定WorkspaceRevision解析DocumentSettingsRevision。Assessment状态不限制manifest创建；附件preparation未ready、技术身份不完整或不一致必须失败。submission模式必须验证watermark为空且不会渲染风险/知识来源。

## 20. Clean-slate替换

Target V2不兼容旧固定PartSet。最终实现必须删除：

- `1、2:*、3、4、5、6:*` RequiredPartSet；
- `part_key -> template_slot`业务身份；
- 按标题或固定part判断报价、授权书、附件和渲染逻辑；
- 旧part update/regenerate API；
- 旧SubmissionGateV1业务阻断语义；
- 旧company-profile、submission-profile和procedural专用API/表，以及旧人工事实/流程专用fulfillment channel；公司证据改由知识检索或人工资产提供，人工事实和流程响应使用通用narrative/table/structured_form内容；
- OutlineFulfillmentBinding独立current pointer或绕过WorkspaceHead的bind/remap/unbind路径；
- legacy schema、alias、兼容façade、双写、旧格式读取和历史数据导入。

只保留现有`QuoteSnapshot`作为Target V2的专用业务快照输入，不迁移其它旧profile/procedural聚合。

不实现 `legacy|outline_v2` 双模式，不处理旧binary共存或在途legacy任务。部署使用fresh baseline。

现有SourceSpan、不可变artifact、ObjectRegistry、报价快照、匹配报告、CAS、幂等和manifest-only render思想可以复用，但必须通过Target V2的新interface接入。

## 21. 最低验收场景

1. 同一项目上传主文件、技术附件、报价清单和格式附件，生成一棵可追踪大纲；
2. 澄清文件只替代原要求中的局部内容；
3. unresolved抽取持续提示，但用户仍可确认大纲和导出；
4. 用户确认大纲后继续新增、删除、改名、移动、拆分和合并；
5. 删除强制要求节点后显示高风险提示，用户仍可继续；
6. 任意节点插入图片、复杂表格和PDF附件，DOCX/PDF位置与Workspace一致；
7. 招标表格/表单被生成成可编辑结构，人工改变结构时显示提示但不阻止导出；
8. AI候选不会覆盖并发或后续人工编辑；
9. 普通文字修改不会错误清除stale；
10. 新增招标附件后只使相关binding/evidence提示过期；
11. 有业务提示时仍可生成干净submission文件和独立检查报告；
12. 资产丢失、Schema非法或renderer失败时稳定技术失败；
13. 同一RenderDocumentSnapshot可按冻结contract重放；
14. 最终导出中不会重新出现被用户删除的固定旧part；
15. Manifest可回溯完整身份链和当时的Assessment；
16. 调整全局页边距、字体、行距、标题编号、页眉页脚或页码后，Preview与DOCX/PDF使用同一冻结设置；
17. 业务事实的知识来源可在Web和检查报告追踪，但不会出现在最终投标正文或脚注；
18. “生成本章”“生成当前子树”“生成全部空章节”都只能由用户触发，后台不会自动生成；
19. 光标插入使用冻结InsertionAnchor，`missing_requirements_only`只为尚未覆盖的Need建议内容；
20. 同一WorkspaceRevision在HTML preview、DOCX和PDF中使用同一DocumentSettingsRevision；
21. review_draft可带受控水印，submission永远不含水印、风险提示或知识来源；
22. 独立或扫描图片OCR形成`image_ocr_region` SourceUnit，并与section/table_row/form_region/attachment_region一样在所选DispositionSet中恰好出现一次；
23. 两个并发bind/remap/unbind使用同一WorkspaceHead CAS，失败方不得写入独立binding current或覆盖胜出方；
24. 人工事实和流程响应使用普通narrative/table/structured_form，只有QuoteSnapshot使用专用业务快照身份。

## 22. 完成语义

“目标契约已确认”只表示本文决策已批准，不表示代码已实现。只有Target V2 schema、API、Web编辑器、候选生成、Assessment、render snapshot、DOCX/PDF、旧路径删除、fresh deploy和端到端验收全部完成，才能称该能力已完成。
