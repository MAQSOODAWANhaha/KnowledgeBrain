"""Single review requests keep the ordinary output reserve."""

from docreader.llm.budget import RequestBudget


def test_existing_unbatched_review_keeps_original_reserve():
    budget = RequestBudget(prompts={"verify": "Read"}, tokenizer="o200k_base")
    assert budget.measure("verify", {"schema_version": 2, "operation": "review",
                                     "work_unit_id": "old-unit"})["output_reserve"] == 32768
