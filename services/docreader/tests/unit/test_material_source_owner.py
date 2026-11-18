"""Physical proximity cannot move explicitly selected material into another tree."""

from docreader.tender.directory_compile import apply_materials, compile_plans, _stable_table_chapter
from test_directory_compile import node, item, plan


def material_case():
    unit = 'page:0:table:0'
    documents = [{'document_id': 'tender', 'role': 'main',
        'pages': ['Source table\nItem Value\nA 10', 'Cover\nSignature', 'Service plan\nInstructions'],
        'line_units': [[None, unit, unit], [None, None], [None, None]],
        'template_parts': {unit: {'source_ref': {'source_index_ref': {'key': 'fixture'}, 'unit_id': unit}, 'selectors': []}},
        'grids': [{'unit_id': unit, 'page': 0, 'rows': [['Item', 'Value'], ['A', '10']]}]}]
    compiled = compile_plans([plan([node('front', 1, 0), node('service', 2, 0)],
                                  [item('front', presentation='cover'), item('service')])], documents)
    return compiled, documents


def test_new_source_contract_keeps_the_explicit_service_owner_instead_of_following_cover():
    compiled, documents = material_case()
    apply_materials(compiled, documents, [{'chapter_id': 'service', 'document_id': 'tender',
        'page': 0, 'start_line': 1, 'end_line': 2}], source_owner=True)
    by_id = {chapter['id']: chapter for chapter in compiled['chapters']}
    assert by_id['service']['material_segments'][0]['part']['source_ref']['unit_id'] == 'page:0:table:0'
    assert not by_id['front'].get('material_segments')


def test_new_source_contract_cannot_explicitly_send_unbound_procurement_material_to_cover():
    compiled, documents = material_case()
    apply_materials(compiled, documents, [{'chapter_id': 'front', 'document_id': 'tender',
        'page': 0, 'start_line': 1, 'end_line': 2}], source_owner=True)
    assert not any(chapter.get('material_segments') for chapter in compiled['chapters'])
    assert any(issue['code'] == 'outline_directory_material_rejected' for issue in compiled['issues'])


def test_new_source_contract_preserves_same_tree_source_position_and_shared_parent():
    parent = {'id': 'parent', 'parent_id': None, 'heading': {'document_id': 'tender', 'page': 3, 'line': 0}}
    first = {'id': 'first', 'parent_id': 'parent', 'heading': {'document_id': 'tender', 'page': 0, 'line': 0}}
    second = {'id': 'second', 'parent_id': 'parent', 'heading': {'document_id': 'tender', 'page': 2, 'line': 0}}
    compiled = {'chapters': [parent, first, second]}
    assert _stable_table_chapter(compiled, 'tender', 1, 0, selected=parent) is first
    assert _stable_table_chapter(compiled, 'tender', 0, -1, selected=second) is parent


def test_exact_source_holder_still_has_priority_over_an_unrelated_selected_chapter():
    compiled, _ = material_case()
    holder, selected = compiled['chapters']
    holder['lines'] = [{'document_id': 'tender', 'page': 0, 'line': 1}]
    assert _stable_table_chapter(compiled, 'tender', 0, 1, selected=selected) is holder


def test_legacy_material_contract_keeps_its_previous_source_position_projection():
    compiled, documents = material_case()
    apply_materials(compiled, documents, [{'chapter_id': 'service', 'document_id': 'tender',
        'page': 0, 'start_line': 1, 'end_line': 2}])
    assert compiled['chapters'][0]['material_segments']
