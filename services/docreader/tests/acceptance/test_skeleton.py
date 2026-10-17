"""Run with TENDER_TEST_DATABASE_URL and TENDER_TEST_REDIS_URL against real services."""
import asyncio
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid

import pytest
import psycopg
from psycopg import sql
from psycopg.conninfo import make_conninfo
from psycopg.rows import dict_row

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import InvalidConfirmation, StaleExecution, Store
from docreader.jobs.dispatcher import dispatch_once
from docreader.jobs.settings import Settings
from docreader.workflow.persistence import FencedPostgresSaver, initialize_checkpoints
from docreader.workflow.graph import build_graph
from docreader.workflow.runner import execute_command

sys.path.insert(0, str(Path(__file__).parent))
from skeleton_worker import SkeletonNodes, skeleton_command

pytestmark = pytest.mark.integration


@pytest.fixture
async def setup(tmp_path, monkeypatch):
    dsn = os.environ.get("TENDER_TEST_DATABASE_URL")
    if not dsn:
        pytest.skip("real PostgreSQL required; set TENDER_TEST_DATABASE_URL")
    base_dsn = dsn
    schema = "tender_accept_" + uuid.uuid4().hex
    async with await psycopg.AsyncConnection.connect(base_dsn, autocommit=True) as conn:
        await conn.execute(sql.SQL("CREATE SCHEMA {}").format(sql.Identifier(schema)))
    dsn = make_conninfo(base_dsn, options=f"-c search_path={schema}")
    redis_url = os.environ.get("TENDER_TEST_REDIS_URL", "redis://127.0.0.1:16379/1")
    monkeypatch.setenv("TENDER_DATABASE_URL", dsn)
    monkeypatch.setenv("TENDER_REDIS_URL", redis_url)
    monkeypatch.setenv("TENDER_ARTIFACT_ROOT", str(tmp_path))
    settings = Settings(dsn, redis_url, tmp_path, lease_seconds=2, heartbeat_seconds=0.2, redeliver_seconds=0.1)
    store = Store(dsn)
    await store.setup()
    await initialize_checkpoints(dsn)
    manifest = ArtifactStore(tmp_path).put_json({"documents": []})
    run = await store.submit(manifest, uuid.uuid4().hex)
    try:
        yield store, settings, run
    finally:
        async with await psycopg.AsyncConnection.connect(base_dsn, autocommit=True) as conn:
            await conn.execute(sql.SQL("DROP SCHEMA {} CASCADE").format(sql.Identifier(schema)))


async def command_for(store, run):
    data = await store.get_run(run["run_id"])
    return data["commands"][-1]


async def wait_status(store, run_id, status, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        row = await store.get_run(run_id)
        if row["status"] == status:
            return row
        await asyncio.sleep(0.1)
    raise AssertionError(f"run did not reach {status}; final status={row['status']}")


async def test_duplicate_pause_restart_confirm_and_cancel(setup):
    store, settings, run = setup
    cmd = await command_for(store, run)
    args = (run["run_id"], cmd["command_id"], 1, settings, SkeletonNodes)
    await asyncio.gather(execute_command(*args), execute_command(*args))
    row = await store.get_run(run["run_id"])
    assert row["status"] == "waiting-confirmation"
    assert len(row["artifacts"]) == 1
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run["run_id"], 1, "stale", "invalid")
    approval = await store.confirm(run["run_id"], 1, row["checklist_digest"], "approval")
    again = await store.confirm(run["run_id"], 1, row["checklist_digest"], "approval")
    assert approval == again
    cmd = await command_for(store, run)
    await execute_command(run["run_id"], cmd["command_id"], 1, settings, SkeletonNodes)
    assert (await store.get_run(run["run_id"]))["status"] == "analysis-ready"


