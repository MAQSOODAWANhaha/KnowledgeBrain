"""Unique same-page native prose candidates; locating text never selects its duty."""
from collections import defaultdict

import hashlib

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.evidence.references import fragment, selector


def _glyphs(text):
    return ''.join(c for c in text if not c.isspace())


def _locations(text, needle):
    result, start = [], 0
    while needle and (start := text.find(needle, start)) >= 0:
        result.append(start)
        start += 1
    return result


def _page(value):
    page = (value.get('physical_locator') or {}).get('page_ordinal')
    return page if type(page) is int else (value.get('locator') or {}).get('page_ordinal')


class NativeProseSources:
    def __init__(self, repository, frozen, documents, applications):
        self.documents, self.artifacts = documents, repository.artifacts
        self.applications = [a for a in applications if a.get('kind') == 'source_decoration']
        self._fact_pages, self._original_bytes, self._original_pages = {}, {}, {}
        self._original_hashes, self._producer_version = {}, None
        self.prose, self.grids = defaultdict(list), defaultdict(list)
        for source in frozen:
            index = repository.source_index(source['source_index_ref'])
            units = {u['unit_id']: u for u in index.raw['units']}
            for base in index.ranges:
                unit, page = units[base['unit_id']], _page(base)
                text = _glyphs(base['text'])
                if type(page) is not int or page < 0 or not text:
                    continue
                point = (source['document_id'], page)
                if unit.get('grid') or base.get('text_basis') in ('grid_row', 'spreadsheet_row'):
                    self.grids[point].append(text)
                elif unit['kind'] != 'image_region' and base.get('text_basis') not in ('literal_ocr', 'pdf_literal'):
                    self.prose[point].append((source, index, base, text))

    def match(self, purpose, active_pages):
        if len(active_pages) != 1:
            return None
        point = next(iter(active_pages))
        wanted = _glyphs(purpose)
        if not wanted or any(_locations(text, wanted) for text in self.grids[point]):
            return None
        page = self.documents[point[0]]['pages'][point[1]]
        glyph_page = _glyphs(page)
        if len(_locations(glyph_page, wanted)) != 1:
            return None
        hits = [(source, index, base, text, position) for source, index, base, text in self.prose[point]
                for position in _locations(text, wanted)]
        if len(hits) != 1:
            return None
        source, index, base, text, position = hits[0]
        offsets = [i for i, c in enumerate(base['text']) if not c.isspace()]
        anchor = (base['start'] + offsets[position], base['start'] + offsets[position + len(wanted) - 1] + 1)
        syntax = self._syntax_range(source, index, base, point)
        if syntax is None:
            return None
        base, generated = syntax
        if not base['start'] <= anchor[0] < anchor[1] <= base['end']:
            return None
        protected = self._protect(source, base, point, anchor)
        if protected is None:
            return None
        base, excluded = protected
        boundaries = base.get('text_boundaries') or {}
        return {'source': source, 'index': index, 'base': base, 'page': point[1],
            'match_start': anchor[0], 'match_end': anchor[1], 'excluded_decorations': excluded,
            'generated_syntax': generated,
            'full_range_exact': base['text'] == purpose and boundaries.get('start') is True and boundaries.get('end') is True,
            'source_excerpt': {'source_ref': {'source_index_ref': source['source_index_ref'], 'unit_id': base['unit_id']},
                               'selectors': [selector(base)]}}

    def _syntax_range(self, source, index, base, point):
        if '#' not in base['text']:
            return base, []
        from docreader.parser.pdf_parser import _atx_heading
        from docreader.parser.source_contract import parser_version
        if self._producer_version is None:
            self._producer_version = parser_version()
        contract = index.raw.get('source_contract') or {}
        unit = next((u for u in index.raw['units'] if u['unit_id'] == base['unit_id']), None)
        identity = next((u for u in contract.get('units', []) if u['unit_id'] == base['unit_id']), None)
        if (index.raw.get('schema_version') != 2 or contract.get('schema_version') != 2
                or contract.get('parser_version') != self._producer_version or unit is None or identity is None
                or unit['kind'] != 'section' or identity.get('kind') != 'section'
                or base['start'] != 0 or base['range_id'] != base['source_range_id'] or base['text'] != unit['text']
                or identity.get('text_sha256') != hashlib.sha256(unit['text'].encode()).hexdigest()):
            return None
        first = base['text'].split('\n', 1)[0]
        heading = _atx_heading(first)
        if heading is None:
            return None
        level, title = heading
        prefix = '#' * level + ' '
        path = (unit.get('locator') or {}).get('heading_path')
        if (not 1 <= level <= 6 or first != prefix + title or '#' in base['text'][len(prefix):]
                or not path or identity.get('heading_path') != path or path.split(' > ')[-1] != title):
            return None
        stack, owners = [index.raw.get('tree')], []
        while stack:
            node = stack.pop()
            if not isinstance(node, dict):
                continue
            if base['unit_id'] in node.get('keys', []):
                owners.append(node)
            stack.extend(node.get('children', []))
        if len(owners) != 1 or owners[0].get('title') != title or owners[0].get('heading_path') != path:
            return None
        original = self._original_stream(source, point[1])
        key = (source['original_ref']['sha256'], point[1])
        if (original is None or self._original_hashes.get(key) is not False
                or len(_locations(_glyphs(original), _glyphs(title))) != 1):
            return None
        suffix = base['text'][len(prefix):]
        layout = self.documents[point[0]]['pages'][point[1]]
        views = [name for name, text in [('frozen_layout', layout), ('pdf_original_chars', original)]
                 if len(_locations(_glyphs(text), _glyphs(suffix))) == 1]
        if not views:
            return None
        projected = fragment(base, base['start'] + len(prefix), base['end'])
        proof = {'kind': 'pdf_atx_heading_prefix', 'source_range_id': base['source_range_id'],
                 'native_start': base['start'], 'native_end': projected['start'], 'title': title,
                 'text_sha256': hashlib.sha256(prefix.encode()).hexdigest(),
                 'producer_version': self._producer_version, 'literal_hash_glyphs_on_pdf_page': 0,
                 'unique_suffix_views': views}
        return projected, [proof]

    def _facts(self, source, point):
        key = (source['original_ref']['sha256'], *point)
        if key in self._fact_pages:
            return self._fact_pages[key]
        page = self.documents[point[0]]['pages'][point[1]]
        lines = page.splitlines(keepends=True)
        records = []
        for app in self.applications:
            if app.get('original_ref') != source['original_ref']:
                continue
            for item in app.get('original_fragments', []):
                if item['page'] != point[1]:
                    continue
                line = item['line']
                if type(line) is not int or not 0 <= line < len(lines):
                    raise ArtifactIntegrityError('global decoration line is outside its frozen page')
                text = page.splitlines()[line]
                low, high = item['start'], item['end']
                if (type(low) is not int or type(high) is not int or not 0 <= low < high <= len(text)
                        or item['line_text'] != text or hashlib.sha256(text[low:high].encode()).hexdigest() != item['text_sha256']):
                    raise ArtifactIntegrityError('global decoration characters do not match their exact frozen line')
                offset = sum(map(len, lines[:line]))
                records.append({'fact_id': app['fact_id'], 'state': app.get('state'), 'fragment': item,
                                'layout_interval': (offset + low, offset + high)})
        self._fact_pages[key] = records
        return records

    def _original_stream(self, source, page):
        digest = source['original_ref']['sha256']
        key = (digest, page)
        if key in self._original_pages:
            return self._original_pages[key]
        if digest not in self._original_bytes:
            self._original_bytes[digest] = self.artifacts.read_bytes(source['original_ref'])
        data = self._original_bytes[digest]
        if not data.startswith(b'%PDF'):
            self._original_pages[key] = None
            return None
        from docreader.parser.pdf_parser import _PDFIUM_LOCK
        import pypdfium2 as pdfium
        import pypdfium2.raw as pdfraw
        with _PDFIUM_LOCK, pdfium.PdfDocument(data) as pdf:
            if not 0 <= page < len(pdf):
                raise ArtifactIntegrityError('global decoration page is outside its original PDF')
            handle = pdf[page]
            textpage = handle.get_textpage()
            try:
                value = textpage.get_text_range()
                self._original_hashes[key] = ('#' in value or any(
                    pdfraw.FPDFText_GetUnicode(textpage.raw, i) == 35 for i in range(textpage.count_chars())))
            finally:
                textpage.close()
                handle.close()
        self._original_pages[key] = value
        return value

    def _overlaps(self, base, stream, intervals, mapping):
        glyph_stream, native = _glyphs(stream), _glyphs(base['text'])
        positions = _locations(glyph_stream, native)
        if len(positions) != 1:
            return None
        begin = positions[0]
        offsets = [i for i, c in enumerate(base['text']) if not c.isspace()]
        result = []
        for fact, low, high in intervals:
            first = max(begin, len(_glyphs(stream[:low]))) - begin
            last = min(begin + len(native), len(_glyphs(stream[:high]))) - begin
            if first < last:
                result.append({'fact_id': fact['fact_id'], 'state': fact['state'],
                    'source_range_id': base['source_range_id'], 'native_start': base['start'] + offsets[first],
                    'native_end': base['start'] + offsets[last - 1] + 1, 'mapping': mapping})
        return result

    def _protect(self, source, base, point, anchor):
        facts = self._facts(source, point)
        if not facts:
            return base, []
        layout = self.documents[point[0]]['pages'][point[1]]
        overlaps = self._overlaps(base, layout, [(f, *f['layout_interval']) for f in facts], 'frozen_layout')
        if overlaps is None:
            original = self._original_stream(source, point[1])
            if original is None:
                return None
            intervals = []
            for fact in facts:
                chars = fact['fragment'].get('original_chars')
                if chars is None:
                    return None
                low, high = chars['start'], chars['end']
                if (type(low) is not int or type(high) is not int or not 0 <= low < high <= len(original)
                        or hashlib.sha256(original[low:high].encode()).hexdigest() != chars['text_sha256']
                        or _glyphs(original[low:high]) != _glyphs(fact['fragment']['line_text'][
                            fact['fragment']['start']:fact['fragment']['end']])):
                    raise ArtifactIntegrityError('global decoration original characters failed their exact PDF binding')
                intervals.append((fact, low, high))
            overlaps = self._overlaps(base, original, intervals, 'pdf_original_chars')
            if overlaps is None:
                return None
        if any(f['state'] not in ('presentation_excluded', 'not_in_formal_body')
               or f['native_start'] < anchor[1] and anchor[0] < f['native_end'] for f in overlaps):
            return None
        bounds = []
        for first, last in sorted((f['native_start'], f['native_end']) for f in overlaps):
            if bounds and first <= bounds[-1][1]:
                bounds[-1] = (bounds[-1][0], max(last, bounds[-1][1]))
            else:
                bounds.append((first, last))
        first, last = base['start'], base['end']
        for low, high in bounds:
            if not _glyphs(base['text'][first - base['start']:low - base['start']]):
                first = high
            else:
                break
        for low, high in reversed(bounds):
            if high <= first:
                continue
            if not _glyphs(base['text'][high - base['start']:last - base['start']]):
                last = low
            else:
                break
        if (not first <= anchor[0] < anchor[1] <= last
                or any(low < last and first < high for low, high in bounds)):
            return None
        selected = fragment(base, first, last) if (first, last) != (base['start'], base['end']) else base
        return selected, overlaps
