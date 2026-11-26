"""Pack scheduling must respect the configured model service's capacity."""

import asyncio

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.workflow.nodes import TenderNodes


@pytest.mark.parametrize("concurrency,pack_count,expected_max", [(None, 12, 6), (2, 6, 2)])
async def test_workflow_respects_configured_pack_concurrency_without_dropping_work(
    tmp_path, monkeypatch, concurrency, pack_count, expected_max
):
    kwargs = {} if concurrency is None else {"concurrency": concurrency}
    nodes = TenderNodes(None, ArtifactStore(tmp_path), None, object(), **kwargs)
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
        for number in range(pack_count)
    ]}
    results = await nodes._packs(index, "extract-pack", process)

    assert maximum == expected_max
    assert [item["pack_id"] for item in results if not isinstance(item, BaseException)] == [
        f"p{number}" for number in range(pack_count)]
