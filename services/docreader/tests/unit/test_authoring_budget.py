"""Verified token counting requires explicit calibrated authoring admission."""

from copy import deepcopy
import json

import pytest
import tiktoken

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelBudgetError, ModelConfigurationError, build_request


def budget(**options):
    return RequestBudget(model="fixture", context_tokens=131072, prompts={}, **options)


def request(payload):
    return build_request("fixture", "按完整候选语义组织实际响应位置。", payload)


def test_explicit_assembly_budget_admits_full_chinese_context_without_modifying_request():
    value = request({"operation": "assemble", "rules": [{"rule_id": "r1", "text": "技术要求。" * 10000}],
                     "chapters": [{"title": "技术响应", "targets": [{"target_id": "t1", "rule_ids": ["r1"]}]}]})
    original = deepcopy(value)
    legacy = budget(tokenizer="o200k_base")
    enabled = budget(tokenizer="o200k_base", tokenized_outline_assembly=True)
    with pytest.raises(ModelBudgetError):
        legacy.require_request(value, "outline")
    measured = enabled.require_request(value, "outline")
    encoding = tiktoken.get_encoding("o200k_base")
    expected = (len(encoding.encode(value["messages"][0]["content"], disallowed_special=()))
                + len(encoding.encode(value["messages"][1]["content"][0]["text"], disallowed_special=())))
    assert measured["text_tokens"] == expected
    assert measured["input_tokens"] == expected + 512
    assert measured["method"] == "o200k_base_decoded_messages_v1"
    assert measured["output_reserve"] == 32768 and measured["safety"] == 4096
    assert measured["effective_input_budget_tokens"] == 94208
    assert measured["total_context_tokens"] == expected + 512 + 32768 + 4096
    assert measured["fits"] is True and value == original


@pytest.mark.parametrize("task,payload", [
    (None, {"operation": "assemble"}),
    ("outline", {}),
    ("outline", {"nested": {"operation": "assemble"}}),
    ("outline", {"operation": "source"}),
    ("outline_assemble", {"operation": "assemble"}),
    ("extract", {"operation": "assemble"}),
    ("authoring_basis", {"operation": "assemble"}),
    ("authoring_write", {"operation": "assemble"}),
    ("authoring_review", {"operation": "assemble"}),
])
def test_other_tasks_and_source_outline_keep_existing_byte_budget(task, payload):
    value = request({**payload, "text": "必须保留的技术原文。"})
    legacy = budget(tokenizer="o200k_base")
    enabled = budget(tokenizer="o200k_base", tokenized_outline_assembly=True)
    assert enabled.measure_request(value, task) == legacy.measure_request(value, task)
    assert enabled.measure_request(value, task)["method"] == "utf8_bytes_decoded_messages_v2"


@pytest.mark.parametrize("tokenizer", [None, "o200k_base"])
def test_disabled_flag_keeps_default_spec_and_assembly_measurement_identical(tokenizer):
    options = {"tokenizer": tokenizer}
    original = budget(**options)
    disabled = budget(**options, tokenized_outline_assembly=False)
    assert json.dumps(original.spec(), sort_keys=True) == json.dumps(disabled.spec(), sort_keys=True)
    assert "operations" not in disabled.spec().get("tokenized", {})
    assert "tokenized_outline_assembly" not in json.dumps(disabled.spec())
    value = request({"operation": "assemble", "text": "中文完整内容。"})
    measured = disabled.measure_request(value, "outline")
    assert measured == original.measure_request(value, "outline")
    assert measured["method"] == "utf8_bytes_decoded_messages_v2" and measured["output_reserve"] == 16384


def test_enabled_spec_records_only_assembly_operation_and_verify_is_unchanged():
    legacy = budget(tokenizer="o200k_base")
    enabled = budget(tokenizer="o200k_base", tokenized_outline_assembly=True)
    expected = deepcopy(legacy.spec())
    expected["tokenized"]["operations"] = {"outline": ["assemble"]}
    assert enabled.spec() == expected
    assert enabled.spec()["tokenized"]["tasks"] == ["verify"]
    value = request({"operation": "review", "text": "独立核验仍使用原计量。"})
    assert enabled.measure_request(value, "verify") == legacy.measure_request(value, "verify")


@pytest.mark.parametrize("invalid", [None, 1, "true"])
def test_assembly_flag_requires_an_explicit_boolean(invalid):
    with pytest.raises(ModelConfigurationError, match="assembly"):
        budget(tokenizer="o200k_base", tokenized_outline_assembly=invalid)


def test_assembly_flag_requires_verified_tokenizer_and_never_guesses_from_model_name():
    with pytest.raises(ModelConfigurationError, match="tokenizer"):
        budget(tokenized_outline_assembly=True)
    value = build_request("gpt-6.1-sol", "组织", {"operation": "assemble", "text": "保留原文"})
    inferred = RequestBudget(model="gpt-6.1-sol", prompts={})
    assert inferred.measure_request(value, "outline")["method"] == "utf8_bytes_decoded_messages_v2"


