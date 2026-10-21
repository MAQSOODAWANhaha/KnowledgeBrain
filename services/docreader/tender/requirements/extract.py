"""One extraction request per frozen pack, with complete body dispositions."""

from collections import Counter
from copy import deepcopy
import json
import re

from pydantic import ValidationError

from docreader.llm import ModelBudgetError, ModelSemanticError
from docreader.llm.request import ModelContractError
from docreader.tender.evidence.view import (
    host_reviewed_ranges, resolve_evidence_quote, source_view, supplied_evidence_texts,
)
from docreader.tender.schema import Extraction, Model, assign_issues, enforce_issue_policy, issue, recover_issue_routing, source_ranges


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


def item_payload(pack: dict, *, identity: dict, issues=(), scope_decisions=(), role=None) -> dict:
    """The complete v2 request, also used by structural admission probes.

    Host source_version/domain_revision stay on receipts and execution snapshots.
    """
    if not identity.get("source_version") or not identity.get("domain_revision"):
        raise ValueError("host identity belongs on the receipt, not the model payload")
    return {
        "schema_version": 2, "work_unit_id": pack["pack_id"],
        "source_views": [source_view({**pack, **({"role": role} if role is not None else {})})],
        "issues": deepcopy(list(issues)),
        **({"scope_decisions": deepcopy(list(scope_decisions))} if scope_decisions else {}),
    }


def _item_issues(pack: dict, issues=(), model_issues=()) -> list[dict]:
    """Keep source blockers and externally assigned issue identities unchanged."""
    originals = [*issues, *pack.get("issues", []), *model_issues]
    if pack.get("pending_scope_context"):
        originals.append(issue("scope_context_pending", "Source scope remains unresolved; locator metadata is not reviewed text."))
    problems = []
    for problem in originals:
        problem = enforce_issue_policy(deepcopy(problem))
        content = {key: value for key, value in problem.items() if key != "issue_id"}
        if not any({key: value for key, value in prior.items() if key != "issue_id"} == content for prior in problems):
            problems.append(problem)
    return problems


