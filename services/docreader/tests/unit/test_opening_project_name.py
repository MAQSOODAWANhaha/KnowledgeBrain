"""The host copies only a source-proven project name onto the opening form."""

from copy import deepcopy
from io import BytesIO
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.evidence.references import SourceIndex, selector

from test_authoring_export import chapter, writing
from test_structural_packing import source


NAME = "甲区公共服务系统维护项目"
LABELS = ("项目名称", "投标报价", "投标保证金", "签章")
ANNOUNCEMENT = (
    "第一章 招标公告",
    "一、项目基本情况",
    "招标编号：示例编号",
    f"项目名称：{NAME}",
    "预算金额：人民币30万元",
)


def opening_grid(*, name="", merge=True, labels=LABELS):
    def cell(row, column, text, span=1):
        return {"row": row, "column": column, "text": text, "row_span": span, "col_span": 1}

    return {"row_count": 3, "column_count": 4, "cells": [
        *(cell(0, column, label) for column, label in enumerate(labels)),
        cell(1, 0, name, 2 if merge else 1), cell(1, 1, "小写："),
        cell(1, 2, "", 2), cell(1, 3, "", 2), cell(2, 1, "大写："),
        *([] if merge else [cell(2, 0, "")]),
    ]}


def source_case(tmp_path, *, lines=ANNOUNCEMENT, grid=None, purpose="开标一览表", document_id="document"):
    artifacts = ArtifactStore(tmp_path)
    specs = [{"text": text} for text in lines]
    initial = source([*specs, {"grid": opening_grid() if grid is None else grid}], document_id=document_id)
    for unit in initial.raw["units"][:-1]:
        unit["text_basis"] = "pdf_literal"
        unit["locator"]["page_ordinal"] = 4
    original = artifacts.put_bytes(b"synthetic project-specific source", "application/pdf")
    native = SourceIndex({**initial.document, "original_ref": original}, initial.raw)
    index_ref = artifacts.put_json(native.descriptor(artifacts.put_json(native.raw)))
    frozen = artifacts.put_json({"documents": [{**native.document, "source_index_ref": index_ref,
                                               "role": "main", "status": "ready"}]})
    part = {"source_ref": {"source_index_ref": index_ref, "unit_id": f"unit:{len(lines)}"}, "selectors": []}
    form = {"target_id": "opening", "kind": "form", "purpose": purpose, "rule_refs": [],
            "source_refs": [], "template_refs": [part], "manual_fill": True, "support_origins": ["tender"]}
    outline = {"schema_version": 2, "issues": [], "chapters": [chapter("form", purpose, [form])],
               "authoring_input": {"schema_version": 2, "run_id": "opening-fixture", "source_version": 1,
                                   "domain_revision": 1, "frozen_ref": frozen}}
    proof = next((ref for ref in native.ranges if ref["text"] == f"项目名称：{NAME}"), None)
    excerpt = ({"source_ref": {"source_index_ref": index_ref, "unit_id": proof["unit_id"]},
                "selectors": [selector(proof)]} if proof else None)
    return artifacts, outline, form, part, excerpt


def word_table(artifacts, result):
    raw = artifacts.read_bytes(result["docx_ref"])
    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as archive:
        assert archive.read("投标文件.docx") == raw
    table, = Document(BytesIO(raw)).tables
    return table


def test_export_writes_only_host_proven_name_to_the_original_merged_cell(tmp_path):
    from docreader.tender.opening_project_name import attach_opening_project_name, opening_project_name_fill

    artifacts, outline, form, part, _ = source_case(tmp_path)
    attach_opening_project_name(artifacts, outline)
    assert form["opening_project_name_excerpt"]
    assert opening_project_name_fill(artifacts, outline["authoring_input"], form, part) == {
        "row": 1, "column": 0, "text": NAME,
    }
    assert form["manual_fill"] is True and form["support_origins"] == ["tender"]
    unsafe = writing("forged-task", "form", ["opening"])
    unsafe["cell_fills"] = [
        {"source_ref": part["source_ref"], "row": row, "column": column, "text": "伪造的投标信息"}
        for row, column in ((1, 0), (1, 1), (1, 2), (1, 3), (2, 0))
    ]
    saved = deepcopy(outline)

    rendered = export_authoring_document(artifacts, outline, [unsafe])

    table = word_table(artifacts, rendered)
    assert [c.text for c in table.rows[0].cells] == list(LABELS)
    assert [c.text for c in table.rows[1].cells] == [NAME, "小写：", "", ""]
    assert [c.text for c in table.rows[2].cells] == [NAME, "大写：", "", ""]
    assert table.cell(1, 0)._tc is table.cell(2, 0)._tc
    assert outline == saved


