"""Complete invalid JSON keeps bounded wire diagnostics and paid-call fences."""

import json

import pytest

from docreader.llm import request as errors
from docreader.llm.streaming import StreamResponse
from test_tender_llm import Audit, CountedArtifacts, environment, model, response
from test_tender_streaming import DONE, ReplayAudit, event


class WireAudit(Audit):
    async def commit_artifact(self, *args, **kwargs):
        pass


def complete(content, *, finish="stop", delta=None, done=True):
    stream = StreamResponse()
    stream.feed(event(content, delta=delta) + event(delta={}, finish=finish) + (DONE if done else b""))
    return stream


def json_error(stream):
    with pytest.raises(errors.ModelSemanticError) as caught:
        stream.result()
    assert type(caught.value).__name__ == "ModelJSONError"
    return caught.value


def test_duplicate_closing_delta_has_exact_json_location_and_short_wire_ids():
    original = '{"range_id":"r1","relations":[],"decisions":[],"findings":[]}'
    stream = StreamResponse()
    stream.feed(event(original) + event("]}") + event(delta={}, finish="stop") + DONE)
    error = json_error(stream)
    assert error.code == "json_syntax_error"
    assert error.line == 1 and error.column == len(original) + 1 and error.offset == len(original)
    assert error.feedback["message"] == "Extra data"
    assert '"range_id":"r1"' in error.feedback["fragment"]
    assert error.feedback["fragment"].endswith("]}")
    assert "r1" not in str(error) and "r1" not in repr(error)


@pytest.mark.parametrize("content", ['\n  [1,2]', '\n  "text"', '\n  null'])
def test_complete_non_object_has_a_json_root_diagnostic(content):
    error = json_error(complete(content))
    assert error.code == "json_object_required"
    assert (error.line, error.column, error.offset) == (2, 3, 3)


@pytest.mark.parametrize("content", ['{"text":"' + "中" * 5000 + '"}]}', '{"text":"' + "\x00" * 5000],
                         ids=["large-unicode", "escaped-controls"])
def test_feedback_is_bounded_after_json_escaping(content):
    error = json_error(complete(content))
    assert len(json.dumps(error.feedback, ensure_ascii=False, separators=(",", ":"))) <= 384
    assert len(error.feedback["fragment"]) < len(content)


@pytest.mark.parametrize("settings", [
    {"finish": "length"}, {"delta": {"refusal": "refused"}},
    {"delta": {"tool_calls": [{"index": 0}]}, "finish": "tool_calls"},
])
def test_other_complete_stream_failures_are_not_json_correction_errors(settings):
    with pytest.raises(errors.ModelSemanticError) as caught:
        complete('{"x":', **settings).result()
    assert type(caught.value).__name__ != "ModelJSONError"


def test_missing_done_and_nonfinite_values_keep_their_previous_policy():
    with pytest.raises(errors.UncertainModelResponse):
        complete('{"x":', done=False).result()
    with pytest.raises(errors.ModelSemanticError) as caught:
        complete('{"x":NaN}').result()
    assert type(caught.value).__name__ != "ModelJSONError"


async def test_client_persists_failed_receipt_before_exposing_wire_json_feedback(tmp_path):
    artifacts = CountedArtifacts(tmp_path)
    payload = {"schema_version": 2, "work_unit_id": "pack-permanent", "source_views": [
        {"body": [{"range_id": "range-permanent", "text": "Original evidence."}]}]}
    client, audit = model(lambda request: response('{"range_id":"r1"}]}'), WireAudit(), artifacts=artifacts)
    async with client:
        with pytest.raises(errors.ModelSemanticError) as caught:
            await client.call("extract", payload)
    assert type(caught.value).__name__ == "ModelJSONError"
    assert '"range_id":"r1"' in caught.value.feedback["fragment"]
    assert "range-permanent" not in json.dumps(caught.value.feedback)
    terminal = audit.events[-1]
    assert terminal["status"] == "failed" and terminal["response_ref"]
    saved = artifacts.read_json(terminal["response_ref"])
    assert saved["done"] and saved["finish_reason"] == "stop"
    assert saved["outcome"] == "invalid_json_response" and terminal["usage"]["total_tokens"] == 20
    assert [e["status"] for e in audit.events] == ["prepared", "sent", "failed"]


