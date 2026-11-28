"""Offline evaluation of the four minimum acceptance criteria (W1.3)."""
from __future__ import annotations

import hashlib
import json

from docx import Document
import pytest

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
        "authoring_input": {
            "run_id": "run-1", "source_version": 1, "domain_revision": 1,
            "frozen_ref": artifacts.put_json({"documents": [{"source_index_ref": parts[0]["source_ref"]["source_index_ref"]}]}),
        },
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
        "authoring_input": outline["authoring_input"],
        "rules": [
            {"rule_ref": rule_1, "rule": {"item_id": "item-1", "text": "须提交开标一览表并盖章"}},
            {"rule_ref": rule_2, "rule": {"item_id": "item-2", "text": "须提供完整运维技术方案"}},
        ]
    }
    coverage = {
        "authoring_input": outline["authoring_input"],
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


def _required_tables(artifacts, outline):
    frozen = outline["authoring_input"]["frozen_ref"]
    doc, = artifacts.read_json(frozen)["documents"]
    return {"frozen_sha256": frozen["sha256"], "complete": True,
            "tables": [{"source_index_sha256": doc["source_index_ref"]["sha256"], "unit_id": "u-grid"}]}


def _human_review(outline, docx_bytes):
    source = outline["authoring_input"]
    return {
        "docx_sha256": hashlib.sha256(docx_bytes).hexdigest(),
        "frozen_sha256": source["frozen_ref"]["sha256"],
        **{key: source[key] for key in ("run_id", "source_version", "domain_revision")},
        "requirements_spot_check": {"passed": True, "checked_clauses": 3, "double_omissions": 0},
        "no_fabrication_audit": {"passed": True},
    }


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
        required_tables=_required_tables(artifacts, outline),
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
        required_tables=_required_tables(artifacts, outline),
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
        human_review=_human_review(outline, docx_bytes),
    )
    assert verified["passed"] is True
    assert verified["summary"]["score_passed"] == 4


@pytest.mark.parametrize(("field", "wrong"), [
    ("docx_sha256", "0" * 64),
    ("frozen_sha256", "0" * 64),
    ("run_id", "another-run"),
    ("source_version", 2),
    ("domain_revision", 2),
])
def test_human_review_cannot_pass_when_its_identity_is_stale(tmp_path, field, wrong):
    artifacts, outline, requirements, coverage, exported = _export_sample(tmp_path)
    docx_bytes = artifacts.read_bytes(exported["docx_ref"])
    receipt = _human_review(outline, docx_bytes)
    receipt[field] = wrong
    for review in (receipt, {key: value for key, value in receipt.items() if key != field}):
        criteria = evaluate_acceptance(
            outline=outline, docx_bytes=docx_bytes, requirements=requirements, coverage=coverage,
            gaps=exported["gaps"], artifacts=artifacts, human_review=review,
        )["criteria"]
        assert criteria["requirements_coverage"]["status"] == "needs_human_check"
        assert criteria["no_fabrication"]["status"] == "needs_human_check"


@pytest.mark.parametrize("component", ["requirements", "coverage"])
def test_mixed_analysis_snapshots_cannot_satisfy_formal_acceptance(tmp_path, component):
    artifacts, outline, requirements, coverage, exported = _export_sample(tmp_path)
    docx_bytes = artifacts.read_bytes(exported["docx_ref"])
    inputs = {"requirements": requirements, "coverage": coverage}
    other = {"authoring_input": {**outline["authoring_input"], "domain_revision": 2}}
    inputs[component] = {**other, **({"rules": []} if component == "requirements" else {"entries": []})}
    score = evaluate_acceptance(
        outline=outline, docx_bytes=docx_bytes, **inputs, artifacts=artifacts, gaps=exported["gaps"],
        human_review=_human_review(outline, docx_bytes),
        onlyoffice_receipt={"docx_sha256": hashlib.sha256(docx_bytes).hexdigest(),
                            **dict.fromkeys(("opened", "saved", "reopened", "pdf_exported"), True)},
    )
    assert not score["passed"]
    criteria = score["criteria"]
    assert criteria["requirements_coverage"]["status"] == (
        "unknown" if component == "requirements" else "needs_human_check"
    )
    assert criteria["no_fabrication"]["status"] == "needs_human_check"


