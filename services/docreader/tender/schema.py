"""The single JSON field contract for the independent Python tender workflow.

Native parser protocols are adapted once at the evidence boundary. These
unpublished workflow contracts deliberately accept no old internal aliases.
"""

import json
import math
from typing import Annotated, Any, Literal, Protocol

from pydantic import (
    AfterValidator, BaseModel, BeforeValidator, ConfigDict, Field, StrictBool,
    StrictFloat, StrictInt, StrictStr, field_validator, model_validator,
)


Identity = Annotated[StrictStr, Field(min_length=1)]
Digest = Annotated[StrictStr, Field(pattern=r"^[0-9a-f]{64}$")]
PositiveInt = Annotated[StrictInt, Field(ge=1)]
NonnegativeInt = Annotated[StrictInt, Field(ge=0)]
Task = Literal["extract", "verify", "plan", "ocr"]
RunStatus = Literal["pending", "running", "waiting-confirmation", "analysis-ready", "response-plan-ready", "blocked", "failed", "cancelled"]
CommandStatus = Literal["pending", "claimed", "published", "running", "completed", "failed", "cancelled"]
ModelRequestStatus = Literal["prepared", "sent", "succeeded", "failed", "uncertain"]
OCRStatus = Literal["complete", "partial", "unresolved"]
ScopeStatus = Literal["bound", "unresolved"]
DispositionStatus = Literal["requirements", "non_requirement", "unresolved"]
DocumentStatus = Literal["complete", "partial", "unresolved"]


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
            for name, field in cls.model_fields.items():
                if not field.is_required() and name in value and value[name] is None:
                    raise ValueError(f"omit optional {name} instead of providing null")
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


def _explicit_analysis_issue(value):
    if isinstance(value, Issue):
        fields = value.model_fields_set
        supplied = {name: getattr(value, name) for name in fields}
    elif isinstance(value, dict):
        fields, supplied = value.keys(), value
    else:
        return value
    if not {"code", "detail", "blocking"}.issubset(fields):
        raise ValueError("analysis issues require explicit code, detail and blocking")
    for name in ("range_id", "item_ids", "related_issue_ids", "needs_source"):
        if name in supplied and supplied[name] is None:
            raise ValueError(f"omit optional issue {name} instead of providing null")
    return value


_AnalysisIssue = Annotated[Issue, BeforeValidator(_explicit_analysis_issue)]


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


class ItemDispositionResponse(_AnalysisContract):
    """LLM-response boundary, not a canonical disposition; the host derives IDs."""

    range_id: Text
    status: Literal["items", "non_item", "unresolved"]
    reason: Text
    item_ids: list[Text] = Field(default_factory=list)


class EffectiveRule(_AnalysisContract):
    text: Text
    scope: Text
    evidence: list[Evidence] = Field(min_length=1)


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


def recover_issue_routing(value, allowed_ids: set[str], path: str, diagnostics: list):
    """Keep a finding when its optional routing cannot bind to this input."""
    if not isinstance(value, dict):
        return value
    result = dict(value)
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
