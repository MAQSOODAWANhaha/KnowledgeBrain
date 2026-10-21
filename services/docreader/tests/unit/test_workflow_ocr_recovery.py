"""OCR structure errors use the shared recovery policy; unreadable images do not."""

from copy import deepcopy

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Lease
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.workflow.nodes import TenderNodes


class Receipts:
    def __init__(self):
        self.rows = {}

    async def receipt(self, lease, stage, scope='', *, input_digest=''):
        return self.rows.get((stage, scope, input_digest))

    async def commit_artifact(self, lease, stage, artifact_ref, scope='', *, input_digest='', **kwargs):
        self.rows[stage, scope, input_digest] = {'artifact_ref': artifact_ref}


class OCRModel:
    def __init__(self, values):
        self.values = iter(values)
        self.calls = []

    async def call(self, task, payload, images=()):
        self.calls.append((task, deepcopy(payload), list(images)))
        return next(self.values)


async def test_invalid_ocr_fields_retry_with_feedback_and_the_same_image(tmp_path):
    valid = {'text': '图中原文', 'status': 'complete', 'issues': []}
    model = OCRModel([{**valid, 'text': None}, {**valid, 'status': 'done'},
                      {**valid, 'issues': 'none'}, valid])
    store = Receipts()
    nodes = TenderNodes(store, ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'), model)
    image = 'data:image/png;base64,YWJj'
    assert await nodes.call('ocr', {'image_sha256': 'image'}, images=[image]) == valid
    assert len(model.calls) == 4
    assert [call[2] for call in model.calls] == [[image]] * 4
    assert [call[1]['contract_feedback']['errors'][0]['path'] for call in model.calls[1:]] == [
        ['text'], ['status'], ['issues']]
    assert len(store.rows) == 3


async def test_invalid_ocr_response_exhausts_the_same_four_attempts(tmp_path):
    model = OCRModel([{'status': 'complete'}] * 5)
    nodes = TenderNodes(Receipts(), ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'), model)
    with pytest.raises(ModelWorkUnitExhausted):
        await nodes.call('ocr', {'image_sha256': 'image'})
    assert len(model.calls) == 4


@pytest.mark.parametrize('status', ['partial', 'unresolved'])
async def test_valid_unreadable_ocr_result_keeps_its_business_issue_without_retry(tmp_path, status):
    value = {'text': '', 'status': status,
             'issues': [{'code': 'image_region_unreadable', 'detail': 'blurred', 'blocking': True}]}
    model = OCRModel([value])
    nodes = TenderNodes(Receipts(), ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'), model)
    assert await nodes.call('ocr', {'image_sha256': 'image'}) == value
    assert len(model.calls) == 1
