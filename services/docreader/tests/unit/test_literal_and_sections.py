"""Scanned sections, layout decoration, literal hyphen presentation, and XML export."""

from io import BytesIO

from docx import Document

from docreader.parser.continuation import mark_layout_decoration
from docreader.parser.pdf_parser import _sectionize_pages
from docreader.tender.authoring_export import _prepare_document, xml_text
from docreader.tender.evidence.native import bind_pdf_literal
from docreader.tender.evidence.pdf_literal import CONTRACT, present_literal_characters


def test_scanned_page_ends_the_open_section():
    _promoted, fragments = _sectionize_pages(
        ["第一章 招标公告\n投标人应具备相应资格。", "", "资格证明材料见附件。"],
        [False, True, False],
    )
    ordinals = [item[0] for page in fragments for item in page]
    assert ordinals[0] != ordinals[-1]
    assert "投标人应具备相应资格。" in fragments[0][0][2]
    assert "资格证明材料见附件。" in fragments[2][0][2]
    assert fragments[2][0][1] != "第一章 招标公告"


def test_layout_decoration_keeps_every_line_and_its_number():
    pages = ["页眉\n正文甲\n页脚\n", "页眉\n正文乙\n页脚\n", "页眉\n正文丙\n页脚\n", "页眉\n正文丁\n页脚\n"]
    original = [page.splitlines() for page in pages]
    flags = mark_layout_decoration(pages)
    assert [page.splitlines() for page in pages] == original
    assert [len(page_flags) for page_flags in flags] == [len(lines) for lines in original]
    assert flags[0][0] is True and flags[0][1] is False and flags[0][2] is True
    assert all(lines[1] == f"正文{name}" for lines, name in zip(original, "甲乙丙丁"))


def test_hyphen_evidence_is_presented_and_the_raw_code_point_stays():
    evidenced = present_literal_characters([
        {'text': '招', 'flags': []},
        {'text': '\ufffe', 'flags': ['hyphen_evidence']},
        {'text': '标', 'flags': []},
    ])
    assert evidenced['text'] == '招-标'
    assert 0xFFFE in evidenced['raw_code_points']
    assert evidenced['pending'] is False

    unproven = present_literal_characters([
        {'text': '招', 'flags': []},
        {'text': '\ufffe', 'flags': []},
        {'text': '标', 'flags': []},
    ])
    assert unproven['text'] == '招标'
    assert '\ufffe' not in unproven['text']
    assert unproven['pending'] is True
    assert 0xFFFE in unproven['raw_code_points']


