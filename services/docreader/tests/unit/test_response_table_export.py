"""Business response rows preserve complete source text and leave unknown answers blank."""

from typing import TypeVar
from copy import deepcopy
from io import BytesIO

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_excerpts import load_source_excerpt
from docreader.tender.evidence.references import EvidenceRepository, SourceIndex, fragment, selector

_T = TypeVar("_T")


def _required(value: _T | None) -> _T:
    assert value is not None
    return value



def excerpts(artifacts, texts):
    units = [{"unit_id": f"page:{ordinal}", "kind": "section", "ordinal": ordinal,
              "text": text, "locator": {"locator_kind": "document", "section_ordinal": ordinal}}
             for ordinal, text in enumerate(texts)]
    raw = {"schema_version": 2, "units": units, "images": {}, "ocr": {},
           "source_contract": {"page_manifest": [], "units": [
               {"unit_id": unit["unit_id"], "section_id": "technical", "parent_section_id": None,
                "heading_path": "项目要求", "physical_locator": unit["locator"]} for unit in units]}}
    original = {"document_id": "source", "document_version": "1",
                "original_ref": artifacts.put_bytes(b"synthetic source")}
    index = SourceIndex(original, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(raw)))
    result = []
    for entry in index.ranges:
        selected = {**selector(entry), "source_range_id": entry["range_id"]}
        result.append({"source_ref": {"source_index_ref": index_ref, "unit_id": entry["unit_id"]},
                       "selectors": [selected]})
    return result


def outline(rows, **fields):
    target = {"target_id": "technical", "kind": "narrative", "purpose": "技术逐项应答",
              "rule_refs": [], "source_refs": [], "response_table": {"rows": rows}, **fields}
    return {"chapters": [{"chapter_id": "chapter", "title": "技术响应", "targets": [target], "children": []}]}


def unit(**fields):
    return {"schema_version": 2, "task_id": "task", "chapter_id": "chapter", "target_ids": ["technical"],
            "status": "placeholder", "blocks": [], "cell_fills": [], "row_fills": [],
            "attachments": [], "gaps": [], **fields}


def opened(artifacts, result):
    return Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))


