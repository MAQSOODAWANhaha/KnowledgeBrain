"""The single JSON field contract for the independent Python tender workflow.

Native parser protocols are adapted once at the evidence boundary. These
unpublished workflow contracts deliberately accept no old internal aliases.
"""

from copy import deepcopy
import hashlib
import json
import math
from typing import Annotated, Any, Literal, Protocol

from pydantic import (
    AfterValidator, BaseModel, BeforeValidator, ConfigDict, Field, StrictBool,
    StrictFloat, StrictInt, StrictStr, field_validator, model_validator,
)


from docreader.tender.status import CommandStatus, ModelRequestStatus, RunStatus


Identity = Annotated[StrictStr, Field(min_length=1)]
Digest = Annotated[StrictStr, Field(pattern=r"^[0-9a-f]{64}$")]
PositiveInt = Annotated[StrictInt, Field(ge=1)]
NonnegativeInt = Annotated[StrictInt, Field(ge=0)]
Task = Literal["extract", "verify", "plan", "ocr"]
OCRStatus = Literal["complete", "partial", "unresolved"]
ScopeStatus = Literal["bound", "unresolved"]
DispositionStatus = Literal["requirements", "non_requirement", "unresolved"]
DocumentStatus = Literal["complete", "partial", "unresolved"]
AUTHORING_GROUNDING_POLICY = "knowledge_required_v2"


class Model(Protocol):
    async def call(self, task: Task, payload: dict, images: tuple = ()) -> dict: ...


class Contract(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)


def _meaningful_text(value: str) -> str:
    if not value.strip():
        raise ValueError("source-backed text cannot be blank")
    return value


def _finite_number(value: int | float) -> int | float:
    if isinstance(value, float) and not math.isfinite(value):
        raise ValueError("numeric facts must be finite")
    return value


def _version_two(value):
    if type(value) is not int or value != 2:
        raise ValueError("analysis schema_version must be integer 2")
    return value


Text = Annotated[StrictStr, Field(min_length=1), AfterValidator(_meaningful_text)]
FiniteNumber = Annotated[StrictInt | StrictFloat, AfterValidator(_finite_number)]
AnalysisVersion = Annotated[Literal[2], BeforeValidator(_version_two)]
ItemKind = Literal["obligation", "scoring_rule", "consequence_rule", "submission_requirement"]


class _AnalysisContract(Contract):
    @model_validator(mode="before")
    @classmethod
    def omit_unknown_optional_fields(cls, value):
        if isinstance(value, dict):
            omitted = {name for name, field in cls.model_fields.items()
                       if not field.is_required() and field.default is None}
            return {name: entry for name, entry in value.items() if entry is not None or name not in omitted}
        return value


class RequestLimits(Contract):
    context_tokens: PositiveInt | None
    input_budget_tokens: PositiveInt = 131072
    correction_input_budget_tokens: PositiveInt = 262144
    correction: StrictBool = False


class ArtifactRef(Contract):
    sha256: Digest
    relative_path: StrictStr = Field(pattern=r"^[0-9a-f]{2}/[0-9a-f]{64}$")
    size: NonnegativeInt
    media_type: Identity

    @model_validator(mode="after")
    def content_address(self):
        if self.relative_path != f"{self.sha256[:2]}/{self.sha256}":
            raise ValueError("artifact path must match its content digest")
        return self


class DocumentIdentity(Contract):
    document_id: Identity
    document_version: Identity


class SourceIdentity(Contract):
    run_id: Identity
    source_version: PositiveInt


class RunIdentity(SourceIdentity):
    domain_revision: PositiveInt


class AuthoringInputIssue(Contract):
    code: Text
    detail: Text


class AuthoringInput(RunIdentity):
    schema_version: AnalysisVersion
    checklist_digest: Digest | None = None
    frozen_ref: ArtifactRef | None = None
    pack_plan_ref: ArtifactRef | None = None
    items_ref: ArtifactRef | None = None
    verification_ref: ArtifactRef | None = None
    confirmation_ref: ArtifactRef | None = None
    extract_refs: list[ArtifactRef] = Field(default_factory=list)
    issues: list[AuthoringInputIssue] = Field(default_factory=list)


class SourceReceiptIdentity(SourceIdentity):
    """Revision zero is reserved for immutable source receipts, not domain work."""
    domain_revision: Annotated[StrictInt, Field(ge=0, le=0)] = 0


class SourceRangeIdentity(DocumentIdentity):
    unit_id: Identity
    range_id: Identity
    source_range_id: Identity
    parent_range_id: Identity | None = None
    section_id: Identity | None = None
    parent_section_id: Identity | None = None
    heading_path: StrictStr = ""
    start: NonnegativeInt
    end: NonnegativeInt

    @model_validator(mode="after")
    def ordered_interval(self):
        if self.end < self.start:
            raise ValueError("source character interval must be ordered")
        return self


class Evidence(Contract):
    range_id: Text
    quote: Text


class ScopeDecision(Contract):
    range_id: Identity
    decision: Literal["bind", "local_only"]
    evidence: list[Evidence] = Field(min_length=1)
    rationale: Identity
    target_range_ids: list[Identity]

    @model_validator(mode="after")
    def explicit_targets(self):
        if not self.rationale.strip():
            raise ValueError("scope decision rationale cannot be blank")
        if len(self.target_range_ids) != len(set(self.target_range_ids)):
            raise ValueError("scope decision targets must be unique")
        if (self.decision == "bind") != bool(self.target_range_ids):
            raise ValueError("bind requires explicit targets; local_only requires no targets")
        return self


class ScopeBinding(Contract):
    section_id: Identity | None = None
    heading_path: StrictStr = ""
    kind: Identity
    relationship: Literal["cumulative"] = "cumulative"
    status: ScopeStatus
    table_ids: list[Identity] = Field(default_factory=list)
    complete_sentence: StrictBool
    applies_to_range_ids: list[Identity] = Field(default_factory=list)
    reviewed_target_range_ids: list[Identity] = Field(default_factory=list)
    decision_range_id: Identity | None = None


class RequirementRef(Contract):
    artifact_ref: ArtifactRef
    requirement_id: Identity


class DispositionRef(Contract):
    artifact_ref: ArtifactRef
    range_id: Identity


class ApplicabilityDecision(Contract):
    requirement_id: Identity
    applicable: StrictBool
    applicability_basis: Identity
    evidence: list[Evidence] = Field(default_factory=list)
    amendment_document_id: Identity | None = None
    source_document_id: Identity | None = None
    source_range_id: Identity | None = None

    @field_validator("applicability_basis")
    @classmethod
    def meaningful_basis(cls, value):
        if not value.strip():
            raise ValueError("applicability decision requires a basis")
        return value