def test_directory_body_uses_literal_presentation_without_changing_the_receipt_or_request_lines():
    from docreader.tender.directory_compile import compile_directory, reading_payload
    from test_directory_compile import item, node, plan

    def project(characters, layout):
        literal = {'contract': CONTRACT, 'source_sha256': 'a' * 64, 'page_count': 1, 'pages': [
            {'page_ordinal': 0, 'characters': characters}]}
        raw = {'original_ref': {'sha256': 'a' * 64}, 'units': [], 'metadata': {'evidence_issues': []},
               'source_contract': {'units': [], 'page_manifest': []}}
        bind_pdf_literal(raw, literal)
        from docreader.tender import directory_compile as module
        original_layout = module._layout_pages
        module._layout_pages = lambda *_args, **_kwargs: ([layout], None)
        try:
            pages, _owners, issues, _unread = module._reading_pages(None, raw)
        finally:
            module._layout_pages = original_layout
        compiled = compile_directory(plan([node('line', 0, 0)], [item('line')]), [{
            'document_id': 'tender', 'role': 'main', 'pages': pages,
            'body_projection': raw.get('body_projection') or []}])
        return pages, compiled, issues, literal, raw

    characters = [
        {'char_index': 0, 'text': '招', 'bbox': [0, 0, 10, 10], 'flags': []},
        {'char_index': 1, 'text': '\ufffe', 'bbox': [10, 0, 12, 10], 'flags': ['hyphen_evidence']},
        {'char_index': 2, 'text': '标', 'bbox': [12, 0, 22, 10], 'flags': []},
    ]
    pages, compiled, issues, literal, raw = project(characters, '招\ufffe标')
    assert pages == ['招\ufffe标']
    assert reading_payload([{'document_id': 'tender', 'role': 'main', 'pages': pages}])['documents'][0]['pages'][0]['lines'][0]['text'] == '招\ufffe标'
    assert compiled['chapters'][0]['text'] == '招-标'
    assert raw['units'][0]['text'] == '招\ufffe标'
    assert literal['pages'][0]['characters'][1]['text'] == '\ufffe'
    assert not any(issue['code'] == 'outline_directory_literal_pending' for issue in issues)

    bare = [
        {'char_index': 0, 'text': '招', 'bbox': [0, 0, 10, 10], 'flags': []},
        {'char_index': 1, 'text': '\ufffe', 'bbox': [10, 0, 12, 10], 'flags': []},
        {'char_index': 2, 'text': '标', 'bbox': [12, 0, 22, 10], 'flags': []},
    ]
    pages, compiled, issues, literal, raw = project(bare, '招\ufffe标')
    assert pages == ['招\ufffe标']
    assert compiled['chapters'][0]['text'] == '招标'
    assert '\ufffe' not in compiled['chapters'][0]['text']
    assert any(issue['code'] == 'outline_directory_literal_pending' for issue in issues)
    assert raw['units'][0]['text'] == '招\ufffe标'


def test_repeated_literal_lines_keep_a_verified_symbol_on_the_same_line():
    from docreader.tender.directory_compile import compile_directory
    from test_directory_compile import item, node, plan

    def glyph(index, text, flags):
        return {'char_index': index, 'text': text, 'bbox': [index * 10, 0, index * 10 + 8, 10], 'flags': flags}

    characters = [
        glyph(0, '\uf052', []), glyph(1, ' ', []), glyph(2, '招', []),
        glyph(3, '\ufffe', ['hyphen_evidence']), glyph(4, '标', []),
        glyph(5, '\n', ['line_break']),
        glyph(6, '\uf052', []), glyph(7, ' ', []), glyph(8, '招', []),
        glyph(9, '\ufffe', ['hyphen_evidence']), glyph(10, '标', []),
    ]
    evidence = [{'page_ordinal': 0, 'char_index': index, 'raw_symbol': '\uf052', 'normalized_symbol': '☑',
                 'font_name': 'ABCDEF+Wingdings2,Bold', 'glyph_name': 'boxcheck', 'font_sha256': 'a' * 64}
                for index in (0, 6)]
    literal = {'contract': CONTRACT, 'source_sha256': 'b' * 64, 'page_count': 1,
               'pages': [{'page_ordinal': 0, 'characters': characters}]}
    raw = {'original_ref': {'sha256': 'b' * 64}, 'units': [], 'metadata': {'evidence_issues': []},
           'source_contract': {'units': [], 'page_manifest': [], 'glyph_normalizations': evidence}}
    bind_pdf_literal(raw, literal)
    layout = '\uf052 招\ufffe标\n\uf052 招\ufffe标'
    from docreader.tender import directory_compile as module
    original_layout = module._layout_pages
    module._layout_pages = lambda *_args, **_kwargs: ([layout], None)
    try:
        pages, _owners, _issues, _unread = module._reading_pages(None, raw)
    finally:
        module._layout_pages = original_layout
    compiled = compile_directory(plan([node('line', 0, 0)], [item('line')]), [{
        'document_id': 'tender', 'role': 'main', 'pages': pages,
        'body_projection': raw.get('body_projection') or []}])
    assert pages[0] == layout
    assert compiled['chapters'][0]['text'] == '☑ 招-标\n☑ 招-标'
    assert raw['units'][0]['text'].count('\ufffe') == 1


