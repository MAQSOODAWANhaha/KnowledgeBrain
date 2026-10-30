"""Complete request admission estimates, never provider usage or output caps."""

import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ImageInput, ModelBudgetError, ModelConfigurationError, ModelSemanticError, build_request


PROMPTS = {"extract": 'Return JSON matching schema {"requirements":[],"dispositions":[]}.',
           "verify": 'Check JSON {"requirements":[],"coverage":[]}.',
           "plan": 'Return JSON {"items":[]}.', "ocr": "Return literal text JSON."}


def budget(context_tokens=131072, **kwargs):
    return RequestBudget(context_tokens=context_tokens, model="gpt-6-luna", prompts=PROMPTS, **kwargs)


def test_decoded_content_and_reserves_decide_admission_wire_bytes_are_separate():
    payload = {"pack_id": "p1", "body": [{"text": '中文 "quoted" \\ path'}],
               "supporting": [{"text": "条件适用", "locator": {"page": 2}}],
               "pending_scope_context": []}
    request = build_request("gpt-6-luna", PROMPTS["extract"], payload, task="extract")
    measured = budget().measure("extract", payload)
    wire_bytes = len(json.dumps(request, ensure_ascii=False, sort_keys=True,
                                separators=(",", ":"), allow_nan=False).encode())
    exact_bytes = len(request["messages"][0]["content"].encode()) + len(request["messages"][1]["content"][0]["text"].encode())
    assert measured["wire_bytes"] == wire_bytes > exact_bytes
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


@pytest.mark.parametrize("task", ["extract", "verify"])
def test_analysis_source_views_report_body_and_support_without_changing_admission(task):
    payload = {
        "schema_version": 2,
        "source_views": [
            {"document_id": "main", "body": [{"range_id": "r1", "text": '正文 "三十天"'}],
             "supporting": [{"range_id": "r2", "text": "适用第一标段。"}]},
            {"document_id": "change", "body": [{"range_id": "r3", "text": "修订\\条件\n原文"}],
             "supporting": []},
        ],
        "items": [{"text": "这部分属于事项开销。", "evidence": [{"quote": "正文"}]}],
    }
    b = budget()
    request = b.request(task, payload)
    measured = b.measure_request(request)
    exact_text_bytes = len(request["messages"][0]["content"].encode()) + len(request["messages"][1]["content"][0]["text"].encode())
    expected = {
        field: sum(len(json.dumps(view[field], ensure_ascii=False, separators=(",", ":")).encode()) - 2
                   for view in payload["source_views"])
        for field in ("body", "supporting")
    }
    assert measured["body_tokens"] == expected["body"] > 0
    assert measured["support_tokens"] == expected["supporting"] > 0
    assert measured["text_tokens"] == exact_text_bytes
    assert measured["overhead_tokens"] == exact_text_bytes - sum(expected.values())
    assert measured["input_tokens"] == exact_text_bytes + 32
    assert measured["total_context_tokens"] == exact_text_bytes + 32 + 16384 + 4096
    assert measured["wire_bytes"] == len(json.dumps(request, ensure_ascii=False, sort_keys=True,
                                                    separators=(",", ":"), allow_nan=False).encode())
    assert measured["fits"] is True
    assert budget(32768, output_reserve=32768).measure_request(request)["fits"] is False


def test_source_contributions_include_top_level_and_nested_views_once_each():
    payload = {"body": [{"text": "旧结构正文"}], "supporting": [{"text": "旧结构条件"}],
               "source_views": [{"body": [{"text": "新结构正文"}], "supporting": [{"text": "新结构条件"}]}],
               "items": [{"body": [{"text": "事项字段不能算作来源正文"}]}]}
    measured = budget().measure("verify", payload)
    for source_field, result_field in (("body", "body_tokens"), ("supporting", "support_tokens")):
        expected = sum(len(json.dumps(view[source_field], ensure_ascii=False, separators=(",", ":")).encode()) - 2
                       for view in [payload, *payload["source_views"]])
        assert measured[result_field] == expected
    assert measured["overhead_tokens"] > 0


def test_complete_required_support_is_never_truncated_to_make_a_package_fit():
    b = budget(32768)
    payload = {"body": [{"text": "tiny"}], "supporting": [{"text": "条件" * 3000}]}
    measured = b.measure("extract", payload)
    assert measured["fits"] is False and measured["support_tokens"] > 18000
    assert b.fits("extract", payload) is False
    with pytest.raises(ModelBudgetError, match="context"):
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
    assert first["wire_bytes"] < second["wire_bytes"]
    assert {key: value for key, value in first.items() if key != "wire_bytes"} == {
        key: value for key, value in second.items() if key != "wire_bytes"}
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