def test_export_leaves_the_valid_legacy_opening_form_without_a_binding_blank(tmp_path):
    artifacts, outline, _, _, _ = source_case(tmp_path)

    table = word_table(artifacts, export_authoring_document(artifacts, outline, []))

    assert [c.text for c in table.rows[1].cells] == ["", "小写：", "", ""]


@pytest.mark.parametrize("options", [
    pytest.param({"grid": opening_grid(name="原式已有名称")}, id="occupied"),
    pytest.param({"grid": opening_grid(name=" ")}, id="nonzero-length"),
    pytest.param({"grid": opening_grid(merge=False)}, id="two-body-anchors"),
    pytest.param({"grid": opening_grid(labels=("项目名称", "项目名称", *LABELS[2:]))}, id="duplicate-title"),
    pytest.param({"purpose": "业绩表"}, id="different-original-form"),
    pytest.param({"lines": ANNOUNCEMENT[1:]}, id="announcement-heading-missing"),
    pytest.param({"lines": (*ANNOUNCEMENT[:-1], "名称原文下一行", ANNOUNCEMENT[-1])}, id="wrapped-name"),
    pytest.param({"lines": (*ANNOUNCEMENT, "项目名称：乙区服务项目", ANNOUNCEMENT[-1])}, id="ambiguous-name"),
    pytest.param({"lines": (*ANNOUNCEMENT[:3], "项目名称：详见采购需求", ANNOUNCEMENT[-1])}, id="name-is-reference"),
])
def test_ambiguous_or_unfillable_forms_get_no_host_binding(tmp_path, options):
    from docreader.tender.opening_project_name import attach_opening_project_name

    artifacts, outline, form, _, _ = source_case(tmp_path, **options)

    attach_opening_project_name(artifacts, outline)

    assert not form.get("opening_project_name_excerpt")


@pytest.mark.parametrize("invalid_proof", ["foreign", "malformed"])
def test_output_revalidates_that_the_project_name_reference_is_in_the_frozen_main(tmp_path, invalid_proof):
    from docreader.tender.opening_project_name import opening_project_name_fill

    artifacts, outline, form, part, _ = source_case(tmp_path)
    _, _, _, _, external_excerpt = source_case(tmp_path, document_id="external")
    form["opening_project_name_excerpt"] = external_excerpt if invalid_proof == "foreign" else {"selectors": []}

    assert opening_project_name_fill(artifacts, outline["authoring_input"], form, part) is None
    table = word_table(artifacts, export_authoring_document(artifacts, outline, []))
    assert [c.text for c in table.rows[1].cells] == ["", "小写：", "", ""]


@pytest.mark.parametrize("role", ["amendment", "clarification"])
def test_supplemented_source_stays_manual_when_the_opening_name_is_unresolved(tmp_path, role):
    from docreader.tender.opening_project_name import attach_opening_project_name

    artifacts, outline, form, _, _ = source_case(tmp_path)
    _, supplement, _, _, _ = source_case(tmp_path, document_id="supplement")
    snapshot = outline["authoring_input"]
    frozen = artifacts.read_json(snapshot["frozen_ref"])
    extra, = artifacts.read_json(supplement["authoring_input"]["frozen_ref"])["documents"]
    frozen["documents"].append({**extra, "role": role})
    snapshot["frozen_ref"] = artifacts.put_json(frozen)

    attach_opening_project_name(artifacts, outline)

    assert not form.get("opening_project_name_excerpt")
