"""Generated output schemas travel through the real budget, transport and replay path."""

from copy import deepcopy
import json
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.contracts import ExtractionOutput
from docreader.llm.request import (
    ModelContractError, ModelWorkUnitExhausted, build_request, request_digest,
)
from docreader.tender.authoring_execution import _OutlineModel
from docreader.tender.schema import AnalysisReviewResponse, Extraction, OutlineResponse, WritingResponse

from test_authoring_send_admission import JournalAudit
from test_tender_llm import CountedArtifacts, model, response
from test_tender_streaming import ReplayAudit


@pytest.fixture
def schema_cache():
    from docreader.llm.contracts import _schema_text

    _schema_text.cache_clear()
    yield _schema_text
    _schema_text.cache_clear()


def assert_embedded_schema(request, response_type):
    prompt = request["messages"][0]["content"]
    expected = response_type.model_json_schema(mode="validation")
    decoder, found = json.JSONDecoder(), []
    for start, character in enumerate(prompt):
        if character != "{":
            continue
        try:
            value, _ = decoder.raw_decode(prompt[start:])
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict) and value.get("type") == "object":
            found.append(value)
    assert found.count(expected) == 1, "The actual system prompt must carry the complete source schema exactly once"
    return expected


async def test_analysis_client_sends_generated_schema_and_measures_the_same_body(tmp_path):
    calls, audit = [], ReplayAudit()
    payload = {"source": "需要保留的原始输入"}
    client, _ = model(lambda sent: (calls.append(json.loads(sent.content)), response())[1],
                      audit, artifacts=CountedArtifacts(tmp_path))
    async with client:
        assert await client.call("extract", payload) == {"result": "ok"}
    request, = calls
    assert_embedded_schema(request, Extraction)
    assert json.loads(request["messages"][1]["content"][0]["text"]) == payload
    assert request["response_format"] == {"type": "json_object"}
    assert request["stream"] is True
    assert request["stream_options"] == {"include_usage": True}
    assert "max_tokens" not in request and "max_completion_tokens" not in request
    measured = client.request_budget().measure_request(request, "extract")
    metadata = next(iter(audit.rows.values()))["metadata"]
    for name in ("input_tokens", "text_tokens", "wire_bytes", "total_context_tokens"):
        assert metadata[name] == measured[name]
    legacy = build_request(client.config.chat.model, "Return JSON evidence.", payload)
    baseline = client.request_budget().measure_request(legacy, "extract")
    schema_bytes = len(request["messages"][0]["content"].encode()) - len(legacy["messages"][0]["content"].encode())
    assert measured["input_tokens"] - baseline["input_tokens"] == schema_bytes > 0


async def test_strict_v2_extraction_uses_one_schema_and_pins_correction_identity(tmp_path):
    calls, audit = [], JournalAudit()
    payload = {"schema_version": 2, "work_unit_id": "pack-test"}
    feedback = {"errors": [{"path": ["dispositions"], "code": "missing", "detail": "Field required"}]}
    client, _ = model(lambda sent: (calls.append(json.loads(sent.content)), response())[1],
                      audit, artifacts=CountedArtifacts(tmp_path))
    async with client:
        await client.call("extract", payload)
        await client.call("extract", {**payload, "contract_feedback": feedback})

    budget = client.request_budget()
    first, correction = calls
    assert first == budget.request("extract", payload)
    assert correction == budget.request("extract", {**payload, "contract_feedback": feedback})
    assert first["response_format"] == correction["response_format"] == {
        "type": "json_schema",
        "json_schema": {"name": "tender_extraction", "strict": True,
                        "schema": ExtractionOutput.model_json_schema(mode="validation")},
    }
    assert first["messages"][0]["content"] == "Return JSON evidence."
    assert budget.spec()["extract_response_format"] == "json_schema"
    assert client.execution_spec()["budget"]["chat"] == budget.spec()
    legacy = budget.request("extract", {})
    assert legacy["response_format"] == {"type": "json_object"}
    assert_embedded_schema(legacy, Extraction)
    verify = RequestBudget(prompts={"verify": "Review."}).request(
        "verify", {"schema_version": 2}
    )
    assert verify["response_format"] == {"type": "json_object"}
    assert_embedded_schema(verify, AnalysisReviewResponse)

    digest = request_digest(client.config.chat.base_url, first)
    assert request_digest(client.config.chat.base_url, correction) != digest
    assert {row["metadata"]["logical_request_digest"] for row in audit.rows.values()} == {digest}
    measured = budget.measure_request(first, "extract")
    row = next(row for row in audit.rows.values() if row["metadata"]["request_digest"] == digest)
    assert row["metadata"]["input_tokens"] == measured["input_tokens"]


