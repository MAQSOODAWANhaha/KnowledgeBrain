"""Explicit semantic rejection applies independently of a model's finding code."""

from copy import deepcopy
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted, build_request
from docreader.tender.authoring_pipeline import _adopt_reviewed, generate_document
from docreader.tender.authoring_revision import revise_units
from docreader.tender.schema import AuthoringReviewResponse

from test_authoring_pipeline_boundaries import snapshot
from test_authoring_revision import bind, budget, finding, payload, response, task, unit
from test_response_table_pipeline import (
    INVENTED, SOURCES, SUPPORTED, assembly_response, raw_requirements,
    request_payload, source_response,
)


def test_reviewer_sees_a_typed_action_separate_from_its_diagnostic_code():
    request = build_request('fixture', '复核实际内容。', {}, task='authoring_review')
    from docreader.llm.contracts import output_schema

    sent = json.loads(request['messages'][0]['content'].splitlines()[-1])
    assert sent == output_schema('authoring_review', {})
    action = sent['$defs']['AuthoringReviewFinding']['properties']['reject_content']
    assert action['type'] == 'boolean' and action['default'] is False
    for supplied, expected in (({}, False), ({'reject_content': True}, True)):
        opinion, = AuthoringReviewResponse.model_validate({'findings': [{
            'code': 'free_diagnostic_code', 'detail': '具体意见', **supplied}]}).findings
        assert opinion.code == 'free_diagnostic_code' and opinion.reject_content is expected


@pytest.mark.parametrize("reject", [False, True])
def test_free_code_only_removes_content_explicitly_rejected_by_the_reviewer(tmp_path, reject):
    artifacts = ArtifactStore(tmp_path)
    original = unit()
    original["blocks"].append({**deepcopy(original["blocks"][0]), "text": "把有条件要求写成无条件。"})
    before, superseded = deepcopy(original), []
    adopted, = _adopt_reviewed([original], [{
        "code": "requirement_narrowed", "detail": "现有表达与完整条件不一致。",
        "task_ids": [original["task_id"]], "content_ids": ["b2"],
        "reject_content": reject,
    }], artifacts, superseded)

    assert original == before
    if reject:
        assert [block["text"] for block in adopted["blocks"]] == ["已有成功正文"]
        assert adopted["status"] == "generated" and adopted["gaps"]
        assert len(superseded) == 1 and artifacts.read_json(superseded[0]) == before
    else:
        assert adopted == before and superseded == []


async def test_revision_receives_the_explicit_rejection_without_reclassifying_its_code(tmp_path):
    sent = []

    async def execute(stage, request, validate):
        sent.append(request_payload(request))
        return validate(response("依据当前资料改正后的正文。"))

    await revise_units(ArtifactStore(tmp_path), budget(), execute, [task()], [unit()],
        [finding(code="requirement_narrowed", reject_content=True, content_ids=["b1"])],
        payload, bind)

    assert len(sent) == 1
    opinion, = sent[0]["review_findings"]
    assert opinion["code"] == "requirement_narrowed"
    assert opinion["reject_content"] is True and opinion["content_ids"] == ["b1"]


@pytest.mark.parametrize('invalid,expected', [
    ({'task_ids': []}, 'review_unlocated_content'),
    ({'content_ids': ['r999']}, 'review_unknown_content'),
    ({'reject_content': 'yes'}, 'bool_type'),
])
async def test_rejection_requires_valid_task_content_and_boolean(tmp_path, invalid, expected):
    artifacts = ArtifactStore(tmp_path)
    frozen = snapshot(artifacts, [{'text': text} for text in SOURCES])

    async def execute(operation, request, validate):
        value = request_payload(request)
        stage = value.get('operation', operation)
        if stage == 'outline':
            return validate(source_response(value))
        if stage == 'assemble':
            return validate(assembly_response(value, explicit=True))
        if stage == 'structure':
            return validate({'findings': []})
        if operation == 'authoring_basis':
            return validate({})
        if operation == 'authoring_write':
            return validate({'row_fills': [{'row_id': 'q1', 'text': SUPPORTED, 'support_ids': ['k1']}]})
        return validate({'findings': [{'code': 'free_code', 'detail': '明确否定已有内容。',
            'task_ids': [value['units'][0]['task_id']], 'content_ids': ['r1'],
            'reject_content': True, **invalid}]})

    with pytest.raises(ModelContractError) as caught:
        await generate_document(artifacts, frozen,
            RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384), execute,
            materials=[{'material_id': 'guide', 'title': '产品资料', 'text': SUPPORTED}])
    assert any(error['code'] == expected for error in caught.value.errors)