def test_frozen_reread_pairs_duplicate_lines_by_source_position(tmp_path):
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.tender.directory_compile import compile_directory
    from test_directory_compile import item, node, plan

    def glyph(index, text, flags, y):
        return {'char_index': index, 'text': text, 'bbox': [index * 10, y, index * 10 + 8, y + 10], 'flags': flags}

    characters = [
        glyph(0, '招', [], 80), glyph(1, '\ufffe', ['hyphen_evidence'], 80), glyph(2, '标', [], 80),
        glyph(3, '\n', ['line_break'], 40),
        glyph(8, '招', [], 20), glyph(9, '\ufffe', [], 20), glyph(10, '标', [], 20),
    ]
    literal = {'contract': CONTRACT, 'source_sha256': 'c' * 64, 'page_count': 1,
               'pages': [{'page_ordinal': 0, 'characters': characters}]}
    raw = {'original_ref': {'sha256': 'c' * 64}, 'units': [], 'metadata': {'evidence_issues': []},
           'source_contract': {'units': [], 'page_manifest': []}}
    bind_pdf_literal(raw, literal)
    artifacts = ArtifactStore(tmp_path)
    loaded = artifacts.read_json(artifacts.put_json(raw))
    assert list(loaded['literal_units']) == sorted(loaded['literal_units'])
    by_top = sorted(loaded['units'], key=lambda unit: -unit['physical_locator']['top'])
    assert list(loaded['literal_units'])[0] == by_top[1]['unit_id']
    layout = '招\ufffe标\n招\ufffe标'
    from docreader.tender import directory_compile as module
    original_layout = module._layout_pages
    module._layout_pages = lambda *_args, **_kwargs: ([layout], None)
    try:
        pages, _owners, _issues, _unread = module._reading_pages(None, loaded)
    finally:
        module._layout_pages = original_layout
    compiled = compile_directory(plan([node('line', 0, 0)], [item('line')]), [{
        'document_id': 'tender', 'role': 'main', 'pages': pages,
        'body_projection': loaded.get('body_projection') or []}])
    assert pages[0] == layout
    assert compiled['chapters'][0]['text'] == '招-标\n招标'
    assert any('第1页第2行' in issue['detail'] for issue in _issues)
    assert not any('第1页第1行' in issue['detail'] and '未证实' in issue['detail'] for issue in _issues)
    assert loaded['units'][0]['text'].count('\ufffe') == 1


def test_literal_binder_keeps_raw_text_and_records_presentation():
    literal = {
        'contract': CONTRACT, 'source_sha256': 'a' * 64, 'page_count': 1,
        'pages': [{'page_ordinal': 0, 'characters': [
            {'char_index': 0, 'text': '招', 'bbox': [0, 0, 10, 10], 'flags': []},
            {'char_index': 1, 'text': '\ufffe', 'bbox': [10, 0, 12, 10], 'flags': ['hyphen_evidence']},
            {'char_index': 2, 'text': '标', 'bbox': [12, 0, 22, 10], 'flags': []},
        ]}],
    }
    raw = {'original_ref': {'sha256': 'a' * 64}, 'units': [], 'metadata': {'evidence_issues': []},
           'source_contract': {'units': [], 'page_manifest': []}}
    bind_pdf_literal(raw, literal)
    unit = next(item for item in raw['units'] if item.get('text_basis') == 'pdf_literal')
    assert unit['text'] == '招\ufffe标'
    mapping = raw['literal_units'][unit['unit_id']]
    assert mapping['presentation'] == '招-标'
    assert mapping['presentation_pending'] is False
    assert 0xFFFE in mapping['raw_code_points']
    assert literal['pages'][0]['characters'][1]['text'] == '\ufffe'