def test_unassigned_summary_and_derived_coverage_do_not_clear_unlisted_items(tmp_path):
    artifacts, outline, requirements, coverage, exported = _export_sample(tmp_path)
    documented, omitted = _rule_ref("documented"), _rule_ref("omitted")
    for reference in (documented, omitted):
        requirements["rules"].append({"rule_ref": reference, "rule": {"item_id": reference["item_id"], "text": "要响应"}})
        coverage["entries"].append({"rule_ref": reference, "resolution": "pending", "gap_codes": ["coverage_target_missing"]})
    outline["issues"].append({"code": "outline_rule_unassigned", "rule_refs": [documented], "detail": "已逐条登记。"})
    result = evaluate_acceptance(
        outline=outline, docx_bytes=artifacts.read_bytes(exported["docx_ref"]), requirements=requirements,
        coverage=coverage, gaps=[{"code": "export_requirement_unassigned"}], artifacts=artifacts,
    )["criteria"]["requirements_coverage"]
    assert [item["item_id"] for item in result["pending_items"]] == ["documented"]
    assert [item["item_id"] for item in result["missing_items"]] == ["omitted"]
    assert result["status"] == "fail"


def test_empty_original_grid_evidence_cannot_pass_the_table_criterion(tmp_path):
    artifacts, outline, _, _, _ = _export_sample(tmp_path)
    outline["chapters"][0]["targets"][0]["template_refs"] = []
    exported = export_authoring_document(artifacts, outline, [])
    result = evaluate_acceptance(
        outline=outline, docx_bytes=artifacts.read_bytes(exported["docx_ref"]),
        artifacts=artifacts, expected_tables={},
    )["criteria"]["original_tables"]
    assert result["counts"]["docx_tables"] == 0
    assert result["counts"]["checked_tables"] == 0
    assert result["grid_comparison"] == "unavailable"
    assert result["status"] == "unknown"


def test_layout_fixed_is_for_the_exact_document_bytes(tmp_path):
    artifacts, outline, _, _, exported = _export_sample(tmp_path)
    docx_bytes = artifacts.read_bytes(exported["docx_ref"])
    for sha, expected in (("0" * 64, False), (hashlib.sha256(docx_bytes).hexdigest(), True)):
        layout = {"status": "fixed", "documents": [{"status": "fixed", "docx_sha256": sha}]}
        criterion = evaluate_acceptance(outline=outline, docx_bytes=docx_bytes, layout=layout)["criteria"]["file_usability"]
        assert criterion["layout_pdf_fixed"] is expected
        assert criterion["status"] == "needs_onlyoffice_check"


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

    # Inject fabrication / ungrounded violations in coverage alongside an unmet coverage_unsupported audit.
    coverage["entries"][0]["content_refs"] = [
        {"unit_ref": {"path": "u", "sha256": "1" * 64, "size": 1}, "content_id": "f1"}
    ]
    coverage["entries"][1]["material_ids"] = []
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
        "content_without_material_or_source",
    }
    assert fab["counts"]["unmet_by_review"] == 1

    usability = score["criteria"]["file_usability"]
    assert usability["status"] == "fail"
    assert any(i["code"] == "onlyoffice_receipt_sha_mismatch" for i in usability["issues"])