@pytest.mark.parametrize("outcome", ["revision_failed", "rereview_failed", "still_rejected"])
async def test_rejected_response_cannot_reappear_and_original_requirements_still_export(tmp_path, outcome):
    artifacts = ArtifactStore(tmp_path)
    frozen = snapshot(artifacts, [{"heading": "技术要求", "text": text} for text in SOURCES])
    writes, reviews = [], []

    async def execute(operation, request, validate):
        value = request_payload(request)
        stage = value.get("operation", operation)
        if stage == "outline":
            return validate(source_response(value))
        if stage == "assemble":
            return validate(assembly_response(value, explicit=True))
        if stage == "structure":
            return validate({"findings": []})
        if operation == "authoring_basis":
            return validate({"shared_arrangements": []})
        if operation == "authoring_write":
            writes.append(value)
            if "previous_content" in value and outcome == "revision_failed":
                raise ModelWorkUnitExhausted(operation)
            # Different wording ensures a real replacement is independently reviewed.
            wrong = INVENTED + ("（修订稿）" if "previous_content" in value else "")
            return validate({"row_fills": [
                {"row_id": "q1", "text": SUPPORTED, "support_ids": ["k1"]},
                {"row_id": "q2", "text": wrong, "support_ids": ["k1"]}]})
        assert operation == "authoring_review"
        reviews.append(value)
        if len(reviews) == 2 and outcome == "rereview_failed":
            raise ModelWorkUnitExhausted(operation)
        reviewed, = value["units"]
        return validate({"findings": [{
            "code": "requirement_narrowed", "detail": "现有应答增加了资料没有记载的完成事实。",
            "task_ids": [reviewed["task_id"]], "content_ids": ["r2"],
            "suggestion": "删除错误的应答，保留原要求和第一行已有功能。",
            "reject_content": True, "can_revise": True}]})

    result = artifacts.read_json(await generate_document(artifacts, frozen,
        RequestBudget(model="fixture", context_tokens=131072, output_reserve=16384), execute,
        materials=[{"material_id": "product-guide", "title": "产品资料", "text": SUPPORTED}]))

    assert len(writes) == 2 and len(reviews) == (1 if outcome == "revision_failed" else 2)
    adopted, = [artifacts.read_json(reference) for reference in result["unit_refs"]]
    assert [fill["text"] for fill in adopted["row_fills"]] == [SUPPORTED]
    assert adopted["status"] == "generated" and adopted["gaps"]
    assert any(INVENTED in fill["text"] for reference in result["superseded_unit_refs"]
               for fill in artifacts.read_json(reference)["row_fills"])
    outline = artifacts.read_json(result["outline_ref"])
    requirements = raw_requirements(artifacts, outline)
    assert requirements == [SOURCES[0], "\n".join(SOURCES[1:])]

    def assert_document(data):
        document = Document(BytesIO(data))
        assert [[cell.text for cell in row.cells] for row in document.tables[0].rows[1:]] == [
            [requirements[0], SUPPORTED], [requirements[1], ""]]
        assert INVENTED not in document.element.xml

    assert_document(artifacts.read_bytes(result["docx_ref"]))
    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as archive:
        document_names = [name for name in archive.namelist() if name.endswith(".docx")]
        assert document_names
        for name in document_names:
            assert_document(archive.read(name))
