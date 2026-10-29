"""Progress describes current immutable receipts, never estimated model completion."""
from copy import deepcopy
from datetime import datetime, timedelta, timezone
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from docreader.integrations.web import WebBridge
from docreader.integrations.web_progress import analysis_progress, authoring_progress


NOW = datetime(2026, 10, 28, tzinfo=timezone.utc)
IDENTITY = {"run_id": "fixed-run", "source_version": 2, "domain_revision": 3}


def request(task="extract", status="sent", *, logical="a", epoch=4, second=0, **metadata):
    return {**IDENTITY, "request_id": f"request-{task}-{second}", "epoch": epoch,
            "task": task, "status": status, "created_at": NOW + timedelta(seconds=second),
            "metadata": {"logical_request_digest": logical * 64, **metadata}}


def receipt(stage, scope="", *, revision=3, source=2, second=0):
    return {**IDENTITY, "source_version": source, "domain_revision": revision,
            "stage": stage, "scope": scope, "artifact_ref": {"sha256": "a" * 64},
            "committed_at": NOW + timedelta(seconds=second)}


def run(**changes):
    return {**IDENTITY, "epoch": 4, "status": "running", "lease_live": True,
            "cancel_requested": False, "usage": [], "artifacts": [], "commands": [], **changes}


def job(**changes):
    return {"execution_id": "fixed-execution", "status": "pending", "error": None,
            "payload": {"identity": IDENTITY, "operation": "document"}, **changes}


def execution(**changes):
    return {"execution_id": "fixed-execution", "authoring_input": IDENTITY,
            "epoch": 4, "status": "running", "lease_live": True, "cancel_requested": False,
            "receipts": [], "requests": [], **changes}


def test_analysis_counts_only_current_accepted_units_and_keeps_source_revision_zero():
    value = run(artifacts=[
        receipt("ocr", "current-image", revision=0, second=1),
        receipt("extract-pack", "current-pack", second=2),
        receipt("analysis-review", "review-one", second=3),
        receipt("analysis-locate", "locate-one", second=4),
        receipt("extract-pack", "old-source", source=1, second=8),
        receipt("extract-pack", "old-revision", revision=2, second=9),
        receipt("extract-pack", "invalid-zero-semantic", revision=0, second=10),
        receipt("ocr", "invalid-revision-source", revision=3, second=11),
        receipt("model-wire", "wire", second=12),
    ], history={"artifacts": [receipt("analysis-joint", "historical", second=20)]})
    before = deepcopy(value)
    progress = analysis_progress(value)
    assert value == before
    assert progress["identity"] == IDENTITY
    assert progress["accepted_units"] == 4
    assert progress["accepted_by_phase"] == [
        {"phase": "ocr", "count": 1}, {"phase": "extract", "count": 1},
        {"phase": "analysis-review", "count": 1}, {"phase": "analysis-locate", "count": 1},
    ]
    assert progress["last_completed_phase"] == "analysis-locate"
    assert progress["total_units"] is None


def test_root_milestones_are_real_recent_progress_but_not_extra_accepted_work_units():
    progress = analysis_progress(run(artifacts=[
        receipt("extract", second=5), receipt("extract-pack", "one", second=2),
        receipt("register", revision=0, second=1), receipt("source-spec", revision=0, second=10),
    ]))
    assert progress["accepted_units"] == 1
    assert progress["last_completed_phase"] == "extract"
    assert progress["current_phase"] is None


def test_transport_success_and_semantic_rejection_do_not_increment_accepted_units():
    progress = analysis_progress(run(usage=[request(status="succeeded")],
        artifacts=[receipt("model-rejection", "feedback-one")]))
    assert progress["accepted_units"] == 0
    assert progress["request_states"]["received"] == 1
    assert progress["rejected_responses"] == 1
    assert progress["last_completed_phase"] is None


