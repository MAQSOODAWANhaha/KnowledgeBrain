"""Freeze native sources, actual images, literal OCR and indexed work packages."""
from __future__ import annotations

import asyncio
import base64
import hashlib
import inspect
import json
import time
from typing import Callable

from pydantic import TypeAdapter, ValidationError
from copy import deepcopy

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.request import ModelSemanticError
from docreader.llm.budget import RequestBudget
from docreader.tender.evidence.native import canonical_image_keys, compact_native_snapshot, parse_native
from docreader.tender.evidence.planner import make_packs
from docreader.tender.evidence.references import EvidenceRepository as EvidenceRepository, SourceIndex
from docreader.tender.evidence.registration import issue
from docreader.tender.schema import OCRStatus, PackDescriptor, PackPlan, SourceDocument

_OCR_STATUS = TypeAdapter(OCRStatus)


def _image_review(reviews: list, source: dict, unit: dict, image_ref: dict | None):
    """A reviewed visual decision is exact-source/occurrence evidence, not a heuristic."""
    for review in reviews:
        for position in review.get("reviewed_positions", []):
            if position.get("document_id") != source["document_id"] or position.get("unit_id") != unit["unit_id"]:
                continue
            valid = (
                image_ref is not None and review.get("source_sha256") == source["original_ref"]["sha256"]
                and review.get("sha256") == image_ref["sha256"] and review.get("classification") == "decorative"
                and review.get("status") == "reviewed" and bool(review.get("reason_code")) and bool(review.get("rationale"))
                and position.get("image_key") == unit["locator"].get("image_key")
                and position.get("page_ordinal") == unit["locator"].get("page_ordinal")
                and isinstance(position.get("locator"), dict)
                and all(unit["locator"].get(key) == value for key, value in position["locator"].items())
            )
            return (review, None) if valid else (None, issue("image_disposition_identity_mismatch", source["document_id"], unit_id=unit["unit_id"]))
    return None, None


