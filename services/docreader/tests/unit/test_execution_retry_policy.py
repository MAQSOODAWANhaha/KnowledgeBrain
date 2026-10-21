"""Every failure retries three times; cancellation, stale leases and exhaustion stop."""

import asyncio
import json

import psycopg
import pytest

from docreader.integrations.store import Cancelled, Conflict, RequestReplayBlocked, StaleExecution
from docreader.jobs.dispatcher import dispatch_once
from docreader.jobs.tasks import execution_retry, tender_command
from docreader.llm.request import (
    ModelBudgetError, ModelConfigurationError, ModelCorrectionStalled, ModelInfrastructureError, ModelSemanticError,
    UncertainModelResponse,
)


@pytest.mark.parametrize("error", [
    ValueError("invalid content"), TypeError("invalid shape"), RuntimeError("operation failed"),
    json.JSONDecodeError("invalid JSON", "{", 1),
    ModelSemanticError("invalid_json_response"), ModelBudgetError("request too large"),
    ModelConfigurationError("configuration unavailable"), ModelInfrastructureError("service rejected"),
    ModelCorrectionStalled("no progress"), UncertainModelResponse("provider may have executed"),
    RequestReplayBlocked("request", "sent"), RequestReplayBlocked("request", "uncertain"),
    ConnectionError("connection failed"), TimeoutError("timed out"),
    psycopg.OperationalError("database unavailable"), psycopg.InterfaceError("connection closed"),
    Conflict("immutable inputs changed"),
])
@pytest.mark.parametrize(("retries", "expected"), [(0, True), (2, True), (3, False), (4, False)])
def test_known_failures_get_at_most_three_retries(error, retries, expected):
    assert execution_retry(retries, error) is expected


@pytest.mark.parametrize("error", [
    Cancelled("cancel requested"), StaleExecution("lease expired"), asyncio.CancelledError(), KeyboardInterrupt(),
])
@pytest.mark.parametrize("retries", [0, 2])
def test_stopped_execution_never_retries(error, retries):
    assert execution_retry(retries, error) is False


def test_actor_uses_three_retries_and_the_common_policy():
    assert tender_command.options["max_retries"] == 3
    assert tender_command.options["retry_when"] is execution_retry


@pytest.mark.parametrize("retries", [0, 2, 3])
def test_exhausted_model_work_unit_never_gets_another_queue_retry(retries):
    from docreader.llm.request import ModelWorkUnitExhausted

    assert execution_retry(retries, ModelWorkUnitExhausted("verify")) is False


class ClaimedCommands:
    def __init__(self):
        self.commands = [
            {"run_id": "run", "command_id": "rejected", "source_version": 1, "claim_token": "first"},
            {"run_id": "run", "command_id": "accepted", "source_version": 1, "claim_token": "second"},
        ]
        self.failures = []
        self.published = []

    async def claim_commands(self, **kwargs):
        return self.commands

    async def publish_failed(self, command_id, claim_token, error, *, retryable):
        self.failures.append((command_id, claim_token, error, retryable))

    async def mark_published(self, command_id, claim_token):
        self.published.append((command_id, claim_token))


@pytest.mark.parametrize("error", [
    ValueError("synthetic private details"), TypeError("synthetic private details"),
    RuntimeError("synthetic private details"), ConnectionError("synthetic private details"),
])
async def test_all_publish_failures_use_the_shared_retry_budget_and_continue(error, caplog):
    store = ClaimedCommands()

    def send(run_id, command_id, source_version):
        if command_id == "rejected":
            raise error

    assert await dispatch_once(store, send) == 1
    assert store.failures == [("rejected", "first", type(error).__name__, True)]
    assert store.published == [("accepted", "second")]
    assert "synthetic private details" not in caplog.text


async def test_publisher_cancellation_propagates_without_consuming_retry_budget():
    store = ClaimedCommands()

    def send(*args):
        raise asyncio.CancelledError()

    with pytest.raises(asyncio.CancelledError):
        await dispatch_once(store, send)
    assert not store.failures
    assert not store.published