def test_retries_are_latest_logical_request_states_not_inflated_finished_units():
    progress = analysis_progress(run(usage=[
        request(status="failed", second=1), request(status="succeeded", second=2),
        request("verify", status="sent", logical="b", second=3, operation="review"),
        request("verify", status="uncertain", logical="c", second=4, operation="associate"),
        request(status="failed", logical="d", second=5),
    ]))
    assert progress["accepted_units"] == 0
    assert progress["request_states"] == {"prepared": 0, "sent": 1, "uncertain": 1, "failed": 1, "received": 1}
    assert progress["active_tasks"] == [{"phase": "analysis-review", "prepared": 0, "sent": 1}]
    assert progress["current_phase"] == "analysis-review"
    assert progress["latest_request_phase"] == "extract"


def test_joint_review_phase_uses_the_existing_host_work_unit_kind_without_exposing_its_id():
    progress = analysis_progress(run(usage=[request("verify", operation="review", work_unit_id="joint-private-identity")]))
    assert progress["current_phase"] == "analysis-joint"
    assert "joint-private-identity" not in str(progress)


def test_group_progress_counts_physical_rejections_once_and_uses_host_joint_phase():
    rejected = {**receipt("model-group-outcome", "physical-attempt"), "metadata": {"rejected_response": True}}
    progress = analysis_progress(run(usage=[request("verify", operation="joint", work_unit_id="")], artifacts=[
        rejected, deepcopy(rejected), receipt("model-group-rejection", "member-a"),
        receipt("model-group-rejection", "member-b"), receipt("analysis-associate", "member-a"),
        receipt("analysis-associate", "member-b"),
        {**receipt("model-group-outcome", "transport-failure"), "metadata": {"rejected_response": False}},
    ]))
    assert progress["current_phase"] == "analysis-joint"
    assert progress["rejected_responses"] == 1
    assert progress["accepted_by_phase"] == [{"phase": "analysis-associate", "count": 2}]
    assert progress["total_units"] is None


@pytest.mark.parametrize("change", [
    {"status": "failed"}, {"status": "cancelled"}, {"cancel_requested": True},
    {"lease_live": False}, {"epoch": 5},
])
def test_inactive_owner_or_old_epoch_cannot_claim_live_model_activity(change):
    progress = analysis_progress(run(usage=[request()], **change))
    assert progress["current_phase"] is None and progress["active_tasks"] == []


def test_old_identity_requests_never_become_current_progress():
    progress = analysis_progress(run(usage=[
        {**request(), "source_version": 1}, {**request(second=1), "domain_revision": 2},
        {**request(second=2), "run_id": "another-run"},
    ]))
    assert progress["current_phase"] is None
    assert progress["latest_request_phase"] is None
    assert not any(progress["request_states"].values())


def test_latest_receipt_uses_real_commit_time_instead_of_stage_query_order():
    progress = analysis_progress(run(artifacts=[
        receipt("analysis-review", "z", second=10), receipt("extract-pack", "a", second=5),
        receipt("verify", second=15), receipt("register", revision=0, second=1),
    ]))
    assert progress["last_completed_phase"] == "verify"


def test_no_timestamp_does_not_invent_latest_analysis_phase():
    value = receipt("extract-pack", "one")
    del value["committed_at"]
    progress = analysis_progress(run(artifacts=[value]))
    assert progress["accepted_units"] == 1 and progress["last_completed_phase"] is None


def test_duplicate_receipt_identity_does_not_double_count_units():
    value = receipt("extract-pack", "one")
    progress = analysis_progress(run(artifacts=[value, deepcopy(value)]))
    assert progress["accepted_units"] == 1


def test_authoring_uses_its_fixed_snapshot_and_semantically_accepted_response_receipts():
    fixed = {**IDENTITY, "source_version": 1, "domain_revision": 1}
    value = execution(authoring_input=fixed, receipts=[
        {"stage": "document-model-response", "scope": "outline:one"},
        {"stage": "document-model-response", "scope": "authoring_write:two"},
        {"stage": "document-model-response", "scope": "authoring_write:two"},
        {"stage": "model-request", "scope": "sent-request"},
        {"stage": "model-rejection", "scope": "bad-response"},
    ], requests=[request("authoring_review", status="prepared")])
    progress = authoring_progress(job(), value)
    assert progress["identity"] == {**fixed, "execution_id": "fixed-execution"}
    assert progress["accepted_units"] == 2
    assert progress["accepted_by_phase"] == [{"phase": "outline", "count": 1}, {"phase": "authoring_write", "count": 1}]
    assert progress["last_completed_phase"] == "authoring_write"
    assert progress["current_phase"] == "authoring_review"
    assert progress["rejected_responses"] == 1


