"""Compile a bid directory from locations in the registered tender documents.

The model may point at a document line and record adoption decisions. It does not
supply chapter titles, template text, sibling order, or a parent that the same
document's own contents can correct. A later amendment or clarification changes
the composition only when the plan cites a priority basis inside that document.
This compiler does not prove that two agreeing plans are complete.
"""

from enum import StrEnum
from copy import deepcopy
import hashlib
import json
import re
from re import compile as _compile
from typing import Annotated, cast

from pydantic import BeforeValidator, Field

from docreader.tender.schema import Contract, Identity, NonnegativeInt, ResponseTargetKind, Text
from docreader.tender.status import DocumentRole


PARAMETER_SOURCE_CONTRACT = 'source_bound_parameter_tables_v1'


class DirectoryPlanRejected(ValueError):
    def __init__(self, detail):
        super().__init__(detail)
        self.detail = detail


_ENUM = _compile(r'^\s*(?:第?[0-9]+|[一二三四五六七八九十]+)[、.．)\]]\s*|^\s*[（(][0-9一二三四五六七八九十]+[)）]\s*')
_CJK_GAP = _compile(r'(?<=[\u4e00-\u9fff])\s+(?=[\u4e00-\u9fff])')
_SPACE = _compile(r'\s+')
_LEADERS = frozenset('.\u2026\u00b7')
class DirectoryDepth(StrEnum):
    SELF = "self"
    CHILDREN = "children"
    SUBTREE = "subtree"


class DirectoryPresentation(StrEnum):
    SECTION = "section"
    CONTENT = "content"
    COVER = "cover"


class RequirementScope(StrEnum):
    CHAPTER = "chapter"
    CHAPTERS = "chapters"
    DOCUMENT = "document"
    UNASSIGNED = "unassigned"


def _wire(enum):
    def parse(value):
        if isinstance(value, enum):
            return value
        if isinstance(value, str):
            return enum(value)
        raise ValueError(f'{enum.__name__} expects a string')
    return BeforeValidator(parse)


WireDepth = Annotated[DirectoryDepth, _wire(DirectoryDepth)]
WirePresentation = Annotated[DirectoryPresentation, _wire(DirectoryPresentation)]
WireScope = Annotated[RequirementScope, _wire(RequirementScope)]


_ROLES = frozenset(DocumentRole)
_REVISIONS = frozenset((DocumentRole.AMENDMENT, DocumentRole.CLARIFICATION))
_ROLE_LABEL = {DocumentRole.AMENDMENT: '补遗', DocumentRole.CLARIFICATION: '答疑'}
_POINT = frozenset(('document_id', 'page', 'line'))
_WINDOW = frozenset(('document_id', 'page', 'start_line', 'end_line'))
_PLAN = frozenset(('nodes', 'composition', 'scopes', 'excluded', 'source_only', 'format_bindings', 'reading_only_sources'))
_BINDING = frozenset(('id', 'source', 'response_node_id', 'reference_node_ids'))
_NODE = frozenset(('id', 'heading', 'body_windows', 'response_kind', 'subjects', 'content_start', 'parent_id', 'name',
                   'heading_lines'))
_SUBJECT = frozenset(('source', 'role'))
_COMPOSITION = frozenset(('priority_basis', 'entries'))
_ENTRY = frozenset(('name', 'excluded_by', 'heading_lines'))
_SCOPE = frozenset(('title', 'items'))
_ITEM = frozenset(('node_id', 'entry', 'depth', 'presentation', 'condition', 'clause'))
_EXCLUSION = frozenset(('node_id', 'source'))
_DEPTHS = frozenset(DirectoryDepth)
_PRESENTATIONS = frozenset(DirectoryPresentation)


class DirectoryPoint(Contract):
    document_id: Identity
    page: NonnegativeInt
    line: NonnegativeInt


class DirectoryWindow(Contract):
    document_id: Identity
    page: NonnegativeInt
    start_line: NonnegativeInt
    end_line: NonnegativeInt


class DirectorySubject(Contract):
    source: DirectoryWindow
    role: Text


class DirectoryNode(Contract):
    id: Identity
    heading: DirectoryPoint
    body_windows: list[DirectoryWindow]
    response_kind: ResponseTargetKind
    subjects: list[DirectorySubject] = Field(default_factory=list)
    content_start: DirectoryPoint | None = None
    parent_id: Identity | None = None
    name: Text | None = None


class DirectoryEntry(Contract):
    name: DirectoryPoint
    excluded_by: list[DirectoryWindow] = Field(default_factory=list)


class DirectoryComposition(Contract):
    priority_basis: list[DirectoryWindow] = Field(default_factory=list)
    entries: list[DirectoryEntry] = Field(default_factory=list)


class DirectoryItem(Contract):
    node_id: Identity | None = None
    entry: NonnegativeInt | None = None
    depth: WireDepth
    presentation: WirePresentation
    condition: DirectoryWindow | None = None
    clause: list[DirectoryWindow] = Field(default_factory=list)


class DirectoryScope(Contract):
    title: DirectoryPoint | None = None
    items: list[DirectoryItem] = Field(default_factory=list)


class DirectoryExclusion(Contract):
    node_id: Identity
    source: DirectoryWindow


class FormatBinding(Contract):
    id: Identity
    source: DirectoryWindow
    response_node_id: Identity
    reference_node_ids: list[Identity] = Field(default_factory=list)


class DirectoryPlan(Contract):
    nodes: list[DirectoryNode] = Field(default_factory=list)
    composition: DirectoryComposition = Field(default_factory=DirectoryComposition)
    scopes: list[DirectoryScope] = Field(default_factory=list)
    excluded: list[DirectoryExclusion] = Field(default_factory=list)
    source_only: list[Identity] = Field(default_factory=list)
    format_bindings: list[FormatBinding] = Field(default_factory=list)


class ReadingDirectoryPlan(DirectoryPlan):
    reading_only_sources: list[DirectoryPoint] = Field(default_factory=list)


class RequirementAssignment(Contract):
    document_id: Identity
    page: NonnegativeInt
    start_line: NonnegativeInt
    end_line: NonnegativeInt
    continuation_windows: list[DirectoryWindow] = Field(default_factory=list)
    scope: WireScope
    chapter_ids: list[Identity] = Field(default_factory=list)


class RequirementPlan(Contract):
    assignments: list[RequirementAssignment] = Field(default_factory=list)


class RequirementAdjudicationScope(StrEnum):
    CHAPTER = "chapter"
    CHAPTERS = "chapters"
    DOCUMENT = "document"
    PROCEDURE = "procedure"
    NOT_APPLICABLE = "not_applicable"
    UNASSIGNED = "unassigned"


class RequirementAdjudication(Contract):
    item_ids: list[Identity]
    scope: Annotated[RequirementAdjudicationScope, _wire(RequirementAdjudicationScope)]
    chapter_ids: list[Identity] = Field(default_factory=list)
    basis_windows: list[DirectoryWindow] = Field(default_factory=list)


class MaterialAssignment(Contract):
    chapter_id: Identity
    document_id: Identity
    page: NonnegativeInt
    start_line: NonnegativeInt
    end_line: NonnegativeInt


class TableHeaderDecision(Contract):
    table_id: Identity
    count: NonnegativeInt | None


class DirectoryAdjudicationAction(StrEnum):
    CURRENT = "current"
    ALTERNATIVE = "alternative"
    SUBJECTS_ALTERNATIVE = "subjects_alternative"
    REFERENCE = "reference"
    NOT_APPLICABLE_REFERENCE = "not_applicable_reference"
    CONTAINER = "container"
    CONDITIONAL = "conditional"
    UNRESOLVED = "unresolved"


class DirectoryAdjudication(Contract):
    case_id: Identity
    action: Annotated[DirectoryAdjudicationAction, _wire(DirectoryAdjudicationAction)]
    basis_windows: list[DirectoryWindow] = Field(default_factory=list)
    chapter_ids: list[Identity] = Field(default_factory=list)


class DirectoryChapterTitle(Contract):
    title_id: Identity
    title: Text = Field(max_length=40)


class ResponseFormSelection(Contract):
    table_id: Identity
    mandate_windows: list[DirectoryWindow] = Field(min_length=1)
    source_windows: list[DirectoryWindow] = Field(min_length=1)


class MaterialPlan(Contract):
    assignments: list[MaterialAssignment] = Field(default_factory=list)
    table_headers: list[TableHeaderDecision] = Field(default_factory=list)
    directory_adjudications: list[DirectoryAdjudication] = Field(default_factory=list)
    chapter_titles: list[DirectoryChapterTitle] = Field(default_factory=list)


class NativeMaterialPlan(MaterialPlan):
    response_forms: list[ResponseFormSelection] = Field(default_factory=list)


class WorkKind(StrEnum):
    GOODS = "goods"
    SERVICE = "service"
    WORKS = "works"
    UNRESOLVED = "unresolved"


WireKind = Annotated[WorkKind, _wire(WorkKind)]


class ClassificationItem(Contract):
    kind: WireKind
    document_id: Identity
    page: NonnegativeInt
    start_line: NonnegativeInt
    end_line: NonnegativeInt


class ClassificationPlan(Contract):
    items: list[ClassificationItem] = Field(default_factory=list)
    requirement_adjudications: list[RequirementAdjudication] = Field(default_factory=list)


class ParameterItem(Contract):
    document_id: Identity
    page: NonnegativeInt
    start_line: NonnegativeInt
    end_line: NonnegativeInt
    content_start_line: NonnegativeInt
    continuation_windows: list[DirectoryWindow] = Field(default_factory=list)
    title: Text | None = Field(default=None, max_length=40)


class ParameterChapter(Contract):
    chapter_id: Identity
    items: list[ParameterItem] = Field(default_factory=list)


class ParameterPlan(Contract):
    chapters: list[ParameterChapter] = Field(default_factory=list)


class _FrozenLayout(dict):
    """Keep the host proof outside the serialized directory reading contract."""

    def bind_layout(self, raw_sha256):
        self.layout_proof = raw_sha256, self._layout_digest()

    def has_layout_proof(self, raw_sha256):
        return getattr(self, 'layout_proof', None) == (raw_sha256, self._layout_digest())

    def _layout_digest(self):
        encoded = json.dumps([self.get(key) for key in ('document_id', 'pages', 'line_units')],
                             ensure_ascii=False, separators=(',', ':'))
        return hashlib.sha256(encoded.encode()).hexdigest()


def documents_from_frozen(artifacts, frozen):
    """Project registered documents in their stored order. One file stays one file."""
    from docreader.tender.evidence.references import EvidenceRepository

    documents = []
    repository = EvidenceRepository(artifacts)
    for record in frozen.get('documents', []):
        index = repository.source_index(record['source_index_ref'])
        pages, line_units, ocr_issues, unread_pages = _reading_pages(artifacts, index.raw)
        document = {'document_id': record['document_id'], 'role': record['role'], 'pages': pages,
                    'line_units': line_units, 'ocr_issues': ocr_issues, 'unread_pages': unread_pages,
                    'template_parts': _grid_parts(index, record['source_index_ref']), 'grids': _grid_catalog(index),
                    'layout_decoration': _layout_decoration(pages),
                    'continuations': _document_continuations(index.raw, pages, unread_pages),
                    'body_projection': list(index.raw.get('body_projection') or [])}
        if index.raw.get('literal_source_ref') and not any(
                issue['code'] == 'outline_directory_layout_rejected' for issue in ocr_issues):
            document = _FrozenLayout(document)
            document.bind_layout(record['raw_ref']['sha256'])
        documents.append(document)
        source_reviews = _frozen_source_reviews(record, index.raw)
        if source_reviews:
            documents[-1]['source_review_issues'] = source_reviews
    if not documents:
        raise DirectoryPlanRejected('directory source requires the registered documents')
    return documents


def _frozen_source_reviews(record, raw):
    name = record.get('file_name') or record['document_id']
    reasons = {'applicability_pending': '已读取，适用关系待核',
               'unsupported': '格式暂不支持读取', 'unreadable': '内容无法读取'}
    issues = [{'code': 'outline_directory_attachment_unconfirmed',
               'detail': f'{name} 内嵌附件 {unit_id}（{" / ".join(entry["names"])}）{reasons[entry["status"]]}'}
              for unit_id, entry in (raw.get('embedded_sources') or {}).items()]
    if not raw.get('literal_source_ref'):
        return issues
    images = {}
    for unit in raw['units']:
        if unit.get('kind') != 'image_region':
            continue
        unit_id, locator = unit['unit_id'], unit['locator']
        ocr = raw.get('ocr', {}).get(unit_id, {})
        if ocr.get('status') == 'complete' and not ocr.get('issues'):
            continue
        ref = raw.get('images', {}).get(locator.get('image_key'), {})
        identity = ref.get('sha256') or unit_id
        images.setdefault(identity, []).append(locator.get('page_ordinal'))
    for group in group_unread_images(raw):
        pages = '、'.join(str(page + 1) for page in group['pages'])
        place = f'第{pages}页' if pages else '无页码定位'
        issues.append({'code': 'outline_directory_image_unconfirmed',
                       'detail': f'{name} {place}，图片 {group["identity"][:16]} 共 {group["count"]} 处，内容或用途待核。'})
    return issues


def group_unread_images(raw):
    """One pending item per image hash, listing every unread page."""
    images = {}
    for unit in raw.get('units') or []:
        if unit.get('kind') != 'image_region':
            continue
        unit_id, locator = unit['unit_id'], unit.get('locator') or {}
        ocr = (raw.get('ocr') or {}).get(unit_id, {})
        if ocr.get('status') == 'complete' and not ocr.get('issues'):
            continue
        ref = (raw.get('images') or {}).get(locator.get('image_key'), {})
        identity = ref.get('sha256') or unit_id
        images.setdefault(identity, []).append(locator.get('page_ordinal'))
    groups = []
    for identity, positions in images.items():
        pages = sorted({page for page in positions if isinstance(page, int)})
        groups.append({'identity': identity, 'pages': pages, 'count': len(positions)})
    return groups


def reading_payload(documents, identity=None):
    """Numbered lines for one directory plan. The model returns locations, not this text."""
    _Source(documents)
    view = []
    for document in documents:
        view.append({'document_id': document['document_id'], 'role': document['role'], 'pages': [
            {'page': number, 'lines': [{'line': line, 'text': text} for line, text in enumerate(page.splitlines())]}
            for number, page in enumerate(document['pages'])]})
    payload = {'documents': view}
    if identity:
        payload['source_version'] = identity.get('source_version')
        payload['domain_revision'] = identity.get('domain_revision')
    return payload


def _unit_lines(unit, recognized: str | None = ''):
    """Grid text is cleared on native units, so rows are read from the canonical cells."""
    text = unit.get('text') or ''
    if not text.strip():
        text = recognized or ''
    if text.strip():
        return text.splitlines()
    rows = {}
    for cell in (unit.get('grid') or {}).get('cells') or []:
        rows.setdefault(cell.get('row'), []).append(cell)
    lines = []
    for row in sorted(row for row in rows if row is not None):
        ordered = sorted(rows[row], key=lambda cell: cell.get('column') or 0)
        lines.append('\t'.join(cell.get('text') or '' for cell in ordered))
    return lines


_USABLE_TEXT = _compile(r'[\u4e00-\u9fffA-Za-z]')


def _reading_pages(artifacts, raw):
    """PDF directory text is the physical page in visual order, indent included."""
    layout, problem = _layout_pages(artifacts, raw)
    if layout is not None:
        pages, issues, unread = _ocr_unreadable_pages(artifacts, raw, layout)
        pdf_bytes = _pdf_bytes(artifacts, raw)
        owners = _bind_grid_lines(pages, raw.get('units') or [], pdf_bytes)
        # Request lines stay on the layout text. Symbol and literal presentation
        # are body projections, so a saved directory receipt still matches.
        projection, body_issues = _body_projections(pages, raw, _layout_word_pages(pdf_bytes))
        raw['body_projection'] = projection
        return pages, owners, body_issues + issues, unread
    texts, issues, unread = _image_texts(artifacts, raw)
    attachments = raw.get('embedded_sources') or {}
    units = [unit for unit in raw.get('units') or [] if unit['unit_id'] not in attachments]
    pages, owners = _project_units(units, texts)
    if problem:
        issues.insert(0, {'code': 'outline_directory_layout_rejected', 'detail': problem})
    return pages, owners, issues, unread


def _layout_word_pages(pdf_bytes):
    """Poppler word boxes in top-left coordinates, one entry per page."""
    if not pdf_bytes:
        return None
    from pathlib import Path
    from subprocess import run
    from tempfile import TemporaryDirectory

    with TemporaryDirectory() as directory:
        path = Path(directory) / 'source.pdf'
        path.write_bytes(pdf_bytes)
        extracted = run(['pdftotext', '-bbox', '-enc', 'UTF-8', str(path), '-'], capture_output=True, check=False)
    if extracted.returncode != 0:
        return None
    xhtml = extracted.stdout.decode('utf-8', errors='replace')
    pages = []
    for part in xhtml.split('<page ')[1:]:
        tag, _, rest = part.partition('>')
        height = _attribute(tag, 'height')
        if height is None:
            return None
        pages.append((_bbox_word_boxes(rest.split('</page>')[0]), float(height)))
    return pages or None


def _visual_word_lines(word_boxes):
    """Group Poppler words into top-to-bottom lines. Whitespace inside a word is already gone."""
    pending = []
    for word in word_boxes or []:
        if not isinstance(word, tuple) or len(word) < 5 or not word[4]:
            continue
        left, top, right, bottom = word[:4]
        if not all(isinstance(value, (int, float)) for value in (left, top, right, bottom)):
            continue
        if right < left or bottom < top:
            continue
        pending.append((float(left), float(top), float(right), float(bottom), word[4]))
    pending.sort(key=lambda word: ((word[1] + word[3]) / 2, word[0]))
    rows = []
    for word in pending:
        center = (word[1] + word[3]) / 2
        if rows and abs(rows[-1][0] - center) <= _SAME_LINE:
            rows[-1][1].append(word)
        else:
            rows.append([center, [word]])
    return [sorted(words, key=lambda word: word[0]) for _center, words in rows]


def _word_match_key(text):
    """Poppler drops a literal U+FFFE, so it cannot separate two otherwise identical lines."""
    return ''.join(char for char in _compact(text) if char != '\ufffe')


def _align_word_spans(line, words):
    """Map compacted words onto the original layout line, keeping its spaces and marks."""
    def skip(cursor, marks):
        while cursor < len(line) and (line[cursor].isspace() or (marks and line[cursor] == '\ufffe')):
            cursor += 1
        return cursor

    cursor, spans = 0, []
    for word in words:
        if not isinstance(word, str) or not word:
            return None
        if word == '\ufffe':
            cursor = skip(cursor, False)
            if cursor >= len(line) or line[cursor] != '\ufffe':
                return None
            spans.append((cursor, cursor + 1))
            cursor += 1
            continue
        cursor = skip(cursor, True)
        if line[cursor:cursor + len(word)] != word:
            return None
        spans.append((cursor, cursor + len(word)))
        cursor += len(word)
    if any(not char.isspace() and char != '\ufffe' for char in line[cursor:]):
        return None
    return spans


def _pair_layout_words(lines, visual_lines):
    from collections import defaultdict

    layout_keys, visual_keys = defaultdict(list), defaultdict(list)
    for index, line in enumerate(lines):
        key = _word_match_key(line)
        if key:
            layout_keys[key].append(index)
    for index, words in enumerate(visual_lines):
        visual_keys[_word_match_key(''.join(word[4] for word in words))].append(index)
    pairs = {}
    for key, layout_ids in layout_keys.items():
        visual_ids = visual_keys.get(key, [])
        if len(layout_ids) != len(visual_ids):
            continue
        for layout_id, visual_id in zip(layout_ids, visual_ids):
            spans = _align_word_spans(lines[layout_id], [word[4] for word in visual_lines[visual_id]])
            if spans is None:
                continue
            pairs[layout_id] = list(zip(visual_lines[visual_id], spans))
    return pairs


def _glyph_poppler_box(glyph, page_height):
    if not isinstance(glyph, dict):
        return None
    if any(not isinstance(glyph.get(key), (int, float)) for key in ('left', 'bottom', 'right', 'top')):
        return None
    left, bottom, right, top = (float(glyph[key]) for key in ('left', 'bottom', 'right', 'top'))
    if right < left or top < bottom:
        return None
    return left, page_height - top, right, page_height - bottom


def _box_contains(word, x_coord, y_coord, slack=2.0):
    left, top, right, bottom, _text = word
    return left - slack <= x_coord <= right + slack and top - slack <= y_coord <= bottom + slack


def _glyph_layout_offset(lines, glyph, pairs, page_height):
    box = _glyph_poppler_box(glyph, page_height)
    raw = glyph.get('raw_symbol') if isinstance(glyph, dict) else None
    if box is None or not isinstance(raw, str) or len(raw) != 1:
        return None
    center_x, center_y = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
    hits = []
    for line_no, items in pairs.items():
        for word, (start, _end) in items:
            if not _box_contains(word, center_x, center_y):
                continue
            indexes = [index for index, char in enumerate(word[4]) if char == raw]
            offset = start + indexes[0] if len(indexes) == 1 else None
            if offset is None or not 0 <= offset < len(lines[line_no]) or lines[line_no][offset] != raw:
                continue
            hits.append((line_no, offset))
    return hits[0] if len(hits) == 1 else None


def bind_symbol_edits(lines, glyphs, word_boxes, page_height, raw=None):
    """Bind a verified glyph to one original layout offset. Stream order is not a position."""
    if not isinstance(page_height, (int, float)) or page_height <= 0:
        return []
    pairs = _pair_layout_words(lines, _visual_word_lines(word_boxes))
    placed, blocked = {}, set()
    for glyph in glyphs or []:
        found = _glyph_layout_offset(lines, glyph, pairs, page_height)
        if found is None:
            continue
        if found in placed or found in blocked:
            placed.pop(found, None)
            blocked.add(found)
            continue
        placed[found] = glyph
    edits = []
    source = raw or {}
    for (line_no, offset), glyph in placed.items():
        char = glyph.get('raw_symbol')
        meaning = _verified_symbol_meaning(glyph, char, source) if isinstance(char, str) else None
        if meaning and meaning != char:
            edits.append((line_no, offset, meaning))
    return sorted(edits)