class Number(Contract):
    value: str | int | float
    unit: StrictStr
    quote: Text

    @field_validator("value")
    @classmethod
    def nonempty_value(cls, value):
        if isinstance(value, bool) or not str(value).strip():
            raise ValueError("number value must be explicit")
        return value


class Requirement(Contract):
    requirement_id: StrictStr = Field(min_length=1)
    text: StrictStr = Field(min_length=1)
    subject: StrictStr = Field(min_length=1)
    evidence: list[Evidence] = Field(min_length=1)
    conditions: list[StrictStr]
    exceptions: list[StrictStr]
    numbers: list[Number]
    scoring: dict[str, Any]
    veto_risk: StrictBool
    cross_references: list[StrictStr]
    applicable: StrictBool
    applicability_basis: StrictStr = Field(min_length=1)

    @field_validator("text", "subject", "applicability_basis")
    @classmethod
    def meaningful_text(cls, value):
        if not value.strip():
            raise ValueError("required text cannot be blank")
        return value


class Disposition(Contract):
    range_id: StrictStr = Field(min_length=1)
    status: DispositionStatus
    reason: StrictStr = Field(min_length=1)
    requirement_ids: list[StrictStr]


class Issue(Contract):
    code: Text
    detail: Text
    blocking: StrictBool = True
    range_id: Text | None = None
    item_ids: Annotated[list[Text], Field(min_length=1)] | None = None
    related_issue_ids: Annotated[list[Text], Field(min_length=1)] | None = None
    needs_source: StrictBool | None = None

    @field_validator("item_ids", mode="before")
    @classmethod
    def omit_absent_item_ids(cls, value):
        if value is None:
            raise ValueError("omit unknown item_ids instead of providing null")
        return value


class AnalysisIssue(_AnalysisContract, Issue):
    blocking: StrictBool = Field()

    @model_validator(mode="before")
    @classmethod
    def explicit_issue_fields(cls, value):
        if isinstance(value, Issue):
            return value.model_dump(exclude_unset=True)
        return value


_AnalysisIssue = AnalysisIssue


class OCRResponse(Contract):
    text: StrictStr
    status: OCRStatus
    issues: list[Issue] = Field(default_factory=list)


class Extraction(Contract):
    requirements: list[Requirement]
    dispositions: list[Disposition]
    issues: list[Issue] = Field(default_factory=list)


class Verification(Contract):
    reviewed_range_ids: list[StrictStr]
    findings: list[Issue]


class ItemRef(_AnalysisContract):
    artifact_ref: ArtifactRef
    item_id: Text


class EffectiveItemRef(_AnalysisContract):
    item_ref: ItemRef
    decision_id: Identity | None = None
    rule_index: NonnegativeInt | None = None

    @model_validator(mode="after")
    def retained_rule_identity(self):
        if (self.decision_id is None) != (self.rule_index is None):
            raise ValueError("a retained rule requires both decision_id and rule_index")
        return self


class SourceUnitRef(Contract):
    source_index_ref: ArtifactRef
    unit_id: Identity


class SourceExcerptRef(Contract):
    source_ref: SourceUnitRef
    selectors: list["RangeSelector"] = Field(min_length=1)


class TemplatePartRef(Contract):
    source_ref: SourceUnitRef
    selectors: list["RangeSelector"]
    header_row_count: NonnegativeInt | None = None


ResponseTargetKind = Literal["narrative", "form", "attachment"]
ChapterPresentationRole = Literal["body", "cover", "submission_note"]
SupportOrigins = Annotated[list[Literal["tender", "knowledge"]], Field(min_length=1, max_length=2),
                           AfterValidator(lambda values: list(dict.fromkeys(values)))]
SearchQuery = Annotated[Text, Field(max_length=256)]


class TemplateSpanResponse(Contract):
    start_part_id: Identity = Field(description=(
        "First supplied template part of one complete native format; the host includes this part and every "
        "supplied part through end_part_id in the same frozen document's native order."))
    end_part_id: Identity = Field(description=(
        "Last supplied template part of the same complete native format, inclusive; never select another "
        "document or an endpoint before start_part_id."))


class ResponseTableRowResponse(Contract):
    range_ids: list[Identity] = Field(min_length=1)
    rule_ids: list[Identity] = Field(default_factory=list)


class ResponseTableResponse(Contract):
    context_range_ids: list[Identity] = Field(default_factory=list, description=(
        "Select supplied exact source ranges for original headers, units, conditions and guidance applying to the whole table. "
        "Use actual sources in the source request and shared source_excerpts in assembly; keep row-specific conditions with their rows."))
    rows: list[ResponseTableRowResponse] = Field(min_length=1)


class OutlineTargetResponse(Contract):
    kind: ResponseTargetKind
    purpose: Text
    rule_ids: list[Identity] = Field(description=(
        "Select only this request's top-level rules[].rule_id. item_id values in related_rules, "
        "relations or issues are read-only context; never create a rule ID by changing an item ID prefix."))
    range_ids: list[Identity] = Field(description=(
        "Select supplied bindable source or evidence/context range references; "
        "these may include context not read in this batch."))
    template_part_ids: list[Identity] = Field(default_factory=list)
    template_spans: list[TemplateSpanResponse] = Field(default_factory=list)
    subject: Text | None = Field(default=None, description=(
        "The source-required subject role for this one form instance, not an invented company name; "
        "different required subjects remain separate targets."))
    response_table: ResponseTableResponse | None = None
    manual_fill: StrictBool = False
    support_origins: SupportOrigins = Field(default_factory=lambda: ["knowledge"])


class OutlineChapterResponse(Contract):
    title: Text
    targets: list[OutlineTargetResponse] = Field(default_factory=list)
    children: list["OutlineChapterResponse"] = Field(default_factory=list)
    is_volume: StrictBool = False
    presentation_role: ChapterPresentationRole = "body"


class OutlineFinding(Contract):
    code: Text
    detail: Text
    rule_ids: list[Identity] = Field(description=(
        "Select only this request's top-level rules[].rule_id. item_id values in related_rules, "
        "relations or issues are read-only context; never create a rule ID by changing an item ID prefix."))


class SearchIntentResponse(Contract):
    query: SearchQuery
    for_ids: list[Identity] = Field(min_length=1)


