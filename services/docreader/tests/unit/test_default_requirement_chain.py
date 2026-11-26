"""New automatic outlines use the bounded requirement chain unless the old contract is named."""

import json
import re
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender import document_execution
from docreader.tender.document_execution import execute_document
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.item_placement import REQUIREMENT_CHAIN_CONTRACT
from docreader.tender.schema import AuthoringInput


MAIN = (
    '第六章 投标文件格式\n'
    '附件1 投标函\n致：招标人\nFULLTEXT_MAIN\n'
    '附件2 报价表\n单价\n'
)
AMENDMENT = (
    '补遗一\n投标文件格式\n'
    '附件1 投标函\n致：招标人（补遗）\nFULLTEXT_AMENDMENT\n'
    '附件9 廉洁承诺书\n我方承诺\n'
)
CLARIFICATION = '答疑\n报价表\n致：采购人（答疑稿）\nFULLTEXT_CLARIFICATION\n'
SENTINELS = ('FULLTEXT_MAIN', 'FULLTEXT_ATTACHMENT', 'FULLTEXT_AMENDMENT', 'FULLTEXT_CLARIFICATION')


def _annex_page(count):
    lines = ['第六章 投标文件格式']
    for number in range(1, count + 1):
        lines.append(f'附件{number} 第{number}号承诺书' + '说明' * 400)
        lines.append('承诺正文')
    lines.append('FULLTEXT_ATTACHMENT')
    return '\n'.join(lines) + '\n'


def _frozen_document(artifacts, document_id, role, text):
    locator = {'locator_kind': 'document', 'section_ordinal': 0, 'page_ordinal': 0,
               'heading_path': document_id}
    raw = {'schema_version': 2, 'units': [{'unit_id': 'unit:0', 'kind': 'section', 'ordinal': 0,
            'text': text, 'grid': None, 'locator': locator, 'physical_path': 'native:unit:0',
            'header_cells': []}],
           'source_contract': {'units': [{'unit_id': 'unit:0', 'section_id': 'section',
                                          'parent_section_id': None, 'heading_path': document_id,
                                          'physical_locator': locator}], 'page_manifest': []},
           'images': {}, 'ocr': {}}
    document = {'document_id': document_id, 'document_version': '1',
                'original_ref': artifacts.put_bytes(text.encode(), 'text/plain')}
    raw_ref = artifacts.put_json(raw)
    index = SourceIndex(document, raw)
    return {**document, 'role': role, 'source_index_ref': artifacts.put_json(index.descriptor(raw_ref))}, index


def _memory_store():
    rows = {}

    async def get(execution_id, **_kwargs):
        row = rows.get(execution_id)
        return None if row is None else json.loads(json.dumps(row))

    async def submit(execution_id, authoring_input, spec):
        current = rows.get(execution_id)
        if current is None:
            rows[execution_id] = {'execution_id': execution_id, 'authoring_input': authoring_input,
                                  'execution_spec': spec, 'status': 'queued', 'cancel_requested': False,
                                  'requests': [], 'receipts': []}
        elif current['authoring_input'] != authoring_input or current['execution_spec'] != spec:
            from docreader.integrations.store import Conflict
            raise Conflict('execution_id already identifies another fixed input')

    async def acquire(execution_id, **_kwargs):
        return None if rows[execution_id]['status'] == 'succeeded' else SimpleNamespace(
            execution_id=execution_id, epoch=1)

    async def release(lease, status, **kwargs):
        rows[lease.execution_id].update(status=status, **kwargs)

    async def commit_artifact(lease, stage, reference, **kwargs):
        rows[lease.execution_id]['receipts'].append({'stage': stage, 'artifact_ref': reference, **kwargs})

    store = SimpleNamespace(setup=AsyncMock(), heartbeat=AsyncMock(), get_execution=get, submit=submit,
                            acquire=acquire, release=release, commit_artifact=commit_artifact,
                            register_plan=AsyncMock(), receipt=AsyncMock(return_value=None), rows=rows)
    return store


def _model(sent, *, fail_second_annex=False):
    annex_calls = {'count': 0}

    class Model:
        def __init__(self, config, *_args, **_kwargs):
            self.config = config

        async def __aenter__(self):
            return self

        async def __aexit__(self, *_args):
            return None

        async def call_request(self, request, budget, *, task, execution_spec, logical_request=None):
            body = json.loads(request['messages'][1]['content'][0]['text'])
            sent.append((task, body))
            if task == 'directory_plan':
                lines = body['lines']
                if any(re.match(r'附件\d+ 第\d+号承诺书', text.strip()) for _ref, text in lines):
                    annex_calls['count'] += 1
                    if fail_second_annex and annex_calls['count'] == 2:
                        raise ModelWorkUnitExhausted(task)
                chapters = []
                for ref, text in lines:
                    match = re.match(r'附件\d+ (第\d+号承诺书|投标函|报价表|廉洁承诺书)', text.strip())
                    if match:
                        chapters.append({'heading_refs': [ref], 'name': match[1]})
                return {'chapters': chapters}
            ids = [row['id'] for row in body['requirements']]
            return {'placements': [[identity, None] for identity in ids]}

    return Model


def _bind(monkeypatch, store):
    monkeypatch.setattr(document_execution, 'AuthoringStore', lambda _dsn: store)
    monkeypatch.setattr(document_execution, 'assert_authoring_active', AsyncMock())