async def test_redis_failure_claim_tokens_and_receipt_replay(setup):
    store, settings, run = setup

    def unavailable(*args):
        raise ConnectionError("injected Redis unavailable")

    await dispatch_once(store, unavailable, redeliver_seconds=0.1)
    cmd = await command_for(store, run)
    assert cmd["status"] == "pending"
    lease = await store.acquire(run["run_id"], cmd["command_id"], 1, lease_seconds=2)
    ref = ArtifactStore(settings.artifact_root).put_json({"committed_before_checkpoint": True})
    first = await store.commit_artifact(lease, "register", ref)
    await store.release(lease, "retry")  # crash boundary: no graph checkpoint exists
    await execute_command(run["run_id"], cmd["command_id"], 1, settings, SkeletonNodes)
    row = await store.get_run(run["run_id"])
    assert row["status"] == "waiting-confirmation"
    assert len(row["artifacts"]) == 1
    assert row["artifacts"][0]["artifact_ref"] == first["artifact_ref"]
    with pytest.raises(StaleExecution):
        await store.commit_artifact(lease, "stale", ref)


async def test_cancel_running_graph_observed_by_heartbeat(setup, monkeypatch, tmp_path):
    store, settings, run = setup
    marker = tmp_path / "hold"
    marker.touch()
    monkeypatch.setenv("TENDER_TEST_BLOCK", str(marker))
    cmd = await command_for(store, run)
    task = asyncio.create_task(execute_command(run["run_id"], cmd["command_id"], 1, settings, SkeletonNodes))
    await wait_status(store, run["run_id"], "running")
    await store.cancel(run["run_id"])
    await asyncio.wait_for(task, 3)
    assert (await store.get_run(run["run_id"]))["status"] == "cancelled"


async def test_old_executor_cannot_write_postgres_checkpoint(setup):
    store, settings, run = setup
    cmd = await command_for(store, run)
    lease = await store.acquire(run["run_id"], cmd["command_id"], 1)
    config = {"configurable": {"thread_id": f"{run['run_id']}:1:1"}}
    async with await psycopg.AsyncConnection.connect(
        settings.dsn, autocommit=True, row_factory=dict_row,
    ) as conn:
        old = FencedPostgresSaver(conn, store, lease)
        graph = build_graph(SkeletonNodes(store, ArtifactStore(settings.artifact_root), lease), old)
        await graph.ainvoke({"run_id": run["run_id"], "source_version": 1, "domain_revision": 1}, config, durability="sync")
        checkpoint = await old.aget_tuple(config)
        await store.release(lease, "retry")
        replacement = await store.acquire(run["run_id"], cmd["command_id"], 1)
        assert replacement.epoch > lease.epoch
        with pytest.raises(StaleExecution):
            await old.aput(checkpoint.config, checkpoint.checkpoint, checkpoint.metadata, {})
        await store.release(replacement, "blocked")


async def test_artifact_initialization_error_has_terminal_receipt(setup, tmp_path):
    store, settings, run = setup
    marker = tmp_path / "is-a-file"
    marker.write_text("not a directory")
    invalid = Settings(settings.dsn, settings.redis_url, marker / "artifacts")
    cmd = await command_for(store, run)
    for attempt in range(4):
        with pytest.raises(OSError):
            await execute_command(run["run_id"], cmd["command_id"], 1, invalid, SkeletonNodes)
        current = await store.get_run(run["run_id"])
        assert current["status"] == ("pending" if attempt < 3 else "failed")
        assert current["commands"][0]["infrastructure_failures"] == attempt + 1
    failed = await store.get_run(run["run_id"])
    assert failed["status"] == "failed"
    assert failed["commands"][0]["receipt"]["status"] == "failed"