def _normalize_layout_symbols(pages, raw):
    """Reuse frozen embedded-font proof; a private code point alone has no meaning."""
    from collections import defaultdict
    from docreader.parser.pdf_symbols import is_private_symbol

    proofs = defaultdict(list)
    for item in (raw.get('source_contract') or {}).get('glyph_normalizations', []) or []:
        if not isinstance(item, dict):
            continue
        page, char = item.get('page_ordinal'), item.get('raw_symbol')
        if type(page) is int and 0 <= page < len(pages) and isinstance(char, str) and is_private_symbol(char):
            proofs[page, char].append(item)
    result = list(pages)
    for (page, char), items in proofs.items():
        indices = [item.get('char_index') for item in items]
        if (any(type(index) is not int or index < 0 for index in indices)
                or len(set(indices)) != len(items) or pages[page].count(char) != len(items)):
            continue
        items = sorted(items, key=lambda item: item.get('char_index') if type(item.get('char_index')) is int else 10**9)
        if len(items) != pages[page].count(char):
            continue
        replacements = {}
        for occurrence, item in enumerate(items):
            mapped = _verified_symbol_meaning(item, char, raw)
            if mapped:
                replacements[occurrence] = mapped
        if replacements:
            result[page] = _replace_occurrences(result[page], char, replacements)
    issues = [{'code': 'outline_directory_symbol_unconfirmed',
               'detail': f'第{page + 1}页的私用符号未通过来源字体证据核对，原字保留；请对照原件确认符号含义并修正Word显示。'}
              for page, text in enumerate(result) if any(is_private_symbol(char) for char in text)]
    return result, issues


def _literal_character_pieces(raw_text, presentation, stored):
    """One contribution per source code point. A stored list comes from the glyph flags."""
    if (isinstance(stored, list) and len(stored) == len(raw_text)
            and all(isinstance(piece, str) for piece in stored)):
        return stored
    return _align_literal_pieces(raw_text, presentation)


def _align_literal_pieces(raw_text, presentation):
    """Pair each source character with the text it contributes.

    An old receipt has no per-glyph pieces. U+FFFE may contribute a hyphen or nothing.
    A source hyphen stays a hyphen. The match has to consume the whole presentation.
    The search is iterative so a long legal line cannot exhaust the call stack.
    """
    if not isinstance(raw_text, str) or not isinstance(presentation, str):
        return None
    cursors = {0}
    steps = []
    for char in raw_text:
        step = {}
        for cursor in cursors:
            options = []
            if char == '\ufffe':
                if cursor < len(presentation) and presentation[cursor] == '-':
                    options.append((cursor + 1, '-'))
                options.append((cursor, ''))
            elif cursor < len(presentation) and presentation[cursor] == char:
                options.append((cursor + 1, char))
            for new_cursor, piece in options:
                if new_cursor not in step:
                    step[new_cursor] = (cursor, piece)
        if not step:
            return None
        steps.append(step)
        cursors = set(step)
    if len(presentation) not in cursors:
        return None
    pieces = []
    cursor = len(presentation)
    for step in reversed(steps):
        cursor, piece = step[cursor]
        pieces.append(piece)
    pieces.reverse()
    return pieces


def _compose_original_line(original, spans, symbols):
    """Apply literal and symbol edits from the original character offsets, then render once."""
    from docreader.parser.pdf_symbols import is_private_symbol

    pieces = list(original)
    for start, literal_pieces in spans:
        if not isinstance(start, int) or start < 0 or start + len(literal_pieces) > len(original):
            continue
        for offset, piece in enumerate(literal_pieces):
            pieces[start + offset] = piece
    for offset, meaning in symbols.items():
        if isinstance(offset, int) and 0 <= offset < len(original) and is_private_symbol(original[offset]):
            pieces[offset] = meaning
    return ''.join(pieces)


def _body_projections(pages, raw, word_pages=None):
    """One body line: symbols and literal presentation merge. Layout pages stay intact."""
    from docreader.parser.pdf_symbols import is_private_symbol

    issues = []
    geometry = bool(word_pages)
    literal_pieces = {}
    if geometry:
        bodies = [page.splitlines() for page in pages]
    else:
        replaced, symbol_issues = _normalize_layout_symbols(list(pages), raw)
        issues.extend(symbol_issues)
        bodies = []
        for original, shown in zip(pages, replaced):
            before, after = original.splitlines(), shown.splitlines()
            bodies.append(list(after if len(after) == len(before) else before))
    units = {unit.get('unit_id'): unit for unit in raw.get('units') or []}
    grouped = {}
    for unit_id, mapping in (raw.get('literal_units') or {}).items():
        if not isinstance(mapping, dict) or 'presentation' not in mapping:
            continue
        unit = units.get(unit_id) or {}
        page = mapping.get('page_ordinal')
        raw_text = unit.get('text') or ''
        if not isinstance(page, int) or page < 0 or page >= len(pages) or not raw_text:
            continue
        grouped.setdefault((page, raw_text), []).append((unit, mapping))
    for (page, raw_text), pairs in grouped.items():
        lines = pages[page].splitlines()
        exact, partial = [], []
        for index, line in enumerate(lines):
            whole = _whole_line_offset(line, raw_text)
            if whole is not None:
                exact.append((index, whole))
            else:
                found = line.find(raw_text)
                if found >= 0:
                    partial.append((index, found))
        # A whole source line, ignoring layout indent, is claimed before a shorter copy inside a longer line.
        if len(exact) == len(pairs):
            hits = exact
        elif len(exact) < len(pairs) and len(exact) + len(partial) == len(pairs):
            hits = sorted(exact + partial)
        else:
            hits = None
        pairs.sort(key=lambda pair: _literal_position(pair[0], pair[1]))
        if hits is None:
            for line_no, _start in exact + partial:
                issues.append({'code': 'outline_directory_literal_pending',
                               'detail': f'第{page + 1}页第{line_no + 1}行的重复原字无法按来源位置对应，正文保留版式行并待核。'})
            continue
        for (line_no, start), (unit, mapping) in zip(hits, pairs):
            presentation = mapping.get('presentation') or ''
            if geometry:
                pieces = _literal_character_pieces(raw_text, presentation, mapping.get('character_pieces'))
                if pieces is not None:
                    literal_pieces.setdefault((page, line_no), []).append((start, pieces))
            else:
                presentation = _present_verified_symbols(
                    presentation, page, raw, mapping.get('char_indices') or [])
                symbol = bodies[page][line_no]
                bodies[page][line_no] = symbol[:start] + presentation + symbol[start + len(raw_text):]
            if mapping.get('presentation_pending'):
                issues.append({'code': 'outline_directory_literal_pending',
                               'detail': f'第{page + 1}页第{line_no + 1}行含未证实的原字占位，正文已省略该字符，请对照原件核对。'})
    if geometry:
        glyphs = [item for item in (raw.get('source_contract') or {}).get('glyph_normalizations', []) or []
                  if isinstance(item, dict)]
        originals = [page.splitlines() for page in pages]
        for page, lines in enumerate(bodies):
            boxes, height = word_pages[page] if page < len(word_pages) else ([], None)
            source_lines = originals[page] if page < len(originals) else []
            symbols = {}
            for line_no, offset, meaning in bind_symbol_edits(
                    source_lines, [item for item in glyphs if item.get('page_ordinal') == page], boxes, height, raw):
                symbols.setdefault(line_no, {})[offset] = meaning
            for line_no, source in enumerate(source_lines):
                spans = literal_pieces.get((page, line_no), [])
                edits = symbols.get(line_no, {})
                if spans or edits:
                    lines[line_no] = _compose_original_line(source, spans, edits)
        issues.extend({'code': 'outline_directory_symbol_unconfirmed',
                       'detail': f'第{page + 1}页的私用符号未通过来源字体证据核对，原字保留；请对照原件确认符号含义并修正Word显示。'}
                      for page, lines in enumerate(bodies) if any(is_private_symbol(char) for line in lines for char in line))
    projection = []
    for page, (original, shown_lines) in enumerate(zip(pages, bodies)):
        for line, (raw_line, body_line) in enumerate(zip(original.splitlines(), shown_lines)):
            pending = any(issue['code'] == 'outline_directory_literal_pending' and f'第{page + 1}页第{line + 1}行' in issue['detail']
                          for issue in issues)
            if raw_line != body_line or pending:
                projection.append({'page': page, 'line': line, 'text': body_line, 'pending': pending})
    return projection, issues


def _whole_line_offset(line, raw_text):
    """Offset of a whole source line. Extra leading layout spaces are allowed; source spaces stay in the match."""
    if not raw_text or not isinstance(line, str) or not line.endswith(raw_text):
        return None
    prefix = line[:-len(raw_text)]
    if prefix.strip(' ') != '':
        return None
    return len(prefix)


def _literal_position(unit, mapping):
    """Visual order, independent of the frozen object-key sort."""
    locator = (unit or {}).get('physical_locator') or (unit or {}).get('locator') or {}
    top = locator.get('top')
    left = locator.get('left')
    chars = mapping.get('char_indices') or []
    return (
        -(float(top) if isinstance(top, (int, float)) else 0.0),
        float(left) if isinstance(left, (int, float)) else 0.0,
        min(chars) if chars else 0,
    )


def _verified_symbol_meaning(item, char, raw):
    from docreader.parser.pdf_symbols import verified_symbol

    if not isinstance(item.get('font_name'), str) or not isinstance(item.get('glyph_name'), str):
        return None
    font_sha = item.get('font_sha256', '')
    binding = item.get('pdf_glyph_binding')
    revision = (raw.get('source_contract') or {}).get('document_revision')
    if binding is not None and (not isinstance(binding, dict)
                                or revision and binding.get('source_document_sha256') != revision):
        return None
    if not isinstance(font_sha, str) or not _compile(r'[0-9a-f]{64}').fullmatch(font_sha):
        return None
    mapped = verified_symbol(item.get('font_name', ''), ord(char), [item.get('glyph_name')], font_sha,
                             glyph_binding=binding)
    if not mapped or mapped[1] != item.get('normalized_symbol'):
        return None
    return mapped[1]


def _replace_occurrences(text, char, replacements):
    occurrence = 0
    characters = []
    for item in text:
        if item == char:
            characters.append(replacements.get(occurrence, item))
            occurrence += 1
        else:
            characters.append(item)
    return ''.join(characters)


def _present_verified_symbols(text, page, raw, char_indices):
    """Decode only the private glyph whose character index is inside this span."""
    from docreader.parser.pdf_symbols import is_private_symbol

    owned = {index for index in char_indices if type(index) is int}
    evidence = [item for item in (raw.get('source_contract') or {}).get('glyph_normalizations', []) or []
                if isinstance(item, dict) and item.get('page_ordinal') == page and item.get('char_index') in owned]
    shown = text
    for char in sorted({symbol for item in evidence if isinstance(symbol := item.get('raw_symbol'), str)}):
        if not is_private_symbol(char) or char not in shown:
            continue
        items = sorted((item for item in evidence if item.get('raw_symbol') == char),
                       key=lambda item: item['char_index'])
        if len(items) != shown.count(char):
            continue
        replacements = {}
        for occurrence, item in enumerate(items):
            meaning = _verified_symbol_meaning(item, char, raw)
            if meaning:
                replacements[occurrence] = meaning
        shown = _replace_occurrences(shown, char, replacements)
    return shown


def _layout_decoration(pages):
    from docreader.parser.continuation import mark_layout_decoration

    marked = mark_layout_decoration(pages)
    # Decoration is a parallel flag. The layout strings are the directory lines.
    if [len(flags) for flags in marked] != [len(page.splitlines()) for page in pages]:
        return [[] for _ in pages]
    return marked


def _document_continuations(raw, pages, unread_pages):
    from docreader.parser.continuation import mark_layout_decoration, paragraph_continuations

    table_links = list((raw.get('metadata') or {}).get('table_continuations') or [])
    unread = set(unread_pages or [])
    decoration = mark_layout_decoration(pages)
    described = []
    for number, page in enumerate(pages):
        flags = decoration[number] if number < len(decoration) else []
        described.append({'page': number, 'scanned': number in unread, 'lines': [
            {'text': text, 'decoration': bool(flags[index]) if index < len(flags) else False}
            for index, text in enumerate(page.splitlines())]})
    return [*table_links, *paragraph_continuations(described)]


def _layout_pages(artifacts, raw):
    ref = raw.get('original_ref')
    if not isinstance(ref, dict):
        return None, None
    try:
        data = artifacts.read_bytes(ref)
    except (OSError, KeyError, ValueError):
        return None, None
    if not data.startswith(b'%PDF'):
        return None, None
    from pathlib import Path
    from subprocess import run
    from tempfile import TemporaryDirectory

    with TemporaryDirectory() as directory:
        path = Path(directory) / 'source.pdf'
        path.write_bytes(data)
        info = run(['pdfinfo', str(path)], capture_output=True, check=False)
        extracted = run(['pdftotext', '-layout', '-enc', 'UTF-8', str(path), '-'], capture_output=True, check=False)
    if info.returncode != 0 or extracted.returncode != 0:
        return None, '版式文字未能读出，改用解析单元'
    count = next((int(line.split(':', 1)[1]) for line in info.stdout.decode('utf-8', errors='replace').splitlines()
                  if line.startswith('Pages:')), None)
    text = extracted.stdout.decode('utf-8', errors='replace')
    if not text.endswith('\f'):
        text += '\f'
    pages = text[:-1].split('\f')
    if count is None or len(pages) != count:
        return None, '版式页数与原件不一致，改用解析单元'
    return pages, None


def _ocr_unreadable_pages(artifacts, raw, pages):
    """Keep a short real title. Recognize only a physical page with no usable text."""
    issues, unread = [], []
    ref = raw.get('original_ref')
    for index, page in enumerate(pages):
        if _USABLE_TEXT.search(page):
            continue
        recognized = _render_page(artifacts, ref, index)
        if recognized and _USABLE_TEXT.search(recognized):
            pages[index] = recognized
            issues.append({'code': 'outline_directory_ocr_unconfirmed',
                           'detail': f'第{index + 1}页没有文字层，由 tesseract 识别，需人工核对'})
        else:
            unread.append(index)
    return pages, issues, unread


def _render_page(artifacts, ref, index):
    from pathlib import Path
    from subprocess import run
    from tempfile import TemporaryDirectory

    try:
        data = artifacts.read_bytes(ref)
    except (OSError, KeyError, ValueError):
        return ''
    number = str(index + 1)
    with TemporaryDirectory() as directory:
        path = Path(directory) / 'source.pdf'
        path.write_bytes(data)
        prefix = Path(directory) / 'page'
        rendered = run(['pdftocairo', '-png', '-r', '300', '-f', number, '-l', number, '-singlefile',
                        str(path), str(prefix)], capture_output=True, check=False)
        image = prefix.with_suffix('.png')
        if rendered.returncode != 0 or not image.is_file():
            return ''
        return _tesseract(image.read_bytes())


def _bind_grid_lines(pages, units, pdf_bytes=None):
    """Locate a native grid from layout lines, or from ruled bands when one row is split."""
    owners = [[None] * len(page.splitlines()) for page in pages]
    pending = []
    for unit in units:
        if not unit.get('grid'):
            continue
        page = (unit.get('locator') or {}).get('page_ordinal')
        if not isinstance(page, int) or page < 0 or page >= len(pages):
            continue
        rows = _substantive_rows(unit)
        if not rows:
            continue
        found = _bind_rows_on_lines(pages[page].splitlines(), rows)
        if found:
            _own(owners, page, found, unit.get('unit_id'))
        else:
            pending.append((page, unit, rows))
    if pdf_bytes:
        _bind_grid_geometry(pages, owners, units, pdf_bytes)
    if pdf_bytes and pending:
        _bind_ruled_rows(pages, owners, pending, pdf_bytes)
    return owners


def _bind_grid_geometry(pages, owners, units, pdf_bytes):
    """Bind exact visual text lines to PDF table bounds; never infer from cell words."""
    from pathlib import Path
    from subprocess import run
    from tempfile import TemporaryDirectory

    grouped = {}
    for unit in units:
        loc = unit.get('locator') or {}
        page = loc.get('page_ordinal')
        if unit.get('grid') and isinstance(page, int) and all(
                isinstance(loc.get(key), (int, float)) for key in ('left', 'right', 'top', 'bottom')):
            grouped.setdefault(page, []).append(unit)
    with TemporaryDirectory() as directory:
        path = Path(directory) / 'source.pdf'
        path.write_bytes(pdf_bytes)
        for page, tables in grouped.items():
            if page < 0 or page >= len(pages):
                continue
            number = str(page + 1)
            result = run(['pdftotext', '-bbox', '-enc', 'UTF-8', '-f', number, '-l', number, str(path), '-'],
                         capture_output=True, check=False)
            if result.returncode:
                continue
            content = result.stdout.decode('utf-8', errors='replace')
            page_tag = content.partition('<page ')[2].partition('>')[0]
            height = _attribute(page_tag, 'height')
            if height is None:
                continue
            height = float(height)
            boxes = _bbox_word_boxes(content)
            visual = []
            words = [((left + right) / 2, (top + bottom) / 2, text)
                     for left, top, right, bottom, text in boxes]
            for x, y, text in sorted(words, key=lambda word: (word[1], word[0])):
                if visual and abs(visual[-1][0] - y) < _SAME_LINE:
                    visual[-1][1].append((x, text))
                else:
                    visual.append((y, [(x, text)]))
            by_text = {}
            for y, words in visual:
                words.sort()
                by_text.setdefault(_compact(''.join(word for _, word in words)), []).append((y, words))
            used, overlap = {}, None
            occurrences = {}
            for line in pages[page].splitlines():
                key = _compact(line)
                occurrences[key] = occurrences.get(key, 0) + 1
            for line_no, line in enumerate(pages[page].splitlines()):
                key = _compact(line)
                if not key:
                    continue
                index = used.get(key, 0)
                used[key] = index + 1
                candidates = by_text.get(key, [])
                if len(candidates) >= occurrences[key]:
                    physical = candidates if len(candidates) > occurrences[key] else candidates[index:index + 1]
                    matched = [_grid_center_owner(y, words, tables, height) for y, words in physical]
                    if matched and None not in matched and len(set(matched)) == 1:
                        owners[page][line_no] = matched[0]
                if owners[page][line_no] is not None or occurrences[key] != 1:
                    continue
                if overlap is None:
                    overlap = _overlap_word_lines(boxes)
                candidates = overlap.get(key, {})
                if len(candidates) != 1:
                    continue
                candidate = next(iter(candidates.values()))
                owner = _grid_word_owner(candidate, tables, height)
                if owner is not None:
                    owners[page][line_no] = owner


def _grid_center_owner(y, words, tables, height):
    matches = []
    for unit in tables:
        loc = unit['locator']
        if (height - loc['top'] - 1 <= y <= height - loc['bottom'] + 1
                and all(loc['left'] - 1 <= x <= loc['right'] + 1 for x, _ in words)):
            matches.append(unit['unit_id'])
    return matches[0] if len(matches) == 1 else None


def _grid_word_owner(words, tables, height):
    if (max(word[1] for word in words) >= min(word[3] for word in words)
            or any(left[2] > right[0] for left, right in zip(words, words[1:]))):
        return None
    matches = []
    for unit in tables:
        loc = unit['locator']
        if all(loc['left'] - 1 <= left <= right <= loc['right'] + 1
               and height - loc['top'] - 1 <= top <= bottom <= height - loc['bottom'] + 1
               for left, top, right, bottom, _ in words):
            matches.append(unit['unit_id'])
    return matches[0] if len(matches) == 1 else None


def _bind_rows_on_lines(lines, rows):
    found, cursor = [], 0
    for cells in rows:
        located = next((offset for offset in range(cursor, len(lines)) if _line_has_cells(lines[offset], cells)), None)
        if located is None:
            return []
        found.append(located)
        cursor = located + 1
    return found


def _own(owners, page, offsets, unit_id):
    for offset in offsets:
        if owners[page][offset] is None:
            owners[page][offset] = unit_id


def _pdf_bytes(artifacts, raw):
    ref = raw.get('original_ref')
    if artifacts is None or not isinstance(ref, dict):
        return None
    try:
        data = artifacts.read_bytes(ref)
    except (OSError, KeyError, ValueError):
        return None
    return data if data.startswith(b'%PDF') else None


_RULE_TOLERANCE = 4.0
_RULE_MIN_LENGTH = 5.0
_RULE_MAX_THICKNESS = 1.5
_SAME_LINE = 3.0


def _numbers(text):
    values, current = [], ''
    for char in text:
        if char.isdigit() or char in '.-':
            current += char
            continue
        if current not in {'', '-', '.', '-.'}:
            values.append(float(current))
        current = ''
    if current not in {'', '-', '.', '-.'}:
        values.append(float(current))
    return values


def _attribute(element, name):
    token = name + '="'
    start = element.find(token)
    if start < 0:
        return None
    start += len(token)
    end = element.find('"', start)
    return None if end < 0 else element[start:end]


def _svg_rules(svg):
    horizontal, vertical = [], []
    for part in svg.split('<path')[1:]:
        element = part[:part.find('>') if '>' in part else len(part)]
        path = _attribute(element, 'd')
        if not path or 'C' in path or 'Q' in path:
            continue
        matrix = _numbers(_attribute(element, 'transform') or '')
        if len(matrix) != 6:
            matrix = [1, 0, 0, 1, 0, 0]
        points = []
        raw = _numbers(path)
        for index in range(0, len(raw) - 1, 2):
            x, y = raw[index], raw[index + 1]
            points.append((matrix[0] * x + matrix[2] * y + matrix[4], matrix[1] * x + matrix[3] * y + matrix[5]))
        stroked = (_attribute(element, 'stroke') or 'none') != 'none' and 'stroke=' in element

        def push(start, end, stroked=stroked):
            x1, y1 = start
            x2, y2 = end
            if abs(y1 - y2) < 0.5 and abs(x1 - x2) > _RULE_MIN_LENGTH:
                horizontal.append((min(x1, x2), max(x1, x2), y1))
            elif abs(x1 - x2) < 0.5 and abs(y1 - y2) > _RULE_MIN_LENGTH:
                vertical.append((min(y1, y2), max(y1, y2), x1))

        if stroked:
            for index in range(len(points) - 1):
                push(points[index], points[index + 1])
        elif len(points) >= 4:
            xs, ys = [point[0] for point in points], [point[1] for point in points]
            left, right, top, bottom = min(xs), max(xs), min(ys), max(ys)
            if bottom - top < _RULE_MAX_THICKNESS:
                push((left, (top + bottom) / 2), (right, (top + bottom) / 2))
            elif right - left < _RULE_MAX_THICKNESS:
                push(((left + right) / 2, top), ((left + right) / 2, bottom))
    return horizontal, vertical