def test_omitted_mark_and_private_symbol_share_the_original_offsets(monkeypatch, tmp_path):
    """Deleting a leading U+FFFE must not move the later private-glyph edit."""
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.tender.authoring_export import export_authoring_document
    from docreader.tender.directory_compile import compile_directory, outline_from_compiled
    from test_directory_compile import item, node, plan

    def project(flags):
        characters = [
            {'char_index': 0, 'text': 'A', 'bbox': [0, 0, 10, 10], 'flags': []},
            {'char_index': 1, 'text': '\ufffe', 'bbox': [10, 0, 14, 10], 'flags': flags},
            {'char_index': 2, 'text': ' ', 'bbox': [14, 0, 18, 10], 'flags': []},
            {'char_index': 3, 'text': '\uf052', 'bbox': [40, 80, 48, 90], 'flags': []},
        ]
        literal = {'contract': CONTRACT, 'source_sha256': 'd' * 64, 'page_count': 1,
                   'pages': [{'page_ordinal': 0, 'characters': characters}]}
        raw = {'original_ref': {'sha256': 'd' * 64}, 'units': [], 'metadata': {'evidence_issues': []},
               'source_contract': {'units': [], 'page_manifest': []}}
        bind_pdf_literal(raw, literal)
        raw['source_contract']['glyph_normalizations'] = [{
            'page_ordinal': 0, 'char_index': 3, 'raw_symbol': '\uf052', 'normalized_symbol': '☑',
            'font_name': 'Wingdings2', 'glyph_name': 'boxcheck', 'font_sha256': 'a' * 64,
            'left': 42, 'bottom': 80, 'right': 48, 'top': 90, 'page_width': 200, 'page_height': 100,
        }]
        # Poppler's word boxes omit U+FFFE. The private glyph still has to land on its own box.
        words = [(0, 10, 10, 22, 'A'), (40, 10, 50, 22, '\uf052')]
        from docreader.tender import directory_compile as module
        monkeypatch.setattr(module, '_layout_pages', lambda *_args, **_kwargs: (['A\ufffe \uf052'], None))
        monkeypatch.setattr(module, '_layout_word_pages', lambda *_args, **_kwargs: [(words, 100.0)])
        pages, _owners, issues, _unread = module._reading_pages(None, raw)
        compiled = compile_directory(plan([node('mark', 0, 0)], [item('mark')]), [{
            'document_id': 'tender', 'role': 'main', 'pages': pages,
            'body_projection': raw.get('body_projection') or []}])
        return pages, compiled, issues

    pages, compiled, issues = project([])
    assert pages == ['A\ufffe \uf052']
    assert compiled['chapters'][0]['text'] == 'A ☑'
    assert any(issue['code'] == 'outline_directory_literal_pending' and '第1页第1行' in issue['detail'] for issue in issues)
    snapshot = {'schema_version': 2, 'run_id': 'literal-symbol-offset', 'source_version': 1, 'domain_revision': 1}
    outline = outline_from_compiled(snapshot, compiled)
    artifacts = ArtifactStore(tmp_path)
    result = export_authoring_document(artifacts, outline, [])
    exported = [paragraph.text for paragraph in Document(BytesIO(artifacts.read_bytes(result['docx_ref']))).paragraphs]
    assert 'A ☑' in exported

    pages, compiled, issues = project(['hyphen_evidence'])
    assert pages == ['A\ufffe \uf052']
    assert compiled['chapters'][0]['text'] == 'A- ☑'
    assert not any(issue['code'] == 'outline_directory_literal_pending' for issue in issues)


