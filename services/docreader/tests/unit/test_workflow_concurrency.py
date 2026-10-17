"""Pack scheduling must respect the configured model service's capacity."""

import asyncio

from docreader.integrations.artifacts import ArtifactStore
from docreader.workflow.nodes import TenderNodes


async def test_workflow_runs_at_most_two_packs_without_dropping_work(tmp_path, monkeypatch):
    nodes = TenderNodes(None, ArtifactStore(tmp_path), None, object())
    monkeypatch.setattr(nodes.repo, "load_pack", lambda ref: ref)

    async def save(stage, operation, **kwargs):
        return await operation()

    monkeypatch.setattr(nodes, "_save", save)
    active = maximum = 0

    async def process(pack):
        nonlocal active, maximum
        active += 1
        maximum = max(maximum, active)
        await asyncio.sleep(0.01)
        active -= 1
        return pack

    index = {"task": "extract", "packs": [
        {"pack_id": f"p{number}", "artifact_ref": {"pack_id": f"p{number}"}}
        for number in range(6)
    ]}
    results = await nodes._packs(index, "extract-pack", process)

    assert maximum == 2
    assert [item["pack_id"] for item in results] == [f"p{number}" for number in range(6)]