def _bbox_words(xhtml):
    return [((left + right) / 2, (top + bottom) / 2, text)
            for left, top, right, bottom, text in _bbox_word_boxes(xhtml)]


def _bbox_word_boxes(xhtml):
    words = []
    for part in xhtml.split('<word ')[1:]:
        element = part[:part.find('>') if '>' in part else len(part)]
        box = [_attribute(element, name) for name in ('xMin', 'yMin', 'xMax', 'yMax')]
        if any(value is None for value in box):
            continue
        end = part.find('</word>')
        text = part[part.find('>') + 1:end if end >= 0 else len(part)]
        for old, new in (('&lt;', '<'), ('&gt;', '>'), ('&quot;', '"'), ('&apos;', "'"), ('&amp;', '&')):
            text = text.replace(old, new)
        x1, y1, x2, y2 = (float(cast(str, value)) for value in box)
        words.append((x1, y1, x2, y2, _compact(text)))
    return [word for word in words if word[4]]


def _overlap_word_lines(boxes):
    """Index physical word groups once; neighbouring lines in one column cannot merge."""
    events, active, result = {}, set(), {}
    for index, (_, top, _, bottom, _) in enumerate(boxes):
        if bottom <= top:
            continue
        events.setdefault(top, [[], []])[0].append(index)
        events.setdefault(bottom, [[], []])[1].append(index)
    for position in sorted(events):
        started, ended = events[position]
        active.difference_update(ended)
        active.update(started)
        if not active:
            continue
        words = sorted((boxes[index] for index in active), key=lambda word: word[0])
        if any(left[2] > right[0] for left, right in zip(words, words[1:])):
            continue
        key = ''.join(word[4] for word in words)
        result.setdefault(key, {})[tuple(sorted(active))] = words
    return result


def _ruled_bands(svg, xhtml):
    horizontal, vertical = _svg_rules(svg)
    if len(horizontal) < 2 or len(vertical) < 2:
        return []
    levels = sorted({rule[2] for rule in horizontal})
    words = _bbox_words(xhtml)
    bands = []
    for top, bottom in zip(levels, levels[1:]):
        if not any(rule[0] - _RULE_TOLERANCE <= top and rule[1] + _RULE_TOLERANCE >= bottom for rule in vertical):
            continue
        inside = [word for word in words if top - _RULE_TOLERANCE <= word[1] <= bottom + _RULE_TOLERANCE]
        lines = []
        for _x, y, text in sorted(inside, key=lambda word: (word[1], word[0])):
            if lines and abs(lines[-1][0] - y) < _SAME_LINE:
                lines[-1][1].append(text)
            else:
                lines.append((y, [text]))
        bands.append({'lines': {_compact(''.join(texts)) for _y, texts in lines}})
    return [band for band in bands if band['lines']]


def _band_has_cells(band, cells):
    return all(any(cell in line for line in band['lines']) for cell in cells)


def _bind_ruled_rows(pages, owners, pending, pdf_bytes):
    """Use page rules only for a native grid whose cells are not on one layout line."""
    from pathlib import Path
    from subprocess import run
    from tempfile import TemporaryDirectory

    by_page = {}
    for page, unit, rows in pending:
        by_page.setdefault(page, []).append((unit, rows))
    with TemporaryDirectory() as directory:
        path = Path(directory) / 'source.pdf'
        path.write_bytes(pdf_bytes)
        for page, units in by_page.items():
            number = str(page + 1)
            svg = run(['pdftocairo', '-svg', '-f', number, '-l', number, str(path), '-'],
                      capture_output=True, check=False)
            words = run(['pdftotext', '-bbox', '-enc', 'UTF-8', '-f', number, '-l', number, str(path), '-'],
                        capture_output=True, check=False)
            if svg.returncode != 0 or words.returncode != 0:
                continue
            bands = _ruled_bands(svg.stdout.decode('utf-8', errors='replace'),
                                 words.stdout.decode('utf-8', errors='replace'))
            lines = pages[page].splitlines()
            for unit, rows in units:
                offsets = []
                for cells in rows:
                    band = next((item for item in bands if _band_has_cells(item, cells)), None)
                    if band is None:
                        offsets = []
                        break
                    matched = [index for index, line in enumerate(lines) if _compact(line) in band['lines']]
                    if not matched or not _line_has_cells('\n'.join(lines[index] for index in matched), cells):
                        offsets = []
                        break
                    offsets.extend(matched)
                _own(owners, page, offsets, unit.get('unit_id'))


def _substantive_rows(unit):
    grouped = {}
    for cell in (unit.get('grid') or {}).get('cells') or []:
        grouped.setdefault(cell.get('row'), []).append(cell)
    rows = []
    for row in sorted(key for key in grouped if key is not None):
        cells = [((cell.get('text') or '').strip()) for cell in sorted(grouped[row], key=lambda item: item.get('column') or 0)]
        cells = [cell for cell in cells if len(cell) >= 2]
        if cells:
            rows.append(cells)
    return rows


def _line_has_cells(line, cells):
    cursor = 0
    for cell in cells:
        found = line.find(cell, cursor)
        if found < 0:
            return False
        cursor = found + len(cell)
    return True


def _grid_parts(index, source_index_ref):
    """A complete native grid is selected with no selectors; row selectors are rejected on export."""
    parts = {}
    for unit in index.raw.get('units', []):
        if not unit.get('grid'):
            continue
        part = {'source_ref': {'source_index_ref': source_index_ref, 'unit_id': unit['unit_id']}, 'selectors': []}
        count = _native_header_count(unit)
        if count is not None:
            part['header_row_count'] = count
        parts[unit['unit_id']] = part
    return parts


def _native_header_count(unit):
    grid = unit['grid']
    coordinates = {(cell.get('row'), cell.get('column')) for cell in unit.get('header_cells') or []}
    rows = {row for row, _column in coordinates}
    count = 0
    while count in rows:
        count += 1
    if not count or count >= grid['row_count'] or rows != set(range(count)):
        return None
    expected = {(cell['row'], cell['column']) for cell in grid['cells'] if cell['row'] < count}
    if coordinates != expected or any(cell['row'] < count < cell['row'] + cell.get('row_span', 1) for cell in grid['cells']):
        return None
    return count


def _grid_catalog(index):
    catalog = []
    for unit in index.raw.get('units') or []:
        if not unit.get('grid'):
            continue
        page = (unit.get('locator') or {}).get('page_ordinal')
        rows = _substantive_rows(unit)
        if isinstance(page, int):
            catalog.append({'unit_id': unit.get('unit_id'), 'page': page, 'rows': rows,
                            'grid': deepcopy(unit['grid'])})
    return catalog


_OCR_MIN_CHARS = 20


def _native_chars(unit):
    """Characters from the text layer or native cells, never from OCR."""
    text = unit.get('text') or ''
    if not text.strip():
        text = ''.join(cell.get('text') or '' for cell in (unit.get('grid') or {}).get('cells') or [])
    return len(''.join(text.split()))


def _page_native_counts(units):
    totals = {}
    for unit in units:
        page = (unit.get('locator') or {}).get('page_ordinal')
        if isinstance(page, int):
            totals[page] = totals.get(page, 0) + _native_chars(unit)
    return totals


def _image_texts(artifacts, raw):
    """OCR only a physical page whose own text layer has fewer than 20 characters.

    Text that can be read directly stays verbatim. OCR never overwrites it, and a short
    heading that has no page number is not treated as a scan.
    """
    units = raw.get('units', [])
    counts = _page_native_counts(units)
    texts, issues, added = {}, [], {}
    for unit in units:
        page = (unit.get('locator') or {}).get('page_ordinal')
        if not isinstance(page, int) or counts.get(page, 0) >= _OCR_MIN_CHARS:
            continue
        record = (raw.get('ocr') or {}).get(unit.get('unit_id')) or {}
        if record.get('disposition'):
            continue
        vision = (record.get('text') or '').strip()
        recognized, engine = vision, '视觉模型'
        if not recognized and unit.get('kind') == 'image_region':
            image_key = (unit.get('locator') or {}).get('image_key')
            image_ref = (raw.get('images') or {}).get(image_key) if image_key else None
            if not isinstance(image_ref, dict):
                continue
            recognized = _tesseract(artifacts.read_bytes(image_ref))
            engine = 'tesseract' if recognized else '未识别'
        elif not recognized:
            continue
        if recognized:
            texts[unit['unit_id']] = recognized
            added[page] = added.get(page, 0) + len(''.join(recognized.split()))
        issues.append(_ocr_issue(unit, engine))
    unread = [page for page, count in sorted(counts.items()) if count + added.get(page, 0) < _OCR_MIN_CHARS]
    return texts, issues, unread


def _ocr_issue(unit, engine):
    page = (unit.get('locator') or {}).get('page_ordinal')
    place = f'第{page + 1}页' if isinstance(page, int) else unit.get('unit_id', '')
    return {'code': 'outline_directory_ocr_unconfirmed',
            'detail': f'{place}扫描文字由{engine}识别，需人工核对'}


def _tesseract(data):
    if not data:
        return ''
    from subprocess import run
    completed = run(['tesseract', 'stdin', 'stdout', '-l', 'chi_sim+eng', '--psm', '6'],
                    input=data, capture_output=True, check=False)
    if completed.returncode != 0:
        return ''
    return completed.stdout.decode('utf-8', errors='replace').strip()


def _project_units(units, recognized=None):
    recognized = recognized or {}
    pages, owners, current, lines, line_owners = [], [], None, [], []

    def flush():
        nonlocal lines, line_owners
        if lines:
            pages.append('\n'.join(lines))
            owners.append(line_owners)
            lines, line_owners = [], []

    for unit in units:
        page = (unit.get('locator') or {}).get('page_ordinal')
        parts = _unit_lines(unit, recognized.get(unit.get('unit_id')))
        owned = [unit.get('unit_id') if unit.get('grid') else None for _ in parts]
        if page is None:
            flush()
            current = None
            if parts:
                pages.append('\n'.join(parts))
                owners.append(owned)
            continue
        if current != page:
            flush()
            current = page
        lines.extend(parts)
        line_owners.extend(owned)
    flush()
    return pages, owners


def outline_from_compiled(snapshot, compiled, *, artifacts=None):
    """Turn one compiled directory into an outline. Titles and template text stay verbatim."""
    from docreader.tender.schema import AuthoringOutline

    node_map = {}
    chapters = _assign_ids(_nest(compiled['chapters'], source_nodes=node_map))
    native_issues = (_bind_parameter_tables(compiled, node_map)
                     if compiled.get('_parameter_source_contract') == PARAMETER_SOURCE_CONTRACT else [])
    source_issues = [*compiled['issues'], *native_issues]
    compiled['_source_issue_cases'] = {index: issue['requirement_case_id']
        for index, issue in enumerate(source_issues) if issue.get('requirement_case_id')}
    outline = {'schema_version': 2, 'authoring_input': snapshot,
               'chapters': chapters,
               'issues': [{'code': issue['code'], 'detail': issue['detail'], 'rule_refs': [],
                           **({'source_evidence_ref': issue['source_evidence_ref']} if issue.get('source_evidence_ref') else {}),
                           **({'target_id': issue['target_id']}
                              if issue['code'] in {'outline_directory_response_form_decision',
                                                   'outline_directory_response_form_row_roles'} else {})}
                          for issue in source_issues]}
    from docreader.tender.source_presentation import collect_source_renderings
    from docreader.tender.authoring_coverage import attach_knowledge_answer_slots
    collect_source_renderings(chapters, compiled)
    attach_knowledge_answer_slots(outline)
    return AuthoringOutline.model_validate(outline).model_dump()


def authoring_outline(snapshot, plans, documents):
    return outline_from_compiled(snapshot, compile_plans(plans, documents))


def requirement_blocks(documents, limit=600):
    """Pack whole pages. A page is never split, even when it is longer than the limit."""
    blocks, current, count = [], [], 0

    def flush():
        nonlocal current, count
        if current:
            blocks.append(current)
            current, count = [], 0

    for document in documents:
        for number, page in enumerate(document['pages']):
            size = len(page.splitlines())
            if current and count + size > limit:
                flush()
            current.append({'document_id': document['document_id'], 'page': number, 'lines': [
                {'line': line, 'text': text} for line, text in enumerate(page.splitlines())]})
            count += size
            if size >= limit:
                flush()
    flush()
    return blocks


def validate_requirement_windows(documents, assignments, block):
    """A requirement may join visible source windows, never guess unseen continuation lines."""
    source = _Source(documents)
    visible = {(page['document_id'], page['page'], line['line'])
               for page in block for line in page['lines']}
    for raw in assignments:
        for document_id, page, start, end in _requirement_windows(raw, source):
            if any((document_id, page, line) not in visible for line in range(start, end + 1)):
                raise DirectoryPlanRejected('requirement window is outside the supplied block')


def apply_requirements(compiled, documents, assignments, *, complete_basis=False):
    """Copy new requirements, keeping an adjudicated basis complete when requested."""
    source = _Source(documents)
    used = _used_lines(compiled)
    known = {chapter['id']: chapter for chapter in compiled['chapters']}
    covers = {identity for identity, chapter in known.items() if chapter.get('presentation') == DirectoryPresentation.COVER}
    obligation_owners = {identity: {(line['document_id'], line['page'], line['line'])
                                  for line in chapter.get('obligation_lines', [])}
                         for identity, chapter in known.items()}
    for raw in assignments:
        try:
            windows = _requirement_windows(raw, source)
            if len(windows) == 1 and not complete_basis:
                text = _requirement_text(source, raw, used)
            else:
                selected = {(document_id, page, line) for document_id, page, start, end in windows
                            for line in range(start, end + 1)}
                text = ('\n'.join(_window_text(source, window) for window in windows).strip()
                        if complete_basis or selected - used else '')
        except DirectoryPlanRejected as error:
            compiled['issues'].append({'code': 'outline_directory_requirement_rejected', 'detail': error.detail})
            continue
        if not text:
            continue
        selected = {(document_id, page, line) for document_id, page, start, end in windows
                    for line in range(start, end + 1)}
        full_rows = _source_rows(source, [dict(document_id=d, page=p, line=number)
            for d, p, start, end in windows for number in range(start, end + 1)])
        text_rows = full_rows if complete_basis or len(windows) > 1 else [r for r in full_rows
            if (r['document_id'], r['page'], r['line']) not in used]

        def append_requirement(chapter, value):
            rows = full_rows if value == '\n'.join(r['text'] for r in full_rows).strip() else text_rows
            origins = chapter.setdefault('_requirement_rows', [None] * len(chapter.get('requirements', [])))
            chapter.setdefault('requirements', []).append(value)
            origins.append(deepcopy(rows))

        def chapter_text(identity):
            owned = obligation_owners[identity]
            if selected <= owned:
                return ''
            if selected & owned:
                locations = '；'.join(f'{document_id}第{page + 1}页第{start + 1}-{end + 1}行'
                                     for document_id, page, start, end in windows)
                compiled['issues'].append({'code': 'outline_directory_requirement_overlap_unconfirmed',
                    'detail': f'“{known[identity]["title"]}”的要求与已承载组成义务部分同源，保留完整条款；'
                              f'{locations}；请在Word中核定并合并应答。'})
                return '\n'.join(_window_text(source, window) for window in windows).strip()
            return text

        ids = raw.get('chapter_ids') or []
        scope = raw.get('scope')
        if scope == RequirementScope.CHAPTER and len(ids) == 1 and ids[0] in known:
            if ids[0] in covers:
                compiled['issues'].append({'code': 'outline_directory_document_requirement', 'detail': text})
            else:
                value = chapter_text(ids[0])
                if value:
                    append_requirement(known[ids[0]], value)
        elif scope == RequirementScope.CHAPTERS and ids and all(identity in known for identity in ids):
            for identity in dict.fromkeys(ids):
                if identity in covers:
                    continue
                known[identity]['requirement_cover'] = True
                value = chapter_text(identity)
                if value:
                    append_requirement(known[identity], value)
            compiled['issues'].append({'code': ('outline_directory_document_requirement' if covers.intersection(ids)
                                               else 'outline_directory_shared_requirement'), 'detail': text})
        elif scope == RequirementScope.DOCUMENT:
            compiled['issues'].append({'code': 'outline_directory_document_requirement', 'detail': text})
        else:
            issue = {'code': 'outline_directory_requirement_unassigned', 'detail': text}
            _record_requirement_case(compiled, source, raw, windows, issue)
            compiled['issues'].append(issue)
    return compiled


def _record_requirement_case(compiled, source, raw, windows, issue):
    source_windows = [dict(zip(('document_id', 'page', 'start_line', 'end_line'), window))
                      for window in windows]
    source_text = '\n'.join(_window_text(source, window) for window in windows).strip()
    identity = json.dumps({'source_windows': source_windows, 'source_text': source_text},
                          ensure_ascii=False, sort_keys=True, separators=(',', ':'))
    case_id = 'requirement-' + hashlib.sha256(identity.encode('utf-8')).hexdigest()
    issue['requirement_case_id'] = case_id
    cases = compiled.setdefault('_requirement_cases', [])
    case = next((case for case in cases if case['id'] == case_id), None)
    if case is None:
        case = {'id': case_id, 'source_windows': source_windows, 'source_text': source_text,
                '_source_rows': _source_rows(source, [dict(document_id=d, page=p, line=number)
                    for d, p, start, end in windows for number in range(start, end + 1)]),
                'raw_assignment': deepcopy(raw), 'issue_indices': []}
        cases.append(case)
    case['issue_indices'].append(len(compiled['issues']))
    return case


def unplaced_tables(compiled, documents):
    """Locate unassigned native tables without copying their text into the request."""
    source = _Source(documents)
    claimed = {_template_identity(part) for chapter in compiled['chapters'] if not chapter.get('unplaced')
               for part in chapter.get('template_parts') or []}
    result = []
    for document in source.documents:
        for grid in document.get('grids') or []:
            part = source.template_part(document['document_id'], grid['unit_id'])
            page = grid.get('page')
            if part is None or _template_identity(part) in claimed or not isinstance(page, int):
                continue
            owners = document.get('line_units') or []
            if not 0 <= page < len(owners):
                continue
            lines = [index for index, unit in enumerate(owners[page]) if unit == grid['unit_id']]
            if lines:
                result.append({'document_id': document['document_id'], 'page': page,
                               'start_line': min(lines), 'end_line': max(lines)})
    return result


def _heading_point(chapter):
    heading = chapter.get('heading') or {}
    return heading.get('document_id'), heading.get('page'), heading.get('line')


def _stable_table_chapter(compiled, document_id, page, start_line, *, selected=None):
    """Place one source table from its position, not from a varying parent/child choice."""
    point = (document_id, page, start_line)
    chapters = [chapter for chapter in compiled['chapters'] if not chapter.get('unplaced')]
    holders = [chapter for chapter in chapters if any(
        (line.get('document_id'), line.get('page'), line.get('line')) == point for line in chapter.get('lines') or [])]
    if holders:
        return min(holders, key=lambda chapter: len(chapter.get('lines') or []))

    def choose(candidate):
        if selected is None:
            return candidate
        if selected.get('presentation') in {'cover', 'content'}:
            return None
        parents = {chapter['id']: chapter.get('parent_id') for chapter in chapters}
        if candidate is not None and _root(candidate['id'], parents) == _root(selected['id'], parents):
            return candidate
        return selected

    same = [chapter for chapter in chapters if _heading_point(chapter)[0] == document_id
            and all(isinstance(value, int) for value in _heading_point(chapter)[1:])]
    after = sorted((chapter for chapter in same if _heading_point(chapter)[1:] > (page, start_line)), key=_heading_point)
    if not after:
        return choose(None)
    following = after[0]
    parent_id = following.get('parent_id')
    siblings = [chapter for chapter in same if chapter.get('parent_id') == parent_id] if parent_id else []
    if parent_id and siblings and min(siblings, key=_heading_point)['id'] == following['id']:
        return choose(next((chapter for chapter in chapters if chapter['id'] == parent_id), following))
    previous = [chapter for chapter in siblings if _heading_point(chapter)[1:] < (page, start_line)]
    return choose(max(previous, key=_heading_point) if previous else following)


