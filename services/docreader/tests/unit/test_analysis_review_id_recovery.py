"""Host-owned review scope replaces the old model ID transcription recovery."""
from copy import deepcopy

import pytest

from docreader.llm import ModelSemanticError
from docreader.tender.checks.analysis import _Analysis
from docreader.tender.evidence.references import fragment
from test_analysis_checks import IDENTITY, Model, budget, range_ids, scenario


async def fragmented_review(tmp_path, *, key="body", include_parent=False):
    data = await scenario(tmp_path, documents=[
        ("main", "main", [("负责人应持A证，并提供证书复印件。", "obligation"),
                          ("售后服务应及时响应。", "obligation")]),
        ("hidden", "attachment", [("未装载的附件要求。", "obligation")]),
    ])
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    native, other = context.indices["main"].ranges
    excerpt = fragment(native, 0, len(native["text"]) // 2)
    context.sources[excerpt["range_id"]] = excerpt
    selected = [excerpt["range_id"], other["range_id"]]
    if include_parent:
        selected.append(native["range_id"])
    views = context.views(selected)
    if key == "supporting":
        value = next(value for value in views[0]["body"] if value["range_id"] == excerpt["range_id"])
        views[0]["body"].remove(value)
        views[0]["supporting"].append(value)
    payload = context.payload("review", "review-1", source_views=views)
    response = await Model().call("verify", payload)
    assert native["range_id"] != excerpt["range_id"]
    assert any(value.get("source_range_id") == native["range_id"]
               for view in views for value in view[key])
    return data, context, payload, response, native["range_id"], excerpt["range_id"]


@pytest.mark.asyncio
@pytest.mark.parametrize("key", ["body", "supporting"])
@pytest.mark.parametrize("include_parent", [False, True])
async def test_scope_is_derived_from_actual_sent_ranges(tmp_path, key, include_parent):
    _, context, payload, response, parent, excerpt = await fragmented_review(
        tmp_path, key=key, include_parent=include_parent)
    original = deepcopy(response)
    result = context.validate(response, payload)
    assert result['reviewed_range_ids'] == range_ids(payload['source_views'])
    assert excerpt in result['reviewed_range_ids']
    assert (parent in result['reviewed_range_ids']) is include_parent
    assert response == original
    assert set(response) == {'schema_version', 'accepted_decision_ids', 'findings'}


@pytest.mark.asyncio
async def test_missing_source_is_reported_without_forging_scope(tmp_path):
    _, context, payload, response, _, excerpt = await fragmented_review(tmp_path)
    response['findings'] = [{'code': 'source_unreadable', 'detail': '该范围文字不完整。',
                             'blocking': True, 'range_id': excerpt}]
    result = context.validate(response, payload)
    assert result['findings'] == response['findings']
    assert result['reviewed_range_ids'] == range_ids(payload['source_views'])


@pytest.mark.asyncio
async def test_unknown_findings_and_decisions_still_fail(tmp_path):
    _, context, payload, response, _, _ = await fragmented_review(tmp_path)
    response['accepted_decision_ids'] = ['unknown']
    with pytest.raises(ModelSemanticError):
        context.validate(response, payload)
    response['accepted_decision_ids'] = []
    response['findings'] = [{'code': 'gap', 'detail': 'unseen', 'blocking': True, 'range_id': 'unseen'}]
    with pytest.raises(ModelSemanticError):
        context.validate(response, payload)


@pytest.mark.asyncio
@pytest.mark.parametrize("claimed", [[], ["unknown"], ["duplicate", "duplicate"], None, "bad-list"])
async def test_redundant_model_scope_is_ignored_not_used_as_coverage(tmp_path, claimed):
    _, context, payload, response, _, _ = await fragmented_review(tmp_path)
    response['reviewed_range_ids'] = claimed
    result = context.validate(response, payload)
    assert result['reviewed_range_ids'] == range_ids(payload['source_views'])
    assert result['diagnostics'] == [{'code': 'model_review_scope_ignored', 'fields': ['reviewed_range_ids']}]