def test_authoring_append_order_and_final_result_are_observed_milestones():
    current = execution(receipts=[
        {"stage": "document-outline", "scope": ""},
        {"stage": "knowledge-snapshot", "scope": "query"},
    ])
    assert authoring_progress(job(), current)["last_completed_phase"] == "knowledge"
    current.update(status="succeeded", result_ref={"sha256": "a" * 64})
    progress = authoring_progress(job(), current)
    assert progress["last_completed_phase"] == "result" and progress["accepted_units"] == 0
    assert progress["state"] == "succeeded"


def test_queued_job_without_execution_has_fixed_identity_and_no_invented_units():
    progress = authoring_progress(job(), None)
    assert progress["identity"] == {**IDENTITY, "execution_id": "fixed-execution"}
    assert progress["state"] == "pending" and progress["accepted_units"] == 0
    assert progress["current_phase"] is None and progress["last_completed_phase"] is None


def test_progress_exposes_only_known_phases_not_private_request_or_scope_identifiers():
    value = run(usage=[request("sensitive-task-identifier", work_unit_id="secret-id")],
        artifacts=[receipt("not-a-known-stage", "private-scope")])
    progress = analysis_progress(value)
    assert progress["current_phase"] == "other"
    assert "secret-id" not in str(progress) and "private-scope" not in str(progress)
    assert "sensitive-task-identifier" not in str(progress) and "request_id" not in str(progress)


async def test_authoring_view_adds_projection_from_existing_execution_read():
    bridge = WebBridge.__new__(WebBridge)
    bridge.authoring = SimpleNamespace(get_execution=AsyncMock(return_value=execution(requests=[])))
    view = await bridge.authoring_view(job())
    assert view["progress"]["identity"] == {**IDENTITY, "execution_id": "fixed-execution"}
    bridge.authoring.get_execution.assert_awaited_once_with("fixed-execution", missing_ok=True)


async def test_saved_review_view_keeps_authoring_progress_with_fixed_word_identity():
    from docreader.integrations.web_saved_review import review_view

    bridge = WebBridge.__new__(WebBridge)
    bridge.authoring = SimpleNamespace(get_execution=AsyncMock(return_value=execution(receipts=[
        {"stage": "document-model-response", "scope": "authoring_review:saved-word-request"},
    ])))
    value = job(project_id="fixed-project", payload={"operation": "saved_review", "identity": IDENTITY,
        "snapshot": {**IDENTITY, "frozen_ref": None},
        "request": {"version_id": "fixed-word-version", "docx_sha256": "a" * 64}})
    view = (await review_view(bridge, value))["review"]
    assert view["progress"]["accepted_by_phase"] == [{"phase": "authoring_review", "count": 1}]
    assert view["progress"]["identity"] == {**IDENTITY, "execution_id": "fixed-execution"}
    assert view["version_id"] == "fixed-word-version" and view["docx_sha256"] == "a" * 64
    assert view["sources_current"] is None and view["source_status_issue"]["code"] == "saved_word_source_status_unavailable"


async def test_analysis_view_adds_projection_without_new_model_or_store_calls(monkeypatch):
    from docreader.integrations import web

    bridge = WebBridge.__new__(WebBridge)
    bridge.store, bridge.artifacts = object(), object()
    snapshot = {"verification_ref": None, "confirmation_ref": None}
    read = AsyncMock(return_value=snapshot)
    monkeypatch.setattr(web, "read_authoring_input", read)
    monkeypatch.setattr("docreader.tender.authoring._OutlineContext", lambda *_args:
        SimpleNamespace(payload={"rules": [], "issues": []}, rules={}, rule_origin="source_only"))
    value = run(artifacts=[receipt("extract-pack", "one")])
    view = await bridge.analysis_view(value)
    assert view["progress"]["accepted_units"] == 1
    read.assert_awaited_once_with(bridge.store, bridge.artifacts, **IDENTITY)
