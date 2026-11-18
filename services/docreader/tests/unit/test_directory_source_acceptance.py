"""Invalid source windows must not become accepted paid-response receipts."""

from types import SimpleNamespace

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender.authoring_pipeline import _request_directory_plans
import docreader.tender.document_execution as execution
from test_directory_compile import docs, item, node, plan, window
from test_directory_plan_replay import Receipts


async def test_invalid_window_is_rejected_before_successful_response_receipt(tmp_path, monkeypatch):
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    budget = RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384)
    client = SimpleNamespace(config=SimpleNamespace(chat=SimpleNamespace(base_url='https://fixture.invalid/v1')))
    executor = execution._DocumentExecutor(client, store, artifacts,
        SimpleNamespace(execution_id='invalid-window', epoch=1), budget, {}, None)

    async def model(_model, _request, validate):
        return validate(plan([{**node('letter', 0, 0), 'body_windows': [window(0, 0, 99)]}], [item('letter')]))

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    with pytest.raises(ModelContractError) as caught:
        await _request_directory_plans(docs(['投标函\n正文']), budget, executor)
    assert caught.value.errors[0]['code'] == 'directory_source_invalid'
    assert store.rows == {}