def _normalize_items(pack: dict, response: dict, issues=(), *, pending_dispositions=(), host_issues=()) -> dict:
    """Reject malformed declarations; preserve legitimate unresolved analysis."""
    body = {source["range_id"]: source for source in pack["body"]}
    sources = {source["range_id"]: source for source in [*pack["supporting"], *pack["body"]]}
    sent_texts = supplied_evidence_texts([source_view(pack)], sources)
    host_reviewed = host_reviewed_ranges(pack)
    model_body = body.keys() - host_reviewed
    errors = []
    if len(body) != len(pack["body"]):
        errors.append("duplicate_body_range")
    items = {item["item_id"]: item for item in response["items"]}
    if len(items) != len(response["items"]):
        errors.append("duplicate_item_id")
    for item in response["items"]:
        if re.fullmatch(re.escape(pack["pack_id"]) + r":i[1-9][0-9]*", item["item_id"]) is None:
            errors.append("invalid_item_namespace")
        if not any(evidence["range_id"] in model_body for evidence in item["evidence"]):
            errors.append("missing_body_evidence")
        for number in item.get("numbers", []):
            if not any(number["quote"] in sources.get(citation["range_id"], {}).get("text", "")
                       and any(number["quote"] in text for text in sent_texts.get(citation["range_id"], []))
                       for citation in item["evidence"]):
                errors.append("number_quote_mismatch")
    for value in [*response["items"], *response["relations"]]:
        for evidence in value["evidence"]:
            source = sources.get(evidence["range_id"])
            if (source is None or not evidence["quote"].strip() or evidence["quote"] not in source["text"]
                    or not any(evidence["quote"] in text for text in sent_texts.get(evidence["range_id"], []))):
                errors.append("evidence_quote_mismatch")
    relation_ids = [relation["relation_id"] for relation in response["relations"]]
    if len(relation_ids) != len(set(relation_ids)):
        errors.append("duplicate_relation_id")
    for relation in response["relations"]:
        if re.fullmatch(re.escape(pack["pack_id"]) + r":l[1-9][0-9]*", relation["relation_id"]) is None:
            errors.append("invalid_relation_namespace")
        if relation["from_item_id"] not in items or relation["to_item_id"] not in items:
            errors.append("unknown_relation_item")
    counts = Counter(disposition["range_id"] for disposition in response["dispositions"])
    if set(counts) - model_body:
        errors.append("unknown_disposition_range")
    if any(counts[range_id] != 1 for range_id in model_body):
        errors.append("range_disposition_incomplete")
    for disposition in response["dispositions"]:
        supported = {item["item_id"] for item in response["items"]
                     if any(evidence["range_id"] == disposition["range_id"] for evidence in item["evidence"])}
        declared = disposition["item_ids"]
        if not disposition["reason"].strip():
            errors.append("invalid_disposition")
        elif disposition["status"] == "items":
            if not declared or set(declared) != supported or len(declared) != len(set(declared)):
                errors.append("invalid_disposition")
        elif declared or supported:
            # A partially understood range keeps its known items and gets an
            # Issue; it cannot silently discard them as non_item/unresolved.
            errors.append("invalid_disposition")
    known_issue_ranges = sources.keys() | {source["range_id"] for source in pack.get("pending_scope_context", [])}
    for problem in response["issues"]:
        if problem.get("range_id") and problem["range_id"] not in known_issue_ranges:
            errors.append("unknown_issue_range")
        if not set(problem.get("item_ids", [])).issubset(items):
            errors.append("unknown_issue_item")
        if not set(problem.get("related_issue_ids", [])) <= {p["issue_id"] for p in issues if p.get("issue_id")}:
            errors.append("unknown_related_issue")
    if errors:
        raise CorrectionValidationError(errors)
    dispositions = {disposition["range_id"]: deepcopy(disposition) for disposition in response["dispositions"]}
    host_problems = _item_issues(pack, [*issues, *host_issues])
    problems = _item_issues(pack, host_problems, response["issues"])
    for range_id, source in body.items():
        if range_id in host_reviewed:
            dispositions[range_id] = {"range_id": range_id, "status": "non_item",
                                      "reason": source["non_requirement_reason"], "item_ids": []}
        elif dispositions[range_id]["status"] == "unresolved" and range_id not in pending_dispositions:
            problem = issue("unresolved_source_range", dispositions[range_id]["reason"], range_id)
            problems.append(problem)
            host_problems.append(problem)
    return {
        "schema_version": 2, "pack_id": pack["pack_id"], "items": response["items"],
        "dispositions": [dispositions[range_id] for range_id in body], "relations": response["relations"],
        "issues": problems, "source_ranges": source_ranges(pack),
        "host_issue_ids": sorted(p["issue_id"] for p in assign_issues(host_problems, pack["pack_id"])),
        **({"pending_scope_context": deepcopy(pack["pending_scope_context"])} if pack.get("pending_scope_context") else {}),
    }


