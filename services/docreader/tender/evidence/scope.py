"""Evidence-bound human scope decisions over immutable native source ranges."""

from collections import Counter
import re

from pydantic import ValidationError

from docreader.tender.evidence.context import condition_context
from docreader.tender.schema import ScopeDecision


class ScopeDecisionValidationError(ValueError):
    """Invalid review input; diagnostics expose codes rather than source content."""

    def __init__(self, codes: list[str]):
        self.codes = sorted(set(codes))
        super().__init__("Invalid scope decision: " + ", ".join(self.codes))


_GENERIC_RATIONALE = re.compile(
    r"(?:误报|局部条款|局部条件|无作用域|不适用|已确认|已审核|通过|正确|绑定|"
    r"false positive|local(?: only| clause)?|approved|reviewed|confirmed|ok|bind)[。.! ]*", re.I,
)
_LOCAL_BOUNDARY = re.compile(r"句|行|列|字段|单元格|表单|条款|局部|该表|本表|clause|sentence|row|cell|field|form|local", re.I)
_LOCAL_BASIS = re.compile(r"仅|只|未|不|没有|自身|针对|约束|因为|因此|only|does not|no |because|limited|constrain", re.I)


def _range_origin(source: dict) -> str:
    return source.get("source_range_id", source.get("parent_range_id", source["range_id"]))


def validate_scope_decisions(frozen_index: dict, pack_plan_index: dict, decisions: list[dict], repo) -> dict:
    """Validate a finite review increment; the store binds its current plan/revision.

    Targets must be complete canonical native ranges, so subsequent structural
    re-planning can map derived body fragments without losing reviewed scope.
    Natural-language correctness remains the accountable human reviewer's decision;
    these checks prove its source, target and explicit reasoning, not semantic truth.
    """
    if not isinstance(decisions, list) or not decisions:
        raise ScopeDecisionValidationError(["scope_decisions_empty"])
    try:
        normalized = [ScopeDecision.model_validate(value).model_dump(mode="json") for value in decisions]
    except ValidationError:
        raise ScopeDecisionValidationError(["scope_decision_schema_invalid"]) from None
    if any(count != 1 for count in Counter(item["range_id"] for item in normalized).values()):
        raise ScopeDecisionValidationError(["scope_duplicate_decision"])

    canonical, candidates = {}, {}
    for document in frozen_index["documents"]:
        if not document.get("source_index_ref"):
            continue  # An unreadable/unsupported sibling does not hide valid reviewable sources.
        source = repo.source_index(document["source_index_ref"])
        canonical.update(source.by_id)
        _, conditions, _, _ = condition_context(source.ranges)
        candidates.update((candidate["range_id"], candidate) for values in conditions.values() for _, candidate in values)
    unresolved = {finding["range_id"] for finding in pack_plan_index.get("issues", [])
                  if finding.get("code") == "condition_scope_unresolved" and finding.get("range_id")}
    available = {**canonical, **candidates}
    requested_evidence = {item["range_id"] for decision in normalized for item in decision["evidence"]}
    # Derived quote IDs can be supplied from the current review view. Load each
    # current pack at most once, only if canonical/context identities are insufficient.
    if requested_evidence - available.keys():
        for entry in pack_plan_index.get("packs", []):
            pack = repo.load_pack(entry)
            available.update((source["range_id"], source) for source in [*pack["body"], *pack["supporting"]])
            if requested_evidence <= available.keys():
                break

    errors = []
    for decision in normalized:
        candidate = candidates.get(decision["range_id"])
        if decision["range_id"] not in unresolved or candidate is None:
            errors.append("scope_issue_not_current")
            continue
        rationale = decision["rationale"].strip()
        if _GENERIC_RATIONALE.fullmatch(rationale):
            errors.append("scope_rationale_unspecific")
        if decision["decision"] == "local_only" and (
            not _LOCAL_BOUNDARY.search(rationale) or not _LOCAL_BASIS.search(rationale)
        ):
            errors.append("scope_local_reason_missing")
        anchored = False
        for evidence in decision["evidence"]:
            cited = available.get(evidence["range_id"])
            quote = evidence["quote"]
            if cited is None or not quote.strip() or quote not in cited["text"]:
                errors.append("scope_evidence_mismatch")
                continue
            # Citing unrelated words elsewhere in the same section is insufficient.
            if (_range_origin(cited) == _range_origin(candidate)
                    and cited["start"] <= candidate["start"] <= candidate["end"] <= cited["end"]
                    and candidate["text"].strip() in quote):
                anchored = True
        if not anchored:
            errors.append("scope_issue_evidence_missing")
        for target_id in decision["target_range_ids"]:
            target = canonical.get(target_id)
            if target is None:
                errors.append("scope_target_not_canonical")
            elif target["document_id"] != candidate["document_id"]:
                # Current source selectors have one native owner. Refuse a target
                # that cannot be materialized rather than dropping required support.
                errors.append("scope_target_document_mismatch")
        decision["rationale"] = rationale
        decision["target_range_ids"] = sorted(decision["target_range_ids"])
    if errors:
        raise ScopeDecisionValidationError(errors)
    return {"schema_version": 1, "scope_decisions": sorted(normalized, key=lambda item: item["range_id"])}
