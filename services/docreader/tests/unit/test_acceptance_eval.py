"""Offline evaluation of the four minimum acceptance criteria (W1.3)."""
from __future__ import annotations

import hashlib
import json

from docx import Document

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.acceptance_eval import evaluate_acceptance, main
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.word_navigation import add_bookmark


def _rule_ref(item_id: str) -> dict:
    return {
        "run_id": "run-1",
        "source_version": 1,
        "domain_revision": 1,
        "pack_id": "pack-1",
        "item_id": item_id,
    }


def _frozen_template(artifacts: ArtifactStore, grid: dict, fixed_text: str | None = None):
    units = []
    contract_units = []
    if fixed_text:
        locator = {"locator_kind": "document", "section_ordinal": 0}
        units.append({"unit_id": "u-text", "kind": "section", "ordinal": 0, "text": fixed_text, "grid": None, "locator": locator})
        contract_units.append({"unit_id": "u-text", "section_id": "section:0", "parent_section_id": None,
                               "heading_path": "投标格式", "physical_locator": locator})
    grid_locator = {"locator_kind": "document", "section_ordinal": len(units)}
    units.append({"unit_id": "u-grid", "kind": "table_region", "ordinal": len(units), "text": "", "grid": grid, "locator": grid_locator})
    contract_units.append({"unit_id": "u-grid", "section_id": "section:0", "parent_section_id": None,
                           "heading_path": "投标格式", "physical_locator": grid_locator})
    raw = {"schema_version": 2, "units": units, "images": {}, "ocr": {},
           "source_contract": {"page_manifest": [], "units": contract_units}}
    document = {"document_id": "main", "document_version": "v1",
                "original_ref": artifacts.put_bytes(b"synthetic original")}
    index = SourceIndex(document, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    parts = []
    if fixed_text:
        parts.append({
            "source_ref": {"source_index_ref": index_ref, "unit_id": "u-text"},
            "selectors": [],
        })
    parts.append({
        "source_ref": {"source_index_ref": index_ref, "unit_id": "u-grid"},
        "selectors": [],
        "header_row_count": 1,
    })
    return parts


def _sample_grid() -> dict:
    return {
        "row_count": 3,
        "column_count": 3,
        "cells": [
            {"row": 0, "column": 0, "row_span": 1, "col_span": 2, "text": "报价明细"},
            {"row": 0, "column": 2, "row_span": 1, "col_span": 1, "text": "备注"},
            {"row": 1, "column": 0, "row_span": 2, "col_span": 1, "text": "合计"},
            {"row": 1, "column": 1, "row_span": 1, "col_span": 1, "text": ""},
            {"row": 1, "column": 2, "row_span": 1, "col_span": 1, "text": "盖章处"},
            {"row": 2, "column": 1, "row_span": 1, "col_span": 1, "text": ""},
            {"row": 2, "column": 2, "row_span": 1, "col_span": 1, "text": ""},
        ],
    }


def _export_sample(tmp_path):
    artifacts = ArtifactStore(tmp_path / "artifacts")
    grid = _sample_grid()
    parts = _frozen_template(artifacts, grid, fixed_text="法定代表人或委托代理人签字：")
    rule_1 = _rule_ref("item-1")
    rule_2 = _rule_ref("item-2")
    outline = {
        "schema_version": 2,
        "chapters": [
            {
                "chapter_id": "ch-1",
                "title": "开标一览表",
                "targets": [
                    {
                        "target_id": "t-1",
                        "kind": "form",
                        "purpose": "填写开标一览表",
                        "manual_fill": True,
                        "rule_refs": [rule_1],
                        "template_refs": parts,
                    }
                ],
                "children": [],
            },
            {
                "chapter_id": "ch-2",
                "title": "技术方案",
                "targets": [
                    {
                        "target_id": "t-2",
                        "kind": "narrative",
                        "purpose": "技术方案响应",
                        "manual_fill": False,
                        "rule_refs": [rule_2],
                        "template_refs": [],
                    }
                ],
                "children": [],
            },
        ],
        "issues": [],
    }
    requirements = {
        "rules": [
            {"rule_ref": rule_1, "rule": {"item_id": "item-1", "text": "须提交开标一览表并盖章"}},
            {"rule_ref": rule_2, "rule": {"item_id": "item-2", "text": "须提供完整运维技术方案"}},
        ]
    }
    coverage = {
        "entries": [
            {
                "coverage_id": "c-1",
                "rule_ref": rule_1,
                "target_id": "t-1",
                "resolution": "manual",
                "material_ids": [],
                "source_excerpts": [],
                "content_refs": [],
                "assessment": None,
                "review_status": "not_required",
                "gap_codes": ["coverage_unfilled"],
            },
            {
                "coverage_id": "c-2",
                "rule_ref": rule_2,
                "target_id": "t-2",
                "resolution": "content",
                "material_ids": ["mat-1"],
                "source_excerpts": [],
                "content_refs": [{"unit_ref": {"path": "u", "sha256": "0" * 64, "size": 1}, "content_id": "b1"}],
                "assessment": "supported",
                "review_status": "reviewed",
                "gap_codes": [],
            },
        ]
    }
    writing_units = [
        {
            "task_id": "task-2",
            "chapter_id": "ch-2",
            "target_ids": ["t-2"],
            "status": "generated",
            "blocks": [{"kind": "paragraph", "text": "基于企业运维规范提供7x24小时响应。", "target_ids": ["t-2"]}],
            "cell_fills": [],
            "row_fills": [],
            "attachments": [],
            "gaps": [],
        }
    ]
    exported = export_authoring_document(
        artifacts, outline, writing_units, requirements=requirements, coverage=coverage
    )
    return artifacts, outline, requirements, coverage, exported


def test_clean_export_passes_machine_checks_and_waits_for_human_and_onlyoffice(tmp_path):
    artifacts, outline, requirements, coverage, exported = _export_sample(tmp_path)
    docx_bytes = artifacts.read_bytes(exported["docx_ref"])
    report = {
        "execution_id": "exec-1",
        "status": "succeeded",
        "kind": "document",
        "requests": 3,
        "targets": {"total": 2, "knowledge_answers": 0, "with_rule_refs": 2},
        "outline_issues": {},
        "document_gaps": {},
        "failures": [],
    }
    score = evaluate_acceptance(
        outline=outline,
        docx_bytes=docx_bytes,
        report=report,
        requirements=requirements,
        coverage=coverage,
        gaps=exported["gaps"],
        artifacts=artifacts,
    )
    assert score["passed"] is False
    assert score["summary"]["machine_passed"] == 4
    assert score["criteria"]["requirements_coverage"]["status"] == "needs_human_check"
    assert score["criteria"]["original_tables"]["status"] == "pass"
    assert score["criteria"]["original_tables"]["counts"]["checked_tables"] == 1
    assert score["criteria"]["original_tables"]["counts"]["checked_fixed_texts"] == 1
    assert score["criteria"]["no_fabrication"]["status"] == "needs_human_check"
    assert score["criteria"]["file_usability"]["status"] == "needs_onlyoffice_check"
    assert score["execution_report"]["execution_id"] == "exec-1"

    # Explicit human spot-check + fabrication audit + matching ONLYOFFICE receipt unlocks 4/4 pass.
    verified = evaluate_acceptance(
        outline=outline,
        docx_bytes=docx_bytes,
        report=report,
        requirements=requirements,
        coverage=coverage,
        gaps=exported["gaps"],
        artifacts=artifacts,
        onlyoffice_receipt={
            "docx_sha256": hashlib.sha256(docx_bytes).hexdigest(),
            "opened": True,
            "saved": True,
            "reopened": True,
            "pdf_exported": True,
        },
        human_review={
            "requirements_spot_check": {"passed": True, "checked_clauses": 3, "double_omissions": 0},
            "no_fabrication_audit": {"passed": True},
        },
    )
    assert verified["passed"] is True
    assert verified["summary"]["score_passed"] == 4


def test_pinpoints_silent_missing_requirement_wrong_table_and_fabrication(tmp_path):
    artifacts, outline, requirements, coverage, exported = _export_sample(tmp_path)
    # Add an unplaced rule that is neither in outline nor in gaps (silent omission).
    missing_ref = _rule_ref("item-99")
    requirements["rules"].append({
        "rule_ref": missing_ref,
        "rule": {"item_id": "item-99", "text": "须提供信息安全等级保护三级备案证明"},
    })
    # Break the Word table (wrong column count, broken merges, missing signature line).
    broken_doc = Document()
    h1 = broken_doc.add_heading("开标一览表", level=1)
    add_bookmark(h1._p, "ch-1", "chapter")
    tbl = broken_doc.add_table(rows=2, cols=2)
    tbl.cell(0, 0).text = "错误表头"
    add_bookmark(tbl._tbl, "t-1", "target")
    h2 = broken_doc.add_heading("技术方案", level=1)
    add_bookmark(h2._p, "ch-2", "chapter")
    p2 = broken_doc.add_paragraph("正文")
    add_bookmark(p2._p, "t-2", "target")
    docx_path = tmp_path / "broken.docx"
    broken_doc.save(docx_path)
    broken_bytes = docx_path.read_bytes()

    # Inject fabrication / ungrounded violations in coverage.
    coverage["entries"][0]["content_refs"] = [
        {"unit_ref": {"path": "u", "sha256": "1" * 64, "size": 1}, "content_id": "f1"}
    ]
    coverage["entries"][1]["assessment"] = "unsupported"

    score = evaluate_acceptance(
        outline=outline,
        docx_bytes=broken_bytes,
        requirements=requirements,
        coverage=coverage,
        gaps=exported["gaps"],
        artifacts=artifacts,
        onlyoffice_receipt={
            "docx_sha256": "0" * 64,
            "opened": True,
            "saved": True,
            "reopened": True,
            "pdf_exported": True,
        },
    )
    assert score["passed"] is False
    assert score["summary"]["fail"] == 4

    req = score["criteria"]["requirements_coverage"]
    assert req["status"] == "fail"
    assert [m["item_id"] for m in req["missing_items"]] == ["item-99"]
    assert req["missing_items"][0]["reason"] == "unplaced_and_unlisted_in_gaps"

    tables = score["criteria"]["original_tables"]
    assert tables["status"] == "fail"
    kinds = {m["kind"] for m in tables["mismatches"]}
    assert {"column_count_mismatch", "row_count_mismatch", "header_mismatch", "merge_mismatch", "fixed_text_missing"} <= kinds
    assert all(m["chapter_id"] == "ch-1" and m["target_id"] == "t-1" for m in tables["mismatches"])

    fab = score["criteria"]["no_fabrication"]
    assert fab["status"] == "fail"
    assert {v["code"] for v in fab["violations"]} == {
        "manual_target_has_generated_content",
        "coverage_unsupported",
    }

    usability = score["criteria"]["file_usability"]
    assert usability["status"] == "fail"
    assert any(i["code"] == "onlyoffice_receipt_sha_mismatch" for i in usability["issues"])


def test_cli_reads_exported_package_zip_offline(tmp_path):
    artifacts, outline, _, _, exported = _export_sample(tmp_path)
    outline_path = tmp_path / "outline.json"
    outline_path.write_text(json.dumps(outline, ensure_ascii=False), encoding="utf-8")
    package_path = artifacts.path(exported["package_ref"])
    out_path = tmp_path / "score.json"

    exit_code = main([
        "--outline", str(outline_path),
        "--package", str(package_path),
        "--artifact-root", str(artifacts.root),
        "--output", str(out_path),
    ])
    assert exit_code == 0
    saved = json.loads(out_path.read_text(encoding="utf-8"))
    assert saved["summary"]["machine_passed"] == 4
    assert saved["criteria"]["original_tables"]["status"] == "pass"

    # Strict mode returns 1 when human / ONLYOFFICE checks are not yet confirmed.
    assert main([
        "--outline", str(outline_path),
        "--package", str(package_path),
        "--strict",
    ]) == 1


def test_unknown_inputs_stay_unknown_and_explicit_pending_is_not_silent_omission(tmp_path):
    artifacts, outline, requirements, coverage, exported = _export_sample(tmp_path)
    docx_bytes = artifacts.read_bytes(exported["docx_ref"])

    # Without requirements or artifact_root, those criteria stay unknown rather than passing.
    unknown_score = evaluate_acceptance(outline=outline, docx_bytes=docx_bytes)
    assert unknown_score["criteria"]["requirements_coverage"]["status"] == "unknown"
    assert unknown_score["criteria"]["original_tables"]["status"] == "unknown"
    assert unknown_score["criteria"]["no_fabrication"]["status"] == "unknown"
    assert unknown_score["criteria"]["file_usability"]["status"] == "needs_onlyoffice_check"

    # An unplaced rule explicitly listed in outline.issues is counted as pending_in_gaps, not missing.
    pending_ref = _rule_ref("item-pending")
    requirements["rules"].append({
        "rule_ref": pending_ref,
        "rule": {"item_id": "item-pending", "text": "待人工安排的补充条款"},
    })
    outline["issues"].append({
        "code": "outline_rule_unassigned",
        "detail": "尚未安排到具体章节。",
        "rule_refs": [pending_ref],
    })
    score = evaluate_acceptance(
        outline=outline,
        docx_bytes=docx_bytes,
        requirements=requirements,
        coverage=coverage,
        gaps=exported["gaps"],
        artifacts=artifacts,
    )
    req = score["criteria"]["requirements_coverage"]
    assert req["machine_passed"] is True
    assert req["counts"]["pending_in_gaps"] == 1 and req["counts"]["missing"] == 0
    assert [item["item_id"] for item in req["pending_items"]] == ["item-pending"]

    # Corrupt DOCX bytes fail usability and table checks cleanly without raising.
    bad = evaluate_acceptance(
        outline=outline,
        docx_bytes=b"not-a-zip",
        requirements=requirements,
        coverage=coverage,
        gaps=exported["gaps"],
        artifacts=artifacts,
    )
    assert bad["criteria"]["file_usability"]["status"] == "fail"
    assert bad["criteria"]["original_tables"]["status"] == "fail"
    assert bad["criteria"]["file_usability"]["issues"][0]["code"] == "docx_invalid"

    # --result + --artifact-root resolves outline, docx, requirements, coverage, and gaps.
    result_path = tmp_path / "result.json"
    result_path.write_text(
        json.dumps({
            "outline_ref": artifacts.put_json(outline),
            "docx_ref": exported["docx_ref"],
            "requirements_ref": artifacts.put_json(requirements),
            "coverage_ref": artifacts.put_json(coverage),
            "gaps": exported["gaps"],
        }, ensure_ascii=False),
        encoding="utf-8",
    )
    out_path = tmp_path / "from-result.json"
    assert main([
        "--result", str(result_path),
        "--artifact-root", str(artifacts.root),
        "--output", str(out_path),
    ]) == 0
    loaded_score = json.loads(out_path.read_text(encoding="utf-8"))
    assert loaded_score["summary"]["machine_passed"] == 4