def test_input_allowances_do_not_expand_the_provider_total_context():
    payload = {"text": "x" * 140000}
    ordinary = budget(300000)
    correction = budget(300000, correction=True)
    assert ordinary.measure("verify", payload)["fits"] is False
    measured = correction.require("verify", payload)
    assert measured["input_budget_tokens"] == 131072
    assert measured["correction_input_budget_tokens"] == 262144
    assert measured["effective_input_budget_tokens"] == 262144
    assert budget(131072, correction=True).measure("verify", payload)["fits"] is False
    assert budget(131072).effective_input_budget_tokens == 110592
    assert not issubclass(ModelBudgetError, ModelSemanticError)
    request = correction.request("verify", payload)
    assert "max_tokens" not in request and "max_completion_tokens" not in request
    with pytest.raises(ModelConfigurationError, match="total context"):
        budget(None, correction=True)


def test_default_domain_prompts_are_actual_schema_text_and_snapshot_is_immutable(monkeypatch):
    from docreader.tender import prompts

    actual = dict(PROMPTS)
    monkeypatch.setattr(prompts, "get_prompt", actual.__getitem__)
    b = RequestBudget()
    original = b.spec()
    assert b.request("extract", {}) == build_request(b.model, actual["extract"], {}, task="extract")
    actual["extract"] += " amended"
    assert b.spec() == original
    assert RequestBudget().spec() != original
    assert "amended" not in json.dumps(original)
    assert "requirements" not in json.dumps(original)


@pytest.mark.parametrize("value", [{"a": float("nan")}, {"a": object()}])
def test_non_json_payload_has_no_content_in_error(value):
    with pytest.raises(ModelSemanticError, match="JSON serializable"):
        budget().measure("extract", value)


def test_verified_tokenizer_counts_verify_only_and_keeps_extract_bytes():
    import tiktoken

    encoding = tiktoken.get_encoding("o200k_base")
    payload = {"source_views": [{"body": [{"range_id": "r1", "text": "投标人应提交营业执照副本。"}],
                                 "supporting": [{"range_id": "r2", "text": "适用第一标段。"}]}]}
    plain, tokenized = budget(), budget(tokenizer="o200k_base")
    request = tokenized.request("verify", payload)
    expected = (len(encoding.encode(request["messages"][0]["content"]))
                + len(encoding.encode(request["messages"][1]["content"][0]["text"])))
    measured = tokenized.measure_request(request, "verify")
    assert tokenized.measure("verify", payload) == measured
    assert tokenized.measure_request(request)["method"] == "utf8_bytes_decoded_messages_v2"
    assert measured["text_tokens"] == expected
    assert measured["input_tokens"] == expected + 512
    assert measured["output_reserve"] == 32768
    assert measured["effective_input_budget_tokens"] == 131072 - 32768 - 4096
    assert measured["total_context_tokens"] == expected + 512 + 32768 + 4096
    assert measured["method"] == "o200k_base_decoded_messages_v1"
    assert measured["body_tokens"] > 0 and measured["support_tokens"] > 0
    assert measured["text_tokens"] < plain.measure("verify", payload)["text_tokens"]
    assert tokenized.measure("extract", payload) == plain.measure("extract", payload)
    assert "tokenized" not in plain.spec()
    assert tokenized.spec()["tokenized"] == {"tokenizer": "o200k_base", "tasks": ["verify"],
                                             "method": "o200k_base_decoded_messages_v1",
                                             "framing_tokens": 512, "output_reserve": 32768}
    assert budget(32768, tokenizer="o200k_base").measure("verify", payload)["output_reserve"] == 8192
    with pytest.raises(ModelConfigurationError, match="tokenizer"):
        budget(tokenizer="cl100k_base")


def test_actual_tool_and_json_schema_fields_count_separately_from_http_escaping():
    b = budget()
    request = b.request("extract", {"body": [{"text": 'quoted "中文"\\line\n'}]})
    before = b.measure_request(request)
    tools = [{"type": "function", "function": {"name": "lookup", "parameters": {"type": "object"}}}]
    schema = {"name": "answer", "schema": {"type": "object", "properties": {"answer": {"type": "string"}}}}
    request.update(tools=tools, response_format={"type": "json_schema", "json_schema": schema})
    measured = b.measure_request(request)
    actual_schema_bytes = sum(len(json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode())
                              for value in (tools, schema))
    assert measured["text_tokens"] == before["text_tokens"] + actual_schema_bytes
    assert measured["wire_bytes"] > measured["text_tokens"]
    assert measured["method"] == "utf8_bytes_decoded_messages_v2"