def apply_materials(compiled, documents, assignments, *, source_owner=False):
    """Copy non-template source into a chapter. Lines already carried are not repeated."""
    source = _Source(documents)
    known = {chapter['id']: chapter for chapter in compiled['chapters'] if not chapter.get('unplaced')}
    for raw in assignments:
        chapter = known.get(raw.get('chapter_id'))
        if chapter is None:
            compiled['issues'].append({'code': 'outline_directory_material_rejected', 'detail': 'material chapter is unknown'})
            continue
        try:
            used = _used_lines(compiled)
            text = _requirement_text(source, raw, used)
        except DirectoryPlanRejected as error:
            compiled['issues'].append({'code': 'outline_directory_material_rejected', 'detail': error.detail})
            continue
        if not text:
            continue
        refs = [{'document_id': raw['document_id'], 'page': raw['page'], 'line': line,
                 **({'unit_id': unit} if (unit := source.unit_at(raw['document_id'], raw['page'], line)) else {})}
                for line in range(raw['start_line'], raw['end_line'] + 1)
                if (raw['document_id'], raw['page'], line) not in used]
        temporary = {'lines': refs}
        _attach_template_parts([temporary], source)
        selected_units = {ref['unit_id'] for ref in refs if ref.get('unit_id')}
        complete_units = {part['source_ref']['unit_id'] for part in temporary.get('template_parts') or []}
        if selected_units - complete_units:
            compiled['issues'].append({'code': 'outline_directory_material_rejected',
                                       'detail': f'第{raw["page"] + 1}页的材料窗口未覆盖完整表格，保留原文并待核'})
        if complete_units:
            owner = _stable_table_chapter(compiled, raw['document_id'], raw['page'], raw['start_line'],
                                          selected=chapter if source_owner else None)
            if owner is None:
                compiled['issues'].append({'code': 'outline_directory_material_rejected',
                                           'detail': f'第{raw["page"] + 1}页的表没有落在已采用章节的原文范围内，未改挂到后文章节'})
                continue
            if owner is not chapter:
                compiled['issues'].append({'code': 'outline_directory_extra',
                                           'detail': f'第{raw["page"] + 1}页的表按原文位置归入“{owner["title"]}”'})
                chapter = owner
        _segment_template_content([temporary], source)
        segments = temporary.get('template_segments')
        if segments:
            chapter.setdefault('material_segments', []).extend(segments)
            moved = {_template_identity(part) for part in temporary.get('template_parts') or []}
            emptied = set()
            for other in compiled['chapters']:
                if other.get('unplaced') and any(_template_identity(part) in moved
                                                  for part in other.get('template_parts') or []):
                    other['template_parts'] = [part for part in other.get('template_parts') or []
                                               if _template_identity(part) not in moved]
                    if not other['template_parts']:
                        emptied.add(other['id'])
            compiled['chapters'][:] = [other for other in compiled['chapters'] if other['id'] not in emptied]
        else:
            chapter.setdefault('_material_rows', [None] * len(chapter.get('materials', []))).append(_source_rows(source, refs))
            chapter.setdefault('materials', []).append(text)
        chapter.setdefault('lines', []).extend(refs)
    return compiled


def _table_id(part):
    reference = part['source_ref']
    identity = (reference['source_index_ref']['sha256'], reference['unit_id'])
    return 'table-' + hashlib.sha256(json.dumps(identity).encode()).hexdigest()[:16]


def _bound_table_parts(compiled):
    for chapter in compiled['chapters']:
        if chapter.get('unplaced'):
            continue
        yield from chapter.get('template_parts') or []
        for field in ('template_segments', 'material_segments'):
            yield from (segment['part'] for segment in chapter.get(field) or [] if segment.get('part'))


def _source_tables(documents):
    tables = {}
    for document in documents:
        grids = {grid['unit_id']: grid for grid in document.get('grids') or []}
        for unit_id, part in (document.get('template_parts') or {}).items():
            grid = grids.get(unit_id, {})
            page = grid.get('page')
            if page is None:
                page = next((page for page, owners in enumerate(document.get('line_units') or [])
                             if unit_id in owners), None)
            identity = _table_id(part)
            tables.setdefault(identity, {'table_id': identity, 'document_id': document['document_id'],
                                         'page': page, 'unit_id': unit_id, 'grid': grid.get('grid')})
    return tables


def table_header_catalog(compiled, documents):
    """Offer complete already-bound native grids once, without adopting any new table."""
    from docreader.tender.authoring_view import _grid_view

    parts = list(_bound_table_parts(compiled))
    bound = {_table_id(part) for part in parts}
    known = {_table_id(part) for part in parts if part.get('header_row_count') is not None}
    return [{**table, 'grid': _grid_view(deepcopy(table['grid']))}
            for identity, table in _source_tables(documents).items()
            if identity in bound - known and table['grid']]


def validate_table_headers(catalog, decisions):
    known = {table['table_id']: table for table in catalog}
    selected = {}
    for decision in decisions:
        identity, count = decision['table_id'], decision['count']
        if identity not in known:
            raise DirectoryPlanRejected(f'table_headers contains an unknown table_id: {identity}')
        if identity in selected and selected[identity] != count:
            raise DirectoryPlanRejected(f'table_headers has contradictory counts for {identity}')
        grid = known[identity]['grid']
        if count is not None:
            if type(count) is not int or count < 0 or (count > 0 and count >= grid['row_count']):
                raise DirectoryPlanRejected(f'table_headers count is outside the source rows for {identity}')
            spans = ([(cell['row'], cell['row_span']) for cell in grid['cells']]
                     if 'cells' in grid else [(row, row_span) for row, _column, row_span, _col_span
                                             in grid['merged_cells']])
            if any(row < count < row + span for row, span in spans):
                raise DirectoryPlanRejected(f'table_headers count crosses a merged cell for {identity}')
        selected[identity] = count


def apply_table_headers(compiled, documents, catalog, decisions=(), *, failure=None):
    """Set display metadata on every source copy; unresolved source tables stay intact."""
    validate_table_headers(catalog, decisions)
    selected = {decision['table_id']: decision['count'] for decision in decisions}
    sources = _source_tables(documents)
    parts = list(_bound_table_parts(compiled))
    known = {}
    for part in parts:
        if part.get('header_row_count') is not None:
            known.setdefault(_table_id(part), set()).add(part['header_row_count'])
    unresolved = {}
    offered = {table['table_id'] for table in catalog}
    for part in parts:
        identity = _table_id(part)
        if part.get('header_row_count') is not None:
            continue
        if len(known.get(identity, ())) == 1:
            part['header_row_count'] = next(iter(known[identity]))
            continue
        count = selected.get(identity)
        if count is not None:
            part['header_row_count'] = count
            continue
        source = sources.get(identity, {'document_id': '来源文件待核', 'page': None,
                                        'unit_id': part['source_ref']['unit_id']})
        page = f'第{source["page"] + 1}页' if source['page'] is not None else '来源页待核'
        reason = failure or ('本轮材料归入后尚未判断' if identity not in offered else '模型未确认')
        unresolved[identity] = {'code': 'outline_directory_table_header_unconfirmed',
                               'detail': f'{source["document_id"]}{page}原表{source["unit_id"]}（{identity}）'
                                         f'重复标题行未确认：{reason}；在 Word 确认重复标题行后保存，'
                                         '并在本项处理结果栏登记。'}
    compiled['issues'].extend(unresolved.values())
    return compiled


def apply_classification(compiled, documents, items):
    source = _Source(documents)
    labels = {WorkKind.GOODS: '货物', WorkKind.SERVICE: '服务', WorkKind.WORKS: '工程', WorkKind.UNRESOLVED: '未能确定'}
    for raw in items:
        try:
            text = _requirement_text(source, raw, set())
        except DirectoryPlanRejected as error:
            compiled['issues'].append({'code': 'outline_directory_classification_rejected', 'detail': error.detail})
            continue
        if not text:
            continue
        kind = raw.get('kind')
        compiled['issues'].append({'code': 'outline_directory_classification',
                                   'detail': f'{labels.get(kind, kind)}：{text}'})
    return compiled


def sparse_page_issues(documents):
    """Only a physical page that is still under 20 characters after direct reading and OCR."""
    issues = []
    for document in documents:
        for page in document.get('unread_pages') or []:
            issues.append({'code': 'outline_directory_sparse_page',
                           'detail': f'{document["document_id"]}第{page + 1}页文字不足，需对照原件人工核对'})
    return issues


def _used_lines(compiled):
    return {(line['document_id'], line['page'], line['line'])
            for chapter in compiled['chapters'] for line in chapter.get('lines', [])}


def parameter_view(chapter, documents):
    """Numbered source lines for one long chapter. The model returns locations, not a copy."""
    pages = {document['document_id']: document['pages'] for document in documents}
    rows = []
    for ref in chapter.get('lines') or []:
        document_id, page, line = ref.get('document_id'), ref.get('page'), ref.get('line')
        try:
            text = pages[document_id][page].splitlines()[line]
        except (KeyError, IndexError, TypeError):
            continue
        rows.append({'document_id': document_id, 'page': page, 'line': line, 'text': text})
    return rows


def apply_parameters(compiled, documents, chapters):
    """Split a chapter's own long body from each item's content start through its end."""
    source = _Source(documents)
    source_contract = compiled.get('_parameter_source_contract')
    native_lines = _parameter_native_lines(source) if source_contract == PARAMETER_SOURCE_CONTRACT else {}
    expanded = set()
    for raw in chapters:
        parent = next((chapter for chapter in compiled['chapters'] if chapter['id'] == raw.get('chapter_id')), None)
        if parent is None:
            compiled['issues'].append({'code': 'outline_directory_parameter_rejected', 'detail': 'parameter chapter is unknown'})
            continue
        if len([line for line in parent.get('text', '').splitlines() if line.strip()]) < 25:
            compiled['issues'].append({'code': 'outline_directory_parameter_short',
                                       'detail': f'“{parent["title"]}”不足25行，不展开'})
            continue
        if parent['id'] in expanded:
            compiled['issues'].append({'code': 'outline_directory_parameter_rejected',
                                       'detail': 'parameter parent is repeated in the same plan'})
            continue
        expanded.add(parent['id'])
        used = {(line['document_id'], line['page'], line['line'])
                for child in compiled['chapters'] if child.get('parent_id') == parent['id']
                for line in [*child.get('lines', []), child.get('heading', {})]
                if all(field in line for field in ('document_id', 'page', 'line'))}
        children = []
        for index, item in enumerate(raw.get('items') or []):
            try:
                child = _parameter_child(parent, source, item, index, source_contract=source_contract)
                selected = {(line['document_id'], line['page'], line['line'])
                            for line in [*child['lines'], child['heading']]}
                overlap = selected & used
                if overlap:
                    document_id, page, line = min(overlap)
                    raise DirectoryPlanRejected(
                        f'parameter items overlap at {document_id} page {page} line {line}')
                if source_contract == PARAMETER_SOURCE_CONTRACT:
                    _prepare_parameter_tables(child, source, item, native_lines)
                children.append(child)
                used.update(selected)
            except DirectoryPlanRejected as error:
                compiled['issues'].append({'code': 'outline_directory_parameter_rejected', 'detail': error.detail})
        compiled['chapters'].extend(sorted(children, key=lambda child: source.position(*_heading(child))))
    return compiled


def note_missing_requirements(compiled):
    parents = {chapter.get('parent_id') for chapter in compiled['chapters']}
    for chapter in compiled['chapters']:
        if chapter['id'] in parents or chapter.get('requirements') or chapter.get('requirement_cover'):
            continue
        if chapter.get('obligations', '').strip() or chapter.get('presentation') in {DirectoryPresentation.CONTENT, DirectoryPresentation.COVER}:
            continue
        compiled['issues'].append({'code': 'outline_directory_requirement_missing',
                                   'detail': f'叶子章节“{chapter["title"]}”没有带来源的章节要求'})
    return compiled


_FORMAT_CHAPTER = re.compile(r'^第[0-9０-９一二三四五六七八九十百千]+章')
_FORMAT_ITEM = re.compile(
    r'^(?:(?P<annex>附件\s*[0-9０-９]+\s*[A-Za-z]?)|(?P<label>[一二三四五六七八九十百千]+、))')
_FORMAT_TOC = re.compile(r'[.．…]{3,}|…{2,}')


def _format_lines(documents):
    rows = []
    for document in documents or []:
        if document.get('role') not in (None, 'main'):
            continue
        for page, text in enumerate(document.get('pages') or []):
            for line, raw in enumerate(str(text).splitlines()):
                rows.append((document.get('document_id'), page, line, raw, raw.strip()))
    return rows


def _format_heading(text):
    compact = text.replace(' ', '').replace('\u3000', '')
    return compact == '投标文件格式' or (
        compact.endswith('投标文件格式') and _FORMAT_CHAPTER.match(text) is not None)


def _chapter_boundary(text):
    """A chapter heading. A sentence that only quotes 第X章 does not end the format section."""
    if not text or '投标文件格式' in text.replace(' ', '').replace('\u3000', ''):
        return False
    match = _FORMAT_CHAPTER.match(text)
    if match is None:
        return False
    rest = text[match.end():].strip(' \t')
    if any(mark in text for mark in ('。', '；', '，', '“', '”', '"')):
        return False
    if re.search(r'第\s*[0-9０-９]', rest):
        return False
    return len(rest) <= 24


def _format_item(text, raw):
    """A format-list line used only to find the bid-format section. It is not a chapter rule."""
    if not text or _FORMAT_TOC.search(raw) or '。' in text:
        return False
    match = _FORMAT_ITEM.match(text)
    if match is None:
        return False
    rest = text[match.end():].strip(' \t：:')
    return bool(rest) and len(rest) <= 80


def _format_anchor(rows):
    anchors = [index for index, row in enumerate(rows) if _format_heading(row[4])]
    best, score = None, -1
    for index in anchors:
        count = 0
        for follow in rows[index + 1:]:
            if _chapter_boundary(follow[4]):
                break
            if _format_item(follow[4], follow[3]):
                count += 1
        if count >= score:
            best, score = index, count
    return best


_ANNEX_ONLY = re.compile(r'^附件\s*[0-9０-９]+\s*[A-Za-z]?\s*$')
_ANNEX_CODE = re.compile(r'^附件\s*([0-9０-９]+)\s*([A-Za-z]?)')
_SUBCODE_TITLE = re.compile(
    r'^(?P<name>.+?)(?:的格式|格式)?\s*[（(]\s*(?P<num>[0-9０-９]+)\s*(?P<letter>[A-Za-z]?)\s*[)）]\s*$')
_LABEL_NOTE = re.compile(
    r'（(?:本次不适用|如有|适用于[^）]*|投标人可根据[^）]*)）'
    r'|\((?:本次不适用|如有|适用于[^)]*|投标人可根据[^)]*)\)')
_VOLUME_LINE = re.compile(
    r'^(?:[0-9]+(?:\.[0-9]+)+\s*)?(?P<name>商务文件|技术文件|报价文件)(?:（[^）]{0,16}）)?(?:应包括)?')
_VOLUME_ITEM = re.compile(r'^[（(]\s*[0-9]+[）)]')
_VOLUME_CODE = re.compile(r'附件\s*([0-9０-９]+)\s*([A-Za-z]?)')
_METRIC = re.compile(r'万元|％|%')


def _join_wrapped_heading(rows, index):
    """Join a layout wrap. Stop once the line is already one complete file label."""
    parts = [rows[index][4]]
    if _ANNEX_ONLY.match(parts[0]):
        return parts[0], 1
    first_title = _single_form_title(parts[0])
    if first_title and not _unfinished_label(rows, index, first_title):
        return parts[0], 1
    step = 1
    while index + step < len(rows) and step < 4:
        if (rows[index + step][0] != rows[index][0]
                or rows[index + step][1] != rows[index][1]
                or rows[index + step][2] != rows[index][2] + step):
            break
        nxt = rows[index + step][4]
        prev = ''.join(parts)
        subcode_tail = (
            _SUBCODE_TITLE.match(nxt) is not None
            and not _FORMAT_ITEM.match(nxt)
            and not nxt.startswith('附件')
            and not _title_closed(prev)
            and not any(mark in prev for mark in '。；，：、')
            and not prev[:1].isdigit()
            and prev[:1] not in '（('
            and not _FORMAT_ITEM.match(prev)
            and len(prev) <= 24
            and _single_form_title(prev + nxt) is not None
        )
        if (not nxt or _chapter_boundary(nxt) or _format_heading(nxt)
                or _FORMAT_ITEM.match(nxt) or nxt.startswith('附件')
                or nxt[:1] in '0123456789（(注致' or '：' in nxt[:6]
                or (_single_form_title(nxt) and not subcode_tail)):
            break
        prev_title = _single_form_title(prev)
        if prev_title and not _unfinished_label(rows, index + step - 1, prev_title):
            break
        if prev.endswith(('。', '；')) or (_title_closed(prev) and not _unclosed(prev)):
            break
        if len(nxt) > 24 and not _unclosed(prev):
            break
        parts.append(nxt)
        step += 1
        joined_title = _single_form_title(''.join(parts))
        if joined_title and not _unfinished_label(rows, index + step - 1, joined_title):
            break
    return ''.join(parts), step


def _title_closed(text):
    return text.endswith(('表', '函', '书', '料', '式', '明', '议', '案', '件', '划', '）', ')'))


def _single_form_title(text):
    """A file name written on this line. A顿号 list after the label is a mapping, not one chapter."""
    if not text or _FORMAT_TOC.search(text):
        return None
    text = re.sub(r'[；;]+$', '', text.strip())
    if any(mark in text for mark in ('。', '；', '，', ';')):
        return None
    subcode = _SUBCODE_TITLE.match(text)
    if subcode and '、' not in subcode.group('name') and 1 < len(subcode.group('name')) <= 40:
        return subcode.group('name').strip()
    match = _FORMAT_ITEM.match(text)
    if match is None:
        return None
    rest = text[match.end():].strip(' \t：:')
    if not rest or '、' in rest or _ANNEX_ONLY.match(text):
        return None
    if rest.startswith(('（', '(')) and rest.endswith(('）', ')')):
        inner = rest[1:-1].strip()
        return inner if inner and '、' not in inner and '。' not in inner and len(inner) <= 30 else None
    core = _LABEL_NOTE.sub('', rest).strip()
    max_len = 45 if core.endswith(_COMPLETE_END) else 30
    if not core or '、' in core or _METRIC.search(core) or not 1 < len(core) <= max_len:
        return None
    return rest


def _line_code(text):
    """Annex or parenthetical form code. The code is a locator, not a title."""
    if not text:
        return None
    annex = _ANNEX_CODE.match(text.strip())
    if annex:
        return _digits(annex.group(1)), (annex.group(2) or '').upper()
    subcode = _SUBCODE_TITLE.match(re.sub(r'[；;]+$', '', text.strip()))
    if subcode:
        return _digits(subcode.group('num')), (subcode.group('letter') or '').upper()
    return None


def _digits(text):
    return int(text.translate(str.maketrans('０１２３４５６７８９', '0123456789')))


_TABLE_CAPTION = re.compile(r'表\s*(?P<name>[\u4e00-\u9fff]{2,20})\s*$')


def _annex_without_file_name(text):
    """附件号单独一行，或后面只有括号说明。文件名要到后文里找。"""
    if not text or not text.startswith('附件'):
        return False
    match = _ANNEX_CODE.match(text.strip())
    if match is None:
        return False
    rest = text.strip()[match.end():].strip()
    return not rest or (rest.startswith(('（', '(')) and rest.endswith(('）', ')')))


def _starts_annex(text):
    return bool(text) and text.startswith('附件') and _line_code(text) is not None


def _next_annex(section, index):
    while index < len(section) and not _starts_annex(section[index][4]):
        index += 1
    return index


def _resolve_annex_title(section, index, code):
    """Name a bare annex from its table caption or the words printed under the code."""
    end = _next_annex(section, index + 1)
    name, name_at = _following_file_name(section, index + 1)
    if (name and name_at is not None and name_at < end
            and not _TABLE_CAPTION.search(section[name_at][4] or '')):
        raw = section[name_at][4]
        if any(mark in raw for mark in '（(，,') or not _unfinished_label(section, name_at, name):
            return name, name_at
    return _table_caption(section, index, end, code)


def _table_caption(section, index, end, code):
    letter = code[1] if code else ''
    found = None
    for cursor in range(index + 1, end):
        text = section[cursor][4]
        match = _TABLE_CAPTION.search(text or '')
        if match is None:
            continue
        if letter and letter not in text:
            continue
        if not _usable_name(match.group('name')):
            continue
        found = match.group('name'), cursor
        break
    return found


def _usable_name(name):
    compact = ''.join(str(name).split())
    return len(compact) >= 2 and not compact.isdigit() and re.search(r'[\u4e00-\u9fff]', compact) is not None


def _name_on_next_line(text):
    """The file name printed under a bare annex code, before the instruction."""
    if not text or text.startswith('附件') or any(mark in text for mark in ('。', '；')):
        return None
    cut = re.split(r'[（(，,]', text, maxsplit=1)[0].strip()
    if cut and '、' not in cut and 1 < len(cut) <= 36:
        return cut
    return None


def _label_key(title):
    key = _LABEL_NOTE.sub('', title)
    if key.endswith('的格式'):
        key = key[:-3]
    elif key.endswith('格式'):
        key = key[:-2]
    return re.sub(r'\s+', '', key)


def _volume_name(text):
    """A bid-volume heading. The sentence that merely lists every volume is not one."""
    stripped = text.strip()
    if stripped.startswith('投标文件应包括') or stripped.startswith('第一卷'):
        return None
    match = _VOLUME_LINE.match(stripped)
    if match is None:
        return None
    if not stripped[:1].isdigit() and '应包括' not in stripped:
        return None
    return match.group('name')


def _bid_volumes(documents):
    """商务文件、技术文件、报价文件，只在招标文件自己这样分册时才建立。"""
    rows = _format_lines(documents)
    groups, current = [], None
    for row in rows:
        name = _volume_name(row[4])
        if name:
            current = {'name': name, 'point': (row[0], row[1], row[2]), 'names': [], 'codes': set()}
            groups.append(current)
            _take_volume_items(row[4], current)
            continue
        if current is None:
            continue
        if re.match(r'^[0-9]+\.[0-9]+(\.[0-9]+)?\s', row[4]) and _volume_name(row[4]) is None:
            current = None
            continue
        if _VOLUME_ITEM.match(row[4]):
            _take_volume_items(row[4], current)
    # A later, more specific list replaces an earlier mention of the same volume.
    kept = {}
    for group in groups:
        kept[group['name']] = group
    return [kept[name] for name in ('商务文件', '技术文件', '报价文件') if name in kept]


def _take_volume_items(text, group):
    for match in _VOLUME_CODE.finditer(text):
        group['codes'].add((_digits(match.group(1)), (match.group(2) or '').upper()))
    body = _VOLUME_ITEM.sub('', text)
    body = re.sub(r'[（(](?:选填|如有|含)[^）)]*[）)]', '', body)
    body = re.sub(r'[（(]附件[^）)]*[）)]', '', body)
    for part in re.split(r'[、；]', body):
        part = re.sub(r'^[0-9]+(?:\.[0-9]+)*\s*', '', part)
        part = re.sub(r'^(?:商务文件|技术文件|报价文件)(?:应包括)?\s*[：:]?', '', part)
        name = part.strip(' ：:，,。')
        if name and 1 < len(name) <= 40 and '应包括' not in name:
            group['names'].append(name)


def _norm_item(name):
    key = _label_key(name)
    return key.replace('规格', '').replace('条款', '')