def test_complete_cross_page_rows_are_exported_without_knowledge_or_generic_placeholder(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    selected = excerpts(artifacts, ["系统应支持协同防御，或提供定制开发承", "诺，盖章并附详细方案。", "另须提供独立功能证明。"])
    rows = [{"row_id": "cooperation", "source_excerpts": selected[:2], "rule_refs": []},
            {"row_id": "proof", "source_excerpts": selected[2:], "rule_refs": []}]
    result = export_authoring_document(artifacts, outline(rows), [unit()])
    doc = opened(artifacts, result)
    assert len(doc.tables) == 1
    assert [cell.text for cell in doc.tables[0].rows[0].cells] == ["招标要求", "投标响应"]
    assert [cell.text for cell in doc.tables[0].rows[1].cells] == ["系统应支持协同防御，或提供定制开发承\n诺，盖章并附详细方案。", ""]
    assert [cell.text for cell in doc.tables[0].rows[2].cells] == ["另须提供独立功能证明。", ""]
    assert not any("本部分内容待补" in p.text for p in doc.paragraphs)
    assert not any(gap["code"] == "export_content_pending" for gap in result["gaps"])


@pytest.mark.parametrize('manual_fill', [False, True])
def test_form_response_rows_are_a_real_table_without_native_template_missing_warning(tmp_path, manual_fill):
    artifacts = ArtifactStore(tmp_path)
    selected = excerpts(artifacts, ['完整技术要求及其适用条件。'])
    rows = [{'row_id': 'requirement', 'source_excerpts': selected, 'rule_refs': []}]
    result = export_authoring_document(artifacts,
        outline(rows, kind='form', manual_fill=manual_fill), [unit()])
    doc = opened(artifacts, result)
    assert len(doc.tables) == 1
    assert [cell.text for cell in doc.tables[0].rows[1].cells] == ['完整技术要求及其适用条件。', '']
    assert '（原格式待补）' not in [p.text for p in doc.paragraphs]
    assert not any(gap['code'] == 'export_template_missing' for gap in result['gaps'])


def test_response_requirements_hide_parser_headings_without_changing_cross_page_sources(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    texts = ["#### （1）协同能力至少支持30Mbps。", "### 续页要求：提供接口 #1，不以型号代替功能。"]
    selected = excerpts(artifacts, texts)
    frozen_refs = deepcopy(selected)
    frozen_indexes = [artifacts.read_bytes(ref["source_ref"]["source_index_ref"]) for ref in selected]
    rows = [{"row_id": "cross-page", "source_excerpts": selected, "rule_refs": []}]

    doc = opened(artifacts, export_authoring_document(artifacts, outline(rows), [unit()]))

    assert [cell.text for cell in doc.tables[0].rows[1].cells] == [
        "（1）协同能力至少支持30Mbps。\n续页要求：提供接口 #1，不以型号代替功能。", ""]
    assert selected == frozen_refs
    assert [load_source_excerpt(artifacts, ref)["text"] for ref in selected] == texts
    assert [artifacts.read_bytes(ref["source_ref"]["source_index_ref"]) for ref in selected] == frozen_indexes


def test_row_answers_join_by_frozen_identity_across_units_and_conflicts_only_blank_their_row(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    selected = excerpts(artifacts, ["要求甲。", "要求乙。", "要求丙。"])
    rows = [{"row_id": identity, "source_excerpts": [excerpt], "rule_refs": []}
            for identity, excerpt in zip(("a", "b", "c"), selected)]
    first = unit(status="generated", row_fills=[{"row_id": "b", "text": "有据乙。", "material_ids": ["knowledge"]},
                                               {"row_id": "a", "text": "甲答案一。", "material_ids": ["knowledge"]}])
    second = unit(task_id="task-two", status="generated", row_fills=[
        {"row_id": "a", "text": "甲答案二。", "material_ids": ["knowledge"]},
        {"row_id": "unknown", "text": "不得串入正文。", "material_ids": ["knowledge"]}])
    result = export_authoring_document(artifacts, outline(rows), [first, second])
    doc = opened(artifacts, result)
    assert len(doc.tables) == 1
    assert [[cell.text for cell in row.cells] for row in doc.tables[0].rows[1:]] == [
        ["要求甲。", ""], ["要求乙。", "有据乙。"], ["要求丙。", ""]]
    assert "不得串入正文。" not in doc.element.xml
    assert {gap["code"] for gap in result["gaps"]} >= {"export_conflicting_row_fills", "export_row_fill_unplaced"}


def test_manual_response_table_keeps_requirements_and_ignores_answers(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    selected = excerpts(artifacts, ["本次报价上限及要求不得丢失。"])
    rows = [{"row_id": "price", "source_excerpts": selected, "rule_refs": []}]
    result = export_authoring_document(artifacts, outline(rows, manual_fill=True), [unit(
        status="generated", row_fills=[{"row_id": "price", "text": "123元", "material_ids": ["historical-price"]}])])
    doc = opened(artifacts, result)
    assert doc.tables[0].cell(1, 0).text == "本次报价上限及要求不得丢失。"
    assert doc.tables[0].cell(1, 1).text == ""
    assert "123元" not in doc.element.xml


def test_row_table_uses_selected_excerpt_not_entire_native_unit(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    selected = excerpts(artifacts, ["前文背景。必须支持双机热备。后文无关说明。"])
    source = EvidenceRepository(artifacts).source_index(selected[0]["source_ref"]["source_index_ref"])
    selected[0]["selectors"] = [selector(fragment(source.ranges[0], 5, 14))]
    rows = [{"row_id": "ha", "source_excerpts": selected, "rule_refs": []}]
    doc = opened(artifacts, export_authoring_document(artifacts, outline(rows), []))
    assert doc.tables[0].cell(1, 0).text == "必须支持双机热备。"
    assert "前文背景" not in doc.element.xml and "后文无关" not in doc.element.xml


def test_following_response_table_shows_the_previous_bound_condition(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    selected = excerpts(artifacts, ["指标要求（同等及以上配置）", "第二表第一条要求。"])
    first = outline([{"row_id": "one", "source_excerpts": selected[1:], "rule_refs": []}])
    first["chapters"][0]["targets"][0]["response_table"]["context_source_excerpts"] = selected[:1]
    second = {"target_id": "more", "kind": "form", "purpose": "续表", "rule_refs": [], "source_refs": [],
              "response_table": {"rows": [{"row_id": "two", "source_excerpts": selected[1:], "rule_refs": []}]}}
    first["chapters"].append({"chapter_id": "next", "title": "后续技术要求", "targets": [second], "children": []})
    doc = opened(artifacts, export_authoring_document(artifacts, first, []))
    body = "\n".join(paragraph.text for paragraph in doc.paragraphs)
    assert body.count("指标要求（同等及以上配置）") == 2


def test_bound_narrative_requirement_is_shown_and_answer_stays_blank(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    selected = excerpts(artifacts, ["按照技术规范逐段应答，并说明是否接受各项技术要求。"])
    tree = {"chapters": [{"chapter_id": "tech", "title": "技术总则", "children": [], "targets": [
        {"target_id": "narrative", "kind": "narrative", "purpose": "逐段应答",
         "rule_refs": [{"decision_id": "shown-rule"}], "source_refs": [], "source_excerpts": selected}]}]}
    doc = opened(artifacts, export_authoring_document(artifacts, tree, [unit(chapter_id="tech", target_ids=["narrative"])]))
    body = "\n".join(paragraph.text for paragraph in doc.paragraphs)
    assert "按照技术规范逐段应答" in body
    assert "本部分内容待补" not in body


def test_unmatched_narrative_does_not_invent_a_general_or_special_chapter(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    selected = excerpts(artifacts, ["专用设备参数见技术规范，本次未提供企业实现。"])
    tree = {"chapters": [{"chapter_id": "tech", "title": "技术文件", "is_volume": True, "children": [
        {"chapter_id": "item", "title": "技术附表A 供货范围及可选配置", "children": [], "targets": [
            {"target_id": "scope", "kind": "narrative", "purpose": "供货范围",
             "rule_refs": [{"decision_id": "shown-rule"}], "source_refs": [], "source_excerpts": selected}]}], "targets": []}]}
    doc = opened(artifacts, export_authoring_document(artifacts, tree, [unit(chapter_id="item", target_ids=["scope"])]))
    headings = [paragraph.text for paragraph in doc.paragraphs if _required(_required(paragraph.style).name).startswith("Heading")]
    assert any(text.endswith("供货范围及可选配置") for text in headings)
    assert not any("技术附表A" in text for text in headings)
    assert not any(text.endswith("通用部分") or text.endswith("专用部分") for text in headings)
    body = "\n".join(paragraph.text for paragraph in doc.paragraphs)
    assert "专用设备参数见技术规范" in body
    assert "拟采用" not in body and "本部分内容待补" not in body


def test_scoring_row_excerpts_render_the_native_grid_with_blank_answers(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    cells = [{"row": 0, "column": column, "row_span": 1, "col_span": 1, "text": text}
             for column, text in enumerate(["序号", "评分因素", "分值", "评分标准"])]
    cells.extend([
        {"row": 1, "column": 0, "row_span": 1, "col_span": 1, "text": "16"},
        {"row": 1, "column": 1, "row_span": 1, "col_span": 1, "text": ""},
        {"row": 1, "column": 2, "row_span": 1, "col_span": 1, "text": ""},
        {"row": 1, "column": 3, "row_span": 1, "col_span": 1, "text": "需提供原厂经审计的财务报表。"},
        {"row": 2, "column": 0, "row_span": 1, "col_span": 1, "text": "2"},
        {"row": 2, "column": 1, "row_span": 1, "col_span": 1, "text": "企业经营状况"},
        {"row": 2, "column": 2, "row_span": 1, "col_span": 1, "text": "20"},
        {"row": 2, "column": 3, "row_span": 1, "col_span": 1, "text": "按近三年财务状况打分。"},
    ])
    unit_id = "page:40:table:0"
    native = {"unit_id": unit_id, "kind": "table_region", "ordinal": 0, "text": "",
              "grid": {"row_count": 3, "column_count": 4, "cells": cells},
              "locator": {"locator_kind": "document", "section_ordinal": 0, "table_ordinal": 0}}
    raw = {"schema_version": 2, "units": [native], "images": {}, "ocr": {}, "source_contract": {
        "page_manifest": [], "units": [{"unit_id": unit_id, "section_id": "score",
            "parent_section_id": None, "heading_path": "评分", "physical_locator": native["locator"]}]}}
    document = {"document_id": "score", "document_version": "1", "original_ref": artifacts.put_bytes(b"score")}
    index = SourceIndex(document, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    rows = [row for row in index.ranges if row.get("row_start") in (1, 2)]
    selected = [{"source_ref": {"source_index_ref": index_ref, "unit_id": unit_id}, "selectors": [selector(row)]}
                for row in rows]
    tree = {"chapters": [{"chapter_id": "biz", "title": "资格审查", "children": [], "targets": [
        {"target_id": "score", "kind": "narrative", "purpose": "财务评分",
         "rule_refs": [{"decision_id": "shown-rule"}], "source_refs": [], "source_excerpts": selected}]}]}
    doc = opened(artifacts, export_authoring_document(artifacts, tree, [unit(chapter_id="biz", target_ids=["score"])]))
    assert len(doc.tables) == 1
    table = doc.tables[0]
    assert [cell.text for cell in table.rows[0].cells] == ["序号", "评分因素", "分值", "评分标准"]
    assert [cell.text for cell in table.rows[2].cells] == ["2", "企业经营状况", "20", "按近三年财务状况打分。"]
    assert " | " not in "\n".join(paragraph.text for paragraph in doc.paragraphs)
    assert all(" | " not in cell.text for row in table.rows for cell in row.cells)


def _section_excerpts(artifacts, text, *, word_paragraphs=False):
    unit_id = "section:wrapped"
    native = {"unit_id": unit_id, "kind": "section", "ordinal": 0, "text": text,
              "locator": {"locator_kind": "document", "section_ordinal": 0}}
    if word_paragraphs:
        native["physical_path"] = ";".join(
            f"/word/document.xml/body/*[{index}]" for index in range(1, text.count("\n") + 2))
    raw = {"schema_version": 2, "units": [native], "images": {}, "ocr": {}, "source_contract": {
        "page_manifest": [], "units": [{"unit_id": unit_id, "section_id": "clause",
            "parent_section_id": None, "heading_path": "财务", "physical_locator": native["locator"]}]}}
    document = {"document_id": "clause", "document_version": "1", "original_ref": artifacts.put_bytes(b"clause")}
    index = SourceIndex(document, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    base = index.ranges[0]
    slices, cursor = [], 0
    for mark in [offset for offset, char in enumerate(text) if char == "\n"] + [len(text)]:
        if mark > cursor:
            slices.append(fragment(base, cursor, mark))
        cursor = mark + 1
    return [{"source_ref": {"source_index_ref": index_ref, "unit_id": unit_id}, "selectors": [selector(piece)]}
            for piece in slices]


def test_pdf_line_wraps_join_but_numbered_items_stay_separate(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    text = "12. 前一条款结束。\n### 13. 需提供原厂财务报表（包含资产负债表、现金流量\n表、利润表等）。\n14. 另一条款保持独立"
    selected = _section_excerpts(artifacts, text)
    tree = {"chapters": [{"chapter_id": "biz", "title": "财务", "children": [], "targets": [
        {"target_id": "clause", "kind": "narrative", "purpose": "财务报表",
         "rule_refs": [{"decision_id": "shown-rule"}], "source_refs": [], "source_excerpts": selected}]}]}
    doc = opened(artifacts, export_authoring_document(artifacts, tree, [unit(chapter_id="biz", target_ids=["clause"])]))
    paragraphs = [paragraph.text for paragraph in doc.paragraphs if paragraph.text]
    assert "13. 需提供原厂财务报表（包含资产负债表、现金流量表、利润表等）。" in paragraphs
    assert "14. 另一条款保持独立" in paragraphs
    assert "12. 前一条款结束。" in paragraphs
    assert not any(paragraph.startswith("表、") or paragraph == "过）" or "###" in paragraph for paragraph in paragraphs)


def test_docx_paragraph_breaks_stay_separate_without_a_period(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    text = "前一段没有句号\n后一段仍应分开"
    selected = _section_excerpts(artifacts, text, word_paragraphs=True)
    tree = {"chapters": [{"chapter_id": "biz", "title": "说明", "children": [], "targets": [
        {"target_id": "clause", "kind": "narrative", "purpose": "分段",
         "rule_refs": [{"decision_id": "shown-rule"}], "source_refs": [], "source_excerpts": selected}]}]}
    doc = opened(artifacts, export_authoring_document(artifacts, tree, [unit(chapter_id="biz", target_ids=["clause"])]))
    paragraphs = [paragraph.text for paragraph in doc.paragraphs if paragraph.text]
    assert "前一段没有句号" in paragraphs
    assert "后一段仍应分开" in paragraphs
    assert "前一段没有句号后一段仍应分开" not in paragraphs


def _template_reference(artifacts, text, *, word_paragraphs=False, grid=None):
    unit_id = "template:body"
    native = {"unit_id": unit_id, "kind": "table_region" if grid else "section", "ordinal": 0,
              "text": "" if grid else text, "grid": grid,
              "locator": {"locator_kind": "document", "section_ordinal": 0}}
    if word_paragraphs:
        native["physical_path"] = ";".join(
            f"/word/document.xml/body/*[{index}]" for index in range(1, text.count("\n") + 2))
    raw = {"schema_version": 2, "units": [native], "images": {}, "ocr": {}, "source_contract": {
        "page_manifest": [], "units": [{"unit_id": unit_id, "section_id": "form",
            "parent_section_id": None, "heading_path": "格式", "physical_locator": native["locator"]}]}}
    document = {"document_id": "form", "document_version": "1", "original_ref": artifacts.put_bytes(b"form")}
    index = SourceIndex(document, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    return {"source_index_ref": index_ref, "unit_id": unit_id}


def test_template_prose_uses_the_same_wrap_and_paragraph_rules(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    wrapped = _template_reference(artifacts, "需提供现金流量\n表。")
    separated = _template_reference(artifacts, "前一段没有句号\n后一段仍应分开", word_paragraphs=True)
    table = {"row_count": 1, "column_count": 2, "cells": [
        {"row": 0, "column": 0, "row_span": 1, "col_span": 1, "text": "序号"},
        {"row": 0, "column": 1, "row_span": 1, "col_span": 1, "text": "合格条件"}]}
    grid_ref = _template_reference(artifacts, "", grid=table)
    tree = {"chapters": [{"chapter_id": "form", "title": "格式", "children": [], "targets": [
        {"target_id": "wrap", "kind": "form", "purpose": "折行", "rule_refs": [], "source_refs": [],
         "template_refs": [{"source_ref": wrapped, "selectors": []}]},
        {"target_id": "split", "kind": "form", "purpose": "真段落", "rule_refs": [], "source_refs": [],
         "template_refs": [{"source_ref": separated, "selectors": []}]},
        {"target_id": "grid", "kind": "form", "purpose": "原表", "rule_refs": [], "source_refs": [],
         "template_refs": [{"source_ref": grid_ref, "selectors": []}]}]}]}
    doc = opened(artifacts, export_authoring_document(artifacts, tree, [unit(chapter_id="form", target_ids=["wrap", "split", "grid"])]))
    paragraphs = [paragraph.text for paragraph in doc.paragraphs if paragraph.text]
    assert "需提供现金流量\n表。" in paragraphs
    assert "前一段没有句号" in paragraphs and "后一段仍应分开" in paragraphs
    assert "前一段没有句号后一段仍应分开" not in paragraphs
    assert len(doc.tables) == 1
    assert [cell.text for cell in doc.tables[0].rows[0].cells] == ["序号", "合格条件"]


def test_citation_lines_stay_separate_while_wrapped_sentences_join(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    text = "具体设计依据以下标准、规范进行：\n《中华人民共和国网络安全法》\n《中华人民共和国数据安全法》\n需提供现金流量\n表。"
    selected = _section_excerpts(artifacts, text)
    tree = {"chapters": [{"chapter_id": "biz", "title": "标准", "children": [], "targets": [
        {"target_id": "clause", "kind": "narrative", "purpose": "适用标准",
         "rule_refs": [{"decision_id": "shown-rule"}], "source_refs": [], "source_excerpts": selected}]}]}
    doc = opened(artifacts, export_authoring_document(artifacts, tree, [unit(chapter_id="biz", target_ids=["clause"])]))
    paragraphs = [paragraph.text for paragraph in doc.paragraphs if paragraph.text]
    assert "《中华人民共和国网络安全法》" in paragraphs
    assert "《中华人民共和国数据安全法》" in paragraphs
    assert "需提供现金流量表。" in paragraphs
    assert not any("网络安全法》《" in paragraph for paragraph in paragraphs)


def test_grid_response_rows_keep_native_columns_and_a_blank_answer(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    cells = [{"row": 0, "column": column, "row_span": 1, "col_span": 1, "text": text}
             for column, text in enumerate(["序号", "指标项", "指标要求（同等及以上配置）"])]
    cells.extend([
        {"row": 1, "column": 0, "row_span": 1, "col_span": 1, "text": "规格配置"},
        {"row": 1, "column": 1, "row_span": 1, "col_span": 1, "text": "规格"},
        {"row": 1, "column": 2, "row_span": 1, "col_span": 1, "text": "★网络处理能力≥20Gbps"},
        {"row": 2, "column": 0, "row_span": 1, "col_span": 1, "text": "网络协议"},
        {"row": 2, "column": 1, "row_span": 1, "col_span": 1, "text": "网络协议"},
        {"row": 2, "column": 2, "row_span": 1, "col_span": 1, "text": "支持 VTEP 模式接入"},
    ])
    unit_id = "page:56:table:0"
    native = {"unit_id": unit_id, "kind": "table_region", "ordinal": 0, "text": "",
              "grid": {"row_count": 3, "column_count": 3, "cells": cells},
              "locator": {"locator_kind": "document", "section_ordinal": 0, "table_ordinal": 0}}
    raw = {"schema_version": 2, "units": [native], "images": {}, "ocr": {}, "source_contract": {
        "page_manifest": [], "units": [{"unit_id": unit_id, "section_id": "spec",
            "parent_section_id": None, "heading_path": "技术参数", "physical_locator": native["locator"]}]}}
    document = {"document_id": "spec", "document_version": "1", "original_ref": artifacts.put_bytes(b"spec")}
    index = SourceIndex(document, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    row_ranges = [row for row in index.ranges if row.get("row_start") in (0, 1, 2)]
    excerpts = [{"source_ref": {"source_index_ref": index_ref, "unit_id": unit_id}, "selectors": [selector(row)]}
                for row in row_ranges]
    tree = {"chapters": [{"chapter_id": "tech", "title": "技术参数", "children": [], "targets": [
        {"target_id": "spec", "kind": "form", "purpose": "逐项响应", "rule_refs": [], "source_refs": [],
         "response_table": {"context_source_excerpts": excerpts[:1],
                             "rows": [{"row_id": "hardware", "source_excerpts": [excerpts[1]], "rule_refs": []},
                                       {"row_id": "protocol", "source_excerpts": [excerpts[2]], "rule_refs": []}]}}]}]}
    doc = opened(artifacts, export_authoring_document(artifacts, tree, [unit(chapter_id="tech", target_ids=["spec"])]))
    assert len(doc.tables) == 1
    table = doc.tables[0]
    assert [cell.text for cell in table.rows[0].cells] == ["序号", "指标项", "指标要求（同等及以上配置）", "投标响应"]
    assert [cell.text for cell in table.rows[1].cells] == ["规格配置", "规格", "★网络处理能力≥20Gbps", ""]
    assert " | " not in "\n".join(cell.text for row in table.rows for cell in row.cells)
    assert not any("序号 |" in paragraph.text for paragraph in doc.paragraphs)