class OutlineResponse(Contract):
    schema_version: AnalysisVersion
    chapters: list[OutlineChapterResponse]
    findings: list[OutlineFinding]
    constraint_rule_ids: list[Identity] = Field(default_factory=list, description=(
        "Select only this request's top-level rules[].rule_id. item_id values in related_rules, "
        "relations or issues are read-only context; never create a rule ID by changing an item ID prefix."))
    constraint_range_ids: list[Identity] = Field(default_factory=list, description=(
        "Select only sources[].range_id actually read in this batch. A range mentioned only in "
        "evidence, relations or issues does not qualify."))
    search_intents: list[SearchIntentResponse] = Field(default_factory=list)


class OutlineAssemblyTarget(Contract):
    kind: ResponseTargetKind
    purpose: Text
    target_ids: list[Identity] = Field(default_factory=list)
    rule_ids: list[Identity] = Field(default_factory=list, description=(
        "Select additional supplied rules. On revision, omit to retain the unique previous position's explicit "
        "selection for the same non-empty candidate ID set and subject; an explicit array replaces that additional selection, "
        "and [] clears it. Original candidate rules always remain."))
    source_ids: list[Identity] = Field(default_factory=list, description=(
        "Select additional bindable submission_sources for understanding. On revision, omit to retain the unique "
        "previous position's explicit selection for the same non-empty candidate ID set and subject; an explicit array replaces "
        "that additional selection, and [] clears it. This selection is independent of rule_ids; original candidate "
        "sources and the currently selected rules' exact evidence remain."))
    template_part_ids: list[Identity] = Field(default_factory=list, description=(
        "Omit both template_part_ids and template_spans to retain the selected instances' complete formats. "
        "Providing either field replaces the format selection; explicit empty arrays clear a mistaken format "
        "while retaining its rules and source context."))
    template_spans: list[TemplateSpanResponse] = Field(default_factory=list, description=(
        "Optional inclusive ranges of a complete format in the supplied native order. "
        "When combined with explicit part IDs, the host keeps native order. Omit both template selection "
        "fields to retain the existing complete selection."))
    subject: Text | None = Field(default=None, description=(
        "The source-required subject role for this one form instance; preserve distinct candidate subjects "
        "as separate instances even when they use the same native format."))
    response_table: ResponseTableResponse | None = None
    related_target_ids: list[Identity] = Field(default_factory=list, description=(
        "Select supplied candidate references. On revision, omit to retain the unique previous position's "
        "selection for the same non-empty candidate ID set and subject; an explicit array replaces it, and [] clears it. "
        "References are resolved against the revised positions; ambiguous instances remain unresolved."))
    manual_fill: StrictBool = Field(default=False, description=(
        "On revision, omit to retain the unique previous position's manual-fill boundary for the same non-empty "
        "candidate ID set and subject. An explicit boolean replaces an assembly-added flag; original candidate manual-fill "
        "requirements always remain."))
    support_origins: SupportOrigins = Field(default_factory=lambda: ["knowledge"])

    @model_validator(mode="after")
    def grounded_position(self):
        if not self.target_ids and not self.rule_ids and not (
                self.kind == "narrative" and self.related_target_ids):
            raise ValueError("a position requires a candidate, an unassigned rule, or a narrative reference to a candidate")
        return self


class OutlineAssemblyChapter(Contract):
    title: Text
    targets: list[OutlineAssemblyTarget] = Field(default_factory=list)
    is_volume: StrictBool = False
    presentation_role: ChapterPresentationRole = "body"
    children: list["OutlineAssemblyChapter"] = Field(default_factory=list)


class OutlineAssemblyExclusion(Contract):
    target_ids: list[Identity] = Field(min_length=1)
    reason: Text


class OutlineAssemblyRuleExclusion(Contract):
    rule_ids: list[Identity] = Field(min_length=1)
    reason: Text


class OutlineTableHeader(Contract):
    part_id: Identity
    header_row_count: NonnegativeInt


class OutlineAssemblyResponse(Contract):
    chapters: list[OutlineAssemblyChapter]
    excluded_targets: list[OutlineAssemblyExclusion] = Field(default_factory=list)
    constraint_rule_ids: list[Identity] = Field(default_factory=list)
    excluded_rules: list[OutlineAssemblyRuleExclusion] = Field(default_factory=list)
    table_headers: list[OutlineTableHeader] = Field(default_factory=list)
    search_intents: list[SearchIntentResponse] = Field(default_factory=list)


class ResponseTableRow(Contract):
    row_id: Identity
    source_excerpts: list[SourceExcerptRef] = Field(min_length=1)
    rule_refs: list[EffectiveItemRef] = Field(default_factory=list)


class ResponseTable(Contract):
    context_source_excerpts: list[SourceExcerptRef] = Field(default_factory=list)
    rows: list[ResponseTableRow] = Field(min_length=1)


class ResponseTarget(Contract):
    target_id: Identity
    kind: ResponseTargetKind
    purpose: Text
    rule_refs: list[EffectiveItemRef]
    source_refs: list[SourceUnitRef]
    source_excerpts: list[SourceExcerptRef] = Field(default_factory=list)
    template_refs: list[TemplatePartRef] = Field(default_factory=list)
    subject: Text | None = None
    response_table: ResponseTable | None = None
    manual_fill: StrictBool = False
    support_origins: SupportOrigins = Field(default_factory=lambda: ["knowledge"])
    related_target_ids: list[Identity] = Field(default_factory=list)


class OutlineChapter(Contract):
    chapter_id: Identity
    title: Text
    targets: list[ResponseTarget]
    children: list["OutlineChapter"]
    is_volume: StrictBool = False
    presentation_role: ChapterPresentationRole = "body"


class OutlineIssue(_AnalysisContract):
    code: Text
    detail: Text
    rule_refs: list[EffectiveItemRef]
    target_id: Identity | None = None


class SearchIntent(Contract):
    query: SearchQuery
    rule_refs: list[EffectiveItemRef] = Field(default_factory=list)
    source_refs: list[SourceUnitRef] = Field(default_factory=list)
    range_ids: list[Identity] = Field(default_factory=list)


class AuthoringOutline(Contract):
    schema_version: AnalysisVersion
    authoring_input: AuthoringInput
    chapters: list[OutlineChapter]
    issues: list[OutlineIssue]
    constraint_rule_refs: list[EffectiveItemRef] = Field(default_factory=list)
    submission_refs: list[SourceUnitRef] = Field(default_factory=list)
    search_intents: list[SearchIntent] = Field(default_factory=list)


