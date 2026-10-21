"""Cancellation and lost fences take priority; model exhaustion stops further retries."""

import pytest

from docreader.integrations.store import Cancelled, RequestReplayBlocked, StaleExecution
from docreader.llm import ModelSemanticError, UncertainModelResponse
from docreader.llm.request import ModelCorrectionStalled, ModelWorkUnitExhausted
from docreader.workflow.nodes import _raise_failures


@pytest.mark.parametrize("stop", [
    Cancelled("Run was cancelled"),
    StaleExecution("Lease was replaced"),
], ids=["cancelled", "stale"])
@pytest.mark.parametrize("other", [
    ModelSemanticError("Complete response contains invalid JSON"),
    ConnectionError("Connection failed before sending"),
    ModelWorkUnitExhausted("verify"),
], ids=["semantic", "connection", "unit-exhausted"])
@pytest.mark.parametrize("stop_first", [False, True], ids=["stop-last", "stop-first"])
def test_cancellation_and_lost_fence_take_priority_over_sibling_failure(stop, other, stop_first):
    successful = {"pack_id": "accepted-pack", "artifact_ref": {"sha256": "a" * 64}}
    results = [stop, successful, other] if stop_first else [other, successful, stop]
    with pytest.raises(type(stop)) as raised:
        _raise_failures(results)
    assert raised.value is stop
    assert results[1] is successful


@pytest.mark.parametrize("other", [
    UncertainModelResponse("Provider execution is uncertain"),
    RequestReplayBlocked("previously-sent-request", "uncertain"),
    ModelCorrectionStalled("Same correction rejected"),
    ModelSemanticError("Invalid JSON"),
])
@pytest.mark.parametrize("exhausted_first", [False, True])
def test_work_unit_exhaustion_stops_outer_retries(other, exhausted_first):
    exhausted = ModelWorkUnitExhausted("verify")
    results = [exhausted, other] if exhausted_first else [other, exhausted]
    with pytest.raises(ModelWorkUnitExhausted) as raised:
        _raise_failures(results)
    assert raised.value is exhausted


@pytest.mark.parametrize("semantic_first", [False, True])
def test_definite_failures_are_still_raised_for_bounded_queue_retry(semantic_first):
    semantic = ModelSemanticError("Complete response contains invalid JSON")
    connection = ConnectionError("Connection failed before sending")
    results = [semantic, connection] if semantic_first else [connection, semantic]
    with pytest.raises(type(results[0])) as raised:
        _raise_failures(results)
    assert raised.value is results[0]


def test_successful_siblings_without_failure_do_not_request_retry():
    assert _raise_failures([{"pack_id": "one"}, {"pack_id": "two"}]) is None
