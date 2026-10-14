"""A queue task owns one fenced graph invocation, ending at persistent interrupt."""
import asyncio
from contextlib import suppress

import psycopg
from langgraph.types import Command
from psycopg.rows import dict_row

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Cancelled, StaleExecution, Store
from docreader.workflow.graph import build_graph
from docreader.workflow.persistence import FencedPostgresSaver


async def execute_command(run_id, command_id, version, settings, node_factory):
    store = Store(settings.dsn)
    lease = await store.acquire(run_id, command_id, version, lease_seconds=settings.lease_seconds)
    if lease is None:
        return  # A completed, obsolete, or concurrently owned delivery is harmless.
    nodes = None

    async def invoke():
        nonlocal nodes
        artifacts = ArtifactStore(settings.artifact_root)
        async with await psycopg.AsyncConnection.connect(
            settings.dsn, autocommit=True, prepare_threshold=0, row_factory=dict_row,
        ) as conn:
            saver = FencedPostgresSaver(conn, store, lease)
            nodes = node_factory(store, artifacts, lease)
            graph = build_graph(nodes, saver)
            config = {"configurable": {"thread_id": f"{run_id}:{version}"}}
            previous = await graph.aget_state(config)
            cmd = await store.get_command(command_id)
            if cmd["kind"] == "resume" and any(task.interrupts for task in previous.tasks):
                value = Command(resume=cmd["payload"])
            elif previous.values:
                value = None
            else:
                value = {"run_id": run_id, "version": version, "status": "running"}
            # sync durability ensures interrupt and node states finish saving before receipt.
            await graph.ainvoke(value, config, durability="sync")
            final = await graph.aget_state(config)
            state = final.values
            paused = any(task.interrupts for task in final.tasks)
            status = "waiting-confirmation" if paused else state.get("status", "failed")
            if status not in {"waiting-confirmation", "response-plan-ready", "blocked", "failed"}:
                raise RuntimeError("graph ended without an explicit terminal state")
            await store.release(lease, status, checklist_digest=state.get("checklist_digest"))

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
    except (psycopg.OperationalError, psycopg.InterfaceError, ConnectionError, TimeoutError):
        with suppress(StaleExecution, psycopg.Error):
            await store.release(lease, "retry")
        raise
    except Exception as error:
        with suppress(StaleExecution, psycopg.Error):
            await store.release(lease, "failed", error=type(error).__name__)
        raise
    finally:
        for task in (graph_task, heart_task):
            task.cancel()
        for task in (graph_task, heart_task):
            with suppress(asyncio.CancelledError, Exception):
                await task
        if nodes is not None and hasattr(nodes, "close"):
            await nodes.close()
