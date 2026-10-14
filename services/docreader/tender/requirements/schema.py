"""JSON contracts shared by extraction, independent checks and response plans."""

from typing import Any, Literal, Protocol

from pydantic import BaseModel, ConfigDict, Field, StrictBool, StrictStr, field_validator


class Model(Protocol):
    async def call(self, task: str, payload: dict, images: tuple = ()) -> dict: ...


class Contract(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)


class Evidence(Contract):
    range_id: StrictStr = Field(min_length=1)
    quote: StrictStr = Field(min_length=1)


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
    status: Literal["requirements", "non_requirement", "unresolved"]
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


class Plan(Contract):
    items: list[PlanItem]


def issue(code: str, detail: str, range_id: str | None = None, **context) -> dict:
    result = {"code": code, "detail": detail, "blocking": True, **context}
    if range_id is not None:
        result["range_id"] = range_id
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