@pytest.mark.parametrize("secret", ["private-" + "Z" * 400 + "-key", 'private-"key\\line\n中文'],
                         ids=["long-boundary-secret", "json-escaped-secret"])
async def test_client_redacts_credentials_before_clipping_error_fragment(tmp_path, secret):
    content = json.dumps({"token": secret, "range_id": "r1"}, ensure_ascii=False, separators=(",", ":")) + "]}"
    client, _ = model(lambda request: response(content), artifacts=CountedArtifacts(tmp_path),
                      values=environment(KNOWLEDGEBRAIN_VLM_BASE_URL="https://vision.invalid/v1",
                                         KNOWLEDGEBRAIN_VLM_MODEL="vision-model",
                                         KNOWLEDGEBRAIN_VLM_API_KEY=secret))
    async with client:
        with pytest.raises(errors.ModelSemanticError) as caught:
            await client.call("extract", {})
    assert type(caught.value).__name__ == "ModelJSONError"
    feedback = caught.value.feedback
    assert len(json.dumps(feedback, ensure_ascii=False, separators=(",", ":"))) <= 512
    assert "Z" * 16 not in feedback["fragment"]
    for literal in (secret, json.dumps(secret, ensure_ascii=False)[1:-1], json.dumps(secret)[1:-1]):
        assert literal not in feedback["fragment"]
    assert '"range_id":"r1"' in feedback["fragment"]
    assert feedback["offset"] == len(content) - 2


async def test_logical_digest_excludes_only_top_level_contract_feedback(monkeypatch, tmp_path):
    payload = {"schema_version": 2, "work_unit_id": "unit-permanent", "source_views": [],
               "issues": [{"code": "note", "detail": "contract_feedback is prose", "blocking": True}]}
    client, audit = model(lambda request: response(), WireAudit(), artifacts=CountedArtifacts(tmp_path))
    budget = client.request_budget()
    requests = []
    build = budget.request

    def measured(*args, **kwargs):
        requests.append(args[1])
        return build(*args, **kwargs)

    monkeypatch.setattr(budget, "request", measured)
    monkeypatch.setattr(client, "request_budget", lambda **kwargs: budget)
    async with client:
        await client.call("extract", payload)
        assert len(requests) == 1
        await client.call("extract", {**payload, "contract_feedback": {"code": "json_syntax_error"}})
    prepared = [e["metadata"] for e in audit.events if e["status"] == "prepared"]
    assert len(requests) == 3 and len(prepared) == 2
    assert prepared[0]["logical_request_digest"] == prepared[0]["request_digest"]
    assert prepared[1]["logical_request_digest"] == prepared[0]["request_digest"]
    assert prepared[1]["request_digest"] != prepared[0]["request_digest"]
    assert "contract_feedback" not in payload


async def test_client_reports_corrupt_success_cache_without_silently_resending(tmp_path):
    artifacts, audit, calls = CountedArtifacts(tmp_path), ReplayAudit(), []
    first, _ = model(lambda request: (calls.append(request), response())[1], audit, artifacts=artifacts)
    async with first:
        await first.call("extract", {})
    cached = next(iter(audit.rows.values()))
    saved = artifacts.read_json(cached["response_ref"])
    saved["raw_body"] = (event('{"x":') + event(delta={}, finish="stop") + DONE).decode()
    cached["response_ref"] = artifacts.put_json(saved)
    second, _ = model(lambda request: calls.append(request), audit, artifacts=artifacts)
    async with second:
        with pytest.raises(errors.ModelCorrectionStalled, match="cached model response"):
            await second.call("extract", {})
    assert len(calls) == 1


def test_terminal_work_unit_error_has_safe_text_and_optional_task():
    assert hasattr(errors, "ModelWorkUnitExhausted")
    error = errors.ModelWorkUnitExhausted("private-source-content")
    assert isinstance(error, RuntimeError)
    assert error.task == "private-source-content"
    assert "private-source-content" not in str(error)
    assert "private-source-content" not in repr(error)
    assert str(errors.ModelWorkUnitExhausted()) == str(error)