class KnowledgeMaterial(Contract):
    material_id: Identity
    title: Text
    text: StrictStr
    content_ref: ArtifactRef | None = None
    original_ref: ArtifactRef | None = None
    locator: dict = Field(default_factory=dict)
    usage: Literal["fact", "reference", "proof"] = "reference"
    attachment: StrictBool = False


class AuthoringArrangement(Contract):
    topic: Text
    text: StrictStr
    nature: Literal["provided", "proposed"]
    material_ids: list[Identity] = Field(default_factory=list)


class AuthoringArrangementResponse(Contract):
    topic: Text
    text: StrictStr
    nature: Literal["provided", "proposed"]
    support_ids: list[Identity] = Field(default_factory=list)


class AuthoringBasisResponse(Contract):
    project_background: StrictStr = ""
    approach: StrictStr = ""
    shared_arrangements: list[AuthoringArrangementResponse] = Field(default_factory=list)
    gaps: list[StrictStr] = Field(default_factory=list)


class AuthoringBasisContent(Contract):
    project_background: StrictStr = ""
    approach: StrictStr = ""
    shared_arrangements: list[AuthoringArrangement] = Field(default_factory=list)
    gaps: list[StrictStr] = Field(default_factory=list)


class AuthoringBasis(Contract):
    schema_version: AnalysisVersion = 2
    authoring_input: AuthoringInput
    response: AuthoringBasisContent
    materials: list[KnowledgeMaterial] = Field(default_factory=list)


class AuthoringBlockResponse(Contract):
    kind: Literal["paragraph", "table"]
    text: StrictStr = ""
    headers: list[StrictStr] = Field(default_factory=list)
    rows: list[list[StrictStr]] = Field(default_factory=list)
    target_ids: list[Identity] = Field(default_factory=list)
    support_ids: list[Identity] = Field(default_factory=list)


class AuthoringBlock(Contract):
    kind: Literal["paragraph", "table"]
    text: StrictStr = ""
    headers: list[StrictStr] = Field(default_factory=list)
    rows: list[list[StrictStr]] = Field(default_factory=list)
    target_ids: list[Identity] = Field(default_factory=list)
    material_ids: list[Identity] = Field(default_factory=list)
    source_refs: list[SourceUnitRef] = Field(default_factory=list)
    source_excerpts: list[SourceExcerptRef] = Field(default_factory=list)


class CellFillResponse(Contract):
    source_id: Identity
    row: NonnegativeInt
    column: NonnegativeInt
    text: StrictStr
    target_ids: list[Identity] = Field(default_factory=list)
    support_ids: list[Identity] = Field(default_factory=list)


class CellFill(Contract):
    source_ref: SourceUnitRef
    row: NonnegativeInt
    column: NonnegativeInt
    text: StrictStr
    target_ids: list[Identity] = Field(default_factory=list)
    material_ids: list[Identity] = Field(default_factory=list)
    source_refs: list[SourceUnitRef] = Field(default_factory=list)
    source_excerpts: list[SourceExcerptRef] = Field(default_factory=list)


class RowFillResponse(Contract):
    row_id: Identity
    text: StrictStr
    support_ids: list[Identity] = Field(default_factory=list)


class RowFill(Contract):
    row_id: Identity
    text: StrictStr
    material_ids: list[Identity] = Field(default_factory=list)
    source_refs: list[SourceUnitRef] = Field(default_factory=list)
    source_excerpts: list[SourceExcerptRef] = Field(default_factory=list)


class AuthoringAttachment(Contract):
    material_id: Identity
    title: Text
    artifact_ref: ArtifactRef


class WritingResponse(Contract):
    blocks: list[AuthoringBlockResponse] = Field(default_factory=list)
    cell_fills: list[CellFillResponse] = Field(default_factory=list)
    row_fills: list[RowFillResponse] = Field(default_factory=list)
    attachment_ids: list[Identity] = Field(default_factory=list)
    gaps: list[StrictStr] = Field(default_factory=list)


class WritingTask(Contract):
    task_id: Identity
    chapter_id: Identity
    title_path: list[Text]
    targets: list[ResponseTarget]


class WritingUnit(Contract):
    schema_version: AnalysisVersion = 2
    task_id: Identity
    chapter_id: Identity
    target_ids: list[Identity]
    status: Literal["generated", "placeholder"]
    blocks: list[AuthoringBlock] = Field(default_factory=list)
    cell_fills: list[CellFill] = Field(default_factory=list)
    row_fills: list[RowFill] = Field(default_factory=list)
    attachments: list[AuthoringAttachment] = Field(default_factory=list)
    gaps: list[StrictStr] = Field(default_factory=list)
    error_type: StrictStr | None = None


class AuthoringReviewRequest(Contract):
    target_ids: list[Identity] = Field(min_length=1)
    reason: Text


class AuthoringReviewFinding(Contract):
    code: Text
    detail: Text
    task_ids: list[Identity] = Field(default_factory=list)
    content_ids: list[Identity] = Field(default_factory=list)
    suggestion: StrictStr = ""
    can_revise: StrictBool = False
    reject_content: StrictBool = Field(default=False, description=(
        "True only when identified generated content is incorrect or unsupported. "
        "Missing content, missing materials and unavailable review do not reject existing content."))


class AuthoringReviewResponse(Contract):
    findings: list[AuthoringReviewFinding]


class SourceDecorationExclusion(Contract):
    target_id: Identity = Field(description="Select the current structure view's exact target_id.")
    support_ids: list[Identity] = Field(min_length=1, description=(
        "Select only this target's template_support_ids that contain solely standalone source page "
        "headers, footers or page numbers. Never select a part containing substantive text, conditions, "
        "notes, table titles, filling instructions or signatures, even if deemed inapplicable. "
        "Preserve mixed or uncertain parts; source/context supports do not qualify."))
    reason: Text = Field(description=(
        "Explain why each selected part is solely source page decoration, with no substantive content. "
        "Inapplicability of a substantive clause never authorizes exclusion."))


class AuthoringStructureResponse(AuthoringReviewResponse):
    source_decoration_exclusions: list[SourceDecorationExclusion] = Field(default_factory=list, description=(
        "Optional exclusions of standalone source page headers, footers or page numbers only. "
        "Never remove substantive fixed-format content or applicability notes. "
        "Executable only when source_decoration_edits_allowed is true; otherwise retained as suggestions. "
        "Do not infer actions from finding codes or prose. Rules, source references, exact excerpts "
        "and response rows remain unchanged."))