def test_unproven_mark_keeps_the_following_hyphen_and_stays_pending(monkeypatch):
    from docreader.tender.directory_compile import compile_directory
    from test_directory_compile import item, node, plan

    def project(flags, drop_pieces=False):
        characters = [
            {'char_index': 0, 'text': 'A', 'bbox': [0, 0, 8, 10], 'flags': []},
            {'char_index': 1, 'text': '\ufffe', 'bbox': [8, 0, 12, 10], 'flags': flags},
            {'char_index': 2, 'text': '-', 'bbox': [12, 0, 18, 10], 'flags': []},
            {'char_index': 3, 'text': 'B', 'bbox': [18, 0, 26, 10], 'flags': []},
        ]
        literal = {'contract': CONTRACT, 'source_sha256': 'e' * 64, 'page_count': 1,
                   'pages': [{'page_ordinal': 0, 'characters': characters}]}
        raw = {'original_ref': {'sha256': 'e' * 64}, 'units': [], 'metadata': {'evidence_issues': []},
               'source_contract': {'units': [], 'page_manifest': []}}
        bind_pdf_literal(raw, literal)
        if drop_pieces:
            for mapping in raw['literal_units'].values():
                mapping.pop('character_pieces', None)
        from docreader.tender import directory_compile as module
        monkeypatch.setattr(module, '_layout_pages', lambda *_args, **_kwargs: (['A\ufffe-B'], None))
        monkeypatch.setattr(module, '_layout_word_pages', lambda *_args, **_kwargs: [([], 100.0)])
        pages, _owners, issues, _unread = module._reading_pages(None, raw)
        compiled = compile_directory(plan([node('line', 0, 0)], [item('line')]), [{
            'document_id': 'tender', 'role': 'main', 'pages': pages,
            'body_projection': raw.get('body_projection') or []}])
        return pages, compiled, issues, raw

    pages, compiled, issues, raw = project([])
    stored = next(iter(raw['literal_units'].values()))['character_pieces']
    assert stored == ['A', '', '-', 'B']
    assert pages == ['A\ufffe-B']
    assert compiled['chapters'][0]['text'] == 'A-B'
    assert '\ufffe' not in compiled['chapters'][0]['text']
    assert any(issue['code'] == 'outline_directory_literal_pending' and '第1页第1行' in issue['detail'] for issue in issues)

    pages, compiled, issues, _raw = project([], drop_pieces=True)
    assert pages == ['A\ufffe-B']
    assert compiled['chapters'][0]['text'] == 'A-B'
    assert any(issue['code'] == 'outline_directory_literal_pending' for issue in issues)

    pages, compiled, issues, raw = project(['hyphen_evidence'])
    assert next(iter(raw['literal_units'].values()))['character_pieces'] == ['A', '-', '-', 'B']
    assert pages == ['A\ufffe-B']
    assert compiled['chapters'][0]['text'] == 'A--B'
    assert not any(issue['code'] == 'outline_directory_literal_pending' for issue in issues)

    pages, compiled, issues, raw = project(['hyphen_evidence'], drop_pieces=True)
    assert 'character_pieces' not in next(iter(raw['literal_units'].values()))
    assert next(iter(raw['literal_units'].values()))['presentation'] == 'A--B'
    assert pages == ['A\ufffe-B']
    assert compiled['chapters'][0]['text'] == 'A--B'
    assert '\ufffe' not in compiled['chapters'][0]['text']
    assert not any(issue['code'] == 'outline_directory_literal_pending' for issue in issues)


def test_long_reread_without_character_pieces_keeps_both_hyphens(monkeypatch):
    from docreader.tender.directory_compile import compile_directory
    from test_directory_compile import item, node, plan

    prefix = '甲' * 1049
    raw_line = prefix + 'A\ufffe-B'
    assert len(raw_line) == 1053
    characters = [{'char_index': index, 'text': text, 'bbox': [index, 0, index + 1, 10],
                   'flags': ['hyphen_evidence'] if text == '\ufffe' else []}
                  for index, text in enumerate(raw_line)]
    literal = {'contract': CONTRACT, 'source_sha256': 'f' * 64, 'page_count': 1,
               'pages': [{'page_ordinal': 0, 'characters': characters}]}
    raw = {'original_ref': {'sha256': 'f' * 64}, 'units': [], 'metadata': {'evidence_issues': []},
           'source_contract': {'units': [], 'page_manifest': []}}
    bind_pdf_literal(raw, literal)
    mapping = next(iter(raw['literal_units'].values()))
    assert mapping['presentation'] == prefix + 'A--B'
    mapping.pop('character_pieces')
    from docreader.tender import directory_compile as module
    monkeypatch.setattr(module, '_layout_pages', lambda *_args, **_kwargs: ([raw_line], None))
    monkeypatch.setattr(module, '_layout_word_pages', lambda *_args, **_kwargs: [([], 100.0)])
    pages, _owners, issues, _unread = module._reading_pages(None, raw)
    compiled = compile_directory(plan([node('line', 0, 0)], [item('line')]), [{
        'document_id': 'tender', 'role': 'main', 'pages': pages,
        'body_projection': raw.get('body_projection') or []}])
    assert pages == [raw_line]
    assert compiled['chapters'][0]['text'] == prefix + 'A--B'
    assert '\ufffe' not in compiled['chapters'][0]['text']
    assert not any(issue['code'] == 'outline_directory_literal_pending' for issue in issues)