def _volume_for(form, volumes):
    if isinstance(form.get('key'), tuple) and form['key'][0] == 'req':
        if any(word in form['name'] for word in ('报价', '价格')) and '偏差' not in form['name']:
            return next((volume for volume in volumes if volume['name'] == '报价文件'), volumes[-1])
        return next((volume for volume in volumes if volume['name'] == '技术文件'), volumes[0])
    code = form.get('code')
    key = _norm_item(form['name'])
    for volume in volumes:
        if code and (code in volume['codes'] or (code[0], '') in volume['codes']):
            return volume
        for name in volume['names']:
            other = _norm_item(name)
            if key == other or (len(other) >= 4 and (key.endswith(other) or other.endswith(key))):
                return volume
    if form['name'].startswith('商务'):
        found = next((volume for volume in volumes if volume['name'] == '商务文件'), None)
        if found is not None:
            return found
    if any(word in form['name'] for word in ('报价', '价格')) and '偏差' not in form['name']:
        found = next((volume for volume in volumes if volume['name'] == '报价文件'), None)
        if found is not None:
            return found
    if any(word in form['name'] for word in ('技术', '规格', '服务方案', '实施团队', '需求理解', '质量标准', '服务计划', '售后', '培训', '部署', '实施')):
        found = next((volume for volume in volumes if volume['name'] == '技术文件'), None)
        if found is not None:
            return found
    return next((volume for volume in volumes if volume['name'] == '商务文件'), None)


_RESPONSE_TOPIC = ('供货', '备件', '参数', '规格', '偏差', '售后', '培训', '服务', '包装', '运输', '验收', '试验', '环境', '调试', '实施', '部署', '集成', '应急', '运维', '需求', '团队', '质保', '交付', '进度', '考核', '监理', '预装')
_RESPONSE_SKIP = ('投标人', '招标人', '如果', '应', '须', '见', '指', '本规范', '本章', '万元')
_RESPONSE_HEAD = re.compile(
    r'^(?:(?P<dot>[0-9]+(?:\.[0-9]+)*)[\s、.．]+|(?P<cn>[一二三四五六七八九十]+)、|(?P<plain>[0-9]+)、)\s*(?P<title>\S.{1,24})$'
)
_SCHEDULE_HEAD = re.compile(r'^技术附表\s*[A-Z]\s*(?P<title>[\u4e00-\u9fff].{1,18})$')


def _response_heading(text):
    """A short tender heading the bid must answer. Requirement sentences stay in the body."""
    stripped = text.strip()
    if not stripped or len(stripped) > 36 or any(mark in stripped for mark in '。；，：:*★/') or re.search(r'\s{2,}', stripped):
        return None
    schedule = _SCHEDULE_HEAD.match(stripped)
    numbered = _RESPONSE_HEAD.match(stripped)
    if schedule:
        title, number = schedule.group('title').strip(), None
    elif numbered:
        title = numbered.group('title').strip()
        number = tuple(int(part) for part in numbered.group('dot').split('.')) if numbered.group('dot') else None
    else:
        return None
    if not any(topic in title for topic in _RESPONSE_TOPIC) or any(word in title for word in _RESPONSE_SKIP):
        return None
    if '、' in title or any(word in title for word in ('计算', '公式', '得分', '评审', '代理服务费')):
        return None
    if not 4 <= len(title) <= 22:
        return None
    return title, number


def _response_forms(documents, existing):
    """Chapters for technical and service headings that the format list does not already name."""
    rows = _format_lines(documents)
    found = []
    for row in rows:
        parsed = _response_heading(row[4])
        if parsed is None:
            continue
        title, number = parsed
        key = _label_key(title)
        if any(_label_key(form['name']) == key or (len(key) >= 4 and len(_label_key(form['name'])) >= 4 and (key.endswith(_label_key(form['name'])) or _label_key(form['name']).endswith(key))) for form in existing):
            continue
        point = (row[0], row[1], row[2])
        found.append({'title': title, 'number': number, 'point': point, 'row': row})
    forms = []
    for index, item in enumerate(found):
        stop = found[index + 1]['point'] if index + 1 < len(found) and found[index + 1]['point'][0] == item['point'][0] else None
        windows, begun = [], False
        for doc, page, line, _raw, _text in rows:
            if doc != item['point'][0]:
                continue
            current = (doc, page, line)
            if not begun:
                if current != item['point']:
                    continue
                begun = True
            if stop is not None and current == stop:
                break
            if _chapter_boundary(_text) and current != item['point']:
                break
            if windows and windows[-1]['page'] == page and windows[-1]['end_line'] + 1 == line:
                windows[-1]['end_line'] = line
            else:
                windows.append({'document_id': doc, 'page': page, 'start_line': line, 'end_line': line})
            if sum(window['end_line'] - window['start_line'] + 1 for window in windows) >= 40:
                break
        point = item['point']
        forms.append({
            'name': item['title'], 'key': ('req', point), 'point': point, 'order_point': point,
            'list': False, 'slot': point, 'letter': '', 'index': 100000 + index, 'mark': None,
            'source': item['row'][4], 'body_from': point, 'code': None, 'fixed_windows': windows,
            'number': item['number'],
        })
    return forms


def _ensure_volumes(documents, volumes):
    if volumes:
        return volumes
    point = None
    for row in _format_lines(documents):
        if '投标文件的组成' in row[4] or _format_heading(row[4]):
            point = (row[0], row[1], row[2])
            break
    if point is None:
        return []
    return [{'name': name, 'point': point, 'names': [], 'codes': set(), 'synthetic': True}
            for name in ('商务文件', '技术文件', '报价文件')]


def bid_format_section(documents):
    """Lines after the bid-format heading, before the next chapter. No semantic selection."""
    return _format_section(_format_lines(documents))


def _format_section(rows):
    anchor = _format_anchor(rows)
    if anchor is None:
        return None
    section = []
    for row in rows[anchor + 1:]:
        if _chapter_boundary(row[4]):
            break
        section.append(row)
    return rows[anchor], section


def bid_format_sections(documents):
    """BUG-332: one bid-format section per registered document of any role, in registered order."""
    found = []
    for document in documents or []:
        rows = [(document.get('document_id'), page, line, raw, raw.strip())
                for page, text in enumerate(document.get('pages') or [])
                for line, raw in enumerate(str(text).splitlines())]
        section = _format_section(rows)
        if section is not None:
            found.append((document.get('role') or 'main', *section))
    return found


def bid_format_plan(documents):
    """Chapters whose file name is already on one tender line.

    附件编号和顿号清单是映射，这里不拆、不猜。那种对应交给模型选择原行。
    """
    found = bid_format_section(documents)
    if found is None:
        return None
    _anchor, section = found
    forms, index, number_slot, slot_heading_lines = [], 0, {}, {}
    while index < len(section):
        text, step = _join_wrapped_heading(section, index)
        code = _line_code(section[index][4]) or _line_code(text)
        if code:
            slot = (section[index][0], section[index][1], section[index][2])
            if code[0] not in number_slot:
                number_slot[code[0]] = slot
                slot_heading_lines[slot] = step
        if _annex_without_file_name(section[index][4]):
            resolved = _resolve_annex_title(section, index, code)
            if resolved:
                name, name_at = resolved
                forms.append(_form(name, code, section[name_at], index, number_slot,
                                   list_item=False, body_from=(section[index][0], section[index][1], section[index][2]),
                                   slot_heading_lines=slot_heading_lines))
                index = _next_annex(section, index + 1)
                continue
        title = _single_form_title(text)
        if title and (_in_metric_run(section, index) or _unfinished_label(section, index + step - 1, title)):
            title = None
        if title and step > 1 and _FORMAT_ITEM.match(text) and not _unclosed(section[index][4]):
            if not _LABEL_NOTE.sub('', title).strip().endswith(_COMPLETE_END):
                title = None
        row = section[index]
        if title:
            forms.append(_form(title, code, row, index, number_slot,
                               list_item=_list_heading(section, index + step),
                               heading_lines=step, source_text=text,
                               slot_heading_lines=slot_heading_lines))
        index += step
    if not forms:
        return None
    chosen, order = {}, []
    for form in forms:
        prior = next((key for key, existing in chosen.items() if _same_listed_form(existing, form)), None)
        if prior is None:
            order.append(form['key'])
            chosen[form['key']] = form
        else:
            chosen[prior] = _prefer_form(chosen[prior], form)
    order.sort(key=lambda key: (chosen[key]['slot'], chosen[key]['index']))
    volumes = _ensure_volumes(documents, _bid_volumes(documents))
    for extra in _response_forms(documents, chosen.values()):
        order.append(extra['key'])
        chosen[extra['key']] = extra
    if volumes:
        for key in order:
            volume = _volume_for(chosen[key], volumes)
            if volume is not None:
                chosen[key]['volume'] = volume
    by_position = sorted(chosen.values(), key=_form_start)
    stops = {id(form): _form_start(by_position[item + 1]) if item + 1 < len(by_position) else None
             for item, form in enumerate(by_position)}
    nodes, entries, adopted = [], [], []
    volume_ids = {}
    for volume in volumes:
        identity = f'volume-{volume["name"]}'
        volume_ids[volume['name']] = identity
        point = {'document_id': volume['point'][0], 'page': volume['point'][1], 'line': volume['point'][2]}
        windows = [] if volume.get('synthetic') else [{'document_id': point['document_id'], 'page': point['page'],
                                                       'start_line': point['line'], 'end_line': point['line']}]
        nodes.append({'id': identity, 'heading': point, 'parent_id': None, 'response_kind': 'form',
                      'name': volume['name'], 'body_windows': windows})
        entries.append({'name': point})
        adopted.append({'node_id': identity, 'entry': len(entries) - 1, 'depth': 'self', 'presentation': 'section', 'clause': []})
    number_ids = {}
    for key in order:
        form = chosen[key]
        document_id, page, line = form['point']
        identity = f'format-{len(nodes) + 1}'
        point = {'document_id': document_id, 'page': page, 'line': line}
        number = form.get('number')
        parent_number = number[:-1] if number and len(number) > 1 else None
        parent = number_ids.get(parent_number) if parent_number in number_ids else volume_ids.get((form.get('volume') or {}).get('name'))
        if number:
            number_ids[number] = identity
        node = {'id': identity, 'heading': point, 'parent_id': parent, 'response_kind': 'form', 'name': form['name']}
        if form.get('heading_lines', 1) > 1:
            node['heading_lines'] = form['heading_lines']
        node['body_windows'] = form['fixed_windows'] if form.get('fixed_windows') is not None else _form_windows(section, form, stops.get(id(form)))
        nodes.append(node)
        order_id, order_page, order_line = form['order_point']
        entry = {'name': {'document_id': order_id, 'page': order_page, 'line': order_line}}
        if form.get('order_heading_lines', 1) > 1:
            entry['heading_lines'] = form['order_heading_lines']
        entries.append(entry)
        adopted.append({'node_id': identity, 'entry': len(entries) - 1, 'depth': 'self', 'presentation': 'section', 'clause': []})
    return {'nodes': nodes,
            'composition': {'entries': entries},
            'scopes': [{'items': adopted}], 'excluded': [], 'source_only': []}


def _form_start(form):
    return form.get('body_from') or form['point']


def _form_windows(section, form, stop):
    """Copy this form until the next form. A format-list line stays one heading."""
    document_id, page, line = form['point']
    if form['list']:
        return [{'document_id': document_id, 'page': page,
                 'start_line': line, 'end_line': line + form.get('heading_lines', 1) - 1}]
    start = _form_start(form)
    windows, begun = [], False
    for doc, number, index, _raw, _text in section:
        point = (doc, number, index)
        if not begun:
            if point != start:
                continue
            begun = True
        if stop is not None and point == stop:
            break
        if windows and windows[-1]['document_id'] == doc and windows[-1]['page'] == number and windows[-1]['end_line'] + 1 == index:
            windows[-1]['end_line'] = index
        else:
            windows.append({'document_id': doc, 'page': number, 'start_line': index, 'end_line': index})
    return windows


def _enum_mark(text):
    match = _FORMAT_ITEM.match(text or '')
    return match.group('label') if match is not None and match.group('label') else None


def _form(name, code, row, index, number_slot, *, list_item, body_from=None,
          heading_lines=1, source_text=None, slot_heading_lines=None):
    key = code if code else ('name', _label_key(name))
    point = (row[0], row[1], row[2])
    # The first time this annex number appears fixes the outline order. Subforms stay with it.
    slot = number_slot.get(code[0], point) if code else point
    order_heading_lines = (slot_heading_lines or {}).get(slot, heading_lines) if slot != point else heading_lines
    return {'name': name, 'key': key, 'point': point, 'order_point': slot, 'list': list_item,
            'slot': slot, 'letter': code[1] if code else '', 'index': index,
            'mark': _enum_mark(row[4]), 'source': source_text if source_text is not None else row[4],
            'body_from': body_from or point,
            'heading_lines': heading_lines, 'order_heading_lines': order_heading_lines}


def _same_listed_form(old, new):
    """目录「八、技术性能指标的详细描述」和正文「八、投标产品技术性能指标的详细描述」是同一章。"""
    if old['key'] == new['key']:
        return True
    if not old.get('mark') or old['mark'] != new.get('mark'):
        return False
    short, long = sorted((_label_key(old['name']), _label_key(new['name'])), key=len)
    return len(short) >= 8 and long.endswith(short)


def _prefer_form(old, new):
    """The format list fixes the order. The later template, when there is one, holds the body."""
    if old['list'] and not new['list']:
        chosen = new
    elif new['list'] and not old['list']:
        chosen = old
    else:
        chosen = new if new['index'] >= old['index'] else old
    earlier = old if old['index'] <= new['index'] else new
    name = _shorter_listed_name(old, new, chosen)
    return {**chosen, 'name': name, 'order_point': earlier['order_point'],
            'order_heading_lines': earlier.get('order_heading_lines', 1),
            'slot': earlier['slot'], 'index': earlier['index']}


def _shorter_listed_name(old, new, chosen):
    ranked = sorted((old, new), key=lambda form: len(_label_key(form['name'])))
    short = ranked[0]['name']
    if (_label_key(ranked[1]['name']).endswith(_label_key(short)) and len(_label_key(short)) >= 8
            and _compact(short) in _compact(chosen.get('source', ''))):
        return short
    return chosen['name']


_COMPLETE_END = ('文件', '资料', '证明', '方案', '计划', '声明', '简介', '情况', '内容', '描述', '理解', '附录', '函', '表', '书', '）', ')')


def _unfinished_label(section, index, title):
    """A long label that still continues on the next line is not a finished file name."""
    core = _LABEL_NOTE.sub('', title).strip()
    if len(core) <= 12 or core.endswith(_COMPLETE_END):
        return False
    cursor = index + 1
    while cursor < len(section) and not section[cursor][4]:
        cursor += 1
    if cursor >= len(section):
        return False
    nxt = section[cursor][4]
    if _FORMAT_ITEM.match(nxt) or nxt.startswith('附件') or nxt[:1] in '0123456789（(注致':
        return False
    return '：' not in nxt[:6]


def _in_metric_run(section, index):
    """一、注册资金万元这一类连排指标不是投标文件。"""
    if not _enum_label(section[index][4]):
        return False
    group = []
    for step in (-1, 1):
        cursor = index
        while True:
            cursor += step
            if cursor < 0 or cursor >= len(section):
                break
            if not section[cursor][4]:
                continue
            if not _enum_label(section[cursor][4]):
                break
            group.append(cursor)
    group.append(index)
    return any(_METRIC.search(section[item][4]) for item in group)


def _enum_label(text):
    match = _FORMAT_ITEM.match(text or '')
    return match is not None and match.group('label')


def _following_file_name(section, index):
    while index < len(section) and not section[index][4]:
        index += 1
    if index >= len(section):
        return None, None
    return _name_on_next_line(section[index][4]), index


def _list_heading(section, index):
    """A heading packed against the next label is a list entry, not the form body."""
    while index < len(section) and not section[index][4]:
        index += 1
    return index < len(section) and _single_form_title(section[index][4]) is not None


def compile_plans(plans, documents, *, format_candidates=False):
    """Compile at most two plans. An illegal plan yields no directory."""
    if len(plans) > 2:
        return _empty('目录对照只接受两份计划，多出的计划不会参与表决')
    if not plans:
        return _empty('没有目录计划')
    compiled, rejected = [], []
    for index, plan in enumerate(plans, start=1):
        try:
            compiled.append(compile_directory(plan, documents, format_candidates=format_candidates))
        except DirectoryPlanRejected as error:
            rejected.append({'code': 'outline_directory_rejected', 'detail': f'计划{index}：{error.detail}'})
    if not compiled:
        chosen = {'chapters': [], 'issues': rejected}
    elif len(compiled) == 1:
        chosen = {**compiled[0], 'issues': [*rejected, *compiled[0]['issues']]}
    else:
        chosen = _choose(compiled[0], compiled[1], rejected, _Source(documents))
    if any('_reading_references' in value for value in compiled):
        chosen['_reading_references'] = []
        seen = set()
        for value in compiled:
            for point in value.get('_reading_references', []):
                origin = point['document_id'], point['page'], point['line']
                if origin not in seen:
                    chosen['_reading_references'].append(deepcopy(point))
                    seen.add(origin)
    return _note_conditions(chosen)


def _note_conditions(compiled):
    seen_details = {value.get('detail') for value in compiled['issues'] if value.get('code') == 'outline_directory_applicability_unconfirmed'}
    for chapter in compiled['chapters']:
        if chapter.get('condition'):
            detail = f'适用性待人工核定：{chapter["title"]}；{chapter["condition"]}'
            if detail not in seen_details:
                compiled['issues'].append({'code': 'outline_directory_applicability_unconfirmed', 'detail': detail})
                seen_details.add(detail)
    return compiled


def soften_directory_plan(plan):
    """Drop one adoption whose depth or presentation is not in the closed set.

    One invalid item must not discard the rest of the plan.
    """
    if not isinstance(plan, dict):
        return plan, []
    plan = {**plan}
    issues = []
    scopes = []
    for scope in plan.get('scopes') or []:
        if not isinstance(scope, dict):
            scopes.append(scope)
            continue
        scope = {**scope}
        kept = []
        for item in scope.get('items') or []:
            if isinstance(item, dict) and (item.get('depth') not in _DEPTHS or item.get('presentation') not in _PRESENTATIONS):
                issues.append({'code': 'outline_directory_extra', 'detail': '一条采用项的深度或呈现方式无效，已跳过'})
                continue
            kept.append(item)
        scope['items'] = kept
        scopes.append(scope)
    if 'scopes' in plan:
        plan['scopes'] = scopes
    return plan, issues


def compile_directory(plan, documents, *, format_candidates=False):
    plan, skipped = soften_directory_plan(plan)
    skipped = [*plan.pop('skipped_item_issues', []), *skipped]
    _keys(plan, _PLAN, 'directory plan')
    source = _Source(documents)
    nodes = _nodes(plan, source)
    reading = _reading_references(plan, source, nodes)
    composition = _composition(plan, source, format_candidates=format_candidates)
    parents = _parents(nodes, source)
    excluded = _excluded(plan, nodes, parents, source, composition)
    adopted = _adopt(plan, source, nodes, parents, composition, excluded)
    _decisions(nodes, adopted['seen'], excluded['ids'], plan.get('source_only', []))
    _validate_body_owners(adopted['chapters'])
    _attach_template_parts(adopted['chapters'], source, adopted['issues'])
    _place_continuation_grids(adopted['chapters'], source)
    _place_wrapped_grids(adopted['chapters'], source, adopted['issues'])
    _segment_template_content(adopted['chapters'], source)
    binding_issues = _apply_format_bindings(plan, source, adopted['chapters'])
    used_pages = {
        (ref['document_id'], ref['page'])
        for chapter in adopted['chapters']
        for ref in chapter.get('lines') or ()
    }
    source_issues = []
    for document in source.documents:
        for key in ('ocr_issues', 'source_review_issues'):
            for issue in document[key]:
                if issue.get('code') == 'outline_directory_literal_pending' and used_pages:
                    match = re.search(r'第(\d+)页', issue.get('detail', ''))
                    if match and (document['document_id'], int(match.group(1)) - 1) not in used_pages:
                        continue
                source_issues.append(issue)
    compiled = {'chapters': adopted['chapters'],
            'issues': [*adopted['issues'], *excluded['issues'], *binding_issues, *skipped,
                       *_revision_issues(source, composition['priority_basis']), *source_issues]}
    if 'reading_only_sources' in plan:
        compiled['_reading_references'] = reading
    return _note_conditions(compiled)


def _reading_references(plan, source, nodes):
    reading = plan.get('reading_only_sources', [])
    if not isinstance(reading, list):
        raise DirectoryPlanRejected('reading_only_sources must be source points')
    seen, headings = set(), {node['heading'] for node in nodes.values()}
    for index, value in enumerate(reading):
        label = f'reading_only_sources[{index}]'
        if not isinstance(value, dict) or any(type(value.get(key)) is not int for key in ('page', 'line')):
            raise DirectoryPlanRejected(f'{label} must be a source line')
        point = _point(value, source, label)
        if point in seen or point in headings:
            raise DirectoryPlanRejected(f'{label} must be unique and cannot select a declared node')
        if not source.line(*point).strip():
            raise DirectoryPlanRejected(f'{label} must select nonempty source text')
        seen.add(point)
    return deepcopy(reading)


