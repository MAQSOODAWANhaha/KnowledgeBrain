"""One extraction request per frozen pack, with complete body dispositions."""

from collections import Counter

from pydantic import ValidationError

from docreader.llm import ModelSemanticError
from docreader.tender.requirements.schema import Extraction, Model, issue, source_ranges


def _unresolved(source: dict, reason: str) -> dict:
    return {
        "range_id": source["range_id"], "status": "unresolved",
        "reason": reason, "requirement_ids": [],
    }


async def extract_pack(pack: dict, model: Model) -> dict:
    body = {source["range_id"]: source for source in pack["body"]}
    blank = {
        key for key, source in body.items()
        if source.get("empty") is True and source.get("non_requirement_reason") in {"empty_table_row", "blank_page"}
        and not source["text"].strip()
    }
    issues = list(pack.get("issues", []))
    if len(body) != len(pack["body"]):
        issues.append(issue("duplicate_body_range", "A body range occurs more than once in this pack."))
    try:
        if set(body) == blank:
            response = Extraction(requirements=[], dispositions=[])
        else:
            response = Extraction.model_validate(await model.call("extract", {
                "pack_id": pack["pack_id"],
                "body": [source for source in pack["body"] if source["range_id"] not in blank],
                "supporting": pack["supporting"],
            }))
        result = response.model_dump(exclude_none=True)
        issues.extend(result.pop("issues"))
    except (ModelSemanticError, ValidationError) as exc:
        # Do not expose a raw model response (or SDK error text) in diagnostics.
        result = {"requirements": [], "dispositions": []}
        issues.append(issue("extraction_semantic_failure", f"Extraction contract rejected ({type(exc).__name__})."))

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
            if invalid:
                disposition = _unresolved(source, "Disposition does not agree with evidence-backed requirements.")
                issues.append(issue("invalid_disposition", disposition["reason"], range_id))
        if disposition["status"] == "unresolved":
            issues.append(issue("unresolved_source_range", disposition["reason"], range_id))
        dispositions.append(disposition)
    return {
        "pack_id": pack["pack_id"], "requirements": result["requirements"],
        "dispositions": dispositions, "issues": issues, "source_ranges": source_ranges(pack),
    }