class AuthoringGap(Contract):
    code: Text
    detail: Text
    task_id: Identity | None = None
    chapter_id: Identity | None = None
    target_id: Identity | None = None


class AuthoringUsage(Contract):
    sent_requests: NonnegativeInt = 0
    retry_requests: NonnegativeInt = 0
    estimated_input_tokens: NonnegativeInt = 0
    prompt_tokens: NonnegativeInt = 0
    completion_tokens: NonnegativeInt = 0
    total_tokens: NonnegativeInt = 0
    unknown_usage_requests: NonnegativeInt = 0


class AuthoringDocument(Contract):
    schema_version: AnalysisVersion = 2
    authoring_input: AuthoringInput
    outline_ref: ArtifactRef
    requirements_ref: ArtifactRef | None = None
    basis_ref: ArtifactRef
    unit_refs: list[ArtifactRef]
    review_ref: ArtifactRef
    initial_review_ref: ArtifactRef | None = None
    superseded_unit_refs: list[ArtifactRef] = Field(default_factory=list)
    docx_ref: ArtifactRef
    package_ref: ArtifactRef
    gaps: list[AuthoringGap]
    usage: AuthoringUsage


class ScoringDetails(_AnalysisContract):
    method: Text | None = None
    points: FiniteNumber | None = None
    max_points: Annotated[FiniteNumber, Field(ge=0)] | None = None

    @model_validator(mode="after")
    def explicit_details(self):
        if not self.model_fields_set:
            raise ValueError("scoring details require at least one source fact")
        return self


class ConsequenceDetails(_AnalysisContract):
    effect: Text | None = None
    trigger: Text | None = None

    @model_validator(mode="after")
    def explicit_details(self):
        if not self.model_fields_set:
            raise ValueError("consequence details require at least one source fact")
        return self


class SubmissionDetails(_AnalysisContract):
    fields: Annotated[list[Text], Field(min_length=1)] | None = None
    documents: Annotated[list[Text], Field(min_length=1)] | None = None

    @model_validator(mode="after")
    def explicit_details(self):
        if not self.model_fields_set:
            raise ValueError("submission details require at least one source fact")
        return self


class TenderItem(_AnalysisContract):
    item_id: Text
    kind: ItemKind
    text: Text
    evidence: list[Evidence] = Field(min_length=1)
    domains: Annotated[list[Text], Field(min_length=1)] | None = None
    subject: Text | None = None
    scope: Text | None = None
    conditions: Annotated[list[Text], Field(min_length=1)] | None = None
    exceptions: Annotated[list[Text], Field(min_length=1)] | None = None
    numbers: Annotated[list[Number], Field(min_length=1)] | None = None
    scoring: ScoringDetails | None = None
    consequence: ConsequenceDetails | None = None
    submission: SubmissionDetails | None = None

    @field_validator("numbers")
    @classmethod
    def finite_quantities(cls, value):
        for number in value:
            if isinstance(number.value, float):
                _finite_number(number.value)
        return value

    @model_validator(mode="after")
    def details_match_kind(self):
        for name, kind in (("scoring", "scoring_rule"), ("consequence", "consequence_rule"),
                           ("submission", "submission_requirement")):
            if getattr(self, name) is not None and self.kind != kind:
                raise ValueError(f"{name} details are only valid for {kind}")
        return self


class Relation(_AnalysisContract):
    relation_id: Text
    from_item_id: Text
    to_item_id: Text
    relation: Text
    evidence: list[Evidence] = Field(min_length=1)


class ItemDisposition(_AnalysisContract):
    range_id: Text
    status: Literal["items", "non_item", "unresolved"]
    reason: Text
    item_ids: list[Text]

    @model_validator(mode="after")
    def explicit_item_coverage(self):
        if (self.status == "items") != bool(self.item_ids):
            raise ValueError("items disposition requires IDs; other dispositions require an empty list")
        if len(set(self.item_ids)) != len(self.item_ids):
            raise ValueError("disposition item IDs must be unique")
        return self


class ItemDispositionOutput(_AnalysisContract):
    range_id: Text
    status: Literal["non_item", "unresolved"]
    reason: Text


class ItemDispositionResponse(ItemDispositionOutput):
    """LLM-response boundary. New extracts declare only non_item or unresolved.

    status=items remains accepted so an older complete packet can be replayed.
    The host derives item coverage and item_ids from evidence.
    """

    status: Literal["items", "non_item", "unresolved"]
    item_ids: list[Text] = Field(default_factory=list)


class EffectiveRule(_AnalysisContract):
    text: Text
    scope: Text
    evidence: list[Evidence] = Field(min_length=1)


class AuthoringRequirement(Contract):
    rule_ref: EffectiveItemRef
    rule: dict[str, Any] | None = None
    target_ids: list[Identity] = Field(default_factory=list)


class AuthoringRequirements(Contract):
    schema_version: AnalysisVersion = 2
    authoring_input: AuthoringInput
    rule_origin: Literal["extraction", "verified_projection", "source_only"]
    rules: list[AuthoringRequirement]
    issues: list[AuthoringInputIssue] = Field(default_factory=list)


class AmendmentEffect(_AnalysisContract):
    target_item_id: Text
    effect: Literal["replace", "delete"]
    affected_scope: Text
    retained_rules: list[EffectiveRule]
    replacement_item_ids: list[Text]

    @model_validator(mode="after")
    def replacement_matches_effect(self):
        if (self.effect == "replace") != bool(self.replacement_item_ids):
            raise ValueError("replace requires replacement IDs; delete requires an empty list")
        if len(set(self.replacement_item_ids)) != len(self.replacement_item_ids):
            raise ValueError("replacement item IDs must be unique")
        return self


class Decision(_AnalysisContract):
    decision_id: Text
    issue_ids: list[Text]
    reason: Text
    evidence: list[Evidence] = Field(min_length=1)
    relation_ids: Annotated[list[Text], Field(min_length=1)] | None = None
    amendment: AmendmentEffect | None = None

    @model_validator(mode="after")
    def explicit_resolution_targets(self):
        if not self.issue_ids and self.amendment is None:
            raise ValueError("only an amendment decision may omit existing issue targets")
        for name in ("issue_ids", "relation_ids"):
            ids = getattr(self, name)
            if ids is not None and len(set(ids)) != len(ids):
                raise ValueError(f"decision {name} must be unique")
        return self


class SectionRef(_AnalysisContract):
    document_id: Text
    section_id: Text | None