async def freeze_sources(registration: dict, artifacts: ArtifactStore, ocr: Callable | None = None,
                         *, budget: RequestBudget | None = None, plan: bool = True) -> dict:
    started = time.monotonic()
    documents, problems = [], list(registration.get("issues", []))
    native_range_count = image_count = ocr_count = 0
    for source in registration["documents"]:
        summary = {key: value for key, value in source.items() if key != "issues"}
        summary["issues"] = list(source.get("issues", []))
        documents.append(summary)
        if not source.get("original_ref") or any(problem["code"] in {
            "duplicate_document_id", "missing_document_id", "missing_document_version"
        } for problem in summary["issues"]):
            summary["status"] = "unresolved"
            continue
        parse_started = time.monotonic()
        # Storage corruption/unavailability is a technical failure, not a document defect.
        content = await asyncio.to_thread(artifacts.read_bytes, source["original_ref"])
        try:
            # File I/O and all CPU/blocking native parsing stay off the event loop.
            native = await asyncio.to_thread(parse_native, source["file_name"], content)
        except (ValueError, OSError, RuntimeError, KeyError) as exc:
            problem = issue("unsupported_document" if "Unsupported file type" in str(exc) else "document_parse_failed",
                            source["document_id"], type(exc).__name__)
            summary["issues"].append(problem)
            problems.append(problem)
            summary.update(status="unresolved", parse_seconds=time.monotonic() - parse_started)
            continue
        summary["parse_seconds"] = time.monotonic() - parse_started
        contract = json.loads(native.metadata["source_contract"])
        units = [unit.model_dump(mode="json") for unit in native.structured_source_units]
        for unit in units:
            unit["unit_id"] = unit.pop("key")
        raw = {
            "schema_version": 2, "document_id": source["document_id"], "document_version": source["document_version"],
            "original_ref": source["original_ref"], "units": units, "source_contract": contract,
            "tree": native.tree.model_dump(mode="json") if native.tree else None,
            "metadata": {key: native.metadata[key] for key in (
                "page_count", "page_classifications", "format", "parser_version", "embedded_image_count",
                "vector_figure_count", "evidence_issues") if key in native.metadata},
            "images": {}, "image_anchors": native.metadata.get("image_anchors", {}), "ocr": {}, "image_reviews": {},
        }
        canonical_image_keys(raw)
        for identity in contract.get("units", []):
            for reason in identity.get("reasons", []):
                problem = issue(reason, source["document_id"], unit_id=identity["unit_id"])
                summary["issues"].append(problem)
                problems.append(problem)
        for problem in native.metadata.get("evidence_issues", []):
            problem = {"document_id": source["document_id"], "blocking": True, **problem}
            summary["issues"].append(problem)
            problems.append(problem)
        incomplete_cells = set()
        for unit in units:
            for cell in unit["locator"].get("cells", []):
                reason = cell.get("display_incomplete_reason")
                key = (unit["locator"].get("sheet_ordinal"), cell["address"], reason)
                if reason and key not in incomplete_cells:
                    incomplete_cells.add(key)
                    problem = issue("spreadsheet_display_incomplete", source["document_id"], reason,
                                    unit_id=unit["unit_id"], cell=cell["address"])
                    summary["issues"].append(problem)
                    problems.append(problem)
        for ref, payload in native.images.items():
            image_unit = next((unit for unit in units if unit["locator"].get("image_key") == ref), None)
            media_type = image_unit["locator"].get("media_type", "application/octet-stream") if image_unit else "application/octet-stream"
            raw["images"][ref] = await asyncio.to_thread(artifacts.put_bytes, base64.b64decode(payload, validate=True), media_type)
        for unit in units:
            if unit["kind"] != "image_region":
                continue
            image_count += 1
            image_ref = raw["images"].get(unit["locator"]["image_key"])
            context = {"document_id": source["document_id"], "unit_id": unit["unit_id"],
                       "locator": unit["locator"], "physical_path": unit["physical_path"],
                       "image_anchor": raw["image_anchors"].get(unit["unit_id"])}
            review, review_problem = _image_review(registration.get("image_dispositions", []), source, unit, image_ref)
            if review_problem:
                summary["issues"].append(review_problem)
                problems.append(review_problem)
            if review is not None:
                review_ref = await asyncio.to_thread(artifacts.put_json, review)
                raw["image_reviews"][review_ref["sha256"]] = review_ref
                result = {"text": "", "status": "complete", "issues": [], "disposition": {
                    "status": "reviewed", "classification": "decorative", "reason_code": review["reason_code"],
                    "review_ref": review_ref, "source_sha256": source["original_ref"]["sha256"], "sha256": image_ref["sha256"]}}
            elif image_ref is None:
                result = {"text": "", "status": "unresolved", "issues": ["image_payload_missing"]}
            elif ocr is None:
                result = {"text": "", "status": "unresolved", "issues": ["ocr_provider_unavailable"]}
            else:
                ocr_count += 1
                try:
                    result = ocr(image_ref, context)
                    if inspect.isawaitable(result):
                        result = await result
                except ModelSemanticError:
                    result = {"text": "", "status": "unresolved", "issues": ["ocr_response_invalid"]}
                # Only a response contract violation is a document/model issue.
                # Storage integrity, configuration and other technical exceptions
                # from provider/cached-receipt reads must propagate unchanged.
                if (not isinstance(result, dict) or not isinstance(result.get("text"), str)
                        or not isinstance(result.get("issues", []), list)):
                    result = {"text": "", "status": "unresolved", "issues": ["ocr_response_invalid"]}
            try:
                _OCR_STATUS.validate_python(result.get("status"), strict=True)
            except ValidationError:
                result = {"text": "", "status": "unresolved", "issues": ["ocr_response_invalid"]}
            raw["ocr"][unit["unit_id"]] = result
            if result.get("status") != "complete" or result.get("issues"):
                reasons = result.get("issues") or ["ocr_unresolved"]
                for reason in reasons:
                    problem = issue(reason.get("code", "ocr_issue") if isinstance(reason, dict) else str(reason),
                                    source["document_id"], unit_id=unit["unit_id"])
                    summary["issues"].append(problem)
                    problems.append(problem)
        compact_native_snapshot(raw)
        raw_ref = await asyncio.to_thread(artifacts.put_json, raw)
        source_index = await asyncio.to_thread(SourceIndex, source, raw)
        source_index_ref = await asyncio.to_thread(artifacts.put_json, source_index.descriptor(raw_ref))
        if not source_index.ranges:
            problem = issue("no_source_ranges", source["document_id"])
            summary["issues"].append(problem)
            problems.append(problem)
        summary.update(raw_ref=raw_ref, source_index_ref=source_index_ref,
                       native_range_count=len(source_index.ranges),
                       status="partial" if summary["issues"] else "complete",
                       freeze_seconds=time.monotonic() - parse_started)
        native_range_count += len(source_index.ranges)
    index = {"schema_version": 2, "documents": documents, "issues": problems,
             "native_range_count": native_range_count,
             "stats": {"document_count": len(documents),
                                                    "native_range_count": native_range_count,
                                                    "image_count": image_count, "ocr_request_count": ocr_count,
                                                    "duration_seconds": time.monotonic() - started}}
    for document in documents:
        SourceDocument.model_validate(document)
    frozen_ref = await asyncio.to_thread(artifacts.put_json, index)
    return await plan_sources(frozen_ref, artifacts, budget=budget) if plan else frozen_ref


