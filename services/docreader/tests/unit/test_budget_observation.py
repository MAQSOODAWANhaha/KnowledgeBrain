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
    assert report["deviations"] == [{
        "task": "extract", "marks": ["completion_above_reserve"],
        "output_reserve": 16384, "prompt_tokens": 1000, "completion_tokens": 36660,
    }]
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


def test_deviation_marks_keep_expected_overestimates_and_fast_first_bytes_quiet():
    rows = [
        {"request_id": "under", "task": "verify", "status": "succeeded",
         "metadata": {"input_tokens": 100, "output_reserve": 16384, "first_data_seconds": 2},
         "usage": {"prompt_tokens": 140, "completion_tokens": 10}},
        {"request_id": "slow", "task": "extract", "status": "succeeded",
         "metadata": {"input_tokens": 8000, "output_reserve": 16384, "first_data_seconds": 60},
         "usage": {"prompt_tokens": 2000, "completion_tokens": 100}},
        {"request_id": "byte-overestimate", "task": "extract", "status": "succeeded",
         "metadata": {"input_tokens": 9000, "output_reserve": 16384, "first_data_seconds": 1},
         "usage": {"prompt_tokens": 2000, "completion_tokens": 100}},
    ]
    marks = {item["request_id"]: item["marks"] for item in usage.budget_observation(rows)["deviations"]}
    assert marks == {"under": ["estimate_below_prompt"], "slow": ["slow_first_data"]}


def test_timing_report_separates_parallel_request_time_from_elapsed_span():
    rows = [
        {"task": "verify", "status": "succeeded", "sent_at": "2026-11-04T00:00:00+00:00",
         "finished_at": "2026-11-04T00:00:10+00:00",
         "metadata": {"first_raw_seconds": 2, "first_content_seconds": 4,
                      "max_raw_idle_seconds": 3, "semaphore_wait_seconds": 0}},
        {"task": "verify", "status": "failed", "sent_at": "2026-11-04T00:00:05+00:00",
         "finished_at": "2026-11-04T00:00:15+00:00",
         "metadata": {"max_raw_idle_seconds": 10, "semaphore_wait_seconds": 2},
         "error": {"type": "first_data_idle_timeout"}},
        {"task": "verify", "status": "succeeded", "sent_at": "2026-11-04T00:00:20+00:00",
         "finished_at": "2026-11-04T00:00:25+00:00", "metadata": {}},
        {"task": "verify", "status": "uncertain", "sent_at": "2026-11-04T00:00:30+00:00",
         "finished_at": None, "metadata": {"first_raw_seconds": None}},
    ]
    before = deepcopy(rows)
    report = usage.budget_observation(rows)
    timing = report["timing"]
    assert timing["completed_intervals"] == 3 and timing["missing_intervals"] == 1
    assert timing["request_seconds_sum"] == 25
    assert timing["request_interval_union_seconds"] == 20
    assert timing["request_span_seconds"] == 25
    assert timing["first_raw_seconds"] == {"observations": 1, "missing": 3, "max": 2, "p50": 2, "p95": 2}
    assert timing["max_raw_idle_seconds"]["max"] == 10
    assert report["tasks"]["verify"]["timing"] == timing
    assert rows == before


def test_invalid_and_missing_timings_remain_unknown():
    rows = [{"task": "extract", "status": "failed", "sent_at": "invalid",
             "finished_at": "2026-11-04T00:00:00+00:00",
             "metadata": {"first_raw_seconds": float("nan"), "first_content_seconds": True,
                          "max_raw_idle_seconds": -1, "semaphore_wait_seconds": float("inf")}}]
    timing = usage.budget_observation(rows)["timing"]
    assert timing["completed_intervals"] == 0 and timing["missing_intervals"] == 1
    assert timing["request_seconds_sum"] is None
    assert timing["request_interval_union_seconds"] is None
    assert timing["request_span_seconds"] is None
    assert timing["max_raw_idle_seconds"] == {"observations": 0, "missing": 1, "max": None, "p50": None, "p95": None}


def test_reasoning_and_visible_output_do_not_treat_unknown_usage_as_zero():
    rows = [
        {"task": "extract", "status": "succeeded", "usage": {
            "completion_tokens": 100, "completion_tokens_details": {"reasoning_tokens": 20}}},
        {"task": "extract", "status": "failed", "usage": {"completion_tokens": 50}},
        {"task": "extract", "status": "uncertain", "usage": None},
    ]
    result = usage.budget_observation(rows)["tasks"]["extract"]
    assert result["reasoning_tokens"] == {"observations": 1, "missing": 2, "total": 20}
    assert result["visible_completion_tokens"] == {"observations": 1, "missing": 2, "total": 80}
