"""Reported usage and planning estimates must never be conflated."""

from copy import deepcopy

from docreader.llm import usage


def test_observation_keeps_missing_usage_and_failed_outputs_visible():
    rows = [
        {"task": "extract", "status": "succeeded", "metadata": {"output_reserve": 16384, "context_tokens": 131072},
         "usage": {"prompt_tokens": 1000, "completion_tokens": 36660, "total_tokens": 37660}},
        {"task": "extract", "status": "failed", "metadata": {"output_reserve": 16384},
         "usage": {"prompt_tokens": 200, "completion_tokens": 20, "total_tokens": 220}},
        {"task": "verify", "status": "uncertain", "usage": None},
        {"task": "verify", "status": "succeeded", "usage": {"prompt_tokens": 100}},
    ]
    before = deepcopy(rows)
    report = usage.budget_observation(rows)
    assert report["requests"] == 4
    extract = report["tasks"]["extract"]
    assert extract["reported_tokens"] == {"prompt_tokens": 1200, "completion_tokens": 36680, "total_tokens": 37880}
    assert extract["completion"]["max"] == 36660 and extract["completion"]["p95"] == 36660
    assert extract["completion"]["exceeds_reserve"] == 1
    assert extract["context"] == {"observations": 2, "max_reported_total": 37660,
                                  "capacity_comparisons": 1, "exceeds_context": 0}
    assert report["tasks"]["verify"]["completion"]["observations"] == 0
    assert report["tasks"]["verify"]["completion"]["max"] is None
    assert report["tasks"]["verify"]["context"]["max_reported_total"] is None
    assert report["tasks"]["verify"]["missing_token_fields"]["completion_tokens"] == 2
    assert report["tasks"]["verify"]["statuses"] == {"uncertain": 1, "succeeded": 1}
    assert rows == before


def test_plan_totals_stay_separate_from_reported_usage():
    plans = [{"phase": "locate", "requests": [
        {"budget": {"input_tokens": 10000, "output_reserve": 16384, "fits": True}},
        {"budget": {"input_tokens": 20000, "output_reserve": 16384, "fits": False}},
    ]}]
    report = usage.budget_observation([], plans)
    assert report["tasks"] == {} and report["requests"] == 0
    assert report["plans"] == [{"phase": "locate", "planned_requests": 2,
        "input_estimate": 30000, "max_input_estimate": 20000,
        "output_reserve_total": 32768, "over_budget_requests": 1}]


def test_operation_samples_exclude_failed_outputs_and_unplanned_stages_are_unknown():
    rows = [
        {"task": "verify", "status": "succeeded", "metadata": {"operation": "locate", "work_unit_id": "reference-4"},
         "usage": {"completion_tokens": 100}},
        {"task": "verify", "status": "failed", "metadata": {"operation": "locate", "work_unit_id": "reference-4"},
         "usage": {"completion_tokens": 900}},
        {"task": "verify", "status": "succeeded", "metadata": {"operation": "review", "work_unit_id": "joint-1"},
         "usage": {"completion_tokens": 150}},
        {"task": "verify", "status": "uncertain", "metadata": {}, "usage": None},
    ]
    result = usage.budget_observation(rows, [{"phase": "review", "requests": []}])
    assert result["operations"]["reference"]["completion"]["max"] == 900
    assert result["operations"]["reference"]["successful_completion"] == {"observations": 1, "max": 100, "p95": 100}
    assert result["operations"]["joint"]["requests"] == 1
    assert result["operations"]["verify_unattributed"]["requests"] == 1
    assert result["planning"]["unplanned_phases"] == ["extract", "locate", "associate", "joint"]
    assert result["planning"]["total_planned_requests"] is None
    assert result["planning"]["known_planned_requests"] == 0
