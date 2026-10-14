"""Real infrastructure probe, deliberately contains no tender semantics."""
import asyncio
import os
from pathlib import Path

import dramatiq
from langgraph.types import interrupt

from docreader.jobs.broker import broker
from docreader.jobs.settings import Settings
from docreader.workflow.runner import execute_command


class SkeletonNodes:
    def __init__(self, store, artifacts, lease):
        self.store, self.artifacts, self.lease = store, artifacts, lease

    async def register(self, state):
        receipt = await self.store.receipt(self.lease, "register")
        if not receipt:
            ref = self.artifacts.put_json({"run_id": state["run_id"], "version": state["version"]})
            receipt = await self.store.commit_artifact(self.lease, "register", ref)
        return {"registered": receipt["artifact_ref"]}

    async def freeze(self, state):
        marker = os.environ.get("TENDER_TEST_BLOCK")
        while marker and Path(marker).exists():
            await asyncio.sleep(0.1)
        return {"frozen": state["registered"]}

    async def extract(self, state):
        return {"requirements": state["frozen"]}

    async def verify(self, state):
        return {"checked": state["frozen"], "checklist_digest": state["frozen"]["sha256"]}

    async def confirm(self, state):
        approved = interrupt({"checklist_digest": state["checklist_digest"], "version": state["version"]})
        if approved.get("checklist_digest") != state["checklist_digest"]:
            raise ValueError("stale confirmation")
        return {}

    async def plan(self, state):
        return {"status": "response-plan-ready", "plan": state["checked"]}


@dramatiq.actor(broker=broker, actor_name="skeleton_command", queue_name="tender-skeleton", max_retries=0,
                time_limit=float("inf"))
async def skeleton_command(run_id, command_id, version):
    s = Settings.from_env()
    settings = Settings(s.dsn, s.redis_url, s.artifact_root, lease_seconds=2,
                        heartbeat_seconds=0.2, redeliver_seconds=0.1)
    await execute_command(run_id, command_id, version, settings, SkeletonNodes)