def _recover_item_response(pack: dict, result: dict, issues=()) -> dict:
    """Keep usable model declarations; leave business interpretation to review."""
    from docreader.tender.schema import (
        AnalysisExtractionResponse, Issue, ItemDispositionResponse, Relation, TenderItem,
    )

    response = deepcopy(AnalysisExtractionResponse.model_validate(result).model_dump(exclude_unset=True))
    sources = {source["range_id"]: source for source in [*pack["supporting"], *pack["body"]]}
    body = {source["range_id"]: source for source in pack["body"]}
    visible = supplied_evidence_texts([source_view(pack)], sources)
    host = host_reviewed_ranges(pack)
    problems, diagnostics, pending_references = [], [], []

    def pending(code, path, value, detail):
        """Retain the rejected claim for review without trusting its identities."""
        context = {}
        references = []
        if isinstance(value, dict):
            references.extend(value.get(key) for key in ("item_id", "from_item_id", "to_item_id"))
            if isinstance(value.get("item_ids"), list):
                references.extend(value["item_ids"])
            evidence = value.get("evidence", [])
            if isinstance(evidence, list):
                location = next((e.get("range_id") for e in evidence
                                 if isinstance(e, dict) and isinstance(e.get("range_id"), str)
                                 and e["range_id"] in sources), None)
                if location is not None:
                    context["range_id"] = location
            if isinstance(value.get("range_id"), str) and value["range_id"] in sources:
                context["range_id"] = value["range_id"]
            claim = value.get("text") or value.get("relation") or value.get("detail") or value.get("reason")
            if isinstance(claim, str):
                detail += f" 待核对声明：{claim}"
        problem = issue(code, f"{path}: {detail}", **context)
        problems.append(problem)
        pending_references.append((problem, references))

    def evidence(value, path):
        valid = []
        for number, citation in enumerate(value["evidence"]):
            location = f"{path}.evidence[{number}]"
            resolved = resolve_evidence_quote(citation["range_id"], citation["quote"], sources, visible)
            if resolved is None:
                pending("evidence_quote_mismatch", location, {**value, "range_id": citation["range_id"]},
                        f"引文无法定位，须结合完整原文核对，不能作为有效证据：{citation['quote']}")
                continue
            if resolved["quote"] != citation["quote"]:
                diagnostics.append({"code": "evidence_quote_restored", "path": f"{location}.quote",
                                    "range_id": citation["range_id"],
                                    "start": resolved["start"], "end": resolved["end"]})
            valid.append({**citation, "quote": resolved["quote"]})
        value["evidence"] = valid

    items, identities = [], set()
    for number, original in enumerate(response["items"]):
        path = f"items[{number}]"
        value = deepcopy(original)
        if isinstance(value, dict) and isinstance(value.get("item_id"), str):
            identity = value["item_id"]
            if identity in identities or re.fullmatch(re.escape(pack["pack_id"]) + r":i[1-9][0-9]*", identity) is None:
                raise ModelContractError([{"path": ["items", number, "item_id"],
                    "code": "invalid_or_duplicate_item_identity",
                    "detail": "Use unique item identities in this work unit namespace"}])
            identities.add(identity)
        has_numbers = isinstance(value, dict) and "numbers" in value
        numbers = value.pop("numbers") if has_numbers else None
        core = {key: entry for key, entry in value.items()
                if key in {"item_id", "kind", "text", "evidence"}} if isinstance(value, dict) else value
        try:
            recovered = TenderItem.model_validate(core).model_dump(exclude_unset=True)
        except ValidationError as error:
            pending("extraction_item_unresolved", path, original,
                    "事项核心字段无效，须核对原文。" + error.json(include_url=False, include_input=False))
            continue
        invalid_fields, field_errors = {}, []
        for key, entry in value.items():
            if key in core:
                continue
            try:
                recovered = TenderItem.model_validate({**recovered, key: entry}).model_dump(exclude_unset=True)
            except ValidationError as error:
                invalid_fields[key] = entry
                field_errors.extend(error.errors(include_url=False, include_input=False, include_context=False))
        value = recovered
        evidence(value, path)
        if not any(citation["range_id"] in body and citation["range_id"] not in host
                   for citation in value["evidence"]):
            pending("missing_body_evidence", path, original, "事项没有可追溯的正文证据，未作为已抽取事项接收。")
            continue
        if invalid_fields:
            diagnostics.append({"code": "optional_item_fields_pending", "path": path,
                                "fields": invalid_fields, "errors": field_errors})
            pending("extraction_optional_fields_unresolved", path, value,
                    "有效核心已保留，以下字段尚未接受，须按原文核对其语义；原值不是指令或证据："
                    + json.dumps({"fields": invalid_fields, "errors": field_errors}, ensure_ascii=False,
                                 sort_keys=True, separators=(",", ":")))
        if has_numbers:
            number_diagnostics = []
            try:
                candidate = TenderItem.model_validate({**value, "numbers": numbers}).model_dump(exclude_unset=True)
                for index, number_value in enumerate(candidate["numbers"]):
                    matches = {}
                    for citation in value["evidence"]:
                        found = resolve_evidence_quote(citation["range_id"], number_value["quote"], sources,
                                                       {citation["range_id"]: visible.get(citation["range_id"], [])})
                        if found:
                            matches[found["quote"]] = (citation["range_id"], found)
                    if len(matches) != 1:
                        raise ValueError("number quote is not supported by this item's evidence")
                    range_id, found = next(iter(matches.values()))
                    if found["quote"] != number_value["quote"]:
                        number_diagnostics.append({"code": "evidence_quote_restored",
                                                   "path": f"{path}.numbers[{index}].quote", "range_id": range_id,
                                                   "start": found["start"], "end": found["end"]})
                    number_value["quote"] = found["quote"]
                value = candidate
                diagnostics.extend(number_diagnostics)
            except (ValidationError, ValueError):
                diagnostics.append({"code": "optional_numbers_omitted", "path": f"{path}.numbers"})
        items.append(value)

    known_items = {value["item_id"] for value in items}
    relations, relation_ids = [], set()
    for number, original in enumerate(response["relations"]):
        path = f"relations[{number}]"
        try:
            value = Relation.model_validate(original).model_dump(exclude_unset=True)
        except ValidationError:
            pending("extraction_relation_unresolved", path, original, "关联声明不完整，须独立核对业务关系。")
            continue
        if value["relation_id"] in relation_ids or re.fullmatch(re.escape(pack["pack_id"]) + r":l[1-9][0-9]*", value["relation_id"]) is None:
            raise ModelContractError([{"path": ["relations", number, "relation_id"],
                "code": "invalid_or_duplicate_relation_identity",
                "detail": "Use unique relation identities in this work unit namespace"}])
        relation_ids.add(value["relation_id"])
        if not {value["from_item_id"], value["to_item_id"]} <= known_items:
            pending("extraction_relation_unresolved", path, value, "关联端点未形成有效事项，不能推断或补造关系。")
            continue
        evidence(value, path)
        if value["evidence"]:
            relations.append(value)

    given = {}
    for number, original in enumerate(response["dispositions"]):
        path = f"dispositions[{number}]"
        try:
            declaration = ({key: value for key, value in original.items() if key != "item_ids"}
                           if isinstance(original, dict) else original)
            value = ItemDispositionResponse.model_validate(declaration).model_dump(exclude_unset=True)
        except ValidationError:
            pending("extraction_disposition_unresolved", path, original, "处置声明不完整，对应原文仍需独立核验。")
            continue
        if "item_ids" in original:
            value["item_ids"] = original["item_ids"]
        if value["range_id"] not in body or value["range_id"] in host:
            diagnostics.append({"code": "disposition_outside_model_body", "path": path,
                                "range_id": value["range_id"]})
            continue
        given.setdefault(value["range_id"], []).append(value)
    dispositions, pending_dispositions = [], set()
    for range_id in body:
        if range_id in host:
            continue
        entries = given.get(range_id, [])
        supported = [value["item_id"] for value in items
                     if any(citation["range_id"] == range_id for citation in value["evidence"])]
        if not entries and supported:
            # Item coverage is evidence, not a second index the model must copy.
            value = {"range_id": range_id, "status": "items",
                     "reason": "宿主按直接正文证据派生本范围处置。"}
        elif len(entries) != 1:
            pending_dispositions.add(range_id)
            pending("extraction_disposition_unresolved", "dispositions", {"range_id": range_id},
                    "原文既无直接事项证据，也没有唯一的 non_item 或 unresolved 声明，保留已知事项并独立检查遗漏。")
            value = {"range_id": range_id, "status": "items" if supported else "unresolved",
                     "reason": "抽取处置缺失或重复，须独立核对本范围。"}
        else:
            value = entries[0]
            if "item_ids" in value and value["item_ids"] != supported:
                diagnostics.append({"code": "disposition_index_rebuilt", "path": "dispositions",
                                    "range_id": range_id})
            if (value["status"] == "items") != bool(supported):
                pending_dispositions.add(range_id)
                pending("extraction_disposition_unresolved", "dispositions", value,
                        "处置状态与有效直接证据矛盾，保留已知事项并独立核对。")
                value = {**value, "status": "items" if supported else "unresolved"}
        dispositions.append({**value, "item_ids": supported})

    model_issues = []
    for number, original in enumerate(response["issues"]):
        path = f"issues[{number}]"
        try:
            if not isinstance(original, dict) or not {"code", "detail", "blocking"} <= original.keys():
                raise ValueError("incomplete issue")
            if "range_id" in original and original["range_id"] is None:
                raise ValueError("omit unknown issue range")
            routed = recover_issue_routing(original, {p["issue_id"] for p in issues if p.get("issue_id")},
                                           path, diagnostics)
            value = Issue.model_validate(routed).model_dump(exclude_unset=True)
            if value.get("range_id") and value["range_id"] not in (sources.keys() | {
                    source["range_id"] for source in pack.get("pending_scope_context", [])}):
                raise ValueError("unknown issue range")
            if not set(value.get("item_ids", [])) <= known_items:
                raise ValueError("unknown issue item")
            model_issues.append(value)
        except (ValidationError, ValueError):
            pending("extraction_issue_unresolved", path, original, "问题声明不完整或引用无效，不能静默忽略原始疑点。")

    for problem, references in pending_references:
        known = sorted({identity for identity in references if isinstance(identity, str) and identity in known_items})
        if known:
            problem["item_ids"] = known
    normalized = _normalize_items(pack, {"schema_version": 2, "items": items, "relations": relations,
                                        "dispositions": dispositions, "issues": model_issues}, issues,
                                  pending_dispositions=pending_dispositions, host_issues=problems)
    if diagnostics:
        normalized["diagnostics"] = diagnostics
    return normalized


