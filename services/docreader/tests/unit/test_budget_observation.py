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
    assert result["planning"]["unplanned_phases"] == ["extract"]
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


def test_analysis_failures_tie_rejections_via_wire_and_rank_by_tokens():
    base = {"schema_version": 2, "operation": "review", "work_unit_id": "review-1"}
    corrected = {**base, "contract_feedback": {"errors": [{"code": "list_type"}], "rejection_count": 1}}
    rows = [
        # Failed resend shares the digest with the succeeded send whose response was rejected.
        {"task": "verify", "status": "failed", "error": {"type": "http_429"}, "usage": None,
         "metadata": {"operation": "review", "work_unit_id": "review-1", "request_digest": "d-base"}},
        {"task": "verify", "status": "succeeded", "usage": {"total_tokens": 110},
         "metadata": {"operation": "review", "work_unit_id": "review-1", "request_digest": "d-base"}},
        {"task": "verify", "status": "succeeded", "usage": {"total_tokens": 125},
         "metadata": {"operation": "review", "work_unit_id": "review-1", "request_digest": "d-corr"}},
        {"task": "extract", "status": "uncertain", "error": {"type": "between_data_idle_timeout"},
         "usage": {"total_tokens": 150}, "metadata": {"operation": "extract", "request_digest": "d-ext"}},
        {"task": "verify", "status": "succeeded", "usage": {"total_tokens": 90},
         "metadata": {"operation": "review", "work_unit_id": "review-1", "request_digest": "d-ok"}},
    ]
    wires = {"d-base": base, "d-corr": corrected}
    rejections = [
        {"task": "verify", "errors": [{"code": "list_type"}, {"code": "other"}], "payload": base},
        {"task": "verify", "errors": [{"code": "list_type"}], "payload": corrected},
        {"task": "ocr", "errors": [{"code": "ocr_response_invalid"}], "payload": {"image_sha256": "abc"}},
    ]
    failures = usage.budget_observation(rows, rejections=rejections, wires=wires)["failures"]
    assert [(f["stage"], f["task"], f["kind"], f["code"], f["count"], f["reported_tokens"], f["unknown_usage"])
            for f in failures] == [
        ("review", "verify", "contract", "list_type", 2, 235, 0),
        ("extract", "extract", "send", "between_data_idle_timeout", 1, 150, 0),
        ("ocr", "ocr", "contract", "ocr_response_invalid", 1, 0, 1),
        ("review", "verify", "send", "http_429", 1, 0, 1),
    ]
    assert sum(f["count"] for f in failures if f["kind"] == "contract") == len(rejections)


async def test_cli_status_reads_wire_and_rejection_artifacts_for_failures(tmp_path, monkeypatch):
    from argparse import Namespace
    from types import SimpleNamespace

    from docreader.integrations import cli
    from docreader.integrations.artifacts import ArtifactStore

    artifacts = ArtifactStore(tmp_path)
    payload = {"schema_version": 2, "operation": "review", "work_unit_id": "review-1"}
    wire_ref = artifacts.put_json({"protocol": "tender_short_refs_v2", "payload": payload, "mapping": {}})
    req_ref = artifacts.put_json(payload)
    rej_ref = artifacts.put_json({"task": "verify", "request_ref": req_ref, "errors": [{"code": "list_type"}]})
    run_row = {
        "run_id": "run-1", "status": "failed",
        "artifacts": [
            {"stage": "model-wire", "scope": "digest-1", "artifact_ref": wire_ref},
            {"stage": "model-rejection", "scope": "id:0", "artifact_ref": rej_ref},
        ],
        "usage": [
            {"task": "verify", "status": "failed", "error": {"type": "http_500"}, "usage": None,
             "metadata": {"operation": "review", "work_unit_id": "review-1", "request_digest": "digest-1"}},
            {"task": "verify", "status": "succeeded", "usage": {"total_tokens": 140},
             "metadata": {"operation": "review", "work_unit_id": "review-1", "request_digest": "digest-1"}},
        ],
    }

    class FakeStore:
        def __init__(self, dsn):
            pass

        async def get_run(self, run_id):
            assert run_id == "run-1"
            return run_row

    monkeypatch.setattr(cli.Settings, "from_env", classmethod(
        lambda cls: SimpleNamespace(dsn="unused", artifact_root=tmp_path)))
    monkeypatch.setattr(cli, "Store", FakeStore)
    status = await cli.run(Namespace(command="status", run_id="run-1"))
    assert isinstance(status, dict)
    observation = status["budget_observation"]
    assert isinstance(observation, dict)
    assert observation["failures"] == [
        {"stage": "review", "task": "verify", "kind": "contract", "code": "list_type",
         "count": 1, "reported_tokens": 140, "unknown_usage": 0},
        {"stage": "review", "task": "verify", "kind": "send", "code": "http_500",
         "count": 1, "reported_tokens": 0, "unknown_usage": 1},
    ]
