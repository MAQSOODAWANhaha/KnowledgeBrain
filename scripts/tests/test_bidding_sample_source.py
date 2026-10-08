"""Exercise the sample freeze through the shared service's real Office parser."""
import importlib.util
import json
from pathlib import Path
import sys

from docx import Document
from openpyxl import Workbook
import pytest

spec = importlib.util.spec_from_file_location(
    'sample_source', Path(__file__).parents[1] / 'bidding_sample_source.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def test_caption_skips_running_headers_unit_lines_and_prefers_a_number():
    cell = lambda row, column, span, text: type('Cell', (), {
        'row': row, 'column': column, 'col_span': span, 'text': text})()
    grid = type('Grid', (), {
        'column_count': 2,
        'cells': [cell(0, 0, 1, '名称'), cell(0, 1, 1, '')],
    })()
    assert module.form_title(grid, '字段一览\n请按下表填写。') == '字段一览'
    assert module.form_title(grid, '说明正文。\n字段一览') == '字段一览'
    assert module.caption_line('字' * 81) == ''
    assert module.is_unit_line('（单位：元）')
    assert module.caption_line('8A 商务部分摘要表\n（单位：元）') == '8A 商务部分摘要表'
    assert module.caption_line('8B 股权结构\n填写说明') == '8B 股权结构'
    units = []
    for page in (0, 1, 2):
        units.append(type('Unit', (), {
            'text': '投标文件\n正文',
            'locator': type('Locator', (), {'page_ordinal': page})(),
        })())
    running = module.running_lines(units)
    assert '投标文件' in running
    assert module.caption_line('8A 商务部分摘要表\n投标文件', running) == '8A 商务部分摘要表'
    assert module.caption_line('投标文件', running) == ''
    spanning = type('Grid', (), {
        'column_count': 2,
        'cells': [cell(0, 0, 2, '（单位：元）'), cell(1, 0, 1, '名称')],
    })()
    assert module.form_title(spanning, '8A 商务部分摘要表') == '8A 商务部分摘要表'
    kept = type('Grid', (), {
        'column_count': 2,
        'cells': [cell(0, 0, 2, '合并表头'), cell(1, 0, 1, '名称')],
    })()
    assert module.form_title(kept, '不应采用这一行') == '合并表头'
    assert module.is_unit_line('单位：元人民币')
    assert module.is_unit_line('unit: yuan')
    assert not module.is_unit_line('8：报价一览')
    assert module.caption_line('单位：元人民币\n8A 报价表') == '8A 报价表'
    assert module.caption_line('出)。后半句还没写完\n字段一览') == '字段一览'
    assert module.caption_line('）后半句还没写完\n字段一览') == '字段一览'
    assert module.is_fragment('出)。后半句还没写完')
    assert module.is_fragment('）后半句还没写完')
    assert module.caption_line('8：报价一览\n单位：元人民币') == '8：报价一览'
    repeated = []
    for page in (0, 1):
        repeated.append(type('Unit', (), {
            'text': '前言\n全书标题反复出现\n正文',
            'locator': type('Locator', (), {'page_ordinal': page})(),
        })())
    running_middle = module.running_lines(repeated)
    assert '全书标题反复出现' in running_middle
    assert module.caption_line('全书标题反复出现\n8A 报价表', running_middle) == '8A 报价表'
    same_page = [type('Unit', (), {
        'text': '甲\n甲\n正文',
        'locator': type('Locator', (), {'page_ordinal': 0})(),
    })()]
    assert '甲' not in module.running_lines(same_page)
    assert not module.is_unit_line('续表 1：标准一览')
    assert module.caption_line('续表 1：标准一览\n标签：短注') == '续表 1：标准一览'
    assert module.is_fragment('名称及件数(后半没有收束')
    assert not module.is_fragment('清单(包括如下设备)')
    assert module.caption_line('名称及件数(后半没有收束\n清单(包括如下设备)') == '清单(包括如下设备)'
    assert module.caption_line('8F 明细一览表\n序 8F') == '8F 明细一览表'
    assert module.caption_line('表 2 报价一览\n表 2 A') == '表 2 报价一览'
    assert module.caption_line('表 2 A') == '表 2 A'
    section_units = []
    for page in (3, 4):
        section_units.append(type('Unit', (), {
            'text': '# 反复出现的书名标题\n正文',
            'key': f'section:1:page:{page}',
            'locator': type('Locator', (), {'heading_path': ''})(),
        })())
    section_running = module.running_lines(section_units)
    assert '反复出现的书名标题' in section_running
    assert module.caption_line('反复出现的书名标题\n续表 1：标准一览', section_running) == '续表 1：标准一览'
    same_section_page = [
        type('Unit', (), {
            'text': '甲',
            'key': 'section:0:page:1',
            'locator': type('Locator', (), {})(),
        })(),
        type('Unit', (), {
            'text': '甲',
            'key': 'section:0:page:1:1',
            'locator': type('Locator', (), {})(),
        })(),
    ]
    assert '甲' not in module.running_lines(same_section_page)
    stub_grid = type('Grid', (), {
        'column_count': 2,
        'cells': [cell(0, 0, 2, '表 2 A'), cell(1, 0, 1, '名称')],
    })()
    assert module.form_title(stub_grid, '表 2 报价一览') == '表 2 报价一览'
    assert module.form_title(stub_grid, '') == '表 2 A'
    assert module.is_fragment('签订后 2 日内收取，对方须在通知发出')
    assert not module.is_fragment('标准一览（不含甲，含乙）')
    assert module.caption_line(
        '收费标准一览\n签订后 2 日内收取，对方须在通知发出') == '收费标准一览'
    assert module.caption_line(
        '签订后 2 日内收取，对方须在通知发出\n续表 1：标准一览') == '续表 1：标准一览'
    assert module.caption_line('8 报价一览', {'8 报价一览'}) == '8 报价一览'
    assert module.caption_line('续表 1：标准一览', {'续表 1：标准一览'}) == '续表 1：标准一览'
    units = []
    for page in (0, 1):
        units.append(type('Unit', (), {
            'text': '正文',
            'key': f'section:1:page:{page}',
            'locator': type('Locator', (), {'heading_path': '全书'})(),
            'grid': None,
        })())
    units.append(type('Unit', (), {
        'text': '表 2 报价一览',
        'key': 'section:2:page:2',
        'locator': type('Locator', (), {'heading_path': '全书'})(),
        'grid': None,
    })())
    units.append(type('Unit', (), {
        'text': '全书',
        'key': 'section:2:page:2:1',
        'locator': type('Locator', (), {'heading_path': '全书'})(),
        'grid': None,
    })())
    units.append(type('Unit', (), {
        'text': '',
        'key': 'page:2:table:0',
        'locator': type('Locator', (), {'page_ordinal': 2, 'heading_path': '全书'})(),
        'grid': grid,
    })())
    assert module.form_captions(units) == ['表 2 报价一览']
    assert '全书' in module.running_lines(units)


@pytest.mark.parametrize('extension', ['docx', 'xlsx'])
def test_freeze_preserves_v3_sparse_grid_and_source_identity(tmp_path, monkeypatch, extension):
    source = tmp_path / f'source.{extension}'
    if extension == 'docx':
        document = Document()
        document.add_paragraph('表前说明')
        table = document.add_table(rows=2, cols=2)
        table.cell(0, 0).merge(table.cell(0, 1)).text = '合并表头'
        table.cell(1, 0).text = '名称：'
        document.add_paragraph('表后说明')
        document.save(source)
    else:
        document = Workbook()
        sheet = document.active
        sheet.merge_cells('A1:B1')
        sheet['A1'] = '合并表头'
        sheet['A2'] = '名称：'
        sheet['B2'] = '待填'
        document.save(source)
    limits = tmp_path / 'limits.json'
    limits.write_text(json.dumps({'extraction': {}}))
    output = tmp_path / 'frozen'
    monkeypatch.setattr(sys, 'argv', ['bidding_sample_source', '--source', str(source),
                        '--output', str(output), '--limits', str(limits)])
    module.main()
    frozen_path = output / 'frozen-input.json'
    original = frozen_path.read_bytes()
    frozen = json.loads(original)
    parsed = json.loads((output / 'parsed-source.json').read_text())
    parsed_grids = [unit['grid'] for unit in parsed['structured_source_units'] if unit['grid']]
    assert len(parsed_grids) == len(frozen['structured_forms']) == 1
    sources = {s['source_unit_revision_id']: s for s in frozen['source_units']}
    form = frozen['structured_forms'][0]
    definition = form['definition']
    grid = parsed_grids[0]
    assert definition['schema_version'] == 3
    assert definition['form_definition_revision_id'] == form['form_definition_revision_id']
    assert definition['source_unit_revision_id'] == form['source_unit_revision_id']
    assert definition['cells'] == grid['cells']
    assert definition['row_count'] == definition['column_count'] == 2
    assert len(definition['cells']) == 3
    assert definition['cells'][0]['col_span'] == 2
    assert definition['title'] == '合并表头'
    assert not definition['title'].startswith('source_unit:')
    assert definition.get('widths_mm') == grid.get('widths_mm')
    table_source = sources[form['source_unit_revision_id']]
    assert table_source['text'] == ''
    assert not table_source['locator'].get('cells')
    assert table_source['locator']['locator_kind'] == ('document' if extension == 'docx' else 'spreadsheet')
    with pytest.raises(SystemExit, match='frozen input exists'):
        module.main()
    assert frozen_path.read_bytes() == original