def test_indented_short_line_is_not_a_substring_of_the_longer_line(monkeypatch):
    from docreader.tender.directory_compile import compile_directory
    from test_directory_compile import item, node, plan

    def line_characters(text, y):
        return [{'char_index': index, 'text': char, 'bbox': [index * 10, y, index * 10 + 8, y + 10],
                 'flags': ['hyphen_evidence'] if char == '\ufffe' else []}
                for index, char in enumerate(text)]

    for drop_pieces, boxes in ((False, False), (False, True), (True, False), (True, True)):
        characters = [*line_characters('A\ufffe-B', 80),
                      {'char_index': 100, 'text': '\n', 'bbox': [0, 40, 1, 41], 'flags': ['line_break']},
                      *line_characters('XA\ufffe-B', 20)]
        for index, character in enumerate(characters):
            character['char_index'] = index
        literal = {'contract': CONTRACT, 'source_sha256': '8' * 64, 'page_count': 1,
                   'pages': [{'page_ordinal': 0, 'characters': characters}]}
        raw = {'original_ref': {'sha256': '8' * 64}, 'units': [], 'metadata': {'evidence_issues': []},
               'source_contract': {'units': [], 'page_manifest': []}}
        bind_pdf_literal(raw, literal)
        if drop_pieces:
            for mapping in raw['literal_units'].values():
                mapping.pop('character_pieces', None)
        layout = '    A\ufffe-B\nXA\ufffe-B'
        from docreader.tender import directory_compile as module
        monkeypatch.setattr(module, '_layout_pages', lambda *_args, **_kwargs: ([layout], None))
        monkeypatch.setattr(module, '_layout_word_pages',
                            lambda *_args, **_kwargs: [([], 100.0)] if boxes else None)
        pages, _owners, issues, _unread = module._reading_pages(None, raw)
        compiled = compile_directory(plan([node('mark', 0, 0)], [item('mark')]), [{
            'document_id': 'tender', 'role': 'main', 'pages': pages,
            'body_projection': raw.get('body_projection') or []}])
        assert pages == [layout]
        assert compiled['chapters'][0]['text'] == '    A--B\nXA--B'
        assert not any(issue['code'] == 'outline_directory_literal_pending' for issue in issues)


