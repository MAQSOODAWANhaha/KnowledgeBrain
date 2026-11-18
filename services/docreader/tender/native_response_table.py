"""Expand only an explicitly bound native response form from immutable source text."""

from copy import deepcopy

from docreader.tender.authoring_excerpts import response_row_id, source_excerpts_text
from docreader.tender.authoring_templates import load_template_part
from docreader.tender.schema import ResponseTable


class NativeResponseTableRejected(ValueError):
    pass


def prepare_native_response_table(artifacts, target, parts, *, repository):
    from docreader.tender.directory_compile import _native_header_count

    table = ResponseTable.model_validate(target['response_table']).model_dump()
    layout = table.get('native_layout')
    if layout is None or target.get('kind') != 'form':
        raise NativeResponseTableRejected('展开布局必须绑定当前填写原式。')
    matches = [part for part in parts if part['source_ref'] == layout['source_ref']]
    if len(matches) != 1:
        raise NativeResponseTableRejected('展开布局未能唯一绑定本响应位置已采用的原表。')
    part = matches[0]
    loaded = load_template_part(artifacts, part, repository=repository)
    grid = loaded['unit'].get('grid')
    if not grid:
        raise NativeResponseTableRejected('本响应位置没有可展开的原生网格。')
    start, end = layout['repeat_start_row'], layout['repeat_end_row']
    if not 0 <= start <= end < grid['row_count']:
        raise NativeResponseTableRejected('重复填写区域超出原表。')
    header_count = part.get('header_row_count')
    if header_count is None:
        header_count = _native_header_count(loaded['unit']) or 0
    if header_count > start:
        raise NativeResponseTableRejected('重复填写区域与已绑定的标题行重叠。')
    columns = [layout[key] for key in ('sequence_column', 'source_column', 'response_column', 'proof_column')]
    if len(set(columns)) != 4 or any(column >= grid['column_count'] for column in columns):
        raise NativeResponseTableRejected('序号、条款、应答和证明位置的列职责重叠或越界。')
    if any(cell['row'] <= end and cell['row'] + cell['row_span'] > start
           and (cell['row_span'] != 1 or cell['col_span'] != 1) for cell in grid['cells']):
        raise NativeResponseTableRejected('重复填写区域包含合并单元格，不能按单行原型自动展开。')
    cells = {(cell['row'], cell['column']): cell for cell in grid['cells']}
    if any((row, column) not in cells for row in range(start, end + 1) for column in range(grid['column_count'])):
        raise NativeResponseTableRejected('重复填写区域没有完整的原生单元格锚点。')
    sequence = []
    for row in range(start, end + 1):
        label = cells[(row, layout['sequence_column'])]['text'].strip()
        try:
            number = int(label)
        except ValueError:
            raise NativeResponseTableRejected('原序号不是已核定的连续整数，保留原表待核。') from None
        if label != str(number) or (sequence and number != sequence[-1] + 1):
            raise NativeResponseTableRejected('原序号不是已核定的连续整数，保留原表待核。')
        sequence.append(number)
        if any(cells[(row, column)]['text'].strip() for column in range(grid['column_count'])
               if column != layout['sequence_column']):
            raise NativeResponseTableRejected('重复填写区域含有原固定文字，不能覆盖或复制为新条款。')
    texts, row_ids = [], set()
    for row in table['rows']:
        expected = response_row_id(target['target_id'], row['source_excerpts'], row['rule_refs'],
            context_source_excerpts=row.get('context_source_excerpts', []),
            source_join=row.get('source_join', 'paragraphs'), native_layout=layout)
        if row['row_id'] != expected or row['row_id'] in row_ids:
            raise NativeResponseTableRejected('条款行身份与当前来源、上下文、续接或原表布局不一致。')
        row_ids.add(row['row_id'])
        context = source_excerpts_text(artifacts, row.get('context_source_excerpts', []), repository=repository)
        body = source_excerpts_text(artifacts, row['source_excerpts'],
                                   source_join=row.get('source_join', 'paragraphs'), repository=repository)
        if not body.strip():
            raise NativeResponseTableRejected('条款行没有可读取的来源正文。')
        texts.append('\n'.join(value for value in (context, body) if value))
    extra = max(0, len(texts) - (end - start + 1))
    expanded = deepcopy(grid)
    expanded['row_count'] += extra
    for cell in expanded['cells']:
        if cell['row'] > end:
            cell['row'] += extra
    for offset in range(extra):
        for column in range(grid['column_count']):
            copied = deepcopy(cells[(end, column)])
            copied['row'] = end + offset + 1
            if column == layout['sequence_column']:
                copied['text'] = str(sequence[-1] + offset + 1)
            expanded['cells'].append(copied)
    expanded['cells'].sort(key=lambda cell: (cell['row'], cell['column']))
    output_cells = {(cell['row'], cell['column']): cell for cell in expanded['cells']}
    for offset, text in enumerate(texts):
        output_cells[(start + offset, layout['source_column'])]['text'] = text
    context = source_excerpts_text(artifacts, table.get('context_source_excerpts', []), repository=repository)
    return {'part': part, 'loaded': loaded, 'grid': expanded, 'context': context,
            'row_origins': [*range(end + 1), *([end] * extra), *range(end + 1, grid['row_count'])],
            'row_positions': {row['row_id']: start + offset for offset, row in enumerate(table['rows'])},
            'response_column': layout['response_column']}