async def test_changed_schema_has_a_new_digest_and_cannot_claim_the_old_paid_cache(tmp_path, monkeypatch, schema_cache):
    budget = RequestBudget(model="gpt-4o", context_tokens=32768, prompts={}, output_reserve=4096)
    payload = {"schema_version": 2, "sources": [], "rules": []}
    old_request = build_request(budget.model, "Organize supplied sources.", payload, task="outline")
    calls, audit = [], ReplayAudit()
    client, _ = model(lambda sent: (calls.append(json.loads(sent.content)), response())[1],
                      audit, artifacts=CountedArtifacts(tmp_path))
    spec = {"protocol": "generated-output-schema-test"}
    async with client:
        for _ in range(2):
            await client.call_request(old_request, budget, task="outline", execution_spec=spec)
        original_schema = OutlineResponse.model_json_schema

        def changed_schema(cls, *args, **kwargs):
            value = original_schema(*args, **kwargs)
            value["description"] = "A new frozen schema revision for this synthetic test."
            return value

        monkeypatch.setattr(OutlineResponse, "model_json_schema", classmethod(changed_schema))
        schema_cache.cache_clear()
        new_request = build_request(budget.model, "Organize supplied sources.", payload, task="outline")
        assert_embedded_schema(new_request, OutlineResponse)
        assert request_digest(client.config.chat.base_url, new_request) != request_digest(client.config.chat.base_url, old_request)
        for _ in range(2):
            await client.call_request(new_request, budget, task="outline", execution_spec=spec)
        await client.call_request(old_request, budget, task="outline", execution_spec=spec)
    assert calls == [old_request, new_request]
    assert len(audit.rows) == 2
    assert all(row["status"] == "succeeded" for row in audit.rows.values())
    assert json.loads(new_request["messages"][1]["content"][0]["text"]) == payload


@pytest.mark.parametrize("accept_on", [2, None])
async def test_contract_feedback_preserves_schema_and_original_payload_with_two_corrections(tmp_path, monkeypatch, accept_on):
    budget = RequestBudget(model="gpt-4o", context_tokens=32768, prompts={}, output_reserve=4096)
    payload = {"title_path": ["技术响应"], "targets": [], "supports": []}
    request = build_request(budget.model, "Write only supported content.", payload, task="authoring_write")
    original, calls, audit = deepcopy(request), [], JournalAudit()

    def handler(sent):
        calls.append(json.loads(sent.content))
        return response('{"blocks":[]}' if len(calls) == accept_on else '{"blocks":"invalid"}')

    def validate(value):
        if not isinstance(value.get("blocks"), list):
            raise ModelContractError([{"path": ["blocks"], "code": "list_type", "detail": "Return an array"}])
        return value

    now = [1000.0]

    def advance(seconds):
        now[0] += seconds

    sleep = AsyncMock(side_effect=advance)
    monkeypatch.setattr("docreader.llm.recovery.time.time", lambda: now[0])
    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", sleep)
    artifacts = CountedArtifacts(tmp_path)
    client, _ = model(handler, audit, artifacts=artifacts)
    lease = SimpleNamespace(execution_id="schema-feedback-test", epoch=1)
    adapter = _OutlineModel(client, audit, artifacts, lease, budget, {"protocol": "generated-output-schema-test"}, task="authoring_write")
    async with client:
        if accept_on is None:
            with pytest.raises(ModelWorkUnitExhausted):
                await adapter(request, validate)
        else:
            assert await adapter(request, validate) == {"blocks": []}
    assert len(calls) == (accept_on or 3)
    assert request == original and calls[0] == original
    for ordinal, sent in enumerate(calls):
        assert_embedded_schema(sent, WritingResponse)
        assert sent["messages"][0] == original["messages"][0]
        current = json.loads(sent["messages"][1]["content"][0]["text"])
        if ordinal:
            feedback = current.pop("contract_feedback")
            assert feedback["errors"][0]["code"] == "list_type"
            assert feedback["rejection_count"] == ordinal
        assert current == payload
        assert "contract_feedback" not in sent
    rows = list(audit.rows.values())
    assert len({row["metadata"]["logical_request_digest"] for row in rows}) == 1
    assert len({row["metadata"]["request_digest"] for row in rows}) == len(calls)
    assert len([key for key in audit.receipts if key[0] == "model-rejection"]) == (3 if accept_on is None else 1)
    assert [call.args[0] for call in sleep.await_args_list] == ([1, 2] if accept_on is None else [1])
