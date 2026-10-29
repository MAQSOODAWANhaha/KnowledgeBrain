"""Small cross-pack/version rules over extraction metadata, without rescanning files."""

import copy
import hashlib
import json
import re
from collections import Counter, defaultdict

from docreader.tender.checks.verify import NUMBER, numeric_values
from docreader.tender.schema import ApplicabilityDecision, enforce_issue_policy, issue


def _targets(replaces):
    if isinstance(replaces, dict):
        return [replaces]
    if isinstance(replaces, list):
        return replaces
    return []


def _apply_amendments(documents: list[dict], requirements: list[dict], sources: dict) -> tuple[list[dict], dict]:
    findings = []
    overrides = {}
    by_document = {str(document["document_id"]): document for document in documents}
    edges = {
        document["document_id"]: {target["document_id"] for target in _targets(document.get("replaces"))
                                  if isinstance(target, dict) and isinstance(target.get("document_id"), str)}
        for document in documents
    }
    cyclic = set()
    visited = set()

    def visit(document_id, path):
        if document_id in path:
            cyclic.update(path[path.index(document_id):])
            return
        if document_id in visited:
            return
        for original in edges.get(document_id, set()):
            visit(original, [*path, document_id])
        visited.add(document_id)

    for document_id in edges:
        visit(document_id, [])
    if cyclic:
        findings.append(issue("cyclic_amendments", "Modification targets form a cycle; applicability cannot be resolved.", document_ids=sorted(cyclic)))
    changes = defaultdict(list)
    for document in documents:
        document_id = document["document_id"]
        if document_id in cyclic:
            continue
        replaces = document.get("replaces")
        if document.get("role") != "amendment" and not replaces:
            continue
        targets = _targets(replaces)
        if not targets:
            findings.append(issue("amendment_target_unresolved", "Amendment must identify its original document and exact modified range/scope.", document_id=document_id))
            continue
        for target in targets:
            if not isinstance(target, dict) or not isinstance(target.get("document_id"), str) or not target["document_id"]:
                findings.append(issue("amendment_target_unresolved", "Modification target must include document_id and range_id or scope.", document_id=document_id))
                continue
            original_document_id = target["document_id"]
            original_document = by_document.get(original_document_id)
            if not original_document or original_document_id == document_id:
                findings.append(issue("amendment_document_unresolved", "Modification target is missing or refers to the amendment itself.", document_id=document_id))
                continue
            range_id = target.get("range_id")
            if not range_id and target.get("scope"):
                # Scope is resolved only against an exact stable range identity. Natural-language
                # interpretations require a later explicit source revision, never a silent guess.
                range_id = target["scope"] if isinstance(target["scope"], str) else None
            source = sources.get(range_id) if isinstance(range_id, str) else None
            if not source or source["document_id"] != original_document_id:
                findings.append(issue("amendment_scope_unresolved", "Modification scope does not uniquely identify a frozen original range.", document_id=document_id))
                continue
            changes[range_id].append(document_id)
    for range_id, replacement_documents in changes.items():
        if len(set(replacement_documents)) != 1:
            findings.append(issue("competing_amendments", "Multiple amendments target the same original range; no date-based precedence is assumed.", range_id))
            continue
        replacement_document = replacement_documents[0]
        original_requirements = [
            requirement for requirement in requirements
            if any(evidence["range_id"] == range_id for evidence in requirement["evidence"])
        ]
        replacement_requirements = [
            requirement for requirement in requirements
            if any(sources.get(evidence["range_id"], {}).get("document_id") == replacement_document for evidence in requirement["evidence"])
        ]
        if not replacement_requirements:
            findings.append(issue("amendment_replacement_unresolved", "No evidence-backed replacement requirement was extracted from the modifying document.", range_id))
            continue
        for requirement in original_requirements:
            # A requirement spanning other body ranges cannot safely be deactivated wholesale.
            body_evidence = [e for e in requirement["evidence"] if sources.get(e["range_id"], {}).get("body")]
            if any(e["range_id"] != range_id for e in body_evidence):
                findings.append(issue("amendment_partial_requirement", "The modified requirement also spans unaffected body ranges; split it before replacing it.", range_id, requirement_id=requirement["requirement_id"]))
                continue
            overrides[requirement["requirement_id"]] = ApplicabilityDecision.model_validate({
                "requirement_id": requirement["requirement_id"],
                "applicable": False,
                "applicability_basis": f"Explicit amendment {replacement_document} replaces {sources[range_id]['document_id']}:{range_id}.",
                "amendment_document_id": replacement_document,
                "source_document_id": sources[range_id]["document_id"],
                "source_range_id": range_id,
                "evidence": [evidence for evidence in requirement["evidence"] if evidence["range_id"] == range_id],
            }).model_dump(mode="json", exclude_none=True)
    return findings, overrides


def apply_applicability_decisions(requirements: list[dict], decisions: dict) -> list[dict]:
    """Read original requirements with verified applicability, without copying evidence."""
    result = []
    for requirement in requirements:
        decision = decisions.get(requirement["requirement_id"])
        if decision:
            decision = ApplicabilityDecision.model_validate(decision).model_dump(mode="json", exclude_none=True)
            if decision["requirement_id"] != requirement["requirement_id"]:
                raise ValueError("applicability_decision_identity_mismatch")
        result.append({**requirement, **{field: decision[field] for field in ("applicable", "applicability_basis")}}
                      if decision else requirement)
    return result


