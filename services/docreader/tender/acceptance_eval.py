"""Offline read-only evaluator for the tender minimum acceptance baseline (W1.3).

Reads an authoring execution's outline, exported DOCX, coverage/requirements/gaps
artifacts and stage report, scores the four criteria in docs/tender/acceptance.md,
and pinpoints every missing requirement and mismatched original table.
It sends no model requests, touches no database, and never marks unverified
human or ONLYOFFICE checks as passed.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
from io import BytesIO
import json
from pathlib import Path
import re
from typing import Any
import xml.etree.ElementTree as ET
from zipfile import BadZipFile, ZipFile

from docx.oxml.ns import qn

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_coverage import build_ledger, rule_ref_key
from docreader.tender.authoring_export import xml_text
from docreader.tender.authoring_templates import load_template_part
from docreader.tender.evidence.references import EvidenceRepository
from docreader.tender.word_navigation import bookmark_name, read_docx_outline


_TEMPLATE_GAP_CODES = frozenset({
    "export_template_missing",
    "export_template_not_editable",
    "export_template_structure_incomplete",
    "export_template_unrendered",
    "export_source_unavailable",
    "export_native_response_table_unconfirmed",
})
_FABRICATION_GAP_CODES = frozenset({
    "unsupported_content",
    "export_manual_content_omitted",
    "export_fixed_cell_preserved",
    "export_unassigned_content",
})
_PENDING_OUTLINE_CODES = frozenset({
    "outline_rule_unassigned",
    "outline_item_placement_failed",
    "outline_directory_requirement_unassigned",
})
_EXCLUDED_OUTLINE_CODES = frozenset({
    "outline_rule_excluded",
    "outline_target_excluded",
})


def _norm(text: str | None) -> str:
    return " ".join(xml_text(text or "").split())


def _element_text(element: Any) -> str:
    return "".join(
        (node.text or "") if node.tag == qn("w:t") else "\t" if node.tag == qn("w:tab") else "\n"
        for node in element.iter()
        if node.tag in {qn("w:t"), qn("w:tab"), qn("w:br"), qn("w:cr")}
    )


def _walk_chapters(chapters: list[dict[str, Any]], external: bool = False):
    for chapter in chapters or []:
        is_external = external or chapter.get("presentation_role") == "submission_note"
        yield chapter, is_external
        yield from _walk_chapters(chapter.get("children", []), is_external)


def _parse_docx_table(tbl_element: Any) -> dict[str, Any]:
    """Extract logical grid geometry, merged spans and cell texts from a Word <w:tbl>."""
    cells: list[dict[str, Any]] = []
    open_vmerge: dict[int, dict[str, Any]] = {}
    row_widths: list[int] = []
    rows = tbl_element.findall(qn("w:tr"))
    for row_index, tr in enumerate(rows):
        col_index = 0
        for tc in tr.findall(qn("w:tc")):
            tc_pr = tc.find(qn("w:tcPr"))
            span_el = tc_pr.find(qn("w:gridSpan")) if tc_pr is not None else None
            span_val = span_el.get(qn("w:val")) if span_el is not None else None
            col_span = int(span_val) if span_val and span_val.isdigit() else 1
            vmerge_el = tc_pr.find(qn("w:vMerge")) if tc_pr is not None else None
            vmerge_val = None if vmerge_el is None else (vmerge_el.get(qn("w:val")) or "continue")
            if vmerge_val == "continue" and col_index in open_vmerge:
                open_vmerge[col_index]["row_span"] += 1
            else:
                text = "\n".join(_element_text(p) for p in tc.findall(qn("w:p")))
                cell = {
                    "row": row_index,
                    "column": col_index,
                    "row_span": 1,
                    "col_span": col_span,
                    "text": text,
                }
                cells.append(cell)
                if vmerge_val == "restart":
                    for offset in range(col_span):
                        open_vmerge[col_index + offset] = cell
                else:
                    for offset in range(col_span):
                        open_vmerge.pop(col_index + offset, None)
            col_index += col_span
        row_widths.append(col_index)
    return {
        "row_count": len(rows),
        "column_count": max(row_widths, default=0),
        "cells": cells,
    }


def _inspect_docx(docx_bytes: bytes | None, outline: dict[str, Any]) -> dict[str, Any]:
    if docx_bytes is None:
        return {
            "present": False,
            "valid": False,
            "docx_sha256": None,
            "error": "docx_missing",
            "missing_chapter_ids": [],
            "missing_target_ids": [],
            "tables_by_target": {},
            "tables_by_chapter": {},
            "texts_by_target": {},
            "texts_by_chapter": {},
            "table_count": 0,
        }
    sha256 = hashlib.sha256(docx_bytes).hexdigest()
    try:
        with ZipFile(BytesIO(docx_bytes)) as archive:
            root = ET.fromstring(archive.read("word/document.xml"))
        nav = read_docx_outline(docx_bytes, outline.get("chapters", []))
    except (BadZipFile, KeyError, ET.ParseError, ValueError) as exc:
        return {
            "present": True,
            "valid": False,
            "docx_sha256": sha256,
            "error": f"{type(exc).__name__}: {exc}",
            "missing_chapter_ids": [],
            "missing_target_ids": [],
            "tables_by_target": {},
            "tables_by_chapter": {},
            "texts_by_target": {},
            "texts_by_chapter": {},
            "table_count": 0,
        }

    chapter_by_bm: dict[str, str] = {}
    target_by_bm: dict[str, tuple[str, str]] = {}
    expected_targets: list[str] = []
    for chapter, is_external in _walk_chapters(outline.get("chapters", [])):
        if is_external:
            continue
        cid = chapter["chapter_id"]
        chapter_by_bm[bookmark_name(cid, "chapter")] = cid
        for target in chapter.get("targets", []):
            tid = target["target_id"]
            target_by_bm[bookmark_name(tid, "target")] = (cid, tid)
            expected_targets.append(tid)

    counts = Counter(node.get(qn("w:name")) for node in root.iter(qn("w:bookmarkStart")))
    unique_bookmarks = set(nav["bookmarks"])
    missing_target_ids = [
        tid for tid in expected_targets if bookmark_name(tid, "target") not in unique_bookmarks
    ]

    tables_by_target: dict[str, list[dict[str, Any]]] = {}
    tables_by_chapter: dict[str, list[dict[str, Any]]] = {}
    texts_by_target: dict[str, list[str]] = {}
    texts_by_chapter: dict[str, list[str]] = {}
    current_chapter: str | None = None
    current_target: str | None = None
    table_index = 0

    def iter_blocks(element: Any):
        for child in element:
            if child.tag in {qn("w:p"), qn("w:tbl")}:
                yield child
            elif child.tag != qn("w:sectPr"):
                yield from iter_blocks(child)

    body = root.find(qn("w:body"))
    if body is not None:
        for block in iter_blocks(body):
            names = [node.get(qn("w:name")) for node in block.iter(qn("w:bookmarkStart"))]
            chapter_hits = [name for name in names if name in chapter_by_bm and counts[name] == 1]
            target_hits = [name for name in names if name in target_by_bm and counts[name] == 1]
            if len(chapter_hits) == 1:
                new_chapter = chapter_by_bm[chapter_hits[0]]
                if new_chapter != current_chapter:
                    current_chapter = new_chapter
                    current_target = None
            if len(target_hits) == 1:
                owner_chapter, current_target = target_by_bm[target_hits[0]]
                current_chapter = owner_chapter
            text = _element_text(block)
            if text.strip():
                if current_chapter is not None:
                    texts_by_chapter.setdefault(current_chapter, []).append(text)
                if current_target is not None:
                    texts_by_target.setdefault(current_target, []).append(text)
            if block.tag == qn("w:tbl"):
                parsed = {"table_index": table_index, **_parse_docx_table(block)}
                table_index += 1
                if current_chapter is not None:
                    tables_by_chapter.setdefault(current_chapter, []).append(parsed)
                if current_target is not None:
                    tables_by_target.setdefault(current_target, []).append(parsed)

    return {
        "present": True,
        "valid": True,
        "docx_sha256": sha256,
        "error": None,
        "missing_chapter_ids": list(nav["missing_chapter_ids"]),
        "missing_target_ids": missing_target_ids,
        "tables_by_target": tables_by_target,
        "tables_by_chapter": tables_by_chapter,
        "texts_by_target": texts_by_target,
        "texts_by_chapter": texts_by_chapter,
        "table_count": table_index,
    }


def _evaluate_requirements(
    outline: dict[str, Any],
    requirements: dict[str, Any] | None,
    docx_info: dict[str, Any],
    human_review: dict[str, Any] | None,
    gaps: list[dict[str, Any]] | None,
) -> dict[str, Any]:
    named_gaps = [issue for issue in (outline.get("authoring_input") or {}).get("issues", [])
                  if issue["code"] in {"missing_expected_document", "source_registration_pending"}]
    reported = {(gap.get("code"), gap.get("detail")) for gap in gaps or []}
    unreported = [issue for issue in named_gaps if (issue["code"], issue["detail"]) not in reported]
    if requirements is None or not isinstance(requirements.get("rules"), list):
        return {
            "status": "fail" if unreported and gaps is not None else "unknown",
            "machine_passed": False,
            "human_verified": False,
            "unreported_source_gaps": unreported,
            "counts": {"total": None, "placed_in_docx": 0, "settled": 0, "pending_in_gaps": 0, "missing": 0},
            "missing_items": [],
            "pending_items": [],
            "detail": "缺少招标要求底稿（requirements），无法以分析事项为分母核对覆盖。",
        }

    rules = requirements["rules"]
    ledger = build_ledger(outline, requirements)
    entries_by_key: dict[str, list[dict[str, Any]]] = {}
    for entry in ledger:
        key = rule_ref_key(entry.get("rule_ref"))
        if key is not None:
            entries_by_key.setdefault(key, []).append(entry)

    target_chapter: dict[str, tuple[str, str]] = {}
    for chapter, is_external in _walk_chapters(outline.get("chapters", [])):
        if is_external:
            continue
        for target in chapter.get("targets", []):
            target_chapter[target["target_id"]] = (chapter["chapter_id"], chapter.get("title", ""))

    missing_chapters = set(docx_info["missing_chapter_ids"])
    missing_targets = set(docx_info["missing_target_ids"])
    outline_pending_keys = {
        key
        for issue in outline.get("issues", [])
        if issue.get("code") in _PENDING_OUTLINE_CODES
        for ref in issue.get("rule_refs", [])
        if (key := rule_ref_key(ref)) is not None
    }
    outline_excluded_keys = {
        key
        for issue in outline.get("issues", [])
        if issue.get("code") in _EXCLUDED_OUTLINE_CODES
        for ref in issue.get("rule_refs", [])
        if (key := rule_ref_key(ref)) is not None
    }
    placed_in_docx = 0
    settled = 0
    missing_items: list[dict[str, Any]] = []
    pending_items: list[dict[str, Any]] = []

    for row in rules:
        ref = row.get("rule_ref")
        key = rule_ref_key(ref)
        rule = row.get("rule") or {}
        item_id = rule.get("item_id") or (ref or {}).get("item_id")
        snippet = _norm(rule.get("text"))[:120]
        matched = entries_by_key.get(key, []) if key is not None else []
        placed_targets = [
            entry["target_id"]
            for entry in matched
            if entry.get("target_id") in target_chapter
        ]
        if placed_targets:
            present_target = next(
                (
                    tid for tid in placed_targets
                    if target_chapter[tid][0] not in missing_chapters and tid not in missing_targets
                ),
                None,
            )
            if present_target is not None:
                placed_in_docx += 1
            else:
                tid = placed_targets[0]
                cid, title = target_chapter[tid]
                reason = "chapter_missing_in_docx" if cid in missing_chapters else "target_missing_in_docx"
                missing_items.append({
                    "item_id": item_id,
                    "rule_ref": ref,
                    "chapter_id": cid,
                    "chapter_title": title,
                    "target_id": tid,
                    "reason": reason,
                    "text": snippet,
                })
            continue

        resolutions = {entry.get("resolution") for entry in matched}
        if resolutions & {"constraint", "excluded", "submission"} or (key in outline_excluded_keys):
            settled += 1
            continue

        record = {
            "item_id": item_id,
            "rule_ref": ref,
            "chapter_id": None,
            "chapter_title": None,
            "target_id": None,
            "text": snippet,
        }
        if key in outline_pending_keys:
            pending_items.append({**record, "reason": "explicitly_pending_in_gaps"})
        else:
            missing_items.append({**record, "reason": "unplaced_and_unlisted_in_gaps"})

    machine_passed = not missing_items and not unreported
    spot = (human_review or {}).get("requirements_spot_check") or {}
    human_verified = bool(
        spot.get("passed") is True
        and type(spot.get("checked_clauses")) is int
        and spot["checked_clauses"] >= 3
        and spot.get("double_omissions") == 0
    )
    if missing_items or (unreported and gaps is not None):
        status = "fail"
    elif unreported:
        status = "unknown"  # The delivery gap list was not supplied.
    else:
        status = "pass" if human_verified else "needs_human_check"
    return {
        "status": status,
        "machine_passed": machine_passed,
        "human_verified": human_verified,
        "unreported_source_gaps": unreported,
        "counts": {
            "total": len(rules),
            "placed_in_docx": placed_in_docx,
            "settled": settled,
            "pending_in_gaps": len(pending_items),
            "missing": len(missing_items),
        },
        "missing_items": missing_items,
        "pending_items": pending_items,
    }


def _grid_merges(grid: dict[str, Any], *, max_row: int | None = None) -> list[list[int]]:
    return sorted(
        [cell["row"], cell["column"], cell.get("row_span", 1), cell.get("col_span", 1)]
        for cell in grid.get("cells", [])
        if (cell.get("row_span", 1) > 1 or cell.get("col_span", 1) > 1)
        and (max_row is None or cell["row"] < max_row)
    )


def _compare_table(
    expected: dict[str, Any],
    actual: dict[str, Any],
    location: dict[str, Any],
    *,
    header_rows: int = 1,
    expanded: bool = False,
) -> list[dict[str, Any]]:
    mismatches: list[dict[str, Any]] = []
    if actual["column_count"] != expected["column_count"]:
        mismatches.append({
            **location,
            "table_index": actual["table_index"],
            "kind": "column_count_mismatch",
            "expected": expected["column_count"],
            "actual": actual["column_count"],
        })
    row_ok = (
        actual["row_count"] >= expected["row_count"]
        if expanded
        else actual["row_count"] == expected["row_count"]
    )
    if not row_ok:
        mismatches.append({
            **location,
            "table_index": actual["table_index"],
            "kind": "row_count_mismatch",
            "expected": expected["row_count"],
            "actual": actual["row_count"],
        })

    header_limit = max(1, header_rows) if expected["row_count"] > 0 else 0
    actual_by_pos = {(c["row"], c["column"]): _norm(c["text"]) for c in actual["cells"]}

    exp_headers = [
        (c["row"], c["column"], _norm(c.get("text")))
        for c in expected.get("cells", [])
        if c["row"] < header_limit and _norm(c.get("text"))
    ]
    bad_headers = [
        {"row": r, "column": c, "expected": text, "actual": actual_by_pos.get((r, c), "")}
        for r, c, text in exp_headers
        if text not in actual_by_pos.get((r, c), "")
    ]
    if bad_headers:
        mismatches.append({
            **location,
            "table_index": actual["table_index"],
            "kind": "header_mismatch",
            "expected": bad_headers[0]["expected"],
            "actual": bad_headers[0]["actual"],
            "cells": bad_headers,
        })

    merge_limit = header_limit if expanded else None
    exp_merges = _grid_merges(expected, max_row=merge_limit)
    act_merges = _grid_merges(actual, max_row=merge_limit)
    if exp_merges != act_merges:
        mismatches.append({
            **location,
            "table_index": actual["table_index"],
            "kind": "merge_mismatch",
            "expected": exp_merges,
            "actual": act_merges,
        })

    fixed_limit = header_limit if expanded else expected["row_count"]
    bad_fixed = []
    for cell in expected.get("cells", []):
        r, c = cell["row"], cell["column"]
        if r >= fixed_limit or r < header_limit:
            continue
        text = _norm(cell.get("text"))
        if not text:
            continue
        actual_text = actual_by_pos.get((r, c), "")
        if text not in actual_text:
            bad_fixed.append({"row": r, "column": c, "expected": text, "actual": actual_text})
    if bad_fixed:
        mismatches.append({
            **location,
            "table_index": actual["table_index"],
            "kind": "fixed_text_mismatch",
            "expected": bad_fixed[0]["expected"],
            "actual": bad_fixed[0]["actual"],
            "cells": bad_fixed,
        })
    return mismatches


def _required_grid_inventory(manifest, outline, repository):
    """Resolve independent source keys against the evaluated freeze, before visiting its outline."""
    if manifest is None:
        return "missing", set()
    if not isinstance(manifest, dict) or type(manifest.get("complete")) is not bool:
        return "invalid", set()
    tables = manifest.get("tables")
    frozen_ref = (outline.get("authoring_input") or {}).get("frozen_ref")
    if (not isinstance(tables, list) or not tables or not frozen_ref or repository is None
            or manifest.get("frozen_sha256") != frozen_ref["sha256"]):
        return "invalid", set()
    try:
        expected = {(row["source_index_sha256"], row["unit_id"]) for row in tables}
        if len(expected) != len(tables):
            return "invalid", set()
        wanted_indices = {sha for sha, _ in expected}
        frozen = repository.artifacts.read_json(frozen_ref)
        known = set()
        for document in frozen["documents"]:
            index_ref = document["source_index_ref"]
            if index_ref["sha256"] in wanted_indices:
                units = repository.source_index(index_ref).raw["units"]
                known.update((index_ref["sha256"], unit["unit_id"]) for unit in units if unit.get("grid"))
        if not expected <= known:
            return "invalid", set()
    except (KeyError, TypeError, ValueError, OSError):
        return "invalid", set()
    return ("complete" if manifest["complete"] else "partial"), expected


def _evaluate_original_tables(
    outline: dict[str, Any],
    gaps: list[dict[str, Any]] | None,
    docx_info: dict[str, Any],
    artifacts: ArtifactStore | None,
    expected_tables: dict[str, list[dict[str, Any]]] | None = None,
    required_tables: dict[str, Any] | None = None,
) -> dict[str, Any]:
    mismatches: list[dict[str, Any]] = []
    chapter_titles = {
        chapter["chapter_id"]: chapter.get("title", "")
        for chapter, _ in _walk_chapters(outline.get("chapters", []))
    }
    for gap in gaps or []:
        if gap.get("code") in _TEMPLATE_GAP_CODES:
            cid = gap.get("chapter_id")
            mismatches.append({
                "chapter_id": cid,
                "chapter_title": chapter_titles.get(cid, "") if cid else "",
                "target_id": gap.get("target_id"),
                "table_index": None,
                "kind": "export_gap",
                "code": gap["code"],
                "detail": gap.get("detail", ""),
            })

    repository = EvidenceRepository(artifacts) if artifacts is not None else None
    inventory_status, missing_grids = _required_grid_inventory(required_tables, outline, repository)
    form_targets = 0
    checked_tables = 0
    checked_fixed_texts = 0
    used_tables: set[int] = set()
    for chapter, is_external in _walk_chapters(outline.get("chapters", [])):
        if is_external:
            continue
        cid, title = chapter["chapter_id"], chapter.get("title", "")
        for target in chapter.get("targets", []):
            tid = target["target_id"]
            parts = target.get("template_refs") or []
            if target.get("kind") != "form" and not parts:
                continue
            form_targets += 1
            location = {"chapter_id": cid, "chapter_title": title, "target_id": tid}
            expanded = (target.get("response_table") or {}).get("native_layout") is not None

            target_grids: list[tuple[dict[str, Any], int, tuple[str, str] | None]] = []
            target_fixed_lines: list[str] = []
            if required_tables is None and expected_tables is not None and tid in expected_tables:
                for spec in expected_tables[tid]:
                    if "grid" in spec:
                        target_grids.append((spec["grid"], spec.get("header_row_count", 1), None))
                    for line in spec.get("fixed_texts", []):
                        if _norm(line):
                            target_fixed_lines.append(_norm(line))
            elif artifacts is not None:
                for part in parts:
                    try:
                        loaded = load_template_part(artifacts, part, repository=repository)
                    except Exception as exc:
                        mismatches.append({
                            **location,
                            "table_index": None,
                            "kind": "source_template_unavailable",
                            "detail": type(exc).__name__,
                        })
                        continue
                    unit = loaded["unit"]
                    if unit.get("grid"):
                        header_count = part.get("header_row_count")
                        source = part["source_ref"]
                        identity = (source["source_index_ref"]["sha256"], source["unit_id"])
                        target_grids.append((unit["grid"], header_count if isinstance(header_count, int) else 1, identity))
                    elif unit.get("text") and not unit.get("running_header"):
                        for line in unit["text"].splitlines():
                            cleaned = _norm(line)
                            if cleaned:
                                target_fixed_lines.append(cleaned)

            if not docx_info["valid"]:
                continue

            candidates = [
                tbl for tbl in docx_info["tables_by_target"].get(tid, [])
                if tbl["table_index"] not in used_tables
            ]
            if not candidates and not any(source_key in missing_grids for _, _, source_key in target_grids):
                candidates = [
                    tbl for tbl in docx_info["tables_by_chapter"].get(cid, [])
                    if tbl["table_index"] not in used_tables
                ]
            for expected_grid, header_rows, source_key in target_grids:
                if not candidates:
                    mismatches.append({
                        **location,
                        "table_index": None,
                        "kind": "table_missing_in_docx",
                        "expected": {
                            "row_count": expected_grid["row_count"],
                            "column_count": expected_grid["column_count"],
                        },
                        "actual": None,
                    })
                    continue
                actual_table = candidates.pop(0)
                checked_tables += 1
                used_tables.add(actual_table["table_index"])
                if source_key is not None:
                    missing_grids.discard(source_key)
                mismatches.extend(
                    _compare_table(expected_grid, actual_table, location,
                                   header_rows=header_rows, expanded=expanded)
                )

            if target_fixed_lines:
                haystack = _norm("\n".join(
                    docx_info["texts_by_target"].get(tid) or docx_info["texts_by_chapter"].get(cid, [])
                ))
                for line in target_fixed_lines:
                    checked_fixed_texts += 1
                    if line not in haystack:
                        mismatches.append({
                            **location,
                            "table_index": None,
                            "kind": "fixed_text_missing",
                            "expected": line,
                            "actual": haystack[:160],
                        })

    mismatches.extend({"kind": "required_table_missing", "source_index_sha256": sha, "unit_id": unit}
                      for sha, unit in sorted(missing_grids))
    if not docx_info["valid"] or mismatches or not outline.get("chapters"):
        status, machine_passed = "fail", False
    elif not checked_tables or inventory_status != "complete":
        status, machine_passed = "unknown", False
    else:
        status, machine_passed = "pass", True
    return {
        "status": status,
        "machine_passed": machine_passed,
        "inventory_status": inventory_status,
        "grid_comparison": "compared" if checked_tables else "unavailable",
        "counts": {
            "form_targets": form_targets,
            "checked_tables": checked_tables,
            "checked_fixed_texts": checked_fixed_texts,
            "docx_tables": docx_info["table_count"],
            "mismatches": len(mismatches),
        },
        "mismatches": mismatches,
    }


_ADOPTED_REMOVAL_MARKER = "复核明确否定的内容未采用"


def _removed_by_review_scopes(gaps: list[dict[str, Any]] | None) -> tuple[set[str], set[tuple[str | None, str | None]]]:
    task_ids: set[str] = set()
    locations: set[tuple[str | None, str | None]] = set()
    for gap in gaps or []:
        if gap.get("code") == "writing_gap" and _ADOPTED_REMOVAL_MARKER in (gap.get("detail") or ""):
            if gap.get("task_id"):
                task_ids.add(str(gap["task_id"]))
            if gap.get("chapter_id") or gap.get("target_id"):
                locations.add((gap.get("chapter_id"), gap.get("target_id")))
    return task_ids, locations


def _evaluate_no_fabrication(
    outline: dict[str, Any],
    coverage: dict[str, Any] | None,
    gaps: list[dict[str, Any]] | None,
    report: dict[str, Any] | None,
    human_review: dict[str, Any] | None,
) -> dict[str, Any]:
    target_meta: dict[str, tuple[str, str, dict[str, Any]]] = {}
    chapter_titles: dict[str, str] = {}
    for chapter, _ in _walk_chapters(outline.get("chapters", [])):
        cid, title = chapter["chapter_id"], chapter.get("title", "")
        chapter_titles[cid] = title
        for target in chapter.get("targets", []):
            target_meta[target["target_id"]] = (cid, title, target)

    violations: list[dict[str, Any]] = []
    unmet_items: list[dict[str, Any]] = []
    removed_task_ids, removed_locations = _removed_by_review_scopes(gaps)
    removed_by_review = 0
    entries = (coverage or {}).get("entries") if isinstance(coverage, dict) else None
    if isinstance(entries, list):
        for entry in entries:
            tid = entry.get("target_id")
            cid, title, target = target_meta.get(tid, (None, "", {})) if tid else (None, "", {})
            base = {
                "chapter_id": cid,
                "chapter_title": title,
                "target_id": tid,
                "coverage_id": entry.get("coverage_id"),
            }
            gap_codes = entry.get("gap_codes") or []
            if entry.get("assessment") == "unsupported" or "coverage_unsupported" in gap_codes:
                unmet_items.append({
                    **base,
                    "code": "coverage_unsupported",
                    "detail": "复核判定当前响应尚不足以满足该要求（需补资料或人工完善，非内容编造）。",
                })
            if (entry.get("resolution") == "manual" or target.get("manual_fill")) and entry.get("content_refs"):
                violations.append({
                    **base,
                    "code": "manual_target_has_generated_content",
                    "detail": "人工填写位置存在自动生成内容引用。",
                })
            if entry.get("resolution") == "content" and entry.get("content_refs"):
                if not entry.get("material_ids") and not entry.get("source_excerpts"):
                    violations.append({
                        **base,
                        "code": "content_without_material_or_source",
                        "detail": "正文写入内容既无企业资料也无来源依据。",
                    })
                if entry.get("review_status") == "unreviewed":
                    violations.append({
                        **base,
                        "code": "generated_content_unreviewed",
                        "detail": "已写入正文尚未完成复核，不能排除编造风险。",
                    })

    seen_gap_keys: set[tuple[Any, ...]] = set()
    for gap in gaps or []:
        code = gap.get("code")
        if code in _FABRICATION_GAP_CODES:
            cid = gap.get("chapter_id")
            tid = gap.get("target_id")
            task_id = gap.get("task_id")
            if code == "unsupported_content" and (
                (task_id and str(task_id) in removed_task_ids)
                or ((cid or tid) and (cid, tid) in removed_locations)
            ):
                removed_by_review += 1
                continue
            key = (code, cid, tid, gap.get("detail"))
            if key in seen_gap_keys:
                continue
            seen_gap_keys.add(key)
            violations.append({
                "chapter_id": cid,
                "chapter_title": chapter_titles.get(cid, "") if cid else "",
                "target_id": tid,
                "coverage_id": None,
                "code": code,
                "detail": gap.get("detail", ""),
            })
    for code, count in ((report or {}).get("document_gaps") or {}).items():
        if code == "unsupported_content" and count and count <= removed_by_review:
            continue
        if code in _FABRICATION_GAP_CODES and count and not any(v["code"] == code for v in violations):
            violations.append({
                "chapter_id": None,
                "chapter_title": "",
                "target_id": None,
                "coverage_id": None,
                "code": code,
                "detail": f"报表记录 {count} 条 {code}。",
            })

    if violations:
        status, machine_passed = "fail", False
    elif entries is None and gaps is None and report is None:
        status, machine_passed = "unknown", False
    else:
        machine_passed = True
        audit = (human_review or {}).get("no_fabrication_audit") or {}
        human_verified = bool(audit.get("passed") is True)
        status = "pass" if human_verified else "needs_human_check"
    return {
        "status": status,
        "machine_passed": machine_passed,
        "human_verified": status == "pass",
        "counts": {
            "coverage_entries": len(entries) if isinstance(entries, list) else None,
            "violations": len(violations),
            "removed_by_review": removed_by_review,
            "unmet_by_review": len(unmet_items),
        },
        "violations": violations,
        "unmet_items": unmet_items,
    }


def _evaluate_file_usability(
    docx_info: dict[str, Any],
    layout: dict[str, Any] | None,
    onlyoffice_receipt: dict[str, Any] | None,
) -> dict[str, Any]:
    issues: list[dict[str, Any]] = []
    if not docx_info["present"]:
        issues.append({"code": "docx_missing", "detail": "缺少导出 DOCX 文件。"})
    elif not docx_info["valid"]:
        issues.append({"code": "docx_invalid", "detail": docx_info["error"] or "DOCX 无法解析。"})
    if docx_info["missing_chapter_ids"]:
        issues.append({
            "code": "docx_missing_chapters",
            "chapter_ids": docx_info["missing_chapter_ids"],
            "detail": f"DOCX 缺少 {len(docx_info['missing_chapter_ids'])} 个大纲章节书签。",
        })
    if docx_info["missing_target_ids"]:
        issues.append({
            "code": "docx_missing_targets",
            "target_ids": docx_info["missing_target_ids"],
            "detail": f"DOCX 缺少 {len(docx_info['missing_target_ids'])} 个响应位置书签。",
        })

    layout_documents = layout.get("documents") if isinstance(layout, dict) else None
    layout_pdf_ok = bool(
        isinstance(layout, dict) and layout.get("status") == "fixed"
        and isinstance(layout_documents, list) and layout_documents and isinstance(layout_documents[0], dict)
        and layout_documents[0].get("status") == "fixed"
        and docx_info["docx_sha256"] and layout_documents[0].get("docx_sha256") == docx_info["docx_sha256"]
    )
    onlyoffice_verified = False
    if onlyoffice_receipt is not None:
        expected_sha = docx_info["docx_sha256"]
        receipt_sha = onlyoffice_receipt.get("docx_sha256")
        if expected_sha and receipt_sha != expected_sha:
            issues.append({
                "code": "onlyoffice_receipt_sha_mismatch",
                "expected": expected_sha,
                "actual": receipt_sha,
                "detail": "ONLYOFFICE 实测回执的 docx_sha256 与当前 DOCX 不一致。",
            })
        else:
            required = ("opened", "saved", "reopened", "pdf_exported")
            failed_steps = [step for step in required if onlyoffice_receipt.get(step) is not True]
            if failed_steps:
                issues.append({
                    "code": "onlyoffice_roundtrip_incomplete",
                    "failed_steps": failed_steps,
                    "detail": f"ONLYOFFICE 往返校验未通过：{', '.join(failed_steps)}。",
                })
            else:
                onlyoffice_verified = True

    machine_passed = docx_info["valid"] and not issues
    if not machine_passed:
        status = "fail"
    elif onlyoffice_verified:
        status = "pass"
    else:
        status = "needs_onlyoffice_check"
    return {
        "status": status,
        "machine_passed": machine_passed,
        "docx_sha256": docx_info["docx_sha256"],
        "docx_valid": docx_info["valid"],
        "missing_chapter_ids": docx_info["missing_chapter_ids"],
        "missing_target_ids": docx_info["missing_target_ids"],
        "layout_pdf_fixed": layout_pdf_ok,
        "onlyoffice_verified": onlyoffice_verified,
        "issues": issues,
    }


def _analysis_binding(artifact: dict[str, Any] | None) -> dict[str, Any] | None:
    source = artifact.get("authoring_input") if isinstance(artifact, dict) else None
    if not isinstance(source, dict):
        return None
    frozen = source.get("frozen_ref")
    frozen_sha = frozen.get("sha256") if isinstance(frozen, dict) else None
    run_id = source.get("run_id")
    versions = {key: source.get(key) for key in ("source_version", "domain_revision")}
    if (not isinstance(frozen_sha, str) or not re.fullmatch(r"[0-9a-f]{64}", frozen_sha)
            or not isinstance(run_id, str) or not run_id
            or any(type(version) is not int or version < 1 for version in versions.values())):
        return None
    return {"frozen_sha256": frozen_sha, "run_id": run_id, **versions}


def _bound_human_review(human_review: dict[str, Any] | None, binding: dict[str, Any] | None,
                        docx_info: dict[str, Any]) -> dict[str, Any] | None:
    if not isinstance(human_review, dict) or binding is None or not docx_info["valid"]:
        return None
    expected = {**binding, "docx_sha256": docx_info["docx_sha256"]}
    if any(type(human_review.get(key)) is not type(value) or human_review[key] != value
           for key, value in expected.items()):
        return None
    return human_review


def evaluate_acceptance(
    *,
    outline: dict[str, Any],
    docx_bytes: bytes | None,
    report: dict[str, Any] | None = None,
    requirements: dict[str, Any] | None = None,
    coverage: dict[str, Any] | None = None,
    gaps: list[dict[str, Any]] | None = None,
    layout: dict[str, Any] | None = None,
    artifacts: ArtifactStore | None = None,
    expected_tables: dict[str, list[dict[str, Any]]] | None = None,
    required_tables: dict[str, Any] | None = None,
    onlyoffice_receipt: dict[str, Any] | None = None,
    human_review: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Score the bid compiled from the inputs available in this frozen execution."""
    docx_info = _inspect_docx(docx_bytes, outline)
    binding = _analysis_binding(outline)
    requirements_binding = _analysis_binding(requirements)
    coverage_binding = _analysis_binding(coverage)
    human_review = _bound_human_review(
        human_review,
        binding if requirements_binding == binding and (coverage is None or coverage_binding == binding) else None,
        docx_info,
    )
    foreign_requirements = bool(requirements is not None and "authoring_input" in requirements
                                and (binding is None or requirements_binding != binding))
    foreign_coverage = bool(coverage is not None and "authoring_input" in coverage
                            and (binding is None or coverage_binding != binding))
    criteria = {
        "requirements_coverage": _evaluate_requirements(
            outline, None if foreign_requirements else requirements, docx_info, human_review, gaps
        ),
        "original_tables": _evaluate_original_tables(
            outline, gaps, docx_info, artifacts, expected_tables=expected_tables, required_tables=required_tables
        ),
        "no_fabrication": _evaluate_no_fabrication(
            outline, None if foreign_coverage else coverage, gaps, report, human_review
        ),
        "file_usability": _evaluate_file_usability(
            docx_info, layout, onlyoffice_receipt
        ),
    }
    if foreign_requirements:
        criteria["requirements_coverage"]["detail"] = "招标要求底稿与大纲的分析快照不一致。"
    if foreign_coverage:
        criteria["no_fabrication"]["detail"] = "覆盖结算与大纲的分析快照不一致。"
    counts = Counter(item["status"] for item in criteria.values())
    passed = all(item["status"] == "pass" for item in criteria.values())
    summary = {
        "score_passed": counts["pass"],
        "score_total": len(criteria),
        "machine_passed": sum(1 for item in criteria.values() if item["machine_passed"]),
        "pass": counts["pass"],
        "fail": counts["fail"],
        "needs_human_check": counts["needs_human_check"],
        "needs_onlyoffice_check": counts["needs_onlyoffice_check"],
        "unknown": counts["unknown"],
    }
    execution_report = None
    if isinstance(report, dict):
        execution_report = {
            key: report[key]
            for key in (
                "execution_id", "status", "kind", "requests",
                "targets", "outline_issues", "document_gaps", "failures",
            )
            if key in report
        }
    return {
        "schema_version": 1,
        "evaluation_scope": "received_inputs",
        "passed": passed,
        "summary": summary,
        "criteria": criteria,
        "execution_report": execution_report,
        "limits": [
            "Offline checks cannot replace manual spot-checking of at least 3 scoring tables and qualification clauses against original files.",
            "Original table acceptance covers the independent inventory of applicable tables in the received sources (--required-tables) and their frozen grids (--artifact-root).",
            "Absence of machine fabrication flags does not verify factual truthfulness against enterprise materials.",
            "DOCX structural validity does not prove live ONLYOFFICE open/save/reopen and PDF roundtrip without a matching receipt.",
        ],
    }


