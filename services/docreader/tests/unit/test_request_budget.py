"""Complete request admission estimates, never provider usage or output caps."""

import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ImageInput, ModelSemanticError, build_request


PROMPTS = {"extract": 'Return JSON matching schema {"requirements":[],"dispositions":[]}.',
           "verify": 'Check JSON {"requirements":[],"coverage":[]}.',
           "plan": 'Return JSON {"items":[]}.', "ocr": "Return literal text JSON."}


def budget(context_tokens=131072, **kwargs):
    return RequestBudget(context_tokens=context_tokens, model="gpt-6-luna", prompts=PROMPTS, **kwargs)


def test_full_canonical_request_and_reserves_decide_admission():
    payload = {"pack_id": "p1", "body": [{"text": '中文 "quoted" \\ path'}],
               "supporting": [{"text": "条件适用", "locator": {"page": 2}}],
               "pending_scope_context": []}
    request = build_request("gpt-6-luna", PROMPTS["extract"], payload)
    measured = budget().measure("extract", payload)
    exact_bytes = len(json.dumps(request, ensure_ascii=False, sort_keys=True,
                                separators=(",", ":"), allow_nan=False).encode())
    assert measured["text_tokens"] == exact_bytes
    assert measured["input_tokens"] == exact_bytes + 32
    assert measured["body_tokens"] > 0 and measured["support_tokens"] > 0
    assert measured["body_tokens"] + measured["support_tokens"] + measured["overhead_tokens"] == exact_bytes
    assert measured["output_reserve"] == 16384
    assert measured["safety"] == 4096
    assert measured["total_context_tokens"] == exact_bytes + 32 + 16384 + 4096
    assert measured["context_tokens"] == 131072 and measured["fits"] is True
    assert "C" not in measured and "max_context_tokens" not in measured
    assert "utf8" in measured["method"]
    assert "max_tokens" not in request and "max_completion_tokens" not in request
    assert request["response_format"] == {"type": "json_object"}


def test_complete_required_support_is_never_truncated_to_make_a_package_fit():
    b = budget(32768)
    payload = {"body": [{"text": "tiny"}], "supporting": [{"text": "条件" * 3000}]}
    measured = b.measure("extract", payload)
    assert measured["fits"] is False and measured["support_tokens"] > 18000
    assert b.fits("extract", payload) is False
    with pytest.raises(ModelSemanticError, match="context"):
        b.require("extract", payload)
    assert b.measure("extract", {"body": payload["body"], "supporting": []})["fits"] is True


def test_capacity_applies_to_entire_prompt_envelope_and_response_reserve():
    payload = {"body": [{"text": "x" * 15000}], "supporting": []}
    assert budget(32768).measure("extract", payload)["fits"] is False
    assert budget(65536).measure("extract", payload)["fits"] is True
    measured = budget(32768).measure("extract", {})
    assert measured["safety"] == 2048
    assert budget(32768, output_reserve=0).measure("extract", payload)["fits"] is True


def test_boundaries_include_full_framing_and_safety():
    # At these capacities safety has reached its 4096 ceiling.
    payload = {"body": [{"text": "x" * 60000}]}
    total = budget().measure("extract", payload)["total_context_tokens"]
    assert budget(total).measure("extract", payload)["fits"] is True
    assert budget(total - 1).measure("extract", payload)["fits"] is False


def test_images_have_reserves_but_base64_is_not_text_and_data_like_text_still_counts():
    small = ImageInput(b"x", "image/png")
    large = ImageInput(b"x" * 10000, "image/png")
    b = budget()
    first = b.measure("ocr", {"text": "image"}, images=[small])
    second = b.measure("ocr", {"text": "image"}, images=[large])
    assert first == second
    assert first["image_tokens"] == 16384 and first["image_count"] == 1
    assert b.measure("ocr", {"text": "data:image/png;base64," + "a" * 10000})["text_tokens"] > 10000
    assert b.measure("ocr", {}, images=[small, small])["image_tokens"] == 32768
    with pytest.raises(ModelSemanticError):
        b.measure("ocr", {}, images=["https://unknown.invalid/image.png"])


def test_unknown_independent_vlm_capacity_is_not_reported_as_proven_fit():
    result = budget(None).require("ocr", {}, images=[ImageInput(b"x", "image/png")])
    assert result["context_tokens"] is None and result["fits"] is None
    assert result["input_tokens"] > 16384


def test_actual_verify_and_plan_payloads_are_rechecked():
    b = budget(32768)
    payload = {"body": [{"text": "short"}], "supporting": []}
    assert b.fits("extract", payload)
    assert not b.fits("verify", {**payload, "requirements": [{"quote": "x" * 18000}]})
    assert not b.fits("plan", {"requirements": [{"details": "x" * 18000}]})


def test_default_domain_prompts_are_actual_schema_text_and_snapshot_is_immutable(monkeypatch):
    from docreader.tender import prompts

    actual = dict(PROMPTS)
    monkeypatch.setattr(prompts, "get_prompt", actual.__getitem__)
    b = RequestBudget()
    original = b.spec()
    assert b.request("extract", {})["messages"][0]["content"] == actual["extract"]
    actual["extract"] += " amended"
    assert b.spec() == original
    assert RequestBudget().spec() != original
    assert "amended" not in json.dumps(original)
    assert "requirements" not in json.dumps(original)


@pytest.mark.parametrize("value", [{"a": float("nan")}, {"a": object()}])
def test_non_json_payload_has_no_content_in_error(value):
    with pytest.raises(ModelSemanticError, match="JSON serializable"):
        budget().measure("extract", value)
