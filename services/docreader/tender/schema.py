"""The single JSON field contract for the independent Python tender workflow.

Native parser protocols are adapted once at the evidence boundary. These
unpublished workflow contracts deliberately accept no old internal aliases.
"""

from typing import Annotated, Any, Literal, Protocol

from pydantic import BaseModel, ConfigDict, Field, StrictBool, StrictInt, StrictStr, field_validator, model_validator


Identity = Annotated[StrictStr, Field(min_length=1)]
Digest = Annotated[StrictStr, Field(pattern=r"^[0-9a-f]{64}$")]
PositiveInt = Annotated[StrictInt, Field(ge=1)]
NonnegativeInt = Annotated[StrictInt, Field(ge=0)]
Task = Literal["extract", "verify", "plan", "ocr"]
RunStatus = Literal["pending", "running", "waiting-confirmation", "response-plan-ready", "blocked", "failed", "cancelled"]
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
    range_id: StrictStr = Field(min_length=1)
    quote: StrictStr = Field(min_length=1)


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
    quote: StrictStr = Field(min_length=1)

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
    code: StrictStr = Field(min_length=1)
    detail: StrictStr = Field(min_length=1)
    blocking: StrictBool = True
    range_id: StrictStr | None = None


class Extraction(Contract):
    requirements: list[Requirement]
    dispositions: list[Disposition]
    issues: list[Issue] = Field(default_factory=list)


class Verification(Contract):
    reviewed_range_ids: list[StrictStr]
    findings: list[Issue]


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
    start: NonnegativeInt
    end: NonnegativeInt
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
    text: StrictStr
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
