"""A queue task owns one fenced graph invocation, ending at persistent interrupt."""
import asyncio
from contextlib import suppress
import inspect
from typing import Any, cast

import psycopg
from langchain_core.runnables import RunnableConfig
from langgraph.types import Command
from psycopg import AsyncConnection
from psycopg.rows import AsyncRowFactory, TupleRow, dict_row

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Cancelled, Conflict, StaleExecution, Store
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender.status import RunStatus
from docreader.workflow.graph import build_graph
from docreader.workflow.persistence import FencedPostgresSaver

_CONFIG_CONFLICT_MARKERS = (
    "execution configuration changed",
    "request budget changed",
    "execution protocol changed",
    "artifact inputs changed",
)


def _config_conflict_reason(error: Conflict) -> str | None:
    """Classify deterministic config/spec drift so operators see the real blocker instead of retry exhaustion."""
    message = str(error).lower()
    if any(marker in message for marker in _CONFIG_CONFLICT_MARKERS):
        return "execution_config_changed"
    return None


async def execute_command(run_id, command_id, source_version, settings, node_factory):
    store = Store(settings.dsn)
    lease = await store.acquire(run_id, command_id, source_version, lease_seconds=settings.lease_seconds)
    if lease is None:
        return  # A completed, obsolete, or concurrently owned delivery is harmless.
    nodes = None

    async def invoke():
        nonlocal nodes
        artifacts = ArtifactStore(settings.artifact_root)
        async with await AsyncConnection.connect(
            settings.dsn, autocommit=True, prepare_threshold=0,
            row_factory=cast(AsyncRowFactory[TupleRow], dict_row),
        ) as conn:
            saver = FencedPostgresSaver(conn, store, lease)
            nodes = node_factory(store, artifacts, lease)
            config: RunnableConfig = {"configurable": {"thread_id": f"{run_id}:{source_version}:{lease.domain_revision}"}}
            cmd = await store.get_command(command_id)
            graph = None
            previous = None
            resuming_confirmation = False
            if cmd is not None and cmd["kind"] == "resume":
                graph = build_graph(nodes, saver)
                previous = await graph.aget_state(config)
                resuming_confirmation = any(task.name == "confirm" and task.interrupts for task in previous.tasks)
            if hasattr(nodes, "prepare"):
                if resuming_confirmation and "resuming_confirmation" in inspect.signature(nodes.prepare).parameters:
                    await nodes.prepare(resuming_confirmation=True)
                else:
                    await nodes.prepare()
            if graph is None or previous is None:
                graph = build_graph(nodes, saver)
                previous = await graph.aget_state(config)
            if cmd is not None and cmd["kind"] == "resume" and any(task.interrupts for task in previous.tasks):
                value = Command(resume={key: value for key, value in cmd["payload"].items() if key != "retry"})
            elif previous.values:
                value = None
            else:
                value = {"run_id": run_id, "source_version": source_version,
                         "domain_revision": lease.domain_revision, "status": RunStatus.RUNNING}
            # sync durability ensures interrupt and node states finish saving before receipt.
            await graph.ainvoke(cast(Any, value), config, durability="sync")
            final = await graph.aget_state(config)
            state = final.values
            paused = any(task.interrupts for task in final.tasks)
            status = RunStatus.WAITING_CONFIRMATION if paused else RunStatus(state["status"])
            if status not in {RunStatus.WAITING_CONFIRMATION, RunStatus.ANALYSIS_READY, RunStatus.BLOCKED, RunStatus.FAILED}:
                raise RuntimeError("graph ended without an explicit terminal state")
            metadata = None
            if status == RunStatus.WAITING_CONFIRMATION and state.get("verification_ref"):
                # Store.confirm validates human verdicts against this before persisting them.
                checked = await asyncio.to_thread(artifacts.read_json, state["verification_ref"])
                metadata = {"human_issue_ids": checked.get("human_issue_ids", [])}
            await store.release(lease, status, checklist_digest=state.get("checklist_digest"), metadata=metadata)

    async def heartbeat():
        while True:
            await asyncio.sleep(settings.heartbeat_seconds)
            await store.heartbeat(lease, lease_seconds=settings.lease_seconds)

    graph_task = asyncio.create_task(invoke())
    heart_task = asyncio.create_task(heartbeat())
    try:
        done, _ = await asyncio.wait({graph_task, heart_task}, return_when=asyncio.FIRST_COMPLETED)
        if heart_task in done:
            await heart_task  # Lost lease/cancel/DB connection stops in-flight graph.
        await graph_task
    except (Cancelled, StaleExecution):
        return
    except ModelWorkUnitExhausted as error:
        with suppress(StaleExecution, psycopg.Error):
            await store.release(lease, cast(RunStatus, RunStatus.FAILED), error=type(error).__name__,
                                metadata={"reason": "model_work_unit_retries_exhausted"})
        raise
    except Conflict as error:
        reason = _config_conflict_reason(error)
        with suppress(StaleExecution, psycopg.Error):
            if reason is not None:
                await store.release(lease, cast(RunStatus, RunStatus.FAILED), error=type(error).__name__,
                                    metadata={"reason": reason})
            else:
                await store.release(lease, "retry", error=type(error).__name__)
        raise
    except Exception as error:
        with suppress(StaleExecution, psycopg.Error):
            await store.release(lease, "retry", error=type(error).__name__)
        raise
    finally:
        for task in (graph_task, heart_task):
            task.cancel()
        for task in (graph_task, heart_task):
            with suppress(asyncio.CancelledError, Exception):
                await task
        if nodes is not None and hasattr(nodes, "close"):
            await cast(Any, nodes).close()
