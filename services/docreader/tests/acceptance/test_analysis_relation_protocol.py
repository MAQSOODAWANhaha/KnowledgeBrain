"""A regrouped review plan cannot resume an immutable prior-protocol run."""

import pytest

from docreader.integrations.store import Conflict
from docreader.tests.acceptance.test_analysis import AnalysisModel
from docreader.tests.acceptance.test_analysis import analysis_runtime as analysis_runtime
from docreader.workflow.nodes import TenderNodes
from docreader.workflow.state import WORKFLOW_PROTOCOL


pytestmark = pytest.mark.integration


@pytest.mark.parametrize("previous_version", [10, 11])
async def test_previous_review_grouping_protocol_cannot_resume(analysis_runtime, previous_version):
    store, artifacts, _, run, calls = analysis_runtime
    command = (await store.get_run(run["run_id"]))["commands"][-1]
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    previous = {**WORKFLOW_PROTOCOL, "normalization_version": previous_version}
    await store.pin_execution_spec(lease, {"workflow_protocol": previous})
    before = await store.get_run(run["run_id"])
    nodes = TenderNodes(store, artifacts, lease, AnalysisModel(store, artifacts, lease, calls, {}))
    with pytest.raises(Conflict, match="prior revisions are read-only"):
        await nodes.prepare()
    assert not calls
    assert await store.get_run(run["run_id"]) == before