def test_source_leading_spaces_still_match_a_whole_indented_line(monkeypatch):
    from docreader.tender.directory_compile import compile_directory
    from test_directory_compile import item, node, plan

    def line_characters(text, y):
        return [{'char_index': index, 'text': char, 'bbox': [index * 10, y, index * 10 + 8, y + 10],
                 'flags': ['hyphen_evidence'] if char == '\ufffe' else []}
                for index, char in enumerate(text)]

    for drop_pieces, boxes in ((False, True), (True, True)):
        characters = [*line_characters('  A\ufffe-B', 80),
                      {'char_index': 100, 'text': '\n', 'bbox': [0, 40, 1, 41], 'flags': ['line_break']},
                      *line_characters('Y  A\ufffe-B', 20)]
        for index, character in enumerate(characters):
            character['char_index'] = index
        literal = {'contract': CONTRACT, 'source_sha256': '7' * 64, 'page_count': 1,
                   'pages': [{'page_ordinal': 0, 'characters': characters}]}
        raw = {'original_ref': {'sha256': '7' * 64}, 'units': [], 'metadata': {'evidence_issues': []},
               'source_contract': {'units': [], 'page_manifest': []}}
        bind_pdf_literal(raw, literal)
        if drop_pieces:
            for mapping in raw['literal_units'].values():
                mapping.pop('character_pieces', None)
        layout = '    A\ufffe-B\nY  A\ufffe-B'
        from docreader.tender import directory_compile as module
        monkeypatch.setattr(module, '_layout_pages', lambda *_args, **_kwargs: ([layout], None))
        monkeypatch.setattr(module, '_layout_word_pages', lambda *_args, **_kwargs: [([], 100.0)])
        pages, _owners, issues, _unread = module._reading_pages(None, raw)
        compiled = compile_directory(plan([node('mark', 0, 0)], [item('mark')]), [{
            'document_id': 'tender', 'role': 'main', 'pages': pages,
            'body_projection': raw.get('body_projection') or []}])
        assert pages == [layout]
        assert compiled['chapters'][0]['text'] == '    A--B\nY  A--B'
        assert not any(issue['code'] == 'outline_directory_literal_pending' for issue in issues)


def test_a_shorter_marked_line_is_not_treated_as_a_duplicate_of_a_longer_line(monkeypatch):
    from docreader.tender.directory_compile import compile_directory
    from test_directory_compile import item, node, plan

    def line_characters(text, y):
        return [{'char_index': index, 'text': char, 'bbox': [index * 10, y, index * 10 + 8, y + 10],
                 'flags': ['hyphen_evidence'] if char == '\ufffe' else []}
                for index, char in enumerate(text)]

    cases = [(False, False), (False, True), (True, False), (True, True)]
    for drop_pieces, boxes in cases:
        characters = [*line_characters('A\ufffe-B', 80),
                      {'char_index': 100, 'text': '\n', 'bbox': [0, 40, 1, 41], 'flags': ['line_break']},
                      *line_characters('XA\ufffe-B', 20)]
        for index, character in enumerate(characters):
            character['char_index'] = index
        literal = {'contract': CONTRACT, 'source_sha256': '9' * 64, 'page_count': 1,
                   'pages': [{'page_ordinal': 0, 'characters': characters}]}
        raw = {'original_ref': {'sha256': '9' * 64}, 'units': [], 'metadata': {'evidence_issues': []},
               'source_contract': {'units': [], 'page_manifest': []}}
        bind_pdf_literal(raw, literal)
        if drop_pieces:
            for mapping in raw['literal_units'].values():
                mapping.pop('character_pieces', None)
        layout = 'A\ufffe-B\nXA\ufffe-B'
        from docreader.tender import directory_compile as module
        monkeypatch.setattr(module, '_layout_pages', lambda *_args, **_kwargs: ([layout], None))
        monkeypatch.setattr(module, '_layout_word_pages',
                            lambda *_args, **_kwargs: [([], 100.0)] if boxes else None)
        pages, _owners, issues, _unread = module._reading_pages(None, raw)
        compiled = compile_directory(plan([node('line', 0, 0)], [item('line')]), [{
            'document_id': 'tender', 'role': 'main', 'pages': pages,
            'body_projection': raw.get('body_projection') or []}])
        assert pages == [layout]
        assert compiled['chapters'][0]['text'] == 'A--B\nXA--B'
        assert not any(issue['code'] == 'outline_directory_literal_pending' for issue in issues)


def test_export_drops_xml_illegal_characters():
    assert xml_text('甲\ufffe乙\x00丙') == '甲乙丙'
    document = _prepare_document()
    document.add_paragraph('甲\ufffe乙')
    output = BytesIO()
    document.save(output)
    text = '\n'.join(paragraph.text for paragraph in Document(output).paragraphs)
    assert text == '甲乙'
    assert '\ufffe' not in text
