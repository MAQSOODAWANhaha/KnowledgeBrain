"""The model selects bid-format chapters. The host copies the tender lines."""

from typing import Annotated

from pydantic import Field, ValidationError

from docreader.llm.request import ModelContractError
from docreader.tender.directory_compile import bid_format_section
from docreader.tender.directory_sources import SourceRefs
from docreader.tender.schema import Contract, NonnegativeInt, Text
from docreader.tender.status import AuthoringTask


BID_FORMAT_OUTLINE_CONTRACT = 'bid_format_source_outline_v1'
BID_FORMAT_OPERATION = 'select_bid_format_outline_v1'
HeadingRefs = Annotated[list[NonnegativeInt], Field(min_length=1, max_length=4)]
BodyRange = Annotated[list[NonnegativeInt], Field(min_length=2, max_length=2)]


class BidFormatChapter(Contract):
    heading_refs: HeadingRefs
    name: Text
    body: BodyRange | None = None


class BidFormatOutline(Contract):
    chapters: list[BidFormatChapter] = Field(default_factory=list)


def _issue(detail, path=()):
    return {'path': list(path), 'code': 'bid_format_outline_invalid', 'detail': detail}


def _compact(text):
    return ''.join(str(text).split())


def _annex_code(name):
    compact = _compact(name)
    return compact.startswith('附件') or compact.startswith('附录') or compact.startswith('附表')


class _Prepared:
    def __init__(self, documents, identity):
        found = bid_format_section(documents)
        self.found = found is not None
        self.refs = SourceRefs(documents, identity)
        self.visible = set()
        self.payload = None
        if found is None:
            return
        anchor, section = found
        points = [(anchor[0], anchor[1], anchor[2])]
        points.extend((row[0], row[1], row[2]) for row in section)
        self.visible = {self.refs.by_point[point] for point in points}
        self.order = [self.refs.by_point[point] for point in points]
        shown = _heading_candidates(section, self.refs)
        self.payload = {'operation': BID_FORMAT_OPERATION, 'source_ref_format': 'host_bound_line_refs_v1',
                        'lines': shown, 'section_refs': [self.order[0], self.order[-1]]}


def _heading_candidates(section, refs):
    """Short title lines and the lines that follow a bare annex code. Not the form body."""
    shown, pending = [], 0
    for row in section:
        text = row[4]
        if not text:
            continue
        ref = refs.by_point[(row[0], row[1], row[2])]
        if pending:
            shown.append([ref, row[3]])
            pending -= 1
            continue
        if _bare_annex(text):
            shown.append([ref, row[3]])
            pending = 2
            continue
        if len(text) <= 48 or text.startswith('附件') or text[:2] in '一二三四五六七八九十':
            shown.append([ref, row[3]])
    return shown


def _bare_annex(text):
    from docreader.tender.directory_compile import _ANNEX_ONLY
    return _ANNEX_ONLY.match(text) is not None


def decode_bid_format_outline(raw, prepared):
    """Accept only source fragments. Reject a name the tender line does not contain."""
    if not prepared.found:
        raise ModelContractError([_issue('the tender has no bid-format section')])
    try:
        plan = BidFormatOutline.model_validate(raw).model_dump()
    except ValidationError as error:
        raise ModelContractError([{'path': list(item['loc']), 'code': item['type'], 'detail': item['msg']}
                                  for item in error.errors(include_input=False)]) from None
    if not plan['chapters']:
        raise ModelContractError([_issue('select the bid forms named in the tender')])
    chapters = []
    used, seen_heads = set(), set()
    for index, chapter in enumerate(plan['chapters']):
        path = ['chapters', index]
        heads = chapter['heading_refs']
        if heads != sorted(heads) or any(reference in seen_heads or reference not in prepared.visible for reference in heads):
            raise ModelContractError([_issue('heading refs must be unused section lines in order', [*path, 'heading_refs'])])
        if any(heads[offset] + 1 != heads[offset + 1] for offset in range(len(heads) - 1)):
            raise ModelContractError([_issue('a wrapped title must be consecutive source lines', [*path, 'heading_refs'])])
        seen_heads.update(heads)
        text = ''.join(prepared.refs.text(reference) for reference in heads)
        name = chapter['name'].strip()
        if _annex_code(name) or _compact(name) not in _compact(text):
            raise ModelContractError([_issue('章节名称必须是标题原行里的文件名，不能是附件编号或新写的名称', [*path, 'name'])])
        if chapter['body'] is None:
            chapters.append((heads, name, None, None))
            continue
        start, end = chapter['body']
        if end < start or any(reference not in prepared.visible for reference in range(start, end + 1)):
            raise ModelContractError([_issue('body must stay inside the bid-format section', [*path, 'body'])])
        if not set(heads) <= set(range(start, end + 1)):
            raise ModelContractError([_issue('the file name line must sit inside its template', [*path, 'body'])])
        span = set(range(start, end + 1))
        if span & used:
            raise ModelContractError([_issue('each tender line belongs to one form', [*path, 'body'])])
        used.update(span)
        chapters.append((heads, name, start, end))
    chapters.sort(key=lambda item: item[0][0])
    if any(item[2] is None for item in chapters):
        if any(item[2] is not None for item in chapters):
            raise ModelContractError([_issue('either every chapter names its body range, or none do')])
        for index, (heads, name, _start, _end) in enumerate(chapters):
            start = heads[0]
            end = chapters[index + 1][0][0] - 1 if index + 1 < len(chapters) else prepared.order[-1]
            if end < start:
                raise ModelContractError([_issue('chapter order leaves an empty template', ['chapters', index])])
            chapters[index] = (heads, name, start, end)
    nodes, entries, adopted = [], [], []
    for number, (heads, name, start, end) in enumerate(chapters, start=1):
        heading = prepared.refs.point(heads[0])
        identity = f'format-{number}'
        nodes.append({'id': identity, 'heading': heading, 'parent_id': None, 'response_kind': 'form', 'name': name,
                      'body_windows': prepared.refs.windows(start, end, visible=prepared.visible)})
        entries.append({'name': heading})
        adopted.append({'node_id': identity, 'entry': number - 1, 'depth': 'self',
                        'presentation': 'section', 'clause': []})
    first = nodes[0]['heading']
    return {'nodes': nodes,
            'composition': {'priority_basis': [{'document_id': first['document_id'], 'page': first['page'],
                                                 'start_line': first['line'], 'end_line': first['line']}],
                            'entries': entries},
            'scopes': [{'items': adopted}], 'excluded': [], 'source_only': []}


async def select_bid_format_plan(documents, budget, execute, identity):
    from docreader.tender.authoring_pipeline import _ask

    prepared = _Prepared(documents, identity)
    if prepared.payload is None:
        return None

    def accept(raw):
        return decode_bid_format_outline(raw, prepared)

    return await _ask(execute, budget, AuthoringTask.DIRECTORY_PLAN, prepared.payload, accept)