def test_cli_reads_exported_package_zip_offline(tmp_path):
    artifacts, outline, _, _, exported = _export_sample(tmp_path)
    outline_path = tmp_path / "outline.json"
    outline_path.write_text(json.dumps(outline, ensure_ascii=False), encoding="utf-8")
    package_path = artifacts.path(exported["package_ref"])
    out_path = tmp_path / "score.json"

    required_path = tmp_path / "required-tables.json"
    required_path.write_text(json.dumps(_required_tables(artifacts, outline)))
    exit_code = main([
        "--required-tables", str(required_path),
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
    assert loaded_score["summary"]["machine_passed"] == 3
    assert loaded_score["criteria"]["original_tables"]["inventory_status"] == "missing"


def test_removed_unsupported_content_is_audited_separately_while_unremoved_still_fails(tmp_path):
    artifacts, outline, requirements, coverage, exported = _export_sample(tmp_path)
    docx_bytes = artifacts.read_bytes(exported["docx_ref"])

    # When _adopt_reviewed stripped the rejected block (matching writing_gap present), no_fabrication stays machine_passed.
    removed_gaps = [
        *exported["gaps"],
        {"code": "writing_gap", "task_id": "task-2", "chapter_id": "ch-2", "target_id": "t-2",
         "detail": "复核明确否定的内容未采用，相应位置留空；其他有据内容保留。"},
        {"code": "unsupported_content", "task_id": "task-2", "chapter_id": "ch-2", "target_id": "t-2",
         "detail": "历史复核发现已由宿主剔除。"},
    ]
    cleaned = evaluate_acceptance(
        outline=outline, docx_bytes=docx_bytes, requirements=requirements,
        coverage=coverage, gaps=removed_gaps, artifacts=artifacts,
    )
    fab_ok = cleaned["criteria"]["no_fabrication"]
    assert fab_ok["machine_passed"] is True
    assert fab_ok["counts"]["violations"] == 0
    assert fab_ok["counts"]["removed_by_review"] == 1

    # Without matching host removal confirmation, unsupported_content MUST still fail no_fabrication.
    leaked_gaps = [
        *exported["gaps"],
        {"code": "unsupported_content", "task_id": "task-2", "chapter_id": "ch-2", "target_id": "t-2",
         "detail": "未剔除的无据内容。"},
    ]
    leaked = evaluate_acceptance(
        outline=outline, docx_bytes=docx_bytes, requirements=requirements,
        coverage=coverage, gaps=leaked_gaps, artifacts=artifacts,
    )
    assert leaked["criteria"]["no_fabrication"]["machine_passed"] is False
    assert leaked["criteria"]["no_fabrication"]["status"] == "fail"
    assert leaked["criteria"]["no_fabrication"]["counts"]["removed_by_review"] == 0


def test_coverage_unsupported_is_audited_as_unmet_not_fabrication_while_ungrounded_still_fails(tmp_path):
    artifacts, outline, requirements, coverage, exported = _export_sample(tmp_path)
    docx_bytes = artifacts.read_bytes(exported["docx_ref"])

    coverage["entries"][1]["assessment"] = "unsupported"
    coverage["entries"][1]["gap_codes"] = ["coverage_unsupported"]
    unmet = evaluate_acceptance(
        outline=outline,
        docx_bytes=docx_bytes,
        requirements=requirements,
        coverage=coverage,
        gaps=exported["gaps"],
        artifacts=artifacts,
    )
    fab = unmet["criteria"]["no_fabrication"]
    assert fab["machine_passed"] is True
    assert fab["status"] == "needs_human_check"
    assert fab["counts"]["violations"] == 0
    assert fab["counts"]["unmet_by_review"] == 1
    assert len(fab["unmet_items"]) == 1
    assert fab["unmet_items"][0]["code"] == "coverage_unsupported"
    assert fab["unmet_items"][0]["coverage_id"] == "c-2"

    # Clearing supporting materials/source excerpts while keeping generated content MUST fail no_fabrication.
    coverage["entries"][1]["material_ids"] = []
    coverage["entries"][1]["source_excerpts"] = []
    ungrounded = evaluate_acceptance(
        outline=outline,
        docx_bytes=docx_bytes,
        requirements=requirements,
        coverage=coverage,
        gaps=exported["gaps"],
        artifacts=artifacts,
    )
    assert ungrounded["criteria"]["no_fabrication"]["machine_passed"] is False
    assert ungrounded["criteria"]["no_fabrication"]["status"] == "fail"
    assert {v["code"] for v in ungrounded["criteria"]["no_fabrication"]["violations"]} == {
        "content_without_material_or_source",
    }



def test_other_matching_word_table_does_not_prove_independent_source_inventory(tmp_path):
    artifacts, outline, requirements, coverage, exported = _export_sample(tmp_path)
    docx_bytes = artifacts.read_bytes(exported["docx_ref"])

    def tables(inventory=None):
        return evaluate_acceptance(outline=outline, docx_bytes=docx_bytes, required_tables=inventory,
                                   requirements=requirements, coverage=coverage, artifacts=artifacts
                                   )["criteria"]["original_tables"]

    missing = tables()
    assert missing["counts"]["checked_tables"] == 1
    assert missing["status"] == "unknown"
    partial = {**_required_tables(artifacts, outline), "complete": False}
    assert tables(partial)["status"] == "unknown"
    # A second, independently identified source contains an identical grid.
    other_parts = _frozen_template(artifacts, _sample_grid(), fixed_text="另一张必交附件")
    other_index = other_parts[-1]["source_ref"]["source_index_ref"]
    freeze = outline["authoring_input"]
    docs = artifacts.read_json(freeze["frozen_ref"])["documents"]
    freeze["frozen_ref"] = artifacts.put_json({"documents": [*docs, {"source_index_ref": other_index}]})
    manifest = {"frozen_sha256": freeze["frozen_ref"]["sha256"], "complete": False,
                "tables": [{"source_index_sha256": other_index["sha256"], "unit_id": "u-grid"}]}
    criterion = tables(manifest)
    assert criterion["status"] == "fail"
    assert criterion["mismatches"] == [{"kind": "required_table_missing", **manifest["tables"][0]}]
    for bad in ({**manifest, "frozen_sha256": "0" * 64}, {**manifest, "tables": manifest["tables"] * 2},
                {**manifest, "tables": [{"source_index_sha256": other_index["sha256"], "unit_id": "u-text"}]}):
        check = tables(bad)
        assert check["status"] == "unknown" and check["inventory_status"] == "invalid"
