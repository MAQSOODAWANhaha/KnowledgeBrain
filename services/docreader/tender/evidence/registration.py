"""Register the complete submitted manifest before any parser/model work."""
from __future__ import annotations

import json
from copy import deepcopy
from pathlib import Path
from typing import Any

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.evidence.native import canonical_image_keys


def issue(code: str, document_id: str = "", detail: str = "", **extra: Any) -> dict:
    return {"code": code, "document_id": document_id, "detail": detail, "blocking": True, **extra}


def _unique(problems: list[dict]) -> list[dict]:
    return list({json.dumps(problem, sort_keys=True, ensure_ascii=False): problem for problem in problems}.values())


def register_manifest(manifest: dict, artifacts: ArtifactStore) -> dict:
    documents, issues = [], list(manifest.get("issues", []))
    seen_ids, seen_contents = set(), {}
    for ordinal, entry in enumerate(manifest.get("documents", [])):
        document_id = str(entry.get("document_id", ""))
        record = {
            "document_id": document_id,
            "role": entry.get("role", "main"),
            "document_version": str(entry.get("document_version", "")),
            "replaces": entry.get("replaces"),
            "modification_scope": entry.get("modification_scope", entry.get("scope")),
            "ordinal": ordinal,
            "issues": list(entry.get("issues", [])),
        }
        if not document_id or document_id in seen_ids:
            record["issues"].append(issue("duplicate_document_id" if document_id else "missing_document_id", document_id))
        seen_ids.add(document_id)
        if record["role"] not in {"main", "attachment", "clarification", "amendment"}:
            record["issues"].append(issue("invalid_document_role", document_id))
        if not record["document_version"]:
            record["issues"].append(issue("missing_document_version", document_id))
        path = Path(entry.get("path", ""))
        record["file_name"] = entry.get("file_name") or entry.get("filename") or path.name or document_id
        try:
            original_ref = entry.get("original_ref", entry.get("file_ref"))
            if original_ref:
                artifacts.path(original_ref)
                record["original_ref"] = original_ref
            elif manifest.get("schema_version") == 1 and entry.get("issues") and not entry.get("path"):
                pass  # The prior registration already recorded why this source is unavailable.
            elif not entry.get("path") or not path.exists():
                record["issues"].append(issue("missing_document", document_id))
            elif not path.is_file():
                record["issues"].append(issue("unreadable_document", document_id, "not a regular file"))
            else:
                record["original_ref"] = artifacts.import_file(path)
            if record.get("original_ref"):
                digest = record["original_ref"]["sha256"]
                if digest in seen_contents:
                    record["issues"].append(issue("duplicate_document_content", document_id,
                                                  duplicate_of=seen_contents[digest]))
                seen_contents[digest] = document_id
                if record["original_ref"]["size"] == 0:
                    record["issues"].append(issue("empty_document", document_id))
        except (OSError, ValueError) as exc:
            record["issues"].append(issue("unreadable_document", document_id, type(exc).__name__))
        record["issues"] = _unique(record["issues"])
        issues.extend(record["issues"])
        documents.append(record)
    for expected in manifest.get("expected_document_ids", []):
        if expected not in seen_ids:
            issues.append(issue("missing_expected_document", str(expected)))
    if not documents:
        issues.append(issue("empty_document_set"))
    reviews = manifest.get("image_dispositions", [])
    if not isinstance(reviews, list) or any(
        not isinstance(review, dict) or not isinstance(review.get("reviewed_positions"), list)
        or not review["reviewed_positions"]
        or any(not isinstance(position, dict) for position in review["reviewed_positions"])
        for review in reviews
    ):
        issues.append(issue("invalid_image_dispositions"))
        reviews = []
    reviews = deepcopy(reviews)
    try:
        canonical_image_keys(reviews)
    except ValueError:
        issues.append(issue("invalid_image_dispositions"))
        reviews = []
    return {"schema_version": 1, "documents": documents,
            "issues": _unique(issues), **({"image_dispositions": reviews} if reviews else {})}