class _Source:
    def __init__(self, documents):
        if not isinstance(documents, list) or not documents:
            raise DirectoryPlanRejected('directory source requires the registered documents')
        self.documents, self.by_id = [], {}
        for index, document in enumerate(documents):
            _keys(document, frozenset(('document_id', 'role', 'pages', 'line_units', 'template_parts',
                                       'ocr_issues', 'source_review_issues', 'unread_pages', 'grids',
                                       'continuations', 'layout_decoration', 'body_projection')), 'source document')
            identity, role, pages = document.get('document_id'), document.get('role'), document.get('pages')
            if not isinstance(identity, str) or not identity or identity in self.by_id:
                raise DirectoryPlanRejected(f'duplicate or empty document {identity}')
            if role not in _ROLES:
                raise DirectoryPlanRejected(f'document {identity} has an unknown role')
            if not isinstance(pages, list) or not all(isinstance(page, str) for page in pages):
                raise DirectoryPlanRejected(f'document {identity} has no source pages')
            record = {'document_id': identity, 'role': role, 'pages': pages, 'index': index,
                      'line_units': document.get('line_units') or [],
                      'template_parts': document.get('template_parts') or {},
                      'grids': document.get('grids') or [],
                      'ocr_issues': document.get('ocr_issues') or [],
                      'source_review_issues': document.get('source_review_issues') or [],
                      'continuations': list(document.get('continuations') or []),
                      'layout_decoration': document.get('layout_decoration') or [],
                      'body_projection': list(document.get('body_projection') or [])}
            self.documents.append(record)
            self.by_id[identity] = record

    def line(self, document_id, page, line):
        document = self._document(document_id)
        if page < 0 or page >= len(document['pages']):
            raise DirectoryPlanRejected('source line is outside the document')
        lines = document['pages'][page].splitlines()
        if line < 0 or line >= len(lines):
            raise DirectoryPlanRejected('source line is outside the document')
        shown = lines[line]
        for item in document.get('body_projection') or []:
            if item.get('page') == page and item.get('line') == line and isinstance(item.get('text'), str):
                shown = item['text']
        return shown

    def pages(self, document_id):
        return self._document(document_id)['pages']

    def unit_at(self, document_id, page, line):
        owners = self._document(document_id).get('line_units') or []
        if page < 0 or page >= len(owners) or line < 0 or line >= len(owners[page]):
            return None
        return owners[page][line]

    def template_part(self, document_id, unit_id):
        return (self._document(document_id).get('template_parts') or {}).get(unit_id)

    def position(self, document_id, page, line):
        return self._document(document_id)['index'], page, line

    def _document(self, document_id):
        try:
            return self.by_id[document_id]
        except KeyError:
            raise DirectoryPlanRejected(f'unknown document {document_id}') from None


def _comparison_body(chapter, source):
    # Keep coordinates and unit-owned blanks; only empty prose lines are noise.
    refs = [ref for ref in chapter.get('lines', [])
            if ref.get('unit_id') or source.unit_at(ref['document_id'], ref['page'], ref['line'])
            or source.line(ref['document_id'], ref['page'], ref['line']).strip()]
    text = [line for line in chapter.get('text', '').splitlines() if line.strip()]
    return text, refs


def _choose(first, second, rejected, source):
    by_a = {_heading(chapter): chapter for chapter in first['chapters']}
    by_b = {_heading(chapter): chapter for chapter in second['chapters']}
    ids_a = {chapter['id']: chapter for chapter in first['chapters']}
    ids_b = {chapter['id']: chapter for chapter in second['chapters']}

    def structure(chapter, chapters, identities):
        return {'title': chapter['title'], 'parent_id': _parent_heading(chapter, chapters),
                'presentation': chapter['presentation'], 'group': _effective_group(chapter, identities),
                'group_heading': _effective_group(chapter, identities, 'group_heading'),
                'response_kind': chapter.get('response_kind', 'form'), 'subjects': chapter.get('subjects', [])}

    shared = by_a.keys() & by_b.keys()
    conflicts, text_diffs, differences = [], [], {}
    for key in shared:
        left, right = by_a[key], by_b[key]
        first_fields, second_fields = structure(left, by_a, ids_a), structure(right, by_b, ids_b)
        changed = {field: (value, second_fields[field]) for field, value in first_fields.items()
                   if value != second_fields[field]}
        structural = bool(changed)
        if _comparison_body(left, source) != _comparison_body(right, source):
            changed['body'] = (left.get('body_windows') or left.get('lines'),
                               right.get('body_windows') or right.get('lines'))
        for field in ('condition', 'obligations', 'obligation_lines'):
            if left.get(field) != right.get(field):
                changed[field] = left.get(field), right.get(field)
        differences[key] = changed
        if structural:
            conflicts.append(key)
        elif changed:
            text_diffs.append(key)
    if not conflicts and set(by_b) <= set(by_a):
        chosen = _superset(first, by_a.keys() - by_b.keys(), rejected)
    elif not conflicts and not text_diffs and set(by_a) <= set(by_b):
        chosen = _superset(second, by_b.keys() - by_a.keys(), rejected)
    else:
        chapters = list(first['chapters'])
        issues = [*rejected, *first['issues']]
        identified = {chapter['id']: chapter for chapter in second['chapters']}
        extra_keys = sorted(set(by_b) - set(by_a))
        omitted = {}
        extra_ids, used_ids = {}, {chapter['id'] for chapter in chapters}
        for key in extra_keys:
            original = identity = by_b[key]['id']
            while identity in used_ids:
                identity += ':extra'
            extra_ids[original] = identity
            used_ids.add(identity)
            for owned in chapters:
                try:
                    _validate_body_owners([owned, by_b[key]])
                except DirectoryPlanRejected as error:
                    omitted[original] = owned, error.detail
                    break
        for key in sorted(conflicts):
            issues.append({'code': 'outline_directory_conflict',
                           'detail': _conflict_detail(by_a[key], by_b[key], by_a, by_b, differences[key])})
        for key in extra_keys:
            extra = {**by_b[key]}
            blocked = extra['id']
            while blocked in identified and blocked not in omitted:
                blocked = identified[blocked].get('parent_id')
            if blocked in omitted:
                owned, reason = omitted[blocked]
                ranges = '；'.join(f'{document_id} page {page} lines {start}-{end}'
                                  for document_id, page, start, end in extra.get('body_windows') or [])
                if not ranges:
                    heading = extra['heading']
                    ranges = f'{heading["document_id"]} page {heading["page"]} line {heading["line"]}（仅标题）'
                overlap = f'候选“{identified[blocked]["title"]}”与已承载章“{owned["title"]}”正文重叠'
                parent = '；所属候选父章未并入，子章不推断改挂' if blocked != extra['id'] else ''
                issues.append({'code': 'outline_directory_extra_unconfirmed',
                               'detail': f'另一份计划“{extra["title"]}”未并入；{overlap}{parent}；'
                                         f'完整候选范围（如有独有部分也不裁剪）保留原计划：{ranges}；{reason}；待人工确认'})
                continue
            extra['id'] = extra_ids[extra['id']]
            parent = identified.get(extra.get('parent_id'))
            parent_key = _heading(parent) if parent is not None else None
            if parent_key in by_a and parent_key not in conflicts:
                extra['parent_id'] = by_a[parent_key]['id']
                issues.append({'code': 'outline_directory_extra',
                               'detail': f'另一份计划多出“{extra["title"]}”，已挂在已一致的父章下'})
            elif extra.get('parent_id') in extra_ids:
                extra['parent_id'] = extra_ids[extra['parent_id']]
                issues.append({'code': 'outline_directory_extra',
                               'detail': f'另一份计划多出“{extra["title"]}”，已挂在同一份计划的父章下'})
            else:
                extra['parent_id'] = None
                issues.append({'code': 'outline_directory_extra_unconfirmed',
                               'detail': f'另一份计划多出“{extra["title"]}”，未并入已确定的父子关系，请人工确认'})
            chapters.append(extra)
        chosen = {'chapters': chapters, 'issues': issues}
    for key in sorted(text_diffs):
        chosen['issues'].append({'code': 'outline_directory_conflict',
                                 'detail': _conflict_detail(by_a[key], by_b[key], by_a, by_b, differences[key])})
    chosen['_directory_candidates'] = deepcopy([first['chapters'], second['chapters']])
    return chosen


def _superset(chosen, missing, rejected):
    issues = [*rejected, *chosen['issues']]
    if missing:
        titles = '、'.join(f'“{chapter["title"]}”' for chapter in chosen['chapters'] if _heading(chapter) in missing)
        issues.append({'code': 'outline_directory_gap',
                       'detail': f'另一份计划未包含{titles}，已采用更完整的一份；两份都漏掉的内容不会被发现'})
    return {'chapters': chosen['chapters'], 'issues': issues}


def _validate_body_owners(chapters):
    owners, conflicts = {}, {}
    for chapter in chapters:
        if chapter.get('body_windows') is None:
            continue
        entry = chapter.get('entry')
        entry_key = (entry['document_id'], entry['page'], entry['line']) if entry else None
        for line in chapter.get('lines', []):
            point = line['document_id'], line['page'], line['line']
            prior = owners.setdefault(point, {})
            groups = prior.values() if entry_key is None else (prior.get(None, ()), prior.get(entry_key, ()))
            for previous_ids in groups:
                for previous_id in previous_ids:
                    conflicts.setdefault((previous_id, chapter['id']), point)
            prior.setdefault(entry_key, []).append(chapter['id'])
    if conflicts:
        detail = '; '.join(f'nodes {previous} and {current} overlap at {point[0]} page {point[1]} line {point[2]}'
                           for (previous, current), point in conflicts.items())
        raise DirectoryPlanRejected('duplicate body ownership: ' + detail +
                                    '; use one node with subjects, or distinct source submission entries')


def _ordered_scopes(plan, source, composition):
    scopes = list(plan.get('scopes', []))
    titles = []
    for scope in scopes:
        _keys(scope, _SCOPE, 'scope')
        if scope.get('title') is not None:
            titles.append(_point(scope['title'], source, 'scope title'))
    if composition['priority_basis'] or len({point[0] for point in titles}) != 1:
        return scopes

    def barrier(scope):
        return scope.get('title') is None or any(item.get('presentation') == 'cover'
                                               for item in scope.get('items', []))

    start = 0
    while start < len(scopes):
        if barrier(scopes[start]):
            start += 1
            continue
        end = start + 1
        while end < len(scopes) and not barrier(scopes[end]):
            end += 1
        scopes[start:end] = sorted(scopes[start:end], key=lambda scope: (scope['title']['page'], scope['title']['line']))
        start = end
    return scopes


def _adopt(plan, source, nodes, parents, composition, excluded):
    chapters, issues, seen = [], [], set()
    for node_id in plan.get('source_only', []):
        if node_id not in nodes:
            raise DirectoryPlanRejected(f'unknown source_only node {node_id}')
    for scope in _ordered_scopes(plan, source, composition):
        group = _scope_title(scope, source)
        group_heading = _point_dict(_point(scope['title'], source, 'scope title')) if scope.get('title') else None
        ordered, repeated = _ordered_items(scope, composition)
        issues.extend(repeated)
        for item in ordered:
            first = len(chapters)
            node_id = item.get('node_id')
            _adopt_item(item, parents.get(node_id) if node_id else None, group, source, nodes, parents,
                        composition, excluded['ids'], chapters, issues, seen, root=True, group_heading=group_heading)
            for chapter in chapters[first:]:
                if chapter.get('group') is not None and group_heading is not None:
                    chapter['group_heading'] = deepcopy(group_heading)
    return {'chapters': chapters, 'issues': issues, 'seen': seen}


def _adopt_item(item, parent_id, group, source, nodes, parents, composition, excluded_ids, chapters, issues, seen, *, root, group_heading=None):
    _keys(item, _ITEM, 'adoption')
    depth, presentation = item.get('depth'), item.get('presentation')
    if depth not in _DEPTHS or presentation not in _PRESENTATIONS:
        raise DirectoryPlanRejected('adoption depth or presentation is invalid')
    node_id = item.get('node_id')
    if presentation in (DirectoryPresentation.CONTENT, DirectoryPresentation.COVER) and depth != DirectoryDepth.SELF:
        raise DirectoryPlanRejected(f'{presentation} cannot have child chapters')
    if node_id is None:
        if item.get('entry') is None:
            issues.append({'code': 'outline_directory_extra', 'detail': '一条采用项没有来源节点，也没有组成条目，已跳过'})
            return
        _adopt_clause(item, parent_id, group if root else None, source, composition, chapters, seen)
        return
    if node_id in seen:
        chapter = next((chapter for chapter in chapters if chapter['id'] == node_id), {})
        condition = _condition(item, source)
        covered = {(ref['document_id'], ref['page'], ref['line']) for ref in chapter.get('lines', [])}
        clause_lines = []
        clause = _obligations(item, None, source, covered, source_lines=clause_lines)
        title = nodes.get(node_id, {}).get('title') or node_id
        if condition:
            if chapter and not chapter.get('condition'):
                chapter['condition'] = condition
                chapter['condition_windows'] = [deepcopy(item['condition'])]
            elif condition != chapter.get('condition'):
                issues.append({'code': 'outline_directory_conflict',
                               'detail': f'来源节点“{title}”重复采用的适用条件待核；保留先值：'
                                         f'{chapter.get("condition") or "未选择"}；后选条件：{condition}'})
        if clause and clause != chapter.get('obligations', ''):
            issues.append({'code': 'outline_directory_conflict',
                           'detail': f'来源节点“{title}”重复采用的补充义务待核，未自动合并：{clause}'})
        issues.append({'code': 'outline_directory_name',
                       'detail': f'来源节点“{nodes.get(node_id, {}).get("title") or node_id}”被重复采用，保留先出现的位置'})
        return
    if node_id in excluded_ids:
        raise DirectoryPlanRejected(f'source node {node_id} is both adopted and excluded')
    if node_id not in nodes:
        raise DirectoryPlanRejected(f'unknown adopted node {node_id}')
    if depth == 'subtree':
        _push_node(item, node_id, parent_id, group if root else None, source, nodes, composition, chapters, issues, seen)
        for child in _children(node_id, parents, nodes, excluded_ids):
            _adopt_item({**item, 'node_id': child, 'entry': None, 'clause': [], 'condition': None, 'depth': 'subtree'},
                        node_id, None, source, nodes, parents, composition, excluded_ids, chapters, issues, seen, root=False)
        return
    folded = [node_id]
    for folded_id in folded:
        if folded_id in seen:
            issues.append({'code': 'outline_directory_name',
                           'detail': f'来源节点“{nodes[folded_id]["title"]}”已包含在先出现的章节中，不重复单列'})
            continue
        if folded_id in excluded_ids and folded_id != node_id:
            continue
        seen.add(folded_id)
    parts, copied = [], []
    for identity in folded:
        if identity not in seen:
            continue
        part, lines = _body(nodes[identity], source, nodes)
        if part:
            parts.append(part)
        copied.extend(lines)
    _push_chapter(item, nodes[node_id], parent_id, group if root else None, '\n'.join(parts), copied,
                  source, composition, chapters, issues)
    if depth == 'children':
        for child in _children(node_id, parents, nodes, excluded_ids):
            _adopt_item({'node_id': child, 'entry': None, 'depth': 'self', 'presentation': 'section',
                         'condition': None, 'clause': []}, node_id, None, source, nodes, parents, composition,
                        excluded_ids, chapters, issues, seen, root=False)


def _adopt_clause(item, parent_id, group, source, composition, chapters, seen):
    entry = item.get('entry')
    if entry is None:
        raise DirectoryPlanRejected('a clause item needs a composition entry')
    record = composition['entries'][entry]
    identity = f'clause-{entry + 1}'
    if identity in seen:
        raise DirectoryPlanRejected(f'composition entry {entry} is used twice')
    seen.add(identity)
    text = _obligations(item, record, source)
    chapters.append(_chapter(identity, record['title'], parent_id, item['presentation'], record['name'], record['name'],
                             text, text, _condition(item, source), group))
    chapters[-1]['condition_windows'] = [deepcopy(item['condition'])] if item.get('condition') else []
    chapters[-1]['clause_windows'] = deepcopy(item.get('clause', []))


def _push_node(item, node_id, parent_id, group, source, nodes, composition, chapters, issues, seen):
    if node_id in seen:
        issues.append({'code': 'outline_directory_name',
                       'detail': f'来源节点“{nodes[node_id]["title"]}”被重复采用，保留先出现的位置'})
        return
    seen.add(node_id)
    node = nodes[node_id]
    text, lines = _body(node, source, nodes)
    _push_chapter(item, node, parent_id, group, text, lines, source, composition, chapters, issues)


def _push_chapter(item, node, parent_id, group, text, lines, source, composition, chapters, issues):
    title = node['title']
    entry = item.get('entry')
    entry_point = None
    if entry is not None:
        record = composition['entries'][entry]
        entry_point = record['name']
        listed = record['title']
        if _compact(title) != _compact(listed):
            if _compact(title) in _compact(listed):
                pass
            elif _compact(listed) in _compact(title):
                stripped = _FORMAT_ITEM.sub('', title.strip(), count=1).strip()
                stripped = re.sub(r'^[（(][一二三四五六七八九十百千]+[）)]\s*', '', stripped).strip()
                if _compact(stripped) != _compact(listed):
                    title = listed
            else:
                issues.append({'code': 'outline_directory_name',
                               'detail': f'章节名称与组成清单不一致，请人工确认：采用原式标题“{title}”，投标文件组成写作“{listed}”'})
    elif _number_only(title):
        issues.append({'code': 'outline_directory_name_unconfirmed',
                       'detail': f'章节标题只有编号，未能从组成清单确认名称，暂用编号，请人工确认：{title}'})
    covered = ({(ref['document_id'], ref['page'], ref['line']) for ref in lines}
               if node.get('body_windows') is not None else set())
    # The composition name locates this instance; explicit windows define its extra obligations.
    obligation_lines, obligation_rows = [], []
    obligations = _obligations(item, None, source, covered, source_lines=obligation_lines,
                               source_rows=obligation_rows)
    chapter = _chapter(node['id'], title, parent_id, item['presentation'], node['heading'], entry_point,
                       text, obligations, _condition(item, source), group, lines)
    chapter['body_windows'] = node.get('body_windows')
    chapter['response_kind'] = node.get('response_kind', 'form')
    chapter['subjects'] = node.get('subjects', [])
    chapter['condition_windows'] = [deepcopy(item['condition'])] if item.get('condition') else []
    chapter['clause_windows'] = deepcopy(item.get('clause', []))
    chapter['_source_body_rows'] = _source_rows(source, lines)
    chapter['_source_obligation_rows'] = obligation_rows
    chapter['_source_condition_rows'] = _source_rows(source, [dict(document_id=d, page=p, line=number)
        for raw in chapter['condition_windows'] for d, p, start, end in [_window(raw, source, 'source condition')]
        for number in range(start, end + 1)])
    if obligation_lines and obligations.strip() != text.strip():
        chapter['obligation_lines'] = obligation_lines
    if entry is not None:
        chapter['listed_title'] = listed
        chapter['heading_title'] = node['title']
    chapters.append(chapter)


def _apply_format_bindings(plan, source, chapters):
    """Link an appendix or alias only where the plan cites the source line and the node ids."""
    known = {chapter['id']: chapter for chapter in chapters}
    issues, seen = [], set()
    for raw in plan.get('format_bindings') or []:
        _keys(raw, _BINDING, 'format binding')
        identity = raw.get('id')
        if not isinstance(identity, str) or not identity or identity in seen:
            issues.append({'code': 'outline_directory_binding_rejected', 'detail': '对应关系编号缺失或重复，未采用'})
            continue
        seen.add(identity)
        try:
            window = _window(raw.get('source'), source, f'format binding {identity}')
        except DirectoryPlanRejected as error:
            issues.append({'code': 'outline_directory_binding_rejected', 'detail': error.detail})
            continue
        response = raw.get('response_node_id')
        references = raw.get('reference_node_ids') or []
        if response not in known or not isinstance(references, list) or response in references or any(
                not isinstance(item, str) or item not in known for item in references):
            issues.append({'code': 'outline_directory_binding_rejected',
                           'detail': f'对应关系“{identity}”没有落到已采用的章节，未采用'})
            continue
        basis = _window_text(source, window)
        known[response].setdefault('format_bindings', []).append(
            {'id': identity, 'source': basis, 'reference_ids': list(references)})
        referred = '、'.join(known[item]['title'] for item in references)
        suffix = f'；引用章节：{referred}' if referred else ''
        issues.append({'code': 'outline_directory_binding',
                       'detail': f'「{known[response]["title"]}」对应原文「{basis}」{suffix}'})
    return issues


def _chapter(identity, title, parent_id, presentation, heading, entry, text, obligations, condition, group, lines=()):
    return {'id': identity, 'title': title, 'parent_id': parent_id, 'presentation': presentation,
            'heading': _point_dict(heading), 'entry': _point_dict(entry), 'text': text,
            'obligations': obligations, 'condition': condition, 'group': group, 'lines': list(lines)}


def _source_rows(source, refs, *, trim=False):
    rows = [{**deepcopy(ref), 'text': source.line(ref['document_id'], ref['page'], ref['line'])} for ref in refs]
    if trim:
        text = '\n'.join(row['text'] for row in rows)
        first, last, offset = len(text) - len(text.lstrip()), len(text.rstrip()), 0
        for row in rows:
            size = len(row['text'])
            start = min(size, max(0, first - offset))
            end = max(start, min(size, last - offset))
            row.update(render_start=start, render_end=end,
                       rendered=offset + size >= first and offset < last)
            offset += size + 1
    return rows


def _parents(nodes, source):
    parents = {node['id']: node['parent_id'] for node in nodes.values()}
    for parent in filter(None, parents.values()):
        if parent not in nodes:
            raise DirectoryPlanRejected(f'unknown parent node {parent}')
    roots = {node_id: _root(node_id, parents) for node_id in parents}
    for document in source.documents:
        entries = _contents(document['pages'])
        counts = {}
        for entry in entries:
            counts[entry['title']] = counts.get(entry['title'], 0) + 1
        located = {}
        for node in nodes.values():
            if node['heading'][0] != document['document_id']:
                continue
            title = _compact(node['title'])
            if counts.get(title) == 1 and title not in located:
                located[title] = node['id']
        for entry in entries:
            parent = entries[entry['parent']] if entry['parent'] is not None else None
            node_id = located.get(entry['title'])
            parent_id = located.get(parent['title']) if parent is not None else None
            if node_id is None or parent_id is None or roots[node_id] != roots[parent_id]:
                continue
            if not _creates_cycle(node_id, parent_id, parents):
                parents[node_id] = parent_id
    for node_id in parents:
        _root(node_id, parents)
    return parents


def _contents(pages):
    entries, stack = [], []
    for page in pages:
        stack = []
        for line in page.splitlines():
            title, indent = _contents_title(line)
            if title is None:
                continue
            while stack and stack[-1][0] >= indent:
                stack.pop()
            entries.append({'title': _compact(title), 'parent': stack[-1][1] if stack else None})
            stack.append((indent, len(entries) - 1))
    return entries


