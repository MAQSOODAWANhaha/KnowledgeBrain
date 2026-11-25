"""An original cover retains its fillable fields before the generated directory."""

from typing import TypeVar
from copy import deepcopy
from io import BytesIO
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_pipeline import generate_document
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import (
    apply_requirements, compile_directory, DirectoryPlanRejected, note_missing_requirements, outline_from_compiled,
)

from test_authoring_pipeline_boundaries import budget, snapshot
from test_directory_explicit_body_integration import TemplateModel, point, window
from test_directory_compile import docs, item, node, plan
from test_directory_compile import window as source_window

_T = TypeVar("_T")


def _required(value: _T | None) -> _T:
    assert value is not None
    return value



async def test_source_cover_fields_survive_real_word_before_directory_without_fake_data(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed = snapshot(artifacts, [{"text": "投标文件\n项目名称：________\n投标人：________（盖章）\n代表：________（签字）\n日期：________"},
        {"text": "投标函\n致招标人：本处保留完整规定原式及所有原文条件，不得另行编造。"}])
    value = {"nodes": [
        {"id": "cover", "heading": point(0, 0), "response_kind": "form", "body_windows": [window(0, 0, 4)]},
        {"id": "letter", "heading": point(1, 0), "response_kind": "form", "body_windows": [window(1, 0, 1)]}],
        "scopes": [{"title": None, "items": [
            {"node_id": "cover", "depth": "self", "presentation": "cover"},
            {"node_id": "letter", "depth": "self", "presentation": "section"}]}]}
    model = TemplateModel(lambda _slot: deepcopy(value))
    outline = artifacts.read_json(await generate_document(artifacts, fixed, budget(), model, outline_only=True))
    assert outline["chapters"][0]["presentation_role"] == "cover"
    result = artifacts.read_json(await generate_document(artifacts, fixed, budget(), model, outline=outline))
    doc = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    texts = [p.text for p in doc.paragraphs]
    assert texts.index("项目名称：________") < texts.index("目录")
    assert texts.count("投标人：________（盖章）") == 1
    assert texts.count("代表：________（签字）") == 1
    assert texts.count("日期：________") == 1
    assert [p.text for p in doc.paragraphs if _required(_required(p.style).name).startswith("Heading")] == ["投标函"]
    assert not any(task.startswith("authoring_write") for task in model.calls)


def test_source_cover_cannot_adopt_a_subtree():
    with pytest.raises(DirectoryPlanRejected, match="cover cannot have child chapters"):
        compile_directory(plan([node("cover", 0, 0)], [item("cover", depth="subtree", presentation="cover")]),
                          docs(["投标文件\n项目名称：________"]))


RUBRIC = '根据文件编制质量、排列顺序和企业情况说明详细程度评分，最高10分。'


def cover_requirement_case(presentation='cover'):
    documents = docs(['封面\n项目名称：________\n投标人：________（盖章）\n代表：________（签字）',
                      '企业情况\n企业历史及人员情况：________', RUBRIC])
    nodes = [{**node('front', 0, 0), 'body_windows': [source_window(0, 0, 3)]},
             {**node('company', 1, 0), 'body_windows': [source_window(1, 0, 1)]}]
    return compile_directory(plan(nodes, [item('front', presentation=presentation), item('company')]), documents), documents


@pytest.mark.parametrize('presentation', ['cover', 'section'])
@pytest.mark.parametrize('scope', ['chapter', 'chapters'])
def test_cover_requirements_keep_compilation_guidance_without_polluting_original_form(
        tmp_path, presentation, scope):
    compiled, documents = cover_requirement_case(presentation)
    ids = ['front'] if scope == 'chapter' else ['front', 'company']
    apply_requirements(compiled, documents, [{**source_window(2, 0), 'scope': scope, 'chapter_ids': ids}])
    front, company = compiled['chapters']
    assert front.get('requirements', []) == ([] if presentation == 'cover' else [RUBRIC])
    assert company.get('requirements', []) == ([RUBRIC] if scope == 'chapters' else [])
    if scope == 'chapters':
        assert company['requirement_cover'] is True
    document_notes = [i for i in compiled['issues'] if i['code'] == 'outline_directory_document_requirement']
    assert [i['detail'] for i in document_notes] == ([RUBRIC] if presentation == 'cover' else [])
    note_missing_requirements(compiled)
    if presentation == 'cover':
        assert not any(i['code'] == 'outline_directory_requirement_missing' and '封面' in i['detail']
                       for i in compiled['issues'])
    artifacts = ArtifactStore(tmp_path)
    fixed = {'run_id': 'cover-test', 'source_version': 1, 'domain_revision': 1, 'schema_version': 2}
    result = export_authoring_document(artifacts, outline_from_compiled(fixed, compiled), [])
    doc = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    before_directory = '\n'.join(p.text for p in doc.paragraphs).split('目录', 1)[0]
    if presentation == 'cover':
        assert RUBRIC not in before_directory and '我方响应' not in before_directory
        assert all(text in before_directory for text in [
            '项目名称：________', '投标人：________（盖章）', '代表：________（签字）'])
        with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
            assert RUBRIC in package.read('提交要求与待办.md').decode()
    else:
        assert RUBRIC in '\n'.join(p.text for p in doc.paragraphs)


@pytest.mark.parametrize('ids', [['front', 'front', 'company'], ['front', 'missing']])
def test_cover_routing_preserves_unknown_references_and_deduplicates_global_guidance(ids):
    compiled, documents = cover_requirement_case()
    apply_requirements(compiled, documents, [{**source_window(2, 0), 'scope': 'chapters', 'chapter_ids': ids}])
    assert not compiled['chapters'][0].get('requirements')
    code = ('outline_directory_requirement_unassigned' if 'missing' in ids
            else 'outline_directory_document_requirement')
    assert [i['detail'] for i in compiled['issues'] if i['code'] == code] == [RUBRIC]
    assert compiled['chapters'][1].get('requirements', []) == ([] if 'missing' in ids else [RUBRIC])
