"""One extraction request per frozen pack, with complete body dispositions."""

from collections import Counter
from copy import deepcopy

from pydantic import ValidationError

from docreader.llm import ModelBudgetError, ModelSemanticError
from docreader.tender.evidence.view import host_reviewed_ranges, source_view
from docreader.tender.schema import Extraction, Model, enforce_issue_policy, issue, source_ranges


def _unresolved(source: dict, reason: str) -> dict:
    return {
        "range_id": source["range_id"], "status": "unresolved",
        "reason": reason, "requirement_ids": [],
    }


def extract_payload(pack: dict) -> dict:
    """The exact sending view, also used to probe the complete request budget."""
    return source_view(pack)


def _normalize_extraction(pack: dict, result: dict) -> dict:
    body = {source["range_id"]: source for source in pack["body"]}
    blank = host_reviewed_ranges(pack)
    issues = [*pack.get("issues", []), *result.get("issues", [])]
    if pack.get("pending_scope_context"):
        issues.append(issue("scope_context_pending", "Source scope context is unresolved; locator metadata is not reviewed source text."))
    if len(body) != len(pack["body"]):
        issues.append(issue("duplicate_body_range", "A body range occurs more than once in this pack."))
    counts = Counter(item["range_id"] for item in result["dispositions"])
    given = {item["range_id"]: item for item in result["dispositions"]}
    requirements = {item["requirement_id"]: item for item in result["requirements"]}
    if len(requirements) != len(result["requirements"]):
        issues.append(issue("duplicate_requirement_id", "Requirement IDs must be unique within a pack."))
    for range_id in given.keys() - body.keys():
        issues.append(issue("unknown_disposition_range", "Only body ranges receive a disposition.", range_id))
    dispositions = []
    for range_id, source in body.items():
        if range_id in blank:
            disposition = {
                "range_id": range_id, "status": "non_requirement", "reason": source["non_requirement_reason"],
                "requirement_ids": [],
            }
        elif counts[range_id] != 1:
            disposition = _unresolved(source, "Missing or duplicate range disposition.")
            issues.append(issue("range_disposition_incomplete", disposition["reason"], range_id))
        else:
            disposition = given[range_id]
            referenced = disposition["requirement_ids"]
            supported = {
                requirement_id for requirement_id, requirement in requirements.items()
                if any(e["range_id"] == range_id for e in requirement["evidence"])
            }
            invalid = not disposition["reason"].strip()
            if disposition["status"] == "requirements":
                invalid |= not referenced or set(referenced) != supported or len(set(referenced)) != len(referenced)
            else:
                invalid |= bool(referenced) or bool(supported)
                if disposition["status"] == "non_requirement":
                    invalid |= disposition["reason"].strip().lower() in {"no requirement", "not a requirement", "none", "非要求", "无要求", "无"}
                    if source.get("non_requirement_reason") == "empty_table_row":
                        invalid |= disposition["reason"].strip().lower() in {
                            "empty_table_row", "blank row", "empty row", "blank template", "空白行", "空行", "空白模板",
                        }
            if invalid:
                disposition = _unresolved(source, "Disposition does not agree with evidence-backed requirements.")
                issues.append(issue("invalid_disposition", disposition["reason"], range_id))
        if disposition["status"] == "unresolved":
            issues.append(issue("unresolved_source_range", disposition["reason"], range_id))
        dispositions.append(disposition)
    return {
        "pack_id": pack["pack_id"], "requirements": result["requirements"],
        "dispositions": dispositions, "issues": [enforce_issue_policy(item) for item in issues],
        "source_ranges": source_ranges(pack),
        **({"pending_scope_context": deepcopy(pack["pending_scope_context"])} if "pending_scope_context" in pack else {}),
    }


async def extract_pack(pack: dict, model: Model) -> dict:
    blank = host_reviewed_ranges(pack)
    try:
        if all(source["range_id"] in blank for source in pack["body"]):
            response = Extraction(requirements=[], dispositions=[])
        else:
            response = Extraction.model_validate(await model.call("extract", extract_payload(pack)))
        result = response.model_dump(exclude_none=True)
    except ModelBudgetError:
        result = {"requirements": [], "dispositions": [], "issues": [
            issue("extraction_budget_exceeded", "Complete extraction input exceeds admission limits.")
        ]}
    except (ModelSemanticError, ValidationError) as exc:
        # Do not expose a raw model response (or SDK error text) in diagnostics.
        result = {"requirements": [], "dispositions": [], "issues": [
            issue("extraction_semantic_failure", f"Extraction contract rejected ({type(exc).__name__}).")
        ]}
    return _normalize_extraction(pack, result)


class CorrectionValidationError(ValueError):
    """A correction has invalid structure or source claims; details contain codes only."""

    def __init__(self, codes: list[str]):
        self.codes = sorted(set(codes))
        super().__init__("Invalid correction: " + ", ".join(self.codes))


def validate_correction(pack: dict, corrected: dict) -> dict:
    """Validate an edited extraction against frozen evidence, without model calls.

    Valid unresolved ranges remain blocking and still require independent review.
    Host-owned source identity and pack issues are rebuilt rather than accepted from
    the editor. An accepted correction is never a verification or confirmation.
    """
    from docreader.tender.checks.verify import check_requirement

    try:
        result = Extraction.model_validate(corrected).model_dump(exclude_none=True)
    except ValidationError:
        raise CorrectionValidationError(["correction_schema_invalid"]) from None
    # Reject malformed edits, rather than quietly converting them into unresolved
    # dispositions. Existing source problems and explicit unresolved items may remain.
    normalized = _normalize_extraction(pack, {**result, "issues": []})
    invalid_codes = {
        "duplicate_body_range", "duplicate_requirement_id", "unknown_disposition_range",
        "range_disposition_incomplete", "invalid_disposition",
    }
    errors = [finding["code"] for finding in normalized["issues"] if finding["code"] in invalid_codes]
    sources = {source["range_id"]: source for source in [*pack["supporting"], *pack["body"]]}
    body_ids = {source["range_id"] for source in pack["body"]}
    for requirement in normalized["requirements"]:
        errors.extend(finding["code"] for finding in check_requirement(requirement, sources, body_ids))
    if errors:
        raise CorrectionValidationError(errors)
    return _normalize_extraction(pack, result)