def _contents_title(line):
    trimmed = line.strip()
    body = trimmed.rstrip('0123456789').rstrip()
    if body == trimmed or not body:
        return None, None
    title = body.rstrip(''.join(_LEADERS) + ' \t')
    if sum(character in _LEADERS for character in body[len(title):]) < 3 or not title.strip():
        return None, None
    return title.strip(), len(line) - len(line.lstrip(' \t'))


def _subject_fragment(source, origin, role):
    """Match Chinese soft wraps while retaining their physical character span."""
    text = _window_text(source, origin)
    lines, skipped, offset = text.split('\n'), set(), 0
    chinese = r'[\u3400-\u4dbf\u4e00-\u9fff]'
    for index, line in enumerate(lines[:-1]):
        left, right = line.rstrip(' \t'), lines[index + 1].lstrip(' \t')
        if (left and right and re.fullmatch(chinese, left[-1]) and re.fullmatch(chinese, right[0])
                and source.unit_at(origin[0], origin[1], origin[2] + index) is None
                and source.unit_at(origin[0], origin[1], origin[2] + index + 1) is None):
            skipped.update(range(offset + len(left),
                offset + len(line) + 1 + len(lines[index + 1]) - len(right)))
        offset += len(line) + 1
    positions = [index for index in range(len(text)) if index not in skipped]
    normalized = ''.join(text[index] for index in positions)
    label = re.sub(rf'(?<={chinese})[ \t]*\n[ \t]*(?={chinese})', '', role)
    matches = []
    literal = text.find(role)
    if literal >= 0:
        last = literal + len(role)
        selected = [i for i in range(literal, last) if i not in skipped]
        matches.append((''.join(text[i] for i in selected), selected[0], selected[-1] + 1))
    found = normalized.find(label)
    if found >= 0:
        matches.append((label, positions[found], positions[found + len(label) - 1] + 1))
    if not matches:
        return None
    label, first, last = min(matches, key=lambda match: match[1:])
    def point(index):
        return origin[2] + text.count('\n', 0, index), index - text.rfind('\n', 0, index) - 1
    return label, point(first), point(last)


def _nodes(plan, source):
    nodes = {}
    for raw in plan.get('nodes', []):
        _keys(raw, _NODE, 'source node')
        node_id = raw.get('id')
        if not isinstance(node_id, str) or not node_id or node_id in nodes:
            raise DirectoryPlanRejected(f'duplicate or empty source node {node_id}')
        heading = _point(raw.get('heading'), source, f'source node {node_id} heading')
        line = _continued_heading(source, heading, raw.get('heading_lines'))
        name = raw.get('name')
        if name is not None and (not str(name).strip() or _compact(name) not in _compact(line)):
            if not str(node_id).startswith('volume-'):
                raise DirectoryPlanRejected(f'source node {node_id} name is not a fragment of its heading')
        title = _business_name(line)
        if isinstance(name, str) and name.strip() and (str(node_id).startswith('volume-') or _compact(name) in _compact(line)):
            title = name.strip()
        if not title:
            raise DirectoryPlanRejected(f'source node {node_id} heading has no business name')
        response_kind = raw.get('response_kind', 'form')
        if response_kind not in ('form', 'narrative', 'attachment'):
            raise DirectoryPlanRejected(f'source node {node_id} response_kind is invalid')
        subjects, seen_subjects = [], set()
        if not isinstance(raw.get('subjects', []), list):
            raise DirectoryPlanRejected(f'source node {node_id} subjects must be source selections')
        for selection in raw.get('subjects', []):
            _keys(selection, _SUBJECT, 'subject')
            origin = _window(selection.get('source'), source, 'subject source')
            role = selection.get('role')
            fragment = _subject_fragment(source, origin, role) if isinstance(role, str) and role.strip() else None
            if fragment is None:
                raise DirectoryPlanRejected('subject role must be an exact fragment of its source')
            role, first, last = fragment
            physical = origin[0], origin[1], first, last
            if physical in seen_subjects:
                raise DirectoryPlanRejected('duplicate subject source selection')
            seen_subjects.add(physical)
            subjects.append((source.position(origin[0], origin[1], first[0]), first[1],
                             {'source': dict(selection['source']), 'role': role}))
        subjects = [selection for _, _, selection in sorted(subjects, key=lambda value: value[:2])]
        body_windows = raw.get('body_windows')
        if body_windows is not None:
            if not isinstance(body_windows, list):
                raise DirectoryPlanRejected(f'source node {node_id} body_windows must be source windows')
            body_windows = [_window(window, source, f'source node {node_id} body window') for window in body_windows]
        content_start = raw.get('content_start')
        if content_start is not None:
            content_start = _point(content_start, source, f'source node {node_id} content_start')
            if content_start[0] != heading[0]:
                raise DirectoryPlanRejected(f'source node {node_id} content_start must be in the heading document')
            if content_start > heading:
                raise DirectoryPlanRejected(f'source node {node_id} content_start must not follow its heading')
        nodes[node_id] = {'id': node_id, 'heading': heading, 'title': title, 'content_start': content_start,
                          'body_windows': body_windows,
                          'response_kind': response_kind, 'subjects': subjects,
                          'parent_id': raw.get('parent_id')}
    for document_id in {node['heading'][0] for node in nodes.values()}:
        ordered = sorted((node for node in nodes.values() if node['heading'][0] == document_id), key=lambda node: node['heading'])
        for index, node in enumerate(ordered):
            if node['content_start'] is None:
                continue
            previous = ordered[index - 1]['heading'] if index else None
            if previous is not None and node['content_start'] <= previous:
                raise DirectoryPlanRejected(f'source node {node["id"]} content_start must not cross the preceding heading')
    return nodes


def _composition(plan, source, *, format_candidates):
    raw = plan.get('composition', {'priority_basis': [], 'entries': []})
    _keys(raw, _COMPOSITION, 'composition')
    entries = []
    for index, entry in enumerate(raw.get('entries', [])):
        _keys(entry, _ENTRY, 'composition entry')
        point = _point(entry.get('name'), source, f'composition entry {index}')
        title = _business_name(_continued_heading(source, point, entry.get('heading_lines')))
        if not title:
            raise DirectoryPlanRejected(f'composition entry {index} has no name')
        excluded_by = [_window(window, source, f'composition entry {index} exclusion') for window in entry.get('excluded_by', [])]
        entries.append({'name': point, 'title': title, 'excluded_by': excluded_by})
    documents = {entry['name'][0] for entry in entries}
    if len(documents) > 1:
        raise DirectoryPlanRejected('a composition list must stay inside one document')
    basis = [_window(window, source, 'priority basis') for window in raw.get('priority_basis', [])]
    if documents and not format_candidates:
        identity = next(iter(documents))
        if source.by_id[identity]['role'] in _REVISIONS and not any(window[0] == identity for window in basis):
            raise DirectoryPlanRejected('a revision composition requires a priority basis in that document')
    return {'entries': entries, 'priority_basis': basis}


def _revision_issues(source, priority_basis):
    cited = {window[0] for window in priority_basis}
    issues = []
    for document in source.documents:
        if document['role'] in _REVISIONS and document['document_id'] not in cited:
            label = _ROLE_LABEL[document['role']]
            issues.append({'code': 'outline_directory_revision_unreviewed',
                           'detail': f'{document["document_id"]}是{label}，本次目录没有引用其中的优先依据，不自动改写组成'})
    return issues


def _ordered_items(scope, composition):
    """One composition entry keeps one list slot. A repeated citation keeps the chapter by its own heading."""
    collapsed, issues, seen = [], [], set()
    for item in scope.get('items', []):
        entry = item.get('entry')
        if entry is None:
            collapsed.append(item)
            continue
        if not isinstance(entry, int) or entry < 0 or entry >= len(composition['entries']):
            raise DirectoryPlanRejected(f'unknown composition entry {entry}')
        if entry in seen:
            title = composition['entries'][entry]['title']
            issues.append({'code': 'outline_directory_name',
                           'detail': f'组成条目“{title}”被重复引用，多出来的章节改按自身标题保留'})
            collapsed.append({**item, 'entry': None})
            continue
        seen.add(entry)
        collapsed.append(item)
    listed = [item.get('entry') for item in collapsed if item.get('entry') is not None]
    ordered = sorted(listed, key=lambda entry: composition['entries'][entry]['name'])
    upcoming = iter(ordered)
    result = []
    for item in collapsed:
        if item.get('entry') is None:
            result.append(item)
            continue
        entry = next(upcoming)
        result.append(next(candidate for candidate in collapsed if candidate.get('entry') == entry))
    return result, issues


def _excluded(plan, nodes, parents, source, composition):
    ids, issues, undecided = set(), [], []
    adopted_entries = {item.get('entry') for scope in plan.get('scopes', []) for item in scope.get('items', [])
                       if item.get('entry') is not None}
    for index, entry in enumerate(composition['entries']):
        if index in adopted_entries and entry['excluded_by']:
            raise DirectoryPlanRejected(f'composition entry {index} is both adopted and excluded')
        if index not in adopted_entries and not entry['excluded_by']:
            undecided.append(entry['title'])
        if entry['excluded_by']:
            basis = '；'.join(_window_text(source, window) for window in entry['excluded_by'])
            issues.append({'code': 'outline_directory_excluded', 'detail': f'原文排除 {entry["title"]}；依据：{basis}'})
    if undecided:
        raise DirectoryPlanRejected('composition entries without a decision: ' + '；'.join(undecided))
    for raw in plan.get('excluded', []):
        _keys(raw, _EXCLUSION, 'exclusion')
        node_id = raw.get('node_id')
        if node_id not in nodes:
            raise DirectoryPlanRejected(f'unknown excluded node {node_id}')
        window = _window(raw.get('source'), source, f'exclusion {node_id}')
        for identity in [node_id, *_descendants(node_id, parents, nodes, set())]:
            ids.add(identity)
        issues.append({'code': 'outline_directory_excluded',
                       'detail': f'原文排除 {nodes[node_id]["title"]}；依据：{_window_text(source, window)}'})
    return {'ids': ids, 'issues': issues}


def _place_continuation_grids(chapters, source):
    """Keep both grids of a link in the chapter that already holds one of them.

    A grid that already belongs to another chapter stays there. Unselected
    prose is not copied into the chapter body.
    """
    def holders(document_id, unit_id):
        found = []
        for chapter in chapters:
            for part in chapter.get('template_parts') or []:
                ref = part.get('source_ref') or {}
                if ref.get('unit_id') != unit_id:
                    continue
                owner = part.get('_document_id')
                if owner not in (None, document_id):
                    continue
                if chapter not in found:
                    found.append(chapter)
        return found

    for document in source.documents:
        for link in document.get('continuations') or []:
            if link.get('kind') != 'ruled_table':
                continue
            before_id, after_id = link.get('before_table_id'), link.get('after_table_id')
            before_holders, after_holders = holders(document['document_id'], before_id), holders(document['document_id'], after_id)
            if len(before_holders) == 1 and not after_holders:
                _append_continuation_part(before_holders[0], source, document['document_id'], after_id, link, later=True)
            elif len(after_holders) == 1 and not before_holders:
                _append_continuation_part(after_holders[0], source, document['document_id'], before_id, link, later=False)
                if link.get('repeated_header_rows'):
                    _mark_later_header(after_holders[0], document['document_id'], after_id, link['repeated_header_rows'])
            elif len(before_holders) == 1 and before_holders == after_holders and link.get('repeated_header_rows'):
                _mark_later_header(before_holders[0], document['document_id'], after_id, link['repeated_header_rows'])
    for chapter in chapters:
        parts = chapter.get('template_parts') or []
        parts.sort(key=lambda part: _continuation_page(source, part))


def _continuation_page(source, part):
    if isinstance(part.get('_source_page'), int):
        document_id = part.get('_document_id')
        for document in source.documents:
            if document_id in (None, document['document_id']):
                return document['index'], part['_source_page']
        return 0, part['_source_page']
    unit_id = (part.get('source_ref') or {}).get('unit_id')
    owner = part.get('_document_id')
    for document in source.documents:
        if owner not in (None, document['document_id']):
            continue
        for page, rows in enumerate(document.get('line_units') or []):
            if unit_id in (rows or []):
                return document['index'], page
    return 0, 0


def _append_continuation_part(chapter, source, document_id, unit_id, link, *, later):
    part = source.template_part(document_id, unit_id)
    if part is None:
        return
    added = deepcopy(part)
    added['_document_id'] = document_id
    added['_source_page'] = link.get('after_page') if later else link.get('before_page')
    if later and link.get('repeated_header_rows') and added.get('header_row_count') is None:
        added['header_row_count'] = link['repeated_header_rows']
    chapter.setdefault('template_parts', []).append(added)


def _mark_later_header(chapter, document_id, unit_id, count):
    for part in chapter.get('template_parts') or []:
        ref = part.get('source_ref') or {}
        if ref.get('unit_id') != unit_id or part.get('_document_id') not in (None, document_id):
            continue
        if part.get('_document_id') != document_id:
            continue
        if part.get('header_row_count') is None:
            part['header_row_count'] = count


def _attach_template_parts(chapters, source, issues=None):
    for chapter in chapters:
        parts, seen = [], set()
        for line in chapter.get('lines', []):
            unit_id = line.get('unit_id')
            identity = line['document_id'], unit_id
            if not unit_id or identity in seen:
                continue
            seen.add(identity)
            if not _covers_grid(chapter, source, line['document_id'], unit_id):
                if chapter.get('body_windows') is not None and issues is not None:
                    issues.append({'code': 'outline_directory_partial_table',
                                   'detail': f'{line["document_id"]}第{line["page"] + 1}页表{unit_id}未完整覆盖，保留已选原文，原表待核'})
                continue
            part = source.template_part(line['document_id'], unit_id)
            if part is None:
                continue
            part = deepcopy(part)
            part['_document_id'] = line['document_id']
            part['_source_page'] = line['page']
            parts.append(part)
        if parts:
            chapter['template_parts'] = parts


def _segment_template_content(chapters, source):
    """Keep prose and fully located tables in source order."""
    for chapter in chapters:
        parts = chapter.get('template_parts') or []
        refs = chapter.get('lines') or []
        if not parts or not refs:
            continue
        positions = {}
        for index, ref in enumerate(refs):
            part = source.template_part(ref['document_id'], ref.get('unit_id'))
            if part is not None:
                positions.setdefault(_template_identity(part), []).append(index)
        spans = {}
        for part in parts:
            identity = _template_identity(part)
            indices = positions.get(identity) or []
            if not indices:
                continue
            start, end = indices[0], indices[-1]
            # An unrelated substantive line inside the span cannot be discarded.
            if any(_part_foreign(source, refs[index], part)
                   and source.line(refs[index]['document_id'], refs[index]['page'], refs[index]['line']).strip()
                   for index in range(start, end + 1)):
                continue
            spans[start] = (end, part)
        if not spans:
            continue
        segments, text, prose_refs, used = [], [], [], set()
        def flush():
            content = '\n'.join(text).strip()
            if content:
                segments.append({'text': content, '_source_rows': _source_rows(source, prose_refs)})
            text.clear()
            prose_refs.clear()
        index = 0
        while index < len(refs):
            if index in spans:
                flush()
                end, part = spans[index]
                segments.append({'part': part})
                used.add(_template_identity(part))
                index = end + 1
                continue
            ref = refs[index]
            text.append(source.line(ref['document_id'], ref['page'], ref['line']))
            prose_refs.append(ref)
            index += 1
        flush()
        unused = [part for part in parts if _template_identity(part) not in used]
        for part in sorted(unused, key=lambda item: item.get('_source_page') if isinstance(item.get('_source_page'), int) else 10**9):
            page = part.get('_source_page') if isinstance(part.get('_source_page'), int) else 10**9
            inserted = False
            for index, segment in enumerate(segments):
                if _segment_page(segment) > page:
                    segments.insert(index, {'part': part})
                    inserted = True
                    break
            if not inserted:
                segments.append({'part': part})
        chapter['template_segments'] = segments


def _part_foreign(source, ref, part):
    owner = source.template_part(ref['document_id'], ref.get('unit_id'))
    if owner is None:
        return True
    return _template_identity(owner) != _template_identity(part)


def _segment_page(segment):
    if 'part' in segment and isinstance(segment['part'].get('_source_page'), int):
        return segment['part']['_source_page']
    rows = segment.get('_source_rows') or []
    pages = [row.get('page') for row in rows if isinstance(row.get('page'), int)]
    return min(pages) if pages else 0


def _template_identity(part):
    return json.dumps(part['source_ref'], sort_keys=True, ensure_ascii=False)


def _public_part(part):
    return {key: value for key, value in part.items() if not str(key).startswith('_')}


def _place_wrapped_grids(chapters, source, issues=None):
    """A native grid still belongs to a chapter when a wrapped cell is not one layout line."""
    claimed = {_template_identity(part) for chapter in chapters for part in chapter.get('template_parts') or []}
    for document in source.documents:
        for grid in document.get('grids') or []:
            part = source.template_part(document['document_id'], grid['unit_id'])
            if part is None or _template_identity(part) in claimed:
                continue
            # A table located by PDF coordinates cannot belong to a chapter
            # whose source range does not include those lines.
            page = grid.get('page')
            owners = document.get('line_units') or []
            if isinstance(page, int) and 0 <= page < len(owners) and grid['unit_id'] in owners[page]:
                continue
            ranked = []
            cells = [cell for row in grid.get('rows') or [] for cell in row]
            if len(cells) < 2:
                continue
            for chapter in chapters:
                owned = [line for line in chapter.get('lines') or []
                         if line.get('document_id') == document['document_id'] and line.get('page') == grid.get('page')]
                if not owned:
                    continue
                if chapter.get('body_windows') is not None:
                    if issues is not None:
                        issues.append({'code': 'outline_directory_table_unbound',
                                       'detail': f'{document["document_id"]}第{page + 1}页表{grid["unit_id"]}未能完整定位到显式正文，保留原文待核'})
                    continue
                text = _compact(chapter.get('text') or '')
                hits = sum(1 for cell in cells if _cell_found(cell, text, chapter.get('text') or ''))
                ratio = hits / len(cells)
                # Wrapped headers often miss half the cells. The grid still belongs to the chapter that holds the page.
                if (hits >= 2 and ratio >= 0.5) or (len(owned) >= 3 and hits >= 1):
                    ranked.append((ratio, len(owned), -len(chapter.get('lines') or []), chapter))
            if not ranked:
                continue
            ranked.sort(key=lambda item: (item[0], item[1], item[2]), reverse=True)
            chapter = ranked[0][-1]
            chapter.setdefault('template_parts', []).append(part)
            claimed.add(_template_identity(part))


def _cell_found(cell, compact_text, raw_text):
    target = _compact(cell)
    if len(target) < 2:
        return False
    if target in compact_text:
        return True
    if len(target) < 4:
        return False
    cursor = 0
    for line in str(raw_text).splitlines():
        piece = _compact(line)
        if len(piece) < 2 or not target.startswith(piece, cursor):
            continue
        cursor += len(piece)
        if cursor == len(target):
            return True
    return False


def _covers_grid(chapter, source, document_id, unit_id):
    present = {(line['page'], line['line']) for line in chapter.get('lines', [])
               if line.get('document_id') == document_id and line.get('unit_id') == unit_id}
    owners = source._document(document_id).get('line_units') or []
    needed = {(page, line) for page, rows in enumerate(owners) for line, owner in enumerate(rows) if owner == unit_id}
    return bool(needed) and needed <= present


def _decisions(nodes, seen, excluded_ids, source_only):
    source_only = set(source_only)
    if source_only & excluded_ids:
        raise DirectoryPlanRejected('a source node is both excluded and source_only')
    if seen & source_only:
        raise DirectoryPlanRejected('a source node is both adopted and source_only')
    missing = [node_id for node_id in nodes if node_id not in seen and node_id not in source_only and node_id not in excluded_ids]
    if missing:
        raise DirectoryPlanRejected(f'source nodes without a decision: {missing}')


def _body(node, source, nodes):
    if node.get('body_windows') is not None:
        points = {(document_id, page, line) for document_id, page, start, end in node['body_windows']
                  for line in range(start, end + 1)}
        lines, copied = [], []
        for document_id, page, line in sorted(points, key=lambda point: source.position(*point)):
            lines.append(source.line(document_id, page, line))
            unit_id = source.unit_at(document_id, page, line)
            copied.append({'document_id': document_id, 'page': page, 'line': line,
                           **({'unit_id': unit_id} if unit_id else {})})
        return '\n'.join(lines), copied
    start = node['content_start'] or node['heading']
    following = [other['content_start'] or other['heading'] for other in nodes.values()
                 if (other['content_start'] or other['heading'])[0] == start[0]
                 and (other['content_start'] or other['heading']) > start]
    end = min(following) if following else None
    lines, copied = [], []
    for page_index, page in enumerate(source.pages(start[0])):
        for line_index, line in enumerate(page.splitlines()):
            point = (start[0], page_index, line_index)
            if point < start or (end is not None and point >= end):
                continue
            unit_id = source.unit_at(start[0], page_index, line_index)
            if not unit_id and (_page_number(line) or _running_header(line, page_index, source.pages(start[0]), node)):
                continue
            lines.append(source.line(start[0], page_index, line_index))
            copied.append({'document_id': start[0], 'page': page_index, 'line': line_index,
                           **({'unit_id': unit_id} if unit_id else {})})
    # Drop blank edges only. A layout indent on the first line is part of that line.
    return '\n'.join(lines).strip('\n'), copied


def _requirement_windows(raw, source):
    first = _window({field: raw.get(field) for field in ('document_id', 'page', 'start_line', 'end_line')},
                    source, 'requirement first span')
    windows = [first]
    continuations = raw.get('continuation_windows', [])
    if not isinstance(continuations, list):
        raise DirectoryPlanRejected('requirement continuation_windows must be source windows')
    previous = first[1], first[3]
    for raw_window in continuations:
        window = _window(raw_window, source, 'requirement continuation')
        if window[0] != first[0] or (window[1], window[2]) <= previous:
            raise DirectoryPlanRejected('requirement continuation must be forward in the same document')
        windows.append(window)
        previous = window[1], window[3]
    return windows


