"""Unknown persisted business states must not enter ordinary execution paths."""
from contextlib import asynccontextmanager
import json
from types import SimpleNamespace
from unittest.mock import AsyncMock
from uuid import uuid4

import pytest
from pydantic import TypeAdapter

from docreader.integrations.authoring_store import AuthoringStore
from docreader.integrations.store import Store
from docreader.integrations.web import WebBridge
from docreader.tender.status import (
    AuthoringExecutionStatus, AuthoringJobStatus, CommandStatus, ModelRequestStatus,
    ProjectStatus, RunStatus,
)
from docreader.workflow.graph import build_graph


@pytest.mark.parametrize("status_type, values", [
    (RunStatus, ["pending", "running", "waiting-confirmation", "analysis-ready", "response-plan-ready", "blocked", "failed", "cancelled"]),
    (CommandStatus, ["pending", "claimed", "published", "running", "completed", "failed", "cancelled"]),
    (ModelRequestStatus, ["prepared", "sent", "succeeded", "failed", "uncertain"]),
    (AuthoringExecutionStatus, ["pending", "running", "succeeded", "failed", "cancelled"]),
    (AuthoringJobStatus, ["pending", "published", "succeeded", "failed", "cancelled"]),
    (ProjectStatus, ["open", "ended"]),
])
def test_host_statuses_preserve_wire_values(status_type, values):
    assert json.loads(json.dumps(list(status_type))) == values
    assert TypeAdapter(status_type).json_schema()["enum"] == values
    for value in values:
        assert TypeAdapter(status_type).validate_json(json.dumps(value), strict=True) is status_type(value)
    with pytest.raises(ValueError):
        status_type("unknown")


def connection(row):
    cursor = SimpleNamespace(fetchone=AsyncMock(return_value=row), fetchall=AsyncMock(return_value=[row]))
    return SimpleNamespace(execute=AsyncMock(return_value=cursor))


@pytest.mark.asyncio
async def test_unknown_run_status_is_rejected_at_read():
    with pytest.raises(ValueError, match="RunStatus"):
        await Store("")._run(connection({"status": "broken"}), "run")


@pytest.mark.asyncio
async def test_unknown_execution_status_is_rejected_at_read():
    with pytest.raises(ValueError, match="AuthoringExecutionStatus"):
        await AuthoringStore("")._execution(connection({"status": "broken"}), "execution")


@pytest.mark.asyncio
async def test_unknown_command_status_is_rejected_at_read():
    @asynccontextmanager
    async def transaction():
        yield connection({"status": "broken"})

    store = Store("")
    store.transaction = transaction
    with pytest.raises(ValueError, match="CommandStatus"):
        await store.get_command("command")


@pytest.mark.asyncio
async def test_known_run_status_is_parsed_without_changing_its_json_value():
    row = await Store("")._run(connection({"status": "running"}), "run")
    assert row["status"] is RunStatus.RUNNING
    assert json.loads(json.dumps(row)) == {"status": "running"}


@pytest.mark.asyncio
async def test_unknown_model_receipt_status_is_rejected_at_read():
    with pytest.raises(ValueError, match="ModelRequestStatus"):
        await Store("")._matching_model_requests(connection({"status": "broken"}), "run", 1, 1, "extract", "a" * 64)


@pytest.mark.asyncio
async def test_unknown_project_status_is_rejected_even_for_read():
    @asynccontextmanager
    async def transaction():
        yield connection({"status": "broken"})

    bridge = WebBridge.__new__(WebBridge)
    bridge.store = SimpleNamespace(transaction=transaction)
    with pytest.raises(ValueError, match="ProjectStatus"):
        await bridge.authorize({"project_id": str(uuid4()), "owner_user_id": str(uuid4())})


@pytest.mark.asyncio
async def test_unknown_job_status_is_not_returned_as_normal_progress():
    bridge = WebBridge.__new__(WebBridge)
    bridge.authoring = SimpleNamespace(get_execution=AsyncMock(return_value=None))
    with pytest.raises(ValueError, match="AuthoringJobStatus"):
        await bridge.authoring_view({"execution_id": "job", "status": "broken", "payload": {}, "error": None})


@pytest.mark.asyncio
@pytest.mark.parametrize("store, method, entity", [
    (Store(""), "release", "RunStatus"),
    (Store(""), "finish_request", "ModelRequestStatus"),
    (AuthoringStore(""), "release", "AuthoringExecutionStatus"),
    (AuthoringStore(""), "finish_request", "ModelRequestStatus"),
])
async def test_unknown_write_status_is_rejected_before_opening_database(store, method, entity):
    with pytest.raises(ValueError, match=entity):
        await getattr(store, method)(None, "broken")


@pytest.mark.asyncio
@pytest.mark.parametrize("state, error", [({"status": "unknown"}, ValueError), ({}, KeyError)])
async def test_graph_does_not_route_unknown_or_missing_status_to_confirmation(state, error):
    nodes = SimpleNamespace(**{name: AsyncMock(return_value={})
                              for name in ("register", "freeze", "extract", "verify", "confirm")})
    graph = build_graph(nodes, checkpointer=None)
    with pytest.raises(error):
        await graph.ainvoke(state)
    nodes.confirm.assert_not_called()