async def _execute(artifacts, store, snapshot, **options):
    config = SimpleNamespace(chat=SimpleNamespace(model='fixture', context_tokens=options.pop('context_tokens'),
                                                  base_url='http://fixture.invalid', reasoning_effort=None))
    budget = RequestBudget(context_tokens=config.chat.context_tokens, model='fixture',
                           output_reserve=options.pop('output_reserve'), prompts={})
    return await execute_document(SimpleNamespace(dsn='fixture'), artifacts, config, budget,
                                  run_id=snapshot['run_id'], source_version=snapshot['source_version'],
                                  domain_revision=snapshot['domain_revision'],
                                  authoring_input_ref=artifacts.put_json(snapshot), **options)


@pytest.mark.asyncio
async def test_new_outline_defaults_to_per_document_bid_format_chain(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    documents = []
    ranges = {}
    for document_id, role, text in (
        ('tender', 'main', MAIN),
        ('forms', 'attachment', _annex_page(24)),
        ('addendum', 'amendment', AMENDMENT),
        ('answers', 'clarification', CLARIFICATION),
    ):
        record, index = _frozen_document(artifacts, document_id, role, text)
        documents.append(record)
        ranges[document_id] = index.ranges[0]['range_id']
    frozen_ref = artifacts.put_json({'documents': documents})
    short = {'item_id': 'quote', 'kind': 'submission_requirement', 'text': '按答疑稿提交报价表',
             'evidence': [{'range_id': ranges['answers'], 'quote': '报价表'}],
             'submission': {'documents': ['报价表']}}
    huge = {'item_id': 'bulk', 'kind': 'obligation', 'text': '义务' * 30000,
            'evidence': [{'range_id': ranges['tender'], 'quote': '投标函'}]}
    extract_ref = artifacts.put_json({'items': [short, huge]})
    snapshot = AuthoringInput.model_validate({'schema_version': 2, 'run_id': 'default-chain', 'source_version': 1, 'domain_revision': 1,
                              'frozen_ref': frozen_ref, 'extract_refs': [extract_ref]}).model_dump()
    sent = []
    store = _memory_store()
    _bind(monkeypatch, store)
    monkeypatch.setattr(document_execution, 'OpenAIModel', _model(sent, fail_second_annex=True))

    reference = await _execute(artifacts, store, snapshot, execution_id='new-outline',
                               outline_only=True, context_tokens=16000, output_reserve=2048)
    outline = artifacts.read_json(reference)
    spec = store.rows['new-outline']['execution_spec']
    assert spec['requirement_chain_contract'] == REQUIREMENT_CHAIN_CONTRACT
    assert spec['directory_planning_contract'] == 'bid_format_source_outline_v1'
    assert 'directory_native_body_contract' not in spec

    directory = [body for task, body in sent if task == 'directory_plan']
    placement = [body for task, body in sent if task == 'item_placement']
    assert directory and placement, [task for task, _body in sent]
    assert all(len(body['requirements']) <= 240 for body in placement)
    for body in directory:
        blob = '\n'.join(text for _ref, text in body['lines'])
        assert sum(sentinel in blob for sentinel in SENTINELS) < len(SENTINELS)
    annex = [body for body in directory
             if any(re.match(r'附件\d+ 第\d+号承诺书', text.strip()) for _ref, text in body['lines'])]
    others = [body for body in directory if body not in annex]
    other_text = '\n'.join(text for body in others for _ref, text in body['lines'])
    assert len(annex) > 1
    assert '附件1 投标函' in other_text and '廉洁承诺书' in other_text and '报价表' in other_text
    assert not any(all(sentinel in '\n'.join(text for _ref, text in body['lines']) for sentinel in SENTINELS)
                   for body in directory)
    titles = [chapter['title'] for chapter in outline['chapters']]
    assert '投标函' in titles and '廉洁承诺书' in titles and '报价表' in titles
    assert titles.count('投标函') == 2
    assert any(title.endswith('承诺书') for title in titles)
    missed = {text.split(' ', 1)[1] for body in annex[1:2] for _ref, text in body['lines'] if text.startswith('附件')}
    assert missed and not missed & set(titles)
    codes = [issue['code'] for issue in outline['issues']]
    assert 'outline_directory_plan_failed' in codes
    assert 'outline_directory_format_conflict' in codes
    assert 'outline_item_placement_failed' in codes
    assert 'outline_directory_revision_unreviewed' not in codes

    def windows(chapters):
        for chapter in chapters:
            yield chapter['title'], {window['document_id'] for window in chapter.get('body_windows') or []}
            yield from windows(chapter.get('children') or [])

    assert all(len(owners) <= 1 for _title, owners in windows(outline['chapters']))


@pytest.mark.asyncio
async def test_successor_outline_inherits_the_requirement_chain(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    record, _index = _frozen_document(artifacts, 'tender', 'main', MAIN)
    frozen_ref = artifacts.put_json({'documents': [record]})
    snapshot = AuthoringInput.model_validate({'schema_version': 2, 'run_id': 'inherit', 'source_version': 1, 'domain_revision': 1,
                              'frozen_ref': frozen_ref}).model_dump()
    store = _memory_store()
    _bind(monkeypatch, store)
    monkeypatch.setattr(document_execution, 'OpenAIModel', _model([]))
    await _execute(artifacts, store, snapshot, execution_id='chain-prev', outline_only=True,
                   context_tokens=16000, output_reserve=2048)
    assert store.rows['chain-prev']['execution_spec']['requirement_chain_contract'] == REQUIREMENT_CHAIN_CONTRACT

    followed = []
    monkeypatch.setattr(document_execution, 'OpenAIModel', _model(followed))
    await _execute(artifacts, store, snapshot, execution_id='chain-next', outline_only=True,
                   previous_execution_id='chain-prev', context_tokens=16000, output_reserve=2048)
    assert store.rows['chain-next']['execution_spec']['requirement_chain_contract'] == REQUIREMENT_CHAIN_CONTRACT
