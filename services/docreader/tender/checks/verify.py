"""Independent source review plus deterministic evidence/number checks."""

import re
import unicodedata
from collections import Counter
from copy import deepcopy
from decimal import Decimal, InvalidOperation

from pydantic import ValidationError

from docreader.llm import ModelBudgetError, ModelSemanticError
from docreader.tender.evidence.view import host_reviewed_ranges, source_view
from docreader.tender.schema import Model, Verification, enforce_issue_policy, issue

NUMBER = re.compile(r"(?<![\d.])[-+]?\d+(?:,\d{3})*(?:\.\d+)?(?![\d.])")
UNIT = re.compile(r"^(?:万元|亿元|元|工作日|小时|分钟|天|日|周|月|年|分|%|‰|千米|厘米|毫米|平方米|立方米|米|吨|公斤|千克|个|件|套|份|人|kg\b|cm\b|mm\b|m[23²³]?\b)", re.I)


def _normal(text: str) -> str:
    return unicodedata.normalize("NFKC", text)


def numeric_values(text: str) -> set[Decimal]:
    return {Decimal(match.group().replace(",", "")) for match in NUMBER.finditer(_normal(text))}


def _numeric_leaves(value):
    if isinstance(value, dict):
        for child in value.values():
            yield from _numeric_leaves(child)
    elif isinstance(value, list):
        for child in value:
            yield from _numeric_leaves(child)
    elif not isinstance(value, bool) and isinstance(value, (int, float, str)):
        yield from numeric_values(str(value))


def _number_supported(number: dict, quotes: list[str]) -> bool:
    quote = number["quote"]
    if not any(quote in evidence_quote for evidence_quote in quotes):
        return False
    value = _normal(str(number["value"])).replace(",", "")
    unit = re.sub(r"\s+", "", _normal(number["unit"]))
    normalized_quote = _normal(quote)
    try:
        expected = Decimal(value)
    except InvalidOperation:
        # Chinese numerals and explicitly named values still require literal support.
        return (value + unit) in re.sub(r"\s+", "", normalized_quote)
    if not expected.is_finite():
        return False
    for match in NUMBER.finditer(normalized_quote):
        if Decimal(match.group().replace(",", "")) != expected:
            continue
        tail = re.sub(r"\s+", "", normalized_quote[match.end():])
        if (not unit and not UNIT.match(tail)) or (unit and tail.startswith(unit)):
            return True
    return False


def check_requirement(requirement: dict, sources: dict, body_ids: set[str]) -> list[dict]:
    findings = []
    requirement_id = requirement["requirement_id"]
    valid_quotes = []
    has_body_evidence = False
    for evidence in requirement["evidence"]:
        range_id, quote = evidence["range_id"], evidence["quote"]
        source = sources.get(range_id)
        if source is None or not quote.strip() or quote not in source["text"]:
            findings.append(issue("evidence_quote_mismatch", "Evidence must quote its exact frozen source.", range_id, requirement_id=requirement_id))
        else:
            valid_quotes.append(quote)
            has_body_evidence |= range_id in body_ids
    if not has_body_evidence:
        findings.append(issue("missing_body_evidence", "A requirement needs at least one body-range quote; supporting context alone is insufficient.", requirement_id=requirement_id))
    source_numbers = numeric_values("\n".join(valid_quotes))
    if not numeric_values(requirement["text"]).issubset(source_numbers):
        findings.append(issue("requirement_number_unsupported", "Requirement text includes a number absent from its valid source quotes.", requirement_id=requirement_id))
    for number in requirement["numbers"]:
        if not _number_supported(number, valid_quotes):
            findings.append(issue("number_or_unit_unsupported", "A declared number/unit pair does not occur in its source quote.", requirement_id=requirement_id))
    # Numeric details in the requirement cannot silently disappear from the structured list.
    declared = set().union(*(numeric_values(str(n["value"])) for n in requirement["numbers"]))
    if not numeric_values(requirement["text"]).issubset(declared):
        findings.append(issue("number_not_recorded", "Requirement text has numeric values missing from its numbers list.", requirement_id=requirement_id))
    scoring = requirement["scoring"]
    if not set(_numeric_leaves(scoring)).issubset(source_numbers):
        findings.append(issue("scoring_number_unsupported", "Scoring values lack evidence; no default score total is assumed.", requirement_id=requirement_id))
    # Sum only an explicitly sourced total and its explicitly supplied components.
    if "total" in scoring and "components" in scoring:
        try:
            components = scoring["components"]
            if not isinstance(components, list) or not components:
                raise ValueError("components must be a nonempty list")
            points = [Decimal(str(item["points"] if isinstance(item, dict) else item)) for item in components]
            total = Decimal(str(scoring["total"]))
            if not total.is_finite() or any(not point.is_finite() for point in points):
                raise ValueError("non-finite scoring")
            if sum(points) != total:
                findings.append(issue("explicit_score_total_mismatch", "Declared scoring components do not match the source's explicit total.", requirement_id=requirement_id))
        except (InvalidOperation, TypeError, KeyError, ValueError):
            findings.append(issue("invalid_explicit_scoring", "Explicit scoring total/components are not numeric.", requirement_id=requirement_id))
    return findings