def load_inputs(args: argparse.Namespace) -> dict[str, Any]:
    artifacts = ArtifactStore(args.artifact_root) if args.artifact_root else None
    outline = json.loads(args.outline.read_text(encoding="utf-8")) if args.outline else None
    docx_bytes = args.docx.read_bytes() if args.docx else None
    report = json.loads(args.report.read_text(encoding="utf-8")) if args.report else None
    requirements = json.loads(args.requirements.read_text(encoding="utf-8")) if args.requirements else None
    coverage = json.loads(args.coverage.read_text(encoding="utf-8")) if args.coverage else None
    gaps = json.loads(args.gaps.read_text(encoding="utf-8")) if args.gaps else None
    layout = json.loads(args.layout.read_text(encoding="utf-8")) if args.layout else None
    onlyoffice_receipt = (
        json.loads(args.onlyoffice_receipt.read_text(encoding="utf-8"))
        if args.onlyoffice_receipt else None
    )
    human_review = (
        json.loads(args.human_review.read_text(encoding="utf-8"))
        if args.human_review else None
    )

    if args.result is not None:
        if artifacts is None:
            raise ValueError("--result requires --artifact-root to resolve artifact references")
        result = json.loads(args.result.read_text(encoding="utf-8"))
        if outline is None and result.get("outline_ref"):
            outline = artifacts.read_json(result["outline_ref"])
        if docx_bytes is None and result.get("docx_ref"):
            docx_bytes = artifacts.read_bytes(result["docx_ref"])
        if requirements is None and result.get("requirements_ref"):
            requirements = artifacts.read_json(result["requirements_ref"])
        if coverage is None and result.get("coverage_ref"):
            coverage = artifacts.read_json(result["coverage_ref"])
        if layout is None and result.get("layout_ref"):
            layout = artifacts.read_json(result["layout_ref"])
        if gaps is None and isinstance(result.get("gaps"), list):
            gaps = result["gaps"]

    if args.package is not None:
        with ZipFile(args.package) as archive:
            names = set(archive.namelist())
            if docx_bytes is None and "投标文件.docx" in names:
                docx_bytes = archive.read("投标文件.docx")
            if requirements is None and "招标要求底稿.json" in names:
                requirements = json.loads(archive.read("招标要求底稿.json").decode("utf-8"))
            if coverage is None and "覆盖结算.json" in names:
                coverage = json.loads(archive.read("覆盖结算.json").decode("utf-8"))
            if gaps is None and "待补事项.json" in names:
                gaps = json.loads(archive.read("待补事项.json").decode("utf-8"))
            if layout is None and "版式核对.json" in names:
                layout = json.loads(archive.read("版式核对.json").decode("utf-8"))

    if outline is None:
        raise ValueError("outline is required (pass --outline or --result with --artifact-root)")
    return {
        "outline": outline,
        "docx_bytes": docx_bytes,
        "report": report,
        "requirements": requirements,
        "coverage": coverage,
        "gaps": gaps,
        "layout": layout,
        "artifacts": artifacts,
        "required_tables": json.loads(args.required_tables.read_text(encoding="utf-8")) if args.required_tables else None,
        "onlyoffice_receipt": onlyoffice_receipt,
        "human_review": human_review,
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--outline", type=Path, help="Authoring outline JSON")
    parser.add_argument("--docx", type=Path, help="Exported tender DOCX")
    parser.add_argument("--package", type=Path, help="Exported delivery package ZIP")
    parser.add_argument("--result", type=Path, help="Authoring document result JSON (requires --artifact-root)")
    parser.add_argument("--artifact-root", type=Path, help="Persistent artifact root for frozen original grids")
    parser.add_argument("--report", type=Path, help="authoring-report JSON")
    parser.add_argument("--requirements", type=Path, help="招标要求底稿 JSON")
    parser.add_argument("--coverage", type=Path, help="覆盖结算 JSON")
    parser.add_argument("--gaps", type=Path, help="待补事项 JSON")
    parser.add_argument("--layout", type=Path, help="版式核对 JSON")
    parser.add_argument("--required-tables", type=Path, help="Independent mandatory grid inventory for the frozen sources")
    parser.add_argument("--onlyoffice-receipt", type=Path, help="ONLYOFFICE open/save/reopen/PDF receipt JSON")
    parser.add_argument("--human-review", type=Path, help="Explicit human spot-check and fabrication audit JSON")
    parser.add_argument("--output", type=Path, help="Write JSON scorecard to this path")
    parser.add_argument("--strict", action="store_true", help="Exit 1 unless all four criteria pass")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    score = evaluate_acceptance(**load_inputs(args))
    rendered = json.dumps(score, ensure_ascii=False, indent=2)
    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n", encoding="utf-8")
    print(rendered)
    return 1 if args.strict and not score["passed"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
