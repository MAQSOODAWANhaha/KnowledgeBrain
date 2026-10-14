"""Freeze native sources, actual images, literal OCR and indexed work packages."""
from __future__ import annotations

import asyncio
import base64
import inspect
import json
import time
from typing import Callable, Iterator

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.evidence.native import parse_native
from docreader.tender.evidence.packing import make_packs, source_ranges
from docreader.tender.evidence.registration import issue


class EvidenceRepository:
    def __init__(self, artifacts: ArtifactStore):
        self.artifacts = artifacts

    def load_pack(self, pack_ref: dict) -> dict:
        return self.artifacts.read_json(pack_ref.get("ref", pack_ref))

    def iter_packs(self, index_ref: dict) -> Iterator[dict]:
        index = self.artifacts.read_json(index_ref)
        for entry in index["packs"]:
            yield self.load_pack(entry)


async def freeze_sources(registration: dict, artifacts: ArtifactStore, ocr: Callable | None = None,
                         *, range_chars: int = 6000, pack_chars: int = 12000) -> dict:
    started = time.monotonic()
    if range_chars < 1 or pack_chars < 1:
        raise ValueError("source range and pack sizes must be positive")
    documents, pack_refs, problems = [], [], list(registration.get("issues", []))
    range_count = image_count = ocr_count = 0
    for source in registration["documents"]:
        summary = {key: value for key, value in source.items() if key != "issues"}
        summary["issues"] = list(source.get("issues", []))
        documents.append(summary)
        if not source.get("original_ref") or any(problem["code"] == "duplicate_document_id" for problem in summary["issues"]):
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
        raw = {
            "schema_version": 1, "document_id": source["document_id"], "document_version": source["version"],
            "original_ref": source["original_ref"], "units": units, "source_contract": contract,
            "tree": native.tree.model_dump(mode="json") if native.tree else None,
            "content": native.content, "metadata": {key: value for key, value in native.metadata.items()
                                                       if key not in {"source_contract", "document_tree", "image_anchors"}},
            "images": {}, "image_anchors": native.metadata.get("image_anchors", {}), "ocr": {},
        }
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
                                    unit_id=unit["key"], cell=cell["address"])
                    summary["issues"].append(problem)
                    problems.append(problem)
        for ref, payload in native.images.items():
            image_unit = next((unit for unit in units if unit["locator"].get("original_ref") == ref), None)
            media_type = image_unit["locator"].get("media_type", "application/octet-stream") if image_unit else "application/octet-stream"
            raw["images"][ref] = await asyncio.to_thread(artifacts.put_bytes, base64.b64decode(payload, validate=True), media_type)
        for unit in units:
            if unit["kind"] != "image_region":
                continue
            image_count += 1
            image_ref = raw["images"].get(unit["locator"]["original_ref"])
            context = {"document_id": source["document_id"], "unit_id": unit["key"],
                       "locator": unit["locator"], "physical_path": unit["physical_path"],
                       "image_anchor": raw["image_anchors"].get(unit["key"])}
            if image_ref is None:
                result = {"text": "", "status": "unresolved", "issues": ["image_payload_missing"]}
            elif ocr is None:
                result = {"text": "", "status": "unresolved", "issues": ["ocr_provider_unavailable"]}
            else:
                ocr_count += 1
                try:
                    result = ocr(image_ref, context)
                    if inspect.isawaitable(result):
                        result = await result
                    if not isinstance(result, dict) or not isinstance(result.get("text"), str):
                        raise ValueError("invalid literal OCR response")
                    if result.get("status") not in {"complete", "succeeded", "unresolved", "partial"}:
                        raise ValueError("invalid literal OCR status")
                except Exception as exc:
                    # Infrastructure failures are retried by the job layer. Model
                    # semantic failures are document problems, isolated per image.
                    if type(exc).__name__ in {"ModelSemanticError", "ModelConfigurationError"} or isinstance(exc, ValueError):
                        result = {"text": "", "status": "unresolved", "issues": ["ocr_response_invalid"]}
                    else:
                        raise
            raw["ocr"][unit["key"]] = result
            if result.get("status") not in {"complete", "succeeded"} or result.get("issues"):
                reasons = result.get("issues") or ["ocr_unresolved"]
                for reason in reasons:
                    problem = issue(reason.get("code", "ocr_issue") if isinstance(reason, dict) else str(reason),
                                    source["document_id"], unit_id=unit["key"])
                    summary["issues"].append(problem)
                    problems.append(problem)
        raw_ref = await asyncio.to_thread(artifacts.put_json, raw)
        ranges = source_ranges(source, raw, range_chars)
        packs = make_packs(source, ranges, pack_chars)
        for pack in packs:
            summary["issues"].extend(pack["issues"])
            problems.extend(pack["issues"])
            pack["raw_ref"] = raw_ref
            ref = await asyncio.to_thread(artifacts.put_json, pack)
            pack_refs.append({"pack_id": pack["pack_id"], "document_id": source["document_id"],
                              "ref": ref, "range_count": len(pack["body"])})
        if not ranges:
            problem = issue("no_source_ranges", source["document_id"])
            summary["issues"].append(problem)
            problems.append(problem)
        summary.update(raw_ref=raw_ref, range_count=len(ranges), pack_count=len(packs),
                       status="partial" if summary["issues"] else "complete",
                       freeze_seconds=time.monotonic() - parse_started)
        range_count += len(ranges)
    index = {"schema_version": 1, "documents": documents, "packs": pack_refs, "issues": problems,
             "range_count": range_count, "stats": {"document_count": len(documents), "pack_count": len(pack_refs),
                                                    "image_count": image_count, "ocr_request_count": ocr_count,
                                                    "duration_seconds": time.monotonic() - started}}
    return await asyncio.to_thread(artifacts.put_json, index)