def _digest(value) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()).hexdigest()


async def plan_sources(frozen_index: dict, artifacts: ArtifactStore, budget: RequestBudget | None = None,
                       scope_decisions: list | None = None, run_identity: dict | None = None,
                       *, payload_builder: Callable[[dict], dict] | None = None, analysis: bool = False) -> dict:
    """A domain-revision plan over immutable sources; never reparse original files."""
    budget = budget or RequestBudget(context_tokens=131072)
    decisions = deepcopy(scope_decisions or [])
    if "documents" in frozen_index:
        frozen = frozen_index
        frozen_ref = await asyncio.to_thread(artifacts.put_json, frozen)
    else:
        frozen_ref = frozen_index
        frozen = await asyncio.to_thread(artifacts.read_json, frozen_ref)
    budget_spec_ref = await asyncio.to_thread(artifacts.put_json, budget.spec())
    budget_digest = _digest(budget.spec())
    input_digest = _digest({"sources": [
        {key: document[key] for key in ("document_id", "document_version", "source_index_ref") if key in document}
        for document in frozen["documents"]], "scope_decisions": decisions})
    repository = EvidenceRepository(artifacts)
    documents, packs, issues, probes = deepcopy(frozen["documents"]), [], list(frozen["issues"]), 0
    payload_digests = []
    for document in documents:
        if "source_index_ref" not in document:
            continue
        source = await asyncio.to_thread(repository.source_index, document["source_index_ref"])
        planned, stats = await asyncio.to_thread(make_packs, source, budget, decisions,
                                                 payload_builder=payload_builder, analysis=analysis)
        probes += stats["probe_count"]
        for pack in planned:
            pack.update(raw_ref=document["raw_ref"], source_index_ref=document["source_index_ref"])
            pack["planning_details"]["budget_spec_ref"] = budget_spec_ref
            if payload_builder is not None:
                materialized = {**pack, "body": [source.materialize(item) for item in pack["body"]],
                                "supporting": [source.materialize(item) for item in pack["supporting"]]}
                payload_digests.append({"pack_id": pack["pack_id"], "payload_digest": _digest(payload_builder(materialized))})
            document["issues"].extend(pack["issues"])
            issues.extend(pack["issues"])
        document.update(range_count=sum(len(pack["body"]) for pack in planned), pack_count=len(planned),
                        plan_stats=stats, status="partial" if document["issues"] else "complete")
        document["issues"] = list({_digest(value): value for value in document["issues"]}.values())
        packs.extend(planned)
    if payload_builder is not None:
        input_digest = _digest({"source_input_digest": input_digest, "request_payloads": payload_digests})
    plan_digest = _digest({"task": "extract", "input_digest": input_digest, "budget_digest": budget_digest,
                           "packs": packs, "scope_decisions": decisions})
    pack_refs = []
    for pack in packs:
        pack.update(task="extract", input_digest=input_digest, budget_digest=budget_digest, plan_digest=plan_digest)
        PackDescriptor.model_validate(pack)
        artifact_ref = await asyncio.to_thread(artifacts.put_json, pack)
        pack_refs.append({"pack_id": pack["pack_id"], "document_id": pack["document_id"],
                          "artifact_ref": artifact_ref, "range_count": len(pack["body"]), "plan_digest": plan_digest})
    result = {"schema_version": 2, "task": "extract", "input_digest": input_digest,
              "budget_digest": budget_digest, "plan_digest": plan_digest, "frozen_ref": frozen_ref,
              "documents": documents, "packs": pack_refs, "issues": list({_digest(value): value for value in issues}.values()),
              "scope_decisions": decisions, "budget_spec_ref": budget_spec_ref,
              "range_count": sum(pack["range_count"] for pack in pack_refs),
              "native_range_count": frozen["native_range_count"],
              "stats": {**frozen["stats"], "pack_count": len(pack_refs), "probe_count": probes}}
    if run_identity is not None:
        result["run_identity"] = deepcopy(run_identity)
    PackPlan.model_validate(result)
    return await asyncio.to_thread(artifacts.put_json, result)