def verify_payload(pack: dict, extraction: dict) -> dict:
    host_reviewed = host_reviewed_ranges(pack)
    return {**source_view(pack), "requirements": extraction["requirements"],
            "dispositions": [item for item in extraction["dispositions"] if item["range_id"] not in host_reviewed]}


async def verify_pack(pack: dict, extraction: dict, model: Model) -> dict:
    sources = {source["range_id"]: source for source in [*pack["supporting"], *pack["body"]]}
    body_ids = {source["range_id"] for source in pack["body"]}
    host_reviewed = host_reviewed_ranges(pack)
    payload = verify_payload(pack, extraction)
    model_source_ids = ({source["range_id"] for source in [*payload["body"], *payload["supporting"],
                                                         *payload.get("source_aliases", [])]}
                        | {range_id for table in payload.get("blank_table_templates", [])
                           for row in table["rows"] for range_id in row["range_ids"]})
    findings = []
    if pack.get("pending_scope_context"):
        findings.append(issue("scope_context_pending", "Scope context has only pending locator metadata; do not claim it was independently reviewed or resolved."))
    for requirement in extraction["requirements"]:
        findings.extend(check_requirement(requirement, sources, body_ids))
    try:
        if pack.get("planning_details", {}).get("budget", {}).get("fits") is False:
            raise ModelBudgetError("indivisible verification input exceeds budget")
        from docreader.tender.evidence.downstream import DEPENDENCY_ISSUES

        dependency_issues = [item for item in pack.get("issues", []) if item["code"] in DEPENDENCY_ISSUES]
        if dependency_issues:
            response = {"reviewed_range_ids": [], "findings": []}
            findings.extend(dependency_issues)
        elif model_source_ids:
            response = await model.call("verify", payload)
        else:
            response = {"reviewed_range_ids": [], "findings": []}
        review = Verification.model_validate(response).model_dump(exclude_none=True)
        findings.extend(enforce_issue_policy(finding) for finding in review["findings"])
        reviewed = review["reviewed_range_ids"]
    except ModelBudgetError:
        reviewed = []
        findings.append(issue("verification_budget_exceeded", "Complete indivisible verification input exceeds admission limits; no semantic result was produced."))
    except (ModelSemanticError, ValidationError) as exc:
        reviewed = []
        findings.append(issue("verification_semantic_failure", f"Independent review contract rejected ({type(exc).__name__})."))
    reviewed_counts = Counter(reviewed)
    for range_id in model_source_ids:
        if reviewed_counts[range_id] != 1:
            findings.append(issue("source_not_independently_reviewed", "Independent review must read each body and supporting range exactly once.", range_id))
    for range_id in reviewed_counts.keys() - model_source_ids:
        findings.append(issue("unknown_review_range", "Independent review refers to a range outside the supplied source view.", range_id))
    for finding in findings:
        if finding.get("range_id") and finding["range_id"] not in sources.keys() | model_source_ids:
            finding["blocking"] = True
    return {
        "pack_id": pack["pack_id"], "reviewed_range_ids": reviewed, "findings": findings,
        **({"host_reviewed_range_ids": sorted(host_reviewed)} if host_reviewed else {}),
        **({"pending_scope_context": deepcopy(pack["pending_scope_context"])} if "pending_scope_context" in pack else {}),
    }
