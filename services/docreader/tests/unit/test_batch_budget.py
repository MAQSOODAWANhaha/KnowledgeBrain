"""Planning and sending use the same actual target-count output allowance."""

from copy import deepcopy

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelBudgetError


def payload(count=4):
    return {"schema_version": 2, "operation": "associate", "work_units": [
        {"work_unit_id": f"unit-{i}", "candidate_ids": [], "issue_ids": []} for i in range(count)],
        "target_work_unit_ids": [f"unit-{i}" for i in range(count)],
        "source_views": [{"body": [{"range_id": "range", "text": "word " * 30000}]}]}


@pytest.mark.parametrize("tokenizer", [None, "o200k_base"])
@pytest.mark.parametrize("count,reserve", [(1, 32768), (2, 49152), (3, 73728), (4, 98304)])
def test_batch_reserve_matches_decoded_request_even_with_correction(tokenizer, count, reserve):
    budget = RequestBudget(prompts={"verify": "Read full evidence"}, tokenizer=tokenizer, correction=True)
    p = payload(count)
    p["contract_feedback"] = {"errors": [{"detail": "Correct only these target groups"}]}
    measured = budget.measure("verify", p)
    assert measured == budget.measure_request(budget.request("verify", p), "verify")
    assert measured["output_reserve"] == reserve
    assert measured["effective_input_budget_tokens"] == 131072 - reserve - 4096


def test_smaller_target_subset_keeps_full_basis_and_reduces_only_output_allowance():
    budget = RequestBudget(prompts={"verify": "Read all original evidence"}, tokenizer="o200k_base")
    p = payload()
    assert budget.measure("verify", p)["fits"] is False
    with pytest.raises(ModelBudgetError):
        budget.require_request(budget.request("verify", p), "verify")
    subset = deepcopy(p)
    subset["target_work_unit_ids"] = ["unit-3"]
    measured = budget.require("verify", subset)
    assert measured["output_reserve"] == 32768
    assert subset["work_units"] == p["work_units"] and subset["source_views"] == p["source_views"]


def test_existing_unbatched_review_keeps_original_reserve():
    budget = RequestBudget(prompts={"verify": "Read"}, tokenizer="o200k_base")
    assert budget.measure("verify", {"schema_version": 2, "operation": "review",
                                     "work_unit_id": "old-unit"})["output_reserve"] == 32768
