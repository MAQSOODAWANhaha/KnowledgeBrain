"""Caption helpers; frozen-input acceptance uses the canonical Rust producer."""
import importlib.util
from pathlib import Path

import pytest

spec = importlib.util.spec_from_file_location(
    'source_captions', Path(__file__).parents[1] / 'bidding_source_captions.py')
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


def _cell(row, column, span, text):
    return type('Cell', (), {
        'row': row, 'column': column, 'col_span': span, 'text': text})()


def _grid(columns, cells):
    return type('Grid', (), {'column_count': columns, 'cells': cells})()


def _unit(text, page, grid=None, heading=''):
    return type('Unit', (), {
        'text': text,
        'key': f'section:1:page:{page}',
        'locator': type('Locator', (), {'page_ordinal': page, 'heading_path': heading})(),
        'grid': grid,
    })()


def test_caption_residuals_follow_structure():
    """Short edge headers, furniture-only gaps, and thin stubs.

    Strings are synthetic. Nothing here is a document-specific list.
    """
    paged = [_unit(f'书名 {page + 1}\n正文', page) for page in (0, 1)]
    paged.append(_unit('项目全称一览\n书名', 2))
    paged.append(_unit('', 2, grid=_grid(2, [
        _cell(0, 0, 1, '名称'), _cell(0, 1, 1, '内容')])))
    assert '书名' in module.running_lines(paged)
    assert module.form_captions(paged) == ['项目全称一览']
    banner = [
        _unit('项目全称一览\n书名短题', 0, heading='书名短题'),
        _unit('', 0, grid=_grid(2, [_cell(0, 0, 1, '名称'), _cell(0, 1, 1, '')])),
    ]
    assert '书名短题' not in module.running_lines(banner)
    assert module.form_captions(banner) == ['项目全称一览']
    spanning = _grid(2, [_cell(0, 0, 2, '书名短题'), _cell(1, 0, 1, '名称')])
    assert module.form_title(spanning, '项目全称一览', banners={'书名短题'}) == '项目全称一览'
    assert module.form_title(spanning, '', banners={'书名短题'}) == '书名短题'
    kept = _grid(2, [_cell(0, 0, 2, '合并表头'), _cell(1, 0, 1, '名称')])
    assert module.form_title(kept, '不应采用这一行') == '合并表头'
    assert module.caption_line('原厂资质证书一览\n附件 8F') == '原厂资质证书一览'
    assert module.caption_line('表 2 报价一览\n表 2 A') == '表 2 报价一览'
    assert module.caption_line('表 2 A') == '表 2 A'
    assert module.caption_line('续表 1：标准一览', {'续表 1：标准一览'}) == '续表 1：标准一览'
    headers = [_cell(0, 0, 1, '名称'), _cell(0, 1, 1, '内容'),
               _cell(1, 0, 1, '甲'), _cell(1, 1, 1, '')]
    continued = [
        _unit('全书书名', 0),
        _unit('全书书名', 1),
        _unit('8A 摘要一览', 2),
        _unit('', 2, grid=_grid(2, headers)),
        _unit('全书书名\n（单位：元）', 3),
        _unit('', 3, grid=_grid(2, headers)),
        _unit('全书书名\n续表 1：标准一览', 4),
        _unit('', 4, grid=_grid(2, headers)),
    ]
    assert module.form_captions(continued) == [
        '8A 摘要一览', '8A 摘要一览', '续表 1：标准一览']
    # A repeating header the parser already stripped can survive once, including
    # with spaces or as a full-span cell. A longer line in the window wins.
    # A thin stub loses to a longer heading segment that starts with the stub.
    once = [
        _unit('项目全称一览\n书 名', 0),
        _unit('', 0, grid=_grid(2, [
            _cell(0, 0, 2, '书名'), _cell(1, 0, 1, '名称')])),
    ]
    assert module.form_captions(once, extra_running=['书名']) == ['项目全称一览']
    assert module.form_title(
        _grid(2, [_cell(0, 0, 2, '合并表头'), _cell(1, 0, 1, '名称')]),
        '不应采用这一行') == '合并表头'
    headed = [
        _unit('附件 3Z', 0, heading='分册 > 附件 3Z 资质一览'),
        _unit('', 0, grid=_grid(2, [_cell(0, 0, 1, '名称'), _cell(0, 1, 1, '')])),
    ]
    assert module.form_captions(headed) == ['附件 3Z 资质一览']
    assert module.caption_line('附件 3Z', headings=['分册', '附件 3Z 资质一览']) == '附件 3Z 资质一览'
    assert module.caption_line('附件 3Z', headings=['分册']) == '附件 3Z'
    only_header = [
        _unit('书名', 0),
        _unit('', 0, grid=_grid(2, [_cell(0, 0, 1, '名称'), _cell(0, 1, 1, '')])),
    ]
    assert module.form_captions(only_header, extra_running=['书名']) == ['']
    # 第n页 makes each page a different string, so a 60% edge set never joins
    # them. The folded label is still furniture. A one-page copy then drops.
    marked = [
        _unit('正文\n书名第1页', 0),
        _unit('正文\n书名第2页', 1),
        _unit('项目全称一览\n书名', 2),
        _unit('', 2, grid=_grid(2, [_cell(0, 0, 1, '名称'), _cell(0, 1, 1, '')])),
    ]
    assert '书名' in module.running_lines(marked)
    assert module.form_captions(marked) == ['项目全称一览']
    # The short line is a field inside the grid, not a second-page header and
    # not in the repeating set. It is empty when nothing else is usable, and
    # a longer line that is not a cell wins. A full-span caption stays.
    label_grid = _grid(3, [
        _cell(0, 0, 1, '书名'), _cell(0, 1, 1, ''), _cell(0, 2, 1, ''),
        _cell(1, 0, 1, '甲'), _cell(1, 1, 1, ''), _cell(1, 2, 1, ''),
        _cell(2, 0, 1, '乙'), _cell(2, 1, 1, ''), _cell(2, 2, 1, ''),
    ])
    assert module.form_captions([
        _unit('书名', 0),
        _unit('', 0, grid=label_grid),
    ]) == ['']
    assert module.form_captions([
        _unit('项目全称一览\n书名', 0),
        _unit('', 0, grid=label_grid),
    ]) == ['项目全称一览']
    assert module.form_title(
        _grid(2, [_cell(0, 0, 2, '合并表头'), _cell(1, 0, 1, '名称')]),
        '不应采用这一行') == '合并表头'
    # The longer title shares the stub's lettered token, or the same letters
    # with different spaces, and may live on the heading or in the grid.
    token_heading = [
        _unit('附件 3Z', 0, heading='分册 > 3Z 资质一览'),
        _unit('', 0, grid=_grid(2, [_cell(0, 0, 1, '名称'), _cell(0, 1, 1, '')])),
    ]
    assert module.form_captions(token_heading) == ['3Z 资质一览']
    packed = [
        _unit('附件 3Z', 0, heading='分册'),
        _unit('', 0, grid=_grid(2, [
            _cell(0, 0, 1, '附件3Z资质一览'), _cell(0, 1, 1, '')])),
    ]
    assert module.form_captions(packed) == ['附件3Z资质一览']
    split = [
        _unit('附件 3Z', 0),
        _unit('', 0, grid=_grid(2, [
            _cell(0, 0, 1, '附件 3Z'), _cell(0, 1, 1, '资质一览')])),
    ]
    assert module.form_captions(split) == ['附件 3Z 资质一览']
    assert module.caption_line('表 2 A', headings=['第 2 节 总则']) == '表 2 A'
    # A short non-index line contained in a longer heading is a banner. Another
    # usable line wins. Alone, and with no full-width caption cell, it is empty.
    # A full-width caption cell stays even when the heading contains it.
    piece_heading = '卷一 书名格式补充说明文字'
    assert len(piece_heading) > module._LABEL_CHARS
    assert '书名' in piece_heading
    signature = _grid(3, [
        _cell(0, 0, 1, '甲'), _cell(0, 1, 1, ''), _cell(0, 2, 1, ''),
        _cell(1, 0, 1, '乙'), _cell(1, 1, 1, ''), _cell(1, 2, 1, ''),
        _cell(2, 0, 1, '丙'), _cell(2, 1, 1, ''), _cell(2, 2, 1, ''),
    ])
    assert module.form_captions([
        _unit('项目全称一览\n书名', 0, heading=piece_heading),
        _unit('', 0, grid=signature),
    ]) == ['项目全称一览']
    assert module.form_captions([
        _unit('书名', 0, heading=piece_heading),
        _unit('', 0, grid=signature),
    ]) == ['']
    assert module.caption_line('项目全称一览\n书名', headings=[piece_heading]) == '项目全称一览'
    span_heading = '卷一 合并表头补充说明文字'
    assert len(span_heading) > module._LABEL_CHARS
    assert module.form_captions([
        _unit('不应采用这一行', 0, heading=span_heading),
        _unit('', 0, grid=_grid(2, [
            _cell(0, 0, 2, '合并表头'), _cell(1, 0, 1, '名称')])),
    ]) == ['合并表头']
    # A thin stub's next line can fail only because of a trailing sentence mark,
    # including a space before that mark. Strip it and take that line when the
    # prefix and the lettered token do not match. A fragment after the strip,
    # or a non-furniture line in between, keeps the stub. The sentence before
    # the stub does not count.
    prose = '供方提交有效等级证书及相关证明文件'
    prose_grid = _grid(2, [_cell(0, 0, 1, '名称'), _cell(0, 1, 1, '')])
    assert module.form_captions([
        _unit(f'附件 3Z\n{prose} 。', 0, heading='分册'),
        _unit('', 0, grid=prose_grid),
    ]) == [prose]
    assert module.caption_line(f'附件 3Z\n{prose} 。') == prose
    assert module.caption_line('附件 3Z\n见 3Z 资质一览。') == '见 3Z 资质一览'
    assert module.caption_line(
        '附件 3Z\n供方提交有效等级证书，及相关证明文件。') == '附件 3Z'
    assert module.caption_line(
        f'附件 3Z\n这是分句，还没写完\n{prose}。') == '附件 3Z'
    assert module.caption_line(f'{prose}。\n附件 3Z') == '附件 3Z'
    assert module.caption_line(
        f'附件 3Z\n书名\n{prose}。', running={'书名'}) == prose