def _requirements_digest(requirements: list[dict]) -> str:
    digest = hashlib.sha256()
    encoder = json.JSONEncoder(sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
    for fragment in encoder.iterencode(requirements):
        digest.update(fragment.encode("utf-8"))
    return digest.hexdigest()


def _conflicts(requirements: list[dict], sources: dict) -> list[dict]:
    groups = defaultdict(list)
    for requirement in requirements:
        if not requirement["applicable"]:
            continue
        key = (
            requirement["subject"], tuple(sorted(requirement["conditions"])), tuple(sorted(requirement["exceptions"])),
            re.sub(r"\s+", "", NUMBER.sub("<number>", requirement["text"])),
        )
        groups[key].append(requirement)
    findings = []
    for group in groups.values():
        for index, first in enumerate(group):
            first_documents = {sources.get(e["range_id"], {}).get("document_id") for e in first["evidence"]}
            for second in group[index + 1:]:
                second_documents = {sources.get(e["range_id"], {}).get("document_id") for e in second["evidence"]}
                if first_documents != second_documents and numeric_values(first["text"]) != numeric_values(second["text"]):
                    findings.append(issue("potential_cross_document_conflict", "Same subject, conditions and wording have differing numeric obligations across knowledge_documents.", requirement_ids=[first["requirement_id"], second["requirement_id"]]))
    return findings


def verify_all(index: dict, extractions: list[dict], verifications: list[dict],
               requirements_ref: dict | None = None, *, verification_packs: list[dict] | None = None) -> dict:
    issues = copy.deepcopy(index.get("issues", []))
    requirements = []
    sources = {}
    body_ids = []
    extraction_ids = []
    for extraction in extractions:
        extraction_ids.append(extraction["pack_id"])
        requirements.extend(extraction["requirements"])
        issues.extend(copy.deepcopy(extraction.get("issues", [])))
        body_ids.extend(disposition["range_id"] for disposition in extraction["dispositions"])
        for range_id, metadata in extraction.get("source_ranges", {}).items():
            existing = sources.get(range_id)
            if existing and (existing["document_id"] != metadata["document_id"] or existing["locator"] != metadata["locator"]):
                issues.append(issue("inconsistent_source_identity", "Same frozen range has inconsistent cross-pack metadata.", range_id))
            sources[range_id] = {**metadata, "body": metadata.get("body", False) or bool(existing and existing.get("body"))}
        for disposition in extraction["dispositions"]:
            if disposition["status"] == "unresolved":
                issues.append(issue("unresolved_source_range", disposition["reason"], disposition["range_id"]))
    verification_ids = [verification["pack_id"] for verification in verifications]
    for verification in verifications:
        issues.extend(copy.deepcopy(verification["findings"]))
    frozen_pack_ids = [entry["pack_id"] for entry in index["packs"]]
    if Counter(extraction_ids) != Counter(frozen_pack_ids):
        issues.append(issue("extraction_pack_count_mismatch", "Not every frozen pack has an extraction result."))
    expected_verifications = (extraction_ids if verification_packs is None
                              else [pack["pack_id"] for pack in verification_packs])
    if Counter(expected_verifications) != Counter(verification_ids):
        issues.append(issue("verification_pack_mismatch", "Every verification plan pack needs exactly one independent result."))
    if verification_packs is not None:
        for pack in verification_packs:
            issues.extend(copy.deepcopy(pack["issues"]))
        if (Counter(item["range_id"] for pack in verification_packs for item in pack["body"]) != Counter(body_ids)
                or Counter(ref["range_id"] for pack in verification_packs for ref in pack["disposition_refs"]) != Counter(body_ids)):
            issues.append(issue("verification_coverage_mismatch", "Verification selectors must audit every original source disposition exactly once."))
        if Counter(ref["requirement_id"] for pack in verification_packs for ref in pack["requirement_refs"]) != Counter(r["requirement_id"] for r in requirements):
            issues.append(issue("verification_requirement_mismatch", "Verification selectors must preserve every complete original requirement exactly once."))
    if any(count != 1 for count in Counter(extraction_ids).values()):
        issues.append(issue("duplicate_extraction_pack", "A frozen pack was counted more than once."))
    if len(body_ids) != index["range_count"] or len(set(body_ids)) != len(body_ids):
        issues.append(issue("body_coverage_mismatch", "Body range dispositions must cover the frozen index exactly once."))
    requirement_counts = Counter(requirement["requirement_id"] for requirement in requirements)
    if any(count != 1 for count in requirement_counts.values()):
        issues.append(issue("duplicate_requirement_id", "Requirement identity collides across packs."))
    amendment_issues, overrides = _apply_amendments(index.get("documents", []), requirements, sources)
    issues.extend(amendment_issues)
    effective_requirements = apply_applicability_decisions(requirements, overrides)
    issues.extend(_conflicts(effective_requirements, sources))
    # Also enforce at the durable aggregate boundary, including resumed artifacts.
    issues = [enforce_issue_policy(finding) for finding in issues]
    return {
        "issues": issues, "applicability_decisions": overrides,
        "requirements_digest": _requirements_digest(requirements),
        "requirements_ref": requirements_ref,
        "blocking": any(item.get("blocking", True) for item in issues),
        "counts": {
            "packs": len(extractions), "verified_packs": len(verifications), "ranges": len(set(body_ids)),
            "requirements": len(requirements), "applicable_requirements": sum(requirement["applicable"] for requirement in effective_requirements),
            "blocking_issues": sum(bool(item.get("blocking", True)) for item in issues),
        },
    }