async def test_real_graph_checkpoint_failure_after_business_commit(setup, monkeypatch):
    store, settings, run = setup
    cmd = await command_for(store, run)
    original = FencedPostgresSaver.aput
    faulted = False

    async def lose_checkpoint(self, config, checkpoint, metadata, versions):
        nonlocal faulted
        if checkpoint["channel_values"].get("registered_ref") and not faulted:
            faulted = True
            raise psycopg.OperationalError("injected checkpoint connection failure")
        return await original(self, config, checkpoint, metadata, versions)

    monkeypatch.setattr(FencedPostgresSaver, "aput", lose_checkpoint)
    with pytest.raises(psycopg.OperationalError):
        await execute_command(run["run_id"], cmd["command_id"], 1, settings, SkeletonNodes)
    failed = await store.get_run(run["run_id"])
    assert faulted and failed["status"] == "pending"
    assert len(failed["artifacts"]) == 1
    ref = failed["artifacts"][0]["artifact_ref"]
    await execute_command(run["run_id"], cmd["command_id"], 1, settings, SkeletonNodes)
    restored = await store.get_run(run["run_id"])
    assert restored["status"] == "waiting-confirmation"
    assert restored["artifacts"][0]["artifact_ref"] == ref
    assert restored["artifacts"][0]["epoch"] == 1


async def test_dispatcher_survives_real_database_connection_failure(setup, monkeypatch):
    from docreader.jobs import dispatcher, tasks
    store, settings, run = setup
    attempts = 0
    delivered = asyncio.Event()
    loop = asyncio.get_running_loop()

    class OnceDisconnected(Store):
        async def claim_commands(self, *args, **kwargs):
            nonlocal attempts
            attempts += 1
            if attempts == 1:
                # Real refused PostgreSQL connection, followed by the healthy original DSN.
                await psycopg.AsyncConnection.connect("host=127.0.0.1 port=9 connect_timeout=1")
            return await super().claim_commands(*args, **kwargs)

    def send(*args):
        loop.call_soon_threadsafe(delivered.set)

    monkeypatch.setattr(dispatcher, "Store", OnceDisconnected)
    monkeypatch.setattr(tasks.tender_command, "send", send)
    task = asyncio.create_task(dispatcher.dispatch_forever())
    try:
        await asyncio.wait_for(delivered.wait(), 8)
        for _ in range(20):
            if (await command_for(store, run))["status"] == "published":
                break
            await asyncio.sleep(0.01)
        assert attempts >= 2
        assert (await command_for(store, run))["status"] == "published"
    finally:
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task


async def test_worker_sigkill_redelivery_and_persistent_interrupt(setup, tmp_path):
    store, settings, run = setup
    marker = tmp_path / "hold-worker"
    marker.touch()
    env = {**os.environ, "TENDER_TEST_BLOCK": str(marker), "PYTHONPATH": str(Path.cwd().parent)}
    log = (tmp_path / "worker.log").open("w")

    def start():
        return subprocess.Popen([sys.executable, "-m", "dramatiq", "skeleton_worker",
                                 "--path", str(Path(__file__).parent), "--processes", "1", "--threads", "2"],
                                env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)

    proc = start()
    try:
        await dispatch_once(store, skeleton_command.send, redeliver_seconds=0.1)
        await wait_status(store, run["run_id"], "running")
        # Ensure at least one committed business artifact precedes the hard kill.
        for _ in range(100):
            if (await store.get_run(run["run_id"]))["artifacts"]:
                break
            await asyncio.sleep(0.05)
        os.killpg(proc.pid, signal.SIGKILL)
        await asyncio.to_thread(proc.wait)
        marker.unlink()
        await asyncio.sleep(2.2)
        proc = start()
        await dispatch_once(store, skeleton_command.send, redeliver_seconds=0.1)
        row = await wait_status(store, run["run_id"], "waiting-confirmation")
        assert len(row["artifacts"]) == 1
        os.killpg(proc.pid, signal.SIGKILL)
        await asyncio.to_thread(proc.wait)
        await store.confirm(run["run_id"], 1, row["checklist_digest"], "after-restart")
        proc = start()
        await dispatch_once(store, skeleton_command.send, redeliver_seconds=0.1)
        await wait_status(store, run["run_id"], "analysis-ready")
    finally:
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGKILL)
            await asyncio.to_thread(proc.wait)
        log.close()