def _requirement_text(source, raw, used):
    document_id, page = raw.get('document_id'), raw.get('page')
    start, end = raw.get('start_line'), raw.get('end_line')
    if not isinstance(start, int) or not isinstance(end, int) or end < start:
        raise DirectoryPlanRejected('requirement window is not continuous')
    lines = []
    for line in range(start, end + 1):
        if (document_id, page, line) in used:
            continue
        lines.append(source.line(document_id, page, line))
    return '\n'.join(lines).strip()


def _parameter_child(parent, source, item, index, *, source_contract=None):
    document_id, page = item.get('document_id'), item.get('page')
    start, end, content = item.get('start_line'), item.get('end_line'), item.get('content_start_line')
    if not all(isinstance(value, int) for value in (start, end, content)) or end < start or not start <= content <= end:
        raise DirectoryPlanRejected('parameter span is not inside its item')
    _window({'document_id': document_id, 'page': page, 'start_line': start, 'end_line': end},
            source, 'parameter first span')
    windows = [(document_id, page, content, end)]
    continuation = item.get('continuation_windows', [])
    if not isinstance(continuation, list):
        raise DirectoryPlanRejected('parameter continuation_windows must be source windows')
    previous = (page, end)
    for raw in continuation:
        window = _window(raw, source, 'parameter continuation')
        if window[0] != document_id or (window[1], window[2]) <= previous:
            raise DirectoryPlanRejected('parameter continuation must be forward in the same document')
        windows.append(window)
        previous = (window[1], window[3])
    selected = {(document_id, page, line) for line in range(start, end + 1)}
    selected.update((identity, number, line) for identity, number, first, last in windows[1:]
                    for line in range(first, last + 1))
    if parent.get('body_windows') is not None:
        visible = {(ref['document_id'], ref['page'], ref['line']) for ref in parent.get('lines', [])}
        if not selected <= visible:
            raise DirectoryPlanRejected('parameter span is outside the explicit parent body')
    title = _business_name(source.line(document_id, page, start))
    copied = [{'document_id': identity, 'page': number, 'line': line}
              for identity, number, first, last in windows for line in range(first, last + 1)]
    if source_contract == PARAMETER_SOURCE_CONTRACT:
        for ref in copied:
            if unit_id := source.unit_at(ref['document_id'], ref['page'], ref['line']):
                ref['unit_id'] = unit_id
    text = '\n'.join(source.line(ref['document_id'], ref['page'], ref['line']) for ref in copied).strip()
    if not title or not text:
        raise DirectoryPlanRejected('parameter item has no source text')
    child = _chapter(f"{parent['id']}:parameter:{index + 1}", title, parent['id'], 'section',
                     (document_id, page, start), None, text, '', None, None, copied)
    child.update(response_kind=parent.get('response_kind', 'form'), subjects=deepcopy(parent.get('subjects', [])),
                 parameter_parent_id=parent['id'], _source_body_rows=_source_rows(source, copied))
    if item.get('title') is not None:
        child['display_title'] = str(item['title']).strip()
    return child


def _parameter_native_lines(source):
    result = {}
    for document in source.documents:
        identity = document['document_id']
        units = set(document['template_parts']) | {grid['unit_id'] for grid in document['grids']}
        for unit_id in units:
            result[identity, unit_id] = set()
        for page, owners in enumerate(document['line_units']):
            for line, unit_id in enumerate(owners):
                if unit_id in units:
                    result[identity, unit_id].add((identity, page, line))
    return result


def _parameter_table_issue(child, reason, *, partial=False):
    return {'code': 'outline_directory_partial_table' if partial else 'outline_directory_table_unbound',
            'detail': f'“{child.get("display_title", child["title"])}”的原表关联尚未确认：{reason}；'
                      '原文已保留，请对照父章原表核定后在Word中安排引用。'}


def _prepare_parameter_tables(child, source, item, native_lines):
    touched = list(dict.fromkeys((ref['document_id'], ref.get('unit_id')) for ref in child['lines']
                                if (ref['document_id'], ref.get('unit_id')) in native_lines))
    if not touched or child.get('response_kind') != 'narrative':
        return
    selected = {(item['document_id'], item['page'], line) for line in range(item['start_line'], item['end_line'] + 1)}
    selected.update((raw['document_id'], raw['page'], line) for raw in item.get('continuation_windows', [])
                    for line in range(raw['start_line'], raw['end_line'] + 1))
    candidates = []
    for document_id, unit_id in touched:
        owned = native_lines[document_id, unit_id]
        part = source.template_part(document_id, unit_id)
        if not owned or part is None or part.get('selectors'):
            child['_parameter_table_issue'] = _parameter_table_issue(child, '缺少可核验的完整来源原表')
            return
        if not owned <= selected:
            child['_parameter_table_issue'] = _parameter_table_issue(child, '本条只选择了部分原表', partial=True)
            return
        for page in {point[1] for point in owned}:
            lines = [point[2] for point in owned if point[1] == page]
            if any(source.line(document_id, page, line).strip() and source.unit_at(document_id, page, line) != unit_id
                   for line in range(min(lines), max(lines) + 1)):
                child['_parameter_table_issue'] = _parameter_table_issue(child, f'来源第{page + 1}页表内仍有未确认的行')
                return
        candidates.append({'document_id': document_id, 'unit_id': unit_id, 'part': deepcopy(part)})
    child['_parameter_native_parts'] = candidates
    child['_parameter_text_lines'] = [source.line(ref['document_id'], ref['page'], ref['line']) for ref in child['lines']]


def _parameter_part_key(part):
    return json.dumps({'source_ref': part['source_ref'], 'selectors': part.get('selectors', [])},
                      sort_keys=True, ensure_ascii=False)


def _bind_parameter_tables(compiled, nodes):
    issues = []
    for child in compiled['chapters']:
        if child.get('_parameter_table_issue'):
            issues.append(child['_parameter_table_issue'])
            continue
        parts = child.get('_parameter_native_parts')
        if not parts:
            continue
        parent_id = child.get('parameter_parent_id')
        parent, node = nodes.get(parent_id), nodes.get(child['id'])
        if parent_id != child.get('parent_id') or parent is None or node is None:
            issues.append(_parameter_table_issue(child, '未能定位同一父章的原表'))
            continue
        targets = [target for target in node['targets'] if target['purpose'] == child['text']]
        links = []
        for target in targets:
            related = []
            for candidate in parts:
                key = _parameter_part_key(candidate['part'])
                matches = [original for original in parent['targets']
                           if original.get('subject') == target.get('subject')
                           for part in original.get('template_refs', []) if _parameter_part_key(part) == key]
                if len(matches) != 1:
                    break
                related.append(matches[0]['target_id'])
            if len(related) != len(parts):
                break
            links.append((target, related))
        if not targets or len(links) != len(targets):
            issues.append(_parameter_table_issue(child, '父原表的来源版本、选择或主体不唯一匹配'))
            continue
        linked = {(candidate['document_id'], candidate['unit_id']) for candidate in parts}
        prose = '\n'.join(text for ref, text in zip(child['lines'], child['_parameter_text_lines'])
                          if (ref['document_id'], ref.get('unit_id')) not in linked).strip()
        rows = deepcopy(child.get('_source_body_rows', []))
        for row in rows:
            if (row['document_id'], row.get('unit_id')) in linked:
                row.update(render_start=0, render_end=len(row['text']), rendered=False)
        for target, related in links:
            target['purpose'] = prose or '原表见关联响应位置。'
            target['_source_rows'] = deepcopy(rows)
            if not prose:
                target['_source_generated'] = True
            target['related_target_ids'] = list(dict.fromkeys([*target.get('related_target_ids', []), *related]))
    return issues


def _obligations(item, entry, source, covered=(), *, source_lines=None, source_rows=None):
    texts, used = [], set(covered)
    for window in item.get('clause') or []:
        document_id, page, start, end = _window(window, source, 'clause')
        text = _requirement_text(source, window, used)
        texts.append(text)
        if text and (source_lines is not None or source_rows is not None):
            refs = [_point_dict((document_id, page, line)) for line in range(start, end + 1)
                    if (document_id, page, line) not in used]
            if source_lines is not None:
                source_lines.extend(refs)
            if source_rows is not None:
                source_rows.extend(_source_rows(source, refs, trim=True))
        used.update((document_id, page, line) for line in range(start, end + 1))
    if entry is not None and entry['name'] not in used:
        texts.append(source.line(*entry['name']))
        if source_lines is not None and texts[-1].strip():
            source_lines.append(_point_dict(entry['name']))
        if source_rows is not None and texts[-1].strip():
            source_rows.extend(_source_rows(source, [_point_dict(entry['name'])]))
    return '\n'.join(text for text in texts if text.strip())


def _condition(item, source):
    window = item.get('condition')
    if window is None:
        return None
    return _window_text(source, _window(window, source, 'condition'))


def _children(node_id, parents, nodes, excluded_ids):
    children = [candidate for candidate, parent in parents.items() if parent == node_id and candidate not in excluded_ids]
    return sorted(children, key=lambda candidate: nodes[candidate]['heading'])


def _descendants(node_id, parents, nodes, excluded_ids):
    result = []
    for child in _children(node_id, parents, nodes, set()):
        if child in excluded_ids:
            continue
        result.append(child)
        result.extend(_descendants(child, parents, nodes, excluded_ids))
    return result


def _scope_title(scope, source):
    title = scope.get('title')
    if title is None:
        return None
    point = _point(title, source, 'scope title')
    return _business_name(source.line(*point))


def _running_header(line, page_index, pages, node):
    text = line.strip()
    if not text or text == node['title']:
        return False
    openings = [next((item.strip() for item in page.splitlines() if item.strip()), '') for page in pages]
    return openings.count(text) > 1 and text == openings[page_index]


def _page_number(line):
    return line.strip().isdigit()


def _number_only(title):
    compact = _compact(title)
    for token in ('附件', '附录', '附表'):
        compact = compact.replace(token, '')
    return not compact or compact.isdigit()


def _continued_heading(source, point, count=None):
    """Join a heading broken by the layout while a parenthesis is open, or over its selected lines."""
    document_id, page, line = point
    lines = source.pages(document_id)[page].splitlines()
    if count is not None:
        if type(count) is not int or not 1 <= count <= 4 or line + count > len(lines):
            raise DirectoryPlanRejected('heading_lines must select consecutive lines on the heading page')
        return ''.join(part.strip() for part in lines[line:line + count])
    parts = [lines[line]]
    while _unclosed(''.join(parts)) and line + len(parts) < len(lines):
        nxt = lines[line + len(parts)]
        if _ENUM.match(nxt.strip()):
            break
        parts.append(nxt)
    return ''.join(part.strip() for part in parts)


def _unclosed(text):
    return text.count('（') + text.count('(') > text.count('）') + text.count(')')


def _business_name(line):
    name = _SPACE.sub(' ', _CJK_GAP.sub('', _ENUM.sub('', line.strip(), count=1))).strip()
    return name.rstrip('；;')


def _parent_heading(chapter, chapters):
    if chapter.get('parent_id') is None:
        return None
    for candidate in chapters.values():
        if candidate['id'] == chapter['parent_id']:
            return _heading(candidate)
    return 'unadopted_parent', chapter['parent_id']


def _effective_group(chapter, identities, key='group'):
    seen = set()
    while chapter and chapter['id'] not in seen:
        seen.add(chapter['id'])
        if chapter.get('group') is not None:
            return chapter.get(key)
        chapter = identities.get(chapter.get('parent_id'))
    return None


def _heading(chapter):
    heading = chapter['heading']
    return heading['document_id'], heading['page'], heading['line']


def _conflict_detail(left, right, by_a, by_b, differences):
    labels = {'title': '标题', 'parent_id': '父级', 'presentation': '呈现方式', 'group': '分组', 'group_heading': '分组来源',
              'response_kind': '响应类型', 'subjects': '响应主体', 'body': '正文范围',
              'condition': '适用条件', 'obligations': '补充义务', 'obligation_lines': '补充义务来源'}

    def display(value):
        if value is None or value == '':
            return '未选择'
        return value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)

    values = []
    for field, (first, second) in differences.items():
        if field == 'parent_id':
            first = f'{_parent_title(left, by_a)} {display(first)}'
            second = f'{_parent_title(right, by_b)} {display(second)}'
        values.append(f'{labels[field]} [{field}]：首份“{display(first)}”；另一份“{display(second)}”')
    return (f'两份计划对“{left["title"]}”的{"、".join(labels[field] for field in differences)}不一致，'
            f'保留先完成的一份；差异待核；' + '；'.join(values))


def _parent_title(chapter, chapters):
    for candidate in chapters.values():
        if candidate['id'] == chapter.get('parent_id'):
            return candidate['title']
    return f'未采用父节点 {chapter["parent_id"]}' if chapter.get('parent_id') else '无'


def _root(node_id, parents):
    seen = set()
    current = node_id
    while parents.get(current):
        if current in seen:
            raise DirectoryPlanRejected(f'source node {node_id} has a parent cycle')
        seen.add(current)
        current = parents[current]
    return current


def _creates_cycle(node_id, parent_id, parents):
    current = parent_id
    seen = {node_id}
    while current:
        if current in seen:
            return True
        seen.add(current)
        current = parents.get(current)
    return False


def _point(value, source, label):
    if not isinstance(value, dict):
        raise DirectoryPlanRejected(f'{label} must be a source line')
    _keys(value, _POINT, label)
    document_id, page, line = value.get('document_id'), value.get('page'), value.get('line')
    if not isinstance(document_id, str) or not isinstance(page, int) or not isinstance(line, int):
        raise DirectoryPlanRejected(f'{label} must be a source line')
    source.line(document_id, page, line)
    return document_id, page, line


def _window(value, source, label) -> tuple[str, int, int, int]:
    if not isinstance(value, dict):
        raise DirectoryPlanRejected(f'{label} must be a continuous source window')
    _keys(value, _WINDOW, label)
    document_id = value.get('document_id')
    page = value.get('page')
    start = value.get('start_line')
    end = value.get('end_line')
    if (not isinstance(document_id, str) or not isinstance(page, int) or not isinstance(start, int)
            or not isinstance(end, int) or end < start):
        raise DirectoryPlanRejected(f'{label} must be a continuous source window')
    source.line(document_id, page, start)
    source.line(document_id, page, end)
    return document_id, page, start, end


def _window_text(source, window):
    document_id, page, start, end = window
    return '\n'.join(source.pages(document_id)[page].splitlines()[start:end + 1])


def _keys(value, allowed, label):
    if not isinstance(value, dict):
        raise DirectoryPlanRejected(f'{label} must be an object')
    unknown = set(value) - allowed
    if unknown:
        raise DirectoryPlanRejected(f'{label} contains {"、".join(sorted(unknown))}')


def _point_dict(point):
    if point is None:
        return None
    if isinstance(point, dict):
        return point
    return {'document_id': point[0], 'page': point[1], 'line': point[2]}


def _heading_key(heading):
    heading = heading or {}
    return heading.get('document_id') or '', heading.get('page') or 0, heading.get('line') or 0


def _compact(text):
    return ''.join(str(text).split())


def _empty(detail):
    return {'chapters': [], 'issues': [{'code': 'outline_directory_rejected', 'detail': detail}]}


def _group_identity(chapter):
    heading = chapter.get('group_heading')
    value = heading if heading is not None else {
        'document_id': chapter['heading']['document_id'], 'source_name': chapter.get('group')}
    return 'group-title-' + hashlib.sha256(
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()
    ).hexdigest()


def _nest(chapters, *, source_nodes=None):
    group_identity = _group_identity
    parameter_parents = {chapter['parameter_parent_id'] for chapter in chapters
                         if chapter.get('parameter_parent_id')
                         and chapter['parameter_parent_id'] == chapter.get('parent_id')}
    nodes = {chapter['id']: _outline_chapter(chapter, parameter_parent=chapter['id'] in parameter_parents)
             for chapter in chapters}
    containers = {}
    for chapter in chapters:
        if (chapter.get('group') and chapter.get('group_heading') == chapter['heading']
                and chapter.get('parent_id') not in nodes
                and chapter.get('presentation') not in (DirectoryPresentation.CONTENT, DirectoryPresentation.COVER)):
            containers.setdefault(group_identity(chapter), []).append(chapter['id'])
    containers = {identity: values[0] for identity, values in containers.items() if len(values) == 1}
    roots, volumes, placed = [], {}, set()
    for chapter in chapters:
        parent = chapter['parent_id']
        node = nodes[chapter['id']]
        if parent and parent in nodes and parent != chapter['id']:
            nodes[parent]['children'].append(node)
            continue
        group = (chapter.get('group') or '').strip()
        identity = group_identity(chapter) if group else None
        if identity in containers:
            volume = nodes[containers[identity]]
            volume['is_volume'] = True
            if identity not in placed:
                roots.append(volume)
                placed.add(identity)
            if chapter['id'] != containers[identity]:
                volume['children'].append(node)
        elif group and (group != chapter['title'] or chapter.get('group_heading') not in (None, chapter['heading'])):
            identity = group_identity(chapter)
            volume = volumes.get(identity)
            if volume is None:
                volume = {'chapter_id': f'volume-{len(volumes) + 1}', 'title': chapter.get('group_display_title') or group, 'targets': [],
                          'children': [], 'is_volume': True, 'presentation_role': 'body', '_pending': []}
                volumes[identity] = volume
                roots.append(volume)
            volume['_pending'].append(chapter)
        else:
            roots.append(node)
    for volume in volumes.values():
        pending = volume.pop('_pending')
        if not any(item.get('entry') is not None for item in pending):
            pending.sort(key=lambda item: _heading_key(item.get('heading')))
        volume['children'] = [nodes[item['id']] for item in pending]
    restored = {}
    for chapter in chapters:
        if chapter.get('group_basis_windows'):
            identity = group_identity(chapter)
            volume = nodes.get(containers.get(identity)) or volumes.get(identity)
            if volume is not None:
                restored[id(volume)] = _heading_key(chapter['group_heading'])
    start = 0
    while start < len(roots):
        order = restored.get(id(roots[start]))
        if order is None:
            start += 1
            continue
        end = start + 1
        while end < len(roots) and (following := restored.get(id(roots[end]))) is not None and following[0] == order[0]:
            end += 1
        roots[start:end] = sorted(roots[start:end], key=lambda volume: restored[id(volume)][1:])
        start = end
    for chapter in chapters:
        node = nodes[chapter['id']]
        if not node['targets'] and not node['children']:
            subjects = chapter.get('subjects') or [None]
            node['targets'] = [{**_verbatim_target('narrative', chapter['title']),
                                '_source_generated': True,
                                **({'subject': subject['role']} if subject else {})} for subject in subjects]
    if source_nodes is not None:
        source_nodes.update(nodes)
    return roots


def _assign_ids(chapters):
    def walk(entries, parent=()):
        for ordinal, chapter in enumerate(entries, 1):
            path = (*parent, str(ordinal))
            chapter['chapter_id'] = 'chapter-' + '.'.join(path)
            for index, target in enumerate(chapter['targets'], 1):
                target['target_id'] = f"{chapter['chapter_id']}:target-{index}"
            walk(chapter['children'], path)
    walk(chapters)
    return chapters


def _outline_chapter(chapter, *, parameter_parent=False):
    targets = []
    def source_target(kind, text, rows=None, *, generated=False):
        target = _verbatim_target(kind, text)
        if rows is not None:
            target['_source_rows'] = deepcopy(rows)
        if generated:
            target['_source_generated'] = True
        return target
    kind = chapter.get('response_kind', 'form')
    original_kind = 'form' if parameter_parent and kind == 'narrative' else kind
    body, obligations = chapter['text'].strip(), chapter['obligations'].strip()
    if chapter.get('template_segments'):
        for segment in chapter['template_segments']:
            target = source_target(original_kind, segment.get('text') or chapter['title'], segment.get('_source_rows'),
                                   generated=not segment.get('text'))
            if segment.get('part'):
                target['template_refs'] = [_public_part(segment['part'])]
            targets.append(target)
    elif body or chapter.get('template_parts'):
        target = source_target(original_kind, body or chapter['title'], chapter.get('_source_body_rows'),
                               generated=not body)
        if chapter.get('template_parts'):
            target['template_refs'] = [_public_part(part) for part in chapter['template_parts']]
        targets.append(target)
    if original_kind != kind:
        targets.append(source_target('narrative', f"{chapter['title']}（本章整体响应）", generated=True))
    common = []
    if obligations and obligations != body:
        common.append(source_target('narrative', obligations, chapter.get('_source_obligation_rows')))
    for index, requirement in enumerate(chapter.get('requirements', [])):
        rows = chapter.get('_requirement_rows', [])
        requirement_target = source_target('narrative', requirement, rows[index] if index < len(rows) else None)
        requirement_target['enterprise_response'] = True
        common.append(requirement_target)
    materials = []
    for index, material in enumerate(chapter.get('materials', [])):
        rows = chapter.get('_material_rows', [])
        materials.append(source_target(kind, material, rows[index] if index < len(rows) else None))
    for segment in chapter.get('material_segments', []):
        target = source_target(kind, segment.get('text') or chapter['title'], segment.get('_source_rows'),
                               generated=not segment.get('text'))
        if segment.get('part'):
            target['template_refs'] = [_public_part(segment['part'])]
        materials.append(target)
    if chapter.get('subjects'):
        targets = [{**deepcopy(target), 'subject': subject['role']}
                   for subject in chapter['subjects'] for target in [*targets, *materials]] + common
    else:
        targets = [*targets, *common, *materials]
    condition = (chapter.get('condition') or '').strip()
    held = '\n'.join(target['purpose'] for target in targets)
    if condition and condition not in held.splitlines():
        targets.append(source_target('narrative', condition, chapter.get('_source_condition_rows')))
    role = chapter['presentation'] if chapter.get('presentation') in (
        DirectoryPresentation.CONTENT, DirectoryPresentation.COVER) else 'body'
    return {'chapter_id': chapter['id'], 'title': chapter.get('display_title') or chapter['title'], 'targets': targets, 'children': [],
            'is_volume': False, 'presentation_role': role}


def _verbatim_target(kind, purpose):
    return {'target_id': 'pending', 'kind': kind, 'purpose': purpose, 'rule_refs': [], 'source_refs': [],
            'manual_fill': True, 'support_origins': ['tender']}