class PageRef(_AnalysisContract):
    document_id: Text
    page_ordinal: NonnegativeInt


class Candidate(_AnalysisContract):
    candidate_id: Text
    issue_ids: list[Text]
    range_ids: list[Text]
    item_ids: list[Text]
    reason: Text
    section_refs: Annotated[list[SectionRef], Field(min_length=1)] | None = None
    page_refs: Annotated[list[PageRef], Field(min_length=1)] | None = None

    @model_validator(mode="after")
    def identifiable_targets(self):
        if not self.range_ids and not self.item_ids and not self.section_refs and not self.page_refs:
            raise ValueError("a candidate must select at least one source, item, section or page")
        for name in ("issue_ids", "range_ids", "item_ids"):
            ids = getattr(self, name)
            if len(set(ids)) != len(ids):
                raise ValueError(f"candidate {name} must be unique")
        if self.section_refs:
            sections = [(ref.document_id, ref.section_id) for ref in self.section_refs]
            if len(set(sections)) != len(sections):
                raise ValueError("candidate section references must be unique")
        if self.page_refs:
            pages = [(ref.document_id, ref.page_ordinal) for ref in self.page_refs]
            if len(set(pages)) != len(pages):
                raise ValueError("candidate page references must be unique")
        return self


class CatalogSection(_AnalysisContract):
    section_id: Text | None
    heading_path: StrictStr
    range_ids: list[Text] | None = Field(default=None, min_length=1)


class CatalogEntry(_AnalysisContract):
    document_id: Text
    document_version: Text
    role: Text
    sections: list[CatalogSection]
    page_count: PositiveInt | None = None

    @model_validator(mode="after")
    def unique_section_identities(self):
        identities = [section.section_id for section in self.sections]
        if len(set(identities)) != len(identities):
            raise ValueError("catalog section identities must be unique within a document")
        return self


class AnalysisExtraction(_AnalysisContract):
    schema_version: AnalysisVersion
    items: list[TenderItem]
    dispositions: list[ItemDisposition]
    relations: list[Relation]
    issues: list[_AnalysisIssue]


class AnalysisExtractionResponse(_AnalysisContract):
    """LLM-response boundary, not an artifact; preserve local defects for review."""

    schema_version: AnalysisVersion
    items: list[Any]
    dispositions: list[Any]
    relations: list[Any]
    issues: list[Any]


class AnalysisReviewResponse(_AnalysisContract):
    """Only semantic judgments cross the model boundary; coverage is host-owned."""

    schema_version: AnalysisVersion
    accepted_decision_ids: list[Text]
    findings: list[_AnalysisIssue]


class AnalysisVerification(_AnalysisContract):
    schema_version: AnalysisVersion
    reviewed_range_ids: list[Text]
    reviewed_item_ids: list[Text]
    reviewed_relation_ids: list[Text]
    reviewed_decision_ids: list[Text]
    accepted_decision_ids: list[Text]
    findings: list[_AnalysisIssue]

    @field_validator("reviewed_range_ids", "reviewed_item_ids", "reviewed_relation_ids",
                     "reviewed_decision_ids", "accepted_decision_ids")
    @classmethod
    def unique_review_ids(cls, value):
        if len(set(value)) != len(value):
            raise ValueError("reviewed and accepted identity lists must be unique")
        return value

    @model_validator(mode="after")
    def acceptance_requires_review(self):
        if not set(self.accepted_decision_ids).issubset(self.reviewed_decision_ids):
            raise ValueError("accepted decisions must have been independently reviewed")
        return self


class LocateResult(_AnalysisContract):
    schema_version: AnalysisVersion
    candidates: list[Candidate]
    findings: list[_AnalysisIssue]


class AssociationResult(_AnalysisContract):
    schema_version: AnalysisVersion
    relations: list[Relation]
    decisions: list[Decision]
    findings: list[_AnalysisIssue]


class AnalysisWorkUnit(_AnalysisContract):
    """One unchanged semantic dependency group within a shared physical request."""

    work_unit_id: Text
    candidate_ids: list[Text]
    issue_ids: list[Text]
    decision_ids: list[Text] | None = None


class AnalysisBatchResponse(_AnalysisContract):
    """Validate the complete envelope; route and validate each member separately."""

    schema_version: AnalysisVersion
    work_units: list[Any]
    shared_findings: list[Any]


class AnalysisSharedFinding(_AnalysisContract):
    work_unit_ids: list[Text] = Field(min_length=1)
    finding: _AnalysisIssue


class PlanItem(Contract):
    requirement_id: StrictStr = Field(min_length=1)
    target_kind: Literal["body", "table_field", "attachment", "manual_action"]
    target: StrictStr = Field(min_length=1)
    action: StrictStr = Field(min_length=1)
    material_needs: list[StrictStr] = Field(min_length=1)
    constraint_digest: StrictStr = Field(pattern=r"^[0-9a-f]{64}$")


class Plan(Contract):
    items: list[PlanItem]


class TextBoundaries(Contract):
    start: StrictBool
    end: StrictBool


class RangeSelector(Contract):
    """A small immutable-content selector; never embeds source prose or grids."""
    range_id: Identity
    source_range_id: Identity
    parent_range_id: Identity | None = None
    start: NonnegativeInt
    end: NonnegativeInt
    reason: StrictStr | None = None
    scope_binding: ScopeBinding | None = None
    split_lineage: list[Identity] = Field(default_factory=list)
    previous_fragment_id: Identity | None = None
    next_fragment_id: Identity | None = None
    fragment_ordinal: NonnegativeInt | None = None
    text_boundaries: TextBoundaries | None = None

    @model_validator(mode="after")
    def ordered_interval(self):
        if self.end < self.start:
            raise ValueError("selector character interval must be ordered")
        return self


class PendingScopeContext(SourceRangeIdentity):
    """Reviewable source metadata, retaining the same canonical range identity."""
    reason: Identity
    scope_binding: ScopeBinding
    locator: dict[str, Any]
    physical_locator: dict[str, Any] | None = None
    physical_path: StrictStr | None = None


class ScopeBindingRef(Contract):
    """Complete condition identity when its text is already in body/supporting."""
    range_id: Identity
    source_range_id: Identity
    start: NonnegativeInt
    end: NonnegativeInt
    scope_binding: ScopeBinding

    @model_validator(mode="after")
    def ordered_interval(self):
        if self.end < self.start:
            raise ValueError("scope binding character interval must be ordered")
        return self


class SectionView(Contract):
    section_id: Identity
    parent_section_id: Identity | None = None
    heading_path: StrictStr