def test_enabled_assembly_still_rejects_a_request_over_its_real_window():
    enabled = RequestBudget(model="fixture", context_tokens=8192, prompts={},
                            tokenizer="o200k_base", tokenized_outline_assembly=True)
    value = request({"operation": "assemble", "text": "技术要求。" * 10000})
    measured = enabled.measure_request(value, "outline")
    assert measured["output_reserve"] == 2048 and measured["safety"] == 512
    assert measured["fits"] is False
    with pytest.raises(ModelBudgetError):
        enabled.require_request(value, "outline")


@pytest.mark.parametrize("task,payload", [
    ("outline", {"operation": "source"}),
    ("outline", {"operation": "assemble"}),
    ("outline", {"operation": "review"}),
    ("authoring_basis", {}),
    ("authoring_write", {}),
    ("authoring_review", {}),
])
def test_tokenized_authoring_admits_complete_context_and_preserves_request(task, payload):
    value = request({**payload, "text": "必须保留的技术原文。" * 5000})
    original = deepcopy(value)
    with pytest.raises(ModelBudgetError):
        budget(tokenizer="o200k_base").require_request(value, task)
    measured = budget(tokenizer="o200k_base", tokenized_authoring=True).require_request(value, task)
    encoding = tiktoken.get_encoding("o200k_base")
    expected = (len(encoding.encode(value["messages"][0]["content"], disallowed_special=()))
                + len(encoding.encode(value["messages"][1]["content"][0]["text"], disallowed_special=())))
    assert measured["text_tokens"] == expected
    assert measured["input_tokens"] == expected + 512
    assert measured["method"] == "o200k_base_decoded_messages_v1"
    assert measured["output_reserve"] == 32768 and measured["safety"] == 4096
    assert measured["effective_input_budget_tokens"] == 94208
    assert measured["total_context_tokens"] == expected + 512 + 32768 + 4096
    assert measured["fits"] is True and value == original
    assert "max_tokens" not in value and "max_completion_tokens" not in value


@pytest.mark.parametrize("tokenizer,assembly", [(None, False), ("o200k_base", False), ("o200k_base", True)])
def test_disabled_tokenized_authoring_preserves_existing_spec_and_measurements(tokenizer, assembly):
    options = {"tokenizer": tokenizer, "tokenized_outline_assembly": assembly}
    original = budget(**options)
    disabled = budget(**options, tokenized_authoring=False)
    assert disabled.spec() == original.spec()
    assert "tokenized_authoring" not in json.dumps(disabled.spec())
    value = request({"operation": "assemble", "text": "完整候选与来源。"})
    for task in ("outline", "authoring_basis", "authoring_write", "authoring_review", "extract", "verify"):
        assert disabled.measure_request(value, task) == original.measure_request(value, task)


@pytest.mark.parametrize("assembly", [False, True])
def test_tokenized_authoring_spec_records_full_tasks_without_changing_analysis(assembly):
    original = budget(tokenizer="o200k_base", tokenized_outline_assembly=assembly)
    enabled = budget(tokenizer="o200k_base", tokenized_outline_assembly=assembly, tokenized_authoring=True)
    expected = deepcopy(original.spec())
    expected["tokenized"]["tasks"] = ["authoring_basis", "authoring_review", "authoring_write", "outline", "verify"]
    assert enabled.spec() == expected
    value = request({"operation": "source", "text": "分析仍使用原计量。"})
    for task in (None, "extract", "verify", "outline_assemble"):
        assert enabled.measure_request(value, task) == original.measure_request(value, task)


@pytest.mark.parametrize("invalid", [None, 1, "true"])
def test_tokenized_authoring_requires_explicit_boolean(invalid):
    with pytest.raises(ModelConfigurationError, match="authoring"):
        budget(tokenizer="o200k_base", tokenized_authoring=invalid)


def test_tokenized_authoring_requires_verified_tokenizer_and_never_infers_from_model():
    with pytest.raises(ModelConfigurationError, match="tokenizer"):
        budget(tokenized_authoring=True)
    value = build_request("gpt-6.1-sol", "写作", {"text": "保留原文"})
    inferred = RequestBudget(model="gpt-6.1-sol", prompts={})
    assert inferred.measure_request(value, "authoring_write")["method"] == "utf8_bytes_decoded_messages_v2"


def test_tokenized_authoring_still_reserves_output_and_rejects_window_overflow():
    enabled = RequestBudget(model="fixture", context_tokens=8192, prompts={},
                            tokenizer="o200k_base", tokenized_authoring=True)
    value = request({"operation": "source", "text": "技术要求。" * 10000})
    measured = enabled.measure_request(value, "outline")
    assert measured["output_reserve"] == 2048 and measured["safety"] == 512
    assert measured["effective_input_budget_tokens"] == 5632
    assert measured["fits"] is False
    with pytest.raises(ModelBudgetError):
        enabled.require_request(value, "outline")
