"""Pure source-review preparation reuses exact inputs without changing model requests."""

from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.llm.budget import RequestBudget
from docreader.llm.request import request_digest
from docreader.tender.checks.analysis import _Analysis, verify_analysis
from test_analysis_checks import IDENTITY, Model, Receipts, budget, scenario


async def verify(data, model, admission, identity=IDENTITY):
    return await verify_analysis(data[1], data[2], data[0], model, admission,
                                 identity=identity, save=data[3])


def cached_reviews(receipts):
    matches = [key for key in receipts.saved if key[0] == "analysis-review-requests"]
    assert len(matches) == 1
    return matches[0]


async def test_source_review_replay_skips_preparation_and_preserves_exact_requests(tmp_path, monkeypatch):
    data = await scenario(tmp_path)
    admission = budget()
    direct = _Analysis(data[1], data[2], data[0], IDENTITY, admission).review_requests()
    model = Model()
    expected = await verify(data, model, admission)
    supplied = [payload for payload in model.calls if payload["operation"] == "review"]
    assert supplied == direct
    assert [request_digest("https://example.test/v1", admission.request("verify", payload))
            for payload in supplied] == [
        request_digest("https://example.test/v1", admission.request("verify", payload)) for payload in direct]
    # The ordinary CAS encoding sorts object fields. Request message text must keep
    # the original order instead, including request-local reference traversal.
    assert json.dumps(direct) != json.dumps(json.loads(json.dumps(direct, sort_keys=True)))
    data[3].saved.pop(cached_reviews(data[3]))
    rebuilt = Model()
    assert await verify(data, rebuilt, admission) == expected
    assert rebuilt.calls == []

    def unexpected_preparation(self):
        raise AssertionError("unchanged source review was prepared again")

    monkeypatch.setattr(_Analysis, "review_requests", unexpected_preparation)
    replay = Model()
    assert await verify(data, replay, admission) == expected
    assert replay.calls == []


@pytest.mark.parametrize("changed", ["source", "items", "scope", "prompt", "budget", "identity"])
async def test_source_review_cache_requires_complete_current_inputs(tmp_path, monkeypatch, changed):
    data = await scenario(tmp_path)
    admission = budget()
    await verify(data, Model(), admission)
    key = cached_reviews(data[3])
    # A different revision must not mistake an imported old preparation for its
    # current inputs. Other stage receipts are revision-owned and are not copied.
    receipts = Receipts(data[0])
    receipts.saved[key] = data[3].saved[key]
    data = data[0], deepcopy(data[1]), deepcopy(data[2]), receipts
    identity = deepcopy(IDENTITY)
    if changed == "source":
        data[1]["documents"][0]["file_name"] = "renamed-source.txt"
    elif changed == "items":
        entry = data[2]["packs"][0]
        value = data[0].read_json(entry["artifact_ref"])
        value["items"][0]["text"] += "核对适用条件。"
        entry["artifact_ref"] = data[0].put_json(value)
    elif changed == "scope":
        context = _Analysis(data[1], data[2], data[0], identity, admission)
        source = next(iter(context.sources.values()))
        data[1]["scope_decisions"] = [{"range_id": source["range_id"], "decision": "local_only",
            "target_range_ids": [], "rationale": "只适用于本处。",
            "evidence": [{"range_id": source["range_id"], "quote": source["text"]}]}]
    elif changed == "prompt":
        admission = RequestBudget(context_tokens=100000, output_reserve=0,
            prompts={**admission.prompts, "verify": "Read all supplied source evidence independently."})
    elif changed == "budget":
        admission = budget(100001)
    else:
        identity["domain_revision"] += 1
    prepared = []
    original = _Analysis.review_requests

    def prepare(self):
        prepared.append(True)
        return original(self)

    monkeypatch.setattr(_Analysis, "review_requests", prepare)
    await verify(data, Model(), admission, identity)
    assert prepared == [True]
    assert len([key for key in receipts.saved if key[0] == "analysis-review-requests"]) == 2


@pytest.mark.parametrize("broken", ["invalid_json", "missing_field", "missing_payload_field",
                                   "non_list_object", "non_list_null", "missing_reference"])
async def test_damaged_review_preparation_fails_without_model_calls_or_rebuilding(tmp_path, monkeypatch, broken):
    data = await scenario(tmp_path)
    admission = budget()
    await verify(data, Model(), admission)
    key = cached_reviews(data[3])
    if broken == "invalid_json":
        data[3].saved[key] = data[0].put_json({"requests_json": "["})
        error = json.JSONDecodeError
    elif broken == "missing_field":
        data[3].saved[key] = data[0].put_json({})
        error = KeyError
    elif broken == "missing_payload_field":
        data[3].saved[key] = data[0].put_json({"requests_json": "[{}]"})
        error = KeyError
    elif broken.startswith("non_list"):
        value = {} if broken == "non_list_object" else None
        data[3].saved[key] = data[0].put_json({"requests_json": json.dumps(value)})
        error = ValueError
    else:
        data[0].path(data[3].saved[key]).unlink()
        error = ArtifactIntegrityError

    def unexpected_preparation(self):
        raise AssertionError("damaged committed preparation must not be silently rebuilt")

    monkeypatch.setattr(_Analysis, "review_requests", unexpected_preparation)
    model = Model()
    with pytest.raises(error):
        await verify(data, model, admission)
    assert model.calls == []
