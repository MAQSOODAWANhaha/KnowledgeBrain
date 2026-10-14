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

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import InvalidConfirmation, StaleExecution, Store
from docreader.jobs.dispatcher import dispatch_once
from docreader.jobs.settings import Settings
from docreader.workflow.persistence import FencedPostgresSaver, initialize_checkpoints
from docreader.workflow.runner import execute_command

sys.path.insert(0, str(Path(__file__).parent))
from skeleton_worker import SkeletonNodes, skeleton_command

pytestmark = pytest.mark.integration


@pytest.fixture
async def setup(tmp_path, monkeypatch):
    dsn = os.environ.get("TENDER_TEST_DATABASE_URL")
    if not dsn:
        pytest.skip("real PostgreSQL required; set TENDER_TEST_DATABASE_URL")
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
    return store, settings, run


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
    assert (await store.get_run(run["run_id"]))["status"] == "response-plan-ready"


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
        await wait_status(store, run["run_id"], "response-plan-ready")
    finally:
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGKILL)
            await asyncio.to_thread(proc.wait)
        log.close()