class BlankRowInterval(Contract):
    row_start: NonnegativeInt
    row_end: PositiveInt
    range_ids: list[Identity] = Field(min_length=1)

    @model_validator(mode="after")
    def exact_rows(self):
        if self.row_end - self.row_start != len(self.range_ids) or len(set(self.range_ids)) != len(self.range_ids):
            raise ValueError("template interval must retain exactly one identity for each original row")
        return self


class MergeView(Contract):
    row: NonnegativeInt
    column: NonnegativeInt
    row_span: PositiveInt
    col_span: PositiveInt


class BlankTableTemplate(Contract):
    table_id: Identity
    column_count: PositiveInt
    section_id: Identity | None = None
    rows: list[BlankRowInterval] = Field(min_length=1)
    merges: list[MergeView] = Field(default_factory=list)
    table_ids: list[Identity] = Field(default_factory=list)


class SourceCellView(Contract):
    column: NonnegativeInt
    text: StrictStr
    row: NonnegativeInt | None = None
    row_span: PositiveInt | None = None
    col_span: PositiveInt | None = None
    formula: StrictStr | None = None
    formula_references: list[StrictStr] = Field(default_factory=list)
    display_complete: StrictBool | None = None
    display_incomplete_reason: StrictStr | None = None
    image_anchor: dict[str, Any] | None = None


class SourceRangeView(Contract):
    range_id: Identity
    text: StrictStr | None = None
    kind: Identity | None = None
    unit_id: Identity | None = None
    source_range_id: Identity | None = None
    start: NonnegativeInt | None = None
    end: NonnegativeInt | None = None
    section_id: Identity | None = None
    table_id: Identity | None = None
    table_ids: list[Identity] = Field(default_factory=list)
    row_start: NonnegativeInt | None = None
    row_end: PositiveInt | None = None
    column_count: PositiveInt | None = None
    cells: list[SourceCellView] = Field(default_factory=list)
    header: StrictBool | None = None
    leading_row: StrictBool | None = None
    text_boundaries: TextBoundaries | None = None
    reason: Identity | None = None
    ocr_status: OCRStatus | None = None
    image_unit_ids: list[Identity] = Field(default_factory=list)
    image_anchor: dict[str, Any] | None = None
    compound_parent: dict[str, Any] | None = None
    scope_binding: ScopeBinding | None = None

    @model_validator(mode="after")
    def supplied_original_text(self):
        if "text" in self.model_fields_set and self.text is None:
            raise ValueError("omit an unsupplied row text instead of providing null")
        if self.text is None and not self.cells:
            raise ValueError("a source range requires original text or cells")
        return self


class PendingScopeView(Contract):
    range_id: Identity
    source_range_id: Identity | None = None
    parent_range_id: Identity | None = None
    start: NonnegativeInt | None = None
    end: NonnegativeInt | None = None
    reason: Identity | None = None
    section_id: Identity | None = None
    scope_binding: ScopeBinding


class SourceSegment(Contract):
    range_id: Identity
    start: NonnegativeInt
    end: NonnegativeInt


class SourceAlias(Contract):
    range_id: Identity
    segments: list[SourceSegment] = Field(min_length=1)


class SourceView(Contract):
    """A necessary sending projection; physical evidence remains canonical."""
    pack_id: Identity
    document_id: Identity | None = None
    document_version: Identity | None = None
    role: Identity | None = None
    body: list[SourceRangeView]
    supporting: list[SourceRangeView]
    sections: list[SectionView] = Field(default_factory=list)
    blank_table_templates: list[BlankTableTemplate] = Field(default_factory=list)
    pending_scope_context: list[PendingScopeView] = Field(default_factory=list)
    scope_bindings: list[ScopeBindingRef] = Field(default_factory=list)
    source_aliases: list[SourceAlias] = Field(default_factory=list)


class PlanningDetails(Contract):
    planner_version: Identity
    boundary_reason: Identity
    section_ids: list[Identity | None]
    budget: dict[str, Any]
    budget_spec_ref: ArtifactRef


class PackDescriptor(DocumentIdentity):
    """One request's selectors, tied to the enclosing revision's PackPlan."""
    schema_version: PositiveInt
    pack_id: Identity
    task: Task
    input_digest: Digest
    budget_digest: Digest
    plan_digest: Digest
    raw_ref: ArtifactRef
    source_index_ref: ArtifactRef
    body: list[RangeSelector]
    supporting: list[RangeSelector]
    pending_scope_context: list[PendingScopeContext]
    scope_bindings: list[ScopeBindingRef] = Field(default_factory=list)
    # Source diagnostics retain their explicit native coordinates; unlike model
    # findings these can have format-specific location fields.
    issues: list[dict[str, Any]]
    compacted_blank_ranges: list[Identity]
    planning_details: PlanningDetails
    requirement_refs: list[RequirementRef] = Field(default_factory=list)
    disposition_refs: list[DispositionRef] = Field(default_factory=list)


class ResponsePackDescriptor(PackDescriptor):
    task: Literal["plan"]


class PackRef(Contract):
    pack_id: Identity
    document_id: Identity
    artifact_ref: ArtifactRef
    range_count: NonnegativeInt
    plan_digest: Digest | None = None


class SourceDocument(Contract):
    """A registered source summary may carry a rejected/missing identity issue."""
    document_id: StrictStr
    document_version: StrictStr
    role: StrictStr
    replaces: Any = None
    modification_scope: Any = None
    ordinal: NonnegativeInt
    issues: list[dict[str, Any]]
    file_name: StrictStr
    original_ref: ArtifactRef | None = None
    raw_ref: ArtifactRef | None = None
    source_index_ref: ArtifactRef | None = None
    status: DocumentStatus | None = None
    native_range_count: NonnegativeInt | None = None
    range_count: NonnegativeInt | None = None
    pack_count: NonnegativeInt | None = None
    parse_seconds: float | None = Field(default=None, ge=0)
    freeze_seconds: float | None = Field(default=None, ge=0)
    plan_stats: dict[str, Any] | None = None


class PlanningStats(Contract):
    document_count: NonnegativeInt
    pack_count: NonnegativeInt
    native_range_count: NonnegativeInt
    probe_count: NonnegativeInt
    image_count: NonnegativeInt = 0
    ocr_request_count: NonnegativeInt = 0
    duration_seconds: float = Field(default=0, ge=0)


