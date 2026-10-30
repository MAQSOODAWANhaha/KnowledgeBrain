"""Authoring scope stays tied to selected requirements, not loaded carrier pages."""
from types import SimpleNamespace

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender import authoring_pipeline as pipeline


def test_loading_associated_evidence_does_not_route_again_from_its_whole_unit(monkeypatch, tmp_path):
    artifacts = ArtifactStore(tmp_path)
    refs = {name: {'source_index_ref': artifacts.put_json({'source': name}), 'unit_id': name}
            for name in ('direct', 'related', 'unrelated')}
    rules = {name: {'kind': 'item', 'item_id': name} for name in refs}
    views = [{'rule_id': name, 'item': {'item_id': name, 'text': name,
              'conditions': ['该事项自身条件'], 'evidence': [{'range_id': name, 'quote': name}]}}
             for name in refs]
    context = SimpleNamespace(input={'issues': []}, rules=rules,
        sources={name: [ref] for name, ref in refs.items()},
        payload={'rules': views, 'issues': [{'issue_id': 'global', 'detail': '全局待核'}],
                 'relations': [
                     {'relation_id': 'needed', 'from_item_id': 'direct', 'to_item_id': 'related',
                      'evidence': [{'range_id': 'related'}]},
                     {'relation_id': 'second-hop', 'from_item_id': 'related', 'to_item_id': 'unrelated',
                      'evidence': [{'range_id': 'related'}]}]})
    monkeypatch.setattr(pipeline, 'load_outline_source', lambda _artifacts, ref: {
        'document': {'role': 'main'}, 'unit': {'text': ref['unit_id']}})
    task = {'task_id': 'w1', 'chapter_id': 'c1', 'title_path': ['正文'], 'targets': [
        {'target_id': 'target', 'kind': 'narrative', 'purpose': '仅应答所选要求',
         'rule_refs': [rules['direct']], 'source_refs': [refs['direct']], 'support_origins': ['tender']}]}
    payload, sources, issues = pipeline._load_task(artifacts, context, task,
        {'response': {'shared_arrangements': []}}, [])
    assert not issues
    assert {r['relation_id'] for r in payload['relations']} == {'needed'}
    assert {r['item']['item_id'] for r in payload['related_rules']} == {'related'}
    assert {r['unit_id'] for r in sources.values()} == {'direct', 'related'}
    assert payload['related_rules'][0]['item']['conditions'] == ['该事项自身条件']
    assert {i['issue_id'] for i in payload['analysis_issues']} == {'global'}


@pytest.mark.parametrize('can_revise', [True, False])
@pytest.mark.parametrize('remains_unresolved', [False, True])
async def test_automatic_outline_is_reviewed_and_repaired_before_publication(tmp_path, can_revise, remains_unresolved):
    from test_authoring_pipeline_boundaries import budget, ordinary_response, payload, snapshot
    artifacts, calls, published = ArtifactStore(tmp_path), [], []

    async def execute(task, request, validate):
        value = payload(request)
        calls.append((task, value.get('operation')))
        if task == 'authoring_structure':
            assert value['units'] == [], 'Do not fake completed writing before outline review.'
            if sum(t == task for t, _ in calls) == 1 or remains_unresolved:
                return validate({'findings': [{'code': 'wrong_order', 'detail': '现有原文要求调整顺序',
                    'suggestion': '按所给候选修正业务章节标题', 'can_revise': can_revise}]})
            return validate({'findings': []})
        response = ordinary_response(task, value)
        if task == 'outline' and value.get('operation') == 'assemble' and 'review_findings' in value:
            assert value['previous_structure']
            response['chapters'][0]['title'] = '修正后的正式组成'
        return validate(response)

    async def publish(reference):
        published.append(artifacts.read_json(reference))

    reference = await pipeline.generate_document(artifacts, snapshot(artifacts), budget(), execute,
        outline_only=True, on_outline=publish)
    final = artifacts.read_json(reference)
    assert final['chapters'][0]['title'] == '修正后的正式组成'
    assert published == [final]
    assert [t for t, _ in calls] == [
        'outline', 'outline', 'authoring_structure', 'outline', 'authoring_structure']
    assert any(issue['code'] == 'wrong_order' for issue in final['issues']) is remains_unresolved


async def test_unchanged_repair_does_not_repeat_structure_review_or_block_export(tmp_path):
    from test_authoring_pipeline_boundaries import budget, ordinary_response, payload, snapshot
    artifacts, calls = ArtifactStore(tmp_path), []

    async def execute(task, request, validate):
        value = payload(request)
        calls.append(task)
        if task == 'authoring_structure':
            return validate({'findings': [{'code': 'check_source', 'detail': '缺少证书原件，无法确认附件内容。',
                'suggestion': '保留已有位置和缺资料问题，待人工补充真实证书。', 'can_revise': False}]})
        if value.get('operation') == 'assemble' and 'review_findings' in value:
            assert value['review_findings'][0]['can_revise'] is False
        return validate(ordinary_response(task, value))

    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, snapshot(artifacts), budget(), execute))
    assert calls.count('authoring_structure') == 1
    assert calls.count('outline') == 3
    assert artifacts.read_bytes(result['docx_ref']).startswith(b'PK')
    assert 'check_source' in {gap['code'] for gap in result['gaps']}
    outline = artifacts.read_json(result['outline_ref'])
    assert outline['chapters'][0]['title'] == '响应部分1'


async def test_structure_feedback_can_address_a_host_preserved_candidate(tmp_path):
    from test_authoring_pipeline_boundaries import budget, ordinary_response, payload, snapshot
    artifacts, revisions = ArtifactStore(tmp_path), []

    async def execute(task, request, validate):
        value = payload(request)
        if task == 'authoring_structure':
            return validate({'findings': [{'code': 'pending_position', 'detail': '待整理位置应核对归属。',
                'suggestion': '核对已有参见和保留位置。', 'can_revise': False}]})
        if value.get('operation') == 'assemble':
            candidate = next(t for c in value['chapters'] for t in c['targets'])
            if 'previous_structure' in value:
                revisions.append(value)
                pending = value['previous_structure'][-1]['targets'][0]
                assert pending['review_target_id'] == 'v2'
                assert pending['candidate_target_ids'] == [candidate['target_id']]
            return validate({'chapters': [{'title': '参见位置', 'targets': [{
                'kind': 'narrative', 'purpose': '参见实际保留位置。',
                'related_target_ids': [candidate['target_id']]}]}]})
        return validate(ordinary_response(task, value))

    saved = artifacts.read_json(await pipeline.generate_document(
        artifacts, snapshot(artifacts, [{'text': '须保留实际技术要求。'}]), budget(), execute,
        outline_only=True))
    assert len(revisions) == 1
    reference, pending = [c['targets'][0] for c in saved['chapters']]
    assert reference['related_target_ids'] == [pending['target_id']]
    assert not any(issue['code'] == 'outline_reference_ambiguous' for issue in saved['issues'])
