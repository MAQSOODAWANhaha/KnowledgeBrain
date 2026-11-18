"""Source decisions stay visible when enterprise writing is intentionally manual."""

from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_coverage import coverage_headline
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import compile_directory

from test_directory_compile import docs, item, node, plan, window


def outline():
    return {"chapters": [{"chapter_id": "volume", "title": "商务资料", "is_volume": True,
        "targets": [], "children": [{"chapter_id": "authorization", "title": "项目授权", "children": [],
            "targets": [{"target_id": "authorization:1", "kind": "attachment", "purpose": "代理商须附项目授权。",
                "rule_refs": [], "source_refs": [], "manual_fill": True, "support_origins": ["tender"]}]}]}],
        "issues": [
            {"code": "outline_directory_applicability_unconfirmed", "detail": "代理商须附项目授权；主体身份待核。", "rule_refs": []},
            {"code": "outline_directory_conflict", "detail": "两份计划对承诺附件的适用条件不同。", "rule_refs": []},
            {"code": "outline_directory_name", "detail": "采用原式名称，保留组成名称映射。", "rule_refs": []},
            {"code": "outline_directory_binding", "detail": "已按来源关联引用型原式。", "rule_refs": []},
        ]}


def test_manual_coverage_does_not_hide_unresolved_source_decisions():
    coverage = {"entries": [{"resolution": "manual", "gap_codes": ["coverage_unfilled"],
        "review_status": "not_required", "content_refs": []}]}
    view = coverage_headline(coverage, outline())
    assert view["pending"] == 1
    assert view["unreviewed"] == 2
    assert view["headline"] == "草稿已生成，1项待补、2项待核"


def test_real_word_keeps_source_decision_and_manual_resolution_without_duplicate_volume(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    result = export_authoring_document(artifacts, outline(), [])
    docx = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    texts = [paragraph.text for paragraph in docx.paragraphs]
    assert "适用性与来源待核" in texts
    assert texts.count("代理商须附项目授权；主体身份待核。") == 1
    assert texts.count("处理结果：________（人工核定）") == 2
    assert "采用原式名称，保留组成名称映射。" not in texts
    assert "已按来源关联引用型原式。" not in texts
    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as package:
        manifest = json.loads(package.read("manifest.json"))
        volume = Document(BytesIO(package.read(manifest["volumes"][0]["package_path"])))
        assert "适用性与来源待核" not in [paragraph.text for paragraph in volume.paragraphs]
        notes = package.read(manifest["submission_notes"]["package_path"]).decode()
        assert "适用性与来源待核" in notes
        assert "处理结果：________（人工核定）" in notes


def test_information_notices_do_not_create_source_decision_tasks(tmp_path):
    value = outline()
    value["issues"] = value["issues"][2:]
    result = export_authoring_document(ArtifactStore(tmp_path), value, [])
    document = Document(BytesIO(ArtifactStore(tmp_path).read_bytes(result["docx_ref"])))
    assert "适用性与来源待核" not in [p.text for p in document.paragraphs]


def test_unverified_source_symbol_has_a_visible_word_review_outlet(tmp_path):
    value = outline()
    detail = '第1页的私用符号未核定；请对照原件确认勾选状态并修正Word显示。'
    value['issues'] = [{'code': 'outline_directory_symbol_unconfirmed', 'detail': detail}]
    assert coverage_headline({'entries': []}, value)['unreviewed'] == 1
    artifacts = ArtifactStore(tmp_path)
    result = export_authoring_document(artifacts, value, [])
    paragraphs = [
        p.text for p in Document(BytesIO(artifacts.read_bytes(result['docx_ref']))).paragraphs]
    assert detail in paragraphs and '处理结果：________（人工核定）' in paragraphs


def test_unconfirmed_source_name_and_failed_reference_are_real_review_tasks():
    value = {"issues": [
        {"code": "outline_directory_name_unconfirmed", "detail": "标题只有编号，未能从组成清单确认名称。"},
        {"code": "outline_directory_binding_rejected", "detail": "引用关系没有落到已采用的章节。"},
        {"code": "outline_directory_extra_unconfirmed", "detail": "另一份计划多出来源章，父子关系尚未确定。"},
    ]}
    assert coverage_headline({"entries": []}, value)["unreviewed"] == 3


def test_failed_source_reference_has_a_review_exit_while_successful_reference_does_not():
    value = plan([node("sheet", 0, 0)], [item("sheet")])
    value["format_bindings"] = [{"id": "unplaced", "source": window(0, 1),
        "response_node_id": "missing", "reference_node_ids": []}]
    compiled = compile_directory(value, docs(["响应表\n报价参见响应表。\n"]))
    assert coverage_headline({"entries": []}, compiled)["unreviewed"] == 1
    value["format_bindings"][0]["response_node_id"] = "sheet"
    compiled = compile_directory(value, docs(["响应表\n报价参见响应表。\n"]))
    assert coverage_headline({"entries": []}, compiled)["unreviewed"] == 0
