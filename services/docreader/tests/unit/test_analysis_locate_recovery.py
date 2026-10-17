"""Normalize redundant source selectors without changing candidate meaning."""

from copy import deepcopy
import json

import httpx
import pytest

from docreader.llm import ModelConfig, ModelSemanticError, OpenAIModel
from docreader.tender.checks.analysis import _Analysis
from test_analysis_checks import IDENTITY, budget, scenario
from test_tender_llm import environment, response as stream_response
from test_tender_streaming import ReplayAudit


async def locate(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    payload = context.locate_requests()[0]
    ranges = [item["evidence"][0]["range_id"] for item in payload["items"][:2]]
    candidate = {"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [],
                 "range_ids": [*ranges, ranges[0]], "item_ids": [], "reason": "需联合核对两处原文。"}
    return data, context, payload, {"schema_version": 2, "candidates": [candidate], "findings": []}


@pytest.mark.asyncio
async def test_locate_deduplicates_exact_range_selectors_preserving_meaning_and_input(tmp_path):
    _, context, payload, response = await locate(tmp_path)
    original, original_payload = deepcopy(response), deepcopy(payload)

    result = context.validate(response, payload)

    expected = deepcopy(response)
    expected["candidates"][0]["range_ids"].pop()
    expected["diagnostics"] = [{"code": "candidate_range_ids_deduplicated",
                                "path": "candidates[0].range_ids",
                                "range_ids": [response["candidates"][0]["range_ids"][0]]}]
    assert result == expected
    assert response == original and payload == original_payload
    assert context.validate({**original, "candidates": expected["candidates"]}, payload) == {
        key: value for key, value in expected.items() if key != "diagnostics"}


@pytest.mark.asyncio
@pytest.mark.parametrize("failure", ["unknown", "object", "number", "empty", "not_list", "candidate_id"])
async def test_locate_deduplication_preserves_identity_and_type_checks(tmp_path, failure):
    _, context, payload, response = await locate(tmp_path)
    candidate = response["candidates"][0]
    if failure == "candidate_id":
        response["candidates"].append(deepcopy(candidate))
    elif failure == "not_list":
        candidate["range_ids"] = candidate["range_ids"][0]
    else:
        invalid = {"unknown": "not-supplied", "object": {}, "number": 7, "empty": ""}[failure]
        candidate["range_ids"].extend([invalid, invalid])
    original = deepcopy(response)

    with pytest.raises(ModelSemanticError):
        context.validate(response, payload)

    assert response == original


@pytest.mark.asyncio
async def test_locate_recovers_successful_transport_cache_after_host_rejection(tmp_path, monkeypatch):
    data, context, payload, response = await locate(tmp_path)
    class WireAudit(ReplayAudit):
        async def commit_artifact(self, lease, stage, ref, **kwargs):
            assert stage == "model-wire"

    audit, sent = WireAudit(), []

    def handler(request):
        sent.append(request)
        supplied = json.loads(json.loads(request.content)["messages"][1]["content"][0]["text"])
        ranges = [item["evidence"][0]["range_id"] for item in supplied["items"][:2]]
        reply = deepcopy(response)
        reply["candidates"][0].update(candidate_id=supplied["work_unit_id"] + ":c1",
                                       range_ids=[*ranges, ranges[0]])
        return stream_response(json.dumps(reply, ensure_ascii=False))

    def client():
        return OpenAIModel(ModelConfig.from_env(environment()), audit, object(),
                           {"verify": "Locate related supplied sources and return JSON."},
                           artifacts=data[0], transport=httpx.MockTransport(handler))

    validate = context.validate

    def legacy_rejection(value, supplied):
        assert value == response and supplied == payload
        raise ModelSemanticError("Analysis response violated its version-two operation contract")

    monkeypatch.setattr(context, "validate", legacy_rejection)
    async with client() as first:
        with pytest.raises(ModelSemanticError):
            await context.execute("locate", [payload], first, data[3])
    original_audit = deepcopy(audit.rows)
    row = next(iter(original_audit.values()))
    assert row["status"] == "succeeded" and row["usage"]["total_tokens"] == 20
    raw = data[0].read_json(row["response_ref"])
    assert context.operation_refs == []

    monkeypatch.setattr(context, "validate", validate)
    async with client() as recovered:
        _, results = await context.execute("locate", [payload], recovered, data[3])
        _, cached = await context.execute("locate", [payload], recovered, data[3])
        assert recovered.usage.snapshot()["requests"] == 0

    assert len(sent) == 1 and audit.rows == original_audit
    assert data[0].read_json(row["response_ref"]) == raw
    assert results == cached
    assert results[0]["candidates"][0]["range_ids"] == response["candidates"][0]["range_ids"][:2]
    assert results[0]["diagnostics"][0]["code"] == "candidate_range_ids_deduplicated"
