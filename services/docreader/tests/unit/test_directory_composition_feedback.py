"""One finite correction sees all composition entries that lack a decision."""

from copy import deepcopy

import pytest

from docreader.tender.directory_compile import DirectoryPlanRejected, compile_directory
from docreader.llm.request import ModelContractError
from docreader.tender.authoring_pipeline import _accept_ref_directory_plan
from docreader.tender.directory_sources import SourceRefs
from test_directory_compile import docs, entry, item, node, plan, window


def test_unresolved_composition_feedback_includes_each_missing_source_entry():
    documents = docs(['投标函\n商务文件分组\n报价文件分组', '投标函\n固定正文'])
    raw = plan([node('letter', 1, 0)], [item('letter', entry=0)],
               entries=[entry(0, 0), entry(0, 1), entry(0, 2)])
    before = deepcopy(raw)
    with pytest.raises(DirectoryPlanRejected) as caught:
        compile_directory(raw, documents)
    assert '商务文件分组' in caught.value.detail
    assert '报价文件分组' in caught.value.detail
    assert raw == before


def test_body_feedback_reports_each_known_conflict_pair_using_model_visible_refs():
    documents = docs(['甲原式\n乙原式\n丙原式\n丁原式\n戊原式\n第一段\n第二段\n第三段'])
    raw = {'nodes': [{'heading_ref': heading, 'response_kind': 'form', 'body_ranges': ranges}
                     for heading, ranges in [(0, [[5, 5]]), (1, [[5, 5]]), (2, [[6, 7]]),
                                             (3, [[6, 6]]), (4, [[7, 7]])]],
           'scopes': [{'items': [{'node_ref': heading, 'depth': 'self', 'presentation': 'section'}
                                for heading in range(5)]}]}
    before = deepcopy(raw)
    with pytest.raises(ModelContractError) as caught:
        _accept_ref_directory_plan(raw, SourceRefs(documents), documents)
    details = '\n'.join(error['detail'] for error in caught.value.errors)
    for left, right in [(0, 1), (2, 3), (2, 4)]:
        assert f'heading_ref={left} and heading_ref={right}' in details
    assert details.count('overlap at') == 3
    assert raw == before


def test_one_unlisted_instance_reports_conflict_with_every_distinct_submission_instance():
    documents = docs(['共同原式\n共同填写位', '甲单独提交\n乙单独提交'])
    raw = plan([{**node(identity, 0, 0), 'body_windows': [window(0, 1)]}
                for identity in ('first', 'second', 'unlisted')],
               [item('first', entry=0), item('second', entry=1), item('unlisted')],
               entries=[entry(1, 0), entry(1, 1)])
    with pytest.raises(DirectoryPlanRejected) as caught:
        compile_directory(raw, documents)
    assert 'nodes first and unlisted' in caught.value.detail
    assert 'nodes second and unlisted' in caught.value.detail
    assert 'nodes first and second' not in caught.value.detail