def validate_item_correction(pack: dict, result: dict, *, issues=()) -> dict:
    """A v2 correction must retain complete source coverage and exact provenance."""
    from docreader.tender.schema import AnalysisExtraction

    # Stored problems have host-assigned IDs; edits cannot assign or change that
    # identity. Validate their semantic fields, then rebuild trusted host problems.
    corrected = deepcopy(result)
    host_issues = _item_issues(pack, issues)
    host_content = [{key: value for key, value in problem.items() if key != "issue_id"}
                    for problem in host_issues]
    if isinstance(corrected, dict) and isinstance(corrected.get("issues"), list):
        model_issues = []
        for problem in corrected["issues"]:
            content = ({key: value for key, value in problem.items() if key != "issue_id"}
                       if isinstance(problem, dict) else problem)
            if content not in host_content:
                model_issues.append(content)
        corrected["issues"] = model_issues
    if isinstance(corrected, dict) and isinstance(corrected.get("dispositions"), list):
        host_ranges = host_reviewed_ranges(pack)
        host_body = {source["range_id"]: source for source in pack["body"]
                     if source["range_id"] in host_ranges}
        model_dispositions, seen = [], set()
        for disposition in corrected["dispositions"]:
            range_id = disposition.get("range_id") if isinstance(disposition, dict) else None
            if not isinstance(range_id, str) or range_id not in host_body:
                model_dispositions.append(disposition)
                continue
            expected = {"range_id": range_id, "status": "non_item", "item_ids": [],
                        "reason": host_body[range_id]["non_requirement_reason"]}
            if disposition != expected or range_id in seen:
                raise CorrectionValidationError(["invalid_host_disposition"])
            seen.add(range_id)
        corrected["dispositions"] = model_dispositions
    try:
        response = AnalysisExtraction.model_validate(corrected).model_dump(exclude_unset=True, exclude_none=True)
    except ValidationError:
        raise CorrectionValidationError(["correction_schema_invalid"]) from None
    return _normalize_items(pack, response, host_issues)