class PackPlan(Contract):
    """Current domain revision's plan; frozen canonical sources remain separate."""
    schema_version: PositiveInt
    task: Task
    input_digest: Digest
    budget_digest: Digest
    plan_digest: Digest
    frozen_ref: ArtifactRef
    budget_spec_ref: ArtifactRef
    documents: list[SourceDocument]
    packs: list[PackRef]
    range_count: NonnegativeInt
    native_range_count: NonnegativeInt
    issues: list[dict[str, Any]]
    scope_decisions: list[ScopeDecision]
    stats: PlanningStats
    run_identity: RunIdentity | None = None
    requirements_ref: ArtifactRef | None = None
    verification_ref: ArtifactRef | None = None


def issue(code: str, detail: str, range_id: str | None = None, **context) -> dict:
    result = {"code": code, "detail": detail, "blocking": True, **context}
    if range_id is not None:
        result["range_id"] = range_id
    return result


# This is a host-owned policy, not a model-configurable severity list. A note in
# this one category may describe presentation only, never obligation correctness.
INFORMATIONAL_ISSUE_CODES = frozenset({"source_formatting_note"})


def enforce_issue_policy(finding: dict) -> dict:
    """Models may raise severity, but cannot downgrade correctness or unknown codes."""
    return {
        **finding,
        "blocking": finding.get("blocking") is not False or finding.get("code") not in INFORMATIONAL_ISSUE_CODES,
    }


ISSUE_EXITS = frozenset({"revise", "review-scope", "correct", "host-fix", "human"})
# Every host code raised on the analysis path is registered (enforced by a unit
# test). Any exit other than "human" is a hard blocker: the analysis itself is
# incomplete, so a confirmation verdict cannot clear it. Model findings remain
# semantic doubts even when they use a registered code; callers check provenance.
HOST_ISSUE_EXITS = {
    **dict.fromkeys((
        "missing_document", "missing_document_id", "missing_expected_document", "missing_document_version",
        "unreadable_document", "unsupported_document", "document_parse_failed", "empty_document",
        "empty_document_set", "duplicate_document_id", "duplicate_document_content", "invalid_document_role",
        "invalid_image_dispositions", "image_disposition_identity_mismatch", "no_source_ranges",
        "spreadsheet_display_incomplete", "analysis_section_source_unavailable", "analysis_page_source_unavailable",
    ), "revise"),
    **dict.fromkeys(("scope_context_pending", "condition_scope_unresolved"), "review-scope"),
    **dict.fromkeys((
        "range_disposition_incomplete", "duplicate_body_range", "extraction_semantic_failure",
        "invalid_disposition", "unknown_disposition_range", "duplicate_requirement_id",
    ), "correct"),
    **dict.fromkeys((
        "analysis_budget_exceeded", "extraction_budget_exceeded", "indivisible_source_context_exceeded",
        "analysis_review_incomplete",
    ), "host-fix"),
    **dict.fromkeys((
        "unresolved_source_range", "extraction_item_unresolved", "extraction_optional_fields_unresolved",
        "evidence_quote_mismatch", "missing_body_evidence",
        "extraction_relation_unresolved", "extraction_issue_unresolved", "extraction_disposition_unresolved",
        "decision_not_accepted",
        "amendment_effect_unresolved", "analysis_association_unresolved",
    ), "human"),
}


def is_hard_blocking(finding: dict) -> bool:
    """Classify a trusted host finding; establish its provenance before calling."""
    return finding["blocking"] is True and HOST_ISSUE_EXITS.get(finding["code"], "human") != "human"


def assign_issues(issues, namespace: str) -> list[dict]:
    """Host identities are stable and idempotent, including across receipt reuse."""
    result = {}
    for original in issues:
        problem = enforce_issue_policy(deepcopy(original))
        digest = hashlib.sha256(json.dumps(problem, ensure_ascii=False, sort_keys=True,
            separators=(",", ":"), allow_nan=False).encode()).hexdigest()
        identity = problem.get("issue_id") or f"{namespace}:q:{digest}"
        problem["issue_id"] = identity
        if identity in result and result[identity] != problem:
            raise ValueError("one issue identity refers to different facts")
        result[identity] = problem
    return list(result.values())


class IssueResolution(Contract):
    issue_id: Identity
    verdict: Literal["dismissed", "acknowledged"]
    reason: Text


class IssueResolutions(Contract):
    schema_version: Literal[1]
    resolutions: list[IssueResolution]


def validate_issue_resolutions(pending_issue_ids, submitted) -> dict | None:
    """One human verdict per pending semantic issue; none when nothing is pending."""
    if submitted is None and not pending_issue_ids:
        return None
    if submitted is None:
        raise ValueError("pending semantic issues require human resolutions")
    value = IssueResolutions.model_validate(submitted).model_dump(mode="json")
    identities = [entry["issue_id"] for entry in value["resolutions"]]
    if len(identities) != len(set(identities)) or set(identities) != set(pending_issue_ids):
        raise ValueError("resolutions must cover exactly the pending semantic issues")
    if not identities:
        return None
    return {**value, "resolutions": sorted(value["resolutions"], key=lambda entry: entry["issue_id"])}


def recover_issue_routing(value, allowed_ids: set[str], path: str, diagnostics: list):
    """Keep a finding when its optional routing cannot bind to this input."""
    if not isinstance(value, dict):
        return value
    result = AnalysisIssue.omit_unknown_optional_fields(value)
    links = result.get("related_issue_ids")
    bad_links = "related_issue_ids" in result and (
        not isinstance(links, list) or not links or any(not isinstance(i, str) or i not in allowed_ids for i in links))
    bad_route = "needs_source" in result and type(result["needs_source"]) is not bool
    if bad_links or bad_route:
        fields = {key: result.pop(key) for key in ("related_issue_ids", "needs_source") if key in result}
        diagnostics.append({"code": "issue_routing_unbound", "path": path, "fields": fields})
        if isinstance(result.get("detail"), str):
            result["detail"] += " 待核对的路由声明（不是证据）：" + json.dumps(fields, ensure_ascii=False, sort_keys=True)
    elif links:
        result["related_issue_ids"] = list(dict.fromkeys(links))
    return result


def source_ranges(pack: dict) -> dict:
    """Small source metadata for cross-pack rules; never duplicate source text."""
    body_ids = {source["range_id"] for source in pack["body"]}
    return {
        source["range_id"]: {
            "document_id": source["document_id"],
            "locator": source.get("locator", {}),
            "body": source["range_id"] in body_ids,
        }
        for source in [*pack["supporting"], *pack["body"]]
    }
