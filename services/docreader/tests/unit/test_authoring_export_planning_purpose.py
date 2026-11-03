"""Planning instructions stay out of Word while native forms retain their text."""

from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document

from test_authoring_export import chapter, document, grid, source_ref, target, writing


@pytest.mark.parametrize("native_grid", [False, True])
def test_shared_form_instances_show_subjects_without_planning_instructions(tmp_path, native_grid):
    artifacts = ArtifactStore(tmp_path)
    fixed_text = "原格式固定说明：请按真实资料填写并签字盖章。"
    refs = [source_ref(artifacts, text=fixed_text)]
    if native_grid:
        refs.append(source_ref(artifacts, grid=grid(), document_id="native-grid"))
    positions = [
        {**target(identity, "form", refs, templates=refs), "subject": subject,
         "purpose": purpose}
        for identity, subject, purpose in [
            ("bidder", "投标人", "内部编制任务甲：完整填写并核对本表各项资料。"),
            ("manufacturer", "产品原厂", "内部编制任务乙：另行填报原厂资料，不复制企业事实。"),
        ]
    ]
    outline = {"chapters": [{**chapter("volume", "资格资料", positions), "is_volume": True}]}
    units = [writing("w-" + position["target_id"], "volume", [position["target_id"]],
                     status="placeholder") for position in positions]
    result = export_authoring_document(artifacts, outline, units)
    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as archive:
        manifest = json.loads(archive.read("manifest.json"))
        volumes = [Document(BytesIO(archive.read(entry["package_path"])))
                   for entry in manifest["volumes"]]
    assert len(volumes) == 1
    for doc in [document(artifacts, result), *volumes]:
        paragraphs = [paragraph.text for paragraph in doc.paragraphs]
        text = "\n".join(paragraphs)
        assert all(position["purpose"] not in text for position in positions)
        assert [paragraph.text for paragraph in doc.paragraphs
                if paragraph.style.name == "Caption"] == ["投标人", "产品原厂"]
        assert paragraphs.count(fixed_text) == 2
        assert len(doc.tables) == (2 if native_grid else 0)
        assert all(table.cell(1, 1).text == "" for table in doc.tables)
    assert [position["purpose"] for position in outline["chapters"][0]["targets"]] == [
        position["purpose"] for position in positions]


def test_reused_template_without_subject_keeps_copies_without_inventing_labels(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    ref = source_ref(artifacts, text="真实原格式：签署人____，日期____。")
    positions = [{**target(identity, "form", [ref], templates=[ref]),
                  "purpose": purpose} for identity, purpose in [
                      ("first", "内部任务一：按适用范围填写。"),
                      ("second", "内部任务二：核对另一填写实例。")]]
    result = export_authoring_document(artifacts, {"chapters": [chapter("body", "提交资料", positions)]}, [])
    doc = document(artifacts, result)
    paragraphs = [paragraph.text for paragraph in doc.paragraphs]
    assert all(position["purpose"] not in "\n".join(paragraphs) for position in positions)
    assert paragraphs.count("真实原格式：签署人____，日期____。") == 2
    assert not [paragraph for paragraph in doc.paragraphs if paragraph.style.name == "Caption"]


def test_missing_template_keeps_subject_and_gap_without_dumping_internal_purpose(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    position = {**target("pending", "form"), "subject": "待填主体", "purpose": "内部任务：补查原表并安排填写。"}
    result = export_authoring_document(artifacts, {"chapters": [chapter("body", "提交资料", [position])]}, [])
    paragraphs = [paragraph.text for paragraph in document(artifacts, result).paragraphs]
    assert position["purpose"] not in "\n".join(paragraphs)
    assert "待填主体" in paragraphs
    assert "（原格式待补）" in paragraphs
    assert any(gap["code"] == "export_template_missing" for gap in result["gaps"])


def test_native_text_equal_to_purpose_is_preserved_as_source_text_once(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed_text = "请按真实资料填写并完成签字盖章。"
    ref = source_ref(artifacts, text=fixed_text)
    position = {**target("form", "form", [ref], templates=[ref]),
                "subject": "投标人", "purpose": fixed_text}
    result = export_authoring_document(artifacts, {"chapters": [chapter("body", "承诺资料", [position])]}, [])
    text = "\n".join(paragraph.text for paragraph in document(artifacts, result).paragraphs)
    assert text.count(fixed_text) == 1