async def extract_items(pack: dict, model: Model, *, identity: dict, issues=(), scope_decisions=(), role=None) -> dict:
    """One bounded extraction; local model defects continue to independent review."""
    host_reviewed = host_reviewed_ranges(pack)
    if pack.get("planning_details", {}).get("budget", {}).get("fits") is False:
        reason = "Complete extraction input exceeds the admission budget; no model analysis was performed."
        issues = [*issues, issue("extraction_budget_exceeded", reason)]
        result = {"schema_version": 2, "items": [], "relations": [], "dispositions": [
            {"range_id": source["range_id"], "status": "unresolved", "reason": reason, "item_ids": []}
            for source in pack["body"] if source["range_id"] not in host_reviewed
        ], "issues": []}
    elif all(source["range_id"] in host_reviewed for source in pack["body"]):
        result = {"schema_version": 2, "items": [], "dispositions": [], "relations": [], "issues": []}
    else:
        payload = item_payload(pack, identity=identity, issues=issues,
                               scope_decisions=scope_decisions, role=role)
        if hasattr(model, "call_validated"):
            return await model.call_validated("extract", payload,
                                              lambda value: _validate_item_response(pack, value, issues))
        result = await model.call("extract", payload)
    return _validate_item_response(pack, result, issues)


def _validate_item_response(pack, result, issues):
    try:
        return _recover_item_response(pack, result, issues)
    except ValidationError as error:
        raise ModelContractError([{"path": list(entry["loc"]), "code": entry["type"], "detail": entry["msg"]}
                                  for entry in error.errors(include_url=False, include_input=False)]) from None
    except CorrectionValidationError as error:
        raise ModelContractError([{"path": [], "code": code, "detail": "Normalized extraction failed: " + code}
                                  for code in error.codes]) from None
